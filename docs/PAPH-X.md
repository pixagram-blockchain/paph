# PAPH-X — implementation record

PAPH-X is the retrieval-native comparison cascade specified in
[SPEC-X-paph-x.md](SPEC-X-paph-x.md): the same wires (format 3), the same comparator-42
verdict vocabulary and calibration (CAL-004-PROPOSED), reached through a cheaper path — a
128-byte route signature screened first, descriptor matching by locality-sensitive buckets
instead of the 512 × 512 Hamming scan, geometry on a 96-keypoint anchor tier that expands only
when it must, structural channels computed only while the verdict lattice can still move, and
the exact comparator (`EXACT42`) as the fallback and the audit. This document is what was built,
how it departs from the specification where it does, what it measures, and which of the
specification's release gates it meets. Every number here was printed by `rust/target/release/xbench`
or `node test/x-bench.mjs` on this machine (one core of a 2.1 GHz Xeon, shared), as §41.7 asks;
none of them is a claim beyond that.

Status: milestones M1–M7 of §40 are built, tested and verified across the native engine and
both WebAssembly builds. M8 (calibration) is done on the specification's synthetic corpus only:
profile **X1-PROVISIONAL** (`b96040d888b21e28…`) is bound to CAL-004-PROPOSED
(`91b545f801f5a095…`); no real moderation corpus has been added, so the profile keeps its
PROVISIONAL name and the fast policy is not the default.

## 1. What the cascade does

```
X0  identity          byte-identical Tier 1 → Identical; corrupt Tier 2 → Indeterminate
X1  route             XRoute vs XRoute: 64 local-MinHash lanes, 32 band-MinHash lanes, 4 global
                      invariant words → REJECT / DEFER / FAST, by rule, never one number
X2  sparse retrieval  per side a 24-projection × 12-bit bucket index over the 256-bit descriptors;
                      rows of the smaller side probe the larger side's buckets, both hypotheses
                      in one pass; Hamming only on nominated pairs; exact 4.2 acceptance
                      (ceiling, two-sided Lowe, mutual best, two-sided margin) on what was found
X3  geometry          4.2's Hough + similarity fit + §7 consumption on the sparse pools of the
                      96 anchor keypoints; expansion 96 → 160 → 256 → 512 rows until a model
                      certificate holds (§9.4) or the rows run out
X4  structural        the seven channels of comparator 42 as an interval; cheapest first, each
                      asked for only while the lattice is undecided; the local channel bounded
                      from the edge set before its assignment is paid for
X5  fallback / audit  DEFER → EXACT42 under the safe policy, Indeterminate under fast; exact
                      policy and audit run EXACT42 outright
```

Every verdict carries an **execution state** beside it — `FAST` (the cascade decided),
`DEFERRED` (it would not, and the policy forbade the fallback), `FALLBACK` (EXACT42 decided),
`AUDIT` (EXACT42 ran, attached) — and a `reason` (`bounded-evidence`, `exact-evidence`, `route`,
`identical`, `uncertified-geometry`, `truncated-pool`, `saturated-control`, …). The verdict
lattice itself is comparator 42's (SPEC-004.2 R1–R4): the structural and the geometric axes
are never averaged, the thresholds are the profile's, and a verdict is stated only when every
corner of the evidence intervals agrees on it (§10.4).

### Where it lives

