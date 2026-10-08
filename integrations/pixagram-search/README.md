# PAPH in pixagram-search

Copy detection for [pixagram-search](https://github.com/pixagram-blockchain/pixagram-search),
the Cloudflare Worker that indexes Pixagram artworks: one patch, one commit, on top of its
`main`. It is the reference implementation of [docs/SEARCH.md](../../docs/SEARCH.md).

```bash
cd pixagram-search
git checkout -b paph
git am path/to/0001-Copy-detection-with-PAPH-4.2-fingerprint-index-compa.patch
npm install
npm run typecheck && npm test        # 66 tests, 26 of them PAPH
npm run e2e:paph                     # the whole pipeline in workerd, offline
```

## What it adds

```
queue consumer:  decode ─► stats ─► paph stage ─► (upscale ─► embed ─► vector pass) ─► describe
                                      │ hash in WASM (5–60 ms)              │ the fresh embedding's
                                      ▼                                     ▼ neighbours, checked too
                     PaphIndex Durable Object (SQLite)
                       paph_works   wires (T1 + T2) and the keys each work is indexed under
                       paph_keys    (kind, key) → post   kind 1: local code, 2: descriptor band
                       paph_df      document frequency per key
                       index(): store, then check against everything indexed before it:
                         nominate — codes and bands (Σ 1/df, top 32 each) ∪ pHash neighbours
                                    ∪ stored-vector neighbours ∪ previous partners (≤ 256)
                         verify   — comparator 42, screen-gated, 64 candidates at a time
                                      │ verdicts ≥ Suspected + the pairs examined
                                      ▼
                     D1 paph_matches (a = earlier post by chain time, b = later; comparator, calibration)
                        + artworks.paph_hash, in the same transaction
```

| file | |
|---|---|
| `vendor/paph/` | `paph.wasm`, `paph.js`, `paph.d.ts` from this repository's `wasm/` (commit `3484423`), with hashes |
| `src/paph/engine.ts` | module registration (Workers import `.wasm` as a compiled module), the hashing budget, `fingerprint()`, verdict states |
| `src/paph/store.ts` | the index on SQLite — schema, put/remove/rekey, nominate (the SQL of SEARCH.md §3), find, report |
| `src/paph/index-do.ts` | `PaphIndex`, the Durable Object around the store |
| `src/paph/copies.ts` | the `paph` stage, pHash/vector channels, D1 verdicts, the API helpers |
| `src/paph/client.ts` | the stub and options from `env` |
| `migrations/0002_paph.sql` | D1 `paph_matches`, `artworks.paph_hash` |
| `src/api.ts` | `GET /copies/:id[?live=1]`, `POST /copies-by-image`, `GET /copies/:a/report/:b[?wires=1]`, `GET /paph/:id`, `/admin/paph`, `/admin/paph/alerts`, `/admin/paph/rekey` |
| `src/enrich/consumer.ts` | the stage right after `stats`, and a vector pass after `embed`; a failure is recorded, retried when transient, never fatal to the other stages |
| `src/db/posts.ts`, `src/search/*` | deleted posts leave the copy index; search items gain `artwork.stages.paph` |
| `wrangler.jsonc` | the `PAPH` binding, migration tag `v2` (`new_sqlite_classes: ["PaphIndex"]`), `PAPH_*` vars |
| `test/paph.test.ts` | real fixtures (mirrored, cropped, upscaled, recoloured, pasted → `Copy`), the hashing budget, the store on `node:sqlite` (query plans held to primary keys, batching, key-less checks), verdict writes on a D1 shim (concurrent checks, stale or deleted partners) |
| `test/e2e/` | `wrangler dev --local` + a mock chain: ingest six artworks, drain the queue, check the API, re-verify a published report with both engines, trip the rate limit, delete a copy on chain |

## Design points

* **The index is a Durable Object.** The comparator needs every candidate's wires, ~24 KB each.
  Through D1 they would cross the network on every query and arrive as JavaScript number arrays;
  in a SQLite-backed Durable Object they sit next to the engine — one fingerprint in, a few
  verdicts out — and copy detection's CPU stays off the D1 database that serves search. D1 keeps
  only verdicts, which list and filter with posts.
* **Memory, not just time.** The hasher needs ~60 bytes per pixel and WebAssembly memory never
  shrinks; an isolate has 128 MB for everything. Images over 768² (`PAPH_MAX_PIXELS`) are
  brought inside it first — an exact blow-up divided back to its pixels, anything else
  box-filtered by the smallest integer factor that fits. Pixagram artworks (≤ ~430 px) are
  hashed exactly as they are.
* **Verdicts are replaced pair by pair.** A check replaces the verdicts of the pairs it
  examined and nothing else, in the same transaction that marks the stage complete, and writes
  a verdict only while both images are the ones it was reached on and both posts are live —
  checks running at the same time can neither erase nor resurrect each other's findings, and a
  run that stopped halfway is redone. A changed image withdraws its old verdicts first.
* **Keep the public off the comparator.** Everything compares in one Durable Object, one
  request at a time. Stored `/copies` reads are D1-only; `live=1` needs the admin token;
  uploads and reports are cached (per image, per image pair); the two routes that compare on
  demand share a per-client budget (`PAPH_LIMITER`, a `ratelimits` binding, 30 a minute).

## Known gap: the screen gate

The verify step ranks with comparator 42's stage-1 screen as a gate (`gate` in the store's
find options, default true). The screen passes a pair only on `geo_min_corr` (8) keypoint
correspondences or more, and a pair has no more correspondences than its smaller side has
keypoints, so every copy of a work with fewer than 8 keypoints is dropped unverified, with some
others whose pools stay thin. On the PAPH-SI corpus that is 178 of the 976 pairs comparator 42
calls Copy, 164 of them for the keypoint count ([PAPH-X.md](../../docs/PAPH-X.md) §6, measured
in `@pixagram/paph-x` 1.1.1). How many Pixagram works have so few keypoints has not been
measured. The patch is unchanged. Passing `gate: false`
compares every nominated candidate. Verifying with XRank under the X2 profile keeps those copies
without comparing every candidate ([SEARCH.md](../../docs/SEARCH.md) §4), but the vendored module
is 4.2.3's and has no XRank: it needs `@pixagram/paph-x` 1.1.1 or later in `vendor/paph/`.

