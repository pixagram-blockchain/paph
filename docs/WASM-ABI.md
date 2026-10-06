# The WebAssembly ABI (ABI 3)

`wasm/paph.wasm` exports a small C ABI — flat integers, length-prefixed byte blocks, opaque
handles — instead of wasm-bindgen glue, so the whole contract fits on this page and any host
can drive it: the browser and Node through `wasm/paph.js`, Cloudflare Workers by importing the
module, Python through `wasmtime`, Go through `wazero`, Rust hosts through `wasmtime` or by
linking the crate directly. The module imports nothing.

Source of truth: [`rust/src/abi.rs`](../rust/src/abi.rs) and, for PAPH-X,
[`rust/src/x/abi.rs`](../rust/src/x/abi.rs). Version checks: `paph_abi()` = 3, `paph_xabi()` = 1,
`paph_version()` = 3 (the wire), `paph_t1_bytes()` = 3952. ABI 3 is ABI 2 plus the PAPH-X
exports below; nothing of ABI 2 moved.

## Memory

| export | |
|---|---|
| `paph_alloc(len) -> ptr` | caller-owned scratch in linear memory |
| `paph_free(ptr, len)` | frees what `paph_alloc` returned |
| `paph_release(block)` | frees a *block* this module returned |

A **block** is `[u32 payload_len][u32 capacity][payload]`, little-endian. Read the payload,
then `paph_release` it. Linear memory can grow during any call: re-read the memory buffer after
every call rather than holding a view across one.

## Configuration

Hash-time and compare-time options travel as a flat `i32[21]` (`paph_config_fields()`); a null
pointer means the shipping defaults, which `paph_default_config(out)` writes out.

| # | field | default | # | field | default |
|---|---|---|---|---|---|
| 0 | foldMatte | 1 | 11 | evidence (0 lift, 1 proportion) | 0 |
| 1 | divideUpscale | 1 | 12 | confidenceAt | 16 |
| 2 | matteTol | 24 | 13 | scoring (0 gate, 1 weighted) | 1 |
| 3 | peakRadius | 5 | 14 | ragEndpoint (0 quantile, 1 rank) | 1 |
| 4 | foldInvert | 1 | 15 | geoEnabled | 1 |
| 5 | localWindows[0] | 8 | 16 | geoConfAt | 16 |
| 6 | localWindows[1] | 16 | 17 | geoEps | 1600 |
| 7 | localCount (4–128) | 128 | 18 | mirrorHypothesis | 1 |
| 8 | kpCount (0–512) | 512 | 19 | geoMinCorr (2–64) | 8 |
| 9 | sketchCount (0–32) | 32 | 20 | kpSelect (0 = 4.1 rule, 1 = 4.2) | 1 |
| 10 | hammingT | 8 | | | |

Fields 0–9 and 20 change the wire; the rest only change how wires are compared.

## Hashing

```
paph_hash_checked(cfg, px, w, h, limits) -> block
```

`px` is `w·h·4` bytes of RGBA. `limits` is null or `i32[3]` = (max width, max height, max
pixels), which can only lower the specification's own (16384, 16384, 2²⁴). The block's payload
is `[u32 t1_len][u32 t2_len][t1][t2]`; a refused image gives `t1_len = 0` and a UTF-8 message
starting `limit:` in place of the wires. `paph_hash` is the same without limits.

## Comparing

A wire pair is parsed once into a **prepared side**, compared against as many others as needed.

| export | |
|---|---|
| `paph_prepare(t1, t1n, t2, t2n, flags) -> handle \| 0` | 0 when Tier 1 is refused. `t2` may be null (Tier-1 sketch only). flags bit 0 **strict**: refuse a Tier 2 claiming more than 512 keypoints — use it for anything user-supplied |
| `paph_prepared_info(h, out_i32x4)` | keypoints, Tier 2 usable (0/1), width, height |
| `paph_prepare_free(h)` | |
| `paph_profile(pcal, n) -> handle \| 0` | a calibration artefact (`.pcal` bytes), decoded; **0 when it does not decode**. Check it: wherever a profile is taken, 0 means the shipped CAL-004-PROPOSED, so passing a failed decode on would silently compare under the default |
| `paph_profile_free(h)` | |
| `paph_compare42(cfg, prof, a, b, flags) -> block` | comparator 42's report as UTF-8 JSON — exactly `JSON.stringify` of what the JavaScript engine returns. flags bit 0 **lean**: same verdict, `v3: null`, ~20 % less work |
| `paph_screen42(cfg, prof, a, b) -> i32` | the stage-1 screen: `pass \| poolDirect << 1 \| poolMirror << 12` (pools saturate at 2047) |
| `paph_rank42(cfg, prof, query, cands, n, flags, out) -> n` | one query against `n` candidates in one call (`cands`: `n` u32 handles). flags bit 0 **gate**: skip the comparison for pairs the screen rejects. Writes `n` records of `i32[16]`, below |

