#!/usr/bin/env bash
set -euo pipefail

SEA_NODE_VERSION="${1:-26}"
OVERRIDE="${2:-}"
FUSE="NODE_SEA_FUSE_fce680ab2cc467b6e072b8b5df1996b2"

has_sea_fuse() {
  local bin="$1"
  [[ -x "$bin" ]] || return 1
  strings "$bin" 2>/dev/null | grep -q "$FUSE"
}

supports_build_sea() {
  local bin="$1"
  [[ -x "$bin" ]] || return 1
  "$bin" --help 2>/dev/null | grep -q -- '--build-sea'
}

if [[ -n "$OVERRIDE" ]]; then
  if has_sea_fuse "$OVERRIDE" || supports_build_sea "$OVERRIDE"; then
    printf '%s\n' "$OVERRIDE"
    exit 0
  fi
  printf 'error: SEA_NODE_BIN does not look SEA-capable: %s\n' "$OVERRIDE" >&2
  printf '       expected either sentinel %s or --build-sea support\n' "$FUSE" >&2
  exit 1
fi

SYSTEM_NODE="$(command -v node || true)"
if [[ -n "$SYSTEM_NODE" ]] && has_sea_fuse "$SYSTEM_NODE"; then
  printf '%s\n' "$SYSTEM_NODE"
  exit 0
fi

if ! command -v npx >/dev/null 2>&1; then
  printf 'error: system node is not SEA-capable and npx is unavailable\n' >&2
  if [[ -n "$SYSTEM_NODE" ]]; then
    printf '       system node: %s\n' "$SYSTEM_NODE" >&2
  fi
  exit 1
fi

NPX_NODE="$(npx --yes "node@${SEA_NODE_VERSION}" -p 'process.execPath')"
if has_sea_fuse "$NPX_NODE" || supports_build_sea "$NPX_NODE"; then
  printf '%s\n' "$NPX_NODE"
  exit 0
fi

printf 'error: neither system node nor npx node@%s is SEA-capable\n' "$SEA_NODE_VERSION" >&2
if [[ -n "$SYSTEM_NODE" ]]; then
  printf '       system node: %s\n' "$SYSTEM_NODE" >&2
fi
printf '       npx node: %s\n' "$NPX_NODE" >&2
exit 1
