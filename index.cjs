/* CommonJS entry for @pixagram/paph-x — PAPH 4.2 with PAPH-X.
   One wire (tier 1 + tier 2), one comparator on the entry (42), one shipped
   calibration; comparator 41 stays reachable as compare41/cal41 so verdicts
   issued under 4.1 remain reproducible.  The WebAssembly engine is an ES module
   (it finds its .wasm through import.meta.url): `await paph.wasm()` loads it
   from CommonJS too. */
const wire = require('./src/wire.cjs');
const paph = require('./src/paph-js.cjs');

/* The comparator layer hashes { px, w, h }; the entry takes every shape the wire layer and the
   WebAssembly engine take — ImageData, { pixels, width, height }, or (bytes, width, height). */
function image(a, b, c) {
  if (a && a.px && a.w && a.h) return a;
  if (a && a.data && a.width && a.height) return { px: a.data, w: a.width, h: a.height };
  if (a && a.pixels && a.width && a.height) return { px: a.pixels, w: a.width, h: a.height };
  if (a && typeof b === 'number' && typeof c === 'number') return { px: a, w: b, h: c };
  return a;
}

module.exports = Object.assign({}, paph, {
  /* hash with a profile's limits enforced first (§16): (image, opts?, limits?) or (bytes, w, h, opts?) */
  hash: function (a, b, c, d) {
    return typeof b === 'number' ? paph.hash(image(a, b, c), d) : paph.hash(image(a), b, c);
  },
  /* the wire layer, for callers that only want fingerprints */
  wire: wire,
  Config: wire.Config,
  Paph: wire.Paph,
  parseT1: wire.parseT1,
  parseT2: wire.parseT2,
  WIRE_VERSION: wire.VERSION,
  T1_BYTES: wire.T1_BYTES,
  KP_MAX: wire.KP_MAX,
  F_KPQ: wire.F_KPQ,
  SECTIONS: wire.SECTIONS,
  SECTION_OFFSETS: wire.SECTION_OFFSETS,
  DEFAULT_CONFIG: wire.DEFAULT_CONFIG,
  /* the WebAssembly engine: the optimized Rust reference, byte-identical */
  wasm: async function (source) {
    const m = await import('./wasm/paph.js');
    return m.init(source);
  }
});
