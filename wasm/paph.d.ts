/**
 * Types for wasm/paph.js — the WebAssembly engine of PAPH 4.2, with PAPH-X.
 */

export type ImageLike =
  | { data: Uint8Array | Uint8ClampedArray; width: number; height: number }
  | { px: Uint8Array | Uint8ClampedArray; w: number; h: number }
  | { pixels: Uint8Array | Uint8ClampedArray; width: number; height: number };

export interface Wires {
  /** Tier 1, exactly 3952 bytes */
  t1: Uint8Array;
  /** Tier 2, 32 + 40n bytes (n <= 512); may be absent (Tier-1 sketch only) */
  t2?: Uint8Array | null;
}

export interface HashResult extends Wires {
  t2: Uint8Array;
  width: number;
  height: number;
  kpCount: number;
  crc: number;
}

/** Hash-time and compare-time options; the same keys as the JavaScript engine's DEFAULTS. */
export interface Options {
  foldMatte?: boolean;
  divideUpscale?: boolean;
  matteTol?: number;
  peakRadius?: number;
  foldInvert?: boolean;
  localWindows?: [number, number];
  localCount?: number;
  kpCount?: number;
  kpSelect?: 0 | 1;
  sketchCount?: number;
  /** the wire format hash() writes: 4 (the default from 1.2) or 3 (1.0–1.1's bytes).  A comparison
   *  of a wire-3 side with a wire-4 side is refused (Indeterminate, reason WIRE_MISMATCH). */
  wire?: 3 | 4;
  hammingT?: number;
  evidence?: 'lift' | 'proportion';
  confidenceAt?: number;
  scoring?: 'weighted' | 'gate';
  ragEndpoint?: 'rank' | 'quantile';
  geoEnabled?: boolean;
  geoConfAt?: number;
  geoEps?: number;
  geoMinCorr?: number;
  mirrorHypothesis?: boolean;
}

export type Verdict = 'Unrelated' | 'Related' | 'Suspected' | 'Copy' | 'Identical' | 'Indeterminate';

/** The comparator-42 report — the object the JavaScript engine returns. */
export interface Report {
  comparator: number;
  verdict: Verdict;
  class: string;
  basis: string[];
  reasons: string[];
  structural: number;
  certifiable: boolean;
  /** the v3 reading in full; null on a lean comparison or a refusal */
  v3: Record<string, unknown> | null;
  local: Record<string, unknown> | null;
  models: Array<{ r00: number; r10: number; tx: number; ty: number; scaleQ16: number; mirror: boolean;
                  inliers: number; medianErr: number; confSum: number }>;
  topology: number;
  totalInliers: number;
  coverage: Record<string, unknown> | null;
  geometryEvidence: number;
  geoMeasurable: boolean;
  geoRaw: number;
  geoCtl: number;
  geoMargin: number;
  geoCtlMember: string;
  swapped: boolean;
  calibration: string;
  calibrationId: string;
  geoWeakInliers: number;
  diversity: { spatial: number; scale: number; model: number; descriptor: number; combined: number; multiplier: number };
  medianErr: number[];
  selection: { a: number; b: number; mixed: boolean };
  kpA: number;
  kpB: number;
  screen: null;
}

export interface Screen {
  pass: boolean;
  poolDirect: number;
  poolMirror: number;
}

export interface RankRecord {
  index: number;
  /** -1 UNSCREENED (not compared), else an index into STATES */
  state: number;
  verdict: Verdict | 'Unscreened';
  certifiable: boolean;
  structural: number;
  geometryEvidence: number;
  totalInliers: number;
  topology: number;
  geoMargin: number;
  /** -1 when the local channel abstained */
  localEvidence: number;
  localMatches: number;
  diversity: number;
  multiplier: number;
  models: number;
  poolDirect: number;
  poolMirror: number;
  swapped: boolean;
  screenPass: boolean;
  mixedSelection: boolean;
  mirrored: boolean;
}

