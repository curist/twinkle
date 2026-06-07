# Native in-place `Vector.sort_by` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the prelude `Vector.sort_by` merge sort with an in-place 3-way quicksort over a uniquely-owned working buffer, so every `sort_by` (and `sort`, and dataframe `order_by`) stops allocating a fresh persistent vector per merge level.

**Architecture:** Pure-prelude change in `boot/prelude/vector.tw`. Keep the existing cheap ordered/reverse pre-scan. Materialize a uniquely-owned buffer with `Vector.make`, fill it, then sort in place using index read/write sugar (`buf[i]`, `buf[i] = v` → `set_at` → `vector$set_unsafe`, in-place under uniqueness), calling the closure comparator. No runtime, builtin, or stage0 changes.

**Tech Stack:** Twinkle (`.tw`) prelude; boot self-hosted compiler; `make bundle-cli` to re-embed prelude; `make boot-test` for the suite.

**Spec:** [native-sort-by-inplace.md](native-sort-by-inplace.md)

---

## Build & test loop (read once)

- **Prelude edits** (`boot/prelude/vector.tw`) only take effect after re-embedding: `make bundle-cli` (rebuilds `core_lib.tw` → `target/boot.wasm` via the self-host loop → `target/twk`). This also catches stage0 bootstrap breakage and self-host divergence.
- **Test-only edits** (`boot/tests/*`) run directly: `make boot-test` (= `target/twk run boot/tests/main.tw`) with no rebuild.
- After any `.tw` edit, run `make fmt` (idempotent canonical formatting).
- The compiler itself uses the prelude, so a broken sort can break `make bundle-cli`. If `bundle-cli` fails after Task 2, the sort logic is wrong — fix before proceeding.

---

## File structure

- **Modify:** `boot/prelude/vector.tw` — replace `merge_sorted` + `sort_by_range` with `swap`, `median_of_three`, `insertion_sort_range`, `quicksort_range`; rewrite `sort_by`'s heavy path. `sort` is unchanged.
- **Modify:** `boot/tests/suites/api_vector_suite.tw` — add two module-private helpers and one robustness test.
- **Modify (Task 4):** `docs/plans/native-sort-by-inplace.md` — record results and stability decision.

---

## Task 1: Characterization tests for `sort_by`

These tests pass on the **current** merge sort and must keep passing after the rewrite. They run test-only, so no rebuild is needed for this task.

**Files:**
- Modify: `boot/tests/suites/api_vector_suite.tw`

- [ ] **Step 1: Add two module-private helpers near the top of the suite file** (after the existing `use` lines, before the suite function)

```tw
fn sort_suite_is_sorted(xs: Vector<Int>) Bool {
  ok := true
  i := 1

  for i < xs.len() and ok {
    if xs[i - 1] > xs[i] {
      ok = false
    }

    i = i + 1
  }

  ok
}

fn sort_suite_sum(xs: Vector<Int>) Int {
  total := 0

  for x in xs {
    total = total + x
  }

  total
}
```

- [ ] **Step 2: Add the robustness test** immediately after the existing `"sort_by keeps duplicates and handles empty singleton"` test (match the surrounding `.test(...)` chaining and trailing comma style)

