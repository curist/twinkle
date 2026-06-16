# Inherent-method-call hint diagnostic

> **Status: satellite design for linter rule L7 (`inherent-method-call`).**
> This is the detailed design for the idiom lint catalogued in
> [`linter.md`](linter.md). It does **not** ship standalone: it lands with the
> linter's lint sink + `twk lint` plumbing (linter Stage 1). The trigger
> predicate, emission sites, and fix below are the rule's substance; **surfacing
> follows the linter's single model** — `twk lint`-only, separate lint sink, off
> the compile path — so this doc does not re-litigate it (see `linter.md` →
> "Surfacing").

## Goal

Flag a call written in **free-function form** when the receiver-method
(inherent) form would resolve to the *same* function, and offer a
machine-applicable fix that rewrites the call. Surfaces **only** when you run
`twk lint`, never in `build`/`check` or ambient LSP diagnostics.

```tw
Vector.map(xs, f)          // hint → xs.map(f)
point.translate(p, 1, 2)   // hint → p.translate(1, 2)
translate(p, 1, 2)         // hint → p.translate(1, 2)   (bare, same-module)
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
registry stores a different canonical name, no hint is emitted.

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

Both sites are guarded by `ctx.lint_mode` — when it is off (every `build`/`check`
compile) the whole block is skipped, so there is zero cost outside `twk lint`.

A shared helper computes the optional finding:

```
fn inherent_method_hint(
  span: Span,            // the full call span s (callee start .. after ')')
  method: String,        // M
  fn_name: String,       // resolved callee
  params: Vector<MonoType>,  // inst.params
  args: Vector<Expr>,
  ctx: InferCtx,
) LintFinding?
```

It returns the finding when the predicate holds, `.None` otherwise. The finding
is collected into the **lint sink**, not the typecheck diagnostics the call site
returns — keeping it off the `build`/`check`/LSP path. Exact plumbing (a
`lint_mode`-gated accumulator on `InferCtx`, drained into
`PipelineArtifacts.lints`) is a Stage 1 detail; the constraint is that it never
joins the general `DiagKind` stream.

## Representation

Because the hint lives on a **separate lint sink** (not the shared `DiagKind`
diagnostics stream — see "Surfacing"), it does **not** need a new arm
on the general `DiagKind` enum, and the dozens of exhaustive `case DiagKind`
sites across the compiler stay untouched. It is its own small payload type
carried by `PipelineArtifacts.lints`:

```tw
pub type LintFinding = {
  InherentMethodCall(.{
    span: Span,          // call span (squiggle target + Edit anchors)
    method: String,      // M, used in message and fix
    arg0_start: Int,     // start byte of args[0]
    arg0_end: Int,       // end byte of args[0]
    second_start: Int?,  // start byte of args[1] if present, else .None
  }),
}
```

(Exact type name/location is a Stage 1 detail; the point is it is a lint-channel
type, distinct from `DiagKind`.) For rendering, `twk lint` converts a
`LintFinding` into the shared `Report`/`SpanLabel`/`FixEdit` structures — reusing
the formatting machinery — and emits the byte-offset fix below as a
`SuggestedFix`.

- The hint **never fails a build**: it is never on the compile/typecheck path at
  all, and `build`/`check` never run lint mode.
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
the next token. This is acceptable for a user-invoked `twk lint` rewrite — noted
so it isn't mistaken for a bug later.

### Message and preview

- `message`: ``"`${method}` can use inherent-method call syntax"`` (the squiggle
  points at the call; the fix shows the rewrite).
- The auto-generated `fix_preview_lines` would show the raw edit fragments, which
  read poorly for a reorder. Keep the message self-contained; the structured fix
  drives LSP code actions and `--fix`. (If a nicer terminal preview is wanted
  later, it requires source access at render time — out of scope here.)

## Surfacing

Follows the linter's single model (see `linter.md` → "Surfacing"): **always on,
no config**, surfaced **only when you run `twk lint`** — never in `twk build` /
`twk check` or ambient LSP diagnostics. Recap of the mechanism specific to this
rule:

- The checker computes the hint at call-resolution sites **only in lint mode** —
  a `lint_mode` flag on `InferCtx`, set exclusively by the `twk lint` entry
  point. `build`/`check` leave it off, so the predicate never runs and costs
  nothing there.
- Results go to a **separate lint sink** (e.g. `PipelineArtifacts.lints`), not
  the `diags` the checker returns to the general pipeline.
- It **reuses** the `Report`/`FixEdit` rendering for nice `twk lint` output and
  the machine-applicable fix; that is sharing the *formatting machinery*, not the
  compiler diagnostic *path*.

This rule is a strong always-on fit: the postfix-atomic guard and fail-closed
resolution check keep false positives near zero, and the rewrite is always
meaning-preserving, so there is nothing to configure away.

## Testing

Drive assertions through the **lint-mode path** (run the frontend with
`lint_mode` on and inspect the lint sink), *not* the typecheck diagnostics. A
companion assertion confirms the general `build`/`check` path (lint mode off)
produces **no** lint findings, so the isolation from the compile path is covered.

- `Vector.map(xs, f)` produces an `InherentMethodCall` finding with method `map`
  and the two expected edits.
- `String.len(s)` and `Dict.has(d, k)` each produce the hint — these exercise the
  builtin method-registration path the Scope section promises, which only fires if
  `lookup_method("String","len")` / `("Dict","has")` resolve. Without explicit
  coverage a quietly-unregistered builtin would fail closed and silently break the
  advertised scope.
- `point.translate(p, 1, 2)` and bare `translate(p, 1, 2)` each produce the hint.
- Applying the fix edits to the source yields the inherent form and re-typechecks
  cleanly.
- A single-arg call (`Vector.len(xs)`) produces the trailing-`)` edit form and the
  applied result is `xs.len()` — guards the span-threading fix (full call span,
  not `callee.span`).
- **No** hint for already-inherent `xs.map(f)`, for `println(x)`, and for a bare
  call whose first param isn't a receiver-typed inherent method.
- **No** hint when `args[0]` is non-postfix-atomic — `Vector.map(a or b, f)`,
  `Vector.len(-x)` — so the precedence-hazard guard stays in place.

## Non-goals

- No Rust/stage0 emission of the hint (stage0 only compiles the new source).
- Never on the compile path; surfaced only by `twk lint` (no config, no flag).
- No human-readable full-rewrite terminal preview (would need render-time source).
