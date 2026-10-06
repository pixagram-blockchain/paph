/*! paph4.js — PAPH v4 comparator, JavaScript engine.  Implements PAPH-SPEC-004
 *  on the UNCHANGED v3 wire (format 3, comparator 4).  Deterministic,
 *  integer-only, zero dependencies; requires the v3 engine (wire.cjs) for the
 *  wire, the six untouched structural channels, and the shared geometric
 *  primitives.
 *
 *  Ported FROM the Rust reference (rust/src/{sha256,calibration,assignment,
 *  nulls,coverage,local_v4,multimodel,lattice,v4}.rs) AGAINST the golden
 *  vectors in docs/golden/GOLDEN-004.json — the vectors were consumed first,
 *  the port written second, and test/v4.cjs holds the port to every section
 *  of the file.  Where wire primitives exist they are reused via _internal
 *  (visibility only); where SPEC-004 §12.2 pins constants that v3 also owns
 *  (HAM_MAX, the Lowe ratio), they are deliberately DUPLICATED here, exactly
 *  as the Rust port duplicates them, so a future retune of one comparator
 *  cannot silently move the other.  MIT. */
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory(require('./wire.cjs'));
  else if (typeof define === 'function' && define.amd) define(['./wire.cjs'], factory);
  else root.paphjs = factory(root.paphWire);
})(typeof self !== 'undefined' ? self : this, function (wire) {
'use strict';

var I = wire._internal;
var SCALE = 10000;
var COMPARATOR = 4, CONTAINER = 1;
var CONTAINER_V2 = 2, COMPARATOR_V41 = 41; /* SPEC-004.1 */
var CONTAINER_V3 = 3, COMPARATOR_V42 = 42; /* SPEC-004.2 */

/* Math.trunc(a / b): truncation toward zero on values that can pass 2^31,
   matching Rust i64 division exactly.  paph3's `(a/b)|0` is only safe inside
   int32; the v4 layer does not inherit that constraint. */
function idiv(a, b) { return Math.trunc(a / b); }
function clamp(v, lo, hi) { return v < lo ? lo : (v > hi ? hi : v); }
function u8(x) { return x instanceof Uint8Array ? x : new Uint8Array(x); }

/* ================================================================ sha256
 * FIPS 180-4, bytes in, 32 bytes out.  Hand-rolled like everything else that
 * two parties must agree on. */
var K256 = new Int32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2]);
function rotr(x, n) { return ((x >>> n) | (x << (32 - n))) >>> 0; }
function sha256(msg) {
  msg = u8(msg);
  var l = msg.length, bitLen = l * 8;
  var padded = new Uint8Array(((l + 8) >> 6 << 6) + 64);
  padded.set(msg); padded[l] = 0x80;
  var dv = padded.length - 8;
  /* message length in bits, big-endian 64-bit; JS numbers hold 2^53 exactly */
  padded[dv + 3] = Math.floor(bitLen / 0x100000000) & 0xff;
  padded[dv + 4] = (bitLen >>> 24) & 0xff; padded[dv + 5] = (bitLen >>> 16) & 0xff;
  padded[dv + 6] = (bitLen >>> 8) & 0xff; padded[dv + 7] = bitLen & 0xff;
  var h0 = 0x6a09e667, h1 = 0xbb67ae85, h2 = 0x3c6ef372, h3 = 0xa54ff53a;
  var h4 = 0x510e527f, h5 = 0x9b05688c, h6 = 0x1f83d9ab, h7 = 0x5be0cd19;
  var w = new Int32Array(64);
  for (var off = 0; off < padded.length; off += 64) {
    for (var t = 0; t < 16; t++) {
      var o = off + t * 4;
      w[t] = (padded[o] << 24) | (padded[o + 1] << 16) | (padded[o + 2] << 8) | padded[o + 3];
    }
    for (t = 16; t < 64; t++) {
      var s0 = rotr(w[t - 15] >>> 0, 7) ^ rotr(w[t - 15] >>> 0, 18) ^ ((w[t - 15] >>> 0) >>> 3);
      var s1 = rotr(w[t - 2] >>> 0, 17) ^ rotr(w[t - 2] >>> 0, 19) ^ ((w[t - 2] >>> 0) >>> 10);
      w[t] = (((w[t - 16] >>> 0) + (s0 >>> 0) + (w[t - 7] >>> 0) + (s1 >>> 0)) & 0xffffffff) | 0;
    }
    var a = h0, b = h1, c = h2, d = h3, e = h4, f = h5, g = h6, hh = h7;
    for (t = 0; t < 64; t++) {
      var S1 = rotr(e >>> 0, 6) ^ rotr(e >>> 0, 11) ^ rotr(e >>> 0, 25);
      var ch = (e & f) ^ (~e & g);
      var t1 = ((hh >>> 0) + (S1 >>> 0) + (ch >>> 0) + (K256[t] >>> 0) + (w[t] >>> 0)) >>> 0;
      var S0 = rotr(a >>> 0, 2) ^ rotr(a >>> 0, 13) ^ rotr(a >>> 0, 22);
      var maj = (a & b) ^ (a & c) ^ (b & c);
      var t2 = ((S0 >>> 0) + (maj >>> 0)) >>> 0;
      hh = g; g = f; f = e; e = ((d >>> 0) + t1) >>> 0;
      d = c; c = b; b = a; a = (t1 + t2) >>> 0;
    }
    h0 = ((h0 >>> 0) + a) >>> 0; h1 = ((h1 >>> 0) + b) >>> 0;
    h2 = ((h2 >>> 0) + c) >>> 0; h3 = ((h3 >>> 0) + d) >>> 0;
    h4 = ((h4 >>> 0) + e) >>> 0; h5 = ((h5 >>> 0) + f) >>> 0;
    h6 = ((h6 >>> 0) + g) >>> 0; h7 = ((h7 >>> 0) + hh) >>> 0;
  }
  var out = new Uint8Array(32);
  [h0, h1, h2, h3, h4, h5, h6, h7].forEach(function (hv, i) {
    out[i * 4] = (hv >>> 24) & 0xff; out[i * 4 + 1] = (hv >>> 16) & 0xff;
    out[i * 4 + 2] = (hv >>> 8) & 0xff; out[i * 4 + 3] = hv & 0xff;
  });
  return out;
}
function hex(bytes) {
  var s = '';
  for (var i = 0; i < bytes.length; i++) s += (bytes[i] | 0x100).toString(16).slice(1);
  return s;
}

/* §7 — hash-profile identity over the ten hash-time fields, LE i32 each. */
function hashProfileId(o) {
  var f = [o.foldMatte ? 1 : 0, o.divideUpscale ? 1 : 0, o.matteTol, o.peakRadius,
           o.foldInvert ? 1 : 0, o.localWindows[0], o.localWindows[1],
           o.localCount, o.kpCount, o.sketchCount];
  var m = new Uint8Array(8 + 40);
  m.set([0x50, 0x41, 0x50, 0x48, 0x2d, 0x48, 0x50, 0x03]); /* "PAPH-HP" + 0x03 */
  for (var i = 0; i < 10; i++) {
    var v = f[i] | 0, b = 8 + i * 4;
    m[b] = v & 0xff; m[b + 1] = (v >>> 8) & 0xff; m[b + 2] = (v >>> 16) & 0xff; m[b + 3] = (v >>> 24) & 0xff;
  }
  return sha256(m);
}

/* ============================================================= assignment
 * §10.2, Appendix A — maximum-cardinality minimum-cost, e-maxx Hungarian,
 * rows once ascending, argmin ties to the smaller column, NO_EDGE completed
 * with BIG so cardinality dominates cost lexicographically. */
var NO_EDGE = -1;
var A_BIG = 1 << 30;
var A_INF = Number.MAX_SAFE_INTEGER / 4;
function assign(n, m, cost) {
  if (!n || !m) return [];
  var transposed = n > m;
  var rn = transposed ? m : n, rm = transposed ? n : m;
  function at(r, c) { var v = transposed ? cost[c * m + r] : cost[r * m + c]; return v < 0 ? A_BIG : v; }
  var u = new Array(rn).fill(0), v = new Array(rm + 1).fill(0);
  var p = new Array(rm + 1).fill(-1), way = new Array(rm).fill(0);
  for (var i = 0; i < rn; i++) {
    p[rm] = i;
    var j0 = rm;
    var minv = new Array(rm).fill(A_INF);
    var used = new Array(rm + 1).fill(false);
    for (;;) {
      used[j0] = true;
      var i0 = p[j0], delta = A_INF, j1 = -1, j;
      for (j = 0; j < rm; j++) {
        if (used[j]) continue;
        var cur = at(i0, j) - u[i0] - v[j];
        if (cur < minv[j]) { minv[j] = cur; way[j] = j0; }
        if (minv[j] < delta) { delta = minv[j]; j1 = j; } /* strict <: smallest column keeps ties */
      }
      for (j = 0; j <= rm; j++) {
        if (used[j]) { if (p[j] !== -1) u[p[j]] += delta; v[j] -= delta; }
        else minv[j] -= delta;
      }
      j0 = j1;
      if (p[j0] === -1) break;
    }
    for (;;) {
      var jw = way[j0];
      p[j0] = p[jw];
      j0 = jw;
      if (j0 === rm) break;
    }
  }
  var out = [];
  for (var jj = 0; jj < rm; jj++) {
    var ii = p[jj];
    if (ii === -1) continue;
    var r = transposed ? jj : ii, c = transposed ? ii : jj;
    if (cost[r * m + c] >= 0) out.push([r, c]); /* BIG edges are completion, not matches */
  }
  out.sort(function (x, y) { return x[0] - y[0] || x[1] - y[1]; });
  return out;
}

/* ================================================================= nulls
 * §9 — LN code maps and GN index permutations (Appendix D). */
function bitrev32(x) {
  x = ((x >>> 1) & 0x55555555) | ((x & 0x55555555) << 1);
  x = ((x >>> 2) & 0x33333333) | ((x & 0x33333333) << 2);
  x = ((x >>> 4) & 0x0f0f0f0f) | ((x & 0x0f0f0f0f) << 4);
  x = ((x >>> 8) & 0x00ff00ff) | ((x & 0x00ff00ff) << 8);
  return ((x >>> 16) | (x << 16)) >>> 0;
}
function rot64(hi, lo, k) {
  k &= 63;
  if (k === 0) return [hi >>> 0, lo >>> 0];
  if (k === 32) return [lo >>> 0, hi >>> 0];
  if (k < 32) return [((hi << k) | (lo >>> (32 - k))) >>> 0, ((lo << k) | (hi >>> (32 - k))) >>> 0];
  var j = k - 32;
  return [((lo << j) | (hi >>> (32 - j))) >>> 0, ((hi << j) | (lo >>> (32 - j))) >>> 0];
}
function bitrev64(hi, lo) { return [bitrev32(lo), bitrev32(hi)]; }
var LOCAL_FAMILY = ['rot16', 'rot32', 'rot48', 'bitrev'];
function applyLocal(name, hi, lo) {
  if (name === 'rot16') return rot64(hi, lo, 16);
  if (name === 'rot32') return rot64(hi, lo, 32);
  if (name === 'rot48') return rot64(hi, lo, 48);
  return bitrev64(hi, lo);
}
function bitrevBag(Y) {
  var o = { hi: new Int32Array(Y.n), lo: new Int32Array(Y.n), x: Y.x, y: Y.y, n: Y.n };
  for (var i = 0; i < Y.n; i++) {
    var r = bitrev64(Y.hi[i], Y.lo[i]);
    o.hi[i] = r[0] | 0; o.lo[i] = r[1] | 0;
  }
  return o;
}
function shiftOffsets(n) {
  var raw = [n >> 1, idiv(n, 3), idiv(n, 5), idiv(2 * n, 3)], out = [];
  for (var i = 0; i < raw.length; i++) {
    var o = raw[i];
    if (o > 0 && o < n && out.indexOf(o) < 0) out.push(o);
  }
  return out;
}
/* Row k keeps its own descriptor distances and confidence; only the B-side
   geometry index is re-dealt. */
