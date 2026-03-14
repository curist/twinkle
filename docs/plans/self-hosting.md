# Stage 10 — Self-Hosted Compiler

**Goal:** Re-implement the compiler pipeline in Twinkle as a clean redesign — not a
line-by-line port of the Rust stage0. The self-hosted compiler should have a pure,
composable pipeline with first-class support for LSP and tooling from the start.

## Design Principles

### 1. Pure Pipeline

Each stage is a pure function: immutable input → new output. No mutable `self`,
no accumulated side effects. Counters, error lists, and registries are threaded
functionally — either returned as part of the result or folded through a list.

```
parse(source: String, file_id: FileId) -> StageResult<Ast>
resolve(ast: Ast, deps: ModuleEnv) -> StageResult<ResolvedAst>
check(resolved: ResolvedAst, env: TypeEnv) -> StageResult<TypedAst>
lower(typed: TypedAst) -> CoreModule
mono(core: CoreModule) -> CoreModule
to_anf(core: CoreModule) -> AnfModule
optimize(anf: AnfModule) -> AnfModule
emit(anf: AnfModule) -> WatModule
```

Where state must grow across a stage (e.g., fresh type variable counter), the
stage takes it as input and returns the updated value:

```
fn infer(expr: Expr, ctx: InferCtx) -> (MonoType, InferCtx)
```

This eliminates the Rust pattern of `&mut self` on `TypeChecker`, `Lowerer`,
`EmitCtx`. The pure design also makes stages trivially testable — no setup/teardown
of mutable objects.

### 2. Partial Results at Every Stage (Tooling-First)

The current Rust pipeline uses `Result<T, Vec<Error>>` — on error, the stage
produces nothing. For LSP, every stage must produce a **best-effort result plus
diagnostics**, so downstream queries (hover, completion, go-to-definition) work
even in files with errors.

```tw
type StageResult<T> = .{ value: T, diagnostics: Vector<Diagnostic> }
```

- The parser produces a partial AST with `ErrorNode` placeholders for
  unrecoverable syntax.
- The resolver produces a partial environment; unresolved names get an
  `Unknown` binding that the checker can propagate.
- The type checker produces a TypeMap with `Unknown` types for unresolved
  expressions — hover still works for the resolved portions.

Frontend stages never short-circuit the pipeline. A file with 3 parse errors
and 2 type errors produces all 5 diagnostics in one pass, plus a partial typed
AST that the LSP can query. Backend stages (lower through emit) only run when
the frontend reports zero errors — they are not designed for partial inputs.

### 3. Unified Diagnostic Type

Instead of separate error enums per stage (`ParseError`, `TypeError`,
`LowerError`), use a single `Diagnostic` throughout:

```tw
type Severity = { Error, Warning, Hint, Info }

type RelatedInfo = .{ span: Span, message: String }

type Diagnostic = .{
  span: Span,
  severity: Severity,
  message: String,
  related: Vector<RelatedInfo>,
}
```

This avoids error type conversion between stages and makes it trivial to collect
all diagnostics for a file. The CLI and LSP both consume `Vector<Diagnostic>`
directly.

### 4. Frontend / Backend Split

The pipeline has a hard architectural boundary between **frontend** (runs on
every keystroke in LSP) and **backend** (runs only on explicit build):

```
Frontend:  parse → resolve → typecheck
Backend:   lower → mono → anf → optimize → emit → link
```

The frontend produces `TypedAst` — sufficient for all LSP features (diagnostics,
hover, go-to-definition, completion, find-references). The backend is never
invoked during interactive editing.

### 5. Position-Indexed Artifacts

LSP queries are "what's at this byte offset?" The typed AST must support
efficient position lookups:

- **Span → ExprId**: sorted span index for binary search to find the innermost
  AST node at a given position.
- **ExprId → MonoType**: the type map (same as current).
- **ExprId → Definition Span**: for go-to-definition.
- **Name → Vector<Span>**: for find-references / rename.
- **Position → Scope**: for completion — what names are in scope at cursor.

These indexes are built as part of the frontend output, not reconstructed on
each query.

