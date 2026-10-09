//! The C ABI the WebAssembly build exports.
//!
//! Deliberately NOT wasm-bindgen.  A consensus artefact should be auditable in
//! one sitting: a handful of exported functions, flat integer arguments,
//! length-prefixed byte blocks, and the JavaScript glue written by hand next
//! to it (`wasm/paph.js`).  A code generator between the source and the shipped
//! binary is one more thing two nodes would have to agree about.
//!
//! Memory
//! ------
//!   paph_alloc(len) -> ptr            caller-owned scratch, freed with
//!   paph_free(ptr, len)
//!   paph_release(block)               frees a block this module returned
//!
//! A *block* is `[u32 payload_len][u32 capacity][payload]`.
//!
//! Hashing
//! -------
//!   paph_hash(cfg, px, w, h)                 -> block [u32 t1n][u32 t2n][t1][t2]
//!   paph_hash_checked(cfg, px, w, h, limits) -> same; on a §16 limit the block
//!                                               is [0][u32 n][utf-8 message]
//!
//! Comparing (comparator 42)
//! -------------------------
//! A wire pair is parsed once into a *prepared side* (an opaque handle), and a
//! side is compared against as many others as you like without re-parsing it.
//! A calibration profile is a handle too (0 = the shipped CAL-007-PROVISIONAL;
//! 1.0–1.1's CAL-004-PROPOSED loads from its artefact).
//!
//!   paph_prepare(t1, t1n, t2, t2n, flags) -> handle | 0 (Tier 1 refused)
//!       flags bit 0: strict — a Tier 2 claiming more than 512 keypoints is
//!       refused as corrupt (a conforming hasher never writes one, and a
//!       comparison is quadratic in it).  Use it for anything user-supplied.
//!   paph_prepared_info(h, out_i32x4)     [kp count, tier-2 ok, width, height]
//!   paph_prepare_free(h)
//!   paph_profile(pcal, n) -> handle | 0  a .pcal artefact, decoded
//!   paph_profile_free(h)
//!   paph_compare42(cfg, prof, a, b, flags) -> block (utf-8 JSON report)
//!       flags bit 0: lean — `v3: null`, the same verdict, ~20% less work
//!   paph_screen42(cfg, prof, a, b) -> i32: pass | poolDirect << 1 | poolMirror << 12
//!   paph_rank42(cfg, prof, query, cands, n, flags, out) -> records written
//!       `cands` is n u32 handles; `out` receives n records of RANK_FIELDS i32
//!       (see `rank_record`).  flags bit 0: gate on the stage-1 screen (a pair
//!       the screen rejects is reported UNSCREENED and not compared).
//!   paph_local_codes(h, out_u32, cap) -> n written   the Tier-1 local codes
//!       (hi, lo), at most 128
//!   paph_descriptors(h, which, out_u32, cap) -> count   keypoint descriptors,
//!       8 u32 each, the first min(count, cap) written (cap 0 asks the count):
//!       which 0 = the compared list, 1 = the Tier-1 sketch, 2 = the compared
//!       list strongest first
//!
//! The config is a flat i32 array of CFG_FIELDS so the glue never serialises a
//! struct; a null pointer means the defaults.

use crate::calibration::{Profile, COMPARATOR_V42};
use crate::compare::Reading;
use crate::config::{Config, Evidence, RagEndpoint, Scoring, MAX_KP_COUNT};
use crate::keypoints::{pattern, RotCache};
use crate::prepared::{canon_swapped, PairCtx, Prepared};
use crate::v4::{bind, R_CORRUPT, R_PROFILE_UNSUPPORTED};
use crate::v42::{compare_in, screen_in};
use std::sync::OnceLock;

pub const CFG_FIELDS: usize = 22;
/// ABI 3: ABI 2 unchanged, plus the PAPH-X exports of `x::abi`.
/// ABI 4 (1.2): the config gains field 21, the wire format `hash` writes
/// (3 or 4; anything else reads as 4), and `paph_version` reports 4.
pub const ABI_VERSION: u32 = 4;

