//! PAPH-SI cells, signatures, probes and the candidate score (SPEC-SI §4–§5).
//!
//! A work's **signature** is what an index stores: one fine cell (0..255) per
//! quantised family it can measure, and the band keys of its two MinHash
//! families — 104 bytes.  A **query** is a signature plus, per quantised
//! family, the probe cells in order of how likely a copy is to have landed in
//! them (the query's own cell first), and the evidence weights.  The score of
//! a candidate is the sum of the weights of the levels its families reach; it
//! is defined here once, and every index (in memory, SQL, or a plain scan)
//! must reproduce it exactly.

use super::features::{families, FAMILIES};
use super::profile::*;
use crate::prepared::Prepared;
use crate::x::route::{XRoute, RF_BAND, RF_LOCAL};
use crate::x::XBound;

/// Presence bits of a signature: bit f for quantised family f, then the two
/// MinHash families.
pub const P_LOCAL: u8 = 1 << FAMILY_LOCAL;
pub const P_BAND: u8 = 1 << FAMILY_BAND;

/// Bytes of a stored signature.
pub const SIG_BYTES: usize = 8 + 2 * LOCAL_BANDS + 2 * BAND_BANDS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SiSig {
    pub present: u8,
    /// fine cell per quantised family (0 where absent)
    pub cells: [u8; FAMILIES],
    pub local: [u16; LOCAL_BANDS],
    pub band: [u16; BAND_BANDS],
}

impl Default for SiSig {
    fn default() -> Self {
        SiSig { present: 0, cells: [0; FAMILIES], local: [0; LOCAL_BANDS], band: [0; BAND_BANDS] }
    }
}

/// The four projections of one family vector, integer exact.
#[inline]
pub fn project(cb: &Codebook, x: &[i32]) -> [i64; AXES] {
    let d = cb.mean.len();
    let mut z = [0i64; AXES];
    for (k, zk) in z.iter_mut().enumerate() {
        let w = &cb.proj[k * d..(k + 1) * d];
        let mut s = 0i64;
        for i in 0..d {
            s += w[i] as i64 * (x[i] - cb.mean[i]) as i64;
        }
        *zk = s;
    }
    z
}

/// Bin of each axis: how many edges lie at or below the value.
#[inline]
pub fn bins(cb: &Codebook, z: &[i64; AXES]) -> [u8; AXES] {
    let mut b = [0u8; AXES];
    for k in 0..AXES {
        b[k] = cb.thr[k].iter().filter(|&&t| t <= z[k]).count() as u8;
    }
    b
}

/// The fine cell of a bin vector: base-4 digits, axis 0 lowest.
#[inline]
pub fn fine(b: &[u8; AXES]) -> u8 {
    let mut c = 0usize;
    for k in (0..AXES).rev() {
        c = c * BINS + b[k] as usize;
    }
    c as u8
}

/// The coarse cell (0..15) a fine cell belongs to: the median bit of each
/// axis.  Exactly sixteen fine cells share each coarse cell.
#[inline]
pub fn coarse(fine: u8) -> u8 {
    let mut c = 0u8;
    let mut f = fine as usize;
    for k in 0..AXES {
        c |= (((f % BINS) >> 1) as u8) << k;
        f /= BINS;
    }
    c
}

/// The cell a family vector lands in.
pub fn cell(cb: &Codebook, x: &[i32]) -> u8 {
    fine(&bins(cb, &project(cb, x)))
}

