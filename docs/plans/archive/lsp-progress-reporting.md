# LSP Diagnostics Progress Reporting

## Goal

Make `twk lsp` diagnostics progress useful and honest while workspace analysis is
running. Today the server emits a fixed early progress update, runs the whole
synchronous diagnostics pipeline, then completes. Editors commonly show this as
"10%, then done", which is accurate to the implementation but not useful to a
person waiting for diagnostics.

The desired behavior is spinner-style progress with richer messages such as:

* `Preparing diagnostics`
* `Parsing compiler.parser`
* `Resolving imports for boot.commands.lsp`
* `Typechecking compiler.query.analyze`
* `Publishing diagnostics`

Percentages are intentionally optional. Unless we add a separate pre-walk to
count work, any percentage would be a heuristic. Rich messages are more honest
and usually more helpful.

---

## Current Behavior

The LSP transport layer in `boot/commands/lsp.tw` owns work-done progress:

* `begin_progress(...)` emits a `$/progress` `begin` notification.
* `report_progress(...)` emits a `$/progress` `report` notification.
* `end_progress(...)` emits a `$/progress` `end` notification.

`run_diagnostics_job(...)` currently reports only coarse fixed milestones around
`ctx.shared.get().publish_due_diagnostics()`:

* begin at `0%`, `Preparing diagnostics`
* report `10%`, `Analyzing open documents`
* run full diagnostics synchronously
* report `90%`, `Publishing diagnostics`
* end at `100%`, `Diagnostics ready`

The expensive work is inside `publish_due_diagnostics()` in
`boot/lib/lsp/server_core.tw`, specifically the full tier:

* `publish_workspace_diagnostics(...)`
* `compiler.query.diagnostics.analyze_workspace(...)`
* `compiler.query.analyze.analyze_module(...)`

That recursive analysis currently has no progress hook, so the LSP cannot emit
module/stage updates while parsing, resolving dependencies, or typechecking.

---

## Design Direction

### Prefer indeterminate progress

LSP work-done progress allows `percentage` to be omitted. Twinkle should use
that for diagnostics analysis so clients can show a spinner/progress item
without misleading percentages.

`end` notifications may also omit `percentage`; completion is represented by the
`end` kind itself.

### Add a compiler-neutral progress sink

Add a small, LSP-independent progress sink to the query/analyze path. The sink
should describe compiler work without depending on JSON-RPC or editor concepts.

A likely shape:

```tw
type ProgressSink = fn(AnalysisProgress) Void

type AnalysisProgress = .{
  stage: AnalysisStage,
  module: String,
}

type AnalysisStage = {
  Loading,
  Parsing,
  PlanningImports,
  AnalyzingDependencies,
  Resolving,
  Typechecking,
  Publishing,
}
```

The exact names can be adjusted during implementation. The important constraint
is that this type lives in the compiler/query layer and remains transport-neutral.

The default path should remain no-op so existing build, CLI, and tests do not
need to care about progress.

### Thread progress through diagnostics analysis

Thread the sink through:

* `compiler.query.diagnostics.analyze_workspace(...)`
* `compiler.query.analyze.new_state(...)` or a `with_progress(...)` helper
* `compiler.query.analyze.analyze_module(...)` / internal recursive helpers

Emit progress at coarse module-stage boundaries only. Avoid expression-level,
function-level, or diagnostic-level progress because that would be noisy and
would add overhead in hot paths.

Good emission points in `compiler/query/analyze.tw` are the existing boundaries:

* before/after source load
* before parse
* before dependency planning
* before recursive dependency analysis
* before local resolve/typecheck
* before interface publication

### Keep server_core mostly reusable

`boot/lib/lsp/server_core.tw` is currently the pure message/state layer: it
returns outgoing JSON after work completes. Real-time progress requires a way to
call back during analysis, so avoid forcing progress into every existing caller.

A clean approach is to add a progress-aware variant next to the current API:

```tw
pub fn publish_due_diagnostics_with_progress(
  state: State,
  progress: fn(query_diagnostics.ProgressEvent) Void,
) Step
```

or an optional sink parameter if the language ergonomics are better.

The existing `publish_due_diagnostics(state)` can delegate to the progress-aware
variant with a no-op sink. Tests and non-LSP code keep using the current entry
point.

