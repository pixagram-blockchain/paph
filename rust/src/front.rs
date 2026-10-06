//! The unified front end (SPEC-003 §5).
//!
//! Both halves of the algorithm — the structural bag of features and the
//! keypoint matcher — consume the identical normalised image.  In v2 they did
//! not, and three of the seven conflicts in SPEC-002 §22 came from that alone.

use crate::config::{clamp, idiv, Config};
use std::collections::HashMap;

pub struct Image {
    pub px: Vec<u8>,
    pub w: usize,
    pub h: usize,
}

#[derive(Clone, Copy)]
pub struct PalEntry {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub rs: i64,
    pub gs: i64,
    pub bs: i64,
    pub k: i32,
    pub n: i32,
    pub lum: i32,
    pub lum_order: i32,
    pub quantile: i32,
    pub band: i32,
}

pub struct Indexed {
    pub idx: Vec<i32>,
    pub pal: Vec<PalEntry>,
    pub w: usize,
    pub h: usize,
    pub opaque: i32,
}

/// One definition of luminance, used by BOTH halves.  v2's keypoint half
/// dropped the rounding term, so the two halves disagreed about brightness.
#[inline(always)]
pub fn luma(r: i32, g: i32, b: i32) -> i32 {
    (77 * r + 150 * g + 29 * b + 128) >> 8
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// Largest k such that the image is exactly a k-fold nearest-neighbour blow-up.
/// A 4x upscale is the same drawing, so it must produce the same fingerprint.
///
/// An image is a k-fold blow-up exactly when every row changes colour only at
/// multiples of k and every row differs from the one above only at multiples
/// of k.  So the largest such k dividing both dimensions is
/// `gcd(w, h, every change position)` — one pass that stops the moment that
/// gcd reaches 1, which on ordinary art is within the first changes.  The
/// reference tried every divisor of gcd(w, h) from the top, re-walking the
/// image from the start each time; `upscale_factor_ref` is kept and tested
/// equal.
pub fn upscale_factor(px: &[u8], w: usize, h: usize) -> usize {
    let mut g = gcd(w, h);
    if g < 2 {
        return 1;
    }
    let pix = |i: usize| -> u32 { u32::from_le_bytes([px[4 * i], px[4 * i + 1], px[4 * i + 2], px[4 * i + 3]]) };
    for y in 0..h {
        let row = &px[y * w * 4..(y + 1) * w * 4];
        if y > 0 && row != &px[(y - 1) * w * 4..y * w * 4] {
            g = gcd(g, y);
            if g < 2 {
                return 1;
            }
        }
        let mut prev = pix(y * w);
        for x in 1..w {
            let v = pix(y * w + x);
            if v != prev {
                g = gcd(g, x);
                if g < 2 {
                    return 1;
                }
                prev = v;
            }
        }
    }
    g
}

#[cfg(test)]
fn upscale_factor_ref(px: &[u8], w: usize, h: usize) -> usize {
    let g = gcd(w, h);
    let mut k = g;
    while k >= 2 {
        if w % k != 0 || h % k != 0 {
            k -= 1;
            continue;
        }
        let mut ok = true;
        'outer: for y in 0..h {
            let by = (y - y % k) * w;
            for x in 0..w {
                let s = (by + x - x % k) << 2;
                let d = (y * w + x) << 2;
                if px[s] != px[d] || px[s + 1] != px[d + 1] || px[s + 2] != px[d + 2] || px[s + 3] != px[d + 3] {
                    ok = false;
                    break 'outer;
                }
            }
        }
        if ok {
            return k;
        }
        k -= 1;
    }
    1
}

pub fn shrink(px: &[u8], w: usize, h: usize, k: usize) -> Image {
    let nw = w / k;
    let nh = h / k;
    let mut out = vec![0u8; nw * nh * 4];
    for y in 0..nh {
        for x in 0..nw {
            let s = ((y * k) * w + x * k) << 2;
            let d = (y * nw + x) << 2;
            out[d..d + 4].copy_from_slice(&px[s..s + 4]);
        }
    }
    Image { px: out, w: nw, h: nh }
}

