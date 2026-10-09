//! The PAPH-SI profile (SPEC-SI §6) — the codebook artefact.
//!
//! Everything that can move a work to another cell or change a candidate's
//! score lives here, as one immutable byte artefact with a SHA-256 identity,
//! the discipline of the comparator-42 `.pcal` and the X1 `.pxcl`: two nodes
//! holding different codebooks must never claim to have built the same index.
//! The identity also covers the X profile whose route lanes the two MinHash
//! families band (`xid`): an SI profile is bound to exactly one X profile.
//!
//! Per quantised family the codebook holds an integer mean, four integer
//! projection axes, three bin edges per axis (the quartiles of the fitting
//! population, so every bin holds a quarter of it), the per-axis transform
//! noise the multi-probe order is measured in, and the family's evidence
//! weights.  Fitting is `fit.rs`; this module only reads, writes and checks.

use super::features::{DIMS, FAMILIES, FEATURES_VERSION};
use crate::sha256::{hex, sha256};

pub const SI_PROFILE_VERSION: u16 = 1;
const MAGIC: &[u8; 4] = b"PXSI";

/// Four projection axes of four bins: 256 fine cells per family, and the
/// median bit of each axis gives the 16 coarse cells (a coarse cell is the
/// union of exactly sixteen fine cells — the hierarchy is exact).
pub const AXES: usize = 4;
pub const BINS: usize = 4;
pub const CELLS: usize = 256;
pub const COARSE: usize = 16;
pub const MAX_PROBES: usize = 8;

/// The two MinHash families band the XRoute lanes two at a time: 64 local
/// lanes make 32 band keys, 32 descriptor-band lanes make 16.
pub const LOCAL_BANDS: usize = 32;
pub const BAND_BANDS: usize = 16;
pub const MH_FAMILIES: usize = 2;
pub const ALL_FAMILIES: usize = FAMILIES + MH_FAMILIES;
pub const FAMILY_LOCAL: usize = FAMILIES;
pub const FAMILY_BAND: usize = FAMILIES + 1;

/// Evidence levels.  A quantised family: 0 none, 1 near (one of the query's
/// other probe cells), 2 exact (the query's own cell).  A MinHash family:
/// 0 no band key equal, 1 one, 2 two or three, 3 four or more.
pub const VLEVELS: usize = 3;
pub const MLEVELS: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Codebook {
    /// integer mean subtracted from the raw vector, `DIMS[f]` entries
    pub mean: Vec<i32>,
    /// projection weights, axis-major: `proj[k * d + i]`
    pub proj: Vec<i32>,
    /// bin edges per axis, non-decreasing; a value equal to an edge lies above it
    pub thr: [[i64; BINS - 1]; AXES],
    /// per-axis transform noise (RMS displacement of a copy along the axis), ≥ 1
    pub sig: [i64; AXES],
    /// evidence weight per level (16 × log-likelihood ratio, rounded)
    pub w: [i32; VLEVELS],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SiProfile {
    pub version: u16,
    /// `FEATURES_VERSION` the codebooks were fitted on
    pub features: u16,
    pub name: [u8; 16],
    /// identity of the X profile whose route lanes are banded
    pub xid: [u8; 32],
    /// probe cells per quantised family, the query's own cell first (1..=8)
    pub probes: u8,
    pub book: Vec<Codebook>,
    pub w_local: [i32; MLEVELS],
    pub w_band: [i32; MLEVELS],
    /// default admission threshold (a candidate needs at least this score)
    pub threshold: i32,
    /// default candidate budget per query
    pub budget: i32,
}

fn w16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn w32(b: &mut Vec<u8>, v: i32) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn w64(b: &mut Vec<u8>, v: i64) {
    b.extend_from_slice(&v.to_le_bytes());
}

struct Rd<'a> {
    b: &'a [u8],
    o: usize,
}

impl<'a> Rd<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], &'static str> {
        if self.o + n > self.b.len() {
            return Err("si profile truncated");
        }
        let s = &self.b[self.o..self.o + n];
        self.o += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, &'static str> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, &'static str> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn i32(&mut self) -> Result<i32, &'static str> {
        let s = self.take(4)?;
        Ok(i32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i64(&mut self) -> Result<i64, &'static str> {
        let s = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        Ok(i64::from_le_bytes(a))
    }
}

