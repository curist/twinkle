# Expected{vt, mono} Emit Coercion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make emit coerce stored/returned/called values to the destination slot's physical `ValType` instead of re-deriving it from mono, so a `case`/`if` whose arms all yield a typed `PVecI64` source can carry a typed result slot without an invalid box→unbox.

**Architecture:** Introduce `Expected = .{ vt: ValType?, mono: MonoType }` and thread it through the emit coercion path in place of the bare `expected_ty: MonoType`. Stage 1 is behavior-preserving: the `Expected` always carries the mono-derived vt, so no codegen changes. Stage 2 injects the real physical vt at each store/return/call site (arm stores, wide-if spine, function return, runtime-call result, direct-call result) and re-applies the reverted `route_func` result-slot typing pass. Soundness rests on the router disqualifying any result slot with a boxed arm, so a typed result slot has only typed arms.

**Tech Stack:** Twinkle boot compiler (`boot/`), self-hosted; verification via `make bundle-cli` (self-host fixed point) + `make boot-test` (`boot/tests/main.tw`) + targeted `.tw` repros + WAT inspection.

**Design doc:** `docs/plans/performance/vector/expected-vt-coercion-design.md`

---

## File Structure

- `boot/compiler/codegen/emit/context.tw` — **home of the new `Expected` type + `expected()` constructor** (already holds `EmitCtx`, `LabelPair`; imported everywhere in emit).
- `boot/compiler/codegen/emit.tw` — `emit_expr`, `emit_tail_expr`, `emit_atom_for_expected`, `emit_return_coerce` migrate to `Expected`; `try_emit_if_spine`, `emit_call` wrapper, `emit_op` gain physical-vt injection.
- `boot/compiler/codegen/emit/control_flow.tw` — `ControlEmitFns.emit_expr` fn-type field changes; arm-emit sites build `Expected`.
- `boot/compiler/codegen/emit/match.tw` — `MatchEmitFns.emit_expr` fn-type field; `MatchCtx` gains `result_vt`; arm-emit sites build `Expected`.
- `boot/compiler/codegen/emit/calls.tw` — direct-call result store coerces to destination vt; `emit_runtime_call`/`adapt_runtime_result` thread destination vt; delete `is_typed_vec_result` box-skip.
- `boot/compiler/codegen/emit/coercions.tw` — `adapt_runtime_result` takes destination vt.
- `boot/compiler/backend/route_typed_vec.tw` — re-author the result-slot typing pass (`slot_typed_after_route` control-flow case, `arm_verdict`, `route_func` retyping).
- `boot/tests/` — router + emit regression `.tw` programs.

**Note on naming:** `calls.tw:92` already defines `DirectExpected = .{ mono, val_type }` (arg-side, non-null vt). Leave it as-is; `Expected` is the result/return-side twin with a nullable vt (`.None` = no-result / `Void` / `Never`). Do not unify them in this plan.

---

## STAGE 1 — Behavior-preserving `Expected` wrapper

Stage 1 introduces the type and threads it, always constructing the mono-derived vt. **Expected end state: identical Wasm, identical self-host fixed point, full suite green.** No `.tw` behavior test can distinguish Stage 1 from `main` — the verification IS "self-host + suite unchanged."

### Task 1: Add the `Expected` type and constructor

**Files:**
- Modify: `boot/compiler/codegen/emit/context.tw`

- [ ] **Step 1: Read the current imports/top of context.tw**

Run: `sed -n '1,25p' boot/compiler/codegen/emit/context.tw`
Confirm it imports `wasm_ir.{... ValType}` and `mono_type.{MonoType}`. If `val_type_of_mono`/`ResolvedEnv` are not already imported, note their modules: `val_type_of_mono` is in `compiler.codegen.wasm_layout`; `ResolvedEnv` is in `compiler.resolved_env` (grep to confirm the exact path used elsewhere: `grep -rn "use compiler.*wasm_layout.{" boot/compiler/codegen/emit.tw` and `grep -rn "ResolvedEnv" boot/compiler/codegen/emit/context.tw`).

- [ ] **Step 2: Add the type and constructor**

Add near `LabelPair` in `boot/compiler/codegen/emit/context.tw`. Add the imports needed (`val_type_of_mono`, `ResolvedEnv`) if not present:

```tw
// The expected shape of a value at an emit destination: its physical Wasm type
// (`.None` for a no-result position — Void/Never — where no coercion is emitted)
// and its mono, kept only as a hint for emit_coerce_stack instruction selection.
pub type Expected = .{ vt: ValType?, mono: MonoType }

// Build the mono-derived Expected. Never/Void carry no vt because
// val_type_of_mono(.Never) hard-errors; those positions are gated upstream by
// the existing .Void/.Never match arms in emit_expr/emit_atom_for_expected.
pub fn expected(mono: MonoType, env: ResolvedEnv) Expected {
  case mono {
    .Void => .{ vt: .None, mono },
    .Never => .{ vt: .None, mono },
    _ => .{ vt: .Some(val_type_of_mono(mono, env)), mono },
  }
}
```