function applyShift(corr, p) {
  var n = corr.length, out = new Array(n);
  for (var k = 0; k < n; k++) {
    var r = corr[k], s = corr[(k + p) % n];
    out[k] = { i: r.i, j: s.j, d: r.d, c: r.c };
  }
  return out;
}
function applyReverse(corr) {
  var n = corr.length, out = new Array(n);
  for (var k = 0; k < n; k++) {
    var r = corr[k], s = corr[n - 1 - k];
    out[k] = { i: r.i, j: s.j, d: r.d, c: r.c };
  }
  return out;
}

/* =============================================================== coverage
 * §11 — g×g grid, occupied / coverage / bbox / 4-connected concentration,
 * two-pass union-find, min label wins. */
function cell(x, g) { return ((clamp(x, 0, 65535) * g) >>> 16); }
function ufFind(parent, i) {
  while (parent[i] !== i) { parent[i] = parent[parent[i]]; i = parent[i]; }
  return i;
}
function ufUnion(parent, a, b) {
  var ra = ufFind(parent, a), rb = ufFind(parent, b);
  if (ra === rb) return;
  if (ra < rb) parent[rb] = ra; else parent[ra] = rb;
}
function coverage(points, g) {
  var gs = g | 0, counts = new Array(gs * gs).fill(0), i;
  for (i = 0; i < points.length; i++) {
    var c = cell(points[i][1], g) * gs + cell(points[i][0], g);
    if (counts[c] < 65535) counts[c]++;
  }
  var occ = [];
  for (i = 0; i < gs * gs; i++) if (counts[i] > 0) occ.push(i);
  if (!occ.length) return { g: g, occupied: 0, coverage: 0, bboxCells: 0, concentration: 0, counts: counts };
  var x0 = gs, x1 = 0, y0 = gs, y1 = 0;
  for (i = 0; i < occ.length; i++) {
    var cx = occ[i] % gs, cy = idiv(occ[i], gs);
    if (cx < x0) x0 = cx; if (cx > x1) x1 = cx;
    if (cy < y0) y0 = cy; if (cy > y1) y1 = cy;
  }
  var bbox = (x1 - x0 + 1) * (y1 - y0 + 1);
  var parent = new Array(gs * gs);
  for (i = 0; i < gs * gs; i++) parent[i] = i;
  for (var yy = 0; yy < gs; yy++) for (var xx = 0; xx < gs; xx++) {
    var k = yy * gs + xx;
    if (!counts[k]) continue;
    if (xx + 1 < gs && counts[k + 1]) ufUnion(parent, k, k + 1);
    if (yy + 1 < gs && counts[k + gs]) ufUnion(parent, k, k + gs);
  }
  var sizes = new Array(gs * gs).fill(0), largest = 0;
  for (i = 0; i < occ.length; i++) {
    var r = ufFind(parent, occ[i]);
    sizes[r]++;
    if (sizes[r] > largest) largest = sizes[r];
  }
  return { g: g, occupied: occ.length, coverage: idiv(occ.length * SCALE, gs * gs),
           bboxCells: bbox, concentration: idiv(largest * SCALE, occ.length), counts: counts };
}

/* ============================================================ calibration
 * §8, Appendix B — the PCAL artefact.  One canonical encoding, SHA-256
 * identity, PROFILE_UNSUPPORTED on any validation failure. */
var PCAL_FIXED = 144, MAX_BREAKPOINTS = 33;
/* Container 3 appends eight i32 after the container-1/2 fixed block, so a
   container-1 or container-2 artefact keeps the byte layout it shipped with
   and its identity hash does not move. */
var PCAL_FIXED_V3 = PCAL_FIXED + 8 * 4;
function pcalFixed(c) { return c === CONTAINER_V3 ? PCAL_FIXED_V3 : PCAL_FIXED; }
function lutIdentity() { return [[0, 0], [SCALE, SCALE]]; }
function lutNeutral() { return [[0, SCALE], [SCALE, SCALE]]; }
function lutValidate(p) {
  if (p.length < 2 || p.length > MAX_BREAKPOINTS) throw new RangeError('lut breakpoint count');
  if (p[0][0] !== 0 || p[p.length - 1][0] !== SCALE) throw new RangeError('lut must span 0..SCALE');
  for (var i = 1; i < p.length; i++) {
    if (p[i][0] <= p[i - 1][0]) throw new RangeError('lut x not strictly increasing');
    if (p[i][1] < p[i - 1][1]) throw new RangeError('lut y not monotone');
  }
  for (i = 0; i < p.length; i++) if (p[i][1] > SCALE) throw new RangeError('lut y out of currency');
}
function lutEval(p, x) {
  x = clamp(x, 0, SCALE);
  var i = 0;
  while (i + 2 < p.length && p[i + 1][0] < x) i++;
  if (i + 2 === p.length && x >= p[i + 1][0]) return p[i + 1][1];
  var x0 = p[i][0], y0 = p[i][1], x1 = p[i + 1][0], y1 = p[i + 1][1];
  return y0 + idiv((y1 - y0) * (x - x0), x1 - x0);
}
function w16(b, o, v) { b[o] = v & 0xff; b[o + 1] = (v >>> 8) & 0xff; return o + 2; }
function w32b(b, o, v) {
  v |= 0;
  b[o] = v & 0xff; b[o + 1] = (v >>> 8) & 0xff; b[o + 2] = (v >>> 16) & 0xff; b[o + 3] = (v >>> 24) & 0xff;
  return o + 4;
}
function r16(b, o) { return b[o] | (b[o + 1] << 8); }
function r32s(b, o) { return (b[o] | (b[o + 1] << 8) | (b[o + 2] << 16) | (b[o + 3] << 24)) | 0; }
function profileEncode(p) {
  var pc = p.container || CONTAINER;
  var luts = [p.lutLocal, p.lutGeometry, p.lutDiversity];
  if (pc === CONTAINER_V2 || pc === CONTAINER_V3)
    luts = luts.concat([p.lutDct, p.lutShape, p.lutTopology, p.lutRuns, p.lutPalette, p.lutSilhouette]);
  if (pc === CONTAINER_V3) luts = luts.concat([p.lutGeoDiversity]);
  var len = pcalFixed(pc);
  for (var li = 0; li < luts.length; li++) len += 2 + luts[li].length * 4;
  var b = new Uint8Array(len), o = 0;
  b[0] = 0x50; b[1] = 0x43; b[2] = 0x41; b[3] = 0x4c; o = 4; /* "PCAL" */
  o = w16(b, o, p.container || CONTAINER);
  o = w16(b, o, p.comparator || COMPARATOR);
  b.set(p.name, o); o += 16;
  b[o++] = p.evidenceRule; b[o++] = p.scoringRule; b[o++] = p.ragEndpoint; b[o++] = p.gridG;
  var scalars = [p.hammingT, p.confidenceAt, p.geoConfAt, p.geoEps, p.geoMinCorr,
                 p.minModelInliers, p.maxModels, p.repExtremeAt, p.coverageFloor, p.minSecondaries];
  var i;
  for (i = 0; i < 10; i++) o = w32b(b, o, scalars[i]);
  for (i = 0; i < 9; i++) o = w32b(b, o, p.thresholds[i]);
  for (i = 0; i < 7; i++) o = w32b(b, o, p.weights[i]);
  for (i = 0; i < 3; i++) o = w32b(b, o, p.limits[i]);
  if (pc === CONTAINER_V3) {
    var v42 = [p.loweNum, p.loweDen, p.loweMargin, p.hamMax,
               p.minPeakMembers, p.exclPct, p.kpSelect, p.scaleSoft];
    for (i = 0; i < 8; i++) o = w32b(b, o, v42[i]);
  }
  for (li = 0; li < luts.length; li++) {
    o = w16(b, o, luts[li].length);
    for (i = 0; i < luts[li].length; i++) {
      o = w16(b, o, luts[li][i][0]);
      o = w16(b, o, luts[li][i][1]);
    }
  }
  return b;
}
function profileDecode(bytes) {
  var b = u8(bytes);
  if (b.length < PCAL_FIXED + 6) throw new RangeError('profile too short');
  if (!(b[0] === 0x50 && b[1] === 0x43 && b[2] === 0x41 && b[3] === 0x4c)) throw new TypeError('bad profile magic');
  var container = r16(b, 4), comparator = r16(b, 6), nlut;
  if (container === CONTAINER && comparator === COMPARATOR) nlut = 3;
  else if (container === CONTAINER_V2 && comparator === COMPARATOR_V41) nlut = 9;
  else if (container === CONTAINER_V3 && comparator === COMPARATOR_V42) nlut = 10;
  else if (container === CONTAINER || container === CONTAINER_V2 || container === CONTAINER_V3)
    throw new RangeError('profile targets another comparator');
  else throw new RangeError('unsupported container version');
  if (b.length < pcalFixed(container) + 6) throw new RangeError('profile too short');
  var p = { name: b.slice(8, 24), container: container, comparator: comparator };
  var o = 24;
  p.evidenceRule = b[o++]; p.scoringRule = b[o++]; p.ragEndpoint = b[o++]; p.gridG = b[o++];
  var s = [];
  var i;
  for (i = 0; i < 10; i++) { s.push(r32s(b, o)); o += 4; }
  p.hammingT = s[0]; p.confidenceAt = s[1]; p.geoConfAt = s[2]; p.geoEps = s[3]; p.geoMinCorr = s[4];
  p.minModelInliers = s[5]; p.maxModels = s[6]; p.repExtremeAt = s[7]; p.coverageFloor = s[8]; p.minSecondaries = s[9];
  p.thresholds = []; for (i = 0; i < 9; i++) { p.thresholds.push(r32s(b, o)); o += 4; }
  p.weights = []; for (i = 0; i < 7; i++) { p.weights.push(r32s(b, o)); o += 4; }
  p.limits = []; for (i = 0; i < 3; i++) { p.limits.push(r32s(b, o)); o += 4; }
  /* container-1/2 artefacts predate the v42 block and decode to the 4.1
     behaviour, so their bytes and their identity hash do not move */
  p.loweNum = 82; p.loweDen = 100; p.loweMargin = 0; p.hamMax = 88;
  p.minPeakMembers = 3; p.exclPct = 0; p.kpSelect = 0; p.scaleSoft = 0;
  if (container === CONTAINER_V3) {
    p.loweNum = r32s(b, o); p.loweDen = r32s(b, o + 4);
    p.loweMargin = r32s(b, o + 8); p.hamMax = r32s(b, o + 12);
    p.minPeakMembers = r32s(b, o + 16); p.exclPct = r32s(b, o + 20);
    p.kpSelect = r32s(b, o + 24); p.scaleSoft = r32s(b, o + 28);
    o += 32;
  }
  var luts = [];
  for (var li = 0; li < nlut; li++) {
    if (o + 2 > b.length) throw new RangeError('profile truncated in lut table');
    var n = r16(b, o); o += 2;
    if (o + n * 4 > b.length) throw new RangeError('profile truncated in lut points');
    var pts = [];
    for (i = 0; i < n; i++) { pts.push([r16(b, o), r16(b, o + 2)]); o += 4; }
    luts.push(pts);
  }
  if (o !== b.length) throw new RangeError('profile length mismatch');
  p.lutLocal = luts[0]; p.lutGeometry = luts[1]; p.lutDiversity = luts[2];
  if (nlut >= 9) {
    p.lutDct = luts[3]; p.lutShape = luts[4]; p.lutTopology = luts[5];
    p.lutRuns = luts[6]; p.lutPalette = luts[7]; p.lutSilhouette = luts[8];
  } else {
    p.lutDct = lutIdentity(); p.lutShape = lutIdentity(); p.lutTopology = lutIdentity();
    p.lutRuns = lutIdentity(); p.lutPalette = lutIdentity(); p.lutSilhouette = lutIdentity();
  }
  p.lutGeoDiversity = nlut === 10 ? luts[9] : lutNeutral();
  return p;
}
function profileValidate(p) {
  var vc = p.container || CONTAINER, vq = p.comparator || COMPARATOR;
  if (!((vc === CONTAINER && vq === COMPARATOR) || (vc === CONTAINER_V2 && vq === COMPARATOR_V41) ||
        (vc === CONTAINER_V3 && vq === COMPARATOR_V42)))
    throw new RangeError('container/comparator pair outside 4, 4.1 or 4.2');
  if (vc === CONTAINER_V3) {
    if (!(p.loweNum >= 1 && p.loweNum <= p.loweDen) || !(p.loweDen >= 1 && p.loweDen <= 1000))
      throw new RangeError('lowe ratio range');
    if (p.loweMargin < 0 || p.loweMargin > 64) throw new RangeError('lowe_margin range');
    if (p.hamMax < 0 || p.hamMax > 256) throw new RangeError('ham_max range');
    if (p.minPeakMembers < 2 || p.minPeakMembers > 64) throw new RangeError('min_peak_members range');
    if (p.exclPct < 0 || p.exclPct > 1000) throw new RangeError('excl_pct range');
    if (!(p.kpSelect === 0 || p.kpSelect === 1) || !(p.scaleSoft === 0 || p.scaleSoft === 1))
      throw new RangeError('kp_select / scale_soft are flags');
  }
  if (p.evidenceRule !== 0 || p.scoringRule !== 1 || p.ragEndpoint !== 1)
    throw new RangeError('bound rule outside v4 (Proportion/Gate are retired)');
  if (p.gridG < 2 || p.gridG > 16) throw new RangeError('grid_g range');
  if (p.hammingT < 0 || p.hammingT > 128) throw new RangeError('hamming_t range');
  if (p.confidenceAt < 1 || p.geoConfAt < 1 || p.geoEps < 0) throw new RangeError('confidence field range');
  if (p.geoMinCorr < 2 || p.geoMinCorr > 64 || p.minModelInliers < 2 || p.minModelInliers > 64)
    throw new RangeError('model inlier floors');
  if (p.maxModels < 1 || p.maxModels > 8) throw new RangeError('max_models range');
  if (p.repExtremeAt < 0 || p.repExtremeAt > SCALE || p.coverageFloor < 0 || p.coverageFloor > SCALE)
    throw new RangeError('currency field range');
  if (p.minSecondaries < 0 || p.minSecondaries > 6) throw new RangeError('min_secondaries range');
  for (var i = 0; i < 9; i++) {
    var hi = i === 7 ? 512 : SCALE;
    if (p.thresholds[i] < 0 || p.thresholds[i] > hi) throw new RangeError('threshold range');
  }
  for (i = 0; i < 7; i++) if (p.weights[i] < 0 || p.weights[i] > SCALE) throw new RangeError('weight range');
  for (i = 0; i < 3; i++) if (p.limits[i] <= 0) throw new RangeError('limit range');
  if (p.limits[0] > 65535 || p.limits[1] > 65535) throw new RangeError('limit range');
  lutValidate(p.lutLocal); lutValidate(p.lutGeometry); lutValidate(p.lutDiversity);
  lutValidate(p.lutDct || lutIdentity()); lutValidate(p.lutShape || lutIdentity());
  lutValidate(p.lutTopology || lutIdentity()); lutValidate(p.lutRuns || lutIdentity());
  lutValidate(p.lutPalette || lutIdentity()); lutValidate(p.lutSilhouette || lutIdentity());
  lutValidate(p.lutGeoDiversity || lutNeutral());
}
/* SPEC-004.1 A3 — a structural channel's table; identity for anything a
 * container-1 profile never heard of. */
