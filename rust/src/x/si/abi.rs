//! The PAPH-SI C ABI (SI ABI 1) — exports added beside ABI 3 and X ABI 1;
//! nothing of either moves.
//!
//! ```text
//!   paph_siabi() -> 1            paph_sisig_bytes() -> 104
//!   paph_siprofile(psi, n) -> handle | 0
//!       an SI artefact, or null for the shipped SI3-PROVISIONAL (bound to
//!       X2, fitted on the chain's artworks; SI2, the synthetic fit, and
//!       1.1.0's SI1 are artefact files); 0 when the artefact does not decode
//!   paph_siprofile_free(h)
//!   paph_siprofile_bytes(h) -> block           the artefact's bytes
//!   paph_siprofile_id(h, out_32)               SHA-256 identity
//!   paph_siprofile_xid(h, out_32)              identity of the X profile it is bound to
//!   paph_siprofile_info(h, out_i32x4)          probes, threshold, budget, features version
//!   paph_sisig(sp, side, out_104) -> 0 | -1 (null) | -2 (side prepared under another X profile)
//!       the signature of an X side (its route is reused)
//!   paph_sisig_wire(sp, xb, t1, t1n, t2, t2n, out_104) -> 0 | -1 | -2 | -3 (tier 1 refused)
//!       the signature straight from wires: no bucket index, no anchor order
//!   paph_sikeys(sig_104, out_i32x54) -> n      posting keys of a signature (SQL rows)
//!   paph_siquery(sp, side) -> handle | 0       0: null, or side under another X profile
//!   paph_siquery_free(h)
//!   paph_siquery_sig(q, out_104)
//!   paph_siquery_score(q, sig_104) -> score    i32::MIN when no family reaches it
//!   paph_siquery_plan(sp, q) -> block          utf-8 JSON: the SQL plan (SPEC-SI §7.2)
//!   paph_siindex_new() -> handle               an in-memory index (SPEC-SI §7.1)
//!   paph_siindex_free(h)
//!   paph_siindex_add(h, sig_104) -> slot       slots are dense and never reused
//!   paph_siindex_remove(h, slot) -> 1 | 0
//!   paph_siindex_len(h) -> live slots          paph_siindex_generation(h) -> mutation count (low 32 bits)
//!   paph_siindex_query(h, q, threshold, budget, out_i32, stats_i32x3) -> n
//!       out: n pairs (slot, score), best first; stats: touched, admitted, postings read
//! ```

use super::code::{SiQuery, SiSig, SIG_BYTES};
use super::features::FAMILIES;
use super::index::SiIndex;
use super::profile::*;
use crate::abi::out_block;
use crate::json::J;
use crate::prepared::Prepared;
use crate::x::prepared::XPrepared;
use crate::x::XBound;

pub const SI_ABI_VERSION: u32 = 1;
/// Posting keys a signature can hold: one per quantised family, one per band.
pub const MAX_KEYS: usize = FAMILIES + LOCAL_BANDS + BAND_BANDS;

#[no_mangle]
pub extern "C" fn paph_siabi() -> u32 {
    SI_ABI_VERSION
}

#[no_mangle]
pub extern "C" fn paph_sisig_bytes() -> u32 {
    SIG_BYTES as u32
}

/// A decoded profile and its identity.
pub struct SiBound {
    pub prof: SiProfile,
    pub id: [u8; 32],
}

#[no_mangle]
pub extern "C" fn paph_siprofile(psi: *const u8, n: usize) -> *mut SiBound {
    let prof = if psi.is_null() || n == 0 {
        SiProfile::shipped()
    } else {
        match SiProfile::decode(unsafe { std::slice::from_raw_parts(psi, n) }) {
            Ok(p) => p,
            Err(_) => return std::ptr::null_mut(),
        }
    };
    let id = prof.id();
    Box::into_raw(Box::new(SiBound { prof, id }))
}

