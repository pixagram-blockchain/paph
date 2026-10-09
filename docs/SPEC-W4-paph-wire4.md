# PAPH wire 4 — sampling that commutes with the square's symmetries

`@pixagram/paph-x` 1.2.0. Wire 4 is SPEC-003's Tier 1 and Tier 2 — the same sections, sizes,
offsets and flags — with five things sampled differently, so that the sections of a mirrored or
quarter-turned image are the sections of the original, moved by the same symmetry, exactly, on
canvases of any size. Three fields change their encoding with it: a shape record's bytes 6–7 hold
its box's width and height instead of their rounded ratio (§4), the silhouette's bytes 40–55 count
each row's and column's opaque runs instead of its transitions, and its bytes 66–69, zero in wire
3, hold its component's box (§6). Wire 3 is unchanged and still written on request. Nothing in
Tier 2 but its version byte and its reference to Tier 1, the keypoint pipeline, the palette, the
adjacency graph, the run geometry, the local fingerprints, the anchors, the colour digest or the
sketch changes: the sections wire 4 rewrites are the DCT, the brightness record, the shapes and the
silhouette.

| | wire 3 (SPEC-003, 1.0–1.1) | wire 4 (1.2) |
|---|---|---|
| header byte 4, Tier 1 and Tier 2 | 3 | 4 |
| 16 × 16 thumbnail (and the brightness record read from it), shapes grid (§2) | cells ⌊i·w/n⌋ … ⌊(i+1)·w/n⌋ | closed: ⌊i·w/n⌋ … ⌈(i+1)·w/n⌉ |
| DCT, 16 × 16 / 8 × 8 / 4 × 4 (§3) | two passes, each rounded `(s + ½) >> 14` | one rounding of the exact double sum, half away from zero |
| shape regions (§4) | the eight largest, ties by position; bytes 6–7 ⌊256·w/h⌋ | ordered by keys no symmetry moves; bytes 6, 7 the box's width and height |
| radial profiles, shapes and silhouette (§5) | from the integer centroid, to the first step outside | from the exact centroid, to the last step inside |
| silhouette (§6) | first largest component; moments about the integer centroid; transitions; open occupancy cells | ties by keys no symmetry moves; exact central moments; opaque runs; closed cells; box at bytes 66–69 |

Why it changed: in wire 3 the 16 × 16 thumbnail's cell edges commute with a flip only when 16
divides the side, a quarter turn swaps which of the DCT's two rounded passes runs first, the
shapes section breaks ties by position and casts rays from a rounded centroid, and the
silhouette counts transitions from one margin only. A mirrored or turned copy then has sections
that are *near* the original's moved sections, not equal to them, and every reader that wants an
invariant — the route's global words, PAPH-SI's cells, an index key — inherits the difference.
91 % of the Pixa chain's artworks have a side that is not a multiple of 16; on their D4 copies
1.1.2 measured the route's DCT word equal to the original's on 25 % and its region word on 46 %
(docs/calibration/SI3-PROVISIONAL.log). Wire 4 removes the cause instead of tolerating it (§10
measures it).

The changes are confined to sampling: no section gains or loses a meaning, and no comparator
threshold reads a wire-4 section differently from a wire-3 one — of the three re-encoded fields
the comparators read only the shape records' bytes 6–7, through a helper that returns wire 3's
value (§8); the silhouette's run counts and box are read by the route and PAPH-SI. The comparators,
calibrations and profiles are format-agnostic — but a wire-3 side and a wire-4 side of one image
differ where the sampling differs, so a pair of mixed formats is refused (§9).

## 1. Identity

* Tier 1 byte 4 and Tier 2 byte 4 hold the format, 3 or 4. The Tier-1 parser checks its byte
  and accepts both and nothing else ("v2 wires MUST be rejected", SPEC-003 §13, still holds); a
  side's format is its Tier 1's. Tier 2's byte is written, not checked: its keypoint records are
  the same in both formats.
