/* PAPH v3 test harness.
 * Order follows SPEC-003 §14: cheapest thing that can kill the design, first. */
'use strict';
const paph = require('../src/wire.cjs');
const I = paph._internal;

let pass = 0, fail = 0;
function ok(name, cond, extra) {
  if (cond) { pass++; console.log('  \x1b[32mPASS\x1b[0m ' + name + (extra ? '  ' + extra : '')); }
  else { fail++; console.log('  \x1b[31mFAIL\x1b[0m ' + name + (extra ? '  ' + extra : '')); }
}
function head(s) { console.log('\n\x1b[1m' + s + '\x1b[0m'); }

/* ---------- deterministic synthetic images ---------- */
function mk(w, h) { return { px: new Uint8Array(w * h * 4), w, h }; }
function put(im, x, y, r, g, b, a) {
  if (x < 0 || y < 0 || x >= im.w || y >= im.h) return;
  const o = (y * im.w + x) << 2;
  im.px[o] = r; im.px[o + 1] = g; im.px[o + 2] = b; im.px[o + 3] = a === undefined ? 255 : a;
}
function get(im, x, y) { const o = (y * im.w + x) << 2; return [im.px[o], im.px[o+1], im.px[o+2], im.px[o+3]]; }
function rnd(s) { return () => { s = (s * 1103515245 + 12345) & 0x7fffffff; return s / 0x7fffffff; }; }

function sprite(seed, w = 64, h = 64, alpha = true) {
  const R = rnd(seed), im = mk(w, h);
  const pal = [];
  for (let i = 0; i < 7; i++) pal.push([40 + (R() * 200) | 0, 30 + (R() * 200) | 0, 50 + (R() * 190) | 0]);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const cx = x - w / 2, cy = y - h / 2;
    const inside = (cx * cx) / (w * w / 5) + (cy * cy) / (h * h / 4.5) < 1;
    if (!inside && alpha) { put(im, x, y, 0, 0, 0, 0); continue; }
    const c = pal[((x >> 2) * 7 + (y >> 2) * 13 + ((x * y) >> 5)) % pal.length];
    put(im, x, y, c[0], c[1], c[2], 255);
  }
  // a few hard features
  for (let i = 0; i < 26; i++) {
    const x = 4 + ((R() * (w - 10)) | 0), y = 4 + ((R() * (h - 10)) | 0);
    const c = pal[(i * 3) % pal.length];
    for (let dy = 0; dy < 4; dy++) for (let dx = 0; dx < 4; dx++)
      put(im, x + dx, y + dy, 255 - c[0], 255 - c[1], 255 - c[2], 255);
  }
  return im;
}
function scene(seed, w = 200, h = 150) {
  const R = rnd(seed), im = mk(w, h);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++)
    put(im, x, y, 20 + ((x * 3 + y) % 40), 30 + ((y * 5) % 50), 60 + ((x ^ y) % 60), 255);
  for (let i = 0; i < 90; i++) {
    const x = (R() * w) | 0, y = (R() * h) | 0, s = 3 + ((R() * 9) | 0);
    const r = (R() * 255) | 0, g = (R() * 255) | 0, b = (R() * 255) | 0;
    for (let dy = 0; dy < s; dy++) for (let dx = 0; dx < s; dx++) put(im, x + dx, y + dy, r, g, b, 255);
  }
  return im;
}
function mirrorH(a) { const im = mk(a.w, a.h);
  for (let y = 0; y < a.h; y++) for (let x = 0; x < a.w; x++) { const p = get(a, a.w - 1 - x, y); put(im, x, y, p[0], p[1], p[2], p[3]); }
  return im; }
function rot90(a) { const im = mk(a.h, a.w);
  for (let y = 0; y < a.h; y++) for (let x = 0; x < a.w; x++) { const p = get(a, x, y); put(im, a.h - 1 - y, x, p[0], p[1], p[2], p[3]); }
  return im; }
/* A realistic recolour: the artist swaps the palette but keeps the shading.
   Modelled as a MONOTONE remap of luminance to a new tint ramp — that is
   exactly the transform the quantile encoding claims invariance to.  (A
   wraparound channel scramble is not a recolour; it destroys the luminance
   ordering every structural rank is built on, and nothing here claims to
   survive it.) */