pub(crate) fn config_from(p: *const i32) -> Config {
    let mut c = Config::default();
    if p.is_null() {
        return c;
    }
    let v = unsafe { std::slice::from_raw_parts(p, CFG_FIELDS) };
    c.fold_matte = v[0] != 0;
    c.divide_upscale = v[1] != 0;
    c.matte_tol = v[2];
    c.peak_radius = v[3];
    c.fold_invert = v[4] != 0;
    c.local_windows = [v[5], v[6]];
    c.local_count = v[7].clamp(4, 128) as usize;
    c.kp_count = v[8].clamp(0, MAX_KP_COUNT as i32) as usize;
    c.sketch_count = v[9].clamp(0, 32) as usize;
    c.hamming_t = v[10];
    c.evidence = if v[11] == 1 { Evidence::Proportion } else { Evidence::Lift };
    c.confidence_at = v[12];
    c.scoring = if v[13] == 1 { Scoring::Weighted } else { Scoring::Gate };
    c.rag_endpoint = if v[14] == 1 { RagEndpoint::Rank } else { RagEndpoint::Quantile };
    c.geo_enabled = v[15] != 0;
    c.geo_conf_at = v[16];
    c.geo_eps = v[17];
    c.mirror_hypothesis = v[18] != 0;
    c.geo_min_corr = v[19].clamp(2, 64) as usize;
    c.kp_select = v[20].clamp(0, 1);
    c.wire = if v[21] == 3 { crate::config::WIRE_3 } else { crate::config::WIRE_4 };
    c
}

/// Write the shipping defaults into a caller-provided i32 array.
#[no_mangle]
pub extern "C" fn paph_default_config(out: *mut i32) {
    let c = Config::default();
    let v = unsafe { std::slice::from_raw_parts_mut(out, CFG_FIELDS) };
    v[0] = c.fold_matte as i32;
    v[1] = c.divide_upscale as i32;
    v[2] = c.matte_tol;
    v[3] = c.peak_radius;
    v[4] = c.fold_invert as i32;
    v[5] = c.local_windows[0];
    v[6] = c.local_windows[1];
    v[7] = c.local_count as i32;
    v[8] = c.kp_count as i32;
    v[9] = c.sketch_count as i32;
    v[10] = c.hamming_t;
    v[11] = (c.evidence == Evidence::Proportion) as i32;
    v[12] = c.confidence_at;
    v[13] = (c.scoring == Scoring::Weighted) as i32;
    v[14] = (c.rag_endpoint == RagEndpoint::Rank) as i32;
    v[15] = c.geo_enabled as i32;
    v[16] = c.geo_conf_at;
    v[17] = c.geo_eps;
    v[18] = c.mirror_hypothesis as i32;
    v[19] = c.geo_min_corr as i32;
    v[20] = c.kp_select;
    v[21] = c.wire as i32;
}

#[no_mangle]
pub extern "C" fn paph_version() -> u32 {
    crate::wire::VERSION as u32
}
#[no_mangle]
pub extern "C" fn paph_abi() -> u32 {
    ABI_VERSION
}
#[no_mangle]
pub extern "C" fn paph_t1_bytes() -> u32 {
    crate::wire::T1_BYTES as u32
}
#[no_mangle]
pub extern "C" fn paph_config_fields() -> u32 {
    CFG_FIELDS as u32
}
#[no_mangle]
pub extern "C" fn paph_rank_fields() -> u32 {
    RANK_FIELDS as u32
}

#[no_mangle]
pub extern "C" fn paph_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

#[no_mangle]
pub extern "C" fn paph_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        unsafe { drop(Vec::from_raw_parts(ptr, 0, len)) }
    }
}

/// A Vec handed to JavaScript as a block it reads and then releases.
pub(crate) fn out_block(payload: &[u8]) -> *mut u8 {
    let mut v = Vec::with_capacity(payload.len() + 8);
    v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    v.extend_from_slice(&((payload.len() + 8) as u32).to_le_bytes());
    v.extend_from_slice(payload);
    let mut v = std::mem::ManuallyDrop::new(v);
    v.as_mut_ptr()
}

#[no_mangle]
pub extern "C" fn paph_release(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let cap = u32::from_le_bytes([*ptr.add(4), *ptr.add(5), *ptr.add(6), *ptr.add(7)]) as usize;
        drop(Vec::from_raw_parts(ptr, 0, cap));
    }
}

fn rot() -> &'static RotCache {
    static ROT: OnceLock<RotCache> = OnceLock::new();
    ROT.get_or_init(|| RotCache::new(&pattern()))
}

