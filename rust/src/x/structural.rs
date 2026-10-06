//! Structural evidence, lazily (PAPH-X §10, §27, §28).
//!
//! The seven structural channels of comparator 42 — the §10 local channel
//! and the six Tier-1 secondaries — with two changes and no change of
//! value:
//!
//!   * every secondary is computed without allocating: the v3 channel code
//!     builds transformed blocks, pair lists and marker vectors on the heap
//!     for every hypothesis, and the DCT channel alone did that 336 times
//!     per pair.  The rewrites here are held equal to the originals, value
//!     for value, by a test over the equivalence corpus' transforms;
//!   * every channel is OPTIONAL.  The scheduler asks for a channel only
//!     when the verdict lattice can still move with it; until then it is
//!     carried as an interval.  The local channel in particular is bounded
//!     from above from the exact edge set alone — rows and columns with an
//!     edge within `hamming_t` and their burst weights — before the
//!     assignment and its four nulls are paid for (§10.2).
//!
//! When every channel has been asked for, the structural score is exactly
//! comparator 42's, to the digit.

use super::prepared::{XPrepared, M_DCT, M_LOCAL, M_PALETTE, M_RUNS, M_SHAPE, M_SILHOUETTE, M_TOPOLOGY};
use crate::calibration::Profile;
use crate::config::{chance_correct, clamp, SCALE};
use crate::local_v4::{local_v4_41_shared, LocalV4};
use crate::prepared::BagShare;
use crate::tables::pop_table;
use crate::wire::{Tier1, F_FLAT, F_SIL};

/// Channel order of the lattice input (SPEC-004 §14).
pub const CH_DCT: usize = 0;
pub const CH_LOCAL: usize = 1;
pub const CH_SHAPE: usize = 2;
pub const CH_TOPOLOGY: usize = 3;
pub const CH_RUNS: usize = 4;
pub const CH_PALETTE: usize = 5;
pub const CH_SILHOUETTE: usize = 6;
pub const CH_NAMES: [&str; 7] = ["dct", "local", "shape", "topology", "runs", "palette", "silhouette"];

// ------------------------------------------------------------------ dct

struct DctSide {
    s0: [u8; 256],
    m0: [u8; 256],
    s1: [[u8; 64]; 4],
    m1: [[u8; 64]; 4],
    s2: [[u8; 16]; 16],
    m2: [[u8; 16]; 16],
}

fn unpack_into(bytes: &[u8], off: usize, n: usize, bits: usize, sign: &mut [u8], mag: &mut [u8]) {
    let mbits = bits - 1;
    let mut bit = 0usize;
    for i in 0..n {
        let mut code = 0u32;
        for _ in 0..bits {
            code = (code << 1) | ((bytes[off + (bit >> 3)] >> (7 - (bit & 7))) & 1) as u32;
            bit += 1;
        }
        sign[i] = ((code >> mbits) & 1) as u8;
        mag[i] = (code & ((1u32 << mbits) - 1)) as u8;
    }
}

fn dct_side(t: &Tier1) -> DctSide {
    let d = t.sec("dct");
    let mut s = DctSide { s0: [0; 256], m0: [0; 256], s1: [[0; 64]; 4], m1: [[0; 64]; 4], s2: [[0; 16]; 16], m2: [[0; 16]; 16] };
    unpack_into(d, 0, 256, 2, &mut s.s0, &mut s.m0);
    for i in 0..4 {
        unpack_into(d, 64 + i * 16, 64, 2, &mut s.s1[i], &mut s.m1[i]);
    }
    for i in 0..16 {
        unpack_into(d, 128 + i * 8, 16, 4, &mut s.s2[i], &mut s.m2[i]);
    }
    s
}

/// `block_sim(d4_block(a, n, tr, fh, fv), b, inv)` without the block.
#[inline(always)]
fn block_sim_t(sa: &[u8], ma: &[u8], sb: &[u8], mb: &[u8], n: usize, bits: i64, tr: bool, fh: bool, fv: bool, inv: bool, pop: &[u8]) -> i64 {
    let f = inv as u8;
    let mut d = 0i64;
    for i in 1..n * n {
        let (v, u) = (i / n, i % n);
        let src = if tr { u * n + v } else { v * n + u };
        let mut s = sa[src];
        if fh && (u & 1) != 0 {
            s ^= 1;
        }
        if fv && (v & 1) != 0 {
            s ^= 1;
        }
        d += ((s ^ f) ^ sb[i]) as i64;
        d += pop[(ma[src] ^ mb[i]) as usize] as i64;
    }
    let nb = (n as i64 * n as i64 - 1) * bits;
    clamp(SCALE - 2 * d * SCALE / nb, 0, SCALE)
}

#[inline(always)]
fn d4_grid(gx: usize, gy: usize, g: usize, tr: bool, fh: bool, fv: bool) -> usize {
    let (mut x, mut y) = (gx, gy);
    if tr {
        std::mem::swap(&mut x, &mut y);
    }
    if fh {
        x = g - 1 - x;
    }
    if fv {
        y = g - 1 - y;
    }
    y * g + x
}

