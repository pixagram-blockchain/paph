# PAPH-SI — the screening index

`@pixagram/paph-x` 1.1.1 · SI ABI 1 · feature derivation 1 · profile **SI2-PROVISIONAL**
(`abfef6f814c4915b…`, 5,334 bytes), bound to X2-PROVISIONAL (`27993afaaca76d11…`). SI2 is 1.1.0's
SI1-PROVISIONAL (`ca0ff1047b03cadd…`) bound to the X profile 1.1.1 ships: the same fit, codebooks,
weights, threshold and budget — X2 changes the route's global words, not the MinHash lanes SI
bands, so under X2 SI2 signs every work with the bytes SI1 signs it with under X1. SI1 stays,
bound to X1 (`docs/calibration/SI1-PROVISIONAL.psi`). The wire (3), comparator 42 and
CAL-004-PROPOSED do not move: the 3,160-case equivalence digest is byte-identical.

PAPH-SI answers the one question a search engine asks before any comparison: *given one new
work, which of the N stored works are worth handing to XRank?* — without reading the others.
It is the "product-quantized screening index" of the design note this document answers (§2):
built, measured, and corrected where the measurements disagreed with the note.

The tables of §3–§5 and §9.1–§9.4 were printed by `rust/target/release/sibench fit` and
`sibench eval` (`npm run bench:si` runs the latter), §3.3's G3 column and second partition and
the counts of §11 by `sibench route` and `sibench lost`, on one core of a shared two-core
container; the full output of those runs is `docs/calibration/X2-PROVISIONAL.log` (1.1.1, X2 and
SI2). 1.1.0's runs, under X1 and SI1, are `docs/calibration/SI1-PROVISIONAL.log`: the same SI
tables, while what X2 changes — the route and gate counts of §11, XRank's column of §9.3 — and
the timings differ. None of them is a claim beyond that corpus and that machine. §9.5
extrapolates from them, under assumptions it states. §9.6 lists the measurements of the Python
prototype that chose between design alternatives before the Rust implementation existed; the
shipped harness does not reproduce them.

**§0 is the answer, §2 the note ruling by ruling, §7 how to run it.**

---

## 0. What came out

1. **The dimensions.** Six quantised families, each a 16-coarse / 256-fine cell hierarchy,
   derived from the wire alone: RUNS (stroke texture), TONE (luminance topology), PAL (palette
   population), SHAPE (quantile-band regions), SIL (silhouette, works with transparency), KPGEO
   (keypoint layout). The two MinHash families XRoute already carries (local codes, descriptor
   bands) are banded into keys beside them. In the note's vocabulary: G = SHAPE + SIL + KPGEO,
   P = PAL, S = RUNS + TONE, L = LOCAL + BAND.
2. **Requiring the families to agree loses copies.** Each transform breaks a different family:
   recolours move tone (a luminance ramp moves the palette profile too), crops move everything
   global, a paste moves all of it. On the pairs comparator 42 calls Copy (one family per letter
   of the note's G ∧ P ∧ S: shape, pal, runs):

   | design | reduction | recall |
   |---|---:|---:|
   | the note: shape ∧ pal ∧ runs, own cells (§2, §9 of the note) | 18,697× | **59.6 %** |
   | the note: shape ∧ pal ∧ runs, multi-probe (§4, §8 of the note) | 698× | **74.3 %** |
   | PAPH-SI: summed evidence, θ = 2 (the default) | 70× | 91.9 % |
   | PAPH-SI, θ = 32 | 98× | 90.5 % |
   | PAPH-SI, θ = 64 | 191× | 87.9 % |

3. **Beside the exact keys, never in front of them.** The note's §10 puts the screening index
   *before* the exact-key postings; measured, SI alone finds 13 % of pasted copies and the keys
   find 100 %, while the queries the keys miss — channel swaps, crops, re-dithers, resamples —
   are mostly found by SI. Their union nominates **99.8 %** of comparator-42 copies at
   N = 4,000 (107.4 candidates per query), and XRank returns `Copy` on **99.8 %** of them end to
   end — all it was shown (1.1.0's X1 gate lost a structure-only copy there: 99.5 %, §11). At
   N = 104,000 the union holds **98.8 %** with no budget on SI's pool, and 97.0 % at the
   profile's default budget of 2,000 candidates (§9.4).
4. **The 16 × 16 × 16 estimate.** A 256-cell family behaves like 107–192 cells on this corpus
   (skew), a random pair shares a fine cell with probability 1/63 to 1/149 (not 1/256), and the
   families are correlated. A match in the query's coarse cell outside its probe cells weighs
   between −12 and +8 against a random pair, where a probed match weighs +17 to +70: the 16-cell
   level carries no evidence for a copy. The multiplicative 4,096× is not reachable at useful
   recall; the measured curve is §9.2.
5. **Scale.** SI's share of the population is constant (1.44–1.45 % at θ = 2 from 4k to 104k
   works) and so is its recall (91.9 %); the exact keys' top-32 recall decays slowly
   (98.1 % → 95.2 %). At millions of works the cost is not the index (0.52 ms per query at 104k)
   but verification: XRank spends 0.20 ms (median query) to 0.62 ms (negatives, mean) on each
   *nominated* candidate and goes past its cheap screen on half of them. §9.5 gives the budget
   arithmetic.
6. **Three findings about the engine, addressed in 1.1.1** (profile X2, docs/PAPH-X.md §2).
   XRoute's G0 (DCT) word equals its original's on 87 % of D4 copies when both sides of the
   canvas are multiples of 16 and on 41 % otherwise (§3.3) — a limit of the wire's DCT section,
   not of the word, documented; two of the route's other words had invariance defects of their
   own, fixed in route derivation 2. The X1 route bars put 49 of 976 comparator-42 copies of this
   corpus in the route's Reject class and the screen dropped two of them; X2 keeps the bars (42
   copies in the class) but drops nothing on the class alone. And XRank's gate dropped 3 of the
   976 pairs (6 of 1,952 queries): recolours of works with 0–3 keypoints, so the anchor-tier
   pools are empty, which the recolour moved out of the route's Fast class, and which comparator
   42 certifies on structure alone. Under X2 the structural door keeps two of them and route
   derivation 2 the third. Comparator 42's own gated rank, unchanged, screens out 178 of the
   976, 164 of them pairs with a side of fewer than 8 keypoints (§11): verify with XRank.

---

## 1. Where it sits

```
ingest   wires ─┬─► SiSig: 6 cells + 48 band keys (104 B) ─► SI postings   (≈ 45 per work)
                └─► indexKeys(): codes + bands             ─► key postings  (≈ 700 per work, docs/SEARCH.md)

query    wires ─┬─► SiQuery: probe cells + weights ─► score ≥ θ, best B ─┐
                └─► query keys ─► Σ 1/df ─► top 32 codes ∪ top 32 bands  ─┴─► ∪ ─► XRank ─► verdicts
```

Keys and SI only **nominate**; XRank (route table, sparse screen, cascade) **decides**, with
comparator 42 behind it under the safe policy. Nothing fuzzy is stored: a signature is six bytes
of cells and 48 integer band keys, and two nodes deriving it from the same wires under the same
profile derive the same bytes.

