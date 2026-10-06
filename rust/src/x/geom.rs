//! XGeom (PAPH-X §9, §26) — comparator 42's geometry on a sparse pool, with
//! nothing allocated per verification, per round or per null.
//!
//! The procedure is SPEC-004.2 §5–§8 verbatim: weighted Hough into the fixed
//! vote table, peak by total order, closed-form fit, loose pass, refit,
//! tight count, median residual; multi-model extraction with §7
//! consumption; the GN control as MAX over the same five permutations each
//! running the identical measurement; §8 diversity and §13 topology.  A
//! test holds it equal to `geom42::extract_from_pools_42` on identical
//! pools.  What changed is only where the pool comes from (the sparse
//! matcher, anchors first) and where the buffers live (here, once).

use super::profile::MAX_TOTAL_CORR;
use crate::calibration::Profile;
use crate::compare::{fit_similarity, level_table, Model};
use crate::config::{clamp, idiv, isqrt, Config, SCALE};
use crate::coverage::{cell, Coverage};
use crate::geom42::{cell_box, count_inliers_into, excl_radius, hamming, key_digits, pack_desc, residual2, CellBox, Corr42, Desc4, Diversity, ModelRec42, VoteTable, SK_NEAR};
use crate::keypoints::{Keypoint, DESC_NOVEL_AT};

pub const MAX_MODELS: usize = 8;

/// One verification's result; its inlier mask lives in the scratch.
#[derive(Clone, Copy, Debug)]
pub struct VerifyX {
    pub inliers: i64,
    pub model: Option<Model>,
    pub median_err: i64,
    pub conf_sum: i64,
}

const EMPTY: VerifyX = VerifyX { inliers: 0, model: None, median_err: i64::MAX, conf_sum: 0 };

/// The extraction result with fixed-capacity storage.
#[derive(Clone, Debug)]
pub struct MultiX {
    pub models: [ModelRec42; MAX_MODELS],
    pub nmodels: usize,
    pub total_inliers: i64,
    pub corr_direct: usize,
    pub corr_mirror: usize,
    pub round0: Option<i64>,
    /// unique B keypoints among the inliers of accepted models
    pub unique_b: usize,
    pub unique_a: usize,
}

impl MultiX {
    pub fn empty() -> MultiX {
        MultiX {
            models: [ModelRec42 { r00: 0, r10: 0, tx: 0, ty: 0, scale_q16: 0, mirror: false, inliers: 0, median_err: 0, conf_sum: 0 }; MAX_MODELS],
            nmodels: 0,
            total_inliers: 0,
            corr_direct: 0,
            corr_mirror: 0,
            round0: None,
            unique_b: 0,
            unique_a: 0,
        }
    }
    pub fn models(&self) -> &[ModelRec42] {
        &self.models[..self.nmodels]
    }
    pub fn any_mirror(&self) -> bool {
        self.models().iter().any(|m| m.mirror)
    }
}

pub struct GeomScratch {
    table: VoteTable,
    boxes: Vec<CellBox>,
    members: Vec<(usize, usize, i32)>,
    inl: Vec<(usize, usize, i32)>,
    mask_d: Vec<bool>,
    mask_m: Vec<bool>,
    res: Vec<i64>,
    blocked: Vec<bool>,
    consumed: Vec<usize>,
    seen_a: Vec<bool>,
    excl_r2: Vec<i64>,
    levels: Option<(i64, i64, Box<[i64; 256]>, Box<[i64; 256]>)>,
    /// working pools of the extraction (pruned by consumption)
    wd: Vec<Corr42>,
    wm: Vec<Corr42>,
    /// permuted pools of the control
    nd: Vec<Corr42>,
    nm: Vec<Corr42>,
    /// inliers of accepted models: B positions, A indices, B levels
    pub inlier_b: Vec<(i64, i64)>,
    pub inlier_a: Vec<u16>,
    pub inlier_level: Vec<u8>,
    heads: Vec<Desc4>,
    cov_counts: Vec<u16>,
    cov_parent: Vec<usize>,
    cov_sizes: Vec<i64>,
}

