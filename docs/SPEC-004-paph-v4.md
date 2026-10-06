# PAPH v4 — Calibrated Multi-Evidence Comparator — Technical Specification

```text
Document:   PAPH-SPEC-004 (remastered — supersedes the draft proposal of the same name)
Status:     Implementation draft
Wire:       UNCHANGED — Tier 1 and Tier 2 exactly as SPEC-003 §6–§7, format_version 3
Comparator: version 4 (verdict-breaking changes on the same wire; see Appendix E)
Reference:  rust/src/ is normative where this document delegates to it;
            golden vectors decide every ambiguity this document fails to close.
```

The draft that preceded this document proposed the right architecture and
misdescribed the project it was proposing it for. Both are fixed here. The
architecture — *measurements become evidence only after surviving a null model,
a calibration table and a consistency check* — is kept in full. The description
of v3 is corrected in §2, because what Phase 1 actually is depends on what
already exists, and more of it exists than the draft admitted.

---

## 1. What this changes and why

v3 shipped a detector that is deterministic, two-engine, integer-only and
null-aware per channel. What it cannot yet do is defend its thresholds. The
structural constants were derived from sixteen works; the geometric constants
are a proposal; the local channel's null is aggregated the *lenient* way while
the geometric channel's is aggregated the *conservative* way; and a raw match
count still carries the same weight whether it is spread across a canvas or
piled onto one repeated tile.

v4 is the comparator release that fixes this. It changes **no wire byte**.
Every v3 hash remains valid and every v3 hash is a valid v4 input. What changes
is what the comparator does with those bytes:

```text
v3                                  v4

measurement                         measurement
    ↓                                   ↓
one null per channel                a FAMILY of structured nulls per channel
    ↓                                   ↓
analytic chance-correction          MAX-aggregated control
    ↓                                   ↓
compiled thresholds                 calibrated evidence (profile LUT)
    ↓                                   ↓
lattice                             consistency rules
                                        ↓
                                    lattice (+ Indeterminate)
```

Because the local-null aggregation flips from lenient to conservative (§9.5),
identical inputs can produce different verdicts under v4. That is a breaking
change in the only contract that matters — same bytes, same verdict — and it is
why this is version 4 of the comparator rather than 3.1 of anything. The wire
format_version stays 3. The two version numbers are now independent on purpose
(§6).

## 2. The v3 baseline, stated accurately

A specification that misstates its baseline produces implementers who "fix"
behaviour that was already correct. The draft claimed v3 maps measurements to
heuristic scores. It does not. For the record, v3 already has:

```text
ALREADY IN v3 (compare.rs, config.rs)                         KEPT IN v4
──────────────────────────────────────────────────────────────────────────
chance_correct(raw, ctl) = (raw−ctl)·SCALE/(SCALE−ctl)        yes — becomes the
    per-channel evidence above an explicit control                input to the LUT
local null: code bit-rotations {16, 32, 48}, re-matched       yes — family grows,
                                                                  aggregation flips
geometric null: correspondence shifts {n/2, n/3}, MAX         yes — family grows
DCT control: median of 16 D4×inversion hypotheses             yes — unchanged
repetition down-weighting: w += SCALE/max(burst_a, burst_b)   yes — unchanged
injective distance-stratified matching (match_bags)           replaced (§10)
corroboration floor: no structural certification under        yes — generalised
    three measurable secondary channels                           into rule R1
two-axis lattice, structure and geometry never averaged       yes — unchanged shape
five verdicts: Identical/Copy/Suspected/Related/Unrelated     yes + Indeterminate
canonical argument order, swapped-output restoration          yes — unchanged
hash-time / compare-time field partition in Config            yes — becomes §7's
                                                                  digest input
──────────────────────────────────────────────────────────────────────────
NOT IN v3                                                     ADDED BY v4
──────────────────────────────────────────────────────────────────────────
calibration as data (profiles are compiled constants)         §8
conservative local-null aggregation                           §9.5
optimal assignment                                            §10, Appendix A
descriptor-diversity statistics as evidence modulators        §10.4
spatial support of matches                                    §11
confidence-weighted Hough votes                               §12.2
multiple geometric models / per-model mirror                  §12.3
copy topology                                                 §13
Indeterminate verdict with reason codes                       §15
resource limits                                               §16
retrieval layer, formally separated                           §17
hash-profile and calibration-profile identity                 §6–§8
```

The consequence for planning: v4's first milestone is not "introduce nulls" —
it is *enlarge the null families, flip one aggregation, and move the constants
out of the binary*. That is a far smaller and far safer step than the draft's
framing implied, and Appendix E lists exactly which behaviours break.

## 3. Design principles

P1–P5 of SPEC-003 continue to apply. v4 adds four:

### P6 — Evidence is what survives the null family, not what a channel reports

No raw measurement reaches the lattice. Every channel value is the margin of
the measurement over the **maximum** of its structured nulls, mapped through a
calibration table. A channel that cannot run its nulls abstains; it does not
report raw.

### P7 — Calibration is data, not code

Thresholds, weights, lookup tables and limits live in one immutable byte
artefact (§8) parsed identically by both engines. Its SHA-256 is its identity.
Re-calibration ships a file, not two synchronised code edits — which also
deletes the largest remaining class of cross-engine parity failures: constants
mirrored by hand.

### P8 — Prefer structure to randomness

Every null in this specification is a closed-form index or bit map: rotations,
reversals, cyclic shifts. None requires a PRNG. A seeded generator is
implementable identically in two languages, but *proving* it identical is a
standing cost on every audit; a rotation is proved by reading it. Seeded nulls
are prohibited in consensus-visible behaviour.

### P9 — A difficult input earns lower evidence, never a guess

