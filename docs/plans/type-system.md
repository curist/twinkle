# Type System — Stages 6a–6c

## Stage 6a — User-Defined Generics & Bidirectional Type Checking ✅

**Goal:** Support generic type declarations and bidirectional checking for common expression forms.

What was done:

* User-defined generic record and sum types (`type Pair<A,B> = .{ ... }`, `type Tree<T> = { ... }`).
* One `TypeId` per generic definition; field/variant types stored with `Var("T")` placeholders.
* Substitution applied at field reads, record literals, variant literals, patterns, and capability records.
* Bidirectional `check_expr` for `case` arms, anonymous record literals, lambda params, and `if` branches.
* `MonoType::Never` (bottom type) for diverging expressions (`break`/`continue`/`return`).
* `try expr` sugar desugared into a match over `Result`.

Remaining limitation: type variables are only introduced via explicit `<T>` parameters on `fn`/`type`
declarations. Call-site inference (e.g. `let f := id` where `id` is polymorphic) is not supported —
see Stage 6b.

---

## Stage 6b — Query-Friendly Pipeline Refactor ✅

**Goal:** Reshape the compiler pipeline so each stage is a pure function with explicit
inputs and outputs, enabling independent stage invocation, per-file incremental
re-compilation, and better testability — without adding any framework dependency.

> **Full design:** See [docs/query-pipeline.md](../query-pipeline.md).

Key changes:

* Replace `CompilationContext` mutation with per-module artifact structs:
  `ResolvedModule`, `TypedModule`, `LoweredModule`, `LinkedProgram`.
* Each stage becomes a pure function: `resolve(ast, deps)`, `typecheck(ast, resolved)`,
  `lower(ast, typed)`, `link(modules)`.
* FuncIds assigned module-locally (starting at USER_FUNC_START, after prelude slots)
  and remapped by the linker with per-module base offsets — stable across re-compilations.
* `compile_module` becomes a thin coordinator; no shared mutable state.
* `CompilationContext` shrinks to just the module loader cache and import stack.

**Current status (2026-02-28):**

* Artifact structs exist (`ResolvedModule`, `TypedModule`, `LoweredModule`) and stage
  boundaries are cleaner than before.
* `resolve`, `typecheck`, and `lower` are callable with explicit inputs and artifacts.
* The `compile_module` coordinator now uses explicit stage data flow (no env-swap
  `mem::replace` pattern).
* `CompileState` is reduced to module-graph/coordinator accumulation state.
* User FuncIds are now module-local during lowering and remapped in `link` with a
  deterministic topo order of modules.
* FuncId stability tests exist for import-order changes and unrelated entry-module edits.
* An in-process content-hash stage cache exists for parse / resolve / typecheck / lower,
  including reverse-dependent invalidation and cache hit/miss tests.
* Tool-facing query API includes structured diagnostics, direct stage entry points, and
  symbol queries without requiring `CompileState` construction.

Stage 6b scope is complete in this repo. The compiler can be called query-style for
parse/resolve/typecheck/lower and supports in-process incremental reuse.

* **6b.1 Stateless stage contracts**
  * done: stage functions consume explicit inputs and return artifacts;
  * done: env swap pattern removed from coordinator stage flow.

* **6b.2 Stable module-local IDs + linker remap**
  * done: module-local FuncIds + deterministic linker remap;
  * done: stability tests for import-order and unrelated edits.

* **6b.3 Incremental cache database**
  * done: content-hash keys + independent stage caches;
  * done: reverse-dependent invalidation;
  * non-goal for Stage 6b: on-disk persistence across process invocations.

* **6b.4 Tool-facing query API**
  * done: parse/resolve/typecheck/lower APIs + structured diagnostics;
  * done: symbol query API + default query context helpers.

Deliverables (done when all below are true):

* All existing tests (`tests/run/`, `tests/modules/`) pass unchanged.
* Each stage independently testable without constructing a full context.
* No Salsa or other framework dependency introduced.
* Incremental tests prove unchanged modules skip resolve/typecheck/lower.
* FuncId stability tests prove deterministic IDs after unrelated edits.

**Execution checklist (file/module map):**

* **Step A — Refactor stage boundaries (`6b.1`)**
  * `src/module/mod.rs`:
    * split orchestration from stage logic; coordinator should pass immutable inputs and collect outputs;
    * remove env swapping pattern in favor of explicit stage data flow.
  * `src/module/context.rs`:
    * shrink or remove `CompileState` as mutable cross-stage carrier;
    * move only loader/cycle detection concerns into a thin context.
  * `src/module/artifacts.rs`:
    * extend artifact structs so all stage outputs are explicit (including method registrations / per-module metadata).
  * `src/types/resolve.rs`, `src/types/check.rs`, `src/ir/lower.rs`:
    * keep stage functions pure over explicit inputs; no hidden mutation dependencies.

