//! Calibration profile X1 (PAPH-X specification §20, §43).
//!
//! Everything that can change an XRoute, an XMatch or an XRank result lives
//! here, as one immutable byte artefact with a SHA-256 identity — the same
//! discipline as the comparator-42 `.pcal`.  Two nodes holding different
//! projection tables, anchor schedules or route bars must never claim to
//! have produced the same PAPH-X result, so the identity covers all of them,
//! and it also covers the identity of the comparator-42 profile the lattice
//! thresholds come from (`base_id`): an X profile is bound to exactly one
//! base profile.
//!
//! The numerical values shipped here are IMPLEMENTATION DEFAULTS for
//! calibration (§43), not calibrated decision constants.  The artefact says
//! PROVISIONAL and the benchmark harness (`xbench`) measures what they do.

use crate::calibration::{Profile, COMPARATOR_V42};
use crate::config::Config;
use crate::sha256::{hex, sha256};

pub const X_COMPARATOR: u16 = 50;
pub const X_PROFILE_VERSION: u16 = 1;
const MAGIC: &[u8; 4] = b"PXCL";

/// Route lane counts (§6.1).  Fixed by the route format; recorded in the
/// artefact so the identity covers them.
pub const LOCAL_LANES: usize = 64;
pub const BAND_LANES: usize = 32;
pub const GLOBAL_WORDS: usize = 4;
/// LSH projections (§8.2): 24 projections of 12 bits, mirror-closed — the
/// second twelve are the first twelve with every position moved by 128,
/// which is exactly what a mirrored descriptor does to its bits.
///
/// Twelve bits, not the eight of §43: with eight, 24 probes into 256
/// buckets nominate n/10 of the candidates even for perfectly uniform
/// codes (46 of 512), and a nominated candidate costs about five times
/// what the streaming exhaustive kernel pays per pair, so the sparse scan
/// only broke even.  At twelve bits the uniform expectation is n/170 and
/// the measured one on pixel art is a few dozen per row for both
/// hypotheses together, while a correspondence at Hamming 40 is still
/// found by at least one of the 24 projections with probability 0.99
/// (0.84 at 50, 0.48 at 70 — the tail the geometry never needed).
pub const LSH_PROJECTIONS: usize = 24;
pub const LSH_BITS: usize = 12;
pub const LSH_BASE: usize = LSH_PROJECTIONS / 2;
/// buckets per projection
pub const LSH_BUCKETS: usize = 1 << LSH_BITS;
pub const MAX_KP: usize = crate::config::MAX_KP_COUNT;
pub const MAX_TOTAL_CORR: usize = 1024;

/// Fallback policy (§29): how a DEFER is resolved.
pub const POLICY_FAST: u8 = 0;
pub const POLICY_SAFE: u8 = 1;
pub const POLICY_EXACT: u8 = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XProfile {
    pub version: u16,
    pub comparator: u16,
    pub name: [u8; 16],
    /// identity of the comparator-42 profile whose lattice this binds to
    pub base_id: [u8; 32],

    // ---- XRoute (§6)
    pub route_seed: u64,
    /// fast-positive certificates: equal lanes (local, band) and agreeing
    /// bits (global, of 256) at or above which the route alone passes
    pub t_local_fast: i32,
    pub t_band_fast: i32,
    pub t_global_fast: i32,
    /// lower bars: with EVERY measurable family below its bar the route is
    /// a hard negative
    pub t_local_low: i32,
    pub t_band_low: i32,
    pub t_global_low: i32,
    /// fewer local codes than this on a side and the local family abstains
    pub route_min_codes: i32,
    /// fewer keypoints than this on a side and the band family abstains
    pub route_min_kp: i32,

    // ---- XBucket / XMatch (§8)
    pub lsh_seed: u64,
    /// the 192 bit positions of the twelve base projections (the twelve
    /// mirror partners are these + 128 mod 256)
    pub lsh_table: [u8; LSH_BASE * LSH_BITS],
    pub hot_bucket_cap: i32,
    /// support (equal projections) a candidate needs when it was only ever
    /// found through buckets above `hot_soft`
    pub support_hot: i32,
    pub hot_soft: i32,
    pub max_sparse_candidates: i32,

    // ---- XAnchor (§9)
    pub anchors: [i32; 4],
    /// certificate (§9.4): a model is FAST-strong when it has at least this
    /// many inliers, unique A and B keypoints, this many occupied coverage
    /// cells, a median residual at most this, and the expanded model keeps
    /// at least this percentage of the anchor model's inliers
    pub cert_min_inliers: i32,
    pub cert_min_unique: i32,
    pub cert_min_cells: i32,
    pub cert_max_median_err: i32,
    pub cert_stability_pct: i32,

    // ---- scheduler / policy (§10–§11, §29)
    pub fallback_policy: u8,
    /// 1: under the fast policy a route hard-negative with no sparse
    /// evidence reads Unrelated (calibrated rejection); 0: it takes the
    /// sparse path like any other pair.  The safe policy always takes the
    /// sparse path: its states are comparator 42's
    pub route_reject_unrelated: u8,
    /// sparse pools at or below this, with a non-weak route, DEFER
    pub defer_pool_max: i32,
}

