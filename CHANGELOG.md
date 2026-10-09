# Changelog

## 1.1.2 — 2026-10-08 — the Pixa chain's artworks: PAPH-SI fitted on them, PAPH-X measured on them

The wire (3), comparator 42, CAL-004-PROPOSED, X2-PROVISIONAL, ABI 3, X ABI 1 and SI ABI 1 do not
move; the equivalence digest (3,160 cases) is byte-identical natively and in both WebAssembly
builds. The shipped PAPH-SI profile is now **SI3-PROVISIONAL**, fitted on the artworks of the
Pixa chain; SI2 and SI1 stay as artefacts. The real-work figures below were measured on one
snapshot of the chain; the runs, with the synthetic ones set beside them, are in
`docs/calibration/SI3-PROVISIONAL.log`.

**The chain's works.** `tools/chain-corpus.mjs` walks the root posts Hivemind lists on
`https://api.pixagram.com` (`get_discussions_by_created`), keeps the artworks — a post whose body
is the image itself, a WebP or PNG data URI — skips deleted ones, and decodes them with the
decoders pixagram-search decodes with (`@jsquash/webp`, `@jsquash/png`, now devDependencies),
writing RGBA, a manifest (author, permlink, time, SHA-256 of the image bytes) and the head block.
The snapshot: head block 987,642 (2026-10-08 19:55:51 UTC), 205 posts, **177 artworks** by 30
authors, all lossless WebP, no re-uploaded image bytes, 17 blog posts, 11 deleted. The images are
not committed. `sibench corpus --chain` builds the PAPH-SI corpus with them as the bases (174 works
under the twenty transforms, a real work as the paste's host, the 8,000 synthetic distractors as
the population: 3,095 comparator-42 Copy pairs), and `sibench chain` measures every pair of the 177
works (15,576 pairs). Real works are not the synthetic bases: 512 keypoints at the median and 469
at the tenth percentile (one work has fewer than 8), 10 % carry the silhouette family (75 % of the
synthetic population does), 91 % have a side that is not a multiple of 16.

**SI2 does not screen real works; SI3 does.** SI2's codebooks, fitted on synthetic art, collapse on
the chain's works (PAL spreads them over 3 effective cells of 256; a random pair shares a PAL cell
with probability 1/2) and SI2 admits **48.7 %** of the pairs of distinct real works at its θ = 2.
`sibench chainfit` fits the way `sibench fit` does, on real works — the codebooks from the
originals, the noise and the weights from their transformed copies, the random pairs from the other
originals (never a copy's own original, nor one comparator 42 calls a copy) — first on each of two
folds, measured on the other, then on all 174. Fitted on one fold and measured on the other, its θ
is the floor, 1, where it admits 0.48–0.90 % of the unrelated real pairs and keeps **85.7 %** of
the copies; SI2 at the same 1 % keeps 71–72 %. SI3-PROVISIONAL is the fit on all 174 works (θ = 1,
budget 2,000; 5,334 bytes, `dfe99f33b6a8dfc0…`; SIL keeps SI2's codebook, which 17 works are too
few to fit; `sibench chainfit --corpus chain.bin` reproduces it byte for byte). Beside the exact
keys it nominates **99.6 %** of the real bases' comparator-42 copies, in-sample (keys alone 98.8 %,
SI3 alone 85.5 % at 0.97 % of the eval population), and XRank reads Copy on all of them. It gives
up most corner crops, pastes and downscales to the keys; it is out of distribution on synthetic art
(22 % of the synthetic population admitted, where SI2 admits 1.4 %); and it is fitted on 174 works
— provisional, to be re-fitted as the chain grows (`npm run bench:chain` runs corpus → chain →
chainfit → eval, fetching a snapshot when there is none; `sibench chainfit --name` names a new
fit).

**X2 on real works, unchanged.** XRank, shown the target alone, reads Copy on all 6,190
comparator-42 Copy queries of the real bases (X1 too), and on none of the 31,148 queries between
distinct works of the chain that comparator 42 does not call Copy; every verdict it states equals
comparator 42's. The pair screen rejects none of the 3,095 copies, though 262 sit in the route's
Reject class (X1: 212; mostly downscales, re-dithers, corner crops and resamples): bars keeping
every copy out of the class would leave 15.3 % of unrelated real pairs in it instead of 93.5 %, so
the bars stay, and nothing acts on the class alone. The structural door shuts on all 15,049
unrelated pairs of real bases, at 66 µs a pair.

