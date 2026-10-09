//! Wire 4's claim, tested: hash a work and any of its seven images under the
//! symmetries of the square, and every section wire 4 resampled comes back
//! as the same section moved by that symmetry — exactly, on canvases of any
//! size (docs/SPEC-W4-paph-wire4.md §7).  Wire 3 fails the same checks on
//! canvases whose sides are not multiples of 16, which is why it changed.
#![cfg(test)]

use crate::config::{Config, WIRE_3, WIRE_4};
use crate::front::normalise;
use crate::golden::{tie_canvas, W4_TIES};
use crate::keypoints::{pattern, RotCache};
use crate::prepared::Prepared;
use crate::sections::{canonical_profile, dct2_exact, silhouette_settled_by, thumbnail16};
use crate::synth::{pixel_art, work, Img};
use crate::wire::{hash, parse_t1, Tier1, F_SIL};
use crate::x::profile::XBound;
use crate::x::route::XRoute;

/// A symmetry of the square as (swap the axes, then flip x, then flip y).
#[derive(Clone, Copy, Debug)]
pub struct G {
    pub swap: bool,
    pub fx: bool,
    pub fy: bool,
}

pub const D4: [G; 7] = [
    G { swap: false, fx: true, fy: false },  // mirror
    G { swap: false, fx: false, fy: true },  // flip vertically
    G { swap: false, fx: true, fy: true },   // half turn
    G { swap: true, fx: false, fy: false },  // transpose
    G { swap: true, fx: true, fy: false },   // quarter turn
    G { swap: true, fx: false, fy: true },   // three quarters
    G { swap: true, fx: true, fy: true },    // anti-transpose
];

impl G {
    /// Where pixel (x, y) of a w × h image lands, and the new size.
    pub fn map(self, x: usize, y: usize, w: usize, h: usize) -> (usize, usize, usize, usize) {
        let (x1, y1, w1, h1) = if self.swap { (y, x, h, w) } else { (x, y, w, h) };
        let x2 = if self.fx { w1 - 1 - x1 } else { x1 };
        let y2 = if self.fy { h1 - 1 - y1 } else { y1 };
        (x2, y2, w1, h1)
    }
    pub fn image(self, a: &Img) -> Img {
        let (_, _, w1, h1) = self.map(0, 0, a.w, a.h);
        let mut o = Img::new(w1, h1);
        for y in 0..a.h {
            for x in 0..a.w {
                let (x2, y2, _, _) = self.map(x, y, a.w, a.h);
                o.set(x2 as i64, y2 as i64, a.get(x, y));
            }
        }
        o
    }
    /// Where ray k (direction 2πk/32, y down) points after the symmetry.
    pub fn ray(self, k: usize) -> usize {
        let mut k = k as i64;
        if self.swap {
            k = 8 - k;
        }
        if self.fx {
            k = 16 - k;
        }
        if self.fy {
            k = -k;
        }
        k.rem_euclid(32) as usize
    }
}

/// Every pixel-art family at sizes that are and are not multiples of 16,
/// below and above the shape grid's 128, on transparency, a matte and a
/// dithered backdrop.
fn works() -> Vec<(String, Img)> {
    let mut v = Vec::new();
    let sizes = [(37, 29), (64, 48), (53, 91), (96, 96), (130, 77), (61, 200), (203, 151), (17, 9), (5, 13), (300, 47)];
    for (i, &(w, h)) in sizes.iter().enumerate() {
        for bg in 0..3u8 {
            let seed = 7 + 31 * i as u64 + 1000 * bg as u64;
            v.push((format!("art-{w}x{h}-bg{bg}"), pixel_art(w, h, seed, 3 + (i * 5) % 14, bg)));
        }
        v.push((format!("work-{w}x{h}"), work(w, h, (w * 13 + h) as i64, i % 2 == 0)));
    }
    // and a seeded spread of sizes and palettes, a third of them blown up
    // by an integer factor the front end divides out again
    let mut r = crate::synth::Rng(0x0057_4934_d4d4_0004);
    for i in 0..36 {
        let (w, h) = (6 + r.below(250) as usize, 6 + r.below(250) as usize);
        let mut im = pixel_art(w, h, r.next(), 2 + r.below(30) as usize, r.below(3) as u8);
        if i % 3 == 0 {
            im = crate::synth::nearest_up(&im, 2 + i % 2);
        }
        v.push((format!("rand-{i}-{}x{}", im.w, im.h), im));
    }
    v
}