- [ ] **Step 3: Build to confirm the type compiles (no call sites yet)**

Run: `cargo run --release -- build boot/main.tw -o /tmp/s1t1.wasm 2>&1 | tail -5`
Expected: builds with no error (unused pub type/fn is fine).

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/codegen/emit/context.tw
git commit -m "codegen: add Expected{vt,mono} type + expected() constructor"
```

### Task 2: Migrate the coercion path to `Expected`

This is one atomic signature change (the functions are mutually recursive through the `ControlEmitFns`/`MatchEmitFns` fn-type fields, so it must compile as a unit). Mechanical: every current `expected_ty: MonoType` becomes `exp: Expected`; every leaf coercion targets `exp.vt` when `.Some`; every call site wraps its mono with `expected(mono, ctx.env)`.

**Files:**
- Modify: `boot/compiler/codegen/emit.tw` (`emit_expr`, `emit_tail_expr`, `emit_atom_for_expected`, `emit_return_coerce`, all internal call sites)
- Modify: `boot/compiler/codegen/emit/control_flow.tw` (`ControlEmitFns`, arm-emit call sites)
- Modify: `boot/compiler/codegen/emit/match.tw` (`MatchEmitFns`, arm-emit call sites)

- [ ] **Step 1: Migrate `emit_atom_for_expected`**

In `boot/compiler/codegen/emit.tw`, change (was emit.tw:1231-1250):

```tw
fn emit_atom_for_expected(
  atom: PreparedAtom,
  exp: Expected,
  ctx: EmitCtx,
  buf: Vector<Instr>,
) Vector<Instr> {
  buf2 := emit_atom(atom, ctx, buf)

  case exp.vt {
    .None => buf2,
    .Some(vt) => emit_coerce_stack(atom_val_type(atom, ctx), vt, exp.mono, ctx, buf2),
  }
}
```

Note: matching on `exp.vt` (`.None` for Void/Never) replaces the old `case expected_ty { .Void => …, .Never => …, _ => … }` — behavior identical because `expected()` maps Void/Never to `vt: .None`.

- [ ] **Step 2: Migrate `emit_return_coerce`**

In `boot/compiler/codegen/emit.tw` (was emit.tw:1213-1229). Keep the `phys_return` special branch UNCHANGED in Stage 1 (folding it is Stage 2, Task 6):

```tw
fn emit_return_coerce(atom: PreparedAtom, exp: Expected, ctx: EmitCtx, buf: Vector<Instr>) Vector<
  Instr,
> {
  ret_mono := current_return_mono(ctx, exp.mono)
  phys := case current_prepared_func(ctx) {
    .Some(pf) => pf.phys_return,
    .None => .None,
  }

  case phys {
    .Some(vt) => {
      buf2 := emit_atom(atom, ctx, buf)
      emit_coerce_stack(atom_val_type(atom, ctx), vt, ret_mono, ctx, buf2)
    },
    .None => emit_atom_for_expected(atom, expected(ret_mono, ctx.env), ctx, buf),
  }
}
```

- [ ] **Step 3: Migrate `emit_expr` and `emit_tail_expr`**

In `boot/compiler/codegen/emit.tw`, change the parameter `expected_ty: MonoType` → `exp: Expected` on both `fn emit_expr` (was 1143) and `fn emit_tail_expr` (was 1256). Inside, replace uses:
- The `case expected_ty { .Void => …, .Never => …, _ => … }` in `emit_expr`'s `.Atom` arm → match on `exp.mono` (the mono still drives the Void-drop / Never-unreachable behavior):

```tw
.Atom(atom) => case exp.mono {
  .Void => {
    mono := atom_mono(atom, ctx)
    case mono {
      .Void => buf,
      .Never => emit_atom(atom, ctx, buf),
      _ => {
        buf2 := emit_atom(atom, ctx, buf)
        buf2.append(.Drop)
      },
    }
  },
  .Never => {
    buf2 := emit_atom(atom, ctx, buf)
    buf2.append(.Unreachable)
  },
  _ => emit_atom_for_expected(atom, exp, ctx, buf),
},
```

- `.Return(.Some(atom))` in both → `emit_return_coerce(atom, exp, ctx, buf)`.
- The forwarding `_ => emit_expr(expr, exp, ctx, buf)` in `emit_tail_expr` and its `.Atom`/`.Return` arms → pass `exp`.
- `emit_tail_let` / `emit_let`: their `expected_ty` param becomes `exp: Expected`, forwarded unchanged to the recursive `emit_expr`/`emit_tail_expr` calls (was emit.tw:1276, 1622, 1642). At the `emit_op` calls inside these (e.g. 1294, 1298, 1318) nothing changes — `emit_op` keeps `(result_vt, result_mono)`.

- [ ] **Step 4: Migrate the `ControlEmitFns` fn-type field + arm sites**

In `boot/compiler/codegen/emit/control_flow.tw`, change (was line 11):

```tw
pub type ControlEmitFns = .{
  emit_atom: fn(PreparedAtom, EmitCtx, Vector<Instr>) Vector<Instr>,
  emit_expr: fn(PreparedExpr, Expected, EmitCtx, Vector<Instr>) Vector<Instr>,
}
```

Add imports at the top of `control_flow.tw`: `use compiler.codegen.emit.context.{EmitCtx, LabelPair, Expected, expected}`. Then at each arm-emit call site (was 36, 37, 104, 110) wrap the mono:

```tw
then_instrs := fns.emit_expr(then_e, expected(result_mono, ctx.env), ctx, [])
else_instrs := fns.emit_expr(else_e, expected(result_mono, ctx.env), ctx, [])
```

and in `emit_if_else_spine` (104, 110) likewise `expected(result_mono, ctx.env)`. Leave the loop-body call at line 153 as `expected(.Void, ctx.env)` (Void → vt `.None`, same as today's `.Void`).

- [ ] **Step 5: Migrate the `MatchEmitFns` fn-type field + arm sites**

In `boot/compiler/codegen/emit/match.tw`, change (was line 13):

```tw
  emit_expr: fn(PreparedExpr, Expected, EmitCtx, Vector<Instr>) Vector<Instr>,
