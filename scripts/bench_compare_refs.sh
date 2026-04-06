#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 ]]; then
  cat <<'EOF'
Usage: scripts/bench_compare_refs.sh <before-ref> <after-ref> [runs]

Benchmarks the same Twinkle workloads against two git refs without keeping
an old runtime path in-tree. The script uses temporary git worktrees, builds
both refs in release mode, then reports median wall time for each benchmark.

Examples:
  scripts/bench_compare_refs.sh 7eafa07 HEAD
  scripts/bench_compare_refs.sh HEAD~1 HEAD 7
EOF
  exit 1
fi

before_ref=$1
after_ref=$2
runs=${3:-5}
repo_root=$(git rev-parse --show-toplevel)
workdir=$(mktemp -d "${TMPDIR:-/tmp}/twk-bench.XXXXXX")
before_dir="$workdir/before"
after_dir="$workdir/after"
bench_source_dir="$repo_root/benches/tw"
bench_rel_dir=".bench-inputs"
benchmark_names=(
  "vector_append_chain.tw"
  "vector_collect_sum.tw"
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

benchmarks=()
for name in "${benchmark_names[@]}"; do
  benchmarks+=("$bench_rel_dir/$name")
done

echo "==> building release binaries"
( cd "$before_dir" && cargo build --release >/dev/null )
( cd "$after_dir" && cargo build --release >/dev/null )

before_bin="$before_dir/target/release/twk"
after_bin="$after_dir/target/release/twk"

python3 - "$before_dir" "$after_dir" "$before_bin" "$after_bin" "$runs" "${benchmarks[@]}" <<'PY'
import statistics
import subprocess
import sys
import time
from pathlib import Path

before_dir = Path(sys.argv[1])
after_dir = Path(sys.argv[2])
before_bin = Path(sys.argv[3])
after_bin = Path(sys.argv[4])
runs = int(sys.argv[5])
benchmarks = [Path(x) for x in sys.argv[6:]]


def run_once(bin_path: Path, cwd: Path, bench_rel: Path) -> float:
    start = time.perf_counter()
    proc = subprocess.run(
        [str(bin_path), "run", str(bench_rel)],
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    elapsed = time.perf_counter() - start
    if proc.returncode != 0:
        sys.stderr.write(proc.stdout)
        sys.stderr.write(proc.stderr)
        raise SystemExit(f"benchmark failed: {bench_rel} with {bin_path}")
    return elapsed


def bench(bin_path: Path, cwd: Path, bench_rel: Path, runs: int):
    warmup = run_once(bin_path, cwd, bench_rel)
    samples = [run_once(bin_path, cwd, bench_rel) for _ in range(runs)]
    return {
        "warmup": warmup,
        "median": statistics.median(samples),
        "min": min(samples),
        "max": max(samples),
        "samples": samples,
    }


print()
print(f"refs: before={before_dir} after={after_dir} runs={runs}")
print()
print(f"{'benchmark':34} {'before(s)':>10} {'after(s)':>10} {'delta':>9} {'speedup':>9}")
print("-" * 78)

for bench_rel in benchmarks:
    before = bench(before_bin, before_dir, bench_rel, runs)
    after = bench(after_bin, after_dir, bench_rel, runs)
    delta = after['median'] - before['median']
    speedup = before['median'] / after['median'] if after['median'] > 0 else float('inf')
    print(
        f"{bench_rel.name:34} "
        f"{before['median']:10.4f} "
        f"{after['median']:10.4f} "
        f"{delta:+9.4f} "
        f"{speedup:9.2f}x"
    )

print()
print("Notes:")
print("- Results are median wall-clock time across repeated process runs.")
print("- 'speedup' > 1 means the after-ref is faster.")
print("- The workloads live in benches/tw and can be edited without keeping")
print("  old runtime codepaths around just for benchmarking.")
PY
