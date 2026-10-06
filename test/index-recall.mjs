/**
 * Does the index find the original?  Recall of the two exact-match key
 * families of Engine.indexKeys (KEYS_VERSION 1) as an inverted index uses them.
 *
 * Index: a few hundred distractor works plus the target.  Query: a transformed
 * copy (or, the other way round, the original against an indexed copy — at
 * ingest time a pair is checked once, in whichever order the two works
 * arrive).  Each family scores every indexed work by the sum of 1/df over the
 * keys it shares with the query (df = how many indexed works hold the key) and
 * nominates its top K; the target is "found" when it is among them.  The
 * comparator's own verdict on the pair is reported beside it — finding a pair
 * and certifying it are different questions.
 *
 *     node test/index-recall.mjs [--k 32] [--distractors 200] [--markdown]
 *
 * The distractors come from the same three generators as the originals, other
 * seeds: the hardest negatives this corpus has.  Real pixel art is more varied;
 * treat the table as a floor for the transforms the keys are built for and an
 * honest map of the ones they are not (resampling, recolouring), which the
 * search engine covers with its other channels (pHash, image embeddings).
 */
import { init } from '../wasm/paph.js';
import * as G from './corpus.mjs';

const arg = (k, d) => { const i = process.argv.indexOf(k); return i > 0 ? +process.argv[i + 1] : d; };
const K = arg('--k', 32), ND = arg('--distractors', 200);
const paph = await init();

const originals = [
  ['scene 288×200', G.scene(288, 200, 9137)], ['scene 256×256', G.scene(256, 256, 4242)],
  ['sprite 96×96', G.sprite(96, 96, 5)], ['sprite 64×64', G.sprite(64, 64, 77)],
  ['work 128×128', G.work(128, 128, 42)], ['scene 400×300', G.scene(400, 300, 777)],
  ['work 240×180', G.work(240, 180, 4711)], ['sprite 128×128', G.sprite(128, 128, 99)]
];
const transforms = [
  ['mirrored', im => G.mirror(im)],
  ['rotated 90°', im => G.rot90(im)],
  ['cropped to 70%', im => G.crop(im, 0.7)],
  ['cropped to 50%, corner', im => G.crop(im, 0.5, 0.1, 0.9)],
  ['2× nearest upscale', im => G.upscale(im, 2)],
  ['resampled to 75%', im => G.resample(im, Math.round(im.w * 0.75), Math.round(im.h * 0.75))],
  ['resampled to 150%', im => G.resample(im, Math.round(im.w * 1.5), Math.round(im.h * 1.5))],
  ['recoloured', im => G.recolour(im)],
  ['inverted', im => G.invert(im)],
  ['pasted into a host', im => G.paste(im, G.scene(Math.max(320, im.w + 40), Math.max(240, im.h + 40), 31337), 20, 20)]
];

const keys = (fp, query) => {
  const k = paph.indexKeys(fp, { query });
  return { codes: new Set(k.codes), bands: new Set(k.bands) };
};

/* the index: distractors, keyed as stored works */
const t0 = performance.now();
const index = [];
for (let i = 0; i < ND; i++) {
  const g = i % 3, z = i % 7;
  const im = g === 0 ? G.scene(160 + z * 40, 140 + (i % 5) * 30, 5000 + i)
           : g === 1 ? G.sprite(64 + (z % 4) * 32, 64 + (z % 4) * 32, 6000 + i)
           : G.work(96 + z * 32, 96 + (i % 4) * 32, 7000 + i);
  index.push(keys(paph.hash(im), false));
}
const df = { codes: new Map(), bands: new Map() };
const count = (k, d) => { for (const f of ['codes', 'bands']) for (const x of k[f]) df[f].set(x, (df[f].get(x) || 0) + d); };
for (const k of index) count(k, 1);

/* rank of `target` among index ∪ {target} for one family; Infinity when it shares nothing */
function rankOf(q, target, fam) {
  count(target, 1);
  const score = w => { let s = 0; for (const x of q[fam]) if (w[fam].has(x)) s += 1 / df[fam].get(x); return s; };
  const st = score(target);
  const r = st === 0 ? Infinity : 1 + index.filter(w => score(w) >= st).length;
  count(target, -1);
  return r;
}

const ofp = originals.map(([, im]) => paph.hash(im));
const rows = [];
for (const [tname, tf] of transforms) {
  const r = { name: tname, fwd: [0, 0, 0], rev: [0, 0, 0], copy: 0 };
  for (let o = 0; o < originals.length; o++) {
    const cfp = paph.hash(tf(originals[o][1]));
    for (const [dir, q, t] of [['fwd', cfp, ofp[o]], ['rev', ofp[o], cfp]]) {
      const qk = keys(q, true), tk = keys(t, false);
      const c = rankOf(qk, tk, 'codes') <= K, b = rankOf(qk, tk, 'bands') <= K;
      r[dir][0] += c; r[dir][1] += b; r[dir][2] += c || b;
    }
    const v = paph.compare(ofp[o], cfp, { lean: true }).verdict;
    if (v === 'Copy' || v === 'Identical') r.copy++;
  }
  rows.push(r);
}

const n = originals.length, f = x => `${x}/${n}`;
console.log(`\nRecall@${K} among ${ND} distractors — codes, bands, either (${((performance.now() - t0) / 1000).toFixed(1)} s)\n`);
console.log('| transform | copy finds original: codes | bands | either | original finds copy: codes | bands | either | comparator: Copy |');
console.log('|---|---:|---:|---:|---:|---:|---:|---:|');
for (const r of rows) {
  console.log(`| ${r.name} | ${f(r.fwd[0])} | ${f(r.fwd[1])} | **${f(r.fwd[2])}** | ${f(r.rev[0])} | ${f(r.rev[1])} | **${f(r.rev[2])}** | ${f(r.copy)} |`);
}