```

Add `Expected, expected` to the `emit.context` import. At each arm-emit site (was 196, 391, 472, 550) wrap:

```tw
body_instrs := mc.fns.emit_expr(arm.body, expected(mc.result_mono, mc.ctx.env), mc.ctx, [])
```

(and the `tc.` variants use `tc.result_mono`, `tc.ctx.env`).

- [ ] **Step 6: Migrate the top-level `emit_expr` call sites in emit.tw**

Wrap the remaining direct `emit_expr(...)`/`emit_tail_expr(...)` call sites (was emit.tw:1001, 3412, 3419, and the `emit_tail_expr` sites at 1442, 1446, 1558, 1559) that pass a raw mono, e.g.:

```tw
.None => emit_expr(prepared.body, expected(prepared.return_mono, ctx.env), ctx, []),
```

and in `try_emit_if_spine` (3412, 3419) and the tail-if/tail-match sites — wrap with `expected(result_mono, ctx.env)`. Grep to catch all: `grep -n "emit_expr(\|emit_tail_expr(" boot/compiler/codegen/emit.tw` — every call must now pass an `Expected`.

- [ ] **Step 7: Build stage0 → boot**

Run: `cargo run --release -- build boot/main.tw -o /tmp/s1t2.wasm 2>&1 | tail -20`
Expected: builds clean. If a call site was missed, the type error names the file:line — wrap that mono with `expected(mono, <env>)`.

- [ ] **Step 8: Self-host fixed point + full suite (the real Stage 1 test)**

Run: `make bundle-cli 2>&1 | tail -5 && make boot-test 2>&1 | tail -15`
Expected: self-host reaches a fixed point (bundle-cli succeeds) and boot-test reports all tests passing (same count as `main`). Because every `Expected.vt` equals the old `val_type_of_mono(mono)`, codegen is byte-identical.

- [ ] **Step 9: Confirm byte-identical codegen against pre-refactor (optional but recommended)**

Run:
```bash
git stash list >/dev/null; target/twk build boot/tests/main.tw -o /tmp/after.wat
git stash; make quick-bundle-cli >/dev/null 2>&1; target/twk build boot/tests/main.tw -o /tmp/before.wat; git stash pop; make quick-bundle-cli >/dev/null 2>&1
diff /tmp/before.wat /tmp/after.wat && echo "IDENTICAL"
```
Expected: `IDENTICAL` (or empty diff). If not identical, a call site is deriving a different vt than before — investigate before proceeding.

- [ ] **Step 10: Commit**

```bash
git add boot/compiler/codegen/emit.tw boot/compiler/codegen/emit/control_flow.tw boot/compiler/codegen/emit/match.tw
git commit -m "codegen: thread Expected{vt,mono} through emit coercion path (behavior-preserving)"
```

---

## STAGE 2 — Inject the real physical targets

Each Stage 2 task swaps a mono-derived `Expected` for one built from the destination's physical vt, or threads the vt to a coercion helper. **Until the router (Task 8) sets any slot's vt to `PVecI64`, every `result_vt`/`phys_return` still equals the mono-derived vt, so Tasks 3–7 are individually behavior-preserving** (verified by self-host + suite green). Task 8 flips routing on; Task 9 proves the win and the soundness cases.

### Task 3: Physical vt at `if`/`match` arm stores

**Files:**
- Modify: `boot/compiler/codegen/emit/control_flow.tw` (`emit_if_op` arm sites; thread `result_vt`)
- Modify: `boot/compiler/codegen/emit/match.tw` (`MatchCtx` gains `result_vt`; arm sites)

- [ ] **Step 1: Build the arm `Expected` from `result_vt` in control_flow.tw**

`emit_if_op` already receives `result_vt: ValType?` (control_flow.tw:21). Replace the arm-emit `expected(result_mono, ctx.env)` (from Stage 1) with a helper that prefers `result_vt`:

```tw
// Prefer the routed physical result vt; fall back to the mono-derived vt.
fn arm_expected(result_vt: ValType?, result_mono: MonoType, env: ResolvedEnv) Expected {
  case result_vt {
    .Some(vt) => .{ vt: .Some(vt), mono: result_mono },
    .None => expected(result_mono, env),
  }
}
```

Add it near the top of `control_flow.tw`. Use it at the `emit_if_op` arm sites (lines ~36-37): `fns.emit_expr(then_e, arm_expected(result_vt, result_mono, ctx.env), ctx, [])`. `emit_if_else_spine` does not receive `result_vt` today; pass it through (add a `result_vt: ValType?` param to `emit_if_else_spine` and forward it from `emit_if_op`'s spine branch at line 30), then use `arm_expected` there too.

- [ ] **Step 2: Thread `result_vt` into `MatchCtx`**

In `boot/compiler/codegen/emit/match.tw`, add the field to `MatchCtx` (was line ~44):

```tw
type MatchCtx = .{
  scrutinee_instrs: Vector<Instr>,
  scrutinee_mono: MonoType,
  result_idx: Int,
  result_vt: ValType?,
  result_mono: MonoType,
  ctx: EmitCtx,
  fns: MatchEmitFns,
}
```

Update its construction in `emit_match_op` (was line 74) to include `result_vt` (the param is already received at line 64):

```tw
mc := MatchCtx.{ scrutinee_instrs, scrutinee_mono, result_idx, result_vt, result_mono, ctx, fns }
```

Copy `arm_expected` into `match.tw` (or export it from `control_flow.tw` and import — prefer exporting from `control_flow.tw` since it already exports `append_result_store`). Use it at the non-tail arm sites (196, 391): `mc.fns.emit_expr(arm.body, arm_expected(mc.result_vt, mc.result_mono, mc.ctx.env), mc.ctx, [])`. Leave the `TailMatchCtx` sites (472, 550) on the mono path — tail arms go through `emit_return_coerce`, handled in Task 6.

- [ ] **Step 3: Self-host + suite (still behavior-preserving pre-routing)**

Run: `make bundle-cli 2>&1 | tail -5 && make boot-test 2>&1 | tail -8`
Expected: fixed point + all tests pass. `result_vt` still equals the mono vt everywhere (router not yet re-applied), so no codegen change.

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/codegen/emit/control_flow.tw boot/compiler/codegen/emit/match.tw
git commit -m "codegen: route if/match arm stores to the physical result vt"
```

