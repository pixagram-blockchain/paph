# PAPH v3 — Proposed Technical Specification

**One algorithm, two halves, one wire.**

| | |
|---|---|
| Document | PAPH-SPEC-003 (proposal) |
| Date | 18 August 2026 |
| Status | **Proposal.** Nothing here is measured yet. Every number is a target or a hypothesis, and §14 says which. |
| Inputs | PAPH-SPEC-002 (descriptive spec of BOF + L2), the 16-work real corpus, the measurements in both |
| Budget | Relaxed. See §4. |
| Format 4 | `@pixagram/paph-x` 1.2 writes this layout as format 4 ([SPEC-W4](SPEC-W4-paph-wire4.md)): the same sections, the DCT, the shapes section and the silhouette sampled so that a mirror or a quarter turn moves them exactly. Format 3, as specified here, is still written on request. |

---

## 1. What this changes and why

v2 shipped two algorithms that do not talk to each other:

- **BOF** — 1357 bytes, integer, indexable, consensus-safe, invariant to recolour / inversion / mirror / D4. **Recall 0.45.**
- **L2** — no artefact, float, O(N) per query, not consensus-safe, invariant to crop / collage / arbitrary rotation and scale. **Recall 0.82.**

The better detector is the one that cannot be stored, cannot be indexed, and cannot be verified by a second party. That is the problem v3 exists to solve.

Three changes carry the proposal:

1. **The geometric stage becomes a stored artefact.** Keypoints, descriptors, scale and orientation go on the wire. Geometric verification then runs **wire-to-wire**, never touching pixels after upload.
2. **The geometric stage becomes integer.** Every float in L2 is replaceable — the orientation by an integer sector test, the RANSAC by a Hough vote plus a closed-form fixed-point least-squares fit. If that holds, geometric verification becomes consensus-verifiable, which it has never been.
3. **Irreversible hash-time decisions become reversible compare-time decisions.** This is what the extra bytes are actually for. See §2.

---

## 2. Design principles

These are the rules the whole proposal follows. Where a later section looks arbitrary, it is following one of these.

### P1 — Spend bytes to move decisions from hash time to compare time

Every knob in v2 that is a genuine trade-off — `keepDC`, `ragEndpoint`, `evidence`, `scoring` — is currently baked into the wire at hash time. Getting it wrong means **re-hashing the corpus** to find out.

v3 stores both sides of every such trade-off and decides at compare time. The cost is bytes. The return is that all remaining calibration debt (SPEC-002 §23) can be paid **without re-hashing anything**, on the first hundred real moderation reports, as many times as it takes.

This is the single best use of a relaxed budget and it is worth more than any individual knob setting.

### P2 — Compute symmetries from stored bits, never store them

v2 already does this twice, well: the DCT stores sign separately from magnitude so all eight D4 symmetries are bit operations at compare time; local fingerprints fold the complement into the canonical form so inversion costs nothing. v3 extends the same trick to BRIEF (§8.6) so mirror-invariance costs zero bytes.

### P3 — Every channel declares a null, or abstains

Inherited unchanged from v2 §5.1. Extended to the geometric channel, which in L2 reports a bare count against no null at all (§9.4).

### P4 — Symmetry by construction, not by promise

Inherited from v2 §5.2: sort the two wires, run everything in that frame, map directional outputs back. L2 violates this today. v3 does not get to.

### P5 — Separate signals that can be present-or-absent from signals that are always present

Silhouette agreement is powerful evidence when it exists and proves nothing when it does not — a sprite lifted from a sheet keeps its silhouette, the same sprite composited into a scene does not. v2's two halves handle this incompatibly (BOF suppresses silhouettes with a 75% opacity floor; L2 keys on them because it flattens alpha to black). v3 **isolates** silhouette evidence into its own abstaining channel rather than banning it or letting it contaminate interior evidence.

---

## 3. Non-goals

Stated so they do not get designed in by accident.

- **Not a learned embedding.** Measured and rejected: DINOv2-small gives 0.27 recall and does not recover the hard pairs, because general *semantic* embeddings have exactly the genre-floor problem geometry was introduced to solve. If a learned stage is ever added it must be **copy-detection-trained** (SSCD-class), and it belongs outside this spec.
- **Not recall 1.0.** The target is ≥ 0.82 at precision 1.00 *while gaining* indexability, recolour/mirror/inversion invariance and consensus-verifiability. Two corpus pairs may be genuinely out of reach for any compact method.
- **Not a new scoring philosophy.** The channel/null/abstain contract of v2 §5.1 is the best-tested thing in this family. It is inherited verbatim.
- **Not backward compatible.** v2 wires cannot be upgraded in place (§13).

