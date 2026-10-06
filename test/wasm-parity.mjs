/**
 * The two engines agree — through the WebAssembly glue a caller actually uses.
 *
 *   wires    hash() in WebAssembly returns the JavaScript engine's bytes
 *   reports  JSON.stringify(js.compare(...)) === wasm.compare(..., { json: true })
 *            for every ordered pair of the corpus, plus Tier-1-only sides,
 *            a 4.1-selection side, and corrupt wires
 *   lean     the lean report is the full one with v3: null
 *   screen   identical
 *   rank     every rank record equals the fields of the full report
 *
 *     node test/wasm-parity.mjs [--quick]
 */
import { createRequire } from 'node:module';
import { init } from '../wasm/paph.js';
import { corpus } from './corpus.mjs';

const require = createRequire(import.meta.url);
const JS = require('../src/wire.cjs');
const V = require('../src/paph-js.cjs');
const quick = process.argv.includes('--quick');

const C = { g: '\u001b[32m', r: '\u001b[31m', d: '\u001b[2m', x: '\u001b[0m' };
let pass = 0, fail = 0;
const ok = (c, name, extra) => {
  if (c) pass++; else fail++;
  if (!c || !quick) console.log(`  ${c ? C.g + 'PASS' : C.r + 'FAIL'}${C.x} ${name}${extra ? ' ' + C.d + extra + C.x : ''}`);
};
function firstDiff(x, y, path = '') {
  if (JSON.stringify(x) === JSON.stringify(y)) return null;
  if (x && y && typeof x === 'object' && typeof y === 'object') {
    const ka = Object.keys(x), kb = Object.keys(y);
    if (!Array.isArray(x) && ka.join() !== kb.join()) return path + ' keys: wasm=[' + ka + '] js=[' + kb + ']';
    for (const k of ka) { const d = firstDiff(x[k], y[k], path + '.' + k); if (d) return d; }
  }
  return path + ': wasm=' + JSON.stringify(x) + ' js=' + JSON.stringify(y);
}

const paph = await init();
console.log(`\npaph — JavaScript engine vs WebAssembly (${paph.simd ? 'SIMD128' : 'baseline'})\n`);

/* ---- wires ---- */
const P = V.cal();
const items = [];
for (const [name, im] of corpus()) {
  const js = V.hash(im, {}, P.limits);
  const wa = paph.hash(im, {}, P.limits);
  const same = Buffer.compare(Buffer.from(js.t1), Buffer.from(wa.t1)) === 0 &&
               Buffer.compare(Buffer.from(js.t2), Buffer.from(wa.t2)) === 0;
  ok(same, 'hash ' + name, `${im.w}x${im.h}, ${wa.kpCount} kp, t2 ${wa.t2.length} B`);
  items.push({ name, fp: { t1: js.t1, t2: js.t2 } });
}
/* 4.1 selection, and limits: refused the same way */
{
  const im = corpus()[0][1];
  const js = V.hash(im, { kpCount: 256, kpSelect: 0 });
  const wa = paph.hash(im, { kpCount: 256, kpSelect: 0 });
  ok(Buffer.compare(Buffer.from(js.t2), Buffer.from(wa.t2)) === 0, 'hash under the 4.1 selection rule');
  items.push({ name: 'work42@4.1', fp: { t1: js.t1, t2: js.t2 } });
  let je = '', we = '';
  try { V.hash(im, {}, [64, 64, 4096]); } catch (e) { je = e.message; }
  try { paph.hash(im, {}, [64, 64, 4096]); } catch (e) { we = e.message; }
  ok(je && je === we, 'a §16 limit refuses identically', je);
}
for (const n of ['work42', 'work42~mirror', 'scene', 'sprite']) {
  const it = items.find(x => x.name === n);
  items.push({ name: n + '/t1', fp: { t1: it.fp.t1, t2: null } });
}

/* ---- reports ---- */
const sides = new Map(items.map(it => [it.name, paph.prepare(it.fp.t1, it.fp.t2)]));
let nrep = 0, nbad = 0;
const t0 = performance.now();
for (let i = 0; i < items.length; i++) for (let j = 0; j < items.length; j++) {
  if (quick && (i * 7 + j) % 5) continue;
  const A = items[i], B = items[j];
  const js = JSON.stringify(V.compare(A.fp.t1, A.fp.t2, B.fp.t1, B.fp.t2, {}, P));
  const wa = paph.compare(sides.get(A.name), sides.get(B.name), { json: true });
  nrep++;
  if (js !== wa) {
    nbad++;
    if (nbad <= 8) ok(false, `report ${A.name} | ${B.name}`, firstDiff(JSON.parse(wa), JSON.parse(js)));
  }
  const lean = paph.compare(sides.get(A.name), sides.get(B.name), { lean: true });
  const full = JSON.parse(wa); full.v3 = null;
  if (JSON.stringify(lean) !== JSON.stringify(full)) { nbad++; ok(false, `lean ${A.name} | ${B.name}`, firstDiff(lean, full)); }
  const sj = V.screen(A.fp.t1, A.fp.t2, B.fp.t1, B.fp.t2, {}, P);
  const sw = paph.screen(sides.get(A.name), sides.get(B.name));
  if (JSON.stringify(sj) !== JSON.stringify(sw)) { nbad++; ok(false, `screen ${A.name} | ${B.name}`, JSON.stringify(sw) + ' vs ' + JSON.stringify(sj)); }
}
ok(nbad === 0, `${nrep} ordered pairs: full report text, lean report, screen`,
   `${((performance.now() - t0) / 1000).toFixed(1)} s`);

