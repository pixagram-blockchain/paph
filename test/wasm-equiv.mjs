/**
 * The WebAssembly builds compute what the native build computes.
 *
 * The equivalence digest (rust/src/equiv.rs) hashes and compares a
 * deterministic synthetic corpus — degenerate sizes, dithered art, mattes,
 * blow-ups, every D4 transform, inversion, recolour, crops, pastes, sixteen
 * hash-time configurations, every comparator, the Tier-1 path, corrupt wires,
 * and the JavaScript-shaped report — one SHA-256 per case.  The digest-
 * exporting test builds (tools/build-wasm.sh --equiv) compute it inside
 * WebAssembly, SIMD and baseline, and both must equal the native digests
 * checked in at test/equiv-digest.txt (wire 3) and test/equiv-digest-4.txt
 * (wire 4) line for line.
 */
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const here = dirname(fileURLToPath(import.meta.url));
let fail = 0;
for (const [name, ref, fn] of [
  ['paph-equiv.wasm', 'equiv-digest.txt', 'paph_equiv_digest'],
  ['paph-baseline-equiv.wasm', 'equiv-digest.txt', 'paph_equiv_digest'],
  ['paph-equiv.wasm', 'equiv-digest-4.txt', 'paph_equiv_digest4'],
  ['paph-baseline-equiv.wasm', 'equiv-digest-4.txt', 'paph_equiv_digest4'],
]) {
  const want = readFileSync(join(here, ref), 'utf8');
  const f = join(here, '.wasm', name);
  if (!existsSync(f)) { console.log('  SKIP ' + name + ' (run tools/build-wasm.sh --equiv)'); continue; }
  const { instance } = await WebAssembly.instantiate(readFileSync(f), {});
  const x = instance.exports;
  const t0 = performance.now();
  const p = x[fn]();
  const d = new DataView(x.memory.buffer);
  const len = d.getUint32(p, true);
  const got = new TextDecoder().decode(new Uint8Array(x.memory.buffer, p + 8, len));
  x.paph_release(p);
  const ms = performance.now() - t0;
  const a = got.split('\n'), b = want.split('\n');
  let bad = 0, first = '';
  for (let i = 0; i < Math.max(a.length, b.length); i++) if (a[i] !== b[i]) { if (!bad) first = (a[i] || '∅') + ' ≠ ' + (b[i] || '∅'); bad++; }
  const label = `${name}, wire ${ref.includes('-4') ? 4 : 3}`;
  if (bad) { fail++; console.log(`  FAIL ${label}: ${bad} of ${b.length - 1} lines differ; first: ${first}`); }
  else console.log(`  PASS ${label}: ${b.length - 1} cases byte-identical to native (${(ms / 1000).toFixed(1)} s)`);
}
process.exit(fail ? 1 : 0);
