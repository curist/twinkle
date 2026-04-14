#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

BOOT_WASM="${BOOT_WASM:-/tmp/boot.wasm}"
ENTRY="${1:-boot/main.tw}"
OUT="/tmp/built.wasm"

node tools/run_wasm_node.mjs "$BOOT_WASM" -- build "$ENTRY" -o "$OUT"
node tools/run_wasm_node.mjs "$OUT"