* **Step B — Stable IDs via linker remap (`6b.2`)**
  * `src/ir/lower.rs`:
    * assign module-local user FuncIds (per-module numbering), not global monotonic IDs.
  * `src/module/artifacts.rs`:
    * store module-local function sets and metadata required for remapping.
  * `src/module/mod.rs` (`link`):
    * topologically order modules, assign module base offsets, and remap all FuncId references.
  * Tests to add:
    * `tests/modules_test.rs` / new dedicated test file asserting FuncId stability under import-order changes.

* **Step C — Incremental cache + invalidation (`6b.3`)**
  * New module recommended: `src/query/` (`mod.rs`, `cache.rs`, `keys.rs`, `graph.rs`).
    * stage cache keying: source hash + transitive dep hashes + stage context hash.
    * dep graph and reverse-dependency invalidation.
  * `src/module/mod.rs`, `src/module/loader.rs`:
    * integrate cache lookup/store and dependency graph updates.
  * `src/cli/check.rs`, `src/cli/build.rs`, `src/cli/run.rs`:
    * add cache-aware execution paths (warm/cold behavior).
  * Tests to add:
    * cache hit/miss behavior;
    * reverse-dependent invalidation when an imported module changes.

* **Step D — Tool-facing query API (`6b.4`)**
  * `src/lib.rs`:
    * export stable query entry points for parse / resolve / typecheck / diagnostics / symbol lookup.
  * New module recommended: `src/query/api.rs`:
    * single ergonomic facade used by CLI, future `twk lsp`, and future `twk lint`.
  * `src/types/error.rs`, `src/syntax/span.rs`:
    * ensure diagnostics include stable machine-readable IDs + spans + severity.
  * Tests to add:
    * API-level snapshots for diagnostics and symbol queries.

**Recommended order:** A -> B -> C -> D (do not start LSP incremental work before C).

---

## Stage 6c — Full Damas–Milner Inference ✅

**Goal:** Complete the type inference engine with unification, generalization, and instantiation at use sites.

Features:

* True type variables and unification:

  * `MonoType::MetaVar(u32)` — fresh unification variables created at each generic instantiation site.
  * Full structural unification engine with occurs check.
  * `zonk` / `zonk_ty` — apply substitution maps to resolve MetaVars after checking.

* Generalization at `fn` declarations (not local `:=` bindings):

  * `fn id<A>(x: A) A { x }` — polymorphic; `A` is generic.
  * `f := id` — error (`AmbiguousType`): cannot bind a polymorphic function without a type annotation.
  * `annotated: fn(Int) Int = id` — annotation provides a concrete type; accepted.

* Instantiation at use sites (all dispatch paths):

  * Direct calls: `id(42)` → fresh MetaVar solved to `Int` by argument.
  * Module-qualified calls: `lib.id("s")` instantiated from full `FunctionSignature`.
  * Inherent method calls: `box.get()` where `box: Box<String>` — receiver type unifies MetaVars.
  * Zero-arg generic variants: `UnfoldStep.Done` via field-access path now uses MetaVars, not raw `Var`.
  * `TypeName.Variant(args)` calls: already used MetaVars; verified consistent.

* Soundness invariants enforced:

  * `Var(_)` wildcard removed from `unify` — `fn bad<A>(x: A) Int { x }` is now a type error.
  * `AmbiguousType` reported for: unannotated bindings holding unsolved MetaVars, inferred function return types containing MetaVars, generic references used without calling.
  * `OccursCheckFailed` guard in `solve_meta` for infinite-type prevention (unreachable at current language level due to required parameter annotations; documented in-code for when unannotated lambdas are introduced).
  * Per-function MetaVar scope: `meta_subst` cleared and TypeMap zonked after each function; final zonk after module-level stmts.

Core IR does not change; it just gets richer type annotations.

Deliverables:

* `twk check` supports call-site type inference for generic functions across all dispatch paths.
* Type inference tests:

  * `tests/typecheck/pass/inference.tw` — direct calls, higher-order (`apply`), annotated binding.
  * `tests/typecheck/fail/polymorphic_binding.tw` — `f := id` rejected as ambiguous.
  * `tests/typecheck/fail/generic_body_return_mismatch.tw` — `fn bad<A>(x: A) Int { x }` rejected.
  * `tests/typecheck/fail/generic_method_mismatch.tw` — `Box<String>.get()` assigned to `Int` rejected.
  * `tests/typecheck/fail/generic_ref_escape.tw` — unapplied generic reference in function body rejected.
