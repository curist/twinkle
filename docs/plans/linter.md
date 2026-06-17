# Linter (`twk lint`) — Design Plan

Status: proposal (engine + first rewrites built; see Rollout). Target
implementation: boot compiler (`boot/`).

## Motivation

`twk lint` is Twinkle's `cargo clippy`: one **opt-in review command** that, in a
single pass, reports two kinds of finding and can apply the safe ones.

- **Lints** — code that type-checks but is *suspected-buggy* or an anti-pattern
  (a discarded pure result, an ignored `Result`, a `with_*` record-copy helper).
- **Rewrites** — code that is *correct* but has a better spelling that is
  **provably meaning-preserving** (`Vector.map(xs, f)` → `xs.map(f)`; an unused
  import removed).

`twk lint` **reports both** with `file:line:col` + a description. The crucial
difference is auto-fixability, and it is the only line that matters:

> **Auto-fixability rule.** An item gets a `--fix` flag **only if** applying it is
> meaning-preserving on already-correct code — no intent-guessing, cannot
> introduce or hide a bug. If the "fix" is a *guess at what the author meant*
> (delete unreachable code, rebind a discarded result), applying the wrong guess
> cements the bug, so it is **report-only**.

Therefore: **rewrites are auto-fixable** (`--fix-<rule>` / `--fix`); **lints are
report-only** — you fix them by hand. The two catalogs are disjoint by
construction; the membership test is "could applying this ever be wrong?" — if
yes, it is a lint.

### Non-intrusive and config-free

- **Opt-in.** None of this runs in `twk build` / `twk check` or ambient
  typecheck-driven LSP diagnostics. Running `twk lint` *is* the opt-in; the
  everyday compile loop pays nothing (the detection is gated behind a review-mode
  flag that only `twk lint` sets). (Pre-existing compiler warnings — unused
  imports, single-line multiline blocks — keep showing in `build`/`check` on
  their own channel; this plan does not touch them.)
- **No persistent configuration.** No `[lint]` table, no severity knobs, no
  allow-by-default tier. A rule earns its place only by clearing a
  near-zero-false-positive bar on real code (`boot/`); a too-noisy candidate is
  **dropped, not hidden behind a flag**. The `--fix-<rule>` flags choose what to
  *apply this run* — a command-level choice, not config; detection stays
  always-on (everything is reported). The only in-source silencer is the discard
  binding for L1/L2 (see Suppression).

Twinkle's value model makes some lints far stronger than elsewhere: because
almost every function is pure, "you computed a value and discarded it" is, by
default, dead code rather than a stylistic nit.

## Catalog

### Lints (report-only — no auto-fix)

**L1 — Discarded pure result** (`unused-result`)
- *Trigger*: a statement-position expression (not `let`/`for`/`return`/assign)
  whose type is not `Void` and whose outermost form is a call to a *pure*
  function (or a pure operator/field access).
- *Rationale*: pure computation evaluated for nothing is dead code or a forgotten
  rebinding (`items.append(x)` instead of `items = items.append(x)`).
- *Escape*: explicitly bind the value (Suppression — discard binding).
- *Home*: type checker (A).

**L2 — Ignored `Result` / `Option`** (`unused-must-use`)
- *Trigger*: a statement-position expression of type `Result<_, _>` or `T?`
  neither `try`-ed, matched, nor bound. A conceptual subset of L1, kept distinct
  because it fires even for *effectful* callees (dropping an error is a bug
  regardless of side effects) and warrants a sharper message.
- *Rationale*: silently dropping an error/`None` is a correctness hazard.
- *Escape*: handle it (`try`, `case`, `.ok_or`, …) or bind it explicitly.
- *Home*: type checker (A).

**L3 — `with_*` field-copy rebuild** (`record-copy-helper`)
- *Trigger*: a function returning a record literal of the same nominal type as
  one of its parameters `p`, where most fields are verbatim `p.<field>` copies.
  CLAUDE.md's "non-trivial work" exception is respected (if any field does real
  computation, not flagged).
- *Rationale*: encodes the house rule (CLAUDE.md *Immutability and Rebinding*):
  use `p.field = v` rebinding, not copy helpers.
