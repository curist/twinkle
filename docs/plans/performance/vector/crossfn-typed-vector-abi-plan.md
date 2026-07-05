# Cross-function Typed-Vector ABI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let typed `PVecI64` vectors flow through function calls and the `gather`/`take` read path instead of boxing at every boundary, so the dataframe column stays unboxed from build through read.

**Architecture:** Two sequenced stages. **Stage 1** adds a typed `gather_i64` runtime op (plus a raw-i64 builder push) and routes `gather` to it when its receiver is already typed — contained, independently shippable, no monomorphization change. **Stage 2** adds a backend-level *specialize-by-representation* pass over `PreparedFunc`s: a param/return-typeability fixpoint, `f$i64` variant emission, and call-site ABI selection that relaxes the escape guard only for param-typeable slots. There is a **re-measure checkpoint** between the stages.

**Tech Stack:** Twinkle (`.tw`) boot compiler. Runtime ops in `boot/compiler/codegen/runtime/arr.tw` (a Wasm-instruction DSL). Backend passes in `boot/compiler/backend/`. Tests are `@std.testing` suites under `boot/tests/suites/`. Build/verify via `make bundle-cli`, `make boot-test`. WAT inspection via a `.wat` output path.

**Spec:** [crossfn-typed-vector-abi.md](crossfn-typed-vector-abi.md). **Evidence:** [crossfn-abi-instrumentation.md](crossfn-abi-instrumentation.md).