/// The DCT channel's (value, raw, control) — `compare::dct_channel`.
pub fn dct_value(a: &Tier1, b: &Tier1) -> Option<(i64, i64, i64)> {
    if a.flags & F_FLAT != 0 || b.flags & F_FLAT != 0 {
        return None;
    }
    let pop = pop_table();
    let (x, y) = (dct_side(a), dct_side(b));
    let mut per = [0i64; 16];
    let mut best = i64::MIN;
    let mut k = 0usize;
    for inv in [false, true] {
        for t in 0..8usize {
            let (tr, fh, fv) = ((t >> 2) & 1 == 1, (t >> 1) & 1 == 1, t & 1 == 1);
            let l0 = block_sim_t(&x.s0, &x.m0, &y.s0, &y.m0, 16, 2, tr, fh, fv, inv, pop);
            let mut l1 = 0i64;
            for i in 0..4usize {
                let j = d4_grid(i & 1, i >> 1, 2, tr, fh, fv);
                l1 += block_sim_t(&x.s1[i], &x.m1[i], &y.s1[j], &y.m1[j], 8, 2, tr, fh, fv, inv, pop);
            }
            l1 /= 4;
            let mut l2 = 0i64;
            for i in 0..16usize {
                let j = d4_grid(i & 3, i >> 2, 4, tr, fh, fv);
                l2 += block_sim_t(&x.s2[i], &x.m2[i], &y.s2[j], &y.m2[j], 4, 4, tr, fh, fv, inv, pop);
            }
            l2 /= 16;
            let s = (2 * l0 + 2 * l1 + l2) / 5;
            per[k] = s;
            if s > best {
                best = s;
            }
            k += 1;
        }
    }
    let mid = 8;
    let med = *per.select_nth_unstable(mid).1;
    Some((chance_correct(best, med), best, med))
}

// ---------------------------------------------------------------- shape

struct ShapeX {
    area: u32,
    per: i64,
    aspect: i64,
    holes: i64,
    radial: [i64; 32],
}

fn read_shapes_x(t: &Tier1, out: &mut [ShapeX; 8]) -> usize {
    let s = t.sec("shapes");
    let n = t.count("shapes").min(8);
    for i in 0..n {
        let o = i * 41;
        let r = &mut out[i];
        r.area = u32::from_le_bytes([s[o], s[o + 1], s[o + 2], s[o + 3]]);
        r.per = u16::from_le_bytes([s[o + 4], s[o + 5]]) as i64;
        r.aspect = u16::from_le_bytes([s[o + 6], s[o + 7]]) as i64;
        r.holes = s[o + 8] as i64;
        for k in 0..32 {
            r.radial[k] = s[o + 9 + k] as i64;
        }
    }
    n
}

/// `compare::radial_score`: the best of 64 alignments (32 rotations, both
/// senses) of two radial signatures against the median of the 64.  The
/// signatures are bytes, so the 64 sums of absolute differences are
/// computed in byte lanes (`sad64`), held to the scalar loop by a test.
fn radial_score_x(ra: &[i64; 32], rb: &[i64; 32]) -> i64 {
    let mut a = [0u8; 32];
    let mut b = [0u8; 32];
    for i in 0..32 {
        a[i] = ra[i] as u8;
        b[i] = rb[i] as u8;
    }
    let mut all = [0i64; 64];
    sad64(&a, &b, &mut all);
    let best = *all.iter().min().unwrap();
    let med = *all.select_nth_unstable(32).1;
    if med <= 0 {
        0
    } else {
        clamp((med - best) * SCALE / med, 0, SCALE)
    }
}

