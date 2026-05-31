#!/usr/bin/env python3
"""Run vector microbenchmarks and fail on vector runtime regressions.

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

BULK_EXTEND_MAX_TAIL_RATIO = 3.25
BULK_EXTEND_MAX_NORMALIZED_APPEND_RATIO = 0.75


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


def normalized_tail_ms_per_item(rows: list[tuple[int, float]]) -> float:
    n, ms = rows[-1]
    return ms / n


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

    bulk_rows = run_bench("builder_extend", args.timeout)
    append_rows = run_bench("concat_append", args.timeout)
    bulk_ratios = tail_ratios(bulk_rows)
    bulk_worst = max(bulk_ratios)
    bulk_scale_ok = bulk_worst <= BULK_EXTEND_MAX_TAIL_RATIO
    bulk_cost = normalized_tail_ms_per_item(bulk_rows)
    append_cost = normalized_tail_ms_per_item(append_rows)
    normalized_ratio = bulk_cost / append_cost if append_cost > 0 else float("inf")
    bulk_constant_ok = normalized_ratio <= BULK_EXTEND_MAX_NORMALIZED_APPEND_RATIO
    status = "ok" if bulk_scale_ok and bulk_constant_ok else "FAIL"
    print(
        "builder_extend: "
        f"tail ratios {', '.join(f'{r:.2f}x' for r in bulk_ratios)}, "
        f"normalized vs concat_append {normalized_ratio:.2f}x ({status})"
    )
    if not bulk_scale_ok:
        print(
            "  bulk builder_extend should stay sub-quadratic; "
            f"max allowed tail ratio is {BULK_EXTEND_MAX_TAIL_RATIO:.2f}x"
        )
        failed = True
    if not bulk_constant_ok:
        print(
            "  bulk builder_extend should copy leaf runs, not replay each element; "
            "normalized cost should stay comfortably below single-element concat_append "
            f"(max {BULK_EXTEND_MAX_NORMALIZED_APPEND_RATIO:.2f}x)"
        )
        failed = True

    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
