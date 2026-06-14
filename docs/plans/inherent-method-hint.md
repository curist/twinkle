# Inherent-method-call hint diagnostic

## Goal

Emit an `Info`-level diagnostic when a call is written in **free-function form**
but the receiver-method (inherent) form would resolve to the *same* function.
The diagnostic carries a machine-applicable fix that rewrites the call.

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

A shared helper computes the optional hint:

```
fn inherent_method_hint(
  span: Span,            // the full call span s (callee start .. after ')')
  method: String,        // M
  fn_name: String,       // resolved callee
  params: Vector<MonoType>,  // inst.params
  args: Vector<Expr>,
  ctx: InferCtx,
) DiagKind?
```

It returns `.Some(.Info(.InherentMethodCall(...)))` when the predicate holds,
`.None` otherwise. Append the result to the diagnostics vector that the call
site actually **returns** (e.g. `bounds_r.diags` in
`try_synth_module_qualified_call`), not the `diags` parameter — that input
vector is re-bound and threaded through `check_instantiated_call`, so appending
to it would be lost.

## Representation

New `Info` arm in `boot/lib/source/diagnostics.tw`:

```tw
pub type InfoDiag = {
  InherentMethodCall(.{
    span: Span,          // call span (squiggle target + Edit anchors)
    method: String,      // M, used in message and fix
    arg0_start: Int,     // start byte of args[0]
    arg0_end: Int,       // end byte of args[0]
    second_start: Int?,  // start byte of args[1] if present, else .None
  }),
}

pub type DiagKind = { Error(ErrorDiag), Warning(WarningDiag), Info(InfoDiag) }
```

- `has_errors` returns `false` for `Info` — never fails a build.
- `callee_start` is `span.start`; `call_end` is `span.end` (the Call node spans
  from callee through the closing paren), so they need not be stored separately.

### Exhaustive `case DiagKind` sites to extend

- `boot/lib/source/diagnostics.tw`: `span`, `message`, `help_lines`,
  `has_errors`, `fixes`, `format_diagnostics` (label `info`).
- `boot/compiler/query/diagnostics.tw`: `kind_to_severity` (`.Info(_) => .Info`),
  `kind_to_message`.
- `boot/compiler/query/diag_render.tw`: `info_to_report` + the top-level
  `case kind` arm; render at `Severity.Info`.
- `boot/compiler/query/analyze.tw`: any exhaustive `DiagKind` match.
- `boot/compiler/pipeline.tw`, `boot/compiler/module_compiler.tw`: the
  `.Error/.Warning` collection matches gain an `.Info(_)` arm.

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
the next token. This is acceptable for an opt-in rewrite — noted so it isn't
mistaken for a bug later.

### Message and preview

- `message`: ``"`${method}` can use inherent-method call syntax"`` (the squiggle
  points at the call; the fix shows the rewrite).
- The auto-generated `fix_preview_lines` would show the raw edit fragments, which
  read poorly for a reorder. Keep the message self-contained; the structured fix
  drives LSP code actions and `--fix`. (If a nicer terminal preview is wanted
  later, it requires source access at render time — out of scope here.)

## Noise handling

No new flag. The CLI `build`/`run` path (`pipeline.tw`) already collects only
`.Warning` diagnostics into its surfaced set, so `Info` is invisible in CLI
output by default — keeping the test suite and the boot self-compile clean.
The LSP/analyze query (`convert_analysis_diags`) surfaces all severities, so
hints appear as editor squiggles/code-actions, which is where they belong.

## Testing

Drive assertions through the analyze/query API (not CLI output), matching the
existing `diag_suite` / `lsp_diagnostics_suite` style:

- `Vector.map(xs, f)` produces an `Info` `InherentMethodCall` with method `map`
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
- No CLI surfacing of `Info` by default.
- No human-readable full-rewrite terminal preview (would need render-time source).
