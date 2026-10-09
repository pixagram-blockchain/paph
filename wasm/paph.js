/**
 * PAPH 4.2 + PAPH-X — the WebAssembly engine.
 *
 * Hand-written glue over a small C ABI (rust/src/abi.rs, rust/src/x/abi.rs,
 * rust/src/x/si/abi.rs),
 * deliberately not wasm-bindgen: what crosses the boundary is flat integers,
 * length-prefixed byte blocks and opaque handles, and the whole contract can
 * be read here.
 *
 *     import { init } from '@pixagram/paph-x/wasm';
 *     const paph = await init();                 // finds paph.wasm next to this file
 *
 *     const a = paph.hash(imageData);            // { t1, t2 } — byte-identical to the JS engine
 *     const b = paph.hash(otherImageData);
 *     paph.compare(a, b).verdict;                // the same report the JS engine returns
 *
 * For an index, parse each side once and rank candidates in one call:
 *
 *     const q = paph.prepare(a.t1, a.t2);        // a handle; free() it, or let GC do it
 *     const sides = rows.map(r => paph.prepare(r.t1, r.t2, { strict: true }));
 *     const hits = paph.rank(q, sides);          // screen, then compare survivors
 *
 * Among many stored works, find the few worth ranking — the PAPH-SI screening
 * index (docs/SPEC-SI-paph-si.md), beside the exact keys of indexKeys():
 *
 *     const idx = paph.siindex();
 *     for (const w of works) w.slot = idx.add(paph.sisig(w.side).bytes);
 *     const q = paph.siquery(querySide);
 *     idx.query(q).hits;                         // [{ slot, score }], best first
 *
 * Cloudflare Workers / bundlers that import .wasm as a module:
 *
 *     import wasm from '@pixagram/paph-x/wasm/paph.wasm';
 *     const paph = await init(wasm);
 */

const ABI = 4;
const X_ABI = 1;
const SI_ABI = 1;
const SI_SIG_BYTES = 104;
const SI_MAX_KEYS = 54;
/* PAPH-SI family names, in signature order (cells, then the two MinHash families) */
const SI_FAMILIES = ['runs', 'tone', 'pal', 'shape', 'sil', 'kpgeo', 'local', 'band'];
/* the wire format hash() writes by default; { wire: 3 } writes 1.0–1.1's */
const WIRE_VERSION = 4;

/* Field order MUST match config_from in rust/src/abi.rs. */
const DEFAULTS = Object.freeze({
  foldMatte: true, divideUpscale: true, matteTol: 24, peakRadius: 5, foldInvert: true,
  localWindows: [8, 16], localCount: 128, kpCount: 512, kpSelect: 1, sketchCount: 32,
  hammingT: 8, evidence: 'lift', confidenceAt: 16, scoring: 'weighted', ragEndpoint: 'rank',
  geoEnabled: true, geoConfAt: 16, geoEps: 1600, geoMinCorr: 8, mirrorHypothesis: true,
  wire: 4
});
const STATES = ['Unrelated', 'Related', 'Suspected', 'Copy', 'Identical', 'Indeterminate', 'NotCopy'];
const EXECUTIONS = ['FAST', 'DEFERRED', 'FALLBACK', 'AUDIT'];
const SCREENS = ['Reject', 'Defer', 'Pass', 'Identical'];
const ROUTE_CLASSES = ['REJECT', 'DEFER', 'FAST', 'ABSENT'];
const XRANK_FIELDS = 24;
const XSCREEN_FIELDS = 12;
const ROUTE_RECORD_BYTES = 136;
/* the index-key derivation in Engine.indexKeys; bump it when that changes */
const KEYS_VERSION = 1;
const RANK_FIELDS = 16;

/* A 1-function module using one SIMD instruction: does this runtime have SIMD128? */
const SIMD_PROBE = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3,
  2, 1, 0, 10, 10, 1, 8, 0, 65, 0, 253, 15, 253, 98, 11]);

export function simdSupported() {
  try { return WebAssembly.validate(SIMD_PROBE); } catch (e) { return false; }
}

const isNode = typeof process !== 'undefined' && !!(process.versions && process.versions.node);

async function readLocal(url) {
  if (isNode && url.protocol === 'file:') {
    const { readFile } = await import('node:fs/promises');
    return readFile(url);
  }
  const r = await fetch(url);
  if (!r.ok) throw new Error('paph: could not fetch ' + url + ' (' + r.status + ')');
  return r.arrayBuffer();
}

async function instantiate(source) {
  if (source === undefined || source === null) {
    const name = simdSupported() ? './paph.wasm' : './paph-baseline.wasm';
    source = new URL(name, import.meta.url);
  }
  if (source instanceof WebAssembly.Instance) return source;
  if (source instanceof WebAssembly.Module) return WebAssembly.instantiate(source, {});
  if (typeof source === 'string') source = new URL(source, import.meta.url);
  if (source instanceof URL) {
    if (!(isNode && source.protocol === 'file:') && WebAssembly.instantiateStreaming) {
      try { return (await WebAssembly.instantiateStreaming(fetch(source), {})).instance; }
      catch (e) { /* wrong MIME type and similar: fall through to bytes */ }
    }
    source = await readLocal(source);
  }
  if (typeof Response !== 'undefined' && source instanceof Response) {
    return (await WebAssembly.instantiate(await source.arrayBuffer(), {})).instance;
  }
  if (ArrayBuffer.isView(source)) {
    source = source.buffer.slice(source.byteOffset, source.byteOffset + source.byteLength);
  }
  return (await WebAssembly.instantiate(source, {})).instance;
}

/**
 * Load the module and return an Engine.  `source` may be omitted (paph.wasm or
 * paph-baseline.wasm next to this file, by SIMD support), or a URL / path, the
 * bytes, a Response, a WebAssembly.Module, or a WebAssembly.Instance.
 */
export async function init(source) {
  const instance = await instantiate(source);
  return new Engine(instance.exports);
}

function readImage(a, b, c) {
  if (a && a.data && a.width && a.height) return { px: a.data, w: a.width | 0, h: a.height | 0 };
  if (a && a.px && a.w && a.h) return { px: a.px, w: a.w | 0, h: a.h | 0 };
  if (a && a.pixels && a.width && a.height) return { px: a.pixels, w: a.width | 0, h: a.height | 0 };
  if (a && typeof b === 'number' && typeof c === 'number') return { px: a, w: b | 0, h: c | 0 };
  throw new TypeError('paph: expected ImageData, {px,w,h}, {pixels,width,height} or (bytes,w,h)');
}

