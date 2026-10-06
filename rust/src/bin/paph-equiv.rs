//! Prints the equivalence digest (see `paph::equiv`).
//!
//!     cargo run --release --features equiv --bin paph-equiv > digest.txt
fn main() {
    print!("{}", paph::equiv::digest());
}