* From 1.2 the Tier-1 parser, in every engine and for both formats, also holds the section table's
  record counts to what each section holds (the checksum covers the sections, not the table, and a
  larger count sent a reader past the section's end), and on wire 4 a counted shape record's box
  sides to 1–128 (§4). A wire the hasher wrote always passes.
* `hash_profile_id` (SPEC-004 §7) hashes `"PAPH-HP"`, a format byte (0x03 or 0x04), then the ten
  hash-time fields: a profile id names the format its wires are in. The defaults' id is
  `8b0af945…` on wire 3 (1.0–1.1's, unchanged) and `de456c49…` on wire 4.
* `Config::wire` (Rust), `wire` (JavaScript and the WebAssembly glue's options), and the 22nd
  field of the ABI's flat configuration (ABI 4; [WASM-ABI.md](WASM-ABI.md)) select the format;
  the default is 4. Wire 3 is produced byte for byte as 1.0–1.1 produced it: the equivalence
  digest of 1.0–1.1 (test/equiv-digest.txt, 3,160 cases) is unchanged, natively and in both
  WebAssembly builds, and a second digest (test/equiv-digest-4.txt, 3,164 cases) records wire 4.

## 2. Closed cells

The area-majority sampling (the 16 × 16 thumbnail and the shapes section's grid) splits `w`
pixels into `n` cells. Wire 3's cell `i` is `[⌊i·w/n⌋, ⌊(i+1)·w/n⌋)`, which gives a pixel cut by
an edge to the cell on its right; a flip gives it to the other side, and the copy's cell `n−1−i`
is not the original's cell `i` unless `n` divides `w`. Wire 4's cell is closed:

    x0 = ⌊i·w/n⌋,   x1 = ⌈(i+1)·w/n⌉        (pixels x0 … x1−1; if x1 ≤ x0 then x1 = x0 + 1)

A pixel an edge cuts belongs to both cells. Since `w − ⌈(i+1)·w/n⌉ = ⌊(n−1−i)·w/n⌋`, a flip maps
cell `i` onto cell `n−1−i` exactly; where `n` divides `w` the spans are wire 3's. The majority
rule is unchanged: the most frequent slot of the pixels in the cell, ties to the higher
luminance rank, transparency ranking below every palette entry — a strict total order, so the
cell's value does not depend on the order its pixels are visited. The rule is used by the
thumbnail (n = 16 both ways) and the shapes grid (⌈w / c⌉ × ⌈h / c⌉ cells, at least 4, c = ⌈long
side / 128⌉), and the silhouette's 8 × 8 occupancy grid over its component's box (§6) uses the
same spans.

## 3. The DCT, rounded once

Each of the hierarchical DCT's blocks (the 16 × 16 thumbnail, its four 8 × 8 quarters, its
sixteen 4 × 4 tiles) is transformed with the integer cosine table `t` (Q14) of SPEC-003. Wire 3
rounds after each pass: `row = (Σᵢ src·t + 2¹³) >> 14`, then the same down the columns. `(s + ½)
>> 14` is not odd (it rounds −½ and +½ differently), so a flip, which negates the odd
frequencies, can move a coefficient by one; and a quarter turn transposes the block, which
exchanges the passes. Wire 4 keeps the double sum exact and rounds once, half away from zero:

    S[v][u] = Σⱼ Σᵢ src[j][i] · t[u][i] · t[v][j]
    out[v][u] = sgn(S) · ((|S| + 2²⁷) >> 28)

The tables are exactly odd/even symmetric (`t[u][n−1−i] = (−1)ᵘ · t[u][i]`, a test checks it), so a
horizontal flip negates exactly the coefficients of odd `u`, a vertical flip those of odd `v`, and
a transpose exchanges `u` and `v` — and every symmetry of the square is a composition of those.
`|S| < 2⁴⁴`, inside the integers a double holds, so the JavaScript engine computes the same
integers. Quantisation (sign bit, Gray-coded magnitude against the block's own order statistics,
DC dropped) is unchanged.

One residue is the code's, not the sampling's: a coefficient that is exactly 0 has no sign, its
stored sign bit is 0, and the bit operation that negates a mirrored block's odd frequencies on
the stored code would set it. The magnitude bits are exact; the sign bits differ only at
coefficients that are 0 (§10).

## 4. The shapes section

The label map (the shapes grid of §2, each cell its colour's luminance-quantile band, 8 bands,
transparent cells unlabelled) and its 4-connected components are wire 3's. What changes is
which eight are kept, in what order, and bytes 6–7 of a record.

**Order.** Wire 3 sorted by area, then by the top edge, then by the left edge of the box: a tie
in area — common on small grids — is decided by position, which a symmetry moves. Wire 4 sorts by
keys no symmetry moves, and keeps the first eight:

1. area, descending;
2. perimeter (cells on the grid's border or with a 4-neighbour outside the component), descending;
3. holes (complement components inside the box that never touch its border), descending;
4. the longer side of the box, descending; 5. the shorter side, descending;
6. the canonical radial profile (§5: the least of the profile's eight images under the square's
   symmetries, compared byte by byte), ascending;
7. the band folded under inversion, `min(b, 7 − b)`, ascending;
8. the scan order (the component's first cell in raster order) — which decides only between
   regions equal on every key above, and their records are then equal up to orientation: a
   record holds nothing the keys leave free but its orientation (each record put in its own
   canonical orientation is the other's). The comparator's 64 alignments, the route and PAPH-SI
   read them through it.

Only components at least as large as the eighth largest are measured for keys 3–7.

**Record** (41 bytes, as wire 3): area (u32), perimeter (u16), **box width (u8), box height
(u8)**, holes (u8, clamped), the 32-byte radial profile (§5). Wire 3 stored ⌊256·w/h⌋ (u16) at
bytes 6–7, which a transpose does not map to ⌊256·h/w⌋ exactly; the sides are 1 to 128 (the grid
is at most 128 cells a side, and a parser refuses a counted record outside that, §1), and a
transpose exchanges them. Readers take the ratio from the sides (§8).

## 5. Radial profiles from the exact centroid

A region's (and the silhouette's) radial profile is its extent along 32 rays at multiples of
2π/32, scaled to its own maximum. Wire 3 cast them from the integer-truncated centroid (or, when
that pixel was outside the component, the nearest member pixel), stepping in rounded Q10 increments
and stopping at the first step outside: three roundings, none of them symmetric.

Wire 4 works in exact rationals. Pixel (i, j) is the closed unit square centred on (i, j). The
centroid is (Σx / area, Σy / area). Ray k's point at step s is the centroid plus
s · (RAYC[k], RAYSN[k]) / 1024, with RAYC, RAYSN SPEC-003's Q10 tables (which satisfy RAYC[k+8] =
−RAYSN[k], RAYSN[k+8] = RAYC[k], RAYC[16−k] = −RAYC[k]; a test checks them). The point is
**inside** when every pixel whose closed square holds it is in the component — one pixel, two on
an edge, four on a corner — and the extent is the **last** step that is inside, not the first that
is not: a centroid in a hole, or between the arms of a U, needs no stand-in pixel. Stepping stops
when the point leaves the component's open bounding box (it cannot re-enter a convex set it
started in) or at s = width + height of the box. The profile is ⌊255 · rₖ / max(1, maxₖ rₖ)⌋.

Every comparison is between exact rationals with a common denominator (2048 · area), the inside
test is symmetric in its edges, and the tables are symmetric, so the profile of a mirrored or
quarter-turned component is the original's, permuted:

    ray k after symmetry e (bit 2: swap the axes, then bit 0: flip x, then bit 1: flip y)
      swap: k → 8 − k     flip x: k → 16 − k     flip y: k → −k        (mod 32)

The arithmetic stays below 2³⁸.

## 6. The silhouette

Computed from the alpha mask after the matte fold, abstaining as in wire 3 (no opaque pixel, more
than 98 % or less than 2 % opaque). Among the opaque pixels' 4-connected components wire 3 kept
the first largest the scan met (and cast its rays from the nearest member pixel, the first the
scan met among equals, when the integer centroid fell outside it — another tie a symmetry moves).
Wire 4 keeps the largest and breaks a tie by keys no symmetry moves, each computed only for the
components still tied:

1. the longest perimeter (as §4's), then the longer and then the shorter side of the box;
2. the least canonical profile (§5);
3. the greater principal moments: the larger of M20 and M02, then the smaller, then |M11| (the
   exact central moments of the table below, which a mirror leaves in place and a transpose
   exchanges);
4. the least canonical occupancy code (bytes 56–63);
5. the scan order.

The record holds more than §4's — moments, fill, occupancy — so a tie the profile leaves needs the
deeper keys. Among the free polyominoes of up to 13 cells the profile first leaves ties at 10 cells
(four pairs), and every group it leaves up to 13 is settled by the moments but one pair, which the
occupancy settles: none reaches the scan order (`node tools/silhouette-ties.cjs`, in
X3-PROVISIONAL.log). The scan order decides only between twins, one shape in two orientations,
which no such key can tell apart; the record is then the moved record up to that orientation.
`docs/golden/GOLDEN-W4.json` holds those five pairs and a pair of twins (§11).

| bytes | wire 3 | wire 4 |
|---|---|---|
| 0–31 | radial profile (wire 3's rays) | radial profile (§5) |
| 32–35 | m20, m02, \|m11\|, sign of m11 about the integer centroid, ⌊1020·m / (n·(w²+h²))⌋ | the same about the exact centroid: with Mₚq = area·Σ… − Σ…·Σ… (integers, below 2¹⁰⁶), ⌊1020·M / (area²·(w²+h²))⌋ |
| 36–37 | ⌊256·w/h⌋ of the box | unchanged |
| 38–39 | fill, component count | unchanged |
| 40–55 | rows / columns by their number of changes between transparent and opaque (0 … 7+), a transparent margin assumed on the left (top) and none on the right (bottom) | rows / columns by their number of opaque runs (0 … 7+); both ⌊255·count / h⌋ for rows and ⌊255·count / w⌋ for columns |
| 56–63 | D4-canonical 8 × 8 occupancy, open cells | the same, closed cells (§2) |
| 64–65 | opaque fraction | unchanged |
| 66–69 | 0 | box width, box height (u16 each) |
| 70–95 | 0 | 0 |

Wire 3's transition count gave a row that ends opaque one fewer than its mirror image (the right
margin was not counted); a run count is the transitions against both margins halved, and a flip
leaves it where it was.

## 7. The claim, and how it is tested

For every image and each of the square's eight symmetries g, wire 4 gives: thumbnail cell (i, j)
of g(image) = cell g(i, j) of the image; every block of the DCT hierarchy (the 16 × 16, the four
8 × 8 quarters, the sixteen 4 × 4 tiles) of g(image) = the block g moves there, its coefficients
with u and v exchanged under a transpose and the odd frequencies negated under each flip — and in
the stored section each code's magnitude bits move with their coefficient and its sign bit flips
exactly where a nonzero coefficient is negated (§3); the same multiset of shape records, each read
back through g (sides exchanged under a transpose, profile permuted), where regions tied on every
key of §4 agree up to their own orientation; the silhouette's profile permuted, its moments
exchanged and its m11 sign flipped as g dictates, its fill, count, occupancy code, opacity and box
exchanged as g dictates, its row and column run histograms exchanged under a transpose, and a
component tied on every key of §6 equal up to its own orientation; the brightness record's first
seven bytes and the flags unchanged, and its byte 7's four quadrant fields moved with their
quadrants. Consequently XRoute's four global words (route derivation 2) are equal.

`rust/src/wire4.rs` checks every one of those, exactly, on 532 D4 copies: every pixel-art family of
`synth.rs` at sizes that are and are not multiples of 16, below and above the shapes grid's 128,
on transparency, a matte and a dithered backdrop, plus 36 random sizes. On 16 of the copies a
region is tied with its own rotated twin, and their records agree up to that orientation. On the
same works wire 3 fails the checks on 493 of the 532 copies (the route's DCT word moves on 388, its
region word on 205). A second test (`silhouette_ties`) runs the checks on GOLDEN-W4's tie
canvases: five pairs tied through the profile — four the moments settle, one the occupancy — come
out exact under all seven symmetries, and the twins equal up to orientation under four of them.

## 8. Readers

Nothing that reads a section changes its meaning. Three helpers of `Tier1` (rust/src/wire.rs)
read the re-encoded boxes — a shape record's bytes 6–7 and the silhouette's — in either format;
the JavaScript engine, whose comparators need only the first, has that one (`shapeAspect`):

* `shape_aspect(record)` — the value wire 3 stores, ⌊256·w/h⌋, recomputed from the sides on wire
  4. The comparators' shape channel reads it, so comparator 3, 4, 41 and 42 read a wire-4 record
  exactly as they read a wire-3 one.
* `shape_asym_q8(record)`, `silhouette_asym_q8()` — the box's elongation, longer side over
  shorter in Q8 (256 = square): exact from the sides on wire 4; from wire 3's rounded ratio `a`
  as max(a, ⌊65536 / a⌋), which a transpose can move across a class edge at ratios past about 7.
  XRoute's region word (G3) and PAPH-SI's SHAPE and SIL families read these.

## 9. Mixed formats, and moving a corpus

The two formats of one image differ wherever the sampling differs, and no calibration was fitted
across them. A pair of a wire-3 side and a wire-4 side is therefore refused, never compared:

| | a mixed pair |
|---|---|
| comparator 3 (`compare`) | error `tier 1 wire formats differ (3 and 4): hash both sides with one format` |
| comparators 4, 41 and 42 (`compare`), comparator 42's `rank` | `Indeterminate`, reason `WIRE_MISMATCH` (`rank`: that candidate's report) |
| comparator 42's `screen` | screened as any pair: it reads only Tier 2's keypoints, which wire 4 does not change, and is never a verdict |
| PAPH-X: `xscreen` | state `Refused`, reason `WIRE_MISMATCH` (the JavaScript glue throws) |
| PAPH-X: `xcompare` | `Indeterminate`, reason `WIRE_MISMATCH` |
| PAPH-X: `xrank` | that candidate's record: `Indeterminate`, screen `Refused` |
| PAPH-SI | signatures carry no format; an index holds one format's (below) |

Either format against itself is compared as before. What wire 4 does not touch is the same in
both formats, byte for byte: Tier 2's keypoint records, the local fingerprints, the anchors, the
sketch, the palette, the adjacency graph, the run geometry and the colour digest — and so the
index keys (`indexKeys`, KEYS_VERSION 1), which come from the local codes and the keypoints. An
exact-key index built on wire-3 wires keeps working across a store that is being re-hashed. A host
that stored 1.0–1.1 fingerprints moves in one of two ways:

1. **Re-hash every stored work** with 1.2 (wire 4), then rebuild what was derived from the
   resampled sections — PAX1 sidecars (1.2's name the Tier 1 they were derived from, and 1.1's are
   refused, so a stale one is never used) and PAPH-SI signatures and postings under SI4 (SPEC-SI
   §7.4); the index keys stay. Until a
   work is re-hashed, hash queries against it in both formats (`hash(px, { wire: 3 })` beside the
   default) and compare each stored side with the query of its own format; Tier 1 byte 4 says
   which. This is the path to take: wire 4 is what the shipped profiles were measured on.
2. **Stay on wire 3**: hash with `{ wire: 3 }` (`Config { wire: WIRE_3, .. }`). Comparator 42
   under CAL-007, XRank under X3 and PAPH-SI under SI4 run on wire-3 sides, but their 1.2 figures
   were measured on wire 4; 1.1's profiles (CAL-004, X2, SI3) load from their artefacts.

Re-hashing is what SPEC-003 §13 already asked of a format change; nothing about a work but its
image is needed.

## 10. Measured

`sibench route` on the D4 copies (mirror, rot90, rot180, transpose) of every base, the same works
hashed in each format: the Pixa chain's 174 real bases (one snapshot, 696 copies; 1.1.2's wires
are the wire-3 column, re-hashed for the wire-4 one) and the PAPH-SI corpus's 120 synthetic bases
(480 copies). Every run is in `docs/calibration/X3-PROVISIONAL.log` but one: the chain's wire-3
SI cells are 1.1.2's run under SI3 (`SI3-PROVISIONAL.log`), the profile it shipped; the X3 log
reads the same hashes under `--x2`, whose SI cells are SI2's.

| equal to the original's on D4 copies | chain, wire 3 | **chain, wire 4** | synthetic, wire 3 | **synthetic, wire 4** |
|---|---:|---:|---:|---:|
| route G0 (DCT), both sides multiples of 16 · other sizes | 96 % · 25 % | **100 % · 100 %** | 87 % · 41 % | **100 % · 100 %** |
| route G3 (regions), both sides multiples of 16 · other sizes | 95 % · 46 % | **100 % · 100 %** | 83 % · 84 % | **100 % · 100 %** |
| route G1, G2 | 100 % | 100 % | 100 % | 100 % |
| a low-frequency DCT magnitude bit differs (per coefficient, range · median) | 2.2–15.2 % · 10.3 % | **0.0 %** | 2.4–13.7 % · 6.8 % | **0.0 %** |
| a low-frequency DCT sign bit differs | 0.7–9.7 % · 5.8 % | 0.0–0.3 % · 0.0 % | 1.3–9.6 % · 4.5 % | 0.0–1.3 % · 0.4 % |
| PAPH-SI SHAPE cell, multiples of 16 · other sizes | 68 % · 53 % | **100 % · 100 %** | 74 % · 80 % | **100 % · 100 %** |
| PAPH-SI SIL cell, multiples of 16 · other sizes | 75 % · 83 % | **100 % · 100 %** | 74 % · 72 % | **100 % · 100 %** |
| PAPH-SI TONE cell, multiples of 16 · other sizes | 100 % · 93 % | **100 % · 100 %** | 100 % · 98 % | **100 % · 100 %** |

The SI cells are SI3's on the chain's wire-3 hashes (1.1.2's shipped profile), SI2's on the
synthetic ones, and SI4's on wire 4; a feature that is exactly invariant lands in the same cell
under any codebook. The sign bits that still differ are the zero coefficients of §3. RUNS and PAL
were already exact; KPGEO reads the keypoints, which wire 4 does not touch (39–58 % of the chain's
copies keep their cell under SI4, as under SI3).

What it changes downstream, on the same works and transforms:

* **Comparator 42 certifies more synthetic copies**: 995 of the 2,400 transformed pairs of the
  PAPH-SI corpus are Copy on wire 4, against 976 on wire 3 (under CAL-004, and under CAL-007,
  which no Copy moves). On the chain's real bases it is 3,095 of 3,480 in both formats.
* **The route class.** Of those copies 255 of 3,095 real and 46 of 995 synthetic sit in the
  route's Reject class (wire 3: 262 and 42); the pair screen rejects none, in either format.
* **PAPH-SI.** SI3's fit, re-run on the wire-4 hashes (SI4, docs/SPEC-SI-paph-si.md §9.8), keeps
  85.0 % of held-out copies at 0.93 % / 0.51 % of unrelated real pairs (SI3 on wire 3: 85.7 % at
  0.90 % / 0.48 %).

**Cost.** Hashing in wire 4 costs what wire 3 did within 9 % either way. Natively
(`rust/bench.sh`, seven images, best of five batches) wire 4 takes 0.92–1.09 times wire 3's time;
in WebAssembly (`npm run bench`, the SIMD128 build) 1.01–1.09 times, the most on the 96 × 96
sprite. The rays of §5 step by carries, not divisions, and regions of one small shape are measured
once, keyed by their box and cell mask; both are output-identical, as the digest shows.

## 11. Conformance

SPEC-003 §10 holds unchanged — integer or fixed-point everywhere, every sort total with a
content-derived tie-break, intermediates below 2⁵³ (the moments of §6 are the one place an engine
needs wider integers: Rust uses i128, JavaScript BigInt). The vectors:

* `docs/golden/GOLDEN-W4.json`, emitted by the reference (`printf g | rust/target/release/paphcli`;
  a test holds the file equal to the emission): the hash-profile ids, closed and open cell spans,
  the DCT rounded once beside wire 3's two passes, the 8 × 32 ray permutation, radial extents on
  four masks (an L, a ring, a U, a staircase), the silhouette's tie canvases of §6 (cells stamped
  on transparency: five pairs and the twins, each canvas's silhouette section and its seven other
  D4 images', and the key that settled it), and six `synth::pixel_art` images — their pixels'
  SHA-256 first, so a port checks its generator — hashed in both formats, every Tier 1 section's
  digest, and the seven other D4 images of three of them. `test/wire4-golden.cjs` checks the
  JavaScript engine and both WebAssembly builds against it without a native binary (`npm test`).
* `test/wire4-parity.mjs`: the JavaScript engine against the native reference, byte for byte, on
  the shared corpus, generated sprites, scenes and works at nine sizes and the tie canvases, each
  with its mirror, a quarter turn and a half turn, in both formats, and the mixed pair refused by
  both engines' comparator 42 (`npm run test:wire4`).
* `rust/src/wire4.rs`: §7's claim, and the refusals.
* The equivalence digests (§1): `rust/check.sh`, `npm run test:equiv`.