/// `out[flip * 32 + rot] = (Σ_i |a[i] − b[j]|) / 32` with `j = (rot + i) & 31`
/// for the direct sense and `j = (rot + 32 − i) & 31` for the reversed one.
pub fn sad64(a: &[u8; 32], b: &[u8; 32], out: &mut [i64; 64]) {
    // the reversed sense reads b backwards: with b'[k] = b[(32 - k) & 31],
    // the window of b' at offset `rot` is b[(32 - rot - i) & 31], which is
    // the scalar loop's reversed alignment at rotation (32 - rot) & 31 — so
    // both doubled sequences are built once and every alignment is one
    // contiguous window
    let mut d = [0u8; 64];
    let mut r = [0u8; 64];
    for i in 0..64 {
        d[i] = b[i & 31];
        r[i] = b[(32 - (i & 31)) & 31];
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "sse2"))]
    unsafe {
        use core::arch::x86_64::*;
        let a0 = _mm_loadu_si128(a.as_ptr() as *const __m128i);
        let a1 = _mm_loadu_si128(a.as_ptr().add(16) as *const __m128i);
        for rot in 0..32usize {
            let b0 = _mm_loadu_si128(d.as_ptr().add(rot) as *const __m128i);
            let b1 = _mm_loadu_si128(d.as_ptr().add(rot + 16) as *const __m128i);
            let s = _mm_add_epi64(_mm_sad_epu8(a0, b0), _mm_sad_epu8(a1, b1));
            let t = _mm_cvtsi128_si64(s) + _mm_cvtsi128_si64(_mm_srli_si128::<8>(s));
            out[rot] = t / 32;
            let c0 = _mm_loadu_si128(r.as_ptr().add(rot) as *const __m128i);
            let c1 = _mm_loadu_si128(r.as_ptr().add(rot + 16) as *const __m128i);
            let s = _mm_add_epi64(_mm_sad_epu8(a0, c0), _mm_sad_epu8(a1, c1));
            let t = _mm_cvtsi128_si64(s) + _mm_cvtsi128_si64(_mm_srli_si128::<8>(s));
            out[32 + ((32 - rot) & 31)] = t / 32;
        }
        return;
    }
    #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
    unsafe {
        use core::arch::wasm32::*;
        let a0 = v128_load(a.as_ptr() as *const v128);
        let a1 = v128_load(a.as_ptr().add(16) as *const v128);
        let sum = |x: v128, y: v128| -> i64 {
            // |x - y| per byte via max - min, widened and summed
            let m = u8x16_sub(u8x16_max(x, y), u8x16_min(x, y));
            let w = u16x8_extadd_pairwise_u8x16(m);
            let w = u32x4_extadd_pairwise_u16x8(w);
            (u32x4_extract_lane::<0>(w) + u32x4_extract_lane::<1>(w) + u32x4_extract_lane::<2>(w) + u32x4_extract_lane::<3>(w)) as i64
        };
        for rot in 0..32usize {
            let b0 = v128_load(d.as_ptr().add(rot) as *const v128);
            let b1 = v128_load(d.as_ptr().add(rot + 16) as *const v128);
            out[rot] = (sum(a0, b0) + sum(a1, b1)) / 32;
            let c0 = v128_load(r.as_ptr().add(rot) as *const v128);
            let c1 = v128_load(r.as_ptr().add(rot + 16) as *const v128);
            out[32 + ((32 - rot) & 31)] = (sum(a0, c0) + sum(a1, c1)) / 32;
        }
        return;
    }
    #[allow(unreachable_code)]
    sad64_scalar(a, b, out)
}

/// The scalar reference of `sad64`: the v3 loops verbatim.
pub fn sad64_scalar(a: &[u8; 32], b: &[u8; 32], out: &mut [i64; 64]) {
    for flip in 0..2 {
        for rot in 0..32usize {
            let mut s = 0i64;
            for i in 0..32usize {
                let j = if flip == 1 { (rot + 32 - i) & 31 } else { (rot + i) & 31 };
                s += (a[i] as i64 - b[j] as i64).abs();
            }
            out[flip * 32 + rot] = s / 32;
        }
    }
}

fn shape_pair_x(a: &ShapeX, b: &ShapeX) -> i64 {
    let v = radial_score_x(&a.radial, &b.radial);
    let (ra, rb) = (a.aspect.max(1), b.aspect.max(1));
    let asp = clamp(ra.min(rb) * SCALE / ra.max(rb), 0, SCALE);
    let ia = if a.area > 0 { a.per * a.per * 256 / a.area as i64 } else { 0 };
    let ib = if b.area > 0 { b.per * b.per * 256 / b.area as i64 } else { 0 };
    let iso = if ia > 0 && ib > 0 { clamp(ia.min(ib) * SCALE / ia.max(ib), 0, SCALE) } else { SCALE };
    let hol = if a.holes == b.holes { SCALE } else { clamp(SCALE - 2500 * (a.holes - b.holes).abs(), 0, SCALE) };
    v * asp / SCALE * iso / SCALE * hol / SCALE
}

const EMPTY_SHAPE: ShapeX = ShapeX { area: 0, per: 0, aspect: 0, holes: 0, radial: [0; 32] };

/// The shape channel's value — `compare::shape_channel`, the greedy pairing
/// by (score desc, area sum asc, pair order) over a fixed array.
pub fn shape_value(a: &Tier1, b: &Tier1) -> Option<i64> {
    let mut xs = [EMPTY_SHAPE; 8];
    let mut ys = [EMPTY_SHAPE; 8];
    let n = read_shapes_x(a, &mut xs);
    let m = read_shapes_x(b, &mut ys);
    if n == 0 || m == 0 {
        return None;
    }
    let mut pairs = [(0u8, 0u8, 0i64, 0i64, 0u8); 64];
    let mut k = 0usize;
    for i in 0..n {
        for j in 0..m {
            pairs[k] = (i as u8, j as u8, shape_pair_x(&xs[i], &ys[j]), xs[i].area as i64 + ys[j].area as i64, k as u8);
            k += 1;
        }
    }
    let list = &mut pairs[..k];
    list.sort_unstable_by(|p, q| q.2.cmp(&p.2).then(p.3.cmp(&q.3)).then(p.4.cmp(&q.4)));
    let (mut ux, mut uy) = ([false; 8], [false; 8]);
    let mut tot = 0i64;
    for p in list.iter() {
        let (i, j) = (p.0 as usize, p.1 as usize);
        if ux[i] || uy[j] {
            continue;
        }
        ux[i] = true;
        uy[j] = true;
        tot += p.2;
    }
    let cover = n.min(m) as i64;
    Some(tot / cover.max(1))
}

