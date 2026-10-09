//! The calibration profile artefact (SPEC-004 §8, Appendix B).
//!
//! Calibration is data, not code (P7): one immutable byte artefact, one
//! canonical encoding, SHA-256 identity.  The struct in memory is a *view* of
//! the bytes; decode(encode(p)) round-trips bit-exactly and the conformance
//! suite asserts it.  Anything that fails validation is PROFILE_UNSUPPORTED —
//! a profile is never "mostly loaded".

use crate::config::SCALE;
use crate::sha256::{hex, sha256};

pub const CONTAINER_VERSION: u16 = 1;
pub const COMPARATOR_VERSION: u16 = 4;
/// SPEC-004.1: container 2 carries nine per-channel tables; comparator 41.
pub const CONTAINER_V2: u16 = 2;
pub const COMPARATOR_V41: u16 = 41;
/// SPEC-004.2: container 3 carries the nine per-channel tables of container 2
/// PLUS the eight matcher/geometry scalars comparator 42 introduces.
pub const CONTAINER_V3: u16 = 3;
pub const COMPARATOR_V42: u16 = 42;
const MAGIC: &[u8; 4] = b"PCAL";
const FIXED: usize = 144; // bytes before the v42 block, Appendix B
/// Container 3 appends eight i32 after the container-1/2 fixed block, so a
/// container-1 or container-2 artefact keeps the byte layout it shipped with
/// and its identity hash does not move.
const FIXED_V3: usize = FIXED + 8 * 4;

const fn fixed_len(container: u16) -> usize {
    if container == CONTAINER_V3 {
        FIXED_V3
    } else {
        FIXED
    }
}
const MAX_BREAKPOINTS: usize = 33;

/// Monotone breakpoint table (§8.3): x strictly increasing 0 → SCALE,
/// y non-decreasing, all in currency.  Evaluation is integer linear
/// interpolation with `idiv` truncation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lut(pub Vec<(u16, u16)>);

