# Stdlib/Prelude Signature Source of Truth Plan

## Goal

Make Twinkle sources in `prelude/*` and `stdlib/*` the single source of truth for callable signatures and method shapes, and remove duplicated signature encoding spread across Rust typecheck/lower/codegen paths.

## Why This Is Painful Today

The same contract is currently encoded in multiple places. A single API change (for example, adding/changing a method on `Vector`, `String`, `Dict`, `Cell`, or primitives) can require edits in several Rust files.

Common duplication hotspots:

- typechecking for module calls and method calls (`src/types/check.rs`) — six `synth_*_call` handler functions with hardcoded match-on-method-name branches
- method-value reference typing (`src/types/check.rs`) — `check_module_func_ref` fallback table duplicates method shapes for first-class references
- lowering dispatch (`src/ir/lower.rs`) — `resolve_builtin_method_value` (19-arm match) and `lower_method_call` (20+ arm match) are separate hardcoded tables
- intrinsic signature contracts (`src/intrinsics/contracts.rs`) — full Rust-side type signatures duplicating what `.tw` sources already declare
- interpreter dispatch (`src/interp/eval.rs`) — 60+ arm `match func_id` block, entirely separate from the codegen registry

Observed failure mode:

1. one path is updated (for example direct method call),
2. another path is missed (for example method value reference),
3. behavior diverges across features/backends until a dedicated test catches it.

This is cross-cutting tax that slows feature work and increases regression risk.

## Current State (What Already Exists)

Before describing changes, here is what the codebase already has:

### Identity: `FuncId`

The codebase uses `FuncId(u32)` as the universal callable identity — from lowering through codegen and interpretation. Prelude intrinsics have well-known constant FuncIds defined in `src/ir/lower.rs::prelude` (e.g., `VECTOR_PUSH = FuncId(11)`). There is no separate `SymbolId` type.

### Intrinsic Infrastructure (`src/intrinsics/`)

- **`registry.rs`**: `IntrinsicSpec` table (`INTRINSIC_SPECS`, 55+ entries) mapping `FuncId` → `twinkle_name` + `IntrinsicDispatch` (Runtime | Intrinsic) + `LoweringKind` (27 variants for WAT codegen dispatch). `populate_func_table()` bootstraps the lowerer's `func_table`.
- **`contracts.rs`**: `IntrinsicContract` with full Rust-side type signatures (type params, param types, return type, ABI result). `function_signatures()` pre-registers intrinsic signatures into `ValueEnv` at startup. **This is a primary duplication target** — it encodes in Rust what `.tw` sources already declare.

### Env-Driven Method Resolution (Partially Wired)

- `TypeEnv.methods: HashMap<(TypeId, String), String>` maps `(type_id, method_name)` → qualified function name.
- `ValueEnv.get_function(name)` returns the `FunctionSignature` for a qualified name.
- `try_synth_registered_method_call` in `check.rs` composes these: receiver type → `TypeEnv` method lookup → `ValueEnv` signature → instantiate/unify. **This is already the target pattern** — it works for named (user-defined) types but is bypassed for builtin receiver types (`Vector`, `String`, `Dict`, `Cell`, etc.), which hit the hardcoded `synth_*_call` branches first.

### Codegen: Already Registry-Driven

`emit_prelude_call` in `emit.rs` dispatches through `registry::lowering_kind(func_id) → Option<LoweringKind>`. This layer is already consolidated — it is not hardcoded per-method. The codegen layer is the closest to the target architecture.

### Interpreter: Not Registry-Driven

`eval.rs` has a monolithic 60+ arm `match func_id` block. It does not use `LoweringKind` or any registry dispatch. This is a consolidation target but lower priority than typechecker/lowerer.

## Design Constraints

1. Do not introduce new language syntax just to represent declarations.
2. Keep `prelude/*` and `stdlib/*` as real Twinkle implementations.
3. Rust still owns runtime ABI/layout and host/intrinsic execution hooks.
4. Preserve existing language behavior (including ambiguity rules and collision checks).

## Target Architecture

1. Treat trusted modules (`prelude/*`, `stdlib/*`) as signature-authoritative.
2. Resolve all function/method signatures from `ValueEnv`/`TypeEnv`, not hardcoded Rust shape tables.
3. Extend the existing `src/intrinsics/` infrastructure to become the single execution registry, keyed by `FuncId`, answering: "given a resolved callable, how is it executed?"
4. Keep special built-in type identity constants (`Option`, `Result`, `Cell`, `Iterator`, etc.) for runtime representation only, not for duplicating callable signatures.

