//! Deterministic assignment (SPEC-004 §10.2, Appendix A).
//!
//! Maximum-cardinality, then minimum-cost, matching on a sparse bipartite
//! graph, computed by successive shortest augmenting paths with potentials —
//! the Hungarian method, pinned down to one function: rows visited once in
//! ascending index, every argmin tie broken toward the smaller column, missing
//! edges carried as one BIG cost so that cardinality dominates cost
//! lexicographically (128 · BIG_real_total < BIG, so no count of real edges
//! can ever buy one forbidden edge).
//!
//! Greedy's failure mode, for the record: edges (0,0)=1, (0,1)=2, (1,0)=2.
//! Stratified greedy takes (0,0) and strands row 1.  The assignment takes
//! (0,1)+(1,0): two matches for cost 4.  With repeated descriptors this is the
//! difference between counting a paste and missing it.

pub const NO_EDGE: i32 = -1;
const BIG: i64 = 1 << 30; // real edge costs are ≤ 2^15; 128 of them < 2^22 ≪ BIG
const INF: i64 = i64::MAX / 4;
const NONE: usize = usize::MAX;

/// `cost` is row-major `n × m`; `NO_EDGE` (or any negative) means no edge,
/// otherwise `0 ..= 32767`.  Returns matched `(row, col)` pairs, ascending by
/// row.  Output is a pure function of the inputs — no iteration-order,
/// allocation, or platform dependence.
///
/// This is the e-maxx Hungarian of `assign_ref` below, step for step and tie
/// for tie, run on the real edges only.  Two things make that possible.
///
/// The reference subtracts each step's `delta` from every unvisited `minv`;
/// here a running offset `dsum` stands for all of those subtractions at once,
/// and each visited column's potential moves once at the end of the phase by
/// `dsum` minus the offset at which it was visited (see `assign_dense`).
///
/// And the completion edges are never materialised.  At a step that scans
/// row `i0` with offset `dsum`, column `j` is offered `c(i0, j) + K - v[j]`
/// where `K = dsum - u[i0]`.  For a missing edge `c` is the one constant
/// `BIG`, so over the steps of a phase every column's best completion offer
/// is `BIG + Kmin - v[j]`, `Kmin` being the smallest `K` so far — a single
/// number, not a row of them.  Offering it to columns the row DOES have a real
/// edge to changes nothing: that row's real offer is strictly smaller, so the
/// phantom offer can neither be a minimum nor tie one.  A column's `minv` is
/// therefore `min(R[j], BIG + Kmin) - v[j]`, with `R[j]` the best real offer,
/// and the step's argmin is the smaller of two candidates:
///
///   * the best column touched by a real edge this phase, scanned directly;
///   * the best completion offer, `BIG + Kmin - max v`, at the smallest
///     column of maximal `v` — found by walking the columns in (v desc,
///     index asc) order past the visited ones, a walk that only moves forward
///     within a phase because the visited set only grows.
///
/// `way[j]` is the step that first reached the column's minimum, exactly as
/// the reference's strict `<` keeps it.  `v` changes only at the end of a
/// phase and only for visited columns, so the walk order is repaired by
/// re-merging those columns.  `assign_dense` (the dense version of the same
/// bookkeeping) and `assign_ref` are kept and tested equal pair for pair.
pub fn assign(n: usize, m: usize, cost: &[i32]) -> Vec<(usize, usize)> {
    debug_assert_eq!(cost.len(), n * m);
    if n == 0 || m == 0 {
        return Vec::new();
    }
    let transposed = n > m;
    let (rn, rm) = if transposed { (m, n) } else { (n, m) };
    // real edges per solving row, ascending column
    let mut start = vec![0u32; rn + 1];
    let mut edges: Vec<(u32, i32)> = Vec::new();
    for r in 0..rn {
        for k in 0..rm {
            let c = if transposed { cost[k * m + r] } else { cost[r * m + k] };
            if c >= 0 {
                edges.push((k as u32, c));
            }
        }
        start[r + 1] = edges.len() as u32;
    }

    let p = solve(rn, rm, &start, &edges);
    let mut out = Vec::new();
    for j in 0..rm {
        let i = p[j];
        if i == NONE {
            continue;
        }
        let (r, c) = if transposed { (j, i) } else { (i, j) };
        // BIG edges are the completion trick, not matches.
        if cost[r * m + c] >= 0 {
            out.push((r, c));
        }
    }
    out.sort_unstable();
    out
}

