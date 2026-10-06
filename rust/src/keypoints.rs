//! The integer keypoint pipeline (SPEC-003 §8).
//!
//! v2's keypoint half was the better detector — 0.82 recall against 0.45 — and
//! could not be stored, indexed or verified by a second party, because every
//! step of it was floating point.  Every one of those floats is replaceable:
//!
//! * `atan2`  -> argmax over 64 integer dot products (§8.4)
//! * RANSAC   -> Hough vote + closed-form fixed-point refit (§9.4)
//! * mirror   -> a bit permutation of the stored descriptor (§8.6)
//!
//! Nothing below uses a float, a PRNG, or a transcendental.

use crate::config::{clamp, idiv, Config, KP_SELECT_QUALITY, MAX_KP_COUNT};
use crate::front::Indexed;
use crate::simd;
use crate::tables::*;

pub const PATCH_R: i64 = 15;
/// A rotated pattern point reaches `ceil(PATCH_R * sqrt2)` from the keypoint,
/// and each sample is a 5x5 box on top of that.  v2 had no margin at all and
/// relied on the box window clamping at the edge, which makes the sums
/// non-comparable — three of 3588 mirrored descriptors disagreed until this
/// was 24.
pub const KP_MARGIN: i64 = 24;
pub const N_BITS: usize = 256;
pub const N_PAIRS: usize = 128;
pub const FAST_T: i32 = 18;
pub const NMS_R: i64 = 4;
/// SPEC-004.2 §2 — 512, not 256.  The per-level cap and the Tier-2 budget are
/// the same number on purpose: a level that could fill the whole budget on its
/// own is a level whose keypoints the selector should be allowed to weigh
/// against every other level's, rather than one the detector truncated first.
pub const KP_PER_LEVEL: usize = MAX_KP_COUNT;

/// SPEC-004.2 §3 — the selection score, in hundredths so the four weights are
/// literally the percentages the specification states.
pub const Q_W_STRENGTH: i64 = 40;
pub const Q_W_SPATIAL: i64 = 25;
pub const Q_W_SCALE: i64 = 20;
pub const Q_W_DESC: i64 = 15;
/// Hamming distance at which descriptor novelty saturates.  Below it a
/// candidate is repeating a structure the selection already holds.
pub const DESC_NOVEL_AT: i64 = 16;
/// `SCALE_Q / DESC_NOVEL_AT`, and it is EXACT: 10000 / 16 = 625.  The
/// descriptor-novelty term is therefore a multiply, not a division that
/// happens to round the same way.
pub const DESC_NOVEL_STEP: i32 = 625;
/// Candidates considered by the selector, as a multiple of the budget.  The
/// pool is the strongest `want * SEL_POOL_MULT` under the pooled order, which
/// bounds the selector at `want x pool` operations without ever discarding a
/// candidate that could have won on strength alone.
pub const SEL_POOL_MULT: usize = 4;
/// The spatial grid the selection counts occupancy over.
pub const SEL_GRID: i64 = 8;
/// The selector's own currency, the same 0..10000 every channel reports in.
pub const SCALE_Q: i64 = 10_000;
/// The smallest level that still HAS an interior.  v2's floor of 96 was set for
/// a 119-736 px corpus and starves small pixel art — a 72 px sprite got ONE
/// scale, so it could not be matched against the same sprite inside a 220 px
/// scene, which is the collage case the whole stage exists for.
pub const LEVEL_MIN: i64 = 2 * KP_MARGIN + 4;

#[derive(Clone)]
pub struct Keypoint {
    pub desc: [u32; 8],
    pub x: i32,
    pub y: i32,
    pub level: u8,
    pub sec: u8,
    pub s: u16,
}

/// Symmetric arithmetic shift for a Q10 divide: `sh10(-v) == -sh10(v)` exactly.
/// The reflection identity in §8.6 depends on it.
#[inline(always)]
fn sh10(v: i64) -> i64 {
    if v >= 0 {
        (v + 512) >> 10
    } else {
        -((-v + 512) >> 10)
    }
}