#[no_mangle]
pub extern "C" fn paph_siprofile_free(h: *mut SiBound) {
    if !h.is_null() {
        unsafe { drop(Box::from_raw(h)) }
    }
}

#[no_mangle]
pub extern "C" fn paph_siprofile_bytes(h: *const SiBound) -> *mut u8 {
    if h.is_null() {
        return out_block(&[]);
    }
    out_block(&unsafe { &*h }.prof.encode())
}

#[no_mangle]
pub extern "C" fn paph_siprofile_id(h: *const SiBound, out: *mut u8) {
    if h.is_null() || out.is_null() {
        return;
    }
    unsafe { std::slice::from_raw_parts_mut(out, 32) }.copy_from_slice(&unsafe { &*h }.id);
}

#[no_mangle]
pub extern "C" fn paph_siprofile_xid(h: *const SiBound, out: *mut u8) {
    if h.is_null() || out.is_null() {
        return;
    }
    unsafe { std::slice::from_raw_parts_mut(out, 32) }.copy_from_slice(&unsafe { &*h }.prof.xid);
}

#[no_mangle]
pub extern "C" fn paph_siprofile_info(h: *const SiBound, out: *mut i32) {
    if h.is_null() || out.is_null() {
        return;
    }
    let p = &unsafe { &*h }.prof;
    let o = unsafe { std::slice::from_raw_parts_mut(out, 4) };
    o[0] = p.probes as i32;
    o[1] = p.threshold;
    o[2] = p.budget;
    o[3] = p.features as i32;
}

fn write_sig(s: &SiSig, out: *mut u8) {
    unsafe { std::slice::from_raw_parts_mut(out, SIG_BYTES) }.copy_from_slice(&s.to_bytes());
}

fn read_sig(p: *const u8) -> SiSig {
    SiSig::from_bytes(unsafe { std::slice::from_raw_parts(p, SIG_BYTES) }).unwrap_or_default()
}

#[no_mangle]
pub extern "C" fn paph_sisig(sp: *const SiBound, side: *const XPrepared, out: *mut u8) -> i32 {
    if sp.is_null() || side.is_null() || out.is_null() {
        return -1;
    }
    let (sb, x) = unsafe { (&*sp, &*side) };
    if x.xid != sb.prof.xid {
        return -2;
    }
    write_sig(&SiSig::build(&x.p, &x.route, &sb.prof), out);
    0
}

#[allow(clippy::too_many_arguments)]
#[no_mangle]
pub extern "C" fn paph_sisig_wire(sp: *const SiBound, xb: *const XBound, t1: *const u8, t1n: usize, t2: *const u8, t2n: usize, out: *mut u8) -> i32 {
    if sp.is_null() || xb.is_null() || t1.is_null() || out.is_null() {
        return -1;
    }
    let (sb, xb) = unsafe { (&*sp, &*xb) };
    if xb.xid != sb.prof.xid {
        return -2;
    }
    let t1 = unsafe { std::slice::from_raw_parts(t1, t1n) };
    let t2 = if t2.is_null() || t2n == 0 { None } else { Some(unsafe { std::slice::from_raw_parts(t2, t2n) }) };
    match Prepared::new(t1, t2) {
        Ok(p) => {
            write_sig(&SiSig::from_prepared(&p, xb, &sb.prof), out);
            0
        }
        Err(_) => -3,
    }
}