fn w32(b: &mut Vec<u8>, v: i32) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn r32(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn r64(b: &[u8], o: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[o..o + 8]);
    u64::from_le_bytes(a)
}

/// splitmix64 — the one mixer every X hash uses.  Exact integer arithmetic,
/// identical on every target.
#[inline(always)]
pub fn mix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// A seeded stream of `mix64` values.
pub struct Seq(pub u64);
impl Seq {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(1);
        mix64(self.0 ^ 0x5851_f42d_4c95_7f2d)
    }
}

/// The X1 projection table: twelve base projections of eight bit positions,
/// chosen by `xbench --table` from the descriptors of the synthetic corpus
/// — each bit greedily minimising the expected collision rate of its
/// projection's code over unrelated art, weighted by the bit's flip rate
/// under the corpus' transforms (mirror, rotations, rescales, recolours,
/// crops, pastes), balanced bits only, mirror partners excluded, every bit
/// used once.  Mean collision rate 0.0066 per projection against 0.0132 for
/// a seeded random table and 0.0039 for a uniform code.  Re-derive it on
/// the production corpus before the profile loses its PROVISIONAL mark;
/// a different table is a different profile (§20).
pub const TABLE_X1: [u8; LSH_BASE * LSH_BITS] = [
    34, 212, 170, 45, 93, 85, 198, 205, 44, 231, 215, 99,
    162, 27, 232, 8, 18, 61, 248, 40, 32, 251, 184, 143,
    84, 173, 62, 146, 52, 167, 96, 25, 227, 218, 56, 189,
    222, 128, 77, 107, 5, 195, 37, 12, 70, 188, 172, 225,
    155, 244, 29, 20, 83, 168, 110, 120, 182, 206, 39, 213,
    0, 94, 145, 181, 190, 24, 161, 239, 47, 138, 16, 226,
    124, 9, 228, 209, 123, 158, 87, 90, 194, 152, 10, 147,
    53, 157, 116, 235, 133, 55, 241, 67, 165, 109, 233, 108,
    252, 2, 149, 76, 203, 102, 201, 33, 208, 112, 144, 98,
    223, 137, 17, 91, 180, 111, 71, 140, 160, 15, 60, 179,
    134, 26, 164, 79, 125, 19, 171, 48, 150, 103, 192, 185,
    42, 6, 245, 196, 216, 100, 148, 253, 115, 242, 75, 49,
];

/// A seeded projection table, for experiments: for each of the twelve base
/// projections, eight distinct bit positions in 0..256 drawn without
/// replacement across the whole table as far as 256 positions allow (96
/// positions, all distinct, none in a mirror pair with another — a bit and
/// its mirror partner say the same thing about an unrelated patch), then
/// filled by rejection.
pub fn default_table(seed: u64) -> [u8; LSH_BASE * LSH_BITS] {
    let mut out = [0u8; LSH_BASE * LSH_BITS];
    let mut s = Seq(seed ^ 0x7a62_5f0c_1d3e_9b41);
    for p in 0..LSH_BASE {
        let mut k = 0usize;
        while k < LSH_BITS {
            let b = (s.next() % 256) as u8;
            let t = &out[p * LSH_BITS..p * LSH_BITS + k];
            if t.iter().any(|&x| x == b || x as usize == (b as usize + 128) & 255) {
                continue;
            }
            out[p * LSH_BITS + k] = b;
            k += 1;
        }
    }
    out
}

impl XProfile {
    pub fn name_str(&self) -> String {
        let end = self.name.iter().position(|&c| c == 0).unwrap_or(16);
        String::from_utf8_lossy(&self.name[..end]).into_owned()
    }

