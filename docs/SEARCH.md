# Wiring PAPH into a search engine

PAPH is a **comparator**: give it two fingerprints and it says how one relates to the other —
`Identical`, `Copy`, `Suspected`, `Related`, `Unrelated`, or `Indeterminate` when it abstains —
with the evidence. A search engine has the opposite problem: one new image, a corpus of N, and
no time to compare against all of them. The bridge is the classic retrieve-then-verify design,
with PAPH supplying both halves:

```
ingest   image ─► hash() ─► wires: Tier 1 (3952 B) + Tier 2 (32 + 40·kp B, ≤ 20.5 KB) ─► store
                            └─► indexKeys()             ─► postings  (kind, key) → work

query    image ─► hash() ─► indexKeys({ query: true })  ─► Σ 1/df per key family ─► top K each
                                 ∪ other channels you already have (pHash, embeddings)
                            └─► rank(query, candidates, { gate: true }) ─► verdicts + evidence
```

Keys only **nominate**; the comparator **decides**. Nothing fuzzy lives in the index — every
key is an exact integer, so any store that can look up integers can hold it (SQLite, Postgres,
a KV store, Elasticsearch keyword fields), and two nodes building the index from the same
wires build the same index.

A complete, tested implementation for Cloudflare Workers (queue consumer, Durable Object
index, D1 verdicts, HTTP API) lives in [`integrations/pixagram-search`](../integrations/pixagram-search/README.md).

---

## 1. Fingerprint once, at ingest

```js
import { init } from '@pixagram/paph-x/wasm';
const paph = await init();                       // or init(compiledModule) in a Worker

const fp = paph.hash({ data: rgba, width, height });   // { t1, t2, width, height, kpCount, crc }
```

| image | hash, WebAssembly SIMD (Node 22) |
|---|---:|
| sprite 96×96 | 3.2 ms |
| scene 288×200 (typical Pixagram size) | 16 ms |
| work 512×384 | 59 ms |
| scene 1024×768 | 236 ms |

Store **both tiers**. Tier 1 alone (the 32-keypoint sketch) can be compared, but Tier 2's up to
512 keypoints are what the geometric evidence — crops, pastes, mirrors — is measured on.

The wire format is versioned (`WIRE_VERSION` 3) and deliberately stable: a comparator change
does not invalidate stored fingerprints. And it is deterministic — the same pixels give the same
bytes in the JavaScript engine, the native build and both WebAssembly builds — so wires can be
published and anyone can recompute a verdict from them.

Pass limits for anything user-supplied: `paph.hash(img, undefined, [maxW, maxH, maxPixels])`
refuses oversized images before reading a byte, and `paph.prepare(t1, t2, { strict: true })`
refuses a Tier 2 claiming more than 512 keypoints.

Budget memory, not just time. Hashing needs about 60 bytes of working memory per pixel
(37 MB at 768², 65 MB at 1024²), and WebAssembly memory never shrinks once grown. In a
128 MB serverless isolate, keep what you hash within ~768²: an exact nearest blow-up costs
only its divided size (the hasher divides it first), and anything larger can be divided or
box-filtered by an integer factor before hashing — deterministically, so the same large
image always yields the same wire. Within the budget, hash images as they are, so the wire
is the one anyone computes from the same pixels.

## 2. Index keys

`Engine.indexKeys(side)` returns two families of integer keys, all below 2⁵³ (so they survive
JSON, JavaScript numbers and SQLite `INTEGER` exactly):

| family | per work | what it is | what it survives |
|---|---:|---|---|
| **codes** | ~115 (≤ 128) | each Tier-1 local code folded to 53 bits: `(hi & 0x1fffff)·2³² + lo` | codes are canonical under the square's eight symmetries and the complement, so mirrors, rotations, inversions, integer upscales, crops and pastes keep many of them |
| **bands** | ~585 | ten 24-bit slices of a keypoint descriptor: band *j* = bytes 3j..3j+2 of its 32 bytes, key `j·2²⁴ + value`; constant bands (0, 0xffffff) dropped | pastes into another work, tight crops, some resampling — where the codes thin out |

The two sides of the index are deliberately asymmetric (`KEYS_VERSION` 1):

* **stored works** are indexed by their codes and the bands of their **64 strongest**
  keypoints (strength descending, then x, then y) — a bounded, small posting set per work;
* **queries** (`indexKeys(side, { query: true })`) use their codes and the bands of **all**
  their keypoints (≤ 512) **and of each keypoint's mirrored descriptor** — a reflection swaps a
  descriptor's two 16-byte halves — 6,000 to 9,500 band keys for a 512-keypoint work, which
  costs nothing to store because queries are not stored.

The derivation is twenty lines in `wasm/paph.js` on top of two ABI calls
(`paph_local_codes`, `paph_descriptors`), so any host can reproduce it. Version it: when it
changes, re-derive keys from the stored wires (no re-hashing).

## 3. Nominate

