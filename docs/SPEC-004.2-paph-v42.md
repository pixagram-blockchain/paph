# SPEC-004.2 — comparator 42, the 512-keypoint budget

Status: **IMPLEMENTED** in both engines, calibration **PROPOSED**. From `@pixagram/paph-x` 1.2 the
shipped calibration is **CAL-007-PROVISIONAL** — CAL-004 with one bar moved (§19).

The wire does not move. Tier 1 is still exactly 3952 bytes, Tier 2 is
still `32 + 40n`, and a 4.1 wire still parses, still compares and still
means what it meant. What moves is the Tier-2 *budget* — 256 keypoints
to 512 — and, because 512 points are only worth having if they are 512
different points, the machinery that selects them, matches them, votes
with them and decides how much independent evidence they amount to.

Comparator identity: the PCAL `comparator` field takes the value **42**,
in container **3**. Comparator 41 stays frozen and callable
(`compare41` / `cal41`); comparator 4 stays decodable but not
computable, exactly as 4.1 left it. Each comparator refuses the others'
profiles, in every direction.

The claim this revision makes is deliberately narrow:

> A larger budget changes how well a pair is **measured**. It does not
> change what a copy **is**.

Every decision constant in CAL-004 is CAL-003's, unchanged. §16 records
what that cost, and what was measured rather than assumed.

---

## §1 — Why 256 was a bottleneck, and why 512 rather than 384

`KP_PER_LEVEL = 256` was never a format constraint. The Tier-2 record
count is a `u16` and the records are a fixed 40 bytes, so the ceiling was
a constant in three files and nothing else. Tier 2 at the new budget is
at most

```
  32 byte header + 512 × 40 byte records = 20512 bytes
```

384 was considered and rejected. It raises recall while still leaving
larger works under-sampled, and it buys that partial improvement at the
same architectural cost as 512 — every change in §4 through §8 is needed
at 384 too. A power-of-two budget also lets the geometry stage size its
vote table without a special case.

The descriptor stays at **256 bits**. Given a fixed byte budget,

```
  256-bit descriptor × 512 keypoints
```

carries substantially more *spatial* evidence than

```
  512-bit descriptor × 256 keypoints
```

and for collage and partial-copy detection spatial multiplicity is the
thing that pays.

---

## §2 — What a 4.2 wire says about itself

The budget change is a hash-time change: a 4.2 fingerprint of an image
is not a 4.1 fingerprint of the same image, because the Tier-1 sketch is
drawn from a different keypoint set. Two reserved bits carry the
provenance, so no version bump is needed and no wire has to be
re-hashed to stay readable:

| where | field | 4.1 | 4.2 |
| --- | --- | ---: | ---: |
| Tier-1 header, flags | `F_KPQ` (0x20) | clear | set |
| Tier-2 header, byte 16 | `select` | 0 | 1 |

Both bytes sit outside the CRC-covered region, so the checksums of
existing wires are unaffected, and both read back as `0` — "4.1
selection" — on every wire ever emitted before this revision.

A comparison between a 4.1 side and a 4.2 side is **well defined and is
performed**. It is also flagged: `report.selection.mixed`. This is a
warning, not a refusal. A 4.1 side brings 256 strength-ranked keypoints
to a 4.2 side's 512 quality-ranked ones, and recall measured across that
boundary is not recall measured within either. **Re-hash a corpus before
re-deriving its thresholds.**

To reproduce a 4.1 wire exactly, hash with `{ kpCount: 256, kpSelect: 0 }`.
The 4.1 selector is kept in both engines for that purpose, not
approximated.

---

## §3 — Selection: 512 observations are not 512 pieces of evidence

This is the change that decides whether the rest is worth anything.

PAPH 4.1 spread the budget round-robin over an 8×8 spatial grid, which
fixes one way a keypoint budget lies — a busy host out-scoring a pasted
figure and crowding every one of its keypoints out — and does nothing
about the other. A brick wall yields two hundred keypoints describing one
local texture. Spreading them out spatially does not make them two
hundred observations:

```
                    512 observations
                            ↓
                perhaps 40 independent structures
```

4.2 therefore selects on a quality score with four terms, all integer,
all in the 0..10000 currency:

```
  Q = 40 · strength
    + 25 · spatial novelty
    + 20 · scale novelty
    + 15 · descriptor novelty
```

- **strength** — `s · 10000 / max(s)`, the level-median-normalised FAST
  score the pipeline already computes.
