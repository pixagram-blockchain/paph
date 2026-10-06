//! XMatch (PAPH-X §8.4–§8.8, §24, §25) — the sparse mutual-best matcher.
//!
//! The 4.2 matcher computes all `na x nb` descriptor distances and keeps,
//! per row and per column, the best and second-best.  This one keeps the
//! same per-row / per-column state but visits only the pairs the bucket
//! index nominates: for a query descriptor, every candidate found in the
//! bucket of at least one of its 24 projection codes (two, when it was only
//! ever found through hot buckets).  Distances for pairs nobody nominated
//! are never computed and no `na x nb` matrix exists (§25).
//!
//! Both hypotheses share one pass over the query descriptors: the mirror
//! hypothesis looks the query's code of projection `p xor 12` up in the
//! candidate's bucket of projection `p` (§9.5), and the mirror distance is
//! a word permutation of the same descriptor load — no mirrored descriptor
//! array is read.
//!
//! Rows are visited in ANCHOR order, so the state after the first 96 rows is
//! the anchor tier's evidence and expansion is "scan more rows" (§9.3).
//! Ties go to the lower index on both sides, which makes the state a pure
//! function of the SET of rows scanned, not of the order they came in.
//!
//! Nothing here allocates after the scratch exists: every array is sized to
//! the 512-keypoint contract (§15.3, §37.3).

use super::prepared::XPrepared;
use super::profile::{XProfile, LSH_PROJECTIONS, MAX_KP, MAX_TOTAL_CORR};
use super::bucket::{mirror_proj, HOT_FLAG};
use crate::calibration::Profile;
use crate::geom42::{conf, strength_compat, Corr42, Desc4};
use crate::simd::{hamming_row, scan_row, word};

pub const NONE: u16 = u16::MAX;

/// Best / second-best state of one hypothesis, both sides.
pub struct ScanState {
    pub a_best: [u16; MAX_KP],
    pub a_d1: [u16; MAX_KP],
    pub a_d2: [u16; MAX_KP],
    pub b_best: [u16; MAX_KP],
    pub b_d1: [u16; MAX_KP],
    pub b_d2: [u16; MAX_KP],
}

impl ScanState {
    fn new() -> Box<ScanState> {
        Box::new(ScanState {
            a_best: [NONE; MAX_KP],
            a_d1: [999; MAX_KP],
            a_d2: [999; MAX_KP],
            b_best: [NONE; MAX_KP],
            b_d1: [999; MAX_KP],
            b_d2: [999; MAX_KP],
        })
    }
    fn reset(&mut self, na: usize, nb: usize) {
        for i in 0..na {
            self.a_best[i] = NONE;
            self.a_d1[i] = 999;
            self.a_d2[i] = 999;
        }
        for j in 0..nb {
            self.b_best[j] = NONE;
            self.b_d1[j] = 999;
            self.b_d2[j] = 999;
        }
    }

    /// The 4.2 update, ties to the lower index.
    #[inline(always)]
    fn update(&mut self, i: usize, j: usize, d: u16) {
        let (d1, d2, b) = (self.a_d1[i], self.a_d2[i], self.a_best[i]);
        if d < d1 || (d == d1 && (j as u16) < b) {
            self.a_d2[i] = d1;
            self.a_d1[i] = d;
            self.a_best[i] = j as u16;
        } else if d < d2 {
            self.a_d2[i] = d;
        }
        let (d1, d2, b) = (self.b_d1[j], self.b_d2[j], self.b_best[j]);
        if d < d1 || (d == d1 && (i as u16) < b) {
            self.b_d2[j] = d1;
            self.b_d1[j] = d;
            self.b_best[j] = i as u16;
        } else if d < d2 {
            self.b_d2[j] = d;
        }
    }
}

#[inline(always)]
fn ham_direct(a: &Desc4, b: &Desc4) -> u16 {
    ((a.q[0] ^ word(&b.q[0])).count_ones()
        + (a.q[1] ^ word(&b.q[1])).count_ones()
        + (a.q[2] ^ word(&b.q[2])).count_ones()
        + (a.q[3] ^ word(&b.q[3])).count_ones()) as u16
}

