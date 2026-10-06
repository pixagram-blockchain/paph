//! The v4 local channel (SPEC-004 §10) — milestone M2.
//!
//! Two deliberate changes from v3's `local_channel`, and only two:
//!
//!   1. the matcher: distance-stratified greedy → deterministic
//!      maximum-cardinality minimum-cost assignment (§10.2), cost = the
//!      descriptor distance and NOTHING else — repetition stays in the burst
//!      weighting where v3 put it, charged once;
//!   2. the null: family {rot16, rot32, rot48, bitrev} aggregated by MAX
//!      (§9.2, §9.5) — THE verdict-breaking change of v4, shipped together
//!      with the matcher change so the two land as one measurable step.
//!
//! Everything else is v3's pipeline verbatim: fingerprint reading, burst
//! counts, the `w += SCALE / max(burst_a, burst_b)` weighting, the capacity
//! normaliser, the Lift evidence rule, `chance_correct`.  New alongside, not
//! instead: diversity (§10.4, indexes the modulation LUT), spatial recording
//! of matched anchors (§10.3, feeds §11), and the calibration LUT between
//! margin and evidence (§8.3) — identity under CAL-001-PROVISIONAL, so M2's
//! numbers differ from v3 only where the matcher and the null differ.
//!
//! `Proportion` is computed as a diagnostic (§8.1) and takes no decisions.

use crate::calibration::Profile;
use crate::compare::{read_local, Bag};
#[cfg(test)]
use crate::compare::{burst, hdf, rot_bag};
use crate::prepared::BagShare;
use crate::config::{chance_correct, clamp, SCALE};
use crate::coverage::{coverage, Coverage};
#[cfg(test)]
use crate::nulls::bitrev64;
use crate::wire::Tier1;

/// LN4 on a whole bag; rot16/32/48 reuse v3's `rot_bag`.
#[cfg(test)]
pub(crate) fn bitrev_bag(y: &Bag) -> Bag {
    let mut o = Bag { hi: vec![0; y.n], lo: vec![0; y.n], x: y.x.clone(), y: y.y.clone(), n: y.n };
    for i in 0..y.n {
        let (nh, nl) = bitrev64(y.hi[i], y.lo[i]);
        o.hi[i] = nh;
        o.lo[i] = nl;
    }
    o
}

/// §10.2: the assignment matcher on the edge set {(i,j) : hdf ≤ t}.
#[cfg(test)]
pub(crate) type Assigner = fn(usize, usize, &[i32]) -> Vec<(usize, usize)>;

/// Which assignment a local channel runs: comparator 4's full-matrix one
/// (frozen) or Appendix A on the edge-induced subgraph (41 and 42).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Matcher {
    Full,
    Sparse,
}

#[cfg(test)]
fn assign_bags(x: &Bag, y: &Bag, t: i32) -> Vec<(usize, usize)> {
    assign_bags_with(x, y, t, crate::assignment::assign)
}

#[cfg(test)]
fn assign_bags_with(x: &Bag, y: &Bag, t: i32, asg: Assigner) -> Vec<(usize, usize)> {
    let (n, m) = (x.n, y.n);
    let mut cost = vec![crate::assignment::NO_EDGE; n * m];
    for i in 0..n {
        for j in 0..m {
            let d = hdf(x, i, y, j);
            if d <= t {
                cost[i * m + j] = d;
            }
        }
    }
    asg(n, m, &cost)
}

#[derive(Clone, Debug)]
pub struct LocalV4 {
    pub measurable: bool,
    pub note: String,
    /// matched pairs (indices into each bag), ascending by A index
    pub pairs: Vec<(usize, usize)>,
    pub matches: i64,
    pub cmax: i64,
    /// burst-weighted match mass and the capacity normaliser (v3 semantics)
    pub w: i64,
    pub cap: i64,
    /// the winning null: its weighted mass, match count, and family member
    pub ctl_w: i64,
    pub ctl_n: i64,
    pub ctl_member: &'static str,
    /// Lift pipeline (decision path): raw, control, margin, calibrated evidence
    pub lift_raw: i64,
    pub lift_ctl: i64,
    pub margin: i64,
    /// LUT_local(margin) · LUT_div(diversity) / SCALE — the channel's output
    pub evidence: i64,
    /// diagnostics only (§8.1): Proportion under the same max-null
    pub prop_raw: i64,
    pub prop_ctl: i64,
    pub prop_margin: i64,
    /// §10.4: min-side mean inverse burst, currency
    pub diversity: i64,
    pub d_a: i64,
    pub d_b: i64,
    /// §10.3 → §11: grid stats of the matched anchors, per side
    pub coverage_a: Coverage,
    pub coverage_b: Coverage,
}

