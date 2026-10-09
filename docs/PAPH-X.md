# PAPH-X — implementation record

PAPH-X is the retrieval-native comparison cascade specified in
[SPEC-X-paph-x.md](SPEC-X-paph-x.md): the same wires (format 4 from 1.2, [SPEC-W4](SPEC-W4-paph-wire4.md);
format 3 read alike), the same comparator-42 verdict vocabulary and calibration (CAL-007-PROVISIONAL
from 1.2, CAL-004-PROPOSED before), reached through a cheaper path — a 128-byte route
signature screened first, descriptor matching by locality-sensitive buckets instead of the 512 ×
512 Hamming scan, geometry on a 96-keypoint anchor tier that expands only when it must, structural
channels computed only while the verdict lattice can still move, and the exact comparator
(`EXACT42`) as the fallback and the audit. This document is what was built, how it departs from the
specification where it does, what it measures, and which of the specification's release gates it
meets. Every number here was printed by `rust/target/release/xbench`, `rust/target/release/sibench`
or `node test/x-bench.mjs` on this machine (one core of a 2.1 GHz Xeon, shared), as §41.7 asks,
except two callgrind profiles that say so; none of them is a claim beyond that.

Status: milestones M1–M7 of §40 are built, tested and verified across the native engine and both
WebAssembly builds. M8 (calibration) is done on synthetic corpora; 1.1.2 measured X2 on Pixagram's
own works — the artworks Hivemind lists on the Pixa chain at one snapshot (§4) — and 1.2 moved what
they showed: the wire's sampling (wire 4), comparator 42's moderate structural bar (CAL-007), and
the profile bound to it. Three profiles:

* **X3-PROVISIONAL** (`8af84dd0abb12192…`, 332 bytes) — the shipped default from 1.2: X2's
  schedule, bars, route derivation and door, bound to CAL-007-PROVISIONAL (`741afad9252f2ccb…`).
  Only its name and base identity differ from X2's bytes. The expansion exit 1.1.2 proposed was
  measured and is not in it (§6).
* **X2-PROVISIONAL** (`27993afaaca76d11…`, 332 bytes) — 1.1.1–1.1.2's default, bound to
  CAL-004-PROPOSED (`91b545f801f5a095…`): X1 with three changes and the same bars — route
  derivation 2 (§2), the structural door on every screen exit (§2), and the version-2 artefact
  layout that records both (`docs/calibration/X2-PROVISIONAL.pxcl`; `--x2` in the harnesses).
* **X1-PROVISIONAL** (`b96040d888b21e28…`, 330 bytes) — 1.0.0's, byte for byte, bound to CAL-004
  (`docs/calibration/X1-PROVISIONAL.pxcl`; `--x1`). §4's main tables and §5 were measured under it.

An X profile is bound to one calibration: X1 and X2 load with CAL-004, and X3's schedule loads
with any base (`XProfile::shipped_for`). The chain's 177 works are not a moderation corpus —
comparator 42 finds two copy pairs among them, each within one author's works, and the rest of the
copies measured are synthetic transforms — so every profile keeps the PROVISIONAL name and the
fast policy is not the default.

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
`identical`, `uncertified-geometry`, `pool-truncated`, `control-saturated`, …). The verdict
lattice itself is comparator 42's (SPEC-004.2 R1–R4): the structural and the geometric axes
are never averaged, the thresholds are the profile's, and a verdict is stated only when every
corner of the evidence intervals agrees on it (§10.4).

### Where it lives

