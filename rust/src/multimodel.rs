//! Multi-model geometry (SPEC-004 §12.2–§12.3) and copy topology (§13).
//!
//! M3 completes the channel: correspondences carry a confidence derived from
//! the Lowe margin (§12.2), Hough votes are confidence-weighted and duplicated
//! across soft bins, models are extracted iteratively from two hypothesis
//! pools — direct and mirrored — with the winner chosen per round (per-model
//! mirror) and its inliers' B-side keypoints consumed from BOTH pools, and the
//! GN null family runs through this IDENTICAL procedure (§9.3): a control that
//! runs a cheaper procedure than the thing it controls is not a control.
//!
//! The v3 single-model path (`compare.rs`) stays untouched and unweighted; the
//! weighted machinery lives only here, on a parallel correspondence type that
//! carries the margin v3 computes and discards.

use crate::compare::{count_inliers, fit_similarity, mirror_side, vote_cells, Verify};
use crate::config::{idiv, isqrt, Config, SCALE};
use crate::coverage::{coverage, Coverage};
use crate::keypoints::Keypoint;
use crate::nulls::{apply_reverse4, apply_shift4};
use std::collections::HashMap;

/// A weighted correspondence: (a index, b index, best distance, confidence).
/// Confidence travels with the ROW under every null permutation — descriptor
/// pairing is preserved, only geometry is re-dealt (Appendix D).
pub type Corr4 = (usize, usize, i32, i32);

/// §12.2: conf = 1 + idiv((d2 − d1)·63, max(d2, 1)) ∈ 1..=64.  `d2` is the
/// SMALLER of the two sides' second-best distances, so ambiguity on either
/// side lowers confidence.
pub fn conf(d1: i32, d2: i32) -> i32 {
    1 + idiv((d2 as i64 - d1 as i64) * 63, (d2 as i64).max(1)) as i32
}

const HAM_MAX: i32 = 88;
const LOWE_NUM: i32 = 82;
const LOWE_DEN: i32 = 100;

fn ham(a: &[u32; 8], b: &[u32; 8]) -> i32 {
    let mut d = 0u32;
    for k in 0..8 {
        d += (a[k] ^ b[k]).count_ones();
    }
    d as i32
}

/// v3's `correspond`, keeping what it already computes: mutual-best pairs
/// under the Lowe ratio, now carrying (d1, conf) instead of discarding the
/// margins.  Projection onto (i, j, d1) is IDENTICAL to `correspond` — the
/// test suite asserts it, pair for pair, order included.
pub fn correspond_w(a: &[Keypoint], b: &[Keypoint]) -> Vec<Corr4> {
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
        let d2 = a_d2[i].min(b_d2[j]);
        out.push((i, j, a_d1[i], conf(a_d1[i], d2)));
    }
    out.sort_by(|p, q| p.2.cmp(&q.2).then(p.0.cmp(&q.0)).then(p.1.cmp(&q.1)));
    out
}

fn proj(corr: &[Corr4]) -> Vec<(usize, usize, i32)> {
    corr.iter().map(|c| (c.0, c.1, c.2)).collect()
}