fn empty(note: &str, g: u8) -> LocalV4 {
    LocalV4 {
        measurable: false,
        note: note.into(),
        pairs: Vec::new(),
        matches: 0,
        cmax: 0,
        w: 0,
        cap: 1,
        ctl_w: 0,
        ctl_n: 0,
        ctl_member: "none",
        lift_raw: 0,
        lift_ctl: 0,
        margin: 0,
        evidence: 0,
        prop_raw: 0,
        prop_ctl: 0,
        prop_margin: 0,
        diversity: 0,
        d_a: 0,
        d_b: 0,
        coverage_a: coverage(&[], g),
        coverage_b: coverage(&[], g),
    }
}

/// The channel on parsed wires (the usual entry).
pub fn local_v4(a: &Tier1, b: &Tier1, p: &Profile) -> LocalV4 {
    local_v4_bags(&read_local(a), &read_local(b), p)
}

/// The 4.1 local channel: identical pipeline, Appendix A on the
/// edge-induced subgraph (SPEC-004.1 §A2).
pub fn local_v4_41(a: &Tier1, b: &Tier1, p: &Profile) -> LocalV4 {
    local_v4_bags_41(&read_local(a), &read_local(b), p)
}

/// The channel on explicit bags — the golden-vector and conformance surface:
/// a bag is just codes and anchors, so cross-engine vectors can state their
/// inputs exactly (SPEC-004 §20).  Crate-visible because `Bag` is.
/// Comparator 4 — the FROZEN full-matrix assignment (Appendix A as shipped).
pub(crate) fn local_v4_bags(x: &Bag, y: &Bag, p: &Profile) -> LocalV4 {
    local_core(x, y, p, Matcher::Full, &BagShare::new(x, y))
}

/// Comparator 41 — Appendix A on the edge-induced subgraph.
pub(crate) fn local_v4_bags_41(x: &Bag, y: &Bag, p: &Profile) -> LocalV4 {
    local_core(x, y, p, Matcher::Sparse, &BagShare::new(x, y))
}

/// Comparator 41/42's channel on a pair whose bag distances are already in
/// hand (the v3 reading of the same pair computed them).
pub fn local_v4_41_shared(x: &Bag, y: &Bag, p: &Profile, share: &BagShare) -> LocalV4 {
    local_core(x, y, p, Matcher::Sparse, share)
}

/// The assignment on the edge set {(i,j) : d(i,j) <= t} of a distance matrix.
fn assign_on(n: usize, m: usize, d: &[u8], t: i32, asg: Matcher) -> Vec<(usize, usize)> {
    match asg {
        Matcher::Full => {
            let cost: Vec<i32> = d
                .iter()
                .map(|&v| if (v as i32) <= t { v as i32 } else { crate::assignment::NO_EDGE })
                .collect();
            crate::assignment::assign(n, m, &cost)
        }
        Matcher::Sparse => {
            // the edges straight from the distances, row-major
            let mut list: Vec<(u32, u32, i32)> = Vec::new();
            for i in 0..n {
                for (j, &v) in d[i * m..(i + 1) * m].iter().enumerate() {
                    if (v as i32) <= t {
                        list.push((i as u32, j as u32, v as i32));
                    }
                }
            }
            crate::assignment::assign_edges(n, m, &list)
        }
    }
}

