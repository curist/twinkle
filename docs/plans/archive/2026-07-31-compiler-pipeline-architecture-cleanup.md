# Compiler Pipeline Architecture Cleanup Plan

## Goal

Keep the boot compiler pipeline understandable as recent ownership, mutable-codegen,
backend preparation, and frontend SCC work make the pass graph more sophisticated.
This is an umbrella cleanup plan: each increment should preserve output behavior and
make one orchestration seam harder to accidentally drift.

## Status

Complete. All four increments landed on branch `stage2-perf-summary-reuse`
across commits `8c7e002e` (increment 1), `ac5d9b59` (increment 2),
`71753d97` (increment 3), `eb0745b3` (increment 4), and `de3441b4` (review
nit cleanup).

## Why this exists

The current compiler pipeline still has reasonable large-scale boundaries:

* frontend analysis is centralized in `boot/compiler/query/analyze.tw` and shared
  by build, LSP, lint, and diagnostics paths;
* per-stage query caching is isolated in `boot/compiler/query/stage_runner.tw`;
* backend preparation has an explicit `PreparedModule` boundary in
  `boot/compiler/backend/prepare.tw`;
* ownership and summary analysis keep their dependency direction acyclic.

The code smells are not “the pipeline is broken”; they are places where new passes
have made ordering and facts rely on duplicated orchestration or convention. This
plan targets those seams without rewriting the core analyses.

---

## Scope

In scope:

1. Consolidate the duplicated post-analysis compile path.
2. Expose a reusable codegen-preparation artifact so tools stop mirroring
   `link_program` by hand.
3. Move backend typed-vector facts out of ad-hoc `ResolvedEnv` mutation.
4. Split `link_program` into named stage helpers once the earlier seams exist.

Out of scope:

* Broad rewrites of `ownership.tw`, `summary.tw`, the optimizer, or SCC frontend
  resolution.
* Frontend `AnalysisState` decomposition. That may become useful later, but it is
  minor compared with the four seams above.
* Performance micro-optimizations unless fresh timing data identifies a specific
  repeated-work issue.

---

## Guiding constraints

* Preserve pass order unless a task explicitly proves a behavior-preserving move.
* Prefer one reusable orchestration helper over “mirror exactly” comments.
* Keep diagnostics formatting separate from compilation semantics.
* Keep prepared-backend facts explicit; avoid hiding backend facts in mutated
  frontend data structures.
* Every increment should be byte-identical or behavior-identical on existing
  compiler inputs before moving to the next increment.
* After editing `.tw` files, run `target/twk fmt <file>` and relevant
  `target/twk lint <entry>` commands.

---

## Current shape and smells

### 1. Duplicated post-analysis compile path

`boot/compiler/pipeline.tw` and `boot/compiler/module_compiler.tw` both lower the
analyzed module graph, construct `CompiledModule`s, select lib exports, link,
monomorphize, lower to ANF, and optimize.

This is already fragile because the paths can drift. The source-string path wraps
optimization with dead-function sweeping, while the entry-file path has its own
post-ANF tail. Whether that difference is intentional or accidental should be made
explicit in one shared implementation.

### 2. Tooling mirrors `link_program` by convention

`boot/commands/ir.tw` reconstructs the authoritative mutable/codegen sequence for
its audit report: builder-region rewrite, variant specialization, seeded mutable
production, closure conversion, backend preparation, and mutable audit. The
comment says this mirrors `link_program` exactly. That is useful, but the compiler
has no reusable object representing “the prepared codegen pipeline state”.

### 3. Backend typed-vector facts are copied through a mutated env

`PreparedModule` carries `typed_vector_fields` and `typed_vector_payloads`, but
`boot/compiler/codegen/codegen.tw` copies those facts into a mutable `env2` before
verify, Wasm type planning, and emission. That weakens the prepared-IR boundary:
some backend facts live on `PreparedModule`, while consumers still discover them
through `ResolvedEnv`.

### 4. `link_program` is now a high-coupling orchestrator

`link_program` owns feature toggles, timing, builder-region rewriting, ownership
variant specialization, closure conversion, mutable decision production, backend
preparation, verification, Wasm planning, emission, runtime linking, metadata
attachment, Wasm DCE, and conditional memory pruning. The comments make the order
clear, so this is not urgent breakage, but the function has become the choke point
where unrelated additions accumulate.

---

## Increment 1: Shared post-analysis compile helper

### Goal

Make source-string compilation and entry-file compilation use the same semantic
post-analysis path.

### Files

* Modify: `boot/compiler/module_compiler.tw`
* Modify: `boot/compiler/pipeline.tw`
* Possibly modify: `boot/compiler/artifacts.tw` if a small shared result type is
  needed