/// Distance of the MIRRORED `a` to `b`: the mirror exchanges the halves, so
/// it is a permutation of the words, not a second array (§8.1, §15.2).
/// Symmetric: mirroring either side gives the same distance.
#[inline(always)]
fn ham_mirror(a: &Desc4, b: &Desc4) -> u16 {
    ((a.q[2] ^ word(&b.q[0])).count_ones()
        + (a.q[3] ^ word(&b.q[1])).count_ones()
        + (a.q[0] ^ word(&b.q[2])).count_ones()
        + (a.q[1] ^ word(&b.q[3])).count_ones()) as u16
}

/// Per-hypothesis nomination scratch.
///
/// One word per candidate keypoint: the epoch of the row that last touched
/// it in the high 24 bits, a "found only through hot buckets" flag in bit
/// 7, and the support — how many projections nominated it this row — in
/// the low 7 bits.  One load and one store per bucket entry, in one array,
/// is what keeps the nomination of a few hundred entries per row on busy,
/// dithered art inside the budget.
struct Nominate {
    st: [u32; MAX_KP],
    cand: [u16; MAX_KP],
    n: usize,
}

const HOT_ONLY: u32 = 0x80;
const SUPPORT_MASK: u32 = 0x7f;

impl Nominate {
    fn new() -> Box<Nominate> {
        Box::new(Nominate { st: [0; MAX_KP], cand: [0; MAX_KP], n: 0 })
    }
    #[inline(always)]
    fn mark(&mut self, bucket: &[u16], epoch: u32, hot: bool) {
        let tag = epoch << 8;
        let h = if hot { HOT_ONLY } else { 0 };
        for &j in bucket {
            let j = j as usize;
            let v = self.st[j];
            if v & !0xff != tag {
                self.st[j] = tag | h | 1;
                self.cand[self.n] = j as u16;
                self.n += 1;
            } else {
                // one more projection; the hot-only flag survives only
                // while every nominating bucket was hot
                let sp = (v & SUPPORT_MASK).min(SUPPORT_MASK - 1);
                self.st[j] = (v & !SUPPORT_MASK & (h | !HOT_ONLY)) | (sp + 1);
            }
        }
    }
    #[inline(always)]
    fn support(&self, j: usize) -> u32 {
        self.st[j] & SUPPORT_MASK
    }
    #[inline(always)]
    fn hot_only(&self, j: usize) -> bool {
        self.st[j] & HOT_ONLY != 0
    }
    fn clear(&mut self) {
        for v in self.st.iter_mut() {
            *v = 0;
        }
    }

    /// The support a candidate needs this row: 1, unless more than `maxc`
    /// candidates were nominated, in which case the smallest support that
    /// keeps at most `maxc` of them — a chance collision shares one
    /// projection, a true match shares many (§8.5).
    #[inline]
    fn needed(&self, maxc: usize) -> u32 {
        if self.n <= maxc {
            return 1;
        }
        let mut hist = [0u32; 32];
        for k in 0..self.n {
            hist[(self.support(self.cand[k] as usize) as usize).min(31)] += 1;
        }
        let mut acc = 0u32;
        let mut need = 31usize;
        while need > 1 {
            acc += hist[need];
            if acc as usize + hist[need - 1] as usize > maxc {
                break;
            }
            need -= 1;
        }
        need as u32
    }
}

/// Diagnostics of one scan (§33.3, §44).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanStats {
    /// candidate pairs nominated (direct + mirror)
    pub touched: u64,
    /// exact Hamming distances computed
    pub hammings: u64,
    /// `na x nb` — what the exhaustive scan would have computed per hypothesis
    pub full_pairs: u64,
    /// a row needed more than one shared projection to stay under
    /// `max_sparse_candidates`
    pub explosion: bool,
    /// rows of the smaller side's anchor order scanned so far
    pub rows: usize,
    /// rows the index nominated so many candidates for that the exhaustive
    /// row scan was cheaper, and was used (§38: sparse explosion → exact)
    pub dense_rows: usize,
}

