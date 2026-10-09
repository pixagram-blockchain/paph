#!/usr/bin/env node
/**
 * Which ties can the silhouette's component choice meet? (docs/SPEC-W4-paph-wire4.md §6)
 *
 * Wire 4 keeps, among the opaque components of the largest area, the one with the longest
 * perimeter, then the longer and the shorter side of its box, then the least canonical profile,
 * then the greater principal moments, then the least canonical occupancy, and only then the first
 * the scan met.  Every free polyomino (one shape up to the square's symmetries) of up to N cells is
 * keyed here with the JavaScript engine's own functions, and the shapes of one size are grouped by
 * those keys, stage by stage: how many groups the profile leaves tied, how many of those the
 * moments leave tied, and how many of those the occupancy leaves — which only the scan order would
 * then decide, between two different shapes.  Twins (one shape in two orientations) are one free
 * polyomino and do not appear: no key can tell them apart.
 *
 *     node tools/silhouette-ties.cjs [N]        N defaults to 13: 238,591 shapes, about a minute
 *
 * Its output is in docs/calibration/X3-PROVISIONAL.log.
 */
'use strict';
const W = require('../src/wire.cjs');
const I = W._internal;
const N = +(process.argv[2] || 13);

/* free polyominoes by growth, one canonical cell list per shape */
function norm(cells) {
  let mx = Infinity, my = Infinity;
  for (const [x, y] of cells) { mx = Math.min(mx, x); my = Math.min(my, y); }
  return cells.map(([x, y]) => [x - mx, y - my]).sort((a, b) => a[1] - b[1] || a[0] - b[0]);
}
const key = c => c.map(p => p[0] + ',' + p[1]).join(';');
function canonKey(cells) {
  let best = null;
  for (let e = 0; e < 8; e++) {
    const k = key(norm(cells.map(([x, y]) => {
      let [a, b] = e & 4 ? [y, x] : [x, y];
      if (e & 1) a = -a;
      if (e & 2) b = -b;
      return [a, b];
    })));
    if (best === null || k < best) best = k;
  }
  return best;
}

/* the keys of silhouette4, on the shape alone in a transparent margin */
function keys(cells) {
  const bw = Math.max(...cells.map(p => p[0])) + 1, bh = Math.max(...cells.map(p => p[1])) + 1;
  const w = bw + 2, h = bh + 2, id = new Int32Array(w * h).fill(-1);
  let sx = 0, sy = 0;
  for (const [x, y] of cells) { id[(y + 1) * w + x + 1] = 0; sx += x + 1; sy += y + 1; }
  const c = { id: 0, area: cells.length, sx, sy, minx: 1, maxx: bw, miny: 1, maxy: bh };
  let per = 0;
  for (const [x0, y0] of cells) {
    const q = (y0 + 1) * w + x0 + 1;
    if (id[q - 1] !== 0 || id[q + 1] !== 0 || id[q - w] !== 0 || id[q + w] !== 0) per++;
  }
  const prof = I.profileBytes(I.raysExact(id, w, 0, c.area, sx, sy, 1, bw, 1, bh, bw + bh));
  const m = I.exactMoments(id, w, c), Z = BigInt(0);
  const occ = I.canonical64(I.occupancyCells(id, w, c), false);
  return {
    profile: [per, Math.max(bw, bh), Math.min(bw, bh), Array.from(I.canonicalProfile(prof)).join(',')].join('|'),
    moments: [m[0] > m[1] ? m[0] : m[1], m[0] > m[1] ? m[1] : m[0], m[2] < Z ? -m[2] : m[2]].join(','),
    occupancy: occ.join(','),
  };
}

function groups(list, k) {
  const g = new Map();
  for (const e of list) { const v = k(e); if (!g.has(v)) g.set(v, []); g.get(v).push(e); }
  return [...g.values()].filter(x => x.length > 1);
}

let level = new Map([[canonKey([[0, 0]]), [[0, 0]]]]);
console.log('| cells | free polyominoes | groups tied through the profile (shapes) | of them tied through the moments | through the occupancy |');
console.log('|---:|---:|---:|---:|---:|');
const settledByOccupancy = [];
for (let n = 1; n <= N; n++) {
  const shapes = [...level.values()].map(c => ({ c, k: keys(c) }));
  const byProfile = groups(shapes, e => e.k.profile);
  const byMoments = byProfile.flatMap(g => groups(g, e => e.k.moments));
  const byOccupancy = byMoments.flatMap(g => groups(g, e => e.k.occupancy));
  for (const g of byMoments) if (!groups(g, e => e.k.occupancy).length) settledByOccupancy.push([n, g.map(e => e.c)]);
  console.log(`| ${n} | ${shapes.length} | ${byProfile.length} (${byProfile.reduce((a, g) => a + g.length, 0)}) | ${byMoments.length} | ${byOccupancy.length} |`);
  if (n === N) break;
  const next = new Map();
  for (const c of level.values()) {
    const has = new Set(c.map(p => p.join(',')));
    for (const [x, y] of c) for (const [dx, dy] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
      const q = [x + dx, y + dy];
      if (has.has(q.join(','))) continue;
      const d = norm(c.concat([q])), k = canonKey(d);
      if (!next.has(k)) next.set(k, d);
    }
  }
  level = next;
}
for (const [n, g] of settledByOccupancy) console.log(`\nsettled by the occupancy (${n} cells): ${g.map(c => JSON.stringify(c)).join('  ')}`);
