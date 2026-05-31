# Vector benchmark suite (RRB Gate B)

Microbenchmarks for `Vector<T>` concat/slice/get/set, written for the
[RRB-tree vector plan](../../docs/plans/rrb-vector-concat.md) **Phase 0 / Gate B**.
They (1) prove the O(n²) curves that RRB exists to fix on the *current*
(non-RRB) runtime and (2) establish the baselines RRB must beat — and must not
regress. Keep this suite as a **permanent regression guard** regardless of the
RRB outcome.

Each benchmark times only the hot loop with `@std.date` (`date.now()`), so
startup/compile time is excluded. Every loop ends in a printed `sink` value so
the optimizer cannot dead-code-eliminate the work.

## Running

```bash
target/twk run boot/bench/<name>.tw      # prints "N<TAB>ms<TAB>sink" rows
make bench                               # run every benchmark
make bench-guard                         # fail if concat/slice tail ratios look quadratic
```

These are standalone programs read from disk — editing them needs **no**
`make bundle-cli`/`make stage2` rebuild (unlike `boot/compiler/*` changes).

## What each one measures

| File | Workload | Expected (pre-RRB) | RRB target |
|---|---|---|---|
| `concat_prepend.tw` | `acc = [i].concat(acc)` (right-operand accumulator) | **quadratic** | linear-ish (O(n log n)) |
| `concat_append.tw` | `acc = acc.concat([i])` (left-base, append) | linear (control) | unchanged |
| `concat_balanced.tw` | pairwise/tree concat of n/8 eight-elem chunks | linear | unchanged/faster |
| `slice_dropfirst.tw` | `acc = acc.slice(1, acc.len())` (dequeue / left-drop) | **quadratic** | linear-ish |
| `slice_droplast.tw` | `acc = acc.slice(0, acc.len()-1)` (drop-last via slice) | **quadratic** | linear-ish |
| `droplast_baseline.tw` | same drop-last via the shipped `drop_last` op | linear (reference) | n/a (already O(log n)) |
| `get_regular.tw` | N strided `get` on an append-built (regular) vector | linear total | **no regression** |
| `get_relaxed.tw` | N strided `get` on a concat-built vector (17-elem chunks) | linear total | ≤ ~1.5–2× `get_regular` |
| `set_regular.tw` | N strided persistent `set` on a regular vector | linear total | **no regression** |
| `set_relaxed.tw` | N strided persistent `set` on a concat-built vector | linear total | ≤ ~1.5–2× `set_regular` |

The `*_relaxed` fixtures concat non-power-of-2 (17-element) chunks so that, once
RRB lands, the result tree carries relaxed seam nodes; **pre-RRB they build a
fully regular tree**, so today they are simply a second regular baseline and read
nearly identically to their `*_regular` twin (confirmed below).

## Baseline results — current non-RRB runtime (2026-05-30)

Single-run, `target/twk run`, Apple Silicon / Deno. Treat absolute ms as
machine-relative; the **per-doubling ratio** is the signal. `~×` = factor vs the
previous (half-size) N.

### Quadratic curves — the cases RRB must fix (each doubling ≈ 4×)

```
concat_prepend     1k 3.63   2k 9.73   4k 42.4   8k 165.9   16k 736.1     ms
   ratio                ×2.7      ×4.4      ×3.9       ×4.4   → quadratic
slice_dropfirst    1k 2.18   2k 5.04   4k 19.2   8k 83.9    16k 317.7     ms
   ratio                ×2.3      ×3.8      ×4.4       ×3.8   → quadratic
slice_droplast     1k 2.20   2k 5.16   4k 20.2   8k 82.8    16k 311.9     ms
   ratio                ×2.3      ×3.9      ×4.1       ×3.8   → quadratic
```

### Linear controls — must stay linear (each doubling ≈ 2×)

```
concat_append      1k 0.17   2k 0.18   4k 0.54   8k 1.08   16k 1.70   32k 2.72  ms
concat_balanced    1k 0.16   2k 0.17   4k 0.33   8k 0.70   16k 1.45   32k 2.33  ms
droplast_baseline  1k 0.08   2k 0.07   4k 0.14   8k 0.40   16k 0.78   32k 1.57  ms
```

`droplast_baseline` (the shipped `drop_last` op) does the **same** drop-last
workload as the quadratic `slice_droplast` but in linear total time — at 16k it
is ~400× faster (0.78 ms vs 312 ms). This is the O(log n) target a drop-last
loop should already be hitting, and why LIFO pop is *not* RRB's job.

### get / set — fast-path baselines RRB must not regress (each doubling ≈ 2×)

```
get_regular   1k 0.012  2k 0.024  4k 0.049  8k 0.109  16k 0.242  32k 0.532  ms
get_relaxed   1k 0.012  2k 0.025  4k 0.051  8k 0.111  16k 0.265  32k 0.492  ms
set_regular   1k 0.11   2k 0.42   4k 0.61   8k 1.40   16k 2.23   32k 3.85   ms
set_relaxed   1k 0.12   2k 0.29   4k 0.59   8k 0.75   16k 1.95   32k 3.40   ms
```

`get_relaxed ≈ get_regular` and `set_relaxed ≈ set_regular` today (concat builds
regular trees pre-RRB). After RRB, `*_relaxed` will exercise size-table
navigation; the decision criterion is that they stay within ~1.5–2× of their
regular twin while `*_regular` does not move.

## After RRB concat and structural slice

Use `make bench-guard` as the quick scaling smoke test for the two formerly
quadratic families. It runs `concat_prepend`, `slice_dropfirst`, and
`slice_droplast`, then checks only the tail doubling ratios so noisy small inputs
are ignored. The guard is intentionally separate from `make test` because
absolute timings are machine-relative.

Full `make bench` should continue to show:

- `concat_prepend`, `slice_dropfirst`, `slice_droplast`: clearly sub-quadratic
  (per-doubling trending toward ≈2× rather than ≈4×).
- `concat_append` / `concat_balanced`: no regression beyond noise.
- `get_regular` / `set_regular`: no regression (stay on the radix fast path).
- `get_relaxed` / `set_relaxed`: regression bounded to ~1.5–2× their regular twin.
