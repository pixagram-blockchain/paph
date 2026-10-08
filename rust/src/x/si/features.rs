//! PAPH-SI feature families (SPEC-SI §3) — the readings of a prepared side
//! that the screening index quantises.
//!
//! Every family is computed from the wire alone (Tier 1, and Tier 2 when it
//! parsed), never from pixels, so stored works can be re-indexed from their
//! wires without re-hashing.  Each is designed to be invariant under the eight
//! symmetries of the square and, where the wire allows it, under the luminance
//! complement and an integer upscale (the hasher divides exact blow-ups
//! first).  What each family does NOT survive is the reason there are six of
//! them: the index weighs the evidence of every family and never asks all of
//! them to agree (SPEC-SI §5.4).  Design intent below; what each one actually
//! keeps under each transform is measured in SPEC-SI §3.2, and falls short of
//! the intent in places the front end's normalisation explains only in part.
//!
//! | family | reads | designed to survive | loses |
//! |---|---|---|---|
//! | RUNS  | run-length histograms, H+V | D4, the complement, palette-preserving recolours, integer upscales | non-integer rescales, re-dithering |
//! | TONE  | RAG quantile gaps/levels, adjacency count, brightness spread, palette quantile mass | D4, the complement, order-keeping recolours | hue-scrambling recolours, crops |
//! | PAL   | palette population profile, colour count | D4, the complement, bijective recolours | resampling (blends add colours), crops |
//! | SHAPE | quantile-band regions: areas, iso/aspect/holes classes, radial profile | D4, order-keeping recolours | crops, rescales |
//! | SIL   | silhouette of the largest opaque component (works with transparency) | D4, recolours, rescales | crops |
//! | KPGEO | keypoint spread, anisotropy, pyramid levels, folded orientations, count | D4, the complement | recolours, crops |
//!
//! Not used, measured (SPEC-SI §3.3): the DCT section (the 16x16 thumbnail's
//! cell edges commute with D4 only when both sides are multiples of 16, and
//! its median-split magnitude bits flip broadly under a one-pixel shift of
//! the cell grid — the same reason XRoute's G0 word is weaker than its
//! construction suggests), the diagonal run histogram (a mirror sends the
//! main diagonal to the anti-diagonal, which the wire does not hold).  Not
//! used, by rule: the colour section (SPEC-003 §6.5: reporting only; a
//! routing key built on hue sends every recoloured copy to the wrong cell).
//!
//! Integer arithmetic only: two nodes indexing the same wires must derive the
//! same cells.

use crate::config::{idiv, isqrt};
use crate::prepared::Prepared;
use crate::wire::{Tier1, F_SIL as WIRE_SIL};

/// The quantised families.
pub const FAMILIES: usize = 6;
pub const F_RUNS: usize = 0;
pub const F_TONE: usize = 1;
pub const F_PAL: usize = 2;
pub const F_SHAPE: usize = 3;
pub const F_SIL: usize = 4;
pub const F_KPGEO: usize = 5;

pub const FAMILY_NAMES: [&str; FAMILIES] = ["runs", "tone", "pal", "shape", "sil", "kpgeo"];

/// Dimensions of each family's vector.
pub const DIMS: [usize; FAMILIES] = [16, 28, 24, 41, 87, 23];
pub const MAX_DIM: usize = 87;

/// The feature derivation's version: a different derivation is a different
/// index, and every SI profile records the version it was fitted on.
pub const FEATURES_VERSION: u16 = 1;

/// RUNS — stroke texture: the horizontal and vertical run-length histograms
/// (same-index runs on the run-length ladder), summed because a quarter turn
/// exchanges them.  Runs of one palette index do not care what colour the
/// index is, so a recolour that maps the palette one to one — the complement
/// included — leaves them where they were, up to how the front end
/// normalises the work (SPEC-SI §3.2).
pub fn runs(t: &Tier1) -> Vec<i32> {
    let r = t.sec("runs");
    (0..16).map(|i| r[i] as i32 + r[16 + i] as i32).collect()
}