### Task 4: Physical vt in the wide-if fast path

**Files:**
- Modify: `boot/compiler/codegen/emit.tw` (`try_emit_if_spine`, was 3390-3448)

- [ ] **Step 1: Thread `result_vt` into `try_emit_if_spine`**

`try_emit_if_spine` currently takes `result_mono` but not `result_vt`, and stores with `append_result_store(..., result_mono)` after `emit_expr(frame.then_expr, result_mono, ...)` (was 3412-3419). Add a `result_vt: ValType?` parameter and build the arm `Expected` from it. The caller `emit_if_op` (emit.tw:3435) already has `result_vt` — pass it:

```tw
case try_emit_if_spine(cond_e, then_e, else_e, result_idx, result_vt, result_mono, ctx, buf) {
```

Inside `try_emit_if_spine`, at each arm/tail emit:

```tw
then_body := emit_expr(frame.then_expr, arm_expected(result_vt, result_mono, ctx.env), ctx, [])
then_body = control_emit.append_result_store(then_body, result_idx, result_mono)
```

and likewise for the `spine.tail` emit. Import `arm_expected` from `control_flow.tw`. `append_result_store` stays mono-keyed — it only emits a bare `LocalSet` (or skips for Void/Never); the value is already at `result_vt`.

- [ ] **Step 2: Self-host + suite**

Run: `make bundle-cli 2>&1 | tail -5 && make boot-test 2>&1 | tail -8`
Expected: fixed point + all tests pass (behavior-preserving pre-routing).

- [ ] **Step 3: Commit**

```bash
git add boot/compiler/codegen/emit.tw
git commit -m "codegen: route wide-if spine arm stores to the physical result vt"
```

### Task 5: (folded into Task 6) — function return

Function return is handled entirely by folding `phys_return` into the general `Expected` path in Task 6; no separate task.

### Task 6: Fold `phys_return`, thread runtime-call vt, delete `is_typed_vec_result`

**Files:**
- Modify: `boot/compiler/codegen/emit.tw` (`emit_return_coerce` fold)
- Modify: `boot/compiler/codegen/emit/coercions.tw` (`adapt_runtime_result` takes vt)
- Modify: `boot/compiler/codegen/emit/calls.tw` (`emit_runtime_call` threads vt; delete box-skip)
- Modify: `boot/compiler/codegen/emit/runtime_abi.tw` (remove now-dead `is_typed_vec_result` if unused)

- [ ] **Step 1: Fold `phys_return` into the general path**

Replace `emit_return_coerce` (from Stage 1) so the physical target is a normal `Expected`:

```tw
fn emit_return_coerce(atom: PreparedAtom, exp: Expected, ctx: EmitCtx, buf: Vector<Instr>) Vector<
  Instr,
> {
  ret_mono := current_return_mono(ctx, exp.mono)
  phys := case current_prepared_func(ctx) {
    .Some(pf) => pf.phys_return,
    .None => .None,
  }
  ret_exp := case phys {
    .Some(vt) => .{ vt: .Some(vt), mono: ret_mono },
    .None => expected(ret_mono, ctx.env),
  }
  emit_atom_for_expected(atom, ret_exp, ctx, buf)
}
```

- [ ] **Step 2: Give `adapt_runtime_result` the destination vt**

In `boot/compiler/codegen/emit/coercions.tw` (was line 106) change the signature to accept the destination vt and target it instead of recomputing from mono:

```tw
pub fn adapt_runtime_result(fid: FuncId, dest_vt: ValType, result_mono: MonoType, ctx: EmitCtx) Vector<Instr> {
  // ... existing body, but coerce the runtime result to dest_vt (was
  // val_type_of_mono(result_mono, ctx.env)). result_mono stays the coerce hint.
}
```

Read the current body first (`sed -n '106,140p' boot/compiler/codegen/emit/coercions.tw`) and replace the internal `val_type_of_mono(result_mono, ctx.env)` target with `dest_vt`.

- [ ] **Step 3: Thread the vt through `emit_runtime_call` and delete the box-skip**

In `boot/compiler/codegen/emit/calls.tw`: `emit_runtime_call` (was 315) must receive the destination vt (add a `dest_vt: ValType` param, or pass `result_vt` from `emit_call`). At the box site (was 357-358) replace:

```tw
if !is_builder_seed(entry_name) {
  out = .concat(adapt_runtime_result(fid, dest_vt, result_mono, ctx))
}
```

Delete the `!is_typed_vec_result(entry_name)` condition — with `dest_vt` correct, a typed-vec result adapts to `dest_vt` (a no-op) instead of boxing. Remove the `is_typed_vec_result` import from `calls.tw`. If `is_typed_vec_result` is now unused everywhere (`grep -rn is_typed_vec_result boot/`), delete the function from `runtime_abi.tw`.

- [ ] **Step 4: Provide `dest_vt` at the `emit_op` → `emit_call` boundary**

`emit_op`'s `.ACall` arm (was emit.tw:1702) drops `result_vt`; pass it into `emit_call`. Give `emit_call`/`emit_runtime_call` a `dest_vt` computed as: `case result_vt { .Some(vt) => vt, .None => val_type_of_mono(result_mono, ctx.env) }` (guard the Void/Never monos — for those, runtime results are dropped/never, so keep the current behavior; only compute `dest_vt` on the value-producing path).

- [ ] **Step 5: Self-host + suite**

