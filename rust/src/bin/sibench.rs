//! PAPH-SI benchmark and fitting harness (SPEC-SI §9), the corpus the
//! 1.1.1 screen fixes were measured on (docs/PAPH-X.md §6), the chain's
//! artworks (1.1.2, SPEC-SI §9.7), and 1.2's calibration, XRank and wire-4
//! measurements (docs/calibration/CAL-007-PROVISIONAL.log, X3-PROVISIONAL.log,
//! SI4-PROVISIONAL.log).
//!
//!     sibench corpus [--bases N] [--distractors N] [--from K] [--wire 3] [--out F]
//!                                                   hash a corpus, cache the wires (`--wire 3`: in
//!                                                   1.0–1.1's format)
//!     sibench corpus --chain DIR [--distractors N] [--wire 3] [--out chain.bin]
//!                                                   the same with the chain's artworks as the bases
//!                                                   (DIR from `node tools/chain-corpus.mjs`)
//!     sibench wire [F]                              the wire format a corpus file holds (3 or 4; 0: none)
//!     sibench dump --base B --tf NAME [--out DIR]   one synthetic work's wires as files, for xcli
//!     sibench chain [--chain DIR]                   the chain's artworks themselves, and every pair
//!                                                   of them through comparator 42 (CAL-007 and
//!                                                   CAL-004), the screens, XRank (X3 and X2), SI3
//!                                                   and SI4
//!     sibench fit [--x2 | --x1] [--out PATH]       fit SI on the synthetic fit split: SI2 (--x2) and
//!                                                   SI1 (--x1) are the artefacts in docs/calibration
//!                                                   (fitted on wire-3 hashes: `--corpus corpus-w3.bin`
//!                                                   reproduces them); under X3 the fit is
//!                                                   SI-SYNTHETIC-X3, no shipped profile, written
//!                                                   under --dir
//!     sibench chainfit [--x2] [--corpus chain.bin] [--name NAME] [--out PATH]
//!                                                   fit SI on the chain's works: two held-out folds
//!                                                   against SI2, then the fit on every base, written
//!                                                   to docs/calibration/NAME.psi — NAME SI4-PROVISIONAL
//!                                                   under X3, SI3-PROVISIONAL under --x2 (fitted on
//!                                                   1.1.2's wire-3 hashes: `--corpus chain-w3.bin`);
//!                                                   a fit on a later snapshot is another profile —
//!                                                   give it another name
//!     sibench calib [--chain DIR] [--out DIR]      calibration snapshots (every quantity comparator
//!                                                   42's lattice reads) of the chain's pairs and of
//!                                                   both corpora's copies and negatives, TSV under
//!                                                   target/calib (tools/cal-lattice.py reads them)
//!     sibench calfit [--chain DIR] [--out PATH]    CAL-007's moderate structural bar from the
//!                                                   chain's pairs of two authors' works, what it moves
//!                                                   against CAL-004 on the chain and both corpora;
//!                                                   writes the .pcal when the fit is CAL-007's
//!     sibench xtrace [--out DIR]                   the cascade's geometry tier by tier on the chain's
//!                                                   queries and every corpus copy query (what an exit
//!                                                   from the anchor expansion would have to know)
//!     sibench xtime [--chain DIR] [--step N]       where XRank's time goes on the chain's unrelated
//!                                                   pairs, under CAL-004 and CAL-007
//!     sibench eval [--profile PATH] [--big F] [--e2e N] [--nogate]
//!                                                   stability matrix, the proposal's designs against
//!                                                   SI, the funnel with the key index and XRank,
//!                                                   scaling to the big distractor file, timings
//!     sibench route [--neg N] [--real] [--profile PATH]
//!                                                   the route class and the pair screen on every
//!                                                   comparator-42 copy and on unrelated pairs (--real:
//!                                                   every base against every other but those
//!                                                   comparator 42 calls Copy), the bars that
//!                                                   would keep every copy out of the Reject class,
//!                                                   the global words, the SI cells and the DCT
//!                                                   section under the square's symmetries (split by
//!                                                   the DCT thumbnail's grid and the shapes section's)
//!     sibench lost [--nogate]                      the copies XRank does not read Copy, shown the
//!                                                   target alone, and why (and the ones X1 loses
//!                                                   and what keeps them); the copies comparator 42's
//!                                                   own gated rank screens out
//!     sibench doorprof [--real]                    the structural door on unrelated pairs: where
//!                                                   it shuts, what each step costs, the cheapest orders
//!                                                   (--real: every pair of distinct bases but those
//!                                                   comparator 42 calls Copy)
//!
//! `--corpus NAME` picks the corpus file under `--dir` (default corpus.bin).
//! Every command runs under the shipped X3-PROVISIONAL (bound to
//! CAL-007-PROVISIONAL), or under 1.1's X2-PROVISIONAL with `--x2` or 1.0's
//! X1-PROVISIONAL with `--x1` (both bound to CAL-004-PROPOSED); where SI is
//! measured, under the SI profile bound to it — the shipped SI4 under X3, the
//! synthetic fits SI2 and SI1 under X2 and X1 — or `--profile PATH`.
//! Experiments: `--bars l,b,g` (route lower bars), `--nodoor`.
//!
//! The caches hold the wires of the release that wrote them: `corpus` hashes
//! in this release's format (wire 4) unless told `--wire 3`, `load` notes a
//! file hashed in another, and `rust/sibench.sh` re-hashes such a file before
//! it measures.  A fitted artefact never replaces a committed one in
//! docs/calibration with other bytes: the run says NOT WRITTEN and exits 2
//! after its report.
//!
//! The synthetic corpus is built from the engine's own generators
//! (`synth.rs`): bases under twenty transforms, and same-style distractors
//! drawn from the same generators with other seeds — the hardest negatives
//! that corpus has.  The chain's corpus takes the chain's artworks as the
//! bases (a real one as the paste's host) and keeps the synthetic
//! distractors as the population; `sibench chain` measures the real pairs.
//! Splits: bases in alternate blocks of eight and the first half of the
//! distractors fit; the other blocks and the second half evaluate.  Recall is
//! measured on the pairs comparator 42 itself calls Copy (or Identical) — a
//! nominator need not find what the verifier cannot confirm — and in both
//! arrival orders.
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
use paph::x::si::fit::{background_count, fit, fit_books, fit_on_books, FitInput, FitOptions};
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

