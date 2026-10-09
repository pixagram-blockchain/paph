//! The PAPH-X cascade (§4, §5, §12, §13, §21, §28, §38).
//!
//! ```text
//!   X0  exact identity and validity
//!   X1  the route
//!   X2  sparse descriptor retrieval, anchors first
//!   X3  adaptive geometry: 96 → 160 → 256 → 512 only as the certificate needs
//!   X4  structural channels, cheapest first, each one only while the lattice
//!       can still move; the geometry control likewise
//!   X5  DEFER when the fast evidence is not enough to be sure, and then
//!       EXACT42 under the safe policy
//! ```
//!
//! The verdict vocabulary is comparator 42's and so is the lattice: the
//! scheduler never invents a rule, it evaluates `lattice_v4` on the corners
//! of what is still unknown and stops when every corner agrees (§10.2,
//! §28).  What PAPH-X adds is an EXECUTION state beside the verdict (§21):
//! FAST, DEFERRED, FALLBACK or AUDIT — never part of the verdict itself.

use super::geom::{control, diversity, measure, topology, Frames, GeomScratch, MultiX};
use super::matcher::{MatchScratch, ScanStats, ScreenCount};
use super::prepared::XPrepared;
use super::profile::{XBound, XProfile, POLICY_EXACT, POLICY_FAST, POLICY_SAFE, X_COMPARATOR};
use super::route::{route_class, route_score, RouteClass, RouteScore};
use super::structural::{Structural, CH_DCT, CH_LOCAL, CH_NAMES, CH_PALETTE, CH_RUNS, CH_SHAPE, CH_SILHOUETTE, CH_TOPOLOGY};
use crate::calibration::Profile;
use crate::compare::{mirror_side, Reading};
use crate::config::{chance_correct, idiv, Config, SCALE};
use crate::coverage::Coverage;
use crate::geom42::{Diversity, ModelRec42};
use crate::json::{arr, J};
use crate::keypoints::Keypoint;
use crate::lattice::{lattice_v4, LatticeIn, V4Verdict};
use crate::prepared::{canon_swapped, PairCtx};
use crate::v4::R_CORRUPT;
use crate::v42::{compare_in, V42Report};

pub const R_FALLBACK_REQUIRED: &str = "FALLBACK_REQUIRED";
pub const R_XPROFILE_MISMATCH: &str = "XPROFILE_MISMATCH";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Execution {
    Fast,
    Deferred,
    Fallback,
    Audit,
}

impl Execution {
    pub fn name(self) -> &'static str {
        match self {
            Execution::Fast => "FAST",
            Execution::Deferred => "DEFERRED",
            Execution::Fallback => "FALLBACK",
            Execution::Audit => "AUDIT",
        }
    }
    pub fn code(self) -> i32 {
        match self {
            Execution::Fast => 0,
            Execution::Deferred => 1,
            Execution::Fallback => 2,
            Execution::Audit => 3,
        }
    }
}

/// Everything one thread reuses across pairs (§15.3, §14.4).
pub struct XCtx {
    pub m: MatchScratch,
    pub g: GeomScratch,
    pub d0: Vec<u8>,
    pub sc42: Box<crate::geom42::Scratch>,
    /// the mirrored A keypoints of the current pair
    am: Vec<Keypoint>,
    /// per geometry measurement of the last comparison: [rows scanned,
    /// direct pool, mirror pool, inliers, models, certificate, the
    /// measurement with the weak signal] — read by the harness (`sibench
    /// xtrace`), never by a verdict
    pub trace: Vec<[i64; 7]>,
}

impl XCtx {
    pub fn new() -> XCtx {
        XCtx {
            m: MatchScratch::new(),
            g: GeomScratch::new(),
            d0: vec![0u8; 128 * 128],
            sc42: Box::new(crate::geom42::Scratch::new()),
            am: Vec::with_capacity(512),
            trace: Vec::with_capacity(8),
        }
    }
}

impl Default for XCtx {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static XCTX: std::cell::Cell<Option<Box<XCtx>>> = const { std::cell::Cell::new(None) };
}

/// Run `f` with the thread's context.
pub fn with_ctx<R>(f: impl FnOnce(&mut XCtx) -> R) -> R {
    let mut c = XCTX.with(|c| c.take()).unwrap_or_else(|| Box::new(XCtx::new()));
    let r = f(&mut c);
    XCTX.with(|s| s.set(Some(c)));
    r
}

// ------------------------------------------------------------------ screen

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenState {
    Identical,
    Pass,
    Defer,
    Reject,
    Refused,
}

