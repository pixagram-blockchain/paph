//! XRoute (PAPH-X §6, §7, §14.1) — the pair-screening signature.
//!
//! A route is 128 bytes derived from a prepared side's Tier-1 and Tier-2
//! data — never from the image — plus 8 bytes of counts and flags.  It is a
//! retrieval sketch, not a digest: two routes that agree say "worth looking
//! at", never "the same work".  Three independent readings:
//!
//!   local    64 lanes of 8-bit MinHash over the canonical Tier-1 local codes
//!            (which are already invariant under the eight symmetries of the
//!            square and the complement);
//!   band     32 lanes of 8-bit MinHash over the 24-bit descriptor bands of
//!            every keypoint AND its mirrored descriptor — the union is the
//!            same set for a work and its mirror image, so the family is
//!            mirror-invariant by construction;
//!   global   four 64-bit words of coarse structure built to be invariant
//!            under D4 and inversion: DCT energy layout, run-length classes,
//!            luminance-adjacency topology, region shape classes.  They are
//!            invariant only as far as the Tier-1 sections they read are
//!            equivariant: the DCT section is not, on canvases whose sides
//!            are not multiples of 16 (the 16 x 16 thumbnail's cells do not
//!            commute with a flip) and under quarter turns (integer DCT
//!            rounding), so G0 can differ on a mirrored or rotated copy.
//!            Route derivation 2 (profile X2) removes the two defects that
//!            were the route's own: G1's diagonal run histogram (a mirror
//!            sends the main diagonal to the anti-diagonal, which the wire
//!            does not hold) and G3's region order (area ties broken by
//!            position).  G3 still moves where the shapes section's own
//!            grid (about ceil(long side / 128) px a cell, edges rounded
//!            down from the top-left corner) does not commute with the
//!            symmetry, on canvases longer than 128 px, and where a tie in
//!            area decides which region is kept eighth.
//!
//! The readings are never collapsed into one number (§6.5): the screen is a
//! rule over the three, and `DEFER` exists so that a cheap screen cannot
//! become a recall gate.
//!
//! For search the routes of many candidates sit column-major (`RouteSoA`) and
//! one query is compared against sixteen candidates per vector instruction
//! (§7, §16.3).  The vector kernels have scalar twins a test holds equal.

use super::profile::{mix64, Seq, XProfile, BAND_LANES, GLOBAL_WORDS, LOCAL_LANES, ROUTE_DERIVATION_2};
use crate::prepared::Prepared;
use crate::wire::{F_FLAT, Tier1};

/// bytes of the packed route: 4 x u64 + 64 + 32
pub const ROUTE_BYTES: usize = 8 * GLOBAL_WORDS + LOCAL_LANES + BAND_LANES;
/// the route plus its 8-byte metadata, as a sidecar stores it
pub const ROUTE_RECORD_BYTES: usize = ROUTE_BYTES + 8;

pub const RF_LOCAL: u16 = 1;
pub const RF_BAND: u16 = 2;
pub const RF_GLOBAL: u16 = 4;
pub const RF_TIER2: u16 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XRoute {
    pub global: [u64; GLOBAL_WORDS],
    pub local_mh: [u8; LOCAL_LANES],
    pub band_mh: [u8; BAND_LANES],
    pub local_count: u16,
    pub desc_count: u16,
    /// 0: no keypoints, 1: Tier-1 sketch only, 2: Tier 2
    pub geometry_class: u16,
    /// RF_* bits — which families are measurable on this side
    pub flags: u16,
}

/// The salts of the two MinHash families, derived once from the profile.
pub struct RouteSalts {
    pub local: [u64; LOCAL_LANES],
    pub band: [u64; BAND_LANES],
}

impl RouteSalts {
    pub fn new(p: &XProfile) -> RouteSalts {
        let mut s = Seq(p.route_seed);
        let mut local = [0u64; LOCAL_LANES];
        for v in local.iter_mut() {
            *v = s.next();
        }
        let mut t = Seq(p.route_seed ^ 0x6261_6e64_5f6d_6873); // "band_mhs"
        let mut band = [0u64; BAND_LANES];
        for v in band.iter_mut() {
            *v = t.next();
        }
        RouteSalts { local, band }
    }
}

