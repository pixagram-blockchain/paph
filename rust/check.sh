#!/usr/bin/env bash
# The equivalence check: build paph-equiv, hash and compare the deterministic
# synthetic corpus (rust/src/equiv.rs), and diff its per-case SHA-256 digest
# against the recorded reference, line for line.
#
#   rust/check.sh [reference]      default: test/equiv-digest.txt
#
# Every optimisation in this crate is held to this: a change that moves one
# byte of one output, anywhere in 3160 cases, fails here.
set -euo pipefail
cd "$(dirname "$0")"
REF="${1:-../test/equiv-digest.txt}"
OUT="$(mktemp)"
trap 'rm -f "$OUT"' EXIT
cargo build --release --features equiv --bin paph-equiv 2>&1 | grep -E "^(error|warning)" -A7 || true
./target/release/paph-equiv > "$OUT"
if diff -q "$REF" "$OUT" >/dev/null; then
  echo "EQUIV OK — $(wc -l < "$REF") cases byte-identical"
else
  echo "EQUIV FAIL"
  diff "$REF" "$OUT" | head -20
  exit 1
fi
