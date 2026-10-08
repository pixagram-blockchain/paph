# PAPH-X — `@pixagram/paph-x`

**An integer-only perceptual fingerprint and a calibrated comparator for detecting copied pixel
art** — mirrored, cropped, rescaled, recoloured, pasted into something else — with the evidence
for every verdict, and **PAPH-X**, the retrieval-native cascade that reaches the same verdicts
through a cheaper path. This repository carries PAPH 4.2 whole — the evidence bench, the
JavaScript engine, the Rust reference rewritten for speed, the WebAssembly modules built from
it, the pieces for wiring it into a search engine — and PAPH-X beside it, sharing the wire, the
comparator-42 verdicts and the calibration.

Three builds of the engine — JavaScript, native Rust, Rust compiled to WebAssembly — produce the
**same wires and the same reports, byte for byte**. That is the design, not a nicety: an index, a
moderator's appeal and a dispute between two artists all rest on anyone being able to recompute
the same answer from the same bytes.

![The evidence bench](docs/img/bench.png)

## The evidence bench

[`demo/paph4x.html`](demo/paph4x.html) — one self-contained file, no server, no network, fonts
included. Open it, drop two images (or derive B from A with a transform), and it walks through
everything comparator 42 measured: the screen, the verdict lattice, the geometric models, every
channel with its calibration table, an attack sweep, the wire itself, and the cost. It runs on
the WebAssembly engine, keeps the JavaScript engine beside it, and checks on every pair that the
two produce the same report.

![Both engines, one answer](docs/img/bench-engines.png)

The page is generated (`npm run build:bench`) from `demo/bench4x/` and the current engines, so it
cannot drift from the package.

## Use it

```js
import { init } from '@pixagram/paph-x/wasm';

const paph = await init();                         // SIMD128 build where available, baseline otherwise
const a = paph.hash(imageDataA);                   // { t1: 3952 B, t2: 32 + 40·kp B, … }
const b = paph.hash(imageDataB);
const r = paph.compare(a, b);                      // the comparator-42 report
r.verdict;                                         // 'Copy'
r.class;                                           // 'certified — structure and geometry agree'
```

Many comparisons against one query — parse once, rank in one call:

```js
const q = paph.prepare(a.t1, a.t2, { strict: true });
const sides = rows.map(r => paph.prepare(r.t1, r.t2));
paph.rank(q, sides, { gate: true });               // [{ verdict, certifiable, structural, mirrored, … }]
```

The JavaScript engine has the same answers with no WebAssembly at all:

```js
import { hash, compare, cal } from '@pixagram/paph-x';
const profile = cal();
const x = hash(imageDataA, {}, profile.limits), y = hash(imageDataB, {}, profile.limits);
compare(x.t1, x.t2, y.t1, y.t2, {}, profile).verdict;   // 'Copy'
```

Cloudflare Workers import the module compiled: `import wasm from '@pixagram/paph-x/wasm/paph.wasm'`
then `await init(wasm)`. Other hosts (Python, Go, Rust services) drive the same module through
its C ABI — [docs/WASM-ABI.md](docs/WASM-ABI.md).

## PAPH-X: the same verdicts through a cheaper path

PAPH-X ([docs/SPEC-X-paph-x.md](docs/SPEC-X-paph-x.md), built as
[docs/PAPH-X.md](docs/PAPH-X.md)) keeps the wire, the comparator-42 verdicts and the
calibration, and reaches them through a six-stage cascade: a 128-byte route signature screened
first, descriptor matching by locality-sensitive buckets instead of the 512 × 512 Hamming
scan, geometry on 96 anchor keypoints that expands only when it must, structural channels
computed only while the verdict can still move, and comparator 42 itself as the fallback and
the audit. Every report says how its verdict was reached (`FAST`, `DEFERRED`, `FALLBACK`,
`AUDIT`).

```js
const a = paph.xprepare(fpA), b = paph.xprepare(fpB);   // route + bucket index + anchor order, once per side
paph.xscreen(a, b).state;                                // 'Reject' | 'Defer' | 'Pass' | 'Identical' — never a verdict
const r = paph.xcompare(a, b);                           // safe: 42's Copy answer, or certified; r.execution, r.reason
paph.xcompare(a, b, { policy: 'fast' });                 // never falls back; 'DEFERRED' where the cascade will not decide
paph.xrank(q, sides);                                    // one call: route table (SIMD) → sparse screen → cascade
```

