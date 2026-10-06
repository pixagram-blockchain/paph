//! The v4 verdict lattice (SPEC-004 §14) — milestone M3.
//!
//! Rule SHAPES live here, comparator-versioned and tested by golden vectors;
//! rule PARAMETERS live in the calibration profile.  A profile cannot invent
//! a rule; it can only move one (§14.1).  The base lattice is v3's, restated
//! with profile parameters: two axes, never averaged.  On top, the four v4
//! rules — each appends its name to `basis` when it fires, because a capped
//! verdict that cannot say which rule capped it is indistinguishable from a
//! threshold miss, and moderators appeal the difference (§14.3).
//!
//! The inputs are already evidence, not measurements: the local value is the
//! §10 channel's calibrated output (LUT and diversity modulation included),
//! and the geometric value is the §12.3 evidence over the GN family.

use crate::calibration::Profile;
use crate::config::SCALE;

pub struct LatticeIn {
    pub identical: bool,
    /// (name, value, measurable) in the fixed order
    /// dct/local/shape/topology/runs/palette/silhouette.  The LOCAL entry
    /// carries `LocalV4.evidence`; the other six carry v3's channel values.
    pub channels: [(&'static str, i64, bool); 7],
    pub geo_measurable: bool,
    /// calibrated §12.3 evidence over the GN family
    pub geo_evidence: i64,
    pub total_inliers: i64,
    /// §13 class 0–4
    pub topology_class: u8,
    /// §10.4, currency
    pub diversity: i64,
    /// min(coverage_a, coverage_b).coverage from the local channel (§11)
    pub coverage_min: i64,
    pub any_mirror_model: bool,
}

pub struct V4Verdict {
    pub state: &'static str,
    pub class: String,
    pub basis: Vec<&'static str>,
    /// profile-weighted structural score with the v4 local evidence
    pub structural: i64,
    /// R1: local measurable AND ≥ min_secondaries other measurable channels
    pub certifiable: bool,
    pub secondaries: usize,
}

fn widx(name: &str) -> usize {
    match name {
        "local" => 0,
        "shape" => 1,
        "topology" => 2,
        "runs" => 3,
        "dct" => 4,
        "palette" => 5,
        "silhouette" => 6,
        _ => usize::MAX,
    }
}

pub fn lattice_v4(x: &LatticeIn, p: &Profile) -> V4Verdict {
    // Weighted structural score, v3 semantics with profile weights: measurable
    // channels only, weights renormalised over what measured.
    let (mut wsum, mut wtot) = (0i64, 0i64);
    let mut secondaries = 0usize;
    let mut local_measurable = false;
    for (name, v, m) in x.channels.iter() {
        if !*m {
            continue;
        }
        let w = p.weights[widx(name)] as i64;
        wsum += v * w;
        wtot += w;
        if *name == "local" {
            local_measurable = true;
        } else {
            secondaries += 1;
        }
    }
    let mut structural = if wtot > 0 { wsum / wtot } else { 0 };

    let t = &p.thresholds;
    // [0] identical  [1] strong  [2] moderate  [3] weak  [4] solo
    // [5] geo strong [6] geo weak [7] geo solo inliers [8] topology dominant
    let certifiable = local_measurable && secondaries >= p.min_secondaries as usize; // R1 shape
    let would_certify = local_measurable && structural >= t[1] as i64;
    let r1_fired = would_certify && !certifiable;

    let gv = if x.geo_measurable { x.geo_evidence } else { 0 };
    let geo_strong = x.geo_measurable && gv >= t[5] as i64;
    let geo_weak = x.geo_measurable && gv >= t[6] as i64;
    let s_strong = certifiable && structural >= t[1] as i64;
    let s_mod = certifiable && structural >= t[2] as i64;
    let s_rel = structural >= t[3] as i64;

    let mut basis: Vec<&'static str> = Vec::new();
    let state: &'static str;
    let class: String;

    if x.identical {
        state = "Identical";
        class = "byte-identical fingerprint".into();
        basis.push("bytes");
        structural = SCALE;
    } else if s_strong && geo_strong {
        state = "Copy";
        class = "certified — structure and geometry agree".into();
        basis.push("structural");
        basis.push("geometric");
    } else if s_strong && structural >= t[4] as i64 {
        // the recolour arm — R4: structure alone must show spatial support
        if x.coverage_min >= p.coverage_floor as i64 {
            state = "Copy";
            class = "structural only — no geometric corroboration".into();
            basis.push("structural");
        } else {
            state = "Suspected";
            class = "capped — structural agreement without spatial support".into();
            basis.push("structural");
            basis.push("R4:support");
        }
    } else if geo_strong && x.total_inliers >= t[7] as i64 {
        // the crop/collage arm — R3: geometry alone must show a coherent region
        if matches!(x.topology_class, 2 | 3 | 4) {
            state = "Copy";
            class = if x.any_mirror_model {
                "geometric only — mirrored crop / collage class".into()
            } else {
                "geometric only — crop / collage class".into()
            };
            basis.push("geometric");
        } else {
            state = "Suspected";
            class = "capped — geometry scattered, no coherent region".into();
            basis.push("geometric");
            basis.push("R3:scatter");
        }
    } else if s_strong || s_mod || geo_strong || geo_weak {
        state = "Suspected";
        class = if geo_strong {
            "geometry agrees but below the solo bar".into()
        } else if s_strong {
            "structure agrees but below the solo bar".into()
        } else {
            "partial agreement".into()
        };
        if s_mod {
            basis.push("structural");
        }
        if geo_weak {
            basis.push("geometric");
        }
    } else if s_rel {
        state = "Related";
        class = "same family, not a copy".into();
    } else {
        state = "Unrelated";
        class = "no agreement above chance".into();
    }

    let (mut state, mut class) = (state, class);
    if r1_fired && state != "Identical" {
        basis.push("R1:corroboration");
    }
    // R2 — the repetition guard: a wall of tiles with no geometric support
    // cannot certify, whatever the structural channels say (§14.3).
    if state == "Copy" && local_measurable && x.diversity < p.rep_extreme_at as i64 && !geo_weak {
        state = "Suspected";
        class = "capped — extreme repetition without geometric support".into();
        basis.push("R2:repetition");
    }

    V4Verdict { state, class, basis, structural, certifiable, secondaries }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> LatticeIn {
        LatticeIn {
            identical: false,
            channels: [
                ("dct", 6000, true),
                ("local", 6000, true),
                ("shape", 6000, true),
                ("topology", 6000, true),
                ("runs", 6000, true),
                ("palette", 6000, true),
                ("silhouette", 6000, true),
            ],
            geo_measurable: true,
            geo_evidence: 4000,
            total_inliers: 20,
            topology_class: 4,
            diversity: 8000,
            coverage_min: 5000,
            any_mirror_model: false,
        }
    }

    fn all(v: i64, x: &mut LatticeIn) {
        for c in x.channels.iter_mut() {
            c.1 = v;
        }
    }

    #[test]
    fn base_states_in_order() {
        let p = Profile::cal001();
        let mut x = base();
        x.identical = true;
        let v = lattice_v4(&x, &p);
        assert_eq!((v.state, v.structural), ("Identical", SCALE));
        assert_eq!(v.basis, vec!["bytes"]);

        let x = base(); // 6000 everywhere, geo 4000 → both strong
        let v = lattice_v4(&x, &p);
        assert_eq!(v.state, "Copy");
        assert_eq!(v.basis, vec!["structural", "geometric"]);
        assert_eq!(v.structural, 6000);
        assert!(v.certifiable && v.secondaries == 6);

        let mut x = base();
        all(3200, &mut x);
        x.geo_evidence = 0; // moderate structure, no geometry
        let v = lattice_v4(&x, &p);
        assert_eq!(v.state, "Suspected");
        assert_eq!(v.basis, vec!["structural"]);

        let mut x = base();
        all(2000, &mut x);
        x.geo_evidence = 0;
        assert_eq!(lattice_v4(&x, &p).state, "Related");

        let mut x = base();
        all(500, &mut x);
        x.geo_evidence = 0;
        let v = lattice_v4(&x, &p);
        assert_eq!(v.state, "Unrelated");
        assert!(v.basis.is_empty());
    }

    #[test]
    fn r1_corroboration_floor() {
        let p = Profile::cal001();
        let mut x = base();
        all(9000, &mut x);
        // only shape and dct measurable besides local: 2 secondaries < 3
        for c in x.channels.iter_mut() {
            if !matches!(c.0, "local" | "shape" | "dct") {
                c.2 = false;
            }
        }
        x.geo_evidence = 0;
        let v = lattice_v4(&x, &p);
        assert!(!v.certifiable);
        assert_eq!(v.secondaries, 2);
        assert_ne!(v.state, "Copy");
        assert!(v.basis.contains(&"R1:corroboration"), "{:?}", v.basis);
        // with a third secondary the same numbers certify
        let mut x2 = x;
        x2.channels[4].2 = true; // runs back
        x2.coverage_min = 5000;
        let v2 = lattice_v4(&x2, &p);
        assert_eq!(v2.state, "Copy");
        assert!(!v2.basis.contains(&"R1:corroboration"));
    }

    #[test]
    fn r2_repetition_guard() {
        let p = Profile::cal001();
        let mut x = base();
        all(7500, &mut x); // struct-solo territory
        x.geo_measurable = false;
        x.geo_evidence = 0;
        x.diversity = 800; // below rep_extreme_at 1500
        let v = lattice_v4(&x, &p);
        assert_eq!(v.state, "Suspected");
        assert!(v.basis.contains(&"R2:repetition"), "{:?}", v.basis);
        // weak geometric support disarms R2
        let mut x2 = base();
        all(7500, &mut x2);
        x2.diversity = 800;
        x2.geo_evidence = 1500; // ≥ GEO_WEAK 1200
        let v2 = lattice_v4(&x2, &p);
        assert_eq!(v2.state, "Copy");
        assert!(!v2.basis.contains(&"R2:repetition"));
    }

    #[test]
    fn r3_scatter_guard() {
        let p = Profile::cal001();
        let mut x = base();
        all(500, &mut x); // structure silent
        x.geo_evidence = 4000;
        x.total_inliers = 20;
        x.topology_class = 1; // scattered
        let v = lattice_v4(&x, &p);
        assert_eq!(v.state, "Suspected");
        assert!(v.basis.contains(&"R3:scatter"), "{:?}", v.basis);
        x.topology_class = 2;
        let v = lattice_v4(&x, &p);
        assert_eq!(v.state, "Copy");
        assert_eq!(v.basis, vec!["geometric"]);
        x.any_mirror_model = true;
        assert!(lattice_v4(&x, &p).class.contains("mirrored"));
    }

    #[test]
    fn r4_support_guard() {
        let p = Profile::cal001();
        let mut x = base();
        all(7500, &mut x); // struct-solo, geometry silent
        x.geo_measurable = false;
        x.geo_evidence = 0;
        x.coverage_min = 1200; // below coverage_floor 2500
        let v = lattice_v4(&x, &p);
        assert_eq!(v.state, "Suspected");
        assert!(v.basis.contains(&"R4:support"), "{:?}", v.basis);
        x.coverage_min = 2500;
        let v = lattice_v4(&x, &p);
        assert_eq!(v.state, "Copy");
        assert_eq!(v.basis, vec!["structural"]);
    }

    #[test]
    fn axes_never_averaged() {
        // a huge structural score with silent geometry cannot reach the
        // certified-both state, and vice versa — there is no blended path.
        let p = Profile::cal001();
        let mut x = base();
        all(9800, &mut x);
        x.geo_measurable = false;
        x.geo_evidence = 0;
        let v = lattice_v4(&x, &p);
        assert_eq!(v.basis, vec!["structural"], "solo arm, never 'both'");
        let mut x = base();
        all(0, &mut x);
        x.geo_evidence = 9800;
        x.total_inliers = 60;
        let v = lattice_v4(&x, &p);
        assert_eq!(v.basis, vec!["geometric"]);
    }
}