Score every indexed work that shares a key with the query by **Σ 1/df** over the shared keys
of each family, where *df* is how many works hold that key — rare keys count, keys every
dithered sky shares do not. Skip keys whose df is above a cap (256 is plenty: they carry almost
no weight and cost a long posting list). Keep the top K of each family (K = 32), and add what
any other channel you have nominates — pHash neighbours for re-encodes, embedding neighbours for
recolours and resamples. Duplicates merge; remember which channels nominated each candidate.

On SQLite — the Cloudflare integration's statement (`SQL.nominate` in its `src/paph/store.ts`)
with generic table names; Postgres takes the same shape with `unnest($1::bigint[])` in place of
`json_each`:

```sql
CREATE TABLE works (work_id INTEGER PRIMARY KEY, t1 BLOB NOT NULL, t2 BLOB,
                    keys TEXT NOT NULL,          -- the stored keys, so removal can undo df exactly
                    keys_version INTEGER NOT NULL);
CREATE TABLE postings (kind INTEGER NOT NULL, key INTEGER NOT NULL, work_id INTEGER NOT NULL,
                       PRIMARY KEY (kind, key, work_id)) WITHOUT ROWID;   -- kind 1 code, 2 band
CREATE TABLE df (kind INTEGER NOT NULL, key INTEGER NOT NULL, n INTEGER NOT NULL,
                 PRIMARY KEY (kind, key)) WITHOUT ROWID;

-- ?1 = {"codes":[…],"bands":[…]} (the query's keys), ?2 = df cap, ?3 = K, ?4 = excluded ids
WITH q AS (SELECT 1 AS kind, value AS key FROM json_each(?1, '$.codes')
           UNION ALL SELECT 2, value FROM json_each(?1, '$.bands')),
     live AS (SELECT q.kind, q.key, d.n FROM q CROSS JOIN df d
              ON d.kind = q.kind AND d.key = q.key WHERE d.n <= ?2),
     hits AS (SELECT p.work_id AS id,
                     SUM(CASE WHEN live.kind = 1 THEN 1.0 / live.n ELSE 0 END) AS codes,
                     SUM(CASE WHEN live.kind = 2 THEN 1.0 / live.n ELSE 0 END) AS bands
              FROM live CROSS JOIN postings p ON p.kind = live.kind AND p.key = live.key
              WHERE p.work_id NOT IN (SELECT value FROM json_each(?4))
              GROUP BY p.work_id),
     ranked AS (SELECT id, codes, bands,
                       ROW_NUMBER() OVER (ORDER BY codes DESC, id) AS rc,
                       ROW_NUMBER() OVER (ORDER BY bands DESC, id) AS rb FROM hits)
SELECT id, codes, bands, rc, rb FROM ranked
WHERE (codes > 0 AND rc <= ?3) OR (bands > 0 AND rb <= ?3)
ORDER BY MIN(CASE WHEN codes > 0 THEN rc ELSE 1e9 END, CASE WHEN bands > 0 THEN rb ELSE 1e9 END), id;
```

The plan is a nested loop — the query's keys, a primary-key probe into `df`, a primary-key
probe into `postings` — and should stay one; `CROSS JOIN` pins the order. Two traps found by
testing the plans: a row-value `IN` must cover the **whole** primary key
(`(kind, key, work_id) IN (SELECT kind, key, ?2 FROM …)`), and a compound key set must sit in a
`FROM` subquery — written bare, SQLite scans the table.

On Elasticsearch / OpenSearch, store `codes` and `bands` as `keyword` arrays and query each with
a `terms`-style `bool.should`: BM25's IDF on keyword fields is the 1/df weighting, for free.

## 4. Verify

```js
const q = paph.prepare(query.t1, query.t2, { strict: true });
const sides = rows.map(r => paph.prepare(r.t1, r.t2));
const hits = paph.rank(q, sides, { gate: true });   // one call, lean readings
// hits[i]: { state, verdict, certifiable, structural, geometryEvidence, totalInliers, mirrored, … }
```

`prepare` parses a wire pair once (~0.1 ms); `rank` runs the stage-1 screen on every candidate
and comparator 42 on the ones it passes (`state: -1` = unscreened, never compared). A screened
pair costs ~1–5 ms in WebAssembly; a nominated set of a few dozen is ~50–200 ms.

What to keep:

| verdict | meaning | typical action |
|---|---|---|
| `Identical` | the same work | duplicate |
| `Copy` | one is a transformed copy of the other — mirrored, cropped, rescaled, recoloured, pasted | the actionable one: alert, link, flag |
| `Suspected` | real shared structure, not enough to certify | review queue |
| `Related` | shares style or assets | usually drop |
| `Unrelated` / `Indeterminate` | — / the comparator abstains | drop |

`certifiable` says the comparator stands behind its verdict (enough evidence either way).
When someone needs the why, `paph.compare(a, b, { json: true })` returns the full report —
every channel, model, inlier count and threshold — as exactly the text the JavaScript engine
produces, so a platform can publish wires and reports and anyone can re-run them.

