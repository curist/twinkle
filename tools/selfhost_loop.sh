#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

ENTRY="${1:-boot/main.tw}"
TMP_DIR="${TMPDIR:-/tmp}/twinkle-selfhost"
STAGE1_WASM="${STAGE1_WASM:-$ROOT_DIR/target/boot-stage1.wasm}"
STAGE2_WASM="${STAGE2_WASM:-${BOOT_WASM:-$ROOT_DIR/target/boot.wasm}}"
STAGE3_WASM="${STAGE3_WASM:-$TMP_DIR/stage3.wasm}"
IR_OUT="$TMP_DIR/selfhost-loop.ir"
VALIDATE_WASMTIME="${VALIDATE_WASMTIME:-0}"

mkdir -p "$TMP_DIR"
mkdir -p "$(dirname "$STAGE1_WASM")" "$(dirname "$STAGE2_WASM")" "$(dirname "$STAGE3_WASM")"

step() {
  printf '\n==> %s\n' "$1"
}

require_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'error: required tool not found: %s\n' "$1" >&2
    exit 1
  fi
}

require_tool node

step "Build bridge module for Node runner"
./target/release/twk run boot/tests/gen_bridge_wasm.tw

step "Build stage1 compiler with stage0 -> $STAGE1_WASM"
./target/release/twk build "$ENTRY" -o "$STAGE1_WASM"

step "Self-hosted check via stage1"
node tools/run_wasm_node.mjs "$STAGE1_WASM" -- check "$ENTRY"

step "Self-hosted IR via stage1"
node tools/run_wasm_node.mjs "$STAGE1_WASM" -- ir "$ENTRY" > "$IR_OUT"
printf 'IR output: %s\n' "$IR_OUT"

step "Build stage2 compiler with stage1 -> $STAGE2_WASM"
node tools/run_wasm_node.mjs "$STAGE1_WASM" -- build "$ENTRY" -o "$STAGE2_WASM"
printf 'Stage2 WASM: %s\n' "$STAGE2_WASM"

step "Run stage2 WASM via Node -- build $ENTRY -> $STAGE3_WASM"
node tools/run_wasm_node.mjs "$STAGE2_WASM" -- build "$ENTRY" -o "$STAGE3_WASM"
printf 'Stage3 WASM: %s\n' "$STAGE3_WASM"

step "Compare stage2 and stage3 WASM"
if cmp -s "$STAGE2_WASM" "$STAGE3_WASM"; then
  printf 'Fixed point reached: %s == %s\n' "$STAGE2_WASM" "$STAGE3_WASM"
else
  printf 'error: fixed point mismatch; compare files: %s %s\n' "$STAGE2_WASM" "$STAGE3_WASM" >&2
  exit 1
fi

if [[ "$VALIDATE_WASMTIME" == "1" ]]; then
  step "Optional Wasmtime validation via Node runner"
  node tools/run_wasm_node.mjs "$STAGE2_WASM" -- --help
  node tools/run_wasm_node.mjs "$STAGE2_WASM" -- build "$ENTRY" -o "$TMP_DIR/wasmtime-check.wasm"
fi

printf '\nSelf-host loop completed successfully.\n'
