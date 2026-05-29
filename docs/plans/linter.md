# Linter (`twk lint`) — Design Plan

Status: proposal. Target implementation: boot compiler (`boot/`).

## Motivation

Twinkle already rejects ill-typed programs, but there is a band of code that
type-checks yet is almost certainly a mistake or a documented anti-pattern:
a pure call whose result is thrown away, an unhandled `Result`, a hand-written
`with_*` record-copy helper that the language explicitly tells you not to write.

`twk fmt` owns layout. The linter owns *semantics*: it only flags things a human
reading the code would agree are suspicious. The bar for shipping a lint is
**near-zero false positives** — a noisy linter gets ignored, then disabled.

Twinkle's value model makes some of these lints far stronger than they are in
other languages: because almost every function is pure, "you computed a value
and discarded it" is, by default, dead code rather than a stylistic nit.

## Scope

In scope (this plan):

- **L1 — Discarded pure result** ("must-use"): a statement-position expression
  whose value is dropped, where the call has no effect.
- **L2 — Ignored `Result`/`Option`**: a value of type `Result<_, _>` or `T?`
  produced and neither `try`-ed, matched, nor otherwise consumed.
- **L3 — `with_*` field-copy rebuild**: a function that reconstructs a record of
  the same type as one of its parameters, copying most fields verbatim — the
  pattern CLAUDE.md forbids in favor of field rebinding.
- **L4 — Unreachable statements**: code following a diverging statement
  (`return`, `break`, `continue`, `error(...)`/trap) in the same block.
- **L5 — Wildcard `_ =>` over a project-local sum type** (opt-in, off by
  default): a catch-all arm on an enum defined in this project, which silently
  swallows future variants.
- **L6 — Suspicious shadowing** (opt-in, off by default): a name rebound to a
  value of an unrelated type within the same scope.

Explicitly **out of scope**:

- Formatting / style (owned by `twk fmt`).
- Unused bindings / params / imports / private fns. Unused-import detection
  already ships (`compiler/unused_imports.tw`, `twk check --fix-unused-imports`);
  the rest of this family is deliberately deferred.
- Persistent-collection performance lints (`concat`/append-in-loop, etc.).
- **Redundant / unreachable `case` arms** and **non-exhaustive matches** —
  already implemented as the hard errors `UnreachableCaseArm` and
  `MissingVariants` in `lib/source/diagnostics.tw`. No linter work needed.

## What already exists (and what we reuse)

The diagnostic plumbing is done; lints are mostly new *producers* on existing
rails:

- `lib/source/diagnostics.tw` — `Severity { Error, Warning, Hint, Info }`,
  `WarningDiag`, `DiagKind`, plus `message`/`span`/`help_lines`/`format_diagnostics`
  rendering. **We add new `WarningDiag` variants here.**
- `compiler/query/analyze.tw` — `AnalysisDiag` carries `kind: DiagKind` tagged
  with module identity, and `analyze_module_impl` already runs a post-typecheck
  analysis (`check_unused_imports`) on non-internal modules and appends its
  warnings to the module's diagnostics. **This is the hook point.**
- `compiler/pipeline.tw` — already filters `AnalysisDiag` for `.Warning(_)` and
  surfaces them on `PipelineArtifacts.warnings`, consumed by `build`, `check`,
  and the LSP. Lints inherit all three surfaces for free.
- `commands/check.tw` — already prints warnings via `print_warnings`.
- `compiler/unused_imports.tw` — the reference implementation for an AST-walking
  analysis that emits `WarningDiag`s with spans.

## Architecture

There are two natural homes for a lint, chosen per-lint by whether it needs
inferred types:

### A. Type-dependent lints → emitted by the type checker (L1, L2)

L1 and L2 need the inferred type of a statement-position expression (is it
`Void`? is it `Result`/`Option`? is the callee effectful?). The type checker
already computes exactly this at the moment it checks each statement. Emitting
the warning there — as a `.Warning(...)` `DiagKind`, alongside the errors the
checker already produces — is both the cheapest and the most accurate option,
and avoids a second typed traversal.

This requires the checker to know, per call, whether the callee is *pure*. We
already distinguish the effectful builtins (`print`, `println`, `error`) ; the
proposal is a small **effect flag on function signatures**: a function is
"effectful" if its body transitively calls an effectful builtin, else pure.
This can start coarse (only the three builtins are effectful; everything calling
them transitively is effectful; closures conservatively effectful) and be
refined later. Until that analysis exists, L1 can ship in a conservative form
(see Rollout).

