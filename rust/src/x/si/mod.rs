//! PAPH-SI — the screening index (docs/SPEC-SI-paph-si.md).
//!
//! An index that, given one query work, returns the few stored works worth
//! handing to XRank, without reading the others.  Six quantised feature
//! families, each a 16 / 256-cell hierarchy, plus the two MinHash families
//! of the XRoute banded into keys; a candidate's score is the summed
//! evidence of the families it agrees on, never a requirement that all of
//! them agree.
//!
//!   features  the six families, from the wire alone                [§3]
//!   profile   the SI profile: codebooks, weights, identity          [§6]
//!   code      cells, signatures, probe order, THE score, the scan   [§4, §5]
//!   index     the in-memory postings index (ScanCount)              [§7.1]
//!   fit       fitting a profile from a corpus                       [§6]
//!   abi       the C ABI (SI ABI 1)                                  [§8]

pub mod abi;
pub mod code;
pub mod features;
pub mod fit;
pub mod index;
pub mod profile;

pub use code::{scan, SiQuery, SiSig, SIG_BYTES};
pub use index::{SiIndex, SiStats};
pub use profile::SiProfile;