| spec | module | what |
|---|---|---|
| §6, §12.1 | `rust/src/x/route.rs` | `XRoute` (136-byte record), b-bit MinHash over the Tier-1 local codes and the descriptor bands, D4- and inversion-invariant global words (DCT symmetrised magnitudes, run classes, RAG cells, shape classes), `route_score`, `route_class`, `RouteSoA` + `route_batch` (SSE2 / SIMD128 lane kernels with a scalar twin held equal by test) |
| §8.2–8.3 | `rust/src/x/bucket.rs` | `project`, `mirror_proj`, `XBucketIndex` (CSR with presence masks, bisection, per-entry hot flag) |
| §9.1 | `rust/src/x/anchor.rs` | `anchor_order`: 4.2's quality selection in pick order |
| §8.4–8.8, §24 | `rust/src/x/matcher.rs` | `MatchScratch`: nomination by support, packed state, fixed scratch; `count` (the §24 screen count without building correspondences), `pools` (the accepted correspondences in 4.2's order) |
| §9.2–9.5 | `rust/src/x/geom.rs` | `measure`, `control`, `diversity`, `topology`, `coverage_into` — 4.2's geometry (`geom42`) on reusable scratch, two-pass §7 consumption |
| §10, §27–28 | `rust/src/x/structural.rs` | the six secondaries rewritten allocation-free (held equal to the v3 channels by test), `local_bound`, `local_exact`, `Structural` with `bounds`/`channels_corner`/`compute`/`exact` |
| §11–13, §23, §29 | `rust/src/x/compare.rs` | `xscreen`, `xcompare`, `xcompare_in`, the lattice corners (`decide`, `all_below_suspected`), the deferral rules, `XCtx`, `report_json` |
| §7, §14, §19 | `rust/src/x/rank.rs` | `xrank`: Stage A route table (SIMD), Stage B sparse screen, Stage C cascade; 24-field `i32` records |
| §15, §17 | `rust/src/x/prepared.rs` | `XPrepared` = `Prepared` + route + index + anchor order + burst weights + measurability bits |
| §18 | `rust/src/x/sidecar.rs` | PAX1: route + index + order, CRC-32, rebuilt on any mismatch |
| §20 | `rust/src/x/profile.rs` | `XProfile` (the `.pxcl` artefact: version, comparator 50, bars, seeds, projection table, caps, anchors, certificate, policy; SHA-256 identity; tamper-refusing decode), `XBound` (a profile bound once: ids, salts, projections) |
| §30 | `rust/src/x/abi.rs`, `wasm/paph.js`, `wasm/paph.d.ts` | the C ABI (ABI 3 / X ABI 1), the JavaScript glue and its types |
| §33–35 | `rust/src/bin/xbench.rs`, `test/x-bench.mjs` | the benchmark and acceptance harness (native) and the WebAssembly timings |
| §36 | `rust/src/x/*/tests`, `test/x-wasm.mjs`, `rust/src/bin/xcli.rs` | equivalence: exact where the specification asks for exactness, semantic against comparator 42 elsewhere, byte-identical across engines |

`rust/src/synth.rs` holds the picture generators (moved out of `equiv.rs`, digest unchanged) that
the harness, the tests and the equivalence digest share. Comparator 42, its calibration and the
wire are untouched: the equivalence digest (3,160 cases) is byte-identical, natively and in both
WebAssembly builds, and `test/wasm-parity.mjs` still passes 625 ordered pairs byte for byte.

## 2. Decisions that depart from the specification, and why

Each of these was measured before it was taken; `xbench --calibrate`, `--table` and `--explain`
print the evidence.

**12-bit projections, not 8 (§8.2).** With 8 bits a bucket of a busy 512-keypoint work holds
dozens of entries and the nomination work exceeded the exhaustive scan it replaces on the
specification's own "repeated texture" and "dense dither" classes. 24 projections of 12 bits
keep the family mirror-closed (projection *p* + 12 reads positions +128 mod 256, so a mirror is
a projection swap — §9.5, §41.4), and a corpus-derived table (`TABLE_X1`, chosen greedily to
minimise collisions weighted by the bits' flip rates under the transforms) keeps buckets near
uniform. Measured on the corpus: `expected touched per row` fell from the hundreds to tens.

**The smaller side scans the larger side's index.** A pasted sprite's keypoints sit low in the
host's anchor order; scanning from the smaller side finds every paste the exhaustive matcher
finds. The pairs nominated are the same set either way (the index is symmetric in what it
holds), so the screen count and the pools are symmetric.

**A dense-row hybrid (§38 "sparse explosion → exact").** A row whose buckets nominate more
than a quarter of the other side is scanned exhaustively with 4.2's own kernels instead — the
exact answer, at the exact cost, only where sparsity has nothing to offer. The `explosion` flag
is reported but no longer defers by itself: the pools it produces are exact.

**Unknown second-best reads as the ceiling.** The sparse matcher often knows a row's best
match but not its second-best. Reading the unknown second-best as `ham_max` makes the Lowe
test exactly as strict as 4.2's would be if the hidden second-best sat at the ceiling, and
strictly stricter otherwise — a sparse pool is a subset of 4.2's acceptances, never a superset
with phantom correspondences.

**The control runs first when geometry could decide alone (§10.1).** On anchor-tier pools the
GN control (five permutations of the pool) is cheaper than the structural channels it would
otherwise make unnecessary, so the scheduler runs it ahead of them whenever the lattice would
be decided by geometry at the top of its interval.

**What defers (§23).** A verdict is stated from the cascade's own evidence only when it is
*safe*: the sparse pool was not truncated, and either the geometry is not pivotal (the verdict
is the same with geometry silenced and with geometry at the most the pool could give), or it
is pivotal and holds a certificate (§9.4: ≥ 12 inliers, ≥ 12 distinct keypoints on each side, ≥ 3
coverage cells, median error ≤ 4000, inliers stable to 80 % across the last expansion). A
control that saturated on a pool whose measurement sits at the confidence point while the
verdict could still move upward is a structured accident and defers too. Everything else is
`FAST`.

**Route hard-negatives read `Unrelated` only under the fast policy.** The route bars of X1
(`local` 6 / `band` 3 / `global` 190, the largest that reject no comparator-42 Copy on the
corpus; fast bars 7 / 5 / 203 above every negative) were calibrated for the *copy* question.
A pair the route rejects with no anchor-tier evidence reads `Unrelated` on the fast path —
calibrated rejection, §12.2 — and on the corpus 449 of 2,379 pairs read `Related` or
`Suspected` under comparator 42 that way (six of them `Suspected`: dithered or resampled copies
42 only suspects). The safe policy promises 42's states, so it takes the sparse path for those
pairs instead and reads what 42 reads — on the corpus, every state of every pair. The profile
knob `route_reject_unrelated` turns the shortcut off for the fast path too.

**`Scope::Copy` and `NotCopy`.** A search needs the copy question, not the full state: under
copy scope the cascade stops as soon as every remaining corner of the lattice lies below
`Suspected` and reports `NotCopy` (state 6), unresolved between `Related` and `Unrelated`. XRank
uses it by default; the full scope is a flag.

**Profiles are bound once (`XBound`).** Hashing the base and X profiles per call made the
screen allocate and cost more than the screen itself; a bound profile carries the identities,
the route salts and the projection table, and the screen hot path runs with zero allocations
(measured: 200 calls, 0 allocations, 0 bytes).

**The X report is comparator 50.** Its verdict vocabulary is 42's; the number says the
evidence was gathered by a different path. `exact` policy, `AUDIT` and `FALLBACK` carry the
comparator-42 report in full under `fallback`.

## 3. API

Rust: `paph::x::{XProfile, XBound, XPrepared, xscreen, xcompare, xrank, XRoute}`; a thread's
scratch is an `XCtx`. C ABI: [WASM-ABI.md](WASM-ABI.md) (`paph_xprofile`, `paph_xprepare`,
`paph_xscreen`, `paph_xcompare`, `paph_xrank`, `paph_xroute`, `paph_xsidecar`). JavaScript, both
Node and the browser:

```js
import { init } from '@pixagram/paph-x/wasm';
const paph = await init();

const a = paph.xprepare(fpA.t1, fpA.t2), b = paph.xprepare(fpB.t1, fpB.t2);
paph.xscreen(a, b);                       // { state: 'Pass'|'Defer'|'Reject'|'Identical', route, poolDirect, … }
const r = paph.xcompare(a, b);            // safe: { verdict: 'Copy', execution: 'FAST', reason, route, sparse, geometry, structural, … }
paph.xcompare(a, b, { policy: 'fast' });  // never falls back: 'DEFERRED' + Indeterminate where the cascade will not decide
paph.xcompare(a, b, { audit: true });     // EXACT42 beside the fast path, attached as r.fallback
paph.xrank(q, sides);                     // one call, copy scope: [{ state, verdict, execution, screen, route, … }]
const pax = a.sidecar();                  // cache beside the wires; paph.xprepare(t1, t2, { sidecar: pax }) skips the derivation
```

Policies: `safe` (default; DEFER → EXACT42, every Copy answer is 42's or certified), `fast`
(DEFER → `Indeterminate`, state `DEFERRED` — the caller decides), `exact` (EXACT42 outright,
`AUDIT`). The native CLI `rust/target/release/xcli` prints the same route, screen, report and
rank records from wire files; `test/x-wasm.mjs` diffs them against the WebAssembly module.

## 4. Measurements

Native, one core, the specification's corpus of §33.1 as `xbench` builds it (344 works: 14 bases
× 17 transforms, 60 busy "large" works, same-style, random, repeated-texture and noise
negatives; 2,379 pairs across the classes of §33.2). Two runs agreed within a few percent; the
figures are the first.

| pairwise, µs | p50 | p90 | p95 | p99 |
|---|---:|---:|---:|---:|
| screen 42 | 44.0 | 910 | 1,691 | 2,803 |
| **xscreen** | **15.8** (2.8×) | 92 | 111 (15.2×) | 185 |
| compare 42, lean | 498 | 1,508 | 2,260 | 3,323 |
| **xcompare, fast** | **19.1** (26×) | 547 | 696 | 1,002 |
| **xcompare, safe** | **220** (2.3×) | 599 | 741 | 1,314 |

Per class, p50 µs (screen 42 / xscreen / compare 42 / xcompare fast / xcompare safe):

| class | n | screen 42 | xscreen | compare 42 | fast | safe |
|---|---:|---:|---:|---:|---:|---:|
| mirror | 14 | 13.8 | 23.3 | 786 | 238 | 224 |
| crop | 14 | 2.8 | 4.9 | 568 | 98 | 355 |
| paste | 14 | 142 | 56 | 820 | 345 | 351 |
| recolour | 14 | 8.2 | 9.5 | 794 | 137 | 227 |
| up2 / up3 | 28 | 12 | 22 | 670 | 246 | 295 |
| rot90 / rot180 / transpose | 42 | 14 | 22 | 693 | 175 | 224 |
| invert | 14 | 10.6 | 8.8 | 728 | 530 | 525 |
| neg: same-style | 472 | 15.5 | 8.0 | 400 | 9.4 | 195 |
| neg: large (busy, 512 kp) | 931 | 105 | 29 | 621 | 30 | 344 |
| neg: random | 123 | 76 | 16 | 505 | 16 | 124 |
| neg: texture | 64 | 1.0 | 1.3 | 213 | 7.7 | 65 |

Agreement with comparator 42 (lean) over the 2,379 pairs:

| | |
|---|---|
| screen hard-rejected a comparator-42 Copy | **0** |
| safe policy, copy disagreements | **0** |
| safe policy, any-state disagreements | **0** |
| fast policy, false Copy | **0** |
| fallback rate (safe) | **1.4 %** (deferrals: truncated pools, uncertified pivotal geometry, saturated controls) |
| full Hamming pairs, 42 → X | 123,338,942 → 1,222,253 (**−99.0 %**) |
| xscreen allocations, 200 calls | **0** |
| xprepare (route + index + order) | p50 201 µs, p95 1.5 ms; cached by the PAX1 sidecar |

Ranking, one query against N prepared candidates, safe policy, copy scope (`rank42` is 4.2's
screen-gated screen + lean compare over the same candidates):

| workload | rank 42 | xrank | |
|---|---:|---:|---:|
| §3.1 reference: B09 (512 kp) vs 100 of the 512-kp class (mean 361 kp) | 222 ms | **31.5 ms** (37 compared, 2 fallbacks, 15 copies found by both) | **7.1×** |
| N = 100, B09 (512 kp), mixed candidates | 126 ms | 30.4 ms | 4.1× |
| N = 100, B11 (25 kp) | 9.8 ms | 10.5 ms (10 fallbacks) | 0.9× |
| N = 100, aggregate of three queries | 136 ms | 40.9 ms | 3.3× |
| N = 1,000, B09 | 859 ms | 132 ms | 6.5× |
| N = 1,000, aggregate | 912 ms | 157 ms | 5.8× |
| route-only screen of 10,000 candidates | — | 185 µs (18.5 ns per candidate, 91 % rejected) | |

WebAssembly (SIMD128 build, Node, best of 5; `node test/x-bench.mjs`), on the JavaScript test
corpus's shared-texture fixtures and on the specification's corpus:

| pair | screen 42 | xscreen | compare 42, lean | xcompare safe | xcompare fast |
|---|---:|---:|---:|---:|---:|
| scene × mirrored scene | 1.18 ms | 0.119 (9.9×) | 3.30 ms | 0.417 (7.9×) | 0.399 |
| scene × unrelated scene | 1.16 | 0.031 (38×) | 1.76 | 0.732 (2.4×) | 0.705 |
| sprite × pasted into a host | 0.296 | 0.034 (8.8×) | 0.829 | 0.222 (3.7×) | 0.225 |
| work × 70 % crop (deferred, falls back) | 1.23 | 0.132 (9.3×) | 2.41 | 4.58 (0.5×) | 2.12 |
| scene × 3× upscale | 1.20 | 0.112 (10.7×) | 4.59 | 0.581 (7.9×) | 0.566 |
| scene × recoloured scene | 1.21 | 0.038 (32×) | 2.06 | 0.524 (3.9×) | 0.507 |
| reference workload: B09 vs 100 of the 512-kp class | 110.5 ms | — | — | **33.2 ms (3.3×)** | 27.3 ms (4.0×) |
| 66 unrelated same-style pairs of the corpus, mean | 0.056 | 0.0082 (6.8×) | 0.451 | 0.250 (1.8×) | |

The WebAssembly ratios are lower than the native ones because comparator 42's exhaustive
descriptor scan is the part of 42 that SIMD128 accelerates best (2.4 ns per pair there against
5.5 ns natively on this machine), while the sparse path's cost is in branches and gathers that
no lane kernel helps.

## 5. The release gates of §34–§35

| gate | result | met |
|---|---|---|
| 1. XRoute screen ≥ 10× faster than the 4.2 screen at p50 | the full pair screen (`xscreen`: route + anchor-tier sparse screen, the same contract as `screen42`) is **2.8×** at p50; the route stage alone is 18.5 ns per candidate (≈ 2,400× per pair) but is not a screen on its own | **no** |
| 2. XRoute screen ≥ 5× at p95 | **15.2×** | yes |
| 3. XCompare unrelated ≥ 10× at p50 | fast policy **18–67×** per negative class (overall p50 26×); safe policy 1.8–4.1× | yes (fast) · no (safe) |
| 4. XCompare copy ≥ 10× at p50, or the aggregate search ≥ 10× | copies 1.4–5.1× per class under safe (up to 8× under fast); aggregate search 3.3–7.1× | **no** |
| 5. XRank 100-candidate ≥ 10× end-to-end | **7.1×** on the §3.1 reference workload natively, 3.3× in WebAssembly; 0.9× on a 25-keypoint query (its ten fallbacks cost more than they save) | **no** |
| 6. heap allocation in the XScreen hot path = 0 | 0 allocations / 200 calls | yes |
| 7. full descriptor-pair evaluations reduced ≥ 90 % | −99.0 % | yes |
| 8. fallback rate reported, within budget | 1.4 % (safe) | yes |
| §35 no copy-positive golden case hard-rejected by XRoute | 0 of 2,379 (and 0 on the JavaScript fixtures) | yes |
| §35 XMatch gives the same Copy class or DEFERs so that fallback recovers it | 0 copy disagreements under safe; 0 false Copies under fast | yes |
| §3.2 symmetry, mirror/direct equivalence, determinism across engines | `xcompare(a,b)` = `xcompare(b,a)` on verdict and execution; routes, screens, reports and rank records byte-identical native / SIMD128 / baseline (`test/x-wasm.mjs`, 3,449 checks) | yes |

**The 10× gates are not met**, and this release does not claim them (§41.7). What is met is
the accuracy contract and the architectural goal — the quadratic descriptor scan is gone
(−99 % Hamming pairs), the screen does not allocate, and every answer is either comparator 42's
or certified by the cascade's own evidence. The speed-up is 3–7× on the reference search
workload natively, 26× on the pairwise fast path, 2–4× on the WebAssembly search workload.

Where the remaining time goes (callgrind on a certified 512-keypoint mirrored copy, 2.9 M
instructions per `xcompare`): 47 % in 4.2's geometry on the sparse pools — a fifth of the whole
in the §7 exclusion loop, which visits every keypoint of the other side for every consumed
inlier; 30 % in the anchor-tier sparse scan (bucket probes, nomination bookkeeping, ~3,800
Hamming distances); 14 % in the structural channels; 9 % elsewhere. The copy path is bounded
below by the geometry it inherits, not by the retrieval it replaced.

## 6. Known gaps and next steps

- **No real corpus (M8).** X1 is calibrated on the synthetic corpus of §33.1 only. The route
  bars, the certificate and the anchor schedule must be re-derived on a moderation corpus before
  the profile loses its PROVISIONAL name or the fast policy becomes the default (§39 Phase 6).
- **States below Copy under the safe policy** equal comparator 42's on the whole corpus, but
  not by construction: where the sparse pools miss chance correspondences that 42's exhaustive
  pool finds, 42 can read a weak geometric signal (3–5 inliers, no model) as `Suspected` where
  X reads `Related`. `test/x-wasm.mjs` lists such pairs on the JavaScript shared-texture
  fixtures (8 of 576 ordered pairs); none is a copy disagreement.
- **Small queries.** A query with few keypoints (B11, 25 kp) defers often (10 of 12 compared
  candidates) because the certificate needs 12 inliers it cannot have; each deferral then costs
  the cascade plus EXACT42. A certificate scaled to the query's keypoint count, or skipping the
  cascade for tiny queries, is the obvious fix — it needs the real corpus to calibrate.
- **Geometry cost.** A grid over the other side's keypoints would make the §7 exclusion
  O(inliers) instead of O(inliers × keypoints) with identical output; the vote-table work per
  correspondence is inherent to 4.2's Hough stage.
- **Sparse scan cost.** Bucket probes are two dependent loads and a bisection each; a lane
  kernel over a row's 48 codes would help natively (BMI2 `pext` for the projections) but has no
  SIMD128 counterpart, which is why it is not done here.
- **The gate on recolours that scramble luminance.** On the PAPH-SI corpus (SPEC-SI §11) the
  gate drops 3 of 976 comparator-42 Copy pairs (6 of 1,952 queries): channel swaps and a
  palette shuffle whose anchor-tier pools are empty while comparator 42 certifies them on
  structure alone (no inliers). Two leave at the gate's first exit (route Reject, pool ≤
  `defer_pool_max`), one at its second (pools below `geo_min_corr` after the expansion tiers).
  Asking the structural channels before either exit drops an empty-pool pair would close it,
  at a cost per candidate the real corpus has to price. The X1 route bars, which put 49 of the
  976 in the route's Reject class, want the real corpus too.
- **`xprepare` is 0.2–1.5 ms** per side (the index and the route). The PAX1 sidecar removes it
  from the query path; an index should store it beside the wires.

## 7. Verification

```bash
cargo test --release --manifest-path rust/Cargo.toml     # 102 tests: 81 of 4.2.3 + 21 of PAPH-X
bash rust/check.sh                                       # the 3,160-case equivalence digest, unchanged
npm test && npm run test:wasm && npm run test:equiv      # 4.2.3's suites, parity, the digest in both wasm builds
npm run test:x                                           # test/x-wasm.mjs: native xcli vs SIMD128 vs baseline, 3,449 checks
rust/target/release/xbench                               # the §33–35 harness (≈ 10 s); --quick, --calibrate, --explain, --table, --noreject
npm run bench:x                                          # test/x-bench.mjs: the WebAssembly timings
```

The PAPH-X tests (`rust/src/x/*/tests`): MinHash lanes agree exactly on equal sets; the route
batch kernels equal the pairwise and scalar readings; the projection table is mirror-closed and
distinct; the CSR index holds every id once in order and random collisions touch tens, not
hundreds; the sparse matcher equals the exhaustive one when everything is nominated and finds a
mirrored copy through the mirror hypothesis; the X geometry equals `geom42` on identical pools;
the rewritten secondaries equal the v3 channels and the local bound holds above the exact value;
`sad64` equals the scalar loops; the cascade agrees with comparator 42 on the fixture family
(every state under safe, the copy question under fast); the screen rejects unrelated art and
passes copies, and never rejects what 42's screen passes on the noise fixture; reports are
history-free; rank records equal pairwise compares; the sidecar round-trips and refuses damage;
the profile round-trips, has an identity and refuses tampering.
