# Tuple Binding-Destructuring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend tuple destructuring from `case` arms to binding positions — `(a, b) := expr`, `(a, b): (Int, Int) = expr`, `for (a, b) in xs`, `for (a, b), i in xs`, and the `collect` equivalents — across both the boot and stage0 compilers.

**Architecture:** boot carries a first-class AST node (`LetPattern`) plus optional `element_pattern` fields on `ForStmt`/`CollectExpr`, type-checks with the existing `check_pattern`, and lowers by desugaring to a temp binding + positional `._N` field reads. stage0 is already pattern-generic in its AST, so it needs a let-LHS parser fix plus filling in checker/lowering placeholders. Both compilers bottom out on the ordinary tuple field-read path (`record_get_expr` / `RecordGet`), which already codegens correctly.

**Tech Stack:** Twinkle (`.tw`, boot compiler self-hosted), Rust (stage0 in `src/`), tree-sitter grammar (JS), Deno-based Wasm-GC runtime for run tests.

## Global Constraints

- **Design source of truth:** `docs/plans/archive/tuple-binding-destructuring-design.md`. This plan implements it verbatim; if a detail conflicts, the design wins.
- **Two-compiler parity is the hard gate:** `make stage2` (self-host fixed point) must reach a fixed point. Both compilers must implement the checker + lowering for the feature to even reach it.
- **In scope:** irrefutable patterns only — identifiers, `_` wildcards, nested tuples `((a, b), c)`, arity 2–4 at each level. RHS evaluated exactly once.
- **Out of scope (must be a clean compile error, not silent):** refutable sub-patterns (`.Some(x)`, qualified variants, literals) in a binder; tuple-pattern *rebind* `(a, b) = e`; function-parameter tuple patterns; arity >4, 1-tuples, `()` unit.
- **Naming rule (memory):** name new files/functions by defect/intent, never by phase/task number.
- **Rebinding style (memory):** use `buf = buf.append(...)`, never `buf2, buf3, ...`.
- **After editing any `.tw` file:** run `target/twk fmt <file>` then `target/twk lint <entry>`. The formatter is idempotent.
- **Do NOT run full `cargo test`** (too slow) — use targeted `cargo test --release <filter>`.
- **Do NOT run `tree-sitter test`** — that is a human step (project rule).
- **Heavy verification runs one at a time** (memory): never run full suite / `make stage2` / `make bundle-cli` concurrently or backgrounded.
- **Commit style:** short imperative subject; body explains what/why/how, no line/count metrics. Only add `Co-Authored-By` when actually correct.

---

## File Structure

**boot (primary implementation):**
- `boot/compiler/ast.tw` — new `LetPattern(LetPatternStmt)` Stmt variant + `LetPatternStmt` type; `element_pattern: Pattern?` field on `ForStmt` and `CollectExpr`.
- `boot/compiler/parser.tw` — disambiguation in `parse_block` tail-fallback + `parse_top_level_stmt`; tuple element binder in `parse_for_stmt` / `parse_collect_expr`.
- `boot/compiler/checker.tw` — `check_let_pattern`; extend `bind_iterable_vars` for `element_pattern`; wire `check_stmt` / `check_top_level_stmt`.
- `boot/compiler/lower_core/statements.tw` — `.LetPattern` desugar arm.
- `boot/compiler/lower_core/iteration.tw` — `setup_indexed_iter` element-pattern destructure prepend.
- `boot/compiler/lower_core.tw` — module-level `LetPattern` global collection.
- `boot/compiler/fmt/printer.tw` — `format_stmt` / `stmt_span` / `format_for` / `format_collect` for the new surface.
- Wiring: `boot/compiler/resolver.tw`, `boot/compiler/lint.tw`, `boot/compiler/query/hover.tw`, `boot/compiler/unused_imports.tw`, `boot/compiler/module_compiler.tw`.

**stage0 (bootstrap reference):**
- `src/syntax/parser.rs` — `is_let_binding` `(`-led lookahead; remove/gate `ColonEq` infix BP.
- `src/types/check.rs` — real `Pattern::Tuple` handling in `check_let_stmt` + `check_for_stmt` (+ collect path via `synth_collect`).
- `src/ir/lower.rs` — `Pattern::Tuple` lowering in `Stmt::Let` + `Stmt::For` (+ collect).

**Grammar & docs:**
- `docs/grammar.ebnf`, `docs/spec.md`, `tree-sitter-twinkle/grammar.js` (+ regenerated `src/`, `.wasm`).

**Tests:**
- `boot/tests/suites/checker_suite.tw`, `boot/tests/suites/codegen_integration_suite.tw`, `boot/tests/suites/fmt_suite.tw` (+ `fmt_cases/` fixtures).
- `tests/tuple_pattern_run_test.rs` (stage0 end-to-end run tests).

---

## Task 1: boot AST nodes + compile-restoring wiring

Add the new AST shapes and make the boot compiler compile again. Adding a `Stmt` variant breaks exhaustive matches (`format_stmt`, `stmt_span`); adding a record field breaks every `ForStmt`/`CollectExpr` literal. This task restores compilation with safe stubs; real behavior lands in later tasks.

**Files:**
- Modify: `boot/compiler/ast.tw:110-141` (Stmt, ForStmt), `boot/compiler/ast.tw:180-188` (CollectExpr)
- Modify: `boot/compiler/parser.tw:2899` (ForStmt literal), `boot/compiler/parser.tw:2440` (CollectExpr literal)
- Modify: `boot/compiler/fmt/printer.tw:1117` (format_stmt), `boot/compiler/fmt/printer.tw:344` (stmt_span)

**Interfaces:**
- Produces: `Stmt.LetPattern(LetPatternStmt)`; `LetPatternStmt = .{ pattern: Pattern, ty: TypeExpr?, value: Expr, span: Span }`; `ForStmt.element_pattern: Pattern?`; `CollectExpr.element_pattern: Pattern?`.

- [ ] **Step 1: Add the AST shapes**

In `boot/compiler/ast.tw`, add to the `Stmt` sum (after `Let(LetStmt),`):

```tw
  LetPattern(LetPatternStmt),
```

Add the record type near `LetStmt`:

```tw
pub type LetPatternStmt = .{
  pattern: Pattern,
  ty: TypeExpr?,
  value: Expr,
  span: Span,
}
```

Add `element_pattern: Pattern?` to `ForStmt` (after `pattern_span: Span?`) and to `CollectExpr` (after `pattern_span: Span?`).

- [ ] **Step 2: Fix the two ForStmt/CollectExpr construction sites**

In `parser.tw`, the `ForStmt.{ ... }` literal at ~2900 and the `CollectExpr.{ ... }` literal at ~2440 must include `element_pattern: .None`. (Later tasks set it to `.Some(...)`.)

- [ ] **Step 3: Restore exhaustive-match compilation with stubs**

