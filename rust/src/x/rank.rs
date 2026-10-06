//! XRank (PAPH-X §7, §14, §19.1) — one query against many candidates.
//!
//! Stage A scores the query's route against every candidate route with
//! the lane kernels over a column-major table (`RouteSoA`).  Stage B runs
//! the anchor-tier sparse screen on the candidates the route did not
//! certify, and Stage C the cascade on the survivors.  Every buffer — the
//! route table, the scan state, the pools, the geometry scratch — is the
//! thread's and is reused across the whole batch (§14.4).  The output is a
//! flat `i32` table, `XRANK_FIELDS` per candidate, in candidate order.

use super::compare::{xcompare_in, Execution, Scope, ScreenState, XCtx, XOptions, V_NOT_COPY};
use super::prepared::XPrepared;
use super::profile::{XBound, POLICY_FAST};
use super::route::{route_batch, route_class, RouteClass, RouteScore, RouteSoA};
use crate::abi::state_code;
use crate::config::Config;
use crate::prepared::canon_swapped;

pub const XRANK_FIELDS: usize = 24;

/// Field index of one rank record.
pub mod field {
    pub const STATE: usize = 0;
    pub const EXECUTION: usize = 1;
    pub const SCREEN: usize = 2;
    pub const ROUTE_LOCAL: usize = 3;
    pub const ROUTE_BAND: usize = 4;
    pub const ROUTE_GLOBAL: usize = 5;
    pub const ROUTE_CLASS: usize = 6;
    pub const POOL_DIRECT: usize = 7;
    pub const POOL_MIRROR: usize = 8;
    pub const ROWS: usize = 9;
    pub const INLIERS: usize = 10;
    pub const MODELS: usize = 11;
    pub const GEO_EVIDENCE: usize = 12;
    pub const GEO_MARGIN: usize = 13;
    pub const TOPOLOGY: usize = 14;
    pub const STRUCT_LO: usize = 15;
    pub const STRUCT_HI: usize = 16;
    pub const STRUCT_EXACT: usize = 17;
    pub const CERTIFIABLE: usize = 18;
    pub const LOCAL_EVIDENCE: usize = 19;
    pub const HAMMINGS: usize = 20;
    pub const FULL_PAIRS: usize = 21;
    pub const SWAPPED: usize = 22;
    pub const FLAGS: usize = 23;
}

pub const XF_CERTIFICATE: i32 = 1;
pub const XF_MIRROR_MODEL: i32 = 2;
pub const XF_EXPLOSION: i32 = 4;
pub const XF_FALLBACK_RAN: i32 = 8;
pub const XF_MIXED_SELECTION: i32 = 16;

/// A typed view of one record.
#[derive(Clone, Copy, Debug)]
pub struct XRankRecord<'a>(pub &'a [i32]);

#[derive(Clone, Copy, Debug)]
pub struct XRankOptions {
    pub policy: Option<u8>,
    /// skip the cascade for candidates the screen rejects (state −1), and
    /// for candidates whose pools stay thin after expansion unless the
    /// route certifies them (§14.3: only a sparse geometric or a strong
    /// structural signal enters the full comparison)
    pub gate: bool,
    /// `Scope::Copy` (the default): a candidate the lattice cannot lift
    /// above Related is reported `NOT_COPY` without resolving which
    pub scope: Scope,
}

impl Default for XRankOptions {
    fn default() -> Self {
        XRankOptions { policy: None, gate: true, scope: Scope::Copy }
    }
}

/// `state` of a candidate the lattice could not lift above Related
/// (Copy scope): not Unrelated, not Related — not resolved between them.
pub const STATE_NOT_COPY: i32 = 6;

fn state_of(v: &str) -> i32 {
    if v == V_NOT_COPY {
        STATE_NOT_COPY
    } else {
        state_code(v)
    }
}

/// Batch scratch: the route table and the scores, kept across calls.
pub struct RankScratch {
    pub soa: RouteSoA,
    pub scores: Vec<RouteScore>,
}

