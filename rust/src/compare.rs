//! Comparison (SPEC-003 §9).
//!
//! The channel contract is inherited verbatim from v2 §5.1 — it is the
//! best-tested thing in this family.  Every channel reports raw, control and
//! measurable; a channel that cannot evaluate ABSTAINS rather than returning a
//! number that happens to read as favourable.
//!
//! What is new is that the geometric channel has a null too (§9.4), so it can
//! be reasoned about beside the others instead of being a bare count bolted on.

use crate::config::*;
use crate::keypoints::{level_dim, mirror_desc, Keypoint};
use crate::tables::{pop_table, RC10, RS10, SCALE_Q16};
use crate::prepared::{canon_swapped, corr_v3, BagShare, PairCtx, Prepared};
use crate::wire::{Tier1, F_FLAT, F_SIL};

#[derive(Clone, Debug, Default)]
pub struct Channel {
    pub value: i64,
    pub raw: i64,
    pub control: i64,
    pub measurable: bool,
    pub control_ran: bool,
    pub note: String,
}

fn abstain(why: &str) -> Channel {
    Channel { note: why.to_string(), ..Default::default() }
}

pub const DIHEDRAL: [&str; 8] = [
    "Identity", "Flip H", "Flip V", "Rotate 180",
    "Transpose", "Rotate 90", "Rotate 270", "Anti-transpose",
];
pub const D4_INVERSE: [usize; 8] = [0, 1, 2, 3, 4, 6, 5, 7];

// ================================================================== DCT

struct Block {
    sign: Vec<u8>,
    mag: Vec<u8>,
    n: usize,
    bits: usize,
}

fn unpack(bytes: &[u8], off: usize, n: usize, bits: usize) -> Block {
    let mbits = bits - 1;
    let mut sign = vec![0u8; n];
    let mut mag = vec![0u8; n];
    let mut bit = 0usize;
    for i in 0..n {
        let mut code = 0u32;
        for _ in 0..bits {
            code = (code << 1) | ((bytes[off + (bit >> 3)] >> (7 - (bit & 7))) & 1) as u32;
            bit += 1;
        }
        sign[i] = ((code >> mbits) & 1) as u8;
        mag[i] = (code & ((1u32 << mbits) - 1)) as u8;
    }
    Block { sign, mag, n, bits }
}

fn dct_blocks(t: &Tier1) -> (Block, Vec<Block>, Vec<Block>) {
    let d = t.sec("dct");
    let l0 = unpack(d, 0, 256, 2);
    let l1 = (0..4).map(|i| unpack(d, 64 + i * 16, 64, 2)).collect();
    let l2 = (0..16).map(|i| unpack(d, 128 + i * 8, 16, 4)).collect();
    (l0, l1, l2)
}

fn d4_block(b: &Block, n: usize, tr: bool, fh: bool, fv: bool) -> Block {
    let mut sign = vec![0u8; n * n];
    let mut mag = vec![0u8; n * n];
    for v in 0..n {
        for u in 0..n {
            let src = if tr { u * n + v } else { v * n + u };
            let mut s = b.sign[src];
            if fh && (u & 1) != 0 {
                s ^= 1;
            }
            if fv && (v & 1) != 0 {
                s ^= 1;
            }
            sign[v * n + u] = s;
            mag[v * n + u] = b.mag[src];
        }
    }
    Block { sign, mag, n: n * n, bits: b.bits }
}

fn d4_grid(gx: usize, gy: usize, g: usize, tr: bool, fh: bool, fv: bool) -> usize {
    let (mut x, mut y) = (gx, gy);
    if tr {
        std::mem::swap(&mut x, &mut y);
    }
    if fh {
        x = g - 1 - x;
    }
    if fv {
        y = g - 1 - y;
    }
    y * g + x
}

fn block_sim(x: &Block, y: &Block, inv: bool, pop: &[u8]) -> i64 {
    let f = if inv { 1u8 } else { 0u8 };
    let mut d = 0i64;
    for i in 1..x.n {
        d += ((x.sign[i] ^ f) ^ y.sign[i]) as i64;
        d += pop[(x.mag[i] ^ y.mag[i]) as usize] as i64;
    }
    let nb = (x.n as i64 - 1) * x.bits as i64;
    clamp(SCALE - 2 * d * SCALE / nb, 0, SCALE)
}

/// SIXTEEN hypotheses: the eight of D4, each with and without a global sign
/// flip.  Inverting a work's luminance negates every AC coefficient, so an
/// inverted copy is a SIGN FLIP on the stored code — the same "compute the
/// symmetry from stored bits" move the local fingerprints make for the
/// complement.  The control is the median of the sixteen, so taking the best of
/// more hypotheses cannot inflate the reading.
/// The winning hypothesis of the DCT channel, as the report states it.
#[derive(Clone, Copy, Debug, Default)]
pub struct DctDetail {
    pub l0: i64,
    pub l1: i64,
    pub l2: i64,
    pub t: usize,
    pub inverted: bool,
}

fn dct_channel(a: &Tier1, b: &Tier1, pop: &[u8]) -> (Channel, usize, bool, Option<DctDetail>) {
    if a.flags & F_FLAT != 0 || b.flags & F_FLAT != 0 {
        return (abstain("one side has no tonal structure to transform"), 0, false, None);
    }
    let (pa0, pa1, pa2) = dct_blocks(a);
    let (pb0, pb1, pb2) = dct_blocks(b);
    let mut per: Vec<(usize, bool, i64, [i64; 3])> = Vec::with_capacity(16);
    for inv in [false, true] {
        for t in 0..8usize {
            let (tr, fh, fv) = ((t >> 2) & 1 == 1, (t >> 1) & 1 == 1, t & 1 == 1);
            let l0 = block_sim(&d4_block(&pa0, 16, tr, fh, fv), &pb0, inv, pop);
            let mut l1 = 0i64;
            for i in 0..4usize {
                l1 += block_sim(
                    &d4_block(&pa1[i], 8, tr, fh, fv),
                    &pb1[d4_grid(i & 1, i >> 1, 2, tr, fh, fv)],
                    inv,
                    pop,
                );
            }
            l1 /= 4;
            let mut l2 = 0i64;
            for i in 0..16usize {
                l2 += block_sim(
                    &d4_block(&pa2[i], 4, tr, fh, fv),
                    &pb2[d4_grid(i & 3, i >> 2, 4, tr, fh, fv)],
                    inv,
                    pop,
                );
            }
            l2 /= 16;
            per.push((t, inv, (2 * l0 + 2 * l1 + l2) / 5, [l0, l1, l2]));
        }
    }
    let mut best = per[0];
    for p in per.iter() {
        if p.2 > best.2 {
            best = *p;
        }
    }
    let mut vals: Vec<i64> = per.iter().map(|p| p.2).collect();
    vals.sort_unstable();
    let med = vals[8];
    let ch = Channel {
        value: chance_correct(best.2, med),
        raw: best.2,
        control: med,
        measurable: true,
        control_ran: true,
        note: format!(
            "global layout at three scales, best of sixteen symmetries ({}{})",
            DIHEDRAL[best.0],
            if best.1 { " + inverted" } else { "" }
        ),
    };
    let d = DctDetail { l0: best.3[0], l1: best.3[1], l2: best.3[2], t: best.0, inverted: best.1 };
    (ch, best.0, best.1, Some(d))
}

// ==================================================== local fingerprints

pub struct Bag {
    // v4 (SPEC-004 §10) reads these; visibility only, behaviour untouched.
    pub(crate) hi: Vec<u32>,
    pub(crate) lo: Vec<u32>,
    pub(crate) x: Vec<i64>,
    pub(crate) y: Vec<i64>,
    pub(crate) n: usize,
}

impl Bag {
    /// Number of local codes.
    pub fn len(&self) -> usize {
        self.n
    }
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
    /// Code `i` as stored, `(hi, lo)`.
    pub fn code(&self, i: usize) -> (u32, u32) {
        (self.hi[i], self.lo[i])
    }
    /// Anchor of code `i`, in the 16-bit aspect-true frame.
    pub fn anchor(&self, i: usize) -> (i64, i64) {
        (self.x[i], self.y[i])
    }
}

pub(crate) fn read_local(t: &Tier1) -> Bag {
    let n = t.count("local");
    let s = t.sec("local");
    let p = t.sec("anchors");
    let mut b = Bag { hi: vec![0; n], lo: vec![0; n], x: vec![0; n], y: vec![0; n], n };
    for i in 0..n {
        let o = i << 3;
        let q = i << 2;
        b.hi[i] = u32::from_le_bytes([s[o], s[o + 1], s[o + 2], s[o + 3]]);
        b.lo[i] = u32::from_le_bytes([s[o + 4], s[o + 5], s[o + 6], s[o + 7]]);
        b.x[i] = u16::from_le_bytes([p[q], p[q + 1]]) as i64;
        b.y[i] = u16::from_le_bytes([p[q + 2], p[q + 3]]) as i64;
    }
    b
}

