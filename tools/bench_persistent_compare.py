#!/usr/bin/env python3
"""Compare Twinkle persistent collections with Clojure.

This is a coarse cross-runtime benchmark. Each generated program measures only
its hot loop, not compiler/runtime startup. Results are still machine-relative:
use the ratios and scaling behavior, not absolute milliseconds, as the signal.

Examples:

    python3 tools/bench_persistent_compare.py
    python3 tools/bench_persistent_compare.py --sizes 1000,5000,20000
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from string import Template

ROOT = Path(__file__).resolve().parents[1]
TWK = ROOT / "target" / "twk"

WORKLOADS = [
    "vector_append_build",
    "vector_random_get",
    "vector_persistent_set",
    "dict_assoc_build",
    "dict_random_get",
    "set_insert_build",
    "set_contains",
]


@dataclass(frozen=True)
class Row:
    runtime: str
    workload: str
    n: int
    ms: float
    sink: str


def parse_rows(output: str) -> list[Row]:
    rows: list[Row] = []
    for line in output.splitlines():
        parts = line.strip().split("\t")
        if len(parts) != 5 or parts[0] == "runtime":
            continue
        runtime, workload, n, ms, sink = parts
        rows.append(Row(runtime, workload, int(n), float(ms), sink))
    return rows


def run(cmd: list[str], cwd: Path, timeout: int) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=timeout,
        check=False,
    )


def render_twinkle(sizes: list[int]) -> str:
    sizes_expr = ", ".join(str(n) for n in sizes)
    return Template(
        r'''use @std.date

fn emit(workload: String, n: Int, ms: Float, sink: Int) {
  println("twinkle\t${workload}\t${n}\t${ms}\t${sink}")
}

fn vector_append_build(n: Int) {
  t0 := date.now()
  v: Vector<Int> = []
  for i in range(n) {
    v = .append(i)
  }
  dt := date.now() - t0
  emit("vector_append_build", n, dt, v.len())
}

fn build_vector(n: Int) Vector<Int> {
  collect i in range(n) {
    i
  }
}

fn vector_random_get(n: Int) {
  v := build_vector(n)
  idx := 0
  sum := 0
  t0 := date.now()
  for _ in range(n) {
    idx = (idx + 40503) % n
    sum = sum + v[idx]
  }
  dt := date.now() - t0
  emit("vector_random_get", n, dt, sum)
}

fn vector_persistent_set(n: Int) {
  v := build_vector(n)
  idx := 0
  t0 := date.now()
  for i in range(n) {
    idx = (idx + 40503) % n
    v[idx] = i
  }
  dt := date.now() - t0
  emit("vector_persistent_set", n, dt, v.len())
}

fn dict_assoc_build(n: Int) {
  d: Dict<Int, Int> = Dict.new()
  t0 := date.now()
  for i in range(n) {
    d[i] = i
  }
  dt := date.now() - t0
  emit("dict_assoc_build", n, dt, d.len())
}

fn build_dict(n: Int) Dict<Int, Int> {
  d: Dict<Int, Int> = Dict.new()
  for i in range(n) {
    d[i] = i
  }
  d
}

fn dict_random_get(n: Int) {
  d := build_dict(n)
  idx := 0
  sum := 0
  t0 := date.now()
  for _ in range(n) {
    idx = (idx + 40503) % n
    sum = sum + case d.get(idx) { .Some(v) => v, .None => 0 }
  }
  dt := date.now() - t0
  emit("dict_random_get", n, dt, sum)
}

fn set_insert_build(n: Int) {
  s: Set<Int> = Set.new()
  t0 := date.now()
  for i in range(n) {
    s = s.insert(i)
  }
  dt := date.now() - t0
  emit("set_insert_build", n, dt, s.len())
}

fn build_set(n: Int) Set<Int> {
  s: Set<Int> = Set.new()
  for i in range(n) {
    s = s.insert(i)
  }
  s
}

fn set_contains(n: Int) {
  s := build_set(n)
  idx := 0
  hits := 0
  t0 := date.now()
  for _ in range(n) {
    idx = (idx + 40503) % n
    if s.contains(idx) {
      hits = hits + 1
    }
  }
  dt := date.now() - t0
  emit("set_contains", n, dt, hits)
}

println("runtime\tworkload\tN\tms\tsink")
for n in [$sizes] {
  vector_append_build(n)
  vector_random_get(n)
  vector_persistent_set(n)
  dict_assoc_build(n)
  dict_random_get(n)
  set_insert_build(n)
  set_contains(n)
}
'''
    ).safe_substitute(sizes=sizes_expr)


def render_clojure(sizes: list[int]) -> str:
    sizes_expr = " ".join(str(n) for n in sizes)
    return Template(
        r'''(set! *warn-on-reflection* true)

(def sizes '($sizes))

(defn now-ms [] (/ (System/nanoTime) 1000000.0))
(defn emit [workload n ms sink]
  (println (str "clojure\t" workload "\t" n "\t" ms "\t" sink)))

(defn vector-append-build [n]
  (let [t0 (now-ms)
        v (loop [i 0 v []]
            (if (< i n) (recur (inc i) (conj v i)) v))]
    (emit "vector_append_build" n (- (now-ms) t0) (count v))))

(defn build-vector [n]
  (loop [i 0 v []]
    (if (< i n) (recur (inc i) (conj v i)) v)))

(defn vector-random-get [n]
  (let [v (build-vector n)
        t0 (now-ms)
        sum (loop [i 0 idx 0 sum 0]
              (if (< i n)
                (let [idx (rem (+ idx 40503) n)]
                  (recur (inc i) idx (+ sum (nth v idx))))
                sum))]
    (emit "vector_random_get" n (- (now-ms) t0) sum)))

(defn vector-persistent-set [n]
  (let [v0 (build-vector n)
        t0 (now-ms)
        v (loop [i 0 idx 0 v v0]
            (if (< i n)
              (let [idx (rem (+ idx 40503) n)]
                (recur (inc i) idx (assoc v idx i)))
              v))]
    (emit "vector_persistent_set" n (- (now-ms) t0) (count v))))

(defn dict-assoc-build [n]
  (let [t0 (now-ms)
        m (loop [i 0 m {}]
            (if (< i n) (recur (inc i) (assoc m i i)) m))]
    (emit "dict_assoc_build" n (- (now-ms) t0) (count m))))

(defn build-map [n]
  (loop [i 0 m {}]
    (if (< i n) (recur (inc i) (assoc m i i)) m)))

(defn dict-random-get [n]
  (let [m (build-map n)
        t0 (now-ms)
        sum (loop [i 0 idx 0 sum 0]
              (if (< i n)
                (let [idx (rem (+ idx 40503) n)]
                  (recur (inc i) idx (+ sum (get m idx 0))))
                sum))]
    (emit "dict_random_get" n (- (now-ms) t0) sum)))

(defn set-insert-build [n]
  (let [t0 (now-ms)
        s (loop [i 0 s #{}]
            (if (< i n) (recur (inc i) (conj s i)) s))]
    (emit "set_insert_build" n (- (now-ms) t0) (count s))))

(defn build-set [n]
  (loop [i 0 s #{}]
    (if (< i n) (recur (inc i) (conj s i)) s)))

(defn set-contains [n]
  (let [s (build-set n)
        t0 (now-ms)
        hits (loop [i 0 idx 0 hits 0]
               (if (< i n)
                 (let [idx (rem (+ idx 40503) n)]
                   (recur (inc i) idx (if (contains? s idx) (inc hits) hits)))
                 hits))]
    (emit "set_contains" n (- (now-ms) t0) hits)))

(println "runtime\tworkload\tN\tms\tsink")
(doseq [n sizes]
  (vector-append-build n)
  (vector-random-get n)
  (vector-persistent-set n)
  (dict-assoc-build n)
  (dict-random-get n)
  (set-insert-build n)
  (set-contains n))
'''
    ).safe_substitute(sizes=sizes_expr)


def print_table(rows: list[Row]) -> None:
    by_key: dict[tuple[str, int], dict[str, Row]] = {}
    runtimes: list[str] = []
    for row in rows:
        if row.runtime not in runtimes:
            runtimes.append(row.runtime)
        by_key.setdefault((row.workload, row.n), {})[row.runtime] = row

    print("\nsummary (ms; lower is better)")
    header = ["workload", "N", *runtimes]
    widths = [max(len(h), 8) for h in header]
    rendered: list[list[str]] = []
    for workload in WORKLOADS:
        ns = sorted(n for (w, n) in by_key if w == workload)
        for n in ns:
            runtime_rows = by_key[(workload, n)]
            cells = [workload, str(n)]
            for runtime in runtimes:
                row = runtime_rows.get(runtime)
                cells.append("-" if row is None else f"{row.ms:.3f}")
            rendered.append(cells)
            widths = [max(w, len(c)) for w, c in zip(widths, cells)]

    print("  ".join(h.ljust(w) for h, w in zip(header, widths)))
    print("  ".join("-" * w for w in widths))
    for cells in rendered:
        print("  ".join(c.ljust(w) for c, w in zip(cells, widths)))


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sizes", default="1000,5000,10000", help="comma-separated N values")
    parser.add_argument("--timeout", type=int, default=180, help="per-runtime timeout in seconds")
    parser.add_argument("--keep-temp", action="store_true", help="keep generated benchmark files")
    args = parser.parse_args()

    sizes = [int(part) for part in args.sizes.split(",") if part.strip()]
    temp_obj = tempfile.TemporaryDirectory(prefix="twinkle-clojure-bench-")
    temp = Path(temp_obj.name)
    rows: list[Row] = []

    try:
        if not TWK.exists():
            print("skip twinkle: target/twk not found; run `make bundle-cli` first", file=sys.stderr)
        else:
            path = temp / "bench.tw"
            path.write_text(render_twinkle(sizes))
            proc = run([str(TWK), "run", str(path)], ROOT, args.timeout)
            if proc.returncode != 0:
                sys.stderr.write(proc.stdout)
                sys.stderr.write(proc.stderr)
                print("twinkle benchmark failed", file=sys.stderr)
            else:
                print(proc.stdout, end="")
                rows.extend(parse_rows(proc.stdout))

        clojure = shutil.which("clojure")
        if clojure is None:
            print("skip clojure: `clojure` not found on PATH", file=sys.stderr)
        else:
            path = temp / "bench.clj"
            path.write_text(render_clojure(sizes))
            proc = run([clojure, str(path)], ROOT, args.timeout)
            if proc.returncode != 0:
                sys.stderr.write(proc.stdout)
                sys.stderr.write(proc.stderr)
                print("clojure benchmark failed", file=sys.stderr)
            else:
                print(proc.stdout, end="")
                rows.extend(parse_rows(proc.stdout))

        if rows:
            print_table(rows)
        else:
            return 1
    finally:
        if args.keep_temp:
            print(f"kept generated benchmark files in {temp}", file=sys.stderr)
        else:
            temp_obj.cleanup()

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
