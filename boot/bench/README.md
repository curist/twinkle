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
make bench-guard                         # fail if concat/slice scaling or bulk extend regresses
```

These are standalone programs read from disk — editing them needs **no**
`make bundle-cli`/`make stage2` rebuild (unlike `boot/compiler/*` changes).

## What each one measures

| File | Workload | Expected (pre-RRB) | RRB target |
|---|---|---|---|
| `concat_prepend.tw` | `acc = [i].concat(acc)` (right-operand accumulator) | **quadratic** | linear-ish (O(n log n)) |
| `concat_append.tw` | `acc = acc.concat([i])` (left-base, append) | linear (control) | unchanged |
| `concat_balanced.tw` | pairwise/tree concat of n/8 eight-elem chunks | linear | unchanged/faster |
| `builder_extend.tw` | append a reused 256-element chunk through optimized concat | linear but element-replay constant | bulk leaf-copy constant |
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

Use `make bench-guard` as the quick smoke test for vector runtime regressions.
It runs `concat_prepend`, `slice_dropfirst`, and `slice_droplast`, then checks
only the tail doubling ratios so noisy small inputs are ignored. It also runs
`builder_extend` and compares its normalized tail cost against `concat_append`;
that catches Phase 6 regressions where append-at-end concat still scales but
falls back to replaying every element instead of copying whole leaf runs. The
guard is intentionally separate from `make test` because timings are
machine-relative.

Full `make bench` should continue to show:

- `concat_prepend`, `slice_dropfirst`, `slice_droplast`: clearly sub-quadratic
  (per-doubling trending toward ≈2× rather than ≈4×).
- `concat_append` / `concat_balanced`: no regression beyond noise.
- `builder_extend`: clearly cheaper per element than single-element concat append
  when extending by reusable trie-backed chunks.
- `get_regular` / `set_regular`: no regression (stay on the radix fast path).
- `get_relaxed` / `set_relaxed`: regression bounded to ~1.5–2× their regular twin.

---

# Dict / Set benchmark suite (typed-dict Phase 0)

Microbenchmarks for `Dict<K,V>` / `Set<K>`, written for the
[typed-dict representation plan](../../docs/plans/typed-dict-representation.md)
**Phase 0**. They establish current-runtime baselines and, by ablation, split
where `set`/`get` time actually goes — the go/no-go signal for whether typed key
families (`i64` / `String`) are worth building. Same harness conventions as the
vector suite (`@std.date` hot-loop timing, printed `sink` to defeat DCE, keys
pre-materialized outside the timed region so only the dict op is measured).

## What each one measures

| File | Workload | Probe |
|---|---|---|
| `dict_int_build` | insert N small-int (i31, unboxed) keys | full `set` cost |
| `dict_int_get` | N strided `get` on prebuilt | read cost (`get_option`) |
| `dict_int_has` | N strided `has` on prebuilt | read cost (no Option) |
| `dict_int_remove` | remove all N keys (strided) | HAMT remove + tombstoned order maintenance |
| `dict_bigint_build` | insert N keys > 2³¹ (forced `BoxedInt`) | **isolates key boxing/hash cost** vs `dict_int_build` |
| `dict_str_build` / `_get` / `_has` | same shapes, `String` keys | `hash_string`+generic string-eq premium vs Int twins |
| `set_int_build` / `_contains` | `Set<Int>` (= `Dict<K,Void>`) | key-spec is a pure win for Set |
| `set_str_build` / `_contains` | `Set<String>` | String-key set path |

## Baseline results — current generic HAMT runtime (2026-06-12)

Single-run, `target/twk run`, Apple Silicon / Deno. Absolute ms is
machine-relative; the **cross-bench deltas** are the signal. Values at N=32000.

```
build (ms@32k):  int 7.13   bigint 7.12   string 7.35   set_int 6.97   set_str 7.81
read  (ms@32k):  int_has 1.16   int_get 1.46   set_int_contains 1.13
                 str_has 2.27   str_get 3.14   set_str_contains 2.57
remove(ms):      int_remove  1k 0.47   2k 0.79   4k 1.61   8k 2.66   16k 6.16   32k 17.45   → linear bulk
```

### Set-cost split (ablation conclusions)

- **Key boxing is already negligible.** `dict_bigint_build` (every key a heap
  `BoxedInt`) is within noise of `dict_int_build` (every key an unboxed i31).
  Build is dominated by HAMT node alloc + path-copy + insertion-order append,
  **not** key handling. A typed `i64` key saves ~0 on build.
- **`build − get` ≈ 5.7 ms (≈80% of build)** is allocation/order-append, only
  ~20% is hash+traversal. Key typing only touches part of that 20%.
- **String reads cost ~2× int reads** (`str_has` 2.27 vs `int_has` 1.16;
  `str_get` 3.14 vs `int_get` 1.46). This is the one clear, measurable
  key-specialization win: a direct `hash_string`+string-eq path replacing
  anyref dispatch + generic `core_eq`. String *build* stays alloc-bound (≈int).
- **`remove` is amortized O(log n) per call** after the tombstoned-order-vector
  change described in `docs/plans/archive/dict-amortized-remove.md`, making bulk remove
  linear instead of quadratic. This is orthogonal to key typing.

**Gate takeaway:** the typed-vector analogy does not transfer to Int keys —
build is alloc-bound and boxing is already cheap, so `Dict<Int,V>` specialization
is a weak first target. The measurable lever is **String-key reads** (~2× today).
The dominant structural cost is node allocation + insertion-order maintenance,
independent of key type.

---

# Heap benchmark suite (@std.heap pairing heap)

Confirms the amortized-complexity claims in `docs/API.md` for the pairing-heap
priority queue. These read from disk like the others — no rebuild needed.

## What each one measures

| File | Workload | Probe |
|------|----------|-------|
| `heap_build_drain` | `from_vector` N pseudo-random ints, then `to_vector` (heapsort), vs `Vector.sort_by` on the same data | N amortized-O(1) pushes + N O(log n) pops; sort is the O(n log n) reference |
| `heap_push_pop` | Dijkstra-style mixed loop: each pop followed by two reinsertions, so the frontier hovers near size N | steady-state amortized push/pop, the access pattern the claims target |

## Baseline results — current runtime (2026-06-15)

`heap_build_drain` (ms):

```
N        heap   sort
4000     4.1    2.0
8000    11.1    2.6
16000   24.1    5.2
32000   55.8   11.7
```

`heap_push_pop` (ms): 4000 → 1.6, 8000 → 2.7, 16000 → 4.1, 32000 → 10.3.

**Takeaways:**

- **No quadratic blowup.** Both workloads scale as n·log n — each doubling past
  the warm-up region costs ≈2.2–2.5×, confirming amortized O(1) push / O(log n)
  pop rather than per-op linear cost.
- **Heapsort is ~3–5× slower than `Vector.sort_by`** for one-shot sorting: the
  drain pays an O(log n) PVec `get` per child meld, where the native merge sort
  does not. For a pure sort, prefer `sort_by`; the heap earns its keep when
  priorities arrive incrementally or you only need the top few elements.
