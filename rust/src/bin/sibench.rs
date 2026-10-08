//! PAPH-SI benchmark and fitting harness (SPEC-SI §9).
//!
//!     sibench corpus [--bases N] [--distractors N] [--from K] [--out F]   hash a corpus, cache the wires
//!     sibench fit [--out PATH]                     fit SI1 on the fit split, write the profile
//!     sibench eval [--profile PATH] [--big F] [--e2e N]
//!                                                   stability matrix, the proposal's designs against
//!                                                   SI, the funnel with the key index and XRank,
//!                                                   scaling to the big distractor file, timings
//!
//! The corpus is built from the engine's own generators (`synth.rs`): bases
//! under twenty transforms, and same-style distractors drawn from the same
//! generators with other seeds — the hardest negatives this corpus has.
//! Splits: bases in alternate blocks of eight (one of each generator kind per
//! block) and the first half of the distractors fit; the other blocks and the
//! second half evaluate.  Recall is measured on the pairs comparator
//! 42 itself calls Copy (or Identical) — a nominator need not find what the
//! verifier cannot confirm — and in both arrival orders.
//!
//! Nothing printed here is a claim until it is printed here.

use paph::abi::state_code;
use paph::calibration::Profile;
use paph::compare::Reading;
use paph::config::Config;
use paph::keypoints::{pattern, RotCache};
use paph::prepared::{canon_swapped, PairCtx, Prepared};
use paph::synth::*;
use paph::v42::compare_in;
use paph::wire::hash;
use paph::x::rank::{xrank, RankScratch, XRankOptions, XRANK_FIELDS};
use paph::x::route::XRoute;
use paph::x::si::code::{SiQuery, SiSig};
use paph::x::si::features::{families, FAMILIES, FAMILY_NAMES};
use paph::x::si::fit::{fit, FitInput, FitOptions};
use paph::x::si::index::SiIndex;
use paph::x::si::profile::*;
use paph::x::{XBound, XCtx, XPrepared, XProfile};
use std::collections::HashMap;
use std::io::Write;
use std::time::Instant;

// ------------------------------------------------------------------ corpus

pub const TRANSFORMS: [&str; 21] = [
    "base", "mirror", "rot90", "rot180", "transpose", "invert", "recolour", "chswap", "palshuffle", "up2", "up3",
    "down70", "resample90", "up150", "crop80", "crop67", "corner50", "paste", "shift1", "dither", "matte",
];

fn texture_wall(w: usize, h: usize, seed: u64) -> Img {
    let tile = pixel_art(12, 12, seed, 5, 2);
    let mut o = Img::new(w, h);
    for y in 0..h {
        for x in 0..w {
            o.set(x as i64, y as i64, tile.get(x % 12, y % 12));
        }
    }
    o
}

