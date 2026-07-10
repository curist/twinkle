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

**FuncId discipline (critical):** `builtins.tw:444` — "Order determines FuncId assignment (0-based, sequential)." The registration order in `builtin_specs()` assigns each builtin's FuncId, and stage0 has FuncId expectations that must not shift. **The `rt(...)` registration MUST be appended at the very end of `builtin_specs()`** — inserting it mid-list renumbers every later builtin and breaks the stage0 bootstrap. The `abi(...)` entry in the match table is keyed by *name*, not position, so it may go wherever it reads best (next to `vector$gather`).

**Files:**
- Modify: `boot/compiler/builtins.tw` — ABI match table (near `:137`) and the END of `builtin_specs()`

- [ ] **Step 1: Add the ABI entry (position-independent)**

After line 137 (`"vector$gather" => abi([pvec_n(), pvec_n()], [pvec_()]),`) add:

```
    "vector$gather_i64" => abi([pvec_i64_n(), pvec_n()], [pvec_i64_()]),
```

(Confirm the helper names `pvec_i64_n()` / `pvec_i64_()` exist in this file — they are used by the neighbouring `vector$len_i64` / `vector$get_i64` entries at lines 141-142.)

- [ ] **Step 2: Append the rt registration at the END of `builtin_specs()`**

Find the last `rt(...)`/`intr(...)` entry in `builtin_specs()` (the function whose header comment is "Order determines FuncId assignment") and append AFTER it:

```
    rt("vector$gather_i64", "rt.arr", "gather_i64", .None),
```

