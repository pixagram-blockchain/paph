/**
 * Wire 4 across the two engines (docs/SPEC-W4-paph-wire4.md).
 *
 *   bytes     the JavaScript engine's Tier 1 and Tier 2 equal the native
 *             reference's (`paphcli` H, and 3 for wire 3), byte for byte, on
 *             every image: the shared corpus, sprites and scenes at sizes
 *             that are and are not multiples of 16, above and below the shape
 *             grid's 128, and each one's mirror and quarter turns
 *   formats   wire 3 is still what 1.0–1.1 wrote: the same images under
 *             { wire: 3 } hash to the native wire-3 bytes too
 *   refusal   a wire-3 side against a wire-4 side: both engines' comparator
 *             42 report Indeterminate with WIRE_MISMATCH, and the reports
 *             agree
 *
 * Build the reference first:  cargo build --release --manifest-path rust/Cargo.toml
 * Then:                       node test/wire4-parity.mjs
 */
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { createRequire } from 'node:module';
import { corpus, sprite, scene, work, mirror, rot90 } from './corpus.mjs';
const require = createRequire(import.meta.url);
const JS = require('../src/wire.cjs');
const V = require('../src/paph-js.cjs');

const here = dirname(fileURLToPath(import.meta.url));
const BIN = join(here, '..', 'rust', 'target', 'release', process.platform === 'win32' ? 'paphcli.exe' : 'paphcli');
if (!existsSync(BIN)) {
  console.error('paphcli not found at ' + BIN + '\nbuild it first:  cargo build --release --manifest-path rust/Cargo.toml');
  process.exit(2);
}
const C = { g: '\u001b[32m', r: '\u001b[31m', d: '\u001b[2m', x: '\u001b[0m' };
let pass = 0, fail = 0;
const ok = (c, name, extra) => {
  if (c) pass++; else fail++;
  if (!c) console.log(`  ${C.r}FAIL${C.x} ${name}` + (extra ? ` ${C.d}${extra}${C.x}` : ''));
};

function nativeHash(img, mode) {
  const head = Buffer.alloc(9);
  head[0] = mode.charCodeAt(0);
  head.writeUInt32LE(img.w, 1);
  head.writeUInt32LE(img.h, 5);
  const r = spawnSync(BIN, [], { input: Buffer.concat([head, Buffer.from(img.px)]), maxBuffer: 1 << 26 });
  if (r.status !== 0) throw new Error('paphcli failed: ' + r.stderr);
  const b = r.stdout, n1 = b.readUInt32LE(0), n2 = b.readUInt32LE(4);
  return { t1: new Uint8Array(b.subarray(8, 8 + n1)), t2: new Uint8Array(b.subarray(8 + n1, 8 + n1 + n2)) };
}
function nativeV42(a, b) {
  const head = Buffer.alloc(17);
  head[0] = 'X'.charCodeAt(0);
  head.writeUInt32LE(a.t1.length, 1); head.writeUInt32LE(a.t2.length, 5);
  head.writeUInt32LE(b.t1.length, 9); head.writeUInt32LE(b.t2.length, 13);
  const r = spawnSync(BIN, [], { input: Buffer.concat([head, Buffer.from(a.t1), Buffer.from(a.t2), Buffer.from(b.t1), Buffer.from(b.t2)]), maxBuffer: 1 << 24 });
  if (r.status !== 0) throw new Error('paphcli failed: ' + r.stderr);
  return JSON.parse(r.stdout.toString());
}
const eq = (x, y) => x.length === y.length && x.every((v, i) => v === y[i]);
function firstDiff(x, y) {
  for (let i = 0; i < Math.max(x.length, y.length); i++) if (x[i] !== y[i]) return i;
  return -1;
}
function section(i) {
  let o = JS.SECTION_OFFSETS, best = 'header';
  for (const s of JS.SECTIONS) if (i >= o[s.name]) best = s.name;
  return best;
}

