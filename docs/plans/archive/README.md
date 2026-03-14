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
| [byte-contextual-int-literals.md](byte-contextual-int-literals.md) | Allow in-range integer literals to satisfy `Byte` in expected-type contexts without enabling general implicit narrowing |
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
| [lsp-diagnostics-completion.md](lsp-diagnostics-completion.md) | Phase 2 plan for `twk lsp` diagnostics publishing, completion, and simple `///` doc comments |
| [lsp-hover-goto-definition.md](lsp-hover-goto-definition.md) | Phase 1 implementation plan for `twk lsp` hover and go-to-definition |
| [twinkle-test-runner-suites.md](twinkle-test-runner-suites.md) | Twinkle-native test runner and suite infrastructure in `boot/tests/` |
| [string-graphemes.md](string-graphemes.md) | `String.graphemes()` for extended grapheme cluster iteration (UAX #29) |
| [iterator-to-vector.md](iterator-to-vector.md) | `Iterator.to_vector()` method-form materialization equivalent to `collect` |
| [module-relative-imports.md](module-relative-imports.md) | Explicit relative module imports via `use .foo` syntax |
| [option-result-ergonomics.md](option-result-ergonomics.md) | `Option.ok_or` / `Option.ok_or_else` and `try Option` support |
| [option-result-transpose.md](option-result-transpose.md) | Symmetric `Option.transpose` / `Result.transpose` conversions |
| [api-ergonomics-minimal.md](api-ergonomics-minimal.md) | Minimal ergonomic APIs: `Vector.sort_by`, lazy `Iterator.map/filter/take`, and `Option/Result map/and_then` |
| [record-constructor-aliases.md](record-constructor-aliases.md) | Alias-based record constructors (`P.{ ... }` where `type P = Point`) |
| [resolver-alias-ordering.md](resolver-alias-ordering.md) | Topological sort for alias resolution ordering to fix alias chain resolution |
| [deterministic-wat-output.md](deterministic-wat-output.md) | Stabilize resolver declaration ordering so emitted WAT snapshots are deterministic across runs |
| [order-comparator-api.md](order-comparator-api.md) | `Order` type, primitive `compare` methods, `Vector.sort_by` migration to `Order` comparators |
| [dict-byte-keys.md](dict-byte-keys.md) | Allow `Byte` as a `Dict` key type alongside `Int` and `String` |
| [pre-selfhost-cleanup.md](pre-selfhost-cleanup.md) | Pre-self-hosting stage0 cleanup tasks — remaining items superseded by self-hosting redesign |
| [boot-source-lib.md](boot-source-lib.md) | `boot/lib/source` — spans, file registry, diagnostics for self-hosted compiler |
