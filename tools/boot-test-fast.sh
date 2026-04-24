#!/usr/bin/env bash
# Fast boot compiler test runner: compile tests with the self-hosted boot
# compiler, then run the produced Wasm directly via Node.js.
#
# Usage:
#   tools/boot-test-fast.sh              # build + run
#   tools/boot-test-fast.sh --run-only   # reuse last .wasm (skip compile)

set -euo pipefail

WASM_OUT="/tmp/twinkle_boot_tests.wasm"
ENTRY="boot/tests/main.tw"

if [[ "${1:-}" != "--run-only" ]]; then
  echo ":: Compiling $ENTRY → $WASM_OUT"
  tools/twk_boot.mjs build "$ENTRY" -o "$WASM_OUT"
fi

echo ":: Running via Node.js"
node tools/run_wasm_node.mjs "$WASM_OUT"