/// Fold a flat backdrop to transparent.
///
/// Seeded from the WHOLE frame using the border ring's modal colour, not from
/// a corner: real backdrops are dithered (23-43 distinct colours in one row, so
/// a tolerance-2 flood folds nothing) and are disconnected by props reaching
/// the edge (a corner flood reached 16% of a canvas whose backdrop was most of
/// it).  Returns None when there is no matte — only a picture that happens to
/// reach its own edges.
pub fn fold_matte(px: &[u8], w: usize, h: usize, tol: i32) -> Option<Vec<u8>> {
    let n = w * h;
    if w < 8 || h < 8 {
        return None;
    }
    let mut opaque = 0i64;
    for i in 0..n {
        if px[(i << 2) + 3] > 8 {
            opaque += 1;
        }
    }
    // already has real alpha; leave it alone
    if (opaque as f64) < (n as f64) * 0.98 {
        return None;
    }

    let mut tally: HashMap<i32, i32> = HashMap::new();
    let mut ring: Vec<usize> = Vec::with_capacity(2 * (w + h));
    {
        let edge = |p: usize, ring: &mut Vec<usize>, tally: &mut HashMap<i32, i32>| {
            ring.push(p);
            let o = p << 2;
            let k = (((px[o] >> 4) as i32) << 8) | (((px[o + 1] >> 4) as i32) << 4) | ((px[o + 2] >> 4) as i32);
            *tally.entry(k).or_insert(0) += 1;
        };
        for x in 0..w {
            edge(x, &mut ring, &mut tally);
            edge((h - 1) * w + x, &mut ring, &mut tally);
        }
        for y in 1..h - 1 {
            edge(y * w, &mut ring, &mut tally);
            edge(y * w + w - 1, &mut ring, &mut tally);
        }
    }

    let mut best_k = -1i32;
    let mut best_n = 0i32;
    let mut keys: Vec<i32> = tally.keys().copied().collect();
    keys.sort_unstable();
    for k in keys {
        let v = tally[&k];
        if v > best_n || (v == best_n && k < best_k) {
            best_n = v;
            best_k = k;
        }
    }
    // no single colour owns half the frame -> no matte
    if (best_n as usize) * 2 < ring.len() {
        return None;
    }

    let (mut r0, mut g0, mut b0, mut cnt) = (0i64, 0i64, 0i64, 0i64);
    for &p in &ring {
        let o = p << 2;
        let k = (((px[o] >> 4) as i32) << 8) | (((px[o + 1] >> 4) as i32) << 4) | ((px[o + 2] >> 4) as i32);
        if k == best_k {
            r0 += px[o] as i64;
            g0 += px[o + 1] as i64;
            b0 += px[o + 2] as i64;
            cnt += 1;
        }
    }
    let r0 = idiv(r0, cnt) as i32;
    let g0 = idiv(g0, cnt) as i32;
    let b0 = idiv(b0, cnt) as i32;
    let t = tol;

    let near = |p: usize| -> bool {
        let o = p << 2;
        (px[o] as i32 - r0).abs() <= t && (px[o + 1] as i32 - g0).abs() <= t && (px[o + 2] as i32 - b0).abs() <= t
    };

    let mut seen = vec![0u8; n];
    let mut stack: Vec<usize> = Vec::with_capacity(n);
    for &p in &ring {
        if seen[p] == 0 && near(p) {
            seen[p] = 1;
            stack.push(p);
        }
    }
    if stack.is_empty() {
        return None;
    }
    let mut area = 0usize;
    while let Some(p) = stack.pop() {
        area += 1;
        let px_ = p % w;
        let py = p / w;
        let nb = [
            if px_ > 0 { p as i64 - 1 } else { -1 },
            if px_ < w - 1 { p as i64 + 1 } else { -1 },
            if py > 0 { p as i64 - w as i64 } else { -1 },
            if py < h - 1 { p as i64 + w as i64 } else { -1 },
        ];
        for q in nb {
            if q < 0 {
                continue;
            }
            let q = q as usize;
            if seen[q] != 0 || !near(q) {
                continue;
            }
            seen[q] = 1;
            stack.push(q);
        }
    }
    // a fold that takes almost everything has eaten the subject
    if (area as f64) < (n as f64) * 0.02 || (area as f64) > (n as f64) * 0.90 {
        return None;
    }
    let mut out = px.to_vec();
    for i in 0..n {
        if seen[i] != 0 {
            out[(i << 2) + 3] = 0;
        }
    }
    Some(out)
}

