#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT_DIR="${OUT_DIR:-target/npm}"
SRC="tools/js_runtime"
BOOT_WASM="${BOOT_WASM:-target/boot.wasm}"
BRIDGE_WASM="${BRIDGE_WASM:-tools/bridge.wasm}"

if [[ ! -f "$BOOT_WASM" ]]; then
  printf 'error: missing compiler payload: %s\n' "$BOOT_WASM" >&2
  printf 'build it with:\n  make stage2\n' >&2
  exit 1
fi
if [[ ! -f "$BRIDGE_WASM" ]]; then
  printf 'error: missing bridge module: %s\n' "$BRIDGE_WASM" >&2
  exit 1
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

cp "$SRC/runtime.mjs"   "$OUT_DIR/runtime.mjs"
cp "$SRC/node_host.mjs" "$OUT_DIR/node_host.mjs"
cp "$SRC/node_main.mjs" "$OUT_DIR/node.mjs"
cp "$SRC/index.mjs"     "$OUT_DIR/index.mjs"
cp "$BOOT_WASM"         "$OUT_DIR/boot.wasm"
cp "$BRIDGE_WASM"       "$OUT_DIR/bridge.wasm"
cp tools/npm/package.json "$OUT_DIR/package.json"
cp tools/npm/README.md    "$OUT_DIR/README.md"

# Ensure the bin is executable in the published tarball.
chmod +x "$OUT_DIR/node.mjs"

VERSION="$(node -p "require('./$OUT_DIR/package.json').version")"
printf 'Staged @twinkle-lang/twinkle v%s in %s\n' "$VERSION" "$OUT_DIR"