---

## 4. Budget

The wire splits into two tiers with different rules, because they have different consumers.

| | **Tier 1 — Core** | **Tier 2 — Forensic** |
|---|---|---|
| Target size | **≤ 4 kB**, fixed layout | **≤ 12 kB**, variable length |
| Consumer | Chain / canonical record / retrieval index | Moderation database |
| Layout | Constant offsets, section table, no length prefixes | Fixed-size records, declared count |
| Addressing | Canonical artefact | Content-addressed by Tier 1's checksum |
| Contains | Everything needed for retrieval and a verdict on the clean cases | Full keypoint records for geometric verification and moderator evidence |
| Must fit a chain operation | Yes, if the chain allows it | No |

**Open constraint.** I do not know the Pixa chain's `custom_json` payload ceiling. If it is 8 kB, Tier 1 fits comfortably even base64-encoded (~5.4 kB). If it is smaller, Tier 1 has a compression path (§6.6). This is a number to confirm before the layout is frozen, not after.

**Storage arithmetic.** 16 kB per work total. One million works = 16 GB. Not a constraint.

---

## 5. Unified front end

Both halves **MUST** consume the identical normalised image. This resolves three of the seven conflicts in SPEC-002 §22 by construction.

1. **Decode.** RGBA8. The decoder is part of the algorithm (SPEC-002 §14) and **MUST** be specified: in-house integer PNG decode, no Canvas, no `sharp`, on every path.
2. **Matte fold** — v2 §2.2 unchanged. Border-ring modal seed, 4-bit colour quantisation, tolerance flood, 2%–90% area gate.
3. **Upscale division** — v2 §2.3 unchanged.
4. **Luma** — `(77r + 150g + 29b + 128) >> 8`, **one definition**, both halves. v2's L2 omitted the rounding term.
5. **Alpha is first-class on both paths.** Transparent pixels are `-1`, never black. Keypoints in low-opacity neighbourhoods are rejected by the same 75% rule the local-fingerprint windows use (v2 §3.6.2). Silhouette evidence is recovered separately and deliberately in §6.5.
6. **Coordinates are aspect-true.** All stored positions are `round(p · 65535 / max(w, h))` from the canvas origin, in the normalised frame. This is the fix for SPEC-002 §13.1 — the single highest-value one-line change available — and it applies to both anchor positions and keypoints.

> **Consequence worth stating.** L2's measured 0.82 recall was achieved *with* silhouette signal available to it. After change 5, some of that recall may move from the geometric channel to the silhouette channel. That is correct behaviour and it is also a re-measurement obligation (§14).

---

## 6. Tier 1 — Core wire

### 6.1 Layout

| Offset | Size | Section | vs v2 |
|---|---|---|---|
| 0 | 64 | header + section table | 16 → 64 |
| 64 | 264 | hierarchical DCT + explicit brightness record | 256 → 264 |
| 328 | 96 | identity palette, 24 × 4 | 64 → 96 |
| 424 | 288 | sparse RAG, 48 × 6 | 128 → 288 |
| 712 | 328 | shape signatures, 8 × 41 | 205 → 328 |
| 1040 | 48 | run-length geometry | unchanged |
| 1088 | 1024 | local fingerprints, 128 × 8 | 512 → 1024 |
| 2112 | 512 | anchor positions, 128 × 4 | 128 → 512 |
| 2624 | 96 | **silhouette signature** | new |
| 2720 | 80 | **absolute colour digest** (reporting only) | new |
| 2800 | 1152 | **keypoint sketch**, 32 × 36 | new |
| | **3952** | | |

### 6.2 Header and section table

64 bytes:

| Bytes | Field |
|---|---|
| 0–3 | magic `"PAPH"` |
| 4 | version = 3 |
| 5 | tier = 1 |
| 6–7 | flags, u16 LE |
| 8–9 | original width, u16 LE |
| 10–11 | original height, u16 LE |
| 12 | upscale factor |
| 13 | normalisation divisor exponent — `max(w,h)` after normalisation, log2 rounded, for sanity checks |
| 14–15 | reserved |
| 16–47 | **section table**: 8 × (u8 id, u8 flags, u16 LE length). Offsets are implied by order. |
| 48–51 | Tier 2 record count, u32 LE (0 = no Tier 2 exists) |
| 52–59 | Tier 2 content hash, first 8 bytes |
| 60–63 | CRC-32 of bytes 64..end, LE |