/// The reflection-closed BRIEF pattern (SPEC-003 §8.6).
///
/// 128 pairs are drawn freely from an integer LCG.  Bits 128..255 use the same
/// pairs with y NEGATED.  Mirroring the patch sends the intensity centroid to
/// `theta -> pi - theta`, so `sector -> 32 - sector`, and
///
/// ```text
///     F . rotate(p, 32-k) == rotate(G.p, k),   F = diag(-1,1), G = diag(1,-1)
/// ```
///
/// Sampling the mirrored image at offset `u` equals sampling the original at
/// `F.u`, so bit *i* of the mirrored keypoint is exactly the bit the ORIGINAL
/// would produce with pattern `G.P_i` — i.e. bit *i+128*.  The mirrored
/// descriptor is therefore the stored one with its two halves exchanged:
/// mirror invariance for ZERO stored bytes.
///
/// Note the correction against SPEC-003 as first written: the second half is
/// the Y-mirrored pattern, not the x-mirrored one.  Verified 2949/2949.
pub fn pattern() -> Vec<i32> {
    let m: i64 = 0x7fff_ffff;
    let mut s: i32 = 0x1234567;
    // exact 32-bit, matching Math.imul on the JavaScript side
    let mut u = || -> i64 {
        s = s.wrapping_mul(1103515245).wrapping_add(12345) & 0x7fff_ffff;
        s as i64
    };
    let mut g = || -> i32 {
        // JS evaluates `(3 * M) >> 1` by coercing 3*M to int32 FIRST, which
        // wraps 6442450941 to 2147483645 before the shift.  Reproduce the wrap
        // rather than the arithmetic, or the two patterns diverge.
        let half3m = (((3i64 * m) as i32) >> 1) as i64;
        let c = u() + u() + u() - half3m;
        let v = if c >= 0 {
            idiv(c * 21 + m, 2 * m)
        } else {
            -idiv(-c * 21 + m, 2 * m)
        };
        clamp(v, -PATCH_R, PATCH_R) as i32
    };
    let mut p = vec![0i32; N_BITS * 4];
    for i in 0..N_PAIRS {
        let (ax, ay, bx, by) = (g(), g(), g(), g());
        let o = i * 4;
        p[o] = ax;
        p[o + 1] = ay;
        p[o + 2] = bx;
        p[o + 3] = by;
        let j = (i + N_PAIRS) * 4; // G . P_i  =  y negated
        p[j] = ax;
        p[j + 1] = -ay;
        p[j + 2] = bx;
        p[j + 3] = -by;
    }
    p
}

/// Rotated pattern per sector, built once.  Recomputing 1024 multiply-shifts
/// per keypoint is the single largest avoidable cost in the hash.
pub struct RotCache {
    tables: Vec<Vec<i64>>,
}
impl RotCache {
    pub fn new(pat: &[i32]) -> Self {
        let mut tables = Vec::with_capacity(64);
        for k in 0..64usize {
            let (c, s) = (RC10[k] as i64, RS10[k] as i64);
            let mut t = vec![0i64; N_BITS * 4];
            for i in 0..N_BITS {
                let o = i * 4;
                let (pax, pay) = (pat[o] as i64, pat[o + 1] as i64);
                let (pbx, pby) = (pat[o + 2] as i64, pat[o + 3] as i64);
                t[o] = sh10(pax * c - pay * s);
                t[o + 1] = sh10(pax * s + pay * c);
                t[o + 2] = sh10(pbx * c - pby * s);
                t[o + 3] = sh10(pbx * s + pby * c);
            }
            tables.push(t);
        }
        RotCache { tables }
    }
}

struct Level {
    d: Vec<u8>,
    op: Vec<u8>,
    w: usize,
    h: usize,
}

/// Summed-area table over a byte map, in wrapping `u32` arithmetic.
///
/// Every box any caller takes sums to less than 2^32 — at most 255 per pixel
/// over at most 2^24 pixels (the §16 limit) — so the four-corner difference
/// taken modulo 2^32 IS the exact sum.  Half the memory of the `i64` table it
/// replaces, and rows are walked as slices, so no index is bounds-checked
/// twice.
fn integral(map: &[u8], w: usize, h: usize) -> Vec<u32> {
    let ws = w + 1;
    let mut s = vec![0u32; ws * (h + 1)];
    for y in 0..h {
        let (prev, cur) = s.split_at_mut((y + 1) * ws);
        let prev = &prev[y * ws..y * ws + ws];
        let cur = &mut cur[..ws];
        let src = &map[y * w..y * w + w];
        let mut row = 0u32;
        for ((c, &p), &v) in cur[1..].iter_mut().zip(prev[1..].iter()).zip(src.iter()) {
            row = row.wrapping_add(v as u32);
            *c = p.wrapping_add(row);
        }
    }
    s
}

/// The same table over `lum` restricted to opaque pixels — the numerator of
/// every pyramid block's mean.
fn integral_opaque(lum: &[u8], op: &[u8], w: usize, h: usize) -> Vec<u32> {
    let ws = w + 1;
    let mut s = vec![0u32; ws * (h + 1)];
    for y in 0..h {
        let (prev, cur) = s.split_at_mut((y + 1) * ws);
        let prev = &prev[y * ws..y * ws + ws];
        let cur = &mut cur[..ws];
        let (l, o) = (&lum[y * w..y * w + w], &op[y * w..y * w + w]);
        let mut row = 0u32;
        for (((c, &p), &lv), &ov) in cur[1..].iter_mut().zip(prev[1..].iter()).zip(l.iter()).zip(o.iter()) {
            row = row.wrapping_add(if ov != 0 { lv as u32 } else { 0 });
            *c = p.wrapping_add(row);
        }
    }
    s
}

