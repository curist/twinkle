# Vector.gather + dataframe gather-path optimization — Implementation Plan

> **Archive status:** Completed and archived. Phase 1, the v1 `Vector.gather` runtime primitive, dataframe routing, and the `order_by` comparator-specialization follow-up landed. The measured conclusion is that typed join null-fill was the clearest win; `head` now uses structural slice; v1 gather mostly flattened against Phase 1 for `filter` because it still performs independent trie lookups; and `order_by` remains comparator/random-read bound. Future work is a trie-aware gather path and/or a deeper typed/key sort strategy.

**Goal:** Make the dataframe's gather/reorder paths (`take`, `filter`, `join`, `head`, `group_by`) fast and DRY, first with zero-risk pure-Twinkle changes, then (optionally) with a runtime `Vector.gather` primitive — a builder-loop in v1 (constant-factor), with a trie-aware bulk path for monotonic index sets as a possible v2.

**Architecture:** Every dataframe reorder/select reduces to "build an index vector, then gather columns by it" (`examples/dataframe/frame/table.tw` `take` → `column.gather`). Today `column.gather` is a hand-rolled `for … { out = out.append(v[i]) }` loop per `ColData` variant. Phase 1 replaces those with `collect` (builder-backed, O(1)/push) and switches `head` to structural `slice`. Phase 2 adds a `Vector.gather(xs, idx)` runtime builtin (mirrors `Vector.drop_last`). Phase 3 routes `column.gather` through it.

**Tech Stack:** Twinkle (`.tw`), the `rt.arr` persistent-vector runtime (`boot/compiler/codegen/runtime/arr.tw`, `src/runtime/arr.rs`), the dataframe stress-test project (`examples/dataframe/`, branch `dataframe-stress-test`).

**Design references:** `docs/plans/dataframe-friction-log.md` (the perf cliff), `docs/plans/dataframe-stress-test.md` (engine design).

---

## Cost model & honest expectations (read first)

Two distinct cost centers; this plan targets the **gather**-bound one. Measured at N=1M
(`bench/main.tw`): `order_by` ≈2.8s, `join` ≈1.5s, `filter` ≈0.21s, `group_by` ≈0.34s.

- **Gather-bound ops** — `filter`, `join`, `head`, `group_by` (its key-column gather), and
  the final `take` of `order_by` — spend their time in `column.gather` (≈n element reads +
  appends). These are what `gather`/`collect` improve, all by a **constant factor** (Phase 1
  `collect` and a Phase 2 v1 builder-loop are both O(n) with `n` independent lookups; they cut
  per-element overhead, not the asymptotics). **`filter`'s index set is strictly increasing**
  (rows kept in source order), so it is the *only* consumer that could see an
  asymptotic/cache win — but ONLY from a trie-aware bulk gather (Phase 2 **v2**, not v1).
  Scattered index sets (sorted permutations, group firsts) get the constant win at best.
- **Sort-bound: `order_by`.** Its dominant cost is the **comparator**, not the gather: for
  n=1M the sort does ≈n·log₂n ≈ 20M comparisons, each calling `compare_at` with two PVec
  reads → ≈40M reads, versus ≈1M reads for the final `take`. So **gather barely moves
  `order_by`** (~1M of ~41M reads). `order_by`'s real lever is materializing the sort key
  once so the comparator avoids per-call `ColData` dispatch + null-branch — that is a
  *related but separate* change, sketched in the appendix and **out of scope** for this plan.

YAGNI consequence: **Phase 1 (pure Twinkle) likely captures most of the realizable win with
zero language risk. Measure after Phase 1 before deciding to do Phase 2.**

---

## File structure

```
examples/dataframe/frame/column.tw   column.gather (collect), column.gather_or_null (join -1), column.slice (head)
examples/dataframe/frame/table.tw    head -> slice; take/order_by/filter unchanged (benefit via column.gather)
examples/dataframe/frame/join.tw     gather_nullable -> column.gather_or_null (drop the Scalar round-trip)
examples/dataframe/tests/column_suite.tw   gather / gather_or_null / slice tests
examples/dataframe/tests/table_suite.tw    head tests
examples/dataframe/bench/main.tw     re-run to record deltas

# Phase 2 (language, branch `main`):
boot/prelude/signatures/vector.tw            gather signature stub
boot/compiler/codegen/runtime/arr.tw         gather_fn() + registration in the FuncDef list
boot/compiler/builtins.tw                     ABI + rt(...) mapping for vector$gather
src/runtime/arr.rs, src/types/env.rs, src/codegen/prelude.rs,
src/intrinsics/{registry.rs,signatures.rs}, src/ir/lower.rs   stage0 parity
docs/API.md                                   Vector.gather row
```

