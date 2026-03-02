# Future — Stages 9, 9.5, 10, Later

## Stage 9 — Host Integration & Validation

**Goal:** Implement the Wasmtime host shell that satisfies the runtime's host import interface,
run compiled programs end-to-end via Wasm, and validate correctness against the interpreter
via differential testing.

**Host shell design:**

The host is a thin Rust + Wasmtime layer. It is *not* the compiler — it merely provides
the host import functions (`host.print`, `host.println`, `host.error`) and instantiates the
linked Wasm module. The compiler pipeline remains in Rust at this stage, but the host interface
is deliberately minimal so any other host (Node, browser shim) can implement it identically.

WASI is a host concern: the Wasmtime host implements `host.read_file`, `host.write_file`,
`host.write_bytes`, `host.mkdirp`, `host.list_dir`, `host.exists` using WASI or native
calls. `twc.wasm` imports file I/O abstractly — it is not aware of WASI. This keeps
`twc.wasm` host-agnostic.

At Stage 9, the host shell only needs console imports (`host.print`, `host.println`,
`host.error`) since the compiler pipeline is still in Rust. File I/O imports become live
in Stage 10 when `twc.wasm` itself reads source files.

CLI:

```bash
twk run file.tw                  # interpreter backend (unchanged)
twk run --backend=wasm file.tw   # compile → link → run via Wasmtime
twk build file.tw -o output.wasm # compile + link only
```

**Differential testing (`tests/wasm_test.rs`):**

For every program in `tests/run/`:

1. Run via interpreter → capture stdout.
2. Compile + link + run via Wasmtime → capture stdout.
3. Assert outputs are identical.

Any divergence is a regression in the WAT emitter or runtime. The interpreter remains the
reference semantic oracle.

**Wasm 3.0 — Tail Calls:** The WAT emitter should emit `return_call $f` / `return_call_ref
$ClosureFunc` for calls in tail position. Tail calls matter for:

* The recursive-descent parser in the self-hosted compiler (Stage 10).
* Mutually-recursive functions that otherwise hit Wasm's call stack limit on large inputs.

The `Instr::ReturnCall` and `Instr::ReturnCallRef` variants in `src/wasm/ir.rs` are available
for the emitter to use. Identify tail-position calls in ANF IR (a `Return(ACall(...))` pattern)
and emit the tail-call form. This is a safety gate for Stage 10 correctness.

Deliverables:

* All `tests/run/*.tw` programs produce correct output via `--backend=wasm`.
* Differential test suite passing.
* Host interface documented: the exact set of imports `twc.wasm` requires from the host,
  their types, and their observable behavior. This is the stability boundary for future hosts.
* Tail-position calls emitted as `return_call` / `return_call_ref`; verified on a
  deeply-recursive test program (e.g. Fibonacci with large N).

---

## Stage 9.5 — Monomorphization

**Goal:** Eliminate all type-variable boxing by specializing generic functions at each unique
instantiation. After this pass, no `MonoType::Var` survives into ANF or codegen — every
function has fully concrete typed params and locals.

**Why not type erasure permanently:** Type erasure (`Var → anyref`) requires boxing/unboxing
at every generic call boundary. For `fn id<T>(x: T) T` called as `id(42)`, the caller boxes
`i64` → `struct.new $BoxedInt` → `anyref`, passes it, the generic body treats `x` as `anyref`,
and the caller unboxes the result. This is 2 heap allocations and 2 casts per call. With
monomorphization, `id` is specialized to `id__Int(x: i64) -> i64` — zero overhead.

**Approach — Core IR → Core IR transform:**

The monomorphization pass runs after type checking and before Core IR → ANF lowering.
It is a whole-program transform:

1. **Collect instantiations.** Walk all `CoreExprKind::Call` nodes. For each call to a generic
   function, look up the solved type args from `TypeMap.generic_instantiations` (recorded during
   type checking per the 8c prep step). Build a map:
   `HashMap<FuncId, BTreeSet<Vec<MonoType>>>` — each generic FuncId to its set of unique
   concrete type-arg tuples.

2. **Specialize.** For each `(FuncId, type_args)` pair, clone the generic `FunctionDef`,
   substitute every `Var("T")` → concrete `MonoType` in params, return type, and body.
   Assign a fresh `FuncId` to each specialization. Name it `original_name__TypeA_TypeB`
   (e.g. `id__Int`, `map__Int_String`).