/// Sum over the half-open box [x0, x1) x [y0, y1) of a table of row stride `ws`.
#[inline(always)]
fn box_sum(s: &[u32], ws: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> u32 {
    s[y1 * ws + x1]
        .wrapping_sub(s[y0 * ws + x1])
        .wrapping_sub(s[y1 * ws + x0])
        .wrapping_add(s[y0 * ws + x0])
}

#[inline(always)]
fn box_win(s: &[u32], w: usize, cx: i64, cy: i64, r: i64) -> i64 {
    let ws = (w + 1) as i64;
    let (x0, y0, x1, y1) = (cx - r, cy - r, cx + r + 1, cy + r + 1);
    s[(y1 * ws + x1) as usize]
        .wrapping_sub(s[(y0 * ws + x1) as usize])
        .wrapping_sub(s[(y1 * ws + x0) as usize])
        .wrapping_add(s[(y0 * ws + x0) as usize]) as i64
}

/// Levels indexed by `max(w, h)`, NOT by width.  v2 indexed by width, which
/// gave a 736x352 work seven scales and a 119x193 work one — a scale-coverage
/// asymmetry that depended on aspect ratio rather than size.
pub fn pyramid_dims(max_dim: i64) -> Vec<i64> {
    let mut out = vec![max_dim]; // level 0 ALWAYS exists
    let mut l = idiv(max_dim * 10 + 6, 13);
    while l >= LEVEL_MIN {
        out.push(l);
        l = idiv(l * 10 + 6, 13);
    }
    out
}

/// A keypoint's descriptor covers a 31 px patch AT ITS OWN LEVEL, so the
/// feature's size in normalised units is proportional to `1 / L_k`.  The scale
/// ratio between two keypoints is therefore `L_kA / L_kB` — recoverable from
/// the stored level index plus the work's maxDim, both of which are on the
/// wire.  Deriving scale from the index difference alone is wrong the moment
/// two works differ in size.
pub fn level_dim(max_dim: i64, k: usize) -> i64 {
    let mut l = max_dim;
    for _ in 0..k {
        l = idiv(l * 10 + 6, 13);
    }
    l.max(1)
}

/// One pyramid level: the mean opaque luminance of each block, `fill` where a
/// block has none, and the 75% opacity flag.
///
/// The blocks are the reference's — `[i·w/lw, max((i+1)·w/lw, x0+1))` per axis
/// — and their sums come from two summed-area tables over the source image
/// (`sl` = opaque luminance, `so` = opaque count) shared by every level, so a
/// level costs four lookups per output pixel instead of a walk over every
/// source pixel under it: nine levels used to re-read the image nine times.
/// Level 0 samples one pixel per block, which is a copy.
fn build_level(
    sl: &[u32],
    so: &[u32],
    lum: &[u8],
    op: &[u8],
    (w, h): (usize, usize),
    (lw, lh): (usize, usize),
    fill: u8,
) -> Level {
    let mut d = vec![0u8; lw * lh];
    let mut o = vec![0u8; lw * lh];
    if lw == w && lh == h {
        for i in 0..w * h {
            let opaque = op[i] != 0;
            d[i] = if opaque { lum[i] } else { fill };
            o[i] = opaque as u8;
        }
        return Level { d, op: o, w: lw, h: lh };
    }
    let ws = w + 1;
    let xs: Vec<(usize, usize)> = (0..lw)
        .map(|i| {
            let x0 = idiv(i as i64 * w as i64, lw as i64) as usize;
            let x1 = (idiv((i as i64 + 1) * w as i64, lw as i64) as usize).max(x0 + 1).min(w);
            (x0, x1)
        })
        .collect();
    for j in 0..lh {
        let y0 = idiv(j as i64 * h as i64, lh as i64) as usize;
        let y1 = (idiv((j as i64 + 1) * h as i64, lh as i64) as usize).max(y0 + 1).min(h);
        let (dr, or) = (&mut d[j * lw..j * lw + lw], &mut o[j * lw..j * lw + lw]);
        for i in 0..lw {
            let (x0, x1) = xs[i];
            let sum = box_sum(sl, ws, x0, y0, x1, y1);
            let cnt = box_sum(so, ws, x0, y0, x1, y1);
            let tot = ((x1 - x0) * (y1 - y0)) as u64;
            dr[i] = if cnt > 0 { (sum / cnt) as u8 } else { fill };
            or[i] = (cnt as u64 * 4 >= tot * 3) as u8;
        }
    }
    Level { d, op: o, w: lw, h: lh }
}

/// FAST-9, unchanged from v2 and already integer.  What is new is the border
/// margin and the 75% opacity floor — the same rule the local-fingerprint
/// windows use.  Without it a transparent background becomes a hard black edge
/// and the detector keys on the SILHOUETTE, which is what v2's keypoint half
/// did and why its recall and the structural half's could not be compared.
fn fast9(lv: &Level) -> Vec<(i64, i64, i64)> {
    let (w, h) = (lv.w, lv.h);
    let mut kps = Vec::new();
    let lo = KP_MARGIN;
    let hix = w as i64 - 1 - KP_MARGIN;
    let hiy = h as i64 - 1 - KP_MARGIN;
    if hix <= lo || hiy <= lo {
        return kps;
    }
    let so = integral(&lv.op, w, h);
    let area = (2 * PATCH_R + 1) * (2 * PATCH_R + 1);
    let mut circ = [0isize; 16];
    for k in 0..16 {
        circ[k] = CIRC[k].1 as isize * w as isize + CIRC[k].0 as isize;
    }
    // Sixteen centres per step.  The pre-test and the arc test are decided in
    // vector lanes (`simd::fast9_x16`); the opacity floor and the score run
    // only for the lanes that pass both, which is a few percent of pixels even
    // on dithered art where half of them pass the pre-test.  Raster order is
    // kept, and a lane past `hix` is masked off — its reads stay in bounds
    // because the margin is 24 and the circle reaches 3.
    for y in lo..=hiy {
        let row = y as usize * w;
        let mut x = lo;
        while x <= hix {
            let lanes = (hix - x + 1).min(16) as u32;
            // SAFETY: every read is within rows y-3..=y+3 and columns
            // x-3..=x+18 <= w-7, inside the level.
            let mut m = unsafe { simd::fast9_x16(&lv.d, row + x as usize, &circ) } as u32;
            if lanes < 16 {
                m &= (1u32 << lanes) - 1;
            }
            while m != 0 {
                let j = m.trailing_zeros() as i64;
                m &= m - 1;
                let xx = x + j;
                if box_win(&so, w, xx, y, PATCH_R) * 4 < area * 3 {
                    continue;
                }
                let c = row + xx as usize;
                let p = lv.d[c] as i32;
                let mut score = 0i64;
                for k in 0..16 {
                    score += (lv.d[(c as isize + circ[k]) as usize] as i32 - p).abs() as i64;
                }
                kps.push((xx, y, score));
            }
            x += 16;
        }
    }
    kps
}

/// True-radius NMS over a total order.  v2 used a grid-approximate suppression
/// whose effective radius varied between 4 and 12 px depending on where a
/// keypoint fell inside its cell — so a 2 px shift could change which of two
/// nearby corners survived.  For an algorithm whose entire premise is finding
/// the SAME points twice, that is a real defect.
fn nms(mut kps: Vec<(i64, i64, i64)>, r: i64) -> Vec<(i64, i64, i64)> {
    kps.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)).then(a.0.cmp(&b.0)));
    let r2 = r * r;
    let mut kept: Vec<(i64, i64, i64)> = Vec::new();
    if kps.is_empty() {
        return kept;
    }
    // Bucket the KEPT points on a grid of cell size r.  `dx*dx + dy*dy <= r2`
    // forces |dx| <= r and |dy| <= r, so a suppressor can only live in the
    // candidate's own cell or one of its eight neighbours — the 3x3 probe sees
    // every point the full scan saw, and the scan runs in the identical sorted
    // order with the identical strict test, so it keeps the identical set.
    // The v2 defect this stage's comment warns about is not reintroduced: the
    // grid here narrows WHERE to look for an exact test, it never IS the test.
    //
    // The full scan was `candidates x kept`, and 4.2 doubled `kept` to 512 —
    // on a busy level that was tens of millions of distance checks and most of
    // the detection stage's time.
    let (mut maxx, mut maxy) = (0i64, 0i64);
    for k in kps.iter() {
        maxx = maxx.max(k.0);
        maxy = maxy.max(k.1);
    }
    let gw = (maxx / r + 1) as usize;
    let gh = (maxy / r + 1) as usize;
    let mut grid: Vec<Vec<u32>> = vec![Vec::new(); gw * gh];
    for k in kps {
        if kept.len() >= KP_PER_LEVEL {
            break;
        }
        let (cx, cy) = ((k.0 / r) as usize, (k.1 / r) as usize);
        let mut ok = true;
        'probe: for ny in cy.saturating_sub(1)..=(cy + 1).min(gh - 1) {
            for nx in cx.saturating_sub(1)..=(cx + 1).min(gw - 1) {
                for &ji in grid[ny * gw + nx].iter() {
                    let j = &kept[ji as usize];
                    let dx = j.0 - k.0;
                    let dy = j.1 - k.1;
                    if dx * dx + dy * dy <= r2 {
                        ok = false;
                        break 'probe;
                    }
                }
            }
        }
        if ok {
            grid[cy * gw + cx].push(kept.len() as u32);
            kept.push(k);
        }
    }
    kept
}

