//! The PAPH-X benchmark and acceptance harness (specification §33–§35).
//!
//!     xbench [--quick] [--calibrate] [--noreject] [--explain] [--table] [--dump DIR]
//!            [--profile out.pxcl] [--xprofile in.pxcl] [--json out.json]
//!
//! Builds the synthetic corpus of §33.1 from the engine's own generators
//! (`synth.rs` — the same pictures the equivalence digest hashes), runs
//! comparator 42 and PAPH-X over every pair class of §33.2, and reports the
//! metrics of §33.3: percentiles, allocations, full-Hamming pairs before and
//! after, fallback rate, and the copy-recall / precision agreement between
//! the two.  `--calibrate` derives the route bars of profile X1 from the
//! corpus (no comparator-42 Copy may be rejected) and prints them.
//!
//! Nothing printed here is a claim until it is printed here (§41.7).

use paph::calibration::Profile;
use paph::compare::Reading;
use paph::config::Config;
use paph::keypoints::{pattern, RotCache};
use paph::prepared::{canon_swapped, PairCtx, Prepared};
use paph::synth::*;
use paph::v42::{compare_in, screen_in};
use paph::wire::hash;
use paph::x::compare::{xcompare, xscreen, Execution, ScreenState, XCtx, XOptions};
use paph::x::profile::{POLICY_FAST, POLICY_SAFE};
use paph::x::rank::{xrank, RankScratch, XRankOptions, XRANK_FIELDS};
use paph::x::route::{route_batch, route_class, route_score, RouteClass, RouteScore, RouteSoA};
use paph::x::{XBound, XPrepared, XProfile};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

// ------------------------------------------------------- counting allocator

struct Counting;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(l.size() as u64, Ordering::Relaxed);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(n as u64, Ordering::Relaxed);
        System.realloc(p, l, n)
    }
}

#[global_allocator]
static A: Counting = Counting;

fn allocs() -> (u64, u64) {
    (ALLOCS.load(Ordering::Relaxed), BYTES.load(Ordering::Relaxed))
}

// ------------------------------------------------------------------ corpus

struct Work {
    name: String,
    family: usize,
    /// "base", a transform name, "same-style", "random", "texture", "noise"
    class: &'static str,
    img: Img,
}

fn texture_wall(w: usize, h: usize, seed: u64) -> Img {
    // one 12x12 tile, repeated — the repeated-texture attack of §37.2
    let tile = pixel_art(12, 12, seed, 5, 2);
    let mut o = Img::new(w, h);
    for y in 0..h {
        for x in 0..w {
            o.set(x as i64, y as i64, tile.get(x % 12, y % 12));
        }
    }
    o
}

fn noise(w: usize, h: usize, seed: u64) -> Img {
    // the v42 fixture: a shared noise field under coloured squares, whose
    // descriptors collide across seeds (the adversarial collision class)
    let mut im = Img::new(w, h);
    for y in 0..h {
        for x in 0..w {
            im.set(x as i64, y as i64, [((x * 7 + y * 3 + seed as usize) % 251) as u8, ((y * 11 + x * 5) % 253) as u8, (((x ^ y) * 13) % 247) as u8, 255]);
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
        let c = [r(256) as u8, r(256) as u8, r(256) as u8, 255];
        for dy in 0..6 {
            for dx in 0..6 {
                im.set((bx + dx) as i64, (by + dy) as i64, c);
            }
        }
    }
    im
}

fn dither_change(a: &Img) -> Img {
    // re-dither: swap the two backdrop colours' Bayer roles where the
    // backdrop shows (approximately: shift the threshold pattern by one)
    let mut o = a.clone();
    for y in 0..a.h {
        for x in 0..a.w {
            let p = a.get(x, y);
            let q = a.get((x + 1) % a.w, y);
            if (x + y) % 2 == 0 && p[3] == 255 && q[3] == 255 {
                o.set(x as i64, y as i64, q);
            }
        }
    }
    o
}

fn matte_variant(a: &Img) -> Img {
    // put a transparent work on a flat matte, or an opaque one on a border
    let mut o = Img::new(a.w + 8, a.h + 8);
    for y in 0..o.h {
        for x in 0..o.w {
            o.set(x as i64, y as i64, [40, 44, 60, 255]);
        }
    }
    for y in 0..a.h {
        for x in 0..a.w {
            let p = a.get(x, y);
            if p[3] >= 128 {
                o.set((x + 4) as i64, (y + 4) as i64, [p[0], p[1], p[2], 255]);
            }
        }
    }
    o
}

fn corpus(quick: bool) -> Vec<Work> {
    let mut v: Vec<Work> = Vec::new();
    let mut r = Rng(0x7061_7068_5f78_0001);
    let nb = if quick { 6 } else { 14 };
    let mut fam = 0usize;
    let push = |v: &mut Vec<Work>, name: String, family: usize, class: &'static str, img: Img| {
        v.push(Work { name, family, class, img });
    };
    for i in 0..nb {
        let (w, h) = match i % 7 {
            0 => (160, 120),
            1 => (96, 96),
            2 => (240, 180),
            3 => (300, 220),
            4 => (120, 90),
            5 => (400, 90),
            _ => (200, 150),
        };
        let bg = (i % 3) as u8;
        let ncol = 4 + (i * 3) % 14;
        let b = if i % 5 == 4 { work(w, h, 7 + i as i64, i % 2 == 0) } else { pixel_art(w, h, 11 + 97 * i as u64, ncol, bg) };
        let n = format!("B{i:02}");
        push(&mut v, n.clone(), fam, "base", b.clone());
        push(&mut v, format!("{n}~mirror"), fam, "mirror", mirror(&b));
        push(&mut v, format!("{n}~rot90"), fam, "rot90", rot90(&b));
        push(&mut v, format!("{n}~rot180"), fam, "rot180", rot90(&rot90(&b)));
        push(&mut v, format!("{n}~transpose"), fam, "transpose", transpose(&b));
        push(&mut v, format!("{n}~invert"), fam, "invert", invert(&b));
        push(&mut v, format!("{n}~recolour"), fam, "recolour", recolour(&b));
        push(&mut v, format!("{n}~up2"), fam, "up2", nearest_up(&b, 2));
        push(&mut v, format!("{n}~up3"), fam, "up3", nearest_up(&b, 3));
        push(&mut v, format!("{n}~down"), fam, "down", area_down(&b, (b.w * 7 / 10).max(8), (b.h * 7 / 10).max(8)));
        push(&mut v, format!("{n}~resample"), fam, "resample", area_down(&b, (b.w * 9 / 10).max(8), (b.h * 9 / 10).max(8)));
        push(&mut v, format!("{n}~crop"), fam, "crop", crop(&b, b.w / 6, b.h / 7, (b.w * 2 / 3).max(8), (b.h * 2 / 3).max(8)));
        push(&mut v, format!("{n}~corner"), fam, "corner", crop(&b, b.w / 2, b.h / 2, (b.w / 2).max(8), (b.h / 2).max(8)));
        let host = pixel_art(b.w * 2 + 20, b.h * 2 + 16, 997 + i as u64, 9, 2);
        push(&mut v, format!("{n}~paste"), fam, "paste", paste(&b, &host, b.w / 2 + 3, b.h / 3 + 5));
        push(&mut v, format!("{n}~shift1"), fam, "shift1", shift1(&b));
        push(&mut v, format!("{n}~dither"), fam, "dither", dither_change(&b));
        push(&mut v, format!("{n}~matte"), fam, "matte", matte_variant(&b));
        fam += 1;
    }
    // unrelated same-style works, random images, texture walls, noise
    let ns = if quick { 10 } else { 30 };
    for k in 0..ns {
        let w = 60 + r.below(300) as usize;
        let h = 50 + r.below(220) as usize;
        push(&mut v, format!("S{k:02}"), fam, "same-style", pixel_art(w, h, 5000 + r.next() % 100_000, 3 + r.below(20) as usize, r.below(3) as u8));
        fam += 1;
    }
    // the reference class of §3.1: busy works at the 512-keypoint budget
    for k in 0..(if quick { 8 } else { 60 }) {
        push(&mut v, format!("L{k:02}"), fam, "large", pixel_art(300 + 8 * (k % 5), 220 + 6 * (k % 4), 70_000 + r.next() % 100_000, 12 + r.below(10) as usize, 2));
        fam += 1;
    }
    for k in 0..(if quick { 4 } else { 8 }) {
        let w = 64 + r.below(200) as usize;
        let h = 64 + r.below(200) as usize;
        let mut im = Img::new(w, h);
        for i in 0..w * h {
            let x = r.next();
            im.px[i * 4..i * 4 + 4].copy_from_slice(&[x as u8, (x >> 8) as u8, (x >> 16) as u8, 255]);
        }
        push(&mut v, format!("R{k:02}"), fam, "random", im);
        fam += 1;
    }
    for k in 0..(if quick { 2 } else { 4 }) {
        push(&mut v, format!("T{k:02}"), fam, "texture", texture_wall(96 + 48 * k, 96 + 24 * k, 77 + k as u64));
        fam += 1;
    }
    for k in 0..(if quick { 2 } else { 4 }) {
        push(&mut v, format!("N{k:02}"), fam, "noise", noise(144, 112, 1000 + 7 * k as u64));
        fam += 1;
    }
    v
}

