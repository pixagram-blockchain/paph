//! Tier-1 section builders (SPEC-003 §6).
//!
//! Every byte written here is produced by integer arithmetic over frozen
//! tables.  Two independent implementations must be able to agree on all
//! 3952 of them; `test/parity.js` checks that against the JavaScript engine.

use crate::config::{clamp, idiv, Config};
use crate::front::Indexed;
use crate::simd::{median_mask, Cells};
use crate::tables::*;

pub const PAL_N: usize = 24;
pub const RAG_N: usize = 48;
pub const SHAPE_N: usize = 8;
pub const SHAPE_BYTES: usize = 41;
pub const Q: i32 = 14;
pub const QONE: i64 = 1 << Q;
pub const R: i32 = 10;
pub const RONE: i64 = 1 << R;

// ---------------------------------------------------------------- DCT

fn dct2(src: &[i32], n: usize) -> Vec<i32> {
    let t = cos_table(n);
    let mut tmp = vec![0i64; n * n];
    let mut out = vec![0i32; n * n];
    for j in 0..n {
        for u in 0..n {
            let mut s = 0i64;
            for i in 0..n {
                s += src[j * n + i] as i64 * t[u * n + i] as i64;
            }
            tmp[j * n + u] = (s + (QONE >> 1)) >> Q;
        }
    }
    for u in 0..n {
        for v in 0..n {
            let mut s = 0i64;
            for j in 0..n {
                s += tmp[j * n + u] * t[v * n + j] as i64;
            }
            out[v * n + u] = ((s + (QONE >> 1)) >> Q) as i32;
        }
    }
    out
}

/// One SIGN BIT plus a Gray-coded magnitude bucket, taken against the order
/// statistics of |coef| in the block's own distribution.
///
/// Bucketing the SIGNED value would cost the dihedral group: under a
/// horizontal flip a 2-D DCT negates every coefficient with odd horizontal
/// frequency, and a signed percentile bucket cannot be negated after the fact.
/// Split the sign off and all eight symmetries become bit operations on the
/// stored code — a mirrored or quarter-turned copy is then found at compare
/// time for zero extra bytes.  Bucketing |coef| against its own block also
/// makes the code invariant to a global contrast scale.
fn quantise_block(co: &[i32], bits: usize, keep_dc: bool, bytes: &mut [u8], off: usize) -> usize {
    let n = co.len();
    let mbits = bits - 1;
    let mut mags: Vec<i32> = Vec::with_capacity(n);
    for i in (if keep_dc { 0 } else { 1 })..n {
        mags.push(co[i].abs());
    }
    mags.sort_unstable();
    let levels = 1usize << mbits;
    let mut cuts = vec![0i32; if levels > 1 { levels - 1 } else { 1 }];
    for i in 1..levels {
        let p = idiv(i as i64 * mags.len() as i64, levels as i64);
        cuts[i - 1] = if mags.is_empty() {
            0
        } else {
            mags[clamp(p, 0, mags.len() as i64 - 1) as usize]
        };
    }
    let gray: &[i32] = match mbits {
        1 => &GRAY1,
        2 => &GRAY2,
        _ => &GRAY3,
    };
    let mut bit = 0usize;
    for i in 0..n {
        let mut sgn = 0i32;
        let mut q = 0usize;
        if !(i == 0 && !keep_dc) {
            let v = co[i];
            sgn = if v < 0 { 1 } else { 0 };
            let m = v.abs();
            while q < levels - 1 && m >= cuts[q] {
                q += 1;
            }
        }
        let code = (sgn << mbits) | gray[q];
        for k in (0..bits).rev() {
            if (code >> k) & 1 != 0 {
                bytes[off + (bit >> 3)] |= 0x80 >> (bit & 7);
            }
            bit += 1;
        }
    }
    (n * bits) >> 3
}

/// 256 B: one 16x16 block at 2 bits, four 8x8 at 2 bits, sixteen 4x4 at 4 bits.
/// DC is ALWAYS dropped (SPEC-003 §6.3) so the index bucket cannot degenerate
/// into "is this picture bright".
pub fn hierarchical_dct(thumb: &[i32]) -> Vec<u8> {
    let mut out = vec![0u8; 256];
    quantise_block(&dct2(thumb, 16), 2, false, &mut out, 0);
    let mut off = 64usize;
    for qy in 0..2 {
        for qx in 0..2 {
            let mut blk = vec![0i32; 64];
            for y in 0..8 {
                for x in 0..8 {
                    blk[y * 8 + x] = thumb[(qy * 8 + y) * 16 + qx * 8 + x];
                }
            }
            off += quantise_block(&dct2(&blk, 8), 2, false, &mut out, off);
        }
    }
    for ty in 0..4 {
        for tx in 0..4 {
            let mut t4 = vec![0i32; 16];
            for y in 0..4 {
                for x in 0..4 {
                    t4[y * 4 + x] = thumb[(ty * 4 + y) * 16 + tx * 4 + x];
                }
            }
            off += quantise_block(&dct2(&t4, 4), 4, false, &mut out, off);
        }
    }
    out
}

// ------------------------------------------------------- area majority

pub fn area_majority(im: &Indexed, nw: usize, nh: usize) -> Vec<i32> {
    let (w, h) = (im.w, im.h);
    let mut out = vec![0i32; nw * nh];
    // slot 0 = transparent, slot v = palette entry v - 1
    let mut tally = vec![0i32; im.pal.len() + 1];
    let mut touched: Vec<usize> = Vec::new();
    // The winner is the slot with the largest (count, rank), transparent
    // ranking below every palette entry and palette ranks all distinct — a
    // strict total order, so it does not matter in which order the occupied
    // slots are visited.  The reference cleared and scanned every palette slot
    // per cell; this visits only the slots the cell's pixels touched.
    let rank = |v: usize| -> i32 { if v == 0 { -1 } else { im.pal[v - 1].lum_order } };
    let xs: Vec<(usize, usize)> = (0..nw)
        .map(|i| {
            let x0 = idiv(i as i64 * w as i64, nw as i64) as usize;
            let mut x1 = idiv((i as i64 + 1) * w as i64, nw as i64) as usize;
            if x1 <= x0 {
                x1 = x0 + 1;
            }
            (x0, x1.min(w))
        })
        .collect();
    for j in 0..nh {
        let y0 = idiv(j as i64 * h as i64, nh as i64) as usize;
        let mut y1 = idiv((j as i64 + 1) * h as i64, nh as i64) as usize;
        if y1 <= y0 {
            y1 = y0 + 1;
        }
        for i in 0..nw {
            let (x0, x1) = xs[i];
            for y in y0..y1.min(h) {
                for &v in im.idx[y * w + x0..y * w + x1.max(x0)].iter() {
                    let slot = (v + 1) as usize;
                    if tally[slot] == 0 {
                        touched.push(slot);
                    }
                    tally[slot] += 1;
                }
            }
            let (mut best, mut bn, mut brank) = (-1i32, 0i32, -1i32);
            for &v in touched.iter() {
                let (c, r) = (tally[v], rank(v));
                if c > bn || (c == bn && r > brank) {
                    bn = c;
                    best = v as i32 - 1;
                    brank = r;
                }
                tally[v] = 0;
            }
            touched.clear();
            out[j * nw + i] = best;
        }
    }
    out
}

/// 16x16 luminance thumbnail.  Cells with no opaque pixel take the MEDIAN
/// opaque luminance, not 0: filling holes with black manufactures an edge that
/// the same sprite composited on a host would not have, and the two maps of one
/// drawing then share almost nothing.
pub fn thumbnail16(im: &Indexed) -> Vec<i32> {
    let m = area_majority(im, 16, 16);
    let mut lums: Vec<i32> = im.pal.iter().map(|p| p.lum).collect();
    lums.sort_unstable();
    let fill = if lums.is_empty() { 128 } else { lums[lums.len() >> 1] };
    (0..256)
        .map(|i| if m[i] < 0 { fill } else { im.pal[m[i] as usize].lum })
        .collect()
}