**Why a section table.** v2's offsets are hardcoded constants and the family has already paid once for a stale offset after a section was inserted. With a table, a v4 parser adding a section does not move an existing one, and a v3 parser skips what it does not recognise. Sections **MUST** be emitted in ascending id order.

**Why CRC-32 and not XOR.** The v2 XOR checksum cannot detect a transposition of two bytes, which is exactly the corruption a byte-range index is most likely to introduce.

### 6.3 Changes to inherited sections

**DCT — 256 B code + 8 B brightness record.** The code is unchanged and **DC is always dropped from it**. The DC information moves into an explicit 8-byte record: mean luminance quantile, luminance quantile at four fixed percentiles, and the L0 DC sign pattern digest.

> This resolves the `keepDC` conflict (SPEC-002 §6.3) instead of choosing a side. The index key never sees brightness — so the Phase-1 bucket cannot degenerate into "is this picture bright", which is what made a sibling index compare 98.9% of a corpus. The compare-time channel can read brightness explicitly when it wants to, and can report *"structurally identical, tonally shifted"* as a distinct finding, which is moderation-relevant and currently invisible.

**RAG — 48 entries × 6 B.** Each entry stores **both** endpoint encodings: `[quantileLo, quantileHi, rankLo, rankHi, count u16 LE]`.

> This resolves the `ragEndpoint` conflict the same way. Quantile survives a rebuilt palette; rank does not, and that is exactly why rank agreement is *stronger* evidence when it occurs — it means the palette was not rebuilt. Store both, score on quantile, report rank agreement as a separate corroborating signal. The screenshot operating point chose `rank`; under v3 that choice does not have to be made at hash time at all.

**Shapes — 8 records instead of 5.** Record format unchanged (41 B). Five was a budget constraint, not a finding.

**Local fingerprints — 128 instead of 64, positions 4 B instead of 2 B.**

> Real matches land at 5–11 collisions of 64 with chance exactly zero — the signal is real but reads as a small fraction of capacity. Doubling the bag doubles the expected intersection for a true pair in absolute terms; burst weighting already handles the corresponding rise in generic collisions. **This is a hypothesis, not a measurement** (§14.3). `localCount` stays a knob.
>
> Positions widen to two u16 in the aspect-true frame (§5.6), which also removes the v2 encode/decode dimension mismatch (SPEC-002 §3.7).

**Palette — 24 entries.** Unchanged format.

### 6.4 New: silhouette signature — 96 B

Computed from the **alpha mask alone**, after the matte fold. Abstains if the work has no transparency.

| Bytes | Field |
|---|---|
| 0–31 | 32-ray radial signature of the largest opaque component, normalised — same construction as §3.4 shapes |
| 32–39 | integer scale-normalised second and third moments |
| 40–47 | opaque area as a fraction of the bounding box; bounding-box aspect |
| 48–55 | hole count, Euler characteristic, boundary run-length digest |
| 56–63 | 8×8 occupancy code, D4-canonical (same `canonical64` as §3.6.3) |
| 64–95 | reserved / second component |

> **Why this is its own channel.** Two sprites cut from the same sheet share a silhouette exactly. A sprite composited into a scene has no silhouette at all. So silhouette agreement is very strong evidence when measurable and carries **zero** information when not — which is precisely the shape of a channel that must be able to abstain, and precisely why letting it leak into the interior descriptors (as L2 does today) is wrong.

### 6.5 New: absolute colour digest — 80 B, reporting only

16 entries × 5 B: quantised RGB (3 B) plus the population share and the luminance quantile of the corresponding identity-palette entry.

**This section MUST NOT contribute to any score.** Recolour invariance is a load-bearing property of the whole design and it dies the moment absolute RGB enters the scoring path. The digest exists so a verdict can *report* whether the palettes are identical, related or unrelated — an exact-palette match is a materially stronger moderation case than a recoloured one, and today that distinction is thrown away at ingest and unrecoverable.

Conformance: an implementation **MUST** be able to produce a full verdict with this section zeroed, and the verdict **MUST** be identical.

