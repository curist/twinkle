# Tail-match result typing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `return_is_typed(as_ints)` fire for tail-position match/if accessors so the function carries a physical `PVecI64` return ABI end-to-end, dropping the dataframe `order_by` sort (~1343ms).

> **STATUS 2026-07-07 — DONE except the sort win.** Tasks 1, 1.5, 2, 3 landed (commits ac30af11, fb5dd236, 7cc6556a); accessor returns type, 2979 tests pass, self-host fixed point. The dataframe sort did **not** drop: the comparator captures the accessor result into a closure, and comparator-capture typing does not type a captured call-result/payload source (capture circularity). See the design doc's *Outcome* section. Dropping the sort is a follow-on feature (type captured call-result/payload sources); brainstorm pending.

**Architecture:** Three cooperating boot-backend pieces (no stage0 parity — semantics-preserving optimization): (1) `is_noreturn` builtin metadata + a noreturn-aware `PreparedExpr` divergence predicate `prep_diverges`; (2) make `return_atom_slots` skip the dead body after an all-diverging op, so the `error` arm's atom and the dead trailing result atom stop poisoning the return-atom set; (3) a gated `tailify` canonicalization that rewrites a tail control-flow result's value-producing arms to `Return`, so the payload read flows through `.Return => false` (consumed-typed-only) with **no** escape relaxation.

**Tech Stack:** Twinkle boot compiler. Verify: `make bundle-cli` (self-host fixed point) + `make boot-test` (~2973) + dataframe repro.

**Design:** `docs/plans/performance/vector/tail-match-result-typing-design.md`
**Depends on:** landed `Expected{vt,mono}` emit foundation (commits 7b91f250..c51ce496).

---

## Key facts (verified against source)

- `PreparedAtom` (prepared_ir.tw:80) has `ASlot(SlotId)` — **not** `ALocal`. The tail is `Atom(ASlot r)`.
- `PreparedExpr` (prepared_ir.tw:91): `Let(SlotId, PreparedOp, PreparedExpr) | Atom(PreparedAtom) | Return(PreparedAtom?) | Break(PreparedAtom?) | Continue`. **No `Unreachable` terminal** — relevant to the R1 fallback.
- `PreparedFunc.return_mono: MonoType` (prepared_ir.tw:145+); `phys_return: ValType?`.
- `return_atom_slots` / `return_atom_slots_op`: typed_param_abi.tw:306-337.
- `return_is_typed`: typed_param_abi.tw:277 (has a `builtins` param already).
- `expr_always_diverges` (PreparedExpr): emit/helpers.tw:41 — `.Let(_, _, body) => diverges(body)`, does **not** inspect the op. `AnfExpr` version with op recursion: anf_analysis.tw:508.
- Builtins: `BuiltinEntry` (builtins.tw:19) has `.name`; `error` registered at builtins.tw:122/456; lookup via `reg.by_id[fid.id]`.
- Pipeline: `prepare_backend` (prepare.tw) does `pre := prepare_backend_preroute(...)`, a depth check, then `repr := analyze_typed_repr(pre.funcs, ...)` and `funcs3 := route_typed_vectors(pre.funcs, ...)`. **Tailify must transform `pre.funcs` before both.**
- `v_group_escapes` `.Return(.Some(_)) => false`: route_typed_vec.tw:872.
- Payload typedness is **producer-driven** (`scan_func_payload_producers` sets only `has_ok_producer`/`bad_producer`; never `bad_consumer` for payloads) — no circular dependency.

---

## Task 1: `is_noreturn` + `prep_diverges` + divergence-aware `return_atom_slots` (R1 make-or-break spike)

This task is the step-0 validation spike: it makes an **explicit-`return`** accessor type (no tailify needed yet) and verifies the retyped module validates and runs. If the dead trailing atom does not validate, stop and address it (fallback note below) before Task 2.

