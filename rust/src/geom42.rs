//! SPEC-004.2 §4–§8 — the comparator-42 geometry stage.
//!
//! Comparator 41's `multimodel.rs` is untouched and stays reachable, because a
//! verdict that was issued has to remain reproducible.  This module is the
//! same *architecture* — mutual Lowe, weighted Hough, closed-form similarity,
//! iterative multi-model extraction, MAX over a structured null family — with
//! the four things that stop working once the budget doubles:
//!
//!   §4  the matcher.  512x512 is four times 256x256, so the inner loop stops
//!       computing distances it has already lost, and the acceptance test gains
//!       an ABSOLUTE margin beside the ratio (d1=2 vs d2=3 passes Lowe at 0.667
//!       and means almost nothing).
//!   §5  the vote.  A `HashMap` keyed by cell plus a `Vec` per correspondence
//!       was affordable at 256 correspondences and is not at 512: the cells are
//!       a bounded integer space, so the table is a fixed open-addressed one
//!       and the per-correspondence cell list is a stack array.  Scale stops
//!       being a hard bucket.
//!   §6  the peak.  Weighted mass alone cannot tell four overwhelming matches
//!       from a genuinely populated transformation, so the peak is compared
//!       lexicographically on (mass, members, confidence) and the model carries
//!       a median residual.
//!   §7  consumption.  One physical structure must not be decomposed into two
//!       models because the first one's inliers stopped one keypoint short.
//!
//! Nothing here is float, and nothing here is order-dependent: the vote table
//! is scanned by slot but reduced by an explicit total order on (mass, members,
//! confidence, key), so the answer does not depend on where a key landed.

use crate::calibration::Profile;
use crate::compare::{fit_similarity, mirror_side, Model};
use crate::config::{clamp, idiv, isqrt, Config, SCALE};
use crate::coverage::{coverage, Coverage};
use crate::keypoints::{level_dim, Keypoint, DESC_NOVEL_AT, PATCH_R};
use crate::tables::{RC10, RS10, SCALE_Q16};

// ------------------------------------------------------------------ §4 pack

/// The wire keeps 32 bytes; the matcher does not have to.  Four `u64` POPCNTs
/// beat eight `u32` ones on every target that has the instruction, and the
/// conversion happens once, while Tier 2 is parsed, rather than 262144 times.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Desc4 {
    pub q: [u64; 4],
}

#[inline(always)]
pub fn pack_desc(d: &[u32; 8]) -> Desc4 {
    Desc4 {
        q: [
            (d[0] as u64) | ((d[1] as u64) << 32),
            (d[2] as u64) | ((d[3] as u64) << 32),
            (d[4] as u64) | ((d[5] as u64) << 32),
            (d[6] as u64) | ((d[7] as u64) << 32),
        ],
    }
}

pub fn pack_all(list: &[Keypoint]) -> Vec<Desc4> {
    list.iter().map(|k| pack_desc(&k.desc)).collect()
}

#[inline(always)]
pub fn hamming(a: &Desc4, b: &Desc4) -> i32 {
    ((a.q[0] ^ b.q[0]).count_ones()
        + (a.q[1] ^ b.q[1]).count_ones()
        + (a.q[2] ^ b.q[2]).count_ones()
        + (a.q[3] ^ b.q[3]).count_ones()) as i32
}

/// §4 — exact Hamming with early abort.
///
/// Returns the true distance whenever it is `<= limit`, and SOME value greater
/// than `limit` otherwise.  That is all the caller needs and it is not an
/// approximation: `limit` is `max(a_d2[i], b_d2[j])`, so a partial sum that has
/// already passed it belongs to a pair that cannot become either side's first
/// or second candidate, and every downstream test is a strict `<` against a
/// value at most `limit`.  Recall is untouched; the work is not.
#[inline(always)]
pub fn ham_cut(a: &Desc4, b: &Desc4, limit: i32) -> i32 {
    let mut d = (a.q[0] ^ b.q[0]).count_ones() as i32;
    if d > limit {
        return d;
    }
    d += (a.q[1] ^ b.q[1]).count_ones() as i32;
    if d > limit {
        return d;
    }
    d += (a.q[2] ^ b.q[2]).count_ones() as i32;
    if d > limit {
        return d;
    }
    d + (a.q[3] ^ b.q[3]).count_ones() as i32
}

// ------------------------------------------------------------ §4 matching

/// A comparator-42 correspondence.  `conf` is the Lowe uniqueness of §12.2,
/// unchanged; `sc` is the §6 strength compatibility, and the Hough weight is
/// their product.  Both travel with the ROW under every null permutation:
/// descriptor pairing is preserved, only geometry is re-dealt (Appendix D).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Corr42 {
    pub a: usize,
    pub b: usize,
    pub d1: i32,
    pub conf: i32,
    pub sc: i32,
}

impl Corr42 {
    /// §6 — the vote a correspondence carries: 1..=4096.
    #[inline(always)]
    pub fn weight(&self) -> i64 {
        self.conf as i64 * self.sc as i64
    }
}

/// §12.2 unchanged: conf = 1 + ⌊(d2 − d1)·63 / max(d2,1)⌋ ∈ 1..=64.
pub fn conf(d1: i32, d2: i32) -> i32 {
    1 + idiv((d2 as i64 - d1 as i64) * 63, (d2 as i64).max(1)) as i32
}

/// §6 — strength compatibility, Q6, floored at ½.
///
/// A weak accidental keypoint can pass Lowe.  A strong keypoint that also
/// agrees geometrically is worth more, and the floor keeps the term from
/// turning a legitimate coarse-level match into no evidence at all just
/// because the two sides normalised their FAST scores against different
/// level medians.
#[inline]
pub fn strength_compat(sa: u16, sb: u16) -> i32 {
    let (a, b) = (sa as i64, sb as i64);
    let (lo, hi) = (a.min(b).max(1), a.max(b).max(1));
    clamp(lo * 64 / hi, 32, 64) as i32
}

