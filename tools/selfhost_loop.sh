#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

BOOT_WASM="${BOOT_WASM:-/tmp/boot.wasm}"
ENTRY="${1:-boot/main.tw}"
TMP_DIR="${TMPDIR:-/tmp}/twinkle-selfhost"
STAGE2_WAT="$TMP_DIR/stage2.wat"
STAGE2_WASM="$TMP_DIR/stage2.wasm"
STAGE3_WAT="$TMP_DIR/stage3.wat"
IR_OUT="$TMP_DIR/selfhost-loop.ir"
VALIDATE_WASMTIME="${VALIDATE_WASMTIME:-0}"
BOOT_WAT_PATH="boot/main.wat"
RESTORE_BOOT_WAT="0"
ORIG_BOOT_WAT="$TMP_DIR/original-boot-main.wat"

mkdir -p "$TMP_DIR"

if [[ -f "$BOOT_WAT_PATH" ]]; then
  cp "$BOOT_WAT_PATH" "$ORIG_BOOT_WAT"
  RESTORE_BOOT_WAT="1"
fi

cleanup() {
  if [[ "$RESTORE_BOOT_WAT" == "1" ]]; then
    cp "$ORIG_BOOT_WAT" "$BOOT_WAT_PATH"
  else
    rm -f "$BOOT_WAT_PATH"
  fi
}
trap cleanup EXIT

step() {
  printf '\n==> %s\n' "$1"
}

require_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'error: required tool not found: %s\n' "$1" >&2
    exit 1
  fi
}

capture_boot_wat() {
  local dest="$1"
  if [[ ! -f "$BOOT_WAT_PATH" ]]; then
    printf 'error: expected generated %s\n' "$BOOT_WAT_PATH" >&2
    exit 1
  fi
  cp "$BOOT_WAT_PATH" "$dest"
}

require_tool node
require_tool wasm-tools

step "Build boot compiler with stage0 -> $BOOT_WASM"
./target/release/twk build "$ENTRY" -o "$BOOT_WASM"

step "Self-hosted check via $BOOT_WASM"
node tools/run_wasm_node.mjs "$BOOT_WASM" -- check "$ENTRY"

step "Self-hosted IR via $BOOT_WASM"
node tools/run_wasm_node.mjs "$BOOT_WASM" -- ir "$ENTRY" > "$IR_OUT"
printf 'IR output: %s\n' "$IR_OUT"

step "Self-hosted build via $BOOT_WASM -> $STAGE2_WAT"
node tools/run_wasm_node.mjs "$BOOT_WASM" -- build "$ENTRY"
capture_boot_wat "$STAGE2_WAT"
printf 'Stage2 WAT: %s\n' "$STAGE2_WAT"

step "Convert stage2 WAT -> $STAGE2_WASM"
wasm-tools parse "$STAGE2_WAT" -o "$STAGE2_WASM"
printf 'Stage2 WASM: %s\n' "$STAGE2_WASM"

step "Run stage2 WASM via Node -- --help"
node tools/run_wasm_node.mjs "$STAGE2_WASM" -- --help

step "Run stage2 WASM via Node -- build $ENTRY -> $STAGE3_WAT"
node tools/run_wasm_node.mjs "$STAGE2_WASM" -- build "$ENTRY"
capture_boot_wat "$STAGE3_WAT"
printf 'Stage3 WAT: %s\n' "$STAGE3_WAT"

step "Compare stage2 and stage3 WAT"
if diff -u "$STAGE2_WAT" "$STAGE3_WAT" > "$TMP_DIR/fixedpoint.diff"; then
  printf 'Fixed point reached: %s == %s\n' "$STAGE2_WAT" "$STAGE3_WAT"
else
  printf 'error: fixed point mismatch; diff: %s\n' "$TMP_DIR/fixedpoint.diff" >&2
  exit 1
fi

if [[ "$VALIDATE_WASMTIME" == "1" ]]; then
  step "Optional Wasmtime validation via twk run"
  cargo run --release -- run "$STAGE2_WAT" -- --help
  cargo run --release -- run "$STAGE2_WAT" -- build "$ENTRY"
fi

printf '\nSelf-host loop completed successfully.\n'
