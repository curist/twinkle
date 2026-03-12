#!/usr/bin/env bash
# Run Twinkle test suites in interpreter and (optionally) Wasm mode.
# Usage:
#   ./scripts/test-suites.sh          # both modes
#   ./scripts/test-suites.sh -i       # interpreter only
#   ./scripts/test-suites.sh -w       # wasm only
#   TWK_TEST_FILTER='vector' ./scripts/test-suites.sh

set -euo pipefail

MAIN=boot/tests/main.tw
MODE="${1:-both}"

run_interp() {
  echo "=== Interpreter mode ==="
  cargo run --quiet -- run -i "$MAIN"
}

run_wasm() {
  echo "=== Wasm mode ==="
  cargo run --quiet -- run "$MAIN"
}

case "$MODE" in
  -i|interp)  run_interp ;;
  -w|wasm)    run_wasm ;;
  both|"")    run_interp && echo && run_wasm ;;
  *)          echo "Usage: $0 [-i | -w | both]" >&2; exit 1 ;;
esac