function bytesOf(x) {
  if (x === undefined || x === null) return null;
  if (x instanceof Uint8Array) return x;
  if (ArrayBuffer.isView(x)) return new Uint8Array(x.buffer, x.byteOffset, x.byteLength);
  if (x instanceof ArrayBuffer) return new Uint8Array(x);
  if (Array.isArray(x)) return Uint8Array.from(x);
  throw new TypeError('paph: expected bytes');
}

const registry = typeof FinalizationRegistry !== 'undefined'
  ? new FinalizationRegistry(function (f) { try { f(); } catch (e) { /* engine gone */ } })
  : null;

/**
 * One prepared side: a wire pair parsed once inside the module.  Compare it
 * against as many others as you like; `free()` it when done (a finalizer frees
 * it otherwise, eventually).
 */
export class Side {
  constructor(engine, handle, info) {
    this.engine = engine;
    this.handle = handle;
    this.kp = info[0];
    this.tier2Ok = info[1] === 1;
    this.width = info[2];
    this.height = info[3];
    const x = engine.x;
    this._unreg = {};
    if (registry) registry.register(this, function () { x.paph_prepare_free(handle); }, this._unreg);
  }
  free() {
    if (!this.handle) return;
    if (registry) registry.unregister(this._unreg);
    this.engine.x.paph_prepare_free(this.handle);
    this.handle = 0;
  }
}

/** A decoded calibration artefact (.pcal).  Comparisons default to the shipped one. */
export class Profile {
  constructor(engine, handle, bytes) {
    this.engine = engine;
    this.handle = handle;
    this.bytes = bytes;
    const x = engine.x;
    this._unreg = {};
    if (registry) registry.register(this, function () { x.paph_profile_free(handle); }, this._unreg);
  }
  free() {
    if (!this.handle) return;
    if (registry) registry.unregister(this._unreg);
    this.engine.x.paph_profile_free(this.handle);
    this.handle = 0;
  }
}

/**
 * A comparator-42 profile and the PAPH-X profile bound to it, decoded once
 * (route salts, projection table, identities).  Every X call takes one;
 * `engine.xprofile()` is the shipped pair.
 */
export class XProfile {
  constructor(engine, handle, base) {
    this.engine = engine;
    this.handle = handle;
    this.base = base;
    const x = engine.x;
    this._unreg = {};
    if (registry) registry.register(this, function () { x.paph_xprofile_free(handle); }, this._unreg);
  }
  /** the X artefact's bytes (store them: the identity covers every parameter) */
  bytes() {
    return this.engine.take(this.engine.x.paph_xprofile_bytes(this.handle));
  }
  /** SHA-256 identity, hex */
  id() {
    const p = this.engine.x.paph_alloc(32);
    try {
      this.engine.x.paph_xprofile_id(this.handle, p);
      return Array.from(this.engine.u8().subarray(p, p + 32), b => b.toString(16).padStart(2, '0')).join('');
    } finally { this.engine.x.paph_free(p, 32); }
  }
  /** 'ok', 'unsupported' or 'mismatch' (the X profile is bound to another base profile) */
  status() {
    return ['ok', 'unsupported', 'mismatch'][this.engine.x.paph_xprofile_status(this.handle)] || 'unsupported';
  }
  free() {
    if (!this.handle) return;
    if (registry) registry.unregister(this._unreg);
    this.engine.x.paph_xprofile_free(this.handle);
    this.handle = 0;
  }
}

/** A side prepared for PAPH-X: the wires parsed plus the route, the bucket index and the anchor order. */
export class XSide {
  constructor(engine, handle, info) {
    this.engine = engine;
    this.handle = handle;
    this.kp = info[0];
    this.tier2Ok = info[1] === 1;
    this.width = info[2];
    this.height = info[3];
    const x = engine.x;
    this._unreg = {};
    if (registry) registry.register(this, function () { x.paph_xprepare_free(handle); }, this._unreg);
  }
  /** the 136-byte route record (128 route bytes + metadata) */
  route() {
    const p = this.engine.x.paph_alloc(ROUTE_RECORD_BYTES);
    try {
      this.engine.x.paph_xroute(this.handle, p);
      return this.engine.u8().slice(p, p + ROUTE_RECORD_BYTES);
    } finally { this.engine.x.paph_free(p, ROUTE_RECORD_BYTES); }
  }
  /** the PAX1 sidecar (version 2: bound to this side's Tier 1 and to the X profile): cache it
      beside the wires and hand it to xprepare to skip the derivation */
  sidecar() {
    return this.engine.take(this.engine.x.paph_xsidecar(this.handle));
  }
  free() {
    if (!this.handle) return;
    if (registry) registry.unregister(this._unreg);
    this.engine.x.paph_xprepare_free(this.handle);
    this.handle = 0;
  }
}

/**
 * A PAPH-SI profile (.psi): six codebooks of 16 / 256 cells, the evidence
 * weights, the default threshold and budget, bound to one X profile.
 * `engine.siprofile()` is the shipped SI4-PROVISIONAL, bound to the shipped
 * X3-PROVISIONAL and fitted on the Pixa chain's artworks hashed in wire 4;
 * 1.1.2's SI3-PROVISIONAL (bound to X2), SI2-PROVISIONAL (the synthetic fit,
 * bound to X2) and SI1-PROVISIONAL (1.1.0's, bound to X1) are
 * `docs/calibration/SI3-PROVISIONAL.psi`, `SI2-…` and `SI1-…`.
 */
export class SIProfile {
  constructor(engine, handle) {
    this.engine = engine;
    this.handle = handle;
    const x = engine.x;
    this._unreg = {};
    if (registry) registry.register(this, function () { x.paph_siprofile_free(handle); }, this._unreg);
  }
  /** the artefact's bytes (store them: the identity covers every parameter) */
  bytes() { return this.engine.take(this.engine.x.paph_siprofile_bytes(this.handle)); }
  /** SHA-256 identity, hex */
  id() { return this.engine._hex32(p => this.engine.x.paph_siprofile_id(this.handle, p)); }
  /** identity of the X profile whose route lanes it bands, hex */
  xid() { return this.engine._hex32(p => this.engine.x.paph_siprofile_xid(this.handle, p)); }
  /** { probes, threshold, budget, features } */
  info() {
    const e = this.engine;
    e.x.paph_siprofile_info(this.handle, e._info);
    const d = e.dv(), v = [0, 1, 2, 3].map(i => d.getInt32(e._info + 4 * i, true));
    return { probes: v[0], threshold: v[1], budget: v[2], features: v[3] };
  }
  free() {
    if (!this.handle) return;
    if (registry) registry.unregister(this._unreg);
    this.engine.x.paph_siprofile_free(this.handle);
    this.handle = 0;
  }
}

