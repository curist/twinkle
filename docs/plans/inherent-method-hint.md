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

When all hold, the inherent form `args[0].M(args[1..])` provably resolves to the
same function, so the rewrite is safe. The predicate fails closed: if the
registry stores a different canonical name, no hint is emitted.

### Emission sites (`boot/compiler/checker.tw`)

1. `try_synth_module_qualified_call` — covers builtin type-qualified and user
   module-qualified calls. `M = method_name`, `fn_name` already resolved here.
2. The `.Ident(name)` → `lookup_function(name)` arm of `synth_call`
   (the bare-call path). `M = name`, `fn_name = name`.

A shared helper computes the optional hint and appends it to `diags`:

```
fn inherent_method_hint(
  span: Span,            // the full call span s
  method: String,        // M
  fn_name: String,       // resolved callee
  params: Vector<MonoType>,  // inst.params
  args: Vector<Expr>,
  ctx: InferCtx,
) DiagKind?
```

It returns `.Some(.Info(.InherentMethodCall(...)))` when the predicate holds,
`.None` otherwise.

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
- `point.translate(p, 1, 2)` and bare `translate(p, 1, 2)` each produce the hint.
- Applying the fix edits to the source yields the inherent form and re-typechecks
  cleanly.
- **No** hint for already-inherent `xs.map(f)`, for `println(x)`, and for a bare
  call whose first param isn't a receiver-typed inherent method.

## Non-goals

- No Rust/stage0 emission of the hint (stage0 only compiles the new source).
- No CLI surfacing of `Info` by default.
- No human-readable full-rewrite terminal preview (would need render-time source).