- **spatial novelty** — `10000 / (1 + n)` in the number already selected
  from that 8×8 cell. The first keypoint in an empty region is worth
  eight of the eighth in a crowded one.
- **scale novelty** — the same decay in the number already selected from
  that pyramid level.
- **descriptor novelty** — `min(d, 16) · 10000 / 16`, where `d` is the
  Hamming distance to the nearest already-selected descriptor. A
  near-duplicate of something already held scores zero on this term
  however strong it is.

Selection is greedy over `want` rounds, ties going to the lower index —
that is, to the stronger candidate under the pooled total order, so the
rule degrades to 4.1's ranking when the novelty terms are flat. The
candidate pool is the strongest `want × 4` under that order, which bounds
the selector at `want × pool` operations without ever discarding a
candidate that could have won on strength alone.

Golden vectors `select_quality` pin the behaviour on a fixture built for
it: forty candidates, half sharing one descriptor exactly. At `want = 8`
the selector takes the eight distinct ones and no others.

---

## §4 — Matching: exact early abort, and a margin beside the ratio

512 × 512 is four times 256 × 256, so the matcher stops computing
distances it has already lost.

```
  limit = max(a_d2[i], b_d2[j])
  d     = ham_cut(A[i], B[j], limit)
```

`ham_cut` accumulates the popcount a word at a time and returns as soon
as the partial sum passes `limit`. This is **exact, not approximate**: a
partial sum already past `limit` belongs to a pair that cannot become
either side's first or second candidate, and every downstream test is a
strict `<` against a value at most `limit`. Recall is untouched.

The Rust engine repacks the stored `[u32; 8]` into `[u64; 4]` once,
while Tier 2 is parsed, so the hot loop issues four POPCNTs rather than
eight. The wire representation is unchanged at 32 bytes. The JavaScript
engine keeps `[u32; 8]`, since it has no native 64-bit integer; the two
engines' *results* are identical, only their speed differs.

> **The abort is exact, and the Rust matcher does not use it.** Because
> `ham_cut` cannot change a distance, *which* abort strategy an engine
> uses is an implementation choice rather than a normative one — and
> measured, aborting is slower. A branch per word in a loop this
> unpredictable costs more than the three saved POPCNTs save: dropping
> it took a full comparison from 13.2 ms to 8.0 ms with every output
> byte unchanged. `ham_cut` remains specified and golden-pinned, and
> remains the right shape for an engine whose popcount is expensive
> relative to its branches. This is the second time in this revision
> that a mechanically sound argument lost to a measurement; see §16.

The acceptance test gains an **absolute margin** beside the Lowe ratio.
Consider

```
  d1 = 2,  d2 = 3     ratio 0.667 — passes easily, means almost nothing
  d1 = 20, d2 = 40    ratio 0.500 — a real separation
```

so 4.2 requires both

```
  d1 · lowe_den < lowe_num · d2        (ratio, 0.82)
  d2 − d1 ≥ lowe_margin                (absolute, 6)
```

The margin is applied on **both** sides, exactly as the ratio is, or
`compare(A,B)` and `compare(B,A)` would stop agreeing. `lowe_margin = 0`
recovers the 4.1 correspondence set exactly, pair for pair and in order;
a test asserts it.

---

## §5 — The vote: bounded space, fixed table, soft scale

4.1 built a `HashMap<i64, i64>` per verification and a `Vec` of cell keys
per correspondence. That was affordable at 256 correspondences and is not
at 512.

The Hough space is bounded — 25 scales × 16 rotations × 64 × 64
translations = 1638400 cells — but a pair only ever touches a few
thousand of them, so neither a dense array nor a hash map with an
allocator behind it is the right shape. 4.2 uses:

- **`VoteCells`** — a stack structure, `[i32; 24]` keys plus their kernel
  weights and a length. No allocation per correspondence.
- **`VoteTable`** — fixed open addressing, capacity
  `next_pow2(max(64, n · 48))`, one multiplicative hash
  (`k · 2654435761`, high bits — `Math.imul` on the JavaScript side),
  linear probing, four parallel arrays for key, mass, member count and
  confidence.

Scale stops being a hard bucket. The scale estimate comes from the level
*dimensions* and is therefore quantised; at 256 keypoints there was not
enough evidence to spend on hedging that quantisation and at 512 there
is. The kernel over 64 is

```
      s−1     s     s+1
       24     64     24
```