Resource exhaustion, profile mismatch, corruption and irreconcilable channel
disagreement produce `Indeterminate` with a reason code — a distinct verdict
that downstream systems MUST NOT collapse into `Unrelated` (§15).

## 4. Non-goals

Unchanged from the draft, restated because they bound the vocabulary of every
report the comparator emits. PAPH does not determine legal ownership, copyright
infringement, authorship, or intent; it does not replace human moderation in
ambiguous cases; it does not use runtime machine learning or any procedure
whose output can differ between conforming implementations. PAPH estimates
**evidence of structural reuse**, and its verdict labels (§14) are defined in
those terms only.

## 5. Terminology and the evidence currency

Terms from the draft (raw measurement, null model, evidence, calibration
profile, evidence channel, copy topology) keep their meanings. In addition:

**Structured null** — a null constructed by a closed-form permutation of stored
bits or indices, requiring no random source (P8).

**Evidence currency** — every evidence value, control value, LUT output,
threshold and weight in this specification is an integer in
`0 ..= SCALE = 10 000`, the currency v3 already uses. The draft's `x / 1000`
examples are void. All intermediate arithmetic is `i64`; every multiplication
of two currency values is divided by `SCALE` before further use; no
intermediate may exceed `2^62`. Rounding is truncation toward zero (`idiv`),
matching v3.

**Profile artefact** — the canonical byte encoding of a calibration profile
(§8, Appendix B). The artefact *is* the profile; a struct in memory is a view
of it.

## 6. Versioning

Three identifiers, fully independent:

```text
format_version         3        what the wire bytes mean (SPEC-003)
comparator_version     4        what the comparator does with them (this spec)
calibration_profile    CAL-xxx  which constants the comparator uses (§8)
```

plus two computed identities:

```text
hash_profile_id        SHA-256 over the hash-time configuration (§7)
calibration_profile_id SHA-256 over the profile artefact bytes (§8.2)
```

Compatibility matrix — a v4 comparator:

```text
reads format 3 wires                REQUIRED
reads format <3 wires               PROHIBITED (v2 wires were never
                                    cross-engine safe; refuse, do not adapt)
compares two wires whose
  hash_profile_ids differ           Indeterminate(PROFILE_MISMATCH), unless the
                                    caller explicitly overrides, in which case
                                    the report MUST carry both ids and the flag
runs without a calibration
  profile                           PROHIBITED — there is no default in code;
                                    CAL-001 is a file like any other
```

Every report names all five identifiers (§21). A moderation decision that
cannot state which calibration judged it is not reproducible, and
reproducibility is the product.

## 7. Hash-profile identity

`Config` already partitions its fields into hash-time (they change the wire)
and compare-time (they do not). The hash-time set is closed and exactly:

```text
order  field            encoding
 0     fold_matte       i32 LE, 0/1
 1     divide_upscale   i32 LE, 0/1
 2     matte_tol        i32 LE
 3     peak_radius      i32 LE
 4     fold_invert      i32 LE, 0/1
 5     local_windows[0] i32 LE
 6     local_windows[1] i32 LE
 7     local_count      i32 LE
 8     kp_count         i32 LE
 9     sketch_count     i32 LE
```

```text
hash_profile_id = SHA-256( "PAPH-HP" || 0x03 || field_0 || … || field_9 )
```

Ten fields, forty bytes of payload, one digest. The draft's "all fields
affecting the generated wire" is replaced by this enumeration precisely so that
adding a field is a *visible* spec change rather than a silent drift. The v3
wire has no room to carry the id; it travels at the index/storage layer
(§17–§18) and as a compare-time argument. A future format_version reserves
header bytes for it.

SHA-256 is chosen over faster digests for one reason consistent with the
crate's zero-dependency rule: both engines implement it by hand in ~150
auditable lines, exactly as they already do CRC-32. Any hand-rolled
implementation MUST pass the FIPS 180-4 vectors in the conformance suite.

## 8. The calibration profile artefact

### 8.1 Contents

One profile binds everything the draft scattered across "channel mappings,
null definitions, quantization rules, thresholds, decision thresholds, minimum
evidence requirements" — concretely:

```text
identity        container_version, comparator_version it targets, profile name
bound choices   evidence rule = Lift, scoring rule = Weighted, rag endpoint
matching        hamming_t, confidence_at
geometry        geo_conf_at, geo_eps, geo_min_corr,
                min_model_inliers, max_models
spatial         grid_g
repetition      rep_extreme_at
lattice         STRUCT_IDENTICAL/STRONG/MODERATE/WEAK/SOLO,
                GEO_STRONG/WEAK, GEO_SOLO_INLIERS, min_secondaries
weights         the seven structural channel weights
LUTs            local, geometry, diversity-modulation — monotone breakpoint
                tables, §8.3
limits          max_width, max_height, max_pixels
```

On the bound choices: v4 **retires `Proportion` evidence and `Gate` scoring
from the decision path**. Both remain computed and reported as diagnostics
(they disagree on real works, and that disagreement is itself a finding), but a
"profile" that leaves the scoring rule caller-chosen is not a profile — it is
two profiles wearing one id. The enums stay in `Config` for v3 compatibility;
a v4 comparison takes its values from the profile and ignores the config's.

### 8.2 Canonicality, identity, immutability

Appendix B fixes the byte layout: magic `PCAL`, fixed field order, little-
endian, length-prefixed tables. There is exactly one encoding of a given
profile; decode(encode(p)) MUST round-trip bit-exactly, and the conformance
suite asserts it.

```text
calibration_profile_id = SHA-256( artefact bytes )
```

