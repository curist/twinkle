# Boolean negation spelling decision

Status: **Rejected** — keep prefix `!` as the canonical boolean negation syntax.

## Decision

Twinkle will keep the current boolean operator surface:

```tw
!ready
ready and enabled
ready or forced
!(ready and enabled)
```

We considered migrating boolean negation to word syntax:

```tw
not ready
ready and enabled
ready or forced
not (ready and enabled)
```

The migration is rejected for now. The current mix is intentional: `and` and
`or` are binary logical connectors, while `!` is a tight unary inversion
operator. These roles read differently, and they benefit from different surface
syntax.

## Rationale

### Binary boolean operators connect propositions

`and` and `or` sit between two boolean expressions. In word form they read like
logical connectors between clauses:

```tw
is_ready and has_work
is_admin or is_owner
```

That fits Twinkle's preference for readable, low-ceremony source. The word forms
make compound boolean conditions scan as propositions instead of punctuation-heavy
operator chains.

### Unary negation is a tight prefix operation

Negation applies to a single following expression. The symbolic form has useful
visual binding:

```tw
!done
!(is_ready and has_work)
```

For grouped expressions, this is especially compact and clear: the `!` attaches
directly to the group being inverted. The word form is readable, but it feels
less visually attached:

```tw
not (is_ready and has_work)
```

The symbolic form also avoids the at-a-glance ambiguity readers may feel with a
sequence like:

```tw
not ready and enabled
```

Even with well-defined precedence, the spaced word prefix can read like prose and
invite a moment of hesitation. The tight prefix form makes the immediate operand
relationship more obvious:

```tw
!ready and enabled
!(ready and enabled)
```

## Non-goals

- Do not add `not` as an alias at this time.
- Do not replace `and`/`or` with `&&`/`||`.
- Do not change `!=` equality syntax.
- Do not change result-type shorthand syntax (`T!E`, `T?!E`, or leading `!E`).
- Do not change the IR operation names or boolean semantics; the operation may
  remain named `Not` internally.

## Historical migration sketch

If this decision is revisited, the safe transition would be two phases:

1. Add `not` support to Rust stage0 and the boot compiler while keeping prefix
   `!` accepted, then teach the formatter to print boolean negation as `not`.
2. After a transition window, reject prefix `!` only in expression-negation
   position while preserving `!=` and result-type shorthand.

That migration remains mechanically straightforward because boolean negation is
already represented semantically as `UnOp.Not`; the complexity is mostly
bootstrap ordering, formatter behavior, docs, and tooling. It is not the chosen
path unless this spelling decision changes.