// ------------------------------------------------------------- topology

#[derive(Clone, Copy, Default)]
struct RagX {
    qa: i64,
    qb: i64,
    ra: i64,
    rb: i64,
    n: i64,
}

fn read_rag_x(t: &Tier1, out: &mut [RagX; 48]) -> usize {
    let s = t.sec("rag");
    let n = t.count("rag").min(48);
    for i in 0..n {
        let o = i * 6;
        out[i] = RagX { qa: s[o] as i64, qb: s[o + 1] as i64, ra: s[o + 2] as i64, rb: s[o + 3] as i64, n: u16::from_le_bytes([s[o + 4], s[o + 5]]) as i64 };
    }
    n
}

fn rag_raw_x(x: &[RagX], y: &[RagX], shift: i64, rank: bool) -> i64 {
    let tol = 16i64;
    let (mut inter, mut tx, mut ty) = (0i64, 0i64, 0i64);
    let mut used = [false; 48];
    for e in x.iter() {
        tx += e.n;
    }
    for e in y.iter() {
        ty += e.n;
    }
    for e in x.iter() {
        let (xa, xb) = if rank { (e.ra, e.rb) } else { (e.qa, e.qb) };
        let (mut best, mut bd) = (-1i64, i64::MAX);
        for (j, f) in y.iter().enumerate() {
            if used[j] {
                continue;
            }
            let (fa, fb) = if rank { (f.ra, f.rb) } else { (f.qa, f.qb) };
            let ya = (fa + shift) & 255;
            let yb = (fb + shift) & 255;
            let (lo, hi) = (ya.min(yb), ya.max(yb));
            let d = (xa - lo).abs() + (xb - hi).abs();
            if d < bd {
                bd = d;
                best = j as i64;
            }
        }
        if best >= 0 && bd <= 2 * tol {
            used[best as usize] = true;
            inter += e.n.min(y[best as usize].n);
        }
    }
    let uni = tx + ty - inter;
    if uni > 0 {
        clamp(inter * SCALE / uni, 0, SCALE)
    } else {
        0
    }
}

/// The topology channel's (value, raw, control) under the Rank endpoint
/// (the only one container 3 binds) — `compare::topology_channel`.
pub fn topology_value(a: &Tier1, b: &Tier1) -> Option<(i64, i64, i64)> {
    let (mut xs, mut ys) = ([RagX::default(); 48], [RagX::default(); 48]);
    let n = read_rag_x(a, &mut xs);
    let m = read_rag_x(b, &mut ys);
    if n < 3 || m < 3 {
        return None;
    }
    let (x, y) = (&xs[..n], &ys[..m]);
    let raw = rag_raw_x(x, y, 0, true);
    let mut ctl = SCALE;
    for s in [85i64, 128, 171] {
        ctl = ctl.min(rag_raw_x(x, y, s, true));
    }
    Some((chance_correct(raw, ctl), raw, ctl))
}

// ------------------------------------------------------------------ runs

fn run_raw(a: &Tier1, b: &Tier1, rot: usize) -> i64 {
    let (x, y) = (a.sec("runs"), b.sec("runs"));
    let mut s = 0i64;
    for axis in 0..3usize {
        for i in 0..16usize {
            s += (x[axis * 16 + i] as i64 - y[axis * 16 + ((i + rot) & 15)] as i64).abs();
        }
    }
    clamp(SCALE - s * SCALE / (3 * 510), 0, SCALE)
}

/// `compare::runs_channel`; the flatness test is per side (`XPrepared.meas`).
pub fn runs_value(a: &Tier1, b: &Tier1) -> (i64, i64, i64) {
    let raw = run_raw(a, b, 0);
    let mut ctl = SCALE;
    for r in [4usize, 8, 12] {
        ctl = ctl.min(run_raw(a, b, r));
    }
    (chance_correct(raw, ctl), raw, ctl)
}

// --------------------------------------------------------------- palette

#[derive(Clone, Copy, Default)]
struct PalX {
    freq: i64,
    lum: i64,
    q: i64,
}

fn read_pal_x(t: &Tier1, out: &mut [PalX; 24]) -> usize {
    let s = t.sec("palette");
    let n = t.count("palette").min(24);
    for i in 0..n {
        let o = i << 2;
        out[i] = PalX { freq: s[o + 1] as i64, lum: s[o + 2] as i64, q: s[o + 3] as i64 };
    }
    n
}