### 6. Independent Module State

The current Rust compiler accumulates a global `CompileState` across all modules,
with snapshot/restore for scoping. The self-hosted compiler should instead cache
each module's frontend result independently:

```tw
type ModuleState = .{
  path: String,
  deps: Vector<String>,
  frontend: StageResult<TypedAst>,
}

type ProjectState = .{
  modules: Dict<String, ModuleState>,
  graph: DependencyGraph,
}
```

On file change:
1. Re-run the frontend for that module.
2. Re-run the frontend for downstream dependents (via reverse dependency graph).
3. Unchanged modules keep their cached `ModuleState`.

This makes incremental recompilation a natural consequence of the data model,
not a bolted-on optimization.

## Redesign Notes (vs. Rust Stage0)

### Mutable State → Pure Threading

| Rust stage0 | Self-hosted |
|---|---|
| `TypeChecker { meta_subst, errors, next_meta, ... }` with `&mut self` | `InferCtx` record passed in, updated copy returned |
| `Lowerer { local_alloc, hoisted, func_table, ... }` with `&mut self` | `LowerCtx` threaded through, `hoisted` accumulated via fold |
| `EmitCtx { repr_flow, specialized_types, ... }` with `&mut self` | `EmitCtx` record threaded, registries grown functionally |
| `errors: Vec<E>` with `push` side effect | `StageResult<T>` with diagnostics in output |

### Trait Object → Capability Record

The one trait object in stage0 (`ModuleSourceAdapter`) becomes:

```tw
type SourceAdapter = .{
  read_source: fn(String) Result<String, String>,
  file_exists: fn(String) Bool,
  list_dir: fn(String) Vector<String>,
}
```

### Tuple-Keyed Maps → Two-Level Dicts

`HashMap<(TypeId, String), usize>` (for record fields, sum variants, methods)
becomes `Dict<Int, Dict<String, Int>>` — outer key is TypeId, inner key is
field/variant/method name. Cleaner than composite string keys and avoids
encoding/decoding overhead.

### HashSet → Dict<K, Bool>

Used throughout for reachability, liveness, seen-sets. Straightforward but
more verbose; a thin wrapper module could help:

```tw
fn has(s: Dict<Int, Bool>, k: Int) Bool { s.get(k).unwrap_or(false) }
fn add(s: Dict<Int, Bool>, k: Int) Dict<Int, Bool> { s.set(k, true) }
```

### Box<T> → Nothing

Rust needs `Box<CoreExpr>` for recursive types. In Twinkle, all record/enum
payloads are GC-managed — recursive data structures just work.

## Lessons from Stage0 Codegen Pain Points

The hardest bugs in the Rust stage0 came from three interrelated codegen systems:
typed closure specialization, type erasure reduction, and sum representation
boundary unification. These are documented in the archived plans. The root
causes share a common pattern that the self-hosted compiler should avoid.

### The Core Problem: Representation Was an Afterthought

In stage0, the backend started with a single universal representation: all
closures use `$rt_types__ClosureFunc (anyref, anyref) → anyref`, all sum values
use `$rt_types__Variant` with a payload array, all record fields are `anyref`.
Specialization was layered on incrementally — typed closures, typed Option,
typed Result, typed iterators, typed cells — each as a separate optimization
pass bolted onto the universal path.

This created a **two-world problem**: every value can exist in either typed or
erased form, and every code path that touches a value must know which form it's
in. The result is `SumRepr` metadata, `LocalBackendInfo`, `ReprFlowCtx`,
scoped save/restore, boundary conversion helpers, and debug assertions — all
to track what the compiler already knows at the type level but loses when
mapping to Wasm.

### What Went Wrong Specifically

1. **Representation policy was scattered.** Whether a value is typed or erased
   was decided independently in literal emission, local loading, match lowering,
   assignment handling, and ABI coercions. When these disagreed about the same
   value, the Wasm module trapped at runtime with `ref.cast` failures.