// -------------------------------------------------------- brightness

pub fn brightness_record(thumb: &[i32]) -> Vec<u8> {
    let mut out = vec![0u8; 8];
    let s: i64 = thumb.iter().map(|&v| v as i64).sum();
    out[0] = clamp(idiv(s, 256), 0, 255) as u8;
    let mut srt = thumb.to_vec();
    srt.sort_unstable();
    out[1] = srt[12] as u8;
    out[2] = srt[64] as u8;
    out[3] = srt[128] as u8;
    out[4] = srt[192] as u8;
    out[5] = srt[243] as u8;
    out[6] = clamp((srt[243] - srt[12]) as i64, 0, 255) as u8;
    let mut q = 0i32;
    for qy in 0..2 {
        for qx in 0..2 {
            let mut t = 0i64;
            for y in 0..8 {
                for x in 0..8 {
                    t += thumb[(qy * 8 + y) * 16 + qx * 8 + x] as i64;
                }
            }
            let m = idiv(t, 64);
            q = (q << 2)
                | (if m > out[0] as i64 { 2 } else { 0 })
                | (if m > out[2] as i64 { 1 } else { 0 });
        }
    }
    out[7] = (q & 255) as u8;
    out
}

// ----------------------------------------------------------- palette

pub fn identity_palette(im: &Indexed) -> (Vec<u8>, usize) {
    let mut out = vec![0u8; PAL_N * 4];
    let n = PAL_N.min(im.pal.len());
    let max_n = if n > 0 { im.pal[0].n as i64 } else { 1 };
    for i in 0..n {
        let p = &im.pal[i];
        let o = i << 2;
        out[o] = i as u8;
        out[o + 1] = clamp(idiv(p.n as i64 * 255 + (max_n >> 1), max_n), 0, 255) as u8;
        out[o + 2] = if im.pal.len() > 1 {
            clamp(idiv(p.lum_order as i64 * 255, im.pal.len() as i64 - 1), 0, 255) as u8
        } else {
            0
        };
        out[o + 3] = p.quantile as u8;
    }
    (out, n)
}

/// BOTH endpoint encodings, on purpose.  Quantile survives a rebuilt palette;
/// rank does not — which is exactly why rank agreement is STRONGER evidence
/// when it occurs: it means the palette was not rebuilt.  v2 forced the choice
/// at hash time.
pub fn sparse_rag(im: &Indexed) -> (Vec<u8>, usize) {
    let (w, h) = (im.w, im.h);
    let np = im.pal.len();
    let n_max = (np as i64 - 1).max(1);
    let rank_of = |v: usize| -> i64 { clamp(idiv(im.pal[v].lum_order as i64 * 255, n_max), 0, 255) };
    // An edge's key depends only on its two palette entries, and symmetrically,
    // so the per-pixel work is one increment in a dense table of unordered
    // palette pairs; keys are formed once per distinct pair afterwards.  The
    // reference hashed every pixel pair into a SipHash map.
    let mut pairs = vec![0u32; np * np];
    {
        let mut bump = |a: i32, b: i32| {
            if a < 0 || b < 0 || a == b {
                return;
            }
            let (lo, hi) = if a < b { (a as usize, b as usize) } else { (b as usize, a as usize) };
            pairs[lo * np + hi] += 1;
        };
        for y in 0..h {
            let row = &im.idx[y * w..y * w + w];
            for x in 0..w {
                let p = row[x];
                if x + 1 < w {
                    bump(p, row[x + 1]);
                }
                if y + 1 < h {
                    bump(p, im.idx[(y + 1) * w + x]);
                }
            }
        }
    }
    let mut keyed: Vec<(i32, i64)> = Vec::new();
    let mut total = 0i64;
    for a in 0..np {
        for b in a + 1..np {
            let c = pairs[a * np + b] as i64;
            if c == 0 {
                continue;
            }
            let (qa, qb) = (im.pal[a].quantile as i64, im.pal[b].quantile as i64);
            let (ra, rb) = (rank_of(a), rank_of(b));
            let (qlo, qhi) = if qa < qb { (qa, qb) } else { (qb, qa) };
            let (rlo, rhi) = if ra < rb { (ra, rb) } else { (rb, ra) };
            if qlo == qhi && rlo == rhi {
                continue;
            }
            // NOTE the i32: (qlo << 24) overflows into the sign bit exactly as
            // it does in JavaScript, and the tie-break below is therefore SIGNED.
            let k = ((qlo as i32) << 24) | ((qhi as i32) << 16) | ((rlo as i32) << 8) | rhi as i32;
            keyed.push((k, c));
            total += c;
        }
    }
    // distinct palette pairs can share a key: merge them
    keyed.sort_unstable_by_key(|e| e.0);
    let mut list: Vec<(i32, i64)> = Vec::with_capacity(keyed.len());
    for (k, c) in keyed {
        match list.last_mut() {
            Some(l) if l.0 == k => l.1 += c,
            _ => list.push((k, c)),
        }
    }
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    list.truncate(RAG_N);
    let mut out = vec![0u8; RAG_N * 6];
    for (i, &(k, cnt)) in list.iter().enumerate() {
        let o = i * 6;
        let k = k as u32;
        out[o] = ((k >> 24) & 255) as u8;
        out[o + 1] = ((k >> 16) & 255) as u8;
        out[o + 2] = ((k >> 8) & 255) as u8;
        out[o + 3] = (k & 255) as u8;
        let nrm = if total > 0 { clamp(idiv(cnt * 65535, total), 0, 65535) } else { 0 };
        out[o + 4] = (nrm & 255) as u8;
        out[o + 5] = ((nrm >> 8) & 255) as u8;
    }
    (out, list.len())
}

// ------------------------------------------------------------ shapes

struct Comp {
    id: i32,
    area: i64,
    cx: i64,
    cy: i64,
    minx: i64,
    maxx: i64,
    miny: i64,
    maxy: i64,
}

fn components(lab: &[i8], w: usize, h: usize) -> (Vec<i32>, Vec<Comp>) {
    let mut id = vec![-1i32; w * h];
    let mut out: Vec<Comp> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for s in 0..w * h {
        if id[s] >= 0 || lab[s] < 0 {
            continue;
        }
        let cid = out.len() as i32;
        let (mut area, mut sx, mut sy) = (0i64, 0i64, 0i64);
        let (mut minx, mut maxx, mut miny, mut maxy) = (w as i64, -1i64, h as i64, -1i64);
        stack.clear();
        stack.push(s);
        id[s] = cid;
        while let Some(p) = stack.pop() {
            let x = (p % w) as i64;
            let y = (p / w) as i64;
            area += 1;
            sx += x;
            sy += y;
            minx = minx.min(x);
            maxx = maxx.max(x);
            miny = miny.min(y);
            maxy = maxy.max(y);
            if x > 0 && id[p - 1] < 0 && lab[p - 1] == lab[p] {
                id[p - 1] = cid;
                stack.push(p - 1);
            }
            if (x as usize) < w - 1 && id[p + 1] < 0 && lab[p + 1] == lab[p] {
                id[p + 1] = cid;
                stack.push(p + 1);
            }
            if y > 0 && id[p - w] < 0 && lab[p - w] == lab[p] {
                id[p - w] = cid;
                stack.push(p - w);
            }
            if (y as usize) < h - 1 && id[p + w] < 0 && lab[p + w] == lab[p] {
                id[p + w] = cid;
                stack.push(p + w);
            }
        }
        out.push(Comp {
            id: cid,
            area,
            cx: idiv(sx, area),
            cy: idiv(sy, area),
            minx,
            maxx,
            miny,
            maxy,
        });
    }
    (id, out)
}