fn rot64(hi: u32, lo: u32, k: u32) -> (u32, u32) {
    let k = k & 63;
    if k == 0 {
        (hi, lo)
    } else if k == 32 {
        (lo, hi)
    } else if k < 32 {
        ((hi << k) | (lo >> (32 - k)), (lo << k) | (hi >> (32 - k)))
    } else {
        let j = k - 32;
        ((lo << j) | (hi >> (32 - j)), (hi << j) | (lo >> (32 - j)))
    }
}

#[cfg(test)]
pub(crate) fn rot_bag(y: &Bag, k: u32) -> Bag {
    let mut o = Bag { hi: vec![0; y.n], lo: vec![0; y.n], x: y.x.clone(), y: y.y.clone(), n: y.n };
    for i in 0..y.n {
        let (nh, nl) = rot64(y.hi[i], y.lo[i], k);
        o.hi[i] = nh;
        o.lo[i] = nl;
    }
    o
}

#[cfg(test)]
#[inline(always)]
pub(crate) fn hdf(a: &Bag, i: usize, b: &Bag, j: usize) -> i32 {
    ((a.hi[i] ^ b.hi[j]).count_ones() + (a.lo[i] ^ b.lo[j]).count_ones()) as i32
}

/// Injective greedy: every fingerprint carries at most one match.  Without
/// injectivity one busy host region answers for every guest region at once,
/// which is exactly the "figure inside a figure" cheat.
#[cfg(test)]
pub(crate) fn match_bags(x: &Bag, y: &Bag, t: i32) -> Vec<(usize, usize)> {
    let (n, m) = (x.n, y.n);
    let mut dist = vec![-1i32; n * m];
    let mut any = vec![false; (t + 1) as usize];
    for i in 0..n {
        let base = i * m;
        for j in 0..m {
            let d = hdf(x, i, y, j);
            if d <= t {
                dist[base + j] = d;
                any[d as usize] = true;
            }
        }
    }
    let mut ux = vec![false; n];
    let mut uy = vec![false; m];
    let mut out = Vec::new();
    for d in 0..=t {
        if !any[d as usize] {
            continue;
        }
        for i in 0..n {
            if ux[i] {
                continue;
            }
            let base = i * m;
            for j in 0..m {
                if uy[j] || dist[base + j] != d {
                    continue;
                }
                ux[i] = true;
                uy[j] = true;
                out.push((i, j));
                break;
            }
        }
    }
    out
}

/// How generic is each code inside its OWN bag?  A plain horizontal boundary
/// produces the same code wherever it appears, in any artwork, so two unrelated
/// pixel-art works share dozens of them.  A code that already matches eight
/// regions of its own work is describing a TEXTURE, not an identity.
#[cfg(test)]
pub(crate) fn burst(x: &Bag, t: i32) -> Vec<i64> {
    let mut c = vec![1i64; x.n];
    for i in 0..x.n {
        for j in 0..x.n {
            if j != i && hdf(x, i, x, j) <= t {
                c[i] += 1;
            }
        }
    }
    c
}

/// A count of collisions cannot tell a paste from a coincidence: an impostor
/// built from the same tileset collides just as often, only everywhere at once.
/// A paste agrees on ONE offset.
/// Placement coherence as the report states it.  `full` is absent when the
/// reading stopped before a scale existed (fewer than three hits, or no pair of
/// hits far enough apart on both sides) — the reference reports only
/// `{value, measurable, inliers}` then.
#[derive(Clone, Copy, Debug, Default)]
pub struct Coh {
    pub value: i64,
    pub measurable: bool,
    pub inliers: i64,
    /// (fraction, scale, dx, dy)
    pub full: Option<(i64, i64, i64, i64)>,
}

fn coherence(x: &Bag, y: &Bag, hits: &[(usize, usize)], tol: i64) -> Coh {
    if hits.len() < 3 {
        return Coh::default();
    }
    let mut ratios: Vec<i64> = Vec::new();
    for i in 0..hits.len() {
        for j in i + 1..hits.len() {
            let gx = x.x[hits[i].0] - x.x[hits[j].0];
            let gy = x.y[hits[i].0] - x.y[hits[j].0];
            let hx = y.x[hits[i].1] - y.x[hits[j].1];
            let hy = y.y[hits[i].1] - y.y[hits[j].1];
            let gd = gx * gx + gy * gy;
            let hd = hx * hx + hy * hy;
            if gd < 262144 || hd < 262144 {
                continue;
            }
            ratios.push(idiv(isqrt(hd) * 256, isqrt(gd).max(1)));
        }
    }
    if ratios.is_empty() {
        return Coh::default();
    }
    // the median is one order statistic; a selection finds the same value
    let mid = ratios.len() >> 1;
    let sig = *ratios.select_nth_unstable(mid).1;
    let (mut best, mut bdx, mut bdy) = (0i64, 0i64, 0i64);
    // the hits' projections, once: every candidate offset is one of them
    let proj: Vec<(i64, i64)> = hits
        .iter()
        .map(|h| (y.x[h.1] - ((sig * x.x[h.0]) >> 8), y.y[h.1] - ((sig * x.y[h.0]) >> 8)))
        .collect();
    let tol2 = tol * tol;
    for &(ox, oy) in proj.iter() {
        let mut n = 0i64;
        for &(px, py) in proj.iter() {
            let (dx, dy) = (px - ox, py - oy);
            if dx * dx + dy * dy <= tol2 {
                n += 1;
            }
        }
        // strict: the first offset reaching the maximum is the one reported
        if n > best {
            best = n;
            bdx = ox;
            bdy = oy;
        }
    }
    // A fraction alone is not evidence: with four matches, one placement
    // explains all four by arithmetic rather than by agreement.
    let frac = clamp(best * SCALE / hits.len() as i64, 0, SCALE);
    let conf = clamp(best * SCALE / 8, 0, SCALE);
    Coh { value: frac * conf / SCALE, measurable: best >= 4, inliers: best, full: Some((frac, sig, bdx, bdy)) }
}

struct LocalOut {
    ch: Channel,
    coh_value: i64,
    coh_measurable: bool,
    lift: i64,
    proportion: i64,
    detail: Option<LocalDetail>,
}

/// The v3 local channel as the report states it.
#[derive(Clone, Debug, Default)]
pub struct LocalDetail {
    pub matched: i64,
    pub chance: i64,
    pub of: i64,
    /// (i, j, distance), canonical order until the verdict maps it back
    pub pairs: Vec<(usize, usize, i32)>,
    pub coherence: Coh,
    pub weighted_match: i64,
}

/// `match_bags` on a precomputed distance matrix (`d[i * m + j]`): the same
/// distance-stratified injective greedy, row order, first free column.
pub(crate) fn match_bags_d(n: usize, m: usize, d: &[u8], t: i32) -> Vec<(usize, usize)> {
    let mut any = vec![false; (t + 1).max(0) as usize];
    for &v in d.iter() {
        if (v as i32) <= t {
            any[v as usize] = true;
        }
    }
    let mut ux = vec![false; n];
    let mut uy = vec![false; m];
    let mut out = Vec::new();
    for dd in 0..=t {
        if !any[dd as usize] {
            continue;
        }
        for i in 0..n {
            if ux[i] {
                continue;
            }
            let row = &d[i * m..(i + 1) * m];
            for j in 0..m {
                if uy[j] || row[j] as i32 != dd {
                    continue;
                }
                ux[i] = true;
                uy[j] = true;
                out.push((i, j));
                break;
            }
        }
    }
    out
}

