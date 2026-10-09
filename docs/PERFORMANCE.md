# Performance

Every optimisation in this repository is **output-identical**: the same pixels hash to the same
wires and the same wires compare to the same report, byte for byte, before and after. That is
not a hope but a test — see [the equivalence discipline](#the-equivalence-discipline) below —
and it is the constraint that shaped every change here: PAPH is a consensus artefact, so a
faster engine that disagrees in one byte is a broken one.

Baseline: the Rust reference and WebAssembly module shipped in `paph-js` 4.2.2. Machine: one
core of an Intel Xeon @ 2.80 GHz (cloud VM), Rust 1.97, Node 22.22. Times are the best of five
batches (a shared machine's noise only ever adds time).

## Native (Rust)

`rust/bench.sh` — the same harness (`rust/src/bin/prof.rs`) built against both crates.
The middle column builds the baseline with this repository's target flags (`x86-64-v2`, which
gives `count_ones` a single `POPCNT`), so the last column is the code alone.

| | baseline, as shipped | baseline, x86-64-v2 | **this repo** | code speedup |
|---|---:|---:|---:|---:|
| hash sprite 128×128 | 5.62 ms | 5.63 ms | **2.35 ms** | 2.4× |
| hash tile 192×192 | 12.95 ms | 13.60 ms | **5.23 ms** | 2.6× |
| hash banner 320×128 | 42.19 ms | 41.66 ms | **8.36 ms** | 5.0× |
| hash scene 288×200 | 48.99 ms | 48.45 ms | **12.41 ms** | 3.9× |
| hash work 512×384 | 119.2 ms | 116.0 ms | **52.4 ms** | 2.2× |
| hash scene 1024×768 | 1112 ms | 1058 ms | **161 ms** | 6.6× |
| compare-42 work × itself | 14.13 ms | 11.62 ms | **3.47 ms** | 3.4× |
| compare-42 scene × mirrored | 14.49 ms | 11.82 ms | **3.46 ms** | 3.4× |
| compare-42 sprite × paste-crop | 3.39 ms | 2.83 ms | **1.13 ms** | 2.5× |
| compare-42 sprite × scene | 3.17 ms | 2.60 ms | **1.06 ms** | 2.5× |
| compare-42 banner × tile | 5.38 ms | 4.26 ms | **1.18 ms** | 3.6× |
| screen scene × mirrored | 2.69 ms | 1.72 ms | **1.03 ms** | 1.7× |

## WebAssembly (Node 22, V8)

`npm run bench` (test/bench-engines.mjs) — the JavaScript engine, the `paph-js` 4.2.2 module,
and this repository's SIMD128 and baseline builds, through their glue.

**Hash**

| image | JavaScript | WASM 4.2.2 | **WASM SIMD** | WASM baseline |
|---|---:|---:|---:|---:|
| sprite 96×96 | 16.5 ms | 5.54 ms | **3.24 ms** (5.1× JS, 1.7× 4.2.2) | 3.27 ms |
| scene 288×200 | 569 ms | 77.3 ms | **16.3 ms** (35× JS, 4.7× 4.2.2) | 25.1 ms |
| work 512×384 | 657 ms | 171 ms | **59.2 ms** (11× JS, 2.9× 4.2.2) | 87.2 ms |
| scene 1024×768 | 17.2 s | 1.98 s | **236 ms** (73× JS, 8.4× 4.2.2) | 378 ms |

**Comparator 42** — in `paph-js` 4.2.2 comparator 42 existed only in JavaScript; the
WebAssembly module now carries it, with the full JavaScript-shaped report or the lean reading
(same verdict, no v3 diagnostics) that ranking uses.

| pair | JavaScript | **WASM SIMD, full** | WASM SIMD, lean | WASM baseline, full | verdict |
|---|---:|---:|---:|---:|---|
| scene × mirrored scene | 68.9 ms | **5.55 ms** (12×) | 4.20 ms | 8.56 ms | Copy |
| scene × unrelated scene | 44.4 ms | **2.75 ms** (16×) | 2.20 ms | 4.65 ms | Suspected |
| sprite × pasted into a host | 15.4 ms | **1.49 ms** (10×) | 1.13 ms | 1.81 ms | Copy |
| work × 70 % crop | 49.8 ms | **3.49 ms** (14×) | 2.98 ms | 5.55 ms | Copy |
| scene × 3× upscale | 79.2 ms | **7.67 ms** (10×) | 6.58 ms | 10.7 ms | Copy |

**The v3 comparator through the C ABI** (the call both modules have): 9.96 → 2.95 ms
(scene × mirrored), 6.73 → 3.55 ms (scene × unrelated), 2.37 → 1.20 ms (sprite × paste).

**Ranking** — one query against 103 prepared candidates in one call: 148 ms screen-gated
(1.44 ms per candidate; 49 of 103 pass the screen), 184 ms comparing every one.

The descriptor scan at the heart of every comparison went from 7.8 to 2.6 ns per descriptor
pair in WebAssembly (see *popcounts* below).

## What changed

**Hashing**

- *Local fingerprints* (93 % of a large dithered hash): each window position evaluated once per
  window size; 16-px windows read precomputed 2×2 block medians stored as four parity planes,
  so a window's 64 stride-2 reads are eight contiguous rows; the canonical threshold
  `2v > s31 + s32` is exactly `v > s31`, so only the lower median is needed — a 10-step
  branch-free binary search in u16 lanes (SIMD128 / SSE2) finds it and the 64-bit mask together;
  selection streams through an exact top-k instead of sorting every candidate.
- *Keypoints*: pyramid levels from two shared summed-area tables; u32 wrapping tables (every box
  is below 2³², so exact); FAST-9 pre-test and nine-arc test in 16 lanes, checked on all 65,536
  masks; quality selection by lazy greedy (scores only fall, so a stale heap entry is an upper
  bound — same pick, same tie order, a handful of re-scores per round instead of 2,048);
  descriptor bits and orientation moments from exact 5×5 / 3×3 box sums.
- *Sections*: region adjacency in a dense pair table (was a SipHash insert per pixel pair); run
  lengths in one row-major pass for all three directions; content peaks with a branch-free
  gradient and a van Herk / Gil-Werman max filter; area majority visiting only touched slots.
- *Front end*: the upscale factor as gcd(w, h, every colour-change position) in one
  early-exiting pass (was a re-walk per divisor — 45 % of hashing a 4× sprite); no copy of the
  input unless a stage changes it; CRC-32 slicing-by-8.

**Comparing**

- *Parse once, scan once*: a side is parsed once into a prepared structure (both geometry
  frames, packed direct and mirrored descriptors, the local bag); a pair's mutual
  best/second-best descriptor scan runs once per hypothesis and is shared by every reading —
  `compare_v42` used to parse each side four times and compute the 512 × 512 descriptor matrix
  four times. The scan itself runs in u16 lanes.
- *Assignment*: the Hungarian algorithm on the real edges only — every column's completion
  offer is `BIG + Kmin − v[j]`, so the dense padded matrix is never built — with lazy potential
  updates; held pair-for-pair to the reference on 2,100 tie-heavy instances.
- *Geometry*: vote slots as one 16-byte record, the peak carried as votes arrive (every vote
  strictly grows its key's tuple), bins as boxes instead of 24 keys per correspondence; the weak
  signal read from round 0 instead of re-verified.
- *Small things that were hot*: an exact integer square root from an f64 guess (one correction
  below 2⁵²); coherence's median by selection; level dimensions from tables; a static popcount
  table that used to be rebuilt per compare.

**WebAssembly**

- *Popcounts*: with SIMD128 enabled, LLVM vectorised every popcount loop into a byte-wise
  `i8x16.popcnt` reduction, which V8 runs at a third of the speed of scalar `i64.popcnt`
  (6.7 ns per descriptor pair against 2.0). Descriptor words are now loaded through a volatile
  read in the SIMD build so no vectoriser can pack those loops; the explicit vector kernels stay
  where they win (the scan, the median search, FAST).
- *ABI 2*: prepared sides as handles, `rank42` (screen + compare a query against n candidates
  in one call, each pair's scans shared between screen and comparison), profiles as handles,
  the lean reading, index keys. One boundary crossing per query instead of per pair.
- *Two builds*: SIMD128 (`paph.wasm`) and baseline (`paph-baseline.wasm`), the glue picks by a
  31-byte probe module; `wasm-opt -O3`.

## Wire 4 (1.2)

`@pixagram/paph-x` 1.2 hashes in wire 4 ([SPEC-W4](SPEC-W4-paph-wire4.md)), which resamples the
DCT, the brightness record (read from the resampled thumbnail), the shapes section and the
silhouette. Measured on another machine than the tables above (one
core of a shared 2.1 GHz Xeon), wire 3 beside wire 4 in the same run, best of five batches:

| image | native, wire 4 | native, wire 3 | | WebAssembly SIMD, wire 4 | WebAssembly SIMD, wire 3 |
|---|---:|---:|---|---:|---:|
| sprite 128×128 (`rust/bench.sh`) · 96×96 (`npm run bench`) | 1.61 ms | 1.74 ms | | 1.40 ms | 1.29 ms |
| scene 288×200 | 7.87 ms | 8.20 ms | | 11.4 ms | 11.3 ms |
| work 512×384 | 34.6 ms | 33.1 ms | | 37.1 ms | 35.1 ms |
| scene 1024×768 | 113 ms | 118 ms | | 171 ms | 170 ms |
| banner 320×128 · tile 192×192 | 5.73 · 3.76 ms | 6.24 · 3.45 ms | | | |

Within 9 % either way: 0.92–1.09× natively, 1.01–1.09× in WebAssembly
(`docs/calibration/X3-PROVISIONAL.log`). Two things keep it there, both output-identical (the
wire-4 digest holds them): the exact radial profiles step along each ray by carrying a remainder
instead of dividing per step, and the shapes section measures each distinct small region shape
once — regions of one small shape, as a dither leaves them on the grid, can tie in numbers at the
eighth area, and each would otherwise be cast 32 rays and its holes counted.

`npm run bench` now warms both WebAssembly builds up before it times anything: V8 runs a
function in its baseline tier for its first calls, and the first image of the table — the
sprite — was otherwise timed partly there.

## The equivalence discipline

- **Digest**: `rust/src/equiv.rs` hashes and compares a deterministic synthetic corpus —
  degenerate sizes, dithered art, mattes, blow-ups, every D4 transform, inversion, recolour,
  crops, pastes, sixteen hash-time configurations, every comparator, the Tier-1-only path,
  corrupt wires, the JavaScript-shaped reports and the lean readings — one SHA-256 per case,
  3,160 cases. `rust/check.sh` diffs it against `test/equiv-digest.txt`; `npm run test:equiv`
  computes it inside both WebAssembly builds. From 1.2 the same corpus runs again in wire 4
  (`test/equiv-digest-4.txt`, 3,164 cases: comparator 42 under CAL-007, and the mixed-format
  pairs every comparator must refuse), and both files are checked. Every optimisation left it untouched; the only
  outputs that moved are the Rust reference's parity fixes listed in the changelog, each
  making it do what the JavaScript engine (and the specification) already did.
- **Unit tests** hold each fast path equal to the reference it replaced (kept, under `cfg(test)`):
  scans against the sequential rule, the lazy Hungarian against the dense one, the
  slicing-by-8 CRC against the bytewise one, FAST's arc test on every mask, the square root on
  200,000 random values.
- **Cross-engine**: `npm run test:wasm` — 625 ordered pairs give byte-identical report text in
  the JavaScript engine and WebAssembly, plus lean readings, screens and rank records;
  `npm run test:native` — the native reference against the JavaScript port, field for field;
  `npm run test:bench` — the evidence bench in a headless DOM, on both engines, with the
  attack sweep identical row for row.

## PAPH-X

The numbers above are comparator 42's, unchanged in `@pixagram/paph-x` 1.0.0. PAPH-X — the retrieval-native
cascade that reaches the same verdicts through a route screen, sparse descriptor retrieval,
anchor-tier geometry and lazy structural evidence — has its own record, with the
specification's release gates and what is and is not met: [PAPH-X.md](PAPH-X.md).