**Files:**
- Modify: `boot/compiler/builtins.tw` (`is_noreturn`)
- Create: `boot/compiler/backend/tailify.tw` (`prep_diverges`, `prep_op_diverges` — tailify itself lands in Task 2)
- Modify: `boot/compiler/backend/typed_param_abi.tw` (`return_atom_slots` divergence-aware; thread `builtins`)
- Test: `/tmp/asints_ret.tw` (explicit-`return` accessor)

- [ ] **Step 1: Add `is_noreturn` to builtins.tw**

Append near the other builtin predicates:

```tw
// True if the builtin never returns (its call diverges). Today only `error`
// (canonical rt.core `trap`). Used by the noreturn-aware divergence predicate.
pub fn is_noreturn(reg: BuiltinRegistry, fid: FuncId) Bool {
  case reg.by_id[fid.id] {
    .Some(e) => e.name == "error",
    .None => false,
  }
}
```

Confirm `FuncId` is imported in builtins.tw (`grep -n "FuncId" boot/compiler/builtins.tw`); add to the `core_ir` use if missing.

- [ ] **Step 2: Create tailify.tw with the divergence predicate**

```tw
//! Tail-match result typing: a noreturn-aware divergence predicate and (Task 2)
//! the tailify canonicalization.

use compiler.backend.prepared_ir.{PreparedExpr, PreparedOp, PreparedFunc, PreparedMatchArm}
use compiler.builtins.{BuiltinRegistry, is_noreturn}
use compiler.core_ir.{FuncId}

// PreparedExpr mirror of expr_always_diverges, but noreturn-aware: a call to an
// is_noreturn builtin diverges, and (unlike emit/helpers.tw's version) it recurses
// into if/match ops. Both are required — adding only the error-call case misses
// `Let(r, AMatch[all arms diverge], ...)`.
pub fn prep_diverges(expr: PreparedExpr, reg: BuiltinRegistry) Bool {
  case expr {
    .Return(_) => true,
    .Break(_) => true,
    .Continue => true,
    .Atom(_) => false,
    .Let(_, op, body) => prep_op_diverges(op, reg) or prep_diverges(body, reg),
  }
}

pub fn prep_op_diverges(op: PreparedOp, reg: BuiltinRegistry) Bool {
  case op {
    .ACall(.AGlobalFunc(fid), _) => is_noreturn(reg, fid),
    .AIf(_, then_e, else_e) => prep_diverges(then_e, reg) and prep_diverges(else_e, reg),
    .AMatch(_, arms) => {
      if arms.len() == 0 {
        return true
      }

      for arm in arms {
        if !prep_diverges(arm.body, reg) {
          return false
        }
      }

      true
    },
    _ => false,
  }
}
```

No central registration is needed — a new `boot/compiler/backend/*.tw` file is pulled in transitively once something `use`s it (Task 1 Step 3 imports `prep_op_diverges`; Task 2 imports `tailify_funcs`). This is compiler source compiled directly by the boot build, not `@std` stdlib, so there is no `core_lib` regeneration. Confirm `BuiltinRegistry`, `PreparedMatchArm`, and `FuncId` resolve from the `use` paths shown (grep an existing backend file for the exact module paths if a build error names one).

- [ ] **Step 3: Make `return_atom_slots` skip the dead body after an all-diverging op**

In `boot/compiler/backend/typed_param_abi.tw`, thread `reg` and add the single dead-body skip. Replace `return_atom_slots`/`return_atom_slots_op` (306-337):