### Throttle in the LSP command layer

The compiler can emit every coarse stage transition; the LSP command should
avoid sending every event to the client.

Throttle in `boot/commands/lsp.tw`, where real I/O already happens:

* send the first meaningful analysis message immediately
* then send only when the message changed and enough time has elapsed
* always send terminal messages such as `Publishing diagnostics`,
  `Diagnostics ready`, and `Diagnostics superseded`

This keeps the compiler/query layer simple and lets the transport layer own
client-facing noise control.

---

## Proposed Implementation Slices

### Progress notification shape

Update `progress_notification(...)` in `boot/commands/lsp.tw` so `percentage` is
optional.

* `begin_progress(...)` can still accept a title/message, but should not require
a percentage.
* `report_progress(...)` should support message-only reports.
* `end_progress(...)` should not need to force `100`.

A helper can keep fixed-percentage support available if another future operation
has real progress, but diagnostics should use message-only progress.

### Analysis progress model

Introduce a small progress model in the query layer, likely in
`boot/compiler/query/analyze.tw` or a new nearby module if it helps avoid import
cycles.

Requirements:

* no dependency on LSP protocol modules
* no JSON construction
* stable enough that diagnostics, semantic snapshots, and future editor features
can share it
* cheap no-op behavior when unused

### Progress-aware workspace diagnostics

Thread the sink from `query_diagnostics.analyze_workspace(...)` into
`analyze.analyze_module(...)`.

`analyze_workspace(...)` can also emit workspace-level messages such as
`Preparing diagnostics` or `Reusing cached dependency closure` if useful, but the
most valuable messages are module/stage-level events from `analyze.tw`.

### LSP progress bridge

In `boot/commands/lsp.tw`, create a small bridge from compiler progress events
to user-facing strings.

Examples:

* `Loading` → `Loading <module>`
* `Parsing` → `Parsing <module>`
* `PlanningImports` → `Resolving imports for <module>`
* `AnalyzingDependencies` → `Analyzing dependencies for <module>`
* `Resolving` / `Typechecking` → `Typechecking <module>` if those are coupled in
  the current implementation, or separate messages if the boundary is available
* `Publishing` → `Publishing diagnostics`

The bridge should also own throttling state, probably local to
`run_diagnostics_job(...)`.

### Supersede behavior

Keep the existing generation check semantics:

* if a newer diagnostics generation appears before publication, end the current
  progress token as `Diagnostics superseded`
* do not publish stale diagnostics

Because analysis is synchronous once started, progress can still continue while a
newer generation is waiting. That is acceptable as long as the final generation
check remains authoritative.

---

## Testing Strategy

Prefer tests at the pure/protocol boundary where possible:

* progress notification JSON omits `percentage` when not provided
* the diagnostics path accepts a progress sink and invokes it for module/stage
  analysis boundaries on a small import graph
* `publish_due_diagnostics(...)` still works without a progress sink
* LSP progress bridge formats representative events into stable user-facing
  messages
* throttling suppresses repeated rapid messages but allows forced terminal
  messages

End-to-end editor behavior can be smoke-tested manually with a workspace large
enough that diagnostics progress remains visible.

---

## Non-goals

* Exact percentages for diagnostics analysis.
* Pre-walking the module graph only to count work.
* Progress updates for parse-only edit diagnostics; those are expected to be
  quick and currently do not show progress.
* Cancellation. Superseding diagnostics is still generation-based, not a true
  cancellation mechanism.
* Moving compiler analysis onto another thread. The current cooperative task
  model remains unchanged.

---

## Resolved Decisions (post-code-review, 2026-06-20)

The plan above was validated against the current code. Conclusions:

### Use delegating variant functions at every layer — do not add sink params to existing signatures

This is the central implementation constraint. The analysis entry points have many
callers that must keep working unchanged:

* `query_diagnostics.analyze_workspace(store, input, snapshot)` — called by
  `compiler/query/semantic.tw` plus ~25 sites in `query_diagnostics_suite.tw`.
* `server_core.publish_due_diagnostics(state)` — called by `lsp.tw` and ~10 LSP
  test suites (`lsp_*_suite.tw`).

