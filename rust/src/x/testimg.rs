//! Deterministic test images shared by the PAPH-X unit tests: the v42
//! fixture generator (noise field plus forty coloured squares) and the
//! transforms the acceptance tests need.
#![cfg(test)]

pub fn image(seed: u64, w: usize, h: usize) -> Vec<u8> {
    let mut px = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let o = (y * w + x) * 4;
            px[o] = ((x * 7 + y * 3 + seed as usize) % 251) as u8;
            px[o + 1] = ((y * 11 + x * 5) % 253) as u8;
            px[o + 2] = (((x ^ y) * 13) % 247) as u8;
            px[o + 3] = 255;
        }
    }
    let mut s = seed as i64;
    let mut r = move |m: usize| -> usize {
        s = (s.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7fffffff;
        (s as usize) % m.max(1)
    };
    for _ in 0..40 {
        let bx = r(w - 10);
        let by = r(h - 10);
        let (cr, cg, cb) = (r(256) as u8, r(256) as u8, r(256) as u8);
        for dy in 0..6 {
            for dx in 0..6 {
                let o = ((by + dy) * w + bx + dx) * 4;
                px[o] = cr;
                px[o + 1] = cg;
                px[o + 2] = cb;
            }
        }
    }
    px
}

pub fn mirror(px: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut o = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let s = (y * w + (w - 1 - x)) * 4;
            let d = (y * w + x) * 4;
            o[d..d + 4].copy_from_slice(&px[s..s + 4]);
        }
    }
    o
}

/// (pixels, w, h) of a crop
pub fn crop(px: &[u8], w: usize, x0: usize, y0: usize, cw: usize, ch: usize) -> Vec<u8> {
    let mut o = vec![0u8; cw * ch * 4];
    for y in 0..ch {
        for x in 0..cw {
            let s = ((y0 + y) * w + x0 + x) * 4;
            let d = (y * cw + x) * 4;
            o[d..d + 4].copy_from_slice(&px[s..s + 4]);
        }
    }
    o
}

pub fn recolour(px: &[u8]) -> Vec<u8> {
    let mut o = px.to_vec();
    for i in 0..px.len() / 4 {
        let p = i * 4;
        let l = (77 * o[p] as u32 + 150 * o[p + 1] as u32 + 29 * o[p + 2] as u32 + 128) >> 8;
        o[p] = (26 + ((l * 184) >> 8)).min(255) as u8;
        o[p + 1] = (8 + ((l * 126) >> 8)).min(255) as u8;
        o[p + 2] = (66 + ((l * 152) >> 8)).min(255) as u8;
    }
    o
}
