# SPEC-004.1 — Proposal: comparator 4.1 and CAL-003

Status: **PROPOSAL**, awaiting decision. Nothing here is implemented; the
engines still ship comparator 4. Every claim is measured — the evidence
base is `docs/ANALYSIS-001-field-forensics.md` and the 496-pair dataset
(`paph4-fields-dataset.json`), all offline experiments run against
unmodified engines. The corpus is one generator, ten families, inferred
labels: this proposal is designed to survive that caveat by keeping every
change reversible through calibration, but M7 revalidation on real
moderation pairs is a gate, not a formality.

Comparator identity: the PCAL `comparator` field takes the value **41**
(read: 4.1). Comparator 4 remains frozen and callable; profiles targeting
4 stay valid against it. Verdict-affecting changes are what force the
bump: A1 changes verdicts on weak-geometry pairs, A2 changes them on
cost-tied assignments.

---

## Part I — Architectural changes

### A1. §12.3 — weak-signal geometry (the floor amendment)

**Problem.** The extraction accepts a first model only at
`inliers ≥ geo_min_corr` (8). Real transformed copies with small keypoint
pools produce verified models of 5–7 inliers; the floor reports them as
zero. Eleven corpus positives carry zero v4 geometric margin; v3, having
no such floor, surfaces 5–7 inliers on six of them from identical pools.

**Change (normative sketch).** §12.3 gains a *weak-signal path*: when the
extraction accepts no model, the geometric measurement is the best single
verified-model inlier count over the two pools with the member floor (≥ 3
— the fit's own requirement) retained and the acceptance floor removed.
The GN family runs the identical rule (§9.3 unchanged in spirit: the
control mirrors the measurement, whatever the measurement is). No model
is *accepted*: `models` stays empty, topology stays scattered, and R3
therefore keeps the weak signal away from solo certification — it can
raise a pair into review, never certify it alone. Report fields:
`geo_weak_inliers`, and `total_inliers` carries the weak count when the
model list is empty.

**Measured.** Geometry AUC 0.892 → 0.951 (matching v3's discrimination
while keeping the full multi-model control). Six positives recovered with
control = 0 on every one; leakage is exactly one negative in 445, at the
3-inlier minimum, margin 1875 — removed from review by the CAL-003
geometry LUT's low band. Projected C∪D recall at ≥ Suspected: 63.0 % →
**81.5 %**, all gates intact.

### A2. Appendix A — assignment on the edge-induced subgraph

**Problem.** The Hungarian runs on BIG-completed full matrices; 93 % of
`compare_v4`'s 53 ms lives in its five runs. Median active rows: 32 of
128.

**Change.** Appendix A redefines the assignment as computed on the
edge-induced subgraph: rows and columns holding at least one edge ≤ T,
taken in ascending original index; dummy completion applies to the
subgraph only. The tie rule ("smaller column wins") is thereby pinned on
real columns, not dummies.

**Measured.** ×14.3 on the local channel (projected `compare_v4` median
~12 ms). Sixteen of sixty-six probed pairs change their matched pair-set
— with **identical cardinality and identical total cost** in every case:
pure tie-break divergence, which is exactly why this is comparator-
versioned with regenerated golden vectors rather than an optimization.

### A3. §8 — per-channel calibration tables (container 2)

**Problem.** The style inflation of same-generator negatives is carried
independently by each channel (max |r| = 0.24), and v4 has calibration
tables only for local and geometry. `runs` awards a median 9066 to
unrelated pairs; nothing in the profile can say so.

**Change.** The PCAL container moves to version 2: after the unchanged
144 fixed bytes, **nine** breakpoint tables in canonical order —
`local, geometry, diversity, dct, shape, topology, runs, palette,
silhouette`. Each structural channel's value passes through its table
before weighting; local and geometry keep their existing semantics;
identity tables are the no-op for mappings, the constant-SCALE table for
the diversity multiplier (Appendix C's distinction, now applied nine
times). Validation, encoding, and SHA-256 identity are unchanged in
mechanism. Container 1 remains decodable for comparator-4 profiles.

**Measured.** With `runs` and `palette` tables active (values below),
negative structural collapses from p50 3902 / max 4759 to p50 1591 / max
2587 — the style floor is removed at the channel where it originates
instead of being absorbed into thresholds.

### A4. §17 — the stage-1 screen, made normative

**Change.** A defined pre-comparator: `screen = max(|pool_direct|,
|pool_mirror|) ≥ geo_min_corr`, computed from correspondences alone.
Advisory for retrieval and batch workloads; never a verdict; a pair the
index screens out is recorded as unscreened, not Unrelated.

**Measured.** 3.2 ms; keeps 50/51 positives; discards 190/445 negatives
before any heavy work. The one screened-out positive is a zero-geometry
re-render the full pipeline leaves at Related regardless.

### A5. Explicit non-changes, with reasons

The lattice shapes, R1–R4, and the LN-max flip stay. The dct null (its
control over-corrects: raw 0.732 vs corrected 0.695) is a **study item**
— one corpus is not enough to redesign a null family. Channel weights
stay at v3's values: the AUC-greedy retune improved rank separation and
wrecked the operating point (review 3+14 → 10+149); weights move only
with corpus-fitted tables, and the tables came first. The Tier-1 wire is
untouched. Two hash-tier items are *flagged for v5, not done here*:
Tier-2's 256-keypoint format ceiling (geometry is the identity channel
and it is provably starved on small re-renders — `kpA` saturates at 256
on every corpus image), and the 88 bytes of brightness + colour that feed
no verdict channel. Changing the wire for one corpus would be exactly the
sin SPEC-004 §19 exists to prevent.

