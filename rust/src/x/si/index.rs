//! The in-memory PAPH-SI index (SPEC-SI §7.1): posting lists per
//! (family, cell) and per (MinHash family, band, key), and a query that
//! accumulates the score of every candidate its probes touch — the
//! T-occurrence "ScanCount" method — then keeps those at or above the
//! threshold, best first, within the budget.
//!
//! The result is defined by `code::scan` and reproduced exactly (a test
//! holds the two equal): the index is an accelerator, never a different
//! answer.  Removal tombstones a slot; the lists are compacted when more than
//! a quarter of the entries they hold belong to removed slots.  Every
//! mutation bumps `generation`, the key a query cache must include
//! (SPEC-SI §7.4).

use super::code::{mh_level, SiQuery, SiSig, P_BAND, P_LOCAL};
use super::features::FAMILIES;
use super::profile::{BAND_BANDS, CELLS, LOCAL_BANDS};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SiStats {
    /// candidates some probe or band key reached
    pub touched: usize,
    /// of those, at or above the threshold
    pub admitted: usize,
    /// posting entries read
    pub postings: usize,
}

pub struct SiIndex {
    sigs: Vec<SiSig>,
    live: Vec<bool>,
    /// removed slots, ever
    dead: usize,
    cells: Vec<Vec<u32>>,
    bands: HashMap<u32, Vec<u32>>,
    /// posting entries held, live and tombstoned
    entries: usize,
    /// of those, entries of removed slots not yet compacted away
    stale: usize,
    compactions: u64,
    generation: u64,
    sc: Scratch,
}

/// Posting entries a signature holds.
fn entries_of(s: &SiSig) -> usize {
    let mut n = (0..FAMILIES).filter(|&f| s.present >> f & 1 != 0).count();
    if s.present & P_LOCAL != 0 {
        n += LOCAL_BANDS;
    }
    if s.present & P_BAND != 0 {
        n += BAND_BANDS;
    }
    n
}

/// Query scratch, sized to the slot count and reused across queries.
#[derive(Default)]
struct Scratch {
    acc: Vec<i32>,
    cnt_l: Vec<u8>,
    cnt_b: Vec<u8>,
    stamp: Vec<u32>,
    epoch: u32,
    touched: Vec<u32>,
}

impl Scratch {
    #[inline]
    fn touch(&mut self, slot: u32) -> usize {
        let s = slot as usize;
        if self.stamp[s] != self.epoch {
            self.stamp[s] = self.epoch;
            self.acc[s] = 0;
            self.cnt_l[s] = 0;
            self.cnt_b[s] = 0;
            self.touched.push(slot);
        }
        s
    }
}

#[inline]
fn band_key(fam: u32, j: usize, v: u16) -> u32 {
    (fam << 21) | ((j as u32) << 16) | v as u32
}