2. **Side metadata drifted from types.** `SumRepr`, `local_typed_option`,
   iterator state info, and closure repr metadata are all shadow type systems
   tracking physical layout alongside the semantic `MonoType`. They must stay
   in sync, but there's no structural guarantee they do — only runtime debug
   assertions.

3. **Multiple parallel flow metadata channels.** Every `Let` binding in
   `emit_let_expr` must push/restore metadata across four independent channels:
   - `push_flow_mono_binding` (MonoType)
   - `push_flow_value_repr_binding` (ValueRepr — closure info)
   - `push_flow_iterator_binding` (IteratorStateInfo)
   - `push_flow_typed_option_binding` (SumRepr — wraps `push_flow_sum_repr_binding`)

   A fifth channel (`push_flow_iterator_next_binding`) exists in `ctx.rs` for
   branch analysis. The system also has `push_flow_sum_repr_binding` as the
   underlying mechanism behind `push_flow_typed_option_binding` — these are
   the same channel with a compatibility alias, not truly independent.

   Missing any channel at a branch point causes miscompilation. The channels
   grew incrementally — each specialization feature added its own metadata
   tracking, and they must all be kept in lockstep.

4. **Boundary conversions were ad hoc.** Converting typed → erased (or vice
   versa) was initially done inline at each call site. This led to duplicated
   conversion snippets that diverged over time. The `emit_sum_local_to_erased`
   centralization helped, but it was a fix applied after the damage. The
   function itself has two paths: one for locals with `SumRepr` metadata, and
   a fallback for `anyref` locals where it re-infers the type from mono —
   which means the "single source of truth" still has a backup heuristic.

5. **Runtime dispatch as a deliberate safety net.**
   `emit_anyref_option_or_variant_local_to_variant` emits a `ref.test` /
   `ref.cast` branch at runtime to handle locals that might hold either a
   typed option struct or an erased Variant. This exists because `anyref`
   locals can carry mixed representations across branch joins — the compiler
   doesn't know which form the value is in, so it checks at runtime. The
   sum-representation-boundary-unification work deliberately preserved this
   as an accepted residual (Phase 5 cleanup found "no obsolete guards"). It
   is a runtime cost accepted in stage0 to avoid a deeper redesign — exactly
   the kind of thing the self-hosted compiler should eliminate structurally.

6. **Seven categories of on-demand type registration.** During emission, the
   emitter calls `request_typed_*` to register struct types it discovers it
   needs:
   - `request_typed_closure` (typed funcref + env struct)
   - `request_typed_cell` (typed Cell container)
   - `request_typed_iterator_state` (seed + step struct)
   - `request_typed_iter_item` (value + rest struct)
   - `request_typed_iter_option` (typed Option wrapping IterItem)
   - `request_typed_unfold_step` (Yield/Done payload struct)
   - `request_typed_general_option` (typed Option/Result struct)

   These are accumulated into `SpecializedTypeRegistry` during emission,
   then bulk-emitted after all function bodies. The emitter's output
   depends on emission order, and duplicate requests must be deduplicated.

7. **Specialization scope crept.** Typed closures started as "just for
   higher-order parameter calls." Then named function values needed it. Then
   closures in cells, records, iterators. Each extension touched more of the
   emitter and added more metadata tracking. The universal fallback path had
   to be preserved alongside every typed path, doubling the code surface.

### Design Rules for the Self-Hosted Backend

These lessons lead to concrete design rules:

#### Rule 1: Representation Is Decided Once, Early

After monomorphization, every value has a concrete type. The backend should
compute a **Wasm layout** for each concrete type exactly once, before emission
begins. This layout is a pure function of the monomorphized type — not
discovered ad hoc during emission.

```tw
type WasmLayout = {
  Scalar(WasmValType),           // i32, i64, f64
  Record(WasmStructDef),         // named struct with typed fields
  Sum(WasmSumDef),               // variant_id + typed payloads
  Closure(WasmClosureDef),       // typed funcref + env
  Iterator(WasmIteratorDef),     // typed state struct
  Array(WasmLayout),             // element layout
}

fn layout_of(ty: MonoType, type_env: TypeEnv) -> WasmLayout
```

