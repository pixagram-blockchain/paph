//! The comparator report, field for field the object the JavaScript engine
//! returns — the same keys, in the same order, holding the same values — so
//! `JSON.stringify` of either engine's report is the same text.
//!
//! The older surfaces (`compare::to_json`, `v4::to_json_v4`, `v42::to_json_v42`)
//! are frozen parity-harness formats and stay as they shipped.  This module is
//! what the WebAssembly build returns to JavaScript callers, so a caller can
//! switch engines without noticing.

use crate::compare::{Channel, Coh, Geo, Verdict, DIHEDRAL, D4_INVERSE};
use crate::config::{Thresholds, SCALE};
use crate::coverage::Coverage;
use crate::json::{arr, J};
use crate::local_v4::LocalV4;
use crate::v42::{Screen42, V42Report};

fn chan_base(c: &Channel) -> Vec<(&'static str, J)> {
    vec![
        ("value", c.value.into()),
        ("raw", c.raw.into()),
        ("control", c.control.into()),
        ("measurable", c.measurable.into()),
        ("controlRan", c.control_ran.into()),
    ]
}

fn finish(mut f: Vec<(&'static str, J)>, c: &Channel) -> J {
    f.push(("note", J::Str(c.note.clone())));
    J::Obj(f)
}

fn coherence(c: &Coh) -> J {
    match c.full {
        None => obj!["value" => c.value, "measurable" => c.measurable, "inliers" => c.inliers],
        Some((fraction, scale, dx, dy)) => obj![
            "value" => c.value,
            "measurable" => c.measurable,
            "fraction" => fraction,
            "inliers" => c.inliers,
            "scale" => scale,
            "dx" => dx,
            "dy" => dy,
        ],
    }
}

fn ijd(pairs: &[(usize, usize, i32)]) -> J {
    J::Arr(pairs.iter().map(|&(i, j, d)| obj!["i" => i, "j" => j, "d" => d]).collect())
}

/// The recovered scale as the reference states it: `scaleQ16 / 65536`, a
/// float, inverted when the pair was read in swapped order and a model exists.
fn geo_scale(g: &Geo, swapped: bool) -> J {
    if g.scale_q16 == 0 {
        return J::Null;
    }
    let s = g.scale_q16 as f64 / 65536.0;
    if swapped && g.model.is_some() {
        J::Num(1.0 / s)
    } else {
        J::Num(s)
    }
}

fn geo(g: &Geo, swapped: bool) -> J {
    if !g.ch.measurable {
        return finish(chan_base(&g.ch), &g.ch);
    }
    let mut f = chan_base(&g.ch);
    f.push(("inliers", g.inliers.into()));
    f.push(("chanceInliers", g.chance.into()));
    f.push(("accepted", g.accepted.into()));
    f.push(("mirrored", g.hypothesis.contains("mirrored").into()));
    f.push(("inverted", g.hypothesis.contains("inverted").into()));
    f.push(("hypothesis", g.hypothesis.into()));
    f.push((
        "model",
        match g.model {
            Some(m) => obj!["r00" => m.r00, "r10" => m.r10, "tx" => m.tx, "ty" => m.ty],
            None => J::Null,
        },
    ));
    f.push(("mask", arr(g.mask.iter().copied())));
    f.push(("pairs", ijd(&g.pairs)));
    f.push(("scale", geo_scale(g, swapped)));
    f.push(("direct", g.direct.into()));
    f.push(("mirrorTried", g.mirror_tried.into()));
    finish(f, &g.ch)
}

/// The v3 reading, as `wire.compare` returns it.  `None` for a lean reading,
/// which never computed the parts this would report.
pub fn v3(v: &Verdict) -> Option<J> {
    let d = v.detail.as_ref()?;
    let mut chans: Vec<(&'static str, J)> = Vec::with_capacity(7);
    let mut dct_transform: Option<&'static str> = None;
    for (name, c) in v.channels.iter() {
        let j = if !c.measurable {
            finish(chan_base(c), c)
        } else {
            let mut f = chan_base(c);
            match *name {
                "dct" => {
                    let x = d.dct.unwrap_or_default();
                    let t = if v.swapped { D4_INVERSE[x.t] } else { x.t };
                    dct_transform = Some(DIHEDRAL[t]);
                    f.push(("l0", x.l0.into()));
                    f.push(("l1", x.l1.into()));
                    f.push(("l2", x.l2.into()));
                    f.push(("t", x.t.into()));
                    f.push(("inverted", x.inverted.into()));
                    f.push(("transform", DIHEDRAL[t].into()));
                }
                "local" => {
                    let x = d.local.clone().unwrap_or_default();
                    f.push(("matched", x.matched.into()));
                    f.push(("chance", x.chance.into()));
                    f.push(("of", x.of.into()));
                    f.push(("pairs", ijd(&x.pairs)));
                    f.push(("coherence", coherence(&x.coherence)));
                    f.push(("weightedMatch", x.weighted_match.into()));
                    f.push(("both", obj!["lift" => v.lift, "proportion" => v.proportion]));
                }
                "shape" => {
                    f.push((
                        "pairs",
                        J::Arr(d.shape_pairs.iter().map(|&(i, j, s)| obj!["i" => i, "j" => j, "s" => s]).collect()),
                    ));
                }
                "topology" => {
                    f.push(("quantile", d.topology.0.into()));
                    f.push(("rank", d.topology.1.into()));
                    f.push(("sameExport", v.same_export.unwrap_or(false).into()));
                }
                "palette" => {
                    f.push(("inverted", d.palette_inverted.into()));
                }
                "silhouette" => {
                    f.push(("radial", d.silhouette.0.into()));
                    f.push(("occupancy", d.silhouette.1.into()));
                }
                _ => {}
            }
            finish(f, c)
        };
        chans.push((name, j));
    }
    let g = &v.geo;
    let gm = g.ch.measurable;
    let local_measurable = v.channels.iter().any(|(n, c)| *n == "local" && c.measurable);
    let pal = d.palette?;
    let br = d.brightness?;
    let palette = match pal.share {
        None => obj!["relation" => pal.relation, "exact" => pal.exact, "of" => pal.of],
        Some(share) => obj!["relation" => pal.relation, "exact" => pal.exact, "of" => pal.of, "share" => share],
    };
    let report = obj![
        "dihedral" => dct_transform,
        "mirrored" => if gm { Some(g.hypothesis.contains("mirrored")) } else { None },
        "geoHypothesis" => if gm { Some(g.hypothesis) } else { None },
        "recoveredScale" => if gm { geo_scale(g, v.swapped) } else { J::Null },
        "inliers" => if gm { g.inliers } else { 0 },
        "chanceInliers" => if gm { Some(g.chance) } else { None },
        "palette" => palette,
        "brightness" => obj![
            "delta" => br.delta,
            "meanDelta" => br.mean_delta,
            "shapeDelta" => br.shape_delta,
            "relation" => br.relation,
        ],
        "sameExport" => v.same_export,
        "evidenceBoth" => if local_measurable {
            obj!["lift" => v.lift, "proportion" => v.proportion]
        } else {
            J::Null
        },
    ];
    Some(obj![
        "verdict" => v.verdict,
        "class" => v.class.clone(),
        "basis" => arr(v.basis.iter().copied()),
        "structural" => v.structural,
        "geometric" => v.geometric,
        "weighted" => v.weighted,
        "gate" => v.gate,
        "channels" => J::Obj(chans),
        "geo" => geo(g, v.swapped),
        "measured" => arr(d.measured.iter().copied()),
        "abstained" => arr(v.abstained.iter().copied()),
        "report" => report,
        "identical" => v.identical,
        "scoring" => d.scoring,
        "thresholds" => obj![
            "STRUCT_IDENTICAL" => Thresholds::STRUCT_IDENTICAL,
            "STRUCT_STRONG" => Thresholds::STRUCT_STRONG,
            "STRUCT_MODERATE" => Thresholds::STRUCT_MODERATE,
            "STRUCT_WEAK" => Thresholds::STRUCT_WEAK,
            "GEO_STRONG" => Thresholds::GEO_STRONG,
            "GEO_WEAK" => Thresholds::GEO_WEAK,
            "GEO_SOLO_INLIERS" => Thresholds::GEO_SOLO_INLIERS,
            "STRUCT_SOLO" => Thresholds::STRUCT_SOLO,
        ],
        "structuralCertifiable" => v.structural_certifiable,
        "swapped" => v.swapped,
    ])
}

pub fn coverage(c: &Coverage) -> J {
    obj![
        "g" => c.g,
        "occupied" => c.occupied,
        "coverage" => c.coverage,
        "bboxCells" => c.bbox_cells,
        "concentration" => c.concentration,
        "counts" => arr(c.counts.iter().copied()),
    ]
}

pub fn local(l: &LocalV4) -> J {
    obj![
        "measurable" => l.measurable,
        "note" => l.note.clone(),
        "pairs" => J::Arr(l.pairs.iter().map(|&(i, j)| arr([i, j])).collect()),
        "matches" => l.matches,
        "cmax" => l.cmax,
        "w" => l.w,
        "cap" => l.cap,
        "ctlW" => l.ctl_w,
        "ctlN" => l.ctl_n,
        "ctlMember" => l.ctl_member,
        "liftRaw" => l.lift_raw,
        "liftCtl" => l.lift_ctl,
        "margin" => l.margin,
        "evidence" => l.evidence,
        "propRaw" => l.prop_raw,
        "propCtl" => l.prop_ctl,
        "propMargin" => l.prop_margin,
        "diversity" => l.diversity,
        "dA" => l.d_a,
        "dB" => l.d_b,
        "coverageA" => coverage(&l.coverage_a),
        "coverageB" => coverage(&l.coverage_b),
    ]
}

/// The comparator-42 report as JavaScript's `compare()` returns it.  A lean
/// reading reports `v3: null`.
pub fn v42(r: &V42Report) -> J {
    let b = &r.base;
    if b.verdict == "Indeterminate" {
        // the refusal object: `indeterminate()`, then the 4.2 fields in the
        // order the reference assigns them
        return obj![
            "comparator" => b.comparator,
            "verdict" => "Indeterminate",
            "class" => "",
            "basis" => J::Arr(vec![]),
            "reasons" => arr(b.reasons.iter().copied()),
            "structural" => 0i64,
            "certifiable" => false,
            "v3" => J::Null,
            "local" => J::Null,
            "models" => J::Arr(vec![]),
            "topology" => 0i64,
            "totalInliers" => 0i64,
            "coverage" => J::Null,
            "geometryEvidence" => 0i64,
            "geoMeasurable" => false,
            "geoRaw" => 0i64,
            "geoCtl" => 0i64,
            "geoMargin" => 0i64,
            "geoCtlMember" => "none",
            "swapped" => false,
            "calibration" => b.calibration.clone(),
            "calibrationId" => b.calibration_id.clone(),
            "geoWeakInliers" => 0i64,
            "screen" => J::Null,
            "diversity" => obj![
                "spatial" => 0i64, "scale" => 0i64, "model" => 0i64,
                "descriptor" => 0i64, "combined" => 0i64, "multiplier" => SCALE,
            ],
            "medianErr" => J::Arr(vec![]),
            "selection" => obj!["a" => 0i64, "b" => 0i64, "mixed" => false],
            "kpA" => 0i64,
            "kpB" => 0i64,
        ];
    }
    let div = &r.diversity_geo;
    let models = J::Arr(
        r.models42
            .iter()
            .map(|m| {
                obj![
                    "r00" => m.r00,
                    "r10" => m.r10,
                    "tx" => m.tx,
                    "ty" => m.ty,
                    "scaleQ16" => m.scale_q16,
                    "mirror" => m.mirror,
                    "inliers" => m.inliers,
                    "medianErr" => m.median_err,
                    "confSum" => m.conf_sum,
                ]
            })
            .collect(),
    );
    obj![
        "comparator" => b.comparator,
        "verdict" => b.verdict,
        "class" => b.class.clone(),
        "basis" => arr(b.basis.iter().copied()),
        "reasons" => arr(b.reasons.iter().copied()),
        "structural" => b.structural,
        "certifiable" => b.certifiable,
        "v3" => b.v3.as_ref().and_then(v3),
        "local" => b.local.as_ref().map(local),
        "models" => models,
        "topology" => b.topology,
        "totalInliers" => b.total_inliers,
        "coverage" => b.coverage.as_ref().map(coverage),
        "geometryEvidence" => b.geometry_evidence,
        "geoMeasurable" => b.geo_measurable,
        "geoRaw" => b.geo_raw,
        "geoCtl" => b.geo_ctl,
        "geoMargin" => b.geo_margin,
        "geoCtlMember" => b.geo_ctl_member,
        "swapped" => b.swapped,
        "calibration" => b.calibration.clone(),
        "calibrationId" => b.calibration_id.clone(),
        "geoWeakInliers" => b.geo_weak_inliers,
        "diversity" => obj![
            "spatial" => div.spatial,
            "scale" => div.scale,
            "model" => div.model,
            "descriptor" => div.descriptor,
            "combined" => div.combined,
            "multiplier" => div.multiplier,
        ],
        "medianErr" => arr(r.models42.iter().map(|m| m.median_err)),
        "selection" => obj!["a" => r.select_a, "b" => r.select_b, "mixed" => r.mixed_selection()],
        "kpA" => r.kp_a,
        "kpB" => r.kp_b,
        "screen" => J::Null,
    ]
}

pub fn screen(s: &Screen42) -> J {
    obj!["pass" => s.pass, "poolDirect" => s.pool_direct, "poolMirror" => s.pool_mirror]
}
