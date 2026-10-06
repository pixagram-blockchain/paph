//! Parse once, compare many — and measure once per pair.
//!
//! A comparison used to parse each side's Tier 1 and Tier 2 four times (the v3
//! reading inside comparator 42 parsed them, then comparator 42 parsed them
//! again), and to compute the 512 x 512 descriptor matrix four times: v3's
//! matcher and 4.2's, each for the direct and the mirrored hypothesis.
//!
//! Both matchers make the SAME scan — rows in order, columns in order, the
//! same "strictly smaller replaces, else strictly smaller than the second"
//! rule on both sides — and differ only in the thresholds they apply to its
//! result afterwards.  So the scan's result, `MatchState`, is computed once per
//! hypothesis and each matcher derives its own correspondences from it.
//!
//! `Prepared` is also the unit a search engine keeps hot: a query is prepared
//! once and compared against every candidate without re-parsing it.

use crate::calibration::Profile;
use crate::compare::{read_local, Bag};
use crate::config::clamp;
use crate::geom42::{conf, pack_desc, strength_compat, Corr42, Desc4};
use crate::keypoints::Keypoint;
use crate::simd::{hamming_row, scan_row, NONE16};
use crate::wire::{parse_t1, parse_t2, read_sketch, Tier1};

/// One side of a comparison, parsed and unpacked.
pub struct Prepared {
    pub t1: Tier1,
    /// keypoints: the Tier-2 list, or the Tier-1 sketch when Tier 2 is absent
    /// or does not parse
    pub kp: Vec<Keypoint>,
    /// the Tier-2 parse error, if one was given and refused.  Comparators
    /// treat it as CORRUPT, exactly as when they parsed it themselves; the
    /// stage-1 screen falls back to the sketch, exactly as it always did.
    pub t2_error: Option<&'static str>,
    /// SPEC-004.2 §3 — Tier-2 selection rule (0 for a sketch)
    pub select: i32,
    /// the v3 comparator's frame: Tier-2 header values when Tier 2 is present,
    /// otherwise Tier-1 values (see `compare::load` history)
    pub v3_max_dim: i64,
    pub v3_xmax: i32,
    /// packed descriptors, direct and mirrored (halves exchanged)
    pub desc: Vec<Desc4>,
    pub desc_m: Vec<Desc4>,
    /// the local-fingerprint bag
    pub bag: Bag,
    /// `kp` came from a Tier 2 that parsed (false: the Tier-1 sketch)
    pub tier2: bool,
}

impl Prepared {
    pub fn new(t1: &[u8], t2: Option<&[u8]>) -> Result<Prepared, &'static str> {
        let t1 = parse_t1(t1)?;
        Ok(match t2.map(parse_t2) {
            Some(Ok(p)) => Self::assemble(t1, p.list, None, p.select, p.max_dim, p.xmax, true),
            Some(Err(e)) => Self::sketch_only(t1, Some(e)),
            None => Self::sketch_only(t1, None),
        })
    }

    /// Tier 1 alone, or Tier 1 with its Tier 2 refused for `err`.
    ///
    /// Tier 1 still carries a 32-keypoint sketch.  The v3 frame for it is the
    /// Tier-1 maximum dimension, and the mirror axis is the last column in
    /// that frame — `(w-1)·65535/maxDim`, as the JavaScript reference has it.
    /// (The Rust reference used `(w-1)·65535` clamped, i.e. 65535, here: a
    /// mirrored sketch was reflected about the wrong axis, and only on this
    /// path.  Comparator 42 never read it; the v3 reading did.)
    pub fn sketch_only(t1: Tier1, err: Option<&'static str>) -> Prepared {
        let kp = read_sketch(&t1);
        let md = t1.max_dim();
        let xmax = clamp(crate::config::idiv((t1.width as i64 - 1) * 65535, md.max(1)), 0, 65535) as i32;
        Self::assemble(t1, kp, err, 0, md, xmax, false)
    }

    fn assemble(
        t1: Tier1,
        kp: Vec<Keypoint>,
        t2_error: Option<&'static str>,
        select: i32,
        v3_max_dim: i64,
        v3_xmax: i32,
        tier2: bool,
    ) -> Prepared {
        let desc: Vec<Desc4> = kp.iter().map(|k| pack_desc(&k.desc)).collect();
        let desc_m: Vec<Desc4> = desc.iter().map(|d| Desc4 { q: [d.q[2], d.q[3], d.q[0], d.q[1]] }).collect();
        let bag = read_local(&t1);
        Prepared { t1, kp, t2_error, select, v3_max_dim, v3_xmax, desc, desc_m, bag, tier2 }
    }

    /// The Tier-1 frame comparators 41 and 42 measure geometry in.
    pub fn t1_max_dim(&self) -> i64 {
        self.t1.max_dim()
    }

    pub fn t1_xmax(&self) -> i32 {
        crate::config::idiv((self.t1.width as i64 - 1) * 65535, self.t1.max_dim().max(1)).clamp(0, 65535) as i32
    }
}