Rotation and both translation axes keep Lowe's nearest-two duplication
exactly as before. Votes are **duplicated** across soft bins, never
split. A correspondence votes at most 3 × 2 × 2 × 2 = 24 cells.

---

## §6 — The peak and the model

Two changes, both aimed at the same confusion, which 512 keypoints makes
more likely rather than less: a handful of overwhelming correspondences
looking like a populated transformation.

**Vote weight** is now two factors:

```
  weight = lowe_confidence × strength_compatibility
  strength_compatibility = clamp(min(sA,sB)·64 / max(sA,sB), 32, 64)
```

A weak accidental keypoint can pass Lowe; a strong keypoint that also
agrees geometrically is worth more. The floor at ½ keeps the term from
erasing a legitimate coarse-level match merely because the two sides
normalised their FAST scores against different level medians.

**Peak selection** is lexicographic on

```
  (mass, member count, summed confidence, −key)
```

rather than on mass alone. The scan walks slots, but the reduction is by
an explicit total order, so the answer cannot depend on where a key
landed in the table. A test asserts that inserting the same votes in a
different order gives the same peak.

**Model quality.** Each verified model now reports a **median squared
residual** over its inliers, alongside its inlier count and vote mass.
The median is used rather than a sum of squares precisely because it is
robust to a few bad matches. The direct/mirror winner per round is chosen
on `(inliers, −median_err, conf_sum)`, ties still going to the direct
hypothesis as in 4.1.

The verification sequence itself is unchanged and was already right:
Hough → gather the winning cell's members → closed-form fit → one loose
pass → refit → one tight count. No RANSAC, no seed, no iteration count.

---

## §7 — Consumption and the exclusion neighbourhood

Multi-model extraction is unchanged in architecture — it is the right
shape for collage detection and 512 keypoints make it more useful, not
less. One refinement:

A descriptor covers a 31 px patch **at its own level**, so the footprint
of the structure a keypoint stands for is `PATCH_R / L_k` in normalised
units. After a model is accepted, its inliers' B-side keypoints are
consumed, and so is any pool entry whose B keypoint falls within

```
  R = 65535 · 15 · excl_pct / (level_dim(mdB, level) · 100)
```

of a consumed one. A second model reaching into that footprint is not
finding a second paste; it is finding the first one again, one keypoint
over.

This prunes the **pools** for later rounds only. The underlying
correspondence lists are untouched, so the measurement and its control
still start from identical evidence. `excl_pct = 0` restores the 4.1
consumption rule exactly.

---

## §8 — Diversity, as a first-class channel

This is the change 512 keypoints make necessary rather than merely nice.

Doubling the budget doubles the matches a repeated texture can produce
without adding one independent observation, and the inlier count cannot
tell those two situations apart. 512 matches concentrated in one corner
of one artwork at one scale under one model are not 512 pieces of
evidence.

Four readings, all integer, all 0..10000, weighted as §3 weights its four
terms:

```
  D = 40 · spatial + 25 · scale + 20 · model + 15 · descriptor
```

- **spatial** — the existing `coverage()` of the accepted models' inlier
  B-positions on the profile's g×g grid.
- **scale** — pyramid levels carrying inliers, over the levels B actually
  has.
- **model** — accepted models over `max_models`.
- **descriptor** — independent descriptor clusters among the inliers'
  A-side descriptors, greedy, a cluster head being further than Hamming
  16 from every head already found, over the inlier count.

`D` passes through its own calibration table and multiplies the
geometric evidence:

```
  geometry_evidence = LUT_geo(chance_margin) · LUT_geodiv(D) / SCALE
```

`LUT_geodiv` is a **tenth** table in container 3, deliberately *not*
`lut_diversity`. That one already modulates the local channel by its own
repetition reading (SPEC-004 Appendix C), and reusing it would have moved
the structural axis silently while claiming to change only geometry.

**The multiplier applies only when at least one model was accepted.**
Where the extraction accepts nothing, the §A1 weak signal reaches the
margin through a path that never produced a model, and multiplying it
down here would punish the same absence twice.

---

## §9 — What did not change

Stated explicitly, because the temptation to change it was real:

- **The lattice.** Comparator 41's, rule for rule — R1 corroboration, R2
  repetition, R3 scatter, R4 support, the six states, the two axes never
  averaged.