impl Lut {
    pub fn identity() -> Lut {
        Lut(vec![(0, 0), (SCALE as u16, SCALE as u16)])
    }
    /// Neutral for a MULTIPLIER table (§10.4): constant SCALE, so
    /// `evidence · eval(D) / SCALE` is a no-op.  Identity is neutral for a
    /// margin→evidence mapping; for a modulation it would multiply by
    /// D/SCALE — the distinction Appendix C spells out.
    pub fn neutral() -> Lut {
        Lut(vec![(0, SCALE as u16), (SCALE as u16, SCALE as u16)])
    }
    pub fn validate(&self) -> Result<(), &'static str> {
        let p = &self.0;
        if p.len() < 2 || p.len() > MAX_BREAKPOINTS {
            return Err("lut breakpoint count");
        }
        if p[0].0 != 0 || p[p.len() - 1].0 as i64 != SCALE {
            return Err("lut must span 0..SCALE");
        }
        for w in p.windows(2) {
            if w[1].0 <= w[0].0 {
                return Err("lut x not strictly increasing");
            }
            if w[1].1 < w[0].1 {
                return Err("lut y not monotone");
            }
        }
        if p.iter().any(|&(_, y)| y as i64 > SCALE) {
            return Err("lut y out of currency");
        }
        Ok(())
    }
    pub fn eval(&self, x: i64) -> i64 {
        let x = x.clamp(0, SCALE);
        let p = &self.0;
        let mut i = 0;
        while i + 2 < p.len() && (p[i + 1].0 as i64) < x {
            i += 1;
        }
        // hold the last breakpoint exactly
        if i + 2 == p.len() && x >= p[i + 1].0 as i64 {
            return p[i + 1].1 as i64;
        }
        let (x0, y0) = (p[i].0 as i64, p[i].1 as i64);
        let (x1, y1) = (p[i + 1].0 as i64, p[i + 1].1 as i64);
        y0 + (y1 - y0) * (x - x0) / (x1 - x0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    /// (container, comparator): (1, 4) or (2, 41) — SPEC-004.1 §A3
    pub container: u16,
    pub comparator: u16,
    pub name: [u8; 16],
    pub evidence_rule: u8, // 0 = Lift (bound; Proportion retired from decisions)
    pub scoring_rule: u8,  // 1 = Weighted (bound; Gate retired)
    pub rag_endpoint: u8,  // 1 = Rank
    pub grid_g: u8,
    pub hamming_t: i32,
    pub confidence_at: i32,
    pub geo_conf_at: i32,
    pub geo_eps: i32,
    pub geo_min_corr: i32,
    pub min_model_inliers: i32,
    pub max_models: i32,
    pub rep_extreme_at: i32,
    pub coverage_floor: i32,
    pub min_secondaries: i32,

    // ---- SPEC-004.2, container 3 only (defaults below are the 4.1 behaviour,
    // so a container-1/2 profile widened to container 3 does not move) ----
    /// §4 the Lowe ratio, as a fraction: accept when `d1 * den < num * d2`
    pub lowe_num: i32,
    pub lowe_den: i32,
    /// §4 the ABSOLUTE margin beside the ratio: `d2 - d1 >= lowe_margin`,
    /// two-sided like the ratio itself.  0 disables it.
    pub lowe_margin: i32,
    /// §4 the hard descriptor-distance ceiling
    pub ham_max: i32,
    /// §6 correspondences that must share the winning Hough cell before a fit
    /// is attempted at all
    pub min_peak_members: i32,
    /// §7 soft exclusion around a consumed keypoint, as a percentage of the
    /// descriptor patch footprint at that keypoint's own pyramid level.  0
    /// disables it and restores the 4.1 consumption rule exactly.
    pub excl_pct: i32,
    /// §3 the keypoint-selection rule this calibration was derived on
    pub kp_select: i32,
    /// §5 soft scale binning in the Hough vote (0 = the 4.1 hard bucket)
    pub scale_soft: i32,

    /// identical, struct strong/moderate/weak, struct solo,
    /// geo strong, geo weak, geo solo inliers, topology dominant at
    pub thresholds: [i32; 9],
    /// local, shape, topology, runs, dct, palette, silhouette
    pub weights: [i32; 7],
    /// max_width, max_height, max_pixels
    pub limits: [i32; 3],
    pub lut_local: Lut,
    pub lut_geometry: Lut,
    pub lut_diversity: Lut,
    /// container-2 per-channel tables (identity when decoded from container 1)
    pub lut_dct: Lut,
    pub lut_shape: Lut,
    pub lut_topology: Lut,
    pub lut_runs: Lut,
    pub lut_palette: Lut,
    pub lut_silhouette: Lut,
    /// SPEC-004.2 §8 — the GEOMETRIC diversity multiplier.  Deliberately NOT
    /// `lut_diversity`: that one modulates the local channel by its own
    /// repetition reading (Appendix C) and moving it would move the structural
    /// axis.  This table sees the diversity of the geometric evidence only.
    pub lut_geo_diversity: Lut,
}

fn w32(b: &mut Vec<u8>, v: i32) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn r32(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

impl Profile {
    pub fn name_str(&self) -> String {
        let end = self.name.iter().position(|&c| c == 0).unwrap_or(16);
        String::from_utf8_lossy(&self.name[..end]).into_owned()
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(FIXED + 64);
        b.extend_from_slice(MAGIC);
        b.extend_from_slice(&self.container.to_le_bytes());
        b.extend_from_slice(&self.comparator.to_le_bytes());
        b.extend_from_slice(&self.name);
        b.push(self.evidence_rule);
        b.push(self.scoring_rule);
        b.push(self.rag_endpoint);
        b.push(self.grid_g);
        for v in [
            self.hamming_t, self.confidence_at, self.geo_conf_at, self.geo_eps,
            self.geo_min_corr, self.min_model_inliers, self.max_models,
            self.rep_extreme_at, self.coverage_floor, self.min_secondaries,
        ] {
            w32(&mut b, v);
        }
        for v in self.thresholds {
            w32(&mut b, v);
        }
        for v in self.weights {
            w32(&mut b, v);
        }
        for v in self.limits {
            w32(&mut b, v);
        }
        debug_assert_eq!(b.len(), FIXED);
        if self.container == CONTAINER_V3 {
            for v in [
                self.lowe_num, self.lowe_den, self.lowe_margin, self.ham_max,
                self.min_peak_members, self.excl_pct, self.kp_select, self.scale_soft,
            ] {
                w32(&mut b, v);
            }
            debug_assert_eq!(b.len(), FIXED_V3);
        }
        for lut in self.lut_table() {
            b.extend_from_slice(&(lut.0.len() as u16).to_le_bytes());
            for &(x, y) in &lut.0 {
                b.extend_from_slice(&x.to_le_bytes());
                b.extend_from_slice(&y.to_le_bytes());
            }
        }
        b
    }

    /// The container's table list, canonical order (SPEC-004.1 §A3).
    fn lut_table(&self) -> Vec<&Lut> {
        let mut v = vec![&self.lut_local, &self.lut_geometry, &self.lut_diversity];
        if self.container == CONTAINER_V2 || self.container == CONTAINER_V3 {
            v.extend([&self.lut_dct, &self.lut_shape, &self.lut_topology,
                      &self.lut_runs, &self.lut_palette, &self.lut_silhouette]);
        }
        if self.container == CONTAINER_V3 {
            v.push(&self.lut_geo_diversity);
        }
        v
    }

    /// A structural channel's table by name — identity semantics for any
    /// channel a container-1 profile never heard of.
    pub fn lut_channel(&self, name: &str) -> &Lut {
        match name {
            "dct" => &self.lut_dct,
            "shape" => &self.lut_shape,
            "topology" => &self.lut_topology,
            "runs" => &self.lut_runs,
            "palette" => &self.lut_palette,
            "silhouette" => &self.lut_silhouette,
            _ => &self.lut_local,
        }
    }

    pub fn decode(b: &[u8]) -> Result<Profile, &'static str> {
        if b.len() < FIXED + 3 * 2 {
            return Err("profile too short");
        }
        if &b[0..4] != MAGIC {
            return Err("bad profile magic");
        }
        let container = u16::from_le_bytes([b[4], b[5]]);
        let comparator = u16::from_le_bytes([b[6], b[7]]);
        let nlut = match (container, comparator) {
            (CONTAINER_VERSION, COMPARATOR_VERSION) => 3,
            (CONTAINER_V2, COMPARATOR_V41) => 9,
            (CONTAINER_V3, COMPARATOR_V42) => 10,
            (CONTAINER_VERSION, _) | (CONTAINER_V2, _) | (CONTAINER_V3, _) => {
                return Err("profile targets another comparator")
            }
            _ => return Err("unsupported container version"),
        };
        let fixed = fixed_len(container);
        if b.len() < fixed + 3 * 2 {
            return Err("profile too short");
        }
        let mut name = [0u8; 16];
        name.copy_from_slice(&b[8..24]);
        let mut p = Profile {
            container,
            comparator,
            name,
            evidence_rule: b[24],
            scoring_rule: b[25],
            rag_endpoint: b[26],
            grid_g: b[27],
            hamming_t: r32(b, 28),
            confidence_at: r32(b, 32),
            geo_conf_at: r32(b, 36),
            geo_eps: r32(b, 40),
            geo_min_corr: r32(b, 44),
            min_model_inliers: r32(b, 48),
            max_models: r32(b, 52),
            rep_extreme_at: r32(b, 56),
            coverage_floor: r32(b, 60),
            min_secondaries: r32(b, 64),
            lowe_num: 82,
            lowe_den: 100,
            lowe_margin: 0,
            ham_max: 88,
            min_peak_members: 3,
            excl_pct: 0,
            kp_select: 0,
            scale_soft: 0,
            thresholds: [0; 9],
            weights: [0; 7],
            limits: [0; 3],
            lut_local: Lut(Vec::new()),
            lut_geometry: Lut(Vec::new()),
            lut_diversity: Lut(Vec::new()),
            lut_dct: Lut::identity(),
            lut_shape: Lut::identity(),
            lut_topology: Lut::identity(),
            lut_runs: Lut::identity(),
            lut_palette: Lut::identity(),
            lut_silhouette: Lut::identity(),
            lut_geo_diversity: Lut::neutral(),
        };
        for i in 0..9 {
            p.thresholds[i] = r32(b, 68 + i * 4);
        }
        for i in 0..7 {
            p.weights[i] = r32(b, 104 + i * 4);
        }
        for i in 0..3 {
            p.limits[i] = r32(b, 132 + i * 4);
        }
        if container == CONTAINER_V3 {
            p.lowe_num = r32(b, FIXED);
            p.lowe_den = r32(b, FIXED + 4);
            p.lowe_margin = r32(b, FIXED + 8);
            p.ham_max = r32(b, FIXED + 12);
            p.min_peak_members = r32(b, FIXED + 16);
            p.excl_pct = r32(b, FIXED + 20);
            p.kp_select = r32(b, FIXED + 24);
            p.scale_soft = r32(b, FIXED + 28);
        }
        let mut o = fixed;
        let mut luts = Vec::with_capacity(nlut);
        for _ in 0..nlut {
            if o + 2 > b.len() {
                return Err("truncated lut header");
            }
            let k = u16::from_le_bytes([b[o], b[o + 1]]) as usize;
            o += 2;
            if o + k * 4 > b.len() {
                return Err("truncated lut");
            }
            let mut pts = Vec::with_capacity(k);
            for i in 0..k {
                pts.push((
                    u16::from_le_bytes([b[o + i * 4], b[o + i * 4 + 1]]),
                    u16::from_le_bytes([b[o + i * 4 + 2], b[o + i * 4 + 3]]),
                ));
            }
            o += k * 4;
            luts.push(Lut(pts));
        }
        if o != b.len() {
            return Err("trailing bytes in profile");
        }
        if nlut == 10 {
            p.lut_geo_diversity = luts.pop().unwrap();
        }
        if nlut >= 9 {
            p.lut_silhouette = luts.pop().unwrap();
            p.lut_palette = luts.pop().unwrap();
            p.lut_runs = luts.pop().unwrap();
            p.lut_topology = luts.pop().unwrap();
            p.lut_shape = luts.pop().unwrap();
            p.lut_dct = luts.pop().unwrap();
        }
        p.lut_diversity = luts.pop().unwrap();
        p.lut_geometry = luts.pop().unwrap();
        p.lut_local = luts.pop().unwrap();
        p.validate()?;
        Ok(p)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match (self.container, self.comparator) {
            (CONTAINER_VERSION, COMPARATOR_VERSION)
            | (CONTAINER_V2, COMPARATOR_V41)
            | (CONTAINER_V3, COMPARATOR_V42) => {}
            _ => return Err("container/comparator pair outside 4, 4.1 or 4.2"),
        }
        if self.container == CONTAINER_V3 {
            if !(1..=self.lowe_den).contains(&self.lowe_num) || !(1..=1000).contains(&self.lowe_den) {
                return Err("lowe ratio range");
            }
            if !(0..=64).contains(&self.lowe_margin) {
                return Err("lowe_margin range");
            }
            if !(0..=256).contains(&self.ham_max) {
                return Err("ham_max range");
            }
            if !(2..=64).contains(&self.min_peak_members) {
                return Err("min_peak_members range");
            }
            if !(0..=1000).contains(&self.excl_pct) {
                return Err("excl_pct range");
            }
            if !(0..=1).contains(&self.kp_select) || !(0..=1).contains(&self.scale_soft) {
                return Err("kp_select / scale_soft are flags");
            }
        }
        if self.evidence_rule != 0 || self.scoring_rule != 1 || self.rag_endpoint != 1 {
            return Err("bound rule outside v4 (Proportion/Gate are retired)");
        }
        if !(2..=16).contains(&self.grid_g) {
            return Err("grid_g range");
        }
        if !(0..=128).contains(&self.hamming_t) {
            return Err("hamming_t range");
        }
        if self.confidence_at < 1 || self.geo_conf_at < 1 || self.geo_eps < 0 {
            return Err("confidence field range");
        }
        if !(2..=64).contains(&self.geo_min_corr) || !(2..=64).contains(&self.min_model_inliers) {
            return Err("model inlier floors");
        }
        if !(1..=8).contains(&self.max_models) {
            return Err("max_models range");
        }
        for v in [self.rep_extreme_at, self.coverage_floor] {
            if !(0..=SCALE as i32).contains(&v) {
                return Err("currency field range");
            }
        }
        if !(0..=6).contains(&self.min_secondaries) {
            return Err("min_secondaries range");
        }
        for (i, &t) in self.thresholds.iter().enumerate() {
            let hi = if i == 7 { 512 } else { SCALE as i32 }; // GEO_SOLO_INLIERS is a count
            if !(0..=hi).contains(&t) {
                return Err("threshold range");
            }
        }
        if self.weights.iter().any(|&w| w < 0 || w > SCALE as i32) {
            return Err("weight range");
        }
        if self.limits.iter().any(|&l| l <= 0) || self.limits[0] > 65535 || self.limits[1] > 65535 {
            return Err("limit range");
        }
        self.lut_local.validate()?;
        self.lut_geometry.validate()?;
        self.lut_diversity.validate()?;
        self.lut_dct.validate()?;
        self.lut_shape.validate()?;
        self.lut_topology.validate()?;
        self.lut_runs.validate()?;
        self.lut_palette.validate()?;
        self.lut_silhouette.validate()?;
        self.lut_geo_diversity.validate()
    }

    /// calibration_profile_id = SHA-256 over the artefact bytes (§8.2).
    pub fn id(&self) -> [u8; 32] {
        sha256(&self.encode())
    }
    pub fn id_hex16(&self) -> String {
        hex(&self.id()[..8])
    }

    /// Appendix C — CAL-001-PROVISIONAL: v3 shipping constants plus every new
    /// mechanism switched to neutral, so that M1 changes only what §9.5
    /// changes.  Every "new" value is a hypothesis for §19 to test.
    pub fn cal001() -> Profile {
        let mut name = [0u8; 16];
        name[..16].copy_from_slice(b"CAL-001-PROVISIO");
        Profile {
            container: CONTAINER_VERSION,
            comparator: COMPARATOR_VERSION,
            name,
            evidence_rule: 0,
            scoring_rule: 1,
            rag_endpoint: 1,
            grid_g: 4,
            hamming_t: 8,
            confidence_at: 16,
            geo_conf_at: 16,
            geo_eps: 1600,
            geo_min_corr: 8,
            min_model_inliers: 6,
            max_models: 4,
            rep_extreme_at: 1500,
            coverage_floor: 2500,
            min_secondaries: 3,
            thresholds: [8000, 4500, 3000, 1500, 6750, 3500, 1200, 15, 6000],
            weights: [35, 25, 15, 10, 10, 5, 10],
            limits: [16384, 16384, 16_777_216],
            lut_local: Lut::identity(),
            lut_geometry: Lut::identity(),
            lut_diversity: Lut::neutral(),
            lut_dct: Lut::identity(),
            lut_shape: Lut::identity(),
            lut_topology: Lut::identity(),
            lut_runs: Lut::identity(),
            lut_palette: Lut::identity(),
            lut_silhouette: Lut::identity(),
            lut_geo_diversity: Lut::neutral(),
            ..Profile::v42_defaults()
        }
    }

    /// SPEC-004.1 Part II — CAL-003-PROPOSED: the comparator-41 default,
    /// every value derived from the 496-pair corpus (knees at measured
    /// quantiles, thresholds re-derived on the rescaled structural axis,
    /// geo-solo floor from the zero-leakage evidence).
    pub fn cal003() -> Profile {
        let mut name = [0u8; 16];
        name.copy_from_slice(b"CAL-003-PROPOSED");
        Profile {
            container: CONTAINER_V2,
            comparator: COMPARATOR_V41,
            name,
            evidence_rule: 0,
            scoring_rule: 1,
            rag_endpoint: 1,
            grid_g: 4,
            hamming_t: 8,
            confidence_at: 16,
            geo_conf_at: 16,
            geo_eps: 1600,
            geo_min_corr: 8,
            min_model_inliers: 6,
            max_models: 4,
            rep_extreme_at: 1500,
            coverage_floor: 2500,
            min_secondaries: 3,
            thresholds: [8000, 4000, 2400, 1000, 6000, 3500, 1200, 11, 6000],
            weights: [35, 25, 15, 10, 10, 5, 10],
            limits: [16384, 16384, 16_777_216],
            lut_local: Lut(vec![(0, 0), (5000, 1000), (8800, 4200), (9400, 8800), (10000, 10000)]),
            lut_geometry: Lut(vec![(0, 0), (1900, 1100), (3000, 3000), (10000, 10000)]),
            lut_diversity: Lut::neutral(),
            lut_dct: Lut::identity(),
            lut_shape: Lut::identity(),
            lut_topology: Lut::identity(),
            lut_runs: Lut(vec![(0, 0), (9100, 900), (9700, 4500), (10000, 10000)]),
            lut_palette: Lut(vec![(0, 0), (3000, 800), (6500, 4000), (10000, 10000)]),
            lut_silhouette: Lut::identity(),
            lut_geo_diversity: Lut::neutral(),
            ..Profile::v42_defaults()
        }
    }

    /// The container-1/2 reading of the container-3 scalars.  A profile that
    /// does not carry the v42 block decodes to exactly these, so every
    /// container-1/2 artefact keeps round-tripping bit for bit AND the
    /// comparator-42 code reading such a profile sees 4.1 behaviour rather
    /// than zeroes.
    fn v42_defaults() -> Profile {
        Profile {
            container: CONTAINER_VERSION,
            comparator: COMPARATOR_VERSION,
            name: [0u8; 16],
            evidence_rule: 0,
            scoring_rule: 1,
            rag_endpoint: 1,
            grid_g: 4,
            hamming_t: 8,
            confidence_at: 16,
            geo_conf_at: 16,
            geo_eps: 1600,
            geo_min_corr: 8,
            min_model_inliers: 6,
            max_models: 4,
            rep_extreme_at: 1500,
            coverage_floor: 2500,
            min_secondaries: 3,
            lowe_num: 82,
            lowe_den: 100,
            lowe_margin: 0,
            ham_max: 88,
            min_peak_members: 3,
            excl_pct: 0,
            kp_select: 0,
            scale_soft: 0,
            thresholds: [0; 9],
            weights: [0; 7],
            limits: [1, 1, 1],
            lut_local: Lut::identity(),
            lut_geometry: Lut::identity(),
            lut_diversity: Lut::neutral(),
            lut_dct: Lut::identity(),
            lut_shape: Lut::identity(),
            lut_topology: Lut::identity(),
            lut_runs: Lut::identity(),
            lut_palette: Lut::identity(),
            lut_silhouette: Lut::identity(),
            lut_geo_diversity: Lut::neutral(),
        }
    }

    /// SPEC-004.2 Part II — CAL-004-PROPOSED, the comparator-42 default of
    /// 1.0–1.1 (from 1.2, CAL-007 below: this with one bar moved).
    ///
    /// Every DECISION constant is CAL-003's, unchanged.  That is the claim
    /// 4.2 is making: a larger budget changes how well the pair is measured,
    /// not what a copy is.
    ///
    /// The two count-valued knobs were the temptation.  `geo_conf_at` and the
    /// geometry-solo inlier floor are absolute counts, and 512 keypoints
    /// plainly put more inliers through them, so doubling both looked
    /// obligatory — the argument being that a control saturating at SCALE
    /// zeroes the margin by construction (`chance_correct`).  That argument
    /// was MEASURED rather than assumed, and it does not hold: on 512-keypoint
    /// works the GN control reaches roughly 3125 of 10000, nowhere near the
    /// saturation point, while doubling `geo_conf_at` halves the reading on
    /// every small work and doubling the solo floor turned a genuine crop from
    /// Copy into Suspected at 128x128.  A floor that only large works can clear
    /// is a size-dependent bias, which is the defect class this family has
    /// already paid for once (v2 indexing its pyramid by width).
    ///
    /// The guard against a repetitive work manufacturing inliers at 512 is
    /// therefore §8 diversity, which measures the problem directly, and not a
    /// raised saturation point, which only measures it by proxy.
    ///
    /// PROPOSED because the corpus has not been re-hashed at 512 yet; §19 must
    /// re-derive the geometric side on 4.2 wires before the word comes off.
    pub fn cal004() -> Profile {
        let mut name = [0u8; 16];
        name.copy_from_slice(b"CAL-004-PROPOSED");
        Profile {
            container: CONTAINER_V3,
            comparator: COMPARATOR_V42,
            name,
            evidence_rule: 0,
            scoring_rule: 1,
            rag_endpoint: 1,
            grid_g: 4,
            hamming_t: 8,
            confidence_at: 16,
            geo_conf_at: 16,
            geo_eps: 1600,
            geo_min_corr: 8,
            min_model_inliers: 6,
            max_models: 4,
            rep_extreme_at: 1500,
            coverage_floor: 2500,
            min_secondaries: 3,
            lowe_num: 82,
            lowe_den: 100,
            lowe_margin: 6,
            ham_max: 88,
            min_peak_members: 3,
            excl_pct: 100,
            kp_select: 1,
            scale_soft: 1,
            thresholds: [8000, 4000, 2400, 1000, 6000, 3500, 1200, 11, 6000],
            weights: [35, 25, 15, 10, 10, 5, 10],
            limits: [16384, 16384, 16_777_216],
            lut_local: Lut(vec![(0, 0), (5000, 1000), (8800, 4200), (9400, 8800), (10000, 10000)]),
            lut_geometry: Lut(vec![(0, 0), (1900, 1100), (3000, 3000), (10000, 10000)]),
            lut_diversity: Lut::neutral(),
            lut_dct: Lut::identity(),
            lut_shape: Lut::identity(),
            lut_topology: Lut::identity(),
            lut_runs: Lut(vec![(0, 0), (9100, 900), (9700, 4500), (10000, 10000)]),
            lut_palette: Lut(vec![(0, 0), (3000, 800), (6500, 4000), (10000, 10000)]),
            lut_silhouette: Lut::identity(),
            // §8 — a floor, not a cliff.  Geometry that agrees in one corner
            // of the canvas, at one scale, under one model, on one repeated
            // texture keeps 60% of its evidence; anything spread past 4000 in
            // the diversity currency keeps all of it.
            lut_geo_diversity: Lut(vec![(0, 6000), (2000, 8000), (4000, 10000), (10000, 10000)]),
        }
    }

    /// PAPH-X 1.2 — CAL-007-PROVISIONAL, the comparator-42 default from 1.2.
    ///
    /// CAL-004 with ONE bar moved: the moderate structural bar (thresholds[2],
    /// the lattice's "partial agreement" arm for structure alone) from 2400
    /// to 3300.  Fitted on the Pixa chain's artworks (`sibench calfit`, the
    /// log in docs/calibration/CAL-007-PROVISIONAL.log): at 2400, 222 of the
    /// 14,412 pairs of two authors' works read Suspected, every one of them
    /// on structure alone with no geometric support, and the highest of them
    /// pair unrelated pictures in one pixelated style (structural 3232).
    /// The bar is the next multiple of 100 above the highest structural score
    /// a certifiable pair of two authors' works reaches.
    ///
    /// No Copy can move: every Copy arm reads the strong and solo bars, the
    /// geometric bars and the guards, none of which changed.  What moves is
    /// Suspected → Related for a pair whose only evidence is structure between
    /// 2400 and 3300 — on the chain's transformed works, 88 of 3,480, 74 of
    /// them palette shuffles, which structure alone at that level does not tell
    /// from two unrelated works in one style.  PROVISIONAL: 177 works, one
    /// snapshot; a larger population will reach past 3300 and the fit says so.
    pub fn cal007() -> Profile {
        let mut p = Profile::cal004();
        p.name.copy_from_slice(b"CAL-007-PROVISIO");
        p.thresholds[2] = 3300;
        p
    }

    /// The comparator-42 profile a caller gets by default: CAL-007-PROVISIONAL
    /// from 1.2 (1.0–1.1: CAL-004-PROPOSED, still `cal004`).
    pub fn shipped() -> Profile {
        Profile::cal007()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cal001_roundtrip_and_id() {
        let p = Profile::cal001();
        p.validate().unwrap();
        let b = p.encode();
        let q = Profile::decode(&b).unwrap();
        assert_eq!(p, q);
        assert_eq!(b, q.encode(), "one encoding per profile");
        let id = p.id_hex16();
        assert_eq!(id.len(), 16);
        println!("CAL-001-PROVISIONAL id: {} ({} bytes)", id, b.len());
    }

    #[test]
    fn cal003_matches_the_proposal_artefact() {
        let p = Profile::cal003();
        p.validate().unwrap();
        let b = p.encode();
        assert_eq!(b.len(), 270, "container-2 artefact length");
        let q = Profile::decode(&b).unwrap();
        assert_eq!(p, q);
        // the id emitted by the SPEC-004.1 proposal's own layout definition —
        // one byte of drift between the two and this line says so
        assert_eq!(
            hex(&p.id()),
            "75319777e4ff7fe6592365612cff9a85d653519166ff1308c8d3489d0247a422"
        );
        // container-1 decoders must refuse it cleanly
        let mut b1 = b.clone();
        b1[4] = 1; // claim container 1
        assert!(Profile::decode(&b1).is_err());
    }

    #[test]
    fn tampering_is_unsupported_not_creative() {
        let p = Profile::cal001();
        let mut b = p.encode();
        let orig = b.clone();
        b[0] = b'X';
        assert!(Profile::decode(&b).is_err()); // magic
        b = orig.clone();
        b[6] = 3;
        assert!(Profile::decode(&b).is_err()); // wrong comparator
        b = orig.clone();
        b.push(0);
        assert!(Profile::decode(&b).is_err()); // trailing bytes
        b = orig.clone();
        b.truncate(b.len() - 1);
        assert!(Profile::decode(&b).is_err()); // truncated lut
        // non-monotone lut
        let mut p2 = Profile::cal001();
        p2.lut_local = Lut(vec![(0, 5000), (5000, 100), (10000, 10000)]);
        assert!(p2.validate().is_err());
        // dip in x
        p2.lut_local = Lut(vec![(0, 0), (5000, 5000), (4000, 6000), (10000, 10000)]);
        assert!(p2.validate().is_err());
        // out-of-range scalar
        let mut p3 = Profile::cal001();
        p3.grid_g = 1;
        assert!(p3.validate().is_err());
        // id moves with any byte
        let mut p4 = Profile::cal001();
        p4.thresholds[1] += 1;
        assert_ne!(p4.id(), p.id());
    }

    /// CAL-007 is CAL-004 with the moderate structural bar alone moved, and
    /// docs/calibration/CAL-007-PROVISIONAL.pcal is its encoding, byte for
    /// byte (`sibench calfit` writes it).
    #[test]
    fn cal007_is_cal004_with_one_bar_moved() {
        let (a, b) = (Profile::cal004(), Profile::cal007());
        b.validate().unwrap();
        let mut c = b.clone();
        c.name = a.name;
        c.thresholds[2] = a.thresholds[2];
        assert_eq!(c, a, "only the name and thresholds[2] differ");
        assert_eq!((a.thresholds[2], b.thresholds[2]), (2400, 3300));
        let bytes = b.encode();
        assert_eq!(Profile::decode(&bytes).unwrap(), b);
        assert_eq!(&bytes[..], &include_bytes!("../../docs/calibration/CAL-007-PROVISIONAL.pcal")[..], "the committed artefact is stale");
        assert_eq!(Profile::shipped(), b);
        println!("CAL-007-PROVISIONAL id {} ({} bytes)", hex(&b.id()), bytes.len());
    }

    #[test]
    fn lut_eval_interpolates_in_currency() {
        let id = Lut::identity();
        for x in [0, 1, 4999, 5000, 9999, 10000, 12000, -3] {
            assert_eq!(id.eval(x), x.clamp(0, SCALE));
        }
        let s = Lut(vec![(0, 0), (2000, 8000), (10000, 10000)]);
        s.validate().unwrap();
        assert_eq!(s.eval(0), 0);
        assert_eq!(s.eval(1000), 4000);
        assert_eq!(s.eval(2000), 8000);
        assert_eq!(s.eval(6000), 9000);
        assert_eq!(s.eval(10000), 10000);
        // truncation, not rounding
        let t = Lut(vec![(0, 0), (3, 2), (10000, 10000)]);
        assert_eq!(t.eval(1), 0);
        assert_eq!(t.eval(2), 1);
    }
}