---

## Conventions

- Run dataframe tests: `target/twk run examples/dataframe/main.tw`; one suite: `TWK_TEST_FILTER="column" target/twk run examples/dataframe/main.tw`.
- Format after edits: `target/twk fmt <file>` (idempotent).
- Phase 1 + 3 are on branch `dataframe-stress-test`. Phase 2 is on `main` (language change). If Phase 2 is done, rebase/merge `main` into the branch (or cherry-pick) before Phase 3 so `target/twk` has `Vector.gather`.
- The scalar type is `Scalar` (not `Cell` — reserved). `Vector` index `v[i]` traps on OOB; `.get(i)` returns `Option`.

---

## Phase 1 — Pure-Twinkle gather wins (no language change)

### Task 1: `column.gather` via `collect`

`collect k in xs { … }` lowers to a mutable builder (O(1) amortized push, one freeze),
replacing the `for … { out = out.append(…) }` pattern (which rebuilds the tail repeatedly).
Same behavior, lower constant factor; also collapses the per-variant boilerplate.

**Files:**
- Modify: `examples/dataframe/frame/column.tw` (`gather`)
- Modify: `examples/dataframe/tests/column_suite.tw` (gather already has a test; add a duplicate-index case)

- [ ] **Step 1: Add a failing test for gather with duplicate + reordered indices**

Append to the `column` suite chain in `examples/dataframe/tests/column_suite.tw`:

```tw
    .test(
      "gather duplicates and reorders, carrying nulls",
      fn() {
        c := column.with_nulls(ColData.IntCol([5, 6, 7]), [false, true, false])
        g := column.gather(c, [2, 2, 1, 0])
        ok1 := try assert.equal(column.as_ints(g), [7, 7, 6, 5])
        ok2 := try assert.is_true(column.is_null(g, 2))
        assert.is_false(column.is_null(g, 0))
      },
    )
```

- [ ] **Step 2: Run to confirm it passes against the CURRENT implementation (baseline)**

Run: `TWK_TEST_FILTER="column" target/twk run examples/dataframe/main.tw`
Expected: PASS (the current loop already handles this). This locks behavior before refactor.

- [ ] **Step 3: Rewrite `gather` to use `collect`**

Replace the entire `pub fn gather(...)` body in `examples/dataframe/frame/column.tw` with:

```tw
/// Gather rows by index, carrying the null mask along.
pub fn gather(c: Column, idx: Vector<Int>) Column {
  new_data := case c.data {
    .IntCol(v) => ColData.IntCol(collect k in idx { v[k] }),
    .FloatCol(v) => ColData.FloatCol(collect k in idx { v[k] }),
    .StrCol(v) => ColData.StrCol(collect k in idx { v[k] }),
    .BoolCol(v) => ColData.BoolCol(collect k in idx { v[k] }),
  }
  new_nulls := collect k in idx { c.nulls[k] }

  Column.{ data: new_data, nulls: new_nulls }
}
```

- [ ] **Step 4: Run column + full suite to confirm behavior preserved**

Run: `target/twk run examples/dataframe/main.tw`
Expected: PASS — all suites green (no behavior change).

Then format: `target/twk fmt examples/dataframe/frame/column.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/column.tw examples/dataframe/tests/column_suite.tw
git commit -m "dataframe: gather via collect (builder) instead of append loop"
```

---

### Task 2: `column.gather_or_null` for the join `-1` sentinel

Left join marks unmatched right rows with index `-1`, today materialized through a
`Scalar` round-trip in `join.gather_nullable` (boxes every element + re-infers dtype). Add a
typed `column.gather_or_null` that builds the result directly per `ColData` variant: a `-1`
index yields a masked placeholder. This removes the boxing and the all-null dtype-guess.

**Files:**
- Modify: `examples/dataframe/frame/column.tw` (add `gather_or_null`)
- Modify: `examples/dataframe/tests/column_suite.tw`

