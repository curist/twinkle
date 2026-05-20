# Extern Types (`externref`-backed opaque host handles)

## Goal

Allow Twinkle programs to declare **opaque types** that represent host-provided
objects (DOM elements, canvas contexts, WebSocket connections, etc.). These
types are backed by Wasm `externref` — the runtime holds the JS object directly,
no handle table or integer indirection needed.

Combined with Twinkle's existing extern functions and inherent method resolution,
this gives a clean, type-safe API for browser/host interop:

```tw
// canvas.tw
extern dom type CanvasContext

extern dom {
  fn get_context(id: String) CanvasContext
  fn fill_rect(ctx: CanvasContext, x: Float, y: Float, w: Float, h: Float)
  fn set_fill_style(ctx: CanvasContext, color: String)
  fn clear_rect(ctx: CanvasContext, x: Float, y: Float, w: Float, h: Float)
}

// Inherent methods — CanvasContext is first param, same module
ctx := dom.get_context("mycanvas")
ctx.set_fill_style("red")
ctx.fill_rect(10.0, 20.0, 100.0, 50.0)
```

JS side provides the import implementations:

```javascript
imports.dom = {
  get_context: (id) => document.getElementById(id).getContext("2d"),
  fill_rect: (ctx, x, y, w, h) => ctx.fillRect(x, y, w, h),
  set_fill_style: (ctx, color) => { ctx.fillStyle = color; },
  clear_rect: (ctx, x, y, w, h) => ctx.clearRect(x, y, w, h),
};
```

## Design Principles

**Approach A: JS glue for non-trivial wiring.** The compiler does not generate
JS code or embed JS snippets. When the host API doesn't map 1:1 to a flat
function (e.g., `document.getElementById(id).getContext("2d")` is a method
chain), the user writes a thin JS wrapper. This is the same approach Gleam uses
for its JavaScript FFI — minimal, predictable, no magic.

**Why not auto-dispatch or inline JS?** We considered two alternatives:

- Inline JS bodies (`= js"ctx.fillRect(x, y, w, h)"`) — embeds a second
  language in Twinkle source, hard to lint/typecheck, tooling complexity.
- Auto method dispatch (`= .fillRect`) — covers the 80% case but adds
  language surface area for a constrained pattern. Can be revisited later.

Both are potential future extensions. Starting with approach A keeps the
language clean and the compiler simple.

## Current State

- Phase 1 extern FFI is fully implemented (both compilers). Extern functions
  support `Int`, `Float`, `Bool`, `String`, `Void` at the boundary.
- The Wasm backend already has `HeapType.Extern` and can encode `(ref extern)` /
  `externref` in WAT and binary output.
- `MonoType` has no variant for extern types yet.
- `is_extern_safe_type` in the resolver rejects anything beyond primitives.

## Non-Goals

- Generating JS glue code from Twinkle declarations.
- Linear/affine ownership for resource handles (orthogonal future work,
  tracked in `docs/open-questions.md` §4).
- Passing Twinkle records, enums, or vectors across the extern boundary
  (Phase 2 of the FFI plan).
- Module-private fields or access control beyond what `pub` already provides.

## Design

### Syntax

Extern type declarations appear inside or alongside extern blocks:

```tw
// Standalone
extern dom type CanvasContext
extern dom type Element

// Inside a grouped block
extern dom {
  type CanvasContext
  type Element
  fn get_context(id: String) CanvasContext
}
```

`pub extern dom type CanvasContext` exports the type to other Twinkle modules.

Each extern type is **nominal**: `CanvasContext` and `Element` are distinct
types even though both lower to `externref`. You cannot pass a `CanvasContext`
where an `Element` is expected.

### Namespace rules

Extern types live in the **declaring Twinkle module's type namespace**, not in
the host-module namespace. The `dom` in `extern dom type CanvasContext` is the
Wasm import module name — it does not create a `dom.CanvasContext` qualified
name in Twinkle. The type is referenced as bare `CanvasContext` within its
declaring module.

This differs from extern functions, which are namespaced as `dom.get_context`.
The distinction:
- **Extern functions** use `host_module.fn_name` at call sites (they're
  runtime-dispatched imports).
- **Extern types** are compile-time-only identifiers — they exist in the
  Twinkle type namespace like any other type.

When `pub`, other Twinkle modules import the type via the declaring module:

```tw
// other_module.tw
use .canvas.{CanvasContext}   // imports the type from canvas.tw
use .canvas                   // or: canvas.CanvasContext
```

### Inherent method resolution for extern types

