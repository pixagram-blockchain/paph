/**
 * PAPH-SI through the WebAssembly glue — against the native engine, against
 * its own definition, and against SQL.
 *
 *   native     xcli (rust/target/release/xcli) and both WebAssembly builds
 *              (SIMD128 and baseline) print the same signatures, posting keys,
 *              SQL plans, scores and index answers for the same wires
 *   wires      a signature derived from the wires alone equals the one taken
 *              from a prepared X side
 *   scan       SIIndex.query equals the definition — every candidate a probe
 *              reaches, scored, at or above the threshold, best first, within
 *              the budget — at several thresholds and budgets, through removals
 *   sql        SI_SQL.query on SQLite (node:sqlite) returns the same rows as
 *              SIIndex.query for every query
 *   invariance a mirrored, a rotated and a 2× upscaled copy land in the
 *              original's cells
 *
 *     node --no-warnings test/si-wasm.mjs [--quick]
 */
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { init, SI_SQL, siSqlParams } from '../wasm/paph.js';
import { corpus, mirror, rot90, upscale, crop, recolour, scene, sprite, work } from './corpus.mjs';

const quick = process.argv.includes('--quick');
const C = { g: '\u001b[32m', r: '\u001b[31m', d: '\u001b[2m', x: '\u001b[0m' };
let pass = 0, fail = 0;
const ok = (c, name, extra) => {
  if (c) pass++; else fail++;
  if (!c || !quick) console.log(`  ${c ? C.g + 'PASS' : C.r + 'FAIL'}${C.x} ${name}${extra ? ' ' + C.d + extra + C.x : ''}`);
};
const hex = b => Array.from(b, x => x.toString(16).padStart(2, '0')).join('');

const root = join(fileURLToPath(import.meta.url), '..', '..');
const simd = await init();
const base = await init(new URL('../wasm/paph-baseline.wasm', import.meta.url));
console.log(`\nPAPH-SI — WebAssembly (${simd.simd ? 'SIMD128' : 'baseline'}) vs native, SI ABI ${simd.x.paph_siabi()}\n`);

/* ---- wires ---- */
const items = [];
for (const [name, im] of corpus()) items.push({ name, im });
for (let k = 0; k < (quick ? 4 : 12); k++) {
  items.push({ name: 'scene' + k, im: scene(120 + 13 * k, 90 + 7 * k, 500 + k) });
  items.push({ name: 'sprite' + k, im: sprite(48 + 4 * k, 48 + 4 * k, 900 + k) });
  items.push({ name: 'work' + k, im: work(64 + 8 * k, 64 + 4 * k, 1300 + k) });
}
const s0 = scene(200, 150, 77);
for (const [n, im] of [['scene77', s0], ['scene77~mirror', mirror(s0)], ['scene77~rot90', rot90(s0)], ['scene77~up2', upscale(s0, 2)],
  ['scene77~crop', crop(s0, 0.6, 0.3, 0.4)], ['scene77~recolour', recolour(s0)]]) items.push({ name: n, im });
const dir = mkdtempSync(join(tmpdir(), 'paph-si-'));
for (const it of items) {
  it.fp = simd.hash(it.im);
  it.t1 = join(dir, it.name + '.t1');
  it.t2 = join(dir, it.name + '.t2');
  writeFileSync(it.t1, it.fp.t1);
  writeFileSync(it.t2, it.fp.t2);
}

/* ---- native ---- */
const xcli = join(root, 'rust', 'target', 'release', 'xcli');
if (!existsSync(xcli)) {
  console.log('  building rust/target/release/xcli …');
  execFileSync('cargo', ['build', '--release', '--bin', 'xcli'], { cwd: join(root, 'rust'), stdio: 'inherit' });
}
const native = (...args) => execFileSync(xcli, args, { maxBuffer: 1 << 26 }).toString().trim();

