#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT="${1:-target/twk}"
BOOT_WASM="${BOOT_WASM:-$ROOT_DIR/target/boot.wasm}"
BRIDGE_WASM="${BRIDGE_WASM:-$ROOT_DIR/tools/bridge.wasm}"
DENO_BIN="${DENO_BIN:-$(command -v deno || true)}"
ASSET_DIR="$ROOT_DIR/target/deno-assets"
BOOT_ASSET="$ASSET_DIR/boot.wasm.bin"
BRIDGE_ASSET="$ASSET_DIR/bridge.wasm.bin"
PACKAGE_ASSET="$ASSET_DIR/package.json"

if [[ -z "$DENO_BIN" ]]; then
  printf 'error: required tool not found: deno\n' >&2
  exit 1
fi

if [[ ! -f "$BOOT_WASM" ]]; then
  printf 'error: missing stage2 compiler payload: %s\n' "$BOOT_WASM" >&2
  printf 'build it with:\n  make stage2\n' >&2
  exit 1
fi

if [[ ! -f "$BRIDGE_WASM" ]]; then
  printf 'error: missing bridge module: %s\n' "$BRIDGE_WASM" >&2
  printf 'regenerate it with:\n  ./target/release/twk run boot/tests/gen_bridge_wasm.tw\n' >&2
  exit 1
fi

mkdir -p "$ASSET_DIR" "$(dirname "$OUT")"

# Deno treats included .wasm files as modules during graph construction. Copy
# the payloads with a data-file suffix so they are embedded as opaque assets.
cp "$BOOT_WASM" "$BOOT_ASSET"
cp "$BRIDGE_WASM" "$BRIDGE_ASSET"
cp "$ROOT_DIR/tools/npm/package.json" "$PACKAGE_ASSET"

"$DENO_BIN" compile \
  --quiet \
  --no-check \
  --allow-read \
  --allow-write \
  --allow-env \
  --include "$BOOT_ASSET" \
  --include "$BRIDGE_ASSET" \
  --include "$PACKAGE_ASSET" \
  --output "$OUT" \
  "$ROOT_DIR/tools/js_runtime/deno_main.mjs"

chmod +x "$OUT"
printf 'Built Deno Twinkle CLI: %s\n' "$OUT"
