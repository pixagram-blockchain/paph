//! Comparator 42 (SPEC-004.2).
//!
//! Everything comparator 41 is — A1 weak-signal geometry, A2 the sparse local
//! assignment, A3 per-channel calibration tables, A4 the stage-1 screen — on a
//! doubled keypoint budget, through the rebuilt geometry stage in `geom42.rs`,
//! with one new evidence channel:
//!
//!   §8  DIVERSITY.  512 matches concentrated in one corner of one artwork at
//!       one scale under one model are not 512 pieces of evidence, and the
//!       inlier count cannot see the difference.  The geometric evidence is
//!       therefore modulated by a calibrated reading of how independent it is.
//!
//! What did NOT change is the part that decides.  The lattice is comparator
//! 41's, rule for rule; the null family is the same five permutations under
//! MAX; proportion and Gate remain out of the decision path, where 41 already
//! put them.  A larger budget is a reason to measure better, not a reason to
//! re-argue what a copy is.
//!
//! Comparator 41 stays frozen in `v41.rs` and remains computable, because a
//! verdict that has been issued has to stay reproducible.  Each comparator
//! refuses the other's profiles, in every direction.

use crate::calibration::{Profile, COMPARATOR_V42};
use crate::compare::{compare_canonical, mirror_side, Reading};
use crate::config::{chance_correct, idiv, Config, SCALE};
use crate::geom42::{diversity_42, gn_control_42, geo_measure_42, topology_42, Diversity, ModelRec42, Scratch};
use crate::coverage::Coverage;
use crate::lattice::{lattice_v4, LatticeIn};
use crate::local_v4::{local_v4_41_shared, LocalV4};
use crate::prepared::{canon_swapped, corr_42, PairCtx, Prepared};
use crate::v4::{bind, V4Report, R_CORRUPT, R_PROFILE_MISMATCH, R_PROFILE_UNSUPPORTED};

/// §A4 — the stage-1 screen: correspondences only, no verification, no
/// verdict.  A pair the screen rejects is UNSCREENED, never Unrelated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen42 {
    pub pass: bool,
    pub pool_direct: usize,
    pub pool_mirror: usize,
}

/// The comparator-42 report.  A superset of `V4Report`: everything 41 said,
/// plus the §8 diversity reading, the per-model residuals, and the selection
/// rule each side's Tier 2 was built with.
#[derive(Clone, Debug)]
pub struct V42Report {
    pub base: V4Report,
    pub models42: Vec<ModelRec42>,
    pub diversity_geo: Diversity,
    /// keypoint-selection rule of each side's Tier 2 (0 = 4.1, 1 = 4.2)
    pub select_a: i32,
    pub select_b: i32,
    /// Tier-2 record counts actually compared
    pub kp_a: usize,
    pub kp_b: usize,
}

impl V42Report {
    /// True when the two sides were hashed under different selection rules.
    ///
    /// Not an error and not a refusal: both wires are valid and the comparison
    /// is well defined.  It is a WARNING, because a 4.1 side brings 256
    /// strength-ranked keypoints to a 4.2 side's 512 quality-ranked ones, and
    /// recall measured across that boundary is not recall measured within
    /// either.  A corpus should be re-hashed before its thresholds are.
    pub fn mixed_selection(&self) -> bool {
        self.select_a != self.select_b
    }
}

pub fn screen_v42(
    a_t1: &[u8],
    a_t2: Option<&[u8]>,
    b_t1: &[u8],
    b_t2: Option<&[u8]>,
    cfg: &Config,
    profile: &Profile,
) -> Screen42 {
    // A Tier 2 that does not parse falls back to the sketch here, as it always
    // has; a Tier 1 that does not parse is UNSCREENED.
    match (Prepared::new(a_t1, a_t2), Prepared::new(b_t1, b_t2)) {
        (Ok(a), Ok(b)) => screen_v42_prepared(&a, &b, cfg, profile),
        _ => Screen42 { pass: false, pool_direct: 0, pool_mirror: 0 },
    }
}