**Found, not changed.**
* XRank on real pairs costs **0.79 ms a query** natively against 1.22 ms for comparator 42's
  gated `rank` (1.6×; the synthetic reference workload, a different measurement, gives 3.2×),
  and its sparse scan reads every row of the smaller side on all but 4 of the 30,330 queries
  that reach the cascade: the anchor tiers expand until a certificate holds or the rows run out.
  An exit from the expansion needs a bound on what a tier can add (docs/PAPH-X.md §6); not in a
  patch.
* Comparator 42 reads Suspected ("partial agreement") on 225 of the 15,576 pairs, 191 of them
  between two authors (1.3 % of cross-author pairs), and Copy on two pairs, each within one
  author's works. A review queue fed with Suspected grows with the number of pairs; the
  threshold is CAL-004's.
* **1.1.1's alarm about comparator 42's own gate, sized.** `rank` with `gate: true` drops 178 of
  the 976 synthetic copies, 164 for a side with fewer than 8 keypoints; on the chain's works it
  drops 8 of 3,095, 4 for keypoints (one artwork in 177 has fewer than 8): works with so few
  keypoints, common among the synthetic corpus's copy pairs, are rare on the chain. The advice
  stands (XRank, or `gate: false`).
* G0 and G3, the route's DCT and region words, equal their original's on 25 % and 46 % of D4
  copies of real works whose sides are not multiples of 16 (96 % and 95 % for the others): the
  symmetric sampling in the hasher that exact invariance needs (new wire bytes, every work
  re-hashed) matters more on real art than the synthetic corpus suggested.
* Comparator 42 certifies nearly every blur-like copy of a real work (166 down70, 171 resample90,
  173 up150 of 174; synthetic: 0, 15, 6 of 120), which the index must then find: the union holds
  93 % of the downscales.

**Compatibility.** No wire, comparator, calibration or X profile changes; nothing is re-hashed and
PAX1 sidecars stay valid. The default SI profile changes, so every SI signature and posting
changes: re-derive the stored signatures from the wires under SI3 (`paph_sisig_wire`) and swap
the postings, keyed by the profile id stored with each row (SPEC-SI §7.4).
Hosts that keep SI2 load it from `docs/calibration/SI2-PROVISIONAL.psi`.

**Harness and tests.** `sibench` adds `corpus --chain`, `chain`, `chainfit [--name]`,
`route --real` and `doorprof --real` (every pair of distinct bases but those comparator 42 calls
Copy), `--corpus NAME` and `--profile PATH`, and `eval` checks every Copy XRank finds among the
distractors against comparator 42 (it agrees on all of them, on both corpora);
`rust/sibench.sh --chain` is `npm run bench:chain`. The fit's random pairs can now skip related
pairs (`FitInput::related`), which the synthetic fit does not use: SI2 and SI1 refit byte for byte.
One new unit test (114 in all): SI3 is the shipped profile, bound to X2, every codebook its own but
SIL's; the copies test checks both SI2 and SI3, SI2 against its unrelated bound too.
`npm run test:si` (1,421 checks) checks the shipped profile is SI3 byte for byte.

**Documentation.** `docs/SPEC-SI-paph-si.md` (header, §0, §2, §4.3, §5.4, §6, §8, §9.1, §9.2, §9.5,
§9.7 new, §10, §11), `docs/PAPH-X.md` (header, status, §2, §4's new last subsection, §6, §7),
`docs/SEARCH.md` (§3b, §4, §6), `docs/WASM-ABI.md`, the README, the integration's README (the gate
on real works, Suspected on real works), the glue's and types' doc comments.

## 1.1.1 — 2026-10-08 — profile X2: 1.1.0's three findings, addressed

The wire (3), comparator 42, CAL-004-PROPOSED, ABI 3, X ABI 1 and SI ABI 1 do not move; the
equivalence digest (3,160 cases) is byte-identical natively and in both WebAssembly builds. The
shipped PAPH-X profile is now **X2-PROVISIONAL** — X1 with the same bars, route derivation 2 and
the structural door — and the shipped PAPH-SI profile **SI2-PROVISIONAL**, 1.1.0's SI1 fit bound
to X2. X1 and SI1 stay, byte for byte: SI1 in `docs/calibration/` as before, and X1, so far
built into the engine only, now also as `docs/calibration/X1-PROVISIONAL.pxcl`. Measured on the
PAPH-SI corpus (120 bases × 20 transforms, 976 comparator-42 Copy pairs) and the PAPH-X corpus
(2,379 pairs), X1 and X2 in one run: `docs/calibration/X2-PROVISIONAL.log`.