/* ---- profile ---- */
const sp = simd.siprofile();
const xp = simd.xprofile();
ok(sp.xid() === xp.id(), 'the shipped SI profile is bound to the shipped X profile', sp.id().slice(0, 16) + ' → ' + sp.xid().slice(0, 16));
ok(simd.siprofile(sp.bytes()).id() === sp.id(), 'the artefact round-trips', sp.bytes().length + ' B');
{
  // SI1 (1.1.0's) is SI2's fit bound to X1: under X1 it signs a work with
  // the same bytes SI2 does under X2 — the route lanes SI bands did not move
  const calib = join(root, 'docs', 'calibration');
  const x1 = simd.xprofile({ x: readFileSync(join(calib, 'X1-PROVISIONAL.pxcl')) });
  const s1 = simd.siprofile(readFileSync(join(calib, 'SI1-PROVISIONAL.psi')));
  ok(s1.xid() === x1.id(), 'SI1 is bound to X1', s1.id().slice(0, 16) + ' → ' + s1.xid().slice(0, 16));
  let same = 0;
  for (const it of items) {
    const a = simd.sisig(it.fp, { profile: s1, xprofile: x1 }), b = simd.sisig(it.fp, { profile: sp });
    same += Buffer.from(a.bytes).equals(Buffer.from(b.bytes)) ? 1 : 0;
  }
  ok(same === items.length, 'SI1 under X1 and SI2 under X2 sign every work alike', `${same} of ${items.length}`);
  let refused = false;
  try { simd.sisig(simd.xprepare(items[0].fp), { profile: s1 }); } catch (e) { refused = true; }
  ok(refused, 'SI1 refuses a side prepared under X2');
}
const info = sp.info();
ok(info.probes >= 1 && info.budget > 0 && info.features === 1, 'profile info', JSON.stringify(info));

/* ---- signatures ---- */
for (const it of items) {
  it.side = simd.xprepare(it.fp);
  it.sig = simd.sisig(it.side);
  it.bside = base.xprepare(it.fp);
}
for (const it of items.slice(0, quick ? 6 : items.length)) {
  const [nhex, nkeys] = native('sisig', it.t1, it.t2).split('\n');
  ok(hex(it.sig.bytes) === nhex && it.sig.keys.join(',') === nkeys, 'native signature ' + it.name, it.sig.present.join(' '));
  const viaWires = simd.sisig({ t1: it.fp.t1, t2: it.fp.t2 });
  ok(hex(viaWires.bytes) === hex(it.sig.bytes), 'signature from the wires alone ' + it.name);
  const bs = base.sisig(it.bside);
  ok(hex(bs.bytes) === hex(it.sig.bytes) && bs.keys.join(',') === it.sig.keys.join(','), 'baseline build agrees on signature and keys ' + it.name);
}
{
  const o = items.find(i => i.name === 'scene77').sig;
  for (const n of ['scene77~mirror', 'scene77~rot90', 'scene77~up2']) {
    const c = items.find(i => i.name === n).sig;
    const same = Object.keys(o.cells).filter(f => c.cells[f] === o.cells[f]);
    ok(same.length >= 4, n + ' lands in the original’s cells', `${same.length} of ${Object.keys(o.cells).length}: ${same.join(' ')}`);
  }
}