/** One PAPH-SI query: a side's signature, probe cells and weights. */
export class SIQuery {
  constructor(engine, handle, profile) {
    this.engine = engine;
    this.handle = handle;
    this.profile = profile;
    const x = engine.x;
    this._unreg = {};
    if (registry) registry.register(this, function () { x.paph_siquery_free(handle); }, this._unreg);
  }
  /** the query side's own signature (104 bytes) */
  signature() {
    const e = this.engine, p = e.x.paph_alloc(SI_SIG_BYTES);
    try { e.x.paph_siquery_sig(this.handle, p); return e.u8().slice(p, p + SI_SIG_BYTES); }
    finally { e.x.paph_free(p, SI_SIG_BYTES); }
  }
  /** the score of a stored signature, or null when no family of it reaches the query */
  score(sig) {
    const e = this.engine, b = bytesOf(sig);
    if (!b || b.length !== SI_SIG_BYTES) throw new TypeError('paph: a PAPH-SI signature is 104 bytes');
    const p = e.put(b);
    try {
      const v = e.x.paph_siquery_score(this.handle, p);
      return v === -2147483648 ? null : v;
    } finally { e.x.paph_free(p, SI_SIG_BYTES); }
  }
  /**
   * The SQL plan (SPEC-SI §7.2): { version, profile, threshold, budget,
   * probes: [[key, family, weight]…], local: [w1, w2, w3], band: [w1, w2, w3],
   * base: [b0…b7] } — the parameters of the one statement every query runs.
   */
  plan() {
    const e = this.engine;
    return JSON.parse(new TextDecoder().decode(e.take(e.x.paph_siquery_plan(e._siprofile(this.profile), this.handle))));
  }
  free() {
    if (!this.handle) return;
    if (registry) registry.unregister(this._unreg);
    this.engine.x.paph_siquery_free(this.handle);
    this.handle = 0;
  }
}

/**
 * An in-memory PAPH-SI index inside the module: posting lists per cell and
 * band key, a query that scores every candidate its probes reach.  Slots are
 * dense and never reused; keep your own slot → work map.  `generation`
 * changes on every add or remove — put it in any cache key.
 */
export class SIIndex {
  constructor(engine, handle) {
    this.engine = engine;
    this.handle = handle;
    const x = engine.x;
    this._unreg = {};
    if (registry) registry.register(this, function () { x.paph_siindex_free(handle); }, this._unreg);
  }
  /** add a signature (104 bytes); returns its slot */
  add(sig) {
    const e = this.engine, b = bytesOf(sig);
    if (!b || b.length !== SI_SIG_BYTES) throw new TypeError('paph: a PAPH-SI signature is 104 bytes');
    const p = e.put(b);
    try { return e.x.paph_siindex_add(this.handle, p); } finally { e.x.paph_free(p, SI_SIG_BYTES); }
  }
  remove(slot) { return this.engine.x.paph_siindex_remove(this.handle, slot >>> 0) === 1; }
  get size() { return this.engine.x.paph_siindex_len(this.handle); }
  get generation() { return this.engine.x.paph_siindex_generation(this.handle) >>> 0; }
  /**
   * The candidates of a query: `{ hits: [{ slot, score }], touched, admitted,
   * postings }`, best first.  Options: `threshold`, `budget` (default the
   * query profile's).
   */
  query(q, o) {
    if (!(q instanceof SIQuery) || !q.handle) throw new TypeError('paph: query needs an SIQuery');
    o = o || {};
    const e = this.engine, info = q.profile ? q.profile.info() : e._siprofileObj().info();
    const threshold = o.threshold === undefined ? info.threshold : o.threshold | 0;
    const budget = o.budget === undefined ? info.budget : Math.max(0, o.budget | 0);
    const cap = Math.min(budget, this.size);
    const out = e.x.paph_alloc(8 * Math.max(1, cap)), st = e.x.paph_alloc(12);
    try {
      const n = e.x.paph_siindex_query(this.handle, q.handle, threshold, cap, out, st);
      const d = e.dv(), hits = new Array(n);
      for (let i = 0; i < n; i++) hits[i] = { slot: d.getInt32(out + 8 * i, true), score: d.getInt32(out + 8 * i + 4, true) };
      return { hits, touched: d.getInt32(st, true), admitted: d.getInt32(st + 4, true), postings: d.getInt32(st + 8, true) };
    } finally { e.x.paph_free(out, 8 * Math.max(1, cap)); e.x.paph_free(st, 12); }
  }
  free() {
    if (!this.handle) return;
    if (registry) registry.unregister(this._unreg);
    this.engine.x.paph_siindex_free(this.handle);
    this.handle = 0;
  }
}

/**
 * PAPH-SI in SQL (SQLite, Cloudflare D1; SPEC-SI §7.2).  Store each work's
 * signature presence byte in `si_works` and one `si_postings` row per key of
 * `engine.sisig(side).keys`; query with `SI_SQL.query` and
 * `siSqlParams(query.plan())`.  The statement returns exactly what
 * `SIIndex.query` returns for the same signatures (test/si-wasm.mjs runs both).
 */
export const SI_SQL = Object.freeze({
  schema: `CREATE TABLE IF NOT EXISTS si_works (
  work_id    INTEGER PRIMARY KEY,
  present    INTEGER NOT NULL,   -- byte 0 of the signature: which families it holds
  sig        BLOB    NOT NULL,   -- the 104-byte signature, to re-key or re-score without the wires
  si_profile TEXT    NOT NULL    -- SIProfile.id().slice(0, 16): re-derive on change
);
CREATE TABLE IF NOT EXISTS si_postings (
  k       INTEGER NOT NULL,      -- a posting key (engine.sikeys)
  work_id INTEGER NOT NULL,
  PRIMARY KEY (k, work_id)
) WITHOUT ROWID;`,
  query: `WITH probe(k, fam, w) AS (
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
SELECT id, score FROM scored WHERE score >= ?16 ORDER BY score DESC, id LIMIT ?17`
});