/// b-bit MinHash: the low 8 bits of the minimum hash over the elements.
///
/// One `mix64` per element, then one lane hash per salt: the mixed element
/// xor the salt, multiplied by an odd constant and byte-rotated — a
/// bijection of the mixed value per lane, so the minimum is taken over a
/// well-spread 64-bit value and the lanes are independent of one another
/// for every element set the local codes and descriptor bands produce.
#[inline(always)]
fn minhash_into(elems: &[u64], salts: &[u64], out: &mut [u8]) {
    const K: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut mins = [u64::MAX; 64];
    let lanes = salts.len().min(64);
    for &e in elems {
        let h = mix64(e);
        for l in 0..lanes {
            let v = (h ^ salts[l]).wrapping_mul(K).rotate_left(23);
            if v < mins[l] {
                mins[l] = v;
            }
        }
    }
    for l in 0..lanes {
        out[l] = (mins[l] & 0xff) as u8;
    }
}

// ------------------------------------------------------------ global words

/// G0 — DCT energy layout.  The Tier-1 L0 block stores a sign and a
/// magnitude bit per coefficient of the 16x16 thumbnail DCT.  Flips negate
/// signs and transposes swap (u, v), inversion negates every sign: the
/// magnitude bit symmetrised over (u, v) and (v, u) is invariant under all
/// sixteen.  The 64 lowest-frequency symmetric cells, by (u + v, u).
///
/// Invariant, that is, under the symmetries of the stored block — which is
/// itself not the symmetric image of the original's when the copy's
/// thumbnail cells fall elsewhere: a flip moves the cell edges ⌊i·w/16⌋ by
/// a pixel unless 16 divides the side, and a quarter turn swaps the two
/// rounded passes of the integer DCT.  Magnitude bits near the block's
/// median then flip (2.4–13.7 % per low-frequency bit on the PAPH-SI
/// corpus' mirrored and rotated copies, `sibench route`), so a word read
/// from this section is invariant only where those bits are.  The route
/// reads G0 as an overlap, which tolerates that; an exact word needs
/// symmetric sampling in the hasher.
fn g0_dct(t: &Tier1) -> u64 {
    if t.flags & F_FLAT != 0 {
        return 0;
    }
    let d = t.sec("dct");
    // 2-bit codes, 256 of them, MSB first: coefficient i is bits 2i..2i+1
    let mag = |u: usize, v: usize| -> u64 {
        let i = v * 16 + u;
        let bit = 2 * i + 1; // sign is the high bit of the pair, magnitude the low
        ((d[bit >> 3] >> (7 - (bit & 7))) & 1) as u64
    };
    let mut out = 0u64;
    let mut k = 0usize;
    let mut s = 1usize;
    while k < 64 {
        let mut u = 0usize;
        while u * 2 <= s && k < 64 {
            let v = s - u;
            if v < 16 {
                let m = mag(u, v) | mag(v, u);
                out |= m << k;
                k += 1;
            }
            u += 1;
        }
        s += 1;
    }
    out
}

/// G1 — run-length classes.  Horizontal and vertical swap under a transpose
/// and are averaged; the diagonal stays.  Two bits per bin.
fn g1_runs(t: &Tier1) -> u64 {
    let r = t.sec("runs");
    let q = |v: i64| -> u64 {
        if v == 0 {
            0
        } else if v < 16 {
            1
        } else if v < 64 {
            2
        } else {
            3
        }
    };
    let mut out = 0u64;
    for i in 0..16 {
        let hv = (r[i] as i64 + r[16 + i] as i64) / 2;
        out |= q(hv) << (2 * i);
        out |= q(r[32 + i] as i64) << (32 + 2 * i);
    }
    out
}

/// G1, route derivation 2 — run-length classes, D4-exact.  The low half
/// as G1 (horizontal and vertical averaged); the high half, where G1 held
/// the main-diagonal histogram, holds |horizontal − vertical| per bin: a
/// transpose exchanges the two and a flip keeps each, so the anisotropy is
/// invariant under all eight where the diagonal was not.
fn g1_runs_v2(t: &Tier1) -> u64 {
    let r = t.sec("runs");
    let q = |v: i64| -> u64 {
        if v == 0 {
            0
        } else if v < 16 {
            1
        } else if v < 64 {
            2
        } else {
            3
        }
    };
    let mut out = 0u64;
    for i in 0..16 {
        let (h, v) = (r[i] as i64, r[16 + i] as i64);
        out |= q((h + v) / 2) << (2 * i);
        out |= q((h - v).abs()) << (32 + 2 * i);
    }
    out
}