### Callable Categories

Every callable in the compiler falls into one of three categories:

| Category | Signature source | Execution | Example |
|---|---|---|---|
| **Pure Twinkle** | `.tw` source | compiled Twinkle body | `String.trim`, `Vector.map` |
| **Intrinsic-backed** | `.tw` source | Rust interpreter hook / WAT emission hook | `Vector.push`, `Dict.get`, `Cell.set` |
| **Compiler-special** | hardcoded in Rust | special typing or lowering rules | `error` (Never return type), variant constructors |

Compiler-special callables should be explicitly documented and kept to an absolute minimum. The goal is for the vast majority of stdlib APIs to be either pure Twinkle or intrinsic-backed, with signatures always coming from `.tw` sources.

### Target Resolution Flow

The canonical resolution path for all callables:

```
Twinkle source (.tw)
  ↓
ValueEnv / TypeEnv (signature registered from .tw parsing)
  ↓
FuncId assigned (resolved identity)
  ↓
IntrinsicRegistry lookup: FuncId → LoweringKind (execution classification)
  ↓
Backend dispatch (interpreter eval / WAT emission)
```

This replaces the current pattern where typechecker and lowerer have their own hardcoded shape tables that bypass the env.

### Module Boot Order

Trusted modules participate in the same module graph as user modules, but are injected first into resolution roots. The target pipeline order:

```
1. Parse all modules (trusted + user)
2. Register type declarations into TypeEnv
3. Register function signatures into ValueEnv (from .tw sources, not contracts.rs)
4. Validate intrinsic bindings against env (arity, generic count)
5. Typecheck all module bodies
6. Lower → backend
```

Currently, `CompileState::initial()` pre-registers intrinsic signatures from `contracts::function_signatures()` (Rust-side) before any `.tw` sources are parsed. The target is to reverse this: `.tw` sources provide signatures, and the intrinsic registry validates against them.

Trusted modules must not depend on user modules. They may depend on each other (with the same circular-import rules as user code).

## Non-Goals

- Rewriting runtime internals in Twinkle as part of this plan.
- Removing core runtime type IDs and backend ABI metadata.
- Changing public language syntax or method resolution rules.
- Consolidating the interpreter's `match func_id` block (lower priority, can follow later).

## Implementation Plan

### Phase 0: Inventory + Guardrails

Files:

- `src/types/check.rs`
- `src/ir/lower.rs`
- `src/intrinsics/contracts.rs`

Changes:

1. Enumerate all hardcoded callable-shape logic:
   - Typechecker: `synth_cell_call`, `synth_dict_module_call`, `synth_iterator_call`, `synth_vector_call`, `synth_string_call`, `synth_byte_call`, and `check_module_func_ref` fallback table.
   - Lowerer: `resolve_builtin_method_value` match table, `lower_method_call` match table.
   - Contracts: `IntrinsicContract` signatures in `contracts.rs`.
2. Add characterization tests for existing behavior before refactor:
   - module-qualified calls (`Vector.push(v, x)`)
   - dot calls (`v.push(x)`)
   - first-class method values (`let f = Vector.push`)
   - interpreter/wasm parity cases

Checklist:

- [ ] Inventory comment/doc listing every hardcoded shape site with file:line
- [ ] Characterization test: module-qualified calls for Vector, String, Dict, Cell, Iterator
- [ ] Characterization test: dot-syntax method calls for all builtin receiver types
- [ ] Characterization test: first-class method values for all builtin types
- [ ] Characterization test: interpreter/wasm parity for each of the above
- [ ] All existing tests pass unchanged

### Phase 1: Trusted Module Authority by Path

Files:

- `src/module/context.rs`
- `src/module/mod.rs`

Changes:

1. Formalize trusted module roots (`prelude/*`, `stdlib/*`) in compile configuration. Currently `is_internal` exists on `ModuleStageRunner` but there is no formal "trusted module" concept — make it explicit.
2. Ensure trusted modules are loaded/resolved before user modules. (Prelude auto-loading via `PlannedDependencyKind::Prelude` already exists — formalize and extend to cover all signature-authoritative modules.)
3. Ensure their signatures and inherent methods are always registered into `ValueEnv`/`TypeEnv` from `.tw` source parsing, not from `contracts::function_signatures()`.

Notes:

- No new syntax required.
- Existing Twinkle function bodies remain the canonical declarations.

Checklist:

- [ ] Explicit trusted-module root configuration (not just `is_internal` bool)
- [ ] Prelude `.tw` signatures registered into `ValueEnv` before user module typechecking
- [ ] Prelude `.tw` methods registered into `TypeEnv` before user module typechecking
- [ ] `contracts::function_signatures()` no longer sole source for intrinsic signatures in `ValueEnv`
- [ ] Boot order validated: no user module can be typechecked before trusted modules are registered
- [ ] All existing tests pass unchanged

### Phase 2: Typechecker De-hardcode

Files:

- `src/types/check.rs`

Changes:

1. Extend the env-driven path (`try_synth_registered_method_call`) to handle builtin receiver types (`Vector`, `String`, `Dict`, `Cell`, `Int`, `Float`, `Bool`, `Byte`). Currently these types hit the hardcoded `synth_*_call` branches before reaching the env-driven path.
2. Replace hardcoded module call typing branches (`synth_module_call` dispatcher) with environment-driven signature lookup. The existing `synth_qualified_call` path already does this for user-defined modules — extend it to cover builtins.
3. Replace `check_module_func_ref` fallback table with the same env lookup used for named types. The function already checks `value_env.get_function()` first and returns early if found — make this the only path.
4. Keep only irreducible compiler-special cases (documented in the callable categories table above).

Checklist:

- [ ] `try_synth_registered_method_call` handles `Vector` methods via env lookup
- [ ] `try_synth_registered_method_call` handles `String` methods via env lookup
- [ ] `try_synth_registered_method_call` handles `Dict` methods via env lookup
- [ ] `try_synth_registered_method_call` handles `Cell` methods via env lookup
- [ ] `try_synth_registered_method_call` handles `Iterator` methods via env lookup
- [ ] `try_synth_registered_method_call` handles primitive methods (`Int`, `Float`, `Bool`, `Byte`) via env lookup
- [ ] `synth_module_call` builtin branches replaced by `synth_qualified_call` env-driven path
- [ ] `check_module_func_ref` fallback table removed — env lookup is the only path
- [ ] Compiler-special exceptions documented (list each one)
- [ ] Characterization tests from Phase 0 still pass
- [ ] No new hardcoded shape logic introduced

### Phase 3: Lowering De-hardcode

Files:

- `src/ir/lower.rs`

Changes:

1. Replace `lower_method_call` hardcoded `match (&base_ty, method)` table with `TypeEnv`-based symbol lookup. Named types already fall through to `type_env.get_method_function()` — make builtin types use the same path.
2. Replace `resolve_builtin_method_value` (19-arm match table) with the same env-driven method resolution path.
3. Keep a single lowering routine for method value references after `FuncId` resolution.

Checklist:

- [ ] `lower_method_call`: Vector methods resolved via `type_env.get_method_function()`
- [ ] `lower_method_call`: String methods resolved via `type_env.get_method_function()`
- [ ] `lower_method_call`: Dict methods resolved via `type_env.get_method_function()`
- [ ] `lower_method_call`: Cell methods resolved via `type_env.get_method_function()`
- [ ] `lower_method_call`: primitive methods resolved via `type_env.get_method_function()`
- [ ] `resolve_builtin_method_value` match table removed
- [ ] Method value references and direct method calls share the same resolution path
- [ ] Characterization tests from Phase 0 still pass

### Phase 4: Intrinsic Execution Registry Consolidation

Files:

- `src/intrinsics/registry.rs` (extend existing)
- `src/intrinsics/contracts.rs` (reduce/remove)
- `src/intrinsics/validate.rs` (new)

Changes:

1. Add startup validation to the existing registry infrastructure:
   - Each `IntrinsicSpec` entry's `twinkle_name` must exist in `ValueEnv` (from `.tw` sources).
   - Coarse ABI checks (arity, generic parameter count) — not full re-typechecking.
   - Fail compilation early if bindings and `.tw` signatures disagree.
2. Migrate signature registration from `contracts::function_signatures()` to `.tw`-source-driven registration. `IntrinsicContract` Rust-side signatures become validation targets, not registration sources.
3. Eventually remove `contracts.rs` signature duplication once `.tw` sources are the sole registration path.
4. Each backend continues to implement its own intrinsic handler keyed by `FuncId` / `LoweringKind`:
   - WAT codegen: already uses `registry::lowering_kind(func_id)` → `LoweringKind` dispatch (no change needed).
   - Interpreter: remains `match func_id` for now (consolidation is a non-goal of this plan).

Migration sub-steps (to land safely):

