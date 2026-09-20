# Tuple-Pattern Rebinding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task by task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** Draft for review.

**Goal:** Support `(a, b) = expression` as a simultaneous rebind of existing tuple-shaped names in both the boot and stage0 compilers.

**Architecture:** Keep `=` as an assignment expression returning `Void`. Recognize a tuple-shaped left side without treating it as a new binding; check it against the evaluated RHS type, then lower through one temporary tuple and positional field reads into assignments to existing slots. Reuse the ordinary tuple type and field layout. No new runtime operation is needed.

**Tech Stack:** Twinkle boot compiler, Rust stage0 compiler, Wasm GC, tree-sitter grammar, Deno test runtime.

**Spec:** `docs/spec.md` §13.7 (tuple syntax and binding-position destructuring) and the existing rebinding rules. This plan replaces only §13.7's current statement that tuple-pattern rebind is unsupported; the archived binding-destructuring design remains a record of the earlier scope decision.

## Semantics to settle in the implementation

```tw
a := 1
b := 2
(a, b) = (b, a)       // a == 2, b == 1
((a, _), b) = ((3, 0), 4)
```

- Every identifier leaf must resolve to an existing rebinding target. `_` discards a position; nested tuple patterns are allowed. A new name, literal, variant, call, field access, or index access in a tuple target is a compile error.
- A named target may occur only once in the pattern. Reject `(a, a) = pair` explicitly, including duplicates across nesting. This avoids order-dependent writes.
- Evaluate the entire RHS once **before** any target is written. Read every projected value from that snapshot, so `(a, b) = (b, a)` swaps correctly even when the RHS refers to targets.
- Require tuple arity and each leaf type to match, including at nested levels. Return `Void`, as other assignments do.
- Apply the same local/module and public-binding rules as `name = value`: private module globals may be rebound; exported globals may not. Resolve a shadowing local before deciding that a name refers to a public global. A mixed new/existing pattern such as `(new_name, old_name) = pair` is rejected; `:=` remains the all-new form.
- Permit the expression wherever ordinary assignment expressions are accepted, including a statement in a block or `case` arm. Do not broaden assignment to function parameters or add field/index targets inside tuple patterns.

## Global constraints

- Implement boot first and keep Rust stage0 as a bootstrap-correct reference. `make stage2` must reach its self-host fixed point.
- Run `target/twk fmt <changed .tw files>` and `target/twk lint <relevant entry>` after editing Twinkle files.
- Use targeted `cargo test --release <filter>`; do not run the full Rust suite for this feature.
- Never run `tree-sitter test` as the agent. Regenerate the grammar and wasm together, then ask the human to run that test.
- Run heavy verification sequentially. Do not overlap `make stage2`, bundling, or broad suites.
- Use short imperative commits with bodies explaining behavior; do not add a co-author trailer unless accurate.

## Review focus

- `(a, b) = (b, a)` must read both old values before either write.
- A side-effecting RHS must execute once, including with nested targets and `_` leaves.
- Shadowed locals must win over same-named globals, and public global rebinds must remain rejected.
- Duplicate or undefined targets must fail before lowering; no partial update should be emitted.
- Parenthesized expressions and tuple literals without `=` must keep their existing parse and formatter behavior.

## Acceptance programs for the test tasks

Each positive program should be run by both compilers. The boot integration harness can use `assert_runs_to(source, 0, label)`; the stage0 integration harness can compile the same source and execute its wasm. Each program ends with `error("bad")` if its expected value is wrong.

**Swap and nested/wildcard rebind:**

```tw
fn main() Int {
  a := 1
  b := 2
  (a, b) = (b, a)
  if a * 10 + b != 21 { error("swap failed") }
  ((a, _), b) = ((3, 99), 4)
  a * 10 + b
}
if main() != 34 { error("bad") }
```

**Single RHS evaluation, including an all-wildcard target:**

```tw
fn bump(counter: Cell<Int>) (Int, Int) {
  counter.update(fn(n) { n + 1 })
  (counter.get(), counter.get() + 1)
}
fn main() Int {
  counter := Cell.new(0)
  a := 0
  b := 0
  (a, b) = bump(counter)
  (_, _) = bump(counter)
  counter.get() * 100 + a * 10 + b
}
if main() != 212 { error("bad") }
```

