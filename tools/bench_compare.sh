#!/usr/bin/env bash
# Compare Twinkle benchmark execution under Wasmtime vs Node.js (V8) vs Bun (JSC).
#
# Usage:
#   ./tools/bench_compare.sh [ROUNDS]
#
# Compiles each benchmark once, then runs it under all available runtimes
# ROUNDS times (default: 5) and reports median wall-clock time.

set -euo pipefail

ROUNDS="${1:-5}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
TWK="$PROJECT_DIR/target/release/twk"
NODE_RUNNER="$SCRIPT_DIR/run_wasm_node.mjs"
BUILD_DIR=$(mktemp -d)

BENCHMARKS=(
  vector_get_tiny
  vector_get_1024
  vector_get_1025
  vector_get_deep
  vector_get_deep_tail_only
  vector_iter_sum
)

cleanup() { rm -rf "$BUILD_DIR"; }
trap cleanup EXIT

echo "=== Twinkle Wasm GC Runtime Comparison ==="
echo "Rounds per benchmark: $ROUNDS"

# Detect available runtimes
RUNTIMES=("wasmtime")
if command -v node &>/dev/null; then
  RUNTIMES+=("node")
  echo "  Node.js:  $(node --version)"
fi
if command -v bun &>/dev/null; then
  RUNTIMES+=("bun")
  echo "  Bun:      $(bun --version)"
fi
echo "Build dir: $BUILD_DIR"
echo ""

# Ensure twk is built
echo "Building twk..."
(cd "$PROJECT_DIR" && cargo build --release --bin twk 2>&1 | tail -1)
echo "  Wasmtime: $($TWK --version 2>/dev/null || echo 'twk')"
echo ""

# Compile all benchmarks
echo "Compiling benchmarks..."
for name in "${BENCHMARKS[@]}"; do
  "$TWK" build "$PROJECT_DIR/benches/tw/${name}.tw" -o "$BUILD_DIR/${name}.wasm" 2>/dev/null
  echo "  $name.wasm"
done
echo ""

# Timing helper: runs a command N times, returns median wall-clock ms
median_time_ms() {
  local cmd=("$@")
  local times=()

  for ((i = 0; i < ROUNDS; i++)); do
    local start end elapsed
    start=$(python3 -c "import time; print(int(time.monotonic_ns()))")
    if ! "${cmd[@]}" > /dev/null 2>&1; then
      return 1
    fi
    end=$(python3 -c "import time; print(int(time.monotonic_ns()))")
    elapsed=$(( (end - start) / 1000000 ))
    times+=("$elapsed")
  done

  # Sort and pick median
  IFS=$'\n' sorted=($(sort -n <<< "${times[*]}")); unset IFS
  local mid=$(( ROUNDS / 2 ))
  echo "${sorted[$mid]}"
}

# Build header
header="%-30s %14s"
divider="%-30s %14s"
header_args=("Benchmark" "Wasmtime(ms)")
divider_args=("------------------------------" "--------------")

if [[ " ${RUNTIMES[*]} " =~ " node " ]]; then
  header+=" %12s %10s"
  divider+=" %12s %10s"
  header_args+=("Node(ms)" "N/W")
  divider_args+=("------------" "----------")
fi
if [[ " ${RUNTIMES[*]} " =~ " bun " ]]; then
  header+=" %11s %10s"
  divider+=" %11s %10s"
  header_args+=("Bun(ms)" "B/W")
  divider_args+=("------------" "----------")
fi

printf "$header\n" "${header_args[@]}"
printf "$divider\n" "${divider_args[@]}"

for name in "${BENCHMARKS[@]}"; do
  wasm="$BUILD_DIR/${name}.wasm"

  # Warm up (1 run each, discarded)
  "$TWK" run "$wasm" > /dev/null 2>&1
  for rt in "${RUNTIMES[@]}"; do
    case "$rt" in
      node) node "$NODE_RUNNER" "$wasm" > /dev/null 2>&1 ;;
      bun)  bun  "$NODE_RUNNER" "$wasm" > /dev/null 2>&1 ;;
    esac
  done

  wasmtime_ms=$(median_time_ms "$TWK" run "$wasm")

  row_fmt="%-30s %14s"
  row_args=("$name" "$wasmtime_ms")

  if [[ " ${RUNTIMES[*]} " =~ " node " ]]; then
    node_ms=$(median_time_ms node "$NODE_RUNNER" "$wasm")
    if [ "$wasmtime_ms" -gt 0 ]; then
      node_ratio=$(python3 -c "print(f'{$node_ms / $wasmtime_ms:.2f}x')")
    else
      node_ratio="N/A"
    fi
    row_fmt+=" %12s %10s"
    row_args+=("$node_ms" "$node_ratio")
  fi

  if [[ " ${RUNTIMES[*]} " =~ " bun " ]]; then
    bun_ms=$(median_time_ms bun "$NODE_RUNNER" "$wasm")
    if [ "$wasmtime_ms" -gt 0 ]; then
      bun_ratio=$(python3 -c "print(f'{$bun_ms / $wasmtime_ms:.2f}x')")
    else
      bun_ratio="N/A"
    fi
    row_fmt+=" %11s %10s"
    row_args+=("$bun_ms" "$bun_ratio")
  fi

  printf "$row_fmt\n" "${row_args[@]}"
done

echo ""
echo "Ratio < 1.0 = faster than Wasmtime;  > 1.0 = slower than Wasmtime."
echo "N/W = Node/Wasmtime ratio;  B/W = Bun/Wasmtime ratio."
echo "Note: timings include process startup + Wasm compilation + execution."