fn pal_raw_x(x: &[PalX], y: &[PalX], shift: i64) -> i64 {
    let mut used = [false; 24];
    let (mut hit, mut tot) = (0i64, 0i64);
    for e in x.iter() {
        tot += e.freq;
    }
    for e in x.iter() {
        let (mut best, mut bd) = (-1i64, i64::MAX);
        for (j, f) in y.iter().enumerate() {
            if used[j] {
                continue;
            }
            let d = (e.q - ((f.q + shift) & 255)).abs() * 2 + (e.freq - f.freq).abs();
            if d < bd {
                bd = d;
                best = j as i64;
            }
        }
        if best >= 0 && bd <= 96 {
            used[best as usize] = true;
            hit += e.freq;
        }
    }
    if tot > 0 {
        clamp(hit * SCALE / tot, 0, SCALE)
    } else {
        0
    }
}

/// `compare::palette_channel`: (value, raw, control, inverted).
pub fn palette_value(a: &Tier1, b: &Tier1) -> Option<(i64, i64, i64, bool)> {
    let (mut xs, mut ys) = ([PalX::default(); 24], [PalX::default(); 24]);
    let n = read_pal_x(a, &mut xs);
    let m = read_pal_x(b, &mut ys);
    if n < 2 || m < 2 {
        return None;
    }
    let mut yi = [PalX::default(); 24];
    for j in 0..m {
        yi[j] = PalX { freq: ys[j].freq, lum: 255 - ys[j].lum, q: 255 - ys[j].q };
    }
    let x = &xs[..n];
    let pass = |z: &[PalX]| -> (i64, i64, i64) {
        let raw = pal_raw_x(x, z, 0).min(pal_raw_x(z, x, 0));
        let mut ctl = SCALE;
        for s in [85i64, 128, 171] {
            ctl = ctl.min(pal_raw_x(x, z, s).min(pal_raw_x(z, x, s)));
        }
        (raw, ctl, chance_correct(raw, ctl))
    };
    let d = pass(&ys[..m]);
    let i = pass(&yi[..m]);
    let inv = i.2 > d.2;
    let u = if inv { i } else { d };
    Some((u.2, u.0, u.1, inv))
}

// ------------------------------------------------------------ silhouette

fn rot64(hi: u32, lo: u32, k: u32) -> (u32, u32) {
    let k = k & 63;
    if k == 0 {
        (hi, lo)
    } else if k == 32 {
        (lo, hi)
    } else if k < 32 {
        ((hi << k) | (lo >> (32 - k)), (lo << k) | (hi >> (32 - k)))
    } else {
        let j = k - 32;
        ((lo << j) | (hi >> (32 - j)), (hi << j) | (lo >> (32 - j)))
    }
}

/// `compare::silhouette_channel`: (value, raw, control).
pub fn silhouette_value(a: &Tier1, b: &Tier1) -> Option<(i64, i64, i64)> {
    if a.flags & F_SIL == 0 || b.flags & F_SIL == 0 {
        return None;
    }
    let (x, y) = (a.sec("silhouette"), b.sec("silhouette"));
    let mut ra = [0i64; 32];
    let mut rb = [0i64; 32];
    for i in 0..32 {
        ra[i] = x[i] as i64;
        rb[i] = y[i] as i64;
    }
    let v = radial_score_x(&ra, &rb);
    let asp_a = u16::from_le_bytes([x[36], x[37]]) as i64;
    let asp_b = u16::from_le_bytes([y[36], y[37]]) as i64;
    let asp = clamp(asp_a.min(asp_b) * SCALE / asp_a.max(asp_b).max(1), 0, SCALE);
    let fill = clamp(SCALE - (x[38] as i64 - y[38] as i64).abs() * 40, 0, SCALE);
    let oa = (u32::from_le_bytes([x[56], x[57], x[58], x[59]]), u32::from_le_bytes([x[60], x[61], x[62], x[63]]));
    let ob = (u32::from_le_bytes([y[56], y[57], y[58], y[59]]), u32::from_le_bytes([y[60], y[61], y[62], y[63]]));
    let hd = |p: (u32, u32), q: (u32, u32)| -> i64 { ((p.0 ^ q.0).count_ones() + (p.1 ^ q.1).count_ones()) as i64 };
    let occ = clamp(SCALE - hd(oa, ob) * SCALE / 32, 0, SCALE);
    let mut mom = 0i64;
    for i in 32..35 {
        mom += (x[i] as i64 - y[i] as i64).abs();
    }
    let mom_s = clamp(SCALE - mom * 60, 0, SCALE);
    let base = v * asp / SCALE * fill / SCALE;
    let raw = base * occ / SCALE * mom_s / SCALE;
    let mut occ_ctl = SCALE;
    for k in [16u32, 32, 48] {
        let r = rot64(ob.0, ob.1, k);
        occ_ctl = occ_ctl.min(clamp(SCALE - hd(oa, r) * SCALE / 32, 0, SCALE));
    }
    let ctl = base * occ_ctl / SCALE * mom_s / SCALE;
    Some((chance_correct(raw, ctl), raw, ctl))
}