A node MUST NOT mutate a loaded profile. There is no runtime training, no
adaptive threshold, no per-request adjustment. Changing one breakpoint is a new
artefact, a new digest, a new name in the ledger of decisions it has judged.

### 8.3 Lookup tables

A LUT is `k ≥ 2` breakpoints `(x_i, y_i)` with `x_0 = 0`, `x_{k-1} = SCALE`,
`x` strictly increasing, `y` non-decreasing, all in currency. Evaluation is
integer linear interpolation between the surrounding breakpoints, `idiv`
rounding. Monotonicity is validated at decode; a non-monotone table is a
corrupt profile, not a creative one — a calibration that could *lower* evidence
as raw margin rises would let an adversary aim for the dip.

The draft's `raw local matches = 27 → null percentile 99.2% → 742` is
unimplementable as written: four deterministic nulls give a max-statistic, not
a percentile. v4 defines it the only closed way:

```text
margin  = chance_correct(raw, max_over_null_family)     (v3's function, kept)
evidence = LUT_channel(margin)
```

The *percentile idea survives in how the table is built* (§19.4): during
calibration, offline, on the corpus, with as much floating point as the
statisticians like — none of which exists at runtime. `CAL-001-PROVISIONAL`
ships identity tables (`evidence = margin`) so that M1 changes only what §9.5
changes and nothing else; the first empirical tables arrive as `CAL-002`
(§19).

## 9. The null framework

### 9.1 Requirement

Every evidence channel evaluates a family of structured nulls and takes the
**maximum** as its control. The family answers one question: *how strong could
this measurement look between images that share statistics but not content?*
Taking the maximum makes the answer conservative; an adversary must beat the
family's best accident, not its average one.

### 9.2 Local family LN

The local channel's nulls operate on the stored 64-bit region codes of side B,
re-matched against side A by the same matcher as the measurement:

```text
LN1   rot16     both words rotated left 16   (v3)
LN2   rot32     hi/lo word swap              (v3, = rotation by 32)
LN3   rot48     both words rotated left 48   (v3)
LN4   bitrev    bit-reversal of the 64-bit code
```

All four preserve each code's popcount and the pairwise-distance structure
within the transformed side, while destroying any genuine correspondence with
the untransformed side — codes with the same statistics that cannot be the
same regions. `bitrev` is added because the three rotations share an orbit; a
reversal is the cheapest map outside it. Exact index maps in Appendix D.

### 9.3 Geometric family GN

The correspondence set is kept exactly as matched — same count, same
distinctiveness — and only the *pairing of geometry* is permuted, which is
precisely v3's insight, retained verbatim. The family, for `n = |corr|`:

```text
GN1   cyclic shift by ⌊n/2⌋        (v3)
GN2   cyclic shift by ⌊n/3⌋        (v3)
GN3   cyclic shift by ⌊n/5⌋
GN4   cyclic shift by ⌊2n/3⌋
GN5   reversal   k ↦ n−1−k
```

When the measurement is the two-pool multi-model procedure (§12.3), each
family member instantiates per pool from that pool's own length, and both
permuted pools run through the full extraction together. A member degenerate
for one pool contributes an EMPTY pool for it, never an unpermuted one — an
unpermuted pool would leak the measurement into its own control. Duplicate
offsets between members are permitted there; under MAX aggregation they cost
compute, never correctness.

For the single-bag local control (§10), duplicate or degenerate offsets (0,
≥ n, or equal to an earlier member) are
skipped. Each null runs through the **identical procedure as the
measurement** — including multi-model extraction once §12.3 is the
measurement — because a control that runs a cheaper procedure than the thing
it controls is not a control.

### 9.4 DCT

The median-of-sixteen control is already conservative by construction (taking
the best of more hypotheses cannot inflate the reading) and is retained
unchanged.

### 9.5 The aggregation flip — a breaking change, named as one

v3 aggregates its geometric nulls with `max` and its local nulls with `min`
(compare.rs, `if wr < e`). The lenient side dates from tuning against sixteen
works, where a pathological rotation occasionally erased real evidence. Under
v4, both channels take the maximum. **This lowers local evidence on real pairs
and will move verdicts.** It is the one deliberate verdict-breaking change of
M1, it is why the comparator version is 4, and the calibration campaign (§19)
MUST report transformed-copy recall before and after it in the same table. If
recall degrades beyond the operating point, the remedy is the `CAL-002` local
LUT — not a return to `min`.

## 10. The local channel

### 10.1 What is kept

Fingerprint reading, `burst` self-repetition counts, the
`w += SCALE / max(burst_a, burst_b)` weighting, the capacity normaliser, and
the Lift evidence rule are unchanged. Repetition weighting stays **here and
only here**: the draft placed a repetition penalty inside the matcher's edge
cost *and* a repetition modulator after it, which charges the same sin twice
and makes the calibration corpus pay for it. v4's assignment cost is descriptor
distance, nothing else.

### 10.2 Assignment

`match_bags`' distance-stratified greedy is replaced by a deterministic
**maximum-cardinality, minimum-cost assignment** on the edge set
`{(i, j) : hdf(i, j) ≤ hamming_t}`, cost = the distance itself. Greedy's
failure mode is real — an early high-quality pair consuming the target that a
better global assignment needed — and it is exactly the repeated-descriptor
case this whole revision targets.

The parity risk of "use the Hungarian algorithm" is not tie-breaking; it is
that textbook implementations differ in augmenting-path order while all being
"the Hungarian algorithm". Appendix A therefore specifies the procedure at the
level SPEC-003 specifies `isqrt`: row order, reduced-cost potentials, the
tie rule (smallest tentative distance, then smallest column index), and the
output order. The reference implementation is `rust/src/assignment.rs`; the
conformance suite verifies it against exhaustive search on every instance small
enough to enumerate. Complexity is O(n³) with n ≤ 128 — some 2M integer
operations, priced into §23.