// ------------------------------------------------------------------ timing

fn pct(v: &mut Vec<f64>, p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let i = ((v.len() as f64 - 1.0) * p).round() as usize;
    v[i.min(v.len() - 1)]
}

struct Timed {
    us: Vec<f64>,
}

impl Timed {
    fn new() -> Timed {
        Timed { us: Vec::new() }
    }
    fn add(&mut self, t: Instant) {
        self.us.push(t.elapsed().as_secs_f64() * 1e6);
    }
    fn row(&mut self) -> String {
        let (a, b, c, d) = (pct(&mut self.us, 0.5), pct(&mut self.us, 0.9), pct(&mut self.us, 0.95), pct(&mut self.us, 0.99));
        format!("p50 {:8.1}  p90 {:8.1}  p95 {:8.1}  p99 {:8.1} us  (n={})", a, b, c, d, self.us.len())
    }
    fn p50(&mut self) -> f64 {
        pct(&mut self.us, 0.5)
    }
    fn p95(&mut self) -> f64 {
        pct(&mut self.us, 0.95)
    }
}

// --------------------------------------------------------------- the run

struct PairRec {
    class: String,
    positive: bool,
    v42: &'static str,
    s42: bool,
    xs: ScreenState,
    route: RouteScore,
    rc: RouteClass,
    pool: usize,
    xsafe: &'static str,
    xsafe_exec: Execution,
    xfast: &'static str,
    xfast_exec: Execution,
    ham42: u64,
    hamx: u64,
    i: usize,
    j: usize,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let quick = args.iter().any(|a| a == "--quick");
    let calibrate = args.iter().any(|a| a == "--calibrate");
    let prof_out = args.iter().position(|a| a == "--profile").map(|i| args[i + 1].clone());
    let xp_in = args.iter().position(|a| a == "--xprofile").map(|i| args[i + 1].clone());
    let cfg = Config::default();
    let mut xp = match xp_in {
        Some(p) => XProfile::decode(&std::fs::read(p).expect("read xprofile")).expect("decode xprofile"),
        None => XProfile::x1(),
    };
    // overrides for experiments: --cert N (certificate inlier floor),
    // --cells N (certificate spread), --bars l,b,g (route lower bars)
    if let Some(i) = args.iter().position(|a| a == "--cert") {
        xp.cert_min_inliers = args[i + 1].parse().unwrap();
    }
    if let Some(i) = args.iter().position(|a| a == "--cells") {
        xp.cert_min_cells = args[i + 1].parse().unwrap();
    }
    if let Some(i) = args.iter().position(|a| a == "--cap") {
        xp.hot_bucket_cap = args[i + 1].parse().unwrap();
    }
    if let Some(i) = args.iter().position(|a| a == "--soft") {
        xp.hot_soft = args[i + 1].parse().unwrap();
    }
    if args.iter().any(|a| a == "--noreject") {
        // route hard-negatives defer to the sparse screen instead of
        // reading Unrelated by calibration
        xp.route_reject_unrelated = 0;
    }
    if let Some(i) = args.iter().position(|a| a == "--maxc") {
        xp.max_sparse_candidates = args[i + 1].parse().unwrap();
    }
    if let Some(i) = args.iter().position(|a| a == "--bars") {
        let v: Vec<i32> = args[i + 1].split(',').map(|x| x.parse().unwrap()).collect();
        xp.t_local_low = v[0];
        xp.t_band_low = v[1];
        xp.t_global_low = v[2];
    }
    let xb_ = XBound::new(Profile::cal004(), xp);
    let (base, xp) = (xb_.base.clone(), xb_.xp.clone());
    println!("PAPH-X benchmark — base {} ({}), X {} ({}){}", base.name_str(), base.id_hex16(), xp.name_str(), xp.id_hex16(), if quick { " [quick]" } else { "" });
    let rot = RotCache::new(&pattern());
    let works = corpus(quick);
    println!("corpus: {} works", works.len());
    if let Some(i) = args.iter().position(|a| a == "--dump") {
        let dir = &args[i + 1];
        std::fs::create_dir_all(dir).unwrap();
        for w in works.iter() {
            let mut b = Vec::with_capacity(8 + w.img.px.len());
            b.extend_from_slice(&(w.img.w as u32).to_le_bytes());
            b.extend_from_slice(&(w.img.h as u32).to_le_bytes());
            b.extend_from_slice(&w.img.px);
            std::fs::write(format!("{}/{}.rgba", dir, w.name), b).unwrap();
        }
        println!("dumped {} works to {}", works.len(), dir);
        return;
    }

