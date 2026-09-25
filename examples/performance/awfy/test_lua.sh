#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../../.."

redbean="${REDBEAN:-}"
if [ -z "$redbean" ]; then
  redbean="$(command -v redbean.com || command -v redbean || true)"
fi
if [ -z "$redbean" ]; then
  echo "redbean not found (set REDBEAN or install redbean.com)" >&2
  exit 1
fi

row="$(AWFY_BENCH=smoke "$redbean" -i examples/performance/awfy/lua/main.lua)"
if ! awk -F '\t' 'NF == 5 && $1 == "lua-redbean" && $2 == "smoke" && $3 == 1 && $4 >= 0 && $5 == 6 { ok = 1 } END { exit !ok }' <<< "$row"; then
  echo "unexpected Lua smoke row: $row" >&2
  exit 1
fi

luajit="${LUAJIT:-$(command -v luajit || true)}"
if [ -z "$luajit" ]; then
  echo "luajit not found (set LUAJIT or install luajit)" >&2
  exit 1
fi

row="$(AWFY_LANG=luajit AWFY_BENCH=smoke "$luajit" examples/performance/awfy/lua/main.lua)"
if ! awk -F '\t' 'NF == 5 && $1 == "luajit" && $2 == "smoke" && $3 == 1 && $4 >= 0 && $5 == 6 { ok = 1 } END { exit !ok }' <<< "$row"; then
  echo "unexpected LuaJIT smoke row: $row" >&2
  exit 1
fi