| spec | module | what |
|---|---|---|
| §6, §12.1 | `rust/src/x/route.rs` | `XRoute` (136-byte record), b-bit MinHash over the Tier-1 local codes and the descriptor bands, D4- and inversion-invariant global words (DCT symmetrised magnitudes, run classes, RAG cells, shape classes — route derivation 2 from X2), `route_score`, `route_class`, `RouteSoA` + `route_batch` (SSE2 / SIMD128 lane kernels with a scalar twin held equal by test) |
| §8.2–8.3 | `rust/src/x/bucket.rs` | `project`, `mirror_proj`, `XBucketIndex` (CSR with presence masks, bisection, per-entry hot flag) |
| §9.1 | `rust/src/x/anchor.rs` | `anchor_order`: 4.2's quality selection in pick order |
| §8.4–8.8, §24 | `rust/src/x/matcher.rs` | `MatchScratch`: nomination by support, packed state, fixed scratch; `count` (the §24 screen count without building correspondences), `pools` (the accepted correspondences in 4.2's order) |
| §9.2–9.5 | `rust/src/x/geom.rs` | `measure`, `control`, `diversity`, `topology`, `coverage_into` — 4.2's geometry (`geom42`) on reusable scratch, two-pass §7 consumption |
| §10, §27–28 | `rust/src/x/structural.rs` | the six secondaries rewritten allocation-free (held equal to the v3 channels by test), `local_bound`, `local_exact`, `Structural` with `bounds`/`channels_corner`/`compute`/`exact` |
| §11–13, §23, §29 | `rust/src/x/compare.rs` | `xscreen`, `xcompare`, `xcompare_in`, the lattice corners (`decide`, `all_below_suspected`), the deferral rules, `structural_door` (X2 on), `XCtx` (with the per-tier geometry trace), `report_json` |
| §7, §14, §19 | `rust/src/x/rank.rs` | `xrank`: Stage A route table (SIMD), Stage B sparse screen, Stage C cascade; 24-field `i32` records |
| §15, §17 | `rust/src/x/prepared.rs` | `XPrepared` = `Prepared` + route + index + anchor order + burst weights + measurability bits |
| §18 | `rust/src/x/sidecar.rs` | PAX1: route + index + order, CRC-32, rebuilt on any mismatch; version 2 (1.2) binds it to its side — the Tier-1 checksum and format, Tier 2 or the sketch — and refuses version 1 |
| §20 | `rust/src/x/profile.rs` | `XProfile` (the `.pxcl` artefact: version, comparator 50, bars, seeds, projection table, caps, anchors, certificate, policy, and from version 2 the route derivation and the gate door; SHA-256 identity; tamper-refusing decode), `XBound` (a profile bound once: ids, salts, projections) |
| §30 | `rust/src/x/abi.rs`, `wasm/paph.js`, `wasm/paph.d.ts` | the C ABI (ABI 4 / X ABI 1), the JavaScript glue and its types |
| §33–35 | `rust/src/bin/xbench.rs`, `test/x-bench.mjs` | the benchmark and acceptance harness (native) and the WebAssembly timings |
| §36 | `rust/src/x/*/tests`, `test/x-wasm.mjs`, `rust/src/bin/xcli.rs` | equivalence: exact where the specification asks for exactness, semantic against comparator 42 elsewhere, byte-identical across engines |

`rust/src/synth.rs` holds the picture generators (moved out of `equiv.rs`, digest unchanged) that
the harness, the tests and the equivalence digest share. PAPH-X changes neither comparator 42 nor
the wire: the equivalence digest of wire 3 (3,160 cases, 1.0–1.1's) is byte-identical, natively and
in both WebAssembly builds, beside wire 4's (3,164), and `test/wasm-parity.mjs` still passes 625
ordered pairs byte for byte. A wire-3 side and a wire-4 side are never compared: `xscreen`,
`xcompare` and `xrank` refuse the pair, as comparator 42's `compare` and `rank` do
(`WIRE_MISMATCH`; SPEC-W4 §9).

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

**Route hard-negatives read `Unrelated` only under the fast policy** — and under X2 only
with the structural door shut (below). The route bars of X1
(`local` 6 / `band` 3 / `global` 190, the largest that reject no comparator-42 Copy on the
corpus; fast bars 7 / 5 / 203 above every negative) were calibrated for the *copy* question.
A pair the route rejects with no anchor-tier evidence reads `Unrelated` on the fast path —
calibrated rejection, §12.2 — and on the corpus 449 of 2,379 pairs read `Related` or
`Suspected` under comparator 42 that way under X1, 448 under X2 (under X1, six of them
`Suspected`: dithered or resampled copies 42 only suspects). The safe policy promises 42's
states, so it takes the sparse path for those
pairs instead and reads what 42 reads — on the corpus, every state of every pair. The profile
knob `route_reject_unrelated` turns the shortcut off for the fast path too.

**Route derivation 2 (X2).** The global words are meant to be invariant under the square's
symmetries and the complement; measured on the 120 bases of the PAPH-SI corpus and their
mirrored, rotated and transposed copies (`sibench route`), X1's were not. Two defects were the
route's own and are fixed: G1 held the main-diagonal run histogram, which a mirror sends to the
anti-diagonal the wire does not hold (equal on 58–59 % of D4 copies; derivation 2 holds the
horizontal / vertical anisotropy there instead: 100 %), and G3 kept the shapes section's region
order, which breaks area ties by position (72–76 %; as a sorted list, 83–84 %). The rest is the
wire's and cannot be fixed by a route derivation: the DCT section itself differs between a work
and its D4 copy whenever the 16 × 16 thumbnail's cells (edges at ⌊i·w/16⌋) do not commute with a
flip — a side not a multiple of 16 — and under quarter turns, where the integer DCT's two rounded
passes swap. Over the lowest 8 × 8 frequencies a magnitude bit differs on 2.4–13.7 % of D4 copies
and a sign bit on 1.3–9.6 %, so G0 equals its original's on 87 % of D4 copies of 16-multiple
canvases and 41 % of the others under either derivation: a word read from those bits is
invariant only where they are. G3's remaining differences come from the shapes section, which
samples a grid of its own — ⌈w / c⌉ × ⌈h / c⌉ cells with c = ⌈long side / 128⌉, edges rounded down
from the top-left corner — and keeps eight regions, breaking a tie in area by position: G3 holds
on 217 of 220 D4 copies of canvases whose long side is at most 128 px, where that grid is the
pixel grid, and on 185 of 260 above. The route reads G0 and G2 as overlaps and G1 and G3 as
symbol agreement, so it tolerates what remains; an exactly invariant G0 needs symmetric
sampling in the hasher, which changes the wire.

**Wire 4 (1.2) is that sampling** ([SPEC-W4](SPEC-W4-paph-wire4.md)): closed thumbnail and
grid cells, the DCT rounded once, regions ordered by keys no symmetry moves and cast from the exact
centroid, an exact silhouette. On wire-4 sides all four words equal their original's on every D4
copy — 696 of 696 on the chain's real bases, where wire 3 gave G0 25 % and G3 46 % on canvases
whose sides are not multiples of 16, and 480 of 480 on the synthetic corpus — and the route keeps
reading G0 and G2 as overlaps, which a wire-3 side a host still holds needs. The route bars did not
move: on wire 4, 255 of the chain's 3,095 real-base copies sit in the Reject class (wire 3: 262)
and 46 of the synthetic corpus's 995 (wire 3: 42 of 976); the pair screen rejects none.

**The structural door (X2).** X1's screen dropped a pair on the route class and the anchor pools: a
route hard negative with at most `defer_pool_max` correspondences was `Reject` in `xscreen`,
`Unrelated` under the fast policy and gated out of XRank, and XRank also gated out every candidate
whose pools stayed below `geo_min_corr` through the expansion tiers. The anchor tier counts
keypoint correspondences, and a work with almost no keypoints gives it none; XRank keeps a copy of
such a work when its route reads the pair as Fast, which it does not gate, and a recolour can move
the route out of that class while comparator 42 still certifies the copy on structure alone. On the
PAPH-SI corpus three such copies were lost (6 of 1,952 queries): two channel swaps and a palette
shuffle of works with 0–3 keypoints a side, two at the first exit (route Reject) and one at the
second (route Defer) (`sibench lost --x1`). Under X2 none of those exits drops a pair until the
structural door has shut, and the door keeps the first two; route derivation 2 reads the third as
Fast (`sibench lost`). The door asks the structural channels, in a near-cheapest order, until the
upper bound of the weighted structural score — unknown channels at their maximum, the local channel
at its edge-set bound — falls below the bar of the lattice's recolour arm (the larger of the strong
and the solo bars: 6000 under CAL-004), or the pair is not certifiable at all (the local channel
not measurable, fewer than three secondaries). The exits already read thin anchor pools as the
geometric arms being out of reach, as X1 did; the door answers for the one arm that needs no
geometry, and answers exactly: a pair whose door shuts cannot be a structure-only Copy, on any
corpus, and one whose door stays open is compared. Its order — runs, silhouette, the local bound,
topology, shape, DCT, palette — costs 20.6 µs per certifiable unrelated pair of the corpus, within
0.1 µs of the cheapest of all 5,040 orders and against 28.4 µs in the cascade's own
(`sibench doorprof`); over all 3,360 unrelated pairs — 500 of them shut before any channel — it
costs 17 µs a pair natively, and it shut on every one. On the chain's real bases (§4, 1.1.2) every
pair is certifiable, the door costs 66 µs a pair, and this order is 13 µs dearer than the cheapest
there. The route bars stay X1's: keeping every copy out of the Reject class would have taken bars
of 1 / 3 / 164, which leave 55 % of unrelated pairs in the class instead of 93 %, and the class is
no longer a filter on its own.

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
negatives; 2,379 pairs across the classes of §33.2). Every table before the last three
subsections, the WebAssembly one included, is 1.0.0's, under X1; two runs agreed within a few
percent and the figures are the first. The next subsection puts X2 beside X1 in one 1.1.1 run, the
second measures both on the chain's works (1.1.2), and the last puts X3 beside X2 on wire 4
(1.2.0).

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