/// The r=7 disc, flattened once.
fn disc7() -> Vec<(i64, i64)> {
    let mut t = Vec::new();
    for dy in -7i64..=7 {
        for dx in -7i64..=7 {
            if dx * dx + dy * dy <= 49 {
                t.push((dx, dy));
            }
        }
    }
    t
}

/// Orientation as the argmax over `m10*cos(phi_k) + m01*sin(phi_k)`, which IS
/// round-to-nearest by construction — and because the tables satisfy
/// `COS64[32-k] = -COS64[k]` and `SIN64[32-k] = SIN64[k]`, it satisfies
/// `sector(-m10, m01) == (32 - sector(m10, m01)) & 63` exactly.
///
/// Returns -1 when the orientation is ambiguous.  A tie is the ONE case where
/// the mirror identity fails: the two tied sectors are adjacent and "lowest
/// index" is not preserved by `k -> 32-k`.  Rather than invent an asymmetric
/// tie-break that quietly breaks mirroring, REFUSE the keypoint — an
/// orientation this ambiguous produces a descriptor that is not repeatable
/// anyway.  Measured rate on real content: under 1%.
fn orient_sector(bf3: &[u16], w: usize, x: i64, y: i64, disc: &[(i64, i64)]) -> i32 {
    let (mut m10, mut m01) = (0i64, 0i64);
    let base = y as isize * w as isize + x as isize;
    for &(dx, dy) in disc {
        // 3x3 SUM, not mean, read from the level's box-filtered copy
        let v = bf3[(base + dy as isize * w as isize + dx as isize) as usize] as i64;
        m10 += dx * v;
        m01 += dy * v;
    }
    if m10 == 0 && m01 == 0 {
        return -1;
    }
    let (mut best_k, mut best_v, mut tied) = (0i32, i64::MIN, 0u32);
    for k in 0..64usize {
        let dot = m10 * COS64[k] as i64 + m01 * SIN64[k] as i64;
        if dot > best_v {
            best_v = dot;
            best_k = k as i32;
            tied = 1;
        } else if dot == best_v {
            tied += 1;
        }
    }
    if tied > 1 {
        -1
    } else {
        best_k
    }
}