---

## 2. The note, ruling by ruling

| § of the note | proposal | ruling | evidence |
|---|---|---|---|
| 1 | a new screening layer before the postings / XRoute | **accepted, in parallel** with the postings, not before them | §0.3; §9.3 |
| 2 | clusters as addressing, not similarity | **accepted** — a cell is an address; the score is evidence about addresses | §5 |
| 3 | 16³ = 4,096 cells → ≈ 4,096× on independent partitions | **rejected as an estimate** — measured effective cells 107–192 of 256 per family, correlated families; the curve is measured, not multiplied | §4.3, §9.2 |
| 4 | multi-probe per property | **accepted** — four probe cells per family, ordered by the cost of crossing bin edges in units of each axis' measured transform noise | §5.3 |
| 5 | adaptive 16 → 256 → 1024 levels | **replaced** by a score threshold θ and a budget B: a coarse-only match weighs −12 to +8 (§4.2), and 1,024 cells measured within half a point of 256 in the prototype (§9.6) | §4.2, §5.4, §9.6 |
| 6 | hierarchical clustering | **kept, exactly**: a coarse cell is the median bit of each of the four axes, the union of sixteen fine cells, derivable from every signature (the odd bits of the fine cell); not scored | §4.2 |
| 7 | geometry / palette / structure / local derived from PAPH's representations; palette not as raw RGB but as dominant-colour, luminance, hue and saturation distributions and colour entropy | **accepted, except hue and saturation**: the wire's colour section is reporting-only (SPEC-003 §6.5), and hue is what a recolour changes; PAL reads the dominant-colour distribution without the colours (the population profile), TONE the luminance distribution. The DCT section the note lists under structure was measured and dropped | §3 |
| 8 | SQL table with g16/g256 … columns, composite indexes, `AND` | **replaced** by one postings table and one scored statement; `AND` loses a quarter to two fifths of the copies | §7.2, §0.2 |
| 9 | bitmaps / postings intersection | **accepted as postings with score accumulation** (the ScanCount form of T-occurrence), which answers every k-of-n question at once; plain intersection is the `AND` row | §7.1 |
| 10 | SCREEN INDEX → PAPH POSTINGS in series | **rejected** — union, not series: pasted copies (SI 13 %, keys 100 %); channel swaps, crops, re-dithers, resamples the other way | §9.3 |
| 11 | the name PAPH-SI | **kept** (PAPH Screening Index) | — |
| 12 | transformation-stable features before clustering | **accepted, measured**: the stability matrix of §3.2 is that test | §3.2 |
| 13 | a one-day routing cache keyed by index generation | **the generation is built** (`SIIndex.generation`, part of any cache key); the cache itself is left to the host — a new upload never repeats a query, re-checks and appeals do | §7.4 |
| 14 | 18 min / 256 ≈ 4.2 s | **arithmetic right**, premise incomplete: at 10M works SI's 1.45 % pool is 145k candidates, and a nominated candidate costs XRank 0.20–0.62 ms | §9.5 |
| 15 | target recall ≥ 99 %, reduction ≥ 100× | **the recall half is met by the union** with no budget on SI up to 64k works (99.0 %, at 37× to 64× of the population), and at the default budget B = 2,000 up to 16k (99.4 %); **no measured configuration meets both** — SI alone gives 90.5 % at 98× | §9.2–§9.4 |
| 16 | the same index serves image search | **limited** — every family is blind to colour by design and to style by construction; "visually related" belongs to the embedding channel the search engine already has | — |

---

## 3. The families (rust/src/x/si/features.rs)

### 3.1 Definitions

Every family is computed from the wire (Tier 1, and Tier 2 when it parsed), never from pixels,
so a stored work can be re-indexed from its wires without being re-hashed. Integer arithmetic
only. The third column is the design; §3.2 is what each family actually keeps.

| family | dims | reads (SPEC-003 section) | designed to survive | loses |
|---|---:|---|---|---|
| **RUNS** | 16 | `runs`: the horizontal and vertical run-length histograms, summed (a quarter turn exchanges them) | D4; the complement and every recolour that maps the palette one to one (runs of one palette index do not care what colour it is); integer upscales | non-integer rescales and re-dithering (runs shift along the ladder) |
| **TONE** | 28 | `rag`: each colour adjacency's luminance-quantile gap and level `min(q, 255 − q)` and rank gap, mass-weighted, and the adjacency count; `brightness`: q95 − q5, q75 − q25, \|median − 128\|; `palette`: quantile mass folded about the middle | D4, the complement, order-keeping recolours | hue-scrambling recolours, crops |
| **PAL** | 24 | `palette`: the 23 runner-up colours' population relative to the most frequent, and the colour count | D4, the complement, bijective recolours — it holds no colour at all | resampling (blends add colours), crops |
| **SHAPE** | 41 | `shapes`: the 8-band luminance-quantile regions (largest first) — areas relative to the largest, isoperimetric / aspect / hole classes, count, the largest region's share of the frame, its radial profile sorted (D4 permutes the 32 rays) | D4, order-keeping recolours, the complement (it relabels bands without moving a boundary) | crops, rescales |
| **SIL** | 87 | `silhouette`, works with real transparency (F_SIL — sprites, and works on a flat matte the front end folds to transparency; 75 % of the eval population): sorted radial profile, the two axis-aligned second moments as min and max (a quarter turn exchanges them) and \|m11\|, fill, components, aspect class, opaque share, transition histograms, the D4-canonical 8×8 occupancy code | D4, recolours, rescales | crops |
| **KPGEO** | 23 | keypoints (Tier 2, or the Tier-1 sketch without it; ≥ 8): radial distribution about their centroid in units of their RMS radius, scatter anisotropy, pyramid levels, orientation sectors folded by D4 and the complement, count | D4, the complement | recolours, crops |
| LOCAL | 32 keys | XRoute's 64 local-code MinHash lanes, two per key | D4, integer upscales, crops (partly), pastes (partly) | resampling, re-dithering, the complement (measured) |
| BAND | 16 keys | XRoute's 32 descriptor-band MinHash lanes (direct ∪ mirrored descriptors), two per key | mirrors, rotations, integer upscales | the complement, resampling, non-integer rescales |

### 3.2 Stability, measured

The test the note's §12 asks for. For every eval base and each of its twenty variants, in both
arrival orders: how often the copy lands in the query's own cell (exact) or in one of its four
probe cells (probed); for the MinHash families, how often at least one band key is equal. The
last row is a random eval distractor.