Run: `make bundle-cli 2>&1 | tail -5 && make boot-test 2>&1 | tail -8`
Expected: fixed point + all tests pass. Do **not** require byte-identical WAT here: `phys_return` and the gather `is_typed_vec_result` exemption already exist today, so folding them into the general `dest_vt` path can legitimately reshuffle call-boundary instructions even pre-routing (the net effect must be equivalent — same box/no-box decisions — but the exact instruction stream may differ). Self-host fixed point + suite green is the gate.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/codegen/emit.tw boot/compiler/codegen/emit/coercions.tw boot/compiler/codegen/emit/calls.tw boot/compiler/codegen/emit/runtime_abi.tw
git commit -m "codegen: return/runtime-call results coerce to destination vt; drop is_typed_vec_result box-skip"
```

### Task 7: Direct user-call result store coerces to destination vt

**Files:**
- Modify: `boot/compiler/codegen/emit/calls.tw` (direct-call store, was 175-182)

- [ ] **Step 1: Coerce the callee return vt to the destination vt before the store**

In the direct-call path (was calls.tw:173-182), after `.Call(sym)`, replace the bare store:

```tw
case result_mono {
  .Void => {
    out = .append(.I32Const(0))
    out = .append(.RefI31)
    out.append(.LocalSet(result_idx))
  },
  .Never => out,
  _ => {
    callee_ret_vt := case expected_func {
      .Some(pf) => case pf.phys_return {
        .Some(vt) => vt,
        .None => val_type_of_mono(result_mono, ctx.env),
      },
      .None => val_type_of_mono(result_mono, ctx.env),
    }
    out = emit_coerce_stack(callee_ret_vt, dest_vt, result_mono, ctx, out)
    out.append(.LocalSet(result_idx))
  },
}
```

`dest_vt` is threaded in from Task 6 Step 4. This coercion is a **no-op** when routing keeps caller slot and callee return vt consistent (the hot path); it exists so a mismatched boundary emits valid Wasm (a residual box/unbox) rather than a trap. Add `emit_coerce_stack` / `val_type_of_mono` imports to `calls.tw` if not present.

- [ ] **Step 2: Gate the direct TAIL call on matching physical return ABI**

A `return_call` requires the callee's physical return type to equal the *current* function's physical return type; `emit_direct_tail_call` (calls.tw:190) emits `ReturnCall` with **no** result adaptation (its doc even says "Caller must verify … matching result type"). Post-routing a typed-return callee (`phys_return = PVecI64`) tail-called from a boxed-return function is invalid Wasm. Gate it in `try_emit_tail_op` (`emit.tw:1494`, the `.ACall(.AGlobalFunc(fid), args)` → `bi.entry … .None` branch that calls `emit_direct_tail_call`).

Add a helper near `try_emit_tail_op`:

```tw
// phys_return ABIs are equal when both absent, or both the same ValType.
// Today the only typed-return ABI is PVecI64, so a structural Ref-name compare
// is sufficient; fall back to conservative inequality for anything else.
fn phys_abi_eq(a: ValType?, b: ValType?) Bool {
  case a {
    .None => case b { .None => true, .Some(_) => false },
    .Some(av) => case b {
      .None => false,
      .Some(bv) => case av {
        .Ref(_, .Named(an)) => case bv {
          .Ref(_, .Named(bn)) => an == bn,
          _ => false,
        },
        _ => false,
      },
    },
  }
}
```

In the `.None => if is_extern_with_conv { .None } else { … emit_direct_tail_call … }` branch, compute both ABIs and only take the tail-call path when they match; otherwise return `.None` so the caller falls back to a normal `emit_op` call (which now coerces to `dest_vt` and returns via `emit_return_coerce`):

```tw
.None => {
  callee_phys := case ctx.prepared_funcs[fid.id] {
    .Some(pf) => pf.phys_return,
    .None => .None,
  }
  self_phys := case current_prepared_func(ctx) {
    .Some(pf) => pf.phys_return,
    .None => .None,
  }
  if is_extern_with_conv or !phys_abi_eq(callee_phys, self_phys) {
    .None
  } else {
    .Some(call_emit.emit_direct_tail_call(fid, args, ctx, buf, call_emit.CallEmitFns.{ /* unchanged */ }))
  }
},
```

(Preserve the existing `CallEmitFns.{...}` literal exactly.) Patterns 2/3 of `try_emit_tail_op` — tail `AIf`/`AMatch` — route arms through `emit_tail_expr → emit_return_coerce`, which already coerces each arm to `phys_return`, so they need no change; only the direct-`ACall` ReturnCall path is unguarded.

- [ ] **Step 3: Self-host + suite**

Run: `make bundle-cli 2>&1 | tail -5 && make boot-test 2>&1 | tail -8`
Expected: fixed point + all tests pass. Pre-routing all `phys_return` are `.None` so `phys_abi_eq` is always true → every current tail call is still emitted → no codegen change.

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/codegen/emit/calls.tw boot/compiler/codegen/emit.tw
git commit -m "codegen: direct-call result coerces to destination vt; gate direct tail call on matching phys-return ABI"
```

### Task 8: Re-apply the `route_func` result-slot typing pass

