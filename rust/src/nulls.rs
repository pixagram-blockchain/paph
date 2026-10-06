//! Structured null families (SPEC-004 §9, Appendix D).  Every map here is a
//! closed-form permutation of stored bits or indices — no PRNG anywhere in
//! consensus-visible behaviour (P8).  Aggregation over a family is MAX (§9.1):
//! the measurement must beat the family's best accident, not its average one.

/// Rotate a 64-bit code stored as `(hi, lo)` left by `k`.
pub fn rot64(hi: u32, lo: u32, k: u32) -> (u32, u32) {
    let k = k & 63;
    if k == 0 {
        (hi, lo)
    } else if k == 32 {
        (lo, hi)
    } else if k < 32 {
        ((hi << k) | (lo >> (32 - k)), (lo << k) | (hi >> (32 - k)))
    } else {
        let j = k - 32;
        ((lo << j) | (hi >> (32 - j)), (hi << j) | (lo >> (32 - j)))
    }
}

/// Bit-reversal of the full 64-bit code: bit 0 ↔ bit 63.
/// As words: `(rev32(lo), rev32(hi))`.
pub fn bitrev64(hi: u32, lo: u32) -> (u32, u32) {
    (lo.reverse_bits(), hi.reverse_bits())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocalNull {
    Rot16,
    Rot32,
    Rot48,
    BitRev,
}

/// LN, in specification order (§9.2).
pub const LOCAL_FAMILY: [LocalNull; 4] = [LocalNull::Rot16, LocalNull::Rot32, LocalNull::Rot48, LocalNull::BitRev];

pub fn apply_local(n: LocalNull, hi: u32, lo: u32) -> (u32, u32) {
    match n {
        LocalNull::Rot16 => rot64(hi, lo, 16),
        LocalNull::Rot32 => rot64(hi, lo, 32),
        LocalNull::Rot48 => rot64(hi, lo, 48),
        LocalNull::BitRev => bitrev64(hi, lo),
    }
}

/// GN cyclic-shift offsets for a correspondence list of length `n`:
/// dedup{⌊n/2⌋, ⌊n/3⌋, ⌊n/5⌋, ⌊2n/3⌋} \ {0}, in that order (§9.3).
pub fn shift_offsets(n: usize) -> Vec<usize> {
    let mut out = Vec::with_capacity(4);
    for p in [n / 2, n / 3, n / 5, (2 * n) / 3] {
        if p != 0 && p < n && !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// GN shift: re-deal the B side by `p` positions.  Pairing statistics
/// (counts, distances) preserved; geometry destroyed.
pub fn apply_shift(corr: &[(usize, usize, i32)], p: usize) -> Vec<(usize, usize, i32)> {
    let n = corr.len();
    (0..n).map(|i| (corr[i].0, corr[(i + p) % n].1, corr[i].2)).collect()
}

/// GN reversal: k ↦ n−1−k on the B side.
pub fn apply_reverse(corr: &[(usize, usize, i32)]) -> Vec<(usize, usize, i32)> {
    let n = corr.len();
    (0..n).map(|i| (corr[i].0, corr[n - 1 - i].1, corr[i].2)).collect()
}

/// GN shift on weighted correspondences (§12.2 + App D): row i keeps its own
/// descriptor distances and confidence; only the B-side geometry is re-dealt.
pub fn apply_shift4(corr: &[(usize, usize, i32, i32)], p: usize) -> Vec<(usize, usize, i32, i32)> {
    let n = corr.len();
    (0..n).map(|i| (corr[i].0, corr[(i + p) % n].1, corr[i].2, corr[i].3)).collect()
}

/// GN reversal on weighted correspondences.
pub fn apply_reverse4(corr: &[(usize, usize, i32, i32)]) -> Vec<(usize, usize, i32, i32)> {
    let n = corr.len();
    (0..n).map(|i| (corr[i].0, corr[n - 1 - i].1, corr[i].2, corr[i].3)).collect()
}

/// Family aggregation (§9.1).
pub fn max_ctl(vals: &[i64]) -> i64 {
    vals.iter().copied().max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_maps_are_popcount_preserving_involutions_or_cycles() {
        let cases = [(0x12345678u32, 0x9abcdef0u32), (0, 1), (u32::MAX, 0), (0xdeadbeef, 0xfeedc0de)];
        for (hi, lo) in cases {
            let w = (hi.count_ones() + lo.count_ones()) as i32;
            for f in LOCAL_FAMILY {
                let (h2, l2) = apply_local(f, hi, lo);
                assert_eq!((h2.count_ones() + l2.count_ones()) as i32, w, "{f:?} popcount");
            }
            // involutions / inverse pairs
            let (h, l) = apply_local(LocalNull::BitRev, hi, lo);
            assert_eq!(apply_local(LocalNull::BitRev, h, l), (hi, lo));
            let (h, l) = apply_local(LocalNull::Rot32, hi, lo);
            assert_eq!(apply_local(LocalNull::Rot32, h, l), (hi, lo));
            let (h, l) = apply_local(LocalNull::Rot16, hi, lo);
            assert_eq!(apply_local(LocalNull::Rot48, h, l), (hi, lo));
        }
        // Appendix D worked example: bitrev as words
        assert_eq!(bitrev64(0x80000000, 0x00000001), (0x80000000, 0x00000001));
        assert_eq!(bitrev64(1, 0), (0, 0x80000000));
    }

    #[test]
    fn offsets_dedup_and_order() {
        assert_eq!(shift_offsets(12), vec![6, 4, 2, 8]);
        assert_eq!(shift_offsets(6), vec![3, 2, 1, 4]);
        assert_eq!(shift_offsets(2), vec![1]);      // n/3 = n/5 = 0 skipped, 2n/3 = 1 dup
        assert_eq!(shift_offsets(1), Vec::<usize>::new());
        assert_eq!(shift_offsets(0), Vec::<usize>::new());
        for n in 1..300 {
            let o = shift_offsets(n);
            for (i, p) in o.iter().enumerate() {
                assert!(*p > 0 && *p < n);
                assert!(!o[..i].contains(p));
            }
        }
    }

    #[test]
    fn shifts_preserve_multisets() {
        let corr: Vec<(usize, usize, i32)> = (0..9).map(|i| (i, 100 + i, i as i32)).collect();
        for p in shift_offsets(corr.len()) {
            let s = apply_shift(&corr, p);
            let mut a: Vec<_> = s.iter().map(|c| c.0).collect();
            let mut b: Vec<_> = s.iter().map(|c| c.1).collect();
            a.sort_unstable();
            b.sort_unstable();
            assert_eq!(a, (0..9).collect::<Vec<_>>());
            assert_eq!(b, (100..109).collect::<Vec<_>>());
            assert_ne!(s, corr);
        }
        let r = apply_reverse(&corr);
        assert_eq!(r[0], (0, 108, 0));
        assert_eq!(apply_reverse(&r), corr);
        // weighted variants: confidence stays with the row
        let c4: Vec<(usize, usize, i32, i32)> = (0..5).map(|i| (i, 50 + i, i as i32, 10 + i as i32)).collect();
        let s4 = apply_shift4(&c4, 2);
        assert_eq!(s4[0], (0, 52, 0, 10));
        assert_eq!(s4[4], (4, 51, 4, 14));
        let r4 = apply_reverse4(&c4);
        assert_eq!(r4[0], (0, 54, 0, 10));
        assert_eq!(apply_reverse4(&r4), c4);
        assert_eq!(max_ctl(&[3, 9, 1]), 9);
        assert_eq!(max_ctl(&[]), 0);
    }
}