| transform | runs | tone | pal | shape | sil | kpgeo | local | band |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| mirror | 100 / 100 | 100 / 100 | 100 / 100 | 73 / 96 | 76 / 92 | 43 / 86 | 100 | 82 |
| rot90 | 100 / 100 | 96 / 100 | 100 / 100 | 77 / 92 | 54 / 86 | 39 / 75 | 100 | 86 |
| rot180 | 100 / 100 | 96 / 100 | 100 / 100 | 70 / 93 | 49 / 78 | 46 / 82 | 100 | 86 |
| transpose | 100 / 100 | 100 / 100 | 100 / 100 | 96 / 97 | 100 / 100 | 97 / 100 | 100 | 100 |
| invert | 98 / 100 | 82 / 97 | 88 / 88 | 68 / 77 | 100 / 100 | 86 / 100 | 8 | 3 |
| recolour | 80 / 88 | 29 / 61 | 54 / 65 | 66 / 81 | 77 / 80 | 41 / 56 | 80 | 63 |
| chswap | 96 / 100 | 41 / 52 | 93 / 96 | 73 / 85 | 100 / 100 | 38 / 59 | 66 | 45 |
| palshuffle | 88 / 88 | 11 / 25 | 86 / 87 | 54 / 68 | 97 / 97 | 22 / 43 | 18 | 7 |
| up2 | 100 / 100 | 100 / 100 | 100 / 100 | 100 / 100 | 100 / 100 | 100 / 100 | 100 | 100 |
| up3 | 100 / 100 | 100 / 100 | 100 / 100 | 100 / 100 | 100 / 100 | 100 / 100 | 100 | 100 |
| down70 | 12 / 30 | 7 / 15 | 2 / 7 | 11 / 32 | 29 / 55 | 4 / 7 | 2 | 0 |
| resample90 | 23 / 41 | 11 / 28 | 2 / 14 | 25 / 49 | 39 / 62 | 7 / 21 | 6 | 0 |
| up150 | 4 / 6 | 46 / 76 | 36 / 65 | 21 / 46 | 78 / 85 | 0 / 10 | 8 | 0 |
| crop80 | 38 / 76 | 21 / 59 | 20 / 46 | 14 / 44 | 24 / 36 | 0 / 28 | 80 | 56 |
| crop67 | 34 / 66 | 12 / 32 | 9 / 22 | 9 / 24 | 10 / 20 | 5 / 9 | 61 | 27 |
| corner50 | 23 / 47 | 11 / 27 | 5 / 12 | 7 / 17 | 6 / 11 | 0 / 0 | 26 | 0 |
| paste | 5 / 10 | 0 / 4 | 2 / 7 | 0 / 4 | 0 / 6 | 0 / 9 | 25 | 7 |
| shift1 | 79 / 99 | 77 / 95 | 86 / 96 | 59 / 88 | 78 / 89 | 43 / 84 | 100 | 86 |
| dither | 12 / 29 | 12 / 26 | 14 / 31 | 12 / 35 | 68 / 68 | 17 / 29 | 4 | 4 |
| matte | 30 / 64 | 62 / 72 | 64 / 76 | 64 / 75 | 44 / 66 | 34 / 59 | 92 | 76 |
| *random pair* | 1.5 / 4.3 | 0.9 / 2.9 | 1.4 / 3.7 | 0.9 / 3.3 | 1.2 / 3.0 | 0.5 / 2.0 | 1.64 | 0.48 |

Read it by column for what a family is for, by row for what saves a transform. Taking "kept" as
at least half the pairs probed: the square's symmetries, integer rescales, the complement, all
three recolours, shift and matte keep at least three families; crop80 keeps four; up150 three
(TONE, PAL, SIL); crop67 two (RUNS, LOCAL); resample90, down70 and re-dithering one (SIL, on
works with transparency); corner50 none (RUNS reaches 47 %); paste none. No family survives
everything. That is the whole case for scoring over requiring — and for the exact keys beside
SI.

Where the matrix falls short of §3.1's design column, two causes are visible upstream of SI, in
how the front end normalises a work. One is measured: the 16 × 16 thumbnail's grid commutes with
D4 only on some canvas sizes (§3.3) — it costs XRoute's G0 word most, and TONE (whose three
brightness readings come from that thumbnail) a few points under rot90 and rot180. The other is
read from the code, not measured pair by pair: the matte fold breaks ties between equally
frequent border colours by colour key, which the complement and recolours reorder, so some
recoloured copies are normalised with a different matte. Neither explains SHAPE, SIL and KPGEO
under mirror, rot90 and rot180 (39–77 % exact, 75–96 % probed), nor LOCAL and BAND under the
complement (8 % and 3 %). The first three hold under transpose (96–100 % exact), the one
symmetry here that leaves the top-left corner where it was, which points at rounding anchored
to that corner; §3.3 rules out the two sampling grids it tests, the thumbnail's and the shapes
section's (the route's region word, read from the same section as SHAPE, follows the latter;
the families follow neither). The candidates are read from the code, not measured: SHAPE takes
its regions' classes in the section's order, which breaks a tie in area by position; SHAPE and
SIL trace their radial profiles from a centroid rounded down, along rays rounded half up; SIL's
occupancy code samples an 8 × 8 grid laid from its bounding box's top-left corner; KPGEO reads
keypoints detected on a pyramid whose blocks are laid from the image's. Which of them costs
what is not established here.

### 3.3 Sampling grids, and what was left out

| canvas | pairs | G0 word | G3 word | runs | tone | pal | shape | sil | kpgeo |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| both sides multiples of 16 | 180 | 87 % | 83 % | 100 % | 100 % | 100 % | 74 % | 74 % | 61 % |
| other sizes | 300 | 41 % | 84 % | 100 % | 98 % | 100 % | 80 % | 72 % | 56 % |
| long side ≤ 128 px | 220 | 65 % | 99 % | 100 % | 100 % | 100 % | 76 % | 76 % | 61 % |
| long side > 128 px | 260 | 52 % | 71 % | 100 % | 98 % | 100 % | 79 % | 70 % | 57 % |

