# Prelude methods returning stdlib types

Status: proposal.

## Goal

Unlock prelude-backed methods whose public signatures mention stdlib-defined
nominal types, starting with:

```tw
Vector.chunks<T>(xs: Vector<T>, size: Int) Vector<view.View<Vector<T>>>
Vector.windows<T>(xs: Vector<T>, size: Int) Vector<view.View<Vector<T>>>
```

The collection ergonomics pass intentionally left these conveniences out because
`View` is defined in `boot/stdlib/view.tw`, while prelude method registration is
bootstrapped from `boot/prelude/*.tw` signatures in an environment that only
knows builtin types.

## Current limitation

`boot/compiler/base_env.tw` builds the initial builtin/prelude environment by
loading `boot/prelude/signatures/*.tw` and public functions from
`boot/prelude/*.tw` through `boot/compiler/signatures.tw`.

That signature loader resolves type annotations against `builtin_type_env()`:

- builtin named types such as `Option`, `Result`, `Cell`, `Iterator`, `Set`
- primitive and builtin container syntax such as `Vector<T>` and `Dict<K, V>`

It does **not** analyze imports or stdlib modules, so a prelude function like:

```tw
use @std.view

pub fn chunks<T>(xs: Vector<T>, size: Int) Vector<view.View<Vector<T>>> { ... }
```

fails during bootstrap with `Undefined type: view.View`.

This is adjacent to, but not solved by, the recursive module interface work in
`docs/plans/recursive-module-groups.md`: that machinery lives in the normal
frontend analysis path, while prelude signature loading is an earlier bootstrap
path used to construct the base environment before regular module analysis runs.

## Desired behavior

Prelude modules should be able to expose methods whose signatures reference
selected stdlib types when those references are acyclic and signature-only. The
first target is `Vector` returning `@std.view.View` windows.

The implementation should preserve these invariants:

- no hidden user-visible imports beyond existing prelude behavior
- stable nominal `TypeId`s for stdlib exported types
- no duplicate `View` type identity between bootstrap signatures and regular
  analysis
- no broadening of prelude method visibility beyond public declarations
- no special case in user code: `xs.chunks(2)` should resolve like any other
  inherent `Vector` method once implemented

## Possible approaches

### Option A — make signature loading import-aware

Teach `signatures.load_signatures` to understand imports enough to resolve types
from imported modules' public interfaces.

For `boot/prelude/vector.tw`, this would let `use @std.view` contribute the
`View` type when resolving the return type of `chunks` / `windows`.

Pros:

- general solution for future prelude methods returning stdlib types
- aligns bootstrap signatures with normal source-level imports

Cons:

- risks duplicating part of frontend analysis in the signature loader
- must carefully reuse canonical type identities, not synthesize parallel ones
- can grow into a second module resolver if not constrained

### Option B — seed selected stdlib type stubs into the bootstrap env

Predeclare a small set of stdlib exported types, beginning with
`@std.view.View`, in the environment used by the prelude signature loader. Later
regular analysis must reuse/fill the same type identity.

Pros:

- focused fix for the immediate blocker
- smaller implementation surface

Cons:

- introduces another curated bootstrap list
- easy to forget when adding future stdlib-returning prelude methods
- still needs origin/type-id unification with regular module analysis

### Option C — move prelude method registration onto normal module analysis

Instead of deriving prelude methods from the signature loader, analyze prelude
modules with the normal recursive module interface pipeline and derive builtin
method entries from those checked interfaces.

Pros:

- one module analysis model
- naturally benefits from recursive interfaces and stdlib imports

Cons:

- largest change
- must preserve early availability of prelude methods while bootstrapping the
  compiler itself
- likely needs careful staging to avoid circular base-env construction

## Recommended path

Start with a constrained version of **Option A**:

1. Extend the signature loader with a signature-only import interface lookup for
   stdlib/prelude modules.
2. Reuse the existing canonical module path and exported type identity machinery
   rather than allocating ad-hoc type ids.
3. Restrict the first pass to resolving imported type names in public function
   signatures; do not typecheck bodies or values.
4. Add regression coverage with `Vector.chunks` / `Vector.windows` returning
   `Vector<view.View<Vector<T>>>`.

If this begins to duplicate too much of `query/analyze.tw`, stop and reassess in
favor of Option C.

## Implementation sketch

1. Add a signature-interface cache keyed by canonical module path.
2. For each imported module referenced while loading signatures:
   - parse the module
   - collect public type declarations into an interface
   - assign/reuse canonical type ids compatible with regular analysis
   - expose those types under the import alias and selective imports
3. Use that enriched type environment when resolving public prelude function
   signatures.
4. Add `Vector.chunks` and `Vector.windows` back to `boot/prelude/vector.tw`,
   forwarding through `@std.view`:

   ```tw
   use @std.view

   pub fn chunks<T>(xs: Vector<T>, size: Int) Vector<view.View<Vector<T>>> {
     view.from(xs).chunks(size)
   }

   pub fn windows<T>(xs: Vector<T>, size: Int) Vector<view.View<Vector<T>>> {
     view.from(xs).windows(size)
   }
   ```

5. Add API docs and boot tests for the vector conveniences.
6. Rebuild the self-hosted payload and run boot tests.

## Acceptance criteria

- `Vector.chunks` and `Vector.windows` are available as inherent vector methods.
- Their return type is `Vector<View<Vector<T>>>`, sharing the original vector via
  `View` windows.
- Invalid sizes return an empty vector, matching `View.chunks` / `View.windows`.
- `View` has the same nominal identity whether referenced from prelude method
  signatures or imported directly from `@std.view`.
- `make stage2` reaches fixed point and `target/twk run boot/tests/main.tw`
  passes.
