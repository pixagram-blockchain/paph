//! Fitting a PAPH-SI profile (SPEC-SI §6).
//!
//! Offline, so floating point is allowed here: what this writes is integers,
//! and nodes agree on the artefact's identity, not on how it was fitted.
//!
//! Per quantised family:
//!
//! 1. standardise the family over the background population (unrelated works);
//! 2. take its four principal axes (cyclic Jacobi on the correlation matrix,
//!    each axis signed so its largest component is positive);
//! 3. fold the standardisation into integer projection weights;
//! 4. cut each axis at the quartiles of the background's projections, so each
//!    of the 4 × 4 bins holds a quarter of the population along its axis;
//! 5. measure each axis' transform noise on copy pairs — the scale the probe
//!    order is reckoned in;
//! 6. weigh each evidence level by its log-likelihood ratio between copy pairs
//!    and random pairs (16 × ln, rounded).
//!
//! The admission threshold is the score a chosen fraction of random pairs
//! reaches.

use super::code::{mh_level, probe_order, project, scan, SiQuery, SiSig, P_BAND, P_LOCAL};
use super::features::{DIMS, FAMILIES, FEATURES_VERSION};
use super::profile::*;
use crate::synth::Rng;

/// What a fit reads.
pub struct FitInput<'a> {
    /// per work: its family vectors
    pub fam: &'a [[Option<Vec<i32>>; FAMILIES]],
    /// per work: its MinHash band keys and presence bits (cells unused)
    pub mh: &'a [SiSig],
    /// unrelated works — the population cells are balanced on and random
    /// pairs are drawn from
    pub background: &'a [usize],
    /// copy pairs (query, target), both orders, for the axis noise
    pub noise_pairs: &'a [(usize, usize)],
    /// copy pairs the verifier can confirm, for the evidence weights
    pub evidence_pairs: &'a [(usize, usize)],
    /// (query, background work) pairs that are not random ones — a copy and
    /// its own original, when the background holds the originals (the
    /// chain's fit) — skipped where a random draw lands on them; `None`
    /// when the background is unrelated to every query (the synthetic fit)
    pub related: Option<&'a dyn Fn(usize, usize) -> bool>,
}

pub struct FitOptions {
    pub name: [u8; 16],
    pub xid: [u8; 32],
    pub probes: u8,
    /// random targets drawn per distinct query of the evidence pairs
    pub random_per_query: usize,
    pub seed: u64,
    /// the fraction of random pairs (parts per million) the default
    /// threshold admits
    pub admit_ppm: u32,
    pub budget: i32,
}

/// Cyclic Jacobi on a symmetric `n × n` matrix (row-major, destroyed):
/// eigenvalues and the eigenvectors as columns of `v` (`v[r * n + k]`).
pub fn jacobi(a: &mut [f64], n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut v = vec![0f64; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    let diag2 = |a: &[f64]| (0..n).map(|i| a[i * n + i] * a[i * n + i]).sum::<f64>();
    for _sweep in 0..200 {
        let mut off = 0f64;
        for p in 0..n {
            for q in p + 1..n {
                off += a[p * n + q] * a[p * n + q];
            }
        }
        if off <= 1e-24 * diag2(a).max(1e-300) {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = a[p * n + q];
                if apq.abs() < 1e-300 {
                    continue;
                }
                let theta = (a[q * n + q] - a[p * n + p]) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                let tau = s / (1.0 + c);
                a[p * n + p] -= t * apq;
                a[q * n + q] += t * apq;
                a[p * n + q] = 0.0;
                a[q * n + p] = 0.0;
                for r in 0..n {
                    if r == p || r == q {
                        continue;
                    }
                    let arp = a[r * n + p];
                    let arq = a[r * n + q];
                    let np = arp - s * (arq + tau * arp);
                    let nq = arq + s * (arp - tau * arq);
                    a[r * n + p] = np;
                    a[p * n + r] = np;
                    a[r * n + q] = nq;
                    a[q * n + r] = nq;
                }
                for r in 0..n {
                    let vrp = v[r * n + p];
                    let vrq = v[r * n + q];
                    v[r * n + p] = vrp - s * (vrq + tau * vrp);
                    v[r * n + q] = vrq + s * (vrp - tau * vrq);
                }
            }
        }
    }
    ((0..n).map(|i| a[i * n + i]).collect(), v)
}