/// The solver behind `assign` and `assign_edges`: `rn <= rm` rows in the
/// solving orientation, row `r`'s real edges `edges[start[r]..start[r + 1]]`
/// as (column, cost), ascending column.  Returns `p`, the row matched to each
/// column (`NONE` for a free one), real edge or completion edge alike.
fn solve(rn: usize, rm: usize, start: &[u32], edges: &[(u32, i32)]) -> Vec<usize> {
    let mut u = vec![0i64; rn];
    let mut v = vec![0i64; rm];
    let mut p = vec![NONE; rm + 1]; // p[j] = row matched to column j
    let mut way = vec![0usize; rm];
    // per-phase state, reset through the lists of what was touched
    let mut real = vec![INF; rm]; // R[j]: best real offer, without -v[j]
    let mut rway = vec![0usize; rm];
    let mut rstep = vec![0u32; rm];
    let mut touched: Vec<usize> = Vec::with_capacity(rm);
    let mut used = vec![false; rm];
    let mut visited: Vec<(usize, i64)> = Vec::with_capacity(rm + 1);
    // columns by (v desc, index asc); v is 0 everywhere at the start
    let mut order: Vec<usize> = (0..rm).collect();
    let mut moved_cols: Vec<usize> = Vec::with_capacity(rm);
    let mut merged: Vec<usize> = Vec::with_capacity(rm);

    for i in 0..rn {
        p[rm] = i;
        let mut j0 = rm;
        let mut dsum = 0i64;
        let (mut kmin, mut kway, mut kstep) = (INF, rm, 0u32);
        let mut walk = 0usize;
        let mut step = 0u32;
        visited.clear();
        visited.push((rm, 0));
        loop {
            let i0 = p[j0];
            let k = dsum - u[i0];
            if k < kmin {
                kmin = k;
                kway = j0;
                kstep = step;
            }
            for &(j, c) in &edges[start[i0] as usize..start[i0 + 1] as usize] {
                let j = j as usize;
                if used[j] {
                    continue;
                }
                let offer = c as i64 + k;
                if offer < real[j] {
                    if real[j] == INF {
                        touched.push(j);
                    }
                    real[j] = offer;
                    rway[j] = j0;
                    rstep[j] = step;
                }
            }
            let bigk = BIG + kmin;
            // the best column touched by a real edge
            let (mut best, mut j1) = (INF, NONE);
            for &j in touched.iter() {
                if used[j] {
                    continue;
                }
                let val = real[j].min(bigk) - v[j];
                if val < best || (val == best && j < j1) {
                    best = val;
                    j1 = j;
                }
            }
            // the best completion offer: the first unvisited column in order
            // (one always exists — at most i < rn <= rm columns are matched)
            while used[order[walk]] {
                walk += 1;
            }
            let jb = order[walk];
            let val = bigk - v[jb];
            if val < best || (val == best && jb < j1) {
                best = val;
                j1 = jb;
            }
            // which step reached j1's minimum first
            let r = real[j1];
            way[j1] = if r < bigk || (r == bigk && rstep[j1] < kstep) { rway[j1] } else { kway };
            dsum = best;
            j0 = j1;
            if p[j0] == NONE {
                break;
            }
            used[j0] = true;
            visited.push((j0, dsum));
            step += 1;
        }
        // potentials, then the walk order repaired for the columns that moved
        moved_cols.clear();
        for &(j, at) in visited.iter() {
            let moved = dsum - at;
            if p[j] != NONE {
                u[p[j]] += moved;
            }
            if j < rm {
                v[j] -= moved;
                used[j] = false;
                moved_cols.push(j);
            }
        }
        for &j in touched.iter() {
            real[j] = INF;
        }
        touched.clear();
        if !moved_cols.is_empty() {
            // by (v desc, index asc); a handful of columns, so insertion
            for a in 1..moved_cols.len() {
                let x = moved_cols[a];
                let mut b = a;
                while b > 0 && {
                    let y = moved_cols[b - 1];
                    v[y] < v[x] || (v[y] == v[x] && y > x)
                } {
                    moved_cols[b] = moved_cols[b - 1];
                    b -= 1;
                }
                moved_cols[b] = x;
            }
            let mut is_moved = std::mem::take(&mut used);
            for &j in moved_cols.iter() {
                is_moved[j] = true;
            }
            merged.clear();
            let mut q = 0usize;
            for &j in order.iter() {
                if is_moved[j] {
                    continue;
                }
                while q < moved_cols.len() && {
                    let x = moved_cols[q];
                    v[x] > v[j] || (v[x] == v[j] && x < j)
                } {
                    merged.push(moved_cols[q]);
                    q += 1;
                }
                merged.push(j);
            }
            merged.extend_from_slice(&moved_cols[q..]);
            for &j in moved_cols.iter() {
                is_moved[j] = false;
            }
            used = is_moved;
            std::mem::swap(&mut order, &mut merged);
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == rm {
                break;
            }
        }
    }

    p
}