    // hash and prepare
    let t0 = Instant::now();
    let mut th = Timed::new();
    let mut tp = Timed::new();
    let mut txp = Timed::new();
    let mut sides: Vec<(Prepared, XPrepared)> = Vec::with_capacity(works.len());
    for w in works.iter() {
        let t = Instant::now();
        let f = hash(&w.img.px, w.img.w, w.img.h, &cfg, &rot);
        th.add(t);
        let t = Instant::now();
        let p = Prepared::new(&f.t1, Some(&f.t2)).unwrap();
        tp.add(t);
        let p2 = Prepared::new(&f.t1, Some(&f.t2)).unwrap();
        let t = Instant::now();
        let x = XPrepared::new(p2, &xb_);
        txp.add(t);
        sides.push((p, x));
    }
    println!("hash      {}\nprepare42 {}\nxprepare  {}  (total {:.1} s)", th.row(), tp.row(), txp.row(), t0.elapsed().as_secs_f64());

    // pairs: every family's base against its transforms (positives), bases
    // against every other base / same-style / random / texture / noise
    // (negatives), plus every transform against a few negatives
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let bases: Vec<usize> = works.iter().enumerate().filter(|(_, w)| w.class == "base").map(|(i, _)| i).collect();
    let negs: Vec<usize> = works.iter().enumerate().filter(|(_, w)| matches!(w.class, "same-style" | "random" | "texture" | "noise" | "large")).map(|(i, _)| i).collect();
    for &b in bases.iter() {
        let fam = works[b].family;
        for (j, w) in works.iter().enumerate() {
            if w.family == fam && j != b {
                pairs.push((b, j));
            }
        }
        for &o in bases.iter() {
            if o > b {
                pairs.push((b, o));
            }
        }
        for &n in negs.iter() {
            pairs.push((b, n));
        }
    }
    let mut r = Rng(42);
    for _ in 0..(if quick { 120 } else { 600 }) {
        let i = r.below(works.len() as u64) as usize;
        let j = r.below(works.len() as u64) as usize;
        if works[i].family != works[j].family {
            pairs.push((i, j));
        }
    }
    println!("pairs: {}", pairs.len());

    let mut ctx = XCtx::new();
    let mut recs: Vec<PairRec> = Vec::with_capacity(pairs.len());
    let (mut t_s42, mut t_c42, mut t_xs, mut t_xf, mut t_xsafe) = (Timed::new(), Timed::new(), Timed::new(), Timed::new(), Timed::new());
    let mut class_t: std::collections::BTreeMap<String, (Timed, Timed, Timed, Timed, Timed)> = std::collections::BTreeMap::new();
    let (mut al0, mut al1) = ((0u64, 0u64), (0u64, 0u64));
    let mut screen_allocs = 0u64;
    for (k, &(i, j)) in pairs.iter().enumerate() {
        let (pa, xa) = &sides[i];
        let (pb, xb) = &sides[j];
        let positive = works[i].family == works[j].family;
        let class = if positive { works[j].class.to_string() } else { format!("neg:{}", works[j].class) };
        // comparator 42: screen and lean compare, each on a fresh pair
        // context, as the search integration runs them
        let swapped = canon_swapped(pa, pb);
        let (ca, cb) = if swapped { (pb, pa) } else { (pa, pb) };
        let bcfg = paph::v4::bind(&cfg, &base);
        let t = Instant::now();
        let mut pc = PairCtx::new(ca, cb);
        let s42 = screen_in(&mut pc, &bcfg, &base);
        t_s42.add(t);
        let t = Instant::now();
        let mut pc = PairCtx::new(ca, cb);
        let r42 = compare_in(&mut pc, swapped, &bcfg, &base, Reading::Lean);
        t_c42.add(t);
        // PAPH-X
        if k == 0 {
            // warm the scratch so the allocation count below is the steady state
            let _ = xscreen(xa, xb, &cfg, &xb_, &mut ctx);
            let _ = xcompare(xa, xb, &cfg, &xb_, &XOptions { policy: Some(POLICY_SAFE), audit: false, scope: paph::x::compare::Scope::Full }, &mut ctx);
            al0 = allocs();
        }
        let t = Instant::now();
        let xs = xscreen(xa, xb, &cfg, &xb_, &mut ctx);
        t_xs.add(t);
        if k == 0 {
            al1 = allocs();
        }
        screen_allocs += 0;
        let t = Instant::now();
        let xf = xcompare(xa, xb, &cfg, &xb_, &XOptions { policy: Some(POLICY_FAST), audit: false, scope: paph::x::compare::Scope::Full }, &mut ctx);
        t_xf.add(t);
        let t = Instant::now();
        let xsafe = xcompare(xa, xb, &cfg, &xb_, &XOptions { policy: Some(POLICY_SAFE), audit: false, scope: paph::x::compare::Scope::Full }, &mut ctx);
        t_xsafe.add(t);
        let e = class_t.entry(class.clone()).or_insert_with(|| (Timed::new(), Timed::new(), Timed::new(), Timed::new(), Timed::new()));
        e.0.us.push(*t_s42.us.last().unwrap());
        e.1.us.push(*t_c42.us.last().unwrap());
        e.2.us.push(*t_xs.us.last().unwrap());
        e.3.us.push(*t_xf.us.last().unwrap());
        e.4.us.push(*t_xsafe.us.last().unwrap());
        let pool = xs.direct.count.max(xs.mirror.count);
        recs.push(PairRec {
            class,
            positive,
            v42: r42.base.verdict,
            s42: s42.pass,
            xs: xs.state,
            route: xs.route,
            rc: xs.route_class,
            pool,
            xsafe: xsafe.verdict,
            xsafe_exec: xsafe.execution,
            xfast: xf.verdict,
            xfast_exec: xf.execution,
            ham42: 2 * (ca.kp.len() * cb.kp.len()) as u64,
            hamx: xsafe.stats.hammings,
            i,
            j,
        });
    }
    let _ = screen_allocs;