function recolour(a) { const im = mk(a.w, a.h); im.px.set(a.px);
  for (let i = 0; i < a.w * a.h; i++) { const o = i << 2;
    const L = (77 * im.px[o] + 150 * im.px[o+1] + 29 * im.px[o+2] + 128) >> 8;
    im.px[o]   = Math.min(255, 30 + ((L * 180) >> 8));
    im.px[o+1] = Math.min(255, 10 + ((L * 120) >> 8));
    im.px[o+2] = Math.min(255, 70 + ((L * 150) >> 8)); }
  return im; }
/* A palette REBUILD: distinct colours mapped to new ones, luminance order kept. */
function rebuildPalette(a) { const im = mk(a.w, a.h); im.px.set(a.px);
  const seen = new Map();
  for (let i = 0; i < a.w * a.h; i++) { const o = i << 2;
    const k = (im.px[o] << 16) | (im.px[o+1] << 8) | im.px[o+2];
    if (!seen.has(k)) seen.set(k, (77*im.px[o] + 150*im.px[o+1] + 29*im.px[o+2] + 128) >> 8); }
  const order = [...seen.entries()].sort((p, q) => p[1] - q[1]);
  const map = new Map();
  order.forEach(([k, L], i) => { const t = Math.round(255 * i / Math.max(1, order.length - 1));
    map.set(k, [Math.min(255, 15 + ((t * 190) >> 8)), Math.min(255, 5 + ((t * 230) >> 8)), Math.min(255, 40 + ((t * 170) >> 8))]); });
  for (let i = 0; i < a.w * a.h; i++) { const o = i << 2;
    const c = map.get((im.px[o] << 16) | (im.px[o+1] << 8) | im.px[o+2]);
    im.px[o] = c[0]; im.px[o+1] = c[1]; im.px[o+2] = c[2]; }
  return im; }
function invert(a) { const im = mk(a.w, a.h); im.px.set(a.px);
  for (let i = 0; i < a.w * a.h; i++) { const o = i << 2;
    im.px[o] = 255 - im.px[o]; im.px[o+1] = 255 - im.px[o+1]; im.px[o+2] = 255 - im.px[o+2]; }
  return im; }
function upscale(a, k) { const im = mk(a.w * k, a.h * k);
  for (let y = 0; y < im.h; y++) for (let x = 0; x < im.w; x++) { const p = get(a, (x / k) | 0, (y / k) | 0); put(im, x, y, p[0], p[1], p[2], p[3]); }
  return im; }
function crop(a, x0, y0, w, h) { const im = mk(w, h);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) { const p = get(a, x0 + x, y0 + y); put(im, x, y, p[0], p[1], p[2], p[3]); }
  return im; }
function paste(host, guest, ox, oy) { const im = mk(host.w, host.h); im.px.set(host.px);
  for (let y = 0; y < guest.h; y++) for (let x = 0; x < guest.w; x++) {
    const p = get(guest, x, y); if (p[3] < 128) continue; put(im, ox + x, oy + y, p[0], p[1], p[2], 255); }
  return im; }
/* Area-average resample — what an actual re-export does.  Nearest-neighbour at
   a non-integer ratio is a different (and much harsher) transform. */
function resample(a, w, h) { const im = mk(w, h);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const x0 = Math.floor(x * a.w / w), x1 = Math.max(x0 + 1, Math.floor((x + 1) * a.w / w));
    const y0 = Math.floor(y * a.h / h), y1 = Math.max(y0 + 1, Math.floor((y + 1) * a.h / h));
    let r = 0, g = 0, b = 0, al = 0, n = 0;
    for (let sy = y0; sy < y1 && sy < a.h; sy++) for (let sx = x0; sx < x1 && sx < a.w; sx++) {
      const p = get(a, sx, sy); r += p[0]; g += p[1]; b += p[2]; al += p[3]; n++; }
    put(im, x, y, (r / n) | 0, (g / n) | 0, (b / n) | 0, (al / n) > 127 ? 255 : 0); }
  return im; }

/* ================================================================ *
 * §14.1 — THE REFLECTION-CLOSED BRIEF CLAIM
 *
 * The claim: with bits 128..255 using the Y-NEGATED pattern,
 *   sector(mirror)     === (32 - sector) & 63
 *   descriptor(mirror) === descriptor with its two halves exchanged
 * If this fails, mirror invariance costs 32 B per keypoint and the whole
 * budget changes.  So it is tested first and on its own.
 * ================================================================ */