/// P4 — `true` when `b` sorts before `a` by first differing Tier-1 byte.
pub fn canon_swapped(a: &Prepared, b: &Prepared) -> bool {
    match a.t1.bytes.iter().zip(b.t1.bytes.iter()).find(|(x, y)| x != y) {
        Some((x, y)) => x > y,
        None => false,
    }
}

/// The mutual best / second-best scan of one hypothesis.
///
/// `a_*[i]` hold row i's best distance, second-best distance and best column;
/// `b_*[j]` the same for column j.  Initial values are the reference's (999 and
/// "none"), which matter: a side with one keypoint keeps a second-best of 999.
/// Distances are at most 256 and indices below 65535, so everything is `u16`.
pub struct MatchState {
    pub a_best: Vec<u16>,
    pub a_d1: Vec<u16>,
    pub a_d2: Vec<u16>,
    pub b_best: Vec<u16>,
    pub b_d1: Vec<u16>,
    pub b_d2: Vec<u16>,
}

/// The scan both matchers made, once.  Per row: the distances, then
/// `simd::scan_row`, which settles the row's own best pair and advances every
/// column's, in row order — the order the reference's tie rule depends on.
pub fn match_state(a: &[Desc4], b: &[Desc4]) -> MatchState {
    let (na, nb) = (a.len(), b.len());
    let mut s = MatchState {
        a_best: vec![NONE16; na],
        a_d1: vec![999; na],
        a_d2: vec![999; na],
        b_best: vec![NONE16; nb],
        b_d1: vec![999; nb],
        b_d2: vec![999; nb],
    };
    if na == 0 || nb == 0 {
        return s;
    }
    let mut row = vec![0u16; nb];
    for i in 0..na {
        hamming_row(&a[i], b, &mut row);
        let (d1, d2, at) = scan_row(&row, i as u16, &mut s.b_d1, &mut s.b_d2, &mut s.b_best);
        s.a_d1[i] = d1;
        s.a_d2[i] = d2;
        s.a_best[i] = at;
    }
    s
}

/// v3's `correspond` on a precomputed scan: HAM_MAX 88, two-sided Lowe 0.82,
/// mutual best; sorted by (d1, a, b).
pub fn corr_v3(s: &MatchState) -> Vec<(usize, usize, i32)> {
    const HAM_MAX: i32 = 88;
    const LOWE_NUM: i32 = 82;
    const LOWE_DEN: i32 = 100;
    let mut out = Vec::new();
    for i in 0..s.a_best.len() {
        let j = s.a_best[i];
        let (ad1, ad2) = (s.a_d1[i] as i32, s.a_d2[i] as i32);
        if j == NONE16 || ad1 > HAM_MAX {
            continue;
        }
        let j = j as usize;
        let (bd1, bd2) = (s.b_d1[j] as i32, s.b_d2[j] as i32);
        if ad1 * LOWE_DEN >= LOWE_NUM * ad2 {
            continue;
        }
        if bd1 * LOWE_DEN >= LOWE_NUM * bd2 {
            continue;
        }
        if s.b_best[j] as usize != i {
            continue;
        }
        out.push((i, j, ad1));
    }
    out.sort_by(|p, q| p.2.cmp(&q.2).then(p.0.cmp(&q.0)).then(p.1.cmp(&q.1)));
    out
}

