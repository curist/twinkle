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

`twk lint` is to Twinkle what `cargo clippy` is to Rust: a **separate,
non-intrusive pass**. None of these lints are fatal — they all describe code that
type-checks fine — so none of them belong in the compile path. They surface
**only when you run `twk lint`**, never in `twk build` / `twk check` or ambient
typecheck-driven LSP diagnostics. Opting in is just running the command. (The
genuine compiler warnings that already exist — unused imports, single-line
multiline blocks — are a separate, pre-existing channel and keep showing in
`build`/`check`; this plan does not touch them.)

**The linter only detects; it never rewrites your code.** Every lint here flags
*suspected-buggy* or anti-pattern code, where the correct fix depends on intent
the tool cannot infer — so auto-applying a fix would risk cementing the very bug
the lint surfaced (delete unreachable code that was meant to run; "fix" a
forgotten rebinding by deleting it). Lints therefore *explain*, at most showing
an illustrative rewrite in their help text, but never carry a machine-applicable
fix. Rewrites that *are* provably meaning-preserving — and so can be applied
unattended — are a separate tool, **`twk fix`** (see [`fix.md`](fix.md)).

**No per-rule configuration.** Every shipped lint is **always on** under
`twk lint`; there is no `[lint]` table, no severity knobs, no allow-by-default
tier. The discipline this enforces is the whole point: a lint earns its place
*only* by clearing the near-zero-false-positive bar on real code (`boot/`). If a
candidate is too noisy to be on for everyone, the answer is to **drop it**, not
to hide it behind a flag. The single sanctioned way to silence a *specific,
intentional* instance is in-source intent (the discard binding for L1/L2 — see
Suppression), never a config switch.

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

**Moved to `twk fix`:**

- **Inherent-method-call rewrite** (`Vector.map(xs, f)` → `xs.map(f)`) started
  life here, but it is the one item that is *correct code with a nicer spelling*,
  not a suspected bug — so it has a provably meaning-preserving auto-fix and
  belongs in the fixer, not the linter. See [`fix.md`](fix.md) and its design doc
  [`inherent-method-hint.md`](inherent-method-hint.md).

**Rejected:**

- **L5 — Wildcard `_ =>` over a project-local sum type**. A blanket catch-all
  on a local enum is frequently *intentional* (a genuine default arm), and there
  is no always-on trigger that reliably separates a lazy catch-all from a
  deliberate one without per-project opt-in. Under the no-config / always-on rule
  it cannot meet the near-zero-false-positive bar, so it is dropped. (Design
  sketch retained in git history.)
- **L6 — Suspicious shadowing** (rebinding a name to a value of an unrelated
  type). Twinkle's semantics already make this a non-problem: `=` rebinding is
  type-preserving — the checker checks the new value against the existing
  binding's type (`check_let`, `boot/compiler/checker.tw`), so an accidental
  wrong-typed clobber is a plain *type error*, not a silent shadow. The only way
  to change a name's type is a deliberate fresh `:=` re-declaration, which is
  idiomatic (like Rust's `let` shadowing) and, being statically checked at every
  downstream use, cannot silently corrupt logic. A shadowing lint would only
  fight Twinkle's core rebinding idiom and generate noise — so it is dropped.

Explicitly **out of scope**:

- Formatting / *layout* (owned by `twk fmt`).
- Meaning-preserving automatic rewrites (owned by `twk fix`, see
  [`fix.md`](fix.md)) — e.g. the free-call → method-call rewrite. The linter may
  *flag* an anti-pattern, but anything it can *safely auto-apply* belongs in the
  fixer, not here.
- Unused bindings / params / imports / private fns. Unused-import detection
  already ships as a `build`/`check` warning (`compiler/unused_imports.tw`); its
  *removal* is owned by `twk fix` (see [`fix.md`](fix.md), R2). The rest of this
  family is deliberately deferred.
- Persistent-collection performance lints (`concat`/append-in-loop, etc.).
- **Redundant / unreachable `case` arms** and **non-exhaustive matches** —
  already implemented as the hard errors `UnreachableCaseArm` and
  `MissingVariants` in `lib/source/diagnostics.tw`. No linter work needed.

## What already exists (and what we reuse)

The diagnostic plumbing is done; lints are mostly new *producers* on existing
rails:

- `lib/source/diagnostics.tw` — `Report`, `SpanLabel`, and the rendering helpers.
  **Lint findings reuse these for output** so `twk lint` looks like the rest of
  the compiler's diagnostics — but they carry no `SuggestedFix` (lints don't
  auto-fix) and do *not* join the general `DiagKind` stream (see below), so the
  dozens of exhaustive `case DiagKind` sites stay untouched.