**Load-bearing invariant (do not violate):** A `Vector<Int>` reaching a genuinely-erased boundary (`anyref`, closure env, generic container, erased builtin-sum variant) MUST box at the crossing. Stage 2 adds exactly one new non-erased boundary (a monomorphic function's param/return ABI) and relaxes the `route_typed_vec` escape guard in exactly one place (arg into a param-typeable slot). Nothing else.

---

## Reference recipes (read before starting)

- Runtime-op wiring: memory `reference_runtime_builtin_wiring.md` — file map, append-at-end FuncId discipline, boot + stage0 parity, build/verify loop.
- The existing typed-read machinery you are extending: `boot/compiler/backend/route_typed_vec.tw` (per-function pass), driven from `boot/compiler/backend/prepare.tw:97-104`.
- The i64 runtime family: `boot/compiler/codegen/runtime/arr.tw` — `family_i64()` (line ~1338) produces `get_i64`/`len_i64`/`builder_new_i64`/`builder_freeze_i64`; `builder_push_i64_fn()` (line ~1838) takes a **boxed** element and unboxes internally; `gather_fn()` (line ~3566) is the boxed gather to mirror; helpers `pvec_i64_null()`/`pvec_i64_ref()` (line ~91), `pvec_null()`/`pvec_ref()` (line ~45), `arr_null()`, constant `t_BOXED_INT`.

---

# STAGE 1 — Typed `gather` twin

`table.take` calls `column.gather` under the hood, so the direct-read path is one runtime op. `column.gather` is `.IntCol(v) => ColData.IntCol(v.gather(idx))`; `v` is the already-typed `PVecI64` payload (Milestone A). Today it boxes: `struct.get ColData 1 → box_i64 → rt_arr__gather`. Goal: `struct.get ColData 1 → rt_arr__gather_i64`, reads unboxed, output typed.

## Task 1.1: Raw-i64 builder push runtime op

`builder_push_i64` takes a boxed `BoxedInt` and unboxes it. `gather_i64` will read raw i64 via `get_i64`, so it needs to push raw i64 without a transient re-box. Add `builder_push_i64_raw(builder, i64)`.

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw` (add `builder_push_i64_raw_fn`, register it)

- [ ] **Step 1: Add the runtime op**

Add this function immediately after `builder_push_i64_fn` (line ~1838). It is `builder_push_i64_fn` with the element already unboxed: param `.I64` instead of `.Anyref`, and the first three instructions (`LocalGet(1)`, `RefCast(false, .Named(tbi))`, `StructGet(tbi, 0)`) replaced by a single `LocalGet(1)`. Copy the entire body of `builder_push_i64_fn` and apply exactly that change:

```
fn builder_push_i64_raw_fn() FuncDef {
  ta := t_ARRAY
  tai := t_ARRAY_I64

  // p0=builder, p1=elem (raw i64); L2=tail_buf, L3=tail_len, L4=pvec_so_far,
  // L5=elem. Same as builder_push_i64 but the element arrives unboxed, so the
  // typed reader (get_i64) can feed it directly with no transient BoxedInt.
  .{
    name: "builder_push_i64_raw",
    params: [arr_null(), .I64],
    results: [],
    // Same locals as builder_push_i64_fn (keep .I64 at L5 so the copied body's
    // LocalGet/Set indices are unchanged); L5 is now assigned from the param.
    locals: [arr_i64_null(), .I32, pvec_i64_null(), .I64],
    body: [
      .LocalGet(1),
      .LocalSet(5),
      // ... REMAINDER IDENTICAL to builder_push_i64_fn body from `.LocalGet(0)`
      // `.RefAsNonNull` `.I32Const(2)` onward. Copy it verbatim.
    ],
  }
}
```

Params are L0 (`builder`) and L1 (`elem`, raw i64); locals start at L2. Keeping `builder_push_i64_fn`'s exact `locals` list means the verbatim-copied tail body needs no index edits.

- [ ] **Step 2: Register it in the runtime funcs list**

In the `funcs := [ ... ]` list (line ~120-175), add `builder_push_i64_raw_fn(),` immediately after `builder_push_i64_fn(),`.

- [ ] **Step 3: Build to confirm the runtime compiles**

Run: `cargo run --release -- build boot/main.tw -o /tmp/boot-main.wasm 2>&1 | tail -5`
Expected: `WASM output: ...` with no error (stage0 compiles the new Twinkle runtime source; no Rust change needed — it is ordinary Twinkle).

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "runtime: add builder_push_i64_raw (raw-i64 typed builder push)"
```

## Task 1.2: `gather_i64` runtime op

**Files:**
- Modify: `boot/compiler/codegen/runtime/arr.tw` (add `gather_i64_fn`, register it)

- [ ] **Step 1: Add the runtime op**

Add immediately after `gather_fn` (line ~3566):

```
// gather_i64(vec: PVecI64?, idx: PVec?) -> PVecI64
// Like gather, but reads vec via get_i64 (raw i64 leaf, no BoxedInt chase) and
// builds a typed result via builder_push_i64_raw. idx stays boxed (the
// permutation vector; read once per output element and truncated to i32).
fn gather_i64_fn() FuncDef {
  // p0=vec (PVecI64), p1=idx (PVec); L2=n, L3=builder, L4=i, L5=k
  .{
    name: "gather_i64",
    params: [pvec_i64_null(), pvec_null()],
    results: [pvec_i64_ref()],
    locals: [.I32, arr_null(), .I32, .I32],
    body: [
      .LocalGet(1),
      .Call("len"),
      .LocalSet(2),
      .Call("builder_new_i64"),
      .LocalSet(3),
      .I32Const(0),
      .LocalSet(4),
      .Block(
        "brk",
        .None,
        [
          .Loop(
            "lp",
            .None,
            [
              .LocalGet(4),
              .LocalGet(2),
              .I32GeS,
              .BrIf("brk"),
              .LocalGet(1),
              .LocalGet(4),
              .Call("get"),
              .RefCast(false, .Named(t_BOXED_INT)),
              .StructGet(t_BOXED_INT, 0),
              .I32WrapI64,
              .LocalSet(5),
              .LocalGet(3),
              .LocalGet(0),
              .LocalGet(5),
              .Call("get_i64"),
              .Call("builder_push_i64_raw"),
              .LocalGet(4),
              .I32Const(1),
              .I32Add,
              .LocalSet(4),
              .Br("lp"),
            ],
          ),
        ],
      ),
      .LocalGet(3),
      .Call("builder_freeze_i64"),
    ],
  }
}
```

- [ ] **Step 2: Register it in the runtime funcs list**

Add `gather_i64_fn(),` immediately after `gather_fn(),` in the `funcs` list.

- [ ] **Step 3: Build to confirm it compiles**

Run: `cargo run --release -- build boot/main.tw -o /tmp/boot-main.wasm 2>&1 | tail -3`
Expected: `WASM output: ...`, no error.

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/codegen/runtime/arr.tw
git commit -m "runtime: add gather_i64 (typed gather over PVecI64)"
```

## Task 1.3: Register the `vector$gather_i64` builtin

**Files:**
- Modify: `boot/compiler/builtins.tw:137` (ABI table) and `:548` (rt registration)

- [ ] **Step 1: Add the ABI entry**

After line 137 (`"vector$gather" => abi([pvec_n(), pvec_n()], [pvec_()]),`) add:

```
    "vector$gather_i64" => abi([pvec_i64_n(), pvec_n()], [pvec_i64_()]),
```

(Confirm the helper names `pvec_i64_n()` / `pvec_i64_()` exist in this file — they are used by the neighbouring `vector$len_i64` / `vector$get_i64` entries at lines 141-142.)

- [ ] **Step 2: Add the rt registration**

After line 548 (`rt("vector$gather", "rt.arr", "gather", .Some("Vector.gather")),`) add:

```
    rt("vector$gather_i64", "rt.arr", "gather_i64", .None),
```

`.None` for the surface method: `gather_i64` is never called from source; it is only the routing target. Match the surrounding `rt(...)` call shape exactly (check the arity of the neighbours; if `_i64` runtime ops there use a different registration form, mirror `vector$len_i64`'s registration instead).

- [ ] **Step 3: Build to confirm registration resolves**

Run: `cargo run --release -- build boot/main.tw -o /tmp/boot-main.wasm 2>&1 | tail -3`
Expected: `WASM output: ...`, no error.

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/builtins.tw
git commit -m "builtins: register vector\$gather_i64 (typed gather routing target)"
```

## Task 1.4: Route `gather` → `gather_i64` when the receiver is typed

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw` (`RouteIds`, `route_ids`, `rewrite_op`)
- Test: `boot/tests/suites/route_typed_vec_suite.tw` (create if absent; otherwise add to `backend_repr_suite.tw`)

- [ ] **Step 1: Write the failing test**

The cleanest testable unit is `rewrite_op`: given an `.ACall(.AGlobalFunc(gather_id), [vec_slot, idx_slot])` whose `vec_slot` is in `eligible_v`, it returns a call to `gather_i64`. Add a test that constructs a minimal `RouteIds` and `PreparedOp` and asserts the swap. Mirror the construction style already used in `backend_repr_suite.tw`. If `rewrite_op` is not `pub`, make it `pub` for the test (it is an internal helper; exporting for test is consistent with the file's other exported helpers).

```
use compiler.backend.route_typed_vec.{rewrite_op}   // export if needed

pub fn suite() runner.Suite {
  runner.suite("route typed gather").test(
    "gather over eligible_v receiver swaps to gather_i64",
    fn() {
      ids := .{ /* fill builder_*/len/gather ids with distinct ints, gather=90, gather_i64=91 */ }
      eligible_v: Dict<Int, Bool> = Dict.new()
      eligible_v[7] = true   // vec slot id 7 is typed
      op := .ACall(.AGlobalFunc(.{ id: 90 }), [.ASlot(.{ id: 7 }), .ASlot(.{ id: 8 })])
      out := rewrite_op(99, op, eligible_v, Dict.new(), ids)
      case out {
        .ACall(.AGlobalFunc(fid), _) => { try assert.is_true(fid.id == 91); .Ok({}) },
        _ => .Err("expected ACall"),
      }
    },
  )
}
```

Register the suite in `boot/tests/main.tw` (add a `use .suites.route_typed_vec_suite` and include `route_typed_vec_suite.suite()` in the suite list), following the exact pattern of `repr_policy_suite` there (lines 22, 207).

- [ ] **Step 2: Run it and confirm it fails**

Run: `make bundle-cli >/dev/null 2>&1 && target/twk test 2>&1 | tail -5`
Expected: FAIL — `rewrite_op` does not yet swap `gather` (either compile error on the added `gather`/`gather_i64` fields, or the assertion fails).

- [ ] **Step 3: Extend `RouteIds` + `route_ids`**

In `route_typed_vec.tw`, add two fields to `type RouteIds` (after `len_i64`):

```
  gather: Int,
  gather_i64: Int,
```

and in `route_ids` (after the `len_i64:` line):

```
    gather: builtins.id("vector$gather").id,
    gather_i64: builtins.id("vector$gather_i64").id,
```

- [ ] **Step 4: Add the `rewrite_op` swap arm**

In `rewrite_op`'s `cond` (line ~1362), add before the `_ => .None` arm:

```
        fid.id == ids.gather and args.len() == 2 and arg0_in(args, eligible_v) => .Some(
          ids.gather_i64,
        ),
```

`arg0_in` already checks `args[0]` (the `vec` receiver) is in the set. The result slot is retyped to `PVecI64` by the existing slot-retyping loop provided it is in `eligible_v` — which it is, because the `gather` result flows into the typed `IntCol` payload store (the `payload_read`/`all_payload_keys_typed` path already marks it). No extra eligibility wiring needed.

- [ ] **Step 5: Run the test and confirm it passes**

Run: `make bundle-cli >/dev/null 2>&1 && target/twk test 2>&1 | tail -5`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/backend/route_typed_vec.tw boot/tests/suites/route_typed_vec_suite.tw boot/tests/main.tw
git commit -m "route: swap gather -> gather_i64 for typed receivers"
```

## Task 1.5: End-to-end WAT probe + full suite

**Files:**
- Create: `examples/performance/sort-bench/typed_gather_probe.tw`

- [ ] **Step 1: Write the probe program**

A minimal build-then-gather over an `IntCol`-style typed payload, mirroring `typed_variant_payload_probe.tw`. It must construct a `Vector<Int>` into a typed variant payload and `gather` it with an index vector, then read the result:

```
type Col = { IntCol(Vector<Int>) }

fn gather_col(c: Col, idx: Vector<Int>) Col {
  case c {
    .IntCol(v) => .IntCol(v.gather(idx)),
  }
}

data := collect i in range(1000) { i * 2 }
idx := collect i in range(1000) { 999 - i }
col := Col.IntCol(data)
out := gather_col(col, idx)
sum := 0
case out { .IntCol(v) => for x in v { sum = sum + x } }
println("sum=${sum}")
```

- [ ] **Step 2: Build to WAT and assert typed gather, no box on the gather**

Run:
```bash
target/twk build examples/performance/sort-bench/typed_gather_probe.tw -o /tmp/g.wat
grep -c rt_arr__gather_i64 /tmp/g.wat   # expect >= 1
```
Expected: `>= 1` (the typed gather fired). Also inspect the `gather_col` function body in the WAT to confirm the `struct.get ...ColData... → box_i64 → rt_arr__gather` sequence is gone, replaced by `rt_arr__gather_i64`.

- [ ] **Step 3: Run the probe for correctness**

Run: `target/twk run examples/performance/sort-bench/typed_gather_probe.tw`
Expected: `sum=999000` (sum of 0,2,4,…,1998 = 1000*999).

- [ ] **Step 4: Self-host + full suite**

Run: `make bundle-cli 2>&1 | tail -3 && make boot-test 2>&1 | tail -2`
Expected: `Fixed point reached`; all boot tests pass.

- [ ] **Step 5: Commit**

```bash
git add examples/performance/sort-bench/typed_gather_probe.tw
git commit -m "test: end-to-end typed gather probe"
```

## Checkpoint A — re-measure before Stage 2

- [ ] Run the dataframe bench and record numbers:

```bash
target/twk run examples/performance/dataframe/bench/main.tw 2>&1 | tee /tmp/bench-stage1.txt
```

Compare `order_by` against the ~2403ms baseline. Expect the gather/take contribution (~830ms) to shrink. **Record the actual delta in the umbrella doc** ([typed-vector-representation.md](typed-vector-representation.md)). If gather did not move, inspect the WAT: confirm `gather_i64` is actually reached in the dataframe `column.gather` (the payload read must be `eligible_v`); a common cause is the `gather` result not being marked typed because the payload store site was not in `typed_payloads`. Do NOT start Stage 2 until Stage 1's win is confirmed and recorded.

---

# STAGE 2 — Cross-call typed ABI (specialize-by-representation)

A whole-program backend pass over `PreparedFunc`s, inserted in `prepare.tw` alongside `analyze_typed_fields` / `analyze_typed_payloads` (lines 97-104). It mirrors those passes' shape: a whole-program analysis produces a fact set, which `route_typed_vectors` consumes.

**Read first:** `analyze_typed_payloads` and `analyze_typed_fields` (find them via `grep -rn 'fn analyze_typed_payloads\|fn analyze_typed_fields' boot/compiler/backend/`). Stage 2's `analyze_typed_params` is the same kind of whole-program scan; study their structure and reuse their traversal helpers.

## Task 2.1: `param_typeable` / `return_typeable` analysis (local facts, no emission)

**Files:**
- Create: `boot/compiler/backend/typed_param_abi.tw`
- Test: `boot/tests/suites/typed_param_abi_suite.tw`

- [ ] **Step 1: Define the fact types and the local classifier**

The analysis answers, per function: which `Vector<Int>` params are used only in typed-compatible ways, and whether the return value is a typed slot. Types:

```
//! Cross-call typed-vector ABI analysis (Stage 2). Pure over PreparedFunc; no
//! IR mutation here — emission and call rewriting live in the router.

pub type ParamAbi = .{
  // func_id.id -> set of param indices that may take PVecI64
  typeable_params: Dict<String, Vector<Int>>,
  // func_id.id (as String) -> return is a typed PVecI64 slot
  typeable_return: Dict<String, Bool>,
}
```

`param_uses_typed_only(pf, param_slot_id, builtins)` returns `Bool`: walk `pf.body`; the param slot is typed-compatible iff every use is one of: index read (`get`/`get_i64`), `len`, stored into a typed field/payload (reuse the `typed_fields`/`typed_payloads` sets), returned as the function result, or passed as an argument into another function's typeable param slot. Any other use (captured into a `ClosureEnv`, placed in a generic container, passed to an `anyref` param, passed to a non-typeable param) makes it non-typeable. For the FIRST iteration, treat "passed to another function's param" as non-typeable (conservative), so this task needs no cross-function knowledge; Task 2.2 adds the fixpoint.

- [ ] **Step 2: Write the failing test**

```
// A function whose Vector<Int> param is only indexed + len -> typeable.
// A function that captures its param into a closure -> not typeable.
```

Construct two small `PreparedFunc`s (mirror the fixtures in `backend_repr_suite.tw`) and assert `param_uses_typed_only` returns `true` for the first, `false` for the second.

- [ ] **Step 3: Run it, confirm it fails** — `make bundle-cli && target/twk test` → FAIL.
- [ ] **Step 4: Implement `param_uses_typed_only`** per Step 1's rules, reusing the escape-classification helpers already in `route_typed_vec.tw` (`v_group_escapes` / `op_group_escapes`) — a param is typeable iff its slot-group does not escape by any route except index/len/typed-store/return. This is the SAME predicate `route_func` already computes for a local, applied to a param slot; factor the shared predicate rather than duplicating it.
- [ ] **Step 5: Run the test, confirm it passes.**
- [ ] **Step 6: Commit** — `feat: param/return typeability classifier (Stage 2 analysis)`

## Task 2.2: Bounded monotone fixpoint over the call graph

**Files:**
- Modify: `boot/compiler/backend/typed_param_abi.tw`
- Test: `boot/tests/suites/typed_param_abi_suite.tw`

- [ ] **Step 1: Write the failing test** — a two-function chain where `f`'s param is passed straight into `g`'s (typeable) param; after the fixpoint, `f`'s param is typeable too. With the Task 2.1 conservative rule it is `false`; after the fixpoint it is `true`.
- [ ] **Step 2: Run it, confirm it fails.**
- [ ] **Step 3: Implement `analyze_typed_params(funcs, builtins, typed_fields, typed_payloads) ParamAbi`** — iterate: start with all params non-typeable; each round, mark a param typeable if `param_uses_typed_only` holds treating "passed to a currently-typeable param" as compatible. Repeat until a round adds nothing or an iteration cap (e.g. `funcs.len() + 4`) is hit. Typeability only grows, so it converges.
- [ ] **Step 4: Run the test, confirm it passes.**
- [ ] **Step 5: Commit** — `feat: call-graph fixpoint for cross-call param typeability`

## Task 2.3: Escape-guard relaxation — arg into a typeable param is not an escape

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw` (thread `ParamAbi` into the escape classifier)
- Test: `boot/tests/suites/route_typed_vec_suite.tw`

- [ ] **Step 1: Write the failing test** — a local `Vector<Int>` passed only into a typeable param must classify as non-escaping (so `route_func` retypes it). Today it escapes.
- [ ] **Step 2: Run it, confirm it fails.**
- [ ] **Step 3: Thread `ParamAbi` into `route_typed_vectors` → `route_func` → `classify_v_group` / `op_group_escapes`.** In the arg-of-call escape check, when the callee argument position is a typeable param (`ParamAbi.typeable_params[callee_id]` contains the index), do NOT mark the slot as escaping. This is the single relaxation point named in the spec. Every other escape route stays.
- [ ] **Step 4: Run the test, confirm it passes.**
- [ ] **Step 5: Self-host + full suite** — `make bundle-cli` fixed point; `make boot-test` green. This proves the relaxation did not mis-type any boot-compiler value.
- [ ] **Step 6: Commit** — `route: don't treat arg-into-typeable-param as an escape`

## Task 2.4: Emit `f$i64` repr variants and select the ABI at call sites

**Files:**
- Modify: `boot/compiler/backend/typed_param_abi.tw` (variant emission)
- Modify: `boot/compiler/backend/prepare.tw` (drive the pass at line ~104)
- Modify: `boot/compiler/backend/route_typed_vec.tw` (rewrite typed calls to the variant)
- Test: `boot/tests/suites/typed_param_abi_suite.tw`

- [ ] **Step 1: Write the failing test** — given a function with a typeable param and a caller passing a typed vector, after the pass there exist two `PreparedFunc`s (original boxed + `$i64` variant with the param slot retyped to `PVecI64`), and the typed call targets the variant's `func_id`.
- [ ] **Step 2: Run it, confirm it fails.**
- [ ] **Step 3: Implement demand-driven emission.** `emit_repr_variants(funcs, param_abi, ...) -> (funcs2, redirect)`:
  - For each call site whose actual arg is typed (`eligible_v`) and lands in a typeable param, demand a variant of the callee.
  - For each demanded `f`, clone its `PreparedFunc` with a fresh `func_id` (derive via the module's id allocator — mirror how monomorphization/closure passes mint new `FuncId`s; find the allocator with `grep -rn 'fresh.*func\|next_func_id\|alloc.*FuncId' boot/compiler/backend/`), retype the typeable param slots (and, if `typeable_return`, the result slot) to `PVecI64`, then run `route_func` on the clone so its body stays typed internally.
  - Return a `redirect: Dict<String, Int>` from `(caller-site callee id)` to the variant id.
  - Emission is demand-driven, so functions with no typed caller get no variant. DCE drops any variant that ends up unreferenced.
- [ ] **Step 4: Rewrite call sites** — in `route_func`, after eligibility is known, rewrite a matching `.ACall(.AGlobalFunc(fid), args)` to the variant `fid` from `redirect`, and drop the return-position `box_i64` when the variant returns `PVecI64` and the caller's result slot is typed.
- [ ] **Step 5: Drive the pass in `prepare.tw`** — after `route_typed_vectors` (line 104), compute `param_abi := analyze_typed_params(...)`, then thread it back so `route_typed_vectors` sees it. Order: `analyze_typed_params` needs `typed_fields`/`typed_payloads` (already computed at 97/103) and must run before/with the routing that consumes it. Restructure the 97-104 block so the relaxation (Task 2.3) and emission both see `param_abi`; keep it a single well-commented sequence.
- [ ] **Step 6: Run the test, confirm it passes.**
- [ ] **Step 7: Self-host + full suite** — `make bundle-cli` fixed point; `make boot-test` green.
- [ ] **Step 8: Commit** — `feat: emit PVecI64-ABI function variants + select at typed call sites`

## Task 2.5: Guard test + build-path WAT probe + bench

**Files:**
- Create: `examples/performance/sort-bench/typed_param_abi_probe.tw`
- Create: `examples/performance/sort-bench/typed_param_capture_guard.tw`

- [ ] **Step 1: Build-path probe** — a program that builds a `Vector<Int>` in a loop and passes it into a function that stores it into a typed payload (the `gen.table → int_col` shape). Build to WAT; assert the builder-loop local is `PVecI64` (no `box_i64` before the call) and the callee has a `$i64` variant:

```bash
target/twk build examples/performance/sort-bench/typed_param_abi_probe.tw -o /tmp/p.wat
grep -c 'gather_i64\|builder_freeze_i64' /tmp/p.wat   # typed build present
grep -c box_i64 /tmp/p.wat                             # fewer than the boxed baseline
```

- [ ] **Step 2: Escape-guard regression probe** — mirror `typed_payload_capture_guard.tw`: pass a typed vector into a function that captures it into a closure and reads it in a loop. It MUST stay boxed (not typed). Assert runtime is ~ms, not seconds:

```bash
timeout 15 target/twk run examples/performance/sort-bench/typed_param_capture_guard.tw
```
Expected: a few ms (the reverted-bug guard — the captured param must not be typed).

- [ ] **Step 3: Bench** — `target/twk run examples/performance/dataframe/bench/main.tw`; record `order_by` and `filter`/`group_by`/`join`. Expect the build path to move; expect `order_by` umbrella to move materially only after step 2 (typed closure env). Record deltas in [typed-vector-representation.md](typed-vector-representation.md).
- [ ] **Step 4: Commit** — `test: Stage 2 build-path probe + capture guard + bench notes`

## Checkpoint B — wrap up

- [ ] Update [typed-vector-representation.md](typed-vector-representation.md) status and [typed-vector-continue-here.md](typed-vector-continue-here.md): mark Stage 1/2 done, note that step 2 (typed closure env / comparator, ~1317ms) is now unblocked because the key column can reach `sort_indices` typed.
- [ ] Final `make bundle-cli` fixed point + `make boot-test` green + `cargo test --release` (targeted) as a last guard.

---

## Notes on stage0 parity

Stage 1's runtime ops (`builder_push_i64_raw`, `gather_i64`) are ordinary Twinkle inside `arr.tw`; stage0 compiles them as source, so no Rust change is expected. If `make stage2` fails because the boot compiler emits a `gather_i64` call while compiling `boot/main.tw` and stage0 lacks it, follow the runtime-builtin-wiring recipe's stage0 trap-stub step. Stage 2 is pure boot-backend codegen (per the "no stage0 parity for boot-codegen opts" rule) — but the same `make stage2` fixed-point check is the authoritative guard and is already in Tasks 2.3/2.4.