/* ---- queries: plans, scores, the index, SQL ---- */
const idx = simd.siindex();
const bidx = base.siindex();
for (const it of items) {
  it.slot = idx.add(it.sig.bytes);
  bidx.add(it.sig.bytes);
}
ok(idx.size === items.length && idx.generation === items.length, 'index holds every signature', `${idx.size} works`);
const { DatabaseSync } = await import('node:sqlite');
const db = new DatabaseSync(':memory:');
db.exec(SI_SQL.schema);
const insW = db.prepare('INSERT INTO si_works (work_id, present, sig, si_profile) VALUES (?, ?, ?, ?)');
const insP = db.prepare('INSERT INTO si_postings (k, work_id) VALUES (?, ?)');
for (const it of items) {
  insW.run(it.slot, it.sig.bytes[0], it.sig.bytes, sp.id().slice(0, 16));
  for (const k of it.sig.keys) insP.run(k, it.slot);
}
const stmt = db.prepare(SI_SQL.query);
const alive = new Set(items.map(i => i.slot));
const settings = [{}, { threshold: -1000, budget: 1000 }, { threshold: 60, budget: 5 }, { threshold: 150, budget: 1000 }, { threshold: 0, budget: 0 }, { threshold: 0, budget: -3 }];
let nq = 0;
for (const it of items.slice(0, quick ? 8 : items.length)) {
  const q = simd.siquery(it.side);
  const plan = q.plan();
  if (nq < (quick ? 4 : 16)) {
    ok(JSON.stringify(plan) === native('siplan', it.t1, it.t2), 'native plan ' + it.name, plan.probes.length + ' probes');
    const files = items.slice(0, 20).flatMap(c => [c.t1, c.t2]);
    const [nscores, nhits] = native('sirank', it.t1, it.t2, ...files).split('\n');
    const sub = simd.siindex();
    for (const c of items.slice(0, 20)) sub.add(c.sig.bytes);
    const ws = items.slice(0, 20).map(c => { const v = q.score(c.sig.bytes); return v === null ? '-' : String(v); }).join(',');
    const wh = sub.query(q).hits.map(h => h.slot + ':' + h.score).join(',');
    ok(ws === nscores && wh === nhits, 'native scores and index answer ' + it.name);
    const bqq = base.siquery(it.bside);
    const bws = items.slice(0, 20).map(c => { const v = bqq.score(c.sig.bytes); return v === null ? '-' : String(v); }).join(',');
    ok(JSON.stringify(bqq.plan()) === JSON.stringify(plan) && bws === ws, 'baseline build agrees on plan and scores ' + it.name);
    bqq.free();
    sub.free();
  }
  nq++;
  const bq = base.siquery(it.bside);
  for (const o of settings) {
    const got = idx.query(q, o);
    // the definition: every live candidate a probe reaches, scored, at or above the threshold
    const th = o.threshold === undefined ? info.threshold : o.threshold;
    const budget = o.budget === undefined ? info.budget : Math.max(0, o.budget);
    const want = items.filter(c => alive.has(c.slot)).map(c => ({ slot: c.slot, score: q.score(c.sig.bytes) }))
      .filter(h => h.score !== null && h.score >= th)
      .sort((a, b) => b.score - a.score || a.slot - b.slot);
    ok(got.admitted === want.length && JSON.stringify(got.hits) === JSON.stringify(want.slice(0, budget)),
      `index = definition ${it.name} ${JSON.stringify(o)}`, `${got.hits.length} hits of ${got.touched} touched, ${got.postings} postings`);
    const rows = stmt.all(...siSqlParams(plan, o)).map(r => ({ slot: Number(r.id), score: Number(r.score) }));
    ok(JSON.stringify(rows) === JSON.stringify(got.hits), `SQL = index ${it.name} ${JSON.stringify(o)}`);
    ok(JSON.stringify(bidx.query(bq, o).hits) === JSON.stringify(got.hits), `baseline index agrees ${it.name} ${JSON.stringify(o)}`);
  }
  bq.free();
  q.free();
}

/* ---- removals ---- */
for (const it of items.filter((_, k) => k % 3 === 0)) {
  ok(idx.remove(it.slot) === true, 'remove ' + it.name);
  alive.delete(it.slot);
  db.prepare('DELETE FROM si_postings WHERE work_id = ?').run(it.slot);
  db.prepare('DELETE FROM si_works WHERE work_id = ?').run(it.slot);
}
ok(idx.remove(items[0].slot) === false, 'a removed slot cannot be removed twice');
for (const it of items.slice(1, quick ? 5 : 20)) {
  const q = simd.siquery(it.side);
  const got = idx.query(q, { threshold: -1000, budget: 1000 });
  const want = items.filter(c => alive.has(c.slot)).map(c => ({ slot: c.slot, score: q.score(c.sig.bytes) }))
    .filter(h => h.score !== null && h.score >= -1000).sort((a, b) => b.score - a.score || a.slot - b.slot);
  ok(JSON.stringify(got.hits) === JSON.stringify(want), 'after removals, index = definition ' + it.name);
  const rows = stmt.all(...siSqlParams(q.plan(), { threshold: -1000, budget: 1000 })).map(r => ({ slot: Number(r.id), score: Number(r.score) }));
  ok(JSON.stringify(rows) === JSON.stringify(got.hits), 'after removals, SQL = index ' + it.name);
  q.free();
}

console.log(`\n${fail ? C.r : C.g}${pass} passed, ${fail} failed${C.x}`);
process.exit(fail ? 1 : 0);
