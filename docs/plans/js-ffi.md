# JS FFI Plan

## Goal

Provide a general-purpose mechanism for Twinkle programs to declare and call
external (host-provided) functions. This replaces the current hardcoded
`__host_*` builtins with a user-facing language feature that maps directly to
WASM import declarations.

The design proceeds in two compiler phases: extern declarations with grouped
blocks (Phase 1), and richer boundary types (Phase 2). Playground/tooling
integration is a separate deployment concern, not a compiler gate.

## Current State

All host interop is internal to the compiler:

- `base_env.tw` registers ~15 `__host_*` functions with fixed signatures.
- `lower_core.tw` maps `__host_xyz` lookups to `host_xyz` builtin IDs.
- Runtime modules (`runtime/core.tw`, `runtime/str.tw`, `codegen/intrinsics.tw`)
  hardcode `ImportDef.{ module: "host", name: ... }` structs.
- The linker special-cases `if imp.module == "host"` to preserve imports.
- Users cannot declare their own imports — the set is closed.

## Non-Goals

- Automatically generating JS glue code or TypeScript bindings.
- Changing how existing `__host_*` builtins work (they remain as-is for the
  stdlib; `extern` is additive).
- Providing a "safe" sandboxed FFI — the host is trusted.
- Supporting WASI or component-model interfaces (orthogonal future work).

## Design

### Phase 1: `extern` Declarations (Individual + Grouped)

New top-level syntax for individual declarations:

```twinkle
// Import a single function from module "console"
extern "console" fn log(msg: String)

// With return type
extern "crypto" fn random() Float

// Default module is "env" when omitted
extern fn my_helper(x: Int) Int
```

Grouped syntax (sugar for multiple declarations sharing an import module):

```twinkle
extern "canvas" {
  fn clear()
  fn draw_rect(x: Float, y: Float, w: Float, h: Float)
  fn set_color(r: Int, g: Int, b: Int)
  fn get_width() Int
}
```

Grouped blocks desugar to individual `extern "canvas" fn ...` declarations.
Can be `pub extern "canvas" { ... }` to export all.

**Grammar addition:**

```ebnf
extern_decl  = "extern" [ string_lit ] "fn" ident "(" params ")" [ type ] ;
extern_block = [ "pub" ] "extern" string_lit "{" { extern_fn_sig } "}" ;
extern_fn_sig = "fn" ident "(" params ")" [ type ] ;
```

**Parser note:** When `extern` is followed by a string literal and then `{`
(instead of `fn`), parse as block.

**Semantics:**

- No function body. The declaration is a type signature + import source.
- Visibility follows normal `pub` rules (can be `pub extern ...`).
- The string literal is the WASM import module name. The function identifier
  becomes the WASM import field name.
- Parameters and return types must be "extern-safe" types (see below).

**Extern-safe types (Phase 1):**

| Twinkle type | WASM type | Notes |
|---|---|---|
| `Int` | `i64` | |
| `Float` | `f64` | |
| `Bool` | `i32` | 0/1 — checker maps `Bool` → `ValType::I32` specifically for extern signatures. **ABI note:** the host is trusted to pass only 0/1; non-boolean `i32` values are interpreted without validation. |
| `String` | `(ref $string)` | GC string ref, host uses bridge to decode |
| `()` (void) | (no result) | |

Note: `Vector<Byte>` is intentionally excluded from Phase 1. The user-facing
`Vector<T>` is a PVec (persistent 32-way trie), not a flat GC array. Passing it
across the boundary as `(ref $array)` would be an ABI mismatch. Raw byte
arrays (the internal `$array` type used for strings) are not directly
expressible in user code. Phase 2 will address compound types with proper
bridge-assisted marshaling.

Compound types (`Option`, `Result`, user structs, `Vector`) are disallowed in
Phase 1. The compiler emits an error: "extern functions only support primitive
types at the boundary".

**Compiler changes:**

| Layer | File(s) | Change |
|---|---|---|
| Lexer | `lexer.tw` | Add `extern` keyword token |
| Parser | `parser.tw` | Parse `extern_decl` as a new top-level item |
| AST | `ast.tw` | Add `ExternFn` node with module, name, params, return type |
| Resolver | `resolver.tw` | Register extern fns in scope; record import metadata |
| Checker | `checker.tw` | Type-check call sites normally; reject non-extern-safe param/return types |
| Lower Core | `lower_core.tw` | Calls to extern fns lower to `Call(GlobalFunc(id), args)` with an extern marker |
| Module IR | `core_ir.tw` | Add `extern_imports` to `CompiledModule` |
| Core Linker | `core_linker.tw` | Propagate `extern_imports` during multi-module merging |
| Codegen | `codegen.tw` | Generate `FuncType` typedef + `ImportDef` from `ExternImport` metadata |
| Linker | `codegen/linker.tw` | Generalize BOTH `module == "host"` guards (see below) |

