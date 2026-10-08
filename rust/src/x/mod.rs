//! PAPH-X — retrieval-native comparison with sub-quadratic screening.
//!
//! Comparator 42 made the quadratic matcher fast; this layer makes it rare.
//! A pair is screened from a 128-byte route, matched through a projection
//! index that nominates tens of candidates per descriptor rather than 512,
//! verified on 96 anchor keypoints before any more are paid for, and judged
//! by a scheduler that stops evaluating evidence the moment the verdict
//! lattice can no longer move.  The exhaustive comparator 42 stays beside it
//! as `EXACT42`, the fallback every DEFER resolves to under the safe policy
//! and the audit path a moderator can always ask for.
//!
//! Module map (specification sections in brackets):
//!
//!   profile    the X1 calibration artefact                        [§20, §43]
//!   route      XRoute: MinHash + global words, batch SIMD compare  [§6, §7]
//!   bucket     XBucket: 24 projections, CSR buckets, mirror map    [§8.2–8.4]
//!   anchor     the deterministic anchor order                      [§9.1]
//!   prepared   XPrepared: a side with its derived structures       [§17, §18]
//!   matcher    the sparse mutual-best scan and the screen count    [§8.5–8.8, §24]
//!   geom       comparator-42 geometry on sparse pools, no allocs   [§9, §26]
//!   structural the structural channels, lazily, with bounds        [§10, §27]
//!   compare    the cascade X0–X5, the lattice scheduler, reports   [§5, §12, §13, §28]
//!   rank       XRank: route SIMD over candidates, then XMatch      [§7, §14]
//!   sidecar    the optional PAX1 accelerator cache                 [§18]
//!   abi        the C ABI exports (ABI 3)                            [§30]

pub mod abi;
pub mod anchor;
pub mod bucket;
pub mod compare;
pub mod geom;
pub mod matcher;
pub mod prepared;
pub mod profile;
pub mod rank;
pub mod route;
pub mod si;
pub mod sidecar;
pub mod structural;
#[cfg(test)]
pub mod testimg;

pub use compare::{xcompare, xscreen, XReport, XScreen, Execution, XCtx};
pub use prepared::XPrepared;
pub use profile::{XBound, XProfile};
pub use rank::{xrank, XRankRecord, XRANK_FIELDS};
pub use route::{XRoute, RouteScore, RouteClass};