/// The 256 test pairs of one orientation sector, as index offsets into a
/// level of width `w`: `[a, b]` with each point at `dy * w + dx`.
type PairOffsets = [[i32; 2]; N_BITS];

fn pair_offsets(rot: &[i64], w: usize) -> Box<PairOffsets> {
    let mut o = Box::new([[0i32; 2]; N_BITS]);
    for i in 0..N_BITS {
        let q = i * 4;
        o[i] = [
            (rot[q + 1] * w as i64 + rot[q]) as i32,
            (rot[q + 3] * w as i64 + rot[q + 2]) as i32,
        ];
    }
    o
}

/// 5x5 box SUMS, not means: the divisor is constant so it cannot change the
/// comparison, and sums stay exact.  The box average is what makes BRIEF work
/// on dithered pixel art at all — a raw two-pixel test measures the dither,
/// not the drawing.  Ties resolve to 0, specified.
///
/// The sums come from the level's box-filtered copy, so a bit is two loads and
/// a compare; the reference took eight summed-area lookups and the index
/// arithmetic for them per bit.
fn describe(bf5: &[u16], w: usize, x: i64, y: i64, pairs: &PairOffsets) -> [u32; 8] {
    let mut bits = [0u32; 8];
    let base = y as isize * w as isize + x as isize;
    for (i, pr) in pairs.iter().enumerate() {
        let sa = bf5[(base + pr[0] as isize) as usize];
        let sb = bf5[(base + pr[1] as isize) as usize];
        bits[i >> 5] |= ((sa < sb) as u32) << (i & 31);
    }
    bits
}

/// Exact (2r+1)² box sums of a level, at every pixel whose box fits inside it
/// (elsewhere 0, and never read: keypoints keep a 24-pixel margin, the pattern
/// reaches 21 and the orientation disc 7).  Two sliding passes.
fn box_filter(d: &[u8], w: usize, h: usize, r: usize) -> Vec<u16> {
    let k = 2 * r + 1;
    let mut out = vec![0u16; w * h];
    if w < k || h < k {
        return out;
    }
    let mut hs = vec![0u16; w * h];
    for y in 0..h {
        let row = &d[y * w..(y + 1) * w];
        let o = &mut hs[y * w..(y + 1) * w];
        let mut acc: u32 = row[..k].iter().map(|&v| v as u32).sum();
        o[r] = acc as u16;
        for x in r + 1..w - r {
            acc = acc + row[x + r] as u32 - row[x - r - 1] as u32;
            o[x] = acc as u16;
        }
    }
    let mut col = vec![0u32; w];
    for y in 0..k {
        for (c, &v) in col.iter_mut().zip(hs[y * w..(y + 1) * w].iter()) {
            *c += v as u32;
        }
    }
    for (o, &c) in out[r * w..(r + 1) * w].iter_mut().zip(col.iter()) {
        *o = c as u16;
    }
    for y in r + 1..h - r {
        let (add, sub) = (&hs[(y + r) * w..(y + r + 1) * w], &hs[(y - r - 1) * w..(y - r) * w]);
        let o = &mut out[y * w..(y + 1) * w];
        for x in 0..w {
            col[x] = col[x] + add[x] as u32 - sub[x] as u32;
            o[x] = col[x] as u16;
        }
    }
    out
}