/// Hash every job in the given wire format (3 or 4), on every core.
fn hash_all(jobs: Vec<(i32, i32, Box<dyn Fn() -> Img + Send + Sync>)>, wire: u8) -> Vec<Rec> {
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).max(1);
    let n = jobs.len();
    let jobs = std::sync::Arc::new(jobs);
    let next = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let out = std::sync::Arc::new(std::sync::Mutex::new((0..n).map(|_| None).collect::<Vec<Option<Rec>>>()));
    std::thread::scope(|s| {
        for _ in 0..threads {
            let (jobs, next, out) = (jobs.clone(), next.clone(), out.clone());
            s.spawn(move || {
                let cfg = Config { wire, ..Config::default() };
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

/// The wire format a corpus file was hashed in: its first work's Tier 1
/// version byte (0 for an empty or unreadable file).  Read from the header
/// alone, so `sibench.sh` can ask before it trusts a cache.
fn file_wire(path: &str) -> u8 {
    use std::io::Read;
    let mut b = [0u8; 25];
    match std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut b)) {
        Ok(()) if &b[0..4] == b"SIC1" => b[24],
        _ => 0,
    }
}

pub fn load(path: &str) -> Vec<Rec> {
    let b = std::fs::read(path).expect("read corpus (run `sibench corpus` first)");
    assert_eq!(&b[0..4], b"SIC1");
    let w = file_wire(path);
    if w != 0 && w != paph::wire::VERSION {
        eprintln!("note: {path} holds wire-{w} hashes, and this release writes wire {}: its measurements are of wire {w}'s sections (delete the file, or `sibench corpus` again, for this release's)", paph::wire::VERSION);
    }
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

// ------------------------------------------------------------------ the chain

/// One artwork of a `tools/chain-corpus.mjs` snapshot.
pub struct ChainWork {
    pub author: String,
    pub permlink: String,
    pub created: String,
    pub sha: String,
    pub img: Img,
}

/// A snapshot's works, oldest first, one per distinct image (the first upload
/// of byte-identical image bytes stands for the others), and how many uploads
/// repeated an earlier one's bytes.
fn load_chain(dir: &str) -> (Vec<ChainWork>, usize) {
    let idx = std::fs::read_to_string(format!("{dir}/index.tsv")).expect("read index.tsv (run `node tools/chain-corpus.mjs` first)");
    let mut seen = std::collections::HashSet::new();
    let (mut out, mut dups) = (Vec::new(), 0usize);
    for line in idx.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 9 {
            continue;
        }
        if !seen.insert(f[5].to_string()) {
            dups += 1;
            continue;
        }
        let b = std::fs::read(format!("{dir}/{}", f[8])).expect("read an rgba file");
        let w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
        let h = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
        assert_eq!(b.len(), 8 + w * h * 4, "{}: truncated", f[8]);
        out.push(ChainWork { author: f[0].into(), permlink: f[1].into(), created: f[2].into(), sha: f[5].into(), img: Img { w, h, px: b[8..].to_vec() } });
    }
    (out, dups)
}

/// The snapshot's head block, time and manifest digest, read from its
/// snapshot.json without a JSON parser.
fn chain_snapshot(dir: &str) -> String {
    let s = std::fs::read_to_string(format!("{dir}/snapshot.json")).unwrap_or_default();
    let field = |k: &str| -> String {
        let key = format!("\"{k}\":");
        s.find(&key).map(|i| s[i + key.len()..].split([',', '\n']).next().unwrap_or("").trim().trim_matches('"').to_string()).unwrap_or_default()
    };
    format!("head block {}, {} UTC, manifest {}", field("head_block"), field("time"), &field("manifest_sha256").chars().take(16).collect::<String>())
}

/// The largest original the corpus takes as a base: its 3× upscale stays
/// within the search engine's decode cap (2048 × 2048 pixels).
const CHAIN_BASE_MAX_PIXELS: usize = 2048 * 2048 / 9;

/// The host a real base is pasted into: the next real base, blown up by the
/// smallest integer factor that covers twice the guest and cropped to it.
fn chain_host(guest: &Img, host: &Img) -> Img {
    let (w, h) = (guest.w * 2 + 20, guest.h * 2 + 16);
    let k = ((w + host.w - 1) / host.w).max((h + host.h - 1) / host.h).max(1);
    crop(&nearest_up(host, k), 0, 0, w, h)
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
    /// `path`: a corpus file `sibench corpus` wrote (`--corpus NAME` under
    /// `--dir`, default corpus.bin; `chain.bin` for the chain's).
    fn open(path: &str, xb: &XBound) -> Corpus {
        let t0 = Instant::now();
        let recs = load(path);
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
        // comparator 42 on every base × variant pair, under the calibration
        // the X profile is bound to: what the verifier can confirm
        let base = xb.base.clone();
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

/// The SI profile a measurement reads: `--profile PATH`, or the profile
/// bound to the X profile in use — SI1 under X1 and SI2 under X2 (the
/// synthetic fit SPEC-SI's synthetic tables were printed with), the shipped
/// SI4 under X3.
fn si_profile_arg(args: &[String], xb: &XBound) -> SiProfile {
    match args.iter().position(|a| a == "--profile") {
        Some(i) => SiProfile::decode(&std::fs::read(&args[i + 1]).expect("read profile")).expect("decode profile"),
        None => {
            if xb.xid == XProfile::x1().id() {
                SiProfile::si1()
            } else if xb.xid == XProfile::x2().id() {
                SiProfile::si2()
            } else {
                SiProfile::shipped()
            }
        }
    }
}

/// The SI profile name a fit writes for the X profile it bands: SI1 for
/// X1 (1.1.0's), SI2 for X2 — the same codebooks and weights (route
/// derivation 2 changes the global words, not the lanes SI bands), bound to
/// the other X profile.
fn si_name_for(xb: &XBound) -> &'static str {
    if xb.xid == XProfile::x1().id() {
        "SI1-PROVISIONAL"
    } else if xb.xid == XProfile::x2().id() {
        "SI2-PROVISIONAL"
    } else {
        // a synthetic fit bound to X3 is no shipped profile
        "SI-SYNTHETIC-X3"
    }
}

fn cmd_fit(c: &Corpus, xb: &XBound, out: &str) {
    let mh: Vec<SiSig> = c.routes.iter().map(SiSig::minhash).collect();
    let paste = TRANSFORMS.iter().position(|&t| t == "paste").unwrap() as i32;
    let noise: Vec<(usize, usize)> = c.pairs(false, |_, t| t != paste).iter().map(|p| (p.1, p.2)).collect();
    let evidence: Vec<(usize, usize)> = c
        .pairs(false, |b, t| t != paste && matches!(c.verdict.get(&(b, t)), Some(2) | Some(3) | Some(4)))
        .iter()
        .map(|p| (p.1, p.2))
        .collect();
    let mut name = [0u8; 16];
    name[..15].copy_from_slice(si_name_for(xb).as_bytes());
    let o = FitOptions {
        name,
        xid: xb.xid,
        probes: 4,
        random_per_query: 400,
        seed: 0x5349_3150_524f_5631,
        admit_ppm: 10_000,
        budget: 2000,
    };
    let t0 = Instant::now();
    let inp = FitInput { fam: &c.fam, mh: &mh, background: &c.fit_d, noise_pairs: &noise, evidence_pairs: &evidence, related: None };
    let prof = fit(&inp, &o);
    let b = prof.encode();
    let wrote = write_artefact(out, &b);
    println!("fitted {} on {} background works, {} noise pairs, {} evidence pairs in {:.2} s", prof.name_str(), c.fit_d.len(), noise.len(), evidence.len(), t0.elapsed().as_secs_f64());
    println!("  {} bytes, id {}  → {}", b.len(), paph::sha256::hex(&prof.id()), if wrote { out.to_string() } else { format!("not written ({out} differs)") });
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

/// `chain`: the corpus' bases are the chain's artworks (`corpus --chain`), which
/// have no generator or backdrop to report false copies by.
fn cmd_eval(c: &Corpus, prof: &SiProfile, xb: &XBound, args: &[String], chain: bool) {
    let xopts = if args.iter().any(|a| a == "--nogate") { XRankOptions { gate: false, ..XRankOptions::default() } } else { XRankOptions::default() };
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
        row(format!("SI score ≥ {th}{}", if th == prof.threshold { " (the profile's default)" } else { "" }), &|r: &QRec| r.ttouch && r.tscore >= th, &|r: &QRec, k: usize| r.dscore[k] >= th);
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
    let mut false_42 = 0usize;
    let base42 = xb.base.clone();
    let bcfg42 = paph::v4::bind(&Config::default(), &base42);
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
            xrank(&xps[&r.qi], &sides, &cfg, xb, &xopts, &mut ctx, &mut rs, &mut out);
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
            xrank(&xps[&r.qi], &sides, &cfg, xb, &xopts, &mut ctx, &mut rs, &mut out);
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
                    // does comparator 42 call the pair Copy too? (XRank's safe
                    // policy promises its Copy answers)
                    let (pa, pb) = (&c.sides[r.qi], &c.sides[i]);
                    let sw = canon_swapped(pa, pb);
                    let (ca, cb) = if sw { (pb, pa) } else { (pa, pb) };
                    let v42 = compare_in(&mut PairCtx::new(ca, cb), sw, &bcfg42, &base42, Reading::Lean).base.verdict;
                    false_42 += (v42 == "Copy" || v42 == "Identical") as usize;
                    let qb = c.recs[r.qi].base.max(0) as usize;
                    let dk = if c.recs[i].base < 0 { distractor_kind(i - first_d) } else { "variant" };
                    *false_kinds.entry((if chain { "artwork" } else { base_kind(qb) }, dk)).or_insert(0) += 1;
                    if c.recs[i].base < 0 && !chain {
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
    println!("\nCopies XRank found among the distractors it was shown: {false_copies} (comparator 42 calls {false_42} of them Copy too). XRank per query: p50 {:.1} ms, p95 {:.1} ms; key nomination p50 {:.0} µs (in-memory sorted arrays).", pct(&mut t_rank, 0.5), pct(&mut t_rank, 0.95), pct(&mut t_keys, 0.5));
    if !per_cand.is_empty() {
        let mean = per_cand.iter().sum::<f64>() / per_cand.len() as f64;
        println!("XRank cost per candidate, per query: p50 {:.0} µs, mean {:.0} µs, p95 {:.0} µs; it compared {:.1}% of the candidates it was shown (the rest stopped at the route and anchor-tier screen).", pct(&mut per_cand, 0.5), mean, pct(&mut per_cand, 0.95), 100.0 * compared as f64 / shown.max(1) as f64);
    }
    if neg_n > 0 {
        println!("XRank on the pools without their target (every sixth query, {neg_n} candidates): {:.0} µs per candidate.", neg_us / neg_n as f64);
    }
    let mut fk: Vec<_> = false_kinds.into_iter().collect();
    fk.sort_by(|a, b| b.1.cmp(&a.1));
    if false_copies > 0 {
        println!("By generator (query base → distractor): {}", fk.iter().map(|((a, b), n)| format!("{a} → {b}: {n}")).collect::<Vec<_>>().join(", "));
        if !chain {
            let mut fb: Vec<_> = false_bg.into_iter().collect();
            fb.sort_by(|a, b| b.1.cmp(&a.1));
            println!("By backdrop (base → distractor; 0 transparent, 1 flat, 2 dithered gradient, −1 noise field): {}", fb.iter().map(|((a, b), n)| format!("{a} → {b}: {n}")).collect::<Vec<_>>().join(", "));
        }
    }
    println!();
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
    // 2000 is the profiles' default budget
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

// ------------------------------------------------------------ the screen

/// The 16 x 16 block of the Tier-1 DCT section: (sign, magnitude) per
/// coefficient, `v * 16 + u`.
fn dct16(t: &paph::wire::Tier1) -> ([u8; 256], [u8; 256]) {
    let d = t.sec("dct");
    let (mut s, mut m) = ([0u8; 256], [0u8; 256]);
    for i in 0..256 {
        let b = 2 * i;
        s[i] = (d[b >> 3] >> (7 - (b & 7))) & 1;
        let b = b + 1;
        m[i] = (d[b >> 3] >> (7 - (b & 7))) & 1;
    }
    (s, m)
}

/// `sibench route`: what the route class and the pair screen do with the
/// comparator-42 copies of the corpus and with unrelated pairs, the bars
/// that would keep every copy out of the Reject class, and how far the
/// route's global words — and the DCT section under them — are invariant
/// under the square's symmetries.
fn cmd_route(c: &Corpus, xb: &XBound, args: &[String]) {
    use paph::x::compare::{xscreen, ScreenState};
    use paph::x::route::{RouteClass, RouteScore, RF_BAND, RF_GLOBAL, RF_LOCAL};
    let cfg = Config::default();
    let mut ctx = XCtx::new();
    let xp = &xb.xp;
    let xside = |i: usize| XPrepared::new(Prepared::new(&c.recs[i].t1, Some(&c.recs[i].t2)).unwrap(), xb);
    let nt = TRANSFORMS.len();
    let (mut tot, mut cls, mut scr) = (vec![0usize; nt], vec![0usize; nt], vec![0usize; nt]);
    let mut pos: Vec<RouteScore> = Vec::new();
    let mut neg: Vec<RouteScore> = Vec::new();
    let dist: Vec<usize> = (0..c.recs.len()).filter(|&i| c.recs[i].base < 0).collect();
    let k_neg = arg(args, "--neg", 40);
    // --real (the chain's corpus): every base against every other base, the
    // population of real works, instead of synthetic distractors
    let real = args.iter().any(|a| a == "--real");
    let originals: Vec<XPrepared> = if real { (0..c.nb).map(|b| xside(c.at[&(b, 0)])).collect() } else { Vec::new() };
    // … but not the originals comparator 42 calls copies of each other
    let copies42 = if real { originals_copies42(c) } else { Default::default() };
    // two partitions of the bases: the thumbnail's (both sides multiples of
    // 16) and the shapes section's (long side at most 128 px after the front
    // end's integer downscale, where its grid is the pixel grid)
    const SPLIT: [&str; 4] = ["both sides multiples of 16", "other sizes", "long side ≤ 128 px", "long side > 128 px"];
    let mut g_eq = [[0usize; 4]; 4];
    let mut g_n = [0usize; 4];
    // the SI cells on the same copies, under the profile `si_profile_arg`
    // picks (SI4 under X3, SI2 under --x2, SI1 under --x1 — SI1 and SI2 are
    // one fit; any other with --profile)
    let prof = si_profile_arg(args, xb);
    let mut si_eq = [[0usize; FAMILIES]; 4];
    let mut si_n = [[0usize; FAMILIES]; 4];
    let mut flip_s = [[0usize; 8]; 8];
    let mut flip_m = [[0usize; 8]; 8];
    let mut n_d4 = 0usize;
    for b in 0..c.nb {
        let i = c.at[&(b, 0)];
        let xa = xside(i);
        let t1 = &c.sides[i].t1;
        let grid = if t1.width % 16 == 0 && t1.height % 16 == 0 { 0 } else { 1 };
        let long = if t1.width.max(t1.height) / (t1.scale.max(1) as usize) <= 128 { 2 } else { 3 };
        for t in 1..nt as i32 {
            let j = c.at[&(b, t)];
            let xj = xside(j);
            if c.copy(b, t) {
                let s = xscreen(&xa, &xj, &cfg, xb, &mut ctx);
                tot[t as usize] += 1;
                cls[t as usize] += (s.route_class == RouteClass::Reject) as usize;
                scr[t as usize] += (s.state == ScreenState::Reject) as usize;
                pos.push(s.route);
            }
            if ["mirror", "rot90", "rot180", "transpose"].contains(&TRANSFORMS[t as usize]) {
                let (sa, sj) = (SiSig::from_parts(&c.fam[i], &c.routes[i], &prof), SiSig::from_parts(&c.fam[j], &c.routes[j], &prof));
                for part in [grid, long] {
                    g_n[part] += 1;
                    for k in 0..4 {
                        g_eq[part][k] += (xa.route.global[k] == xj.route.global[k]) as usize;
                    }
                    for f in 0..FAMILIES {
                        if sa.present >> f & 1 != 0 && sj.present >> f & 1 != 0 {
                            si_n[part][f] += 1;
                            si_eq[part][f] += (sa.cells[f] == sj.cells[f]) as usize;
                        }
                    }
                }
                // the DCT section itself: the copy's block against the
                // original's under the D4 element that fits it best
                if c.sides[i].t1.flags & paph::wire::F_FLAT == 0 {
                    let (so, mo) = dct16(&c.sides[i].t1);
                    let (sc, mc) = dct16(&c.sides[j].t1);
                    let mut best = (usize::MAX, 0usize);
                    for e in 0..8usize {
                        let (tr, fh, fv) = (e & 4 != 0, e & 2 != 0, e & 1 != 0);
                        let mut mis = 0usize;
                        for v in 0..16usize {
                            for u in 0..16usize {
                                if u + v == 0 {
                                    continue;
                                }
                                let (su, sv) = if tr { (v, u) } else { (u, v) };
                                let s = so[sv * 16 + su] ^ ((fh as u8) & (u as u8) & 1) ^ ((fv as u8) & (v as u8) & 1);
                                mis += (s != sc[v * 16 + u]) as usize;
                            }
                        }
                        if mis < best.0 {
                            best = (mis, e);
                        }
                    }
                    let e = best.1;
                    let (tr, fh, fv) = (e & 4 != 0, e & 2 != 0, e & 1 != 0);
                    n_d4 += 1;
                    for v in 0..8usize {
                        for u in 0..8usize {
                            let (su, sv) = if tr { (v, u) } else { (u, v) };
                            let s = so[sv * 16 + su] ^ ((fh as u8) & (u as u8) & 1) ^ ((fv as u8) & (v as u8) & 1);
                            flip_s[v][u] += (s != sc[v * 16 + u]) as usize;
                            flip_m[v][u] += (mo[sv * 16 + su] != mc[v * 16 + u]) as usize;
                        }
                    }
                }
            }
        }
        if real {
            let i = c.at[&(b, 0)];
            for o in 0..c.nb {
                let j = c.at[&(o, 0)];
                if o != b && !copies42.contains(&(i.min(j), i.max(j))) {
                    neg.push(xscreen(&xa, &originals[o as usize], &cfg, xb, &mut ctx).route);
                }
            }
            continue;
        }
        for k in 0..k_neg {
            let d = dist[(b as usize * 7919 + k * 104_729) % dist.len()];
            neg.push(xscreen(&xa, &xside(d), &cfg, xb, &mut ctx).route);
        }
        for k in 1..=5 {
            let o = c.at[&((b + 13 * k) % c.nb, 0)];
            neg.push(xscreen(&xa, &xside(o), &cfg, xb, &mut ctx).route);
        }
    }
    println!("Route class and pair screen on comparator-42 Copy pairs — {} (route derivation {}, bars local {} / band {} / global {}, structural door {}):\n", xp.name_str(), xp.route_derivation, xp.t_local_low, xp.t_band_low, xp.t_global_low, if xp.gate_door != 0 { "on" } else { "off" });
    println!("| transform | copies | in the Reject class | rejected by the screen |");
    println!("|---|---:|---:|---:|");
    for t in 1..nt {
        if tot[t] > 0 {
            println!("| {} | {} | {} | {} |", TRANSFORMS[t], tot[t], cls[t], scr[t]);
        }
    }
    println!("| total | {} | {} | {} |", tot.iter().sum::<usize>(), cls.iter().sum::<usize>(), scr.iter().sum::<usize>());
    // the bars that keep every copy out of the class
    let low = |r: &RouteScore, l: i32, b: i32, g: i32| -> bool {
        let m = r.measurable;
        m != 0 && (m & RF_LOCAL == 0 || r.local < l) && (m & RF_BAND == 0 || r.band < b) && (m & RF_GLOBAL == 0 || r.global < g)
    };
    let rate = |l: i32, b: i32, g: i32| neg.iter().filter(|r| low(r, l, b, g)).count() as f64 / neg.len() as f64;
    let mut best = (-1.0f64, 0, 0, 0);
    for l in 0..=16 {
        for b in 0..=10 {
            for g in 0..=256 {
                if pos.iter().any(|r| low(r, l, b, g)) {
                    continue;
                }
                let v = rate(l, b, g);
                if v > best.0 {
                    best = (v, l, b, g);
                }
            }
        }
    }
    let who = if real { format!("each base against every other base but the {} pairs comparator 42 calls Copy", copies42.len()) } else { format!("each base against {k_neg} distractors and 5 other bases") };
    println!("\nUnrelated pairs ({who}, {} pairs): the profile's bars put {:.1}% in the Reject class; the largest bars that put no copy there — local {} / band {} / global {} — put {:.1}%.", neg.len(), 100.0 * rate(xp.t_local_low, xp.t_band_low, xp.t_global_low), best.1, best.2, best.3, 100.0 * best.0);
    println!("\nThe route's global words on D4 copies (mirror, rot90, rot180, transpose; every base): share equal to the original's, under two partitions of the bases — the DCT thumbnail's (16 × 16 cells) and the shapes section's (cells of ⌈long side / 128⌉ px, sizes after the front end's integer downscale).\n");
    println!("| canvas | pairs | G0 (DCT) | G1 (runs) | G2 (adjacency) | G3 (regions) |");
    println!("|---|---:|---:|---:|---:|---:|");
    for g in 0..4 {
        let n = g_n[g].max(1) as f64;
        print!("| {} | {} |", SPLIT[g], g_n[g]);
        for k in 0..4 {
            print!(" {:.0}% ({}) |", 100.0 * g_eq[g][k] as f64 / n, g_eq[g][k]);
        }
        println!();
    }
    println!("\nThe SI cells ({}) on the same copies: share equal to the original's, of the pairs where the family is present in both (counts in brackets).\n", prof.name_str());
    print!("| canvas |");
    for f in 0..FAMILIES {
        print!(" {} |", FAMILY_NAMES[f]);
    }
    print!("\n|---|");
    for _ in 0..FAMILIES {
        print!("---:|");
    }
    println!();
    for g in 0..4 {
        print!("| {} |", SPLIT[g]);
        for f in 0..FAMILIES {
            print!(" {:.0}% ({}/{}) |", 100.0 * si_eq[g][f] as f64 / si_n[g][f].max(1) as f64, si_eq[g][f], si_n[g][f]);
        }
        println!();
    }
    let mut ms: Vec<f64> = Vec::new();
    let mut ss: Vec<f64> = Vec::new();
    for v in 0..8 {
        for u in 0..8 {
            if u + v > 0 {
                ms.push(100.0 * flip_m[v][u] as f64 / n_d4.max(1) as f64);
                ss.push(100.0 * flip_s[v][u] as f64 / n_d4.max(1) as f64);
            }
        }
    }
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    ss.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("\nThe Tier-1 DCT section itself on the {n_d4} D4 copies with a measurable thumbnail, aligned under the best of the eight symmetries: over the 63 AC coefficients of the lowest 8 x 8 frequencies, a magnitude bit differs from the original's on {:.1}–{:.1}% of copies (median {:.1}%), a sign bit on {:.1}–{:.1}% (median {:.1}%).", ms[0], ms[ms.len() - 1], ms[ms.len() / 2], ss[0], ss[ss.len() - 1], ss[ss.len() / 2]);
}

/// `sibench chain [--chain DIR]`: the chain's artworks themselves — what they
/// are (sizes, integer upscales, transparency, palettes, keypoints, hashing
/// cost) and every pair of them through comparator 42 (the shipped CAL-007,
/// and 1.1's CAL-004 beside it), its gated rank's screen, the pair screen,
/// XRank shown the target alone under the shipped X3 and 1.1's X2, and the
/// shipped SI4's score.  The pairs comparator 42 calls Copy are written, with
/// their accounts, to DIR/copies.tsv beside the snapshot, not printed.
fn cmd_chain(src: &str) {
    use paph::x::compare::xscreen;
    use paph::x::route::RouteClass;
    let (works, dups) = load_chain(src);
    let n = works.len();
    let authors: std::collections::BTreeSet<&str> = works.iter().map(|w| w.author.as_str()).collect();
    let (first, last) = (works.iter().map(|w| w.created.as_str()).min().unwrap_or(""), works.iter().map(|w| w.created.as_str()).max().unwrap_or(""));
    // the containers, as the manifest records them (one JSON object a line)
    let manifest = std::fs::read_to_string(format!("{src}/manifest.jsonl")).unwrap_or_default();
    let mut containers: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for line in manifest.lines().filter(|l| !l.trim().is_empty()) {
        let mime = line.split("\"mime\":\"").nth(1).and_then(|r| r.split('"').next()).unwrap_or("?");
        let lossy = if line.contains("\"lossy\":true") { "lossy" } else { "lossless" };
        *containers.entry(format!("{mime} {lossy}")).or_insert(0) += 1;
    }
    println!("The chain's artworks — {src} ({}): {} artworks, {} distinct images ({} byte-identical re-uploads left out), by {} authors, posted {first} to {last} UTC; as uploaded: {}\n", chain_snapshot(src), n + dups, n, dups, authors.len(), containers.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", "));
    let cfg = Config::default();
    let rot = RotCache::new(&pattern());
    let (mut t_hash, mut t_prep) = (Vec::new(), Vec::new());
    let mut fps = Vec::new();
    for w in works.iter() {
        let t0 = Instant::now();
        fps.push(hash(&w.img.px, w.img.w, w.img.h, &cfg, &rot));
        t_hash.push(t0.elapsed().as_secs_f64() * 1e3);
    }
    let sides: Vec<Prepared> = fps.iter().map(|f| Prepared::new(&f.t1, Some(&f.t2)).unwrap()).collect();
    // the shipped pair (X3, CAL-007) and 1.1's (X2, CAL-004)
    let (x1b, x2b) = (XBound::x2(), XBound::shipped());
    let mut xs2 = Vec::new();
    for f in fps.iter() {
        let t0 = Instant::now();
        xs2.push(XPrepared::new(Prepared::new(&f.t1, Some(&f.t2)).unwrap(), &x2b));
        t_prep.push(t0.elapsed().as_secs_f64() * 1e3);
    }
    let xs1: Vec<XPrepared> = fps.iter().map(|f| XPrepared::new(Prepared::new(&f.t1, Some(&f.t2)).unwrap(), &x1b)).collect();

    // ---- what the works are
    let q = |v: &mut Vec<f64>| format!("{:.0} / {:.0} / {:.0} / {:.0} / {:.0}", pct(v, 0.0), pct(v, 0.1), pct(v, 0.5), pct(v, 0.9), pct(v, 1.0));
    let mut wv: Vec<f64> = works.iter().map(|w| w.img.w as f64).collect();
    let mut hv: Vec<f64> = works.iter().map(|w| w.img.h as f64).collect();
    let norm = |i: usize| -> (usize, usize) { let s = sides[i].t1.scale.max(1) as usize; (sides[i].t1.width / s, sides[i].t1.height / s) };
    let mut nlong: Vec<f64> = (0..n).map(|i| { let (a, b) = norm(i); a.max(b) as f64 }).collect();
    let mut kp: Vec<f64> = sides.iter().map(|p| p.kp.len() as f64).collect();
    let mut pal: Vec<f64> = sides.iter().map(|p| p.t1.count("palette") as f64).collect();
    let mut codes: Vec<f64> = sides.iter().map(|p| p.bag.len() as f64).collect();
    println!("| per work | min / p10 / p50 / p90 / max |");
    println!("|---|---:|");
    println!("| width, height (px) | {} · {} |", q(&mut wv), q(&mut hv));
    println!("| long side after the front end's integer downscale (px) | {} |", q(&mut nlong));
    println!("| Tier-2 keypoints | {} |", q(&mut kp));
    println!("| palette entries on the wire | {} |", q(&mut pal));
    println!("| Tier-1 local codes | {} |", q(&mut codes));
    println!("| hash, native, one core (ms) | {} |", q(&mut t_hash.clone()));
    println!("| xprepare under X3 (ms) | {} |", q(&mut t_prep.clone()));
    let count = |f: &dyn Fn(usize) -> bool| (0..n).filter(|&i| f(i)).count();
    let pctn = |c: usize| 100.0 * c as f64 / n.max(1) as f64;
    let up = count(&|i| sides[i].t1.scale > 1);
    let sil = count(&|i| sides[i].t1.flags & paph::wire::F_SIL != 0);
    let matte = count(&|i| sides[i].t1.flags & paph::wire::F_MATTE != 0);
    let flat = count(&|i| sides[i].t1.flags & paph::wire::F_FLAT != 0);
    let m16 = count(&|i| { let (a, b) = norm(i); a % 16 == 0 && b % 16 == 0 });
    let l128 = count(&|i| { let (a, b) = norm(i); a.max(b) <= 128 });
    println!("\nIntegerly upscaled (the hasher divides them first): {up} ({:.0}%). Real transparency or a folded matte (the SIL family present): {sil} ({:.0}%), a matte folded: {matte}. Flat thumbnail: {flat}. Both sides multiples of 16 after the downscale: {m16} ({:.0}%); long side at most 128 px: {l128} ({:.0}%).", pctn(up), pctn(sil), pctn(m16), pctn(l128));
    // the pixagram-search integration hashes a work as it is up to 768² pixels
    // (`PAPH_MAX_PIXELS`) and brings larger ones inside that budget first
    let over_budget: Vec<String> = works.iter().filter(|w| w.img.w * w.img.h > 768 * 768).map(|w| format!("{} × {}", w.img.w, w.img.h)).collect();
    println!("As uploaded, long side over 430 px: {}; over 768² pixels, the pixagram-search integration's hashing budget: {} ({}).", count(&|i| works[i].img.w.max(works[i].img.h) > 430), over_budget.len(), over_budget.join(", "));
    for th in [8usize, 12, 32, 64] {
        let c = count(&|i| sides[i].kp.len() < th);
        print!("{}fewer than {th} keypoints: {c} ({:.0}%)", if th == 8 { "Works with " } else { ", " }, pctn(c));
    }
    println!(".");

    // ---- every pair
    let base = Profile::shipped();
    let bcfg = paph::v4::bind(&cfg, &base);
    // comparator 42 under 1.1's calibration too, for the table
    let base4 = Profile::cal004();
    let mut st4 = [0usize; 6];
    let mut st4_same = [0usize; 6];
    let si = SiProfile::shipped();
    let sigs: Vec<SiSig> = (0..n).map(|i| SiSig::from_prepared(&sides[i], &x2b, &si)).collect();
    let sq: Vec<SiQuery> = (0..n).map(|i| SiQuery::from_prepared(&sides[i], &x2b, &si)).collect();
    let (mut ctx, mut rs) = (XCtx::new(), RankScratch::new());
    let opts = XRankOptions::default();
    let mut st42 = [0usize; 6];
    let mut st42_same = [0usize; 6];
    let mut why: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut why4: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let (mut t42, mut tx2, mut tx1, mut tscr, mut trank42) = (0f64, 0f64, 0f64, 0f64, 0f64);
    let mut cls = [0usize; 4];
    let mut scr = [0usize; 5];
    let mut door_open = 0usize;
    let (mut gate42_pass, mut si_adm, mut si_adm_copy) = (0usize, 0usize, 0usize);
    let mut gate42_out = [0usize; 6];
    let (mut x2_copy, mut x1_copy, mut x2_false, mut x1_false, mut gate42_lost) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut copies: Vec<String> = Vec::new();
    // XRank under X3 by how it decided: [gated, FAST, FALLBACK / EXACT42 ran] × (count, seconds)
    let mut exec2 = [(0usize, 0f64); 3];
    let (mut all_rows, mut hams, mut full, mut exact_struct) = (0usize, 0u64, 0u64, 0usize);
    // (comparator 42's state, XRank's under X3) → queries
    let mut xstate: std::collections::BTreeMap<(i32, i32), usize> = std::collections::BTreeMap::new();
    let mut copy_pairs: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    let (mut ncopy, mut same_author) = (0usize, 0usize);
    let np = n * n.saturating_sub(1) / 2;
    for i in 0..n {
        for j in i + 1..n {
            let (pa, pb) = (&sides[i], &sides[j]);
            let swapped = canon_swapped(pa, pb);
            let (ca, cb) = if swapped { (pb, pa) } else { (pa, pb) };
            let t0 = Instant::now();
            let v = compare_in(&mut PairCtx::new(ca, cb), swapped, &bcfg, &base, Reading::Lean).base;
            t42 += t0.elapsed().as_secs_f64();
            let s42 = state_code(v.verdict);
            st42[s42.clamp(0, 5) as usize] += 1;
            st42_same[s42.clamp(0, 5) as usize] += (works[i].author == works[j].author) as usize;
            let r4 = compare_in(&mut PairCtx::new(ca, cb), swapped, &paph::v4::bind(&cfg, &base4), &base4, Reading::Lean).base;
            let v4 = state_code(r4.verdict);
            if v4 == 2 {
                *why4.entry(r4.class.to_string()).or_insert(0) += 1;
            }
            st4[v4.clamp(0, 5) as usize] += 1;
            st4_same[v4.clamp(0, 5) as usize] += (works[i].author == works[j].author) as usize;
            if s42 == 2 {
                *why.entry(v.class.to_string()).or_insert(0) += 1;
            }
            let is_copy = s42 == 3 || s42 == 4;
            // comparator 42's own gated rank on this candidate, as `rank` (gate:
            // true) runs it: the stage-1 screen, then the lean compare on the
            // pairs it passes, sharing the descriptor scans (xbench's rank42)
            let t0 = Instant::now();
            let mut pc = PairCtx::new(ca, cb);
            let g = paph::v42::screen_in(&mut pc, &bcfg, &base).pass;
            if g {
                std::hint::black_box(compare_in(&mut pc, swapped, &bcfg, &base, Reading::Lean));
            }
            trank42 += t0.elapsed().as_secs_f64();
            gate42_pass += g as usize;
            gate42_out[s42.clamp(0, 5) as usize] += (!g) as usize;
            let t0 = Instant::now();
            let s = xscreen(&xs2[i], &xs2[j], &cfg, &x2b, &mut ctx);
            tscr += t0.elapsed().as_secs_f64();
            cls[match s.route_class { RouteClass::Reject => 0, RouteClass::Defer => 1, RouteClass::Fast => 2, _ => 3 }] += 1;
            scr[s.state.code().clamp(0, 4) as usize] += 1;
            door_open += (s.reason == "structure") as usize;
            for (qi, ti) in [(i, j), (j, i)] {
                let mut out = vec![0i32; XRANK_FIELDS];
                let t0 = Instant::now();
                xrank(&xs2[qi], &[Some(&xs2[ti])], &cfg, &x2b, &opts, &mut ctx, &mut rs, &mut out);
                let dt = t0.elapsed().as_secs_f64();
                tx2 += dt;
                let e = if out[0] < 0 { 0 } else if out[23] & 8 != 0 { 2 } else { 1 };
                exec2[e].0 += 1;
                exec2[e].1 += dt;
                if e > 0 {
                    // how far the sparse scan went, and what it evaluated
                    all_rows += (out[9] as usize >= sides[qi].kp.len().min(sides[ti].kp.len())) as usize;
                    exact_struct += (out[17] != 0) as usize;
                    hams += out[20] as u64;
                    full += out[21] as u64;
                }
                *xstate.entry((s42, out[0])).or_insert(0) += 1;
                let c2 = out[0] == 3 || out[0] == 4;
                let t0 = Instant::now();
                xrank(&xs1[qi], &[Some(&xs1[ti])], &cfg, &x1b, &opts, &mut ctx, &mut rs, &mut out);
                tx1 += t0.elapsed().as_secs_f64();
                let c1 = out[0] == 3 || out[0] == 4;
                let adm = sq[qi].score(&sigs[ti]) >= si.threshold;
                if is_copy {
                    x2_copy += c2 as usize;
                    x1_copy += c1 as usize;
                    si_adm_copy += adm as usize;
                } else {
                    x2_false += c2 as usize;
                    x1_false += c1 as usize;
                    si_adm += adm as usize;
                }
            }
            if is_copy {
                ncopy += 1;
                copy_pairs.insert((i, j));
                gate42_lost += (!g) as usize;
                let same = works[i].author == works[j].author;
                same_author += same as usize;
                copies.push(format!("{}\t{}/{}\t{}/{}\t{}\t{}\t{}\n", v.verdict, works[i].author, works[i].permlink, works[j].author, works[j].permlink, if same { "same author" } else { "two authors" }, v.class, if g { "gate passes" } else { "gate screens out" }));
            }
        }
    }
    std::fs::write(format!("{src}/copies.tsv"), copies.concat()).unwrap();

    let names = ["Unrelated", "Related", "Suspected", "Copy", "Identical", "Indeterminate"];
    println!("\nEvery pair of distinct works ({np} pairs, {} queries in both arrival orders):\n", 2 * np);
    let same_pairs: usize = st42_same.iter().sum();
    println!("| comparator 42 (lean) | pairs, {} | of them one author's ({same_pairs} such pairs in all) | pairs, {} | of them one author's |", base.name_str(), base4.name_str());
    println!("|---|---:|---:|---:|---:|");
    for k in 0..6 {
        println!("| {} | {} | {} | {} | {} |", names[k], st42[k], st42_same[k], st4[k], st4_same[k]);
    }
    let classes = |w: &std::collections::BTreeMap<String, usize>| -> String {
        if w.is_empty() { "none (no pair is Suspected)".to_string() } else { w.iter().rev().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join("; ") }
    };
    println!("\nComparator 42's class on the Suspected pairs: under {} {}; under {} {}.", base.name_str(), classes(&why), base4.name_str(), classes(&why4));
    println!("\nOf the {ncopy} pairs comparator 42 calls Copy or Identical, {same_author} have one author; XRank shown the target alone reads Copy on {x2_copy} of {} queries under X3 and {x1_copy} under X2; comparator 42's own gated rank screens out {gate42_lost} of the {ncopy}; SI4 admits {si_adm_copy} of the {} queries at θ = {}. (The pairs: {src}/copies.tsv.)", 2 * ncopy, 2 * ncopy, si.threshold);
    let nneg = 2 * (np - ncopy);
    println!("On the {} other queries: XRank reads Copy on {x2_false} under X3 and {x1_false} under X2; SI4 admits {si_adm} ({:.2}%).", nneg, 100.0 * si_adm as f64 / nneg.max(1) as f64);
    println!("The route class under X3: Reject {} ({:.1}%), Defer {} ({:.1}%), Fast {} ({:.1}%), Absent {}; the pair screen: Reject {}, Defer {} ({door_open} of them on the open structural door), Pass {}, Identical {}; comparator 42's stage-1 screen passes {gate42_pass} ({:.1}%).", cls[0], 100.0 * cls[0] as f64 / np as f64, cls[1], 100.0 * cls[1] as f64 / np as f64, cls[2], 100.0 * cls[2] as f64 / np as f64, cls[3], scr[0], scr[1], scr[2], scr[3], 100.0 * gate42_pass as f64 / np as f64);
    println!("Mean cost a pair, native, one core: comparator 42 {:.0} µs; comparator 42's gated rank (`rank`, gate: true — the stage-1 screen, and comparator 42 on the pairs it passes) {:.0} µs a candidate; the pair screen (X3) {:.1} µs; XRank a query, X3 {:.0} µs, X2 {:.0} µs.", 1e6 * t42 / np as f64, 1e6 * trank42 / np as f64, 1e6 * tscr / np as f64, 1e6 * tx2 / (2 * np) as f64, 1e6 * tx1 / (2 * np) as f64);
    let lab = ["gated out", "decided by the cascade", "EXACT42 ran (fallback)"];
    println!("XRank under X3, by how it decided: {}.", (0..3).map(|e| format!("{} {} ({:.0} µs each)", lab[e], exec2[e].0, 1e6 * exec2[e].1 / exec2[e].0.max(1) as f64)).collect::<Vec<_>>().join("; "));
    let ncas = exec2[1].0 + exec2[2].0;
    let sname = |s: i32| -> &'static str { match s { -1 => "gated out", 6 => "NotCopy", 0 => "Unrelated", 1 => "Related", 2 => "Suspected", 3 => "Copy", 4 => "Identical", _ => "Indeterminate" } };
    let by42: Vec<String> = (0..6).filter_map(|s| {
        let row: Vec<String> = xstate.iter().filter(|((a, _), _)| *a == s).map(|((_, x), c)| format!("{} {c}", sname(*x))).collect();
        if row.is_empty() { None } else { Some(format!("{} → {}", names[s as usize], row.join(", "))) }
    }).collect();
    println!("Its states (Copy scope; queries, by comparator 42's state of the pair): {}. Comparator 42's own gated rank screens out, of the pairs it calls Unrelated / Related / Suspected / Copy: {} / {} / {} / {}.", by42.join("; "), gate42_out[0], gate42_out[1], gate42_out[2], gate42_out[3] + gate42_out[4]);
    println!("On the {ncas} queries that reached the cascade (decided there or by EXACT42), the sparse scan read every row of the smaller side on {all_rows} ({:.1}%) and computed {:.1}% of the descriptor distances the exhaustive scan computes ({hams} of {full}); every structural channel was computed exactly on {exact_struct} ({:.1}%).", 100.0 * all_rows as f64 / ncas.max(1) as f64, 100.0 * hams as f64 / full.max(1) as f64, 100.0 * exact_struct as f64 / ncas.max(1) as f64);

    // ---- SI on the real population: how the cells spread, which families
    // carry an unrelated pair's score, what is admitted — 1.1.2's SI3 on
    // these wire-4 hashes, and SI4 (fitted on them: in-sample, the held-out
    // numbers are `sibench chainfit`'s)
    use paph::x::si::code::coarse;
    let fam_names = ["runs", "tone", "pal", "shape", "sil", "kpgeo", "local", "band"];
    for (prof, label, xbb) in [(SiProfile::si3(), "SI3, 1.1.2's chain fit on wire-3 hashes (bound to X2)", &x1b), (SiProfile::shipped(), "SI4, fitted on these works' wire-4 hashes (in-sample; bound to X3)", &x2b)] {
        let sigs: Vec<SiSig> = (0..n).map(|i| SiSig::from_prepared(&sides[i], xbb, &prof)).collect();
        let sq: Vec<SiQuery> = (0..n).map(|i| SiQuery::from_prepared(&sides[i], xbb, &prof)).collect();
        println!("\n{label}: its cells over the {n} works (fine 256 / coarse 16):\n");
        println!("| family | present | occupied fine | effective fine cells | largest fine cell | effective coarse cells | P(random pair shares a fine cell) |");
        println!("|---|---:|---:|---:|---:|---:|---:|");
        for f in 0..FAMILIES {
            let (mut h, mut hc, mut m) = ([0u64; CELLS], [0u64; COARSE], 0u64);
            for s in sigs.iter() {
                if s.present >> f & 1 != 0 {
                    h[s.cells[f] as usize] += 1;
                    hc[coarse(s.cells[f]) as usize] += 1;
                    m += 1;
                }
            }
            let ent = |h: &[u64]| -> f64 { h.iter().filter(|&&x| x > 0).map(|&x| { let p = x as f64 / m as f64; -p * p.ln() }).sum::<f64>().exp() };
            let coll: f64 = h.iter().map(|&x| (x as f64 / m.max(1) as f64).powi(2)).sum();
            println!("| {} | {:.0}% | {} | {:.0} | {:.1}% | {:.1} | 1/{:.0} |", FAMILY_NAMES[f], 100.0 * m as f64 / n as f64, h.iter().filter(|&&x| x > 0).count(), ent(&h), 100.0 * *h.iter().max().unwrap() as f64 / m.max(1) as f64, ent(&hc), 1.0 / coll.max(1e-9));
        }
        let (mut reach, mut wsum) = ([0u64; ALL_FAMILIES], [0i64; ALL_FAMILIES]);
        let mut scores: Vec<f64> = Vec::new();
        let mut copy_adm = 0usize;
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                if copy_pairs.contains(&(i.min(j), i.max(j))) {
                    copy_adm += (sq[i].score(&sigs[j]) >= prof.threshold) as usize;
                    continue;
                }
                let l = sq[i].levels(&sigs[j]);
                for f in 0..ALL_FAMILIES {
                    if let Some(v) = l[f] {
                        if v > 0 {
                            reach[f] += 1;
                        }
                        wsum[f] += (if f < FAMILIES { sq[i].wv[f][v] } else if f == FAMILY_LOCAL { sq[i].wl[v] } else { sq[i].wb[v] }) as i64;
                    }
                }
                scores.push(sq[i].score(&sigs[j]) as f64);
            }
        }
        let nq = scores.len().max(1) as f64;
        println!("\nOn the {} unrelated queries (ordered pairs, copies left out), per family: the share whose candidate reaches a probe level (a probed cell; for the MinHash families, enough equal lanes), and the mean weight it adds to the score.\n", scores.len());
        println!("| family | reaches a level | mean weight |");
        println!("|---|---:|---:|");
        for f in 0..ALL_FAMILIES {
            println!("| {} | {:.1}% | {:+.1} |", fam_names[f], 100.0 * reach[f] as f64 / nq, wsum[f] as f64 / nq);
        }
        let adm = |t: f64| 100.0 * scores.iter().filter(|&&s| s >= t).count() as f64 / nq;
        let mut sc = scores.clone();
        println!("\nTheir scores: p50 {:.0}, p90 {:.0}, p99 {:.0}, max {:.0}; admitted at the profile's θ = {}: {:.2}%, at θ = 32 / 64 / 96: {:.1}% / {:.1}% / {:.1}%. The copy queries admitted at θ: {copy_adm} of {}.", pct(&mut sc, 0.5), pct(&mut sc, 0.9), pct(&mut sc, 0.99), pct(&mut sc, 1.0), prof.threshold, adm(prof.threshold as f64), adm(32.0), adm(64.0), adm(96.0), 2 * copy_pairs.len());
    }
}

/// The pairs of distinct originals comparator 42 calls Copy (or Identical),
/// as (record, record) with the smaller first: on the chain's corpus, works
/// that are copies of each other before any transform — never unrelated.
fn originals_copies42(c: &Corpus) -> std::collections::HashSet<(usize, usize)> {
    // Copy and Identical read the same under every comparator-42 calibration
    // this crate ships (CAL-007 moved a Suspected bar only)
    let base = Profile::shipped();
    let bcfg = paph::v4::bind(&Config::default(), &base);
    let mut copies42 = std::collections::HashSet::new();
    for a in 0..c.nb {
        for b in a + 1..c.nb {
            let (i, j) = (c.at[&(a, 0)], c.at[&(b, 0)]);
            let (pa, pb) = (&c.sides[i], &c.sides[j]);
            let swapped = canon_swapped(pa, pb);
            let (ca, cb) = if swapped { (pb, pa) } else { (pa, pb) };
            let v = state_code(compare_in(&mut PairCtx::new(ca, cb), swapped, &bcfg, &base, Reading::Lean).base.verdict);
            if v == 3 || v == 4 {
                copies42.insert((i.min(j), i.max(j)));
            }
        }
    }
    copies42
}

/// A family whose codebook would rest on fewer background works than this
/// keeps SI2's (on the chain: SIL, which only works with transparency carry).
const CHAIN_MIN_BOOK: usize = 40;

/// Copy pairs of the given bases, both arrival orders, (query, target).
fn pairs_of(c: &Corpus, bases: &[i32], keep: &dyn Fn(i32, i32) -> bool) -> Vec<(usize, usize)> {
    let mut v = Vec::new();
    for &b in bases {
        let i = c.at[&(b, 0)];
        for t in 1..TRANSFORMS.len() as i32 {
            if keep(b, t) {
                let j = c.at[&(b, t)];
                v.push((j, i));
                v.push((i, j));
            }
        }
    }
    v
}

/// An SI profile fitted on the chain's works: the background is the given
/// bases' originals, the noise and evidence their copies — what `sibench
/// fit` does with the synthetic population, on real works.  `copies42`: the
/// pairs of distinct originals comparator 42 calls Copy.  Returns the
/// profile and the families that kept SI2's codebook.
fn chain_profile(c: &Corpus, mh: &[SiSig], train: &[i32], name: &str, xb: &XBound, copies42: &std::collections::HashSet<(usize, usize)>) -> (SiProfile, Vec<&'static str>) {
    let background: Vec<usize> = train.iter().map(|&b| c.at[&(b, 0)]).collect();
    let paste = TRANSFORMS.iter().position(|&t| t == "paste").unwrap() as i32;
    let noise = pairs_of(c, train, &|_, t| t != paste);
    let evidence = pairs_of(c, train, &|b, t| t != paste && matches!(c.verdict.get(&(b, t)), Some(2) | Some(3) | Some(4)));
    // the background is the originals themselves, so a random draw can land
    // on a copy's own original, or on an original comparator 42 calls a copy
    // of the query's: neither is a random pair
    let orig = |i: usize| c.at[&(c.recs[i].base, 0)];
    let related = |q: usize, t: usize| -> bool {
        let (oq, ot) = (orig(q), orig(t));
        oq == ot || copies42.contains(&(oq.min(ot), oq.max(ot)))
    };
    let inp = FitInput { fam: &c.fam, mh, background: &background, noise_pairs: &noise, evidence_pairs: &evidence, related: Some(&related) };
    let mut books = fit_books(&inp);
    let si2 = SiProfile::si2();
    let mut kept = Vec::new();
    for f in 0..FAMILIES {
        if background_count(&inp, f) < CHAIN_MIN_BOOK {
            books[f] = si2.book[f].clone();
            kept.push(FAMILY_NAMES[f]);
        }
    }
    let mut nm = [0u8; 16];
    nm[..name.len()].copy_from_slice(name.as_bytes());
    let o = FitOptions { name: nm, xid: xb.xid, probes: 4, random_per_query: 400, seed: 0x5349_3350_524f_5631, admit_ppm: 10_000, budget: 2000 };
    (fit_on_books(books, &inp, &o), kept)
}

/// What a profile does on held-out real works: its copies against their
/// originals (both orders) and every ordered pair of distinct originals
/// comparator 42 does not call Copy.
struct SiHeld {
    copy: Vec<i32>,
    /// the transform of each copy query
    tf: Vec<usize>,
    unrel: Vec<i32>,
    reach_copy: [u64; ALL_FAMILIES],
    reach_unrel: [u64; ALL_FAMILIES],
}

fn si_heldout(c: &Corpus, prof: &SiProfile, test: &[i32], copies42: &std::collections::HashSet<(usize, usize)>) -> SiHeld {
    let sig = |i: usize| SiSig::from_parts(&c.fam[i], &c.routes[i], prof);
    let mut h = SiHeld { copy: Vec::new(), tf: Vec::new(), unrel: Vec::new(), reach_copy: [0; ALL_FAMILIES], reach_unrel: [0; ALL_FAMILIES] };
    let score = |q: usize, t: usize, reach: &mut [u64; ALL_FAMILIES]| -> i32 {
        let qq = SiQuery::from_parts(sig(q), &c.fam[q], prof);
        let st = sig(t);
        for (f, l) in qq.levels(&st).iter().enumerate() {
            if matches!(l, Some(v) if *v > 0) {
                reach[f] += 1;
            }
        }
        if qq.touches(&st) { qq.score(&st) } else { i32::MIN }
    };
    for (q, t) in pairs_of(c, test, &|b, t| c.copy(b, t)) {
        let s = score(q, t, &mut h.reach_copy);
        h.copy.push(s);
        h.tf.push(c.recs[if c.recs[q].tf > 0 { q } else { t }].tf as usize);
    }
    let orig: Vec<usize> = test.iter().map(|&b| c.at[&(b, 0)]).collect();
    for &i in orig.iter() {
        for &j in orig.iter() {
            if i != j && !copies42.contains(&(i.min(j), i.max(j))) {
                let s = score(i, j, &mut h.reach_unrel);
                h.unrel.push(s);
            }
        }
    }
    h
}

/// `sibench chainfit [--out PATH]` on the chain's corpus (`--corpus`,
/// default chain.bin): SI fitted on real works.  Two folds (the corpus' own
/// split) fit on one half and are measured on the other against SI2; then
/// the fit on every base, written under `name` — SI4-PROVISIONAL under X3 (on
/// the wire-4 hashes), SI3-PROVISIONAL under X2 (1.1.2's, on wire-3 hashes).
fn cmd_chainfit(c: &Corpus, xb: &XBound, out: &str, name: &str) {
    assert!(!name.is_empty() && name.len() <= 16, "--name: 1 to 16 bytes");
    let mh: Vec<SiSig> = c.routes.iter().map(SiSig::minhash).collect();
    let fold_a: Vec<i32> = (0..c.nb).filter(|&b| !is_eval_base(b)).collect();
    let fold_b: Vec<i32> = (0..c.nb).filter(|&b| is_eval_base(b)).collect();
    let all: Vec<i32> = (0..c.nb).collect();
    // comparator 42 between distinct originals: the pairs that are not unrelated
    let copies42 = originals_copies42(c);
    println!("PAPH-SI fitted on the chain's works — {} real bases ({} and {} in the two folds), {} pairs of distinct originals comparator 42 calls Copy left out of the unrelated ones.\n", c.nb, fold_a.len(), fold_b.len(), copies42.len());
    let si2 = SiProfile::si2();
    let rate = |v: &[i32], th: i32| v.iter().filter(|&&s| s >= th).count() as f64 / v.len().max(1) as f64;
    // the recall at the threshold that admits a given share of unrelated pairs
    let at_share = |h: &SiHeld, share: f64| -> (i32, f64) {
        let mut u = h.unrel.clone();
        u.sort_unstable_by(|a, b| b.cmp(a));
        let k = ((u.len() as f64 * share) as usize).min(u.len().saturating_sub(1));
        let th = u[k].saturating_add(1).max(1);
        (th, rate(&h.copy, th))
    };
    let names = ["runs", "tone", "pal", "shape", "sil", "kpgeo", "local", "band"];
    println!("| fitted on → measured on | profile | θ | copies admitted at θ | unrelated admitted at θ | at θ = 1: copies / unrelated | recall with 1% / 5% of unrelated admitted |");
    println!("|---|---|---:|---:|---:|---:|---:|");
    let mut reach_rows: Vec<String> = Vec::new();
    let mut per_tf: Vec<(String, Vec<(usize, usize)>)> = Vec::new();
    for (train, test, label) in [(&fold_a, &fold_b, "fold A → fold B"), (&fold_b, &fold_a, "fold B → fold A")] {
        let (p3, kept) = chain_profile(c, &mh, train, name, xb, &copies42);
        for (p, nm) in [(&si2, "SI2 (synthetic)"), (&p3, "chain fit")] {
            let h = si_heldout(c, p, test, &copies42);
            let (t1, r1) = at_share(&h, 0.01);
            let (t5, r5) = at_share(&h, 0.05);
            println!("| {label} | {nm} | {} | {:.1}% of {} | {:.2}% of {} | {:.1}% / {:.2}% | {:.1}% (θ {t1}) / {:.1}% (θ {t5}) |", p.threshold, 100.0 * rate(&h.copy, p.threshold), h.copy.len(), 100.0 * rate(&h.unrel, p.threshold), h.unrel.len(), 100.0 * rate(&h.copy, 1), 100.0 * rate(&h.unrel, 1), 100.0 * r1, 100.0 * r5);
            let mut v = vec![(0usize, 0usize); TRANSFORMS.len()];
            for (k, &sc) in h.copy.iter().enumerate() {
                v[h.tf[k]].0 += 1;
                v[h.tf[k]].1 += (sc >= p.threshold) as usize;
            }
            match per_tf.iter_mut().find(|r| r.0 == nm) {
                Some(r) => {
                    for t in 0..v.len() {
                        r.1[t].0 += v[t].0;
                        r.1[t].1 += v[t].1;
                    }
                }
                None => per_tf.push((nm.to_string(), v)),
            }
            reach_rows.push(format!("| {label} | {nm} | {} |", (0..ALL_FAMILIES).map(|f| format!("{:.0} / {:.0}", 100.0 * h.reach_copy[f] as f64 / h.copy.len().max(1) as f64, 100.0 * h.reach_unrel[f] as f64 / h.unrel.len().max(1) as f64)).collect::<Vec<_>>().join(" | ")));
        }
        if !kept.is_empty() {
            println!("|  | ({} kept SI2's codebook: under {CHAIN_MIN_BOOK} works in the fold carry it) | | | | |", kept.join(", "));
        }
    }
    println!("\nCopies admitted at each profile's own θ, both folds together, by transform:\n");
    println!("| transform | {} |", per_tf.iter().map(|r| r.0.clone()).collect::<Vec<_>>().join(" | "));
    println!("|---|{}", "---:|".repeat(per_tf.len()));
    for t in 1..TRANSFORMS.len() {
        if per_tf[0].1[t].0 > 0 {
            println!("| {} | {} |", TRANSFORMS[t], per_tf.iter().map(|r| format!("{:.0}% of {}", 100.0 * r.1[t].1 as f64 / r.1[t].0 as f64, r.1[t].0)).collect::<Vec<_>>().join(" | "));
        }
    }
    println!("\nPer family, the share of copy queries / unrelated queries whose candidate reaches a probe level (%):\n");
    println!("| | | {} |", names.join(" | "));
    println!("|---|---|{}", "---:|".repeat(ALL_FAMILIES));
    for r in reach_rows {
        println!("{r}");
    }
    let (p3, kept) = chain_profile(c, &mh, &all, name, xb, &copies42);
    let b = p3.encode();
    let wrote = write_artefact(out, &b);
    println!("\nfitted {} on all {} bases' originals ({} kept SI2's codebook): {} bytes, id {} → {}", p3.name_str(), c.nb, if kept.is_empty() { "none".to_string() } else { kept.join(", ") }, b.len(), paph::sha256::hex(&p3.id()), if wrote { out.to_string() } else { format!("not written ({out} differs)") });
    println!("  probes {}  threshold {}  budget {}", p3.probes, p3.threshold, p3.budget);
    println!("  | family | weights none / near / exact | axis noise σ (projection units) |");
    for f in 0..FAMILIES {
        let cb = &p3.book[f];
        println!("  | {} | {} / {} / {} | {:?} |", FAMILY_NAMES[f], cb.w[0], cb.w[1], cb.w[2], cb.sig);
    }
    println!("  | local | {:?} (0 / 1 / 2–3 / ≥4 equal band keys) | |", p3.w_local);
    println!("  | band | {:?} | |", p3.w_band);
}

/// `sibench doorprof`: where the structural door shuts on unrelated pairs
/// (each eval base against 60 eval distractors), what each step costs, and
/// the cheapest orders of the steps on those pairs.
fn cmd_doorprof(c: &Corpus, xb: &XBound, real: bool) {
    use paph::x::compare::{door_step, DOOR_ORDER, DOOR_LOCAL_BOUND};
    use paph::x::structural::*;
    let base = &xb.base;
    let mut d0 = vec![0u8; 128 * 128];
    let label = |k: usize| -> &'static str { if k == DOOR_LOCAL_BOUND || k == 7 { "local bound" } else { CH_NAMES[k] } };
    // the sides once, the pairs as indices into them, canonical order
    let mut xs: Vec<XPrepared> = Vec::new();
    let mut idx: HashMap<usize, usize> = HashMap::new();
    let mut side = |i: usize, xs: &mut Vec<XPrepared>| -> usize {
        *idx.entry(i).or_insert_with(|| {
            xs.push(XPrepared::new(Prepared::new(&c.recs[i].t1, Some(&c.recs[i].t2)).unwrap(), xb));
            xs.len() - 1
        })
    };
    let mut raw: Vec<(usize, usize)> = Vec::new();
    let mut left_out = 0usize;
    if real {
        // --real (the chain's corpus): every pair of distinct originals but
        // those comparator 42 calls copies of each other
        let copies42 = originals_copies42(c);
        left_out = copies42.len();
        for a in 0..c.nb {
            for b in a + 1..c.nb {
                let (i, j) = (c.at[&(a, 0)], c.at[&(b, 0)]);
                if !copies42.contains(&(i.min(j), i.max(j))) {
                    raw.push((side(i, &mut xs), side(j, &mut xs)));
                }
            }
        }
    } else {
        for b in 0..c.nb {
            if !is_eval_base(b) {
                continue;
            }
            let i = c.at[&(b, 0)];
            for k in 0..60 {
                let d = c.ev_d[(b as usize * 7919 + k * 104_729) % c.ev_d.len()];
                raw.push((side(i, &mut xs), side(d, &mut xs)));
            }
        }
    }
    let pairs: Vec<(&XPrepared, &XPrepared)> = raw.iter().map(|&(q, x)| if canon_swapped(&xs[q].p, &xs[x].p) { (&xs[x], &xs[q]) } else { (&xs[q], &xs[x]) }).collect();
    let mut at = [0usize; 10];
    let t0 = Instant::now();
    for &(a, b) in pairs.iter() {
        at[door_step(a, b, base, &mut d0).1] += 1;
    }
    let us = t0.elapsed().as_secs_f64() * 1e6 / pairs.len() as f64;
    let whose = if real { format!(" (every pair of distinct bases but the {left_out} comparator 42 calls Copy)") } else { String::new() };
    println!("The structural door on {} unrelated pairs{whose}: {:.1} µs a pair natively, open on {}.", pairs.len(), us, at[9]);
    println!("  shut before any channel: not certifiable {}, the bar out of reach {}", at[0], at[1]);
    for (k, &st) in DOOR_ORDER.iter().enumerate() {
        println!("  shut after {:<12} {}", label(st), at[2 + k]);
    }
    // every step on every certifiable pair: per-call cost, and every order
    let mut rows: Vec<([i64; 7], [bool; 7], i64, i64, [f64; 8])> = Vec::new();
    for &(a, b) in pairs.iter() {
        let mut s = Structural::new(a, b, base);
        if !s.measurable[CH_LOCAL] || s.secondaries() < base.min_secondaries.max(0) as usize {
            continue;
        }
        let lh0 = s.local_hi;
        let mut tm = [0f64; 8];
        for ch in [CH_DCT, CH_SHAPE, CH_TOPOLOGY, CH_RUNS, CH_PALETTE, CH_SILHOUETTE] {
            let tc = Instant::now();
            s.compute(ch, a, b, base, &mut d0);
            tm[ch] = tc.elapsed().as_secs_f64() * 1e6;
        }
        let tc = Instant::now();
        s.bound_local(a, b, base, &mut d0);
        tm[7] = tc.elapsed().as_secs_f64() * 1e6;
        rows.push((s.value, s.measurable, lh0, s.local_hi, tm));
    }
    let n = rows.len() as f64;
    let mean = |k: usize| rows.iter().map(|r| r.4[k]).sum::<f64>() / n;
    println!("Per call, over the {} certifiable pairs (µs): runs {:.1}, silhouette {:.1}, local bound {:.1}, topology {:.1}, shape {:.1}, dct {:.1}, palette {:.1}.", rows.len(), mean(CH_RUNS), mean(CH_SILHOUETTE), mean(7), mean(CH_TOPOLOGY), mean(CH_SHAPE), mean(CH_DCT), mean(CH_PALETTE));
    let bar = (base.thresholds[1] as i64).max(base.thresholds[4] as i64);
    let w = |k: usize| -> i64 {
        let idx = match k { CH_LOCAL => 0, CH_SHAPE => 1, CH_TOPOLOGY => 2, CH_RUNS => 3, CH_DCT => 4, CH_PALETTE => 5, _ => 6 };
        base.weights[idx] as i64
    };
    let simulate = |ord: &[usize]| -> f64 {
        let mut cost = 0f64;
        for r in rows.iter() {
            let (vals, meas, lh0, lh1, tm) = (r.0, r.1, r.2, r.3, r.4);
            let mut known = [false; 7];
            let mut lhi = lh0;
            let bound = |known: &[bool; 7], lhi: i64| -> i64 {
                let (mut hi, mut wt) = (0i64, 0i64);
                for k in 0..7 {
                    if meas[k] {
                        wt += w(k);
                        hi += w(k) * if k == CH_LOCAL { lhi } else if known[k] { vals[k] } else { 10_000 };
                    }
                }
                if wt == 0 { 0 } else { hi / wt }
            };
            if bound(&known, lhi) < bar {
                continue;
            }
            for &st in ord {
                if st == 7 {
                    cost += tm[7];
                    lhi = lh1;
                } else {
                    cost += tm[st];
                    known[st] = true;
                }
                if bound(&known, lhi) < bar {
                    break;
                }
            }
        }
        cost / n
    };
    let shipped: Vec<usize> = DOOR_ORDER.iter().map(|&k| if k == DOOR_LOCAL_BOUND { 7 } else { k }).collect();
    let cascade = vec![CH_RUNS, CH_PALETTE, CH_SILHOUETTE, CH_TOPOLOGY, 7, CH_SHAPE, CH_DCT];
    println!("Expected cost per certifiable pair: the shipped order {:.1} µs; the cascade's own order {:.1} µs.", simulate(&shipped), simulate(&cascade));
    let mut perm = vec![CH_DCT, CH_SHAPE, CH_TOPOLOGY, CH_RUNS, CH_PALETTE, CH_SILHOUETTE, 7usize];
    let mut all: Vec<(f64, Vec<usize>)> = vec![(simulate(&perm), perm.clone())];
    let np = perm.len();
    let mut cidx = vec![0usize; np];
    let mut i = 0;
    while i < np {
        if cidx[i] < i {
            if i % 2 == 0 {
                perm.swap(0, i);
            } else {
                perm.swap(cidx[i], i);
            }
            all.push((simulate(&perm), perm.clone()));
            cidx[i] += 1;
            i = 0;
        } else {
            cidx[i] = 0;
            i += 1;
        }
    }
    all.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    for (c_, o) in all.iter().take(3) {
        println!("  the cheapest orders: {} — {:.1} µs", o.iter().map(|&k| label(k)).collect::<Vec<_>>().join(", "), c_);
    }
}

// ------------------------------------------------------------- calibration

/// The calibration snapshot of one pair (1.2, CAL-007): every quantity
/// comparator 42's lattice reads, taken BEFORE the profile's decision tables
/// and bars — the six structural channels' raw values, the local channel's
/// chance-corrected margin and diversity, the geometric margin, the geometric
/// diversity reading, the inliers, the topology class — so a candidate
/// profile's verdict on the pair is recomputed without re-measuring it
/// (demo/cal-core.cjs's snapshot, in Rust).  The measurement fields of the
/// profile (hamming_t, confidence and geometry anchors, model floors, grid)
/// stay CAL-004's, and the row ends with CAL-004's verdict, which a
/// recomputation under CAL-004 must reproduce.
pub const CALIB_HEADER: &str = "kind\ta\tb\ttf\tsame\tidentical\tdct\tdct_m\tshape\tshape_m\ttopology\ttopology_m\truns\truns_m\tpalette\tpalette_m\tsilhouette\tsilhouette_m\tlocal_m\tlocal_margin\tlocal_div\tcoverage_min\tgeo_m\tgeo_margin\tnmodels\tdiv_combined\tinliers\ttopo\tmirror\tverdict\tclass\tbasis";

fn calib_row(pa: &Prepared, pb: &Prepared, base: &Profile, bcfg: &Config) -> String {
    let swapped = canon_swapped(pa, pb);
    let (ca, cb) = if swapped { (pb, pa) } else { (pa, pb) };
    let mut pc = PairCtx::new(ca, cb);
    let r = compare_in(&mut pc, swapped, bcfg, base, Reading::Full);
    let b = &r.base;
    let v3 = b.v3.as_ref().expect("full reading");
    let mut s = format!("{}", v3.identical as i32);
    for name in ["dct", "shape", "topology", "runs", "palette", "silhouette"] {
        let ch = &v3.channels.iter().find(|(n, _)| *n == name).unwrap().1;
        s.push_str(&format!("\t{}\t{}", ch.value, ch.measurable as i32));
    }
    let l = b.local.as_ref().unwrap();
    s.push_str(&format!(
        "\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        l.measurable as i32,
        l.margin,
        l.diversity,
        l.coverage_a.coverage.min(l.coverage_b.coverage),
        b.geo_measurable as i32,
        b.geo_margin,
        r.models42.len(),
        r.diversity_geo.combined,
        b.total_inliers,
        b.topology,
        r.models42.iter().any(|m| m.mirror) as i32,
        b.verdict,
        b.class,
        b.basis.join(",")
    ));
    s
}

/// `sibench calib`: snapshots of the chain's pairs and of a corpus's copies
/// and unrelated pairs, as TSV under target/calib/ (`tools/cal-lattice.py`
/// recomputes the lattice on them under candidate profiles).
fn cmd_calib(args: &[String], dir: &str) {
    let out_dir = sarg(args, "--out", "target/calib");
    std::fs::create_dir_all(&out_dir).unwrap();
    let base = Profile::cal004();
    let bcfg = paph::v4::bind(&Config::default(), &base);
    let t0 = Instant::now();
    // the chain's works, every pair
    let src = sarg(args, "--chain", "target/chain-corpus");
    if std::path::Path::new(&format!("{src}/index.tsv")).exists() {
        let (works, _) = load_chain(&src);
        let cfg = Config::default();
        let rot = RotCache::new(&pattern());
        let sides: Vec<Prepared> = works.iter().map(|w| { let f = hash(&w.img.px, w.img.w, w.img.h, &cfg, &rot); Prepared::new(&f.t1, Some(&f.t2)).unwrap() }).collect();
        let mut o = String::from(CALIB_HEADER);
        o.push('\n');
        for i in 0..works.len() {
            for j in i + 1..works.len() {
                o.push_str(&format!("chain\t{i}\t{j}\t-\t{}\t{}\n", (works[i].author == works[j].author) as i32, calib_row(&sides[i], &sides[j], &base, &bcfg)));
            }
        }
        std::fs::write(format!("{out_dir}/chain-pairs.tsv"), o).unwrap();
        eprintln!("calib: {} chain works, every pair ({:.1} s)", works.len(), t0.elapsed().as_secs_f64());
    }
    // the corpora: every base against its twenty transforms, every pair of
    // bases, and each base against 24 distractors of the evaluation half
    for name in ["chain.bin", "corpus.bin"] {
        let path = format!("{dir}/{name}");
        if !std::path::Path::new(&path).exists() {
            continue;
        }
        let recs = load(&path);
        let sides: Vec<Prepared> = recs.iter().map(|r| Prepared::new(&r.t1, Some(&r.t2)).unwrap()).collect();
        let mut at = HashMap::new();
        for (i, r) in recs.iter().enumerate() {
            if r.base >= 0 {
                at.insert((r.base, r.tf), i);
            }
        }
        let nb = recs.iter().map(|r| r.base).max().unwrap_or(-1) + 1;
        let dist: Vec<usize> = (0..recs.len()).filter(|&i| recs[i].base < 0).collect();
        let ev = &dist[dist.len() / 2..];
        let mut o = String::from(CALIB_HEADER);
        o.push('\n');
        for b in 0..nb {
            let i = at[&(b, 0)];
            for t in 1..TRANSFORMS.len() as i32 {
                let j = at[&(b, t)];
                o.push_str(&format!("copy\t{b}\t{b}\t{}\t-\t{}\n", TRANSFORMS[t as usize], calib_row(&sides[i], &sides[j], &base, &bcfg)));
            }
            if name == "corpus.bin" {
                for b2 in b + 1..nb {
                    o.push_str(&format!("bases\t{b}\t{b2}\t-\t-\t{}\n", calib_row(&sides[i], &sides[at[&(b2, 0)]], &base, &bcfg)));
                }
            }
            for k in 0..24 {
                let d = ev[(b as usize * 97 + k * 331) % ev.len()];
                o.push_str(&format!("distractor\t{b}\t{d}\t-\t-\t{}\n", calib_row(&sides[i], &sides[d], &base, &bcfg)));
            }
        }
        let outf = format!("{out_dir}/{}.tsv", if name == "chain.bin" { "realbase" } else { "synth" });
        std::fs::write(&outf, o).unwrap();
        eprintln!("calib: {path} → {outf} ({:.1} s)", t0.elapsed().as_secs_f64());
    }
}

/// `sibench calfit`: CAL-007's moderate structural bar from the chain's
/// works, and what CAL-007 changes against CAL-004 on the chain's pairs, the
/// chain's real-base corpus and the synthetic corpus.  Writes the artefact
/// (docs/calibration/CAL-007-PROVISIONAL.pcal, or `--out`) when the fitted
/// bar is the one `Profile::cal007` holds.
fn cmd_calfit(args: &[String], dir: &str) {
    let (p4, p7) = (Profile::cal004(), Profile::cal007());
    let bcfg = paph::v4::bind(&Config::default(), &p4);
    let src = sarg(args, "--chain", "target/chain-corpus");
    let (works, _) = load_chain(&src);
    let cfg = Config::default();
    let rot = RotCache::new(&pattern());
    let sides: Vec<Prepared> = works.iter().map(|w| { let f = hash(&w.img.px, w.img.w, w.img.h, &cfg, &rot); Prepared::new(&f.t1, Some(&f.t2)).unwrap() }).collect();
    let n = works.len();
    let judge = |pa: &Prepared, pb: &Prepared, p: &Profile| -> (i32, i64, bool, String) {
        let swapped = canon_swapped(pa, pb);
        let (ca, cb) = if swapped { (pb, pa) } else { (pa, pb) };
        let r = compare_in(&mut PairCtx::new(ca, cb), swapped, &bcfg, p, Reading::Lean).base;
        (state_code(r.verdict), r.structural, r.certifiable, r.class)
    };
    // every pair of two authors' works: the structural scores the bar must clear
    let authors: Vec<&str> = works.iter().map(|w| w.author.as_str()).collect();
    let mut cross: Vec<(i64, usize, usize)> = Vec::new();
    let mut st = [[[0usize; 6]; 2]; 2]; // [profile][same author][state]
    for i in 0..n {
        for j in i + 1..n {
            let same = (authors[i] == authors[j]) as usize;
            let a = judge(&sides[i], &sides[j], &p4);
            let b = judge(&sides[i], &sides[j], &p7);
            st[0][same][a.0.clamp(0, 5) as usize] += 1;
            st[1][same][b.0.clamp(0, 5) as usize] += 1;
            if same == 0 && a.2 && a.1 < p4.thresholds[1] as i64 {
                cross.push((a.1, i, j));
            }
        }
    }
    cross.sort_unstable_by(|x, y| y.cmp(x));
    let top = cross.first().map(|c| c.0).unwrap_or(0);
    let bar = (top / 100 + 1) * 100;
    let ncross = st[0][0].iter().sum::<usize>();
    let nsame = st[0][1].iter().sum::<usize>();
    println!("The chain's works — {src} ({}): {n} works by {} authors, {} pairs: {ncross} of two authors' works, {nsame} of one author's.", chain_snapshot(&src), authors.iter().collect::<std::collections::BTreeSet<_>>().len(), ncross + nsame);
    println!("\nThe highest structural scores of certifiable pairs of two authors' works (below the strong bar {}): {}.", p4.thresholds[1], cross.iter().take(12).map(|c| c.0.to_string()).collect::<Vec<_>>().join(", "));
    println!("The fitted moderate structural bar: the next multiple of 100 above {top}: {bar} (CAL-004: {}; Profile::cal007: {}).", p4.thresholds[2], p7.thresholds[2]);
    // the curve, and the two author folds
    let above = |t: i64, v: &[(i64, usize, usize)]| v.iter().filter(|c| c.0 >= t).count();
    println!("\n| moderate bar | pairs of two authors' works at or above it, certifiable | share of {ncross} |");
    println!("|---:|---:|---:|");
    for t in [2400i64, 2600, 2800, 3000, 3200, 3300, 3400] {
        println!("| {t} | {} | {:.3} % |", above(t, &cross), 100.0 * above(t, &cross) as f64 / ncross.max(1) as f64);
    }
    let fold = |a: &str| -> usize { (paph::sha256::sha256(a.as_bytes())[0] & 1) as usize };
    for f in 0..2usize {
        let tr: Vec<(i64, usize, usize)> = cross.iter().copied().filter(|c| fold(authors[c.1]) == f && fold(authors[c.2]) == f).collect();
        let te: Vec<(i64, usize, usize)> = cross.iter().copied().filter(|c| fold(authors[c.1]) != f && fold(authors[c.2]) != f).collect();
        let fb = (tr.first().map(|c| c.0).unwrap_or(0) / 100 + 1) * 100;
        let ntr: usize = (0..n).flat_map(|i| (i + 1..n).map(move |j| (i, j))).filter(|&(i, j)| authors[i] != authors[j] && fold(authors[i]) == f && fold(authors[j]) == f).count();
        let nte: usize = (0..n).flat_map(|i| (i + 1..n).map(move |j| (i, j))).filter(|&(i, j)| authors[i] != authors[j] && fold(authors[i]) != f && fold(authors[j]) != f).count();
        println!("Author fold {f} alone ({ntr} pairs of two of its authors' works) fits {fb}; of the other fold's {nte} such pairs, {} reach it.", above(fb, &te));
    }
    let names = ["Unrelated", "Related", "Suspected", "Copy", "Identical", "Indeterminate"];
    println!("\n| the chain's pairs | two authors: CAL-004 | CAL-007 | one author: CAL-004 | CAL-007 |");
    println!("|---|---:|---:|---:|---:|");
    for k in 0..6 {
        if st[0][0][k] + st[1][0][k] + st[0][1][k] + st[1][1][k] > 0 {
            println!("| {} | {} | {} | {} | {} |", names[k], st[0][0][k], st[1][0][k], st[0][1][k], st[1][1][k]);
        }
    }
    // the corpora: every transform of every base, and the negatives
    for (name, label) in [("chain.bin", "the chain's real-base corpus (each work under the twenty transforms)"), ("corpus.bin", "the synthetic corpus")] {
        let path = format!("{dir}/{name}");
        if !std::path::Path::new(&path).exists() {
            continue;
        }
        let recs = load(&path);
        let cs: Vec<Prepared> = recs.iter().map(|r| Prepared::new(&r.t1, Some(&r.t2)).unwrap()).collect();
        let mut at = HashMap::new();
        for (i, r) in recs.iter().enumerate() {
            if r.base >= 0 {
                at.insert((r.base, r.tf), i);
            }
        }
        let nb = recs.iter().map(|r| r.base).max().unwrap_or(-1) + 1;
        let dist: Vec<usize> = (0..recs.len()).filter(|&i| recs[i].base < 0).collect();
        let ev = &dist[dist.len() / 2..];
        println!("\nOn {label}: comparator 42's states of each base against each transform of it (Copy includes Identical), CAL-004 → CAL-007.\n");
        println!("| transform | Copy | Suspected | Related | Unrelated |");
        println!("|---|---:|---:|---:|---:|");
        let mut tot = [[0usize; 6]; 2];
        let mut demoted = 0usize;
        for t in 1..TRANSFORMS.len() as i32 {
            let mut c = [[0usize; 6]; 2];
            for b in 0..nb {
                let (i, j) = (at[&(b, 0)], at[&(b, t)]);
                let x = judge(&cs[i], &cs[j], &p4).0.clamp(0, 5) as usize;
                let y = judge(&cs[i], &cs[j], &p7).0.clamp(0, 5) as usize;
                c[0][x] += 1;
                c[1][y] += 1;
                demoted += (x != y) as usize;
            }
            let f = |k: usize| -> String { if c[0][k] == c[1][k] { c[0][k].to_string() } else { format!("{} → {}", c[0][k], c[1][k]) } };
            let cp = |p: usize| c[p][3] + c[p][4];
            let copy = if cp(0) == cp(1) { cp(0).to_string() } else { format!("{} → {}", cp(0), cp(1)) };
            println!("| {} | {copy} | {} | {} | {} |", TRANSFORMS[t as usize], f(2), f(1), f(0));
            for p in 0..2 {
                for k in 0..6 {
                    tot[p][k] += c[p][k];
                }
            }
        }
        let ge = |p: usize| tot[p][2] + tot[p][3] + tot[p][4];
        println!("\nAll {} transformed copies: Copy {} → {}; Suspected or above {} → {} ({demoted} pairs change state, every one Suspected → Related).", tot[0].iter().sum::<usize>(), tot[0][3] + tot[0][4], tot[1][3] + tot[1][4], ge(0), ge(1));
        // negatives: every pair of bases (synthetic: same-generator works) and
        // each base against 24 evaluation distractors
        let mut neg = [[0usize; 6]; 2];
        let mut nn = 0usize;
        for b in 0..nb {
            let i = at[&(b, 0)];
            let mut others: Vec<usize> = (0..24).map(|k| ev[(b as usize * 97 + k * 331) % ev.len()]).collect();
            if name == "corpus.bin" {
                others.extend((b + 1..nb).map(|b2| at[&(b2, 0)]));
            }
            for &j in others.iter() {
                neg[0][judge(&cs[i], &cs[j], &p4).0.clamp(0, 5) as usize] += 1;
                neg[1][judge(&cs[i], &cs[j], &p7).0.clamp(0, 5) as usize] += 1;
                nn += 1;
            }
        }
        println!("Unrelated pairs ({nn}: each base against 24 distractors{}): Suspected {} → {}, Copy {} → {}, Related {} → {}.", if name == "corpus.bin" { " and every other base" } else { "" }, neg[0][2], neg[1][2], neg[0][3] + neg[0][4], neg[1][3] + neg[1][4], neg[0][1], neg[1][1]);
    }
    if bar == p7.thresholds[2] as i64 {
        let out = sarg(args, "--out", "../docs/calibration/CAL-007-PROVISIONAL.pcal");
        write_artefact(&out, &p7.encode());
        println!("\nwrote {out}: CAL-007-PROVISIONAL, {} bytes, calibration_profile_id {}", p7.encode().len(), paph::sha256::hex(&p7.id()));
    } else {
        println!("\nthe fitted bar {bar} is not CAL-007's {}: a fit on another snapshot is another profile — nothing written", p7.thresholds[2]);
    }
}

/// `sibench xtrace`: the cascade's geometry tier by tier (`XCtx::trace`) on
/// every query between the chain's works and on every copy query of the
/// real-base and synthetic corpora, under X2's schedule bound to CAL-007, with
/// XRank's state and comparator 42's: what an exit from the anchor expansion
/// would have to know.  TSV under target/calib/.
fn cmd_xtrace(args: &[String], dir: &str) {
    let base = Profile::cal007();
    let xb = XBound::new(base.clone(), XProfile::x2_for(&base));
    let bcfg = paph::v4::bind(&Config::default(), &base);
    let cfg = Config::default();
    let out_dir = sarg(args, "--out", "target/calib");
    std::fs::create_dir_all(&out_dir).unwrap();
    let (mut ctx, mut rs) = (XCtx::new(), RankScratch::new());
    let opts = XRankOptions::default();
    let mut o = String::from("kind\tq\tt\ttf\tx_state\tx_exec\tc42\trows\troute_local\troute_band\troute_global\troute_class\ttrace\n");
    // (kind, XRank's state, the trace) of every query, for the summary
    let mut all: Vec<(String, i32, Vec<[i64; 7]>)> = Vec::new();
    let mut run = |kind: &str, tf: &str, q: &XPrepared, t: &XPrepared, qi: usize, ti: usize, c42: i32, o: &mut String| {
        let mut out = vec![0i32; XRANK_FIELDS];
        ctx.trace.clear();
        xrank(q, &[Some(t)], &cfg, &xb, &opts, &mut ctx, &mut rs, &mut out);
        all.push((kind.to_string(), out[0], ctx.trace.clone()));
        let tr: Vec<String> = ctx.trace.iter().map(|e| e.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")).collect();
        o.push_str(&format!("{kind}\t{qi}\t{ti}\t{tf}\t{}\t{}\t{c42}\t{}\t{}\t{}\t{}\t{}\t{}\n", out[0], out[1], out[9], out[3], out[4], out[5], out[6], tr.join(";")));
    };
    let c42 = |pa: &Prepared, pb: &Prepared| -> i32 {
        let swapped = canon_swapped(pa, pb);
        let (ca, cb) = if swapped { (pb, pa) } else { (pa, pb) };
        state_code(compare_in(&mut PairCtx::new(ca, cb), swapped, &bcfg, &base, Reading::Lean).base.verdict)
    };
    let t0 = Instant::now();
    let src = sarg(args, "--chain", "target/chain-corpus");
    let (works, _) = load_chain(&src);
    let rot = RotCache::new(&pattern());
    let fps: Vec<_> = works.iter().map(|w| hash(&w.img.px, w.img.w, w.img.h, &cfg, &rot)).collect();
    let sides: Vec<Prepared> = fps.iter().map(|f| Prepared::new(&f.t1, Some(&f.t2)).unwrap()).collect();
    let xs: Vec<XPrepared> = fps.iter().map(|f| XPrepared::new(Prepared::new(&f.t1, Some(&f.t2)).unwrap(), &xb)).collect();
    for i in 0..works.len() {
        for j in i + 1..works.len() {
            let s = c42(&sides[i], &sides[j]);
            let kind = if works[i].author == works[j].author { "chain-same" } else { "chain" };
            run(kind, "-", &xs[i], &xs[j], i, j, s, &mut o);
            run(kind, "-", &xs[j], &xs[i], j, i, s, &mut o);
        }
    }
    eprintln!("xtrace: chain pairs ({:.1} s)", t0.elapsed().as_secs_f64());
    for name in ["chain.bin", "corpus.bin"] {
        let path = format!("{dir}/{name}");
        if !std::path::Path::new(&path).exists() {
            continue;
        }
        let recs = load(&path);
        let mut at = HashMap::new();
        for (i, r) in recs.iter().enumerate() {
            if r.base >= 0 {
                at.insert((r.base, r.tf), i);
            }
        }
        let nb = recs.iter().map(|r| r.base).max().unwrap_or(-1) + 1;
        let kind = if name == "chain.bin" { "realbase" } else { "synth" };
        for b in 0..nb {
            let i = at[&(b, 0)];
            let pi = Prepared::new(&recs[i].t1, Some(&recs[i].t2)).unwrap();
            let xi = XPrepared::new(Prepared::new(&recs[i].t1, Some(&recs[i].t2)).unwrap(), &xb);
            for t in 1..TRANSFORMS.len() as i32 {
                let j = at[&(b, t)];
                let pj = Prepared::new(&recs[j].t1, Some(&recs[j].t2)).unwrap();
                let xj = XPrepared::new(Prepared::new(&recs[j].t1, Some(&recs[j].t2)).unwrap(), &xb);
                let s = c42(&pi, &pj);
                run(kind, TRANSFORMS[t as usize], &xj, &xi, j, i, s, &mut o);
                run(kind, TRANSFORMS[t as usize], &xi, &xj, i, j, s, &mut o);
            }
        }
        eprintln!("xtrace: {path} ({:.1} s)", t0.elapsed().as_secs_f64());
    }
    std::fs::write(format!("{out_dir}/xtrace.tsv"), o).unwrap();
    // the summary: where the geometric evidence appears, and what an exit
    // from the expansion after a flat tier would cost
    let model = |e: &[i64; 7]| e[4] > 0;
    for kind in ["chain", "realbase", "synth"] {
        let q: Vec<&(String, i32, Vec<[i64; 7]>)> = all.iter().filter(|r| r.0 == kind && !r.2.is_empty()).collect();
        let late = q.iter().filter(|r| !model(&r.2[0]) && r.2.iter().any(model)).count();
        let late0 = q.iter().filter(|r| !model(&r.2[0]) && r.2[0][6] == 0 && r.2.iter().any(model)).count();
        let anym = q.iter().filter(|r| r.2.iter().any(model)).count();
        let full = q.iter().filter(|r| r.2.len() == 4).count();
        println!("{kind}: {} queries reached the cascade's geometry; {anym} found a model at some tier, {late} only after the anchor tier ({late0} with no weak signal there either); {full} measured all four tiers.", q.len());
    }
    println!("\nAn exit after anchor tier k when no tier so far found a model and the weak signal stayed at or below w: the queries between two authors' works it stops, the rows it saves them, and the copy queries (XRank Suspected or Copy) whose model or weak signal appears only on a later tier.\n");
    println!("| exit after | w | unrelated stopped | rows saved a query | copies at risk |");
    println!("|---|---:|---:|---:|---:|");
    for k in 0..3usize {
        for w in [0i64, 3, 5] {
            let (mut stopped, mut saved, mut nun, mut risk) = (0usize, 0i64, 0usize, 0usize);
            for r in all.iter() {
                if r.2.is_empty() {
                    continue;
                }
                if r.0 == "chain" {
                    nun += 1;
                }
                if r.2.len() <= k + 1 {
                    continue;
                }
                let flat = r.2[..=k].iter().all(|e| !model(e) && e[6] <= w);
                if !flat {
                    continue;
                }
                let later = r.2[k + 1..].iter().any(|e| model(e) || e[6] > w);
                if r.0 == "chain" {
                    stopped += 1;
                    saved += r.2[r.2.len() - 1][0] - r.2[k][0];
                } else if (r.0 == "realbase" || r.0 == "synth") && later && (2..=4).contains(&r.1) {
                    risk += 1;
                }
            }
            println!("| tier {} ({} rows) | {w} | {stopped} of {nun} | {:.0} | {risk} |", k, xb.xp.anchors[k], saved as f64 / nun.max(1) as f64);
        }
    }
}

/// `sibench xtime`: where XRank's time goes on the chain's unrelated pairs —
/// the whole query, the sparse scan of every row, the pools and one
/// measurement on every row, one measurement per anchor tier, and the
/// structural channels — under X2's schedule bound to CAL-004 and to CAL-007.
fn cmd_xtime(args: &[String]) {
    use paph::x::geom::{measure, Frames};
    use paph::x::structural::Structural;
    let src = sarg(args, "--chain", "target/chain-corpus");
    let (works, _) = load_chain(&src);
    let cfg = Config::default();
    let rot = RotCache::new(&pattern());
    let fps: Vec<_> = works.iter().map(|w| hash(&w.img.px, w.img.w, w.img.h, &cfg, &rot)).collect();
    let n = works.len();
    let step = arg(args, "--step", 3);
    for base in [Profile::cal004(), Profile::cal007()] {
        let xb = XBound::new(base.clone(), XProfile::x2_for(&base));
        let bcfg = xb.bind(&cfg);
        let xs: Vec<XPrepared> = fps.iter().map(|f| XPrepared::new(Prepared::new(&f.t1, Some(&f.t2)).unwrap(), &xb)).collect();
        let (mut ctx, mut rs) = (XCtx::new(), RankScratch::new());
        let opts = XRankOptions::default();
        let (mut nq, mut t_all, mut t_scan, mut t_meas_full, mut t_meas_tiers, mut t_struct, mut t_exh) = (0usize, 0f64, 0f64, 0f64, 0f64, 0f64, 0f64);
        let (mut st_rows, mut st_touched, mut st_ham, mut st_dense) = (0u64, 0u64, 0u64, 0u64);
        let mut d0 = vec![0u8; 128 * 128];
        for i in (0..n).step_by(step) {
            for j in 0..n {
                if i == j || works[i].author == works[j].author {
                    continue;
                }
                let (q, t) = (&xs[i], &xs[j]);
                let mut out = vec![0i32; XRANK_FIELDS];
                let t0 = Instant::now();
                xrank(q, &[Some(t)], &cfg, &xb, &opts, &mut ctx, &mut rs, &mut out);
                t_all += t0.elapsed().as_secs_f64();
                nq += 1;
                let swapped = canon_swapped(&q.p, &t.p);
                let (ca, cb) = if swapped { (t, q) } else { (q, t) };
                let t0 = Instant::now();
                ctx.m.begin(ca, cb);
                ctx.m.scan_rows(ca, cb, &xb.xp, 512);
                t_scan += t0.elapsed().as_secs_f64();
                st_rows += ctx.m.stats.rows as u64;
                st_touched += ctx.m.stats.touched;
                st_ham += ctx.m.stats.hammings;
                st_dense += ctx.m.stats.dense_rows as u64;
                let am = paph::compare::mirror_side(&ca.p.kp, ca.p.t1_xmax());
                let (mda, mdb) = (ca.p.t1_max_dim(), cb.p.t1_max_dim());
                let t0 = Instant::now();
                ctx.m.pools(ca, cb, &xb.base);
                let f = Frames { a: &ca.p.kp, am: &am, b: &cb.p.kp, mda, mdb };
                std::hint::black_box(measure(&f, &ctx.m.pd, &ctx.m.pm, &bcfg, &xb.base, &mut ctx.g));
                t_meas_full += t0.elapsed().as_secs_f64();
                // one pools + measurement per tier, the scan between tiers excluded
                ctx.m.begin(ca, cb);
                for &tier in xb.xp.anchors.iter() {
                    ctx.m.scan_rows(ca, cb, &xb.xp, tier as usize);
                    let t0 = Instant::now();
                    ctx.m.pools(ca, cb, &xb.base);
                    let f = Frames { a: &ca.p.kp, am: &am, b: &cb.p.kp, mda, mdb };
                    std::hint::black_box(measure(&f, &ctx.m.pd, &ctx.m.pm, &bcfg, &xb.base, &mut ctx.g));
                    t_meas_tiers += t0.elapsed().as_secs_f64();
                }
                let t0 = Instant::now();
                let mut s = Structural::new(ca, cb, &xb.base);
                for k in 0..7 {
                    s.compute(k, ca, cb, &xb.base, &mut d0);
                }
                t_struct += t0.elapsed().as_secs_f64();
                // comparator 42's exhaustive scans of the same pair, both hypotheses
                let t0 = Instant::now();
                let mut pc = PairCtx::new(&ca.p, &cb.p);
                std::hint::black_box(pc.direct());
                std::hint::black_box(pc.mirror());
                t_exh += t0.elapsed().as_secs_f64();
            }
        }
        let us = |t: f64| 1e6 * t / nq.max(1) as f64;
        println!("  the scan: {:.0} rows a query, {:.1} candidates nominated a row (both hypotheses), {:.1} distances a row, {:.2}% of rows dense", st_rows as f64 / nq.max(1) as f64, st_touched as f64 / st_rows.max(1) as f64, st_ham as f64 / st_rows.max(1) as f64, 100.0 * st_dense as f64 / st_rows.max(1) as f64);
        println!("{} ({} queries between two authors' works): XRank {:.0} µs a query; the sparse scan of every row {:.0} µs (comparator 42's exhaustive scans, both hypotheses: {:.0} µs); pools and one measurement on every row {:.0} µs; one per anchor tier (four) {:.0} µs; every structural channel exactly {:.0} µs.", base.name_str(), nq, us(t_all), us(t_scan), us(t_exh), us(t_meas_full), us(t_meas_tiers), us(t_struct));
    }
}

/// The X profile a command runs under: the shipped X3 (bound to CAL-007),
/// 1.1's X2 with `--x2`, or 1.0's X1 with `--x1` (both bound to CAL-004).
fn xbound_for(args: &[String]) -> XBound {
    if args.iter().any(|a| a == "--x1") {
        return XBound::x1();
    }
    if args.iter().any(|a| a == "--x2") {
        return XBound::x2();
    }
    let mut xp = XProfile::x3();
    // experiments: --bars l,b,g (route lower bars), --nodoor
    if let Some(i) = args.iter().position(|a| a == "--bars") {
        let v: Vec<i32> = args[i + 1].split(',').map(|x| x.parse().unwrap()).collect();
        xp.t_local_low = v[0];
        xp.t_band_low = v[1];
        xp.t_global_low = v[2];
        eprintln!("route bars overridden: {:?}", v);
    }
    if args.iter().any(|a| a == "--nodoor") {
        xp.gate_door = 0;
    }
    XBound::new(Profile::cal007(), xp)
}

/// `--wire 3` hashes a corpus in 1.0–1.1's format (the default is 4).
fn wire_arg(args: &[String]) -> u8 {
    match sarg(args, "--wire", "4").as_str() {
        "3" => paph::config::WIRE_3,
        "4" => paph::config::WIRE_4,
        w => panic!("--wire {w}: the formats are 3 and 4"),
    }
}

/// Set when an artefact was not written (`write_artefact`); the process then
/// exits 2 after its report.
static REFUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Write a fitted artefact.  One under docs/calibration is a committed
/// profile, embedded by the crate: a refit may rewrite it with the same bytes
/// — which is how the logs show a refit reproduces it — and never replace it
/// with others (a fit on another corpus, format or profile is another
/// profile; `--out` writes it elsewhere).
fn write_artefact(path: &str, bytes: &[u8]) -> bool {
    if path.contains("docs/calibration/") {
        if let Ok(old) = std::fs::read(path) {
            if old != bytes {
                eprintln!("NOT WRITTEN: {path} is committed and these {} bytes (sha256 {}) differ from it; pass --out to write them elsewhere", bytes.len(), paph::sha256::hex(&paph::sha256::sha256(bytes)));
                REFUSED.store(true, std::sync::atomic::Ordering::Relaxed);
                return false;
            }
        }
    }
    std::fs::write(path, bytes).expect("write artefact");
    true
}

fn main() {
    run();
    if REFUSED.load(std::sync::atomic::Ordering::Relaxed) {
        std::process::exit(2);
    }
}

fn run() {
    let args: Vec<String> = std::env::args().collect();
    let dir = sarg(&args, "--dir", "target/si-corpus");
    let cfile = format!("{dir}/{}", sarg(&args, "--corpus", "corpus.bin"));
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        "corpus" if args.iter().any(|a| a == "--chain") => {
            // real bases: the chain's artworks under the twenty transforms, a
            // real host for the paste; the population stays the synthetic
            // distractors (the chain is far smaller than any population worth
            // measuring a reduction on — `sibench chain` measures its pairs)
            let src = sarg(&args, "--chain", "target/chain-corpus");
            let nd = arg(&args, "--distractors", 8000);
            let name = sarg(&args, "--out", "chain.bin");
            let wire = wire_arg(&args);
            std::fs::create_dir_all(&dir).unwrap();
            let (works, dups) = load_chain(&src);
            let keep: Vec<usize> = (0..works.len()).filter(|&i| works[i].img.w * works[i].img.h <= CHAIN_BASE_MAX_PIXELS).collect();
            let imgs: std::sync::Arc<Vec<Img>> = std::sync::Arc::new(keep.iter().map(|&i| works[i].img.clone()).collect());
            let nb = imgs.len();
            let mut jobs: Vec<(i32, i32, Box<dyn Fn() -> Img + Send + Sync>)> = Vec::new();
            for i in 0..nb {
                for t in 0..TRANSFORMS.len() {
                    let imgs = imgs.clone();
                    jobs.push((i as i32, t as i32, Box::new(move || {
                        let b = &imgs[i];
                        if TRANSFORMS[t] == "paste" {
                            paste(b, &chain_host(b, &imgs[(i + 1) % imgs.len()]), b.w / 2 + 3, b.h / 3 + 5)
                        } else {
                            transform(b, t, 31 + i as u64)
                        }
                    })));
                }
            }
            for k in 0..nd {
                jobs.push((-1, -1, Box::new(move || distractor(k))));
            }
            let t0 = Instant::now();
            let n = jobs.len();
            let recs = hash_all(jobs, wire);
            save(&format!("{dir}/{name}"), &recs);
            // which artwork each base is, for the reports that name pairs
            let map: String = keep.iter().enumerate().map(|(b, &i)| format!("{b}\t{}\t{}\t{}\t{}\n", works[i].author, works[i].permlink, works[i].created, works[i].sha)).collect();
            std::fs::write(format!("{dir}/{name}.bases.tsv"), map).unwrap();
            println!("{} ({}): {} artworks, {} byte-identical re-uploads left out, {} over {} px left out as bases; hashed {n} works ({nb} bases × {} variants + {nd} synthetic distractors){} in {:.1} s → {dir}/{name}", src, chain_snapshot(&src), works.len() + dups, dups, works.len() - nb, CHAIN_BASE_MAX_PIXELS, TRANSFORMS.len(), if wire == 3 { " in wire 3" } else { "" }, t0.elapsed().as_secs_f64());
        }
        "corpus" => {
            let nb = arg(&args, "--bases", 120);
            let nd = arg(&args, "--distractors", 8000);
            let from = arg(&args, "--from", 0);
            let name = sarg(&args, "--out", "corpus.bin");
            let wire = wire_arg(&args);
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
            let recs = hash_all(jobs, wire);
            save(&format!("{dir}/{name}"), &recs);
            println!("hashed {n} works ({nb} bases × {} variants + {nd} distractors){} in {:.1} s → {dir}/{name}", TRANSFORMS.len(), if wire == 3 { " in wire 3" } else { "" }, t0.elapsed().as_secs_f64());
        }
        "fit" => {
            let xb = xbound_for(&args);
            let c = Corpus::open(&cfile, &xb);
            // SI1's and SI2's fits are the artefacts in docs/calibration; a
            // synthetic fit bound to X3 is no shipped profile and stays here
            let name = si_name_for(&xb);
            let out = sarg(&args, "--out", &if name.starts_with("SI-") { format!("{dir}/{name}.psi") } else { format!("../docs/calibration/{name}.psi") });
            cmd_fit(&c, &xb, &out);
        }
        "lost" => {
            // comparator-42 Copy pairs XRank does not read Copy when shown the
            // target alone (both arrival orders): which stage lets them go,
            // what comparator 42 certified them on, and whether its own
            // gated rank screens them out too
            let xb = xbound_for(&args);
            let c = Corpus::open(&cfile, &xb);
            let cfg = Config::default();
            let base = xb.base.clone();
            let bcfg = paph::v4::bind(&cfg, &base);
            let mut ctx = XCtx::new();
            let mut rs = RankScratch::new();
            let mut d0 = vec![0u8; 128 * 128];
            let opts = if args.iter().any(|a| a == "--nogate") { XRankOptions { gate: false, ..XRankOptions::default() } } else { XRankOptions::default() };
            // under another profile than X1, also name the queries X1 loses
            // and this profile keeps, and what keeps them
            let x1b = XBound::x1();
            let (mut ctx1, mut rs1) = (XCtx::new(), RankScratch::new());
            const CLASS: [&str; 4] = ["Reject", "Defer", "Fast", "Absent"];
            let (mut n, mut lost) = (0, 0);
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
                        xrank(&q, &[Some(&x)], &cfg, &xb, &opts, &mut ctx, &mut rs, &mut out);
                        n += 1;
                        if out[0] == 3 || out[0] == 4 {
                            if xb.xid != x1b.xid {
                                let q1 = XPrepared::new(Prepared::new(&c.recs[qi].t1, Some(&c.recs[qi].t2)).unwrap(), &x1b);
                                let x1 = XPrepared::new(Prepared::new(&c.recs[ti].t1, Some(&c.recs[ti].t2)).unwrap(), &x1b);
                                let mut o1 = vec![0i32; XRANK_FIELDS];
                                xrank(&q1, &[Some(&x1)], &cfg, &x1b, &opts, &mut ctx1, &mut rs1, &mut o1);
                                if !(o1[0] == 3 || o1[0] == 4) {
                                    let s = paph::x::compare::xscreen(&q, &x, &cfg, &xb, &mut ctx);
                                    println!("lost under X1, kept: base {b} {:<10} route class {} (X1: {}), pair screen {:?} ({}), pools {}/{}, state {}", TRANSFORMS[t as usize], CLASS[out[6].clamp(0, 3) as usize], CLASS[o1[6].clamp(0, 3) as usize], s.state, s.reason, out[7], out[8], out[0]);
                                }
                            }
                            continue;
                        }
                        lost += 1;
                        let (pa, pb) = (&c.sides[i], &c.sides[j]);
                        let swapped = canon_swapped(pa, pb);
                        let (ca, cb) = if swapped { (pb, pa) } else { (pa, pb) };
                        let mut pc = PairCtx::new(ca, cb);
                        let v = compare_in(&mut pc, swapped, &bcfg, &base, Reading::Lean).base;
                        println!("base {b} {:<10} state {:>2} execution {} screen {} route class {} (local {}, band {}, global {}) pools {}/{}, keypoints {}/{}; comparator 42: {} — {} {:?} (structural {}, geometry evidence {}, inliers {})", TRANSFORMS[t as usize], out[0], out[1], out[2], out[6], out[3], out[4], out[5], out[7], out[8], pa.kp.len(), pb.kp.len(), v.verdict, v.class, v.basis, v.structural, v.geometry_evidence, v.total_inliers);
                        use paph::x::structural::*;
                        let (xa, xc) = if canon_swapped(&q.p, &x.p) { (&x, &q) } else { (&q, &x) };
                        let mut st = Structural::new(xa, xc, &base);
                        for k in 0..7 {
                            st.compute(k, xa, xc, &base, &mut d0);
                        }
                        println!("    channels (dct, local, shape, topology, runs, palette, silhouette; -1 not measurable) {:?}", (0..7).map(|k| if st.measurable[k] { st.value[k] } else { -1 }).collect::<Vec<_>>());
                        let sc = paph::v42::screen_v42_prepared(&c.sides[i], &c.sides[j], &cfg, &base);
                        println!("    comparator 42's stage-1 screen (its rank's gate): {} (pools {}/{})", if sc.pass { "pass" } else { "screened out" }, sc.pool_direct, sc.pool_mirror);
                    }
                }
            }
            println!("{lost} of {n} comparator-42 Copy queries not read Copy by XRank alone ({})", xb.xp.name_str());
            // comparator 42's own gated rank (`rank` with the gate on) drops a
            // pair its stage-1 screen rejects; the screen reads the pair in
            // canonical order, so one count per pair
            let mut by_t = vec![(0usize, 0usize); TRANSFORMS.len()];
            let (mut thin, mut fast) = (0usize, 0usize);
            for b in 0..c.nb {
                for t in 1..TRANSFORMS.len() as i32 {
                    if c.copy(b, t) {
                        let (i, j) = (c.at[&(b, 0)], c.at[&(b, t)]);
                        let (pa, pb) = (&c.sides[i], &c.sides[j]);
                        let sc = paph::v42::screen_v42_prepared(pa, pb, &cfg, &base);
                        by_t[t as usize].0 += 1;
                        if !sc.pass {
                            by_t[t as usize].1 += 1;
                            thin += ((pa.kp.len().min(pb.kp.len()) as i64) < bcfg.geo_min_corr as i64) as usize;
                            let xa = XPrepared::new(Prepared::new(&c.recs[i].t1, Some(&c.recs[i].t2)).unwrap(), &xb);
                            let xc = XPrepared::new(Prepared::new(&c.recs[j].t1, Some(&c.recs[j].t2)).unwrap(), &xb);
                            fast += (paph::x::compare::xscreen(&xa, &xc, &cfg, &xb, &mut ctx).route_class == paph::x::route::RouteClass::Fast) as usize;
                        }
                    }
                }
            }
            let (np, ns): (usize, usize) = (by_t.iter().map(|x| x.0).sum(), by_t.iter().map(|x| x.1).sum());
            let per: Vec<String> = by_t.iter().enumerate().filter(|(_, x)| x.1 > 0).map(|(t, x)| format!("{} {} of {}", TRANSFORMS[t], x.1, x.0)).collect();
            println!("comparator 42's own gated rank (its stage-1 screen, either profile) screens out {ns} of {np} comparator-42 Copy pairs, {thin} of them because a side has fewer keypoints than geo_min_corr ({}) and so can never pass; XRank's route reads {fast} of the {ns} as Fast under {}, and does not gate those: {}", bcfg.geo_min_corr, xb.xp.name_str(), per.join(", "));
        }
        "doorprof" => {
            let xb = xbound_for(&args);
            let c = Corpus::open(&cfile, &xb);
            cmd_doorprof(&c, &xb, args.iter().any(|a| a == "--real"));
        }
        "wire" => println!("{}", file_wire(args.get(2).map(|s| s.as_str()).unwrap_or(&cfile))),
        "dump" => {
            // one corpus work's wires as files, for xcli: synthetic bases
            // only (a chain corpus's bases are the chain's artworks, which
            // stay out of every file but the corpus itself)
            assert!(!std::path::Path::new(&format!("{cfile}.bases.tsv")).exists(), "dump reads the synthetic corpus only");
            let b = arg(&args, "--base", 0) as i32;
            let tf = sarg(&args, "--tf", "base");
            let t = TRANSFORMS.iter().position(|&x| x == tf).unwrap_or_else(|| panic!("--tf {tf}: one of {TRANSFORMS:?}")) as i32;
            let out = sarg(&args, "--out", "target/dump");
            let r = load(&cfile).into_iter().find(|r| r.base == b && r.tf == t).unwrap_or_else(|| panic!("no work {b} / {tf} in {cfile}"));
            std::fs::create_dir_all(&out).unwrap();
            let stem = format!("{out}/{b}-{tf}");
            std::fs::write(format!("{stem}.t1"), &r.t1).unwrap();
            std::fs::write(format!("{stem}.t2"), &r.t2).unwrap();
            println!("{stem}.t1 ({} B, wire {}), {stem}.t2 ({} B)", r.t1.len(), r.t1[4], r.t2.len());
        }
        "chain" => cmd_chain(&sarg(&args, "--chain", "target/chain-corpus")),
        "calib" => cmd_calib(&args, &dir),
        "calfit" => cmd_calfit(&args, &dir),
        "xtrace" => cmd_xtrace(&args, &dir),
        "xtime" => cmd_xtime(&args),
        "chainfit" => {
            // the chain's corpus unless told otherwise, and the profile named
            // after the X profile it is bound to: SI4 under X3, SI3 under X2
            // (each committed: `write_artefact` rewrites one only with its own
            // bytes); a fit bound to X1 is no shipped profile and stays here
            let xb = xbound_for(&args);
            let c = Corpus::open(&format!("{dir}/{}", sarg(&args, "--corpus", "chain.bin")), &xb);
            let named = if xb.xid == XProfile::x2().id() { "SI3-PROVISIONAL" } else if xb.xid == XProfile::x1().id() { "SI-CHAIN-X1" } else { "SI4-PROVISIONAL" };
            let name = sarg(&args, "--name", named);
            let out = if name.starts_with("SI-") { format!("{dir}/{name}.psi") } else { format!("../docs/calibration/{name}.psi") };
            cmd_chainfit(&c, &xb, &sarg(&args, "--out", &out), &name);
        }
        "route" => {
            let xb = xbound_for(&args);
            let c = Corpus::open(&cfile, &xb);
            cmd_route(&c, &xb, &args);
        }
        "eval" => {
            let xb = xbound_for(&args);
            let prof = si_profile_arg(&args, &xb);
            let c = Corpus::open(&cfile, &xb);
            let chain = std::path::Path::new(&format!("{cfile}.bases.tsv")).exists();
            cmd_eval(&c, &prof, &xb, &args, chain);
        }
        _ => {
            println!("sibench corpus | fit | eval   (see the header of rust/src/bin/sibench.rs)");
        }
    }
    let _ = std::io::stdout().flush();
}