/// G2 — luminance-adjacency topology.  Each RAG entry is a pair of quantile
/// bytes; inversion maps (qa, qb) to (255-qb, 255-qa), under which the gap
/// |qa-qb| and the level min(qa, 255-qb) are both invariant.  The 24 most
/// frequent adjacencies mark cells of an 8x8 (gap, level) grid.
fn g2_rag(t: &Tier1) -> u64 {
    let s = t.sec("rag");
    let n = t.count("rag").min(48);
    let mut idx: [(i64, usize); 48] = [(0, 0); 48];
    for i in 0..n {
        let o = i * 6;
        idx[i] = (u16::from_le_bytes([s[o + 4], s[o + 5]]) as i64, i);
    }
    let list = &mut idx[..n];
    list.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut out = 0u64;
    for &(_, i) in list.iter().take(24) {
        let o = i * 6;
        let (qa, qb) = (s[o] as i64, s[o + 1] as i64);
        let (lo, hi) = (qa.min(qb), qa.max(qb));
        let gap = hi - lo;
        let level = lo.min(255 - hi);
        out |= 1u64 << (((gap >> 5) * 8 + (level >> 5)) as u32);
    }
    out
}

/// G3 — region shape classes.  Per stored region (largest first): the
/// isoperimetric class, the bounding-box aspect class symmetrised over a
/// transpose, and the hole count — eight bits per region, eight regions.
fn g3_shapes(t: &Tier1) -> u64 {
    let mut out = 0u64;
    for (i, c) in region_codes(t).iter().enumerate() {
        out |= (*c as u64) << (8 * i);
    }
    out
}

/// G3, route derivation 2 — the same eight-bit region codes as a sorted
/// list, largest code first.  The shapes section orders regions by area and
/// breaks ties by position, which a flip or a quarter turn moves; the
/// multiset of codes does not care.
fn g3_shapes_v2(t: &Tier1) -> u64 {
    let mut c = region_codes(t);
    c.sort_unstable_by(|a, b| b.cmp(a));
    let mut out = 0u64;
    for (i, v) in c.iter().enumerate() {
        out |= (*v as u64) << (8 * i);
    }
    out
}

/// The region codes of the stored regions, in stored order.
fn region_codes(t: &Tier1) -> Vec<u8> {
    let s = t.sec("shapes");
    let n = t.count("shapes").min(8);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let o = i * 41;
        let area = u32::from_le_bytes([s[o], s[o + 1], s[o + 2], s[o + 3]]) as i64;
        let per = u16::from_le_bytes([s[o + 4], s[o + 5]]) as i64;
        let aspect = u16::from_le_bytes([s[o + 6], s[o + 7]]) as i64;
        let holes = s[o + 8] as u64;
        let iso = if area > 0 { per * per * 256 / area } else { 0 };
        let ic = [512i64, 1024, 2048, 4096, 8192, 16384, 32768].iter().filter(|&&t| iso >= t).count() as u64;
        let asym = if aspect > 0 { aspect.max(65536 / aspect) } else { 256 };
        let ac = [320i64, 410, 512, 768, 1280, 2048, 4096].iter().filter(|&&t| asym >= t).count() as u64;
        let hc = holes.min(3);
        out.push((ic | (ac << 3) | (hc << 6)) as u8);
    }
    out
}