- [ ] **Step 1: Write the failing test**

Append to the `column` suite chain in `examples/dataframe/tests/column_suite.tw`:

```tw
    .test(
      "gather_or_null fills -1 indices with null",
      fn() {
        c := column.str_col(["x", "y"])
        g := column.gather_or_null(c, [1, -1, 0])
        ok1 := try assert.equal(column.as_strs(g), ["y", "", "x"])
        ok2 := try assert.is_true(column.is_null(g, 1))
        ok3 := try assert.is_false(column.is_null(g, 0))
        assert.is_false(column.is_null(g, 2))
      },
    )
```

- [ ] **Step 2: Run to confirm it fails**

Run: `target/twk run examples/dataframe/main.tw`
Expected: FAIL — compile error, unknown function `gather_or_null`.

- [ ] **Step 3: Implement `gather_or_null` in `column.tw`**

Add to `examples/dataframe/frame/column.tw`:

```tw
/// Like gather, but a negative index produces a null cell (type-appropriate
/// placeholder + masked). Used by left/outer joins for unmatched rows.
pub fn gather_or_null(c: Column, idx: Vector<Int>) Column {
  nulls := collect k in idx { k < 0 or c.nulls[k] }

  new_data := case c.data {
    .IntCol(v) => ColData.IntCol(collect k in idx { if k < 0 {
      0
    } else {
      v[k]
    } }),
    .FloatCol(v) => ColData.FloatCol(collect k in idx { if k < 0 {
      0.0
    } else {
      v[k]
    } }),
    .StrCol(v) => ColData.StrCol(collect k in idx { if k < 0 {
      ""
    } else {
      v[k]
    } }),
    .BoolCol(v) => ColData.BoolCol(collect k in idx { if k < 0 {
      false
    } else {
      v[k]
    } }),
  }

  Column.{ data: new_data, nulls }
}
```

- [ ] **Step 4: Run to confirm it passes**

Run: `TWK_TEST_FILTER="column" target/twk run examples/dataframe/main.tw`
Expected: PASS.

Then format: `target/twk fmt examples/dataframe/frame/column.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/column.tw examples/dataframe/tests/column_suite.tw
git commit -m "dataframe: add typed column.gather_or_null for join null-fill"
```

---

### Task 3: Route `join` through `gather_or_null`; inner join remains correct (no `-1` indices)

`join.build_output` currently always calls `gather_nullable` (Scalar round-trip) for right
columns. Switch right columns to `column.gather_or_null`; the right columns of an inner join
never see `-1` so they could use plain `gather`, but `gather_or_null` is correct for both and
keeps one path. Left columns already use `column.gather` via no-op indices (no `-1`).

**Files:**
- Modify: `examples/dataframe/frame/join.tw` (`build_output`, delete `gather_nullable`)

- [ ] **Step 1: Confirm existing join tests cover both paths**

Run: `TWK_TEST_FILTER="join" target/twk run examples/dataframe/main.tw`
Expected: PASS (2 tests: inner + left-with-null). These lock behavior before refactor.

- [ ] **Step 2: Replace `gather_nullable` call with `column.gather_or_null`**

In `examples/dataframe/frame/join.tw`, in `build_output`, change the right-column append from:

```tw
      out_cols = .append(gather_nullable(c, ridx))
```

to:

```tw
      out_cols = .append(column.gather_or_null(c, ridx))
```

- [ ] **Step 3: Delete the now-unused `gather_nullable` and its `Scalar` import if unused**

Remove the entire `fn gather_nullable(...) { … }` from `join.tw`. If `use frame.cell.{Scalar}`
and `use frame.cell` are now unused, remove them (the compiler will warn on unused imports;
keep `cell` only if `key_string` still uses `cell.to_string`).

- [ ] **Step 4: Run join + full suite**

Run: `target/twk run examples/dataframe/main.tw`
Expected: PASS — all suites green.

Then format: `target/twk fmt examples/dataframe/frame/join.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/join.tw
git commit -m "dataframe: join uses column.gather_or_null (drop Scalar round-trip)"
```

---

### Task 4: `head` via structural `slice`, not gather

