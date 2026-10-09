//! Wire format (SPEC-003 §6, §7).
//!
//! Tier 1 — 3952 B, fixed layout, constant offsets, section TABLE.  v2
//! hardcoded its offsets as constants and this family has already paid once for
//! a stale offset after a section was inserted.
//!
//! Tier 2 — variable length, fixed 40 B records, content-addressed to Tier 1 by
//! CRC.  Different rules on purpose: Tier 1 is index-scanned by byte range and
//! Tier 2 is not.
//!
//! CRC-32 rather than v2's XOR: an XOR cannot detect a transposition of two
//! bytes, which is exactly the corruption a byte-range index introduces.

use crate::config::{clamp, idiv, Config, KP_SELECT_QUALITY, MAX_KP_COUNT, WIRE_3, WIRE_4};
use crate::front::normalise;
use crate::keypoints::{keypoints, Keypoint, RotCache};
use crate::sections::*;

/// The wire format `hash` writes by default (`Config::wire`); `parse_t1`
/// reads 3 and 4.  docs/SPEC-W4-paph-wire4.md.
pub const VERSION: u8 = WIRE_4;
pub const HEADER1: usize = 64;
pub const HEADER2: usize = 32;
pub const KP_REC: usize = 40;

pub const F_MATTE: u16 = 1;
pub const F_FLAT: u16 = 2;
pub const F_INVFOLD: u16 = 4;
pub const F_SIL: u16 = 8;
pub const F_UPSCALED: u16 = 16;
/// SPEC-004.2 §3 — Tier 2 was selected by the 4.2 quality rule, not by the
/// 4.1 strength/grid rule.  A 4.1 wire has this bit clear, which is exactly
/// what a 4.1 wire means, so nothing has to be re-hashed to be readable.
pub const F_KPQ: u16 = 32;

pub struct SectionDef {
    pub id: u8,
    pub name: &'static str,
    pub len: usize,
}

pub const SECTIONS: [SectionDef; 11] = [
    SectionDef { id: 1, name: "dct", len: 256 },
    SectionDef { id: 2, name: "brightness", len: 8 },
    SectionDef { id: 3, name: "palette", len: 96 },
    SectionDef { id: 4, name: "rag", len: 288 },
    SectionDef { id: 5, name: "shapes", len: 328 },
    SectionDef { id: 6, name: "runs", len: 48 },
    SectionDef { id: 7, name: "local", len: 1024 },
    SectionDef { id: 8, name: "anchors", len: 512 },
    SectionDef { id: 9, name: "silhouette", len: 96 },
    SectionDef { id: 10, name: "colour", len: 80 },
    SectionDef { id: 11, name: "sketch", len: 1152 },
];

/// The most records each section holds, in `SECTIONS` order: its length over
/// its record's (the DCT, the brightness record and the silhouette are one
/// record; the runs three; a palette entry 4 B, an adjacency 6, a shape 41, a
/// local code 8 and its anchor 4, a colour 5, a sketch keypoint 36).
pub const SECTION_CAP: [usize; 11] = [1, 1, 24, 48, 8, 3, 128, 128, 1, 16, 32];

pub const T1_BYTES: usize = 3952;

pub fn section_offset(name: &str) -> usize {
    let mut t = HEADER1;
    for s in SECTIONS.iter() {
        if s.name == name {
            return t;
        }
        t += s.len;
    }
    t
}

/// CRC-32 (IEEE 802.3, reflected 0xEDB88320) lookup tables for slicing by 8,
/// built at compile time.  `T[0]` is the classic byte table; `T[k][b]` is the
/// CRC of byte `b` followed by `k` zero bytes, which lets eight input bytes be
/// folded per step.  Same polynomial, same init and final XOR, same result as
/// the bytewise loop it replaces — `crc32_bytewise` is kept and tested equal.
const CRC_TABLES: [[u32; 256]; 8] = {
    let mut t = [[0u32; 256]; 8];
    let mut n = 0usize;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[0][n] = c;
        n += 1;
    }
    let mut n = 0usize;
    while n < 256 {
        let mut k = 1;
        while k < 8 {
            let prev = t[k - 1][n];
            t[k][n] = (prev >> 8) ^ t[0][(prev & 0xff) as usize];
            k += 1;
        }
        n += 1;
    }
    t
};