In 1.0.0's run the WebAssembly ratios are lower than the native ones because comparator 42's
exhaustive descriptor scan is the part of 42 that SIMD128 accelerates best (2.4 ns per pair
there against 5.5 ns natively on this machine), while the sparse path's cost is in branches and
gathers that no lane kernel helps. In 1.1.1's run comparator 42 was faster natively, and the
reference workload's ratio under X2 is 3.2× natively and in WebAssembly (X1: 3.2× and 3.4×;
below).

### 1.1.1 — X2 beside X1, one run

`docs/calibration/X2-PROVISIONAL.log` holds every run below (`xbench`, `xbench --x1`,
`sibench …`, `node test/x-bench.mjs [--x1]`). Comparator 42 ran slower in 1.0.0's run than in
this one (screen 42 p95 1,691 µs there, 553–570 µs here), so the ratios of the tables above do
not carry over; the two columns here are the same machine, minutes apart.

**What the fixes buy** — the PAPH-SI corpus (120 bases × 20 transforms, 976 comparator-42 Copy
pairs, 1,952 queries in both arrival orders; `sibench lost`, `sibench route`, and
`sibench eval [--x1]` for the funnel):

| | X1 | X2 |
|---|---:|---:|
| Copy queries XRank does not read Copy, shown the target alone | 6 | **0** |
| Copy pairs the pair screen rejects (`xscreen` Reject) | 2 | **0** |
| Copy pairs in the route's Reject class | 49 | 42 |
| the PAPH-SI funnel, end to end (SI ∪ keys → XRank, 832 queries) | 99.5 % | **99.8 %** |
| global words equal on D4 copies, G1 / G3 | 58–59 % / 72–76 % | **100 %** / 83–84 % |
| xbench: screen hard-rejected a 42-Copy · safe copy disagreements · fast false Copy | 0 · 0 · 0 | 0 · 0 · 0 |

