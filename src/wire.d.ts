/** @pixagram/paph-x/wire — the v3 wire on its own (SPEC-003): fingerprints, the v3 comparator,
 *  serialisation.  Shared types live in ../index.d.ts. */
import type { ConfigInit, Fingerprint, ImageInput, Verdict } from '../index.js';

export { Config, Paph, parseT1, parseT2, T1_BYTES, KP_MAX, F_KPQ, SECTIONS, SECTION_OFFSETS, DEFAULT_CONFIG } from '../index.js';
export type { ConfigInit, Fingerprint, ImageInput, Verdict } from '../index.js';

/** the wire format: 3 */
export declare const VERSION: 3;
export declare const WIRE_VERSION: 3;
/** bytes per Tier-2 keypoint record: 40 */
export declare const KP_REC: 40;
export declare const DEFAULTS: Readonly<Required<ConfigInit>>;
export declare const THRESHOLDS: Readonly<Record<string, number>>;
export declare const WEIGHTS: Readonly<Record<string, number>>;
export declare const backend: 'js';

export declare function hash(image: ImageInput, opts?: ConfigInit): Fingerprint;
export declare function hash(bytes: Uint8Array | Uint8ClampedArray, width: number, height: number, opts?: ConfigInit): Fingerprint;
/** the v3 comparator: two fingerprints (or Tier-1 bytes) in, a v3 verdict out */
export declare function compare(a: Fingerprint | Uint8Array, b: Fingerprint | Uint8Array, opts?: ConfigInit): Verdict;
export declare function serializeT1(parsed: unknown): Uint8Array;
export declare function serializeT2(keypoints: unknown[], t1crc: number, maxDim: number, xmax: number, select?: number): Uint8Array;
export declare function selectGrid(keypoints: unknown[], want: number): unknown[];
export declare function selectQuality(keypoints: unknown[], want: number, ...rest: unknown[]): unknown[];

declare const _default: Record<string, unknown>;
export default _default;
