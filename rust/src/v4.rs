//! The v4 comparator entry (SPEC-004) — through milestone M3.
//!
//! `compare_v4` is the explicit comparator-version switch of §24: calling it
//! is choosing comparator 4.  It binds a calibration profile (§8), enforces
//! the live Indeterminate paths (§15), and produces the v4 verdict from the
//! §14 lattice over the rebuilt evidence channels:
//!
//!   local      §10 — assignment matcher, LN-max control, diversity
//!              modulation, calibrated (M2)
//!   geometric  §12 — confidence-weighted votes, multi-model with per-model
//!              mirror, GN family run through the identical multi-model
//!              procedure, calibrated (M3)
//!   structural §14 — profile-weighted combination with the v4 local value
//!              substituted; the other six channels are v3's, unchanged
//!
//! The v3 verdict is still computed and carried in full — it is the
//! comparator-3 reading of the same wires, and M5's calibration campaign
//! compares the two on every corpus category.
//!
//! P4 holds here as it does in v3: the two wires are sorted once by first
//! differing byte, everything runs in canonical order, and the directional
//! outputs are swapped back for the caller at the end.

use crate::calibration::Profile;
use crate::compare::{compare, mirror_side, Verdict};
use crate::config::{chance_correct, idiv, Config, Evidence, RagEndpoint, Scoring, SCALE};
use crate::coverage::Coverage;
use crate::lattice::{lattice_v4, LatticeIn};
use crate::local_v4::{local_v4, LocalV4};
use crate::multimodel::{
    correspond_w, extract_from_pools, gn_control, topology, ModelRec, MultiModel,
};
use crate::wire::{parse_t1, parse_t2, read_sketch};

pub const COMPARATOR: u16 = 4;

pub const R_CORRUPT: &str = "CORRUPT";
pub const R_PROFILE_MISMATCH: &str = "PROFILE_MISMATCH";
pub const R_PROFILE_UNSUPPORTED: &str = "PROFILE_UNSUPPORTED";
/// Raised by callers when `wire::hash_checked` refuses an input (§16).
/// Compare-time work is format-bounded, so `compare_v4` itself never emits it.
pub const R_LIMIT: &str = "LIMIT";

#[derive(Clone, Debug)]
pub struct V4Report {
    pub comparator: u16,
    /// the §14 lattice state, or "Indeterminate"
    pub verdict: &'static str,
    pub class: String,
    /// lattice basis, including fired rule names (R1–R4)
    pub basis: Vec<&'static str>,
    pub reasons: Vec<&'static str>,
    /// profile-weighted structural score with the v4 local evidence
    pub structural: i64,
    pub certifiable: bool,
    /// the comparator-3 reading of the same wires, in full
    pub v3: Option<Verdict>,
    /// §10 — the rebuilt local channel
    pub local: Option<LocalV4>,
    /// §12.3 — models and inlier coordinates are in CANONICAL wire order;
    /// `swapped` says whether that differs from the caller's order
    pub models: Vec<ModelRec>,
    pub topology: u8,
    pub total_inliers: i64,
    /// SPEC-004.1 A1 — the weak-signal inlier count when no model was
    /// accepted; always 0 under comparator 4
    pub geo_weak_inliers: i64,
    pub coverage: Option<Coverage>,
    /// LUT_geo(chance_correct(raw, max over GN family)), currency
    pub geometry_evidence: i64,
    pub geo_measurable: bool,
    /// the §12.3 inputs BEFORE the calibration LUT, for §19 table work:
    /// scaled measurement, scaled control, and the chance-corrected margin
    pub geo_raw: i64,
    pub geo_ctl: i64,
    pub geo_margin: i64,
    /// which GN member supplied the control
    pub geo_ctl_member: &'static str,
    pub swapped: bool,
    pub calibration: String,
    pub calibration_id: String,
}

pub(crate) fn indeterminate(reasons: Vec<&'static str>, p: &Profile) -> V4Report {
    V4Report {
        comparator: COMPARATOR,
        verdict: "Indeterminate",
        class: String::new(),
        basis: Vec::new(),
        reasons,
        structural: 0,
        certifiable: false,
        v3: None,
        local: None,
        models: Vec::new(),
        topology: 0,
        total_inliers: 0,
        geo_weak_inliers: 0,
        coverage: None,
        geometry_evidence: 0,
        geo_measurable: false,
        geo_raw: 0,
        geo_ctl: 0,
        geo_margin: 0,
        geo_ctl_member: "none",
        swapped: false,
        calibration: p.name_str(),
        calibration_id: p.id_hex16(),
    }
}

