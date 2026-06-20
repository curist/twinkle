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

## Archived Self-Hosting Plans (Stage 10 — Fixed-Point Achieved)

| Plan | Description |
|------|-------------|
| [self-hosting.md](self-hosting.md) | Stage 10 design document for the self-hosted compiler pipeline and bootstrap architecture |
| [self-hosting-status.md](self-hosting-status.md) | Fixed-point self-hosting status tracker and post-milestone follow-up index |
| [boot-multi-module.md](boot-multi-module.md) | Phase E multi-module compilation — import scanning, env merging, recursive loading, linker integration; complete with fixed-point self-host loop |
| [boot-selfhosted-wasm-repr-parity.md](boot-selfhosted-wasm-repr-parity.md) | Close self-hosted/runtime representation mismatches (sum match/pattern lowering, erased boundaries, alias-backed layouts) — unblocked by fixed-point self-hosting |
| [boot-backend-physical-typing.md](boot-backend-physical-typing.md) | Centralize actual-vs-expected Wasm type adaptation at erased backend boundaries — canonical coercion helper, boundary shims, verifier edge checks |

---

## Archived Language Feature Plans

| Plan | Description |
|------|-------------|
| [compound-rebinding.md](compound-rebinding.md) | Rebinding receiver shorthand: `x = .method(args)` desugars to `x = x.method(args)` at parse time |
| [cond.md](cond.md) | `cond` expression for boolean dispatch chains — desugars to nested `If`, replaces deeply nested `if/else if/else` |
| [extern-types.md](extern-types.md) | `externref`-backed opaque host handles for FFI — Phase 1 (non-null) and Phase 2 (nullable `Option<ExternType>`) |
| [tuple.md](tuple.md) | Design rationale for a library-first `@std.tuple` (`Pair` + terse constructor); why nominal records over `(a, b)` syntax. Superseded by `tuple-and-alias-reexport.md` |
| [tuple-and-alias-reexport.md](tuple-and-alias-reexport.md) | Shipped `@std.tuple` (`Pair`/`Triple`, `.first`/`.second`/`.third`, `tuple.pair`/`tuple.triple`) on top of three resolver fixes that make transparent type-alias re-export work (TypeId-keyed cycle detection, eager alias expansion, inherent-method re-export through aliases) |
| [string-literals.md](string-literals.md) | Raw single-line (`r"…"`) and Zig-style `\\` multiline string literals, both with interpolation; lexer-only resolution emitting the existing string tokens, formatter form-preservation (incl. interpolated raw/multiline), lint L1 for single-line `\\` blocks, EBNF + tree-sitter grammar. Supersedes [regexp-percent-escapes.md](regexp-percent-escapes.md) |
| [regexp-v2-linear.md](regexp-v2-linear.md) | Extended `@std.regexp` with option flags, scoped inline flags, lazy quantifiers, named groups, and explicit rejection of backreference syntax while preserving the Pike VM linear-time model |
| [not-operator-transition.md](not-operator-transition.md) | Decision record rejecting a migration from prefix `!` to word-form `not`; keeps `!` as canonical boolean negation |
| [direct-control-flow-case-arms.md](direct-control-flow-case-arms.md) | Direct `return`, `break`, and `continue` arm bodies for `case`/`cond`, plus formatter normalization from braced terminal arm blocks |

---

## Archived Boot Compiler Refactor Plans