### 6.6 New: keypoint sketch — 32 × 36 B

The 32 strongest keypoints, in full: 32 B descriptor + 2 B x + 2 B y (aspect-true). Scale and orientation are omitted here; Tier 2 has them.

**Purpose: retrieval, and a Tier-1-only weak geometric check.** Without this, Tier 1 can retrieve candidates only through the local-fingerprint bag — and the pairs the geometric stage is *for* (a figure lifted out of a complex scene) are exactly the ones whose fingerprint bags barely intersect. A retrieval index built on Tier 1 alone would never surface the pairs Tier 2 is best at, which would waste the entire proposal.

**Compression path.** If Tier 1 must fit a tighter chain limit, this section is the one to cut first: drop to 16 keypoints (576 B) or move it to Tier 2 entirely and accept that retrieval requires a database read. Both degrade retrieval, neither degrades any verdict.

---

## 7. Tier 2 — Forensic wire

Header 32 B (magic, version, tier=2, record count, Tier 1 checksum back-reference, CRC-32), then `count` fixed 40-byte keypoint records, `count ≤ 256`.

| Bytes | Field |
|---|---|
| 0–31 | BRIEF-256 descriptor, 8 × u32 LE |
| 32–33 | x, u16 LE, aspect-true (§5.6) |
| 34–35 | y, u16 LE, aspect-true |
| 36 | pyramid level index (0–15) |
| 37 | orientation, 6-bit sector in bits 0–5; bit 6 = interior/border; bit 7 = reserved |
| 38–39 | FAST strength, u16 LE, **scale-normalised** |

Maximum size: 32 + 256 × 40 = **10 272 B**.

> **Scale-normalised strength** fixes a v2 defect: L2 pooled keypoints across pyramid levels and ranked them by raw FAST score, which compares scores computed at different resolutions (SPEC-002 §11.4). Normalise by the level's linear scale factor before ranking.

Records **MUST** be sorted by `(descriptor hi u32, descriptor lo u32, x, y)` ascending — a total order derived from content, so two identical keypoint sets serialise identically and comparison is order-independent, exactly as the local fingerprint section already does.

---

## 8. The integer keypoint pipeline

This is the substance of the proposal: L2's behaviour with none of L2's floats. Every step below is integer or fixed-point.

### 8.1 Pyramid

Levels indexed by `max(w, h)`, not by width — L2's width-indexed pyramid gives a 736×352 work seven scales and a 119×193 work one, an asymmetry that depends on aspect ratio rather than size (SPEC-002 §11.2).

```
L_k = round(maxDim / 1.3^k),  k = 0 .. while L_k ≥ 96
```

Downsampling **MUST** use the existing in-house integer area-average (the `smart-downscaler` construction), not a float box filter and not `sharp`.

### 8.2 FAST-9

Unchanged from L2 and already integer: 16-point Bresenham circle of radius 3, threshold 18, 4-point quick reject requiring ≥3 on one side, then a contiguous arc of ≥9. Score = `Σ|v_k − p|` over the circle.

**Border margin.** A keypoint **MUST** be at least `PATCH_R + 3 = 18` px from every edge at its own level, so that every box average in §8.4 and §8.5 is a full-size window. Clamped windows at the border make sums non-comparable and are a hidden source of non-determinism.

**Opacity floor.** A keypoint **MUST** be rejected unless its 31×31 neighbourhood is ≥ 75% opaque — the same rule the local-fingerprint windows use (§5.5).

### 8.3 Non-maximum suppression — true radius, deterministic order

Replace L2's grid-approximate NMS, which is not translation-equivariant (a 2 px shift changes which of two nearby corners survives — a real weakness for an algorithm whose premise is finding the *same* points twice).

Sort candidates by `(strength desc, y asc, x asc)` — a total order. Walk in that order, accept a candidate if no already-accepted keypoint lies within `NMS_R = 4` px (true Euclidean, integer squared distance). Cap at 256 per level.

### 8.4 Orientation — integer sector, no `atan2`

Intensity centroid over a disc of radius 7, sampling 3×3 **sums** rather than means (the divisor is constant, so it cannot affect the angle):

```
m10 = Σ dx · S3(x+dx, y+dy)
m01 = Σ dy · S3(x+dx, y+dy)        over dx² + dy² ≤ 49
```