**What they cost** — one core, natively unless marked:

| | X1 | X2 |
|---|---:|---:|
| xscreen, p50 / p95 (µs; xbench, 2,379 pairs) | 16.6 / 116 | 34.7 / 117 |
| xcompare fast, p50 / p95 | 19.2 / 561 | 36.9 / 577 |
| xcompare safe, p50 / p95 | 181 / 611 | 177 / 607 |
| xscreen p50 on unrelated same-style / busy 512-kp / random pairs | 8.1 / 28.3 / 18.9 | 22.4 / 52.6 / 40.9 |
| xrank, §3.1 reference (B09 vs 100 of the 512-kp class; rank 42 84–89 ms) | 26.3 ms | 27.9 ms |
| xrank, N = 1,000: B09 / B11 (25 kp) / aggregate of three | 120 / 23 / 143 ms | 137 / 47 / 184 ms |
| xrank in the PAPH-SI funnel, per candidate: median query / mean / unrelated pools | 153 / 374 / 593 µs | 196 / 399 / 616 µs |
| WebAssembly: xscreen on 66 unrelated same-style pairs, mean (screen 42: 55 µs) | 8.2 µs | 22 µs |
| WebAssembly: xrank, §3.1 reference (rank 42 106–109 ms) | 31.0 ms (3.4×) | 33.8 ms (3.2×) |

The door costs about 17 µs natively on an unrelated pair (§2), paid wherever a screen exit would
have dropped the pair. That doubles the pairwise screen's median and the fast policy's — their
median pair is an unrelated one the route rejects — and leaves the safe compare unchanged. In
ranking the door is asked only on the candidates the gate would drop, and X2 costs +6 % on the
reference workload, +7 % on the mean candidate of the PAPH-SI funnel, +14.5 % for the
512-keypoint query against 1,000 candidates, and double for the 25-keypoint query, whose pools
are thin against every candidate (route derivation 2 also moves which candidates the gate sees;
no run separates the two). WebAssembly pays the same
(`node test/x-bench.mjs [--x1]`): about 14 µs on an unrelated same-style pair, +9 % on the
reference workload; on the JavaScript fixtures of the table above, whose pairs pass the anchor
screen, the two profiles time alike.

### 1.1.2 — X2 on the chain's works

The first measurement on Pixagram's own works: the artworks Hivemind lists on the Pixa chain at
one snapshot (177 works, head block 987,642; `node tools/chain-corpus.mjs`,
docs/SPEC-SI-paph-si.md §9.7). `docs/calibration/SI3-PROVISIONAL.log` holds the runs:
`sibench chain` over every pair of the 177 works (15,576 pairs, 31,152 queries in both arrival
orders), and `sibench route`, `lost` and `doorprof` on the real-base corpus — 174 of the works
under the twenty transforms of the PAPH-SI corpus, 3,095 of the transformed pairs
comparator-42 Copy. Nothing of X2 changed.

| on the chain's works | X1 | X2 |
|---|---:|---:|
| real-base Copy queries XRank does not read Copy, shown the target alone (of 6,190) | 0 | **0** |
| real-base Copy pairs the pair screen rejects (of 3,095) | 0 | **0** |
| real-base Copy pairs in the route's Reject class | 212 | 262 |
| unrelated pairs of real bases in the Reject class (of 30,098: every ordered pair of distinct bases but the 2 pairs comparator 42 calls Copy) | 93.5 % | 93.5 % |
| the largest bars that keep every real copy out of the class, and the unrelated share they leave in it | 1 / 8 / 145: 19.6 % | 1 / 6 / 141: 15.3 % |
| queries between distinct works XRank reads Copy (of 31,148 that comparator 42 does not call Copy) | 0 | **0** |
| global words equal on D4 copies, G1 / G3 (both sides multiples of 16 · the others) | 64 % · 78 % / 79 % · 44 % | **100 % · 100 %** / 95 % · 46 % |

