# Tuple Destructuring in `case` Arms — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add tuple **patterns** to `case` arms — `case pt { (x, y) => … }`, arity 2–4, with full nesting — in both the boot compiler and the Rust stage0 bootstrap compiler.

**Architecture:** A dedicated surface pattern node (`PatternKind.Tuple` / `Pattern::Tuple`) lowers to the *existing* single-constructor variant Core pattern, `CorePattern::Variant(tuple_tid, vid 0, subs)`. No new Core IR node, so the many `CorePattern` consumers (ANF, opt, ir_print, cfg, linker) are untouched. "Tuple-ness" is confined to three places: the surface parser/AST, the checker's type + refutability-based exhaustiveness rules, and the codegen layer, which must project fields through the **record** layout (no variant tag) instead of the sum layout the `.Variant` emit paths assume by default.

**Tech Stack:** Twinkle (`.tw`) for the boot compiler; Rust for stage0. Boot tests via `@std.testing`; stage0 tests via `cargo test`.

**Design doc:** [tuple-destructuring.md](tuple-destructuring.md). Read it first — this plan implements it.

## Global Constraints

- **Scope is `case` arms only.** No `(a, b) :=` let-binding, no `for (a, b) in …`, no function-parameter tuple patterns. Do **not** migrate any `boot/compiler/*.tw` source to use tuple destructuring — this plan makes the feature *available*, not adopted.
- **Arity is 2–4.** `Tuple2`/`Tuple3`/`Tuple4`. No 1-tuples, no unit. `(p)` with no top-level comma is a parse error in pattern position.
- **Full nesting.** Any pattern at each tuple position (ident, wildcard, literal, variant, sub-tuple) and tuple patterns nested inside variant patterns (`.Some((k, v))`). This rides existing nested-refutable machinery — no new decision-tree code.
- **Conservative exhaustiveness (v1).** A fully-irrefutable tuple arm (all elements irrefutable) counts as catch-all. A `case` over a tuple whose arms are all refutable with no `_` is reported non-exhaustive. Full product-coverage totality is out of scope.
- **Reuse `CorePattern.Variant`.** Lowering emits `.Variant(tuple_tid, VariantId{id:0}, subs)`. Do **not** add a new `CorePattern` kind.
- **Two compilers must stay in lockstep for `make stage2`.** But because boot source is *not* migrated here, no new tuple-pattern syntax enters stage0-compiled source, so `make stage2` only needs to keep reaching a fixed point (Task 7). Boot and stage0 workstreams are otherwise independent; do boot first.
- **Verification gates:** `make boot-test` (boot suite → `Ran N tests: N passed`); `cargo test --release` (targeted filters) for stage0; `make stage2` (`stage3 == stage4`) for the final fixed-point gate. Do **not** run full `cargo test` unfiltered or `make bundle-cli` mid-plan — too slow.
- **Format + lint** every edited `.tw`: `target/twk fmt <file>`, then `target/twk lint boot/main.tw`. Prefer range literals `0..n`; self-rebind sugar (`acc = acc.append(…)`), no numbered copies.
- **Never run `tree-sitter test`** — hand it to the human (project rule).
- Commit messages: imperative subject, what/why/how, no line/count metrics.

## File Map

### Boot compiler (Twinkle)