1. Add validation checks alongside existing `contracts.rs` registration — compare Rust-side contracts against `.tw`-registered signatures, fail tests on disagreement.
2. Switch `ValueEnv` initialization to register from `.tw` sources instead of `contracts::function_signatures()`.
3. Reduce `IntrinsicContract` to ABI-only metadata (what the backend needs that isn't derivable from signatures).
4. Delete redundant signature fields from `contracts.rs`.

Checklist:

- [ ] `src/intrinsics/validate.rs` created with arity/generic-count checks
- [ ] Startup validation runs after trusted modules are loaded, before user typechecking
- [ ] Validation covers every `IntrinsicSpec` entry (no silent skips)
- [ ] Intentional signature mismatch in a `.tw` source causes a validation failure (test this)
- [ ] `ValueEnv` initialized from `.tw` sources, not `contracts::function_signatures()`
- [ ] `IntrinsicContract` reduced to ABI-only fields (`IntrinsicAbiResult`, dispatch kind)
- [ ] Redundant signature fields (type params, param types, return type) removed from `contracts.rs`
- [ ] WAT codegen still dispatches through `LoweringKind` (no regression)
- [ ] Interpreter `match func_id` still works (no regression)
- [ ] All existing tests pass

### Phase 5: Cleanup + Drift Prevention

Files:

- `tests/typecheck/*`
- `tests/run_test.rs`
- `tests/run_wasm_test.rs`

Changes:

1. Delete obsolete hardcoded signature tables from typecheck/lower (the `synth_*_call` functions, `resolve_builtin_method_value`, `lower_method_call` match arms).
2. Add drift tests that fail if `.tw` source signatures and intrinsic bindings disagree.
3. Expand parity coverage (interpreter and wasm) for:
   - module-qualified refs
   - direct dot calls
   - first-class method refs (including polymorphic + annotated cases)

Checklist:

- [ ] `synth_cell_call` removed from `check.rs`
- [ ] `synth_dict_module_call` removed from `check.rs`
- [ ] `synth_iterator_call` removed from `check.rs`
- [ ] `synth_vector_call` removed from `check.rs`
- [ ] `synth_string_call` removed from `check.rs`
- [ ] `synth_byte_call` removed from `check.rs`
- [ ] `check_module_func_ref` fallback arms removed from `check.rs`
- [ ] `resolve_builtin_method_value` removed from `lower.rs`
- [ ] `lower_method_call` builtin match arms removed from `lower.rs`
- [ ] Drift test: adding a new function to a prelude `.tw` file requires zero Rust changes
- [ ] Drift test: changing an intrinsic's arity in `.tw` without updating registry causes compile-time failure
- [ ] Parity tests: module-qualified refs pass in both interpreter and wasm
- [ ] Parity tests: dot calls pass in both interpreter and wasm
- [ ] Parity tests: first-class method refs pass in both interpreter and wasm
- [ ] No hardcoded callable-shape tables remain outside documented compiler-special exceptions

## Debugging Guarantees

The compiler should support diagnostic output showing resolved call targets. For any callable invocation, it should be possible to inspect:

```
resolved call:
  symbol: Vector.push (FuncId(11))
  signature: fn push<T>(Vector<T>, T) -> Vector<T>
  impl: Intrinsic(LoweringKind::VectorPush)
```

This aids debugging during migration and ongoing development. Implementation can be a `--dump-resolved-calls` flag or trace-level logging, not necessarily a stable CLI interface.

## Risks

- Boot-order mistakes can cause missing signatures during early stages — `.tw` sources must be parsed and registered before the typechecker runs user code.
- Removing hardcoded fallbacks too early can break existing builtin behavior — the `synth_*_call` functions are load-bearing today.
- Some APIs may still require explicit contextual typing rules even after de-hardcoding (e.g., `Dict.new()` with no type context).
- `contracts.rs` removal must be gradual — backends still need ABI metadata (`IntrinsicAbiResult`) that isn't in `.tw` sources.

## Mitigations

1. Land in phases with characterization tests first.
2. Keep old and new lookup paths behind temporary checks during migration (Phase 4 sub-steps).
3. Add strict startup/assertion checks for intrinsic binding integrity.
4. Validate every phase with interpreter + wasm suites.

## Success Criteria

1. Changing a trusted-module signature requires no Rust typechecker/lower signature edits.
2. Method call and method-value reference paths resolve through the same signature source (`try_synth_registered_method_call` or equivalent for all types).
3. Interpreter and wasm backends stay in parity for method/module call behavior.
4. No remaining hardcoded callable-shape tables for stdlib/prelude APIs outside explicitly documented compiler-special exceptions.
5. Intrinsic registry startup validation catches signature/binding drift before any user code is compiled.