The result is unique by construction, so JavaScript and Rust agree because
they compute the same function, not because they happen to iterate alike.

### 10.3 Spatial recording

Each retained pair records its anchor coordinates on both sides (the wire has
always carried them; v3 read them only for the coherence probe). They feed §11.

### 10.4 Diversity

Alongside the match weight, the channel computes for each side:

```text
D_side = ( Σ_i  SCALE / burst_i ) / n          diversity  =  min(D_a, D_b)
```

`D = SCALE` means every code is unique at radius `hamming_t`; `D → 0` means
the work is one tile wearing costumes. Diversity does not gate and does not
score. It does two things only: it indexes the modulation LUT
(`evidence = evidence · LUT_div(D) / SCALE`, neutral in CAL-001), and it
arms lattice rule R2 (§14.3). The draft's further statistics
(`frequency_entropy`, `duplicate_cluster_count`) are deferred to the corpus:
if `D` alone separates the tileset negatives, they are complexity without
evidence.

## 11. Spatial support

Anchor and keypoint coordinates live in the wire's 16-bit normalised frame
(0…65535 on the longest side). The canvas is divided into a `g × g` grid,
`g` from the profile (CAL-001: 4):

```text
cell(x) = (x · g) >> 16
```

For any set of matched positions the channel computes, per side, in currency:

```text
occupied      cells containing ≥ 1 match
coverage      occupied · SCALE / g²
bbox_cells    area of the occupied cells' bounding box, in cells
concentration largest 4-connected occupied region · SCALE / occupied
```

Connectivity is 4-neighbour; labelling is the deterministic two-pass row-major
union-find of `rust/src/coverage.rs` (min-label wins). The draft's separate
"dispersion" metric is dropped as derivable from these three. The lattice
consumes `min(coverage_a, coverage_b)` — twenty matches inside one tile and
twenty spanning the canvas stop being the same evidence, which was the point.

`g = 4` saturates near 16 matches; that is acceptable for a *support* signal
(it distinguishes 1–3 cells from many) and `g` is a profile field precisely so
CAL-002 can raise it with corpus justification rather than taste.

## 12. The geometric channel

### 12.1 What is kept

`correspond`'s mutual-best Lowe-ratio pairing, the level-dimension scale
derivation, Lowe soft binning over up to eight cells, the closed-form
fixed-point least-squares refit, and the loose-gather / refit / tight-count
pattern are unchanged. RANSAC remains banned.

### 12.2 Weighted votes

`correspond` already computes best and second-best distances on both sides and
then discards the margins. v4 keeps them. Each correspondence carries

```text
conf = 1 + idiv( (d2 − d1) · 63 , max(d2, 1) )         conf ∈ 1 ..= 64
```

where `d1` is the matched distance and `d2` is the SMALLER of the two sides'
second-best distances — ambiguity on either side lowers confidence, matching
the mutual-best requirement it rides on. Each correspondence votes `conf`
into **each** of its soft-binned cells — duplicated, not split. Duplication
is normative because splitting changes peak statistics near bin boundaries,
which is where Lowe binning operates by design. A correspondence whose
nearest neighbour is unambiguous (`d2 ≫ d1`) now moves a peak up to 64× more
than one that barely won its ratio test; the accidental agreements that
survive ratio testing are precisely the barely-won ones. The minimum-peak
floor applies to the winning cell's MEMBER COUNT (≥ 3 correspondences), not
its weighted mass: the floor exists for the model fit, and the fit counts
points, not confidence. Peak
selection is unchanged: maximum vote total, ties to the smallest cell key.

### 12.3 Multiple models

One transformation per comparison cannot represent a collage. v4 extracts
models iteratively:

```text
pools:   corr_direct = correspond(A, B)
         corr_mirror = correspond(mirror(A), B)        (per P2, free)

loop (at most max_models times):
    for each pool with ≥ geo_min_corr live pairs:
        run the vote → fit → refit → count procedure
    winner = pool with more inliers; tie → direct
    if winner.inliers < threshold: stop
         threshold = geo_min_corr for the first model,
                     min_model_inliers thereafter
    record model { r00, r10, tx, ty, mirror, inliers }
    consumed = B-side keypoint indices of the winner's inliers
    remove from BOTH pools every pair whose B index ∈ consumed
```

Consumption is by **B-side keypoint**, across both pools: one host region
explains one pasted element once, whether the paste was mirrored or not. The
mirror hypothesis thereby becomes **per-model** — v3 chose direct-or-mirrored
once per comparison, so a collage containing one mirrored element among direct
ones was invisible; this also progresses SPEC-003 §15's open inversion-
invariance risk. Scale, per model, is `isqrt(r00² + r10²)` in Q16, exactly as
v3 reports it.

The channel's raw measurement becomes the total inlier count over accepted
models; its control (§9.3) runs this same loop on each permuted pool and takes
the family maximum. Evidence = `LUT_geo(chance_correct(raw, control))` with
both counts normalised by `geo_conf_at` as in v3.

## 13. Copy topology

From the accepted models and the grid of their inlier B-positions:

```text
0  none        corr_direct and corr_mirror both below geo_min_corr
1  scattered   correspondences exist, no model reached its threshold
2  single      1 model,  bbox_cells · SCALE / g²  <  6000
4  dominant    1 model,  bbox_cells · SCALE / g²  ≥  6000
3  multiple    ≥ 2 models
```

Topology is reported always and consumed by exactly one lattice rule (R3);
forty-five scattered inliers and forty-five forming one coherent object stop
sharing a meaning. The constants are profile fields.

## 14. Consistency and the verdict lattice