fn dither_change(a: &Img) -> Img {
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

/// red and blue exchanged — a recolour that does not keep luminance order
fn chswap(a: &Img) -> Img {
    let mut o = a.clone();
    for i in 0..a.w * a.h {
        o.px.swap(i * 4, i * 4 + 2);
    }
    o
}

/// every distinct colour sent to another, bijectively: a palette swap that
/// keeps nothing of the colours but the partition of the pixels
fn palshuffle(a: &Img, seed: u64) -> Img {
    let mut cols: Vec<u32> = (0..a.w * a.h)
        .filter(|&i| a.px[i * 4 + 3] >= 128)
        .map(|i| u32::from_le_bytes([a.px[i * 4], a.px[i * 4 + 1], a.px[i * 4 + 2], 0]))
        .collect();
    cols.sort_unstable();
    cols.dedup();
    let mut r = Rng(seed | 1);
    let mut perm = cols.clone();
    for i in (1..perm.len()).rev() {
        let j = r.below(i as u64 + 1) as usize;
        perm.swap(i, j);
    }
    let mut o = a.clone();
    for i in 0..a.w * a.h {
        if a.px[i * 4 + 3] < 128 {
            continue;
        }
        let c = u32::from_le_bytes([a.px[i * 4], a.px[i * 4 + 1], a.px[i * 4 + 2], 0]);
        let k = cols.binary_search(&c).unwrap();
        let d = perm[k].to_le_bytes();
        o.px[i * 4..i * 4 + 3].copy_from_slice(&d[..3]);
    }
    o
}

/// nearest-neighbour rescale by 3/2 — a non-integer blow-up the hasher
/// cannot divide out
fn up150(a: &Img) -> Img {
    let (w, h) = (a.w * 3 / 2, a.h * 3 / 2);
    let mut o = Img::new(w, h);
    for y in 0..h {
        for x in 0..w {
            o.set(x as i64, y as i64, a.get((x * 2 / 3).min(a.w - 1), (y * 2 / 3).min(a.h - 1)));
        }
    }
    o
}

pub fn transform(b: &Img, t: usize, seed: u64) -> Img {
    match TRANSFORMS[t] {
        "base" => b.clone(),
        "mirror" => mirror(b),
        "rot90" => rot90(b),
        "rot180" => rot90(&rot90(b)),
        "transpose" => transpose(b),
        "invert" => invert(b),
        "recolour" => recolour(b),
        "chswap" => chswap(b),
        "palshuffle" => palshuffle(b, seed),
        "up2" => nearest_up(b, 2),
        "up3" => nearest_up(b, 3),
        "down70" => area_down(b, (b.w * 7 / 10).max(8), (b.h * 7 / 10).max(8)),
        "resample90" => area_down(b, (b.w * 9 / 10).max(8), (b.h * 9 / 10).max(8)),
        "up150" => up150(b),
        "crop80" => crop(b, b.w / 10, b.h / 10, (b.w * 4 / 5).max(8), (b.h * 4 / 5).max(8)),
        "crop67" => crop(b, b.w / 6, b.h / 7, (b.w * 2 / 3).max(8), (b.h * 2 / 3).max(8)),
        "corner50" => crop(b, b.w / 2, b.h / 2, (b.w / 2).max(8), (b.h / 2).max(8)),
        "paste" => {
            let host = pixel_art(b.w * 2 + 20, b.h * 2 + 16, 997 + seed, 9, 2);
            paste(b, &host, b.w / 2 + 3, b.h / 3 + 5)
        }
        "shift1" => shift1(b),
        "dither" => dither_change(b),
        "matte" => matte_variant(b),
        _ => unreachable!(),
    }
}

const SIZES: [(usize, usize); 12] = [
    (160, 120), (96, 96), (240, 180), (300, 220), (120, 90), (400, 90),
    (200, 150), (64, 64), (48, 48), (320, 240), (256, 256), (128, 96),
];

pub fn base_image(i: usize) -> Img {
    let (w, h) = SIZES[i % SIZES.len()];
    let seed = 11 + 97 * i as u64;
    match i % 8 {
        5 => work(w, h, 7 + i as i64, i % 2 == 0),
        6 => pixel_art(32 + (i * 7) % 64, 32 + (i * 11) % 64, seed, 3 + i % 9, 0),
        7 => pixel_art(w.max(200), h.max(150), seed, 12 + i % 12, 2),
        _ => pixel_art(w, h, seed, 3 + (i * 3) % 18, (i % 3) as u8),
    }
}

/// Which generator drew distractor `k` (the first draws of `distractor`).
pub fn distractor_kind(k: usize) -> &'static str {
    let mut r = Rng(0x5349_5f64_6973_7472 ^ (k as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    let _ = r.below(360);
    let _ = r.below(260);
    match r.below(100) {
        0..=9 => "work",
        10..=11 => "texture",
        12 => "random",
        _ => "pixel-art",
    }
}

/// The backdrop a pixel-art distractor was drawn on (0 transparent, 1 flat
/// matte, 2 dithered gradient), or −1 for the other generators.
pub fn distractor_bg(k: usize) -> i32 {
    let mut r = Rng(0x5349_5f64_6973_7472 ^ (k as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    let _ = r.below(360);
    let _ = r.below(260);
    if r.below(100) < 13 {
        return -1;
    }
    let _ = r.next();
    let _ = r.below(23);
    r.below(3) as i32
}

/// The backdrop of base `i` (−1 for the noise-field generator).
pub fn base_bg(i: usize) -> i32 {
    match i % 8 {
        5 => -1,
        6 => 0,
        7 => 2,
        _ => (i % 3) as i32,
    }
}

pub fn base_kind(i: usize) -> &'static str {
    match i % 8 {
        5 => "work",
        6 => "sprite",
        7 => "scene",
        _ => "pixel-art",
    }
}

pub fn distractor(k: usize) -> Img {
    let mut r = Rng(0x5349_5f64_6973_7472 ^ (k as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    let w = 40 + r.below(360) as usize;
    let h = 40 + r.below(260) as usize;
    match r.below(100) {
        0..=9 => work(w, h, 100_000 + k as i64, r.below(2) == 0),
        10..=11 => texture_wall(w.max(48), h.max(48), 200_000 + k as u64),
        12 => {
            let mut im = Img::new(w.min(160), h.min(160));
            for i in 0..im.w * im.h {
                let x = r.next();
                im.px[i * 4..i * 4 + 4].copy_from_slice(&[x as u8, (x >> 8) as u8, (x >> 16) as u8, 255]);
            }
            im
        }
        _ => pixel_art(w, h, 300_000 + r.next() % 1_000_000, 2 + r.below(23) as usize, r.below(3) as u8),
    }
}

/// One cached work: which base it derives from (−1: a distractor), which
/// transform, and its wires.
pub struct Rec {
    pub base: i32,
    pub tf: i32,
    pub t1: Vec<u8>,
    pub t2: Vec<u8>,
}

fn hash_all(jobs: Vec<(i32, i32, Box<dyn Fn() -> Img + Send + Sync>)>) -> Vec<Rec> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(1);
    let n = jobs.len();
    let jobs = std::sync::Arc::new(jobs);
    let next = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let out = std::sync::Arc::new(std::sync::Mutex::new((0..n).map(|_| None).collect::<Vec<Option<Rec>>>()));
    std::thread::scope(|s| {
        for _ in 0..threads {
            let (jobs, next, out) = (jobs.clone(), next.clone(), out.clone());
            s.spawn(move || {
                let cfg = Config::default();
                let rot = RotCache::new(&pattern());
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let (b, t, f) = &jobs[i];
                    let im = f();
                    let fp = hash(&im.px, im.w, im.h, &cfg, &rot);
                    out.lock().unwrap()[i] = Some(Rec { base: *b, tf: *t, t1: fp.t1, t2: fp.t2 });
                }
            });
        }
    });
    let v = std::sync::Arc::try_unwrap(out).ok().unwrap().into_inner().unwrap();
    v.into_iter().map(|r| r.unwrap()).collect()
}

fn save(path: &str, recs: &[Rec]) {
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(b"SIC1");
    b.extend_from_slice(&(recs.len() as u32).to_le_bytes());
    for r in recs {
        b.extend_from_slice(&r.base.to_le_bytes());
        b.extend_from_slice(&r.tf.to_le_bytes());
        b.extend_from_slice(&(r.t2.len() as u32).to_le_bytes());
        b.extend_from_slice(&r.t1);
        b.extend_from_slice(&r.t2);
    }
    std::fs::write(path, b).expect("write corpus");
}

pub fn load(path: &str) -> Vec<Rec> {
    let b = std::fs::read(path).expect("read corpus (run `sibench corpus` first)");
    assert_eq!(&b[0..4], b"SIC1");
    let n = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
    let mut o = 8;
    let mut v = Vec::with_capacity(n);
    let rd = |o: usize| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    for _ in 0..n {
        let base = rd(o);
        let tf = rd(o + 4);
        let l2 = rd(o + 8) as usize;
        o += 12;
        let t1 = b[o..o + 3952].to_vec();
        o += 3952;
        let t2 = b[o..o + l2].to_vec();
        o += l2;
        v.push(Rec { base, tf, t1, t2 });
    }
    v
}

/// `Engine.indexKeys` (KEYS_VERSION 1, wasm/paph.js): the Tier-1 local codes
/// folded to 53 bits, and the 24-bit bands of the 64 strongest keypoints
/// (stored) or of every keypoint and its mirrored descriptor (query).
pub fn index_keys(p: &Prepared, query: bool) -> (Vec<u64>, Vec<u64>) {
    let mut codes: Vec<u64> = (0..p.bag.len()).map(|i| {
        let (hi, lo) = p.bag.code(i);
        ((hi as u64 & 0x1f_ffff) << 32) | lo as u64
    }).collect();
    codes.sort_unstable();
    codes.dedup();
    let mut ord: Vec<usize> = (0..p.kp.len()).collect();
    ord.sort_by(|&a, &b| p.kp[b].s.cmp(&p.kp[a].s).then(p.kp[a].x.cmp(&p.kp[b].x)).then(p.kp[a].y.cmp(&p.kp[b].y)));
    let nk = ord.len().min(if query { 512 } else { 64 });
    let mut bands: Vec<u64> = Vec::new();
    for &k in ord[..nk].iter() {
        let mut bytes = [0u8; 32];
        for j in 0..8 {
            bytes[4 * j..4 * j + 4].copy_from_slice(&p.kp[k].desc[j].to_le_bytes());
        }
        let mut mir = [0u8; 32];
        mir[..16].copy_from_slice(&bytes[16..]);
        mir[16..].copy_from_slice(&bytes[..16]);
        let both: &[[u8; 32]] = if query { &[bytes, mir] } else { &[bytes, bytes] };
        for b in both.iter().take(if query { 2 } else { 1 }) {
            for j in 0..10 {
                let v = ((b[3 * j] as u64) << 16) | ((b[3 * j + 1] as u64) << 8) | b[3 * j + 2] as u64;
                if v != 0 && v != 0xff_ffff {
                    bands.push(((j as u64) << 24) | v);
                }
            }
        }
    }
    bands.sort_unstable();
    bands.dedup();
    (codes, bands)
}

fn arg(args: &[String], k: &str, d: usize) -> usize {
    args.iter().position(|a| a == k).map(|i| args[i + 1].parse().unwrap()).unwrap_or(d)
}

fn sarg(args: &[String], k: &str, d: &str) -> String {
    args.iter().position(|a| a == k).map(|i| args[i + 1].clone()).unwrap_or_else(|| d.to_string())
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[(((v.len() - 1) as f64) * p).round() as usize]
}

// ---------------------------------------------------------------- the splits

struct Corpus {
    recs: Vec<Rec>,
    sides: Vec<Prepared>,
    fam: Vec<[Option<Vec<i32>>; FAMILIES]>,
    routes: Vec<XRoute>,
    at: HashMap<(i32, i32), usize>,
    nb: i32,
    fit_d: Vec<usize>,
    ev_d: Vec<usize>,
    /// comparator-42 state of (base, transform) against its base
    verdict: HashMap<(i32, i32), i32>,
}

impl Corpus {
    fn open(dir: &str, xb: &XBound) -> Corpus {
        let t0 = Instant::now();
        let recs = load(&format!("{dir}/corpus.bin"));
        let sides: Vec<Prepared> = recs.iter().map(|r| Prepared::new(&r.t1, Some(&r.t2)).unwrap()).collect();
        let fam: Vec<_> = sides.iter().map(families).collect();
        let routes: Vec<XRoute> = sides.iter().map(|p| XRoute::build(p, &xb.salts, &xb.xp)).collect();
        let mut at = HashMap::new();
        for (i, r) in recs.iter().enumerate() {
            if r.base >= 0 {
                at.insert((r.base, r.tf), i);
            }
        }
        let nb = recs.iter().map(|r| r.base).max().unwrap_or(-1) + 1;
        let dist: Vec<usize> = (0..recs.len()).filter(|&i| recs[i].base < 0).collect();
        let (fit_d, ev_d) = (dist[..dist.len() / 2].to_vec(), dist[dist.len() / 2..].to_vec());
        // comparator 42 on every base × variant pair: what the verifier can confirm
        let base = Profile::cal004();
        let bcfg = paph::v4::bind(&Config::default(), &base);
        let mut verdict = HashMap::new();
        for b in 0..nb {
            let i = at[&(b, 0)];
            for t in 1..TRANSFORMS.len() as i32 {
                let j = at[&(b, t)];
                let (pa, pb) = (&sides[i], &sides[j]);
                let swapped = canon_swapped(pa, pb);
                let (ca, cb) = if swapped { (pb, pa) } else { (pa, pb) };
                let mut pc = PairCtx::new(ca, cb);
                verdict.insert((b, t), state_code(compare_in(&mut pc, swapped, &bcfg, &base, Reading::Lean).base.verdict));
            }
        }
        eprintln!("corpus: {} works ({} bases × {} variants, {} distractors) opened and judged in {:.1} s", recs.len(), nb, TRANSFORMS.len(), dist.len(), t0.elapsed().as_secs_f64());
        Corpus { recs, sides, fam, routes, at, nb, fit_d, ev_d, verdict }
    }

    fn copy(&self, b: i32, t: i32) -> bool {
        matches!(self.verdict.get(&(b, t)), Some(3) | Some(4))
    }

    /// (transform, query, target) — both arrival orders
    fn pairs(&self, eval: bool, keep: impl Fn(i32, i32) -> bool) -> Vec<(usize, usize, usize)> {
        let mut v = Vec::new();
        for b in 0..self.nb {
            if is_eval_base(b) != eval {
                continue;
            }
            let i = self.at[&(b, 0)];
            for t in 1..TRANSFORMS.len() as i32 {
                if !keep(b, t) {
                    continue;
                }
                let j = self.at[&(b, t)];
                v.push((t as usize, j, i));
                v.push((t as usize, i, j));
            }
        }
        v
    }
}

/// The split: bases come in blocks of eight, one of each generator kind
/// (`base_image` switches on i % 8), and alternate blocks fit and evaluate,
/// so every kind is on both sides of the split.
fn is_eval_base(b: i32) -> bool {
    (b / 8) % 2 == 1
}

// ------------------------------------------------------------- the key index

/// The exact-key nomination of docs/SEARCH.md on sorted arrays: Σ 1/df over
/// shared keys, keys above the df cap skipped.
struct KeyIndex {
    keys: Vec<u64>,
    slots: Vec<u32>,
    n: usize,
}

impl KeyIndex {
    fn build(sets: &[&Vec<u64>]) -> KeyIndex {
        let mut v: Vec<(u64, u32)> = Vec::new();
        for (s, set) in sets.iter().enumerate() {
            for &k in set.iter() {
                v.push((k, s as u32));
            }
        }
        v.sort_unstable();
        KeyIndex { keys: v.iter().map(|x| x.0).collect(), slots: v.iter().map(|x| x.1).collect(), n: sets.len() }
    }

    fn range(&self, k: u64) -> (usize, usize) {
        (self.keys.partition_point(|&x| x < k), self.keys.partition_point(|&x| x <= k))
    }

    /// Scores every indexed slot into `acc` and returns the target's score
    /// (`target` sorted; df counts the target where it holds the key).
    fn score(&self, q: &[u64], target: &[u64], cap: usize, acc: &mut Vec<f64>, touched: &mut Vec<u32>) -> f64 {
        for &t in touched.iter() {
            acc[t as usize] = 0.0;
        }
        touched.clear();
        acc.resize(self.n, 0.0);
        let mut st = 0.0f64;
        for &k in q.iter() {
            let (lo, hi) = self.range(k);
            let in_t = target.binary_search(&k).is_ok();
            let df = hi - lo + in_t as usize;
            if df == 0 || df > cap {
                continue;
            }
            let w = 1.0 / df as f64;
            if in_t {
                st += w;
            }
            for &s in self.slots[lo..hi].iter() {
                if acc[s as usize] == 0.0 {
                    touched.push(s);
                }
                acc[s as usize] += w;
            }
        }
        st
    }
}

/// Rank of a target with score `st` among the scored slots (1 = best); MAX when it shares nothing.
fn rank_of(st: f64, acc: &[f64], touched: &[u32]) -> usize {
    if st <= 0.0 {
        return usize::MAX;
    }
    1 + touched.iter().filter(|&&s| acc[s as usize] >= st).count()
}

/// The top `k` slots by score (score descending, slot ascending).
fn top_k(acc: &[f64], touched: &[u32], k: usize) -> Vec<u32> {
    let mut v: Vec<u32> = touched.to_vec();
    v.sort_unstable_by(|&a, &b| acc[b as usize].partial_cmp(&acc[a as usize]).unwrap().then(a.cmp(&b)));
    v.truncate(k);
    v
}

const KEY_K: usize = 32;
const DF_CAP: usize = 256;

fn groups() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("symmetries", vec!["mirror", "rot90", "rot180", "transpose"]),
        ("integer rescale", vec!["up2", "up3"]),
        ("recolour", vec!["invert", "recolour", "chswap", "palshuffle"]),
        ("resample", vec!["down70", "resample90", "up150"]),
        ("crop", vec!["crop80", "crop67", "corner50"]),
        ("paste", vec!["paste"]),
        ("edits", vec!["shift1", "dither", "matte"]),
    ]
}

fn fam_name(f: usize) -> &'static str {
    if f < FAMILIES {
        FAMILY_NAMES[f]
    } else if f == FAMILY_LOCAL {
        "local"
    } else {
        "band"
    }
}