### B. Structural lints → a dedicated AST visitor module (L3, L4, L5, L6)

These are syntactic/structural and best run on the parsed `Module`, mirroring
`unused_imports.tw`. New module `compiler/lint.tw` exposes:

```
pub fn lint_module(module: Module, env: ResolvedEnv) Vector<DiagKind>
```

invoked from `analyze_module_impl` right after `check_unused_imports`, gated on
`!dep_plan.is_internal`. It receives the resolved `env` so it can answer
"is this nominal type defined in this project?" (L5) and "does this returned
record type match this parameter's type?" (L3).

Both homes feed the same `WarningDiag` channel, so there is one rendering path
and one config model.

## Lint catalog

Each lint specifies its trigger, rationale, in-source escape hatch, default
severity, and where it lives.

### L1 — Discarded pure result  (`unused-result`)

- **Trigger**: a statement whose value is an expression (not a `let`/`for`/
  `return`/assignment), whose type is not `Void`, and whose outermost form is a
  call to a *pure* function (or a pure operator/field access).
- **Rationale**: pure computation evaluated for nothing is dead code or a
  forgotten rebinding (`items.append(x)` instead of `items = items.append(x)`).
- **Escape**: explicitly bind the value (see Suppression — discard binding).
- **Default**: warn.
- **Home**: type checker (A).

### L2 — Ignored `Result` / `Option`  (`unused-must-use`)

- **Trigger**: a statement-position expression of type `Result<_, _>` or `T?`
  that is neither `try`-ed nor pattern-matched nor bound. A strict subset of L1
  conceptually, but kept distinct because it fires even for *effectful* callees
  (ignoring an error is a bug regardless of side effects) and warrants a
  sharper message.
- **Rationale**: silently dropping an error/`None` is a correctness hazard.
- **Escape**: handle it (`try`, `case`, `.ok_or`, …) or bind it explicitly.
- **Default**: warn.
- **Home**: type checker (A).

### L3 — `with_*` field-copy rebuild  (`record-copy-helper`)