fn t1_of(im: &Img, wire: u8, rot: &RotCache) -> (Tier1, Vec<u8>) {
    let cfg = Config { wire, ..Config::default() };
    let f = hash(&im.px, im.w, im.h, &cfg, rot);
    (parse_t1(&f.t1).unwrap(), f.t2)
}

/// The mismatches between a work's sections and its copy's, read back
/// through the symmetry; empty when the copy's sections are the moved
/// sections.
fn mismatches(a: &Img, g: G, wire: u8, rot: &RotCache) -> Vec<String> {
    let b = g.image(a);
    let mut bad = Vec::new();
    let cfg = Config { wire, ..Config::default() };

    // the thumbnail: cell (i, j) of the copy is cell g(i, j) of the work
    let (na, nb) = (normalise(&a.px, a.w, a.h, &cfg), normalise(&b.px, b.w, b.h, &cfg));
    let (ta, tb) = (thumbnail16(&na.im, wire), thumbnail16(&nb.im, wire));
    for j in 0..16 {
        for i in 0..16 {
            let (i2, j2, _, _) = g.map(i, j, 16, 16);
            if tb[j2 * 16 + i2] != ta[j * 16 + i] {
                bad.push(format!("thumbnail cell ({i},{j})"));
            }
        }
    }
    let (pa, pb) = (t1_of(a, wire, rot), t1_of(&b, wire, rot));
    let (sa, sb) = (&pa.0, &pb.0);

    // the hierarchy's 21 transforms — the 16 × 16, the four 8 × 8 quadrants,
    // the sixteen 4 × 4 tiles: the block at g(q) holds block q's coefficients
    // moved, D'[v][u] = ±D[v][u] or ±D[u][v], negated where the frequency is
    // odd along a flipped axis; and the stored section follows: each code's
    // magnitude bucket moves with its coefficient (a block's buckets are cut
    // from its own multiset of magnitudes, which a symmetry leaves alone) and
    // its sign bit flips exactly where a nonzero coefficient is negated
    if wire == WIRE_4 && bad.is_empty() {
        let (ca, cb) = (sa.sec("dct"), sb.sec("dct"));
        let code = |sec: &[u8], off: usize, bits: usize, i: usize| -> u8 {
            let mut c = 0u8;
            for k in 0..bits {
                let bit = i * bits + k;
                c = (c << 1) | ((sec[off + (bit >> 3)] >> (7 - (bit & 7))) & 1);
            }
            c
        };
        // (side, x, y, byte offset, bits a code) of every block, in stored order
        let mut blocks = vec![(16usize, 0usize, 0usize, 0usize, 2usize)];
        for q in 0..4 {
            blocks.push((8, (q % 2) * 8, (q / 2) * 8, 64 + q * 16, 2));
        }
        for t in 0..16 {
            blocks.push((4, (t % 4) * 4, (t / 4) * 4, 128 + t * 8, 4));
        }
        let coefs = |thumb: &[i32], n: usize, x0: usize, y0: usize| -> Vec<i32> {
            let blk: Vec<i32> = (0..n * n).map(|i| thumb[(y0 + i / n) * 16 + x0 + i % n]).collect();
            dct2_exact(&blk, n)
        };
        for &(n, x0, y0, off, bits) in blocks.iter() {
            // where the block lands: the moved block's corner nearest the origin
            let (p, q) = (g.map(x0, y0, 16, 16), g.map(x0 + n - 1, y0 + n - 1, 16, 16));
            let (x1, y1) = (p.0.min(q.0), p.1.min(q.1));
            let &(_, _, _, off_b, _) = blocks.iter().find(|e| e.0 == n && e.1 == x1 && e.2 == y1).unwrap();
            let (da, db) = (coefs(&ta, n, x0, y0), coefs(&tb, n, x1, y1));
            let mut coef_bad = false;
            let mut code_bad = false;
            for v in 0..n {
                for u in 0..n {
                    let i_src = if g.swap { u * n + v } else { v * n + u };
                    let neg = (g.fx && u % 2 == 1) ^ (g.fy && v % 2 == 1);
                    let src = da[i_src];
                    coef_bad |= db[v * n + u] != if neg { -src } else { src };
                    let flip = if neg && src != 0 { 1u8 << (bits - 1) } else { 0 };
                    code_bad |= code(cb, off_b, bits, v * n + u) != code(ca, off, bits, i_src) ^ flip;
                }
            }
            if coef_bad {
                bad.push(format!("dct {n}×{n} block at ({x0},{y0})"));
            }
            if code_bad {
                bad.push(format!("dct codes, {n}×{n} block at ({x0},{y0})"));
            }
        }
    }

    // the brightness record's order statistics, the palette, the adjacency
    // and run sections: symmetry-free; byte 7's four quadrant fields move with
    // their quadrants
    let (ba, bb) = (sa.sec("brightness"), sb.sec("brightness"));
    if ba[..7] != bb[..7] {
        bad.push("brightness".into());
    }
    let field = |b: u8, qx: usize, qy: usize| (b >> ((3 - (qy * 2 + qx)) * 2)) & 3;
    for qy in 0..2 {
        for qx in 0..2 {
            let (x2, y2, _, _) = g.map(qx, qy, 2, 2);
            if field(bb[7], x2, y2) != field(ba[7], qx, qy) {
                bad.push(format!("brightness quadrant ({qx},{qy})"));
            }
        }
    }
    if sa.flags != sb.flags {
        bad.push(format!("flags {} / {}", sa.flags, sb.flags));
    }

    // the shapes section, each record read back into the work's frame
    let rec = |t: &Tier1, i: usize, back: Option<G>| -> Vec<i64> {
        let s = &t.sec("shapes")[i * 41..(i + 1) * 41];
        let (mut bw, mut bh) = t.shape_dims(s).unwrap_or((-1, -1));
        let mut rad = [0i64; 32];
        for k in 0..32 {
            rad[k] = s[9 + k] as i64;
        }
        if let Some(g) = back {
            if g.swap {
                std::mem::swap(&mut bw, &mut bh);
            }
            let mut r2 = [0i64; 32];
            for k in 0..32 {
                r2[k] = rad[g.ray(k)];
            }
            rad = r2;
        }
        let mut v = vec![
            u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as i64,
            u16::from_le_bytes([s[4], s[5]]) as i64,
            bw,
            bh,
            s[8] as i64,
        ];
        v.extend_from_slice(&rad);
        v
    };
    let (n1, n2) = (sa.count("shapes"), sb.count("shapes"));
    if n1 != n2 {
        bad.push(format!("shape count {n1} / {n2}"));
    } else {
        let mut x: Vec<Vec<i64>> = (0..n1).map(|i| rec(sa, i, None)).collect();
        let mut y: Vec<Vec<i64>> = (0..n2).map(|i| rec(sb, i, Some(g))).collect();
        x.sort();
        y.sort();
        if x != y {
            // regions tied on every invariant key are one shape in two
            // orientations: the records must agree once each is put in its
            // own canonical orientation
            let canon = |r: &Vec<i64>| -> Vec<i64> {
                let mut best: Option<Vec<i64>> = None;
                for e in 0..8usize {
                    let h = G { swap: e & 4 != 0, fx: e & 1 != 0, fy: e & 2 != 0 };
                    let mut v = vec![r[0], r[1], r[2].max(r[3]), r[2].min(r[3]), r[4]];
                    for k in 0..32 {
                        v.push(r[5 + h.ray(k)]);
                    }
                    if best.as_ref().map_or(true, |b| v < *b) {
                        best = Some(v);
                    }
                }
                best.unwrap()
            };
            let mut cx: Vec<Vec<i64>> = x.iter().map(canon).collect();
            let mut cy: Vec<Vec<i64>> = y.iter().map(canon).collect();
            cx.sort();
            cy.sort();
            bad.push(if cx == cy { "shape records (tie: one shape, two orientations)".into() } else { "shape records".to_string() });
        }
    }

    // the silhouette: what the whole image gives (the component count, the
    // run histograms, the opacity) exactly; what the kept component gives
    // exactly too, or — wire 4, components tied on every invariant key, one
    // shape in two orientations — equal once each record is put in its own
    // canonical orientation
    if (sa.flags & F_SIL != 0) && (sb.flags & F_SIL != 0) {
        let (x, y) = (sa.sec("silhouette"), sb.sec("silhouette"));
        if x[39] != y[39] || x[64..66] != y[64..66] {
            bad.push("silhouette count / opacity".into());
        }
        let (r, c) = if g.swap { (&y[48..56], &y[40..48]) } else { (&y[40..48], &y[48..56]) };
        if r != &x[40..48] || c != &x[48..56] {
            bad.push("silhouette transitions".into());
        }
        let mut sil: Vec<String> = Vec::new();
        for k in 0..32 {
            if y[g.ray(k)] != x[k] {
                sil.push(format!("silhouette ray {k}"));
                break;
            }
        }
        let (m20, m02) = if g.swap { (y[33], y[32]) } else { (y[32], y[33]) };
        if (m20, m02, y[34]) != (x[32], x[33], x[34]) {
            sil.push("silhouette moments".into());
        }
        if x[34] > 0 && (x[35] != y[35]) != (g.fx ^ g.fy) {
            sil.push("silhouette m11 sign".into());
        }
        if x[38] != y[38] || x[56..64] != y[56..64] {
            sil.push("silhouette fill / occupancy".into());
        }
        if wire == WIRE_4 {
            let (dx, dy) = (sa.silhouette_dims().unwrap(), sb.silhouette_dims().unwrap());
            if (if g.swap { (dy.1, dy.0) } else { dy }) != dx {
                sil.push("silhouette box".into());
            }
        }
        if !sil.is_empty() {
            let canon = |s: &[u8], t: &Tier1| -> Vec<i64> {
                let mut p = [0u8; 32];
                p.copy_from_slice(&s[..32]);
                let mut v: Vec<i64> = canonical_profile(&p).iter().map(|&b| b as i64).collect();
                v.extend([s[32].max(s[33]) as i64, s[32].min(s[33]) as i64, s[34] as i64, s[38] as i64]);
                v.extend(s[56..64].iter().map(|&b| b as i64));
                let (bw, bh) = t.silhouette_dims().unwrap_or((-1, -1));
                v.extend([bw.max(bh), bw.min(bh)]);
                v
            };
            if wire == WIRE_4 && canon(x, sa) == canon(y, sb) {
                bad.push("silhouette (tie: one shape, two orientations)".into());
            } else {
                bad.extend(sil);
            }
        }
    }

    // the route's four global words
    let xb = XBound::shipped();
    let ra = XRoute::build(&Prepared::new(&a_bytes(&pa), Some(&pa.1)).unwrap(), &xb.salts, &xb.xp);
    let rb = XRoute::build(&Prepared::new(&a_bytes(&pb), Some(&pb.1)).unwrap(), &xb.salts, &xb.xp);
    for k in 0..4 {
        if ra.global[k] != rb.global[k] {
            bad.push(format!("route G{k}"));
        }
    }
    bad
}

