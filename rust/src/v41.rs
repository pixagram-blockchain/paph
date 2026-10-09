//! Comparator 41 (SPEC-004.1) — milestone M6.
//!
//! Everything comparator 4 is, plus exactly the four amendments:
//!
//!   A1  §12.3 weak-signal geometry: when the extraction accepts no model,
//!       the measurement is the best single verified model's inlier count
//!       (member floor 3 kept, acceptance floor absent); every GN member
//!       runs the identical rule.  No accepted model ⇒ topology stays
//!       scattered ⇒ R3 keeps the weak signal away from solo certification.
//!   A2  Appendix A on the edge-induced subgraph — the local channel's
//!       assignment via `assign_sparse` (cardinality- and cost-identical to
//!       the full matrix; tie cases may differ, which is why 41 exists).
//!   A3  container-2 profiles: each structural channel's value passes
//!       through its own calibration table before the lattice.
//!   A4  the stage-1 screen — advisory, never a verdict.
//!
//! Comparator 4 stays frozen in `v4.rs`; a profile targeting one comparator
//! is refused by the other, in both directions.

use crate::calibration::{Profile, COMPARATOR_V41};
use crate::compare::{compare, mirror_side};
use crate::config::{chance_correct, idiv, Config, SCALE};
use crate::lattice::{lattice_v4, LatticeIn};
use crate::local_v4::local_v4_41;
use crate::multimodel::{correspond_w, geo_measure_41, gn_control_41, topology, MultiModel};
use crate::v4::{
    bind, indeterminate, V4Report, R_CORRUPT, R_PROFILE_MISMATCH, R_PROFILE_UNSUPPORTED,
};
use crate::wire::{parse_t1, parse_t2, read_sketch};

/// §A4 — the stage-1 screen: correspondences only, no verification, no
/// verdict.  A pair the screen rejects is UNSCREENED, never Unrelated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    pub pass: bool,
    pub pool_direct: usize,
    pub pool_mirror: usize,
}

pub fn screen_v41(
    a_t1: &[u8],
    a_t2: Option<&[u8]>,
    b_t1: &[u8],
    b_t2: Option<&[u8]>,
    cfg: &Config,
    profile: &Profile,
) -> Screen {
    let cfg = bind(cfg, profile);
    let swapped = match a_t1.iter().zip(b_t1.iter()).find(|(x, y)| x != y) {
        Some((x, y)) => x > y,
        None => false,
    };
    let (c_a1, c_a2, c_b1, c_b2) =
        if swapped { (b_t1, b_t2, a_t1, a_t2) } else { (a_t1, a_t2, b_t1, b_t2) };
    let (ta, tb) = match (parse_t1(c_a1), parse_t1(c_b1)) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return Screen { pass: false, pool_direct: 0, pool_mirror: 0 },
    };
    let ka = match c_a2.map(parse_t2) {
        Some(Ok(t2)) => t2.list,
        _ => read_sketch(&ta),
    };
    let kb = match c_b2.map(parse_t2) {
        Some(Ok(t2)) => t2.list,
        _ => read_sketch(&tb),
    };
    let mda = ta.max_dim();
    let xmax_a = idiv((ta.width as i64 - 1) * 65535, mda.max(1)).clamp(0, 65535) as i32;
    let am = mirror_side(&ka, xmax_a);
    let pd = correspond_w(&ka, &kb);
    let pm = if cfg.mirror_hypothesis { correspond_w(&am, &kb) } else { Vec::new() };
    Screen {
        pass: pd.len().max(pm.len()) >= cfg.geo_min_corr,
        pool_direct: pd.len(),
        pool_mirror: pm.len(),
    }
}

/// SPEC-004.1 — the comparator-41 verdict.
pub fn compare_v41(
    a_t1: &[u8],
    a_t2: Option<&[u8]>,
    b_t1: &[u8],
    b_t2: Option<&[u8]>,
    cfg: &Config,
    profile: &Profile,
    hash_profile_a: Option<[u8; 32]>,
    hash_profile_b: Option<[u8; 32]>,
) -> V4Report {
    if profile.validate().is_err() || profile.comparator != COMPARATOR_V41 {
        let mut r = indeterminate(vec![R_PROFILE_UNSUPPORTED], profile);
        r.comparator = COMPARATOR_V41;
        return r;
    }
    if let (Some(ha), Some(hb)) = (hash_profile_a, hash_profile_b) {
        if ha != hb {
            let mut r = indeterminate(vec![R_PROFILE_MISMATCH], profile);
            r.comparator = COMPARATOR_V41;
            return r;
        }
    }
    let cfg = bind(cfg, profile);
    let v3 = match compare(a_t1, a_t2, b_t1, b_t2, &cfg) {
        Ok(v) => v,
        Err(e) => {
            let why = if e == crate::wire::E_WIRE_MISMATCH { crate::wire::R_WIRE_MISMATCH } else { R_CORRUPT };
            let mut r = indeterminate(vec![why], profile);
            r.comparator = COMPARATOR_V41;
            return r;
        }
    };

    // P4 — canonical argument order, the v4 rule verbatim.
    let swapped = match a_t1.iter().zip(b_t1.iter()).find(|(x, y)| x != y) {
        Some((x, y)) => x > y,
        None => false,
    };
    let (c_a1, c_a2, c_b1, c_b2) =
        if swapped { (b_t1, b_t2, a_t1, a_t2) } else { (a_t1, a_t2, b_t1, b_t2) };
    let (ta, tb) = (parse_t1(c_a1).unwrap(), parse_t1(c_b1).unwrap());

    // A2 — the sparse-assignment local channel.
    let mut loc = local_v4_41(&ta, &tb, profile);

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

    // A1 — extraction first, the weak signal only where it accepts nothing;
    // the control family runs the identical measurement.
    let (mm, measure, weak): (MultiModel, i64, i64) =
        geo_measure_41(&ka, &am, &kb, &pd, &pm, &cfg, mda, mdb, mmi, maxm);
    let (topo, cov) = topology(&mm, profile.grid_g, profile.thresholds[8] as i64, cfg.geo_min_corr);
    let (ctl_raw, ctl_member) =
        gn_control_41(&ka, &am, &kb, &pd, &pm, &cfg, mda, mdb, mmi, maxm);

    let geo_measurable = ka.len() >= cfg.geo_min_corr && kb.len() >= cfg.geo_min_corr;
    let raw = (measure * SCALE / cfg.geo_conf_at as i64).clamp(0, SCALE);
    let ctl = (ctl_raw * SCALE / cfg.geo_conf_at as i64).clamp(0, SCALE);
    let geo_margin = chance_correct(raw, ctl);
    let geometry_evidence = profile.lut_geometry.eval(geo_margin);

    // §14 over A3-transformed channels: the local slot carries the §10
    // channel's evidence (its tables applied inside local_v4_41); every
    // other structural channel passes through its container-2 table.
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

    V4Report {
        comparator: COMPARATOR_V41,
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
    }
}