// ------------------------------------------------------------------- fit

fn cmd_fit(c: &Corpus, out: &str) {
    let mh: Vec<SiSig> = c.routes.iter().map(SiSig::minhash).collect();
    let paste = TRANSFORMS.iter().position(|&t| t == "paste").unwrap() as i32;
    let noise: Vec<(usize, usize)> = c.pairs(false, |_, t| t != paste).iter().map(|p| (p.1, p.2)).collect();
    let evidence: Vec<(usize, usize)> = c
        .pairs(false, |b, t| t != paste && matches!(c.verdict.get(&(b, t)), Some(2) | Some(3) | Some(4)))
        .iter()
        .map(|p| (p.1, p.2))
        .collect();
    let mut name = [0u8; 16];
    name[..15].copy_from_slice(b"SI1-PROVISIONAL");
    let o = FitOptions {
        name,
        xid: XProfile::x1().id(),
        probes: 4,
        random_per_query: 400,
        seed: 0x5349_3150_524f_5631,
        admit_ppm: 10_000,
        budget: 2000,
    };
    let t0 = Instant::now();
    let inp = FitInput { fam: &c.fam, mh: &mh, background: &c.fit_d, noise_pairs: &noise, evidence_pairs: &evidence };
    let prof = fit(&inp, &o);
    let b = prof.encode();
    std::fs::write(out, &b).expect("write profile");
    println!("fitted {} on {} background works, {} noise pairs, {} evidence pairs in {:.2} s", prof.name_str(), c.fit_d.len(), noise.len(), evidence.len(), t0.elapsed().as_secs_f64());
    println!("  {} bytes, id {}  → {out}", b.len(), paph::sha256::hex(&prof.id()));
    println!("  probes {}  threshold {}  budget {}", prof.probes, prof.threshold, prof.budget);
    println!("  | family | weights none / near / exact | axis noise σ (projection units) |");
    for f in 0..FAMILIES {
        let cb = &prof.book[f];
        println!("  | {} | {} / {} / {} | {:?} |", FAMILY_NAMES[f], cb.w[0], cb.w[1], cb.w[2], cb.sig);
    }
    println!("  | local | {:?} (0 / 1 / 2–3 / ≥4 equal band keys) | |", prof.w_local);
    println!("  | band | {:?} | |", prof.w_band);
}

// ------------------------------------------------------------------ eval

