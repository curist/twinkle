#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT_DIR="${OUT_DIR:-target/npm}"
SRC="tools/js_runtime"
BOOT_WASM="${BOOT_WASM:-target/boot.wasm}"

if [[ ! -f "$BOOT_WASM" ]]; then
  printf 'error: missing compiler payload: %s\n' "$BOOT_WASM" >&2
  printf 'build it with:\n  make stage2\n' >&2
  exit 1
fi
if [[ ! -f "$SRC/bridge_bytes.mjs" ]]; then
  printf 'error: missing embedded bridge: %s\n' "$SRC/bridge_bytes.mjs" >&2
  printf 'regenerate it with:\n  node tools/generate_bridge_bytes.mjs\n' >&2
  exit 1
fi

rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

cp "$SRC/runtime.mjs"      "$OUT_DIR/runtime.mjs"
cp "$SRC/bridge_bytes.mjs" "$OUT_DIR/bridge_bytes.mjs"
cp "$SRC/node_host.mjs"    "$OUT_DIR/node_host.mjs"
cp "$SRC/web.mjs"          "$OUT_DIR/web.mjs"
cp "$SRC/node_main.mjs"    "$OUT_DIR/node.mjs"
cp "$SRC/index.mjs"        "$OUT_DIR/index.mjs"
cp "$BOOT_WASM"            "$OUT_DIR/boot.wasm"
cp tools/npm/package.json "$OUT_DIR/package.json"
cp tools/npm/README.md    "$OUT_DIR/README.md"

# Ensure the bin is executable in the published tarball.
chmod +x "$OUT_DIR/node.mjs"

VERSION="$(node -p "require('./$OUT_DIR/package.json').version")"
printf 'Staged @twinkle-lang/twinkle v%s in %s\n' "$VERSION" "$OUT_DIR"