* Test: existing suites that use `pipeline.compile_source`,
  `pipeline.compile_source_lib`, `pipeline.compile_entry_path`, and
  `module_compiler.compile_entry`

### Proposed shape

Add a shared helper in `module_compiler.tw` or a small new module such as
`boot/compiler/compile_graph.tw`:

```tw
type CompileGraphInput = .{
  canonical_entry: String,
  env: ResolvedEnv,
  state: analyze.AnalysisState,
  builtins: BuiltinRegistry,
  lib_build: Bool,
}

type CompileGraphResult = .{
  artifacts: PipelineArtifacts,
  lower_diagnostics: Vector<analyze.AnalysisDiag>,
}
```

The helper should own:

* iterating `state.module_order`;
* fetching parsed and typed cache artifacts;
* calling `stage_runner.lower`;
* constructing `CompiledModule`;
* selecting entry lib exports when requested;
* core linking, monomorphization, ANF lowering, optimization, and ANF-level DCE;
* returning warnings and lower diagnostics in a caller-format-neutral form.

The caller should own only:

* building the initial analysis state;
* converting analysis or lower diagnostics to its public error type;
* inline-source diagnostic rendering for `compile_source`.

### Steps

- [ ] Add the shared helper and result type without changing callers.
- [ ] Move the duplicated lower/module-assembly loop into the helper.
- [ ] Move the post-link tail into the helper.
- [ ] Route `module_compiler.compile_entry_with_lib` through the helper.
- [ ] Route `pipeline.compile_source_impl` through the helper.
- [ ] Decide and document the ANF-level dead-function sweep as part of the single
      shared tail.
- [ ] Run focused multi-module, lib-export, and compile-source tests.
- [ ] Run the boot compiler test suite if the focused tests pass.

### Acceptance

* `compile_source`, `compile_source_lib`, `compile_entry_path`, and
  `compile_entry_path_lib` still expose the same public behavior.
* Lowering diagnostics still render correctly for inline source and file-backed
  builds.
* There is only one implementation of the lower/link/mono/ANF/opt/DCE tail.

---

## Increment 2: Reusable codegen-preparation artifact

### Goal

Stop tools from manually replaying the authoritative codegen preparation order.
Both `link_program` and `twk ir` should consume a shared prepared-codegen result.

### Files

* Modify: `boot/compiler/codegen/codegen.tw`
* Modify: `boot/commands/ir.tw`
* Possibly create: `boot/compiler/codegen/pipeline_state.tw` if the extracted type
  would make `codegen.tw` too broad
* Test: mutable produce, variant specialize, builder-region, codegen emit, and IR
  command coverage

### Proposed shape

Introduce a public helper around the semantic-ANF-to-prepared-backend portion:

```tw
type CodegenPrepared = .{
  sem_env: OptimizerSemantics,
  rewritten: AnfModule,
  specialized: variant_specialize.SpecializeResult,
  closure_conversion: ClosureConvertResult,
  produced_decisions: mutable_produce.ProducedDecisions,
  prepared: PreparedModule,
}

pub fn prepare_codegen(
  anf: AnfModule,
  env: ResolvedEnv,
  builtins: BuiltinRegistry,
  options: CodegenOptions,
) CodegenPrepared
```

`CodegenOptions` should cover existing kill-switches and policies without making
callers read environment variables themselves. The default option constructor can
still read the environment for normal builds.

`link_program` should call `prepare_codegen`, then continue with verify, type
planning, emission, runtime linking, metadata, DCE, and memory pruning.

`boot/commands/ir.tw` should call the same helper and render its audit from
`CodegenPrepared` rather than reconstructing the sequence.

### Steps

- [ ] Define `CodegenPrepared` and `CodegenOptions` near `link_program` or in a
      new focused module.
- [ ] Extract builder-region rewrite, variant specialization, closure conversion,
      mutable decision production, and backend preparation into `prepare_codegen`.
- [ ] Keep timing labels identical so performance docs and scripts remain useful.
- [ ] Change `link_program` to consume the extracted result.
- [ ] Change `render_census_report` in `boot/commands/ir.tw` to consume the same
      result for authoritative mutable auditing.
- [ ] Run focused tests for mutable decisions, variant routes, builder regions,
      and codegen emission.

### Acceptance

* `twk ir` no longer has a “mirror `link_program` exactly” implementation block.
* `link_program` and `twk ir` share the same prepared-codegen helper.
* Existing timing output remains recognizable.
* Existing mutable/variant/codegen tests pass.

---

## Increment 3: Explicit backend context for typed-vector facts

### Goal

Keep typed-vector backend facts on the prepared-backend side of the boundary
instead of copying them into a mutated `ResolvedEnv` in `link_program`.