impl SiProfile {
    pub fn name_str(&self) -> String {
        let end = self.name.iter().position(|&c| c == 0).unwrap_or(16);
        String::from_utf8_lossy(&self.name[..end]).into_owned()
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(12_000);
        b.extend_from_slice(MAGIC);
        w16(&mut b, self.version);
        w16(&mut b, self.features);
        b.extend_from_slice(&self.name);
        b.extend_from_slice(&self.xid);
        b.push(AXES as u8);
        b.push(BINS as u8);
        b.push(self.probes);
        b.push(self.book.len() as u8);
        for (f, cb) in self.book.iter().enumerate() {
            w16(&mut b, DIMS.get(f).copied().unwrap_or(cb.mean.len()) as u16);
            for &v in cb.mean.iter() {
                w32(&mut b, v);
            }
            for &v in cb.proj.iter() {
                w32(&mut b, v);
            }
            for k in 0..AXES {
                for j in 0..BINS - 1 {
                    w64(&mut b, cb.thr[k][j]);
                }
            }
            for k in 0..AXES {
                w64(&mut b, cb.sig[k]);
            }
            for &v in cb.w.iter() {
                w32(&mut b, v);
            }
        }
        b.push(LOCAL_BANDS as u8);
        b.push(BAND_BANDS as u8);
        for &v in self.w_local.iter().chain(self.w_band.iter()) {
            w32(&mut b, v);
        }
        w32(&mut b, self.threshold);
        w32(&mut b, self.budget);
        b
    }

    pub fn decode(b: &[u8]) -> Result<SiProfile, &'static str> {
        let mut r = Rd { b, o: 0 };
        if r.take(4)? != MAGIC {
            return Err("bad si profile magic");
        }
        let version = r.u16()?;
        let features = r.u16()?;
        if version != SI_PROFILE_VERSION {
            return Err("si profile version");
        }
        let mut name = [0u8; 16];
        name.copy_from_slice(r.take(16)?);
        let mut xid = [0u8; 32];
        xid.copy_from_slice(r.take(32)?);
        if r.u8()? as usize != AXES || r.u8()? as usize != BINS {
            return Err("si cell layout");
        }
        let probes = r.u8()?;
        let nf = r.u8()? as usize;
        if nf != FAMILIES {
            return Err("si family count");
        }
        let mut book = Vec::with_capacity(nf);
        for f in 0..nf {
            let d = r.u16()? as usize;
            if d != DIMS[f] {
                return Err("si family dimension");
            }
            let mut mean = Vec::with_capacity(d);
            for _ in 0..d {
                mean.push(r.i32()?);
            }
            let mut proj = Vec::with_capacity(AXES * d);
            for _ in 0..AXES * d {
                proj.push(r.i32()?);
            }
            let mut thr = [[0i64; BINS - 1]; AXES];
            for k in 0..AXES {
                for j in 0..BINS - 1 {
                    thr[k][j] = r.i64()?;
                }
            }
            let mut sig = [0i64; AXES];
            for v in sig.iter_mut() {
                *v = r.i64()?;
            }
            let mut w = [0i32; VLEVELS];
            for v in w.iter_mut() {
                *v = r.i32()?;
            }
            book.push(Codebook { mean, proj, thr, sig, w });
        }
        if r.u8()? as usize != LOCAL_BANDS || r.u8()? as usize != BAND_BANDS {
            return Err("si band layout");
        }
        let mut w_local = [0i32; MLEVELS];
        let mut w_band = [0i32; MLEVELS];
        for v in w_local.iter_mut().chain(w_band.iter_mut()) {
            *v = r.i32()?;
        }
        let threshold = r.i32()?;
        let budget = r.i32()?;
        if r.o != b.len() {
            return Err("si profile has trailing bytes");
        }
        let p = SiProfile { version, features, name, xid, probes, book, w_local, w_band, threshold, budget };
        p.validate()?;
        Ok(p)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.version != SI_PROFILE_VERSION {
            return Err("si profile version");
        }
        if self.features != FEATURES_VERSION {
            return Err("si profile fitted on another feature derivation");
        }
        if self.probes < 1 || self.probes as usize > MAX_PROBES {
            return Err("si probe count");
        }
        if self.book.len() != FAMILIES {
            return Err("si family count");
        }
        for (f, cb) in self.book.iter().enumerate() {
            if cb.mean.len() != DIMS[f] || cb.proj.len() != AXES * DIMS[f] {
                return Err("si codebook dimension");
            }
            for k in 0..AXES {
                if cb.thr[k][0] > cb.thr[k][1] || cb.thr[k][1] > cb.thr[k][2] {
                    return Err("si bin edges out of order");
                }
                if cb.sig[k] < 1 {
                    return Err("si axis noise");
                }
            }
            // a projected value must stay far inside i64: |x − mean| ≤ 2^10
            // per dimension, at most 87 dimensions, weights below 2^31
            for &m in cb.mean.iter() {
                if !(-(1 << 12)..=(1 << 12)).contains(&m) {
                    return Err("si codebook mean");
                }
            }
        }
        if self.budget < 1 {
            return Err("si budget");
        }
        Ok(())
    }

    pub fn id(&self) -> [u8; 32] {
        sha256(&self.encode())
    }

    pub fn id_hex16(&self) -> String {
        hex(&self.id()[..8])
    }

    /// SI1-PROVISIONAL, fitted by `sibench fit` on the synthetic corpus and
    /// bound to X1-PROVISIONAL — 1.1.0's profile.
    pub fn si1() -> SiProfile {
        SiProfile::decode(SI1).expect("the shipped SI1 profile decodes")
    }

    /// SI2-PROVISIONAL: SI1's fit bound to X2-PROVISIONAL.  Route derivation
    /// 2 changes the route's global words, not the MinHash lanes SI bands,
    /// so the codebooks, weights, threshold and budget are SI1's.
    pub fn si2() -> SiProfile {
        SiProfile::decode(SI2).expect("the shipped SI2 profile decodes")
    }

    /// SI3-PROVISIONAL: fitted by `sibench chainfit` on the Pixa chain's
    /// artworks (`tools/chain-corpus.mjs`) and bound to X2-PROVISIONAL — the
    /// codebooks balanced on real works, the weights measured on their copies
    /// (SIL keeps SI2's codebook: too few real works carry it).
    pub fn si3() -> SiProfile {
        SiProfile::decode(SI3).expect("the shipped SI3 profile decodes")
    }

    /// The shipped default: SI3, bound to the shipped X2.
    pub fn shipped() -> SiProfile {
        Self::si3()
    }
}