/// §12.2: the weighted Hough.  Structure identical to v3's `hough_verify` —
/// same soft bins (votes DUPLICATED across them, not split), same peak tie
/// rule (smallest cell key), same loose-gather / refit / tight-count — with
/// two normative differences: each correspondence votes its confidence, and
/// the minimum-peak floor applies to the MEMBER COUNT (≥ 3 correspondences in
/// the winning cell), because the floor exists for the fit, and the fit
/// counts points, not confidence.
pub(crate) fn hough_verify_w(
    a: &[Keypoint],
    b: &[Keypoint],
    corr: &[Corr4],
    cfg: &Config,
    mda: i64,
    mdb: i64,
) -> Verify {
    let empty = Verify { inliers: 0, mask: vec![false; corr.len()], model: None };
    if corr.len() < cfg.geo_min_corr {
        return empty;
    }
    let mut votes: HashMap<i64, i64> = HashMap::new();
    let mut per: Vec<Vec<i64>> = Vec::with_capacity(corr.len());
    for c in corr.iter() {
        let cells = vote_cells(&a[c.0], &b[c.1], mda, mdb);
        for &k in cells.iter() {
            *votes.entry(k).or_insert(0) += c.3 as i64;
        }
        per.push(cells);
    }
    let (mut best_key, mut best_n) = (i64::MAX, 0i64);
    for (&k, &n) in votes.iter() {
        if n > best_n || (n == best_n && k < best_key) {
            best_n = n;
            best_key = k;
        }
    }
    let members: Vec<(usize, usize, i32)> = corr
        .iter()
        .enumerate()
        .filter(|(i, _)| per[*i].contains(&best_key))
        .map(|(_, c)| (c.0, c.1, c.2))
        .collect();
    if members.len() < 3 {
        return empty;
    }
    let tol = (cfg.geo_eps as i64 >> 4).max(2);
    let tol2 = tol * tol;
    let mut m = match fit_similarity(a, b, &members) {
        Some(m) => m,
        None => return empty,
    };
    let all = proj(corr);
    let (mask, _) = count_inliers(a, b, &all, &m, tol2 * 9 / 4);
    let inl: Vec<(usize, usize, i32)> =
        all.iter().enumerate().filter(|(i, _)| mask[*i]).map(|(_, c)| *c).collect();
    if inl.len() >= 2 {
        if let Some(m2) = fit_similarity(a, b, &inl) {
            m = m2;
        }
    }
    let (mask, n) = count_inliers(a, b, &all, &m, tol2);
    Verify { inliers: n, mask, model: Some(m) }
}

#[derive(Clone, Debug)]
pub struct ModelRec {
    pub r00: i64,
    pub r10: i64,
    pub tx: i64,
    pub ty: i64,
    /// isqrt(r00² + r10²), Q16 — exactly the scale v3 reports.
    pub scale_q16: i64,
    pub mirror: bool,
    pub inliers: i64,
}

#[derive(Clone, Debug)]
pub struct MultiModel {
    pub models: Vec<ModelRec>,
    pub total_inliers: i64,
    /// initial pool sizes, before any consumption
    pub corr_direct: usize,
    pub corr_mirror: usize,
    /// B-side coordinates (16-bit frame) of every accepted model's inliers
    pub inlier_b: Vec<(i64, i64)>,
}

fn round(
    a: &[Keypoint],
    b: &[Keypoint],
    pool: &[Corr4],
    cfg: &Config,
    mda: i64,
    mdb: i64,
) -> Verify {
    if pool.len() >= cfg.geo_min_corr {
        hough_verify_w(a, b, pool, cfg, mda, mdb)
    } else {
        Verify { inliers: 0, mask: vec![false; pool.len()], model: None }
    }
}

/// §12.3 on prepared pools — the shared body of the measurement AND its
/// control (§9.3: the null family runs the identical procedure, pools and
/// all).
pub fn extract_from_pools(
    a: &[Keypoint],
    am: &[Keypoint],
    b: &[Keypoint],
    mut pd: Vec<Corr4>,
    mut pm: Vec<Corr4>,
    cfg: &Config,
    mda: i64,
    mdb: i64,
    min_model_inliers: i64,
    max_models: usize,
) -> MultiModel {
    let (cd0, cm0) = (pd.len(), pm.len());
    let mut out = MultiModel {
        models: Vec::new(),
        total_inliers: 0,
        corr_direct: cd0,
        corr_mirror: cm0,
        inlier_b: Vec::new(),
    };
    for r in 0..max_models {
        let floor = if r == 0 { cfg.geo_min_corr as i64 } else { min_model_inliers };
        let vd = round(a, b, &pd, cfg, mda, mdb);
        let vm = round(am, b, &pm, cfg, mda, mdb);
        // winner per round; ties → direct (§12.3)
        let use_mirror = vm.inliers > vd.inliers;
        let (v, pool) = if use_mirror { (vm, &pm) } else { (vd, &pd) };
        let Some(m) = v.model else { break };
        if v.inliers < floor {
            break;
        }
        let mut consumed: Vec<usize> = Vec::new();
        for (i, c) in pool.iter().enumerate() {
            if v.mask[i] {
                consumed.push(c.1);
                out.inlier_b.push((b[c.1].x as i64, b[c.1].y as i64));
            }
        }
        out.total_inliers += v.inliers;
        out.models.push(ModelRec {
            r00: m.r00,
            r10: m.r10,
            tx: m.tx,
            ty: m.ty,
            scale_q16: isqrt(m.r00 * m.r00 + m.r10 * m.r10),
            mirror: use_mirror,
            inliers: v.inliers,
        });
        // consumption is by B-side keypoint, across BOTH pools
        pd.retain(|c| !consumed.contains(&c.1));
        pm.retain(|c| !consumed.contains(&c.1));
    }
    out
}

