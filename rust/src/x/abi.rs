//! The PAPH-X C ABI (specification §30) — ABI 3 adds these beside ABI 2's
//! exports; nothing of ABI 2 moves.
//!
//! ```text
//!   paph_xabi() -> 1
//!   paph_xprofile(prof, pxcl, n) -> handle | 0
//!       `prof`: a comparator-42 profile handle (0 = the shipped
//!       CAL-004-PROPOSED); `pxcl`: an X profile artefact (version 1, X1,
//!       or version 2, X2 onwards), or null for the shipped X2-PROVISIONAL
//!       bound to that profile.  0 when the artefact does not decode.
//!       Check it.
//!   paph_xprofile_free(h)
//!   paph_xprofile_bytes(h) -> block          the X artefact's bytes
//!   paph_xprofile_id(h, out_32)              SHA-256 identity
//!   paph_xprofile_status(h) -> 0 ok, 1 unsupported, 2 base mismatch
//!   paph_xprepare(xb, t1, t1n, t2, t2n, flags) -> handle | 0
//!       flags bit 0: strict (as paph_prepare)
//!   paph_xprepare_sidecar(xb, t1, t1n, t2, t2n, side, siden, flags) -> handle | 0
//!       as above, structures taken from a PAX1 sidecar when it is usable
//!   paph_xprepare_free(h)
//!   paph_xprepared_info(h, out_i32x4)        as paph_prepared_info
//!   paph_xroute(h, out_u8x136)               the route record
//!   paph_xsidecar(h) -> block                the PAX1 sidecar
//!   paph_xscreen(cfg, xb, a, b, out_i32) -> state   XSCREEN_FIELDS fields
//!   paph_xcompare(cfg, xb, a, b, flags) -> block (utf-8 JSON report)
//!       flags bits 0-1: policy (0 the profile's, 1 fast, 2 safe, 3 exact);
//!       bit 2: audit (EXACT42 beside the fast path, attached);
//!       bit 3: Copy scope
//!   paph_xrank(cfg, xb, query, cands, n, flags, out) -> n
//!       `cands`: n u32 handles; `out`: n records of XRANK_FIELDS i32;
//!       flags bits 0-1: policy; bit 2: no gate; bit 3: full scope
//!   paph_xrank_fields() -> 24,  paph_xscreen_fields() -> 12
//! ```

use super::compare::{report_json, xcompare, xscreen, Scope, XCtx, XOptions};
use super::prepared::XPrepared;
use super::profile::{XBound, XProfile, POLICY_EXACT, POLICY_FAST, POLICY_SAFE};
use super::rank::{xrank, RankScratch, XRankOptions, XRANK_FIELDS};
use super::route::ROUTE_RECORD_BYTES;
use crate::abi::{config_from, out_block, profile_ref};
use crate::calibration::Profile;
use crate::config::MAX_KP_COUNT;
use crate::prepared::Prepared;

pub const X_ABI_VERSION: u32 = 1;
pub const XSCREEN_FIELDS: usize = 12;

#[no_mangle]
pub extern "C" fn paph_xabi() -> u32 {
    X_ABI_VERSION
}

#[no_mangle]
pub extern "C" fn paph_xrank_fields() -> u32 {
    XRANK_FIELDS as u32
}

#[no_mangle]
pub extern "C" fn paph_xscreen_fields() -> u32 {
    XSCREEN_FIELDS as u32
}

// ---------------------------------------------------------------- profiles

#[no_mangle]
pub extern "C" fn paph_xprofile(prof: *const Profile, pxcl: *const u8, n: usize) -> *mut XBound {
    let base = profile_ref(prof).clone();
    let xp = if pxcl.is_null() || n == 0 {
        XProfile::shipped_for(&base)
    } else {
        match XProfile::decode(unsafe { std::slice::from_raw_parts(pxcl, n) }) {
            Ok(p) => p,
            Err(_) => return std::ptr::null_mut(),
        }
    };
    Box::into_raw(Box::new(XBound::new(base, xp)))
}

#[no_mangle]
pub extern "C" fn paph_xprofile_free(h: *mut XBound) {
    if !h.is_null() {
        unsafe { drop(Box::from_raw(h)) }
    }
}

#[no_mangle]
pub extern "C" fn paph_xprofile_bytes(h: *const XBound) -> *mut u8 {
    if h.is_null() {
        return out_block(&[]);
    }
    out_block(&unsafe { &*h }.xp.encode())
}

