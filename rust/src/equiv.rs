//! Equivalence digest: everything the engine emits over a deterministic
//! synthetic corpus, one SHA-256 per case.
//!
//!     cargo run --release --features equiv --bin paph-equiv > digest.txt
//!
//! The corpus is generated here, from integer arithmetic only, so the same
//! binary on any machine prints the same lines.  A refactor is output-
//! identical exactly when this file does not change.  It deliberately reaches
//! far past the golden vectors: degenerate sizes, dithered pixel art, mattes,
//! nearest-neighbour blow-ups, every D4 transform, inversion, recolour, crops,
//! pastes, sixteen hash-time configurations (including the non-default window
//! sizes that take the generic median path), every comparator, the sketch-only
//! (Tier-1) path, corrupted wires, and compare-time configuration.

use crate::calibration::Profile;
use crate::config::{Config, Evidence, RagEndpoint, Scoring};
use crate::keypoints::{pattern, RotCache};
use crate::sha256::{hex, sha256};
use crate::wire::hash;

use crate::synth::*;

fn corpus() -> Vec<(String, Img)> {
    let mut v: Vec<(String, Img)> = Vec::new();
    // degenerate and boundary sizes: around KP_MARGIN, LEVEL_MIN, the window
    // sizes, and the 128-long shape grid
    for &(w, h) in &[
        (1, 1), (1, 7), (7, 1), (2, 2), (3, 5), (7, 7), (8, 8), (9, 9), (15, 17), (16, 16), (17, 16),
        (24, 24), (31, 33), (32, 32), (51, 51), (52, 52), (53, 53), (56, 200), (200, 56), (127, 129),
        (128, 128), (129, 127), (255, 3), (3, 255), (1200, 9),
    ] {
        v.push((format!("work-{}x{}", w, h), work(w, h, (w * 31 + h) as i64, false)));
        v.push((format!("art-{}x{}", w, h), pixel_art(w, h, (w * 131 + h * 7) as u64, 6, 2)));
    }
    // flat, transparent, checkerboard, all-distinct
    let mut flat = Img::new(64, 48);
    for i in 0..64 * 48 {
        flat.px[i * 4..i * 4 + 4].copy_from_slice(&[90, 120, 200, 255]);
    }
    v.push(("flat-64x48".into(), flat));
    v.push(("transparent-40x40".into(), Img::new(40, 40)));
    let mut half = Img::new(80, 60);
    for y in 0..60 {
        for x in 0..80 {
            if x < 40 {
                half.set(x, y, [200, (x * 3) as u8, (y * 4) as u8, 255]);
            }
        }
    }
    v.push(("half-alpha-80x60".into(), half));
    let mut chk = Img::new(96, 96);
    for y in 0..96 {
        for x in 0..96 {
            let c = if (x + y) & 1 == 0 { [255, 255, 255, 255] } else { [0, 0, 0, 255] };
            chk.set(x, y, c);
        }
    }
    v.push(("checker-96".into(), chk));
    let mut uniq = Img::new(181, 181);
    for i in 0..181 * 181 {
        let k = (i as u32).wrapping_mul(2654435761);
        uniq.px[i * 4..i * 4 + 4].copy_from_slice(&[k as u8, (k >> 8) as u8, (k >> 16) as u8, 255]);
    }
    v.push(("alldistinct-181".into(), uniq));

    // pixel art families with every transform
    let bases: Vec<(String, Img)> = vec![
        ("dither-160x120".into(), pixel_art(160, 120, 11, 8, 2)),
        ("sprite-96-alpha".into(), pixel_art(96, 96, 23, 5, 0)),
        ("matte-120x90".into(), pixel_art(120, 90, 37, 7, 1)),
        ("busy-300x220".into(), pixel_art(300, 220, 41, 16, 2)),
        ("work-200x150".into(), work(200, 150, 7, false)),
        ("work-alpha-180x140".into(), work(180, 140, 9, true)),
        ("tiny-art-60x44".into(), pixel_art(60, 44, 53, 4, 2)),
        ("wide-art-400x90".into(), pixel_art(400, 90, 61, 12, 2)),
    ];
    for (name, b) in bases.iter() {
        v.push((name.clone(), b.clone()));
        v.push((format!("{}~mirror", name), mirror(b)));
        v.push((format!("{}~rot90", name), rot90(b)));
        v.push((format!("{}~transpose", name), transpose(b)));
        v.push((format!("{}~invert", name), invert(b)));
        v.push((format!("{}~recolour", name), recolour(b)));
        v.push((format!("{}~up2", name), nearest_up(b, 2)));
        v.push((format!("{}~up3", name), nearest_up(b, 3)));
        v.push((format!("{}~down", name), area_down(b, (b.w * 7 / 10).max(1), (b.h * 7 / 10).max(1))));
        v.push((format!("{}~crop", name), crop(b, b.w / 6, b.h / 7, (b.w * 2 / 3).max(1), (b.h * 2 / 3).max(1))));
        v.push((format!("{}~shift1", name), shift1(b)));
        let host = pixel_art(b.w * 2 + 20, b.h * 2 + 16, 997, 9, 2);
        v.push((format!("{}~paste", name), paste(b, &host, b.w / 2 + 3, b.h / 3 + 5)));
    }
    // a seeded spread of random pixel art at random sizes
    let mut r = Rng(0x5eed_1234_abcd_0001);
    for i in 0..40 {
        let w = 8 + r.below(420) as usize;
        let h = 8 + r.below(320) as usize;
        let ncol = 2 + r.below(40) as usize;
        let bg = r.below(3) as u8;
        let mut im = pixel_art(w, h, r.next(), ncol, bg);
        if i % 5 == 0 {
            im = nearest_up(&im, 2 + (i / 5) % 3);
        }
        v.push((format!("rand-{:02}-{}x{}", i, im.w, im.h), im));
    }
    v
}