fn local_channel(ctx: &mut PairCtx, cfg: &Config) -> LocalOut {
    let (pa, pb) = (ctx.a, ctx.b);
    let (x, y) = (&pa.bag, &pb.bag);
    if x.n < 4 || y.n < 4 {
        return LocalOut {
            ch: abstain(&format!("too few distinctive regions on one side ({}/{})", x.n, y.n)),
            coh_value: 0,
            coh_measurable: false,
            lift: 0,
            proportion: 0,
            detail: None,
        };
    }
    let t = cfg.hamming_t;
    let cmax = x.n.min(y.n) as i64;
    let (n, m) = (x.n, y.n);
    let share = ctx.bags();
    let bx = BagShare::burst(&share.dxx, n, t);
    let by = BagShare::burst(&share.dyy, m, t);
    let hits = match_bags_d(n, m, &share.d[0], t);
    let c = hits.len() as i64;
    let mut w = 0i64;
    for h in hits.iter() {
        w += SCALE / bx[h.0].max(by[h.1]);
    }
    let cap = {
        let ca: i64 = bx.iter().map(|&v| SCALE / v).sum();
        let cb: i64 = by.iter().map(|&v| SCALE / v).sum();
        ca.min(cb).max(1)
    };
    // The null: rotate every code by 16, 32 and 48 bits and re-match.  These are
    // codes with the same statistics that cannot be the same regions.
    let (mut e, mut en) = (i64::MAX, 0i64);
    for k in 1..=3usize {
        // rot16, rot32, rot48: a rotation preserves within-bag distances, so
        // the rotated bag's burst profile IS `by`
        let byr = &by;
        let hr = match_bags_d(n, m, &share.d[k], t);
        let mut wr = 0i64;
        for h in hr.iter() {
            wr += SCALE / bx[h.0].max(byr[h.1]);
        }
        if wr < e {
            e = wr;
            en = hr.len() as i64;
        }
    }
    let prior = SCALE;
    let conf_at = cfg.confidence_at as i64;
    // BOTH evidence rules are computed and both reported.  They disagree on real
    // works, and choosing between them is a decision about false-positive
    // tolerance in a moderation queue, not a fact about the images.
    let purity = w * SCALE / (w + e + prior);
    let conf = clamp(c * SCALE / conf_at, 0, SCALE);
    let lift_raw = clamp(purity * conf / SCALE, 0, SCALE);
    let lift_ctl = clamp(
        (e * SCALE / (e + e + prior)) * clamp(en * SCALE / conf_at, 0, SCALE) / SCALE,
        0,
        SCALE,
    );
    let prop_raw = clamp(w * SCALE / cap, 0, SCALE);
    let prop_ctl = clamp(e * SCALE / cap, 0, SCALE);
    let (raw, ctl) = match cfg.evidence {
        Evidence::Proportion => (prop_raw, prop_ctl),
        Evidence::Lift => (lift_raw, lift_ctl),
    };
    let coh = coherence(x, y, &hits, 5500);
    let (cohv, cohm, cohn) = (coh.value, coh.measurable, coh.inliers);
    let detail = LocalDetail {
        matched: c,
        chance: en,
        of: cmax,
        pairs: hits.iter().map(|&(i, j)| (i, j, share.d[0][i * m + j] as i32)).collect(),
        coherence: coh,
        weighted_match: w,
    };
    LocalOut {
        ch: Channel {
            value: chance_correct(raw, ctl),
            raw,
            control: ctl,
            measurable: true,
            control_ran: true,
            note: format!(
                "{} of {} regions collide, {} expected by chance{}",
                c,
                cmax,
                en,
                if cohm { format!("; {} agree on one placement", cohn) } else { String::new() }
            ),
        },
        coh_value: cohv,
        coh_measurable: cohm,
        lift: chance_correct(lift_raw, lift_ctl),
        proportion: chance_correct(prop_raw, prop_ctl),
        detail: Some(detail),
    }
}

// =============================================================== shapes

struct Shape {
    area: u32,
    per: i64,
    aspect: i64,
    holes: i64,
    radial: [i64; 32],
}

fn read_shapes(t: &Tier1) -> Vec<Shape> {
    let s = t.sec("shapes");
    (0..t.count("shapes"))
        .map(|i| {
            let o = i * 41;
            let mut radial = [0i64; 32];
            for k in 0..32 {
                radial[k] = s[o + 9 + k] as i64;
            }
            Shape {
                area: u32::from_le_bytes([s[o], s[o + 1], s[o + 2], s[o + 3]]),
                per: u16::from_le_bytes([s[o + 4], s[o + 5]]) as i64,
                aspect: u16::from_le_bytes([s[o + 6], s[o + 7]]) as i64,
                holes: s[o + 8] as i64,
                radial,
            }
        })
        .collect()
}

/// Best over all 32 rotations and both reflections, scored against the MEDIAN
/// of those 64 alignments rather than against zero: a signature that matches
/// every rotation of itself equally well is a disc, and has told us nothing.
fn radial_score(ra: &[i64; 32], rb: &[i64; 32]) -> i64 {
    let mut all: Vec<i64> = Vec::with_capacity(64);
    for flip in 0..2 {
        for rot in 0..32usize {
            let mut s = 0i64;
            for i in 0..32usize {
                let j = if flip == 1 { (rot + 32 - i) & 31 } else { (rot + i) & 31 };
                s += (ra[i] - rb[j]).abs();
            }
            all.push(s / 32);
        }
    }
    let best = *all.iter().min().unwrap();
    let mut sorted = all.clone();
    sorted.sort_unstable();
    let med = sorted[sorted.len() >> 1];
    if med <= 0 {
        0
    } else {
        clamp((med - best) * SCALE / med, 0, SCALE)
    }
}

fn shape_pair(a: &Shape, b: &Shape) -> i64 {
    let v = radial_score(&a.radial, &b.radial);
    let (ra, rb) = (a.aspect.max(1), b.aspect.max(1));
    let asp = clamp(ra.min(rb) * SCALE / ra.max(rb), 0, SCALE);
    let ia = if a.area > 0 { a.per * a.per * 256 / a.area as i64 } else { 0 };
    let ib = if b.area > 0 { b.per * b.per * 256 / b.area as i64 } else { 0 };
    let iso = if ia > 0 && ib > 0 {
        clamp(ia.min(ib) * SCALE / ia.max(ib), 0, SCALE)
    } else {
        SCALE
    };
    let hol = if a.holes == b.holes {
        SCALE
    } else {
        clamp(SCALE - 2500 * (a.holes - b.holes).abs(), 0, SCALE)
    };
    v * asp / SCALE * iso / SCALE * hol / SCALE
}

fn shape_channel(a: &Tier1, b: &Tier1) -> (Channel, Vec<(usize, usize, i64)>) {
    let x = read_shapes(a);
    let y = read_shapes(b);
    if x.is_empty() || y.is_empty() {
        return (abstain("no regions stored on one side"), Vec::new());
    }
    let mut pairs: Vec<(usize, usize, i64)> = Vec::new();
    for i in 0..x.len() {
        for j in 0..y.len() {
            pairs.push((i, j, shape_pair(&x[i], &y[j])));
        }
    }
    pairs.sort_by(|p, q| {
        q.2.cmp(&p.2)
            .then((x[p.0].area as i64 + y[p.1].area as i64).cmp(&(x[q.0].area as i64 + y[q.1].area as i64)))
    });
    let mut ux = vec![false; x.len()];
    let mut uy = vec![false; y.len()];
    let (mut tot, mut n) = (0i64, 0i64);
    let mut kept: Vec<(usize, usize, i64)> = Vec::new();
    for p in pairs.iter() {
        if ux[p.0] || uy[p.1] {
            continue;
        }
        ux[p.0] = true;
        uy[p.1] = true;
        tot += p.2;
        n += 1;
        kept.push(*p);
    }
    let cover = x.len().min(y.len()) as i64;
    let v = tot / cover.max(1);
    let ch = Channel {
        value: v,
        raw: v,
        control: 0,
        measurable: true,
        control_ran: true,
        note: format!("{} of {} regions paired by radial signature", n, cover),
    };
    (ch, kept)
}

// ============================================================= topology

struct Rag {
    qa: i64,
    qb: i64,
    ra: i64,
    rb: i64,
    n: i64,
}

fn read_rag(t: &Tier1) -> Vec<Rag> {
    let s = t.sec("rag");
    (0..t.count("rag"))
        .map(|i| {
            let o = i * 6;
            Rag {
                qa: s[o] as i64,
                qb: s[o + 1] as i64,
                ra: s[o + 2] as i64,
                rb: s[o + 3] as i64,
                n: u16::from_le_bytes([s[o + 4], s[o + 5]]) as i64,
            }
        })
        .collect()
}

fn rag_raw(x: &[Rag], y: &[Rag], shift: i64, rank: bool) -> i64 {
    let tol = 16i64;
    let (mut inter, mut tx, mut ty) = (0i64, 0i64, 0i64);
    let mut used = vec![false; y.len()];
    for e in x.iter() {
        tx += e.n;
    }
    for e in y.iter() {
        ty += e.n;
    }
    for e in x.iter() {
        let (xa, xb) = if rank { (e.ra, e.rb) } else { (e.qa, e.qb) };
        let (mut best, mut bd) = (-1i64, i64::MAX);
        for (j, f) in y.iter().enumerate() {
            if used[j] {
                continue;
            }
            let (fa, fb) = if rank { (f.ra, f.rb) } else { (f.qa, f.qb) };
            let ya = (fa + shift) & 255;
            let yb = (fb + shift) & 255;
            let (lo, hi) = (ya.min(yb), ya.max(yb));
            let d = (xa - lo).abs() + (xb - hi).abs();
            if d < bd {
                bd = d;
                best = j as i64;
            }
        }
        if best >= 0 && bd <= 2 * tol {
            used[best as usize] = true;
            inter += e.n.min(y[best as usize].n);
        }
    }
    let uni = tx + ty - inter;
    if uni > 0 {
        clamp(inter * SCALE / uni, 0, SCALE)
    } else {
        0
    }
}