/// Connected components over an 8-band luminance-QUANTILE label map, modally
/// downsampled: quantile bands survive a tone curve and a rebuilt palette, and
/// the modal downsample kills the dither confetti that would otherwise shatter
/// every region into noise.
pub fn shape_signatures(im: &Indexed) -> (Vec<u8>, usize) {
    let long = im.w.max(im.h) as i64;
    let cell = idiv(long + 127, 128).max(1);
    let gw = (idiv(im.w as i64 + cell - 1, cell)).max(4) as usize;
    let gh = (idiv(im.h as i64 + cell - 1, cell)).max(4) as usize;
    let m = area_majority(im, gw, gh);
    let lab: Vec<i8> = m
        .iter()
        .map(|&v| if v < 0 { -1i8 } else { im.pal[v as usize].band as i8 })
        .collect();
    let (cid, comps) = components(&lab, gw, gh);
    let (w, h) = (gw, gh);

    let mut order: Vec<usize> = (0..comps.len()).collect();
    order.sort_by(|&a, &b| {
        comps[b]
            .area
            .cmp(&comps[a].area)
            .then(comps[a].miny.cmp(&comps[b].miny))
            .then(comps[a].minx.cmp(&comps[b].minx))
    });
    order.truncate(SHAPE_N);

    let mut out = vec![0u8; SHAPE_BYTES * SHAPE_N];
    for (s, &ci) in order.iter().enumerate() {
        let c = &comps[ci];
        let o = s * SHAPE_BYTES;

        // perimeter: cells with a 4-neighbour outside the component
        let mut per = 0i64;
        for y in c.miny..=c.maxy {
            for x in c.minx..=c.maxx {
                let p = (y as usize) * w + x as usize;
                if cid[p] != c.id {
                    continue;
                }
                if x == 0
                    || y == 0
                    || x == w as i64 - 1
                    || y == h as i64 - 1
                    || cid[p - 1] != c.id
                    || cid[p + 1] != c.id
                    || cid[p - w] != c.id
                    || cid[p + w] != c.id
                {
                    per += 1;
                }
            }
        }

        // holes: complement components inside the bbox that never touch its border
        let bw = (c.maxx - c.minx + 1) as usize;
        let bh = (c.maxy - c.miny + 1) as usize;
        let mut holes = 0i64;
        let mut seen = vec![0u8; bw * bh];
        let mut st: Vec<usize> = Vec::new();
        for y in 0..bh {
            for x in 0..bw {
                let q = y * bw + x;
                if seen[q] != 0 || cid[(y + c.miny as usize) * w + x + c.minx as usize] == c.id {
                    continue;
                }
                let mut touch = false;
                st.clear();
                st.push(q);
                seen[q] = 1;
                while let Some(r) = st.pop() {
                    let rx = r % bw;
                    let ry = r / bw;
                    if rx == 0 || ry == 0 || rx == bw - 1 || ry == bh - 1 {
                        touch = true;
                    }
                    let nb: [i64; 4] = [
                        if rx > 0 { r as i64 - 1 } else { -1 },
                        if rx < bw - 1 { r as i64 + 1 } else { -1 },
                        if ry > 0 { r as i64 - bw as i64 } else { -1 },
                        if ry < bh - 1 { r as i64 + bw as i64 } else { -1 },
                    ];
                    for t in nb {
                        if t < 0 {
                            continue;
                        }
                        let t = t as usize;
                        if seen[t] != 0 {
                            continue;
                        }
                        let tx = t % bw;
                        let ty = t / bw;
                        if cid[(ty + c.miny as usize) * w + tx + c.minx as usize] == c.id {
                            continue;
                        }
                        seen[t] = 1;
                        st.push(t);
                    }
                }
                if !touch {
                    holes += 1;
                }
            }
        }

        // the centroid must sit inside the component or the rays start outside
        let (mut cx, mut cy) = (c.cx, c.cy);
        if cid[(cy as usize) * w + cx as usize] != c.id {
            let mut bd = i64::MAX;
            for y in c.miny..=c.maxy {
                for x in c.minx..=c.maxx {
                    if cid[(y as usize) * w + x as usize] != c.id {
                        continue;
                    }
                    let d = (x - c.cx) * (x - c.cx) + (y - c.cy) * (y - c.cy);
                    if d < bd {
                        bd = d;
                        cx = x;
                        cy = y;
                    }
                }
            }
        }

        // 32 rays, integer DDA, distance to the FIRST contour crossing
        let mut rad = [0i64; 32];
        let mut rmax = 1i64;
        for k in 0..32 {
            let mut t2 = 0i64;
            let lim = (bw + bh) as i64;
            for step in 1..=lim {
                let px2 = cx + ((RAYC[k] as i64 * step + (RONE >> 1)) >> R);
                let py2 = cy + ((RAYSN[k] as i64 * step + (RONE >> 1)) >> R);
                if px2 < 0 || py2 < 0 || px2 >= w as i64 || py2 >= h as i64 {
                    break;
                }
                if cid[(py2 as usize) * w + px2 as usize] != c.id {
                    break;
                }
                t2 = step;
            }
            rad[k] = t2;
            rmax = rmax.max(t2);
        }

        let aspect = clamp(idiv(bw as i64 * 256, (bh as i64).max(1)), 0, 65535);
        let a = c.area as u32;
        out[o] = (a & 255) as u8;
        out[o + 1] = ((a >> 8) & 255) as u8;
        out[o + 2] = ((a >> 16) & 255) as u8;
        out[o + 3] = ((a >> 24) & 255) as u8;
        out[o + 4] = (per & 255) as u8;
        out[o + 5] = ((per >> 8) & 255) as u8;
        out[o + 6] = (aspect & 255) as u8;
        out[o + 7] = ((aspect >> 8) & 255) as u8;
        out[o + 8] = clamp(holes, 0, 255) as u8;
        for k in 0..32 {
            out[o + 9 + k] = clamp(idiv(255 * rad[k], rmax), 0, 255) as u8;
        }
    }
    (out, order.len())
}

// -------------------------------------------------------------- runs

fn run_bin(n: i32) -> usize {
    // RUN_LADDER's last finite rung is 90: every longer run lands in bin 15
    const LAST: i32 = 91;
    static BIN: [u8; LAST as usize + 1] = {
        let mut t = [0u8; LAST as usize + 1];
        let mut n = 0;
        while n <= LAST as usize {
            let mut i = 0;
            while RUN_LADDER[i] < n as i32 {
                i += 1;
            }
            t[n] = i as u8;
            n += 1;
        }
        t
    };
    if n <= LAST {
        BIN[n.max(0) as usize] as usize
    } else {
        15
    }
}