3. **Rewrite call sites.** Replace each generic `Call(func_id, args)` with
   `Call(specialized_func_id, args)` based on the call's type args.

4. **Remove generic originals.** The original generic `FunctionDef` (with `Var` types) is
   dropped — no function with `Var` types reaches ANF.

**Scope and edge cases:**

* **Rank-1 guarantee:** Damas-Milner ensures every instantiation is fully concrete and known
  at compile time. There are no higher-rank or existential types that would require runtime
  dispatch. The set of specializations is always finite.

* **Recursive generics:** `fn f<T>(x: T) { f(x) }` — the recursive call uses the same type
  args as the outer call, so it produces no new instantiations. The pass terminates because
  rank-1 prevents type args from growing (no `f(wrap(x))` where `wrap` adds a layer).

* **Transitive specialization:** If `f<T>` calls `g<T>` internally, specializing `f` to
  `f__Int` reveals a call to `g<Int>`. The pass must iterate (or process in dependency order)
  until no new instantiations are discovered. In practice this converges in 2-3 rounds for
  typical code.

* **Generic functions used as first-class values:** `let f = id` where the binding has a
  concrete type annotation (e.g. `f: fn(Int) Int = id`) — the monomorphizer generates
  `id__Int` and the closure wraps that specialization. If a generic function is stored without
  a concrete type context (e.g. `let f = id` with no annotation), the type checker already
  rejects this as `AmbiguousType`.

* **Cross-module generics:** A generic function exported from module A and called from module B
  with concrete types — the monomorphization pass runs on the linked Core IR (after all modules
  are lowered but before ANF), so cross-module instantiations are visible.

**Integration with the emitter:**

After monomorphization, the emitter never sees `MonoType::Var`. The `mono_to_valtype` mapping
for `Var` becomes `unreachable!()`. All functions have concrete Wasm signatures. The closure
trampoline generator uses concrete types. The `anyref` row in the value representation table
is dead code.

**Pipeline position:**

```text
parse → resolve → typecheck → lower (Core IR) → **monomorphize** → lower (ANF) → optimize → emit
```

**Deliverables:**

* `src/ir/monomorphize.rs` — the pass.
* All `tests/run/*.tw` programs produce identical output before and after monomorphization
  (differential test against interpreter).
* Wasm output for generic-heavy test programs (e.g. `generic_types.tw`, `iterator.tw`)
  shows specialized function names and no `anyref` locals in specialized bodies.
* Code-size report: compare total WAT line count with and without monomorphization on the
  test suite. Document the bloat ratio.

---

## Stage 10 — Self-Hosted Compiler

**Goal:** Re-implement the compiler pipeline in Twinkle, use the stage0 Rust compiler to
compile it to `twc.wasm`, then run and verify the Twinkle-hosted compiler.

**Bootstrapping sequence:**

1. Write the compiler in Twinkle under `compiler/` (lexer, parser, type checker, Core IR
   lowering, ANF lowering, optimizer, WAT emitter, Runtime IR + linker).
2. Stage0 Rust: `twk build compiler/main.tw -o twc.wasm`.
3. Verify: run `twc.wasm` under Wasmtime on `hello.tw`; output must match stage0 output.
4. Self-hosting round: compile `compiler/main.tw` with `twc.wasm` → new `twc.wasm`; verify
   the two are behaviorally equivalent on the compatibility suite.

**Prerequisites:** The Twinkle language must be expressive enough to write a compiler.
File I/O (reading source files) is provided by the host via WASI or a custom import — the
compiler sources import it as an abstract interface. String manipulation, arrays, and dicts
(already in the runtime) are sufficient for symbol tables and AST representations.

**Porting note:** The Runtime IR + Linker (`src/wasm/`) is implemented in Rust for stage0 but
must be ported to Twinkle for self-hosting. It is the largest self-hosting prerequisite beyond
the compiler pipeline itself.

**Compatibility suite:**

A set of `.tw` programs compiled by both stage0 (Rust) and stage1 (Twinkle self-hosted);
outputs (Wasm execution results) must be identical.

Deliverables:

* `twc.wasm` produced by stage0 can compile real Twinkle programs.
* `twc.wasm` produced by itself compiles the same programs to equivalent results.
* Stage0 Rust implementation frozen as a reference and bootstrap tool.

---

## Later Stages — Tooling & Ecosystem

> **Full design:** See [docs/tooling.md](../tooling.md).

**Prerequisites before tooling:**

* **Lossless lexer**: comments preserved as trivia tokens (required by formatter and LSP).
* **Parser error recovery**: partial AST on syntax errors (required by LSP; nice-to-have for formatter).

**Practical goals for "easy tooling":**

* **Whole-program formatting is easy to run**
  * `twk fmt --all` discovers project files from root and formats all `.tw` files deterministically;
  * formatting is idempotent (`fmt` twice yields no diff);
  * on syntax error, command reports file + span and continues other files (non-zero exit at end).

* **Incremental LSP is easy to keep fast**
  * on file change, re-run parse/resolve/typecheck only for the changed file and affected reverse-dependents;
  * unchanged modules are served from stage caches;
  * diagnostics/hover/go-to-definition read query artifacts directly (no full lower/link requirement).

**Milestones to reach those goals:**

* **T1 — Formatter core**
  * finish lossless lexer trivia model and formatter AST printer;
  * add `twk fmt <file>` with golden tests + idempotence tests.

* **T2 — Whole-project formatter UX**
  * add `twk fmt --all` file discovery, include/exclude rules, and stable output ordering;
  * add CI-friendly exit codes and summary reporting.

* **T3 — Incremental diagnostics for LSP**
  * complete Stage 6b query-cache work;
  * add dependency graph invalidation + reverse-dependency tracking;
  * expose diagnostics query endpoint reused by CLI and LSP host.

* **T4 — Interactive LSP features**
  * add hover / go-to-definition / completion on top of cached query artifacts;
  * add latency benchmarks with warm cache and edit-loop workloads.

**Tooling implementation map (files):**

* **Formatter core (`T1`)**
  * `src/syntax/lexer.rs`, `src/syntax/tokens.rs`:
    * add trivia/comment preservation model required by formatter.
  * `src/syntax/parser.rs`:
    * ensure parser exposes token/trivia links needed by formatting decisions.
  * `src/syntax/pretty.rs`:
    * implement canonical formatter printer.
  * `src/cli/mod.rs`, new `src/cli/fmt.rs`, `src/main.rs`:
    * wire `twk fmt <file>` and `--check`.
  * Tests: add formatter golden/idempotence suite under `tests/` (new formatter tests file + fixtures).

* **Whole-project formatting UX (`T2`)**
  * New module recommended: `src/cli/project_files.rs` (shared project file discovery).
  * `src/cli/fmt.rs`:
    * implement `--all`, deterministic file ordering, partial-failure reporting, CI exit codes.

* **Incremental diagnostics (`T3`)**
  * Build on query cache modules from Stage 6b Step C.
  * New module recommended: `src/diagnostics/mod.rs` for normalized diagnostic structures.
  * `src/cli/check.rs`:
    * consume diagnostics query API (no full lower/link unless requested).

* **LSP (`T4`)**
  * New module recommended: `src/lsp/mod.rs` (or standalone crate later).
  * Integrate with query API from Stage 6b Step D; keep lower/link out of hot path.
  * Add edit-loop latency benchmark harness (new `benches/lsp_latency.rs`).

**Planned tools** (all as `twk` subcommands initially, rewritten in Twinkle post-self-hosting):

* **`twk fmt`**: canonical formatter. Only needs parse stage + lossless lexer. No config; one official style.
* **`twk lint`**: linter with syntactic rules (parse only) and semantic rules (parse + typecheck). Key rule: warn on rebinding-without-use (the "looks like mutation but isn't" trap).
* **`twk lsp`**: language server (LSP protocol). Needs query-friendly pipeline + lossless lexer + error recovery. Initial features: diagnostics, hover types, go-to-definition, completion.
* **Standard library** in Twinkle (collections, JSON, I/O via WASI).
* **Package manager**, **test runner**, **doc generator**, **build system**.

These tools are separate concerns and plug into the pipeline via the existing compiler API (parse, typecheck, IR, codegen).