```tw
fn return_atom_slots(expr: PreparedExpr, acc: Vector<Int>, reg: BuiltinRegistry) Vector<Int> {
  case expr {
    .Let(_, op, body) => {
      acc2 := return_atom_slots_op(op, acc, reg)

      // If the op never falls through (all arms diverge, or a noreturn call), the
      // body is dead code — do not collect its atoms. This drops both the dead
      // trailing `Atom(r)` after an all-diverging match and a diverging arm's own
      // `Let(t, error(...), Atom(t))` tail. A value-producing arm still reaches
      // its `Return(Some(v))` (collected below) because that arm does not diverge.
      if prep_op_diverges(op, reg) {
        acc2
      } else {
        return_atom_slots(body, acc2, reg)
      }
    },
    .Atom(a) => append_slot(a, acc),
    .Return(.Some(a)) => append_slot(a, acc),
    _ => acc,
  }
}

fn return_atom_slots_op(op: PreparedOp, acc: Vector<Int>, reg: BuiltinRegistry) Vector<Int> {
  case op {
    .AIf(_, then_e, else_e) => {
      acc2 := return_atom_slots(then_e, acc, reg)
      return_atom_slots(else_e, acc2, reg)
    },
    .AMatch(_, arms) => {
      cur := acc

      for arm in arms {
        cur = return_atom_slots(arm.body, cur, reg)
      }

      cur
    },
    .ALoop(inner) => return_atom_slots(inner, acc, reg),
    .ADefer(inner) => return_atom_slots(inner, acc, reg),
    _ => acc,
  }
}
```

Import `prep_op_diverges` into typed_param_abi.tw: add `use compiler.backend.tailify.{prep_op_diverges}`. Update the sole caller in `return_is_typed` (typed_param_abi.tw:288): `slots := return_atom_slots(pf.body, [], builtins)`.

- [ ] **Step 4: Build stage0 → boot**

Run: `cargo run --release -- build boot/main.tw -o /tmp/t1.wasm 2>&1 | tail -8`
Expected: builds clean. Fix any missed `return_atom_slots(...)` call site the type error names.

- [ ] **Step 5: Rebuild the CLI and write the explicit-return spike program**

Run: `make bundle-cli 2>&1 | tail -3`
Then create `/tmp/asints_ret.tw`:

```tw
type Col = { IntCol(Vector<Int>), Other }

fn as_ints(c: Col) Vector<Int> {
  case c {
    .IntCol(v) => return v,
    _ => error("not int"),
  }
}

fn sum_via(c: Col) Int {
  keys := as_ints(c)
  total := 0
  for i in range(keys.len()) { total = total + keys[i] }
  total
}

xs: Vector<Int> = collect i in range(5) { i }
c: Col = .IntCol(xs)
println("${sum_via(c)}")
```

- [ ] **Step 6: THE MAKE-OR-BREAK CHECK — validate + run + confirm typed**

Run:
```bash
target/twk run /tmp/asints_ret.tw 2>&1 | tail -3          # must print 10, no trap/validation error
target/twk build /tmp/asints_ret.tw -o /tmp/asints_ret.wat 2>&1 | tail -2
grep -n "func .*as_ints" /tmp/asints_ret.wat
```
Then inspect the `as_ints` function in the WAT:
- Expected PASS: result type is `(ref null $rt_types__PVecI64)` (typed return fired); the `IntCol` arm has **no** `call $rt_arr__box_i64` before returning `v`; the module validates and `run` prints `10`.
- If it prints 10 and validates: **the make-or-break assumption holds.** Proceed to Task 2.
- If it fails to validate (invalid module / trap at the dead trailing `local.get`): **STOP.** The dead trailing atom does not validate as-is. Fallback: since `PreparedExpr` has no `Unreachable` terminal, add an emit-level guard — in `emit_tail_expr`/`emit_tail_let`, when the terminal atom follows an op for which `expr_always_diverges` holds, emit `.Unreachable` instead of `local.get; coerce; return`. Implement that, re-run this step, and only then continue.

- [ ] **Step 7: Self-host + suite**

Run: `make bundle-cli 2>&1 | tail -3 && make boot-test 2>&1 | tail -1`
Expected: fixed point + all tests pass. The `return_atom_slots` change only affects `Vector<Int>`-returning functions with diverging tail arms; boot has few/none, so the suite should be unchanged.

- [ ] **Step 8: Commit**