Every state XRank states on the 31,152 queries between distinct works equals comparator 42's:
Unrelated 506, Related 18,550, Suspected 442, Copy 4 (the two pairs comparator 42 calls Copy, each
within one author's works), NotCopy 10,828 (copy scope: below Suspected, not resolved further), and
822 gated out. On the 225 pairs comparator 42 calls Suspected its gate drops 8 of the 450 queries;
comparator 42's own gated rank screens out 30 of the pairs. Route derivation 2 puts more real blurs
and re-dithers in the Reject class than derivation 1 (resample90 25 against 16, up150 15 against 6,
dither 51 against 20); nothing acts on the class alone. The structural door shuts on all 15,049
unrelated pairs of real bases, at 66 µs a pair (`doorprof --real`; 17.4 µs on the PAPH-SI corpus's
unrelated pairs), and XRank's gate drops 822 of the 31,152 queries, each after the door shut. Every
real pair is certifiable, and the door's order, within 0.1 µs of the cheapest on the PAPH-SI corpus
(§2), is 13 µs dearer than the cheapest here: 65.6 against 52.3 µs per pair.

**What it costs on real pairs** — one core, natively, over the 15,576 pairs:

| | per pair or query |
|---|---:|
| comparator 42, lean | 1,327 µs |
| comparator 42's gated `rank` (`paph_rank42` with gate: the stage-1 screen, which passes 79.4 % of real pairs, then comparator 42 on those) | 1,223 µs |
| the pair screen, `xscreen` (X2) | 127.3 µs |
| XRank under X2, one candidate per call | **786 µs** (1.6× faster than the gated rank) |
| — of it, the 822 queries the gate drops | 97 µs each |
| — the 30,300 the cascade decides | 803 µs each |
| — the 30 that fall back to EXACT42 | 1,941 µs each |
| XRank under X1 | 781 µs |

On the synthetic reference workload — one query against 100 candidates in one call — XRank is
3.2× comparator 42's ranking; over the real pairs, one candidate per call, 1.6×. The cascade
runs whole: on 30,326 of the 30,330 queries that reach it (the 30,300 it decides and the 30 it
hands to EXACT42), the sparse scan reads every row of the smaller side. The anchor tiers (96 →
160 → 256 → 512 rows) expand until a model certificate holds or the rows run out, and on these
pairs they ran out. The scan still evaluates only
1.6 % of the descriptor distances the exhaustive scan computes; a callgrind profile outside the
log (a build with symbols, three works against the other 176: 528 XRank calls) puts 38 % of the
instructions in it. Every structural channel is computed exactly on 19,110 of the 30,330
queries (63 %): comparator 42 reads 81 % of real pairs Related, and under copy scope the cascade
stops short of exact evidence only once no corner of the lattice can reach Suspected. §6 says
what would change this.

### 1.2.0 — X3 on wire 4, beside X2

`docs/calibration/X3-PROVISIONAL.log` holds the runs — both corpora re-hashed in wire 4; `route`
on the chain's real bases, `lost` on both corpora, `xbench` and the WebAssembly benchmark under X3
and again with `--x2`, under X2 (bound to CAL-004, on the same wire-4 sides); the synthetic
`route`, `doorprof` and `xtrace` under X3; `xtime`, X2's schedule under each calibration; the
route's tables on both corpora's wire-3 hashes under X2 for comparison; and the pair behind the fix
below, through `xcli` — except `sibench chain`, every pair of the chain's works under both
profiles, which is the first run of `SI4-PROVISIONAL.log`. X3 is X2's schedule bound
to CAL-007, so what differs between the two columns is the calibration alone: the moderate
structural bar, 3300 instead of 2400 (SPEC-004.2 §19).

| | X2 (CAL-004) | **X3 (CAL-007)** |
|---|---:|---:|
| the chain: pairs of distinct works comparator 42 reads Suspected (of 15,576; two authors' of 14,412) | 259 (222) | **0 (0)** |
| the chain: queries XRank reads Copy / should (comparator 42's 2 Copy pairs, both orders) | 4 / 4 | 4 / 4 |
| the chain: queries between distinct works XRank reads Copy (of 31,148) | 0 | 0 |
| real-base Copy queries XRank does not read Copy, shown the target alone (of 6,190) | 0 | **0** |
| synthetic Copy queries XRank does not read Copy (of 1,990) | 0 | **0** (2 before the fix below) |
| real-base Copy pairs in the route's Reject class (of 3,095) · rejected by the pair screen | 255 · 0 | 255 · 0 |
| xbench, 2,379 pairs: screen hard-rejected a 42-Copy · safe copy disagreements · fast false Copy · fallbacks | 0 · 0 · 0 · 1.4 % | 0 · 0 · 0 · 1.4 % |
| XRank a query over the chain's pairs, one candidate per call (comparator 42's gated `rank`: 1,100 µs) | 717 µs | **648 µs** (1.7×) |
| the same, X2's schedule under each calibration (`sibench xtime`, 9,599 queries between two authors' works) | 722 µs | 642 µs |
| reference workload, B09 vs 100 of the 512-kp class: natively · in WebAssembly | 3.1× · 3.1× | 3.2× · 3.3× |

**Why X3 is faster on real pairs.** Not by doing less geometry: on the 30,330 queries that reach
the cascade under X3 the sparse scan still reads every row of the smaller side on all but 4, as it
did under X2 on wire 3 (1.1.2). Under copy scope the cascade stops as soon as no corner of the lattice can reach
Suspected, and on the chain's pairs the corner that kept it open was the structural partial
agreement: structure between 2400 and the strong bar. With the bar at 3300 the stop comes before
the expensive channels — every structural channel is computed exactly on 24 % of the queries
under X3 (1.1.2, X2 on wire 3: 63 %) — and `sibench xtime` puts the difference at 80 µs of a
722 µs query (11 %) with nothing else changed; over all the chain's pairs 717 → 648 µs (10 %).
Where the time goes, per query between two authors' works under X3: 405 µs in the sparse scan of
every row (comparator 42's two exhaustive scans would take 732 µs), 42 µs measuring the four
anchor tiers, 223 µs if every structural channel is computed exactly; 16.8 candidates nominated
a row and 16.6 distances computed.

**The fix the measurement found.** The first 1.2 run lost two synthetic copy queries under X3
(`sibench lost`): the PAPH-SI corpus's base 29 pasted into its host, both orders, which comparator
42 certifies on geometry alone ("geometric only — crop / collage class", 47 inliers). On the
sparse pools the GN control saturated — the permuted pools matched as well as the real ones (48
inliers, control 10000, evidence 0) — and with the structural interval at 563–3085, its upper
bound under CAL-007's moderate bar, every corner then read below Suspected, so the copy-scope stop
(`NotCopy`, `FAST`) came before the deferral the cascade takes for a saturated control (a
structured accident the exhaustive comparator must re-judge). Under CAL-004 the same bound kept
the lattice open, the full verdict was reached (structure exactly 1900) and deferred, and EXACT42
read Copy. The deferral now also covers the copy-scope stop: when geometry at its potential would
lift a corner to Suspected and the control saturated, the pair falls back (`control-saturated`),
in every profile. Both queries now read Copy, X2 loses none (`lost --x2`, both corpora), and X3's
states on all 31,152 of the chain's queries are as they were (still 30 fallbacks). The X3 log
prints the pair through `xcli` (`sibench dump` writes its wires), every number above in it. On the
WebAssembly fixture ranking of `test/x-bench.mjs` X3 falls back on 9 of 103 candidates, X2 on 8.
A test pins the behaviour on a constructed pair (`copy_scope_defers_a_saturated_control`).


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
| §3.2 symmetry, mirror/direct equivalence, determinism across engines | `xcompare(a,b)` = `xcompare(b,a)` on verdict and execution; routes, screens, reports and rank records byte-identical native / SIMD128 / baseline (`test/x-wasm.mjs`, 3,464 checks in 1.1.1) | yes |

These are 1.0.0's figures, under X1. Under X2, in one run beside X1 (§4, the 1.1.1 subsection),
the pairwise screen is 0.9× screen 42 at p50 (X1 1.9× in that run) and 4.7× at p95 (X1 4.9×),
the fast compare 10.6× at p50 (X1 20.5×), the safe compare 2.2× (both), and the §3.1 reference
search 3.2× (both; in WebAssembly 3.2×, X1 3.4×); the accuracy rows hold unchanged, and the
PAPH-SI corpus adds the copies X1's screen lost (6 queries → 0). Under X3, on wire 4 (1.2, §4's
last subsection): the screen 1.0× at p50 and 4.7× at p95, the fast compare 10.5×, the safe compare
2.3×, the reference search 3.2× natively and 3.3× in WebAssembly, every accuracy row unchanged.

**The 10× gates are not met**, and this release does not claim them (§41.7). What is met is
the accuracy contract and the architectural goal — the quadratic descriptor scan is gone
(−99 % Hamming pairs), the screen does not allocate, and every answer is either comparator 42's
or certified by the cascade's own evidence. In 1.0.0's run, under X1, the speed-up was 7.1× on
the reference search workload natively (3.3–7.1× on the aggregate searches), 26× on the
pairwise fast path and 3.3× on the WebAssembly reference workload; in 1.1.1's run, under X2,
3.2×, 10.6× and 3.2×.

Where the remaining time goes (callgrind on a certified 512-keypoint mirrored copy, 2.9 M
instructions per `xcompare`): 47 % in 4.2's geometry on the sparse pools — a fifth of the whole
in the §7 exclusion loop, which visits every keypoint of the other side for every consumed
inlier; 30 % in the anchor-tier sparse scan (bucket probes, nomination bookkeeping, ~3,800
Hamming distances); 14 % in the structural channels; 9 % elsewhere. The copy path is bounded
below by the geometry it inherits, not by the retrieval it replaced.

## 6. Known gaps and next steps

- **No moderation corpus (M8).** X1's parameters, which X2 and X3 keep, are calibrated on the
  synthetic corpus of §33.1. 1.1.2 measured them on the chain's works (§4) — no copy of a real
  base lost, no false Copy between real works, every stated state equal to comparator 42's — and
  1.2 fitted one calibration bar on them (CAL-007), but 177 works among which comparator 42 finds
  two copy pairs are not a moderation corpus. The route bars, the certificate and the anchor
  schedule must be re-derived on one before the profile loses its PROVISIONAL name or the fast
  policy becomes the default (§39 Phase 6).
- **Real pairs run the whole cascade, and the exit 1.1.2 proposed was measured and not taken.**
  Over the chain's pairs XRank under X3 costs 648 µs a query natively against 1,100 µs for
  comparator 42's gated `rank` (1.7×; 1.1.2, X2 on wire 3: 786 against 1,223 µs), and its sparse
  scan reads every row of the smaller side on all but 4 of the 30,330 queries that reach the
  cascade: the anchor schedule (96 → 160 → 256 → 512 rows) expands until a certificate holds or
  the rows run out. 1.1.2 proposed an exit from the expansion once no corner of the lattice can
  reach Suspected whatever the remaining tiers add. 1.2 traced every query tier by tier
  (`sibench xtrace`, `XCtx::trace`): on the 28,094 queries between two authors' works that reach
  the geometry, no tier ever finds a model — an exit would stop nearly all of them — but on both
  corpora some copies find their model only after the anchor tier: 156 of the 6,890 real-base
  copy queries and 86 of the 1,988 synthetic ones, 50 and 22 of them with no weak signal at the
  anchor tier either. Every rule measured — stop after tier k when no tier so far found a model
  and the weak signal stayed at or below w — puts copies at risk:

  | exit after | w | unrelated queries stopped (of 28,094) | rows saved a query | copy queries at risk |
  |---|---:|---:|---:|---:|
  | the anchor tier (96 rows) | 0 | 27,708 | 370 | 122 |
  | 160 rows | 0 | 26,662 | 311 | 56 |
  | 256 rows | 0 | 25,836 | 220 | 26 |
  | 256 rows | 5 | 26,214 | 223 | 40 |

  A bound on what a tier can add would have to hold for those copies, and none that the trace
  supports does; so X3 keeps X2's schedule, and the speed-up it brings on real pairs is the
  calibration's (§4). The rows are what remains: the sparse scan is 405 of a query's 642 µs. Per-
  side caches of the decoded structural sections for ranking, and a cheaper probe per row (below),
  are what is left to try.
- **States below Copy under the safe policy** equal comparator 42's on the whole corpus, but
  not by construction: where the sparse pools miss chance correspondences that 42's exhaustive
  pool finds, 42 can read a weak geometric signal (3–5 inliers, no model) as `Suspected` where
  X reads `Related`. `test/x-wasm.mjs` lists such pairs on the JavaScript shared-texture
  fixtures (8 of 576 ordered pairs); none is a copy disagreement.
- **Small queries.** A query with few keypoints (B11, 25 kp) defers often (10 of 12 compared
  candidates) because the certificate needs 12 inliers it cannot have; each deferral then costs
  the cascade plus EXACT42. A certificate scaled to the query's keypoint count, or skipping the
  cascade for tiny queries, is the obvious fix. On the chain, small queries are rare: one work in
  177 has fewer than 8 keypoints, the median has 512 (SPEC-SI §9.7), and 30 of 31,152 queries
  between its works fall back to EXACT42.
- **Geometry cost.** A grid over the other side's keypoints would make the §7 exclusion
  O(inliers) instead of O(inliers × keypoints) with identical output; the vote-table work per
  correspondence is inherent to 4.2's Hough stage.
- **Sparse scan cost.** Bucket probes are two dependent loads and a bisection each; a lane
  kernel over a row's 48 codes would help natively (BMI2 `pext` for the projections) but has no
  SIMD128 counterpart, which is why it is not done here.
- **The structural door's cost (X2 on).** About 17 µs natively on an unrelated pair of the PAPH-SI
  corpus (§2) and 66 µs on a pair of the chain's works, where its order is 13 µs dearer than the
  cheapest (§4; on wire 4: 61.3 µs, its order 61.3 µs against the cheapest 49.9 µs per certifiable
  pair), paid wherever a screen exit would drop a pair: the price of an exact bound over
  seven channels, most of it the local channel's edge set (16k popcounts) and the shape and
  topology channels. A cheaper sound bound per channel, or per-side caches of the decoded sections
  for ranking, would lower it; small queries, thin against every candidate, pay it on every
  candidate.
- **Comparator 42's own gated rank drops copies of works with few keypoints.** 4.2.3's `rank` with
  `gate: true` (`paph_rank42`, flags bit 0) — the call `docs/SEARCH.md` §4 and
  `integrations/pixagram-search` make — screens out every pair whose exhaustive stage-1 pools stay
  below `geo_min_corr` (8), a pool no larger than the smaller side's keypoint count. On the PAPH-SI
  corpus that is 178 of the 976 comparator-42 copies, 164 of them — the three structure-only
  recolours above among them — because a side has fewer than 8 keypoints and so can never pass
  (`sibench lost`). 1.1.1 called that far more than XRank drops, and it is, on that corpus; on the
  chain's works the same gate screens out 8 of the 3,095 real-base copies, 4 of them for keypoints
  — one artwork in 177 has fewer than 8 — and 30 of the 225 pairs comparator 42 calls Suspected
  (XRank's gate: 8 of the 450 queries on them). XRank does not gate a candidate its route reads as
  Fast — 172 of the 178 under X2, all 8 of the real ones — and under X2 reads Copy on all of them.
  Use `xrank` (X2 on), or `rank` with `gate: false`. On wire 4 the same gate screens out 194 of the
  synthetic corpus's 995 copies (177 for keypoints) and 9 of the chain's 3,095 (5 for keypoints);
  XRank under X3 reads Copy on all of them. SPEC-004 §A4.1.4 calls the screen advisory: a
  screened-out pair is unscreened, never Unrelated, and the screen "may never substitute" for the
  comparator.
- **G0 and G3 are as invariant as the wire's sections** (§2) — closed by wire 4 for wire-4 sides:
  on wire 3 the 16 × 16 thumbnail's cells and the integer DCT's rounding (G0), the shapes
  section's grid above 128 pixels and its eight-region tie (G3) made them differ on D4 copies (on
  the chain's works whose sides are not multiples of 16, G0 equal on 25 % and G3 on 46 %); wire 4
  samples symmetrically and both hold on every D4 copy of both corpora. A host that keeps wire-3
  sides keeps wire 3's limits.
- **The route class is a sketch, calibrated on one synthetic corpus.** X2 and X3 keep X1's bars,
  calibrated on the xbench corpus; measured on the PAPH-SI corpus, 42 of its 976 copies score
  in the Reject class, and no screen exit drops them on the class alone. On the chain's real
  bases 262 of 3,095 copies do (X1: 212), mostly blurs, re-dithers and corner crops, and bars
  that keep every one out would leave 15.3 % of the unrelated pairs of real bases in the class
  instead of 93.5 %: the real works confirm the decision to keep the bars and act on the class
  only behind the door. On wire 4 the class holds 255 of the 3,095 real copies (bars keeping all
  of them out would leave 21.4 % of unrelated real pairs in it) and 46 of the synthetic 995.
- **`xprepare` is 0.2–1.5 ms** per side (the index and the route). The PAX1 sidecar removes it
  from the query path; an index should store it beside the wires. From 1.2 a sidecar names the
  side it was derived from (the Tier-1 checksum and format, and whether Tier 2 was read), so a
  store re-hashed in wire 4 derives each side once more and stores the new sidecar; 1.1's
  version-1 sidecars are refused the same way.

## 7. Verification

```bash
cargo test --release --manifest-path rust/Cargo.toml     # 126 tests: 81 of 4.2.3, 21 of PAPH-X, 7 of PAPH-SI, 4 of 1.1.1, 1 of 1.1.2, 12 of 1.2
bash rust/check.sh                                       # both equivalence digests: wire 3 (3,160 cases, unchanged) and wire 4 (3,164)
npm test && npm run test:wasm && npm run test:equiv      # 4.2.3's suites and wire 4's golden vectors, parity, both digests in both wasm builds
npm run test:x                                           # test/x-wasm.mjs: native xcli vs SIMD128 vs baseline, 3,466 checks
rust/target/release/xbench [--x2 | --x1]                 # the §33–35 harness (≈ 10 s); --quick, --calibrate, --explain, --table, --noreject
npm run bench:x [-- --x2 | --x1]                         # test/x-bench.mjs: the WebAssembly timings
rust/target/release/sibench lost|route|doorprof [--x2 | --x1]   # the PAPH-SI corpus: the screen's losses, the route class, the door
rust/target/release/sibench chain                        # the chain's works, every pair (a snapshot: node tools/chain-corpus.mjs)
rust/target/release/sibench lost --corpus chain.bin      # the real-base corpus (sibench corpus --chain): XRank's losses
rust/target/release/sibench route|doorprof --real --corpus chain.bin   # its route class and door: every pair of distinct bases but those comparator 42 calls Copy
rust/target/release/sibench xtrace | xtime               # 1.2: the cascade's geometry tier by tier; where XRank's time goes
```

1.2's tests: the copy-scope stop after a saturated control defers (a constructed pair of the
first 1.2 run's kind, under X3 and X2, XRank in both orders, the fast policy deferred); X3's
artefact is X2's but for its name and base identity and is pinned byte for byte, X2's still
decodes and binds to CAL-004; CAL-007 is CAL-004 with one bar moved and its artefact pinned; wire
4's sections are exactly equivariant on 532 D4 copies, where wire 3's move on 493, and the
silhouette's ties are settled alike in every orientation (`rust/src/wire4.rs`,
docs/SPEC-W4-paph-wire4.md §7); a PAX1 sidecar is refused by the other format's side of the same
work, by its Tier 1 read without Tier 2, and in 1.1's version 1; and Tier 1 parsing holds the
section table's counts to what each section holds. `test/x-wasm.mjs` checks the shipped profile is X3 byte for byte,
loads X1 and X2 from their artefacts with CAL-004 (and refuses X2 against CAL-007), and runs the
door cases under X1 and X3.

1.1.1's tests: X1's artefact is pinned byte for byte and X2 round-trips (a version-1 artefact
cannot claim version-2 behaviour); route derivation 2's G1 and G2 are equal on every D4 copy of
twelve generated works while derivation 1's G1 moves on 18 of 48; two channel swaps comparator 42
certifies on structure alone are dropped by X1's gate (the first also by its pair screen and
fast policy) and kept by X2's; SI2 is SI1 bound to X2. `test/x-wasm.mjs` checked that the shipped
profile was X2 byte for byte (1.2: X3), loads X1 from its artefact, and runs the two channel swaps
through XRank, the pair screen and the fast policy under both profiles in the SIMD128 build, with
the XRank records equal natively and in the baseline build.

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