**`ExternImport` type:**

```twinkle
pub type ExternImport = .{
  module: String,    // WASM import module ("console", "env", etc.)
  name: String,      // WASM import field name
  params: Vector<ValType>,
  results: Vector<ValType>,
}
```

**Alternative: reuse `ExternalRef` (boot compiler).**

The boot compiler already has an `ExternalRef` type in `core_ir.tw` that tracks
phantom FuncIds for imported functions (`{ module_path, func_name }`), with
plumbing through `core_linker.tw`. Instead of adding a wholly new
`ExternImport` side table, consider extending `ExternalRef` with a
`wasm_module: Option<String>` field. When present, this marks the ref as a
user-declared extern import rather than a cross-module reference. This reuses
the existing phantom-FuncId infrastructure and avoids duplicating the pipeline
threading logic. The stage 0 (Rust) compiler does not have `ExternalRef`, so it
would need a new structure regardless.

**Linker: two guards must be generalized.**

The current linker has two independent `module == "host"` checks:

1. **Resolution-skip guard** (`linker.tw:259`, `linker.rs:225+246`): Skips
   redirect resolution for host imports so they aren't treated as inter-module
   references. The Rust linker has this in two adjacent loops (qualified and
   unqualified redirect passes).
2. **Import-emission guard** (`linker.tw:318`, `linker.rs:330`): Only emits
   imports where `module == "host"` into `merged_imports`; all others are dropped.

Both must be generalized to an `is_external_import(imp)` predicate. The
predicate returns true when the import's `(module, name)` pair was NOT resolved
to any compiled Twinkle module's exports — i.e., it inverts the existing
resolution logic rather than maintaining a list of "known external" module
names. If only guard (1) is changed, user extern imports compile but are
silently dropped from the final WASM binary — producing a confusing
instantiation-time error.

**Pipeline threading: extern metadata through mono/ANF.**

The compilation pipeline is: `lower_core → CoreModule → monomorphize →
lower_anf → AnfModule → prepare_backend → PreparedModule → emit_module`.

Extern functions have no body, so they don't participate in monomorphization or
ANF lowering. The `ExternImport` metadata is carried as a side-table:

- `CompiledModule.extern_imports` stores the declarations.
- `core_linker.tw` merges extern_imports from all modules during linking.
- After linking, the merged extern_imports map is passed directly to
  `emit_module` as a parameter (or attached to `PreparedModule`).
- Codegen reads it to produce `FuncType` typedefs and `ImportDef` entries.
  Each extern import requires a corresponding `(type ...)` declaration in the
  WASM output (existing `__host_*` imports get theirs from
  `codegen/intrinsics.tw`; user externs need the emitter to synthesize one from
  the `ExternImport.params`/`.results` vectors).

Extern FuncIds are never lowered to function bodies — calls to them emit
`call $extern_name` referencing the import index. The monomorphizer must
explicitly skip extern FuncIds: when it encounters a `Call(GlobalFunc(id), ...)`
where `id` is in the `extern_imports` map, it leaves the call as-is rather than
attempting to look up and clone a body. Without this guard, the monomorphizer
will panic or silently miscompile when it cannot find a body for the extern
FuncId.

**Visibility semantics.**

`pub` on an extern fn controls Twinkle-level visibility (whether other modules
can call it), NOT whether the WASM import is emitted. Any extern fn — `pub` or
not — always produces a WASM import declaration. A non-`pub` extern fn is
callable only within its declaring module but still requires the host to provide
the import at instantiation time.

### Playground & Tooling Integration (non-blocking)

Once extern declarations compile, the playground needs a way to provide
implementations. This is a deployment/tooling concern and does not block
compiler work.

**Option 2a — Curated web modules:**

Ship a set of pre-declared modules with fixed worker implementations:

```twinkle
extern "console" fn log(msg: String)
extern "console" fn warn(msg: String)
extern "console" fn error(msg: String)
extern "performance" fn now() Float
```

The worker provides these in its `hostImports` object alongside the existing
`host` module. This requires no user-authored JS.

**Option 2b — User JS preamble:**