| Plan | Description |
|------|-------------|
| [boot-wasm-type-ordering-refactor.md](boot-wasm-type-ordering-refactor.md) | Shared Wasm GC type dependency/SCC ordering between WAT and binary emission, with planner ownership intentionally kept as an emitter utility decision |
| [boot-optimizer-pass-refactor.md](boot-optimizer-pass-refactor.md) | Made optimizer context, pass ordering, fixed-point policy, and timing aggregation explicit while preserving existing optimization semantics |
| [boot-backend-verifier-refactor.md](boot-backend-verifier-refactor.md) | Split backend verification into slot, expression, and shared-helper modules with explicit basic/full verification levels |
| [boot-frontend-query-refactor.md](boot-frontend-query-refactor.md) | Split frontend analysis around module interfaces, dependency-interface fingerprints, and lower-churn import environment construction |
| [boot-phase-file-decomposition.md](boot-phase-file-decomposition.md) | Decomposed the core lowerer and Wasm emitter into focused helper modules while preserving public entrypoints; parser split remains intentionally conservative and checker decomposition deferred |
| [boot-compiler-cleanup.md](boot-compiler-cleanup.md) | Six independent cleanup tasks: extract fix_unused_imports, break up core_linker, deduplicate LSP JSON helpers, fix quadratic workspace diagnostics, shared AST offset walker for hover/definition, fix method_function_name dispatch |
| [br-table-case-dispatch.md](br-table-case-dispatch.md) | Emit `br_table` dispatch for eligible variant case expressions in the boot compiler |
| [string-case-dispatch.md](string-case-dispatch.md) | Emit length-plus-trie dispatch for eligible string case expressions in the boot compiler |
| [unreachable-case-arms.md](unreachable-case-arms.md) | Diagnose case arms shadowed by earlier catch-all, duplicate literal, or covering variant patterns |
| [dedup-failed-module-diagnostics.md](dedup-failed-module-diagnostics.md) | Memoize attempted-and-failed modules so a broken shared dependency reports its errors once, not once per importer |
| [boot-typed-builtin-refs.md](boot-typed-builtin-refs.md) | Resolve builtin `Option`/`Result` variant ids by name into `builtin_refs.tw` instead of hardcoding layout in `try`/`for`-in lowering and `variant_index`/`type_id_from_mono`; site-local resolution keeps names at the bootstrap boundary. Follow-up for builtin *types* tracked in [../boot-typed-builtin-type-refs.md](../boot-typed-builtin-type-refs.md) |

---

## Archived Tooling Plans

| Plan | Description |
|------|-------------|
| [formatter.md](formatter.md) | Source formatter (`twk fmt`) — lossless trivia model, Wadler-Lindig Doc IR, AST printer, Fill layout for imports, trailing comment preservation |
| [linter.md](linter.md) | `twk lint`: opt-in review command with report-only lints L2/L3/L4/L5, auto-fixable rewrites R1/R2, import-chain scope, per-rule `--fix-*`, and `--explain`; L1 unused-pure-result explicitly deferred |
| [inherent-method-hint.md](inherent-method-hint.md) | `twk lint` R1 rewrite: free-function calls rewritten into equivalent inherent-method call syntax when resolution proves the forms identical |
| [rebinding-through-path-lint.md](rebinding-through-path-lint.md) | `twk lint` L5 `direct-rebinding`: report-only lint flagging a `tmp := <path>` alias whose only use is rebinding-updates-then-copy-back, where direct field/index-path rebinding expresses the update. Report-only rule shipped (auto-fix deferred); structural AST visitor in `compiler/lint.tw` |
| [js-package-browser-command-api.md](js-package-browser-command-api.md) | Browser `command(args, opts)` API for `@twinkle-lang/twinkle/web`, exposing the shipped compiler payload to embedders for `fmt`, `check`, `ir`, and `build` against an in-memory project |
| [tooling.md](tooling.md) | Historical tooling and ecosystem roadmap from before the boot compiler became the primary implementation path |

---

## Archived Cross-Cutting Plans