```bash
git add boot/compiler/builtins.tw boot/compiler/backend/tailify.tw boot/compiler/backend/typed_param_abi.tw
git commit -m "backend: noreturn-aware divergence + dead-body skip in return_atom_slots (typed explicit-return accessors)"
```

---

## Task 1.5: alias-group-aware call-result/payload eligibility (discovered during T1 spike)

The T1 spike (`tv_mkfn.tw` — accessor + `mk()` producer) validated that `as_ints`
retypes correctly in isolation, but the **caller** `keys := as_ints(mk())` produced
invalid Wasm: `route_func` typed the call-result slot but left its copy (`keys`)
boxed, so the copy store was an un-coerced `PVecI64 -> PVec` mismatch. Root cause:
sections 2c (payload) and 2c'' (call-result) in `route_func` marked only the single
source slot `eligible_v`, unlike section 2 (builder candidates) which propagates
across the `aliases_for` copy group. Fixed by making 2c/2c'' group-aware —
`aliases_for(sid, copy_map)` + a whole-group `!v_group_escapes` gate, retyping every
alias. This both restores validity and types the copy (caller reads route to
`get_i64`/`len_i64` — the actual win). Committed with T1. (Note: the plan's original
`/tmp/asints_ret.tw` spike stays boxed due to its top-level-global producer — the
documented producer-cleanliness contingency; use the `mk()`-function form to observe
retyping.)

## Task 2: `tailify` canonicalization (store-form accessors)

Now handle the idiomatic `=> v` (store) form by rewriting a tail control-flow result's value-producing arms to `Return`, so the payload read flows through `.Return => false`.

**Files:**
- Modify: `boot/compiler/backend/tailify.tw` (add `tailify_func`/`tailify_funcs`)
- Modify: `boot/compiler/backend/prepare.tw` (wire into the pipeline before `analyze_typed_repr`/`route_typed_vectors`)
- Test: `/tmp/asints.tw` (store form)

- [ ] **Step 1: Add tailify to tailify.tw**

```tw
// Rewrite a tail-position control-flow result's value-producing arms to Return, so
// the returned payload flows through the escape analysis's `.Return => false` rule
// (consumed-typed-only) with no store-escape relaxation. Gated to Vector<Int>
// returns to keep the IR-shape change confined to accessor-shaped functions.
pub fn tailify_funcs(funcs: Vector<PreparedFunc>, reg: BuiltinRegistry) Vector<PreparedFunc> {
  collect pf in funcs {
    tailify_func(pf, reg)
  }
}

fn tailify_func(pf: PreparedFunc, reg: BuiltinRegistry) PreparedFunc {
  is_vec_int := case pf.return_mono {
    .Vector(.Int) => true,
    _ => false,
  }

  if !is_vec_int {
    return pf
  }

  pf.body = tailify_spine(pf.body, reg)
  pf
}

// Walk the top-level Let-spine to the terminal `Let(r, AMatch|AIf, Atom(ASlot r))`
// and rewrite that op's arms. Other tail shapes are left unchanged.
fn tailify_spine(expr: PreparedExpr, reg: BuiltinRegistry) PreparedExpr {
  case expr {
    .Let(s, op, rest) => case rest {
      .Atom(.ASlot(s2)) => if s2.id == s.id {
        .Let(s, tailify_result_op(op, reg), rest)
      } else {
        .Let(s, op, tailify_spine(rest, reg))
      },
      _ => .Let(s, op, tailify_spine(rest, reg)),
    },
    _ => expr,
  }
}

fn tailify_result_op(op: PreparedOp, reg: BuiltinRegistry) PreparedOp {
  case op {
    .AMatch(scrut, arms) => .AMatch(scrut, tailify_arms(arms, reg)),
    .AIf(c, t, e) => .AIf(c, tailify_arm(t, reg), tailify_arm(e, reg)),
    _ => op,
  }
}

fn tailify_arms(arms: Vector<PreparedMatchArm>, reg: BuiltinRegistry) Vector<PreparedMatchArm> {
  collect arm in arms {
    PreparedMatchArm.{ pattern: arm.pattern, body: tailify_arm(arm.body, reg) }
  }
}

// A non-diverging arm's tail value becomes a Return; diverging arms (Return / error
// spine) are left untouched. Nested control-flow in the arm tail is returned as-is
// (conservative: its result may be boxed — correct, just not typed).
fn tailify_arm(body: PreparedExpr, reg: BuiltinRegistry) PreparedExpr {
  if prep_diverges(body, reg) {
    return body
  }

  case body {
    .Atom(a) => .Return(.Some(a)),
    .Let(s, op, rest) => .Let(s, op, tailify_arm(rest, reg)),
    _ => body,
  }
}
```