Allow a JS snippet (e.g., in a second editor pane or a `// @ffi` header) that
the playground evaluates to produce import objects:

```javascript
// Playground binds this to the instantiation imports
export default (bridge) => ({
  canvas: {
    clear: () => ctx.clearRect(0, 0, w, h),
    draw_rect: (x, y, w, h) => ctx.fillRect(x, y, w, h),
  }
})
```

The worker merges user-provided imports with the standard `host` imports before
instantiation.

**Option 2c — Signature-only validation:**

The compiler validates that extern calls type-check but doesn't verify the host
provides them. Missing imports produce a WASM instantiation error at runtime
(standard WASM behavior). This is the simplest approach and may be sufficient.

**Recommendation:** Start with 2c (no special playground support — just let
instantiation fail with a clear error if imports are missing). Add 2a for
common web APIs as a convenience layer.

### Phase 2: Richer Boundary Types

Once the basic mechanism is proven, extend extern-safe types:

| Type | Encoding | Notes |
|---|---|---|
| `Option<T>` | Variant GC ref | Host uses `bridge.variant_new` |
| `Result<T, E>` | Variant GC ref | Same as Option |
| `Vector<T>` | PVec GC ref | Host uses bridge to iterate (NOT a flat array) |
| `Vector<Byte>` | PVec GC ref | Or: auto-marshal to/from flat `$array` at boundary |
| Struct types | GC struct ref | Need bridge accessors per struct |

Note: `Vector<T>` is a persistent trie (PVec), not a flat WASM GC array. The
host must use bridge helpers (`array_len`, `array_get` on the trie's internal
nodes) or the compiler must emit marshaling code to copy into a flat array at
the boundary. The auto-marshal approach is cleaner for the host but has O(n)
cost.

This requires the compiler to emit bridge accessor exports for any struct type
that appears at an extern boundary, or to document that the host must use the
generic bridge helpers.

## Migration Path for Existing `__host_*` Builtins

The internal `__host_*` mechanism remains unchanged for stdlib modules (`fs.tw`,
`proc.tw`, etc.) since these are part of the compiler's own runtime. However,
once `extern` is stable, stdlib could optionally be rewritten as:

```twinkle
// stdlib/fs.tw — future form
extern "host" fn read_file(path: String) Result<Vector<Byte>, String>
extern "host" fn write_file(path: String, text: String)
// ...
```

This is a cleanup, not a blocker for the FFI feature.

## Open Questions

1. **Import deduplication**: If a user declares `extern "host" fn print(...)`,
   the runtime already emits an `ImportDef` for `(host, print)`. Emitting a
   duplicate WASM import for the same `(module, name)` pair is a WASM validation
   error. **Resolution:** The WAT linker (`src/wasm/linker.rs` /
   `boot/compiler/codegen/linker.tw`) must deduplicate imports by
   `(module, name)` key when building `merged_imports`. If a user extern matches
   an existing runtime import, the linker uses the existing one and patches the
   user's call to reference it. If signatures conflict, emit a compile error:
   "extern declaration conflicts with runtime import". Dedup must happen at link
   time (not in the checker or lower_core) to catch duplicates arising from
   different compiled modules. This also means `extern "host" fn ...` effectively
   shadows/overrides the builtin from the user's perspective but reuses the same
   WASM import slot. The same rule applies to user-to-user collisions: if two
   modules both declare `extern "canvas" fn clear()` with matching signatures,
   the linker emits one WASM import and both call sites reference it. If the
   signatures differ (e.g., one returns `()` and the other takes a parameter),
   the linker emits a compile error: "conflicting extern signatures for
   (canvas, clear)".

2. **Multi-value returns**: WASM supports multi-value. Should extern fns allow
   returning tuples? Maps naturally to `(result i64 f64)` etc. Defer to Phase 2.

3. **Callback / funcref passing**: Passing Twinkle closures to JS requires
   exporting the closure's funcref. Defer to a later phase.

4. **String encoding**: Current strings are UTF-8 byte arrays in GC memory.
   JS TextDecoder handles this via bridge. Document this as the ABI contract.

5. **Error handling**: If an extern fn traps (JS throws), it becomes a WASM
   trap. Should we wrap in `Result` automatically? Probably not — keep it
   explicit.

## Affected Files

### Language specification & grammar

| File | Change |
|------|--------|
| `docs/spec.md` | Document `extern` declaration syntax and semantics |
| `docs/grammar.ebnf` | Add `extern_decl` and `extern_block` productions |

### Tree-sitter grammar (syntax highlighting & editors)

