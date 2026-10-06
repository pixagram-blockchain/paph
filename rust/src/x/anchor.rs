//! XAnchor (PAPH-X §9.1) — a deterministic order over a side's keypoints.
//!
//! The first 96 keypoints in this order are the anchor tier; 160, 256 and
//! 512 are the expansion tiers.  The order is the SPEC-004.2 §3 quality
//! greedy run over the keypoints the wire holds — strength, spatial novelty,
//! scale novelty, descriptor novelty, with the same weights and the same
//! tie rule — so an anchor set is the same kind of thing the Tier-2 budget
//! already is: the most independent evidence first.  Wire order is
//! content-sorted, not selection-sorted, so the order has to be re-derived;
//! it is derived once per prepared side and never per pair.

use crate::config::{clamp, idiv, MAX_KP_COUNT};
use crate::keypoints::{hamming_min_into, pack4, Keypoint, DESC_NOVEL_STEP, N_BITS, Q_W_DESC, Q_W_SCALE, Q_W_SPATIAL, Q_W_STRENGTH, SCALE_Q, SEL_GRID};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

#[inline]
fn cell(k: &Keypoint) -> usize {
    (idiv(k.y as i64 * SEL_GRID, 65536) * SEL_GRID + idiv(k.x as i64 * SEL_GRID, 65536)) as usize
}

#[inline(always)]
fn recip(k: u32) -> i32 {
    (SCALE_Q / (1 + k as i64)) as i32
}

/// Every keypoint index, best first.  `kp` is the compared list in wire
/// order; the result indexes into it.
pub fn anchor_order(kp: &[Keypoint]) -> Vec<u16> {
    let n = kp.len().min(MAX_KP_COUNT);
    if n == 0 {
        return Vec::new();
    }
    // the pooled order of §3: strength descending, then level, y, x — the
    // order the selector's "ties to the lower index" refers to
    let mut pool: Vec<usize> = (0..n).collect();
    pool.sort_by(|&a, &b| {
        let (p, q) = (&kp[a], &kp[b]);
        q.s.cmp(&p.s).then(p.level.cmp(&q.level)).then(p.y.cmp(&q.y)).then(p.x.cmp(&q.x)).then(a.cmp(&b))
    });
    let smax = pool.iter().map(|&i| kp[i].s as i64).max().unwrap_or(1).max(1);
    let dpack: Vec<[u64; 4]> = pool.iter().map(|&i| pack4(&kp[i].desc)).collect();
    let qs: Vec<i32> = pool.iter().map(|&i| clamp(kp[i].s as i64 * SCALE_Q / smax, 0, SCALE_Q) as i32).collect();
    let cells: Vec<u16> = pool.iter().map(|&i| cell(&kp[i]) as u16).collect();
    let levels: Vec<u16> = pool.iter().map(|&i| kp[i].level as u16).collect();
    let mut cellc = vec![0u32; (SEL_GRID * SEL_GRID) as usize];
    let mut levc = vec![0u32; 256];
    let mut dmin = vec![N_BITS as i32; n];
    let mut taken = vec![false; n];

    let score = |i: usize, cellc: &[u32], levc: &[u32], dmin: &[i32]| -> i32 {
        let q_desc = (dmin[i] * DESC_NOVEL_STEP).min(SCALE_Q as i32);
        (Q_W_STRENGTH as i32 * qs[i]
            + Q_W_SPATIAL as i32 * recip(cellc[cells[i] as usize])
            + Q_W_SCALE as i32 * recip(levc[levels[i] as usize])
            + Q_W_DESC as i32 * q_desc)
            / 100
    };
    let mut heap: BinaryHeap<(i32, Reverse<usize>)> =
        (0..n).map(|i| (score(i, &cellc, &levc, &dmin), Reverse(i))).collect();
    let mut out: Vec<u16> = Vec::with_capacity(n);
    while out.len() < n {
        let mut best = usize::MAX;
        while let Some((stale, Reverse(i))) = heap.pop() {
            let q = score(i, &cellc, &levc, &dmin);
            if q == stale {
                best = i;
                break;
            }
            heap.push((q, Reverse(i)));
        }
        if best == usize::MAX {
            break;
        }
        taken[best] = true;
        cellc[cells[best] as usize] += 1;
        levc[levels[best] as usize] += 1;
        out.push(pool[best] as u16);
        hamming_min_into(&dpack[best], &dpack, &taken, &mut dmin);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keypoints::select_quality;

    fn desc(seed: u64) -> [u32; 8] {
        let mut s = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_add(1);
        let mut d = [0u32; 8];
        for w in d.iter_mut() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            *w = (s >> 16) as u32;
        }
        d
    }

    /// The anchor order of a list IS the §3 selection order of that list
    /// when the whole list is the pool: the first k anchors are exactly
    /// `select_quality(list, k)` in pick order.
    #[test]
    fn anchors_are_the_quality_selection_in_pick_order() {
        let mut kp: Vec<Keypoint> = (0..300u64)
            .map(|i| Keypoint {
                desc: if i % 9 == 0 { desc(1) } else { desc(i) },
                x: ((i * 7919) % 65535) as i32,
                y: ((i * 104729) % 65535) as i32,
                level: (i % 4) as u8,
                sec: (i % 64) as u8,
                s: ((i * 37) % 2000) as u16,
            })
            .collect();
        // the pooled order the selector assumes
        kp.sort_by(|a, b| b.s.cmp(&a.s).then(a.level.cmp(&b.level)).then(a.y.cmp(&b.y)).then(a.x.cmp(&b.x)));
        let order = anchor_order(&kp);
        assert_eq!(order.len(), kp.len());
        let mut seen = vec![false; kp.len()];
        for &i in order.iter() {
            assert!(!seen[i as usize]);
            seen[i as usize] = true;
        }
        for k in [8usize, 96, 160] {
            let sel = select_quality(&kp, k);
            for (j, s) in sel.iter().enumerate() {
                let o = &kp[order[j] as usize];
                assert_eq!((s.x, s.y, s.level, s.s, s.desc), (o.x, o.y, o.level, o.s, o.desc), "pick {j} of {k}");
            }
        }
        // and shuffling the wire order does not change which keypoints lead
        let mut shuffled = kp.clone();
        shuffled.reverse();
        let o2 = anchor_order(&shuffled);
        for j in 0..96 {
            let (a, b) = (&kp[order[j] as usize], &shuffled[o2[j] as usize]);
            assert_eq!((a.x, a.y, a.desc), (b.x, b.y, b.desc));
        }
    }
}