/// Contiguous same-index runs along H, V and the main diagonal, binned on a log
/// ladder.  Captures stroke texture with no colour in it at all.
///
/// All three directions are walked in ONE row-major pass: a column or a
/// diagonal carries its open run in a state array, so the vertical and
/// diagonal walks read memory in order instead of striding the whole image.
/// The histogram is a sum, so the order in which runs close does not matter.
pub fn run_lengths(im: &Indexed) -> Vec<u8> {
    let (w, h) = (im.w, im.h);
    let mut hist = [[0i64; 16], [0i64; 16], [0i64; 16]];
    if w > 0 && h > 0 {
        let nd = w + h - 1;
        let mut col_prev = im.idx[..w].to_vec();
        let mut col_run = vec![1i32; w];
        let mut dia_prev = vec![0i32; nd];
        let mut dia_run = vec![0i32; nd];
        for y in 0..h {
            let row = &im.idx[y * w..y * w + w];
            // horizontal
            let mut run = 1i32;
            let mut prev = row[0];
            for &v in row[1..].iter() {
                if v == prev {
                    run += 1;
                } else {
                    hist[0][run_bin(run)] += 1;
                    run = 1;
                    prev = v;
                }
            }
            hist[0][run_bin(run)] += 1;
            // vertical
            if y > 0 {
                for x in 0..w {
                    let v = row[x];
                    if v == col_prev[x] {
                        col_run[x] += 1;
                    } else {
                        hist[1][run_bin(col_run[x])] += 1;
                        col_run[x] = 1;
                        col_prev[x] = v;
                    }
                }
            }
            // diagonal x - y = d, indexed d + h - 1; it starts on row 0 or column 0
            for x in 0..w {
                let k = x + h - 1 - y;
                let v = row[x];
                if x == 0 || y == 0 {
                    dia_prev[k] = v;
                    dia_run[k] = 1;
                } else if v == dia_prev[k] {
                    dia_run[k] += 1;
                } else {
                    hist[2][run_bin(dia_run[k])] += 1;
                    dia_run[k] = 1;
                    dia_prev[k] = v;
                }
            }
        }
        for x in 0..w {
            hist[1][run_bin(col_run[x])] += 1;
        }
        // a diagonal of one pixel was never walked
        for k in 0..nd {
            let d = k as i64 - (h as i64 - 1);
            let (x0, y0) = (d.max(0) as usize, (-d).max(0) as usize);
            if (w - x0).min(h - y0) >= 2 {
                hist[2][run_bin(dia_run[k])] += 1;
            }
        }
    }
    let mut out = vec![0u8; 48];
    for s in 0..3 {
        let tot: i64 = hist[s].iter().sum();
        for i in 0..16 {
            out[s * 16 + i] = if tot > 0 {
                clamp(idiv(hist[s][i] * 255, tot), 0, 255) as u8
            } else {
                0
            };
        }
    }
    out
}

// ------------------------------------------------- local fingerprints

/// D4 index maps — the reference implementations walk them.
#[cfg(test)]
fn d4_maps() -> [[usize; 64]; 8] {
    let mut m = [[0usize; 64]; 8];
    for y in 0..8usize {
        for x in 0..8usize {
            let i = y * 8 + x;
            m[0][i] = y * 8 + x;
            m[1][i] = y * 8 + (7 - x);
            m[2][i] = (7 - y) * 8 + x;
            m[3][i] = (7 - y) * 8 + (7 - x);
            m[4][i] = x * 8 + y;
            m[5][i] = x * 8 + (7 - y);
            m[6][i] = (7 - x) * 8 + y;
            m[7][i] = (7 - x) * 8 + (7 - y);
        }
    }
    m
}

/// `med2` is the SUM of the two central order statistics, so the test is
/// `2v > s[31] + s[32]`.  Splitting at `s[32]` alone is not the symmetric
/// middle of 64 values, and an asymmetric threshold does not survive a
/// complement — which is exactly what the inversion fold needs it to do.
#[cfg(test)]
fn bits_of(g: &[i32; 64], med2: i64) -> (u32, u32) {
    let mut hi = 0u32;
    let mut lo = 0u32;
    for i in 0..32 {
        if 2 * g[i] as i64 > med2 {
            hi |= 1u32 << (31 - i);
        }
    }
    for i in 32..64 {
        if 2 * g[i] as i64 > med2 {
            lo |= 1u32 << (63 - i);
        }
    }
    (hi, lo)
}

/// Canonical over D4 x {identity, complement} — sixteen variants.
///
/// Complementing the bits is what an INVERTED palette does to this descriptor:
/// the median test simply reverses.  Folding the complement into the canonical
/// form makes an inverted copy produce byte-identical fingerprints, which a
/// `min(d, 64 - d)` distance at compare time cannot do — complementation
/// commutes with D4, so the canonical form of the complement is the complement
/// of the lexicographic MAXIMUM, not of the minimum, and the two do not meet.
/// The cost is real: light/dark polarity is discarded.
fn canonical64(cell: &[i32; 64], fold_invert: bool, maps: &[[usize; 64]; 8]) -> (u32, u32) {
    let _ = maps; // kept in the signature: the reference implementation below uses them
    let mut any = 0u32;
    for &v in cell.iter() {
        any |= v as u32;
    }
    if any < 1024 {
        let mut c: Cells = [0; 64];
        for (d, &v) in c.iter_mut().zip(cell.iter()) {
            *d = v as u16;
        }
        return canonical_cells(&c, fold_invert);
    }
    // outside the 0..1024 every caller stays inside: the plain selection
    let mut srt = *cell;
    let (_, m31, _) = srt.select_nth_unstable(31);
    let s31 = *m31;
    let mut r = 0u64;
    for (i, &v) in cell.iter().enumerate() {
        r |= ((v > s31) as u64) << i;
    }
    canonical_from_mask(r.reverse_bits(), fold_invert)
}

/// `canonical64` on cells already in `0..1024`.
///
/// The specified test is `2v > s[31] + s[32]`, and it is EXACTLY `v > s[31]`.
/// A value at or below s[31] cannot pass (2v <= 2·s31 <= s31 + s32).  A value
/// above s[31] sits at sorted position 32 or later, so v >= s32, and
/// 2v >= 2·s32 >= s31 + s32 with equality only when v == s32 == s31, which
/// v > s31 excludes.  So only the LOWER median is needed — one order
/// statistic, not two — and `simd::median_mask` finds it and the mask together
/// without sorting anything.  This runs once per candidate window, and on
/// dithered art that is close to once per pixel per window size.
#[inline]
fn canonical_cells(c: &Cells, fold_invert: bool) -> (u32, u32) {
    let (_, r) = median_mask(c);
    // cell index i lands at bit (63 - i), exactly the bit `bits_of` would set
    canonical_from_mask(r.reverse_bits(), fold_invert)
}

/// The sixteen D4 x {identity, complement} variants of a mask, minimised.
#[inline]
fn canonical_from_mask(b0: u64, fold_invert: bool) -> (u32, u32) {
    // The eight D4 variants are BIT PERMUTATIONS of one mask, so the threshold
    // runs once and the geometry runs on a u64.  Pack the identity mask with
    // cell index i at bit (63 - i) — exactly the bit `bits_of` would set, so
    // hi/lo are the top and bottom halves unchanged.  On that packing, byte
    // (7-y) holds row y with x at bit (7-x): a horizontal flip reverses the
    // bits of every byte, a vertical flip is `swap_bytes`, and the transpose is
    // the three-mask 8x8 bit-matrix exchange.  Per variant that is a handful of
    // scalar ops where the reference walks 64 permuted loads and 64 tests —
    // and this function runs sixteen times per candidate window, thousands of
    // windows per hash.
    let fx = |b: u64| -> u64 {
        // reverse the bits of every byte in parallel
        let b = ((b & 0x5555_5555_5555_5555) << 1) | ((b >> 1) & 0x5555_5555_5555_5555);
        let b = ((b & 0x3333_3333_3333_3333) << 2) | ((b >> 2) & 0x3333_3333_3333_3333);
        ((b & 0x0f0f_0f0f_0f0f_0f0f) << 4) | ((b >> 4) & 0x0f0f_0f0f_0f0f_0f0f)
    };
    let tr = |mut b: u64| -> u64 {
        // 8x8 bit-matrix transpose (Hacker's Delight 7-3)
        let t = (b ^ (b >> 7)) & 0x00aa_00aa_00aa_00aa;
        b ^= t ^ (t << 7);
        let t = (b ^ (b >> 14)) & 0x0000_cccc_0000_cccc;
        b ^= t ^ (t << 14);
        let t = (b ^ (b >> 28)) & 0x0000_0000_f0f0_f0f0;
        b ^ t ^ (t << 28)
    };
    let b0t = tr(b0);
    // maps[0..8] in their exact order: id, flip-x, flip-y, both, transpose and
    // the transpose's three flips
    let variants = [
        b0,
        fx(b0),
        b0.swap_bytes(),
        fx(b0.swap_bytes()),
        b0t,
        b0t.swap_bytes(),
        fx(b0t),
        fx(b0t).swap_bytes(),
    ];
    let mut best = u64::MAX;
    for &v in variants.iter() {
        if v < best {
            best = v;
        }
        if fold_invert && !v < best {
            best = !v;
        }
    }
    ((best >> 32) as u32, best as u32)
}