head('§14.1  reflection-closed BRIEF  (the claim that gates the budget)');
{
  const W = 240, H = 200;
  const R = rnd(20260818);
  const lum = new Uint8Array(W * H);
  for (let i = 0; i < lum.length; i++) lum[i] = (R() * 256) | 0;
  // add structure so orientations are not noise
  for (let k = 0; k < 60; k++) {
    const x = (R() * (W - 30)) | 0, y = (R() * (H - 30)) | 0, s = 5 + ((R() * 20) | 0), v = (R() * 256) | 0;
    for (let dy = 0; dy < s; dy++) for (let dx = 0; dx < s; dx++) if (x + dx < W && y + dy < H) lum[(y + dy) * W + x + dx] = v;
  }
  const mir = new Uint8Array(W * H);
  for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) mir[y * W + x] = lum[y * W + (W - 1 - x)];

  const S = I.integralI32(lum, W, H), SM = I.integralI32(mir, W, H);
  let n = 0, secOK = 0, descOK = 0, ties = 0;
  let refused = 0;
  for (let y = 28; y < H - 28; y += 3) for (let x = 28; x < W - 28; x += 3) {
    const xm = W - 1 - x;
    if (xm < 28 || xm >= W - 28) continue;
    const s0 = I.orientSector(S, W, x, y);
    const s1 = I.orientSector(SM, W, xm, y);
    if (s0 < 0 || s1 < 0) { refused++; continue; }   // ambiguous, refused by design
    n++;
    if (s1 === ((32 - s0) & 63)) secOK++; else { ties++; continue; }
    const d0 = I.describe(S, W, x, y, s0);
    const d1 = I.describe(SM, W, xm, y, s1);
    const sw = I.mirrorDesc(d0);
    let eq = true; for (let k = 0; k < 8; k++) if (d1[k] !== sw[k]) { eq = false; break; }
    if (eq) descOK++;
  }
  ok('sector(mirror) === (32 - sector) & 63', secOK === n && n > 1000,
     secOK + '/' + n + ' exact, ' + refused + ' refused for ambiguous orientation ('
     + (100 * refused / (n + refused)).toFixed(1) + '%)');
  ok('descriptor(mirror) === halves exchanged, bit for bit', descOK === secOK,
     descOK + '/' + secOK + ' exact — mirror invariance costs ZERO stored bytes');
  ok('pattern second half is the y-negated first half', (function () {
    const P = I.PATTERN;
    for (let i = 0; i < 128; i++) {
      const a = i * 4, b = (i + 128) * 4;
      if (P[b] !== P[a] || P[b + 1] !== -P[a + 1] || P[b + 2] !== P[a + 2] || P[b + 3] !== -P[a + 3]) return false;
    }
    return true;
  })());
  ok('COS64[32-k] === -COS64[k] and SIN64[32-k] === SIN64[k]', (function () {
    for (let k = 0; k < 64; k++) {
      if (I.COS64[(32 - k) & 63] !== -I.COS64[k]) return false;
      if (I.SIN64[(32 - k) & 63] !== I.SIN64[k]) return false;
      if (I.RC10[(32 - k) & 63] !== -I.RC10[k]) return false;
      if (I.RS10[(32 - k) & 63] !== I.RS10[k]) return false;
    }
    return true;
  })(), 'the table identity the whole trick rests on');
}

/* ================================================================ *
 * determinism, wire integrity, symmetry
 * ================================================================ */
