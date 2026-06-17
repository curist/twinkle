# LSP diagnostics: dependency-closure snapshot

## Problem

The LSP republishes workspace diagnostics on every edit by calling
`query_diagnostics.analyze_workspace`, which re-walks the entry's entire
dependency graph. Parse/resolve/typecheck are cache hits (via `stage_runner`
keyed by `source_hash`/`deps_hash`/`context_hash`), but the *walk itself* re-runs
every time: `load` (disk read + `keys.hash_text` per module) ~120ms,
`plan_deps` ~180ms, and especially **`import_merge`** (rebuild each module's env
from its dependencies' interfaces, `analyze_dependencies` → `merge_import_interface`)
~390ms. Warm re-analysis after a trivial entry edit costs ~780ms.

### Why the obvious fix doesn't work

"Skip clean dependency subtrees" is blocked by **position-dependent TypeIds**.
`analyze_module_impl` computes `local_type_start := deps.env.types.len()`
(analyze.tw:385) *after* import merges, and folds it into `context_hash`
(`keys.mix_word(...)`, :391) — the cache key for resolved/typed.
`merge_module_exports` → `register_imported_interface_types` → `register_type_entry`
grows `env.types` (resolver.tw:439). So a module's cache key cannot be reproduced
without first redoing its merges. The ~390ms is structurally on the critical path
per-module.

## Approach: snapshot the dependency-closure analysis state

For the LSP editing pattern, the *same entry* is edited repeatedly while its entire
dependency closure stays byte-identical. So: after analyzing the entry's
dependencies once, **snapshot the resulting analysis state**, and on a re-edit
where the closure is unchanged, **restore the snapshot and re-run only the entry's
own module** (its direct-import merge + resolve + typecheck + publish). This
sidesteps the TypeId-position issue entirely: `shared_types` are restored
verbatim, so `local_type_start` for the entry is reproduced exactly.

The entry's own direct-import merge still runs, but that is a small fraction of the
390ms (which is summed over all ~213 modules); only the entry's direct imports are
merged.

### Snapshot contents (`analyze.ClosureSnapshot`)

Captured *after* `analyze_dependencies(entry)` returns and *before* the entry's own
`resolve_and_check_local`:

- `entry: String` — canonical path of the entry.
- `entry_dep_paths: Vector<String>` — the entry's planned direct-dependency paths
  (to detect when the entry's import list changes).
- `closure_modules: Vector<String>` — every non-entry module analyzed (= snapshot's
  `module_order` minus entry). Used for validation.
- The reusable analysis-derived state: `exports`, `interfaces`, `shared_types`,
  `shared_type_origins`, `module_order`, `module_runners`, `failed`.
- `dep_diagnostics: Vector<AnalysisDiag>` — diagnostics accumulated from the
  closure (e.g. unused-import warnings) so the full diagnostic set is reproduced
  when the snapshot is reused.

The snapshot is **not** stored in `cache.Store` (would create a circular import —
`cache.tw` can't reference `analyze` types). It lives in `analyze.tw` and is carried
by `server_core.State` (alongside `query_cache`), threaded through
`analyze_workspace` in/out.

### Validity check (cheap, O(closure))

The Store's dirty-tracking is already wired: `note_source_hash` →
`invalidate_changed_module` → `graph.reverse_dependents_closure` clears
`module_hashes` for the changed module **and everything that transitively imports
it**. Therefore `store.module_hash(m)` present ⟺ `m` and its whole import subtree are
unchanged.

A snapshot is reusable iff:
1. `snapshot.entry == current entry`, and
2. the entry's freshly planned `entry_dep_paths` set is unchanged (entry's import
   list didn't change), and
3. every `m` in `snapshot.closure_modules` still has `store.module_hash(m).is_some()`.

If any check fails → discard snapshot, do a full analysis, capture a fresh snapshot.

### Reuse path

When the snapshot is valid:
1. Build a fresh `AnalysisState` seeded with current `cache`/`overlay`/`project_root`
   but with `exports`/`interfaces`/`shared_types`/`shared_type_origins`/`module_order`/
   `module_runners`/`failed` restored from the snapshot.
2. Run only the entry's own analysis against it: `load_source(entry)` (entry text from
   overlay), `parse_cached`, build the entry's env via `extend_env_from_shared` +
   merge the entry's direct-dep interfaces (from restored `interfaces`),
   `dependency_hashes` over the entry's deps, `resolve_and_check_local`,
   `publish_interface`, unused-import check.
3. Diagnostics = `snapshot.dep_diagnostics` ++ entry's own diagnostics.
4. Re-capture the snapshot (the closure state is unchanged, so this is essentially
   the same snapshot; cheap to keep as-is).

## Stages

- **Stage 1 — plumbing, no behavior change. DONE.** `ClosureSnapshot` defined;
  `analyze_workspace` takes `prior_snapshot` and returns `snapshot`;
  `server_core.State.closure_snapshot` threads it. 2745 tests green.
- **Stage 2 — capture. DONE.** Captured in `analyze_module_impl` when
  `caller_id == .None` (the entry signal) and `capture_snapshot` is set
  (`with_snapshot_capture()`), right after `analyze_dependencies`.
- **Stage 3 — reuse. DONE.** Validity = `s.entry == entry_canonical` and every
  `closure_modules` member still has a `module_hash`. Reuse restores the snapshot and
  re-runs `analyze_module(entry)`; deps hit the existing `state.exports` early return.
  **Correctness fix discovered:** `analyze_workspace` must `note_source_hash` every
  open doc up front (the reuse path skips `load_source` for deps), or a changed open
  dependency wouldn't invalidate. Results: warm compute ~630ms → ~55ms (~12×),
  mid-compile format latency ~0.7s → 23ms; 2748 boot tests green (3 new snapshot
  tests: reuse-correct / entry-error-still-reported / dep-change-busts); self-host
  green; LSP driver confirms entry+dep errors appear and clear.

## Status: COMPLETE

Shipped boot-only in `analyze.tw`, `diagnostics.tw`, `server_core.tw`, `semantic.tw`
(+ test `State` literals). Possible future refinement (not required): entry-in-cycle
is handled safely by falling back to a full run (validity check fails), and an
entry import-list change is handled naturally because new imports are analyzed fresh
and removed ones are simply not merged.

## Risks / correctness gates

- The self-host loop recompiles the entire compiler through this path; a TypeId or
  state-restore bug would miscompile and fail the 2745-test suite — strong gate.
- `tests/cow_analysis.rs` census (stage0 over `boot/main.tw`) as an additional guard.
- Diagnostics must be **identical** with/without reuse (golden comparison in Stage 3).
- Snapshot must be discarded whenever any closure module changes (rely on
  `module_hash` invalidation; add a test that editing a dep busts the snapshot).

## Non-goals

- Position-independent/content-addressed TypeIds (the general fix; major redesign).
- Persisting the snapshot to disk across LSP restarts (in-memory only).