The share of D4 copies (mirror, rot90, rot180, transpose; every base) whose reading equals the
original's exactly, under two partitions of the same 120 bases: the DCT thumbnail's (both sides
multiples of 16, or not) and the shapes section's (long side at most 128 px, where that
section's grid is the pixel grid, or more). `sibench route` prints both partitions; `sibench
eval` prints the first, without G3. SIL and KPGEO are read over the pairs where the family is
present on both sides: 100 / 204 / 144 / 160 and 92 / 176 / 61 / 207 of them, row by row.
G0 — XRoute's DCT word, built to be D4- and complement-invariant — holds on 87 % of copies
of canvases whose sides are multiples of 16 and on 41 % of the others: the thumbnail's cell edges
sit at ⌊i·w/16⌋, which a mirror or a quarter turn moves by a pixel otherwise, and the
median-split magnitude bits flip under that shift. A family that should be D4-invariant and is
not on most real canvas sizes cannot address copies, so the DCT section is not a PAPH-SI family.
G3 — XRoute's region word under route derivation 2, read from the shapes section — follows that
section's grid instead (⌈w / c⌉ × ⌈h / c⌉ cells with c = ⌈long side / 128⌉, edges rounded down
from the top-left corner): 217 of 220 at 128 px or less, 71 % above. SHAPE, SIL and KPGEO follow
neither partition, so neither of these two grids explains their shortfall (§3.2).

Also left out: the diagonal run histogram (a mirror sends the main diagonal to the
anti-diagonal, which the wire does not hold), and — by rule, not by measurement — the colour
section: SPEC-003 §6.5, "This section MUST NOT contribute to any score. Recolour invariance is a
load-bearing property of the whole design and it dies the moment absolute RGB enters the
scoring path." A routing key is a scoring path.

---

## 4. Cells (rust/src/x/si/code.rs, fit.rs)

### 4.1 Projection and bins

Per family, the profile holds an integer mean μ, four integer projection axes P, and per axis
three bin edges t₀ ≤ t₁ ≤ t₂:

```
z_k    = Σ_i P[k][i] · (x_i − μ_i)                    (i64, exact)
b_k    = #{ j : t_k,j ≤ z_k }                         (0..3; a value on an edge lies above it)
fine   = b₀ + 4·b₁ + 16·b₂ + 64·b₃                    (0..255)
coarse = (b₀≫1) + 2·(b₁≫1) + 4·(b₂≫1) + 8·(b₃≫1)      (0..15)
```

The axes are the four principal axes of the family standardised over the fitting population
(cyclic Jacobi on the correlation matrix), each signed so its largest component is positive,
with the standardisation folded into the weights (`round(v_i / σ_i · 2¹⁴)`). The edges are the
quartiles of the population's projections, so each bin holds a quarter of it along its axis.

### 4.2 The hierarchy, and what its coarse level is worth

A coarse cell is the median bit of each axis: exactly sixteen fine cells share it (held by a
test), and it is derivable from any signature (the odd bits of the fine cell). It is the note's
G16 → G256 tree made exact rather than clustered twice. It is not scored: a candidate outside
the query's probe cells but inside its coarse cell is about as likely to be a copy as a random
pair is (weights −12 to +8, odds 0.5 to 1.6) — less evidence against than no match at all (−23
to −36), but no evidence for one — and scoring it would read the postings of sixteen fine cells
per family instead of four. Measured on the comparator-42 copies of the eval bases and on random
eval distractors:

| family | copy: probed | copy: coarse only | copy: neither | random: probed | random: coarse only | random: neither | weight of coarse only | weight of neither |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| runs | 88.1 % | 2.4 % | 9.5 % | 5.4 % | 4.3 % | 90.2 % | −9 | −36 |
| tone | 83.1 % | 2.2 % | 14.8 % | 3.2 % | 4.7 % | 92.1 % | −12 | −29 |
| pal | 81.9 % | 2.8 % | 15.4 % | 3.4 % | 5.9 % | 90.7 % | −12 | −28 |
| shape | 84.7 % | 5.6 % | 9.6 % | 3.6 % | 5.4 % | 91.0 % | +1 | −36 |
| sil | 84.8 % | 3.9 % | 11.3 % | 2.4 % | 6.1 % | 91.5 % | −7 | −33 |
| kpgeo | 68.6 % | 9.1 % | 22.3 % | 2.1 % | 5.6 % | 92.3 % | +8 | −23 |

### 4.3 How the population spreads

Eval population: 4,000 same-style distractors.

| family | present | occupied fine | effective fine cells | largest fine cell | effective coarse cells | P(random pair shares a fine cell) |
|---|---:|---:|---:|---:|---:|---:|
| runs | 100 % | 217 | 112 | 6.2 % | 14.7 | 1/63 |
| tone | 100 % | 248 | 163 | 3.2 % | 15.3 | 1/117 |
| pal | 100 % | 174 | 107 | 4.3 % | 12.6 | 1/79 |
| shape | 100 % | 219 | 137 | 3.8 % | 14.4 | 1/97 |
| sil | 75 % | 204 | 109 | 4.2 % | 12.6 | 1/68 |
| kpgeo | 65 % | 253 | 192 | 2.2 % | 15.3 | 1/149 |

Effective cells = exp(entropy). Quartile edges balance each axis; the cells are not balanced
because the axes are not independent — the note's 1/4,096 per triple would need both.

---

## 5. Probes and the score (rust/src/x/si/code.rs)

### 5.1 The signature

What an index stores per work, 104 bytes: byte 0 the presence bits (bit *f* for quantised
family *f* in the order runs, tone, pal, shape, sil, kpgeo; bit 6 local; bit 7 band), bytes 1–6
the fine cells, byte 7 zero, then the 32 local band keys and the 16 descriptor band keys as
little-endian `u16` (band *j* = lane 2*j* · 256 + lane 2*j*+1).

### 5.2 Levels

| family kind | level 0 | level 1 | level 2 | level 3 |
|---|---|---|---|---|
| quantised | none of the query's probe cells | near: one of its other probe cells | exact: the query's own cell | — |
| MinHash | no equal band key | one | two or three | four or more |

A family absent on either side contributes nothing.

### 5.3 Probe order

The query's own cell first; then every cell reached by moving one or more axes one bin down or
up across an edge, cheapest first. Moving axis *k* costs `⌊d_k · 1024 / σ_k⌋²` — `d_k` the
distance from the query's projection to the edge crossed, capped at 2²⁰ before squaring, `σ_k`
the axis' transform noise (the RMS displacement of a copy along the axis over √2, measured on
the fitting pairs) — and a move of several axes costs the sum. Ties go to the lower cell. The
profile uses four probes: the prototype found 8 and 16 no better at equal pool share (§9.6), and each
probe reads one more cell's posting list.

### 5.4 Weights, the score, and admission

Each level's weight is 16 × the log-likelihood ratio of reaching it on a copy pair versus a
random pair, from the fitting split (copy pairs: the twenty transforms but paste, where
comparator 42 reads Suspected, Copy or Identical; random: 400 background works per query):

| family | none | near | exact |
|---|---:|---:|---:|
| runs | −27 | 22 | 62 |
| tone | −21 | 29 | 70 |
| pal | −22 | 17 | 66 |
| shape | −23 | 33 | 65 |
| sil | −34 | 26 | 66 |
| kpgeo | −17 | 42 | 69 |

| MinHash | 0 keys | 1 | 2–3 | ≥ 4 |
|---|---:|---:|---:|---:|
| local | −20 | 23 | 64 | 140 |
| band | −13 | 57 | 107 | 185 |

**The score** of a candidate is the sum, over the families present on both sides, of the weight
of the level it reaches. It is defined once (`SiQuery::score`) and every index reproduces it
exactly. A candidate is **touched** when some family reaches level 1 or above — the only
candidates an inverted index can see — and **admitted** when it is touched and scores at least
θ. The answer to a query is the admitted candidates, score descending then slot ascending, at
most B of them (`code::scan` is the reference; the in-memory index and the SQL statement are
held equal to it by test).

**θ** is fitted, not chosen: the score that 1 % of the fit's random pairs reach, floored at 1
(net evidence for a copy). For SI1 and SI2 that is θ = 2. On the eval split θ = 2 admits 1.44 % of the
population; a 1 % pool needs θ ≈ 32 (§9.2). **B** defaults to 2,000. Raising θ trades recall for
reduction along the curve of §9.2; a budget cuts the tail without changing the order.

---

## 6. The profile (rust/src/x/si/profile.rs, fit.rs)

`docs/calibration/SI2-PROVISIONAL.psi` (the shipped default, `SiProfile::si2()` and
`SiProfile::shipped()`, bound to X2) and `docs/calibration/SI1-PROVISIONAL.psi`
(`SiProfile::si1()`, 1.1.0's, bound to X1), both embedded in the crate, decoded and checked on
load; a profile's SHA-256 is its identity. Layout, little-endian:

| bytes | field |
|---|---|
| 4 | magic `PXSI` |
| 2 + 2 | profile version (1), feature derivation version (1) |
| 16 | name (`SI2-PROVISIONAL`, zero-padded) |
| 32 | identity of the X profile whose route lanes are banded |
| 1 + 1 + 1 + 1 | axes (4), bins (4), probes (4), families (6) |
| per family | dims `u16`; mean `i32 × d`; projection `i32 × 4d`; edges `i64 × 12`; noise `i64 × 4`; weights `i32 × 3` |
| 1 + 1 | local bands (32), descriptor bands (16) |
| 32 | MinHash weights `i32 × 4 × 2` |
| 4 + 4 | default threshold, default budget |

Decoding refuses: another magic, profile version or feature derivation; a cell layout other than
4 axes × 4 bins; a probe count outside 1–8; a family count other than 6 or a dimension other
than the derivation's; edges out of order; an axis noise below 1; a mean outside ±4,096; a band
layout other than 32 + 16; a budget below 1; truncation and trailing bytes. Two nodes holding
different codebooks therefore cannot claim the same index, and a profile fitted against one X
profile refuses sides prepared under another (signatures and queries return −2 / 0).

**Fitting** (`sibench fit`, fit.rs) is offline and may use floating point: what it writes is
integers, and nodes agree on the artefact's identity, not on how it was fitted. It is
deterministic all the same: re-running `sibench fit` on the same corpus reproduces SI2 byte for
byte, and `sibench fit --x1` SI1 (checked with `cmp`). Both were fitted on the corpus's fitting
split (4,000 background works, 2,432 noise pairs, 1,958 evidence pairs) in 0.07 s, with the same
seed; they differ in the name and the bound X profile only. The codebooks come from the background
population alone; the noise scales and the weights need copy pairs — so a production fit takes
its axes and edges from the stored works' wires and its noise and weights from transformed
copies of a sample of them.

---

## 7. Running it

### 7.1 In memory (rust/src/x/si/index.rs; `SIIndex` in the glue)

Posting lists per (family, fine cell) and per (MinHash family, band, key). A query reads the
lists of its probe cells and band keys and accumulates, per touched slot, the excess weight of
the level reached (one cell per family per slot, so at most one probe of a family reaches it)
and the count of equal band keys; then adds the none-weights of the families both sides hold
(a 256-entry table on the presence byte), admits by θ and selects the best B with a partial
sort. Removal tombstones a slot; the lists are compacted when more than a quarter of the entries
they hold belong to removed slots. 45.5 posting entries per work; at 104,000 works a query reads
20,140 entries in 0.52 ms.

### 7.2 SQL — SQLite, Cloudflare D1 (`SI_SQL` and `siSqlParams` in wasm/paph.js)

```sql
CREATE TABLE IF NOT EXISTS si_works (
  work_id    INTEGER PRIMARY KEY,
  present    INTEGER NOT NULL,   -- byte 0 of the signature: which families it holds
  sig        BLOB    NOT NULL,   -- the 104-byte signature, to re-key or re-score without the wires
  si_profile TEXT    NOT NULL    -- SIProfile.id().slice(0, 16): re-derive on change
);
CREATE TABLE IF NOT EXISTS si_postings (
  k       INTEGER NOT NULL,      -- a posting key (engine.sikeys)
  work_id INTEGER NOT NULL,
  PRIMARY KEY (k, work_id)
) WITHOUT ROWID;
```

A posting key is below 2²⁷: quantised family *f* in cell *c* is `f·2²⁴ + c`; local band *j*
holding *v* is `6·2²⁴ + j·2¹⁶ + v`; descriptor band *j*, `7·2²⁴ + j·2¹⁶ + v`. Ingest writes one
`si_works` row and `sisig(…).keys` (≤ 54 rows) into `si_postings`. The query is one statement
whose parameters come from `siquery(…).plan()`:

```sql
WITH probe(k, fam, w) AS (
  SELECT value->>0, value->>1, value->>2 FROM json_each(?1)),