- [ ] **Step 2: Wire tailify into the prepare pipeline**

In `boot/compiler/backend/prepare.tw`, after the `prepared_funcs_depth_exceeds` early-return guard and before `analyze_typed_repr`, transform the funcs once and feed the result to **both** analyses. Read the exact region first (`grep -n "analyze_typed_repr\|route_typed_vectors\|pre.funcs" boot/compiler/backend/prepare.tw`), then:

```tw
funcs_t := tailify_funcs(pre.funcs, builtins)
repr := analyze_typed_repr(funcs_t, builtins, first_user_type_id())
typed_fields := repr.typed_fields
typed_payloads := repr.typed_payloads
funcs3 := route_typed_vectors(
  funcs_t,
  builtins,
  typed_fields,
  typed_payloads,
  repr.capture_abi,
  repr.param_abi.typeable_return,
)
```

Add `use compiler.backend.tailify.{tailify_funcs}` to prepare.tw. (The early-return depth-guard path keeps `pre.funcs` untailified — fine, it bails out entirely.)

- [ ] **Step 3: Build + store-form repro**

Run:
```bash
cargo run --release -- build boot/main.tw -o /tmp/t2.wasm 2>&1 | tail -8
make bundle-cli 2>&1 | tail -3
target/twk run /tmp/asints.tw 2>&1 | tail -3          # /tmp/asints.tw = the `=> v` store form; must print 10
target/twk build /tmp/asints.tw -o /tmp/asints.wat && grep -n "func .*as_ints" /tmp/asints.wat
```
`/tmp/asints.tw` is the store-form program (`case c { .IntCol(v) => v, _ => error(...) }` + sum caller). Expected: prints `10`; `as_ints` result type is `(ref null $rt_types__PVecI64)`; `IntCol` arm has no `box_i64`.

- [ ] **Step 4: Self-host + suite**

Run: `make bundle-cli 2>&1 | tail -3 && make boot-test 2>&1 | tail -1`
Expected: fixed point + all tests pass. Tailify is gated to `Vector<Int>` returns, so only such functions in boot change shape (store-then-return → per-arm return) — semantics-preserving.

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/backend/tailify.tw boot/compiler/backend/prepare.tw
git commit -m "backend: tailify tail control-flow results (Vector<Int> returns) so store-form accessors type"
```

---

## Task 3: Regression tests + dataframe integration

**Files:**
- Create: `boot/tests/tail_match_result_typing_test.tw`
- Verify: dataframe `order_by` bench

- [ ] **Step 1: Write regression cases**

Create `boot/tests/tail_match_result_typing_test.tw` using the project's assertion helper (`grep -rn "fn assert\|expect(" boot/tests/ | head` to find it). Cover the R3/R4 surfaces — correctness of output is the assertion (they exercise the routed path end-to-end):

```tw
type Col = { IntCol(Vector<Int>), FloatCol(Vector<Float>), Other }

// store form + diverging arm
fn as_ints(c: Col) Vector<Int> {
  case c { .IntCol(v) => v, _ => error("not int") }
}

// multiple typed arms (all Vector<Int>)
fn pick(tag: Int, a: Vector<Int>, b: Vector<Int>) Vector<Int> {
  case tag { 0 => a, 1 => b, _ => error("bad") }
}