/** The parameters of `SI_SQL.query` for a plan; `o.threshold`, `o.budget` override the profile's. */
export function siSqlParams(plan, o) {
  o = o || {};
  const th = o.threshold === undefined ? plan.threshold : o.threshold | 0;
  /* SQLite reads LIMIT −1 as "no limit"; the index reads a negative budget as none */
  const budget = o.budget === undefined ? plan.budget : Math.max(0, o.budget | 0);
  return [JSON.stringify(plan.probes), ...plan.local, ...plan.band, ...plan.base, th, budget];
}

const POLICIES = { fast: 1, safe: 2, exact: 3 };

export class Engine {
  constructor(exports) {
    this.x = exports;
    if (exports.paph_abi() !== ABI) throw new Error('paph: wasm ABI ' + exports.paph_abi() + ', glue expects ' + ABI);
    if (exports.paph_xabi() !== X_ABI) throw new Error('paph: wasm X ABI ' + exports.paph_xabi() + ', glue expects ' + X_ABI);
    if (exports.paph_siabi() !== SI_ABI) throw new Error('paph: wasm SI ABI ' + exports.paph_siabi() + ', glue expects ' + SI_ABI);
    if (exports.paph_version() !== WIRE_VERSION) throw new Error('paph: wasm reports wire version ' + exports.paph_version());
    this.backend = 'wasm';
    this.simd = simdSupported();
    this.T1_BYTES = exports.paph_t1_bytes();
    this._u8 = null;
    this._cfg = exports.paph_alloc(4 * exports.paph_config_fields());
    this._lim = exports.paph_alloc(12);
    this._info = exports.paph_alloc(16);
    this._profiles = new Map();
    this._xshipped = null;
    this._sishipped = null;
  }

  _hex32(fill) {
    const p = this.x.paph_alloc(32);
    try {
      fill(p);
      return Array.from(this.u8().subarray(p, p + 32), b => b.toString(16).padStart(2, '0')).join('');
    } finally { this.x.paph_free(p, 32); }
  }

  /* views are detached whenever memory grows, so never hold one across a call */
  u8() {
    const b = this.x.memory.buffer;
    if (!this._u8 || this._u8.buffer !== b) this._u8 = new Uint8Array(b);
    return this._u8;
  }
  dv() { return new DataView(this.x.memory.buffer); }

  put(bytes) {
    if (!bytes || !bytes.length) return 0;
    const p = this.x.paph_alloc(bytes.length);
    this.u8().set(bytes, p);
    return p;
  }

  /* takes a block: [u32 len][u32 cap][payload] -> copy of payload */
  take(ptr) {
    const d = this.dv();
    const len = d.getUint32(ptr, true);
    const out = this.u8().slice(ptr + 8, ptr + 8 + len);
    this.x.paph_release(ptr);
    return out;
  }

  /* the flat config, or 0 for the defaults (the common case costs nothing) */
  config(opts) {
    if (!opts) return 0;
    let any = false;
    for (const k in opts) if (opts[k] !== undefined) { any = true; break; }
    if (!any) return 0;
    const o = Object.assign({}, DEFAULTS);
    for (const k in opts) {
      if (opts[k] === undefined) continue;
      if (!(k in DEFAULTS)) throw new RangeError('paph: unknown config key "' + k + '"');
      o[k] = opts[k];
    }
    const flat = [
      o.foldMatte ? 1 : 0, o.divideUpscale ? 1 : 0, o.matteTol | 0, o.peakRadius | 0,
      o.foldInvert ? 1 : 0, o.localWindows[0] | 0, o.localWindows[1] | 0, o.localCount | 0,
      o.kpCount | 0, o.sketchCount | 0, o.hammingT | 0, o.evidence === 'proportion' ? 1 : 0,
      o.confidenceAt | 0, o.scoring === 'weighted' ? 1 : 0, o.ragEndpoint === 'rank' ? 1 : 0,
      o.geoEnabled ? 1 : 0, o.geoConfAt | 0, o.geoEps | 0, o.mirrorHypothesis ? 1 : 0,
      o.geoMinCorr | 0, o.kpSelect === 0 ? 0 : 1, o.wire === 3 ? 3 : 4
    ];
    const d = this.dv();
    for (let i = 0; i < flat.length; i++) d.setInt32(this._cfg + 4 * i, flat[i], true);
    return this._cfg;
  }

  /**
   * Fingerprint an image: ImageData, {px,w,h}, {pixels,width,height} or
   * (bytes, w, h).  `limits` ([maxW, maxH, maxPixels], e.g. a profile's) may
   * only lower SPEC-004 §16's; a refused image throws RangeError('limit: …'),
   * as the JavaScript engine does.
   */
  hash(a, b, c, d) {
    let opts, limits, im;
    if (typeof b === 'number') { im = readImage(a, b, c); opts = d; }
    else { im = readImage(a); opts = b; limits = c; }
    const px = bytesOf(im.px);
    if (!im.w || !im.h) throw new RangeError('limit: empty image');
    if (px.length !== im.w * im.h * 4) throw new RangeError('limit: pixel buffer length mismatch');
    let lim = 0;
    if (limits) {
      const dv = this.dv();
      for (let i = 0; i < 3; i++) dv.setInt32(this._lim + 4 * i, limits[i] | 0, true);
      lim = this._lim;
    }
    const cfg = this.config(opts);
    const p = this.put(px);
    let blk;
    try { blk = this.take(this.x.paph_hash_checked(cfg, p, im.w, im.h, lim)); }
    finally { if (p) this.x.paph_free(p, px.length); }
    const v = new DataView(blk.buffer, blk.byteOffset, blk.byteLength);
    const n1 = v.getUint32(0, true), n2 = v.getUint32(4, true);
    if (n1 === 0) throw new RangeError(new TextDecoder().decode(blk.subarray(8, 8 + n2)));
    const t1 = blk.slice(8, 8 + n1), t2 = blk.slice(8 + n1, 8 + n1 + n2);
    const h = new DataView(t1.buffer);
    return { t1, t2, width: h.getUint16(8, true), height: h.getUint16(10, true),
             kpCount: h.getUint16(14, true), crc: h.getUint32(60, true) };
  }

