# fold_core_expr Combinator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `fold_children` child-traversal combinator for Core IR and migrate the closure free-variable analysis onto it, so the "recurse into all children" logic lives in one exhaustive, checker-guarded place instead of being re-implemented (with unsound no-op `_ =>` arms) in every pass.

**Architecture:** `fold_children<A>(expr, acc, f)` is the single exhaustive `case expr.kind` over every `CoreExprKind`'s immediate `CoreExpr` children (no `_ =>`). Analysis passes keep explicit arms only for nodes they specialize and delegate the rest via `_ => fold_children(...)`, whose default soundly recurses. Pilot consumer: `collect_free_vars` in `lower_core/closures.tw`.

**Tech Stack:** Twinkle (boot compiler, `.tw`). Build: `make bundle-cli` (self-host). Boot tests: `target/twk run boot/tests/main.tw`. Format: `target/twk fmt`.

**Spec:** `docs/plans/fold-core-expr.md`.

---

## File structure

- Create `boot/compiler/core_fold.tw` — the `fold_children` combinator (one responsibility: enumerate a Core IR node's immediate children).
- Create `boot/tests/suites/core_fold_suite.tw` — unit tests for the combinator.
- Modify `boot/tests/main.tw` — register the new suite.
- Modify `boot/compiler/lower_core/closures.tw` — migrate `collect_free_vars_inner` onto `fold_children`.

---

## Task 1: The `fold_children` combinator + unit tests

**Files:**
- Create: `boot/compiler/core_fold.tw`
- Create: `boot/tests/suites/core_fold_suite.tw`
- Modify: `boot/tests/main.tw`

Task 1 touches only a new module + tests (not the compiler's own pipeline yet), so it is verified with `target/twk run boot/tests/main.tw` — **no `make bundle-cli` needed** (the current `target/twk` compiles the new `.tw` source as part of the test program).

- [ ] **Step 1: Write the failing test suite**

Create `boot/tests/suites/core_fold_suite.tw`:

```tw
use compiler.core_fold.{fold_children}
use compiler.core_ir.{CoreExpr}
use lib.source.span
use tests.assert
use tests.runner

fn lit_int(n: Int) CoreExpr {
  .{ kind: .LitInt(n), ty: .Void, span: span.new(0, 0, 0) }
}

// Collect the LitInt values of the IMMEDIATE children only (fold_children is
// one level deep; it does not descend).
fn child_ints(expr: CoreExpr) Vector<Int> {
  fold_children(
    expr,
    [],
    fn(acc, child) {
      case child.kind {
        .LitInt(n) => acc.append(n),
        _ => acc,
      }
    },
  )
}

pub fn suite() runner.Suite {
  runner
    .suite("core_fold")
    .test(
      "call folds callee then args in order",
      fn() {
        e := CoreExpr.{
          kind: .Call(lit_int(1), [lit_int(2), lit_int(3)]),
          ty: .Void,
          span: span.new(0, 0, 0),
        }
        try assert.equal(child_ints(e), [1, 2, 3])
        .Ok({})
      },
    )
    .test(
      "contract call folds receiver then args",
      fn() {
        e := CoreExpr.{
          kind: .ContractCall(.IntoIterator, "iter", lit_int(7), [lit_int(8)]),
          ty: .Void,
          span: span.new(0, 0, 0),
        }
        try assert.equal(child_ints(e), [7, 8])
        .Ok({})
      },
    )
    .test(
      "leaf has no children",
      fn() {
        try assert.equal(child_ints(lit_int(5)), [])
        .Ok({})
      },
    )
    .test(
      "make closure has no expr children",
      fn() {
        e := CoreExpr.{
          kind: .MakeClosure(.{ id: 0 }, []),
          ty: .Void,
          span: span.new(0, 0, 0),
        }
        try assert.equal(child_ints(e), [])
        .Ok({})
      },
    )
    .test(
      "folds one level only (does not descend)",
      fn() {
        inner := CoreExpr.{ kind: .Call(lit_int(9), []), ty: .Void, span: span.new(0, 0, 0) }
        e := CoreExpr.{ kind: .Call(inner, [lit_int(2)]), ty: .Void, span: span.new(0, 0, 0) }
        // Visits `inner` (a Call, not a LitInt) and lit_int(2); does NOT descend
        // into `inner` to reach lit_int(9).
        try assert.equal(child_ints(e), [2])
        .Ok({})
      },
    )
}
```

- [ ] **Step 2: Register the suite in `boot/tests/main.tw`**

Add the import next to the other `use .suites.*` lines (alphabetical-ish, near `core_ir_suite`):

```tw
use .suites.core_fold_suite
```

Add the suite to the list passed to the runner, next to the `core_ir_suite.suite(),` entry:

```tw
  core_fold_suite.suite(),
```

- [ ] **Step 3: Run the suite to verify it fails**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -5`
Expected: a compile error — `core_fold` module / `fold_children` not found (the module doesn't exist yet).

- [ ] **Step 4: Implement `fold_children`**

Create `boot/compiler/core_fold.tw`:

```tw
//! Generic one-level child traversal for Core IR analysis passes.
//!
//! `fold_children` is the SINGLE exhaustive enumeration of every CoreExprKind's
//! immediate CoreExpr children. Analysis passes keep explicit arms only for the
//! nodes they specialize and delegate the rest via `_ => fold_children(...)`,
//! whose default soundly recurses into all children.
//!
//! INVARIANT: keep this `case` exhaustive — NEVER add a `_ =>` arm here. Adding a
//! CoreExprKind variant must break this function until its children are wired;
//! that is the compiler-enforced checklist keeping every analysis pass correct.
//!
//! BINDING-UNAWARE: this folds `Let` body and `Match` arm bodies with no context
//! change. Scope-sensitive passes (threading a bound-set) must special-case
//! `Let`/`Match` themselves and must NOT delegate them to `fold_children`.

use compiler.core_ir.{CoreExpr}

pub fn fold_children<A>(expr: CoreExpr, acc: A, f: fn(A, CoreExpr) A) A {
  case expr.kind {
    .LitInt(_) => acc,
    .LitFloat(_) => acc,
    .LitBool(_) => acc,
    .LitStr(_) => acc,
    .LitVoid => acc,
    .Local(_) => acc,
    .GlobalLocal(_) => acc,
    .GlobalFunc(_) => acc,
    .Continue => acc,
    .Let(_, value, body) => {
      cur := f(acc, value)
      f(cur, body)
    },
    .Assign(_, value) => f(acc, value),
    .GlobalSet(_, value) => f(acc, value),
    .BinOp(_, lhs, rhs) => {
      cur := f(acc, lhs)
      f(cur, rhs)
    },
    .UnOp(_, inner) => f(acc, inner),
    .Call(callee, args) => {
      cur := f(acc, callee)
      args.fold(cur, f)
    },
    .ContractCall(_, _, recv, args) => {
      cur := f(acc, recv)
      args.fold(cur, f)
    },
    .MakeClosure(_, _) => acc,
    .If(cond_e, then_e, else_e) => {
      cur := f(acc, cond_e)
      cur = f(cur, then_e)
      f(cur, else_e)
    },
    .Match(scrut, arms) => {
      cur := f(acc, scrut)
      arms.fold(cur, fn(a, arm) { f(a, arm.body) })
    },
    .Loop(body) => f(acc, body),
    .Break(val) => case val {
      .Some(v) => f(acc, v),
      .None => acc,
    },
    .Return(val) => case val {
      .Some(v) => f(acc, v),
      .None => acc,
    },
    .Defer(inner) => f(acc, inner),
    .Record(_, fields) => fields.fold(acc, fn(a, fi) { f(a, fi.value) }),
    .RecordGet(target, _) => f(acc, target),
    .RecordUpdate(base, _, value) => {
      cur := f(acc, base)
      f(cur, value)
    },
    .Variant(_, _, args) => args.fold(acc, f),
    .ArrayLit(elems) => elems.fold(acc, f),
    .Index(base, idx) => {
      cur := f(acc, base)
      f(cur, idx)
    },
  }
}
```

- [ ] **Step 5: Format the new files**

Run: `target/twk fmt boot/compiler/core_fold.tw boot/tests/suites/core_fold_suite.tw`
Expected: `Formatted: ...` (idempotent on re-run).

- [ ] **Step 6: Run the suite to verify it passes**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -1`
Expected: `Ran <N> tests: <N> passed` (count increased by 5 vs. before Task 1).

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/core_fold.tw boot/tests/suites/core_fold_suite.tw boot/tests/main.tw
git commit -m "Add fold_children Core IR child-traversal combinator

One exhaustive (no-wildcard) enumeration of every CoreExprKind's immediate
CoreExpr children, with unit tests covering Call/ContractCall ordering, leaves,
zero-child MakeClosure, and the one-level-only contract.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Migrate `collect_free_vars` onto `fold_children`

**Files:**
- Modify: `boot/compiler/lower_core/closures.tw`

This is a behavior-preserving refactor of a pass *inside* the boot compiler, so it requires a self-host rebuild (`make bundle-cli`) to take effect, and is verified by the full boot suite staying green (the channel fan-in/out test exercises free-vars over a `ContractCall` inside a closure — the original bug's guard).

- [ ] **Step 1: Confirm the regression guard exists and is green (baseline)**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -1`
Expected: `Ran <N> tests: <N> passed` (includes `channel concurrency` → "fan-in fan-out delivers every value exactly once", which uses `for value in ch` inside `Task.spawn`). Note the count.

- [ ] **Step 2: Rewrite `collect_free_vars_inner` to use `fold_children`**

In `boot/compiler/lower_core/closures.tw`, add to the imports (near the other `use compiler.*` lines):

```tw
use compiler.core_fold.{fold_children}
```

Replace the entire `collect_free_vars_inner` function (the `case expr.kind { ... }` body, currently with arms for `.Local`, `.GlobalLocal`, `.Let`, `.Assign`, `.GlobalSet`, `.BinOp`, `.UnOp`, `.Call`, `.If`, `.Match`, `.Loop`, `.Break`, `.Return`, `.Record`, `.RecordGet`, `.RecordUpdate`, `.Variant`, `.ArrayLit`, `.Index`, `.MakeClosure`, `.Defer`, and a `_ =>`) with:

```tw
fn collect_free_vars_inner(
  expr: CoreExpr,
  bound: Dict<Int, Bool>,
  captured: Dict<Int, Bool>,
  result: Vector<LocalId>,
) FreeVarState {
  case expr.kind {
    .Local(id) => {
      if !bound.has(id.id) and !captured.has(id.id) {
        captured[id.id] = true
        result = .append(id)
      }

      .{ bound, captured, result }
    },
    .GlobalLocal(_) => .{ bound, captured, result },
    .Let(local, value, body) => {
      st := collect_free_vars_inner(value, bound, captured, result)
      st.bound[local.id] = true
      collect_free_vars_inner(body, st.bound, st.captured, st.result)
    },
    .Assign(local, value) => {
      if !bound.has(local.id) and !captured.has(local.id) {
        captured[local.id] = true
        result = .append(local)
      }

      collect_free_vars_inner(value, bound, captured, result)
    },
    .Match(scrut, arms) => {
      st := collect_free_vars_inner(scrut, bound, captured, result)

      for arm in arms {
        arm_bound := collect_pattern_bound(arm.pattern, st.bound)
        st = collect_free_vars_inner(arm.body, arm_bound, st.captured, st.result)
      }

      st
    },
    .MakeClosure(_, free_vars) => {
      for id in free_vars {
        if !bound.has(id.id) and !captured.has(id.id) {
          captured[id.id] = true
          result = .append(id)
        }
      }

      .{ bound, captured, result }
    },
    // Every non-binding, non-special node: recurse into all children with the
    // same bound-set. This sound default replaces ~15 hand-written recurse arms
    // and covers ContractCall and any future non-binding variant for free.
    // (Let/Match stay explicit above because they introduce bindings, which
    // fold_children is intentionally unaware of.)
    _ => fold_children(
      expr,
      FreeVarState.{ bound, captured, result },
      fn(st, child) { collect_free_vars_inner(child, st.bound, st.captured, st.result) },
    ),
  }
}
```

(`collect_pattern_bound`, `FreeVarState`, and `collect_free_vars` are unchanged.)

- [ ] **Step 3: Format**

Run: `target/twk fmt boot/compiler/lower_core/closures.tw`
Expected: `Formatted: ...` or no change.

- [ ] **Step 4: Rebuild the self-hosted compiler**

Run: `make bundle-cli 2>&1 | grep -iE "Fixed point|error"`
Expected: `Fixed point reached: stage3 == stage4` (the boot compiler compiles itself with the refactor; no errors).

- [ ] **Step 5: Run the full boot suite (behavior preserved)**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -1`
Expected: `Ran <N> tests: <N> passed` — same `<N>` as Step 1 (refactor changes no behavior; the channel fan-in/out guard still passes).

- [ ] **Step 6: Lint**

Run: `target/twk lint boot/main.tw 2>&1 | grep -i closures; echo done`
Expected: `done` with no findings for `closures.tw`.

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/lower_core/closures.tw
git commit -m "Migrate collect_free_vars onto fold_children

Replace the hand-written structural recursion (whose no-op _ => arm caused the
ContractCall capture miscompile) with explicit arms for the binding/special
nodes plus a sound _ => fold_children default. Behavior-preserving; the channel
fan-in/out test guards the original bug.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Follow-up candidates (NOT in this plan)

These are deferred per the spec ("migrate only if they fit cleanly"); they are not bugs and need their own evaluation before becoming tasks:

- **`core_linker/dce.tw` `collect_func_refs_into`** — a CoreExpr reachability fold. It already special-cases `ContractCall` (via `contract_call_refs`), so it was never affected by the original bug. Migrating it (keep the ref-collecting arms: `GlobalFunc`, `GlobalLocal`, `GlobalSet`, `MakeClosure`, `Call` callee, `ContractCall`; `_ => fold_children` for the rest) is a clean refactor but optional.
- **`monomorphize.tw` collector sub-passes** — only the pure CoreExpr *analysis* folds qualify; its substitution/clone work is a transform and out of scope for this combinator.
- **Not a candidate:** the planner scan (`codegen/wasm_plan_scan.tw`) traverses `PreparedExpr`, a different IR — `fold_children` (CoreExpr) does not apply.

---

## Self-review notes

- Spec coverage: combinator (Task 1) + free-vars pilot (Task 2) + binding-unaware doc (in `core_fold.tw` header and the Task 2 comment) + tests (Task 1 unit tests + Task 2 regression guard) — all covered. DCE/monomorphize are spec "candidates," captured as follow-ups.
- The combinator's `case` lists all 29 `CoreExprKind` variants with no `_ =>`.
- Type consistency: `fold_children<A>(expr, acc, f)` is used identically in Task 1 (`A = Vector<Int>`) and Task 2 (`A = FreeVarState`).