/// The map-walking reference `canonical64` is checked against, kept so the
/// bit-permutation version above is held to something written independently
/// of it.
#[cfg(test)]
fn canonical64_ref(cell: &[i32; 64], fold_invert: bool, maps: &[[usize; 64]; 8]) -> (u32, u32) {
    let mut srt = *cell;
    srt.sort_unstable();
    let med2 = srt[31] as i64 + srt[32] as i64;
    let mut best: Option<(u32, u32)> = None;
    let mut var = [0i32; 64];
    for m in maps.iter() {
        for k in 0..64 {
            var[k] = cell[m[k]];
        }
        let b = bits_of(&var, med2);
        if best.is_none() || b < best.unwrap() {
            best = Some(b);
        }
        if fold_invert {
            let c = (!b.0, !b.1);
            if c < best.unwrap() {
                best = Some(c);
            }
        }
    }
    best.unwrap_or((0, 0))
}

fn mix64(hi: u32, lo: u32) -> u32 {
    let mut a = hi ^ 0x9e37_79b9;
    let mut b = lo ^ 0x85eb_ca6b;
    a = (a ^ (a >> 16)).wrapping_mul(0x7feb_352d);
    b = (b ^ (b >> 15)).wrapping_mul(0x846c_a68b);
    a ^= b;
    a = (a ^ (a >> 13)).wrapping_mul(0xc2b2_ae35);
    a ^ (a >> 16)
}

fn integral_u8(map: &[u8], w: usize, h: usize) -> Vec<i64> {
    let mut s = vec![0i64; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0i64;
        for x in 0..w {
            row += map[y * w + x] as i64;
            s[(y + 1) * (w + 1) + x + 1] = s[y * (w + 1) + x + 1] + row;
        }
    }
    s
}


/// Candidate positions: a STRICT LOCAL MAXIMUM of a content function, decided
/// inside a fixed pixel radius and nowhere else.  Ties break on a hash of the
/// patch GRADIENT rather than position, because position is what a crop
/// changes — and gradient rather than level, because level does not survive an
/// inversion and the complement fold then never gets the chance to fire.
///
/// The order is packed into one integer, `e · 2^32 + tie`, with 0 standing for
/// "no candidate" (the reference used -1; every real value is above both).  A
/// peak is a pixel that equals the maximum of its (2r+1)² window, the window
/// clipped to the frame — and since every value is non-negative, clipping is
/// the same as padding with 0, which is what lets the maximum be computed by
/// the van Herk / Gil-Werman scheme: block prefix and suffix maxima, three
/// branch-free `max` per element per axis, instead of the monotonic deques,
/// whose pops were the least predictable branches in the hash.
fn content_peaks(qmap: &[i32], sw: usize, sh: usize, r: usize) -> Vec<(i64, i64, i64)> {
    let n = sw * sh;
    let mut pack = vec![0u64; n];
    if sw >= 3 && sh >= 3 {
        for y in 1..sh - 1 {
            let (r0, r1, r2) = (&qmap[(y - 1) * sw..y * sw], &qmap[y * sw..(y + 1) * sw], &qmap[(y + 1) * sw..(y + 2) * sw]);
            let out = &mut pack[y * sw..(y + 1) * sw];
            for x in 1..sw - 1 {
                let c = r1[x];
                if c < 0 {
                    continue;
                }
                // the nine neighbours in the reference's order: dy, then dx
                let nb = [r0[x - 1], r0[x], r0[x + 1], r1[x - 1], c, r1[x + 1], r2[x - 1], r2[x], r2[x + 1]];
                let mut e = 0u32;
                let mut t = 0u32;
                for &v in nb.iter() {
                    let g = if v < 0 { 64 } else { (v - c).unsigned_abs() };
                    e += g;
                    t = t.wrapping_mul(31).wrapping_add(g);
                }
                if e > 0 {
                    out[x] = ((e as u64) << 32) | mix64(t, e) as u64;
                }
            }
        }
    }
    let row_max = max_filter_rows(&pack, sw, sh, r);
    let col_max = max_filter_cols(&row_max, sw, sh, r);
    let mut pts = Vec::new();
    if sh > 2 * r && sw > 2 * r {
        for y in r..sh - r {
            for x in r..sw - r {
                let p = y * sw + x;
                if pack[p] > 0 && pack[p] == col_max[p] {
                    pts.push(((x as i64) << 1, (y as i64) << 1, (pack[p] >> 32) as i64));
                }
            }
        }
    }
    pts
}

/// `out[i] = max(a[i - r ..= i + r])` over each row, the window clipped to the
/// row (equivalently: padded with 0, which no value is below).
fn max_filter_rows(a: &[u64], sw: usize, sh: usize, r: usize) -> Vec<u64> {
    let k = 2 * r + 1;
    let len = sw + 2 * r;
    let mut pad = vec![0u64; len];
    let mut g = vec![0u64; len];
    let mut h = vec![0u64; len];
    let mut out = vec![0u64; sw * sh];
    for y in 0..sh {
        pad[r..r + sw].copy_from_slice(&a[y * sw..(y + 1) * sw]);
        // prefix maxima within blocks of k, then suffix maxima
        let mut i = 0;
        while i < len {
            let end = (i + k).min(len);
            g[i] = pad[i];
            for j in i + 1..end {
                g[j] = g[j - 1].max(pad[j]);
            }
            h[end - 1] = pad[end - 1];
            for j in (i..end - 1).rev() {
                h[j] = h[j + 1].max(pad[j]);
            }
            i = end;
        }
        // the window [x, x + 2r] in padded coordinates straddles at most two
        // blocks: the suffix of the first and the prefix of the second
        let o = &mut out[y * sw..(y + 1) * sw];
        for x in 0..sw {
            o[x] = h[x].max(g[x + 2 * r]);
        }
    }
    out
}

/// The same over columns, computed row by row so every pass is sequential.
fn max_filter_cols(a: &[u64], sw: usize, sh: usize, r: usize) -> Vec<u64> {
    let k = 2 * r + 1;
    let len = sh + 2 * r;
    // padded rows: row i of the padded column space is a's row i - r, or 0
    let row = |i: usize| -> Option<&[u64]> {
        if i >= r && i < r + sh {
            Some(&a[(i - r) * sw..(i - r + 1) * sw])
        } else {
            None
        }
    };
    let mut g = vec![0u64; len * sw];
    let mut h = vec![0u64; len * sw];
    let mut i = 0;
    while i < len {
        let end = (i + k).min(len);
        for j in i..end {
            let (gp, gc) = g.split_at_mut(j * sw);
            let gc = &mut gc[..sw];
            match (row(j), j > i) {
                (Some(src), true) => {
                    let gprev = &gp[(j - 1) * sw..j * sw];
                    for x in 0..sw {
                        gc[x] = gprev[x].max(src[x]);
                    }
                }
                (Some(src), false) => gc.copy_from_slice(src),
                (None, true) => {
                    let gprev = &gp[(j - 1) * sw..j * sw];
                    gc.copy_from_slice(gprev);
                }
                (None, false) => {}
            }
        }
        for j in (i..end).rev() {
            let (hc, hn) = h.split_at_mut((j + 1) * sw);
            let hc = &mut hc[j * sw..];
            match (row(j), j + 1 < end) {
                (Some(src), true) => {
                    let hnext = &hn[..sw];
                    for x in 0..sw {
                        hc[x] = hnext[x].max(src[x]);
                    }
                }
                (Some(src), false) => hc.copy_from_slice(src),
                (None, true) => {
                    let hnext = &hn[..sw];
                    hc.copy_from_slice(hnext);
                }
                (None, false) => {}
            }
        }
        i = end;
    }
    let mut out = vec![0u64; sw * sh];
    for y in 0..sh {
        let (hr, gr) = (&h[y * sw..(y + 1) * sw], &g[(y + 2 * r) * sw..(y + 2 * r + 1) * sw]);
        let o = &mut out[y * sw..(y + 1) * sw];
        for x in 0..sw {
            o[x] = hr[x].max(gr[x]);
        }
    }
    out
}