/// Rank agreement is not a better score — it is a DIFFERENT finding.  Rank does
/// not survive a rebuilt palette, so rank agreement means the palette was NOT
/// rebuilt: same export rather than recolour.
fn topology_channel(a: &Tier1, b: &Tier1, cfg: &Config) -> (Channel, bool, (i64, i64)) {
    let x = read_rag(a);
    let y = read_rag(b);
    if x.len() < 3 || y.len() < 3 {
        return (abstain("fewer than 3 colour boundaries on one side"), false, (0, 0));
    }
    let score = |rank: bool| -> (i64, i64, i64) {
        let raw = rag_raw(&x, &y, 0, rank);
        let mut ctl = SCALE;
        for s in [85i64, 128, 171] {
            ctl = ctl.min(rag_raw(&x, &y, s, rank));
        }
        (raw, ctl, chance_correct(raw, ctl))
    };
    let q = score(false);
    let r = score(true);
    let use_ = if cfg.rag_endpoint == RagEndpoint::Rank { r } else { q };
    let ch = Channel {
        value: use_.2,
        raw: use_.0,
        control: use_.1,
        measurable: true,
        control_ran: true,
        note: format!("{}/{} boundary pairs; quantile {}, rank {}", x.len(), y.len(), q.2, r.2),
    };
    (ch, r.2 >= q.2 && r.2 >= 3000, (q.2, r.2))
}

// ================================================================= runs

fn run_raw(a: &Tier1, b: &Tier1, rot: usize) -> i64 {
    let (x, y) = (a.sec("runs"), b.sec("runs"));
    let mut s = 0i64;
    for axis in 0..3usize {
        for i in 0..16usize {
            s += (x[axis * 16 + i] as i64 - y[axis * 16 + ((i + rot) & 15)] as i64).abs();
        }
    }
    clamp(SCALE - s * SCALE / (3 * 510), 0, SCALE)
}

fn runs_flat(t: &Tier1) -> bool {
    let x = t.sec("runs");
    for ax in 0..3usize {
        let (mut tot, mut top) = (0i64, 0i64);
        for i in 0..16usize {
            let v = x[ax * 16 + i] as i64;
            tot += v;
            top = top.max(v);
        }
        if tot > 0 && top * 100 < tot * 95 {
            return false;
        }
    }
    true
}

fn runs_channel(a: &Tier1, b: &Tier1) -> Channel {
    if runs_flat(a) || runs_flat(b) {
        return abstain("one side has no run-length texture (a single run per axis)");
    }
    let raw = run_raw(a, b, 0);
    let mut ctl = SCALE;
    for r in [4usize, 8, 12] {
        ctl = ctl.min(run_raw(a, b, r));
    }
    Channel {
        value: chance_correct(raw, ctl),
        raw,
        control: ctl,
        measurable: true,
        control_ran: true,
        note: "stroke-length texture, H/V/diagonal".into(),
    }
}

// ============================================================== palette

struct Pal {
    freq: i64,
    lum: i64,
    q: i64,
}

fn read_pal(t: &Tier1) -> Vec<Pal> {
    let s = t.sec("palette");
    (0..t.count("palette"))
        .map(|i| {
            let o = i << 2;
            Pal { freq: s[o + 1] as i64, lum: s[o + 2] as i64, q: s[o + 3] as i64 }
        })
        .collect()
}

fn pal_raw(x: &[Pal], y: &[Pal], shift: i64) -> i64 {
    let mut used = vec![false; y.len()];
    let (mut hit, mut tot) = (0i64, 0i64);
    for e in x.iter() {
        tot += e.freq;
    }
    for e in x.iter() {
        let (mut best, mut bd) = (-1i64, i64::MAX);
        for (j, f) in y.iter().enumerate() {
            if used[j] {
                continue;
            }
            let d = (e.q - ((f.q + shift) & 255)).abs() * 2 + (e.freq - f.freq).abs();
            if d < bd {
                bd = d;
                best = j as i64;
            }
        }
        if best >= 0 && bd <= 96 {
            used[best as usize] = true;
            hit += e.freq;
        }
    }
    if tot > 0 {
        clamp(hit * SCALE / tot, 0, SCALE)
    } else {
        0
    }
}

/// An inverted palette REFLECTS every quantile, q -> 255-q.  Try it as a second
/// hypothesis and take whichever beats its own chance floor by more.
fn palette_channel(a: &Tier1, b: &Tier1) -> (Channel, bool) {
    let x = read_pal(a);
    let y = read_pal(b);
    if x.len() < 2 || y.len() < 2 {
        return (abstain("fewer than 2 palette entries on one side"), false);
    }
    let yi: Vec<Pal> = y.iter().map(|e| Pal { freq: e.freq, lum: 255 - e.lum, q: 255 - e.q }).collect();
    let pass = |z: &[Pal]| -> (i64, i64, i64) {
        let raw = pal_raw(&x, z, 0).min(pal_raw(z, &x, 0));
        let mut ctl = SCALE;
        for s in [85i64, 128, 171] {
            ctl = ctl.min(pal_raw(&x, z, s).min(pal_raw(z, &x, s)));
        }
        (raw, ctl, chance_correct(raw, ctl))
    };
    let d = pass(&y);
    let n = pass(&yi);
    let use_ = if n.2 > d.2 { n } else { d };
    let ch = Channel {
        value: use_.2,
        raw: use_.0,
        control: use_.1,
        measurable: true,
        control_ran: true,
        note: format!(
            "share of one palette explained by the other, absolute RGB discarded{}",
            if n.2 > d.2 { " (inverted)" } else { "" }
        ),
    };
    (ch, n.2 > d.2)
}

// =========================================================== silhouette

fn silhouette_channel(a: &Tier1, b: &Tier1) -> (Channel, (i64, i64)) {
    if a.flags & F_SIL == 0 || b.flags & F_SIL == 0 {
        return (abstain("one side has no silhouette (opaque canvas, or nothing but backdrop)"), (0, 0));
    }
    let (x, y) = (a.sec("silhouette"), b.sec("silhouette"));
    let mut ra = [0i64; 32];
    let mut rb = [0i64; 32];
    for i in 0..32 {
        ra[i] = x[i] as i64;
        rb[i] = y[i] as i64;
    }
    let v = radial_score(&ra, &rb);
    let asp_a = u16::from_le_bytes([x[36], x[37]]) as i64;
    let asp_b = u16::from_le_bytes([y[36], y[37]]) as i64;
    let asp = clamp(asp_a.min(asp_b) * SCALE / asp_a.max(asp_b).max(1), 0, SCALE);
    let fill = clamp(SCALE - (x[38] as i64 - y[38] as i64).abs() * 40, 0, SCALE);
    let oa = (
        u32::from_le_bytes([x[56], x[57], x[58], x[59]]),
        u32::from_le_bytes([x[60], x[61], x[62], x[63]]),
    );
    let ob = (
        u32::from_le_bytes([y[56], y[57], y[58], y[59]]),
        u32::from_le_bytes([y[60], y[61], y[62], y[63]]),
    );
    let hd = |p: (u32, u32), q: (u32, u32)| -> i64 {
        ((p.0 ^ q.0).count_ones() + (p.1 ^ q.1).count_ones()) as i64
    };
    let occ = clamp(SCALE - hd(oa, ob) * SCALE / 32, 0, SCALE);
    let mut mom = 0i64;
    for i in 32..35 {
        mom += (x[i] as i64 - y[i] as i64).abs();
    }
    let mom_s = clamp(SCALE - mom * 60, 0, SCALE);
    let base = v * asp / SCALE * fill / SCALE;
    let raw = base * occ / SCALE * mom_s / SCALE;
    // a silhouette that agrees with every rotation of itself is a blob, not an
    // outline
    let mut occ_ctl = SCALE;
    for k in [16u32, 32, 48] {
        let r = rot64(ob.0, ob.1, k);
        occ_ctl = occ_ctl.min(clamp(SCALE - hd(oa, r) * SCALE / 32, 0, SCALE));
    }
    let ctl = base * occ_ctl / SCALE * mom_s / SCALE;
    let ch = Channel {
        value: chance_correct(raw, ctl),
        raw,
        control: ctl,
        measurable: true,
        control_ran: true,
        note: "outline shape — strong when present, silent when not".into(),
    };
    (ch, (v, occ))
}

// ================================================== the geometric channel

#[cfg(test)]
const HAM_MAX: i32 = 88;
#[cfg(test)]
const LOWE_NUM: i32 = 82;
#[cfg(test)]
const LOWE_DEN: i32 = 100;
const TBIN_W: i64 = 4096;
const TBIN_N: i64 = 64;
const TBIN_OFF: i64 = 131072;

#[inline(always)]
fn sh10(v: i64) -> i64 {
    if v >= 0 {
        (v + 512) >> 10
    } else {
        -((-v + 512) >> 10)
    }
}

#[cfg(test)]
fn ham(a: &[u32; 8], b: &[u32; 8]) -> i32 {
    let mut d = 0u32;
    for k in 0..8 {
        d += (a[k] ^ b[k]).count_ones();
    }
    d as i32
}