/// Convenience wrapper: builds the two hypothesis pools and extracts.
pub fn extract(
    a: &[Keypoint],
    b: &[Keypoint],
    cfg: &Config,
    xmax_a: i32,
    mda: i64,
    mdb: i64,
    min_model_inliers: i64,
    max_models: usize,
) -> MultiModel {
    let am = mirror_side(a, xmax_a);
    let pd = correspond_w(a, b);
    let pm = if cfg.mirror_hypothesis { correspond_w(&am, b) } else { Vec::new() };
    extract_from_pools(a, &am, b, pd, pm, cfg, mda, mdb, min_model_inliers, max_models)
}

fn shift_or_empty(c: &[Corr4], num: usize, den: usize) -> Vec<Corr4> {
    let n = c.len();
    if n == 0 {
        return Vec::new();
    }
    let p = n * num / den;
    if p == 0 || p >= n {
        // a degenerate permutation cannot serve as a null, and an unpermuted
        // pool would leak the measurement into its own control — the pool
        // sits this member out (§9.3).
        return Vec::new();
    }
    apply_shift4(c, p)
}

fn reverse_or_empty(c: &[Corr4]) -> Vec<Corr4> {
    if c.len() < 2 {
        return Vec::new();
    }
    apply_reverse4(c)
}

