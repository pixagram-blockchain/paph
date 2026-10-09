#!/usr/bin/env python3
"""Comparator 42's lattice on calibration snapshots, for trying candidate
profiles without re-measuring a pair.

`sibench calib` (rust/src/bin/sibench.rs) writes one TSV row per pair: every
quantity the lattice reads, taken before the profile's decision tables and
bars, and the verdict comparator 42 reached under CAL-004-PROPOSED.  This
script is the lattice (rust/src/lattice.rs) and the LUT arithmetic, integer
and truncating like the reference; it recomputes each row under CAL-004 —
which must reproduce the recorded verdict, row for row — and under
CAL-007-PROVISIONAL (CAL-004 with the moderate structural bar at 3300), and
prints how the states move.  Edit CAL007 below to try another profile.

    cargo build --release --bin sibench --manifest-path rust/Cargo.toml
    (cd rust && ./target/release/sibench calib)
    python3 tools/cal-lattice.py [rust/target/calib]

Python 3, standard library only.
"""
import csv
import os
import sys
from collections import Counter

SCALE = 10000

CAL004 = dict(
    thresholds=[8000, 4000, 2400, 1000, 6000, 3500, 1200, 11, 6000],
    weights=dict(local=35, shape=25, topology=15, runs=10, dct=10, palette=5, silhouette=10),
    min_secondaries=3, coverage_floor=2500, rep_extreme_at=1500,
    lut_local=[(0, 0), (5000, 1000), (8800, 4200), (9400, 8800), (10000, 10000)],
    lut_geometry=[(0, 0), (1900, 1100), (3000, 3000), (10000, 10000)],
    lut_diversity=[(0, 10000), (10000, 10000)],
    lut_dct=[(0, 0), (10000, 10000)],
    lut_shape=[(0, 0), (10000, 10000)],
    lut_topology=[(0, 0), (10000, 10000)],
    lut_runs=[(0, 0), (9100, 900), (9700, 4500), (10000, 10000)],
    lut_palette=[(0, 0), (3000, 800), (6500, 4000), (10000, 10000)],
    lut_silhouette=[(0, 0), (10000, 10000)],
    lut_geo_diversity=[(0, 6000), (2000, 8000), (4000, 10000), (10000, 10000)],
)
# CAL-007-PROVISIONAL: the moderate structural bar (thresholds[2]) at 3300
CAL007 = dict(CAL004, thresholds=CAL004['thresholds'][:2] + [3300] + CAL004['thresholds'][3:])

ORDER = ['dct', 'local', 'shape', 'topology', 'runs', 'palette', 'silhouette']
TEXT = ('kind', 'tf', 'same', 'verdict', 'class', 'basis')
STATES = ['Identical', 'Copy', 'Suspected', 'Related', 'Unrelated']


def lut_eval(p, x):
    x = max(0, min(SCALE, x))
    i = 0
    while i + 2 < len(p) and p[i + 1][0] < x:
        i += 1
    if i + 2 == len(p) and x >= p[i + 1][0]:
        return p[i + 1][1]
    (x0, y0), (x1, y1) = p[i], p[i + 1]
    return y0 + idiv((y1 - y0) * (x - x0), x1 - x0)


def idiv(a, b):
    """Integer division truncating toward zero, as Rust's `/` on i64."""
    q = abs(a) // abs(b)
    return q if (a >= 0) == (b > 0) else -q


def load(path):
    rows = []
    with open(path) as f:
        for row in csv.DictReader(f, delimiter='\t'):
            for k, v in list(row.items()):
                if k not in TEXT:
                    row[k] = int(v)
            rows.append(row)
    return rows