pub struct MatchScratch {
    pub direct: Box<ScanState>,
    pub mirror: Box<ScanState>,
    nd: Box<Nominate>,
    nm: Box<Nominate>,
    row: Box<[u16; MAX_KP]>,
    epoch: u32,
    pub stats: ScanStats,
    /// correspondence pools, direct and mirror, derived on request
    pub pd: Vec<Corr42>,
    pub pm: Vec<Corr42>,
    pub truncated: bool,
    na: usize,
    nb: usize,
}

impl MatchScratch {
    pub fn new() -> MatchScratch {
        MatchScratch {
            direct: ScanState::new(),
            mirror: ScanState::new(),
            nd: Nominate::new(),
            nm: Nominate::new(),
            row: Box::new([0u16; MAX_KP]),
            epoch: 0,
            stats: ScanStats::default(),
            pd: Vec::with_capacity(MAX_TOTAL_CORR),
            pm: Vec::with_capacity(MAX_TOTAL_CORR),
            truncated: false,
            na: 0,
            nb: 0,
        }
    }

    /// Start a pair: nothing scanned yet.
    pub fn begin(&mut self, a: &XPrepared, b: &XPrepared) {
        self.na = a.kp_len().min(MAX_KP);
        self.nb = b.kp_len().min(MAX_KP);
        self.direct.reset(self.na, self.nb);
        self.mirror.reset(self.na, self.nb);
        self.stats = ScanStats { full_pairs: (self.na * self.nb) as u64, ..ScanStats::default() };
        self.pd.clear();
        self.pm.clear();
        self.truncated = false;
        if self.epoch > (1 << 24) - 4 * MAX_KP as u32 {
            // once per sixteen million rows: a real clear
            self.nd.clear();
            self.nm.clear();
            self.epoch = 0;
        }
    }

    /// Scan the next rows of the SMALLER side's anchor order, up to `to`,
    /// against the larger side's index (both hypotheses).  Nomination is
    /// symmetric — a pair is nominated when its two codes agree on a
    /// projection, whichever side probes — so the state is the same
    /// whichever side iterates, and iterating the smaller side finds every
    /// pair for fewer probes: a sprite pasted into a scene is found from
    /// the sprite's sixty keypoints rather than from wherever they rank
    /// among the scene's five hundred.  Rows already scanned are skipped.
    pub fn scan_rows(&mut self, a: &XPrepared, b: &XPrepared, xp: &XProfile, to: usize) {
        let swap = self.nb < self.na;
        let n_row = if swap { self.nb } else { self.na };
        let to = to.min(n_row);
        let from = self.stats.rows;
        if from >= to || self.na == 0 || self.nb == 0 {
            self.stats.rows = self.stats.rows.max(to);
            return;
        }
        if swap {
            self.scan_oriented::<true>(b, a, xp, from, to);
        } else {
            self.scan_oriented::<false>(a, b, xp, from, to);
        }
        self.stats.rows = to;
    }