/// Probe order (SPEC-SI §5.3): the query's own cell, then every cell reached
/// by moving one or more axes one bin down or up across an edge, cheapest
/// first, where crossing axis k costs ⌊d·1024/σ_k⌋² (d the distance to that
/// edge, capped at 2^20 before squaring) summed over the axes moved — σ_k the
/// axis' transform noise — the cells a copy is likeliest to have drifted
/// into.  Ties go to the lower cell; the order is exact integer arithmetic.
pub fn probe_order(cb: &Codebook, x: &[i32], m: usize, out: &mut [u8; MAX_PROBES]) -> usize {
    let z = project(cb, x);
    let b = bins(cb, &z);
    let home = fine(&b);
    // per axis: (bin delta, cost) options
    let mut opt = [[(0i8, 0u64); 3]; AXES];
    let mut nopt = [1usize; AXES];
    for k in 0..AXES {
        let bk = b[k] as usize;
        let norm = |d: i64| -> u64 {
            let q = ((d.max(0) as i128) * 1024 / cb.sig[k].max(1) as i128).min(1 << 20) as u64;
            q * q
        };
        if bk > 0 {
            opt[k][nopt[k]] = (-1, norm(z[k] - cb.thr[k][bk - 1]));
            nopt[k] += 1;
        }
        if bk < BINS - 1 {
            opt[k][nopt[k]] = (1, norm(cb.thr[k][bk] - z[k]));
            nopt[k] += 1;
        }
    }
    let mut cand: [(u64, u8); 81] = [(0, 0); 81];
    let mut n = 0usize;
    for i0 in 0..nopt[0] {
        for i1 in 0..nopt[1] {
            for i2 in 0..nopt[2] {
                for i3 in 0..nopt[3] {
                    let pick = [opt[0][i0], opt[1][i1], opt[2][i2], opt[3][i3]];
                    if pick.iter().all(|p| p.0 == 0) {
                        continue;
                    }
                    let mut bb = b;
                    let mut cost = 0u64;
                    for k in 0..AXES {
                        bb[k] = (bb[k] as i8 + pick[k].0) as u8;
                        cost += pick[k].1;
                    }
                    cand[n] = (cost, fine(&bb));
                    n += 1;
                }
            }
        }
    }
    let c = &mut cand[..n];
    c.sort_unstable();
    out[0] = home;
    let m = m.clamp(1, MAX_PROBES);
    let mut k = 1usize;
    for &(_, cell) in c.iter() {
        if k >= m {
            break;
        }
        out[k] = cell;
        k += 1;
    }
    k
}

impl SiSig {
    /// The signature of a prepared side whose route was built under the X
    /// profile the SI profile is bound to.
    pub fn build(p: &Prepared, route: &XRoute, prof: &SiProfile) -> SiSig {
        SiSig::from_parts(&families(p), route, prof)
    }

    /// The signature from a side's family vectors and route.
    pub fn from_parts(fam: &[Option<Vec<i32>>; FAMILIES], route: &XRoute, prof: &SiProfile) -> SiSig {
        let mut s = SiSig::minhash(route);
        for f in 0..FAMILIES {
            if let Some(x) = &fam[f] {
                s.cells[f] = cell(&prof.book[f], x);
                s.present |= 1 << f;
            }
        }
        s
    }

    /// The MinHash half of a signature: the route's lanes banded two at a
    /// time, where the route measures them.  No codebook involved.
    pub fn minhash(route: &XRoute) -> SiSig {
        let mut s = SiSig::default();
        if route.flags & RF_LOCAL != 0 {
            for j in 0..LOCAL_BANDS {
                s.local[j] = ((route.local_mh[2 * j] as u16) << 8) | route.local_mh[2 * j + 1] as u16;
            }
            s.present |= P_LOCAL;
        }
        if route.flags & RF_BAND != 0 {
            for j in 0..BAND_BANDS {
                s.band[j] = ((route.band_mh[2 * j] as u16) << 8) | route.band_mh[2 * j + 1] as u16;
            }
            s.present |= P_BAND;
        }
        s
    }

    /// The signature of a bare prepared side: its route is derived here
    /// (MinHash over codes and bands, no bucket index), which is all a
    /// re-index from stored wires needs.
    pub fn from_prepared(p: &Prepared, xb: &XBound, prof: &SiProfile) -> SiSig {
        let route = XRoute::build(p, &xb.salts, &xb.xp);
        SiSig::build(p, &route, prof)
    }

    pub fn to_bytes(&self) -> [u8; SIG_BYTES] {
        let mut b = [0u8; SIG_BYTES];
        b[0] = self.present;
        b[1..1 + FAMILIES].copy_from_slice(&self.cells);
        let mut o = 8;
        for v in self.local.iter().chain(self.band.iter()) {
            b[o..o + 2].copy_from_slice(&v.to_le_bytes());
            o += 2;
        }
        b
    }

    pub fn from_bytes(b: &[u8]) -> Option<SiSig> {
        if b.len() < SIG_BYTES {
            return None;
        }
        let mut s = SiSig { present: b[0], ..Default::default() };
        s.cells.copy_from_slice(&b[1..1 + FAMILIES]);
        let mut o = 8;
        for j in 0..LOCAL_BANDS {
            s.local[j] = u16::from_le_bytes([b[o], b[o + 1]]);
            o += 2;
        }
        for j in 0..BAND_BANDS {
            s.band[j] = u16::from_le_bytes([b[o], b[o + 1]]);
            o += 2;
        }
        Some(s)
    }
}

/// MinHash level of `m` equal band keys.
#[inline]
pub fn mh_level(m: u32) -> usize {
    match m {
        0 => 0,
        1 => 1,
        2 | 3 => 2,
        _ => 3,
    }
}

