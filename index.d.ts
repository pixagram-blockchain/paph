/** @pixagram/paph-x — PAPH 4.2 with PAPH-X: integer-only perceptual hash for pixel-art copy detection.
 *  Wire: SPEC-003 (format 3).  Comparator: SPEC-004.2 (comparator 42).
 *  The WebAssembly engine's own types: wasm/paph.d.ts (`@pixagram/paph-x/wasm`). */

export type Scoring = 'gate' | 'weighted';
export type Evidence = 'lift' | 'proportion';
export type RagEndpoint = 'quantile' | 'rank';
export type VerdictName = 'Identical' | 'Copy' | 'Suspected' | 'Related' | 'Unrelated';
export type ChannelName =
  | 'dct' | 'local' | 'shape' | 'topology' | 'runs' | 'palette' | 'silhouette';

export interface ConfigInit {
  // --- hash time: these change the wire ---
  foldMatte?: boolean;
  divideUpscale?: boolean;
  matteTol?: number;
  peakRadius?: number;
  foldInvert?: boolean;
  localWindows?: [number, number];
  localCount?: number;
  /** Tier-2 budget, at most 512 (SPEC-004.2 §1).  Default 512. */
  kpCount?: number;
  /** Keypoint selection rule (SPEC-004.2 §3): 0 = the 4.1 strength/grid rule,
   *  1 = the 4.2 quality score.  Default 1.  Hash `{ kpCount: 256, kpSelect: 0 }`
   *  to reproduce a 4.1 wire exactly. */
  kpSelect?: 0 | 1;
  sketchCount?: number;
  // --- compare time: free to re-derive without re-hashing ---
  hammingT?: number;
  evidence?: Evidence;
  confidenceAt?: number;
  scoring?: Scoring;
  ragEndpoint?: RagEndpoint;
  geoEnabled?: boolean;
  geoConfAt?: number;
  geoEps?: number;
  geoMinCorr?: number;
  mirrorHypothesis?: boolean;
}

/** Frozen on construction — derive with `.with()` rather than mutating. */
export declare class Config implements Required<ConfigInit> {
  constructor(over?: ConfigInit);
  readonly foldMatte: boolean;
  readonly divideUpscale: boolean;
  readonly matteTol: number;
  readonly peakRadius: number;
  readonly foldInvert: boolean;
  readonly localWindows: [number, number];
  readonly localCount: number;
  readonly kpCount: number;
  readonly kpSelect: 0 | 1;
  readonly sketchCount: number;
  readonly hammingT: number;
  readonly evidence: Evidence;
  readonly confidenceAt: number;
  readonly scoring: Scoring;
  readonly ragEndpoint: RagEndpoint;
  readonly geoEnabled: boolean;
  readonly geoConfAt: number;
  readonly geoEps: number;
  readonly geoMinCorr: number;
  readonly mirrorHypothesis: boolean;
  with(over: ConfigInit): Config;
  toJSON(): Required<ConfigInit>;
  static defaults(): Config;
  static validate(c: ConfigInit): string | null;
}

export interface Fingerprint {
  /** always exactly 3952 bytes */
  t1: Uint8Array;
  /** 32 + 40n bytes, n <= 512 */
  t2: Uint8Array;
  kpCount: number;
  crc: number;
  width: number;
  height: number;
}

export interface Channel {
  /** 0..10000, chance-corrected */
  value: number;
  raw: number;
  /** what this measurement scores against a deliberately wrong answer */
  control: number;
  /** false means the channel ABSTAINED and contributed nothing */
  measurable: boolean;
  controlRan: boolean;
  note: string;
}

export interface GeoChannel extends Channel {
  inliers: number;
  chanceInliers: number;
  accepted: number;
  hypothesis: 'direct' | 'mirrored' | 'none';
  /** recovered similarity scale, Q16 */
  scaleQ16?: number;
}

