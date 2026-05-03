#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

OUT="${1:-target/twk}"
BOOT_WASM="${BOOT_WASM:-$ROOT_DIR/target/boot.wasm}"
BRIDGE_WASM="${BRIDGE_WASM:-$ROOT_DIR/tools/bridge.wasm}"
SEA_MAIN="$ROOT_DIR/tools/twk_cli_sea.cjs"
TMP_DIR="${TMPDIR:-/tmp}/twinkle-sea-build.$$"
SEA_CONFIG="$TMP_DIR/sea-config.json"
SEA_BLOB="$TMP_DIR/twk-sea.blob"
NODE_BIN="${NODE_BIN:-$(command -v node || true)}"
POSTJECT_BIN="${POSTJECT_BIN:-}"
FUSE="NODE_SEA_FUSE_fce680ab2cc467b6e072b8b5df1996b2"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

if [[ -z "$NODE_BIN" ]]; then
  printf 'error: required tool not found: node\n' >&2
  exit 1
fi

if ! command -v npx >/dev/null 2>&1 && [[ -z "$POSTJECT_BIN" ]]; then
  printf 'error: npx is required to run postject (or set POSTJECT_BIN)\n' >&2
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

mkdir -p "$TMP_DIR" "$(dirname "$OUT")"

cat > "$SEA_CONFIG" <<JSON
{
  "main": "$SEA_MAIN",
  "output": "$SEA_BLOB",
  "disableExperimentalSEAWarning": true,
  "useCodeCache": true,
  "assets": {
    "boot.wasm": "$BOOT_WASM",
    "bridge.wasm": "$BRIDGE_WASM"
  }
}
JSON

"$NODE_BIN" --experimental-sea-config "$SEA_CONFIG"
cp "$NODE_BIN" "$OUT"
chmod +w "$OUT"

case "$(uname -s)" in
  Darwin)
    if command -v codesign >/dev/null 2>&1; then
      codesign --remove-signature "$OUT" >/dev/null 2>&1 || true
    fi
    ;;
esac

if [[ -n "$POSTJECT_BIN" ]]; then
  POSTJECT=("$POSTJECT_BIN")
else
  POSTJECT=(npx --yes postject)
fi

POSTJECT_ARGS=("$OUT" NODE_SEA_BLOB "$SEA_BLOB" --sentinel-fuse "$FUSE")
case "$(uname -s)" in
  Darwin)
    POSTJECT_ARGS+=(--macho-segment-name NODE_SEA)
    ;;
esac

"${POSTJECT[@]}" "${POSTJECT_ARGS[@]}"

case "$(uname -s)" in
  Darwin)
    if command -v codesign >/dev/null 2>&1; then
      codesign --sign - "$OUT" >/dev/null 2>&1 || true
    fi
    ;;
esac

chmod +x "$OUT"
printf 'Built Node SEA Twinkle CLI: %s\n' "$OUT"