fn isqrt_u128(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    let mut x = 1u128 << ((128 - n.leading_zeros()).div_ceil(2));
    loop {
        let y = (x + n / x) >> 1;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// One family's codebook without weights.
fn fit_codebook(f: usize, inp: &FitInput) -> Codebook {
    let d = DIMS[f];
    let bg: Vec<&Vec<i32>> = inp.background.iter().filter_map(|&i| inp.fam[i][f].as_ref()).collect();
    let n = bg.len().max(1) as f64;
    let mut mu = vec![0f64; d];
    for x in bg.iter() {
        for i in 0..d {
            mu[i] += x[i] as f64;
        }
    }
    for m in mu.iter_mut() {
        *m /= n;
    }
    let mut sd = vec![0f64; d];
    for x in bg.iter() {
        for i in 0..d {
            let e = x[i] as f64 - mu[i];
            sd[i] += e * e;
        }
    }
    for s in sd.iter_mut() {
        *s = (*s / n).sqrt();
    }
    let mut c = vec![0f64; d * d];
    let mut z = vec![0f64; d];
    for x in bg.iter() {
        for i in 0..d {
            z[i] = if sd[i] > 1e-9 { (x[i] as f64 - mu[i]) / sd[i] } else { 0.0 };
        }
        for i in 0..d {
            for j in i..d {
                c[i * d + j] += z[i] * z[j];
            }
        }
    }
    for i in 0..d {
        for j in i..d {
            c[i * d + j] /= n;
            c[j * d + i] = c[i * d + j];
        }
    }
    let (ev, vecs) = jacobi(&mut c, d);
    let mut order: Vec<usize> = (0..d).collect();
    order.sort_by(|&a, &b| ev[b].partial_cmp(&ev[a]).unwrap().then(a.cmp(&b)));
    let mean: Vec<i32> = mu.iter().map(|&m| m.round() as i32).collect();
    let mut proj = vec![0i32; AXES * d];
    for k in 0..AXES {
        let col = order[k];
        let comp: Vec<f64> = (0..d).map(|i| vecs[i * d + col]).collect();
        let mut big = 0usize;
        for i in 1..d {
            if comp[i].abs() > comp[big].abs() + 1e-12 {
                big = i;
            }
        }
        let sign = if comp[big] < 0.0 { -1.0 } else { 1.0 };
        for i in 0..d {
            let w = if sd[i] > 1e-9 { sign * comp[i] / sd[i] * 16384.0 } else { 0.0 };
            proj[k * d + i] = w.round().clamp(-(1 << 24) as f64, (1 << 24) as f64) as i32;
        }
    }
    let mut cb = Codebook { mean, proj, thr: [[0; BINS - 1]; AXES], sig: [1; AXES], w: [0; VLEVELS] };
    // quartile edges of the background's integer projections
    let zs: Vec<[i64; AXES]> = bg.iter().map(|x| project(&cb, x)).collect();
    for k in 0..AXES {
        let mut a: Vec<i64> = zs.iter().map(|z| z[k]).collect();
        a.sort_unstable();
        let m = a.len();
        if m > 0 {
            cb.thr[k] = [a[m / 4], a[m / 2], a[(3 * m) / 4]];
        }
    }
    // axis noise: RMS displacement of a copy, per axis, over √2
    let mut acc = [0u128; AXES];
    let mut np = 0u128;
    for &(a, b) in inp.noise_pairs.iter() {
        if let (Some(x), Some(y)) = (&inp.fam[a][f], &inp.fam[b][f]) {
            let (za, zb) = (project(&cb, x), project(&cb, y));
            for k in 0..AXES {
                let dd = (za[k] - zb[k]).unsigned_abs() as u128;
                acc[k] += dd * dd;
            }
            np += 1;
        }
    }
    for k in 0..AXES {
        cb.sig[k] = if np > 0 { (isqrt_u128(acc[k] / (2 * np)) as i64).max(1) } else { 1 };
    }
    cb
}

fn llr(copy: &[u64], rand: &[u64]) -> Vec<i32> {
    let (tc, tr) = (copy.iter().sum::<u64>() as f64, rand.iter().sum::<u64>() as f64);
    let k = copy.len() as f64;
    copy.iter()
        .zip(rand.iter())
        .map(|(&c, &r)| {
            let pc = (c as f64 + 0.5) / (tc + 0.5 * k);
            let pr = (r as f64 + 0.5) / (tr + 0.5 * k);
            (16.0 * (pc / pr).ln()).round() as i32
        })
        .collect()
}

/// Fit a profile.
pub fn fit(inp: &FitInput, o: &FitOptions) -> SiProfile {
    fit_on_books(fit_books(inp), inp, o)
}

/// Steps 1–5: one codebook per quantised family, its weights still zero, and
/// how many background works carried the family (what each was fitted on).
pub fn fit_books(inp: &FitInput) -> Vec<Codebook> {
    (0..FAMILIES).map(|f| fit_codebook(f, inp)).collect()
}

/// How many background works carry family `f` — a codebook fitted on a
/// handful of works is a guess, and a caller may keep another one instead.
pub fn background_count(inp: &FitInput, f: usize) -> usize {
    inp.background.iter().filter(|&&i| inp.fam[i][f].is_some()).count()
}

/// Step 6 and the default threshold, on the given codebooks (the evidence
/// weights are re-estimated for every family, whichever fit its codebook
/// came from).
pub fn fit_on_books(book: Vec<Codebook>, inp: &FitInput, o: &FitOptions) -> SiProfile {
    let mut prof = SiProfile {
        version: SI_PROFILE_VERSION,
        features: FEATURES_VERSION,
        name: o.name,
        xid: o.xid,
        probes: o.probes,
        book,
        w_local: [0; MLEVELS],
        w_band: [0; MLEVELS],
        threshold: 0,
        budget: o.budget,
    };
    // every work's signature under the new codebooks
    let sigs: Vec<SiSig> = (0..inp.fam.len())
        .map(|i| {
            let mut s = inp.mh[i];
            s.present &= P_LOCAL | P_BAND;
            s.cells = [0; FAMILIES];
            for f in 0..FAMILIES {
                if let Some(x) = &inp.fam[i][f] {
                    s.cells[f] = super::code::cell(&prof.book[f], x);
                    s.present |= 1 << f;
                }
            }
            s
        })
        .collect();
    // probe level tables per query
    let lut_of = |q: usize, f: usize| -> Option<[u8; CELLS]> {
        let x = inp.fam[q][f].as_ref()?;
        let mut pr = [0u8; MAX_PROBES];
        let n = probe_order(&prof.book[f], x, prof.probes as usize, &mut pr);
        let mut lut = [0u8; CELLS];
        for r in (0..n).rev() {
            lut[pr[r] as usize] = if r == 0 { 2 } else { 1 };
        }
        Some(lut)
    };
    let mut queries: Vec<usize> = inp.evidence_pairs.iter().map(|p| p.0).collect();
    queries.sort_unstable();
    queries.dedup();
    let mut rng = Rng(o.seed | 1);
    let mut rand_pairs: Vec<(usize, usize)> = Vec::new();
    for &q in queries.iter() {
        for _ in 0..o.random_per_query {
            let t = inp.background[rng.below(inp.background.len() as u64) as usize];
            if t != q && !inp.related.is_some_and(|r| r(q, t)) {
                rand_pairs.push((q, t));
            }
        }
    }
    let mut cv = vec![[0u64; VLEVELS]; FAMILIES];
    let mut rv = vec![[0u64; VLEVELS]; FAMILIES];
    let mut cm = [[0u64; MLEVELS]; 2];
    let mut rm = [[0u64; MLEVELS]; 2];
    let tally = |pairs: &[(usize, usize)], v: &mut Vec<[u64; VLEVELS]>, m: &mut [[u64; MLEVELS]; 2]| {
        let mut last = usize::MAX;
        let mut luts: Vec<Option<[u8; CELLS]>> = vec![None; FAMILIES];
        for &(q, t) in pairs.iter() {
            if q != last {
                for f in 0..FAMILIES {
                    luts[f] = lut_of(q, f);
                }
                last = q;
            }
            let (sq, st) = (&sigs[q], &sigs[t]);
            for f in 0..FAMILIES {
                if let Some(l) = &luts[f] {
                    if st.present >> f & 1 != 0 {
                        v[f][l[st.cells[f] as usize] as usize] += 1;
                    }
                }
            }
            if sq.present & st.present & P_LOCAL != 0 {
                let e = sq.local.iter().zip(st.local.iter()).filter(|(a, b)| a == b).count() as u32;
                m[0][mh_level(e)] += 1;
            }
            if sq.present & st.present & P_BAND != 0 {
                let e = sq.band.iter().zip(st.band.iter()).filter(|(a, b)| a == b).count() as u32;
                m[1][mh_level(e)] += 1;
            }
        }
    };
    let mut ep = inp.evidence_pairs.to_vec();
    ep.sort_unstable();
    tally(&ep, &mut cv, &mut cm);
    tally(&rand_pairs, &mut rv, &mut rm);
    for f in 0..FAMILIES {
        let w = llr(&cv[f], &rv[f]);
        prof.book[f].w = [w[0], w[1], w[2]];
    }
    let wl = llr(&cm[0], &rm[0]);
    let wb = llr(&cm[1], &rm[1]);
    prof.w_local = [wl[0], wl[1], wl[2], wl[3]];
    prof.w_band = [wb[0], wb[1], wb[2], wb[3]];
    // the default threshold: the score the chosen share of random pairs reaches
    let mut scores: Vec<i32> = Vec::with_capacity(rand_pairs.len());
    let mut last = usize::MAX;
    let mut qy: Option<SiQuery> = None;
    for &(q, t) in rand_pairs.iter() {
        if q != last {
            qy = Some(query_of(&prof, inp.fam, &sigs, q));
            last = q;
        }
        let qq = qy.as_ref().unwrap();
        scores.push(if qq.touches(&sigs[t]) { qq.score(&sigs[t]) } else { i32::MIN });
    }
    scores.sort_unstable();
    let k = ((scores.len() as u64 * (1_000_000 - o.admit_ppm.min(1_000_000) as u64)) / 1_000_000) as usize;
    prof.threshold = scores.get(k.min(scores.len().saturating_sub(1))).copied().unwrap_or(0).max(1);
    let _ = scan; // the selection the threshold feeds (code::scan)
    prof
}

/// The query of work `q` of a fit (the same construction as `SiQuery::new`).
pub fn query_of(prof: &SiProfile, fam: &[[Option<Vec<i32>>; FAMILIES]], sigs: &[SiSig], q: usize) -> SiQuery {
    SiQuery::from_parts(sigs[q], &fam[q], prof)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jacobi_diagonalises() {
        let n = 7;
        let mut r = Rng(99);
        let mut m = vec![0f64; n * n];
        for i in 0..n {
            for j in i..n {
                let v = (r.below(2000) as f64 - 1000.0) / 100.0;
                m[i * n + j] = v;
                m[j * n + i] = v;
            }
        }
        let orig = m.clone();
        let (ev, v) = jacobi(&mut m, n);
        for k in 0..n {
            for i in 0..n {
                let av: f64 = (0..n).map(|j| orig[i * n + j] * v[j * n + k]).sum();
                assert!((av - ev[k] * v[i * n + k]).abs() < 1e-9, "A v = λ v");
            }
        }
    }
}