`head(t, n)` builds indices `[0, n)` and gathers — O(n) element copies. A contiguous prefix
is exactly `Vector.slice(0, n)`, which is O(log n) and shares the source tree. Add
`column.slice` and make `head` use it.

**Files:**
- Modify: `examples/dataframe/frame/column.tw` (add `slice`)
- Modify: `examples/dataframe/frame/table.tw` (`head`)
- Modify: `examples/dataframe/tests/table_suite.tw`

- [ ] **Step 1: Write the failing test**

Append to the `table` suite chain in `examples/dataframe/tests/table_suite.tw`:

```tw
    .test(
      "head returns the first n rows",
      fn() {
        t := sample().head(2)
        ok1 := try assert.equal(t.nrows, 2)
        assert.equal(column.as_ints(try t.column("id")), [1, 2])
      },
    )
```

- [ ] **Step 2: Run to confirm it passes against current head (baseline behavior)**

Run: `TWK_TEST_FILTER="table" target/twk run examples/dataframe/main.tw`
Expected: PASS (current head already returns first n). Locks behavior before refactor.

- [ ] **Step 3: Add `column.slice`**

Add to `examples/dataframe/frame/column.tw`:

```tw
/// Structural subrange [start, end) of a column, sharing the source tree.
pub fn slice(c: Column, start: Int, end: Int) Column {
  new_data := case c.data {
    .IntCol(v) => ColData.IntCol(v.slice(start, end)),
    .FloatCol(v) => ColData.FloatCol(v.slice(start, end)),
    .StrCol(v) => ColData.StrCol(v.slice(start, end)),
    .BoolCol(v) => ColData.BoolCol(v.slice(start, end)),
  }

  Column.{ data: new_data, nulls: c.nulls.slice(start, end) }
}
```

- [ ] **Step 4: Rewrite `head` to slice each column**

Replace `pub fn head(...)` in `examples/dataframe/frame/table.tw` with:

```tw
pub fn head(t: Table, n: Int) Table {
  // Clamp at the source: negative n -> 0, n > nrows -> nrows. A negative limit
  // would otherwise reach column.slice(c, 0, limit) and trap.
  limit := n.max(0).min(t.nrows)
  out_cols := collect c in t.cols { column.slice(c, 0, limit) }

  Table.{ names: t.names, cols: out_cols, nrows: limit }
}
```

- [ ] **Step 5: Run table + full suite**

Run: `target/twk run examples/dataframe/main.tw`
Expected: PASS — all suites green (including the new head test).

Then format: `target/twk fmt examples/dataframe/frame/column.tw examples/dataframe/frame/table.tw`

- [ ] **Step 6: Commit**

```bash
git add examples/dataframe/frame/column.tw examples/dataframe/frame/table.tw examples/dataframe/tests/table_suite.tw
git commit -m "dataframe: head via structural column.slice instead of gather"
```

---

### Task 5: Re-benchmark Phase 1 and record the delta

**Files:**
- Modify: `docs/plans/dataframe-friction-log.md` (record Phase-1 numbers)

- [ ] **Step 1: Capture timings**

Run: `target/twk run examples/dataframe/bench/main.tw | tee /tmp/dataframe-bench-phase1.txt`
Expected: prints filter/order_by/group_by/join for N=10000/100000/1000000.

- [ ] **Step 2: Append a "Phase 1 (collect/slice) results" block** to the Performance section
of `docs/plans/dataframe-friction-log.md`, pasting before/after numbers and noting which ops
moved (expect `filter`/`join`/`group_by` to improve; `order_by` ~flat because it is
sort-bound, confirming the cost model).

- [ ] **Step 3: Decide on Phase 2.** If `filter`/`join` improved enough, Phase 2 (runtime
primitive) may be unnecessary — record the decision in the friction log. Commit:

```bash
git add docs/plans/dataframe-friction-log.md
git commit -m "dataframe: record Phase-1 gather optimization benchmark results"
```

---

## Phase 2 — Runtime `Vector.gather` primitive (language change, branch `main`)

Only do this if Phase 1 measurements justify it. Be clear-eyed about payoff: the **v1**
builtin below is a builder-loop — a constant-factor consolidation over Phase 1 `collect`
(one runtime call, no Twinkle loop / `ColData` dispatch), but the SAME `n` independent
lookups. The asymptotic/cache win for `filter`'s monotonic indices needs a **v2** trie-aware
bulk descent (sketched at the end of Task 6, not specified here). If Phase 1 already closed
the gap, this phase may not be worth the cross-compiler surface. Mirrors the
`Vector.drop_last` wiring exactly. **Do this on `main`, then rebase/merge into the dataframe
branch for Phase 3.**