    // allocation audit of the screen hot path: many screens, zero allocations
    let (a0, b0) = allocs();
    let (xa, xb) = (&sides[bases[0]].1, &sides[bases[1]].1);
    for _ in 0..200 {
        std::hint::black_box(xscreen(xa, xb, &cfg, &xb_, &mut ctx));
    }
    let (a1, b1) = allocs();
    println!("\n== allocations: 200 xscreen calls: {} allocations, {} bytes (first-call warm-up: {} allocations)", a1 - a0, b1 - b0, al1.0 - al0.0);

    println!("\n== pairwise timings (native, one core)");
    println!("screen42      {}", t_s42.row());
    println!("xscreen       {}", t_xs.row());
    println!("compare42     {}", t_c42.row());
    println!("xcompare fast {}", t_xf.row());
    println!("xcompare safe {}", t_xsafe.row());
    let (s42p, xsp, c42p, xfp, xsafep) = (t_s42.p50(), t_xs.p50(), t_c42.p50(), t_xf.p50(), t_xsafe.p50());
    println!("speedups p50: screen {:.1}x  compare fast {:.1}x  compare safe {:.1}x;  screen p95 {:.1}x", s42p / xsp, c42p / xfp, c42p / xsafep, t_s42.p95() / t_xs.p95());

    println!("\n== per class (p50 us): screen42 / xscreen / compare42 / xfast / xsafe");
    for (c, t) in class_t.iter_mut() {
        println!("{:<16} {:8.1} {:8.1} {:8.1} {:8.1} {:8.1}   n={}", c, t.0.p50(), t.2.p50(), t.1.p50(), t.3.p50(), t.4.p50(), t.0.us.len());
    }

    // agreement
    println!("\n== agreement with comparator 42 (lean), per class");
    println!("{:<16} {:>5} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7} {:>8} {:>8}", "class", "n", "42copy", "Xsafe=", "Xsafe≠", "fbk%", "Xfast=", "defer%", "scr42%", "xscr%");
    let mut classes: Vec<String> = recs.iter().map(|r| r.class.clone()).collect();
    classes.sort();
    classes.dedup();
    let is_copy = |v: &str| v == "Copy" || v == "Identical";
    let (mut tot_fb, mut tot_n) = (0usize, 0usize);
    let (mut ham42_sum, mut hamx_sum) = (0u64, 0u64);
    let mut screen_missed: Vec<String> = Vec::new();
    let mut safe_missed: Vec<String> = Vec::new();
    let mut fast_fp: Vec<String> = Vec::new();
    for c in classes.iter() {
        let rs: Vec<&PairRec> = recs.iter().filter(|r| &r.class == c).collect();
        let n = rs.len();
        let c42 = rs.iter().filter(|r| is_copy(r.v42)).count();
        let eq_safe = rs.iter().filter(|r| r.xsafe == r.v42).count();
        let fb = rs.iter().filter(|r| r.xsafe_exec == Execution::Fallback).count();
        let eq_fast = rs.iter().filter(|r| r.xfast_exec == Execution::Fast && r.xfast == r.v42).count();
        let defer = rs.iter().filter(|r| r.xfast_exec == Execution::Deferred).count();
        let s42 = rs.iter().filter(|r| r.s42).count();
        let xs = rs.iter().filter(|r| r.xs == ScreenState::Pass || r.xs == ScreenState::Defer || r.xs == ScreenState::Identical).count();
        println!("{:<16} {:>5} {:>7} {:>7} {:>7} {:>6.1}% {:>7} {:>6.1}% {:>7.1}% {:>7.1}%", c, n, c42, eq_safe, n - eq_safe, 100.0 * fb as f64 / n.max(1) as f64, eq_fast, 100.0 * defer as f64 / n.max(1) as f64, 100.0 * s42 as f64 / n.max(1) as f64, 100.0 * xs as f64 / n.max(1) as f64);
        tot_fb += fb;
        tot_n += n;
        for r in rs.iter() {
            ham42_sum += r.ham42;
            hamx_sum += r.hamx;
            if is_copy(r.v42) && r.xs == ScreenState::Reject {
                screen_missed.push(format!("{} route {:?} {:?} pool {}", c, r.route, r.rc, r.pool));
            }
            if is_copy(r.v42) != is_copy(r.xsafe) {
                safe_missed.push(format!("{} 42 {} X {} ({})", c, r.v42, r.xsafe, r.xsafe_exec.name()));
            }
            if r.xfast_exec == Execution::Fast && is_copy(r.xfast) && !is_copy(r.v42) {
                fast_fp.push(format!("{} 42 {} X {}", c, r.v42, r.xfast));
            }
        }
    }
    println!("\nfallback rate (safe): {:.1}%   full Hamming pairs: 42 {}  X {}  ({:.1}% fewer)", 100.0 * tot_fb as f64 / tot_n.max(1) as f64, ham42_sum, hamx_sum, 100.0 * (1.0 - hamx_sum as f64 / ham42_sum.max(1) as f64));
    println!("screen hard-rejected a 42-Copy: {}  | safe copy-disagreements: {}  | fast false Copy: {}", screen_missed.len(), safe_missed.len(), fast_fp.len());
    // where the safe verdicts differ at all: by (42 verdict, X verdict, X execution)
    {
        let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for r in recs.iter().filter(|r| r.xsafe != r.v42) {
            *kinds.entry(format!("42 {:<10} → X {:<10} ({}) [{}]", r.v42, r.xsafe, r.xsafe_exec.name(), r.class)).or_insert(0) += 1;
        }
        let total: usize = kinds.values().sum();
        println!("safe verdict differences (any state): {} of {} pairs", total, recs.len());
        for (k, n) in kinds.iter() {
            println!("  {:>5}  {}", n, k);
        }
    }
    for s in screen_missed.iter().take(20) {
        println!("  SCREEN MISS  {s}");
    }
    for s in safe_missed.iter().take(20) {
        println!("  SAFE DIFF    {s}");
    }
    if args.iter().any(|a| a == "--explain") {
        for r in recs.iter().filter(|r| is_copy(r.v42) != is_copy(r.xsafe)) {
            let (pa, xa) = &sides[r.i];
            let (pb, xb) = &sides[r.j];
            let r42 = paph::v42::compare_v42_lean(pa, pb, &cfg, &base);
            let rx = xcompare(xa, xb, &cfg, &xb_, &XOptions { policy: Some(POLICY_SAFE), audit: false, scope: paph::x::compare::Scope::Full }, &mut ctx);
            let b = &r42.base;
            println!("\n-- {} x {} [{}]  kp {} x {}", works[r.i].name, works[r.j].name, r.class, pa.kp.len(), pb.kp.len());
            println!("   42: {} {:?} structural {} cert {} geo {} raw {} ctl {} ({}) inliers {} models {} topo {} div {} mult {} weak {}", b.verdict, b.basis, b.structural, b.certifiable, b.geometry_evidence, b.geo_raw, b.geo_ctl, b.geo_ctl_member, b.total_inliers, r42.models42.len(), b.topology, r42.diversity_geo.combined, r42.diversity_geo.multiplier, b.geo_weak_inliers);
            if let Some(l) = &b.local { println!("       local evidence {} matches {} margin {} div {} covmin {}", l.evidence, l.matches, l.margin, l.diversity, l.coverage_a.coverage.min(l.coverage_b.coverage)); }
            println!("    X: {} {:?} {} ({}) route {:?} {:?}", rx.verdict, rx.basis, rx.execution.name(), rx.reason, rx.route, rx.route_class);
            if let Some(g) = &rx.geometry { println!("       geo: evidence {} raw {} ctl {} ({}, ran {}) inliers {} models {} topo {} div {} mult {} cert {} pools {}/{} rows {} uniqueA {} uniqueB {}", g.evidence, g.raw, g.ctl, g.ctl_member, g.ctl_ran, g.total_inliers, g.models.len(), g.topology, g.diversity.combined, g.diversity.multiplier, g.certificate, g.pool_direct, g.pool_mirror, g.expanded_to, g.unique_a, g.unique_b); }
            if let Some(st) = &rx.structural { println!("       structural [{}, {}] exact {} channels {:?} local {:?} covmin {:?}", st.lo, st.hi, st.exact, st.channels, st.local_evidence, st.coverage_min); }
        }
    }
    for s in fast_fp.iter().take(20) {
        println!("  FAST FALSE+  {s}");
    }

