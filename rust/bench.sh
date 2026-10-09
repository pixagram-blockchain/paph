#!/usr/bin/env bash
# Native timing table: hash (wire 4, and wire 3 beside it) and compare over the
# bench corpus, best and median of five batches each (rust/src/bin/prof.rs).
#
#   rust/bench.sh [label]
#
# The corpus is generated on first use by tools/gen-corpus.mjs (Node).
set -uo pipefail
cd "$(dirname "$0")"
C=target/bench-corpus
[ -f "$C/scene-1024x768.rgba" ] || node ../tools/gen-corpus.mjs "$C" > /dev/null
cargo build --release --bin prof 2>&1 | grep -E "^error" -A7
P=./target/release/prof
echo "== ${1:-run}"
for f in work-512x384:3 scene-1024x768:1 scene:5 sprite:20 sprite-up4:20 banner:5 tile:10; do
  n=${f%%:*}; it=${f##*:}; printf "hash %-16s %s\n" "$n" "$($P h $it $C/$n.rgba)"
  printf "  wire 3         %s\n" "$($P h3 $it $C/$n.rgba)"; done
for p in "work-512x384 work-512x384 5" "scene scene-mirror 10" "sprite sprite-pastecrop 20" "sprite scene 20" "banner tile 20"; do
  set -- $p; printf "cmp  %-30s %s\n" "$1|$2" "$($P c $3 $C/$1.rgba $C/$2.rgba)"; done
printf "scr  %-30s %s\n" "scene|scene-mirror" "$($P s 20 $C/scene.rgba $C/scene-mirror.rgba)"