export interface IndexKeys {
  version: number;
  /** Tier-1 local codes folded to 53 bits, ascending */
  codes: number[];
  /** 24-bit descriptor bands, j·2^24 + value (j = 0..9), ascending */
  bands: number[];
}

export declare class Side {
  readonly handle: number;
  readonly kp: number;
  readonly tier2Ok: boolean;
  readonly width: number;
  readonly height: number;
  free(): void;
}

export declare class Profile {
  readonly handle: number;
  readonly bytes: Uint8Array;
  free(): void;
}

type SideOrWires = Side | Wires;

export interface CompareOptions {
  profile?: Profile | Uint8Array;
  opts?: Options;
  /** same verdict, `v3: null`, ~20% less work */
  lean?: boolean;
}

/* ------------------------------------------------------------------ PAPH-X */

export type Execution = 'FAST' | 'DEFERRED' | 'FALLBACK' | 'AUDIT';
export type ScreenState = 'Reject' | 'Defer' | 'Pass' | 'Identical' | 'Refused';
export type RouteClass = 'REJECT' | 'DEFER' | 'FAST' | 'ABSENT';
export type Policy = 'fast' | 'safe' | 'exact';
/** the comparator-42 vocabulary plus 'NotCopy' (copy scope: not lifted above Related, not resolved further) */
export type XVerdict = Verdict | 'NotCopy';

/** A PAPH-X profile (an X artefact bound to a comparator-42 profile). */
export declare class XProfile {
  readonly handle: number;
  /** the X artefact's bytes — store them: the identity covers every parameter */
  bytes(): Uint8Array;
  /** SHA-256 identity, hex */
  id(): string;
  /** 'ok', or 'mismatch' (bound to another base profile: every comparison is Indeterminate), or 'unsupported' */
  status(): 'ok' | 'unsupported' | 'mismatch';
  free(): void;
}

/** A side prepared for PAPH-X: the wires parsed plus the route, the bucket index and the anchor order. */
export declare class XSide {
  readonly handle: number;
  readonly kp: number;
  readonly tier2Ok: boolean;
  readonly width: number;
  readonly height: number;
  /** the 136-byte route record (128 route bytes + metadata) */
  route(): Uint8Array;
  /** the PAX1 sidecar (version 2: bound to this side's Tier 1 and to the X profile): cache it beside
   *  the wires and hand it to `xprepare` to skip the derivation */
  sidecar(): Uint8Array;
  free(): void;
}

type XSideOrWires = XSide | Wires;

/* ---- PAPH-SI (docs/SPEC-SI-paph-si.md) ---- */

export type SIFamily = 'runs' | 'tone' | 'pal' | 'shape' | 'sil' | 'kpgeo' | 'local' | 'band';

/** A PAPH-SI profile (.psi): six 16 / 256-cell codebooks, evidence weights, default threshold and budget. */
export declare class SIProfile {
  readonly handle: number;
  /** the artefact's bytes — store them: the identity covers every parameter */
  bytes(): Uint8Array;
  /** SHA-256 identity, hex */
  id(): string;
  /** identity of the X profile whose route lanes it bands, hex */
  xid(): string;
  info(): { probes: number; threshold: number; budget: number; features: number };
  free(): void;
}

/** What an index stores for one work. */
export interface SISignature {
  /** the 104-byte signature */
  bytes: Uint8Array;
  /** the families the work can be measured on */
  present: SIFamily[];
  /** the fine cell (0–255) of each quantised family present; its coarse cell (0–15), the median bit of each axis, is its odd bits: `(c >> 1 & 1) | (c >> 2 & 2) | (c >> 3 & 4) | (c >> 4 & 8)` */
  cells: Partial<Record<'runs' | 'tone' | 'pal' | 'shape' | 'sil' | 'kpgeo', number>>;
  /** the posting keys for an SQL index (integers below 2^27) */
  keys: number[];
}