struct QRec {
    t: usize,
    qi: usize,
    ti: usize,
    tlev: [Option<usize>; ALL_FAMILIES],
    tscore: i32,
    ttouch: bool,
    /// per eval distractor: score (i32::MIN when untouched)
    dscore: Vec<i32>,
    /// per eval distractor: packed levels, 2 bits per family
    dlev: Vec<u16>,
}

fn pack(l: &[Option<usize>; ALL_FAMILIES]) -> u16 {
    let mut v = 0u16;
    for (f, x) in l.iter().enumerate() {
        // 0: absent, 1: level 0, 2: level 1, 3: level ≥ 2
        let c = match x {
            None => 0,
            Some(0) => 1,
            Some(1) => 2,
            Some(_) => 3,
        };
        v |= c << (2 * f);
    }
    v
}

fn lev(v: u16, f: usize) -> Option<usize> {
    match (v >> (2 * f)) & 3 {
        0 => None,
        1 => Some(0),
        2 => Some(1),
        _ => Some(2),
    }
}

type Design = (&'static str, Box<dyn Fn(u16) -> bool>);

fn designs() -> Vec<Design> {
    let and = |fs: Vec<usize>, min: usize| -> Box<dyn Fn(u16) -> bool> {
        Box::new(move |v: u16| {
            let mut any = false;
            for &f in fs.iter() {
                match lev(v, f) {
                    None => {}
                    Some(l) if l >= min => any = true,
                    Some(_) => return false,
                }
            }
            any
        })
    };
    let votes = |k: usize| -> Box<dyn Fn(u16) -> bool> {
        Box::new(move |v: u16| (0..ALL_FAMILIES).filter(|&f| matches!(lev(v, f), Some(l) if l >= 1)).count() >= k)
    };
    let (shape, pal, runs) = (3usize, 2usize, 0usize);
    vec![
        ("proposal: shape ∧ pal ∧ runs, own cell only", and(vec![shape, pal, runs], 2)),
        ("proposal: shape ∧ pal ∧ runs, multi-probe", and(vec![shape, pal, runs], 1)),
        ("proposal: all six quantised families, own cell", and(vec![0, 1, 2, 3, 4, 5], 2)),
        ("proposal: all six, multi-probe", and(vec![0, 1, 2, 3, 4, 5], 1)),
        ("votes: ≥ 2 of 8 families agree", votes(2)),
        ("votes: ≥ 3 of 8", votes(3)),
        ("votes: ≥ 4 of 8", votes(4)),
    ]
}

fn cmd_eval(c: &Corpus, prof: &SiProfile, xb: &XBound, args: &[String]) {
    let n = c.recs.len();
    println!("PAPH-SI evaluation — profile {} ({}), X {} — {} works, eval population {} distractors\n", prof.name_str(), prof.id_hex16(), xb.xp.name_str(), n, c.ev_d.len());
    let sigs: Vec<SiSig> = (0..n).map(|i| SiSig::from_parts(&c.fam[i], &c.routes[i], prof)).collect();
    // the cost an ingest pays: families + route + cells from a prepared side
    let t0 = Instant::now();
    let sample = n.min(2000);
    for i in 0..sample {
        let s = SiSig::from_prepared(&c.sides[i * n / sample], xb, prof);
        assert_eq!(s, sigs[i * n / sample]);
    }
    let dt_sig = t0.elapsed().as_secs_f64() / sample as f64;
    let t0 = Instant::now();
    for i in 0..sample {
        let r = &c.recs[i * n / sample];
        let p = Prepared::new(&r.t1, Some(&r.t2)).unwrap();
        let _ = SiSig::from_prepared(&p, xb, prof);
    }
    let dt_wire = t0.elapsed().as_secs_f64() / sample as f64;

    // ---------------------------------------------------------- balance
    println!("## Cells: how the eval population spreads (fine 256 / coarse 16)\n");
    println!("| family | present | occupied fine | effective fine cells | largest fine cell | effective coarse cells | P(random pair shares a fine cell) |");
    println!("|---|---:|---:|---:|---:|---:|---:|");
    for f in 0..FAMILIES {
        let mut h = [0u64; CELLS];
        let mut hc = [0u64; COARSE];
        let mut m = 0u64;
        for &i in c.ev_d.iter() {
            if sigs[i].present >> f & 1 != 0 {
                h[sigs[i].cells[f] as usize] += 1;
                hc[paph::x::si::code::coarse(sigs[i].cells[f]) as usize] += 1;
                m += 1;
            }
        }
        let ent = |h: &[u64]| -> f64 { h.iter().filter(|&&x| x > 0).map(|&x| { let p = x as f64 / m as f64; -p * p.ln() }).sum::<f64>().exp() };
        let coll: f64 = h.iter().map(|&x| (x as f64 / m as f64).powi(2)).sum();
        println!("| {} | {:.0}% | {} | {:.0} | {:.1}% | {:.1} | 1/{:.0} |", FAMILY_NAMES[f], 100.0 * m as f64 / c.ev_d.len() as f64, h.iter().filter(|&&x| x > 0).count(), ent(&h), 100.0 * *h.iter().max().unwrap() as f64 / m as f64, ent(&hc), 1.0 / coll);
    }

    // ---------------------------------------------------------- stability
    println!("\n## Stability: where a copy lands relative to the query's probes (eval bases, every pair, both orders)\n");
    println!("Exact = the copy is in the query's own cell; probed = in one of its {} probe cells. Random = a random eval distractor.\n", prof.probes);
    let all = c.pairs(true, |_, _| true);
    let mut hit = vec![vec![[0u32; 3]; ALL_FAMILIES]; TRANSFORMS.len()];
    let mut rnd = vec![[0u64; 3]; ALL_FAMILIES];
    let mut rng = Rng(0x5354_4142_0001);
    for &(t, qi, ti) in all.iter() {
        let q = SiQuery::from_parts(sigs[qi], &c.fam[qi], prof);
        let l = q.levels(&sigs[ti]);
        for f in 0..ALL_FAMILIES {
            if let Some(v) = l[f] {
                hit[t][f][0] += 1;
                if v >= 1 {
                    hit[t][f][1] += 1;
                }
                if (f < FAMILIES && v >= 2) || (f >= FAMILIES && v >= 1) {
                    hit[t][f][2] += 1;
                }
            }
        }
        if t == 1 {
            for _ in 0..200 {
                let d = c.ev_d[rng.below(c.ev_d.len() as u64) as usize];
                let l = q.levels(&sigs[d]);
                for f in 0..ALL_FAMILIES {
                    if let Some(v) = l[f] {
                        rnd[f][0] += 1;
                        if v >= 1 {
                            rnd[f][1] += 1;
                        }
                        if (f < FAMILIES && v >= 2) || (f >= FAMILIES && v >= 1) {
                            rnd[f][2] += 1;
                        }
                    }
                }
            }
        }
    }
    print!("| transform |");
    for f in 0..ALL_FAMILIES {
        print!(" {} |", fam_name(f));
    }
    println!();
    print!("|---|");
    for _ in 0..ALL_FAMILIES {
        print!("---:|");
    }
    println!();
    let cellfmt = |h: [u64; 3], f: usize| -> String {
        if h[0] == 0 {
            return "—".into();
        }
        if f < FAMILIES {
            format!("{:.0} / {:.0}", 100.0 * h[2] as f64 / h[0] as f64, 100.0 * h[1] as f64 / h[0] as f64)
        } else {
            format!("{:.0}", 100.0 * h[1] as f64 / h[0] as f64)
        }
    };
    for t in 1..TRANSFORMS.len() {
        print!("| {} |", TRANSFORMS[t]);
        for f in 0..ALL_FAMILIES {
            let h = hit[t][f];
            print!(" {} |", cellfmt([h[0] as u64, h[1] as u64, h[2] as u64], f));
        }
        println!();
    }
    print!("| *random pair* |");
    for f in 0..ALL_FAMILIES {
        let h = rnd[f];
        if f < FAMILIES {
            print!(" {:.1} / {:.1} |", 100.0 * h[2] as f64 / h[0].max(1) as f64, 100.0 * h[1] as f64 / h[0].max(1) as f64);
        } else {
            print!(" {:.2} |", 100.0 * h[1] as f64 / h[0].max(1) as f64);
        }
    }
    println!("\n\nQuantised families: % exact / % probed. MinHash families: % with at least one equal band key.");

    // ---------------------------------------------------------- the coarse level
    // what a match in the query's coarse cell, outside its probe cells, is worth
    println!("\n## What a coarse-only match is worth (the note's 16-cell level)\n");
    println!("A candidate outside the query's {} probe cells but inside its coarse cell, against one in neither: share of comparator-42 Copy pairs and of random pairs, and 16 × the log-likelihood ratio.\n", prof.probes);
    println!("| family | copy: probed | copy: coarse only | copy: neither | random: probed | random: coarse only | random: neither | weight of coarse only | weight of neither |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    {
        let copies = c.pairs(true, |b, t| c.copy(b, t));
        let mut cnt = vec![[[0u64; 3]; 2]; FAMILIES];
        let mut rng = Rng(0x434f_4152_5345_0001);
        for &(_, qi, ti) in copies.iter() {
            let q = SiQuery::from_parts(sigs[qi], &c.fam[qi], prof);
            let mut tally = |t: usize, which: usize| {
                for f in 0..FAMILIES {
                    if sigs[qi].present >> f & 1 == 0 || sigs[t].present >> f & 1 == 0 {
                        continue;
                    }
                    let lv = q.lut[f][sigs[t].cells[f] as usize];
                    let k = if lv > 0 {
                        0
                    } else if paph::x::si::code::coarse(sigs[t].cells[f]) == paph::x::si::code::coarse(sigs[qi].cells[f]) {
                        1
                    } else {
                        2
                    };
                    cnt[f][which][k] += 1;
                }
            };
            tally(ti, 0);
            for _ in 0..50 {
                tally(c.ev_d[rng.below(c.ev_d.len() as u64) as usize], 1);
            }
        }
        for f in 0..FAMILIES {
            let (cc, rr) = (cnt[f][0], cnt[f][1]);
            let (tc, tr) = (cc.iter().sum::<u64>().max(1) as f64, rr.iter().sum::<u64>().max(1) as f64);
            let w = |k: usize| 16.0 * (((cc[k] as f64 + 0.5) / tc) / ((rr[k] as f64 + 0.5) / tr)).ln();
            println!("| {} | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:+.0} | {:+.0} |", FAMILY_NAMES[f], 100.0 * cc[0] as f64 / tc, 100.0 * cc[1] as f64 / tc, 100.0 * cc[2] as f64 / tc, 100.0 * rr[0] as f64 / tr, 100.0 * rr[1] as f64 / tr, 100.0 * rr[2] as f64 / tr, w(1), w(2));
        }
    }

    // ---------------------------------------------------------- the grid
    // a family computed on a sampling grid is exactly D4-invariant only when
    // the grid's cells commute with the symmetry: split by canvas size
    println!("\n## Sampling grids and the square's symmetries (every base, both splits)\n");
    println!("Share of D4 copies (mirror, rot90, rot180, transpose) whose reading equals the original's exactly, for bases whose sides are both multiples of 16 and for the others. G0 is XRoute's DCT word; the rest are SI cells.\n");
    println!("| canvas | pairs | G0 word | runs | tone | pal | shape | sil | kpgeo |");
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    {
        let mut agg = [[0u64; 8]; 2];
        let mut npairs = [0u64; 2];
        let mut nfam = [[0u64; FAMILIES]; 2];
        for b in 0..c.nb {
            let i = c.at[&(b, 0)];
            let t1 = &c.sides[i].t1;
            let g = if t1.width % 16 == 0 && t1.height % 16 == 0 { 0 } else { 1 };
            for t in ["mirror", "rot90", "rot180", "transpose"] {
                let j = c.at[&(b, TRANSFORMS.iter().position(|&x| x == t).unwrap() as i32)];
                npairs[g] += 1;
                if c.routes[i].global[0] == c.routes[j].global[0] {
                    agg[g][0] += 1;
                }
                for f in 0..FAMILIES {
                    if sigs[i].present >> f & 1 != 0 && sigs[j].present >> f & 1 != 0 {
                        nfam[g][f] += 1;
                        if sigs[i].cells[f] == sigs[j].cells[f] {
                            agg[g][1 + f] += 1;
                        }
                    }
                }
            }
        }
        for g in 0..2 {
            print!("| {} | {} | {:.0}% |", if g == 0 { "both sides multiples of 16" } else { "other sizes" }, npairs[g], 100.0 * agg[g][0] as f64 / npairs[g].max(1) as f64);
            for f in 0..FAMILIES {
                print!(" {:.0}% |", 100.0 * agg[g][1 + f] as f64 / nfam[g][f].max(1) as f64);
            }
            println!();
        }
    }

    // ------------------------------------------------- per-query readings
    let ev = c.pairs(true, |b, t| c.copy(b, t));
    let t1 = Instant::now();
    let mut recs: Vec<QRec> = Vec::with_capacity(ev.len());
    for &(t, qi, ti) in ev.iter() {
        let q = SiQuery::from_parts(sigs[qi], &c.fam[qi], prof);
        let tl = q.levels(&sigs[ti]);
        let mut dscore = Vec::with_capacity(c.ev_d.len());
        let mut dlev = Vec::with_capacity(c.ev_d.len());
        for &d in c.ev_d.iter() {
            let l = q.levels(&sigs[d]);
            dlev.push(pack(&l));
            dscore.push(if q.touches(&sigs[d]) { q.score(&sigs[d]) } else { i32::MIN });
        }
        recs.push(QRec { t, qi, ti, tlev: tl, tscore: q.score(&sigs[ti]), ttouch: q.touches(&sigs[ti]), dscore, dlev });
    }
    eprintln!("scored {} queries × {} distractors in {:.1} s", recs.len(), c.ev_d.len(), t1.elapsed().as_secs_f64());
    let nd = c.ev_d.len() as f64;
    let mut per_t = vec![0usize; TRANSFORMS.len()];
    for r in recs.iter() {
        per_t[r.t] += 1;
    }
    println!("\n## Recall against reduction (comparator-42 Copy pairs of the eval bases, both orders: {} queries)\n", recs.len());
    print!("Pairs per transform:");
    for t in 1..TRANSFORMS.len() {
        print!(" {} {},", TRANSFORMS[t], per_t[t] / 2);
    }
    println!("\n");
    let gs = groups();
    print!("| design | pool | reduction | recall |");
    for (g, _) in gs.iter() {
        print!(" {g} |");
    }
    println!();
    print!("|---|---:|---:|---:|");
    for _ in gs.iter() {
        print!("---:|");
    }
    println!();
    let row = |name: String, pass_t: &dyn Fn(&QRec) -> bool, pass_d: &dyn Fn(&QRec, usize) -> bool| {
        let mut pool = 0f64;
        let mut found = vec![0usize; TRANSFORMS.len()];
        for r in recs.iter() {
            pool += (0..r.dscore.len()).filter(|&k| pass_d(r, k)).count() as f64 / nd;
            if pass_t(r) {
                found[r.t] += 1;
            }
        }
        pool /= recs.len() as f64;
        let tot: usize = found.iter().sum();
        print!("| {name} | {:.3}% | {} | {:.1}% |", 100.0 * pool, if pool > 0.0 { format!("{:.0}×", 1.0 / pool) } else { "∞".into() }, 100.0 * tot as f64 / recs.len() as f64);
        for (_, members) in gs.iter() {
            let (mut f, mut k) = (0, 0);
            for &m in members.iter() {
                let t = TRANSFORMS.iter().position(|&x| x == m).unwrap();
                f += found[t];
                k += per_t[t];
            }
            if k == 0 {
                print!(" — |");
            } else {
                print!(" {:.0}% |", 100.0 * f as f64 / k as f64);
            }
        }
        println!();
    };
    for (name, d) in designs().iter() {
        row(name.to_string(), &|r: &QRec| d(pack(&r.tlev)), &|r: &QRec, k: usize| d(r.dlev[k]));
    }
    let mut all_scores: Vec<i32> = recs.iter().flat_map(|r| r.dscore.iter().copied()).filter(|&s| s > i32::MIN).collect();
    all_scores.sort_unstable();
    let tot_pairs = recs.len() as f64 * nd;
    let mut ths: Vec<i32> = Vec::new();
    for frac in [0.02f64, 0.01, 0.005, 0.002, 0.001] {
        let k = (tot_pairs * frac) as usize;
        if k < all_scores.len() {
            ths.push(all_scores[all_scores.len() - 1 - k]);
        }
    }
    ths.push(prof.threshold);
    ths.sort_unstable();
    ths.dedup();
    for &th in ths.iter() {
        row(format!("SI score ≥ {th}{}", if th == prof.threshold { " (SI1 default)" } else { "" }), &|r: &QRec| r.ttouch && r.tscore >= th, &|r: &QRec, k: usize| r.dscore[k] >= th);
    }

    // ------------------------------------------------- the funnel
    println!("\n## The funnel: SI ∪ the exact-key index, then XRank\n");
    let keyset: Vec<(Vec<u64>, Vec<u64>)> = c.sides.iter().map(|p| index_keys(p, false)).collect();
    let dsets_c: Vec<&Vec<u64>> = c.ev_d.iter().map(|&i| &keyset[i].0).collect();
    let dsets_b: Vec<&Vec<u64>> = c.ev_d.iter().map(|&i| &keyset[i].1).collect();
    let (kc, kb) = (KeyIndex::build(&dsets_c), KeyIndex::build(&dsets_b));
    let e2e = arg(args, "--e2e", usize::MAX).min(recs.len());
    let cfg = Config::default();
    let t2 = Instant::now();
    let xps: HashMap<usize, XPrepared> = {
        let mut need: Vec<usize> = c.ev_d.clone();
        for r in recs.iter() {
            need.push(r.qi);
            need.push(r.ti);
        }
        need.sort_unstable();
        need.dedup();
        need.into_iter().map(|i| (i, XPrepared::new(Prepared::new(&c.recs[i].t1, Some(&c.recs[i].t2)).unwrap(), xb))).collect()
    };
    eprintln!("xprepared {} sides in {:.1} s", xps.len(), t2.elapsed().as_secs_f64());
    let mut ctx = XCtx::new();
    let mut rs = RankScratch::new();
    let mut acc = Vec::new();
    let mut touched = Vec::new();
    let th = prof.threshold;
    let budget = prof.budget as usize;
    let mut f_keys = vec![0usize; TRANSFORMS.len()];
    let mut f_si = vec![0usize; TRANSFORMS.len()];
    let mut f_union = vec![0usize; TRANSFORMS.len()];
    let mut f_e2e = vec![0usize; TRANSFORMS.len()];
    let mut n_e2e = vec![0usize; TRANSFORMS.len()];
    let (mut pool_si, mut pool_keys, mut pool_union) = (0usize, 0usize, 0usize);
    let mut false_copies = 0usize;
    let mut false_kinds: HashMap<(&'static str, &'static str), usize> = HashMap::new();
    let mut false_bg: HashMap<(i32, i32), usize> = HashMap::new();
    let first_d = c.recs.iter().position(|r| r.base < 0).unwrap_or(0);
    let mut t_rank: Vec<f64> = Vec::new();
    // XRank on negatives only (the candidates without the target): the
    // verification cost a pool of unrelated works charges per candidate
    let (mut neg_us, mut neg_n) = (0f64, 0usize);
    let mut per_cand: Vec<f64> = Vec::new();
    let (mut compared, mut shown) = (0usize, 0usize);
    let mut t_keys: Vec<f64> = Vec::new();
    for (k, r) in recs.iter().enumerate() {
        let tk = Instant::now();
        let (qc, qb) = index_keys(&c.sides[r.qi], true);
        let sc = kc.score(&qc, &keyset[r.ti].0, DF_CAP, &mut acc, &mut touched);
        let rc = rank_of(sc, &acc, &touched);
        let mut cand: Vec<usize> = top_k(&acc, &touched, KEY_K).into_iter().map(|s| c.ev_d[s as usize]).collect();
        let sb = kb.score(&qb, &keyset[r.ti].1, DF_CAP, &mut acc, &mut touched);
        let rb = rank_of(sb, &acc, &touched);
        cand.extend(top_k(&acc, &touched, KEY_K).into_iter().map(|s| c.ev_d[s as usize]));
        t_keys.push(tk.elapsed().as_secs_f64() * 1e6);
        let by_keys = rc <= KEY_K || rb <= KEY_K;
        let by_si = r.ttouch && r.tscore >= th;
        cand.sort_unstable();
        cand.dedup();
        pool_keys += cand.len();
        let mut si: Vec<(i32, usize)> = (0..r.dscore.len()).filter(|&d| r.dscore[d] >= th).map(|d| (r.dscore[d], c.ev_d[d])).collect();
        si.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        si.truncate(budget);
        pool_si += si.len();
        cand.extend(si.iter().map(|x| x.1));
        cand.sort_unstable();
        cand.dedup();
        pool_union += cand.len();
        if by_keys {
            f_keys[r.t] += 1;
        }
        if by_si {
            f_si[r.t] += 1;
        }
        if by_keys || by_si {
            f_union[r.t] += 1;
        }
        if k < e2e && k % 6 == 0 && !cand.is_empty() {
            let sides: Vec<Option<&XPrepared>> = cand.iter().filter(|&&i| i != r.ti).map(|i| Some(&xps[i])).collect();
            let mut out = vec![0i32; XRANK_FIELDS * sides.len().max(1)];
            let tr = Instant::now();
            xrank(&xps[&r.qi], &sides, &cfg, xb, &XRankOptions::default(), &mut ctx, &mut rs, &mut out);
            neg_us += tr.elapsed().as_secs_f64() * 1e6;
            neg_n += sides.len();
        }
        if k < e2e {
            n_e2e[r.t] += 1;
            if by_keys || by_si {
                cand.push(r.ti);
            }
            let sides: Vec<Option<&XPrepared>> = cand.iter().map(|i| Some(&xps[i])).collect();
            let mut out = vec![0i32; XRANK_FIELDS * sides.len().max(1)];
            let tr = Instant::now();
            xrank(&xps[&r.qi], &sides, &cfg, xb, &XRankOptions::default(), &mut ctx, &mut rs, &mut out);
            let ms = tr.elapsed().as_secs_f64() * 1e3;
            t_rank.push(ms);
            per_cand.push(1e3 * ms / cand.len().max(1) as f64);
            for (m, &i) in cand.iter().enumerate() {
                let st = out[m * XRANK_FIELDS];
                shown += 1;
                if st >= 0 {
                    compared += 1;
                }
                let is_copy = st == 3 || st == 4;
                if i == r.ti {
                    if is_copy {
                        f_e2e[r.t] += 1;
                    }
                } else if is_copy {
                    false_copies += 1;
                    let qb = c.recs[r.qi].base.max(0) as usize;
                    let dk = if c.recs[i].base < 0 { distractor_kind(i - first_d) } else { "variant" };
                    *false_kinds.entry((base_kind(qb), dk)).or_insert(0) += 1;
                    if c.recs[i].base < 0 {
                        *false_bg.entry((base_bg(qb), distractor_bg(i - first_d))).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    let q = recs.len() as f64;
    println!("Keys: top {KEY_K} by Σ 1/df per key family (codes, bands; df cap {DF_CAP}) as `docs/SEARCH.md`. SI: score ≥ {th}, budget {budget}. Eval population {} distractors.\n", c.ev_d.len());
    println!("| | candidates per query | recall |");
    println!("|---|---:|---:|");
    let tot = |v: &Vec<usize>| v.iter().sum::<usize>() as f64 / q;
    println!("| exact keys alone | {:.1} | {:.1}% |", pool_keys as f64 / q, 100.0 * tot(&f_keys));
    println!("| SI alone | {:.1} | {:.1}% |", pool_si as f64 / q, 100.0 * tot(&f_si));
    println!("| SI ∪ keys | {:.1} | {:.1}% |", pool_union as f64 / q, 100.0 * tot(&f_union));
    let ne: usize = n_e2e.iter().sum();
    println!("| SI ∪ keys → XRank says Copy | {:.1} | {:.1}% of {} |", pool_union as f64 / q, 100.0 * f_e2e.iter().sum::<usize>() as f64 / ne.max(1) as f64, ne);
    println!("\nCopies XRank found among the distractors it was shown: {false_copies}. XRank per query: p50 {:.1} ms, p95 {:.1} ms; key nomination p50 {:.0} µs (in-memory sorted arrays).", pct(&mut t_rank, 0.5), pct(&mut t_rank, 0.95), pct(&mut t_keys, 0.5));
    if !per_cand.is_empty() {
        let mean = per_cand.iter().sum::<f64>() / per_cand.len() as f64;
        println!("XRank cost per candidate, per query: p50 {:.0} µs, mean {:.0} µs, p95 {:.0} µs; it compared {:.1}% of the candidates it was shown (the rest stopped at the route and anchor-tier screen).", pct(&mut per_cand, 0.5), mean, pct(&mut per_cand, 0.95), 100.0 * compared as f64 / shown.max(1) as f64);
    }
    if neg_n > 0 {
        println!("XRank on the pools without their target (every sixth query, {neg_n} candidates): {:.0} µs per candidate.", neg_us / neg_n as f64);
    }
    let mut fk: Vec<_> = false_kinds.into_iter().collect();
    fk.sort_by(|a, b| b.1.cmp(&a.1));
    println!("By generator (query base → distractor): {}", fk.iter().map(|((a, b), n)| format!("{a} → {b}: {n}")).collect::<Vec<_>>().join(", "));
    let mut fb: Vec<_> = false_bg.into_iter().collect();
    fb.sort_by(|a, b| b.1.cmp(&a.1));
    println!("By backdrop (base → distractor; 0 transparent, 1 flat, 2 dithered gradient, −1 noise field): {}\n", fb.iter().map(|((a, b), n)| format!("{a} → {b}: {n}")).collect::<Vec<_>>().join(", "));
    println!("| transform | pairs | keys | SI | SI ∪ keys | → XRank Copy |");
    println!("|---|---:|---:|---:|---:|---:|");
    for t in 1..TRANSFORMS.len() {
        if per_t[t] == 0 {
            continue;
        }
        let p = |v: &Vec<usize>, n: usize| if n == 0 { "—".to_string() } else { format!("{:.0}%", 100.0 * v[t] as f64 / n as f64) };
        println!("| {} | {} | {} | {} | {} | {} |", TRANSFORMS[t], per_t[t] / 2, p(&f_keys, per_t[t]), p(&f_si, per_t[t]), p(&f_union, per_t[t]), p(&f_e2e, n_e2e[t]));
    }

    // ------------------------------------------------- index timing
    println!("\n## The index itself\n");
    let mut idx = SiIndex::new();
    for &d in c.ev_d.iter() {
        idx.add(sigs[d]);
    }
    let mut out = Vec::new();
    let mut tq: Vec<f64> = Vec::new();
    let mut tb: Vec<f64> = Vec::new();
    let mut same = 0usize;
    let mut post = 0usize;
    for r in recs.iter() {
        let t = Instant::now();
        let q = SiQuery::from_prepared(&c.sides[r.qi], xb, prof);
        tb.push(t.elapsed().as_secs_f64() * 1e6);
        let t = Instant::now();
        let st = idx.query(&q, th, usize::MAX, &mut out);
        tq.push(t.elapsed().as_secs_f64() * 1e6);
        post += st.postings;
        let expect = r.dscore.iter().filter(|&&s| s >= th).count();
        if expect == st.admitted {
            same += 1;
        }
    }
    println!("Index over {} works: {} posting entries ({:.1} per work). Query build p50 {:.0} µs; index query p50 {:.0} µs, p95 {:.0} µs, {:.0} postings read per query; signature from a parsed side {:.0} µs, from the wires (parse included) {:.0} µs per work.", idx.len(), idx.entries(), idx.entries() as f64 / idx.len() as f64, pct(&mut tb, 0.5), pct(&mut tq, 0.5), pct(&mut tq, 0.95), post as f64 / recs.len() as f64, 1e6 * dt_sig, 1e6 * dt_wire);
    println!("Index admissions equal the reference scan on {same} of {} queries.", recs.len());

    // ------------------------------------------------- scaling
    if let Some(i) = args.iter().position(|a| a == "--big") {
        scaling(c, prof, xb, &sigs, &keyset, &recs, &args[i + 1]);
    }
}

fn scaling(c: &Corpus, prof: &SiProfile, xb: &XBound, sigs: &[SiSig], keyset: &[(Vec<u64>, Vec<u64>)], recs: &[QRec], path: &str) {
    println!("\n## Scaling: the same queries against growing populations\n");
    let t0 = Instant::now();
    let big = load(path);
    let mut bsig: Vec<SiSig> = Vec::with_capacity(big.len());
    let mut bkeys: Vec<(Vec<u64>, Vec<u64>)> = Vec::with_capacity(big.len());
    for r in big.iter() {
        let p = Prepared::new(&r.t1, Some(&r.t2)).unwrap();
        let fam = families(&p);
        let route = XRoute::build(&p, &xb.salts, &xb.xp);
        bsig.push(SiSig::from_parts(&fam, &route, prof));
        bkeys.push(index_keys(&p, false));
    }
    drop(big);
    eprintln!("big population: {} works signed and keyed in {:.1} s", bsig.len(), t0.elapsed().as_secs_f64());
    let th = prof.threshold;
    // 2000 is SI1's default budget
    const BUDGETS: [usize; 5] = [250, 1000, 2000, 4000, 16000];
    println!("| population | SI pool | SI pool share | SI recall | keys@{KEY_K} recall | SI ∪ keys recall | SI top-250 ∪ keys | top-1000 ∪ keys | top-2000 ∪ keys | top-4000 ∪ keys | top-16000 ∪ keys | SI postings read / query | SI query p50 | keys query p50 |");
    println!("|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    let mut ns: Vec<usize> = vec![4000, 16_000, 64_000];
    ns.push(c.ev_d.len() + bsig.len());
    ns.retain(|&n| n <= c.ev_d.len() + bsig.len());
    ns.dedup();
    for &n in ns.iter() {
        let extra = n - c.ev_d.len();
        let pop_sigs: Vec<SiSig> = c.ev_d.iter().map(|&i| sigs[i]).chain(bsig[..extra].iter().copied()).collect();
        let pc: Vec<&Vec<u64>> = c.ev_d.iter().map(|&i| &keyset[i].0).chain(bkeys[..extra].iter().map(|k| &k.0)).collect();
        let pb: Vec<&Vec<u64>> = c.ev_d.iter().map(|&i| &keyset[i].1).chain(bkeys[..extra].iter().map(|k| &k.1)).collect();
        let (kc, kb) = (KeyIndex::build(&pc), KeyIndex::build(&pb));
        let mut idx = SiIndex::new();
        for s in pop_sigs.iter() {
            idx.add(*s);
        }
        let (mut pool, mut fs, mut fk, mut fu, mut post) = (0usize, 0usize, 0usize, 0usize, 0usize);
        let mut fb = [0usize; BUDGETS.len()];
        let mut tq: Vec<f64> = Vec::new();
        let mut tk: Vec<f64> = Vec::new();
        let (mut acc, mut touched) = (Vec::new(), Vec::new());
        let mut out = Vec::new();
        for r in recs.iter() {
            let q = SiQuery::from_parts(sigs[r.qi], &c.fam[r.qi], prof);
            let t = Instant::now();
            let st = idx.query(&q, th, usize::MAX, &mut out);
            tq.push(t.elapsed().as_secs_f64() * 1e6);
            pool += st.admitted;
            post += st.postings;
            let s_ok = r.ttouch && r.tscore >= th;
            let t = Instant::now();
            let (qc, qb) = index_keys(&c.sides[r.qi], true);
            let sc = kc.score(&qc, &keyset[r.ti].0, DF_CAP, &mut acc, &mut touched);
            let rc = rank_of(sc, &acc, &touched);
            let top_c = top_k(&acc, &touched, KEY_K);
            let sb = kb.score(&qb, &keyset[r.ti].1, DF_CAP, &mut acc, &mut touched);
            let rb = rank_of(sb, &acc, &touched);
            let top_b = top_k(&acc, &touched, KEY_K);
            tk.push(t.elapsed().as_secs_f64() * 1e6);
            let _ = (top_c.len(), top_b.len());
            let k_ok = rc <= KEY_K || rb <= KEY_K;
            fs += s_ok as usize;
            fk += k_ok as usize;
            fu += (s_ok || k_ok) as usize;
            // the SI part cut to a budget: the target needs fewer than B
            // distractors scoring at least as high
            let ahead = out.partition_point(|x| x.1 >= r.tscore);
            for (k, &b) in BUDGETS.iter().enumerate() {
                if k_ok || (s_ok && ahead < b) {
                    fb[k] += 1;
                }
            }
        }
        let q = recs.len() as f64;
        println!("| {} | {:.0} | {:.3}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.0} | {:.0} µs | {:.0} µs |", n, pool as f64 / q, 100.0 * pool as f64 / q / n as f64, 100.0 * fs as f64 / q, 100.0 * fk as f64 / q, 100.0 * fu as f64 / q, 100.0 * fb[0] as f64 / q, 100.0 * fb[1] as f64 / q, 100.0 * fb[2] as f64 / q, 100.0 * fb[3] as f64 / q, 100.0 * fb[4] as f64 / q, post as f64 / q, pct(&mut tq, 0.5), pct(&mut tk, 0.5));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = sarg(&args, "--dir", "target/si-corpus");
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "corpus" => {
            let nb = arg(&args, "--bases", 120);
            let nd = arg(&args, "--distractors", 8000);
            let from = arg(&args, "--from", 0);
            let name = sarg(&args, "--out", "corpus.bin");
            std::fs::create_dir_all(&dir).unwrap();
            let mut jobs: Vec<(i32, i32, Box<dyn Fn() -> Img + Send + Sync>)> = Vec::new();
            for i in 0..nb {
                for t in 0..TRANSFORMS.len() {
                    jobs.push((i as i32, t as i32, Box::new(move || transform(&base_image(i), t, 31 + i as u64))));
                }
            }
            for k in from..from + nd {
                jobs.push((-1, -1, Box::new(move || distractor(k))));
            }
            let t0 = Instant::now();
            let n = jobs.len();
            let recs = hash_all(jobs);
            save(&format!("{dir}/{name}"), &recs);
            println!("hashed {n} works ({nb} bases × {} variants + {nd} distractors) in {:.1} s → {dir}/{name}", TRANSFORMS.len(), t0.elapsed().as_secs_f64());
        }
        "fit" => {
            let xb = XBound::shipped();
            let c = Corpus::open(&dir, &xb);
            cmd_fit(&c, &sarg(&args, "--out", "../docs/calibration/SI1-PROVISIONAL.psi"));
        }
        "lost" => {
            // comparator-42 Copy pairs XRank does not read Copy when shown the
            // target alone: which stage lets them go
            let xb = XBound::shipped();
            let c = Corpus::open(&dir, &xb);
            let cfg = Config::default();
            let base = Profile::cal004();
            let bcfg = paph::v4::bind(&cfg, &base);
            let mut ctx = XCtx::new();
            let mut rs = RankScratch::new();
            let mut n = 0;
            let mut lost = 0;
            for b in 0..c.nb {
                for t in 1..TRANSFORMS.len() as i32 {
                    if !c.copy(b, t) {
                        continue;
                    }
                    let (i, j) = (c.at[&(b, 0)], c.at[&(b, t)]);
                    for (qi, ti) in [(j, i), (i, j)] {
                        let q = XPrepared::new(Prepared::new(&c.recs[qi].t1, Some(&c.recs[qi].t2)).unwrap(), &xb);
                        let x = XPrepared::new(Prepared::new(&c.recs[ti].t1, Some(&c.recs[ti].t2)).unwrap(), &xb);
                        let mut out = vec![0i32; XRANK_FIELDS];
                        xrank(&q, &[Some(&x)], &cfg, &xb, &XRankOptions::default(), &mut ctx, &mut rs, &mut out);
                        n += 1;
                        if out[0] != 3 && out[0] != 4 {
                            lost += 1;
                            // and what comparator 42 certified the pair on
                            let (pa, pb) = (&c.sides[i], &c.sides[j]);
                            let swapped = canon_swapped(pa, pb);
                            let (ca, cb) = if swapped { (pb, pa) } else { (pa, pb) };
                            let mut pc = PairCtx::new(ca, cb);
                            let v = compare_in(&mut pc, swapped, &bcfg, &base, Reading::Lean).base;
                            println!("base {b} {:<10} state {:>2} execution {} screen {} route class {} (local {}, band {}, global {}) pools {}/{}; comparator 42: {} — {} {:?} (structural {}, geometry evidence {}, inliers {})", TRANSFORMS[t as usize], out[0], out[1], out[2], out[6], out[3], out[4], out[5], out[7], out[8], v.verdict, v.class, v.basis, v.structural, v.geometry_evidence, v.total_inliers);
                        }
                    }
                }
            }
            println!("{lost} of {n} comparator-42 Copy queries not read Copy by XRank alone");
        }
        "route" => {
            // the XRoute class of every comparator-42 Copy pair (all bases):
            // is the route class alone a safe filter?
            let xb = XBound::shipped();
            let c = Corpus::open(&dir, &xb);
            let mut rej = vec![0usize; TRANSFORMS.len()];
            let mut tot = vec![0usize; TRANSFORMS.len()];
            for b in 0..c.nb {
                for t in 1..TRANSFORMS.len() as i32 {
                    if !c.copy(b, t) {
                        continue;
                    }
                    let (i, j) = (c.at[&(b, 0)], c.at[&(b, t)]);
                    let s = paph::x::route::route_score(&c.routes[i], &c.routes[j]);
                    tot[t as usize] += 1;
                    if paph::x::route::route_class(&s, &xb.xp) == paph::x::route::RouteClass::Reject {
                        rej[t as usize] += 1;
                    }
                }
            }
            println!("XRoute class Reject on comparator-42 Copy pairs (X1 bars local {} / band {} / global {}):", xb.xp.t_local_low, xb.xp.t_band_low, xb.xp.t_global_low);
            for t in 1..TRANSFORMS.len() {
                if tot[t] > 0 {
                    println!("  {:<11} {:>3} of {:>3}", TRANSFORMS[t], rej[t], tot[t]);
                }
            }
            println!("  total       {} of {}", rej.iter().sum::<usize>(), tot.iter().sum::<usize>());
        }
        "eval" => {
            let xb = XBound::shipped();
            let prof = match args.iter().position(|a| a == "--profile") {
                Some(i) => SiProfile::decode(&std::fs::read(&args[i + 1]).expect("read profile")).expect("decode profile"),
                None => SiProfile::si1(),
            };
            let c = Corpus::open(&dir, &xb);
            cmd_eval(&c, &prof, &xb, &args);
        }
        _ => {
            println!("sibench corpus | fit | eval   (see the header of rust/src/bin/sibench.rs)");
        }
    }
    let _ = std::io::stdout().flush();
}