/// §9.3 through §12.3: the GN control.  Each family member permutes BOTH
/// pools (each from its own length) and runs the full multi-model extraction;
/// the control is the family maximum of total inliers.  Duplicate offsets
/// between members are permitted — under MAX they cost compute, never
/// correctness.
pub fn gn_control(
    a: &[Keypoint],
    am: &[Keypoint],
    b: &[Keypoint],
    pd0: &[Corr4],
    pm0: &[Corr4],
    cfg: &Config,
    mda: i64,
    mdb: i64,
    min_model_inliers: i64,
    max_models: usize,
) -> (i64, &'static str) {
    let shifts: [(&'static str, usize, usize); 4] =
        [("gn:half", 1, 2), ("gn:third", 1, 3), ("gn:fifth", 1, 5), ("gn:twothirds", 2, 3)];
    let (mut best, mut member) = (0i64, "gn:none");
    for (name, num, den) in shifts {
        let pd = shift_or_empty(pd0, num, den);
        let pm = shift_or_empty(pm0, num, den);
        if pd.is_empty() && pm.is_empty() {
            continue;
        }
        let t = extract_from_pools(a, am, b, pd, pm, cfg, mda, mdb, min_model_inliers, max_models)
            .total_inliers;
        if t > best {
            best = t;
            member = name;
        }
    }
    let pd = reverse_or_empty(pd0);
    let pm = reverse_or_empty(pm0);
    if !(pd.is_empty() && pm.is_empty()) {
        let t = extract_from_pools(a, am, b, pd, pm, cfg, mda, mdb, min_model_inliers, max_models)
            .total_inliers;
        if t > best {
            best = t;
            member = "gn:reverse";
        }
    }
    (best, member)
}

/// SPEC-004.1 §A1 — the weak signal: the best single verified model's inlier
/// count over the two pools, member floor (≥ 3, the fit's requirement) kept,
/// the model-ACCEPTANCE floor absent.  Consulted only when the extraction
/// accepts no model; no model means topology stays scattered and R3 keeps
/// this away from solo certification.
pub fn weak_inliers(
    a: &[Keypoint],
    am: &[Keypoint],
    b: &[Keypoint],
    pd: &[Corr4],
    pm: &[Corr4],
    cfg: &Config,
    mda: i64,
    mdb: i64,
) -> i64 {
    let vd = if pd.len() >= cfg.geo_min_corr {
        hough_verify_w(a, b, pd, cfg, mda, mdb).inliers
    } else {
        0
    };
    let vm = if pm.len() >= cfg.geo_min_corr {
        hough_verify_w(am, b, pm, cfg, mda, mdb).inliers
    } else {
        0
    };
    vd.max(vm)
}

/// §A1 measurement: extraction first; the weak signal only where the
/// extraction accepts nothing.  Returns (extracted models, measure,
/// weak_used) — `measure` feeds the margin, `weak_used` is reported.
pub fn geo_measure_41(
    a: &[Keypoint],
    am: &[Keypoint],
    b: &[Keypoint],
    pd: &[Corr4],
    pm: &[Corr4],
    cfg: &Config,
    mda: i64,
    mdb: i64,
    min_model_inliers: i64,
    max_models: usize,
) -> (MultiModel, i64, i64) {
    let mm = extract_from_pools(
        a, am, b, pd.to_vec(), pm.to_vec(), cfg, mda, mdb, min_model_inliers, max_models,
    );
    if mm.total_inliers > 0 {
        let t = mm.total_inliers;
        (mm, t, 0)
    } else {
        let w = weak_inliers(a, am, b, pd, pm, cfg, mda, mdb);
        (mm, w, w)
    }
}

/// §A1 control: every GN member runs the IDENTICAL measurement —
/// extraction with weak fallback — on its permuted pools.
pub fn gn_control_41(
    a: &[Keypoint],
    am: &[Keypoint],
    b: &[Keypoint],
    pd0: &[Corr4],
    pm0: &[Corr4],
    cfg: &Config,
    mda: i64,
    mdb: i64,
    min_model_inliers: i64,
    max_models: usize,
) -> (i64, &'static str) {
    let measure = |pd: &[Corr4], pm: &[Corr4]| -> i64 {
        geo_measure_41(a, am, b, pd, pm, cfg, mda, mdb, min_model_inliers, max_models).1
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
        let t = measure(&pd, &pm);
        if t > best {
            best = t;
            member = name;
        }
    }
    let rd = reverse_or_empty(pd0);
    let rm = reverse_or_empty(pm0);
    if !(rd.is_empty() && rm.is_empty()) {
        let t = measure(&rd, &rm);
        if t > best {
            best = t;
            member = "gn:reverse";
        }
    }
    (best, member)
}

/// §13 — the five topology classes, from the accepted models and the grid of
/// their inlier B-positions.
pub fn topology(mm: &MultiModel, g: u8, dominant_at: i64, geo_min_corr: usize) -> (u8, Coverage) {
    let cov = coverage(&mm.inlier_b, g);
    let class = if mm.models.len() >= 2 {
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
    };
    (class, cov)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::{correspond, hough_verify};

    /// Deterministic 256-bit descriptor stream, pairwise-far with overwhelming
    /// probability at this count; partners share descriptors exactly (d1 = 0),
    /// so the Lowe ratio and mutual-best checks pass by construction.
    fn desc(seed: u64) -> [u32; 8] {
        let mut s = seed.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(1);
        let mut d = [0u32; 8];
        for w in d.iter_mut() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            *w = (s >> 16) as u32;
        }
        d
    }

    fn kp(desc: [u32; 8], x: i32, y: i32) -> Keypoint {
        Keypoint { desc, x, y, level: 0, sec: 0, s: 0 }
    }

    fn cfg() -> Config {
        Config::default()
    }

    #[test]
    fn conf_bounds_and_monotonicity() {
        assert_eq!(conf(0, 999), 64);
        assert_eq!(conf(0, 1), 64);
        assert_eq!(conf(10, 10), 1);
        assert_eq!(conf(0, 2), 64);
        assert_eq!(conf(1, 2), 32);
        assert_eq!(conf(40, 88), 35);
        for d1 in 0..=88 {
            for d2 in d1..=256 {
                let c = conf(d1, d2);
                assert!((1..=64).contains(&c), "conf({d1},{d2}) = {c}");
                if d2 > d1 {
                    assert!(conf(d1, d2) >= conf(d1 + 1, d2), "monotone in d1");
                }
            }
        }
    }

    #[test]
    fn correspond_w_projects_onto_correspond() {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in 0..30u64 {
            let d = desc(400 + i);
            a.push(kp(d, (i as i32) * 2000, (i as i32) * 1500));
            let mut d2 = d;
            if i % 3 == 0 {
                d2[0] ^= 7; // a few imperfect partners: d1 = 3
            }
            b.push(kp(d2, (i as i32) * 2000 + 500, (i as i32) * 1500 - 300));
        }
        let v3 = correspond(&a, &b);
        let v4: Vec<(usize, usize, i32)> = correspond_w(&a, &b).iter().map(|c| (c.0, c.1, c.2)).collect();
        assert_eq!(v3, v4, "same pairs, same order — only the margin is new");
        for c in correspond_w(&a, &b) {
            assert!((1..=64).contains(&c.3));
        }
    }

    /// The §12.2 demonstration: three unambiguous correspondences agreeing on
    /// one placement outvote four barely-won ones agreeing on another.  The
    /// unweighted v3 Hough picks the count; the weighted v4 Hough picks the
    /// confidence.
    #[test]
    fn weighted_peak_beats_count() {
        let mut c = cfg();
        c.geo_min_corr = 4;
        let (mut a, mut b) = (Vec::new(), Vec::new());
        // cluster X: 3 pairs, translation (+20000, 0), conf 64.  Translations
        // sit ≥ 4 translation bins apart (TBIN_W = 4096), so Lowe soft
        // binning cannot merge the two peaks.
        for i in 0..3 {
            let d = desc(100 + i);
            let (x, y) = (10000 + (i as i32) * 4000, 12000 + (i as i32) * 3000);
            a.push(kp(d, x, y));
            b.push(kp(d, x + 20000, y));
        }
        // cluster Y: 4 pairs, translation (0, +20000), conf 1 (hand-set below)
        for i in 0..4 {
            let d = desc(200 + i);
            let (x, y) = (30000 + (i as i32) * 3500, 5000 + (i as i32) * 2800);
            a.push(kp(d, x, y));
            b.push(kp(d, x, y + 20000));
        }
        // hand-build the weighted pool: confidences 64/64/64 and 1/1/1/1
        let pool: Vec<Corr4> = (0..7).map(|i| (i, i, 0, if i < 3 { 64 } else { 1 })).collect();
        let vw = hough_verify_w(&a, &b, &pool, &c, 256, 256);
        let v3 = hough_verify(&a, &b, &proj(&pool), &c, 256, 256);
        assert_eq!(v3.inliers, 4, "unweighted follows the count");
        assert_eq!(vw.inliers, 3, "weighted follows the confidence");
        let mw = vw.model.unwrap();
        let m3 = v3.model.unwrap();
        assert!(mw.tx > m3.tx, "the two peaks are different placements");
        assert!((0..3).all(|i| vw.mask[i]) && !(3..7).any(|i| vw.mask[i]));
    }

    /// Two translated clusters plus scattered noise: the loop must find two
    /// models, consume each paste once, and classify topology 3.
    #[test]
    fn collage_yields_two_models() {
        let c = cfg();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        // cluster 1: 10 points spread over ~6000², pasted at +(9000, 2500)
        for i in 0..10 {
            let d = desc(1000 + i);
            let (x, y) = (6000 + (i as i32) * 2313 % 6000, 6000 + (i as i32) * 3517 % 6000);
            a.push(kp(d, x, y));
            b.push(kp(d, x + 9000, y + 2500));
        }
        // cluster 2: 8 points spread over ~6000², pasted at (−8000, +6000).
        // Centres are close and the translations differ sharply, so one
        // similarity absorbing both would leave within-cluster residuals far
        // beyond geo_eps — the two pastes are genuinely distinct models.
        for i in 0..8 {
            let d = desc(2000 + i);
            let (x, y) = (20000 + (i as i32) * 2401 % 6000, 20000 + (i as i32) * 3269 % 6000);
            a.push(kp(d, x, y));
            b.push(kp(d, x - 8000, y + 6000));
        }
        // noise: 6 matched pairs with incoherent placement
        for i in 0..6 {
            let d = desc(3000 + i);
            a.push(kp(d, 3000 + (i as i32) * 9001 % 60000, 60000 - (i as i32) * 7013 % 55000));
            b.push(kp(d, 61000 - (i as i32) * 8837 % 58000, 2000 + (i as i32) * 6151 % 59000));
        }
        let mm = extract(&a, &b, &c, 65535, 256, 256, 6, 4);
        assert_eq!(mm.corr_direct, 24, "all pairs pass the ratio test");
        assert_eq!(mm.models.len(), 2, "two coherent pastes");
        assert!(mm.models[0].inliers >= 10);
        assert!(mm.models[1].inliers >= 8);
        assert!(mm.models[0].inliers >= mm.models[1].inliers, "largest first");
        assert!(mm.models.iter().all(|m| !m.mirror));
        assert_eq!(mm.total_inliers, mm.models.iter().map(|m| m.inliers).sum::<i64>());
        assert_eq!(mm.inlier_b.len() as i64, mm.total_inliers);
        for m in &mm.models {
            assert!((m.scale_q16 - 65536).abs() < 3000, "scale_q16 = {}", m.scale_q16);
        }
        let (topo, _cov) = topology(&mm, 4, 6000, c.geo_min_corr);
        assert_eq!(topo, 3);

        // §9.3: the control, run through the same procedure, sees almost
        // nothing — permuted pools cannot agree on a placement.
        let am = mirror_side(&a, 65535);
        let pd = correspond_w(&a, &b);
        let pm = correspond_w(&am, &b);
        let (ctl, member) = gn_control(&a, &am, &b, &pd, &pm, &c, 256, 256, 6, 4);
        assert!(ctl < mm.total_inliers / 3, "ctl {} vs total {} ({})", ctl, mm.total_inliers, member);
    }

    /// One tight cluster: one model, small footprint, topology 2; the same
    /// correspondences spread over the canvas would be topology 4.
    #[test]
    fn single_region_vs_dominant() {
        let c = cfg();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in 0..12 {
            let d = desc(500 + i);
            let (x, y) = (20000 + (i as i32) * 199 % 900, 30000 + (i as i32) * 331 % 900);
            a.push(kp(d, x, y));
            b.push(kp(d, x + 1500, y - 700));
        }
        let mm = extract(&a, &b, &c, 65535, 256, 256, 6, 4);
        assert_eq!(mm.models.len(), 1);
        let (topo, cov) = topology(&mm, 4, 6000, c.geo_min_corr);
        assert_eq!(topo, 2, "tight paste is a region, not the canvas");
        assert!(cov.bbox_cells <= 4);

        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in 0..12 {
            let d = desc(700 + i);
            let (x, y) = ((i as i32) * 5077 % 60000 + 2000, (i as i32) * 4931 % 60000 + 2000);
            a.push(kp(d, x, y));
            b.push(kp(d, x + 1500, y - 700));
        }
        let mm = extract(&a, &b, &c, 65535, 256, 256, 6, 4);
        assert_eq!(mm.models.len(), 1);
        let (topo, _) = topology(&mm, 4, 6000, c.geo_min_corr);
        assert_eq!(topo, 4, "canvas-wide agreement is dominant");
    }

    /// §A1: five coherent correspondences among nine — the extraction's
    /// acceptance floor (8) reports zero, the weak signal reports five, and
    /// the GN family sees nothing on the same pools permuted.
    #[test]
    fn weak_signal_surfaces_subfloor_models() {
        let c = cfg();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in 0..5 {
            let d = desc(7000 + i);
            let (x, y) = (10000 + (i as i32) * 4200, 12000 + (i as i32) * 3100);
            a.push(kp(d, x, y));
            b.push(kp(d, x + 20000, y - 4000));
        }
        for i in 0..4 {
            let d = desc(7100 + i);
            a.push(kp(d, 3000 + (i as i32) * 9001 % 60000, 60000 - (i as i32) * 7013 % 55000));
            b.push(kp(d, 61000 - (i as i32) * 8837 % 58000, 2000 + (i as i32) * 6151 % 59000));
        }
        let am = mirror_side(&a, 65535);
        let pd = correspond_w(&a, &b);
        let pm = correspond_w(&am, &b);
        assert_eq!(pd.len(), 9);
        let (mm, measure, weak) = geo_measure_41(&a, &am, &b, &pd, &pm, &c, 256, 256, 6, 4);
        assert!(mm.models.is_empty(), "below the acceptance floor");
        assert_eq!(measure, 5, "the weak signal carries the sub-floor model");
        assert_eq!(weak, 5);
        let (ctl, _) = gn_control_41(&a, &am, &b, &pd, &pm, &c, 256, 256, 6, 4);
        assert!(ctl < measure, "control {} vs measure {}", ctl, measure);
        // and on a pool that DOES extract, the weak path stays out of it
        let (mm2, meas2, weak2) = {
            let (mut a2, mut b2) = (Vec::new(), Vec::new());
            for i in 0..12 {
                let d = desc(7500 + i);
                let (x, y) = (9000 + (i as i32) * 4100, 8000 + (i as i32) * 3300);
                a2.push(kp(d, x, y));
                b2.push(kp(d, x + 9000, y + 2500));
            }
            let am2 = mirror_side(&a2, 65535);
            let pd2 = correspond_w(&a2, &b2);
            let pm2 = correspond_w(&am2, &b2);
            geo_measure_41(&a2, &am2, &b2, &pd2, &pm2, &c, 256, 256, 6, 4)
        };
        assert!(!mm2.models.is_empty());
        assert_eq!(meas2, mm2.total_inliers);
        assert_eq!(weak2, 0);
    }

    /// Matches without coherent placement: no model, topology 1; nothing at
    /// all: topology 0.
    #[test]
    fn scattered_and_none() {
        let c = cfg();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for i in 0..10 {
            let d = desc(9000 + i);
            a.push(kp(d, (i as i32) * 6553 % 65000, (i as i32) * 4099 % 65000));
            b.push(kp(d, 64000 - (i as i32) * 5417 % 63000, (i as i32) * 7211 % 64000));
        }
        let mm = extract(&a, &b, &c, 65535, 256, 256, 6, 4);
        assert!(mm.corr_direct >= c.geo_min_corr);
        assert_eq!(mm.models.len(), 0);
        let (topo, _) = topology(&mm, 4, 6000, c.geo_min_corr);
        assert_eq!(topo, 1);

        let mm = extract(&a[..2], &b[..2], &c, 65535, 256, 256, 6, 4);
        let (topo, _) = topology(&mm, 4, 6000, c.geo_min_corr);
        assert_eq!(topo, 0);
    }
}
