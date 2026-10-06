/*! cal-core.js — PAPH calibration core (SPEC-004 §19, milestone M5 tooling).
 *
 *  The pure logic under demo/paph-js-console.html, kept engine-adjacent and
 *  requireable from node so test/v4.cjs can hold it to the one invariant the
 *  whole console rests on: a pair's verdict recomputed from its SNAPSHOT under a
 *  profile equals the verdict the full comparator produces under that profile.
 *
 *  A snapshot stores everything the §14 lattice consumes EXCEPT the outputs of
 *  the three calibration LUTs — those are re-derived live from the stored raw
 *  margins, which is what makes threshold and LUT tuning instant over a whole
 *  corpus.  Fields that change the MEASUREMENT itself (hash-time fields,
 *  hammingT, confidence anchors, geoEps, model floors, gridG) are recorded as
 *  the profile the pair was measured under; editing those requires
 *  re-measuring from the stored wires, and the bench says so.  MIT. */
(function (root, factory) {
  if (typeof module === 'object' && module.exports) module.exports = factory();
  else root.calCore = factory();
})(typeof self !== 'undefined' ? self : this, function () {
'use strict';

var SCALE = 10000;
var CHANNEL_ORDER = ['dct', 'local', 'shape', 'topology', 'runs', 'palette', 'silhouette'];

/* §19.1 — the corpus partition.  A–D are positives, E–I negatives. */
var CATEGORIES = [
  { key: 'A', name: 'exact duplicates', positive: true },
  { key: 'B', name: 'benign transformations', positive: true },
  { key: 'C', name: 'partial copies', positive: true },
  { key: 'D', name: 'collages', positive: true },
  { key: 'E', name: 'same-generator negatives', positive: false },
  { key: 'F', name: 'repeated-pattern negatives', positive: false },
  { key: 'G', name: 'shared-template negatives', positive: false },
  { key: 'H', name: 'random unrelated', positive: false },
  { key: 'I', name: 'adversarial hard negatives', positive: false }
];
function isPositive(cat) { return 'ABCD'.indexOf(cat) >= 0; }

var RANK = { Indeterminate: 0, Unrelated: 1, Related: 2, Suspected: 3, Copy: 4, Identical: 5 };
function certified(v) { return v === 'Copy' || v === 'Identical'; }
function atLeastSuspected(v) { return RANK[v] >= RANK.Suspected; }

/* The measurement-side profile fields: identical values ⇒ snapshots transfer;
   any difference ⇒ the pair must be re-measured from its wires. */
var MEASUREMENT_FIELDS = ['hammingT', 'confidenceAt', 'geoConfAt', 'geoEps',
                          'geoMinCorr', 'minModelInliers', 'maxModels', 'gridG'];
function measurementKey(P) {
  return MEASUREMENT_FIELDS.map(function (f) { return f + ':' + P[f]; }).join(' ');
}

/* Everything the lattice consumes, with LUT outputs left OUT and their raw
   inputs kept IN.  The six non-local channels keep their RAW values and pass
   through the profile's per-channel tables at relattice time (§A3); the
   geometry measurement may be the weak signal (§A1), which the snapshot
   records so a bench can show which pairs lean on it.

   SPEC-004.2 adds one more re-derivable input: the §8 geometric-diversity
   READING is a measurement and is stored, while the table it passes through is
   a calibration and is not — same split as everywhere else here, so dragging
   lutGeoDiversity retunes a whole corpus instantly. */
function makeSnapshot(report, P) {
  var six = [];
  for (var i = 0; i < 7; i++) {
    var name = CHANNEL_ORDER[i];
    if (name === 'local') continue;
    var ch = report.v3.channels[name];
    six.push([name, ch.value, ch.measurable]);
  }
  var s = {
    identical: report.v3.identical,
    six: six,
    localMeasurable: report.local.measurable,
    localMargin: report.local.margin,
    diversity: report.local.diversity,
    coverageMin: Math.min(report.local.coverageA.coverage, report.local.coverageB.coverage),
    geoMargin: report.geoMargin,
    geoRaw: report.geoRaw,
    geoCtl: report.geoCtl,
    totalInliers: report.totalInliers,
    topology: report.topology,
    anyMirror: report.models.some(function (m) { return m.mirror; }),
    /* §8 — the combined diversity reading, or null under comparator 41 */
    geoDiversity: report.diversity ? report.diversity.combined : null,
    modelCount: report.models.length,
    geoMeas: report.geoMeasurable,
    v3Verdict: report.v3.verdict,
    v3Structural: report.v3.structural,
    measuredUnder: measurementKey(P)
  };
  s.comparator = report.comparator;
  s.geoWeak = report.geoWeakInliers || 0;
  return s;
}

function relattice(engine, snap, P) {
  var localEv = 0;
  if (snap.localMeasurable)
    localEv = Math.trunc(engine.lutEval(P.lutLocal, snap.localMargin) *
                         engine.lutEval(P.lutDiversity, snap.diversity) / SCALE);
  var geoEv = engine.lutEval(P.lutGeometry, snap.geoMargin);
  /* §8 — the diversity multiplier, and ONLY where a model was accepted: the
     weak-signal path reached the margin without producing one, and multiplying
     it down here would punish the same absence twice.  A comparator-41 snapshot
     carries no diversity reading and is left exactly as it was. */
  if (snap.geoDiversity !== null && snap.geoDiversity !== undefined && snap.modelCount > 0)
    geoEv = Math.trunc(geoEv * engine.lutEval(P.lutGeoDiversity, snap.geoDiversity) / SCALE);
  var channels = [], k = 0;
  for (var i = 0; i < 7; i++) {
    var name = CHANNEL_ORDER[i];
    if (name === 'local') { channels.push(['local', localEv, snap.localMeasurable]); continue; }
    var six = snap.six[k]; k++;
    channels.push([six[0], engine.lutEval(engine.lutChannel(P, six[0]), six[1]), six[2]]);
  }
  var out = engine.lattice({
    identical: snap.identical, channels: channels,
    geoMeasurable: snap.geoMeas, geoEvidence: geoEv,
    totalInliers: snap.totalInliers, topologyClass: snap.topology,
    diversity: snap.diversity, coverageMin: snap.coverageMin,
    anyMirrorModel: snap.anyMirror
  }, P);
  out.localEvidence = localEv;
  out.geoEvidence = geoEv;
  out.channels = channels;
  return out;
}

/* The scoreboard.  CERTIFIED and AT-REVIEW are reported separately and
   named separately — they are different questions, and reading one as the
   other is how a calibration gets mis-sold. */
function metrics(engine, pairs, P) {
  var out = {
    n: pairs.length, stale: 0,
    falseCert: [], byCat: {}, arms: {},
    certB: 0, nB: 0, atRevB: 0, certCD: 0, nCD: 0, atRevCD: 0,
    reviewPos: [], reviewNeg: [], lostVs4: [], demotedVs4: [],
    negStructMax: 0, weakUsed: 0, weakPos: 0
  };
  var mkey = measurementKey(P);
  CATEGORIES.forEach(function (c) {
    out.byCat[c.key] = { n: 0, cert: 0, review: 0, below: 0 };
  });
  pairs.forEach(function (p, idx) {
    var r = relattice(engine, p.snap, P);
    p.live = r;
    var v = r.state, pos = isPositive(p.cat);
    if (p.snap.measuredUnder !== mkey) out.stale++;
    if (!out.byCat[p.cat]) out.byCat[p.cat] = { n: 0, cert: 0, review: 0, below: 0 };
    var b = out.byCat[p.cat];
    b.n++;
    if (certified(v)) b.cert++; else if (v === 'Suspected') b.review++; else b.below++;
    if (certified(v)) {
      var arm = v + ':' + r.basis.join('+');
      out.arms[arm] = (out.arms[arm] || 0) + 1;
    }
    if (p.snap.geoWeak > 0) { out.weakUsed++; if (pos) out.weakPos++; }
    if (pos) {
      if (p.cat === 'B') { out.nB++; if (certified(v)) out.certB++; if (atLeastSuspected(v)) out.atRevB++; }
      if (p.cat === 'C' || p.cat === 'D') { out.nCD++; if (certified(v)) out.certCD++; if (atLeastSuspected(v)) out.atRevCD++; }
      if (v === 'Suspected') out.reviewPos.push(idx);
      if (p.snap.v4Verdict && certified(p.snap.v4Verdict) && !certified(v)) {
        (v === 'Suspected' ? out.demotedVs4 : out.lostVs4).push(idx);
      }
    } else {
      if (certified(v)) out.falseCert.push(idx);
      if (v === 'Suspected') out.reviewNeg.push(idx);
      if (r.structural > out.negStructMax) out.negStructMax = r.structural;
    }
  });
  return out;
}

return { SCALE: SCALE, CHANNEL_ORDER: CHANNEL_ORDER, CATEGORIES: CATEGORIES,
         RANK: RANK, isPositive: isPositive, certified: certified,
         atLeastSuspected: atLeastSuspected,
         MEASUREMENT_FIELDS: MEASUREMENT_FIELDS, measurementKey: measurementKey,
         makeSnapshot: makeSnapshot, relattice: relattice, metrics: metrics };
});