impl GeomScratch {
    pub fn new() -> GeomScratch {
        let c = MAX_TOTAL_CORR;
        GeomScratch {
            table: VoteTable::new(),
            boxes: Vec::with_capacity(c),
            members: Vec::with_capacity(c),
            inl: Vec::with_capacity(c),
            mask_d: Vec::with_capacity(c),
            mask_m: Vec::with_capacity(c),
            res: Vec::with_capacity(c),
            blocked: Vec::with_capacity(512),
            consumed: Vec::with_capacity(512),
            seen_a: Vec::with_capacity(512),
            excl_r2: Vec::with_capacity(256),
            levels: None,
            wd: Vec::with_capacity(c),
            wm: Vec::with_capacity(c),
            nd: Vec::with_capacity(c),
            nm: Vec::with_capacity(c),
            inlier_b: Vec::with_capacity(c),
            inlier_a: Vec::with_capacity(c),
            inlier_level: Vec::with_capacity(c),
            heads: Vec::with_capacity(c),
            cov_counts: Vec::with_capacity(256),
            cov_parent: Vec::with_capacity(256),
            cov_sizes: Vec::with_capacity(256),
        }
    }

    fn level_tables(&mut self, mda: i64, mdb: i64) {
        match &self.levels {
            Some((a, b, _, _)) if *a == mda && *b == mdb => {}
            _ => self.levels = Some((mda, mdb, Box::new(level_table(mda)), Box::new(level_table(mdb)))),
        }
    }
}

impl Default for GeomScratch {
    fn default() -> Self {
        Self::new()
    }
}

/// The pair's frames: keypoints of A (direct), A mirrored, B, and the two
/// maximum dimensions.
pub struct Frames<'a> {
    pub a: &'a [Keypoint],
    pub am: &'a [Keypoint],
    pub b: &'a [Keypoint],
    pub mda: i64,
    pub mdb: i64,
}