/// The one or two window origins a peak nominates along one axis, clamped to
/// the frame and deduplicated in order — the reference allocated a `Vec` for
/// this once per peak per axis per window size.
#[inline]
fn offsets(c2: i64, win: i64, span: i64) -> ([i64; 2], usize) {
    let num = c2 + 1 - win;
    let base = num >> 1;
    let v0 = clamp(base, 0, span - win);
    if num & 1 != 0 {
        let v1 = clamp(base + 1, 0, span - win);
        if v1 != v0 {
            return ([v0, v1], 2);
        }
    }
    ([v0, 0], 1)
}

pub struct LocalOut {
    pub bytes: Vec<u8>,
    pub pos: Vec<u8>,
    pub count: usize,
}

/// The 2x2 block medians a 16-pixel window reads, stored as four phase planes.
///
/// A q = 2 window at (x0, y0) reads the third smallest of each 2x2 block at
/// (x0 + 2cx, y0 + 2cy) — the `(q·q) >> 1`-th order statistic, exactly what the
/// reference selected per cell per window.  Windows on dithered art overlap
/// almost everywhere, so every block median is computed once per image; and
/// because a window only ever reads blocks of ONE parity in x and y, storing
/// the four parities as separate planes turns its 64 stride-2 reads into eight
/// contiguous rows.
struct Median2 {
    pw: usize,
    ph: usize,
    planes: Vec<u16>,
}

impl Median2 {
    fn build(qv: &[u16], sw: usize, sh: usize) -> Median2 {
        let (pw, ph) = ((sw + 1) / 2, (sh + 1) / 2);
        let mut planes = vec![0u16; 4 * pw * ph];
        if sw >= 2 && sh >= 2 {
            for y in 0..sh - 1 {
                let r0 = &qv[y * sw..(y + 1) * sw];
                let r1 = &qv[(y + 1) * sw..(y + 2) * sw];
                for x in 0..sw - 1 {
                    let (a, b, c, d) = (r0[x], r0[x + 1], r1[x], r1[x + 1]);
                    let (l1, h1) = (a.min(b), a.max(b));
                    let (l2, h2) = (c.min(d), c.max(d));
                    // sorted[2] of four = the larger of (the smaller top, the larger bottom)
                    let v = h1.min(h2).max(l1.max(l2));
                    let plane = ((y & 1) << 1) | (x & 1);
                    planes[plane * pw * ph + (y >> 1) * pw + (x >> 1)] = v;
                }
            }
        }
        Median2 { pw, ph, planes }
    }

    /// The eight cells of window row `cy`, for a window whose origin is (x0, y0).
    #[inline(always)]
    fn row(&self, x0: usize, y0: usize, cy: usize) -> &[u16] {
        let plane = ((y0 & 1) << 1) | (x0 & 1);
        let o = plane * self.pw * self.ph + ((y0 >> 1) + cy) * self.pw + (x0 >> 1);
        &self.planes[o..o + 8]
    }
}

#[derive(Clone, Copy)]
struct Cand {
    key: u32,
    hi: u32,
    lo: u32,
    x: i64,
    y: i64,
    size: i64,
}

/// The `want` smallest DISTINCT codes under (key, hi, lo), each at the
/// position the reference's stable sort on (key, hi, lo, x, y) would have
/// reached first — without holding every candidate.
///
/// The reference collected every window (hundreds of thousands on a dithered
/// 1024-pixel work), sorted them all, and walked the front.  `key` is a
/// function of (hi, lo), so equal codes are adjacent in that order and the
/// survivor of a code is its least (x, y), earliest inserted on a tie.
///
/// Here every code is held once: a repeat either moves its code to a strictly
/// smaller (x, y) or is dropped, which keeps the earliest of equal positions.
/// When more than `CAP` codes are held, the buffer is sorted, cut to `want`,
/// and the last survivor becomes a threshold that no later candidate above it
/// can ever get under, so those are dropped on one comparison.  Dithered art
/// repeats a few textures thousands of times, and those repeats are now a
/// table probe each rather than a stream of sorts.
struct TopCodes {
    want: usize,
    buf: Vec<Cand>,
    /// open addressing on `key` (already a mixed hash of the code): slot ->
    /// 1 + index into `buf`, 0 = empty
    slots: Vec<u32>,
    thresh: Option<(u32, u32, u32)>,
}

impl TopCodes {
    const CAP: usize = 4096;
    const SLOTS: usize = 2 * Self::CAP;

    fn new(want: usize) -> Self {
        TopCodes {
            want,
            buf: Vec::with_capacity(Self::CAP + 1),
            slots: vec![0u32; Self::SLOTS],
            thresh: None,
        }
    }

    #[inline]
    fn push(&mut self, c: Cand) {
        if let Some(t) = self.thresh {
            if (c.key, c.hi, c.lo) > t {
                return;
            }
        }
        let mask = Self::SLOTS - 1;
        let mut s = c.key as usize & mask;
        loop {
            let e = self.slots[s];
            if e == 0 {
                break;
            }
            let held = &mut self.buf[(e - 1) as usize];
            if held.hi == c.hi && held.lo == c.lo {
                if (c.x, c.y) < (held.x, held.y) {
                    held.x = c.x;
                    held.y = c.y;
                    held.size = c.size;
                }
                return;
            }
            s = (s + 1) & mask;
        }
        self.buf.push(c);
        self.slots[s] = self.buf.len() as u32;
        if self.buf.len() > Self::CAP {
            self.compact();
        }
    }

    fn compact(&mut self) {
        // codes are distinct here, so (key, hi, lo) is already a total order
        self.buf.sort_unstable_by(|a, b| (a.key, a.hi, a.lo).cmp(&(b.key, b.hi, b.lo)));
        self.buf.truncate(self.want);
        if self.want > 0 && self.buf.len() == self.want {
            let l = self.buf[self.want - 1];
            self.thresh = Some((l.key, l.hi, l.lo));
        }
        for v in self.slots.iter_mut() {
            *v = 0;
        }
        let mask = Self::SLOTS - 1;
        for (i, c) in self.buf.iter().enumerate() {
            let mut s = c.key as usize & mask;
            while self.slots[s] != 0 {
                s = (s + 1) & mask;
            }
            self.slots[s] = i as u32 + 1;
        }
    }

    fn finish(mut self) -> Vec<Cand> {
        self.compact();
        self.buf
    }
}