pub fn crc32(b: &[u8]) -> u32 {
    let t = &CRC_TABLES;
    let mut c: u32 = 0xFFFF_FFFF;
    let mut chunks = b.chunks_exact(8);
    for q in &mut chunks {
        let lo = c ^ u32::from_le_bytes([q[0], q[1], q[2], q[3]]);
        let hi = u32::from_le_bytes([q[4], q[5], q[6], q[7]]);
        c = t[7][(lo & 0xff) as usize]
            ^ t[6][((lo >> 8) & 0xff) as usize]
            ^ t[5][((lo >> 16) & 0xff) as usize]
            ^ t[4][(lo >> 24) as usize]
            ^ t[3][(hi & 0xff) as usize]
            ^ t[2][((hi >> 8) & 0xff) as usize]
            ^ t[1][((hi >> 16) & 0xff) as usize]
            ^ t[0][(hi >> 24) as usize];
    }
    for &v in chunks.remainder() {
        c = t[0][((c ^ v as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

#[cfg(test)]
fn crc32_bytewise(b: &[u8]) -> u32 {
    let t = &CRC_TABLES[0];
    let mut c: u32 = 0xFFFF_FFFF;
    for &v in b {
        c = t[((c ^ v as u32) & 0xff) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

pub struct Fingerprint {
    pub t1: Vec<u8>,
    pub t2: Vec<u8>,
    pub kp_count: usize,
    pub max_dim: i64,
    pub xmax: i32,
}

/// Parsed view of a Tier-1 wire.
pub struct Tier1 {
    pub bytes: Vec<u8>,
    /// the wire format, 3 or 4
    pub version: u8,
    pub flags: u16,
    pub width: usize,
    pub height: usize,
    pub scale: u8,
    pub kp_count: usize,
    pub counts: [usize; 11],
    pub crc: u32,
}

impl Tier1 {
    pub fn sec(&self, name: &str) -> &[u8] {
        let o = section_offset(name);
        let l = SECTIONS.iter().find(|s| s.name == name).unwrap().len;
        &self.bytes[o..o + l]
    }
    pub fn count(&self, name: &str) -> usize {
        let i = SECTIONS.iter().position(|s| s.name == name).unwrap();
        self.counts[i]
    }
    pub fn max_dim(&self) -> i64 {
        self.width.max(self.height) as i64
    }
    /// The aspect field of shape record `i` (`rec` = the record's 41 bytes),
    /// as wire 3 stores it: ⌊256·width / height⌋ of the region's box.  Wire 4
    /// stores the width and the height and this recomputes the same value.
    pub fn shape_aspect(&self, rec: &[u8]) -> i64 {
        match self.shape_dims(rec) {
            Some((bw, bh)) => clamp(idiv(bw * 256, bh.max(1)), 0, 65535),
            None => u16::from_le_bytes([rec[6], rec[7]]) as i64,
        }
    }
    /// The box of shape record `rec` as (width, height), on a wire that
    /// stores it (4); `None` on wire 3.
    pub fn shape_dims(&self, rec: &[u8]) -> Option<(i64, i64)> {
        if self.version >= WIRE_4 {
            Some((rec[6] as i64, rec[7] as i64))
        } else {
            None
        }
    }
    /// The silhouette component's box as (width, height), on a wire that
    /// stores it (4, bytes 66–69 of the section); `None` on wire 3.
    pub fn silhouette_dims(&self) -> Option<(i64, i64)> {
        if self.version >= WIRE_4 {
            let s = self.sec("silhouette");
            Some((u16::from_le_bytes([s[66], s[67]]) as i64, u16::from_le_bytes([s[68], s[69]]) as i64))
        } else {
            None
        }
    }
    /// Shape record `rec`'s box, longer side over shorter, in Q8 (256 =
    /// square) — the orientation-free elongation the route (G3) and PAPH-SI
    /// read.  Exact from the sides on wire 4, where a transpose only
    /// exchanges them; from wire 3's rounded ratio `a` as max(a, 65536/a)
    /// otherwise, which a transpose can move across a class edge at ratios
    /// past about 7.
    pub fn shape_asym_q8(&self, rec: &[u8]) -> i64 {
        match self.shape_dims(rec) {
            Some((bw, bh)) => asym_q8(bw, bh),
            None => asym_q8_legacy(u16::from_le_bytes([rec[6], rec[7]]) as i64),
        }
    }
    /// The silhouette component's elongation, as `shape_asym_q8`.
    pub fn silhouette_asym_q8(&self) -> i64 {
        match self.silhouette_dims() {
            Some((bw, bh)) => asym_q8(bw, bh),
            None => {
                let s = self.sec("silhouette");
                asym_q8_legacy(u16::from_le_bytes([s[36], s[37]]) as i64)
            }
        }
    }
}

/// ⌊256 · longer / shorter⌋ of a box.
pub fn asym_q8(bw: i64, bh: i64) -> i64 {
    idiv(bw.max(bh) * 256, bw.min(bh).max(1))
}

/// The same from a stored ⌊256·w/h⌋ (the 1.0–1.1 rule).
pub fn asym_q8_legacy(a: i64) -> i64 {
    if a > 0 {
        a.max(65536 / a)
    } else {
        256
    }
}

/// Refusal reason for a pair whose two wires are of different formats: the
/// sections of a wire-3 and a wire-4 hash of one image differ where wire 4
/// changed the sampling, and no calibration was fitted across the two.
pub const R_WIRE_MISMATCH: &str = "WIRE_MISMATCH";
/// Comparator 3's error for the same pair (`compare::compare`).
pub const E_WIRE_MISMATCH: &str = "tier 1 wire formats differ (3 and 4): hash both sides with one format";

pub fn parse_t1(b: &[u8]) -> Result<Tier1, &'static str> {
    if b.len() != T1_BYTES {
        return Err("tier 1 must be 3952 bytes");
    }
    if &b[0..4] != b"PAPH" {
        return Err("bad magic");
    }
    if b[4] != WIRE_3 && b[4] != WIRE_4 {
        return Err("unsupported version (v2 wires MUST be rejected, never reinterpreted)");
    }
    if b[5] != 1 {
        return Err("not a tier 1 wire");
    }
    let c = crc32(&b[HEADER1..]);
    let want = u32::from_le_bytes([b[60], b[61], b[62], b[63]]);
    if c != want {
        return Err("tier 1 checksum mismatch");
    }
    let mut counts = [0usize; 11];
    for (i, s) in SECTIONS.iter().enumerate() {
        let o = 16 + i * 4;
        if b[o] != s.id {
            return Err("section table mismatch");
        }
        counts[i] = b[o + 1] as usize;
        // the checksum covers the sections, not this table: a count past what
        // its section holds would send every reader past the section's end
        if counts[i] > SECTION_CAP[i] {
            return Err("section count exceeds what the section holds");
        }
    }
    // wire 4's shape records carry their region's box on the shape grid,
    // whose sides are 1 to 128
    if b[4] == WIRE_4 {
        let o = section_offset("shapes");
        for r in b[o..o + counts[4] * 41].chunks_exact(41) {
            if !(1..=128).contains(&r[6]) || !(1..=128).contains(&r[7]) {
                return Err("shape record box out of range");
            }
        }
    }
    Ok(Tier1 {
        bytes: b.to_vec(),
        version: b[4],
        flags: u16::from_le_bytes([b[6], b[7]]),
        width: u16::from_le_bytes([b[8], b[9]]) as usize,
        height: u16::from_le_bytes([b[10], b[11]]) as usize,
        scale: b[12],
        kp_count: u16::from_le_bytes([b[14], b[15]]) as usize,
        counts,
        crc: c,
    })
}

pub struct Tier2 {
    pub list: Vec<Keypoint>,
    pub max_dim: i64,
    pub xmax: i32,
    /// SPEC-004.2 §3 — the selection rule that built these records.  Byte 16
    /// of the Tier-2 header was reserved and therefore zero on every 4.1
    /// wire, which reads back as `KP_SELECT_LEGACY` without a version bump.
    pub select: i32,
}

pub fn parse_t2(b: &[u8]) -> Result<Tier2, &'static str> {
    if b.len() < HEADER2 {
        return Err("tier 2 too short");
    }
    if &b[0..4] != b"PAP2" {
        return Err("bad tier 2 magic");
    }
    let n = u16::from_le_bytes([b[6], b[7]]) as usize;
    if b.len() != HEADER2 + n * KP_REC {
        return Err("tier 2 length does not match its record count");
    }
    let c = crc32(&b[HEADER2..]);
    let want = u32::from_le_bytes([b[28], b[29], b[30], b[31]]);
    if c != want {
        return Err("tier 2 checksum mismatch");
    }
    let mut list = Vec::with_capacity(n);
    for i in 0..n {
        let o = HEADER2 + i * KP_REC;
        let mut desc = [0u32; 8];
        for j in 0..8 {
            desc[j] = u32::from_le_bytes([b[o + j * 4], b[o + j * 4 + 1], b[o + j * 4 + 2], b[o + j * 4 + 3]]);
        }
        list.push(Keypoint {
            desc,
            x: u16::from_le_bytes([b[o + 32], b[o + 33]]) as i32,
            y: u16::from_le_bytes([b[o + 34], b[o + 35]]) as i32,
            level: b[o + 36],
            sec: b[o + 37] & 63,
            s: u16::from_le_bytes([b[o + 38], b[o + 39]]),
        });
    }
    Ok(Tier2 {
        list,
        max_dim: u16::from_le_bytes([b[12], b[13]]) as i64,
        xmax: u16::from_le_bytes([b[14], b[15]]) as i32,
        select: b[16] as i32,
    })
}

fn serialize_t2(kps: &[Keypoint], t1crc: u32, max_dim: i64, xmax: i32, select: i32, version: u8) -> Vec<u8> {
    let n = kps.len().min(MAX_KP_COUNT);
    let mut b = vec![0u8; HEADER2 + n * KP_REC];
    b[0..4].copy_from_slice(b"PAP2");
    b[4] = version;
    b[5] = 2;
    b[6..8].copy_from_slice(&(n as u16).to_le_bytes());
    b[8..12].copy_from_slice(&t1crc.to_le_bytes());
    b[12..14].copy_from_slice(&(max_dim as u16).to_le_bytes());
    b[14..16].copy_from_slice(&(xmax as u16).to_le_bytes());
    b[16] = select as u8;
    for (i, k) in kps.iter().take(n).enumerate() {
        let o = HEADER2 + i * KP_REC;
        for j in 0..8 {
            b[o + j * 4..o + j * 4 + 4].copy_from_slice(&k.desc[j].to_le_bytes());
        }
        b[o + 32..o + 34].copy_from_slice(&(k.x as u16).to_le_bytes());
        b[o + 34..o + 36].copy_from_slice(&(k.y as u16).to_le_bytes());
        b[o + 36] = k.level;
        b[o + 37] = (k.sec & 63) | 64;
        b[o + 38..o + 40].copy_from_slice(&k.s.to_le_bytes());
    }
    let c = crc32(&b[HEADER2..]);
    b[28..32].copy_from_slice(&c.to_le_bytes());
    b
}

/// The 32-keypoint Tier-1 sketch — retrieval, and a Tier-1-only weak geometric
/// check.  Without it an index built on Tier 1 alone could only retrieve
/// through the fingerprint bag, and the pairs the geometric stage EXISTS for
/// are exactly the ones whose bags barely intersect.
fn sketch_bytes(kps: &[Keypoint], want: usize) -> (Vec<u8>, usize) {
    let mut out = vec![0u8; want * 36];
    let mut by: Vec<&Keypoint> = kps.iter().collect();
    by.sort_by(|a, b| b.s.cmp(&a.s).then(a.x.cmp(&b.x)).then(a.y.cmp(&b.y)));
    by.truncate(want);
    by.sort_by(|a, b| a.desc[0].cmp(&b.desc[0]).then(a.x.cmp(&b.x)).then(a.y.cmp(&b.y)));
    for (i, k) in by.iter().enumerate() {
        let o = i * 36;
        for j in 0..8 {
            out[o + j * 4..o + j * 4 + 4].copy_from_slice(&k.desc[j].to_le_bytes());
        }
        out[o + 32..o + 34].copy_from_slice(&(k.x as u16).to_le_bytes());
        out[o + 34..o + 36].copy_from_slice(&(k.y as u16).to_le_bytes());
    }
    let n = by.len();
    (out, n)
}

pub fn read_sketch(t: &Tier1) -> Vec<Keypoint> {
    let s = t.sec("sketch");
    let n = t.count("sketch");
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let o = i * 36;
        let mut desc = [0u32; 8];
        for j in 0..8 {
            desc[j] = u32::from_le_bytes([s[o + j * 4], s[o + j * 4 + 1], s[o + j * 4 + 2], s[o + j * 4 + 3]]);
        }
        out.push(Keypoint {
            desc,
            x: u16::from_le_bytes([s[o + 32], s[o + 33]]) as i32,
            y: u16::from_le_bytes([s[o + 34], s[o + 35]]) as i32,
            level: 0,
            sec: 0,
            s: 0,
        });
    }
    out
}

/// One pass, both tiers, one normalised image.
pub fn hash(px: &[u8], w: usize, h: usize, cfg: &Config, rot: &RotCache) -> Fingerprint {
    let norm = normalise(px, w, h, cfg);
    let im = &norm.im;

    let wire = if cfg.wire == WIRE_3 { WIRE_3 } else { WIRE_4 };
    let thumb = thumbnail16(im, wire);
    let flat = thumb.iter().all(|&v| v == thumb[0]);

    let mut flags: u16 = 0;
    if norm.matte {
        flags |= F_MATTE;
    }
    if norm.scale > 1 {
        flags |= F_UPSCALED;
    }
    if cfg.fold_invert {
        flags |= F_INVFOLD;
    }
    if flat {
        flags |= F_FLAT;
    }
    if cfg.kp_select == KP_SELECT_QUALITY {
        flags |= F_KPQ;
    }

    let dct = hierarchical_dct(&thumb, wire);
    let brt = brightness_record(&thumb);
    let (pal, pal_n) = identity_palette(im);
    let (rag, rag_n) = sparse_rag(im);
    let (shp, shp_n) = shape_signatures(im, wire);
    let runs = run_lengths(im);
    let loc = local_fingerprints(im, cfg);
    let (sil, sil_ok) = silhouette(im, wire);
    let (col, col_n) = colour_digest(im);
    if sil_ok {
        flags |= F_SIL;
    }

    let kp = keypoints(im, cfg, rot);
    let (sk, sk_n) = sketch_bytes(&kp.list, cfg.sketch_count);

    let bodies: [&[u8]; 11] = [&dct, &brt, &pal, &rag, &shp, &runs, &loc.bytes, &loc.pos, &sil, &col, &sk];
    let counts: [usize; 11] = [
        1,
        1,
        pal_n,
        rag_n,
        shp_n,
        3,
        loc.count,
        loc.count,
        if sil_ok { 1 } else { 0 },
        col_n,
        sk_n,
    ];

    let mut b = vec![0u8; T1_BYTES];
    b[0..4].copy_from_slice(b"PAPH");
    b[4] = wire;
    b[5] = 1;
    b[6..8].copy_from_slice(&flags.to_le_bytes());
    b[8..10].copy_from_slice(&(norm.orig_w as u16).to_le_bytes());
    b[10..12].copy_from_slice(&(norm.orig_h as u16).to_le_bytes());
    b[12] = clamp(norm.scale as i64, 1, 255) as u8;
    // integer ceil(log2): Math.log2 is implementation-approximated and this
    // byte reaches the wire
    let md = (norm.orig_w.max(norm.orig_h).max(2)) as i64;
    let mut lg = 0i64;
    while (1i64 << lg) < md {
        lg += 1;
    }
    b[13] = clamp(lg, 1, 31) as u8;
    b[14..16].copy_from_slice(&(kp.list.len() as u16).to_le_bytes());

    let mut off = HEADER1;
    for (i, s) in SECTIONS.iter().enumerate() {
        let o = 16 + i * 4;
        b[o] = s.id;
        b[o + 1] = clamp(counts[i] as i64, 0, 255) as u8;
        b[o + 2..o + 4].copy_from_slice(&(s.len as u16).to_le_bytes());
        // A section body may be SHORTER than its slot — the sketch is
        // `sketch_count * 36` bytes — and the rest of the slot stays zero,
        // exactly as the JavaScript engine's `Uint8Array.set` leaves it.
        // (Copying `s.len` bytes here panicked for any sketchCount < 32.)
        let n = s.len.min(bodies[i].len());
        b[off..off + n].copy_from_slice(&bodies[i][..n]);
        off += s.len;
    }
    let c = crc32(&b[HEADER1..]);
    b[60..64].copy_from_slice(&c.to_le_bytes());

    let t2 = serialize_t2(&kp.list, c, kp.max_dim, kp.xmax, cfg.kp_select, wire);
    Fingerprint {
        t1: b,
        t2,
        kp_count: kp.list.len(),
        max_dim: kp.max_dim,
        xmax: kp.xmax,
    }
}

// keep idiv referenced for the integer-ceil comment above staying honest
#[allow(dead_code)]
fn _unused(a: i64, b: i64) -> i64 {
    idiv(a, b)
}

// ======================================================= SPEC-004 §16 limits
// v3 defined no limits; front.rs accepts anything.  v4 rejects at hash time,
// deterministically, through this checked entry.  The unchecked `hash` stays
// exactly as shipped for the v3 path and the C ABI; nothing about existing
// wires changes.  Errors are prefixed "limit: " so callers can map them to
// Indeterminate(LIMIT) — never to a silent resize (the comparator must not
// substitute a different image for the one it was asked about).

pub const MAX_WIDTH: i64 = 16_384;
pub const MAX_HEIGHT: i64 = 16_384;
pub const MAX_PIXELS: i64 = 1 << 24;

pub fn hash_checked(
    px: &[u8],
    w: usize,
    h: usize,
    cfg: &Config,
    rot: &RotCache,
    limits: Option<&[i32; 3]>,
) -> Result<Fingerprint, &'static str> {
    let (mut mw, mut mh, mut mp) = (MAX_WIDTH, MAX_HEIGHT, MAX_PIXELS);
    if let Some(l) = limits {
        // Profiles may lower the limits, never raise them (§16).
        mw = mw.min(l[0].max(1) as i64);
        mh = mh.min(l[1].max(1) as i64);
        mp = mp.min(l[2].max(1) as i64);
    }
    if w == 0 || h == 0 {
        return Err("limit: empty image");
    }
    if px.len() != w * h * 4 {
        return Err("limit: pixel buffer length mismatch");
    }
    if (w as i64) > mw {
        return Err("limit: width exceeds maximum");
    }
    if (h as i64) > mh {
        return Err("limit: height exceeds maximum");
    }
    if (w as i64) * (h as i64) > mp {
        return Err("limit: pixel count exceeds maximum");
    }
    Ok(hash(px, w, h, cfg, rot))
}

#[cfg(test)]
mod crc_tests {
    use super::*;

    #[test]
    fn slicing_by_8_is_the_bytewise_crc() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926, "the CRC-32 check value");
        assert_eq!(crc32(b""), 0);
        let mut s = 0x2545_f491_4f6c_dd1du64;
        let data: Vec<u8> = (0..5000)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                s as u8
            })
            .collect();
        for len in [0usize, 1, 7, 8, 9, 15, 16, 17, 63, 64, 65, 3888, 4999, 5000] {
            for off in [0usize, 1, 3] {
                if off + len <= data.len() {
                    let d = &data[off..off + len];
                    assert_eq!(crc32(d), crc32_bytewise(d), "len {len} off {off}");
                }
            }
        }
    }
}

