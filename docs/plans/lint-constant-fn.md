# Lint: `constant-fn` — nullary function returning only a constant

Status: **Design.** Extends the shipped linter
([archive/linter.md](archive/linter.md)) with one new report-only lint. Follows
that doc's conventions (catalog membership, auto-fixability rule, no-config /
near-zero-false-positive bar, import-chain scope). Auto-fix is intentionally out
of scope; see Classification.

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
linter already plays for rewrites, but report-only because safe call-site editing
needs a closed-world reference graph Twinkle does not currently have.

This was hand-applied across `boot/compiler/codegen/runtime/task_abi.tw`; the lint
exists so the pattern is caught rather than re-litigated by review.

## Catalog entry

Adds to [archive/linter.md](archive/linter.md) §Catalog → Lints:

**L6 — Constant-returning function** (`constant-fn`)
- *Trigger*: a function declaration with **no parameters** and **no type
  parameters**, not `extern`, whose body has **no statements** and whose tail is a
  **single scalar constant expression** — a `String` / `Int` / `Float` / `Bool`
  literal. Signed numeric constants count when they are spelled as direct unary
  minus over an integer/float literal (`-1`, `-1.0`). Collection/variant literals
  remain out of v1 scope; see Open questions.
- *Rationale*: it is a module constant spelled as a function; Twinkle has
  `pub name := value` module constants, so prefer those. Removes a per-call
  indirection and states intent ("this is a constant") directly.
- *Report*: include the constant spelling in the help text, preserving
  `pub`/private and an explicit return annotation if present:
  - no explicit return type: `pub? <name> := <literal>`
  - explicit return type: `pub? <name>: <ReturnType> = <literal>`
  Also note call sites drop the `()`.
- *Escape*: no config or lint attribute. If a nullary function shape is genuinely
  intentional (for example because the symbol is passed as a `fn() T` value or is
  part of an API), make that intent ordinary code by hoisting the literal into a
  module constant and returning the constant identifier from the wrapper:
  `pub value := "x"; pub fn value_fn() String { value }`. The wrapper body is no
  longer a literal, so it does not trigger.
- *Home*: AST visitor (B) — `compiler/lint.tw` `lint_module`, like L3/L4/L5. Pure
  syntax over one declaration; no checker/type info required.

(The archived doc's "Rejected" section reuses the labels `L5`/`L6` for two
abandoned *candidates* that were never built; the implemented lints are L2–L5.
This assigns `L6` to a new built rule — track by the rule name `constant-fn` to
avoid confusion.)

## Classification: report-only, no auto-fix

By the linter's auto-fixability rule, an item is auto-fixable only if applying it
is provably meaning-preserving on correct code. Converting a constant-fn is not
safe under Twinkle's current source model:

1. **Function-as-value use.** If the symbol is ever used as a first-class value
   (passed where a `fn() T` is expected, stored in a capability record, etc.),
   replacing it with a constant breaks those sites. A pure declaration lint cannot
   know that every reference is a direct nullary call.
2. **Cross-module call-site edits.** A `pub` constant-fn may be called from other
   modules; an auto-fix must rewrite the declaration and every resolved direct
   call `name()` / `module.name()` → `name` / `module.name`.
3. **No closed-world project graph.** This is the same dilemma as LSP rename:
   Twinkle currently has script entries and their import closures, not a project
   or workspace model that enumerates every consumer of a public module API. The
   compiler can know the calls reachable from the current entry, but it cannot
   know every external call site that may need edits.

A private-only rewrite would be possible in principle because private symbols are
module-local, but it would add checker/reference plumbing for a narrow subset
while the motivating cases are often public API constants. To avoid split
semantics and false confidence, this rule has **no `--fix-constant-fn` flag**.
It reports the declaration shape and leaves the declaration plus call-site edits
to the human.

Because `twk lint` is commonly used as a CI gate and has no per-rule suppression,
"report-only" is not an excuse for noisy findings: calibration must find no
intentional nullary literal functions in `boot/`, or the trigger must be narrowed
before shipping.

## False positives & calibration

Per the no-config / near-zero-FP bar, the rule must be calibrated on `boot/`
before shipping. Expected shapes that should **not** fire:
- `extern` functions (no body) — excluded by construction.
- Functions with parameters or type parameters — excluded.
- Functions whose body does real work / is a non-literal expression — excluded
  (v1 only matches a single literal).

A nullary literal-returning function deliberately kept as a function because it
is **used as a value** is a potential false positive, not an acceptable steady
state: with no config and no `@allow`, it would still fail a lint-gated CI run.
If `boot/` calibration finds this pattern, either narrow the trigger, require the
ordinary-code escape (literal hoisted to a constant; wrapper returns the
identifier), or reuse the rewrite tier's resolved-reference check as a detection
guard.

## Implementation sketch

Report-only lint:
- Make the structural pass source-aware enough to render the suggested spelling
  (either pass source text into `lint_module` or add a sibling helper used by the
  lint command/tests). Source slices are preferred over re-rendering AST values.
- In `compiler/lint.tw`, add a visitor over top-level function declarations:
  - match: `params.len() == 0`, `type_params.len() == 0`, `body.stmts.len() == 0`,
    and `body.tail` is `.Some(literal)` where `literal` is `StringLit` / `IntLit`
    / `FloatLit` / `BoolLit`, or `Unary(.Neg, IntLit|FloatLit)`.
  - explicitly do **not** match a block with side-effect statements before a
    literal tail.
  - emit a finding at the declaration span with rule name `constant-fn` and a help
    string showing the replacement form; preserve `pub` and explicit return type
    as described above.
- Reuse the existing `Report`/`SpanLabel`/line-index rendering and the
  import-chain finding aggregation (entry + imports, stdlib/internal excluded), so
  no new report plumbing.
- No `DiagKind` arm. The report-only lint needs no checker hook; the auto-fix
  tier does, for resolved reference classification.

No auto-fix tier:
- Do not add `Rewrite` variants or a `--fix-constant-fn` selector.
- Do not add checker reference tracking for this rule unless Twinkle later gains a
  project/workspace model that makes public call-site rewriting closed-world.

## Testing

Report-only lint:
- A `lint` suite case (or fixture) with a constant-returning `fn` asserts the
  finding fires with the suggested constant text.
- Positive cases cover private/public declarations, explicit return annotations,
  strings needing escapes/raw spelling preservation, booleans, ints/floats, and
  direct signed numeric literals.
- Negative cases cover a parameterized fn, a type-parameterized fn, an `extern`, a
  nullary fn returning a non-literal expression, and a block with side-effect
  statements before a literal tail.

Calibration:
- Run `twk lint boot/main.tw` and confirm the findings are all genuine
  constant-fns. If intentional function-valued constants appear, narrow the rule
  or add the resolved-reference guard before shipping.
- Confirm there is no `--fix-constant-fn` path; this lint remains report-only
  until Twinkle has a project/workspace graph that can make public call-site
  rewriting closed-world.

## Open questions

- **Scope of "literal":** v1 = scalar literals. Should empty collection literals
  (`[]`, `Dict.new()`-style) or nullary variant literals (`.None`) count? They are
  valid module constants too, but `[]`/`.None` as a fn body is rarer and slightly
  more likely intentional — defer to calibration.
- **Future project model:** If Twinkle later gains an explicit project/workspace
  graph, should this rule grow a closed-world rewrite that edits every resolved
  direct call site? Until then, no auto-fix.
