//! XPrepared — a prepared side with its PAPH-X accelerator structures.
//!
//! Everything here is DERIVED from the 4.2 wire at prepare time (§17, §18):
//! the route, the bucket index, the anchor order, the local bag's burst
//! profile and the per-side measurability of the structural channels.  None
//! of it is consensus state; a sidecar may cache it and may be discarded.

use super::anchor::anchor_order;
use super::bucket::XBucketIndex;
use super::profile::XBound;
use super::route::XRoute;
use crate::config::SCALE;
use crate::prepared::Prepared;
use crate::wire::{F_FLAT, F_SIL};

/// Which structural channels this side can measure (the abstention rules
/// of the v3 channels, evaluated on one side; a pair measures a channel
/// when both sides do).
pub const M_DCT: u8 = 1;
pub const M_LOCAL: u8 = 2;
pub const M_SHAPE: u8 = 4;
pub const M_TOPOLOGY: u8 = 8;
pub const M_RUNS: u8 = 16;
pub const M_PALETTE: u8 = 32;
pub const M_SILHOUETTE: u8 = 64;

pub struct XPrepared {
    pub p: Prepared,
    pub route: XRoute,
    pub index: XBucketIndex,
    /// keypoint indices, anchor order (§9.1)
    pub order: Vec<u16>,
    /// 1 + the number of OTHER codes within `hamming_t` of each local code
    /// (v3's `burst`), computed once per side
    pub burst: Vec<i64>,
    /// Σ SCALE / burst — the local channel's capacity term for this side
    pub cap_sum: i64,
    /// §10.4 per-side diversity: cap_sum / n
    pub d_side: i64,
    /// the `hamming_t` the bursts were computed at
    pub hamming_t: i32,
    pub meas: u8,
    /// identity of the X profile the structures were built under
    pub xid: [u8; 32],
}

fn runs_flat(t: &crate::wire::Tier1) -> bool {
    let x = t.sec("runs");
    for ax in 0..3usize {
        let (mut tot, mut top) = (0i64, 0i64);
        for i in 0..16usize {
            let v = x[ax * 16 + i] as i64;
            tot += v;
            top = top.max(v);
        }
        if tot > 0 && top * 100 < tot * 95 {
            return false;
        }
    }
    true
}

impl XPrepared {
    pub fn new(p: Prepared, xb: &XBound) -> XPrepared {
        let (base, xp) = (&xb.base, &xb.xp);
        let route = XRoute::build(&p, &xb.salts, xp);
        let index = XBucketIndex::build(&p.desc, &xb.projections, xp.hot_bucket_cap as usize);
        let order = anchor_order(&p.kp);
        let t = base.hamming_t;
        let n = p.bag.n;
        let mut burst = vec![1i64; n];
        for i in 0..n {
            let ci = ((p.bag.hi[i] as u64) << 32) | p.bag.lo[i] as u64;
            for j in 0..n {
                if j == i {
                    continue;
                }
                let cj = ((p.bag.hi[j] as u64) << 32) | p.bag.lo[j] as u64;
                if (ci ^ cj).count_ones() as i32 <= t {
                    burst[i] += 1;
                }
            }
        }
        let cap_sum: i64 = burst.iter().map(|&b| SCALE / b).sum();
        let d_side = if n > 0 { cap_sum / n as i64 } else { 0 };
        let t1 = &p.t1;
        let mut meas = 0u8;
        if t1.flags & F_FLAT == 0 {
            meas |= M_DCT;
        }
        if n >= 4 {
            meas |= M_LOCAL;
        }
        if t1.count("shapes") > 0 {
            meas |= M_SHAPE;
        }
        if t1.count("rag") >= 3 {
            meas |= M_TOPOLOGY;
        }
        if !runs_flat(t1) {
            meas |= M_RUNS;
        }
        if t1.count("palette") >= 2 {
            meas |= M_PALETTE;
        }
        if t1.flags & F_SIL != 0 {
            meas |= M_SILHOUETTE;
        }
        XPrepared { p, route, index, order, burst, cap_sum, d_side, hamming_t: t, meas, xid: xb.xid }
    }

    pub fn kp_len(&self) -> usize {
        self.p.kp.len()
    }
}