impl RankScratch {
    pub fn new() -> RankScratch {
        RankScratch { soa: RouteSoA::with_capacity(64), scores: Vec::with_capacity(64) }
    }
}

impl Default for RankScratch {
    fn default() -> Self {
        Self::new()
    }
}

fn clear(rec: &mut [i32]) {
    for v in rec.iter_mut() {
        *v = 0;
    }
}

fn route_code(c: RouteClass) -> i32 {
    match c {
        RouteClass::Reject => 0,
        RouteClass::Defer => 1,
        RouteClass::Fast => 2,
        RouteClass::Absent => 3,
    }
}

#[inline]
fn sat(v: u64) -> i32 {
    v.min(i32::MAX as u64) as i32
}

/// Rank: writes `cands.len()` records of `XRANK_FIELDS` into `out`.
#[allow(clippy::too_many_arguments)]
pub fn xrank(
    q: &XPrepared,
    cands: &[Option<&XPrepared>],
    cfg: &Config,
    xb: &XBound,
    opts: &XRankOptions,
    ctx: &mut XCtx,
    rs: &mut RankScratch,
    out: &mut [i32],
) -> usize {
    let n = cands.len();
    assert!(out.len() >= n * XRANK_FIELDS);
    let (base, xp) = (&xb.base, &xb.xp);
    let cfg = xb.bind(cfg);
    let xid = xb.xid;
    let profiles_ok = xb.refused.is_none() && q.xid == xid;
    let policy = opts.policy.unwrap_or(xp.fallback_policy);
    let copts = XOptions { policy: Some(policy), audit: false, scope: opts.scope };

    // Stage A — the route table and one SIMD pass
    rs.soa.begin(n);
    for (i, c) in cands.iter().enumerate() {
        if let Some(c) = c {
            rs.soa.set(i, &c.route);
        }
    }
    rs.scores.clear();
    rs.scores.resize(n.max(1), RouteScore::default());
    if n > 0 {
        route_batch(&q.route, &rs.soa, &mut rs.scores);
    }

    for (i, c) in cands.iter().enumerate() {
        let rec = &mut out[i * XRANK_FIELDS..(i + 1) * XRANK_FIELDS];
        clear(rec);
        let Some(c) = c else {
            rec[field::STATE] = state_code("Indeterminate");
            rec[field::SCREEN] = ScreenState::Refused.code();
            continue;
        };
        if !profiles_ok || c.xid != xid {
            rec[field::STATE] = state_code("Indeterminate");
            rec[field::SCREEN] = ScreenState::Refused.code();
            continue;
        }
        let score = rs.scores[i];
        let rc = route_class(&score, xp);
        rec[field::ROUTE_LOCAL] = score.local;
        rec[field::ROUTE_BAND] = score.band;
        rec[field::ROUTE_GLOBAL] = score.global;
        rec[field::ROUTE_CLASS] = route_code(rc);
        let swapped = canon_swapped(&q.p, &c.p);
        rec[field::SWAPPED] = swapped as i32;
        let c: &XPrepared = c;
        let (ca, cb) = if swapped { (c, q) } else { (q, c) };
        if ca.p.t1.bytes == cb.p.t1.bytes {
            rec[field::STATE] = state_code("Identical");
            rec[field::SCREEN] = ScreenState::Identical.code();
            rec[field::EXECUTION] = Execution::Fast.code();
            continue;
        }
        // Stage B — the sparse screen (the cascade's own first steps): a
        // route hard-negative with an empty anchor pool stops here; so does
        // a candidate whose pools stay thin through the expansion tiers
        // unless the route certifies it — the gate 4.2's rank applies, with
        // the route as the structural door it never had
        let mut prescanned = false;
        if opts.gate && rc != RouteClass::Fast {
            ctx.m.begin(ca, cb);
            prescanned = true;
            ctx.m.scan_rows(ca, cb, xp, xp.anchors[0] as usize);
            let limit = cfg.geo_min_corr.max(1);
            let mut d = ctx.m.count(false, ca, cb, base, limit).count;
            let mut m = if cfg.mirror_hypothesis { ctx.m.count(true, ca, cb, base, limit).count } else { 0 };
            if rc == RouteClass::Reject && d.max(m) as i32 <= xp.defer_pool_max {
                rec[field::POOL_DIRECT] = d as i32;
                rec[field::POOL_MIRROR] = m as i32;
                rec[field::ROWS] = ctx.m.stats.rows as i32;
                rec[field::HAMMINGS] = sat(ctx.m.stats.hammings);
                rec[field::FULL_PAIRS] = sat(2 * ctx.m.stats.full_pairs);
                rec[field::STATE] = -1;
                rec[field::SCREEN] = ScreenState::Reject.code();
                continue;
            }
            let na = ca.p.kp.len().min(cb.p.kp.len());
            let mut tier = 1usize;
            while d.max(m) < cfg.geo_min_corr && ctx.m.stats.rows < na && tier < xp.anchors.len() {
                let before = d.max(m);
                let rows_before = ctx.m.stats.rows;
                ctx.m.scan_rows(ca, cb, xp, xp.anchors[tier] as usize);
                d = ctx.m.count(false, ca, cb, base, limit).count;
                m = if cfg.mirror_hypothesis { ctx.m.count(true, ca, cb, base, limit).count } else { 0 };
                tier += 1;
                let added = ctx.m.stats.rows - rows_before;
                if d.max(m) < before + (added / 32).max(1) {
                    break;
                }
            }
            if d.max(m) < cfg.geo_min_corr {
                rec[field::POOL_DIRECT] = d as i32;
                rec[field::POOL_MIRROR] = m as i32;
                rec[field::ROWS] = ctx.m.stats.rows as i32;
                rec[field::HAMMINGS] = sat(ctx.m.stats.hammings);
                rec[field::FULL_PAIRS] = sat(2 * ctx.m.stats.full_pairs);
                rec[field::STATE] = -1;
                rec[field::SCREEN] = ScreenState::Defer.code();
                continue;
            }
        }
        // Stage C — the cascade
        let r = xcompare_in(ca, cb, swapped, &cfg, xb, &copts, Some(score), prescanned, ctx);
        rec[field::STATE] = if r.verdict == "Indeterminate" && r.execution == Execution::Deferred && policy == POLICY_FAST {
            state_code("Indeterminate")
        } else {
            state_of(r.verdict)
        };
        rec[field::EXECUTION] = r.execution.code();
        rec[field::SCREEN] = match r.reason {
            "route" => ScreenState::Reject.code(),
            _ => ScreenState::Pass.code(),
        };
        rec[field::ROWS] = r.stats.rows as i32;
        rec[field::HAMMINGS] = sat(r.stats.hammings);
        rec[field::FULL_PAIRS] = sat(2 * r.stats.full_pairs);
        let mut flags = 0i32;
        if r.stats.explosion {
            flags |= XF_EXPLOSION;
        }
        if r.fallback.is_some() {
            flags |= XF_FALLBACK_RAN;
        }
        if r.mixed_selection {
            flags |= XF_MIXED_SELECTION;
        }
        if let Some(g) = &r.geometry {
            rec[field::POOL_DIRECT] = g.pool_direct as i32;
            rec[field::POOL_MIRROR] = g.pool_mirror as i32;
            rec[field::INLIERS] = g.total_inliers as i32;
            rec[field::MODELS] = g.models.len() as i32;
            rec[field::GEO_EVIDENCE] = g.evidence as i32;
            rec[field::GEO_MARGIN] = g.margin as i32;
            rec[field::TOPOLOGY] = g.topology as i32;
            if g.certificate {
                flags |= XF_CERTIFICATE;
            }
            if g.models.iter().any(|m| m.mirror) {
                flags |= XF_MIRROR_MODEL;
            }
        }
        if let Some(s) = &r.structural {
            rec[field::STRUCT_LO] = s.lo as i32;
            rec[field::STRUCT_HI] = s.hi as i32;
            rec[field::STRUCT_EXACT] = s.exact as i32;
            rec[field::CERTIFIABLE] = s.certifiable as i32;
            rec[field::LOCAL_EVIDENCE] = s.local_evidence.map(|v| v as i32).unwrap_or(-1);
        } else {
            rec[field::LOCAL_EVIDENCE] = -1;
        }
        if let Some(f) = &r.fallback {
            // the exact comparator's fields, where it ran
            rec[field::INLIERS] = f.base.total_inliers as i32;
            rec[field::MODELS] = f.models42.len() as i32;
            rec[field::GEO_EVIDENCE] = f.base.geometry_evidence as i32;
            rec[field::GEO_MARGIN] = f.base.geo_margin as i32;
            rec[field::TOPOLOGY] = f.base.topology as i32;
            rec[field::CERTIFIABLE] = f.base.certifiable as i32;
            if let Some(l) = &f.base.local {
                rec[field::LOCAL_EVIDENCE] = if l.measurable { l.evidence as i32 } else { -1 };
            }
        }
        rec[field::FLAGS] = flags;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::Prepared;
    use crate::wire::hash;
    use crate::x::testimg::{image, mirror};

    fn side(px: &[u8], w: usize, h: usize, xb: &XBound) -> XPrepared {
        let rot = crate::keypoints::RotCache::new(&crate::keypoints::pattern());
        let f = hash(px, w, h, &Config::default(), &rot);
        XPrepared::new(Prepared::new(&f.t1, Some(&f.t2)).unwrap(), xb)
    }

    #[test]
    fn rank_records_match_pairwise_compares() {
        let cfg = Config::default();
        let xb = XBound::shipped();
        let (w, h) = (128, 96);
        let q = side(&image(70, w, h), w, h, &xb);
        let mut cands: Vec<XPrepared> = (0..20u64).map(|i| side(&image(1000 + i, w, h), w, h, &xb)).collect();
        cands.push(side(&mirror(&image(70, w, h), w, h), w, h, &xb));
        cands.push(side(&image(70, w, h), w, h, &xb));
        let refs: Vec<Option<&XPrepared>> = cands.iter().map(Some).collect();
        let mut ctx = XCtx::new();
        let mut rs = RankScratch::new();
        let mut out = vec![0i32; refs.len() * XRANK_FIELDS];
        let n = xrank(&q, &refs, &cfg, &xb, &XRankOptions::default(), &mut ctx, &mut rs, &mut out);
        assert_eq!(n, refs.len());
        let mut rejected = 0;
        for (i, c) in cands.iter().enumerate() {
            let rec = &out[i * XRANK_FIELDS..(i + 1) * XRANK_FIELDS];
            if rec[field::STATE] == -1 {
                rejected += 1;
                continue;
            }
            let r = super::super::compare::xcompare(&q, c, &cfg, &xb, &XOptions { scope: Scope::Copy, ..XOptions::default() }, &mut ctx);
            assert_eq!(rec[field::STATE], state_of(r.verdict), "candidate {i}");
            assert_eq!(rec[field::EXECUTION], r.execution.code(), "candidate {i}");
        }
        let last = &out[(refs.len() - 1) * XRANK_FIELDS..];
        assert_eq!(last[field::STATE], state_code("Identical"));
        let mirror_rec = &out[(refs.len() - 2) * XRANK_FIELDS..(refs.len() - 1) * XRANK_FIELDS];
        assert_eq!(mirror_rec[field::STATE], state_code("Copy"), "{:?}", mirror_rec);
        println!("rejected {} of {} unrelated candidates by the screen", rejected, 20);
    }
}
