//! PAPH — integer-only perceptual hash for pixel-art plagiarism detection.
//! Wire: PAPH-SPEC-003's layout, written as format 4 (docs/SPEC-W4-paph-
//! wire4.md: the same sections, the DCT, the shapes and the silhouette
//! sampled so a mirror or a quarter turn moves them exactly) or, on request,
//! as format 3, byte for byte 1.0–1.1's.
//! Comparator: PAPH-SPEC-004.2 (comparator 42) with 4 and 41 frozen beside
//! it.  Zero dependencies, on purpose.
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
#[cfg(test)]
mod wire4;
pub mod x;