fn a_bytes(p: &(Tier1, Vec<u8>)) -> Vec<u8> {
    p.0.bytes.clone()
}

/// Wire 4: every check holds, on every work and every symmetry.
#[test]
fn wire4_sections_are_exactly_equivariant() {
    let rot = RotCache::new(&pattern());
    let mut n = 0;
    let mut fails: Vec<String> = Vec::new();
    for (name, a) in works().iter() {
        for g in D4 {
            n += 1;
            for m in mismatches(a, g, WIRE_4, &rot) {
                fails.push(format!("{name} {g:?}: {m}"));
            }
        }
    }
    let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for f in fails.iter() {
        *kinds.entry(f.rsplit(": ").next().unwrap_or("").split(" (").next().unwrap_or("").to_string()).or_insert(0) += 1;
    }
    let ties = fails.iter().filter(|f| f.contains("(tie: one shape, two orientations)")).count();
    fails.retain(|f| !f.contains("(tie: one shape, two orientations)"));
    assert!(fails.is_empty(), "{} mismatches over {n} copies: {:?}; first: {:?}", fails.len(), kinds, &fails[..fails.len().min(8)]);
    println!("wire 4: thumbnail, DCT, shapes, silhouette and the route's four words equivariant on all {n} D4 copies ({ties} of them keep a region tied with its own rotated twin, the records equal up to that orientation)");
}