// ----------------------------------------------------------------- local

/// The local channel's upper bound from the exact edge set (§10.2).
///
/// The §10 measurement assigns codes within `hamming_t` and weighs each
/// matched pair by `SCALE / max(burst_a, burst_b)`.  Any matching uses a
/// row at most once and a column at most once, and each pair's weight is at
/// most `SCALE / burst` of EITHER endpoint, so the matched mass is bounded
/// by the smaller of the two endpoint sums over the rows (columns) that
/// have any edge at all; the cardinality by the smaller of the two counts.
/// With the control at its floor of zero, Lift's raw reading is then bounded
/// too, and `chance_correct` cannot lift a reading above its raw value.
pub struct LocalBound {
    pub evidence_hi: i64,
    pub c_hi: i64,
    pub w_hi: i64,
    /// rows and columns carrying at least one edge
    pub rows: usize,
    pub cols: usize,
}

pub fn local_bound(a: &XPrepared, b: &XPrepared, p: &Profile, d0: &mut [u8]) -> LocalBound {
    let (x, y) = (&a.p.bag, &b.p.bag);
    let (n, m) = (x.n, y.n);
    let t = p.hamming_t;
    let mut row_has = [false; 128];
    let mut col_has = [false; 128];
    for i in 0..n {
        let ci = ((x.hi[i] as u64) << 32) | x.lo[i] as u64;
        for j in 0..m {
            let cj = ((y.hi[j] as u64) << 32) | y.lo[j] as u64;
            let d = (ci ^ cj).count_ones() as u8;
            d0[i * m + j] = d;
            if (d as i32) <= t {
                row_has[i] = true;
                col_has[j] = true;
            }
        }
    }
    let (mut rows, mut cols) = (0usize, 0usize);
    let (mut wr, mut wc) = (0i64, 0i64);
    for i in 0..n {
        if row_has[i] {
            rows += 1;
            wr += SCALE / a.burst[i];
        }
    }
    for j in 0..m {
        if col_has[j] {
            cols += 1;
            wc += SCALE / b.burst[j];
        }
    }
    let c_hi = rows.min(cols) as i64;
    let w_hi = wr.min(wc);
    let purity_hi = w_hi * SCALE / (w_hi + SCALE);
    let conf_hi = clamp(c_hi * SCALE / p.confidence_at as i64, 0, SCALE);
    let raw_hi = clamp(purity_hi * conf_hi / SCALE, 0, SCALE);
    let diversity = a.d_side.min(b.d_side);
    let evidence_hi = p.lut_local.eval(raw_hi) * p.lut_diversity.eval(diversity) / SCALE;
    LocalBound { evidence_hi, c_hi, w_hi, rows, cols }
}

/// The exact local channel (comparator 42's), on freshly computed bag
/// distances.
pub fn local_exact(a: &XPrepared, b: &XPrepared, p: &Profile) -> LocalV4 {
    let share = BagShare::new(&a.p.bag, &b.p.bag);
    local_v4_41_shared(&a.p.bag, &b.p.bag, p, &share)
}

// ------------------------------------------------------------- the state

/// The structural axis as the scheduler sees it.
pub struct Structural {
    /// LUT-calibrated channel values, where known
    pub value: [i64; 7],
    pub measurable: [bool; 7],
    pub known: [bool; 7],
    pub local_lo: i64,
    pub local_hi: i64,
    pub local: Option<LocalV4>,
    pub local_bound: Option<LocalBound>,
    /// §10.4 diversity (min of the two sides), exact from the sides
    pub diversity: i64,
    /// raw readings for the report
    pub raw: [i64; 7],
    pub ctl: [i64; 7],
    pub palette_inverted: bool,
}

impl Structural {
    pub fn new(a: &XPrepared, b: &XPrepared, p: &Profile) -> Structural {
        let both = a.meas & b.meas;
        let m = [
            both & M_DCT != 0,
            both & M_LOCAL != 0,
            both & M_SHAPE != 0,
            both & M_TOPOLOGY != 0,
            both & M_RUNS != 0,
            both & M_PALETTE != 0,
            both & M_SILHOUETTE != 0,
        ];
        let diversity = a.d_side.min(b.d_side);
        let local_hi = if m[CH_LOCAL] { p.lut_local.eval(SCALE) * p.lut_diversity.eval(diversity) / SCALE } else { 0 };
        Structural {
            value: [0; 7],
            measurable: m,
            known: [!m[0], !m[1], !m[2], !m[3], !m[4], !m[5], !m[6]],
            local_lo: 0,
            local_hi,
            local: None,
            local_bound: None,
            diversity,
            raw: [0; 7],
            ctl: [0; 7],
            palette_inverted: false,
        }
    }