  /**
   * Parse a wire pair once.  `strict` refuses a Tier 2 claiming more than 512
   * keypoints (no conforming hasher writes one; a comparison is quadratic in
   * it) — use it for anything a user supplied.  Throws when Tier 1 is refused.
   */
  prepare(t1, t2, opts) {
    if (t1 && t1.t1 && !(t1 instanceof Uint8Array)) { opts = t2; t2 = t1.t2; t1 = t1.t1; }
    const b1 = bytesOf(t1), b2 = bytesOf(t2);
    if (!b1) throw new TypeError('paph: prepare needs a Tier 1');
    const p1 = this.put(b1), p2 = this.put(b2);
    let h;
    try { h = this.x.paph_prepare(p1, b1.length, p2, b2 ? b2.length : 0, opts && opts.strict ? 1 : 0); }
    finally { if (p1) this.x.paph_free(p1, b1.length); if (p2) this.x.paph_free(p2, b2.length); }
    if (!h) throw new Error('paph: tier 1 refused (length, version, CRC or section table)');
    this.x.paph_prepared_info(h, this._info);
    const d = this.dv(), info = [0, 1, 2, 3].map(i => d.getInt32(this._info + 4 * i, true));
    return new Side(this, h, info);
  }

  /** A .pcal calibration artefact.  Throws if it does not decode. */
  profile(bytes) {
    const b = bytesOf(bytes);
    const p = this.put(b);
    let h;
    try { h = this.x.paph_profile(p, b.length); } finally { this.x.paph_free(p, b.length); }
    if (!h) throw new Error('paph: calibration artefact does not decode');
    return new Profile(this, h, b);
  }

  _profile(p) {
    if (!p) return 0;
    if (p instanceof Profile) return p.handle;
    const b = bytesOf(p);
    /* cache decoded artefacts by content, so a caller passing the same bytes
       on every call decodes them once */
    let key = b.length + ':';
    for (let i = 0; i < b.length; i += 97) key += b[i].toString(16);
    let pr = this._profiles.get(key);
    if (!pr || pr.bytes.length !== b.length || !pr.bytes.every((v, i) => v === b[i])) {
      pr = this.profile(b);
      if (this._profiles.size > 16) this._profiles.clear();
      this._profiles.set(key, pr);
    }
    return pr.handle;
  }

  _side(x, temps, strict) {
    if (x instanceof Side) {
      if (!x.handle) throw new Error('paph: side was freed');
      return x.handle;
    }
    const s = this.prepare(x.t1, x.t2, { strict: !!strict });
    temps.push(s);
    return s.handle;
  }

  /**
   * Comparator 42.  `a`, `b`: Sides or { t1, t2 }.  Options: `profile` (a
   * Profile or .pcal bytes; default the shipped CAL-007-PROVISIONAL — 1.1's
   * CAL-004-PROPOSED is `docs/calibration/CAL-004-PROPOSED.pcal`), `opts`
   * (compare-time config), `lean` (same verdict, `v3: null`, ~20% faster),
   * `json` (return the text instead of parsing it).
   *
   * The report is the object the JavaScript engine returns — `JSON.stringify`
   * of either is the same string.
   */
  compare(a, b, o) {
    o = o || {};
    const temps = [];
    let text;
    try {
      const ha = this._side(a, temps), hb = this._side(b, temps);
      text = new TextDecoder().decode(this.take(this.x.paph_compare42(
        this.config(o.opts), this._profile(o.profile), ha, hb, o.lean ? 1 : 0)));
    } finally { for (const s of temps) s.free(); }
    return o.json ? text : JSON.parse(text);
  }

  /** §A4 — the stage-1 screen.  Not a verdict: a rejected pair is UNSCREENED, never Unrelated. */
  screen(a, b, o) {
    o = o || {};
    const temps = [];
    let v;
    try {
      v = this.x.paph_screen42(this.config(o.opts), this._profile(o.profile),
                               this._side(a, temps), this._side(b, temps));
    } finally { for (const s of temps) s.free(); }
    if (v < 0) throw new Error('paph: screen needs two sides');
    return { pass: (v & 1) === 1, poolDirect: (v >> 1) & 2047, poolMirror: (v >> 12) & 2047 };
  }

  /**
   * Screen and compare one query against many candidates in one call.
   * Returns one record per candidate, in candidate order:
   *   { index, state, verdict, certifiable, structural, geometryEvidence,
   *     totalInliers, topology, geoMargin, localEvidence, localMatches,
   *     diversity, multiplier, models, poolDirect, poolMirror, swapped,
   *     screenPass, mixedSelection, mirrored }
   * `state` is -1 (UNSCREENED: the screen rejected it, it was not compared)
   * or an index into ['Unrelated','Related','Suspected','Copy','Identical',
   * 'Indeterminate'].  Options: `gate` (default true), `profile`, `opts`,
   * `raw` (also return the Int32Array as `.records`).
   * The gate passes a pair only on `geo_min_corr` (8) keypoint
   * correspondences or more, so it drops every copy of a work with fewer
   * keypoints, and some others, that comparator 42 would certify:
   * `gate: false`, or `xrank` under its shipped X3 profile, keeps those
   * (docs/SEARCH.md §4).
   */
  rank(query, candidates, o) {
    o = o || {};
    const n = candidates.length;
    const temps = [];
    const hp = this.x.paph_alloc(4 * Math.max(1, n));
    const op = this.x.paph_alloc(4 * RANK_FIELDS * Math.max(1, n));
    let recs;
    try {
      const hq = this._side(query, temps);
      const hs = candidates.map(c => this._side(c, temps, true));
      const d = this.dv();
      for (let i = 0; i < n; i++) d.setUint32(hp + 4 * i, hs[i], true);
      this.x.paph_rank42(this.config(o.opts), this._profile(o.profile), hq, hp, n,
                         o.gate === false ? 0 : 1, op);
      recs = new Int32Array(this.x.memory.buffer.slice(op, op + 4 * RANK_FIELDS * n));
    } finally {
      this.x.paph_free(hp, 4 * Math.max(1, n));
      this.x.paph_free(op, 4 * RANK_FIELDS * Math.max(1, n));
      for (const s of temps) s.free();
    }
    const out = new Array(n);
    for (let i = 0; i < n; i++) {
      const r = recs.subarray(i * RANK_FIELDS, (i + 1) * RANK_FIELDS);
      out[i] = {
        index: i, state: r[0], verdict: r[0] < 0 ? 'Unscreened' : STATES[r[0]],
        certifiable: r[1] === 1, structural: r[2], geometryEvidence: r[3], totalInliers: r[4],
        topology: r[5], geoMargin: r[6], localEvidence: r[7], localMatches: r[8],
        diversity: r[9], multiplier: r[10], models: r[11], poolDirect: r[12], poolMirror: r[13],
        swapped: r[14] === 1, screenPass: (r[15] & 1) === 1, mixedSelection: (r[15] & 2) === 2,
        mirrored: (r[15] & 4) === 4
      };
    }
    if (o.raw) out.records = recs;
    return out;
  }