/// Descriptor of the horizontally mirrored patch: exchange the two halves.
pub fn mirror_desc(d: &[u32; 8]) -> [u32; 8] {
    [d[4], d[5], d[6], d[7], d[0], d[1], d[2], d[3]]
}


/// The 8x8 cell a keypoint falls in, in the shared 16-bit frame.
#[inline]
fn sel_cell(k: &Keypoint) -> usize {
    (idiv(k.y as i64 * SEL_GRID, 65536) * SEL_GRID + idiv(k.x as i64 * SEL_GRID, 65536)) as usize
}

/// PAPH 4.1 selection, kept verbatim so a 4.1 wire can still be reproduced.
///
/// Spread the budget over an 8x8 spatial grid before capping.  Taking the
/// globally strongest `want` keypoints looks fair and is not: a busy host
/// out-scores a pasted figure and crowds every one of its keypoints out of the
/// budget, so the collage case the geometric stage EXISTS for is exactly the
/// one the cap silences.
pub fn select_grid(all: &[Keypoint], want: usize) -> Vec<Keypoint> {
    let mut cells: std::collections::BTreeMap<i64, Vec<usize>> = std::collections::BTreeMap::new();
    for (i, k) in all.iter().enumerate() {
        let gk = idiv(k.y as i64 * 8, 65536) * 8 + idiv(k.x as i64 * 8, 65536);
        cells.entry(gk).or_default().push(i);
    }
    let mut picked: Vec<usize> = Vec::with_capacity(want);
    let mut round = 0usize;
    loop {
        let mut took = 0;
        for bucket in cells.values() {
            if picked.len() >= want {
                break;
            }
            if round < bucket.len() {
                picked.push(bucket[round]);
                took += 1;
            }
        }
        if took == 0 || picked.len() >= want {
            break;
        }
        round += 1;
    }
    picked.into_iter().map(|i| all[i].clone()).collect()
}

/// SPEC-004.2 §3 — selection by quality rather than by strength.
///
/// 512 keypoints are not 512 pieces of evidence.  A brick wall yields two
/// hundred keypoints describing one local texture; a round-robin over a spatial
/// grid spreads them out but does nothing about the fact that they say the same
/// thing.  The score therefore pays for four different kinds of being new:
///
/// ```text
///     Q = 40 strength + 25 spatial novelty + 20 scale novelty + 15 descriptor novelty
/// ```
///
/// all in the 0..10000 currency, all integer, greedy, ties to the strongest
/// candidate under the pooled order.  Spatial and scale novelty decay as
/// `1/(1+n)` in the number already taken from that cell or level, so the first
/// keypoint in an empty region is worth eight of the eighth in a crowded one;
/// descriptor novelty saturates at Hamming `DESC_NOVEL_AT` from the nearest
/// already-selected descriptor, so a near-duplicate of something held scores
/// zero on that term however strong it is.
pub fn select_quality(all: &[Keypoint], want: usize) -> Vec<Keypoint> {
    let n = all.len().min(want * SEL_POOL_MULT);
    let pool = &all[..n];
    let smax = pool.iter().map(|k| k.s as i64).max().unwrap_or(1).max(1);

    // Structure of arrays.  The selector touches `desc` n times per round and
    // nothing else in the Keypoint, so walking a contiguous descriptor block
    // beats striding a 40-byte record: at want=512 that is a megabyte of
    // pointer-chasing removed from the hash's hottest loop.
    let mut dpack: Vec<[u64; 4]> = Vec::with_capacity(n);
    let mut q_strength = vec![0i32; n];
    let mut cell = vec![0u16; n];
    let mut level = vec![0u16; n];
    for i in 0..n {
        dpack.push(pack4(&pool[i].desc));
        q_strength[i] = clamp(pool[i].s as i64 * SCALE_Q / smax, 0, SCALE_Q) as i32;
        cell[i] = sel_cell(&pool[i]) as u16;
        level[i] = pool[i].level as u16;
    }

    let mut cellc = vec![0u32; (SEL_GRID * SEL_GRID) as usize];
    let mut levc = vec![0u32; 256];
    let mut dmin = vec![N_BITS as i32; n];
    let mut taken = vec![false; n];
    let mut picked: Vec<usize> = Vec::with_capacity(want);

    // Both novelty terms are `SCALE_Q / (1 + count)` — a division by a small
    // integer, tabulated.  `q_desc` divides by DESC_NOVEL_AT = 16, which
    // divides 10000 exactly, so it is a multiply by 625 and not an
    // approximation of one.
    #[inline(always)]
    fn score(i: usize, qs: &[i32], cell: &[u16], level: &[u16], cellc: &[u32], levc: &[u32], dmin: &[i32]) -> i32 {
        let q_spatial = recip(cellc[cell[i] as usize]);
        let q_scale = recip(levc[level[i] as usize]);
        let q_desc = (dmin[i] * DESC_NOVEL_STEP).min(SCALE_Q as i32);
        (Q_W_STRENGTH as i32 * qs[i]
            + Q_W_SPATIAL as i32 * q_spatial
            + Q_W_SCALE as i32 * q_scale
            + Q_W_DESC as i32 * q_desc)
            / 100
    }

    // Lazy greedy, exact.  Every candidate's score only ever FALLS: the cell
    // and level counts only rise and `dmin` only shrinks, so a score computed
    // in an earlier round is an upper bound on its value now.  The heap is
    // ordered by (stale score desc, index asc) — the reference's own order,
    // "strictly greater, ties to the lower index" — and its top is popped and
    // re-scored: if the fresh score equals the stale one, no other candidate
    // can beat it (each is bounded by its stale entry, which the heap ranks no
    // higher), so it is exactly the candidate the full scan would have picked.
    // Otherwise it goes back with its fresh score.  The reference re-scored
    // all `n` candidates every round; this re-scores a handful.
    let mut heap: std::collections::BinaryHeap<(i32, std::cmp::Reverse<usize>)> = (0..n)
        .map(|i| (score(i, &q_strength, &cell, &level, &cellc, &levc, &dmin), std::cmp::Reverse(i)))
        .collect();
    while picked.len() < want {
        let mut best_i = usize::MAX;
        while let Some((stale, std::cmp::Reverse(i))) = heap.pop() {
            let q = score(i, &q_strength, &cell, &level, &cellc, &levc, &dmin);
            if q == stale {
                best_i = i;
                break;
            }
            heap.push((q, std::cmp::Reverse(i)));
        }
        if best_i == usize::MAX {
            break;
        }
        taken[best_i] = true;
        cellc[cell[best_i] as usize] += 1;
        levc[level[best_i] as usize] += 1;
        picked.push(best_i);
        hamming_min_into(&dpack[best_i], &dpack, &taken, &mut dmin);
    }
    picked.into_iter().map(|i| pool[i].clone()).collect()
}