| Plan | Description |
|------|-------------|
| [channels.md](channels.md) | CSP-style `Channel<T>` on top of the cooperative `Task` scheduler: unbuffered/bounded channels, `send`/`recv`/`close`, iterator support, and LSP migration away from `Cell` queues and poll loops |
| [channels-impl-handoff.md](channels-impl-handoff.md) | Historical Phase 1 implementation handoff for the channel primitive, including compiler/runtime/stage0 integration notes |
| [channels-phase2-handoff.md](channels-phase2-handoff.md) | Historical Phase 2 handoff for migrating the LSP queues to channels |
| [node-standalone-runtime.md](node-standalone-runtime.md) | Standalone Node.js Twinkle compiler/runtime entry (`twk_boot.mjs` + `run_wasm_node.mjs`) without requiring Rust `twk` |
| [builtin-surface-binding-cleanup.md](builtin-surface-binding-cleanup.md) | Make boot builtin visibility explicit — canonical public names and internal helpers separated by construction via `with_registered_functions` + `bind_public_free_builtins` |
| [builtin-identity-resolution.md](builtin-identity-resolution.md) | Separate compiler-owned builtin identities from user function names across lowering, backend callable materialization, and codegen dispatch |
| [defer-implementation-drift.md](defer-implementation-drift.md) | Block-scoped defer semantics — aligned docs, interpreter, ANF defer-elim pass, pipeline ordering, and tests across Rust and boot compilers |
| [boot-first-class-builtin-functions.md](boot-first-class-builtin-functions.md) | First-class builtin / prelude function values in the boot backend; archived after the closure-materialization pipeline and regression coverage landed |
| [deferred-persistence.md](deferred-persistence.md) | Earlier consolidated uniqueness/persistence strategy doc now superseded by the active static-uniqueness and persistent-container plans |
| [boot-function-identity-canonicalization.md](boot-function-identity-canonicalization.md) | Canonical imported function identity across resolver, lowering, module compilation, and core linking so alias spelling no longer affects cross-module linkage |
| [boot-module-type-identity.md](boot-module-type-identity.md) | Canonical imported nominal type identity across full/selective/transitive boot module boundaries; behavioral closure complete, deeper hidden-import cleanup deferred to the follow-on binding-model plan |
| [boot-no-hidden-imports.md](boot-no-hidden-imports.md) | Remove hidden selective-import namespaces by splitting canonical import storage from visible bindings, completing support-type closure, and moving method lookup to receiver identity |
| [module-global-type-tracking.md](module-global-type-tracking.md) | Gave module globals distinct `GlobalId` identity through Core/ANF/prepared IR, sourced concrete global types from linked `GlobalSet` values, and restored `CompletionItemKind` constants as module-level value bindings |
| [equality-followup.md](equality-followup.md) | Finish typed-sum structural equality, broaden collection-equality regression coverage, and lock down the boundary between structural collection equality and record identity |
| [backend-pipeline-alignment.md](backend-pipeline-alignment.md) | Align backend pipeline to operate on monomorphized Core IR |
| [optimizer-generalization.md](optimizer-generalization.md) | Generalize optimizer semantics, shared analysis, and canonical builder/transient boundaries without requiring explicit transient IR forms yet |
| [pragmatic-persistent-vector.md](pragmatic-persistent-vector.md) | Pragmatic `PVec` implementation plan using existing `rt.arr` ABI; now landed on both stage0 and boot mirror |
| [persistent-vector-benchmark-followup.md](persistent-vector-benchmark-followup.md) | Early benchmark investigation for persistent vector regressions before the later read-path follow-up settled the direction |
| [persistent-vector-i64-poc.md](persistent-vector-i64-poc.md) | Historical stage0-only `Vector<Int>` typed-family POC that predates the shipped pragmatic `PVec` approach |
| [persistent-vector-read-path-followup.md](persistent-vector-read-path-followup.md) | Read-path performance investigation and landed leaf-wrapper removal follow-up for persistent vectors |
| [pvec-performance-enhancements.md](pvec-performance-enhancements.md) | Incremental PVec runtime optimizations: i31ref builder tail, promote_full_tail, bulk array conversion, leaf-oriented slice/extend, true in-place set |
| [dict-performance-enhancements.md](dict-performance-enhancements.md) | Incremental HAMT dict runtime optimizations: hot-path inlining, in-place mutation for unique dicts |
| [dict-in-place-alias-safety.md](dict-in-place-alias-safety.md) | Deep ownership analysis ensuring in-place dict/vector mutation is alias-safe |
| [typed-dict-representation.md](typed-dict-representation.md) | NOT PURSUED — Phase 0 microbenchmarks (`boot/bench/dict_*`/`set_*`) showed dict cost is allocation/insertion-order-bound, not key-handling: typed `i64` keys save ≈0 (forced `BoxedInt` build == unboxed i31 build), only `String` reads (~2×) would gain. Surfaced the real lever — O(n²) bulk `remove`. Baselines kept as a regression guard in `boot/bench/README.md` |
| [dict-amortized-remove.md](dict-amortized-remove.md) | Replaced per-remove insertion-order-vector rebuilds with tombstoned order slots and density-triggered compaction, making `Dict.remove`/`Set.remove` amortized O(log n) and bulk remove linear |
| [string-builder-uniqueness-plan.md](string-builder-uniqueness-plan.md) | Internal string builder runtime helpers plus static uniqueness rewrites for loop and straight-line `String.concat` accumulator regions |
| [collections-access.md](collections-access.md) | Umbrella for the collection-access cluster (cheap + general indexing/slice/concat/traversal); archived once the audit, `drop_last`, access-contracts, `View`, RRB, and the `[a..b]` range-slice ([sliceable.md](sliceable.md)) work all landed |
| [sliceable.md](sliceable.md) | `foo[a..b]` range-slice syntax via a `Self`-only `Sliceable` contract — the final piece of the collection-access cluster. A literal-range index arm in `synth_index` plus uniform `Sliceable.slice` lowering; `Vector`/`String`/`View` satisfy it. Boot-only; incidentally removed the monomorphize no-generics early-return so concrete contract calls still lower |
| [rrb-vector-concat.md](rrb-vector-concat.md) | RRB-tree `Vector` concat/slice implementation plan and benchmarks; runtime work landed in boot and stage0, with Phase 7 classical-slack experiment archived as a wash |
| [vector-gather.md](vector-gather.md) | `Vector.gather` v1 runtime primitive plus dataframe gather-path cleanup; join null-fill and head improved, while filter's monotonic-index win remains future trie-aware gather work |
| [slice-performance.md](slice-performance.md) | Boot-compiler `slice`/`concat` audit — the evidence behind the collections-access cluster; Vector LIFO landed via `drop_last`, String-slice routed to `View` |
| [stack.md](stack.md) | O(1)-amortized `Vector.drop_last` runtime op (LIFO pop sites migrated); the thin `Stack<T>` wrapper it once carried was tried and removed as unused |
| [queue.md](queue.md) | Shipped library-first `@std.queue` persistent double-ended queue; FIFO/deque-heavy code avoids repeated `Vector.drop_first()` front slices by using a two-vector representation oriented around append/drop-last |
| [priority-queue.md](priority-queue.md) | Shipped library-first `@std.heap` persistent priority queue as a pairing heap with the comparator stored in the heap; min/max-heap by choice of comparator (or `new_ord` for the `Ord` contract), amortized O(1) `push`/`merge` and O(log n) `pop` |
| [access-contracts.md](access-contracts.md) | Parameterized contracts `IndexRead<E>`/`IndexWrite<E>`/`IntoIterator<E>` with a `Self → E` functional dependency; write-once generic access monomorphized to direct reads; `v[i]` and `for x in` wired through the contracts |
| [access-contract-hardening.md](access-contract-hardening.md) | Hardening follow-up for access contracts: bound arity diagnostics, element-aware proof caching, explicit iteration lowering, contract lookup cleanup, signature-driven bound-only monomorphization recovery, and total `View.sub` clamping |
| [view.md](view.md) | `View<C>` zero-copy windows over any `IndexRead` backing (`@std.view`); O(1) `drop_first`/`drop_last`/`sub`, the first stdlib access-contract satisfier |
| [collection-ergonomics.md](collection-ergonomics.md) | Common `Vector` and `View` helpers (`take`, `drop`, `zip_with`, materializing view operations, and owned-vector conveniences); `Vector.chunks`/`windows` deferred to [prelude-stdlib-type-interfaces.md](prelude-stdlib-type-interfaces.md) |
| [prelude-stdlib-type-interfaces.md](prelude-stdlib-type-interfaces.md) | Reserved stdlib type interface mechanism that lets prelude methods return stdlib-defined nominal types; shipped `Vector.chunks` / `Vector.windows` returning `View` windows with shared `View` identity |
| [wasm-tail-calls.md](wasm-tail-calls.md) | Wasm tail-call emission (`return_call` for direct calls, `return_call_ref` for closures) as a required target feature |
| [dict-wyhash-i64.md](dict-wyhash-i64.md) | Migrate active FNV-derived hashing to deterministic wyhash v3 and carry 64-bit hashes through the Dict HAMT |
| [string-unicode-semantics.md](string-unicode-semantics.md) | Byte-first string semantics with explicit Unicode APIs |
| [byte-first-fs-read-api.md](byte-first-fs-read-api.md) | Migrate file-read host ABI and stdlib layering to byte-first semantics |
| [bytes-followup-hardening.md](bytes-followup-hardening.md) | Follow-up hardening for byte semantics, intrinsic contracts, and unfold callback typing |
| [byte-contextual-int-literals.md](byte-contextual-int-literals.md) | Allow in-range integer literals to satisfy `Byte` in expected-type contexts without enabling general implicit narrowing |
| [vector-type.md](vector-type.md) | Replace `Array<T>` with `Vector<T>` |
| [vector-backend-repr-inference.md](vector-backend-repr-inference.md) | Stage0 backend slice for separating semantic `Vector<T>` typing from physical vector-family representation, completed for the concrete `Vector<Int>` typed container/helper path |
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
| [stage0-illegal-cast.md](stage0-illegal-cast.md) | Postmortem for the stage0 module-global LocalId collision that made a `PVec` local emit as `Array`, plus the stage0/boot init-binding fixes |
| [intrinsic-registry-unification.md](intrinsic-registry-unification.md) | Unify intrinsic/prelude metadata into one canonical registry |
| [stdlib-signature-source-of-truth.md](stdlib-signature-source-of-truth.md) | Make `prelude/*` and `stdlib/*` the signature source of truth; remove cross-file Rust signature duplication |
| [module-compile-orchestrator-refactor.md](module-compile-orchestrator-refactor.md) | Refactor module compile orchestration into dependency, stage-runner, and env-integration layers |
| [codegen-boundary-separation.md](codegen-boundary-separation.md) | Separate codegen planning, representation-flow analysis, and instruction emission |
| [boot-shared-type-helpers.md](boot-shared-type-helpers.md) | Consolidate shared boot compiler type substitution, prepared backend fact, and Wasm type comparison helpers |
| [boot-contract-reachability-index.md](boot-contract-reachability-index.md) | Pre-index contract-call fallback targets in the Core linker reachability pass |
| [string-escape-sequences.md](string-escape-sequences.md) | Add ergonomic string escapes (`\xNN`, `\e`, `\u{...}`) with lexer diagnostics and runtime coverage |
| [anf-verifier-pass.md](anf-verifier-pass.md) | ANF invariant verifier pass for control-flow, local binding, representation, and codegen metadata consistency |
| [first-class-inherent-methods.md](first-class-inherent-methods.md) | First-class inherent method values (`receiver.method` → closure) |
| [lsp-completion.md](lsp-completion.md) | LSP completion reliability — broken-edit fallback, cursor-hole re-parsing, local variable completions, import module discovery |
| [lsp-editor-source-recovery.md](lsp-editor-source-recovery.md) | Shared LSP editor-source recovery — central editor snapshots and cursor/source context for completion and signature help |
| [lsp-diagnostics-completion.md](lsp-diagnostics-completion.md) | Phase 2 plan for `twk lsp` diagnostics publishing, completion, and simple `///` doc comments |
| [lsp-hover-goto-definition.md](lsp-hover-goto-definition.md) | Phase 1 implementation plan for `twk lsp` hover and go-to-definition |
| [lsp-file-watching.md](lsp-file-watching.md) | Watched-file refresh plan for `twk lsp` disk snapshot updates and diagnostics refresh |
| [task-concurrency.md](task-concurrency.md) | Superseded stackless task-concurrency plan; retained for public API rationale and historical context before the JSPI-fiber backend replaced the state-machine approach |
| [task-concurrency-jspi-fiber.md](task-concurrency-jspi-fiber.md) | Stackful cooperative `Task<T>` design using JSPI fibers as the current backend, archived after implementation landed |
| [task-concurrency-jspi-fiber-implementation.md](task-concurrency-jspi-fiber-implementation.md) | Checkpoint implementation plan for the JSPI task backend, archived after the scheduler, compiler lowering, stage0 parity, and LSP adoption landed |
| [lsp-task-migration.md](lsp-task-migration.md) | CP19 migration of LSP transport, debounce timers, and diagnostics publication onto cooperative `Task` primitives; future worker-isolation end-game notes preserved in the plan |
| [task-api-separation-of-concerns.md](task-api-separation-of-concerns.md) | Kept `Task<T>` focused on task composition by moving timer/stdin suspension points to `@std.time` and `@std.io`, with scheduler-aware host imports preserving JSPI task invariants |
| [lsp-save-vs-edit-gating.md](lsp-save-vs-edit-gating.md) | Split LSP diagnostics into parse-only edit-tier work and full workspace analysis on open/save/close |
| [lsp-save-vs-edit-gating-implementation.md](lsp-save-vs-edit-gating-implementation.md) | Implementation checklist for save-vs-edit diagnostics gating, archived after the branch landed |
| [boot-lsp-query-layer.md](boot-lsp-query-layer.md) | Boot LSP query layer for workspace-aware diagnostics, source overlays, cache invalidation, and reusable semantic snapshots |
| [boot-lsp-hover-parity.md](boot-lsp-hover-parity.md) | Boot LSP hover parity with stage0: symbol-aware function/method hover, binders, user and prelude docs, variant hover, and UTF-16 stability |
| [twinkle-test-runner-suites.md](twinkle-test-runner-suites.md) | Twinkle-native test runner and suite infrastructure in `boot/tests/` |
| [string-graphemes.md](string-graphemes.md) | `String.graphemes()` for extended grapheme cluster iteration (UAX #29) |
| [iterator-to-vector.md](iterator-to-vector.md) | `Iterator.to_vector()` method-form materialization equivalent to `collect` |
| [module-relative-imports.md](module-relative-imports.md) | Explicit relative module imports via `use .foo` syntax |
| [option-result-ergonomics.md](option-result-ergonomics.md) | `Option.ok_or` / `Option.ok_or_else` and `try Option` support |
| [option-result-transpose.md](option-result-transpose.md) | Symmetric `Option.transpose` / `Result.transpose` conversions |
| [scripting-ergonomics.md](scripting-ergonomics.md) | Scripting ergonomics API gaps — string helpers, vector combinators, Set type, iterator adapters |
| [api-ergonomics-minimal.md](api-ergonomics-minimal.md) | Minimal ergonomic APIs: `Vector.sort_by`, lazy `Iterator.map/filter/take`, and `Option/Result map/and_then` |
| [record-constructor-aliases.md](record-constructor-aliases.md) | Alias-based record constructors (`P.{ ... }` where `type P = Point`) |
| [record-field-punning.md](record-field-punning.md) | Record literal field punning shorthand (`.{ x }` => `.{ x: x }`) with parser/tooling/docs alignment |
| [resolver-alias-ordering.md](resolver-alias-ordering.md) | Topological sort for alias resolution ordering to fix alias chain resolution |
| [deterministic-wat-output.md](deterministic-wat-output.md) | Stabilize resolver declaration ordering so emitted WAT snapshots are deterministic across runs |
| [order-comparator-api.md](order-comparator-api.md) | `Order` type, primitive `compare` methods, `Vector.sort_by` migration to `Order` comparators |
| [dict-byte-keys.md](dict-byte-keys.md) | Allow `Byte` as a `Dict` key type alongside `Int` and `String` |
| [pre-selfhost-cleanup.md](pre-selfhost-cleanup.md) | Pre-self-hosting stage0 cleanup tasks — remaining items superseded by self-hosting redesign |
| [boot-source-lib.md](boot-source-lib.md) | `boot/lib/source` — spans, file registry, diagnostics for self-hosted compiler |
| [inference-contextual-gaps.md](inference-contextual-gaps.md) | Contextual inference for variant shorthand in generic args and unannotated closure callback params |
| [qualified-variant-constructor-paths.md](qualified-variant-constructor-paths.md) | Allow `module.Type.Variant` in expressions while keeping `x.Variant` rejected |
| [naming-case-enforcement.md](naming-case-enforcement.md) | Enforce initial-case naming rules in compiler behavior to match spec/grammar |
| [destructuring-imports.md](destructuring-imports.md) | Add `use module.{...}` for value/type/mixed imports with per-item aliasing (no `self`/wildcard) |
| [import-type-keyword-removal.md](import-type-keyword-removal.md) | Remove redundant `type` keyword from destructuring imports — infer from PascalCase/snake_case |
| [boot-parser-gap-closure.md](boot-parser-gap-closure.md) | Close bootstrap parser gaps: richer token coverage, structural AST, robust recovery, multiline continuity, and destructuring imports |
| [eq-ne-type-propagation.md](eq-ne-type-propagation.md) | Propagate known operand type across `==`/`!=` so context-dependent literals type-check bidirectionally |
| [boot-compiler-refactor.md](boot-compiler-refactor.md) | Reduce duplication in boot compiler parser/lexer via helper extraction and dead code removal |
| [boot-parser-test-coverage.md](boot-parser-test-coverage.md) | Boot parser test coverage vs grammar; hex literals, void result `!E`, collect, and edge cases |
| [boot-resolver-fixes.md](boot-resolver-fixes.md) | Boot resolver fixes: arity checks, duplicate fn dedup, TypeEntry spans, error collection, topo-sorted type resolution |
| [bug-codegen-cell-verify-panic.md](bug-codegen-cell-verify-panic.md) | Fix debug_assert panic for module-global Cell locals in codegen verification |
| [bug-record-field-type-leak.md](bug-record-field-type-leak.md) | Record field type leak across functions — no longer reproduces |
| [checker-variant-dispatch.md](checker-variant-dispatch.md) | Unify Optional/Result/Sum variant dispatch in self-hosted checker |
| [boot-checker-stage0-parity.md](boot-checker-stage0-parity.md) | Stage0 parity closure plan for the self-hosted checker (assignment/call dispatch/bitwise/interpolation/pass-order alignment) |
| [boot-checker-inference-consistency.md](boot-checker-inference-consistency.md) | Tighten contextual call inference, closure annotation reconciliation, duplicate-field validation, ambiguity deduplication, and checker semantics docs |
| [boot-checker-drift-fixes.md](boot-checker-drift-fixes.md) | Post-parity drift fixes: range type, Byte/Int promotion, shift ops, defer Never, directional equality, call_expected_ret |
| [boot-checker-refactor.md](boot-checker-refactor.md) | Extract helpers (bind_optional, find_record_field_type, check_args, etc.) to reduce duplication in boot checker |
| [parser-diagnostic-parity.md](parser-diagnostic-parity.md) | Parser diagnostic message parity between Rust and boot compilers (phases 1-4: message templates, context, shared fixtures) |
| [boot-pub-rebinding.md](boot-pub-rebinding.md) | Align boot checker with stage0 pub-only rebinding restriction |
| [boot-snapshot-testing.md](boot-snapshot-testing.md) | Snapshot testing for boot compiler diagnostics (`.boot.expected` files, `TWK_SNAP_UPDATE=1`) |
| [boot-builtin-registry.md](boot-builtin-registry.md) | Centralized builtin FuncId registry for boot compiler — eliminates hardcoded magic numbers |
| [boot-core-ir.md](boot-core-ir.md) | Boot Core IR types, AST→Core IR lowering, monomorphization (Phase B) |
| [boot-anf-lowering.md](boot-anf-lowering.md) | Boot ANF lowering & optimization — ANF IR types, Core→ANF lowering, analysis, peephole passes, liveness, uniqueness, defer elimination (Phase C) |
| [boot-type-checker.md](boot-type-checker.md) | Boot type checker milestones M1–M9 (Phase A) |
| [boot-resolver-method-registry.md](boot-resolver-method-registry.md) | Boot resolver method registry M1–M4; M5 deferred to multi-module |
| [boot-resolver-hardening.md](boot-resolver-hardening.md) | Boot resolver hardening and edge-case fixes |
| [string-interning.md](string-interning.md) | Compile-time string literal interning (Phase 1 landed; runtime Phase 2 optional) |
| [method-resolution-spec-alignment.md](method-resolution-spec-alignment.md) | Align dot-method resolution with spec: defining-module inherent methods only (Phases 1–2 done; Phase 3–4 deferred) |
| [method-resolution-via-type.md](method-resolution-via-type.md) | Method resolution via type origin — destructured/transitive imports resolve inherent methods without the defining module in scope |
| [boot-codegen-m11-gap-closure.md](boot-codegen-m11-gap-closure.md) | Boot codegen M11 gap closure — sum/variant, control-flow, match, record parity fixes and regression matrix |
| [boot-codegen.md](boot-codegen.md) | Boot codegen Phase D — Wasm IR, type planning, boundary insertion, WAT emission, linker, runtime modules |
| [boot-codegen-hardening.md](boot-codegen-hardening.md) | Boot codegen hardening — checked narrowing, closure repr, deterministic string pool, match unification, typed module globals, boundary tightening |
| [boot-codegen-followup.md](boot-codegen-followup.md) | Boot codegen follow-up — ABI metadata consolidation, match-emitter unification, structural M11 validation, typed-ref boundary fix |
| [boot-multi-module-cleanup.md](boot-multi-module-cleanup.md) | Follow-up cleanup after landing boot multi-module compilation steps 1–3; selective-import parity, canonical path caching, helper dedup, and snapshot/diagnostic polish |
| [boot-signature-source-of-truth.md](boot-signature-source-of-truth.md) | Boot-side signature source of truth via `prelude/signatures/*.tw` plus `builtins.tw` canonical mapping |
| [interproc-uniqueness.md](interproc-uniqueness.md) | Interprocedural consume-at-call-site analysis — Gap 1 (ARecordGet dying struct propagation) and Gap 2 (consumed-param shell reuse) both landed in boot compiler; ~11% compile_modules improvement |
| [range-literal-syntax.md](range-literal-syntax.md) | `m..n` range literal syntax desugaring to `range_from` — stage0 and boot compiler, tree-sitter grammar, precedence fix |
| [collection-literal-type-inference.md](collection-literal-type-inference.md) | Stage0 deferred type inference for `Dict.new()` and empty `[]` literals — MetaVar emission with scope-exit drain; workaround removed from `boot/lib/json.tw` |
| [monomorphize-return-type-inference.md](monomorphize-return-type-inference.md) | Return-type-driven monomorphization inference for generic calls whose type params are not solved by arguments, plus adjacent closure-target and boot type-param collection fixes discovered during validation |
| [contracts.md](contracts.md) | Contracts MVP: builtin `Stringify`, bounded type parameters, conditional witness-based satisfaction, deferred contract-backed lowering, and prelude-backed builtin generic conformance |
| [boot-test-suite-cast-fix.md](boot-test-suite-cast-fix.md) | Fix `illegal cast` failures when large cross-module `Vector<runner.Suite>` values are aggregated — root cause was unqualified suffix-name aliasing in monomorphization; fixed in `lower.rs` and `monomorphize.rs` |
| [salsa-refactor-plan.md](salsa-refactor-plan.md) | Salsa-like incremental query refactor — shared analyze module, exports fingerprinting for early cutoff, revision tracking, TypeId registry, unified build/LSP path |
| [unused-import-cleanup.md](unused-import-cleanup.md) | Unused import detection with edit metadata, LSP code actions for removal, module-alias-aware false-negative fix |
| [architecture-cleanup.md](architecture-cleanup.md) | Architecture cleanup, consolidate ownership boundaries, harden invariants, unify pipelines |
| [diagnostic-reporting.md](diagnostic-reporting.md) | Rich multi-line diagnostic output with typed DiagKind variants, source snippets, underlines, and ANSI color |
| [js-ffi.md](js-ffi.md) | User-facing `extern` declarations for JS/host FFI via WASM imports (Phase 1 complete; playground and Phase 2 richer types deferred) |
| [anonymous-record-variant.md](anonymous-record-variant.md) | Anonymous record types as variant payloads in enum declarations, with synthesized scoped record types and cross-module export support |
| [unify-js-runtime.md](unify-js-runtime.md) | Shared Node Wasm runtime (`runtime.mjs`) between library tools and SEA CLI |
| [boot-jspi-runtime.md](boot-jspi-runtime.md) | JSPI entry ABI, async runtime path, suspending host/extern imports, browser async APIs (timer, fetch, fetch-backed VFS) |
| [boot-nested-variant-pattern-lowering.md](boot-nested-variant-pattern-lowering.md) | Fix nested variant pattern lowering — concrete type substitution for generic variant fields in both stage0 lowerer and emitter |