/// TONE — luminance topology.  The adjacency (RAG) section keys every colour
/// adjacency by the two colours' luminance quantiles and ranks; the
/// complement maps (qa, qb) to (255 − qb, 255 − qa), under which the gap and
/// the level min(q, 255 − q) are invariant.  Quantiles are mass midpoints in
/// luminance order, so a recolour that keeps the luminance order keeps them
/// exactly.  Plus the brightness record's spread readings and the palette's
/// quantile mass folded about the middle.
pub fn tone(t: &Tier1) -> Vec<i32> {
    let mut v = vec![0i32; DIMS[F_TONE]];
    let s = t.sec("rag");
    let n = t.count("rag").min(48);
    let mut tot = 0i64;
    let (mut gq, mut lv, mut gr) = ([0i64; 8], [0i64; 4], [0i64; 8]);
    for i in 0..n {
        let o = i * 6;
        let (qa, qb, ra, rb) = (s[o] as i64, s[o + 1] as i64, s[o + 2] as i64, s[o + 3] as i64);
        let c = u16::from_le_bytes([s[o + 4], s[o + 5]]) as i64;
        let (qlo, qhi) = (qa.min(qb), qa.max(qb));
        let (rlo, rhi) = (ra.min(rb), ra.max(rb));
        gq[((qhi - qlo) >> 5) as usize] += c;
        lv[(qlo.min(255 - qhi).max(0) >> 5).min(3) as usize] += c;
        gr[((rhi - rlo) >> 5) as usize] += c;
        tot += c;
    }
    let mut k = 0;
    for b in gq.iter().chain(lv.iter()).chain(gr.iter()) {
        v[k] = if tot > 0 { idiv(b * 255, tot) as i32 } else { 0 };
        k += 1;
    }
    // brightness: q95 − q5, q75 − q25, |median − 128| (all complement-invariant)
    let b = t.sec("brightness");
    v[20] = b[6] as i32;
    v[21] = (b[4] as i32 - b[2] as i32).max(0);
    v[22] = (b[3] as i32 - 128).abs() * 2;
    v[23] = (n as i32) * 5;
    // palette quantile mass, 8 bins folded about the middle
    let p = t.sec("palette");
    let pn = t.count("palette").min(24);
    let mut qm = [0i64; 4];
    let mut qt = 0i64;
    for i in 0..pn {
        let c = p[4 * i + 1] as i64;
        let q = (p[4 * i + 3] as i64 >> 5) as usize; // 0..7
        qm[q.min(7 - q)] += c;
        qt += c;
    }
    for j in 0..4 {
        v[24 + j] = if qt > 0 { idiv(qm[j] * 255, qt) as i32 } else { 0 };
    }
    v
}

/// PAL — the palette's population profile: the share of each of the 24 most
/// frequent colours relative to the most frequent, and how many there are.
/// It holds no colour at all, only how the mass is spread over colours, so
/// any bijective recolour (palette swap, channel swap, hue rotation, the
/// complement) leaves it where it was.
pub fn pal(t: &Tier1) -> Vec<i32> {
    let mut v = vec![0i32; DIMS[F_PAL]];
    let p = t.sec("palette");
    let n = t.count("palette").min(24);
    for i in 1..n {
        v[i - 1] = p[4 * i + 1] as i32;
    }
    v[23] = (n as i32) * 10;
    v
}

/// The normalised image's shape grid (sections.rs `shape_signatures`),
/// recomputed from the header: the long side cut into at most 128 cells.
fn shape_grid(t: &Tier1) -> (i64, i64) {
    let sc = (t.scale as i64).max(1);
    let (w, h) = (t.width as i64 / sc, t.height as i64 / sc);
    let long = w.max(h).max(1);
    let cell = idiv(long + 127, 128).max(1);
    let gw = idiv(w + cell - 1, cell).max(4);
    let gh = idiv(h + cell - 1, cell).max(4);
    (gw, gh)
}

fn iso_class(area: i64, per: i64) -> i32 {
    let iso = if area > 0 { per * per * 256 / area } else { 0 };
    [512i64, 1024, 2048, 4096, 8192, 16384, 32768].iter().filter(|&&x| iso >= x).count() as i32
}

fn aspect_class(aspect: i64) -> i32 {
    let a = if aspect > 0 { aspect.max(65536 / aspect) } else { 256 };
    [320i64, 410, 512, 768, 1280, 2048, 4096].iter().filter(|&&x| a >= x).count() as i32
}

