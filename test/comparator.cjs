/* PAPH 4.1 conformance harness (SPEC-004.1, §20 vectors).
 * Order is the golden file's order: the port consumed these vectors FIRST and
 * was written against them; this file keeps it that way.  Behavioural
 * end-to-end checks (verdicts on real wires) come after the vectors, and
 * cross-engine equality on full reports lives in test/parity-native.mjs (native)
 * and test/wasm-parity.mjs (WebAssembly). */
'use strict';
const fs = require('node:fs');
const path = require('node:path');
const wire = require('../src/wire.cjs');
const paph = require('../src/paph-js.cjs');
const cal = require('../demo/cal-core.cjs');

let pass = 0, fail = 0;
function ok(name, cond, extra) {
  if (cond) { pass++; console.log('  \x1b[32mPASS\x1b[0m ' + name + (extra ? '  ' + extra : '')); }
  else { fail++; console.log('  \x1b[31mFAIL\x1b[0m ' + name + (extra ? '  ' + extra : '')); }
}
function head(s) { console.log('\n\x1b[1m' + s + '\x1b[0m'); }
function deq(a, b) { return JSON.stringify(a) === JSON.stringify(b); }

const G = JSON.parse(fs.readFileSync(path.join(__dirname, '..', 'docs', 'golden', 'GOLDEN-004.json'), 'utf8'));

/* ---------- vectors, section by section ---------- */
head('golden: header');
ok('golden vectors declare the comparator they were emitted under',
   G.spec === 'PAPH-SPEC-004.2' && G.format === 3 && G.comparator === 42 && G.container === 3);
ok('the entry computes 4.2', paph.COMPARATOR === 42 && paph.CONTAINER === 3 && paph.VERSION === '4.2');
ok('and 41 stays frozen but reachable',
   typeof paph.compare41 === 'function' && paph.cal41().comparator === 41);

head('golden: sha256');
for (const t of G.sha256)
  ok('sha256("' + t.msg.slice(0, 12) + (t.msg.length > 12 ? '…' : '') + '")',
     paph.hex(paph.sha256(Buffer.from(t.msg, 'ascii'))) === t.digest);

head('golden: hash-profile identity');
/* GOLDEN-004 is format 3's file: its identity is the wire-3 defaults' (1.2's
   wire-4 identity is GOLDEN-W4's, checked by test/wire4-golden.cjs) */
ok('hash_profile_id(defaults, wire 3)', paph.hex(paph.hashProfileId(Object.assign({}, wire.DEFAULT_CONFIG, { wire: 3 }))) === G.hash_profile_id_default);

head('golden: CAL-001-PROVISIONAL');
{
  const p = paph.legacyCal001();
  const bytes = paph.profileEncode(p);
  /* a container-1 artefact still DECODES — that is the audit path — but the
     comparator refuses to compute with it, which the next check asserts */
  ok('name', paph.profileName(p) === G.profile_cal001.name);
  ok('artefact bytes', paph.hex(bytes) === G.profile_cal001.bytes, bytes.length + ' B');
  ok('length', bytes.length === G.profile_cal001.len);
  ok('id', paph.hex(paph.profileId(p)) === G.profile_cal001.id);
  const q = paph.profileDecode(bytes);
  ok('decode∘encode round-trips', paph.hex(paph.profileEncode(q)) === G.profile_cal001.bytes);
  let threw = false;
  try { paph.profileValidate(q); } catch (e) { threw = true; }
  ok('decoded profile validates', !threw);
  const bad = Uint8Array.from(bytes); bad[6] = 3;
  threw = false;
  try { paph.profileDecode(bad); } catch (e) { threw = true; }
  ok('wrong comparator rejected', threw);
}

head('golden: LUT evaluation');
{
  const pts = G.lut.points;
  let all = true;
  for (let i = 0; i < G.lut.x.length; i++)
    if (paph.lutEval(pts, G.lut.x[i]) !== G.lut.y[i]) all = false;
  ok('shaped LUT at ' + G.lut.x.length + ' probes', all);
}