fn wires_block(t1: &[u8], t2: &[u8]) -> *mut u8 {
    let mut payload = Vec::with_capacity(8 + t1.len() + t2.len());
    payload.extend_from_slice(&(t1.len() as u32).to_le_bytes());
    payload.extend_from_slice(&(t2.len() as u32).to_le_bytes());
    payload.extend_from_slice(t1);
    payload.extend_from_slice(t2);
    out_block(&payload)
}

/// -> block [u32 t1n][u32 t2n][t1][t2].  Unchecked, as shipped; prefer
/// `paph_hash_checked`.
#[no_mangle]
pub extern "C" fn paph_hash(cfg: *const i32, px: *const u8, w: u32, h: u32) -> *mut u8 {
    paph_hash_checked(cfg, px, w, h, std::ptr::null())
}

/// SPEC-004 §16: the limits are checked before a byte is read; `limits` is
/// null or three i32 (max width, max height, max pixels) that may only lower
/// the specification's.  A refused image returns [0][u32 n][message].
#[no_mangle]
pub extern "C" fn paph_hash_checked(cfg: *const i32, px: *const u8, w: u32, h: u32, limits: *const i32) -> *mut u8 {
    let (w, h) = (w as usize, h as usize);
    let c = config_from(cfg);
    let lim = if limits.is_null() { None } else { Some(unsafe { &*(limits as *const [i32; 3]) }) };
    // the length is checked by hash_checked against w*h*4; refuse anything
    // whose byte count cannot even be formed before touching the pointer
    let n = match w.checked_mul(h).and_then(|p| p.checked_mul(4)) {
        Some(n) if (w * h) as i64 <= crate::wire::MAX_PIXELS => n,
        _ => {
            let msg = b"limit: pixel count exceeds maximum";
            let mut payload = vec![0u8; 4];
            payload.extend_from_slice(&(msg.len() as u32).to_le_bytes());
            payload.extend_from_slice(msg);
            return out_block(&payload);
        }
    };
    let pixels: &[u8] = if n == 0 || px.is_null() { &[] } else { unsafe { std::slice::from_raw_parts(px, n) } };
    match crate::wire::hash_checked(pixels, w, h, &c, rot(), lim) {
        Ok(fp) => wires_block(&fp.t1, &fp.t2),
        Err(e) => {
            let mut payload = vec![0u8; 4];
            payload.extend_from_slice(&(e.len() as u32).to_le_bytes());
            payload.extend_from_slice(e.as_bytes());
            out_block(&payload)
        }
    }
}

/// The v3 comparator's JSON, as shipped (ABI 1).
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub extern "C" fn paph_compare(
    cfg: *const i32,
    a1: *const u8,
    a1n: usize,
    a2: *const u8,
    a2n: usize,
    b1: *const u8,
    b1n: usize,
    b2: *const u8,
    b2n: usize,
) -> *mut u8 {
    let c = config_from(cfg);
    let s = |p: *const u8, n: usize| -> Option<&'static [u8]> {
        if p.is_null() || n == 0 {
            None
        } else {
            Some(unsafe { std::slice::from_raw_parts(p, n) })
        }
    };
    let json = match crate::compare::compare(
        s(a1, a1n).unwrap_or(&[]),
        s(a2, a2n),
        s(b1, b1n).unwrap_or(&[]),
        s(b2, b2n),
        &c,
    ) {
        Ok(v) => crate::compare::to_json(&v),
        Err(e) => format!("{{\"error\":\"{}\"}}", e),
    };
    out_block(json.as_bytes())
}

// ------------------------------------------------------------ prepared sides