- **The null family.** The same five geometric permutations
  (`1/2, 1/3, 1/5, 2/3, reverse`), each running the *identical*
  measurement including the weak-signal fallback, aggregated by **MAX**.
  The question is unchanged and is the right one: how much better is the
  real correspondence set than the best structured accident? Averaging
  the nulls would ask a weaker question.
- **Proportion and Gate.** Still out of the decision path, where 4.1 put
  them. Container 3 continues to bind `evidence_rule = Lift` and
  `scoring_rule = Weighted` and to refuse anything else. This matters
  more at 512 than at 256, since the proportion denominator changes
  meaning with the budget.
- **A1, A2, A3, A4.** Weak-signal geometry, the sparse local assignment,
  per-channel calibration tables and the stage-1 screen all carry over
  unchanged.
- **Tier 1.** All eleven sections, all 3952 bytes, all offsets.

---

## §10 — Container 3

Container 3 is container 2 plus eight `i32` appended after the fixed
block, plus a tenth LUT. A container-1 or container-2 artefact keeps the
byte layout it shipped with, so **CAL-001 and CAL-003 encode to the same
bytes and hash to the same ids as before** — a test asserts both.

| field | CAL-004 | meaning |
| --- | ---: | --- |
| `lowe_num` / `lowe_den` | 82 / 100 | the ratio, unchanged |
| `lowe_margin` | 6 | §4 absolute margin; 0 disables |
| `ham_max` | 88 | distance ceiling, unchanged |
| `min_peak_members` | 3 | §6 correspondences sharing the winning cell before a fit is attempted |
| `excl_pct` | 100 | §7 exclusion, as a percentage of the patch footprint; 0 disables |
| `kp_select` | 1 | the selection rule the calibration was derived on |
| `scale_soft` | 1 | §5 soft scale binning; 0 restores the 4.1 hard bucket |

Tenth LUT: `lut_geo_diversity`, neutral (constant SCALE) when absent.

Artefact: `docs/calibration/CAL-004-PROPOSED.pcal`, 320 bytes,
`calibration_profile_id`
`91b545f801f5a0959475270996b190b5b395f98cb22a2d731eaa065a3c03d078`.

---

## §11 — Report surface

Everything comparator 41 reported, plus:

```jsonc
{
  "diversity": { "spatial": …, "scale": …, "model": …,
                 "descriptor": …, "combined": …, "multiplier": … },
  "medianErr": [ … ],                    // per accepted model
  "selection": { "a": 0|1, "b": 0|1, "mixed": bool },
  "kpA": …, "kpB": …
}
```

`to_json_v4` is untouched. A shipped surface must not move under a later
comparator.

---

## §12 — Cost

Naïvely, 512 keypoints is 4× the descriptor pairs:

```
  256 × 256 =  65536
  512 × 512 = 262144
```

Measured on the native Rust reference at 512 keypoints (512×384,
`paphcli` mode `B`), and the shape of it is not what the design note
predicted:

| stage | ms | whose code |
| --- | ---: | --- |
| hash, total | 91 (was 111) | — |
| — keypoint stage | 34 | SPEC-003 + §3 |
| — of which §3 selection | 8 | 4.2 |
| — local fingerprints | 27 (was 31) | SPEC-003, frozen output |
| — normalise | 13 (was 22) | SPEC-003, frozen output |
| — RAG + shapes + runs + the rest | 17 | SPEC-003, frozen output |
| compare, unrelated pair | 7.7 (was 13.2) | — |
| compare, mirrored copy | 8.9 (was 14.2) | — |
| compare, self | 11.5 (was 19.7) | — |
| — of which the v3 structural half | 5.0 | SPEC-003, frozen |

**Hashing is dominated by the frozen v3 structural sections, not by
anything 4.2 owns.** The §3 selector is 8 ms of 91 — worth optimising,
not worth blaming. The wire's own implementation has since been through
the same output-identical treatment (§18), which is what moved 111 to
91; what remains is dominated by the local-fingerprint window walk and
FAST-9 detection, both of which this synthetic fixture — noise-dense,
corner-rich — exercises harder than real pixel art does.

Comparison did **not** rise 4× with the pair count. Against the 4.1
shapes on the same fixture, the M9 pass (§18) took a self-compare from
19.7 ms to 11.8 ms; the honest statement remains that the multiplier
should be **measured per deployment rather than quoted**.

---

## §13 — Golden vectors

`docs/golden/GOLDEN-004.json` is emitted by the Rust reference and
asserted byte-for-byte by a test. New sections:

| section | pins |
| --- | --- |
| `profile_cal004` | container-3 encoding, length, id |
| `ham_cut` | 72 vectors: exactness below the limit, abort above it |
| `strength_compat` | 8 vectors including both saturation ends |
| `vote_cells_42` | 4 cases, hard and soft scale, keys and kernel weights |
| `select_quality` | 3 budgets on the shared-descriptor fixture |
| `geometry_42` | full extraction: pools, models, residuals, vote mass, GN control member, all four diversity components |

The JavaScript engine is held to every one of them, and the two engines
are additionally compared field-for-field through `paphcli` mode `X`.

---

## §14 — Migration

1. **Re-hash.** 4.2 fingerprints differ from 4.1 fingerprints for the
   same image. Mixed comparisons work and are flagged, but a corpus used
   for calibration should be homogeneous.
2. **Storage.** Tier 2 grows from at most 10272 bytes to at most 20512.
   Tier 1 is unchanged at 3952.
3. **Profiles.** A container-2 artefact will not compute under comparator
   42, and vice versa. Both still decode, so old verdicts keep their
   provenance.
4. **Reproducing a 4.1 verdict.** Hash with
   `{ kpCount: 256, kpSelect: 0 }` and compare with `compare41` under
   `cal41()`.

5. **Expect the shared-asset class to surface.** On the worked example
   `G1×G2` goes Suspected → Copy purely because 512 keypoints find the
   shared tile that 256 was missing — comparator 42 on 256-keypoint
   wires reproduces 4.1's Suspected exactly. If a deployment treats
   shared licensed assets as negatives, that policy needs a human
   decision and real pairs (M7), not a threshold move; §17 explains why
   §8 cannot make it for you.

---

## §15 — Sequencing, for the record

The design note this revision implements recommended landing the changes
in stages so each could be attributed. That advice was taken in
structure, not in release cadence: every §4–§8 change is independently
switchable through container-3 fields —

```
  lowe_margin = 0     → 4.1 matching
  scale_soft  = 0     → 4.1 hard scale bucket
  excl_pct    = 0     → 4.1 consumption
  lut_geo_diversity neutral → no §8 modulation
  kpSelect    = 0, kpCount = 256 → 4.1 wire
```

so an ablation is a profile edit, not a rebuild. That is the property the
staged sequence was after.

---

## §16 — What was measured rather than assumed

Two count-valued knobs looked obligatory and were not.

`geo_conf_at` (16) and `GEO_SOLO_INLIERS` (11) are absolute inlier
counts, and 512 keypoints plainly put more inliers through them. The
argument for doubling both was mechanical and specific: `chance_correct`
returns **0** when the control reaches SCALE, so a control saturating at
16 inliers would zero the geometric margin on exactly the strongest
pairs.

Measured, the mechanism does not fire. On 512-keypoint works the GN
control reaches ≈3125 of 10000 — nowhere near saturation — while
doubling `geo_conf_at` halves the reading on every work that never
reaches the budget at all, and doubling the solo floor turned a genuine
crop from **Copy** into **Suspected** at 128×128. A floor only large
works can clear is a size-dependent bias, and that is the defect class
this family has already paid for once, when v2 indexed its pyramid by
width and gave a 736×352 work seven scales and a 119×193 work one.

Both knobs therefore keep CAL-003's values. The guard against a
repetitive work manufacturing inliers at 512 is §8 diversity, which
measures the problem directly, rather than a raised saturation point,
which measures it by proxy.

**One reading of the source note is recorded as a judgement call.** Its
parameter table lists `minimum model 3`. This is read as the Hough peak
member floor — the fit's own requirement, and now the explicit profile
field `min_peak_members` — rather than as `min_model_inliers`, which
stays at 6. Lowering the *acceptance* floor from 6 to 3 is a substantive
recall/precision move that the same note argues should be calibrated
last, and nothing else in it discusses that floor. If the other reading
was intended, it is a one-field profile edit.

---

## §17 — Open, and gating

- **CAL-004 says PROPOSED.** The corpus has not been re-hashed at 512.
  §19 of SPEC-004 must re-derive the geometric side, and `lut_geometry`
  in particular, on 4.2 wires.
- **`lut_geo_diversity` is a first guess.** `[[0,6000],[2000,8000],
  [4000,10000],[10000,10000]]` — a floor at 60%, full evidence past
  4000 — chosen to be conservative, not because 4000 was measured.

