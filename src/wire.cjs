/*! paph-js — the wire engine (PAPH tier 1 + tier 2, SPEC-003).
 *  Formerly paph3.js; — Pixel Art Perceptual Hash (PAPH) v3.0, JavaScript reference engine.
 *  Implements PAPH-SPEC-003.  Deterministic, integer-only, zero dependencies.
 *
 *  One algorithm, two halves, one wire:
 *    Tier 1 (3952 B, fixed layout)  structural bag-of-features + keypoint sketch
 *    Tier 2 (<=10272 B, variable)   full keypoint records for geometric verification
 *
 *  Every value that reaches either wire is derived with |0, >>, >>> and exact
 *  integer products.  No Math.random, no Date, no locale, no atan2, no sqrt on
 *  floats at hash time.  The same input bytes produce the same wire bytes on any
 *  engine, which is the property a chain needs — and, for the first time in this
 *  family, that now includes the GEOMETRIC stage.  MIT. */
(function (root, factory) {
  var m = factory();
  if (typeof module === 'object' && module.exports) module.exports = m;
  else if (typeof define === 'function' && define.amd) define([], function () { return m; });
  else root.paphWire = m;
})(typeof self !== 'undefined' ? self : this, function () {
'use strict';

var VERSION = 3;

/* ---- knobs --------------------------------------------------------
 * SPEC-003 P1: every knob that is a genuine trade-off is now a COMPARE-time
 * knob.  Nothing below changes the wire bytes except localCount, localWindows
 * and kpCount.  Re-calibrating `evidence`, `scoring`, `hammingT`, thresholds
 * or the verdict lattice costs milliseconds per pair and no re-hashing.
 *
 * `keepDC` and `ragEndpoint` are GONE as knobs: v3 stores both sides
 * (§6.3) and the compare reads whichever it wants.
 * ------------------------------------------------------------------ */
var DEFAULTS = {
  /* --- hash time (these do change the wire) --- */
  foldMatte:     true,
  divideUpscale: true,
  matteTol:      24,
  peakRadius:    5,
  foldInvert:    true,
  localWindows:  [8, 16],
  localCount:    128,     // v2 had 64.  SPEC-003 §14.3 — hypothesis, measure it.
  kpCount:       512,     // Tier 2 cap — SPEC-004.2 §2 raised it from 256
  kpSelect:      1,       // SPEC-004.2 §3: 0 = 4.1 strength/grid, 1 = 4.2 quality
  sketchCount:   32,      // Tier 1 cap

  /* --- compare time (free to re-derive at any moment) --- */
  hammingT:      8,
  evidence:      'lift',
  confidenceAt:  16,
  scoring:       'weighted',
  ragEndpoint:   'rank',      // which encoding SCORES; the other is reported
  geoEnabled:    true,
  geoConfAt:     16,
  geoEps:        1600,        // aspect-true units of 65535 -> ~2.4%
  geoMinCorr:    8,
  mirrorHypothesis: true
};

/* ---- fixed-point scales ---- */
var SCALE = 10000;

/* Verdict thresholds.  Structural side inherited from v2, where they were
   derived from 16 REAL works rather than fixtures.  Geometric side is a
   PROPOSAL — SPEC-003 §14.7 says re-derive on real moderation reports. */
var THRESH = {
  STRUCT_IDENTICAL: 8000, STRUCT_STRONG: 4500, STRUCT_MODERATE: 3000, STRUCT_WEAK: 1500,
  GEO_STRONG: 3500, GEO_WEAK: 1200,
  GEO_SOLO_INLIERS: 15,      // geometry certifying alone needs more than this
  STRUCT_SOLO: 6750          // 1.5 x STRUCT_STRONG — single-channel certification
};
var WEIGHTS = { local: 35, shape: 25, topology: 15, runs: 10, dct: 10, palette: 5, silhouette: 10 };
/* ---- fixed-point tables ------------------------------------------- */
var Q = 14, QONE = 1 << Q;          /* DCT cosines            */
var R = 10, RONE = 1 << R;          /* ray direction cosines  */

/* DCT basis, frozen.  Math.round(Math.cos(...)) reaches the wire through
   every coefficient; two engines must not be able to differ by one here. */
var DCT4 = Int32Array.of(
  16384, 16384, 16384, 16384, 15137, 6270, -6270, -15137, 11585, -11585, -11585, 11585, 6270,
  -15137, 15137, -6270);
var DCT8 = Int32Array.of(
  16384, 16384, 16384, 16384, 16384, 16384, 16384, 16384, 16069, 13623, 9102, 3196, -3196,
  -9102, -13623, -16069, 15137, 6270, -6270, -15137, -15137, -6270, 6270, 15137, 13623, -3196,
  -16069, -9102, 9102, 16069, 3196, -13623, 11585, -11585, -11585, 11585, 11585, -11585,
  -11585, 11585, 9102, -16069, 3196, 13623, -13623, -3196, 16069, -9102, 6270, -15137, 15137,
  -6270, -6270, 15137, -15137, 6270, 3196, -9102, 13623, -16069, 16069, -13623, 9102, -3196);
var DCT16 = Int32Array.of(
  16384, 16384, 16384, 16384, 16384, 16384, 16384, 16384, 16384, 16384, 16384, 16384, 16384,
  16384, 16384, 16384, 16305, 15679, 14449, 12665, 10394, 7723, 4756, 1606, -1606, -4756,
  -7723, -10394, -12665, -14449, -15679, -16305, 16069, 13623, 9102, 3196, -3196, -9102,
  -13623, -16069, -16069, -13623, -9102, -3196, 3196, 9102, 13623, 16069, 15679, 10394, 1606,
  -7723, -14449, -16305, -12665, -4756, 4756, 12665, 16305, 14449, 7723, -1606, -10394, -15679,
  15137, 6270, -6270, -15137, -15137, -6270, 6270, 15137, 15137, 6270, -6270, -15137, -15137,
  -6270, 6270, 15137, 14449, 1606, -12665, -15679, -4756, 10394, 16305, 7723, -7723, -16305,
  -10394, 4756, 15679, 12665, -1606, -14449, 13623, -3196, -16069, -9102, 9102, 16069, 3196,
  -13623, -13623, 3196, 16069, 9102, -9102, -16069, -3196, 13623, 12665, -7723, -15679, 1606,
  16305, 4756, -14449, -10394, 10394, 14449, -4756, -16305, -1606, 15679, 7723, -12665, 11585,
  -11585, -11585, 11585, 11585, -11585, -11585, 11585, 11585, -11585, -11585, 11585, 11585,
  -11585, -11585, 11585, 10394, -14449, -4756, 16305, -1606, -15679, 7723, 12665, -12665,
  -7723, 15679, 1606, -16305, 4756, 14449, -10394, 9102, -16069, 3196, 13623, -13623, -3196,
  16069, -9102, -9102, 16069, -3196, -13623, 13623, 3196, -16069, 9102, 7723, -16305, 10394,
  4756, -15679, 12665, 1606, -14449, 14449, -1606, -12665, 15679, -4756, -10394, 16305, -7723,
  6270, -15137, 15137, -6270, -6270, 15137, -15137, 6270, 6270, -15137, 15137, -6270, -6270,
  15137, -15137, 6270, 4756, -12665, 16305, -14449, 7723, 1606, -10394, 15679, -15679, 10394,
  -1606, -7723, 14449, -16305, 12665, -4756, 3196, -9102, 13623, -16069, 16069, -13623, 9102,
  -3196, -3196, 9102, -13623, 16069, -16069, 13623, -9102, 3196, 1606, -4756, 7723, -10394,
  12665, -14449, 15679, -16305, 16305, -15679, 14449, -12665, 10394, -7723, 4756, -1606);
function cosTable(N) { return N === 16 ? DCT16 : N === 8 ? DCT8 : DCT4; }

/* 32 ray directions, Q10 */
/* Frozen alongside COS64 — Math.round is not symmetric and this table
   reaches the wire through every radial signature. */
var RAYS = { c: Int32Array.of(
  1024, 1004, 946, 851, 724, 569, 392, 200, 0, -200, -392, -569, -724, -851, -946, -1004,
  -1024, -1004, -946, -851, -724, -569, -392, -200, 0, 200, 392, 569, 724, 851, 946, 1004),
             s: Int32Array.of(
  0, 200, 392, 569, 724, 851, 946, 1004, 1024, 1004, 946, 851, 724, 569, 392, 200, 0, -200,
  -392, -569, -724, -851, -946, -1004, -1024, -1004, -946, -851, -724, -569, -392, -200) };

var POP = (function () { var t = new Uint8Array(65536);
  for (var i = 1; i < 65536; i++) t[i] = t[i >> 1] + (i & 1); return t; })();
function popcount32(x) { return POP[x & 0xffff] + POP[(x >>> 16) & 0xffff]; }

/* ---- small deterministic helpers ---------------------------------- */
function clamp(v, lo, hi) { return v < lo ? lo : (v > hi ? hi : v); }
function idiv(a, b) { return (a / b) | 0; }
/* luminance, integer BT.601 */
function luma(r, g, b) { return (77 * r + 150 * g + 29 * b + 128) >> 8; }
/* ------------------------------------------------------------------ *
 * 1. Ingestion
 * ------------------------------------------------------------------ */

/* Accept the four shapes a browser actually hands us, and REFUSE
   anything else loudly.  A coercion that cannot succeed must never
   fall back to a value that happens to type-check. */
function readImage(a, b, c) {
  var px, w, h;
  if (a && a.data && a.width && a.height) {                 // ImageData
    px = a.data; w = a.width | 0; h = a.height | 0;
  } else if (a && a.px && a.w && a.h) {                     // {px,w,h}
    px = a.px; w = a.w | 0; h = a.h | 0;
  } else if (a && a.pixels && a.width && a.height) {        // {pixels,width,height}
    px = a.pixels; w = a.width | 0; h = a.height | 0;
  } else if (a && typeof b === 'number' && typeof c === 'number') {
    px = a; w = b | 0; h = c | 0;                           // (bytes, w, h)
  } else {
    throw new TypeError('paph: expected ImageData, {px,w,h}, {pixels,width,height} or (bytes,w,h)');
  }
  if (!(w > 0 && h > 0)) throw new RangeError('paph: bad dimensions ' + w + 'x' + h);
  var view;
  if (px instanceof Uint8ClampedArray || px instanceof Uint8Array) {
    view = new Uint8Array(px.buffer, px.byteOffset, px.byteLength);  // honour a window into a bigger buffer
  } else if (px instanceof ArrayBuffer) {
    view = new Uint8Array(px);
  } else if (Array.isArray(px)) {
    view = Uint8Array.from(px);
  } else {
    throw new TypeError('paph: pixel data must be a byte array or ArrayBuffer');
  }
  if (view.length < w * h * 4)
    throw new RangeError('paph: need ' + (w * h * 4) + ' bytes for ' + w + 'x' + h + ', got ' + view.length);
  return { px: view, w: w, h: h };
}

/* Largest k such that the image is an exact k-times nearest-neighbour
   upscale.  A 4x export then hashes to the same wire as the original,
   with the factor recorded in the header instead of smeared through
   every descriptor. */
function upscaleFactor(px, w, h) {
  var g = 0, x, y;
  function gcd(a, b) { while (b) { var t = a % b; a = b; b = t; } return a; }
  g = gcd(w, h);
  for (var k = g; k >= 2; k--) {
    if (w % k || h % k) continue;
    var ok = true;
    for (y = 0; y < h && ok; y++) {
      var by = (y - y % k) * w;
      for (x = 0; x < w; x++) {
        var s = (by + x - x % k) << 2, d = (y * w + x) << 2;
        if (px[s] !== px[d] || px[s + 1] !== px[d + 1] ||
            px[s + 2] !== px[d + 2] || px[s + 3] !== px[d + 3]) { ok = false; break; }
      }
    }
    if (ok) return k;
  }
  return 1;
}

function shrink(px, w, h, k) {
  var nw = idiv(w, k), nh = idiv(h, k), out = new Uint8Array(nw * nh * 4);
  for (var y = 0; y < nh; y++)
    for (var x = 0; x < nw; x++) {
      var s = ((y * k) * w + x * k) << 2, d = (y * nw + x) << 2;
      out[d] = px[s]; out[d + 1] = px[s + 1]; out[d + 2] = px[s + 2]; out[d + 3] = px[s + 3];
    }
  return { px: out, w: nw, h: nh };
}

/* ---- matte fold ---------------------------------------------------
 * A flat export background must become transparency BEFORE the palette
 * is built: a masked matte still occupies a palette slot, and every
 * structural rank downstream is a position in that palette.  Detection
 * is by the SHAPE of a border flood fill, with the gates measured on
 * real art: a matte reads perimeter/area 0.064, line art 0.148, a
 * cropped scene 0.153, busy scenes 0.34-0.69.
 * ------------------------------------------------------------------ */
/* Fold an exported backdrop back to transparency.
 *
 * Two things about real exports that the synthetic fixture hid:
 *
 * The backdrop is DITHERED.  A tolerance-2 flood cannot cross it — real
 * works here carry 23 to 43 distinct colours along the top row alone,
 * so an exact-match flood reads a flat grey backdrop as forty separate
 * regions and folds none of them.
 *
 * The backdrop is DISCONNECTED.  Braids, limbs and props reach the
 * frame and cut the background into pieces, so a flood seeded at the
 * top-left corner reaches one piece and stops: measured 16% of the
 * canvas on a work whose backdrop is plainly most of it.
 *
 * So the seed is the whole border ring, not a corner, and the colour is
 * the ring's modal colour rather than whatever happens to sit at pixel
 * zero.  A matte is the region connected to the FRAME; that is the
 * definition, and the corner was only ever a cheap proxy for it. */
function foldMatte(px, w, h, tol) {
  var n = w * h, i, opaque = 0;
  for (i = 0; i < n; i++) if (px[(i << 2) + 3] > 8) opaque++;
  if (opaque < n * 0.98) return { changed: false };   // already has real alpha
  if (w < 8 || h < 8) return { changed: false };

  /* modal colour of the border ring, quantised so dither collapses */
  var tally = new Map(), ring = [], x, y;
  function edge(p) { ring.push(p);
    var o = p << 2, k = ((px[o] >> 4) << 8) | ((px[o + 1] >> 4) << 4) | (px[o + 2] >> 4);
    tally.set(k, (tally.get(k) || 0) + 1); }
  for (x = 0; x < w; x++) { edge(x); edge((h - 1) * w + x); }
  for (y = 1; y < h - 1; y++) { edge(y * w); edge(y * w + w - 1); }

  var bestK = -1, bestN = 0;
  tally.forEach(function (v, k) { if (v > bestN || (v === bestN && k < bestK)) { bestN = v; bestK = k; } });
  /* If no single colour owns half the frame, there is no matte here —
     there is a picture that happens to reach its own edges. */
  if (bestN * 2 < ring.length) return { changed: false };

  var r0 = 0, g0 = 0, b0 = 0, cnt = 0;
  for (i = 0; i < ring.length; i++) {
    var o2 = ring[i] << 2;
    var k2 = ((px[o2] >> 4) << 8) | ((px[o2 + 1] >> 4) << 4) | (px[o2 + 2] >> 4);
    if (k2 === bestK) { r0 += px[o2]; g0 += px[o2 + 1]; b0 += px[o2 + 2]; cnt++; }
  }
  r0 = idiv(r0, cnt); g0 = idiv(g0, cnt); b0 = idiv(b0, cnt);

  var t = tol === undefined ? 24 : tol | 0;
  function near(p) { var o = p << 2;
    return Math.abs(px[o] - r0) <= t && Math.abs(px[o + 1] - g0) <= t && Math.abs(px[o + 2] - b0) <= t; }

  var seen = new Uint8Array(n), stack = new Int32Array(n), sp = 0, area = 0;
  for (i = 0; i < ring.length; i++)
    if (!seen[ring[i]] && near(ring[i])) { seen[ring[i]] = 1; stack[sp++] = ring[i]; }
  if (!sp) return { changed: false };
  while (sp) {
    var p = stack[--sp]; area++;
    var px_ = p % w, py = (p / w) | 0;
    var nb = [px_ > 0 ? p - 1 : -1, px_ < w - 1 ? p + 1 : -1, py > 0 ? p - w : -1, py < h - 1 ? p + w : -1];
    for (var j = 0; j < 4; j++) {
      var q = nb[j];
      if (q < 0 || seen[q] || !near(q)) continue;
      seen[q] = 1; stack[sp++] = q;
    }
  }
  /* A fold that takes almost everything has eaten the subject. */
  if (area < n * 0.02 || area > n * 0.90) return { changed: false };

  var out = new Uint8Array(px.length); out.set(px);
  for (i = 0; i < n; i++) if (seen[i]) out[(i << 2) + 3] = 0;
  return { changed: true, px: out, area: area };
}
/* ------------------------------------------------------------------ *
 * 2. Palette + quantile bands
 * ------------------------------------------------------------------ */

/* RGBA5551 quantisation, then exact counting.  Pixel art is already
   indexed; this only collapses export noise. */
function indexImage(px, w, h) {
  var n = w * h, key = new Int32Array(n), map = new Map(), i;
  for (i = 0; i < n; i++) {
    var o = i << 2, a = px[o + 3];
    var k = a < 128 ? -1
          : (((px[o] >> 3) << 11) | ((px[o + 1] >> 3) << 6) | ((px[o + 2] >> 3) << 1) | 1);
    key[i] = k;
    if (k < 0) continue;
    var e = map.get(k);
    if (e) { e.n++; e.r += px[o]; e.g += px[o + 1]; e.b += px[o + 2]; }
    else map.set(k, { k: k, n: 1, r: px[o], g: px[o + 1], b: px[o + 2] });
  }
  var pal = Array.from(map.values());
  /* deterministic order: population desc, then key asc */
  pal.sort(function (a, b) { return b.n - a.n || a.k - b.k; });
  if (pal.length > 255) pal = pal.slice(0, 255);
  for (i = 0; i < pal.length; i++) {
    var p = pal[i];
    p.r = idiv(p.r, p.n); p.g = idiv(p.g, p.n); p.b = idiv(p.b, p.n);
    p.lum = luma(p.r, p.g, p.b);
    p.slot = i;
  }
  var byKey = new Map(); for (i = 0; i < pal.length; i++) byKey.set(pal[i].k, i);
  /* nearest surviving entry for anything past the cap */
  var idx = new Int32Array(n);
  var spill = new Map();
  for (i = 0; i < n; i++) {
    var kk = key[i];
    if (kk < 0) { idx[i] = -1; continue; }
    var v = byKey.get(kk);
    if (v === undefined) {
      v = spill.get(kk);
      if (v === undefined) {
        var rr = (kk >> 11) & 31, gg = (kk >> 6) & 31, bb = (kk >> 1) & 31, best = 0, bd = 1 << 30;
        for (var j = 0; j < pal.length; j++) {
          var dr = (pal[j].r >> 3) - rr, dg = (pal[j].g >> 3) - gg, db = (pal[j].b >> 3) - bb;
          var d = dr * dr + dg * dg + db * db;
          if (d < bd) { bd = d; best = j; }
        }
        v = best; spill.set(kk, v);
      }
      pal[v].n++;
    }
    idx[i] = v;
  }

  /* luminance order + quantile band.
     quantile = share of opaque pixel mass STRICTLY darker than this
     entry, 0..255.  Invariant to any monotone tone curve AND to the
     palette being rebuilt — which palette RANK is not, and that is the
     difference that decides whether a recoloured copy is found. */
  var order = pal.map(function (p) { return p; }).sort(function (a, b) {
    return a.lum - b.lum || a.k - b.k;
  });
  var opaque = 0; for (i = 0; i < pal.length; i++) opaque += pal[i].n;
  var acc = 0;
  for (i = 0; i < order.length; i++) {
    order[i].lumOrder = i;
    /* MIDPOINT of this entry's own mass, not the mass strictly below it.
       Only the midpoint satisfies q(inverted) === 255 - q exactly, which is
       what the complement fold in canonical64 and the reflection hypothesis in
       the palette channel both depend on. */
    order[i].quantile = opaque
      ? clamp(idiv((2 * acc + order[i].n) * 255 + opaque, 2 * opaque), 0, 255) : 0;
    order[i].band = order[i].quantile >> 5;                 // 8 quantile bands
    acc += order[i].n;
  }
  return { idx: idx, pal: pal, w: w, h: h, opaque: opaque };
}

/* ------------------------------------------------------------------ *
 * 3. Area-majority aggregation  (spec §3.4)
 *    mode of the covered source pixels; ties -> highest luminance rank.
 *    Hard pixel boundaries survive; bilinear would invent colours that
 *    are not in the palette.
 * ------------------------------------------------------------------ */
function areaMajority(idx, w, h, nw, nh, pal) {
  var out = new Int32Array(nw * nh);
  var tally = new Int32Array(pal.length + 1);       // slot 0 = transparent
  for (var j = 0; j < nh; j++) {
    var y0 = idiv(j * h, nh), y1 = idiv((j + 1) * h, nh); if (y1 <= y0) y1 = y0 + 1;
    for (var i = 0; i < nw; i++) {
      var x0 = idiv(i * w, nw), x1 = idiv((i + 1) * w, nw); if (x1 <= x0) x1 = x0 + 1;
      tally.fill(0);
      for (var y = y0; y < y1 && y < h; y++)
        for (var x = x0; x < x1 && x < w; x++) tally[idx[y * w + x] + 1]++;
      var best = -1, bn = 0, brank = -1;
      for (var v = 0; v < tally.length; v++) {
        if (!tally[v]) continue;
        var rank = v === 0 ? -1 : pal[v - 1].lumOrder;
        if (tally[v] > bn || (tally[v] === bn && rank > brank)) { bn = tally[v]; best = v - 1; brank = rank; }
      }
      out[j * nw + i] = best;
    }
  }
  return out;
}

/* band map (-1 keeps meaning transparent) */
function bandMap(idx, pal) {
  var out = new Int8Array(idx.length);
  for (var i = 0; i < idx.length; i++) out[i] = idx[i] < 0 ? -1 : pal[idx[i]].band;
  return out;
}
/* ------------------------------------------------------------------ *
 * 4. Hierarchical DCT   (spec §3.1, 256 B)
 *
 *  Level 0  16x16 thumbnail  -> 256 coefficients @ 2 bits = 512 b = 64 B
 *  Level 1  4 x 8x8 quadrant ->  64 coefficients @ 2 bits = 128 b = 16 B each
 *  Level 2  16 x 4x4 tile    ->  16 coefficients @ 4 bits =  64 b =  8 B each
 *
 *  All three levels read the SAME 16x16 thumbnail: the quadrants are its
 *  8x8 quarters and the tiles its 4x4 sixteenths, so the pyramid costs
 *  one aggregation pass, and a quadrant descriptor is exactly what you
 *  would get by hashing that quarter of the canvas on its own.
 *
 *  Coefficients are quantised against the PERCENTILES of their own
 *  level, not against fixed magnitudes, which makes the descriptor
 *  invariant to global contrast scaling for free.  Buckets are Gray
 *  coded so neighbouring buckets differ in one bit and Hamming distance
 *  stays meaningful.
 *
 *  DC is dropped by default.  Kept, its sign dominates the code: a
 *  sibling index built with DC had to compare 98.9% of the corpus
 *  because every bucket was "is this picture bright".  Dropping it also
 *  buys tone-shift invariance.  `keepDC` puts it back.
 * ------------------------------------------------------------------ */

function dct2(src, N) {
  var t = cosTable(N), tmp = new Int32Array(N * N), out = new Int32Array(N * N), u, i, j, s;
  for (j = 0; j < N; j++)
    for (u = 0; u < N; u++) {
      s = 0; for (i = 0; i < N; i++) s += src[j * N + i] * t[u * N + i];
      tmp[j * N + u] = (s + (QONE >> 1)) >> Q;
    }
  for (u = 0; u < N; u++)
    for (var v = 0; v < N; v++) {
      s = 0; for (j = 0; j < N; j++) s += tmp[j * N + u] * t[v * N + j];
      out[v * N + u] = (s + (QONE >> 1)) >> Q;
    }
  return out;
}

var GRAY = [
  [0], [0, 1], [0, 1, 3, 2], [0, 1, 3, 2, 6, 7, 5, 4]
];

/* Each coefficient is stored as ONE SIGN BIT plus a Gray-coded
   magnitude bucket, taken against the order statistics of |coef| in
   its own block.
 *
 * The obvious encoding — bucket the signed value — costs the dihedral
 * group.  Under a horizontal flip a 2-D DCT negates every coefficient
 * with odd horizontal frequency, and you cannot negate a signed
 * percentile bucket after the fact.  Split off the sign and all eight
 * symmetries become bit operations on the stored code: transpose swaps
 * the frequency indices, a flip toggles sign bits on odd rows or
 * columns.  A mirrored or quarter-turned copy is then found by the
 * global channel at compare time, for zero extra bytes.
 *
 * Bucketing |coef| against its own block also makes the code invariant
 * to a global contrast scale, which bucketing raw magnitudes is not. */
function quantiseBlock(co, bits, keepDC, bytes, off) {
  var n = co.length, i, mbits = bits - 1;
  var mags = [];
  for (i = keepDC ? 0 : 1; i < n; i++) mags.push(co[i] < 0 ? -co[i] : co[i]);
  mags.sort(function (a, b) { return a - b; });
  var levels = 1 << mbits, cuts = new Int32Array(levels > 1 ? levels - 1 : 1);
  for (i = 1; i < levels; i++) {
    var p = idiv(i * mags.length, levels);
    cuts[i - 1] = mags.length ? mags[clamp(p, 0, mags.length - 1)] : 0;
  }
  var gray = GRAY[mbits];
  var bit = 0;
  for (i = 0; i < n; i++) {
    var sgn = 0, q = 0;
    if (!(i === 0 && !keepDC)) {
      var v = co[i]; sgn = v < 0 ? 1 : 0;
      var m = v < 0 ? -v : v;
      while (q < levels - 1 && m >= cuts[q]) q++;
    }
    var code = (sgn << mbits) | gray[q];
    for (var k = bits - 1; k >= 0; k--) {
      if ((code >> k) & 1) bytes[off + (bit >> 3)] |= 0x80 >> (bit & 7);
      bit++;
    }
  }
  return (n * bits) >> 3;
}

function hierarchicalDCT(thumb16, keepDC) {
  var out = new Uint8Array(256), i, x, y;
  quantiseBlock(dct2(thumb16, 16), 2, keepDC, out, 0);              // 64 B
  var off = 64;
  for (var qy = 0; qy < 2; qy++)
    for (var qx = 0; qx < 2; qx++) {
      var blk = new Int32Array(64);
      for (y = 0; y < 8; y++) for (x = 0; x < 8; x++)
        blk[y * 8 + x] = thumb16[(qy * 8 + y) * 16 + qx * 8 + x];
      off += quantiseBlock(dct2(blk, 8), 2, keepDC, out, off);      // 16 B each
    }
  for (var ty = 0; ty < 4; ty++)
    for (var tx = 0; tx < 4; tx++) {
      var t4 = new Int32Array(16);
      for (y = 0; y < 4; y++) for (x = 0; x < 4; x++)
        t4[y * 4 + x] = thumb16[(ty * 4 + y) * 16 + tx * 4 + x];
      off += quantiseBlock(dct2(t4, 4), 4, keepDC, out, off);       // 8 B each
    }
  return out;
}

/* 16x16 luminance thumbnail.  Cells with no opaque pixel take the
   median opaque luminance rather than 0: filling holes with black
   manufactures an edge that the same sprite composited on a host would
   not have, and the two maps of one drawing then share almost nothing. */
function thumbnail16(im) {
  var m = areaMajority(im.idx, im.w, im.h, 16, 16, im.pal);
  var lums = [];
  for (var i = 0; i < im.pal.length; i++) for (var k = 0; k < 1; k++) lums.push(im.pal[i].lum);
  lums.sort(function (a, b) { return a - b; });
  var fill = lums.length ? lums[lums.length >> 1] : 128;
  var out = new Int32Array(256);
  for (i = 0; i < 256; i++) out[i] = m[i] < 0 ? fill : im.pal[m[i]].lum;
  return out;
}

/* ------------------------------------------------------------------ *
 * Brightness record   (SPEC-003 §6.3, 8 B)
 *
 * The DC coefficient is now ALWAYS dropped from the DCT code, so the index
 * bucket can never degenerate into "is this picture bright" — which is what
 * made a sibling index compare 98.9% of a corpus.  The information it carried
 * moves here, where a compare-time channel can read it explicitly and report
 * "structurally identical, tonally shifted" as its own finding.
 * ------------------------------------------------------------------ */
function brightnessRecord(im, thumb) {
  var out = new Uint8Array(8), i, s = 0;
  for (i = 0; i < 256; i++) s += thumb[i];
  out[0] = clamp(idiv(s, 256), 0, 255);                 // mean thumbnail luminance
  var srt = Array.prototype.slice.call(thumb).sort(function (a, b) { return a - b; });
  out[1] = srt[12]; out[2] = srt[64]; out[3] = srt[128]; out[4] = srt[192]; out[5] = srt[243];
  /* spread and a sign digest of the L0 DC quadrant pattern */
  out[6] = clamp(srt[243] - srt[12], 0, 255);
  var q = 0;
  for (var qy = 0; qy < 2; qy++) for (var qx = 0; qx < 2; qx++) {
    var t = 0;
    for (var y = 0; y < 8; y++) for (var x = 0; x < 8; x++) t += thumb[(qy * 8 + y) * 16 + qx * 8 + x];
    q = (q << 2) | (idiv(t, 64) > out[0] ? 2 : 0) | (idiv(t, 64) > out[2] ? 1 : 0);
  }
  out[7] = q & 255;
  return out;
}

/* ------------------------------------------------------------------ *
 * Identity palette   (SPEC-003 §6.3, 96 B = 24 x 4)
 *    Absolute RGB is discarded on purpose: a recolour must not move it.
 * ------------------------------------------------------------------ */
var PAL_N = 24;
function identityPalette(im) {
  var out = new Uint8Array(PAL_N * 4), n = Math.min(PAL_N, im.pal.length);
  var top = im.pal.slice(0, n);
  var maxN = top.length ? top[0].n : 1;
  for (var i = 0; i < n; i++) {
    var p = top[i], o = i << 2;
    out[o] = i;
    out[o + 1] = clamp(idiv(p.n * 255 + (maxN >> 1), maxN), 0, 255);
    out[o + 2] = im.pal.length > 1 ? clamp(idiv(p.lumOrder * 255, im.pal.length - 1), 0, 255) : 0;
    out[o + 3] = p.quantile;
  }
  return { bytes: out, count: n };
}

/* ------------------------------------------------------------------ *
 * Sparse RAG   (SPEC-003 §6.3, 288 B = 48 x 6)
 *
 * BOTH endpoint encodings are stored.  Quantile survives a rebuilt palette;
 * rank does not — and that is exactly why rank agreement is STRONGER evidence
 * when it occurs: it means the palette was not rebuilt.  v2 forced the choice
 * at hash time; v3 scores on one and reports the other.
 * ------------------------------------------------------------------ */
var RAG_N = 48;
function sparseRAG(im) {
  var w = im.w, h = im.h, idx = im.idx, pal = im.pal;
  var acc = new Map(), total = 0;
  var nMax = Math.max(1, pal.length - 1);
  function rankOf(v) { return clamp(idiv(pal[v].lumOrder * 255, nMax), 0, 255); }
  function add(a, b) {
    if (a < 0 || b < 0 || a === b) return;
    var qa = pal[a].quantile, qb = pal[b].quantile;
    var ra = rankOf(a), rb = rankOf(b);
    var qlo = qa < qb ? qa : qb, qhi = qa < qb ? qb : qa;
    var rlo = ra < rb ? ra : rb, rhi = ra < rb ? rb : ra;
    if (qlo === qhi && rlo === rhi) return;
    var k = (qlo << 24) | (qhi << 16) | (rlo << 8) | rhi;
    acc.set(k, (acc.get(k) || 0) + 1); total++;
  }
  for (var y = 0; y < h; y++)
    for (var x = 0; x < w; x++) {
      var p = idx[y * w + x];
      if (x + 1 < w) add(p, idx[y * w + x + 1]);
      if (y + 1 < h) add(p, idx[(y + 1) * w + x]);
    }
  var list = Array.from(acc.entries()).map(function (e) { return { k: e[0], n: e[1] }; });
  list.sort(function (a, b) { return b.n - a.n || a.k - b.k; });
  list = list.slice(0, RAG_N);
  var out = new Uint8Array(RAG_N * 6);
  for (var i = 0; i < list.length; i++) {
    var o = i * 6, e = list[i], k = e.k;
    out[o]     = (k >>> 24) & 255;   // quantile lo
    out[o + 1] = (k >>> 16) & 255;   // quantile hi
    out[o + 2] = (k >>> 8) & 255;    // rank lo
    out[o + 3] = k & 255;            // rank hi
    var nrm = total ? clamp(idiv(e.n * 65535, total), 0, 65535) : 0;
    out[o + 4] = nrm & 255; out[o + 5] = (nrm >> 8) & 255;
  }
  return { bytes: out, count: list.length };
}

/* ------------------------------------------------------------------ *
 * Silhouette signature   (SPEC-003 §6.4, 96 B)
 *
 * Its own channel, on purpose.  Two sprites cut from the same sheet share a
 * silhouette exactly; the same sprite composited into a scene has no
 * silhouette at all.  So the evidence is very strong when measurable and
 * carries ZERO information when not — which is the shape of a channel that
 * must be able to abstain, and the reason letting it leak into the interior
 * descriptors (as the v2 L2 half did, by flattening alpha to black) is wrong.
 * ------------------------------------------------------------------ */
function silhouetteSignature(im) {
  var w = im.w, h = im.h, n = w * h, i, x, y;
  var out = new Uint8Array(96);
  var opaque = 0;
  for (i = 0; i < n; i++) if (im.idx[i] >= 0) opaque++;
  /* no transparency, or almost none -> no silhouette to speak of */
  if (opaque === 0 || opaque * 100 > n * 98 || opaque * 100 < n * 2)
    return { bytes: out, measurable: false };

  /* largest opaque 4-connected component */
  var id = new Int32Array(n).fill(-1), stack = new Int32Array(n);
  var best = -1, bestArea = 0, comps = [];
  for (var s0 = 0; s0 < n; s0++) {
    if (id[s0] >= 0 || im.idx[s0] < 0) continue;
    var cid = comps.length, sp = 0, area = 0, sx = 0, sy = 0;
    var minx = w, maxx = -1, miny = h, maxy = -1;
    stack[sp++] = s0; id[s0] = cid;
    while (sp) {
      var p = stack[--sp], px = p % w, py = (p / w) | 0;
      area++; sx += px; sy += py;
      if (px < minx) minx = px; if (px > maxx) maxx = px;
      if (py < miny) miny = py; if (py > maxy) maxy = py;
      if (px > 0     && id[p - 1] < 0 && im.idx[p - 1] >= 0) { id[p - 1] = cid; stack[sp++] = p - 1; }
      if (px < w - 1 && id[p + 1] < 0 && im.idx[p + 1] >= 0) { id[p + 1] = cid; stack[sp++] = p + 1; }
      if (py > 0     && id[p - w] < 0 && im.idx[p - w] >= 0) { id[p - w] = cid; stack[sp++] = p - w; }
      if (py < h - 1 && id[p + w] < 0 && im.idx[p + w] >= 0) { id[p + w] = cid; stack[sp++] = p + w; }
    }
    comps.push({ id: cid, area: area, cx: idiv(sx, area), cy: idiv(sy, area),
                 minx: minx, maxx: maxx, miny: miny, maxy: maxy });
    if (area > bestArea) { bestArea = area; best = cid; }
  }
  if (best < 0) return { bytes: out, measurable: false };
  var c = comps[best];
  var bw = c.maxx - c.minx + 1, bh = c.maxy - c.miny + 1;

  /* centroid inside the component, or the rays start outside it */
  var cx = c.cx, cy = c.cy;
  if (id[cy * w + cx] !== c.id) {
    var bd = 1 << 30;
    for (y = c.miny; y <= c.maxy; y++) for (x = c.minx; x <= c.maxx; x++) {
      if (id[y * w + x] !== c.id) continue;
      var d = (x - c.cx) * (x - c.cx) + (y - c.cy) * (y - c.cy);
      if (d < bd) { bd = d; cx = x; cy = y; }
    }
  }
  /* 32 rays, integer DDA, first crossing */
  var rad = new Int32Array(32), rmax = 1;
  for (var k = 0; k < 32; k++) {
    var t2 = 0, lim = bw + bh;
    for (var step = 1; step <= lim; step++) {
      var rx = cx + ((RAYS.c[k] * step + (RONE >> 1)) >> R);
      var ry = cy + ((RAYS.s[k] * step + (RONE >> 1)) >> R);
      if (rx < 0 || ry < 0 || rx >= w || ry >= h) break;
      if (id[ry * w + rx] !== c.id) break;
      t2 = step;
    }
    rad[k] = t2; if (t2 > rmax) rmax = t2;
  }
  for (k = 0; k < 32; k++) out[k] = clamp(idiv(255 * rad[k], rmax), 0, 255);

  /* scale-free moments over the component, in Q8 of the bbox diagonal */
  var m20 = 0, m02 = 0, m11 = 0, cnt = 0;
  for (y = c.miny; y <= c.maxy; y++) for (x = c.minx; x <= c.maxx; x++) {
    if (id[y * w + x] !== c.id) continue;
    var dx = x - cx, dy = y - cy;
    m20 += dx * dx; m02 += dy * dy; m11 += dx * dy; cnt++;
  }
  var norm = Math.max(1, cnt * (bw * bw + bh * bh));
  out[32] = clamp(idiv(m20 * 1020, norm), 0, 255);
  out[33] = clamp(idiv(m02 * 1020, norm), 0, 255);
  out[34] = clamp(idiv((m11 < 0 ? -m11 : m11) * 1020, norm), 0, 255);
  out[35] = m11 < 0 ? 1 : 0;
  var asp = clamp(idiv(bw * 256, Math.max(1, bh)), 0, 65535);
  out[36] = asp & 255; out[37] = (asp >> 8) & 255;
  var fill = clamp(idiv(c.area * 255, Math.max(1, bw * bh)), 0, 255);
  out[38] = fill;
  out[39] = clamp(comps.length, 0, 255);

  /* boundary-run digest: transitions per row and per column, 8 bins each */
  var rowT = new Int32Array(8), colT = new Int32Array(8);
  for (y = 0; y < h; y++) { var t = 0, prev = 0;
    for (x = 0; x < w; x++) { var v = im.idx[y * w + x] >= 0 ? 1 : 0; if (v !== prev) t++; prev = v; }
    rowT[Math.min(7, t)]++; }
  for (x = 0; x < w; x++) { var t2c = 0, pv = 0;
    for (y = 0; y < h; y++) { var v2 = im.idx[y * w + x] >= 0 ? 1 : 0; if (v2 !== pv) t2c++; pv = v2; } 
    colT[Math.min(7, t2c)]++; }
  for (i = 0; i < 8; i++) { out[40 + i] = clamp(idiv(rowT[i] * 255, h), 0, 255);
                            out[48 + i] = clamp(idiv(colT[i] * 255, w), 0, 255); }

  /* 8x8 occupancy code of the component's bbox, D4-canonical (64 bits) */
  var cell = new Int32Array(64);
  for (var gy = 0; gy < 8; gy++) for (var gx = 0; gx < 8; gx++) {
    var x0 = c.minx + idiv(gx * bw, 8), x1 = c.minx + idiv((gx + 1) * bw, 8);
    var y0 = c.miny + idiv(gy * bh, 8), y1 = c.miny + idiv((gy + 1) * bh, 8);
    if (x1 <= x0) x1 = x0 + 1; if (y1 <= y0) y1 = y0 + 1;
    var on = 0, tot = 0;
    for (y = y0; y < y1 && y < h; y++) for (x = x0; x < x1 && x < w; x++) {
      tot++; if (id[y * w + x] === c.id) on++;
    }
    cell[gy * 8 + gx] = tot ? idiv(on * 255, tot) : 0;
  }
  var bits = canonical64(cell, false);
  out[56] = bits[0] & 255; out[57] = (bits[0] >>> 8) & 255;
  out[58] = (bits[0] >>> 16) & 255; out[59] = (bits[0] >>> 24) & 255;
  out[60] = bits[1] & 255; out[61] = (bits[1] >>> 8) & 255;
  out[62] = (bits[1] >>> 16) & 255; out[63] = (bits[1] >>> 24) & 255;
  var frac = clamp(idiv(opaque * 65535, n), 0, 65535);
  out[64] = frac & 255; out[65] = (frac >> 8) & 255;
  return { bytes: out, measurable: true };
}

/* ------------------------------------------------------------------ *
 * Absolute colour digest   (SPEC-003 §6.5, 80 B = 16 x 5)
 *
 * REPORTING ONLY.  This section MUST NOT contribute to any score: recolour
 * invariance is load-bearing and dies the moment absolute RGB enters the
 * scoring path.  It exists so a verdict can say whether the palettes are
 * identical, related or rebuilt — an exact-palette match is a materially
 * stronger moderation case, and v2 threw that away at ingest.
 *
 * Conformance: zeroing this section MUST NOT change any verdict.
 * ------------------------------------------------------------------ */
function colourDigest(im) {
  var out = new Uint8Array(80), n = Math.min(16, im.pal.length);
  var maxN = im.pal.length ? im.pal[0].n : 1;
  for (var i = 0; i < n; i++) {
    var p = im.pal[i], o = i * 5;
    out[o] = p.r; out[o + 1] = p.g; out[o + 2] = p.b;
    out[o + 3] = clamp(idiv(p.n * 255 + (maxN >> 1), maxN), 0, 255);
    out[o + 4] = p.quantile;
  }
  return { bytes: out, count: n };
}
/* ------------------------------------------------------------------ *
 * 7. Shape signatures   (spec §3.3)
 *
 *  Connected components run over an 8-band luminance-QUANTILE label
 *  map, modally downsampled: quantile bands survive a tone curve and a
 *  rebuilt palette, and the modal downsample kills dither confetti that
 *  would otherwise shatter every region into noise.
 *
 *  41 B per shape, 5 shapes.  (The spec's table says 36 B while its own
 *  field list adds to 40 and its Rust struct to 41 — the fields win.)
 * ------------------------------------------------------------------ */
var SHAPE_BYTES = 41, SHAPE_N = 8;   /* v2 stored 5; five was a budget
   constraint, not a finding (SPEC-003 §6.3). */

function shapeGrid(im, mode) {
  var long = Math.max(im.w, im.h);
  var cell = Math.max(1, idiv(long + 127, 128));   /* integer ceil */
  var gw = Math.max(4, idiv(im.w + cell - 1, cell)), gh = Math.max(4, idiv(im.h + cell - 1, cell));
  var m = areaMajority(im.idx, im.w, im.h, gw, gh, im.pal);
  var lab = new Int8Array(gw * gh);
  for (var i = 0; i < m.length; i++)
    lab[i] = m[i] < 0 ? -1 : (mode === 'palette' ? (im.pal[m[i]].slot & 7) : im.pal[m[i]].band);
  return { lab: lab, w: gw, h: gh, cell: cell };
}

function components(g) {
  var w = g.w, h = g.h, lab = g.lab, id = new Int32Array(w * h).fill(-1), out = [];
  var stack = new Int32Array(w * h);
  for (var s = 0; s < w * h; s++) {
    if (id[s] >= 0 || lab[s] < 0) continue;
    var cid = out.length, sp = 0, area = 0, sx = 0, sy = 0;
    var minx = w, maxx = -1, miny = h, maxy = -1;
    stack[sp++] = s; id[s] = cid;
    while (sp) {
      var p = stack[--sp], x = p % w, y = (p / w) | 0;
      area++; sx += x; sy += y;
      if (x < minx) minx = x; if (x > maxx) maxx = x;
      if (y < miny) miny = y; if (y > maxy) maxy = y;
      if (x > 0     && id[p - 1] < 0 && lab[p - 1] === lab[p]) { id[p - 1] = cid; stack[sp++] = p - 1; }
      if (x < w - 1 && id[p + 1] < 0 && lab[p + 1] === lab[p]) { id[p + 1] = cid; stack[sp++] = p + 1; }
      if (y > 0     && id[p - w] < 0 && lab[p - w] === lab[p]) { id[p - w] = cid; stack[sp++] = p - w; }
      if (y < h - 1 && id[p + w] < 0 && lab[p + w] === lab[p]) { id[p + w] = cid; stack[sp++] = p + w; }
    }
    out.push({ id: cid, area: area, cx: idiv(sx, area), cy: idiv(sy, area),
               minx: minx, maxx: maxx, miny: miny, maxy: maxy, band: lab[s] });
  }
  return { id: id, list: out };
}

function shapeSignatures(im, mode) {
  var g = shapeGrid(im, mode), cc = components(g), w = g.w, h = g.h;
  var list = cc.list.slice().sort(function (a, b) {
    return b.area - a.area || a.miny - b.miny || a.minx - b.minx;
  }).slice(0, SHAPE_N);

  var out = new Uint8Array(SHAPE_BYTES * SHAPE_N), shapes = [];
  for (var s = 0; s < list.length; s++) {
    var c = list[s], o = s * SHAPE_BYTES;

    /* perimeter: cells with a 4-neighbour outside the component */
    var per = 0, holes = 0, x, y, p;
    for (y = c.miny; y <= c.maxy; y++)
      for (x = c.minx; x <= c.maxx; x++) {
        p = y * w + x; if (cc.id[p] !== c.id) continue;
        if (x === 0 || y === 0 || x === w - 1 || y === h - 1 ||
            cc.id[p - 1] !== c.id || cc.id[p + 1] !== c.id ||
            cc.id[p - w] !== c.id || cc.id[p + w] !== c.id) per++;
      }

    /* holes: components of the complement inside the bbox that never
       touch the bbox border */
    var bw = c.maxx - c.minx + 1, bh = c.maxy - c.miny + 1;
    var seen = new Uint8Array(bw * bh), st = new Int32Array(bw * bh);
    for (y = 0; y < bh; y++) for (x = 0; x < bw; x++) {
      var q = y * bw + x;
      if (seen[q] || cc.id[(y + c.miny) * w + x + c.minx] === c.id) continue;
      var sp2 = 0, touch = false; st[sp2++] = q; seen[q] = 1;
      while (sp2) {
        var r = st[--sp2], rx = r % bw, ry = (r / bw) | 0;
        if (rx === 0 || ry === 0 || rx === bw - 1 || ry === bh - 1) touch = true;
        var nb = [rx > 0 ? r - 1 : -1, rx < bw - 1 ? r + 1 : -1, ry > 0 ? r - bw : -1, ry < bh - 1 ? r + bw : -1];
        for (var j = 0; j < 4; j++) {
          var t = nb[j]; if (t < 0 || seen[t]) continue;
          var tx = t % bw, ty = (t / bw) | 0;
          if (cc.id[(ty + c.miny) * w + tx + c.minx] === c.id) continue;
          seen[t] = 1; st[sp2++] = t;
        }
      }
      if (!touch) holes++;
    }

    /* centroid must sit inside the component or the rays start outside */
    var cx = c.cx, cy = c.cy;
    if (cc.id[cy * w + cx] !== c.id) {
      var bd = 1 << 30;
      for (y = c.miny; y <= c.maxy; y++) for (x = c.minx; x <= c.maxx; x++) {
        if (cc.id[y * w + x] !== c.id) continue;
        var d = (x - c.cx) * (x - c.cx) + (y - c.cy) * (y - c.cy);
        if (d < bd) { bd = d; cx = x; cy = y; }
      }
    }

    /* 32 rays, integer DDA, distance to the FIRST contour crossing */
    var rad = new Int32Array(32), rmax = 1;
    for (var k = 0; k < 32; k++) {
      var t2 = 0, lim = bw + bh;
      for (var step = 1; step <= lim; step++) {
        var px2 = cx + ((RAYS.c[k] * step + (RONE >> 1)) >> R);
        var py2 = cy + ((RAYS.s[k] * step + (RONE >> 1)) >> R);
        if (px2 < 0 || py2 < 0 || px2 >= w || py2 >= h) break;
        if (cc.id[py2 * w + px2] !== c.id) break;
        t2 = step;
      }
      rad[k] = t2; if (t2 > rmax) rmax = t2;
    }

    var aspect = clamp(idiv(bw * 256, Math.max(1, bh)), 0, 65535);
    out[o] = c.area & 255; out[o + 1] = (c.area >> 8) & 255;
    out[o + 2] = (c.area >> 16) & 255; out[o + 3] = (c.area >> 24) & 255;
    out[o + 4] = per & 255; out[o + 5] = (per >> 8) & 255;
    out[o + 6] = aspect & 255; out[o + 7] = (aspect >> 8) & 255;
    out[o + 8] = clamp(holes, 0, 255);
    for (k = 0; k < 32; k++) out[o + 9 + k] = clamp(idiv(255 * rad[k], rmax), 0, 255);

    shapes.push({ area: c.area, per: per, aspect: aspect, holes: holes,
                  cx: cx, cy: cy, gw: w, gh: h, cell: g.cell, rmax: rmax,
                  radial: Array.prototype.slice.call(out.subarray(o + 9, o + 41)) });
  }
  return { bytes: out, count: list.length, shapes: shapes, grid: g, cc: cc };
}
/* ------------------------------------------------------------------ *
 * 8. Run-length geometry   (spec §3.5, 48 B)
 *    Contiguous same-index runs along H, V and the main diagonal,
 *    binned on a log ladder.  Captures stroke texture with no colour
 *    in it at all.
 * ------------------------------------------------------------------ */
var RUN_LADDER = [1, 2, 3, 4, 5, 6, 8, 10, 13, 16, 22, 30, 42, 60, 90, 0x7fffffff];
function runBin(n) { var i = 0; while (RUN_LADDER[i] < n) i++; return i; }

function runLengths(im) {
  var w = im.w, h = im.h, idx = im.idx;
  var H = new Int32Array(16), V = new Int32Array(16), D = new Int32Array(16);
  function walk(get, len, hist) {
    var run = 1, prev = get(0);
    for (var i = 1; i < len; i++) {
      var v = get(i);
      if (v === prev) run++; else { hist[runBin(run)]++; run = 1; prev = v; }
    }
    hist[runBin(run)]++;
  }
  var y, x;
  for (y = 0; y < h; y++) walk(function (i) { return idx[y * w + i]; }, w, H);
  for (x = 0; x < w; x++) walk(function (i) { return idx[i * w + x]; }, h, V);
  for (var d = -(h - 1); d < w; d++) {
    var x0 = d > 0 ? d : 0, y0 = d > 0 ? 0 : -d, n = Math.min(w - x0, h - y0);
    if (n < 2) continue;
    walk((function (x0, y0) { return function (i) { return idx[(y0 + i) * w + x0 + i]; }; })(x0, y0), n, D);
  }
  var out = new Uint8Array(48);
  [H, V, D].forEach(function (hist, s) {
    var tot = 0, i; for (i = 0; i < 16; i++) tot += hist[i];
    for (i = 0; i < 16; i++) out[s * 16 + i] = tot ? clamp(idiv(hist[i] * 255, tot), 0, 255) : 0;
  });
  return out;
}
/* ------------------------------------------------------------------ *
 * 9. Local fingerprints   (spec §3.2, 512 B)
 *
 *  The collage channel, and the one that decides whether a sprite
 *  buried in a busy scene is ever found.  Up to 64 regions, each
 *  reduced to an 8x8 grid of quantile bands, thresholded at the
 *  region's own median into 64 bits, then canonicalised over the eight
 *  symmetries of D4 by taking the lexicographic minimum.  A copy that
 *  was mirrored or turned produces the identical 64 bits.
 *
 *  Regions are chosen by a purely LOCAL saliency, so a crop keeps the
 *  same selections in the part it kept; a global criterion would move
 *  every window the moment the surroundings change.
 * ------------------------------------------------------------------ */
function d4Variants(cell) {                 /* cell: 64 band values, 8x8 */
  var v = [], k, x, y;
  var maps = [
    function (x, y) { return y * 8 + x; },
    function (x, y) { return y * 8 + (7 - x); },
    function (x, y) { return (7 - y) * 8 + x; },
    function (x, y) { return (7 - y) * 8 + (7 - x); },
    function (x, y) { return x * 8 + y; },
    function (x, y) { return x * 8 + (7 - y); },
    function (x, y) { return (7 - x) * 8 + y; },
    function (x, y) { return (7 - x) * 8 + (7 - y); }
  ];
  for (k = 0; k < 8; k++) {
    var g = new Int32Array(64);
    for (y = 0; y < 8; y++) for (x = 0; x < 8; x++) g[y * 8 + x] = cell[maps[k](x, y)];
    v.push(g);
  }
  return v;
}

var _srt = new Int32Array(64);
/* med2 is the SUM of the two central order statistics, so the test is
   2*v > s[31] + s[32].  Splitting at s[32] alone is not the symmetric middle
   of 64 values, and an asymmetric threshold does not survive a complement —
   which is exactly what the inversion fold below needs it to do. */
function bitsOf(g, med2) {
  if (med2 === undefined) {
    _srt.set(g); Array.prototype.sort.call(_srt, function (a, b) { return a - b; });
    med2 = _srt[31] + _srt[32];
  }
  var hi = 0, lo = 0;
  for (var i = 0; i < 32; i++) if (2 * g[i] > med2) hi |= (1 << (31 - i));
  for (i = 32; i < 64; i++) if (2 * g[i] > med2) lo |= (1 << (63 - i));
  return [hi >>> 0, lo >>> 0];
}

/* Canonical over D4 x {identity, complement} — sixteen variants.
 *
 * Complementing the bits is what an INVERTED palette does to this
 * descriptor: the median test simply reverses.  Folding the complement
 * into the canonical form makes an inverted copy produce byte-identical
 * fingerprints, which a min(d, 64-d) distance at compare time cannot
 * do: complementation commutes with D4, so the canonical form of the
 * complement is the complement of the lexicographic MAXIMUM, not of the
 * minimum, and the two do not meet.
 *
 * The cost is real and worth naming: light/dark polarity is discarded,
 * so a work and its negative are indistinguishable to this channel.
 * That is the trade his spec asks for — inversion is on the list of
 * filters a copy must not survive by. */
/* D4 index maps, built once.  v2 rebuilt eight Int32Array(64) per window. */
var D4MAP = (function () {
  var f = [function (x, y) { return y * 8 + x; },       function (x, y) { return y * 8 + (7 - x); },
           function (x, y) { return (7 - y) * 8 + x; }, function (x, y) { return (7 - y) * 8 + (7 - x); },
           function (x, y) { return x * 8 + y; },       function (x, y) { return x * 8 + (7 - y); },
           function (x, y) { return (7 - x) * 8 + y; }, function (x, y) { return (7 - x) * 8 + (7 - y); }];
  var m = [];
  for (var k = 0; k < 8; k++) { var t = new Int32Array(64);
    for (var y = 0; y < 8; y++) for (var x = 0; x < 8; x++) t[y * 8 + x] = f[k](x, y);
    m.push(t); }
  return m;
})();
var _var = new Int32Array(64);
function canonical64(cell, foldInvert) {
  /* The eight variants are PERMUTATIONS of one multiset, so they share one
     median — compute it once instead of sorting 64 values eight times. */
  _srt.set(cell); Array.prototype.sort.call(_srt, function (a, b) { return a - b; });
  var med2 = _srt[31] + _srt[32], best = null, i, k, b;
  for (i = 0; i < 8; i++) {
    var m = D4MAP[i];
    for (k = 0; k < 64; k++) _var[k] = cell[m[k]];
    b = bitsOf(_var, med2);
    if (!best || b[0] < best[0] || (b[0] === best[0] && b[1] < best[1])) best = b;
    if (foldInvert) {
      var c = [(~b[0]) >>> 0, (~b[1]) >>> 0];
      if (c[0] < best[0] || (c[0] === best[0] && c[1] < best[1])) best = c;
    }
  }
  return best;
}

/* Cells carry the full 0..255 luminance QUANTILE, not the 8-band
   reduction.  With 8 levels the median threshold ties on most cells of
   a simple region, half the bag comes out near-constant, and chance
   collisions ran at 62%: the channel measured nothing.  The band map
   stays for saliency, where coarseness is what you want. */
/* summed-area table over a 0/1 map, so any window total is 4 lookups */
function integral(map, w, h) {
  var S = new Int32Array((w + 1) * (h + 1));
  for (var y = 0; y < h; y++) {
    var row = 0;
    for (var x = 0; x < w; x++) {
      row += map[y * w + x];
      S[(y + 1) * (w + 1) + x + 1] = S[y * (w + 1) + x + 1] + row;
    }
  }
  return S;
}
function boxSum(S, w, x, y, n) {
  var W = w + 1;
  return S[(y + n) * W + x + n] - S[y * W + x + n] - S[(y + n) * W + x] + S[y * W + x];
}

function offsets(c2, win, span) {
  var num = c2 + 1 - win, base = num >> 1;
  var list = (num & 1) ? [base, base + 1] : [base];
  var out = [];
  for (var i = 0; i < list.length; i++) {
    var v = clamp(list[i], 0, span - win);
    if (out.indexOf(v) < 0) out.push(v);
  }
  return out;
}

/* 8x8 reduction of a window (median quantile per sub-block, so one
   stray pixel cannot move a cell), then the D4-canonical 64 bits */
var _cellScratch = new Int32Array(4096);
function fingerprintOf(k) {
  var cell = new Int32Array(64), q = k.win >> 3;
  for (var cy = 0; cy < 8; cy++)
    for (var cx = 0; cx < 8; cx++) {
      if (q === 1) {
        var v1 = k.q[(k.y + cy) * k.sw + k.x + cx];
        cell[cy * 8 + cx] = v1 < 0 ? 257 : 2 * (v1 + 1);
        continue;
      }
      /* Median of the sub-block, insertion-sorted into a preallocated scratch.
         This ran once per cell per window and allocated a fresh JS array each
         time — thousands of short-lived arrays per work. */
      var nv = 0;
      for (var by = 0; by < q; by++)
        for (var bx = 0; bx < q; bx++) {
          var vq = k.q[(k.y + cy * q + by) * k.sw + k.x + cx * q + bx];
          var val = vq < 0 ? 257 : 2 * (vq + 1), j = nv;
          while (j > 0 && _cellScratch[j - 1] > val) { _cellScratch[j] = _cellScratch[j - 1]; j--; }
          _cellScratch[j] = val; nv++;
        }
      cell[cy * 8 + cx] = _cellScratch[nv >> 1];
    }
  var bits = canonical64(cell, k.foldInvert);
  /* A window with no tonal structure produces an all-zero code, and an
     all-zero code collides with every featureless patch in every work
     ever hashed — including with its own rotations, so it poisons the
     empirical null as well as the count.  Measured on region-centroid
     anchors: 15 genuine collisions on a pixel-exact paste scored 0.000,
     because the control also counted 15.
     Popcount is invariant under D4, so this filter cannot break the
     dihedral canonicalisation it sits next to. */
  var pc = popcount32(bits[0]) + popcount32(bits[1]);
  if (pc < 6 || pc > 58) return null;
  return { hi: bits[0], lo: bits[1], x: k.x, y: k.y, size: k.win, wi: k.wi, sal: k.sal };
}

/* Scrambler.  Selection must sample the candidate set uniformly, not
   favour numerically small codes: taking the smallest fingerprints
   verbatim would fill both bags with codes sharing long runs of leading
   zeros, which are close in Hamming distance by construction and would
   inflate the very collision count the channel reads as evidence. */
function mix64(hi, lo) {
  var a = (hi ^ 0x9e3779b9) >>> 0, b = (lo ^ 0x85ebca6b) >>> 0;
  a = Math.imul(a ^ (a >>> 16), 0x7feb352d) >>> 0;
  b = Math.imul(b ^ (b >>> 15), 0x846ca68b) >>> 0;
  a = (a ^ b) >>> 0;
  a = Math.imul(a ^ (a >>> 13), 0xc2b2ae35) >>> 0;
  return (a ^ (a >>> 16)) >>> 0;
}
/* Candidate positions.
 *
 * The first four attempts chose anchors by a property of the CANVAS —
 * a stride grid, saliency maxima, region centroids, quarter buckets —
 * and every one of them moved when the canvas changed, which is the
 * one thing an anchor must not do.  On real dithered art the junction
 * test that replaced them selects 20-40% of all pixels, so it selects
 * nothing at all.
 *
 * What survives cropping and compositing is a STRICT LOCAL MAXIMUM of
 * a content function, decided inside a fixed pixel radius and nowhere
 * else.  Ties are broken by a hash of the patch rather than by
 * position, because position is the thing the crop changes: two crops
 * of one drawing hand the same neighbourhood to the same comparison
 * and therefore elect the same winner. */
function contentPeaks(qmap, sw, sh, r) {
  var n = sw * sh, sal = new Int32Array(n), tie = new Uint32Array(n), x, y, i;
  for (y = 1; y + 1 < sh; y++)
    for (x = 1; x + 1 < sw; x++) {
      var p = y * sw + x, c = qmap[p];
      if (c < 0) continue;
      var e = 0, t = 0;
      for (var dy = -1; dy <= 1; dy++)
        for (var dx = -1; dx <= 1; dx++) {
          var v = qmap[p + dy * sw + dx];
          var g = v < 0 ? 64 : (v > c ? v - c : c - v);
          e += g;
          t = (Math.imul(t, 31) + g) | 0;   /* gradient, not level: survives inversion */
        }
      sal[p] = e; tie[p] = mix64(t, e);
    }
  /* Strict local maximum via a separable running max.
   *
   * The direct test compares every pixel with its (2r+1)^2 neighbours —
   * 31 million comparisons on a 736x352 work, and it dominated hashing
   * at ~600 ms per image.  A max filter is separable, so a horizontal
   * pass followed by a vertical one gives the same answer in O(n) with
   * a monotonic deque.
   *
   * Strength and tie-break are packed into ONE comparable number so the
   * filter can carry the lexicographic order intact: strength needs ten
   * bits and the tie hash thirty-two, which is forty-two, inside the
   * fifty-three a double holds exactly.  Packing loses nothing here,
   * and a peak is then simply a pixel that equals the filtered maximum
   * over its own window. */
  var pack = new Float64Array(n);
  for (i = 0; i < n; i++) pack[i] = sal[i] > 0 ? sal[i] * 4294967296 + (tie[i] >>> 0) : -1;

  var rowMax = new Float64Array(n), q = new Int32Array(Math.max(sw, sh) + 1);
  for (y = 0; y < sh; y++) {
    var base = y * sw, head = 0, tail = 0;
    for (x = 0; x < sw + r; x++) {
      if (x < sw) {
        while (tail > head && pack[base + q[tail - 1]] <= pack[base + x]) tail--;
        q[tail++] = x;
      }
      var cx = x - r;
      if (cx >= 0) {
        while (tail > head && q[head] < cx - r) head++;
        rowMax[base + cx] = pack[base + q[head]];
      }
    }
  }
  var colMax = new Float64Array(n);
  for (x = 0; x < sw; x++) {
    var head2 = 0, tail2 = 0;
    for (y = 0; y < sh + r; y++) {
      if (y < sh) {
        while (tail2 > head2 && rowMax[q[tail2 - 1] * sw + x] <= rowMax[y * sw + x]) tail2--;
        q[tail2++] = y;
      }
      var cy = y - r;
      if (cy >= 0) {
        while (tail2 > head2 && q[head2] < cy - r) head2++;
        colMax[cy * sw + x] = rowMax[q[head2] * sw + x];
      }
    }
  }

  var pts = [];
  for (y = r; y + r < sh; y++)
    for (x = r; x + r < sw; x++) {
      var p3 = y * sw + x;
      if (pack[p3] > 0 && pack[p3] === colMax[p3]) pts.push({ x2: x << 1, y2: y << 1, s: sal[p3] });
    }
  return pts;
}

function localFingerprints(im, opt) {
  var wins = opt.localWindows, want = opt.localCount | 0;
  var sidx = im.idx, sw = im.w, sh = im.h;
  var qmap = new Int32Array(sidx.length);
  for (var qi = 0; qi < sidx.length; qi++) qmap[qi] = sidx[qi] < 0 ? -1 : im.pal[sidx[qi]].quantile;
  var op = new Uint8Array(sw * sh);
  for (var oi = 0; oi < op.length; oi++) op[oi] = sidx[oi] < 0 ? 0 : 1;
  var SO = integral(op, sw, sh);

  var pts = contentPeaks(qmap, sw, sh, opt.peakRadius | 0);
  var cand = [];
  for (var wi = 0; wi < wins.length; wi++) {
    var win = wins[wi] | 0;
    if (sw < win || sh < win) continue;
    var area = win * win;
    for (var i = 0; i < pts.length; i++) {
      var xs = offsets(pts[i].x2, win, sw), ys = offsets(pts[i].y2, win, sh);
      for (var xi = 0; xi < xs.length; xi++)
        for (var yi = 0; yi < ys.length; yi++) {
          var x = xs[xi], y = ys[yi];
          if (x < 0 || y < 0 || x + win > sw || y + win > sh) continue;
          /* The opacity floor is what lets a sprite hashed on its own
             match the same sprite composited onto a host.  Admit
             half-transparent windows and the bits encode the
             SILHOUETTE, which the composite does not have. */
          if (boxSum(SO, sw, x, y, win) * 4 < area * 3) continue;
          var fp = fingerprintOf({ x: x, y: y, win: win, wi: wi, sw: sw, q: qmap,
                                   sal: pts[i].s, foldInvert: opt.foldInvert });
          if (!fp) continue;
          fp.key = mix64(fp.hi, fp.lo);
          cand.push(fp);
        }
    }
  }

  /* Keep the sixty-four smallest scrambled keys.
   *
   * This is the spec's own "MinHash-style" wording, which the earlier
   * canvas-bucketed selection quietly abandoned.  It matters because
   * the decision to keep a window depends ONLY on that window's own
   * content: a drawing hashed alone and the same drawing pasted into a
   * scene nominate the identical windows, so the two bags can
   * intersect at all.  A budget shared out across canvas quarters
   * cannot do that — measured on real works, every bag saturated at
   * the cap and two near-identical crops matched 12 of 64. */
  cand.sort(function (a, b) {
    return (a.key >>> 0) - (b.key >>> 0) || (a.hi >>> 0) - (b.hi >>> 0) ||
           (a.lo >>> 0) - (b.lo >>> 0) || (a.x - b.x) || (a.y - b.y);
  });
  var picked = [], seen = new Set();
  for (var ci = 0; ci < cand.length && picked.length < want; ci++) {
    var e = cand[ci], k = e.hi + ':' + e.lo;
    if (seen.has(k)) continue;      /* a repeated texture is one vote, not forty */
    seen.add(k); picked.push(e);
  }

  /* wire order is sorted by value so the section is comparison-order
     independent and two identical bags serialise identically */
  var wire = picked.slice().sort(function (a, b) {
    return (a.hi >>> 0) - (b.hi >>> 0) || (a.lo >>> 0) - (b.lo >>> 0);
  });
  var cap = 128;
  var out = new Uint8Array(cap * 8), pos = new Uint8Array(cap * 4);
  var maxDim = Math.max(im.w, im.h);
  for (var i2 = 0; i2 < wire.length && i2 < cap; i2++) {
    var o = i2 << 3, e = wire[i2], q = i2 << 2;
    out[o]     = e.hi & 255;        out[o + 1] = (e.hi >>> 8) & 255;
    out[o + 2] = (e.hi >>> 16) & 255; out[o + 3] = (e.hi >>> 24) & 255;
    out[o + 4] = e.lo & 255;        out[o + 5] = (e.lo >>> 8) & 255;
    out[o + 6] = (e.lo >>> 16) & 255; out[o + 7] = (e.lo >>> 24) & 255;
    /* Where each fingerprint sits.  A count cannot answer the question that
       decides a paste: do the matches agree on a PLACEMENT?  Scattered
       agreement is confetti; agreement on one offset is a paste.
       v3 change (SPEC-003 §5.6): ASPECT-TRUE u16 in units of 1/65535 of
       max(w,h), the SAME frame the keypoints use.  v2 encoded these against
       the normalised canvas and decoded them against the original dimensions,
       and normalised x by width and y by height — which turns a similarity
       into an anisotropic map the moment two works differ in aspect ratio. */
    var px = clamp(idiv((e.x + (e.size >> 1)) * 65535, maxDim), 0, 65535);
    var py = clamp(idiv((e.y + (e.size >> 1)) * 65535, maxDim), 0, 65535);
    pos[q] = px & 255; pos[q + 1] = (px >> 8) & 255;
    pos[q + 2] = py & 255; pos[q + 3] = (py >> 8) & 255;
  }
  return { bytes: out, pos: pos, count: Math.min(wire.length, cap), regions: picked };
}

/* ================================================================== *
 * THE INTEGER KEYPOINT PIPELINE   (SPEC-003 §8)
 *
 * v2's L2 half was the better detector (0.82 recall against 0.45) and could
 * not be stored, indexed or verified by a second party, because every step of
 * it was floating point.  Every one of those floats is replaceable:
 *
 *   atan2      -> argmax over 64 integer dot products (§8.4)
 *   RANSAC     -> Hough vote + closed-form fixed-point refit (§9.4)
 *   Canvas     -> the same integer front end the structural half uses (§5)
 *   mirror     -> a bit permutation of the stored descriptor (§8.6)
 *
 * Nothing below uses Math.random, Math.atan2, Math.hypot or a float divide.
 * ================================================================== */

var PATCH_R = 15, N_BITS = 256, N_PAIRS = 128;
/* Border margin.  A rotated pattern point reaches ceil(PATCH_R*sqrt2) from the
   keypoint, and each sample is a 5x5 box on top of that.  v2's L2 had no margin
   at all and relied on boxAvg clamping its window at the edge — which makes the
   sums non-comparable and is a silent source of non-determinism.  Caught by
   tools/test.js: 3 of 3588 mirrored descriptors disagreed until this was 24. */
var KP_MARGIN = Math.ceil(PATCH_R * 1.41422) + 2;   // = 24 at PATCH_R 15
var FAST_T = 18, NMS_R = 4, KP_PER_LEVEL = 512;
/* SPEC-004.2 §2–§3 — the Tier-2 budget and the selection score.  Percentages
   literally: Q = 40 strength + 25 spatial + 20 scale + 15 descriptor novelty,
   every term in the 0..10000 currency, every division truncating. */
var MAX_KP_COUNT = 512;
var Q_W_STRENGTH = 40, Q_W_SPATIAL = 25, Q_W_SCALE = 20, Q_W_DESC = 15;
var DESC_NOVEL_AT = 16, SEL_POOL_MULT = 4, SEL_GRID = 8, SCALE_Q = 10000;
/* The pyramid floor is the smallest level that still HAS an interior: anything
   below 2*KP_MARGIN+4 admits no keypoint at all.  v2's L2 floor of 106 px was
   set for a 119-736 px corpus and starves small pixel art — a 72 px sprite got
   ONE scale, so it could not be matched against the same sprite inside a
   220 px scene, which is the collage case the whole stage exists for. */
var LEVEL_MIN = 2 * KP_MARGIN + 4;

/* Symmetric rounding was needed while the trig tables were built at load time:
   rnd(-v) === -rnd(v) EXACTLY, which Math.round does not give
   (Math.round(0.5)=1 but Math.round(-0.5)=0), and the reflection identity in
   §8.6 depends on it.  The tables are now frozen literals generated under that
   rule by tools/tables.cjs, so no rounding happens at run time at all and
   there is no float anywhere in the wire path.  tools/tables.cjs enforces it. */
/* symmetric arithmetic shift for a Q10 divide */
function sh10(v) { return v >= 0 ? (v + 512) >> 10 : -((-v + 512) >> 10); }

/* 64 orientation sectors.  Built so that, as integers,
      COS64[(32-k)&63] === -COS64[k]   and   SIN64[(32-k)&63] === SIN64[k]
   which is what makes sector(pi - theta) === (32 - sector(theta)) exact. */
/* Frozen as integer literals rather than computed from Math.cos.  V8 and
   Rust's f64::cos may disagree in the last ulp, and after round(x*32768)
   that is a table entry off by one in a CONSENSUS artefact.  Generated once;
   tools/tables.js regenerates and re-checks them.  The identities
   COS64[32-k] === -COS64[k] and SIN64[32-k] === SIN64[k] hold exactly, which
   is what §8.6's mirror trick rests on. */
var COS64 = Int32Array.of(
  32768, 32610, 32138, 31357, 30274, 28899, 27246, 25330, 23170, 20788, 18205, 15447, 12540,
  9512, 6393, 3212, 0, -3212, -6393, -9512, -12540, -15447, -18205, -20788, -23170, -25330,
  -27246, -28899, -30274, -31357, -32138, -32610, -32768, -32610, -32138, -31357, -30274,
  -28899, -27246, -25330, -23170, -20788, -18205, -15447, -12540, -9512, -6393, -3212, 0, 3212,
  6393, 9512, 12540, 15447, 18205, 20788, 23170, 25330, 27246, 28899, 30274, 31357, 32138,
  32610);
var SIN64 = Int32Array.of(
  0, 3212, 6393, 9512, 12540, 15447, 18205, 20788, 23170, 25330, 27246, 28899, 30274, 31357,
  32138, 32610, 32768, 32610, 32138, 31357, 30274, 28899, 27246, 25330, 23170, 20788, 18205,
  15447, 12540, 9512, 6393, 3212, 0, -3212, -6393, -9512, -12540, -15447, -18205, -20788,
  -23170, -25330, -27246, -28899, -30274, -31357, -32138, -32610, -32768, -32610, -32138,
  -31357, -30274, -28899, -27246, -25330, -23170, -20788, -18205, -15447, -12540, -9512, -6393,
  -3212);
var RC10  = Int32Array.of(
  1024, 1019, 1004, 980, 946, 903, 851, 792, 724, 650, 569, 483, 392, 297, 200, 100, 0, -100,
  -200, -297, -392, -483, -569, -650, -724, -792, -851, -903, -946, -980, -1004, -1019, -1024,
  -1019, -1004, -980, -946, -903, -851, -792, -724, -650, -569, -483, -392, -297, -200, -100,
  0, 100, 200, 297, 392, 483, 569, 650, 724, 792, 851, 903, 946, 980, 1004, 1019);
var RS10  = Int32Array.of(
  0, 100, 200, 297, 392, 483, 569, 650, 724, 792, 851, 903, 946, 980, 1004, 1019, 1024, 1019,
  1004, 980, 946, 903, 851, 792, 724, 650, 569, 483, 392, 297, 200, 100, 0, -100, -200, -297,
  -392, -483, -569, -650, -724, -792, -851, -903, -946, -980, -1004, -1019, -1024, -1019,
  -1004, -980, -946, -903, -851, -792, -724, -650, -569, -483, -392, -297, -200, -100);

/* ---- the reflection-closed BRIEF pattern  (SPEC-003 §8.6) ----------
 *
 * 128 pairs are drawn freely.  Bits 128..255 use the same pairs with y
 * NEGATED.  The derivation:
 *
 *   mirror the patch horizontally  ->  the intensity centroid gives
 *   theta' = pi - theta, so sector' = 32 - sector, and
 *
 *       F . rotate(p, 32-k)  ==  rotate(G.p, k),     F = diag(-1,1),
 *                                                    G = diag(1,-1)
 *
 *   Sampling the mirrored image at offset u equals sampling the original at
 *   F.u, so bit_i(mirror) is exactly the bit the ORIGINAL would produce with
 *   pattern G.P_i — i.e. bit_{i+128}.
 *
 *   Therefore  descriptor(mirror) === descriptor(original) with its two
 *   128-bit halves exchanged.  Mirror invariance for ZERO stored bytes,
 *   the same move the DCT already makes for D4 and the local fingerprints
 *   for inversion.
 *
 * NOTE the correction against SPEC-003 as first written: the second half is
 * the Y-mirrored pattern, not the x-mirrored one.  See tools/mirror-test.
 * ------------------------------------------------------------------ */
var PATTERN = (function () {
  var s = 0x1234567 >>> 0, M = 0x7fffffff;
  /* Math.imul, not `*`: s * 1103515245 reaches 2.4e18, past the 2^53 a double
     holds exactly, so the low bits of a plain multiply are rounding noise.
     That is deterministic inside JavaScript and unreproducible anywhere else,
     which is not good enough for an artefact two nodes must agree on. */
  function u() { s = (Math.imul(s, 1103515245) + 12345) & M; return s; }
  function g() {
    var c = u() + u() + u() - ((3 * M) >> 1);          // centred, |c| <= 1.5M
    var v = c >= 0 ? idiv(c * 21 + M, 2 * M) : -idiv(-c * 21 + M, 2 * M);
    return clamp(v, -PATCH_R, PATCH_R);
  }
  var p = new Int32Array(N_BITS * 4);
  for (var i = 0; i < N_PAIRS; i++) {
    var ax = g(), ay = g(), bx = g(), by = g();
    p[i * 4] = ax; p[i * 4 + 1] = ay; p[i * 4 + 2] = bx; p[i * 4 + 3] = by;
    var j = (i + N_PAIRS) * 4;                          // G . P_i  =  y negated
    p[j] = ax; p[j + 1] = -ay; p[j + 2] = bx; p[j + 3] = -by;
  }
  return p;
})();

/* FAST-9 circle, radius 3 */
var CIRC = [[0,-3],[1,-3],[2,-2],[3,-1],[3,0],[3,1],[2,2],[1,3],
            [0,3],[-1,3],[-2,2],[-3,1],[-3,0],[-3,-1],[-2,-2],[-1,-3]];

function integralI32(map, w, h) {
  var S = new Int32Array((w + 1) * (h + 1));
  for (var y = 0; y < h; y++) {
    var row = 0;
    for (var x = 0; x < w; x++) { row += map[y * w + x];
      S[(y + 1) * (w + 1) + x + 1] = S[y * (w + 1) + x + 1] + row; }
  }
  return S;
}
/* sum over the (2r+1)^2 window centred on (cx,cy); caller guarantees it fits */
function boxWin(S, w, cx, cy, r) {
  var W = w + 1, x0 = cx - r, y0 = cy - r, x1 = cx + r + 1, y1 = cy + r + 1;
  return S[y1 * W + x1] - S[y0 * W + x1] - S[y1 * W + x0] + S[y0 * W + x0];
}

/* ---- pyramid ------------------------------------------------------
 * Levels are indexed by max(w,h), NOT by width.  v2's L2 indexed by width,
 * which gave a 736x352 work seven scales and a 119x193 work one — a
 * scale-coverage asymmetry that depended on aspect ratio rather than size.
 *
 * L_{k+1} = round(L_k / 1.3) computed as (10*L + 6) / 13, so the ladder is
 * integer and reproducible rather than a chain of float powers.
 * ------------------------------------------------------------------ */
function pyramidDims(maxDim) {
  var out = [maxDim], L = idiv(maxDim * 10 + 6, 13);   // level 0 ALWAYS exists
  while (L >= LEVEL_MIN) { out.push(L); L = idiv(L * 10 + 6, 13); }
  return out;
}
/* A keypoint's descriptor covers a 31 px patch AT ITS OWN LEVEL, so the
   feature's size in normalised units is proportional to 1 / L_k.  The scale
   ratio between two keypoints is therefore L_kA / L_kB — recoverable from the
   stored level INDEX plus the work's maxDim, both of which are on the wire.
   Deriving scale from the index difference alone (as a naive Hough would) is
   wrong the moment two works differ in size. */
function levelDim(maxDim, k) {
  var L = maxDim;
  for (var i = 0; i < k; i++) L = idiv(L * 10 + 6, 13);
  return Math.max(1, L);
}

function buildLevel(lum, op, w, h, lw, lh, fill) {
  var d = new Uint8Array(lw * lh), o = new Uint8Array(lw * lh);
  for (var j = 0; j < lh; j++) {
    var y0 = idiv(j * h, lh), y1 = idiv((j + 1) * h, lh); if (y1 <= y0) y1 = y0 + 1;
    for (var i = 0; i < lw; i++) {
      var x0 = idiv(i * w, lw), x1 = idiv((i + 1) * w, lw); if (x1 <= x0) x1 = x0 + 1;
      var sum = 0, cnt = 0, tot = 0;
      for (var y = y0; y < y1 && y < h; y++)
        for (var x = x0; x < x1 && x < w; x++) {
          tot++; if (op[y * w + x]) { sum += lum[y * w + x]; cnt++; }
        }
      var p = j * lw + i;
      d[p] = cnt ? idiv(sum, cnt) : fill;
      o[p] = (cnt * 4 >= tot * 3) ? 1 : 0;
    }
  }
  return { d: d, op: o, w: lw, h: lh };
}

/* ---- FAST-9 -------------------------------------------------------
 * Unchanged from v2's L2 and already integer.  What is new is the border
 * margin (so every box sample downstream is a full window — a clamped window
 * makes sums non-comparable and is a hidden source of non-determinism) and
 * the opacity floor, which is the same 75% rule the structural half already
 * applies to its local-fingerprint windows.  Without it, a transparent
 * background becomes a hard black edge and the detector keys on the
 * SILHOUETTE — which is what v2's L2 did, and which is why its recall and
 * the structural half's could not be compared honestly.
 * ------------------------------------------------------------------ */
function fast9(lv) {
  var d = lv.d, w = lv.w, h = lv.h, kps = [];
  var SO = integralI32(lv.op, w, h);
  var lo_ = KP_MARGIN, hix = w - 1 - KP_MARGIN, hiy = h - 1 - KP_MARGIN;
  if (hix <= lo_ || hiy <= lo_) return kps;
  var area = (2 * PATCH_R + 1) * (2 * PATCH_R + 1);
  for (var y = lo_; y <= hiy; y++)
    for (var x = lo_; x <= hix; x++) {
      var p = d[y * w + x], hi = p + FAST_T, low = p - FAST_T, b = 0, dk = 0, q;
      for (q = 0; q < 16; q += 4) {
        var v0 = d[(y + CIRC[q][1]) * w + (x + CIRC[q][0])];
        if (v0 > hi) b++; else if (v0 < low) dk++;
      }
      if (b < 3 && dk < 3) continue;
      if (boxWin(SO, w, x, y, PATCH_R) * 4 < area * 3) continue;   // opacity floor
      var br = 0, dr = 0, score = 0;
      for (var k = 0; k < 16; k++) {
        var v = d[(y + CIRC[k][1]) * w + (x + CIRC[k][0])];
        if (v > hi) br |= (1 << k);
        if (v < low) dr |= (1 << k);
        score += v > p ? v - p : p - v;
      }
      var run = 0, ok = false;
      for (k = 0; k < 24 && !ok; k++) { if ((br >> (k & 15)) & 1) { if (++run >= 9) ok = true; } else run = 0; }
      if (!ok) { run = 0; for (k = 0; k < 24 && !ok; k++) { if ((dr >> (k & 15)) & 1) { if (++run >= 9) ok = true; } else run = 0; } }
      if (ok) kps.push({ x: x, y: y, s: score });
    }
  return kps;
}

/* ---- true-radius NMS, total order ---------------------------------
 * v2's L2 used a grid-approximate suppression whose effective radius varied
 * between 4 and 12 px depending on where a keypoint fell inside its cell — so
 * a 2 px shift could change which of two nearby corners survived.  For an
 * algorithm whose entire premise is finding the SAME points twice, that is a
 * real defect.  A true Euclidean radius over a total order fixes it.
 * ------------------------------------------------------------------ */
function nms(kps, r) {
  kps.sort(function (a, b) { return b.s - a.s || a.y - b.y || a.x - b.x; });
  var kept = [], r2 = r * r;
  for (var i = 0; i < kps.length && kept.length < KP_PER_LEVEL; i++) {
    var k = kps[i], ok = true;
    for (var j = 0; j < kept.length; j++) {
      var dx = kept[j].x - k.x, dy = kept[j].y - k.y;
      if (dx * dx + dy * dy <= r2) { ok = false; break; }
    }
    if (ok) kept.push(k);
  }
  return kept;
}

/* ---- orientation: integer sector, no atan2  (SPEC-003 §8.4) --------
 * The sector is the argmax over k of  m10*cos(phi_k) + m01*sin(phi_k),
 * which IS round-to-nearest by construction — and, because the tables satisfy
 * COS64[32-k] = -COS64[k] and SIN64[32-k] = SIN64[k], it satisfies
 *
 *      sector(-m10, m01) === (32 - sector(m10, m01)) & 63
 *
 * exactly.  That identity is what §8.6's mirror trick rests on.  ECMAScript
 * only requires Math.atan2 to be implementation-APPROXIMATED, so two
 * conforming engines can disagree in the last ulp and a keypoint on a sector
 * boundary then gets a different descriptor.  This removes that entirely.
 *
 * Ties (two sectors exactly equidistant) resolve to the lower index — the one
 * measure-zero case where the mirror identity does not hold.  The test harness
 * counts them.
 * ------------------------------------------------------------------ */
/* the r=7 disc flattened once as (dx, dy) pairs — no branch in the inner loop */
var DISC7 = (function () {
  var t = [];
  for (var dy = -7; dy <= 7; dy++) for (var dx = -7; dx <= 7; dx++)
    if (dx * dx + dy * dy <= 49) t.push(dx, dy);
  return Int32Array.from(t);
})();
function orientSector(S3, w, x, y) {
  var m10 = 0, m01 = 0, W = (w + 1) | 0, n = DISC7.length | 0;
  x = x | 0; y = y | 0;
  for (var i = 0; i < n; i += 2) {
    var dx = DISC7[i] | 0, dy = DISC7[i + 1] | 0;
    var cy = (y + dy) | 0, cx = (x + dx) | 0;
    var r0 = (cy - 1) * W, r1 = (cy + 2) * W, c0 = (cx - 1) | 0, c1 = (cx + 2) | 0;
    var v = (S3[r1 + c1] - S3[r0 + c1] - S3[r1 + c0] + S3[r0 + c0]) | 0;   /* 3x3 SUM */
    m10 += dx * v; m01 += dy * v;
  }
  if (m10 === 0 && m01 === 0) return -1;      // no orientation at all
  var bestK = 0, bestV = -Infinity, tied = 0;
  for (var k = 0; k < 64; k++) {
    var dot = m10 * COS64[k] + m01 * SIN64[k];
    if (dot > bestV) { bestV = dot; bestK = k; tied = 1; }
    else if (dot === bestV) tied++;
  }
  /* A tie is the ONE case where sector(pi - theta) != 32 - sector(theta): the
     two tied sectors are adjacent, and "lowest index" is not preserved by the
     map k -> 32-k.  Rather than invent an asymmetric tie-break that quietly
     breaks the mirror identity, REFUSE the keypoint.  An orientation this
     ambiguous produces a descriptor that is not repeatable anyway.  Measured
     rate on random content: under 1% of candidates. */
  return tied > 1 ? -1 : bestK;
}

/* ---- steered BRIEF-256, integer -----------------------------------
 * The rotated pattern for a sector is identical for every keypoint at that
 * sector, so it is built ONCE and cached rather than recomputed as 1024
 * multiply-shifts per keypoint.  64 sectors x 1024 int32 = 256 kB, lazy; on a
 * work with 256 keypoints that removes about 260k multiplies from the hash.
 * ------------------------------------------------------------------ */
var ROT_CACHE = new Array(64);
function rotatedPattern(k) {
  var t = ROT_CACHE[k];
  if (t !== undefined) return t;
  t = new Int32Array(N_BITS * 4);
  var c = RC10[k] | 0, s = RS10[k] | 0;
  for (var i = 0; i < N_BITS; i++) {
    var o = i << 2;
    var pax = PATTERN[o] | 0, pay = PATTERN[o + 1] | 0;
    var pbx = PATTERN[o + 2] | 0, pby = PATTERN[o + 3] | 0;
    t[o]     = sh10(pax * c - pay * s);
    t[o + 1] = sh10(pax * s + pay * c);
    t[o + 2] = sh10(pbx * c - pby * s);
    t[o + 3] = sh10(pbx * s + pby * c);
  }
  ROT_CACHE[k] = t;
  return t;
}
function describe(S, w, x, y, k) {
  var bits = new Uint32Array(8), R = rotatedPattern(k), W = (w + 1) | 0;
  x = x | 0; y = y | 0;
  for (var i = 0; i < N_BITS; i++) {
    var o = i << 2;
    var ax = (x + R[o]) | 0, ay = (y + R[o + 1]) | 0;
    var bx = (x + R[o + 2]) | 0, by = (y + R[o + 3]) | 0;
    /* box SUMS, not means: the divisor is constant so it cannot change the
       comparison, and sums stay exact.  The 5x5 average is what makes BRIEF
       work on dithered pixel art at all — a raw two-pixel test measures the
       dither, not the drawing.  Ties resolve to 0, specified rather than
       left to whichever comparison operator happened to be typed. */
    var ar0 = (ay - 2) * W, ar1 = (ay + 3) * W, ac0 = (ax - 2) | 0, ac1 = (ax + 3) | 0;
    var sa = (S[ar1 + ac1] - S[ar0 + ac1] - S[ar1 + ac0] + S[ar0 + ac0]) | 0;
    var br0 = (by - 2) * W, br1 = (by + 3) * W, bc0 = (bx - 2) | 0, bc1 = (bx + 3) | 0;
    var sb = (S[br1 + bc1] - S[br0 + bc1] - S[br1 + bc0] + S[br0 + bc0]) | 0;
    if (sa < sb) bits[i >> 5] |= (1 << (i & 31));
  }
  return bits;
}

/* descriptor of the horizontally mirrored patch: exchange the two halves */
function mirrorDesc(d) {
  var o = new Uint32Array(8);
  o[0] = d[4]; o[1] = d[5]; o[2] = d[6]; o[3] = d[7];
  o[4] = d[0]; o[5] = d[1]; o[6] = d[2]; o[7] = d[3];
  return o;
}


/* ---- SPEC-004.2 §3: selection ------------------------------------
 * PAPH 4.1 spread the budget over an 8x8 grid, which fixes the collage case
 * (a busy host out-scoring a pasted figure) and does nothing at all about the
 * OTHER way 512 keypoints can lie: a brick wall yields two hundred keypoints
 * describing one local texture, and spreading them out spatially does not make
 * them two hundred observations.  4.2 therefore pays for four different kinds
 * of being new, and the descriptor term is the one that matters most.
 *
 * Both rules are kept.  A 4.1 wire has to stay reproducible, and reproducing
 * it means running the code that made it, not an approximation of it.
 * ------------------------------------------------------------------ */
function selectGrid(all, want) {
  var cells = new Map(), gi;
  for (gi = 0; gi < all.length; gi++) {
    var gk = ((all[gi].y * 8 / 65536) | 0) * 8 + ((all[gi].x * 8 / 65536) | 0);
    if (!cells.has(gk)) cells.set(gk, []);
    cells.get(gk).push(all[gi]);
  }
  var keys = Array.from(cells.keys()).sort(function (a, b) { return a - b; });
  var picked = [], round = 0;
  while (picked.length < want) {
    var took = 0;
    for (gi = 0; gi < keys.length && picked.length < want; gi++) {
      var bucket = cells.get(keys[gi]);
      if (round < bucket.length) { picked.push(bucket[round]); took++; }
    }
    if (!took) break;
    round++;
  }
  return picked;
}

function hamDesc8(a, b) {
  var d = 0, k;
  for (k = 0; k < 8; k++) d += popcount32((a[k] ^ b[k]) >>> 0);
  return d;
}

function selectQuality(all, want) {
  var n = Math.min(all.length, want * SEL_POOL_MULT), i, j;
  var pool = all.slice(0, n);
  var smax = 1;
  for (i = 0; i < n; i++) if (pool[i].s > smax) smax = pool[i].s;
  var qs = new Int32Array(n), cell = new Int32Array(n);
  for (i = 0; i < n; i++) {
    qs[i] = clamp(idiv(pool[i].s * SCALE_Q, smax), 0, SCALE_Q);
    cell[i] = idiv(pool[i].y * SEL_GRID, 65536) * SEL_GRID + idiv(pool[i].x * SEL_GRID, 65536);
  }
  var cellc = new Int32Array(SEL_GRID * SEL_GRID), levc = new Int32Array(256);
  var dmin = new Int32Array(n).fill(N_BITS);
  var taken = new Uint8Array(n), picked = [];
  while (picked.length < want) {
    var bestI = -1, bestQ = -1;
    for (i = 0; i < n; i++) {
      if (taken[i]) continue;
      var qSpatial = idiv(SCALE_Q, 1 + cellc[cell[i]]);
      var qScale = idiv(SCALE_Q, 1 + levc[pool[i].level]);
      var qDesc = clamp(idiv(dmin[i] * SCALE_Q, DESC_NOVEL_AT), 0, SCALE_Q);
      var q = idiv(Q_W_STRENGTH * qs[i] + Q_W_SPATIAL * qSpatial +
                   Q_W_SCALE * qScale + Q_W_DESC * qDesc, 100);
      /* strictly greater: ties go to the lower index, i.e. the stronger
         candidate under the pooled total order */
      if (q > bestQ) { bestQ = q; bestI = i; }
    }
    if (bestI < 0) break;
    taken[bestI] = 1;
    cellc[cell[bestI]]++;
    levc[pool[bestI].level]++;
    picked.push(pool[bestI]);
    for (j = 0; j < n; j++) if (!taken[j]) {
      var d = hamDesc8(pool[j].desc, pool[bestI].desc);
      if (d < dmin[j]) dmin[j] = d;
    }
  }
  return picked;
}

/* ---- the whole keypoint stage ------------------------------------- */
function keypoints(im, want, select) {
  var w = im.w, h = im.h, n = w * h, i;
  var lum = new Uint8Array(n), op = new Uint8Array(n);
  var lums = [];
  for (i = 0; i < n; i++) {
    if (im.idx[i] < 0) { op[i] = 0; lum[i] = 0; }
    else { op[i] = 1; lum[i] = im.pal[im.idx[i]].lum; lums.push(lum[i]); }
  }
  lums.sort(function (a, b) { return a - b; });
  var fill = lums.length ? lums[lums.length >> 1] : 128;
  for (i = 0; i < n; i++) if (!op[i]) lum[i] = fill;

  var maxDim = Math.max(w, h), dims = pyramidDims(maxDim), all = [];
  for (var li = 0; li < dims.length; li++) {
    var L = dims[li];
    var lw = Math.max(1, idiv(w * L, maxDim)), lh = Math.max(1, idiv(h * L, maxDim));
    if (lw < 2 * KP_MARGIN + 4 || lh < 2 * KP_MARGIN + 4) continue;
    var lv = buildLevel(lum, op, w, h, lw, lh, fill);
    var raw = fast9(lv);
    if (!raw.length) continue;
    var kept = nms(raw, NMS_R);
    /* Normalise strength by the LEVEL's own median before pooling.  v2's L2
       ranked pooled keypoints by raw FAST score, comparing scores computed at
       different resolutions — downsampling smooths, so coarse levels were
       systematically starved. */
    var sc = kept.map(function (k) { return k.s; }).sort(function (a, b) { return a - b; });
    var med = Math.max(1, sc[sc.length >> 1]);
    var S = integralI32(lv.d, lw, lh);
    for (var q = 0; q < kept.length; q++) {
      var kp = kept[q];
      var sec = orientSector(S, lw, kp.x, kp.y);
      if (sec < 0) continue;                     // ambiguous orientation, refused
      var desc = describe(S, lw, kp.x, kp.y, sec);
      all.push({
        desc: desc, level: li, sec: sec,
        x: clamp(idiv(idiv(kp.x * w, lw) * 65535, maxDim), 0, 65535),
        y: clamp(idiv(idiv(kp.y * h, lh) * 65535, maxDim), 0, 65535),
        s: clamp(idiv(kp.s * 1024, med), 0, 65535)
      });
    }
  }
  all.sort(function (a, b) { return b.s - a.s || a.level - b.level || a.y - b.y || a.x - b.x; });
  /* Spread the budget over an 8x8 spatial grid before capping.  Taking the
     globally strongest `want` keypoints looks fair and is not: a busy host —
     a city skyline, a dithered background — out-scores a pasted figure and
     crowds every one of its keypoints out of the budget, so the collage case
     the geometric stage EXISTS for is exactly the one the cap silences.
     Round-robin over occupied cells keeps a foothold everywhere. */
  if (all.length > want) all = (select === 1 ? selectQuality : selectGrid)(all, want);
  all = all.slice(0, want);
  /* wire order is content-derived and total, so two identical keypoint sets
     serialise identically and comparison is order-independent */
  all.sort(function (a, b) {
    return (a.desc[0] >>> 0) - (b.desc[0] >>> 0) || (a.desc[1] >>> 0) - (b.desc[1] >>> 0) ||
           a.x - b.x || a.y - b.y || a.level - b.level; });
  return { list: all, maxDim: maxDim, xmax: clamp(idiv((w - 1) * 65535, maxDim), 0, 65535) };
}

/* ================================================================== *
 * WIRE FORMAT   (SPEC-003 §6, §7)
 *
 * Tier 1 — 3952 B, fixed layout, constant offsets, section TABLE.
 *          v2 hardcoded its offsets as constants and this family has already
 *          paid once for a stale offset after a section was inserted.  With a
 *          table a v4 parser adding a section cannot move an existing one, and
 *          a v3 parser skips what it does not recognise.
 *
 * Tier 2 — variable length, fixed 40 B records, content-addressed to Tier 1
 *          by CRC.  Different rules on purpose: Tier 1 is index-scanned by
 *          byte range, Tier 2 is not.
 *
 * CRC-32 rather than v2's XOR: an XOR cannot detect a transposition of two
 * bytes, which is exactly the corruption a byte-range index introduces.
 * ================================================================== */

var CRC_T = (function () {
  var t = new Int32Array(256);
  for (var n = 0; n < 256; n++) {
    var c = n;
    for (var k = 0; k < 8; k++) c = (c & 1) ? (0xEDB88320 ^ (c >>> 1)) : (c >>> 1);
    t[n] = c;
  }
  return t;
})();
function crc32(b, from, to) {
  var c = -1;
  for (var i = from; i < to; i++) c = CRC_T[(c ^ b[i]) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}

var SECTIONS = [
  { id: 1,  name: 'dct',        len: 256  },
  { id: 2,  name: 'brightness', len: 8    },
  { id: 3,  name: 'palette',    len: 96   },
  { id: 4,  name: 'rag',        len: 288  },
  { id: 5,  name: 'shapes',     len: 328  },
  { id: 6,  name: 'runs',       len: 48   },
  { id: 7,  name: 'local',      len: 1024 },
  { id: 8,  name: 'anchors',    len: 512  },
  { id: 9,  name: 'silhouette', len: 96   },
  { id: 10, name: 'colour',     len: 80   },
  { id: 11, name: 'sketch',     len: 1152 }
];
var HEADER1 = 64, HEADER2 = 32, KP_REC = 40;
var T1_BYTES = (function () { var t = HEADER1;
  for (var i = 0; i < SECTIONS.length; i++) t += SECTIONS[i].len; return t; })();   // 3952
var SEC_OFF = (function () { var o = {}, t = HEADER1;
  for (var i = 0; i < SECTIONS.length; i++) { o[SECTIONS[i].name] = t; t += SECTIONS[i].len; }
  return o; })();

var F_MATTE = 1, F_FLAT = 2, F_INVFOLD = 4, F_SIL = 8, F_UPSCALED = 16;
/* SPEC-004.2 §3 — Tier 2 was selected by the 4.2 quality rule.  A 4.1 wire has
   this bit clear, which is exactly what a 4.1 wire means: nothing has to be
   re-hashed to stay readable. */
var F_KPQ = 32;

function serializeT1(d) {
  var b = new Uint8Array(T1_BYTES);
  b[0] = 0x50; b[1] = 0x41; b[2] = 0x50; b[3] = 0x48;      // "PAPH"
  b[4] = VERSION; b[5] = 1;
  b[6] = d.flags & 255; b[7] = (d.flags >> 8) & 255;
  b[8] = d.width & 255; b[9] = (d.width >> 8) & 255;
  b[10] = d.height & 255; b[11] = (d.height >> 8) & 255;
  b[12] = clamp(d.scale, 1, 255);
  var md = Math.max(d.width, d.height, 2), lg = 0;
  while ((1 << lg) < md) lg++;          /* integer ceil(log2) — Math.log2 is
                                           implementation-approximated, and this
                                           byte reaches the wire */
  b[13] = clamp(lg, 1, 31);
  b[14] = d.kpCount & 255; b[15] = (d.kpCount >> 8) & 255;
  for (var i = 0; i < SECTIONS.length; i++) {
    var s = SECTIONS[i], o = 16 + i * 4;
    b[o] = s.id; b[o + 1] = clamp(d.counts[s.name] || 0, 0, 255);
    b[o + 2] = s.len & 255; b[o + 3] = (s.len >> 8) & 255;
    b.set(d.sec[s.name], SEC_OFF[s.name]);
  }
  var c = crc32(b, HEADER1, T1_BYTES);
  b[60] = c & 255; b[61] = (c >>> 8) & 255; b[62] = (c >>> 16) & 255; b[63] = (c >>> 24) & 255;
  return b;
}

function parseT1(bytes) {
  var b = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  if (b.length !== T1_BYTES) throw new RangeError('paph3: tier 1 must be ' + T1_BYTES + ' bytes, got ' + b.length);
  if (!(b[0] === 0x50 && b[1] === 0x41 && b[2] === 0x50 && b[3] === 0x48)) throw new TypeError('paph3: bad magic');
  if (b[4] !== VERSION) throw new RangeError('paph3: unsupported version ' + b[4] + ' (v2 wires MUST be rejected, never reinterpreted)');
  if (b[5] !== 1) throw new RangeError('paph3: not a tier 1 wire');
  var c = crc32(b, HEADER1, T1_BYTES);
  var want = (b[60] | (b[61] << 8) | (b[62] << 16) | (b[63] << 24)) >>> 0;
  if (c !== want) throw new RangeError('paph3: tier 1 checksum mismatch');
  var d = { version: b[4], tier: 1, flags: b[6] | (b[7] << 8),
            width: b[8] | (b[9] << 8), height: b[10] | (b[11] << 8),
            scale: b[12], kpCount: b[14] | (b[15] << 8), counts: {}, bytes: b, crc: c };
  for (var i = 0; i < SECTIONS.length; i++) {
    var s = SECTIONS[i], o = 16 + i * 4;
    if (b[o] !== s.id) throw new RangeError('paph3: section table mismatch at ' + i);
    d.counts[s.name] = b[o + 1];
    d[s.name] = b.subarray(SEC_OFF[s.name], SEC_OFF[s.name] + s.len);
  }
  d.maxDim = Math.max(d.width, d.height);
  return d;
}

function serializeT2(kps, t1crc, maxDim, xmax, select) {
  var n = Math.min(kps.length, MAX_KP_COUNT);
  var b = new Uint8Array(HEADER2 + n * KP_REC);
  b[0] = 0x50; b[1] = 0x41; b[2] = 0x50; b[3] = 0x32;      // "PAP2"
  b[4] = VERSION; b[5] = 2;
  b[6] = n & 255; b[7] = (n >> 8) & 255;
  b[8] = t1crc & 255; b[9] = (t1crc >>> 8) & 255; b[10] = (t1crc >>> 16) & 255; b[11] = (t1crc >>> 24) & 255;
  b[12] = maxDim & 255; b[13] = (maxDim >> 8) & 255;
  b[14] = xmax & 255; b[15] = (xmax >> 8) & 255;
  /* byte 16 was reserved and therefore zero on every 4.1 wire, which reads
     back as "legacy selection" without a version bump */
  b[16] = (select | 0) & 255;
  for (var i = 0; i < n; i++) {
    var k = kps[i], o = HEADER2 + i * KP_REC;
    for (var j = 0; j < 8; j++) {
      var v = k.desc[j] >>> 0;
      b[o + j * 4] = v & 255; b[o + j * 4 + 1] = (v >>> 8) & 255;
      b[o + j * 4 + 2] = (v >>> 16) & 255; b[o + j * 4 + 3] = (v >>> 24) & 255;
    }
    b[o + 32] = k.x & 255; b[o + 33] = (k.x >> 8) & 255;
    b[o + 34] = k.y & 255; b[o + 35] = (k.y >> 8) & 255;
    b[o + 36] = k.level & 255;
    b[o + 37] = (k.sec & 63) | 64;
    b[o + 38] = k.s & 255; b[o + 39] = (k.s >> 8) & 255;
  }
  var c = crc32(b, HEADER2, b.length);
  b[28] = c & 255; b[29] = (c >>> 8) & 255; b[30] = (c >>> 16) & 255; b[31] = (c >>> 24) & 255;
  return b;
}

function parseT2(bytes) {
  var b = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  if (b.length < HEADER2) throw new RangeError('paph3: tier 2 too short');
  if (!(b[0] === 0x50 && b[1] === 0x41 && b[2] === 0x50 && b[3] === 0x32)) throw new TypeError('paph3: bad tier 2 magic');
  var n = b[6] | (b[7] << 8);
  if (b.length !== HEADER2 + n * KP_REC) throw new RangeError('paph3: tier 2 length does not match its record count');
  var c = crc32(b, HEADER2, b.length);
  var want = (b[28] | (b[29] << 8) | (b[30] << 16) | (b[31] << 24)) >>> 0;
  if (c !== want) throw new RangeError('paph3: tier 2 checksum mismatch');
  var list = [];
  for (var i = 0; i < n; i++) {
    var o = HEADER2 + i * KP_REC, desc = new Uint32Array(8);
    for (var j = 0; j < 8; j++)
      desc[j] = (b[o + j * 4] | (b[o + j * 4 + 1] << 8) | (b[o + j * 4 + 2] << 16) | (b[o + j * 4 + 3] << 24)) >>> 0;
    list.push({ desc: desc, x: b[o + 32] | (b[o + 33] << 8), y: b[o + 34] | (b[o + 35] << 8),
                level: b[o + 36], sec: b[o + 37] & 63, s: b[o + 38] | (b[o + 39] << 8) });
  }
  return { tier: 2, count: n, select: b[16] | 0,
           t1crc: (b[8] | (b[9] << 8) | (b[10] << 16) | (b[11] << 24)) >>> 0,
           maxDim: b[12] | (b[13] << 8), xmax: b[14] | (b[15] << 8), list: list, bytes: b };
}

/* the 32-keypoint Tier 1 sketch — retrieval, and a Tier-1-only weak
   geometric check.  Without it an index built on Tier 1 alone could only
   retrieve through the fingerprint bag, and the pairs the geometric stage
   EXISTS for are exactly the ones whose bags barely intersect. */
function sketchBytes(kps, want) {
  var out = new Uint8Array(want * 36);
  var byStrength = kps.slice().sort(function (a, b) {
    return b.s - a.s || a.x - b.x || a.y - b.y; }).slice(0, want);
  byStrength.sort(function (a, b) {
    return (a.desc[0] >>> 0) - (b.desc[0] >>> 0) || a.x - b.x || a.y - b.y; });
  for (var i = 0; i < byStrength.length; i++) {
    var k = byStrength[i], o = i * 36;
    for (var j = 0; j < 8; j++) {
      var v = k.desc[j] >>> 0;
      out[o + j * 4] = v & 255; out[o + j * 4 + 1] = (v >>> 8) & 255;
      out[o + j * 4 + 2] = (v >>> 16) & 255; out[o + j * 4 + 3] = (v >>> 24) & 255;
    }
    out[o + 32] = k.x & 255; out[o + 33] = (k.x >> 8) & 255;
    out[o + 34] = k.y & 255; out[o + 35] = (k.y >> 8) & 255;
  }
  return { bytes: out, count: byStrength.length };
}
function readSketch(d) {
  var n = d.counts.sketch, out = [];
  for (var i = 0; i < n; i++) {
    var o = i * 36, desc = new Uint32Array(8);
    for (var j = 0; j < 8; j++)
      desc[j] = (d.sketch[o + j * 4] | (d.sketch[o + j * 4 + 1] << 8) |
                 (d.sketch[o + j * 4 + 2] << 16) | (d.sketch[o + j * 4 + 3] << 24)) >>> 0;
    out.push({ desc: desc, x: d.sketch[o + 32] | (d.sketch[o + 33] << 8),
               y: d.sketch[o + 34] | (d.sketch[o + 35] << 8), level: 0, sec: 0, s: 0 });
  }
  return out;
}

/* ------------------------------------------------------------------ *
 * hash()  —  one pass, both tiers, one normalised image
 * ------------------------------------------------------------------ */
function hash(a, b, c, opts) {
  if (a && typeof b === 'object' && b !== null) { opts = b; b = undefined; c = undefined; }
  var o = {}; for (var k in DEFAULTS) o[k] = DEFAULTS[k];
  if (opts) for (k in opts) if (opts[k] !== undefined) o[k] = opts[k];

  var img = readImage(a, b, c);
  var W0 = img.w, H0 = img.h, px = img.px, w = img.w, h = img.h;
  var flags = 0, scale = 1;

  if (o.foldMatte) { var m = foldMatte(px, w, h, o.matteTol);
    if (m.changed) { px = m.px; flags |= F_MATTE; } }
  if (o.divideUpscale) { scale = upscaleFactor(px, w, h);
    if (scale > 1) { var s = shrink(px, w, h, scale); px = s.px; w = s.w; h = s.h; flags |= F_UPSCALED; } }
  if (o.foldInvert) flags |= F_INVFOLD;

  var im = indexImage(px, w, h);
  var thumb = thumbnail16(im);
  var flat = true;
  for (var ti = 1; ti < thumb.length; ti++) if (thumb[ti] !== thumb[0]) { flat = false; break; }
  if (flat) flags |= F_FLAT;

  /* DC is ALWAYS dropped from the code (SPEC-003 §6.3); it lives in its own
     record instead, so the index bucket cannot degenerate into brightness. */
  var dct   = hierarchicalDCT(thumb, false);
  var brt   = brightnessRecord(im, thumb);
  var pal   = identityPalette(im);
  var rag   = sparseRAG(im);
  var shp   = shapeSignatures(im, 'band');
  var runs  = runLengths(im);
  var loc   = localFingerprints(im, o);
  var sil   = silhouetteSignature(im);
  var col   = colourDigest(im);
  if (sil.measurable) flags |= F_SIL;

  var kpSel = (o.kpSelect === 0 || o.kpSelect === 1) ? o.kpSelect : 1;
  if (kpSel === 1) flags |= F_KPQ;
  var kp    = keypoints(im, o.kpCount, kpSel);
  var sk    = sketchBytes(kp.list, o.sketchCount);

  var d = {
    version: VERSION, flags: flags, width: W0, height: H0, scale: scale,
    kpCount: kp.list.length,
    sec: { dct: dct, brightness: brt, palette: pal.bytes, rag: rag.bytes,
           shapes: shp.bytes, runs: runs, local: loc.bytes, anchors: loc.pos,
           silhouette: sil.bytes, colour: col.bytes, sketch: sk.bytes },
    counts: { dct: 1, brightness: 1, palette: pal.count, rag: rag.count,
              shapes: shp.count, runs: 3, local: loc.count, anchors: loc.count,
              silhouette: sil.measurable ? 1 : 0, colour: col.count, sketch: sk.count }
  };
  var t1 = serializeT1(d);
  var t1crc = (t1[60] | (t1[61] << 8) | (t1[62] << 16) | (t1[63] << 24)) >>> 0;
  var t2 = serializeT2(kp.list, t1crc, kp.maxDim, kp.xmax, kpSel);

  var parsed = parseT1(t1);
  parsed.t1 = t1; parsed.t2 = t2;
  parsed.detail = { thumb: thumb, palette: im.pal, shapes: shp.shapes, grid: shp.grid,
                    regions: loc.regions, keypoints: kp.list, opts: o,
                    normalized: { w: w, h: h }, xmax: kp.xmax, maxDim: kp.maxDim };
  return parsed;
}

/* ================================================================== *
 * COMPARISON   (SPEC-003 §9)
 *
 * The channel contract is inherited verbatim from v2 §5.1 — it is the
 * best-tested thing in this family.  Every channel reports raw, control,
 * measurable and controlRan; a channel that cannot evaluate ABSTAINS and never
 * returns a number that happens to read as favourable.
 *
 * What is new: the geometric channel now has a null (§9.4), so it can be
 * REASONED about beside the others instead of being a bare count bolted on.
 * ================================================================== */

function chanceCorrect(raw, ctl) {
  if (ctl >= SCALE) return 0;
  return clamp(idiv((raw - ctl) * SCALE, SCALE - ctl), 0, SCALE);
}
function abstain(why) { return { value: 0, raw: 0, control: 0, measurable: false, controlRan: false, note: why }; }
/* Exact integer square root.  The seed comes from Math.sqrt but the two
   correction loops pin the result to the unique integer r with
   r*r <= n < (r+1)*(r+1), so the OUTPUT is engine-independent even though the
   seed is not.  The naive linear version was O(sqrt n) and, called twice per
   pair inside the O(n^2) coherence check, was 49 ms of a 52 ms compare. */
function isqrt(n) {
  if (n <= 0) return 0;
  var r = Math.floor(Math.sqrt(n));
  while (r > 0 && r * r > n) r--;
  while ((r + 1) * (r + 1) <= n) r++;
  return r;
}
function popcnt32(x) { return POP[x & 0xffff] + POP[(x >>> 16) & 0xffff]; }

/* ---- 9.3a  hierarchical DCT, dihedral-aware ----------------------- */
var D4_INVERSE = [0, 1, 2, 3, 4, 6, 5, 7];
var DIHEDRAL = ['Identity', 'Flip H', 'Flip V', 'Rotate 180',
                'Transpose', 'Rotate 90', 'Rotate 270', 'Anti-transpose'];

function unpackBlock(bytes, off, n, bits) {
  var sign = new Uint8Array(n), mag = new Uint8Array(n), bit = 0, mbits = bits - 1;
  for (var i = 0; i < n; i++) {
    var code = 0;
    for (var k = 0; k < bits; k++) { code = (code << 1) | ((bytes[off + (bit >> 3)] >> (7 - (bit & 7))) & 1); bit++; }
    sign[i] = (code >> mbits) & 1; mag[i] = code & ((1 << mbits) - 1);
  }
  return { sign: sign, mag: mag, n: n, bits: bits };
}
function dctBlocks(d) {
  var L1 = [], L2 = [], i;
  for (i = 0; i < 4; i++) L1.push(unpackBlock(d.dct, 64 + i * 16, 64, 2));
  for (i = 0; i < 16; i++) L2.push(unpackBlock(d.dct, 128 + i * 8, 16, 4));
  return { l0: unpackBlock(d.dct, 0, 256, 2), l1: L1, l2: L2 };
}
function d4Block(B, N, tr, fh, fv) {
  var n = N * N, sign = new Uint8Array(n), mag = new Uint8Array(n);
  for (var v = 0; v < N; v++) for (var u = 0; u < N; u++) {
    var src = tr ? (u * N + v) : (v * N + u), s = B.sign[src];
    if (fh && (u & 1)) s ^= 1;
    if (fv && (v & 1)) s ^= 1;
    sign[v * N + u] = s; mag[v * N + u] = B.mag[src];
  }
  return { sign: sign, mag: mag, n: n, bits: B.bits };
}
function d4Grid(gx, gy, G, tr, fh, fv) {
  var x = gx, y = gy, t;
  if (tr) { t = x; x = y; y = t; }
  if (fh) x = G - 1 - x;
  if (fv) y = G - 1 - y;
  return y * G + x;
}
function blockSim(X, Y, inv) {
  var d = 0, n = X.n, f = inv ? 1 : 0;
  for (var i = 1; i < n; i++) { d += (X.sign[i] ^ f) ^ Y.sign[i]; d += POP[X.mag[i] ^ Y.mag[i]]; }
  var nb = (n - 1) * X.bits;
  return clamp(SCALE - idiv(2 * d * SCALE, nb), 0, SCALE);
}
function dctChannel(A, B) {
  if ((A.flags & F_FLAT) || (B.flags & F_FLAT))
    return abstain('one side has no tonal structure to transform');
  var PA = dctBlocks(A), PB = dctBlocks(B), i, per = [];
  /* SIXTEEN hypotheses: the eight of D4, each with and without a global sign
     flip.  Inverting a work's luminance negates every AC coefficient, so an
     inverted copy is a SIGN FLIP on the stored code — the same "compute the
     symmetry from stored bits" move the local fingerprints make for the
     complement and the sign/magnitude split makes for D4.  v2 had inversion
     invariance in the local channel ONLY, so an inverted copy lost every
     corroborating channel and the gate collapsed. */
  for (var inv = 0; inv < 2; inv++)
  for (var t = 0; t < 8; t++) {
    var tr = (t >> 2) & 1, fh = (t >> 1) & 1, fv = t & 1;
    var l0 = blockSim(d4Block(PA.l0, 16, tr, fh, fv), PB.l0, inv), l1 = 0, l2 = 0;
    for (i = 0; i < 4; i++) l1 += blockSim(d4Block(PA.l1[i], 8, tr, fh, fv), PB.l1[d4Grid(i & 1, i >> 1, 2, tr, fh, fv)], inv);
    l1 = idiv(l1, 4);
    for (i = 0; i < 16; i++) l2 += blockSim(d4Block(PA.l2[i], 4, tr, fh, fv), PB.l2[d4Grid(i & 3, i >> 2, 4, tr, fh, fv)], inv);
    l2 = idiv(l2, 16);
    per.push({ t: t, inv: inv, v: idiv(2 * l0 + 2 * l1 + l2, 5), l0: l0, l1: l1, l2: l2 });
  }
  var best = per[0]; for (i = 1; i < per.length; i++) if (per[i].v > best.v) best = per[i];
  /* taking the best of sixteen inflates chance agreement, so the control is
     the median of the sixteen — the measurement is only worth what it beats */
  var med = per.map(function (p) { return p.v; }).sort(function (a, b) { return a - b; })[8];
  return { value: chanceCorrect(best.v, med), raw: best.v, control: med, measurable: true, controlRan: true,
           l0: best.l0, l1: best.l1, l2: best.l2, t: best.t, inverted: !!best.inv,
           transform: DIHEDRAL[best.t],
           note: 'global layout at three scales, best of sixteen symmetries ('
                 + DIHEDRAL[best.t] + (best.inv ? ' + inverted' : '') + ')' };
}

/* ---- brightness: reported, never summed  (SPEC-003 §6.3) ---------- */
function brightnessRelation(A, B) {
  var d = 0, i;
  for (i = 0; i < 6; i++) { var v = A.brightness[i] - B.brightness[i]; d += v < 0 ? -v : v; }
  var mean = A.brightness[0] - B.brightness[0];
  var shape = 0;
  for (i = 1; i < 6; i++) {
    var da = A.brightness[i] - A.brightness[0], db = B.brightness[i] - B.brightness[0];
    var s = da - db; shape += s < 0 ? -s : s;
  }
  return { delta: d, meanDelta: mean, shapeDelta: shape,
           relation: shape <= 24 ? (Math.abs(mean) <= 8 ? 'same tone' : 'tone-shifted') : 'different tone' };
}

/* ---- 9.3b  local fingerprints — the collage channel ---------------
 * Bags are held FLAT (four parallel typed arrays) rather than as an array of
 * arrays.  Burst weighting and the rotation null are both O(n^2) over a bag of
 * 128, so the memory layout is worth more here than any algorithmic change.
 * ------------------------------------------------------------------ */
function readLocal(d) {
  var n = d.counts.local, s = d.local, p = d.anchors;
  var hi = new Int32Array(n), lo = new Int32Array(n), x = new Int32Array(n), y = new Int32Array(n);
  for (var i = 0; i < n; i++) {
    var o = i << 3, q = i << 2;
    hi[i] = (s[o] | (s[o + 1] << 8) | (s[o + 2] << 16) | (s[o + 3] << 24)) | 0;
    lo[i] = (s[o + 4] | (s[o + 5] << 8) | (s[o + 6] << 16) | (s[o + 7] << 24)) | 0;
    x[i] = p[q] | (p[q + 1] << 8); y[i] = p[q + 2] | (p[q + 3] << 8);
  }
  return { hi: hi, lo: lo, x: x, y: y, n: n };
}
function rotBag(Y, k) {
  var o = { hi: new Int32Array(Y.n), lo: new Int32Array(Y.n), x: Y.x, y: Y.y, n: Y.n }, i;
  k &= 63;
  for (i = 0; i < Y.n; i++) {
    var hi = Y.hi[i], lo = Y.lo[i];
    if (k === 0) { o.hi[i] = hi; o.lo[i] = lo; }
    else if (k === 32) { o.hi[i] = lo; o.lo[i] = hi; }
    else if (k < 32) { o.hi[i] = ((hi << k) | (lo >>> (32 - k))) | 0; o.lo[i] = ((lo << k) | (hi >>> (32 - k))) | 0; }
    else { var j = k - 32; o.hi[i] = ((lo << j) | (hi >>> (32 - j))) | 0; o.lo[i] = ((hi << j) | (lo >>> (32 - j))) | 0; }
  }
  return o;
}
function hdf(A, i, B, j) {
  var a = (A.hi[i] ^ B.hi[j]) | 0, b = (A.lo[i] ^ B.lo[j]) | 0;
  return POP[a & 0xffff] + POP[(a >>> 16) & 0xffff] + POP[b & 0xffff] + POP[(b >>> 16) & 0xffff];
}
function hd64(a, b) {   /* still used by the 64-bit silhouette occupancy code */
  var x = (a[0] ^ b[0]) | 0, y = (a[1] ^ b[1]) | 0;
  return POP[x & 0xffff] + POP[(x >>> 16) & 0xffff] + POP[y & 0xffff] + POP[(y >>> 16) & 0xffff];
}
function rot64(hi, lo, k) {
  k &= 63;
  if (k === 0) return [hi >>> 0, lo >>> 0];
  if (k === 32) return [lo >>> 0, hi >>> 0];
  if (k < 32) return [((hi << k) | (lo >>> (32 - k))) >>> 0, ((lo << k) | (hi >>> (32 - k))) >>> 0];
  var j = k - 32;
  return [((lo << j) | (hi >>> (32 - j))) >>> 0, ((hi << j) | (lo >>> (32 - j))) >>> 0];
}
/* Injective greedy: every fingerprint carries at most one match.  Without
   injectivity one busy host region answers for every guest region at once,
   which is exactly the "figure inside a figure" cheat.
   Distances are bounded by T (<= a few bits), so this counting-sorts by
   distance instead of materialising and sorting every candidate pair — on a
   degenerate bag the pair list is O(n^2) and the sort dominated the whole
   pairwise compare.  The wire is stored in (hi, lo) order, so ascending index
   order IS content order and the greedy pass stays record-derived rather than
   loop-derived. */
function matchBags(X, Y, T) {
  var n = X.n, m = Y.n, i, j, d;
  var dist = new Int8Array(n * m), cnt = new Int32Array(T + 2);
  for (i = 0; i < n; i++) {
    var base = i * m;
    for (j = 0; j < m; j++) {
      d = hdf(X, i, Y, j);
      if (d <= T) { dist[base + j] = d + 1; cnt[d]++; } else dist[base + j] = 0;
    }
  }
  var ux = new Uint8Array(n), uy = new Uint8Array(m), out = [];
  for (d = 0; d <= T; d++) {
    if (!cnt[d]) continue;
    for (i = 0; i < n; i++) {
      if (ux[i]) continue;
      var b = i * m;
      for (j = 0; j < m; j++) {
        if (uy[j] || dist[b + j] !== d + 1) continue;
        ux[i] = 1; uy[j] = 1; out.push({ i: i, j: j, d: d }); break;
      }
    }
  }
  return out;
}

/* How generic is each code inside its OWN bag?  A plain horizontal boundary
   produces the same code wherever it appears, in any artwork, so two unrelated
   pixel-art works share dozens of them.  A code that already matches eight
   regions of its own work is describing a TEXTURE, not an identity. */
function burst(X, T) {
  var c = new Int32Array(X.n);
  for (var i = 0; i < X.n; i++) { c[i] = 1;
    for (var j = 0; j < X.n; j++) if (j !== i && hdf(X, i, X, j) <= T) c[i]++; }
  return c;
}
/* Placement coherence.  A count of collisions cannot tell a paste from a
   coincidence: an impostor built from the same tileset collides just as often,
   only everywhere at once.  A paste agrees on ONE offset.
   Positions are now in the SAME aspect-true frame on both sides, so no
   per-work rescale is needed and v2's encode/decode mismatch is gone. */
function coherence(X, Y, hits, tol) {
  if (hits.length < 3) return { value: 0, measurable: false, inliers: 0 };
  var ratios = [], i, j;
  for (i = 0; i < hits.length; i++) for (j = i + 1; j < hits.length; j++) {
    var gx = X.x[hits[i].i] - X.x[hits[j].i], gy = X.y[hits[i].i] - X.y[hits[j].i];
    var hx = Y.x[hits[i].j] - Y.x[hits[j].j], hy = Y.y[hits[i].j] - Y.y[hits[j].j];
    var gd = gx * gx + gy * gy, hd = hx * hx + hy * hy;
    if (gd < 262144 || hd < 262144) continue;
    ratios.push(idiv(isqrt(hd) * 256, Math.max(1, isqrt(gd))));
  }
  if (!ratios.length) return { value: 0, measurable: false, inliers: 0 };
  ratios.sort(function (a, b) { return a - b; });
  var sig = ratios[ratios.length >> 1], best = 0, bdx = 0, bdy = 0;
  for (i = 0; i < hits.length; i++) {
    var ox = Y.x[hits[i].j] - ((sig * X.x[hits[i].i]) >> 8);
    var oy = Y.y[hits[i].j] - ((sig * X.y[hits[i].i]) >> 8), n = 0;
    for (j = 0; j < hits.length; j++) {
      var dx = Y.x[hits[j].j] - ((sig * X.x[hits[j].i]) >> 8) - ox;
      var dy = Y.y[hits[j].j] - ((sig * X.y[hits[j].i]) >> 8) - oy;
      if (dx * dx + dy * dy <= tol * tol) n++;
    }
    if (n > best) { best = n; bdx = ox; bdy = oy; }
  }
  /* A fraction alone is not evidence: with four matches, one placement explains
     all four by arithmetic rather than by agreement. */
  var frac = clamp(idiv(best * SCALE, hits.length), 0, SCALE);
  var conf = clamp(idiv(best * SCALE, 8), 0, SCALE);
  return { value: idiv(frac * conf, SCALE), measurable: best >= 4,
           fraction: frac, inliers: best, scale: sig, dx: bdx, dy: bdy };
}
function localChannel(A, B, T, o) {
  var X = readLocal(A), Y = readLocal(B);
  if (X.n < 4 || Y.n < 4)
    return abstain('too few distinctive regions on one side (' + X.n + '/' + Y.n + ')');
  var Cmax = Math.min(X.n, Y.n);
  var bx = burst(X, T), by = burst(Y, T);
  var hits = matchBags(X, Y, T), C = hits.length, W = 0, i;
  for (i = 0; i < hits.length; i++) W += idiv(SCALE, Math.max(bx[hits[i].i], by[hits[i].j]));
  function capacity(c) { var t = 0; for (var k = 0; k < c.length; k++) t += idiv(SCALE, c[k]); return t; }
  var cap = Math.max(1, Math.min(capacity(bx), capacity(by)));
  var E = 1 << 30, En = 0, fam = [16, 32, 48];
  for (var f = 0; f < fam.length; f++) {
    var Yr = rotBag(Y, fam[f]), byr = burst(Yr, T), hr = matchBags(X, Yr, T), wr = 0;
    for (var q = 0; q < hr.length; q++) wr += idiv(SCALE, Math.max(bx[hr[q].i], byr[hr[q].j]));
    if (wr < E) { E = wr; En = hr.length; }
  }
  /* BOTH evidence rules are computed and both reported.  They disagree on real
     works, and choosing between them is a decision about false-positive
     tolerance in a moderation queue, not a fact about the images. */
  var prior = SCALE, confAt = o.confidenceAt | 0;
  var purity = idiv(W * SCALE, W + E + prior), conf = clamp(idiv(C * SCALE, confAt), 0, SCALE);
  var lift = { raw: clamp(idiv(purity * conf, SCALE), 0, SCALE),
               ctl: clamp(idiv(idiv(E * SCALE, E + E + prior) * clamp(idiv(En * SCALE, confAt), 0, SCALE), SCALE), 0, SCALE) };
  var prop = { raw: clamp(idiv(W * SCALE, cap), 0, SCALE), ctl: clamp(idiv(E * SCALE, cap), 0, SCALE) };
  var use = o.evidence === 'proportion' ? prop : lift;
  var coh = coherence(X, Y, hits, 5500);
  return { value: chanceCorrect(use.raw, use.ctl), raw: use.raw, control: use.ctl,
           measurable: true, controlRan: true, matched: C, chance: En, of: Cmax,
           pairs: hits, coherence: coh, weightedMatch: W,
           both: { lift: chanceCorrect(lift.raw, lift.ctl), proportion: chanceCorrect(prop.raw, prop.ctl) },
           note: C + ' of ' + Cmax + ' regions collide, ' + En + ' expected by chance'
                 + (coh.measurable ? '; ' + coh.inliers + ' agree on one placement' : '') };
}
/* ---- 9.3c  shapes ------------------------------------------------- */
function readShapes(d) {
  var out = [];
  for (var i = 0; i < d.counts.shapes; i++) {
    var o = i * SHAPE_BYTES, s = d.shapes;
    out.push({ area: (s[o] | (s[o + 1] << 8) | (s[o + 2] << 16) | (s[o + 3] << 24)) >>> 0,
               per: s[o + 4] | (s[o + 5] << 8), aspect: s[o + 6] | (s[o + 7] << 8), holes: s[o + 8],
               radial: Array.prototype.slice.call(s.subarray(o + 9, o + 41)) });
  }
  return out;
}
function radialDiff(a, b, rot, flip) {
  var s = 0;
  for (var i = 0; i < 32; i++) { var j = flip ? (rot + 32 - i) & 31 : (rot + i) & 31;
    var d = a[i] - b[j]; s += d < 0 ? -d : d; }
  return idiv(s, 32);
}
function radialScore(ra, rb) {
  var all = [], rot, flip;
  for (flip = 0; flip < 2; flip++) for (rot = 0; rot < 32; rot++) all.push(radialDiff(ra, rb, rot, flip));
  var best = Math.min.apply(null, all);
  var sorted = all.slice().sort(function (x, y) { return x - y; });
  var med = sorted[sorted.length >> 1];
  return med <= 0 ? 0 : clamp(idiv((med - best) * SCALE, med), 0, SCALE);
}
function shapePair(a, b) {
  var v = radialScore(a.radial, b.radial);
  var ra = a.aspect || 1, rb = b.aspect || 1;
  var asp = clamp(idiv(Math.min(ra, rb) * SCALE, Math.max(ra, rb)), 0, SCALE);
  var ia = a.area ? idiv(a.per * a.per * 256, a.area) : 0, ib = b.area ? idiv(b.per * b.per * 256, b.area) : 0;
  var iso = (ia && ib) ? clamp(idiv(Math.min(ia, ib) * SCALE, Math.max(ia, ib)), 0, SCALE) : SCALE;
  var hol = (a.holes === b.holes) ? SCALE : clamp(SCALE - 2500 * Math.abs(a.holes - b.holes), 0, SCALE);
  return idiv(idiv(idiv(v * asp, SCALE) * iso, SCALE) * hol, SCALE);
}
function shapeChannel(A, B) {
  var X = readShapes(A), Y = readShapes(B);
  if (!X.length || !Y.length) return abstain('no regions stored on one side');
  var pairs = [];
  for (var i = 0; i < X.length; i++) for (var j = 0; j < Y.length; j++)
    pairs.push({ i: i, j: j, s: shapePair(X[i], Y[j]) });
  pairs.sort(function (a, b) { return b.s - a.s || (X[a.i].area + Y[a.j].area) - (X[b.i].area + Y[b.j].area); });
  var ux = {}, uy = {}, tot = 0, n = 0, kept = [];
  for (var p = 0; p < pairs.length; p++) { var q = pairs[p];
    if (ux[q.i] || uy[q.j]) continue; ux[q.i] = uy[q.j] = 1; tot += q.s; n++; kept.push(q); }
  var cover = Math.min(X.length, Y.length), v = idiv(tot, Math.max(1, cover));
  return { value: v, raw: v, control: 0, measurable: true, controlRan: true, pairs: kept,
           note: n + ' of ' + cover + ' regions paired by radial signature' };
}

/* ---- 9.3d  topology: BOTH endpoint encodings  (SPEC-003 §6.3) ----- */
function readRAG(d) {
  var out = [];
  for (var i = 0; i < d.counts.rag; i++) {
    var o = i * 6, s = d.rag;
    out.push({ qa: s[o], qb: s[o + 1], ra: s[o + 2], rb: s[o + 3], n: s[o + 4] | (s[o + 5] << 8) });
  }
  return out;
}
function ragRaw(X, Y, shift, useRank) {
  var tol = 16, inter = 0, tx = 0, ty = 0, i, j, used = {};
  var ka = useRank ? 'ra' : 'qa', kb = useRank ? 'rb' : 'qb';
  for (i = 0; i < X.length; i++) tx += X[i].n;
  for (j = 0; j < Y.length; j++) ty += Y[j].n;
  for (i = 0; i < X.length; i++) {
    var best = -1, bd = 1 << 30;
    for (j = 0; j < Y.length; j++) {
      if (used[j]) continue;
      var ya = (Y[j][ka] + shift) & 255, yb = (Y[j][kb] + shift) & 255;
      var lo = Math.min(ya, yb), hi = Math.max(ya, yb);
      var d = Math.abs(X[i][ka] - lo) + Math.abs(X[i][kb] - hi);
      if (d < bd) { bd = d; best = j; }
    }
    if (best >= 0 && bd <= 2 * tol) { used[best] = 1; inter += Math.min(X[i].n, Y[best].n); }
  }
  var uni = tx + ty - inter;
  return uni > 0 ? clamp(idiv(inter * SCALE, uni), 0, SCALE) : 0;
}
function topologyChannel(A, B, o) {
  var X = readRAG(A), Y = readRAG(B);
  if (X.length < 3 || Y.length < 3) return abstain('fewer than 3 colour boundaries on one side');
  var useRank = o.ragEndpoint === 'rank';
  function score(rank) {
    var raw = ragRaw(X, Y, 0, rank), ctl = SCALE;
    [85, 128, 171].forEach(function (s) { var v = ragRaw(X, Y, s, rank); if (v < ctl) ctl = v; });
    return { raw: raw, ctl: ctl, value: chanceCorrect(raw, ctl) };
  }
  var q = score(false), r = score(true), use = useRank ? r : q;
  /* Rank agreement is not a better score — it is a DIFFERENT finding.  Rank
     does not survive a rebuilt palette, so rank agreement means the palette
     was NOT rebuilt: same export rather than recolour.  v2 forced this choice
     at hash time; here it is a report. */
  return { value: use.value, raw: use.raw, control: use.ctl, measurable: true, controlRan: true,
           quantile: q.value, rank: r.value,
           sameExport: r.value >= q.value && r.value >= 3000,
           note: X.length + '/' + Y.length + ' boundary pairs; quantile ' + q.value + ', rank ' + r.value };
}

/* ---- 9.3e  run-length geometry ------------------------------------ */
function runRaw(A, B, rot) {
  var s = 0;
  for (var axis = 0; axis < 3; axis++) for (var i = 0; i < 16; i++) {
    var d = A.runs[axis * 16 + i] - B.runs[axis * 16 + ((i + rot) & 15)]; s += d < 0 ? -d : d;
  }
  return clamp(SCALE - idiv(s * SCALE, 3 * 510), 0, SCALE);
}
function runsFlat(X) {
  for (var ax = 0; ax < 3; ax++) {
    var tot = 0, top = 0;
    for (var i = 0; i < 16; i++) { var v = X.runs[ax * 16 + i]; tot += v; if (v > top) top = v; }
    if (tot > 0 && top * 100 < tot * 95) return false;
  }
  return true;
}
function runsChannel(A, B) {
  if (runsFlat(A) || runsFlat(B)) return abstain('one side has no run-length texture (a single run per axis)');
  var raw = runRaw(A, B, 0), ctl = SCALE;
  [4, 8, 12].forEach(function (r) { var v = runRaw(A, B, r); if (v < ctl) ctl = v; });
  return { value: chanceCorrect(raw, ctl), raw: raw, control: ctl, measurable: true, controlRan: true,
           note: 'stroke-length texture, H/V/diagonal' };
}

/* ---- 9.3f  identity palette --------------------------------------- */
function readPal(d) {
  var out = [];
  for (var i = 0; i < d.counts.palette; i++) {
    var o = i << 2, s = d.palette;
    out.push({ rank: s[o], freq: s[o + 1], lum: s[o + 2], q: s[o + 3] });
  }
  return out;
}
function palRaw(X, Y, shift) {
  var used = {}, hit = 0, tot = 0, i, j;
  for (i = 0; i < X.length; i++) tot += X[i].freq;
  for (i = 0; i < X.length; i++) {
    var best = -1, bd = 1 << 30;
    for (j = 0; j < Y.length; j++) {
      if (used[j]) continue;
      var d = Math.abs(X[i].q - ((Y[j].q + shift) & 255)) * 2 + Math.abs(X[i].freq - Y[j].freq);
      if (d < bd) { bd = d; best = j; }
    }
    if (best >= 0 && bd <= 96) { used[best] = 1; hit += X[i].freq; }
  }
  return tot ? clamp(idiv(hit * SCALE, tot), 0, SCALE) : 0;
}
function paletteChannel(A, B) {
  var X = readPal(A), Y = readPal(B);
  if (X.length < 2 || Y.length < 2) return abstain('fewer than 2 palette entries on one side');
  /* An inverted palette REFLECTS every quantile, q -> 255-q.  Try it as a second
     hypothesis and take whichever beats its own chance floor by more. */
  var Yi = Y.map(function (e) { return { rank: e.rank, freq: e.freq, lum: 255 - e.lum, q: 255 - e.q }; });
  function pass(Z) {
    var raw = Math.min(palRaw(X, Z, 0), palRaw(Z, X, 0)), ctl = SCALE;
    [85, 128, 171].forEach(function (s) { var v = Math.min(palRaw(X, Z, s), palRaw(Z, X, s)); if (v < ctl) ctl = v; });
    return { raw: raw, ctl: ctl, value: chanceCorrect(raw, ctl) };
  }
  var d = pass(Y), n = pass(Yi), use = n.value > d.value ? n : d;
  return { value: use.value, raw: use.raw, control: use.ctl, measurable: true, controlRan: true,
           inverted: n.value > d.value,
           note: 'share of one palette explained by the other, absolute RGB discarded'
                 + (n.value > d.value ? ' (inverted)' : '') };
}

/* ---- 9.3g  silhouette — abstains by design  (SPEC-003 §6.4) ------- */
function silhouetteChannel(A, B) {
  if (!(A.flags & F_SIL) || !(B.flags & F_SIL))
    return abstain('one side has no silhouette (opaque canvas, or nothing but backdrop)');
  var a = A.silhouette, b = B.silhouette;
  var ra = Array.prototype.slice.call(a.subarray(0, 32)), rb = Array.prototype.slice.call(b.subarray(0, 32));
  var v = radialScore(ra, rb);
  var aspA = a[36] | (a[37] << 8), aspB = b[36] | (b[37] << 8);
  var asp = clamp(idiv(Math.min(aspA, aspB) * SCALE, Math.max(1, Math.max(aspA, aspB))), 0, SCALE);
  var fill = clamp(SCALE - Math.abs(a[38] - b[38]) * 40, 0, SCALE);
  var occA = [(a[56] | (a[57] << 8) | (a[58] << 16) | (a[59] << 24)) >>> 0,
              (a[60] | (a[61] << 8) | (a[62] << 16) | (a[63] << 24)) >>> 0];
  var occB = [(b[56] | (b[57] << 8) | (b[58] << 16) | (b[59] << 24)) >>> 0,
              (b[60] | (b[61] << 8) | (b[62] << 16) | (b[63] << 24)) >>> 0];
  var occ = clamp(SCALE - idiv(hd64(occA, occB) * SCALE, 32), 0, SCALE);
  var mom = 0;
  for (var i = 32; i < 35; i++) { var d = a[i] - b[i]; mom += d < 0 ? -d : d; }
  var momS = clamp(SCALE - mom * 60, 0, SCALE);
  var raw = idiv(idiv(idiv(idiv(v * asp, SCALE) * fill, SCALE) * occ, SCALE) * momS, SCALE);
  /* control: the same occupancy code against its own rotations — a silhouette
     that agrees with every rotation of itself is a blob, not an outline */
  var ctl = SCALE;
  [16, 32, 48].forEach(function (k) {
    var r = rot64(occB[0], occB[1], k);
    var c = clamp(SCALE - idiv(hd64(occA, r) * SCALE, 32), 0, SCALE);
    if (c < ctl) ctl = c;
  });
  ctl = idiv(idiv(idiv(idiv(v * asp, SCALE) * fill, SCALE) * ctl, SCALE) * momS, SCALE);
  return { value: chanceCorrect(raw, ctl), raw: raw, control: ctl, measurable: true, controlRan: true,
           radial: v, occupancy: occ, note: 'outline shape — strong when present, silent when not' };
}

/* ---- 9.5 reporting only: palette relationship  (SPEC-003 §6.5) ---- *
 * MUST NOT contribute to any score.  Zeroing the colour section MUST NOT
 * change a verdict.  It exists so a moderator can be told whether the copy
 * was recoloured, which is a materially different case. */
function paletteRelation(A, B) {
  var n = Math.min(A.counts.colour, B.counts.colour);
  if (n < 4) return { relation: 'unknown', exact: 0, of: 0 };
  var used = {}, exact = 0;
  for (var i = 0; i < A.counts.colour; i++) {
    var best = -1, bd = 1 << 30;
    for (var j = 0; j < B.counts.colour; j++) {
      if (used[j]) continue;
      var d = Math.abs(A.colour[i * 5] - B.colour[j * 5]) + Math.abs(A.colour[i * 5 + 1] - B.colour[j * 5 + 1]) +
              Math.abs(A.colour[i * 5 + 2] - B.colour[j * 5 + 2]);
      if (d < bd) { bd = d; best = j; }
    }
    if (best >= 0 && bd <= 24) { used[best] = 1; exact++; }
  }
  var share = idiv(exact * 100, Math.max(1, Math.min(A.counts.colour, B.counts.colour)));
  return { relation: share >= 80 ? 'identical palette' : share >= 40 ? 'related palette' : 'rebuilt palette',
           exact: exact, of: Math.min(A.counts.colour, B.counts.colour), share: share };
}

/* ================================================================== *
 * 9.4  THE GEOMETRIC CHANNEL
 *
 * Correspondences -> Hough vote -> fixed-point refit -> permutation null.
 * No RANSAC, no PRNG, no float.  The vote is O(n) where 900 RANSAC
 * hypotheses were O(900n), and it USES the scale and orientation each
 * keypoint already carries, which RANSAC throws away.
 * ================================================================== */

var HAM_MAX = 88, LOWE_NUM = 82, LOWE_DEN = 100;
var SCALE_Q16 = Int32Array.of(
  2813, 3657, 4754, 6180, 8034, 10444, 13578, 17651, 22946, 29830, 38779, 50412, 65536, 85197,
  110756, 143983, 187177, 243331, 316330, 411229, 534597, 694976, 903469, 1174510, 1526863);   /* 1.3^d * 65536, d = -12..12 */
function hamDesc(a, b) {
  var d = 0;
  for (var k = 0; k < 8; k++) d += popcnt32((a[k] ^ b[k]) >>> 0);
  return d;
}
function mirrorSide(list, xmax) {
  return list.map(function (k) {
    return { desc: mirrorDesc(k.desc), x: xmax - k.x, y: k.y,
             level: k.level, sec: (32 - k.sec) & 63, s: k.s };
  });
}
/* Inverting a work's luminance flips every BRIEF bit AND moves the sector to
   its antipode — and the second effect defeats the first: the steered pattern
   then lands on the OPPOSITE side of the keypoint and samples different
   pixels.  Measured mean Hamming between a descriptor and the complement of
   its inverted twin: 129.9 of 256, i.e. chance.  Kept only for reference; a
   working version needs a centrally symmetric pattern, which costs descriptor
   distinctiveness rather than bytes. */
function invertSide(list) {
  return list.map(function (k) {
    var d = new Uint32Array(8);
    for (var i = 0; i < 8; i++) d[i] = (~k.desc[i]) >>> 0;
    return { desc: d, x: k.x, y: k.y, level: k.level, sec: k.sec ^ 32, s: k.s };
  });
}

/* two-sided Lowe + mutual nearest neighbour.  v2's L2 applied the ratio test
   from one side only, which is why match(A,B) != match(B,A) there. */
/* Descriptors are packed into ONE Int32Array of eight-word lanes so the
   O(nA*nB) Hamming sweep walks contiguous memory instead of chasing 256
   separate Uint32Arrays through the heap, and the eight words are unrolled.
   This is the inner loop of the entire compare: ~1M popcounts at full caps. */
function packDesc(list) {
  var n = list.length | 0, out = new Int32Array(n << 3);
  for (var i = 0; i < n; i++) {
    var d = list[i].desc, o = i << 3;
    out[o] = d[0] | 0; out[o + 1] = d[1] | 0; out[o + 2] = d[2] | 0; out[o + 3] = d[3] | 0;
    out[o + 4] = d[4] | 0; out[o + 5] = d[5] | 0; out[o + 6] = d[6] | 0; out[o + 7] = d[7] | 0;
  }
  return out;
}
function correspond(A, B) {
  var na = A.length | 0, nb = B.length | 0, i, j;
  if (!na || !nb) return [];
  var PA = packDesc(A), PB = packDesc(B);
  var aBest = new Int32Array(na).fill(-1), aD1 = new Int32Array(na).fill(999), aD2 = new Int32Array(na).fill(999);
  var bBest = new Int32Array(nb).fill(-1), bD1 = new Int32Array(nb).fill(999), bD2 = new Int32Array(nb).fill(999);
  for (i = 0; i < na; i++) {
    var ao = i << 3;
    var a0 = PA[ao] | 0, a1 = PA[ao + 1] | 0, a2 = PA[ao + 2] | 0, a3 = PA[ao + 3] | 0;
    var a4 = PA[ao + 4] | 0, a5 = PA[ao + 5] | 0, a6 = PA[ao + 6] | 0, a7 = PA[ao + 7] | 0;
    var d1 = 999, d2 = 999, bi = -1;
    for (j = 0; j < nb; j++) {
      var bo = j << 3, x = 0, d = 0;
      x = (a0 ^ PB[bo])     | 0; d  = POP[x & 0xffff] + POP[(x >>> 16) & 0xffff];
      x = (a1 ^ PB[bo + 1]) | 0; d += POP[x & 0xffff] + POP[(x >>> 16) & 0xffff];
      x = (a2 ^ PB[bo + 2]) | 0; d += POP[x & 0xffff] + POP[(x >>> 16) & 0xffff];
      x = (a3 ^ PB[bo + 3]) | 0; d += POP[x & 0xffff] + POP[(x >>> 16) & 0xffff];
      x = (a4 ^ PB[bo + 4]) | 0; d += POP[x & 0xffff] + POP[(x >>> 16) & 0xffff];
      x = (a5 ^ PB[bo + 5]) | 0; d += POP[x & 0xffff] + POP[(x >>> 16) & 0xffff];
      x = (a6 ^ PB[bo + 6]) | 0; d += POP[x & 0xffff] + POP[(x >>> 16) & 0xffff];
      x = (a7 ^ PB[bo + 7]) | 0; d += POP[x & 0xffff] + POP[(x >>> 16) & 0xffff];
      if (d < d1) { d2 = d1; d1 = d; bi = j; } else if (d < d2) d2 = d;
      if (d < bD1[j]) { bD2[j] = bD1[j]; bD1[j] = d; bBest[j] = i; } else if (d < bD2[j]) bD2[j] = d;
    }
    aD1[i] = d1; aD2[i] = d2; aBest[i] = bi;
  }
  var out = [];
  for (i = 0; i < na; i++) {
    j = aBest[i];
    if (j < 0 || aD1[i] > HAM_MAX) continue;
    if (aD1[i] * LOWE_DEN >= LOWE_NUM * aD2[i]) continue;
    if (bD1[j] * LOWE_DEN >= LOWE_NUM * bD2[j]) continue;
    if (bBest[j] !== i) continue;
    out.push({ i: i, j: j, d: aD1[i] });
  }
  out.sort(function (a, b) { return a.d - b.d || a.i - b.i || a.j - b.j; });
  return out;
}

var TBIN_W = 4096, TBIN_N = 64, TBIN_OFF = 131072;
/* nearest ladder bin to a Q16 scale ratio, by integer comparison */
function scaleBin(q16) {
  var best = 0, bd = 1 << 30;
  for (var d = 0; d < 25; d++) {
    var e = q16 - SCALE_Q16[d]; if (e < 0) e = -e;
    if (e < bd) { bd = e; best = d; }
  }
  return best;
}
/* `>>` coerces to int32 first, and these products reach ~1e12 (sq * rx in the
   Hough vote) or pass 2^31 (r00 * ax once the recovered scale exceeds ~4x), so
   a shift here wraps silently.  Floor-divide exactly instead — this is the same
   operation Rust's arithmetic `>> 16` on a 64-bit value performs. */
function shr16(v) { return Math.floor(v / 65536); }
function voteCells(A, B, c, mdA, mdB) {
  var a = A[c.i], b = B[c.j];
  /* scale = L_kA / L_kB, NOT 1.3^(kB-kA).  See levelDim(). */
  var LA = levelDim(mdA, a.level), LB = levelDim(mdB, b.level);
  var sq = clamp(idiv(LA * 65536, Math.max(1, LB)), 1, 1 << 24);
  var dl = scaleBin(sq);
  var ds = (b.sec - a.sec) & 63;
  var rx = sh10(a.x * RC10[ds] - a.y * RS10[ds]);
  var ry = sh10(a.x * RS10[ds] + a.y * RC10[ds]);
  var tx = b.x - shr16(sq * rx), ty = b.y - shr16(sq * ry);
  if (sq > (SCALE_Q16[24] << 1) || sq < (SCALE_Q16[0] >> 1)) return { cells: [], dl: dl, ds: ds, tx: 0, ty: 0, sq: sq };
  var xb = clamp(idiv(tx + TBIN_OFF, TBIN_W), 0, TBIN_N - 1);
  var yb = clamp(idiv(ty + TBIN_OFF, TBIN_W), 0, TBIN_N - 1);
  var rb = ds >> 2;
  /* nearest-two on rotation and on each translation axis: a correspondence
     sitting on a bin boundary must not be lost (Lowe's soft binning) */
  var xs = [xb], ys = [yb], rs = [rb];
  if (((tx + TBIN_OFF) - xb * TBIN_W) * 2 >= TBIN_W) { if (xb + 1 < TBIN_N) xs.push(xb + 1); }
  else if (xb > 0) xs.push(xb - 1);
  if (((ty + TBIN_OFF) - yb * TBIN_W) * 2 >= TBIN_W) { if (yb + 1 < TBIN_N) ys.push(yb + 1); }
  else if (yb > 0) ys.push(yb - 1);
  if ((ds & 3) >= 2) rs.push((rb + 1) & 15); else rs.push((rb + 15) & 15);
  var cells = [];
  for (var a1 = 0; a1 < rs.length; a1++) for (var b1 = 0; b1 < xs.length; b1++) for (var c1 = 0; c1 < ys.length; c1++)
    cells.push(((dl * 16 + rs[a1]) * TBIN_N + xs[b1]) * TBIN_N + ys[c1]);
  return { cells: cells, dl: dl, ds: ds, tx: tx, ty: ty, sq: sq };
}

/* closed-form 4-DOF similarity by fixed-point least squares.  This is a
   LINEAR SOLVE, not an optimisation.  Coordinates drop to 12 bits so every
   product stays under 2^48 — inside the 2^53 a double holds exactly, and
   inside an i64 in Rust. */
function fitSimilarity(A, B, corr) {
  var n = corr.length, i;
  if (n < 2) return null;
  var sax = 0, say = 0, sbx = 0, sby = 0;
  for (i = 0; i < n; i++) {
    sax += A[corr[i].i].x >> 4; say += A[corr[i].i].y >> 4;
    sbx += B[corr[i].j].x >> 4; sby += B[corr[i].j].y >> 4;
  }
  var ax0 = idiv(sax, n), ay0 = idiv(say, n), bx0 = idiv(sbx, n), by0 = idiv(sby, n);
  var Sx = 0, Sy = 0, sig = 0;
  for (i = 0; i < n; i++) {
    var ax = (A[corr[i].i].x >> 4) - ax0, ay = (A[corr[i].i].y >> 4) - ay0;
    var bx = (B[corr[i].j].x >> 4) - bx0, by = (B[corr[i].j].y >> 4) - by0;
    Sx += ax * bx + ay * by; Sy += ax * by - ay * bx; sig += ax * ax + ay * ay;
  }
  if (sig === 0) return null;
  var r00 = idiv(Sx * 65536, sig), r10 = idiv(Sy * 65536, sig);
  var tx = bx0 - shr16(r00 * ax0 - r10 * ay0);
  var ty = by0 - shr16(r10 * ax0 + r00 * ay0);
  return { r00: r00, r10: r10, tx: tx, ty: ty };
}
function countInliers(A, B, corr, M, tol2) {
  var mask = new Array(corr.length), n = 0;
  for (var i = 0; i < corr.length; i++) {
    var ax = A[corr[i].i].x >> 4, ay = A[corr[i].i].y >> 4;
    var mx = shr16(M.r00 * ax - M.r10 * ay) + M.tx;
    var my = shr16(M.r10 * ax + M.r00 * ay) + M.ty;
    var dx = mx - (B[corr[i].j].x >> 4), dy = my - (B[corr[i].j].y >> 4);
    var ok = dx * dx + dy * dy <= tol2;
    mask[i] = ok; if (ok) n++;
  }
  return { mask: mask, n: n };
}

function houghVerify(A, B, corr, o, mdA, mdB) {
  if (corr.length < o.geoMinCorr) return { inliers: 0, mask: [], model: null, cell: null };
  var votes = new Map(), per = [], i;
  for (i = 0; i < corr.length; i++) {
    var v = voteCells(A, B, corr[i], mdA, mdB); per.push(v);
    for (var c = 0; c < v.cells.length; c++) votes.set(v.cells[c], (votes.get(v.cells[c]) || 0) + 1);
  }
  var bestKey = -1, bestN = 0;
  votes.forEach(function (n, k) { if (n > bestN || (n === bestN && k < bestKey)) { bestN = n; bestKey = k; } });
  if (bestN < 3) return { inliers: 0, mask: corr.map(function () { return false; }), model: null, cell: null };
  var members = [];
  for (i = 0; i < corr.length; i++) if (per[i].cells.indexOf(bestKey) >= 0) members.push(corr[i]);

  var tol = Math.max(2, o.geoEps >> 4), tol2 = tol * tol;
  var M = fitSimilarity(A, B, members);
  if (!M) return { inliers: 0, mask: corr.map(function () { return false; }), model: null, cell: bestKey };
  /* one loose pass to gather, one refit, one tight count */
  var r = countInliers(A, B, corr, M, tol2 * 9 / 4 | 0);
  var inl = []; for (i = 0; i < corr.length; i++) if (r.mask[i]) inl.push(corr[i]);
  if (inl.length >= 2) { var M2 = fitSimilarity(A, B, inl); if (M2) M = M2; }
  r = countInliers(A, B, corr, M, tol2);
  var sQ16 = isqrt(M.r00 * M.r00 + M.r10 * M.r10);
  return { inliers: r.n, mask: r.mask, model: M, cell: bestKey, scaleQ16: sQ16,
           rotSectors: per.length ? per[0].ds : 0 };
}

function geometricChannel(A2, B2, o, ctx) {
  var mdA = (ctx && ctx.mdA) || 512, mdB = (ctx && ctx.mdB) || 512;
  if (!o.geoEnabled) return abstain('geometric channel disabled');
  if (!A2 || !B2 || A2.length < o.geoMinCorr || B2.length < o.geoMinCorr)
    return abstain('too few keypoints on one side (' + (A2 ? A2.length : 0) + '/' + (B2 ? B2.length : 0) + ')');

  function run(Aside, label) {
    var corr = correspond(Aside, B2);
    if (corr.length < o.geoMinCorr) return { inliers: 0, corr: corr, mask: [], model: null, label: label, ctl: 0 };
    var v = houghVerify(Aside, B2, corr, o, mdA, mdB);
    /* THE NULL.  Keep the correspondence set exactly as matched — same count,
       same distinctiveness — and permute only which B keypoint's GEOMETRY each
       A keypoint is paired with.  That measures precisely the right thing:
       would these matches have agreed on one placement by chance? */
    var ctl = 0, n = corr.length;
    [n >> 1, idiv(n, 3) || 1].forEach(function (p) {
      if (p <= 0 || p >= n) return;
      var permuted = corr.map(function (c, k) { return { i: c.i, j: corr[(k + p) % n].j, d: c.d }; });
      var pv = houghVerify(Aside, B2, permuted, o, mdA, mdB);
      if (pv.inliers > ctl) ctl = pv.inliers;
    });
    return { inliers: v.inliers, corr: corr, mask: v.mask, model: v.model,
             scaleQ16: v.scaleQ16, label: label, ctl: ctl };
  }

  /* Four hypotheses, all derived from the SAME stored bits: direct, mirrored,
     inverted, and both.  None of them costs a byte on the wire. */
  var direct = run(A2, 'direct'), best = direct, mirrored = null;
  var mir = (o.mirrorHypothesis && ctx && ctx.xmaxA !== undefined) ? mirrorSide(A2, ctx.xmaxA) : null;
  if (mir) { mirrored = run(mir, 'mirrored'); if (mirrored.inliers > best.inliers) best = mirrored; }
  /* No inversion hypothesis here: see invertSide()'s note.  Inversion is
     carried by the DCT sign flip, the palette quantile reflection and the
     local complement fold, all of which DO hold exactly. */
  var raw = clamp(idiv(best.inliers * SCALE, o.geoConfAt), 0, SCALE);
  var ctl = clamp(idiv(best.ctl * SCALE, o.geoConfAt), 0, SCALE);
  return { value: chanceCorrect(raw, ctl), raw: raw, control: ctl, measurable: true, controlRan: true,
           inliers: best.inliers, chanceInliers: best.ctl, accepted: best.corr.length,
           mirrored: best.label.indexOf('mirrored') >= 0, inverted: best.label.indexOf('inverted') >= 0,
           hypothesis: best.label, model: best.model, mask: best.mask, pairs: best.corr,
           scale: best.scaleQ16 ? best.scaleQ16 / 65536 : null,
           direct: direct.inliers, mirrorTried: mirrored ? mirrored.inliers : null,
           note: best.inliers + ' of ' + best.corr.length + ' correspondences agree on one similarity, '
                 + best.ctl + ' by chance' + (best.label !== 'direct' ? ' (' + best.label + ')' : '') };
}

/* ================================================================== *
 * compare()  —  channels, then the verdict LATTICE (SPEC-003 §9.5)
 * ================================================================== */
function compare(a, b, opts) {
  var o = {}; for (var k in DEFAULTS) o[k] = DEFAULTS[k];
  if (opts) for (k in opts) if (opts[k] !== undefined) o[k] = opts[k];

  function side(x) {
    if (!x) throw new TypeError('paph3: compare needs two fingerprints');
    if (x.tier === 1 && x.dct) return x;
    if (x.t1) { var p = parseT1(x.t1); p.t1 = x.t1; p.t2 = x.t2; return p; }
    return parseT1(x);
  }
  var A = side(a), B = side(b);
  var A2 = (a && a.t2) ? a.t2 : (a && a.tier === 1 && a.t2) ? a.t2 : null;
  var B2 = (b && b.t2) ? b.t2 : null;

  /* P4 — canonical argument order.  compare(x,y) MUST equal compare(y,x).
     Sort the two wires once and swap the directional outputs back; fixing each
     site separately is a promise that has to be re-kept with every new
     channel, and the geometric channel is the most directional thing here. */
  var swapped = false;
  for (var si = 0; si < A.bytes.length; si++) {
    if (A.bytes[si] !== B.bytes[si]) { swapped = A.bytes[si] > B.bytes[si]; break; }
  }
  if (swapped) { var t_ = A; A = B; B = t_; var u_ = A2; A2 = B2; B2 = u_; }

  var same = A.bytes.length === B.bytes.length;
  if (same) for (var i = 0; i < A.bytes.length; i++) if (A.bytes[i] !== B.bytes[i]) { same = false; break; }

  var kpA = A2 ? parseT2(A2).list : readSketch(A);
  var kpB = B2 ? parseT2(B2).list : readSketch(B);
  var xmaxA = A2 ? parseT2(A2).xmax : clamp(idiv((A.width - 1) * 65535, A.maxDim), 0, 65535);

  var ch = {
    dct:        dctChannel(A, B),
    local:      localChannel(A, B, o.hammingT, o),
    shape:      shapeChannel(A, B),
    topology:   topologyChannel(A, B, o),
    runs:       runsChannel(A, B),
    palette:    paletteChannel(A, B),
    silhouette: silhouetteChannel(A, B)
  };
  var geo = geometricChannel(kpA, kpB, o, { xmaxA: xmaxA,
      mdA: A2 ? parseT2(A2).maxDim : A.maxDim, mdB: B2 ? parseT2(B2).maxDim : B.maxDim });

  if (swapped) {
    var co = ch.local.coherence;
    if (co && co.measurable) { co.dx = -co.dx; co.dy = -co.dy; if (co.scale) co.scale = idiv(65536, co.scale); }
    if (ch.local.pairs) ch.local.pairs = ch.local.pairs.map(function (q) { return { i: q.j, j: q.i, d: q.d }; });
    if (geo.pairs) geo.pairs = geo.pairs.map(function (q) { return { i: q.j, j: q.i, d: q.d }; });
    ch.dct.transform = DIHEDRAL[D4_INVERSE[ch.dct.t]];
    if (geo.model) { geo.scale = geo.scale ? 1 / geo.scale : null; }
  }

  /* ---- structural score: both rules always computed ---- */
  var wsum = 0, wtot = 0, measured = [], secondary = [];
  for (var name in ch) {
    var c = ch[name];
    if (!c.measurable) continue;
    measured.push(name);
    wsum += c.value * WEIGHTS[name]; wtot += WEIGHTS[name];
    if (name !== 'local') secondary.push({ n: name, v: c.value });
  }
  var weighted = wtot ? idiv(wsum, wtot) : 0;
  secondary.sort(function (x, y) { return x.v - y.v || (x.n < y.n ? -1 : 1); });
  var corrob = secondary.length ? secondary[secondary.length >> 1] : null;
  var evidence = ch.local.measurable ? { n: 'local', v: ch.local.value } : null;
  var cohv = ch.local.measurable && ch.local.coherence && ch.local.coherence.measurable
           ? ch.local.coherence.value : 0;
  if (corrob && cohv > corrob.v) corrob = { n: 'placement', v: cohv };
  var gateSrc = evidence && corrob ? (evidence.v < corrob.v ? evidence : corrob) : (evidence || corrob);
  var gate = gateSrc ? gateSrc.v : 0;
  var structural = o.scoring === 'weighted' ? weighted : gate;
  /* Corroboration means INDEPENDENT channels agreeing.  With fewer than three
     measurable secondaries the median degenerates to a single channel, and the
     structural side must not be allowed to certify on it: two flat canvases
     certified as Copy through the shape channel alone until this existed.
     Evidence is likewise required — there is nothing to corroborate without it. */
  var structuralCertifiable = !!evidence && secondary.length >= 3;

  /* ---- the lattice.  Do NOT average the two scores: they measure different
     things, they fail differently, and both were measured at precision 1.00
     on their own.  A weighted sum destroys exactly that. ---- */
  var gv = geo.measurable ? geo.value : 0, gi = geo.measurable ? geo.inliers : 0;
  var geoStrong = geo.measurable && gv >= THRESH.GEO_STRONG;
  var geoWeak   = geo.measurable && gv >= THRESH.GEO_WEAK;
  var sStrong = structuralCertifiable && structural >= THRESH.STRUCT_STRONG;
  var sMod    = structuralCertifiable && structural >= THRESH.STRUCT_MODERATE;
  var sRel    = structural >= THRESH.STRUCT_WEAK;

  var verdict, klass, basis = [];
  if (same) { verdict = 'Identical'; klass = 'byte-identical fingerprint'; basis = ['bytes']; }
  else if (sStrong && geoStrong) { verdict = 'Copy'; klass = 'certified — structure and geometry agree'; basis = ['structural', 'geometric']; }
  else if (sStrong && structural >= THRESH.STRUCT_SOLO) {
    verdict = 'Copy';
    var pr = paletteRelation(A, B).relation;
    klass = (ch.dct.measurable && ch.dct.value >= 3000 && ch.dct.transform !== 'Identity')
          ? 'structural only — ' + ch.dct.transform.toLowerCase() + ' class'
          : (ch.dct.measurable && ch.dct.value >= 3000 && ch.dct.inverted) ? 'structural only — inversion class'
          : (pr === 'rebuilt palette' || pr === 'related palette') ? 'structural only — recolour class'
          : 'structural only — no geometric corroboration';
    basis = ['structural'];
  }
  else if (geoStrong && gi >= THRESH.GEO_SOLO_INLIERS) {
    verdict = 'Copy'; klass = geo.mirrored ? 'geometric only — mirrored crop / collage class'
                                           : 'geometric only — crop / collage class';
    basis = ['geometric'];
  }
  else if ((sStrong || sMod) || geoStrong || geoWeak) {
    verdict = 'Suspected';
    klass = geoStrong ? 'geometry agrees but below the solo bar'
          : sStrong   ? 'structure agrees but below the solo bar'
          : 'partial agreement';
    basis = (sMod ? ['structural'] : []).concat(geoWeak ? ['geometric'] : []);
  }
  else if (sRel) { verdict = 'Related'; klass = 'same family, not a copy'; basis = []; }
  else { verdict = 'Unrelated'; klass = 'no agreement above chance'; basis = []; }

  if (same) { structural = SCALE; weighted = SCALE; gate = SCALE; }

  return {
    verdict: verdict, class: klass, basis: basis,
    structural: structural, geometric: gv, weighted: weighted, gate: gate,
    channels: ch, geo: geo, measured: measured,
    abstained: Object.keys(ch).filter(function (n) { return !ch[n].measurable; })
                              .concat(geo.measurable ? [] : ['geometric']),
    report: {
      dihedral: ch.dct.measurable ? ch.dct.transform : null,
      mirrored: geo.measurable ? geo.mirrored : null,
      geoHypothesis: geo.measurable ? geo.hypothesis : null,
      recoveredScale: geo.measurable ? geo.scale : null,
      inliers: gi, chanceInliers: geo.measurable ? geo.chanceInliers : null,
      palette: paletteRelation(A, B),
      brightness: brightnessRelation(A, B),
      sameExport: ch.topology.measurable ? ch.topology.sameExport : null,
      evidenceBoth: ch.local.measurable ? ch.local.both : null
    },
    identical: same, scoring: o.scoring, thresholds: THRESH,
    structuralCertifiable: structuralCertifiable, swapped: swapped
  };
}

/* ================================================================== *
 * PUBLIC API
 *
 * Two shapes, same engine:
 *   functional  — paph3.hash(img, opts), paph3.compare(a, b, opts)
 *   class       — new Paph(config).hash(img) / .compare(a, b)
 *
 * A Config is frozen on construction: the whole point of SPEC-003 P1 is that
 * these are COMPARE-time choices, so a config is a value you pass around and
 * derive from, not a mutable bag that drifts between two call sites.
 * ================================================================== */

function Config(over) {
  var k;
  for (k in DEFAULTS) this[k] = DEFAULTS[k];
  if (over) for (k in over) {
    if (!Object.prototype.hasOwnProperty.call(DEFAULTS, k))
      throw new RangeError('paph3: unknown config key "' + k + '"');
    if (over[k] !== undefined) this[k] = over[k];
  }
  var e = Config.validate(this);
  if (e) throw new RangeError('paph3: ' + e);
  if (Object.freeze) Object.freeze(this);
}
Config.prototype.with = function (over) {
  var m = {}, k;
  for (k in DEFAULTS) m[k] = this[k];
  if (over) for (k in over) m[k] = over[k];
  return new Config(m);
};
Config.prototype.toJSON = function () {
  var m = {}; for (var k in DEFAULTS) m[k] = this[k]; return m;
};
Config.defaults = function () { return new Config(); };
Config.validate = function (c) {
  if (['gate', 'weighted'].indexOf(c.scoring) < 0) return 'scoring must be gate or weighted';
  if (['lift', 'proportion'].indexOf(c.evidence) < 0) return 'evidence must be lift or proportion';
  if (['quantile', 'rank'].indexOf(c.ragEndpoint) < 0) return 'ragEndpoint must be quantile or rank';
  if (!(c.hammingT >= 0 && c.hammingT <= 64)) return 'hammingT out of range';
  if (!(c.confidenceAt >= 1)) return 'confidenceAt must be at least 1';
  if (!(c.geoConfAt >= 1)) return 'geoConfAt must be at least 1';
  if (!(c.geoEps >= 1 && c.geoEps <= 32767)) return 'geoEps out of range';
  if (!(c.localCount >= 4 && c.localCount <= 128)) return 'localCount out of range (wire holds 128)';
  if (!(c.kpCount >= 0 && c.kpCount <= MAX_KP_COUNT)) return 'kpCount out of range (tier 2 holds 512)';
  if (!(c.kpSelect === 0 || c.kpSelect === 1)) return 'kpSelect must be 0 (4.1) or 1 (4.2)';
  if (!(c.sketchCount >= 0 && c.sketchCount <= 32)) return 'sketchCount out of range (tier 1 holds 32)';
  if (!(c.geoMinCorr >= 2 && c.geoMinCorr <= 64)) return 'geoMinCorr out of range';
  return null;
};

function Paph(config) {
  this.config = (config instanceof Config) ? config : new Config(config);
}
Paph.prototype.hash = function (a, b, c) {
  return hash(a, b, c, this.config);
};
Paph.prototype.compare = function (a, b) {
  return compare(a, b, this.config);
};
Paph.prototype.with = function (over) { return new Paph(this.config.with(over)); };
Paph.prototype.parseTier1 = function (bytes) { return parseT1(bytes); };
Paph.prototype.parseTier2 = function (bytes) { return parseT2(bytes); };
Paph.prototype.backend = 'js';

/* frozen snapshot of the shipping defaults, for callers that want the values
   without constructing anything */
var DEFAULT_CONFIG = (function () {
  var m = {}; for (var k in DEFAULTS) m[k] = DEFAULTS[k];
  return Object.freeze ? Object.freeze(m) : m;
})();

var paph3 = {
  VERSION: VERSION, T1_BYTES: T1_BYTES, KP_REC: KP_REC, KP_MAX: MAX_KP_COUNT,
  F_KPQ: F_KPQ, selectGrid: selectGrid, selectQuality: selectQuality,
  SECTIONS: SECTIONS, SECTION_OFFSETS: SEC_OFF,
  DEFAULTS: DEFAULTS, DEFAULT_CONFIG: DEFAULT_CONFIG,
  THRESHOLDS: THRESH, WEIGHTS: WEIGHTS,
  Config: Config, Paph: Paph, backend: 'js',
  hash: hash, compare: compare,
  serializeT1: serializeT1, parseT1: parseT1, serializeT2: serializeT2, parseT2: parseT2,
  _internal: { indexImage: indexImage, foldMatte: foldMatte, upscaleFactor: upscaleFactor,
               readImage: readImage, keypoints: keypoints, describe: describe,
               orientSector: orientSector, mirrorDesc: mirrorDesc, correspond: correspond,
               houghVerify: houghVerify, fitSimilarity: fitSimilarity, crc32: crc32,
               integralI32: integralI32, boxWin: boxWin, PATTERN: PATTERN,
               COS64: COS64, SIN64: SIN64, RC10: RC10, RS10: RS10, hamDesc: hamDesc,
               silhouetteSignature: silhouetteSignature, readLocal: readLocal,
               /* v4 (SPEC-004) reads these; visibility only, behaviour untouched. */
               readSketch: readSketch, chanceCorrect: chanceCorrect, matchBags: matchBags,
               burst: burst, rotBag: rotBag, hdf: hdf, mirrorSide: mirrorSide,
               voteCells: voteCells, countInliers: countInliers, isqrt: isqrt,
               /* v4.2 (SPEC-004.2 geom42) reads these; visibility only. */
               popcount: popcount32, levelDim: levelDim, scaleBin: scaleBin,
               sh10: sh10, shr16: shr16, SCALE_Q16: SCALE_Q16,
               MAX_KP_COUNT: MAX_KP_COUNT, F_KPQ: F_KPQ }
};

return paph3;
});