def lattice_in(s, P):
    ch = []
    for name in ORDER:
        if name == 'local':
            ev = idiv(lut_eval(P['lut_local'], s['local_margin']) * lut_eval(P['lut_diversity'], s['local_div']), SCALE) if s['local_m'] else 0
            ch.append((name, ev, bool(s['local_m'])))
        else:
            ch.append((name, lut_eval(P['lut_' + name], s[name]), bool(s[name + '_m'])))
    mult = SCALE if s['nmodels'] == 0 else lut_eval(P['lut_geo_diversity'], s['div_combined'])
    gev = max(0, min(SCALE, idiv(lut_eval(P['lut_geometry'], s['geo_margin']) * mult, SCALE)))
    return dict(identical=bool(s['identical']), channels=ch, geo_measurable=bool(s['geo_m']), geo_evidence=gev,
                total_inliers=s['inliers'], topology_class=s['topo'], diversity=s['local_div'],
                coverage_min=s['coverage_min'])


def lattice(x, P):
    wsum = wtot = sec = 0
    local_m = False
    for name, v, m in x['channels']:
        if not m:
            continue
        w = P['weights'][name]
        wsum += v * w
        wtot += w
        if name == 'local':
            local_m = True
        else:
            sec += 1
    structural = idiv(wsum, wtot) if wtot > 0 else 0
    t = P['thresholds']
    certifiable = local_m and sec >= P['min_secondaries']
    gv = x['geo_evidence'] if x['geo_measurable'] else 0
    geo_strong = x['geo_measurable'] and gv >= t[5]
    geo_weak = x['geo_measurable'] and gv >= t[6]
    s_strong = certifiable and structural >= t[1]
    s_mod = certifiable and structural >= t[2]
    if x['identical']:
        st = 'Identical'
    elif s_strong and geo_strong:
        st = 'Copy'
    elif s_strong and structural >= t[4]:
        st = 'Copy' if x['coverage_min'] >= P['coverage_floor'] else 'Suspected'
    elif geo_strong and x['total_inliers'] >= t[7]:
        st = 'Copy' if x['topology_class'] in (2, 3, 4) else 'Suspected'
    elif s_strong or s_mod or geo_strong or geo_weak:
        st = 'Suspected'
    elif structural >= t[3]:
        st = 'Related'
    else:
        st = 'Unrelated'
    if st == 'Copy' and local_m and x['diversity'] < P['rep_extreme_at'] and not geo_weak:
        st = 'Suspected'
    return st


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    d = sys.argv[1] if len(sys.argv) > 1 else os.path.join(here, '..', 'rust', 'target', 'calib')
    files = [f for f in ('chain-pairs.tsv', 'realbase.tsv', 'synth.tsv') if os.path.exists(os.path.join(d, f))]
    if not files:
        sys.exit(f'no snapshots under {d}: run `sibench calib` first')
    bad_total = 0
    for name in files:
        rows = load(os.path.join(d, name))
        bad = 0
        moves = Counter()
        for s in rows:
            a = lattice(lattice_in(s, CAL004), CAL004)
            b = lattice(lattice_in(s, CAL007), CAL007)
            if a != s['verdict']:
                bad += 1
                if bad <= 5:
                    print(f'  MISMATCH {name} {s["kind"]} {s["a"]} {s["b"]} {s["tf"]}: {a}, recorded {s["verdict"]}')
            key = s['kind'] + (' (one author)' if s['kind'] == 'chain' and s['same'] == '1' else ' (two authors)' if s['kind'] == 'chain' else '')
            moves[(key, a, b)] += 1
        bad_total += bad
        print(f'\n{name}: {len(rows)} rows; CAL-004 recomputed differs from the recorded verdict on {bad}')
        print('| pairs | CAL-004 → CAL-007 | count |')
        print('|---|---|---:|')
        for (key, a, b), n in sorted(moves.items(), key=lambda kv: (kv[0][0], STATES.index(kv[0][1]), STATES.index(kv[0][2]))):
            if a != b:
                print(f'| {key} | {a} → {b} | {n} |')
        unmoved = sum(n for (k, a, b), n in moves.items() if a == b)
        print(f'| (every other row keeps its state) | | {unmoved} |')
    sys.exit(1 if bad_total else 0)


if __name__ == '__main__':
    main()