  /* ------------------------------------------------------------ PAPH-X */

  /**
   * A PAPH-X profile: `base` a Profile (or .pcal bytes; default the shipped
   * CAL-007-PROVISIONAL), `x` the X artefact bytes (default the shipped
   * X3-PROVISIONAL's schedule bound to that base; 1.1's X2-PROVISIONAL and
   * 1.0.0's X1-PROVISIONAL, both bound to CAL-004-PROPOSED, are
   * `docs/calibration/X2-PROVISIONAL.pxcl` and `X1-PROVISIONAL.pxcl`).
   * Check `status()`: a mismatch between the two makes every comparison
   * Indeterminate.
   */
  xprofile(o) {
    o = o || {};
    const base = this._profile(o.base);
    const xb = bytesOf(o.x);
    const p = xb ? this.put(xb) : 0;
    let h;
    try { h = this.x.paph_xprofile(base, p, xb ? xb.length : 0); } finally { if (p) this.x.paph_free(p, xb.length); }
    if (!h) throw new Error('paph: X profile artefact does not decode');
    return new XProfile(this, h, base);
  }

  _xprofile(p) {
    if (p instanceof XProfile) {
      if (!p.handle) throw new Error('paph: X profile was freed');
      return p.handle;
    }
    if (!this._xshipped) this._xshipped = this.xprofile();
    return this._xshipped.handle;
  }

  /**
   * Prepare a side for PAPH-X: `t1`, `t2` (or `{ t1, t2 }`), options
   * `{ strict, profile, sidecar }` — `sidecar` the PAX1 bytes a previous
   * `XSide.sidecar()` returned (ignored when it describes another side or
   * profile, or is 1.1's version 1).
   */
  xprepare(t1, t2, opts) {
    if (t1 && t1.t1 && !(t1 instanceof Uint8Array)) { opts = t2; t2 = t1.t2; t1 = t1.t1; }
    opts = opts || {};
    const b1 = bytesOf(t1), b2 = bytesOf(t2), sc = bytesOf(opts.sidecar);
    if (!b1) throw new TypeError('paph: xprepare needs a Tier 1');
    const xp = this._xprofile(opts.profile);
    const p1 = this.put(b1), p2 = this.put(b2), ps = sc ? this.put(sc) : 0;
    let h;
    try {
      h = sc
        ? this.x.paph_xprepare_sidecar(xp, p1, b1.length, p2, b2 ? b2.length : 0, ps, sc.length, opts.strict ? 1 : 0)
        : this.x.paph_xprepare(xp, p1, b1.length, p2, b2 ? b2.length : 0, opts.strict ? 1 : 0);
    } finally {
      if (p1) this.x.paph_free(p1, b1.length);
      if (p2) this.x.paph_free(p2, b2.length);
      if (ps) this.x.paph_free(ps, sc.length);
    }
    if (!h) throw new Error('paph: tier 1 refused (length, version, CRC or section table)');
    this.x.paph_xprepared_info(h, this._info);
    const d = this.dv(), info = [0, 1, 2, 3].map(i => d.getInt32(this._info + 4 * i, true));
    return new XSide(this, h, info);
  }

  _xside(x, temps, profile, strict) {
    if (x instanceof XSide) {
      if (!x.handle) throw new Error('paph: side was freed');
      return x.handle;
    }
    const s = this.xprepare(x.t1, x.t2, { strict: !!strict, profile });
    temps.push(s);
    return s.handle;
  }

  /**
   * The PAPH-X pair screen (§12): never a verdict.  `state` is 'Reject'
   * (not worth the comparator under the profile), 'Defer' (not eliminated),
   * 'Pass' or 'Identical'; the route readings and the anchor-tier pools
   * come with it.
   */
  xscreen(a, b, o) {
    o = o || {};
    const temps = [];
    const out = this.x.paph_alloc(4 * XSCREEN_FIELDS);
    let r;
    try {
      const xp = this._xprofile(o.profile);
      const v = this.x.paph_xscreen(this.config(o.opts), xp, this._xside(a, temps, o.profile), this._xside(b, temps, o.profile), out);
      if (v < 0) throw new Error('paph: xscreen refused (profiles or wire formats differ)');
      r = new Int32Array(this.x.memory.buffer.slice(out, out + 4 * XSCREEN_FIELDS));
    } finally { this.x.paph_free(out, 4 * XSCREEN_FIELDS); for (const s of temps) s.free(); }
    return {
      state: SCREENS[r[0]], route: { local: r[1], band: r[2], global: r[3], class: ROUTE_CLASSES[r[4]] },
      poolDirect: r[5], poolMirror: r[6], supportDirect: r[7], supportMirror: r[8],
      rowsScanned: r[9], hammingPairs: r[10], swapped: r[11] === 1
    };
  }

  /**
   * The PAPH-X comparison (§13).  Options: `profile` (an XProfile), `opts`,
   * `policy` ('fast' | 'safe' | 'exact'; default the profile's, safe),
   * `audit` (run EXACT42 beside the fast path and attach it), `scope`
   * ('full' | 'copy'), `json`.  The report carries `verdict` (the
   * comparator-42 vocabulary, plus 'NotCopy' under copy scope) and
   * `execution` ('FAST' | 'DEFERRED' | 'FALLBACK' | 'AUDIT').
   */
  xcompare(a, b, o) {
    o = o || {};
    const temps = [];
    let text;
    try {
      const xp = this._xprofile(o.profile);
      const flags = (POLICIES[o.policy] || 0) | (o.audit ? 4 : 0) | (o.scope === 'copy' ? 8 : 0);
      text = new TextDecoder().decode(this.take(this.x.paph_xcompare(
        this.config(o.opts), xp, this._xside(a, temps, o.profile), this._xside(b, temps, o.profile), flags)));
    } finally { for (const s of temps) s.free(); }
    return o.json ? text : JSON.parse(text);
  }