fn configs() -> Vec<(&'static str, Config)> {
    let d = Config::default();
    let mut out: Vec<(&'static str, Config)> = vec![("default", d)];
    let mut c = d;
    c.kp_select = 0;
    c.kp_count = 256;
    out.push(("legacy41", c));
    let mut c = d;
    c.fold_matte = false;
    out.push(("nomatte", c));
    let mut c = d;
    c.divide_upscale = false;
    out.push(("noupscale", c));
    let mut c = d;
    c.fold_invert = false;
    out.push(("noinvert", c));
    let mut c = d;
    c.local_windows = [8, 24];
    out.push(("win8-24", c));
    let mut c = d;
    c.local_windows = [12, 32];
    out.push(("win12-32", c));
    let mut c = d;
    c.local_windows = [16, 16];
    out.push(("win16-16", c));
    let mut c = d;
    c.local_windows = [24, 40];
    out.push(("win24-40", c));
    let mut c = d;
    c.peak_radius = 2;
    out.push(("peak2", c));
    let mut c = d;
    c.peak_radius = 9;
    out.push(("peak9", c));
    let mut c = d;
    c.local_count = 32;
    out.push(("local32", c));
    let mut c = d;
    c.sketch_count = 7;
    c.kp_count = 33;
    out.push(("kp33-sk7", c));
    let mut c = d;
    c.kp_count = 0;
    c.sketch_count = 0;
    out.push(("kp0", c));
    let mut c = d;
    c.matte_tol = 60;
    out.push(("matte60", c));
    let mut c = d;
    c.matte_tol = 0;
    c.kp_count = 1;
    out.push(("matte0-kp1", c));
    out
}

fn line(out: &mut String, id: &str, bytes: &[u8]) {
    out.push_str(id);
    out.push(' ');
    out.push_str(&hex(&sha256(bytes))[..32]);
    out.push('\n');
}

/// The digest text, one line per case.  Identical on every target that
/// runs the engine correctly — native, WebAssembly with or without SIMD.
pub fn digest() -> String {
    let mut out = String::with_capacity(1 << 17);
    let rot = RotCache::new(&pattern());
    let imgs = corpus();
    let cfgs = configs();
    let mut wires: Vec<(String, Vec<u8>, Vec<u8>)> = Vec::new();
    for (name, im) in imgs.iter() {
        for (ci, (cn, c)) in cfgs.iter().enumerate() {
            // the expensive non-default configs run on every third image
            if ci > 0 && (im.w * im.h > 200_000 || (name.len() + ci) % 3 != 0) && !name.starts_with("work-1") {
                continue;
            }
            let f = hash(&im.px, im.w, im.h, c, &rot);
            let mut b = f.t1.clone();
            b.extend_from_slice(&f.t2);
            line(&mut out, &format!("hash/{}/{}", cn, name), &b);
            if ci == 0 {
                wires.push((name.clone(), f.t1, f.t2));
            }
        }
    }

    let p42 = Profile::cal004();
    let p41 = Profile::cal003();
    let p4 = Profile::cal001();
    let d = Config::default();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    // every family against its transforms, and neighbours against each other
    for i in 0..wires.len() {
        let (ni, _, _) = &wires[i];
        if let Some(base) = ni.split('~').next() {
            for j in 0..wires.len() {
                if i != j && wires[j].0.starts_with(&format!("{}~", base)) && !ni.contains('~') {
                    pairs.push((i, j));
                }
            }
        }
        if i + 1 < wires.len() {
            pairs.push((i, i + 1));
        }
    }
    let mut r = Rng(0xfeed_beef_0000_0042);
    for _ in 0..60 {
        let i = r.below(wires.len() as u64) as usize;
        let j = r.below(wires.len() as u64) as usize;
        pairs.push((i, j));
    }
    for (i, j) in pairs.iter().copied() {
        let (na, a1, a2) = &wires[i];
        let (nb, b1, b2) = &wires[j];
        let id = format!("{}|{}", na, nb);
        let r42 = crate::v42::compare_v42(a1, Some(a2), b1, Some(b2), &d, &p42, None, None);
        line(&mut out, &format!("v42/{}", id), crate::v42::to_json_v42(&r42).as_bytes());
        // the JavaScript-shaped report (what the WebAssembly build returns),
        // full and lean
        line(&mut out, &format!("r42/{}", id), crate::report::v42(&r42).to_string().as_bytes());
        if let (Ok(pa), Ok(pb)) = (crate::prepared::Prepared::new(a1, Some(a2)), crate::prepared::Prepared::new(b1, Some(b2))) {
            let lean = crate::v42::compare_v42_lean(&pa, &pb, &d, &p42);
            line(&mut out, &format!("l42/{}", id), crate::report::v42(&lean).to_string().as_bytes());
        }
        let s42 = crate::v42::screen_v42(a1, Some(a2), b1, Some(b2), &d, &p42);
        line(&mut out, &format!("s42/{}", id), crate::v42::screen_json_42(&s42).as_bytes());
        if (i + j) % 2 == 0 {
            let r41 = crate::v41::compare_v41(a1, Some(a2), b1, Some(b2), &d, &p41, None, None);
            line(&mut out, &format!("v41/{}", id), crate::v41::to_json_v41(&r41).as_bytes());
        }
        if (i + j) % 3 == 0 {
            let r4 = crate::v4::compare_v4(a1, Some(a2), b1, Some(b2), &d, &p4, None, None);
            line(&mut out, &format!("v4/{}", id), crate::v4::to_json_v4(&r4).as_bytes());
        }
        if (i * 7 + j) % 4 == 0 {
            // Tier 1 alone: the 32-keypoint sketch path
            let r = crate::v42::compare_v42(a1, None, b1, None, &d, &p42, None, None);
            line(&mut out, &format!("v42-t1/{}", id), crate::v42::to_json_v42(&r).as_bytes());
        }
        match crate::compare::compare(a1, Some(a2), b1, Some(b2), &d) {
            Ok(v) => line(&mut out, &format!("v3/{}", id), crate::compare::to_json(&v).as_bytes()),
            Err(e) => line(&mut out, &format!("v3/{}", id), e.as_bytes()),
        }
        if (i + 2 * j) % 5 == 0 {
            let mut c = d;
            c.scoring = Scoring::Gate;
            c.evidence = Evidence::Proportion;
            c.rag_endpoint = RagEndpoint::Quantile;
            c.hamming_t = 4;
            c.geo_eps = 900;
            c.mirror_hypothesis = (i & 1) == 0;
            c.geo_enabled = (j & 3) != 0;
            match crate::compare::compare(a1, Some(a2), b1, Some(b2), &c) {
                Ok(v) => line(&mut out, &format!("v3cfg/{}", id), crate::compare::to_json(&v).as_bytes()),
                Err(e) => line(&mut out, &format!("v3cfg/{}", id), e.as_bytes()),
            }
        }
    }
    // corrupted wires take the refusal paths, never a panic
    if let Some((n, a1, a2)) = wires.iter().find(|w| w.0.starts_with("work-128x128")) {
        let mut bad1 = a1.clone();
        bad1[100] ^= 1;
        let mut bad2 = a2.clone();
        if bad2.len() > 40 {
            bad2[40] ^= 0x80;
        }
        let r = crate::v42::compare_v42(&bad1, Some(a2), a1, Some(a2), &d, &p42, None, None);
        line(&mut out, &format!("corrupt-t1/{}", n), crate::v42::to_json_v42(&r).as_bytes());
        let r = crate::v42::compare_v42(a1, Some(&bad2), a1, Some(a2), &d, &p42, None, None);
        line(&mut out, &format!("corrupt-t2/{}", n), crate::v42::to_json_v42(&r).as_bytes());
        let s = crate::v42::screen_v42(&bad1, Some(a2), a1, Some(&bad2), &d, &p42);
        line(&mut out, &format!("corrupt-screen/{}", n), crate::v42::screen_json_42(&s).as_bytes());
    }
    let _ = (imgs.len(), pairs.len());
    out
}
