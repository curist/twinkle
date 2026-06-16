# Fixer (`twk fix`) — Design Plan

Status: proposal. Target implementation: boot compiler (`boot/`).

## Motivation

Some code is *correct* but has a better spelling — and the better spelling is a
**provably meaning-preserving rewrite** the tool can just apply. That is a
fundamentally different job from the linter:

- **`twk lint`** *detects* suspected bugs / anti-patterns and **never mutates**
  your code (the correct fix depends on intent it can't infer — see
  [`linter.md`](linter.md)).
- **`twk fix`** *applies* rewrites that cannot change behavior, so they are safe
  to run unattended (CI, pre-commit). This is the `cargo fix` / `gofmt` family.
- **`twk fmt`** owns *layout*. `fix` is the type-aware sibling: rewrites `fmt`
  cannot do because `fmt` has no typechecker.

### The safe-rewrite bar

A rewrite belongs in `twk fix` **only if** applying it is meaning-preserving on
already-correct code — no intent-guessing, and it cannot introduce or hide a bug.
This is the exact line that separates `fix` from `lint`:

- If the transform is *unambiguously* the same program (e.g. a call written two
  equivalent ways), it is a fixer rewrite.
- If the "fix" is a *guess at what the author meant* (delete unreachable code,
  rebind a discarded result, handle an ignored `Result`), it is **not** a
  rewrite — it is a lint, because auto-applying the wrong guess cements the bug.

So the catalogs are disjoint by construction, and the test for membership is
"could applying this ever be wrong?" If yes → linter.

## Command surface

- **`twk fix <file>`** — runs the frontend (analysis only, no codegen) in **fix
  mode**, computes the safe rewrites, and **applies** their edits to the file in
  place. Idempotent: running it again is a no-op.
- **`twk fix --check <file>`** — computes rewrites but applies nothing; exits
  non-zero if any rewrite is pending (and prints what/where). The CI gate and
  pre-commit check.
- Optionally `--diff` / `--dry-run` to preview edits without writing.

`twk fmt` and `twk lint` are separate commands. Recommended order in tooling:
`twk fix` (rewrites) then `twk fmt` (normalizes layout of the rewritten code).
Both are idempotent, so order only matters for fewest passes.

## Architecture

`twk fix` is a generic **collect-edits → apply-to-disk** engine fed by multiple
rewrite producers. It runs the frontend once, harvests every safe `FixEdit`, and
applies them per file. The apply step already exists — `commands/fix_unused_imports.tw`
sorts edits end-to-start and splices them offset-stably — and generalizes into
`commands/fix.tw` (see R2).

Two producer kinds feed the engine:

1. **Diagnostic-attached fixes** — some analyses already emit their edits as
   `data.fixes` on a diagnostic (unused-imports does this today). `twk fix`
   harvests those directly; no new plumbing.
2. **Rewrite sink** — rewrites that need extra computation run behind a `fix_mode`
   flag (reusing the linter's analysis-mode/sink machinery, see
   [`linter.md`](linter.md) → "What we add") and drain into a dedicated sink.
   Used by R1, whose detection needs *call-resolution time* in the checker
   (pre-lowering — the free-call form and the resolved callee/receiver type only
   coexist there). `build`/`check` leave `fix_mode` off, so they never pay for it.

Shared pieces:

- **Rewrite type** (`boot/lib/source/rewrite.tw`): `Rewrite`, one variant per
  sink-based rewrite rule, each projecting to a `SuggestedFix` via
  `fixes(rewrite)`. Reuses `report.{FixEdit, SuggestedFix}`.
- **Application** (`commands/fix.tw`): merge all `FixEdit`s for a file from both
  producers, assert non-overlap, apply offset-stably (end-to-start), write the
  file. `--check` skips the write and reports instead.

## Catalog

### R1 — Inherent-method-call rewrite  (`inherent-method-call`)

`Vector.map(xs, f)` → `xs.map(f)`, `point.translate(p, …)` → `p.translate(…)`,
bare `translate(p, …)` → `p.translate(…)`. Fires only when the receiver-method
form provably resolves to the *same* function and the receiver is postfix-atomic
(so the reorder can't reparse with different precedence). Provably
meaning-preserving; fails closed.

**Full design**: [`inherent-method-hint.md`](inherent-method-hint.md) — the
trigger predicate, the call-resolution emission sites in `checker.tw`, the
two byte-offset edits, and the tests. (The pure pieces — the `fixes()` edit
projection and the `is_postfix_atomic` guard — are already implemented and tested;
they re-home from `lib/source/lint.tw` into `lib/source/rewrite.tw`.)

### R2 — Unused-import removal  (`unused-imports`)

Remove imports a module never uses. Already implemented as a working
apply-to-disk fixer (`commands/fix_unused_imports.tw` + `compiler/unused_imports.tw`),
today reachable only via `twk check --fix-unused-imports`. The clean migration
makes `twk fix` its **single** home:

- The unused-import *warning* keeps showing in `build`/`check` (pre-existing
  compiler warning, untouched — that is the *detection*).
- The *removal* moves to `twk fix`: its edits are already emitted as `data.fixes`
  on the `UnusedImport` warning, which the fix engine harvests (producer kind 1).
- **`twk check --fix-unused-imports` is removed.** Its `collect_fixes` /
  `apply_fixes` logic generalizes into `commands/fix.tw`; `commands/check.tw`
  drops the flag.

So `twk fix` launches owning both R1 and R2 — it is the one place that applies
safe rewrites, with no leftover fix flag on `check`.

## Rollout

1. **Engine + R2 migration** — build `commands/fix.tw` (the collect→apply engine,
   generalized from `fix_unused_imports.tw`) and `twk fix` / `twk fix --check`;
   migrate unused-imports onto it and remove `twk check --fix-unused-imports`.
   R2 needs no `fix_mode` (its edits ride existing warning `data.fixes`).
2. **R1 inherent-method-call** — add the `fix_mode` flag + rewrite sink, the
   `Rewrite` type + `fixes()` (re-homed from `lib/source/lint.tw`), and the
   call-resolution detection; feed the sink into the engine. Validate idempotence
   and non-overlap on `boot/`.

## Open questions

- **Apply-by-default vs require confirmation**: `twk fix` writing in place by
  default (like `gofmt -w`) vs defaulting to `--check` and requiring an explicit
  `--write`. Leaning apply-by-default with `--check` for CI, matching `fmt`.
- **Project-wide vs single-file**: first cut is per-file; whether `twk fix` walks
  the project (like `build`) is a later question.
- **Is the inherent-method rewrite always *wanted*?** It is always *safe*
  (meaning-preserving). Whether to canonicalize universally (like `fmt`) is a
  style call; because `fix` is invoked deliberately, shipping it as an applied
  rewrite is fine — nobody is forced into it on `build`.

## Non-goals

- Layout / formatting (owned by `twk fmt`).
- Detecting suspected bugs (owned by `twk lint`).
- Any rewrite whose correctness depends on author intent — that is a lint, by the
  safe-rewrite bar above.