- `compiler/query/analyze.tw` — `analyze_module_impl` already runs a
  post-typecheck analysis (`check_unused_imports`) on non-internal modules. The
  structural lints hook in alongside it, **but only in lint mode**.
- `compiler/unused_imports.tw` — the reference implementation for an AST-walking
  analysis that produces span-carrying findings.

What we add: a **separate lint channel**, distinct from the compiler's
`warnings`/diagnostics. `PipelineArtifacts` gains a `lints` vector, populated
only when the pipeline runs in **lint mode** — a flag set exclusively by the
`twk lint` entry point. `build`/`check` leave lint mode off, so lints cost
nothing and surface nowhere outside `twk lint`. No new `DiagKind` arm is needed:
findings are their own small type, rendered through the shared `Report` path at
`twk lint` time. (`twk fix` reuses this same lint-mode/sink machinery for its own
rewrites — see [`fix.md`](fix.md) — but applies edits instead of printing.)

## Architecture

There are two natural homes for a lint, chosen per-lint by whether it needs
inferred types:

### A. Type-dependent lints → emitted by the type checker (L1, L2)

L1 and L2 need the inferred type of a statement-position expression (is it
`Void`? is it `Result`/`Option`? is the callee effectful?). The type checker
already computes exactly this at the moment it checks each statement. Computing
the finding there — gated behind `InferCtx.lint_mode` and collected into the
lint sink, *not* the `diags` the checker returns — is both the cheapest and the
most accurate option, and avoids a second typed traversal. When lint mode is off
(every `build`/`check`) the check is a single skipped branch.

This requires the checker to know, per call, whether the callee is *pure*. We
already distinguish the effectful builtins (`print`, `println`, `error`) ; the
proposal is a small **effect flag on function signatures**: a function is
"effectful" if its body transitively calls an effectful builtin, else pure.
This can start coarse (only the three builtins are effectful; everything calling
them transitively is effectful; closures conservatively effectful) and be
refined later. Until that analysis exists, L1 can ship in a conservative form
(see Rollout).

### B. Structural lints → a dedicated AST visitor module (L3, L4)

These are syntactic/structural and best run on the parsed `Module`, mirroring
`unused_imports.tw`. New module `compiler/lint.tw` exposes:

```
pub fn lint_module(module: Module, env: ResolvedEnv) Vector<LintFinding>
```

invoked from `analyze_module_impl` right after `check_unused_imports` — but
**only in lint mode**, and gated on `!dep_plan.is_internal`. Because this pass
runs only when `twk lint` asks for it, it needs no per-finding gating. It
receives the resolved `env` so it can answer structural questions like "does this
returned record type match this parameter's type?" (L3).

Both homes feed the same lint sink, so there is one rendering path.

## Lint catalog

Each lint specifies its trigger, rationale, in-source escape hatch, and where it
lives. All shipped lints are always on; there is no per-lint severity (see
Motivation).

### L1 — Discarded pure result  (`unused-result`)

- **Trigger**: a statement whose value is an expression (not a `let`/`for`/
  `return`/assignment), whose type is not `Void`, and whose outermost form is a
  call to a *pure* function (or a pure operator/field access).
- **Rationale**: pure computation evaluated for nothing is dead code or a
  forgotten rebinding (`items.append(x)` instead of `items = items.append(x)`).
- **Escape**: explicitly bind the value (see Suppression — discard binding).
- **Home**: type checker (A).

### L2 — Ignored `Result` / `Option`  (`unused-must-use`)

- **Trigger**: a statement-position expression of type `Result<_, _>` or `T?`
  that is neither `try`-ed nor pattern-matched nor bound. A strict subset of L1
  conceptually, but kept distinct because it fires even for *effectful* callees
  (ignoring an error is a bug regardless of side effects) and warrants a
  sharper message.
- **Rationale**: silently dropping an error/`None` is a correctness hazard.
- **Escape**: handle it (`try`, `case`, `.ok_or`, …) or bind it explicitly.
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
- **Help line**: show the rebinding rewrite.
- **Home**: AST visitor (B), using `env` to confirm the nominal type match.

### L4 — Unreachable statements  (`unreachable-code`)

- **Trigger**: any statement that follows a diverging statement in the same
  block — `return`, `break`, `continue`, or a call to `error(...)`/other trap.
- **Rationale**: the trailing code never runs; usually a logic error.
- **Escape**: delete the dead code.
- **Home**: AST visitor (B). Complements the existing `UnreachableCaseArm`
  error, which covers only `case` arms. *Verify during implementation whether
  any general unreachable-after-divergence detection already exists; this lint
  fills that gap if not.*