impl ScreenState {
    pub fn code(self) -> i32 {
        match self {
            ScreenState::Reject => 0,
            ScreenState::Defer => 1,
            ScreenState::Pass => 2,
            ScreenState::Identical => 3,
            ScreenState::Refused => -1,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct XScreen {
    pub state: ScreenState,
    pub reason: &'static str,
    pub route: RouteScore,
    pub route_class: RouteClass,
    pub direct: ScreenCount,
    pub mirror: ScreenCount,
    pub stats: ScanStats,
    pub swapped: bool,
}

fn refused_screen(reason: &'static str) -> XScreen {
    XScreen {
        state: ScreenState::Refused,
        reason,
        route: RouteScore::default(),
        route_class: RouteClass::Absent,
        direct: ScreenCount::default(),
        mirror: ScreenCount::default(),
        stats: ScanStats::default(),
        swapped: false,
    }
}

fn check_profiles(a: &XPrepared, b: &XPrepared, xb: &XBound) -> Option<&'static str> {
    if let Some(r) = xb.refused {
        return Some(r);
    }
    if a.xid != xb.xid || b.xid != xb.xid {
        return Some(R_XPROFILE_MISMATCH);
    }
    if a.p.t1.version != b.p.t1.version {
        return Some(crate::wire::R_WIRE_MISMATCH);
    }
    None
}

/// The structural door (profile `gate_door`, X2): could the structure of
/// this pair still certify a Copy on its own — the lattice's recolour arm:
/// the local channel measurable, at least `min_secondaries` other channels,
/// and the weighted structural score at or above both the strong and the
/// solo bar — with nothing from the geometry?  A screen exit sees a pair
/// whose anchor-tier pools are thin, which X1 already read as the geometric
/// arms being out of reach; the door answers, exactly, for the one arm that
/// needs no geometry.  The door shuts as soon as
/// the score's upper bound (every unknown channel at its maximum, the local
/// channel at its bound) falls below the bar, and stays open only when the
/// six secondaries and the local channel's edge-set bound all leave the bar
/// in reach.  The steps go in a near-cheapest order on the PAPH-SI corpus'
/// unrelated pairs (`sibench doorprof`: 20.6 µs per certifiable pair
/// natively, within 0.1 µs of the cheapest of all 5,040 orders, against
/// 28.4 µs in the cascade's own order): the two nearly free
/// channels, the local channel's bound — the heaviest weight, for the price
/// of 16k popcounts — then the rest.  `true`: open — the pair must be compared.
pub fn structural_door(ca: &XPrepared, cb: &XPrepared, base: &Profile, d0: &mut [u8]) -> bool {
    door_step(ca, cb, base, d0).0
}

/// The local channel's edge-set bound, as a step of `DOOR_ORDER`.
pub const DOOR_LOCAL_BOUND: usize = usize::MAX;
/// The door's steps, cheapest-to-shut first.
pub const DOOR_ORDER: [usize; 7] = [CH_RUNS, CH_SILHOUETTE, DOOR_LOCAL_BOUND, CH_TOPOLOGY, CH_SHAPE, CH_DCT, CH_PALETTE];

/// The door and the step that settled it: 0 not certifiable, 1 the bar out
/// of reach before any channel, 2 + k after the k-th step of `DOOR_ORDER`,
/// 9 open.  (`sibench doorprof` reads the steps.)
pub fn door_step(ca: &XPrepared, cb: &XPrepared, base: &Profile, d0: &mut [u8]) -> (bool, usize) {
    let mut s = Structural::new(ca, cb, base);
    if !s.measurable[CH_LOCAL] || s.secondaries() < base.min_secondaries.max(0) as usize {
        return (false, 0);
    }
    let bar = (base.thresholds[1] as i64).max(base.thresholds[4] as i64);
    if s.bounds(base).1 < bar {
        return (false, 1);
    }
    for (i, k) in DOOR_ORDER.into_iter().enumerate() {
        if k == DOOR_LOCAL_BOUND {
            s.bound_local(ca, cb, base, d0);
        } else {
            s.compute(k, ca, cb, base, d0);
        }
        if s.bounds(base).1 < bar {
            return (false, 2 + i);
        }
    }
    (true, 9)
}

/// §12 — the pair screen: route, then the anchor tier's sparse pools.
/// Never a verdict: `Reject` means the expensive comparator is not worth
/// spending here under the profile; `Defer` means it was not eliminated.
pub fn xscreen(a: &XPrepared, b: &XPrepared, cfg: &Config, xb: &XBound, ctx: &mut XCtx) -> XScreen {
    if let Some(r) = check_profiles(a, b, xb) {
        return refused_screen(r);
    }
    let (base, xp) = (&xb.base, &xb.xp);
    let swapped = canon_swapped(&a.p, &b.p);
    let (ca, cb) = if swapped { (b, a) } else { (a, b) };
    if ca.p.t1.bytes == cb.p.t1.bytes {
        let mut s = refused_screen("identical");
        s.state = ScreenState::Identical;
        s.swapped = swapped;
        return s;
    }
    let cfg = xb.bind(cfg);
    let route = route_score(&ca.route, &cb.route);
    let rc = route_class(&route, xp);
    ctx.m.begin(ca, cb);
    ctx.m.scan_rows(ca, cb, xp, xp.anchors[0] as usize);
    let limit = cfg.geo_min_corr.max(1);
    let d = ctx.m.count(false, ca, cb, base, limit);
    let m = if cfg.mirror_hypothesis { ctx.m.count(true, ca, cb, base, limit) } else { ScreenCount::default() };
    let best = d.count.max(m.count);
    let (state, reason) = if best >= cfg.geo_min_corr {
        (ScreenState::Pass, "sparse")
    } else if rc == RouteClass::Fast {
        (ScreenState::Pass, "route")
    } else if rc == RouteClass::Reject && best as i32 <= xp.defer_pool_max {
        if xp.gate_door != 0 && structural_door(ca, cb, base, &mut ctx.d0) {
            (ScreenState::Defer, "structure")
        } else {
            (ScreenState::Reject, "route+sparse")
        }
    } else {
        (ScreenState::Defer, "thin")
    };
    XScreen { state, reason, route, route_class: rc, direct: d, mirror: m, stats: ctx.m.stats, swapped }
}

// ----------------------------------------------------------------- report

#[derive(Clone, Debug)]
pub struct XGeometry {
    pub anchors: usize,
    pub expanded_to: usize,
    pub pool_direct: usize,
    pub pool_mirror: usize,
    pub models: Vec<ModelRec42>,
    pub total_inliers: i64,
    pub weak_inliers: i64,
    pub measurable: bool,
    pub raw: i64,
    pub ctl: i64,
    pub ctl_member: &'static str,
    pub ctl_ran: bool,
    pub margin: i64,
    pub evidence: i64,
    pub diversity: Diversity,
    pub coverage: Option<Coverage>,
    pub topology: u8,
    pub certificate: bool,
    pub unique_a: usize,
    pub unique_b: usize,
}

#[derive(Clone, Debug)]
pub struct XStructural {
    pub lo: i64,
    pub hi: i64,
    pub exact: bool,
    pub channels: [(&'static str, i64, bool, bool); 7],
    pub local_evidence: Option<i64>,
    pub local_matches: Option<i64>,
    pub diversity: i64,
    pub coverage_min: Option<i64>,
    pub certifiable: bool,
}

#[derive(Clone, Debug)]
pub struct XReport {
    pub comparator: u16,
    pub verdict: &'static str,
    pub class: String,
    pub basis: Vec<&'static str>,
    pub reasons: Vec<&'static str>,
    pub execution: Execution,
    pub reason: &'static str,
    pub route: RouteScore,
    pub route_class: RouteClass,
    pub stats: ScanStats,
    pub geometry: Option<XGeometry>,
    pub structural: Option<XStructural>,
    pub swapped: bool,
    pub mixed_selection: bool,
    pub kp_a: usize,
    pub kp_b: usize,
    pub fallback: Option<Box<V42Report>>,
    pub calibration: String,
    pub calibration_id: String,
    pub xcalibration: String,
    pub xcalibration_id: String,
}

fn refuse(reason: &'static str, base: &Profile, xp: &XProfile) -> XReport {
    XReport {
        comparator: X_COMPARATOR,
        verdict: "Indeterminate",
        class: String::new(),
        basis: Vec::new(),
        reasons: vec![reason],
        execution: Execution::Fast,
        reason,
        route: RouteScore::default(),
        route_class: RouteClass::Absent,
        stats: ScanStats::default(),
        geometry: None,
        structural: None,
        swapped: false,
        mixed_selection: false,
        kp_a: 0,
        kp_b: 0,
        fallback: None,
        calibration: base.name_str(),
        calibration_id: base.id_hex16(),
        xcalibration: xp.name_str(),
        xcalibration_id: xp.id_hex16(),
    }
}

/// What the caller needs to know (§28).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// the six-state verdict
    Full,
    /// whether the pair reaches Suspected or Copy: a pair the lattice can
    /// no longer lift above Related is reported `NotCopy` without the
    /// evidence that would only have told Related from Unrelated — the
    /// reading a search ranks by
    Copy,
}

/// The verdict word of a Copy-scope stop.
pub const V_NOT_COPY: &str = "NotCopy";
pub const R_BELOW_SUSPECTED: &str = "BELOW_SUSPECTED";

/// Options of one comparison.
#[derive(Clone, Copy, Debug)]
pub struct XOptions {
    /// `POLICY_FAST`, `POLICY_SAFE`, `POLICY_EXACT`, or `None` for the
    /// profile's
    pub policy: Option<u8>,
    /// also run EXACT42 beside the fast path and attach its report (audit)
    pub audit: bool,
    pub scope: Scope,
}

impl Default for XOptions {
    fn default() -> Self {
        XOptions { policy: None, audit: false, scope: Scope::Full }
    }
}

/// Is a state at or below Related?
fn below_suspected(state: &str) -> bool {
    state == "Related" || state == "Unrelated"
}

/// Every corner at or below Related?
fn all_below_suspected(c: &Corners, s: &Structural, g_lo: i64, g_hi: i64, p: &Profile) -> bool {
    let covs: [i64; 2] = match s.coverage_min() {
        Some(v) => [v; 2],
        None => [0, SCALE],
    };
    for hi in [false, true] {
        let channels = s.channels_corner(hi);
        for &g in [g_lo, g_hi].iter() {
            for &cv in covs.iter() {
                let x = LatticeIn {
                    identical: c.identical,
                    channels,
                    geo_measurable: c.geo_measurable,
                    geo_evidence: g,
                    total_inliers: c.total_inliers,
                    topology_class: c.topology_class,
                    diversity: c.diversity,
                    coverage_min: cv,
                    any_mirror_model: c.any_mirror,
                };
                if !below_suspected(lattice_v4(&x, p).state) {
                    return false;
                }
            }
        }
    }
    true
}

// --------------------------------------------------------------- lattice

/// The lattice evaluated on the corners of what is unknown.
struct Corners {
    identical: bool,
    geo_measurable: bool,
    total_inliers: i64,
    topology_class: u8,
    diversity: i64,
    any_mirror: bool,
}

/// Every state the lattice can take with the unknown structural channels
/// at their extremes, the geometric evidence in `[g_lo, g_hi]` and the
/// coverage minimum either known or spanning its range.  Returns the one
/// verdict if all corners agree.
///
/// The weighted structural score is monotone in every channel value and the
/// lattice reads it only through thresholds, so its two extremes — every
/// unknown channel at 0 with the local channel at its lower bound, and
/// every unknown at SCALE with the local channel at its upper bound —
/// bracket every state an intermediate value could reach.
fn decide(c: &Corners, s: &Structural, g_lo: i64, g_hi: i64, p: &Profile) -> Option<V4Verdict> {
    let covs: [i64; 2] = match s.coverage_min() {
        Some(v) => [v; 2],
        None => [0, SCALE],
    };
    let mut first: Option<V4Verdict> = None;
    for hi in [false, true] {
        let channels = s.channels_corner(hi);
        for &g in [g_lo, g_hi].iter() {
            for &cv in covs.iter() {
                let x = LatticeIn {
                    identical: c.identical,
                    channels,
                    geo_measurable: c.geo_measurable,
                    geo_evidence: g,
                    total_inliers: c.total_inliers,
                    topology_class: c.topology_class,
                    diversity: c.diversity,
                    coverage_min: cv,
                    any_mirror_model: c.any_mirror,
                };
                let v = lattice_v4(&x, p);
                match &first {
                    None => first = Some(v),
                    Some(f) => {
                        if f.state != v.state {
                            return None;
                        }
                    }
                }
            }
        }
    }
    first
}

// ------------------------------------------------------------- the cascade

/// The comparison, both sides already in canonical order and the profiles
/// accepted.  `route` may be supplied by a batch caller, and `prescanned`
/// says the caller already ran `ctx.m.begin` and scanned at least the
/// anchor tier of this pair, so the scan continues rather than restarts.
#[allow(clippy::too_many_arguments)]
pub fn xcompare_in(
    ca: &XPrepared,
    cb: &XPrepared,
    swapped: bool,
    cfg: &Config,
    xb: &XBound,
    opts: &XOptions,
    route: Option<RouteScore>,
    prescanned: bool,
    ctx: &mut XCtx,
) -> XReport {
    let (base, xp): (&Profile, &XProfile) = (&xb.base, &xb.xp);
    let policy = opts.policy.unwrap_or(xp.fallback_policy);
    let mut r = refuse("", base, xp);
    r.reasons.clear();
    r.swapped = swapped;
    r.kp_a = if swapped { cb.p.kp.len() } else { ca.p.kp.len() };
    r.kp_b = if swapped { ca.p.kp.len() } else { cb.p.kp.len() };
    r.mixed_selection = ca.p.select != cb.p.select;

    // X0 — identity
    if ca.p.t1.bytes == cb.p.t1.bytes {
        r.verdict = "Identical";
        r.class = "byte-identical fingerprint".into();
        r.basis = vec!["bytes"];
        r.reason = "identical";
        if policy == POLICY_EXACT || opts.audit {
            r.execution = Execution::Audit;
        }
        return r;
    }
    if ca.p.t2_error.is_some() || cb.p.t2_error.is_some() {
        r.verdict = "Indeterminate";
        r.reasons = vec![R_CORRUPT];
        r.reason = R_CORRUPT;
        return r;
    }

    // X5 first when asked: the exact comparator IS the answer
    if policy == POLICY_EXACT {
        let mut pc = PairCtx::new(&ca.p, &cb.p);
        let v42 = compare_in(&mut pc, swapped, cfg, base, Reading::Lean);
        return adopt(r, v42, Execution::Audit, "exact");
    }

    // X1 — route
    let route = route.unwrap_or_else(|| route_score(&ca.route, &cb.route));
    let rc = route_class(&route, xp);
    r.route = route;
    r.route_class = rc;

    // X2 — sparse retrieval, anchors first
    let anchors = xp.anchors[0] as usize;
    if !prescanned {
        ctx.m.begin(ca, cb);
    }
    ctx.m.scan_rows(ca, cb, xp, anchors);
    let limit = cfg.geo_min_corr.max(1);
    let mut cd = ctx.m.count(false, ca, cb, base, limit);
    let mut cm = if cfg.mirror_hypothesis { ctx.m.count(true, ca, cb, base, limit) } else { ScreenCount::default() };
    let na = ca.p.kp.len().min(cb.p.kp.len());

    // a route hard-negative with no sparse evidence at the anchor tier:
    // Unrelated, by calibration (§12.2) — on the fast path only.  The route
    // bars were calibrated so that no comparator-42 Copy is rejected, not
    // so that every rejected pair reads Unrelated under 42: a few read
    // Related or Suspected there (a dithered or heavily resampled copy 42
    // only suspects).  The safe policy promises 42's states, so it takes
    // the sparse path instead and reads what 42 reads; the profile knob
    // turns the shortcut off for the fast path too.  The bars were
    // calibrated on one corpus, and a route is a sketch: on the PAPH-SI
    // corpus X1's put 49 of 976 copies in the Reject class, two of them
    // with empty pools — a channel swap and a palette shuffle 42 certifies
    // on structure alone.
    // Under a profile with `gate_door` the shortcut also needs the door
    // shut
    if rc == RouteClass::Reject
        && cd.count.max(cm.count) as i32 <= xp.defer_pool_max
        && xp.route_reject_unrelated == 1
        && policy == POLICY_FAST
        && !(xp.gate_door != 0 && structural_door(ca, cb, base, &mut ctx.d0))
    {
        r.stats = ctx.m.stats;
        r.verdict = "Unrelated";
        r.class = "no agreement above chance (route)".into();
        r.reason = "route";
        return r;
    }
    // thin at the anchor tier: look further, one tier at a time, while the
    // pools keep growing — a crop or a paste keeps its evidence wherever
    // the anchors are not, an unrelated pair gains nothing from more rows
    // the pools keep growing when a tier adds at least one correspondence
    // per 32 rows: anchors are spread over the work, so a shared region
    // of a thirtieth of it yields that much; less is the trickle of
    // chance matches a busy work produces whatever the tier
    let mut tier_scan = 1usize;
    while cd.count.max(cm.count) < cfg.geo_min_corr && ctx.m.stats.rows < na && tier_scan < xp.anchors.len() {
        let before = cd.count.max(cm.count);
        let rows_before = ctx.m.stats.rows;
        ctx.m.scan_rows(ca, cb, xp, xp.anchors[tier_scan] as usize);
        cd = ctx.m.count(false, ca, cb, base, limit);
        cm = if cfg.mirror_hypothesis { ctx.m.count(true, ca, cb, base, limit) } else { ScreenCount::default() };
        tier_scan += 1;
        let added = ctx.m.stats.rows - rows_before;
        if cd.count.max(cm.count) < before + (added / 32).max(1) {
            break;
        }
    }
    let thin = cd.count.max(cm.count) < cfg.geo_min_corr;

    // frames
    ctx.am.clear();
    ctx.am.extend(mirror_side(&ca.p.kp, ca.p.t1_xmax()));
    let (mda, mdb) = (ca.p.t1_max_dim(), cb.p.t1_max_dim());
    let geo_measurable = ca.p.kp.len() >= cfg.geo_min_corr && cb.p.kp.len() >= cfg.geo_min_corr;

    // X3 — adaptive geometry
    let mut tier = 0usize;
    let mut mm: MultiX = MultiX::empty();
    let (mut meas, mut weak) = (0i64, 0i64);
    let mut cert = false;
    let mut anchor_inliers: Option<i64> = None;
    // the inlier lists are `measure`'s; when geometry does not run they
    // must not carry the previous pair's into this pair's coverage
    ctx.g.inlier_a.clear();
    ctx.g.inlier_b.clear();
    ctx.g.inlier_level.clear();
    ctx.trace.clear();
    if !thin && geo_measurable {
        loop {
            ctx.m.pools(ca, cb, base);
            if !cfg.mirror_hypothesis {
                ctx.m.pm.clear();
            }
            let f = Frames { a: &ca.p.kp, am: &ctx.am, b: &cb.p.kp, mda, mdb };
            let (x, m_, w_) = measure(&f, &ctx.m.pd, &ctx.m.pm, cfg, base, &mut ctx.g);
            mm = x;
            meas = m_;
            weak = w_;
            // §9.4 — the certificate
            let stable = match anchor_inliers {
                None => true,
                Some(prev) => mm.total_inliers * 100 >= prev * xp.cert_stability_pct as i64,
            };
            let spread = {
                let inl = std::mem::take(&mut ctx.g.inlier_b);
                let cov = super::geom::coverage_into(&inl, base.grid_g, &mut ctx.g);
                ctx.g.inlier_b = inl;
                cov.occupied
            };
            let med_ok = mm.models().iter().all(|m| m.median_err <= xp.cert_max_median_err as i64);
            cert = mm.nmodels > 0
                && mm.total_inliers >= xp.cert_min_inliers as i64
                && mm.unique_a >= xp.cert_min_unique as usize
                && mm.unique_b >= xp.cert_min_unique as usize
                && spread >= xp.cert_min_cells as i64
                && med_ok
                && stable;
            if anchor_inliers.is_none() {
                anchor_inliers = Some(mm.total_inliers);
            }
            ctx.trace.push([ctx.m.stats.rows as i64, ctx.m.pd.len() as i64, ctx.m.pm.len() as i64, mm.total_inliers, mm.nmodels as i64, cert as i64, meas]);
            if cert || tier + 1 >= xp.anchors.len() || ctx.m.stats.rows >= na {
                break;
            }
            tier += 1;
            ctx.m.scan_rows(ca, cb, xp, xp.anchors[tier] as usize);
        }
    }
    r.stats = ctx.m.stats;
    let pool_d = ctx.m.pd.len();
    let pool_m = ctx.m.pm.len();

    // geometry evidence: raw now, the control only when it can matter
    let raw = (meas * SCALE / cfg.geo_conf_at as i64).clamp(0, SCALE);
    let (div, cov) = diversity(&mm, &ca.p.kp, &cb.p.kp, base, &mut ctx.g);
    let topo = topology(&mm, &cov, base.grid_g, base.thresholds[8] as i64, cfg.geo_min_corr);
    let pool_max = pool_d.max(pool_m) as i64;
    // the control cannot exceed the pool it permutes
    let ctl_hi = (pool_max * SCALE / cfg.geo_conf_at as i64).clamp(0, SCALE);
    let ev = |ctl: i64| -> i64 { idiv(base.lut_geometry.eval(chance_correct(raw, ctl)) * div.multiplier, SCALE).clamp(0, SCALE) };
    let mut g_lo = ev(ctl_hi);
    let mut g_hi = ev(0);
    let mut ctl_val = 0i64;
    let mut ctl_member = "gn:none";
    let mut ctl_ran = false;

    // X4 — structural evidence, cheapest first
    let mut s = Structural::new(ca, cb, base);
    for k in [CH_RUNS, CH_PALETTE, CH_SILHOUETTE, CH_TOPOLOGY] {
        s.compute(k, ca, cb, base, &mut ctx.d0);
    }
    let corners = Corners {
        identical: false,
        geo_measurable,
        total_inliers: meas,
        topology_class: topo,
        diversity: s.diversity,
        any_mirror: mm.any_mirror(),
    };
    // evidence steps, each taken only while the lattice is undecided.  The
    // structural steps go cheapest first; the geometry control is taken
    // ahead of them whenever geometry at the top of its interval would
    // decide the verdict by itself — on the anchor-tier pools it is the
    // cheaper step, and the structural channels it skips are the ones a
    // certified geometric copy never needs (§10.2, §28)
    let mut step = 0usize;
    let mut not_copy = false;
    let run_control = |ctx: &mut XCtx, g_lo: &mut i64, g_hi: &mut i64, ctl_val: &mut i64, ctl_member: &mut &'static str, ctl_ran: &mut bool| {
        if *ctl_ran || !geo_measurable || thin {
            return;
        }
        let f = Frames { a: &ca.p.kp, am: &ctx.am, b: &cb.p.kp, mda, mdb };
        let (c, name) = control(&f, &ctx.m.pd, &ctx.m.pm, cfg, base, &mut ctx.g, cfg.geo_conf_at as i64);
        *ctl_val = (c * SCALE / cfg.geo_conf_at as i64).clamp(0, SCALE);
        *ctl_member = name;
        *ctl_ran = true;
        let e = ev(*ctl_val);
        *g_lo = e;
        *g_hi = e;
    };
    let verdict: V4Verdict = loop {
        if let Some(v) = decide(&corners, &s, g_lo, g_hi, base) {
            break v;
        }
        if opts.scope == Scope::Copy && all_below_suspected(&corners, &s, g_lo, g_hi, base) {
            not_copy = true;
            break V4Verdict { state: V_NOT_COPY, class: "below Suspected on every remaining corner".into(), basis: Vec::new(), structural: 0, certifiable: false, secondaries: 0 };
        }
        // geometry resolved to the top of its interval decides on its own:
        // the control settles it either way, so it goes first
        let geo_blocks = !ctl_ran && geo_measurable && !thin && g_lo != g_hi && decide(&corners, &s, g_hi, g_hi, base).is_some();
        if geo_blocks {
            run_control(ctx, &mut g_lo, &mut g_hi, &mut ctl_val, &mut ctl_member, &mut ctl_ran);
            continue;
        }
        match step {
            0 => s.bound_local(ca, cb, base, &mut ctx.d0),
            1 => s.compute(CH_SHAPE, ca, cb, base, &mut ctx.d0),
            2 => s.compute(CH_DCT, ca, cb, base, &mut ctx.d0),
            3 => run_control(ctx, &mut g_lo, &mut g_hi, &mut ctl_val, &mut ctl_member, &mut ctl_ran),
            4 => s.compute(CH_LOCAL, ca, cb, base, &mut ctx.d0),
            _ => {
                // everything is exact; the corners must agree now
                let v = decide(&corners, &s, g_lo, g_hi, base).expect("exact inputs decide");
                break v;
            }
        }
        step += 1;
    };
    let geo_evidence = if ctl_ran { g_lo } else { g_hi };
    let margin = if ctl_ran { chance_correct(raw, ctl_val) } else { chance_correct(raw, 0) };

    // X5 — is the fast verdict safe to state?
    // geometry is pivotal when the verdict changes with geometry silenced,
    // or with geometry at the most the pool could have given (a control
    // at zero): in both directions the sparse pool, not the exhaustive
    // one, decided — so the certificate must hold, and a control that
    // saturated on a pool whose measurement is itself at the confidence
    // point is a structured accident the exact comparator must re-judge
    let silenced = Corners { geo_measurable: false, total_inliers: 0, topology_class: 0, any_mirror: false, ..corners };
    let pivotal = match decide(&silenced, &s, 0, 0, base) {
        Some(v) => v.state != verdict.state,
        None => true,
    };
    let potential = ev(0);
    // A copy-scope stop is not a lattice state: geometry at its potential
    // moves it when it lifts any corner to Suspected or above.  Without
    // this a NotCopy stated after a saturated control skipped the deferral
    // below — and once the control has zeroed the evidence, a structure
    // whose upper bound sits under the moderate bar reads below Suspected on
    // every corner, so the copy-scope stop came first: a pasted copy
    // comparator 42 certifies on geometry alone (its exhaustive pool's
    // control does not saturate) read NotCopy, `FAST` (`sibench lost`, 1.2;
    // CAL-007's higher moderate bar made the stop reachable there)
    let upward = if not_copy {
        !all_below_suspected(&corners, &s, potential, potential, base)
    } else {
        match decide(&corners, &s, potential, potential, base) {
            Some(v) => v.state != verdict.state,
            None => true,
        }
    };
    let saturated = ctl_ran && ctl_val >= SCALE && meas >= cfg.geo_conf_at as i64;
    // a truncated pool is evidence that was thrown away; a row that needed
    // two shared projections is not (its strong matches share many more)
    let unsafe_sparse = ctx.m.truncated;
    let defer = unsafe_sparse || (pivotal && !cert && !below_suspected(verdict.state) && !not_copy) || (upward && saturated);
    let (lo, hi) = s.bounds(base);
    r.geometry = Some(XGeometry {
        anchors,
        expanded_to: ctx.m.stats.rows,
        pool_direct: pool_d,
        pool_mirror: pool_m,
        models: mm.models().to_vec(),
        total_inliers: meas,
        weak_inliers: weak,
        measurable: geo_measurable,
        raw,
        ctl: ctl_val,
        ctl_member,
        ctl_ran,
        margin,
        evidence: geo_evidence,
        diversity: div,
        coverage: Some(cov),
        topology: topo,
        certificate: cert,
        unique_a: mm.unique_a,
        unique_b: mm.unique_b,
    });
    r.structural = Some(XStructural {
        lo,
        hi,
        exact: s.exact(),
        channels: {
            let mut c = [("", 0i64, false, false); 7];
            for k in 0..7 {
                c[k] = (CH_NAMES[k], s.value[k], s.measurable[k], s.known[k]);
            }
            c
        },
        local_evidence: if s.known[CH_LOCAL] && s.measurable[CH_LOCAL] { Some(s.value[CH_LOCAL]) } else { None },
        local_matches: s.local.as_ref().map(|l| l.matches),
        diversity: s.diversity,
        coverage_min: s.coverage_min(),
        certifiable: verdict.certifiable,
    });

    if defer {
        if policy == POLICY_SAFE {
            let mut pc = PairCtx::new(&ca.p, &cb.p);
            let v42 = compare_in(&mut pc, swapped, cfg, base, Reading::Lean);
            let reason = if unsafe_sparse { "pool-truncated" } else if upward && saturated { "control-saturated" } else { "uncertified-geometry" };
            return adopt(r, v42, Execution::Fallback, reason);
        }
        r.verdict = "Indeterminate";
        r.class = String::new();
        r.basis = Vec::new();
        r.reasons = vec![R_FALLBACK_REQUIRED];
        r.execution = Execution::Deferred;
        r.reason = if unsafe_sparse { "pool-truncated" } else if upward && saturated { "control-saturated" } else { "uncertified-geometry" };
        return r;
    }
    r.verdict = verdict.state;
    r.class = verdict.class;
    r.basis = verdict.basis;
    r.execution = Execution::Fast;
    r.reason = if not_copy { R_BELOW_SUSPECTED } else if s.exact() && ctl_ran { "exact-evidence" } else { "bounded-evidence" };
    if not_copy {
        r.reasons = vec![R_BELOW_SUSPECTED];
    }
    if opts.audit {
        let mut pc = PairCtx::new(&ca.p, &cb.p);
        let v42 = compare_in(&mut pc, swapped, cfg, base, Reading::Lean);
        r.fallback = Some(Box::new(v42));
        r.execution = Execution::Audit;
    }
    r
}

/// Take the verdict of a comparator-42 report into an X report.
fn adopt(mut r: XReport, v42: V42Report, exec: Execution, reason: &'static str) -> XReport {
    r.verdict = v42.base.verdict;
    r.class = v42.base.class.clone();
    r.basis = v42.base.basis.clone();
    r.reasons = v42.base.reasons.clone();
    r.execution = exec;
    r.reason = reason;
    if let Some(s) = r.structural.as_mut() {
        if v42.base.verdict != "Indeterminate" {
            s.lo = v42.base.structural;
            s.hi = v42.base.structural;
            s.exact = true;
            s.certifiable = v42.base.certifiable;
        }
    }
    r.fallback = Some(Box::new(v42));
    r
}

/// The full entry: profiles checked, canonical order chosen.
pub fn xcompare(a: &XPrepared, b: &XPrepared, cfg: &Config, xb: &XBound, opts: &XOptions, ctx: &mut XCtx) -> XReport {
    if let Some(reason) = check_profiles(a, b, xb) {
        return refuse(reason, &xb.base, &xb.xp);
    }
    let cfg = xb.bind(cfg);
    let swapped = canon_swapped(&a.p, &b.p);
    let (ca, cb) = if swapped { (b, a) } else { (a, b) };
    xcompare_in(ca, cb, swapped, &cfg, xb, opts, None, false, ctx)
}

// ------------------------------------------------------------------- JSON

fn model_json(m: &ModelRec42) -> J {
    obj![
        "r00" => m.r00, "r10" => m.r10, "tx" => m.tx, "ty" => m.ty,
        "scaleQ16" => m.scale_q16, "mirror" => m.mirror, "inliers" => m.inliers,
        "medianErr" => m.median_err, "confSum" => m.conf_sum,
    ]
}

pub fn report_json(r: &XReport) -> J {
    let route = obj![
        "local" => r.route.local, "band" => r.route.band, "global" => r.route.global,
        "measurable" => r.route.measurable as i64,
        "class" => match r.route_class { RouteClass::Fast => "FAST", RouteClass::Defer => "DEFER", RouteClass::Reject => "REJECT", RouteClass::Absent => "ABSENT" },
    ];
    let sparse = obj![
        "directPool" => r.geometry.as_ref().map(|g| g.pool_direct as i64).unwrap_or(0),
        "mirrorPool" => r.geometry.as_ref().map(|g| g.pool_mirror as i64).unwrap_or(0),
        "touchedPairs" => r.stats.touched as i64,
        "hammingPairs" => r.stats.hammings as i64,
        "fullPairs" => (2 * r.stats.full_pairs) as i64,
        "rowsScanned" => r.stats.rows,
        "denseRows" => r.stats.dense_rows,
        "explosion" => r.stats.explosion,
    ];
    let geometry = match &r.geometry {
        None => J::Null,
        Some(g) => obj![
            "anchors" => g.anchors, "expandedTo" => g.expanded_to,
            "inliers" => g.total_inliers, "weakInliers" => g.weak_inliers,
            "models" => J::Arr(g.models.iter().map(model_json).collect()),
            "measurable" => g.measurable, "raw" => g.raw, "ctl" => g.ctl, "ctlMember" => g.ctl_member,
            "ctlRan" => g.ctl_ran, "margin" => g.margin, "evidence" => g.evidence,
            "diversity" => obj![
                "spatial" => g.diversity.spatial, "scale" => g.diversity.scale, "model" => g.diversity.model,
                "descriptor" => g.diversity.descriptor, "combined" => g.diversity.combined, "multiplier" => g.diversity.multiplier,
            ],
            "coverage" => g.coverage.as_ref().map(crate::report::coverage),
            "topology" => g.topology, "certificate" => g.certificate,
            "uniqueA" => g.unique_a, "uniqueB" => g.unique_b,
        ],
    };
    let structural = match &r.structural {
        None => J::Null,
        Some(s) => obj![
            "lo" => s.lo, "hi" => s.hi, "exact" => s.exact,
            "channels" => J::Obj(s.channels.iter().map(|(n, v, m, k)| (*n, obj!["value" => *v, "measurable" => *m, "known" => *k])).collect()),
            "localEvidence" => s.local_evidence, "localMatches" => s.local_matches,
            "diversity" => s.diversity, "coverageMin" => s.coverage_min, "certifiable" => s.certifiable,
        ],
    };
    obj![
        "comparator" => r.comparator,
        "verdict" => r.verdict,
        "class" => r.class.clone(),
        "basis" => arr(r.basis.iter().copied()),
        "reasons" => arr(r.reasons.iter().copied()),
        "execution" => r.execution.name(),
        "reason" => r.reason,
        "route" => route,
        "sparse" => sparse,
        "geometry" => geometry,
        "structural" => structural,
        "swapped" => r.swapped,
        "selection" => obj!["mixed" => r.mixed_selection],
        "kpA" => r.kp_a, "kpB" => r.kp_b,
        "fallback" => r.fallback.as_ref().map(|v| crate::report::v42(v)).unwrap_or(J::Null),
        "calibration" => r.calibration.clone(), "calibrationId" => r.calibration_id.clone(),
        "xcalibration" => r.xcalibration.clone(), "xcalibrationId" => r.xcalibration_id.clone(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::Prepared;
    use crate::wire::hash;
    use crate::x::testimg::{crop, image, mirror, recolour};

    fn side(px: &[u8], w: usize, h: usize, xb: &XBound) -> XPrepared {
        let rot = crate::keypoints::RotCache::new(&crate::keypoints::pattern());
        let f = hash(px, w, h, &Config::default(), &rot);
        XPrepared::new(Prepared::new(&f.t1, Some(&f.t2)).unwrap(), xb)
    }

    #[test]
    fn cascade_agrees_with_42_on_the_fixture_family() {
        let cfg = Config::default();
        let xb = XBound::shipped();
        let base = &xb.base;
        let (w, h) = (192, 144);
        let px = image(12, w, h);
        let sides = vec![
            side(&px, w, h, &xb),
            side(&mirror(&px, w, h), w, h, &xb),
            side(&crop(&px, w, 30, 20, 120, 100), 120, 100, &xb),
            side(&recolour(&px), w, h, &xb),
            side(&image(900, w, h), w, h, &xb),
            side(&image(901, 96, 96), 96, 96, &xb),
        ];
        let mut ctx = XCtx::new();
        for i in 0..sides.len() {
            for j in 0..sides.len() {
                let r42 = crate::v42::compare_v42_lean(&sides[i].p, &sides[j].p, &cfg, base);
                for policy in [POLICY_SAFE, POLICY_FAST] {
                    let rx = xcompare(&sides[i], &sides[j], &cfg, &xb, &XOptions { policy: Some(policy), ..Default::default() }, &mut ctx);
                    let ryx = xcompare(&sides[j], &sides[i], &cfg, &xb, &XOptions { policy: Some(policy), ..Default::default() }, &mut ctx);
                    assert_eq!(rx.verdict, ryx.verdict, "symmetry {i}x{j} policy {policy}");
                    assert_eq!(rx.execution, ryx.execution, "symmetry {i}x{j} policy {policy}");
                    println!("{i}x{j} p{policy}: 42 {} | X {} {} ({}) rows {} ham {}/{}", r42.base.verdict, rx.verdict, rx.execution.name(), rx.reason, rx.stats.rows, rx.stats.hammings, 2 * rx.stats.full_pairs);
                    if policy == POLICY_SAFE {
                        // under the safe policy the verdict is 42's unless the
                        // fast path certified it itself — and then it must
                        // agree on the copy question; on this family it
                        // agrees on every state
                        let copy42 = r42.base.verdict == "Copy" || r42.base.verdict == "Identical";
                        let copyx = rx.verdict == "Copy" || rx.verdict == "Identical";
                        assert_eq!(copy42, copyx, "{i}x{j}: 42 {} vs X {} ({})", r42.base.verdict, rx.verdict, rx.reason);
                        assert_ne!(rx.verdict, "Indeterminate");
                        assert_eq!(rx.verdict, r42.base.verdict, "{i}x{j}: safe state {} vs 42 {} ({}, {})", rx.verdict, r42.base.verdict, rx.execution.name(), rx.reason);
                    } else if rx.execution == Execution::Fast {
                        let copy42 = r42.base.verdict == "Copy" || r42.base.verdict == "Identical";
                        let copyx = rx.verdict == "Copy" || rx.verdict == "Identical";
                        assert_eq!(copy42, copyx, "fast {i}x{j}: 42 {} vs X {}", r42.base.verdict, rx.verdict);
                    }
                }
                let j1 = report_json(&xcompare(&sides[i], &sides[j], &cfg, &xb, &XOptions::default(), &mut ctx)).to_string();
                assert!(j1.contains("\"comparator\":50"));
            }
        }
    }

    #[test]
    fn screen_rejects_unrelated_and_passes_copies() {
        let cfg = Config::default();
        let xb = XBound::shipped();
        let base = &xb.base;
        // real pixel art, not the shared-texture noise fixture: two works
        // that share nothing but a style
        let art = crate::synth::pixel_art(160, 120, 11, 8, 2);
        let other = crate::synth::pixel_art(160, 120, 1234, 9, 2);
        let mir = crate::synth::mirror(&art);
        let a = side(&art.px, 160, 120, &xb);
        let m = side(&mir.px, 160, 120, &xb);
        let u = side(&other.px, 160, 120, &xb);
        let mut ctx = XCtx::new();
        let s = xscreen(&a, &m, &cfg, &xb, &mut ctx);
        assert_eq!(s.state, ScreenState::Pass, "{:?}", s);
        let s2 = xscreen(&m, &a, &cfg, &xb, &mut ctx);
        assert_eq!((s.state, s.direct.count, s.mirror.count), (s2.state, s2.direct.count, s2.mirror.count));
        let s3 = xscreen(&a, &u, &cfg, &xb, &mut ctx);
        assert_eq!(s3.state, ScreenState::Reject, "{:?}", s3);
        assert!(!s3.stats.explosion);
        assert_eq!(xscreen(&a, &a, &cfg, &xb, &mut ctx).state, ScreenState::Identical);
        // the shared-texture noise fixture: comparator 42 itself passes it
        // (its descriptors collide everywhere), and so must X — a screen
        // that rejects what 42 would verify is a recall gate
        let (w, h) = (160, 120);
        let n1 = side(&image(44, w, h), w, h, &xb);
        let n2 = side(&image(4545, w, h), w, h, &xb);
        let s42 = crate::v42::screen_v42_prepared(&n1.p, &n2.p, &cfg, base);
        let sx = xscreen(&n1, &n2, &cfg, &xb, &mut ctx);
        assert_eq!(sx.state == ScreenState::Pass, s42.pass, "42 {:?} vs X {:?}", s42, sx);
        println!("mirror screen {:?}\nunrelated screen {:?}\nnoise screen {:?}", s, s3, sx);
    }

    /// A report must not depend on what the scratch context compared
    /// before: a thin pair after a geometric one reports the same text as
    /// from a fresh context (the coverage of no inliers is no cells).
    #[test]
    fn reports_are_history_free() {
        let cfg = Config::default();
        let xb = XBound::shipped();
        let (w, h) = (192, 144);
        let px = image(12, w, h);
        let a = side(&px, w, h, &xb);
        let m = side(&mirror(&px, w, h), w, h, &xb);
        let c = side(&crop(&px, w, 30, 20, 120, 100), 120, 100, &xb);
        let u = side(&image(901, 96, 96), 96, 96, &xb);
        let art = crate::synth::pixel_art(160, 120, 11, 8, 2);
        let art2 = crate::synth::pixel_art(160, 120, 1234, 9, 2);
        let p1 = side(&art.px, 160, 120, &xb);
        let p2 = side(&art2.px, 160, 120, &xb);
        let sides = [&a, &m, &c, &u, &p1, &p2];
        let mut warm = XCtx::new();
        for policy in [None, Some(POLICY_FAST), Some(POLICY_SAFE)] {
            for &x in sides.iter() {
                for &y in sides.iter() {
                    // warm the context on a geometric pair, then compare
                    let _ = xcompare(&a, &m, &cfg, &xb, &XOptions { policy, ..Default::default() }, &mut warm);
                    let o = XOptions { policy, ..Default::default() };
                    let r1 = report_json(&xcompare(x, y, &cfg, &xb, &o, &mut warm)).to_string();
                    let r2 = report_json(&xcompare(x, y, &cfg, &xb, &o, &mut XCtx::new())).to_string();
                    assert_eq!(r1, r2, "policy {policy:?}");
                    let s1 = format!("{:?}", xscreen(x, y, &cfg, &xb, &mut warm));
                    let s2 = format!("{:?}", xscreen(x, y, &cfg, &xb, &mut XCtx::new()));
                    assert_eq!(s1, s2);
                }
            }
        }
    }

    /// A copy-scope stop after a saturated control is deferred like any
    /// other verdict the control zeroed.  The pair is the PAPH-SI corpus's
    /// base 29 pasted into its host (`sibench lost`, 1.2): comparator 42
    /// certifies it on geometry alone, while on the anchor tier's sparse
    /// pools the control saturates and the evidence reads 0 — and under
    /// CAL-007, whose moderate bar the structural upper bound (3085) stays
    /// under, every corner then read below Suspected and the cascade stated
    /// NotCopy, `FAST`.  Under CAL-004 the same bound kept the lattice open,
    /// so the full verdict deferred.
    #[test]
    fn copy_scope_defers_a_saturated_control() {
        use crate::synth::{paste, pixel_art, work};
        let cfg = Config::default();
        let base = work(400, 90, 36, false);
        let host = pixel_art(820, 196, 1057, 9, 2);
        let copy = paste(&base, &host, 203, 35);
        for xb in [XBound::shipped(), XBound::x2()] {
            let (a, b) = (side(&base.px, base.w, base.h, &xb), side(&copy.px, copy.w, copy.h, &xb));
            let r42 = crate::v42::compare_v42_lean(&a.p, &b.p, &cfg, &xb.base);
            assert_eq!(r42.base.verdict, "Copy");
            let mut ctx = XCtx::new();
            let safe = XOptions { policy: Some(POLICY_SAFE), audit: false, scope: Scope::Copy };
            let r = xcompare(&a, &b, &cfg, &xb, &safe, &mut ctx);
            assert_eq!((r.verdict, r.execution, r.reason), ("Copy", Execution::Fallback, "control-saturated"), "{}", xb.xp.name_str());
            let fast = XOptions { policy: Some(POLICY_FAST), ..safe };
            let r = xcompare(&a, &b, &cfg, &xb, &fast, &mut ctx);
            assert_eq!((r.verdict, r.execution), ("Indeterminate", Execution::Deferred), "{}", xb.xp.name_str());
            // XRank, shown the target alone, in both arrival orders
            let mut rs = crate::x::rank::RankScratch::new();
            for (q, t) in [(&a, &b), (&b, &a)] {
                let mut out = vec![0i32; crate::x::rank::XRANK_FIELDS];
                crate::x::rank::xrank(q, &[Some(t)], &cfg, &xb, &crate::x::rank::XRankOptions::default(), &mut ctx, &mut rs, &mut out);
                assert_eq!(out[0], crate::abi::state_code("Copy"), "{}", xb.xp.name_str());
            }
        }
    }
}