Extern functions where the first parameter is an extern type become dot-call
candidates, following the same rules as regular inherent methods:

1. The extern function must be **in scope** at the call site — either declared
   in the same Twinkle module or imported via `use`.
2. The extern type must match the first parameter's type.
3. Resolution order: record fields first (N/A for extern types — they have
   none), then inherent/module methods.

This means importing a type alone is not enough for dot syntax — you must also
import (or have in scope) the extern functions that operate on it:

```tw
use .canvas.{CanvasContext}
// ctx.fill_rect(...)  — only works if fill_rect is also in scope
use .canvas.{fill_rect}
// now ctx.fill_rect(...) resolves
```

Or import the whole module:

```tw
use .canvas
// canvas.CanvasContext, and all functions are in scope for dot-call
```

### Extern types as boundary types

Extern types are valid in extern function signatures — both as parameters and
return types. They join the existing extern-safe set:

| Twinkle type | Wasm type | Notes |
|---|---|---|
| `Int` | `i64` | |
| `Float` | `f64` | |
| `Bool` | `i32` | 0/1 |
| `String` | `(ref $string)` | GC string ref |
| `Void` | (no result) | |
| **extern type** | **`(ref extern)`** | **new; non-null** |

Extern types are valid **everywhere regular types are**: record fields, function
parameters, local bindings, and data structures (`Vector<Element>`,
`Dict<String, CanvasContext>`) — not just extern function signatures. This is
essential for real programs that need to thread host handles through application
state:

```tw
type GameState = .{ canvas: CanvasContext, score: Int }
fn draw(state: GameState) { state.canvas.clear_rect(0.0, 0.0, 800.0, 600.0) }

// Collections of handles work naturally
elements: Vector<Element> = collect id in ids { dom.get_element(id) }
```

Twinkle's immutable value semantics already prevent misuse — you can't mutate
a handle, only pass it around. If the extern type appears as a record field,
the backend emits the field as `(field (ref extern))`. If it appears as a
function parameter or local, the backend uses `(ref extern)` as the local type.
For container storage, `any.convert_extern` / `extern.convert_any` handle the
`externref` ↔ `anyref` conversion at boxing boundaries (see Monomorphizer
section).

**Opacity:** extern types cannot declare fields, variants, or methods
intrinsically. They are pure opaque nominal handles. `case` matching on an
extern type is a type error. The only operations are passing them to functions
(extern or regular) and storing them in data structures.

Extern types do not implement equality, ordering, or hashing by default. They
cannot be used as `Dict` keys or in `==` / `!=` / `<` expressions. Users who
need these semantics must provide explicit host functions (e.g.,
`dom.same_node(a, b) Bool`, `dom.hash_node(el) Int`).

Extern types are runtime-only and cannot participate in compile-time constants,
serialization, or persistence. Converting to/from a serializable form requires
explicit host functions (e.g., `dom.element_id(el) String`). This is the first
"ambient runtime identity" type in Twinkle — unlike all other values, extern
handles have identity that exists outside the language's value semantics.

### Nullability

Extern types are **non-null** by default, matching Twinkle's general philosophy.
At the Wasm level, `Element` lowers to `(ref extern)` (non-null).

**Boundary null behavior:** if a JS import is declared as returning a non-null
extern type but the JS function returns `null` or `undefined`, the Wasm runtime
traps with a type error at the call boundary. This is standard Wasm behavior
for non-null ref types — the embedder validates the returned value against the
declared import signature. The compiler does not insert additional checks.
Document this in the spec: "the host is trusted to return non-null values for
non-null extern types; violations are Wasm runtime traps."

**Nullable extern types (Phase 2):** `Option<ExternType>` support is deferred.
The natural lowering would map `Element?` to nullable `externref` with `None`
as `ref.null extern`:

```tw
// Phase 2
extern dom {
  type Element
  fn query_selector(el: Element, sel: String) Element?
}
```

At the type system level, `Option<ExternType>` uses the existing `Option`
machinery — no special type-level handling needed.

**Deferred to Phase 2.** The Wasm lowering of `Option<ExternType>` is an
ABI/layout specialization, not a trivial reuse of existing Option codegen.
The current `Option<T>` lowering assumes GC ref types (e.g.,
`(ref null $struct)` with `ref.null none` for `None`). For
`Option<ExternType>`, the null must be `ref.null extern` instead — a different
Wasm type hierarchy. This also touches monomorphization keys, null-test
codegen, and JS boundary semantics. Phase 1 delivers non-null extern types
end-to-end; nullable extern types are added once that foundation is solid.

In Phase 1, if a user needs "maybe no element", model it explicitly:

```tw
extern dom {
  fn try_get_element(id: String) Bool  // returns whether found
  fn get_element(id: String) Element   // traps if not found
}
```

Or use a Result-style pattern with an error string via a JS wrapper that
validates before returning.

### Type representation

Add a new `MonoType` variant:

```tw
pub type MonoType = {
  // ... existing variants ...
  ExternRef(ExternTypeId),  // opaque host handle
}

pub type ExternTypeId = .{ id: Int }
```

Each `extern type` declaration gets a globally unique `ExternTypeId.id`
(allocated from the same counter as `TypeId`). Nominal identity is carried
solely by `id`.

The host module name and type name are stored in the resolver's type entry
metadata (for diagnostics, error messages, and debug printing), not in the
`ExternTypeId` itself. This keeps the identity object minimal and avoids
coupling type identity to import module strings — which matters if modules are
renamed, aliased, or reexported.

**Alternative:** reuse `Named(TypeId, [])` with a flag on the type entry
marking it as extern. This avoids adding a new `MonoType` variant but muddies
the distinction between Twinkle-defined and host-defined types. A dedicated
variant is cleaner.

### Wasm lowering

All extern types lower to non-null `(ref extern)` in Wasm. In WAT text format,
note that bare `externref` is shorthand for `(ref null extern)` (nullable).
Phase 1 uses the non-null form everywhere:

- **Value type:** `(ref extern)` — non-null
- **Record field:** `(field $name (ref extern))` — non-null
- **Function param/result:** `(ref extern)` — non-null
- **Local variable:** `(ref extern)` — non-null

The `val_type_of_mono` function in `wasm_layout.tw` adds:

```tw
.ExternRef(_) => .Ref(false, .Extern),  // (ref extern), non-null
// Note: Ref(true, .Extern) would be (ref null extern) aka externref — Phase 2
```

**Default value for locals:** Wasm requires locals to be initialized. For
non-null `(ref extern)`, there is no default value — the compiler must ensure
extern-type locals are always assigned before use. This is already the case for
Twinkle's `let`/`:=` bindings (always initialized at declaration), but worth
verifying that no codegen path emits uninitialized `(ref extern)` locals.

### Layout

Add a `WExternref` variant to `WasmValType`:

```tw
pub type WasmValType = { WI32, WI64, WF64, WAnyref, WExternref }
```

`layout_of` in `wasm_layout.tw` returns `Scalar(WExternref)` for extern types.

This distinction matters because `externref` and `anyref` are separate Wasm
type hierarchies — `externref` is **not** a subtype of `anyref` in the Wasm GC
spec. If codegen paths that handle `WAnyref` emit `ref.cast`, `struct.get`, or
other GC-specific operations, applying them to an `externref` would produce
Wasm validation errors. A dedicated `WExternref` variant prevents this class
of bugs and keeps the anyref-elimination work cleanly separated from extern
type support.

### GC reachability

An `externref` keeps the underlying host object reachable for as long as the
Wasm runtime retains the reference. Lifetime and reclamation are delegated to
the host GC integration — the Wasm VM and the host GC cooperate to trace
references across the boundary. This is standard behavior in browser Wasm
runtimes and V8/SpiderMonkey/JavaScriptCore.

Extern references are **strong** — there is no weak-reference variant. A
Twinkle program holding an `Element` in a record or vector keeps the
corresponding JS DOM node alive. This is the expected behavior for handle-based
FFI and matches how other Wasm languages (Gleam, Kotlin/Wasm) handle host
references.

## Compiler Changes

### Parser

| File | Change |
|---|---|
| `parser.tw` | In `parse_extern_block`, recognize `type Ident` in addition to `fn` signatures. For standalone: `extern <module> type <Ident>`. |
| `ast.tw` | Add `ExternTypeDecl` to `Item` or extend `ExternFunctionDecl` parent to cover types. |

Grammar addition:

```ebnf
extern_type_decl = [ "pub" ] "extern" ident "type" UPPER_IDENT ;
extern_block_item = extern_fn_sig | "type" UPPER_IDENT ;
```

**Parser note:** inside extern blocks, `type` is already a keyword, so the
parser dispatches on it unambiguously (alongside `fn`). This does not conflict
with type aliases because type aliases are a top-level construct, not valid
inside extern blocks. If future syntax allows type aliases inside extern blocks
(e.g., `type Ctx = CanvasContext`), disambiguation would be needed — but that
is not planned.

### Resolver

| File | Change |
|---|---|
| `resolver.tw` | Register extern types in the type namespace. Create a `TypeEntry` with no fields/variants, flagged as extern. Extend `is_extern_safe_type` to accept extern type `MonoType`s. |

