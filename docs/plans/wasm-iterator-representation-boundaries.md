# Wasm Iterator Representation Boundaries

**Goal:** Stabilize the Wasm backend by making iterator-related specialization an explicit
local optimization rather than an implicit ABI change that leaks across function, closure,
and helper boundaries.

This is a follow-up to [wasm-type-erasure-reduction.md](./wasm-type-erasure-reduction.md).
That plan successfully introduced typed iterator fast paths, but it also exposed a broader
backend design problem: iterator representation policy is currently spread across inference,
local-slot allocation, helper generation, function ABI emission, closure func types, and
closure trampolines.

The result is a recurring bug pattern:

* one subsystem switches to a typed iterator-adjacent representation
* a neighboring subsystem still assumes the erased runtime representation
* the generated WAT either fails validation or traps at runtime

This document proposes a refactor direction intended to stop that class of bug.

---

## Problem Summary

`Iterator<T>` currently has two backend representations:

* erased runtime form: `$rt_types__IterState`
* specialized form: `iter_state__...`

That same split now also exists for:

* `UnfoldStep<Y, S>`
* `IterItem<T>`
* `Option<IterItem<T>>`
* closure func types for functions returning those values
* typed closure trampolines for functions returning those values

The backend has no single source of truth for when a value should use the erased form
versus the specialized form. Instead, the policy is duplicated across:

* [src/codegen/ctx.rs](../../src/codegen/ctx.rs)
* [src/codegen/emit.rs](../../src/codegen/emit.rs)
* [src/wasm/linker.rs](../../src/wasm/linker.rs)

That duplication is the root cause.

---

## Confirmed Failure Modes

These are all instances of the same representation-boundary bug.

### 1. Rebound iterator locals

Example shape:

```tw
it := Iterator.unfold(0, ...)
it = Iterator.unfold(true, ...)
for x in it { ... }
```

Observed failure:

* local setup preallocated `Iterator.next` / `match` temporaries using the first iterator shape
* flow-sensitive emission later followed the reassigned iterator shape
* generated WAT contained unresolved or invalid typed iterator-adjacent locals

This was fixed locally, but the bug was a symptom of the broader policy split.

### 2. Typed closure trampoline result mismatch

Example shape:

```tw
fn mk(n: Int) Iterator<Int> { ... }
fn apply(f: fn(Int) Iterator<Int>) Int { ... }
apply(mk)
```

Observed failure:

* the user function returned typed `iter_state__...`
* the typed closure trampoline or typed closure func type still described the result as
  erased `$rt_types__IterState`
* Wasm validation failed on the closure/trampoline boundary

Again, the root issue was not one local bug; it was that multiple ABI-emitting paths were
making representation decisions independently.

---

## Root Cause

The backend currently conflates three distinct concerns:

1. semantic type
2. local storage representation
3. cross-boundary ABI representation

`MonoType` is being used as the starting point for all three, but iterator-related codegen
now needs stronger distinctions:

* a value may have semantic type `Iterator<Int>`
* it may be stored locally as typed `iter_state__Int__Int`
* but it may still need to cross a function or closure boundary as erased
  `$rt_types__IterState`

Because those decisions are not centralized, the backend currently relies on scattered
conditionals such as:

* “if `IteratorStateInfo` is available, use typed state here”
* “if this is a concrete closure signature, emit typed closure type there”
* “if this helper is specialized, emit typed payload structs here”

Each local change is reasonable in isolation, but the combined effect is unstable because
the surrounding ABI paths are not forced to agree.

---

## Design Direction

Prefer an explicit **universal ABI with local specialization**:

* function params/results use erased runtime types by default
* closure func types and closure trampolines use the same erased ABI by default
* specialization is allowed inside function bodies and dedicated helpers
* conversions between erased and typed forms happen only at explicit boundary helpers

Why this direction:

* it matches the current runtime model
* it reduces linker and trampoline complexity
* it preserves the important typed fast paths for `Iterator.next`, `for`, and `match`
* it avoids continuing to grow a fragile “partially specialized ABI” system

This does **not** reject specialization. It narrows where specialization is allowed until
the backend has a single shared ABI/signature source.