  /**
   * XRank (§14): one query against many candidates in one call — the route
   * table screened with SIMD, the sparse screen on what the route did not
   * reject, the cascade on the survivors.  Options: `profile`, `opts`,
   * `policy`, `gate` (default true), `scope` ('copy' default | 'full'),
   * `raw`.  One record per candidate:
   *   { index, state, verdict, execution, screen, route: {local, band,
   *     global, class}, poolDirect, poolMirror, rowsScanned, inliers,
   *     models, geometryEvidence, geoMargin, topology, structuralLo,
   *     structuralHi, structuralExact, certifiable, localEvidence,
   *     hammingPairs, fullPairs, swapped, certificate, mirrored,
   *     explosion, fallbackRan, mixedSelection }
   * `state` is -1 (rejected / unscreened, not compared) or an index into
   * STATES (6 = NotCopy: not lifted above Related, not resolved further).
   */
  xrank(query, candidates, o) {
    o = o || {};
    const n = candidates.length;
    const temps = [];
    const hp = this.x.paph_alloc(4 * Math.max(1, n));
    const op = this.x.paph_alloc(4 * XRANK_FIELDS * Math.max(1, n));
    let recs;
    try {
      const xp = this._xprofile(o.profile);
      const hq = this._xside(query, temps, o.profile);
      const hs = candidates.map(c => this._xside(c, temps, o.profile, true));
      const d = this.dv();
      for (let i = 0; i < n; i++) d.setUint32(hp + 4 * i, hs[i], true);
      const flags = (POLICIES[o.policy] || 0) | (o.gate === false ? 4 : 0) | (o.scope === 'full' ? 8 : 0);
      this.x.paph_xrank(this.config(o.opts), xp, hq, hp, n, flags, op);
      recs = new Int32Array(this.x.memory.buffer.slice(op, op + 4 * XRANK_FIELDS * n));
    } finally {
      this.x.paph_free(hp, 4 * Math.max(1, n));
      this.x.paph_free(op, 4 * XRANK_FIELDS * Math.max(1, n));
      for (const s of temps) s.free();
    }
    const out = new Array(n);
    for (let i = 0; i < n; i++) {
      const r = recs.subarray(i * XRANK_FIELDS, (i + 1) * XRANK_FIELDS);
      out[i] = {
        index: i, state: r[0], verdict: r[0] < 0 ? 'Unscreened' : STATES[r[0]], execution: EXECUTIONS[r[1]],
        screen: SCREENS[r[2]] || 'Refused',
        route: { local: r[3], band: r[4], global: r[5], class: ROUTE_CLASSES[r[6]] },
        poolDirect: r[7], poolMirror: r[8], rowsScanned: r[9], inliers: r[10], models: r[11],
        geometryEvidence: r[12], geoMargin: r[13], topology: r[14],
        structuralLo: r[15], structuralHi: r[16], structuralExact: r[17] === 1, certifiable: r[18] === 1,
        localEvidence: r[19], hammingPairs: r[20], fullPairs: r[21], swapped: r[22] === 1,
        certificate: (r[23] & 1) === 1, mirrored: (r[23] & 2) === 2, explosion: (r[23] & 4) === 4,
        fallbackRan: (r[23] & 8) === 8, mixedSelection: (r[23] & 16) === 16
      };
    }
    if (o.raw) out.records = recs;
    return out;
  }

  /**
   * A PAPH-SI profile from its artefact bytes, or the shipped SI4-PROVISIONAL
   * without them.  Its `xid()` names the X profile it was fitted against:
   * signatures and queries refuse sides prepared under any other.
   */
  siprofile(bytes) {
    const b = bytesOf(bytes);
    const p = b ? this.put(b) : 0;
    let h;
    try { h = this.x.paph_siprofile(p, b ? b.length : 0); } finally { if (p) this.x.paph_free(p, b.length); }
    if (!h) throw new Error('paph: SI profile artefact does not decode');
    return new SIProfile(this, h);
  }

  _siprofileObj(p) {
    if (p instanceof SIProfile) {
      if (!p.handle) throw new Error('paph: SI profile was freed');
      return p;
    }
    if (!this._sishipped) this._sishipped = this.siprofile();
    return this._sishipped;
  }

  _siprofile(p) { return this._siprofileObj(p).handle; }

  /**
   * The PAPH-SI signature of a side — what an index stores: `{ bytes (104),
   * present: [family names], cells: { runs: 0..255, … }, keys }`, where `keys`
   * are the integer posting keys for an SQL index (SPEC-SI §7.2).  `side`: an
   * XSide (its route is reused) or `{ t1, t2 }` (derived from the wires alone,
   * no bucket index — the cheap path for re-indexing stored works).  Options:
   * `profile` (SIProfile), `xprofile` (XProfile, for wires).
   */
  sisig(side, o) {
    o = o || {};
    const sp = this._siprofile(o.profile);
    const out = this.x.paph_alloc(SI_SIG_BYTES);
    let bytes;
    try {
      let r;
      if (side instanceof XSide) {
        if (!side.handle) throw new Error('paph: side was freed');
        r = this.x.paph_sisig(sp, side.handle, out);
      } else {
        const b1 = bytesOf(side && side.t1), b2 = bytesOf(side && side.t2);
        if (!b1) throw new TypeError('paph: sisig needs an XSide or { t1, t2 }');
        const p1 = this.put(b1), p2 = this.put(b2);
        try { r = this.x.paph_sisig_wire(sp, this._xprofile(o.xprofile), p1, b1.length, p2, b2 ? b2.length : 0, out); }
        finally { if (p1) this.x.paph_free(p1, b1.length); if (p2) this.x.paph_free(p2, b2.length); }
      }
      if (r === -2) throw new Error('paph: the side was prepared under another X profile than the SI profile is bound to');
      if (r === -3) throw new Error('paph: tier 1 refused (length, version, CRC or section table)');
      if (r !== 0) throw new Error('paph: sisig refused');
      bytes = this.u8().slice(out, out + SI_SIG_BYTES);
    } finally { this.x.paph_free(out, SI_SIG_BYTES); }
    return this._sigInfo(bytes);
  }

  _sigInfo(bytes) {
    const present = [], cells = {};
    for (let f = 0; f < 8; f++) if (bytes[0] >> f & 1) present.push(SI_FAMILIES[f]);
    for (let f = 0; f < 6; f++) if (bytes[0] >> f & 1) cells[SI_FAMILIES[f]] = bytes[1 + f];
    return { bytes, present, cells, keys: this.sikeys(bytes) };
  }

