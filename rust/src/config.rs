//! Compare-time configuration.
//!
//! SPEC-003 P1: every knob here is a COMPARE-time choice.  Moving one
//! re-decides a pair without re-hashing anything, which is the whole argument
//! for the larger wire budget.  The hash-time fields — the front end's, the
//! local windows and counts, the keypoint budget and rule, and `wire`, the
//! format itself — do change the wire, and sit apart in `Config`.

/// Fixed-point scale for every channel reading: 0 ..= 10000.
pub const SCALE: i64 = 10_000;

/// SPEC-004.2 §2 — the Tier-2 keypoint budget.  The record count on the wire
/// was ALREADY a `u16` and the records are a fixed 40 bytes, so raising the
/// cap from 256 to 512 costs no format change: Tier 2 becomes at most
/// `32 + 512 x 40 = 20512` bytes and a 256-keypoint wire still parses.
pub const MAX_KP_COUNT: usize = 512;

/// Keypoint selection rules (hash time).
///   0 — 4.1: strongest-first, round-robin over an 8x8 grid
///   1 — 4.2: the quality score of SPEC-004.2 §3
pub const KP_SELECT_LEGACY: i32 = 0;
pub const KP_SELECT_QUALITY: i32 = 1;

/// Wire formats the hasher writes (the Tier-1 and Tier-2 version byte).
///   3 — PAPH-X 1.0–1.1: the sampling of SPEC-003 §6
///   4 — PAPH-X 1.2 (docs/SPEC-W4-paph-wire4.md): the same sections, sampled
///       so that a mirrored or quarter-turned image hashes to the mirrored or
///       quarter-turned sections exactly, on canvases of any size
pub const WIRE_3: u8 = 3;
pub const WIRE_4: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scoring {
    /// the weakest of evidence and corroboration — refuses to certify on one channel
    Gate,
    /// weighted mean over measurable channels
    Weighted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// purity x confidence
    Lift,
    /// share of achievable match
    Proportion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RagEndpoint {
    /// survives a rebuilt palette
    Quantile,
    /// does not — so agreement means the palette was NOT rebuilt
    Rank,
}

#[derive(Clone, Copy, Debug)]
pub struct Config {
    // --- hash time: these DO change the wire ---
    pub fold_matte: bool,
    pub divide_upscale: bool,
    pub matte_tol: i32,
    pub peak_radius: i32,
    pub fold_invert: bool,
    pub local_windows: [i32; 2],
    pub local_count: usize,
    pub kp_count: usize,
    pub sketch_count: usize,
    /// SPEC-004.2 §3 — which keypoint-selection rule builds Tier 2.
    pub kp_select: i32,
    /// `WIRE_4` (the default) or `WIRE_3` — which wire format `hash` writes.
    /// Comparisons read either; a pair of different formats is refused.
    pub wire: u8,

    // --- compare time: free to re-derive at any moment ---
    pub hamming_t: i32,
    pub evidence: Evidence,
    pub confidence_at: i32,
    pub scoring: Scoring,
    pub rag_endpoint: RagEndpoint,
    pub geo_enabled: bool,
    pub geo_conf_at: i32,
    pub geo_eps: i32,
    pub geo_min_corr: usize,
    pub mirror_hypothesis: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            fold_matte: true,
            divide_upscale: true,
            matte_tol: 24,
            peak_radius: 5,
            fold_invert: true,
            local_windows: [8, 16],
            local_count: 128,
            kp_count: MAX_KP_COUNT,
            sketch_count: 32,
            kp_select: KP_SELECT_QUALITY,
            wire: WIRE_4,

            hamming_t: 8,
            evidence: Evidence::Lift,
            confidence_at: 16,
            scoring: Scoring::Weighted,
            rag_endpoint: RagEndpoint::Rank,
            geo_enabled: true,
            geo_conf_at: 16,
            geo_eps: 1600,
            geo_min_corr: 8,
            mirror_hypothesis: true,
        }
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(0..=64).contains(&self.hamming_t) {
            return Err("hammingT out of range");
        }
        if self.confidence_at < 1 {
            return Err("confidenceAt must be at least 1");
        }
        if self.geo_conf_at < 1 {
            return Err("geoConfAt must be at least 1");
        }
        if !(1..=32767).contains(&self.geo_eps) {
            return Err("geoEps out of range");
        }
        if !(4..=128).contains(&self.local_count) {
            return Err("localCount out of range (wire holds 128)");
        }
        if self.kp_count > MAX_KP_COUNT {
            return Err("kpCount out of range (tier 2 holds 512)");
        }
        if !(KP_SELECT_LEGACY..=KP_SELECT_QUALITY).contains(&self.kp_select) {
            return Err("kpSelect must be 0 (4.1) or 1 (4.2)");
        }
        if self.sketch_count > 32 {
            return Err("sketchCount out of range (tier 1 holds 32)");
        }
        if self.wire != WIRE_3 && self.wire != WIRE_4 {
            return Err("wire must be 3 or 4");
        }
        Ok(())
    }
}

