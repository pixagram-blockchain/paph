//! Prints the equivalence digest (see `paph::equiv`).
//!
//!     cargo run --release --features equiv --bin paph-equiv > digest.txt            wire 3
//!     cargo run --release --features equiv --bin paph-equiv -- --wire 4 > digest4.txt
fn main() {
    if std::env::args().any(|a| a == "4") {
        print!("{}", paph::equiv::digest_wire4());
    } else {
        print!("{}", paph::equiv::digest());
    }
}