/// §A4 on prepared sides — what an index runs per candidate.
pub fn screen_v42_prepared(a: &Prepared, b: &Prepared, cfg: &Config, profile: &Profile) -> Screen42 {
    let cfg = bind(cfg, profile);
    let swapped = canon_swapped(a, b);
    let (ca, cb) = if swapped { (b, a) } else { (a, b) };
    let mut ctx = PairCtx::new(ca, cb);
    screen_in(&mut ctx, &cfg, profile)
}

/// The screen on a pair context in canonical order, with `cfg` already bound
/// to the profile.  The descriptor scans it runs stay in `ctx`, so a compare
/// that follows on the same context does not repeat them.
pub fn screen_in(ctx: &mut PairCtx, cfg: &Config, profile: &Profile) -> Screen42 {
    let (ca, cb) = (ctx.a, ctx.b);
    let pd = corr_42(ctx.direct(), &ca.kp, &cb.kp, profile).len();
    let pm = if cfg.mirror_hypothesis {
        corr_42(ctx.mirror(), &ca.kp, &cb.kp, profile).len()
    } else {
        0
    };
    Screen42 { pass: pd.max(pm) >= cfg.geo_min_corr, pool_direct: pd, pool_mirror: pm }
}

thread_local! {
    static SCRATCH: std::cell::Cell<Option<Box<Scratch>>> = const { std::cell::Cell::new(None) };
}

fn take_scratch() -> Box<Scratch> {
    SCRATCH.with(|c| c.take()).unwrap_or_else(|| Box::new(Scratch::new()))
}

fn put_scratch(sc: Box<Scratch>) {
    SCRATCH.with(|c| c.set(Some(sc)));
}

/// The refusal report (Indeterminate, with `reason`).
pub fn refuse_report(reason: &'static str, profile: &Profile) -> V42Report {
    refuse(reason, profile)
}

fn refuse(reason: &'static str, profile: &Profile) -> V42Report {
    let mut base = crate::v4::indeterminate(vec![reason], profile);
    base.comparator = COMPARATOR_V42;
    V42Report {
        base,
        models42: Vec::new(),
        diversity_geo: Diversity { multiplier: SCALE, ..Default::default() },
        select_a: 0,
        select_b: 0,
        kp_a: 0,
        kp_b: 0,
    }
}

/// SPEC-004.2 — the comparator-42 verdict.
#[allow(clippy::too_many_arguments)]
pub fn compare_v42(
    a_t1: &[u8],
    a_t2: Option<&[u8]>,
    b_t1: &[u8],
    b_t2: Option<&[u8]>,
    cfg: &Config,
    profile: &Profile,
    hash_profile_a: Option<[u8; 32]>,
    hash_profile_b: Option<[u8; 32]>,
) -> V42Report {
    if profile.validate().is_err() || profile.comparator != COMPARATOR_V42 {
        return refuse(R_PROFILE_UNSUPPORTED, profile);
    }
    if let (Some(ha), Some(hb)) = (hash_profile_a, hash_profile_b) {
        if ha != hb {
            return refuse(R_PROFILE_MISMATCH, profile);
        }
    }
    match (Prepared::new(a_t1, a_t2), Prepared::new(b_t1, b_t2)) {
        (Ok(a), Ok(b)) => compare_v42_prepared(&a, &b, cfg, profile, None, None),
        _ => refuse(R_CORRUPT, profile),
    }
}

/// Comparator 42 on prepared sides: parse once, then every comparison against
/// the same query reuses it.  Identical to `compare_v42` on the same bytes.
#[allow(clippy::too_many_arguments)]
pub fn compare_v42_prepared(
    a: &Prepared,
    b: &Prepared,
    cfg: &Config,
    profile: &Profile,
    hash_profile_a: Option<[u8; 32]>,
    hash_profile_b: Option<[u8; 32]>,
) -> V42Report {
    compare_v42_reading(a, b, cfg, profile, hash_profile_a, hash_profile_b, Reading::Full)
}