Then resolve the 64-sector index by integer comparison, never by `atan2`:

1. Octant from `sign(m10)`, `sign(m01)`, and `|m01| ≤ |m10|` — 3 comparisons.
2. Within the octant, binary-search 7 precomputed integer tangent thresholds: sector `k` iff `|m01| · TAN_DEN[k] < |m10| · TAN_NUM[k]` — 3 comparisons.

Eleven integer comparisons, exact, engine-independent. `Math.atan2` is only *implementation-approximated* by ECMAScript, so two conforming engines can disagree in the last ulp — and a keypoint sitting on a sector boundary then gets a different descriptor. This removes that failure mode entirely.

### 8.5 Steered BRIEF-256 — integer

The pattern is rotated at runtime from Q10 integer cosine/sine tables (`(P·cos - P·sin + 512) >> 10`), not from a precomputed 64-rotation table — 256 multiply-shift pairs per keypoint costs nothing and saves 128 kB of static table in WASM.

Each bit compares two 5×5 box **sums** (`r = 2`). The box average is what makes BRIEF work on dithered pixel art at all — a raw two-pixel comparison measures the dither, not the drawing — and using sums instead of means keeps it exact.

Ties (`sumA == sumB`) **MUST** resolve to 0, specified rather than left to a `<` that happens to be there.

### 8.6 The reflection-closed pattern — mirror invariance for zero bytes

BRIEF is not reflection-invariant, which is why L2 cannot find a mirrored copy while BOF can.

**Construction.** Generate 128 test pairs freely from the seeded LCG. Bits 0–127 use those pairs. Bits 128–255 use their **x-mirrored** images: `P'_i = (−a_x, a_y, −b_x, b_y)`.

**Claim.** Under a horizontal mirror of the patch, the intensity-centroid orientation reflects (`θ → −θ`), so the steered pattern reflects with it, and the value bit `i` takes on the mirrored patch equals the value bit `i+128` takes on the original. The mirrored descriptor is therefore the stored descriptor **with its two 128-bit halves exchanged** — computable at compare time from stored bits, costing zero bytes.

This is the same move v2 already makes twice: sign/magnitude split for D4 on the DCT, complement-folding for inversion on the local fingerprints (P2).

**This claim requires verification before the pattern is frozen.** The reflection of a rotated pattern is the rotation of the reflected pattern only if the sector quantisation reflects exactly, which is true for an even number of sectors about the axis but should be proved against a synthetic mirror test, not assumed. It is the first thing in §14 for a reason.

---

## 9. Comparison

### 9.1 Contract

Unchanged from v2 §5.1. Every channel reports `raw`, `control`, `measurable`, `controlRan`, and

```
value = control ≥ SCALE ? 0 : clamp((raw − control)·SCALE / (SCALE − control), 0, SCALE)
```

A channel that cannot evaluate **MUST** abstain and **MUST NOT** return a favourable-looking number.

### 9.2 Canonical argument order

Unchanged from v2 §5.2, extended to cover the geometric channel: sort the two Tier-1 wires lexicographically, run every channel in that fixed frame, invert directional outputs afterwards (`dx,dy → −dx,−dy`; `scale → 1/scale`; `θ → −θ`; swap correspondence indices; invert the dihedral index). The geometric channel is the most directional thing in this design and must be inside the same discipline, not beside it.

### 9.3 Structural channels

The six v2 channels, inherited, with these changes:

| Channel | Change |
|---|---|
| `dct` | DC always excluded from the code; a new `brightness` sub-reading uses the explicit record (§6.3) and is reported, not summed |
| `topology` | scored on quantile endpoints; rank agreement reported as a separate boolean corroborator |
| `local` | bag of 128; burst weighting, injective greedy match, rotation null, **both** evidence rules computed and both reported |
| `shape`, `runs`, `palette` | unchanged |
| `silhouette` | **new**, §6.4; abstains when either side has no transparency |

`evidence` and `scoring` stay knobs, but under P1 they are now *pure* compare-time knobs — re-running the whole corpus under a different setting costs milliseconds per pair and no re-hashing.

### 9.4 Geometric channel

#### Step 1 — correspondences

Brute-force Hamming over the Tier-2 descriptors, or the Tier-1 sketch when Tier 2 is unavailable.

Accept `i ↔ j` iff **all** hold, with the ratio test applied from **both** sides (L2 applies it from one, which is why `match(A,B) ≠ match(B,A)`):

