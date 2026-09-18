# Tuple follow-ups — plan

Tracks the work deferred out of the case-arm tuple-destructuring effort
(shipped on `main`, see `docs/plans/archive/tuple-destructuring.md` and
`…-implementation.md`). Three independent bugs surfaced during that work but
were out of scope, one feature fast-follow was always planned, and a few
cosmetic/parity items remain. Each is independent; tackle in priority order.

## Priority summary

| # | Item | Kind | Priority | Own design needed? |
|---|------|------|----------|--------------------|
| B1 | Nested tuple literal fails check-mode | bug | High | no |
| A | `(a,b) :=` / `for` / param destructuring | feature | High | yes |
| B2 | Module-global sum scrutinee erases to anyref | bug | Medium | no |
| B3 | Byte tuple-field cast trap under re-match | bug | Low | no |
| C | Cosmetic / parity cleanups | chore | Low | no |

---

## B1 — Nested tuple *literal* fails to type-check in check-mode (boot)

**Symptom.** A nested tuple literal such as `((1, 2), 3)` does not type-check.
Flat literals (`(1, 2)`) and nested tuple *patterns* (`((a, b), c)`) both work;
only a nested literal in a checked position fails. This is unrelated to
destructuring — it is a gap in the original tuple-literal feature — but it is
the most user-facing of the three (anyone writing a nested tuple hits it), so
it is first.

**Root cause (suspected).** Tuple literals desugar to `TupleN` record literals.
The inner tuple is checked against an expected type via `check_record_lit`
(`boot/compiler/checker.tw:2772`) / `synth_tuple` (`checker.tw:2931`), which
needs a resolved `Named` type in check-mode; a bare nested literal doesn't
supply one for the inner element, so it fails. Confirm whether the fix belongs
in `synth_tuple` (synthesize the inner tuple, then unify) or in propagating the
expected element type down into the nested `check_record_lit` call.

**Scope.** Boot checker only (plus stage0 parity — stage0 desugars tuple
literals at parse time, so verify whether it has the same gap). No codegen
change expected; this is purely a type-checking/synthesis fix.

**Verification.** `case`/binding sites over nested literals type-check;
`((1,2),3)` synthesizes `((Int,Int),Int)`; the Task 2/3 tests that currently
work around this with annotated-parameter scrutinees can drop the workaround.

---

## A — `(a, b) :=` binding, `for (a, b) in …`, and parameter tuple patterns

**What.** The planned fast-follow to case-arm destructuring. Extends tuple
patterns to the other binding surfaces:

- `(q, r) := divmod(17, 5)` — the headline multi-return ergonomic.
- `for (a, b) in pairs { … }` — tuple-pattern loop binding.
- `fn f((a, b): (Int, Int)) { … }` — function-parameter tuple patterns.

**Why it's separate.** These need a real grammar edit beyond case arms
(widening the `LetStmt` LHS to accept a pattern, the `for`-binder, and
`Param` to accept a pattern) in **both** compilers. `:=` can largely desugar
onto the existing case-arm mechanism (all tuple patterns are irrefutable, so
no exhaustiveness concern), but the parser/AST/threading is non-trivial.

**Needs its own design + implementation plan** (brainstorm → spec → plan), the
same shape as the case-arm effort. Start from the "Fast-follow" section of
`docs/plans/archive/tuples.md`, which already sketches the `LetBinding`
widening. Do B1 first — nested literals are likely to show up in `:=` tests.

---

## B2 — `case` over a module-level global with a sum type erases to anyref (boot)

**Symptom.** Matching on a module-level global whose type is a sum/variant
(e.g. a top-level `Result<Int, String>` or `Option<T>` global) mis-lowers.
Reproduces with a plain sum global, independent of tuples — the tuple work
only surfaced it (tests sidestep it by using function-parameter scrutinees).

**Root cause.** `boot/compiler/backend/slot_assign.tw:646` maps
`atom_mono` for `.AGlobalLocal(_) => .Anyref_` — a module-global read is typed
as erased anyref rather than its real mono type, so a `case` scrutinee sourced
from a global loses the type needed for sum-layout dispatch. (The
`.AGlobalLocal` construction itself is at `slot_assign.tw:378-386`.)

**Scope.** Thread the global's real mono type through instead of erasing to
anyref. Check the module-global type oracle (see
`[[project_module_global_type_tracking]]`) for the resolved type of a global
id, and use it in `atom_mono` for `.AGlobalLocal`. Verify no regression in
existing global reads (functions/closures stored in globals).

**Verification.** `case` over a top-level `Result`/`Option`/enum global
dispatches correctly at runtime; add a boot integration test that today needs
a function-parameter workaround.

---

## B3 — Byte-typed tuple-field cast trap under nested rebind + re-match (stage0)

**Symptom.** A `Byte`-typed tuple field, bound then rebound and re-matched
against a literal pattern, traps at runtime with a cast failure. None of the
shipped scenarios use Byte tuple fields, so it did not block the feature.

**Root cause (suspected).** A typed-representation / i31-boxing precision issue
in stage0 codegen: the field mono for a `Byte` element loses precision through
the rebind, so `StructGet`/coercion (near `src/codegen/emit.rs:2077`
`emit_pattern_bindings`, `record_struct_sym` field projection at ~2110) emits a
cast that doesn't match the stored repr. Independent of the record-dispatch
path itself.

**Scope.** stage0 codegen only. Lowest priority — narrow trigger, no shipped
code path hits it. Confirm whether boot has the analogous issue.

**Verification.** A stage0 run-test with a `Byte` tuple field, rebound and
re-matched against a `Byte` literal, produces the correct value instead of
trapping.

---

## C — Cosmetic / parity cleanups (batch when convenient)

- **EBNF trailing comma.** `docs/grammar.ebnf` `TuplePattern` omits the optional
  trailing comma that `grammar.js` and both parsers accept; the sibling
  `TupleLiteral` rule documents it with `[ "," ]`. Align for accuracy.
- **Boot `lower_pattern` arity fallback.** Boot's `.Tuple` arm
  (`lower_core/patterns.tw`) lowers to `.Wildcard` on a `.None` tid (silently
  matching everything with no bindings); stage0 falls back to `TUPLE{2,3,4}` by
  arity and errors. Unreachable for well-typed programs, but give boot the same
  explicit error for symmetry/future-proofing.
- **Dead builder work.** Both bindings record branches build `StructGet` instrs
  for wildcard/ident fields that the recursion discards. Harmless; skip when the
  field binds nothing.
- **`slot_assign.tw` classifier dup.** Re-derives `is_record_mono` /
  `record_field_mono` instead of reusing `layout_helpers`' pair; a shared
  `pub(crate)`/module helper would remove the duplication (module-dependency
  direction currently justifies it).
- **CI note.** stage0 `tests/tuple_pattern_run_test.rs` requires `wasmtime` on
  PATH (fails loudly if absent) — ensure CI provides it or gate the test.

---

## Suggested sequencing

1. **B1** (nested tuple literals) — small, high-value, unblocks clean `:=` tests.
2. **A** (`:=` / `for` / param patterns) — its own design→plan cycle; the
   headline ergonomic. Do after B1.
3. **B2** (global sum scrutinee) — medium, self-contained backend fix.
4. **B3** + **C** — low priority; batch when touching the relevant files.