The extern type must be resolvable by name in the declaring module (and in
importing modules if `pub`). Any extern type can be used in any extern function
signature regardless of which host module declared it — a `dom.query_selector`
can return an `Element` declared via `extern html type Element`. All extern
types are `externref` at the Wasm level, so there is no ABI concern from
mixing them across host modules. The Twinkle type checker enforces nominal
safety; the Wasm boundary is untyped for `externref` anyway.

### Type checker

No major changes. Extern types are nominal — the checker already handles
nominal types via `TypeId` / `MonoType` comparison. Unification treats
`ExternRef(id1) ≠ ExternRef(id2)` when `id1 ≠ id2`.

### Lower Core

| File | Change |
|---|---|
| `lower_core.tw` | Extern types have no body to lower. Ensure `ExternImport` entries use `externref` for extern-type params/returns. |
| `core_ir.tw` | `ExternImport.param_tys` / `.return_ty` already use `MonoType`, so the new `ExternRef` variant flows through naturally. |

### Monomorphizer

Extern types are already concrete (no type parameters on extern types in Phase
1). The monomorphizer skips them like it skips extern functions.

Extern types are already concrete (no type parameters on extern types in
Phase 1). The monomorphizer skips them like it skips extern functions.

`ExternRef` is allowed as a generic type argument in Phase 1 —
`Vector<Element>`, `Dict<String, CanvasContext>`, and user-defined generic
records all work. The monomorphizer treats `ExternRef` like any other concrete
payload.

**PVec / Dict boxing:** PVec stores all elements as `anyref`, and `externref`
is not a subtype of `anyref` in the Wasm GC type hierarchy. However, the
WasmGC spec provides zero-cost conversion instructions:

- `any.convert_extern` — `(ref extern)` → `(ref any)` (for storing)
- `extern.convert_any` — `(ref any)` → `(ref extern)` (for loading)

These are part of Wasm 3.0 (W3C standard since September 2025) and ship in
all engines that support WasmGC — Chrome 119+, Firefox 120+, Safari 18.2+,
and current Node.js. Since Twinkle already requires WasmGC, these are
available everywhere Twinkle runs.

Implementation: add `ExternRef` / `WExternref` cases to `emit_box_to_anyref`
(emit `any.convert_extern`) and `emit_unbox_from_anyref` (emit
`extern.convert_any`). No allocation or wrapping structs — these are pure
type-system casts.

The one restriction is `Option<ExternType>`, which requires nullable
`externref` lowering — deferred to Phase 2.

**Future:** generic extern types (`extern dom type NodeList<T>`) would need
their own monomorphization. Defer this — it's not needed for either phase.

### Codegen

| File | Change |
|---|---|
| `wasm_layout.tw` | `val_type_of_mono`: return `Ref(false, .Extern)` for `ExternRef`. `layout_of`: return `Scalar(WExternref)`. |
| `emit.tw` | When building `ImportDef` params/results for extern imports, emit `(ref extern)` for extern-type parameters. |

### Stage 0 (Rust)

Mirror the boot compiler changes. The Rust compiler needs to parse `extern type`
so it can bootstrap boot source that uses the feature.

| File | Change |
|---|---|
| `src/syntax/ast.rs` | Add `ExternType` variant to `Item`. |
| `src/syntax/parser.rs` | Parse `extern <mod> type <Ident>` and inside blocks. |
| `src/types/env.rs` | Register extern types. |
| `src/ir/lower.rs` | Handle extern type references. |
| `src/codegen/emit.rs` | Emit `(ref extern)` for extern type params. |

## Examples

### Canvas drawing

```tw
// canvas.tw
extern dom type CanvasContext

extern dom {
  fn get_context(id: String) CanvasContext
  fn fill_rect(ctx: CanvasContext, x: Float, y: Float, w: Float, h: Float)
  fn stroke_rect(ctx: CanvasContext, x: Float, y: Float, w: Float, h: Float)
  fn set_fill_style(ctx: CanvasContext, color: String)
  fn set_stroke_style(ctx: CanvasContext, color: String)
  fn clear_rect(ctx: CanvasContext, x: Float, y: Float, w: Float, h: Float)
  fn begin_path(ctx: CanvasContext)
  fn move_to(ctx: CanvasContext, x: Float, y: Float)
  fn line_to(ctx: CanvasContext, x: Float, y: Float)
  fn stroke(ctx: CanvasContext)
}

// Usage — inherent methods via dot syntax
ctx := dom.get_context("game")
ctx.set_fill_style("black")
ctx.fill_rect(0.0, 0.0, 800.0, 600.0)

ctx.set_stroke_style("white")
ctx.begin_path()
ctx.move_to(100.0, 100.0)
ctx.line_to(200.0, 200.0)
ctx.stroke()
```