    /// The weighted structural score's interval under the profile's
    /// weights, unknown channels spanning their whole range.
    pub fn bounds(&self, p: &Profile) -> (i64, i64) {
        let w = |k: usize| -> i64 {
            // profile weights are ordered local, shape, topology, runs, dct,
            // palette, silhouette (SPEC-004 §8)
            let idx = match k {
                CH_LOCAL => 0,
                CH_SHAPE => 1,
                CH_TOPOLOGY => 2,
                CH_RUNS => 3,
                CH_DCT => 4,
                CH_PALETTE => 5,
                _ => 6,
            };
            p.weights[idx] as i64
        };
        let (mut lo, mut hi, mut wt) = (0i64, 0i64, 0i64);
        for k in 0..7 {
            if !self.measurable[k] {
                continue;
            }
            let wk = w(k);
            wt += wk;
            if k == CH_LOCAL {
                if self.known[k] {
                    lo += self.value[k] * wk;
                    hi += self.value[k] * wk;
                } else {
                    lo += self.local_lo * wk;
                    hi += self.local_hi * wk;
                }
            } else if self.known[k] {
                lo += self.value[k] * wk;
                hi += self.value[k] * wk;
            } else {
                hi += SCALE * wk;
            }
        }
        if wt == 0 {
            (0, 0)
        } else {
            (lo / wt, hi / wt)
        }
    }

    pub fn exact(&self) -> bool {
        (0..7).all(|k| self.known[k])
    }

    pub fn secondaries(&self) -> usize {
        (0..7).filter(|&k| k != CH_LOCAL && self.measurable[k]).count()
    }

    /// Evaluate channel `k` (idempotent).
    pub fn compute(&mut self, k: usize, a: &XPrepared, b: &XPrepared, p: &Profile, d0: &mut [u8]) {
        if self.known[k] {
            return;
        }
        let (ta, tb) = (&a.p.t1, &b.p.t1);
        let lut = |name: &str, v: i64| p.lut_channel(name).eval(v);
        match k {
            CH_DCT => {
                if let Some((v, raw, ctl)) = dct_value(ta, tb) {
                    self.value[k] = lut("dct", v);
                    self.raw[k] = raw;
                    self.ctl[k] = ctl;
                }
            }
            CH_SHAPE => {
                if let Some(v) = shape_value(ta, tb) {
                    self.value[k] = lut("shape", v);
                    self.raw[k] = v;
                }
            }
            CH_TOPOLOGY => {
                if let Some((v, raw, ctl)) = topology_value(ta, tb) {
                    self.value[k] = lut("topology", v);
                    self.raw[k] = raw;
                    self.ctl[k] = ctl;
                }
            }
            CH_RUNS => {
                let (v, raw, ctl) = runs_value(ta, tb);
                self.value[k] = lut("runs", v);
                self.raw[k] = raw;
                self.ctl[k] = ctl;
            }
            CH_PALETTE => {
                if let Some((v, raw, ctl, inv)) = palette_value(ta, tb) {
                    self.value[k] = lut("palette", v);
                    self.raw[k] = raw;
                    self.ctl[k] = ctl;
                    self.palette_inverted = inv;
                }
            }
            CH_SILHOUETTE => {
                if let Some((v, raw, ctl)) = silhouette_value(ta, tb) {
                    self.value[k] = lut("silhouette", v);
                    self.raw[k] = raw;
                    self.ctl[k] = ctl;
                }
            }
            CH_LOCAL => {
                let l = local_exact(a, b, p);
                self.value[k] = if l.measurable { l.evidence } else { 0 };
                self.raw[k] = l.lift_raw;
                self.ctl[k] = l.lift_ctl;
                self.local_lo = self.value[k];
                self.local_hi = self.value[k];
                self.local = Some(l);
            }
            _ => {}
        }
        self.known[k] = true;
        let _ = d0;
    }

    /// Tighten the local channel's upper bound from the edge set.
    pub fn bound_local(&mut self, a: &XPrepared, b: &XPrepared, p: &Profile, d0: &mut [u8]) {
        if self.known[CH_LOCAL] || self.local_bound.is_some() || !self.measurable[CH_LOCAL] {
            return;
        }
        let lb = local_bound(a, b, p, d0);
        self.local_hi = self.local_hi.min(lb.evidence_hi);
        self.local_bound = Some(lb);
    }

