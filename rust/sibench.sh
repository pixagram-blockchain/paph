#!/usr/bin/env bash
# The PAPH-SI harness (rust/src/bin/sibench.rs): hash the synthetic corpus on
# first use (120 bases × 21 variants + 8,000 distractors, about 20 s on two
# cores), then the evaluation — stability matrix, the proposal's designs against
# SI, the funnel with the exact keys and XRank, the index timings.
#
#   rust/sibench.sh            evaluation on the 8k corpus (≈ 2 min, XRank on every query),
#                              under SI2, the synthetic fit SPEC-SI's synthetic tables were printed with
#   rust/sibench.sh --big      also the scaling table: 100,000 more distractors (hashed once, ≈ 4 min)
#   rust/sibench.sh --fit      re-fit SI2 from the corpus first (writes docs/calibration/SI2-PROVISIONAL.psi;
#                              `sibench fit --x1` writes 1.1.0's SI1)
#   rust/sibench.sh --chain    the Pixa chain's artworks instead (SPEC-SI §9.7): fetch a snapshot when
#                              target/chain-corpus holds none (`node tools/chain-corpus.mjs`; `npm install` first
#                              for its decoders; delete the directory for a new snapshot), hash the real-base
#                              corpus, every pair of real works (`sibench chain`), the held-out fit (`sibench
#                              chainfit`; its fit on every base, named SI-CHAIN, goes to
#                              target/si-corpus/SI-chain.psi — `sibench chainfit --corpus chain.bin` alone
#                              writes docs/calibration/SI3-PROVISIONAL.psi) and the funnel under the shipped SI3
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --bin sibench 2>&1 | grep -E "^(error|warning)" -A7 || true
B=./target/release/sibench
C=target/si-corpus
for a in "$@"; do
  if [ "$a" = "--chain" ]; then
    H=target/chain-corpus
    [ -f "$H/index.tsv" ] || node ../tools/chain-corpus.mjs --out "$H"
    [ "$C/chain.bin" -nt "$H/index.tsv" ] || $B corpus --chain "$H"
    $B chain --chain "$H"
    $B chainfit --corpus chain.bin --out "$C/SI-chain.psi" --name SI-CHAIN
    $B eval --corpus chain.bin --profile ../docs/calibration/SI3-PROVISIONAL.psi
    exit 0
  fi
done
[ -f "$C/corpus.bin" ] || $B corpus
EXTRA=()
for a in "$@"; do
  case "$a" in
    --fit) $B fit ;;
    --big)
      [ -f "$C/corpus-big.bin" ] || $B corpus --bases 0 --distractors 100000 --from 8000 --out corpus-big.bin
      EXTRA+=(--big "$C/corpus-big.bin") ;;
  esac
done
$B eval "${EXTRA[@]}"