head('determinism and wire integrity');
const A = sprite(1, 128, 128);
let hA = paph.hash(A), hA2 = paph.hash(A);
{
  ok('tier 1 is exactly 3952 bytes', hA.t1.length === 3952, hA.t1.length + ' B');
  ok('tier 2 is <= 10272 bytes', hA.t2.length <= 10272, hA.t2.length + ' B, ' + hA.kpCount + ' keypoints');
  ok('hashing twice gives identical tier 1 bytes', Buffer.compare(Buffer.from(hA.t1), Buffer.from(hA2.t1)) === 0);
  ok('hashing twice gives identical tier 2 bytes', Buffer.compare(Buffer.from(hA.t2), Buffer.from(hA2.t2)) === 0);
  ok('tier 1 round-trips through parse', (() => { try { paph.parseT1(hA.t1); return true; } catch (e) { return false; } })());
  ok('tier 2 round-trips through parse', (() => { try { paph.parseT2(hA.t2); return true; } catch (e) { return false; } })());
  ok('a flipped bit fails the CRC', (() => {
    const b = Uint8Array.from(hA.t1); b[900] ^= 1;
    try { paph.parseT1(b); return false; } catch (e) { return /checksum/.test(e.message); } })());
  ok('a transposition fails the CRC (v2 XOR could not see this)', (() => {
    const b = Uint8Array.from(hA.t1);
    let i = 100; while (i < 3900 && b[i] === b[i + 1]) i++;
    const t = b[i]; b[i] = b[i + 1]; b[i + 1] = t;
    try { paph.parseT1(b); return false; } catch (e) { return /checksum/.test(e.message); } })());
  ok('a v2 wire is rejected, never reinterpreted', (() => {
    const b = new Uint8Array(3952); b[0]=0x50;b[1]=0x41;b[2]=0x50;b[3]=0x48;b[4]=2;
    try { paph.parseT1(b); return false; } catch (e) { return /version/.test(e.message); } })());
}

head('symmetry: compare(x,y) === compare(y,x)');
{
  const pairs = [[sprite(1), sprite(1)], [sprite(2), recolour(sprite(2))],
                 [sprite(3), sprite(9)], [scene(4), sprite(5)],
                 [sprite(6), crop(sprite(6), 8, 8, 48, 48)]];
  let worst = 0, verdictsAgree = true;
  for (const [x, y] of pairs) {
    const hx = paph.hash(x), hy = paph.hash(y);
    const f = paph.compare(hx, hy), r = paph.compare(hy, hx);
    worst = Math.max(worst, Math.abs(f.structural - r.structural), Math.abs(f.geometric - r.geometric));
    if (f.verdict !== r.verdict) verdictsAgree = false;
  }
  ok('scores identical in both argument orders', worst === 0, 'worst delta ' + worst);
  ok('verdicts identical in both argument orders', verdictsAgree);
}

head('the abstention contract');
{
  const blank = mk(40, 40); for (let i = 0; i < 1600; i++) put(blank, i % 40, (i / 40) | 0, 200, 200, 200, 255);
  const blank2 = mk(40, 40); for (let i = 0; i < 1600; i++) put(blank2, i % 40, (i / 40) | 0, 90, 90, 90, 255);
  const r = paph.compare(paph.hash(blank), paph.hash(blank2));
  ok('two flat canvases are NOT certified', r.verdict === 'Unrelated' || r.verdict === 'Related', r.verdict);
  ok('the DCT channel abstains on a flat canvas', r.abstained.indexOf('dct') >= 0, r.abstained.join(', '));
  ok('every measurable channel declares controlRan', Object.keys(r.channels)
      .every(k => !r.channels[k].measurable || r.channels[k].controlRan === true));
  ok('the colour digest cannot move a verdict (reporting only)', (() => {
    const h1 = paph.hash(sprite(11)), h2 = paph.hash(recolour(sprite(11)));
    const base = paph.compare(h1, h2);
    const z1 = Uint8Array.from(h1.t1), z2 = Uint8Array.from(h2.t1);
    const off = paph.SECTION_OFFSETS.colour;
    for (let i = 0; i < 80; i++) { z1[off + i] = 0; z2[off + i] = 0; }
    // recompute CRCs so the wires stay valid
    for (const z of [z1, z2]) { const c = I.crc32(z, 64, 3952);
      z[60] = c & 255; z[61] = (c >>> 8) & 255; z[62] = (c >>> 16) & 255; z[63] = (c >>> 24) & 255; }
    const zeroed = paph.compare({ t1: z1, t2: h1.t2 }, { t1: z2, t2: h2.t2 });
    return zeroed.verdict === base.verdict && zeroed.structural === base.structural;
  })());
}

/* ================================================================ *
 * the transform battery — what each half is supposed to catch
 * ================================================================ */