### 14.1 Division of labour

Rule **shapes** are comparator-versioned — they are code, tested by golden
vectors. Rule **parameters** are profile data. A profile cannot invent a rule;
it can only move one. This is narrower than the draft's "the lattice SHOULD
contain interaction rules", and it is what makes the rules testable.

### 14.2 States

```text
Identical      byte-identical Tier 1
Copy           evidence over the high thresholds with required consistency
Suspected      evidence over the suspicion thresholds, consistency incomplete
Related        structural family resemblance below suspicion
Unrelated      nothing above the null family
Indeterminate  §15 — the comparison could not be completed honestly
```

The draft proposed renaming `Copy` to `Likely copy` and silently dropped
`Identical` and `Related`. Rejected, all three ways: `Related` is load-bearing
(same-family-not-a-copy is the false-positive class §2 exists to protect, and
the state in which same-generator negatives should land); `Identical` is the
one certification that is *proof*, not evidence; and the label `Copy` keeps
three releases of moderation tooling working while §4 already bounds its
semantics to structural reuse. The report, not the enum, carries the nuance.

### 14.3 Rules

The two axes are never averaged (SPEC-003 P-lattice, unchanged). On top of the
v3 lattice conditions, restated with profile parameters, v4 adds:

```text
R1  corroboration floor     structural certification requires evidence from
    (v3, generalised)       the local channel plus ≥ min_secondaries other
                            measurable structural channels
R2  repetition guard        diversity < rep_extreme_at AND geometric evidence
                            < GEO_WEAK  ⇒  verdict capped at Suspected,
                            reason "repetition"
R3  scatter guard           geometry certifying alone (the crop/collage arm)
                            additionally requires topology ∈ {2, 3, 4}
R4  support guard           structural certification alone (the recolour arm)
                            additionally requires min-side coverage ≥
                            coverage_floor (CAL-001: 2500)
```

Each rule, when it fires, appends its name to the report's `basis` — a capped
verdict that cannot say which rule capped it is indistinguishable from a
threshold miss, and moderators appeal the difference.

## 15. Indeterminate

Returned, with one or more reason codes, when the comparison cannot complete
honestly:

```text
CORRUPT             wire fails CRC or structural validation
LIMIT               a §16 limit was exceeded
PROFILE_MISMATCH    hash_profile_ids differ and no override was given
PROFILE_UNSUPPORTED calibration artefact invalid, or targets another
                    comparator version
CONTRADICTION       reserved: channel disagreement beyond profile bounds
```

`Indeterminate` MUST NOT be treated as `Unrelated` by any downstream system: a
queue that drops indeterminates has decided that resource exhaustion is
exoneration, and adversaries construct resource exhaustion.

## 16. Resource limits

v3 defines none; `front.rs` accepts anything. v4 rejects, deterministically,
at hash time:

```text
MAX_WIDTH   16 384      MAX_HEIGHT  16 384      MAX_PIXELS  2^24
```

(The wire caps dimensions at 65 535 regardless; palette and keypoint counts
are already format-bounded at 128/256/32.) Compare-time work is bounded by
construction and priced in §23: assignment ≤ 128³, correspondence ≤ 256²,
models ≤ max_models, nulls ≤ |LN| + |GN| runs. Profiles may lower the limits,
never raise them. Exceeding a limit is `Indeterminate(LIMIT)`, not a resize:
the comparator never silently substitutes a different image for the one it was
asked about.

## 17. The retrieval layer

Retrieval answers *which stored works are plausible candidates*; the
comparator answers *whether a candidate shows evidence of reuse*. v4 makes the
separation formal:

```text
The retrieval layer MUST NOT emit verdicts, and no comparator input may be
restricted by anything retrieval computed.
```

Candidate sets are the union of independent recall-first indexes over Tier-1
material: the 32-keypoint sketch, the DCT section, palette statistics, and the
local codes under multi-band exact-match hashing (bands of 16 bits, band
count/width profile-scoped at the retrieval layer, which is NOT
consensus-visible and may evolve freely). A missed candidate is unrecoverable
downstream; a spurious one costs a comparison. Retrieval therefore tunes for
recall and is permitted to be embarrassing about precision.

## 18. Tier-2 binding

CRC-32 stays for what it is good at: fast corruption detection, including the
byte transposition an index introduces. It is not content binding. v4 requires
storage and index layers to maintain

```text
tier1_digest = SHA-256(Tier-1 bytes), truncated to 128 bits
```

alongside every stored Tier 2, and comparators SHOULD verify it when supplied.
The v3 wire is not modified to carry it; a future format_version reserves
Tier-2 header bytes 16…31 for exactly this. 128 bits is the floor; the full
256 is recommended wherever storage is not the constraint.

## 19. Calibration procedure and corpus

### 19.1 Categories

The benchmark corpus is partitioned as the draft proposed:

```text
A exact duplicates          E same-generator negatives
B benign transformations    F repeated-pattern negatives
C partial copies            G shared-template negatives
D collages                  H random unrelated
                            I adversarial hard negatives
```

No aggregate number is acceptable; every reported metric is per-category. The
sixteen-work corpus behind v3's structural thresholds is the acknowledged debt
this section retires.

### 19.2 Sourcing

Concretely, for Pixagram's deployment: positives and ambiguous cases from
moderation reports (with resolution labels); **E** mass-produced from the
platform's own SDXL pixel-art converter — same prompt family, varied seeds —
which manufactures the same-generator statistics no public corpus has; **F**
and **G** from published tilesets and icon packs (Kenney, OpenGameArt;
licence-checked); **I** generated against the null families of §9, per §22.

### 19.3 Protocol

