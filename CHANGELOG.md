# Changelog

## 1.1.0 — 2026-10-08 — PAPH-SI: which stored works are worth comparing

Nothing of 1.0.0 moves: the wire (3), comparator 42, CAL-004-PROPOSED, PAPH-X and profile
X1-PROVISIONAL, ABI 3 and X ABI 1; the equivalence digest (3,160 cases) is byte-identical.
Beside them, **PAPH-SI**, the screening index of `docs/SPEC-SI-paph-si.md` — built from a design
note that proposed a product-quantised index over geometry, palette, structure and local
clusters at 16 / 256 resolution, measured, and corrected where the measurements disagreed.

**The index.** Six feature families read from the wire alone, each designed to survive the
square's symmetries and, where the wire allows, the complement and the recolours that keep the
palette or the luminance order: RUNS (run-length texture), TONE (luminance-quantile topology),
PAL (palette population profile — no colour), SHAPE (quantile-band regions), SIL (silhouette),
KPGEO (keypoint layout). Each is projected on four principal axes and cut at the quartiles: 256
fine cells, whose median bits are 16 exact coarse parents. XRoute's 64 local and 32 band MinHash
lanes are banded into 48 keys beside them. A 104-byte signature and ~45 postings per work; a
query probes four cells per family in order of the measured transform noise, and a candidate's
score is the summed log-likelihood evidence of the families it reaches — never a requirement
that all of them agree. The DCT section is measured out; the colour section is excluded by rule
(SPEC-003 §6.5).