/// 4.2's `correspond_42` on a precomputed scan: the profile's ratio, maximum
/// and two-sided absolute margin, plus confidence and strength compatibility.
pub fn corr_42(s: &MatchState, a: &[Keypoint], b: &[Keypoint], p: &Profile) -> Vec<Corr42> {
    let (num, den, margin, hmax) = (p.lowe_num, p.lowe_den, p.lowe_margin, p.ham_max);
    let mut out = Vec::new();
    for i in 0..s.a_best.len() {
        let j = s.a_best[i];
        let (ad1, ad2) = (s.a_d1[i] as i32, s.a_d2[i] as i32);
        if j == NONE16 || ad1 > hmax {
            continue;
        }
        let j = j as usize;
        let (bd1, bd2) = (s.b_d1[j] as i32, s.b_d2[j] as i32);
        if ad1 * den >= num * ad2 {
            continue;
        }
        if bd1 * den >= num * bd2 {
            continue;
        }
        if s.b_best[j] as usize != i {
            continue;
        }
        if ad2 - ad1 < margin || bd2 - bd1 < margin {
            continue;
        }
        let d2 = ad2.min(bd2);
        out.push(Corr42 { a: i, b: j, d1: ad1, conf: conf(ad1, d2), sc: strength_compat(a[i].s, b[j].s) });
    }
    out.sort_by(|p, q| p.d1.cmp(&q.d1).then(p.a.cmp(&q.a)).then(p.b.cmp(&q.b)));
    out
}

/// The local-bag distances both local channels read.
///
/// v3's channel matches the bags greedily and controls with three rotations
/// of B's codes; 4.2's assigns them and controls with the same three plus a
/// bit reversal.  Every one of those matchings reads the same raw distances,
/// and every burst count reads the same self-distances — and the null
/// members' bursts EQUAL B's own, because a rotation or a reversal applied to
/// both codes preserves their distance.  So all of it is computed here once.
pub struct BagShare {
    /// row-major `n x n` / `m x m` self-distances
    pub dxx: Vec<u8>,
    pub dyy: Vec<u8>,
    /// row-major `n x m` distances of A's codes to B's, under B transformed
    /// by: identity, rot16, rot32, rot48, bit reversal
    pub d: [Vec<u8>; 5],
}

/// A bag's codes as `u64`, `hi` in the upper word — so the LN maps are
/// `rotate_left` and `reverse_bits`.
fn codes(b: &Bag) -> Vec<u64> {
    (0..b.n).map(|i| ((b.hi[i] as u64) << 32) | b.lo[i] as u64).collect()
}

fn dist(x: &[u64], y: &[u64]) -> Vec<u8> {
    let mut d = vec![0u8; x.len() * y.len()];
    for (i, &a) in x.iter().enumerate() {
        for (o, b) in d[i * y.len()..(i + 1) * y.len()].iter_mut().zip(y.iter()) {
            // `word`: keep the popcount scalar in the SIMD WebAssembly build
            *o = (a ^ crate::simd::word(b)).count_ones() as u8;
        }
    }
    d
}

impl BagShare {
    pub fn new(x: &Bag, y: &Bag) -> BagShare {
        let (cx, cy) = (codes(x), codes(y));
        let rot = |k: u32| -> Vec<u64> { cy.iter().map(|c| c.rotate_left(k)).collect() };
        let rev: Vec<u64> = cy.iter().map(|c| c.reverse_bits()).collect();
        BagShare {
            dxx: dist(&cx, &cx),
            dyy: dist(&cy, &cy),
            d: [dist(&cx, &cy), dist(&cx, &rot(16)), dist(&cx, &rot(32)), dist(&cx, &rot(48)), dist(&cx, &rev)],
        }
    }