/// Verdict thresholds.
///
/// The structural side was derived from 16 real works rather than fixtures.
/// The geometric side is a PROPOSAL — SPEC-003 §14.7 says re-derive it on real
/// moderation reports, not on a 16-work corpus.
pub struct Thresholds;
impl Thresholds {
    pub const STRUCT_IDENTICAL: i64 = 8000;
    pub const STRUCT_STRONG: i64 = 4500;
    pub const STRUCT_MODERATE: i64 = 3000;
    pub const STRUCT_WEAK: i64 = 1500;
    pub const GEO_STRONG: i64 = 3500;
    pub const GEO_WEAK: i64 = 1200;
    /// geometry certifying on its own needs more than this many inliers
    pub const GEO_SOLO_INLIERS: i64 = 15;
    /// 1.5 x STRUCT_STRONG — structure certifying on its own
    pub const STRUCT_SOLO: i64 = 6750;
}

/// Channel weights for `Scoring::Weighted`.
pub fn weight(name: &str) -> i64 {
    match name {
        "local" => 35,
        "shape" => 25,
        "topology" => 15,
        "runs" => 10,
        "dct" => 10,
        "palette" => 5,
        "silhouette" => 10,
        _ => 0,
    }
}

// ---- small deterministic helpers, shared by every module ----

/// Integer divide, truncating toward zero — matches JavaScript's `(a / b) | 0`.
#[inline(always)]
pub fn idiv(a: i64, b: i64) -> i64 {
    if b == 0 {
        0
    } else {
        a / b
    }
}

#[inline(always)]
pub fn clamp(v: i64, lo: i64, hi: i64) -> i64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// Exact integer square root: the largest `r` with `r * r <= n`.
///
/// Below 2^52 every `n` is exactly representable and IEEE `sqrt` is correctly
/// rounded on every target, so the guess `r = floor(sqrt(n))` is already the
/// exact floor root there: a non-integer root `s` lies at least `1/(2s)` from
/// both neighbouring integers, which is more than half an ulp of `s` for
/// `s < 2^26`.  The single downward correction is kept as a guard.  Above
/// 2^52, where `n` itself is rounded, the guess is corrected in integers in
/// both directions until `r² <= n < (r+1)²` holds exactly.  Either way the
/// result is the exact floor square root on every engine and platform, which
/// is the only property any caller depends on; `isqrt_bits`, the bit-by-bit
/// method this replaces, is kept and tested equal.
#[inline]
pub fn isqrt(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let r = (n as f64).sqrt() as i64;
    if n < (1i64 << 52) {
        return if r * r > n { r - 1 } else { r };
    }
    isqrt_wide(n, r)
}

#[cold]
#[inline(never)]
fn isqrt_wide(n: i64, mut r: i64) -> i64 {
    // at most a step or two either way; the bounds keep r*r from overflowing
    while r > 0 && (r > 3_037_000_499 || r * r > n) {
        r -= 1;
    }
    while r < 3_037_000_499 && (r + 1) * (r + 1) <= n {
        r += 1;
    }
    r
}

/// The bit-by-bit integer square root `isqrt` is held to.
pub fn isqrt_bits(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n as u64;
    let mut r: u64 = 0;
    let mut bit: u64 = 1u64 << 62;
    while bit > x {
        bit >>= 2;
    }
    while bit != 0 {
        if x >= r + bit {
            x -= r + bit;
            r = (r >> 1) + bit;
        } else {
            r >>= 1;
        }
        bit >>= 2;
    }
    r as i64
}

#[cfg(test)]
mod isqrt_tests {
    use super::*;

    #[test]
    fn isqrt_is_exact_floor() {
        // squares, their neighbours, and the extremes, where an f64 guess
        // rounds the wrong way
        let mut probes: Vec<i64> = vec![0, 1, 2, 3, 4, 5, 8, 9, 10, i64::MAX, i64::MAX - 1];
        for k in [1i64, 2, 3, 1000, 65535, 65536, 94906265, 94906266, 3_037_000_498, 3_037_000_499] {
            for d in -2i64..=2 {
                let v = k.saturating_mul(k).saturating_add(d);
                if v >= 0 {
                    probes.push(v);
                }
            }
        }
        let mut s = 0x0123_4567_89ab_cdefu64;
        for _ in 0..200_000 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            probes.push((s >> (s % 63)) as i64 & i64::MAX);
        }
        for &n in probes.iter() {
            assert_eq!(isqrt(n), isqrt_bits(n), "n = {n}");
        }
    }

    #[test]
    fn isqrt_fast_path_near_squares() {
        // every k*k - 1, k*k, k*k + 1 below 2^52 on a stride, plus the top of
        // the fast path, where the f64 guess is most likely to round up
        let top = 1i64 << 26; // (2^26)^2 = 2^52
        let mut ks: Vec<i64> = (1..200_000).collect();
        ks.extend((1..2_000).map(|i| top - i));
        ks.extend((0..20_000).map(|i| 3 + i * 3_331));
        for &k in ks.iter() {
            for d in -1i64..=1 {
                let n = k * k + d;
                if n > 0 {
                    assert_eq!(isqrt(n), isqrt_bits(n), "n = {n}");
                }
            }
        }
        for n in [(1i64 << 52) - 1, 1i64 << 52, (1i64 << 52) + 1] {
            assert_eq!(isqrt(n), isqrt_bits(n), "n = {n}");
        }
    }
}

/// Subtract a channel's own chance floor, in the 0..SCALE currency.
#[inline]
pub fn chance_correct(raw: i64, ctl: i64) -> i64 {
    if ctl >= SCALE {
        0
    } else {
        clamp((raw - ctl) * SCALE / (SCALE - ctl), 0, SCALE)
    }
}
