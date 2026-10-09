//! The optional PAX1 accelerator sidecar (PAPH-X §18).
//!
//! ```text
//!   magic       "PAX1"
//!   version     u16 = 2
//!   flags       u16 — bits 0–7 the Tier-1 format the side was hashed in
//!               (3 or 4); bit 8 set when its keypoints are Tier 2's (clear:
//!               the Tier-1 sketch)
//!   t1_crc      u32 — the checksum the side's Tier 1 carries
//!   profile_id  32 bytes  — the X profile the structures were built under
//!   route       136 bytes — XRoute record (128 + metadata)
//!   anchors     u16 n, then n x u16  — the anchor order
//!   buckets     u16 n, then 24 x n x u16 codes, 24 x 257 x u16 offsets,
//!               24 x n x u16 ids, 24 x n low code bits
//!   crc         u32 over everything before it
//! ```
//!
//! Content-derived from the wire: safe to cache, discard, recompute or
//! regenerate when the profile changes.  Never consensus state.  A sidecar
//! that does not decode, was built under another profile, or from another
//! side — another Tier 1 (its checksum), another format, the sketch where the
//! side now has Tier 2 or the reverse — is ignored and the structures are
//! rebuilt from the wire (§38).  Version 1 (1.0–1.1) bound only the profile
//! and the keypoint count, so a store re-hashed in wire 4 could have handed
//! a wire-3 side's route to the wire-4 side of the same work; 1.2 refuses
//! every version-1 sidecar and the structures are rebuilt once.

use super::bucket::XBucketIndex;
use super::prepared::XPrepared;
use super::profile::{XBound, LSH_PROJECTIONS};
use super::route::{XRoute, ROUTE_RECORD_BYTES};
use crate::prepared::Prepared;
use crate::wire::crc32;

const MAGIC: &[u8; 4] = b"PAX1";
const VERSION: u16 = 2;
const HEAD: usize = 44;

/// The side a sidecar describes, as its flags and Tier-1 checksum record it.
fn binding(p: &Prepared) -> (u16, u32) {
    (p.t1.version as u16 | if p.tier2 { 0x100 } else { 0 }, p.t1.crc)
}

pub fn encode(x: &XPrepared) -> Vec<u8> {
    let n = x.index.n;
    let mut b = Vec::with_capacity(HEAD + 4 + ROUTE_RECORD_BYTES + 2 + 2 * n + 2 + 2 * n * LSH_PROJECTIONS + 2 * LSH_PROJECTIONS * 257 + 3 * LSH_PROJECTIONS * n + 4);
    let (flags, t1_crc) = binding(&x.p);
    b.extend_from_slice(MAGIC);
    b.extend_from_slice(&VERSION.to_le_bytes());
    b.extend_from_slice(&flags.to_le_bytes());
    b.extend_from_slice(&t1_crc.to_le_bytes());
    b.extend_from_slice(&x.xid);
    b.extend_from_slice(&x.route.to_bytes());
    b.extend_from_slice(&(x.order.len() as u16).to_le_bytes());
    for &i in x.order.iter() {
        b.extend_from_slice(&i.to_le_bytes());
    }
    b.extend_from_slice(&(n as u16).to_le_bytes());
    for &c in x.index.codes.iter() {
        b.extend_from_slice(&c.to_le_bytes());
    }
    for &o in x.index.offsets.iter() {
        b.extend_from_slice(&o.to_le_bytes());
    }
    for &i in x.index.ids.iter() {
        b.extend_from_slice(&i.to_le_bytes());
    }
    b.extend_from_slice(&x.index.low);
    let c = crc32(&b);
    b.extend_from_slice(&c.to_le_bytes());
    b
}