/// Comparator 42 for ranking: the same verdict and every comparator-42 field,
/// without the v3 diagnostic reading (`base.v3` is `None`).  The v3 local and
/// geometric channels are the only parts of that reading comparator 42 never
/// reads, and they are a fifth of the work.
pub fn compare_v42_lean(a: &Prepared, b: &Prepared, cfg: &Config, profile: &Profile) -> V42Report {
    compare_v42_reading(a, b, cfg, profile, None, None, Reading::Lean)
}

#[allow(clippy::too_many_arguments)]
pub fn compare_v42_reading(
    a: &Prepared,
    b: &Prepared,
    cfg: &Config,
    profile: &Profile,
    hash_profile_a: Option<[u8; 32]>,
    hash_profile_b: Option<[u8; 32]>,
    reading: Reading,
) -> V42Report {
    if profile.validate().is_err() || profile.comparator != COMPARATOR_V42 {
        return refuse(R_PROFILE_UNSUPPORTED, profile);
    }
    if let (Some(ha), Some(hb)) = (hash_profile_a, hash_profile_b) {
        if ha != hb {
            return refuse(R_PROFILE_MISMATCH, profile);
        }
    }
    // the v3 reading refuses a Tier 2 that does not parse, and so does 42
    if a.t2_error.is_some() || b.t2_error.is_some() {
        return refuse(R_CORRUPT, profile);
    }
    // a wire-3 and a wire-4 side: refused, never compared across formats
    if a.t1.version != b.t1.version {
        return refuse(crate::wire::R_WIRE_MISMATCH, profile);
    }
    let cfg = bind(cfg, profile);

    // P4 — canonical argument order, the rule verbatim from 4 and 4.1; the v3
    // reading canonicalises by the same rule, so both layers share one pair
    // context and every descriptor scan in it.
    let swapped = canon_swapped(a, b);
    let (ca, cb) = if swapped { (b, a) } else { (a, b) };
    let mut ctx = PairCtx::new(ca, cb);
    compare_in(&mut ctx, swapped, &cfg, profile, reading)
}

