/**
 * Cross-engine parity for the 4.2 comparator (SPEC-004.2), with comparator 41
 * kept as a second, frozen arm.
 *
 * test/parity.mjs proves the two engines hash and compare identically under
 * comparator 3.  This file proves comparator 4: the SAME wires (hashed once,
 * by the JS engine) go through the native Rust reference (`paphcli` mode 'V')
 * and through the JavaScript port, and the two full reports must be equal
 * field for field — verdict, basis, rule firings, every evidence number,
 * every model, every coverage cell count.
 *
 * Build the reference first:  cargo build --release --manifest-path rust/Cargo.toml
 * Then:                       npm run test:native
 */
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const JS = require('../src/wire.cjs');
const paph = require('../src/paph-js.cjs');

const here = dirname(fileURLToPath(import.meta.url));
const BIN = join(here, '..', 'rust', 'target', 'release', process.platform === 'win32' ? 'paphcli.exe' : 'paphcli');
if (!existsSync(BIN)) {
  console.error('paphcli not found at ' + BIN);
  console.error('build it first:  cargo build --release --manifest-path rust/Cargo.toml');
  process.exit(2);
}

const C = { g: '\u001b[32m', r: '\u001b[31m', d: '\u001b[2m', x: '\u001b[0m' };
let pass = 0, fail = 0;
const ok = (c, name, extra) => {
  if (c) { pass++; console.log(`  ${C.g}PASS${C.x} ${name}` + (extra ? ` ${C.d}${extra}${C.x}` : '')); }
  else { fail++; console.log(`  ${C.r}FAIL${C.x} ${name}` + (extra ? ` ${C.d}${extra}${C.x}` : '')); }
};

function work(w, h, seed) {
  const px = new Uint8Array(w * h * 4);
  let s = seed;
  const R = () => { s = (s * 1103515245 + 12345) & 0x7fffffff; return s / 0x7fffffff; };
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const o = (y * w + x) << 2;
    px[o] = (x * 7 + y * 3 + seed) % 251; px[o + 1] = (y * 11 + x * 5) % 253;
    px[o + 2] = ((x ^ y) * 13) % 247; px[o + 3] = 255;
  }
  for (let i = 0; i < 40; i++) {
    const bx = (R() * (w - 10)) | 0, by = (R() * (h - 10)) | 0;
    const r = (R() * 255) | 0, g = (R() * 255) | 0, b = (R() * 255) | 0;
    for (let dy = 0; dy < 6; dy++) for (let dx = 0; dx < 6; dx++) {
      const o = ((by + dy) * w + bx + dx) << 2;
      px[o] = r; px[o + 1] = g; px[o + 2] = b;
    }
  }
  return { px, w, h };
}
const mirror = ({ px, w, h }) => {
  const o = new Uint8Array(w * h * 4);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const s = (y * w + (w - 1 - x)) << 2, d = (y * w + x) << 2;
    o[d] = px[s]; o[d + 1] = px[s + 1]; o[d + 2] = px[s + 2]; o[d + 3] = px[s + 3];
  }
  return { px: o, w, h };
};
const crop = ({ px, w, h }) => {
  const nw = (w * .7) | 0, nh = (h * .7) | 0, x0 = ((w - nw) / 2) | 0, y0 = ((h - nh) / 2) | 0;
  const o = new Uint8Array(nw * nh * 4);
  for (let y = 0; y < nh; y++) for (let x = 0; x < nw; x++) {
    const s = ((y0 + y) * w + x0 + x) << 2, d = (y * nw + x) << 2;
    o[d] = px[s]; o[d + 1] = px[s + 1]; o[d + 2] = px[s + 2]; o[d + 3] = px[s + 3];
  }
  return { px: o, w: nw, h: nh };
};
const up2 = ({ px, w, h }) => {
  const nw = w * 2, nh = h * 2, o = new Uint8Array(nw * nh * 4);
  for (let y = 0; y < nh; y++) for (let x = 0; x < nw; x++) {
    const s = ((y >> 1) * w + (x >> 1)) << 2, d = (y * nw + x) << 2;
    o[d] = px[s]; o[d + 1] = px[s + 1]; o[d + 2] = px[s + 2]; o[d + 3] = px[s + 3];
  }
  return { px: o, w: nw, h: nh };
};

function rustCmp(mode, a, b) {
  const head = Buffer.alloc(17);
  head[0] = mode.charCodeAt(0);
  head.writeUInt32LE(a.t1.length, 1);
  head.writeUInt32LE(a.t2.length, 5);
  head.writeUInt32LE(b.t1.length, 9);
  head.writeUInt32LE(b.t2.length, 13);
  const input = Buffer.concat([head, Buffer.from(a.t1), Buffer.from(a.t2), Buffer.from(b.t1), Buffer.from(b.t2)]);
  const r = spawnSync(BIN, [], { input, maxBuffer: 1 << 24 });
  if (r.status !== 0) throw new Error('paphcli failed: ' + r.stderr);
  return JSON.parse(r.stdout.toString());
}
function pickCov(c) {
  return { g: c.g, occupied: c.occupied, coverage: c.coverage, bboxCells: c.bboxCells,
           concentration: c.concentration, counts: Array.from(c.counts) };
}
function pick41(r) {
  const base = pickReport(r);
  base.geoWeakInliers = r.geoWeakInliers;
  return base;
}
/* SPEC-004.2 adds §8 diversity, the per-model residuals and the selection
   provenance to the surface; all three are compared field for field. */
