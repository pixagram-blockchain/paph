/**
 * @pixagram/paph-x — PAPH 4.2 with PAPH-X.
 *
 * One wire, one comparator on the entry, one shipped calibration — and two
 * engines that agree on all of it byte for byte: JavaScript (this entry) and
 * the optimized Rust reference as WebAssembly (`wasm()`, or
 * `@pixagram/paph-x/wasm`).
 *
 *     import { hash, compare, cal } from '@pixagram/paph-x';
 *
 *     const profile = cal();
 *     const a = hash(imageA, {}, profile.limits);
 *     const b = hash(imageB, {}, profile.limits);
 *     compare(a.t1, a.t2, b.t1, b.t2, {}, profile).verdict;   // 'Copy'
 *
 * The same thing, 5–70× faster to hash and 10–20× faster to compare:
 *
 *     import { wasm } from '@pixagram/paph-x';
 *     const paph = await wasm();
 *     paph.compare(paph.hash(imageA), paph.hash(imageB)).verdict;   // 'Copy'
 *
 * Comparator 41 is FROZEN, not deleted: `compare41` / `cal41` still compute, so
 * a verdict issued under 4.1 stays reproducible.
 */
import wire from './src/wire.js';
import paph from './src/paph-js.js';

export const VERSION = paph.VERSION;
export const COMPARATOR = paph.COMPARATOR;
export const CONTAINER = paph.CONTAINER;

/* The comparator layer hashes { px, w, h }; the entry takes every shape the wire layer and the
   WebAssembly engine take — ImageData, { pixels, width, height }, or (bytes, width, height). */
function image(a, b, c) {
  if (a && a.px && a.w && a.h) return a;
  if (a && a.data && a.width && a.height) return { px: a.data, w: a.width, h: a.height };
  if (a && a.pixels && a.width && a.height) return { px: a.pixels, w: a.width, h: a.height };
  if (a && typeof b === 'number' && typeof c === 'number') return { px: a, w: b, h: c };
  return a;
}

/** Hash with a profile's limits enforced first (§16): hash(image, opts?, limits?) or hash(bytes, w, h, opts?). */
export function hash(a, b, c, d) {
  return typeof b === 'number' ? paph.hash(image(a, b, c), d) : paph.hash(image(a), b, c);
}
export const compare = paph.compare;
export const screen = paph.screen;
export const cal = paph.cal;
export const calibration = paph.calibration;
export const profileEncode = paph.profileEncode;
export const profileDecode = paph.profileDecode;
export const profileId = paph.profileId;
export const profileName = paph.profileName;
export const legacyCal001 = paph.legacyCal001;

/** comparator 41, frozen — for reproducing verdicts issued under 4.1 */
export const compare41 = paph.compare41;
export const screen41 = paph.screen41;
export const cal41 = paph.cal41;
export const lutEval = paph.lutEval;
export const lutChannel = paph.lutChannel;

/** the wire layer */
export const Config = wire.Config;
export const Paph = wire.Paph;
export const parseT1 = wire.parseT1;
export const parseT2 = wire.parseT2;
export const WIRE_VERSION = wire.VERSION;
export const T1_BYTES = wire.T1_BYTES;
export const KP_MAX = wire.KP_MAX;
export const F_KPQ = wire.F_KPQ;
export const SECTIONS = wire.SECTIONS;
export const SECTION_OFFSETS = wire.SECTION_OFFSETS;
export const DEFAULT_CONFIG = wire.DEFAULT_CONFIG;

/**
 * The WebAssembly engine (wasm/paph.js).  `source` as for its `init`: omitted
 * (the .wasm next to the glue, SIMD128 or baseline by what the runtime has), a
 * URL or path, the bytes, a Response, or a compiled WebAssembly.Module (what a
 * Cloudflare Worker imports).
 */
export async function wasm(source) {
  const m = await import('./wasm/paph.js');
  return m.init(source);
}

export default { ...wire, ...paph, hash, wasm, wire };