/* ---- corrupt wires: the refusal objects ---- */
{
  const a = items[0].fp, b = items[1].fp;
  const t1 = a.t1.slice(); t1[100] ^= 0xff;
  let threw = false;
  try { paph.prepare(t1, a.t2); } catch (e) { threw = true; }
  const js = V.compare(t1, a.t2, b.t1, b.t2, {}, P);
  ok(threw && js.verdict === 'Indeterminate' && js.reasons[0] === 'CORRUPT',
     'a corrupt Tier 1 is refused (prepare throws; the reference says CORRUPT)');
  const bad2 = a.t2.slice(0, 40);
  const jr = JSON.stringify(V.compare(a.t1, bad2, b.t1, b.t2, {}, P));
  const wr = paph.compare({ t1: a.t1, t2: bad2 }, { t1: b.t1, t2: b.t2 }, { json: true });
  ok(jr === wr, 'a corrupt Tier 2: identical refusal report', JSON.parse(wr).reasons.join(','));
  const sj = V.screen(a.t1, bad2, b.t1, b.t2, {}, P), sw = paph.screen({ t1: a.t1, t2: bad2 }, b);
  ok(JSON.stringify(sj) === JSON.stringify(sw), 'a corrupt Tier 2 screens on the sketch, identically');
}

/* ---- rank records = report fields ---- */
{
  const q = sides.get('scene');
  const cands = items.map(it => sides.get(it.name));
  const recs = paph.rank(q, cands, { gate: false });
  let bad = 0;
  for (let k = 0; k < items.length; k++) {
    const r = paph.compare(q, cands[k], { lean: true }), x = recs[k];
    const want = r.verdict === 'Indeterminate' ? 5 : ['Unrelated', 'Related', 'Suspected', 'Copy', 'Identical'].indexOf(r.verdict);
    const same = x.state === want && x.certifiable === r.certifiable && x.structural === r.structural &&
      x.geometryEvidence === r.geometryEvidence && x.totalInliers === r.totalInliers &&
      x.topology === r.topology && x.geoMargin === r.geoMargin && x.models === r.models.length &&
      x.diversity === r.diversity.combined && x.multiplier === r.diversity.multiplier &&
      x.swapped === r.swapped && x.mixedSelection === r.selection.mixed &&
      (r.local && r.local.measurable ? x.localEvidence === r.local.evidence : x.localEvidence === -1);
    if (!same) { bad++; if (bad < 4) console.log('    rank mismatch', items[k].name, JSON.stringify(x), r.verdict); }
  }
  ok(bad === 0, `rank: ${items.length} records equal their reports`);
  const gated = paph.rank(q, cands);
  const scr = items.map((it, k) => paph.screen(q, cands[k]).pass);
  ok(gated.every((x, k) => scr[k] ? x.state >= 0 : x.state === -1), 'rank gated on the screen: rejected pairs are UNSCREENED, never compared');
  const hits = gated.filter(x => x.state >= 2).map(x => items[x.index].name);
  console.log(`    ${C.d}scene ranks as copies/suspects: ${hits.join(', ')}${C.x}`);
}

/* ---- profiles ---- */
{
  const bytes = V.profileEncode(P);
  const prof = paph.profile(bytes);
  const a = items[0], b = items[2];
  const r1 = paph.compare(sides.get(a.name), sides.get(b.name), { json: true });
  const r2 = paph.compare(sides.get(a.name), sides.get(b.name), { json: true, profile: prof });
  ok(r1 === r2, 'the shipped profile, passed explicitly, changes nothing');
  const cal41 = paph.profile(V.profileEncode(V.cal41()));
  const jr = JSON.stringify(V.compare(a.fp.t1, a.fp.t2, b.fp.t1, b.fp.t2, {}, V.cal41()));
  const wr = paph.compare(sides.get(a.name), sides.get(b.name), { json: true, profile: cal41 });
  ok(jr === wr, "comparator 42 refuses 4.1's profile identically", JSON.parse(wr).reasons.join(','));
}

for (const s of sides.values()) s.free();
console.log(`\n${pass} passed, ${fail ? C.r : ''}${fail} failed${C.x}`);
process.exit(fail ? 1 : 0);