`format_stmt` (`printer.tw:1117`): add `.LetPattern(s) => format_let_pattern(s, tm),` and define a stub `fn format_let_pattern(s: LetPatternStmt, tm: TriviaMap) Doc { doc.text("/* let-pattern */") }` (real body in Task 7).

`stmt_span` (`printer.tw:344`): add `.LetPattern(s) => s.span,`.

- [ ] **Step 4: Verify boot source type-checks**

Run: `cargo run --release -- build boot/main.tw -o /tmp/boot-main.wasm`
Expected: builds with no errors (stage0 compiles boot source). The new variant is unreachable at runtime so far — this only proves the shapes and stubs compile.

- [ ] **Step 5: Run boot suite to confirm no regression**

Run: `make boot-test`
Expected: PASS (no test exercises the new node yet).

- [ ] **Step 6: fmt + lint the edited files**

Run: `target/twk fmt boot/compiler/ast.tw boot/compiler/parser.tw boot/compiler/fmt/printer.tw && target/twk lint boot/main.tw`
Expected: no changes on re-run; no new lint findings.

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/ast.tw boot/compiler/parser.tw boot/compiler/fmt/printer.tw
git commit -m "feat(tuple): add LetPattern AST node and element_pattern fields"
```

---

## Task 2: boot parser disambiguation (the fiddly part)

Recognize `(a, b) := e` and `(a, b): T = e` as a `LetPattern` at statement start — inside `parse_block`'s tail-producing fallback (NOT `is_stmt_leader`) and in `parse_top_level_stmt`. A bare `=` after a tuple LHS is a rebind attempt → clean diagnostic.

**Files:**
- Modify: `boot/compiler/parser.tw:2990-3015` (parse_block generic-expression fallback)
- Modify: `boot/compiler/parser.tw:3027+` (parse_top_level_stmt)
- Test: `boot/tests/suites/parser_suite.tw` (or checker_suite if no parser suite exercises blocks — see Step 1)

**Interfaces:**
- Consumes: `Stmt.LetPattern` (Task 1), `Expr.Tuple` (existing), `parse_type_expr`, `parse_expr_bp`.
- Produces: `fn tuple_expr_to_pattern(e: Expr) Pattern?` (returns `.Some` only when every element is `Ident`/`_`/nested tuple — bindable), used by both call sites.

- [ ] **Step 1: Write the failing test**

Add to the boot parser test suite (find it: `grep -rln "fn suite" boot/tests/suites/parser_suite.tw`; if none, add these as parse-then-check assertions in `checker_suite.tw` using `check_src`). Test that a tuple-let parses and binds:

```tw
// tuple let binds both names (block position)
src := "fn f() Int {\n  (a, b) := (3, 4)\n  a * 10 + b\n}"
r := try check_ok(src)
try assert.equal(try type_at(r, src, "a * 10 + b"), "Int")
.Ok({})
```

And a regression guard that a tuple *tail* expression still works:

```tw
// tuple tail expr still parses as the block tail (not a statement)
src := "fn divmod(a: Int, b: Int) (Int, Int) { (a / b, a % b) }"
_ := try check_ok(src)
.Ok({})
```

- [ ] **Step 2: Run to verify it fails**

Run: `make boot-test` (or the targeted suite)
Expected: FAIL — the tuple-let currently parses `(a, b)` as a tuple expression, `:=` is not infix so parsing stops, producing a malformed block / checker error.

- [ ] **Step 3: Add the `tuple_expr_to_pattern` helper**

In `parser.tw`, near the other pattern helpers:

```tw
fn tuple_expr_to_pattern(e: Expr) Pattern? {
  case e.kind {
    .Ident(name) => .Some(Pattern.{ kind: .Ident(name), span: e.span }),
    .Tuple(elems) => {
      subs: Vector<Pattern> = []
      for el in elems {
        p := try tuple_expr_to_pattern(el)
        subs = .append(p)
      }
      .Some(Pattern.{ kind: .Tuple(subs), span: e.span })
    },
    // `_` lexes as an Ident named "_"; map it to Wildcard.
    _ => .None,
  }
}
```

Handle `_`: if `.Ident("_")`, return `.Some(Pattern.{ kind: .Wildcard, span: e.span })`. (Confirm how `_` is lexed with `grep -n "Wildcard\|\"_\"" boot/compiler/parser.tw`; mirror the case-arm pattern parser at `parse_pattern`.)

- [ ] **Step 4: Wire the `parse_block` fallback**

In `parse_block` (`parser.tw:2990`), after `parsed_expr := parse_expr_bp(c, 0)` and before the `.Semi`/`.RBrace`/tail branches, insert: if `expr.kind` is `.Tuple`, peek `next_c.kind()`:
- `.ColonEq` → build `LetPattern` (infer): advance past `:=`, parse the value with `parse_expr_bp(next_c.advance(), 0)`, `pattern := tuple_expr_to_pattern(expr)`; on `.None` emit a diagnostic "tuple binding pattern must contain only names, `_`, or nested tuples". Append `.LetPattern(LetPatternStmt.{ pattern, ty: .None, value, span })`, set `c`, `continue`.
- `.Colon` → annotated form: parse the type (`parse_type_expr`), expect `=`, parse value; `ty: .Some(...)`.
- `.Eq` → rebind attempt: emit `diag.error(next_c.span(), "tuple-pattern rebind not supported; use ':=' for a new binding or 'case' to match")`, recover by consuming to end of statement, `continue`.
- otherwise → leave `expr` as the tail/expr-stmt exactly as today (this preserves `divmod`).

- [ ] **Step 5: Wire `parse_top_level_stmt`**

`parse_top_level_stmt` (`parser.tw:3027`) calls `parse_stmt` directly, which for a `(`-led statement takes the `_ =>` branch → `parse_expr_bp` → tuple expr, dropping the binding. Add the same peek-and-reinterpret: before calling `parse_stmt`, if `c.kind() == .LParen`, parse the expression, peek for `:=`/`:`/`=`, and build `LetPattern` (or the rebind diagnostic) exactly as in Step 4. Factor the shared logic into a helper `fn try_parse_tuple_let(c: Cursor) StmtParse?` used by both sites to stay DRY.

- [ ] **Step 6: Run tests to verify they pass**

Run: `make boot-test`
Expected: PASS — both the tuple-let and the `divmod` tail-expr tests are green.

- [ ] **Step 7: Add module-level + rebind-rejection regression tests**

```tw
// module-level tuple let
src := "fn dm(a: Int, b: Int) (Int, Int) { (a / b, a % b) }\n(q, r) := dm(17, 5)\nfn use_it() Int { q * 100 + r }"
_ := try check_ok(src)   // q, r must be in module scope
.Ok({})
```

```tw
// rebind is a clean error, not an ICE
src := "fn f() Int {\n  a := 1\n  b := 2\n  (a, b) = (3, 4)\n  a\n}"
errs := try check_errs(src)
try assert.str_contains(errs.join("\n"), "rebind")
.Ok({})
```

(The module-level binding won't fully type-check until Task 5 wires `check_top_level_stmt` + globals; if this test fails on *checker* grounds rather than parse grounds, mark it `// pending Task 5` and move the assertion there. The parse itself must succeed now.)

- [ ] **Step 8: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/parser.tw && target/twk lint boot/main.tw
git add boot/compiler/parser.tw boot/tests/suites/*.tw
git commit -m "feat(tuple): parse tuple-let bindings without regressing tuple tail-expressions"
```

---

## Task 3: boot checker for `LetPattern`

Type-check the pattern binding: resolve the RHS type (or check against the annotation), enforce irrefutability, then bind the destructured names into the **current** frame.

**Files:**
- Modify: `boot/compiler/checker.tw:5295-5317` (check_stmt), `boot/compiler/checker.tw:5319-5347` (check_top_level_stmt — see Task 5 for module globals), add `check_let_pattern`
- Test: `boot/tests/suites/checker_suite.tw`

**Interfaces:**
- Consumes: `check_pattern(pat, expected, ctx, diags) CheckOut` (`checker.tw:3573`), `pattern_is_irrefutable(pat) Bool` (`checker.tw:3735`), `ctx.synth`, `ctx.check_expr`, `resolve_type`.
- Produces: `fn check_let_pattern(lp: LetPatternStmt, ctx: InferCtx, diags: Vector<DiagKind>) CheckOut`.

- [ ] **Step 1: Write the failing tests** (in `checker_suite.tw`)

```tw
// arity mismatch surfaces against RHS tuple type
src := "fn f() Int {\n  (a, b, c) := (1, 2)\n  a\n}"
errs := try check_errs(src)
try assert.str_contains(errs.join("\n"), "Tuple")   // arity/type error mentions tuple
.Ok({})
```

```tw
// refutable sub-pattern rejected
src := "fn f() Int {\n  (a, .Some(x)) := (1, .Some(2))\n  a\n}"
errs := try check_errs(src)
try assert.str_contains(errs.join("\n"), "refutable")
.Ok({})
```

```tw
// annotated form: element type flows, wildcard binds nothing
src := "fn f() Int {\n  (a, _): (Int, Int) = (10, 20)\n  a\n}"
r := try check_ok(src)
try assert.equal(try type_at(r, src, "a\n}"), "Int")
.Ok({})
```

- [ ] **Step 2: Run to verify failure**

Run: `make boot-test`
Expected: FAIL — `check_stmt` currently hits the `_ => .{ ctx, diags }` fallthrough for `.LetPattern`, so no bindings and no diagnostics.

- [ ] **Step 3: Implement `check_let_pattern`**

```tw
fn check_let_pattern(lp: LetPatternStmt, ctx: InferCtx, diags: Vector<DiagKind>) CheckOut {
  // Irrefutability first: a refutable sub-pattern is a hard error.
  if !pattern_is_irrefutable(lp.pattern) {
    return .{
      ctx,
      diags: diags.append(
        .Error(.ParseError(.{
          span: lp.pattern.span,
          message: "refutable pattern not allowed in a binding; use `case`",
          help_lines: [],
        })),
      ),
    }
  }

  // Resolve RHS type: annotated → check mode; else synth.
  rhs := case lp.ty {
    .Some(ty_expr) => {
      resolved := ctx.resolve_type_expr(ty_expr)   // confirm the exact resolver entry point
      cr := resolved.ctx.check_expr(lp.value, resolved.ty, diags)
      .{ ty: resolved.ty, ctx: cr.ctx, diags: cr.diags }
    },
    .None => {
      sr := ctx.synth(lp.value, diags)
      .{ ty: sr.ty, ctx: sr.ctx, diags: sr.diags }
    },
  }

  // Bind directly into the current frame — NO push_scope/pop_scope
  // (mirror check_let at checker.tw:5349, not check_case_arm at :3446).
  check_pattern(lp.pattern, rhs.ty, rhs.ctx, rhs.diags)
}
```

Confirm the annotation-resolver entry point by reading how `check_let` handles `ls.ty` (`checker.tw:5349+`) and reuse the identical call — do not invent `resolve_type_expr` if the real name differs.

- [ ] **Step 4: Wire `check_stmt`**

Add to the `check_stmt` case (`checker.tw:5297`): `.LetPattern(lp) => check_let_pattern(lp, ctx, diags),`.

- [ ] **Step 5: Run to verify pass**

Run: `make boot-test`
Expected: PASS — all three Step-1 tests green.

- [ ] **Step 6: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/checker.tw && target/twk lint boot/main.tw
git add boot/compiler/checker.tw boot/tests/suites/checker_suite.tw
git commit -m "feat(tuple): type-check LetPattern bindings with irrefutability guard"
```

---

## Task 4: boot lowering for `LetPattern`

Desugar `(a, b) := e` into a temp binding plus positional `._N` field reads, using the ordinary record-get path.

**Files:**
- Modify: `boot/compiler/lower_core/statements.tw:31-154` (lower_stmts case)
- Test: `boot/tests/suites/codegen_integration_suite.tw` (via `run_exit_code`)

**Interfaces:**
- Consumes: `record_get_expr(base, field, ty, span)` (`lower_core/helpers.tw:20`), `tuple_field_types(ty, env)` (`lower_core/types.tw:200`), `ctx.alloc_anon_local()`, `ctx.alloc_local(name)`, `LoweredStmt.LocalLet` / `.AnonLet`.
- Produces: `fn lower_let_pattern(ctx, lp, lower_expr_fn) LetPatternLowerOut` where `LetPatternLowerOut = .{ stmts: Vector<LoweredStmt>, ctx: LowerCtx }`.

- [ ] **Step 1: Write the failing run tests** (in `codegen_integration_suite.tw`, mirror the `run_exit_code` tests at ~667)

```tw
// flat destructure computes both elements
src := "fn dm(a: Int, b: Int) (Int, Int) { (a / b, a % b) }\nfn main() Int {\n  (q, r) := dm(17, 5)\n  q * 100 + r\n}\ncode := main()\nif code != 302 { error(\"bad\") }"
code := try codegen_harness.run_exit_code(src)
try assert.equal(code, 0)
.Ok({})
```

```tw
// nested destructure + wildcard
src := "fn main() Int {\n  ((a, b), _) := ((1, 2), 3)\n  a * 10 + b\n}\nc := main()\nif c != 12 { error(\"bad\") }"
code := try codegen_harness.run_exit_code(src)
try assert.equal(code, 0)
.Ok({})
```

```tw
// RHS evaluated exactly once (side-effecting counter)
src := "cnt := Cell.new(0)\nfn bump() (Int, Int) {\n  cnt = cnt.update(fn(n) { n + 1 })\n  (cnt.get(), 0)\n}\nfn main() Int {\n  (a, _) := bump()\n  a * 10 + cnt.get()\n}\nc := main()\nif c != 11 { error(\"bad\") }"
code := try codegen_harness.run_exit_code(src)
try assert.equal(code, 0)
.Ok({})
```

(Adjust the `Cell` API call to match `docs/API.md`; the point is one increment ⇒ `a == 1` and `cnt == 1` ⇒ `11`.)

- [ ] **Step 2: Run to verify failure**

Run: `make boot-test`
Expected: FAIL — `lower_stmts` hits `_ => emit_error("unsupported statement kind")` for `.LetPattern`.

- [ ] **Step 3: Implement the desugar helper**

Add a recursive helper that, given a base `CoreExpr` of tuple type and a `Pattern`, emits `LocalLet`s for each leaf ident (introducing an intermediate anon temp per nesting level):

```tw
fn lower_pattern_bindings(
  ctx: LowerCtx,
  base: CoreExpr,
  pat: Pattern,
  span: Span,
) LetPatternLowerOut {
  case pat.kind {
    .Wildcard => .{ stmts: [], ctx },   // binds nothing
    .Ident(name) => {
      lo := ctx.alloc_local(name)
      .{ stmts: [.LocalLet(lo.local, base, span)], ctx: lo.ctx }
    },
    .Tuple(subs) => {
      field_tys := tuple_field_types(base.ty, ctx.env)
      out_stmts: Vector<LoweredStmt> = []
      cur_ctx := ctx
      for i in 0..subs.len() {
        sub_ty := if i < field_tys.len() { field_tys[i] } else { MonoType.Void }
        field := record_get_expr(base, FieldId.{ id: i }, sub_ty, span)
        // For a nested tuple sub-pattern, bind an intermediate temp so `base`
        // (the field read) is evaluated once, then recurse against the temp.
        case subs[i].kind {
          .Tuple(_) => {
            tlo := cur_ctx.alloc_anon_local()
            cur_ctx = tlo.ctx
            out_stmts = .append(.LocalLet(tlo.local, field, span))
            temp_expr := local_expr(tlo.local, sub_ty, span)
            r := lower_pattern_bindings(cur_ctx, temp_expr, subs[i], span)
            cur_ctx = r.ctx
            out_stmts = .concat(r.stmts)
          },
          _ => {
            r := lower_pattern_bindings(cur_ctx, field, subs[i], span)
            cur_ctx = r.ctx
            out_stmts = .concat(r.stmts)
          },
        }
      }
      .{ stmts: out_stmts, ctx: cur_ctx }
    },
    _ => .{ stmts: [], ctx },   // unreachable: irrefutability enforced by checker
  }
}
```

- [ ] **Step 4: Wire the `lower_stmts` case**

Add to the `case stmt` block (`statements.tw:31`):

```tw
.LetPattern(lp) => {
  r := lower_expr_fn(cur_ctx, lp.value)
  tlo := r.ctx.alloc_anon_local()
  temp := tlo.local
  lowered = .append(.LocalLet(temp, r.expr, lp.span))
  temp_expr := local_expr(temp, r.expr.ty, lp.span)
  br := lower_pattern_bindings(tlo.ctx, temp_expr, lp.pattern, lp.span)
  lowered = .concat(br.stmts)
  cur_ctx = br.ctx
},
```

Confirm imports in `statements.tw` include `record_get_expr`, `tuple_field_types`, `local_expr`, `FieldId`, `MonoType` (add `use` lines as needed).

- [ ] **Step 5: Run to verify pass**

Run: `make boot-test`
Expected: PASS — flat, nested, wildcard, and single-evaluation tests all green.

- [ ] **Step 6: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/lower_core/statements.tw && target/twk lint boot/main.tw
git add boot/compiler/lower_core/statements.tw boot/tests/suites/codegen_integration_suite.tw
git commit -m "feat(tuple): lower LetPattern to temp + positional field reads"
```

---

## Task 5: boot module-level `LetPattern` + full wiring checklist

Wire the remaining `Stmt` touch points that would otherwise silently `_ =>` no-op, and make **module-level** `(a, b) := …` work (checker + global collection + lowering).

**Files:**
- Modify: `boot/compiler/checker.tw:5319` (check_top_level_stmt)
- Modify: `boot/compiler/lower_core.tw:268-390` (module-let global collection, two passes at ~274 and ~381)
- Modify: `boot/compiler/resolver.tw` (~902), `boot/compiler/lint.tw` (~15 Stmt sites), `boot/compiler/query/hover.tw` (~348), `boot/compiler/unused_imports.tw` (~332), `boot/compiler/module_compiler.tw` (~446)
- Test: `boot/tests/suites/codegen_integration_suite.tw`

**Interfaces:**
- Consumes: `check_let_pattern` (Task 3), `lower_pattern_bindings` (Task 4), the module-global collection machinery keyed on `LetStmt` at `lower_core.tw:274`/`:381`.

- [ ] **Step 1: Write the failing module-level run test**

```tw
src := "fn dm(a: Int, b: Int) (Int, Int) { (a / b, a % b) }\n(q, r) := dm(17, 5)\nfn main() Int { q * 100 + r }\nc := main()\nif c != 302 { error(\"bad\") }"
code := try codegen_harness.run_exit_code(src)
try assert.equal(code, 0)
.Ok({})
```

- [ ] **Step 2: Run to verify failure**

Run: `make boot-test`
Expected: FAIL — `check_top_level_stmt` has `.Let(_) => …`/`_ =>` no-op for `LetPattern`, and `lower_core.tw`'s module-let collection only handles `.Let(ls)`.

- [ ] **Step 3: Enumerate every `Stmt` match site and classify**

Run: `grep -rn "case stmt\|\.Let(\|\.For(\| Stmt " boot/compiler/*.tw boot/compiler/**/*.tw` and build the checklist. For each site decide: does it need real `LetPattern` handling, or is a same-as-`Let` treatment correct? Wire every one — do not leave `LetPattern` to a silent `_ =>`. Known sites (from the design):
  - `checker.tw` `check_top_level_stmt` (~5319) — **must wire**.
  - `query/hover.tw` (~348) — hover over destructured names.
  - `resolver.tw` (~902), `lower_core.tw` (~274/~381 module-let), `lint.tw` (~15 sites: rebinding/redundant-binding), `module_compiler.tw` (~446), `unused_imports.tw` (~332).

- [ ] **Step 4: Implement module-level checking + global collection**

`check_top_level_stmt`: add `.LetPattern(lp) => check_let_pattern(lp, ctx, diags),` — but module-level bindings bind into module scope; verify how `.Let` module globals get their types visible to later top-level items (there is a pre-pass — read `lower_core.tw:268-390` and the checker's top-level binding pass). Mirror that route for each destructured ident in the pattern.

In `lower_core.tw` at both `.Let(ls)` arms (~274 collection, ~381 emission), add a parallel `.LetPattern(lp)` arm that: allocates a `GlobalId` per **leaf ident** in `lp.pattern` (walk the pattern), and at emission time expands to the temp + `._N` reads writing into those globals — reuse `lower_pattern_bindings` but targeting module globals rather than locals (follow how `.Let(ls)` maps `module_let_globals[i]`).

- [ ] **Step 5: Wire the remaining touch points**

For `lint.tw`, `hover.tw`, `resolver.tw`, `unused_imports.tw`, `module_compiler.tw`: add `LetPattern` arms that walk `lp.pattern`'s leaf idents and treat them like the names a `LetStmt` introduces (so rebinding/unused/hover rules see the destructured names). Where a site currently pattern-matches `.Let(ls)` to read `ls.name`, add `.LetPattern(lp) => <iterate leaf idents>`.

- [ ] **Step 6: Run to verify pass**

Run: `make boot-test`
Expected: PASS — module-level test green.

- [ ] **Step 7: Self-host smoke check**

Run: `make stage2`
Expected: reaches a fixed point (boot compiles boot with the new node fully wired). If it errors on an unwired `_ =>` site, return to Step 3.

- [ ] **Step 8: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/checker.tw boot/compiler/lower_core.tw boot/compiler/resolver.tw boot/compiler/lint.tw boot/compiler/query/hover.tw boot/compiler/unused_imports.tw boot/compiler/module_compiler.tw && target/twk lint boot/main.tw
git add -A
git commit -m "feat(tuple): wire module-level LetPattern and all Stmt consumers"
```

---

## Task 6: boot `for` / `collect` element binder

Add the destructuring element binder to `for` and `collect` (both reduce to the let case). Both loop forms route through `setup_indexed_iter`, so lowering has a single change point.

**Files:**
- Modify: `boot/compiler/parser.tw:2849-2912` (parse_for_stmt), `boot/compiler/parser.tw:2387-2453` (parse_collect_expr)
- Modify: `boot/compiler/checker.tw:4016-4053` (bind_iterable_vars)
- Modify: `boot/compiler/lower_core/iteration.tw:35-141` (setup_indexed_iter)
- Test: `boot/tests/suites/codegen_integration_suite.tw`, `boot/tests/suites/checker_suite.tw`

**Interfaces:**
- Consumes: `check_pattern`, `lower_pattern_bindings` (Task 4), `tuple_expr_to_pattern` (Task 2).
- Produces: `ForStmt.element_pattern`/`CollectExpr.element_pattern` set by the parser; `bind_iterable_vars` extended with `element_pattern: Pattern?`.

- [ ] **Step 1: Write failing run tests**

```tw
// for (a, b) in xs — destructure element
src := "fn main() Int {\n  xs := [(1, 2), (3, 4)]\n  sum := 0\n  for (a, b) in xs { sum = sum + a * 10 + b }\n  sum\n}\nc := main()\nif c != 44 { error(\"bad\") }"
code := try codegen_harness.run_exit_code(src)
try assert.equal(code, 0)
.Ok({})
```

```tw
// for (a, b), i in xs — destructure + index
src := "fn main() Int {\n  xs := [(1, 2), (3, 4)]\n  acc := 0\n  for (a, b), i in xs { acc = acc + (a + b) * i }\n  acc\n}\nc := main()\nif c != 7 { error(\"bad\") }"
code := try codegen_harness.run_exit_code(src)
try assert.equal(code, 0)
.Ok({})
```

```tw
// collect (a, b) in xs
src := "fn main() Int {\n  xs := [(1, 2), (3, 4)]\n  ys := collect (a, b) in xs { a + b }\n  ys[0] * 10 + ys[1]\n}\nc := main()\nif c != 37 { error(\"bad\") }"
code := try codegen_harness.run_exit_code(src)
try assert.equal(code, 0)
.Ok({})
```

- [ ] **Step 2: Run to verify failure**

Run: `make boot-test`
Expected: FAIL — parser doesn't recognize the `(` binder; `element_pattern` is always `.None`.

- [ ] **Step 3: Parse the tuple element binder**

In `parse_for_stmt` (`parser.tw:2849`) and `parse_collect_expr` (`parser.tw:2387`), before the existing `if c.kind() == .Ident` branch, add: if `c.kind() == .LParen`, parse a pattern via the existing `parse_pattern` (`parser.tw:1640`, which already handles `Pattern::Tuple`), then expect `.In` (with optional `, <idx>` before `.In` for the index form). Set `element_pattern = .Some(parsed_pattern)`, leave `pattern = .None`, set `index` if present. A leading `(` after `for`/`collect` is unambiguously a tuple pattern (no tail conflict).

- [ ] **Step 4: Extend `bind_iterable_vars` (checker)**

Add an `element_pattern: Pattern?` parameter to `bind_iterable_vars` (`checker.tw:4016`). When `.Some(pat)`, call `check_pattern(pat, info.elem_ty, cur_ctx, cur_diags)` (with the irrefutability guard) instead of `bind_optional(pattern, info.elem_ty)`. Update both callers — `check_for` (`checker.tw:3992`) and `synth_collect`/`check_collect` (`checker.tw:4193`, `:4211`) — to pass `for_stmt.element_pattern` / `ce.element_pattern`.

- [ ] **Step 5: Prepend destructure in `setup_indexed_iter` (lowering)**

Add an `element_pattern: Pattern?` parameter to `setup_indexed_iter` (`iteration.tw:35`). When `.Some(pat)`: allocate an anon element local (instead of `alloc_local(elem_name)`), then before `lower_block_fn(ctx, body)`, prepend the `lower_pattern_bindings` output (Task 4) against `local_expr(elem_local, elem_ty, s)` to the body. Update both callers — `lower_for` (`iteration.tw:723`) and `lower_collect` (`iteration.tw:664`) — to pass `for_stmt.element_pattern` / `ce.element_pattern`. Concretely, wrap `body_r.expr` so the destructure `Let`s precede the user body.

- [ ] **Step 6: Add a checker test for a refutable loop binder**

```tw
src := "fn main() Void {\n  xs := [(1, 2)]\n  for (a, .Some(x)) in xs { }\n}"
errs := try check_errs(src)
try assert.str_contains(errs.join("\n"), "refutable")
.Ok({})
```

- [ ] **Step 7: Run to verify pass**

Run: `make boot-test`
Expected: PASS.

- [ ] **Step 8: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/parser.tw boot/compiler/checker.tw boot/compiler/lower_core/iteration.tw && target/twk lint boot/main.tw
git add -A
git commit -m "feat(tuple): destructure element binder in for and collect"
```

---

## Task 7: boot formatter (`LetPattern`, for/collect binders)

Make `twk fmt` print the surface syntax and be idempotent — this is why boot uses a first-class node instead of a parse-time desugar.

**Files:**
- Modify: `boot/compiler/fmt/printer.tw:1130` (replace the Task 1 `format_let_pattern` stub), `printer.tw:1233` (format_for), `printer.tw:2377` (format_collect)
- Test: `boot/tests/suites/fmt_suite.tw` + `boot/tests/suites/fmt_cases/` fixtures

**Interfaces:**
- Consumes: `format_pattern(pat, tm)` (`printer.tw:2052`, already handles `.Tuple`), `format_type_expr`, `format_expr`.

- [ ] **Step 1: Add fmt fixtures + failing test**

Create `boot/tests/suites/fmt_cases/tuple_binding.original` and `.expected` (identical, canonical form):

```tw
fn dm(a: Int, b: Int) (Int, Int) { (a / b, a % b) }

(q, r) := dm(17, 5)

fn main() Int {
  (a, _): (Int, Int) = (10, 20)
  ((x, y), z) := ((1, 2), 3)
  sum := 0
  for (p, q) in [(1, 2)] { sum = sum + p + q }
  for (p, q), i in [(1, 2)] { sum = sum + (p + q) * i }
  ys := collect (m, n) in [(1, 2)] { m + n }
  a + x + y + z + sum + ys[0]
}
```

Add to `fmt_suite.tw`'s suite: `.test("tuple binding forms", assert_fmt_case("tuple_binding"))`.

- [ ] **Step 2: Run to verify failure**

Run: `make boot-test`
Expected: FAIL — `format_let_pattern` is the Task 1 stub (`/* let-pattern */`); for/collect don't print the element binder.

- [ ] **Step 3: Implement `format_let_pattern`**

```tw
fn format_let_pattern(s: LetPatternStmt, tm: TriviaMap) Doc {
  parts: Vector<Doc> = [format_pattern(s.pattern, tm)]
  case s.ty {
    .Some(ty) => {
      parts = .append(doc.text(": "))
      parts = .append(format_type_expr(ty))
      parts = .append(doc.text(" = "))
    },
    .None => parts = .append(doc.text(" := ")),
  }
  parts = .append(format_expr(s.value, tm))
  doc.concat(parts).group()
}
```

- [ ] **Step 4: Print the for/collect element binder**

In `format_for` (`printer.tw:1233`) and `format_collect` (`printer.tw:2377`), before the existing `case s.pattern` handling, add `case s.element_pattern { .Some(pat) => print format_pattern(pat) then optional ", idx" then " in ", .None => <existing pattern path> }`.

- [ ] **Step 5: Run to verify pass (incl. idempotence)**

Run: `make boot-test`
Expected: PASS — `assert_fmt_case` checks `format(original) == expected` and `format(expected) == expected`.

- [ ] **Step 6: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/fmt/printer.tw && target/twk lint boot/main.tw
git add boot/compiler/fmt/printer.tw boot/tests/suites/fmt_suite.tw boot/tests/suites/fmt_cases/tuple_binding.*
git commit -m "feat(tuple): format tuple bindings and destructuring loop binders"
```

---

## Task 8: stage0 let-LHS parser fix

Make stage0 recognize a `(`-led tuple-pattern let-LHS (routing it to `parse_let_stmt`, which already runs `parse_pattern`) and stop `:=` from being mis-parsed as an infix operator (the current ICE via `token_to_binop(ColonEq)` → `unreachable!()`).

**Files:**
- Modify: `src/syntax/parser.rs:2190-2204` (is_let_binding), `src/syntax/parser.rs:2656-2660` (infix_binding_power `ColonEq` BP)
- Test: `tests/tuple_pattern_run_test.rs` (add a compile-only assertion) or a `src/syntax/parser.rs` `#[test]`

**Interfaces:**
- Consumes: `parse_let_stmt` (`parser.rs:2205`, already calls `parse_pattern`), `parse_pattern` (`parser.rs:1991`, handles `Pattern::Tuple`).

- [ ] **Step 1: Write the failing test**

Add to `tests/tuple_pattern_run_test.rs`:

```rust
#[test]
fn tuple_let_binding_runs() {
    assert_program_matches_expected(
        r#"
fn dm(a: Int, b: Int) (Int, Int) { (a / b, a % b) }
(q, r) := dm(17, 5)
if q * 100 + r != 302 {
  error("mismatch")
}
"#,
    );
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --release tuple_let_binding_runs`
Expected: FAIL / panic — `(q, r)` parses as a tuple expression, `:=` hits `infix_binding_power` then `token_to_binop(ColonEq)` → `unreachable!()` (ICE), so `compile_to_wasm` panics.

- [ ] **Step 3: Extend `is_let_binding` for a `(`-led LHS**

`is_let_binding` (`parser.rs:2190`) currently only returns true when token 0 is an `Ident` followed by `ColonEq`/`Colon`. Add: if token 0 is `LParen`, scan forward across balanced parens; if the token immediately after the matching `)` is `ColonEq` or `Colon`, return true. (Do **not** claim a trailing `Eq` — that is the rebind case, left to fall through so it can be rejected cleanly in Task 9.)

- [ ] **Step 4: Remove/gate the `ColonEq` infix BP**

In `infix_binding_power` (`parser.rs:2660`), the arm `Eq | ColonEq => (2, 1)` gives `:=` a binding power so the Pratt parser treats it as infix. Split it so `ColonEq` returns `None` (no infix BP) — the let path must claim `:=` first. Confirm nothing else depends on `ColonEq` being infix (`grep -n "ColonEq" src/syntax/parser.rs`). Do **not** add a `ColonEq` arm to `token_to_binop` — that would produce a `Binary` expression, not `Stmt::Let { pattern: Pattern::Tuple }`.

- [ ] **Step 5: Run to verify pass**

Run: `cargo test --release tuple_let_binding_runs`
Expected: PASS — but the checker/lowering placeholders may now fire (`UnsupportedFeature`). If the test fails with an `UnsupportedFeature` compile error rather than an ICE, that is expected progress: mark this test `#[ignore = "enabled in Task 10/11"]` and instead assert parse success here via a smaller `#[test]` that calls the parser directly and matches `Stmt::Let { pattern: Pattern::Tuple(..), .. }`. Remove the `#[ignore]` in Task 11.

- [ ] **Step 6: Confirm tuple tail-expr still parses**

Run: `cargo test --release` for an existing tuple test (e.g. `grep -l "Int, Int)" tests/*.rs`), or add a parser `#[test]` that `fn f() (Int, Int) { (1, 2) }` still parses with a tuple tail. Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/syntax/parser.rs tests/tuple_pattern_run_test.rs
git commit -m "fix(stage0): parse tuple-let LHS instead of ICEing on :="
```

---

## Task 9: stage0 checker — tuple patterns in let / for / collect

Replace the `Pattern::Tuple` placeholders with real handling: recurse against the RHS / element type, bind each identifier, enforce irrefutability. Factor a shared helper (the `for` branch is per-iterable-kind). Also reject tuple-pattern rebind cleanly.

**Files:**
- Modify: `src/types/check.rs:2749-2756` (check_let_stmt Tuple arm), `src/types/check.rs:4017-4110+` (check_for_stmt per-kind branches), `src/types/check.rs:4172-4210` (synth_collect / check_collect)
- Test: `tests/tuple_pattern_run_test.rs` (compile-succeeds assertions) + a `check.rs` `#[test]` for the error cases

**Interfaces:**
- Produces: `fn bind_tuple_pattern(&mut self, pat: &Pattern, expected: &MonoType, span: Span)` — recurses `Pattern::Tuple` against a `MonoType::Named{TupleN}` (or fresh element metas), binds `Pattern::Ident`, skips `Pattern::Wildcard`, and pushes a `TypeError` for any refutable sub-pattern. Reused by let, for, and collect.

- [ ] **Step 1: Write the failing error-case test**

Add a `#[test]` in `src/types/check.rs` (or a checker test file) asserting a refutable binder produces a diagnostic and a matching arity produces none. Model it on existing checker unit tests (`grep -n "#\[test\]" src/types/check.rs | head`). Assert: `(a, .Some(x)) := (1, .Some(2))` yields an error mentioning refutable; `(a, b) := (1, 2)` yields no error and binds `a`, `b` as `Int`.

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --release check` (targeted to the new test name)
Expected: FAIL — the placeholder emits `UnsupportedFeature` for any tuple pattern.

- [ ] **Step 3: Implement `bind_tuple_pattern`**

Recurse: for `Pattern::Tuple(subs, span)`, resolve the expected type to a `TupleN` named type (arity `subs.len()`); if the expected type's arity/shape mismatches, push a type error; for each element, recurse against the element type (`args[i]`). For `Pattern::Ident(name,_)` bind `name` to the expected type. For `Pattern::Wildcard(_)` bind nothing. For `Pattern::Variant`/`Pattern::Literal` push a "refutable pattern not allowed in a binding; use `case`" error. Mirror the arity/element resolution stage0 already does for tuple *case* patterns (`grep -n "Tuple" src/types/check.rs` for the case-arm path) so binding-position and case-position agree.

- [ ] **Step 4: Wire the three call sites**

- `check_let_stmt` (`check.rs:2749`): split the combined `Variant | Literal | Tuple` arm — keep `Variant | Literal` erroring, route `Pattern::Tuple(..)` through: resolve RHS type (annotation → check mode via `resolve_type` + `check_expr`; else `synth_expr`), then `self.bind_tuple_pattern(pattern, &rhs_ty, span)`.
- `check_for_stmt` (`check.rs:4017`): in each iterable-kind branch (Vector/String/Range/Iterator/IntoIterator), replace the `_ => UnsupportedFeature` pattern arm with `Pattern::Tuple(..) => self.bind_tuple_pattern(pattern, &elem_ty, iter.span)`. Since the branches repeat, call the one shared helper.
- `synth_collect` / `check_collect` (`check.rs:4172`): same treatment for the collect element pattern.

- [ ] **Step 5: Add the tuple-rebind rejection**

`(a, b) = e` still falls to the expression path (Task 8 deliberately didn't claim `Eq`). Find where stage0 synths/checks `BinOp::Assign` (or the assignment lowering) and, when the LHS is a tuple expression, push "tuple-pattern rebind not supported; use `:=` for a new binding or `case` to match" instead of the confusing generic error. (`grep -n "Assign" src/types/check.rs src/ir/lower.rs`.)

- [ ] **Step 6: Run to verify pass**

Run: `cargo test --release check` (new tests)
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/types/check.rs
git commit -m "feat(stage0): type-check tuple patterns in let, for, and collect binders"
```

---

## Task 10: stage0 lowering — tuple patterns in let / for / collect

Expand the tuple pattern into a temp + positional `RecordGet` field reads, mirroring boot.

**Files:**
- Modify: `src/ir/lower.rs:790-797` (Stmt::Let `_` arm), the `Stmt::For` element-binding site (`lower.rs:1016+` and the per-kind helpers `lower_range_for_stmt` / `lower_iterator_for_stmt` / `lower_dict_for_stmt` + the generic Vector/String path at ~1060), and `lower_collect` (`lower.rs:5085`)
- Test: `tests/tuple_pattern_run_test.rs` (enable Task 8's ignored test + add for/collect run tests)

**Interfaces:**
- Consumes: `CoreExprKind::RecordGet { record, index }` (existing tuple field read), `self.type_env.get_field_index(type_id, "_N")` (or direct positional index — confirm how the case-arm Tuple lowering at `lower.rs:3443` addresses fields), `self.local_allocator.alloc_and_bind`.
- Produces: `fn lower_tuple_pattern_bindings(&mut self, base_local: LocalId, base_ty: &MonoType, pat: &Pattern, body: CoreExpr) -> CoreExpr` — wraps `body` in nested `Let`s binding each leaf ident to a `RecordGet` chain.

- [ ] **Step 1: Enable/write failing run tests**

Remove the `#[ignore]` from Task 8's `tuple_let_binding_runs`. Add:

```rust
#[test]
fn tuple_for_destructure_runs() {
    assert_program_matches_expected(
        r#"
fn main() Int {
  xs := [(1, 2), (3, 4)]
  sum := 0
  for (a, b) in xs { sum = sum + a * 10 + b }
  sum
}
r := main()
if r != 44 { error("mismatch") }
"#,
    );
}

#[test]
fn tuple_collect_destructure_runs() {
    assert_program_matches_expected(
        r#"
fn main() Int {
  xs := [(1, 2), (3, 4)]
  ys := collect (a, b) in xs { a + b }
  ys[0] * 10 + ys[1]
}
r := main()
if r != 37 { error("mismatch") }
"#,
    );
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --release tuple_`
Expected: FAIL — `Stmt::Let` `_` arm and the for/collect element paths push `LowerError::UnsupportedFeature`.

- [ ] **Step 3: Implement `lower_tuple_pattern_bindings`**

Recurse the pattern: for `Pattern::Ident(name,_)` allocate/bind a local and wrap the continuation in `Let { local, value: RecordGet(base, i), body }`; for `Pattern::Wildcard` emit nothing (skip the field); for a nested `Pattern::Tuple` bind an intermediate temp to the `RecordGet`, then recurse against it. Determine the field index the same way the case-arm Tuple lowering at `lower.rs:3443` does (positional; `TupleN` fields `_0.._n` map 1:1).

- [ ] **Step 4: Wire the let path**

Replace the `Stmt::Let` `_ =>` arm (`lower.rs:790`): for `Pattern::Tuple`, lower `value` into a temp local, then `lower_tuple_pattern_bindings(temp, &value_ty, pattern, body_from_rest)`. Preserve the `in_init_context` module-global handling — module-level tuple lets must bind each leaf ident to its pre-assigned global id (mirror the `Pattern::Ident` branch at `lower.rs:754`).

- [ ] **Step 5: Wire the for + collect element paths**

In the generic Vector/String for path (`lower.rs:1060+`) and the range/iterator/dict helpers, when the element `pattern` is a `Pattern::Tuple`, bind the element to a temp local and prepend `lower_tuple_pattern_bindings` to the loop body before lowering it. Do the same in `lower_collect` (`lower.rs:5085`).

- [ ] **Step 6: Run to verify pass**

Run: `cargo test --release tuple_`
Expected: PASS — let, for, collect run tests all green.

- [ ] **Step 7: Commit**

```bash
git add src/ir/lower.rs tests/tuple_pattern_run_test.rs
git commit -m "feat(stage0): lower tuple binding patterns to temp + field reads"
```

---

## Task 11: Self-host parity gate + disambiguation regression guards

Prove the two compilers agree and the risky parser change didn't regress anything.

**Files:**
- Test: `tests/tuple_pattern_run_test.rs`, `boot/tests/suites/*`

- [ ] **Step 1: Add cross-compiler regression tests**

Ensure both suites cover: a tuple **tail expression** (`fn f() (Int, Int) { (1, 2) }`) still type-checks and runs; a **module-level** `(a, b) := …` works; the stage0 `(a, b) := …` ICE is gone (Task 8's test); an all-wildcard binder `(_, _) := p` is legal and binds nothing.

- [ ] **Step 2: Run the full boot suite**

Run: `make boot-test`
Expected: PASS.

- [ ] **Step 3: Run the stage0 tuple run tests**

Run: `cargo test --release tuple_`
Expected: PASS.

- [ ] **Step 4: Self-host fixed point (the hard gate)**

Run: `make stage2`
Expected: reaches a fixed point (boot compiles boot to a stable artifact). This proves boot's new node + stage0's handling behaviorally agree.

- [ ] **Step 5: Rebuild the CLI**

Run: `make bundle-cli`
Expected: `target/twk` rebuilds cleanly.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "test(tuple): cross-compiler regression guards for binding-destructuring"
```

---

## Task 12: Grammar & spec docs

**Files:**
- Modify: `docs/grammar.ebnf`, `docs/spec.md`

- [ ] **Step 1: Update `docs/grammar.ebnf`**

Extend the `LetStmt`, `ForStmt`/for-binder, and `Collect` productions to admit a tuple pattern LHS/binder. Reuse the existing pattern production; restrict to the irrefutable subset in prose if the grammar can't express it.

- [ ] **Step 2: Update `docs/spec.md`**

Document the binding forms (`:=`, `: T =`), the `for`/`collect` destructuring binders, single-evaluation semantics, and the irrefutability rule (refutable ⇒ compile error, use `case`). Note tuple-pattern rebind and function-parameter patterns are out of scope.

- [ ] **Step 3: Commit**

```bash
git add docs/grammar.ebnf docs/spec.md
git commit -m "docs(tuple): document binding-destructuring in spec and grammar"
```

---

## Task 13: tree-sitter grammar (human test step)

**Files:**
- Modify: `tree-sitter-twinkle/grammar.js` (+ regenerated `src/parser.c`, `grammar.json`, `node-types.json`, rebuilt `tree-sitter-twinkle.wasm`)

- [ ] **Step 1: Edit `grammar.js`**

Let the let-binding LHS and the `for`/`collect` element binder accept a tuple pattern.

- [ ] **Step 2: Regenerate + rebuild**

Run: `cd tree-sitter-twinkle && npx tree-sitter generate && npx tree-sitter build --wasm` (Docker required for the wasm).

- [ ] **Step 3: Ask the human to run tree-sitter tests**

Per project rule, the agent does NOT run `tree-sitter test`. Stop and ask the human to run it and confirm green.

- [ ] **Step 4: Commit (after human confirms)**

```bash
git add tree-sitter-twinkle/grammar.js tree-sitter-twinkle/src/ tree-sitter-twinkle/tree-sitter-twinkle.wasm
git commit -m "feat(tuple): tree-sitter grammar for binding-destructuring"
```

---

## Task 14: Plan bookkeeping

- [ ] **Step 1: Remove the design-doc plan rows and archive**

Per memory ([[feedback_plans_readme_remove_when_done]]): on completion, delete this plan's row and the design-doc's row from `docs/plans/README.md` (do not mark Done), and move `docs/plans/tuple-binding-destructuring-design.md` and this file to `docs/plans/archive/`.

- [ ] **Step 2: Note the follow-on (not in this plan)**

Adopting `(a, b) :=` for multi-return inside `boot/compiler/*.tw` (replacing helper records) is the motivating payoff and a natural next branch — it also stress-tests `make stage2` in anger. Leave a one-line note in the archived design doc.

- [ ] **Step 3: Commit**

```bash
git add docs/plans/
git commit -m "docs(plans): archive tuple binding-destructuring plan"
```

---

## Self-Review

**Spec coverage:** let (`:=` + annotated) → Tasks 2–5; for/collect binders → Task 6; irrefutability → Tasks 3, 9; single-evaluation → Task 4 (test) + Task 6; nested + wildcard → Tasks 4, 10; module-level → Task 5; tuple-rebind rejection → Tasks 2 (boot), 9 (stage0); disambiguation (boot dual-path + stage0) → Tasks 2, 8; boot AST + wiring checklist → Tasks 1, 5; formatter → Task 7; stage0 checker/lowering → Tasks 9, 10; grammar/spec/tree-sitter → Tasks 12, 13; parity gate → Task 11. All design sections map to a task.

**Open verification points flagged inline for the implementer** (confirm against source before coding, don't assume): the exact annotation-resolver call in boot (`check_let` at `checker.tw:5349`); how `_` is lexed in boot (`Ident("_")` vs a dedicated token); how boot module-global lets flow types to later top-level items (`lower_core.tw:268-390` + checker pre-pass); the stage0 tuple field-index convention (`lower.rs:3443` case-arm Tuple); the stage0 assignment site for rebind rejection.

**Type consistency:** `LetPatternStmt` fields (`pattern`, `ty`, `value`, `span`) are used identically in Tasks 1/3/4/7; `element_pattern: Pattern?` consistent across ForStmt/CollectExpr in Tasks 1/6/7; `lower_pattern_bindings` (boot, Task 4) and `lower_tuple_pattern_bindings` (stage0, Task 10) are distinct names for the two compilers by design; `bind_tuple_pattern` (stage0, Task 9) and `bind_iterable_vars` extension (boot, Task 6) are consistent within their compilers.