/// One query: its signature, its probe cells and the weights, with a level
/// table per quantised family so scoring a candidate is six lookups and two
/// band comparisons.
#[derive(Clone, Debug)]
pub struct SiQuery {
    pub sig: SiSig,
    /// probe cells per quantised family, the query's own cell first
    pub probes: [[u8; MAX_PROBES]; FAMILIES],
    pub nprobe: [u8; FAMILIES],
    /// level of every fine cell per family (0 none, 1 near, 2 exact)
    pub lut: [[u8; CELLS]; FAMILIES],
    pub wv: [[i32; VLEVELS]; FAMILIES],
    pub wl: [i32; MLEVELS],
    pub wb: [i32; MLEVELS],
    /// `base[mask]`: the score of a candidate whose families present on both
    /// sides are `mask` and none of which matched
    pub base: Vec<i32>,
}

impl SiQuery {
    pub fn new(p: &Prepared, route: &XRoute, prof: &SiProfile) -> SiQuery {
        let fam = families(p);
        SiQuery::from_parts(SiSig::from_parts(&fam, route, prof), &fam, prof)
    }

    /// A query from a signature and the family vectors it was built from.
    pub fn from_parts(sig: SiSig, fam: &[Option<Vec<i32>>; FAMILIES], prof: &SiProfile) -> SiQuery {
        let mut q = SiQuery {
            sig,
            probes: [[0; MAX_PROBES]; FAMILIES],
            nprobe: [0; FAMILIES],
            lut: [[0; CELLS]; FAMILIES],
            wv: [[0; VLEVELS]; FAMILIES],
            wl: prof.w_local,
            wb: prof.w_band,
            base: vec![0; 256],
        };
        for f in 0..FAMILIES {
            q.wv[f] = prof.book[f].w;
            if sig.present >> f & 1 == 0 {
                continue;
            }
            if let Some(x) = &fam[f] {
                let n = probe_order(&prof.book[f], x, prof.probes as usize, &mut q.probes[f]);
                q.nprobe[f] = n as u8;
                for r in (0..n).rev() {
                    q.lut[f][q.probes[f][r] as usize] = if r == 0 { 2 } else { 1 };
                }
            }
        }
        for mask in 0..256usize {
            let both = mask as u8 & q.sig.present;
            let mut s = 0i32;
            for f in 0..FAMILIES {
                if both >> f & 1 != 0 {
                    s += q.wv[f][0];
                }
            }
            if both & P_LOCAL != 0 {
                s += q.wl[0];
            }
            if both & P_BAND != 0 {
                s += q.wb[0];
            }
            q.base[mask] = s;
        }
        q
    }

    pub fn from_prepared(p: &Prepared, xb: &XBound, prof: &SiProfile) -> SiQuery {
        let route = XRoute::build(p, &xb.salts, &xb.xp);
        SiQuery::new(p, &route, prof)
    }

    /// Levels a candidate reaches per family (`None`: absent on either side).
    pub fn levels(&self, c: &SiSig) -> [Option<usize>; ALL_FAMILIES] {
        let both = self.sig.present & c.present;
        let mut l = [None; ALL_FAMILIES];
        for f in 0..FAMILIES {
            if both >> f & 1 != 0 {
                l[f] = Some(self.lut[f][c.cells[f] as usize] as usize);
            }
        }
        if both & P_LOCAL != 0 {
            let m = self.sig.local.iter().zip(c.local.iter()).filter(|(a, b)| a == b).count() as u32;
            l[FAMILY_LOCAL] = Some(mh_level(m));
        }
        if both & P_BAND != 0 {
            let m = self.sig.band.iter().zip(c.band.iter()).filter(|(a, b)| a == b).count() as u32;
            l[FAMILY_BAND] = Some(mh_level(m));
        }
        l
    }

    /// True when some family of the candidate reaches level 1 or above: the
    /// candidates an inverted index can see at all.
    pub fn touches(&self, c: &SiSig) -> bool {
        self.levels(c).iter().any(|l| matches!(l, Some(v) if *v > 0))
    }

    /// THE score (SPEC-SI §5.4): the sum, over the families present on both
    /// sides, of the weight of the level the candidate reaches.
    pub fn score(&self, c: &SiSig) -> i32 {
        let l = self.levels(c);
        let mut s = 0i32;
        for f in 0..FAMILIES {
            if let Some(v) = l[f] {
                s += self.wv[f][v];
            }
        }
        if let Some(v) = l[FAMILY_LOCAL] {
            s += self.wl[v];
        }
        if let Some(v) = l[FAMILY_BAND] {
            s += self.wb[v];
        }
        s
    }
}