**Private module globals and an assignment arm:**

```tw
a := 1
b := 2
(a, b) = (b, a)
fn main() Int {
  case true {
    true => (a, b) = (3, 4),
    false => {},
  }
  a * 10 + b
}
if main() != 34 { error("bad") }
```

**Negative checker cases:**

| Source after `a := 1; b := 2` | Required result |
| --- | --- |
| `(a, missing) = (3, 4)` | Undefined `missing`; no new binding. |
| `(a, a) = (3, 4)` | Duplicate target `a`. |
| `(a, b) = (3, 4, 5)` | Tuple arity mismatch. |
| `(a, b) = ("x", 4)` | Leaf type mismatch at `a`. |
| `(a, 0) = (3, 4)` | Invalid/refutable target. |
| `(a, obj.field) = (3, 4)` | Invalid field target inside tuple. |
| `(a, xs[0]) = (3, 4)` | Invalid index target inside tuple. |

Also cover `pub a := 1; b := 2; (a, b) = (3, 4)` as a public-global rejection, a tuple assignment inside a function that targets public global `a` as a rejection, and a function-local `a` shadowing that public global as a successful local rebind.

## File map

| Area | Files | Responsibility |
| --- | --- | --- |
| Boot parse/check | `boot/compiler/parser.tw`, `boot/compiler/checker.tw` | Accept tuple assignment and validate existing targets/types. |
| Boot lowering | `boot/compiler/lower_core/operators.tw`, `boot/compiler/lower_core/statements.tw`, `boot/compiler/lower_core.tw`, a helper in `boot/compiler/lower_core/` | Snapshot RHS and assign projected leaves in expression, block, and module paths. |
| Stage0 | `src/syntax/parser.rs`, `src/types/check.rs`, `src/ir/lower.rs` | Preserve tuple-assignment shape, check it, and lower it with the same semantics. |
| Tooling/docs | `boot/compiler/fmt/printer.tw`, `docs/spec.md`, `docs/grammar.ebnf`, `tree-sitter-twinkle/grammar.js` and generated artifacts | Round-trip syntax and describe/highlight the accepted target. |
| Tests | `boot/tests/suites/parser_suite.tw`, `checker_suite.tw`, `codegen_integration_suite.tw`, `fmt_suite.tw` and `fmt_cases/`, `tests/tuple_pattern_run_test.rs` | Parser, diagnostic, runtime, and parity coverage. |

## Task 1: Accept the syntax without changing tuple expressions

**Files:** `boot/compiler/parser.tw`, `src/syntax/parser.rs`, `boot/tests/suites/parser_suite.tw`, stage0 parser tests in `src/syntax/parser.rs`.

**Interface produced:** A tuple-target `=` remains an assignment expression in each AST. Boot's LHS is `ExprKind.Tuple`; stage0's parenthesized tuple is the existing positional `TupleN` record-literal representation. `:=` and annotated tuple lets retain their current AST paths.

- [ ] Add parser tests for `(a, b) = rhs`, `((a, _), b) = rhs`, a tuple tail expression, `(a, b) := rhs`, and `(a, b): (Int, Int) = rhs`. The `=` tests must assert an assignment expression with the tuple-shaped LHS; the others must assert their pre-existing shapes.
- [ ] Run `target/twk run boot/tests/main.tw` and `cargo test --release syntax::parser::` to observe the current `=` rejection/failure.
- [ ] In `try_parse_tuple_let` (`boot/compiler/parser.tw`), replace the dedicated `nk == .Eq` error branch with a fallthrough to normal assignment-expression parsing. Keep the `:=`/`:` branches and the `parse_block` tail-expression fallback. Confirm that normal `parse_expr_bp(c, 0)` consumes `=` and preserves the tuple LHS.
- [ ] Keep stage0's `is_let_binding` lookahead restricted to `:=`/`:`; its general expression path already sees trailing `=`. Update its parser test to assert the assignment shape and retain the `(a, b) :=` and tuple-tail regression tests.
- [ ] Rerun `target/twk run boot/tests/main.tw` and `cargo test --release syntax::parser::`, then format/lint `boot/compiler/parser.tw` and commit the parser change.

