/**
 * PAPH-X through the WebAssembly glue — and against the native engine.
 *
 *   native     xcli (rust/target/release/xcli) and the WebAssembly module
 *              print the same route, screen, report and rank records for
 *              the same wires (built if missing: cargo build --release)
 *   symmetry   xcompare(a, b) and xcompare(b, a) agree on verdict and
 *              execution; xscreen too
 *   exact      under the 'exact' policy the verdict is compare42's
 *   safe       under 'safe' the Copy question is answered as compare42
 *              answers it on every pair of the corpus (the states below
 *              Copy are listed where they differ)
 *   rank       every rank record's state equals the pairwise copy-scope
 *              verdict; gated-out candidates are never comparator-42 copies
 *   sidecar    a side prepared through its sidecar reports identically
 *   builds     SIMD128 and baseline produce the same text
 *
 *     node test/x-wasm.mjs [--quick]
 */
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { init } from '../wasm/paph.js';
import { corpus, mirror, crop, recolour, scene } from './corpus.mjs';

const quick = process.argv.includes('--quick');
const C = { g: '\u001b[32m', r: '\u001b[31m', d: '\u001b[2m', x: '\u001b[0m' };
let pass = 0, fail = 0;
const ok = (c, name, extra) => {
  if (c) pass++; else fail++;
  if (!c || !quick) console.log(`  ${c ? C.g + 'PASS' : C.r + 'FAIL'}${C.x} ${name}${extra ? ' ' + C.d + extra + C.x : ''}`);
};

const root = join(fileURLToPath(import.meta.url), '..', '..');
const simd = await init();
const base = await init(new URL('../wasm/paph-baseline.wasm', import.meta.url));
console.log(`\nPAPH-X — WebAssembly (${simd.simd ? 'SIMD128' : 'baseline'}) vs native, X ABI ${simd.x.paph_xabi()}\n`);