Measured on the specification's corpus (2,379 pairs; `rust/target/release/xbench`,
`npm run bench:x`): zero copy disagreements with comparator 42 under the safe policy, zero
false Copies under fast, zero copies hard-rejected by the screen, 1.4 % fallbacks, 99 % fewer
descriptor pairs evaluated, zero allocations in the screen. In 1.1.1's run the reference search
workload (a 512-keypoint query against 100 busy candidates) runs **3.2×** faster than comparator
42's ranking, natively and in WebAssembly, and the fast-path compare 10.6× faster at p50; the
pairwise screen is 4.7× faster at p95 and 0.9× at p50, where X2's structural door is paid.
(1.0.0's run, under X1 and with a slower comparator 42, measured 7.1×, 26× and 15×.) On the
PAPH-SI corpus's 976 comparator-42 copies, XRank shown the target alone reads Copy on every one,
in both arrival orders. The specification's ≥ 10× release gates are **not** met and not
claimed; the gate table, the per-class numbers and the known gaps are in
[docs/PAPH-X.md](docs/PAPH-X.md). The shipped profile, X2, is provisional: calibrated on a
synthetic corpus, not yet on a moderation corpus. 1.1.1 made it the default, keeping 1.0.0's
X1 beside it, to address three screen problems — see [CHANGELOG.md](CHANGELOG.md).

## PAPH-SI: which stored works are worth comparing

PAPH-SI ([docs/SPEC-SI-paph-si.md](docs/SPEC-SI-paph-si.md)) is the screening index in front of
XRank: six transformation-stable feature families read from the wire — stroke texture,
luminance topology, palette population, quantile-band regions, silhouette, keypoint layout —
each quantised into a 16-coarse / 256-fine cell hierarchy, plus XRoute's MinHash lanes banded
into keys. A 104-byte signature and ~45 postings per work; a candidate is scored by the summed
evidence of the families it agrees on, never required to agree on all of them.

```js
const index = paph.siindex();                 // in memory; or SQLite / D1 with SI_SQL.schema
const sig = paph.sisig(fp);                   // ingest, from the wires { t1, t2 }: 104 bytes + posting keys
const slot = index.add(sig.bytes);            // in SQL: one si_works row, sig.keys as si_postings rows
const q = paph.siquery(upload);               // an XSide (xprepare) or { t1, t2 }
index.query(q).hits;                          // [{ slot, score }], best first
db.prepare(SI_SQL.query).all(...siSqlParams(q.plan()));   // the same answer from SQLite / D1
```

Measured on the synthetic corpus (comparator-42 copies, `npm run bench:si`): requiring shape,
palette and structure to agree keeps 74 % of copies at 698×; PAPH-SI's score keeps 92 % at 70×
and 90.5 % at 98×; **in union with the exact keys, 99.8 % at 4,000 works** — XRank returns Copy
on all of them end to end — **and 98.8 % at 104,000** (97.0 % with SI's pool cut to its
default 2,000). The shipped profile, SI2 (1.1.0's SI1 fit, bound to X2), is provisional: fitted
on synthetic art only.

## Faster, with the same bytes

| | before | after |
|---|---:|---:|
| hash a 288×200 scene, native | 49.0 ms | **12.4 ms** |
| hash a 1024×768 scene, native | 1112 ms | **161 ms** |
| comparator 42, scene × mirrored, native | 14.5 ms | **3.46 ms** |
| hash a 288×200 scene, WebAssembly (vs `paph-js` 4.2.2's module) | 77.3 ms | **16.3 ms** |
| comparator 42, scene × mirrored, WebAssembly (vs the JavaScript engine — 4.2.2's module had no comparator 42) | 68.9 ms | **5.55 ms** |
| descriptor scan per pair, WebAssembly | 7.8 ns | **2.6 ns** |

Every one of these changes is output-identical: a 3,160-case equivalence digest — every
transform, sixteen hash configurations, every comparator, corrupt wires, the JSON reports — is
byte-identical before and after, natively and inside both WebAssembly builds, and 625 ordered
pairs give byte-identical report text in JavaScript and WebAssembly.
[docs/PERFORMANCE.md](docs/PERFORMANCE.md) has the full tables and what changed.

## Wiring it into a search engine

PAPH compares pairs; a search engine needs to find, among N works, the few worth comparing. The
answer is retrieve-then-verify, with PAPH supplying both halves:

1. **Fingerprint at ingest** — `hash()`, store both tiers (≤ 24.5 KB).
2. **Index exact-match keys** — `indexKeys()` derives Tier-1 local codes (canonical under the
   square's symmetries and inversion) and 24-bit descriptor bands; integers in any store.
3. **Nominate** — score works sharing keys with a query by Σ 1/df, keep the top 32 per family,
   union with PAPH-SI's candidates (`siquery`, SQL or in memory) and whatever else you have
   (pHash, embeddings).
4. **Verify** — `rank(query, candidates, { gate: true })`; the comparator decides, and its report
   is the evidence. The gate passes a pair only on 8 keypoint correspondences or more, so it
   drops every copy of a work with fewer keypoints and some others — 178 of 976 comparator-42
   copies on the PAPH-SI corpus: `gate: false`, or `xrank` under X2, keeps them.

Measured on the test corpus (eight originals, 200 same-style distractors), the keys nominate the
original of every mirrored, rotated, cropped, upscaled and inverted copy from either side of the
pair, and of 15 in 16 pasted ones.
[docs/SEARCH.md](docs/SEARCH.md) is the design, the SQL and the numbers;
[integrations/pixagram-search](integrations/pixagram-search/README.md) is a complete
implementation for the Pixagram search Worker on Cloudflare — queue stage, a Durable Object
index that runs the comparator next to the fingerprints, verdicts in D1, `/copies` API, tested
end to end in workerd.

## Layout

```
demo/paph4x.html          the evidence bench (generated) · demo/bench4x/ its sources
src/                      the JavaScript engine: wire.cjs (fingerprints), paph-js.cjs (comparators)
rust/                     the Rust reference (crate `paph`, no dependencies)
  src/abi.rs              the C ABI the WebAssembly build exports
  src/equiv.rs            the equivalence digest      check.sh  bench.sh
  src/x/                  PAPH-X: route, bucket, anchor, matcher, geom, structural, compare, rank,
                          prepared, sidecar, profile, abi
  src/x/si/               PAPH-SI: features, profile, code (cells, probes, score), index, fit, abi
  src/bin/                xbench (the §33–35 harness) · sibench (PAPH-SI) · xprof · xcli · prof · paphcli · paph-equiv
wasm/                     paph.wasm (SIMD128), paph-baseline.wasm, paph.js (glue), paph.d.ts
index.js · index.cjs      the package entry (JavaScript engine + wasm())
docs/                     SPEC-003, SPEC-004, SPEC-004.1, SPEC-004.2, SPEC-X, SPEC-SI · PAPH-X · PERFORMANCE ·
                          SEARCH · WASM-ABI · calibration/ (.pcal, .psi artefacts) · golden/ (conformance vectors)
integrations/             pixagram-search: the Cloudflare integration as a patch
test/                     suites, parity, benchmarks, recall · x-wasm.mjs, x-bench.mjs (PAPH-X) · si-wasm.mjs (PAPH-SI)
tools/                    build-wasm.sh · build-paph4x.mjs · gen-corpus.mjs
```

## Build and verify

```bash
npm install                 # jsdom (bench test), binaryen (wasm-opt)
npm test                    # wire + comparator suites (JavaScript engine, golden vectors)
npm run test:rust           # cargo build + the 3,160-case equivalence digest
npm run test:native         # native reference vs JavaScript, field for field
npm run build:wasm          # both WebAssembly builds (rustup target add wasm32-unknown-unknown)
npm run test:equiv          # the digest computed inside both WebAssembly builds
npm run test:wasm           # 625 ordered pairs, JavaScript vs WebAssembly, byte for byte
npm run build:bench         # regenerate demo/paph4x.html
npm run test:bench          # the bench, headless, on both engines
npm run bench               # engine timings          npm run recall   index-key recall
npm run test:x              # PAPH-X: native xcli vs both WebAssembly builds, symmetry, exactness, rank, sidecar
npm run bench:x             # PAPH-X timings in WebAssembly;  rust/target/release/xbench  the native harness
npm run test:si             # PAPH-SI: native vs both WebAssembly builds, index = definition, SQL = index
npm run bench:si            # PAPH-SI: rust/sibench.sh — stability, recall vs reduction, funnel (--big: scaling)
```

`test:equiv` needs the digest-exporting builds: `tools/build-wasm.sh --equiv`.

## Versions

| layer | version | changes when |
|---|---|---|
| wire | **3** — Tier 1 3952 B, Tier 2 32 + 40n B (n ≤ 512), eleven sections, CRC-32 | only with a new wire specification |
| comparator | **42** (SPEC-004.2); 41 frozen beside it; PAPH-X reports as **50** with 42's vocabulary | when the evidence says the judgement should |
| calibration | **CAL-004-PROPOSED**, identified by its SHA-256; PAPH-X profile **X2-PROVISIONAL** bound to it (1.0.0's X1-PROVISIONAL kept beside it) | whenever a corpus is re-derived |
| index keys | **KEYS_VERSION 1** | re-derive keys from stored wires; nothing is re-hashed |
| screening index | **SI ABI 1**, feature derivation 1, profile **SI2-PROVISIONAL** bound to X2 (SI1-PROVISIONAL, the same fit, bound to X1) | re-derive signatures from stored wires; nothing is re-hashed |
| WebAssembly ABI | **3** (ABI 2 unchanged, plus the PAPH-X exports, X ABI 1, and the PAPH-SI exports, SI ABI 1) | |

`@pixagram/paph` numbered its releases by the comparator (4.2.3: comparator 42 with engines that
are faster and agree in four more places). `@pixagram/paph-x` starts over at **1.0.0**: that
package, renamed, with PAPH-X beside it and nothing of 4.2.3 changed — comparator 42,
CAL-004-PROPOSED, ABI 3 / X ABI 1, profile X1-PROVISIONAL; 1.1.0 adds PAPH-SI beside them; 1.1.1
ships profile X2 and addresses three screen problems without moving the wire, comparator 42 or the
calibration (see [CHANGELOG.md](CHANGELOG.md)).

## License

MIT — Pixagram SA.
