/**
 * Wire 4's golden vectors (docs/golden/GOLDEN-W4.json, emitted by the Rust
 * reference: `printf g | rust/target/release/paphcli`), against the
 * JavaScript engine and both WebAssembly builds — no native binary needed.
 *
 *   identity   the hash-profile id of the defaults in each format
 *   cells      closed and open cell spans (SPEC-W4 §2)
 *   dct        the transform rounded once, beside wire 3's two passes (§3)
 *   rays       the D4 permutation of the 32 rays, and radial extents from
 *              the exact centroid on an L, a ring, a U and a staircase (§5) —
 *              then the same masks under all eight symmetries, whose extents
 *              must be the extents permuted
 *   ties       the silhouette's component when shapes share the largest
 *              area (§4): pairs the moments or the occupancy settle, and
 *              twins only the scan order can — the section on each canvas
 *              and on its seven other D4 images
 *   images     synth::pixel_art ported below (checked against pixels_sha256
 *              first), each hashed in both formats: Tier 1 section by
 *              section, Tier 2, and the seven other D4 images of three
 *
 *     node test/wire4-golden.cjs
 */
'use strict';
const fs = require('fs');
const path = require('path');
const { createHash } = require('crypto');
const { pathToFileURL } = require('url');
const W = require('../src/wire.cjs');
const V = require('../src/paph-js.cjs');
const I = W._internal;

const G = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'docs', 'golden', 'GOLDEN-W4.json'), 'utf8'));
const C = { g: '\u001b[32m', r: '\u001b[31m', d: '\u001b[2m', x: '\u001b[0m' };
let pass = 0, fail = 0;
const ok = (c, name, extra) => {
  if (c) pass++; else fail++;
  console.log(`  ${c ? C.g + 'PASS' : C.r + 'FAIL'}${C.x} ${name}${extra ? ' ' + C.d + extra + C.x : ''}`);
};
const head = s => console.log(`\n${s}`);
const sha = b => createHash('sha256').update(b).digest('hex');
const hex = b => Buffer.from(b).toString('hex');
const same = (a, b) => a.length === b.length && Array.from(a).every((v, i) => v === b[i]);

/* ---- synth::pixel_art, ported (rust/src/synth.rs): integers only ---- */
const M64 = (1n << 64n) - 1n;
function rng(seed) {
  let s = ((BigInt(seed) * 0x9e3779b97f4a7c15n) & M64) | 1n;
  const next = () => {
    let x = s;
    x ^= (x << 13n) & M64;
    x ^= x >> 7n;
    x ^= (x << 17n) & M64;
    s = x;
    return x;
  };
  return { below: n => (n === 0 ? 0 : Number(next() % BigInt(n))) };
}
const BAYER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
function pixelArt(w, h, seed, ncol, bg) {
  const r = rng(seed);
  const pal = [];
  for (let i = 0; i < Math.max(ncol, 2); i++) pal.push([r.below(256), r.below(256), r.below(256), 255]);
  const px = new Uint8Array(w * h * 4);
  const set = (x, y, c) => {
    if (x < 0 || y < 0 || x >= w || y >= h) return;
    px.set(c, (y * w + x) * 4);
  };
  const a = pal[0], b = pal[1 % pal.length];
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    let c;
    if (bg === 0) c = [0, 0, 0, 0];
    else if (bg === 1) c = [40, 44, 60, 255];
    else { const t = Math.floor((y * 15) / Math.max(h, 1)); c = BAYER[((y & 3) << 2) | (x & 3)] < t ? a : b; }
    set(x, y, c);
  }
  const dither = (x, y, thr, c1, c2) => (BAYER[((y & 3) << 2) | (x & 3)] < thr ? c1 : c2);
  const shapes = 3 + r.below(10);
  for (let n = 0; n < shapes; n++) {
    const c1 = pal[r.below(pal.length)], c2 = pal[r.below(pal.length)];
    const x0 = r.below(w), y0 = r.below(h);
    const sw = 1 + r.below(Math.max(Math.floor(w / 2), 1)), sh = 1 + r.below(Math.max(Math.floor(h / 2), 1));
    const thr = r.below(16);
    switch (r.below(4)) {
      case 0:
        for (let y = y0; y < y0 + sh; y++) for (let x = x0; x < x0 + sw; x++) {
          const edge = x === x0 || y === y0 || x === x0 + sw - 1 || y === y0 + sh - 1;
          set(x, y, edge ? [12, 10, 20, 255] : dither(x, y, thr, c1, c2));
        }
        break;
      case 1: {
        const rx = Math.max(sw, 2), ry = Math.max(sh, 2);
        for (let y = y0 - ry; y <= y0 + ry; y++) for (let x = x0 - rx; x <= x0 + rx; x++) {
          const d = (x - x0) * (x - x0) * ry * ry + (y - y0) * (y - y0) * rx * rx;
          if (d <= rx * rx * ry * ry) set(x, y, dither(x, y, thr, c1, c2));
        }
        break;
      }
      case 2: {
        const len = Math.max(sw, sh) * 2;
        for (let t = 0; t < len; t++) { set(x0 + t, y0 + (t >> 1), c1); set(x0 + t, y0 + (t >> 1) + 1, c2); }
        break;
      }
      default:
        for (let k = 0; k < Math.max(Math.floor((sw * sh) / 3), 1); k++) set(x0 + ((k * 7919) % sw), y0 + ((k * 104729) % sh), c1);
    }
  }
  return { px, w, h };
}
function mirror({ px, w, h }) {
  const o = new Uint8Array(px.length);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) o.set(px.subarray((y * w + w - 1 - x) * 4, (y * w + w - x) * 4), (y * w + x) * 4);
  return { px: o, w, h };
}
function rot90({ px, w, h }) {
  const o = new Uint8Array(px.length);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) o.set(px.subarray((y * w + x) * 4, (y * w + x + 1) * 4), (x * h + h - 1 - y) * 4);
  return { px: o, w: h, h: w };
}
/* "mirror.rotK" is mirror(rotK(image)) */
function d4Named(im, g) {
  const turns = { rot90: 1, rot180: 2, rot270: 3 }[g.replace(/^mirror\.?/, '')] || 0;
  let b = im;
  for (let k = 0; k < turns; k++) b = rot90(b);
  return g.startsWith('mirror') ? mirror(b) : b;
}