```tw
    .test(
      "sort_by handles large, duplicate-heavy, and adversarial inputs",
      fn() {
        // duplicate-heavy: tiny value range forces the 3-way partition path
        dup := collect i in range(2000) {
          (i * 7919) % 11
        }
        sorted_dup := dup.sort_by(Int.compare)
        try assert.equal(sorted_dup.len(), 2000)
        try assert.is_true(sort_suite_is_sorted(sorted_dup))
        try assert.equal(sort_suite_sum(sorted_dup), sort_suite_sum(dup))

        // wide pseudo-random range; crosses the insertion cutoff and trie boundaries
        wide := collect i in range(5000) {
          (i * 7919 + 17) % 100000 - 50000
        }
        sorted_wide := wide.sort_by(Int.compare)
        try assert.equal(sorted_wide.len(), 5000)
        try assert.is_true(sort_suite_is_sorted(sorted_wide))
        try assert.equal(sort_suite_sum(sorted_wide), sort_suite_sum(wide))

        // already-sorted and reverse-sorted hit the pre-scan fast paths
        asc := collect i in range(1000) {
          i
        }
        try assert.is_true(sort_suite_is_sorted(asc.sort_by(Int.compare)))
        desc := collect i in range(1000) {
          1000 - i
        }
        try assert.is_true(sort_suite_is_sorted(desc.sort_by(Int.compare)))

        // all-equal input
        flat := Vector.make(500, 7)
        sorted_flat := flat.sort_by(Int.compare)
        try assert.is_true(sort_suite_is_sorted(sorted_flat))
        try assert.equal(sorted_flat.len(), 500)

        // the input vector must not be mutated by the sort
        original: Vector<Int> = [3, 1, 2]
        ignored := original.sort_by(Int.compare)
        try assert.equal(original[0], 3)
        try assert.equal(original[1], 1)
        try assert.equal(original[2], 2)
        .Ok({})
      },
    )
```

- [ ] **Step 3: Format**

Run: `make fmt`
Expected: completes; re-running produces no further diff.

- [ ] **Step 4: Run the suite against the current implementation**

Run: `make boot-test`
Expected: PASS, including the new `"sort_by handles large, duplicate-heavy, and adversarial inputs"` test. (This confirms the tests are correct against the known-good merge sort before we change anything.)

- [ ] **Step 5: Commit**

```bash
git add boot/tests/suites/api_vector_suite.tw
git commit -m "test: characterize Vector.sort_by on large/duplicate/adversarial inputs"
```

---

## Task 2: Reimplement `sort_by` as in-place 3-way quicksort

**Files:**
- Modify: `boot/prelude/vector.tw`

- [ ] **Step 1: Delete the old merge-sort helpers** `merge_sorted` and `sort_by_range` (the two `fn` definitions immediately above `pub fn sort_by`). Leave `sort_by`'s signature and the `sort` function in place for now.

- [ ] **Step 2: Add the new private helpers** in their place (above `pub fn sort_by`)

```tw
fn swap<T>(buf: Vector<T>, i: Int, j: Int) Vector<T> {
  tmp := buf[i]
  buf[i] = buf[j]
  buf[j] = tmp
  buf
}

/// Median value of buf[a], buf[b], buf[c] under cmp. Used as the quicksort
/// pivot value (a copy, so partition swaps don't invalidate it).
fn median_of_three<T>(buf: Vector<T>, a: Int, b: Int, c: Int, cmp: fn(T, T) Order) T {
  va := buf[a]
  vb := buf[b]
  vc := buf[c]

  case cmp(va, vb) {
    .Gt => case cmp(va, vc) {
      .Gt => case cmp(vb, vc) {
        .Gt => vb,
        _ => vc,
      },
      _ => va,
    },
    _ => case cmp(vb, vc) {
      .Gt => case cmp(va, vc) {
        .Gt => va,
        _ => vc,
      },
      _ => vb,
    },
  }
}

/// In-place insertion sort over [lo, hi). Used for small partitions.
fn insertion_sort_range<T>(buf: Vector<T>, lo: Int, hi: Int, cmp: fn(T, T) Order) Vector<T> {
  i := lo + 1

  for i < hi {
    key := buf[i]
    j := i - 1
    placing := true

    for j >= lo and placing {
      case cmp(buf[j], key) {
        .Gt => {
          buf[j + 1] = buf[j]
          j = j - 1
        },
        _ => {
          placing = false
        },
      }
    }

    buf[j + 1] = key
    i = i + 1
  }

  buf
}

/// In-place Dijkstra 3-way quicksort over [lo, hi). Three-way partitioning
/// keeps duplicate-heavy keys (e.g. low-cardinality dataframe columns) near
/// O(n log n) instead of degrading to O(n^2).
fn quicksort_range<T>(buf: Vector<T>, lo: Int, hi: Int, cmp: fn(T, T) Order) Vector<T> {
  n := hi - lo

  if n <= 1 {
    return buf
  }

  if n <= 16 {
    return insertion_sort_range(buf, lo, hi, cmp)
  }

  mid := lo + n / 2
  pivot := median_of_three(buf, lo, mid, hi - 1, cmp)
  lt := lo
  i := lo
  gt := hi - 1

  for i <= gt {
    case cmp(buf[i], pivot) {
      .Lt => {
        buf = swap(buf, lt, i)
        lt = lt + 1
        i = i + 1
      },
      .Gt => {
        buf = swap(buf, i, gt)
        gt = gt - 1
      },
      .Eq => {
        i = i + 1
      },
    }
  }

  buf = quicksort_range(buf, lo, lt, cmp)
  buf = quicksort_range(buf, gt + 1, hi, cmp)
  buf
}
```