#[cfg(test)]
mod limit_tests {
    use super::*;
    use crate::config::Config;
    use crate::keypoints::{pattern, RotCache};

    #[test]
    fn checked_hash_enforces_and_profiles_only_lower() {
        let cfg = Config::default();
        let rot = RotCache::new(&pattern());
        let px16 = vec![128u8; 16 * 16 * 4];
        assert!(hash_checked(&px16, 16, 16, &cfg, &rot, None).is_ok());
        // profile lowers below the input
        let e = hash_checked(&px16, 16, 16, &cfg, &rot, Some(&[8, 8, 64])).err().unwrap();
        assert!(e.starts_with("limit: "), "{e}");
        // a profile cannot RAISE past the spec ceiling
        let e = hash_checked(&px16, 16, 16, &cfg, &rot, Some(&[1 << 20, 1 << 20, i32::MAX]));
        assert!(e.is_ok(), "raising is a no-op above inputs within spec limits");
        // buffer mismatch and empties
        assert!(hash_checked(&px16, 16, 15, &cfg, &rot, None).is_err());
        assert!(hash_checked(&[], 0, 0, &cfg, &rot, None).is_err());
        // pixel-count gate without a giant allocation: 9x8 vs mp=64
        let px = vec![0u8; 9 * 8 * 4];
        let e = hash_checked(&px, 9, 8, &cfg, &rot, Some(&[16, 16, 64])).err().unwrap();
        assert_eq!(e, "limit: pixel count exceeds maximum");
    }