Adding a sink parameter to these would force edits across all those sites. Instead,
mirror the pattern the plan already proposes for `server_core`: keep each existing
function as a thin wrapper that delegates to a new `_with_progress` variant with a
`.None` sink. Chain of new variants:

* `lsp.run_diagnostics_job` builds the sink
* → `server_core.publish_due_diagnostics_with_progress(state, sink)`
* → `server_core.publish_workspace_diagnostics_with_progress(state, sink)`
* → `query_diagnostics.analyze_workspace_with_progress(store, input, snapshot, sink)`
* → `analyze.new_state(...).with_progress(sink)`

Each plain function (`publish_due_diagnostics`, `publish_workspace_diagnostics`,
`analyze_workspace`) becomes `... = ..._with_progress(args, .None)`.

### Carry the sink inside `AnalysisState`, not as a threaded parameter

`AnalysisState` is already the record threaded through every recursive
`analyze_module_impl` call (and into `analyze_dependencies`). Add an optional field
`progress: ProgressSink?` (default `.None`) plus a `with_progress(state, sink)`
builder mirroring the existing `with_snapshot_capture(state)`. This means the sink
reaches every recursion point for free — no extra params on `analyze_module` /
`analyze_module_impl` / `analyze_dependencies`. Emit via a single ctx-first helper
`state.note_progress(stage, canonical)` that no-ops when the field is `.None`.

The sink is never captured into `ClosureSnapshot` (snapshots copy only
exports/interfaces/types/etc.), so snapshot reuse is unaffected.

### Progress model lives in a new `boot/compiler/query/progress.tw`

Keeps the type transport-neutral and avoids import-cycle risk between `analyze.tw`,
`diagnostics.tw`, and the LSP command layer. Shape:

```tw
pub type ProgressSink = fn(AnalysisProgress) Void
pub type AnalysisProgress = .{ stage: AnalysisStage, module: String }
pub type AnalysisStage = {
  Loading, Parsing, PlanningImports, AnalyzingDependencies, Resolving, Publishing,
}
```

(`Resolving` covers the combined resolve+typecheck step — the current
`resolve_and_check_local` boundary does not separate them, so a single stage is
honest. Add a separate `Typechecking` stage only if that boundary is later split.)

### Emission points in `analyze_module_impl`

Use the existing timing boundaries, emitting before each:

* `Loading` before `load_source` (`t_load0`)
* `Parsing` before `parse_cached` (`t_parse0`)
* `PlanningImports` before `plan_dependencies` (`t_plan0`)
* `AnalyzingDependencies` before `analyze_dependencies`
* `Resolving` before `resolve_and_check_local`
* `Publishing` before `publish_interface`

### Optional percentage

Change `percentage` to `Int?` in `progress_notification`, `begin_progress`,
`report_progress`, and `end_progress`; omit the JSON `percentage` key when `.None`.
Diagnostics progress passes `.None` (indeterminate / spinner). Keep the ability to
pass a concrete `Int` for any future real-progress operation.

### Open questions — resolved

* **Module name format:** the canonical path is an *absolute* filesystem path
  (`canonical_module_path` just normalizes), which is too noisy to show. Emit a
  display-friendly name instead: strip the project root (so a workspace module
  shows as `commands/lsp.tw`), or the stdlib root (shown as `@std/...`) or prelude
  root for internal dependencies, falling back to the bare filename. This is done
  at the emission point (`note_progress` in `analyze.tw`, which has
  `project_root` + `canonical_roots`); the `AnalysisProgress.module` field
  therefore carries a display name, not a path/identity.
* **Dependency-closure reuse message:** stay silent. Warm reuse is fast and only
  the entry module re-analyzes; the workspace-level `Preparing diagnostics` plus
  per-module stages are enough.
* **Throttle interval:** a single tunable const (start at ~50ms) gating distinct
  messages; send the first message immediately and always force terminal messages
  (`Publishing diagnostics`, `Diagnostics ready`, `Diagnostics superseded`).
* **Progress model location:** dedicated `compiler/query/progress.tw` (above).

### Scope note

All changes are pure Twinkle in `boot/`. No host builtins or intrinsics are added,
so the Rust stage0 needs no changes beyond bootstrapping the updated `boot/main.tw`.