export interface Verdict {
  verdict: VerdictName;
  /** human-readable reason, e.g. "certified — structure and geometry agree" */
  class: string;
  /** which axes certified, if any */
  basis: Array<'structural' | 'geometric' | 'bytes'>;
  /** 0..10000 */
  structural: number;
  /** 0..10000; 0 when the geometric channel abstained */
  geometric: number;
  weighted: number;
  gate: number;
  identical: boolean;
  /** false when too few secondary channels were measurable to corroborate */
  structuralCertifiable: boolean;
  /** true when the two wires were exchanged to reach canonical argument order */
  swapped: boolean;
  channels: Record<ChannelName, Channel>;
  geo: GeoChannel;
  abstained: string[];
  report: {
    dihedral: string | null;
    inverted: boolean;
    mirrored: boolean | null;
    recoveredScale?: number;
    inliers: number;
    chanceInliers: number;
    palette: { relation: string; exact?: number; of?: number } | string;
    sameExport: boolean | null;
    geoHypothesis: string;
    evidenceBoth?: { lift: number; proportion: number };
  };
}

export type ImageInput =
  | ImageData
  | { px: Uint8Array | Uint8ClampedArray; w: number; h: number }
  | { pixels: Uint8Array | Uint8ClampedArray; width: number; height: number };

export declare class Paph {
  constructor(config?: Config | ConfigInit);
  readonly config: Config;
  readonly backend: 'js' | 'wasm';
  hash(image: ImageInput): Fingerprint;
  hash(bytes: Uint8Array | Uint8ClampedArray, width: number, height: number): Fingerprint;
  compare(a: Fingerprint, b: Fingerprint): Verdict;
  /** a new engine with a derived configuration; nothing is mutated */
  with(over: ConfigInit): Paph;
  parseTier1?(bytes: Uint8Array): unknown;
  parseTier2?(bytes: Uint8Array): unknown;
}

/** The WIRE format version — 3, and it stays 3.  Not the package version:
 *  measurements should not move every time a judgement does.  (The wire layer
 *  on its own, `@pixagram/paph-x/wire`, is typed by src/wire.d.ts.) */
export declare const WIRE_VERSION: 3;
/** 3952 */
export declare const T1_BYTES: number;
export declare const SECTIONS: ReadonlyArray<{ id: number; name: string; len: number }>;
export declare const SECTION_OFFSETS: Readonly<Record<string, number>>;
export declare const DEFAULT_CONFIG: Readonly<Required<ConfigInit>>;

export declare function parseT1(bytes: Uint8Array): unknown;
export declare function parseT2(bytes: Uint8Array): unknown;

/**
 * The WebAssembly engine: the optimized Rust reference, SIMD128 when the
 * runtime has it (every current browser, Node >= 16.4, Deno, Cloudflare
 * Workers), a baseline build otherwise.  Same wires and same reports as the
 * JavaScript engine, byte for byte — `npm run test:wasm` is the check that says
 * so.  Also importable on its own as `@pixagram/paph-x/wasm` (`init`).
 */
export declare function wasm(
  source?: string | URL | ArrayBuffer | ArrayBufferView | Response | WebAssembly.Module | WebAssembly.Instance
): Promise<import('./wasm/paph.js').Engine>;


/* ------------------------------------------------------------------------
 * PAPH 4.2 — comparator 42 (SPEC-004.2).
 *
 * The wire FORMAT above is unchanged: the same 3952-byte tier 1 and 32 + 40n
 * tier 2 feed the comparator.  What moved is the tier-2 budget — 256 keypoints
 * to 512, which the format always allowed — and the selection, matching, voting
 * and diversity machinery that makes 512 points worth having.
 *
 * What the comparator adds is a CALIBRATION PROFILE — an immutable container-3
 * artefact with its own SHA-256 identity — and a verdict lattice that scores
 * each channel against its own null and then through that channel's own table.
 *
 * Comparator 41 is FROZEN, not deleted: `compare41` / `screen41` / `cal41` still
 * compute, so a verdict issued under 4.1 stays reproducible.  A container-1
 * (comparator-4) artefact still decodes, so an old verdict's provenance can be
 * read; nothing here will compute with it.
 * ---------------------------------------------------------------------- */

/** A monotone calibration table: strictly increasing x from 0 to 10000. */
export type Lut = ReadonlyArray<readonly [number, number]>;