---

## Proposed Architecture

### 1. Introduce explicit backend representation metadata

Keep `MonoType` for semantic typing only.

Add a backend layer that answers questions such as:

* what is the storage representation of this local?
* what is the ABI representation of this function result?
* does this expression produce a typed iterator fast-path value or an erased one?

Possible shape:

```text
enum ValueRepr {
  Erased,
  TypedClosure { params, ret },
  TypedCell { elem_ty },
  TypedIterator(IteratorStateInfo),
  TypedIterOption(IteratorStateInfo),
  TypedIterItem(IteratorStateInfo),
  TypedUnfoldStep { yield_ty, seed_ty },
}
```

Or, if a more general structure is preferred:

```text
struct BackendValueInfo {
  mono: MonoType,
  repr: ValueRepr,
}
```

**Current state:** `ValueRepr` exists with `TypedClosure` and `TypedCell` variants only.
Iterator-specific variants (`TypedIterator`, `TypedIterOption`, etc.) were not added to
`ValueRepr`. Instead, iterator state info lives as three separate optional fields on
`LocalBackendInfo`:

```text
struct LocalBackendInfo {
  repr: Option<ValueRepr>,              // TypedClosure | TypedCell only
  iterator_state: Option<IteratorStateInfo>,
  iterator_next_state: Option<IteratorStateInfo>,
  iter_item_state: Option<IteratorStateInfo>,
}
```

This is a pragmatic split — closures/cells use `ValueRepr`, iterators use dedicated fields —
but it means the repr model is not fully unified. Folding iterator state into `ValueRepr`
remains an option if the two-channel layout becomes a maintenance burden.

Even if the first rollout only changes iterator boundaries, this repr layer should account for
the other specialization families already present in the backend:

* typed closures
* typed cells

Otherwise `local_mono` plus closure/cell-specific helpers remain a parallel source of truth,
and the refactor only moves the iterator-specific duplication rather than removing the
underlying backend split.

### 2. Separate local storage repr from ABI repr

The backend should be able to say:

* local `it` stores typed `iter_state__Int__Int`
* function result ABI is still erased `$rt_types__IterState`

This separation is currently implicit and fragile. It should become explicit helpers:

* `local_repr(local_id)`
* `expr_repr(expr/op)`
* `func_result_abi(func_id)`
* `func_param_abi(func_id, idx)`

### 3. Centralize function ABI decisions

Build one helper as the only source of truth for user-function ABI:

```text
user_func_abi(func_id) -> {
  params: Vec<ValType>,
  results: Vec<ValType>,
  result_repr: ValueRepr,
}
```

Everything that currently emits or depends on function signatures should consume that:

* function stub emission
* direct user call emission
* dynamic closure-call lowering
* typed closure func type emission
* universal closure trampoline emission
* typed closure trampoline emission

This is the single most important refactor step. The typed closure trampoline bug and the
typed closure func type bug are both consequences of not having this.

### 4. Centralize typed <-> erased conversions

Iterator-related conversions should not be hand-written ad hoc at every boundary.

Introduce explicit helpers for:

* typed iterator state -> erased iterator state
* erased iterator state -> typed iterator state
* typed `UnfoldStep<Y, S>` -> erased `Variant`
* erased `Variant` -> typed `UnfoldStep<Y, S>`
* typed `IterItem` / `Option<IterItem>` -> erased fallback form
* erased fallback form -> typed iterator-adjacent structs

Once these helpers exist, call boundaries should use them instead of directly emitting:

* `StructNew`
* `RefCast`
* payload-array boxing

### 5. Restrict typed specialization to local fast paths first

Until ABI is fully centralized, typed iterator representations should only appear in:

* locals proven flow-concretely typed
* specialized iterator helpers
* concrete `match` lowering for `Iterator.next`
* `for` lowering on typed iterator results
* typed `IterItem` record field access

They should **not** silently become public ABI for:

* user function params/results
* closure func types
* closure trampoline results
* generic function-value signatures

unless the centralized ABI layer explicitly opts into that policy.

### 6. Replace ad hoc iterator state maps with one flow environment

