# Inherent-method-call rewrite (`twk lint` R1)

> **Status: archived; implemented as `twk lint` rewrite R1 (`inherent-method-call`).**
> This is the detailed design for the rewrite catalogued in
> [`linter.md`](linter.md). It does **not** ship standalone: it rides the review
> command's review-mode + rewrite sink. The trigger predicate, emission sites,
> and edits below are the rule's substance; **surfacing follows the review
> model** — reported by `twk lint`, applied by `twk lint --fix-inherent-calls`
> (or `--fix`), off the `build`/`check` path — so this doc does not re-litigate it
> (see `linter.md` → "Command surface").
>
> This is an auto-fixable *rewrite*, not a report-only lint, because it is
> provably meaning-preserving: correct code respelled, not a suspected bug (the
> auto-fixability rule in [`linter.md`](linter.md) → Motivation).

## Goal

Rewrite a call written in **free-function form** into receiver-method (inherent)
form when both provably resolve to the *same* function. Reported by `twk lint`
and applied by `twk lint --fix-inherent-calls` (or `--fix`). Never on the
`build`/`check` path.

```tw
Vector.map(xs, f)          // → xs.map(f)
point.translate(p, 1, 2)   // → p.translate(1, 2)
translate(p, 1, 2)         // → p.translate(1, 2)   (bare, same-module)
```

This is a boot-compiler-only feature. The Rust stage0 compiler needs no changes
beyond compiling the new (plain Twinkle) source.

## Scope

Covered free-function syntaxes:

- **Builtin type-qualified** — `Vector.map`, `Dict.has`, `String.len`, etc.
  These work because builtin type names are registered as module aliases
  (`collect_module_aliases`), so they flow through
  `try_synth_module_qualified_call`.
- **User module-qualified** — `point.translate(p, ...)`, where the module owns
  the receiver type and `translate` is one of its inherent methods.
- **Bare same-module call** — `translate(p, ...)`, resolved via the
  `.Ident(name)` → `lookup_function` path.

Out of scope: calls where the inherent form would *not* resolve to the same
function (e.g. `println(x)`), and calls already in receiver form (`xs.map(f)`).

## Trigger predicate (unified)

At each call-resolution site, after `inst := instantiate(sig, ctx)`, with a
candidate method name `M`:

- `M` is the `.field` name for qualified calls, or the ident for bare calls.
- `args.len() >= 1` and `inst.params.len() >= 1`.
- Let `T` be the head shape of `inst.params[0]` (the declared receiver type).
  Metavars do not need to be solved first — `resolve_method_func_name` matches
  on the head constructor (`.Vector(_)`, `.Named(tid, _)`, `.String`, …), which
  `instantiate` preserves.
- `resolve_method_func_name(T, M) == Some(fn_name)`, where `fn_name` is the
  function the call actually resolved to.
- **`args[0]` is postfix-atomic** — its `kind` is one of `Ident`, `Field`
  (field access / method chain), `Call`, index, or a literal. See "Receiver
  must be postfix-atomic" below.

When all hold, the inherent form `args[0].M(args[1..])` provably resolves to the
same function, so the rewrite is safe. The predicate fails closed: if the
registry stores a different canonical name, no rewrite is emitted.

### Receiver must be postfix-atomic

`.` is a postfix operator and binds tighter than every binary and prefix
operator, so naively reordering `Callee(arg0, …)` → `arg0.M(…)` changes meaning
when `arg0` is not already a postfix-atomic expression:

```
Vector.map(a or b, f)            → a or b.map(f)         // = a or (b.map(f))   ✗
Vector.contains(x == y, xs)      → x == y.contains(xs)   // precedence flip     ✗
Vector.len(-x)                   → -x.len()              // = -(x.len())        ✗
Vector.map(if c { a } else { b }, f) → if c {a} else {b}.map(f)               ✗
```

