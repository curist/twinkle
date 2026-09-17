# Tuple destructuring in `case` arms — design

## Summary

Add tuple **patterns** to `case` arms in both compilers, e.g.

```tw
case pt { (x, y) => x + y }
case lookup(k) { .Some((key, val)) => use(key, val), .None => dflt }
case pair { (0, y) => y, (x, _) => x }   // refutable positions allowed
```

This is the fast-follow the first-class-tuples work called out (see
`docs/plans/archive/tuples.md`, "Fast-follow"). Tuples already exist as literals
(`(a, b)`), types (`(A, B)`), and positional access (`t._0`); this adds the one
real grammar edit for patterns so multi-return values can be unpacked.

Both stage0 and the boot compiler gain full support in this work, so boot
compiler source is *ready* to adopt tuple destructuring on demand — but this
change does not migrate any boot source (adoption is a separate follow-up).

## Scope

**In scope (v1):**

* Tuple patterns in `case` arms, arity 2–4 (mirrors tuple literals/types).
* **Full nesting**: any pattern at each tuple position — idents, wildcards,
  literals, variants, and sub-tuples — and tuple patterns nested inside variant
  patterns (`.Some((k, v))`). This rides the existing nested-refutable-pattern
  machinery (see "Codegen" below), which already composes sub-pattern conditions
  and provides arm fallthrough.
* Both compilers (boot + stage0) in parallel.

**Out of scope (deferred / unchanged from the archived tuples plan):**

* `(a, b) := …` let-binding destructuring, `for (a, b) in …` loop patterns, and
  function-parameter patterns. (The headline `(q, r) := divmod(…)` ergonomic
  therefore is not available yet; multi-return unpacking is written as
  `case divmod(…) { (q, r) => … }`. `:=` can later desugar onto this mechanism.)
* Migrating boot compiler source to use tuple destructuring.
* Arity > 4, 1-tuples / unit, `.first`/`.second` accessors, variadic-generic
  tuple operations.
* Full product-coverage exhaustiveness (see "Exhaustiveness" for the
  conservative v1 rule).

## Background: what already exists

Tuples desugar to compiler-known nominal **records** `Tuple2`/`Tuple3`/`Tuple4`
with positional fields `_0.._3`:

* Boot registers them as `.Record` type defs in `base_env.tw`
  (`builtin_tuple_type_entries`); stage0 registers `TypeId(13..15)` in
  `src/types/env.rs`.
* `TypeExpr.Tuple` / `ExprKind.Tuple` (boot) and parse-time desugar (stage0) are
  already in place for literals and types.

Pattern infrastructure to extend:

* **Boot:** `PatternKind` (`ast.tw`), `parse_pattern`/`parse_pattern_list`
  (`parser.tw`), `check_pattern` + `check_exhaustiveness` +
  `check_unreachable_arms` (`checker.tw`), `lower_pattern`
  (`lower_core/patterns.tw`), and the match emitter (`codegen/emit.tw` +
  `codegen/emit/match.tw`). `fmt/printer.tw` renders patterns.
* **Stage0:** `Pattern` (`src/syntax/ast.rs`), the pattern parser
  (`src/syntax/parser.rs`), `PatternChecker` (`src/types/patterns.rs`),
  pattern lowering, and codegen.

**Key fact confirmed during design:** nested *refutable* patterns already work.
`emit_variant_pattern_condition` recurses into non-trivial sub-patterns and
`combine_and_checks` ANDs them; `emit_arm_chain` is a linear `if cond { … }`
chain that provides "no match → next arm" fallthrough. So full nesting for
tuples requires no new decision-tree capability — only a **record-layout** path
wherever the variant emit code assumes a **sum layout** (tag + payload offset).

## Representation decision: reuse the variant Core pattern

Tuple patterns lower to the existing single-constructor variant Core pattern
rather than introducing a new Core IR node:

* Boot `lower_pattern` emits
  `CorePattern.Variant(tuple_tid, VariantId{ id: 0 }, lowered_subs)`.
* Stage0 lowering emits `CorePattern::Variant { type_id: tuple_tid,
  variant: vid 0, fields }`.

Because there is **no new `CorePattern` kind**, the many Core-pattern consumers
(ANF, optimizer passes, `ir_print`, `cfg`, linker, etc.) handle tuple patterns
through their existing `.Variant` arms unchanged. "Tuple-ness" is confined to
three places: the parser/AST (a dedicated surface pattern node), the checker's
exhaustiveness/type rules, and the codegen layout/tag handling.

At the **surface AST**, both compilers get a dedicated node:

* Boot: `PatternKind.Tuple(Vector<Pattern>)`.
* Stage0: `Pattern::Tuple(Vec<Pattern>, Span)`.