/// Eight order statistics of a 32-ray radial profile.  The rays sit at
/// multiples of 2π/32, so every symmetry of the square permutes them: the
/// sorted profile is invariant where the profile itself is not.
fn sorted_rays(r: &[u8]) -> [i32; 8] {
    let mut a = [0u8; 32];
    a.copy_from_slice(&r[..32]);
    a.sort_unstable();
    let mut out = [0i32; 8];
    for k in 0..8 {
        out[k] = a[k * 4 + 2] as i32;
    }
    out
}

/// SHAPE — the regions of the 8-band luminance-quantile map (largest first):
/// their areas relative to the largest, isoperimetric, aspect and hole
/// classes, how many there are, the largest region's share of the frame and
/// its sorted radial profile.  Quantile bands survive any order-keeping
/// recolour, and the complement relabels them without moving a boundary.
pub fn shape(t: &Tier1) -> Vec<i32> {
    let mut v = vec![0i32; DIMS[F_SHAPE]];
    let s = t.sec("shapes");
    let n = t.count("shapes").min(8);
    let rd = |o: usize| -> (i64, i64, i64, i64) {
        let area = u32::from_le_bytes([s[o], s[o + 1], s[o + 2], s[o + 3]]) as i64;
        let per = u16::from_le_bytes([s[o + 4], s[o + 5]]) as i64;
        let aspect = u16::from_le_bytes([s[o + 6], s[o + 7]]) as i64;
        let holes = s[o + 8] as i64;
        (area, per, aspect, holes)
    };
    if n == 0 {
        return v;
    }
    let (a0, _, _, _) = rd(0);
    for i in 1..8 {
        if i < n {
            let (a, _, _, _) = rd(i * 41);
            v[i - 1] = idiv(a * 255, a0.max(1)) as i32;
        }
    }
    for i in 0..8 {
        if i < n {
            let (a, p, asp, h) = rd(i * 41);
            v[7 + 3 * i] = iso_class(a, p) * 36;
            v[8 + 3 * i] = aspect_class(asp) * 36;
            v[9 + 3 * i] = (h.min(3) as i32) * 85;
        }
    }
    v[31] = (n as i32) * 32;
    let (gw, gh) = shape_grid(t);
    v[32] = idiv(a0 * 255, (gw * gh).max(1)).min(255) as i32;
    let r = sorted_rays(&s[9..41]);
    v[33..41].copy_from_slice(&r);
    v
}

/// SIL — the silhouette of the largest opaque component, present only when
/// the work has real transparency (the wire's F_SIL flag — sprites, and works
/// on a flat matte the front end folds to transparency): sorted radial
/// profile, the two axis-aligned second moments as min and max (a quarter
/// turn exchanges them) and |m11|, fill, component count, aspect class,
/// opaque share, transition histograms (rows and columns exchange, so
/// averaged) and the D4-canonical 8x8 occupancy code bit by bit.  It never
/// reads a colour.
pub fn sil(t: &Tier1) -> Option<Vec<i32>> {
    if t.flags & WIRE_SIL == 0 {
        return None;
    }
    let s = t.sec("silhouette");
    let mut v = vec![0i32; DIMS[F_SIL]];
    v[..8].copy_from_slice(&sorted_rays(&s[0..32]));
    let (m20, m02) = (s[32] as i32, s[33] as i32);
    v[8] = m20.min(m02);
    v[9] = m20.max(m02);
    v[10] = s[34] as i32;
    let asp = u16::from_le_bytes([s[36], s[37]]) as i64;
    v[11] = aspect_class(asp) * 36;
    v[12] = s[38] as i32;
    v[13] = (s[39] as i32).min(255);
    v[14] = (u16::from_le_bytes([s[64], s[65]]) >> 8) as i32;
    for i in 0..8 {
        v[15 + i] = (s[40 + i] as i32 + s[48 + i] as i32) / 2;
    }
    let hi = u32::from_le_bytes([s[56], s[57], s[58], s[59]]) as u64;
    let lo = u32::from_le_bytes([s[60], s[61], s[62], s[63]]) as u64;
    let code = (hi << 32) | lo;
    for b in 0..64 {
        v[23 + b] = (((code >> (63 - b)) & 1) as i32) * 255;
    }
    Some(v)
}