Train/validation/test splits are fixed before any tuning; the test split is
touched once per candidate profile; every published profile names its corpus
versions, split hashes, operating point, and the resulting per-category table.
The operating point for CAL-002 is stated in advance:

```text
primary     false positives on E ∪ F ∪ G at `Copy`:  ≤ 1 in 1 000
secondary   recall on B at `Suspected` or better:    ≥ 0.95
            recall on C ∪ D at `Suspected` or better: report, no gate yet
```

### 19.4 Table construction

Offline, per channel: compute the margin (§8.3) for every corpus pair, take
the empirical mapping from margin to the label-separating statistic of choice,
enforce monotonicity (isotonic regression or by hand), quantise to ≤ 33
breakpoints in currency, encode. Floating point is permitted here and only
here; what ships is integers. `CAL-002` is the first artefact from this
procedure; `CAL-001-PROVISIONAL` (Appendix C) exists so that M1 is testable
before the corpus exists.

## 20. Conformance

Extends SPEC-003 §10. Golden vectors are REQUIRED for: SHA-256 (FIPS 180-4),
profile encode/decode/digest round-trip, every LN and GN index map, the
assignment procedure (including exhaustive verification on all instances with
n·m ≤ 49), coverage labelling, multi-model extraction with per-model mirror,
topology classification, each lattice rule firing and not firing, and every
`Indeterminate` reason. Cross-engine, the existing property is extended: same
wires + same profile artefact ⇒ same verdict, same evidence integers, same
basis list, same reason codes. Category coverage per the draft §25 is kept:
identical / copies / partial / multi-region / collages / extreme palettes /
empty channels / limit cases / calibration boundaries / ties.

## 21. The report

The v3 JSON gains, without renaming anything it already has:

```text
comparator: 4
profile:    { format: 3, hash: <hex16>, calibration: "CAL-…", id: <hex16> }
evidence:   { local, geometry, … }        calibrated, currency
spatial:    { coverageA, coverageB, bboxCells, concentration }
geometry:   { models: [ {r00, r10, tx, ty, scaleQ16, mirror, inliers} ], … }
topology:   0…4
diversity:  0…10000
basis:      now including fired rule names
reasons:    for Indeterminate
diagnostics:{ lift, proportion, gate }    retired from decision, kept visible
```

A moderator reading the report can answer *why* — which channels, which
models, where on the canvas, under which calibration — which is Phase 5 of the
draft delivered as a schema instead of an aspiration.

## 22. Security considerations

The threat list of the draft stands: descriptor-collision farming, repetition
flooding, calibration-boundary aiming, geometric decoys, palette manipulation,
counter overflow, pathological Tier-2, worst-case compute. v4's specific
answers: every counter path is `i64` with stated bounds (§5); compute is
bounded by §16 with no allocation proportional to attacker-chosen values
beyond them; boundary-aiming is blunted by monotone LUTs (§8.3); repetition
flooding lands in R2; decoys must now defeat the *maximum* of the null family
(§9.1) and the multi-model consumption rule (§12.3) simultaneously. The
standing test: for every optimisation, an adversarial input is added to
category I that would have exploited its absence. A difficult image earns
lower evidence, never a guess (P9).

## 23. Performance budget

Additions over SPEC-003 §12, worst case, per comparison:

```text
assignment            128³ ≈ 2.1 M int ops          (once)
null re-matching      |LN| = 4 × assignment          (§9.2 uses the same matcher)
multi-model           ≤ max_models × vote/fit/count  (each ≤ v3's single pass)
geometric nulls       |GN| ≤ 5 × the above
coverage              O(matches + g²)
profile decode        once per process, cached by id
```

Order of magnitude: the v4 comparator costs single-digit multiples of v3's
compare path and nothing at hash time. If the null-family matching dominates in
practice, the sanctioned relief is reducing |LN| by corpus evidence — never
per-image shortcuts, which would make cost a side channel.

## 24. Migration and milestones

```text
M1  foundations         profile artefact + SHA-256 + null framework +
                        assignment + coverage + multi-model + topology,
                        Rust reference with golden tests.  CAL-001-PROVISIONAL.
                        v3 behaviour untouched and still shipping.
M2  local channel       rebuild on assignment + LN-max + diversity; emit
                        cross-language golden vectors from the Rust reference.
M3  full v4 verdict     weighted votes, nulls-through-multimodel, rules
                        R1–R4, Indeterminate; the v4 lattice goes live behind
                        an explicit comparator-version switch.
M4  JavaScript engine   port; byte-identical profile parsing; parity suite
                        extended to v4 reports.
M5  CAL-002             corpus (§19), first empirical profile, per-category
                        benchmark publication; §9.5's recall check.
```

v3 comparisons remain available throughout under comparator_version 3; indexes
re-judge pairs by re-running the comparator, never by re-hashing. The draft's
Phase 1–5 map onto M1–M5 with one correction of order: the corpus (draft
Phase 1) is M5's deliverable but M1's *scaffold* — categories and sourcing are
fixed now, so that every milestone in between knows what it will be measured
against.

## 25. The principle

Retained from the draft, now with the machinery to enforce it:

> **Similarity is not evidence of copying unless the observed similarity is
> sufficiently improbable under the relevant null family and is supported by
> coherent independent structure.**

Improbable-under-the-null is §8–§9. Coherent is §11–§13. Independent is §14.
Everything else in this document is bookkeeping so that two strangers'
machines agree about all three.

---

## Appendix A — The assignment procedure, exactly

Input: `n × m` integer costs, `-1` meaning no edge, otherwise `0 ≤ c ≤ 2^15`.
Output: the unique matching that (1) has maximum cardinality, (2) minimum total
cost among those, (3) and among those, the lexicographically smallest sequence
of `(row, col)` pairs when listed in ascending row order.

