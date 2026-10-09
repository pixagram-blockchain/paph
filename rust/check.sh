#!/usr/bin/env bash
# The equivalence check: build paph-equiv, hash and compare the deterministic
# synthetic corpus (rust/src/equiv.rs), and diff its per-case SHA-256 digest
# against the recorded reference, line for line — once for each wire format.
#
#   rust/check.sh [reference3 [reference4]]
#       defaults: test/equiv-digest.txt (wire 3, the 1.0–1.1 file, 3,160 cases)
#                 test/equiv-digest-4.txt (wire 4, 3,164 cases)
#
# Every optimisation in this crate is held to this: a change that moves one
# byte of one output, anywhere in either digest, fails here.  Wire 4 moved no
# wire-3 output: the first file is 1.1's, unchanged.
set -euo pipefail
cd "$(dirname "$0")"
REF3="${1:-../test/equiv-digest.txt}"
REF4="${2:-../test/equiv-digest-4.txt}"
OUT="$(mktemp)"
LOG="$(mktemp)"
trap 'rm -f "$OUT" "$LOG"' EXIT
if ! cargo build --release --features equiv --bin paph-equiv >"$LOG" 2>&1; then
  grep -E "^(error|warning)" -A7 "$LOG" || cat "$LOG"
  echo "EQUIV FAIL — paph-equiv did not build"
  exit 1
fi
grep -E "^warning" -A7 "$LOG" || true
status=0
for pair in "3:$REF3" "4:$REF4"; do
  wire="${pair%%:*}"; ref="${pair#*:}"
  if [ "$wire" = 4 ]; then ./target/release/paph-equiv 4 > "$OUT"; else ./target/release/paph-equiv > "$OUT"; fi
  if diff -q "$ref" "$OUT" >/dev/null; then
    echo "EQUIV OK — wire $wire: $(wc -l < "$ref") cases byte-identical"
  else
    echo "EQUIV FAIL — wire $wire"
    diff "$ref" "$OUT" | head -20
    status=1
  fi
done
exit $status