/** The parameters of `SI_SQL.query` for one query (see `siSqlParams`). */
export interface SIPlan {
  version: number;
  /** SIProfile.id().slice(0, 16) — the rows must have been keyed under the same profile */
  profile: string;
  threshold: number;
  budget: number;
  /** [posting key, family (0–5 quantised, 6 local, 7 band), weight] */
  probes: [number, number, number][];
  /** the score a candidate gains for 1, 2–3 and ≥ 4 equal local / descriptor band keys */
  local: [number, number, number];
  band: [number, number, number];
  /** per presence bit of a candidate: the none-weight it adds when the query holds that family too */
  base: number[];
}

/** One PAPH-SI query: a side's signature, probe cells and weights. */
export declare class SIQuery {
  readonly handle: number;
  signature(): Uint8Array;
  /** the score of a stored signature, or null when none of its families reaches the query */
  score(sig: Uint8Array): number | null;
  plan(): SIPlan;
  free(): void;
}

export interface SIHits {
  /** best first: score descending, slot ascending */
  hits: { slot: number; score: number }[];
  /** candidates some probe or band key reached */
  touched: number;
  /** of those, at or above the threshold (before the budget cut) */
  admitted: number;
  /** posting entries read */
  postings: number;
}

/** An in-memory PAPH-SI index inside the module.  Slots are dense and never reused. */
export declare class SIIndex {
  readonly handle: number;
  add(sig: Uint8Array): number;
  remove(slot: number): boolean;
  readonly size: number;
  /** changes on every add or remove: part of any cache key */
  readonly generation: number;
  query(q: SIQuery, o?: { threshold?: number; budget?: number }): SIHits;
  free(): void;
}

export interface RouteReading {
  /** agreeing local MinHash lanes, 0–64 */
  local: number;
  /** agreeing band MinHash lanes, 0–32 */
  band: number;
  /** global invariant similarity, 0–255 */
  global: number;
  class: RouteClass;
}

export interface XScreen {
  state: ScreenState;
  route: RouteReading;
  poolDirect: number;
  poolMirror: number;
  supportDirect: number;
  supportMirror: number;
  rowsScanned: number;
  hammingPairs: number;
  swapped: boolean;
}

export interface XGeometry {
  anchors: number;
  expandedTo: number;
  inliers: number;
  weakInliers: number;
  models: Report['models'];
  measurable: boolean;
  raw: number;
  ctl: number;
  ctlMember: string;
  ctlRan: boolean;
  margin: number;
  evidence: number;
  diversity: Report['diversity'];
  coverage: Record<string, unknown> | null;
  topology: number;
  /** §9.4: the sparse geometry is trusted on its own */
  certificate: boolean;
  uniqueA: number;
  uniqueB: number;
}

export interface XStructural {
  /** the structural lattice interval: lo = every unknown channel at its worst, hi at its best */
  lo: number;
  hi: number;
  /** every channel computed (lo === hi) */
  exact: boolean;
  channels: Record<'dct' | 'local' | 'shape' | 'topology' | 'runs' | 'palette' | 'silhouette',
                   { value: number; measurable: boolean; known: boolean }>;
  localEvidence: number | null;
  localMatches: number | null;
  diversity: number;
  coverageMin: number | null;
  certifiable: boolean;
}

/** The PAPH-X report (comparator 50). */
export interface XReport {
  comparator: number;
  verdict: XVerdict;
  class: string;
  basis: string[];
  reasons: string[];
  execution: Execution;
  /** why the execution state is what it is: 'bounded-evidence', 'exact-evidence', 'route', 'uncertified-geometry', … */
  reason: string;
  route: RouteReading & { measurable: number };
  sparse: {
    directPool: number; mirrorPool: number; touchedPairs: number; hammingPairs: number;
    /** the descriptor pairs the exhaustive scan would have evaluated (both hypotheses) */
    fullPairs: number;
    rowsScanned: number; denseRows: number; explosion: boolean;
  };
  geometry: XGeometry | null;
  structural: XStructural | null;
  swapped: boolean;
  selection: { mixed: boolean };
  kpA: number;
  kpB: number;
  /** the comparator-42 report when EXACT42 ran (FALLBACK or AUDIT), else null */
  fallback: Report | null;
  calibration: string;
  calibrationId: string;
  xcalibration: string;
  xcalibrationId: string;
}