Stage0 uses a real node rather than parse-desugaring into `Pattern::Variant`
(as it does for tuple literals/types). Records are not variants, so routing a
record-destructure through the variant machinery — which resolves on a variant
*name* — is semantically wrong; a dedicated node keeps variant resolution clean.
This is a deliberate deviation from stage0's literal/type desugar style.

## Design by pipeline stage

The same shape applies to both compilers unless noted.

### 1. Parser + AST

* Add the surface node (`PatternKind.Tuple` / `Pattern::Tuple`).
* Extend the pattern parser with a `(`-case: parse a comma-separated pattern
  list terminated by `)` (boot reuses `parse_pattern_list(RParen, …)`), require
  arity 2–4. A single parenthesized pattern `(p)` with no top-level comma is a
  parse error (no 1-tuples), mirroring the literal rule; reuse the arity
  diagnostics already present for tuple literals/types
  (`parser.tw:896`/`2225`, `src/syntax/parser.rs:1483`).

### 2. Type checking

* `check_pattern` gains a tuple case: zonk the expected type, unify with the
  `TupleN` named type of matching arity, take element types from the type args,
  and recurse `check_pattern` on each element against its element type.
* Arity/shape mismatch (e.g. a 3-tuple pattern against a `Tuple2` scrutinee, or
  a tuple pattern against a non-tuple scrutinee) produces a clear diagnostic
  (reuse the existing arity/variant-mismatch diagnostics or add a
  tuple-specific one).

### 3. Exhaustiveness and reachability

Conservative v1 rule — a tuple is a single-constructor product, so coverage is
driven by refutability, not by variant enumeration:

* Extend `pattern_is_irrefutable` to recurse: a tuple pattern is irrefutable iff
  **all** its element patterns are irrefutable.
* `check_exhaustiveness`'s "does any arm cover everything" early-out switches
  from Wildcard/Ident-only to `pattern_is_irrefutable`. A fully-irrefutable
  tuple arm (e.g. `(a, b)`, `(_, x)`) therefore counts as covering-all.
* A `case` over a tuple whose arms are all refutable and with no catch-all is
  reported **non-exhaustive** (the user adds `_`). Proving that a set of
  refutable tuple arms is total (full product coverage) is **deferred**.
* `coverage_from_pattern` (unreachable-arm detection): an irrefutable tuple →
  `CatchAll`; a refutable tuple → no coverage entry (mirrors how variant
  patterns with non-trivial sub-patterns are treated).

Stage0's `PatternChecker` mirrors the same rule.

### 4. Lowering

As above: tuple pattern → `CorePattern::Variant{ tuple_tid, vid 0, lowered
element patterns }`, with element types resolved from the tuple type's args.

### 5. Codegen

Tuples are records (no variant tag) but reuse the variant Core pattern, so the
variant emit paths are gated on "is this `type_id` a record type?":

* `emit_pattern_condition` / `emit_variant_pattern_condition`: for a record tid,
  emit the AND of the sub-pattern conditions over record field projections
  (`StructGet(record_sym, i)`) with **no** tag test; emit `I32Const(1)` when all
  sub-patterns are trivial. This mirrors the existing
  `is_nullable_extern_option` special-case already in these functions.
* `emit_pattern_bindings`: for a record tid, project field `i` via the record
  layout (fields at offset `i`), **not** the sum-layout tag offset, then recurse.
* `is_br_table_eligible`: record tids are ineligible (no tag to switch on),
  forcing the `emit_arm_chain` path, which already handles nested refutable
  patterns and fallthrough.

Stage0 codegen applies the analogous record-layout gating. Stage0 already emits
record field reads (tuple `._0` access works), so the record layout is available.

### 6. Formatter

`fmt/printer.tw` renders `PatternKind.Tuple` as `(a, b)` (round-trip stable).

## Testing

* **Parser** (`parser_suite`, stage0 syntax tests): tuple pattern parse for
  arities 2–4; arity-0/1/>4 errors.
* **Checker**: element type mismatch; non-tuple scrutinee mismatch;
  non-exhaustive-needs-catch-all for refutable tuple arms; unreachable-arm
  detection for a redundant arm after an irrefutable tuple.
* **Codegen** (emit/match suite, stage0 equivalents): irrefutable binding
  (`(a, b) => a + b`); nested variant-holding-tuple (`.Some((k, v))`); refutable
  positions with fallthrough (`(0, y) => …, (x, _) => …`); nested sub-tuple
  (`((a, b), c) => …`).
* **Formatter**: round-trip for tuple patterns.
* Full boot test suite green; `make stage2` reaches a self-host fixed point.

## Non-goals recap

Deferred to later work, not addressed here: `:=`/`for`/parameter tuple patterns,
boot-source adoption, full product-coverage exhaustiveness, arity > 4, and any
`Vector.pop`/`drop` API revision (noted: the tuple-returning `Vector.pop` has no
boot-source call sites today, so it can be revised freely; `drop_last`/
`drop_first` are load-bearing and should be left alone).
