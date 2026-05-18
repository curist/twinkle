# Refactoring Patterns

This note records small compiler-code refactoring patterns that emerged while simplifying `boot/compiler/query/hover.tw`. They are useful when cleaning other boot compiler query and analysis modules.

## Prefer `try` for required Options

When a helper returns `T?` and a missing intermediate value means the helper should also return `None`, use `try` instead of a `case` unwrap.

```tw
value := try maybe_value
```

Use this for lookup chains, optional AST context, and type-map access where there is no useful fallback in the current helper.

## Do not use `try` for probe-and-continue

When `None` means “try the next candidate”, `try` is wrong because it exits the current helper immediately.

For those cases, use a first-success helper or an explicit fallback chain.

```tw
result := candidate_a()
if result.is_some() { return result }

candidate_b()
```

## Use `find_map` for first-success loops

Many query functions scan child nodes and return the first hover/definition/completion result. A local helper removes repetitive `case .Some/.None` boilerplate.

```tw
fn find_map<A, B>(xs: Vector<A>, f: fn(A) B?) B? {
  for x in xs {
    v := f(x)
    if v.is_some() { return v }
  }
  .None
}
```

For indexed scans:

```tw
fn find_map_indexed<A, B>(xs: Vector<A>, f: fn(A, Int) B?) B? {
  for x, i in xs {
    v := f(x, i)
    if v.is_some() { return v }
  }
  .None
}
```

This keeps traversal intent visible without destructuring and re-wrapping the successful result.

## Use `.or_else` for short fallback chains

For a small number of fallbacks, `.or_else` reads well.

```tw
hover_a()
  .or_else(fn() { hover_b() })
  .or_else(fn() { hover_c() })
```

Avoid deeply nested closure-heavy chains if they make capture behavior or diagnostics harder to understand. In those cases, split the alternatives into named helpers or use explicit early returns.

## Split probes into small helpers

Large `case` arms become easier to read when each probe gets its own helper:

```tw
fn hover_stmt_special(stmt: Stmt, offset: Int, typed: CheckResult) HoverResult? {
  case stmt {
    .Let(ls) => hover_let_stmt(ls, offset, typed),
    .For(fs) => hover_for_stmt(fs, offset, typed),
    _ => .None,
  }
}
```

Small helpers make it clearer whether `None` means “not applicable”, “not found”, or “fall through to the next strategy”.

## Omit redundant tail-position record type names

When the expected return type is known, prefer inferred record literals in tail position.

```tw
.Some(.{ content, span: expr.span })
```

instead of:

```tw
.Some(HoverResult.{ content, span: expr.span })
```

Keep named constructors when there is no useful expected type or when explicitness improves readability.

## Apply carefully

These patterns are most useful in query-style code: hover, definition, completion, and AST walkers. Preserve explicit pattern matching when the inner value is needed for branching or when matching domain variants is the clearest expression of the logic.
