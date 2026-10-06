//! Timing harness: prof <mode> <iters> <a.rgba> [b.rgba]
//!   mode h  — hash a
//!   mode c  — compare_v42(a, b)
//!   mode s  — screen_v42(a, b)
//!   mode 3  — the v3 comparator(a, b)
//! Images are raw dumps: [u32 w][u32 h][RGBA].  Prints the best and the median
//! of five batches, because a shared machine's noise is one-sided.
use paph::config::Config;
use paph::keypoints::{pattern, RotCache};
use paph::wire::hash;
use std::time::Instant;

fn load(p: &str) -> (Vec<u8>, usize, usize) {
    let b = std::fs::read(p).unwrap();
    let w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let h = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
    (b[8..].to_vec(), w, h)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mode = a[1].clone();
    let iters: usize = a[2].parse().unwrap();
    let (pa, wa, ha) = load(&a[3]);
    let cfg = Config::default();
    let rot = RotCache::new(&pattern());
    let prof = paph::calibration::Profile::cal004();
    let (fa, fb) = if mode != "h" {
        let (pb, wb, hb) = load(&a[4]);
        (Some(hash(&pa, wa, ha, &cfg, &rot)), Some(hash(&pb, wb, hb, &cfg, &rot)))
    } else {
        (None, None)
    };
    let mut acc = 0i64;
    let mut run = |n: usize| -> f64 {
        let t = Instant::now();
        for _ in 0..n {
            match mode.as_str() {
                "h" => {
                    let f = hash(&pa, wa, ha, &cfg, &rot);
                    acc += f.kp_count as i64 + f.t1[100] as i64;
                }
                "c" => {
                    let (x, y) = (fa.as_ref().unwrap(), fb.as_ref().unwrap());
                    let r = paph::v42::compare_v42(&x.t1, Some(&x.t2), &y.t1, Some(&y.t2), &cfg, &prof, None, None);
                    acc += r.base.total_inliers + r.base.structural;
                }
                "s" => {
                    let (x, y) = (fa.as_ref().unwrap(), fb.as_ref().unwrap());
                    let r = paph::v42::screen_v42(&x.t1, Some(&x.t2), &y.t1, Some(&y.t2), &cfg, &prof);
                    acc += r.pool_direct as i64;
                }
                "3" => {
                    let (x, y) = (fa.as_ref().unwrap(), fb.as_ref().unwrap());
                    let v = paph::compare::compare(&x.t1, Some(&x.t2), &y.t1, Some(&y.t2), &cfg).unwrap();
                    acc += v.structural;
                }
                _ => panic!("mode"),
            }
        }
        t.elapsed().as_secs_f64() * 1000.0 / n as f64
    };
    run(1);
    let mut v: Vec<f64> = (0..5).map(|_| run(iters)).collect();
    v.sort_by(|p, q| p.partial_cmp(q).unwrap());
    println!("{:.3} ms best, {:.3} ms median (5x{})", v[0], v[2], iters);
    std::hint::black_box(acc);
}