/// `hough_verify_42`, the mask written into `mask` (a scratch buffer).
#[allow(clippy::too_many_arguments)]
fn verify(
    a: &[Keypoint],
    b: &[Keypoint],
    corr: &[Corr42],
    cfg: &Config,
    p: &Profile,
    table: &mut VoteTable,
    boxes: &mut Vec<CellBox>,
    members: &mut Vec<(usize, usize, i32)>,
    inl: &mut Vec<(usize, usize, i32)>,
    mask: &mut Vec<bool>,
    res: &mut Vec<i64>,
    lta: &[i64; 256],
    ltb: &[i64; 256],
) -> VerifyX {
    mask.clear();
    mask.resize(corr.len(), false);
    if corr.len() < cfg.geo_min_corr {
        return EMPTY;
    }
    let soft = p.scale_soft != 0;
    table.begin(corr.len());
    boxes.clear();
    for c in corr.iter() {
        let (ka, kb) = (&a[c.a], &b[c.b]);
        let bx = cell_box(ka, kb, lta[ka.level as usize], ltb[kb.level as usize], soft);
        let (w, cf) = (c.weight(), c.conf as i64);
        bx.for_each(|k, kw| table.add(k, if kw == SK_NEAR { w } else { w * kw / SK_NEAR }, cf));
        boxes.push(bx);
    }
    let (best_key, _, _) = table.peak();
    if best_key == i32::MAX {
        return EMPTY;
    }
    let (bl, br, bx_, by) = key_digits(best_key);
    members.clear();
    for (i, c) in corr.iter().enumerate() {
        if boxes[i].has(bl, br, bx_, by) {
            members.push((c.a, c.b, c.d1));
        }
    }
    if (members.len() as i32) < p.min_peak_members {
        return EMPTY;
    }
    let tol = (cfg.geo_eps as i64 >> 4).max(2);
    let tol2 = tol * tol;
    let mut m = match fit_similarity(a, b, members) {
        Some(m) => m,
        None => return EMPTY,
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
    let median_err = if res.is_empty() {
        i64::MAX
    } else {
        let mid = res.len() >> 1;
        *res.select_nth_unstable(mid).1
    };
    VerifyX { inliers: n, model: Some(m), median_err, conf_sum }
}

/// §12.3 extraction with §7 consumption on the scratch's working pools —
/// `extract_from_pools_42`, allocation-free.  The working pools must have
/// been filled by the caller (`wd`, `wm`).
fn extract_work(f: &Frames, cfg: &Config, p: &Profile, sc: &mut GeomScratch, keep_inliers: bool) -> MultiX {
    let mut out = MultiX::empty();
    out.corr_direct = sc.wd.len();
    out.corr_mirror = sc.wm.len();
    let min_model_inliers = p.min_model_inliers as i64;
    let max_models = (p.max_models as usize).min(MAX_MODELS);
    sc.level_tables(f.mda, f.mdb);
    let pct = p.excl_pct;
    sc.excl_r2.clear();
    let maxlev = f.b.iter().map(|k| k.level as usize).max().unwrap_or(0);
    for lv in 0..=maxlev {
        let r = excl_radius(lv as u8, f.mdb, pct);
        sc.excl_r2.push(r * r);
    }
    if keep_inliers {
        sc.inlier_b.clear();
        sc.inlier_a.clear();
        sc.inlier_level.clear();
    }
    sc.blocked.clear();
    sc.blocked.resize(f.b.len(), false);
    sc.seen_a.clear();
    sc.seen_a.resize(f.a.len(), false);

    for r in 0..max_models {
        let floor = if r == 0 { cfg.geo_min_corr as i64 } else { min_model_inliers };
        let GeomScratch { table, boxes, members, inl, mask_d, mask_m, res, levels, wd, wm, .. } = sc;
        let (_, _, lta, ltb) = levels.as_ref().unwrap();
        let vd = verify(f.a, f.b, wd, cfg, p, table, boxes, members, inl, mask_d, res, lta, ltb);
        let vm = verify(f.am, f.b, wm, cfg, p, table, boxes, members, inl, mask_m, res, lta, ltb);
        if r == 0 {
            out.round0 = Some(vd.inliers.max(vm.inliers));
        }
        let use_mirror = vm.inliers > vd.inliers
            || (vm.inliers == vd.inliers
                && vm.inliers > 0
                && (vm.median_err < vd.median_err || (vm.median_err == vd.median_err && vm.conf_sum > vd.conf_sum)));
        let v = if use_mirror { vm } else { vd };
        let Some(m) = v.model else { break };
        if v.inliers < floor {
            break;
        }
        // §7 consumption across both pools, plus the exclusion neighbourhood,
        // in 4.2's two passes: every inlier's B keypoint is consumed first,
        // then the neighbourhoods of the NEWLY consumed ones are excluded
        {
            let (pool, mask) = if use_mirror { (&*wm, &*mask_m) } else { (&*wd, &*mask_d) };
            sc.consumed.clear();
            for (i, c) in pool.iter().enumerate() {
                if mask[i] {
                    if !sc.blocked[c.b] {
                        sc.consumed.push(c.b);
                        sc.blocked[c.b] = true;
                        out.unique_b += 1;
                    }
                    if keep_inliers {
                        sc.inlier_b.push((f.b[c.b].x as i64, f.b[c.b].y as i64));
                        sc.inlier_a.push(c.a as u16);
                        sc.inlier_level.push(f.b[c.b].level);
                    }
                    if !sc.seen_a[c.a] {
                        sc.seen_a[c.a] = true;
                        out.unique_a += 1;
                    }
                }
            }
            out.total_inliers += v.inliers;
            out.models[out.nmodels] = ModelRec42 {
                r00: m.r00,
                r10: m.r10,
                tx: m.tx,
                ty: m.ty,
                scale_q16: isqrt(m.r00 * m.r00 + m.r10 * m.r10),
                mirror: use_mirror,
                inliers: v.inliers,
                median_err: v.median_err,
                conf_sum: v.conf_sum,
            };
            out.nmodels += 1;
            if pct > 0 {
                for k in 0..sc.consumed.len() {
                    let j = sc.consumed[k];
                    let rr2 = sc.excl_r2[f.b[j].level as usize];
                    if rr2 == 0 {
                        continue;
                    }
                    let (jx, jy) = (f.b[j].x as i64, f.b[j].y as i64);
                    for (t, kp) in f.b.iter().enumerate() {
                        if sc.blocked[t] {
                            continue;
                        }
                        let dx = kp.x as i64 - jx;
                        let dy = kp.y as i64 - jy;
                        if dx * dx + dy * dy <= rr2 {
                            sc.blocked[t] = true;
                        }
                    }
                }
            }
        }
        let blocked = &sc.blocked;
        sc.wd.retain(|c| !blocked[c.b]);
        sc.wm.retain(|c| !blocked[c.b]);
    }
    out
}

/// §A1 — extraction first, the weak signal only where it accepts nothing.
/// Returns (extraction, measure, weak).
pub fn measure(f: &Frames, pd: &[Corr42], pm: &[Corr42], cfg: &Config, p: &Profile, sc: &mut GeomScratch) -> (MultiX, i64, i64) {
    sc.wd.clear();
    sc.wd.extend_from_slice(pd);
    sc.wm.clear();
    sc.wm.extend_from_slice(pm);
    let mm = extract_work(f, cfg, p, sc, true);
    if mm.total_inliers > 0 {
        let t = mm.total_inliers;
        (mm, t, 0)
    } else {
        let w = mm.round0.unwrap_or(0);
        (mm, w, w)
    }
}

fn shift_into(src: &[Corr42], num: usize, den: usize, out: &mut Vec<Corr42>) {
    out.clear();
    let n = src.len();
    if n == 0 {
        return;
    }
    let p = n * num / den;
    if p == 0 || p >= n {
        return;
    }
    for i in 0..n {
        out.push(Corr42 { b: src[(i + p) % n].b, ..src[i] });
    }
}

fn reverse_into(src: &[Corr42], out: &mut Vec<Corr42>) {
    out.clear();
    let n = src.len();
    if n < 2 {
        return;
    }
    for i in 0..n {
        out.push(Corr42 { b: src[n - 1 - i].b, ..src[i] });
    }
}

/// §9.3 — the GN control, MAX over the five permutations, each the
/// identical measurement.  `stop_at`: once a member reaches this, the
/// remaining members cannot change what the caller does with the control
/// (the margin is already zero), so the control returns early — an
/// optimisation that cannot move a verdict because the lattice reads the
/// control only through `chance_correct`, which is 0 for every control at
/// or past saturation.
pub fn control(f: &Frames, pd: &[Corr42], pm: &[Corr42], cfg: &Config, p: &Profile, sc: &mut GeomScratch, stop_at: i64) -> (i64, &'static str) {
    let shifts: [(&'static str, usize, usize); 4] = [("gn:half", 1, 2), ("gn:third", 1, 3), ("gn:fifth", 1, 5), ("gn:twothirds", 2, 3)];
    let (mut best, mut member) = (0i64, "gn:none");
    let one = |sc: &mut GeomScratch, name: &'static str, best: &mut i64, member: &mut &'static str| -> bool {
        if sc.nd.is_empty() && sc.nm.is_empty() {
            return false;
        }
        sc.wd.clear();
        sc.wm.clear();
        // swap the permuted pools in as the working pools without copying
        std::mem::swap(&mut sc.wd, &mut sc.nd);
        std::mem::swap(&mut sc.wm, &mut sc.nm);
        let mm = extract_work(f, cfg, p, sc, false);
        let t = if mm.total_inliers > 0 { mm.total_inliers } else { mm.round0.unwrap_or(0) };
        std::mem::swap(&mut sc.wd, &mut sc.nd);
        std::mem::swap(&mut sc.wm, &mut sc.nm);
        if t > *best {
            *best = t;
            *member = name;
        }
        *best >= stop_at
    };
    for (name, num, den) in shifts {
        let mut nd = std::mem::take(&mut sc.nd);
        let mut nm = std::mem::take(&mut sc.nm);
        shift_into(pd, num, den, &mut nd);
        shift_into(pm, num, den, &mut nm);
        sc.nd = nd;
        sc.nm = nm;
        if one(sc, name, &mut best, &mut member) {
            return (best, member);
        }
    }
    let mut nd = std::mem::take(&mut sc.nd);
    let mut nm = std::mem::take(&mut sc.nm);
    reverse_into(pd, &mut nd);
    reverse_into(pm, &mut nm);
    sc.nd = nd;
    sc.nm = nm;
    one(sc, "gn:reverse", &mut best, &mut member);
    (best, member)
}

/// `coverage::coverage` into reusable buffers.
pub fn coverage_into(points: &[(i64, i64)], g: u8, sc: &mut GeomScratch) -> Coverage {
    let gs = g as usize;
    let counts = &mut sc.cov_counts;
    counts.clear();
    counts.resize(gs * gs, 0);
    for &(x, y) in points {
        let c = cell(y, g) * gs + cell(x, g);
        counts[c] = counts[c].saturating_add(1);
    }
    let occupied = counts.iter().filter(|&&c| c > 0).count() as i64;
    if occupied == 0 {
        return Coverage { g, occupied: 0, coverage: 0, bbox_cells: 0, concentration: 0, counts: counts.clone() };
    }
    let (mut x0, mut x1, mut y0, mut y1) = (gs, 0usize, gs, 0usize);
    for i in 0..gs * gs {
        if counts[i] == 0 {
            continue;
        }
        let (cx, cy) = (i % gs, i / gs);
        x0 = x0.min(cx);
        x1 = x1.max(cx);
        y0 = y0.min(cy);
        y1 = y1.max(cy);
    }
    let bbox_cells = ((x1 - x0 + 1) * (y1 - y0 + 1)) as i64;
    let parent = &mut sc.cov_parent;
    parent.clear();
    parent.extend(0..gs * gs);
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for cy in 0..gs {
        for cx in 0..gs {
            let i = cy * gs + cx;
            if counts[i] == 0 {
                continue;
            }
            for j in [if cx + 1 < gs && counts[i + 1] > 0 { i + 1 } else { usize::MAX }, if cy + 1 < gs && counts[i + gs] > 0 { i + gs } else { usize::MAX }] {
                if j == usize::MAX {
                    continue;
                }
                let (ra, rb) = (find(parent, i), find(parent, j));
                if ra != rb {
                    let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
                    parent[hi] = lo;
                }
            }
        }
    }
    let sizes = &mut sc.cov_sizes;
    sizes.clear();
    sizes.resize(gs * gs, 0);
    let mut largest = 0i64;
    for i in 0..gs * gs {
        if counts[i] == 0 {
            continue;
        }
        let r = find(parent, i);
        sizes[r] += 1;
        largest = largest.max(sizes[r]);
    }
    Coverage { g, occupied, coverage: occupied * SCALE / (gs * gs) as i64, bbox_cells, concentration: largest * SCALE / occupied, counts: counts.clone() }
}

/// §8 diversity on the scratch's inlier lists (the last `measure`).
pub fn diversity(mm: &MultiX, a: &[Keypoint], b: &[Keypoint], p: &Profile, sc: &mut GeomScratch) -> (Diversity, Coverage) {
    let inl_b = std::mem::take(&mut sc.inlier_b);
    let cov = coverage_into(&inl_b, p.grid_g, sc);
    sc.inlier_b = inl_b;
    if mm.nmodels == 0 {
        return (Diversity { multiplier: SCALE, ..Default::default() }, cov);
    }
    let spatial = cov.coverage;
    let mut avail = [false; 256];
    for k in b.iter() {
        avail[k.level as usize] = true;
    }
    let navail = avail.iter().filter(|&&v| v).count().max(1) as i64;
    let mut used = [false; 256];
    for &l in sc.inlier_level.iter() {
        used[l as usize] = true;
    }
    let nused = used.iter().filter(|&&v| v).count() as i64;
    let scale = clamp(nused * SCALE / navail, 0, SCALE);
    let model = clamp(mm.nmodels as i64 * SCALE / (p.max_models as i64).max(1), 0, SCALE);
    sc.heads.clear();
    for &ia in sc.inlier_a.iter() {
        let ia = ia as usize;
        if ia >= a.len() {
            continue;
        }
        let d = pack_desc(&a[ia].desc);
        if sc.heads.iter().all(|h| hamming(h, &d) as i64 > DESC_NOVEL_AT) {
            sc.heads.push(d);
        }
    }
    let descriptor = clamp(sc.heads.len() as i64 * SCALE / (sc.inlier_a.len() as i64).max(1), 0, SCALE);
    let combined = idiv(40 * spatial + 25 * scale + 20 * model + 15 * descriptor, 100);
    let multiplier = p.lut_geo_diversity.eval(combined);
    (Diversity { spatial, scale, model, descriptor, combined, multiplier }, cov)
}

/// §13 — the five topology classes.
pub fn topology(mm: &MultiX, cov: &Coverage, g: u8, dominant_at: i64, geo_min_corr: usize) -> u8 {
    if mm.nmodels >= 2 {
        3
    } else if mm.nmodels == 1 {
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
    use crate::compare::mirror_side;
    use crate::geom42::{correspond_42, diversity_42, extract_from_pools_42, gn_control_42, pack_all, topology_42, Scratch};
    use crate::prepared::Prepared;
    use crate::wire::hash;

    /// On the same pools, the sparse-path geometry equals 4.2's: models,
    /// residuals, inliers, control member and diversity, on self, mirrored
    /// and unrelated pairs.
    #[test]
    fn equals_geom42_on_identical_pools() {
        let cfg = Config::default();
        let p = Profile::cal004();
        let rot = crate::keypoints::RotCache::new(&crate::keypoints::pattern());
        let img = |s: u64| crate::x::testimg::image(s, 180, 130);
        let base = img(5);
        let mir = crate::x::testimg::mirror(&base, 180, 130);
        let other = img(99);
        let cases = [(&base, &mir), (&base, &base), (&base, &other), (&mir, &other)];
        let mut sc42 = Scratch::new();
        let mut scx = GeomScratch::new();
        for (ia, ib) in cases.iter() {
            let fa = hash(ia, 180, 130, &cfg, &rot);
            let fb = hash(ib, 180, 130, &cfg, &rot);
            let pa = Prepared::new(&fa.t1, Some(&fa.t2)).unwrap();
            let pb = Prepared::new(&fb.t1, Some(&fb.t2)).unwrap();
            let am = mirror_side(&pa.kp, pa.t1_xmax());
            let (ad, amd, bd) = (pack_all(&pa.kp), pack_all(&am), pack_all(&pb.kp));
            let pd = correspond_42(&pa.kp, &ad, &pb.kp, &bd, &p);
            let pm = correspond_42(&am, &amd, &pb.kp, &bd, &p);
            let (mda, mdb) = (pa.t1_max_dim(), pb.t1_max_dim());
            let f = Frames { a: &pa.kp, am: &am, b: &pb.kp, mda, mdb };
            let m42 = extract_from_pools_42(&pa.kp, &am, &pb.kp, pd.clone(), pm.clone(), &cfg, &p, mda, mdb, &mut sc42);
            let (mx, meas, _) = measure(&f, &pd, &pm, &cfg, &p, &mut scx);
            assert_eq!(mx.nmodels, m42.models.len());
            for (x, y) in mx.models().iter().zip(m42.models.iter()) {
                assert_eq!((x.r00, x.r10, x.tx, x.ty, x.scale_q16, x.mirror, x.inliers, x.median_err, x.conf_sum), (y.r00, y.r10, y.tx, y.ty, y.scale_q16, y.mirror, y.inliers, y.median_err, y.conf_sum));
            }
            assert_eq!(mx.total_inliers, m42.total_inliers);
            assert_eq!(mx.round0, m42.round0);
            assert_eq!(scx.inlier_b, m42.inlier_b);
            assert_eq!(scx.inlier_a.iter().map(|&v| v as usize).collect::<Vec<_>>(), m42.inlier_a);
            let (dx, cx) = diversity(&mx, &pa.kp, &pb.kp, &p, &mut scx);
            let (d42, c42) = diversity_42(&m42, &pa.kp, &pb.kp, &p);
            assert_eq!(dx, d42);
            assert_eq!(cx, c42);
            assert_eq!(topology(&mx, &cx, p.grid_g, p.thresholds[8] as i64, cfg.geo_min_corr), topology_42(&m42, &c42, p.grid_g, p.thresholds[8] as i64, cfg.geo_min_corr));
            let (c1, n1) = control(&f, &pd, &pm, &cfg, &p, &mut scx, i64::MAX);
            let (c2, n2) = gn_control_42(&pa.kp, &am, &pb.kp, &pd, &pm, &cfg, &p, mda, mdb, &mut sc42);
            assert_eq!((c1, n1), (c2, n2));
            let _ = meas;
        }
    }
}