- [ ] **Step 3: Rewrite the heavy path of `sort_by`.** Keep the pre-scan exactly as-is; replace only the final `sort_by_range(xs, 0, n, cmp)` call with buffer materialization + quicksort. The full function should read:

```tw
/// Return a new vector sorted by comparator cmp.
///
/// Not a stable sort: elements that compare Equal may be reordered.
pub fn sort_by<T>(xs: Vector<T>, cmp: fn(T, T) Order) Vector<T> {
  n := xs.len()

  if n <= 1 {
    return xs
  }

  ascending := true
  strictly_descending := true
  i := 1

  for i < n and (ascending or strictly_descending) {
    case cmp(xs[i - 1], xs[i]) {
      .Gt => {
        ascending = false
      },
      .Lt => {
        strictly_descending = false
      },
      .Eq => {
        strictly_descending = false
      },
    }

    i = i + 1
  }

  if ascending {
    return xs
  }

  if strictly_descending {
    return xs.reverse()
  }

  buf := make(n, xs[0])
  k := 0

  for k < n {
    buf[k] = xs[k]
    k = k + 1
  }

  quicksort_range(buf, 0, n, cmp)
}
```

- [ ] **Step 4: Format**

Run: `make fmt`
Expected: completes; re-running produces no further diff.

- [ ] **Step 5: Re-embed the prelude and rebuild the CLI**

Run: `make bundle-cli`
Expected: completes through the self-host loop with no error. (A failure here means the new sort miscompiled or broke the self-hosting compiler — fix the sort before continuing.)

- [ ] **Step 6: Run the boot suite**

Run: `make boot-test`
Expected: PASS, including both the new robustness test from Task 1 and the existing `sort_by` / dataframe `order_by` tests.

- [ ] **Step 7: Commit**

```bash
git add boot/prelude/vector.tw boot/lib/module/core_lib.tw
git commit -m "prelude: sort_by in place via 3-way quicksort over an owned buffer

Replace the merge sort (which allocated a fresh persistent vector at every
merge level) with an in-place Dijkstra 3-way quicksort over a uniquely-owned
buffer materialized by Vector.make. Keeps the cheap ordered/reverse pre-scan
and the closure comparator. Now unstable; equal elements may reorder."
```

(Note: `make bundle-cli` regenerates `boot/lib/module/core_lib.tw`; include it in the commit.)

---

## Task 3: Verify in-place behavior and run the benchmark gate

The decisive question for Approach A is whether `buf[i] = v` actually mutates in place. If uniqueness analysis does **not** keep it in place across the helper-call boundaries, each write copies the whole buffer → O(n²) → the N=1,000,000 benchmark will hang or take tens of seconds. The benchmark timing is therefore the primary in-place signal.

**Files:** none modified except the spec doc in Task 4.

- [ ] **Step 1: Run the order-by microbenchmarks**

Run: `target/twk run examples/dataframe/bench/order_by_micro.tw`
Expected: completes in seconds (not hanging). Record `sort values`, `sort idx id`, `sort idx key` at each N.
Baseline to beat (N=1,000,000): `sort values` ~829ms, `sort idx key` ~1674ms.
**Red flag:** if any N=1,000,000 line takes many seconds, writes are copying — go to Step 5.