/// The comparator on a pair context in canonical order, `cfg` bound, the
/// profile and both Tier 2s already accepted.
pub fn compare_in(ctx: &mut PairCtx, swapped: bool, cfg: &Config, profile: &Profile, reading: Reading) -> V42Report {
    let cfg = *cfg;
    let (ca, cb) = (ctx.a, ctx.b);
    let v3 = compare_canonical(ctx, swapped, &cfg, reading);
    let (ta, tb) = (&ca.t1, &cb.t1);

    // A2 — the sparse-assignment local channel, unchanged, on the bag
    // distances the v3 reading already computed.
    let mut loc: LocalV4 = local_v4_41_shared(&ca.bag, &cb.bag, profile, ctx.bags());

    let (ka, sel_a) = (&ca.kp, ca.select);
    let (kb, sel_b) = (&cb.kp, cb.select);
    let (mda, mdb) = (ta.max_dim(), tb.max_dim());
    let xmax_a = idiv((ta.width as i64 - 1) * 65535, mda.max(1)).clamp(0, 65535) as i32;

    // §12 — pools once, shared by the measurement and its control.
    let am = mirror_side(ka, xmax_a);
    let pd = corr_42(ctx.direct(), ka, kb, profile);
    let pm = if cfg.mirror_hypothesis {
        corr_42(ctx.mirror(), &am, kb, profile)
    } else {
        Vec::new()
    };

    // A1 — extraction first, the weak signal only where it accepts nothing.
    // One scratch for the whole comparison: the measurement and its five null
    // members run a few dozen verifications between them, and every buffer
    // any of them needs is allocated once.  The scratch also outlives the
    // comparison — a thread keeps one — because its vote table is half a
    // megabyte, and allocating and zeroing that per pair was a measurable
    // share of a comparison in WebAssembly.  Nothing in it carries a result
    // from one verification to the next (the table is cleared by epoch).
    let mut sc = take_scratch();
    let (mm, measure, weak) =
        geo_measure_42(ka, &am, kb, &pd, &pm, &cfg, profile, mda, mdb, &mut sc);
    let (div, cov): (Diversity, Coverage) = diversity_42(&mm, ka, kb, profile);
    let topo = topology_42(&mm, &cov, profile.grid_g, profile.thresholds[8] as i64, cfg.geo_min_corr);
    let (ctl_raw, ctl_member) =
        gn_control_42(ka, &am, kb, &pd, &pm, &cfg, profile, mda, mdb, &mut sc);
    put_scratch(sc);

    let geo_measurable = ka.len() >= cfg.geo_min_corr && kb.len() >= cfg.geo_min_corr;
    let raw = (measure * SCALE / cfg.geo_conf_at as i64).clamp(0, SCALE);
    let ctl = (ctl_raw * SCALE / cfg.geo_conf_at as i64).clamp(0, SCALE);
    let geo_margin = chance_correct(raw, ctl);
    // §8 — the calibrated margin, modulated by how independent the evidence is
    let geometry_evidence =
        idiv(profile.lut_geometry.eval(geo_margin) * div.multiplier, SCALE).clamp(0, SCALE);

    // §14 over A3-transformed channels, exactly as 41 does it.
    let mut channels: [(&'static str, i64, bool); 7] = [("", 0, false); 7];
    for (k, (name, ch)) in v3.channels.iter().enumerate() {
        channels[k] = if *name == "local" {
            (*name, loc.evidence, loc.measurable)
        } else {
            (*name, profile.lut_channel(name).eval(ch.value), ch.measurable)
        };
    }
    let latin = LatticeIn {
        identical: v3.identical,
        channels,
        geo_measurable,
        geo_evidence: geometry_evidence,
        total_inliers: measure,
        topology_class: topo,
        diversity: loc.diversity,
        coverage_min: loc.coverage_a.coverage.min(loc.coverage_b.coverage),
        any_mirror_model: mm.models.iter().any(|m| m.mirror),
    };
    let verdict = lattice_v4(&latin, profile);

    if swapped {
        std::mem::swap(&mut loc.coverage_a, &mut loc.coverage_b);
        std::mem::swap(&mut loc.d_a, &mut loc.d_b);
        loc.pairs = loc.pairs.iter().map(|&(i, j)| (j, i)).collect();
    }

    let models42 = mm.models.clone();
    let base = V4Report {
        comparator: COMPARATOR_V42,
        verdict: verdict.state,
        class: verdict.class,
        basis: verdict.basis,
        reasons: Vec::new(),
        structural: verdict.structural,
        certifiable: verdict.certifiable,
        v3: if reading == Reading::Full { Some(v3) } else { None },
        local: Some(loc),
        models: mm
            .models
            .iter()
            .map(|m| crate::multimodel::ModelRec {
                r00: m.r00,
                r10: m.r10,
                tx: m.tx,
                ty: m.ty,
                scale_q16: m.scale_q16,
                mirror: m.mirror,
                inliers: m.inliers,
            })
            .collect(),
        topology: topo,
        total_inliers: measure,
        geo_weak_inliers: weak,
        coverage: Some(cov),
        geometry_evidence,
        geo_measurable,
        geo_raw: raw,
        geo_ctl: ctl,
        geo_margin,
        geo_ctl_member: ctl_member,
        swapped,
        calibration: profile.name_str(),
        calibration_id: profile.id_hex16(),
    };
    V42Report {
        base,
        models42,
        diversity_geo: div,
        select_a: if swapped { sel_b } else { sel_a },
        select_b: if swapped { sel_a } else { sel_b },
        kp_a: if swapped { kb.len() } else { ka.len() },
        kp_b: if swapped { ka.len() } else { kb.len() },
    }
}

/// The comparator-42 parity surface: everything comparator 41 reports, plus
/// §8 and the selection provenance.  `to_json_v4` itself is untouched — a
/// shipped surface must not move under a later comparator.
pub fn to_json_v42(r: &V42Report) -> String {
    let base = crate::v4::to_json_v4(&r.base);
    debug_assert!(base.ends_with('}'));
    let d = &r.diversity_geo;
    let res: Vec<String> = r.models42.iter().map(|m| m.median_err.to_string()).collect();
    format!(
        "{},\"geoWeakInliers\":{},\"diversity\":{{\"spatial\":{},\"scale\":{},\"model\":{},\
\"descriptor\":{},\"combined\":{},\"multiplier\":{}}},\"medianErr\":[{}],\
\"selection\":{{\"a\":{},\"b\":{},\"mixed\":{}}},\"kpA\":{},\"kpB\":{},\"screen\":null}}",
        &base[..base.len() - 1],
        r.base.geo_weak_inliers,
        d.spatial,
        d.scale,
        d.model,
        d.descriptor,
        d.combined,
        d.multiplier,
        res.join(","),
        r.select_a,
        r.select_b,
        r.mixed_selection(),
        r.kp_a,
        r.kp_b
    )
}

pub fn screen_json_42(s: &Screen42) -> String {
    format!(
        "{{\"pass\":{},\"poolDirect\":{},\"poolMirror\":{}}}",
        s.pass, s.pool_direct, s.pool_mirror
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{KP_SELECT_LEGACY, MAX_KP_COUNT};
    use crate::keypoints::{pattern, RotCache};
    use crate::wire::{hash, parse_t1, parse_t2, F_KPQ};

    fn image(seed: u64, w: usize, h: usize) -> Vec<u8> {
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let o = (y * w + x) * 4;
                px[o] = ((x * 7 + y * 3 + seed as usize) % 251) as u8;
                px[o + 1] = ((y * 11 + x * 5) % 253) as u8;
                px[o + 2] = (((x ^ y) * 13) % 247) as u8;
                px[o + 3] = 255;
            }
        }
        let mut s = seed as i64;
        let mut r = move |m: usize| -> usize {
            s = (s.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7fffffff;
            (s as usize) % m.max(1)
        };
        for _ in 0..40 {
            let bx = r(w - 10);
            let by = r(h - 10);
            let (cr, cg, cb) = (r(256) as u8, r(256) as u8, r(256) as u8);
            for dy in 0..6 {
                for dx in 0..6 {
                    let o = ((by + dy) * w + bx + dx) * 4;
                    px[o] = cr;
                    px[o + 1] = cg;
                    px[o + 2] = cb;
                }
            }
        }
        px
    }

    fn mirror_px(px: &[u8], w: usize, h: usize) -> Vec<u8> {
        let mut o = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let s = (y * w + (w - 1 - x)) * 4;
                let d = (y * w + x) * 4;
                o[d..d + 4].copy_from_slice(&px[s..s + 4]);
            }
        }
        o
    }

    fn wires(px: &[u8], w: usize, h: usize) -> (Vec<u8>, Vec<u8>) {
        let rot = RotCache::new(&pattern());
        let f = hash(px, w, h, &Config::default(), &rot);
        (f.t1, f.t2)
    }

    #[test]
    fn self_compare_is_identical() {
        let px = image(11, 96, 96);
        let (t1, t2) = wires(&px, 96, 96);
        let r = compare_v42(
            &t1, Some(&t2), &t1, Some(&t2), &Config::default(), &Profile::cal004(), None, None,
        );
        assert_eq!(r.base.comparator, 42);
        assert_eq!(r.base.verdict, "Identical");
        assert!(!r.mixed_selection());
    }

    #[test]
    fn unrelated_never_certifies() {
        let (a1, a2) = wires(&image(21, 96, 96), 96, 96);
        let (b1, b2) = wires(&image(22_777, 96, 96), 96, 96);
        let r = compare_v42(
            &a1, Some(&a2), &b1, Some(&b2), &Config::default(), &Profile::cal004(), None, None,
        );
        assert!(r.base.verdict != "Copy" && r.base.verdict != "Identical", "{}", r.base.verdict);
    }

    #[test]
    fn mirrored_copy_is_certified_and_order_free() {
        let px = image(5, 128, 96);
        let mp = mirror_px(&px, 128, 96);
        let (a1, a2) = wires(&px, 128, 96);
        let (b1, b2) = wires(&mp, 128, 96);
        let c = Config::default();
        let p = Profile::cal004();
        let r = compare_v42(&a1, Some(&a2), &b1, Some(&b2), &c, &p, None, None);
        assert_eq!(r.base.verdict, "Copy", "basis {:?}", r.base.basis);
        assert!(r.models42.iter().any(|m| m.mirror));
        let r2 = compare_v42(&b1, Some(&b2), &a1, Some(&a2), &c, &p, None, None);
        assert_eq!(r.base.verdict, r2.base.verdict);
        assert_eq!(r.base.structural, r2.base.structural);
        assert_eq!(r.base.total_inliers, r2.base.total_inliers);
        assert_eq!(r.diversity_geo, r2.diversity_geo, "diversity is order-free too");
    }

    #[test]
    fn comparators_refuse_each_others_profiles() {
        let (t1, t2) = wires(&image(31, 64, 64), 64, 64);
        let c = Config::default();
        let r = compare_v42(&t1, Some(&t2), &t1, Some(&t2), &c, &Profile::cal003(), None, None);
        assert_eq!(r.base.verdict, "Indeterminate");
        assert_eq!(r.base.reasons, vec![R_PROFILE_UNSUPPORTED]);
        let r41 = crate::v41::compare_v41(
            &t1, Some(&t2), &t1, Some(&t2), &c, &Profile::cal004(), None, None,
        );
        assert_eq!(r41.verdict, "Indeterminate");
        assert_eq!(r41.reasons, vec![R_PROFILE_UNSUPPORTED]);
    }

    /// The budget really is 512, the marker really is on the wire, and a 4.1
    /// wire really does still parse and compare.
    #[test]
    fn selection_provenance_survives_the_wire() {
        let px = image(77, 320, 320);
        let rot = RotCache::new(&pattern());
        let mut c42 = Config::default();
        c42.kp_count = MAX_KP_COUNT;
        let f42 = hash(&px, 320, 320, &c42, &rot);
        let mut c41 = Config::default();
        c41.kp_count = 256;
        c41.kp_select = KP_SELECT_LEGACY;
        let f41 = hash(&px, 320, 320, &c41, &rot);

        assert!(f42.kp_count > 256, "the budget moved: {}", f42.kp_count);
        assert!(f42.kp_count <= MAX_KP_COUNT);
        assert!(f41.kp_count <= 256);
        let t42 = parse_t1(&f42.t1).unwrap();
        let t41 = parse_t1(&f41.t1).unwrap();
        assert!(t42.flags & F_KPQ != 0);
        assert_eq!(t41.flags & F_KPQ, 0);
        assert_eq!(parse_t2(&f42.t2).unwrap().select, 1);
        assert_eq!(parse_t2(&f41.t2).unwrap().select, 0);

        // a mixed pair compares, and says so
        let r = compare_v42(
            &f42.t1, Some(&f42.t2), &f41.t1, Some(&f41.t2),
            &Config::default(), &Profile::cal004(), None, None,
        );
        assert!(r.mixed_selection(), "a 4.1 side against a 4.2 side is flagged");
        assert_ne!(r.base.verdict, "Indeterminate", "flagged, never refused");
    }

    #[test]
    fn json_carries_the_new_channels() {
        let (t1, t2) = wires(&image(41, 96, 96), 96, 96);
        let c = Config::default();
        let p = Profile::cal004();
        let j = to_json_v42(&compare_v42(&t1, Some(&t2), &t1, Some(&t2), &c, &p, None, None));
        assert!(j.contains("\"comparator\":42"));
        assert!(j.contains("\"diversity\":{\"spatial\":"));
        assert!(j.contains("\"selection\":{\"a\":1,\"b\":1,\"mixed\":false}"));
        let s = screen_v42(&t1, Some(&t2), &t1, Some(&t2), &c, &p);
        assert!(s.pass);
        assert!(screen_json_42(&s).contains("\"pass\":true"));
    }
}