/// The comparator-41 parity surface: everything `to_json_v4` says, plus the
/// A1 report field.  `to_json_v4` itself is untouched — the comparator-4
/// surface must not move.
pub fn to_json_v41(r: &V4Report) -> String {
    let base = crate::v4::to_json_v4(r);
    debug_assert!(base.ends_with('}'));
    format!(
        "{},\"geoWeakInliers\":{},\"screen\":null}}",
        &base[..base.len() - 1],
        r.geo_weak_inliers
    )
}

pub fn screen_json(s: &Screen) -> String {
    format!(
        "{{\"pass\":{},\"poolDirect\":{},\"poolMirror\":{}}}",
        s.pass, s.pool_direct, s.pool_mirror
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibration::Profile;
    use crate::config::Config;
    use crate::keypoints::{pattern, RotCache};
    use crate::wire::hash;

    /// Same generator as the v4 suite: gradients + 40 flat blocks.
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
        let r = compare_v41(
            &t1, Some(&t2), &t1, Some(&t2), &Config::default(), &Profile::cal003(), None, None,
        );
        assert_eq!(r.comparator, 41);
        assert_eq!(r.verdict, "Identical");
    }

    #[test]
    fn unrelated_never_certifies() {
        let (a1, a2) = wires(&image(21, 96, 96), 96, 96);
        let (b1, b2) = wires(&image(22_777, 96, 96), 96, 96);
        let r = compare_v41(
            &a1, Some(&a2), &b1, Some(&b2), &Config::default(), &Profile::cal003(), None, None,
        );
        assert!(r.verdict != "Copy" && r.verdict != "Identical", "{}", r.verdict);
    }

    #[test]
    fn mirrored_copy_is_certified_with_a_mirror_model() {
        let px = image(5, 128, 96);
        let mp = mirror_px(&px, 128, 96);
        let (a1, a2) = wires(&px, 128, 96);
        let (b1, b2) = wires(&mp, 128, 96);
        let c = Config::default();
        let p = Profile::cal003();
        let r = compare_v41(&a1, Some(&a2), &b1, Some(&b2), &c, &p, None, None);
        assert_eq!(r.verdict, "Copy", "basis {:?}", r.basis);
        assert!(r.models.iter().any(|m| m.mirror));
        let r2 = compare_v41(&b1, Some(&b2), &a1, Some(&a2), &c, &p, None, None);
        assert_eq!(r.verdict, r2.verdict);
        assert_eq!(r.structural, r2.structural);
        assert_eq!(r.total_inliers, r2.total_inliers);
    }

    #[test]
    fn comparators_refuse_each_others_profiles() {
        let (t1, t2) = wires(&image(31, 64, 64), 64, 64);
        let c = Config::default();
        let r = compare_v41(&t1, Some(&t2), &t1, Some(&t2), &c, &Profile::cal001(), None, None);
        assert_eq!(r.verdict, "Indeterminate");
        assert_eq!(r.reasons, vec![R_PROFILE_UNSUPPORTED]);
        let r4 = crate::v4::compare_v4(
            &t1, Some(&t2), &t1, Some(&t2), &c, &Profile::cal003(), None, None,
        );
        assert_eq!(r4.verdict, "Indeterminate");
        assert_eq!(r4.reasons, vec![R_PROFILE_UNSUPPORTED]);
    }

    #[test]
    fn screen_reports_pools_and_never_verdicts() {
        let (a1, a2) = wires(&image(41, 96, 96), 96, 96);
        let (b1, b2) = wires(&image(90_001, 96, 96), 96, 96);
        let c = Config::default();
        let p = Profile::cal003();
        let s = screen_v41(&a1, Some(&a2), &a1, Some(&a2), &c, &p);
        assert!(s.pass);
        assert!(s.pool_direct >= 8);
        let s2 = screen_v41(&a1, Some(&a2), &b1, Some(&b2), &c, &p);
        assert_eq!(s2.pass, s2.pool_direct.max(s2.pool_mirror) >= 8);
        let j = to_json_v41(&compare_v41(&a1, Some(&a2), &a1, Some(&a2), &c, &p, None, None));
        assert!(j.contains("\"geoWeakInliers\":0"));
        assert!(screen_json(&s).contains("\"pass\":true"));
    }
}