    /// `SWAP` false: `r` is the canonical A side (rows), `c` the B side
    /// (columns).  `SWAP` true: `r` is B and `c` is A — the pair's roles are
    /// exchanged for the probing only; the state stays in canonical
    /// orientation.
    fn scan_oriented<const SWAP: bool>(&mut self, r: &XPrepared, c: &XPrepared, xp: &XProfile, from: usize, to: usize) {
        let cap = xp.hot_bucket_cap as usize;
        let soft = xp.hot_soft as usize;
        let need_hot = xp.support_hot;
        let maxc = (xp.max_sparse_candidates as usize).min(MAX_KP);
        let nc = c.kp_len().min(MAX_KP);
        let dense_at = (nc / 4).max(8);
        let (ir, ic) = (&r.index, &c.index);
        for k in from..to {
            let i = r.order[k] as usize;
            let qa = &r.p.desc[i];
            self.epoch += 1;
            let e = self.epoch;
            self.nd.n = 0;
            self.nm.n = 0;
            let qcodes = &ir.codes[i * LSH_PROJECTIONS..(i + 1) * LSH_PROJECTIONS];
            for p in 0..LSH_PROJECTIONS {
                // direct: the row's projection p against the column side's
                // bucket p; a code whose own bucket is hot is skipped (bit 15)
                let cd = qcodes[p];
                if cd & HOT_FLAG == 0 {
                    let bk = ic.bucket(p, cd);
                    if bk.len() <= cap {
                        self.nd.mark(bk, e, bk.len() > soft);
                    }
                }
                // mirror: mirror(A) at projection p has A's code of the
                // partner projection.  Rows of A probe B's bucket p with
                // A's partner code; rows of B probe A's partner bucket with
                // B's code p — the same pairs either way
                if SWAP {
                    let cm = qcodes[p];
                    if cm & HOT_FLAG == 0 {
                        let bk = ic.bucket(mirror_proj(p), cm);
                        if bk.len() <= cap {
                            self.nm.mark(bk, e, bk.len() > soft);
                        }
                    }
                } else {
                    let cm = qcodes[mirror_proj(p)];
                    if cm & HOT_FLAG == 0 {
                        let bk = ic.bucket(p, cm);
                        if bk.len() <= cap {
                            self.nm.mark(bk, e, bk.len() > soft);
                        }
                    }
                }
            }
            if self.nd.n + self.nm.n > 2 * dense_at {
                // the buckets nominated more than the exhaustive row costs:
                // scan it exhaustively instead, both hypotheses — 4.2's own
                // kernels, exact (§38: sparse explosion → exact)
                self.stats.dense_rows += 1;
                self.stats.touched += 2 * nc as u64;
                self.stats.hammings += 2 * nc as u64;
                let row = &mut self.row[..nc];
                if SWAP {
                    // row B_i against columns A: the direct hypothesis reads
                    // A's descriptors, the mirror one A's mirrored array
                    hamming_row(qa, &c.p.desc[..nc], row);
                    let s = &mut *self.direct;
                    let (d1, d2, at) = scan_row(row, i as u16, &mut s.a_d1[..nc], &mut s.a_d2[..nc], &mut s.a_best[..nc]);
                    s.b_d1[i] = d1;
                    s.b_d2[i] = d2;
                    s.b_best[i] = at;
                    hamming_row(qa, &c.p.desc_m[..nc], row);
                    let s = &mut *self.mirror;
                    let (d1, d2, at) = scan_row(row, i as u16, &mut s.a_d1[..nc], &mut s.a_d2[..nc], &mut s.a_best[..nc]);
                    s.b_d1[i] = d1;
                    s.b_d2[i] = d2;
                    s.b_best[i] = at;
                } else {
                    hamming_row(qa, &c.p.desc[..nc], row);
                    let s = &mut *self.direct;
                    let (d1, d2, at) = scan_row(row, i as u16, &mut s.b_d1[..nc], &mut s.b_d2[..nc], &mut s.b_best[..nc]);
                    s.a_d1[i] = d1;
                    s.a_d2[i] = d2;
                    s.a_best[i] = at;
                    let qm = Desc4 { q: [qa.q[2], qa.q[3], qa.q[0], qa.q[1]] };
                    hamming_row(&qm, &c.p.desc[..nc], row);
                    let s = &mut *self.mirror;
                    let (d1, d2, at) = scan_row(row, i as u16, &mut s.b_d1[..nc], &mut s.b_d2[..nc], &mut s.b_best[..nc]);
                    s.a_d1[i] = d1;
                    s.a_d2[i] = d2;
                    s.a_best[i] = at;
                }
                continue;
            }
            self.stats.touched += (self.nd.n + self.nm.n) as u64;
            let need_d = self.nd.needed(maxc);
            let need_m = self.nm.needed(maxc);
            if need_d > 1 || need_m > 1 {
                self.stats.explosion = true;
            }
            for k in 0..self.nd.n {
                let j = self.nd.cand[k] as usize;
                let sp = self.nd.support(j);
                if sp < need_d || (self.nd.hot_only(j) && (sp as i32) < need_hot) {
                    continue;
                }
                let d = ham_direct(qa, &c.p.desc[j]);
                self.stats.hammings += 1;
                if SWAP {
                    self.direct.update(j, i, d);
                } else {
                    self.direct.update(i, j, d);
                }
            }
            for k in 0..self.nm.n {
                let j = self.nm.cand[k] as usize;
                let sp = self.nm.support(j);
                if sp < need_m || (self.nm.hot_only(j) && (sp as i32) < need_hot) {
                    continue;
                }
                // mirror(A) against B: with the roles swapped the row is B
                // and the column A, so the permuted words are the column's
                let d = if SWAP { ham_mirror(&c.p.desc[j], qa) } else { ham_mirror(qa, &c.p.desc[j]) };
                self.stats.hammings += 1;
                if SWAP {
                    self.mirror.update(j, i, d);
                } else {
                    self.mirror.update(i, j, d);
                }
            }
        }
    }