    /// X1-PROVISIONAL bound to CAL-004-PROPOSED — the shipped defaults.
    pub fn x1() -> XProfile {
        Self::x1_for(&Profile::cal004())
    }

    /// X1-PROVISIONAL bound to `base`.
    pub fn x1_for(base: &Profile) -> XProfile {
        let mut name = [0u8; 16];
        name[..14].copy_from_slice(b"X1-PROVISIONAL");
        let lsh_seed = 0x5041_5048_5831_0001; // "PAPHX1\0\1"
        XProfile {
            version: X_PROFILE_VERSION,
            comparator: X_COMPARATOR,
            name,
            base_id: base.id(),
            route_seed: 0x5041_5048_5852_0001,
            // route bars from `xbench --calibrate` on the synthetic corpus:
            // the lower bars are the largest that reject no comparator-42
            // Copy there (local 6 / band 3 / global 190, 71% of negatives
            // rejected), the fast bars sit above every negative's reading
            t_local_fast: 7,
            t_band_fast: 5,
            t_global_fast: 203,
            t_local_low: 6,
            t_band_low: 3,
            t_global_low: 190,
            route_min_codes: 4,
            route_min_kp: 8,
            lsh_seed,
            lsh_table: TABLE_X1,
            hot_bucket_cap: 16,
            support_hot: 2,
            hot_soft: 8,
            max_sparse_candidates: 96,
            anchors: [96, 160, 256, 512],
            cert_min_inliers: 12,
            cert_min_unique: 12,
            cert_min_cells: 3,
            cert_max_median_err: 4000,
            cert_stability_pct: 80,
            fallback_policy: POLICY_SAFE,
            route_reject_unrelated: 1,
            defer_pool_max: 3,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(400);
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&self.version.to_le_bytes());
        b.extend_from_slice(&self.comparator.to_le_bytes());
        b.extend_from_slice(&self.name);
        b.extend_from_slice(&self.base_id);
        b.extend_from_slice(&self.route_seed.to_le_bytes());
        for v in [
            LOCAL_LANES as i32, BAND_LANES as i32, GLOBAL_WORDS as i32,
            self.t_local_fast, self.t_band_fast, self.t_global_fast,
            self.t_local_low, self.t_band_low, self.t_global_low,
            self.route_min_codes, self.route_min_kp,
        ] {
            w32(&mut b, v);
        }
        b.extend_from_slice(&self.lsh_seed.to_le_bytes());
        for v in [LSH_PROJECTIONS as i32, LSH_BITS as i32] {
            w32(&mut b, v);
        }
        b.extend_from_slice(&self.lsh_table);
        for v in [
            self.hot_bucket_cap, self.support_hot, self.hot_soft, self.max_sparse_candidates,
            MAX_TOTAL_CORR as i32,
        ] {
            w32(&mut b, v);
        }
        for v in self.anchors {
            w32(&mut b, v);
        }
        for v in [
            self.cert_min_inliers, self.cert_min_unique, self.cert_min_cells,
            self.cert_max_median_err, self.cert_stability_pct,
        ] {
            w32(&mut b, v);
        }
        b.push(self.fallback_policy);
        b.push(self.route_reject_unrelated);
        w32(&mut b, self.defer_pool_max);
        b
    }