impl Default for SiIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl SiIndex {
    pub fn new() -> SiIndex {
        SiIndex {
            sigs: Vec::new(),
            live: Vec::new(),
            dead: 0,
            cells: vec![Vec::new(); FAMILIES * CELLS],
            bands: HashMap::new(),
            entries: 0,
            stale: 0,
            compactions: 0,
            generation: 0,
            sc: Scratch::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.sigs.len() - self.dead
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn slots(&self) -> usize {
        self.sigs.len()
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Posting entries held (live and tombstoned).
    pub fn entries(&self) -> usize {
        self.entries
    }

    /// Entries of removed slots still in the lists.
    pub fn stale(&self) -> usize {
        self.stale
    }

    /// How many times the lists were compacted.
    pub fn compactions(&self) -> u64 {
        self.compactions
    }

    pub fn sig(&self, slot: u32) -> Option<&SiSig> {
        let s = slot as usize;
        if s < self.sigs.len() && self.live[s] {
            Some(&self.sigs[s])
        } else {
            None
        }
    }

    fn post(&mut self, slot: u32, s: &SiSig) {
        for f in 0..FAMILIES {
            if s.present >> f & 1 != 0 {
                self.cells[f * CELLS + s.cells[f] as usize].push(slot);
                self.entries += 1;
            }
        }
        if s.present & P_LOCAL != 0 {
            for j in 0..LOCAL_BANDS {
                self.bands.entry(band_key(0, j, s.local[j])).or_default().push(slot);
            }
            self.entries += LOCAL_BANDS;
        }
        if s.present & P_BAND != 0 {
            for j in 0..BAND_BANDS {
                self.bands.entry(band_key(1, j, s.band[j])).or_default().push(slot);
            }
            self.entries += BAND_BANDS;
        }
    }

    /// Add a signature; returns its slot (slots are dense and never reused).
    pub fn add(&mut self, s: SiSig) -> u32 {
        let slot = self.sigs.len() as u32;
        self.post(slot, &s);
        self.sigs.push(s);
        self.live.push(true);
        self.sc.acc.push(0);
        self.sc.cnt_l.push(0);
        self.sc.cnt_b.push(0);
        self.sc.stamp.push(0);
        self.generation += 1;
        slot
    }

    /// Remove a slot; false when it was not live.
    pub fn remove(&mut self, slot: u32) -> bool {
        let s = slot as usize;
        if s >= self.sigs.len() || !self.live[s] {
            return false;
        }
        self.live[s] = false;
        self.dead += 1;
        self.stale += entries_of(&self.sigs[s]);
        self.generation += 1;
        if self.stale * 4 > self.entries {
            self.compact();
        }
        true
    }

    /// Drop tombstoned entries from every list (slots keep their numbers).
    pub fn compact(&mut self) {
        let live = &self.live;
        let mut entries = 0usize;
        for l in self.cells.iter_mut() {
            l.retain(|&x| live[x as usize]);
            entries += l.len();
        }
        self.bands.retain(|_, l| {
            l.retain(|&x| live[x as usize]);
            entries += l.len();
            !l.is_empty()
        });
        self.entries = entries;
        self.stale = 0;
        self.compactions += 1;
    }

    /// The candidates of one query (see `code::scan` for the definition):
    /// `(slot, score)`, best first, at most `budget`.
    pub fn query(&mut self, q: &SiQuery, threshold: i32, budget: usize, out: &mut Vec<(u32, i32)>) -> SiStats {
        out.clear();
        let (cells, bands, live, sigs, sc) = (&self.cells, &self.bands, &self.live, &self.sigs, &mut self.sc);
        sc.epoch = sc.epoch.wrapping_add(1);
        if sc.epoch == 0 {
            for v in sc.stamp.iter_mut() {
                *v = 0;
            }
            sc.epoch = 1;
        }
        sc.touched.clear();
        let mut postings = 0usize;
        // quantised families: each slot sits in exactly one cell per family,
        // so at most one probe of a family can reach it
        for f in 0..FAMILIES {
            if q.sig.present >> f & 1 == 0 {
                continue;
            }
            let w0 = q.wv[f][0];
            for r in 0..q.nprobe[f] as usize {
                let list = &cells[f * CELLS + q.probes[f][r] as usize];
                let excess = q.wv[f][if r == 0 { 2 } else { 1 }] - w0;
                postings += list.len();
                for &slot in list.iter() {
                    if live[slot as usize] {
                        let s = sc.touch(slot);
                        sc.acc[s] += excess;
                    }
                }
            }
        }
        // MinHash families: count equal band keys per slot
        if q.sig.present & P_LOCAL != 0 {
            for j in 0..LOCAL_BANDS {
                if let Some(list) = bands.get(&band_key(0, j, q.sig.local[j])) {
                    postings += list.len();
                    for &slot in list.iter() {
                        if live[slot as usize] {
                            let s = sc.touch(slot);
                            sc.cnt_l[s] = sc.cnt_l[s].saturating_add(1);
                        }
                    }
                }
            }
        }
        if q.sig.present & P_BAND != 0 {
            for j in 0..BAND_BANDS {
                if let Some(list) = bands.get(&band_key(1, j, q.sig.band[j])) {
                    postings += list.len();
                    for &slot in list.iter() {
                        if live[slot as usize] {
                            let s = sc.touch(slot);
                            sc.cnt_b[s] = sc.cnt_b[s].saturating_add(1);
                        }
                    }
                }
            }
        }
        // finish every touched slot: the base term for the families both
        // sides hold, plus the MinHash levels, then admit by threshold
        for &slot in sc.touched.iter() {
            let s = slot as usize;
            let present = sigs[s].present;
            let mut score = sc.acc[s] + q.base[present as usize];
            let both = q.sig.present & present;
            if both & P_LOCAL != 0 {
                score += q.wl[mh_level(sc.cnt_l[s] as u32)] - q.wl[0];
            }
            if both & P_BAND != 0 {
                score += q.wb[mh_level(sc.cnt_b[s] as u32)] - q.wb[0];
            }
            if score >= threshold {
                out.push((slot, score));
            }
        }
        let admitted = out.len();
        let order = |a: &(u32, i32), b: &(u32, i32)| b.1.cmp(&a.1).then(a.0.cmp(&b.0));
        if budget == 0 {
            out.clear();
        } else if out.len() > budget {
            out.select_nth_unstable_by(budget - 1, order);
            out.truncate(budget);
        }
        out.sort_unstable_by(order);
        SiStats { touched: sc.touched.len(), admitted, postings }
    }
}

#[cfg(test)]
mod tests {
    use super::super::code::scan;
    use super::super::profile::*;
    use super::*;
    use crate::synth::Rng;

    fn rand_sig(r: &mut Rng) -> SiSig {
        let mut s = SiSig::default();
        s.present = (r.next() & 0xff) as u8 | 0b0000_1111;
        for f in 0..FAMILIES {
            // skewed cells, many collisions
            s.cells[f] = if s.present >> f & 1 != 0 { (r.below(24) * r.below(9)) as u8 } else { 0 };
        }
        for j in 0..LOCAL_BANDS {
            s.local[j] = r.below(40) as u16;
        }
        for j in 0..BAND_BANDS {
            s.band[j] = r.below(30) as u16;
        }
        s
    }

    fn rand_query(r: &mut Rng) -> SiQuery {
        let sig = rand_sig(r);
        let mut q = SiQuery {
            sig,
            probes: [[0; MAX_PROBES]; FAMILIES],
            nprobe: [0; FAMILIES],
            lut: [[0; CELLS]; FAMILIES],
            wv: [[0; VLEVELS]; FAMILIES],
            wl: [-20, 30, 70, 120],
            wb: [-10, 50, 90, 130],
            base: vec![0; 256],
        };
        for f in 0..FAMILIES {
            q.wv[f] = [-(r.below(40) as i32), r.below(30) as i32, 40 + r.below(40) as i32];
            if sig.present >> f & 1 != 0 {
                let n = 1 + r.below(4) as usize;
                let mut used = std::collections::HashSet::new();
                let mut k = 0;
                while k < n {
                    let c = if k == 0 { sig.cells[f] } else { (r.below(24) * r.below(9)) as u8 };
                    if used.insert(c) {
                        q.probes[f][k] = c;
                        k += 1;
                    }
                }
                q.nprobe[f] = n as u8;
                for k in (0..n).rev() {
                    q.lut[f][q.probes[f][k] as usize] = if k == 0 { 2 } else { 1 };
                }
            }
        }
        for mask in 0..256usize {
            let both = mask as u8 & q.sig.present;
            let mut s = 0;
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

    /// The index is the scan, slot for slot, score for score, order included —
    /// through adds, removals and compactions, at five thresholds and budgets
    /// (an empty budget and no budget among them).
    #[test]
    fn index_equals_scan() {
        let mut r = Rng(0x5349_5f69_6478_0001);
        let mut idx = SiIndex::new();
        let mut all: Vec<SiSig> = Vec::new();
        let mut alive: Vec<bool> = Vec::new();
        for round in 0..6 {
            for _ in 0..400 {
                let s = rand_sig(&mut r);
                let slot = idx.add(s);
                assert_eq!(slot as usize, all.len());
                all.push(s);
                alive.push(true);
            }
            // rounds 0–2 remove a little, rounds 3–5 a lot: compactions happen
            for _ in 0..(if round < 3 { 90 } else { 700 }) {
                let k = r.below(all.len() as u64) as usize;
                assert_eq!(idx.remove(k as u32), alive[k]);
                alive[k] = false;
            }
            assert!(idx.stale() * 4 <= idx.entries(), "never more than a quarter stale after a removal");
            for _ in 0..25 {
                let q = rand_query(&mut r);
                for (th, budget) in [(i32::MIN + 1, usize::MAX), (0, 50), (60, 7), (120, 1000), (-40, 0)] {
                    let mut a = Vec::new();
                    let st = idx.query(&q, th, budget, &mut a);
                    // scan over live signatures with their real slot numbers
                    let mut b = Vec::new();
                    for (i, s) in all.iter().enumerate() {
                        if alive[i] && q.touches(s) {
                            let sc = q.score(s);
                            if sc >= th {
                                b.push((i as u32, sc));
                            }
                        }
                    }
                    let admitted = b.len();
                    b.sort_unstable_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
                    b.truncate(budget);
                    assert_eq!(a, b, "round {round} th {th} budget {budget}");
                    assert_eq!(st.admitted, admitted);
                }
            }
        }
        assert!(idx.compactions() >= 2, "compactions exercised: {}", idx.compactions());
        let live_entries: usize = all.iter().zip(alive.iter()).filter(|(_, &a)| a).map(|(s, _)| entries_of(s)).sum();
        assert_eq!(idx.entries() - idx.stale(), live_entries, "entry accounting");
        // and the free function is the same definition on a dense array
        let q = rand_query(&mut r);
        let live: Vec<SiSig> = all.iter().zip(alive.iter()).filter(|(_, &a)| a).map(|(s, _)| *s).collect();
        let mut out = Vec::new();
        scan(&q, &live, 10, 100, &mut out);
        assert!(out.windows(2).all(|w| w[0].1 >= w[1].1));
    }
}