/// `SCALE_Q / (1 + k)`, tabulated.  `k` is a count of already-selected
/// keypoints in one cell or one level, so it cannot exceed the budget.
#[inline(always)]
fn recip(k: u32) -> i32 {
    const N: usize = MAX_KP_COUNT + 1;
    static TABLE: [i32; N] = {
        let mut t = [0i32; N];
        let mut i = 0;
        while i < N {
            t[i] = (SCALE_Q / (1 + i as i64)) as i32;
            i += 1;
        }
        t
    };
    let k = k as usize;
    if k < N {
        TABLE[k]
    } else {
        (SCALE_Q / (1 + k as i64)) as i32
    }
}

#[inline(always)]
pub fn pack4(d: &[u32; 8]) -> [u64; 4] {
    [
        (d[0] as u64) | ((d[1] as u64) << 32),
        (d[2] as u64) | ((d[3] as u64) << 32),
        (d[4] as u64) | ((d[5] as u64) << 32),
        (d[6] as u64) | ((d[7] as u64) << 32),
    ]
}

/// `dmin[j] = min(dmin[j], hamming(q, pack[j]))` over every candidate not yet
/// taken.
///
/// Scalar `popcnt` on every target.  The WebAssembly build used to count
/// bytes with `i8x16.popcnt` and fold the lanes; in V8 that is slower than
/// four scalar `i64.popcnt`, provided the loads stay scalar — which is what
/// `simd::word` is for.
#[inline]
pub(crate) fn hamming_min_into(q: &[u64; 4], pack: &[[u64; 4]], taken: &[bool], dmin: &mut [i32]) {
    use crate::simd::word;
    for j in 0..pack.len() {
        if taken[j] {
            continue;
        }
        let p = &pack[j];
        let d = ((q[0] ^ word(&p[0])).count_ones()
            + (q[1] ^ word(&p[1])).count_ones()
            + (q[2] ^ word(&p[2])).count_ones()
            + (q[3] ^ word(&p[3])).count_ones()) as i32;
        if d < dmin[j] {
            dmin[j] = d;
        }
    }
}

/// The plain reference.
#[cfg(test)]
pub(crate) fn hamming_min_into_scalar(
    q: &[u64; 4],
    pack: &[[u64; 4]],
    taken: &[bool],
    dmin: &mut [i32],
) {
    for j in 0..pack.len() {
        if taken[j] {
            continue;
        }
        let p = &pack[j];
        let d = ((q[0] ^ p[0]).count_ones()
            + (q[1] ^ p[1]).count_ones()
            + (q[2] ^ p[2]).count_ones()
            + (q[3] ^ p[3]).count_ones()) as i32;
        if d < dmin[j] {
            dmin[j] = d;
        }
    }
}