| File | Change |
|------|--------|
| `tree-sitter-twinkle/grammar.js` | Add `extern_declaration` and `extern_block` rules (Phase 1) |
| `tree-sitter-twinkle/queries/highlights.scm` | Highlight `extern` as keyword, module string as `@string.special` |
| `tree-sitter-twinkle/test/corpus/` | Add test cases for extern syntax |

### Stage 0 (Rust compiler)

| File | Change |
|------|--------|
| `src/syntax/tokens.rs` | Add `Extern` keyword variant |
| `src/syntax/ast.rs` | Add `ExternFn` to `Item` enum |
| `src/syntax/parser.rs` | Parse extern declarations |
| `src/types/env.rs` | Register extern fns in type environment |
| `src/ir/lower.rs` | Handle extern FuncIds (no body); produce `GlobalFunc(id)` refs |
| `src/module/mod.rs` | Carry extern_imports through module assembly |
| `src/codegen/ctx.rs` | Track extern imports for emission |
| `src/codegen/emit.rs` | Synthesize `FuncType` typedef + emit `ImportDef` from extern metadata |
| `src/wasm/ir.rs` | (Already has `ImportDef` — may need no change) |
| `src/wasm/linker.rs` | Generalize BOTH `module == "host"` guards (lines 225+246 and 330) |

### Boot compiler (self-hosted)

| File | Change |
|------|--------|
| `boot/compiler/lexer.tw` | Add `extern` keyword token |
| `boot/compiler/ast.tw` | Add `ExternFn` AST node |
| `boot/compiler/parser.tw` | Parse extern declarations |
| `boot/compiler/resolver.tw` | Register extern fns in scope |
| `boot/compiler/checker.tw` | Validate extern-safe types |
| `boot/compiler/lower_core.tw` | Lower extern calls; record extern metadata |
| `boot/compiler/core_ir.tw` | Add `extern_imports` to `CompiledModule` |
| `boot/compiler/core_linker.tw` | Propagate `extern_imports` during multi-module merging |
| `boot/compiler/module_compiler.tw` | Thread extern metadata through pipeline |
| `boot/compiler/codegen/codegen.tw` | Generate `ImportDef` from extern metadata |
| `boot/compiler/codegen/linker.tw` | Generalize BOTH `module == "host"` guards (lines 259 and 318) |

### Playground

| File | Change |
|------|--------|
| `playground/public/worker.js` | Merge user-declared import modules into instantiation |

## Stage 0 vs Boot: Implementation Strategy

Both compilers need the feature because:

1. **Stage 0 compiles the boot compiler.** If boot source starts using `extern`
   declarations (e.g., to replace `__host_*` builtins), stage 0 must parse them.
2. **Stage 0 is used for development iteration** — `cargo test` runs stage 0
   tests, which is the fast feedback loop for language changes.
3. **Boot compiler is the production compiler** — it produces final WASM and is
   what the playground uses.

**Recommended order:**

1. **Stage 0 first (parser + resolver + codegen).** This gives fast `cargo test`
   iteration and validates the design before touching the self-hosted compiler.
2. **Tree-sitter grammar in parallel** — independent of either compiler.
3. **Boot compiler second.** Port the same logic. Since boot is compiled by
   stage 0, adding `extern` keyword parsing to stage 0 first avoids bootstrap
   issues.
4. **Spec & grammar docs last** — finalize after implementation validates the
   design.

If the scope feels large, a minimal viable approach is:
- Stage 0: parse + ignore (treat extern fns as opaque signatures, emit imports)
- Boot: full implementation with type validation
- This unblocks boot from using `extern` syntax immediately.

## Implementation Order

1. Add `extern` keyword to stage 0 lexer/parser (`src/syntax/`)
2. Stage 0: register extern fns in type env, synthesize `FuncType` + emit WASM imports
3. Stage 0: generalize both linker guards, add import deduplication
4. End-to-end test: extern fn called from user code, assert WAT output contains
   correct `(import ...)` declaration
5. Stage 0: parse extern blocks (grouped syntax)
6. Tree-sitter: add `extern_declaration` + `extern_block` rules + highlights
7. Boot compiler: lexer/parser/resolver/checker
8. Boot compiler: lower_core + codegen + linker (consider `ExternalRef` reuse)
9. Boot compiler: add monomorphizer guard for extern FuncIds
10. Update `docs/spec.md` and `docs/grammar.ebnf`
11. Playground: curated web API modules
12. Extend extern-safe type set (Phase 2: richer boundary types)