hits AS (
  SELECT p.work_id AS id, probe.fam AS fam, probe.w AS w
  FROM probe CROSS JOIN si_postings p ON p.k = probe.k),
agg AS (
  SELECT id, SUM(CASE WHEN fam < 6 THEN w ELSE 0 END) AS ex, SUM(fam = 6) AS nl, SUM(fam = 7) AS nb
  FROM hits GROUP BY id),
scored AS (
  SELECT agg.id AS id, ex
    + CASE WHEN nl = 0 THEN 0 WHEN nl = 1 THEN ?2 WHEN nl <= 3 THEN ?3 ELSE ?4 END
    + CASE WHEN nb = 0 THEN 0 WHEN nb = 1 THEN ?5 WHEN nb <= 3 THEN ?6 ELSE ?7 END
    + (w.present & 1) * ?8 + ((w.present >> 1) & 1) * ?9 + ((w.present >> 2) & 1) * ?10
    + ((w.present >> 3) & 1) * ?11 + ((w.present >> 4) & 1) * ?12 + ((w.present >> 5) & 1) * ?13
    + ((w.present >> 6) & 1) * ?14 + ((w.present >> 7) & 1) * ?15 AS score
  FROM agg CROSS JOIN si_works w ON w.work_id = agg.id)
SELECT id, score FROM scored WHERE score >= ?16 ORDER BY score DESC, id LIMIT ?17
```

`?1` is the plan's probes as JSON (`[key, family, excess weight]`), `?2`–`?7` the MinHash
excesses, `?8`–`?15` the none-weights by presence bit, `?16` θ, `?17` B (`siSqlParams` clamps a
negative budget to 0: SQLite reads `LIMIT −1` as no limit). The plan is a nested loop — probe
rows, a primary-key range in `si_postings`, a primary-key probe in `si_works` — and `CROSS JOIN`
pins that order. `test/si-wasm.mjs` runs this statement on SQLite (`node:sqlite`) and checks it
returns exactly the rows `SIIndex.query` returns, through removals; with work ids equal to slots
the order matches too.

### 7.3 In the funnel

Run SI and the exact keys of docs/SEARCH.md for the same query, union the two candidate sets,
and hand the union to XRank (`gate: true`, copy scope). Never filter one by the other, and never
filter either by the XRoute class alone (§11). The keys' top 32 per family is a fixed-size
nomination; SI's is a share of the population, cut by the budget.

### 7.4 Caching and operations

* **Generation.** Every add and remove bumps `SIIndex.generation`. A query cache key is (the
  query's plan — a hash of `q.plan()`, which names the profile, θ, B and every probe with its
  weight — any θ / B override, and the generation; in SQL, the host's own corpus generation).
  The 104-byte signature is not enough: the probes depend on where the query's projections fall
  inside its cells. A time-to-live is the host's choice; a fresh upload never repeats a query,
  re-checks do.
* **Re-deriving.** A profile change (a new `.psi`, a new feature derivation) re-derives every
  signature from the stored wires — `paph_sisig_wire` builds it from Tier 1 + Tier 2 without the
  bucket index, 106 µs a work natively, parse included — and swaps the postings. Store the
  profile id with each row (`si_profile`).
* **Deletion.** Delete the work's postings and its `si_works` row; in memory, `remove(slot)`.
* **Order of arrival.** As with the keys: index a work after querying with it, so each pair is
  examined once, by the later of the two.

---

## 8. API

Rust (`paph::x::si`): `SiProfile::{shipped, si2, si1}` (`shipped()` is SI2),
`SiSig::{build, from_prepared, to_bytes, from_bytes}`,
`SiQuery::{new, from_prepared, score, touches, levels}`, `scan`, `SiIndex::{add, remove, query,
generation}`, `fit::fit`. C ABI: docs/WASM-ABI.md, *PAPH-SI (SI ABI 1)*. JavaScript:

```js
import { init, SI_SQL, siSqlParams } from '@pixagram/paph-x/wasm';
const paph = await init();
const index = paph.siindex();                           // in memory; or SQL, below