**1. XRoute's global words under the square's symmetries.** Two of the four had invariance
defects of their own, fixed by route derivation 2. G1's high half held the main-diagonal run
histogram, which a mirror sends to the anti-diagonal the wire does not hold; it now holds
|horizontal − vertical| per run bin (the low half stays their average), and G1 equals its
original's on 100 % of D4 copies (X1: 58–59 %). G3 kept the shapes section's region order, which
breaks ties in area by position; it is now the sorted list of region codes: 83–84 % (X1:
72–76 %), and 217 of 220 on canvases whose long side is at most 128 px, where that section's
grid is the pixel grid. G0, the DCT word, is unchanged at 87 % / 41 % (sides multiples of 16 /
not): the limit is the wire's DCT section, which differs between a work and its D4 copy — the
16 × 16 thumbnail's cells do not commute with a flip unless the side is a multiple of 16, and a
quarter turn swaps the integer DCT's two rounded passes; over the lowest 8 × 8 frequencies a
magnitude bit differs on 2.4–13.7 % of D4 copies. Exact invariance needs symmetric sampling in
the hasher — new wire bytes and every stored work re-hashed — so it is documented, not patched.

**2. The route class as a filter.** X2 keeps X1's route bars — bars that keep every copy out of
the Reject class would leave 55 % of unrelated pairs in it instead of 93 % — and 42 of the 976
copies score in the class (X1: 49). What changes is that no exit acts on the class alone: the
pair screen's route-Reject exit and the fast policy's `Unrelated` shortcut ask the structural
door (below) first. Copies the pair screen rejects: 2 → 0.

**3. XRank's gate.** Both of its exits — route Reject with an anchor pool of at most
`defer_pool_max` (3), and pools below `geo_min_corr` (8) after the expansion tiers — ask the
structural door before dropping a pair. Comparator-42 Copy queries XRank does not read Copy,
shown the target alone: 6 of 1,952 → 0; the PAPH-SI funnel end to end: 99.5 % → 99.8 %, all it
nominates. The three pairs are two channel swaps and a palette shuffle of works with 0–3
keypoints a side, so their anchor pools are empty; the recolour moved them out of the route's
Fast class, which XRank does not gate; comparator 42 certifies them on structure alone. (1.1.0
put this down to the recolour moving the keypoints; there are almost none to move.) Under X2 the
door keeps two of them and route derivation 2 reads the third as Fast (`sibench lost`).