head('golden: assignment');
for (const c of G.assignment) {
  const got = paph.assign(c.n, c.m, c.cost);
  const total = got.reduce((s, p) => s + c.cost[p[0] * c.m + p[1]], 0);
  ok(c.n + 'x' + c.m, deq(got, c.pairs) && total === c.total,
     got.length + ' pairs, cost ' + total);
}

head('golden: LN maps');
for (const t of G.nulls_local) {
  const hi = parseInt(t.code.slice(0, 8), 16) | 0, lo = parseInt(t.code.slice(8, 16), 16) | 0;
  let all = true;
  for (const m of paph.LOCAL_FAMILY) {
    const r = paph.applyLocal(m, hi, lo);
    const hx = (r[0] >>> 0).toString(16).padStart(8, '0') + (r[1] >>> 0).toString(16).padStart(8, '0');
    if (hx !== t[m]) all = false;
  }
  ok('code ' + t.code, all);
}

head('golden: GN offsets and permutations');
{
  let all = true;
  for (const n of Object.keys(G.shift_offsets))
    if (!deq(paph.shiftOffsets(+n), G.shift_offsets[n])) all = false;
  ok('shift offsets', all);
  const corr = G.shift_apply.b.map((j, k) => ({ i: k, j: j, d: k }));
  ok('shift by 3', deq(paph.applyShift(corr, 3).map(c => c.j), G.shift_apply.shift3_b));
  ok('reversal', deq(paph.applyReverse(corr).map(c => c.j), G.shift_apply.reverse_b));
}

head('golden: coverage');
for (const t of G.coverage) {
  const pts = [];
  for (let i = 0; i < t.points.length; i += 2) pts.push([t.points[i], t.points[i + 1]]);
  const c = paph.coverage(pts, t.g);
  ok(t.name, c.occupied === t.occupied && c.coverage === t.coverage &&
             c.bboxCells === t.bbox_cells && c.concentration === t.concentration);
}

head('golden: local channel (§10)');
function bagOf(b) {
  const n = b.codes.length;
  const X = { hi: new Int32Array(n), lo: new Int32Array(n),
              x: Int32Array.from(b.x), y: Int32Array.from(b.y), n: n };
  for (let i = 0; i < n; i++) {
    X.hi[i] = parseInt(b.codes[i].slice(0, 8), 16) | 0;
    X.lo[i] = parseInt(b.codes[i].slice(8, 16), 16) | 0;
  }
  return X;
}
{
  const P = paph.legacyCal001();   /* these vectors were emitted under the frozen path */
  const KEYMAP = { matches: 'matches', w: 'w', cap: 'cap', ctl_w: 'ctlW', ctl_n: 'ctlN',
                   ctl_member: 'ctlMember', lift_raw: 'liftRaw', lift_ctl: 'liftCtl',
                   margin: 'margin', evidence: 'evidence', prop_raw: 'propRaw',
                   prop_ctl: 'propCtl', diversity: 'diversity', d_a: 'dA', d_b: 'dB' };
  for (const t of G.local_v4) {
    const r = paph.localBagsFull(bagOf(t.a), bagOf(t.b), P);
    let bad = [];
    for (const k of Object.keys(KEYMAP))
      if (r[KEYMAP[k]] !== t.expect[k]) bad.push(k + '=' + r[KEYMAP[k]] + '≠' + t.expect[k]);
    if (!deq(r.pairs, t.expect.pairs)) bad.push('pairs');
    if (r.coverageA.occupied !== t.expect.coverage_a_occupied) bad.push('covA.occupied');
    if (r.coverageA.concentration !== t.expect.coverage_a_concentration) bad.push('covA.concentration');
    ok(t.name, bad.length === 0, bad.join(' '));
  }
}

head('golden: confidence (§12.2)');
{
  let all = true;
  for (const t of G.conf) if (paph.conf(t.d1, t.d2) !== t.conf) all = false;
  ok('conf table (' + G.conf.length + ' entries)', all);
}