/// The lazy-offset Hungarian on the dense BIG-completed matrix — the previous
/// `assign`, kept as a second reference.
#[cfg(test)]
pub fn assign_dense(n: usize, m: usize, cost: &[i32]) -> Vec<(usize, usize)> {
    debug_assert_eq!(cost.len(), n * m);
    if n == 0 || m == 0 {
        return Vec::new();
    }
    let transposed = n > m;
    let (rn, rm) = if transposed { (m, n) } else { (n, m) };
    // dense costs in the solving orientation, BIG for a missing edge
    let mut c = vec![0i64; rn * rm];
    for r in 0..rn {
        let row = &mut c[r * rm..(r + 1) * rm];
        for (k, slot) in row.iter_mut().enumerate() {
            let v = if transposed { cost[k * m + r] } else { cost[r * m + k] };
            *slot = if v < 0 { BIG } else { v as i64 };
        }
    }

    let mut u = vec![0i64; rn];
    let mut v = vec![0i64; rm + 1];
    let mut p = vec![NONE; rm + 1]; // p[j] = row matched to column j
    let mut way = vec![0usize; rm];
    let mut stored = vec![INF; rm];
    let mut used = vec![false; rm];
    let mut visited: Vec<(usize, i64)> = Vec::with_capacity(rm + 1);

    for i in 0..rn {
        p[rm] = i;
        let mut j0 = rm;
        for x in stored.iter_mut() {
            *x = INF;
        }
        for x in used.iter_mut() {
            *x = false;
        }
        visited.clear();
        let mut dsum = 0i64;
        visited.push((rm, 0));
        loop {
            let i0 = p[j0];
            let ui0 = u[i0];
            let crow = &c[i0 * rm..(i0 + 1) * rm];
            let mut best = INF;
            let mut j1 = NONE;
            for j in 0..rm {
                if used[j] {
                    continue;
                }
                let cd = crow[j] - ui0 - v[j] + dsum;
                if cd < stored[j] {
                    stored[j] = cd;
                    way[j] = j0;
                }
                if stored[j] < best {
                    best = stored[j];
                    j1 = j; // strict `<`: first (smallest) column keeps ties
                }
            }
            // delta = best - dsum; every unvisited minv falls by it at once
            dsum = best;
            j0 = j1;
            if p[j0] == NONE {
                break;
            }
            used[j0] = true;
            visited.push((j0, dsum));
        }
        for &(j, at) in visited.iter() {
            let moved = dsum - at;
            if p[j] != NONE {
                u[p[j]] += moved;
            }
            v[j] -= moved;
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == rm {
                break;
            }
        }
    }

    let mut out = Vec::new();
    for j in 0..rm {
        let i = p[j];
        if i == NONE {
            continue;
        }
        let (r, c) = if transposed { (j, i) } else { (i, j) };
        // BIG edges are the completion trick, not matches.
        if cost[r * m + c] >= 0 {
            out.push((r, c));
        }
    }
    out.sort_unstable();
    out
}