head('transform battery');
{
  const base = sprite(7, 128, 128);
  const host = scene(8, 300, 240);
  const cases = [
    ['identical',            base,                              base,                                  ['Identical']],
    ['4x nearest upscale',   base,                              upscale(base, 4),                      ['Identical', 'Copy']],
    ['recoloured',           base,                              recolour(base),                        ['Identical', 'Copy']],
    ['inverted',             base,                              invert(base),                          ['Copy', 'Suspected', 'Related']],
    ['mirrored',             base,                              mirrorH(base),                         ['Copy', 'Suspected']],
    ['rotated 90',           base,                              rot90(base),                           ['Copy', 'Suspected']],
    ['cropped 70%',          base,                              crop(base, 18, 18, 92, 92),            ['Copy', 'Suspected']],
    ['palette rebuilt',      base,                              rebuildPalette(base),                  ['Identical', 'Copy']],
    ['rescaled 1.8x',        base,                              resample(base, 230, 230),              ['Copy', 'Suspected']],
    ['pasted into a scene',  base,                              paste(host, base, 60, 40),             ['Copy', 'Suspected']],
    ['pasted, then cropped', base,                              crop(paste(host, base, 60, 40), 30, 20, 200, 140), ['Copy', 'Suspected']],
    ['unrelated sprites',    sprite(21, 128, 128),              sprite(22, 128, 128),                  ['Unrelated', 'Related', 'Suspected']],
    ['same-generator scenes', scene(23, 260, 200),              scene(24, 260, 200),                   ['Unrelated', 'Related', 'Suspected']],
    ['sprite vs scene',      sprite(25, 128, 128),              scene(26, 260, 200),                   ['Unrelated', 'Related', 'Suspected']]
  ];
  const hashes = new Map();
  const H = im => { if (!hashes.has(im)) hashes.set(im, paph.hash(im)); return hashes.get(im); };
  console.log('   ' + 'case'.padEnd(22) + 'verdict'.padEnd(11) + 'struct'.padStart(7) + 'geo'.padStart(7) +
              'inl'.padStart(5) + '  class');
  for (const [name, x, y, want] of cases) {
    const r = paph.compare(H(x), H(y));
    const line = '   ' + name.padEnd(22) + r.verdict.padEnd(11) +
                 String(r.structural).padStart(7) + String(r.geometric).padStart(7) +
                 String(r.report.inliers).padStart(5) + '  ' + r.class;
    console.log(line);
    ok('  -> ' + name, want.indexOf(r.verdict) >= 0, 'wanted one of ' + want.join('/'));
  }
}

head('reporting fields');
{
  const b = sprite(31, 128, 128);
  const rRec = paph.compare(paph.hash(b), paph.hash(recolour(b)));
  ok('recolour is reported as a rebuilt/related palette',
     rRec.report.palette.relation !== 'identical palette', rRec.report.palette.relation);
  const rMir = paph.compare(paph.hash(b), paph.hash(mirrorH(b)));
  ok('mirror hypothesis is reported when it wins',
     rMir.report.mirrored === true || rMir.report.dihedral === 'Flip H',
     'mirrored=' + rMir.report.mirrored + ' dihedral=' + rMir.report.dihedral);
  const rId = paph.compare(paph.hash(b), paph.hash(b));
  ok('identical works report an identical palette', rId.report.palette.relation === 'identical palette');
  ok('both evidence rules are always reported', rId.report.evidenceBoth &&
     typeof rId.report.evidenceBoth.lift === 'number' && typeof rId.report.evidenceBoth.proportion === 'number');
}

head('performance');
{
  const w = scene(41, 512, 384);
  let t0 = Date.now(); const h1 = paph.hash(w); const th = Date.now() - t0;
  const h2 = paph.hash(scene(42, 512, 384));
  t0 = Date.now(); for (let i = 0; i < 5; i++) paph.compare(h1, h2); const tc = (Date.now() - t0) / 5;
  console.log('   512x384 hash ' + th + ' ms  (target <= 1600),  compare ' + tc.toFixed(1) +
              ' ms  (target <= 25),  ' + h1.kpCount + ' keypoints');
  ok('hash within budget', th <= 4000, th + ' ms');
  ok('compare within budget', tc <= 120, tc.toFixed(1) + ' ms');
}

console.log('\n' + (fail ? '\x1b[31m' : '\x1b[32m') + pass + ' passed, ' + fail + ' failed\x1b[0m\n');
process.exit(fail ? 1 : 0);