    // rank benchmarks: three queries — the base with the most keypoints, the
    // median one, and the smallest — each against its family's transforms
    // and negatives up to N
    println!("\n== rank: one query against N candidates (native)");
    let mut rs = RankScratch::new();
    let mut by_kp: Vec<usize> = bases.clone();
    by_kp.sort_by_key(|&i| std::cmp::Reverse(sides[i].0.kp.len()));
    let queries = [by_kp[0], by_kp[by_kp.len() / 2], by_kp[by_kp.len() - 1]];
    for &ncand in [100usize, 1000].iter() {
        if ncand > 4 * works.len() && quick {
            continue;
        }
        let (mut sum42, mut sumx) = (0.0f64, 0.0f64);
        for &q in queries.iter() {
            let mut cands: Vec<usize> = works.iter().enumerate().filter(|(j, w)| w.family == works[q].family && *j != q).map(|(j, _)| j).collect();
            let mut k = 0usize;
            while cands.len() < ncand {
                let j = (k * 7919 + 13) % works.len();
                if works[j].family != works[q].family {
                    cands.push(j);
                }
                k += 1;
            }
            cands.truncate(ncand);
            let (pq, xq) = &sides[q];
            let bcfg = paph::v4::bind(&cfg, &base);
            let mut t42 = Timed::new();
            let mut surv42 = 0usize;
            let mut copies42 = 0usize;
            for _ in 0..(if ncand == 100 { 5 } else { 2 }) {
                let t = Instant::now();
                surv42 = 0;
                copies42 = 0;
                for &j in cands.iter() {
                    let pc_ = &sides[j].0;
                    let swapped = canon_swapped(pq, pc_);
                    let (ca, cb) = if swapped { (pc_, pq) } else { (pq, pc_) };
                    let mut pc = PairCtx::new(ca, cb);
                    let s = screen_in(&mut pc, &bcfg, &base);
                    if s.pass {
                        surv42 += 1;
                        let r = compare_in(&mut pc, swapped, &bcfg, &base, Reading::Lean);
                        if r.base.verdict == "Copy" || r.base.verdict == "Identical" {
                            copies42 += 1;
                        }
                    }
                }
                t42.add(t);
            }
            let refs: Vec<Option<&XPrepared>> = cands.iter().map(|&j| Some(&sides[j].1)).collect();
            let mut out = vec![0i32; ncand * XRANK_FIELDS];
            let mut tx = Timed::new();
            let (ma, _) = allocs();
            for _ in 0..(if ncand == 100 { 5 } else { 2 }) {
                let t = Instant::now();
                xrank(xq, &refs, &cfg, &xb_, &XRankOptions { policy: Some(POLICY_SAFE), gate: true, scope: paph::x::compare::Scope::Copy }, &mut ctx, &mut rs, &mut out);
                tx.add(t);
            }
            let (mb, _) = allocs();
            let survx = (0..ncand).filter(|&i| out[i * XRANK_FIELDS] >= 0).count();
            let fbx = (0..ncand).filter(|&i| out[i * XRANK_FIELDS + 23] & 8 != 0).count();
            let copies = (0..ncand).filter(|&i| (3..=4).contains(&out[i * XRANK_FIELDS])).count();
            println!("N={:<5} query {} ({} kp): rank42 {:9.1} us [{} survivors, {} copies]   xrank {:9.1} us [{} compared, {} fallbacks, {} copies]  → {:.1}x   ({} allocations/run)", ncand, works[q].name, pq.kp.len(), t42.p50(), surv42, copies42, tx.p50(), survx, fbx, copies, t42.p50() / tx.p50(), (mb - ma) / if ncand == 100 { 5 } else { 2 });
            sum42 += t42.p50();
            sumx += tx.p50();
        }
        println!("N={:<5} aggregate over the three queries: rank42 {:.1} us  xrank {:.1} us  → {:.1}x", ncand, sum42, sumx, sum42 / sumx);
        if ncand == 100 {
            // §3.1's reference workload: the largest query against 100
            // candidates of the 512-keypoint class — its own transforms
            // and busy unrelated works
            let q = queries[0];
            let mut cands: Vec<usize> = works.iter().enumerate().filter(|(j, w)| w.family == works[q].family && *j != q).map(|(j, _)| j).collect();
            let large: Vec<usize> = works.iter().enumerate().filter(|(_, w)| w.class == "large").map(|(j, _)| j).collect();
            let mut k = 0usize;
            while cands.len() < ncand && !large.is_empty() {
                cands.push(large[k % large.len()]);
                k += 1;
            }
            cands.truncate(ncand);
            let (pq, xq) = &sides[q];
            let bcfg = paph::v4::bind(&cfg, &base);
            let mut t42 = Timed::new();
            let (mut surv42, mut copies42) = (0usize, 0usize);
            for _ in 0..3 {
                let t = Instant::now();
                surv42 = 0;
                copies42 = 0;
                for &j in cands.iter() {
                    let pc_ = &sides[j].0;
                    let swapped = canon_swapped(pq, pc_);
                    let (ca, cb) = if swapped { (pc_, pq) } else { (pq, pc_) };
                    let mut pc = PairCtx::new(ca, cb);
                    let s = screen_in(&mut pc, &bcfg, &base);
                    if s.pass {
                        surv42 += 1;
                        let r = compare_in(&mut pc, swapped, &bcfg, &base, Reading::Lean);
                        if r.base.verdict == "Copy" || r.base.verdict == "Identical" {
                            copies42 += 1;
                        }
                    }
                }
                t42.add(t);
            }
            let refs: Vec<Option<&XPrepared>> = cands.iter().map(|&j| Some(&sides[j].1)).collect();
            let mut out = vec![0i32; ncand * XRANK_FIELDS];
            let mut tx = Timed::new();
            for _ in 0..3 {
                let t = Instant::now();
                xrank(xq, &refs, &cfg, &xb_, &XRankOptions { policy: Some(POLICY_SAFE), gate: true, scope: paph::x::compare::Scope::Copy }, &mut ctx, &mut rs, &mut out);
                tx.add(t);
            }
            let survx = (0..ncand).filter(|&i| out[i * XRANK_FIELDS] >= 0).count();
            let fbx = (0..ncand).filter(|&i| out[i * XRANK_FIELDS + 23] & 8 != 0).count();
            let copies = (0..ncand).filter(|&i| (3..=4).contains(&out[i * XRANK_FIELDS])).count();
            let kps: Vec<usize> = cands.iter().map(|&j| sides[j].0.kp.len()).collect();
            let mean_kp = kps.iter().sum::<usize>() / kps.len().max(1);
            println!("REFERENCE 512-kp class: query {} ({} kp) vs {} candidates (mean {} kp): rank42 {:.1} us [{} survivors, {} copies]   xrank {:.1} us [{} compared, {} fallbacks, {} copies]  → {:.1}x", works[q].name, pq.kp.len(), ncand, mean_kp, t42.p50(), surv42, copies42, tx.p50(), survx, fbx, copies, t42.p50() / tx.p50());
        }
        if ncand == 100 && args.iter().any(|a| a == "--explain") {
            // where the rank time goes for the largest query
            let q = queries[0];
            let mut cands: Vec<usize> = works.iter().enumerate().filter(|(j, w)| w.family == works[q].family && *j != q).map(|(j, _)| j).collect();
            let large: Vec<usize> = works.iter().enumerate().filter(|(_, w)| w.class == "large").map(|(j, _)| j).collect();
            let mut k = 0usize;
            while cands.len() < ncand && !large.is_empty() {
                cands.push(large[k % large.len()]);
                k += 1;
            }
            cands.truncate(ncand);
            let xq = &sides[q].1;
            let mut rows: Vec<(f64, f64, String)> = Vec::new();
            for &j in cands.iter() {
                let xc = &sides[j].1;
                let t = Instant::now();
                let sc = xscreen(xq, xc, &cfg, &xb_, &mut ctx);
                let ts = t.elapsed().as_secs_f64() * 1e6;
                let t = Instant::now();
                let r = xcompare(xq, xc, &cfg, &xb_, &XOptions { policy: Some(POLICY_SAFE), audit: false, scope: paph::x::compare::Scope::Copy }, &mut ctx);
                let tc = t.elapsed().as_secs_f64() * 1e6;
                rows.push((ts, tc, format!("{:<16} kp {:>3} screen {:?}/{} pools {}/{} → {} {} ({}) rows {} ham {}", works[j].name, xc.p.kp.len(), sc.state, sc.reason, sc.direct.count, sc.mirror.count, r.verdict, r.execution.name(), r.reason, r.stats.rows, r.stats.hammings)));
            }
            rows.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
            println!("-- the twenty most expensive candidates of query {} in the reference set (screen us, compare us):", works[q].name);
            for (ts, tc, d) in rows.iter().take(20) {
                println!("   {:7.1} {:8.1}  {}", ts, tc, d);
            }
            let mut by_reason: std::collections::BTreeMap<String, (usize, f64)> = std::collections::BTreeMap::new();
            for (_, tc, d) in rows.iter() {
                let key = d.split("→ ").nth(1).unwrap_or("?").split(" rows").next().unwrap_or("?").to_string();
                let e = by_reason.entry(key).or_insert((0, 0.0));
                e.0 += 1;
                e.1 += tc;
            }
            for (k, (n, t)) in by_reason.iter() {
                println!("   {:>3} x {:<44} {:9.1} us total, {:7.1} us each", n, k, t, t / *n as f64);
            }
            let tot: f64 = rows.iter().map(|r| r.1).sum();
            let scr: f64 = rows.iter().map(|r| r.0).sum();
            println!("   total compare {:.1} us, total screen {:.1} us", tot, scr);
        }
    }
    // route-only screening of 10,000 candidates
    {
        let n = 10_000usize;
        let routes: Vec<paph::x::XRoute> = (0..n).map(|i| sides[i % sides.len()].1.route).collect();
        let soa = RouteSoA::from_routes(&routes);
        let mut scores = vec![RouteScore::default(); n];
        let q = &sides[bases[0]].1.route;
        let mut t = Timed::new();
        for _ in 0..20 {
            let s = Instant::now();
            route_batch(q, &soa, &mut scores);
            t.add(s);
        }
        let rej = scores.iter().filter(|s| route_class(s, &xp) == RouteClass::Reject).count();
        println!("route-only screen of 10,000 candidates: {:.1} us  ({:.2} ns per candidate; {} route-rejected)", t.p50(), t.p50() * 1000.0 / n as f64, rej);
        let _ = route_score;
    }