head('golden: lattice (§14)');
for (const t of G.lattice) {
  const r = paph.lattice({
    identical: t.in.identical, channels: t.in.channels,
    geoMeasurable: t.in.geo_measurable, geoEvidence: t.in.geo_evidence,
    totalInliers: t.in.total_inliers, topologyClass: t.in.topology,
    diversity: t.in.diversity, coverageMin: t.in.coverage_min,
    anyMirrorModel: t.in.mirror
  }, paph.legacyCal001());
  ok(t.name, r.state === t.expect.state && deq(r.basis, t.expect.basis) &&
             r.structural === t.expect.structural && r.certifiable === t.expect.certifiable,
     r.state + ' [' + r.basis.join(',') + ']');
}

head('golden: limits (§16)');
ok('constants', paph.MAX_WIDTH === G.limits.max_width && paph.MAX_HEIGHT === G.limits.max_height &&
                paph.MAX_PIXELS === G.limits.max_pixels);

/* ---------- end-to-end on real wires ---------- */
function work(w, h, seed) {
  const px = new Uint8Array(w * h * 4);
  let s = seed;
  const R = () => { s = (s * 1103515245 + 12345) & 0x7fffffff; return s / 0x7fffffff; };
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const o = (y * w + x) << 2;
    px[o] = (x * 7 + y * 3 + seed) % 251; px[o + 1] = (y * 11 + x * 5) % 253;
    px[o + 2] = ((x ^ y) * 13) % 247; px[o + 3] = 255;
  }
  for (let i = 0; i < 40; i++) {
    const bx = (R() * (w - 10)) | 0, by = (R() * (h - 10)) | 0;
    const r = (R() * 255) | 0, g = (R() * 255) | 0, b = (R() * 255) | 0;
    for (let dy = 0; dy < 6; dy++) for (let dx = 0; dx < 6; dx++) {
      const o = ((by + dy) * w + bx + dx) << 2;
      px[o] = r; px[o + 1] = g; px[o + 2] = b;
    }
  }
  return { px, w, h };
}
function mirrorIm({ px, w, h }) {
  const o = new Uint8Array(w * h * 4);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
    const s = (y * w + (w - 1 - x)) << 2, d = (y * w + x) << 2;
    o[d] = px[s]; o[d + 1] = px[s + 1]; o[d + 2] = px[s + 2]; o[d + 3] = px[s + 3];
  }
  return { px: o, w, h };
}

head('end-to-end: verdicts');
{
  const P = paph.cal();
  const A = paph.hash(work(128, 128, 42));
  const B = paph.hash(work(128, 128, 1337));
  const rs = paph.compare(A.t1, A.t2, A.t1, A.t2, {}, P);
  ok('self is Identical', rs.verdict === 'Identical' && rs.structural === 10000 && deq(rs.basis, ['bytes']));
  ok('self local evidence', rs.local.measurable && rs.local.evidence > 7000, String(rs.local.evidence));
  ok('self geometry', rs.models.length > 0 && rs.geometryEvidence > 5000 && rs.topology >= 2,
     rs.totalInliers + ' inliers, evidence ' + rs.geometryEvidence);
  const ru = paph.compare(A.t1, A.t2, B.t1, B.t2, {}, P);
  ok('unrelated is not Copy/Identical', ru.verdict !== 'Copy' && ru.verdict !== 'Identical', ru.verdict);
  ok('unrelated earns less', ru.geometryEvidence < rs.geometryEvidence && ru.local.evidence < rs.local.evidence);

  const ab = paph.compare(A.t1, A.t2, B.t1, B.t2, {}, P);
  const ba = paph.compare(B.t1, B.t2, A.t1, A.t2, {}, P);
  ok('argument-order symmetry',
     ab.verdict === ba.verdict && ab.structural === ba.structural &&
     ab.geometryEvidence === ba.geometryEvidence && ab.totalInliers === ba.totalInliers &&
     ab.topology === ba.topology && ab.swapped !== ba.swapped &&
     deq(ab.local.coverageA, ba.local.coverageB) && deq(ab.local.coverageB, ba.local.coverageA));

  const M = paph.hash(mirrorIm(work(128, 128, 42)));
  const rm = paph.compare(A.t1, A.t2, M.t1, M.t2, {}, P);
  ok('mirrored copy carries mirror models',
     rm.models.length > 0 && rm.models.some(m => m.mirror) &&
     rm.verdict !== 'Unrelated' && rm.verdict !== 'Related', rm.verdict);
}

