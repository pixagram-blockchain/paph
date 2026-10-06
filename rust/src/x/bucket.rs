//! XBucket (PAPH-X §8.2–§8.4) — the per-side projection index.
//!
//! Twenty-four 8-bit projections of the 256-bit descriptor, each a fixed
//! choice of eight bit positions from the profile's table.  The family is
//! mirror-closed: projection `p + 12` reads the positions of projection `p`
//! moved by 128, and a mirrored descriptor is the stored one with its halves
//! exchanged (SPEC-003 §8.6), so
//!
//! ```text
//!     code_p(mirror(d)) == code_{p xor 12}(d)
//! ```
//!
//! One descriptor load gives both hypotheses their codes, and one bucket
//! index per side serves both (§9.5, §41.4).
//!
//! The index is CSR: for every projection, 256 buckets of keypoint ids in
//! ascending order.  It is derived data, rebuilt from the wire at prepare
//! time, never part of the consensus fingerprint (§8.3, §17).

use super::profile::{LSH_BASE, LSH_BITS, LSH_BUCKETS, LSH_PROJECTIONS};
#[cfg(test)]
use super::profile::XProfile;
use crate::geom42::Desc4;

#[inline(always)]
pub fn desc_bit(d: &Desc4, b: u8) -> u8 {
    ((d.q[(b >> 6) as usize] >> (b & 63)) & 1) as u8
}

/// The code of one projection: bit `i` of the code is descriptor bit
/// `pos[i]`.
#[inline(always)]
pub fn project(d: &Desc4, pos: &[u8; LSH_BITS]) -> u16 {
    let mut c = 0u16;
    for (i, &b) in pos.iter().enumerate() {
        c |= (desc_bit(d, b) as u16) << i;
    }
    c
}

/// The mirror partner of projection `p`.
#[inline(always)]
pub const fn mirror_proj(p: usize) -> usize {
    if p < LSH_BASE {
        p + LSH_BASE
    } else {
        p - LSH_BASE
    }
}

/// bits of the coarse bucket (the code's top byte)
const COARSE: usize = 8;
const FINE_BITS: usize = LSH_BITS - COARSE;
const FINE_MASK: u16 = (1 << FINE_BITS) - 1;

/// Per projection, the keypoint ids sorted by code.  A coarse table of 257
/// offsets on the top eight code bits finds the run of ids whose codes
/// share that prefix (a few entries), and the low bits beside each id
/// finish the lookup — so a 4096-bucket projection costs the same 12 KiB of
/// offsets a 256-bucket one did (§8.3).
pub struct XBucketIndex {
    pub n: usize,
    /// `codes[i * 24 + p]`: projection `p` of descriptor `i`, row-major
    /// so one query reads one cache line.  Bit 15 is set when the
    /// descriptor's own bucket for that projection holds more than
    /// `hot_cap` entries — the side's half of the symmetric hot-bucket rule,
    /// precomputed so a query never looks its own buckets up
    pub codes: Vec<u16>,
    pub hot_cap: usize,
    /// `offsets[p * 257 + hi]`: start of the coarse bucket `hi` of
    /// projection `p` in `ids` / `low`
    pub offsets: Vec<u16>,
    /// `present[p * 256 + hi]`: bit `low` set when the coarse bucket holds
    /// an entry with that low code — most lookups end here, on one load
    pub present: Vec<u16>,
    /// keypoint ids, projection by projection, sorted by (code, id)
    pub ids: Vec<u16>,
    /// the low `FINE_BITS` of each entry's code, parallel to `ids`
    pub low: Vec<u8>,
}

/// the code without its hot flag
pub const CODE_MASK: u16 = (1 << LSH_BITS) - 1;
pub const HOT_FLAG: u16 = 0x8000;

impl XBucketIndex {
    pub fn build(desc: &[Desc4], tab: &[[u8; LSH_BITS]; LSH_PROJECTIONS], hot_cap: usize) -> XBucketIndex {
        let n = desc.len();
        let mut codes = vec![0u16; LSH_PROJECTIONS * n];
        for (i, d) in desc.iter().enumerate() {
            for p in 0..LSH_PROJECTIONS {
                codes[i * LSH_PROJECTIONS + p] = project(d, &tab[p]);
            }
        }
        let mut offsets = vec![0u16; LSH_PROJECTIONS * 257];
        let mut present = vec![0u16; LSH_PROJECTIONS * 256];
        let mut ids = vec![0u16; LSH_PROJECTIONS * n];
        let mut low = vec![0u8; LSH_PROJECTIONS * n];
        let mut order: Vec<u16> = Vec::with_capacity(n);
        for p in 0..LSH_PROJECTIONS {
            // counting sort by full code, ties by id
            let mut cnt = vec![0u16; LSH_BUCKETS + 1];
            for i in 0..n {
                cnt[codes[i * LSH_PROJECTIONS + p] as usize + 1] += 1;
            }
            for c in 0..LSH_BUCKETS {
                cnt[c + 1] += cnt[c];
            }
            order.clear();
            order.resize(n, 0);
            let mut fill = cnt.clone();
            for i in 0..n {
                let c = codes[i * LSH_PROJECTIONS + p] as usize;
                order[fill[c] as usize] = i as u16;
                fill[c] += 1;
            }
            let off = &mut offsets[p * 257..(p + 1) * 257];
            for hi in 0..=256usize {
                off[hi] = cnt[(hi << FINE_BITS).min(LSH_BUCKETS)];
            }
            for (k, &i) in order.iter().enumerate() {
                ids[p * n + k] = i;
                let c = codes[i as usize * LSH_PROJECTIONS + p];
                low[p * n + k] = (c & FINE_MASK) as u8;
                present[p * 256 + (c >> FINE_BITS) as usize] |= 1 << (c & FINE_MASK);
            }
            // the hot flag: this descriptor's own bucket is over the cap
            for i in 0..n {
                let c = codes[i * LSH_PROJECTIONS + p] as usize;
                if (cnt[c + 1] - cnt[c]) as usize > hot_cap {
                    codes[i * LSH_PROJECTIONS + p] |= HOT_FLAG;
                }
            }
        }
        XBucketIndex { n, codes, offsets, present, ids, low, hot_cap }
    }

