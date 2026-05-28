# LSP Type Definition Plan

## Goal

Implement `textDocument/typeDefinition` so users can jump from a symbol to the
definition of its **type** rather than the symbol's own definition site.

---

## Scope

In scope:

* Expressions: local bindings, parameters, function calls, field accesses,
  method calls, literals with a named type.
* Type annotations: jump to the referenced type definition directly.
* Variant constructors and patterns: jump to the parent enum type.

Out of scope:

* Primitive types (`Int`, `Float`, `Bool`, `Void`) -- no user-navigable
  definition site.
* Generic type parameters (`T`) -- no definition to jump to.
* Multi-target responses (e.g. union-like scenarios) -- Twinkle has no union
  types outside enums.

---

## Design

The feature reuses the existing definition infrastructure. The key difference
from `textDocument/definition` is one extra step: after identifying the symbol
under the cursor, look up its **type** from `CheckResult.type_map`, then resolve
that type to a definition location.

### Type resolution chain

1. Find the expression/pattern at the cursor position (reuse existing
   offset-to-node logic from `definition.tw`).
2. Look up the expression's type via `snap.typed.type_map[expr_id]`.
3. Extract the `TypeId` from the `MonoType`:
   - `.Named(tid, _)` -- user-defined type or builtin with a source definition.
   - `.Func(...)`, `.Var(...)`, primitives -- return null (no navigable target).
4. Map `TypeId` to source location via `env.type_origins[tid.id]` (gives
   `"module_path::type_name"`), then `find_type_in_module()`.

For symbols already in type-annotation position (e.g. cursor is on `Point` in
`x: Point`), the existing `resolve_type_name` path already finds the type
definition -- the handler can fall through to that.

---

## Implementation Steps

1. Advertise `typeDefinitionProvider: true` in server capabilities.
2. Add a `"textDocument/typeDefinition"` case in `handle_request` in
   `server_core.tw`, routing to a new handler.
3. Add a `type_definition(snap, offset)` query function in
   `query/definition.tw` (or a new `query/type_definition.tw` module) that:
   - Finds the node at the offset.
   - Gets its type from `CheckResult.type_map`.
   - Resolves `.Named(tid, _)` to a definition location via `type_origins` and
     `find_type_in_module`.
   - Returns `DefinitionResult?`.
4. Reuse the existing `lsp_definition.definition_response()` to format the LSP
   response -- the response shape is identical to `textDocument/definition`.
5. Add tests.

---

## Test Plan

* Local binding with a record type -- jumps to the record's `type` declaration.
* Function return value -- jumps to the return type definition.
* Enum variant expression -- jumps to the parent enum type.
* Pattern binding in `case` arm -- jumps to the matched type.
* Cursor on a type annotation -- jumps to the referenced type (same as go-to-def
  for types).
* Primitive-typed binding -- returns empty/null response.
* Generic-typed binding (`Vector<Point>`) -- jumps to `Vector` (the outer type).
* Imported type -- jumps to the type in the defining module.
* Multibyte text before the cursor maps positions correctly.

---

## Exit Criteria

`textDocument/typeDefinition` returns the source location of the type for common
expression and pattern forms, with null responses for primitives and unresolvable
types.