export interface Profile {
  /** 3 — nine per-channel tables plus the geometric-diversity table */
  container: number;
  /** 42 */
  comparator: number;
  name: Uint8Array;
  evidenceRule: number; scoringRule: number; ragEndpoint: number; gridG: number;
  hammingT: number; confidenceAt: number; geoConfAt: number; geoEps: number;
  geoMinCorr: number; minModelInliers: number; maxModels: number;
  repExtremeAt: number; coverageFloor: number; minSecondaries: number;
  /** STRUCT_IDENTICAL, STRUCT_STRONG, STRUCT_MODERATE, STRUCT_WEAK, STRUCT_SOLO,
   *  GEO_STRONG, GEO_WEAK, GEO_SOLO_INLIERS, topology_dominant_at */
  thresholds: number[];
  /** local, shape, topology, runs, dct, palette, silhouette */
  weights: number[];
  /** max_width, max_height, max_pixels */
  limits: number[];
  // ---- SPEC-004.2, container 3 only.  A container-1/2 artefact decodes to
  // these defaults, which ARE the 4.1 behaviour, so old bytes do not move. ----
  /** the Lowe ratio as a fraction: accept when d1 * loweDen < loweNum * d2 */
  loweNum: number; loweDen: number;
  /** §4 the ABSOLUTE margin beside the ratio: d2 - d1 >= loweMargin.  0 disables. */
  loweMargin: number;
  /** §4 the hard descriptor-distance ceiling */
  hamMax: number;
  /** §6 correspondences that must share the winning Hough cell before a fit */
  minPeakMembers: number;
  /** §7 soft exclusion around a consumed keypoint, as a percentage of the
   *  descriptor patch footprint at that keypoint's own level.  0 disables. */
  exclPct: number;
  /** §3 the keypoint-selection rule this calibration was derived on */
  kpSelect: number;
  /** §5 soft scale binning in the Hough vote (0 = the 4.1 hard bucket) */
  scaleSoft: number;

  lutLocal: Lut; lutGeometry: Lut; lutDiversity: Lut;
  lutDct: Lut; lutShape: Lut; lutTopology: Lut;
  lutRuns: Lut; lutPalette: Lut; lutSilhouette: Lut;
  /** §8 — the GEOMETRIC diversity multiplier.  Deliberately not `lutDiversity`:
   *  that one modulates the local channel by its own repetition reading. */
  lutGeoDiversity: Lut;
}

export interface ModelRec {
  r00: number; r10: number; tx: number; ty: number;
  scaleQ16: number; mirror: boolean; inliers: number;
  /** §6 — median squared residual over this model's inliers */
  medianErr: number;
  /** total vote weight carried by this model's inliers */
  confSum: number;
}

/** §8 — how much INDEPENDENT evidence the geometry rests on.  All 0..10000. */
export interface DiversityRec {
  spatial: number;
  scale: number;
  model: number;
  descriptor: number;
  /** 40 spatial + 25 scale + 20 model + 15 descriptor */
  combined: number;
  /** lutGeoDiversity(combined); 10000 (neutral) when no model was accepted */
  multiplier: number;
}

/** §2 — which selection rule built each side's tier 2. */
export interface SelectionRec {
  /** 0 = the 4.1 strength/grid rule, 1 = the 4.2 quality score */
  a: 0 | 1;
  b: 0 | 1;
  /** true when the two sides were hashed under different rules.  A WARNING,
   *  not a refusal: the comparison is well defined, but recall measured across
   *  that boundary is not recall measured within either.  Re-hash a corpus
   *  before re-deriving its thresholds. */
  mixed: boolean;
}

export interface CoverageRec {
  g: number; occupied: number; coverage: number;
  bboxCells: number; concentration: number; counts: ArrayLike<number>;
}

