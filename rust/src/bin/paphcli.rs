//! Parity harness.
//!   hash:    [b"H"][u32 w][u32 h][RGBA]  ->  [u32 t1len][u32 t2len][t1][t2]
//!   compare: [b"C"][u32 a1][u32 a2][u32 b1][u32 b2][wires] -> [u32 len][json]
//!   golden:  [b"G"] -> the SPEC-004 §20 golden vectors as JSON (see golden.rs)
//!   bench:   [b"B"][u32 w][u32 h][u32 iters][RGBA] -> stage timings as JSON.
//!            Split by stage on purpose: the interesting question is never
//!            "how long did it take" but "whose code was it", and the answer
//!            here is that hashing is the frozen v3 wire and not the 4.2
//!            keypoint budget.
//!   v4:      [b"V"][u32 a1][u32 a2][u32 b1][u32 b2][wires] -> compare_v4 JSON
//!            (Config::default + CAL-001-PROVISIONAL; the parity4 surface)
//!   v41:     [b"W"] same framing -> compare_v41 JSON under CAL-003-PROPOSED
//!   v42:     [b"X"] same framing -> compare_v42 JSON under CAL-004-PROPOSED
use paph::compare::compare;
use paph::config::Config;
use paph::keypoints::{pattern, RotCache};
use paph::wire::hash;
use std::io::{Read, Write};

fn rd(b: &[u8], o: usize) -> usize {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize
}