Procedure — successive shortest augmenting paths with potentials:

```text
u[0…n) = 0 ;  v[0…m) = 0 ;  row_of[0…m) = NONE ;  col_of[0…n) = NONE

for r in 0, 1, …, n−1:                        # rows in ascending index, once
    dist[0…m) = +∞ ; prev[0…m) = NONE ; final[0…m) = false
    for each j with cost(r,j) ≥ 0:
        dist[j] = cost(r,j) − u[r] − v[j] ; prev[j] = SOURCE
    loop:
        j* = argmin over non-final j of (dist[j], j)      # tie → smaller j
        if no such j* or dist[j*] = +∞: row r stays unmatched; break
        final[j*] = true
        r' = row_of[j*]
        if r' = NONE:                          # augment along prev links
            walk prev from j*, alternating assignments; update potentials:
                for every final j:  v[j] += dist[j] − dist[j*]
                u[r] += dist[j*] ;  for every re-routed row r'': u[r''] adjusted
                by the standard reduced-cost update
            break
        else: relax every non-final j' with cost(r',j') ≥ 0 through r'
```

Two clauses carry the determinism: the row loop visits each row exactly once
in index order (the standard lemma — no augmenting path appears later for a
row that had none — makes a single pass sufficient for maximum cardinality),
and every argmin breaks ties toward the smaller column index. All arithmetic
is `i64`; `+∞` is `i64::MAX/4`. The reference implementation is
`rust/src/assignment.rs`; where prose and reference disagree, the golden
vectors decide, and the exhaustive-search cross-check in its test module is
the ultimate arbiter of clauses (1) and (2).

## Appendix B — PCAL artefact layout

All integers little-endian. One encoding per profile; no optional fields, no
padding, no extensions — a future field is a new container_version.

```text
off  size  field
0    4     magic "PCAL"
4    2     container_version   = 1
6    2     comparator_version  = 4
8    16    name, ASCII, NUL-padded            "CAL-001-PROVISIONAL…"
24   1     evidence_rule       0 = Lift       (bound; Proportion retired)
25   1     scoring_rule        1 = Weighted   (bound; Gate retired)
26   1     rag_endpoint        1 = Rank
27   1     grid_g
28   4     hamming_t           i32
32   4     confidence_at       i32
36   4     geo_conf_at         i32
40   4     geo_eps             i32
44   4     geo_min_corr        i32
48   4     min_model_inliers   i32
52   4     max_models          i32
56   4     rep_extreme_at      i32 (currency)
60   4     coverage_floor      i32 (currency)
64   4     min_secondaries     i32
68   36    thresholds: STRUCT_IDENTICAL, STRUCT_STRONG, STRUCT_MODERATE,
           STRUCT_WEAK, STRUCT_SOLO, GEO_STRONG, GEO_WEAK,
           GEO_SOLO_INLIERS, topology_dominant_at   — 9 × i32
104  28    weights: local, shape, topology, runs, dct, palette, silhouette
           — 7 × i32
132  12    limits: max_width, max_height, max_pixels — 3 × i32
144  …     three LUTs, each:  u16 count k, then k × (u16 x, u16 y)
           order: local, geometry, diversity
```

Decode MUST validate: magic, versions, `x` strictly increasing from 0 to
10 000, `y` non-decreasing and ≤ 10 000, every scalar within its documented
range, and total length exact. Any failure is `PROFILE_UNSUPPORTED`.

## Appendix C — CAL-001-PROVISIONAL

Purpose: make M1 testable before the corpus exists, changing nothing but what
§9.5 changes. Provenance: every value is either a v3 shipping constant
(`config.rs`) or a new mechanism switched to neutral.

```text
from v3, unchanged        hamming_t 8 · confidence_at 16 · geo_conf_at 16
                          geo_eps 1600 · geo_min_corr 8 · thresholds
                          4500/3000/1500/6750, 3500/1200, 15, identical 8000
                          weights 35/25/15/10/10/5/10 · min_secondaries 3
new, provisional          grid_g 4 · min_model_inliers 6 · max_models 4
                          rep_extreme_at 1500 · coverage_floor 2500
                          topology_dominant_at 6000
                          limits 16384 / 16384 / 16777216
LUTs                      local, geometry: identity, (0,0)–(10000,10000)
                          diversity: NEUTRAL — constant, (0,10000)–(10000,10000)
                          (identity is neutral for a margin→evidence map;
                          for a multiplier it would scale evidence by D/SCALE)
```

Every "new, provisional" value is a hypothesis for §19 to test, and CAL-002 is
expected to move all of them. Its digest is printed by
`cargo test -p paph3 cal001_id -- --nocapture` and recorded in IMPL-004.

## Appendix D — Null index maps

Local, on a 64-bit code stored as words `(hi, lo)`:

```text
rot16(hi,lo)  = (hi<<16 | lo>>16 ,  lo<<16 | hi>>16)
rot32(hi,lo)  = (lo, hi)
rot48(hi,lo)  = (lo<<16 | hi>>16 ,  hi<<16 | lo>>16)
bitrev(hi,lo) = (rev32(lo), rev32(hi))        rev32 = 32-bit bit reversal
```

Applied to every code of side B; side A untouched; matching and weighting
identical to the measurement. Geometric, on a correspondence list of length n:

```text
shift_p:  (aᵢ, bᵢ, dᵢ)  ↦  (aᵢ, b₍ᵢ₊p₎ mod n, dᵢ)    p ∈ dedup{⌊n/2⌋,⌊n/3⌋,⌊n/5⌋,⌊2n/3⌋}\{0}
reverse:  (aᵢ, bᵢ, dᵢ)  ↦  (aᵢ, b₍n−1−ᵢ₎, dᵢ)
```

