//! Deterministic synthetic images, integer arithmetic only: the generators
//! the equivalence digest (`equiv.rs`) is built from, shared with the
//! PAPH-X benchmark corpus and tests so that every harness draws the same
//! pictures.  Moving them here changed no byte of the digest.

pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

#[derive(Clone)]
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

impl Img {
    pub fn new(w: usize, h: usize) -> Img {
        Img { w, h, px: vec![0u8; w * h * 4] }
    }
    pub fn set(&mut self, x: i64, y: i64, c: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return;
        }
        let o = (y as usize * self.w + x as usize) * 4;
        self.px[o..o + 4].copy_from_slice(&c);
    }
    pub fn get(&self, x: usize, y: usize) -> [u8; 4] {
        let o = (y * self.w + x) * 4;
        [self.px[o], self.px[o + 1], self.px[o + 2], self.px[o + 3]]
    }
}

/// The parity suite's generator (test/parity-wire.mjs `work`).
pub fn work(w: usize, h: usize, seed: i64, alpha: bool) -> Img {
    let mut im = Img::new(w, h);
    let mut s = seed;
    let mut r = || -> f64 {
        s = (s.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7fffffff;
        s as f64 / 0x7fffffff as f64
    };
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * 4;
            let (cx, cy) = (x as f64 - w as f64 / 2.0, y as f64 - h as f64 / 2.0);
            if alpha && (cx * cx) / (w as f64 * w as f64 / 5.0) + (cy * cy) / (h as f64 * h as f64 / 4.5) >= 1.0 {
                im.px[o + 3] = 0;
                continue;
            }
            im.px[o] = ((x * 7 + y * 3 + seed as usize) % 251) as u8;
            im.px[o + 1] = ((y * 11 + x * 5) % 253) as u8;
            im.px[o + 2] = (((x ^ y) * 13) % 247) as u8;
            im.px[o + 3] = 255;
        }
    }
    for _ in 0..40 {
        if w <= 10 || h <= 10 {
            break;
        }
        let bx = (r() * (w - 10) as f64) as usize;
        let by = (r() * (h - 10) as f64) as usize;
        let (cr, cg, cb) = ((r() * 255.0) as u8, (r() * 255.0) as u8, (r() * 255.0) as u8);
        for dy in 0..6 {
            for dx in 0..6 {
                let o = ((by + dy) * w + bx + dx) * 4;
                if im.px[o + 3] == 0 {
                    continue;
                }
                im.px[o] = cr;
                im.px[o + 1] = cg;
                im.px[o + 2] = cb;
            }
        }
    }
    im
}

pub const BAYER: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

/// Pixel art: a palette, a dithered backdrop, rectangles, ellipses, outlines
/// and diagonal strokes, optionally on transparency or on a flat matte.
pub fn pixel_art(w: usize, h: usize, seed: u64, ncol: usize, bg: u8) -> Img {
    let mut r = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let pal: Vec<[u8; 4]> = (0..ncol.max(2))
        .map(|_| [r.below(256) as u8, r.below(256) as u8, r.below(256) as u8, 255])
        .collect();
    let mut im = Img::new(w, h);
    let (a, b) = (pal[0], pal[1 % pal.len()]);
    for y in 0..h {
        for x in 0..w {
            let c = match bg {
                0 => [0, 0, 0, 0],             // transparent backdrop
                1 => [40, 44, 60, 255],        // flat matte
                _ => {
                    let t = ((y * 15) / h.max(1)) as u8;
                    if BAYER[((y & 3) << 2) | (x & 3)] < t { a } else { b }
                }
            };
            im.set(x as i64, y as i64, c);
        }
    }
    let shapes = 3 + r.below(10) as usize;
    for _ in 0..shapes {
        let c1 = pal[r.below(pal.len() as u64) as usize];
        let c2 = pal[r.below(pal.len() as u64) as usize];
        let x0 = r.below(w as u64) as i64;
        let y0 = r.below(h as u64) as i64;
        let sw = 1 + r.below((w as u64 / 2).max(1)) as i64;
        let sh = 1 + r.below((h as u64 / 2).max(1)) as i64;
        let thr = r.below(16) as u8;
        match r.below(4) {
            0 => {
                for y in y0..y0 + sh {
                    for x in x0..x0 + sw {
                        let edge = x == x0 || y == y0 || x == x0 + sw - 1 || y == y0 + sh - 1;
                        let c = if edge { [12, 10, 20, 255] } else if BAYER[(((y & 3) << 2) | (x & 3)) as usize] < thr { c1 } else { c2 };
                        im.set(x, y, c);
                    }
                }
            }
            1 => {
                let (cx, cy, rx, ry) = (x0, y0, sw.max(2), sh.max(2));
                for y in cy - ry..=cy + ry {
                    for x in cx - rx..=cx + rx {
                        let d = (x - cx) * (x - cx) * ry * ry + (y - cy) * (y - cy) * rx * rx;
                        if d <= rx * rx * ry * ry {
                            let c = if BAYER[(((y & 3) << 2) | (x & 3)) as usize] < thr { c1 } else { c2 };
                            im.set(x, y, c);
                        }
                    }
                }
            }
            2 => {
                let len = sw.max(sh) * 2;
                for t in 0..len {
                    im.set(x0 + t, y0 + t / 2, c1);
                    im.set(x0 + t, y0 + t / 2 + 1, c2);
                }
            }
            _ => {
                for k in 0..(sw * sh / 3).max(1) {
                    let x = x0 + (k * 7919) % sw;
                    let y = y0 + (k * 104729) % sh;
                    im.set(x, y, c1);
                }
            }
        }
    }
    im
}