pub struct KpOut {
    pub list: Vec<Keypoint>,
    pub max_dim: i64,
    pub xmax: i32,
}

pub fn keypoints(im: &Indexed, cfg: &Config, rot: &RotCache) -> KpOut {
    let (w, h) = (im.w, im.h);
    let n = w * h;
    let mut lum = vec![0u8; n];
    let mut op = vec![0u8; n];
    let mut lums: Vec<u8> = Vec::with_capacity(n);
    for i in 0..n {
        if im.idx[i] < 0 {
            op[i] = 0;
        } else {
            op[i] = 1;
            lum[i] = im.pal[im.idx[i] as usize].lum as u8;
            lums.push(lum[i]);
        }
    }
    lums.sort_unstable();
    let fill = if lums.is_empty() { 128 } else { lums[lums.len() >> 1] };
    for i in 0..n {
        if op[i] == 0 {
            lum[i] = fill;
        }
    }

    let max_dim = w.max(h) as i64;
    let dims = pyramid_dims(max_dim);
    let disc = disc7();
    let mut all: Vec<Keypoint> = Vec::new();
    // the two tables every level's block means come from (level 0 is a copy
    // and needs neither; an image too small for any level never builds them)
    let tables = if dims.len() > 1 && w.min(h) as i64 >= 2 * KP_MARGIN + 4 {
        Some((integral_opaque(&lum, &op, w, h), integral(&op, w, h)))
    } else {
        None
    };
    let empty: Vec<u32> = Vec::new();

    for (li, &l) in dims.iter().enumerate() {
        let lw = (idiv(w as i64 * l, max_dim)).max(1) as usize;
        let lh = (idiv(h as i64 * l, max_dim)).max(1) as usize;
        if (lw as i64) < 2 * KP_MARGIN + 4 || (lh as i64) < 2 * KP_MARGIN + 4 {
            continue;
        }
        let (sl, so) = match &tables {
            Some((a, b)) => (a.as_slice(), b.as_slice()),
            None => (empty.as_slice(), empty.as_slice()),
        };
        let lv = build_level(sl, so, &lum, &op, (w, h), (lw, lh), fill);
        let raw = fast9(&lv);
        if raw.is_empty() {
            continue;
        }
        let kept = nms(raw, NMS_R);
        // Normalise strength by the LEVEL's own median before pooling.  v2
        // ranked pooled keypoints by raw FAST score, comparing scores computed
        // at different resolutions — downsampling smooths, so coarse levels
        // were systematically starved.
        let mut sc: Vec<i64> = kept.iter().map(|k| k.2).collect();
        sc.sort_unstable();
        let med = sc[sc.len() >> 1].max(1);
        let bf3 = box_filter(&lv.d, lw, lh, 1);
        let bf5 = box_filter(&lv.d, lw, lh, 2);
        let mut sector_pairs: Vec<Option<Box<PairOffsets>>> = (0..64).map(|_| None).collect();
        for &(kx, ky, ks) in kept.iter() {
            let sec = orient_sector(&bf3, lw, kx, ky, &disc);
            if sec < 0 {
                continue; // ambiguous orientation, refused
            }
            let pairs = sector_pairs[sec as usize]
                .get_or_insert_with(|| pair_offsets(&rot.tables[sec as usize], lw));
            let desc = describe(&bf5, lw, kx, ky, pairs);
            all.push(Keypoint {
                desc,
                level: li as u8,
                sec: sec as u8,
                x: clamp(idiv(idiv(kx * w as i64, lw as i64) * 65535, max_dim), 0, 65535) as i32,
                y: clamp(idiv(idiv(ky * h as i64, lh as i64) * 65535, max_dim), 0, 65535) as i32,
                s: clamp(idiv(ks * 1024, med), 0, 65535) as u16,
            });
        }
    }

    all.sort_by(|a, b| {
        b.s.cmp(&a.s)
            .then(a.level.cmp(&b.level))
            .then(a.y.cmp(&b.y))
            .then(a.x.cmp(&b.x))
    });

    let want = cfg.kp_count;
    if all.len() > want {
        all = if cfg.kp_select == KP_SELECT_QUALITY {
            select_quality(&all, want)
        } else {
            select_grid(&all, want)
        };
    }
    all.truncate(want);

    // wire order is content-derived and total, so two identical keypoint sets
    // serialise identically and comparison is order-independent
    all.sort_by(|a, b| {
        a.desc[0]
            .cmp(&b.desc[0])
            .then(a.desc[1].cmp(&b.desc[1]))
            .then(a.x.cmp(&b.x))
            .then(a.y.cmp(&b.y))
            .then(a.level.cmp(&b.level))
    });

    KpOut {
        list: all,
        max_dim,
        xmax: clamp(idiv((w as i64 - 1) * 65535, max_dim), 0, 65535) as i32,
    }
}