impl XRoute {
    /// The route of a prepared side under the profile's salts.
    pub fn build(p: &Prepared, salts: &RouteSalts, xp: &XProfile) -> XRoute {
        let mut r = XRoute {
            global: [0; GLOBAL_WORDS],
            local_mh: [0; LOCAL_LANES],
            band_mh: [0; BAND_LANES],
            local_count: p.bag.n as u16,
            desc_count: p.kp.len() as u16,
            geometry_class: if p.kp.is_empty() { 0 } else if p.tier2 { 2 } else { 1 },
            flags: 0,
        };
        if p.tier2 {
            r.flags |= RF_TIER2;
        }
        // local family: distinct canonical codes
        let n = p.bag.n;
        let mut codes = [0u64; 128];
        for i in 0..n.min(128) {
            codes[i] = ((p.bag.hi[i] as u64) << 32) | p.bag.lo[i] as u64;
        }
        let c = &mut codes[..n.min(128)];
        c.sort_unstable();
        let mut m = 0usize;
        for i in 0..c.len() {
            if i == 0 || c[i] != c[i - 1] {
                c[m] = c[i];
                m += 1;
            }
        }
        if m as i32 >= xp.route_min_codes && m > 0 {
            minhash_into(&c[..m], &salts.local, &mut r.local_mh);
            r.flags |= RF_LOCAL;
        } else {
            r.local_mh = [0xff; LOCAL_LANES];
        }
        // band family: the ten 24-bit bands of every descriptor, direct and
        // mirrored, constant bands dropped — the derivation of
        // `Engine.indexKeys` (KEYS_VERSION 1), on both halves' orders
        let nk = p.kp.len();
        if nk as i32 >= xp.route_min_kp && nk > 0 {
            let mut elems: Vec<u64> = Vec::with_capacity(nk * 20);
            for k in p.kp.iter() {
                let mut bytes = [0u8; 32];
                for j in 0..8 {
                    bytes[4 * j..4 * j + 4].copy_from_slice(&k.desc[j].to_le_bytes());
                }
                let mut mir = [0u8; 32];
                mir[..16].copy_from_slice(&bytes[16..]);
                mir[16..].copy_from_slice(&bytes[..16]);
                for b in [&bytes, &mir] {
                    for j in 0..10 {
                        let v = ((b[3 * j] as u64) << 16) | ((b[3 * j + 1] as u64) << 8) | b[3 * j + 2] as u64;
                        if v != 0 && v != 0xff_ffff {
                            elems.push(((j as u64) << 24) | v);
                        }
                    }
                }
            }
            elems.sort_unstable();
            elems.dedup();
            if !elems.is_empty() {
                minhash_into(&elems, &salts.band, &mut r.band_mh);
                r.flags |= RF_BAND;
            } else {
                r.band_mh = [0xff; BAND_LANES];
            }
        } else {
            r.band_mh = [0xff; BAND_LANES];
        }
        // global words, as the profile's route derivation reads them
        let t = &p.t1;
        r.global = if xp.route_derivation >= ROUTE_DERIVATION_2 {
            [g0_dct(t), g1_runs_v2(t), g2_rag(t), g3_shapes_v2(t)]
        } else {
            [g0_dct(t), g1_runs(t), g2_rag(t), g3_shapes(t)]
        };
        if t.flags & F_FLAT == 0 {
            r.flags |= RF_GLOBAL;
        }
        r
    }

    /// The 136-byte record: 128 route bytes then the metadata.
    pub fn to_bytes(&self) -> [u8; ROUTE_RECORD_BYTES] {
        let mut b = [0u8; ROUTE_RECORD_BYTES];
        for (i, w) in self.global.iter().enumerate() {
            b[8 * i..8 * i + 8].copy_from_slice(&w.to_le_bytes());
        }
        b[32..32 + LOCAL_LANES].copy_from_slice(&self.local_mh);
        b[96..96 + BAND_LANES].copy_from_slice(&self.band_mh);
        b[128..130].copy_from_slice(&self.local_count.to_le_bytes());
        b[130..132].copy_from_slice(&self.desc_count.to_le_bytes());
        b[132..134].copy_from_slice(&self.geometry_class.to_le_bytes());
        b[134..136].copy_from_slice(&self.flags.to_le_bytes());
        b
    }

    pub fn from_bytes(b: &[u8]) -> Option<XRoute> {
        if b.len() < ROUTE_RECORD_BYTES {
            return None;
        }
        let mut r = XRoute {
            global: [0; GLOBAL_WORDS],
            local_mh: [0; LOCAL_LANES],
            band_mh: [0; BAND_LANES],
            local_count: u16::from_le_bytes([b[128], b[129]]),
            desc_count: u16::from_le_bytes([b[130], b[131]]),
            geometry_class: u16::from_le_bytes([b[132], b[133]]),
            flags: u16::from_le_bytes([b[134], b[135]]),
        };
        for i in 0..GLOBAL_WORDS {
            let mut w = [0u8; 8];
            w.copy_from_slice(&b[8 * i..8 * i + 8]);
            r.global[i] = u64::from_le_bytes(w);
        }
        r.local_mh.copy_from_slice(&b[32..32 + LOCAL_LANES]);
        r.band_mh.copy_from_slice(&b[96..96 + BAND_LANES]);
        Some(r)
    }
}

// -------------------------------------------------------------- similarity

/// The three readings of a pair of routes (§6.5), each with its own
/// measurability: equal local lanes of 64, equal band lanes of 32, agreeing
/// global bits of 256.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RouteScore {
    pub local: i32,
    pub band: i32,
    pub global: i32,
    /// RF_LOCAL | RF_BAND | RF_GLOBAL for the families both sides measure
    pub measurable: u16,
}

