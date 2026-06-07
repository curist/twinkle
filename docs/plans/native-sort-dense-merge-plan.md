# Native dense-buffer stable merge sort (Approach C) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the prelude `Vector.sort_by` recursive merge sort (which allocates a fresh persistent vector at every level and reads through the trie) with a stable bottom-up merge over a flat, mutable, dense Wasm-GC array — keeping the generic closure comparator, restoring stable sort, and adding only three trivial runtime primitives.

**Architecture:** Add an internal opaque builtin type `Scratch<T>` (anyref layout, backed by the existing `rt_types__Array` GC type — no new GC type), plus three tiny `rt.arr` ops `scratch_new` / `scratch_get` / `scratch_set`. Compose the PVec→buffer copy and buffer→PVec freeze in pure Twinkle (`scratch_set` loop and `collect`), and write the stable bottom-up merge in Twinkle in `boot/prelude/vector.tw`. Mirror everything across the boot compiler and the stage0 reference compiler.

**Tech Stack:** Twinkle (`.tw`) prelude + self-hosted boot compiler (`boot/`), Rust stage0 reference compiler (`src/`), hand-written Wasm-GC IR for runtime ops. `make bundle-cli` re-embeds the prelude and self-hosts; `make boot-test` runs the suite.

**Spec:** [native-sort-dense-merge.md](native-sort-dense-merge.md)

---

## Build & test loop (read once)

- **Compiler/runtime edits** (`arr.tw`, `builtins.tw`, `base_env.tw`, `wasm_layout.tw`, `prelude/signatures/*`, `prelude/vector.tw`) only take effect after `make bundle-cli`, which compiles `boot/main.tw` with the current `target/twk` (stage1), self-hosts to a fixed point, and rebuilds `target/twk`. It must print that the fixed point is reached. This is also the codegen correctness gate.
- **Test-only edits** (`boot/tests/*`) run directly with `make boot-test` (no rebuild).
- After any `.tw` edit run `make fmt` (idempotent canonical formatting). Markdown docs do not need `fmt`.
- **Stage0 edits** (`src/*.rs`) require `cargo build --release` and are exercised by `make bundle-cli`'s self-host fixed point + differential behavior. Do **not** run full `cargo test` (too slow); the self-host convergence in `bundle-cli` is the stage0 gate.
- Language constraints honored throughout: no `break`, no `+=` (use boolean guards + `x = x + 1`), `=` is rebinding.

## File structure

- **Modify** `boot/compiler/codegen/runtime/arr.tw` — three new `FuncDef`s (`scratch_new_fn`, `scratch_get_fn`, `scratch_set_fn`) + register them in `module()`.
- **Modify** `boot/compiler/builtins.tw` — three `rt(...)` specs + three `builtin_abi` arms.
- **Modify** `boot/compiler/base_env.tw` — append the `Scratch` builtin type entry.
- **Modify** `boot/compiler/codegen/wasm_layout.tw` — `is_scratch_type` helper + an anyref guard in `layout_of_named` (mirror every `Task`/`is_task_type` site).
- **Modify** `boot/prelude/signatures/vector.tw` — three stub signatures (`scratch_new`, `scratch_get`, `scratch_set`).
- **Modify** `boot/prelude/vector.tw` — delete `merge_sorted`/`sort_by_range`; add `scratch_from_vector`, `scratch_freeze`, `merge_run`, `merge_sort_dense`; rewrite `sort_by`'s heavy path. `sort` is unchanged.
- **Modify** stage0 mirror: `src/runtime/arr.rs`, `src/ir/lower.rs`, `src/intrinsics/registry.rs`, `src/intrinsics/signatures.rs`, `src/codegen/prelude.rs`, and stage0's builtin-type registration (parallel to boot `base_env.tw` + `wasm_layout.tw`).
- **Modify** `boot/tests/suites/api_vector_suite.tw` — a `Scratch` round-trip test (Task 1) and stability tests (Task 4). The existing robustness test from the Approach-A branch stays.

---

## Task 1: Boot — `Scratch<T>` type + `scratch_new`/`get`/`set` runtime ops