*(L5 — wildcard over project-local enum — and L6 — suspicious shadowing — are
both rejected; see Scope → Rejected. The inherent-method-call rewrite moved to
`twk fix`; see [`fix.md`](fix.md).)*

## Suppression model (no config, no attributes)

There is **no per-lint config and no `@allow(...)` attribute** — those are
exactly the knobs the always-on rule rejects (see Motivation). A lint that needs
a switch to be tolerable is a lint that should be dropped.

The only sanctioned way to silence a finding is **in-source intent that reads as
ordinary code** — and it applies only to the value-discard lints (L1/L2), where
discarding can be deliberate. The way to say "I really mean to throw this away"
is to *bind* the value:

```tw
_ := some_pure_call(x)      // explicit discard — silences L1/L2
```

`_` as a let target is a discard, not a usable name. This is the single language
touch the linter motivates; it is intent expressed in the code, not lint
metadata. **Open question** (below): adopt `_ :=` discard, or require binding to
a named throwaway instead.

Every other lint (L3, L4) has a precise enough trigger that no escape is needed —
fixing the flagged construct removes it outright. If a rule ever seems to *need* a
blanket escape, that is the signal to drop the rule, not to add one.

## Surfacing

Every lint surfaces through exactly one path: a **separate lint sink**
(`PipelineArtifacts.lints`), populated only when the pipeline runs in **lint
mode**, and reported only by `twk lint`. Lints never ride the compiler's
`warnings`/diagnostics channel, never appear in `build`/`check`, and are not
published as ambient LSP diagnostics. They reuse the `Report` rendering for
output — that is sharing the formatting, not the diagnostic path.

This keeps the everyday compile loop quiet and fast (lint mode off ⇒ home-A
checks are skipped branches, the home-B pass never runs) and makes the lints a
deliberate, opt-in step exactly like `cargo clippy`.

## Command surface

- **`twk lint <file>`** — new command (`commands/lint.tw`): runs the frontend in
  **lint mode** (analysis only, no codegen), prints every finding, and exits
  non-zero if any finding fired (all lints are always on, so there is no severity
  to consult). This is the sole surface for lints and the CI entry point.
- **`twk build` / `twk check`** — unchanged. They show only the pre-existing
  compiler warnings (unused imports, etc.); lints are invisible because lint mode
  is off and never computed.
- **LSP** — lints are *not* published as ambient diagnostics. If surfaced at all
  it would be an explicit, on-demand lint request (future work), keeping editors
  quiet by default. Lints offer no code-action fixes (they don't auto-fix); that
  is `twk fix`'s job.

`twk fmt` and `twk fix` are separate tools.

## Rollout

Ship in stages, validating signal-to-noise on `boot/` itself at each step
(it is a large real codebase — if a lint is noisy there, it is miscalibrated):

1. **Plumbing + L4 (unreachable) + L3 (record-copy)** — pure AST lints, no type
   or effect analysis required, near-zero false positives. Adds `compiler/lint.tw`
   + the `LintFinding` type, the lint sink (`PipelineArtifacts.lints`) +
   `InferCtx.lint_mode`, the lint-mode-gated `analyze_module_impl` call, and
   `twk lint`.
2. **L2 (ignored `Result`/`Option`)** — checker computes the finding; needs only
   the inferred statement type, which the checker already has. Decide the discard
   escape here (drives the `_ :=` open question).
3. **L1 (unused pure result)** — needs the purity/effect flag on signatures.
   Until that lands, optionally ship a conservative L1 limited to calls of
   functions known-pure structurally (no transitive `print`/`error`).

Each lint is gated on `boot/` before it ships: if it is noisy there, it is
recalibrated or dropped — never demoted behind a config switch.

(The lint-mode/sink plumbing built in Stage 1 is shared by `twk fix`; the
inherent-method-call rewrite ships on the `twk fix` track — see [`fix.md`](fix.md).)

## Open questions

- **Discard syntax**: adopt `_ :=` / `_ =` as a discard binding (clean, but a
  small parser/checker change), or require a named throwaway and accept that
  L1/L2 have no terse escape? Leaning toward `_ :=`.
- **Effect analysis granularity** for L1: is "transitively calls `print`/
  `println`/`error`" a good enough purity approximation, or do we need to treat
  host externs / closures more precisely?
- **L3 threshold**: exact cutoff for "mostly verbatim copies" before flagging,
  tuned against false positives on real constructors in `boot/`.
- **Lint-sink plumbing**: exact shape of the separate channel — a `lints` vector
  on `PipelineArtifacts` + an `InferCtx.lint_mode` flag is the working proposal.
  Confirm the checker can carry `lint_mode` cheaply (one branch per call site,
  skipped entirely when off) without threading churn.