Rank record (`paph_rank_fields()` = 16):

| # | field | # | field |
|---|---|---|---|
| 0 | state: −1 unscreened, 0 Unrelated, 1 Related, 2 Suspected, 3 Copy, 4 Identical, 5 Indeterminate | 8 | local matches |
| 1 | certifiable (0/1) | 9 | diversity (combined) |
| 2 | structural (0–10000) | 10 | diversity multiplier |
| 3 | geometry evidence (0–10000) | 11 | models |
| 4 | total inliers | 12 | screen pool, direct |
| 5 | topology | 13 | screen pool, mirrored |
| 6 | geometric margin | 14 | swapped (canonical order) |
| 7 | local evidence (−1 when the channel abstains) | 15 | flags: 1 screen pass, 2 mixed 4.1/4.2 selection, 4 a mirrored model |

Every record equals the fields of the full report for the same pair (`test/wasm-parity.mjs`).

## Index keys

| export | |
|---|---|
| `paph_local_codes(h, out_u32, cap) -> n` | the Tier-1 local codes as (hi, lo) pairs, at most 128 — canonical under the square's eight symmetries and the complement |
| `paph_descriptors(h, which, out_u32, cap) -> count` | keypoint descriptors, 8 u32 each, the first `min(count, cap)` written (`cap` 0 asks the count). `which` 0: the compared keypoints in wire order; 1: the 32-keypoint Tier-1 sketch; 2: the compared keypoints strongest first (strength descending, then x, then y) |

`Engine.indexKeys` in `wasm/paph.js` turns these into the exact-match keys of
[SEARCH.md](SEARCH.md) — that derivation is twenty lines of JavaScript, easy to port to any
host, and versioned as `KEYS_VERSION`.

## PAPH-X (X ABI 1)

The retrieval-native cascade of [PAPH-X.md](PAPH-X.md): a side is derived once (`xprepare`:
route, bucket index, anchor order), then screened, compared or ranked. Verdicts keep the
comparator-42 vocabulary; every report also carries an **execution state** (`FAST`, `DEFERRED`,
`FALLBACK`, `AUDIT`) saying how the verdict was reached.

