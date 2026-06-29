# LSP completion — meaningful continuation at `LHS = .`

Status: Planned · Boot-only · 2026-06-29

## Goal

Limit completion candidates at a type-constrained position to **meaningful /
logical continuations** — candidates whose result type could actually typecheck
in that position. The concrete instance is the rebinding receiver shorthand:

```tw
xs: Vector<Int> = ...
xs = .<cursor>
```

Today the cursor sits after `=` then `.`, and the classifier routes `= .` to
the catch-all `.Variant` context, which lists *every* variant in scope (plus
`Option`/`Result`) — noisy and mostly wrong. We want `xs = .` to offer only the
continuations that produce `Vector<Int>`: methods whose return unifies with the
target type, plus variant constructors of that type.

## Principle

At a position whose type is fixed by context (the rebind target or the typed
binding's annotation in `LHS = .`), offer only candidates whose **result type
unifies with the expected type A**, restricted to the syntactic forms that are
legal at that position.

| Position | Expected type A | Legal forms |
|---|---|---|
| `a = .` (rebind) | type of `a` | methods returning ≈ A, variants of A |
| `a: T = .` (typed binding) | `T` | variants / literals of `T` (no method shorthand) |
| `state.items = .` (field-path rebind) | type of the field path | methods returning ≈ A, variants of A |

### Why "unifies with A", not "returns exactly A"

`xs = .map(cb)` desugars to `xs = xs.map(cb)`. `map` returns `Vector<B>`; the
rebind only typechecks if `Vector<B> = Vector<Int>`, i.e. the type system forces
`cb` to return `Int`. So `.map` **is** a valid continuation — the rebind doesn't
forbid it, it *constrains* the callback. The correct filter is therefore "the
method's return type can unify with A", treating the method's own generic params
as flexible:

- `.append(x)` → `Vector<Int>` — unifies → shown.
- `.map(cb)` → `Vector<B>` — `B` is the method's own generic ⇒ wildcard ⇒
  `Vector<_>` unifies with `Vector<Int>` → shown.
- `.slice(lo, hi)` → `Vector<Int>` — unifies → shown.
- `.len()` → `Int` — cannot unify with `Vector<Int>` → hidden.

### Why fields are excluded

The rebinding receiver shorthand fires only for `.lowercase(` — a method call
*with parens* (`parser.tw:2551-2556`, `2645-2648`). `a = .x` (bare field, no
paren) does **not** desugar to `a = a.x`; the parser treats `.x` as a normal
expression. Offering a plain record field would insert syntactically invalid
text, so fields are not candidates. Variant constructors (`.Some(x)`,
uppercase) reach the same `= .` cursor position through a different parser path
and *do* construct A directly, so they are kept.

## Design

### 1. Classification (`boot/compiler/query/cursor_context.tw`)

When `prev == .Dot` and the token before the dot is `.Eq`, scan backward over
the current statement to classify the assignment LHS:

- `Ident =` → rebind; expected type = type of that ident; forms = methods + variants.
- `Ident : <type> =` → typed binding; expected type = the annotation; forms = variants only.
- `Ident (. Ident)+ =` → field-path rebind; expected type = the field path; forms = methods + variants.

Emit a new context:

```tw
pub type RebindInfo = .{
  dot_offset: Int,
  target: RebindTarget,   // Ident(name) | FieldPath(Vector<String>) | Typed(name, TypeExpr)
  allow_methods: Bool,    // false for typed bindings
}

pub type CompletionContext = {
  Member(MemberContext),
  General(String),
  Import(ImportPrefix),
  Variant,
  RebindContinuation(RebindInfo),
}
```

If the back-scan cannot classify the LHS, fall back to today's `.Variant` so the
user is never blocked. This module stays syntax-only (token/text inspection,
no semantic lookup) per its existing contract.

### 2. Dispatch + expected-type resolution (`boot/compiler/query/completion.tw`)

Add a `.RebindContinuation(info)` arm to `complete()` calling a new
`rebind_continuation_completions(snap, info, source)`. It resolves A:

- `Ident(name)` → reuse `resolve_receiver_by_name` (name-based, edit-stable),
  then the existing type_map fallback.
- `FieldPath(segments)` → resolve the head ident's type, then walk fields using
  the same substitution logic as `completions_for_type`'s record-field branch.
- `Typed(_, type_expr)` → resolve the annotation `TypeExpr` to a `MonoType`
  through the resolved env.

If A cannot be resolved, fall back to unfiltered candidates (do not block).

### 3. Candidate gathering + filter

- Methods (only when `info.allow_methods`): gather via the existing
  `method_completions_for_type(env, A)`, then keep only entries whose registered
  signature's **return type unifies with A** (predicate below).
- Variants: gather via the existing `variants_for_type(env, A)`. These construct
  A directly, so they pass by construction; kept for all positions.
- Fields: excluded.

### 4. Unify predicate

```tw
fn return_unifies(env, ret_ty: MonoType, method_type_params, a_ty: MonoType) Bool
```

Substitute the method's own generic params in `ret_ty` with wildcards, then
structurally match against the ground `a_ty`:

- wildcard (a method generic) matches anything;
- `Named(tid, args)` vs `Named(tid, args')` — same `tid` and arity, args match pairwise;
- builtin shapes (`Vector`, `Optional`, `Result`, `Dict`, `String`, `Int`, …)
  matched structurally;
- otherwise false.

Examples: `Vector<B>` vs `Vector<Int>` → true; `Int` vs `Vector<Int>` → false.

This is a one-directional matchability check ("could the rebind typecheck"),
deliberately lighter than the checker's `InferCtx`-bound `unify`.

## Touch points

- `boot/compiler/query/cursor_context.tw` — new context + LHS back-scan.
- `boot/compiler/query/completion.tw` — dispatch arm, handler, filter, predicate.
- No change to `boot/lib/lsp/completion.tw` (item shape unchanged).
- No stage0 change (LSP is boot-only).

## Testing (`boot/tests/suites/lsp_completion_suite.tw`)

- `xs = .` on `Vector<Int>` → includes `append`, `map`, `slice`; excludes `len`.
- `o = .` on `Option<Int>` → includes `Some`, `None`.
- `a: T = .` (sum type `T`) → offers variants of `T`, no methods.
- `state.items = .` field-path target → resolves and filters like a plain rebind.
- Unresolved target type → falls back to unfiltered candidates.
- Plain record field never appears as a `= .` candidate.

Verify with `make boot-test`; exercise the live server with `make bundle-cli`
when validating end to end.