pub fn mirror_side(list: &[Keypoint], xmax: i32) -> Vec<Keypoint> {
    list.iter()
        .map(|k| Keypoint {
            desc: mirror_desc(&k.desc),
            x: xmax - k.x,
            y: k.y,
            level: k.level,
            sec: ((32 - k.sec as i32) & 63) as u8,
            s: k.s,
        })
        .collect()
}

/// Two-sided Lowe plus mutual nearest neighbour.  v2 applied the ratio test from
/// one side only, which is why `match(A,B) != match(B,A)` there.
///
/// The engine now derives this from a shared scan (`prepared::corr_v3`); this
/// is the reference that derivation is tested against.
#[cfg(test)]
pub(crate) fn correspond(a: &[Keypoint], b: &[Keypoint]) -> Vec<(usize, usize, i32)> {
    let (na, nb) = (a.len(), b.len());
    if na == 0 || nb == 0 {
        return Vec::new();
    }
    let mut a_best = vec![-1i64; na];
    let mut a_d1 = vec![999i32; na];
    let mut a_d2 = vec![999i32; na];
    let mut b_best = vec![-1i64; nb];
    let mut b_d1 = vec![999i32; nb];
    let mut b_d2 = vec![999i32; nb];
    for i in 0..na {
        for j in 0..nb {
            let d = ham(&a[i].desc, &b[j].desc);
            if d < a_d1[i] {
                a_d2[i] = a_d1[i];
                a_d1[i] = d;
                a_best[i] = j as i64;
            } else if d < a_d2[i] {
                a_d2[i] = d;
            }
            if d < b_d1[j] {
                b_d2[j] = b_d1[j];
                b_d1[j] = d;
                b_best[j] = i as i64;
            } else if d < b_d2[j] {
                b_d2[j] = d;
            }
        }
    }
    let mut out = Vec::new();
    for i in 0..na {
        let j = a_best[i];
        if j < 0 || a_d1[i] > HAM_MAX {
            continue;
        }
        let j = j as usize;
        if a_d1[i] * LOWE_DEN >= LOWE_NUM * a_d2[i] {
            continue;
        }
        if b_d1[j] * LOWE_DEN >= LOWE_NUM * b_d2[j] {
            continue;
        }
        if b_best[j] != i as i64 {
            continue;
        }
        out.push((i, j, a_d1[i]));
    }
    out.sort_by(|p, q| p.2.cmp(&q.2).then(p.0.cmp(&q.0)).then(p.1.cmp(&q.1)));
    out
}

fn scale_bin(q16: i64) -> i64 {
    let (mut best, mut bd) = (0i64, i64::MAX);
    for d in 0..25usize {
        let e = (q16 - SCALE_Q16[d] as i64).abs();
        if e < bd {
            bd = e;
            best = d as i64;
        }
    }
    best
}

/// Which Hough cells this correspondence votes for.
///
/// The scale comes from the LEVEL DIMENSIONS, not the level indices.  A 72 px
/// sprite and the same sprite inside a 220 px host both sit at level index 0 —
/// index difference zero — while the true normalised scale ratio is 0.327.
pub(crate) fn vote_cells(a: &Keypoint, b: &Keypoint, mda: i64, mdb: i64) -> Vec<i64> {
    let la = level_dim(mda, a.level as usize);
    let lb = level_dim(mdb, b.level as usize);
    let (c, n) = cells_v3(a, b, la, lb);
    c[..n].to_vec()
}

/// v3's vote cells for one correspondence, given the two level dimensions:
/// at most 2 rotations x 2 x 2 translations, all distinct, in the reference's
/// order.
#[inline]
pub(crate) fn cells_v3(a: &Keypoint, b: &Keypoint, la: i64, lb: i64) -> ([i64; 8], usize) {
    let mut out = [0i64; 8];
    let sq = clamp(la * 65536 / lb.max(1), 1, 1 << 24);
    if sq > (SCALE_Q16[24] as i64) << 1 || sq < (SCALE_Q16[0] as i64) >> 1 {
        return (out, 0);
    }
    let dl = scale_bin(sq);
    let ds = ((b.sec as i32 - a.sec as i32) & 63) as i64;
    let rx = sh10(a.x as i64 * RC10[ds as usize] as i64 - a.y as i64 * RS10[ds as usize] as i64);
    let ry = sh10(a.x as i64 * RS10[ds as usize] as i64 + a.y as i64 * RC10[ds as usize] as i64);
    let tx = b.x as i64 - ((sq * rx) >> 16);
    let ty = b.y as i64 - ((sq * ry) >> 16);
    let xb = clamp((tx + TBIN_OFF) / TBIN_W, 0, TBIN_N - 1);
    let yb = clamp((ty + TBIN_OFF) / TBIN_W, 0, TBIN_N - 1);
    let rb = ds >> 2;
    // Nearest-two on rotation and each translation axis (Lowe's soft binning):
    // a correspondence sitting on a bin boundary must not be lost.
    let (mut xs, mut xn) = ([xb, 0], 1usize);
    let (mut ys, mut yn) = ([yb, 0], 1usize);
    if ((tx + TBIN_OFF) - xb * TBIN_W) * 2 >= TBIN_W {
        if xb + 1 < TBIN_N {
            xs[1] = xb + 1;
            xn = 2;
        }
    } else if xb > 0 {
        xs[1] = xb - 1;
        xn = 2;
    }
    if ((ty + TBIN_OFF) - yb * TBIN_W) * 2 >= TBIN_W {
        if yb + 1 < TBIN_N {
            ys[1] = yb + 1;
            yn = 2;
        }
    } else if yb > 0 {
        ys[1] = yb - 1;
        yn = 2;
    }
    let rs = [rb, if (ds & 3) >= 2 { (rb + 1) & 15 } else { (rb + 15) & 15 }];
    let mut k = 0;
    for &r in rs.iter() {
        for &x in xs[..xn].iter() {
            for &y in ys[..yn].iter() {
                out[k] = ((dl * 16 + r) * TBIN_N + x) * TBIN_N + y;
                k += 1;
            }
        }
    }
    (out, k)
}

/// `level_dim(md, k)` for every level index a wire can carry.
pub(crate) fn level_table(md: i64) -> [i64; 256] {
    let mut t = [0i64; 256];
    let mut l = md;
    for (k, v) in t.iter_mut().enumerate() {
        if k > 0 {
            l = idiv(l * 10 + 6, 13);
        }
        *v = l.max(1);
    }
    t
}

/// A small counting table over non-negative cell keys: open addressing,
/// cleared by walking only the slots written.
pub(crate) struct CountTable {
    key: Vec<i64>,
    cnt: Vec<i64>,
    live: Vec<u32>,
    mask: usize,
}

impl CountTable {
    pub(crate) fn new(entries: usize) -> CountTable {
        let mut cap = 64usize;
        while cap < entries * 2 {
            cap <<= 1;
        }
        CountTable { key: vec![-1; cap], cnt: vec![0; cap], live: Vec::new(), mask: cap - 1 }
    }