function lutChannel(p, name) {
  var t = { dct: p.lutDct, shape: p.lutShape, topology: p.lutTopology,
            runs: p.lutRuns, palette: p.lutPalette, silhouette: p.lutSilhouette }[name];
  return t || lutIdentity();
}
function profileId(p) { return sha256(profileEncode(p)); }
function profileIdHex16(p) { return hex(profileId(p).slice(0, 8)); }
function profileName(p) {
  var end = 16;
  for (var i = 0; i < 16; i++) if (p.name[i] === 0) { end = i; break; }
  var s = '';
  for (i = 0; i < end; i++) s += String.fromCharCode(p.name[i]);
  return s;
}
function legacyCal001() {   /* container 1 / comparator 4 — readable, never computable */
  var name = new Uint8Array(16);
  var n = 'CAL-001-PROVISIO';
  for (var i = 0; i < 16; i++) name[i] = n.charCodeAt(i);
  return {
    container: CONTAINER, comparator: COMPARATOR,
    name: name, evidenceRule: 0, scoringRule: 1, ragEndpoint: 1, gridG: 4,
    hammingT: 8, confidenceAt: 16, geoConfAt: 16, geoEps: 1600, geoMinCorr: 8,
    minModelInliers: 6, maxModels: 4, repExtremeAt: 1500, coverageFloor: 2500, minSecondaries: 3,
    loweNum: 82, loweDen: 100, loweMargin: 0, hamMax: 88,
    minPeakMembers: 3, exclPct: 0, kpSelect: 0, scaleSoft: 0,
    thresholds: [8000, 4500, 3000, 1500, 6750, 3500, 1200, 15, 6000],
    weights: [35, 25, 15, 10, 10, 5, 10],
    limits: [16384, 16384, 16777216],
    lutLocal: lutIdentity(), lutGeometry: lutIdentity(), lutDiversity: lutNeutral(),
    lutDct: lutIdentity(), lutShape: lutIdentity(), lutTopology: lutIdentity(),
    lutRuns: lutIdentity(), lutPalette: lutIdentity(), lutSilhouette: lutIdentity(),
    lutGeoDiversity: lutNeutral()
  };
}
/* SPEC-004.1 Part II — CAL-003-PROPOSED, the comparator-41 default. */
function cal003() {
  var name = new Uint8Array(16);
  var n = 'CAL-003-PROPOSED';
  for (var i = 0; i < 16; i++) name[i] = n.charCodeAt(i);
  return {
    container: CONTAINER_V2, comparator: COMPARATOR_V41,
    name: name, evidenceRule: 0, scoringRule: 1, ragEndpoint: 1, gridG: 4,
    hammingT: 8, confidenceAt: 16, geoConfAt: 16, geoEps: 1600, geoMinCorr: 8,
    minModelInliers: 6, maxModels: 4, repExtremeAt: 1500, coverageFloor: 2500, minSecondaries: 3,
    loweNum: 82, loweDen: 100, loweMargin: 0, hamMax: 88,
    minPeakMembers: 3, exclPct: 0, kpSelect: 0, scaleSoft: 0,
    thresholds: [8000, 4000, 2400, 1000, 6000, 3500, 1200, 11, 6000],
    weights: [35, 25, 15, 10, 10, 5, 10],
    limits: [16384, 16384, 16777216],
    lutLocal: [[0, 0], [5000, 1000], [8800, 4200], [9400, 8800], [10000, 10000]],
    lutGeometry: [[0, 0], [1900, 1100], [3000, 3000], [10000, 10000]],
    lutDiversity: lutNeutral(),
    lutDct: lutIdentity(), lutShape: lutIdentity(), lutTopology: lutIdentity(),
    lutRuns: [[0, 0], [9100, 900], [9700, 4500], [10000, 10000]],
    lutPalette: [[0, 0], [3000, 800], [6500, 4000], [10000, 10000]],
    lutSilhouette: lutIdentity(), lutGeoDiversity: lutNeutral()
  };
}

/* SPEC-004.2 Part II — CAL-004-PROPOSED, the comparator-42 default.
 *
 * Every DECISION constant is CAL-003's, unchanged.  That is the claim 4.2 is
 * making: a larger budget changes how well the pair is measured, not what a
 * copy is.
 *
 * The two count-valued knobs were the temptation.  geoConfAt and the
 * geometry-solo inlier floor are absolute counts, and 512 keypoints plainly put
 * more inliers through them, so doubling both looked obligatory — the argument
 * being that a control saturating at SCALE zeroes the margin by construction
 * (chanceCorrect).  That argument was MEASURED rather than assumed, and it does
 * not hold: on 512-keypoint works the GN control reaches roughly 3125 of 10000,
 * nowhere near the saturation point, while doubling geoConfAt halves the
 * reading on every small work and doubling the solo floor turned a genuine crop
 * from Copy into Suspected at 128x128.  A floor only large works can clear is a
 * size-dependent bias, which is the defect class this family has already paid
 * for once (v2 indexing its pyramid by width).
 *
 * The guard against a repetitive work manufacturing inliers at 512 is therefore
 * §8 diversity, which measures the problem directly, and not a raised
 * saturation point, which only measures it by proxy.
 *
 * PROPOSED because the corpus has not been re-hashed at 512 yet. */
function cal004() {
  var name = new Uint8Array(16);
  var n = 'CAL-004-PROPOSED';
  for (var i = 0; i < 16; i++) name[i] = n.charCodeAt(i);
  return {
    container: CONTAINER_V3, comparator: COMPARATOR_V42,
    name: name, evidenceRule: 0, scoringRule: 1, ragEndpoint: 1, gridG: 4,
    hammingT: 8, confidenceAt: 16,
    geoConfAt: 16, geoEps: 1600, geoMinCorr: 8,
    minModelInliers: 6, maxModels: 4, repExtremeAt: 1500, coverageFloor: 2500, minSecondaries: 3,
    loweNum: 82, loweDen: 100, loweMargin: 6, hamMax: 88,
    minPeakMembers: 3, exclPct: 100, kpSelect: 1, scaleSoft: 1,
    thresholds: [8000, 4000, 2400, 1000, 6000, 3500, 1200, 11, 6000],
    weights: [35, 25, 15, 10, 10, 5, 10],
    limits: [16384, 16384, 16777216],
    lutLocal: [[0, 0], [5000, 1000], [8800, 4200], [9400, 8800], [10000, 10000]],
    lutGeometry: [[0, 0], [1900, 1100], [3000, 3000], [10000, 10000]],
    lutDiversity: lutNeutral(),
    lutDct: lutIdentity(), lutShape: lutIdentity(), lutTopology: lutIdentity(),
    lutRuns: [[0, 0], [9100, 900], [9700, 4500], [10000, 10000]],
    lutPalette: [[0, 0], [3000, 800], [6500, 4000], [10000, 10000]],
    lutSilhouette: lutIdentity(),
    /* §8 — a floor, not a cliff.  Geometry that agrees in one corner of the
       canvas, at one scale, under one model, on one repeated texture keeps 60%
       of its evidence; anything past 4000 in the diversity currency keeps all
       of it. */
    lutGeoDiversity: [[0, 6000], [2000, 8000], [4000, 10000], [10000, 10000]]
  };
}

/* =============================================================== local v4
 * §10 — the rebuilt local channel.  v3's pipeline verbatim (burst weighting,
 * capacity, Lift, chanceCorrect) with the assignment matcher and the
 * MAX-aggregated LN family; diversity, modulation LUT, per-side coverage of
 * matched anchors. */
/* SPEC-004.1 Appendix A — the assignment on the edge-induced subgraph. */
function assignSparse(n, m, cost) {
  if (!n || !m) return [];
  var rowHas = new Array(n).fill(false), colHas = new Array(m).fill(false);
  var r, c;
  for (r = 0; r < n; r++) for (c = 0; c < m; c++)
    if (cost[r * m + c] >= 0) { rowHas[r] = true; colHas[c] = true; }
  var ri = [], ci = [];
  for (r = 0; r < n; r++) if (rowHas[r]) ri.push(r);
  for (c = 0; c < m; c++) if (colHas[c]) ci.push(c);
  if (!ri.length || !ci.length) return [];
  var rn = ri.length, rm = ci.length;
  var sub = new Int32Array(rn * rm).fill(NO_EDGE);
  for (r = 0; r < rn; r++) for (c = 0; c < rm; c++) sub[r * rm + c] = cost[ri[r] * m + ci[c]];
  var out = assign(rn, rm, sub).map(function (p) { return [ri[p[0]], ci[p[1]]]; });
  out.sort(function (a, b) { return a[0] - b[0] || a[1] - b[1]; });
  return out;
}