/// The collage channel, and the one that decides whether a sprite buried in a
/// busy scene is ever found.  Keeps the `localCount` SMALLEST scrambled keys —
/// a MinHash selection, so the decision to keep a window depends only on that
/// window's own content and a drawing hashed alone nominates the same windows
/// as the same drawing pasted into a scene.
///
/// Byte-identical to the reference kept under `cfg(test)` below; what changed
/// is only how the same windows are evaluated:
///   * a window position is evaluated once per window size — on dithered art
///     peaks are dense and most windows were nominated several times over;
///   * 16-pixel windows read a precomputed 2x2 block-median map instead of
///     sorting four values per cell (the default sizes are 8 and 16, so the
///     per-cell selection is gone entirely; other sizes keep it);
///   * the selection streams through `TopCodes` instead of sorting every
///     candidate the image produced.
pub fn local_fingerprints(im: &Indexed, cfg: &Config) -> LocalOut {
    let (sw, sh) = (im.w, im.h);
    let qmap: Vec<i32> = im
        .idx
        .iter()
        .map(|&v| if v < 0 { -1 } else { im.pal[v as usize].quantile })
        .collect();
    let op: Vec<u8> = im.idx.iter().map(|&v| if v < 0 { 0u8 } else { 1u8 }).collect();
    let so = integral_u8(&op, sw, sh);
    let pts = content_peaks(&qmap, sw, sh, cfg.peak_radius as usize);
    // the transparent-or-scale transform depends only on the pixel, so it runs
    // once per pixel here rather than once per gather; every value is in
    // 2..=514, so it travels as u16 and eight cells fill one vector
    let qv: Vec<u16> = qmap.iter().map(|&v| if v < 0 { 257 } else { (2 * (v + 1)) as u16 }).collect();
    let mut med2: Option<Median2> = None;

    let mut top = TopCodes::new(cfg.local_count);
    let mut cell: Cells = [0; 64];
    let mut scratch: Vec<u16> = Vec::new();
    let mut done_wins: Vec<i64> = Vec::new();
    let mut seen: Vec<u64> = Vec::new();
    for &win in cfg.local_windows.iter() {
        let win = win as i64;
        if (sw as i64) < win || (sh as i64) < win {
            continue;
        }
        let q = (win >> 3) as usize;
        // A window narrower than one 8x8 cell grid has no cells; the reference
        // indexed an empty selection here.  Config validation refuses it.
        if q == 0 {
            continue;
        }
        // The same size listed twice re-nominates the same windows, which can
        // only produce duplicates the selection discards anyway.
        if done_wins.contains(&win) {
            continue;
        }
        done_wins.push(win);
        if q == 2 && med2.is_none() {
            med2 = Some(Median2::build(&qv, sw, sh));
        }
        seen.clear();
        seen.resize((sw * sh + 63) >> 6, 0);
        let area = win * win;
        for &(x2, y2, _sal) in pts.iter() {
            let (xs, xn) = offsets(x2, win, sw as i64);
            let (ys, yn) = offsets(y2, win, sh as i64);
            for &x in xs[..xn].iter() {
                for &y in ys[..yn].iter() {
                    if x < 0 || y < 0 || x + win > sw as i64 || y + win > sh as i64 {
                        continue;
                    }
                    let (x0, y0) = (x as usize, y as usize);
                    let at = y0 * sw + x0;
                    if seen[at >> 6] & (1u64 << (at & 63)) != 0 {
                        continue;
                    }
                    seen[at >> 6] |= 1u64 << (at & 63);
                    // the opacity floor is what lets a sprite hashed on its own
                    // match the same sprite composited onto a host: admit
                    // half-transparent windows and the bits encode the
                    // SILHOUETTE, which the composite does not have
                    let w1 = w_box(&so, sw, x0, y0, win as usize);
                    if w1 * 4 < area * 3 {
                        continue;
                    }
                    match q {
                        1 => {
                            for cy in 0..8usize {
                                let row = &qv[(y0 + cy) * sw + x0..(y0 + cy) * sw + x0 + 8];
                                cell[cy * 8..cy * 8 + 8].copy_from_slice(row);
                            }
                        }
                        2 => {
                            let m = med2.as_ref().unwrap();
                            for cy in 0..8usize {
                                cell[cy * 8..cy * 8 + 8].copy_from_slice(m.row(x0, y0, cy));
                            }
                        }
                        _ => {
                            // the (nv >> 1)-th ORDER STATISTIC of the sub-block —
                            // the same value whether the rest is sorted around
                            // it or not, so a selection serves
                            scratch.resize(q * q, 0);
                            for cy in 0..8usize {
                                for cx in 0..8usize {
                                    let mut nv = 0usize;
                                    for by in 0..q {
                                        let row = (y0 + cy * q + by) * sw + x0 + cx * q;
                                        scratch[nv..nv + q].copy_from_slice(&qv[row..row + q]);
                                        nv += q;
                                    }
                                    let m = nv >> 1;
                                    scratch[..nv].select_nth_unstable(m);
                                    cell[cy * 8 + cx] = scratch[m];
                                }
                            }
                        }
                    }
                    let (hi, lo) = canonical_cells(&cell, cfg.fold_invert);
                    // an all-zero code collides with every featureless patch in
                    // every work ever hashed, INCLUDING its own rotations, so it
                    // poisons the empirical null as well as the count
                    let pc = hi.count_ones() + lo.count_ones();
                    if pc < 6 || pc > 58 {
                        continue;
                    }
                    top.push(Cand { key: mix64(hi, lo), hi, lo, x, y, size: win });
                }
            }
        }
    }

    let mut wire = top.finish();
    wire.sort_by(|a, b| a.hi.cmp(&b.hi).then(a.lo.cmp(&b.lo)));

    let cap = 128usize;
    let mut out = vec![0u8; cap * 8];
    let mut pos = vec![0u8; cap * 4];
    let max_dim = im.w.max(im.h) as i64;
    for (i, e) in wire.iter().take(cap).enumerate() {
        let o = i << 3;
        let qo = i << 2;
        out[o..o + 4].copy_from_slice(&e.hi.to_le_bytes());
        out[o + 4..o + 8].copy_from_slice(&e.lo.to_le_bytes());
        // ASPECT-TRUE u16 in units of 1/65535 of max(w,h), the SAME frame the
        // keypoints use (SPEC-003 §5.6)
        let px = clamp(idiv((e.x + (e.size >> 1)) * 65535, max_dim), 0, 65535) as u16;
        let py = clamp(idiv((e.y + (e.size >> 1)) * 65535, max_dim), 0, 65535) as u16;
        pos[qo..qo + 2].copy_from_slice(&px.to_le_bytes());
        pos[qo + 2..qo + 4].copy_from_slice(&py.to_le_bytes());
    }
    LocalOut { bytes: out, pos, count: wire.len().min(cap) }
}

/// `canonical64` no longer walks index maps; the parameter stays for the
/// reference implementations under test.
const D4_UNUSED: [[usize; 64]; 8] = [[0; 64]; 8];

#[inline]
fn w_box(s: &[i64], w: usize, x: usize, y: usize, win: usize) -> i64 {
    let ws = w + 1;
    s[(y + win) * ws + x + win] - s[y * ws + x + win] - s[(y + win) * ws + x] + s[y * ws + x]
}

// -------------------------------------------------------- silhouette