/// The global reading, 0..256: each word scored on 0..64 by what it holds.
///
/// G0 and G2 are sparse sets (coefficients with energy, adjacency cells),
/// so plain bit agreement would be dominated by the zeros two unrelated
/// works share: they are read as Jaccard overlap.  G1 holds 32 two-bit run
/// classes and G3 eight regions of three fields: those are read as equal
/// symbols.
#[inline]
pub fn global_sim(a: &[u64; GLOBAL_WORDS], b: &[u64; GLOBAL_WORDS]) -> i32 {
    let jac = |x: u64, y: u64| -> i32 {
        let u = (x | y).count_ones() as i32;
        if u == 0 {
            64
        } else {
            64 * (x & y).count_ones() as i32 / u
        }
    };
    let g0 = jac(a[0], b[0]);
    let g2 = jac(a[2], b[2]);
    // two-bit symbols: equal when both bits of the pair agree
    let x1 = a[1] ^ b[1];
    let g1 = 2 * (32 - ((x1 | (x1 >> 1)) & 0x5555_5555_5555_5555).count_ones() as i32);
    // eight regions x (iso 3 bits, aspect 3 bits, holes 2 bits)
    let x3 = a[3] ^ b[3];
    let iso = x3 & 0x0707_0707_0707_0707;
    let asp = (x3 >> 3) & 0x0707_0707_0707_0707;
    let hol = (x3 >> 6) & 0x0303_0303_0303_0303;
    let fold = |v: u64| -> u64 { (v | (v >> 1) | (v >> 2)) & 0x0101_0101_0101_0101 };
    let eq = 24 - (fold(iso).count_ones() + fold(asp).count_ones() + fold(hol).count_ones()) as i32;
    let g3 = eq * 64 / 24;
    g0 + g1 + g2 + g3
}

pub fn route_score(a: &XRoute, b: &XRoute) -> RouteScore {
    let mut s = RouteScore::default();
    let both = a.flags & b.flags;
    s.measurable = both & (RF_LOCAL | RF_BAND | RF_GLOBAL);
    if both & RF_LOCAL != 0 {
        s.local = a.local_mh.iter().zip(b.local_mh.iter()).filter(|(x, y)| x == y).count() as i32;
    }
    if both & RF_BAND != 0 {
        s.band = a.band_mh.iter().zip(b.band_mh.iter()).filter(|(x, y)| x == y).count() as i32;
    }
    if both & RF_GLOBAL != 0 {
        s.global = global_sim(&a.global, &b.global);
    }
    s
}

/// The route screen class (§6.5, §12.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteClass {
    /// a fast-positive certificate on at least one family
    Fast,
    /// something is moderately positive, nothing certifies
    Defer,
    /// every measurable family sits below its lower bar
    Reject,
    /// no family is measurable on both sides
    Absent,
}

pub fn route_class(s: &RouteScore, p: &XProfile) -> RouteClass {
    if s.measurable == 0 {
        return RouteClass::Absent;
    }
    let l = s.measurable & RF_LOCAL != 0;
    let b = s.measurable & RF_BAND != 0;
    let g = s.measurable & RF_GLOBAL != 0;
    if (l && s.local >= p.t_local_fast) || (b && s.band >= p.t_band_fast) || (g && s.global >= p.t_global_fast) {
        return RouteClass::Fast;
    }
    let low_l = !l || s.local < p.t_local_low;
    let low_b = !b || s.band < p.t_band_low;
    let low_g = !g || s.global < p.t_global_low;
    if low_l && low_b && low_g {
        RouteClass::Reject
    } else {
        RouteClass::Defer
    }
}

// ----------------------------------------------------------- batch (XRank)

/// Candidate routes column-major: `local[lane * n + c]`, `band[lane * n + c]`,
/// `global[word * n + c]` — the layout §7 asks for, so one query lane byte is
/// compared against sixteen candidates at once.  `n` is rounded up to a
/// multiple of 16 with zero padding; `flags` keeps each candidate's family
/// bits.
pub struct RouteSoA {
    pub n: usize,
    pub stride: usize,
    pub local: Vec<u8>,
    pub band: Vec<u8>,
    pub global: Vec<u64>,
    pub flags: Vec<u16>,
}

impl RouteSoA {
    pub fn with_capacity(n: usize) -> RouteSoA {
        let stride = (n.max(1) + 15) & !15;
        RouteSoA {
            n: 0,
            stride,
            local: vec![0; stride * LOCAL_LANES],
            band: vec![0; stride * BAND_LANES],
            global: vec![0; stride * GLOBAL_WORDS],
            flags: vec![0; stride],
        }
    }

