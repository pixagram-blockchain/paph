/** @pixagram/paph-x/comparator — comparator 42 (SPEC-004.2) with comparator 41 frozen beside it,
 *  calibration profiles, and the hash that enforces a profile's limits.  Shared types live in
 *  ../index.d.ts. */
export {
  VERSION, COMPARATOR, CONTAINER, compare, screen, cal, calibration,
  compare41, screen41, cal41, legacyCal001,
  profileEncode, profileDecode, profileId, profileName, lutEval, lutChannel,
} from '../index.js';
export type { ConfigInit, Fingerprint, ImageInput, Profile, Report, Screen, Lut } from '../index.js';
import type { ConfigInit, Fingerprint, Profile } from '../index.js';

/** Hash with a profile's limits enforced first (§16).  This layer takes { px, w, h } only;
 *  the package entry also takes ImageData and (bytes, width, height). */
export declare function hash(
  image: { px: Uint8Array | Uint8ClampedArray; w: number; h: number }, opts?: ConfigInit, limits?: number[]
): Fingerprint;

export declare const backend: 'js';
/** throws a RangeError naming what is wrong with the artefact; returns nothing when it is valid */
export declare function profileValidate(p: Profile): void;
/** the first 8 bytes of profileId as hex — the `calibrationId` a report cites */
export declare function profileIdHex16(p: Profile): string;

declare const _default: Record<string, unknown>;
export default _default;