/// Wire 3, the same works: the checks fail somewhere (the reason for wire 4);
/// on works whose sides are multiples of 16 and at most 128 the thumbnail
/// already commuted with a flip.
#[test]
fn wire3_sections_are_not() {
    let rot = RotCache::new(&pattern());
    let (mut n, mut moved, mut g0, mut g3) = (0, 0, 0, 0);
    for (_, a) in works().iter() {
        for g in D4 {
            n += 1;
            let m = mismatches(a, g, WIRE_3, &rot);
            moved += !m.is_empty() as usize;
            g0 += m.iter().any(|s| s == "route G0") as usize;
            g3 += m.iter().any(|s| s == "route G3") as usize;
        }
    }
    assert!(moved > n / 4, "wire 3 moved on only {moved} of {n}");
    println!("wire 3: some section moved on {moved} of {n} D4 copies; G0 moved on {g0}, G3 on {g3}");
}

/// The silhouette's component when several share the largest area
/// (docs/SPEC-W4-paph-wire4.md §4 and §7).  Two shapes of one area,
/// perimeter, box and canonical profile, which only the moments or the
/// occupancy tell apart, are told apart the same way in every orientation,
/// and the record is exactly the moved record.  Twins — one shape in two
/// orientations — leave every such key tied, the scan order decides, and the
/// record is the moved record up to that orientation.
#[test]
fn silhouette_ties() {
    let rot = RotCache::new(&pattern());
    let settled = |im: &Img| -> Option<u8> { silhouette_settled_by(&normalise(&im.px, im.w, im.h, &Config::default()).im) };
    let (mut keys, mut up_to) = (Vec::new(), 0);
    for t in W4_TIES.iter() {
        let im = tie_canvas(t);
        // the two shapes share the largest area
        let cells: Vec<usize> = t.stamps.iter().map(|s| s.1.len()).collect();
        assert_eq!(cells[0], cells[1], "{}", t.name);
        let key = settled(&im).unwrap();
        keys.push(key);
        if t.name == "twins" {
            assert_eq!(key, 5, "twins settled by key {key}, not by the scan order");
            for g in D4 {
                let m = mismatches(&im, g, WIRE_4, &rot);
                assert!(m.iter().all(|s| s.ends_with("(tie: one shape, two orientations)")), "twins {g:?}: {m:?}");
                up_to += m.iter().any(|s| s.starts_with("silhouette")) as usize;
            }
        } else {
            assert!(key == 3 || key == 4, "{}: settled by key {key}, not by the moments or the occupancy", t.name);
            for g in D4 {
                let m = mismatches(&im, g, WIRE_4, &rot);
                assert!(m.is_empty(), "{} {g:?}: {m:?}", t.name);
            }
        }
    }
    let pairs = &keys[..keys.len() - 1];
    assert!(pairs.contains(&3) && pairs.contains(&4), "the pairs exercise both keys: {pairs:?}");
    println!("silhouette ties: {} pairs settled by keys {pairs:?} (3 the moments, 4 the occupancy), exact under all seven symmetries; twins by the scan order, the record equal up to orientation under {up_to} of seven", pairs.len());
}

