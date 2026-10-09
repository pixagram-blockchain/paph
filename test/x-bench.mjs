/**
 * PAPH-X timings inside WebAssembly (SIMD128 and baseline) beside comparator
 * 42's, on the pairs and the candidate set of bench-engines.mjs.  Prints
 * markdown tables.  Nothing printed here is a claim until it is printed
 * here (specification §41.7).
 *
 *     node test/x-bench.mjs [--reps 5] [--x1 | --x2]
 *
 * Under the shipped X3-PROVISIONAL (bound to CAL-007-PROVISIONAL), or 1.1's
 * X2-PROVISIONAL with --x2, or 1.0.0's X1-PROVISIONAL with --x1 (both bound
 * to CAL-004-PROPOSED, and loaded with it; comparator 42's own timings stay
 * the shipped calibration's, whose measurement is CAL-004's).
 */
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { init } from '../wasm/paph.js';
import { work, scene, sprite, mirror, crop, paste, upscale, recolour } from './corpus.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const arg = (k, d) => { const i = process.argv.indexOf(k); return i > 0 ? process.argv[i + 1] : d; };
const REPS = +arg('--reps', 5);

const simd = await init(join(here, '..', 'wasm', 'paph.wasm'));
const base = await init(join(here, '..', 'wasm', 'paph-baseline.wasm'));
const OLD = process.argv.includes('--x1') ? 'X1' : process.argv.includes('--x2') ? 'X2' : null;
if (OLD) {
  // every PAPH-X call under X1 or X2: the sides and the calls carry the profile
  const calib = join(here, '..', 'docs', 'calibration');
  const bytes = readFileSync(join(calib, OLD + '-PROVISIONAL.pxcl'));
  const cal004 = readFileSync(join(calib, 'CAL-004-PROPOSED.pcal'));
  for (const e of [simd, base]) {
    const prof = e.xprofile({ base: cal004, x: bytes });
    if (prof.status() !== 'ok') throw new Error(OLD + '-PROVISIONAL did not bind to CAL-004-PROPOSED: ' + prof.status());
    const [xp, xs, xc, xr] = [e.xprepare.bind(e), e.xscreen.bind(e), e.xcompare.bind(e), e.xrank.bind(e)];
    e.xprepare = (a, b, o) => (a && a.t1 && !(a instanceof Uint8Array)) ? xp(a, { ...(b || {}), profile: prof }) : xp(a, b, { ...(o || {}), profile: prof });
    e.xscreen = (a, b, o) => xs(a, b, { ...(o || {}), profile: prof });
    e.xcompare = (a, b, o) => xc(a, b, { ...(o || {}), profile: prof });
    e.xrank = (q, c, o) => xr(q, c, { ...(o || {}), profile: prof });
  }
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
const ms = v => v < 0.01 ? v.toFixed(4) : v < 1 ? v.toFixed(3) : v < 10 ? v.toFixed(2) : v.toFixed(1);
const x = (a, b) => ` (${(a / b).toFixed(1)}×)`;

const S = scene(288, 200, 9137), W = work(512, 384, 7), P = sprite(96, 96, 5);
const fp = im => simd.hash(im);
const pairs = [
  ['scene × mirrored scene', fp(S), fp(mirror(S))],
  ['scene × unrelated scene', fp(S), fp(scene(288, 200, 4242))],
  ['sprite × pasted into a host', fp(P), fp(paste(P, scene(320, 220, 77), 140, 60))],
  ['work × 70% crop', fp(W), fp(crop(W))],
  ['scene × 3× upscale', fp(S), fp(upscale(S, 3))],
  ['scene × recoloured scene', fp(S), fp(recolour(S))],
  ['work × unrelated work', fp(W), fp(work(512, 384, 8))]
];

console.log(`\nPAPH-X in WebAssembly (${simd.simd ? 'SIMD128' : 'baseline'} build) — X ABI ${simd.x.paph_xabi()}, ${OLD || 'X3'}-PROVISIONAL, best of ${REPS}\n`);
console.log('### Pair screen (ms per pair)\n');
console.log('| pair | screen 42 | xscreen | xscreen, baseline build | xscreen state |');
console.log('|---|---:|---:|---:|---|');
for (const [name, a, b] of pairs) {
  const sa = simd.prepare(a), sb = simd.prepare(b);
  const xa = simd.xprepare(a), xb = simd.xprepare(b), ya = base.xprepare(a), yb = base.xprepare(b);
  const s42 = time(() => simd.screen(sa, sb), 20);
  const sx = time(() => simd.xscreen(xa, xb), 20);
  const sy = time(() => base.xscreen(ya, yb), 20);
  const r = simd.xscreen(xa, xb);
  console.log(`| ${name} | ${ms(s42)} | **${ms(sx)}**${x(s42, sx)} | ${ms(sy)} | ${r.state} (${r.route.class}, pools ${r.poolDirect}/${r.poolMirror}) |`);
  for (const s of [sa, sb, xa, xb, ya, yb]) s.free();
}

console.log('\n### Compare (ms per pair)\n');
console.log('| pair | comparator 42, lean | xcompare safe | xcompare fast | xcompare exact | verdict (42 / X safe, execution) |');
console.log('|---|---:|---:|---:|---:|---|');
for (const [name, a, b] of pairs) {
  const sa = simd.prepare(a), sb = simd.prepare(b), xa = simd.xprepare(a), xb = simd.xprepare(b);
  const l = time(() => simd.compare(sa, sb, { lean: true, json: true }), 10);
  const safe = time(() => simd.xcompare(xa, xb, { policy: 'safe', json: true }), 10);
  const fast = time(() => simd.xcompare(xa, xb, { policy: 'fast', json: true }), 10);
  const exact = time(() => simd.xcompare(xa, xb, { policy: 'exact', json: true }), 10);
  const r42 = simd.compare(sa, sb, { lean: true });
  const rx = simd.xcompare(xa, xb, { policy: 'safe' });
  console.log(`| ${name} | ${ms(l)} | **${ms(safe)}**${x(l, safe)} | ${ms(fast)}${x(l, fast)} | ${ms(exact)} | ${r42.verdict} / ${rx.verdict}, ${rx.execution} |`);
  for (const s of [sa, sb, xa, xb]) s.free();
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
  const xq = simd.xprepare(fp(S));
  const xsides = cands.map(c => simd.xprepare(c.t1, c.t2, { strict: true }));
  const g = time(() => simd.rank(q, sides), 2);
  const xg = time(() => simd.xrank(xq, xsides), 2);
  const xf = time(() => simd.xrank(xq, xsides, { policy: 'fast' }), 2);
  const r42 = simd.rank(q, sides);
  const rx = simd.xrank(xq, xsides);
  const copies42 = r42.filter(r => r.state >= 3 && r.state <= 4).map(r => r.index);
  const copiesX = rx.filter(r => r.state >= 3 && r.state <= 4).map(r => r.index);
  const compared = rx.filter(r => r.state >= 0).length;
  const fallbacks = rx.filter(r => r.execution === 'FALLBACK').length;
  console.log(`\n### Ranking (one query, ${cands.length} candidates, prepared sides)\n`);
  console.log('| call | ms per call | per candidate | copies found |');
  console.log('|---|---:|---:|---|');
  console.log(`| rank, comparator 42, screen-gated | ${ms(g)} | ${ms(g / cands.length)} | ${copies42.join(', ')} |`);
  console.log(`| xrank, safe (${compared} compared, ${fallbacks} fallbacks) | **${ms(xg)}**${x(g, xg)} | ${ms(xg / cands.length)} | ${copiesX.join(', ')} |`);
  console.log(`| xrank, fast | ${ms(xf)}${x(g, xf)} | ${ms(xf / cands.length)} | |`);
  /* route-only: the Stage-A table over a large candidate set */
  const many = [];
  for (let i = 0; i < 2000; i++) many.push(xsides[i % xsides.length]);
  const ro = time(() => simd.xrank(xq, many, { policy: 'fast' }), 1);
  const rr = simd.xrank(xq, many, { policy: 'fast' });
  console.log(`\nxrank over ${many.length} candidates (the same ${cands.length}, repeated): ${ms(ro)} ms, ${(ro * 1000 / many.length).toFixed(1)} µs per candidate, ${rr.filter(r => r.state < 0).length} route-rejected or unscreened`);
}

/* the specification's corpus (§33.1), when xbench has dumped it:
   rust/target/release/xbench --dump rust/target/xcorpus */
{
  const dir = join(here, '..', 'rust', 'target', 'xcorpus');
  if (existsSync(dir)) {
    const load = name => {
      const b = readFileSync(join(dir, name + '.rgba'));
      return { px: new Uint8Array(b.buffer, b.byteOffset + 8, b.length - 8), w: b.readUInt32LE(0), h: b.readUInt32LE(4) };
    };
    const names = readdirSync(dir).filter(f => f.endsWith('.rgba')).map(f => f.slice(0, -5)).sort();
    const fam = names.filter(n => n.startsWith('B09~'));
    const large = names.filter(n => n.startsWith('L'));
    const pick = fam.slice();
    for (let k = 0; pick.length < 100 && large.length; k++) pick.push(large[k % large.length]);
    const q = load('B09');
    const qfp = fp(q);
    const cfp = pick.map(n => fp(load(n)));
    const sides = cfp.map(c => simd.prepare(c.t1, c.t2, { strict: true }));
    const xsides = cfp.map(c => simd.xprepare(c.t1, c.t2, { strict: true }));
    const qs = simd.prepare(qfp), xq = simd.xprepare(qfp);
    const g = time(() => simd.rank(qs, sides), 2);
    const xg = time(() => simd.xrank(xq, xsides), 2);
    const xf = time(() => simd.xrank(xq, xsides, { policy: 'fast' }), 2);
    const r42 = simd.rank(qs, sides), rx = simd.xrank(xq, xsides);
    const c42 = r42.filter(r => r.state >= 3 && r.state <= 4).length, cx = rx.filter(r => r.state >= 3 && r.state <= 4).length;
    const mean = Math.round(cfp.reduce((s, c) => s + c.kpCount, 0) / cfp.length);
    console.log(`\n### The specification's reference workload in WebAssembly: B09 (${qfp.kpCount} kp) vs ${pick.length} candidates of the 512-keypoint class (mean ${mean} kp)\n`);
    console.log('| call | ms per call | copies found |');
    console.log('|---|---:|---|');
    console.log(`| rank, comparator 42, screen-gated (${r42.filter(r => r.state >= 0).length} survivors) | ${ms(g)} | ${c42} |`);
    console.log(`| xrank, safe (${rx.filter(r => r.state >= 0).length} compared, ${rx.filter(r => r.execution === 'FALLBACK').length} fallbacks) | **${ms(xg)}**${x(g, xg)} | ${cx} |`);
    console.log(`| xrank, fast | ${ms(xf)}${x(g, xf)} | |`);
    /* unrelated same-style pairs of the corpus: the route's bread and butter */
    const unrel = names.filter(n => n.startsWith('S')).slice(0, 12).map(n => fp(load(n)));
    let s42 = 0, sx = 0, c42t = 0, cxt = 0, n = 0;
    for (let i = 0; i < unrel.length; i++) for (let j = i + 1; j < unrel.length; j++) {
      const a = simd.prepare(unrel[i]), b = simd.prepare(unrel[j]), xa = simd.xprepare(unrel[i]), xb = simd.xprepare(unrel[j]);
      s42 += time(() => simd.screen(a, b), 5); sx += time(() => simd.xscreen(xa, xb), 5);
      c42t += time(() => simd.compare(a, b, { lean: true, json: true }), 3); cxt += time(() => simd.xcompare(xa, xb, { json: true }), 3);
      n++;
      for (const s of [a, b, xa, xb]) s.free();
    }
    console.log(`\n### Unrelated same-style pairs of the specification's corpus (${n} pairs, mean ms per pair)\n`);
    console.log('| | comparator 42 | PAPH-X |');
    console.log('|---|---:|---:|');
    console.log(`| screen | ${ms(s42 / n)} | **${ms(sx / n)}**${x(s42, sx)} |`);
    console.log(`| compare (lean / safe) | ${ms(c42t / n)} | **${ms(cxt / n)}**${x(c42t, cxt)} |`);
  }
}