/// The reference selection every index must reproduce: the touched
/// candidates scoring at least `threshold`, best first (score descending,
/// then slot ascending), at most `budget` of them.  Returns how many were
/// admitted before the budget cut.
pub fn scan(q: &SiQuery, cands: &[SiSig], threshold: i32, budget: usize, out: &mut Vec<(u32, i32)>) -> usize {
    out.clear();
    for (i, c) in cands.iter().enumerate() {
        if q.touches(c) {
            let s = q.score(c);
            if s >= threshold {
                out.push((i as u32, s));
            }
        }
    }
    let admitted = out.len();
    out.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out.truncate(budget);
    admitted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coarse_is_the_median_bit_parent() {
        let mut members = [0usize; COARSE];
        for f in 0..CELLS {
            members[coarse(f as u8) as usize] += 1;
        }
        assert!(members.iter().all(|&n| n == CELLS / COARSE), "{members:?}");
        assert_eq!(coarse(0), 0);
        assert_eq!(coarse(255), 15);
        assert_eq!(fine(&[3, 3, 3, 3]), 255);
        assert_eq!(fine(&[1, 0, 0, 0]), 1);
        assert_eq!(fine(&[0, 1, 0, 0]), 4);
    }

    /// The profiles on real wires: the signature taken from an X side equals
    /// the one derived from the wires alone, and copies under the square's
    /// symmetries and an integer upscale clear the default threshold against
    /// their original — under SI2, fitted on this generator's kind of art,
    /// and the shipped SI3, fitted on the chain's.  Unrelated works of this
    /// generator mostly stay below SI2's threshold; SI3's population is real
    /// art, on which it is measured instead (`sibench chainfit`).
    #[test]
    fn copies_score_and_unrelated_works_do_not() {
        use crate::config::Config;
        use crate::keypoints::{pattern, RotCache};
        use crate::synth::{mirror, nearest_up, pixel_art, rot90, Img};
        use crate::wire::hash;
        use crate::x::{XBound, XPrepared};
        let xb = XBound::shipped();
        let (cfg, rot) = (Config::default(), RotCache::new(&pattern()));
        let side = |im: &Img| -> XPrepared {
            let f = hash(&im.px, im.w, im.h, &cfg, &rot);
            XPrepared::new(Prepared::new(&f.t1, Some(&f.t2)).unwrap(), &xb)
        };
        let bases: Vec<Img> = (0..10).map(|i| pixel_art(96 + 16 * (i % 4), 80 + 8 * (i % 3), 4242 + 31 * i as u64, 4 + i % 9, (i % 3) as u8)).collect();
        let xs: Vec<XPrepared> = bases.iter().map(&side).collect();
        let copies: Vec<Vec<XPrepared>> = bases.iter().map(|b| [mirror(b), rot90(b), nearest_up(b, 2)].iter().map(&side).collect()).collect();
        for (prof, unrelated_bound) in [(SiProfile::si2(), true), (SiProfile::shipped(), false)] {
            let (mut found, mut tried) = (0, 0);
            for i in 0..bases.len() {
                let q = SiQuery::new(&xs[i].p, &xs[i].route, &prof);
                assert_eq!(SiSig::from_prepared(&xs[i].p, &xb, &prof), q.sig, "wire-only signature = X-side signature");
                for cx in copies[i].iter() {
                    let s = SiSig::build(&cx.p, &cx.route, &prof);
                    tried += 1;
                    if q.touches(&s) && q.score(&s) >= prof.threshold {
                        found += 1;
                    }
                }
            }
            assert!(found * 10 >= tried * 9, "{}: copies admitted: {found} of {tried}", prof.name_str());
            let mut admitted = 0;
            let mut pairs = 0;
            for i in 0..xs.len() {
                let q = SiQuery::new(&xs[i].p, &xs[i].route, &prof);
                for j in 0..xs.len() {
                    if i != j {
                        let s = SiSig::build(&xs[j].p, &xs[j].route, &prof);
                        pairs += 1;
                        if q.touches(&s) && q.score(&s) >= prof.threshold {
                            admitted += 1;
                        }
                    }
                }
            }
            if unrelated_bound {
                assert!(admitted * 10 <= pairs, "{}: unrelated admitted: {admitted} of {pairs}", prof.name_str());
            }
            println!("{}: copies {found}/{tried}, unrelated {admitted}/{pairs}", prof.name_str());
        }
    }

    #[test]
    fn signature_bytes_roundtrip() {
        let mut s = SiSig::default();
        s.present = 0b1011_0111;
        s.cells = [1, 2, 3, 250, 0, 9];
        for j in 0..LOCAL_BANDS {
            s.local[j] = (j * 977) as u16;
        }
        for j in 0..BAND_BANDS {
            s.band[j] = (65535 - j * 31) as u16;
        }
        assert_eq!(SiSig::from_bytes(&s.to_bytes()), Some(s));
        assert_eq!(SIG_BYTES, 104);
    }
}