// ingest — fp = { t1, t2 } from paph.hash(), or the stored wires
const sig = paph.sisig(fp);                             // { bytes (104), present, cells, keys }
const slot = index.add(sig.bytes);                      // or one si_works row + sig.keys as si_postings rows

// query
const side = paph.xprepare(fp.t1, fp.t2);
const q = paph.siquery(side);
const { hits } = index.query(q);                        // [{ slot, score }], best first, θ = 2, B = 2000
db.prepare(SI_SQL.query).all(...siSqlParams(q.plan())); // the same answer from SQLite / D1
const keys = paph.indexKeys(fp, { query: true });       // the exact-key half of the funnel (SEARCH.md)
// … union the two candidate sets, then paph.xrank(side, candidates)
```

`npm run bench:si` runs the harness (`rust/sibench.sh`; `--big` adds the scaling table, `--fit`
re-fits SI2); `npm run test:si` the cross-engine and SQL checks.

---

## 9. Measurements

### 9.1 The corpus

`sibench corpus`: 120 bases from the engine's generators (`synth.rs`), in fifteen blocks of eight
— one of each kind per block: dithered and flat pixel art, sprites on transparency, busy
scenes, the noise-field `work` — each under twenty transforms: mirror, rot90, rot180,
transpose, invert, recolour (a luminance ramp), chswap (red ↔ blue), palshuffle (a random
bijection of the colours), up2, up3, down70 and resample90 (box filters), up150 (nearest, 3/2),
crop80, crop67, corner50, paste (into a host twice the size), shift1, dither, matte. Then 8,000
same-style distractors from the same generators with other seeds — the hardest negatives this
corpus has — and, for the scaling table, 100,000 more (108,000 in all). Splits: alternate blocks
of bases (64 bases) and the first 4,000 distractors fit SI1 and SI2; the other blocks (56 bases) and the
other 4,000 distractors evaluate, so every kind of base is on both sides. Recall is counted on
the eval pairs comparator 42 itself calls Copy or Identical (416 pairs, 832 queries in both
arrival orders): a nominator need not find what the verifier cannot confirm. Over all 120
bases, comparator 42 reads Copy on 0 down70 pairs, 15 resample90, 6 up150 and 5 corner50 — so
those rows are thin, and an eval row of 1–6 pairs is an anecdote, not a rate.

### 9.2 Recall against reduction

Eval population 4,000 distractors. *Pool*: the mean share of the population a query admits.

| design | pool | reduction | recall | symmetries | integer rescale | recolour | resample | crop | paste | edits |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| the note: shape ∧ pal ∧ runs, own cell only | 0.005 % | 18,697× | 59.6 % | 82 % | 100 % | 61 % | 0 % | 0 % | 0 % | 24 % |
| the note: shape ∧ pal ∧ runs, multi-probe | 0.143 % | 698× | 74.3 % | 99 % | 100 % | 73 % | 6 % | 10 % | 0 % | 60 % |
| the note: all six quantised families, own cell | 0.000 % | 3,328,000× | 39.4 % | 47 % | 100 % | 21 % | 0 % | 0 % | 0 % | 11 % |
| the note: all six, multi-probe | 0.018 % | 5,584× | 60.5 % | 84 % | 100 % | 38 % | 0 % | 0 % | 0 % | 47 % |
| votes: ≥ 2 of 8 families agree | 3.203 % | 31× | 94.6 % | 100 % | 100 % | 98 % | 62 % | 97 % | 32 % | 95 % |
| votes: ≥ 3 of 8 | 1.067 % | 94× | 90.3 % | 100 % | 100 % | 95 % | 44 % | 72 % | 8 % | 93 % |
| votes: ≥ 4 of 8 | 0.281 % | 356× | 83.4 % | 100 % | 100 % | 82 % | 19 % | 44 % | 0 % | 84 % |
| SI score ≥ −10 | 2.008 % | 50× | 92.9 % | 100 % | 100 % | 97 % | 62 % | 88 % | 16 % | 94 % |
| SI score ≥ 2 (the default) | 1.435 % | 70× | 91.9 % | 100 % | 100 % | 96 % | 62 % | 84 % | 13 % | 93 % |
| SI score ≥ 32 | 1.025 % | 98× | 90.5 % | 100 % | 100 % | 94 % | 44 % | 79 % | 5 % | 92 % |
| SI score ≥ 64 | 0.523 % | 191× | 87.9 % | 100 % | 100 % | 91 % | 31 % | 71 % | 3 % | 87 % |
| SI score ≥ 117 | 0.201 % | 499× | 83.2 % | 100 % | 100 % | 82 % | 0 % | 44 % | 0 % | 85 % |
| SI score ≥ 141 | 0.107 % | 937× | 82.2 % | 100 % | 100 % | 79 % | 0 % | 40 % | 0 % | 84 % |

Two of SI's measured points fall in the note's operating range (85–256×): 90.5 % recall at 98×
and 87.9 % at 191×. At those two: the square's symmetries and integer rescales 100 % (and
still at 937×), recolours 94 % and 91 %, crops 79 % and 71 %, resamples 44 % and 31 %, pastes
5 % and 3 %. SI alone meets the
note's 99 % nowhere in that range — which is why §7.3 unions it with the exact keys
instead of putting it in front of them.

### 9.3 The funnel at N = 4,000

SI at θ = 2, B = 2,000; exact keys as docs/SEARCH.md (top 32 by Σ 1/df per family, df cap 256);
XRank under the safe policy, copy scope, on the union.

| | candidates per query | recall |
|---|---:|---:|
| exact keys alone | 58.1 | 98.1 % |
| SI alone | 57.4 | 91.9 % |
| SI ∪ keys | 107.4 | 99.8 % |
| SI ∪ keys → XRank says Copy | 107.4 | 99.8 % of 832 (X1: 99.5 %) |

| transform | pairs | keys | SI | SI ∪ keys | → XRank Copy |
|---|---:|---:|---:|---:|---:|
| mirror | 35 | 100 % | 100 % | 100 % | 100 % |
| rot90 | 27 | 100 % | 100 % | 100 % | 100 % |
| rot180 | 37 | 100 % | 100 % | 100 % | 100 % |
| transpose | 31 | 100 % | 100 % | 100 % | 100 % |
| invert | 3 | 100 % | 100 % | 100 % | 100 % |
| recolour | 32 | 100 % | 92 % | 100 % | 100 % |
| chswap | 29 | 93 % | 98 % | 100 % | 100 % (X1: 97 %) |
| palshuffle | 6 | 100 % | 100 % | 100 % | 100 % |
| up2 | 40 | 100 % | 100 % | 100 % | 100 % |
| up3 | 40 | 100 % | 100 % | 100 % | 100 % |
| resample90 | 6 | 92 % | 67 % | 100 % | 100 % |
| up150 | 2 | 25 % | 50 % | 75 % | 75 % |
| crop80 | 20 | 100 % | 90 % | 100 % | 100 % |
| crop67 | 13 | 85 % | 73 % | 100 % | 100 % |
| corner50 | 1 | 50 % | 100 % | 100 % | 100 % |
| paste | 19 | 100 % | 13 % | 100 % | 100 % |
| shift1 | 36 | 100 % | 100 % | 100 % | 100 % |
| dither | 9 | 83 % | 61 % | 94 % | 94 % |
| matte | 30 | 100 % | 93 % | 100 % | 100 % |

XRank per query: p50 17.5 ms, p95 461 ms. Per candidate, per query: p50 196 µs, mean 399 µs,
p95 1,539 µs; on every sixth query's pool without its target (13,976 candidates), 616 µs a
candidate. XRank went past its route and anchor-tier screen on 52.9 % of the candidates it was
shown — nominated candidates are, by construction, the works that look most like the query.
Under X1 (`sibench eval --x1`, the same session): 99.5 % end to end, 153 / 374 / 593 µs, and
52.7 % of candidates past the screen. The difference is X2's two changes together — the
structural door, asked on every candidate the gate would drop, and route derivation 2, which
moves some candidates between route classes; no run here separates them.

XRank read Copy on 92 of the ~89,000 (query, distractor) pairs it was shown, all of them a
noise-field `work` base against a `work` distractor: that generator draws every work over one
shared noise field (the xbench "adversarial collision" class), so these distractors share real
pixels with the bases. A property of the synthetic corpus and of the verifier, not of the
nomination: the index only decides which pairs XRank sees.

### 9.4 Scaling

The same 832 queries against growing populations (the eval distractors, then the extra ones).
The key timings are in-memory sorted arrays, top-32 selection included, not SQL.

| population | SI pool | SI pool share | SI recall | keys@32 recall | SI ∪ keys, no budget | SI top-250 ∪ keys | top-1,000 ∪ keys | top-2,000 ∪ keys (the default B) | top-4,000 ∪ keys | top-16,000 ∪ keys | SI postings read / query | SI query p50 | keys query p50 |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 4,000 | 57 | 1.435 % | 91.9 % | 98.1 % | 99.8 % | 99.6 % | 99.8 % | 99.8 % | 99.8 % | 99.8 % | 778 | 17 µs | 655 µs |
| 16,000 | 231 | 1.442 % | 91.9 % | 96.8 % | 99.4 % | 97.7 % | 99.3 % | 99.4 % | 99.4 % | 99.4 % | 3,093 | 69 µs | 1,639 µs |
| 64,000 | 929 | 1.452 % | 91.9 % | 95.7 % | 99.0 % | 96.4 % | 97.1 % | 97.8 % | 98.9 % | 99.0 % | 12,380 | 286 µs | 3,319 µs |
| 104,000 | 1,509 | 1.451 % | 91.9 % | 95.2 % | 98.8 % | 95.9 % | 96.5 % | 97.0 % | 97.8 % | 98.8 % | 20,140 | 517 µs | 4,037 µs |

SI is scale-free: its share and its recall do not move, so its pool grows with N, and a budget
turns that into a recall cost (the top-B columns). At the default B = 2,000 the union gives up
1.8 points to no budget at 104k: the average pool is 1,509 there, and the larger ones are cut.
The exact keys hold a fixed-size nomination whose recall decays by about 0.6 points per doubling
of N here; the union without a budget decays by about a third of that (0.2 points per
doubling).

### 9.5 What a query costs at scale

Measured per query (native, one core): the SI index 0.52 ms at 104k works, linear in N (0.19
postings read per stored work); the key nomination 4.0 ms in memory at 104k; XRank 0.20 ms (the
median query) to 0.62 ms (negatives, on average) per nominated candidate under X2 (§9.3).

Extrapolated to N = 10 million, under three stated assumptions: (a) recall decays log-linearly
in N at the slope measured from 4k to 104k — the lower figure uses the steeper last segment
(64k → 104k); (b) SI's share stays at the measured 1.45 %; (c) XRank's per-candidate cost stays
at 0.20–0.62 ms. None of the three is measured beyond 104k. The shards are shards by work: each
a complete index (SI and keys) over its own 200k works — about what one 10 GB Durable Object
holds (docs/SEARCH.md §6) — with every query sent to all of them and the shards running in
parallel. (SEARCH.md's other option, sharding the postings by key range, keeps one global
nomination and the first two rows.)

| deployment | SI candidates | key candidates | verified per query | XRank CPU | recall, extrapolated |
|---|---:|---:|---:|---:|---:|
| one index, θ = 2, no budget | 145,000 | 64 | ≈ 145,000 | 28–89 s | ≈ 97 % |
| one index, budget 1,000 | 1,000 | 64 | ≈ 1,060 | 0.21–0.65 s | ≈ 91–92 % |
| 50 shards of 200k, θ = 2, no budget | 2,900 per shard | 64 per shard | ≈ 148,000 in all | 0.58–1.83 s per shard | ≈ 98.5 % |
| 50 shards of 200k, budget 1,000 | 1,000 per shard | 64 per shard | ≈ 53,000 in all | 0.21–0.65 s per shard | ≈ 96 % |

The default B = 2,000 sits between the two kinds of row: twice the budget rows' XRank time
(0.40–1.27 s per index or shard), and a recall between theirs and the unbudgeted rows'.

So the note's arithmetic (§14: 18 min ÷ 256 ≈ 4.2 s) is right about the index and silent about
the verifier: what decides whether a 10-million-work query takes 100 ms or a minute is how many
*nominated* candidates XRank must see. The levers, in order of what they are worth: a real
corpus (§11), sharding (each shard's keys compete against 200k works, not 10M), the budget, and
a cheaper first verifier stage for nominated candidates.

### 9.6 The prototype

Before the Rust implementation, a Python prototype read the same corpus (as exported family
vectors) under the earlier split — odd bases evaluate, which left the `work` and scene kinds out
of the fitting split; §9.1's block split replaced it — with its own PCA and quartiles. Its
numbers chose between alternatives; they are recall of comparator-42 copies at the pool share
given, SI alone, and are not reproduced by `sibench`:

| alternative | pool | recall | against (four axes, four probes) |
|---|---:|---:|---:|
| four probes / eight / sixteen | ≈ 1 % | 86.5 % / 86.3 % / 86.6 % | — |
| five axes (1,024 cells) | 1.00 % | 87.0 % | 86.5 % |
| three axes (64 cells) | 1.04 % | 84.0 % | 86.5 % |
| without KPGEO | 1.04 % | 85.5 % | 86.5 % |
| with the DCT family added | 1.02 % | 85.8 % | 86.5 % |
| re-ranking the admitted pool by projected distance | 1.0 % / 0.5 % / 0.1 % | +2.3 / +2.4 / +0.7 points | the cell score |

It also measured the DCT family's instability directly — a rot90 copy displaced by 0.49 of a
random pair's distance (rot180 0.52, the complement 0.64) — and found second-order products of
the low-frequency DCT signs (invariant by construction under every sign pattern the square's
symmetries and the complement apply) D4-exact but no better under resampling (0.58–0.70) or
crops (0.82–0.98; the corner crop 1.01–1.04).

---

## 10. Verification

```bash
cargo test --release --manifest-path rust/Cargo.toml   # 113 tests: 102 of 1.0.0, 7 of PAPH-SI, 4 of 1.1.1
bash rust/check.sh                                     # equivalence digest: 3,160 cases, byte-identical
npm run test:si                                        # 1,420 checks: native = SIMD128 = baseline for signatures,
                                                       # keys, plans, scores and index answers; index =
                                                       # definition; SQL = index; through removals
