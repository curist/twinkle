# Boot Checker Refactoring Plan

**Status:** Complete
**Date:** 2026-03-19
**Scope:** `boot/compiler/checker.tw` — extract helpers to reduce duplication and flatten nesting
**Primary tests:** `boot/tests/suites/checker_suite.tw`, `boot/tests/suites/checker_coverage_suite.tw`

---

## Completed (pre-plan)

- `span.Span` → `Span` via destructured import
- Long inline `diag.error(...)` calls split into variable definitions
- `synth`, `synth_inner`, `check_expr`, `unify` — InferCtx moved to first param for method-call syntax
- `builtin_module_aliases()` inlined into `collect_module_aliases`

---

## R1 — Extract `check_args` helper

**Pattern:** Three call paths repeat the same arg-checking loop:

```tw
for i in range(args.len()) {
  ar := cur_ctx.check_expr(args[i], param_tys[i], cur_diags)
  cur_ctx = ar.ctx
  cur_diags = ar.diags
}
```

**Locations:**
- `synth_call` direct call path (line ~547)
- `try_synth_module_qualified_call` (line ~694)
- `try_synth_method_call` (line ~759)

**Proposed helper:**
```tw
fn check_args(ctx: InferCtx, args: Vector<Expr>, param_tys: Vector<MonoType>, diags: Vector<Diagnostic>) CheckOut
```

**Files:** `boot/compiler/checker.tw`

---

## R2 — Extract iterable binding setup

**Pattern:** `synth_collect`, `check_collect`, and `check_for` all repeat: synth iterator, call `iterable_binding_info_of`, bind pattern name, bind optional index with secondary type check, check optional condition.

The iterable + pattern + index binding block is ~25 lines duplicated across all three functions. The condition check (~6 lines) is duplicated between `synth_collect` and `check_collect`.

**Proposed helper:**
```tw
type IterBindResult = .{ ctx: InferCtx, diags: Vector<Diagnostic>, elem_ty: MonoType }

fn bind_iterable_vars(ctx: InferCtx, iter_expr: Expr, pattern: String?, index: String?, diags: Vector<Diagnostic>) IterBindResult
```

Then `check_for`, `synth_collect`, and `check_collect` call this helper and only differ in how they handle the body.

**Files:** `boot/compiler/checker.tw`

---

## R3 — Extract `find_record_field_type`

**Pattern:** Record field lookup with type-arg substitution appears in three places:

```tw
var_map := build_var_map(type_params, type_args)
for f in fields {
  if f.name == field_name {
    return subst_vars(f.ty, var_map)
  }
}
```

**Locations:**
- `synth_field` — field access (line ~849)
- `check_record_lit` — record construction field checking (line ~934)
- `synth_assign_op` — field assignment (line ~1749)

**Proposed helper:**
```tw
fn find_record_field_type(fields: Vector<ResolvedField>, type_params: Vector<String>, type_args: Vector<MonoType>, name: String) MonoType?
```

**Files:** `boot/compiler/checker.tw`

---

## R4 — Extract `bind_optional`

**Pattern:** Four+ places repeat the optional-name binding pattern:

```tw
case name_opt {
  .Some(name) => { ctx = bind_local(ctx, name, ty) },
  .None => {},
}
```

**Locations:**
- `check_for` pattern binding (line ~1514)
- `synth_collect` pattern binding (line ~1600)
- `check_collect` pattern binding (line ~1648)
- `check_pattern` ident binding (line ~1350)

**Proposed helper:**
```tw
fn bind_optional(ctx: InferCtx, name: String?, ty: MonoType) InferCtx {
  case name {
    .Some(n) => bind_local(ctx, n, ty),
    .None => ctx,
  }
}
```

**Files:** `boot/compiler/checker.tw`

---

## R5 — Flatten `synth_if` Never-branch handling

**Current:** 3-level nested case for Never-type skipping:

```tw
case then_r.ty {
  .Never => ...,
  _ => {
    case else_r.ty {
      .Never => ...,
      _ => { u2 := ... },
    }
  }
}
```

**Proposed:** Flatten with early returns:

```tw
if then_r.ty == .Never { return .{ ty: else_r.ty, ... } }
if else_r.ty == .Never { return .{ ty: then_r.ty, ... } }
u2 := else_r.ctx.unify(then_r.ty, else_r.ty, s, else_r.diags)
.{ ty: then_r.ty, ctx: u2.ctx, diags: u2.diags }
```

Note: requires a `is_never(ty)` helper or matching `.Never` in a guard, since Twinkle doesn't support `==` on enums directly. Could use:

```tw
fn is_never(ty: MonoType) Bool {
  case ty { .Never => true, _ => false }
}
```

**Files:** `boot/compiler/checker.tw`

---

## R6 — Deduplicate pre-unification in call paths

**Pattern:** All three call dispatchers repeat the same pre-unification block:

```tw
case call_expected {
  .Some(expected_ret) => {
    pre_u := cur_ctx.unify(inst.ret, expected_ret, span, cur_diags)
    cur_ctx = pre_u.ctx
  },
  .None => {},
}
```

**Locations:**
- `synth_call` direct path (line ~528)
- `try_synth_module_qualified_call` (line ~679)
- `try_synth_method_call` (line ~733)

**Proposed helper:**
```tw
fn pre_unify_return(ctx: InferCtx, inst_ret: MonoType, call_expected: MonoType?, s: Span, diags: Vector<Diagnostic>) CheckOut
```

Could combine with R1 into a single `check_call_args` helper that handles pre-unification + arg checking together, since they always appear in sequence.

**Files:** `boot/compiler/checker.tw`

---

## R7 — Deduplicate interpolation error message

**Pattern:** `check_interpolation_expr` constructs `missing_err` at the top and also has the same message in the final `_ =>` fallback (line ~2178). The `missing_err` variable is already shared across 3 early-return paths, but the tail fallback constructs a new identical diagnostic.

**Fix:** Reuse `missing_err` in the fallback branch instead of constructing a new one.

**Files:** `boot/compiler/checker.tw`

---

## Suggested Execution Order

1. **R4** (bind_optional) — smallest, most mechanical, zero risk
2. **R3** (find_record_field_type) — small pure helper, easy to verify
3. **R1** (check_args) — moderate, clear pattern
4. **R5** (flatten synth_if) — small, improves readability
5. **R7** (interpolation error dedup) — one-line fix
6. **R2** (iterable binding setup) — larger extraction, most duplication removed
7. **R6** (pre-unify return) — optional, pairs well with R1

Rationale: start with low-risk helpers that reduce line count, then tackle the larger structural extractions.

---

## Exit Criteria

1. All `boot/tests/suites/checker_suite.tw` and `checker_coverage_suite.tw` pass after each step.
2. No behavioral changes — refactoring only.
3. Net line reduction from duplicated code.
4. Each helper is called from 2+ sites (no premature abstractions).