Descriptor pairing, counts and confidences are preserved; only geometry is
re-dealt. All maps are involutions or cycles with no random source (P8).

## Appendix E — Breaking changes, comparator 3 → 4

```text
1  local null aggregation min → max over LN         verdicts can move (§9.5)
2  matcher: stratified greedy → optimal assignment   match sets can differ (§10.2)
3  Proportion / Gate removed from the decision path  configs requesting them
                                                     are ignored in v4 (§8.1)
4  new caps R2–R4 can lower a v3 Copy to Suspected   (§14.3)
5  Indeterminate exists; error paths that returned
   Err or Unrelated now return it with reasons       (§15)
6  a calibration artefact is REQUIRED                (§8)
```

Not breaking: every wire byte, every hash, Tier-2, the sketch, all channel
definitions not named above, the report fields v3 already emits.

## Amendment 4.1 (normative) — comparator 41

Landed as milestone M6, 2026-08-19.  Comparator 4 above is FROZEN: every
section of this document keeps its meaning for the field value 4, and a
conforming comparator-4 engine is unchanged by this chapter.  Comparator 41
is the same machine with exactly four amendments; each engine refuses the
other's calibration artefacts, in both directions.

### A4.1.1 — Weak-signal geometry (amends §12.3)

The measurement is extraction-first.  When `extract_from_pools` accepts at
least one model, the geometric measurement is `total_inliers`, exactly as in
§12.3.  When it accepts none, the measurement is the WEAK SIGNAL: the best
single verified model's inlier count over the two pools — the §12.3 verifier
run once per pool of size ≥ `geo_min_corr`, member floor 3 kept, the
model-ACCEPTANCE floor (`min_model_inliers`) absent.  Every GN family member
(Appendix D) runs this identical measurement on its permuted pools; the
control is their maximum.  The report gains `geo_weak_inliers` (the weak
count when it was used, else 0), and `total_inliers` carries the weak count
in that case.  Topology (§13) is computed from the extraction's models as
before — no accepted model still means class ≤ 1, so R3 keeps a weak signal
away from solo certification by construction.  The weak signal changes
margins, never the certification topology floor.

### A4.1.2 — Assignment on the edge-induced subgraph (amends Appendix A)

Under comparator 41 the assignment of §10.2 is computed on the subgraph
induced by the edge set: the rows and columns holding at least one edge
(cost ≥ 0), taken in ascending original index, with dummy completion applied
to the subgraph only.  The algorithm on the subgraph is Appendix A verbatim.
Cardinality and total cost always equal the full matrix's; matched pair-sets
may differ where cost ties exist, because full-matrix dummy columns no
longer participate in tie-breaking.  That divergence is why this is a
comparator bump and not a patch.

### A4.1.3 — Container 2 (amends §8 / Appendix B)

A container-2 artefact carries NINE monotone tables in canonical order:

```text
local, geometry, diversity, dct, shape, topology, runs, palette, silhouette
```

Header, fixed block, and per-table wire format are Appendix B verbatim; only
the table count changes.  The (container, comparator) pairs (1, 4) and
(2, 41) are valid; any other pair is refused at decode with the Appendix B
error strings.  Under comparator 41 each non-local structural channel's
value passes through its named table before the §14 lattice; the local slot
keeps its §10 tables; a container-1 profile decoded by a comparator-4 engine
behaves exactly as before.

### A4.1.4 — The stage-1 screen (amends §17)

The screen is normative-advisory: `pass ⇔ max(pool_direct, pool_mirror) ≥
geo_min_corr`, computed from the §12.2 correspondence pools in canonical
argument order, with no verification and no verdict.  A pair the screen
rejects is UNSCREENED — it never becomes Unrelated, and a screened-out pair
entering the full comparator is not an error.  The screen exists so a
marketplace can afford the full comparator where it matters; it may never
substitute for it.

### A4.1.5 — CAL-003-PROPOSED and the measured record

`CAL-003-PROPOSED` (container 2, comparator 41, 270 bytes, id
`75319777e4ff7fe6592365612cff9a85d653519166ff1308c8d3489d0247a422`) is the
comparator-41 default pending M7 validation.  Golden vectors: GOLDEN-004.json
milestone M6 adds `assignment_sparse`, `weak_geometry`, and
`profile_cal003`; both engines satisfy all sections, and cross-engine parity
holds field-for-field on both comparators.

> **Superseded as the package default, not as a record.**  SPEC-004.2 raised
> the Tier-2 budget to 512 and made comparator 42 the entry; comparator 41 and
> `CAL-003-PROPOSED` stay frozen and callable (`compare41` / `cal41`), and this
> chapter remains the specification of what they do.  The numbers below were
> measured at 256 keypoints and have NOT been re-derived on 4.2 wires.  The
> golden file has since moved to milestone M8.  See
> `docs/SPEC-004.2-paph-v42.md`.

On the 496-pair development corpus (real engine, not the offline evaluator):
zero false certifications; every §19 negative-category invariant lands
exactly (review negatives 9, Related 402, Unrelated 34, negative structural
maximum 2587); B-category 95.5% certified and 100% at review-or-better;
C+D 55.6% certified and 81.5% at review-or-better.  Metric honesty: the
proposal's projected "B 100% / CD 81.5%" were REVIEW-OR-BETTER rates (the
evaluator counted Suspected as recall); the implementation reproduces them
exactly on that definition.  Against shipped comparator 4, one certified
pair moves to review and none below — certification narrows to
geometry-anchored evidence, review widens to hold what narrowed.  These are
development-corpus numbers; §19.3 split discipline means M7 decides.