- **§8 does not resolve the shared-asset case, and should not be sold as
  if it did.** On the worked example, `G1×G2` (a shared tile, labelled a
  negative) reads `D = 3583`; `A1×C1_composite` (a genuine collage,
  labelled a positive) reads `D = 3583` as well, to the digit. Diversity
  separates either of them from a near-duplicate (≈7900) and separates
  neither from the other, because both genuinely *are* one coherent
  region reused at one scale. This is ANALYSIS-002's overlap restated on
  a new axis rather than escaped by it. A table tight enough to demote
  the shared asset demotes the collage with it. The difference remains
  provenance, and provenance is not in the pixels.
- **M7 remains the gate.** Real moderation pairs, not one generator's
  ten families. 4.2 does not change that and does not claim to.
- **The §3 selector's weights** (40/25/20/15) and `DESC_NOVEL_AT` (16)
  are hash-time and therefore *not* re-derivable without re-hashing.
  They are the most expensive numbers in this revision to get wrong and
  the ones most worth an ablation before the corpus grows.

---

## §18 — Implementation notes (M9), all output-identical

None of this is normative. Every change below was landed against the
byte-for-byte golden assertion and the cross-engine parity suites, which
is the only reason any of it is allowed to exist.

**The vote table is reused, not rebuilt.** `VoteTable` grows once and is
cleared by incrementing an epoch counter; a slot whose stamp is not the
current epoch *is* empty. A single comparison runs roughly forty
verifications, and at 512 correspondences the table is 32768 slots, so
rebuilding it each time was most of the geometry stage's memory traffic
and all of its allocator traffic. This is legitimate rather than merely
convenient because `peak` reduces by an explicit total order on
(mass, members, confidence, key): the answer cannot depend on capacity,
on the probe sequence, or on which slot a key landed in.

**`peak` walks what was written.** The table keeps a list of the slots
touched since `begin()`, so the reduction visits a few thousand live
cells instead of striding 786 kB.

**One scratch per comparison.** `Scratch` owns every buffer the geometry
stage would otherwise allocate per verification — vote table, per-
correspondence cells, member and inlier lists, the inlier mask, the
residual buffer, the §7 blocked set and its per-level radii. None of
them carries information between calls, so none of them needs to be new.

**§7 consumption is linear.** Membership was a linear scan of a `Vec`
inside a `retain`, i.e. quadratic in the inlier count, with
`excl_radius` — which re-walks `level_dim` — called inside it. It is now
a byte per B keypoint marked once, against radii computed once per pair.

**`scale_bin` stops early.** `SCALE_Q16` is strictly increasing, so
`|q16 − SCALE_Q16[d]|` falls and then rises; once it has risen no later
entry can win. Same answer, and it runs about twenty-five thousand times
per compared pair.

**The §3 selector is structure-of-arrays.** Descriptors are packed into
a contiguous `[u64; 4]` block rather than strided through 40-byte
records, the two novelty denominators come from a reciprocal table
instead of an integer divide, and `SCALE_Q / DESC_NOVEL_AT` is exactly
625 so the descriptor term is a multiply.

**SIMD, and where it is not worth it.** For a single 256-bit descriptor,
scalar `u64::count_ones()` is already one instruction per word on both
targets and vectorising it is a wash. The one shape a vector unit can
use is a fixed query against a contiguous stream, which is exactly the
§3 novelty update, so that is where the SIMD128 path lives —
`i8x16.popcnt` plus two pairwise widening adds, gated on
`target_feature = "simd128"` and enabled for the shipped WebAssembly in
`rust/build.sh`. It is held to the scalar path bit for bit by
`descriptor_batch_matches_pairwise`, because an optimisation that
changes one distance changes a verdict.

