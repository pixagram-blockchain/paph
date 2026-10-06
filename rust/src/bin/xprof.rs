//! Stage profile of one comparator-42 lean comparison: xprof <a.rgba> <b.rgba>
//! Prints the cost of each stage so the PAPH-X scheduler is ordered by
//! measurement rather than by guesswork.
use paph::calibration::Profile;
use paph::compare::{compare_canonical, Reading};
use paph::config::Config;
use paph::geom42::{diversity_42, geo_measure_42, gn_control_42, Scratch};
use paph::keypoints::{pattern, RotCache};
use paph::local_v4::local_v4_41_shared;
use paph::prepared::{canon_swapped, corr_42, match_state, BagShare, PairCtx, Prepared};
use paph::wire::hash;
use std::time::Instant;

fn load(p: &str) -> (Vec<u8>, usize, usize) {
    let b = std::fs::read(p).unwrap();
    let w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let h = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
    (b[8..].to_vec(), w, h)
}

fn time<F: FnMut()>(name: &str, n: usize, mut f: F) {
    f();
    let t = Instant::now();
    for _ in 0..n {
        f();
    }
    println!("{:<34} {:9.3} us", name, t.elapsed().as_secs_f64() * 1e6 / n as f64);
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (pa, wa, ha) = load(&a[1]);
    let (pb, wb, hb) = load(&a[2]);
    let cfg = Config::default();
    let rot = RotCache::new(&pattern());
    let prof = Profile::cal004();
    let fa = hash(&pa, wa, ha, &cfg, &rot);
    let fb = hash(&pb, wb, hb, &cfg, &rot);
    let a0 = Prepared::new(&fa.t1, Some(&fa.t2)).unwrap();
    let b0 = Prepared::new(&fb.t1, Some(&fb.t2)).unwrap();
    let swapped = canon_swapped(&a0, &b0);
    let (ca, cb) = if swapped { (&b0, &a0) } else { (&a0, &b0) };
    println!("kp {} x {}", ca.kp.len(), cb.kp.len());
    let n = 200;
    time("prepare both (parse)", 50, || {
        let _ = Prepared::new(&fa.t1, Some(&fa.t2)).unwrap();
        let _ = Prepared::new(&fb.t1, Some(&fb.t2)).unwrap();
    });
    time("scan direct 512x512", n, || {
        std::hint::black_box(match_state(&ca.desc, &cb.desc));
    });
    time("scan mirror 512x512", n, || {
        std::hint::black_box(match_state(&ca.desc_m, &cb.desc));
    });
    let sd = match_state(&ca.desc, &cb.desc);
    let sm = match_state(&ca.desc_m, &cb.desc);
    time("corr_42 both (alloc+sort)", n, || {
        std::hint::black_box(corr_42(&sd, &ca.kp, &cb.kp, &prof));
        std::hint::black_box(corr_42(&sm, &ca.kp, &cb.kp, &prof));
    });
    let pd = corr_42(&sd, &ca.kp, &cb.kp, &prof);
    let pm = corr_42(&sm, &ca.kp, &cb.kp, &prof);
    println!("pools direct {} mirror {}", pd.len(), pm.len());
    time("v3 lean secondaries (6 channels)", n, || {
        let mut ctx = PairCtx::new(ca, cb);
        std::hint::black_box(compare_canonical(&mut ctx, swapped, &cfg, Reading::Lean));
    });
    time("BagShare (7 bag matrices)", n, || {
        std::hint::black_box(BagShare::new(&ca.bag, &cb.bag));
    });
    let share = BagShare::new(&ca.bag, &cb.bag);
    time("local_v4 (5 assignments)", n, || {
        std::hint::black_box(local_v4_41_shared(&ca.bag, &cb.bag, &prof, &share));
    });
    let am = paph::compare::mirror_side(&ca.kp, ca.t1_xmax());
    let (mda, mdb) = (ca.t1_max_dim(), cb.t1_max_dim());
    let mut sc = Box::new(Scratch::new());
    time("geo_measure_42 (extraction)", n, || {
        std::hint::black_box(geo_measure_42(&ca.kp, &am, &cb.kp, &pd, &pm, &cfg, &prof, mda, mdb, &mut sc));
    });
    let (mm, _, _) = geo_measure_42(&ca.kp, &am, &cb.kp, &pd, &pm, &cfg, &prof, mda, mdb, &mut sc);
    println!("models {} inliers {}", mm.models.len(), mm.total_inliers);
    time("gn_control_42 (5 nulls)", n, || {
        std::hint::black_box(gn_control_42(&ca.kp, &am, &cb.kp, &pd, &pm, &cfg, &prof, mda, mdb, &mut sc));
    });
    time("diversity_42", n, || {
        std::hint::black_box(diversity_42(&mm, &ca.kp, &cb.kp, &prof));
    });
    time("compare_v42_lean (total)", n, || {
        std::hint::black_box(paph::v42::compare_v42_lean(&a0, &b0, &cfg, &prof));
    });
    time("screen_v42_prepared", n, || {
        std::hint::black_box(paph::v42::screen_v42_prepared(&a0, &b0, &cfg, &prof));
    });
    let r = paph::v42::compare_v42_lean(&a0, &b0, &cfg, &prof);
    println!("verdict {} structural {} geo {} inliers {}", r.base.verdict, r.base.structural, r.base.geometry_evidence, r.base.total_inliers);

    // ---- PAPH-X stages
    use paph::x::compare::{xcompare, xscreen, XCtx, XOptions};
    use paph::x::profile::{POLICY_FAST, POLICY_SAFE};
    use paph::x::route::route_score;
    use paph::x::{XBound, XPrepared};
    let mut xpp = paph::x::XProfile::x1();
    if let Ok(v) = std::env::var("XCAP") { xpp.hot_bucket_cap = v.parse().unwrap(); }
    if let Ok(v) = std::env::var("XSOFT") { xpp.hot_soft = v.parse().unwrap(); }
    let xb = XBound::new(paph::calibration::Profile::cal004(), xpp);
    let t = std::time::Instant::now();
    let xa = XPrepared::new(Prepared::new(&fa.t1, Some(&fa.t2)).unwrap(), &xb);
    let xb_ = XPrepared::new(Prepared::new(&fb.t1, Some(&fb.t2)).unwrap(), &xb);
    println!("xprepare both                      {:9.3} us", t.elapsed().as_secs_f64() * 1e6);
    {
        // bucket statistics of B's index and the bit balance of its descriptors
        let ix = &xb_.index;
        let n = ix.n;
        let mut hot = 0usize;
        let mut maxb = 0usize;
        let mut sum_sq = 0u64;
        for p in 0..24 {
            for c in 0..paph::x::profile::LSH_BUCKETS as u16 {
                let l = ix.bucket_len(p, c);
                maxb = maxb.max(l);
                if l > 32 { hot += 1; }
                sum_sq += (l * l) as u64;
            }
        }
        // expected candidates per random query = Σ over projections of E[bucket size of the query's code] = Σ_p Σ_c (l_c/n)·l_c
        println!("index B: n {} max bucket {} hot buckets(>32) {} expected touched per row {:.1} (uniform would be {:.1})", n, maxb, hot, sum_sq as f64 / n.max(1) as f64, 24.0 * n as f64 / paph::x::profile::LSH_BUCKETS as f64);
        let mut ones = [0u32; 256];
        for d in xb_.p.desc.iter() { for b in 0..256usize { ones[b] += ((d.q[b >> 6] >> (b & 63)) & 1) as u32; } }
        let mut bal: Vec<f64> = ones.iter().map(|&o| o as f64 / n.max(1) as f64).collect();
        bal.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("bit balance p(1): min {:.2} p10 {:.2} median {:.2} p90 {:.2} max {:.2}", bal[0], bal[25], bal[128], bal[230], bal[255]);
    }
    let (xa, xbp) = if swapped { (&xb_, &xa) } else { (&xa, &xb_) };
    let mut ctx = XCtx::new();
    time("x route score", 2000, || {
        std::hint::black_box(route_score(&xa.route, &xbp.route));
    });
    {
        // the nomination phase alone: bucket lookups for the first 96 anchors
        use paph::x::bucket::{mirror_proj, HOT_FLAG};
        let (ra, cb_) = if xbp.kp_len() < xa.kp_len() { (xbp, xa) } else { (xa, xbp) };
        let mut probes = 0u64; let mut hits = 0u64; let mut entries = 0u64; let mut hot = 0u64;
        let t = std::time::Instant::now();
        for _ in 0..n {
            probes = 0; hits = 0; entries = 0; hot = 0;
            for k in 0..96.min(ra.order.len()) {
                let i = ra.order[k] as usize;
                let qc = &ra.index.codes[i * 24..(i + 1) * 24];
                for p in 0..24 {
                    for cc in [qc[p], qc[mirror_proj(p)]] {
                        probes += 1;
                        if cc & HOT_FLAG != 0 { hot += 1; continue; }
                        let bk = cb_.index.bucket(p, cc);
                        if !bk.is_empty() { hits += 1; entries += bk.len() as u64; }
                        std::hint::black_box(bk);
                    }
                }
            }
        }
        println!("x nominate 96 rows (lookups only)  {:9.3} us   probes {} hot-skipped {} hits {} entries {}", t.elapsed().as_secs_f64() * 1e6 / n as f64, probes, hot, hits, entries);
        let hamm = entries.max(1);
        let t = std::time::Instant::now();
        let mut acc = 0u32;
        for _ in 0..n {
            for k in 0..96.min(ra.order.len()) {
                let i = ra.order[k] as usize;
                for j in 0..((entries / 96) as usize).min(cb_.kp_len()) {
                    acc = acc.wrapping_add(paph::geom42::hamming(&ra.p.desc[i], &cb_.p.desc[(j * 37 + i) % cb_.kp_len()]) as u32);
                }
            }
        }
        println!("x hamming {} scattered pairs            {:9.3} us", hamm, t.elapsed().as_secs_f64() * 1e6 / n as f64);
        std::hint::black_box(acc);
    }
    time("x scan 96 anchors (both hyps)", n, || {
        ctx.m.begin(xa, xbp);
        ctx.m.scan_rows(xa, xbp, &xb.xp, 96);
    });
    println!("   stats after 96 rows {:?}", ctx.m.stats);
    time("x scan all rows (both hyps)", n, || {
        ctx.m.begin(xa, xbp);
        ctx.m.scan_rows(xa, xbp, &xb.xp, 512);
    });
    ctx.m.begin(xa, xbp);
    ctx.m.scan_rows(xa, xbp, &xb.xp, 512);
    println!("   stats {:?}", ctx.m.stats);
    time("x count direct+mirror", n, || {
        std::hint::black_box(ctx.m.count(false, xa, xbp, &prof, 8));
        std::hint::black_box(ctx.m.count(true, xa, xbp, &prof, 8));
    });
    time("x pools (sorted)", n, || {
        ctx.m.pools(xa, xbp, &prof);
    });
    println!("   pools {} / {}", ctx.m.pd.len(), ctx.m.pm.len());
    {
        use paph::x::geom::{control, measure, Frames};
        let am = paph::compare::mirror_side(&xa.p.kp, xa.p.t1_xmax());
        let f = Frames { a: &xa.p.kp, am: &am, b: &xbp.p.kp, mda: xa.p.t1_max_dim(), mdb: xbp.p.t1_max_dim() };
        let bc = xb.bind(&cfg);
        time("x geometry measure (sparse pools)", n, || {
            std::hint::black_box(measure(&f, &ctx.m.pd, &ctx.m.pm, &bc, &prof, &mut ctx.g));
        });
        time("x geometry control (5 nulls)", n, || {
            std::hint::black_box(control(&f, &ctx.m.pd, &ctx.m.pm, &bc, &prof, &mut ctx.g, 16));
        });
    }
    {
        use paph::x::structural::*;
        let mut d0 = vec![0u8; 128 * 128];
        time("x structural: runs+pal+sil+topo", n, || {
            let mut s = Structural::new(xa, xbp, &prof);
            for k in [CH_RUNS, CH_PALETTE, CH_SILHOUETTE, CH_TOPOLOGY] {
                s.compute(k, xa, xbp, &prof, &mut d0);
            }
            std::hint::black_box(s.bounds(&prof));
        });
        time("x structural: local bound", n, || {
            let mut s = Structural::new(xa, xbp, &prof);
            s.bound_local(xa, xbp, &prof, &mut d0);
            std::hint::black_box(s.bounds(&prof));
        });
        time("x structural: shape", n, || {
            std::hint::black_box(shape_value(&xa.p.t1, &xbp.p.t1));
        });
        time("x structural: dct", n, || {
            std::hint::black_box(dct_value(&xa.p.t1, &xbp.p.t1));
        });
        time("x structural: local exact", n, || {
            std::hint::black_box(local_exact(xa, xbp, &prof));
        });
    }
    time("xscreen", n, || {
        std::hint::black_box(xscreen(xa, xbp, &cfg, &xb, &mut ctx));
    });
    time("xcompare fast", n, || {
        std::hint::black_box(xcompare(xa, xbp, &cfg, &xb, &XOptions { policy: Some(POLICY_FAST), audit: false, scope: paph::x::compare::Scope::Full }, &mut ctx));
    });
    time("xcompare safe", n, || {
        std::hint::black_box(xcompare(xa, xbp, &cfg, &xb, &XOptions { policy: Some(POLICY_SAFE), audit: false, scope: paph::x::compare::Scope::Full }, &mut ctx));
    });
    {
        // recall diagnostics: the exhaustive pools against the sparse ones
        let bc = xb.bind(&cfg);
        let sd = paph::prepared::match_state(&xa.p.desc, &xbp.p.desc);
        let sm = paph::prepared::match_state(&xa.p.desc_m, &xbp.p.desc);
        let pd = paph::prepared::corr_42(&sd, &xa.p.kp, &xbp.p.kp, &prof);
        let pm = paph::prepared::corr_42(&sm, &xa.p.kp, &xbp.p.kp, &prof);
        ctx.m.begin(xa, xbp);
        ctx.m.scan_rows(xa, xbp, &xb.xp, 512);
        ctx.m.pools(xa, xbp, &prof);
        let mut hist = [0usize; 9];
        for c in pd.iter().chain(pm.iter()) {
            hist[(c.d1 as usize / 12).min(8)] += 1;
        }
        println!("exhaustive pools {}/{} (d1 histogram by 12: {:?}); sparse pools {}/{} after all rows", pd.len(), pm.len(), hist, ctx.m.pd.len(), ctx.m.pm.len());
        let mut found = 0;
        let mut nominated = 0;
        for c in pd.iter() {
            if ctx.m.direct.a_best[c.a] == c.b as u16 { found += 1; }
            // was the pair nominated at all?  codes equal on some projection
            let mut shared = 0;
            for p in 0..24 { if xa.index.code(p, c.a) & 0x0fff == xbp.index.code(p, c.b) & 0x0fff { shared += 1; } }
            if shared > 0 { nominated += 1; }
        }
        println!("of {} exhaustive direct correspondences: {} share a projection code, {} are the sparse best", pd.len(), nominated, found);
        let _ = bc;
    }
    let rx = xcompare(xa, xbp, &cfg, &xb, &XOptions { policy: Some(POLICY_SAFE), audit: false, scope: paph::x::compare::Scope::Full }, &mut ctx);
    println!("X verdict {} {} ({}) route {:?} {:?}", rx.verdict, rx.execution.name(), rx.reason, rx.route, rx.route_class);
    if let Some(g) = &rx.geometry { println!("  geo inliers {} models {} evidence {} cert {} rows {} pools {}/{} ctl ran {}", g.total_inliers, g.models.len(), g.evidence, g.certificate, g.expanded_to, g.pool_direct, g.pool_mirror, g.ctl_ran); }
    if let Some(st) = &rx.structural { println!("  structural [{}, {}] exact {} known {:?}", st.lo, st.hi, st.exact, st.channels.iter().map(|c| (c.0, c.3)).collect::<Vec<_>>()); }
}

#[allow(dead_code)]
fn unused() {}