/// Rebuild an `XPrepared` from a prepared side and its sidecar.  `Err`,
/// handing the side back, when the sidecar is not usable for this side under
/// this profile — the caller then builds from the wire.
pub fn decode(p: Prepared, xb: &XBound, b: &[u8]) -> Result<XPrepared, Prepared> {
    let n = p.kp.len();
    let need = HEAD + ROUTE_RECORD_BYTES + 2 + 2 * n + 2 + 2 * n * LSH_PROJECTIONS + 2 * LSH_PROJECTIONS * 257 + 3 * LSH_PROJECTIONS * n + 4;
    if b.len() != need || &b[0..4] != MAGIC || u16::from_le_bytes([b[4], b[5]]) != VERSION {
        return Err(p);
    }
    let c = crc32(&b[..b.len() - 4]);
    let want = u32::from_le_bytes([b[b.len() - 4], b[b.len() - 3], b[b.len() - 2], b[b.len() - 1]]);
    let (flags, t1_crc) = binding(&p);
    if c != want || u16::from_le_bytes([b[6], b[7]]) != flags || u32::from_le_bytes([b[8], b[9], b[10], b[11]]) != t1_crc || b[12..44] != xb.xid {
        return Err(p);
    }
    let mut o = HEAD;
    let Some(route) = XRoute::from_bytes(&b[o..o + ROUTE_RECORD_BYTES]) else { return Err(p) };
    o += ROUTE_RECORD_BYTES;
    let no = u16::from_le_bytes([b[o], b[o + 1]]) as usize;
    o += 2;
    if no != n {
        return Err(p);
    }
    let mut order = Vec::with_capacity(n);
    for i in 0..n {
        let v = u16::from_le_bytes([b[o + 2 * i], b[o + 2 * i + 1]]);
        if v as usize >= n {
            return Err(p);
        }
        order.push(v);
    }
    o += 2 * n;
    let nb = u16::from_le_bytes([b[o], b[o + 1]]) as usize;
    o += 2;
    if nb != n {
        return Err(p);
    }
    let mut codes = Vec::with_capacity(n * LSH_PROJECTIONS);
    for i in 0..n * LSH_PROJECTIONS {
        codes.push(u16::from_le_bytes([b[o + 2 * i], b[o + 2 * i + 1]]));
    }
    o += 2 * n * LSH_PROJECTIONS;
    let mut offsets = Vec::with_capacity(LSH_PROJECTIONS * 257);
    for i in 0..LSH_PROJECTIONS * 257 {
        offsets.push(u16::from_le_bytes([b[o + 2 * i], b[o + 2 * i + 1]]));
    }
    o += 2 * LSH_PROJECTIONS * 257;
    let mut ids = Vec::with_capacity(LSH_PROJECTIONS * n);
    for i in 0..LSH_PROJECTIONS * n {
        let v = u16::from_le_bytes([b[o + 2 * i], b[o + 2 * i + 1]]);
        if v as usize >= n {
            return Err(p);
        }
        ids.push(v);
    }
    o += 2 * LSH_PROJECTIONS * n;
    let low = b[o..o + LSH_PROJECTIONS * n].to_vec();
    // the bucket layout must be internally consistent
    for q in 0..LSH_PROJECTIONS {
        if offsets[q * 257] != 0 || offsets[q * 257 + 256] as usize != n {
            return Err(p);
        }
        for c in 0..256 {
            if offsets[q * 257 + c] > offsets[q * 257 + c + 1] {
                return Err(p);
            }
        }
        for k in 1..n {
            // sorted by code within a projection
            let (a, b_) = (ids[q * n + k - 1] as usize, ids[q * n + k] as usize);
            if codes[a * LSH_PROJECTIONS + q] > codes[b_ * LSH_PROJECTIONS + q] {
                return Err(p);
            }
        }
    }
    // the presence masks are derived from the entries
    let mut present = vec![0u16; LSH_PROJECTIONS * 256];
    for q in 0..LSH_PROJECTIONS {
        for k in 0..n {
            let c = codes[ids[q * n + k] as usize * LSH_PROJECTIONS + q] & super::bucket::CODE_MASK;
            present[q * 256 + (c >> (super::profile::LSH_BITS - 8)) as usize] |= 1 << (c & ((1 << (super::profile::LSH_BITS - 8)) - 1));
        }
    }
    let index = XBucketIndex { n, codes, offsets, present, ids, low, hot_cap: xb.xp.hot_bucket_cap as usize };
    // the burst profile and measurability are cheap: derive them
    let fresh = XPrepared::new(p, xb);
    Ok(XPrepared { route, index, order, ..fresh })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::wire::hash;
    use crate::x::profile::XProfile;

    #[test]
    fn sidecar_roundtrips_and_refuses_damage() {
        let xb = XBound::shipped();
        let rot = crate::keypoints::RotCache::new(&crate::keypoints::pattern());
        let px = crate::x::testimg::image(3, 120, 90);
        let f = hash(&px, 120, 90, &Config::default(), &rot);
        let x = XPrepared::new(Prepared::new(&f.t1, Some(&f.t2)).unwrap(), &xb);
        let b = encode(&x);
        let y = decode(Prepared::new(&f.t1, Some(&f.t2)).unwrap(), &xb, &b).ok().unwrap();
        assert_eq!(y.route, x.route);
        assert_eq!(y.order, x.order);
        assert_eq!((y.index.codes.clone(), y.index.offsets.clone(), y.index.ids.clone(), y.index.low.clone(), y.index.present.clone()), (x.index.codes.clone(), x.index.offsets.clone(), x.index.ids.clone(), x.index.low.clone(), x.index.present.clone()));
        assert_eq!(encode(&y), b);
        let mut bad = b.clone();
        bad[100] ^= 1;
        assert!(decode(Prepared::new(&f.t1, Some(&f.t2)).unwrap(), &xb, &bad).is_err());
        let mut other = XProfile::x1();
        other.hot_bucket_cap += 1;
        let xb2 = XBound::new(xb.base.clone(), other);
        assert!(decode(Prepared::new(&f.t1, Some(&f.t2)).unwrap(), &xb2, &b).is_err(), "another profile");
    }

    /// A sidecar describes one side: the same work's other format, or the
    /// same Tier 1 read without its Tier 2, refuses it; and 1.1's version 1,
    /// which bound neither, is refused outright.
    #[test]
    fn sidecar_is_bound_to_its_side() {
        let xb = XBound::shipped();
        let rot = crate::keypoints::RotCache::new(&crate::keypoints::pattern());
        let px = crate::x::testimg::image(5, 140, 100);
        let f4 = hash(&px, 140, 100, &Config::default(), &rot);
        let f3 = hash(&px, 140, 100, &Config { wire: crate::config::WIRE_3, ..Config::default() }, &rot);
        let side = |f: &crate::wire::Fingerprint, t2: bool| Prepared::new(&f.t1, if t2 { Some(&f.t2[..]) } else { None }).unwrap();
        let (b4, b3) = (encode(&XPrepared::new(side(&f4, true), &xb)), encode(&XPrepared::new(side(&f3, true), &xb)));
        // the formats share the keypoints, so only the binding tells them apart
        assert_eq!(f3.t2[32..], f4.t2[32..], "the two formats' keypoint records");
        assert_eq!(b3.len(), b4.len());
        assert!(decode(side(&f4, true), &xb, &b4).is_ok() && decode(side(&f3, true), &xb, &b3).is_ok());
        assert!(decode(side(&f4, true), &xb, &b3).is_err(), "a wire-3 sidecar on the wire-4 side");
        assert!(decode(side(&f3, true), &xb, &b4).is_err(), "a wire-4 sidecar on the wire-3 side");
        let sketch = encode(&XPrepared::new(side(&f4, false), &xb));
        assert!(decode(side(&f4, false), &xb, &sketch).is_ok());
        assert!(decode(side(&f4, false), &xb, &b4).is_err() && decode(side(&f4, true), &xb, &sketch).is_err(), "tier 2 against the sketch");
        // 1.1's layout: version 1, flags 0, no Tier-1 checksum
        let mut v1 = b"PAX1".to_vec();
        v1.extend_from_slice(&1u16.to_le_bytes());
        v1.extend_from_slice(&0u16.to_le_bytes());
        v1.extend_from_slice(&b3[12..b3.len() - 4]);
        let c = crc32(&v1);
        v1.extend_from_slice(&c.to_le_bytes());
        assert!(decode(side(&f3, true), &xb, &v1).is_err(), "version 1");
    }
}