function assignBagsWith(X, Y, t, asg) {
  var n = X.n, m = Y.n, cost = new Int32Array(n * m).fill(NO_EDGE);
  for (var i = 0; i < n; i++) for (var j = 0; j < m; j++) {
    var d = I.hdf(X, i, Y, j);
    if (d <= t) cost[i * m + j] = d;
  }
  return asg(n, m, cost);
}
function assignBags(X, Y, t) { return assignBagsWith(X, Y, t, assign); }
function emptyLocal(note, g) {
  return { measurable: false, note: note, pairs: [], matches: 0, cmax: 0, w: 0, cap: 1,
           ctlW: 0, ctlN: 0, ctlMember: 'none', liftRaw: 0, liftCtl: 0, margin: 0, evidence: 0,
           propRaw: 0, propCtl: 0, propMargin: 0, diversity: 0, dA: 0, dB: 0,
           coverageA: coverage([], g), coverageB: coverage([], g) };
}
function localCore(X, Y, P, asg) {
  if (X.n < 4 || Y.n < 4) return emptyLocal('too few distinctive regions on one side', P.gridG);
  var t = P.hammingT, cmax = Math.min(X.n, Y.n);
  var bx = I.burst(X, t), by = I.burst(Y, t);
  var ca = 0, cb = 0, i;
  for (i = 0; i < X.n; i++) ca += idiv(SCALE, bx[i]);
  for (i = 0; i < Y.n; i++) cb += idiv(SCALE, by[i]);
  var dA = idiv(ca, X.n), dB = idiv(cb, Y.n);
  var diversity = Math.min(dA, dB);
  var cap = Math.max(Math.min(ca, cb), 1);

  var pairs = assignBagsWith(X, Y, t, asg);
  var c = pairs.length, w = 0;
  for (i = 0; i < pairs.length; i++) w += idiv(SCALE, Math.max(bx[pairs[i][0]], by[pairs[i][1]]));

  var family = [['rot16', I.rotBag(Y, 16)], ['rot32', I.rotBag(Y, 32)],
                ['rot48', I.rotBag(Y, 48)], ['bitrev', bitrevBag(Y)]];
  var e = 0, en = 0, em = 'none';
  for (var f = 0; f < 4; f++) {
    var yr = family[f][1];
    var byr = I.burst(yr, t); /* provably equals by; recomputed as the Rust reference does */
    var hr = assignBagsWith(X, yr, t, asg);
    var wr = 0;
    for (i = 0; i < hr.length; i++) wr += idiv(SCALE, Math.max(bx[hr[i][0]], byr[hr[i][1]]));
    if (wr > e || em === 'none') { e = wr; en = hr.length; em = family[f][0]; }
  }

  var prior = SCALE, confAt = P.confidenceAt;
  var purity = idiv(w * SCALE, w + e + prior);
  var conf = clamp(idiv(c * SCALE, confAt), 0, SCALE);
  var liftRaw = clamp(idiv(purity * conf, SCALE), 0, SCALE);
  var liftCtl = clamp(idiv(idiv(e * SCALE, e + e + prior) * clamp(idiv(en * SCALE, confAt), 0, SCALE), SCALE), 0, SCALE);
  var propRaw = clamp(idiv(w * SCALE, cap), 0, SCALE);
  var propCtl = clamp(idiv(e * SCALE, cap), 0, SCALE);
  var margin = I.chanceCorrect(liftRaw, liftCtl);
  var evidence = idiv(lutEval(P.lutLocal, margin) * lutEval(P.lutDiversity, diversity), SCALE);

  var aPts = [], bPts = [];
  for (i = 0; i < pairs.length; i++) {
    aPts.push([X.x[pairs[i][0]], X.y[pairs[i][0]]]);
    bPts.push([Y.x[pairs[i][1]], Y.y[pairs[i][1]]]);
  }
  return { measurable: true,
           note: c + ' of ' + cmax + ' regions assigned, ' + en + ' expected under ' + em,
           pairs: pairs, matches: c, cmax: cmax, w: w, cap: cap,
           ctlW: e, ctlN: en, ctlMember: em,
           liftRaw: liftRaw, liftCtl: liftCtl, margin: margin, evidence: evidence,
           propRaw: propRaw, propCtl: propCtl, propMargin: I.chanceCorrect(propRaw, propCtl),
           diversity: diversity, dA: dA, dB: dB,
           coverageA: coverage(aPts, P.gridG), coverageB: coverage(bPts, P.gridG) };
}
function localV4Bags(X, Y, P) { return localCore(X, Y, P, assign); }
function localV4Bags41(X, Y, P) { return localCore(X, Y, P, assignSparse); }
function localV4(dA, dB, P) { return localV4Bags(I.readLocal(dA), I.readLocal(dB), P); }
function localV4_41(dA, dB, P) { return localV4Bags41(I.readLocal(dA), I.readLocal(dB), P); }

/* ============================================================== multimodel
 * §12.2–§12.3, §13 — weighted correspondences, weighted Hough, iterative
 * two-pool extraction, GN control through the identical procedure, topology.
 * HAM_MAX and the Lowe ratio are DUPLICATED from v3 on purpose (see header). */
var HAM_MAX = 88, LOWE_NUM = 82, LOWE_DEN = 100;
function conf(d1, d2) { return 1 + idiv((d2 - d1) * 63, Math.max(d2, 1)); }
function correspondW(A, B) {
  var na = A.length, nb = B.length;
  if (!na || !nb) return [];
  var aBest = new Int32Array(na).fill(-1), aD1 = new Int32Array(na).fill(999), aD2 = new Int32Array(na).fill(999);
  var bBest = new Int32Array(nb).fill(-1), bD1 = new Int32Array(nb).fill(999), bD2 = new Int32Array(nb).fill(999);
  var i, j, d;
  for (i = 0; i < na; i++) for (j = 0; j < nb; j++) {
    d = I.hamDesc(A[i].desc, B[j].desc);
    if (d < aD1[i]) { aD2[i] = aD1[i]; aD1[i] = d; aBest[i] = j; } else if (d < aD2[i]) aD2[i] = d;
    if (d < bD1[j]) { bD2[j] = bD1[j]; bD1[j] = d; bBest[j] = i; } else if (d < bD2[j]) bD2[j] = d;
  }
  var out = [];
  for (i = 0; i < na; i++) {
    j = aBest[i];
    if (j < 0 || aD1[i] > HAM_MAX) continue;
    if (aD1[i] * LOWE_DEN >= LOWE_NUM * aD2[i]) continue;
    if (bD1[j] * LOWE_DEN >= LOWE_NUM * bD2[j]) continue;
    if (bBest[j] !== i) continue;
    var d2 = Math.min(aD2[i], bD2[j]);
    out.push({ i: i, j: j, d: aD1[i], c: conf(aD1[i], d2) });
  }
  out.sort(function (p, q) { return p.d - q.d || p.i - q.i || p.j - q.j; });
  return out;
}
function houghVerifyW(A, B, corr, o, mdA, mdB) {
  function empty() { return { inliers: 0, mask: corr.map(function () { return false; }), model: null }; }
  if (corr.length < o.geoMinCorr) return empty();
  var votes = new Map(), per = [], i, k;
  for (i = 0; i < corr.length; i++) {
    var vc = I.voteCells(A, B, corr[i], mdA, mdB);
    per.push(vc.cells);
    for (k = 0; k < vc.cells.length; k++)
      votes.set(vc.cells[k], (votes.get(vc.cells[k]) || 0) + corr[i].c);
  }
  var bestKey = Infinity, bestN = 0;
  votes.forEach(function (n, key) { if (n > bestN || (n === bestN && key < bestKey)) { bestN = n; bestKey = key; } });
  var members = [];
  for (i = 0; i < corr.length; i++) if (per[i].indexOf(bestKey) >= 0) members.push(corr[i]);
  if (members.length < 3) return empty();
  var tol = Math.max(2, o.geoEps >> 4), tol2 = tol * tol;
  var M = I.fitSimilarity(A, B, members);
  if (!M) return empty();
  var r = I.countInliers(A, B, corr, M, idiv(tol2 * 9, 4));
  var inl = [];
  for (i = 0; i < corr.length; i++) if (r.mask[i]) inl.push(corr[i]);
  if (inl.length >= 2) { var M2 = I.fitSimilarity(A, B, inl); if (M2) M = M2; }
  r = I.countInliers(A, B, corr, M, tol2);
  return { inliers: r.n, mask: r.mask, model: M };
}
function mmRound(A, B, pool, o, mdA, mdB) {
  if (pool.length >= o.geoMinCorr) return houghVerifyW(A, B, pool, o, mdA, mdB);
  return { inliers: 0, mask: pool.map(function () { return false; }), model: null };
}
function extractFromPools(A, AM, B, pd, pm, o, mdA, mdB, minModelInliers, maxModels) {
  pd = pd.slice(); pm = pm.slice();
  var out = { models: [], totalInliers: 0, corrDirect: pd.length, corrMirror: pm.length, inlierB: [] };
  for (var r = 0; r < maxModels; r++) {
    var floor = r === 0 ? o.geoMinCorr : minModelInliers;
    var vd = mmRound(A, B, pd, o, mdA, mdB);
    var vm = mmRound(AM, B, pm, o, mdA, mdB);
    var useMirror = vm.inliers > vd.inliers; /* ties → direct */
    var v = useMirror ? vm : vd, pool = useMirror ? pm : pd;
    if (!v.model) break;
    if (v.inliers < floor) break;
    var consumed = new Set();
    for (var i = 0; i < pool.length; i++) if (v.mask[i]) {
      consumed.add(pool[i].j);
      out.inlierB.push([B[pool[i].j].x, B[pool[i].j].y]);
    }
    out.totalInliers += v.inliers;
    out.models.push({ r00: v.model.r00, r10: v.model.r10, tx: v.model.tx, ty: v.model.ty,
                      scaleQ16: I.isqrt(v.model.r00 * v.model.r00 + v.model.r10 * v.model.r10),
                      mirror: useMirror, inliers: v.inliers });
    pd = pd.filter(function (c) { return !consumed.has(c.j); });
    pm = pm.filter(function (c) { return !consumed.has(c.j); });
  }
  return out;
}
function extract(A, B, o, xmaxA, mdA, mdB, minModelInliers, maxModels) {
  var AM = I.mirrorSide(A, xmaxA);
  var pd = correspondW(A, B);
  var pm = o.mirrorHypothesis ? correspondW(AM, B) : [];
  return extractFromPools(A, AM, B, pd, pm, o, mdA, mdB, minModelInliers, maxModels);
}
function shiftOrEmpty(c, num, den) {
  var n = c.length;
  if (!n) return [];
  var p = idiv(n * num, den);
  if (p === 0 || p >= n) return []; /* degenerate → empty, never unpermuted (§9.3) */
  return applyShift(c, p);
}
function reverseOrEmpty(c) { return c.length < 2 ? [] : applyReverse(c); }
function gnControl(A, AM, B, pd0, pm0, o, mdA, mdB, minModelInliers, maxModels) {
  var shifts = [['gn:half', 1, 2], ['gn:third', 1, 3], ['gn:fifth', 1, 5], ['gn:twothirds', 2, 3]];
  var best = 0, member = 'gn:none';
  for (var s = 0; s < shifts.length; s++) {
    var pd = shiftOrEmpty(pd0, shifts[s][1], shifts[s][2]);
    var pm = shiftOrEmpty(pm0, shifts[s][1], shifts[s][2]);
    if (!pd.length && !pm.length) continue;
    var t = extractFromPools(A, AM, B, pd, pm, o, mdA, mdB, minModelInliers, maxModels).totalInliers;
    if (t > best) { best = t; member = shifts[s][0]; }
  }
  var rd = reverseOrEmpty(pd0), rm = reverseOrEmpty(pm0);
  if (rd.length || rm.length) {
    var tr = extractFromPools(A, AM, B, rd, rm, o, mdA, mdB, minModelInliers, maxModels).totalInliers;
    if (tr > best) { best = tr; member = 'gn:reverse'; }
  }
  return { ctl: best, member: member };
}
/* SPEC-004.1 A1 — the weak signal: best single verified model over the two
 * pools, member floor kept, acceptance floor absent. */