`.None` for the surface method: `gather_i64` is never called from source; it is only the routing target. Match the surrounding `rt(...)` call shape exactly (mirror `vector$len_i64`'s registration form if the arity differs).

- [ ] **Step 3: Build to confirm registration resolves AND stage0 still bootstraps**

Run: `cargo run --release -- build boot/main.tw -o /tmp/boot-main.wasm 2>&1 | tail -3`
Expected: `WASM output: ...`, no error. (Append-at-end means no earlier FuncId moved, so stage0 dispatch is unaffected.)

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/builtins.tw
git commit -m "builtins: register vector\$gather_i64 (typed gather routing target)"
```

## Task 1.4: Route `gather` → `gather_i64` when the receiver is typed

**Why the naive one-arm swap does NOT work (verified in code):** the payload read `v` in `.IntCol(v) => ColData.IntCol(v.gather(idx))` only becomes `eligible_v` if `result_consumed_typed_only(v)` holds, i.e. `v` does not escape. But `op_group_escapes` (route_typed_vec.tw:607) whitelists only `len` (line 611); a `gather(v, idx)` call falls through to `atoms_contain_any(args, vs)` (line 617) and counts `v` as **escaping**. So `v` stays boxed, `eligible_v` never contains it, and a `rewrite_op` swap gated on `arg0_in(args, eligible_v)` never fires on the real path. A seeded `rewrite_op` unit test would pass while the real `IntCol(v.gather(idx))` stays boxed. Stage 1 must therefore (a) whitelist `gather` as a non-escaping use of a typed receiver, and (b) mark the `gather` **result** slot `eligible_v` so it retypes to `PVecI64`, stores typed into the payload, and routes to `gather_i64`.

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw` (`RouteIds`, `route_ids`, `op_group_escapes`, result-eligibility collection, `rewrite_op`)
- Test: `boot/tests/suites/route_typed_vec_suite.tw` (new; end-to-end via `prepare`/routing, not a seeded unit)

- [ ] **Step 1: Extend `RouteIds` + `route_ids`**

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

Because `op_group_escapes`/`rewrite_op` currently only receive `len_id` (not the full `RouteIds`), pass the `gather` id (and `gather_i64`) through to them. Thread `ids: RouteIds` into `v_group_escapes`/`op_group_escapes` (they already take `len_id`; widen to also know `gather`), or pass `gather_id` alongside `len_id`. Keep the change minimal and mechanical.

- [ ] **Step 2: Whitelist `gather` in the escape classifier**

In `op_group_escapes` (line ~607), extend the `.AGlobalFunc(fid)` arm so a `gather` whose receiver (`args[0]`) is the tracked vector is NOT an escape (mirror the `len` whitelist at line 611), while the index arg (`args[1]`) is unrestricted:

```
      .AGlobalFunc(fid) => if fid.id == len_id and args.len() == 1 and slot_in(args[0], vs) {
        false
      } else if fid.id == gather_id and args.len() == 2 and slot_in(args[0], vs) {
        // gather(v, idx): reading v is a typed-safe use (like len). The RESULT is
        // a new typed vector handled by result-eligibility below; v itself does
        // not escape through the call.
        false
      } else {
        if user_direct_call_accepts_boxed_arg(fid, args, vs, builtins) {
          false
        } else {
          atoms_contain_any(args, vs) or slot_in(callee, vs)
        }
      },
```

- [ ] **Step 3: Mark the `gather` result slot eligible**

Add a collection (mirroring `collect_typed_payload_reads` / `collect_candidates`) that, for every `.Let(result_slot, .ACall(gather, [v, idx]), _)` where `v`'s slot is already in `eligible_v`, adds `result_slot` to `eligible_v`. Because a gather result can feed another gather, run this to a fixpoint over the function body (bounded by slot count). Insert it in `route_func` (route_typed_vec.tw:79-199) after the `payload_read_sids` join (line ~162) and before the `eligible_v.keys().len() == 0` early-out (line ~168), so gather results ride the same slot-retyping loop.

- [ ] **Step 4: Add the `rewrite_op` swap arm**

In `rewrite_op`'s `cond` (line ~1362), add before the `_ => .None` arm:

```
        fid.id == ids.gather and args.len() == 2 and arg0_in(args, eligible_v) => .Some(
          ids.gather_i64,
        ),
```

- [ ] **Step 5: Write the END-TO-END test (not a seeded unit)**

The test must drive real routing so it catches the eligibility gap the seeded unit would miss. Build a tiny module through the prepare/route pipeline (or compile a fixture to WAT) and assert the typed gather is chosen. The simplest robust form is a WAT assertion on a fixture — put it in Task 1.5's probe and additionally add a routing-level suite test that constructs a `PreparedFunc` with a typed payload read feeding `gather` feeding a typed payload store, runs `route_func`, and asserts the resulting body calls `gather_i64` and the result slot is retyped to `PVecI64`. Mirror the `PreparedFunc` fixture construction in `backend_repr_suite.tw`. Register the suite in `boot/tests/main.tw` (add `use .suites.route_typed_vec_suite` and include `route_typed_vec_suite.suite()`), following the `repr_policy_suite` pattern (lines 22, 207).

- [ ] **Step 6: Run the suite and confirm it passes**

Run: `make bundle-cli >/dev/null 2>&1 && target/twk test 2>&1 | tail -5`
Expected: PASS. (The Task 1.5 WAT probe is the authoritative end-to-end check on the actual `IntCol(v.gather(idx))` shape.)

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/backend/route_typed_vec.tw boot/tests/suites/route_typed_vec_suite.tw boot/tests/main.tw
git commit -m "route: gather->gather_i64 dataflow (escape whitelist + result eligibility)"
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

Compare `order_by` against the ~2403ms baseline. Expect the gather/take contribution (~830ms) to shrink. **Record the actual delta in the umbrella doc** ([typed-vector-representation.md](../typed-vector-representation.md)). If gather did not move, inspect the WAT: confirm `gather_i64` is actually reached in the dataframe `column.gather` (the payload read must be `eligible_v`); a common cause is the `gather` result not being marked typed because the payload store site was not in `typed_payloads`. Do NOT start Stage 2 until Stage 1's win is confirmed and recorded.

---

# STAGE 2 — Cross-call typed ABI (specialize-by-representation)

**Read first:** `analyze_typed_payloads` / `analyze_typed_fields` (find via `grep -rn 'fn analyze_typed_payloads\|fn analyze_typed_fields' boot/compiler/backend/`) and the emission path (`emit.tw:968` derives the physical return ValType from `prepared.return_mono`; `PreparedModule` at `prepare.tw:36` carries BOTH `.anf.functions`, the emitted bodies, AND `.funcs`, the slot metadata).

## The single pipeline (do this ordering — #5)

`ParamAbi` is a **pre-route** analysis; routing and emission **consume** it. There is exactly one ordered sequence in `prepare_backend` (`prepare.tw:79-104`). Restructure the 97-104 block to:

```
1. assign_repr_for_module                              (existing, :68)
2. typed_fields   := analyze_typed_fields(...)         (existing, :97)
3. typed_payloads := analyze_typed_payloads(...)       (existing, :103)
4. param_abi      := analyze_typed_params(funcs, builtins, typed_fields, typed_payloads)   ← NEW, pre-route
5. funcs3         := route_typed_vectors(funcs, builtins, typed_fields, typed_payloads, param_abi)
                     — route consumes param_abi for the escape relaxation (Task 2.3)
                       AND performs variant emission + call-site ABI selection (Task 2.5)
```

`param_abi` is computed once, before routing, and threaded in. Nothing computes it *after* routing.

## Two facts about emission that shape the design

- **A variant needs a real ANF body, not just a PreparedFunc (#3).** Emission iterates `PreparedModule.anf.functions` for bodies; `.funcs` is only slot metadata. A `PreparedFunc`-only `$i64` clone gets metadata but no emitted, callable function. Every variant must clone BOTH the `AnfModule` function (into `.anf.functions`, with a fresh `FuncId`/symbol) AND its `PreparedFunc` (into `.funcs`).
- **Typed return needs a physical-return-ABI source of truth (#4).** `emit.tw:968` computes the return ValType from `prepared.return_mono` (semantic `Vector<Int>` → boxed `PVec`) and coerces the body to it. A cloned variant with the same `return_mono` still returns boxed. Task 2.4 adds an explicit physical-return override the emitter consults; Task 2.5's variant sets it.

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

`param_uses_typed_only(pf, param_slot_id, typed_fields, typed_payloads, builtins)` returns `Bool` — note it takes `typed_fields`/`typed_payloads` explicitly, because "stored into a typed field/payload" is a typed-compatible use and needs those sets to decide. Walk `pf.body`; the param slot is typed-compatible iff every use is one of: index read (`get`/`get_i64`), `len`, stored into a field/payload present in `typed_fields`/`typed_payloads`, returned as the function result, or passed as an argument into another function's typeable param slot. Any other use (captured into a `ClosureEnv`, placed in a generic container, passed to an `anyref` param, passed to a non-typeable param) makes it non-typeable. For the FIRST iteration, treat "passed to another function's param" as non-typeable (conservative), so this task needs no cross-function knowledge; Task 2.2 adds the fixpoint.

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
- [ ] **Step 3: Establish the pipeline step + thread `ParamAbi` into routing.** In `prepare.tw` add pipeline step 4 (per the Stage 2 header): after `analyze_typed_payloads` (`:103`), compute `param_abi := analyze_typed_params(pre.funcs, builtins, typed_fields, typed_payloads)`, and widen `route_typed_vectors` to take `param_abi` (5th arg). Thread it through `route_func` → `classify_v_group` / `op_group_escapes`. In the arg-of-call escape check, when the callee arg position is a typeable param (`ParamAbi.typeable_params[callee_id]` contains the index), do NOT mark the slot as escaping. This is the single relaxation point. Every other escape route stays. (Task 2.5 later extends this same `route_typed_vectors` call to also do variant emission — the pipeline is introduced here, not rebuilt there.)
- [ ] **Step 4: Run the test, confirm it passes.**
- [ ] **Step 5: Self-host + full suite** — `make bundle-cli` fixed point; `make boot-test` green. This proves the relaxation did not mis-type any boot-compiler value. (At this point `param_abi` only relaxes the guard; no variants exist yet, so a relaxed local that reaches a still-boxed callee ABI must still box at the call — confirm the WAT is correct, i.e. no typed value is passed to a boxed-ABI param. If that intermediate state is unsound, gate the relaxation on "a variant will be emitted" and land Task 2.3 + 2.5 together.)
- [ ] **Step 6: Commit** — `route: don't treat arg-into-typeable-param as an escape`

## Task 2.4: Physical return-ABI source of truth (#4)

Before a variant can return `PVecI64`, the emitter needs a physical-return signal it consults *instead of* deriving purely from `return_mono`. Add an optional physical-return override to `PreparedFunc` and make `emit.tw` honour it.

**Files:**
- Modify: `boot/compiler/backend/prepared_ir.tw` (add field to `PreparedFunc`)
- Modify: `boot/compiler/backend/repr_assign.tw:115` (carry the field through — it already copies `return_mono`)
- Modify: `boot/compiler/codegen/emit.tw:968` (consult the override for `ret_vt`) and the return-coercion path (`emit.tw:990`, `current_return_mono`)
- Test: `boot/tests/suites/wasm_layout_suite.tw` (or `codegen_emit_suite.tw`)

- [ ] **Step 1: Add the field**

To `type PreparedFunc` (`prepared_ir.tw:145`) add:

```
  phys_return: ValType?,   // physical return ABI override; .None => derive from return_mono
```

Default `.None` at every existing construction site (grep `grep -rn 'PreparedFunc\s*\.{\|\.{ func_id' boot/compiler/backend/` and add `phys_return: .None,` — the compiler will error on any missed site, which is the checklist).

- [ ] **Step 2: Write the failing test** — a `PreparedFunc` with `phys_return: .Some(pvec_i64 ValType)` emits a function whose result type is `PVecI64`, and the body's return position coerces to `PVecI64`. Assert against the emitted signature/WAT. Mirror an existing emit/layout suite test.
- [ ] **Step 3: Run it, confirm it fails.**
- [ ] **Step 4: Honour the override in emit** — at `emit.tw:968`, if `prepared.phys_return` is `.Some(vt)`, use `[vt]` as `ret_vt`; the body coercion at `:990` must target the physical type when the override is set (thread the physical return type through `current_return_mono`/the return-emit path so `.Return`/tail atoms coerce to `PVecI64`, not the boxed `return_mono`).
- [ ] **Step 5: Run the test, confirm it passes.**
- [ ] **Step 6: Self-host + full suite** — `make bundle-cli` fixed point (override defaults `.None`, so existing funcs are unchanged); `make boot-test` green.
- [ ] **Step 7: Commit** — `emit: physical return-ABI override on PreparedFunc`

## Task 2.5: Emit `f$i64` repr variants (ANF + PreparedFunc) and select the ABI

**Files:**
- Modify: `boot/compiler/backend/typed_param_abi.tw` (variant emission)
- Modify: `boot/compiler/backend/prepare.tw` (the single pipeline, per the header)
- Modify: `boot/compiler/backend/route_typed_vec.tw` (rewrite typed calls to the variant)
- Test: `boot/tests/suites/typed_param_abi_suite.tw`

- [ ] **Step 1: Write the failing test** — given a function with a typeable param and a caller passing a typed vector, after the pass there exist BOTH a cloned `AnfModule` function in `.anf.functions` and a cloned `PreparedFunc` in `.funcs` for the `$i64` variant (param slot `PVecI64`; `phys_return` set if the return is typeable), and the typed call targets the variant's `func_id`.
- [ ] **Step 2: Run it, confirm it fails.**
- [ ] **Step 3: Implement demand-driven emission over BOTH representations (#3).** `emit_repr_variants(module, param_abi, ...) -> (module2, redirect)`:
  - For each call site whose actual arg is typed (`eligible_v`) and lands in a typeable param, demand a variant of the callee.
  - For each demanded `f`: mint a fresh `FuncId` (find the module id allocator: `grep -rn 'next_func_id\|fresh.*[Ff]unc\|alloc.*FuncId\|new_func_id' boot/compiler/`), then clone **both**:
    - the `AnfModule` function for `f` into `module.anf.functions` under the new id (so a body is actually emitted, plus whatever symbol registration the emitter needs — check how `.anf.functions` entries become symbols and replicate it);
    - the `PreparedFunc` for `f` into `module.funcs` under the new id, retyping the typeable param slots to `PVecI64`, setting `phys_return` (Task 2.4) if `typeable_return`, then running `route_func` on the clone so its body stays typed internally.
  - Return `redirect: Dict<String, Int>` from `(call-site callee id)` to the variant id.
  - Demand-driven: functions with no typed caller get no variant. DCE drops any unreferenced variant.
- [ ] **Step 4: Rewrite call sites + drop return box** — in `route_func`, rewrite a matching `.ACall(.AGlobalFunc(fid), args)` to the variant id from `redirect`; when the variant's `phys_return` is `PVecI64` and the caller's result slot is typed, the result needs no `box_i64` (mark the result slot `eligible_v`); a boxed caller of the same variant coerces the `PVecI64` result via `emit_coerce_stack` (already handles `PVecI64→PVec`).
- [ ] **Step 5: Wire the single pipeline** — implement the ordering in the Stage 2 header: compute `param_abi` PRE-route (new line after `analyze_typed_payloads`, `prepare.tw:103`), pass it into `route_typed_vectors`, and run `emit_repr_variants` as part of that routing step. One well-commented sequence; nothing computes `param_abi` after routing.
- [ ] **Step 6: Run the test, confirm it passes.**
- [ ] **Step 7: Self-host + full suite** — `make bundle-cli` fixed point; `make boot-test` green.
- [ ] **Step 8: Commit** — `feat: emit PVecI64-ABI variants (anf+prepared) + typed call selection`

## Task 2.6: Guard test + build-path WAT probe + bench

**Files:**
- Create: `examples/performance/sort-bench/typed_param_abi_probe.tw`
- Create: `examples/performance/sort-bench/typed_param_capture_guard.tw`

- [ ] **Step 1: Build-path probe** — a program that builds a `Vector<Int>` in a loop and passes it into a function that stores it into a typed payload (the `gen.table → int_col` shape). Build to WAT; assert the callee has a `$i64` variant and the build stays typed:

```bash
target/twk build examples/performance/sort-bench/typed_param_abi_probe.tw -o /tmp/p.wat
grep -c 'builder_freeze_i64' /tmp/p.wat   # typed build present (>=1)
grep -c box_i64 /tmp/p.wat                 # record; must be < the pre-Stage-2 count for this fixture
```
Record the pre-Stage-2 `box_i64` count for the same fixture first (build it at Checkpoint A) so "fewer" is a concrete number, not a vibe.

- [ ] **Step 2: Escape-guard regression probe (WAT assertion, not just timing).** Mirror `typed_payload_capture_guard.tw`: pass a typed vector into a function that captures it into a closure and reads it in a loop. It MUST stay boxed. Assert BOTH:

```bash
target/twk build examples/performance/sort-bench/typed_param_capture_guard.tw -o /tmp/cg.wat
# The captured column's comparator/closure trampoline reads via boxed get, NOT get_i64:
#   inspect the closure trampoline body — it must call $rt_arr__get, not $rt_arr__get_i64.
# And no $i64 variant is emitted for the capturing function.
timeout 15 target/twk run examples/performance/sort-bench/typed_param_capture_guard.tw   # ~ms, not seconds
```
Expected: the trampoline uses boxed `get` (WAT assertion), and runtime is a few ms (timing). The WAT check is authoritative; timing is the coarse backstop.

- [ ] **Step 3: Bench** — `target/twk run examples/performance/dataframe/bench/main.tw`; record `order_by` and `filter`/`group_by`/`join`. Expect the build path to move; expect `order_by` umbrella to move materially only after step 2 (typed closure env). Record deltas in [typed-vector-representation.md](../typed-vector-representation.md).
- [ ] **Step 4: Commit** — `test: Stage 2 build-path probe + capture guard (WAT+timing) + bench`

## Checkpoint B — wrap up

- [ ] Update [typed-vector-representation.md](../typed-vector-representation.md) status and [typed-vector-continue-here.md](typed-vector-continue-here.md): mark Stage 1/2 done, note that step 2 (typed closure env / comparator, ~1317ms) is now unblocked because the key column can reach `sort_indices` typed.
- [ ] Final `make bundle-cli` fixed point + `make boot-test` green + `cargo test --release` (targeted) as a last guard.

---

## Notes on stage0 parity

Stage 1's runtime ops (`builder_push_i64_raw`, `gather_i64`) are ordinary Twinkle inside `arr.tw`; stage0 compiles them as source, so no Rust change is expected. If `make stage2` fails because the boot compiler emits a `gather_i64` call while compiling `boot/main.tw` and stage0 lacks it, follow the runtime-builtin-wiring recipe's stage0 trap-stub step. Stage 2 is pure boot-backend codegen (per the "no stage0 parity for boot-codegen opts" rule) — but the same `make stage2` fixed-point check is the authoritative guard and is already in Tasks 2.3/2.4.