head('end-to-end: indeterminate and limits');
{
  const P = paph.cal();
  const A = paph.hash(work(128, 128, 7));
  const bad = Uint8Array.from(A.t1); bad[0] ^= 0xff;
  const rc = paph.compare(A.t1, A.t2, bad, A.t2, {}, P);
  ok('corrupt wire → Indeterminate(CORRUPT)', rc.verdict === 'Indeterminate' && deq(rc.reasons, ['CORRUPT']));
  const rp = paph.compare(A.t1, A.t2, A.t1, A.t2, {}, P, new Uint8Array(32).fill(1), new Uint8Array(32).fill(2));
  ok('hash-profile mismatch', rp.verdict === 'Indeterminate' && deq(rp.reasons, ['PROFILE_MISMATCH']));
  const broken = paph.cal(); broken.gridG = 1;
  const rb = paph.compare(A.t1, A.t2, A.t1, A.t2, {}, broken);
  ok('unsupported profile', rb.verdict === 'Indeterminate' && deq(rb.reasons, ['PROFILE_UNSUPPORTED']));
  let threw = '';
  try { paph.hash(work(16, 16, 1), {}, [8, 8, 64]); } catch (e) { threw = String(e.message || e); }
  ok('hashChecked enforces (profiles lower only)', threw.indexOf('limit: ') === 0, threw);
  let okHash = false;
  try { okHash = !!paph.hash(work(16, 16, 1), {}).t1; } catch (e) {}
  ok('hashChecked passes lawful input', okHash);
}

head('calibration core (demo/cal-core.cjs)');
{
  const P = paph.cal();
  const A = paph.hash(work(128, 128, 42));
  const B = paph.hash(work(128, 128, 1337));
  const M = paph.hash(mirrorIm(work(128, 128, 42)));
  const mk = (a, b, cat) => {
    const r = paph.compare(a.t1, a.t2, b.t1, b.t2, {}, P);
    return { cat, snap: cal.makeSnapshot(r, P), report: r };
  };
  const pairs = [mk(A, A, 'A'), mk(A, M, 'B'), mk(A, B, 'H')];
  let exact = true;
  for (const p of pairs) {
    const r = cal.relattice(paph, p.snap, P);
    if (r.state !== p.report.verdict || r.localEvidence !== p.report.local.evidence ||
        r.geoEvidence !== p.report.geometryEvidence) exact = false;
  }
  ok('relattice(snapshot, profile) ≡ the full comparator', exact);
  const m0 = cal.metrics(paph, pairs, P);
  ok('scoreboard on the clean corpus',
     m0.falseCert.length === 0 && m0.certB + m0.reviewPos.length >= 1 &&
     m0.byCat.A.n === 1 && m0.byCat.B.n === 1 && m0.byCat.H.n === 1,
     JSON.stringify({ fc: m0.falseCert.length, certB: m0.certB, nB: m0.nB }));
  const hard = paph.cal();
  hard.thresholds = P.thresholds.slice();
  /* close BOTH certification routes: the structural arms and the geo-solo
     arm (its inlier floor to the format maximum) */
  hard.thresholds[1] = 9999; hard.thresholds[4] = 10000; hard.thresholds[7] = 256;
  const m1 = cal.metrics(paph, pairs, hard);
  ok('closing every arm certifies nothing', m1.certB === 0 && m1.falseCert.length === 0,
     m1.certB + ' certified');
  ok('measurement key detects a re-measure need',
     cal.measurementKey(P) === pairs[0].snap.measuredUnder &&
     cal.measurementKey(Object.assign({}, P, { geoEps: 999 })) !== pairs[0].snap.measuredUnder);
}


