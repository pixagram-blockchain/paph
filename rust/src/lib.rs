//! PAPH — integer-only perceptual hash for pixel-art plagiarism detection.
//! Wire: PAPH-SPEC-003 (format 3, unchanged).  Comparator: PAPH-SPEC-004.2
//! (comparator 42) with 4 and 41 frozen beside it.  Zero dependencies, on
//! purpose.
pub mod abi;
pub mod assignment;
pub mod calibration;
pub mod compare;
pub mod coverage;
#[cfg(feature = "equiv")]
pub mod equiv;
pub mod config;
pub mod front;
#[macro_use]
pub mod json;
pub mod geom42;
pub mod golden;
pub mod keypoints;
pub mod lattice;
pub mod local_v4;
pub mod multimodel;
pub mod nulls;
pub mod prepared;
pub mod report;
pub mod sha256;
pub mod synth;
pub mod v4;
pub mod v41;
pub mod v42;
pub mod sections;
pub mod simd;
pub mod tables;
pub mod wire;
pub mod x;