```
d₁(i)  ≤ HAM_MAX
d₁(i)  <  LOWE · d₂(i)          // A-side ratio
d₁(j)  <  LOWE · d₂(j)          // B-side ratio
argmin_B(i) = j  and  argmin_A(j) = i   // mutual NN
```

Also compute the correspondence set for the **mirrored** descriptors of B (§8.6, halves exchanged). Both hypotheses proceed; the one with more geometric inliers wins and the winner is reported.

Cost at full caps: `2 × 256 × 256 × 8` popcounts ≈ 1 M ops per hypothesis. A prefilter is available if needed but is not required to hit §12's budget.

#### Step 2 — Hough vote, replacing RANSAC

Every correspondence carries its own scale and orientation, which RANSAC throws away. Use them.

Each correspondence votes for one cell of a quantised similarity space:

| Parameter | Derivation | Bins |
|---|---|---|
| Δ scale | `level_B − level_A`, in pyramid steps | 25 (half-step) |
| Δ orientation | `(sector_B − sector_A) mod 64`, coarsened | 16 |
| Δ x | `x_B − r·(rotated x_A)`, quantised to 1/32 of the frame | 64 |
| Δ y | as above | 64 |

Vote into the 2×2×2×2 = 16 adjacent cells (Lowe's soft binning) so a correspondence sitting on a bin boundary is not lost. Sparse hash map; ~200 correspondences × 16 = 3 200 increments.

**This is O(n), not O(iterations × n).** L2 runs 900 RANSAC hypotheses; the vote runs once. It is also deterministic by construction — no sampling, no seed, no `Math.random` — which RANSAC can only approximate with a seeded PRNG that still depends on iteration count.

#### Step 3 — fixed-point least-squares refit

Take the correspondences in the winning cell (and its immediate neighbours). Fit the 4-DOF similarity in closed form — this is a linear solve, not an optimisation:

```
reduce coordinates to 12 bits (u16 >> 4)     // keeps every product inside 2^48
centroids ā, b̄  (integer means)
a'ₖ = aₖ − ā,  b'ₖ = bₖ − b̄
Sx = Σ (a'ₖₓ·b'ₖₓ + a'ₖᵧ·b'ₖᵧ)
Sy = Σ (a'ₖₓ·b'ₖᵧ − a'ₖᵧ·b'ₖₓ)
σ  = Σ (a'ₖₓ² + a'ₖᵧ²)
Rs = [[Sx, −Sy], [Sy, Sx]] · 2¹⁶ / σ          // Q16, one integer division
t  = b̄ − Rs·ā
```

Bounds check: with 12-bit coordinates and `n ≤ 256`, `σ ≤ 2³²`, `Sx·2¹⁶ ≤ 2⁴⁸` — inside the 2⁵³ a double holds exactly, and inside `i64` in Rust. Fully deterministic.

Then count inliers with an integer squared-residual test at tolerance `EPS`, and iterate the fit **once** on the inlier set.

**`EPS` can now be tight.** L2 needed 3.8% of the frame partly because per-axis normalisation forced the model to absorb an aspect-ratio mismatch. With aspect-true coordinates (§5.6) that error term is gone. Proposed starting point **1.5%**, to be swept (§14.2).

#### Step 4 — the null

The geometric channel reports a bare inlier count in L2 and therefore cannot be fused with anything (SPEC-002 §22, conflict 5). v3 gives it a null in the same currency as every other channel.

**Construction.** Keep the correspondence set exactly as matched, but replace each B-side keypoint's *geometry* — position, level, sector — with that of a different B keypoint under a fixed permutation (rotate the index by `⌊n/2⌋`, then by `⌊n/3⌋`). Re-run Steps 2–3. Take the **maximum** winning-inlier count over the permutations as the control.

This preserves the descriptor-match statistics — how many matches there were, how distinctive — while destroying only the geometry. It measures exactly the right thing: *would these matches have agreed on one placement by chance?*

```
raw     = clamp(inliers · SCALE / confidenceAt_geo, 0, SCALE) · purity / SCALE
control = same formula on the permuted run
value   = chanceCorrect(raw, control)
```

`measurable` iff there were at least `MIN_CORR` accepted correspondences to vote with.

### 9.5 The verdict lattice

**Do not average the two scores.** They measure different things, they have different failure modes, and both were independently measured at precision 1.00 on the corpus. A weighted sum destroys that.

| | **geometric strong** | **geometric weak** | **geometric abstains** |
|---|---|---|---|
| **structural strong** | `COPY — certified` (both) | `COPY — recolour / mirror / inversion class` | `COPY — structural only` |
| **structural moderate** | `COPY — crop / collage class` | `SUSPECTED` | `SUSPECTED` |
| **structural weak** | `COPY — collage class` | `UNRELATED` | `UNRELATED` |
| **both weak** | — | `UNRELATED` | `UNRELATED` |

Single-channel certification is justified by measurement — BOF alone gave 0/125 false positives at its threshold, L2 alone gave 9–280 inliers for true pairs against a strongest impostor of 5 — but on a corpus of 125 impostor pairs, which is small. Therefore:

> **A single-channel certification MUST require a strictly higher threshold than a two-channel one.** Proposed: two-channel `CERTIFY` at each channel's own threshold; single-channel `CERTIFY` at 1.5× that channel's threshold, or at an inlier count of ≥ 15 for geometry alone.

The verdict **MUST** carry its class label. "Copy" is what a moderator sees; *which kind* of copy is what they act on, and the recolour case, the mirror case and the collage case need different evidence in front of a human.

### 9.6 Report structure

Every verdict carries, without extra bytes:

- the class label from §9.5
- which dihedral symmetry the DCT channel elected, and whether the mirror hypothesis won
- recovered scale, rotation and translation from the geometric fit
- inlier count against its null
- palette relationship from §6.5 — *identical / related / rebuilt*
- brightness relationship from §6.3
- rank-vs-quantile RAG agreement — *same export* vs *recoloured*
- every abstaining channel, with its reason

---

## 10. Determinism and conformance

A conforming implementation **MUST** produce byte-identical Tier 1 and Tier 2 wires for identical input bytes on any architecture, and identical verdicts for identical wire pairs.

1. Integer / fixed-point only, everywhere, on both halves. All intermediates < 2⁵³.
2. No `atan2`, `hypot`, `sqrt` on floats, `Math.random`, `Date`, or locale. Integer square root where needed.
3. Every sort **total**, falling through to a content-derived tie-break — never insertion or loop order.
4. The **decoder is specified**, not delegated. v2 needed two different thresholds (7 vs 8) purely because Canvas and `sharp` decode differently; that is a bug, not a calibration.
5. All tables built from a series at load, not pasted.
6. `compare(x,y) ≡ compare(y,x)` by construction (§9.2).
7. Ties in every comparison specified explicitly.

**If all seven hold, geometric copy detection becomes consensus-verifiable for the first time in this project.** That is the headline claim of this proposal and the thing most worth testing early — §14.1.

---

## 11. Retrieval index — open

The proposal is incomplete here on purpose, because the right answer depends on the corpus size and query rate, which I do not have.

Two candidate indices, not exclusive:

**A — fingerprint postings.** Insert each work under all 128 of its local-fingerprint codes. Query by looking up the query's 128 codes and collecting works appearing ≥ t times. Direct, no banding, and it is exactly what the MinHash selection rule was designed for. **Weakness:** the pairs the geometric stage exists for are the ones whose fingerprint bags barely intersect — so this index cannot surface them.

**B — keypoint-sketch LSH.** From each of the 32 sketch descriptors, take `b` fixed bit-subsets of 24 bits as bucket keys. 32 × b postings per work. Two works sharing even a handful of keypoints collide in some band. **Weakness:** untested here; bit-subset selection needs to be chosen against the corpus, not arbitrarily.

**Recommendation:** implement both, union the candidate sets, and measure retrieval recall separately from verdict recall. They are different numbers and conflating them is how an index quietly caps a detector's performance.

**Phase 1 must actually gate.** In v2 it is reported but never enforced (SPEC-002 §5.5), so the O(1) claim was never earned. In v3 it either gates or it is deleted from the spec.

---

## 12. Performance budget

Targets, not measurements.

| | Target | Basis |
|---|---|---|
| Tier 1 hash | ≤ 700 ms | v2 BOF is ~440 ms at 64 fingerprints; 128 roughly doubles the fingerprint stage |
| Tier 2 hash | ≤ 900 ms | FAST + describe over ~7 levels |
| Structural compare | ≤ 5 ms | v2 is ~2 ms |
| Geometric compare | ≤ 15 ms | ~1 M popcounts × 2 hypotheses + an O(n) vote |
| Full pair verdict | **≤ 25 ms** | |

`peakRadius` remains the sharpest time/quality trade: 9 vs 5 halves hashing time on the largest work for about a tenth of the F1.

---

## 13. Migration

v2 wires **cannot** be upgraded in place — the DCT drops DC unconditionally, positions change frame, sections change size. The corpus must be re-hashed.

At ~1.6 s per work that is under an hour for the first hundred thousand works, single-threaded, and it is embarrassingly parallel. This is not a constraint on the design; it is a one-time cost, and P1 exists so it is the **last** time it is paid for a calibration reason.

Version 2 wires **MUST** be rejected by a v3 parser rather than reinterpreted. Mixed-version comparison **MUST NOT** be attempted.

---

## 14. What to measure, in order

Nothing above is measured. The order below is chosen so that a failure kills the cheapest thing first.

1. **The reflection-closed BRIEF claim (§8.6).** Synthetic: mirror 200 patches, check that the descriptor of the mirror equals the stored descriptor with halves exchanged, bit for bit. **A day. If it fails, mirror invariance costs 32 B per keypoint and the budget changes.**
2. **The aspect-true coordinate fix (§5.6) on v2's L2, in isolation.** One line. Prediction: the two missed `RoWLrDezuDf` pairs are aspect-mismatched and recover. Also sweep `EPS` downward — if it can tighten from 3.8% toward 1.5%, that is free precision headroom. **This is the highest expected value per hour of work in the entire proposal.**
3. **Bag size 64 → 128 (§6.3), on v2's BOF, in isolation.** Does the intersection grow faster than the generic-collision floor? Burst weighting says it should. Measure, do not assume.
4. **Integer orientation and integer refit (§8.4, §9.3), against the float versions.** How many keypoints change sector? How many verdicts move? If the answer is "none on the corpus", §10's headline claim is cheap to keep.
5. **Alpha unification (§5.5) and the silhouette channel (§6.4).** L2's 0.82 was measured with silhouette signal available; re-measure with it isolated. Expect recall to move between channels, and watch the total.
6. **The Hough vote against RANSAC** on the same correspondences. Same inliers? Faster? If the vote is worse, the fallback is a *deterministic* RANSAC (canonical hypothesis enumeration instead of sampling), which is more expensive but keeps §10.
7. **The verdict lattice thresholds (§9.5)**, on the first hundred real moderation reports — not before, and not on the 16-work corpus.
8. **Retrieval recall (§11)**, separately from verdict recall.

---

## 15. Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| The reflection-closure claim is wrong | Medium | §14.1 is a day's work and kills it early; fallback costs 32 B/keypoint |
| 128 fingerprints raise the generic-collision floor faster than the signal | Medium | `localCount` stays a knob; §14.3 measures before committing |
| Tier 1 does not fit the chain's operation ceiling | Unknown — **confirm before freezing the layout** | §6.6 compression path; worst case Tier 1 goes off-chain and only its hash is anchored |
| Single-channel certification (§9.5) raises false positives beyond what 125 impostor pairs can reveal | **High — this is the real one** | Higher single-channel thresholds; class labels so a moderator sees which evidence certified; §14.7 on real reports |
| Ground truth is still inferred, so every threshold rests on 11 pairs one of which was already proved mislabelled by a measurement | **High** | Confirming the corpus labels is a human task and remains the single highest-value input available to this project |
| Integer geometric pipeline is slower than the float one by more than the budget allows | Low | §12 has slack; the Hough vote is asymptotically cheaper than 900 RANSAC iterations |

---

## Appendix — Budget summary

| | v2 | v3 proposed |
|---|---|---|
| Core wire | 1 357 B | **3 952 B** |
| Forensic wire | — | ≤ **10 272 B** |
| Total per work | 1 357 B | ≤ **14 224 B** |
| Local fingerprints | 64 | 128 |
| Shapes | 5 | 8 |
| RAG entries | 32 (one endpoint encoding) | 48 (both encodings) |
| Palette entries | 16 | 24 |
| Keypoints stored | 0 | 32 (Tier 1) + 256 (Tier 2) |
| Consensus-safe geometry | no | **yes, if §10 holds** |
| Re-calibration without re-hashing | no | **yes** |

*End of PAPH-SPEC-003 (proposal).*