/// SI1-PROVISIONAL (`docs/calibration/SI1-PROVISIONAL.psi`), bound to X1.
pub const SI1: &[u8] = include_bytes!("../../../../docs/calibration/SI1-PROVISIONAL.psi");
/// SI2-PROVISIONAL (`docs/calibration/SI2-PROVISIONAL.psi`), bound to X2.
pub const SI2: &[u8] = include_bytes!("../../../../docs/calibration/SI2-PROVISIONAL.psi");
/// SI3-PROVISIONAL (`docs/calibration/SI3-PROVISIONAL.psi`), bound to X2.
pub const SI3: &[u8] = include_bytes!("../../../../docs/calibration/SI3-PROVISIONAL.psi");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x::XProfile;

    #[test]
    fn si1_roundtrips_is_bound_and_refuses_tampering() {
        let p = SiProfile::si1();
        p.validate().unwrap();
        let b = p.encode();
        assert_eq!(b, SI1, "decode ∘ encode is the identity on the shipped bytes");
        assert_eq!(SiProfile::decode(&b).unwrap(), p);
        assert_eq!(p.xid, XProfile::x1().id(), "SI1 bands the X1 route lanes");
        assert_eq!(p.id_hex16().len(), 16);
        println!("{} id {} ({} bytes)", p.name_str(), hex(&p.id()), b.len());
        let mut x = b.clone();
        x[0] = b'Q';
        assert!(SiProfile::decode(&x).is_err());
        let mut x = b.clone();
        x.push(0);
        assert!(SiProfile::decode(&x).is_err());
        let mut x = b.clone();
        x.truncate(b.len() - 1);
        assert!(SiProfile::decode(&x).is_err());
        let mut q = p.clone();
        q.book[0].thr[0] = [5, 4, 3];
        assert!(q.validate().is_err());
        let mut q = p.clone();
        q.probes = 0;
        assert!(q.validate().is_err());
        let mut q = p.clone();
        q.book[1].w[2] += 1;
        assert_ne!(q.id(), p.id(), "the weights are covered by the identity");
    }

    #[test]
    fn si2_is_si1_bound_to_x2() {
        let (s1, s2) = (SiProfile::si1(), SiProfile::si2());
        assert_eq!(s2.encode(), SI2);
        assert_eq!(s2.xid, XProfile::x2().id(), "SI2 bands the X2 route lanes");
        assert_eq!(s2.name_str(), "SI2-PROVISIONAL");
        // everything but the name and the binding is SI1's
        let mut t = s2.clone();
        t.name = s1.name;
        t.xid = s1.xid;
        assert_eq!(t, s1);
        println!("{} id {}", s2.name_str(), hex(&s2.id()));
    }

    #[test]
    fn si3_is_the_shipped_profile_bound_to_x2() {
        let (s2, s3) = (SiProfile::si2(), SiProfile::si3());
        assert_eq!(SiProfile::shipped(), s3);
        s3.validate().unwrap();
        assert_eq!(s3.encode(), SI3, "decode ∘ encode is the identity on the shipped bytes");
        assert_eq!(s3.xid, XProfile::x2().id(), "SI3 bands the X2 route lanes");
        assert_eq!(s3.name_str(), "SI3-PROVISIONAL");
        assert_eq!((s3.probes, s3.features, s3.budget), (s2.probes, s2.features, s2.budget));
        // a fit of its own on every family but SIL, which keeps SI2's codebook
        // (too few of the chain's works carry a silhouette)
        for f in 0..FAMILIES {
            let same = (s3.book[f].mean.clone(), s3.book[f].proj.clone(), s3.book[f].thr, s3.book[f].sig) == (s2.book[f].mean.clone(), s2.book[f].proj.clone(), s2.book[f].thr, s2.book[f].sig);
            assert_eq!(same, f == crate::x::si::features::F_SIL, "family {f}");
        }
        println!("{} id {}", s3.name_str(), hex(&s3.id()));
    }
}
