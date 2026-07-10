# B6 — Buffer-backed Int sort kernel Implementation Plan

> ⚠️ **REVERTED (2026-07-10, `bec1bd5c`).** This plan was fully executed (both
> phases, ~35% `order_by` win) and then reverted: it banked the win by
> special-casing a named stdlib function into the compiler (monomorphize + linker),
> which is a point solution, not the principled direction. Kept as a record. The
> reusable *findings* (read-wall decomposition; typed-parameter ABI for named
> functions doesn't exist) live in [boundary-tracklist.md](boundary-tracklist.md)
> and drive the typed-representation "Extend" path instead. Do not re-execute.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Sort a `Vector<Int>` under a custom comparator ~2.2× faster by running a stable bottom-up merge sort over an `@std.buffer` i64 buffer (typed linear-memory reads) instead of the boxed persistent-vector merge.

**Architecture:** A new pure-Twinkle `@std.sort` module holds the kernel (`ints_by` + a private `merge_run`). It copies the input into two ping-ponging i64 buffers, merges in place calling the comparator per comparison, and rebuilds a `Vector<Int>`. Phase 1 adopts it explicitly in the dataframe `order_by` path (the benchmark target); Phase 2 (stretch) makes idiomatic `xs.sort_by(cmp)` on `Vector<Int>` route to it transparently via a monomorphize swap, mirroring the existing `vector$sort_i64` recognition.

**Tech Stack:** Twinkle boot compiler (`boot/stdlib/*.tw`, `boot/compiler/monomorphize.tw`), `@std.buffer` (already in the bootstrap path — `boot/compiler/pipeline.tw:1`), `tools/generate_core_lib.py`, boot test suites (`@std.testing`), `target/twk`.

**Validated baseline (prototype, N=1M):** `sort_by` (boxed idx) ~739–773ms vs buffer kernel ~338–355ms, **0 mismatches** (stable, exact match). The prototype is ordinary Twinkle with no compiler changes.

## Global Constraints

- Treat the boot compiler in `boot/` as the primary implementation.
- After editing `.tw` files, run `target/twk fmt <files>` and `target/twk lint <entry>`.
- `make bundle-cli` (rebuilds `target/boot.wasm` + `target/twk`) is slow (minutes). Develop/validate the kernel as a standalone `target/twk run` probe FIRST (fast, uses the already-bundled `@std.buffer`); only rebundle once the module is promoted to `@std.sort`.
- The kernel manually manages memory: it MUST `free` both buffers before every return path.
- Do not claim the `order_by` win until the benchmark is re-run after `make bundle-cli`.

## File structure

- `examples/performance/sort-bench/buffer_argsort_probe.tw` — **create.** Standalone correctness-matrix + benchmark probe; the fast dev/regression artifact.
- `boot/stdlib/sort.tw` — **create.** The `@std.sort` module: `pub fn ints_by` + private `merge_run`.
- `boot/lib/module/core_lib.tw` — **regenerated** (gitignored-ish generated file) by `tools/generate_core_lib.py`; do not hand-edit.
- `boot/tests/suites/std_sort_suite.tw` — **create.** Correctness suite for `@std.sort`.
- `boot/tests/main.tw` — **modify.** Register the new suite.
- `examples/performance/dataframe/frame/table.tw` — **modify.** Route `sort_indices_by_column` through `@std.sort.ints_by`.
- `boot/compiler/monomorphize.tw` — **modify (Phase 2 only).** Add the transparent `sort_by<Int>` swap.
- `docs/plans/performance/vector/{boundary-tracklist,README}.md` — **modify.** Record the shipped win.

---

## Phase 1 — Kernel + dataframe adoption

### Task 1: Standalone correctness + benchmark probe (fast dev loop)

**Files:**
- Create: `examples/performance/sort-bench/buffer_argsort_probe.tw`

- [ ] **Step 1: Write the probe with the validated kernel + a correctness matrix**

Create `examples/performance/sort-bench/buffer_argsort_probe.tw`:

```tw
use @std.date
use @std.buffer
use @std.buffer.{I64View}

// Merge src[lo..mid) and src[mid..hi) into aux[lo..hi). Stable (left wins ties).
fn merge_run(src: I64View, aux: I64View, lo: Int, mid: Int, hi: Int, cmp: fn(Int, Int) Order) {
  i := lo
  j := mid
  k := lo

  for i < mid and j < hi {
    a := src.at(i)
    b := src.at(j)

    case cmp(a, b) {
      .Gt => {
        aux.set(k, b)
        j = j + 1
      },
      _ => {
        aux.set(k, a)
        i = i + 1
      },
    }

    k = k + 1
  }

  for i < mid {
    aux.set(k, src.at(i))
    i = i + 1
    k = k + 1
  }

  for j < hi {
    aux.set(k, src.at(j))
    j = j + 1
    k = k + 1
  }
}

// Bottom-up buffer-backed sort of the values of `xs` under `cmp`.
fn ints_by(xs: Vector<Int>, cmp: fn(Int, Int) Order) Vector<Int> {
  n := xs.len()

  if n <= 1 {
    return xs
  }

  src_buf := buffer.new(n * 8)
  aux_buf := buffer.new(n * 8)
  src := src_buf.view_i64(0, n)
  aux := aux_buf.view_i64(0, n)

  for i in range(n) {
    src.set(i, xs[i])
  }

  width := 1
  cur_src := src
  cur_aux := aux

  for width < n {
    lo := 0

    for lo < n {
      mid_raw := lo + width
      hi_raw := lo + width * 2
      mid := if mid_raw < n {
        mid_raw
      } else {
        n
      }
      hi := if hi_raw < n {
        hi_raw
      } else {
        n
      }
      merge_run(cur_src, cur_aux, lo, mid, hi, cmp)
      lo = lo + width * 2
    }

    tmp := cur_src
    cur_src = cur_aux
    cur_aux = tmp
    width = width * 2
  }

  out := collect i in range(n) {
    cur_src.at(i)
  }
  src_buf.free()
  aux_buf.free()
  out
}

fn asc(a: Int, b: Int) Order {
  Int.compare(a, b)
}

// Correctness: kernel must match the prelude sort_by exactly across shapes.
fn check(label: String, xs: Vector<Int>) Void {
  want := xs.sort_by(asc)
  got := ints_by(xs, asc)
  mism := 0

  if want.len() != got.len() {
    mism = mism + 1
  }

  for i in range(want.len()) {
    if want[i] != got[i] {
      mism = mism + 1
    }
  }

  println("${label}: mismatches=${mism} (len ${got.len()})")
}

fn seeded(n: Int, mult: Int) Vector<Int> {
  collect i in range(n) {
    i * mult % n
  }
}

// Correctness matrix.
empty: Vector<Int> = []
check("empty", empty)
check("single", [42])
check("sorted", seeded(1000, 1))
check("reverse", collect i in range(1000) { 1000 - i })
check("all-equal", collect i in range(1000) { 7 })
check("random", seeded(1000, 7919))
check("dups", collect i in range(1000) { i % 10 })

// Benchmark at N=1M (index-sort shape: cmp reads an external typed column).
fn bench(n: Int) Void {
  amounts := collect i in range(n) {
    i * 2654435761 % n
  }
  idx := collect i in range(n) {
    i
  }
  cmp := fn(a: Int, b: Int) Order { Int.compare(amounts[a], amounts[b]) }

  t0 := date.now()
  base := idx.sort_by(cmp)
  println("sort_by (boxed idx) : ${date.now() - t0}ms (${base.len()})")

  t1 := date.now()
  kern := ints_by(idx, cmp)
  println("ints_by (buffer)    : ${date.now() - t1}ms (${kern.len()})")
}

println("── bench N=1000000 ──")
bench(1000000)
```

- [ ] **Step 2: Run it — expect all mismatches=0 and a ~2× kernel speedup**

Run: `target/twk fmt examples/performance/sort-bench/buffer_argsort_probe.tw; timeout 120 target/twk run examples/performance/sort-bench/buffer_argsort_probe.tw`
Expected: every `check` line prints `mismatches=0`; `ints_by (buffer)` roughly half of `sort_by (boxed idx)` at N=1M.

- [ ] **Step 3: Commit**

```bash
git add examples/performance/sort-bench/buffer_argsort_probe.tw
git commit -m "probe: buffer-backed Int sort kernel (correctness matrix + N=1M bench)"
```

---

### Task 2: Promote the kernel to the `@std.sort` module

**Files:**
- Create: `boot/stdlib/sort.tw`

- [ ] **Step 1: Create the module (kernel only, no probe/bench code)**

Create `boot/stdlib/sort.tw`:

```tw
//! Buffer-backed sorts for primitive vectors. `ints_by` runs a stable bottom-up
//! merge sort over two ping-ponging i64 linear-memory buffers, reading typed i64
//! instead of boxed persistent-vector elements — ~2.2x faster than the generic
//! `sort_by` for `Vector<Int>` on large inputs. The comparator is called with the
//! unboxed element values.

use @std.buffer
use @std.buffer.{I64View}

// Merge src[lo..mid) and src[mid..hi) into aux[lo..hi). Stable: on a tie the
// left run's element is emitted first, matching prelude `sort_by`.
fn merge_run(src: I64View, aux: I64View, lo: Int, mid: Int, hi: Int, cmp: fn(Int, Int) Order) {
  i := lo
  j := mid
  k := lo

  for i < mid and j < hi {
    a := src.at(i)
    b := src.at(j)

    case cmp(a, b) {
      .Gt => {
        aux.set(k, b)
        j = j + 1
      },
      _ => {
        aux.set(k, a)
        i = i + 1
      },
    }

    k = k + 1
  }

  for i < mid {
    aux.set(k, src.at(i))
    i = i + 1
    k = k + 1
  }

  for j < hi {
    aux.set(k, src.at(j))
    j = j + 1
    k = k + 1
  }
}

/// Stable sort of `xs` under `cmp`, backed by linear-memory i64 buffers.
/// Equivalent in result to `xs.sort_by(cmp)`.
pub fn ints_by(xs: Vector<Int>, cmp: fn(Int, Int) Order) Vector<Int> {
  n := xs.len()

  if n <= 1 {
    return xs
  }

  src_buf := buffer.new(n * 8)
  aux_buf := buffer.new(n * 8)
  src := src_buf.view_i64(0, n)
  aux := aux_buf.view_i64(0, n)

  for i in range(n) {
    src.set(i, xs[i])
  }

  width := 1
  cur_src := src
  cur_aux := aux

  for width < n {
    lo := 0

    for lo < n {
      mid_raw := lo + width
      hi_raw := lo + width * 2
      mid := if mid_raw < n {
        mid_raw
      } else {
        n
      }
      hi := if hi_raw < n {
        hi_raw
      } else {
        n
      }
      merge_run(cur_src, cur_aux, lo, mid, hi, cmp)
      lo = lo + width * 2
    }

    tmp := cur_src
    cur_src = cur_aux
    cur_aux = tmp
    width = width * 2
  }

  out := collect i in range(n) {
    cur_src.at(i)
  }
  src_buf.free()
  aux_buf.free()
  out
}
```

- [ ] **Step 2: Format and lint**

Run: `target/twk fmt boot/stdlib/sort.tw && target/twk lint boot/stdlib/sort.tw`
Expected: no diffs from fmt on a second run; lint reports nothing actionable.

- [ ] **Step 3: Commit**

```bash
git add boot/stdlib/sort.tw
git commit -m "stdlib: add @std.sort with buffer-backed ints_by"
```

---

### Task 3: Regenerate core_lib and rebundle the CLI

**Files:**
- Regenerated: `boot/lib/module/core_lib.tw` (via `tools/generate_core_lib.py`)

- [ ] **Step 1: Regenerate the embedded stdlib and rebundle**

Run: `make bundle-cli 2>&1 | tail -5`
Expected: `generate_core_lib.py` runs (new `@std.sort` embedded), self-host loop reaches "Fixed point reached", `target/twk` rebuilds. (If it says "Nothing to be done", `touch boot/stdlib/sort.tw` and retry.)

- [ ] **Step 2: Verify `@std.sort` is resolvable through the bundled CLI**

Run: `printf 'use @std.sort\nprintln(sort.ints_by([3, 1, 2], fn(a: Int, b: Int) Order { Int.compare(a, b) }).to_string())\n' > /tmp/sort_smoke.tw && target/twk run /tmp/sort_smoke.tw`
Expected: prints the sorted vector `[1, 2, 3]`.

- [ ] **Step 3: Commit the regenerated core_lib**

```bash
git add boot/lib/module/core_lib.tw
git commit -m "build: regenerate core_lib with @std.sort"
```

---

### Task 4: Correctness test suite for `@std.sort`

**Files:**
- Create: `boot/tests/suites/std_sort_suite.tw`
- Modify: `boot/tests/main.tw`

- [ ] **Step 1: Write the suite (matches prelude `sort_by` across shapes)**

Create `boot/tests/suites/std_sort_suite.tw`:

```tw
//! Correctness of @std.sort.ints_by: must equal prelude sort_by across shapes,
//! including stability on duplicate keys (compare by key, check payload order).
use @std.testing.assert as assert
use @std.testing as runner
use @std.sort

fn asc(a: Int, b: Int) Order {
  Int.compare(a, b)
}

fn matches_sort_by(xs: Vector<Int>) Result<Void, String> {
  want := xs.sort_by(asc)
  got := sort.ints_by(xs, asc)
  try assert.equal(got.len(), want.len())

  for i in range(want.len()) {
    try assert.equal(got[i], want[i])
  }

  .Ok({})
}

fn seeded(n: Int, mult: Int) Vector<Int> {
  collect i in range(n) {
    i * mult % n
  }
}

pub fn suite() runner.Suite {
  runner
    .suite("std_sort")
    .test(
      "empty and single",
      fn() {
        empty: Vector<Int> = []
        try matches_sort_by(empty)
        try matches_sort_by([42])
        .Ok({})
      },
    )
    .test(
      "sorted, reverse, all-equal",
      fn() {
        try matches_sort_by(seeded(500, 1))
        try matches_sort_by(collect i in range(500) { 500 - i })
        try matches_sort_by(collect i in range(500) { 7 })
        .Ok({})
      },
    )
    .test(
      "random and duplicate-heavy",
      fn() {
        try matches_sort_by(seeded(1000, 7919))
        try matches_sort_by(collect i in range(1000) { i % 10 })
        .Ok({})
      },
    )
    .test(
      "stability: equal keys keep input order",
      fn() {
        // Encode payload in low bits, key in high bits; compare by key only.
        xs := collect i in range(300) {
          (i % 5) * 1000 + i
        }
        by_key := fn(a: Int, b: Int) Order { Int.compare(a / 1000, b / 1000) }
        want := xs.sort_by(by_key)
        got := sort.ints_by(xs, by_key)
        try assert.equal(got.len(), want.len())

        for i in range(want.len()) {
          try assert.equal(got[i], want[i])
        }

        .Ok({})
      },
    )
}
```

- [ ] **Step 2: Register the suite in `boot/tests/main.tw`**

Add with the other `use .suites.*` lines (alphabetical neighborhood of `std`):

```tw
use .suites.std_sort_suite
```

And add to the suite list passed to the runner (near `scripting_vector_suite.suite(),`):

```tw
  std_sort_suite.suite(),
```

- [ ] **Step 3: Run the boot test suite — expect the new suite green**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -iE "std_sort|Failed|Ran [0-9]+ tests"`
Expected: no failures; `std_sort` tests pass. (If `@std.sort` is unresolved, Task 3's rebundle did not embed it — re-run `make bundle-cli`.)

- [ ] **Step 4: Commit**

```bash
git add boot/tests/suites/std_sort_suite.tw boot/tests/main.tw
git commit -m "test: @std.sort.ints_by matches sort_by (shapes + stability)"
```

---

### Task 5: Adopt the kernel in the dataframe `order_by` and benchmark

**Files:**
- Modify: `examples/performance/dataframe/frame/table.tw`

- [ ] **Step 1: Inspect the current sort call sites**

Run: `grep -n "sort_by\|fn sort_indices_by_column\|^use " examples/performance/dataframe/frame/table.tw`
Expected: `sort_indices_by_column` has eight `idx.sort_by(...)` calls — one per column type (`IntCol`/`FloatCol`/`StrCol`/`BoolCol`) across an ascending and a descending block. Only the two `.IntCol(keys) => idx.sort_by(...)` arms (one per block) are accelerated by the Int kernel; the benchmark sorts by the `amount` column (IntCol), so those two arms are what move it.

- [ ] **Step 2: Route the two IntCol arms through `@std.sort.ints_by`**

Add `use @std.sort` to the module's imports. For each of the two `.IntCol(keys) => idx.sort_by(fn(a, b) { ... })` arms, replace `idx.sort_by(fn(a, b) { ... })` with `sort.ints_by(idx, fn(a, b) { ... })` — keep the exact same comparator body; only the sort call changes. Leave the `FloatCol`/`StrCol`/`BoolCol` arms on the generic `sort_by` (Float family deferred; Str/Bool not covered by this kernel).

- [ ] **Step 3: Format, lint, and run the dataframe tests**

Run: `target/twk fmt examples/performance/dataframe/frame/table.tw && target/twk lint examples/performance/dataframe/bench/order_by_breakdown.tw && target/twk run examples/performance/dataframe/tests/query_suite.tw 2>&1 | grep -iE "Failed|Ran|pass"`
Expected: fmt idempotent, lint clean, dataframe query/table tests pass (order_by still correct).

- [ ] **Step 4: Benchmark — expect full order_by to drop ~1.2s → ~0.9–1.0s**

Run: `timeout 180 target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw 2>&1 | sed -n '/N = 1000000/,/^$/p'`
Expected at N=1M: `full order_by` meaningfully lower than the ~1215ms baseline (target ~0.9–1.0s); results still correct (checksum unchanged).

- [ ] **Step 5: Commit**

```bash
git add examples/performance/dataframe/frame/table.tw
git commit -m "dataframe: route order_by index sort through @std.sort.ints_by"
```

---

### Task 6: Record the shipped win in the vector-perf docs

**Files:**
- Modify: `docs/plans/performance/vector/boundary-tracklist.md`
- Modify: `docs/plans/performance/vector/README.md`

- [ ] **Step 1: Update the tracklist**

In `boundary-tracklist.md`, refresh the stale numbers and status: full `order_by` is ~1.2s (not 1.84s); the sort floor is read-bound (not mechanics); mark the buffer-kernel sort win as landed (Phase 1) with the `@std.sort` mechanism, and note the transparent `sort_by<Int>` swap (Phase 2) as the remaining generalization.

- [ ] **Step 2: Update the README**

In `README.md`, add a "Latest" note pointing at `b6-buffer-argsort-kernel-plan.md` and `b6-representation-preserving-sort-design.md`, with the measured before/after (`sort idx by amount` ~745ms; buffer kernel ~338–355ms; full `order_by` ~1.2s → measured Phase-1 number).

- [ ] **Step 3: Commit**

```bash
git add docs/plans/performance/vector/boundary-tracklist.md docs/plans/performance/vector/README.md
git commit -m "docs: record buffer-backed Int sort kernel win; refresh order_by numbers"
```

---

## Phase 2 (stretch) — Transparent `sort_by<Int>` swap

> **OUTCOME (landed, commit `4416dfe3`):** shipped, but with a correction the
> original Task 7 below missed. The swap target `@std.sort.ints_by` is a Twinkle
> *source* function, so the linker's DCE drops it when it has no explicit caller
> (the swap introduces the only call, and DCE runs *before* monomorphize). Fix: a
> DCE **root-retention** step in `core_linker.tw` (append the kernel fid to
> `entry_func_ids` before `compute_reachable` when `@std.sort` is linked) so the
> kernel survives. Scope: the swap is transparent for any program that `use`s
> `@std.sort` (fid lookup returns `.None` otherwise → clean fall-through to generic
> `sort_by`; the boot compiler keeps generic `sort_by`, self-host unaffected).
> Confirmed keys: `method_table["Vector::sort_by"]`, `func_table["ints_by"]`.
> Result: standalone `sort idx by amount` ~745ms → ~400ms via the transparent
> swap; self-host green; 2987 boot tests pass. Universal (no-import) transparency
> would require relocating the kernel into the always-linked prelude — deliberately
> NOT done. The task text below is kept for the record; the retention step is the
> delta that actually shipped.

### Task 7: Recognize `sort_by<Int>` at monomorphize and swap to the kernel

**Files:**
- Modify: `boot/compiler/monomorphize.tw`

- [ ] **Step 1: Add recognition + fid lookup helpers**

Near the existing `is_vector_sort` / `vector_sort_i64_fid` (around `boot/compiler/monomorphize.tw:993-1010`), add analogous helpers:

```tw
// True when `fid` is the prelude `Vector.sort_by`. Resolved via the method table,
// keyed "Vector::sort_by".
fn is_vector_sort_by(ctx: MonoCtx, fid: FuncId) Bool {
  case ctx.method_table["Vector::sort_by"] {
    .Some(f) => f.id == fid.id,
    .None => false,
  }
}

// FuncId of the @std.sort buffer kernel (`sort::ints_by`), if present.
fn sort_ints_by_fid(ctx: MonoCtx) FuncId? {
  ctx.func_table["sort::ints_by"]
}
```

Note: confirm the exact `func_table` / `method_table` key for a resolved `@std.sort.ints_by` by grepping the built module's tables (add a temporary debug print in `monomorphize.tw` or inspect via the pipeline); adjust the key string to the real one before relying on it.

- [ ] **Step 2: Add the swap in the call-rewrite site**

In the type-directed fast path (the `if ctx.is_vector_sort(fid) ...` block around line 1510), add a sibling branch for `sort_by` with an `Int` element type argument, swapping the callee `GlobalFunc` to `sort_ints_by_fid()` and passing `new_args` (the `(xs, cmp)` arguments) unchanged. Fall through to the generic specialization when the fid isn't the kernel or the element type isn't `Int`.

- [ ] **Step 3: Rebundle and verify the swap fires**

Run: `make bundle-cli 2>&1 | tail -3 && target/twk build examples/performance/dataframe/bench/order_by_breakdown.tw -o /tmp/ob2.wat && grep -c "ints_by\|merge_run" /tmp/ob2.wat`
Expected: the WAT references the kernel; a build of a program using `idx.sort_by(cmp)` on `Vector<Int>` now calls the kernel path.

- [ ] **Step 4: Revert the explicit dataframe call (now redundant) and re-benchmark**

Revert Task 5's edit so the dataframe uses idiomatic `idx.sort_by(cmp)` again; confirm the benchmark still shows the improved number (now via the transparent swap).

Run: `timeout 180 target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw 2>&1 | sed -n '/N = 1000000/,/^$/p'`
Expected: `sort idx by amount` and `full order_by` at the improved Phase-1 levels, achieved without the explicit `sort.ints_by` call.

- [ ] **Step 5: Full test + self-host + commit**

Run: `make boot-test 2>&1 | grep -iE "Failed|Ran [0-9]+ tests" && make bundle-cli 2>&1 | tail -3`
Expected: all boot tests pass; self-host reaches "Fixed point reached" (the compiler sorts through this path — a miscompile would break the bootstrap).

```bash
git add boot/compiler/monomorphize.tw examples/performance/dataframe/frame/table.tw
git commit -m "backend: route Vector<Int> sort_by through the @std.sort buffer kernel"
```

**Stage0 note:** this swap lives in boot's `monomorphize.tw`; stage0 (Rust) compiling `boot/main.tw` will use the generic `sort_by` (correct, just slower). No stage0 change is required for correctness — only confirm `make stage2` still succeeds.

---

## Self-review notes (coverage vs the design spec)

- Kernel (typed reads, comparator per compare, no new runtime ops, box once at the boundary): Tasks 1–2. The design's "PVecI64" is realized here as linear-memory i64 (mechanism (d)), which the prototype proved beats the boxed path — an equivalent-or-better route to the same win.
- Recognition/emission: Phase 1 uses explicit adoption (Task 5); Phase 2 adds the transparent monomorphize swap (Task 7), mirroring `sort_i64`.
- Element families: Int only (design "Int first"). Float/Bool are out of scope here; a future `floats_by` follows the same shape with `view_f64`.
- Testing: correctness matrix + stability (Tasks 1, 4), benchmark gate (Tasks 1, 5), self-host + boot-test (Task 7), boundary correctness via the dataframe suite (Task 5).
- Memory safety: every return path frees both buffers (Task 2 kernel).
- Docs: Task 6.