#[no_mangle]
pub extern "C" fn paph_xprofile_id(h: *const XBound, out: *mut u8) {
    if h.is_null() || out.is_null() {
        return;
    }
    let xb = unsafe { &*h };
    unsafe { std::slice::from_raw_parts_mut(out, 32) }.copy_from_slice(&xb.xid);
}

#[no_mangle]
pub extern "C" fn paph_xprofile_status(h: *const XBound) -> i32 {
    if h.is_null() {
        return 1;
    }
    match unsafe { &*h }.refused {
        None => 0,
        Some(crate::v4::R_PROFILE_MISMATCH) => 2,
        Some(_) => 1,
    }
}

// ---------------------------------------------------------------- sides

fn prepare_side(t1: *const u8, t1n: usize, t2: *const u8, t2n: usize, flags: u32) -> Option<Prepared> {
    if t1.is_null() {
        return None;
    }
    let t1 = unsafe { std::slice::from_raw_parts(t1, t1n) };
    let t2 = if t2.is_null() || t2n == 0 { None } else { Some(unsafe { std::slice::from_raw_parts(t2, t2n) }) };
    match Prepared::new(t1, t2) {
        Ok(mut p) => {
            if flags & 1 != 0 && p.t2_error.is_none() && t2.is_some() && p.kp.len() > MAX_KP_COUNT {
                p = Prepared::sketch_only(p.t1, Some("tier 2 exceeds 512 keypoints"));
            }
            Some(p)
        }
        Err(_) => None,
    }
}