/// The e-maxx Hungarian as shipped — the reference `assign` is held to.
#[cfg(test)]
pub fn assign_ref(n: usize, m: usize, cost: &[i32]) -> Vec<(usize, usize)> {
    debug_assert_eq!(cost.len(), n * m);
    if n == 0 || m == 0 {
        return Vec::new();
    }
    let at = |r: usize, c: usize, transposed: bool| -> i64 {
        let v = if transposed { cost[c * m + r] } else { cost[r * m + c] };
        if v < 0 { BIG } else { v as i64 }
    };
    let transposed = n > m;
    let (rn, rm) = if transposed { (m, n) } else { (n, m) };

    // e-maxx Hungarian, 0-indexed, virtual start column `rm`.
    let mut u = vec![0i64; rn];
    let mut v = vec![0i64; rm + 1];
    let mut p = vec![NONE; rm + 1]; // p[j] = row matched to column j
    let mut way = vec![0usize; rm];

    for i in 0..rn {
        p[rm] = i;
        let mut j0 = rm;
        let mut minv = vec![INF; rm];
        let mut used = vec![false; rm + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = INF;
            let mut j1 = NONE;
            for j in 0..rm {
                if !used[j] {
                    let cur = at(i0, j, transposed) - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j; // strict `<`: first (smallest) column keeps ties
                    }
                }
            }
            for j in 0..=rm {
                if used[j] {
                    if p[j] != NONE {
                        u[p[j]] += delta;
                    }
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == NONE {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == rm {
                break;
            }
        }
    }

    let mut out = Vec::new();
    for j in 0..rm {
        let i = p[j];
        if i == NONE {
            continue;
        }
        let (r, c) = if transposed { (j, i) } else { (i, j) };
        // BIG edges are the completion trick, not matches.
        if cost[r * m + c] >= 0 {
            out.push((r, c));
        }
    }
    out.sort_unstable();
    out
}

/// SPEC-004.1 Appendix A (comparator 41): the assignment computed on the
/// edge-induced subgraph — rows and columns holding at least one real edge,
/// taken in ascending original index; dummy completion applies to the
/// subgraph only.  Cardinality and total cost always equal `assign`'s; the
/// matched pair-set may differ where cost ties exist, because the dummy
/// columns of the full matrix no longer participate in tie-breaking.  That
/// is why 4.1 is a comparator bump and this sits beside `assign`, never in
/// place of it.
pub fn assign_sparse(n: usize, m: usize, cost: &[i32]) -> Vec<(usize, usize)> {
    debug_assert_eq!(cost.len(), n * m);
    let mut list: Vec<(u32, u32, i32)> = Vec::new();
    for r in 0..n {
        for c in 0..m {
            let v = cost[r * m + c];
            if v >= 0 {
                list.push((r as u32, c as u32, v));
            }
        }
    }
    assign_edges(n, m, &list)
}

/// `assign_sparse` on an edge list — `(row, col, cost)`, cost `0..=32767`,
/// in row-major order — without the dense matrix: the edge-induced
/// subgraph's rows and columns are ranked, the solving orientation's
/// adjacency is built by a counting pass, and the solver runs on it.  The
/// pair set is the dense path's, pair for pair (tested).
pub fn assign_edges(n: usize, m: usize, list: &[(u32, u32, i32)]) -> Vec<(usize, usize)> {
    if n == 0 || m == 0 || list.is_empty() {
        return Vec::new();
    }
    debug_assert!(list.windows(2).all(|w| (w[0].0, w[0].1) < (w[1].0, w[1].1)), "row-major, no duplicates");
    // ranks of the rows and columns holding an edge, ascending original index
    let mut rrank = vec![u32::MAX; n];
    let mut crank = vec![u32::MAX; m];
    for &(r, c, _) in list.iter() {
        rrank[r as usize] = 0;
        crank[c as usize] = 0;
    }
    let mut ri: Vec<usize> = Vec::new();
    for (r, k) in rrank.iter_mut().enumerate() {
        if *k == 0 {
            *k = ri.len() as u32;
            ri.push(r);
        }
    }
    let mut ci: Vec<usize> = Vec::new();
    for (c, k) in crank.iter_mut().enumerate() {
        if *k == 0 {
            *k = ci.len() as u32;
            ci.push(c);
        }
    }
    let (sn, sm) = (ri.len(), ci.len());
    let transposed = sn > sm;
    let (rn, rm) = if transposed { (sm, sn) } else { (sn, sm) };
    // adjacency in the solving orientation, each row ascending by column: in
    // row-major input that is the input order, and transposed it is the
    // input order bucketed by column (a stable counting pass)
    let mut start = vec![0u32; rn + 1];
    let mut edges = vec![(0u32, 0i32); list.len()];
    for &(r, c, _) in list.iter() {
        let row = if transposed { crank[c as usize] } else { rrank[r as usize] };
        start[row as usize + 1] += 1;
    }
    for k in 0..rn {
        start[k + 1] += start[k];
    }
    let mut fill: Vec<u32> = start[..rn].to_vec();
    for &(r, c, w) in list.iter() {
        let (row, col) = if transposed {
            (crank[c as usize], rrank[r as usize])
        } else {
            (rrank[r as usize], crank[c as usize])
        };
        edges[fill[row as usize] as usize] = (col, w);
        fill[row as usize] += 1;
    }
    let p = solve(rn, rm, &start, &edges);
    let mut out = Vec::new();
    for j in 0..rm {
        let i = p[j];
        if i == NONE {
            continue;
        }
        // BIG edges are the completion trick, not matches
        let adj = &edges[start[i] as usize..start[i + 1] as usize];
        if adj.binary_search_by(|e| e.0.cmp(&(j as u32))).is_err() {
            continue;
        }
        let (a, b) = if transposed { (j, i) } else { (i, j) };
        out.push((ri[a], ci[b]));
    }
    out.sort_unstable();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total(cost: &[i32], m: usize, sol: &[(usize, usize)]) -> i64 {
        sol.iter().map(|&(r, c)| cost[r * m + c] as i64).sum()
    }

    /// Exhaustive optimum by bitmask DP: (max cardinality, min cost among them).
    fn brute(n: usize, m: usize, cost: &[i32]) -> (usize, i64) {
        let full = 1usize << m;
        let mut best = vec![(0usize, 0i64); full]; // per used-column mask after all rows? do row DP
        // dp over rows: state = used column mask -> (card, cost) best
        let mut dp = vec![None::<(usize, i64)>; full];
        dp[0] = Some((0, 0));
        for r in 0..n {
            let mut nx = vec![None::<(usize, i64)>; full];
            for mask in 0..full {
                let Some((card, cst)) = dp[mask] else { continue };
                // skip row r
                upd(&mut nx[mask], (card, cst));
                for c in 0..m {
                    if mask & (1 << c) != 0 || cost[r * m + c] < 0 {
                        continue;
                    }
                    upd(&mut nx[mask | (1 << c)], (card + 1, cst + cost[r * m + c] as i64));
                }
            }
            dp = nx;
        }
        let mut ans = (0usize, 0i64);
        let mut have = false;
        for mask in 0..full {
            if let Some(s) = dp[mask] {
                if !have || better(s, ans) {
                    ans = s;
                    have = true;
                }
            }
        }
        let _ = &mut best;
        ans
    }
    fn better(a: (usize, i64), b: (usize, i64)) -> bool {
        a.0 > b.0 || (a.0 == b.0 && a.1 < b.1)
    }
    fn upd(slot: &mut Option<(usize, i64)>, s: (usize, i64)) {
        match slot {
            None => *slot = Some(s),
            Some(cur) => {
                if better(s, *cur) {
                    *slot = Some(s);
                }
            }
        }
    }

    #[test]
    fn beats_greedy_on_the_canonical_case() {
        // (0,0)=1 (0,1)=2 (1,0)=2 (1,1)=none — greedy strands row 1.
        let cost = [1, 2, 2, NO_EDGE];
        let sol = assign(2, 2, &cost);
        assert_eq!(sol, vec![(0, 1), (1, 0)]);
        assert_eq!(total(&cost, 2, &sol), 4);
    }

    #[test]
    fn exhaustive_cross_check() {
        // Every instance up to 7×7 on a deterministic LCG stream: cardinality
        // and cost must equal the enumerated optimum (SPEC-004 §20).
        let mut s: u64 = 0x9e3779b97f4a7c15;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for n in 1..=7usize {
            for m in 1..=7usize {
                for _case in 0..40 {
                    let cost: Vec<i32> = (0..n * m)
                        .map(|_| {
                            let r = next();
                            if r % 3 == 0 { NO_EDGE } else { (r >> 8) as i32 & 0x3fff }
                        })
                        .collect();
                    let sol = assign(n, m, &cost);
                    // validity: injective both sides, real edges only
                    let mut ru = vec![false; n];
                    let mut cu = vec![false; m];
                    for &(r, c) in &sol {
                        assert!(cost[r * m + c] >= 0);
                        assert!(!ru[r] && !cu[c]);
                        ru[r] = true;
                        cu[c] = true;
                    }
                    let (bc, bcost) = brute(n, m, &cost);
                    assert_eq!(sol.len(), bc, "cardinality {n}x{m}");
                    assert_eq!(total(&cost, m, &sol), bcost, "cost {n}x{m}");
                }
            }
        }
    }

    /// The lazy Hungarian returns the reference's pair SET — not just the same
    /// cardinality and cost — on tie-heavy, sparse and rectangular instances
    /// up to the 128 x 128 a bag produces.  Ties are where an implementation
    /// detail could pick a different optimum, so most cases are built of them.
    #[test]
    fn lazy_matches_reference_pair_for_pair() {
        let mut s: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for case in 0..1500 {
            let n = 1 + (next() % [8u64, 24, 64, 128][case % 4]) as usize;
            let m = 1 + (next() % [8u64, 24, 64, 128][(case / 4) % 4]) as usize;
            let density = [2u64, 5, 20, 60, 100][case % 5];
            let range = [1u64, 3, 9, 65, 32768][(case / 5) % 5];
            let cost: Vec<i32> = (0..n * m)
                .map(|_| if next() % 100 < density { (next() % range) as i32 } else { NO_EDGE })
                .collect();
            let want = assign_ref(n, m, &cost);
            assert_eq!(assign(n, m, &cost), want, "case {case} {n}x{m}");
            assert_eq!(assign_dense(n, m, &cost), want, "dense, case {case} {n}x{m}");
            assert_eq!(assign_sparse(n, m, &cost), {
                // the sparse wrapper over the reference
                let mut rh = vec![false; n];
                let mut ch = vec![false; m];
                for r in 0..n {
                    for c in 0..m {
                        if cost[r * m + c] >= 0 {
                            rh[r] = true;
                            ch[c] = true;
                        }
                    }
                }
                let ri: Vec<usize> = (0..n).filter(|&r| rh[r]).collect();
                let ci: Vec<usize> = (0..m).filter(|&c| ch[c]).collect();
                let mut sub = vec![NO_EDGE; ri.len() * ci.len()];
                for (a, &r) in ri.iter().enumerate() {
                    for (b, &c) in ci.iter().enumerate() {
                        sub[a * ci.len() + b] = cost[r * m + c];
                    }
                }
                let mut o: Vec<(usize, usize)> =
                    assign_ref(ri.len(), ci.len(), &sub).iter().map(|&(a, b)| (ri[a], ci[b])).collect();
                o.sort_unstable();
                o
            }, "sparse case {case}");
        }
    }

    /// The shape the local bags actually produce, where the completion edges
    /// carry most phases: large, very sparse, many-to-many repeated codes
    /// (identical rows and columns), tiny cost ranges — so long alternating
    /// trees, potentials on the scale of BIG, and ties everywhere.
    #[test]
    fn completion_heavy_instances_match_the_reference() {
        let mut s: u64 = 0x51ce_7a1e_0dd5_eed5;
        let mut next = move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        for case in 0..600 {
            let n = 40 + (next() % 89) as usize;
            let m = 40 + (next() % 89) as usize;
            let per_mille = [5u64, 10, 20, 40][case % 4];
            let range = [1u64, 2, 4, 9][(case / 4) % 4];
            let mut cost: Vec<i32> = (0..n * m)
                .map(|_| if next() % 1000 < per_mille { (next() % range) as i32 } else { NO_EDGE })
                .collect();
            // repeated codes: copy some rows and some columns over others
            for _ in 0..(case % 7) * 3 {
                let (a, b) = ((next() % n as u64) as usize, (next() % n as u64) as usize);
                for c in 0..m {
                    cost[b * m + c] = cost[a * m + c];
                }
                let (x, y) = ((next() % m as u64) as usize, (next() % m as u64) as usize);
                for r in 0..n {
                    cost[r * m + y] = cost[r * m + x];
                }
            }
            let want = assign_ref(n, m, &cost);
            assert_eq!(assign(n, m, &cost), want, "case {case} {n}x{m}");
            let sp = assign_sparse(n, m, &cost);
            let total_sp: i64 = sp.iter().map(|&(r, c)| cost[r * m + c] as i64).sum();
            assert_eq!((sp.len(), total_sp), (want.len(), total(&cost, m, &want)), "sparse optimum, case {case}");
        }
    }

    #[test]
    fn deterministic_and_shape_edges() {
        let cost = [3, 3, 3, 3, 3, 3];
        let a = assign(2, 3, &cost);
        let b = assign(2, 3, &cost);
        assert_eq!(a, b);
        assert_eq!(a, vec![(0, 0), (1, 1)]); // ties resolve to smallest columns
        assert!(assign(0, 5, &[]).is_empty());
        assert!(assign(3, 3, &[NO_EDGE; 9]).is_empty());
        // rectangular tall: transposition path
        let tall = [1, NO_EDGE, 5, 2, NO_EDGE, 1]; // 3 rows × 2 cols
        let s = assign(3, 2, &tall);
        let (bc, bcost) = {
            // quick manual optimum: rows {0,2} → cost 1+1=2, card 2
            (2usize, 2i64)
        };
        assert_eq!(s.len(), bc);
        assert_eq!(total(&tall, 2, &s), bcost);
    }
}