- [ ] **Step 2: Run the breakdown and end-to-end benchmarks**

Run:
```bash
target/twk run examples/dataframe/bench/order_by_breakdown.tw
target/twk run examples/dataframe/bench/main.tw
```
Record `sort idx by amount`, `full order_by` (breakdown) and `order_by` (main).
Baseline to beat (N=1,000,000): `order_by` ~2531ms.

- [ ] **Step 3 (optional, if timings are ambiguous): inspect generated code for copies**

Run: `target/twk build examples/dataframe/bench/order_by_micro.tw -o /tmp/sort.wat`
Then grep the sort path:
```bash
grep -nc "vector\$set_unsafe\|set_in_place" /tmp/sort.wat
grep -nc "builder_from\|copy_leaves\|builder_new" /tmp/sort.wat
```
Expected when in-place: the sort uses `set`/`set_in_place` writes; it should not be funnelling every write through a full-vector copy/builder. (Use `boot/prelude/feedback`: prefer grep/sed over reading the whole WAT.)

- [ ] **Step 4: Record results in the spec** — see Task 4. Then evaluate the decision gate:
  - **PASS:** `order_by` (main, N=1M) drops materially from ~2531ms and suites are green → Approach A succeeds; continue to Task 4.
  - **FAIL (weak gain or O(n²) blowup):** escalate to Approach C (flat dense scratch-array + stable merge). Capture the measured numbers and the in-place finding in the spec, commit that note, and stop — Approach C is a separate plan.

- [ ] **Step 5 (only if Step 1 showed O(n²) blowup): fallback before escalating**

The likely cause is uniqueness not surviving the `swap` / `quicksort_range` call boundaries. Before abandoning A, try inlining the entire sort into a single iterative function (explicit stack instead of recursion, no `swap`/helper calls) so the buffer never leaves one function body and stays trivially unique. Re-run Step 1. If still O(n²), escalate to Approach C.

- [ ] **Step 6: Commit the recorded benchmark numbers** (spec edit from Task 4 may be combined here)

```bash
git add docs/plans/native-sort-by-inplace.md
git commit -m "docs: record in-place sort_by benchmark results and gate decision"
```

---

## Task 4: Finalize stability decision and spec status

**Files:**
- Modify: `docs/plans/native-sort-by-inplace.md`

- [ ] **Step 1: Update the spec `Status` line** to reflect the outcome — either `implemented (Approach A); kept` with the measured `order_by` number, or `Approach A measured insufficient; escalating to Approach C` with the numbers.

- [ ] **Step 2: Record the benchmark table** (before/after for `sort values`, `sort idx key`, `order_by` at N=1,000,000) in the `Verification & decision gate` section.

- [ ] **Step 3: Confirm the stability note** — the `sort_by` doc comment now states it is not stable (added in Task 2 Step 3). If any consumer turns out to need stability, note it here as the trigger to do Approach C.

- [ ] **Step 4: Format and commit** (skip if already committed in Task 3 Step 6)

```bash
make fmt
git add docs/plans/native-sort-by-inplace.md
git commit -m "docs: finalize native sort_by Approach A status and stability note"
```

---

## Self-review notes

- **Spec coverage:** pre-scan retained (Task 2 Step 3); uniquely-owned buffer via `make` (Task 2 Step 3); 3-way quicksort + insertion cutoff + median-of-three (Task 2 Step 2); dead `merge_sorted`/`sort_by_range` removed (Task 2 Step 1); stability change documented (Task 2 Step 3, Task 4); in-place verification + decision gate + escalation path (Task 3); benchmark targets match spec baselines (Task 3). All spec sections covered.
- **No new public API**, no runtime/builtin/stage0 changes — matches spec "out of scope".
- **Type consistency:** helpers `swap`, `median_of_three`, `insertion_sort_range`, `quicksort_range` are referenced exactly as defined; `make`, `reverse`, `len`, index sugar are existing prelude members; `cmp: fn(T, T) Order` signature matches `sort_by`.