    /// Projection `p` of descriptor `i`, with the hot flag in bit 15.
    #[inline(always)]
    pub fn code(&self, p: usize, i: usize) -> u16 {
        self.codes[i * LSH_PROJECTIONS + p]
    }

    /// The ids in bucket `c` of projection `p`.
    #[inline(always)]
    pub fn bucket(&self, p: usize, c: u16) -> &[u16] {
        let c = c & CODE_MASK;
        let hi = (c >> FINE_BITS) as usize;
        let want = (c & FINE_MASK) as u8;
        if self.present[p * 256 + hi] & (1 << want) == 0 {
            return &[];
        }
        let o = p * 257 + hi;
        let (s, e) = (self.offsets[o] as usize, self.offsets[o + 1] as usize);
        let base = p * self.n;
        let low = &self.low[base + s..base + e];
        // the run is sorted by low code; on clustered art a coarse run can
        // hold dozens of entries, so the bounds are found by bisection
        let (a, b) = if low.len() <= 8 {
            let mut a = 0usize;
            while a < low.len() && low[a] < want {
                a += 1;
            }
            let mut b = a;
            while b < low.len() && low[b] == want {
                b += 1;
            }
            (a, b)
        } else {
            (low.partition_point(|&l| l < want), low.partition_point(|&l| l <= want))
        };
        &self.ids[base + s + a..base + s + b]
    }

    #[inline(always)]
    pub fn bucket_len(&self, p: usize, c: u16) -> usize {
        self.bucket(p, c).len()
    }

    /// Bytes this index holds (for the report's cost diagnostics).
    pub fn bytes(&self) -> usize {
        2 * self.codes.len() + 2 * self.offsets.len() + 2 * self.present.len() + 2 * self.ids.len() + self.low.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom42::pack_desc;

    fn desc(seed: u64) -> Desc4 {
        let mut s = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_add(1);
        let mut d = [0u32; 8];
        for w in d.iter_mut() {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            *w = (s >> 16) as u32;
        }
        pack_desc(&d)
    }

    #[test]
    fn mirror_is_a_projection_swap() {
        let xp = XProfile::x1();
        let tab = xp.projections();
        for i in 0..200u64 {
            let d = desc(i);
            let m = Desc4 { q: [d.q[2], d.q[3], d.q[0], d.q[1]] };
            for p in 0..LSH_PROJECTIONS {
                assert_eq!(project(&m, &tab[p]), project(&d, &tab[mirror_proj(p)]), "desc {i} proj {p}");
            }
            let _ = CODE_MASK;
        }
    }

    #[test]
    fn csr_holds_every_id_once_in_order() {
        let xp = XProfile::x1();
        for n in [0usize, 1, 7, 100, 512] {
            let ds: Vec<Desc4> = (0..n as u64).map(desc).collect();
            let ix = XBucketIndex::build(&ds, &xp.projections(), 32);
            for p in 0..LSH_PROJECTIONS {
                let mut seen = vec![false; n];
                let mut tot = 0;
                for c in 0..LSH_BUCKETS as u16 {
                    let b = ix.bucket(p, c);
                    assert_eq!(b.len(), ix.bucket_len(p, c));
                    for w in b.windows(2) {
                        assert!(w[0] < w[1], "ascending");
                    }
                    for &i in b {
                        assert_eq!(ix.code(p, i as usize) & CODE_MASK, c);
                        assert!(!seen[i as usize]);
                        seen[i as usize] = true;
                        tot += 1;
                    }
                }
                assert_eq!(tot, n);
            }
        }
    }

    /// Unrelated descriptors collide on a projection about 1/256 of the
    /// time, so the touched set per query is tens of candidates, not 512
    /// (§8.6) — the number the whole design rests on.
    #[test]
    fn random_collisions_touch_tens_not_hundreds() {
        let xp = XProfile::x1();
        let ds: Vec<Desc4> = (0..512u64).map(desc).collect();
        let ix = XBucketIndex::build(&ds, &xp.projections(), 32);
        let mut total = 0usize;
        for q in 0..512u64 {
            let d = desc(10_000 + q);
            let tab = xp.projections();
            let mut mark = [false; 512];
            for p in 0..LSH_PROJECTIONS {
                for &j in ix.bucket(p, project(&d, &tab[p])) {
                    mark[j as usize] = true;
                }
            }
            total += mark.iter().filter(|&&m| m).count();
        }
        let avg = total as f64 / 512.0;
        println!("average touched candidates per query: {avg:.1}");
        assert!(avg > 1.0 && avg < 30.0, "{avg}");
    }
}