    /// The section table sits outside the checksum, so a parser must hold its
    /// counts to what each section holds — past that, every reader would run
    /// off the section's end — and wire 4's shape boxes to the shape grid.
    #[test]
    fn parse_holds_counts_and_shape_boxes_to_their_sections() {
        let rot = RotCache::new(&pattern());
        let im = crate::synth::pixel_art(150, 110, 9, 7, 0);
        for wire in [crate::config::WIRE_3, crate::config::WIRE_4] {
            let f = hash(&im.px, im.w, im.h, &Config { wire, ..Config::default() }, &rot);
            let t = parse_t1(&f.t1).unwrap();
            for (i, s) in SECTIONS.iter().enumerate() {
                assert!(t.counts[i] <= SECTION_CAP[i], "{} count {}", s.name, t.counts[i]);
                assert!(SECTION_CAP[i] >= 1 && s.len % SECTION_CAP[i] == 0, "{}", s.name);
                let mut b = f.t1.clone();
                b[16 + i * 4 + 1] = SECTION_CAP[i] as u8 + 1;
                assert_eq!(parse_t1(&b).err(), Some("section count exceeds what the section holds"), "{} wire {wire}", s.name);
                b[16 + i * 4 + 1] = SECTION_CAP[i] as u8;
                assert!(parse_t1(&b).is_ok() || (wire == crate::config::WIRE_4 && s.name == "shapes"), "{} at capacity, wire {wire}", s.name);
            }
            // a counted shape record's box, on the wire that stores one
            assert!(t.count("shapes") > 0);
            let o = section_offset("shapes");
            for (k, v) in [(6usize, 0u8), (7, 0), (6, 129), (7, 200)] {
                let mut b = f.t1.clone();
                b[o + k] = v;
                let c = crc32(&b[HEADER1..]);
                b[60..64].copy_from_slice(&c.to_le_bytes());
                let r = parse_t1(&b);
                if wire == crate::config::WIRE_4 {
                    assert_eq!(r.err(), Some("shape record box out of range"), "byte {k} = {v}");
                } else {
                    assert!(r.is_ok(), "wire 3 stores an aspect there");
                }
            }
        }
    }
}