/// Its own channel, on purpose.  Two sprites cut from the same sheet share a
/// silhouette exactly; the same sprite composited into a scene has no
/// silhouette at all.  Strong when measurable, ZERO information when not —
/// which is the shape of a channel that must be able to abstain.
pub fn silhouette(im: &Indexed) -> (Vec<u8>, bool) {
    let (w, h) = (im.w, im.h);
    let n = w * h;
    let mut out = vec![0u8; 96];
    let opaque = im.idx.iter().filter(|&&v| v >= 0).count() as i64;
    if opaque == 0 || opaque * 100 > n as i64 * 98 || opaque * 100 < n as i64 * 2 {
        return (out, false);
    }
    let mut id = vec![-1i32; n];
    let mut stack: Vec<usize> = Vec::new();
    let mut comps: Vec<Comp> = Vec::new();
    let (mut best, mut best_area) = (-1i32, 0i64);
    for s0 in 0..n {
        if id[s0] >= 0 || im.idx[s0] < 0 {
            continue;
        }
        let cid = comps.len() as i32;
        let (mut area, mut sx, mut sy) = (0i64, 0i64, 0i64);
        let (mut minx, mut maxx, mut miny, mut maxy) = (w as i64, -1i64, h as i64, -1i64);
        stack.clear();
        stack.push(s0);
        id[s0] = cid;
        while let Some(p) = stack.pop() {
            let px = (p % w) as i64;
            let py = (p / w) as i64;
            area += 1;
            sx += px;
            sy += py;
            minx = minx.min(px);
            maxx = maxx.max(px);
            miny = miny.min(py);
            maxy = maxy.max(py);
            if px > 0 && id[p - 1] < 0 && im.idx[p - 1] >= 0 {
                id[p - 1] = cid;
                stack.push(p - 1);
            }
            if (px as usize) < w - 1 && id[p + 1] < 0 && im.idx[p + 1] >= 0 {
                id[p + 1] = cid;
                stack.push(p + 1);
            }
            if py > 0 && id[p - w] < 0 && im.idx[p - w] >= 0 {
                id[p - w] = cid;
                stack.push(p - w);
            }
            if (py as usize) < h - 1 && id[p + w] < 0 && im.idx[p + w] >= 0 {
                id[p + w] = cid;
                stack.push(p + w);
            }
        }
        if area > best_area {
            best_area = area;
            best = cid;
        }
        comps.push(Comp { id: cid, area, cx: idiv(sx, area), cy: idiv(sy, area), minx, maxx, miny, maxy });
    }
    if best < 0 {
        return (out, false);
    }
    let c = &comps[best as usize];
    let bw = c.maxx - c.minx + 1;
    let bh = c.maxy - c.miny + 1;

    let (mut cx, mut cy) = (c.cx, c.cy);
    if id[(cy as usize) * w + cx as usize] != c.id {
        let mut bd = i64::MAX;
        for y in c.miny..=c.maxy {
            for x in c.minx..=c.maxx {
                if id[(y as usize) * w + x as usize] != c.id {
                    continue;
                }
                let d = (x - c.cx) * (x - c.cx) + (y - c.cy) * (y - c.cy);
                if d < bd {
                    bd = d;
                    cx = x;
                    cy = y;
                }
            }
        }
    }
    let mut rad = [0i64; 32];
    let mut rmax = 1i64;
    for k in 0..32 {
        let mut t2 = 0i64;
        for step in 1..=(bw + bh) {
            let rx = cx + ((RAYC[k] as i64 * step + (RONE >> 1)) >> R);
            let ry = cy + ((RAYSN[k] as i64 * step + (RONE >> 1)) >> R);
            if rx < 0 || ry < 0 || rx >= w as i64 || ry >= h as i64 {
                break;
            }
            if id[(ry as usize) * w + rx as usize] != c.id {
                break;
            }
            t2 = step;
        }
        rad[k] = t2;
        rmax = rmax.max(t2);
    }
    for k in 0..32 {
        out[k] = clamp(idiv(255 * rad[k], rmax), 0, 255) as u8;
    }

    let (mut m20, mut m02, mut m11, mut cnt) = (0i64, 0i64, 0i64, 0i64);
    for y in c.miny..=c.maxy {
        for x in c.minx..=c.maxx {
            if id[(y as usize) * w + x as usize] != c.id {
                continue;
            }
            let dx = x - cx;
            let dy = y - cy;
            m20 += dx * dx;
            m02 += dy * dy;
            m11 += dx * dy;
            cnt += 1;
        }
    }
    let norm = (cnt * (bw * bw + bh * bh)).max(1);
    out[32] = clamp(idiv(m20 * 1020, norm), 0, 255) as u8;
    out[33] = clamp(idiv(m02 * 1020, norm), 0, 255) as u8;
    out[34] = clamp(idiv(m11.abs() * 1020, norm), 0, 255) as u8;
    out[35] = if m11 < 0 { 1 } else { 0 };
    let asp = clamp(idiv(bw * 256, bh.max(1)), 0, 65535) as u16;
    out[36..38].copy_from_slice(&asp.to_le_bytes());
    out[38] = clamp(idiv(c.area * 255, (bw * bh).max(1)), 0, 255) as u8;
    out[39] = clamp(comps.len() as i64, 0, 255) as u8;

    let mut row_t = [0i64; 8];
    let mut col_t = [0i64; 8];
    for y in 0..h {
        let mut t = 0usize;
        let mut prev = 0i32;
        for x in 0..w {
            let v = if im.idx[y * w + x] >= 0 { 1 } else { 0 };
            if v != prev {
                t += 1;
            }
            prev = v;
        }
        row_t[t.min(7)] += 1;
    }
    for x in 0..w {
        let mut t = 0usize;
        let mut prev = 0i32;
        for y in 0..h {
            let v = if im.idx[y * w + x] >= 0 { 1 } else { 0 };
            if v != prev {
                t += 1;
            }
            prev = v;
        }
        col_t[t.min(7)] += 1;
    }
    for i in 0..8 {
        out[40 + i] = clamp(idiv(row_t[i] * 255, h as i64), 0, 255) as u8;
        out[48 + i] = clamp(idiv(col_t[i] * 255, w as i64), 0, 255) as u8;
    }

    let mut cell = [0i32; 64];
    for gy in 0..8i64 {
        for gx in 0..8i64 {
            let x0 = c.minx + idiv(gx * bw, 8);
            let mut x1 = c.minx + idiv((gx + 1) * bw, 8);
            let y0 = c.miny + idiv(gy * bh, 8);
            let mut y1 = c.miny + idiv((gy + 1) * bh, 8);
            if x1 <= x0 {
                x1 = x0 + 1;
            }
            if y1 <= y0 {
                y1 = y0 + 1;
            }
            let (mut on, mut tot) = (0i64, 0i64);
            for y in y0..y1.min(h as i64) {
                for x in x0..x1.min(w as i64) {
                    tot += 1;
                    if id[(y as usize) * w + x as usize] == c.id {
                        on += 1;
                    }
                }
            }
            cell[(gy * 8 + gx) as usize] = if tot > 0 { idiv(on * 255, tot) as i32 } else { 0 };
        }
    }
    let (hi, lo) = canonical64(&cell, false, &D4_UNUSED);
    out[56..60].copy_from_slice(&hi.to_le_bytes());
    out[60..64].copy_from_slice(&lo.to_le_bytes());
    let frac = clamp(idiv(opaque * 65535, n as i64), 0, 65535) as u16;
    out[64..66].copy_from_slice(&frac.to_le_bytes());
    (out, true)
}

// ----------------------------------------------------- colour digest

/// REPORTING ONLY.  This section MUST NOT contribute to any score: recolour
/// invariance is load-bearing and dies the moment absolute RGB enters the
/// scoring path.  Conformance: zeroing it MUST NOT change any verdict.
pub fn colour_digest(im: &Indexed) -> (Vec<u8>, usize) {
    let mut out = vec![0u8; 80];
    let n = 16.min(im.pal.len());
    let max_n = if im.pal.is_empty() { 1 } else { im.pal[0].n as i64 };
    for i in 0..n {
        let p = &im.pal[i];
        let o = i * 5;
        out[o] = p.r;
        out[o + 1] = p.g;
        out[o + 2] = p.b;
        out[o + 3] = clamp(idiv(p.n as i64 * 255 + (max_n >> 1), max_n), 0, 255) as u8;
        out[o + 4] = p.quantile as u8;
    }
    (out, n)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// D4 as bit permutations must equal D4 as index maps, variant order and
    /// all — on dense, sparse, tied and constant cells, with and without the
    /// inversion fold.  One disagreeing bit here is one wrong local code on
    /// the wire, so this is exhaustive-ish rather than a smoke test.
    #[test]
    fn canonical64_bitperm_matches_reference() {
        let maps = d4_maps();
        let mut s = 0x1234_5678_9abc_def0u64;
        let mut r = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for case in 0..2000 {
            let mut cell = [0i32; 64];
            for v in cell.iter_mut() {
                *v = match case % 5 {
                    0 => (r() % 512) as i32,
                    1 => (r() % 3) as i32 * 257,      // heavy ties
                    2 => 7,                            // constant
                    3 => (r() % 2) as i32,             // binary
                    _ => 2 * ((r() % 256) as i32 + 1), // the caller's actual range
                };
            }
            for fold in [false, true] {
                assert_eq!(
                    canonical64(&cell, fold, &maps),
                    canonical64_ref(&cell, fold, &maps),
                    "case {case} fold {fold}"
                );
            }
        }
    }
}