/// Parse a wire pair once.  0 when Tier 1 is refused.  A Tier 2 that does not
/// parse is kept as an error on the side: the comparator reports such a pair
/// Indeterminate (CORRUPT), the screen falls back to the Tier-1 sketch — both
/// exactly as they do when handed the bytes.
#[no_mangle]
pub extern "C" fn paph_prepare(t1: *const u8, t1n: usize, t2: *const u8, t2n: usize, flags: u32) -> *mut Prepared {
    if t1.is_null() {
        return std::ptr::null_mut();
    }
    let t1 = unsafe { std::slice::from_raw_parts(t1, t1n) };
    let t2 = if t2.is_null() || t2n == 0 { None } else { Some(unsafe { std::slice::from_raw_parts(t2, t2n) }) };
    match Prepared::new(t1, t2) {
        Ok(mut p) => {
            if flags & 1 != 0 && p.t2_error.is_none() && t2.is_some() && p.kp.len() > MAX_KP_COUNT {
                // strict: refuse rather than compare quadratically
                p = Prepared::sketch_only(p.t1, Some("tier 2 exceeds 512 keypoints"));
            }
            Box::into_raw(Box::new(p))
        }
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn paph_prepare_free(h: *mut Prepared) {
    if !h.is_null() {
        unsafe { drop(Box::from_raw(h)) }
    }
}

/// [keypoints compared, tier 2 accepted (1/0), width, height]
#[no_mangle]
pub extern "C" fn paph_prepared_info(h: *const Prepared, out: *mut i32) {
    if h.is_null() || out.is_null() {
        return;
    }
    let p = unsafe { &*h };
    let o = unsafe { std::slice::from_raw_parts_mut(out, 4) };
    o[0] = p.kp.len() as i32;
    o[1] = p.t2_error.is_none() as i32;
    o[2] = p.t1.width as i32;
    o[3] = p.t1.height as i32;
}

/// The Tier-1 local codes, `(hi, lo)` per code, as stored: canonical under the
/// eight symmetries of the square and the complement, so a mirrored, rotated
/// or inverted copy stores the same codes for the same regions.  These are the
/// natural exact-match keys of an index (see docs/SEARCH.md).
#[no_mangle]
pub extern "C" fn paph_local_codes(h: *const Prepared, out: *mut u32, cap: u32) -> u32 {
    if h.is_null() {
        return 0;
    }
    let b = unsafe { &(*h).bag };
    let n = b.len().min(cap as usize);
    if !out.is_null() {
        let o = unsafe { std::slice::from_raw_parts_mut(out, 2 * n) };
        for i in 0..n {
            let (hi, lo) = b.code(i);
            o[2 * i] = hi;
            o[2 * i + 1] = lo;
        }
    }
    n as u32
}

/// Keypoint descriptors, eight u32 per keypoint as stored: `which` 0 = the
/// keypoints a comparison uses (Tier 2, or the sketch when there is none), in
/// wire order; 1 = the 32-keypoint Tier-1 sketch; 2 = the compared keypoints
/// strongest first (the order the sketch was chosen in: strength descending,
/// then x, then y).  Returns how many there are, and writes the first `cap`
/// of them (so a call with `cap` 0 asks for the count).
#[no_mangle]
pub extern "C" fn paph_descriptors(h: *const Prepared, which: u32, out: *mut u32, cap: u32) -> u32 {
    if h.is_null() {
        return 0;
    }
    let p = unsafe { &*h };
    let owned: Vec<crate::keypoints::Keypoint>;
    let list: &[crate::keypoints::Keypoint] = match which {
        1 => {
            owned = crate::wire::read_sketch(&p.t1);
            &owned
        }
        2 => {
            let mut v = p.kp.clone();
            v.sort_by(|a, b| b.s.cmp(&a.s).then(a.x.cmp(&b.x)).then(a.y.cmp(&b.y)));
            owned = v;
            &owned
        }
        _ => &p.kp,
    };
    let n = list.len().min(cap as usize);
    if !out.is_null() && n > 0 {
        let o = unsafe { std::slice::from_raw_parts_mut(out, 8 * n) };
        for (i, k) in list.iter().take(n).enumerate() {
            o[8 * i..8 * i + 8].copy_from_slice(&k.desc);
        }
    }
    list.len() as u32
}

// ------------------------------------------------------------------ profiles

/// A calibration artefact (.pcal bytes), decoded.  0 when it does not decode.
/// A profile that decodes but does not validate is kept: comparing under it
/// reports Indeterminate (PROFILE_UNSUPPORTED), as the reference does.
#[no_mangle]
pub extern "C" fn paph_profile(p: *const u8, n: usize) -> *mut Profile {
    if p.is_null() {
        return std::ptr::null_mut();
    }
    match Profile::decode(unsafe { std::slice::from_raw_parts(p, n) }) {
        Ok(pr) => Box::into_raw(Box::new(pr)),
        Err(_) => std::ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn paph_profile_free(h: *mut Profile) {
    if !h.is_null() {
        unsafe { drop(Box::from_raw(h)) }
    }
}

/// The default comparator-42 profile: CAL-007-PROVISIONAL from 1.2.
fn shipped() -> &'static Profile {
    static P: OnceLock<Profile> = OnceLock::new();
    P.get_or_init(Profile::shipped)
}

pub(crate) fn profile_ref<'a>(h: *const Profile) -> &'a Profile {
    if h.is_null() {
        shipped()
    } else {
        unsafe { &*h }
    }
}

fn usable(p: &Profile) -> bool {
    p.validate().is_ok() && p.comparator == COMPARATOR_V42
}

// ---------------------------------------------------------------- comparing

/// The comparator-42 report as JSON — the same text the JavaScript engine's
/// `JSON.stringify(compare(...))` produces.
#[no_mangle]
pub extern "C" fn paph_compare42(cfg: *const i32, prof: *const Profile, a: *const Prepared, b: *const Prepared, flags: u32) -> *mut u8 {
    let c = config_from(cfg);
    let p = profile_ref(prof);
    let r = if a.is_null() || b.is_null() {
        crate::v42::refuse_report(R_CORRUPT, p)
    } else {
        let (a, b) = unsafe { (&*a, &*b) };
        let reading = if flags & 1 != 0 { Reading::Lean } else { Reading::Full };
        crate::v42::compare_v42_reading(a, b, &c, p, None, None, reading)
    };
    out_block(crate::report::v42(&r).to_string().as_bytes())
}

/// pass | poolDirect << 1 | poolMirror << 12; -1 for a missing side.
#[no_mangle]
pub extern "C" fn paph_screen42(cfg: *const i32, prof: *const Profile, a: *const Prepared, b: *const Prepared) -> i32 {
    if a.is_null() || b.is_null() {
        return -1;
    }
    let (a, b) = unsafe { (&*a, &*b) };
    let s = crate::v42::screen_v42_prepared(a, b, &config_from(cfg), profile_ref(prof));
    pack_screen(&s)
}

fn pack_screen(s: &crate::v42::Screen42) -> i32 {
    (s.pass as i32) | ((s.pool_direct.min(2047) as i32) << 1) | ((s.pool_mirror.min(2047) as i32) << 12)
}

/// Fields per rank record.
pub const RANK_FIELDS: usize = 16;

/// Verdict states as integers, ordered by how much they claim.
pub fn state_code(s: &str) -> i32 {
    match s {
        "Unrelated" => 0,
        "Related" => 1,
        "Suspected" => 2,
        "Copy" => 3,
        "Identical" => 4,
        _ => 5, // Indeterminate
    }
}

/// One candidate's record:
///
/// ```text
///  0 state        -1 unscreened, 0 Unrelated, 1 Related, 2 Suspected,
///                  3 Copy, 4 Identical, 5 Indeterminate
///  1 certifiable   2 structural        3 geometryEvidence  4 totalInliers
///  5 topology      6 geoMargin         7 local evidence    8 local matches
///  9 diversity     10 multiplier       11 models           12 poolDirect
///  13 poolMirror   14 swapped          15 flags: 1 screen pass, 2 mixed
///                                         selection, 4 a mirrored model
/// ```
fn rank_record(out: &mut [i32], r: Option<&crate::v42::V42Report>, s: &crate::v42::Screen42, swapped: bool) {
    for v in out.iter_mut() {
        *v = 0;
    }
    out[12] = s.pool_direct as i32;
    out[13] = s.pool_mirror as i32;
    out[14] = swapped as i32;
    out[15] = s.pass as i32;
    let Some(r) = r else {
        out[0] = -1;
        return;
    };
    let b = &r.base;
    out[0] = state_code(b.verdict);
    out[1] = b.certifiable as i32;
    out[2] = b.structural as i32;
    out[3] = b.geometry_evidence as i32;
    out[4] = b.total_inliers as i32;
    out[5] = b.topology as i32;
    out[6] = b.geo_margin as i32;
    if let Some(l) = &b.local {
        out[7] = if l.measurable { l.evidence as i32 } else { -1 };
        out[8] = l.matches as i32;
    }
    out[9] = r.diversity_geo.combined as i32;
    out[10] = r.diversity_geo.multiplier as i32;
    out[11] = r.models42.len() as i32;
    out[15] |= ((r.mixed_selection() as i32) << 1) | ((r.models42.iter().any(|m| m.mirror) as i32) << 2);
}

/// Screen and compare one query against many candidates: one call across
/// the boundary, the query parsed once, and each pair's descriptor scans
/// shared between its screen and its comparison.  Lean readings throughout.
#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub extern "C" fn paph_rank42(
    cfg: *const i32,
    prof: *const Profile,
    query: *const Prepared,
    cands: *const u32,
    n: u32,
    flags: u32,
    out: *mut i32,
) -> u32 {
    if query.is_null() || (cands.is_null() && n > 0) || out.is_null() {
        return 0;
    }
    let p = profile_ref(prof);
    let q = unsafe { &*query };
    let handles = unsafe { std::slice::from_raw_parts(cands, n as usize) };
    let out = unsafe { std::slice::from_raw_parts_mut(out, n as usize * RANK_FIELDS) };
    let cfg = bind(&config_from(cfg), p);
    let ok_profile = usable(p);
    for (k, &h) in handles.iter().enumerate() {
        let rec = &mut out[k * RANK_FIELDS..(k + 1) * RANK_FIELDS];
        let cand = h as usize as *const Prepared;
        if cand.is_null() {
            let r = crate::v42::refuse_report(R_CORRUPT, p);
            rank_record(rec, Some(&r), &crate::v42::Screen42 { pass: false, pool_direct: 0, pool_mirror: 0 }, false);
            continue;
        }
        let c = unsafe { &*cand };
        if c.t1.version != q.t1.version {
            // a candidate of the other wire format: refused, not screened
            let r = crate::v42::refuse_report(crate::wire::R_WIRE_MISMATCH, p);
            rank_record(rec, Some(&r), &crate::v42::Screen42 { pass: false, pool_direct: 0, pool_mirror: 0 }, false);
            continue;
        }
        let swapped = canon_swapped(q, c);
        let (ca, cb) = if swapped { (c, q) } else { (q, c) };
        let mut ctx = PairCtx::new(ca, cb);
        let s = screen_in(&mut ctx, &cfg, p);
        if flags & 1 != 0 && !s.pass {
            rank_record(rec, None, &s, swapped);
            continue;
        }
        let r = if !ok_profile {
            crate::v42::refuse_report(R_PROFILE_UNSUPPORTED, p)
        } else if q.t2_error.is_some() || c.t2_error.is_some() {
            crate::v42::refuse_report(R_CORRUPT, p)
        } else {
            compare_in(&mut ctx, swapped, &cfg, p, Reading::Lean)
        };
        rank_record(rec, Some(&r), &s, swapped);
    }
    n
}

// ------------------------------------------------------------------ testing

/// The equivalence digest, computed inside this build (feature `equiv`, test
/// builds only).  Equal to the native digest exactly when this build — its
/// vector paths included — computes what the native one does.
#[cfg(feature = "equiv")]
#[no_mangle]
pub extern "C" fn paph_equiv_digest() -> *mut u8 {
    out_block(crate::equiv::digest().as_bytes())
}

/// The wire-4 equivalence digest (test builds only), as `paph_equiv_digest`.
#[cfg(feature = "equiv")]
#[no_mangle]
pub extern "C" fn paph_equiv_digest4() -> *mut u8 {
    out_block(crate::equiv::digest_wire4().as_bytes())
}

/// Test builds only: `reps` descriptor scans of a prepared pair (direct
/// hypothesis), for timing the kernel inside the engine that runs it.
#[cfg(feature = "equiv")]
#[no_mangle]
pub extern "C" fn paph_bench_scan(a: *const Prepared, b: *const Prepared, reps: u32) -> u32 {
    let (a, b) = unsafe { (&*a, &*b) };
    let mut acc = 0u32;
    for _ in 0..reps {
        let s = crate::prepared::match_state(&a.desc, &b.desc);
        acc = acc.wrapping_add(s.a_d1.iter().map(|&v| v as u32).sum::<u32>());
    }
    acc
}

/// Test builds only: `reps` Hamming rows (no scan), kernel alone.
#[cfg(feature = "equiv")]
#[no_mangle]
pub extern "C" fn paph_bench_ham(a: *const Prepared, b: *const Prepared, reps: u32) -> u32 {
    let (a, b) = unsafe { (&*a, &*b) };
    let mut row = vec![0u16; b.desc.len()];
    let mut acc = 0u32;
    for _ in 0..reps {
        for q in a.desc.iter() {
            crate::simd::hamming_row(q, &b.desc, &mut row);
            acc = acc.wrapping_add(row[0] as u32);
        }
    }
    acc
}
