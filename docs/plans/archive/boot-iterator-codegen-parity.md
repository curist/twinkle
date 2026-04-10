# Boot iterator codegen implementation plan

## Context

While moving intrinsic helper implementations out of `boot/compiler/codegen/emit.tw`,
we found that the boot compiler's iterator path is not just missing a helper
function. The generated WAT shows that iterator lowering itself is currently not
coherent.

## Current status

The first iterator-representation fix is now in place:

- `Iterator<T>` is recognized explicitly in `boot/compiler/codegen/wasm_layout.tw`
- iterator layout now lowers to the runtime `rt_types__IterState`
- `val_type_of_mono(...)` no longer falls back to a user record for `Iterator<T>`

That corrected the earlier bad representation selection. Generated WAT now shows
iterator construction through `struct.new $rt_types__IterState`, and
`IterItem.rest` uses `rt_types__IterState` instead of an accidental empty user
iterator struct.

A first pass at specialized iterator-next helper emission also landed in
`boot/compiler/codegen/emit.tw`:

- iterator-next call sites now target a result-specific helper symbol
- helper discovery tracks which concrete `Option<IterItem<T>>` results are used
- helper emission now produces monomorphized helper bodies such as
  `$user__iterator_next_helper_opt_t5_str`

That fixed the old missing-symbol failure for `$iterator_next_helper`.

## New blocker discovered after helper work

The current failure is now earlier and more structural than the helper body
itself. Validating the generated Wasm fails with:

- `unknown type 276: type index out of bounds`

Using `wasm-tools dump` on the parsed binary shows the problematic type entry is
not an iterator helper type. It is a user recursive AST type:

- type 255 references type 276
- those correspond to `Expr` and `ExprKind`

In other words, the new iterator work got us far enough that validation now
exposes a separate backend limitation: boot is still emitting recursive GC type
cycles as individually ordered types rather than as an explicit recursive type
group.

This matters because a pair like:

- `Expr` → `ExprKind`
- `ExprKind` → `Expr`

cannot be made valid by simple reordering alone. One direction will always be a
forward type reference unless the backend emits a proper recursive group.

So the immediate blocker was no longer “iterator helper missing” and may not
even be “iterator helper body is malformed”. The generated helper can still need
more work, but the first validation error was pointing at recursive type
emission.

## Follow-up debugging after the recursive-type fix

After teaching the WAT emitter to emit explicit `(rec ...)` groups and ordering
those SCCs topologically, validation moved past the old type-index error.

That exposed a second set of backend issues unrelated to iterator layout:

- `Never`-returning calls were still being lowered as though they produced a
  value, leading to invalid `local.set` sites after calls like `host_exit`
- helper imports that return erased `anyref` values were being stored directly
  into typed locals without unboxing or `ref.cast`
- host `read_file` needed an explicit cast back to `rt_types__Variant` before
  calling the array-result bridge helper

Those fixes moved validation further forward again.

## Current blocker after those fixes

The next validator failure is now in vector-builder lowering, not iterator-next:

- boot is lowering transient builder operations through runtime functions such
  as `rt_arr__builder_new`
- the runtime builder API uses raw `rt_types__Array` builder values
- the boot compiler is still assigning ordinary `Vector<T>` / `rt_types__PVec`
  storage types to those temporaries in emitted functions

That currently produces Wasm type mismatches at builder call sites, for example
when storing the result of `rt_arr__builder_new` into a local that is still
typed as `PVec`.

So the latest state is:

1. iterator representation is fixed
2. specialized iterator-next helper symbol emission is in place
3. recursive GC type groups are now emitted correctly
4. validation now fails later in vector-builder transient representation

## Additional progress after the builder investigation

The builder mismatch was partially fixed:

- builder temporaries discovered through `vector_builder_new` / `from` and
  simple alias propagation are now emitted as erased `anyref` locals in codegen
- `vector_builder_push` / `vector_builder_freeze` now cast their builder input
  back to `rt_types__Array` at the runtime boundary
- `vector_builder_push` no longer assumes the runtime helper returns a value;
  codegen now synthesizes a placeholder result for the rewritten-but-unused ANF
  binding

Fixing that moved validation forward again.

## Additional backend issues exposed while validating further

As validation progressed, several other representation mismatches surfaced and
were fixed:

- `iterator_unfold` now boxes scalar seeds before storing them in
  `rt_types__IterState.seed : anyref`
- `Cell<anyref-backed>` operations now box and unbox correctly for
  `cell_new`, `cell_get`, `cell_set`, and `cell_update`
- `from_code_point` now narrows its integer argument to the helper's expected
  `i32` ABI before calling the import

These are not iterator-specific bugs, but they were previously hidden behind
earlier validation failures.

## Current blocker

Validation now reaches even later functions and currently fails in another
representation mismatch:

- function `user__$f219_110101119`
- `type mismatch: expected i64, found anyref`

So the current state is now:

1. iterator representation fix is in place
2. specialized iterator-next helper symbols and bodies are emitted
3. recursive GC type groups are emitted with `(rec ...)`
4. builder-region transient storage is partially repaired
5. several anyref/scalar boxing bugs have been fixed
6. validation still fails later in unrelated backend representation plumbing

## Recommended next step from here

Continue the same validator-driven cleanup loop:

- inspect the failing `user__$f219_110101119` body around the reported offset
- identify which intrinsic/runtime boundary is still leaving an `anyref` where
  a scalar `i64` is expected
- patch that boundary in the emitter rather than papering over it in WAT

At this point the iterator-next helper is no longer the first blocker. The work
has turned into a broader boot-backend representation cleanup that the iterator
changes helped uncover.

The visible validation error was:

- missing `iterator_next_helper`

But inspection of the emitted WAT for `boot/main.tw` also shows that boot is
sometimes constructing user-defined empty iterator structs instead of a stable
runtime iterator state representation. That means this is not a small wiring
issue. It is a correctness gap in iterator codegen.

This document turns the earlier parity notes into a concrete implementation
plan. The goal is to build proper iterator codegen support in boot rather than
relying on generic fallback behavior.

## Problem statement

Boot currently has partial iterator scaffolding but not a complete iterator
backend model.

### What we see today

- `emit_intrinsic_iterator_unfold()` lowers to `StructNew(sym)` based on
  `layout_of(result_mono, ctx.env)`.
- `emit_intrinsic_iterator_next()` lowers directly to
  `Call("iterator_next_helper")`.
- `Iterator<T>` still exists in the builtin type environment as a named record.
- `layout_of_named()` can therefore still lower `Iterator<T>` through the
  generic named-record path.
- Generated WAT contains user-defined iterator types such as an empty
  `user__$Iterator_...` struct and sites that build them directly.

That combination means the boot backend does not yet have a single, intentional,
end-to-end iterator representation.

### Why a generic fallback is not enough

The issue is not simply that boot lacks one generic helper implementation.
Even if we added a generic `iterator_next_helper`, the surrounding lowering can
still be wrong if:

- `iterator_unfold` constructs one representation
- `IterItem.rest` expects another representation
- `iterator_next` assumes a third representation
- result wrapping uses monomorphized user structs that do not match the helper
  surface

So we should not paper over the problem by adding one more helper while leaving
iterator lowering ambiguous.

## Goals

1. Give `Iterator<T>` a deliberate backend representation in boot.
2. Make `iterator_unfold`, `iterator_next`, `IterItem.rest`, and loop lowering
   agree on that representation.
3. Keep iterator helper implementation logic out of `emit.tw`.
4. Keep backend-internal iterator support separate from public runtime modules.
5. Build a path that can later converge toward stage0-style specialization and
   optimization.

## Non-goals

This plan is not trying to fully match every stage0 iterator optimization
immediately.

In particular, the first milestone is correctness, not performance:

- no requirement to replicate all stage0 iterator helper specialization at once
- no requirement to optimize away all erased/typed transitions on the first pass
- no requirement to restructure unrelated string/parse helper work

## Recommended architecture

### 1. Treat iterator lowering as backend-internal, not generic record lowering

`Iterator<T>` should not be allowed to drift through the ordinary record path.
It needs explicit lowering.

That means boot should have a clear answer for:

- what runtime state shape represents an iterator value
- how that state is constructed by `iterator_unfold`
- how that state is consumed by `iterator_next`
- how user-facing typed values such as `IterItem<T>` and `Option<IterItem<T>>`
  are reconstructed

### 2. Keep iterator helpers out of public runtime modules

Iterator helper implementations should not live beside `runtime/arr.tw`,
`runtime/str.tw`, or `runtime/core.tw`.

They are runtime-executed code, but they are still backend-internal lowering
support, not public runtime surface.

Preferred home:

- `boot/compiler/codegen/intrinsics.tw`

That file can own iterator helper generation alongside other backend-internal
helper implementations.

### 3. Prefer an explicit non-generic iterator codegen path

The target should be proper iterator codegen support, not “hope the generic path
works”.

That means we should explicitly implement the iterator path instead of relying
on:

- generic named-record lowering for `Iterator<T>`
- accidental compatibility between generic records and runtime iterator state
- shared helper code that assumes shapes boot does not yet enforce

## Implementation plan

## Phase 1: Fix iterator representation selection

### Goal

Make `Iterator<T>` lower intentionally and consistently.

### Required changes

#### A. Detect iterator types explicitly in the layout layer

Add iterator-specific recognition in `boot/compiler/codegen/wasm_layout.tw`,
parallel to the existing `Cell` special case.

Likely steps:

- add an `is_iterator_type(env, tid)` helper
- update `layout_of_named(...)` so `Iterator<T>` lowers to `.Iterator_(...)`
  instead of generic `.Record(...)`