    /// Reset for `n` candidates, growing only when the stride must.
    pub fn begin(&mut self, n: usize) {
        let stride = (n.max(1) + 15) & !15;
        if stride > self.stride {
            self.stride = stride;
            self.local = vec![0; stride * LOCAL_LANES];
            self.band = vec![0; stride * BAND_LANES];
            self.global = vec![0; stride * GLOBAL_WORDS];
            self.flags = vec![0; stride];
        }
        self.n = n;
        // padding lanes must not accidentally equal a query lane: 0xff
        // against a query of 0xff would count, so clear the flags instead and
        // let the caller ignore padded columns (they are never reported)
        for v in self.flags[n..self.stride].iter_mut() {
            *v = 0;
        }
    }

    pub fn set(&mut self, c: usize, r: &XRoute) {
        let st = self.stride;
        for l in 0..LOCAL_LANES {
            self.local[l * st + c] = r.local_mh[l];
        }
        for l in 0..BAND_LANES {
            self.band[l * st + c] = r.band_mh[l];
        }
        for w in 0..GLOBAL_WORDS {
            self.global[w * st + c] = r.global[w];
        }
        self.flags[c] = r.flags;
    }

    pub fn from_routes(rs: &[XRoute]) -> RouteSoA {
        let mut s = RouteSoA::with_capacity(rs.len());
        s.begin(rs.len());
        for (i, r) in rs.iter().enumerate() {
            s.set(i, r);
        }
        s
    }
}

/// Score one query against every candidate of the SoA, into `out[0..n]`.
pub fn route_batch(q: &XRoute, soa: &RouteSoA, out: &mut [RouteScore]) {
    let n = soa.n;
    assert!(out.len() >= n);
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    {
        unsafe { wasm_batch::lanes(q, soa, out) };
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
    {
        unsafe { x86_batch::lanes(q, soa, out) };
    }
    #[cfg(not(any(
        all(target_arch = "wasm32", target_feature = "simd128"),
        all(target_arch = "x86_64", target_feature = "sse2")
    )))]
    {
        lanes_scalar(q, soa, out);
    }
    finish_batch(q, soa, out);
}

/// The global words and the measurability mask, scalar on every target
/// (four popcounts per candidate).
fn finish_batch(q: &XRoute, soa: &RouteSoA, out: &mut [RouteScore]) {
    let st = soa.stride;
    for c in 0..soa.n {
        let both = q.flags & soa.flags[c];
        let s = &mut out[c];
        s.measurable = both & (RF_LOCAL | RF_BAND | RF_GLOBAL);
        if both & RF_LOCAL == 0 {
            s.local = 0;
        }
        if both & RF_BAND == 0 {
            s.band = 0;
        }
        s.global = 0;
        if both & RF_GLOBAL != 0 {
            let g = [soa.global[c], soa.global[st + c], soa.global[2 * st + c], soa.global[3 * st + c]];
            s.global = global_sim(&q.global, &g);
        }
    }
}

/// The scalar twin of the lane kernels: equal lanes per candidate.
pub fn lanes_scalar(q: &XRoute, soa: &RouteSoA, out: &mut [RouteScore]) {
    let st = soa.stride;
    for c in 0..soa.n {
        let mut l = 0i32;
        for lane in 0..LOCAL_LANES {
            l += (soa.local[lane * st + c] == q.local_mh[lane]) as i32;
        }
        let mut b = 0i32;
        for lane in 0..BAND_LANES {
            b += (soa.band[lane * st + c] == q.band_mh[lane]) as i32;
        }
        out[c].local = l;
        out[c].band = b;
    }
}

