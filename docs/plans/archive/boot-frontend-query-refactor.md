# Boot Frontend Query and Interface Refactor

## Goal

Make boot frontend analysis cheaper and easier to reason about by separating
module interfaces from full `ResolvedEnv` construction.

The current shared frontend path in `boot/compiler/query/analyze.tw` already
provides explicit stage caching for parse, resolve, typecheck, and lower. This
plan keeps that architecture, but reduces repeated whole-environment rebuilding
while improving invalidation clarity for builds and LSP diagnostics.

---

## Motivation

`analyze_module` currently performs several responsibilities at once:

* reads source through overlay/disk
* computes source and dependency fingerprints
* plans imports and prelude imports
* recursively analyzes dependencies
* extends a large environment from accumulated shared types
* merges imported exports into the local environment
* runs resolve and typecheck
* captures local types and stores exports
* updates cache metadata and module order

That makes the function hard to change safely. It also encourages repeated
scans and copies of shared type vectors as dependency graphs grow.

---

## Non-Goals

* No type-system behavior changes
* No change to import syntax or module resolution rules
* No removal of the explicit `cache.Store` threading model
* No persistent on-disk cache in this plan
* No LSP protocol changes

---

## Target Shape

Introduce a compact module interface layer:

```tw
type ModuleInterface = .{
  path: String,
  exports: ModuleExports,
  exported_type_ids: Vector<TypeId>,
  exported_function_names: Vector<String>,
  type_origins: Dict<Int, String>,
  fingerprint: Int,
}
```

The exact fields may change during implementation. The important boundary is:

* module interface = what downstream modules need to import
* full checked environment = local implementation detail of that module

`analyze_module` should become an orchestration wrapper over smaller helpers:

* `load_source`
* `parse_cached`
* `plan_dependencies`
* `analyze_dependencies`
* `build_import_env`
* `resolve_and_check_local`
* `publish_interface`
* `update_module_cache`

---

## Work Plan

### Phase 1: Extract helper boundaries without behavior changes

- [x] Split source loading and source-hash handling into a helper.
- [x] Split dependency planning into a helper that returns canonical dependency
      entries and import/prelude kind.
- [x] Split dependency analysis loop out of `analyze_module`.
- [x] Split resolve/typecheck/cache-update code out of `analyze_module`.
- [x] Keep all data structures unchanged during this phase.

### Phase 2: Add module interface records

- [x] Define `ModuleInterface` in an appropriate query/frontend module.
- [x] Store interfaces in `AnalysisState` separately from full exports if useful.
- [x] Compute interface fingerprints from exported surface only.
- [x] Add tests showing unchanged dependency implementation details do not force
      unnecessary downstream cache invalidation when the export fingerprint is
      stable.

### Phase 3: Build import environments from interfaces

- [x] Replace dependency merge logic that needs full module state with merge from
      `ModuleInterface`.
- [x] Keep type identity/origin preservation explicit.
- [x] Ensure selective imports, module aliases, relative imports, and prelude
      imports all use the same interface path.

### Phase 4: Reduce repeated shared type extension

- [x] Avoid repeatedly extending from the full accumulated shared type vector
      when only a small set of newly visible dependency types changed.
- [x] Keep a stable TypeId lookup path for checked modules.
- [x] Validate multi-module tests, signature drift tests, and LSP diagnostics.

### Phase 5: Cache/invalidation cleanup

- [x] Document which keys depend on source hash, dependency hash, context hash,
      and internal-module status.
- [x] Make invalidation comments in `query/cache.tw` match the new interface
      layer.
- [x] Add targeted tests for dependency implementation-only changes, export
      changes, and parse/typecheck failures.

---

## Validation

- [x] `target/twk run boot/tests/main.tw`
- [x] `target/twk build boot/main.tw -o /tmp/boot.wasm`
- [x] LSP diagnostic suites
- [x] Query cache and stage runner suites
- [x] Multi-module/import suites

---

## Risks

* Interface fingerprints must include every semantic fact that affects downstream
  checking.
* TypeId stability across modules is easy to regress.
* LSP overlays must continue to behave exactly like disk sources from the query
  layer's perspective.