  /** the posting keys of a stored signature (integers below 2^27) */
  sikeys(sig) {
    const b = bytesOf(sig);
    const p = this.put(b), out = this.x.paph_alloc(4 * SI_MAX_KEYS);
    try {
      const n = this.x.paph_sikeys(p, out);
      return Array.from(new Int32Array(this.x.memory.buffer.slice(out, out + 4 * n)));
    } finally { this.x.paph_free(p, SI_SIG_BYTES); this.x.paph_free(out, 4 * SI_MAX_KEYS); }
  }

  /**
   * A PAPH-SI query for a side (an XSide, or `{ t1, t2 }` prepared here).
   * Options: `profile` (SIProfile), `xprofile` (XProfile).
   */
  siquery(side, o) {
    o = o || {};
    const prof = this._siprofileObj(o.profile);
    const temps = [];
    let h;
    try { h = this.x.paph_siquery(prof.handle, this._xside(side, temps, o.xprofile)); }
    finally { for (const s of temps) s.free(); }
    if (!h) throw new Error('paph: the side was prepared under another X profile than the SI profile is bound to');
    return new SIQuery(this, h, prof);
  }

  /** a new, empty in-memory PAPH-SI index */
  siindex() { return new SIIndex(this, this.x.paph_siindex_new()); }

  /**
   * Keypoint descriptors of a side, eight u32 words per keypoint as stored:
   * the keypoints a comparison uses (Tier 2, or the sketch without one) in
   * wire order, or strongest first with `{ strongest: true }`, or with
   * `{ sketch: true }` the 32 Tier-1 sketch keypoints.
   */
  descriptors(side, o) {
    const temps = [];
    try {
      const h = this._side(side, temps);
      const which = o && o.sketch ? 1 : o && o.strongest ? 2 : 0;
      const n = this.x.paph_descriptors(h, which, 0, 0);
      const p = this.x.paph_alloc(32 * Math.max(1, n));
      try {
        this.x.paph_descriptors(h, which, p, n);
        return new Uint32Array(this.x.memory.buffer.slice(p, p + 32 * n));
      } finally { this.x.paph_free(p, 32 * Math.max(1, n)); }
    } finally { for (const s of temps) s.free(); }
  }

  /**
   * Exact-match keys for an inverted index (KEYS_VERSION 1), as JavaScript
   * numbers (all below 2^53, so they survive JSON and SQLite exactly):
   *
   *   codes  each Tier-1 local code, folded to 53 bits: (hi & 0x1fffff)·2^32 + lo.
   *          Codes are canonical under the square's eight symmetries and the
   *          complement, so mirrored, rotated, inverted, cropped, integer-
   *          upscaled and pasted copies share many of them with the original.
   *   bands  ten 24-bit bands of a keypoint descriptor: band j is its bytes
   *          3j..3j+2, key j·2^24 + value.  A stored work is indexed by the
   *          bands of its `maxKeypoints` strongest keypoints (default 64); a
   *          query (`{ query: true }`) uses all its keypoints (default up to
   *          512) and also the bands of their mirrored descriptors (a mirror
   *          swaps a descriptor's halves), so a reflected copy finds its
   *          original through bands as well as codes.  Bands catch pastes and
   *          tight crops the codes miss.  Constant bands (all zeros, all ones)
   *          are dropped: they are texture, not identity.
   *
   * Index both families; score a candidate by the sum of 1/df over the keys it
   * shares with the query (df: how many indexed works hold that key), and keep
   * the top few per family.  Keys only nominate — the comparator decides.  The
   * index side defines KEYS_VERSION: re-key stored works when it changes.  See
   * docs/SEARCH.md for what each family finds and misses.
   */
  indexKeys(side, o) {
    const query = !!(o && o.query);
    let tmp = null;
    const s = side instanceof Side ? side : (tmp = this.prepare(side.t1, side.t2));
    try {
      const c = this.localCodes(s);
      const codes = new Set();
      for (let i = 0; i < c.length; i += 2) codes.add((c[i] & 0x1fffff) * 4294967296 + (c[i + 1] >>> 0));
      const d = this.descriptors(s, { strongest: true });
      const nk = Math.min(d.length / 8, (o && o.maxKeypoints) || (query ? 512 : 64));
      const mirror = query && !(o && o.mirror === false);
      const bands = new Set();
      const put = (bytes, b) => {
        for (let j = 0; j < 10; j++) {
          const v = (bytes[b + 3 * j] << 16) | (bytes[b + 3 * j + 1] << 8) | bytes[b + 3 * j + 2];
          if (v !== 0 && v !== 0xffffff) bands.add(j * 16777216 + v);
        }
      };
      const bytes = new Uint8Array(d.buffer, d.byteOffset, d.byteLength);
      const m = new Uint8Array(32);
      for (let k = 0; k < nk; k++) {
        put(bytes, 32 * k);
        if (mirror) {
          m.set(bytes.subarray(32 * k + 16, 32 * k + 32), 0);
          m.set(bytes.subarray(32 * k, 32 * k + 16), 16);
          put(m, 0);
        }
      }
      const num = (x, y) => x - y;
      return { version: KEYS_VERSION, codes: [...codes].sort(num), bands: [...bands].sort(num) };
    } finally { if (tmp) tmp.free(); }
  }

  /**
   * The Tier-1 local codes of a side, `[hi0, lo0, hi1, lo1, …]` — canonical
   * under the eight symmetries of the square and the complement, so a
   * mirrored, rotated or inverted copy stores the same code for the same
   * region.  The natural exact-match keys of an inverted index.
   */
  localCodes(side) {
    const temps = [];
    try {
      const h = this._side(side, temps);
      const p = this.x.paph_alloc(8 * 128);
      try {
        const n = this.x.paph_local_codes(h, p, 128);
        return new Uint32Array(this.x.memory.buffer.slice(p, p + 8 * n));
      } finally { this.x.paph_free(p, 8 * 128); }
    } finally { for (const s of temps) s.free(); }
  }
}

export { DEFAULTS, STATES, EXECUTIONS, RANK_FIELDS, XRANK_FIELDS, XSCREEN_FIELDS, ABI, X_ABI, SI_ABI, SI_SIG_BYTES, SI_FAMILIES, WIRE_VERSION, KEYS_VERSION };
export const backend = 'wasm';