fn local_core(x: &Bag, y: &Bag, p: &Profile, asg: Matcher, share: &BagShare) -> LocalV4 {
    if x.n < 4 || y.n < 4 {
        return empty("too few distinctive regions on one side", p.grid_g);
    }
    let t = p.hamming_t;
    let (n, m) = (x.n, y.n);
    let cmax = x.n.min(y.n) as i64;
    let bx = BagShare::burst(&share.dxx, n, t);
    let by = BagShare::burst(&share.dyy, m, t);

    // §10.4 — diversity, from the same sums the capacity normaliser needs.
    let ca: i64 = bx.iter().map(|&v| SCALE / v).sum();
    let cb: i64 = by.iter().map(|&v| SCALE / v).sum();
    let d_a = ca / x.n as i64;
    let d_b = cb / y.n as i64;
    let diversity = d_a.min(d_b);
    let cap = ca.min(cb).max(1);

    // §10.2 — the measurement.
    let pairs = assign_on(n, m, &share.d[0], t, asg);
    let c = pairs.len() as i64;
    let mut w = 0i64;
    for h in pairs.iter() {
        w += SCALE / bx[h.0].max(by[h.1]);
    }

    // §9.2 / §9.5 — the null family, re-matched by the SAME matcher, control
    // aggregated by MAX.  Ties keep the earlier family member.  Every LN
    // member is distance-preserving within a side, so the burst profile of the
    // permuted bag IS `by` — the reference recomputed it and asserted that.
    let family: [(&'static str, usize); 4] = [("rot16", 1), ("rot32", 2), ("rot48", 3), ("bitrev", 4)];
    let (mut e, mut en, mut em) = (0i64, 0i64, "none");
    for &(name, k) in family.iter() {
        let byr = &by;
        let hr = assign_on(n, m, &share.d[k], t, asg);
        let mut wr = 0i64;
        for h in hr.iter() {
            wr += SCALE / bx[h.0].max(byr[h.1]);
        }
        if wr > e || em == "none" {
            e = wr;
            en = hr.len() as i64;
            em = name;
        }
    }

    // v3's Lift arithmetic, verbatim (compare.rs local_channel).
    let prior = SCALE;
    let conf_at = p.confidence_at as i64;
    let purity = w * SCALE / (w + e + prior);
    let conf = clamp(c * SCALE / conf_at, 0, SCALE);
    let lift_raw = clamp(purity * conf / SCALE, 0, SCALE);
    let lift_ctl = clamp(
        (e * SCALE / (e + e + prior)) * clamp(en * SCALE / conf_at, 0, SCALE) / SCALE,
        0,
        SCALE,
    );
    let prop_raw = clamp(w * SCALE / cap, 0, SCALE);
    let prop_ctl = clamp(e * SCALE / cap, 0, SCALE);

    let margin = chance_correct(lift_raw, lift_ctl);
    // §8.3 + §10.4 — calibration, then diversity modulation.  Identity LUTs
    // under CAL-001-PROVISIONAL make this a no-op by construction.
    let evidence = p.lut_local.eval(margin) * p.lut_diversity.eval(diversity) / SCALE;

    // §10.3 — spatial recording of the matched anchors, per side.
    let a_pts: Vec<(i64, i64)> = pairs.iter().map(|&(i, _)| (x.x[i], x.y[i])).collect();
    let b_pts: Vec<(i64, i64)> = pairs.iter().map(|&(_, j)| (y.x[j], y.y[j])).collect();

    LocalV4 {
        measurable: true,
        note: format!("{} of {} regions assigned, {} expected under {}", c, cmax, en, em),
        pairs,
        matches: c,
        cmax,
        w,
        cap,
        ctl_w: e,
        ctl_n: en,
        ctl_member: em,
        lift_raw,
        lift_ctl,
        margin,
        evidence,
        prop_raw,
        prop_ctl,
        prop_margin: chance_correct(prop_raw, prop_ctl),
        diversity,
        d_a,
        d_b,
        coverage_a: coverage(&a_pts, p.grid_g),
        coverage_b: coverage(&b_pts, p.grid_g),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::match_bags;

    /// A bag from explicit 64-bit codes; anchors on a diagonal so coverage has
    /// something to see.
    fn bag(codes: &[(u32, u32)]) -> Bag {
        let n = codes.len();
        Bag {
            hi: codes.iter().map(|c| c.0).collect(),
            lo: codes.iter().map(|c| c.1).collect(),
            x: (0..n).map(|i| (i as i64 * 65535) / n.max(1) as i64).collect(),
            y: (0..n).map(|i| (i as i64 * 65535) / n.max(1) as i64).collect(),
            n,
        }
    }

    /// Deterministic far-apart codes: xorshift stream, pairwise distance ~32.
    fn far(seed: u64, n: usize) -> Vec<(u32, u32)> {
        let mut s = seed.wrapping_mul(0x9e3779b97f4a7c15) | 1;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                ((s >> 32) as u32, s as u32)
            })
            .collect()
    }

    #[test]
    fn assignment_recovers_what_greedy_strands() {
        // b0 = base; a0 = base^{1 bit}; a1 = base^{8 bits}; b1 = a0^{2 bits}.
        // Edges at t=8:  (a0,b0)=1  (a0,b1)=2  (a1,b0)=8;  (a1,b1)=11 > t.
        // Greedy takes (a0,b0) and strands a1; the assignment matches both.
        let base = (0x0f0f0f0fu32, 0xf0f0f0f0u32);
        let a0 = (base.0 ^ 1, base.1);
        let a1 = (base.0 ^ 0x0000ff00, base.1); // 8 bits, disjoint from bit 0
        let b0 = base;
        let b1 = (a0.0 ^ 0x00030000, a0.1); // 2 more bits, disjoint again
        let x = bag(&[a0, a1, far(9, 1)[0], far(10, 1)[0]]);
        let y = bag(&[b0, b1, far(11, 1)[0], far(12, 1)[0]]);
        let t = 8;
        let greedy = match_bags(&x, &y, t);
        let optimal = assign_bags(&x, &y, t);
        assert_eq!(greedy.len(), 1, "greedy strands the second row");
        assert_eq!(optimal.len(), 2, "assignment recovers it");
        assert!(optimal.contains(&(0, 1)) && optimal.contains(&(1, 0)));
    }

    #[test]
    fn self_bag_has_positive_margin_and_full_diversity() {
        let codes = far(1, 24);
        let x = bag(&codes);
        let y = bag(&codes);
        let p = Profile::cal001();
        let r = local_v4_bags(&x, &y, &p);
        assert!(r.measurable);
        assert_eq!(r.matches, 24, "self-assignment is total");
        assert_eq!(r.diversity, SCALE, "unique codes at radius t");
        assert_eq!(r.d_a, r.d_b);
        assert!(r.ctl_w <= r.w);
        assert!(r.margin > 0, "margin {}", r.margin);
        // identity LUTs: evidence == margin exactly
        assert_eq!(r.evidence, r.margin);
        assert_eq!(r.lift_raw, clamp(r.lift_raw, 0, SCALE));
        // anchors on the diagonal: coverage sees a spread, both sides equal
        assert_eq!(r.coverage_a, r.coverage_b);
        assert!(r.coverage_a.occupied >= 3);
    }

    #[test]
    fn max_aggregation_dominates_every_family_member() {
        // Recompute each member's control by hand and check ctl_w is their max
        // — the §9.5 flip, asserted rather than assumed.
        let codes = far(7, 40);
        let x = bag(&codes);
        let mut y = bag(&codes);
        // perturb a few codes so the pair is realistic, not byte-identical
        for i in 0..8 {
            y.hi[i] ^= 3;
        }
        let p = Profile::cal001();
        let r = local_v4_bags(&x, &y, &p);
        let t = p.hamming_t;
        let bx = burst(&x, t);
        let mut members = Vec::new();
        for (name, yr) in [
            ("rot16", rot_bag(&y, 16)),
            ("rot32", rot_bag(&y, 32)),
            ("rot48", rot_bag(&y, 48)),
            ("bitrev", bitrev_bag(&y)),
        ] {
            let byr = burst(&yr, t);
            let hr = assign_bags(&x, &yr, t);
            let wr: i64 = hr.iter().map(|h| SCALE / bx[h.0].max(byr[h.1])).sum();
            members.push((name, wr));
        }
        let max_w = members.iter().map(|m| m.1).max().unwrap();
        assert_eq!(r.ctl_w, max_w);
        assert!(members.iter().any(|m| m.0 == r.ctl_member && m.1 == r.ctl_w));
        // and MAX is at least as conservative as v3's MIN, by construction
        let min_w = members.iter().map(|m| m.1).min().unwrap();
        assert!(r.ctl_w >= min_w);
    }

    #[test]
    fn repetition_collapses_diversity_but_not_the_weighting_contract() {
        // One tile wearing costumes: every code identical.
        let n = 16usize;
        let codes: Vec<(u32, u32)> = std::iter::repeat((0xdeadbeef, 0x01234567)).take(n).collect();
        let x = bag(&codes);
        let y = bag(&codes);
        let p = Profile::cal001();
        let r = local_v4_bags(&x, &y, &p);
        // burst_i = n for every code → D = SCALE / n
        assert_eq!(r.diversity, SCALE / n as i64);
        // every match weighs SCALE/n; the mass of the whole wall of tiles is
        // one tile's worth — v3's contract, preserved under the new matcher
        assert_eq!(r.w, (SCALE / n as i64) * r.matches);
        assert_eq!(r.matches, n as i64, "assignment still matches them all");
        // The LN null rightly reports ~nothing here (a rotated tile is not the
        // tile), so it is the WEIGHTING that discounts the wall: w saturates at
        // one tile's mass, purity = w/(w+0+SCALE) = 1/2 exactly, and the
        // extreme diversity is what arms the modulation LUT / rule R2.
        assert_eq!(r.margin, 5000);
        let unique = bag(&far(21, n));
        let ru = local_v4_bags(&unique, &unique, &p);
        assert!(r.margin < ru.margin, "wall {} vs unique {}", r.margin, ru.margin);
        assert!(r.diversity < p.rep_extreme_at as i64, "diversity {} arms R2", r.diversity);
    }

    #[test]
    fn abstains_below_four_and_diversity_modulation_applies() {
        let p = Profile::cal001();
        let r = local_v4_bags(&bag(&far(2, 3)), &bag(&far(3, 12)), &p);
        assert!(!r.measurable);
        assert_eq!(r.evidence, 0);

        // a profile that zeroes evidence at low diversity actually bites
        let mut p2 = Profile::cal001();
        p2.lut_diversity = crate::calibration::Lut(vec![(0, 0), (3000, 0), (10000, 10000)]);
        p2.validate().unwrap();
        let codes: Vec<(u32, u32)> = std::iter::repeat((0xabcd1234, 0x5678ef01)).take(12).collect();
        let r2 = local_v4_bags(&bag(&codes), &bag(&codes), &p2);
        assert_eq!(r2.diversity, SCALE / 12);
        assert_eq!(r2.evidence, 0, "modulation LUT gates the wall of tiles");
    }
}
