# Lint: `constant-fn` — nullary function returning only a constant

Status: **Design.** Extends the shipped linter
([archive/linter.md](archive/linter.md)) with one new report-only lint. Follows
that doc's conventions (catalog membership, auto-fixability rule, no-config /
near-zero-false-positive bar, import-chain scope).

## Motivation

A recurring shape is a zero-argument function whose whole body is a single
constant, used only to name that constant:

```tw
pub fn sym_channel_send() String { "channel_send" }
// ... called everywhere as sym_channel_send()
```

That is a module-level constant wearing a function costume. Twinkle has
first-class module constants (`pub name := value`, e.g. `boot/lib/lsp/kinds.tw`),
so the constant should be spelled as one:

```tw
pub sym_channel_send := "channel_send"
// ... called as sym_channel_send
```

The function form adds a call at every use, an extra indirection, and reads as if
there were logic where there is none. This lint flags the declaration and points
at the constant spelling — the same "correct code, better spelling" role the
linter already plays for rewrites, but report-only (see Classification).

This was hand-applied across `boot/compiler/codegen/runtime/task_abi.tw`; the lint
exists so the pattern is caught rather than re-litigated by review.

## Catalog entry

Adds to [archive/linter.md](archive/linter.md) §Catalog → Lints:

**L6 — Constant-returning function** (`constant-fn`)
- *Trigger*: a function declaration with **no parameters** and **no type
  parameters**, not `extern`, whose body is a **single constant literal
  expression** — a `String` / `Int` / `Float` / `Bool` literal (v1 scope; see
  Extensions for collection/variant literals).
- *Rationale*: it is a module constant spelled as a function; Twinkle has
  `pub name := value` module constants, so prefer those. Removes a per-call
  indirection and states intent ("this is a constant") directly.
- *Report*: include the constant spelling in the help text — `pub <name> :=
  <literal>` (preserve `pub`/private), and note call sites drop the `()`.
- *Escape*: none needed in v1 — if the form is intentional (see False positives),
  the lint is report-only, so a human simply leaves it.
- *Home*: AST visitor (B) — `compiler/lint.tw` `lint_module`, like L3/L4/L5. Pure
  syntax over one declaration; no checker/type info required.

(The archived doc's "Rejected" section reuses the labels `L5`/`L6` for two
abandoned *candidates* that were never built; the implemented lints are L2–L5.
This assigns `L6` to a new built rule — track by the rule name `constant-fn` to
avoid confusion.)

## Classification: report-only (not auto-fix by default)

By the linter's auto-fixability rule, an item is auto-fixable only if applying it
is provably meaning-preserving on correct code. Converting a constant-fn is *not*
unconditionally safe:

1. **Function-as-value use.** If the symbol is ever used as a first-class value
   (passed where a `fn() T` is expected, stored in a capability record, etc.),
   replacing it with a constant breaks those sites. Proving "only ever called
   directly" needs the checker's resolved call data across the import chain — not
   available to a pure single-module AST pass.
2. **Cross-module call-site edits.** A `pub` constant-fn is called from other
   modules; an auto-fix must rewrite the declaration *and* every `name()` → `name`
   across the chain, then re-fmt.

Neither is a guess a mechanical fix should make, so v1 is **report-only**: detect
the declaration shape and print the suggested constant form. The human applies it
(as was done for `task_abi.tw`).

### Optional follow-up: an auto-fix rewrite tier

If demand justifies it, a rewrite `R3 — constant-fn → module constant`
(`--fix-constant-fn`) could apply it, gated on:
- the body is a literal (as above), **and**
- the symbol is referenced **only in direct-call position** anywhere in the
  import-chain scope (computed from the checker's resolved-call data, like R1),
  **and**
- the engine emits the decl edit plus every call-site `()`-drop edit, splices
  offset-stably (the existing `apply_edits`), and relies on `twk fmt` after.

This is the harder piece (cross-module call-site rewriting + a function-value
guard); deferred behind the report-only lint, consistent with how the linter
shipped rewrites only once provably safe.

## False positives & calibration

Per the no-config / near-zero-FP bar, the rule must be calibrated on `boot/`
before shipping. Expected shapes that should **not** fire:
- `extern` functions (no body) — excluded by construction.
- Functions with parameters or type parameters — excluded.
- Functions whose body does real work / is a non-literal expression — excluded
  (v1 only matches a single literal).

Known acceptable FP for a report-only rule: a nullary literal-returning function
deliberately kept as a function because it is **used as a value**. Since the lint
is report-only, the human skips it; if `boot/` calibration shows this is common,
either narrow the trigger or add the call-position check from the rewrite tier as
a detection guard.

## Implementation sketch

- In `compiler/lint.tw` `lint_module`, add a visitor over top-level function
  declarations:
  - match: `params.len() == 0`, `type_params.len() == 0`, not `extern`, body is a
    block whose single tail expression is a literal node (String/Int/Float/Bool).
  - emit a finding at the declaration span with the rule name `constant-fn` and a
    help string showing `pub? <name> := <literal>`.
- Reuse the existing `Report`/`SpanLabel`/line-index rendering and the
  import-chain finding aggregation (entry + imports, stdlib/internal excluded), so
  no new plumbing.
- No review-mode checker hook (home B), no `DiagKind` arm.

## Testing

- A `lint` suite case (or fixture) with a constant-returning `fn` asserts the
  finding fires with the suggested constant text; a parameterized fn, an `extern`,
  and a real-logic nullary fn assert it does **not** fire.
- Calibrate on `boot/`: run `twk lint boot/main.tw` and confirm the findings are
  all genuine constant-fns (zero spurious), adjusting the trigger if not.

## Open questions

- **Scope of "literal":** v1 = scalar literals. Should empty collection literals
  (`[]`, `Dict.new()`-style) or nullary variant literals (`.None`) count? They are
  valid module constants too, but `[]`/`.None` as a fn body is rarer and slightly
  more likely intentional — defer to calibration.
- **Auto-fix:** ship report-only first; revisit `R3` only if the manual fix proves
  frequent enough to warrant the cross-module rewrite machinery.