**The structural door.** The structural channels — runs, silhouette, the local channel's
edge-set bound, topology, shape, DCT, palette, a near-cheapest order — until the upper bound of
the weighted structural score (unknown channels at their maximum) falls below the bar of the
lattice's recolour arm (the larger of the strong and solo bars, 6000 under CAL-004), or the pair
is not certifiable (local not measurable, fewer than three secondaries). The exits still read
thin anchor pools as the geometric arms being out of reach, as X1 did; the door answers exactly,
on any corpus, for the arm that needs no geometry: a pair whose door shuts cannot be a
structure-only Copy, and one whose door stays open is compared. Its order costs 20.6 µs per
certifiable unrelated pair of the PAPH-SI corpus, within 0.1 µs of the cheapest of all 5,040
orders (the cascade's own: 28.4 µs), and it shut on all 3,360 unrelated pairs (`sibench
doorprof`). It is a profile field (`gate_door`), on in X2.

**What it costs**, one core, X1 → X2: the door costs about 17 µs natively on an unrelated pair
(the mean over those 3,360), paid wherever a screen exit would have dropped the pair. `xscreen`
p50 16.6 → 34.7 µs (p95 116 → 117); `xcompare` fast p50 19.2 → 36.9 µs, safe unchanged;
`xrank` on the §3.1 reference workload 26.3 → 27.9 ms (3.2× rank 42 either way), N = 1,000
aggregate 143 → 184 ms; per nominated candidate in the PAPH-SI funnel 153 / 374 / 593 →
196 / 399 / 616 µs (median query / mean / unrelated pools). WebAssembly: `xscreen` on unrelated
same-style pairs 8.2 → 22 µs, the reference workload 31.0 → 33.8 ms.

**Found, not changed: comparator 42's own gated rank drops far more.** `rank` with
`gate: true` — 4.2.3's `paph_rank42` with flags bit 0, the call `docs/SEARCH.md` §4 and
`integrations/pixagram-search` make — screens out every pair whose stage-1 pools stay below
`geo_min_corr` (8), and a pool is never larger than the smaller side's keypoint count. On the
PAPH-SI corpus that is 178 of the 976 comparator-42 copies, 164 of them because a side has fewer
than 8 keypoints (`sibench lost`). XRank does not gate 172 of the 178 (its route reads them as
Fast) and under X2 reads Copy on all 976. The gate is 4.2.3's and stays as it is in a patch
release; `docs/SEARCH.md` §4, `docs/PAPH-X.md` §6, the glue's `rank` and the integration's README
now say so: verify with `xrank` (X2), or `rank` with `gate: false`.

**Artefacts and API.** The X profile artefact gains layout version 2: two bytes,
`route_derivation` and `gate_door`; a version-1 artefact (X1) decodes as before and cannot claim
version-2 behaviour. `docs/calibration/X2-PROVISIONAL.pxcl` (332 bytes, `27993afaaca76d11…`),
`X1-PROVISIONAL.pxcl` (330 bytes, `b96040d888b21e28…`), `SI2-PROVISIONAL.psi` (5,334 bytes,
`abfef6f814c4915b…`; `sibench fit` reproduces it byte for byte). The glue's `xprofile()` and
`siprofile()`, and `paph_xprofile` / `paph_siprofile` given no artefact, now build X2 and SI2.
`--x1` runs `sibench`, `xbench`, `xcli` and `node test/x-bench.mjs` under X1 and SI1. `sibench
route` adds the zero-copy bar search, the global words, the SI cells and the DCT section under the
square's symmetries; `sibench lost` prints each lost pair's channels, keypoints and comparator
42's own screen, under X2 the pairs X1 loses and what keeps them, and the copies comparator 42's
gated rank screens out; `sibench doorprof` is new, and so are `xcli xprofile`, `xcli doorcase`
and `xbench --dump-route`.

**Compatibility.** Nothing is re-hashed and no wire changes. PAX1 sidecars name the profile they
were derived under, so an X1 sidecar does not match X2: `xprepare` derives the side again —
store the new sidecar. SI2 signs every work with the bytes SI1 signs it with (the route's MinHash
lanes, which SI bands, are unchanged), so an SI index built under 1.1.0 holds the same signatures;
only the profile id it records changes. Hosts that need 1.1.0's behaviour load X1 and SI1 from
their artefacts.

**Verification.** 4 new unit tests (113 in all): X1's artefact pinned byte for byte and X2
round-tripping; derivation 2's G1 and G2 equal on every D4 copy of twelve generated works
(derivation 1's G1 moves on 18 of 48); two channel swaps comparator 42 certifies on structure
alone, dropped by X1's gate (the first also by its pair screen and fast policy) and kept under
X2 — the first by the door, the second by route derivation 2 — and both by the door under X1's
derivation with the door on, the second at the gate's second exit; SI2 is SI1 bound to X2.
`npm run test:x` (3,464 checks) checks the shipped profile is X2 byte for byte, loads X1, runs the
two channel swaps through XRank, the pair screen and the fast policy under both profiles, and
holds their rank records equal natively and in both WebAssembly builds; `npm run test:si` (1,420
checks) checks that SI1 under X1 and SI2 under X2 sign every work alike and that SI1 refuses an
X2 side.

**Documentation.** `docs/PAPH-X.md` (§2 route derivation 2 and the door, §4 X1 beside X2, §5,
§6), `docs/SPEC-SI-paph-si.md` (header, §0, §3.2–§3.3 — now split by the shapes section's grid
as well as the thumbnail's: the route's region word follows the former, the SHAPE, SIL and KPGEO
families follow neither — §8–§11), `docs/SEARCH.md` §4 and §6, `docs/WASM-ABI.md`, the README and
the integration's README.

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