    // projection table from the corpus' descriptors (§8.2)
    if args.iter().any(|a| a == "--table") {
        use paph::geom42::Desc4;
        use paph::x::bucket::desc_bit;
        // unrelated art: every base and same-style work's descriptors
        let mut sample: Vec<Desc4> = Vec::new();
        for (i, w) in works.iter().enumerate() {
            if w.class == "base" || w.class == "same-style" {
                sample.extend(sides[i].0.desc.iter().copied());
            }
        }
        // transform pairs: the exhaustive 4.2 correspondences of every
        // positive pair give the per-bit flip rate under the transforms
        let mut flips = [0u64; 256];
        let mut matched = 0u64;
        for &(i, j) in pairs.iter() {
            if works[i].family != works[j].family {
                continue;
            }
            let (pa, pb) = (&sides[i].0, &sides[j].0);
            for (ad, hyp) in [(&pa.desc, false), (&pa.desc_m, true)] {
                let st = paph::prepared::match_state(ad, &pb.desc);
                let corr = paph::prepared::corr_42(&st, &pa.kp, &pb.kp, &base);
                for c in corr.iter() {
                    let (x, y) = (&ad[c.a], &pb.desc[c.b]);
                    for b in 0..=255u8 {
                        if desc_bit(x, b) != desc_bit(y, b) {
                            // under the mirror hypothesis bit b of the mirrored
                            // descriptor is bit b^128 of the stored one
                            let bb = if hyp { b ^ 128 } else { b };
                            flips[bb as usize] += 1;
                        }
                    }
                    matched += 1;
                }
            }
        }
        let flip: Vec<f64> = flips.iter().map(|&f| f as f64 / matched.max(1) as f64).collect();
        let n = sample.len();
        println!("\n== projection table: {} descriptors of unrelated art, {} matched pairs under transforms", n, matched);
        let mut ones = vec![0u32; 256];
        for d in sample.iter() {
            for b in 0..=255u8 {
                ones[b as usize] += desc_bit(d, b) as u32;
            }
        }
        // greedy: twelve projections of eight bits; each bit chosen to
        // minimise the expected collision rate of the code so far, weighted
        // by its flip rate under transforms; mirror partners excluded, every
        // bit used once
        let mut used = [false; 256];
        let mut table = [0u8; 12 * 12];
        let mut codes = vec![0u16; n];
        let coll = |codes: &[u16], bits: usize| -> f64 {
            let mut hist = vec![0u32; 1 << bits];
            for &c in codes.iter() {
                hist[c as usize] += 1;
            }
            hist.iter().map(|&h| (h as f64 / n as f64).powi(2)).sum::<f64>()
        };
        for p in 0..12 {
            for v in codes.iter_mut() {
                *v = 0;
            }
            for k in 0..12 {
                let mut best = (f64::MAX, 0u8);
                for b in 0..256usize {
                    // a bit once per table where possible, never twice in a
                    // projection, never beside its mirror partner
                    let row = &table[p * 12..p * 12 + k];
                    if used[b] || row.iter().any(|&x| x as usize == b || x as usize == (b + 128) & 255) {
                        continue;
                    }
                    let bal = ones[b] as f64 / n as f64;
                    if !(0.2..=0.8).contains(&bal) {
                        continue;
                    }
                    let mut trial = codes.clone();
                    for (c, d) in trial.iter_mut().zip(sample.iter()) {
                        *c |= (desc_bit(d, b as u8) as u16) << k;
                    }
                    // a collision is as bad as a flip is: both cost one
                    // probe its chance of finding the twin
                    let score = coll(&trial, k + 1) * (1.0 + 4.0 * flip[b]);
                    if score < best.0 {
                        best = (score, b as u8);
                    }
                }
                if best.0 == f64::MAX {
                    // every balanced bit is taken: allow reuse across projections
                    for b in 0..256usize {
                        let row = &table[p * 12..p * 12 + k];
                        if row.iter().any(|&x| x as usize == b || x as usize == (b + 128) & 255) {
                            continue;
                        }
                        let mut trial = codes.clone();
                        for (c, d) in trial.iter_mut().zip(sample.iter()) {
                            *c |= (desc_bit(d, b as u8) as u16) << k;
                        }
                        let score = coll(&trial, k + 1) * (1.0 + 4.0 * flip[b]);
                        if score < best.0 {
                            best = (score, b as u8);
                        }
                    }
                }
                let b = best.1;
                used[b as usize] = true;
                table[p * 12 + k] = b;
                for (c, d) in codes.iter_mut().zip(sample.iter()) {
                    *c |= (desc_bit(d, b) as u16) << k;
                }
            }
            println!("projection {:2}: bits {:?}  collision rate {:.6} (uniform {:.6})  mean flip {:.3}", p, &table[p * 12..p * 12 + 12], coll(&codes, 12), 1.0 / 4096.0, (0..12).map(|k| flip[table[p * 12 + k] as usize]).sum::<f64>() / 12.0);
        }
        // the seeded table, for comparison
        let seeded = paph::x::profile::default_table(xp.lsh_seed);
        let mut tot = 0.0;
        for p in 0..12 {
            for v in codes.iter_mut() {
                *v = 0;
            }
            for k in 0..12 {
                for (c, d) in codes.iter_mut().zip(sample.iter()) {
                    *c |= (desc_bit(d, seeded[p * 12 + k]) as u16) << k;
                }
            }
            tot += coll(&codes, 12);
        }
        println!("seeded table mean collision rate {:.6}", tot / 12.0);
        println!("pub const TABLE_X1: [u8; LSH_BASE * LSH_BITS] = [");
        for p in 0..12 {
            println!("    {},", table[p * 12..p * 12 + 12].iter().map(|b| b.to_string()).collect::<Vec<_>>().join(", "));
        }
        println!("];");
    }