API: `pub fn gather<T>(xs: Vector<T>, idx: Vector<Int>) Vector<T>` — `result[k] = xs[idx[k]]`,
length `idx.len()`, traps on OOB index (matches `xs[i]`).

### Task 6: Boot-compiler `Vector.gather`

**Files:**
- Modify: `boot/prelude/signatures/vector.tw` (after the `drop_last` stub, ~line 33)
- Modify: `boot/compiler/codegen/runtime/arr.tw` (add `gather_fn()`; register it in the FuncDef list near `drop_last_fn()`, ~line 149)
- Modify: `boot/compiler/builtins.tw` (ABI ~line 128; `rt(...)` ~line 507)

- [ ] **Step 1: Add the prelude signature stub**

In `boot/prelude/signatures/vector.tw`, add after the `drop_last` stub:

```tw
pub fn gather<T>(xs: Vector<T>, idx: Vector<Int>) Vector<T> {
  xs
}
```

- [ ] **Step 2: Add the ABI + runtime mapping in `builtins.tw`**

Next to the `drop_last` entries:

```tw
// with the other abi(...) entries (near line 128):
"vector$gather" => abi([pvec_n(), pvec_n()], [pvec_()]),
```
```tw
// with the other rt(...) registrations (near line 507):
rt("vector$gather", "rt.arr", "gather", .Some("Vector.gather")),
```

- [ ] **Step 3: Implement `gather_fn()` in `arr.tw`, modeled on `drop_last_fn`/`concat_fn`**

Add a `gather_fn()` `FuncDef` and register it in the FuncDef list (the vector that contains
`drop_last_fn(), builder_new_fn(), …` near line 149) by appending `gather_fn(),`.

Algorithm (use the existing runtime helpers — do NOT hand-roll trie traversal for v1).
NOTE the builder is **mutated in place**: `builder_new` returns a builder Array (not a PVec),
`builder_push(builder, elem)` returns **void** and mutates it, and `builder_freeze(builder)`
turns the builder Array into the final PVec. Do not reassign `builder` from `builder_push`.

```
gather(vec: PVec?, idx: PVec?) -> PVec:
  n       = len(idx)                 // call "len"
  builder = builder_new()            // builder Array (arr_ref); created ONCE
  i       = 0
  loop while i < n:
    k    = <i32 index = element i of idx>   // read idx[i] (an Int) via "get", unbox to i32
    elem = get(vec, k)                       // element as anyref
    builder_push(builder, elem)              // VOID — mutates `builder` in place
    i    = i + 1
  return builder_freeze(builder)             // builder Array -> PVec
```

Write the `FuncDef` in the same instruction-list DSL as `concat_fn`. Exact shapes to mirror
(confirmed in `arr.tw`/`builtins.tw`): params `[pvec_null(), pvec_null()]`, results
`[pvec_ref()]`; `builder_new` → `arr_ref()`; `builder_push` params `[arr_null(), .Anyref]`
results `[]`; `builder_freeze` params `[arr_null()]` results `[pvec_ref()]`. Use a `.Loop`/`.Br`
over `i`. **Verification point to check against `get_fn` (arr.tw ~line 1268) while
implementing:** how an `Int` element is read out of `idx` and converted to the i32 index
passed to `get(vec, …)`. The build/verify loop (Step 4) catches mismatches.

**v1 is a constant-factor change only** — it removes the Twinkle-level loop, the `ColData`
enum dispatch, and the per-element `append` rebuild, but it still does `n` independent
root-to-leaf `get`s, so it is NOT asymptotically better than Phase 1's `collect` for
arbitrary indices. The monotonic-`filter` speedup requires **v2 (optional, separate)**: a
trie-aware bulk descent that walks `idx` in order and reuses shared spine/leaf nodes. Do not
promise monotonic-specific gains until v2 exists.

- [ ] **Step 4: Regenerate core_lib, rebuild, and verify**