| export | |
|---|---|
| `paph_xabi() -> 1` | |
| `paph_xprofile(prof, pxcl, n) -> handle \| 0` | `prof`: a comparator-42 profile handle (0 = the shipped CAL-004-PROPOSED); `pxcl`: an X profile artefact (`.pxcl` bytes), or null for the shipped X1-PROVISIONAL bound to that profile. 0 when the artefact does not decode — check it |
| `paph_xprofile_free(h)` | |
| `paph_xprofile_bytes(h) -> block` | the X artefact's bytes (store them: the identity covers every parameter) |
| `paph_xprofile_id(h, out_32)` | SHA-256 identity |
| `paph_xprofile_status(h) -> i32` | 0 ok, 1 unsupported, 2 base mismatch (the X profile was derived for another base profile: every comparison under it is Indeterminate) |
| `paph_xprepare(xb, t1, t1n, t2, t2n, flags) -> handle \| 0` | as `paph_prepare`, plus the route, the bucket index and the anchor order (≈ 0.2–1.5 ms; cache the sidecar to skip it). flags bit 0 **strict** |
| `paph_xprepare_sidecar(xb, t1, t1n, t2, t2n, side, siden, flags) -> handle \| 0` | the same, the structures taken from a PAX1 sidecar when it matches the wires and the profile (a stale or damaged sidecar is ignored and the structures rebuilt) |
| `paph_xprepare_free(h)` | |
| `paph_xprepared_info(h, out_i32x4)` | as `paph_prepared_info` |
| `paph_xroute(h, out_u8x136)` | the route record: 4 global invariant words (u64 LE), 64 local MinHash lanes, 32 band MinHash lanes (128 route bytes), then local-code count, descriptor count, geometry class and measurability flags (u16 LE each) |
| `paph_xsidecar(h) -> block` | the PAX1 sidecar (CRC-32 protected) |
| `paph_xscreen(cfg, xb, a, b, out_i32x12) -> state` | the pair screen, never a verdict: −1 refused (profiles), 0 Reject, 1 Defer, 2 Pass, 3 Identical. Fields: 0 state · 1 route local · 2 route band · 3 route global · 4 route class (0 reject, 1 defer, 2 fast, 3 absent) · 5 pool direct · 6 pool mirror · 7 support direct · 8 support mirror · 9 rows scanned · 10 Hamming pairs · 11 swapped |
| `paph_xcompare(cfg, xb, a, b, flags) -> block` | the X report as UTF-8 JSON (`comparator: 50`). flags bits 0–1: policy (0 the profile's, 1 fast, 2 safe, 3 exact); bit 2 **audit** (EXACT42 beside the fast path, attached as `fallback`); bit 3 **copy scope** (a pair the lattice cannot lift above Related is `NotCopy`, not resolved further) |
| `paph_xrank(cfg, xb, query, cands, n, flags, out) -> n` | one query against `n` candidates (`cands`: `n` u32 handles, 0 for a missing one): the route table screened with SIMD lanes, the sparse screen on what the route did not reject, the cascade on the survivors. flags bits 0–1: policy; bit 2: **no gate** (compare every candidate); bit 3: **full scope** (default copy scope). Writes `n` records of `i32[24]`, below |
| `paph_xrank_fields() -> 24`, `paph_xscreen_fields() -> 12` | |

X rank record (`paph_xrank_fields()` = 24):

| # | field | # | field |
|---|---|---|---|
| 0 | state: −1 rejected/unscreened, 0 Unrelated, 1 Related, 2 Suspected, 3 Copy, 4 Identical, 5 Indeterminate, 6 NotCopy | 12 | geometry evidence (0–10000) |
| 1 | execution: 0 FAST, 1 DEFERRED, 2 FALLBACK, 3 AUDIT | 13 | geometric margin |
| 2 | screen state (as `paph_xscreen`) | 14 | topology |
| 3 | route local (0–64) | 15 | structural lower bound |
| 4 | route band (0–32) | 16 | structural upper bound |
| 5 | route global (0–255) | 17 | structural exact (0/1) |
| 6 | route class | 18 | certifiable (0/1) |
| 7 | pool direct | 19 | local evidence (−1 when abstained or not needed) |
| 8 | pool mirror | 20 | Hamming pairs evaluated |
| 9 | rows scanned | 21 | full pairs the exhaustive scan would have evaluated |
| 10 | inliers | 22 | swapped (canonical order) |
| 11 | models | 23 | flags: 1 certificate, 2 a mirrored model, 4 sparse explosion, 8 EXACT42 ran, 16 mixed 4.1/4.2 selection |

Every record equals the fields of the pairwise copy-scope report for the same pair
(`test/x-wasm.mjs`), and the records are byte-identical between the native engine and both
WebAssembly builds.

## From another host

Python, with `wasmtime`:

```python
from wasmtime import Engine, Store, Module, Instance
import struct

store = Store(Engine())
x = Instance(store, Module.from_file(store.engine, "paph-baseline.wasm"), []).exports(store)
mem = x["memory"]

def put(b):
    p = x["paph_alloc"](store, len(b)); mem.write(store, b, p); return p

def take(block):
    n = struct.unpack("<I", mem.read(store, block, block + 4))[0]
    out = bytes(mem.read(store, block + 8, block + 8 + n)); x["paph_release"](store, block); return out

def hash_rgba(px, w, h):
    p = put(px); blk = take(x["paph_hash_checked"](store, 0, p, w, h, 0)); x["paph_free"](store, p, len(px))
    n1, n2 = struct.unpack("<II", blk[:8])
    if n1 == 0: raise ValueError(blk[8:8 + n2].decode())
    return blk[8:8 + n1], blk[8 + n1:8 + n1 + n2]

def prepare(t1, t2):
    a, b = put(t1), put(t2)
    h = x["paph_prepare"](store, a, len(t1), b, len(t2), 1)
    x["paph_free"](store, a, len(t1)); x["paph_free"](store, b, len(t2)); return h

def compare(ha, hb):          # comparator 42, the JSON report
    return take(x["paph_compare42"](store, 0, 0, ha, hb, 0)).decode()
```

Use `paph-baseline.wasm` where the runtime has no SIMD128; both builds compute the same bytes
(`npm run test:equiv`).