export interface Report {
  /** 42 from `compare`, 41 from `compare41` */
  comparator: number;
  verdict: 'Identical' | 'Copy' | 'Suspected' | 'Related' | 'Unrelated' | 'Indeterminate';
  class: string;
  /** what carried the verdict: 'bytes', 'structural', 'geometric', and any rule that fired */
  basis: string[];
  /** populated only for Indeterminate: CORRUPT, PROFILE_MISMATCH, PROFILE_UNSUPPORTED, LIMIT */
  reasons: string[];
  structural: number;
  certifiable: boolean;
  v3: Verdict | null;
  local: unknown;
  models: ModelRec[];
  topology: number;
  totalInliers: number;
  coverage: CoverageRec | null;
  geometryEvidence: number;
  geoMeasurable: boolean;
  geoRaw: number; geoCtl: number; geoMargin: number; geoCtlMember: string;
  swapped: boolean;
  calibration: string;
  calibrationId: string;
  /** §A1 — the weak-signal inlier count, 0 when a model was accepted */
  geoWeakInliers: number;
  /** SPEC-004.2 §8; absent on a comparator-41 report */
  diversity?: DiversityRec;
  /** SPEC-004.2 §6 — median squared residual per accepted model */
  medianErr?: number[];
  /** SPEC-004.2 §2 — selection provenance of each side's tier 2 */
  selection?: SelectionRec;
  /** tier-2 record counts actually compared */
  kpA?: number;
  kpB?: number;
  screen: Screen | null;
}

/** §A4 — correspondence pools only, no verification, no verdict. */
export interface Screen {
  /** a rejected pair is UNSCREENED, never Unrelated */
  pass: boolean;
  poolDirect: number;
  poolMirror: number;
}

/** Hash with the profile's declared limits enforced first (§16). */
export declare function hash(
  image: ImageInput, opts?: ConfigInit, limits?: number[]
): Fingerprint;
export declare function hash(
  bytes: Uint8Array | Uint8ClampedArray, width: number, height: number, opts?: ConfigInit
): Fingerprint;

/** The comparator. Requires a container-3 / comparator-42 profile. */
export declare function compare(
  aT1: Uint8Array, aT2: Uint8Array | null | undefined,
  bT1: Uint8Array, bT2: Uint8Array | null | undefined,
  opts: ConfigInit | undefined, profile: Profile,
  hashProfileA?: Uint8Array, hashProfileB?: Uint8Array
): Report;

/** The stage-1 screen: cheap, advisory, and never a verdict. */
export declare function screen(
  aT1: Uint8Array, aT2: Uint8Array | null | undefined,
  bT1: Uint8Array, bT2: Uint8Array | null | undefined,
  opts: ConfigInit | undefined, profile: Profile
): Screen;

/** CAL-004-PROPOSED — the shipped calibration, pending M7 validation. */
export declare function cal(): Profile;
export declare function calibration(): Profile;

/* ---- comparator 41, frozen: for reproducing verdicts issued under 4.1 ---- */

/** Requires a container-2 / comparator-41 profile — `cal41()`. */
export declare function compare41(
  aT1: Uint8Array, aT2: Uint8Array | null | undefined,
  bT1: Uint8Array, bT2: Uint8Array | null | undefined,
  opts: ConfigInit | undefined, profile: Profile,
  hashProfileA?: Uint8Array, hashProfileB?: Uint8Array
): Report;
export declare function screen41(
  aT1: Uint8Array, aT2: Uint8Array | null | undefined,
  bT1: Uint8Array, bT2: Uint8Array | null | undefined,
  opts: ConfigInit | undefined, profile: Profile
): Screen;
/** CAL-003-PROPOSED — the comparator-41 calibration. */
export declare function cal41(): Profile;

/** A container-1 / comparator-4 profile, for READING legacy artefacts only. */
export declare function legacyCal001(): Profile;

export declare function profileEncode(p: Profile): Uint8Array;
export declare function profileDecode(bytes: Uint8Array): Profile;
/** SHA-256 over the encoded artefact — the identity a verdict cites. */
export declare function profileId(p: Profile): Uint8Array;
export declare function profileName(p: Profile): string;

export declare const VERSION: '4.2';
export declare const COMPARATOR: 42;
export declare const CONTAINER: 3;
/** the tier-2 keypoint ceiling: 512 */
export declare const KP_MAX: 512;
/** tier-1 flag bit set when tier 2 was built by the 4.2 quality selector */
export declare const F_KPQ: 32;

/** Evaluate a calibration table (piecewise linear, 0..10000) at x. */
export declare function lutEval(lut: Lut, x: number): number;
/** A profile's table for one structural channel (identity for one it does not carry). */
export declare function lutChannel(profile: Profile, channel: ChannelName): Lut;

declare const _default: Record<string, unknown> & { wasm: typeof wasm };
export default _default;
