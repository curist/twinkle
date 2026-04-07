#!/usr/bin/env bash
# Fast boot compiler test runner: compile to Wasm, then run via Node.js.
# ~3s vs ~16s for the full `twk run` path.
#
# Usage:
#   tools/boot-test-fast.sh              # build + run
#   tools/boot-test-fast.sh --run-only   # reuse last .wasm (skip compile)

set -euo pipefail

WASM_OUT="/tmp/twinkle_boot_tests.wasm"
ENTRY="boot/tests/main.tw"

if [[ "${1:-}" != "--run-only" ]]; then
  echo ":: Compiling $ENTRY → $WASM_OUT"
  cargo run --release --bin twk -- build -o "$WASM_OUT" "$ENTRY"
fi

echo ":: Running via Node.js"
node tools/run_wasm_node.mjs "$WASM_OUT"