The current growth in separate maps is a design smell, and it already sits beside a separate
`local_mono` channel that also influences representation choices.

The iterator-specific maps are:

* `local_iterator_states`
* `local_iterator_next_states`
* `local_iter_item_states`

Replace them with one flow environment carrying backend info per local, for example:

```text
HashMap<LocalId, LocalBackendInfo>
```

This should track:

* semantic mono type
* current storage repr
* specialization metadata when relevant (`IteratorStateInfo`, typed closure sig, typed cell elem)

That reduces the risk that one map is updated/restored while another is forgotten, and gives the
backend one place to answer “what representation does this local currently have?”

### 7. Split inference from specialized type registration

`EmitCtx` currently does both:

* infer concrete iterator-adjacent shapes
* accumulate requested specialized Wasm types

Those roles should be separated:

* inference / repr selection
* type registry / emission bookkeeping

This makes it easier to reason about whether a value is typed because of backend policy or
only because some specialized type happened to be registered.

---

## Implementation Strategy

### Phase 1. Introduce shared ABI helpers without changing policy

Create the shared ABI helper layer first, but do **not** require every existing emitter to share
one helper until they are parameterized by the same boundary policy.

Concretely: the current backend already has real disagreement on iterator-returning functions
between user-function result selection and typed closure/type emission. So “shared helper without
changing policy” is only realistic if Phase 1 first introduces an explicit boundary-policy input
or a precomputed ABI table that those paths can all read from.

Deliverables:

* shared ABI data model for params/results plus repr metadata
* one boundary-policy input or precomputed ABI table per user function
* adapter helpers for user funcs, closure func types, and closure trampolines to read that model

Success condition:

* all signature-emitting paths that are meant to agree read from the same ABI source
* any intentionally different policy is explicit at the call site, not hidden in ad hoc
  `mono_to_valtype*` selection

### Phase 2. Make closure paths consume the shared ABI

Move these paths onto the centralized ABI helper:

* user func stubs
* direct user calls
* dynamic closure-call lowering
* universal closure trampoline
* typed closure func type emission
* typed closure trampoline emission

Success condition:

* iterator-returning function values do not produce signature mismatches
* buggy cases are allowed to change emitted signatures here if that is what is required to make
  the boundary policy coherent

### Phase 3. Move iterator conversions behind boundary helpers

Replace ad hoc iterator conversion logic with named helpers.

Success condition:

* the only places where typed iterator structs cross erased boundaries are the dedicated
  conversion helpers

### Phase 4. Narrow specialization to local/body fast paths

Once boundary helpers exist, explicitly forbid typed iterator-adjacent ABI leakage except
where the shared ABI layer permits it.

Success condition:

* local typed fast paths still work
* closure and function boundaries become stable

### Phase 5. Re-evaluate full specialized ABI later

If end-to-end specialized ABI is still desired, build it as a second step on top of the
shared ABI infrastructure, not as scattered local overrides.

Success condition:

* specialized ABI becomes an explicit backend mode, not an emergent side effect

### Phase 6. Unify typed closures/cells in the same repr layer

Once iterator boundaries are stable, fold typed closure and typed cell specialization into the
same backend representation model (`ValueRepr` / `LocalBackendInfo`) rather than leaving them as
parallel policy channels.

Deliverables:

* represent closure/cell specialization metadata in the same local/backend repr structure used by
  iterator-adjacent values
* route closure/cell local-slot decisions through shared local repr helpers rather than ad hoc
  `local_mono` + subsystem-specific checks
* keep ABI boundary policy explicit: default erased ABI for closures/cells unless a dedicated mode
  explicitly opts into typed ABI
* ensure specialized type registration for closures/cells is driven from the same repr/registry
  pipeline used by iterator-adjacent types

Success condition:

* adding or modifying typed closure/cell specialization touches one shared repr/ABI policy layer
  instead of multiple disconnected emit/inference paths
* closure/cell optimization remains available for local fast paths without silently changing
  public function/closure ABI
* no new representation-boundary mismatch class is introduced when adding future specialized types

---

## Checklist

### Immediate stabilization

