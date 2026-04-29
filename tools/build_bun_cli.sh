#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT="${1:-target/twk}"
BOOT_WASM="${BOOT_WASM:-$ROOT_DIR/target/boot.wasm}"
BRIDGE_WASM="${BRIDGE_WASM:-$ROOT_DIR/tools/bridge.wasm}"

if ! command -v bun >/dev/null 2>&1; then
  printf 'error: required tool not found: bun\n' >&2
  exit 1
fi

if [[ ! -f "$BOOT_WASM" ]]; then
  printf 'error: missing stage2 compiler payload: %s\n' "$BOOT_WASM" >&2
  printf 'build it with:\n  cargo build --release\n  tools/selfhost_loop.sh boot/main.tw\n' >&2
  exit 1
fi

if [[ ! -f "$BRIDGE_WASM" ]]; then
  printf 'error: missing bridge module: %s\n' "$BRIDGE_WASM" >&2
  printf 'regenerate it with:\n  ./target/release/twk run boot/tests/gen_bridge_wasm.tw\n' >&2
  exit 1
fi

BOOT_WASM="$BOOT_WASM" BRIDGE_WASM="$BRIDGE_WASM" node tools/gen_bundled_cli_payload.mjs
bun build --compile tools/twk_cli_bundled.mjs --outfile "$OUT"
printf 'Built bundled Twinkle CLI: %s\n' "$OUT"