```bash
python3 tools/generate_core_lib.py
make bundle-cli
```
Then verify with a scratch program:
```bash
mkdir -p /tmp/twg && printf 'name="g"\n' > /tmp/twg/twinkle.toml
printf 'xs := [10, 20, 30, 40]\nprintln("${xs.gather([3, 0, 0, 2])}")\n' > /tmp/twg/main.tw
target/twk run /tmp/twg/main.tw   # expect: [40, 10, 10, 30]
```
Expected: prints `[40, 10, 10, 30]`.

- [ ] **Step 5: Run the boot suite and commit**

Run: `make boot-test`
Expected: all boot tests pass.

```bash
git add boot/prelude/signatures/vector.tw boot/compiler/codegen/runtime/arr.tw boot/compiler/builtins.tw
git commit -m "Add Vector.gather runtime builtin (bulk index/permute)"
```

---

### Task 7: Stage0 parity for `Vector.gather`

Mirror the boot builtin in the Rust reference compiler so `cargo run` matches. Use the
`drop_last` touch points as the map.

**Files:**
- Modify: `src/runtime/arr.rs` (add `gather_fn()`; register in the list ~line 85)
- Modify: `src/types/env.rs` (method entry ~line 257)
- Modify: `src/codegen/prelude.rs` (name mapping ~line 176)
- Modify: `src/intrinsics/registry.rs` (`spec!` ~line 219; name→id ~line 546)
- Modify: `src/intrinsics/signatures.rs` (twinkle_name + signature ~line 508)
- Modify: `src/ir/lower.rs` (lowering note/dispatch ~line 107, if drop_last has one)

- [ ] **Step 1: Replicate each `drop_last` entry for `gather`**

For every line found by `grep -rn "drop_last" src/`, add the analogous `gather` entry
(new prelude id constant `VECTOR_GATHER`, twinkle name `"Vector.gather"`, runtime symbol
`rt_arr__gather`, signature `fn<T>(Vector<T>, Vector<Int>) Vector<T>`). Implement
`gather_fn()` in `src/runtime/arr.rs` with the same builder-loop algorithm as Task 6 Step 3.

- [ ] **Step 2: Build stage0 and verify parity**

Run: `cargo build --release && cargo run --release -- run /tmp/twg/main.tw`
Expected: prints `[40, 10, 10, 30]` (same as boot).

- [ ] **Step 3: Targeted Rust tests + commit**

Run: `cargo test --release vector` (or the relevant filter)
Expected: pass.

```bash
git add src/
git commit -m "stage0: Vector.gather parity"
```

---

### Task 8: Document `Vector.gather`

**Files:**
- Modify: `docs/API.md` (Vector method table)

- [ ] **Step 1: Add the row** next to `.slice` / `.drop_last` in the Vector table:

```markdown
| `.gather(idx)` | `fn<T>(xs: Vector<T>, idx: Vector<Int>) Vector<T>` | Bulk index: `result[k] = xs[idx[k]]`, length `idx.len()`. Traps on OOB index. Permute/select/duplicate in one op. |
```

- [ ] **Step 2: Commit**

```bash
git add docs/API.md
git commit -m "docs: document Vector.gather"
```

---

## Phase 3 — Route the dataframe through `Vector.gather` (branch `dataframe-stress-test`)

Prereq: `main`'s `Vector.gather` is merged/rebased into the branch so `target/twk` has it.

### Task 9: `column.gather`/`gather_or_null` use `Vector.gather`

**Files:**
- Modify: `examples/dataframe/frame/column.tw` (`gather`)

- [ ] **Step 1: Confirm column tests pass (lock behavior)**

Run: `TWK_TEST_FILTER="column" target/twk run examples/dataframe/main.tw`
Expected: PASS.

- [ ] **Step 2: Rewrite `gather` to call `Vector.gather`**

Replace `pub fn gather(...)` in `examples/dataframe/frame/column.tw` with:

```tw
/// Gather rows by index, carrying the null mask along.
pub fn gather(c: Column, idx: Vector<Int>) Column {
  new_data := case c.data {
    .IntCol(v) => ColData.IntCol(v.gather(idx)),
    .FloatCol(v) => ColData.FloatCol(v.gather(idx)),
    .StrCol(v) => ColData.StrCol(v.gather(idx)),
    .BoolCol(v) => ColData.BoolCol(v.gather(idx)),
  }

  Column.{ data: new_data, nulls: c.nulls.gather(idx) }
}
```