fn json_esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn main() {
    let mut buf = Vec::new();
    std::io::stdin().read_to_end(&mut buf).unwrap();
    let out = std::io::stdout();
    let mut o = out.lock();
    let cfg = Config::default();
    match buf[0] {
        b'V' => {
            let (a1, a2, b1, b2) = (rd(&buf, 1), rd(&buf, 5), rd(&buf, 9), rd(&buf, 13));
            let mut p = 17usize;
            let at1 = &buf[p..p + a1];
            p += a1;
            let at2 = &buf[p..p + a2];
            p += a2;
            let bt1 = &buf[p..p + b1];
            p += b1;
            let bt2 = &buf[p..p + b2];
            let prof = paph::calibration::Profile::cal001();
            let r = paph::v4::compare_v4(
                at1, if a2 > 0 { Some(at2) } else { None },
                bt1, if b2 > 0 { Some(bt2) } else { None },
                &cfg, &prof, None, None,
            );
            o.write_all(paph::v4::to_json_v4(&r).as_bytes()).unwrap();
        }
        b'W' => {
            // SPEC-004.1: same framing as 'V', comparator 41 under cal003.
            let (a1, a2, b1, b2) = (rd(&buf, 1), rd(&buf, 5), rd(&buf, 9), rd(&buf, 13));
            let mut p = 17usize;
            let at1 = &buf[p..p + a1];
            p += a1;
            let at2 = &buf[p..p + a2];
            p += a2;
            let bt1 = &buf[p..p + b1];
            p += b1;
            let bt2 = &buf[p..p + b2];
            let prof = paph::calibration::Profile::cal003();
            let r = paph::v41::compare_v41(
                at1, if a2 > 0 { Some(at2) } else { None },
                bt1, if b2 > 0 { Some(bt2) } else { None },
                &cfg, &prof, None, None,
            );
            o.write_all(paph::v41::to_json_v41(&r).as_bytes()).unwrap();
        }
        b'X' => {
            // SPEC-004.2: same framing as 'V'/'W', comparator 42 under cal004.
            let (a1, a2, b1, b2) = (rd(&buf, 1), rd(&buf, 5), rd(&buf, 9), rd(&buf, 13));
            let mut p = 17usize;
            let at1 = &buf[p..p + a1];
            p += a1;
            let at2 = &buf[p..p + a2];
            p += a2;
            let bt1 = &buf[p..p + b1];
            p += b1;
            let bt2 = &buf[p..p + b2];
            let prof = paph::calibration::Profile::cal004();
            let r = paph::v42::compare_v42(
                at1, if a2 > 0 { Some(at2) } else { None },
                bt1, if b2 > 0 { Some(bt2) } else { None },
                &cfg, &prof, None, None,
            );
            o.write_all(paph::v42::to_json_v42(&r).as_bytes()).unwrap();
        }
        b'R' | b'L' | b'S' => {
            // Comparator 42 through the JavaScript-shaped report (report.rs):
            // 'R' full, 'L' lean (v3: null), 'S' the stage-1 screen.  Same
            // framing as 'X'.
            let (a1, a2, b1, b2) = (rd(&buf, 1), rd(&buf, 5), rd(&buf, 9), rd(&buf, 13));
            let mut p = 17usize;
            let at1 = &buf[p..p + a1];
            p += a1;
            let at2 = &buf[p..p + a2];
            p += a2;
            let bt1 = &buf[p..p + b1];
            p += b1;
            let bt2 = &buf[p..p + b2];
            let prof = paph::calibration::Profile::cal004();
            let (o2a, o2b) = (if a2 > 0 { Some(at2) } else { None }, if b2 > 0 { Some(bt2) } else { None });
            let j = match buf[0] {
                b'S' => paph::report::screen(&paph::v42::screen_v42(at1, o2a, bt1, o2b, &cfg, &prof)),
                b'R' => paph::report::v42(&paph::v42::compare_v42(at1, o2a, bt1, o2b, &cfg, &prof, None, None)),
                _ => match (paph::prepared::Prepared::new(at1, o2a), paph::prepared::Prepared::new(bt1, o2b)) {
                    (Ok(x), Ok(y)) => paph::report::v42(&paph::v42::compare_v42_lean(&x, &y, &cfg, &prof)),
                    _ => paph::report::v42(&paph::v42::compare_v42(at1, o2a, bt1, o2b, &cfg, &prof, None, None)),
                },
            };
            o.write_all(j.to_string().as_bytes()).unwrap();
        }
        b'B' => {
            // Bench: [b"B"][u32 w][u32 h][u32 iters][RGBA] -> timings, one line each.
            let (w, h, iters) = (rd(&buf, 1), rd(&buf, 5), rd(&buf, 9));
            let px = &buf[13..13 + w * h * 4];
            let rot = RotCache::new(&pattern());
            let t0 = std::time::Instant::now();
            let mut fp = hash(px, w, h, &cfg, &rot);
            for _ in 1..iters {
                fp = hash(px, w, h, &cfg, &rot);
            }
            let hash_ms = t0.elapsed().as_secs_f64() * 1000.0 / iters as f64;

            // per-section split of the tier-1 body — implementation timing only,
            // every function is the shipped one
            {
                use paph::sections as sec;
                let norm = paph::front::normalise(px, w, h, &cfg);
                let im = &norm.im;
                let reps = iters.max(3);
                macro_rules! t {
                    ($name:expr, $e:expr) => {{
                        let t = std::time::Instant::now();
                        for _ in 0..reps { let _ = $e; }
                        eprintln!("  {:>12} {:8.2} ms", $name, t.elapsed().as_secs_f64() * 1000.0 / reps as f64);
                    }};
                }
                eprintln!("hash sections at {}x{}:", w, h);
                t!("normalise", paph::front::normalise(px, w, h, &cfg));
                let thumb = sec::thumbnail16(im);
                t!("thumb16", sec::thumbnail16(im));
                t!("dct", sec::hierarchical_dct(&thumb));
                t!("brightness", sec::brightness_record(&thumb));
                t!("palette", sec::identity_palette(im));
                t!("rag", sec::sparse_rag(im));
                t!("shapes", sec::shape_signatures(im));
                t!("runs", sec::run_lengths(im));
                t!("local", sec::local_fingerprints(im, &cfg));
                t!("silhouette", sec::silhouette(im));
                t!("colour", sec::colour_digest(im));
                t!("keypoints", paph::keypoints::keypoints(im, &cfg, &rot));
            }

            // stage split: the same hash with the 4.1 grid selector (cheap) and
            // at the 4.1 budget, so the selector and the budget are separable
            let mut c_grid = cfg.clone();
            c_grid.kp_select = paph::config::KP_SELECT_LEGACY;
            let t = std::time::Instant::now();
            for _ in 0..iters { let _ = hash(px, w, h, &c_grid, &rot); }
            let grid_ms = t.elapsed().as_secs_f64() * 1000.0 / iters as f64;

            let mut c41 = c_grid.clone();
            c41.kp_count = 256;
            let t = std::time::Instant::now();
            for _ in 0..iters { let _ = hash(px, w, h, &c41, &rot); }
            let kp41_ms = t.elapsed().as_secs_f64() * 1000.0 / iters as f64;

            // keypoint stage alone, both selectors
            let norm = paph::front::normalise(px, w, h, &cfg);
            let t = std::time::Instant::now();
            for _ in 0..iters { let _ = paph::keypoints::keypoints(&norm.im, &cfg, &rot); }
            let kpq_ms = t.elapsed().as_secs_f64() * 1000.0 / iters as f64;
            let t = std::time::Instant::now();
            for _ in 0..iters { let _ = paph::keypoints::keypoints(&norm.im, &c_grid, &rot); }
            let kpg_ms = t.elapsed().as_secs_f64() * 1000.0 / iters as f64;

            // three pairs, because they exercise completely different amounts of
            // the geometry stage: a mirrored copy runs the full multi-model
            // extraction and all five null members on large pools, an unrelated
            // pair breaks out of round 0, and v3-only isolates the frozen
            // structural half from anything 4.2 owns.
            let mut px2 = px.to_vec();
            for (i, bb) in px2.iter_mut().enumerate() {
                if i % 4 != 3 {
                    *bb = bb.wrapping_add(((i * 37) % 61) as u8);
                }
            }
            let mut pxm = vec![0u8; px.len()];
            for y in 0..h {
                for x in 0..w {
                    let s0 = (y * w + (w - 1 - x)) * 4;
                    let d0 = (y * w + x) * 4;
                    pxm[d0..d0 + 4].copy_from_slice(&px[s0..s0 + 4]);
                }
            }
            let fp2 = hash(&px2, w, h, &cfg, &rot);
            let fpm = hash(&pxm, w, h, &cfg, &rot);
            let prof = paph::calibration::Profile::cal004();
            let mut acc = 0i64;
            let mut timed = |x: &paph::wire::Fingerprint, y: &paph::wire::Fingerprint| -> f64 {
                let t = std::time::Instant::now();
                for _ in 0..iters {
                    let r = paph::v42::compare_v42(
                        &x.t1, Some(&x.t2), &y.t1, Some(&y.t2), &cfg, &prof, None, None,
                    );
                    acc += r.base.total_inliers;
                }
                t.elapsed().as_secs_f64() * 1000.0 / iters as f64
            };
            let cmp_ms = timed(&fp, &fp2);
            let mir_ms = timed(&fp, &fpm);
            let self_ms = timed(&fp, &fp);
            let tv = std::time::Instant::now();
            for _ in 0..iters {
                let v = compare(&fp.t1, Some(&fp.t2), &fpm.t1, Some(&fpm.t2), &cfg).unwrap();
                acc += v.geo.inliers;
            }
            let v3_ms = tv.elapsed().as_secs_f64() * 1000.0 / iters as f64;

            let j = format!(
                "{{\"w\":{},\"h\":{},\"iters\":{},\"kp\":{},\"hashMs\":{:.2},\"gridSelMs\":{:.2},\"budget256Ms\":{:.2},\"kpStageQualityMs\":{:.2},\"kpStageGridMs\":{:.2},\"cmpUnrelMs\":{:.2},\"cmpMirrorMs\":{:.2},\"cmpSelfMs\":{:.2},\"v3OnlyMs\":{:.2},\"acc\":{}}}",
                w, h, iters, fp.kp_count, hash_ms, grid_ms, kp41_ms, kpq_ms, kpg_ms,
                cmp_ms, mir_ms, self_ms, v3_ms, acc
            );
            o.write_all(j.as_bytes()).unwrap();
        }
        b'G' => {
            o.write_all(paph::golden::golden_json().as_bytes()).unwrap();
        }
        b'H' => {
            let (w, h) = (rd(&buf, 1), rd(&buf, 5));
            let rot = RotCache::new(&pattern());
            let fp = hash(&buf[9..9 + w * h * 4], w, h, &cfg, &rot);
            o.write_all(&(fp.t1.len() as u32).to_le_bytes()).unwrap();
            o.write_all(&(fp.t2.len() as u32).to_le_bytes()).unwrap();
            o.write_all(&fp.t1).unwrap();
            o.write_all(&fp.t2).unwrap();
        }
        b'C' => {
            let (a1, a2, b1, b2) = (rd(&buf, 1), rd(&buf, 5), rd(&buf, 9), rd(&buf, 13));
            let mut p = 17usize;
            let at1 = &buf[p..p + a1];
            p += a1;
            let at2 = &buf[p..p + a2];
            p += a2;
            let bt1 = &buf[p..p + b1];
            p += b1;
            let bt2 = &buf[p..p + b2];
            let v = compare(at1, Some(at2), bt1, Some(bt2), &cfg).unwrap();
            let mut ch = String::new();
            for (i, (n, c)) in v.channels.iter().enumerate() {
                if i > 0 {
                    ch.push(',');
                }
                ch.push_str(&format!(
                    "\"{}\":{{\"value\":{},\"raw\":{},\"control\":{},\"measurable\":{},\"note\":\"{}\"}}",
                    n, c.value, c.raw, c.control, c.measurable, json_esc(&c.note)
                ));
            }
            let j = format!(
                "{{\"verdict\":\"{}\",\"class\":\"{}\",\"structural\":{},\"geometric\":{},\"weighted\":{},\"gate\":{},\
                 \"identical\":{},\"structuralCertifiable\":{},\"swapped\":{},\"inliers\":{},\"chanceInliers\":{},\
                 \"accepted\":{},\"hypothesis\":\"{}\",\"dihedral\":{},\"inverted\":{},\"palette\":\"{}\",\
                 \"basis\":[{}],\"abstained\":[{}],\"channels\":{{{}}}}}",
                v.verdict, json_esc(&v.class), v.structural, v.geometric, v.weighted, v.gate,
                v.identical, v.structural_certifiable, v.swapped, v.geo.inliers, v.geo.chance,
                v.geo.accepted, v.geo.hypothesis,
                match v.dihedral { Some(d) => format!("\"{}\"", d), None => "null".into() },
                v.inverted, v.palette_relation,
                v.basis.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(","),
                v.abstained.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(","),
                ch
            );
            o.write_all(&(j.len() as u32).to_le_bytes()).unwrap();
            o.write_all(j.as_bytes()).unwrap();
        }
        _ => panic!("bad mode byte"),
    }
}