There is no "erased fallback" at the layout level. If a type is concrete
(which it always is after monomorphization), it gets a concrete layout.
`anyref` still exists at the Wasm runtime ABI boundary — calls into runtime
helpers (`rt.arr`, `rt.str`, etc.) require boxing/unboxing — but this is
handled by explicit boundary nodes (Rule 3), not by falling back to an
erased layout for the value itself.

#### Rule 2: No Shadow Type System

The Wasm layout is derived from `MonoType` by a pure function. There is no
separate `SumRepr`, no `LocalBackendInfo.sum_repr`, no `ReprFlowCtx`. The
emitter looks up the layout from the type, not from side metadata. If you
know the type, you know the layout. Period.

This eliminates the entire category of bugs where metadata drifts from the
actual type.

#### Rule 3: Boundary Conversions Are Explicit in the IR

When a value must cross a representation boundary (e.g., passed to a runtime
helper that expects `anyref`), the conversion should be an explicit IR node,
not an implicit coercion during emission.

```tw
// In ANF, after boundary insertion:
let boxed = WrapAnyref(typed_value)     // typed → anyref
let typed = UnwrapAnyref(anyref_value, target_type)  // anyref → typed
```

This makes boundary crossings visible, auditable, and optimizable. The emitter
never guesses — it just emits what the IR says.

**Pipeline ordering note:** inserting these nodes requires knowing which values
need wrapping, which depends on Wasm layout decisions. Therefore boundary
insertion runs as a dedicated pass *after* `plan_wasm_types` computes the
layout registry, not during `to_anf`. The pipeline becomes:

```
to_anf → optimize → plan_wasm_types → insert_boundaries → emit
```

#### Rule 4: Layout Registry, Not On-Demand Generation

Stage0 generates typed structs on-demand during emission via
`request_typed_closure_struct`, `request_typed_option_struct`, etc. This
means the set of types in the output depends on emission order, and the
emitter must track what it's already requested.

Instead, compute the full set of needed Wasm type definitions in a pre-pass
over the ANF module. The emitter receives a complete `WasmTypeRegistry` and
just references entries — it never creates new types during emission.

```tw
fn plan_wasm_types(anf: AnfModule, type_env: TypeEnv) -> WasmTypeRegistry
fn emit(anf: AnfModule, types: WasmTypeRegistry) -> WasmModule
```

#### Rule 5: No Flow-Sensitive Backend Metadata

Stage0's `push_flow_sum_repr_binding` / `restore_flow_sum_repr_binding` exists
because the emitter tracks per-local physical layout that changes at branch
points. This is inherently fragile.

With Rule 1 (layout derived from type) and Rule 3 (explicit boundary nodes),
there is no flow-sensitive metadata to track. A local's layout is determined
by its type, which is fixed at its binding site. Branch arms that produce
different representations go through explicit IR conversion nodes.

#### Rule 6: One Metadata Channel, Not Six

Stage0's multiple parallel push/restore channels exist because each specialization
feature tracks its own metadata independently. In the self-hosted compiler,
a local's backend info is fully determined by its type + the layout registry.
There is no per-local metadata to push/restore — the emitter is stateless
with respect to local representations.