- **Trigger**: a function whose return expression is a record literal
  (`.{…}` or `T.{…}`) of the same nominal type as one of its parameters `p`,
  where most fields are verbatim same-named copies `p.<field>` and only a few
  differ. (Threshold tunable; start at "≥1 field copied verbatim and every
  field is either a verbatim copy of `p` or one of the function's own
  params/locals.") The "non-trivial work" exception in CLAUDE.md is respected:
  if any field value does real computation, the function is not flagged.
- **Rationale**: directly encodes the documented house rule (CLAUDE.md
  *Immutability and Rebinding*): use `p.field = v` rebinding, not copy helpers.
- **Escape**: none needed — the trigger is precise. Rewriting with field
  rebinding removes the construct entirely.
- **Default**: warn. **Help line**: show the rebinding rewrite.
- **Home**: AST visitor (B), using `env` to confirm the nominal type match.

### L4 — Unreachable statements  (`unreachable-code`)

- **Trigger**: any statement that follows a diverging statement in the same
  block — `return`, `break`, `continue`, or a call to `error(...)`/other trap.
- **Rationale**: the trailing code never runs; usually a logic error.
- **Escape**: delete the dead code.
- **Default**: warn.
- **Home**: AST visitor (B). Complements the existing `UnreachableCaseArm`
  error, which covers only `case` arms. *Verify during implementation whether
  any general unreachable-after-divergence detection already exists; this lint
  fills that gap if not.*

### L5 — Wildcard over project-local sum type  (`wildcard-local-enum`)

- **Trigger**: a `case` whose scrutinee is a sum type **defined in this project**
  (not stdlib/prelude) that uses a `_ =>` catch-all instead of listing variants.
- **Rationale**: a catch-all means adding a variant later compiles silently
  instead of surfacing every site that must change. For large stdlib enums
  (token kinds, AST nodes) a catch-all is reasonable, hence the project-local
  restriction.
- **Escape**: enumerate the variants, or leave the lint off (it is opt-in).
- **Default**: **off** (opt-in via config). Opinionated.
- **Home**: AST visitor (B), using `env` to classify the scrutinee's origin.

### L6 — Suspicious shadowing  (`shadow`)

- **Trigger**: a binding that rebinds a name already in scope to a value of an
  unrelated type. Because rebinding is idiomatic in Twinkle, same-type rebinding
  is *never* flagged; only a type change is.
- **Rationale**: catches accidental name collisions; deliberately narrow.
- **Escape**: rename.
- **Default**: **off** (opt-in). Needs care to stay quiet.
- **Home**: AST visitor (B); needs inferred types, so may instead piggyback on
  the checker — decide during implementation.

## Suppression model (no new syntax)

Per the constraint, **no `@allow(...)` attributes** are added to the language.
Suppression has exactly two mechanisms:

1. **Project config in `twinkle.toml`** — a `[lint]` table sets per-lint
   severity. This is the coarse, project-wide control (and how the opt-in lints
   L5/L6 get turned on).

   ```toml
   [lint]
   unused-result      = "warn"   # off | warn | error
   unused-must-use    = "error"
   record-copy-helper = "warn"
   unreachable-code   = "warn"
   wildcard-local-enum = "off"
   shadow             = "off"
   ```

2. **In-source intent via an explicit discard binding** — for the value-discard
   lints (L1/L2), the way to say "I really mean to throw this away" is to *bind*
   the value. Today Twinkle has no discard binding, so this plan proposes a
   small, natural language affordance (distinct from an attribute):

   ```tw
   _ := some_pure_call(x)      // explicit discard — silences L1/L2
   ```

   `_` as a let target is a discard, not a usable name. This is the single
   language touch the linter motivates; it is orthogonal to lint metadata and
   reads as ordinary code. **Open question** (below): adopt `_ :=` discard, or
   require binding to a named throwaway instead.

The structural lints (L3/L4) need no per-site escape: their triggers are precise
and the fix removes the flagged construct.

## Command surface

- **`twk lint <file>`** — new command (`commands/lint.tw`): runs the frontend
  (`pipeline` analysis only, no codegen), prints all lint diagnostics, and exits
  non-zero if any lint at `error` severity fired. This is the CI entry point.
- **`twk build` / `twk check`** — already print the shared warning channel, so
  lints show up there automatically; no behavior change beyond new warnings.
- **LSP** — `AnalysisDiag` already flows to the editor, so lints appear inline
  with no extra wiring. L3's help line / suggested rewrite can later become a
  code action, reusing the `unused_imports` edit pattern.

`twk fmt` is untouched.

## Rollout

Ship in stages, validating signal-to-noise on `boot/` itself at each step
(it is a large real codebase — if a lint is noisy there, it is miscalibrated):

1. **Plumbing + L4 (unreachable) + L3 (record-copy)** — pure AST lints, no type
   or effect analysis required, near-zero false positives. Adds `compiler/lint.tw`,
   the `WarningDiag` variants, the `analyze_module_impl` call, and `twk lint`.
2. **L2 (ignored `Result`/`Option`)** — checker emits warnings; needs only the
   inferred statement type, which the checker already has. Decide the discard
   escape here (drives the `_ :=` open question).
3. **L1 (unused pure result)** — needs the purity/effect flag on signatures.
   Until that lands, optionally ship a conservative L1 limited to calls of
   functions known-pure structurally (no transitive `print`/`error`).
4. **L5, L6** — opt-in, off by default; enable on `boot/` experimentally to
   tune before recommending.

`twinkle.toml` `[lint]` config lands with stage 1 (even if only a couple of
keys are meaningful at first).

## Open questions

- **Discard syntax**: adopt `_ :=` / `_ =` as a discard binding (clean, but a
  small parser/checker change), or require a named throwaway and accept that
  L1/L2 have no terse escape? Leaning toward `_ :=`.
- **Effect analysis granularity** for L1: is "transitively calls `print`/
  `println`/`error`" a good enough purity approximation, or do we need to treat
  host externs / closures more precisely?
- **`twk lint` vs folding into `twk check`**: a dedicated command keeps CI
  intent explicit and lets `lint` default to non-zero exit on findings without
  changing `check`'s contract. Confirm this is the desired split.
- **L3 threshold**: exact cutoff for "mostly verbatim copies" before flagging,
  tuned against false positives on real constructors in `boot/`.