    /// `burst` of v3 on a self-distance matrix: 1 + the codes within `t`.
    pub fn burst(dself: &[u8], n: usize, t: i32) -> Vec<i64> {
        (0..n)
            .map(|i| {
                1 + dself[i * n..(i + 1) * n]
                    .iter()
                    .enumerate()
                    .filter(|&(j, &d)| j != i && (d as i32) <= t)
                    .count() as i64
            })
            .collect()
    }
}

/// One pair in canonical order, with the scans computed on first use.
pub struct PairCtx<'a> {
    pub a: &'a Prepared,
    pub b: &'a Prepared,
    pub direct: Option<MatchState>,
    mirror: Option<MatchState>,
    bags: Option<BagShare>,
}

impl<'a> PairCtx<'a> {
    pub fn new(a: &'a Prepared, b: &'a Prepared) -> Self {
        PairCtx { a, b, direct: None, mirror: None, bags: None }
    }

    pub fn bags(&mut self) -> &BagShare {
        let (a, b) = (self.a, self.b);
        self.bags.get_or_insert_with(|| BagShare::new(&a.bag, &b.bag))
    }

    pub fn direct(&mut self) -> &MatchState {
        let (a, b) = (self.a, self.b);
        self.direct.get_or_insert_with(|| match_state(&a.desc, &b.desc))
    }

    pub fn mirror(&mut self) -> &MatchState {
        let (a, b) = (self.a, self.b);
        self.mirror.get_or_insert_with(|| match_state(&a.desc_m, &b.desc))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::{correspond, mirror_side};
    use crate::geom42::{correspond_42, pack_all};

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

    /// One scan, two derivations: each equals the matcher it replaces, pair
    /// for pair and order included, on near-duplicate, unrelated and
    /// degenerate keypoint sets, direct and mirrored.
    #[test]
    fn shared_scan_reproduces_both_matchers() {
        let p = Profile::cal004();
        for case in 0..60u64 {
            let na = [0usize, 1, 2, 17, 128, 300][(case % 6) as usize];
            let nb = [1usize, 0, 5, 64, 200, 511][((case / 6) % 6) as usize];
            let mut a = Vec::new();
            let mut b = Vec::new();
            for i in 0..na {
                let mut d = desc(case * 1000 + i as u64);
                if i % 5 == 0 {
                    d[1] = d[0]; // repeated words: ties
                }
                a.push(Keypoint { desc: d, x: (i * 97 % 65535) as i32, y: (i * 31 % 65535) as i32, level: (i % 4) as u8, sec: (i % 64) as u8, s: (i * 13 % 3000) as u16 });
            }
            for j in 0..nb {
                let d = if j < na && j % 3 != 0 {
                    let mut d = a[j].desc;
                    d[(j % 8) as usize] ^= 1 << (j % 32);
                    d
                } else {
                    desc(case * 7919 + 50_000 + j as u64)
                };
                b.push(Keypoint { desc: d, x: (j * 211 % 65535) as i32, y: (j * 17 % 65535) as i32, level: (j % 5) as u8, sec: (j % 64) as u8, s: (j * 7 % 2000) as u16 });
            }
            let (ad, bd) = (pack_all(&a), pack_all(&b));
            let s = match_state(&ad, &bd);
            assert_eq!(corr_v3(&s), correspond(&a, &b), "v3 direct, case {case}");
            assert_eq!(corr_42(&s, &a, &b, &p), correspond_42(&a, &ad, &b, &bd, &p), "42 direct, case {case}");
            let am = mirror_side(&a, 60000);
            let amd = pack_all(&am);
            let sm = match_state(&amd, &bd);
            assert_eq!(corr_v3(&sm), correspond(&am, &b), "v3 mirror, case {case}");
            assert_eq!(corr_42(&sm, &am, &b, &p), correspond_42(&am, &amd, &b, &bd, &p), "42 mirror, case {case}");
        }
    }
}
