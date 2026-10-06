/**
 * Engine timings: the JavaScript reference, the previous WebAssembly module
 * (paph-js 4.2.2, when its checkout is next to this one), and this one — SIMD
 * and baseline.  Prints a markdown table.
 *
 *     node test/bench-engines.mjs [--old ../paph-js] [--reps 5]
 */
import { createRequire } from 'node:module';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { dirname, join, resolve } from 'node:path';
import { init } from '../wasm/paph.js';
import { work, scene, sprite, mirror, crop, paste, upscale } from './corpus.mjs';

const require = createRequire(import.meta.url);
const V = require('../src/paph-js.cjs');
const here = dirname(fileURLToPath(import.meta.url));
const arg = (k, d) => { const i = process.argv.indexOf(k); return i > 0 ? process.argv[i + 1] : d; };
const REPS = +arg('--reps', 5);
const oldDir = resolve(arg('--old', join(here, '..', '..', 'paph-js')));

const simd = await init(join(here, '..', 'wasm', 'paph.wasm'));
const base = await init(join(here, '..', 'wasm', 'paph-baseline.wasm'));
let old = null;
if (existsSync(join(oldDir, 'wasm', 'paph-js-wasm.js'))) {
  const m = await import(pathToFileURL(join(oldDir, 'wasm', 'paph-js-wasm.js')).href);
  await m.init(readFileSync(join(oldDir, 'wasm', 'paph.wasm')));
  old = { m, eng: new m.Paph() };
}

/* best-of-REPS of the mean over n calls: noise on a shared machine is one-sided */
function time(fn, n) {
  fn();
  let best = Infinity;
  for (let r = 0; r < REPS; r++) {
    const t = performance.now();
    for (let i = 0; i < n; i++) fn();
    best = Math.min(best, (performance.now() - t) / n);
  }
  return best;
}
const ms = v => v === null ? '—' : v < 1 ? v.toFixed(3) : v < 10 ? v.toFixed(2) : v.toFixed(1);
const x = (a, b) => (a === null || b === null) ? '' : ` (${(a / b).toFixed(1)}×)`;

const S = scene(288, 200, 9137), SB = scene(1024, 768, 31), W = work(512, 384, 7), P = sprite(96, 96, 5);
const images = [['sprite 96×96', P], ['scene 288×200', S], ['work 512×384', W], ['scene 1024×768', SB]];

console.log(`\n### Hash (ms per image, best of ${REPS})\n`);
console.log('| image | JS reference | WASM 4.2.2 | WASM SIMD | WASM baseline |');
console.log('|---|---:|---:|---:|---:|');
for (const [name, im] of images) {
  const n = im.w * im.h > 300000 ? 2 : 8;
  const j = time(() => V.hash(im), n);
  const o = old ? time(() => old.eng.hash(im), n) : null;
  const s = time(() => simd.hash(im), n);
  const b = time(() => base.hash(im), n);
  console.log(`| ${name} | ${ms(j)} | ${ms(o)}${x(o, s)} | **${ms(s)}**${x(j, s)} | ${ms(b)} |`);
}

const P1 = V.cal();
const fp = im => simd.hash(im);
const pairs = [
  ['scene × mirrored scene', fp(S), fp(mirror(S))],
  ['scene × unrelated scene', fp(S), fp(scene(288, 200, 4242))],
  ['sprite × pasted into a host', fp(P), fp(paste(P, scene(320, 220, 77), 140, 60))],
  ['work × 70% crop', fp(W), fp(crop(W))],
  ['scene × 3× upscale', fp(S), fp(upscale(S, 3))]
];
console.log(`\n### Compare, comparator 42 (ms per pair, best of ${REPS})\n`);
console.log('| pair | JS reference | WASM SIMD, full report | WASM SIMD, lean | WASM baseline, full | verdict |');
console.log('|---|---:|---:|---:|---:|---|');
for (const [name, a, b] of pairs) {
  const sa = simd.prepare(a), sb = simd.prepare(b), ba = base.prepare(a), bb = base.prepare(b);
  const j = time(() => V.compare(a.t1, a.t2, b.t1, b.t2, {}, P1), 3);
  const f = time(() => simd.compare(sa, sb, { json: true }), 10);
  const l = time(() => simd.compare(sa, sb, { lean: true, json: true }), 10);
  const bf = time(() => base.compare(ba, bb, { json: true }), 10);
  const r = simd.compare(sa, sb, { lean: true });
  console.log(`| ${name} | ${ms(j)} | **${ms(f)}**${x(j, f)} | ${ms(l)}${x(j, l)} | ${ms(bf)} | ${r.verdict} |`);
  for (const s of [sa, sb, ba, bb]) s.free();
}

if (old) {
  console.log(`\n### The v3 comparator through the C ABI (ms per pair)\n`);
  console.log('| pair | WASM 4.2.2 | WASM now |');
  console.log('|---|---:|---:|');
  for (const [name, a, b] of pairs.slice(0, 3)) {
    const o = time(() => old.eng.compare(a, b), 5);
    const pa = simd.put(a.t1), pa2 = simd.put(a.t2), pb = simd.put(b.t1), pb2 = simd.put(b.t2);
    const n = time(() => simd.x.paph_release(simd.x.paph_compare(0, pa, a.t1.length, pa2, a.t2.length, pb, b.t1.length, pb2, b.t2.length)), 5);
    console.log(`| ${name} | ${ms(o)} | **${ms(n)}**${x(o, n)} |`);
  }
}

/* ranking: one query against a mixed candidate set */
{
  const cands = [];
  for (let i = 0; i < 60; i++) cands.push(fp(scene(200 + (i % 5) * 20, 160, 1000 + i)));
  for (let i = 0; i < 30; i++) cands.push(fp(sprite(96, 96, 300 + i)));
  for (let i = 0; i < 10; i++) cands.push(fp(work(128, 128, 500 + i)));
  cands.push(fp(mirror(S)), fp(crop(S, 0.6, 0.2, 0.8)), fp(upscale(S, 3)));
  const q = simd.prepare(fp(S));
  const sides = cands.map(c => simd.prepare(c.t1, c.t2, { strict: true }));
  const g = time(() => simd.rank(q, sides), 2);
  const u = time(() => simd.rank(q, sides, { gate: false }), 2);
  const res = simd.rank(q, sides);
  const screened = res.filter(r => r.state >= 0).length;
  const top = res.filter(r => r.state >= 3).map(r => r.index);
  console.log(`\n### Ranking (one query, ${cands.length} candidates, prepared sides)\n`);
  console.log('| mode | ms per call | per candidate |');
  console.log('|---|---:|---:|');
  console.log(`| screen-gated (${screened} of ${cands.length} pass the screen) | ${ms(g)} | ${ms(g / cands.length)} |`);
  console.log(`| every candidate compared | ${ms(u)} | ${ms(u / cands.length)} |`);
  console.log(`\nCopies found: ${top.length} (candidates ${top.join(', ')})`);
}