async function main() {
  head('golden: header');
  ok(G.spec === 'PAPH-SPEC-W4' && G.format === 4 && W.VERSION === 4, 'GOLDEN-W4 declares wire 4, and the engine writes it by default', `${G.spec}, format ${G.format}`);

  head('golden: hash-profile identity (SPEC-W4 §1)');
  ok(V.hex(V.hashProfileId(Object.assign({}, W.DEFAULT_CONFIG, { wire: 4 }))) === G.hash_profile_id.wire4, 'hash_profile_id(defaults, wire 4)', G.hash_profile_id.wire4.slice(0, 16));
  ok(V.hex(V.hashProfileId(Object.assign({}, W.DEFAULT_CONFIG, { wire: 3 }))) === G.hash_profile_id.wire3, 'hash_profile_id(defaults, wire 3)', G.hash_profile_id.wire3.slice(0, 16));

  head('golden: closed cells (§2)');
  for (const c of G.cell_span) {
    const cl = [], op = [];
    for (let i = 0; i < c.n; i++) { cl.push(I.cellSpan(i, c.w, c.n, true)); op.push(I.cellSpan(i, c.w, c.n, false)); }
    ok(JSON.stringify(cl) === JSON.stringify(c.closed) && JSON.stringify(op) === JSON.stringify(c.open), `${c.n} cells across ${c.w} pixels, closed and open`);
    // the property the closed cover exists for: a flip maps cell i onto cell n-1-i
    ok(cl.every(([x0, x1], i) => { const [y0, y1] = cl[c.n - 1 - i]; return c.w - x1 === y0 && c.w - x0 === y1; }), `${c.n} cells across ${c.w}: a flip maps each closed cell onto its mirror cell`);
  }

  head('golden: the DCT rounded once (§3)');
  for (const d of G.dct) {
    ok(same(I.dct2Exact(d.src, d.n), d.exact), `${d.n}×${d.n} wire 4 (one odd rounding)`);
    ok(same(I.dct2Wire3(d.src, d.n), d.wire3), `${d.n}×${d.n} wire 3 (two passes)`);
    // a horizontal flip negates exactly the odd horizontal frequencies
    const fl = d.src.map((_, k) => d.src[Math.floor(k / d.n) * d.n + d.n - 1 - (k % d.n)]);
    const e = I.dct2Exact(fl, d.n);
    ok(Array.from(e).every((v, k) => v === ((k % d.n) & 1 ? -d.exact[k] : d.exact[k])), `${d.n}×${d.n} a flipped block's coefficients are the block's, odd frequencies negated`);
  }

  head('golden: rays (§5)');
  ok(G.ray_d4.every((row, e) => row.every((v, k) => I.rayD4(k, e) === v)), 'ray k after symmetry e, all 8×32');
  for (const m of G.rays) {
    const w = m.rows[0].length, h = m.rows.length;
    const id = new Int32Array(w * h);
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) id[y * w + x] = m.rows[y][x] === '#' ? 0 : -1;
    const [minx, maxx, miny, maxy] = m.box;
    const rad = I.raysExact(id, w, 0, m.area, m.sx, m.sy, minx, maxx, miny, maxy, m.lim);
    const prof = I.profileBytes(rad);
    ok(same(rad, m.rays) && same(prof, m.profile) && same(I.canonicalProfile(prof), m.canonical), `${m.name}: extents, profile and canonical profile`, m.rays.join(' '));
    // the mask under each symmetry e (bit 2 swap the axes, then bit 0 flip x,
    // then bit 1 flip y): its extents are the extents permuted
    let good = 0;
    for (let e = 0; e < 8; e++) {
      const tw = e & 4 ? h : w, th = e & 4 ? w : h;
      const tid = new Int32Array(tw * th).fill(-1);
      let area = 0, sx = 0, sy = 0, bx0 = 1e9, bx1 = -1, by0 = 1e9, by1 = -1;
      for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
        if (id[y * w + x] !== 0) continue;
        let u = x, v = y;
        if (e & 4) [u, v] = [v, u];
        if (e & 1) u = tw - 1 - u;
        if (e & 2) v = th - 1 - v;
        tid[v * tw + u] = 0; area++; sx += u; sy += v;
        bx0 = Math.min(bx0, u); bx1 = Math.max(bx1, u); by0 = Math.min(by0, v); by1 = Math.max(by1, v);
      }
      const tr = I.raysExact(tid, tw, 0, area, sx, sy, bx0, bx1, by0, by1, m.lim);
      if (m.rays.every((v, k) => tr[I.rayD4(k, e)] === v)) good++;
    }
    ok(good === 8, `${m.name}: under all eight symmetries the extents are the extents permuted`, `${good} of 8`);
  }

  /* ---- engines ---- */
  const engines = [['JavaScript', (im, wire) => W.hash(im.px, im.w, im.h, { wire })]];
  const wasm = path.join(__dirname, '..', 'wasm', 'paph.wasm');
  if (fs.existsSync(wasm)) {
    const { init } = await import(pathToFileURL(path.join(__dirname, '..', 'wasm', 'paph.js')).href);
    const simd = await init();
    const base = await init(pathToFileURL(path.join(__dirname, '..', 'wasm', 'paph-baseline.wasm')));
    engines.push([`WebAssembly (${simd.simd ? 'SIMD128' : 'baseline'})`, (im, wire) => simd.hash(im, { wire })]);
    engines.push(['WebAssembly (baseline)', (im, wire) => base.hash(im, { wire })]);
  }

  head('golden: the silhouette\'s ties (§4)');
  const T = G.silhouette_ties;
  for (const t of T.canvases) {
    const px = new Uint8Array(t.w * t.h * 4);
    for (const s of t.stamps) for (const [x, y] of s.cells) px.set(T.rgba, ((s.at[1] + y) * t.w + s.at[0] + x) * 4);
    const im = { px, w: t.w, h: t.h };
    const o = W.SECTION_OFFSETS.silhouette;
    for (const [en, hash] of engines) {
      const sil = m => hex(hash(m, 4).t1.subarray(o, o + 96));
      const miss = [['identity', im]].concat(t.d4.map(d => [d.g, d4Named(im, d.g)]))
        .filter(([g, m], k) => sil(m) !== (k === 0 ? t.silhouette : t.d4[k - 1].silhouette)).map(([g]) => g);
      ok(miss.length === 0, `${en} ${t.name} (settled by ${t.settled_by}): the silhouette on all eight images`, miss.length ? 'differs: ' + miss.join(' ') : '');
    }
  }

  head('golden: images (synth::pixel_art)');
  for (const g of G.images) {
    const im = pixelArt(g.w, g.h, g.seed, g.ncol, g.bg);
    ok(sha(im.px) === g.pixels_sha256, `${g.name}: the port draws the reference's pixels`, g.pixels_sha256.slice(0, 16));
    for (const [en, hash] of engines) {
      const f4 = hash(im, 4), f3 = hash(im, 3);
      const bad = W.SECTIONS.filter(s => {
        const o = W.SECTION_OFFSETS[s.name];
        return sha(f4.t1.subarray(o, o + s.len)).slice(0, 16) !== g.wire4.sections[s.name];
      }).map(s => s.name);
      ok(bad.length === 0, `${en} ${g.name}: every Tier 1 section, wire 4`, bad.length ? 'differs: ' + bad.join(' ') : '');
      ok(sha(f4.t1) === g.wire4.t1_sha256 && (!g.wire4.t1 || hex(f4.t1) === g.wire4.t1) && f4.t1[4] === 4, `${en} ${g.name}: Tier 1, wire 4${g.wire4.t1 ? ' (all 3952 bytes)' : ''}`);
      ok(f4.t2.length === g.wire4.t2_len && sha(f4.t2) === g.wire4.t2_sha256 && f4.t2[4] === 4, `${en} ${g.name}: Tier 2, wire 4`, `${g.wire4.t2_len} B`);
      ok(sha(f3.t1) === g.wire3.t1_sha256 && f3.t2.length === g.wire3.t2_len && sha(f3.t2) === g.wire3.t2_sha256 && f3.t1[4] === 3, `${en} ${g.name}: Tier 1 and Tier 2, wire 3`);
      if (g.d4) {
        const miss = g.d4.filter(d => { const f = hash(d4Named(im, d.g), 4); return sha(f.t1) !== d.t1_sha256 || sha(f.t2) !== d.t2_sha256; }).map(d => d.g);
        ok(miss.length === 0, `${en} ${g.name}: its seven other D4 images, both tiers`, miss.length ? 'differs: ' + miss.join(' ') : '');
      }
    }
  }

  console.log(`\n${fail ? C.r : C.g}${pass} passed, ${fail} failed${C.x}`);
  process.exit(fail ? 1 : 0);
}
main().catch(e => { console.error(e); process.exit(1); });
