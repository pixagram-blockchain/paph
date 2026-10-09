#!/usr/bin/env bash
# The PAPH-SI harness (rust/src/bin/sibench.rs): hash the synthetic corpus on
# first use (120 bases × 21 variants + 8,000 distractors, about 20 s on two
# cores), then the evaluation — stability matrix, the proposal's designs against
# SI, the funnel with the exact keys and XRank, the index timings — under the
# shipped X3 and SI4.
#
#   rust/sibench.sh            evaluation on the 8k corpus (≈ 2 min, XRank on every query)
#   rust/sibench.sh --big      also the scaling table: 100,000 more distractors (hashed once, ≈ 4 min)
#   rust/sibench.sh --fit      a synthetic SI fit first, bound to X3 (SI-SYNTHETIC-X3, no shipped profile,
#                              under target/si-corpus; 1.1's SI2 and 1.1.0's SI1 were fitted on wire-3
#                              hashes: `sibench fit --x2 --corpus corpus-w3.bin` reproduces SI2)
#   rust/sibench.sh --chain    the Pixa chain's artworks instead (SPEC-SI §9.7): fetch a snapshot when
#                              target/chain-corpus holds none (`node tools/chain-corpus.mjs`; `npm install` first
#                              for its decoders; delete the directory for a new snapshot), hash the real-base
#                              corpus, every pair of real works (`sibench chain`), the held-out fit (`sibench
#                              chainfit`; its fit on every base, named SI-CHAIN, goes to
#                              target/si-corpus/SI-chain.psi — `sibench chainfit` alone rewrites
#                              docs/calibration/SI4-PROVISIONAL.psi only when it reproduces it, on the
#                              snapshot it was fitted on) and the funnel under the shipped SI4
#
# Every cache is re-hashed when it holds another wire format than this
# release writes (1.2: wire 4; a 1.1 cache holds wire 3).
set -euo pipefail
cd "$(dirname "$0")"
LOG="$(mktemp)"
trap 'rm -f "$LOG"' EXIT
if ! cargo build --release --bin sibench >"$LOG" 2>&1; then
  grep -E "^error" -A7 "$LOG" || cat "$LOG"
  echo "sibench did not build" >&2
  exit 1
fi
grep -E "^warning" -A7 "$LOG" || true
B=./target/release/sibench
C=target/si-corpus
# fresh: the file exists and was hashed in this release's wire format
fresh() { [ -f "$1" ] && [ "$($B wire "$1")" = 4 ]; }
for a in "$@"; do
  if [ "$a" = "--chain" ]; then
    H=target/chain-corpus
    [ -f "$H/index.tsv" ] || node ../tools/chain-corpus.mjs --out "$H"
    { fresh "$C/chain.bin" && [ "$C/chain.bin" -nt "$H/index.tsv" ]; } || $B corpus --chain "$H"
    $B chain --chain "$H"
    $B chainfit --corpus chain.bin --out "$C/SI-chain.psi" --name SI-CHAIN
    $B eval --corpus chain.bin
    exit 0
  fi
done
fresh "$C/corpus.bin" || $B corpus
EXTRA=()
for a in "$@"; do
  case "$a" in
    --fit) $B fit ;;
    --big)
      fresh "$C/corpus-big.bin" || $B corpus --bases 0 --distractors 100000 --from 8000 --out corpus-big.bin
      EXTRA+=(--big "$C/corpus-big.bin") ;;
  esac
done
$B eval "${EXTRA[@]}"