/// Quantise to a 5-5-5 palette, order by luminance, and give every entry the
/// MIDPOINT of its own mass as a quantile.
///
/// The midpoint matters: only it satisfies `q(inverted) == 255 - q`, which is
/// what the complement fold in `canonical64` and the reflection hypothesis in
/// the palette channel both depend on.  v2 used "mass strictly darker", so
/// under a luminance flip each entry shifted by its own mass and the fold could
/// never fire — measured, an inverted copy collided on 0 of 128 regions.
pub fn index_image(px: &[u8], w: usize, h: usize) -> Indexed {
    let n = w * h;
    // 5-5-5 with a low guard bit, so a valid key is never 0 — and 0 can stand
    // for "transparent" in a u16 key map
    let mut key = vec![0u16; n];
    // The 5-5-5-guard key is 16 bits by construction, so "map" is a 65536-entry
    // array and not a hash table.  This function runs per pixel, and a SipHash
    // per pixel was a fifth of the whole hash's time.
    const NO: u32 = u32::MAX;
    /// a key folded onto its nearest survivor, cached: the survivor's index
    /// with this bit set
    const SPILL: u32 = 0x8000_0000;
    let mut map = vec![NO; 65536];
    let mut pal: Vec<PalEntry> = Vec::new();

    for (i, p) in px[..n * 4].chunks_exact(4).enumerate() {
        if p[3] < 128 {
            continue;
        }
        let k = ((p[0] >> 3) as u16) << 11 | ((p[1] >> 3) as u16) << 6 | ((p[2] >> 3) as u16) << 1 | 1;
        key[i] = k;
        let e = map[k as usize];
        if e != NO {
            let e = &mut pal[e as usize];
            e.n += 1;
            e.rs += p[0] as i64;
            e.gs += p[1] as i64;
            e.bs += p[2] as i64;
        } else {
            map[k as usize] = pal.len() as u32;
            pal.push(PalEntry {
                r: 0,
                g: 0,
                b: 0,
                rs: p[0] as i64,
                gs: p[1] as i64,
                bs: p[2] as i64,
                k: k as i32,
                n: 1,
                lum: 0,
                lum_order: 0,
                quantile: 0,
                band: 0,
            });
        }
    }

    // forget every pre-sort slot (only those were ever written)
    for p in pal.iter() {
        map[p.k as usize] = NO;
    }
    // deterministic order: population desc, then key asc — never insertion order
    pal.sort_by(|a, b| b.n.cmp(&a.n).then(a.k.cmp(&b.k)));
    pal.truncate(255);
    for p in pal.iter_mut() {
        p.r = idiv(p.rs, p.n as i64) as u8;
        p.g = idiv(p.gs, p.n as i64) as u8;
        p.b = idiv(p.bs, p.n as i64) as u8;
        p.lum = luma(p.r as i32, p.g as i32, p.b as i32);
    }
    for (i, p) in pal.iter().enumerate() {
        map[p.k as usize] = i as u32;
    }
    // the survivors' 5-bit components, as arrays the nearest search can stream
    let (pr, pg, pb): (Vec<i32>, Vec<i32>, Vec<i32>) = (
        pal.iter().map(|p| (p.r >> 3) as i32).collect(),
        pal.iter().map(|p| (p.g >> 3) as i32).collect(),
        pal.iter().map(|p| (p.b >> 3) as i32).collect(),
    );
    let mut dist = vec![0i32; pal.len()];

    // anything past the cap folds onto its nearest surviving entry
    let mut idx = vec![-1i32; n];
    for i in 0..n {
        let kk = key[i];
        if kk == 0 {
            continue;
        }
        let slot = map[kk as usize];
        let v = if slot & SPILL == 0 {
            // a survivor (NO has the bit set, so it never lands here)
            slot as usize
        } else {
            let v = if slot != NO {
                (slot & !SPILL) as usize
            } else {
                let rr = ((kk >> 11) & 31) as i32;
                let gg = ((kk >> 6) & 31) as i32;
                let bb = ((kk >> 1) & 31) as i32;
                // the FIRST entry at the least squared distance — a full
                // distance pass the compiler vectorises, then the first index
                // that attains its minimum
                for (j, d) in dist.iter_mut().enumerate() {
                    let (dr, dg, db) = (pr[j] - rr, pg[j] - gg, pb[j] - bb);
                    *d = dr * dr + dg * dg + db * db;
                }
                let bd = dist.iter().copied().min().unwrap_or(i32::MAX);
                let best = dist.iter().position(|&d| d == bd).unwrap_or(0);
                map[kk as usize] = best as u32 | SPILL;
                best
            };
            pal[v].n += 1;
            v
        };
        idx[i] = v as i32;
    }

    // luminance order + MIDPOINT quantile.  Only the midpoint satisfies
    // q(inverted) == 255 - q exactly, which is what the complement fold in
    // canonical64 and the reflection hypothesis in the palette channel both
    // depend on.  v2 used "mass strictly darker" and the fold never fired.
    let mut order: Vec<usize> = (0..pal.len()).collect();
    order.sort_by(|&a, &b| pal[a].lum.cmp(&pal[b].lum).then(pal[a].k.cmp(&pal[b].k)));
    let opaque: i32 = pal.iter().map(|p| p.n).sum();
    let mut acc = 0i64;
    for (rank, &i) in order.iter().enumerate() {
        pal[i].lum_order = rank as i32;
        pal[i].quantile = if opaque > 0 {
            clamp(
                idiv((2 * acc + pal[i].n as i64) * 255 + opaque as i64, 2 * opaque as i64),
                0,
                255,
            ) as i32
        } else {
            0
        };
        pal[i].band = pal[i].quantile >> 5;
        acc += pal[i].n as i64;
    }

    Indexed { idx, pal, w, h, opaque }
}

