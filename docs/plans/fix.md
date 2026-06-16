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

`twk fix` reuses the linter's **analysis-mode + sink** machinery (see
[`linter.md`](linter.md) → "What we add"): the frontend runs with a mode flag set
only by `twk fix`, computes findings into a separate sink, and `build`/`check`
never pay for it. The difference from `twk lint` is purely the consumer — `fix`
**applies** the collected `FixEdit`s to the source bytes instead of printing.

- **Rewrite type** (`boot/lib/source/rewrite.tw`): `Rewrite`, one variant per
  rewrite rule, each projecting to a `SuggestedFix` (one or more non-overlapping
  `FixEdit`s) via `fixes(rewrite)`. Reuses `report.{FixEdit, SuggestedFix}`.
- **Detection** lives wherever the rewrite's preconditions are available. For the
  inherent-method-call rewrite that is *call-resolution time* in the checker
  (pre-lowering), because the rewrite needs both the syntactic free-call form and
  the resolved callee/receiver type, which only coexist there.
- **Application** (`commands/fix.tw`): collect all `FixEdit`s for the file, assert
  they are non-overlapping, apply them in a single offset-stable pass (apply
  right-to-left or adjust offsets), write the file. `--check` skips the write.

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

### Future candidate — fold in unused-import removal

`twk check --fix-unused-imports` already exists (`compiler/unused_imports.tw`) and
is a textbook safe rewrite. Migrating it under `twk fix` (so `fix` is the single
home for "apply safe rewrites") is a natural follow-up, out of scope for the
first cut.

## Rollout

1. **Plumbing + R1** — fix-mode flag + rewrite sink (shared with the linter's
   Stage 1 plumbing), the `Rewrite` type + `fixes()` (re-homed), the
   call-resolution detection for inherent-method-call, and the `twk fix` /
   `twk fix --check` command. Validate idempotence and non-overlap on `boot/`.
2. **Migrate unused-imports** into `twk fix` (optional, later).

## Open questions

- **Apply-by-default vs require confirmation**: `twk fix` writing in place by
  default (like `gofmt -w`) vs defaulting to `--check` and requiring an explicit
  `--write`. Leaning apply-by-default with `--check` for CI, matching `fmt`.
- **Fold unused-imports now or later**: ship R1 alone first, or migrate the
  existing fixer in the same cut so `fix` launches with two rewrites.
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