/* ================================================================== M6
 * SPEC-004.1 (comparator 41): golden sections first, then behaviour. */
{
  head('golden: assignment_sparse (SPEC-004.1 A2)');
  let allEq = true, allSorted = true;
  for (let i = 0; i < G.assignment_sparse.length; i++) {
    const c = G.assignment_sparse[i], cost = G.assignment[i].cost;
    const got = paph.assignSparse(c.n, c.m, cost);
    if (!deq(got, c.pairs)) allEq = false;
    const full = paph.assign(c.n, c.m, cost);
    const tot = p => p.reduce((s, e) => s + cost[e[0] * c.m + e[1]], 0);
    if (got.length !== full.length || tot(got) !== tot(full)) allEq = false;
    for (let k = 1; k < got.length; k++)
      if (got[k][0] < got[k - 1][0]) allSorted = false;
    ok('case ' + i + ' pairs+total (differs_from_full=' + c.differs_from_full + ')',
       deq(got, c.pairs) && tot(got) === c.total);
  }
  ok('sparse ≡ full in cardinality and cost on every case', allEq);
  ok('ascending row order', allSorted);

  head('golden: weak_geometry (SPEC-004.1 A1)');
  const wg = G.weak_geometry;
  const mkKp = r => {
    const d = new Uint32Array(8);
    for (let i = 0; i < 8; i++) d[i] = parseInt(r.d.slice(i * 8, i * 8 + 8), 16) >>> 0;
    return { desc: d, x: r.x, y: r.y, level: 0, sec: 0, s: 0 };
  };
  const wa = wg.a.map(mkKp), wb = wg.b.map(mkKp);
  const wo = { geoEps: 1600, geoMinCorr: 8, geoConfAt: 16, mirrorHypothesis: true };
  const wAM = wire._internal.mirrorSide(wa, 65535);
  const wpd = paph.correspondW(wa, wb), wpm = paph.correspondW(wAM, wb);
  ok('pool sizes', wpd.length === wg.pool_direct && wpm.length === wg.pool_mirror,
     wpd.length + '/' + wpm.length);
  const g41 = paph._internal.geoMeasure41(wa, wAM, wb, wpd, wpm, wo, 256, 256, 6, 4);
  ok('extraction accepts nothing', g41.mm.models.length === wg.models);
  ok('the weak signal carries the sub-floor model',
     g41.measure === wg.measure && g41.weak === wg.weak, g41.measure + '/' + g41.weak);
  const gc = paph._internal.gnControl41(wa, wAM, wb, wpd, wpm, wo, 256, 256, 6, 4);
  ok('control member ' + wg.ctl_member, gc.ctl === wg.ctl && gc.member === wg.ctl_member,
     gc.ctl + ' ' + gc.member);

  head('golden: profile_cal003 (SPEC-004.1 A3, container 2) — frozen');
  const p3c = paph.cal41();
  const b3 = paph.profileEncode(p3c);
  ok('length ' + G.profile_cal003.len, b3.length === G.profile_cal003.len);
  ok('bytes', paph.hex(b3) === G.profile_cal003.bytes);
  ok('calibration_profile_id', paph.hex(paph.profileId(p3c)) === G.profile_cal003.id);
  const q3 = paph.profileDecode(b3);
  ok('container-2 roundtrip', q3.comparator === 41 && q3.container === 2 &&
     deq(q3.lutRuns, p3c.lutRuns) && deq(q3.lutPalette, p3c.lutPalette));
  let c1rej = false;
  try { const bb = b3.slice(); bb[4] = 1; paph.profileDecode(bb); } catch (e) { c1rej = true; }
  ok('a container-1 decoder refuses it', c1rej);

  head('golden: profile_cal004 (SPEC-004.2, container 3)');
  const p4c = paph.cal004();
  const b4 = paph.profileEncode(p4c);
  ok('length ' + G.profile_cal004.len, b4.length === G.profile_cal004.len);
  ok('bytes', paph.hex(b4) === G.profile_cal004.bytes);
  ok('calibration_profile_id', paph.hex(paph.profileId(p4c)) === G.profile_cal004.id);
  const q4 = paph.profileDecode(b4);
  ok('container-3 roundtrip', q4.comparator === 42 && q4.container === 3 &&
     q4.loweMargin === 6 && q4.exclPct === 100 && q4.kpSelect === 1 && q4.scaleSoft === 1 &&
     deq(q4.lutGeoDiversity, p4c.lutGeoDiversity));
  ok('the decision constants are CAL-003\'s, unchanged',
     deq(q4.thresholds, p3c.thresholds) && deq(q4.weights, p3c.weights) &&
     q4.geoConfAt === p3c.geoConfAt && q4.geoMinCorr === p3c.geoMinCorr);
  let c42rej = false;
  try { const bb = b4.slice(); bb[4] = 2; paph.profileDecode(bb); } catch (e) { c42rej = true; }
  ok('a container-2 decoder refuses it', c42rej);

  head('golden: SPEC-004.2 §4 the matcher');
  const d42 = seed => { const d = new Uint32Array(8); let s2 = seed >>> 0;
    for (let i = 0; i < 8; i++) { s2 = (Math.imul(s2, 1664525) + 1013904223) >>> 0; d[i] = s2; }
    return d; };
  let scOk = true;
  for (const c of G.strength_compat) if (paph.strengthCompat(c.sa, c.sb) !== c.sc) scOk = false;
  ok('strength compatibility, ' + G.strength_compat.length + ' vectors', scOk);
  let hcOk = true;
  for (const c of G.ham_cut) {
    const got = paph.hamCut(d42(c.a), d42(c.b), c.limit);
    if (c.exact ? got !== c.full : !(got > c.limit)) hcOk = false;
  }
  ok('early abort is exact below the limit, ' + G.ham_cut.length + ' vectors', hcOk);

  head('golden: SPEC-004.2 §5 soft scale binning');
  let vcOk = true;
  for (const c of G.vote_cells_42) {
    const A2 = [{ desc: new Uint32Array(8), x: c.ax, y: c.ay, level: c.alevel, sec: c.asec, s: 1024 }];
    const B2 = [{ desc: new Uint32Array(8), x: c.bx, y: c.by, level: c.blevel, sec: c.bsec, s: 1024 }];
    const v = paph.voteCells42(A2, B2, { i: 0, j: 0 }, c.mda, c.mdb, c.soft);
    if (!deq(v.key, c.keys) || !deq(v.kw, c.kw)) vcOk = false;
  }
  ok('cells and kernel weights, ' + G.vote_cells_42.length + ' vectors', vcOk);

  head('golden: SPEC-004.2 §7-§8 extraction and diversity');
  const hx = str => { const d = new Uint32Array(8);
    for (let i = 0; i < 8; i++) d[i] = parseInt(str.slice(i * 8, i * 8 + 8), 16) >>> 0;
    return d; };
  const kside = v => v.map(k => ({ desc: hx(k.d), x: k.x, y: k.y, level: k.level, sec: 0, s: k.s }));
  const gA = kside(G.geometry_42.a), gB = kside(G.geometry_42.b);
  const gP = paph.cal();
  const gO = { geoMinCorr: gP.geoMinCorr, geoEps: gP.geoEps, geoConfAt: gP.geoConfAt,
               mirrorHypothesis: true };
  const gAM = paph._internal.wire._internal.mirrorSide(gA, 65535);
  const gpd = paph.correspond42(gA, gB, gP), gpm = paph.correspond42(gAM, gB, gP);
  ok('pools ' + G.geometry_42.pool_direct + '/' + G.geometry_42.pool_mirror,
     gpd.length === G.geometry_42.pool_direct && gpm.length === G.geometry_42.pool_mirror);
  const gm = paph.geoMeasure42(gA, gAM, gB, gpd, gpm, gO, gP, 256, 256);
  ok('measure ' + G.geometry_42.measure,
     gm.measure === G.geometry_42.measure && gm.weak === G.geometry_42.weak);
  ok('models, residuals and vote mass',
     deq(gm.mm.models.map(m => ({ inliers: m.inliers, mirror: m.mirror, scaleQ16: m.scaleQ16,
                                  medianErr: m.medianErr, confSum: m.confSum })),
         G.geometry_42.models));
  const gn42 = paph.gnControl42(gA, gAM, gB, gpd, gpm, gO, gP, 256, 256);
  ok('GN control ' + G.geometry_42.ctl + ' (' + G.geometry_42.ctl_member + ')',
     gn42.ctl === G.geometry_42.ctl && gn42.member === G.geometry_42.ctl_member);
  ok('diversity channel', deq(paph.diversity42(gm.mm, gA, gB, gP).div, G.geometry_42.diversity));

  head('golden: SPEC-004.2 §3 the quality selector');
  const shared = d42(0x5eed1234);
  const cands = [];
  for (let i = 0; i < 40; i++) {
    cands.push({ desc: i % 2 === 0 ? d42((0xabcd0000 + i) >>> 0) : shared,
                 x: (i % 4) * 16000 + 4000, y: (Math.floor(i / 4) % 4) * 16000 + 4000,
                 level: i % 3, sec: 0, s: 65535 - i * 700 });
  }
  cands.sort((a, b) => (b.s - a.s) || (a.level - b.level) || (a.y - b.y) || (a.x - b.x));
  let selOk = true;
  for (const c of G.select_quality) {
    const got = wire.selectQuality(cands, c.want).map(k =>
      cands.findIndex(q => q.x === k.x && q.y === k.y && q.level === k.level && q.desc === k.desc));
    if (!deq(got, c.picked)) selOk = false;
  }
  ok('descriptor novelty outranks raw strength, ' + G.select_quality.length + ' budgets', selOk);

  head('comparator 42: behaviour');
  const work = (seed, w, h) => {
    const px = new Uint8Array(w * h * 4);
    for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) {
      const o = (y * w + x) * 4;
      px[o] = (x * 7 + y * 3 + seed) % 251; px[o + 1] = (y * 11 + x * 5) % 253;
      px[o + 2] = ((x ^ y) * 13) % 247; px[o + 3] = 255;
    }
    let s = seed;
    const rnd = m => { s = (s * 1103515245 + 12345) & 0x7fffffff; return s % Math.max(1, m); };
    for (let k = 0; k < 40; k++) {
      const bx = rnd(w - 10), by = rnd(h - 10);
      const cr = rnd(256), cg = rnd(256), cb = rnd(256);
      for (let dy = 0; dy < 6; dy++) for (let dx = 0; dx < 6; dx++) {
        const o = ((by + dy) * w + bx + dx) * 4;
        px[o] = cr; px[o + 1] = cg; px[o + 2] = cb;
      }
    }
    return { px, w, h };
  };
  const mirrorImg = im => {
    const px = new Uint8Array(im.px.length);
    for (let y = 0; y < im.h; y++) for (let x = 0; x < im.w; x++)
      px.set(im.px.subarray((y * im.w + (im.w - 1 - x)) * 4, (y * im.w + (im.w - 1 - x)) * 4 + 4),
             (y * im.w + x) * 4);
    return { px, w: im.w, h: im.h };
  };
  const P41 = paph.cal();   /* the entry: comparator 42 */
  const wi = work(5, 128, 96), wm = mirrorImg(wi);
  const fA = paph.hash(wi, {}, P41.limits), fM = paph.hash(wm, {}, P41.limits);
  const rSelf = paph.compare(fA.t1, fA.t2, fA.t1, fA.t2, {}, P41);
  ok('self → Identical under 42', rSelf.verdict === 'Identical' && rSelf.comparator === 42);
  ok('report carries geoWeakInliers, diversity, residuals and provenance',
     rSelf.geoWeakInliers === 0 && rSelf.screen === null &&
     typeof rSelf.diversity.combined === 'number' && Array.isArray(rSelf.medianErr) &&
     rSelf.selection.a === 1 && rSelf.selection.mixed === false);
  const rMir = paph.compare(fA.t1, fA.t2, fM.t1, fM.t2, {}, P41);
  ok('mirrored copy → Copy with a mirror model',
     rMir.verdict === 'Copy' && rMir.models.some(m => m.mirror), rMir.verdict);
  const rBack = paph.compare(fM.t1, fM.t2, fA.t1, fA.t2, {}, P41);
  ok('argument-order symmetry', rMir.verdict === rBack.verdict &&
     rMir.structural === rBack.structural && rMir.totalInliers === rBack.totalInliers);
  const rWrong = paph.compare(fA.t1, fA.t2, fA.t1, fA.t2, {}, paph.legacyCal001());
  ok('compareV42 refuses a comparator-4 profile',
     rWrong.verdict === 'Indeterminate' && rWrong.reasons[0] === paph.R_PROFILE_UNSUPPORTED);
  /* the audit path: a legacy container-1 artefact still decodes, so an old
     verdict's provenance can be read — and the comparator still refuses it */
  const legacyBytes = paph.profileEncode(paph.legacyCal001());
  const legacyRead = paph.profileDecode(legacyBytes);
  ok('a legacy artefact decodes for reading',
     legacyRead.container === 1 && legacyRead.comparator === 4);
  const rLegacy = paph.compare(fA.t1, fA.t2, fA.t1, fA.t2, {}, legacyRead);
  ok('and the comparator still refuses to compute with it',
     rLegacy.verdict === 'Indeterminate' && rLegacy.reasons[0] === paph.R_PROFILE_UNSUPPORTED);
  ok('this package exposes no comparator-4 entry point',
     paph.compareV4 === undefined && paph.cal001 === undefined);
  const r41 = paph.compare41(fA.t1, fA.t2, fM.t1, fM.t2, {}, paph.cal41());
  ok('comparator 41 is frozen, and still computes',
     r41.comparator === 41 && r41.verdict === 'Copy');
  ok('each comparator refuses the other\'s profile',
     paph.compare(fA.t1, fA.t2, fA.t1, fA.t2, {}, paph.cal41()).reasons[0] === paph.R_PROFILE_UNSUPPORTED &&
     paph.compare41(fA.t1, fA.t2, fA.t1, fA.t2, {}, paph.cal()).reasons[0] === paph.R_PROFILE_UNSUPPORTED);
  /* a 4.1 wire still parses, still compares, and says so */
  const fLegacy = paph.hash(wi, { kpCount: 256, kpSelect: 0 }, P41.limits);
  const rMix = paph.compare(fA.t1, fA.t2, fLegacy.t1, fLegacy.t2, {}, P41);
  ok('a mixed 4.1/4.2 pair is flagged, never refused',
     rMix.selection.mixed === true && rMix.verdict !== 'Indeterminate',
     rMix.verdict + ' sel ' + rMix.selection.a + '/' + rMix.selection.b);

  const sc = paph.screen(fA.t1, fA.t2, fA.t1, fA.t2, {}, P41);
  ok('screen: self passes with the pool rule',
     sc.pass === true && sc.poolDirect >= 8, sc.poolDirect + '/' + sc.poolMirror);
}

console.log('\n\x1b[1m' + pass + ' passed, ' + (fail ? '\x1b[31m' : '') + fail + ' failed\x1b[0m');
process.exit(fail ? 1 : 0);