**The second pass went after the wire's implementation, not its bytes.**
Section-level timing (the `paphcli` bench prints it) showed hashing was
never the keypoint budget: it was three implementation choices in
SPEC-003 code whose *output* is frozen but whose *speed* is not.
`index_image` did a `HashMap` lookup per pixel for a key that is 16 bits
by construction — three hash tables became one flat 65536-entry array,
first-occurrence order and the spill cache preserved exactly, and
`normalise` halved. `canonical64` walked eight D4 index maps over every
candidate window, sixteen variants of 64 loads and 64 threshold tests;
the eight variants are bit permutations of one mask, so the threshold now
runs once and the geometry runs on a `u64` — byte-reverse for a
horizontal flip, `swap_bytes` for a vertical one, the three-mask
exchange for the transpose — held to the map-walking reference by a
2000-case equivalence test across dense, tied, binary and constant
cells. And `nms`, whose full scan against the kept list is the one cost
§1's budget genuinely doubled, now probes a grid of cell size `r`: the
radius test forces a suppressor into the 3×3 neighbourhood, so the probe
sees every point the scan saw, in the same order, with the same strict
test — the grid narrows *where* to look for an exact test, it never *is*
the test, which is precisely the line the v2 defect crossed. The
window median became `select_nth_unstable` (the same order statistic the
full sort produced), and the transparent-or-scale transform moved out of
four nested loops to one pass per image. Net, on the reference fixture:
hash 111 → 91 ms, WebAssembly 152 → 122 ms, every suite green and the
golden file untouched.

---

## §19 — CAL-007-PROVISIONAL (`@pixagram/paph-x` 1.2)

Comparator 42 does not change; its calibration does, by one decision constant.
CAL-007-PROVISIONAL is CAL-004-PROPOSED with **`thresholds[2]`, the moderate structural bar, at
3300 instead of 2400** — the bar of the lattice's "partial agreement" arm, which reads a pair
`Suspected` on certifiable structure alone between the moderate and the strong bar (4000).
Every other field is CAL-004's — the measurement fields, the LUTs, every bar of a `Copy` arm — but
the name. The artefact is `docs/calibration/CAL-007-PROVISIONAL.pcal`, 320 bytes (its 16-byte name field reads
`CAL-007-PROVISIO`, as CAL-001's reads `CAL-001-PROVISIO`), `calibration_profile_id`
`741afad9252f2ccb4ef6f691725b740d1b100a92858b71346910aed457eee899`.

**Why.** On the artworks of the Pixa chain (`@pixagram/paph-x` 1.1.2's snapshot: 177 works by 30
authors, 15,576 pairs) comparator 42 under CAL-004 read `Suspected` on 222 of the 14,412 pairs of
two authors' works (1.5 %, on wire 4) and `Copy` on none: every one on the partial-agreement arm,
structure between 2400 and the strong bar, no geometry. A review queue fed with `Suspected` grows
with the number of pairs. The highest structural score of a certifiable pair of two authors' works
is 3232; the bar is the next multiple of 100 above it, 3300. Fitted on each of two author folds
alone, the bar is 3200 and 3300: 1 of the other fold's 1,748 cross-author pairs reaches the first,
none of its 5,482 the second.

**What it moves.** Only `Suspected → Related`, and only where structure alone made the
suspicion: no `Copy` arm reads the moderate bar, so no `Copy` moves, and no pair moves up.

| | CAL-004 | CAL-007 |
|---|---:|---:|
| the chain: pairs of two authors' works read Suspected | 222 | **0** |
| the chain: pairs of one author's works read Suspected | 37 | 0 |
| the chain: Copy (both within one author's works) | 2 | 2 |
| the chain's real bases under the twenty transforms (3,480 pairs): Copy | 3,095 | 3,095 |
| — Suspected or above | 3,397 | 3,309 |
| the synthetic corpus (2,400 transformed pairs): Copy | 995 | 995 |
| — Suspected or above | 1,857 | 1,682 |
| synthetic negatives (10,020 pairs): Suspected | 140 | 74 |
| synthetic negatives: Copy | 3 | 3 |

The copies it demotes are the weakest, whose structure sits between the two bars without a
geometric model: on the real bases 74 of the 88 are palette shuffles, on the synthetic corpus
resamples, crops, re-dithers, inversions and palette shuffles. A review queue loses them; an alert
on `Copy` never had them. `sibench calfit` refits the bar and prints these tables;
`sibench calib` writes every pair's lattice inputs, and `tools/cal-lattice.py` recomputes the
lattice on them under any candidate profile (it reproduces CAL-004's recorded verdict on every
row). The runs are `docs/calibration/CAL-007-PROVISIONAL.log`.

**Still provisional.** 177 works by 30 authors, among which comparator 42 finds two copy pairs,
are not the moderation corpus §17 asks for; the bar is the edge of what one snapshot shows
between authors, not a measured error rate, and is to be refitted as the chain grows (a fit on
another snapshot is another profile, under another name). Verdicts issued under CAL-004 stay
reproducible from `docs/calibration/CAL-004-PROPOSED.pcal`, and every report names the
calibration it was issued under.