---

## Part II — CAL-003-PROPOSED, the default configuration

Every value is derived from the architecture plus a measured quantile;
the derivation is the point — a future corpus re-derives, it does not
re-taste.

```text
artefact       270 B, container 2, comparator 41
name           CAL-003-PROPOSED
id             75319777e4ff7fe6592365612cff9a85d653519166ff1308c8d3489d0247a422

weights        35 / 25 / 15 / 10 / 10 / 5 / 10        (unchanged — see A5)
guards         rep_extreme 1500 · coverage_floor 2500 · min_secondaries 3

thresholds     identical 8000  (byte-equality gate, unused by the lattice arms)
               struct strong   4000    = 1.55 × the worst negative structural (2587);
                                          insensitive 3800–4500 on this corpus
               struct moderate 2400    just under negative p98 (2412) → a 9-pair
                                          review band exists by construction
               struct weak     1000    splits Related from Unrelated below the band
               struct solo     6000    = 1.5 × strong, v3's ratio preserved
               geo strong/weak 3500 / 1200   (geometry axis unchanged)
               geo solo inliers 11     zero geometric leakage across 445 negatives;
                                          rescues 14- and 11-inlier true crops
               topology dominant 6000  (unchanged)

tables (canonical order)
  local        (0,0)(5000,1000)(8800,4200)(9400,8800)(10000,10000)
                 — same-generator local-margin band (neg max 8801) squashed
  geometry     (0,0)(1900,1100)(3000,3000)(10000,10000)
                 — the A1 weak signal admitted; the single 3-inlier leak
                   (margin 1875) held below GEO_WEAK
  diversity    neutral (constant 10000)
  dct          identity          shape  identity
  topology     identity          silhouette identity
  runs         (0,0)(9100,900)(9700,4500)(10000,10000)
                 — knees at negative p50/p90 (9066 / 9626): the style floor
                   removed, the top decile kept
  palette      (0,0)(3000,800)(6500,4000)(10000,10000)
                 — knees at negative p50/p90 (2993 / 5872)

measurement    hamming_t 8 · confidence_at 16 · geo_conf_at 16 · geo_eps 1600
               geo_min_corr 8 · min_model_inliers 6 · max_models 4 · grid 4
               limits 16384 × 16384 × 2^24            (all unchanged)
```

Projected operating point (496 pairs, offline v4.1 evaluator, real §14
lattice):

```text
                          CAL-001 / v4    CAL-002-DRAFT / v4    CAL-003 / v4.1
false certifications        0 / 0              0 / 0                0 / 0
§9.5 losses                 4                  3                    3 (all ≥ Suspected)
recall B  ≥ Suspected       100 %              100 %                100 %
recall C∪D ≥ Suspected      100 %*             63.0 %               81.5 %
review load (pos+neg)       14 + 429           3 + 14               8 + 9
negatives S / R / U         429 / 16 / 0       14 / 423 / 8         9 / 402 / 34
```

\* CAL-001's C∪D "100 %" is bought with 429 negatives in the same review
queue — recall at the price of a queue that reviews everything.

The certification arms shift and the shift is intent, not accident:
under CAL-003, 24 positives certify on geometry alone (R3-guarded,
11-inlier floor), 12 on structure-and-geometry, 2 byte-identical. On a
same-generator platform, **geometry certifies; structure corroborates,
gates, and routes to review** — that is what the forensics said the
fields can actually support, and the configuration now says it too.

Residual review items are named, not hidden: three v3-certified pairs
sit at Suspected (the scribbled crop, two character re-renders — all
human-judgement cases), nine top-band same-generator negatives, and five
zero-evidence re-renders at Related.

---

## Part III — Migration and validation

```text
M6   implement 4.1     A1 + A2 + A3 + A4 in both engines; container 2;
                       GOLDEN-0041 regenerated (assignment ties, weak-
                       geometry cases, nine-table profile); parity4 green;
                       comparator 4 kept frozen beside it
M7   validate CAL-003  the bench, real moderation pairs, held-out split
                       (§19.3); E1×E2-type policy decided by a human first;
                       adversarial keypoint-disruption pairs added (§22);
                       PROPOSED suffix drops only on passing the stated
                       operating point
```

Risks, stated: single-generator corpus; the strong-threshold plateau
means 4000 is chosen by principle (headroom ratio), not by a corpus that
can distinguish 3800 from 4500; H-category negatives land mostly at
Related rather than Unrelated (shared converter statistics — semantically
tolerable, aesthetically imperfect); and A2 changes tie-case verdicts,
which is precisely why it ships under a comparator bump with golden
vectors or not at all.
