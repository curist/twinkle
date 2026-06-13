# Deduplicate diagnostics from failed shared modules

Status: **completed and archived**. Small, self-contained correctness fix in the
frontend analysis driver. No language-surface change.

## Problem

When a module that fails analysis (parse or type error) is imported by more than
one other module, its diagnostics are reported **once per importing edge** instead
of once. Building `boot/main.tw` with a single broken line in a widely-imported
file like `checker.tw` prints the same error four times; a shared dependency
imported by N modules prints N copies.

Reproduction (a dependency imported by three siblings):

```
twinkle.toml
bad.tw          # contains one parse or type error
a.tw  → use .bad
b.tw  → use .bad
c.tw  → use .bad
main.tw → use .a / .b / .c
```

`twk build main.tw` emits the `bad.tw` error **three times**.

## Root cause

In `boot/compiler/query/analyze.tw`, `analyze_module_impl` memoizes a module as
"already analyzed" only on **success**:

```tw
// "Already analyzed? Return cached exports."
case state.exports[canonical] {
  .Some(_) => { return .{ env: base, state, diagnostics: [], ok: true } },
  .None => {},
}
```

`state.exports[canonical]` is populated only after a module fully resolves and
type-checks. A module that fails returns early through one of the `.Err` paths
(`load_source`, `parse_cached`, `plan_dependencies`, resolve, check) and is
**never recorded as visited**. So the dependency-graph walk re-enters the failing
module once per importer:

- Parsing itself *is* cached (`store.parsed`), but `parse_cached` re-wraps the
  cached parse diagnostics and re-returns `.Err` on every visit.
- `analyze_dependencies` concatenates each visit's diagnostics
  (`all_diags = .concat(dep_result.diagnostics)`), yielding N copies.

A *successful* shared dependency does not duplicate — it short-circuits with
empty diagnostics on the second visit. Only the failure path lacks memoization.

## Fix — memoize attempted-and-failed modules

Mirror the success-path memoization for failures: record each module that has
been **attempted and failed**, and on a repeat visit return `ok: false` with
**empty diagnostics** (the first visit already reported them). Importers still see
the failure and propagate it; they just don't re-emit the dependency's errors.

This also stops redundant re-analysis of broken subgraphs — when a core file is
broken, every dependent currently re-walks it. Memoizing the failure makes the
broken file analyzed at most once.

### Sketch

1. Add a failure-memo to `AnalysisState`, e.g. `failed: Dict<String, Bool>`
   (keyed by canonical module path), alongside the existing `exports`/`interfaces`
   maps.
2. At the top of `analyze_module_impl`, after the `state.exports` short-circuit,
   add a `state.failed[canonical]` short-circuit that returns
   `.{ env: base, state, diagnostics: [], ok: false }`.
3. At each failure return in `analyze_module_impl` (the `.Err` arms for
   `load_source`, `parse_cached`, `plan_dependencies`, and the resolve/check
   failures surfaced via `analyze_local`/`analyze_dependencies`), set
   `state.failed[canonical] = true` on the returned state before propagating.
   - Take care to thread the **updated** state (the one carrying `failed`) back
     out so the memo survives — these paths currently return `err.state`.

### Decisions to lock during implementation

- **Granularity:** memoize per *module*, not per *diagnostic*. A failing module
  reports its full diagnostic set exactly once (on first visit); repeat visits are
  silent. This is simpler than a global dedup keyed by (stage, span, message) and
  matches the existing success-path memoization shape.
- **Cycle interaction:** the failure-memo check must come *after* the existing
  `stack.contains(canonical)` cycle handling so a back-edge into an
  in-progress-but-not-yet-failed module is still handled by the cycle path, not
  swallowed as a failure.
- **State threading:** failures currently return `err.state` from helper results;
  ensure the `failed` entry is written onto that state (or onto the state the
  caller keeps) so siblings analyzed later observe it. This is the main
  correctness subtlety — a memo written onto a discarded state copy is a no-op.

### Alternative considered (rejected)

Deduplicating the final `all_diags` list before rendering (keyed by stage + span +
message). Smaller and fully localized, but it only hides the symptom: the broken
subgraph is still re-analyzed once per edge, and identical-but-legitimately-
distinct diagnostics are harder to reason about. The memo fixes the cause and is a
modest perf win on broken graphs.

## Validation

- Unit-level: the three-importer reproduction above prints each `bad.tw` error
  exactly once (both a parse-error and a type-error variant).
- Building `boot/main.tw` with a deliberately broken core file prints each error
  once rather than once per dependent.
- `make boot-test` green; `make bundle-cli` reaches the self-host fixed point.
- Confirm no regression in the happy path: a clean multi-importer build still
  compiles and the success-path short-circuit is unaffected.

## Scope

Boot compiler only (`boot/compiler/query/analyze.tw`, plus the `AnalysisState`
type). The LSP diagnostics path and the build path both flow through
`analyze_module`, so both benefit. stage0 is unaffected — this is a boot-only
driver concern.
