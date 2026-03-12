# Archived Plans Index

This directory contains completed or historical plan documents.

Use [../README.md](../README.md) for active WIP/planned docs.

---

## Archived Stage Plans

| Stage | Description | Status | Details |
|-------|-------------|--------|---------|
| 0 | Skeleton & Testing Infrastructure | Done | [frontend.md](frontend.md) |
| 1 | Lexer, Parser, Spans | Done | [frontend.md](frontend.md) |
| 2 | Name Resolution & Monomorphic Typechecking | Done | [frontend.md](frontend.md) |
| 3 | Core IR Design & Lowering | Done | [core-ir.md](core-ir.md) |
| 4 | Module System & Inherent Method Desugaring | Done | [core-ir.md](core-ir.md) |
| 5 | Core IR Interpreter | Done | [core-ir.md](core-ir.md) |
| 6a | User-Defined Generics | Done | [type-system.md](type-system.md) |
| 6b | Query-Friendly Pipeline Refactor | Done | [type-system.md](type-system.md) |
| 6c | Full Damas-Milner Inference | Done | [type-system.md](type-system.md) |
| 7 | ANF IR (Backend-Oriented) | Done | [optimization.md](optimization.md) |
| 7.5 | Dataflow Analysis & ANF Optimization | Done | [optimization.md](optimization.md) |
| 7.6 | Defer | Done | [optimization.md](optimization.md) |
| 8a | Runtime IR + Linker | Done | [wasm-backend.md](wasm-backend.md) |
| 8b | Runtime Modules | Done | [wasm-backend.md](wasm-backend.md) |
| 8c | ANF → WAT Emitter | Done | [wasm-backend.md](wasm-backend.md) |
| 8d | Full Build Pipeline | Done | [wasm-backend.md](wasm-backend.md) |
| 8e | Standard Library | Done | [wasm-backend.md](wasm-backend.md) |
| 9 | Host Integration & Validation | Done | [host-validation.md](host-validation.md) |
| 9.6 | Typed Closure Specialization | Done | [typed-closure-specialization.md](typed-closure-specialization.md) |
| 9.7 | Standard Library & API Gaps | Done | [stdlib-api-gaps.md](stdlib-api-gaps.md) |

Related historical context outside this folder:

* [monomorphization.md](../../internals/monomorphization.md)

---

## Archived Cross-Cutting Plans

| Plan | Description |
|------|-------------|
| [backend-pipeline-alignment.md](backend-pipeline-alignment.md) | Align backend pipeline to operate on monomorphized Core IR |
| [string-unicode-semantics.md](string-unicode-semantics.md) | Byte-first string semantics with explicit Unicode APIs |
| [byte-first-fs-read-api.md](byte-first-fs-read-api.md) | Migrate file-read host ABI and stdlib layering to byte-first semantics |
| [bytes-followup-hardening.md](bytes-followup-hardening.md) | Follow-up hardening for byte semantics, intrinsic contracts, and unfold callback typing |
| [vector-type.md](vector-type.md) | Replace `Array<T>` with `Vector<T>` |
| [to-string-method-unification.md](to-string-method-unification.md) | Unify string conversion via `.to_string()` |
| [bitwise-operations.md](bitwise-operations.md) | Add bitwise operators for Int/Byte with interpreter/Wasm parity |
| [uniqueness-optimization.md](uniqueness-optimization.md) | Uniqueness-based in-place update optimization |
| [hex-literals.md](hex-literals.md) | Hexadecimal integer literal syntax |
| [prelude-stdlib.md](prelude-stdlib.md) | Auto-available prelude inherent methods |
| [wasm-iterator-representation-boundaries.md](wasm-iterator-representation-boundaries.md) | Stabilize iterator specialization boundaries in Wasm backend |
| [wasm-type-erasure-reduction.md](wasm-type-erasure-reduction.md) | Reduce type erasure in Wasm backend with monomorphized layouts |
| [wasm-sum-representation-boundary-unification.md](wasm-sum-representation-boundary-unification.md) | Unify typed/erased Option/Result/Variant boundary handling to prevent cast-failure regressions |
| [wasm-option-amatch-typed-metadata.md](wasm-option-amatch-typed-metadata.md) | Preserve typed Option/Result metadata for `AMatch`-produced locals |
| [anf-analysis-consolidation.md](anf-analysis-consolidation.md) | Consolidate ANF traversal analyses into shared utilities with codegen/optimizer parity guardrails |
| [intrinsic-registry-unification.md](intrinsic-registry-unification.md) | Unify intrinsic/prelude metadata into one canonical registry |
| [stdlib-signature-source-of-truth.md](stdlib-signature-source-of-truth.md) | Make `prelude/*` and `stdlib/*` the signature source of truth; remove cross-file Rust signature duplication |
| [module-compile-orchestrator-refactor.md](module-compile-orchestrator-refactor.md) | Refactor module compile orchestration into dependency, stage-runner, and env-integration layers |
| [codegen-boundary-separation.md](codegen-boundary-separation.md) | Separate codegen planning, representation-flow analysis, and instruction emission |
| [string-escape-sequences.md](string-escape-sequences.md) | Add ergonomic string escapes (`\xNN`, `\e`, `\u{...}`) with lexer diagnostics and runtime coverage |
| [anf-verifier-pass.md](anf-verifier-pass.md) | ANF invariant verifier pass for control-flow, local binding, representation, and codegen metadata consistency |
| [first-class-inherent-methods.md](first-class-inherent-methods.md) | First-class inherent method values (`receiver.method` → closure) |