/// The posting keys of a signature, all below 2^27: quantised family f in
/// cell c is `f·2^24 + c`; local band j holding value v is `6·2^24 + j·2^16 + v`;
/// descriptor band j, `7·2^24 + j·2^16 + v`.
pub fn sig_keys(s: &SiSig, out: &mut [i32; MAX_KEYS]) -> usize {
    let mut n = 0;
    for f in 0..FAMILIES {
        if s.present >> f & 1 != 0 {
            out[n] = ((f as i32) << 24) | s.cells[f] as i32;
            n += 1;
        }
    }
    if s.present & super::code::P_LOCAL != 0 {
        for j in 0..LOCAL_BANDS {
            out[n] = (6 << 24) | ((j as i32) << 16) | s.local[j] as i32;
            n += 1;
        }
    }
    if s.present & super::code::P_BAND != 0 {
        for j in 0..BAND_BANDS {
            out[n] = (7 << 24) | ((j as i32) << 16) | s.band[j] as i32;
            n += 1;
        }
    }
    n
}

#[no_mangle]
pub extern "C" fn paph_sikeys(sig: *const u8, out: *mut i32) -> u32 {
    if sig.is_null() || out.is_null() {
        return 0;
    }
    let mut k = [0i32; MAX_KEYS];
    let n = sig_keys(&read_sig(sig), &mut k);
    unsafe { std::slice::from_raw_parts_mut(out, n) }.copy_from_slice(&k[..n]);
    n as u32
}

#[no_mangle]
pub extern "C" fn paph_siquery(sp: *const SiBound, side: *const XPrepared) -> *mut SiQuery {
    if sp.is_null() || side.is_null() {
        return std::ptr::null_mut();
    }
    let (sb, x) = unsafe { (&*sp, &*side) };
    if x.xid != sb.prof.xid {
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(SiQuery::new(&x.p, &x.route, &sb.prof)))
}

#[no_mangle]
pub extern "C" fn paph_siquery_free(h: *mut SiQuery) {
    if !h.is_null() {
        unsafe { drop(Box::from_raw(h)) }
    }
}

#[no_mangle]
pub extern "C" fn paph_siquery_sig(q: *const SiQuery, out: *mut u8) {
    if q.is_null() || out.is_null() {
        return;
    }
    write_sig(&unsafe { &*q }.sig, out);
}

#[no_mangle]
pub extern "C" fn paph_siquery_score(q: *const SiQuery, sig: *const u8) -> i32 {
    if q.is_null() || sig.is_null() {
        return i32::MIN;
    }
    let (q, s) = (unsafe { &*q }, read_sig(sig));
    if q.touches(&s) {
        q.score(&s)
    } else {
        i32::MIN
    }
}

/// The SQL plan of a query (SPEC-SI §7.2): the posting keys to look up with
/// the score each contributes, and the terms the statement adds per
/// candidate.  Keys and weights only — the statement is the same for every
/// query.
pub fn plan_json(sb: &SiBound, q: &SiQuery) -> J {
    let mut probes = Vec::new();
    for f in 0..FAMILIES {
        if q.sig.present >> f & 1 == 0 {
            continue;
        }
        for r in 0..q.nprobe[f] as usize {
            let lvl = if r == 0 { 2 } else { 1 };
            probes.push(J::Arr(vec![
                J::Int((((f as i64) << 24) | q.probes[f][r] as i64) as i64),
                J::Int(f as i64),
                J::Int((q.wv[f][lvl] - q.wv[f][0]) as i64),
            ]));
        }
    }
    if q.sig.present & super::code::P_LOCAL != 0 {
        for j in 0..LOCAL_BANDS {
            probes.push(J::Arr(vec![J::Int((6i64 << 24) | ((j as i64) << 16) | q.sig.local[j] as i64), J::Int(6), J::Int(0)]));
        }
    }
    if q.sig.present & super::code::P_BAND != 0 {
        for j in 0..BAND_BANDS {
            probes.push(J::Arr(vec![J::Int((7i64 << 24) | ((j as i64) << 16) | q.sig.band[j] as i64), J::Int(7), J::Int(0)]));
        }
    }
    let excess = |w: &[i32; MLEVELS], on: bool| -> J {
        J::Arr((1..MLEVELS).map(|l| J::Int(if on { (w[l] - w[0]) as i64 } else { 0 })).collect())
    };
    let mut base = Vec::new();
    for f in 0..FAMILIES {
        base.push(J::Int(if q.sig.present >> f & 1 != 0 { q.wv[f][0] as i64 } else { 0 }));
    }
    base.push(J::Int(if q.sig.present & super::code::P_LOCAL != 0 { q.wl[0] as i64 } else { 0 }));
    base.push(J::Int(if q.sig.present & super::code::P_BAND != 0 { q.wb[0] as i64 } else { 0 }));
    J::Obj(vec![
        ("version", J::Int(SI_ABI_VERSION as i64)),
        ("profile", J::Str(crate::sha256::hex(&sb.id[..8]))),
        ("threshold", J::Int(sb.prof.threshold as i64)),
        ("budget", J::Int(sb.prof.budget as i64)),
        ("probes", J::Arr(probes)),
        ("local", excess(&q.wl, q.sig.present & super::code::P_LOCAL != 0)),
        ("band", excess(&q.wb, q.sig.present & super::code::P_BAND != 0)),
        ("base", J::Arr(base)),
    ])
}

