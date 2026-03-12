# Stdlib/Prelude Signature Source of Truth Plan

## Goal

Make Twinkle sources in `prelude/*` and `stdlib/*` the single source of truth for callable signatures and method shapes, and remove duplicated signature encoding spread across Rust typecheck/lower/codegen paths.

## Why This Is Painful Today

The same contract is currently encoded in multiple places. A single API change (for example, adding/changing a method on `Vector`, `String`, `Dict`, `Cell`, or primitives) can require edits in several Rust files.

Common duplication hotspots:

- typechecking for module calls and method calls (`src/types/check.rs`)
- method-value reference typing (`src/types/check.rs`)
- lowering dispatch and first-class method reference lowering (`src/ir/lower.rs`)
- intrinsic/builtin `FuncId` binding surfaces (`src/ir/lower.rs`)
- backend specialization assumptions (`src/codegen/ctx.rs`)

Observed failure mode:

1. one path is updated (for example direct method call),
2. another path is missed (for example method value reference),
3. behavior diverges across features/backends until a dedicated test catches it.

This is cross-cutting tax that slows feature work and increases regression risk.

## Design Constraints

1. Do not introduce new language syntax just to represent declarations.
2. Keep `prelude/*` and `stdlib/*` as real Twinkle implementations.
3. Rust still owns runtime ABI/layout and host/intrinsic execution hooks.
4. Preserve existing language behavior (including ambiguity rules and collision checks).

## Target Architecture

1. Treat trusted modules (`prelude/*`, `stdlib/*`) as signature-authoritative.
2. Resolve all function/method signatures from `ValueEnv`/`TypeEnv`, not hardcoded Rust shape tables.
3. Keep a narrow Rust binding layer that maps qualified symbol names to runtime/intrinsic execution details (`FuncId`, interpreter hook, wasm emission hook).
4. Keep special built-in type identity constants (`Option`, `Result`, `Cell`, `Iterator`, etc.) for runtime representation only, not for duplicating callable signatures.

## Non-Goals

- Rewriting runtime internals in Twinkle as part of this plan.
- Removing core runtime type IDs and backend ABI metadata.
- Changing public language syntax or method resolution rules.

## Implementation Plan

### Phase 0: Inventory + Guardrails

Files:

- `src/types/check.rs`
- `src/ir/lower.rs`
- `src/codegen/ctx.rs`

Changes:

1. Enumerate all hardcoded callable-shape logic (module call shape checks, method dispatch matches, method-value allowlists).
2. Add characterization tests for existing behavior before refactor:
   - module-qualified calls
   - dot calls
   - first-class method values
   - interpreter/wasm parity cases

### Phase 1: Trusted Module Authority by Path

Files:

- `src/module/context.rs`
- `src/module/mod.rs`

Changes:

1. Formalize trusted module roots (`prelude/*`, `stdlib/*`) in compile configuration.
2. Ensure trusted modules are loaded/resolved before user modules.
3. Ensure their signatures and inherent methods are always registered into `ValueEnv`/`TypeEnv`.

Notes:

- No new syntax required.
- Existing Twinkle function bodies remain the canonical declarations.

### Phase 2: Typechecker De-hardcode

Files:

- `src/types/check.rs`

Changes:

1. Replace hardcoded module call typing branches with environment-driven signature lookup where possible.
2. Replace hardcoded method shape checks with:
   - resolve receiver type to method owner id,
   - lookup method symbol in `TypeEnv`,
   - lookup function signature in `ValueEnv`,
   - instantiate/unify/check arguments from that signature.
3. Keep only irreducible special cases (for example, context-required constructors like `Dict.new()` if still needed).
4. Remove fallback shape tables in module-function reference checking once environment-driven checks are complete.

### Phase 3: Lowering De-hardcode

Files:

- `src/ir/lower.rs`

Changes:

1. Replace hardcoded receiver+method dispatch tables with symbol-based lookup from `TypeEnv`.
2. Replace builtin method-value allowlist with the same env-driven method resolution path used for named types.
3. Keep a single lowering routine for method value references after `FuncId` resolution.

### Phase 4: Intrinsic Binding Layer Split

Files:

- `src/ir/lower.rs` (or new `src/intrinsics/registry.rs`)
- interpreter/wasm intrinsic binding modules

Changes:

1. Centralize binding of qualified symbol name -> runtime/intrinsic implementation metadata.
2. Keep signatures in Twinkle sources; keep Rust binding table responsible only for execution wiring.
3. Add startup validation:
   - each intrinsic binding name exists in trusted-module signatures,
   - arity/signature agreement checks at compile startup or test time.

### Phase 5: Cleanup + Drift Prevention

Files:

- `tests/typecheck/*`
- `tests/run_test.rs`
- `tests/run_wasm_test.rs`

Changes:

1. Delete obsolete hardcoded signature tables from typecheck/lower.
2. Add drift tests that fail if trusted-module signatures and intrinsic bindings disagree.
3. Expand parity coverage (interpreter and wasm) for:
   - module-qualified refs
   - direct dot calls
   - first-class method refs (including polymorphic + annotated cases)

## Risks

- Boot-order mistakes can cause missing signatures during early stages.
- Removing hardcoded fallbacks too early can break existing builtin behavior.
- Some APIs may still require explicit contextual typing rules even after de-hardcoding.

## Mitigations

1. Land in phases with characterization tests first.
2. Keep old and new lookup paths behind temporary checks during migration.
3. Add strict startup/assertion checks for intrinsic binding integrity.
4. Validate every phase with interpreter + wasm suites.

## Success Criteria

1. Changing a trusted-module signature requires no Rust typechecker/lower signature edits.
2. Method call and method-value reference paths resolve through the same signature source.
3. Interpreter and wasm backends stay in parity for method/module call behavior.
4. No remaining hardcoded callable-shape tables for stdlib/prelude APIs outside explicit, documented exceptions.