// non-typed arm present -> must stay correct (result not uniformly typed)
fn with_default(c: Col, d: Vector<Int>) Vector<Int> {
  case c { .IntCol(v) => v, _ => d }
}

// let-spine in arm + diverging arm
fn first_or_die(c: Col) Vector<Int> {
  case c {
    .IntCol(v) => { w := v; w },
    _ => error("no"),
  }
}
```

**Typedness expectations (assert values, not typing):** `as_ints` and `first_or_die`
are the shapes that *do* get a `PVecI64` return. `pick` (returns params) and
`with_default` (mixed arm: typed payload + param `d`) **stay boxed** — a returned
param fails `slot_typed_after_route`, so `return_is_typed` is false. They verify the
conservative path stays *correct*, not that they type. (Aside: in `with_default`,
`route_func` still types the `IntCol` payload `v` independently, then the landed
return-coercion gate re-boxes it at the boxed return — correct, a tiny O(n) box; a
good exercise of the emit foundation.)

Assert (fill in the real helper): `as_ints(.IntCol(collect i in range(4){i}))[2] == 2`; `pick(1, xs, ys)[0] == ys[0]`; `with_default(.Other, xs)[1] == xs[1]`; `first_or_die(.IntCol(xs))[3] == xs[3]`. Run: `target/twk run boot/tests/tail_match_result_typing_test.tw`.

- [ ] **Step 2: Nested-tailify unit sanity (R4)**

Add one case where an arm's tail is itself a match/if, and one already ending in `return`, and assert correct values (tailify must not mis-return or double-return):

```tw
fn nested(c: Col, tag: Int) Vector<Int> {
  case c {
    .IntCol(v) => if tag > 0 { v } else { v },
    _ => return error("x"),
  }
}
```
Assert `nested(.IntCol(xs), 1)[0] == xs[0]` and `nested(.IntCol(xs), 0)[0] == xs[0]`.

- [ ] **Step 3: Dataframe integration + the win**

Run:
```bash
target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw 2>&1 | grep -i "sort idx by amount"
```
Expected: no trap; `sort idx by amount` drops well below ~1342ms. Record the number. Also confirm `as_ints` in the real column module is typed:
```bash
target/twk build examples/performance/dataframe/frame/column.tw -o /tmp/df.wat
grep -n "box_i64" /tmp/df.wat | head    # the as_ints IntCol arm should have none before its return
```
If the sort does **not** drop, check the producer-cleanliness contingency: grep how `IntCol` columns are constructed and confirm no site feeds a boxed source (`bad_producer`). Document the finding.

- [ ] **Step 4: Full suite + commit**

Run: `make boot-test 2>&1 | tail -1`
```bash
git add boot/tests/tail_match_result_typing_test.tw
git commit -m "test: tail-match result typing (store/multi-arm/non-typed-arm/nested/diverging) + dataframe order_by win"
```

- [ ] **Step 5: Update docs**

Record the dataframe sort number in the design doc; if this completes the typed-vector read-wall work, update `typed-vector-representation.md` and remove the plan row from `docs/plans/README.md`, moving both docs to `archive/` per house rules.

```bash
git add docs/plans/
git commit -m "docs: record tail-match result typing landed + dataframe order_by number"
```

---

## Self-review checklist

- **Spec coverage:** is_noreturn (design §1) → T1S1; prep_diverges with op-recursion (§3 note) → T1S2; divergence-aware return_atom_slots (§3) → T1S3; R1 validation spike (§Wasm-validation) → T1S6; tailify gated to Vector<Int> (§2) → T2S1–2; producer-cleanliness contingency (§payload) → T3S3; R3 regressions + R4 nested (§3, §2) → T3S1–2.
- **No placeholders:** every code step shows real code against verified signatures.
- **Type consistency:** `ASlot` (not `ALocal`); `PreparedMatchArm.{ pattern, body }`; `return_atom_slots(_, _, builtins)` threaded at the single caller (return_is_typed); `reg`/`builtins` naming consistent per file.