- *Report*: include the rebinding rewrite in the help text (illustrative; not
  auto-applied — the rewrite is intent-shaped, not mechanical).
- *Home*: AST visitor (B), using `env` for the nominal-type match.

**L4 — Unreachable statements** (`unreachable-code`)
- *Trigger*: a statement following a diverging statement (`return`, `break`,
  `continue`, `error(...)`/trap) in the same block.
- *Rationale*: the trailing code never runs; usually a logic error — and *which*
  is the bug (the early return vs. the dead code) is the author's call, so this
  is report-only.
- *Home*: AST visitor (B). Complements the existing `UnreachableCaseArm` error
  (which covers only `case` arms).

**L5 — Rebinding-through-path** (`direct-rebinding`)
- *Trigger*: a `tmp := <ident-or-field-path>` binding whose only later uses are
  rebinding updates rooted at `tmp` (`tmp[k] = v`, `tmp.x = v`,
  `cur = cur.method(...)`), completed by either copying `tmp` back to the source
  path (`reg.by_name = tmp`, `state = updated`) or returning `tmp` as the block
  tail. Straight-line only; loop bodies of pure-alias accumulators are scanned,
  but `if`/`case`/`cond`/closures/early returns are not crossed.
- *Rationale*: the temporary is just an alias; rebinding the field/index path
  directly states the house rule (CLAUDE.md *Immutability and Rebinding*) without
  the extra name. Complements L3, which catches full record reconstruction.
- *Conservative*: fails closed on any independent read of the source (or a prefix
  of it), escape of `tmp` to a non-receiver position, shadowing, or indexed
  source paths (deferred — would change call/evaluation count). See
  [archive/rebinding-through-path-lint.md](archive/rebinding-through-path-lint.md).
- *Home*: AST visitor (B); syntax/dataflow over one block, no checker needed.

### Rewrites (auto-fixable — `--fix-<rule>` / `--fix`)