(`gather_or_null` keeps its `collect` form — it needs the per-element `-1` branch, which
plain `Vector.gather` cannot express. Leave it as written in Task 2.)

- [ ] **Step 3: Run full suite**

Run: `target/twk run examples/dataframe/main.tw`
Expected: PASS — all tests green (behavior unchanged).

Then format: `target/twk fmt examples/dataframe/frame/column.tw`

- [ ] **Step 4: Re-benchmark and record**

Run: `target/twk run examples/dataframe/bench/main.tw | tee /tmp/dataframe-bench-phase3.txt`
Compare against `/tmp/dataframe-bench-phase1.txt`; append the delta to the Performance section
of `docs/plans/dataframe-friction-log.md`. Measure without assuming gains: a v1 builder-loop
may show little movement over Phase 1 `collect` (same `n` lookups). Only a v2 trie-aware path
would give `filter`'s monotonic indices a measurable edge — if v1 is flat, that is the
expected, reportable result, and the v2 work (or dropping Phase 2/3) is the follow-up.

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/column.tw docs/plans/dataframe-friction-log.md
git commit -m "dataframe: column.gather uses Vector.gather; record benchmark delta"
```

---

## How each consumer ends up improved (summary)

- **`take`** — the shared primitive; rewritten once (`column.gather`). Everything below funnels through it.
- **`filter`** — builds a strictly-increasing index set, then `take`. Phase 1 `collect` gives a constant win; a Phase 2 **v1** builtin adds only a smaller constant; an asymptotic/cache win needs Phase 2 **v2** (trie-aware bulk over the monotonic indices).
- **`join`** — left/right `take`; the `-1` null-fill goes through `column.gather_or_null` (Phase 1), dropping the per-element `Scalar` boxing + dtype re-inference.
- **`head`** — switched off gather entirely to structural `column.slice` (O(log n), shares the tree).
- **`group_by`** — its key-column gather (`firsts`, one row per group) routes through `column.gather`; the agg output columns still go through `from_cells` (Scalar) and are unaffected.
- **`order_by`** — only its final `take` improves (small); it is sort-bound. See appendix.

## Appendix — `order_by` comparator (related, OUT OF SCOPE)

`order_by`'s dominant cost is the sort comparator (≈40× the gather at 1M rows), so this plan
barely moves it. The real lever is to materialize the sort key once into a typed vector and
sort with a comparator that reads that vector directly (no `ColData` dispatch / null-branch
per comparison). Sketch (not part of this plan; do as separate work if `order_by` perf
matters):

```tw
// in table.order_by, replacing the compare_at comparator:
//   keys := column.as_ints(col)            // one extraction (per dtype)
//   sorted := idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })
// requires a per-ColData branch to pick the typed key vector + comparator.
```

A further structural win would be a key/radix sort that emits the permuted column directly,
avoiding the separate gather — but that is a larger change.

---

## Self-Review

- **Spec coverage:** gather primitive (Tasks 6–8) + every requested consumer — `filter`
  (via take/Vector.gather), `join` (Task 3 + gather_or_null), `head` (Task 4, slice),
  `group_by` (via column.gather) — covered; the summary table maps each. `order_by` honestly
  scoped out with rationale + appendix.
- **Placeholder scan:** Phase 1 and 3 steps contain complete code. The one non-literal piece
  is `gather_fn`'s wasm body (Task 6 Step 3), given as algorithm + exact helper calls +
  template (`concat_fn`/`drop_last_fn`) + named verification points, because the instruction
  DSL body must be validated by the build/verify loop, not fabricated blind. This is the
  documented `rt.arr` procedure (see memory `reference_runtime_builtin_wiring`), not a TODO.
- **Type consistency:** `column.gather(c, idx)`, `column.gather_or_null(c, idx)`,
  `column.slice(c, start, end)`, `Vector.gather<T>(xs, idx)` used consistently across tasks;
  `head` returns `Table`; `Scalar` (not `Cell`).
- **Sequencing:** Phase 1 stands alone (no language change). Phase 2 is gated on Phase 1
  measurements and lives on `main`. Phase 3 requires Phase 2 merged into the branch.