#[no_mangle]
pub extern "C" fn paph_siquery_plan(sp: *const SiBound, q: *const SiQuery) -> *mut u8 {
    if sp.is_null() || q.is_null() {
        return out_block(b"{\"error\":\"siquery_plan needs a profile and a query\"}");
    }
    out_block(plan_json(unsafe { &*sp }, unsafe { &*q }).to_string().as_bytes())
}

#[no_mangle]
pub extern "C" fn paph_siindex_new() -> *mut SiIndex {
    Box::into_raw(Box::new(SiIndex::new()))
}

#[no_mangle]
pub extern "C" fn paph_siindex_free(h: *mut SiIndex) {
    if !h.is_null() {
        unsafe { drop(Box::from_raw(h)) }
    }
}

#[no_mangle]
pub extern "C" fn paph_siindex_add(h: *mut SiIndex, sig: *const u8) -> i32 {
    if h.is_null() || sig.is_null() {
        return -1;
    }
    unsafe { &mut *h }.add(read_sig(sig)) as i32
}

#[no_mangle]
pub extern "C" fn paph_siindex_remove(h: *mut SiIndex, slot: u32) -> i32 {
    if h.is_null() {
        return 0;
    }
    unsafe { &mut *h }.remove(slot) as i32
}

#[no_mangle]
pub extern "C" fn paph_siindex_len(h: *const SiIndex) -> u32 {
    if h.is_null() {
        return 0;
    }
    unsafe { &*h }.len() as u32
}

#[no_mangle]
pub extern "C" fn paph_siindex_generation(h: *const SiIndex) -> u32 {
    if h.is_null() {
        return 0;
    }
    unsafe { &*h }.generation() as u32
}

#[no_mangle]
pub extern "C" fn paph_siindex_query(h: *mut SiIndex, q: *const SiQuery, threshold: i32, budget: u32, out: *mut i32, stats: *mut i32) -> u32 {
    if h.is_null() || q.is_null() || (out.is_null() && budget > 0) {
        return 0;
    }
    let (idx, q) = (unsafe { &mut *h }, unsafe { &*q });
    let mut v = Vec::new();
    let st = idx.query(q, threshold, budget as usize, &mut v);
    if !out.is_null() {
        let o = unsafe { std::slice::from_raw_parts_mut(out, 2 * v.len()) };
        for (k, &(slot, score)) in v.iter().enumerate() {
            o[2 * k] = slot as i32;
            o[2 * k + 1] = score;
        }
    }
    if !stats.is_null() {
        let s = unsafe { std::slice::from_raw_parts_mut(stats, 3) };
        s[0] = st.touched.min(i32::MAX as usize) as i32;
        s[1] = st.admitted.min(i32::MAX as usize) as i32;
        s[2] = st.postings.min(i32::MAX as usize) as i32;
    }
    v.len() as u32
}