**R1 — Inherent-method-call** (`--fix-inherent-calls`) — **built**
- `Vector.map(xs, f)` → `xs.map(f)`, `point.translate(p, …)` → `p.translate(…)`,
  bare `translate(p, …)` → `p.translate(…)`. Fires only when the receiver-method
  form provably resolves to the *same* function and the receiver is
  postfix-atomic (so the reorder can't reparse with different precedence).
  Provably meaning-preserving; fails closed.
- *Full design*: [`inherent-method-hint.md`](archive/inherent-method-hint.md) — predicate,
  the call-resolution emission sites in `checker.tw`, the two byte-offset edits,
  tests.

**R2 — Unused-import removal** (`--fix-unused-imports`) — **built**
- Remove imports a module never uses. The unused-import *warning* keeps showing
  in `build`/`check` (pre-existing detection); `twk lint` reports the *removal
  opportunity* and `--fix-unused-imports` applies it. Edits ride the
  `UnusedImport` warning's `data.fixes` (no review-mode needed to detect).

### Rejected

- **L5 — Wildcard `_ =>` over a project-local sum type.** A blanket catch-all is
  frequently an *intentional* default; no always-on trigger separates a lazy
  catch-all from a deliberate one without per-project opt-in, so it can't meet
  the no-config / near-zero-false-positive bar. Dropped. (Sketch in git history.)
- **L6 — Suspicious shadowing.** A non-problem in Twinkle: `=` rebinding is
  type-preserving (the checker checks the new value against the existing binding's
  type — `check_let`), so a wrong-typed clobber is already a *type error*. Only a
  deliberate fresh `:=` re-declaration changes a name's type, which is idiomatic
  (like Rust's `let` shadowing) and statically safe. Dropped.

### Out of scope

- Formatting / *layout* (owned by `twk fmt`).
- Unused bindings / params / private fns (deferred). Unused-*import* is R2.
- Persistent-collection performance lints (`concat`/append-in-loop, etc.).
- Redundant/unreachable `case` arms and non-exhaustive matches — already hard
  errors (`UnreachableCaseArm`, `MissingVariants`). No work needed.

## Command surface

- **`twk lint <file>`** — report mode (default): runs the frontend in review mode
  (analysis only, no codegen), prints every finding (lints + rewrites) as
  `file:line:col` + description, and exits non-zero if any finding fired (the CI
  gate). Auto-fixable findings are tagged with the flag that applies them. Writes
  nothing.
- **`twk lint --fix <file>`** — apply *all* auto-fixable rewrites, then report any
  remaining (non-fixable) lint findings.
- **`twk lint --fix-unused-imports <file>`**, **`--fix-inherent-calls <file>`** —
  apply one rewrite rule; report the rest. (Consistent `--fix-<rule>`.)
- Lints have no fix flag — they are always report-only.
- **`twk build` / `twk check`** — unchanged; review mode stays off, so they never
  compute or show lint/rewrite findings.
- `twk fmt` is separate (layout). Recommended tooling order: `twk lint --fix`
  then `twk fmt` (normalize the rewritten layout). Both idempotent.

There is no separate `twk fix` command: applying rewrites is `twk lint --fix*`.

## Architecture

One review pass produces findings of both kinds into a **separate sink**
(`PipelineArtifacts`), distinct from the compiler's `warnings`/`DiagKind` stream,
populated only in **review mode** (a flag set solely by `twk lint`;
`build`/`check` leave it off). No new `DiagKind` arm — findings are their own
small types, so the exhaustive `case DiagKind` sites stay untouched. Findings
reuse the `Report`/`SpanLabel`/line-index rendering for output; rewrites also
project to `report.{FixEdit, SuggestedFix}` for application.

### Detecting lints

- **Type-dependent (L1, L2)** — emitted by the type checker, which already has
  the inferred statement type at the right moment. Gated behind the review-mode
  flag on `InferCtx`; off ⇒ a single skipped branch. L1 needs a purity/effect
  flag on signatures (a function is effectful iff it transitively calls
  `print`/`println`/`error`; closures conservatively effectful); until that
  lands, L1 can ship conservatively.
- **Structural (L3, L4)** — a dedicated AST visitor (`compiler/lint.tw`,
  `lint_module(module, env)`), invoked from `analyze_module_impl` only in review
  mode, gated on `!dep_plan.is_internal`. Runs only when `twk lint` asks, so no
  per-finding gating needed.

### Detecting + applying rewrites (R1, R2)

The rewrite engine is a generic **collect-edits → apply-to-disk** step fed by two
producers:

1. **Diagnostic-attached** — edits already emitted as `data.fixes` on a
   diagnostic (R2 rides the `UnusedImport` warning). Harvested directly.
2. **Review-mode sink** — rewrites needing extra computation (R1) run behind the
   review-mode flag and drain into a dedicated sink. R1 *must* compute at
   call-resolution time in the checker (pre-lowering — the free-call form and the
   resolved callee/receiver type only coexist there).

Shared pieces (built):
- `boot/lib/source/rewrite.tw` — `Rewrite`, one variant per sink-based rewrite,
  projecting to a `SuggestedFix` via `fixes(rewrite)`; plus `is_postfix_atomic`.
- the apply step — merge all `FixEdit`s for a file from the selected rules, assert
  non-overlap, splice offset-stably (end-to-start), write. Report mode skips the
  write and renders locations instead.

## Suppression model (no config, no attributes)

No per-rule config and no `@allow(...)` attribute — those are the knobs the
no-config rule rejects. A lint that needs a switch to be tolerable should be
dropped. The only sanctioned silencer is **in-source intent that reads as
ordinary code**, and only for the value-discard lints (L1/L2):

```tw
_ := some_pure_call(x)      // explicit discard — silences L1/L2
```

`_` as a let target is a discard, not a usable name — intent expressed in code,
not lint metadata. **Open question** (below): adopt `_ :=` discard, or require a
named throwaway.

## Rollout

The command, both rewrites, and L2/L3/L4/L5 are **done**. L1 is **dropped for
now** (see §6 for the scoping reasoning); the linter is otherwise complete.

1. **Done — rewrite engine + R2 (unused-imports)** — the collect→apply engine
   (`apply_edits` end-to-start splice), R2 migrated off the removed
   `twk check --fix-unused-imports`.
2. **Done — R1 (inherent-method-call)** — review-mode flag + rewrite sink, the
   `Rewrite` type + `fixes()`, call-resolution detection, dedup by call span.
3. **Done — unified `twk lint`** — report-only default + `--fix` / `--fix-<rule>`;
   the location/description **report renderer**; `lint_mode` flag; standalone
   `twk fix` removed.
4. **Done — L4 (unreachable) + L3 (record-copy)** — pure AST visitor
   `compiler/lint.tw` (`lint_module`); report-only (no edits). Findings (rewrites
   *and* lints) now aggregate **up the import chain** — the entry module plus
   everything it imports, stdlib/internal excluded — so R1's entry-only limit is
   gone.
4b. **Done — L5 (direct-rebinding)** — same AST visitor: a `tmp := <path>`
   alias whose only uses are rebinding updates rooted at `tmp`, completed by a
   copy-back to the source path or a tail return of `tmp`. Straight-line +
   pure-alias loop bodies; fails closed on independent source reads, escapes,
   shadowing, and indexed source paths. Design:
   [archive/rebinding-through-path-lint.md](archive/rebinding-through-path-lint.md).
5. **Done — L2 (ignored Result/Option)** — emitted by the checker (home A):
   `maybe_unused_value` flags a discarded statement-position expression of type
   `Result`/`Option`. Calibrated clean on `boot/` (0 findings — guards against
   future drops).
6. **Dropped for now — L1 (unused pure result).** A conservative version (flag
   any discarded non-Void value) was tried and rejected: on `boot/` it
   false-positived on effectful calls whose value is legitimately ignored (the
   `Cell`-mutating `registry.add_file(...)` pattern — 4/5 findings). Doing L1
   *correctly* is the biggest piece in this plan, for three reasons surfaced
   while scoping it:
   - **No `FunctionSig` purity flag.** `FunctionSig` has ~60 construction sites
     plus cache serialization — too invasive/risky to carry `effectful`.
   - **It must be cross-module.** The canonical bug `items.append(x)` calls a
     *prelude* function; the `add_file` false positive lives in another module.
     So purity must be known for imported functions, i.e. accumulated across the
     import graph.
   - **It must be type-aware, not a pure-AST pass.** `.set` is ambiguous —
     `Cell.set` is effectful, `Vector.set` is pure — so purity has to key off the
     checker's *resolved* call targets (`method_calls`), not the syntax.

   So the real solution is a **whole-program purity map** (keyed by resolved
   func-name; effectful = transitively prints/errors or mutates a `Cell`),
   accumulated bottom-up through `analyze` and threaded into the checker — no
   `FunctionSig` change, but a new `AnalysisState` field, a `check()` signature
   change, an intra-module fixpoint, name-consistency work, and its own `boot/`
   calibration. Deferred until that's worth building.

   **Cheaper alternative if revived:** a *type-based* L1 — flag discarding a
   freshly-built persistent collection (`Vector`/`Dict`/`Set`/`String`) returned
   from a call. Keys off the discarded value's type (already known), no effect
   analysis; safely catches `items.append(x)` / `dict.set(k,v)` / `s.concat(t)`
   (the `add_file` result is a record, not a collection, so no false positive).
   Misses pure non-collection discards (e.g. a discarded `point.translate(...)`).

## Open questions

- **Discard syntax**: adopt `_ :=` / `_ =` as a discard binding (small
  parser/checker change), or require a named throwaway? Leaning `_ :=`.
- **`--fix` exit code**: report mode exits non-zero on any finding (CI). `--fix`
  currently exits 0 after applying (remaining report-only lints are printed).
  Revisit if CI wants `--fix` to also fail on leftover lints.
- **Scope** (resolved): findings follow the **import chain** — the entry file
  plus everything it transitively imports (stdlib/internal excluded) — not a
  directory crawl. Applies uniformly to rewrites and lints.
- **L1 is dropped for now** — see Rollout §6 for the full reasoning (cross-module
  + type-aware purity → a whole-program purity map; or a cheaper type-based
  heuristic). Revisit if the dead-code value justifies the build.
- **L3 threshold**: cutoff for "mostly verbatim copies" before flagging.