Byte-for-byte preservation of the `arg0` text (next section) keeps the *bytes*
intact but not the *parse*. Rather than wrap `arg0` in parens (more edits, more
ways to be wrong), the predicate simply does **not fire** unless `args[0].kind`
is postfix-atomic. The dropped cases (binary/prefix/`if`/`case`/closure
receivers) are rare in real call sites and arguably shouldn't be auto-rewritten
anyway. This is the same precedence hazard that already bites the formatter when
it strips `!(…)` parens — treat reorder-as-text as unsafe by default.

### Emission sites (`boot/compiler/checker.tw`)

1. `try_synth_module_qualified_call` — covers builtin type-qualified and user
   module-qualified calls. `M = method_name`, `fn_name` already resolved here.
   **Note:** this function only receives `callee_span` (which ends before the
   `(` — e.g. `Vector.map`), not the full call span. The hint needs the call's
   end byte (for the trailing-`)` edit and the stored anchor span), so thread
   the full call span `s` in as a new parameter, or compute the hint back in
   `synth_call` after this function returns (where `s` is in scope).
2. The `.Ident(name)` → `lookup_function(name)` arm of `synth_call`
   (the bare-call path). `M = name`, `fn_name = name`. `s` is already in scope.

Both sites are guarded by `ctx.fix_mode` (the review-mode flag) — when it is off
(every `build`/`check` compile) the whole block is skipped, so there is zero cost
outside `twk lint`.

A shared helper computes the optional rewrite:

```
fn inherent_method_rewrite(
  span: Span,            // the full call span s (callee start .. after ')')
  method: String,        // M
  fn_name: String,       // resolved callee
  params: Vector<MonoType>,  // inst.params
  args: Vector<Expr>,
  ctx: InferCtx,
) Rewrite?
```

It returns the rewrite when the predicate holds, `.None` otherwise. The rewrite
is collected into the **rewrite sink**, not the typecheck diagnostics the call
site returns — keeping it off the `build`/`check`/LSP path. Exact plumbing (a
`fix_mode`-gated accumulator on `InferCtx`, drained into the rewrite sink) is a
Stage 1 detail; the constraint is that it never joins the general `DiagKind`
stream.

## Representation

The rewrite lives on the **separate rewrite sink** (not the shared `DiagKind`
diagnostics stream), so it needs **no** new arm on `DiagKind` — the dozens of
exhaustive `case DiagKind` sites stay untouched. It is its own small type
(`boot/lib/source/rewrite.tw`):

```tw
pub type Rewrite = {
  InherentMethodCall(.{
    span: Span,          // whole call span (Edit anchors)
    method: String,      // M, used in the fix
    arg0_start: Int,     // start byte of args[0]
    arg0_end: Int,       // end byte of args[0]
    second_start: Int?,  // start byte of args[1] if present, else .None
  }),
}
```

`fixes(rewrite)` projects this into the shared `report.{SuggestedFix, FixEdit}`
(the byte-offset edits below); `twk lint --fix-inherent-calls` (or `--fix`) applies them, `twk lint` lists
them. (This module + its `fixes()` and `is_postfix_atomic()` are implemented and
tested in `boot/lib/source/rewrite.tw`.)

- The rewrite **never fails a build**: it is never on the compile/typecheck path,
  and `build`/`check` never run review mode.
- `callee_start` is `span.start`; `call_end` is `span.end` (the Call node spans
  from callee through the closing paren), so they need not be stored separately.

## Machine-applicable fix (no source slicing)

Two non-overlapping byte-offset edits reorder the call text. `fixes()` builds
them purely from the payload:

- **Edit A** — delete `[span.start, arg0_start)`: removes the `Callee(` prefix
  (callee, `(`, and any whitespace up to `args[0]`).
- **Edit B** — reinsert the method:
  - `second_start == .Some(s2)`: replace `[arg0_end, s2)` (the `, ` separator)
    with `.${method}(`.
  - `second_start == .None`: replace `[arg0_end, span.end)` (the trailing `)`
    and any whitespace) with `.${method}()`.

Worked examples:

```
Vector.map(xs, f)   A: del "Vector.map("   B: ", " → ".map("    ⇒ xs.map(f)
Vector.len(xs)      A: del "Vector.len("   B: ")"  → ".len()"   ⇒ xs.len()
point.translate(p)  A: del "point.translate("  B: ")" → ".translate()"  ⇒ p.translate()
```

The two edits never overlap: A ends at `arg0_start`, B starts at `arg0_end`, and
`arg0_start < arg0_end`, so the receiver text `args[0]` is preserved verbatim.
Verbatim preservation is only *safe* because the trigger predicate restricts
`args[0]` to postfix-atomic expressions (see "Receiver must be postfix-atomic");
without that guard, reordering a low-precedence receiver would reparse with
different meaning even though the bytes are unchanged.

Trivia between the affected tokens is dropped: Edit A removes any comment between
the callee and `args[0]`, and Edit B removes any comment between `args[0]` and
the next token. This is acceptable for a user-invoked `twk lint --fix` rewrite —
noted so it isn't mistaken for a bug later.

### Report message

In `twk lint` report mode, each opportunity reports
``"`${method}` can use inherent-method call syntax"`` at the call span. The
applied form is what `twk lint --fix-inherent-calls` writes; the structured
`SuggestedFix` is the single source of truth for both.

## Surfacing

Follows the review model (see `linter.md` → "Command surface"): **reported by
`twk lint`**, applied by `twk lint --fix-inherent-calls` (or `--fix`), never in
`twk build` / `twk check` or ambient LSP diagnostics. Mechanism specific to this
rule:

- The checker computes the rewrite at call-resolution sites **only in review
  mode** — a `fix_mode` flag on `InferCtx`, set exclusively by the `twk lint`
  entry point. `build`/`check` leave it off, so the predicate never runs and costs
  nothing.
- Results go to the **rewrite sink**, not the `diags` the checker returns.
- `fixes()` projects to `report.{SuggestedFix, FixEdit}`; `twk lint --fix*`
  applies the edits (reusing only the edit machinery, not the compiler diagnostic
  *path*).

Strong fit for an applied rewrite: the postfix-atomic guard and fail-closed
resolution check keep false positives near zero, and the rewrite is always
meaning-preserving, so applying it unattended is safe.

## Testing

Two layers:

- **Pure (done):** `fixes()` edit projection and `is_postfix_atomic()` are unit
  tested in `boot/tests/suites/lint_suite.tw` (re-homes to a rewrite suite).
- **Integration:** drive the **review-mode path** (run the frontend with `fix_mode`
  on, inspect the rewrite sink), *not* the typecheck diagnostics. A companion
  assertion confirms `build`/`check` (review mode off) produce **no** rewrites.

- `Vector.map(xs, f)` produces an `InherentMethodCall` rewrite with method `map`
  and the two expected edits.
- `String.len(s)` and `Dict.has(d, k)` each produce a rewrite — these exercise the
  builtin method-registration path the Scope section promises, which only fires if
  `lookup_method("String","len")` / `("Dict","has")` resolve. Without explicit
  coverage a quietly-unregistered builtin would fail closed and silently break the
  advertised scope.
- `point.translate(p, 1, 2)` and bare `translate(p, 1, 2)` each produce a rewrite.
- Applying the edits to the source yields the inherent form and re-typechecks
  cleanly (and is idempotent — a second `twk lint --fix` is a no-op).
- A single-arg call (`Vector.len(xs)`) produces the trailing-`)` edit form and the
  applied result is `xs.len()` — guards the span-threading fix (full call span,
  not `callee.span`).
- **No** rewrite for already-inherent `xs.map(f)`, for `println(x)`, and for a bare
  call whose first param isn't a receiver-typed inherent method.
- **No** rewrite when `args[0]` is non-postfix-atomic — `Vector.map(a or b, f)`,
  `Vector.len(-x)` — so the precedence-hazard guard stays in place.

## Non-goals

- No Rust/stage0 emission of the rewrite (stage0 only compiles the new source).
- Never on the compile path; applied only by `twk lint --fix*` (no config).
- No layout normalization — that is `twk fmt`'s job, run after `twk lint --fix`.