### Files

* Modify: `boot/compiler/backend/prepare.tw`
* Modify: `boot/compiler/backend/verify*.tw`
* Modify: `boot/compiler/codegen/wasm_plan*.tw`
* Modify: `boot/compiler/codegen/emit*.tw`
* Modify: `boot/compiler/codegen/codegen.tw`
* Test: typed vector, typed record field, typed param ABI, backend prepare, Wasm
  plan, and codegen emit suites

### Proposed shape

Introduce a small backend context that pairs the source env with prepared facts:

```tw
type BackendContext = .{
  env: ResolvedEnv,
  typed_vector_fields: Dict<String, Bool>,
  typed_vector_payloads: Dict<String, Bool>,
}

pub fn backend_context(env: ResolvedEnv, prepared: PreparedModule) BackendContext
```

Then migrate backend consumers from `ResolvedEnv` to `BackendContext` where they
need typed-vector facts. Consumers that only need ordinary type names or layouts
can continue to read `ctx.env`.

Avoid a huge all-at-once churn if possible:

1. add `BackendContext` while preserving the old env fields;
2. migrate verify/plan/emit one area at a time;
3. remove the `env2.typed_vector_* = ...` mutation once no backend consumer needs
   it.

### Steps

- [ ] Add `BackendContext` and constructor.
- [ ] Update verification entry points to accept the context where typed-vector
      facts are needed.
- [ ] Update Wasm type planning to read typed-vector facts through the context.
- [ ] Update emission helpers to read typed-vector facts through the context.
- [ ] Remove the `env2` mutation in `link_program` or `prepare_codegen`.
- [ ] Run typed-vector and backend/codegen focused suites.
- [ ] Run a self-host byte-identical check before and after the migration.

### Acceptance

* Backend typed-vector facts have one authoritative source after preparation.
* `link_program` no longer mutates `ResolvedEnv` to smuggle prepared facts.
* Typed-vector and backend/codegen tests pass.
* Self-host output is unchanged for a fixed input.

---

## Increment 4: Split `link_program` into named stage helpers

### Goal

Make the final codegen pipeline easier to scan and harder to extend in the wrong
place, after the reusable preparation and backend-context seams exist.

### Files

* Modify: `boot/compiler/codegen/codegen.tw`
* Possibly create: focused helper modules only if `codegen.tw` remains too large
  after extraction
* Test: full codegen path through `pipeline.emit_wat`, `pipeline.emit_wasm`, and
  build/run commands

### Proposed helper boundaries

After Increment 2, `link_program` can become a short orchestration function:

```tw
pub fn link_program(anf: AnfModule, env: ResolvedEnv, builtins: BuiltinRegistry) LinkedModule {
  prepared := prepare_codegen(anf, env, builtins, default_codegen_options())
  registry := verify_and_plan(prepared, builtins)
  user_module := emit_user_module(prepared, registry, builtins)
  linked := link_with_runtime(user_module, registry)
  finalize_linked_module(linked, anf)
}
```

The exact names can change, but the boundaries should stay semantic:

* preparation of codegen inputs;
* verification and type planning;
* user module emission;
* runtime linking;
* metadata, Wasm DCE, and memory/export pruning.

### Steps

- [ ] Extract `verify_and_plan` or equivalent, preserving verify-level behavior.
- [ ] Extract user module emission.
- [ ] Extract runtime linking and link-error rendering.
- [ ] Extract final metadata/DCE/memory pruning.
- [ ] Confirm `codegen`, `codegen_wasm`, and `codegen_wasm_buffer` still differ
      only in final serialization.
- [ ] Run focused codegen and CLI build tests.

### Acceptance

* `link_program` reads as a compact pass graph rather than a long mixed-purpose
  implementation.
* Feature toggles and timing remain centralized and discoverable.
* No pass order changes accidentally.
* WAT and Wasm output remain unchanged for representative fixtures.

---

## Validation strategy

For each increment, use the smallest useful validation first:

```bash
target/twk fmt <changed .tw files>
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

For increments that touch post-analysis or codegen ordering, also run a self-host
fixed-output check appropriate to the current workflow, for example:

```bash
make stage2
```

For behavior-sensitive backend changes, compare WAT or Wasm output on a fixed
input before and after the change. Prefer byte-identical output when the change is
intended to be pure restructuring.

---

## Suggested implementation order

1. Shared post-analysis compile helper.
2. Reusable codegen-preparation artifact.
3. Explicit backend context for prepared typed-vector facts.
4. `link_program` helper split.

This order removes drift first, then creates a reusable pass artifact, then
cleans up fact ownership, and only then splits the large orchestrator. Doing the
`link_program` split first would move code around without solving the strongest
sources of convention-based duplication.