const images = corpus();
let seed = 0x5eed;
const R = () => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed; };
for (const [w, h] of [[37, 29], [53, 91], [130, 77], [61, 200], [203, 151], [17, 9], [300, 47], [96, 96], [129, 257]]) {
  images.push([`sprite-${w}x${h}`, sprite(w, h, R())], [`scene-${w}x${h}`, scene(Math.max(w, 24), Math.max(h, 24), R())], [`work-${w}x${h}`, work(Math.max(w, 12), Math.max(h, 12), R() & 1023)]);
}
// the silhouette's ties (docs/golden/GOLDEN-W4.json, silhouette_ties): pairs
// of shapes of one area, perimeter, box and canonical profile, which the
// moments or the occupancy tell apart, and twins — one F in two orientations —
// which only the scan order can
const TIES = JSON.parse(readFileSync(join(here, '..', 'docs', 'golden', 'GOLDEN-W4.json'), 'utf8')).silhouette_ties;
for (const t of TIES.canvases) {
  const px = new Uint8Array(t.w * t.h * 4);
  for (const s of t.stamps) for (const [x, y] of s.cells) px.set(TIES.rgba, ((s.at[1] + y) * t.w + s.at[0] + x) << 2);
  images.push([`tie-${t.name}`, { px, w: t.w, h: t.h }]);
}

const all = [];
for (const [name, im] of images) {
  all.push([name, im], [name + '~mirror', mirror(im)], [name + '~rot90', rot90(im)], [name + '~rot180', rot90(rot90(im))]);
}

console.log(`\nwire 4 — JavaScript engine vs native reference, ${all.length} images × 2 formats\n`);
let n = 0;
for (const [name, im] of all) {
  for (const wire of [4, 3]) {
    const js = JS.hash(im.px, im.w, im.h, { wire });
    const nat = nativeHash(im, wire === 4 ? 'H' : '3');
    n++;
    const d1 = firstDiff(js.t1, nat.t1);
    ok(d1 < 0, `${name} wire ${wire} tier 1`, d1 >= 0 ? `first difference at byte ${d1} (${section(d1)})` : '');
    ok(eq(js.t2, nat.t2), `${name} wire ${wire} tier 2`);
    ok(js.t1[4] === wire && js.t2[4] === wire, `${name} wire ${wire} version bytes`);
  }
}
console.log(`  ${fail ? C.r + 'FAIL' : C.g + 'PASS'}${C.x} ${n} hashes, tier 1 and tier 2 byte-identical across the engines${fail ? ` (${fail} failed)` : ''}`);

// the refusal, in both engines
const im = scene(160, 120, 77);
const w3 = JS.hash(im.px, im.w, im.h, { wire: 3 }), w4 = JS.hash(im.px, im.w, im.h);
const P = V.cal();
const jsr = V.compare(w3.t1, w3.t2, w4.t1, w4.t2, {}, P);
const nr = nativeV42(w3, w4);
const f0 = fail;
ok(jsr.verdict === 'Indeterminate' && jsr.reasons.length === 1 && jsr.reasons[0] === 'WIRE_MISMATCH', 'JavaScript comparator 42 refuses a mixed pair', JSON.stringify(jsr.reasons));
ok(nr.verdict === 'Indeterminate' && JSON.stringify(nr.reasons) === '["WIRE_MISMATCH"]', 'native comparator 42 refuses a mixed pair', JSON.stringify(nr.reasons));
ok(V.compare(w4.t1, w4.t2, w4.t1, w4.t2, {}, P).verdict === 'Identical' && V.compare(w3.t1, w3.t2, w3.t1, w3.t2, {}, P).verdict === 'Identical', 'each format against itself is compared');
console.log(`  ${fail > f0 ? C.r + 'FAIL' : C.g + 'PASS'}${C.x} a wire-3 side against a wire-4 side: Indeterminate (WIRE_MISMATCH) in both engines`);

console.log(`\n${fail ? C.r : C.g}${pass} passed, ${fail} failed${C.x}`);
process.exit(fail ? 1 : 0);