- [x] Add a shared `user_func_abi` helper and route function stub emission through it
- [x] Route direct-call lowering through the same ABI helper
- [x] Route dynamic closure-call lowering through the same ABI helper
- [x] Route typed closure func type emission through the same ABI helper
- [x] Route universal closure trampoline emission through the same ABI helper
- [x] Route typed closure trampoline emission through the same ABI helper
- [x] Route typed tail-closure-call lowering through the same ABI helper if applicable

### Iterator boundary helpers

- [x] Add helper: typed iterator state -> erased iterator state
- [x] Add helper: erased iterator state -> typed iterator state
- [x] Add helper: typed `UnfoldStep` -> erased `Variant`
- [x] Add helper: erased `Variant` -> typed `UnfoldStep`
- [x] Add helper: typed iterator option/item -> erased fallback form
- [x] Add helper: erased fallback form -> typed iterator option/item

### Flow / local representation cleanup

- [x] Introduce a single local backend info structure
- [x] Replace `local_iterator_states`, `local_iterator_next_states`, and
      `local_iter_item_states` with the unified flow environment
- [x] Make local-slot allocation consume backend repr info rather than ad hoc checks
- [x] Make flow restoration/update logic operate on one unified structure

### Type registry cleanup

- [x] Separate specialized-type registration from inference state
- [x] Keep specialized type emission registry deterministic and independent of flow updates
- [x] Decide whether typed closures / typed cells are represented in the same repr metadata layer
      or explicitly left out of scope for this refactor (tracked by Phase 6 below)
      **Decision:** closures/cells use `ValueRepr`; iterator state uses dedicated
      `LocalBackendInfo` fields. Not unified yet — acceptable for now, revisit if the
      two-channel layout causes maintenance issues.
- [x] Audit linker rewrites for all places where specialized types can appear

### Regression coverage

- [x] Function returning `Iterator<T>` used as first-class value
- [x] First-class function returning `Iterator<T>` called through closure dispatch agrees with
      closure-call lowering, closurefunc type, and both trampoline paths
- [x] Reassignment across different iterator shapes
- [x] Typed iterator crossing erased function parameter boundary
- [x] Closure returning iterator across both typed and universal trampolines
- [x] Typed `UnfoldStep` producer consumed through erased path
- [x] Linker rewrite coverage for `if` / `block` / `loop` result types carrying specialized refs

### Closure/Cell Unification (Phase 6)

- [x] Add explicit `ValueRepr` variants (or equivalent) for typed closure and typed cell metadata
      in the shared backend repr model
- [x] Make closure/cell local representation lookup use shared local repr helpers
- [x] Remove closure/cell-specific fallback representation decisions that bypass shared repr policy
      (local closure/cell call-site typed fast paths no longer fall back to `local_mono` when
      shared repr metadata is absent)
- [x] Add shared ABI policy hooks for closure/cell boundaries (default erased, explicit typed mode)
      **Implemented:** `mono_to_valtype_for_user_abi_result` forces iterator-adjacent types to
      erased ABI; closures/cells use typed ABI via `mono_to_valtype_specialized`. Documented by
      `abi_boundary_policy_iterator_erased_closure_cell_typed` regression test.
- [x] Route closure/cell specialized-type registration through the unified registry/repr pipeline
      **Implemented:** `SpecializedTypeRegistry` now has `typed_closures` and `typed_cells`
      fields. `emit_user_module_typed` populates the registry via `request_typed_closure` /
      `request_typed_cell`, then emits from `requested_typed_closures` / `requested_typed_cells`.
- [x] Add regression: typed closure param and dispatch agree (monomorphized closure uses typed
      param + typed `call_ref`, no mixed erased/typed boundary)
- [x] Add regression: universal closure trampoline uses erased ABI for closure results
- [x] Add regression: specialized type names are properly module-qualified after linking

---

## Near-Term Exit Criteria

This plan is successful when:

* iterator specialization remains active inside local fast paths
* function and closure boundaries no longer fail because one side chose typed and the
  other side chose erased
* adding a new typed iterator optimization requires changing one shared ABI/repr layer,
  not multiple unrelated code paths

At that point, iterator specialization becomes a controlled optimization rather than a
source of recurring backend inconsistency.