function weakInliers(A, AM, B, pd, pm, o, mdA, mdB) {
  var vd = pd.length >= o.geoMinCorr ? houghVerifyW(A, B, pd, o, mdA, mdB).inliers : 0;
  var vm = pm.length >= o.geoMinCorr ? houghVerifyW(AM, B, pm, o, mdA, mdB).inliers : 0;
  return Math.max(vd, vm);
}
/* A1 measurement: extraction first, weak fallback; returns {mm, measure, weak}. */
function geoMeasure41(A, AM, B, pd, pm, o, mdA, mdB, minModelInliers, maxModels) {
  var mm = extractFromPools(A, AM, B, pd, pm, o, mdA, mdB, minModelInliers, maxModels);
  if (mm.totalInliers > 0) return { mm: mm, measure: mm.totalInliers, weak: 0 };
  var w = weakInliers(A, AM, B, pd, pm, o, mdA, mdB);
  return { mm: mm, measure: w, weak: w };
}
/* A1 control: every GN member runs the identical measurement. */
function gnControl41(A, AM, B, pd0, pm0, o, mdA, mdB, minModelInliers, maxModels) {
  var shifts = [['gn:half', 1, 2], ['gn:third', 1, 3], ['gn:fifth', 1, 5], ['gn:twothirds', 2, 3]];
  var best = 0, member = 'gn:none';
  var measure = function (pd, pm) {
    return geoMeasure41(A, AM, B, pd, pm, o, mdA, mdB, minModelInliers, maxModels).measure;
  };
  for (var si = 0; si < shifts.length; si++) {
    var pd = shiftOrEmpty(pd0, shifts[si][1], shifts[si][2]);
    var pm = shiftOrEmpty(pm0, shifts[si][1], shifts[si][2]);
    if (!pd.length && !pm.length) continue;
    var t = measure(pd, pm);
    if (t > best) { best = t; member = shifts[si][0]; }
  }
  var rd = reverseOrEmpty(pd0), rm = reverseOrEmpty(pm0);
  if (rd.length || rm.length) {
    var tr = measure(rd, rm);
    if (tr > best) { best = tr; member = 'gn:reverse'; }
  }
  return { ctl: best, member: member };
}
function topology(mm, g, dominantAt, geoMinCorr) {
  var cov = coverage(mm.inlierB, g);
  var cls;
  if (mm.models.length >= 2) cls = 3;
  else if (mm.models.length === 1)
    cls = idiv(cov.bboxCells * SCALE, g * g) >= dominantAt ? 4 : 2;
  else if (Math.max(mm.corrDirect, mm.corrMirror) >= geoMinCorr) cls = 1;
  else cls = 0;
  return { class: cls, coverage: cov };
}

/* ================================================================ lattice
 * §14 — six states, base shape from v3 restated with profile parameters,
 * rules R1–R4 on top; fired rules name themselves in `basis`. */
function widx(name) {
  return { local: 0, shape: 1, topology: 2, runs: 3, dct: 4, palette: 5, silhouette: 6 }[name];
}
function latticeV4(x, P) {
  var wsum = 0, wtot = 0, secondaries = 0, localMeasurable = false, i;
  for (i = 0; i < 7; i++) {
    var name = x.channels[i][0], val = x.channels[i][1], meas = x.channels[i][2];
    if (!meas) continue;
    var w = P.weights[widx(name)];
    wsum += val * w; wtot += w;
    if (name === 'local') localMeasurable = true; else secondaries++;
  }
  var structural = wtot > 0 ? idiv(wsum, wtot) : 0;
  var t = P.thresholds;
  var certifiable = localMeasurable && secondaries >= P.minSecondaries;
  var wouldCertify = localMeasurable && structural >= t[1];
  var r1Fired = wouldCertify && !certifiable;

  var gv = x.geoMeasurable ? x.geoEvidence : 0;
  var geoStrong = x.geoMeasurable && gv >= t[5];
  var geoWeak = x.geoMeasurable && gv >= t[6];
  var sStrong = certifiable && structural >= t[1];
  var sMod = certifiable && structural >= t[2];
  var sRel = structural >= t[3];

  var basis = [], state, klass;
  if (x.identical) {
    state = 'Identical'; klass = 'byte-identical fingerprint'; basis.push('bytes');
    structural = SCALE;
  } else if (sStrong && geoStrong) {
    state = 'Copy'; klass = 'certified — structure and geometry agree';
    basis.push('structural'); basis.push('geometric');
  } else if (sStrong && structural >= t[4]) {
    if (x.coverageMin >= P.coverageFloor) {
      state = 'Copy'; klass = 'structural only — no geometric corroboration'; basis.push('structural');
    } else {
      state = 'Suspected'; klass = 'capped — structural agreement without spatial support';
      basis.push('structural'); basis.push('R4:support');
    }
  } else if (geoStrong && x.totalInliers >= t[7]) {
    if (x.topologyClass === 2 || x.topologyClass === 3 || x.topologyClass === 4) {
      state = 'Copy';
      klass = x.anyMirrorModel ? 'geometric only — mirrored crop / collage class'
                               : 'geometric only — crop / collage class';
      basis.push('geometric');
    } else {
      state = 'Suspected'; klass = 'capped — geometry scattered, no coherent region';
      basis.push('geometric'); basis.push('R3:scatter');
    }
  } else if (sStrong || sMod || geoStrong || geoWeak) {
    state = 'Suspected';
    klass = geoStrong ? 'geometry agrees but below the solo bar'
          : sStrong ? 'structure agrees but below the solo bar' : 'partial agreement';
    if (sMod) basis.push('structural');
    if (geoWeak) basis.push('geometric');
  } else if (sRel) {
    state = 'Related'; klass = 'same family, not a copy';
  } else {
    state = 'Unrelated'; klass = 'no agreement above chance';
  }

  if (r1Fired && state !== 'Identical') basis.push('R1:corroboration');
  if (state === 'Copy' && localMeasurable && x.diversity < P.repExtremeAt && !geoWeak) {
    state = 'Suspected';
    klass = 'capped — extreme repetition without geometric support';
    basis.push('R2:repetition');
  }
  return { state: state, class: klass, basis: basis, structural: structural,
           certifiable: certifiable, secondaries: secondaries };
}


/* ================================================================ geom 4.2
 * SPEC-004.2 §4–§8.  Comparator 41's machinery above is untouched and stays
 * reachable, because a verdict that was issued has to remain reproducible.
 * This block is the same ARCHITECTURE — mutual Lowe, weighted Hough,
 * closed-form similarity, iterative multi-model extraction, MAX over a
 * structured null family — with the four things that stop working once the
 * budget doubles.  Ported from rust/src/geom42.rs against the golden vectors,
 * vectors first. */
var SK_NEAR = 64, SK_ADJ = 24, MAX_CELLS = 24;
var TBIN_W42 = 4096, TBIN_N42 = 64, TBIN_OFF42 = 131072;
var PATCH_R42 = 15, DESC_NOVEL_AT42 = 16;

/* §4 — exact Hamming with early abort.  Returns the true distance whenever it
   is <= limit, and SOME value greater than limit otherwise, which is all the
   matcher needs: limit is max(aD2[i], bD2[j]), so a partial sum past it belongs
   to a pair that cannot become either side's first or second candidate, and
   every downstream test is a strict < against a value at most limit.  Recall is
   untouched; the work is not. */
function hamCut(a, b, limit) {
  var d = I.popcount((a[0] ^ b[0]) >>> 0) + I.popcount((a[1] ^ b[1]) >>> 0);
  if (d > limit) return d;
  d += I.popcount((a[2] ^ b[2]) >>> 0) + I.popcount((a[3] ^ b[3]) >>> 0);
  if (d > limit) return d;
  d += I.popcount((a[4] ^ b[4]) >>> 0) + I.popcount((a[5] ^ b[5]) >>> 0);
  if (d > limit) return d;
  return d + I.popcount((a[6] ^ b[6]) >>> 0) + I.popcount((a[7] ^ b[7]) >>> 0);
}

/* §6 — strength compatibility, Q6, floored at one half.  A weak accidental
   keypoint can pass Lowe; a strong keypoint that also agrees geometrically is
   worth more.  The floor keeps the term from turning a legitimate coarse-level
   match into no evidence at all just because the two sides normalised their
   FAST scores against different level medians. */
function strengthCompat(sa, sb) {
  var a = Math.max(1, sa | 0), b = Math.max(1, sb | 0);
  return clamp(idiv(Math.min(a, b) * 64, Math.max(a, b)), 32, 64);
}

/* §4 — mutual-best under the Lowe ratio AND an absolute margin.  With
   loweMargin = 0 this is exactly comparator 41's correspondence set; the parity
   suite asserts that, pair for pair. */
function correspond42(A, B, P) {
  var na = A.length, nb = B.length;
  if (!na || !nb) return [];
  var aBest = new Int32Array(na).fill(-1), aD1 = new Int32Array(na).fill(999), aD2 = new Int32Array(na).fill(999);
  var bBest = new Int32Array(nb).fill(-1), bD1 = new Int32Array(nb).fill(999), bD2 = new Int32Array(nb).fill(999);
  var i, j, d;
  for (i = 0; i < na; i++) {
    var da = A[i].desc;
    for (j = 0; j < nb; j++) {
      d = hamCut(da, B[j].desc, aD2[i] > bD2[j] ? aD2[i] : bD2[j]);
      if (d < aD1[i]) { aD2[i] = aD1[i]; aD1[i] = d; aBest[i] = j; } else if (d < aD2[i]) aD2[i] = d;
      if (d < bD1[j]) { bD2[j] = bD1[j]; bD1[j] = d; bBest[j] = i; } else if (d < bD2[j]) bD2[j] = d;
    }
  }
  var num = P.loweNum, den = P.loweDen, margin = P.loweMargin, hmax = P.hamMax, out = [];
  for (i = 0; i < na; i++) {
    j = aBest[i];
    if (j < 0 || aD1[i] > hmax) continue;
    if (aD1[i] * den >= num * aD2[i]) continue;
    if (bD1[j] * den >= num * bD2[j]) continue;
    if (bBest[j] !== i) continue;
    /* the margin is two-sided exactly like the ratio, or compare(A,B) and
       compare(B,A) would stop agreeing */
    if (aD2[i] - aD1[i] < margin || bD2[j] - bD1[j] < margin) continue;
    var d2 = Math.min(aD2[i], bD2[j]);
    out.push({ i: i, j: j, d: aD1[i], c: conf(aD1[i], d2), sc: strengthCompat(A[i].s, B[j].s) });
  }
  out.sort(function (p, q) { return p.d - q.d || p.i - q.i || p.j - q.j; });
  return out;
}

function cWeight(c) { return c.c * c.sc; }

/* §5 — the cells a correspondence votes for, scale soft-binned rather than
   bucketed.  The scale estimate comes from the LEVEL DIMENSIONS and is
   therefore quantised; at 256 keypoints there was not enough evidence to spend
   on hedging that quantisation and at 512 there is.  Rotation and both
   translation axes keep Lowe's nearest-two duplication exactly as before —
   votes are DUPLICATED across soft bins, never split. */
function voteCells42(A, B, c, mdA, mdB, soft) {
  var a = A[c.i], b = B[c.j];
  var LA = I.levelDim(mdA, a.level), LB = I.levelDim(mdB, b.level);
  var sq = clamp(idiv(LA * 65536, Math.max(1, LB)), 1, 1 << 24);
  if (sq > (I.SCALE_Q16[24] << 1) || sq < (I.SCALE_Q16[0] >> 1)) return { key: [], kw: [] };
  var dl = I.scaleBin(sq);
  var ds = (b.sec - a.sec) & 63;
  var rx = I.sh10(a.x * I.RC10[ds] - a.y * I.RS10[ds]);
  var ry = I.sh10(a.x * I.RS10[ds] + a.y * I.RC10[ds]);
  var tx = b.x - I.shr16(sq * rx), ty = b.y - I.shr16(sq * ry);
  var xb = clamp(idiv(tx + TBIN_OFF42, TBIN_W42), 0, TBIN_N42 - 1);
  var yb = clamp(idiv(ty + TBIN_OFF42, TBIN_W42), 0, TBIN_N42 - 1);
  var rb = ds >> 2;
  var ls = [[dl, SK_NEAR]];
  if (soft) {
    if (dl > 0) ls.push([dl - 1, SK_ADJ]);
    if (dl < 24) ls.push([dl + 1, SK_ADJ]);
  }
  var xs = [xb], ys = [yb];
  if (((tx + TBIN_OFF42) - xb * TBIN_W42) * 2 >= TBIN_W42) { if (xb + 1 < TBIN_N42) xs.push(xb + 1); }
  else if (xb > 0) xs.push(xb - 1);
  if (((ty + TBIN_OFF42) - yb * TBIN_W42) * 2 >= TBIN_W42) { if (yb + 1 < TBIN_N42) ys.push(yb + 1); }
  else if (yb > 0) ys.push(yb - 1);
  var rs = [rb, (ds & 3) >= 2 ? (rb + 1) & 15 : (rb + 15) & 15];
  var key = [], kw = [], li, ri, xi, yi;
  for (li = 0; li < ls.length; li++) for (ri = 0; ri < rs.length; ri++)
    for (xi = 0; xi < xs.length; xi++) for (yi = 0; yi < ys.length; yi++) {
      key.push(((ls[li][0] * 16 + rs[ri]) * TBIN_N42 + xs[xi]) * TBIN_N42 + ys[yi]);
      kw.push(ls[li][1]);
    }
  return { key: key, kw: kw };
}