/* ---- wires, from the shared corpus plus a few transforms ---- */
const items = [];
for (const [name, im] of corpus()) items.push({ name, fp: simd.hash(im) });
{
  const s = scene(200, 150, 77);
  items.push({ name: 'scene77', fp: simd.hash(s) });
  items.push({ name: 'scene77~mirror', fp: simd.hash(mirror(s)) });
  items.push({ name: 'scene77~crop', fp: simd.hash(crop(s, 0.6, 0.3, 0.4)) });
  items.push({ name: 'scene77~recolour', fp: simd.hash(recolour(s)) });
}
const dir = mkdtempSync(join(tmpdir(), 'paph-x-'));
for (const it of items) {
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
const xp = simd.xprofile();
ok(xp.status() === 'ok', 'shipped X profile binds to the shipped base', xp.id().slice(0, 16));
ok(xp.bytes().length > 200, 'X artefact bytes', xp.bytes().length + ' B');

/* ---- routes ---- */
const sides = items.map(it => ({ ...it, side: simd.xprepare(it.fp.t1, it.fp.t2) }));
for (const it of sides.slice(0, quick ? 4 : sides.length)) {
  const hex = Array.from(it.side.route(), b => b.toString(16).padStart(2, '0')).join('');
  ok(hex === native('route', it.t1, it.t2), 'route ' + it.name, `${it.side.kp} kp`);
}

/* ---- pairs ---- */
const pairs = [];
for (let i = 0; i < sides.length; i++) for (let j = 0; j < sides.length; j++) {
  if (quick && (i * 7 + j) % 5 !== 0 && i !== j) continue;
  pairs.push([i, j]);
}
let nativeChecked = 0;
const stateDiffs = [];
for (const [i, j] of pairs) {
  const a = sides[i], b = sides[j];
  const label = a.name + ' × ' + b.name;
  const r42 = simd.compare(a.fp, b.fp, { lean: true });
  const safe = simd.xcompare(a.side, b.side, { policy: 'safe' });
  const safeBack = simd.xcompare(b.side, a.side, { policy: 'safe' });
  ok(safe.verdict === safeBack.verdict && safe.execution === safeBack.execution, 'symmetry ' + label, `${safe.verdict} ${safe.execution}`);
  const copy42 = r42.verdict === 'Copy' || r42.verdict === 'Identical';
  const copyX = safe.verdict === 'Copy' || safe.verdict === 'Identical';
  ok(copy42 === copyX, 'safe agrees on the copy question ' + label, `42 ${r42.verdict}, X ${safe.verdict} (${safe.execution}, ${safe.reason})`);
  if (safe.verdict !== r42.verdict) stateDiffs.push(`${label}: 42 ${r42.verdict}, X ${safe.verdict} (${safe.execution}, ${safe.reason})`);
  const exact = simd.xcompare(a.side, b.side, { policy: 'exact' });
  ok(exact.verdict === r42.verdict && exact.execution === 'AUDIT', 'exact policy is comparator 42 ' + label);
  const s1 = simd.xscreen(a.side, b.side), s2 = simd.xscreen(b.side, a.side);
  ok(s1.state === s2.state && s1.poolDirect === s2.poolDirect, 'screen symmetric ' + label, `${s1.state}/${s1.route.class} pools ${s1.poolDirect}/${s1.poolMirror}`);
  ok(!(copy42 && s1.state === 'Reject'), 'screen never hard-rejects a 42 copy ' + label);
  if (nativeChecked < (quick ? 6 : 40) && (i + 2 * j) % 3 === 0) {
    nativeChecked++;
    for (const flags of [0, 1, 2, 3, 4, 8]) {
      const text = simd.xcompare(a.side, b.side, { policy: ['', 'fast', 'safe', 'exact'][flags & 3] || undefined, audit: !!(flags & 4), scope: flags & 8 ? 'copy' : 'full', json: true });
      const nat = native('compare', a.t1, a.t2, b.t1, b.t2, String(flags));
      ok(text === nat, `native report ${label} flags ${flags}`);
      const bt = base.xcompare(base.xprepare(a.fp), base.xprepare(b.fp), { policy: ['', 'fast', 'safe', 'exact'][flags & 3] || undefined, audit: !!(flags & 4), scope: flags & 8 ? 'copy' : 'full', json: true });
      ok(text === bt, `baseline build agrees ${label} flags ${flags}`);
    }
    const sn = JSON.parse(native('screen', a.t1, a.t2, b.t1, b.t2));
    ok(['Reject', 'Defer', 'Pass', 'Identical'][sn.state] === s1.state && sn.local === s1.route.local && sn.poolDirect === s1.poolDirect, `native screen ${label}`);
  }
}

/* the states below Copy: 42 reads a few chance inliers as a weak geometric
   signal (Suspected) where the sparse pools hold none — expected on this
   shared-texture fixture, reported, never a copy disagreement */
console.log(`  ${C.d}safe state differs from comparator 42 on ${stateDiffs.length} of ${pairs.length} ordered pairs${stateDiffs.length ? ':' : ''}${C.x}`);
for (const d of stateDiffs) console.log(`  ${C.d}  ${d}${C.x}`);

/* ---- rank ---- */
{
  const q = sides.find(s => s.name === 'scene77');
  const cands = sides.filter(s => s !== q);
  const recs = simd.xrank(q.side, cands.map(c => c.side));
  let gatedCopies = 0;
  for (let k = 0; k < cands.length; k++) {
    const r = recs[k];
    const r42 = simd.compare(q.fp, cands[k].fp, { lean: true });
    if (r.state < 0) {
      if (r42.verdict === 'Copy' || r42.verdict === 'Identical') gatedCopies++;
      continue;
    }
    const pw = simd.xcompare(q.side, cands[k].side, { scope: 'copy' });
    ok(r.verdict === pw.verdict && r.execution === pw.execution, 'rank record equals the pairwise copy-scope verdict: ' + cands[k].name, `${r.verdict} ${r.execution}`);
  }
  ok(gatedCopies === 0, 'the gate never drops a comparator-42 copy', `${recs.filter(r => r.state < 0).length} of ${cands.length} gated out`);
  const args = ['rank', q.t1, q.t2];
  for (const c of cands) args.push(c.t1, c.t2);
  const nat = native(...args).split('\n');
  const raw = simd.xrank(q.side, cands.map(c => c.side), { raw: true }).records;
  let same = true;
  for (let k = 0; k < cands.length; k++) {
    if (Array.from(raw.subarray(k * 24, (k + 1) * 24)).join(',') !== nat[k]) { same = false; break; }
  }
  ok(same, 'native rank records are the WebAssembly ones', `${cands.length} candidates`);
  const full = simd.xrank(q.side, cands.map(c => c.side), { gate: false, scope: 'full', policy: 'safe' });
  ok(full.every(r => r.state >= 0 && r.state <= 5), 'ungated full-scope rank resolves every candidate');
}

/* ---- sidecar ---- */
{
  const it = sides.find(s => s.name === 'scene77~mirror');
  const sc = it.side.sidecar();
  const through = simd.xprepare(it.fp.t1, it.fp.t2, { sidecar: sc });
  const other = sides.find(s => s.name === 'scene77');
  const r1 = simd.xcompare(it.side, other.side, { json: true });
  const r2 = simd.xcompare(through, other.side, { json: true });
  ok(r1 === r2, 'a side prepared through its sidecar reports identically', `${sc.length} B sidecar`);
  const stale = simd.xprepare(it.fp.t1, it.fp.t2, { sidecar: sc.subarray(0, sc.length - 1) });
  ok(simd.xcompare(stale, other.side, { json: true }) === r1, 'a damaged sidecar is ignored and rebuilt');
  const nat = native('sidecar', it.t1, it.t2, join(dir, 'x.pax1'));
  ok(nat.startsWith(sc.length + ' bytes, route true, order true'), 'native sidecar agrees', nat);
}

console.log(`\n${pass} passed, ${fail} failed${C.x}`);
process.exit(fail ? 1 : 0);