export interface XCompareOptions {
  profile?: XProfile;
  opts?: Options;
  /** default the profile's (safe) */
  policy?: Policy;
  /** run EXACT42 beside the fast path and attach it */
  audit?: boolean;
  /** 'copy': a pair the lattice cannot lift above Related is 'NotCopy', not resolved further */
  scope?: 'full' | 'copy';
}

export interface XRankRecord {
  index: number;
  /** -1 rejected or unscreened (not compared), else an index into STATES (6 = NotCopy) */
  state: number;
  verdict: XVerdict | 'Unscreened';
  execution: Execution;
  screen: ScreenState;
  route: RouteReading;
  poolDirect: number;
  poolMirror: number;
  rowsScanned: number;
  inliers: number;
  models: number;
  geometryEvidence: number;
  geoMargin: number;
  topology: number;
  structuralLo: number;
  structuralHi: number;
  structuralExact: boolean;
  certifiable: boolean;
  /** -1 when the local channel abstained or was never needed */
  localEvidence: number;
  hammingPairs: number;
  fullPairs: number;
  swapped: boolean;
  certificate: boolean;
  mirrored: boolean;
  explosion: boolean;
  fallbackRan: boolean;
  mixedSelection: boolean;
}

export declare class Engine {
  readonly backend: 'wasm';
  readonly simd: boolean;
  readonly T1_BYTES: number;
  hash(img: ImageLike, opts?: Options, limits?: [number, number, number]): HashResult;
  hash(bytes: Uint8Array | Uint8ClampedArray, w: number, h: number, opts?: Options): HashResult;
  prepare(t1: Uint8Array, t2?: Uint8Array | null, opts?: { strict?: boolean }): Side;
  prepare(wires: Wires, opts?: { strict?: boolean }): Side;
  profile(pcal: Uint8Array): Profile;
  compare(a: SideOrWires, b: SideOrWires, o?: CompareOptions & { json?: false }): Report;
  compare(a: SideOrWires, b: SideOrWires, o: CompareOptions & { json: true }): string;
  screen(a: SideOrWires, b: SideOrWires, o?: { profile?: Profile | Uint8Array; opts?: Options }): Screen;
  /** Screen, then compare the survivors.  `gate` (default true) passes a pair only on
   *  `geo_min_corr` (8) keypoint correspondences or more, so it drops every copy of a work with
   *  fewer keypoints, and some others, that comparator 42 would certify: `gate: false`, or
   *  `xrank` (X2 onwards), keeps those (docs/SEARCH.md §4). */
  rank(query: SideOrWires, candidates: SideOrWires[],
       o?: { gate?: boolean; profile?: Profile | Uint8Array; opts?: Options; raw?: boolean }): RankRecord[] & { records?: Int32Array };
  descriptors(side: SideOrWires, o?: { sketch?: boolean; strongest?: boolean }): Uint32Array;
  /** index side: codes + bands of the 64 strongest keypoints; `query: true`: codes + bands of
   *  every keypoint (≤ 512) and of its mirrored descriptor */
  indexKeys(side: SideOrWires, o?: { query?: boolean; maxKeypoints?: number; mirror?: boolean }): IndexKeys;
  localCodes(side: SideOrWires): Uint32Array;

  /* ---- PAPH-X ---- */