Surface `PatternKind` consumers — each needs a `.Tuple` arm (the type checker's exhaustiveness will flag any missed one at build time):
- `boot/compiler/ast.tw` — `PatternKind` def (line ~200): add `Tuple(Vector<Pattern>)`.
- `boot/compiler/parser.tw` — `parse_pattern` (~1640): add the `(`-case. `parse_pattern_list(c, end_kind, error_msg)` (~494) is reused as-is.
- `boot/compiler/checker.tw` — `check_pattern` (~3564), `pattern_is_irrefutable` (~3690), `coverage_from_pattern` (~3723), `check_exhaustiveness` (~3820).
- `boot/compiler/lower_core/patterns.tw` — `lower_pattern` (~13).
- `boot/compiler/fmt/printer.tw` — pattern printing.
- `boot/compiler/query/completion.tw`, `query/definition.tw`, `query/hover.tw`, `query/occurrence_build.tw` — recurse into sub-patterns (mirror the `.Variant` arm).
- `boot/compiler/unused_imports.tw` — mirror the `.Variant` arm.

Codegen (`CorePattern.Variant` reused, so only these need the record-tid branch):
- `boot/compiler/codegen/emit.tw` — `emit_pattern_condition` (~3875, guard at ~3911), `emit_pattern_bindings` (~4025, guard at ~4041). Helpers to reuse: `can_match_variant_pattern` (~3859), `record_layout_of_ctx`/`find_field_index` (`codegen/emit/layout_helpers.tw`).
- `boot/compiler/codegen/emit/match.tw` — `is_br_table_eligible` (~223): record tids ineligible.

Tests:
- `boot/tests/suites/parser_suite.tw`, `checker_suite.tw`, `checker_coverage_suite.tw`, `codegen_integration_suite.tw`, `fmt_suite.tw`.

### Stage0 (Rust)

- `src/syntax/ast.rs` — `Pattern` enum (~401): add `Tuple(Vec<Pattern>, Span)`.
- `src/syntax/parser.rs` — `parse_pattern` (~1991): add the `(`-case.
- `src/syntax/pretty.rs` — print `Pattern::Tuple`.
- `src/types/patterns.rs` — `PatternChecker` type + exhaustiveness.
- `src/types/check.rs` — pattern type-checking dispatch if separate from `patterns.rs`.
- `src/ir/lower.rs` — `lower_pattern` (~3286): map `Pattern::Tuple` → `CorePattern::Variant`.
- `src/codegen/emit.rs` — `emit_pattern_condition` (~1709), `emit_pattern_bindings` (~2046): record-tid branch using `record_struct_sym`/`StructGet` (see `emit_record_get` ~3943). `emit_match_arm_chain` (~1567) provides fallthrough.
- `src/query/api.rs`, `src/module/mod.rs` — add `Pattern::Tuple` arms if they match on `Pattern` (Rust exhaustiveness will flag).
- Tests: inline `#[test]` modules in the touched files (follow existing conventions, e.g. `src/syntax/mod.rs` tuple tests).

### Docs / grammar (Task 7)

- `docs/spec.md` — §13.7 line 1126 ("There is no destructuring binding in this release").
- `docs/grammar.ebnf` — `Pattern` rule (~361).
- `tree-sitter-twinkle/grammar.js` — `_pattern` choice (~497); regenerate `src/` + wasm; **hand `tree-sitter test` to the human**.

---

## Task 1: Boot — surface node + parser (`(p, …)` parses to `PatternKind.Tuple`)

Adds the AST node and parsing, threads `.Tuple` through every surface consumer so the build stays green, and stubs the checker/lowering arms (real logic lands in Tasks 2–3). After this task the boot compiler parses tuple patterns and round-trips them through `fmt`, but does not yet type-check or run them.

**Files:**
- Modify: `boot/compiler/ast.tw`, `boot/compiler/parser.tw`, `boot/compiler/fmt/printer.tw`, `boot/compiler/checker.tw`, `boot/compiler/lower_core/patterns.tw`, `boot/compiler/query/completion.tw`, `boot/compiler/query/definition.tw`, `boot/compiler/query/hover.tw`, `boot/compiler/query/occurrence_build.tw`, `boot/compiler/unused_imports.tw`
- Test: `boot/tests/suites/parser_suite.tw`, `boot/tests/suites/fmt_suite.tw`

**Interfaces:**
- Produces: `PatternKind.Tuple(Vector<Pattern>)` — a surface pattern holding 2–4 sub-patterns. Consumed by Tasks 2 (check), 3 (lower).

- [ ] **Step 1: Write the failing parser test**

In `parser_suite.tw`, add a test that parses a `case` with a tuple arm and asserts the arm pattern is a `.Tuple` with the expected arity. Follow the suite's existing parse-assertion style (parse source → inspect AST). Example shape:

```tw
fn test_tuple_pattern_parses() Result<Void, String> {
  m := try parse_module_ok("x := case p { (a, b) => a, _ => 0 }")
  arm := first_case_arm(m)               // suite helper; add if absent
  case arm.pattern.kind {
    .Tuple(subs) => try assert.eq(subs.len(), 2),
    _ => return .Err("expected .Tuple pattern"),
  }
  .Ok({})
}
```

Also add a test that a 5-element tuple pattern `(a, b, c, d, e)` and a bare `(a)` (no comma) each produce a diagnostic.

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — `.Tuple` variant does not exist / parser produces something else.

- [ ] **Step 3: Add the AST variant**

In `ast.tw`, extend `PatternKind`:

```tw
pub type PatternKind = {
  Wildcard,
  Ident(String),
  Literal(Expr),
  Variant(String, Vector<Pattern>),
  QualifiedVariant(Vector<String>, String, Vector<Pattern>),
  Tuple(Vector<Pattern>),
  ErrorPattern,
}
```

- [ ] **Step 4: Parse the `(`-case in `parse_pattern`**

In `parser.tw`, at the top of `parse_pattern` (after the `k := c.kind()` line, alongside the `.Dot` and `.Ident` cases), add:

```tw
if k == .LParen {
  paren_start := c.span()
  parsed := parse_pattern_list(
    c.advance(),
    TokenKind.RParen,
    "expected ',' or ')' in tuple pattern",
  )
  diagnostics_out = .concat(parsed.diagnostics)
  c = parsed.cursor

  if c.kind() == .RParen {
    c = .advance()
  }

  n := parsed.value.len()
  if n < 2 or n > 4 {
    diagnostics_out = .append(
      diag.error(c.span_from(start_pos), "tuple patterns support 2–4 elements; use a record for more"),
    )
    patt := Pattern.{ kind: .ErrorPattern, span: c.span_from(start_pos) }
    return .{ value: patt, cursor: c, diagnostics: diagnostics_out }
  }

  patt := Pattern.{ kind: .Tuple(parsed.value), span: c.span_from(start_pos) }
  return .{ value: patt, cursor: c, diagnostics: diagnostics_out }
}
```

(`start_pos` and `diagnostics_out` are already in scope at the top of `parse_pattern`.)

- [ ] **Step 5: Thread `.Tuple` through the surface consumers**

The build fails until every non-`_` `case … .kind` over a `PatternKind` handles `.Tuple`. Add arms:

- `fmt/printer.tw`: render as `(` + comma-joined printed sub-patterns + `)`. Mirror how `.Variant` arg lists are printed.
- `query/occurrence_build.tw`, `query/completion.tw`, `query/definition.tw`, `query/hover.tw`: recurse into `subs` exactly like the `.Variant(_, subs)` arm does (collect occurrences / binders from each sub-pattern).
- `unused_imports.tw`: mirror the `.Variant` arm (recurse into `subs`).
- `checker.tw` `check_pattern`: **stub** for now —
  ```tw
  .Tuple(_) => .{ ctx, diags: diags.append(.Error(.ParseError(.{ span: pat.span, message: "tuple pattern typing not yet implemented", help_lines: [] }))) },
  ```
  and add trivial arms to `pattern_is_irrefutable` (`.Tuple(_) => false`), `coverage_from_pattern` (`.Tuple(_) => .None`), and the `check_exhaustiveness` collect `case` (`.Tuple(_) => continue`). These stubs are replaced in Task 2.
- `lower_core/patterns.tw` `lower_pattern`: **stub** — `.Tuple(_) => .{ pattern: .Wildcard, ctx }` (replaced in Task 3).

- [ ] **Step 6: Run to verify it passes; fmt round-trips**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS (parse + arity-error tests green). Add/confirm a `fmt_suite.tw` case that `(a, b)` in a case arm formats stably (idempotent).

- [ ] **Step 7: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ast.tw boot/compiler/parser.tw boot/compiler/fmt/printer.tw
target/twk lint boot/main.tw
git add boot/compiler boot/tests
git commit -m "feat(tuple): parse (a, b) case-arm patterns to PatternKind.Tuple"
```

---

## Task 2: Boot — type-check and exhaustiveness for tuple patterns

Replaces the Task 1 checker stubs with real logic: element type-checking against the tuple type, refutability recursion, and the conservative exhaustiveness rule.

**Files:**
- Modify: `boot/compiler/checker.tw` (`check_pattern`, `pattern_is_irrefutable`, `coverage_from_pattern`, `check_exhaustiveness`)
- Test: `boot/tests/suites/checker_suite.tw`, `boot/tests/suites/checker_coverage_suite.tw`

**Interfaces:**
- Consumes: `PatternKind.Tuple(Vector<Pattern>)` (Task 1).
- Uses: `zonk`, `resolve_named_type_id(env, name)`, `unify`, `check_pattern` (recursion), `Named` `MonoType` with `type_id` + `args`.
- Produces: element-scoped bindings visible to arm bodies; a non-exhaustive diagnostic when refutable tuple arms lack a catch-all.

- [ ] **Step 1: Write the failing tests**

In `checker_suite.tw`: a well-typed `case (1, 2) { (a, b) => a + b }` type-checks clean; a mismatched `case (1, 2) { (a, b, c) => 0, _ => 0 }` (arity 3 vs Tuple2) reports a diagnostic; binding types flow (`case (1, "s") { (a, b) => b }` gives `b: String`).

In `checker_coverage_suite.tw`: `case pair { (a, b) => 0 }` (irrefutable, no other arm) is exhaustive (no diagnostic); `case pair { (0, y) => y }` (refutable, no `_`) reports non-exhaustive; `case pair { (a, b) => 0, _ => 1 }` reports the `_` arm unreachable.

- [ ] **Step 2: Run to verify they fail**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — the stub emits "typing not yet implemented" / exhaustiveness wrong.

- [ ] **Step 3: Implement `check_pattern` tuple case**

Replace the stub arm in `check_pattern` with:

```tw
.Tuple(subs) => {
  arity := subs.len()
  tuple_tid := case resolve_named_type_id(ctx.env, "Tuple${arity}") {
    .Some(tid) => tid,
    .None => return .{ ctx, diags: diags.append(.Error(.ParseError(.{ span: pat.span, message: "unknown tuple arity", help_lines: [] }))) },
  }
  elem_metas := collect _ in 0..arity { ctx.fresh_meta() }
  tuple_ty := MonoType.Named(tuple_tid, elem_metas)
  u := ctx.unify(tuple_ty, expected, pat.span, diags)
  cur_ctx := u.ctx
  cur_diags := u.diags
  for i in 0..arity {
    pr := check_pattern(subs[i], elem_metas[i], cur_ctx, cur_diags)
    cur_ctx = pr.ctx
    cur_diags = pr.diags
  }
  .{ ctx: cur_ctx, diags: cur_diags }
},
```

(Confirm the exact spellings of `fresh_meta`/`resolve_named_type_id` against `checker.tw`; if `resolve_named_type_id` is not in scope there, use the same lookup `synth_named_record` uses to reach `Tuple${n}`. Unifying against `expected` yields both the arity/shape mismatch diagnostic and the element metavars.)

- [ ] **Step 4: Implement refutability + coverage + exhaustiveness**

Replace the Task 1 trivial arms:

- `pattern_is_irrefutable`:
  ```tw
  .Tuple(subs) => subs.all(pattern_is_irrefutable),
  ```
- `coverage_from_pattern`:
  ```tw
  .Tuple(subs) => if subs.all(pattern_is_irrefutable) {
    .Some(.CatchAll(pat.span))
  } else {
    .None
  },
  ```
- `check_exhaustiveness`: change the early-out loop from Wildcard/Ident-only to any irrefutable pattern:
  ```tw
  for arm in arms {
    if pattern_is_irrefutable(arm.pattern) {
      return diags
    }
  }
  ```
  Leave the tuple arm in the `covered` collect as `continue` (tuple arms contribute no variant-name coverage). For a tuple scrutinee `get_variant_specs` returns `.None`, so a non-covered refutable-only tuple match falls through to "no diagnostic" today — add: when `get_variant_specs` is `.None` **and** the scrutinee is a tuple record **and** no arm is irrefutable, emit a non-exhaustive diagnostic. Reuse the existing missing-arm diagnostic shape or a simple `.ParseError`-style "non-exhaustive tuple match; add a `_` arm" on `s`.

- [ ] **Step 5: Run to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS.

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/checker.tw
target/twk lint boot/main.tw
git add boot/compiler/checker.tw boot/tests
git commit -m "feat(tuple): type-check and exhaustiveness for case-arm tuple patterns"
```

---

## Task 3: Boot — lower + codegen (feature works end-to-end)

Replaces the lowering stub and adds the record-layout branch to the two emit functions, so tuple patterns actually bind and match at runtime.

**Files:**
- Modify: `boot/compiler/lower_core/patterns.tw` (`lower_pattern`), `boot/compiler/codegen/emit.tw` (`emit_pattern_condition`, `emit_pattern_bindings`), `boot/compiler/codegen/emit/match.tw` (`is_br_table_eligible`)
- Test: `boot/tests/suites/codegen_integration_suite.tw`

**Interfaces:**
- Consumes: `PatternKind.Tuple` (Task 1), tuple typing (Task 2).
- Uses: `type_id_from_mono`, `variant_field_types`/type args for element types, `CorePattern.Variant`, `record_layout_of_ctx(mono, ctx).sym`, `find_field_index`, `env.lookup_type_def`.
- Produces: correct runtime binding/matching for tuple patterns.

- [ ] **Step 1: Write the failing end-to-end tests**

In `codegen_integration_suite.tw`, add programs compiled + run (follow the suite's existing compile-and-assert pattern):
- irrefutable bind: `case (3, 4) { (a, b) => a * 10 + b }` → `34`.
- nested variant-holding-tuple: `case find(m, k) { .Some((key, val)) => val, .None => -1 }`.
- refutable positions with fallthrough: `case (0, 7) { (0, y) => y, (x, _) => x }` → `7`; and `(5, 7)` → `5`.
- nested sub-tuple: `case ((1, 2), 3) { ((a, b), c) => a + b + c }` → `6`.

- [ ] **Step 2: Run to verify they fail**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — lowering stub maps tuple patterns to `.Wildcard`, so bindings are missing / results wrong.

- [ ] **Step 3: Implement `lower_pattern` tuple case**

Replace the stub in `lower_core/patterns.tw`:

```tw
.Tuple(sub_pats) => {
  tid := case type_id_from_mono(scrut_ty, ctx.env) {
    .Some(id) => id,
    .None => return .{ pattern: .Wildcard, ctx },
  }
  field_tys := tuple_field_types(scrut_ty, ctx.env)   // element types from the Named args
  lowered_pats: Vector<CorePattern> = []
  cur_ctx := ctx
  for i in 0..sub_pats.len() {
    sub_ty := if i < field_tys.len() { field_tys[i] } else { .Void }
    pr := lower_pattern(cur_ctx, sub_pats[i], sub_ty)
    lowered_pats = .append(pr.pattern)
    cur_ctx = pr.ctx
  }
  .{ pattern: .Variant(tid, .{ id: 0 }, lowered_pats), ctx: cur_ctx }
},
```

For `tuple_field_types`: a tuple is `MonoType.Named(tuple_tid, args)` where `args` are exactly the element types, so this is just the zonked `args` of the scrutinee's `Named` type. Reuse `variant_field_types` if it already returns the record's field types for a `Named` record; otherwise read the `Named` args directly.

- [ ] **Step 4: Add the record-tid branch to the emit functions**

In `emit.tw`, both `emit_pattern_condition` and `emit_pattern_bindings` currently do, in their `.Variant(tid, vid, fields)` arm, `if !can_match_variant_pattern(scrutinee_mono, ctx.env) { <no-op> } else { <sum-layout path> }`. `can_match_variant_pattern` returns `false` for a record tid, so add a record branch **before** that guard. For the condition:

```tw
.Variant(tid, vid, fields) => if is_record_scrutinee(scrutinee_mono, ctx.env) {
  rec := record_layout_of_ctx(scrutinee_mono, ctx)
  inner_checks: Vector<Vector<Instr>> = []
  for fp, i in fields {
    if !pattern_is_trivial(fp) {
      field_instrs := scrutinee_instrs.append(.StructGet(rec.sym, i))
      field_mono := tuple_field_mono(scrutinee_mono, i, ctx.env)
      inner_checks = .append(emit_pattern_condition(fp, field_instrs, field_mono, ctx, []))
    }
  }
  if inner_checks.len() == 0 {
    buf.append(.I32Const(1))
  } else {
    buf.concat(combine_and_checks(inner_checks))
  }
} else if !can_match_variant_pattern(scrutinee_mono, ctx.env) {
  buf.append(.I32Const(0))
} else {
  emit_variant_pattern_condition(tid, vid, fields, scrutinee_instrs, scrutinee_mono, ctx, buf)
},
```

For the bindings, mirror the sum-layout branch but use the record layout (field `i` at `StructGet(rec.sym, i)`, no tag offset):

```tw
.Variant(tid, vid, fields) => if is_record_scrutinee(scrutinee_mono, ctx.env) {
  rec := record_layout_of_ctx(scrutinee_mono, ctx)
  out: Vector<Instr> = []
  for fp, i in fields {
    field_instrs := scrutinee_instrs.append(.StructGet(rec.sym, i))
    field_mono := tuple_field_mono(scrutinee_mono, i, ctx.env)
    out = .concat(emit_pattern_bindings(fp, field_instrs, field_mono, ctx))
  }
  out
} else if !can_match_variant_pattern(scrutinee_mono, ctx.env) {
  []
} else {
  <existing sum-layout binding body>
},
```

Add helpers in `emit.tw`: `is_record_scrutinee(mono, env)` → `case mono { .Named(tid, _) => case env.lookup_type_def(tid) { .Some(.Record(_, _, _)) => true, _ => false }, _ => false }`; `tuple_field_mono(mono, i, env)` → the i-th zonked `Named` arg. (Field index == positional index for tuple records, whose fields are declared `_0.._3` in order.)

- [ ] **Step 5: Make record tids br-table-ineligible**

In `emit/match.tw` `is_br_table_eligible`, return `false` when the scrutinee is a record (no tag to switch on) so matching always uses `emit_arm_chain`. Add the guard near the top:

```tw
if is_record_scrutinee(scrutinee_mono, ctx.env) {
  return false
}
```

- [ ] **Step 6: Run to verify they pass; full suite green**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS on the new integration tests and `Ran N tests: N passed` overall.

- [ ] **Step 7: Format, lint, commit**

```bash
target/twk fmt boot/compiler/lower_core/patterns.tw boot/compiler/codegen/emit.tw boot/compiler/codegen/emit/match.tw
target/twk lint boot/main.tw
git add boot/compiler boot/tests
git commit -m "feat(tuple): lower and codegen case-arm tuple patterns via record-layout matching"
```

---

## Task 4: Stage0 — surface node + parser + pretty

Mirrors Task 1 in Rust. The Rust compiler's exhaustiveness checking flags every `Pattern` match needing a new arm.

**Files:**
- Modify: `src/syntax/ast.rs` (`Pattern`), `src/syntax/parser.rs` (`parse_pattern`), `src/syntax/pretty.rs`, and any `Pattern`-matching site the compiler flags (`src/query/api.rs`, `src/module/mod.rs`, etc.)
- Test: inline `#[test]` in `src/syntax/mod.rs` (mirror the existing tuple-literal desugar tests)

**Interfaces:**
- Produces: `Pattern::Tuple(Vec<Pattern>, Span)` — consumed by Tasks 5–6.

- [ ] **Step 1: Write the failing Rust test**

In `src/syntax/mod.rs` tests, parse `x := case p { (a, b) => a, _ => 0 }` and assert the first arm's pattern is `Pattern::Tuple` with two elements; assert `(a)` (no comma) and a 5-element tuple pattern each yield a parse/arity error.

```rust
#[test]
fn tuple_pattern_parses_to_pattern_tuple() {
    let m = parse_ok("x := case p { (a, b) => a, _ => 0 }");
    let pat = first_case_arm_pattern(&m);
    match pat {
        Pattern::Tuple(subs, _) => assert_eq!(subs.len(), 2),
        other => panic!("expected Pattern::Tuple, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --release syntax::tests::tuple_pattern`
Expected: FAIL — `Pattern::Tuple` does not exist.

- [ ] **Step 3: Add the AST variant**

In `src/syntax/ast.rs`, add to `Pattern`:

```rust
/// Tuple pattern: (a, b), (a, b, c), (a, b, c, d) — arity 2–4.
Tuple(Vec<Pattern>, Span),
```

- [ ] **Step 4: Parse the `(`-case in `parse_pattern`**

In `src/syntax/parser.rs` `parse_pattern`, add a `Some(TokenKind::LParen)` arm that consumes `(`, parses a comma-separated `parse_pattern()` list until `)`, enforces arity 2–4 (emit a parse error mirroring `src/syntax/parser.rs:1483`'s tuple-literal arity error otherwise), and returns `Pattern::Tuple(subs, span)`.

- [ ] **Step 5: Thread `Pattern::Tuple` through flagged matches**

Build; add arms wherever `rustc` reports a non-exhaustive `match` on `Pattern`: `src/syntax/pretty.rs` (print `(a, b)`), and any `src/query/api.rs` / `src/module/mod.rs` sites (recurse into sub-patterns like the `Pattern::Variant { fields, .. }` arm). Leave `src/types/patterns.rs` and `src/ir/lower.rs` arms as minimal stubs (typing error / lower to `CorePattern::Wildcard`) — replaced in Tasks 5–6.

- [ ] **Step 6: Run to verify it passes**

Run: `cargo test --release syntax::tests::tuple_pattern`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/syntax src/query src/module
git commit -m "feat(tuple): stage0 parses (a, b) case-arm patterns to Pattern::Tuple"
```

---

## Task 5: Stage0 — `PatternChecker` typing + exhaustiveness

Mirrors Task 2.

**Files:**
- Modify: `src/types/patterns.rs` (and `src/types/check.rs` if pattern dispatch lives there)
- Test: inline `#[test]` in `src/types/patterns.rs`

**Interfaces:**
- Consumes: `Pattern::Tuple` (Task 4).
- Uses: the `TupleN` `TypeId`s (13/14/15 in `src/types/env.rs`), `MonoType::Named { type_id, args }`, unification, `PatternChecker::check` recursion.

- [ ] **Step 1: Write failing tests**

In `src/types/patterns.rs` tests: a `(a, b)` pattern against `Tuple2<Int, Int>` binds `a: Int`, `b: Int`; an arity mismatch errors; `check_exhaustiveness` treats an all-irrefutable tuple arm as exhaustive and a refutable-only tuple match as non-exhaustive (mirror the existing exhaustiveness tests at `src/types/patterns.rs`).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --release types::patterns`
Expected: FAIL.

- [ ] **Step 3: Implement tuple typing**

In the pattern type-checker, add a `Pattern::Tuple(subs, span)` arm: resolve the `TupleN` `TypeId` by `subs.len()`, build `MonoType::Named { type_id, args: fresh_vars }`, unify with the expected type, then recurse the checker on each `subs[i]` against `args[i]`. Bindings accumulate into the same environment the other arms use.

- [ ] **Step 4: Implement exhaustiveness/refutability**

Add the tuple cases to stage0's refutability + exhaustiveness helpers, matching boot's rule: a tuple is irrefutable iff all elements are; the "any irrefutable arm ⇒ exhaustive" early-out covers a fully-irrefutable tuple arm; a refutable-only tuple `case` with no wildcard is non-exhaustive.

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test --release types::patterns`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/types
git commit -m "feat(tuple): stage0 type-checks and exhaustiveness-checks tuple patterns"
```

---

## Task 6: Stage0 — lower + codegen (parity, `cargo test` green)

Mirrors Task 3.

**Files:**
- Modify: `src/ir/lower.rs` (`lower_pattern`), `src/codegen/emit.rs` (`emit_pattern_condition`, `emit_pattern_bindings`)
- Test: a stage0 end-to-end compile/run test (follow existing codegen test conventions)

**Interfaces:**
- Consumes: `Pattern::Tuple` (Task 4), tuple typing (Task 5).
- Uses: `CorePattern::Variant { type_id, variant, fields }` with `variant = VariantId(0)`; `record_struct_sym(tuple_tid, …)` + `Instr::StructGet(sym, i)` (see `emit_record_get`, `src/codegen/emit.rs:3943`); `emit_match_arm_chain` for fallthrough.

- [ ] **Step 1: Write the failing end-to-end tests**

Mirror Task 3's four scenarios (irrefutable bind, nested `.Some((k, v))`, refutable fallthrough, nested sub-tuple) as stage0 compile-and-run tests producing the same expected values (`34`, found value, `7`/`5`, `6`).

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --release <tuple codegen test filter>`
Expected: FAIL — lowering stub maps to `Wildcard`.

- [ ] **Step 3: Implement `lower_pattern` tuple case**

In `src/ir/lower.rs` `lower_pattern`, add a `Pattern::Tuple(subs, _)` arm: resolve `tuple_tid` from the scrutinee `MonoType::Named { type_id, .. }` (or by arity), recurse `lower_pattern` on each sub-pattern against the corresponding `args[i]` element type, and return `CorePattern::Variant { type_id: tuple_tid, variant: VariantId(0), fields }`. Do **not** route through `get_variant_index` (records have no variant index).

- [ ] **Step 4: Add the record-tid branch to stage0 emit**

In `src/codegen/emit.rs`, `emit_pattern_condition` (~1709) and `emit_pattern_bindings` (~2046) branch on the scrutinee representation (erased `T_VARIANT` vs typed sum). Add a record/tuple-tid branch that precedes both: detect a record `type_id` (via the type env's `TypeDef::Record`), then project field `i` with `record_struct_sym(tuple_tid, base_mono, …)` + `Instr::StructGet(sym, i as u32)` — the same path `emit_record_get` (~3943) uses — recursing for conditions (AND of non-trivial sub-checks, `I32Const(1)` if all trivial) and for bindings. Ensure the arm chain (`emit_match_arm_chain`) is used (no tag-switch is emitted for records).

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test --release <tuple codegen test filter>`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/ir src/codegen
git commit -m "feat(tuple): stage0 lowers and codegens tuple patterns via record layout"
```

---

## Task 7: Fixed-point gate + docs + grammar

Confirms both compilers agree, updates the spec/grammar, and refreshes tree-sitter.

**Files:**
- Modify: `docs/spec.md` (§13.7), `docs/grammar.ebnf` (~361), `tree-sitter-twinkle/grammar.js` (~497) + regenerated `src/` + `.wasm`
- Verify: `make stage2`, `make boot-test`, targeted `cargo test --release`

- [ ] **Step 1: Self-host fixed point**

Run: `make stage2`
Expected: reaches a fixed point (`stage3 == stage4`). Because boot source is not migrated, this should pass without source changes — it confirms the two compilers' pattern handling agrees on the existing corpus.

- [ ] **Step 2: Update the spec**

In `docs/spec.md` §13.7, replace the "There is no destructuring binding in this release." sentence (line ~1126) with a short note that tuple patterns are supported in `case` arms — e.g. `case p { (x, y) => … }`, arity 2–4, full nesting — while `(a, b) :=` binding, `for` patterns, and parameter patterns are not yet available. Add one worked `case`-arm example.

- [ ] **Step 3: Update the EBNF grammar**

In `docs/grammar.ebnf`, add a `TuplePattern` alternative to the `Pattern` rule (~361): `TuplePattern = "(" Pattern "," Pattern { "," Pattern } ")" ;` with a note that arity is 2–4 and `(p)` stays non-tuple.

- [ ] **Step 4: Update tree-sitter grammar**

In `tree-sitter-twinkle/grammar.js`, add a `tuple_pattern` rule (a parenthesized comma-separated `_pattern` list requiring a top-level comma, mirroring how `tuple_literal` was added) and include it in the `_pattern` choice (~497). Then:

```bash
cd tree-sitter-twinkle
npx tree-sitter generate
npx tree-sitter build --wasm
```

Bump `package.json` + `tree-sitter.json` to the next patch version (the current published grammar is one patch ahead of the last tuple change — follow the repo's version-bump precedent).

- [ ] **Step 5: Hand tree-sitter tests to the human**

Do **not** run `tree-sitter test`. Ask the human to run it and confirm before committing the regenerated artifacts.

- [ ] **Step 6: Final gate**

Run: `make boot-test` (green) and targeted `cargo test --release` (green). Confirm `target/twk fmt` is idempotent and `target/twk lint boot/main.tw` is clean on edited files.

- [ ] **Step 7: Commit + close out the plan**

```bash
git add docs/spec.md docs/grammar.ebnf tree-sitter-twinkle
git commit -m "docs(tuple): document case-arm tuple patterns; grammar + tree-sitter rules"
```

Then move both `docs/plans/tuple-destructuring.md` and `docs/plans/tuple-destructuring-implementation.md` to `docs/plans/archive/`, and remove any plan-index row per the plan lifecycle. Note the still-deferred `(a, b) :=` / `for` / parameter destructuring as the next fast-follow so it is not lost.

---

## Self-Review

**Spec coverage:** Every design section maps to a task — parser+AST (Tasks 1/4), type-checking (2/5), exhaustiveness+reachability (2/5), lowering (3/6), codegen record-layout gating (3/6), formatter (1), grammar/docs (7), both compilers (boot 1–3, stage0 4–6), fixed point (7). Full nesting and the conservative exhaustiveness rule are carried in Tasks 2/3 and 5/6.

**Placeholder scan:** Code steps carry real snippets; the two spots that say "confirm the exact spelling against the file" (`fresh_meta`/`resolve_named_type_id`; the sum-layout binding body to preserve) are verification instructions, not deferred design — the surrounding arm is fully specified.

**Type consistency:** `PatternKind.Tuple(Vector<Pattern>)` / `Pattern::Tuple(Vec<Pattern>, Span)` used consistently; lowering target `CorePattern.Variant(tid, {id:0}, subs)` / `CorePattern::Variant { type_id, variant: VariantId(0), fields }` consistent across Tasks 3/6; helper names `is_record_scrutinee`/`tuple_field_mono`/`tuple_field_types` (boot) and `record_struct_sym`/`emit_record_get` (stage0) match their introduction points.
