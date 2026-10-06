#!/usr/bin/env bash
# Build the WebAssembly modules.
#
#   wasm/paph.wasm            SIMD128 (every current browser, Node >= 16.4,
#                             Deno, Cloudflare Workers) — the shipped build
#   wasm/paph-baseline.wasm   no SIMD, for the rare runtime without it
#
# Both are the same source; the vector kernels have scalar twins the tests hold
# equal, and test/wasm-equiv.mjs proves each binary reproduces the native
# equivalence digest byte for byte.
#
#   tools/build-wasm.sh            release builds (+ wasm-opt when available)
#   tools/build-wasm.sh --equiv    also the digest-exporting test builds
#
# wasm-opt comes from the `binaryen` npm package (a devDependency) or $WASM_OPT.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root/rust"
out="$root/wasm"
mkdir -p "$out"

WASM_OPT="${WASM_OPT:-}"
if [ -z "$WASM_OPT" ]; then
  for c in "$root/node_modules/.bin/wasm-opt" "$(command -v wasm-opt || true)"; do
    if [ -n "$c" ] && [ -x "$c" ]; then WASM_OPT="$c"; break; fi
  done
fi

build() { # <name> <rustflags> <features> <dest>
  local name="$1" flags="$2" feats="$3" dest="$4"
  RUSTFLAGS="$flags" cargo build --quiet --release --target wasm32-unknown-unknown --lib \
    $feats --target-dir "target/wasm-$name"
  cp "target/wasm-$name/wasm32-unknown-unknown/release/paph.wasm" "$dest"
  if [ -n "$WASM_OPT" ]; then
    local simd=""; case "$flags" in *simd128*) simd="--enable-simd";; esac
    "$WASM_OPT" -O3 $simd --enable-bulk-memory --enable-sign-ext --enable-mutable-globals \
      --enable-nontrapping-float-to-int --enable-multivalue --enable-reference-types \
      --strip-debug --strip-producers "$dest" -o "$dest.opt"
    mv "$dest.opt" "$dest"
  fi
  printf '  %-28s %7d bytes\n' "$(basename "$dest")" "$(wc -c < "$dest")"
}

echo "wasm builds${WASM_OPT:+ (wasm-opt: $WASM_OPT)}:"
build simd     "-C target-feature=+simd128" "" "$out/paph.wasm"
build baseline ""                           "" "$out/paph-baseline.wasm"
if [ "${1:-}" = "--equiv" ]; then
  mkdir -p "$root/test/.wasm"
  build simd-equiv     "-C target-feature=+simd128" "--features equiv" "$root/test/.wasm/paph-equiv.wasm"
  build baseline-equiv ""                           "--features equiv" "$root/test/.wasm/paph-baseline-equiv.wasm"
fi