  /** An X profile: `base` a Profile or .pcal bytes (default the shipped CAL-007-PROVISIONAL), `x` the X
   *  artefact bytes (default the shipped X3-PROVISIONAL's schedule bound to that base; 1.1's
   *  X2-PROVISIONAL and 1.0.0's X1-PROVISIONAL, both bound to CAL-004-PROPOSED, are
   *  docs/calibration/X2-PROVISIONAL.pxcl and X1-PROVISIONAL.pxcl).  Check `status()`. */
  xprofile(o?: { base?: Profile | Uint8Array; x?: Uint8Array }): XProfile;
  /** Parse and derive a side once: route, bucket index, anchor order.  `sidecar`: PAX1 bytes a
   *  previous `XSide.sidecar()` returned, used when they match the wires and the profile. */
  xprepare(t1: Uint8Array, t2?: Uint8Array | null, opts?: { strict?: boolean; profile?: XProfile; sidecar?: Uint8Array }): XSide;
  xprepare(wires: Wires, opts?: { strict?: boolean; profile?: XProfile; sidecar?: Uint8Array }): XSide;
  /** The pair screen (§12): never a verdict. */
  xscreen(a: XSideOrWires, b: XSideOrWires, o?: { profile?: XProfile; opts?: Options }): XScreen;
  /** The PAPH-X comparison (§13). */
  xcompare(a: XSideOrWires, b: XSideOrWires, o?: XCompareOptions & { json?: false }): XReport;
  xcompare(a: XSideOrWires, b: XSideOrWires, o: XCompareOptions & { json: true }): string;
  /** One query against many candidates in one call (§14): the route table, the sparse screen, the
   *  cascade on the survivors.  `gate` (default true) skips the cascade for candidates the screen
   *  rejects; `scope` defaults to 'copy'. */
  xrank(query: XSideOrWires, candidates: XSideOrWires[],
        o?: { profile?: XProfile; opts?: Options; policy?: Policy; gate?: boolean; scope?: 'full' | 'copy'; raw?: boolean }): XRankRecord[] & { records?: Int32Array };

  /* ---- PAPH-SI ---- */

  /** An SI profile from its artefact, or the shipped SI4-PROVISIONAL (bound to X3, fitted on the Pixa
   *  chain's artworks hashed in wire 4; 1.1.2's SI3 and SI2, bound to X2, and SI1, bound to X1, are
   *  docs/calibration/SI3-PROVISIONAL.psi, SI2-PROVISIONAL.psi and SI1-PROVISIONAL.psi). */
  siprofile(bytes?: Uint8Array): SIProfile;
  /** The signature of a side: from an XSide (route reused) or from `{ t1, t2 }` alone (no bucket index). */
  sisig(side: XSideOrWires, o?: { profile?: SIProfile; xprofile?: XProfile }): SISignature;
  /** the posting keys of a stored signature */
  sikeys(sig: Uint8Array): number[];
  siquery(side: XSideOrWires, o?: { profile?: SIProfile; xprofile?: XProfile }): SIQuery;
  siindex(): SIIndex;
}

/** Load the module: omitted (paph.wasm / paph-baseline.wasm next to the glue), a URL or path,
 *  the bytes, a Response, a WebAssembly.Module, or a WebAssembly.Instance. */
export declare function init(source?: string | URL | ArrayBuffer | ArrayBufferView | Response |
                             WebAssembly.Module | WebAssembly.Instance): Promise<Engine>;
export declare function simdSupported(): boolean;

export declare const DEFAULTS: Readonly<Required<Options>>;
export declare const STATES: readonly XVerdict[];
export declare const EXECUTIONS: readonly Execution[];
export declare const RANK_FIELDS: number;
export declare const XRANK_FIELDS: number;
export declare const XSCREEN_FIELDS: number;
export declare const ABI: number;
export declare const X_ABI: number;
export declare const SI_ABI: number;
export declare const SI_SIG_BYTES: number;
export declare const SI_FAMILIES: readonly SIFamily[];
/** PAPH-SI in SQLite / D1: the schema and the one query statement (parameters: `siSqlParams`). */
export declare const SI_SQL: { readonly schema: string; readonly query: string };
export declare function siSqlParams(plan: SIPlan, o?: { threshold?: number; budget?: number }): (string | number)[];
/** the wire format hash() writes by default: 4 */
export declare const WIRE_VERSION: number;
export declare const KEYS_VERSION: number;
export declare const backend: 'wasm';