Verdicts are symmetric; *direction* is not something the comparator can know. On a ledger it
does not have to: the earlier timestamp is the candidate original. PAPH says the later work is
a copy; the chain says which came first. Neither alone is a legal finding; together they are
the evidence an artist needs.

## 5. What it finds

`npm run recall` (test/index-recall.mjs): eight originals, ten transforms, 200 distractors
drawn from the same three generators (the hardest negatives this corpus has). "Found" = the
original is in the top 32 of that family. Both arrival orders are measured, because an index
checks each pair once, when the second of the two arrives: either the copy queries for the
original, or the original queries for an already-indexed copy.

| transform | copy finds original: codes | bands | **either** | original finds copy: codes | bands | **either** | comparator says Copy |
|---|---:|---:|---:|---:|---:|---:|---:|
| mirrored | 8/8 | 8/8 | **8/8** | 8/8 | 8/8 | **8/8** | 8/8 |
| rotated 90° | 8/8 | 8/8 | **8/8** | 8/8 | 8/8 | **8/8** | 8/8 |
| cropped to 70% | 8/8 | 6/8 | **8/8** | 8/8 | 7/8 | **8/8** | 7/8 |
| cropped to 50%, corner | 7/8 | 5/8 | **8/8** | 7/8 | 5/8 | **8/8** | 4/8 |
| 2× nearest upscale | 8/8 | 8/8 | **8/8** | 8/8 | 8/8 | **8/8** | 8/8 |
| resampled to 75% | 1/8 | 5/8 | **5/8** | 0/8 | 6/8 | **6/8** | 5/8 |
| resampled to 150% | 1/8 | 4/8 | **4/8** | 0/8 | 4/8 | **4/8** | 2/8 |
| recoloured | 3/8 | 5/8 | **5/8** | 3/8 | 3/8 | **3/8** | 3/8 |
| inverted | 8/8 | 3/8 | **8/8** | 8/8 | 3/8 | **8/8** | 1/8 |
| pasted into a host | 7/8 | 7/8 | **8/8** | 7/8 | 5/8 | **7/8** | 7/8 |

Read it as: for the transforms the keys are built for, the index nominates the original every
time in both orders — except one paste of eight when the original is the query and the collage
is the indexed work; for resampling and recolouring it nominates at least as often as the
comparator would certify. The two families earn their places on different rows — codes
carry inversion and the corner crop, bands carry resampling and recolouring — and the 1/df
weighting is what makes both work against same-style distractors (with plain shared-key counts
the reverse-order resample row drops to 2/8). Non-integer resampling and recolouring are where
a pHash or embedding channel still pays.

On real Pixagram artworks (the `pixagram-search` fixtures), mirrored, cropped, 3× upscaled,
red/blue-swapped and pasted-into-another-artwork copies are all `Copy`, and the two fixtures
are `Unrelated` to each other.

## 6. Costs

Per stored work: one hash (5–60 ms CPU at Pixagram sizes), ~24.5 KB of wires (512 keypoints;
smaller works have fewer), ~700 postings and their df rows — about 50 KB in SQLite, measured.
Per query: the nomination statement (a few ms on a local SQLite) and a comparison per screened
candidate. One SQLite database of 10 GB (a Cloudflare Durable Object's limit) holds about
200,000 works; past that, shard the postings by key range and merge the nominations.

## 7. Operations

* **Backfill**: hash and index every work once; check each against what is already indexed. In
  arrival order every pair is examined exactly once.
* **Key derivation changes** (`KEYS_VERSION`): re-derive keys from stored wires — no image is
  fetched, nothing is re-hashed.
* **Comparator changes**: store the comparator (42) and the calibration identity
  (`calibrationId`, from any report) beside each verdict — the Cloudflare integration does — and
  re-verify stored pairs when you adopt a new one. The wires stay.
* **Wire changes** (`WIRE_VERSION`): rare by design; re-hash.
* **Deletion**: remove the work's postings using its stored key set (decrementing df), then the
  work.
* **Concurrency**: when two works are indexed at the same moment, each check sees what was
  indexed before it, so the pair is still examined once — by the later. When storing verdicts,
  replace only the pairs a check actually examined (the candidates it ranked), never "everything
  involving this work": the other check's verdict may be the only record of the pair. Re-check
  a work's previous partners whenever its image changes; a verdict about an image must not
  outlive it.

## 8. Other hosts

The engine is one WebAssembly module with a small C ABI ([WASM-ABI.md](WASM-ABI.md)) and no imports:
the glue (`wasm/paph.js`) covers browsers, Node, Deno and Workers; Python (`wasmtime`), Go
(`wazero`) and others drive the same module directly, and Rust services can link the crate.
Hashing where the pixels are and verifying where the wires are is the one design rule worth
keeping: shipping 24 KB wires to the comparator is cheaper than shipping the comparator's
inputs anywhere else.