    /// The lattice's channel array at one structural corner: every unknown
    /// channel at its lowest (`hi` false) or highest (`hi` true) value, the
    /// local channel at its current bound.
    pub fn channels_corner(&self, hi: bool) -> [(&'static str, i64, bool); 7] {
        let mut out = [("", 0i64, false); 7];
        for k in 0..7 {
            let v = if self.known[k] {
                self.value[k]
            } else if k == CH_LOCAL {
                if hi { self.local_hi } else { self.local_lo }
            } else if hi {
                SCALE
            } else {
                0
            };
            out[k] = (CH_NAMES[k], v, self.measurable[k]);
        }
        out
    }

    /// min(coverage_a, coverage_b).coverage of the local channel, once it
    /// is known.
    pub fn coverage_min(&self) -> Option<i64> {
        self.local.as_ref().map(|l| l.coverage_a.coverage.min(l.coverage_b.coverage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compare::{compare_canonical, Reading};

    #[test]
    fn sad64_matches_the_scalar_loops() {
        let mut s = 0x1234_5678_9abc_def1u64;
        let mut r = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for case in 0..2000 {
            let (mut a, mut b) = ([0u8; 32], [0u8; 32]);
            for i in 0..32 {
                a[i] = match case % 3 { 0 => (r() % 256) as u8, 1 => [0u8, 255][(r() % 2) as usize], _ => (i * 8) as u8 };
                b[i] = match case % 4 { 0 => (r() % 256) as u8, 1 => a[(i + 5) & 31], 2 => a[(32 - i) & 31], _ => 255 - a[i] };
            }
            let (mut x, mut y) = ([0i64; 64], [0i64; 64]);
            sad64(&a, &b, &mut x);
            sad64_scalar(&a, &b, &mut y);
            assert_eq!(x, y, "case {case}");
        }
    }
    use crate::config::Config;
    use crate::prepared::{canon_swapped, PairCtx, Prepared};
    use crate::wire::hash;
    use crate::x::profile::XBound;
    use crate::x::testimg::{crop, image, mirror, recolour};

    /// Every rewritten secondary equals the v3 channel it replaces, value,
    /// raw and control, on self, mirrored, cropped, recoloured and
    /// unrelated pairs; the local upper bound holds above the exact value.
    #[test]
    fn secondaries_equal_the_v3_channels_and_the_local_bound_holds() {
        let cfg = Config::default();
        let xb = XBound::shipped();
        let p = xb.base.clone();
        let rot = crate::keypoints::RotCache::new(&crate::keypoints::pattern());
        let (w, h) = (176, 128);
        let base = image(8, w, h);
        let imgs: Vec<(Vec<u8>, usize, usize)> = vec![
            (base.clone(), w, h),
            (mirror(&base, w, h), w, h),
            (crop(&base, w, 20, 15, 110, 90), 110, 90),
            (recolour(&base), w, h),
            (image(777, w, h), w, h),
            (image(31, 64, 200), 64, 200),
        ];
        let sides: Vec<XPrepared> = imgs
            .iter()
            .map(|(px, w, h)| {
                let f = hash(px, *w, *h, &cfg, &rot);
                XPrepared::new(Prepared::new(&f.t1, Some(&f.t2)).unwrap(), &xb)
            })
            .collect();
        let mut d0 = vec![0u8; 128 * 128];
        for i in 0..sides.len() {
            for j in 0..sides.len() {
                let (a, b) = (&sides[i], &sides[j]);
                let swapped = canon_swapped(&a.p, &b.p);
                let (ca, cb) = if swapped { (b, a) } else { (a, b) };
                let mut ctx = PairCtx::new(&ca.p, &cb.p);
                let v3 = compare_canonical(&mut ctx, swapped, &crate::v4::bind(&cfg, &p), Reading::Full);
                let mut s = Structural::new(ca, cb, &p);
                for k in 0..7 {
                    s.compute(k, ca, cb, &p, &mut d0);
                }
                for (k, (name, ch)) in v3.channels.iter().enumerate() {
                    assert_eq!(s.measurable[k], ch.measurable, "{name} measurable {i}x{j}");
                    if !ch.measurable || *name == "local" {
                        continue;
                    }
                    assert_eq!(s.raw[k], ch.raw, "{name} raw {i}x{j}");
                    assert_eq!(s.value[k], p.lut_channel(name).eval(ch.value), "{name} value {i}x{j}");
                    if *name != "shape" {
                        assert_eq!(s.ctl[k], ch.control, "{name} control {i}x{j}");
                    }
                }
                // the local channel: exact equals 42's, and the bound holds
                let share = BagShare::new(&ca.p.bag, &cb.p.bag);
                let l42 = local_v4_41_shared(&ca.p.bag, &cb.p.bag, &p, &share);
                let lx = s.local.as_ref().unwrap();
                assert_eq!((lx.evidence, lx.matches, lx.margin), (l42.evidence, l42.matches, l42.margin));
                if lx.measurable {
                    let lb = local_bound(ca, cb, &p, &mut d0);
                    assert!(lb.evidence_hi >= lx.evidence, "bound {} < exact {} ({i}x{j})", lb.evidence_hi, lx.evidence);
                    assert!(lb.c_hi >= lx.matches);
                    assert!(lb.w_hi >= lx.w);
                    assert_eq!(s.diversity, l42.diversity);
                }
                // and the fully known structural score is comparator 42's
                let r42 = crate::v42::compare_v42_lean(&ca.p, &cb.p, &cfg, &p);
                let (lo, hi) = s.bounds(&p);
                assert_eq!(lo, hi);
                if r42.base.verdict != "Identical" {
                    assert_eq!(lo, r42.base.structural, "structural {i}x{j}");
                }
            }
        }
    }
}