/// §4 — mutual-best under the Lowe ratio AND an absolute margin.
///
/// Projection onto (a, b, d1) is the 4.1 correspondence set minus whatever the
/// margin removes; with `lowe_margin = 0` it is that set exactly, which is what
/// the parity suite checks against `correspond_w`.
pub fn correspond_42(
    a: &[Keypoint],
    ad: &[Desc4],
    b: &[Keypoint],
    bd: &[Desc4],
    p: &Profile,
) -> Vec<Corr42> {
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
    // §4, revised on measurement.  The early abort is EXACT and it is also,
    // on every target measured, slower than not aborting: three saved POPCNTs
    // do not pay for a branch per word in a loop this unpredictable, and
    // removing it took a whole comparison from 13.2 ms to 8.0 ms with every
    // output byte unchanged.  `ham_cut` stays specified, golden-pinned and
    // correct — which abort strategy an engine uses is an implementation
    // choice precisely because it cannot change a distance.
    for i in 0..na {
        for j in 0..nb {
            let d = hamming(&ad[i], &bd[j]);
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
    let (num, den, margin, hmax) = (p.lowe_num, p.lowe_den, p.lowe_margin, p.ham_max);
    let mut out = Vec::new();
    for i in 0..na {
        let j = a_best[i];
        if j < 0 || a_d1[i] > hmax {
            continue;
        }
        let j = j as usize;
        if a_d1[i] * den >= num * a_d2[i] {
            continue;
        }
        if b_d1[j] * den >= num * b_d2[j] {
            continue;
        }
        if b_best[j] != i as i64 {
            continue;
        }
        // §4 — the absolute margin, two-sided exactly like the ratio, or
        // compare(A,B) and compare(B,A) would stop agreeing.
        if a_d2[i] - a_d1[i] < margin || b_d2[j] - b_d1[j] < margin {
            continue;
        }
        let d2 = a_d2[i].min(b_d2[j]);
        out.push(Corr42 {
            a: i,
            b: j,
            d1: a_d1[i],
            conf: conf(a_d1[i], d2),
            sc: strength_compat(a[i].s, b[j].s),
        });
    }
    out.sort_by(|p, q| p.d1.cmp(&q.d1).then(p.a.cmp(&q.a)).then(p.b.cmp(&q.b)));
    out
}

// -------------------------------------------------------------- §5 vote cells

const TBIN_W: i64 = 4096;
const TBIN_N: i64 = 64;
const TBIN_OFF: i64 = 131072;
/// §5 — the soft scale kernel, over 64.  Nearest bin 64, each neighbour 24.
pub(crate) const SK_NEAR: i64 = 64;
pub(crate) const SK_ADJ: i64 = 24;
/// 3 scale bins x 2 rotation x 2 x x 2 y.
pub const MAX_CELLS: usize = 24;

#[inline(always)]
fn sh10(v: i64) -> i64 {
    if v >= 0 {
        (v + 512) >> 10
    } else {
        -((-v + 512) >> 10)
    }
}

/// Nearest scale bin, ties to the lower index.
///
/// `SCALE_Q16` is strictly increasing, so `|q16 − SCALE_Q16[d]|` falls and then
/// rises: once it has risen, no later entry can win.  Breaking there is the
/// same answer as scanning all 25, and this runs once per correspondence per
/// verification — roughly twenty-five thousand times per compared pair.
fn scale_bin(q16: i64) -> i64 {
    let (mut best, mut bd) = (0i64, i64::MAX);
    for d in 0..25usize {
        let e = (q16 - SCALE_Q16[d] as i64).abs();
        if e < bd {
            bd = e;
            best = d as i64;
        } else {
            break;
        }
    }
    best
}

/// §5 — the cells a correspondence votes for, with no allocation.
#[derive(Clone, Copy)]
pub struct VoteCells {
    pub key: [i32; MAX_CELLS],
    /// scale-kernel weight of that cell, over 64
    pub kw: [i32; MAX_CELLS],
    pub len: u8,
}

impl VoteCells {
    #[inline]
    fn empty() -> Self {
        VoteCells { key: [0; MAX_CELLS], kw: [0; MAX_CELLS], len: 0 }
    }
    #[inline]
    pub fn contains(&self, k: i32) -> bool {
        self.key[..self.len as usize].contains(&k)
    }
}

/// §5 — the same cells as a product of bins: the scale bin `dl` (and, when
/// soft, its neighbours `dl - 1` and `dl + 1` inside 0..=24), two rotation
/// bins, one or two x bins and one or two y bins.  The key
/// `((l·16 + r)·64 + x)·64 + y` is a mixed-radix number, so a key belongs to
/// the cells exactly when its four digits belong to the four bin sets — which
/// is how the verifier tests membership without keeping 24 keys per
/// correspondence.  `cells()` expands the box in `vote_cells_42` order.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct CellBox {
    /// no cells at all: the level ratio is outside the scale range
    pub empty: bool,
    pub dl: u8,
    pub lo: bool,
    pub hi: bool,
    pub r: [u8; 2],
    pub x: [u8; 2],
    pub y: [u8; 2],
    pub xn: u8,
    pub yn: u8,
}

/// A vote key's digits: (scale bin, rotation bin, x bin, y bin).
#[inline(always)]
pub(crate) fn key_digits(k: i32) -> (i32, i32, i32, i32) {
    let n = TBIN_N as i32;
    (k / (16 * n * n), (k / (n * n)) & 15, (k / n) % n, k % n)
}

impl CellBox {
    /// Membership of the key whose digits are `(l, r, x, y)`.
    #[inline(always)]
    pub fn has(&self, l: i32, r: i32, x: i32, y: i32) -> bool {
        let dl = self.dl as i32;
        !self.empty
            && (l == dl || (self.lo && l == dl - 1) || (self.hi && l == dl + 1))
            && (r == self.r[0] as i32 || r == self.r[1] as i32)
            && (x == self.x[0] as i32 || (self.xn == 2 && x == self.x[1] as i32))
            && (y == self.y[0] as i32 || (self.yn == 2 && y == self.y[1] as i32))
    }

    /// The scale bins with their kernel weights, in vote order.
    #[inline(always)]
    fn scales(&self) -> ([(i32, i64); 3], usize) {
        let dl = self.dl as i32;
        let mut ls = [(dl, SK_NEAR), (0, 0), (0, 0)];
        let mut ln = 1usize;
        if self.lo {
            ls[ln] = (dl - 1, SK_ADJ);
            ln += 1;
        }
        if self.hi {
            ls[ln] = (dl + 1, SK_ADJ);
            ln += 1;
        }
        (ls, ln)
    }

    /// Calls `f(key, kernel weight)` for every cell, in `vote_cells_42` order.
    #[inline(always)]
    pub fn for_each(&self, mut f: impl FnMut(i32, i64)) {
        if self.empty {
            return;
        }
        let n = TBIN_N as i32;
        let (ls, ln) = self.scales();
        for &(l, kw) in ls[..ln].iter() {
            for &r in self.r.iter() {
                for &x in self.x[..self.xn as usize].iter() {
                    for &y in self.y[..self.yn as usize].iter() {
                        f(((l * 16 + r as i32) * n + x as i32) * n + y as i32, kw);
                    }
                }
            }
        }
    }

    pub fn cells(&self) -> VoteCells {
        let mut out = VoteCells::empty();
        let mut n = 0usize;
        self.for_each(|k, kw| {
            out.key[n] = k;
            out.kw[n] = kw as i32;
            n += 1;
        });
        out.len = n as u8;
        out
    }
}

/// §5 — identical geometry to 4.1's `vote_cells`, with scale soft-binned
/// instead of bucketed.
///
/// The scale estimate comes from the LEVEL DIMENSIONS and is therefore
/// quantised; at 256 keypoints there was not enough evidence to spend on
/// hedging that quantisation and at 512 there is.  Rotation and both
/// translation axes keep Lowe's nearest-two duplication exactly as before —
/// votes are DUPLICATED across soft bins, never split.
pub fn vote_cells_42(a: &Keypoint, b: &Keypoint, mda: i64, mdb: i64, soft: bool) -> VoteCells {
    let la = level_dim(mda, a.level as usize);
    let lb = level_dim(mdb, b.level as usize);
    vote_cells_42_l(a, b, la, lb, soft)
}

/// `vote_cells_42` with the two level dimensions already looked up.
#[inline]
pub fn vote_cells_42_l(a: &Keypoint, b: &Keypoint, la: i64, lb: i64, soft: bool) -> VoteCells {
    cell_box(a, b, la, lb, soft).cells()
}

/// The bins of `vote_cells_42_l`, unexpanded.
#[inline]
pub fn cell_box(a: &Keypoint, b: &Keypoint, la: i64, lb: i64, soft: bool) -> CellBox {
    let sq = clamp(la * 65536 / lb.max(1), 1, 1 << 24);
    if sq > (SCALE_Q16[24] as i64) << 1 || sq < (SCALE_Q16[0] as i64) >> 1 {
        return CellBox { empty: true, ..Default::default() };
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

    let mut xs = [xb as u8, 0];
    let mut xn = 1u8;
    if ((tx + TBIN_OFF) - xb * TBIN_W) * 2 >= TBIN_W {
        if xb + 1 < TBIN_N {
            xs[1] = (xb + 1) as u8;
            xn = 2;
        }
    } else if xb > 0 {
        xs[1] = (xb - 1) as u8;
        xn = 2;
    }
    let mut ys = [yb as u8, 0];
    let mut yn = 1u8;
    if ((ty + TBIN_OFF) - yb * TBIN_W) * 2 >= TBIN_W {
        if yb + 1 < TBIN_N {
            ys[1] = (yb + 1) as u8;
            yn = 2;
        }
    } else if yb > 0 {
        ys[1] = (yb - 1) as u8;
        yn = 2;
    }
    let r2 = if (ds & 3) >= 2 { (rb + 1) & 15 } else { (rb + 15) & 15 };
    CellBox {
        empty: false,
        dl: dl as u8,
        lo: soft && dl > 0,
        hi: soft && dl < 24,
        r: [rb as u8, r2 as u8],
        x: xs,
        y: ys,
        xn,
        yn,
    }
}

// ------------------------------------------------------------- §5 vote table

/// §5 — a fixed open-addressed integer table.
///
/// The Hough space is bounded (25 scales x 16 rotations x 64 x 64 translations
/// = 1638400 cells) but a pair only ever touches a few thousand of them, so
/// neither a dense array nor a hash map with an allocator behind it is the
/// right shape.  Capacity comes from the correspondence count, the hash is one
/// multiply, probing is linear, and nothing is allocated per correspondence.
///
/// A slot is one 16-byte record, so a vote touches one cache line rather than
/// one line in each of five parallel arrays, and the table at 512
/// correspondences is a quarter of a megabyte rather than a megabyte.
pub struct VoteTable {
    slots: Vec<Slot>,
    epoch: u16,
    mask: usize,
    shift: u32,
    /// the running peak: (key, mass, members, confidence) — see `add`
    bk: i32,
    bm: i64,
    bn: i32,
    bc: i64,
}

/// One cell's tally.  The widths are exact for every input the table can be
/// given: a correspondence adds at most once to a key, a pool holds at most
/// one correspondence per Tier-2 record (u16), and a vote carries mass at
/// most 4096 and confidence at most 64 — so members <= 65535, mass <=
/// 65535 * 4096 < 2^31 and confidence <= 65535 * 64 < 2^31.
#[derive(Clone, Copy, Default)]
#[repr(C, align(16))]
struct Slot {
    key: i32,
    mass: i32,
    conf: i32,
    /// low half: which `begin()` last wrote this slot — a slot whose stamp is
    /// not the current epoch IS empty, so a table is cleared by incrementing
    /// one integer; high half: members
    tag: u32,
}

impl VoteTable {
    pub fn new() -> VoteTable {
        VoteTable { slots: Vec::new(), epoch: 0, mask: 0, shift: 32, bk: i32::MAX, bm: -1, bn: 0, bc: 0 }
    }

    fn capacity_for(ncorr: usize) -> (usize, u32) {
        let want = (ncorr.max(1) * MAX_CELLS * 2).max(64);
        let (mut cap, mut bits) = (64usize, 6u32);
        while cap < want && bits < 22 {
            cap <<= 1;
            bits += 1;
        }
        (cap, bits)
    }

    /// Reset for a verification of `ncorr` correspondences.
    ///
    /// Growing is allowed and changes nothing observable: the peak is the
    /// maximum of an explicit total order on (mass, members, confidence, key),
    /// so the answer does not depend on capacity, on the probe sequence, or on
    /// which slot a key happened to land in.  That property is what makes a
    /// shared table legitimate rather than merely convenient.
    pub fn begin(&mut self, ncorr: usize) {
        let (cap, bits) = Self::capacity_for(ncorr);
        if cap > self.slots.len() {
            self.slots = vec![Slot::default(); cap];
            self.mask = cap - 1;
            self.shift = 32 - bits;
            self.epoch = 0;
        }
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            // once every 65536 verifications, pay for a real clear
            for v in self.slots.iter_mut() {
                v.tag = 0;
            }
            self.epoch = 1;
        }
        self.bk = i32::MAX;
        self.bm = -1;
        self.bn = 0;
        self.bc = 0;
    }

    #[inline(always)]
    fn slot(&self, k: i32) -> usize {
        // Knuth multiplicative, taken from the high bits.  Identical in both
        // engines: JavaScript spells it Math.imul.
        (((k as u32).wrapping_mul(2654435761)) >> self.shift) as usize & self.mask
    }

    /// Adds one vote, and keeps the peak current.
    ///
    /// Every vote adds a non-negative mass and confidence and exactly one
    /// member, so a key's (mass, members, confidence) only ever grows, strictly.
    /// The maximum of the total order over all keys can therefore be carried
    /// along: after each vote the peak is either the old peak or the key just
    /// voted for (if the old peak WAS that key, its new value beats its old
    /// one).  That is the same answer as scanning every live slot afterwards,
    /// without the scan or the list of live slots it needed.
    #[inline]
    pub(crate) fn add(&mut self, k: i32, mass: i64, conf: i64) {
        debug_assert!(mass >= 0 && conf >= 0 && mass < (1 << 24) && conf < (1 << 16));
        let mut s = self.slot(k);
        let e = self.epoch as u32;
        loop {
            let sl = &mut self.slots[s];
            if sl.tag & 0xffff != e {
                *sl = Slot { key: k, mass: mass as i32, conf: conf as i32, tag: e | (1 << 16) };
                break;
            }
            if sl.key == k {
                sl.mass += mass as i32;
                sl.conf += conf as i32;
                sl.tag += 1 << 16;
                break;
            }
            s = (s + 1) & self.mask;
        }
        let sl = &self.slots[s];
        let (m, n, c) = (sl.mass as i64, (sl.tag >> 16) as i32, sl.conf as i64);
        let better = m > self.bm
            || (m == self.bm
                && (n > self.bn || (n == self.bn && (c > self.bc || (c == self.bc && k < self.bk)))));
        if better {
            self.bk = k;
            self.bm = m;
            self.bn = n;
            self.bc = c;
        }
    }

    /// §6 — the peak, by an explicit total order rather than by whichever key
    /// the scan reached first: (mass, members, confidence) descending, then
    /// key ascending.
    ///
    /// Mass alone lets three overwhelming correspondences outrank a genuinely
    /// populated transformation, which is precisely the confusion 512 keypoints
    /// makes more likely rather than less.
    pub(crate) fn peak(&self) -> (i32, i64, i32) {
        if self.bm < 0 {
            (i32::MAX, 0, 0)
        } else {
            (self.bk, self.bm, self.bn)
        }
    }
}

impl Default for VoteTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Every buffer the geometry stage would otherwise allocate per verification,
/// owned once per comparison and reused.
///
/// The 4.1 shapes — a `HashMap` per verification, a `Vec` per correspondence,
/// a fresh mask and residual vector per inlier pass — were affordable at 256
/// keypoints.  At 512 they are the stage's dominant cost, and none of them
/// carries information between calls, so none of them needs to be new.
pub struct Scratch {
    table: VoteTable,
    boxes: Vec<CellBox>,
    members: Vec<(usize, usize, i32)>,
    inl: Vec<(usize, usize, i32)>,
    mask: Vec<bool>,
    res: Vec<i64>,
    /// B-side indices consumed, or excluded by §7, this round
    blocked: Vec<bool>,
    /// squared exclusion radius per pyramid level, computed once per pair
    excl_r2: Vec<i64>,
    /// `level_dim` of both sides for every level index, for the (mda, mdb)
    /// they were built for — one pair has one pair of frames
    levels: Option<(i64, i64, Box<[i64; 256]>, Box<[i64; 256]>)>,
}

impl Scratch {
    pub fn new() -> Scratch {
        Scratch {
            table: VoteTable::new(),
            boxes: Vec::new(),
            members: Vec::new(),
            inl: Vec::new(),
            mask: Vec::new(),
            res: Vec::new(),
            blocked: Vec::new(),
            excl_r2: Vec::new(),
            levels: None,
        }
    }

    fn level_tables(&mut self, mda: i64, mdb: i64) {
        match &self.levels {
            Some((a, b, _, _)) if *a == mda && *b == mdb => {}
            _ => {
                self.levels = Some((
                    mda,
                    mdb,
                    Box::new(crate::compare::level_table(mda)),
                    Box::new(crate::compare::level_table(mdb)),
                ))
            }
        }
    }
}

impl Default for Scratch {
    fn default() -> Self {
        Self::new()
    }
}

// ------------------------------------------------------------- §6 verification

pub(crate) struct Verify42 {
    pub inliers: i64,
    pub mask: Vec<bool>,
    pub model: Option<Model>,
    /// §6 — median squared residual over the inliers, in the fitting frame.
    /// Robust to a handful of bad matches in a way that a sum of squares is
    /// not, and the reason two models with equal inlier counts are no longer
    /// indistinguishable.
    pub median_err: i64,
    /// total vote weight carried by the inliers
    pub conf_sum: i64,
}

fn empty_verify(n: usize) -> Verify42 {
    Verify42 { inliers: 0, mask: vec![false; n], model: None, median_err: i64::MAX, conf_sum: 0 }
}


/// Squared residuals of a correspondence set under a model, in the >>4 frame
/// `fit_similarity` and `count_inliers` both work in.
pub(crate) fn residual2(a: &[Keypoint], b: &[Keypoint], c: &Corr42, m: &Model) -> i64 {
    let ax = (a[c.a].x >> 4) as i64;
    let ay = (a[c.a].y >> 4) as i64;
    let mx = ((m.r00 * ax - m.r10 * ay) >> 16) + m.tx;
    let my = ((m.r10 * ax + m.r00 * ay) >> 16) + m.ty;
    let dx = mx - (b[c.b].x >> 4) as i64;
    let dy = my - (b[c.b].y >> 4) as i64;
    dx * dx + dy * dy
}

/// The inlier test of `compare::count_inliers`, arithmetic for arithmetic, on
/// `Corr42` directly and into a caller-owned mask.  Avoids projecting the pool
/// into a fresh `Vec<(usize, usize, i32)>` twice per verification purely to
/// satisfy a signature.
pub(crate) fn count_inliers_into(
    a: &[Keypoint],
    b: &[Keypoint],
    corr: &[Corr42],
    m: &Model,
    tol2: i64,
    mask: &mut Vec<bool>,
) -> i64 {
    mask.clear();
    mask.resize(corr.len(), false);
    let mut n = 0i64;
    for (i, c) in corr.iter().enumerate() {
        let ax = (a[c.a].x >> 4) as i64;
        let ay = (a[c.a].y >> 4) as i64;
        let mx = ((m.r00 * ax - m.r10 * ay) >> 16) + m.tx;
        let my = ((m.r10 * ax + m.r00 * ay) >> 16) + m.ty;
        let dx = mx - (b[c.b].x >> 4) as i64;
        let dy = my - (b[c.b].y >> 4) as i64;
        if dx * dx + dy * dy <= tol2 {
            mask[i] = true;
            n += 1;
        }
    }
    n
}

/// §5–§6 — the weighted Hough, rebuilt.
///
/// Same shape as 4.1: soft bins, gather the winning cell's members, closed-form
/// fit, one loose pass, one refit, one tight count.  What moved is underneath —
/// the table, the vote weight, the peak comparison — plus the residual the
/// model now reports about itself, and, since M9, the fact that not one byte of
/// any of it is allocated here.
pub(crate) fn hough_verify_42(
    a: &[Keypoint],
    b: &[Keypoint],
    corr: &[Corr42],
    cfg: &Config,
    p: &Profile,
    mda: i64,
    mdb: i64,
    sc: &mut Scratch,
) -> Verify42 {
    if corr.len() < cfg.geo_min_corr {
        return empty_verify(corr.len());
    }
    let soft = p.scale_soft != 0;
    sc.level_tables(mda, mdb);
    let Scratch { table, boxes, members, inl, mask, res, levels, .. } = sc;
    let (_, _, lta, ltb) = levels.as_ref().unwrap();
    table.begin(corr.len());
    boxes.clear();
    for c in corr.iter() {
        let (ka, kb) = (&a[c.a], &b[c.b]);
        let bx = cell_box(ka, kb, lta[ka.level as usize], ltb[kb.level as usize], soft);
        let (w, cf) = (c.weight(), c.conf as i64);
        // the nearest scale bin carries w * 64 / 64 = w exactly
        bx.for_each(|k, kw| table.add(k, if kw == SK_NEAR { w } else { w * kw / SK_NEAR }, cf));
        boxes.push(bx);
    }
    let (best_key, _mass, _members) = table.peak();
    if best_key == i32::MAX {
        return empty_verify(corr.len());
    }
    let (bl, br, bx_, by) = key_digits(best_key);
    members.clear();
    for (i, c) in corr.iter().enumerate() {
        if boxes[i].has(bl, br, bx_, by) {
            members.push((c.a, c.b, c.d1));
        }
    }
    if (members.len() as i32) < p.min_peak_members {
        return empty_verify(corr.len());
    }
    let tol = (cfg.geo_eps as i64 >> 4).max(2);
    let tol2 = tol * tol;
    let mut m = match fit_similarity(a, b, members) {
        Some(m) => m,
        None => return empty_verify(corr.len()),
    };
    count_inliers_into(a, b, corr, &m, tol2 * 9 / 4, mask);
    inl.clear();
    for (i, c) in corr.iter().enumerate() {
        if mask[i] {
            inl.push((c.a, c.b, c.d1));
        }
    }
    if inl.len() >= 2 {
        if let Some(m2) = fit_similarity(a, b, inl) {
            m = m2;
        }
    }
    let n = count_inliers_into(a, b, corr, &m, tol2, mask);

    res.clear();
    let mut conf_sum = 0i64;
    for (i, c) in corr.iter().enumerate() {
        if mask[i] {
            res.push(residual2(a, b, c, &m));
            conf_sum += c.weight();
        }
    }
    res.sort_unstable();
    let median_err = if res.is_empty() { i64::MAX } else { res[res.len() >> 1] };
    Verify42 { inliers: n, mask: mask.clone(), model: Some(m), median_err, conf_sum }
}

// ------------------------------------------------------- §7 model extraction

#[derive(Clone, Copy, Debug)]
pub struct ModelRec42 {
    pub r00: i64,
    pub r10: i64,
    pub tx: i64,
    pub ty: i64,
    pub scale_q16: i64,
    pub mirror: bool,
    pub inliers: i64,
    /// §6 — median squared residual, the model's own account of how well it fits
    pub median_err: i64,
    pub conf_sum: i64,
}

#[derive(Clone, Debug)]
pub struct MultiModel42 {
    pub models: Vec<ModelRec42>,
    pub total_inliers: i64,
    pub corr_direct: usize,
    pub corr_mirror: usize,
    pub inlier_b: Vec<(i64, i64)>,
    /// A-side indices of every accepted model's inliers, for §8 diversity
    pub inlier_a: Vec<usize>,
    /// B-side pyramid levels of the same, for §8 scale diversity
    pub inlier_level: Vec<u8>,
    /// the better tight inlier count of round 0's two verifications, on the
    /// untouched pools — exactly what §A1's weak signal measures, so a
    /// measurement whose extraction accepted nothing reads it from here
    /// instead of verifying the same two pools again
    pub round0: Option<i64>,
}

#[allow(clippy::too_many_arguments)]
fn round42(
    a: &[Keypoint],
    b: &[Keypoint],
    pool: &[Corr42],
    cfg: &Config,
    p: &Profile,
    mda: i64,
    mdb: i64,
    sc: &mut Scratch,
) -> Verify42 {
    if pool.len() >= cfg.geo_min_corr {
        hough_verify_42(a, b, pool, cfg, p, mda, mdb, sc)
    } else {
        empty_verify(pool.len())
    }
}

/// §7 — the exclusion radius around a consumed keypoint, in the 16-bit frame.
///
/// A descriptor covers a 31 px patch AT ITS OWN LEVEL, so the footprint of the
/// structure a keypoint stands for is `PATCH_R / L_k` in normalised units.  A
/// second model that reaches into that footprint is not finding a second paste,
/// it is finding the first one again one keypoint over.
#[inline]
pub(crate) fn excl_radius(level: u8, mdb: i64, pct: i32) -> i64 {
    if pct <= 0 {
        return 0;
    }
    let l = level_dim(mdb, level as usize).max(1);
    clamp(65535 * PATCH_R * pct as i64 / (l * 100), 0, 65535)
}

/// §12.3 on prepared pools, with §7 consumption.  Shared body of the
/// measurement AND its control — §9.3 has not moved: a control that runs a
/// cheaper procedure than the thing it controls is not a control.
#[allow(clippy::too_many_arguments)]
pub fn extract_from_pools_42(
    a: &[Keypoint],
    am: &[Keypoint],
    b: &[Keypoint],
    mut pd: Vec<Corr42>,
    mut pm: Vec<Corr42>,
    cfg: &Config,
    p: &Profile,
    mda: i64,
    mdb: i64,
    sc: &mut Scratch,
) -> MultiModel42 {
    let (cd0, cm0) = (pd.len(), pm.len());
    let mut out = MultiModel42 {
        models: Vec::new(),
        total_inliers: 0,
        corr_direct: cd0,
        corr_mirror: cm0,
        inlier_b: Vec::new(),
        inlier_a: Vec::new(),
        inlier_level: Vec::new(),
        round0: None,
    };
    let min_model_inliers = p.min_model_inliers as i64;
    let max_models = p.max_models as usize;

    // §7 radii depend only on the level, so they are computed once per pair
    // rather than once per (pool entry x consumed keypoint) — which is where
    // they were, inside a loop that also re-walked `level_dim`.
    let pct = p.excl_pct;
    sc.excl_r2.clear();
    let maxlev = b.iter().map(|k| k.level as usize).max().unwrap_or(0);
    for lv in 0..=maxlev {
        let r = excl_radius(lv as u8, mdb, pct);
        sc.excl_r2.push(r * r);
    }

    for r in 0..max_models {
        let floor = if r == 0 { cfg.geo_min_corr as i64 } else { min_model_inliers };
        let vd = round42(a, b, &pd, cfg, p, mda, mdb, sc);
        let vm = round42(am, b, &pm, cfg, p, mda, mdb, sc);
        if r == 0 {
            out.round0 = Some(vd.inliers.max(vm.inliers));
        }
        // §6 — winner per round on (inliers, −median residual, confidence);
        // ties still go to the direct hypothesis, as in 4.1.
        let use_mirror = vm.inliers > vd.inliers
            || (vm.inliers == vd.inliers
                && vm.inliers > 0
                && (vm.median_err < vd.median_err
                    || (vm.median_err == vd.median_err && vm.conf_sum > vd.conf_sum)));
        let (v, pool) = if use_mirror { (vm, &pm) } else { (vd, &pd) };
        let Some(m) = v.model else { break };
        if v.inliers < floor {
            break;
        }

        // §7 — consumption by B-side keypoint across BOTH pools, plus the soft
        // exclusion neighbourhood.  Membership was a linear scan of a `Vec`
        // inside a `retain`, i.e. quadratic in the inlier count; it is now a
        // byte per B keypoint, marked once.
        sc.blocked.clear();
        sc.blocked.resize(b.len(), false);
        let mut consumed: Vec<usize> = Vec::new();
        for (i, c) in pool.iter().enumerate() {
            if v.mask[i] {
                if !sc.blocked[c.b] {
                    consumed.push(c.b);
                }
                sc.blocked[c.b] = true;
                out.inlier_b.push((b[c.b].x as i64, b[c.b].y as i64));
                // the A index is into `a` or `am`, which are the same keypoints
                // in the same order — §8 only asks what STRUCTURE was matched
                out.inlier_a.push(c.a);
                out.inlier_level.push(b[c.b].level);
            }
        }
        out.total_inliers += v.inliers;
        out.models.push(ModelRec42 {
            r00: m.r00,
            r10: m.r10,
            tx: m.tx,
            ty: m.ty,
            scale_q16: isqrt(m.r00 * m.r00 + m.r10 * m.r10),
            mirror: use_mirror,
            inliers: v.inliers,
            median_err: v.median_err,
            conf_sum: v.conf_sum,
        });

        if pct > 0 {
            for &j in consumed.iter() {
                let rr2 = sc.excl_r2[b[j].level as usize];
                if rr2 == 0 {
                    continue;
                }
                let (jx, jy) = (b[j].x as i64, b[j].y as i64);
                for (t, k) in b.iter().enumerate() {
                    if sc.blocked[t] {
                        continue;
                    }
                    let dx = k.x as i64 - jx;
                    let dy = k.y as i64 - jy;
                    if dx * dx + dy * dy <= rr2 {
                        sc.blocked[t] = true;
                    }
                }
            }
        }
        // This prunes the POOLS for later rounds; the underlying correspondence
        // lists are untouched, so the measurement and its control still see the
        // identical starting evidence.
        let blocked = &sc.blocked;
        pd.retain(|c| !blocked[c.b]);
        pm.retain(|c| !blocked[c.b]);
    }
    out
}

/// Convenience wrapper: build the two hypothesis pools and extract.
#[allow(clippy::too_many_arguments)]
pub fn extract_42(
    a: &[Keypoint],
    b: &[Keypoint],
    cfg: &Config,
    p: &Profile,
    xmax_a: i32,
    mda: i64,
    mdb: i64,
    sc: &mut Scratch,
) -> MultiModel42 {
    let am = mirror_side(a, xmax_a);
    let (ad, amd, bd) = (pack_all(a), pack_all(&am), pack_all(b));
    let pd = correspond_42(a, &ad, b, &bd, p);
    let pm = if cfg.mirror_hypothesis {
        correspond_42(&am, &amd, b, &bd, p)
    } else {
        Vec::new()
    };
    extract_from_pools_42(a, &am, b, pd, pm, cfg, p, mda, mdb, sc)
}

// -------------------------------------------------------------- §9 the nulls

fn shift42(c: &[Corr42], p: usize) -> Vec<Corr42> {
    let n = c.len();
    (0..n)
        .map(|i| Corr42 { b: c[(i + p) % n].b, ..c[i] })
        .collect()
}

fn reverse42(c: &[Corr42]) -> Vec<Corr42> {
    let n = c.len();
    (0..n).map(|i| Corr42 { b: c[n - 1 - i].b, ..c[i] }).collect()
}

fn shift_or_empty(c: &[Corr42], num: usize, den: usize) -> Vec<Corr42> {
    let n = c.len();
    if n == 0 {
        return Vec::new();
    }
    let p = n * num / den;
    if p == 0 || p >= n {
        return Vec::new();
    }
    shift42(c, p)
}

fn reverse_or_empty(c: &[Corr42]) -> Vec<Corr42> {
    if c.len() < 2 {
        Vec::new()
    } else {
        reverse42(c)
    }
}

/// §A1 — the weak signal, unchanged in substance: the best single verified
/// model over the two pools, member floor kept, acceptance floor absent.
#[allow(clippy::too_many_arguments)]
pub fn weak_inliers_42(
    a: &[Keypoint],
    am: &[Keypoint],
    b: &[Keypoint],
    pd: &[Corr42],
    pm: &[Corr42],
    cfg: &Config,
    p: &Profile,
    mda: i64,
    mdb: i64,
    sc: &mut Scratch,
) -> i64 {
    let vd = if pd.len() >= cfg.geo_min_corr {
        hough_verify_42(a, b, pd, cfg, p, mda, mdb, sc).inliers
    } else {
        0
    };
    let vm = if pm.len() >= cfg.geo_min_corr {
        hough_verify_42(am, b, pm, cfg, p, mda, mdb, sc).inliers
    } else {
        0
    };
    vd.max(vm)
}

/// §A1 measurement: extraction first, the weak signal only where the extraction
/// accepts nothing.
#[allow(clippy::too_many_arguments)]
pub fn geo_measure_42(
    a: &[Keypoint],
    am: &[Keypoint],
    b: &[Keypoint],
    pd: &[Corr42],
    pm: &[Corr42],
    cfg: &Config,
    p: &Profile,
    mda: i64,
    mdb: i64,
    sc: &mut Scratch,
) -> (MultiModel42, i64, i64) {
    let mm =
        extract_from_pools_42(a, am, b, pd.to_vec(), pm.to_vec(), cfg, p, mda, mdb, sc);
    if mm.total_inliers > 0 {
        let t = mm.total_inliers;
        (mm, t, 0)
    } else {
        // round 0 verified exactly these two pools, unpruned, with exactly
        // this procedure; only a profile with no rounds at all has to ask
        let w = match mm.round0 {
            Some(w) => w,
            None => weak_inliers_42(a, am, b, pd, pm, cfg, p, mda, mdb, sc),
        };
        (mm, w, w)
    }
}

/// §9.3 — the GN control: the same five geometric permutations, each running
/// the IDENTICAL measurement, aggregated by MAX.
///
/// The question the null answers has not changed and should not: how much
/// better is the real correspondence set than the best structured accident?
#[allow(clippy::too_many_arguments)]
pub fn gn_control_42(
    a: &[Keypoint],
    am: &[Keypoint],
    b: &[Keypoint],
    pd0: &[Corr42],
    pm0: &[Corr42],
    cfg: &Config,
    p: &Profile,
    mda: i64,
    mdb: i64,
    sc: &mut Scratch,
) -> (i64, &'static str) {
    let measure = |pd: &[Corr42], pm: &[Corr42], sc: &mut Scratch| -> i64 {
        geo_measure_42(a, am, b, pd, pm, cfg, p, mda, mdb, sc).1
    };
    let shifts: [(&'static str, usize, usize); 4] =
        [("gn:half", 1, 2), ("gn:third", 1, 3), ("gn:fifth", 1, 5), ("gn:twothirds", 2, 3)];
    let (mut best, mut member) = (0i64, "gn:none");
    for (name, num, den) in shifts {
        let pd = shift_or_empty(pd0, num, den);
        let pm = shift_or_empty(pm0, num, den);
        if pd.is_empty() && pm.is_empty() {
            continue;
        }
        let t = measure(&pd, &pm, sc);
        if t > best {
            best = t;
            member = name;
        }
    }
    let rd = reverse_or_empty(pd0);
    let rm = reverse_or_empty(pm0);
    if !(rd.is_empty() && rm.is_empty()) {
        let t = measure(&rd, &rm, sc);
        if t > best {
            best = t;
            member = "gn:reverse";
        }
    }
    (best, member)
}

// ----------------------------------------------------------- §8 diversity

/// §8 — how much INDEPENDENT evidence the geometry actually rests on.
///
/// This is the channel 512 keypoints make necessary rather than merely nice.
/// Doubling the budget doubles the matches a repeated texture can produce
/// without adding one independent observation, and inlier count alone cannot
/// tell those two situations apart.  Four readings, all in the 0..10000
/// currency, weighted exactly as the §3 selection score weights its four terms:
///
/// ```text
///     D = 40 spatial + 25 scale + 20 model + 15 descriptor
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diversity {
    pub spatial: i64,
    pub scale: i64,
    pub model: i64,
    pub descriptor: i64,
    pub combined: i64,
    /// the calibrated multiplier applied to the geometric evidence
    pub multiplier: i64,
}

pub fn diversity_42(
    mm: &MultiModel42,
    a: &[Keypoint],
    b: &[Keypoint],
    p: &Profile,
) -> (Diversity, Coverage) {
    let cov = coverage(&mm.inlier_b, p.grid_g);
    if mm.models.is_empty() {
        // No accepted model means no geometric evidence to modulate.  The
        // §A1 weak signal reaches the margin through a path that never
        // produced a model, and multiplying it down here would be punishing
        // the same absence twice.
        return (
            Diversity { multiplier: SCALE, ..Default::default() },
            cov,
        );
    }
    let spatial = cov.coverage;

    // scale: levels actually carrying inliers, against the levels B HAS
    let mut avail = [false; 256];
    for k in b.iter() {
        avail[k.level as usize] = true;
    }
    let navail = avail.iter().filter(|&&v| v).count().max(1) as i64;
    let mut used = [false; 256];
    for &l in mm.inlier_level.iter() {
        used[l as usize] = true;
    }
    let nused = used.iter().filter(|&&v| v).count() as i64;
    let scale = clamp(nused * SCALE / navail, 0, SCALE);

    let model = clamp(mm.models.len() as i64 * SCALE / (p.max_models as i64).max(1), 0, SCALE);

    // descriptor: greedy independent clusters over the inliers' A descriptors,
    // in extraction order.  A cluster head is a descriptor further than
    // DESC_NOVEL_AT from every head already found.
    let mut heads: Vec<Desc4> = Vec::new();
    for &ia in mm.inlier_a.iter() {
        if ia >= a.len() {
            continue;
        }
        let d = pack_desc(&a[ia].desc);
        if heads.iter().all(|h| hamming(h, &d) as i64 > DESC_NOVEL_AT) {
            heads.push(d);
        }
    }
    let descriptor = clamp(
        heads.len() as i64 * SCALE / (mm.inlier_a.len() as i64).max(1),
        0,
        SCALE,
    );

    let combined = idiv(40 * spatial + 25 * scale + 20 * model + 15 * descriptor, 100);
    let multiplier = p.lut_geo_diversity.eval(combined);
    (
        Diversity { spatial, scale, model, descriptor, combined, multiplier },
        cov,
    )
}

/// §13 — the five topology classes, unchanged.
pub fn topology_42(
    mm: &MultiModel42,
    cov: &Coverage,
    g: u8,
    dominant_at: i64,
    geo_min_corr: usize,
) -> u8 {
    if mm.models.len() >= 2 {
        3
    } else if mm.models.len() == 1 {
        if cov.bbox_cells * SCALE / ((g as i64) * (g as i64)) >= dominant_at {
            4
        } else {
            2
        }
    } else if mm.corr_direct.max(mm.corr_mirror) >= geo_min_corr {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multimodel::correspond_w;

    fn desc(seed: u64) -> [u32; 8] {
        let mut s = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_add(1);
        let mut d = [0u32; 8];
        for w in d.iter_mut() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            *w = (s >> 16) as u32;
        }
        d
    }
    fn kp(d: [u32; 8], x: i32, y: i32) -> Keypoint {
        Keypoint { desc: d, x, y, level: 0, sec: 0, s: 1024 }
    }
    fn prof_margin0() -> Profile {
        let mut p = Profile::cal004();
        p.lowe_margin = 0;
        p.scale_soft = 0;
        p
    }

    #[test]
    fn ham_cut_is_exact_below_the_limit() {
        for i in 0..64u64 {
            for j in 0..8u64 {
                let (x, y) = (pack_desc(&desc(i)), pack_desc(&desc(1000 + j)));
                let full = hamming(&x, &y);
                assert_eq!(ham_cut(&x, &y, 256), full);
                assert_eq!(ham_cut(&x, &y, full), full, "exact at the limit");
                assert!(ham_cut(&x, &y, full - 1) >= full.min(full), "abort only over");
                // whatever it returns when it aborts, it is strictly greater
                // than the limit — which is all the matcher relies on
                assert!(ham_cut(&x, &y, full - 1) > full - 1);
            }
        }
    }

    #[test]
    fn strength_compat_is_symmetric_and_floored() {
        assert_eq!(strength_compat(100, 100), 64);
        assert_eq!(strength_compat(100, 50), 32);
        assert_eq!(strength_compat(50, 100), 32);
        assert_eq!(strength_compat(1000, 1), 32, "floored at one half");
        assert_eq!(strength_compat(100, 75), 48);
        for a in [1u16, 7, 64, 1024, 65535] {
            for b in [1u16, 7, 64, 1024, 65535] {
                assert_eq!(strength_compat(a, b), strength_compat(b, a));
                assert!((32..=64).contains(&strength_compat(a, b)));
            }
        }
    }

    /// With the margin off, 42's matcher must find exactly 41's pairs — the
    /// early abort is an optimisation, not a policy.
    #[test]
    fn matcher_agrees_with_41_when_the_margin_is_off() {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in 0..60u64 {
            let d = desc(400 + i);
            a.push(kp(d, (i as i32) * 900, (i as i32) * 700));
            let mut d2 = d;
            if i % 3 == 0 {
                d2[0] ^= 7;
            }
            if i % 7 == 0 {
                d2[2] ^= 0x0f0f;
            }
            b.push(kp(d2, (i as i32) * 900 + 500, (i as i32) * 700 - 300));
        }
        let (ad, bd) = (pack_all(&a), pack_all(&b));
        let v41: Vec<(usize, usize, i32)> =
            correspond_w(&a, &b).iter().map(|c| (c.0, c.1, c.2)).collect();
        let v42: Vec<(usize, usize, i32)> = correspond_42(&a, &ad, &b, &bd, &prof_margin0())
            .iter()
            .map(|c| (c.a, c.b, c.d1))
            .collect();
        assert_eq!(v41, v42, "same pairs, same order");
    }

    /// The margin removes exactly the pairs whose separation is decorative:
    /// d1 = 2 against d2 = 3 passes the ratio at 0.667 and says nothing.
    #[test]
    fn absolute_margin_drops_thin_separations() {
        let mut p = prof_margin0();
        p.lowe_margin = 6;
        let (mut a, mut b) = (Vec::new(), Vec::new());
        // one clean pair (d1 = 0, d2 far) and one thin pair (d1 = 2, d2 = 3)
        let clean = desc(11);
        a.push(kp(clean, 1000, 1000));
        b.push(kp(clean, 2000, 1000));
        let base = desc(22);
        let mut near1 = base;
        near1[0] ^= 0b11; // 2 bits
        let mut near2 = base;
        near2[0] ^= 0b111; // 3 bits
        a.push(kp(base, 5000, 5000));
        b.push(kp(near1, 6000, 5000));
        b.push(kp(near2, 7000, 5000));
        let (ad, bd) = (pack_all(&a), pack_all(&b));
        let with = correspond_42(&a, &ad, &b, &bd, &p);
        let without = correspond_42(&a, &ad, &b, &bd, &prof_margin0());
        assert!(without.iter().any(|c| c.a == 1), "the thin pair passes the ratio");
        assert!(!with.iter().any(|c| c.a == 1), "and fails the margin");
        assert!(with.iter().any(|c| c.a == 0), "the clean pair survives both");
    }

    #[test]
    fn vote_table_peak_is_layout_independent() {
        let mut t = VoteTable::new();
        t.begin(64);
        t.add(9, 100, 10);
        t.add(3, 100, 10);
        t.add(3, 0, 5); // 3 now has two members, same mass
        let (k, m, n) = t.peak();
        assert_eq!((k, m, n), (3, 100, 2), "members break the mass tie");
        let mut u = VoteTable::new();
        u.begin(64);
        u.add(3, 100, 10);
        u.add(3, 0, 5);
        u.add(9, 100, 10);
        assert_eq!(u.peak(), t.peak(), "insertion order cannot matter");

        // and a REUSED table must behave as a fresh one: begin() is the only
        // clearing this design does, so if the epoch stamp leaked, the second
        // verification would inherit the first one's votes.
        let mut reused = VoteTable::new();
        reused.begin(64);
        reused.add(7, 5000, 1);
        let fresh_first = reused.peak();
        reused.begin(64);
        reused.add(3, 100, 10);
        reused.add(9, 100, 10);
        reused.add(3, 0, 5);
        assert_eq!(reused.peak(), t.peak(), "a reused table is a cleared table");
        assert_eq!(fresh_first, (7, 5000, 1));
        for _ in 0..300 {
            reused.begin(1024); // growth mid-life must not resurrect anything
        }
        reused.add(11, 1, 1);
        assert_eq!(reused.peak(), (11, 1, 1));
    }

    /// The engine's descriptor batch and the plain reference must agree bit
    /// for bit, taken candidates skipped and running minima kept.  An
    /// optimisation that changes one distance changes a verdict, so this is
    /// an equivalence test, not a smoke test.
    #[test]
    fn descriptor_batch_matches_pairwise() {
        use crate::keypoints::{hamming_min_into, hamming_min_into_scalar, pack4};
        let mut pack = Vec::new();
        for i in 0..257u64 {
            pack.push(pack4(&desc(i)));
        }
        let taken: Vec<bool> = (0..pack.len()).map(|j| j % 7 == 3).collect();
        for qi in [0usize, 1, 128, 256] {
            let mut got = vec![i32::MAX; pack.len()];
            let mut want = vec![i32::MAX; pack.len()];
            hamming_min_into(&pack[qi], &pack, &taken, &mut got);
            hamming_min_into_scalar(&pack[qi], &pack, &taken, &mut want);
            assert_eq!(got, want, "query {qi}");
            for j in 0..pack.len() {
                let d = hamming(&Desc4 { q: pack[qi] }, &Desc4 { q: pack[j] });
                assert_eq!(got[j], if taken[j] { i32::MAX } else { d }, "query {qi} candidate {j}");
            }
            // a second query only lowers the running minimum
            hamming_min_into(&pack[(qi + 1) % pack.len()], &pack, &taken, &mut got);
            hamming_min_into_scalar(&pack[(qi + 1) % pack.len()], &pack, &taken, &mut want);
            assert_eq!(got, want, "query {qi}, second pass");
        }
    }

    /// The bin box and its expansion are the same cell set: membership by
    /// digits equals membership in the expanded keys, for every key the box
    /// produces and for every key one digit away from them, hard and soft,
    /// at the edges of every axis.
    #[test]
    fn cell_box_membership_is_the_expanded_set() {
        let mut r = 0x00c0_ffeeu64;
        let mut next = || {
            r ^= r << 13;
            r ^= r >> 7;
            r ^= r << 17;
            r
        };
        for case in 0..3000 {
            let mut ka = kp(desc(case), (next() % 65536) as i32, (next() % 65536) as i32);
            let mut kb = kp(desc(case), (next() % 65536) as i32, (next() % 65536) as i32);
            ka.level = (next() % 6) as u8;
            kb.level = (next() % 6) as u8;
            ka.sec = (next() % 64) as u8;
            kb.sec = (next() % 64) as u8;
            if case % 7 == 0 {
                kb.x = 0;
                ka.x = 65535;
            }
            let soft = case % 3 != 0;
            let (la, lb) = (level_dim(256, ka.level as usize), level_dim(200, kb.level as usize));
            let bx = cell_box(&ka, &kb, la, lb, soft);
            let vc = vote_cells_42_l(&ka, &kb, la, lb, soft);
            let mut probes: Vec<i32> = vc.key[..vc.len as usize].to_vec();
            for &k in vc.key[..vc.len as usize].iter() {
                for d in [1, -1, 64, -64, 4096, -4096, 65536, -65536] {
                    probes.push(k + d);
                }
            }
            probes.push((next() % 1_638_400) as i32);
            for &k in probes.iter().filter(|&&k| (0..1_638_400).contains(&k)) {
                let (l, rr, x, y) = key_digits(k);
                assert_eq!(bx.has(l, rr, x, y), vc.contains(k), "case {case} key {k}");
            }
        }
    }

    /// The running peak is the scan it replaced: the maximum over every key's
    /// final (mass, members, confidence, -key), whatever the vote order.
    #[test]
    fn running_peak_equals_a_full_scan() {
        let mut r = 0x0bad_5eedu64;
        let mut next = || {
            r ^= r << 13;
            r ^= r >> 7;
            r ^= r << 17;
            r
        };
        let mut t = VoteTable::new();
        for case in 0..400 {
            let votes = 1 + (next() % 300) as usize;
            let keys = 1 + (next() % 40) as i64;
            t.begin(votes / 4 + 1);
            let mut want: std::collections::BTreeMap<i32, (i64, i32, i64)> = Default::default();
            for _ in 0..votes {
                let k = (next() % keys as u64) as i32 * 7919;
                // small masses force ties on mass, members and confidence
                let m = (next() % 3) as i64 * if case % 2 == 0 { 1 } else { 1000 };
                let c = (next() % 2) as i64;
                t.add(k, m, c);
                let e = want.entry(k).or_insert((0, 0, 0));
                e.0 += m;
                e.1 += 1;
                e.2 += c;
            }
            let best = want
                .iter()
                .max_by(|x, y| (x.1 .0, x.1 .1, x.1 .2, -x.0).cmp(&(y.1 .0, y.1 .1, y.1 .2, -y.0)))
                .unwrap();
            assert_eq!(t.peak(), (*best.0, best.1 .0, best.1 .1), "case {case}");
        }
    }

    /// The weak signal read from round 0 is the one `weak_inliers_42`
    /// measures, on pools where the extraction accepts nothing.
    #[test]
    fn weak_signal_from_round_zero_is_the_reverification() {
        let cfg = Config::default();
        let p = Profile::cal004();
        let (mut quiet, mut nonzero) = (0usize, 0usize);
        for seed in 0..40u64 {
            let (mut a, mut b) = (Vec::new(), Vec::new());
            let n = 8 + (seed % 23) as usize;
            for i in 0..n {
                let d = desc(seed * 100 + i as u64);
                let x = ((seed * 7919 + i as u64 * 104729) % 60000) as i32;
                let y = ((seed * 104729 + i as u64 * 7919) % 60000) as i32;
                a.push(kp(d, x, y));
                // a few coherent pairs, the rest scattered
                let (bx, by) = if i % 4 == 0 { (x / 2 + 3000, y / 2 + 1000) } else { ((x * 7) % 60000, (y * 13) % 60000) };
                b.push(kp(d, bx, by));
            }
            let am = mirror_side(&a, 65535);
            let (ad, amd, bd) = (pack_all(&a), pack_all(&am), pack_all(&b));
            let pd = correspond_42(&a, &ad, &b, &bd, &p);
            let pm = correspond_42(&am, &amd, &b, &bd, &p);
            let mut sc = Scratch::new();
            let (mm, w, weak) = geo_measure_42(&a, &am, &b, &pd, &pm, &cfg, &p, 256, 256, &mut sc);
            if mm.total_inliers == 0 {
                let again = weak_inliers_42(&a, &am, &b, &pd, &pm, &cfg, &p, 256, 256, &mut sc);
                assert_eq!((w, weak), (again, again), "seed {seed}");
                quiet += 1;
                nonzero += (again > 0) as usize;
            }
        }
        assert!(quiet >= 10 && nonzero >= 1, "the branch is exercised: {quiet} quiet, {nonzero} non-zero");
    }

    #[test]
    fn soft_scale_votes_three_bins_and_hard_votes_one() {
        let a = kp(desc(1), 10000, 10000);
        let mut b = kp(desc(1), 20000, 10000);
        b.level = 1;
        let hard = vote_cells_42(&a, &b, 256, 256, false);
        let soft = vote_cells_42(&a, &b, 256, 256, true);
        assert!(soft.len as usize >= hard.len as usize * 2);
        assert_eq!(soft.kw[0], 64, "the nearest bin keeps full weight");
        assert!(soft.key[..soft.len as usize].iter().any(|&k| hard.contains(k)));
    }

    /// Two pastes, one canvas: the extraction must still find both, and the
    /// control must still see nothing.
    #[test]
    fn collage_yields_two_models_and_a_quiet_control() {
        let cfg = Config::default();
        let p = Profile::cal004();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in 0..12 {
            let d = desc(1000 + i);
            let (x, y) = (6000 + (i as i32) * 2313 % 6000, 6000 + (i as i32) * 3517 % 6000);
            a.push(kp(d, x, y));
            b.push(kp(d, x + 9000, y + 2500));
        }
        for i in 0..10 {
            let d = desc(2000 + i);
            let (x, y) = (20000 + (i as i32) * 2401 % 6000, 20000 + (i as i32) * 3269 % 6000);
            a.push(kp(d, x, y));
            b.push(kp(d, x - 8000, y + 6000));
        }
        let mut sc = Scratch::new();
        let mm = extract_42(&a, &b, &cfg, &p, 65535, 256, 256, &mut sc);
        assert_eq!(mm.models.len(), 2, "two coherent pastes");
        assert!(mm.models.iter().all(|m| !m.mirror));
        assert_eq!(mm.inlier_a.len() as i64, mm.total_inliers);
        assert_eq!(mm.inlier_level.len() as i64, mm.total_inliers);
        for m in &mm.models {
            assert!(m.median_err < 100, "a real paste fits tightly: {}", m.median_err);
        }
        let am = mirror_side(&a, 65535);
        let (ad, amd, bd) = (pack_all(&a), pack_all(&am), pack_all(&b));
        let pd = correspond_42(&a, &ad, &b, &bd, &p);
        let pm = correspond_42(&am, &amd, &b, &bd, &p);
        let (ctl, member) = gn_control_42(&a, &am, &b, &pd, &pm, &cfg, &p, 256, 256, &mut sc);
        assert!(ctl < mm.total_inliers / 3, "ctl {} of {} ({})", ctl, mm.total_inliers, member);
        let (div, _cov) = diversity_42(&mm, &a, &b, &p);
        assert_eq!(div.model, SCALE / 2, "two of four models");
        assert!(div.descriptor > 9000, "22 unrelated descriptors are 22 structures");
        assert!(div.multiplier > 0);
    }

    /// §7 — the exclusion neighbourhood must not let one paste become two.
    #[test]
    fn exclusion_keeps_one_structure_one_model() {
        let cfg = Config::default();
        let mut p = Profile::cal004();
        p.min_model_inliers = 3;
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in 0..16 {
            let d = desc(5000 + i);
            let (x, y) = (20000 + (i as i32) * 137, 20000 + (i as i32) * 211);
            a.push(kp(d, x, y));
            b.push(kp(d, x + 4000, y + 4000));
        }
        let mut sc = Scratch::new();
        let off = extract_42(&a, &b, &cfg, &{ let mut q = p.clone(); q.excl_pct = 0; q }, 65535, 256, 256, &mut sc);
        let on = extract_42(&a, &b, &cfg, &p, 65535, 256, 256, &mut sc);
        assert!(!on.models.is_empty());
        assert!(on.models.len() <= off.models.len(), "exclusion never adds models");
    }

    #[test]
    fn diversity_is_zero_multiplied_only_where_a_model_exists() {
        let p = Profile::cal004();
        let empty = MultiModel42 {
            models: Vec::new(),
            total_inliers: 0,
            corr_direct: 0,
            corr_mirror: 0,
            inlier_b: Vec::new(),
            inlier_a: Vec::new(),
            inlier_level: Vec::new(),
            round0: None,
        };
        let (d, _) = diversity_42(&empty, &[], &[], &p);
        assert_eq!(d.multiplier, SCALE, "no model, no modulation");
        assert_eq!(d.combined, 0);
    }
}
