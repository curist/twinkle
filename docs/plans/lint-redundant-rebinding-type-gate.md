# Type-aware auto-fix for `redundant-rebinding` computed advances

**Status: design (approved, pre-plan).** Extends item **B** of
[lint-rebinding-fixers.md](lint-rebinding-fixers.md) (`redundant-rebinding`).
Supersedes the report-only restriction on computed advances with inferred
bindings.

## Goal

Make the **computed-advance** sub-case of `redundant-rebinding` auto-fixable
for **type-inferred (`:=`) bindings**, not just ones with byte-identical
explicit annotations.

A computed advance is `acc2 := step(acc)` followed by forward uses of `acc2`
while `acc` goes dead — the fixer rewrites it to `acc = step(acc)` and rethreads
`acc`. Rebinding `acc` in place is only type-safe when the advance's result type
equals `acc`'s type. Today the only proof the linter can make is syntactic:
both bindings carry **byte-identical source annotations** (the `ann_ok` gate in
`redundant_rebinding_edits`). Inferred `:=` bindings have no annotation to
compare, so they stay report-only — which is every real site in the boot
source.

The counterexample that makes the gate necessary:

```tw
acc := 0                  // Int
acc2 := to_string(acc)    // String — reads acc, returns a different type
consume(acc2)
```

`acc2` is a numbered sibling of `acc` and its RHS reads `acc`, so the rule flags
it — but rewriting to `acc = to_string(acc)` rebinds `acc` from `Int` to
`String`, a type error. Any relaxation MUST keep declining this.

## Mechanism — reuse the checker's existing inferred-type side-table

No new inference machinery. The checker already publishes the table we need:

- `CheckResult.type_map: Dict<Int, MonoType>` (`boot/compiler/checker.tw`),
  keyed by `Expr.id` (`boot/compiler/ast.tw`: `Expr = .{ id, kind, span }`).
- It is written during `synth`/`check` (`set_type_map_span(expr.id, …, ty)`)
  and **zonked** in the checker's `finalize` sweep, so it holds fully-resolved
  `MonoType`s, not raw meta-vars.
- It already rides the per-module `checked` result and is consumed in the same
  `analyze_module` pass that runs the linter — so it is always in sync with the
  AST being linted (no incremental-cache staleness).

The linter already receives `checked.env`; we additionally hand it
`checked.type_map` and look up the two bindings' result types.

## Threading (touch points)

`type_map` follows the exact path `source` already travels:

1. `lint_module(module, env, source)` → add a `type_map: Dict<Int, MonoType>`
   parameter. Call site in `boot/compiler/query/analyze.tw` passes
   `checked.type_map`.
2. `LintCtx` gains a `type_map` field; both constructors set it — the
   `empty_lint_ctx` helper (top-level statements) and the inline record literal
   built per function body in `lint_module`.
3. `lint_redundant_rebinding(block, ctx.source)` → also pass `ctx.type_map`.
4. `shape_one_redundant_finding` and `redundant_rebinding_edits` → receive
   `type_map`.

Shape 2 (loop-seed) and the pure-alias case are untouched — a pure alias
(`foo := bar`) trivially shares its base's type and needs no gate.

## The type gate (replaces `ann_ok`)

In `redundant_rebinding_edits`, for a computed advance (non-alias RHS):

- `temp_ty = type_map[ls.value.id]` — the RHS `acc2` is bound to.
- `base_ty = type_map[base_decl.value.id]` — `acc`'s binding RHS.
- Auto-fixable iff `mono_eq(temp_ty, base_ty)`.

This single check subsumes the old annotation comparison: an annotated binding
has an inferred type in `type_map` too, so the "matching annotations" case keeps
working without a separate code path.

**Fail-closed guards** — stay report-only (empty `edits`), never emit an
unproven rewrite — when any of:

- either `Expr.id` is absent from `type_map`;
- either looked-up type is `MonoType.ErrorType`;
- either type `contains_meta` (an unresolved meta-var slipped past zonk).

The existing comment/prefix guard (decline when the binding prefix contains
`//`) stays.

## Two orthogonal gates (unchanged framing)

- **Sibling-name heuristic** (`acc`→`acc2`, closed marker set) decides *whether
  to flag* a computed advance at all — ceremony detection. Unchanged.
- **Type-equality** decides *whether it is safe to auto-fix*. This is the only
  thing this change touches.

A meaningful rename (`items` → `sorted`) is still not a sibling and is never
flagged; a sibling whose advance changes the type is flagged but declines the
fix.

## `mono_eq` sourcing

A complete structural `mono_eq(a: MonoType, b: MonoType) Bool` already exists
and is `pub` (`boot/compiler/backend/verify_common.tw`), with a second private
copy in `closure_convert.tw`. A linter importing from `backend/` is a layering
smell.

**Plan:** relocate the canonical `mono_eq`/`mono_vec_eq` to
`compiler.mono_type` (beside `ty_to_string`, the type's natural home); have
`verify_common` delegate to it so its public name and callers are unaffected;
import it into `lint.tw` from `compiler.mono_type`. Diff the two existing copies
first and only consolidate if byte-identical in behavior. `closure_convert`'s
private copy is out of scope.

## Tests (TDD, `boot/tests/suites/lint_pass_suite.tw`)

- **Unlocks (RED→GREEN):** `acc := 0` / `acc2 := step(acc)` / `consume(acc2)`
  with `step: Int → Int` now reports `edits.len() > 0`; the applied result
  threads `acc` and drops `acc2`.
- **Soundness guard:** `acc := 0` / `acc2 := to_string(acc)` / `consume(acc2)`
  still fires as a finding but **declines** (report-only, empty edits) because
  `Int ≠ String`.
- **Annotated regression:** the existing "rewrites a computed candidate with
  matching annotations" test stays green via the same type path.
- **Fail-closed:** a candidate whose inferred type is unresolved/error stays
  report-only.

## Verification

`make fmt` on touched files → full `make boot-test` → `make stage2` self-host
fixed point (this is analysis-only and MUST NOT change codegen) → dogfood
`twk lint` on boot. Boot-only; no Rust/stage0 change expected.

## Risks / non-goals

- **Annotated-widening edge** (e.g. `x: Anyref = foo()`): the base type is read
  from `type_map[base_decl.value.id]`. Under check-mode the value's id should
  record the pushed-down expected (annotation) type; verify during
  implementation, and rely on the fail-closed guards otherwise. The unlocked
  target case — inferred `:=` — is always exact (the bound type *is* the value's
  synthesized type).
- **Non-goals:** the sibling-name heuristic, Shape 2 (loop-seed),
  `closure_convert`'s private `mono_eq`.
