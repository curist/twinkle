# Diagnose unreachable case arms

## Context

`case` expressions currently preserve source-order semantics at codegen time, but
the checker does not report arms that can never be selected. For example:

```tw
s := "foo"
x := case s {
  _ => 1,
  "foo" => 2,
}
```

This compiles today and evaluates to `1`. The `_` arm shadows the later
`"foo"` arm, but no diagnostic is emitted.

This is a correctness and tooling issue on its own, and it also matters for
future codegen optimizations such as string trie dispatch and variant
`br_table` dispatch. Those optimizations should still keep defensive eligibility
checks, but the language checker should reject obviously unreachable arms before
codegen.

## Goal

Emit an error when a `case` arm is definitely unreachable because previous arms
already cover every value the arm could match.

Initial scope:

- Catch-all arms (`_` and variable patterns) make all following arms unreachable.
- Duplicate literal arms are unreachable.
- Duplicate/covered variant arms are unreachable when an earlier arm for the
  same variant has irrefutable payload sub-patterns.

Non-goal: implementing a complete ML-style pattern usefulness algorithm in the
first pass. More precise reasoning over nested pattern matrices can be added
later.

## Current code path

Type checking for case expressions lives in `boot/compiler/checker.tw`:

```text
synth_case(...)
  -> synth_case_arm(...) / check_case_arm(...)
  -> check_exhaustiveness(...)

check_case(...)
  -> check_case_arm(...)
  -> check_exhaustiveness(...)
```

`check_exhaustiveness` currently only checks whether a match is missing variants
for enum-like scrutinees. It returns immediately if it sees any wildcard or
identifier arm, regardless of where that arm appears. It does not validate that
later arms are reachable.

## Design

### Diagnostic

Add a structured checker error, for example:

```tw
UnreachableCaseArm(.{ span: Span, covered_by: Span })
```

Render it as:

```text
unreachable case arm
```

with labels such as:

- primary: `this arm can never match`
- secondary: `covered by this earlier arm`

Files to update:

- `boot/lib/source/diagnostics.tw`
- `boot/compiler/query/diag_render.tw`
- `boot/compiler/query/diagnostics.tw`

### Coverage model

Track a conservative set of patterns already covered while scanning arms in
source order.

A later arm is unreachable if any earlier coverage item definitely covers it.

#### Catch-all coverage

`_` and identifier patterns are irrefutable for the scrutinee type. Once seen,
every following arm is unreachable.

```tw
case x {
  _ => a,
  .Some(v) => b, // unreachable
}
```

#### Literal coverage

For literal scrutinees (`Int`, `Bool`, `String`), a later literal with the same
value is unreachable.

```tw
case s {
  "foo" => a,
  "foo" => b, // unreachable
}
```

The check should compare literal values after pattern checking has already
ensured the literal type is compatible with the scrutinee.

#### Variant coverage

For enum-like scrutinees, a top-level variant arm covers later arms for the same
variant when all of its payload sub-patterns are irrefutable.

Examples:

```tw
case opt {
  .Some(x) => a,
  .Some(_) => b, // unreachable: previous .Some(x) covers all Some values
  .None => c,
}
```

```tw
case result {
  .Ok(_) => a,
  .Ok(1) => b, // unreachable: previous .Ok(_) covers all Ok values
  .Err(e) => c,
}
```

A variant arm with non-irrefutable payload sub-patterns does not cover the whole
variant:

```tw
case result {
  .Ok(1) => a,
  .Ok(_) => b, // reachable
  .Err(e) => c,
}
```

For the first pass, `pattern_is_irrefutable` can be conservative:

- `_` and identifiers are irrefutable.
- A variant payload sub-pattern is irrefutable for that field only if it is `_`,
  an identifier, or a recursively irrefutable pattern for the field type.
- Literal patterns are not irrefutable.
- Unknown/error patterns are not treated as covering anything.

### Ordering with exhaustiveness

Run unreachable-arm checking in addition to exhaustiveness checking. Suggested
order:

1. Type-check/check each arm pattern and body as today.
2. Run `check_unreachable_arms(scrut_ty, arms, ctx, diags)`.
3. Run `check_exhaustiveness(scrut_ty, arms, case_span, ctx, diags)`.

This keeps the new diagnostic independent from missing-variant reporting.

If a case contains earlier pattern errors, the unreachable check should be
conservative and avoid cascading diagnostics from `.ErrorPattern` or invalid
variant/literal patterns.

## Implementation sketch

Add helpers in `checker.tw`:

```tw
fn check_unreachable_arms(
  scrut_ty: MonoType,
  arms: Vector<CaseArm>,
  ctx: InferCtx,
  diags: Vector<DiagKind>,
) Vector<DiagKind>
```

Internal helpers:

```tw
type Coverage = {
  CatchAll(Span),
  Literal(String, Span),
  Variant(String, Span),
}

fn coverage_from_pattern(pat: Pattern, scrut_ty: MonoType, ctx: InferCtx) Coverage?
fn coverage_covers(cov: Coverage, pat: Pattern, scrut_ty: MonoType, ctx: InferCtx) Bool
fn literal_key(pat: Pattern) String?
fn top_variant_name_if_valid(pat: Pattern, scrut_ty: MonoType, ctx: InferCtx) String?
fn pattern_is_irrefutable(pat: Pattern, expected: MonoType, ctx: InferCtx) Bool
```

Notes:

- Use existing qualifier validation logic for qualified variants so
  `Other.A` does not count as covering the scrutinee's `.A`.
- Literal keys should include kind to avoid collisions, e.g. `str:foo`,
  `int:1`, `bool:true`.
- Once a catch-all coverage item is seen, every later non-error arm can be
  diagnosed as unreachable.
- Continue scanning after reporting an unreachable arm, but do not add coverage
  from unreachable arms. This avoids later diagnostics being attributed to an
  arm that itself can never run.

## Tests

Add checker/compiler tests covering:

- Wildcard before string literal is rejected.
- Variable catch-all before a later arm is rejected.
- Duplicate string literal is rejected.
- Duplicate int and bool literals are rejected.
- Duplicate nullary variant is rejected.
- Variant with irrefutable payload covers later same-variant arms.
- Variant with literal/nested restrictive payload does not cover later broader
  same-variant arm.
- Catch-all as the final arm remains valid.
- Exhaustive enum match with no unreachable arms remains valid.

Also include a regression for current behavior:

```tw
case "foo" {
  _ => 1,
  "foo" => 2,
}
```

This should now fail during checking instead of compiling and printing `1`.

## Interaction with dispatch optimizations

Even after this checker fix lands, optimized dispatch codegen should keep its
own conservative eligibility checks:

- string trie dispatch should require the catch-all arm to be last and reject
  duplicate literals;
- variant `br_table` dispatch should require the catch-all arm to be last and
  reject duplicate top-level tags for the first implementation.

Those checks protect codegen from malformed or future IR and preserve semantics
if diagnostics are downgraded or bypassed in tooling modes.

## Future work

- Implement a full pattern usefulness/exhaustiveness algorithm over pattern
  matrices. This would catch subtler unreachable arms, such as a broad arm made
  redundant by several earlier nested arms that collectively cover the space.
- Improve exhaustiveness for literal matches beyond booleans, where practical.
- Consider warning vs error policy if the language wants to allow unreachable
  arms during exploratory editing. For compiler builds, unreachable arms should
  be treated as errors.