    pub fn decode(b: &[u8]) -> Result<XProfile, &'static str> {
        const LEN: usize = 4 + 2 + 2 + 16 + 32 + 8 + 11 * 4 + 8 + 2 * 4 + LSH_BASE * LSH_BITS + 5 * 4 + 4 * 4 + 5 * 4 + 2 + 4;
        if b.len() != LEN {
            return Err("x profile length");
        }
        if &b[0..4] != MAGIC {
            return Err("bad x profile magic");
        }
        let version = u16::from_le_bytes([b[4], b[5]]);
        let comparator = u16::from_le_bytes([b[6], b[7]]);
        if version != X_PROFILE_VERSION || comparator != X_COMPARATOR {
            return Err("x profile targets another comparator");
        }
        let mut name = [0u8; 16];
        name.copy_from_slice(&b[8..24]);
        let mut base_id = [0u8; 32];
        base_id.copy_from_slice(&b[24..56]);
        let mut o = 56;
        let route_seed = r64(b, o);
        o += 8;
        let mut f = [0i32; 11];
        for v in f.iter_mut() {
            *v = r32(b, o);
            o += 4;
        }
        if f[0] != LOCAL_LANES as i32 || f[1] != BAND_LANES as i32 || f[2] != GLOBAL_WORDS as i32 {
            return Err("route lane layout");
        }
        let lsh_seed = r64(b, o);
        o += 8;
        if r32(b, o) != LSH_PROJECTIONS as i32 || r32(b, o + 4) != LSH_BITS as i32 {
            return Err("lsh layout");
        }
        o += 8;
        let mut lsh_table = [0u8; LSH_BASE * LSH_BITS];
        lsh_table.copy_from_slice(&b[o..o + LSH_BASE * LSH_BITS]);
        o += LSH_BASE * LSH_BITS;
        let mut g = [0i32; 5];
        for v in g.iter_mut() {
            *v = r32(b, o);
            o += 4;
        }
        if g[4] != MAX_TOTAL_CORR as i32 {
            return Err("corr capacity");
        }
        let mut anchors = [0i32; 4];
        for v in anchors.iter_mut() {
            *v = r32(b, o);
            o += 4;
        }
        let mut c = [0i32; 5];
        for v in c.iter_mut() {
            *v = r32(b, o);
            o += 4;
        }
        let fallback_policy = b[o];
        let route_reject_unrelated = b[o + 1];
        o += 2;
        let defer_pool_max = r32(b, o);
        let p = XProfile {
            version,
            comparator,
            name,
            base_id,
            route_seed,
            t_local_fast: f[3],
            t_band_fast: f[4],
            t_global_fast: f[5],
            t_local_low: f[6],
            t_band_low: f[7],
            t_global_low: f[8],
            route_min_codes: f[9],
            route_min_kp: f[10],
            lsh_seed,
            lsh_table,
            hot_bucket_cap: g[0],
            support_hot: g[1],
            hot_soft: g[2],
            max_sparse_candidates: g[3],
            anchors,
            cert_min_inliers: c[0],
            cert_min_unique: c[1],
            cert_min_cells: c[2],
            cert_max_median_err: c[3],
            cert_stability_pct: c[4],
            fallback_policy,
            route_reject_unrelated,
            defer_pool_max,
        };
        p.validate()?;
        Ok(p)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != X_PROFILE_VERSION || self.comparator != X_COMPARATOR {
            return Err("x profile version");
        }
        if !(0..=LOCAL_LANES as i32).contains(&self.t_local_fast) || !(0..=LOCAL_LANES as i32).contains(&self.t_local_low) {
            return Err("local route bars");
        }
        if !(0..=BAND_LANES as i32).contains(&self.t_band_fast) || !(0..=BAND_LANES as i32).contains(&self.t_band_low) {
            return Err("band route bars");
        }
        if !(0..=256).contains(&self.t_global_fast) || !(0..=256).contains(&self.t_global_low) {
            return Err("global route bars");
        }
        if self.t_local_low > self.t_local_fast || self.t_band_low > self.t_band_fast || self.t_global_low > self.t_global_fast {
            return Err("route low bar above fast bar");
        }
        if !(0..=128).contains(&self.route_min_codes) || !(0..=512).contains(&self.route_min_kp) {
            return Err("route minimums");
        }
        for p in 0..LSH_BASE {
            let t = &self.lsh_table[p * LSH_BITS..(p + 1) * LSH_BITS];
            for i in 0..LSH_BITS {
                for j in 0..i {
                    if t[i] == t[j] || t[i] as usize == (t[j] as usize + 128) & 255 {
                        return Err("projection repeats a bit or holds a mirror pair");
                    }
                }
            }
        }
        if !(1..=512).contains(&self.hot_bucket_cap) || !(1..=24).contains(&self.support_hot) || !(0..=512).contains(&self.hot_soft) {
            return Err("bucket policy");
        }
        if !(1..=512).contains(&self.max_sparse_candidates) {
            return Err("sparse candidate cap");
        }
        let a = self.anchors;
        if a[0] < 8 || a[0] > a[1] || a[1] > a[2] || a[2] > a[3] || a[3] != MAX_KP as i32 {
            return Err("anchor schedule");
        }
        if self.cert_min_inliers < 2 || self.cert_min_unique < 2 || self.cert_min_cells < 1 || self.cert_max_median_err < 0 || !(0..=100).contains(&self.cert_stability_pct) {
            return Err("certificate");
        }
        if self.fallback_policy > POLICY_EXACT || self.route_reject_unrelated > 1 || self.defer_pool_max < 0 {
            return Err("policy");
        }
        Ok(())
    }

    pub fn id(&self) -> [u8; 32] {
        sha256(&self.encode())
    }
    pub fn id_hex16(&self) -> String {
        hex(&self.id()[..8])
    }

    /// The full 24-projection table: base projections then their mirror
    /// partners (positions + 128 mod 256).
    pub fn projections(&self) -> [[u8; LSH_BITS]; LSH_PROJECTIONS] {
        let mut out = [[0u8; LSH_BITS]; LSH_PROJECTIONS];
        for p in 0..LSH_BASE {
            for i in 0..LSH_BITS {
                let b = self.lsh_table[p * LSH_BITS + i];
                out[p][i] = b;
                out[p + LSH_BASE][i] = ((b as usize + 128) & 255) as u8;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x1_roundtrips_and_has_an_identity() {
        let p = XProfile::x1();
        p.validate().unwrap();
        let b = p.encode();
        let q = XProfile::decode(&b).unwrap();
        assert_eq!(p, q);
        assert_eq!(b, q.encode());
        assert_eq!(p.id_hex16().len(), 16);
        println!("X1-PROVISIONAL id {} ({} bytes)", hex(&p.id()), b.len());
        // bound to CAL-004: a different base changes the identity
        let mut base = Profile::cal004();
        base.thresholds[1] += 1;
        assert_ne!(XProfile::x1_for(&base).id(), p.id());
    }

    #[test]
    fn table_is_mirror_closed_and_distinct() {
        let p = XProfile::x1();
        let t = p.projections();
        for q in 0..LSH_BASE {
            for i in 0..LSH_BITS {
                assert_eq!(t[q + LSH_BASE][i] as usize, (t[q][i] as usize + 128) & 255);
                for j in 0..i {
                    assert_ne!(t[q][i], t[q][j], "distinct within a projection");
                }
            }
        }
        assert!(XProfile::decode(&p.encode()).is_ok());
        let mut q = p.clone();
        q.lsh_table = default_table(7);
        q.validate().unwrap();
    }

    #[test]
    fn tampering_is_refused() {
        let p = XProfile::x1();
        let b = p.encode();
        let mut x = b.clone();
        x[0] = b'Q';
        assert!(XProfile::decode(&x).is_err());
        let mut x = b.clone();
        x.push(0);
        assert!(XProfile::decode(&x).is_err());
        let mut q = p.clone();
        q.lsh_table[1] = q.lsh_table[0];
        assert!(q.validate().is_err());
        let mut q = p.clone();
        q.anchors = [96, 90, 256, 512];
        assert!(q.validate().is_err());
    }
}

/// A comparator-42 profile and the X profile bound to it, with everything
/// derived from them computed once: identities, route salts, projection
/// table, the bound compare-time config.  Every X entry point takes one of
/// these, so no comparison hashes a profile.
pub struct XBound {
    pub base: Profile,
    pub xp: XProfile,
    pub base_id: [u8; 32],
    pub xid: [u8; 32],
    pub salts: super::route::RouteSalts,
    pub projections: [[u8; LSH_BITS]; LSH_PROJECTIONS],
    /// why this pair of profiles cannot compare, if it cannot
    pub refused: Option<&'static str>,
}

impl XBound {
    pub fn new(base: Profile, xp: XProfile) -> XBound {
        let base_id = base.id();
        let xid = xp.id();
        let refused = if base.validate().is_err() || base.comparator != COMPARATOR_V42 || xp.validate().is_err() {
            Some(crate::v4::R_PROFILE_UNSUPPORTED)
        } else if xp.base_id != base_id {
            Some(crate::v4::R_PROFILE_MISMATCH)
        } else {
            None
        };
        let salts = super::route::RouteSalts::new(&xp);
        let projections = xp.projections();
        XBound { base, xp, base_id, xid, salts, projections, refused }
    }

    /// The shipped pair: CAL-004-PROPOSED and X1-PROVISIONAL.
    pub fn shipped() -> XBound {
        XBound::new(Profile::cal004(), XProfile::x1())
    }

    /// The compare-time config bound to the base profile.
    pub fn bind(&self, cfg: &Config) -> Config {
        crate::v4::bind(cfg, &self.base)
    }
}