Re-author the reverted routing so a slot bound from an `AMatch`/`AIf` is typed `PVecI64` when every non-diverging arm yields a typed source, and retype the result slot + typed arm-atom slots. This is where behavior actually changes.

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw`

- [ ] **Step 1: Read the current routing structures**

Run: `sed -n '199,330p' boot/compiler/backend/route_typed_vec.tw` and `sed -n '855,930p' boot/compiler/backend/route_typed_vec.tw`
Study `route_func`, `result_consumed_typed_only`, `v_group_escapes`/`op_group_escapes`, and how a slot's `wasm_type` is currently retyped to `PVecI64` (the payload/field/builder-candidate cases). The new code mirrors those patterns.

- [ ] **Step 2 (PREREQUISITE — highest-risk step): tail-context escape relaxation**

`v_group_escapes` treats a terminal `.Atom(a)` as an escape (`slot_in(a, vs)`, route_typed_vec.tw:871) while `.Return(.Some(_)) => false`. This asymmetry is a *false positive*: when a function's tail result is a bare `Atom(a)` (rather than `Return(Some(a))`), the walker flags `a` as escaping even though emit coerces it at the physical-return boundary. This same false positive blocks payload/control-result slots from `eligible_v`, so it must be fixed first.

**Do NOT make a blanket `.Atom(a) => false`** — that is unsound. An arm atom that yields into an *intermediate* result slot (which may itself be boxed) genuinely escapes; only the atom consumed as the **function's physical return** may be relaxed. Thread a `tail: Bool` context:

```tw
// tail = true only at the function-body / tail-position terminal, where the
// atom becomes the physical return (coerced by emit_return_coerce). It stays
// true through peeled `.Let` bodies in tail position, but is set FALSE when
// descending into an op's sub-expressions (arm bodies, loop bodies), because
// those atoms flow into an intermediate slot, not the function return.
fn v_group_escapes_t(
  expr: PreparedExpr, vs: Dict<Int, Bool>, relaxed: Dict<Int, Bool>,
  ids: RouteIds, builtins: BuiltinRegistry, tail: Bool,
) Bool {
  case expr {
    .Let(_, op, body) => op_group_escapes(op, vs, relaxed, ids, builtins)
      or v_group_escapes_t(body, vs, relaxed, ids, builtins, tail),
    .Atom(a) => if tail { false } else { slot_in(a, vs) },
    .Return(.Some(_)) => false,
    .Break(.Some(a)) => slot_in(a, vs),
    _ => false,
  }
}
```

Keep the existing `v_group_escapes` as the `tail: false` entry point (`op_group_escapes` recurses into arm/loop bodies via `v_group_escapes`, i.e. `tail: false` — unchanged, sound). Add a tail-aware entry used by result-consumption checks:

```tw
fn result_consumed_typed_only_tail(
  expr: PreparedExpr, result_sid: Int, relaxed: Dict<Int, Bool>,
  ids: RouteIds, builtins: BuiltinRegistry,
) Bool {
  singleton: Dict<Int, Bool> = Dict.new()
  singleton[result_sid] = true
  !v_group_escapes_t(expr, singleton, relaxed, ids, builtins, true)
}
```

Switch the payload (2c) and control-result eligibility checks to `result_consumed_typed_only_tail`. **Verify no regression**: `make bundle-cli && make boot-test`. If a test regresses, the tail context leaked into a non-tail position — tighten where `tail: true` is passed.

- [ ] **Step 3: Add `arm_verdict` + `arm_result_atom`, consulting `eligible_v`**

`arm_verdict` must consult **physical** typedness (`eligible_v`, the "will be `PVecI64`" set built in `route_func`), NOT the mono-level `atom_is_int_vector_slot` — a `Vector<Int>` *param* is mono-`Vector<Int>` but physically boxed `PVec`, so a mono check would type a boxed arm and reintroduce the exact box-into-typed store. Import `expr_always_diverges` from `compiler.codegen.emit.helpers`.

```tw
// The tail atom of an arm body, if it ends in a bare Atom/Return (post-let chain).
fn arm_result_atom(body: PreparedExpr) PreparedAtom? {
  case body {
    .Atom(a) => .Some(a),
    .Let(_, _, rest) => arm_result_atom(rest),
    .Return(.Some(a)) => .Some(a),
    _ => .None,
  }
}

// 0 = diverging (skip), 1 = physically-typed slot source, 2 = disqualify.
fn arm_verdict(body: PreparedExpr, eligible_v: Dict<Int, Bool>) Int {
  if expr_always_diverges(body) { return 0 }
  case arm_result_atom(body) {
    .Some(.ASlot(s)) => if eligible_v.has(s.id) { 1 } else { 2 },
    _ => 2,
  }
}
```

Because `eligible_v` gates on physical typedness, a result slot fed by a **closure/global-closure call** (never added to `eligible_v`; closures.tw stores mono-derived results with no dest_vt plumbing) is automatically verdict 2 → left boxed. That is the intended invariant for this milestone: control-result typing covers payload/typed-return-call arm sources only, not closure-call sources. Task 9 adds a negative test asserting a closure-call-fed `case` result stays correct (boxed).

- [ ] **Step 4: Add the control-flow-result eligibility pass (after `eligible_v` is populated)**

`arm_verdict` reads `eligible_v`, so this pass must run **after** the payload (2c), typed-return-call (2c''), capture (2c'), and gather-fixpoint (2d) sections populate it. Add it as a new section (call it 2e) right after the gather fixpoint, and iterate to a fixpoint too (a typed control-result slot can feed another control-result or gather):

```tw
fn control_result_typed(binding_op: PreparedOp, eligible_v: Dict<Int, Bool>) Bool {
  arms_bodies := case binding_op {
    .AMatch(_, arms) => arms.map(fn(arm) { arm.body }),
    .AIf(_, then_e, else_e) => [then_e, else_e],
    _ => return false,
  }
  saw_typed := false
  for body in arms_bodies {
    v := arm_verdict(body, eligible_v)
    if v == 2 { return false }
    if v == 1 { saw_typed = true }
  }
  saw_typed
}
```

For each slot `sid` bound from an `AMatch`/`AIf` op whose `control_result_typed` is true AND `result_consumed_typed_only_tail(pf.body, sid, …)` holds, set `eligible_v[sid] = true`. Recover the binding op via the existing slot→binding-op walk used by the other sections (grep how 2c locates `payload_read_sids`' binding op; reuse that traversal to collect `(sid, AMatch/AIf op)` pairs). The subsequent existing retype step (which turns every `eligible_v` slot's `wasm_type` into `PVecI64`) then retypes these result slots uniformly — no separate retyping code needed. The typed arm-atom slots are already in `eligible_v` (that is why the arm passed verdict 1), so they are retyped by the same step.

- [ ] **Step 5: Self-host + suite**

Run: `make bundle-cli 2>&1 | tail -5 && make boot-test 2>&1 | tail -12`
Expected: fixed point + all tests pass. The boot compiler's own `case` accessors have no `eligible_v` payload arms, so the pass is inert on boot source. If a test regresses, a slot was typed whose arm is not actually in `eligible_v` → check the fixpoint ordering and `arm_verdict`.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/backend/route_typed_vec.tw
git commit -m "route: type case/if result slots when every non-diverging arm is a typed source"
```