/* §5 — a fixed open-addressed integer table.  The Hough space is bounded
   (25 x 16 x 64 x 64 = 1638400 cells) but a pair only ever touches a few
   thousand of them, so neither a dense array nor a Map with an allocator behind
   it is the right shape.  Capacity comes from the correspondence count, the
   hash is one multiply, probing is linear, nothing is allocated per
   correspondence. */
function VoteTable(ncorr) {
  var want = Math.max(64, Math.max(1, ncorr) * MAX_CELLS * 2), cap = 64, bits = 6;
  while (cap < want && bits < 22) { cap <<= 1; bits++; }
  this.key = new Int32Array(cap).fill(-1);
  this.mass = new Float64Array(cap);
  this.members = new Int32Array(cap);
  this.conf = new Float64Array(cap);
  this.mask = cap - 1;
  this.shift = 32 - bits;
}
VoteTable.prototype.slot = function (k) {
  /* Knuth multiplicative, high bits — Math.imul is the same multiply the Rust
     side spells wrapping_mul */
  return ((Math.imul(k, 2654435761) >>> this.shift) & this.mask) >>> 0;
};
VoteTable.prototype.add = function (k, mass, cf) {
  var s = this.slot(k);
  for (;;) {
    if (this.key[s] === k) { this.mass[s] += mass; this.members[s]++; this.conf[s] += cf; return; }
    if (this.key[s] === -1) { this.key[s] = k; this.mass[s] = mass; this.members[s] = 1; this.conf[s] = cf; return; }
    s = (s + 1) & this.mask;
  }
};
/* §6 — the peak, by an explicit total order rather than by whichever key the
   scan reached first.  Mass alone lets three overwhelming correspondences
   outrank a genuinely populated transformation, which is precisely the
   confusion 512 keypoints makes more likely rather than less. */
VoteTable.prototype.peak = function () {
  var bk = Infinity, bm = -1, bn = 0, bc = 0;
  for (var s = 0; s < this.key.length; s++) {
    var k = this.key[s];
    if (k === -1) continue;
    var m = this.mass[s], n = this.members[s], c = this.conf[s];
    if (m > bm || (m === bm && n > bn) || (m === bm && n === bn && c > bc) ||
        (m === bm && n === bn && c === bc && k < bk)) { bk = k; bm = m; bn = n; bc = c; }
  }
  return bm < 0 ? { key: Infinity, mass: 0, members: 0 } : { key: bk, mass: bm, members: bn };
};

function residual2(A, B, c, M) {
  var ax = A[c.i].x >> 4, ay = A[c.i].y >> 4;
  var mx = I.shr16(M.r00 * ax - M.r10 * ay) + M.tx;
  var my = I.shr16(M.r10 * ax + M.r00 * ay) + M.ty;
  var dx = mx - (B[c.j].x >> 4), dy = my - (B[c.j].y >> 4);
  return dx * dx + dy * dy;
}

function emptyVerify42(n) {
  return { inliers: 0, mask: new Array(n).fill(false), model: null,
           medianErr: Infinity, confSum: 0 };
}

/* §5–§6 — the weighted Hough, rebuilt.  Same shape as 4.1: soft bins, gather
   the winning cell's members, closed-form fit, one loose pass, one refit, one
   tight count.  What moved is underneath — the table, the vote weight, the peak
   comparison — plus the residual the model now reports about itself. */
function houghVerify42(A, B, corr, o, P, mdA, mdB) {
  if (corr.length < o.geoMinCorr) return emptyVerify42(corr.length);
  var soft = P.scaleSoft !== 0, table = new VoteTable(corr.length), per = [], i, t;
  for (i = 0; i < corr.length; i++) {
    var vc = voteCells42(A, B, corr[i], mdA, mdB, soft), w = cWeight(corr[i]);
    for (t = 0; t < vc.key.length; t++) table.add(vc.key[t], idiv(w * vc.kw[t], SK_NEAR), corr[i].c);
    per.push(vc.key);
  }
  var pk = table.peak();
  if (pk.key === Infinity) return emptyVerify42(corr.length);
  var members = [];
  for (i = 0; i < corr.length; i++) if (per[i].indexOf(pk.key) >= 0) members.push(corr[i]);
  if (members.length < P.minPeakMembers) return emptyVerify42(corr.length);
  var tol = Math.max(2, o.geoEps >> 4), tol2 = tol * tol;
  var M = I.fitSimilarity(A, B, members);
  if (!M) return emptyVerify42(corr.length);
  var r = I.countInliers(A, B, corr, M, idiv(tol2 * 9, 4));
  var inl = [];
  for (i = 0; i < corr.length; i++) if (r.mask[i]) inl.push(corr[i]);
  if (inl.length >= 2) { var M2 = I.fitSimilarity(A, B, inl); if (M2) M = M2; }
  r = I.countInliers(A, B, corr, M, tol2);
  var res = [], confSum = 0;
  for (i = 0; i < corr.length; i++) if (r.mask[i]) { res.push(residual2(A, B, corr[i], M)); confSum += cWeight(corr[i]); }
  res.sort(function (x, y) { return x - y; });
  return { inliers: r.n, mask: r.mask, model: M,
           medianErr: res.length ? res[res.length >> 1] : Infinity, confSum: confSum };
}

function round42(A, B, pool, o, P, mdA, mdB) {
  if (pool.length >= o.geoMinCorr) return houghVerify42(A, B, pool, o, P, mdA, mdB);
  return emptyVerify42(pool.length);
}

/* §7 — the exclusion radius around a consumed keypoint, in the 16-bit frame.
   A descriptor covers a 31 px patch AT ITS OWN LEVEL, so the footprint of the
   structure a keypoint stands for is PATCH_R / L_k in normalised units.  A
   second model reaching into that footprint is not finding a second paste, it
   is finding the first one again one keypoint over. */
function exclRadius(level, mdB, pct) {
  if (pct <= 0) return 0;
  var L = Math.max(1, I.levelDim(mdB, level));
  return clamp(idiv(65535 * PATCH_R42 * pct, L * 100), 0, 65535);
}

function extractFromPools42(A, AM, B, pd, pm, o, P, mdA, mdB) {
  pd = pd.slice(); pm = pm.slice();
  var out = { models: [], totalInliers: 0, corrDirect: pd.length, corrMirror: pm.length,
              inlierB: [], inlierA: [], inlierLevel: [] };
  for (var r = 0; r < P.maxModels; r++) {
    var floor = r === 0 ? o.geoMinCorr : P.minModelInliers;
    var vd = round42(A, B, pd, o, P, mdA, mdB);
    var vm = round42(AM, B, pm, o, P, mdA, mdB);
    /* §6 — winner per round on (inliers, -median residual, confidence); ties
       still go to the direct hypothesis, as in 4.1 */
    var useMirror = vm.inliers > vd.inliers ||
      (vm.inliers === vd.inliers && vm.inliers > 0 &&
       (vm.medianErr < vd.medianErr ||
        (vm.medianErr === vd.medianErr && vm.confSum > vd.confSum)));
    var v = useMirror ? vm : vd, pool = useMirror ? pm : pd;
    if (!v.model) break;
    if (v.inliers < floor) break;
    var consumed = [], i;
    for (i = 0; i < pool.length; i++) if (v.mask[i]) {
      consumed.push(pool[i].j);
      out.inlierB.push([B[pool[i].j].x, B[pool[i].j].y]);
      out.inlierA.push(pool[i].i);
      out.inlierLevel.push(B[pool[i].j].level);
    }
    out.totalInliers += v.inliers;
    out.models.push({ r00: v.model.r00, r10: v.model.r10, tx: v.model.tx, ty: v.model.ty,
                      scaleQ16: I.isqrt(v.model.r00 * v.model.r00 + v.model.r10 * v.model.r10),
                      mirror: useMirror, inliers: v.inliers,
                      medianErr: v.medianErr, confSum: v.confSum });
    /* §7 — consumption by B-side keypoint across BOTH pools, plus the soft
       exclusion neighbourhood.  This prunes the POOLS for later rounds; the
       underlying correspondence lists are untouched, so the measurement and its
       control still see the identical starting evidence. */
    var cset = new Set(consumed), pct = P.exclPct;
    var keep = function (c) {
      if (cset.has(c.j)) return false;
      if (pct <= 0) return true;
      var cx = B[c.j].x, cy = B[c.j].y;
      for (var q = 0; q < consumed.length; q++) {
        var rr = exclRadius(B[consumed[q]].level, mdB, pct);
        if (!rr) continue;
        var dx = cx - B[consumed[q]].x, dy = cy - B[consumed[q]].y;
        if (dx * dx + dy * dy <= rr * rr) return false;
      }
      return true;
    };
    pd = pd.filter(keep); pm = pm.filter(keep);
  }
  return out;
}

function extract42(A, B, o, P, xmaxA, mdA, mdB) {
  var AM = I.mirrorSide(A, xmaxA);
  var pd = correspond42(A, B, P);
  var pm = o.mirrorHypothesis ? correspond42(AM, B, P) : [];
  return extractFromPools42(A, AM, B, pd, pm, o, P, mdA, mdB);
}

function shift42(c, p) {
  var n = c.length, out = new Array(n);
  for (var i = 0; i < n; i++) out[i] = { i: c[i].i, j: c[(i + p) % n].j, d: c[i].d, c: c[i].c, sc: c[i].sc };
  return out;
}
function reverse42(c) {
  var n = c.length, out = new Array(n);
  for (var i = 0; i < n; i++) out[i] = { i: c[i].i, j: c[n - 1 - i].j, d: c[i].d, c: c[i].c, sc: c[i].sc };
  return out;
}
function shiftOrEmpty42(c, num, den) {
  var n = c.length;
  if (!n) return [];
  var p = idiv(n * num, den);
  if (p === 0 || p >= n) return [];   /* degenerate → empty, never unpermuted (§9.3) */
  return shift42(c, p);
}
function reverseOrEmpty42(c) { return c.length < 2 ? [] : reverse42(c); }

/* §A1 — the weak signal, unchanged in substance. */
function weakInliers42(A, AM, B, pd, pm, o, P, mdA, mdB) {
  var vd = pd.length >= o.geoMinCorr ? houghVerify42(A, B, pd, o, P, mdA, mdB).inliers : 0;
  var vm = pm.length >= o.geoMinCorr ? houghVerify42(AM, B, pm, o, P, mdA, mdB).inliers : 0;
  return Math.max(vd, vm);
}
function geoMeasure42(A, AM, B, pd, pm, o, P, mdA, mdB) {
  var mm = extractFromPools42(A, AM, B, pd, pm, o, P, mdA, mdB);
  if (mm.totalInliers > 0) return { mm: mm, measure: mm.totalInliers, weak: 0 };
  var w = weakInliers42(A, AM, B, pd, pm, o, P, mdA, mdB);
  return { mm: mm, measure: w, weak: w };
}
/* §9.3 — the GN control: the same five geometric permutations, each running
   the IDENTICAL measurement, aggregated by MAX.  The question the null answers
   has not changed and should not: how much better is the real correspondence
   set than the best structured accident? */
function gnControl42(A, AM, B, pd0, pm0, o, P, mdA, mdB) {
  var shifts = [['gn:half', 1, 2], ['gn:third', 1, 3], ['gn:fifth', 1, 5], ['gn:twothirds', 2, 3]];
  var best = 0, member = 'gn:none', s;
  var measure = function (pd, pm) { return geoMeasure42(A, AM, B, pd, pm, o, P, mdA, mdB).measure; };
  for (s = 0; s < shifts.length; s++) {
    var pd = shiftOrEmpty42(pd0, shifts[s][1], shifts[s][2]);
    var pm = shiftOrEmpty42(pm0, shifts[s][1], shifts[s][2]);
    if (!pd.length && !pm.length) continue;
    var t = measure(pd, pm);
    if (t > best) { best = t; member = shifts[s][0]; }
  }
  var rd = reverseOrEmpty42(pd0), rm = reverseOrEmpty42(pm0);
  if (rd.length || rm.length) {
    var tr = measure(rd, rm);
    if (tr > best) { best = tr; member = 'gn:reverse'; }
  }
  return { ctl: best, member: member };
}