## Task 2: Check existing names, shape, and types

**Files:** `boot/compiler/checker.tw`, `boot/tests/suites/checker_suite.tw`, `src/types/check.rs`.

**Interface produced:** A checker helper per compiler that recursively validates a tuple assignment target against an already synthesized RHS tuple type. It never binds a name or changes scope.

- [ ] Add checker tests for a swap, nested target plus wildcard, an undefined leaf, duplicate leaf, tuple arity mismatch, mismatched leaf type, literal/variant/field/index target, and public global targets at module scope and inside a function. Include a local shadowing a same-named public global.
- [ ] Run `target/twk run boot/tests/main.tw` and `cargo test --release tuple_pattern_rebind` to observe the current rejection.
- [ ] In boot's `synth_assign_op`, add an `.Tuple` arm. Synthesize the RHS once, obtain each positional type from the `Tuple2`/`Tuple3`/`Tuple4` type arguments (using the same tuple recognition as `check_pattern`), and recursively compare each leaf with local-first lookup. Use the checker's unification path and existing `UndefinedVar`, type-mismatch, and public-rebind diagnostics. Track seen target names through recursion and report a dedicated duplicate-target error. `_` has no lookup or write. Keep simple-name assignment consistent if its public-name guard currently rejects a shadowing local.
- [ ] In stage0's `synth_assign`, replace the `TupleN` unsupported-feature arm with equivalent recursive validation over its positional `_0.._N` fields. Resolve each leaf in `local_env` before `value_env`; reject it if it resolves to a public global, whether the assignment occurs at module scope or inside a function. A shadowing function local remains writable. Use `check_expr`/type unification conventions from the simple identifier arm, but do not copy its `!in_function` public-binding guard, which would allow function-level writes to public globals. Preserve a diagnostic for unsupported subexpressions. Accept only the exact positional field sequence that `parse_grouped` produces; other record-shaped assignment targets stay invalid. The stage0 AST currently erases the distinction between tuple syntax and an explicitly written `TupleN` constructor with identical fields, so do not claim to distinguish those without adding an AST marker.
- [ ] Rerun `target/twk run boot/tests/main.tw` and `cargo test --release tuple_pattern_rebind`, format/lint changed Twinkle files, and commit checker parity.

## Task 3: Lower a tuple assignment as one snapshot and leaf writes

**Files:** `boot/compiler/lower_core/operators.tw`, `boot/compiler/lower_core/statements.tw`, `boot/compiler/lower_core.tw`, a new `boot/compiler/lower_core/tuple_rebind.tw`, `boot/tests/suites/codegen_integration_suite.tw`; `src/ir/lower.rs`, `tests/tuple_pattern_run_test.rs`.

**Interface produced:** Boot `lower_tuple_rebind(ctx: LowerCtx, lhs: Expr, rhs: CoreExpr, span: Span) ExprOut` and a stage0 counterpart returning a `Void` `CoreExpr`. Both receive an already-lowered RHS and wrap it in one temporary before projecting leaves.

- [ ] Add boot runtime tests for swap, nested targets, wildcard, RHS single evaluation, assignment in a `case` arm, module-level non-public rebinding, and local-over-global shadowing. Add matching stage0 run tests. Use observable values, not only IR shape.
- [ ] Run `target/twk run boot/tests/main.tw` and `cargo test --release tuple_pattern_rebind` to see the lowering failure.
- [ ] Implement boot `tuple_rebind.tw`: create a temp `Let` for the RHS; traverse the tuple target in source order; project `._N` from the temp using the existing `record_get_expr`/tuple field types; emit `CoreExpr.Assign` for each resolved local (and the established global write form for a module global); nest/sequence writes to yield `Void`. For nested tuples, bind an intermediate projected tuple once before recursing. Never lower the target identifiers as value expressions.
- [ ] Call this helper before the generic `lower_lvalue_assign` path in `operators.tw`, `statements.tw`, and module-level statement lowering. Avoid lowering the RHS a second time on fallback. The checker guarantees valid targets; an impossible lowering lookup emits an internal diagnostic rather than silently using a sentinel ID.
- [ ] In stage0, identify the parser's positional `TupleN` LHS, lower the RHS into a temp, read each `_N` field from that temp, and assign the pre-existing `LocalId`/module global. Wire both `Stmt::Expr` lowering and the expression lowering used by case arms. Do not call `lower_expr` on the LHS record literal (that would read target names or construct a new tuple).
- [ ] Rerun `target/twk run boot/tests/main.tw` and `cargo test --release tuple_pattern_rebind`, format/lint changed Twinkle files, then commit lowering parity.