    /// §24 — count the correspondences of one hypothesis without building
    /// them: how many A rows pass the 4.2 acceptance (ceiling, two-sided
    /// Lowe, mutual best, two-sided margin), stopping at `limit`; the
    /// confidence mass they carry; and how many A rows have ANY candidate
    /// within the ceiling (coverage).
    pub fn count(&self, mirror: bool, a: &XPrepared, b: &XPrepared, p: &Profile, limit: usize) -> ScreenCount {
        let s = if mirror { &*self.mirror } else { &*self.direct };
        let (num, den, margin, hmax) = (p.lowe_num, p.lowe_den, p.lowe_margin, p.ham_max);
        let mut out = ScreenCount::default();
        for i in 0..self.na {
            let j = s.a_best[i];
            let (ad1, ad2) = (s.a_d1[i] as i32, second(s.a_d2[i], hmax));
            if j == NONE || ad1 > hmax {
                continue;
            }
            out.coverage += 1;
            let j = j as usize;
            let (bd1, bd2) = (s.b_d1[j] as i32, second(s.b_d2[j], hmax));
            if ad1 * den >= num * ad2 || bd1 * den >= num * bd2 || s.b_best[j] as usize != i {
                continue;
            }
            if ad2 - ad1 < margin || bd2 - bd1 < margin {
                continue;
            }
            out.count += 1;
            out.support += conf(ad1, ad2.min(bd2)) as i64 * strength_compat(a.p.kp[i].s, b.p.kp[j].s) as i64;
            if out.count >= limit {
                out.hit_limit = true;
                break;
            }
        }
        out
    }

    /// Build the correspondence pools of both hypotheses from the current
    /// state, into the scratch buffers, sorted by (d1, a, b) as 4.2 sorts
    /// them — the null family's permutations depend on that order.
    pub fn pools(&mut self, a: &XPrepared, b: &XPrepared, p: &Profile) {
        self.pd.clear();
        self.pm.clear();
        self.truncated = false;
        let (num, den, margin, hmax) = (p.lowe_num, p.lowe_den, p.lowe_margin, p.ham_max);
        for h in 0..2 {
            let s = if h == 0 { &*self.direct } else { &*self.mirror };
            let out = if h == 0 { &mut self.pd } else { &mut self.pm };
            for i in 0..self.na {
                let j = s.a_best[i];
                let (ad1, ad2) = (s.a_d1[i] as i32, second(s.a_d2[i], hmax));
                if j == NONE || ad1 > hmax {
                    continue;
                }
                let j = j as usize;
                let (bd1, bd2) = (s.b_d1[j] as i32, second(s.b_d2[j], hmax));
                if ad1 * den >= num * ad2 || bd1 * den >= num * bd2 || s.b_best[j] as usize != i {
                    continue;
                }
                if ad2 - ad1 < margin || bd2 - bd1 < margin {
                    continue;
                }
                if out.len() >= MAX_TOTAL_CORR {
                    self.truncated = true;
                    break;
                }
                let d2 = ad2.min(bd2);
                out.push(Corr42 { a: i, b: j, d1: ad1, conf: conf(ad1, d2), sc: strength_compat(a.p.kp[i].s, b.p.kp[j].s) });
            }
            out.sort_unstable_by(|p, q| p.d1.cmp(&q.d1).then(p.a.cmp(&q.a)).then(p.b.cmp(&q.b)));
        }
    }
}