/* §8 — how much INDEPENDENT evidence the geometry actually rests on.
 *
 * This is the channel 512 keypoints make necessary rather than merely nice.
 * Doubling the budget doubles the matches a repeated texture can produce
 * without adding one independent observation, and inlier count alone cannot
 * tell those two situations apart.  Four readings, all in the 0..10000
 * currency, weighted exactly as the §3 selection score weights its four terms:
 *
 *     D = 40 spatial + 25 scale + 20 model + 15 descriptor
 */
function diversity42(mm, A, B, P) {
  var cov = coverage(mm.inlierB, P.gridG), i;
  if (!mm.models.length) {
    /* No accepted model means no geometric evidence to modulate.  The §A1 weak
       signal reaches the margin through a path that never produced a model, and
       multiplying it down here would punish the same absence twice. */
    return { div: { spatial: 0, scale: 0, model: 0, descriptor: 0, combined: 0, multiplier: SCALE },
             coverage: cov };
  }
  var spatial = cov.coverage;
  var avail = new Set(), used = new Set();
  for (i = 0; i < B.length; i++) avail.add(B[i].level);
  for (i = 0; i < mm.inlierLevel.length; i++) used.add(mm.inlierLevel[i]);
  var scale = clamp(idiv(used.size * SCALE, Math.max(1, avail.size)), 0, SCALE);
  var model = clamp(idiv(mm.models.length * SCALE, Math.max(1, P.maxModels)), 0, SCALE);
  /* descriptor: greedy independent clusters over the inliers' A descriptors, in
     extraction order.  A cluster head is a descriptor further than
     DESC_NOVEL_AT from every head already found. */
  var heads = [];
  for (i = 0; i < mm.inlierA.length; i++) {
    var ia = mm.inlierA[i];
    if (ia >= A.length) continue;
    var d = A[ia].desc, novel = true;
    for (var h = 0; h < heads.length; h++)
      if (I.hamDesc(heads[h], d) <= DESC_NOVEL_AT42) { novel = false; break; }
    if (novel) heads.push(d);
  }
  var descriptor = clamp(idiv(heads.length * SCALE, Math.max(1, mm.inlierA.length)), 0, SCALE);
  var combined = idiv(40 * spatial + 25 * scale + 20 * model + 15 * descriptor, 100);
  return { div: { spatial: spatial, scale: scale, model: model, descriptor: descriptor,
                  combined: combined, multiplier: lutEval(P.lutGeoDiversity, combined) },
           coverage: cov };
}

/* §13 — the five topology classes, unchanged. */
function topology42(mm, cov, g, dominantAt, geoMinCorr) {
  if (mm.models.length >= 2) return 3;
  if (mm.models.length === 1)
    return idiv(cov.bboxCells * SCALE, g * g) >= dominantAt ? 4 : 2;
  if (Math.max(mm.corrDirect, mm.corrMirror) >= geoMinCorr) return 1;
  return 0;
}

/* ================================================================== entry
 * §8.1, §15, §24 — compare_v4: profile binding, live Indeterminate paths,
 * canonical argument order, the v4 verdict with the full v3 reading aboard. */
var R_CORRUPT = 'CORRUPT', R_PROFILE_MISMATCH = 'PROFILE_MISMATCH',
    R_PROFILE_UNSUPPORTED = 'PROFILE_UNSUPPORTED', R_LIMIT = 'LIMIT';
var CHANNEL_ORDER = ['dct', 'local', 'shape', 'topology', 'runs', 'palette', 'silhouette'];
function indeterminate(reasons, P) {
  return { comparator: COMPARATOR, verdict: 'Indeterminate', class: '', basis: [], reasons: reasons,
           structural: 0, certifiable: false, v3: null, local: null, models: [], topology: 0,
           totalInliers: 0, coverage: null, geometryEvidence: 0, geoMeasurable: false,
           geoRaw: 0, geoCtl: 0, geoMargin: 0, geoCtlMember: 'none',
           swapped: false, calibration: profileName(P), calibrationId: profileIdHex16(P) };
}
function bind(opts, P) {
  var o = {}, k;
  for (k in wire.DEFAULTS) o[k] = wire.DEFAULTS[k];
  if (opts) for (k in opts) if (opts[k] !== undefined) o[k] = opts[k];
  o.hammingT = P.hammingT; o.confidenceAt = P.confidenceAt;
  o.geoConfAt = P.geoConfAt; o.geoEps = P.geoEps; o.geoMinCorr = P.geoMinCorr;
  o.evidence = 'lift'; o.scoring = 'weighted'; o.ragEndpoint = 'rank';
  return o;
}
/* ================================================================ v4.1
 * SPEC-004.1 (comparator 41): A1 weak-signal geometry, A2 sparse local
 * assignment, A3 per-channel tables, A4 the stage-1 screen.  Comparator 4
 * above stays frozen; each refuses the other's profiles. */
function screenV41(aT1, aT2, bT1, bT2, opts, P) {
  var o = bind(opts, P);
  var A1 = u8(aT1), B1 = u8(bT1), swapped = false;
  for (var si = 0; si < Math.min(A1.length, B1.length); si++) {
    if (A1[si] !== B1[si]) { swapped = A1[si] > B1[si]; break; }
  }
  var cA1 = swapped ? bT1 : aT1, cA2 = swapped ? bT2 : aT2;
  var cB1 = swapped ? aT1 : bT1, cB2 = swapped ? aT2 : bT2;
  var dA, dB;
  try { dA = wire.parseT1(cA1); dB = wire.parseT1(cB1); }
  catch (e) { return { pass: false, poolDirect: 0, poolMirror: 0 }; }
  var kA, kB;
  try { kA = cA2 ? wire.parseT2(cA2).list : I.readSketch(dA); }
  catch (e) { kA = I.readSketch(dA); }
  try { kB = cB2 ? wire.parseT2(cB2).list : I.readSketch(dB); }
  catch (e) { kB = I.readSketch(dB); }
  var xmaxA = clamp(idiv((dA.width - 1) * 65535, Math.max(1, dA.maxDim)), 0, 65535);
  var AM = I.mirrorSide(kA, xmaxA);
  var pd = correspondW(kA, kB);
  var pm = o.mirrorHypothesis ? correspondW(AM, kB) : [];
  return { pass: Math.max(pd.length, pm.length) >= o.geoMinCorr,
           poolDirect: pd.length, poolMirror: pm.length };
}

function compareV41(aT1, aT2, bT1, bT2, opts, P, hpA, hpB) {
  var bad = false;
  try { profileValidate(P); } catch (e) { bad = true; }
  if (bad || P.comparator !== COMPARATOR_V41) {
    var ri = indeterminate([R_PROFILE_UNSUPPORTED], P);
    ri.comparator = COMPARATOR_V41; ri.geoWeakInliers = 0; ri.screen = null;
    return ri;
  }
  if (hpA && hpB) {
    var same = hpA.length === 32 && hpB.length === 32;
    if (same) for (var hi = 0; hi < 32; hi++) if (hpA[hi] !== hpB[hi]) { same = false; break; }
    if (!same) {
      var rm2 = indeterminate([R_PROFILE_MISMATCH], P);
      rm2.comparator = COMPARATOR_V41; rm2.geoWeakInliers = 0; rm2.screen = null;
      return rm2;
    }
  }
  var o = bind(opts, P);
  var v3;
  try {
    v3 = wire.compare({ t1: aT1, t2: aT2 || undefined }, { t1: bT1, t2: bT2 || undefined }, o);
  } catch (e) {
    var rc = indeterminate([R_CORRUPT], P);
    rc.comparator = COMPARATOR_V41; rc.geoWeakInliers = 0; rc.screen = null;
    return rc;
  }

  var A1 = u8(aT1), B1 = u8(bT1), swapped = false;
  for (var si = 0; si < Math.min(A1.length, B1.length); si++) {
    if (A1[si] !== B1[si]) { swapped = A1[si] > B1[si]; break; }
  }
  var cA1 = swapped ? bT1 : aT1, cA2 = swapped ? bT2 : aT2;
  var cB1 = swapped ? aT1 : bT1, cB2 = swapped ? aT2 : bT2;

  var dA = wire.parseT1(cA1), dB = wire.parseT1(cB1);
  var loc = localV4_41(dA, dB, P);                       /* A2 */
  var kA = cA2 ? wire.parseT2(cA2).list : I.readSketch(dA);
  var kB = cB2 ? wire.parseT2(cB2).list : I.readSketch(dB);
  var mdA = dA.maxDim, mdB = dB.maxDim;
  var xmaxA = clamp(idiv((dA.width - 1) * 65535, Math.max(1, mdA)), 0, 65535);

  var AM = I.mirrorSide(kA, xmaxA);
  var pd = correspondW(kA, kB);
  var pm = o.mirrorHypothesis ? correspondW(AM, kB) : [];
  var mmi = P.minModelInliers, maxm = P.maxModels;
  var g41 = geoMeasure41(kA, AM, kB, pd, pm, o, mdA, mdB, mmi, maxm);  /* A1 */
  var mm = g41.mm;
  var topo = topology(mm, P.gridG, P.thresholds[8], o.geoMinCorr);
  var gn = gnControl41(kA, AM, kB, pd, pm, o, mdA, mdB, mmi, maxm);

  var geoMeasurable = kA.length >= o.geoMinCorr && kB.length >= o.geoMinCorr;
  var raw = clamp(idiv(g41.measure * SCALE, o.geoConfAt), 0, SCALE);
  var ctl = clamp(idiv(gn.ctl * SCALE, o.geoConfAt), 0, SCALE);
  var geoMargin = I.chanceCorrect(raw, ctl);
  var geometryEvidence = lutEval(P.lutGeometry, geoMargin);

  var channels = [];
  for (var ci = 0; ci < 7; ci++) {
    var name = CHANNEL_ORDER[ci], ch = v3.channels[name];
    channels.push(name === 'local'
      ? [name, loc.evidence, loc.measurable]
      : [name, lutEval(lutChannel(P, name), ch.value), ch.measurable]);  /* A3 */
  }
  var verdict = latticeV4({
    identical: v3.identical, channels: channels,
    geoMeasurable: geoMeasurable, geoEvidence: geometryEvidence,
    totalInliers: g41.measure, topologyClass: topo.class,
    diversity: loc.diversity,
    coverageMin: Math.min(loc.coverageA.coverage, loc.coverageB.coverage),
    anyMirrorModel: mm.models.some(function (m) { return m.mirror; })
  }, P);

  if (swapped) {
    var tc = loc.coverageA; loc.coverageA = loc.coverageB; loc.coverageB = tc;
    var td = loc.dA; loc.dA = loc.dB; loc.dB = td;
    loc.pairs = loc.pairs.map(function (p) { return [p[1], p[0]]; });
  }

  return { comparator: COMPARATOR_V41, verdict: verdict.state, class: verdict.class,
           basis: verdict.basis, reasons: [], structural: verdict.structural,
           certifiable: verdict.certifiable, v3: v3, local: loc, models: mm.models,
           topology: topo.class, totalInliers: g41.measure, coverage: topo.coverage,
           geometryEvidence: geometryEvidence, geoMeasurable: geoMeasurable,
           geoRaw: raw, geoCtl: ctl, geoMargin: geoMargin,
           geoCtlMember: gn.member, swapped: swapped,
           calibration: profileName(P), calibrationId: profileIdHex16(P),
           geoWeakInliers: g41.weak, screen: null };
}