### Task 9: Integration + soundness verification

**Files:**
- Create: `boot/tests/typed_control_result_test.tw` (or add to an existing routing test module — grep `boot/tests/` for the typed-vector test file: `grep -rln "PVecI64\|typed.*vector\|as_ints" boot/tests/`)

- [ ] **Step 1: Dataframe repro must not trap and must drop the sort**

Run: `target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw 2>&1 | grep -i "sort idx by amount"`
Expected: runs without trap; `sort idx by amount` time drops well below the ~1342ms baseline. Record the number.

- [ ] **Step 2: WAT assertion — no `box_i64` before the typed arm store**

Run (`as_ints` is defined at `examples/performance/dataframe/frame/column.tw:74`; build the entry that uses it):
```bash
target/twk build examples/performance/dataframe/frame/column.tw -o /tmp/df.wat
# locate as_ints in the WAT (grep the function's name comment) and inspect the IntCol arm:
grep -n "box_i64\|unbox_i64\|as_ints" /tmp/df.wat | head
```
Expected: the `as_ints` `IntCol` arm has no `call $rt_arr__box_i64` before its `local.set` of the result slot. (Find the function via its name comment / runtime index as per the WAT-tracing workflow.)

- [ ] **Step 3: Write focused router regression programs**

Create `boot/tests/typed_control_result_test.tw` with runnable cases that must produce correct values (they exercise the routed path end-to-end; correctness of output is the assertion). Use the project's assertion style (grep an existing test for the helper, e.g. `assert_eq`/`expect`):

```tw
// All-typed non-diverging arms → typed result slot, correct values.
fn pick(tag: Int, a: Vector<Int>, b: Vector<Int>) Vector<Int> {
  case tag { 0 => a, 1 => b, _ => a }
}

// Diverging arm alongside typed arms → still typed (Never skipped).
fn as_ints_or_die(tag: Int, v: Vector<Int>) Vector<Int> {
  case tag { 0 => v, _ => error("no") }
}

// Boxed caller of a typed-return function → valid Wasm, correct sum.
fn sum(xs: Vector<Int>) Int {
  total := 0
  for x in xs { total = total + x }
  total
}

// Negative: a case whose typed arm is fed by a CLOSURE call must stay correct
// (result slot left boxed — closure calls are excluded from control-result
// typing this milestone). Value correctness is the assertion; this guards that
// the exclusion holds rather than producing a boxed-into-typed store.
fn via_closure(tag: Int, v: Vector<Int>) Vector<Int> {
  f := fn(w: Vector<Int>) Vector<Int> { w }
  case tag { 0 => f(v), _ => v }
}

// exercise them
xs: Vector<Int> = collect i in range(5) { i }
ys: Vector<Int> = collect i in range(5) { i * 2 }
// assert pick(0, xs, ys)[2] == 2
// assert pick(1, xs, ys)[2] == 4
// assert as_ints_or_die(0, xs)[3] == 3
// assert sum(pick(1, xs, ys)) == 20
// assert via_closure(0, xs)[3] == 3   // closure-fed arm, boxed result, still correct
// assert via_closure(1, xs)[3] == 3
```

Replace the `// assert` comments with the real assertion helper. The mixed typed/boxed rejection case is covered implicitly: a `case` returning a `Vector<Int>` in one arm and a boxed source in another must still produce correct values (the router leaves it boxed). Add such a case too.

- [ ] **Step 4: Run the new tests + full suite**

Run: `target/twk run boot/tests/typed_control_result_test.tw 2>&1 | tail -5 && make boot-test 2>&1 | tail -8`
Expected: the new program runs correct; full suite green.

- [ ] **Step 5: Commit**

```bash
git add boot/tests/typed_control_result_test.tw
git commit -m "test: typed case/if result-slot routing (all-typed/diverging/mixed/boxed-caller)"
```

- [ ] **Step 6: Update the design/history docs**

Mark the design doc and `typed-vector-representation.md` as landed (remove the "reverted routing" caveat; record the dataframe sort number from Step 1). Commit:

```bash
git add docs/plans/performance/vector/
git commit -m "docs: record landed Expected{vt,mono} coercion + dataframe sort win"
```

---

## Verification summary

- **Stage 1 (Tasks 1–2):** self-host fixed point + full suite + byte-identical WAT (`IDENTICAL` diff). Any codegen difference means a call site derived a different vt — a bug.
- **Stage 2 pre-routing (Tasks 3–7):** each self-hosts + suite green with no codegen change (physical vt still equals mono vt until Task 8).
- **Stage 2 routing (Task 8):** self-host + suite green (inert on boot source).
- **Integration (Task 9):** dataframe repro doesn't trap and drops the sort; WAT shows no `box_i64` before typed stores; router regression program produces correct values across all-typed / diverging / mixed / boxed-caller shapes.