## Task 4: Formatter and tooling round-trip

**Files:** `boot/compiler/fmt/printer.tw`, `boot/tests/suites/fmt_suite.tw`, `boot/tests/suites/fmt_cases/tuple_rebind.original`, `tuple_rebind.expected`.

- [ ] Add fixtures covering flat and nested tuple assignment, `_`, a swap, and the existing `(a, b) :=`/annotated forms side by side. Assert idempotence and that `=` stays `=`.
- [ ] Run `target/twk run boot/tests/main.tw`. If generic binary formatting already renders the shape correctly, make no printer change; otherwise update only tuple assignment formatting in `format_expr` and preserve current grouping and line-break behavior.
- [ ] Run `target/twk fmt` on changed `.tw` files, lint the relevant test entry, and commit the formatter coverage.

## Task 5: Specify the surface and update tree-sitter

**Files:** `docs/spec.md`, `docs/grammar.ebnf`, `tree-sitter-twinkle/grammar.js`, generated `tree-sitter-twinkle/src/` and `tree-sitter-twinkle.wasm`.

- [ ] Replace the unsupported-rebind sentences in spec §13.7 and EBNF with the precise rules above: existing names only, wildcards/nesting, duplicate rejection, arity/type checking, RHS snapshot, `Void` result, public-binding rule, and no mixed new/existing tuple assignment. Correct EBNF's stale `TopLevelStmt` comment that says module-scope rebinding is forbidden: the checker already allows private top-level `name = value`.
- [ ] Extend tree-sitter's `_lvalue` to accept the tuple target and resolve any tuple-literal/tuple-pattern ambiguity without changing ordinary tuple expressions. Query a sample containing `(a, b) = (b, a)` and `(a, b) := pair` to confirm the first is an `assignment_expression` and the second a `let_binding`.
- [ ] Run `npx tree-sitter generate` and `npx tree-sitter build --wasm`; commit `grammar.js`, regenerated `src/`, and wasm together. The agent must **not** run `tree-sitter test`; request a human run and record the result before final integration.
- [ ] Commit the spec and grammar update with the generated artifacts.

## Task 6: Regression gates and plan bookkeeping

- [ ] Run the boot tuple/parser/checker/codegen/formatter suites and targeted stage0 tuple tests. Re-run the swap and side-effecting RHS cases through both compilers.
- [ ] Run `make stage2` to a fixed point, then rebuild the CLI and run `target/twk run examples/leetcode/main.tw` as a broad existing-program regression.
- [ ] Run `git diff --check` and review the complete branch for missed assignment contexts, scope lookup, and invalid-target diagnostics.
- [ ] Once the human confirms the tree-sitter tests and all gates pass, move this plan to `docs/plans/archive/`, remove its active row from `docs/plans/README.md`, and commit the bookkeeping. Publishing npm packages is a separate release decision.

## Self-review of this draft

- The spec rules map to parser, checker, lowering, formatter, and grammar tasks. Swaps, side effects, duplicate names, shadowing, public globals, module scope, and non-tuple expressions have explicit verification.
- Boot already has `ExprKind.Tuple`, while stage0 desugars tuples to `TupleN` record literals. The plan keeps that difference visible rather than assuming a shared AST shape.
- The largest implementation risk is preserving assignment-expression contexts while avoiding duplicate RHS lowering in the block/module special cases; Task 3 tests both paths.
- The existing tuple-let design intentionally excluded rebinding. This plan supersedes that exclusion only; it does not add mixed declaration/rebind patterns.
