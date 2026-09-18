# Tuple follow-ups — plan

Tracks the work deferred out of the case-arm tuple-destructuring effort
(shipped on `main`, see `docs/plans/archive/tuple-destructuring.md` and
`…-implementation.md`). Three independent bugs surfaced during that work but
were out of scope, one feature fast-follow was always planned, and a few
cosmetic/parity items remain. Each is independent; tackle in priority order.

## Priority summary

| # | Item | Kind | Priority | Own design needed? |
|---|------|------|----------|--------------------|
| ~~B1~~ | ~~Nested tuple literal fails synth-mode~~ **DONE** | bug | High | no |
| A | `(a,b) :=` / `for` / param destructuring | feature | High | yes |
| ~~B2~~ | ~~Module-global scrutinee erases to anyref~~ **DONE** | bug | Medium | no |
| B3 | Byte tuple-field cast trap under re-match | bug | Low | no |
| C | Cosmetic / parity cleanups | chore | Low | no |

---

## B1 — Nested tuple *literal* fails in synth mode (boot) — **DONE**

**Symptom.** A nested tuple literal such as `((1, 2), 3)` fails to type-check
in **synth mode** (`t := ((1, 2), 3)`, no annotation). Note: the original
"check-mode" framing was wrong — an *annotated* binding
(`t: ((Int, Int), Int) = ((1, 2), 3)`) always worked; only the no-expected-type
synth path failed.

**Root cause (confirmed).** `synth_tuple` → `synth_named_record` builds
`expected = Named(TupleN, [meta, meta])` and checks each element via
`check_expr(elem, field_ty)` where `field_ty` is an unresolved metavar. The
`.Tuple` check-mode arm (`checker.tw:2983`) unconditionally called
`check_record_lit` with that metavar expected; `check_record_lit` zonks it,
finds a non-`Named` type, and errors `NotARecord` ("expected a record type,
got `?N`"). The inner literal was never given the chance to synthesize itself.

**Fix.** Guard the `.Tuple` check-mode arm on the zonked expected being a
resolved `.Named` record (mirroring the `.Array`/`.Collect` arms); a bare
metavar falls through to the generic synth+unify fallback, so the inner literal
synthesizes `(Int, Int)` and unifies with the field metavar.

**Stage0 parity.** No gap — stage0 desugars tuple literals to nested `TupleN`
constructor calls at parse time, which produce named types directly, so
synthesis already works. Verified via `twk check`/`build`.

**Verified.** Boot checker test added
(`checker_suite.tw` "nested tuple literal synthesizes in synth mode");
self-host fixed point green; end-to-end `((1,2),3)` and `(1,(2,3))` run.

**Note — surfaced B2.** Destructuring a *top-level (module-global)* tuple
(`t := (1,2)` then `case t { (a,b) => … }`) traps at codegen
(`local.set expected anyref, found struct.get i64`). This is B2 (global type
erasure), pre-existing and independent of B1, and it reproduces with tuple/record
globals too — not only sum globals. In-function scrutinees work.

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

## B2 — `case` over a module-level global scrutinee erases to anyref (boot) — **DONE**

**Symptom.** Matching directly on a module-level global mis-lowers. Reproduced
with a **tuple/record global** (`t := (1,2)` then `case t { (a,b) => … }`),
which trapped at Wasm validation (`local.set expected anyref, found struct.get
i64`; the boot backend verifier flags it as "binary lhs has incompatible repr
OpaqueAnyref for Int op"). Also reproduced with a **sum global**
(`r: Result<Int,String> = .Ok(42)` then `case r { … }`). Not sum-specific — any
typed global scrutinee whose fields carry unboxed valtypes hits it. In-function
scrutinees were unaffected.

**Root cause (confirmed).** `atom_mono` (`slot_assign.tw`) mapped
`.AGlobalLocal(_) => .Anyref_`, so a `case` scrutinee sourced from a global lost
its real mono. `collect_binding_monos` then saw a non-record/non-sum anyref,
bound no field monos, and the pattern-bound field locals defaulted to anyref
slots — while emission projected them with `struct.get` of the real (i64) field
type.

**Fix.** Thread the module's `AnfModule.global_monos` oracle (see
`[[project_module_global_type_tracking]]`) from `prepare_backend` down through
`assign_slots_for_module` → `assign_slots` → `collect_pattern_monos*` into
`atom_mono`, and resolve `.AGlobalLocal(gid)` to `global_monos[gid.id]` (falling
back to anyref when absent, e.g. function/closure globals). No regression: only
match-scrutinee mono resolution consults `atom_mono`, and non-record/non-sum
globals bind nothing as before.

**Stage0 parity.** No analogous bug — stage0 wraps top-level statements in a
function (so `t` is a local, not a global) *and* boxes tuple int fields
(`BoxedInt`), so its pattern-field slots are uniformly anyref. Verified via
stage0 WAT.

**Verified.** Boot run-based tests over tuple, nested-tuple, and Result globals
(`codegen_integration_suite.tw`); self-host fixed point green; end-to-end runs
of tuple/nested/Result/Option globals.

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