npm run bench:si -- --big                              # sibench eval --big: §3, §4, §9.1–§9.4 (-- --fit first: §5)
rust/target/release/sibench route|lost [--x1]          # §3.3's G3 column and second partition, §11's counts
```

The PAPH-SI unit tests: the coarse cell is the exact median-bit parent of sixteen fine cells;
signatures round-trip; `isqrt128` is exact; Jacobi diagonalises; the index equals the scan slot
for slot and score for score through adds, removals and compactions at five thresholds and
budgets (an empty budget and no budget among them), and its entry accounting holds; SI1
round-trips, is bound to X1 and refuses tampering, and SI2 is SI1 bound to X2; on real wires, a
signature from the wires alone equals the one from an X side, mirrored, rotated and 2×-upscaled
copies clear the default
threshold (30 of 30 on the logged run; the test requires 27) and unrelated works do not (0 of 90;
it allows 9). `npm run test:si` also checks that SI1 under X1 and SI2 under X2 sign every work
of its corpus alike and that SI1 refuses a side prepared under X2. Separately, re-running the
fit reproduces SI2 (and, with `--x1`, SI1) byte for byte.

---

## 11. Known gaps and next steps

* **No real corpus.** SI2 (SI1's fit) is fitted and measured on the synthetic corpus only, and its name says
  PROVISIONAL. Synthetic same-style art is homogeneous — the effective cells of §4.3 are a floor,
  not an estimate — and every threshold here must be re-derived on Pixagram's own works before
  any operating point is trusted. `sibench fit` takes a corpus file in the format
  `sibench corpus` writes.
* **The XRoute class is a sketch, not a filter.** On this corpus X2's route bars — X1's,
  local 6 / band 3 / global 190, calibrated on the 344-work xbench corpus — put 42 of 976
  comparator-42 Copy pairs in the route's Reject class (`sibench route`; 49 under X1's
  derivation): paste 9 of 40, dither 8 of 20, chswap 5 of 72, crop80 5 of 47, resample90 4 of 15,
  corner50 4 of 5, recolour 3 of 75, palshuffle 2 of 20, up150 1 of 6, crop67 1 of 25. Bars that
  keep every copy out of the class (local 1 / band 3 / global 164) would leave 55 % of unrelated
  pairs in it instead of 93 %, so 1.1.1 keeps the bars and stops acting on the class alone
  instead: under X2 neither the pair screen nor XRank's gate drops a route Reject until the
  structural door has shut — an exact bound, so the pair cannot be a structure-only Copy — and
  the pair screen rejects none of the 976 copies (X1: 2). A host must not drop candidates on the
  route class alone either, and the bars want the real corpus.
* **XRank's gate lost three pairs under X1; X2 keeps them.** Shown the target alone, XRank under
  X1 (`gate: true`) did not read Copy on 6 of 1,952 comparator-42 Copy queries — three pairs,
  both arrival orders (`sibench lost --x1`): two channel-swapped pairs and one palette-shuffled
  pair. All three are works with 0–3 keypoints a side, so the anchor-tier pools are empty under
  both hypotheses, and comparator 42 certifies the pair on structure alone ("structural only —
  no geometric corroboration": structural 6,059–6,279, geometry evidence 0, no inliers). XRank
  keeps such copies when its route reads them as Fast, which it does not gate; the recolour moved
  these three out of that class, and X1's gate read nothing else: two left at its first exit
  (route Reject, pool ≤ 3), the third (route Defer) at its second (pools still below
  `geo_min_corr`, 8, after the expansion tiers). 1.1.0 put this down to the recolour moving the
  keypoints; there are almost none to move. Under X2 both exits ask the structural door first,
  which keeps the first two, and route derivation 2 reads the third as Fast; XRank reads Copy on
  all 1,952 queries (`sibench lost`). The one pair in the eval split was §9.3's 0.3-point gap
  between nomination and verdict, now closed. The door costs about 17 µs natively on an
  unrelated pair, asked only where the gate would drop one (docs/PAPH-X.md §4).
* **Comparator 42's own gated rank drops far more, and is unchanged.** `rank` with `gate: true`
  (`paph_rank42`, flags bit 0) — the verifier of docs/SEARCH.md §4 and of the pixagram-search
  integration — screens out every pair whose stage-1 pools stay below `geo_min_corr` (8): on
  this corpus 178 of the 976 comparator-42 copies, 164 of them — the three recolours above among
  them — because a side has fewer than 8 keypoints and can never pass (`sibench lost`). XRank
  does not gate a candidate its route reads as Fast (172 of the 178), and under X2 reads Copy on
  all 976. Verify with XRank, or with `rank` and `gate: false`.
* **Blur-like transforms.** Box-filter resamples and non-integer rescales move every wire section
  the families read (the front end quantises to a palette and downsamples by majority, which
  blending defeats). Comparator 42 rarely certifies them either: of 120 pairs each, it reads
  Copy on 0 down70, 15 resample90 and 6 up150 pairs.
* **Unexplained shortfalls.** SHAPE, SIL and KPGEO under mirror, rot90 and rot180, and LOCAL and
  BAND under the complement, fall short of their design (§3.2) for reasons not established here.
  For the first three, §3.3 rules out the thumbnail's and the shapes section's grids, and their
  holding under transpose points at rounding anchored to the top-left corner; §3.2 names the
  candidates read from the code. Fixing any of them changes the cells, so it is a new SI profile
  and a re-index, not a patch.
* **Continuous re-ranking.** Storing each family's four projections (24 bytes) and re-ranking the
  admitted pool by noise-scaled distance measured +2.3–2.4 points at 0.5–1 % pools and +0.7 at
  0.1 % in the prototype (§9.6). Not worth a signature change before the real corpus says
  otherwise.
* **The verifier's cost on nominated candidates** (§9.5) is now the binding constraint at
  millions of works; a cheaper first stage for them (anchor-tier counts only, before any
  cascade) is the next lever, and belongs to PAPH-X, not to the index.
* **The pixagram-search integration** (`integrations/pixagram-search`, a patch on that
  repository: the `PaphIndex` Durable Object, D1 verdicts) does not carry PAPH-SI yet. The
  schema, the statement and the glue are here; adding `si_works` / `si_postings` beside its
  key tables and the SI nomination beside its Σ 1/df one is the next step, made against that
  repository.
