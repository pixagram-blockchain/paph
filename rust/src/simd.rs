//! Vector kernels.
//!
//! Every function here has a scalar twin, and a test holds the two equal on
//! every input class that matters — so a vector path can only ever change
//! speed.  Three builds: WebAssembly SIMD128 (the shipped binary), x86-64 SSE2
//! (the architectural baseline, so no runtime detection is needed), and plain
//! scalar for everything else.
//!
//! All arithmetic is exact integer arithmetic on the same bits the scalar path
//! reads.  Nothing here rounds, saturates in a way the scalar path does not, or
//! depends on lane order beyond what the bit layout below states.

/// The 64 cells of one local window, row-major, as `u16`.  Every value is in
/// `0..1024`: doubled quantiles 2..514, the transparent 257, or the 0..255
/// occupancy of the silhouette.
pub type Cells = [u16; 64];

/// `(s31, r)`: the lower median — the value at sorted index 31 — and the mask
/// with bit `i` set exactly when `cells[i] > s31`.
///
/// `canonical64` needs nothing else: the specified threshold `2v > s31 + s32`
/// is `v > s31` (see there).
#[inline]
pub fn median_mask(c: &Cells) -> (u16, u64) {
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    {
        return unsafe { wasm::median_mask(c) };
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
    {
        return unsafe { x86::median_mask(c) };
    }
    #[allow(unreachable_code)]
    median_mask_scalar(c)
}

/// Scalar reference: a two-level radix select (32 coarse buckets of 32), then
/// a compare per cell.
pub fn median_mask_scalar(c: &Cells) -> (u16, u64) {
    let mut hi = [0u8; 32];
    for &v in c.iter() {
        hi[((v >> 5) & 31) as usize] += 1;
    }
    let mut want = 31u8;
    let mut b = 0usize;
    while want >= hi[b] {
        want -= hi[b];
        b += 1;
    }
    let mut lo = [0u8; 32];
    for &v in c.iter() {
        lo[(v & 31) as usize] += (((v >> 5) & 31) as usize == b) as u8;
    }
    let mut j = 0usize;
    while want >= lo[j] {
        want -= lo[j];
        j += 1;
    }
    let s31 = ((b << 5) | j) as u16;
    let mut r = 0u64;
    for (i, &v) in c.iter().enumerate() {
        r |= ((v > s31) as u64) << i;
    }
    (s31, r)
}

// The vector paths find s31 as the largest t in 0..1024 with #{v < t} <= 31:
// #{v < t} is non-decreasing in t, it is <= 31 at t = s31 (only values at
// sorted positions below 31 can be strictly smaller) and >= 32 for every
// t > s31.  Ten branch-free halvings, each one compare and one subtract per
// row of eight, and no histogram — so no store-to-load chains on the repeated
// values dithered art is made of.

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod wasm {
    use core::arch::wasm32::*;

    #[inline(always)]
    unsafe fn rows(c: &super::Cells) -> [v128; 8] {
        let p = c.as_ptr() as *const v128;
        [
            v128_load(p),
            v128_load(p.add(1)),
            v128_load(p.add(2)),
            v128_load(p.add(3)),
            v128_load(p.add(4)),
            v128_load(p.add(5)),
            v128_load(p.add(6)),
            v128_load(p.add(7)),
        ]
    }

    #[inline(always)]
    unsafe fn count_below(r: &[v128; 8], t: u16) -> u32 {
        let tv = u16x8_splat(t);
        // a lane mask is 0 or -1; subtracting it counts
        let mut acc = i16x8_sub(i16x8_splat(0), u16x8_lt(r[0], tv));
        acc = i16x8_sub(acc, u16x8_lt(r[1], tv));
        acc = i16x8_sub(acc, u16x8_lt(r[2], tv));
        acc = i16x8_sub(acc, u16x8_lt(r[3], tv));
        acc = i16x8_sub(acc, u16x8_lt(r[4], tv));
        acc = i16x8_sub(acc, u16x8_lt(r[5], tv));
        acc = i16x8_sub(acc, u16x8_lt(r[6], tv));
        acc = i16x8_sub(acc, u16x8_lt(r[7], tv));
        let s = i32x4_extadd_pairwise_i16x8(acc);
        let s = i32x4_add(s, i32x4_shuffle::<2, 3, 0, 1>(s, s));
        let s = i32x4_add(s, i32x4_shuffle::<1, 0, 3, 2>(s, s));
        i32x4_extract_lane::<0>(s) as u32
    }

    #[inline(always)]
    pub unsafe fn median_mask(c: &super::Cells) -> (u16, u64) {
        let r = rows(c);
        let mut t = 0u16;
        let mut bit = 512u16;
        while bit != 0 {
            let cand = t | bit;
            if count_below(&r, cand) <= 31 {
                t = cand;
            }
            bit >>= 1;
        }
        let tv = u16x8_splat(t);
        let mut out = 0u64;
        for k in 0..4 {
            let a = u16x8_gt(r[2 * k], tv);
            let b = u16x8_gt(r[2 * k + 1], tv);
            // 0xFFFF narrows to 0xFF and 0 to 0; lane j's sign is bit j
            let m = i8x16_bitmask(i8x16_narrow_i16x8(a, b)) as u64;
            out |= m << (16 * k);
        }
        (t, out)
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
mod x86 {
    use core::arch::x86_64::*;

    #[inline(always)]
    unsafe fn rows(c: &super::Cells) -> [__m128i; 8] {
        let p = c.as_ptr() as *const __m128i;
        [
            _mm_loadu_si128(p),
            _mm_loadu_si128(p.add(1)),
            _mm_loadu_si128(p.add(2)),
            _mm_loadu_si128(p.add(3)),
            _mm_loadu_si128(p.add(4)),
            _mm_loadu_si128(p.add(5)),
            _mm_loadu_si128(p.add(6)),
            _mm_loadu_si128(p.add(7)),
        ]
    }

    // Every value is below 1024, so the signed 16-bit compares are exact.
    #[inline(always)]
    unsafe fn count_below(r: &[__m128i; 8], t: u16) -> u32 {
        let tv = _mm_set1_epi16(t as i16);
        let mut acc = _mm_sub_epi16(_mm_setzero_si128(), _mm_cmplt_epi16(r[0], tv));
        acc = _mm_sub_epi16(acc, _mm_cmplt_epi16(r[1], tv));
        acc = _mm_sub_epi16(acc, _mm_cmplt_epi16(r[2], tv));
        acc = _mm_sub_epi16(acc, _mm_cmplt_epi16(r[3], tv));
        acc = _mm_sub_epi16(acc, _mm_cmplt_epi16(r[4], tv));
        acc = _mm_sub_epi16(acc, _mm_cmplt_epi16(r[5], tv));
        acc = _mm_sub_epi16(acc, _mm_cmplt_epi16(r[6], tv));
        acc = _mm_sub_epi16(acc, _mm_cmplt_epi16(r[7], tv));
        let s = _mm_madd_epi16(acc, _mm_set1_epi16(1));
        let s = _mm_add_epi32(s, _mm_shuffle_epi32::<0b01_00_11_10>(s));
        let s = _mm_add_epi32(s, _mm_shuffle_epi32::<0b10_11_00_01>(s));
        _mm_cvtsi128_si32(s) as u32
    }

    #[inline(always)]
    pub unsafe fn median_mask(c: &super::Cells) -> (u16, u64) {
        let r = rows(c);
        let mut t = 0u16;
        let mut bit = 512u16;
        while bit != 0 {
            let cand = t | bit;
            if count_below(&r, cand) <= 31 {
                t = cand;
            }
            bit >>= 1;
        }
        let tv = _mm_set1_epi16(t as i16);
        let mut out = 0u64;
        for k in 0..4 {
            let a = _mm_cmpgt_epi16(r[2 * k], tv);
            let b = _mm_cmpgt_epi16(r[2 * k + 1], tv);
            let m = _mm_movemask_epi8(_mm_packs_epi16(a, b)) as u32 as u64;
            out |= m << (16 * k);
        }
        (t, out)
    }
}

// ------------------------------------------------------------------ FAST-9

/// FAST threshold, duplicated from `keypoints::FAST_T` so this module stays
/// leaf-level; `keypoints` asserts the two agree.
pub const FAST_T: u8 = 18;

/// The circular nine-run test, on a 16-bit circle mask: true when some nine
/// consecutive positions (modulo 16) are all set.
///
/// The reference walks 24 positions, `k & 15`, and counts a run — which sees
/// exactly the circular runs, because any run of nine starting in 0..16 ends
/// before position 24.  Doubling the mask makes circular runs linear, and four
/// AND-shifts find a run of 2, 4, 8 and then 9.
#[inline(always)]
pub fn arc9(mask: u16) -> bool {
    let m = (mask as u32) | ((mask as u32) << 16);
    let a = m & (m >> 1);
    let b = a & (a >> 2);
    let c = b & (b >> 4);
    (c & (m >> 8)) != 0
}

/// FAST-9 on the sixteen consecutive pixels starting at index `p` of a level
/// whose bytes are `d`.  `circ[k]` is the k-th circle point as an index offset
/// (`dy * w + dx`).  Bit `j` of the result is set when pixel `p + j` passes the
/// four-point pre-test (three of points 0, 4, 8, 12 brighter than `c + 18`, or
/// three darker than `c - 18`) AND carries a run of nine brighter or nine
/// darker points.  The caller applies the opacity floor and the score, which
/// do not depend on the lane.
///
/// Saturating `c + 18` and `c - 18` are exact here: when `c + 18` exceeds 255
/// nothing can be brighter in either formulation, and when `c < 18` nothing
/// can be darker.
///
/// # Safety
/// Every index `p + j + circ[k]`, `j < 16`, must be inside `d`.
#[inline]
pub unsafe fn fast9_x16(d: &[u8], p: usize, circ: &[isize; 16]) -> u16 {
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    {
        return wasm_fast::fast9_x16(d, p, circ);
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
    {
        return x86_fast::fast9_x16(d, p, circ);
    }
    #[allow(unreachable_code)]
    fast9_x16_scalar(d, p, circ)
}

/// The per-pixel reference the vector paths are held to.
///
/// # Safety
/// As `fast9_x16`.
pub unsafe fn fast9_x16_scalar(d: &[u8], p: usize, circ: &[isize; 16]) -> u16 {
    let mut out = 0u16;
    for j in 0..16usize {
        let c = d[p + j] as i32;
        let hi = c + FAST_T as i32;
        let lo = c - FAST_T as i32;
        let at = |k: usize| d[((p + j) as isize + circ[k]) as usize] as i32;
        let (mut b, mut k4) = (0, 0);
        for q in [0usize, 4, 8, 12] {
            let v = at(q);
            if v > hi {
                b += 1;
            } else if v < lo {
                k4 += 1;
            }
        }
        if b < 3 && k4 < 3 {
            continue;
        }
        let (mut br, mut dr) = (0u16, 0u16);
        for k in 0..16usize {
            let v = at(k);
            if v > hi {
                br |= 1 << k;
            }
            if v < lo {
                dr |= 1 << k;
            }
        }
        if arc9(br) || arc9(dr) {
            out |= 1 << j;
        }
    }
    out
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod wasm_fast {
    use core::arch::wasm32::*;

    #[inline(always)]
    unsafe fn arc(m: &[v128; 16]) -> v128 {
        let mut a = [i8x16_splat(0); 16];
        for s in 0..16 {
            a[s] = v128_and(m[s], m[(s + 1) & 15]);
        }
        let mut b = [i8x16_splat(0); 16];
        for s in 0..16 {
            b[s] = v128_and(a[s], a[(s + 2) & 15]);
        }
        let mut any = i8x16_splat(0);
        for s in 0..16 {
            let c = v128_and(b[s], b[(s + 4) & 15]);
            any = v128_or(any, v128_and(c, m[(s + 8) & 15]));
        }
        any
    }

    #[inline(always)]
    pub unsafe fn fast9_x16(d: &[u8], p: usize, circ: &[isize; 16]) -> u16 {
        let base = d.as_ptr().add(p);
        let c = v128_load(base as *const v128);
        let hi = u8x16_add_sat(c, u8x16_splat(super::FAST_T));
        let lo = u8x16_sub_sat(c, u8x16_splat(super::FAST_T));
        let mut br = [i8x16_splat(0); 16];
        let mut dk = [i8x16_splat(0); 16];
        for k in 0..16 {
            let v = v128_load(base.offset(circ[k]) as *const v128);
            br[k] = u8x16_gt(v, hi);
            dk[k] = u8x16_lt(v, lo);
        }
        // masks are 0 or -1 per byte, so a sum of four is -count
        let nb = i8x16_add(i8x16_add(br[0], br[4]), i8x16_add(br[8], br[12]));
        let nd = i8x16_add(i8x16_add(dk[0], dk[4]), i8x16_add(dk[8], dk[12]));
        let m3 = i8x16_splat(-3);
        let quick = v128_or(i8x16_le(nb, m3), i8x16_le(nd, m3));
        if !v128_any_true(quick) {
            return 0;
        }
        let res = v128_and(quick, v128_or(arc(&br), arc(&dk)));
        i8x16_bitmask(res)
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
mod x86_fast {
    use core::arch::x86_64::*;

    #[inline(always)]
    unsafe fn arc(m: &[__m128i; 16]) -> __m128i {
        let z = _mm_setzero_si128();
        let mut a = [z; 16];
        for s in 0..16 {
            a[s] = _mm_and_si128(m[s], m[(s + 1) & 15]);
        }
        let mut b = [z; 16];
        for s in 0..16 {
            b[s] = _mm_and_si128(a[s], a[(s + 2) & 15]);
        }
        let mut any = z;
        for s in 0..16 {
            let c = _mm_and_si128(b[s], b[(s + 4) & 15]);
            any = _mm_or_si128(any, _mm_and_si128(c, m[(s + 8) & 15]));
        }
        any
    }

    #[inline(always)]
    pub unsafe fn fast9_x16(d: &[u8], p: usize, circ: &[isize; 16]) -> u16 {
        let base = d.as_ptr().add(p);
        let c = _mm_loadu_si128(base as *const __m128i);
        let t = _mm_set1_epi8(super::FAST_T as i8);
        let hi = _mm_adds_epu8(c, t);
        let lo = _mm_subs_epu8(c, t);
        let z = _mm_setzero_si128();
        let ones = _mm_cmpeq_epi8(z, z);
        let mut br = [z; 16];
        let mut dk = [z; 16];
        for k in 0..16 {
            let v = _mm_loadu_si128(base.offset(circ[k]) as *const __m128i);
            // unsigned v > hi  <=>  (v -sat hi) != 0 ; v < lo  <=>  (lo -sat v) != 0
            br[k] = _mm_xor_si128(_mm_cmpeq_epi8(_mm_subs_epu8(v, hi), z), ones);
            dk[k] = _mm_xor_si128(_mm_cmpeq_epi8(_mm_subs_epu8(lo, v), z), ones);
        }
        let nb = _mm_add_epi8(_mm_add_epi8(br[0], br[4]), _mm_add_epi8(br[8], br[12]));
        let nd = _mm_add_epi8(_mm_add_epi8(dk[0], dk[4]), _mm_add_epi8(dk[8], dk[12]));
        let m2 = _mm_set1_epi8(-2);
        let quick = _mm_or_si128(_mm_cmplt_epi8(nb, m2), _mm_cmplt_epi8(nd, m2));
        if _mm_movemask_epi8(quick) == 0 {
            return 0;
        }
        let res = _mm_and_si128(quick, _mm_or_si128(arc(&br), arc(&dk)));
        _mm_movemask_epi8(res) as u16
    }
}

// ------------------------------------------------------- descriptor matrix

use crate::geom42::Desc4;

/// A word load the vectorisers will not pack.
///
/// With SIMD128 enabled, LLVM turns popcount loops (`count_ones` over
/// consecutive words) into a byte-wise `i8x16.popcnt` reduction — or packs
/// the XORs into vectors only to extract every lane again for a scalar
/// popcount — and both run at a third of the speed of plain `i64.popcnt` in
/// V8 (measured on the descriptor scan: 6.7 ns per pair against 2.0).  A
/// volatile load is an ordinary `i64.load` that no vectoriser may merge, so
/// the loop stays the scalar one the baseline build already gets.  Natively,
/// where the scalar loop is what LLVM emits anyway, this is a plain load.
#[inline(always)]
pub fn word(p: &u64) -> u64 {
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    {
        // SAFETY: `p` is a valid, aligned reference
        return unsafe { (p as *const u64).read_volatile() };
    }
    #[allow(unreachable_code)]
    *p
}

/// `row[j] = hamming(q, b[j])` for every `j` — one row of the descriptor
/// matrix: four 64-bit popcounts per pair, one instruction each on x86-64-v2
/// and in WebAssembly.  The words of `b` go through `word` (see there).
#[inline]
pub fn hamming_row(q: &Desc4, b: &[Desc4], row: &mut [u16]) {
    let (q0, q1, q2, q3) = (q.q[0], q.q[1], q.q[2], q.q[3]);
    for (r, d) in row.iter_mut().zip(b.iter()) {
        *r = ((q0 ^ word(&d.q[0])).count_ones()
            + (q1 ^ word(&d.q[1])).count_ones()
            + (q2 ^ word(&d.q[2])).count_ones()
            + (q3 ^ word(&d.q[3])).count_ones()) as u16;
    }
}

/// The plain reference `hamming_row` is held to.
pub fn hamming_row_scalar(q: &Desc4, b: &[Desc4], row: &mut [u16]) {
    let (q0, q1, q2, q3) = (q.q[0], q.q[1], q.q[2], q.q[3]);
    for (r, d) in row.iter_mut().zip(b.iter()) {
        *r = ((q0 ^ d.q[0]).count_ones()
            + (q1 ^ d.q[1]).count_ones()
            + (q2 ^ d.q[2]).count_ones()
            + (q3 ^ d.q[3]).count_ones()) as u16;
    }
}

/// "No index" in the scan's `u16` best-index arrays.
pub const NONE16: u16 = u16::MAX;

/// One row of the mutual best / second-best scan, `i` being the row.
///
/// Returns the row's `(best, second, first index of best)`, exactly what the
/// reference's sequential "strictly smaller replaces, else strictly smaller
/// than the second" rule leaves after walking the row from 999/999/none: the
/// minimum, the second smallest WITH multiplicity, and the first position of
/// the minimum.  And it applies the same rule to every column, in row order:
///
/// ```text
///     if d < d1[j]       { d2[j] = d1[j]; d1[j] = d; best[j] = i }
///     else if d < d2[j]  { d2[j] = d }
/// ```
///
/// written as selects — the new second is the old first when `d` takes the
/// lead, else the smaller of the old second and `d` — so eight columns update
/// per vector.  Every distance is at most 256 and every sentinel 999, so the
/// signed 16-bit compares are exact.
pub fn scan_row(row: &[u16], i: u16, d1: &mut [u16], d2: &mut [u16], best: &mut [u16]) -> (u16, u16, u16) {
    let n = row.len();
    assert!(d1.len() >= n && d2.len() >= n && best.len() >= n);
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    {
        return unsafe { wasm_scan::scan_row(row, i, d1, d2, best) };
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "sse4.1"))]
    {
        return unsafe { x86_scan::scan_row(row, i, d1, d2, best) };
    }
    #[allow(unreachable_code)]
    scan_row_portable(row, i, d1, d2, best)
}

/// The scan without vectors and without branches: the same rule written as
/// selects, which compile to conditional moves.  The sequential form's
/// branches are data-dependent and mispredict on nearly every column of a
/// descriptor matrix.
pub fn scan_row_portable(row: &[u16], i: u16, d1: &mut [u16], d2: &mut [u16], best: &mut [u16]) -> (u16, u16, u16) {
    let n = row.len();
    let (d1, d2, best) = (&mut d1[..n], &mut d2[..n], &mut best[..n]);
    let (mut m1, mut m2, mut at) = (999u16, 999u16, NONE16);
    for j in 0..n {
        let d = row[j];
        // row: the second becomes the larger of the old minimum and d, if
        // that beats it; the first position of the minimum is kept on ties
        let lead = d < m1;
        m2 = m2.min(m1.max(d));
        at = if lead { j as u16 } else { at };
        m1 = m1.min(d);
        // column j, in row order
        let (c1, c2) = (d1[j], d2[j]);
        let take = d < c1;
        d2[j] = if take { c1 } else { c2.min(d) };
        d1[j] = c1.min(d);
        best[j] = if take { i } else { best[j] };
    }
    (m1, m2, at)
}

/// The sequential reference `scan_row` is held to.
pub fn scan_row_scalar(row: &[u16], i: u16, d1: &mut [u16], d2: &mut [u16], best: &mut [u16]) -> (u16, u16, u16) {
    let (mut m1, mut m2, mut at) = (999u16, 999u16, NONE16);
    for (j, &d) in row.iter().enumerate() {
        if d < m1 {
            m2 = m1;
            m1 = d;
            at = j as u16;
        } else if d < m2 {
            m2 = d;
        }
        if d < d1[j] {
            d2[j] = d1[j];
            d1[j] = d;
            best[j] = i;
        } else if d < d2[j] {
            d2[j] = d;
        }
    }
    (m1, m2, at)
}

/// Lane reductions of the per-lane (min, second) pairs into the row's pair:
/// the minimum of the minima, and the second smallest of the minima and the
/// seconds together — the row's second smallest is either the second of the
/// lane holding the minimum or another lane's minimum.
#[allow(dead_code)]
#[inline(always)]
fn fold_lanes(m1: &[u16; 8], m2: &[u16; 8]) -> (u16, u16) {
    let (mut a, mut b) = (999u16, 999u16);
    for k in 0..8 {
        for &v in [m1[k], m2[k]].iter() {
            if v < a {
                b = a;
                a = v;
            } else if v < b {
                b = v;
            }
        }
    }
    (a, b)
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
mod wasm_scan {
    use core::arch::wasm32::*;

    pub unsafe fn scan_row(row: &[u16], i: u16, d1: &mut [u16], d2: &mut [u16], best: &mut [u16]) -> (u16, u16, u16) {
        let n = row.len();
        let full = n & !7;
        let iv = u16x8_splat(i);
        let mut l1 = u16x8_splat(999);
        let mut l2 = u16x8_splat(999);
        let mut j = 0;
        while j < full {
            let d = v128_load(row.as_ptr().add(j) as *const v128);
            let p1 = d1.as_mut_ptr().add(j) as *mut v128;
            let p2 = d2.as_mut_ptr().add(j) as *mut v128;
            let pb = best.as_mut_ptr().add(j) as *mut v128;
            let c1 = v128_load(p1);
            let c2 = v128_load(p2);
            let lead = i16x8_lt(d, c1);
            v128_store(p2, v128_bitselect(c1, u16x8_min(c2, d), lead));
            v128_store(p1, u16x8_min(c1, d));
            v128_store(pb, v128_bitselect(iv, v128_load(pb), lead));
            // per-lane (min, second): the second becomes the larger of the
            // old min and d, if that beats it
            l2 = u16x8_min(l2, u16x8_max(l1, d));
            l1 = u16x8_min(l1, d);
            j += 8;
        }
        let (mut a1, mut a2) = ([0u16; 8], [0u16; 8]);
        v128_store(a1.as_mut_ptr() as *mut v128, l1);
        v128_store(a2.as_mut_ptr() as *mut v128, l2);
        let (mut m1, mut m2) = super::fold_lanes(&a1, &a2);
        for j in full..n {
            let d = row[j];
            if d < m1 {
                m2 = m1;
                m1 = d;
            } else if d < m2 {
                m2 = d;
            }
            if d < d1[j] {
                d2[j] = d1[j];
                d1[j] = d;
                best[j] = i;
            } else if d < d2[j] {
                d2[j] = d;
            }
        }
        // the first position of the minimum
        let mut at = super::NONE16;
        if m1 < 999 {
            let mv = u16x8_splat(m1);
            let mut j = 0;
            while j < full {
                let m = i16x8_bitmask(i16x8_eq(v128_load(row.as_ptr().add(j) as *const v128), mv));
                if m != 0 {
                    at = (j + m.trailing_zeros() as usize) as u16;
                    break;
                }
                j += 8;
            }
            if at == super::NONE16 {
                at = (full + row[full..].iter().position(|&d| d == m1).unwrap()) as u16;
            }
        }
        (m1, m2, at)
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "sse4.1"))]
mod x86_scan {
    use core::arch::x86_64::*;

    pub unsafe fn scan_row(row: &[u16], i: u16, d1: &mut [u16], d2: &mut [u16], best: &mut [u16]) -> (u16, u16, u16) {
        let n = row.len();
        let full = n & !7;
        let iv = _mm_set1_epi16(i as i16);
        let mut l1 = _mm_set1_epi16(999);
        let mut l2 = _mm_set1_epi16(999);
        let mut j = 0;
        while j < full {
            let d = _mm_loadu_si128(row.as_ptr().add(j) as *const __m128i);
            let p1 = d1.as_mut_ptr().add(j) as *mut __m128i;
            let p2 = d2.as_mut_ptr().add(j) as *mut __m128i;
            let pb = best.as_mut_ptr().add(j) as *mut __m128i;
            let c1 = _mm_loadu_si128(p1);
            let c2 = _mm_loadu_si128(p2);
            let lead = _mm_cmplt_epi16(d, c1);
            _mm_storeu_si128(p2, _mm_blendv_epi8(_mm_min_epu16(c2, d), c1, lead));
            _mm_storeu_si128(p1, _mm_min_epu16(c1, d));
            _mm_storeu_si128(pb, _mm_blendv_epi8(_mm_loadu_si128(pb), iv, lead));
            l2 = _mm_min_epu16(l2, _mm_max_epu16(l1, d));
            l1 = _mm_min_epu16(l1, d);
            j += 8;
        }
        let (mut a1, mut a2) = ([0u16; 8], [0u16; 8]);
        _mm_storeu_si128(a1.as_mut_ptr() as *mut __m128i, l1);
        _mm_storeu_si128(a2.as_mut_ptr() as *mut __m128i, l2);
        let (mut m1, mut m2) = super::fold_lanes(&a1, &a2);
        for j in full..n {
            let d = row[j];
            if d < m1 {
                m2 = m1;
                m1 = d;
            } else if d < m2 {
                m2 = d;
            }
            if d < d1[j] {
                d2[j] = d1[j];
                d1[j] = d;
                best[j] = i;
            } else if d < d2[j] {
                d2[j] = d;
            }
        }
        let mut at = super::NONE16;
        if m1 < 999 {
            let mv = _mm_set1_epi16(m1 as i16);
            let mut j = 0;
            while j < full {
                let eq = _mm_cmpeq_epi16(_mm_loadu_si128(row.as_ptr().add(j) as *const __m128i), mv);
                let m = _mm_movemask_epi8(eq) as u32;
                if m != 0 {
                    at = (j + (m.trailing_zeros() as usize >> 1)) as u16;
                    break;
                }
                j += 8;
            }
            if at == super::NONE16 {
                at = (full + row[full..].iter().position(|&d| d == m1).unwrap()) as u16;
            }
        }
        (m1, m2, at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xs(seed: u64) -> impl FnMut() -> u64 {
        let mut s = seed | 1;
        move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        }
    }

    /// The vector median and mask equal the radix reference and a plain sort
    /// on every input class the windows produce: dense ranges, two-valued
    /// dither, heavy ties, constants, the 0 and 1023 extremes.
    #[test]
    fn median_mask_matches_sort() {
        let mut r = xs(0x1234_5678);
        for case in 0..20000 {
            let mut c: Cells = [0; 64];
            for v in c.iter_mut() {
                *v = match case % 7 {
                    0 => (r() % 1024) as u16,
                    1 => [2u16, 514][(r() % 2) as usize],
                    2 => (2 * ((r() % 256) + 1)) as u16,
                    3 => 257,
                    4 => (r() % 3) as u16 * 511,
                    5 => [0u16, 1023, 512, 511][(r() % 4) as usize],
                    _ => (r() % 256) as u16,
                };
            }
            let mut srt = c;
            srt.sort_unstable();
            let want = srt[31];
            let mut m = 0u64;
            for (i, &v) in c.iter().enumerate() {
                if 2 * v as u32 > srt[31] as u32 + srt[32] as u32 {
                    m |= 1u64 << i;
                }
            }
            assert_eq!(median_mask_scalar(&c), (want, m), "scalar, case {case}");
            assert_eq!(median_mask(&c), (want, m), "vector, case {case}");
        }
    }

    /// The doubled-mask run test is the reference's 24-step walk, on every one
    /// of the 65536 circle masks.
    #[test]
    fn arc9_is_the_24_step_walk() {
        for mask in 0..=u16::MAX {
            let mut run = 0;
            let mut want = false;
            for k in 0..24usize {
                if (mask >> (k & 15)) & 1 != 0 {
                    run += 1;
                    if run >= 9 {
                        want = true;
                        break;
                    }
                } else {
                    run = 0;
                }
            }
            assert_eq!(arc9(mask), want, "mask {mask:#06x}");
        }
    }

    /// The vector FAST block equals the per-pixel reference on dithered,
    /// noisy, flat and saturated levels, including values within 18 of 0
    /// and 255 where the saturating thresholds matter.
    #[test]
    fn fast9_block_matches_scalar() {
        let mut r = xs(0xfa57_0009);
        for case in 0..400 {
            let (w, h) = (64usize, 24usize);
            let mut d = vec![0u8; w * h];
            for y in 0..h {
                for x in 0..w {
                    d[y * w + x] = match case % 5 {
                        0 => (r() % 256) as u8,
                        1 => if ((x ^ y) & 1) == 0 { 10 } else { 250 },
                        2 => [0u8, 17, 18, 19, 36, 237, 238, 255][(r() % 8) as usize],
                        3 => ((x * 37 + y * 11) % 256) as u8,
                        _ => if (r() % 7) == 0 { (r() % 256) as u8 } else { 128 },
                    };
                }
            }
            let circ: [isize; 16] = {
                const C: [(isize, isize); 16] = [
                    (0, -3), (1, -3), (2, -2), (3, -1), (3, 0), (3, 1), (2, 2), (1, 3),
                    (0, 3), (-1, 3), (-2, 2), (-3, 1), (-3, 0), (-3, -1), (-2, -2), (-1, -3),
                ];
                let mut o = [0isize; 16];
                for k in 0..16 {
                    o[k] = C[k].1 * w as isize + C[k].0;
                }
                o
            };
            for y in 3..h - 3 {
                for x in 3..w - 3 - 16 {
                    let p = y * w + x;
                    unsafe {
                        assert_eq!(fast9_x16(&d, p, &circ), fast9_x16_scalar(&d, p, &circ), "case {case} ({x},{y})");
                    }
                }
            }
        }
    }

    /// The row kernel equals the per-pair popcount on every length class the
    /// vector loop and its tail produce.
    #[test]
    fn hamming_row_matches_scalar() {
        let mut r = xs(0xd15c_0001);
        for n in [0usize, 1, 7, 8, 9, 15, 16, 17, 63, 64, 65, 512] {
            let b: Vec<Desc4> = (0..n).map(|_| Desc4 { q: [r(), r(), r(), r()] }).collect();
            for _ in 0..4 {
                let q = Desc4 { q: [r(), r(), r(), r()] };
                let (mut x, mut y) = (vec![0u16; n], vec![0u16; n]);
                hamming_row(&q, &b, &mut x);
                hamming_row_scalar(&q, &b, &mut y);
                assert_eq!(x, y, "n = {n}");
                // and against the definition, including the extremes
                for j in 0..n {
                    let d: u32 = (0..4).map(|k| (q.q[k] ^ b[j].q[k]).count_ones()).sum();
                    assert_eq!(x[j] as u32, d);
                }
            }
            if n > 0 {
                let q = b[0];
                let mut x = vec![0u16; n];
                hamming_row(&q, &b, &mut x);
                assert_eq!(x[0], 0, "self distance");
                let inv = Desc4 { q: [!q.q[0], !q.q[1], !q.q[2], !q.q[3]] };
                hamming_row(&inv, &b, &mut x);
                assert_eq!(x[0], 256, "complement distance");
            }
        }
    }

    /// The lane scan equals the sequential rule on random rows, rows of
    /// repeated minima, rows shorter than a vector and odd tails, with the
    /// column state carried across many rows.
    #[test]
    fn scan_row_matches_sequential() {
        let mut r = xs(0x5ca1_ab1e);
        for case in 0..300 {
            let n = [1usize, 3, 7, 8, 9, 15, 16, 17, 64, 100, 512][case % 11];
            let rows = 1 + (r() % 40) as usize;
            let (mut a1, mut a2, mut ab) = (vec![999u16; n], vec![999u16; n], vec![NONE16; n]);
            let (mut b1, mut b2, mut bb) = (a1.clone(), a2.clone(), ab.clone());
            let (mut c1, mut c2, mut cb) = (a1.clone(), a2.clone(), ab.clone());
            for i in 0..rows {
                let row: Vec<u16> = (0..n)
                    .map(|_| match case % 4 {
                        0 => (r() % 257) as u16,
                        1 => (r() % 3) as u16 * 40,
                        2 => 100 + (r() % 2) as u16,
                        _ => (r() % 20) as u16,
                    })
                    .collect();
                let x = scan_row(&row, i as u16, &mut a1, &mut a2, &mut ab);
                let y = scan_row_scalar(&row, i as u16, &mut b1, &mut b2, &mut bb);
                let z = scan_row_portable(&row, i as u16, &mut c1, &mut c2, &mut cb);
                assert_eq!(x, y, "row result, case {case} row {i}");
                assert_eq!(z, y, "portable row result, case {case} row {i}");
            }
            assert_eq!((&a1, &a2, &ab), (&b1, &b2, &bb), "column state, case {case}");
            assert_eq!((&c1, &c2, &cb), (&b1, &b2, &bb), "portable column state, case {case}");
        }
    }
}