/// The tables the exact paths rest on.
#[test]
fn tables_are_exactly_symmetric() {
    use crate::tables::{cos_table, RAYC, RAYSN};
    for n in [4usize, 8, 16] {
        let t = cos_table(n);
        for u in 0..n {
            for i in 0..n {
                let s = if u % 2 == 1 { -1 } else { 1 };
                assert_eq!(t[u * n + n - 1 - i], s * t[u * n + i], "DCT{n} u {u} i {i}");
            }
        }
    }
    for k in 0..32 {
        assert_eq!(RAYC[(k + 8) % 32], -RAYSN[k]);
        assert_eq!(RAYSN[(k + 8) % 32], RAYC[k]);
        assert_eq!(RAYC[(48 - k) % 32], -RAYC[k]);
        assert_eq!(RAYSN[(48 - k) % 32], RAYSN[k]);
        assert_eq!(RAYC[(40 - k) % 32], RAYSN[k]);
    }
}

/// A mixed pair is refused by every comparator; either format alone is not.
#[test]
fn mixed_formats_are_refused() {
    let rot = RotCache::new(&pattern());
    let im = work(120, 90, 5, false);
    let (c3, c4) = (Config { wire: WIRE_3, ..Config::default() }, Config::default());
    let (f3, f4) = (hash(&im.px, im.w, im.h, &c3, &rot), hash(&im.px, im.w, im.h, &c4, &rot));
    assert_eq!((f3.t1[4], f4.t1[4], f3.t2[4], f4.t2[4]), (3, 4, 3, 4));
    let p = crate::calibration::Profile::cal004();
    let d = Config::default();
    let r = crate::v42::compare_v42(&f3.t1, Some(&f3.t2), &f4.t1, Some(&f4.t2), &d, &p, None, None);
    assert_eq!((r.base.verdict, r.base.reasons.clone()), ("Indeterminate", vec![crate::wire::R_WIRE_MISMATCH]));
    for x in [&f3, &f4] {
        let r = crate::v42::compare_v42(&x.t1, Some(&x.t2), &x.t1, Some(&x.t2), &d, &p, None, None);
        assert_eq!(r.base.verdict, "Identical");
    }
    assert_eq!(crate::compare::compare(&f4.t1, Some(&f4.t2), &f3.t1, Some(&f3.t2), &d).err(), Some(crate::wire::E_WIRE_MISMATCH));
}