**Measured** (`rust/target/release/sibench`, `npm run bench:si`; 120 bases × 20 transforms, 8,000
same-style distractors and 100,000 more for scaling; recall on comparator-42 Copy pairs): the
note's `AND` of shape, palette and structure keeps 74.3 % of copies at 698× (59.6 % at 18,697×
without probes); PAPH-SI keeps 91.9 % at 70× (the default θ = 2), 90.5 % at 98×, 87.9 % at 191×.
In union with the exact keys of `docs/SEARCH.md` — never in series: pasted copies are found by
the keys (100 %) and not by SI (13 %) — 99.8 % at 4,000 works, where XRank returns Copy on
99.5 % end to end, and 98.8 % at 104,000 (97.0 % at SI's default budget of 2,000). SI's pool
share and recall do not move with N; the index query reads ≈ 0.19 postings per stored work
(0.54 ms at 104k, native). The runs are in `docs/calibration/SI1-PROVISIONAL.log`.

**Profile SI1-PROVISIONAL** (`docs/calibration/SI1-PROVISIONAL.psi`, 5,334 bytes, SHA-256
`ca0ff1047b03cadd…`, bound to X1): codebooks, probe count, evidence weights, default threshold
and budget; fitted deterministically by `sibench fit` (re-running it reproduces the file byte for
byte), on synthetic art only.

**One definition, four implementations**: `scan` (the reference), `SiIndex` (in memory,
ScanCount), SQL (`SI_SQL` and `siSqlParams` in the glue — SQLite / D1, one postings table, one
statement), and the C ABI (SI ABI 1: `paph_siprofile*`, `paph_sisig`, `paph_sisig_wire`,
`paph_sikeys`, `paph_siquery*`, `paph_siindex*`). The glue adds `siprofile`, `sisig`, `sikeys`,
`siquery`, `siindex` and the `SIProfile`, `SIQuery`, `SIIndex` classes, with types. `xcli` gains
`sisig`, `siplan`, `sirank`; `sibench` (with `rust/sibench.sh`) is the harness.

**Verification.** 7 new unit tests (109 in all): the index equals the scan through adds,
removals and compactions at five thresholds and budgets; the coarse cell is the exact parent of
sixteen fine cells; SI1 round-trips and refuses tampering; copies clear the default threshold on
real wires and unrelated works do not. `npm run test:si` (1,417 checks): signatures, keys,
plans, scores and index answers identical natively and in both WebAssembly builds; the SQL
statement on SQLite returns exactly the index's rows, through removals.

**Findings about 1.0.0, not changed here** (SPEC-SI §3.3, §11): XRoute's G0 word equals its
original's on 87 % of D4 copies of canvases whose sides are multiples of 16 and on 41 % of the
others; the X1 route bars put 49 of 976 comparator-42 copies of this corpus in the route's
Reject class (XRank screens them anyway); XRank's gate drops 3 of those 976 pairs — recolours
that scramble luminance, empty anchor pools, certified by comparator 42 on structure.

## 1.0.0 — 2026-10-06 — PAPH-X: the same verdicts through a cheaper path

The package is now **`@pixagram/paph-x`**, and its versions start over at 1.0.0 (it was
`@pixagram/paph` 4.2.3, numbered by the comparator); the Rust package is `paph-x` 1.0.0, its
library crate still `paph`, the WebAssembly module still `paph.wasm`. Import paths change
accordingly (`@pixagram/paph-x`, `@pixagram/paph-x/wasm`); nothing else of the API moves.

Nothing of 4.2.3 moves: the wire (3), comparator 42, CAL-004-PROPOSED, the equivalence digest
(3,160 cases, byte-identical), the 625-pair parity, every ABI 2 export. Beside them, PAPH-X —
the retrieval-native cascade of `docs/SPEC-X-paph-x.md`, built as `docs/PAPH-X.md`.

**The cascade.** `xprepare` derives, once per side, a 136-byte route record (64 local-MinHash
lanes, 32 band-MinHash lanes, 4 D4-and-inversion-invariant global words), a 24-projection ×
12-bit bucket index over the descriptors (mirror-closed: a mirror is a projection swap) and
the anchor order. `xscreen` reads the route by rule (REJECT / DEFER / FAST, never one number)
and counts the anchor-tier correspondences without building them. `xcompare` runs geometry on
the sparse pools of 96 anchors, expanding 96 → 160 → 256 → 512 until a model certificate holds,
computes the structural channels as an interval and only while comparator 42's verdict lattice
can still move, and states a verdict only when every corner of the evidence agrees; otherwise
it defers — to `EXACT42` under the safe policy (the default), to `Indeterminate` under fast.
`xrank` does one query against many candidates in one call: the route table screened in SIMD
lanes, the sparse screen on what survived, the cascade on the rest, 24-field records. Every
report carries an execution state (`FAST`, `DEFERRED`, `FALLBACK`, `AUDIT`) and a reason beside
the verdict, and reports as comparator 50 with 42's vocabulary (plus `NotCopy` under copy scope).

**Measured** (`rust/target/release/xbench`, 2,379 pairs of the specification's corpus; the
tables are in `docs/PAPH-X.md`): 0 copy disagreements with comparator 42 under safe, 0 false
Copies under fast, 0 copies hard-rejected by the screen, 1.4 % fallbacks, 99.0 % fewer
descriptor pairs evaluated, 0 allocations in the screen hot path. Natively: pairwise screen
2.8× at p50 and 15× at p95, fast-path compare 26× at p50, the reference 512-keypoint search
workload 7.1×, 1,000 candidates 5.8×, the route-only screen 18.5 ns per candidate. In
WebAssembly: screens 7–38×, compares 2.4–8×, the reference search workload 3.3×. The
specification's ≥ 10× release gates are not met and not claimed. Profile X1-PROVISIONAL is
calibrated on the synthetic corpus only.

**ABI 3** adds `paph_xprofile`, `paph_xprepare` (+ `_sidecar`), `paph_xroute`, `paph_xsidecar`
(PAX1), `paph_xscreen`, `paph_xcompare`, `paph_xrank` (`docs/WASM-ABI.md`); `wasm/paph.js` adds
`xprofile`, `xprepare`, `xscreen`, `xcompare`, `xrank`, the `XProfile` and `XSide` classes, with
types in `wasm/paph.d.ts`. The Rust crate adds `paph::x` and the binaries `xbench` (the §33–35
harness), `xprof` (stage profiler) and `xcli` (the native side of the cross-engine test).

**Verification.** 21 PAPH-X unit tests (exact equivalence where the specification asks for it,
semantic equivalence against comparator 42 elsewhere); `npm run test:x` — native `xcli` against
both WebAssembly builds: routes, screens, reports and rank records byte-identical, symmetry,
exact policy = comparator 42, safe policy = 42's Copy answer on every pair, the gate never drops
a 42 copy, sidecars (3,449 checks); `npm run bench:x` — the WebAssembly timings.

## `@pixagram/paph` 4.2.3 — 2026-10-05 — the same answers, faster, and a way to search with them

The wire does not move (format 3), the comparator does not move (42), the calibration does not
move (CAL-004-PROPOSED). Every output of this release is byte-identical to `paph-js` 4.2.2's
JavaScript engine — the reference — and the Rust engine now agrees with it in four places where
it did not (below).

**A new repository and package, `@pixagram/paph`.** The evidence bench (`demo/paph4x.html`),
the JavaScript engine (`src/`), the Rust reference (`rust/`), the WebAssembly modules and their
glue (`wasm/`), the specifications (`docs/`) — and a search-engine integration
(`docs/SEARCH.md`, `integrations/pixagram-search`). The JavaScript API is `paph-js`'s, with
three differences: `load()` is replaced by `wasm()`, which returns the WebAssembly `Engine`; the
entry's `hash()` takes every image shape the wire layer takes (ImageData, `{ pixels, width,
height }`, `(bytes, width, height)`), not only `{ px, w, h }`; and each entry point
(`.`, `./wire`, `./comparator`, `./wasm`) has declarations that match what it exports.

**Rust: 2.2–6.6× faster hashing, 2.5–3.6× faster comparing, same bytes.** Local fingerprints
from block-median parity planes and a branch-free median search; keypoint pyramids from shared
summed-area tables, FAST-9 in vector lanes, lazy greedy quality selection, box-filtered
descriptors; dense adjacency tables, one-pass run lengths, a van Herk max filter, gcd upscale
detection, slicing-by-8 CRC. Comparisons parse each side once and scan each pair once per
hypothesis; the Hungarian assignment runs on real edges with lazy potentials; geometry votes
into 16-byte slots with a running peak. `docs/PERFORMANCE.md` has the tables and the reasoning.

**WebAssembly: comparator 42 in the module, ABI 2.** `paph-js` 4.2.2's module hashed and ran the
v3 comparator; this one also runs comparator 42 — the full report, as `JSON.stringify` of the
JavaScript engine's object, key for key, or a lean reading for ranking — 10–16× faster than the
JavaScript engine. Hashing is 1.7–8.4× faster than 4.2.2's module (5–73× the JavaScript engine).
New in the ABI: prepared sides as handles (parse once, compare many; *strict* mode refuses Tier 2
over 512 keypoints), calibration profiles as handles, `paph_rank42` (screen and compare one
query against n candidates in one call), `paph_local_codes` and `paph_descriptors`.
`wasm/paph.js` is hand-written ES-module glue for browsers, Node, Deno and Workers, with
TypeScript types (`wasm/paph.d.ts`); a SIMD128 build and a baseline build, chosen by a probe.
`docs/WASM-ABI.md` documents the ABI for other hosts.

**Index keys.** `Engine.indexKeys()` derives exact-match integer keys from a fingerprint —
Tier-1 local codes and 24-bit descriptor bands (`KEYS_VERSION` 1) — for an inverted index that
nominates candidates for the comparator. Measured recall and the reference SQL are in
`docs/SEARCH.md`.

**The evidence bench runs on WebAssembly.** `demo/paph4x.html` stays one self-contained file; it
now carries the WebAssembly engine (gzip + base64) beside the JavaScript one, switches to it once
loaded, checks on every pair that both engines produce the same report (section 09), and lets
you switch back. `tools/build-paph4x.mjs` generates it from `demo/bench4x/` and the engines.

**Rust parity fixes** (the JavaScript engine already behaved this way):

- a `sketchCount` below 32 made the Tier-1 serialiser read past the end of the sketch and panic;
  the slot is zero-filled past the records, as `Uint8Array.set` leaves it;
- the v3 reading's abstain notes carry the counts the reference prints;
- a Tier-1-only side's mirror axis is `(w − 1)·65535 / maxDim`, as in the reference (the Rust
  engine reflected sketches about 65535; only the v3 reading's geometry saw it);
- the v3 structural class is named after the transform as reported: a rotation by 90° read in
  swapped order is a rotation by 270°.

**Verification.** `rust/check.sh` — the equivalence digest (3,160 cases) against
`test/equiv-digest.txt`; `npm run test:equiv` — the same digest computed inside both
WebAssembly builds; `npm run test:wasm` — 625 ordered pairs, byte-identical reports in both
engines; `npm test` — the wire and comparator suites from `paph-js`; `npm run test:native`;
`npm run test:bench` — the bench, headless, on both engines.

## Earlier

`paph-js` 4.2.2 and before: see the `paph-js` repository's changelog. The specifications in
`docs/` (SPEC-003, SPEC-004, SPEC-004.1, SPEC-004.2) are unchanged.
