#!/usr/bin/env python3
"""Run vector microbenchmarks and fail on quadratic-looking Phase 5 regressions.

This is intentionally not part of `make test`: timings are machine-relative.
Use it as a local/CI performance smoke test after vector runtime changes:

    python3 tools/check_vector_scaling.py
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TWK = ROOT / "target" / "twk"


@dataclass(frozen=True)
class BenchSpec:
    name: str
    max_tail_ratio: float
    note: str


SPECS = [
    BenchSpec("concat_prepend", 3.25, "RRB concat should keep prepend loops sub-quadratic"),
    BenchSpec("slice_dropfirst", 3.25, "structural slice should keep dequeue loops sub-quadratic"),
    BenchSpec("slice_droplast", 3.25, "structural slice should keep slice-pop loops sub-quadratic"),
]


def parse_rows(output: str) -> list[tuple[int, float]]:
    rows: list[tuple[int, float]] = []
    for line in output.splitlines():
        parts = line.strip().split()
        if len(parts) < 2 or not parts[0].isdigit():
            continue
        rows.append((int(parts[0]), float(parts[1])))
    return rows


def run_bench(name: str, timeout_s: int) -> list[tuple[int, float]]:
    proc = subprocess.run(
        [str(TWK), "run", f"boot/bench/{name}.tw"],
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout_s,
        check=False,
    )
    if proc.returncode != 0:
        sys.stderr.write(proc.stdout)
        sys.stderr.write(proc.stderr)
        raise SystemExit(f"{name}: benchmark failed with exit code {proc.returncode}")

    rows = parse_rows(proc.stdout)
    if len(rows) < 4:
        sys.stderr.write(proc.stdout)
        raise SystemExit(f"{name}: expected benchmark rows")
    return rows


def tail_ratios(rows: list[tuple[int, float]]) -> list[float]:
    ratios: list[float] = []
    for (_, prev), (_, cur) in zip(rows[-3:-1], rows[-2:]):
        if prev <= 0:
            continue
        ratios.append(cur / prev)
    return ratios


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--timeout", type=int, default=60, help="per-benchmark timeout in seconds")
    args = parser.parse_args()

    if not TWK.exists():
        raise SystemExit("target/twk not found; run `make bundle-cli` first")

    failed = False
    for spec in SPECS:
        rows = run_bench(spec.name, args.timeout)
        ratios = tail_ratios(rows)
        worst = max(ratios)
        status = "ok" if worst <= spec.max_tail_ratio else "FAIL"
        print(f"{spec.name}: tail ratios {', '.join(f'{r:.2f}x' for r in ratios)} ({status})")
        if worst > spec.max_tail_ratio:
            print(f"  {spec.note}; max allowed tail ratio is {spec.max_tail_ratio:.2f}x")
            failed = True

    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