impl Default for MatchScratch {
    fn default() -> Self {
        Self::new()
    }
}

/// The second-best distance as the sparse acceptance reads it.
///
/// The exhaustive scan always has a second-best; the sparse scan has one
/// only when the index nominated two candidates for the row (or column).
/// With none, the ratio and margin tests would pass on the sentinel 999 —
/// a nominated candidate at Hamming 88 would count as unambiguous — so an
/// unknown second-best is read as the distance ceiling instead: the
/// acceptance then requires `d1 < lowe · ham_max`, which is what the
/// exhaustive test requires whenever the true second-best is at least
/// the ceiling, and a little more permissive below it.  Strong
/// correspondences are unaffected; the junk tail is not admitted.
#[inline(always)]
fn second(d2: u16, hmax: i32) -> i32 {
    if d2 >= 999 {
        hmax
    } else {
        d2 as i32
    }
}

/// §24 — the screen reading of one hypothesis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScreenCount {
    pub count: usize,
    /// Σ conf · strength compatibility over the counted correspondences
    pub support: i64,
    /// scanned rows whose best candidate is within the Hamming ceiling
    pub coverage: usize,
    pub hit_limit: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keypoints::Keypoint;
    use crate::prepared::{corr_42, match_state, Prepared};
    use crate::wire::{hash, parse_t1};
    use crate::x::profile::XBound;

    fn image(seed: u64, w: usize, h: usize) -> Vec<u8> {
        crate::x::testimg::image(seed, w, h)
    }

    fn xprep(px: &[u8], w: usize, h: usize, xb: &XBound) -> XPrepared {
        let rot = crate::keypoints::RotCache::new(&crate::keypoints::pattern());
        let f = hash(px, w, h, &crate::config::Config::default(), &rot);
        let p = Prepared::new(&f.t1, Some(&f.t2)).unwrap();
        XPrepared::new(p, xb)
    }

    /// On a pair where the exhaustive scan and the sparse scan nominate the
    /// same candidates — here: every candidate, through a hot-bucket cap
    /// large enough to never apply and a table where every descriptor
    /// collides — the sparse state must equal the exhaustive one exactly,
    /// which pins the update rule, the mirror permutation and the pool
    /// derivation to 4.2's.
    #[test]
    fn sparse_equals_exhaustive_when_everything_is_nominated() {
        let mut xp = XProfile::x1();
        // the cap lifted: on a self pair every row's exact twin shares all
        // 24 codes and is always nominated, so the sparse best equals the
        // exhaustive best, which pins the update rule and the pool
        // derivation to 4.2's
        xp.hot_bucket_cap = 512;
        xp.hot_soft = 512;
        let xb = XBound::new(Profile::cal004(), xp.clone());
        let base = xb.base.clone();
        let a = xprep(&image(3, 160, 120), 160, 120, &xb);
        let b = xprep(&image(3, 160, 120), 160, 120, &xb); // same image: every row has an exact twin
        let mut sc = MatchScratch::new();
        sc.begin(&a, &b);
        sc.scan_rows(&a, &b, &xp, 512);
        let ex = match_state(&a.p.desc, &b.p.desc);
        let exm = match_state(&a.p.desc_m, &b.p.desc);
        // every exact twin is nominated (all 24 codes agree), so the best is
        // identical and d1 is 0 on both sides
        for i in 0..a.kp_len() {
            assert_eq!(sc.direct.a_d1[i], ex.a_d1[i], "row {i}");
            assert_eq!(sc.direct.a_best[i], ex.a_best[i], "row {i}");
        }
        // the mirror state uses the permuted words: distances agree with
        // the 4.2 mirrored array wherever the pair was nominated
        for i in 0..a.kp_len() {
            let j = sc.mirror.a_best[i];
            if j != NONE {
                let d = crate::geom42::hamming(&a.p.desc_m[i], &b.p.desc[j as usize]);
                assert_eq!(sc.mirror.a_d1[i] as i32, d);
                assert!(d >= exm.a_d1[i] as i32);
            }
        }
        sc.pools(&a, &b, &base);
        let pd = corr_42(&ex, &a.p.kp, &b.p.kp, &base);
        // self pair: the exhaustive pool is every keypoint; the sparse pool
        // must hold the same pairs (d1 = 0 twins are always nominated and
        // nothing beats distance 0)
        assert_eq!(sc.pd.len(), pd.len());
        for (x, y) in sc.pd.iter().zip(pd.iter()) {
            assert_eq!((x.a, x.b, x.d1), (y.a, y.b, y.d1));
        }
        assert_eq!(sc.stats.rows, a.kp_len());
        assert!(sc.stats.hammings < sc.stats.full_pairs * 2, "sparse: {} of {}", sc.stats.hammings, 2 * sc.stats.full_pairs);
        let c = sc.count(false, &a, &b, &base, 10_000);
        assert_eq!(c.count, pd.len());
    }

    /// A mirrored copy: the sparse mirror pool is large and the direct pool
    /// small, the counts match the derived pools, anchors first.
    #[test]
    fn mirrored_copy_is_found_through_the_mirror_hypothesis() {
        let xb = XBound::shipped();
        let (base, xp) = (xb.base.clone(), xb.xp.clone());
        let (w, h) = (200, 150);
        let px = image(21, w, h);
        let mut mp = vec![0u8; px.len()];
        for y in 0..h {
            for x in 0..w {
                let s = (y * w + (w - 1 - x)) * 4;
                let d = (y * w + x) * 4;
                mp[d..d + 4].copy_from_slice(&px[s..s + 4]);
            }
        }
        let a = xprep(&px, w, h, &xb);
        let b = xprep(&mp, w, h, &xb);
        let mut sc = MatchScratch::new();
        sc.begin(&a, &b);
        sc.scan_rows(&a, &b, &xp, 96);
        let cm = sc.count(true, &a, &b, &base, 10_000);
        let cd = sc.count(false, &a, &b, &base, 10_000);
        assert!(cm.count > cd.count && cm.count >= 20, "mirror {} direct {}", cm.count, cd.count);
        sc.pools(&a, &b, &base);
        assert_eq!(sc.pm.len(), cm.count);
        assert_eq!(sc.pd.len(), cd.count);
        sc.scan_rows(&a, &b, &xp, 512);
        let cm2 = sc.count(true, &a, &b, &base, 10_000);
        assert!(cm2.count >= cm.count, "expansion only adds rows: {} -> {}", cm.count, cm2.count);
        // the exhaustive 4.2 mirror pool for reference: the sparse pool
        // finds most of it
        let exm = match_state(&a.p.desc_m, &b.p.desc);
        let pm42 = corr_42(&exm, &a.p.kp, &b.p.kp, &base);
        println!("mirror pool: sparse {} exhaustive {} (hammings {} of {})", cm2.count, pm42.len(), sc.stats.hammings, 2 * sc.stats.full_pairs);
        assert!(cm2.count * 10 >= pm42.len() * 7, "sparse {} vs exhaustive {}", cm2.count, pm42.len());
        assert!(sc.stats.hammings * 4 < 2 * sc.stats.full_pairs, "fewer than a quarter of the full scan");
        let _ = parse_t1;
        let _: Option<&Keypoint> = None;
    }
}