pub fn mirror(a: &Img) -> Img {
    let mut o = Img::new(a.w, a.h);
    for y in 0..a.h {
        for x in 0..a.w {
            o.set(x as i64, y as i64, a.get(a.w - 1 - x, y));
        }
    }
    o
}
pub fn rot90(a: &Img) -> Img {
    let mut o = Img::new(a.h, a.w);
    for y in 0..a.h {
        for x in 0..a.w {
            o.set((a.h - 1 - y) as i64, x as i64, a.get(x, y));
        }
    }
    o
}
pub fn transpose(a: &Img) -> Img {
    let mut o = Img::new(a.h, a.w);
    for y in 0..a.h {
        for x in 0..a.w {
            o.set(y as i64, x as i64, a.get(x, y));
        }
    }
    o
}
pub fn invert(a: &Img) -> Img {
    let mut o = a.clone();
    for i in 0..a.w * a.h {
        for c in 0..3 {
            o.px[i * 4 + c] = 255 - o.px[i * 4 + c];
        }
    }
    o
}
pub fn recolour(a: &Img) -> Img {
    let mut o = a.clone();
    for i in 0..a.w * a.h {
        let p = i * 4;
        let l = (77 * o.px[p] as u32 + 150 * o.px[p + 1] as u32 + 29 * o.px[p + 2] as u32 + 128) >> 8;
        o.px[p] = (26 + ((l * 184) >> 8)).min(255) as u8;
        o.px[p + 1] = (8 + ((l * 126) >> 8)).min(255) as u8;
        o.px[p + 2] = (66 + ((l * 152) >> 8)).min(255) as u8;
    }
    o
}
pub fn nearest_up(a: &Img, k: usize) -> Img {
    let mut o = Img::new(a.w * k, a.h * k);
    for y in 0..a.h * k {
        for x in 0..a.w * k {
            o.set(x as i64, y as i64, a.get(x / k, y / k));
        }
    }
    o
}
pub fn area_down(a: &Img, w: usize, h: usize) -> Img {
    let mut o = Img::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let x0 = x * a.w / w;
            let x1 = ((x + 1) * a.w / w).max(x0 + 1).min(a.w);
            let y0 = y * a.h / h;
            let y1 = ((y + 1) * a.h / h).max(y0 + 1).min(a.h);
            let (mut s, mut n) = ([0u32; 4], 0u32);
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let p = a.get(xx, yy);
                    for c in 0..4 {
                        s[c] += p[c] as u32;
                    }
                    n += 1;
                }
            }
            let al = if s[3] / n.max(1) > 127 { 255 } else { 0 };
            o.set(x as i64, y as i64, [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8, al]);
        }
    }
    o
}
pub fn crop(a: &Img, x0: usize, y0: usize, w: usize, h: usize) -> Img {
    let mut o = Img::new(w, h);
    for y in 0..h {
        for x in 0..w {
            o.set(x as i64, y as i64, a.get((x0 + x).min(a.w - 1), (y0 + y).min(a.h - 1)));
        }
    }
    o
}
pub fn paste(guest: &Img, host: &Img, ox: usize, oy: usize) -> Img {
    let mut o = host.clone();
    for y in 0..guest.h {
        for x in 0..guest.w {
            let p = guest.get(x, y);
            if p[3] >= 128 {
                o.set((ox + x) as i64, (oy + y) as i64, [p[0], p[1], p[2], 255]);
            }
        }
    }
    o
}
pub fn shift1(a: &Img) -> Img {
    let mut o = Img::new(a.w, a.h);
    for y in 0..a.h {
        for x in 0..a.w {
            o.set(x as i64, y as i64, a.get((x + 1).min(a.w - 1), y));
        }
    }
    o
}