Add the internal buffer type and the three primitives to the **boot** compiler only. No sort changes yet. The user-callable surface is `Vector.scratch_new(n) → Scratch<T>`, `Vector.scratch_get(buf, i) → T`, `Vector.scratch_set(buf, i, v) → Void` (internal-by-convention; named under the `Vector` namespace so they reuse the existing `vector$*` resolution path).

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw`
- Modify: `boot/compiler/builtins.tw`
- Modify: `boot/compiler/base_env.tw`
- Modify: `boot/compiler/codegen/wasm_layout.tw`
- Modify: `boot/prelude/signatures/vector.tw`
- Test: `boot/tests/suites/api_vector_suite.tw`

- [ ] **Step 1: Write the failing round-trip test** in `boot/tests/suites/api_vector_suite.tw`, immediately after the `"sort_by handles large, duplicate-heavy, and adversarial inputs"` test (match the `.test(...)` chaining + trailing-comma style):

```tw
    .test(
      "scratch buffer round-trips and mutates in place",
      fn() {
        buf: Scratch<Int> = Vector.scratch_new(3)
        Vector.scratch_set(buf, 0, 10)
        Vector.scratch_set(buf, 1, 20)
        Vector.scratch_set(buf, 2, 30)
        try assert.equal(Vector.scratch_get(buf, 1), 20)
        out: Vector<Int> = collect i in range(3) {
          Vector.scratch_get(buf, i)
        }
        try assert.equal(out[0], 10)
        try assert.equal(out[2], 30)
        // in-place mutation: overwrite and confirm the same handle changed
        Vector.scratch_set(buf, 0, 99)
        try assert.equal(Vector.scratch_get(buf, 0), 99)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `make boot-test`
Expected: FAIL — the current `target/twk` does not know `Scratch` / `Vector.scratch_new`, so the suite fails to compile (e.g. "unknown type Scratch" or "no function Vector.scratch_new"). This confirms the test exercises the new surface.

- [ ] **Step 3: Add the three runtime ops** to `boot/compiler/codegen/runtime/arr.tw`. Place these `FuncDef`s near the other leaf-array ops (e.g. just after `set_in_place_fn`). They take/return the buffer as `.Anyref` and `RefCast` to `rt_types__Array` internally, so no boundary-cast support is required. `t_array()` returns the `rt_types__Array` symbol; element type is `eqref`, so `.RefNull(.None_)` is the array-fill init. `array.new` consumes `[init, count]` (init pushed first, count on top).

```tw
// scratch_new(len: i32) -> anyref (a fresh dense rt_types__Array of `len` null slots)
fn scratch_new_fn() FuncDef {
  .{
    name: "scratch_new",
    params: [.I32],
    results: [.Anyref],
    locals: [],
    body: [.RefNull(.None_), .LocalGet(0), .ArrayNew(t_array())],
  }
}

// scratch_get(buf: anyref, idx: i32) -> anyref
fn scratch_get_fn() FuncDef {
  .{
    name: "scratch_get",
    params: [.Anyref, .I32],
    results: [.Anyref],
    locals: [],
    body: [
      .LocalGet(0),
      .RefCast(false, .Named(t_array())),
      .LocalGet(1),
      .ArrayGet(t_array()),
    ],
  }
}

// scratch_set(buf: anyref, idx: i32, val: anyref) -> ()  (mutates in place)
fn scratch_set_fn() FuncDef {
  .{
    name: "scratch_set",
    params: [.Anyref, .I32, .Anyref],
    results: [],
    locals: [],
    body: [
      .LocalGet(0),
      .RefCast(false, .Named(t_array())),
      .LocalGet(1),
      .LocalGet(2),
      .ArraySet(t_array()),
    ],
  }
}
```

- [ ] **Step 4: Register the three ops in `module()`** in `boot/compiler/codegen/runtime/arr.tw`. Find `pub fn module()` (the funcs list starting `tailoff_fn(), vi_nav_fn(), ...`) and append the three entries to that list:

```tw
    scratch_new_fn(),
    scratch_get_fn(),
    scratch_set_fn(),
```

- [ ] **Step 5: Register the builtins** in `boot/compiler/builtins.tw`. In `builtin_specs()` (near the other `vector$...` `rt(...)` entries, e.g. after `vector$set_in_place`), add:

```tw
    rt("vector$scratch_new", "rt.arr", "scratch_new", .Some("Vector.scratch_new")),
    rt("vector$scratch_get", "rt.arr", "scratch_get", .Some("Vector.scratch_get")),
    rt("vector$scratch_set", "rt.arr", "scratch_set", .Some("Vector.scratch_set")),
```

In `builtin_abi(name)` (near the other `vector$...` arms), add (note `arr_n()`/`arr_()` exist; we use `.Anyref` for the buffer to match the ops' anyref params/results):

```tw
    "vector$scratch_new" => abi([.I32], [.Anyref]),
    "vector$scratch_get" => abi([.Anyref, .I32], [.Anyref]),
    "vector$scratch_set" => abi([.Anyref, .I32, .Anyref], []),
```

- [ ] **Step 6: Register the `Scratch<T>` type** in `boot/compiler/base_env.tw`. In `builtin_type_entries()`, **append** a new entry at the **end** of the list (after the `Set` entry, inside the `.concat([...])` block) so existing builtin `TypeId`s are not renumbered:

```tw
      builtin_type_entry(next_type_id, 1, .Record("Scratch", rtp_t, [])),
```

- [ ] **Step 7: Give `Scratch` an anyref layout** in `boot/compiler/codegen/wasm_layout.tw`, mirroring `Task` exactly. Add a helper next to `is_task_type`:

```tw
fn is_scratch_type(env: ResolvedEnv, tid: TypeId) Bool {
  case env.type_index["Scratch"] {
    .Some(idx) => env.types[idx].id.id == tid.id,
    .None => false,
  }
}
```

Then in `layout_of_named`, immediately after the `is_task_type` guard, add the parallel guard so `Scratch<T>` is an opaque anyref GC reference (its real GC type is `rt_types__Array`, reached only inside the ops):

```tw
  // Scratch<T> is an opaque GC reference to rt_types__Array (the dense sort
  // buffer). Layout stays Scalar(.WAnyref); the scratch_* ops cast internally.
  if is_scratch_type(env, tid) {
    return .Scalar(.WAnyref)
  }
```

Then `grep -n "is_task_type\|\"Task\"" boot/compiler` and, for **every** other site that special-cases `Task` for layout/representation purposes, add the parallel `Scratch` case with the same `.WAnyref`/anyref treatment. (If `is_task_type` only appears in `wasm_layout.tw`, this single guard is sufficient.)

- [ ] **Step 8: Add the stub signatures** in `boot/prelude/signatures/vector.tw`. Prelude signature stub bodies are **not** typechecked — `signatures/cell.tw::get` returns `0` for return type `T`, and `signatures/task.tw::await` returns `0` for return type `T`. So the bodies below (a bare `0` / `{}`, type-mismatched but accepted) are the established pattern:

```tw
/// INTERNAL: allocate a fresh dense mutable sort buffer of `n` null slots.
pub fn scratch_new<T>(n: Int) Scratch<T> {
  0
}

/// INTERNAL: read element `i` from a dense sort buffer.
pub fn scratch_get<T>(buf: Scratch<T>, i: Int) T {
  0
}

/// INTERNAL: write `v` to element `i` of a dense sort buffer, mutating in place.
pub fn scratch_set<T>(buf: Scratch<T>, i: Int, v: T) Void {}
```

If `make bundle-cli` (Step 10) reports a type error on the `scratch_new` body (i.e. stub bodies turn out to be checked after all), copy whatever form `signatures/task.tw::spawn` uses for its `Task<T>` return and report it as a concern.

- [ ] **Step 9: Format**

Run: `make fmt`
Expected: completes; re-running produces no further diff.

- [ ] **Step 10: Rebuild the compiler with the new ops/type**

Run: `make bundle-cli`
Expected: self-host loop converges (prints the fixed-point message), no error. A failure means the ops/type/layout are miswired — fix before continuing.

- [ ] **Step 11: Run the suite (round-trip test must now pass)**

Run: `make boot-test`
Expected: PASS, including `"scratch buffer round-trips and mutates in place"`. The in-place assertion (`scratch_set(buf,0,99)` then `scratch_get(buf,0) == 99`) confirms the buffer genuinely mutates (no COW), which is the whole point of the dense buffer.

- [ ] **Step 12: Commit**

```bash
git add boot/compiler/codegen/runtime/arr.tw boot/compiler/builtins.tw boot/compiler/base_env.tw boot/compiler/codegen/wasm_layout.tw boot/prelude/signatures/vector.tw boot/tests/suites/api_vector_suite.tw
git commit -m "boot: add internal Scratch<T> dense buffer + scratch_new/get/set ops

Opaque anyref-layout builtin type backed by the existing rt_types__Array GC
type (no new GC type); three trivial rt.arr primitives that array.new/get/set
over it, casting anyref->Array internally. Round-trip test confirms in-place
mutation. Groundwork for the dense-buffer stable merge sort."
```

---

## Task 2: Stage0 — mirror the `Scratch` type + three ops

Mirror Task 1 in the Rust stage0 reference compiler so the two compilers stay in lockstep (gated by the self-host fixed point). FuncIds are internal per-compiler and reconcile by canonical name — append new ids, never renumber; pick free `1000+` ids in stage0.

**Files:**
- Modify: `src/runtime/arr.rs`
- Modify: `src/ir/lower.rs`
- Modify: `src/intrinsics/registry.rs`
- Modify: `src/intrinsics/signatures.rs`
- Modify: `src/codegen/prelude.rs`
- Modify: stage0 builtin-type registration (the file(s) that register `Cell`/`Task` types and their anyref layout — find via the grep in Step 1)

- [ ] **Step 1: Locate the stage0 parallels.** Run:

```bash
grep -rn "set_in_place" src/runtime/arr.rs
grep -rn "\"Task\"\|Cell\b\|builtin.*type\|BuiltinType" src/types/ src/intrinsics/ src/ir/ src/codegen/ | grep -i "task\|cell" | head -40
grep -rn "rt_arr__set_in_place\|rt_arr__set\b" src/codegen/prelude.rs
```

Read the `set_in_place` op in `src/runtime/arr.rs` (the array.set exemplar), and how `Task`/`Cell` builtin types are registered (type entry + anyref layout/representation). These are the patterns to mirror.

- [ ] **Step 2: Add the three ops to `src/runtime/arr.rs`.** Mirror the boot `arr.tw` bodies from Task 1 Step 3 using stage0's `Instr` syntax (`Instr::ArrayNew{..}` / `Instr::ArrayGet{..}` / `Instr::ArraySet{..}` / `Instr::RefCast{nullable,heap}` / `Instr::RefNull(HeapType::None)`), following `set_in_place` as the exemplar. The three functions are `scratch_new` (`array.new` of `rt_types__Array` with a `ref.null none` init and the i32 length → returns the array), `scratch_get` (`ref.cast` param0 to `rt_types__Array`, `array.get`), `scratch_set` (`ref.cast` param0, `array.set`). Push all three into the `make()` funcs list.

- [ ] **Step 3: Register the intrinsics.** In `src/ir/lower.rs`, add three `prelude_ids` constants for `Vector.scratch_new`/`get`/`set` using free `1000+` ids. In `src/intrinsics/registry.rs`, add `spec!(...)` entries to `INTRINSIC_SPECS` and the names to `COMMON_BOOTSTRAP_FUNC_NAMES`. In `src/intrinsics/signatures.rs`, add the `contract()` arm (param/result kinds: `scratch_new` = `[i32] -> [ref]`, `scratch_get` = `[ref, i32] -> [ref]`, `scratch_set` = `[ref, i32, ref] -> []`, using the anyref/ref kinds stage0 uses for `vector$set_in_place`'s anyref args) and a doc-string arm. In `src/codegen/prelude.rs`, add the runtime entries pointing at `rt_arr__scratch_new`/`get`/`set`.

- [ ] **Step 4: Register the `Scratch` builtin type in stage0.** Mirror however `Cell`/`Task` are registered (a builtin type entry with arity 1 named `"Scratch"`) and give it the same anyref/opaque-GC-reference layout that `Task` gets, mapping its GC representation to the existing `Array` type (no new GC type). Match the canonical name `"Scratch"` so it reconciles with boot. (We call the ops as qualified `Vector.scratch_*`, not via `buf.method()`, so a `builtin_methods` method-resolution entry is **not** required — but if stage0 needs `Scratch` present in the type env for the prelude signatures to resolve, add only the type entry + layout.)

- [ ] **Step 5: Build stage0**

Run: `cargo build --release`
Expected: compiles cleanly.

- [ ] **Step 6: Self-host gate**

Run: `make bundle-cli`
Expected: converges to the fixed point with no error (this exercises stage0 building `boot/main.tw` and confirms boot↔stage0 emit identical wasm for the new ops/type).

- [ ] **Step 7: Run the suite**

Run: `make boot-test`
Expected: PASS, including the round-trip test from Task 1.

- [ ] **Step 8: Commit**

```bash
git add src/runtime/arr.rs src/ir/lower.rs src/intrinsics/registry.rs src/intrinsics/signatures.rs src/codegen/prelude.rs src/types/
git commit -m "stage0: mirror Scratch<T> type + scratch_new/get/set ops

Stage0 reference parity for the dense sort buffer primitives added to boot.
Self-host fixed point confirms identical codegen."
```

---

## Task 3: Prelude — rewrite `sort_by` as a stable dense bottom-up merge

Replace the recursive merge sort with: pre-scan (unchanged) → copy into a dense `Scratch` buffer → stable bottom-up merge over the buffer (ping-ponging with an auxiliary buffer) → freeze back to a `Vector`. `sort` is unchanged and inherits the new path.

**Files:**
- Modify: `boot/prelude/vector.tw`

- [ ] **Step 1: Delete the old helpers.** Remove the two `fn` definitions `merge_sorted` and `sort_by_range` (immediately above `pub fn sort_by`). Leave `sort` untouched.

- [ ] **Step 2: Add the dense-buffer helpers** in their place (above `pub fn sort_by`):

```tw
/// Copy a vector's elements into a fresh dense scratch buffer of length n.
fn scratch_from_vector<T>(xs: Vector<T>, n: Int) Scratch<T> {
  buf: Scratch<T> = Vector.scratch_new(n)
  i := 0

  for i < n {
    Vector.scratch_set(buf, i, xs[i])
    i = i + 1
  }

  buf
}

/// Build a vector from the first n elements of a dense scratch buffer.
fn scratch_freeze<T>(buf: Scratch<T>, n: Int) Vector<T> {
  collect i in range(n) {
    Vector.scratch_get(buf, i)
  }
}

/// Stably merge src[lo, mid) and src[mid, hi) into dst[lo, hi). On a tie
/// (cmp returns Eq) the left run's element is taken first, preserving input
/// order. Handles an empty right run (mid == hi) by copying the left run.
fn merge_run<T>(
  src: Scratch<T>,
  dst: Scratch<T>,
  lo: Int,
  mid: Int,
  hi: Int,
  cmp: fn(T, T) Order,
) Void {
  i := lo
  j := mid
  k := lo

  for k < hi {
    take_left := true

    if i >= mid {
      take_left = false
    } else {
      if j < hi {
        case cmp(Vector.scratch_get(src, i), Vector.scratch_get(src, j)) {
          .Gt => {
            take_left = false
          },
          _ => {
            take_left = true
          },
        }
      }
    }

    if take_left {
      Vector.scratch_set(dst, k, Vector.scratch_get(src, i))
      i = i + 1
    } else {
      Vector.scratch_set(dst, k, Vector.scratch_get(src, j))
      j = j + 1
    }

    k = k + 1
  }
}

/// In-place-over-scratch stable bottom-up merge sort. Ping-pongs between src
/// and aux by rebinding the handles each pass; returns the buffer holding the
/// fully sorted result.
fn merge_sort_dense<T>(
  src: Scratch<T>,
  aux: Scratch<T>,
  n: Int,
  cmp: fn(T, T) Order,
) Scratch<T> {
  width := 1

  for width < n {
    lo := 0

    for lo < n {
      mid := if lo + width < n {
        lo + width
      } else {
        n
      }
      hi := if lo + width + width < n {
        lo + width + width
      } else {
        n
      }
      merge_run(src, aux, lo, mid, hi, cmp)
      lo = lo + width + width
    }

    tmp := src
    src = aux
    aux = tmp
    width = width + width
  }

  src
}
```

- [ ] **Step 3: Rewrite `sort_by`'s heavy path.** Keep the pre-scan exactly; replace only the final `sort_by_range(xs, 0, n, cmp)` call. The full function should read:

```tw
/// Return a new vector sorted by comparator cmp.
///
/// Stable: elements that compare Equal keep their input order.
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

  src := scratch_from_vector(xs, n)
  aux: Scratch<T> = Vector.scratch_new(n)
  src = merge_sort_dense(src, aux, n, cmp)
  scratch_freeze(src, n)
}
```

- [ ] **Step 4: Format**

Run: `make fmt`
Expected: completes; re-running produces no further diff.

- [ ] **Step 5: Re-embed the prelude and rebuild the CLI**

Run: `make bundle-cli`
Expected: self-host loop converges, no error. The compiler itself uses `sort_by`, so a broken sort breaks `bundle-cli` — fix before continuing.

- [ ] **Step 6: Run the boot suite**

Run: `make boot-test`
Expected: PASS — including the existing `"sort_by handles large, duplicate-heavy, and adversarial inputs"` robustness test, the `Scratch` round-trip test, and the dataframe `query_suite` null-ordering test.

- [ ] **Step 7: Commit**

```bash
git add boot/prelude/vector.tw
git commit -m "prelude: sort_by via stable dense bottom-up merge over a scratch buffer

Replace the recursive merge sort (fresh persistent vector per level + trie
reads) with a stable bottom-up merge over a flat mutable Scratch buffer: copy
PVec -> buffer, ping-pong merge with an auxiliary buffer, freeze back to a
Vector. Keeps the ordered/reverse pre-scan and the generic closure comparator.
Restores stable sort (equal elements keep input order)."
```

---

## Task 4: Stability tests + benchmark gate + spec status

Add direct stability tests (Approach A could not pass these) and measure the perf gate. No production code changes unless a benchmark reveals a problem.

**Files:**
- Modify: `boot/tests/suites/api_vector_suite.tw`
- Modify: `docs/plans/native-sort-dense-merge.md`

- [ ] **Step 1: Add a stability test** in `boot/tests/suites/api_vector_suite.tw`, after the `Scratch` round-trip test (match the chaining/comma style). It sorts records by a key that produces many ties and asserts equal-key elements keep their original relative order (tracked by a `seq` field):

```tw
    .test(
      "sort_by is stable: equal keys keep input order",
      fn() {
        // 60 items, key = seq % 5 → 5 buckets of 12 ties each.
        items := collect seq in range(60) {
          .{ key: seq % 5, seq }
        }
        sorted := items.sort_by(fn(a, b) {
          Int.compare(a.key, b.key)
        })
        // Within each key bucket, seq must be strictly increasing (stable).
        ok := true
        i := 1

        for i < sorted.len() {
          prev := sorted[i - 1]
          cur := sorted[i]

          if prev.key == cur.key and prev.seq >= cur.seq {
            ok = false
          }

          i = i + 1
        }

        try assert.is_true(ok)
        try assert.equal(sorted.len(), 60)
        // keys overall non-decreasing
        sorted_keys := true
        j := 1

        for j < sorted.len() {
          if sorted[j - 1].key > sorted[j].key {
            sorted_keys = false
          }

          j = j + 1
        }

        try assert.is_true(sorted_keys)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Format and run the suite**

Run: `make fmt` then `make boot-test`
Expected: PASS, including the new stability test. (Stability is genuinely provided by the bottom-up merge; this test would FAIL on Approach A's quicksort, which is why it's added now.)

- [ ] **Step 3: Run the benchmark gate**

Run:
```bash
target/twk run examples/dataframe/bench/order_by_micro.tw
target/twk run examples/dataframe/bench/order_by_breakdown.tw
target/twk run examples/dataframe/bench/main.tw
```
Record `sort values` and `sort idx key` (micro), `sort idx by amount` / `full order_by` (breakdown), and `order_by` (main) at each N, focusing on N = 1,000,000.

Baselines to beat (N = 1,000,000): `sort values` ~829ms, `sort idx key` ~1674ms, `order_by` (main) ~2531ms.

- [ ] **Step 4: Evaluate the gate.**
  - **PASS:** `sort values` and `order_by` (main) drop materially from baseline, suites green, `bundle-cli` at fixed point → Approach C succeeds.
  - **WEAK GAIN:** if the dense path helps but `order_by` is still far from competitive and the breakdown shows the closure comparator dominating, that is the documented trigger for the **next** plan (typed Int-key argsort + comparator-shape recognition, parent plan Phase 4) — record the numbers and the finding; do not start Phase 4 here.

- [ ] **Step 5: Record results in the spec.** Update `docs/plans/native-sort-dense-merge.md`:
  - set the `Status` line to `implemented; kept` with the measured `order_by` number (or `dense path landed; closure cost dominates → escalating to typed argsort` with the numbers);
  - fill a before/after table for `sort values`, `sort idx key`, and `order_by` at N = 1,000,000 in the "Verification & success gate" section;
  - confirm the stability note (now a stable sort again).

- [ ] **Step 6: Commit**

```bash
git add boot/tests/suites/api_vector_suite.tw docs/plans/native-sort-dense-merge.md
git commit -m "test+docs: stability test for dense merge sort; record benchmark gate results"
```

---

## Self-review notes

- **Spec coverage:** dense flat-buffer + stable bottom-up merge (Task 3); three runtime primitives `scratch_new`/`get`/`set` reusing `rt_types__Array`, no new GC type (Task 1/2); `Scratch<T>` opaque anyref type modeled on `Task` (Task 1 Steps 6–7); pre-scan retained (Task 3 Step 3); generic closure comparator kept (Task 3); stability restored + tested (Task 3 Step 3 doc comment, Task 4 Step 1); cross-compiler boot+stage0 parity (Tasks 1–2); benchmark gate + escalation trigger (Task 4); `from_vector`/`freeze` realized in Twinkle (Task 3 Step 2) — a deliberate simplification of the spec's "five intrinsics" down to three runtime ops, noted here, with no change to the architecture or the dense-buffer guarantee. All spec sections covered.
- **Placeholder scan:** Task 1 Step 8 flags a genuine open question (the exact non-typechecked stub body for a `Scratch<T>`-returning signature) with a concrete fallback (model on `task$spawn`) and a report-as-concern instruction — this is a real branch point, not a hidden TODO. No other placeholders.
- **Type/name consistency:** `Scratch<T>`, `Vector.scratch_new`/`scratch_get`/`scratch_set`, and helpers `scratch_from_vector`/`scratch_freeze`/`merge_run`/`merge_sort_dense` are used with identical names and signatures across all tasks. ABI/op signatures (`[.I32]→[.Anyref]`, `[.Anyref,.I32]→[.Anyref]`, `[.Anyref,.I32,.Anyref]→[]`) match between boot (Task 1) and stage0 (Task 2). `sort` is left unchanged in every task that touches `vector.tw`.
- **Risk note:** the load-bearing assumptions are (a) prelude signature stub bodies are not typechecked (confirmed against `Cell`/`Task` stubs), and (b) an opaque anyref builtin reaching its GC type only inside ops works (confirmed: `Task` does exactly this). Both are validated by `make bundle-cli` reaching its fixed point at the end of Task 1.
```