function pick42(r) {
  const base = pick41(r);
  base.diversity = r.diversity;
  base.medianErr = r.medianErr;
  base.selection = r.selection;
  base.kpA = r.kpA; base.kpB = r.kpB;
  base.models = r.models.map(m => ({ r00: m.r00, r10: m.r10, tx: m.tx, ty: m.ty,
                                     scaleQ16: m.scaleQ16, mirror: m.mirror,
                                     inliers: m.inliers }));
  return base;
}
function pickReport(r) {
  return {
    comparator: r.comparator, verdict: r.verdict, class: r.class, basis: r.basis,
    reasons: r.reasons, structural: r.structural, certifiable: r.certifiable,
    /* the Rust surface flattens the v3 reading to one field; the JavaScript
       one carries the whole Verdict object.  Read whichever is present — this
       line was comparing '' against a real verdict on every case. */
    v3Verdict: r.v3 ? r.v3.verdict : (r.v3Verdict || ''),
    topology: r.topology, totalInliers: r.totalInliers,
    geometryEvidence: r.geometryEvidence, geoMeasurable: r.geoMeasurable, geoRaw: r.geoRaw, geoCtl: r.geoCtl,
    geoMargin: r.geoMargin, geoCtlMember: r.geoCtlMember, swapped: r.swapped,
    calibration: r.calibration, calibrationId: r.calibrationId,
    models: r.models.map(m => ({ r00: m.r00, r10: m.r10, tx: m.tx, ty: m.ty,
                                 scaleQ16: m.scaleQ16, mirror: m.mirror, inliers: m.inliers })),
    coverage: r.coverage ? pickCov(r.coverage) : null,
    local: r.local ? {
      measurable: r.local.measurable, note: r.local.note, matches: r.local.matches,
      cmax: r.local.cmax, w: r.local.w, cap: r.local.cap, ctlW: r.local.ctlW,
      ctlN: r.local.ctlN, ctlMember: r.local.ctlMember, liftRaw: r.local.liftRaw,
      liftCtl: r.local.liftCtl, margin: r.local.margin, evidence: r.local.evidence,
      propRaw: r.local.propRaw, propCtl: r.local.propCtl, propMargin: r.local.propMargin,
      diversity: r.local.diversity, dA: r.local.dA, dB: r.local.dB,
      coverageA: pickCov(r.local.coverageA), coverageB: pickCov(r.local.coverageB)
    } : null
  };
}
function diff(a, b, path, out) {
  if (a === b) return;
  if (a && b && typeof a === 'object' && typeof b === 'object') {
    const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
    for (const k of keys) diff(a[k], b[k], path + '.' + k, out);
    return;
  }
  out.push(path + ': rust=' + JSON.stringify(a) + ' js=' + JSON.stringify(b));
}

console.log('\n\u001b[1mpaph-js 4.2 cross-engine parity — native Rust reference vs JavaScript\u001b[0m');
const A = JS.hash(work(128, 128, 42));
const B = JS.hash(work(128, 128, 1337));
const M = JS.hash(mirror(work(128, 128, 42)));
const K = JS.hash(crop(work(128, 128, 42)));
const U = JS.hash(up2(work(128, 128, 42)));
const cases = [
  ['self (42, 42)', A, A],
  ['unrelated (42, 1337)', A, B],
  ['mirrored copy (42, mirror 42)', A, M],
  ['cropped copy (42, crop 42)', A, K],
  ['upscaled copy (42, 2x 42)', A, U],
  ['swapped order (1337, 42)', B, A]
];
console.log('\ncomparator 42 (CAL-007-PROVISIONAL) — the shipped entry');
for (const [name, a, b] of cases) {
  const ru = pick42(rustCmp('X', a, b));
  const js = pick42(paph.compare(a.t1, a.t2, b.t1, b.t2, {}, paph.cal()));
  const d = [];
  diff(ru, js, '', d);
  ok(d.length === 0, name,
     d.length ? d.slice(0, 3).join(' | ')
              : ru.verdict + ' [' + ru.basis.join(',') + '] inliers=' + ru.totalInliers +
                ' D=' + ru.diversity.combined + ' x' + ru.diversity.multiplier);
  if (d.length) console.log('    ' + d.slice(0, 12).join('\n    '));
}

console.log('\ncomparator 41 (CAL-003-PROPOSED) — frozen, and still reproducible');
for (const [name, a, b] of cases) {
  const ru = pick41(rustCmp('W', a, b));
  const js = pick41(paph.compare41(a.t1, a.t2, b.t1, b.t2, {}, paph.cal41()));
  delete ru.screen;
  const d = [];
  diff(ru, js, '', d);
  ok(d.length === 0, name,
     d.length ? d.slice(0, 3).join(' | ')
              : ru.verdict + ' [' + ru.basis.join(',') + '] weak=' + ru.geoWeakInliers);
  if (d.length) console.log('    ' + d.slice(0, 12).join('\n    '));
}

/* The 4.1 wire still parses, still compares, and says so. */
{
  const legacy = JS.hash(work(128, 128, 42), { kpCount: 256, kpSelect: 0 });
  const r = paph.compare(A.t1, A.t2, legacy.t1, legacy.t2, {}, paph.cal());
  ok(r.selection.mixed === true && r.verdict !== 'Indeterminate',
     'mixed 4.1/4.2 selection is flagged, never refused',
     r.verdict + ' sel a=' + r.selection.a + ' b=' + r.selection.b +
     ' kp ' + r.kpA + '/' + r.kpB);
}

console.log('\n\u001b[1m' + pass + ' passed, ' + (fail ? C.r : '') + fail + ' failed\u001b[0m');
process.exit(fail ? 1 : 0);