### Multiple extern types

```tw
extern dom type Element
extern dom type CanvasContext

extern dom {
  fn get_element(id: String) Element       // traps if not found (non-null)
  fn get_context_2d(el: Element) CanvasContext
  fn set_text(el: Element, text: String)
}

// Thread handles through a record
type App = .{
  root: Element,
  canvas: CanvasContext,
  score: Int,
}
```

### JS glue

```javascript
// Provided by the host at instantiation
const imports = {
  dom: {
    get_element: (id) => {
      const el = document.getElementById(id);
      if (!el) throw new Error(`Element not found: ${id}`);
      return el;  // must be non-null — Wasm traps on null (ref extern)
    },
    get_context_2d: (el) => el.getContext("2d"),
    set_text: (el, text) => { el.textContent = text; },
    fill_rect: (ctx, x, y, w, h) => ctx.fillRect(x, y, w, h),
    // ...
  },
};
```

## Implementation Order

### Phase 1: Non-null extern types

1. [ ] Add `ExternRef` variant to `MonoType` (both compilers — Rust `MonoType` enum too)
2. [ ] Add `WExternref` to `WasmValType`
3. [ ] Add exhaustive match arms for `ExternRef` / `WExternref` in all total `case` expressions: `val_type_of_mono`, `layout_of`, `val_type_key`, `mono_to_key`, `emit_box_to_anyref` (emit `any.convert_extern`), `emit_unbox_from_anyref` (emit `extern.convert_any`), and any others
4. [ ] Parser: `extern <mod> type <Ident>` syntax, standalone first, then inside blocks (both compilers)
5. [ ] Resolver: register extern types, extend `is_extern_safe_type`, forbid `==` on extern types, reject extern types in `Option` type args
6. [ ] Wasm layout: `val_type_of_mono` returns `(ref extern)` for `ExternRef`; `layout_of` returns `Scalar(WExternref)`
7. [ ] Codegen: emit `(ref extern)` in import signatures; support extern types in record fields, function params, locals
8. [ ] Monomorphizer: verify `ExternRef` works as generic payload (`Vector<Element>`, `Dict<String, Element>`, user generics)
9. [ ] Verify no codegen path emits uninitialized `(ref extern)` locals (test: `case` branches where extern-type local assigned in only one branch)
10. [ ] End-to-end test: extern type passed through extern fn, bound in local, stored in record field, stored in `Vector` — verify WAT output
11. [ ] Update `docs/spec.md` §7.2 with extern type syntax, semantics, and boundary null behavior
12. [ ] Playground: example with canvas extern types

### Phase 2: Nullable extern types (`Option<ExternType>`)

13. [ ] `Option<ExternType>` layout specialization: nullable `externref`, `None` → `ref.null extern`
14. [ ] Null-test codegen: `ref.is_null` for `None` check on extern Option
15. [ ] End-to-end test: `Option<ExternType>` round-trip through extern boundary

## Open Questions

1. **Generic extern types:** `extern dom type NodeList<T>` — useful for typed
   collections from the host. Defer to a later phase.

2. **Extern type equality / ordering / hashing:** all forbidden by default
   (decided, not open). Identity on host objects is not semantically portable
   — objects may be wrapped, proxied, or recreated across calls. Twinkle
   should not promise semantics it cannot define. The compiler emits an error
   for `==`, `!=`, `<`, `>`, and dict-key usage on extern types. Host-provided
   comparison and hashing functions are the escape hatch:

   ```tw
   extern dom {
     fn same_node(a: Element, b: Element) Bool
     fn hash_node(el: Element) Int
   }
   ```

   This fits naturally into Twinkle's capability-record pattern.

3. **String interop at the boundary:** Current strings are GC byte arrays.
   The JS bridge already handles UTF-8 decode/encode. Extern types don't
   change this — strings continue to use the existing bridge. Document this
   as part of the extern type ABI contract.

## Related Docs

- [archive/js-ffi.md](archive/js-ffi.md) — Phase 1 extern FFI plan
- [backend-anyref-elimination.md](backend-anyref-elimination.md) — anyref reduction strategy
- [../open-questions.md](../open-questions.md) §4–6 — resource ownership and FFI handles
- [../spec.md](../spec.md) §7.2 — current extern declaration syntax