/// KPGEO — how the keypoints are spread (Tier 2, or the Tier-1 sketch when
/// Tier 2 is absent): radial distribution about their centroid in units of
/// their own RMS radius (scale-free), the anisotropy of their scatter, the
/// pyramid levels they came from, their orientation sectors folded by the
/// square's symmetries and the complement (a quarter turn adds 16 of 64
/// sectors, a mirror reflects them, the complement adds 32), and their count
/// on a log scale.  Absent below eight keypoints.
pub fn kpgeo(p: &Prepared) -> Option<Vec<i32>> {
    let n = p.kp.len();
    if n < 8 {
        return None;
    }
    let mut v = vec![0i32; DIMS[F_KPGEO]];
    let (mut sx, mut sy) = (0i64, 0i64);
    for k in p.kp.iter() {
        sx += k.x as i64;
        sy += k.y as i64;
    }
    let (cx, cy) = (idiv(sx, n as i64), idiv(sy, n as i64));
    let (mut a, mut b, mut c) = (0i64, 0i64, 0i64);
    for k in p.kp.iter() {
        let (dx, dy) = (k.x as i64 - cx, k.y as i64 - cy);
        a += dx * dx;
        b += dx * dy;
        c += dy * dy;
    }
    let (a, b, c) = (idiv(a, n as i64), idiv(b, n as i64), idiv(c, n as i64));
    let rms = isqrt(a + c).max(1);
    let mut rh = [0i64; 8];
    let mut lvh = [0i64; 4];
    let mut sec = [0i64; 9];
    for k in p.kp.iter() {
        let (dx, dy) = (k.x as i64 - cx, k.y as i64 - cy);
        let r = isqrt(dx * dx + dy * dy);
        rh[(idiv(r * 4, rms) as usize).min(7)] += 1;
        lvh[(k.level as usize).min(3)] += 1;
        let m = (k.sec as usize) & 15;
        sec[m.min(16 - m) & 15] += 1;
    }
    for i in 0..8 {
        v[i] = idiv(rh[i] * 255, n as i64) as i32;
    }
    // anisotropy: λmin / λmax of the scatter, 0..255
    let tr = (a + c) as i128;
    let det_term = ((a - c) as i128) * ((a - c) as i128) + 4 * (b as i128) * (b as i128);
    let disc = isqrt128(det_term);
    let lmax = (tr + disc).max(1);
    let lmin = (tr - disc).max(0);
    v[8] = ((lmin * 255) / lmax) as i32;
    for i in 0..4 {
        v[9 + i] = idiv(lvh[i] * 255, n as i64) as i32;
    }
    for i in 0..9 {
        v[13 + i] = idiv(sec[i] * 255, n as i64) as i32;
    }
    let mut lg = 0i32;
    while (1usize << lg) < n {
        lg += 1;
    }
    v[22] = lg * 25;
    Some(v)
}

/// floor(sqrt(n)) for a non-negative i128, exact: Newton's iteration from a
/// power of two above the root, integers only.
pub fn isqrt128(n: i128) -> i128 {
    if n <= 0 {
        return 0;
    }
    let bits = 128 - n.leading_zeros() as i128;
    let mut x: i128 = 1i128 << ((bits + 1) / 2);
    loop {
        let y = (x + n / x) >> 1;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// Every family of a side (`None` where the family is not measurable).
pub fn families(p: &Prepared) -> [Option<Vec<i32>>; FAMILIES] {
    let t = &p.t1;
    [Some(runs(t)), Some(tone(t)), Some(pal(t)), Some(shape(t)), sil(t), kpgeo(p)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isqrt128_is_exact() {
        for n in [0i128, 1, 2, 3, 4, 15, 16, 17, 99, 100, 101, (1 << 62) - 1, 1 << 62, 4_839_204_820_485_202_222_222] {
            let r = isqrt128(n);
            assert!(r * r <= n && (r + 1) * (r + 1) > n, "{n} -> {r}");
        }
        let mut s = 0x1234_5678_9abc_def0u64;
        for _ in 0..2000 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let n = (s as i128) * ((s >> 7) as i128);
            let n = n.abs();
            let r = isqrt128(n);
            assert!(r * r <= n && (r + 1) * (r + 1) > n);
        }
    }
}