/* ================================================================= v4.2
 * SPEC-004.2 (comparator 42).  Everything comparator 41 is — A1 weak-signal
 * geometry, A2 the sparse local assignment, A3 per-channel calibration tables,
 * A4 the stage-1 screen — on a doubled keypoint budget, through the rebuilt
 * geometry stage above, with one new evidence channel (§8 diversity).
 *
 * What did NOT change is the part that decides.  The lattice is comparator
 * 41's, rule for rule; the null family is the same five permutations under MAX;
 * proportion and Gate stay out of the decision path, where 41 already put them.
 * A larger budget is a reason to measure better, not a reason to re-argue what
 * a copy is.  Comparator 41 above stays frozen and computable, because a
 * verdict that has been issued has to stay reproducible. */
function canon42(aT1, bT1) {
  var A1 = u8(aT1), B1 = u8(bT1);
  for (var i = 0; i < Math.min(A1.length, B1.length); i++)
    if (A1[i] !== B1[i]) return A1[i] > B1[i];
  return false;
}

function screenV42(aT1, aT2, bT1, bT2, opts, P) {
  var o = bind(opts, P), swapped = canon42(aT1, bT1);
  var cA1 = swapped ? bT1 : aT1, cA2 = swapped ? bT2 : aT2;
  var cB1 = swapped ? aT1 : bT1, cB2 = swapped ? aT2 : bT2;
  var dA, dB;
  try { dA = wire.parseT1(cA1); dB = wire.parseT1(cB1); }
  catch (e) { return { pass: false, poolDirect: 0, poolMirror: 0 }; }
  var kA, kB;
  try { kA = cA2 ? wire.parseT2(cA2).list : I.readSketch(dA); } catch (e) { kA = I.readSketch(dA); }
  try { kB = cB2 ? wire.parseT2(cB2).list : I.readSketch(dB); } catch (e) { kB = I.readSketch(dB); }
  var xmaxA = clamp(idiv((dA.width - 1) * 65535, Math.max(1, dA.maxDim)), 0, 65535);
  var AM = I.mirrorSide(kA, xmaxA);
  var pd = correspond42(kA, kB, P);
  var pm = o.mirrorHypothesis ? correspond42(AM, kB, P) : [];
  return { pass: Math.max(pd.length, pm.length) >= o.geoMinCorr,
           poolDirect: pd.length, poolMirror: pm.length };
}

function indeterminate42(reasons, P) {
  var r = indeterminate(reasons, P);
  r.comparator = COMPARATOR_V42;
  r.geoWeakInliers = 0; r.screen = null;
  r.diversity = { spatial: 0, scale: 0, model: 0, descriptor: 0, combined: 0, multiplier: SCALE };
  r.medianErr = []; r.selection = { a: 0, b: 0, mixed: false }; r.kpA = 0; r.kpB = 0;
  return r;
}

function compareV42(aT1, aT2, bT1, bT2, opts, P, hpA, hpB) {
  var bad = false;
  try { profileValidate(P); } catch (e) { bad = true; }
  if (bad || P.comparator !== COMPARATOR_V42) return indeterminate42([R_PROFILE_UNSUPPORTED], P);
  if (hpA && hpB) {
    var same = hpA.length === 32 && hpB.length === 32;
    if (same) for (var hi = 0; hi < 32; hi++) if (hpA[hi] !== hpB[hi]) { same = false; break; }
    if (!same) return indeterminate42([R_PROFILE_MISMATCH], P);
  }
  var o = bind(opts, P), v3;
  try { v3 = wire.compare({ t1: aT1, t2: aT2 || undefined }, { t1: bT1, t2: bT2 || undefined }, o); }
  catch (e) { return indeterminate42([R_CORRUPT], P); }

  /* P4 — canonical argument order, the rule verbatim from 4 and 4.1. */
  var swapped = canon42(aT1, bT1);
  var cA1 = swapped ? bT1 : aT1, cA2 = swapped ? bT2 : aT2;
  var cB1 = swapped ? aT1 : bT1, cB2 = swapped ? aT2 : bT2;
  var dA = wire.parseT1(cA1), dB = wire.parseT1(cB1);
  var loc = localV4_41(dA, dB, P);                       /* A2, unchanged */

  var t2a = cA2 ? wire.parseT2(cA2) : null, t2b = cB2 ? wire.parseT2(cB2) : null;
  var kA = t2a ? t2a.list : I.readSketch(dA), selA = t2a ? t2a.select : 0;
  var kB = t2b ? t2b.list : I.readSketch(dB), selB = t2b ? t2b.select : 0;
  var mdA = dA.maxDim, mdB = dB.maxDim;
  var xmaxA = clamp(idiv((dA.width - 1) * 65535, Math.max(1, mdA)), 0, 65535);

  var AM = I.mirrorSide(kA, xmaxA);
  var pd = correspond42(kA, kB, P);
  var pm = o.mirrorHypothesis ? correspond42(AM, kB, P) : [];
  var g42 = geoMeasure42(kA, AM, kB, pd, pm, o, P, mdA, mdB);   /* A1 */
  var mm = g42.mm;
  var dv = diversity42(mm, kA, kB, P);                          /* §8 */
  var topo = topology42(mm, dv.coverage, P.gridG, P.thresholds[8], o.geoMinCorr);
  var gn = gnControl42(kA, AM, kB, pd, pm, o, P, mdA, mdB);

  var geoMeasurable = kA.length >= o.geoMinCorr && kB.length >= o.geoMinCorr;
  var raw = clamp(idiv(g42.measure * SCALE, o.geoConfAt), 0, SCALE);
  var ctl = clamp(idiv(gn.ctl * SCALE, o.geoConfAt), 0, SCALE);
  var geoMargin = I.chanceCorrect(raw, ctl);
  /* §8 — the calibrated margin, modulated by how independent the evidence is */
  var geometryEvidence = clamp(idiv(lutEval(P.lutGeometry, geoMargin) * dv.div.multiplier, SCALE), 0, SCALE);

  var channels = [];
  for (var ci = 0; ci < 7; ci++) {
    var name = CHANNEL_ORDER[ci], ch = v3.channels[name];
    channels.push(name === 'local'
      ? [name, loc.evidence, loc.measurable]
      : [name, lutEval(lutChannel(P, name), ch.value), ch.measurable]);  /* A3 */
  }
  var verdict = latticeV4({
    identical: v3.identical, channels: channels,
    geoMeasurable: geoMeasurable, geoEvidence: geometryEvidence,
    totalInliers: g42.measure, topologyClass: topo,
    diversity: loc.diversity,
    coverageMin: Math.min(loc.coverageA.coverage, loc.coverageB.coverage),
    anyMirrorModel: mm.models.some(function (m) { return m.mirror; })
  }, P);

  if (swapped) {
    var tc = loc.coverageA; loc.coverageA = loc.coverageB; loc.coverageB = tc;
    var td = loc.dA; loc.dA = loc.dB; loc.dB = td;
    loc.pairs = loc.pairs.map(function (q) { return [q[1], q[0]]; });
  }

  return { comparator: COMPARATOR_V42, verdict: verdict.state, class: verdict.class,
           basis: verdict.basis, reasons: [], structural: verdict.structural,
           certifiable: verdict.certifiable, v3: v3, local: loc, models: mm.models,
           topology: topo, totalInliers: g42.measure, coverage: dv.coverage,
           geometryEvidence: geometryEvidence, geoMeasurable: geoMeasurable,
           geoRaw: raw, geoCtl: ctl, geoMargin: geoMargin,
           geoCtlMember: gn.member, swapped: swapped,
           calibration: profileName(P), calibrationId: profileIdHex16(P),
           geoWeakInliers: g42.weak, diversity: dv.div,
           medianErr: mm.models.map(function (m) { return m.medianErr; }),
           /* Not an error and not a refusal: both wires are valid and the
              comparison is well defined.  It is a WARNING, because a 4.1 side
              brings 256 strength-ranked keypoints to a 4.2 side's 512
              quality-ranked ones, and recall measured across that boundary is
              not recall measured within either.  Re-hash a corpus before you
              re-derive its thresholds. */
           selection: { a: swapped ? selB : selA, b: swapped ? selA : selB,
                        mixed: selA !== selB },
           kpA: swapped ? kB.length : kA.length, kpB: swapped ? kA.length : kB.length,
           screen: null };
}

/* §16 — hash-time limits.  Profiles may lower, never raise. */
var MAX_WIDTH = 16384, MAX_HEIGHT = 16384, MAX_PIXELS = 1 << 24;
function hashChecked(img, opts, limits) {
  var mw = MAX_WIDTH, mh = MAX_HEIGHT, mp = MAX_PIXELS;
  if (limits) {
    mw = Math.min(mw, Math.max(1, limits[0]));
    mh = Math.min(mh, Math.max(1, limits[1]));
    mp = Math.min(mp, Math.max(1, limits[2]));
  }
  if (!img || !img.w || !img.h) throw new RangeError('limit: empty image');
  if (img.px.length !== img.w * img.h * 4) throw new RangeError('limit: pixel buffer length mismatch');
  if (img.w > mw) throw new RangeError('limit: width exceeds maximum');
  if (img.h > mh) throw new RangeError('limit: height exceeds maximum');
  if (img.w * img.h > mp) throw new RangeError('limit: pixel count exceeds maximum');
  return wire.hash(img, opts);
}

return {
  /* ------------------------------------------------------------------ 4.2
   * One comparator on the entry, one container, one shipped calibration.
   * Comparator 41 is FROZEN, not deleted — SPEC-004.2 keeps it computable
   * under `compare41`/`cal41` so a verdict that has been issued stays
   * reproducible.  Comparator 4 is neither: a container-1 artefact still
   * DECODES, so an old verdict's provenance can be read, but nothing in this
   * package will compute with it. */
  VERSION: '4.2', COMPARATOR: COMPARATOR_V42, CONTAINER: CONTAINER_V3, backend: 'js',

  hash: hashChecked, compare: compareV42, screen: screenV42,
  cal: cal004, calibration: cal004,

  /* frozen, for reproducing verdicts issued under 4.1 */
  compare41: compareV41, screen41: screenV41, cal41: cal003,
  COMPARATOR_V41: COMPARATOR_V41, CONTAINER_V2: CONTAINER_V2,

  profileEncode: profileEncode, profileDecode: profileDecode, profileValidate: profileValidate,
  profileId: profileId, profileIdHex16: profileIdHex16, profileName: profileName,
  legacyCal001: legacyCal001,

  localChannel: localV4_41, localBags: localV4Bags41, localBagsFull: localV4Bags,
  lattice: latticeV4, topology: topology, coverage: coverage, cell: cell,
  correspondW: correspondW, extract: extract, extractFromPools: extractFromPools,
  gnControl: gnControl41, conf: conf,

  /* SPEC-004.2 §4–§8 */
  correspond42: correspond42, extract42: extract42, extractFromPools42: extractFromPools42,
  gnControl42: gnControl42, geoMeasure42: geoMeasure42, diversity42: diversity42,
  topology42: topology42, hamCut: hamCut, strengthCompat: strengthCompat,
  voteCells42: voteCells42, VoteTable: VoteTable,
  selectQuality: wire._internal.selectQuality || null,

  assign: assign, assignSparse: assignSparse, NO_EDGE: NO_EDGE,
  rot64: rot64, bitrev64: bitrev64, LOCAL_FAMILY: LOCAL_FAMILY, applyLocal: applyLocal,
  shiftOffsets: shiftOffsets, applyShift: applyShift, applyReverse: applyReverse,
  lutIdentity: lutIdentity, lutNeutral: lutNeutral, lutValidate: lutValidate,
  lutEval: lutEval, lutChannel: lutChannel,
  sha256: sha256, hex: hex, hashProfileId: hashProfileId,

  MAX_WIDTH: MAX_WIDTH, MAX_HEIGHT: MAX_HEIGHT, MAX_PIXELS: MAX_PIXELS,
  R_CORRUPT: R_CORRUPT, R_PROFILE_MISMATCH: R_PROFILE_MISMATCH,
  R_PROFILE_UNSUPPORTED: R_PROFILE_UNSUPPORTED, R_LIMIT: R_LIMIT,
  _internal: { assignBags: assignBags, assignBagsWith: assignBagsWith, bitrevBag: bitrevBag,
               houghVerifyW: houghVerifyW, weakInliers: weakInliers,
               geoMeasure41: geoMeasure41, gnControl41: gnControl41, wire: wire,
               houghVerify42: houghVerify42, weakInliers42: weakInliers42,
               exclRadius: exclRadius, residual2: residual2, cWeight: cWeight }
};
});