/// Bind the profile's compare-time choices over the caller's config (§8.1):
/// the profile decides, the config's requests for retired rules are ignored.
pub fn bind(cfg: &Config, p: &Profile) -> Config {
    let mut c = cfg.clone();
    c.hamming_t = p.hamming_t;
    c.confidence_at = p.confidence_at;
    c.geo_conf_at = p.geo_conf_at;
    c.geo_eps = p.geo_eps;
    c.geo_min_corr = p.geo_min_corr as usize;
    c.evidence = Evidence::Lift;
    c.scoring = Scoring::Weighted;
    c.rag_endpoint = RagEndpoint::Rank;
    c
}

pub fn compare_v4(
    a_t1: &[u8],
    a_t2: Option<&[u8]>,
    b_t1: &[u8],
    b_t2: Option<&[u8]>,
    cfg: &Config,
    profile: &Profile,
    hash_profile_a: Option<[u8; 32]>,
    hash_profile_b: Option<[u8; 32]>,
) -> V4Report {
    if profile.validate().is_err() || profile.comparator != COMPARATOR {
        return indeterminate(vec![R_PROFILE_UNSUPPORTED], profile);
    }
    if let (Some(ha), Some(hb)) = (hash_profile_a, hash_profile_b) {
        if ha != hb {
            return indeterminate(vec![R_PROFILE_MISMATCH], profile);
        }
    }
    let cfg = bind(cfg, profile);
    // v3 canonicalises internally with the same rule; both layers therefore
    // agree about which side is A.
    let v3 = match compare(a_t1, a_t2, b_t1, b_t2, &cfg) {
        Ok(v) => v,
        Err(_) => return indeterminate(vec![R_CORRUPT], profile),
    };

    // P4 — canonical argument order for the v4 layer.
    let swapped = match a_t1.iter().zip(b_t1.iter()).find(|(x, y)| x != y) {
        Some((x, y)) => x > y,
        None => false,
    };
    let (c_a1, c_a2, c_b1, c_b2) =
        if swapped { (b_t1, b_t2, a_t1, a_t2) } else { (a_t1, a_t2, b_t1, b_t2) };

    // compare() already validated both wires; a failure here is a logic
    // error, not input error.
    let (ta, tb) = (parse_t1(c_a1).unwrap(), parse_t1(c_b1).unwrap());
    let mut loc = local_v4(&ta, &tb, profile);
    let ka = match c_a2.map(parse_t2) {
        Some(Ok(t2)) => t2.list,
        _ => read_sketch(&ta),
    };
    let kb = match c_b2.map(parse_t2) {
        Some(Ok(t2)) => t2.list,
        _ => read_sketch(&tb),
    };
    let (mda, mdb) = (ta.max_dim(), tb.max_dim());
    let xmax_a = idiv((ta.width as i64 - 1) * 65535, mda.max(1)).clamp(0, 65535) as i32;

    // §12 — pools once, shared by the measurement and its control.
    let am = mirror_side(&ka, xmax_a);
    let pd = correspond_w(&ka, &kb);
    let pm = if cfg.mirror_hypothesis { correspond_w(&am, &kb) } else { Vec::new() };
    let mmi = profile.min_model_inliers as i64;
    let maxm = profile.max_models as usize;
    let mm: MultiModel =
        extract_from_pools(&ka, &am, &kb, pd.clone(), pm.clone(), &cfg, mda, mdb, mmi, maxm);
    let (topo, cov) = topology(&mm, profile.grid_g, profile.thresholds[8] as i64, cfg.geo_min_corr);
    let (ctl_raw, ctl_member) =
        gn_control(&ka, &am, &kb, &pd, &pm, &cfg, mda, mdb, mmi, maxm);

    let geo_measurable = ka.len() >= cfg.geo_min_corr && kb.len() >= cfg.geo_min_corr;
    let raw = (mm.total_inliers * SCALE / cfg.geo_conf_at as i64).clamp(0, SCALE);
    let ctl = (ctl_raw * SCALE / cfg.geo_conf_at as i64).clamp(0, SCALE);
    let geo_margin = chance_correct(raw, ctl);
    let geometry_evidence = profile.lut_geometry.eval(geo_margin);

    // §14 — the lattice, over v4 evidence.  The six non-local structural
    // channels are v3's; the local slot carries the §10 channel's output.
    let mut channels: [(&'static str, i64, bool); 7] = [("", 0, false); 7];
    for (k, (name, ch)) in v3.channels.iter().enumerate() {
        channels[k] = if *name == "local" {
            (*name, loc.evidence, loc.measurable)
        } else {
            (*name, ch.value, ch.measurable)
        };
    }
    let latin = LatticeIn {
        identical: v3.identical,
        channels,
        geo_measurable,
        geo_evidence: geometry_evidence,
        total_inliers: mm.total_inliers,
        topology_class: topo,
        diversity: loc.diversity,
        coverage_min: loc.coverage_a.coverage.min(loc.coverage_b.coverage),
        any_mirror_model: mm.models.iter().any(|m| m.mirror),
    };
    let verdict = lattice_v4(&latin, profile);

    // Directional outputs back to the caller's order (P4).
    if swapped {
        std::mem::swap(&mut loc.coverage_a, &mut loc.coverage_b);
        std::mem::swap(&mut loc.d_a, &mut loc.d_b);
        loc.pairs = loc.pairs.iter().map(|&(i, j)| (j, i)).collect();
    }

    V4Report {
        comparator: COMPARATOR,
        verdict: verdict.state,
        class: verdict.class,
        basis: verdict.basis,
        reasons: Vec::new(),
        structural: verdict.structural,
        certifiable: verdict.certifiable,
        v3: Some(v3),
        local: Some(loc),
        models: mm.models,
        topology: topo,
        total_inliers: mm.total_inliers,
        geo_weak_inliers: 0,
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
    }
}

// ------------------------------------------------------------------ JSON
// The cross-engine parity surface (test/parity4.mjs).  Hand-rolled like v3's
// to_json; camelCase keys match the JavaScript report so the two engines'
// outputs can be compared object-for-object with no mapping layer.

pub(crate) fn jesc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(crate) fn jcov(c: &Coverage) -> String {
    let counts: Vec<String> = c.counts.iter().map(|v| v.to_string()).collect();
    format!(
        "{{\"g\":{},\"occupied\":{},\"coverage\":{},\"bboxCells\":{},\"concentration\":{},\"counts\":[{}]}}",
        c.g, c.occupied, c.coverage, c.bbox_cells, c.concentration, counts.join(",")
    )
}

pub fn to_json_v4(r: &V4Report) -> String {
    let strs = |v: &Vec<&'static str>| -> String {
        v.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(",")
    };
    let models: Vec<String> = r
        .models
        .iter()
        .map(|m| {
            format!(
                "{{\"r00\":{},\"r10\":{},\"tx\":{},\"ty\":{},\"scaleQ16\":{},\"mirror\":{},\"inliers\":{}}}",
                m.r00, m.r10, m.tx, m.ty, m.scale_q16, m.mirror, m.inliers
            )
        })
        .collect();
    let local = match &r.local {
        None => "null".into(),
        Some(l) => format!(
            "{{\"measurable\":{},\"note\":\"{}\",\"matches\":{},\"cmax\":{},\"w\":{},\"cap\":{},\
\"ctlW\":{},\"ctlN\":{},\"ctlMember\":\"{}\",\"liftRaw\":{},\"liftCtl\":{},\"margin\":{},\
\"evidence\":{},\"propRaw\":{},\"propCtl\":{},\"propMargin\":{},\"diversity\":{},\"dA\":{},\"dB\":{},\
\"coverageA\":{},\"coverageB\":{}}}",
            l.measurable, jesc(&l.note), l.matches, l.cmax, l.w, l.cap,
            l.ctl_w, l.ctl_n, l.ctl_member, l.lift_raw, l.lift_ctl, l.margin,
            l.evidence, l.prop_raw, l.prop_ctl, l.prop_margin, l.diversity, l.d_a, l.d_b,
            jcov(&l.coverage_a), jcov(&l.coverage_b)
        ),
    };
    format!(
        "{{\"comparator\":{},\"verdict\":\"{}\",\"class\":\"{}\",\"basis\":[{}],\"reasons\":[{}],\
\"structural\":{},\"certifiable\":{},\"v3Verdict\":\"{}\",\"topology\":{},\"totalInliers\":{},\
\"geometryEvidence\":{},\"geoMeasurable\":{},\"geoRaw\":{},\"geoCtl\":{},\"geoMargin\":{},\"geoCtlMember\":\"{}\",\"swapped\":{},\"calibration\":\"{}\",\
\"calibrationId\":\"{}\",\"models\":[{}],\"coverage\":{},\"local\":{}}}",
        r.comparator, r.verdict, jesc(&r.class), strs(&r.basis), strs(&r.reasons),
        r.structural, r.certifiable,
        r.v3.as_ref().map(|v| v.verdict).unwrap_or(""),
        r.topology, r.total_inliers,
        r.geometry_evidence, r.geo_measurable, r.geo_raw, r.geo_ctl, r.geo_margin,
        r.geo_ctl_member, r.swapped, jesc(&r.calibration),
        jesc(&r.calibration_id), models.join(","),
        match &r.coverage { Some(c) => jcov(c), None => "null".into() },
        local
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keypoints::{pattern, RotCache};
    use crate::wire::hash;

    /// The parity suite's known-good generator (test/parity.mjs work()),
    /// ported: per-pixel gradients with 40 flat 6x6 blocks — plenty of FAST
    /// corners, distinctive descriptors.  Different seeds share statistics.
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

    fn wires_px(px: &[u8], w: usize, h: usize) -> (Vec<u8>, Vec<u8>) {
        let cfg = Config::default();
        let rot = RotCache::new(&pattern());
        let fp = hash(px, w, h, &cfg, &rot);
        (fp.t1, fp.t2)
    }

    fn wires(seed: u64) -> (Vec<u8>, Vec<u8>) {
        wires_px(&image(seed, 128, 128), 128, 128)
    }

    #[test]
    fn self_compare_is_identical_with_a_dominant_model() {
        let cfg = Config::default();
        let p = Profile::cal001();
        let (t1, t2) = wires(42);
        let r = compare_v4(&t1, Some(&t2), &t1, Some(&t2), &cfg, &p, None, None);
        assert_eq!(r.verdict, "Identical");
        assert_eq!(r.basis, vec!["bytes"]);
        assert_eq!(r.structural, SCALE);
        assert!(r.reasons.is_empty());
        assert!(!r.models.is_empty(), "self-similarity is one big model");
        assert!(!r.models[0].mirror);
        assert!(r.topology >= 2, "topology {}", r.topology);
        assert!(r.geometry_evidence > 5000, "evidence {}", r.geometry_evidence);
        assert_eq!(r.calibration, "CAL-001-PROVISIO");
        assert_eq!(r.comparator, 4);
        let loc = r.local.as_ref().unwrap();
        assert!(loc.measurable);
        assert!(loc.evidence > 7000, "local evidence {}", loc.evidence);
        assert!(loc.diversity > 0 && loc.diversity <= 10000);
        assert!(loc.coverage_a.occupied >= 2);
        let cov = r.coverage.unwrap();
        assert!(cov.occupied > 1);
    }

    #[test]
    fn unrelated_images_earn_less_than_self() {
        let cfg = Config::default();
        let p = Profile::cal001();
        let (a1, a2) = wires(42);
        let (b1, b2) = wires(1337);
        let rs = compare_v4(&a1, Some(&a2), &a1, Some(&a2), &cfg, &p, None, None);
        let ru = compare_v4(&a1, Some(&a2), &b1, Some(&b2), &cfg, &p, None, None);
        assert_ne!(ru.verdict, "Identical");
        assert_ne!(ru.verdict, "Copy");
        assert!(ru.geometry_evidence < rs.geometry_evidence);
        assert!(ru.total_inliers < rs.total_inliers);
        let (ls, lu) = (rs.local.as_ref().unwrap(), ru.local.as_ref().unwrap());
        assert!(lu.evidence < ls.evidence, "local: unrelated {} vs self {}", lu.evidence, ls.evidence);
    }

    /// P4 for the v4 layer: swapping the arguments changes nothing except
    /// which directional slot each side occupies.
    #[test]
    fn argument_order_symmetry() {
        let cfg = Config::default();
        let p = Profile::cal001();
        let (a1, a2) = wires(42);
        let (b1, b2) = wires(1337);
        let ab = compare_v4(&a1, Some(&a2), &b1, Some(&b2), &cfg, &p, None, None);
        let ba = compare_v4(&b1, Some(&b2), &a1, Some(&a2), &cfg, &p, None, None);
        assert_eq!(ab.verdict, ba.verdict);
        assert_eq!(ab.class, ba.class);
        assert_eq!(ab.basis, ba.basis);
        assert_eq!(ab.structural, ba.structural);
        assert_eq!(ab.geometry_evidence, ba.geometry_evidence);
        assert_eq!(ab.total_inliers, ba.total_inliers);
        assert_eq!(ab.topology, ba.topology);
        assert_ne!(ab.swapped, ba.swapped, "exactly one call order is canonical");
        let (la, lb) = (ab.local.as_ref().unwrap(), ba.local.as_ref().unwrap());
        assert_eq!(la.evidence, lb.evidence);
        assert_eq!(la.diversity, lb.diversity);
        assert_eq!(la.coverage_a, lb.coverage_b, "caller-A's coverage, both orders");
        assert_eq!(la.coverage_b, lb.coverage_a);
    }

    /// A mirrored copy: the mirror hypothesis pool carries the models, and
    /// the per-model flag says so (§12.3).
    #[test]
    fn mirrored_copy_finds_mirror_models() {
        let cfg = Config::default();
        let p = Profile::cal001();
        let px = image(42, 128, 128);
        let (a1, a2) = wires_px(&px, 128, 128);
        let mpx = mirror_px(&px, 128, 128);
        let (b1, b2) = wires_px(&mpx, 128, 128);
        let r = compare_v4(&a1, Some(&a2), &b1, Some(&b2), &cfg, &p, None, None);
        assert!(!r.models.is_empty(), "a mirrored copy is still one placement");
        assert!(r.models.iter().any(|m| m.mirror), "the mirror pool wins it");
        assert_ne!(r.verdict, "Unrelated");
        assert_ne!(r.verdict, "Related");
    }

    #[test]
    fn indeterminate_paths() {
        let cfg = Config::default();
        let p = Profile::cal001();
        let (t1, t2) = wires(7);
        // corruption: flip one byte inside the Tier-1 body
        let mut bad = t1.clone();
        let mid = bad.len() / 2;
        bad[mid] ^= 0x40;
        let r = compare_v4(&t1, Some(&t2), &bad, Some(&t2), &cfg, &p, None, None);
        assert_eq!(r.verdict, "Indeterminate");
        assert_eq!(r.reasons, vec![R_CORRUPT]);
        assert!(r.v3.is_none());
        // hash-profile mismatch
        let r = compare_v4(&t1, Some(&t2), &t1, Some(&t2), &cfg, &p, Some([1; 32]), Some([2; 32]));
        assert_eq!(r.verdict, "Indeterminate");
        assert_eq!(r.reasons, vec![R_PROFILE_MISMATCH]);
        // unsupported profile
        let mut broken = Profile::cal001();
        broken.grid_g = 1;
        let r = compare_v4(&t1, Some(&t2), &t1, Some(&t2), &cfg, &p, None, None);
        assert!(r.reasons.is_empty());
        let r = compare_v4(&t1, Some(&t2), &t1, Some(&t2), &cfg, &broken, None, None);
        assert_eq!(r.reasons, vec![R_PROFILE_UNSUPPORTED]);
    }

    #[test]
    fn retired_rules_are_ignored_under_a_profile() {
        // A config asking for Proportion/Gate must not change a v4 result.
        let p = Profile::cal001();
        let (t1, t2) = wires(9);
        let c1 = Config::default();
        let mut c2 = Config::default();
        c2.evidence = Evidence::Proportion;
        c2.scoring = Scoring::Gate;
        c2.hamming_t = 3; // profile overrides this too
        let r1 = compare_v4(&t1, Some(&t2), &t1, Some(&t2), &c1, &p, None, None);
        let r2 = compare_v4(&t1, Some(&t2), &t1, Some(&t2), &c2, &p, None, None);
        assert_eq!(r1.verdict, r2.verdict);
        assert_eq!(r1.structural, r2.structural);
        assert_eq!(r1.geometry_evidence, r2.geometry_evidence);
        assert_eq!(r1.total_inliers, r2.total_inliers);
    }
}
