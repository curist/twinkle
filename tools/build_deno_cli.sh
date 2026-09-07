#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT="${1:-target/twk}"
BOOT_WASM="${BOOT_WASM:-$ROOT_DIR/target/boot.wasm}"
DENO_BIN="${DENO_BIN:-$(command -v deno || true)}"
ASSET_DIR="$ROOT_DIR/target/deno-assets"
BOOT_ASSET="$ASSET_DIR/boot.wasm.bin"
PACKAGE_ASSET="$ASSET_DIR/package.json"
RENDERER_WASM="${RENDERER_WASM:-$ROOT_DIR/target/renderer.wasm}"
RENDERER_ASSET="$ASSET_DIR/renderer.wasm.bin"

if [[ -z "$DENO_BIN" ]]; then
  printf 'error: required tool not found: deno\n' >&2
  exit 1
fi

if [[ ! -f "$BOOT_WASM" ]]; then
  printf 'error: missing stage2 compiler payload: %s\n' "$BOOT_WASM" >&2
  printf 'build it with:\n  make stage2\n' >&2
  exit 1
fi

mkdir -p "$ASSET_DIR" "$(dirname "$OUT")"

# Deno treats included .wasm files as modules during graph construction. Copy
# the compiler payload with a data-file suffix so it is embedded as an opaque
# asset. The JS<->Wasm-GC bridge is embedded in runtime.mjs (bridge_bytes.mjs),
# so it rides along in the module graph with no separate include.
cp "$BOOT_WASM" "$BOOT_ASSET"
cp "$ROOT_DIR/tools/npm/package.json" "$PACKAGE_ASSET"

# The trap-trace renderer rides along as an opaque asset (same .bin trick as the
# boot payload) so `twk run` can render source-mapped traces. Optional: if it is
# not built, the CLI still runs and child traps propagate without a trace.
RENDERER_INCLUDE=()
if [[ -f "$RENDERER_WASM" ]]; then
  cp "$RENDERER_WASM" "$RENDERER_ASSET"
  RENDERER_INCLUDE=(--include "$RENDERER_ASSET")
else
  printf 'note: %s not found; twk run will not render trap traces\n' "$RENDERER_WASM" >&2
fi

"$DENO_BIN" compile \
  --quiet \
  --no-check \
  --allow-read \
  --allow-write \
  --allow-env \
  --include "$BOOT_ASSET" \
  --include "$PACKAGE_ASSET" \
  "${RENDERER_INCLUDE[@]}" \
  --output "$OUT" \
  "$ROOT_DIR/tools/js_runtime/deno_main.mjs"

chmod +x "$OUT"
printf 'Built Deno Twinkle CLI: %s\n' "$OUT"