- define the canonical state symbol and associated layout used for iterators

#### B. Verify all iterator-adjacent types agree with that lowering

Check that the following now line up with the same representation model:

- `emit_intrinsic_iterator_unfold()`
- `emit_intrinsic_iterator_next()`
- `IterItem.rest`
- iterator unboxing / casting paths
- any helper signatures we introduce later

### Deliverable

After this phase, boot should no longer emit empty user-defined iterator state
records in places where a real iterator runtime state is intended.

## Phase 2: Choose the iterator-next helper strategy

Once representation is fixed, choose one explicit strategy for `iterator_next`.

### Option A: erased shared helper + typed wrapping in the caller

Use a shared helper implementation in `boot/compiler/codegen/intrinsics.tw`
that operates only on runtime-level erased shapes such as:

- runtime iterator state
- erased variant payloads
- `anyref`
- `rt_types__Array`

Then have `emit_intrinsic_iterator_next()`:

1. call that helper
2. rebuild the final typed `Option<IterItem<T>>` at the call site

#### Pros

- simple shared implementation
- easier helper export/import story
- lower infrastructure cost up front

#### Cons

- caller-side wrapping becomes more complicated
- emitter must know how to rebuild typed iterator results
- can increase erased/typed round-tripping

### Option B: specialized helper generation per concrete iterator shape

Track requested iterator helper shapes during emission, then generate matching
helper functions from `boot/compiler/codegen/intrinsics.tw`.

This is closer to stage0's model.

#### Pros

- helper signatures match the actual monomorphized result shapes
- less reconstruction logic at each caller
- better long-term fit for optimization and parity with stage0

#### Cons

- requires helper-request tracking and symbol generation
- more boot backend machinery
- somewhat larger implementation step

### Recommendation

Treat Option B as the long-term target.

If needed, Option A can be an intermediate correctness step, but only if it is
implemented as a deliberate temporary phase and not as the permanent iterator
architecture.

## Phase 3: Implement backend-internal iterator helper support

### Goal

Move iterator helper logic out of `emit.tw` and into backend-internal codegen
support.

### Required changes

Add iterator helper support to:

- `boot/compiler/codegen/intrinsics.tw`

This should eventually include:

- base or specialized iterator-next helper definitions
- any shared helper utilities needed by those functions
- exported symbols consumed by the user-emitted module through linker imports

### Wiring model

`emit.tw` should only:

- request/import the helper symbols it needs
- lower calls to those helper symbols

It should not own the helper bodies.

## Phase 4: Add iterator helper request tracking

If we choose specialized helpers, boot needs lightweight iterator metadata in
codegen state.

### Needed information

At minimum:

- iterator state shape
- yield type
- result option shape
- helper symbol naming
- deduplication of repeated requests

### Purpose

This lets boot emit only the iterator helpers actually needed by a module and
keeps helper signatures aligned with the lowered monomorphized shapes.

## Phase 5: Correct loop and consumer lowering

After representation and helper strategy are in place, verify iterator consumers
really use the intended model.

That includes:

- `Iterator.next()` call lowering
- `for x in iter` loops
- repeated `next()` calls
- storage of `IterItem.rest`
- interaction with closure-converted unfold callbacks

This phase is where we confirm that boot is not just producing valid WAT, but
also preserving the intended iterator semantics.

## Phase 6: Iterator-specific optimization and parity follow-up

Once correctness is established, revisit the gap with stage0's richer iterator
machinery.

Potential follow-up work:

- specialized helper request tracking parity with stage0
- representation-flow metadata for iterator temporaries
- reducing erased/typed boundary churn
- better lowering for unfold callbacks and result reconstruction
- optimization of common iterator paths

This phase is explicitly after correctness.

## Testing plan

Add focused coverage for iterator lowering and execution.

### Required tests

- iterator unfold construction produces the intended representation
- `Iterator.next()` on a simple integer iterator
- `Iterator.next()` on a string-yielding iterator
- `for x in iter` loop lowering and execution
- nested or chained iterator consumption
- boot self-host path that exercises iterator lowering in the compiler itself

### What to assert

Tests should verify both:

- generated WAT shape where practical
- actual runtime behavior

We should avoid relying only on one or the other.

## Recommended order of work

1. Fix `Iterator<T>` layout selection in `wasm_layout.tw`.
2. Confirm emitted WAT stops constructing accidental empty user iterator state
   records.
3. Choose the iterator-next helper strategy.
4. Implement iterator helper support in `codegen/intrinsics.tw`.
5. Wire `emit.tw` to import/lower through that support.
6. Add focused iterator tests.
7. Revisit stage0-style specialization and optimization.

## Decision summary

The iterator issue should be treated as a real iterator codegen task, not a
small missing-helper cleanup.

The right direction is:

- explicit iterator lowering
- backend-internal helper generation
- proper non-generic iterator support
- later optimization/specialization work after correctness is restored