    // calibration
    if calibrate {
        println!("\n== calibration of the route bars from this corpus");
        let pos: Vec<&PairRec> = recs.iter().filter(|r| is_copy(r.v42)).collect();
        let neg: Vec<&PairRec> = recs.iter().filter(|r| !is_copy(r.v42) && !r.positive).collect();
        println!("positives (42 Copy/Identical): {}   negatives: {}", pos.len(), neg.len());
        let m = |r: &PairRec, f: u16| r.route.measurable & f != 0;
        use paph::x::route::{RF_BAND, RF_GLOBAL, RF_LOCAL};
        // distributions
        let mut pl: Vec<i32> = pos.iter().filter(|r| m(r, RF_LOCAL)).map(|r| r.route.local).collect();
        let mut pb: Vec<i32> = pos.iter().filter(|r| m(r, RF_BAND)).map(|r| r.route.band).collect();
        let mut pg: Vec<i32> = pos.iter().filter(|r| m(r, RF_GLOBAL)).map(|r| r.route.global).collect();
        let mut nl: Vec<i32> = neg.iter().filter(|r| m(r, RF_LOCAL)).map(|r| r.route.local).collect();
        let mut nb_: Vec<i32> = neg.iter().filter(|r| m(r, RF_BAND)).map(|r| r.route.band).collect();
        let mut ng: Vec<i32> = neg.iter().filter(|r| m(r, RF_GLOBAL)).map(|r| r.route.global).collect();
        for v in [&mut pl, &mut pb, &mut pg, &mut nl, &mut nb_, &mut ng] {
            v.sort();
        }
        let q = |v: &Vec<i32>, p: f64| -> i32 { if v.is_empty() { 0 } else { v[((v.len() - 1) as f64 * p) as usize] } };
        println!("local  pos min/p5/p50 {}/{}/{}   neg p50/p95/max {}/{}/{}", q(&pl, 0.0), q(&pl, 0.05), q(&pl, 0.5), q(&nl, 0.5), q(&nl, 0.95), q(&nl, 1.0));
        println!("band   pos min/p5/p50 {}/{}/{}   neg p50/p95/max {}/{}/{}", q(&pb, 0.0), q(&pb, 0.05), q(&pb, 0.5), q(&nb_, 0.5), q(&nb_, 0.95), q(&nb_, 1.0));
        println!("global pos min/p5/p50 {}/{}/{}   neg p50/p95/max {}/{}/{}", q(&pg, 0.0), q(&pg, 0.05), q(&pg, 0.5), q(&ng, 0.5), q(&ng, 0.95), q(&ng, 1.0));
        let mut pp: Vec<i32> = pos.iter().map(|r| r.pool as i32).collect();
        let mut np: Vec<i32> = neg.iter().map(|r| r.pool as i32).collect();
        pp.sort();
        np.sort();
        println!("anchor pool pos min/p5/p25/p50 {}/{}/{}/{}   neg p50/p75/p90/p95/max {}/{}/{}/{}/{}", q(&pp, 0.0), q(&pp, 0.05), q(&pp, 0.25), q(&pp, 0.5), q(&np, 0.5), q(&np, 0.75), q(&np, 0.9), q(&np, 0.95), q(&np, 1.0));
        let low_route = |r: &PairRec| (!m(r, RF_LOCAL) || r.route.local < 6) && (!m(r, RF_BAND) || r.route.band < 3) && (!m(r, RF_GLOBAL) || r.route.global < 190);
        let mut npl: Vec<i32> = neg.iter().filter(|r| low_route(r)).map(|r| r.pool as i32).collect();
        npl.sort();
        println!("negatives with a low route: {} — their anchor pools p50/p75/p90/max {}/{}/{}/{}", npl.len(), q(&npl, 0.5), q(&npl, 0.75), q(&npl, 0.9), q(&npl, 1.0));
        // the lower bars: maximise negatives rejected subject to rejecting no
        // positive (a pair is rejected when every measurable family is
        // below its bar AND the anchor pools are at most defer_pool_max)
        let rejects = |r: &PairRec, l: i32, b: i32, g: i32| -> bool {
            let low_l = !m(r, RF_LOCAL) || r.route.local < l;
            let low_b = !m(r, RF_BAND) || r.route.band < b;
            let low_g = !m(r, RF_GLOBAL) || r.route.global < g;
            r.route.measurable != 0 && low_l && low_b && low_g && r.pool as i32 <= xp.defer_pool_max
        };
        let mut best = (0usize, 0, 0, 0);
        for l in 0..=16 {
            for b in 0..=10 {
                for g in (40..=220).step_by(5) {
                    if pos.iter().any(|r| rejects(r, l, b, g)) {
                        continue;
                    }
                    let n = neg.iter().filter(|r| rejects(r, l, b, g)).count();
                    if n > best.0 {
                        best = (n, l, b, g);
                    }
                }
            }
        }
        // one lane of safety margin on the MinHash families where the data
        // allows it: the bar is lowered by one while it still rejects
        let (nrej, mut l, mut b, mut g) = best;
        println!("lower bars with zero positive rejections: local {l} band {b} global {g}  → {nrej} of {} negatives rejected ({:.1}%)", neg.len(), 100.0 * nrej as f64 / neg.len().max(1) as f64);
        if l > 1 {
            l -= 1;
        }
        if b > 1 {
            b -= 1;
        }
        g = (g - 10).max(0);
        let n2 = neg.iter().filter(|r| rejects(r, l, b, g)).count();
        println!("with margin: local {l} band {b} global {g}  → {n2} negatives rejected ({:.1}%)", 100.0 * n2 as f64 / neg.len().max(1) as f64);
        // the fast bars: above every negative's reading
        let fl = q(&nl, 1.0) + 2;
        let fb = q(&nb_, 1.0) + 2;
        let fg = (q(&ng, 1.0) + 8).min(256);
        println!("fast bars (above every negative): local {fl} band {fb} global {fg}");
        let mut out = xp.clone();
        out.t_local_low = l;
        out.t_band_low = b;
        out.t_global_low = g;
        out.t_local_fast = fl.min(64);
        out.t_band_fast = fb.min(32);
        out.t_global_fast = fg;
        if let Err(e) = out.validate() {
            println!("calibrated profile does not validate: {e}");
        } else if let Some(p) = prof_out {
            std::fs::write(&p, out.encode()).expect("write profile");
            println!("wrote {} ({})", p, out.id_hex16());
        }
        println!("Rust defaults to paste into XProfile::x1_for:\n    t_local_fast: {}, t_band_fast: {}, t_global_fast: {},\n    t_local_low: {}, t_band_low: {}, t_global_low: {},", out.t_local_fast, out.t_band_fast, out.t_global_fast, out.t_local_low, out.t_band_low, out.t_global_low);
    }
}