/// The batch scored the scalar way end to end — the reference `route_batch`
/// is held to.
pub fn route_batch_scalar(q: &XRoute, soa: &RouteSoA, out: &mut [RouteScore]) {
    lanes_scalar(q, soa, out);
    finish_batch(q, soa, out);
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
mod x86_batch {
    use super::*;
    use core::arch::x86_64::*;

    /// Sixteen candidates per vector: a lane byte of the query splatted,
    /// compared for equality against the sixteen candidate bytes of that
    /// lane, the 0/-1 mask subtracted from a byte counter.  Counts are at
    /// most 64, so a byte holds them.
    pub unsafe fn lanes(q: &XRoute, soa: &RouteSoA, out: &mut [RouteScore]) {
        let st = soa.stride;
        let mut c = 0usize;
        while c < soa.n {
            let mut acc_l = _mm_setzero_si128();
            for lane in 0..LOCAL_LANES {
                let v = _mm_loadu_si128(soa.local.as_ptr().add(lane * st + c) as *const __m128i);
                let m = _mm_cmpeq_epi8(v, _mm_set1_epi8(q.local_mh[lane] as i8));
                acc_l = _mm_sub_epi8(acc_l, m);
            }
            let mut acc_b = _mm_setzero_si128();
            for lane in 0..BAND_LANES {
                let v = _mm_loadu_si128(soa.band.as_ptr().add(lane * st + c) as *const __m128i);
                let m = _mm_cmpeq_epi8(v, _mm_set1_epi8(q.band_mh[lane] as i8));
                acc_b = _mm_sub_epi8(acc_b, m);
            }
            let mut l = [0u8; 16];
            let mut b = [0u8; 16];
            _mm_storeu_si128(l.as_mut_ptr() as *mut __m128i, acc_l);
            _mm_storeu_si128(b.as_mut_ptr() as *mut __m128i, acc_b);
            for k in 0..16 {
                if c + k < soa.n {
                    out[c + k].local = l[k] as i32;
                    out[c + k].band = b[k] as i32;
                }
            }
            c += 16;
        }
    }
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod wasm_batch {
    use super::*;
    use core::arch::wasm32::*;

    pub unsafe fn lanes(q: &XRoute, soa: &RouteSoA, out: &mut [RouteScore]) {
        let st = soa.stride;
        let mut c = 0usize;
        while c < soa.n {
            let mut acc_l = i8x16_splat(0);
            for lane in 0..LOCAL_LANES {
                let v = v128_load(soa.local.as_ptr().add(lane * st + c) as *const v128);
                acc_l = i8x16_sub(acc_l, i8x16_eq(v, i8x16_splat(q.local_mh[lane] as i8)));
            }
            let mut acc_b = i8x16_splat(0);
            for lane in 0..BAND_LANES {
                let v = v128_load(soa.band.as_ptr().add(lane * st + c) as *const v128);
                acc_b = i8x16_sub(acc_b, i8x16_eq(v, i8x16_splat(q.band_mh[lane] as i8)));
            }
            let mut l = [0u8; 16];
            let mut b = [0u8; 16];
            v128_store(l.as_mut_ptr() as *mut v128, acc_l);
            v128_store(b.as_mut_ptr() as *mut v128, acc_b);
            for k in 0..16 {
                if c + k < soa.n {
                    out[c + k].local = l[k] as i32;
                    out[c + k].band = b[k] as i32;
                }
            }
            c += 16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::Prepared;

    fn route(seed: u64, flags: u16) -> XRoute {
        let mut s = Seq(seed);
        let mut r = XRoute {
            global: [s.next(), s.next(), s.next(), s.next()],
            local_mh: [0; LOCAL_LANES],
            band_mh: [0; BAND_LANES],
            local_count: 100,
            desc_count: 400,
            geometry_class: 2,
            flags,
        };
        for v in r.local_mh.iter_mut() {
            *v = (s.next() % 7) as u8; // few values: many equal lanes
        }
        for v in r.band_mh.iter_mut() {
            *v = (s.next() % 5) as u8;
        }
        r
    }

    #[test]
    fn batch_equals_pairwise_and_scalar() {
        let q = route(1, RF_LOCAL | RF_BAND | RF_GLOBAL | RF_TIER2);
        for n in [0usize, 1, 5, 16, 17, 33, 100] {
            let rs: Vec<XRoute> = (0..n)
                .map(|i| route(100 + i as u64, if i % 4 == 3 { RF_BAND } else { RF_LOCAL | RF_BAND | RF_GLOBAL }))
                .collect();
            let soa = RouteSoA::from_routes(&rs);
            let mut a = vec![RouteScore::default(); n];
            let mut b = vec![RouteScore::default(); n];
            route_batch(&q, &soa, &mut a);
            route_batch_scalar(&q, &soa, &mut b);
            assert_eq!(a, b, "n = {n}");
            for i in 0..n {
                assert_eq!(a[i], route_score(&q, &rs[i]), "n = {n}, candidate {i}");
            }
        }
    }

    #[test]
    fn record_roundtrips() {
        let r = route(9, RF_LOCAL | RF_GLOBAL);
        let b = r.to_bytes();
        assert_eq!(XRoute::from_bytes(&b), Some(r));
        assert_eq!(ROUTE_BYTES, 128);
    }

    #[test]
    fn classes_follow_the_rule() {
        let p = XProfile::x1();
        let m = RF_LOCAL | RF_BAND | RF_GLOBAL;
        let s = |l, b, g, meas| RouteScore { local: l, band: b, global: g, measurable: meas };
        assert_eq!(route_class(&s(64, 0, 0, m), &p), RouteClass::Fast);
        assert_eq!(route_class(&s(0, 0, 0, m), &p), RouteClass::Reject);
        assert_eq!(route_class(&s(p.t_local_low, 0, 0, m), &p), RouteClass::Defer);
        assert_eq!(route_class(&s(0, 0, 0, 0), &p), RouteClass::Absent);
        // an absent family cannot certify and cannot reject on its own
        assert_eq!(route_class(&s(64, 0, 0, RF_BAND), &p), RouteClass::Reject);
        assert_eq!(route_class(&s(0, 0, 256, RF_GLOBAL), &p), RouteClass::Fast);
    }

    /// Route derivation 2's run-length and adjacency words are exactly
    /// invariant under the square's symmetries (the runs and adjacency
    /// sections are computed at full resolution); derivation 1's run-length
    /// word is not, because its diagonal half moves under a flip.  The
    /// region word, a sorted list in derivation 2, holds more often than
    /// derivation 1's stored order but not always: the shapes section keeps
    /// eight regions, breaks area ties for the eighth place by position, and
    /// resamples canvases whose long side exceeds 128 pixels.
    #[test]
    fn derivation_2_words_are_d4_invariant() {
        use crate::config::Config;
        use crate::synth::{mirror, pixel_art, rot90, transpose};
        use crate::wire::hash;
        use crate::x::profile::XBound;
        let cfg = Config::default();
        let rot = crate::keypoints::RotCache::new(&crate::keypoints::pattern());
        let (x1, x2) = (XBound::x1(), XBound::shipped());
        let route = |im: &crate::synth::Img, xb: &XBound| -> XRoute {
            let f = hash(&im.px, im.w, im.h, &cfg, &rot);
            let p = Prepared::new(&f.t1, Some(&f.t2)).unwrap();
            XRoute::build(&p, &xb.salts, &xb.xp)
        };
        let (mut n, mut g3_eq, mut g3_eq_x1, mut g1_moved_x1) = (0, 0, 0, 0);
        for i in 0..12u64 {
            let (w, h) = (40 + 7 * (i as usize % 9), 36 + 11 * (i as usize % 7));
            let b = pixel_art(w, h, 900 + 37 * i, 3 + (i as usize * 5) % 14, (i % 3) as u8);
            let (r1, r2) = (route(&b, &x1), route(&b, &x2));
            for c in [mirror(&b), rot90(&b), rot90(&rot90(&b)), transpose(&b)] {
                let (c1, c2) = (route(&c, &x1), route(&c, &x2));
                n += 1;
                assert_eq!(r2.global[1], c2.global[1], "G1 (derivation 2) moved on base {i}");
                assert_eq!(r2.global[2], c2.global[2], "G2 moved on base {i}");
                g3_eq += (r2.global[3] == c2.global[3]) as usize;
                g3_eq_x1 += (r1.global[3] == c1.global[3]) as usize;
                g1_moved_x1 += (r1.global[1] != c1.global[1]) as usize;
            }
        }
        assert!(g3_eq >= g3_eq_x1 && g3_eq * 4 >= n * 3, "G3 equal on {g3_eq} of {n} (derivation 1: {g3_eq_x1})");
        assert!(g1_moved_x1 > 0, "derivation 1's diagonal half never moved — the test lost its point");
        println!("derivation 2: G1 and G2 equal on {n} of {n} D4 copies, G3 on {g3_eq} (derivation 1: {g3_eq_x1}); derivation 1's G1 moved on {g1_moved_x1}");
    }

    #[test]
    fn minhash_of_equal_sets_agrees_everywhere() {
        let salts = RouteSalts::new(&XProfile::x1());
        let e: Vec<u64> = (0..50).map(|i| mix64(i)).collect();
        let (mut a, mut b) = ([0u8; LOCAL_LANES], [0u8; LOCAL_LANES]);
        minhash_into(&e, &salts.local, &mut a);
        let mut e2 = e.clone();
        e2.reverse();
        minhash_into(&e2, &salts.local, &mut b);
        assert_eq!(a, b, "order-free");
        // half the elements replaced: roughly half the lanes move
        let e3: Vec<u64> = (0..50).map(|i| if i < 25 { mix64(i) } else { mix64(1000 + i) }).collect();
        minhash_into(&e3, &salts.local, &mut b);
        let eq = a.iter().zip(b.iter()).filter(|(x, y)| x == y).count();
        assert!(eq > 10 && eq < 55, "{eq}");
    }
}