#[no_mangle]
pub extern "C" fn paph_xprepare(xb: *const XBound, t1: *const u8, t1n: usize, t2: *const u8, t2n: usize, flags: u32) -> *mut XPrepared {
    if xb.is_null() {
        return std::ptr::null_mut();
    }
    match prepare_side(t1, t1n, t2, t2n, flags) {
        Some(p) => Box::into_raw(Box::new(XPrepared::new(p, unsafe { &*xb }))),
        None => std::ptr::null_mut(),
    }
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub extern "C" fn paph_xprepare_sidecar(
    xb: *const XBound,
    t1: *const u8,
    t1n: usize,
    t2: *const u8,
    t2n: usize,
    side: *const u8,
    siden: usize,
    flags: u32,
) -> *mut XPrepared {
    if xb.is_null() {
        return std::ptr::null_mut();
    }
    let xb = unsafe { &*xb };
    let Some(p) = prepare_side(t1, t1n, t2, t2n, flags) else { return std::ptr::null_mut() };
    if side.is_null() || siden == 0 {
        return Box::into_raw(Box::new(XPrepared::new(p, xb)));
    }
    let sc = unsafe { std::slice::from_raw_parts(side, siden) };
    let x = match super::sidecar::decode(p, xb, sc) {
        Ok(x) => x,
        Err(p) => XPrepared::new(p, xb),
    };
    Box::into_raw(Box::new(x))
}

#[no_mangle]
pub extern "C" fn paph_xprepare_free(h: *mut XPrepared) {
    if !h.is_null() {
        unsafe { drop(Box::from_raw(h)) }
    }
}

#[no_mangle]
pub extern "C" fn paph_xprepared_info(h: *const XPrepared, out: *mut i32) {
    if h.is_null() || out.is_null() {
        return;
    }
    let p = unsafe { &(*h).p };
    let o = unsafe { std::slice::from_raw_parts_mut(out, 4) };
    o[0] = p.kp.len() as i32;
    o[1] = p.t2_error.is_none() as i32;
    o[2] = p.t1.width as i32;
    o[3] = p.t1.height as i32;
}

#[no_mangle]
pub extern "C" fn paph_xroute(h: *const XPrepared, out: *mut u8) {
    if h.is_null() || out.is_null() {
        return;
    }
    let b = unsafe { &*h }.route.to_bytes();
    unsafe { std::slice::from_raw_parts_mut(out, ROUTE_RECORD_BYTES) }.copy_from_slice(&b);
}

#[no_mangle]
pub extern "C" fn paph_xsidecar(h: *const XPrepared) -> *mut u8 {
    if h.is_null() {
        return out_block(&[]);
    }
    out_block(&super::sidecar::encode(unsafe { &*h }))
}

// ---------------------------------------------------------------- comparing

thread_local! {
    static CTX: std::cell::RefCell<Option<(Box<XCtx>, RankScratch)>> = const { std::cell::RefCell::new(None) };
}

fn with_scratch<R>(f: impl FnOnce(&mut XCtx, &mut RankScratch) -> R) -> R {
    CTX.with(|c| {
        let mut b = c.borrow_mut();
        let (ctx, rs) = b.get_or_insert_with(|| (Box::new(XCtx::new()), RankScratch::new()));
        f(ctx, rs)
    })
}

fn policy_of(flags: u32) -> Option<u8> {
    match flags & 3 {
        1 => Some(POLICY_FAST),
        2 => Some(POLICY_SAFE),
        3 => Some(POLICY_EXACT),
        _ => None,
    }
}

/// The screen record:
///
/// ```text
///  0 state: 0 reject, 1 defer, 2 pass, 3 identical, -1 refused
///  1 route local   2 route band   3 route global   4 route class (0 reject, 1 defer, 2 fast, 3 absent)
///  5 pool direct   6 pool mirror  7 support direct 8 support mirror
///  9 rows scanned  10 hammings    11 swapped
/// ```
#[no_mangle]
pub extern "C" fn paph_xscreen(cfg: *const i32, xb: *const XBound, a: *const XPrepared, b: *const XPrepared, out: *mut i32) -> i32 {
    if xb.is_null() || a.is_null() || b.is_null() {
        return -1;
    }
    let (xb, a, b) = unsafe { (&*xb, &*a, &*b) };
    let c = config_from(cfg);
    let s = with_scratch(|ctx, _| xscreen(a, b, &c, xb, ctx));
    if !out.is_null() {
        let o = unsafe { std::slice::from_raw_parts_mut(out, XSCREEN_FIELDS) };
        o[0] = s.state.code();
        o[1] = s.route.local;
        o[2] = s.route.band;
        o[3] = s.route.global;
        o[4] = match s.route_class {
            super::route::RouteClass::Reject => 0,
            super::route::RouteClass::Defer => 1,
            super::route::RouteClass::Fast => 2,
            super::route::RouteClass::Absent => 3,
        };
        o[5] = s.direct.count as i32;
        o[6] = s.mirror.count as i32;
        o[7] = s.direct.support.min(i32::MAX as i64) as i32;
        o[8] = s.mirror.support.min(i32::MAX as i64) as i32;
        o[9] = s.stats.rows as i32;
        o[10] = s.stats.hammings.min(i32::MAX as u64) as i32;
        o[11] = s.swapped as i32;
    }
    s.state.code()
}

#[no_mangle]
pub extern "C" fn paph_xcompare(cfg: *const i32, xb: *const XBound, a: *const XPrepared, b: *const XPrepared, flags: u32) -> *mut u8 {
    if xb.is_null() || a.is_null() || b.is_null() {
        return out_block(b"{\"error\":\"xcompare needs a profile and two sides\"}");
    }
    let (xb, a, b) = unsafe { (&*xb, &*a, &*b) };
    let c = config_from(cfg);
    let opts = XOptions { policy: policy_of(flags), audit: flags & 4 != 0, scope: if flags & 8 != 0 { Scope::Copy } else { Scope::Full } };
    let r = with_scratch(|ctx, _| xcompare(a, b, &c, xb, &opts, ctx));
    out_block(report_json(&r).to_string().as_bytes())
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub extern "C" fn paph_xrank(
    cfg: *const i32,
    xb: *const XBound,
    query: *const XPrepared,
    cands: *const u32,
    n: u32,
    flags: u32,
    out: *mut i32,
) -> u32 {
    if xb.is_null() || query.is_null() || (cands.is_null() && n > 0) || out.is_null() {
        return 0;
    }
    let (xb, q) = unsafe { (&*xb, &*query) };
    let c = config_from(cfg);
    let handles = unsafe { std::slice::from_raw_parts(cands, n as usize) };
    let refs: Vec<Option<&XPrepared>> = handles
        .iter()
        .map(|&h| {
            let p = h as usize as *const XPrepared;
            if p.is_null() {
                None
            } else {
                Some(unsafe { &*p })
            }
        })
        .collect();
    let out = unsafe { std::slice::from_raw_parts_mut(out, n as usize * XRANK_FIELDS) };
    let opts = XRankOptions { policy: policy_of(flags), gate: flags & 4 == 0, scope: if flags & 8 != 0 { Scope::Full } else { Scope::Copy } };
    with_scratch(|ctx, rs| xrank(q, &refs, &c, xb, &opts, ctx, rs, out)) as u32
}
