#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 ]]; then
  cat <<'EOF'
Usage: scripts/bench_exec_compare_refs.sh <before-ref> <after-ref> [runs]

Compares two git refs by building each ref once, compiling each benchmark to
Wasm once inside a dedicated bench_exec binary, and timing only repeated Wasm
execution.

Examples:
  scripts/bench_exec_compare_refs.sh 795d1c8 HEAD
  scripts/bench_exec_compare_refs.sh 795d1c8 HEAD 15
EOF
  exit 1
fi

before_ref=$1
after_ref=$2
runs=${3:-10}
repo_root=$(git rev-parse --show-toplevel)
workdir=$(mktemp -d "${TMPDIR:-/tmp}/twk-bench-exec.XXXXXX")
before_dir="$workdir/before"
after_dir="$workdir/after"
bench_source_dir="$repo_root/benches/tw"
bench_driver_source="$repo_root/src/bin/bench_exec.rs"
bench_rel_dir=".bench-inputs"
benchmark_names=(
  "vector_append_chain.tw"
  "vector_append_indirect.tw"
  "vector_collect_sum.tw"
  "vector_iter_sum.tw"
  "vector_get_sum.tw"
  "vector_set_chain.tw"
)

cleanup() {
  set +e
  git -C "$repo_root" worktree remove --force "$before_dir" >/dev/null 2>&1
  git -C "$repo_root" worktree remove --force "$after_dir" >/dev/null 2>&1
  rm -rf "$workdir"
}
trap cleanup EXIT

echo "==> creating worktrees"
git -C "$repo_root" worktree add --detach "$before_dir" "$before_ref" >/dev/null
git -C "$repo_root" worktree add --detach "$after_dir" "$after_ref" >/dev/null

echo "==> copying benchmark inputs"
mkdir -p "$before_dir/$bench_rel_dir" "$after_dir/$bench_rel_dir"
for name in "${benchmark_names[@]}"; do
  cp "$bench_source_dir/$name" "$before_dir/$bench_rel_dir/$name"
  cp "$bench_source_dir/$name" "$after_dir/$bench_rel_dir/$name"
done

mkdir -p "$before_dir/src/bin" "$after_dir/src/bin"
cp "$bench_driver_source" "$before_dir/src/bin/bench_exec.rs"
cp "$bench_driver_source" "$after_dir/src/bin/bench_exec.rs"

benchmarks=()
for name in "${benchmark_names[@]}"; do
  benchmarks+=("$bench_rel_dir/$name")
done

echo "==> building release bench_exec binaries"
( cd "$before_dir" && cargo build --release --bin bench_exec >/dev/null )
( cd "$after_dir" && cargo build --release --bin bench_exec >/dev/null )

before_bin="$before_dir/target/release/bench_exec"
after_bin="$after_dir/target/release/bench_exec"

python3 - "$before_dir" "$after_dir" "$before_bin" "$after_bin" "$runs" "${benchmarks[@]}" <<'PY'
import subprocess
import sys
from pathlib import Path

before_dir = Path(sys.argv[1])
after_dir = Path(sys.argv[2])
before_bin = Path(sys.argv[3])
after_bin = Path(sys.argv[4])
runs = int(sys.argv[5])
benchmarks = [Path(x) for x in sys.argv[6:]]


def run_exec_bench(bin_path: Path, cwd: Path, bench_rel: Path, runs: int):
    proc = subprocess.run(
        [str(bin_path), str(bench_rel), "--runs", str(runs), "--warmup", "1"],
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    if proc.returncode != 0:
        sys.stderr.write(proc.stdout)
        sys.stderr.write(proc.stderr)
        raise SystemExit(f"benchmark failed: {bench_rel} with {bin_path}")

    data = {}
    for line in proc.stdout.splitlines():
        if "=" in line:
            k, v = line.split("=", 1)
            data[k.strip()] = v.strip()
    return {
        "median": float(data["median_seconds"]),
        "min": float(data["min_seconds"]),
        "max": float(data["max_seconds"]),
        "samples": data["samples_seconds"],
    }


print()
print(f"refs: before={before_dir} after={after_dir} runs={runs}")
print()
print(f"{'benchmark':34} {'before(s)':>10} {'after(s)':>10} {'delta':>9} {'speedup':>9}")
print("-" * 78)

for bench_rel in benchmarks:
    before = run_exec_bench(before_bin, before_dir, bench_rel, runs)
    after = run_exec_bench(after_bin, after_dir, bench_rel, runs)
    delta = after['median'] - before['median']
    speedup = before['median'] / after['median'] if after['median'] > 0 else float('inf')
    print(
        f"{bench_rel.name:34} "
        f"{before['median']:10.6f} "
        f"{after['median']:10.6f} "
        f"{delta:+9.6f} "
        f"{speedup:9.2f}x"
    )

print()
print("Notes:")
print("- This script times execution only: each benchmark is compiled to Wasm once,")
print("  then the already-built module is executed repeatedly inside bench_exec.")
print("- 'speedup' > 1 means the after-ref is faster.")
PY