/// Run the whole front end.  Returns the normalised image plus what happened.
pub struct Normalised {
    pub im: Indexed,
    pub orig_w: usize,
    pub orig_h: usize,
    pub scale: usize,
    pub matte: bool,
}

pub fn normalise(px: &[u8], w: usize, h: usize, cfg: &Config) -> Normalised {
    let (orig_w, orig_h) = (w, h);
    // the input is only copied when a stage actually changes it
    let mut owned: Option<Vec<u8>> = None;
    let (mut cw, mut ch) = (w, h);
    let mut matte = false;
    let mut scale = 1usize;

    if cfg.fold_matte {
        if let Some(folded) = fold_matte(px, cw, ch, cfg.matte_tol) {
            owned = Some(folded);
            matte = true;
        }
    }
    if cfg.divide_upscale {
        let cur: &[u8] = owned.as_deref().unwrap_or(px);
        let k = upscale_factor(cur, cw, ch);
        if k > 1 {
            let s = shrink(cur, cw, ch, k);
            owned = Some(s.px);
            cw = s.w;
            ch = s.h;
            scale = k;
        }
    }
    let cur: &[u8] = owned.as_deref().unwrap_or(px);
    Normalised {
        im: index_image(cur, cw, ch),
        orig_w,
        orig_h,
        scale,
        matte,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The change-point gcd equals the divisor walk on blow-ups of every
    /// factor, on near-blow-ups broken by one pixel, and on flat images.
    #[test]
    fn upscale_factor_matches_the_divisor_walk() {
        let mut s = 0x9e37_79b9_7f4a_7c15u64;
        let mut r = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for case in 0..600 {
            let (bw, bh) = (1 + (r() % 12) as usize, 1 + (r() % 12) as usize);
            let k = 1 + (r() % 6) as usize;
            let ncol = 1 + (r() % 4);
            let base: Vec<[u8; 4]> = (0..bw * bh)
                .map(|_| {
                    let c = (r() % ncol) as u8;
                    [c * 60, c * 30, 255 - c * 50, if c == 3 { 0 } else { 255 }]
                })
                .collect();
            let (w, h) = (bw * k, bh * k);
            let mut px = vec![0u8; w * h * 4];
            for y in 0..h {
                for x in 0..w {
                    px[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&base[(y / k) * bw + x / k]);
                }
            }
            if case % 3 == 0 && w * h > 1 {
                let i = (r() as usize) % (w * h);
                px[i * 4] ^= 1;
            }
            assert_eq!(upscale_factor(&px, w, h), upscale_factor_ref(&px, w, h), "case {case} {w}x{h} k {k}");
        }
    }
}