If branch-sensitive information is ever needed (e.g., "this local is known
non-None in this branch"), it should be a single unified `BranchFacts` record
that is saved/restored once per branch, not N independent channels.

### Summary: What Changes

| Stage0 (Rust) problem | Self-hosted solution |
|---|---|
| Layout discovered ad hoc during emission | `layout_of(type)` computed once before emission |
| `SumRepr` / `LocalBackendInfo` shadow type system | No shadow metadata — layout derived from `MonoType` |
| Four+ parallel flow metadata channels | Zero or one unified `BranchFacts` channel |
| Inline boundary conversions that diverge | Explicit `WrapAnyref` / `UnwrapAnyref` IR nodes |
| Seven categories of `request_typed_*` | Pre-computed `WasmTypeRegistry` from type scan |
| `emit_anyref_option_or_variant` runtime dispatch | No mixed representations — layout is always known |
| Universal + typed paths coexist everywhere | Concrete layouts are the only path; runtime helpers get explicit wrapping |

## Bootstrapping Sequence

1. Write the compiler in Twinkle under `boot/` (lexer, parser, resolver, type
   checker, Core IR lowering, ANF lowering, optimizer, WAT emitter, linker).
   - Entry point: `boot/main.tw`
   - Shared libraries: `boot/lib/` (source, module, graph, query, argparse)
2. Stage0 Rust: `twk build boot/main.tw -o twc.wasm`.
3. Verify: run `twc.wasm` to compile `hello.tw`, then execute the resulting
   Wasm; execution output must match stage0-compiled execution output. (WAT
   text is not expected to be identical — symbol names and emission order will
   differ.)
4. Self-hosting round: compile `boot/main.tw` with `twc.wasm` → new `twc.wasm`;
   verify the two are behaviorally equivalent on the compatibility suite.

## Implementation Phases

### Phase A — Frontend (Lexer + Parser + Resolver + Type Checker)

Cleanest data structures, minimal state. The frontend is the most important
piece because it's what the LSP uses. Build it first with LSP in mind:

- Lossless lexer (trivia/comments preserved as tokens)
- Error-recovering parser (partial AST with error nodes)
- Resolver producing partial environments
- Bidirectional type checker with `InferCtx` threading
- Position index built during type checking
- All stages return `StageResult<T>` with partial results

### Phase B — Core IR Lowering + Monomorphization

Tree-rewriting passes with tractable state:

- AST → Core IR lowering with `LowerCtx` threaded purely
- Lambda hoisting accumulated via fold (not side-effecting push)
- Monomorphization BFS with dict-based memo table

### Phase C — ANF Lowering + Optimization

- Core IR → ANF via let-accumulator pattern (already clean)
- Optimization passes are pure tree rewrites — natural fit
- Fixed-point loop checking a `changed` flag

### Phase D — Codegen + Linker

Most complex stage. Build last so the other phases are stable:

- WAT emission is mostly string building from structured data
- Typed closure / sum repr specialization is the hardest part
- Linker is mechanical: namespace prefixing + symbol resolution

### Phase E — Integration + Self-Hosting

- Wire up multi-module compilation with `SourceAdapter` capability
- Implement `ProjectState` + dependency graph for incremental
- Build CLI entry point with argparse
- Run compatibility suite against stage0

## Repository Layout

```text
boot/
  main.tw                    # compiler entry point (CLI)
  compiler/
    lexer.tw                 # lossless lexer
    parser.tw                # error-recovering parser
    ast.tw                   # AST data types
    resolver.tw              # name resolution
    checker.tw               # bidirectional type checker
    types.tw                 # MonoType, TypeEnv, TypeMap
    core_ir.tw               # Core IR types
    lower_core.tw            # AST → Core IR
    monomorphize.tw          # monomorphization pass
    anf.tw                   # ANF IR types
    lower_anf.tw             # Core IR → ANF
    optimize.tw              # optimization passes
    emit.tw                  # ANF → Wasm IR
    wasm_ir.tw               # Wasm IR types
    linker.tw                # module linking
    wat.tw                   # Wasm IR → WAT text
  lib/
    source/                  # spans, file registry, diagnostics
    module/                  # project root, path resolution
    graph/                   # dependency graph, topo sort
    query/                   # stage cache, invalidation
    argparse/                # CLI argument parsing
```

## Compatibility Suite

A set of `.tw` programs compiled by both stage0 (Rust) and stage1 (Twinkle
self-hosted); outputs (Wasm execution results) must be identical.

## Deliverables

* `twc.wasm` produced by stage0 can compile real Twinkle programs.
* `twc.wasm` produced by itself compiles the same programs to equivalent results.
* Frontend stages return `StageResult<T>` with partial results — ready for LSP.
* Position-indexed TypedAst supports hover, go-to-definition, completion.
* Independent `ModuleState` caching enables incremental recompilation.
* Stage0 Rust implementation frozen as a reference and bootstrap tool.