## Deploy

```bash
npm run db:migrate                                   # adds paph_matches and artworks.paph_hash
npm run deploy                                       # creates the PaphIndex class (migration v2)
scripts/admin.sh reindex-all '"paph"'                # fingerprint and check every artwork once
scripts/admin.sh paph                                # index size, verdicts, stage health
scripts/admin.sh paph-alerts 30                      # cross-author copies, earlier → later
```

From then on every new or edited artwork goes through the stage with the others.

## The end-to-end run

```
  PASS six artworks ingested
  queue drained in 1.5 s — index: {"works":6, "identity":{"comparator":42,"calibration":"CAL-004-PROPOSED",…}, …}
  /copies of the original:
    @recolorist Copy  later  structural 8914  via codes+bands+phash
    @copycat    Copy  later  structural 7232  via codes+bands  (mirrored)
    @collager   Copy  later  structural 3517  via codes+bands
  PASS the mirrored copy: Copy, published later, reflection
  PASS the collage containing it: Copy
  PASS the recoloured copy: Copy
  PASS the unrelated artworks are not listed
  PASS seen from the copy, the original is earlier and by someone else
  PASS the pasted-into artwork lists only the collage
  PASS every artwork's stages.paph is true
  PASS live=1 recomputes the same verdicts
  PASS live=1 without the admin token is refused
  PASS an uploaded crop finds the original
  PASS the same upload again is answered from the cache, identically
  PASS the report endpoint returns comparator 42's report
  PASS re-running the engine on the published wires gives the same report, byte for byte
  PASS and so does the JavaScript engine (paph-js), independently written
  PASS the on-demand routes are rate-limited per client
  PASS a copy deleted on chain leaves the verdicts
  PASS and the index
  PASS the index names its comparator and calibration
  PASS the alerts feed lists cross-author copies
```
