# Checker Semantics

Decisions frozen here exist so later cleanups do not accidentally change them.

## `Never` unification

`Never` unifies with any type.  A branch arm or case arm whose checked type is
`Never` does not constrain the join type of the surrounding expression.  This
applies whether the divergence is spelled literally (`error(...)`) or reached
through any other known `Never`-returning function.

## Field lookup vs method-value fallback

Real record fields take priority over the method-value fallback path.  If a
type has a field named `f` and a method also named `f`, `x.f` resolves to the
field, not to a bound method value.

## Alias expansion boundaries

Type aliases are expanded at selected checker boundaries rather than
universally.  Expansion happens when:

- a type is zonked and compared against a structural pattern (e.g. `Function`,
  `Named`, `Optional`),
- a record or variant literal is being checked against an expected type,
- method lookup needs the concrete head of a type.

Aliases are NOT expanded before storing types in the type map or returning
synthesis results.  This keeps error messages readable and avoids infinite
expansion of recursive aliases.

## Top-level function inference is source-order sensitive

Unannotated top-level functions are inferred in source order.  A call to a
function that appears later in the file may fail to infer if the callee's
signature depends on information not yet available.  Adding a return-type
annotation on either the callee or the call site resolves this.

## `pre_unify_return` is best-effort and silent

Before checking call arguments, the checker attempts to solve MetaVars
introduced by instantiation by unifying the instantiated return type with the
expected type at the call site.  Any mismatch diagnostics from this step are
discarded intentionally — the real error is reported later by the outer
call-site check or the final zonk pass.  This matches the stage0 policy.

## Explicit closure return annotations are enforced in check mode

When a closure is checked against an expected function type and the closure
carries an explicit return annotation, that annotation is unified with the
expected return type.  A conflict is a type error — the annotation is never
silently ignored.

## Ambiguous-type diagnostic deduplication

The final zonk pass deduplicates "cannot infer type" diagnostics by source
position.  Multiple `type_map` entries that share the same span emit at most
one diagnostic, preventing a single ambiguous root from cascading into many
identical reports.