    #[inline]
    pub(crate) fn add(&mut self, k: i64, by: i64) {
        let mut s = ((k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as usize & self.mask;
        loop {
            if self.key[s] == k {
                self.cnt[s] += by;
                return;
            }
            if self.key[s] < 0 {
                self.key[s] = k;
                self.cnt[s] = by;
                self.live.push(s as u32);
                return;
            }
            s = (s + 1) & self.mask;
        }
    }

    /// The key with the largest count, the smallest key on a tie — an order
    /// on (count, key), so the slot layout cannot matter.
    pub(crate) fn peak(&self) -> (i64, i64) {
        let (mut bk, mut bn) = (i64::MAX, 0i64);
        for &sl in self.live.iter() {
            let (k, n) = (self.key[sl as usize], self.cnt[sl as usize]);
            if n > bn || (n == bn && k < bk) {
                bn = n;
                bk = k;
            }
        }
        (bk, bn)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    // v4 (SPEC-004 §12.3) reads these; visibility only, behaviour untouched.
    pub r00: i64,
    pub r10: i64,
    pub tx: i64,
    pub ty: i64,
}

/// Closed-form 4-DOF similarity by fixed-point least squares.  A LINEAR SOLVE,
/// not an optimisation: no random sampling, no iteration count, no seed.
/// Coordinates drop to 12 bits so every product stays under 2^48.
pub(crate) fn fit_similarity(a: &[Keypoint], b: &[Keypoint], corr: &[(usize, usize, i32)]) -> Option<Model> {
    let n = corr.len() as i64;
    if n < 2 {
        return None;
    }
    let (mut sax, mut say, mut sbx, mut sby) = (0i64, 0i64, 0i64, 0i64);
    for c in corr {
        sax += (a[c.0].x >> 4) as i64;
        say += (a[c.0].y >> 4) as i64;
        sbx += (b[c.1].x >> 4) as i64;
        sby += (b[c.1].y >> 4) as i64;
    }
    let (ax0, ay0, bx0, by0) = (sax / n, say / n, sbx / n, sby / n);
    let (mut sx, mut sy, mut sig) = (0i64, 0i64, 0i64);
    for c in corr {
        let ax = (a[c.0].x >> 4) as i64 - ax0;
        let ay = (a[c.0].y >> 4) as i64 - ay0;
        let bx = (b[c.1].x >> 4) as i64 - bx0;
        let by = (b[c.1].y >> 4) as i64 - by0;
        sx += ax * bx + ay * by;
        sy += ax * by - ay * bx;
        sig += ax * ax + ay * ay;
    }
    if sig == 0 {
        return None;
    }
    let r00 = sx * 65536 / sig;
    let r10 = sy * 65536 / sig;
    Some(Model {
        r00,
        r10,
        tx: bx0 - ((r00 * ax0 - r10 * ay0) >> 16),
        ty: by0 - ((r10 * ax0 + r00 * ay0) >> 16),
    })
}

pub(crate) fn count_inliers(
    a: &[Keypoint],
    b: &[Keypoint],
    corr: &[(usize, usize, i32)],
    m: &Model,
    tol2: i64,
) -> (Vec<bool>, i64) {
    let mut mask = vec![false; corr.len()];
    let mut n = 0i64;
    for (i, c) in corr.iter().enumerate() {
        let ax = (a[c.0].x >> 4) as i64;
        let ay = (a[c.0].y >> 4) as i64;
        let mx = ((m.r00 * ax - m.r10 * ay) >> 16) + m.tx;
        let my = ((m.r10 * ax + m.r00 * ay) >> 16) + m.ty;
        let dx = mx - (b[c.1].x >> 4) as i64;
        let dy = my - (b[c.1].y >> 4) as i64;
        if dx * dx + dy * dy <= tol2 {
            mask[i] = true;
            n += 1;
        }
    }
    (mask, n)
}

pub(crate) struct Verify {
    pub(crate) inliers: i64,
    pub(crate) mask: Vec<bool>,
    pub(crate) model: Option<Model>,
}

/// The Hough vote replacing RANSAC: O(n) where 900 hypotheses were O(900n),
/// deterministic by construction, and it USES the scale and orientation each
/// keypoint already carries — which RANSAC throws away.
pub(crate) fn hough_verify(
    a: &[Keypoint],
    b: &[Keypoint],
    corr: &[(usize, usize, i32)],
    cfg: &Config,
    mda: i64,
    mdb: i64,
) -> Verify {
    let empty = Verify { inliers: 0, mask: vec![false; corr.len()], model: None };
    if corr.len() < cfg.geo_min_corr {
        return empty;
    }
    // The reference counted into a HashMap and kept a Vec of cells per
    // correspondence; the peak is the maximum of (count, -key), a total order,
    // so any table that counts the same keys finds the same one.
    let (lta, ltb) = (level_table(mda), level_table(mdb));
    let mut votes = CountTable::new(corr.len() * 8);
    let mut per: Vec<([i64; 8], usize)> = Vec::with_capacity(corr.len());
    for c in corr.iter() {
        let (ka, kb) = (&a[c.0], &b[c.1]);
        let cells = cells_v3(ka, kb, lta[ka.level as usize], ltb[kb.level as usize]);
        for &k in cells.0[..cells.1].iter() {
            votes.add(k, 1);
        }
        per.push(cells);
    }
    let (best_key, best_n) = votes.peak();
    if best_n < 3 {
        return empty;
    }
    let members: Vec<(usize, usize, i32)> = corr
        .iter()
        .enumerate()
        .filter(|(i, _)| per[*i].0[..per[*i].1].contains(&best_key))
        .map(|(_, c)| *c)
        .collect();
    let tol = (cfg.geo_eps as i64 >> 4).max(2);
    let tol2 = tol * tol;
    let mut m = match fit_similarity(a, b, &members) {
        Some(m) => m,
        None => return empty,
    };
    // one loose pass to gather, one refit, one tight count
    let (mask, _) = count_inliers(a, b, corr, &m, tol2 * 9 / 4);
    let inl: Vec<(usize, usize, i32)> =
        corr.iter().enumerate().filter(|(i, _)| mask[*i]).map(|(_, c)| *c).collect();
    if inl.len() >= 2 {
        if let Some(m2) = fit_similarity(a, b, &inl) {
            m = m2;
        }
    }
    let (mask, n) = count_inliers(a, b, corr, &m, tol2);
    Verify { inliers: n, mask, model: Some(m) }
}

#[derive(Clone, Debug)]
pub struct Geo {
    pub ch: Channel,
    pub inliers: i64,
    pub chance: i64,
    pub accepted: usize,
    pub hypothesis: &'static str,
    pub scale_q16: i64,
    pub mask: Vec<bool>,
    pub pairs: Vec<(usize, usize, i32)>,
    /// the winning hypothesis' model, if its verification produced one
    pub model: Option<Model>,
    /// the direct hypothesis' inliers, and the mirrored one's when it ran
    pub direct: i64,
    pub mirror_tried: Option<i64>,
}

fn geometric_channel(ctx: &mut PairCtx, cfg: &Config, xmax_a: i32, mda: i64, mdb: i64) -> Geo {
    let (a, b) = (&ctx.a.kp[..], &ctx.b.kp[..]);
    if !cfg.geo_enabled {
        return geo_abstain("geometric channel disabled".into());
    }
    if a.len() < cfg.geo_min_corr || b.len() < cfg.geo_min_corr {
        return geo_abstain(format!("too few keypoints on one side ({}/{})", a.len(), b.len()));
    }

    struct Run {
        inliers: i64,
        chance: i64,
        mask: Vec<bool>,
        pairs: Vec<(usize, usize, i32)>,
        scale_q16: i64,
        label: &'static str,
        model: Option<Model>,
    }
    let run = |aside: &[Keypoint], corr: Vec<(usize, usize, i32)>, label: &'static str| -> Run {
        if corr.len() < cfg.geo_min_corr {
            // the reference reports an EMPTY mask here, not one per pair
            return Run { inliers: 0, chance: 0, mask: Vec::new(), pairs: corr, scale_q16: 0, label, model: None };
        }
        let v = hough_verify(aside, b, &corr, cfg, mda, mdb);
        // THE NULL.  Keep the correspondence set exactly as matched — same
        // count, same distinctiveness — and permute only which B keypoint's
        // GEOMETRY each A keypoint is paired with.  That measures precisely the
        // right thing: would these matches have agreed on one placement by
        // chance?  Permuting the descriptors instead would measure whether the
        // descriptors matched, which is a different and already-answered
        // question.
        let n = corr.len();
        let mut ctl = 0i64;
        for p in [n >> 1, (n / 3).max(1)] {
            if p == 0 || p >= n {
                continue;
            }
            let permuted: Vec<(usize, usize, i32)> =
                corr.iter().enumerate().map(|(k, c)| (c.0, corr[(k + p) % n].1, c.2)).collect();
            ctl = ctl.max(hough_verify(aside, b, &permuted, cfg, mda, mdb).inliers);
        }
        let sq = match v.model {
            Some(m) => isqrt(m.r00 * m.r00 + m.r10 * m.r10),
            None => 0,
        };
        Run { inliers: v.inliers, chance: ctl, mask: v.mask, pairs: corr, scale_q16: sq, label, model: v.model }
    };

    // both hypotheses' correspondences come from the pair's shared scans
    let corr = corr_v3(ctx.direct());
    let mut best = run(a, corr, "direct");
    let direct = best.inliers;
    let mut mirror_tried = None;
    if cfg.mirror_hypothesis {
        // free: a bit permutation of the stored descriptor, no second wire
        let m = mirror_side(a, xmax_a);
        let corr = corr_v3(ctx.mirror());
        let mir = run(&m, corr, "mirrored");
        mirror_tried = Some(mir.inliers);
        if mir.inliers > best.inliers {
            best = mir;
        }
    }
    let raw = clamp(best.inliers * SCALE / cfg.geo_conf_at as i64, 0, SCALE);
    let ctl = clamp(best.chance * SCALE / cfg.geo_conf_at as i64, 0, SCALE);
    Geo {
        ch: Channel {
            value: chance_correct(raw, ctl),
            raw,
            control: ctl,
            measurable: true,
            control_ran: true,
            note: format!(
                "{} of {} correspondences agree on one similarity, {} by chance{}",
                best.inliers,
                best.pairs.len(),
                best.chance,
                if best.label != "direct" { format!(" ({})", best.label) } else { String::new() }
            ),
        },
        inliers: best.inliers,
        chance: best.chance,
        accepted: best.pairs.len(),
        hypothesis: best.label,
        scale_q16: best.scale_q16,
        mask: best.mask,
        pairs: best.pairs,
        model: best.model,
        direct,
        mirror_tried,
    }
}

fn geo_abstain(why: String) -> Geo {
    Geo {
        ch: Channel { note: why, ..Default::default() },
        inliers: 0,
        chance: 0,
        accepted: 0,
        hypothesis: "none",
        scale_q16: 0,
        mask: Vec::new(),
        pairs: Vec::new(),
        model: None,
        direct: 0,
        mirror_tried: None,
    }
}

// ========================================================== the verdict

/// SPEC-003 §6.5 palette relationship, as reported.  `share` is absent when
/// either side stores fewer than four colours (`relation` is then "unknown").
#[derive(Clone, Copy, Debug)]
pub struct PaletteRel {
    pub relation: &'static str,
    pub exact: i64,
    pub of: i64,
    pub share: Option<i64>,
}

/// SPEC-003 §6.3 brightness relationship — reported, never summed.
#[derive(Clone, Copy, Debug)]
pub struct Brightness {
    pub delta: i64,
    pub mean_delta: i64,
    pub shape_delta: i64,
    pub relation: &'static str,
}

fn brightness_relation(a: &Tier1, b: &Tier1) -> Brightness {
    let (x, y) = (a.sec("brightness"), b.sec("brightness"));
    let mut d = 0i64;
    for i in 0..6 {
        d += (x[i] as i64 - y[i] as i64).abs();
    }
    let mean = x[0] as i64 - y[0] as i64;
    let mut shape = 0i64;
    for i in 1..6 {
        let da = x[i] as i64 - x[0] as i64;
        let db = y[i] as i64 - y[0] as i64;
        shape += (da - db).abs();
    }
    let relation = if shape <= 24 {
        if mean.abs() <= 8 {
            "same tone"
        } else {
            "tone-shifted"
        }
    } else {
        "different tone"
    };
    Brightness { delta: d, mean_delta: mean, shape_delta: shape, relation }
}

/// Everything the v3 reading reports beyond its verdict — the reference's
/// per-channel fields.  Filled only when a full report was asked for.
#[derive(Clone, Debug, Default)]
pub struct V3Detail {
    pub dct: Option<DctDetail>,
    pub local: Option<LocalDetail>,
    /// (i, j, s) in the order the greedy kept them
    pub shape_pairs: Vec<(usize, usize, i64)>,
    /// (quantile, rank) values of the topology channel
    pub topology: (i64, i64),
    pub palette_inverted: bool,
    /// (radial, occupancy) of the silhouette channel
    pub silhouette: (i64, i64),
    pub palette: Option<PaletteRel>,
    pub brightness: Option<Brightness>,
    pub measured: Vec<&'static str>,
    /// "weighted" or "gate" — the rule `structural` was read with
    pub scoring: &'static str,
}

#[derive(Clone, Debug)]
pub struct Verdict {
    pub verdict: &'static str,
    pub class: String,
    pub basis: Vec<&'static str>,
    pub structural: i64,
    pub geometric: i64,
    pub weighted: i64,
    pub gate: i64,
    pub channels: Vec<(&'static str, Channel)>,
    pub geo: Geo,
    pub abstained: Vec<&'static str>,
    pub dihedral: Option<&'static str>,
    pub inverted: bool,
    pub palette_relation: &'static str,
    pub same_export: Option<bool>,
    pub identical: bool,
    pub structural_certifiable: bool,
    pub swapped: bool,
    pub lift: i64,
    pub proportion: i64,
    /// the full report's extra fields; `None` on the lean (verdict-only) path
    pub detail: Option<Box<V3Detail>>,
}

/// REPORTING ONLY — must not move a verdict.  An exact-palette match is a
/// materially stronger moderation case than a recoloured one, and v2 threw that
/// distinction away at ingest.
fn palette_relation(a: &Tier1, b: &Tier1) -> PaletteRel {
    let (na, nb) = (a.count("colour"), b.count("colour"));
    if na.min(nb) < 4 {
        return PaletteRel { relation: "unknown", exact: 0, of: 0, share: None };
    }
    let (x, y) = (a.sec("colour"), b.sec("colour"));
    let mut used = vec![false; nb];
    let mut exact = 0usize;
    for i in 0..na {
        let (mut best, mut bd) = (-1i64, i64::MAX);
        for j in 0..nb {
            if used[j] {
                continue;
            }
            let d = (x[i * 5] as i64 - y[j * 5] as i64).abs()
                + (x[i * 5 + 1] as i64 - y[j * 5 + 1] as i64).abs()
                + (x[i * 5 + 2] as i64 - y[j * 5 + 2] as i64).abs();
            if d < bd {
                bd = d;
                best = j as i64;
            }
        }
        if best >= 0 && bd <= 24 {
            used[best as usize] = true;
            exact += 1;
        }
    }
    let share = exact * 100 / na.min(nb);
    let relation = if share >= 80 {
        "identical palette"
    } else if share >= 40 {
        "related palette"
    } else {
        "rebuilt palette"
    };
    PaletteRel { relation, exact: exact as i64, of: na.min(nb) as i64, share: Some(share as i64) }
}

pub fn compare(
    a_t1: &[u8],
    a_t2: Option<&[u8]>,
    b_t1: &[u8],
    b_t2: Option<&[u8]>,
    cfg: &Config,
) -> Result<Verdict, &'static str> {
    // Errors surface in the reference's order: A's tiers, then B's.
    let a = Prepared::new(a_t1, a_t2)?;
    if let Some(e) = a.t2_error {
        return Err(e);
    }
    let b = Prepared::new(b_t1, b_t2)?;
    if let Some(e) = b.t2_error {
        return Err(e);
    }
    Ok(compare_prepared(&a, &b, cfg))
}

/// The v3 comparator on prepared sides (whose Tier 2, if given, parsed).
pub fn compare_prepared(a: &Prepared, b: &Prepared, cfg: &Config) -> Verdict {
    // P4 — canonical argument order.  compare(x, y) MUST equal compare(y, x).
    // Sort the two wires ONCE and swap the directional outputs back afterwards;
    // fixing each asymmetric site separately is a promise that has to be re-kept
    // every time a channel is added, and v2 broke it in nine verdicts.
    let swapped = canon_swapped(a, b);
    let (ca, cb) = if swapped { (b, a) } else { (a, b) };
    let mut ctx = PairCtx::new(ca, cb);
    compare_canonical(&mut ctx, swapped, cfg, Reading::Full)
}

/// How much of the v3 reading to compute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// everything, with every field the reference reports
    Full,
    /// what comparator 42's verdict reads and nothing more: the six secondary
    /// channels and `identical`.  The v3 local and geometric channels — and
    /// the v3 verdict built on them — are not computed; comparator 42 has its
    /// own local channel and its own geometry, and never reads v3's.
    Lean,
}

/// The v3 comparator on a pair already in canonical order, sharing the pair's
/// descriptor scans with whoever else holds `ctx`.
pub fn compare_canonical(ctx: &mut PairCtx, swapped: bool, cfg: &Config, reading: Reading) -> Verdict {
    let (a, b) = (ctx.a, ctx.b);
    let identical = a.t1.bytes == b.t1.bytes;
    let full = reading == Reading::Full;

    let pop = pop_table();
    let (dct, dt, dinv, dct_detail) = dct_channel(&a.t1, &b.t1, pop);
    let mut loc = if full {
        local_channel(ctx, cfg)
    } else {
        LocalOut {
            ch: abstain("not computed (lean reading)"),
            coh_value: 0,
            coh_measurable: false,
            lift: 0,
            proportion: 0,
            detail: None,
        }
    };
    let (shape, shape_pairs) = shape_channel(&a.t1, &b.t1);
    let (topology, same_export, topo_qr) = topology_channel(&a.t1, &b.t1, cfg);
    let runs = runs_channel(&a.t1, &b.t1);
    let (palette, pal_inverted) = palette_channel(&a.t1, &b.t1);
    let (sil, sil_detail) = silhouette_channel(&a.t1, &b.t1);
    let mut geo = if full {
        geometric_channel(ctx, cfg, a.v3_xmax, a.v3_max_dim, b.v3_max_dim)
    } else {
        geo_abstain("not computed (lean reading)".into())
    };
    if swapped {
        // report pair indices in the CALLER's argument order
        geo.pairs = geo.pairs.iter().map(|p| (p.1, p.0, p.2)).collect();
        if let Some(d) = loc.detail.as_mut() {
            d.pairs = d.pairs.iter().map(|p| (p.1, p.0, p.2)).collect();
            let c = &mut d.coherence;
            if c.measurable {
                if let Some((f, sc, dx, dy)) = c.full {
                    c.full = Some((f, if sc != 0 { idiv(65536, sc) } else { sc }, -dx, -dy));
                }
            }
        }
    }
    let local_detail = loc.detail.take();

    let dct_measurable = dct.measurable;
    let dct_value = dct.value;
    let topo_measurable = topology.measurable;
    let channels: Vec<(&'static str, Channel)> = vec![
        ("dct", dct),
        ("local", loc.ch),
        ("shape", shape),
        ("topology", topology),
        ("runs", runs),
        ("palette", palette),
        ("silhouette", sil),
    ];

    let (mut wsum, mut wtot) = (0i64, 0i64);
    let mut secondary: Vec<(&'static str, i64)> = Vec::new();
    let mut evidence: Option<i64> = None;
    for (name, c) in channels.iter() {
        if !c.measurable {
            continue;
        }
        wsum += c.value * weight(name);
        wtot += weight(name);
        if *name == "local" {
            evidence = Some(c.value);
        } else {
            secondary.push((name, c.value));
        }
    }
    let weighted = if wtot > 0 { wsum / wtot } else { 0 };
    secondary.sort_by(|p, q| p.1.cmp(&q.1).then(p.0.cmp(q.0)));
    let mut corrob = if secondary.is_empty() { None } else { Some(secondary[secondary.len() >> 1].1) };
    if loc.coh_measurable && (corrob.is_none() || loc.coh_value > corrob.unwrap()) {
        corrob = Some(loc.coh_value);
    }
    let gate = match (evidence, corrob) {
        (Some(e), Some(c)) => e.min(c),
        (Some(e), None) => e,
        (None, Some(c)) => c,
        (None, None) => 0,
    };
    let mut structural = match cfg.scoring {
        Scoring::Weighted => weighted,
        Scoring::Gate => gate,
    };
    // Corroboration means INDEPENDENT channels agreeing.  With fewer than three
    // measurable secondaries the median degenerates to a single channel, and the
    // structural side must not certify on it: two flat canvases certified as
    // Copy through the shape channel alone until this existed.
    let structural_certifiable = evidence.is_some() && secondary.len() >= 3;

    let gv = if geo.ch.measurable { geo.ch.value } else { 0 };
    let gi = geo.inliers;
    let geo_strong = geo.ch.measurable && gv >= Thresholds::GEO_STRONG;
    let geo_weak = geo.ch.measurable && gv >= Thresholds::GEO_WEAK;
    let s_strong = structural_certifiable && structural >= Thresholds::STRUCT_STRONG;
    let s_mod = structural_certifiable && structural >= Thresholds::STRUCT_MODERATE;
    let s_rel = structural >= Thresholds::STRUCT_WEAK;

    let pal = palette_relation(&a.t1, &b.t1);
    let pal_rel = pal.relation;

    // The lattice (§9.5).  The two scores are NEVER averaged: they measure
    // different things and fail differently, and a weighted sum lets a strong
    // structural reading drag a silent geometric one over the line.
    let (verdict, class, basis): (&'static str, String, Vec<&'static str>) = if identical {
        ("Identical", "byte-identical fingerprint".into(), vec!["bytes"])
    } else if s_strong && geo_strong {
        ("Copy", "certified — structure and geometry agree".into(), vec!["structural", "geometric"])
    } else if s_strong && structural >= Thresholds::STRUCT_SOLO {
        // the transform as reported, i.e. in the caller's argument order —
        // the reference names the class after it (a rotation by 90 read the
        // other way round is a rotation by 270)
        let shown = if swapped { D4_INVERSE[dt] } else { dt };
        let k = if dct_measurable && dct_value >= 3000 && shown != 0 {
            format!("structural only — {} class", DIHEDRAL[shown].to_lowercase())
        } else if dct_measurable && dct_value >= 3000 && dinv {
            "structural only — inversion class".into()
        } else if pal_rel == "rebuilt palette" || pal_rel == "related palette" {
            "structural only — recolour class".into()
        } else {
            "structural only — no geometric corroboration".into()
        };
        ("Copy", k, vec!["structural"])
    } else if geo_strong && gi >= Thresholds::GEO_SOLO_INLIERS {
        let k = if geo.hypothesis == "mirrored" {
            "geometric only — mirrored crop / collage class"
        } else {
            "geometric only — crop / collage class"
        };
        ("Copy", k.into(), vec!["geometric"])
    } else if s_strong || s_mod || geo_strong || geo_weak {
        let k = if geo_strong {
            "geometry agrees but below the solo bar"
        } else if s_strong {
            "structure agrees but below the solo bar"
        } else {
            "partial agreement"
        };
        let mut basis: Vec<&'static str> = Vec::new();
        if s_mod {
            basis.push("structural");
        }
        if geo_weak {
            basis.push("geometric");
        }
        ("Suspected", k.into(), basis)
    } else if s_rel {
        ("Related", "same family, not a copy".into(), vec![])
    } else {
        ("Unrelated", "no agreement above chance".into(), vec![])
    };

    if identical {
        structural = SCALE;
    }
    let mut abstained: Vec<&'static str> =
        channels.iter().filter(|(_, c)| !c.measurable).map(|(n, _)| *n).collect();
    if !geo.ch.measurable {
        abstained.push("geometric");
    }
    let detail = if full {
        Some(Box::new(V3Detail {
            dct: dct_detail,
            local: local_detail,
            shape_pairs,
            topology: topo_qr,
            palette_inverted: pal_inverted,
            silhouette: sil_detail,
            palette: Some(pal),
            brightness: Some(brightness_relation(&a.t1, &b.t1)),
            measured: channels.iter().filter(|(_, c)| c.measurable).map(|(n, _)| *n).collect(),
            scoring: match cfg.scoring {
                Scoring::Weighted => "weighted",
                Scoring::Gate => "gate",
            },
        }))
    } else {
        None
    };

    Verdict {
        verdict,
        class,
        basis,
        structural,
        geometric: gv,
        weighted: if identical { SCALE } else { weighted },
        gate: if identical { SCALE } else { gate },
        dihedral: if dct_measurable {
            Some(DIHEDRAL[if swapped { D4_INVERSE[dt] } else { dt }])
        } else {
            None
        },
        inverted: dinv,
        palette_relation: pal_rel,
        same_export: if topo_measurable { Some(same_export) } else { None },
        channels,
        geo,
        abstained,
        identical,
        structural_certifiable,
        swapped,
        lift: loc.lift,
        proportion: loc.proportion,
        detail,
    }
}

// ---------------------------------------------------------------- JSON

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Hand-rolled rather than serde: one dependency fewer in a binary two parties
/// have to agree on, and the shape is fixed anyway.
pub fn to_json(v: &Verdict) -> String {
    let mut ch = String::new();
    for (i, (n, c)) in v.channels.iter().enumerate() {
        if i > 0 {
            ch.push(',');
        }
        ch.push_str(&format!(
            "\"{}\":{{\"value\":{},\"raw\":{},\"control\":{},\"measurable\":{},\"controlRan\":{},\"note\":\"{}\"}}",
            n, c.value, c.raw, c.control, c.measurable, c.control_ran, esc(&c.note)
        ));
    }
    let strs = |v: &Vec<&'static str>| -> String {
        v.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(",")
    };
    format!(
        "{{\"verdict\":\"{}\",\"class\":\"{}\",\"basis\":[{}],\"structural\":{},\"geometric\":{},\
\"weighted\":{},\"gate\":{},\"identical\":{},\"structuralCertifiable\":{},\"swapped\":{},\
\"abstained\":[{}],\"channels\":{{{}}},\
\"geo\":{{\"measurable\":{},\"value\":{},\"raw\":{},\"control\":{},\"inliers\":{},\"chanceInliers\":{},\
\"accepted\":{},\"hypothesis\":\"{}\",\"scaleQ16\":{},\"note\":\"{}\"}},\
\"report\":{{\"dihedral\":{},\"inverted\":{},\"mirrored\":{},\"recoveredScaleQ16\":{},\"inliers\":{},\
\"chanceInliers\":{},\"palette\":\"{}\",\"sameExport\":{},\"geoHypothesis\":\"{}\",\
\"evidenceBoth\":{{\"lift\":{},\"proportion\":{}}}}}}}",
        v.verdict, esc(&v.class), strs(&v.basis), v.structural, v.geometric,
        v.weighted, v.gate, v.identical, v.structural_certifiable, v.swapped,
        strs(&v.abstained), ch,
        v.geo.ch.measurable, v.geo.ch.value, v.geo.ch.raw, v.geo.ch.control,
        v.geo.inliers, v.geo.chance, v.geo.accepted, v.geo.hypothesis, v.geo.scale_q16,
        esc(&v.geo.ch.note),
        match v.dihedral { Some(d) => format!("\"{}\"", d), None => "null".into() },
        v.inverted,
        if v.geo.ch.measurable { (v.geo.hypothesis == "mirrored").to_string() } else { "null".into() },
        v.geo.scale_q16, v.geo.inliers, v.geo.chance, v.palette_relation,
        match v.same_export { Some(b) => b.to_string(), None => "null".into() },
        v.geo.hypothesis, v.lift, v.proportion
    )
}
