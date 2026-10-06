//! Spatial support (SPEC-004 §11).  Matched positions live in the wire's
//! 16-bit normalised frame; a g×g grid turns them into four integers the
//! lattice can consume: occupied cells, coverage, bounding-box area, and the
//! concentration of the largest 4-connected region.  Labelling is two-pass
//! row-major union-find, min-label wins — deterministic by construction.

use crate::config::SCALE;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Coverage {
    pub g: u8,
    pub occupied: i64,
    /// occupied · SCALE / g²
    pub coverage: i64,
    /// bounding box of occupied cells, in cells (0 when empty)
    pub bbox_cells: i64,
    /// largest 4-connected occupied region · SCALE / occupied (0 when empty)
    pub concentration: i64,
    /// per-cell match counts, row-major g×g
    pub counts: Vec<u16>,
}

/// §11: cell(x) = (x · g) >> 16 on the 16-bit frame.
pub fn cell(x: i64, g: u8) -> usize {
    ((x.clamp(0, 65535) as u32 * g as u32) >> 16) as usize
}

fn find(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    if ra != rb {
        // min root wins: label choice never depends on visit order
        let (lo, hi) = if ra < rb { (ra, rb) } else { (rb, ra) };
        parent[hi] = lo;
    }
}

pub fn coverage(points: &[(i64, i64)], g: u8) -> Coverage {
    let gs = g as usize;
    let mut counts = vec![0u16; gs * gs];
    for &(x, y) in points {
        let c = cell(y, g) * gs + cell(x, g);
        counts[c] = counts[c].saturating_add(1);
    }
    let occ: Vec<usize> = (0..gs * gs).filter(|&i| counts[i] > 0).collect();
    let occupied = occ.len() as i64;
    if occupied == 0 {
        return Coverage { g, occupied: 0, coverage: 0, bbox_cells: 0, concentration: 0, counts };
    }
    let (mut x0, mut x1, mut y0, mut y1) = (gs, 0usize, gs, 0usize);
    for &i in &occ {
        let (cx, cy) = (i % gs, i / gs);
        x0 = x0.min(cx);
        x1 = x1.max(cx);
        y0 = y0.min(cy);
        y1 = y1.max(cy);
    }
    let bbox_cells = ((x1 - x0 + 1) * (y1 - y0 + 1)) as i64;

    let mut parent: Vec<usize> = (0..gs * gs).collect();
    for cy in 0..gs {
        for cx in 0..gs {
            let i = cy * gs + cx;
            if counts[i] == 0 {
                continue;
            }
            if cx + 1 < gs && counts[i + 1] > 0 {
                union(&mut parent, i, i + 1);
            }
            if cy + 1 < gs && counts[i + gs] > 0 {
                union(&mut parent, i, i + gs);
            }
        }
    }
    let mut sizes = vec![0i64; gs * gs];
    let mut largest = 0i64;
    for &i in &occ {
        let r = find(&mut parent, i);
        sizes[r] += 1;
        largest = largest.max(sizes[r]);
    }
    Coverage {
        g,
        occupied,
        coverage: occupied * SCALE / (gs * gs) as i64,
        bbox_cells,
        concentration: largest * SCALE / occupied,
        counts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// place a point at the centre of grid cell (cx, cy) for g=4
    fn at(cx: i64, cy: i64) -> (i64, i64) {
        (cx * 16384 + 8192, cy * 16384 + 8192)
    }

    #[test]
    fn empty_and_single() {
        let c = coverage(&[], 4);
        assert_eq!((c.occupied, c.coverage, c.bbox_cells, c.concentration), (0, 0, 0, 0));
        let c = coverage(&[at(2, 1), at(2, 1)], 4);
        assert_eq!(c.occupied, 1);
        assert_eq!(c.coverage, SCALE / 16);
        assert_eq!(c.bbox_cells, 1);
        assert_eq!(c.concentration, SCALE);
        assert_eq!(c.counts[1 * 4 + 2], 2);
    }

    #[test]
    fn l_shape_is_one_region_diagonal_is_two() {
        // L: (0,0)(0,1)(1,1) — 4-connected
        let c = coverage(&[at(0, 0), at(0, 1), at(1, 1)], 4);
        assert_eq!(c.occupied, 3);
        assert_eq!(c.concentration, SCALE);
        assert_eq!(c.bbox_cells, 4);
        // diagonal: (0,0)(1,1) — NOT connected under 4-connectivity
        let c = coverage(&[at(0, 0), at(1, 1)], 4);
        assert_eq!(c.occupied, 2);
        assert_eq!(c.concentration, SCALE / 2);
        // two blobs, larger wins: {(0,0)(1,0)(0,1)} + {(3,3)}
        let c = coverage(&[at(0, 0), at(1, 0), at(0, 1), at(3, 3)], 4);
        assert_eq!(c.occupied, 4);
        assert_eq!(c.concentration, 3 * SCALE / 4);
        assert_eq!(c.bbox_cells, 16);
    }

    #[test]
    fn full_grid_and_frame_edges() {
        let mut pts = Vec::new();
        for cy in 0..4 {
            for cx in 0..4 {
                pts.push(at(cx, cy));
            }
        }
        let c = coverage(&pts, 4);
        assert_eq!(c.occupied, 16);
        assert_eq!(c.coverage, SCALE);
        assert_eq!(c.concentration, SCALE);
        // 65535 lands in the last cell; clamping handles strays
        assert_eq!(cell(65535, 4), 3);
        assert_eq!(cell(0, 4), 0);
        assert_eq!(cell(70000, 4), 3);
        assert_eq!(cell(-5, 4), 0);
    }
}
