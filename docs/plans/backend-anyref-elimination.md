# Backend `anyref` Elimination Plan

## Goal

Eliminate non-essential `anyref` from the Twinkle Wasm backend.

## Plan Role

This is the longer-term architecture plan for reducing and eventually removing
most erased backend/runtime boundaries.

It should not be the first response to current self-hosted validation failures.
For the active cleanup sequence:

1. [boot-selfhosted-wasm-repr-parity.md](boot-selfhosted-wasm-repr-parity.md)
   tracks the current self-hosting representation blockers
2. [boot-backend-physical-typing.md](boot-backend-physical-typing.md)
   stabilizes the current backend by making erased-boundary adaptation explicit
   and verifiable
3. this plan then shrinks or removes those erased surfaces by introducing typed
   helper/container families and a stricter representation policy

For concrete monomorphized programs, the backend should prefer concrete Wasm
layouts, concrete helper families, and concrete container families throughout.
`anyref` should stop being the default representation strategy and become a
temporary boundary tool used only where unavoidable.

The near-term target is:

* no incidental `anyref` in backend-internal representations of concrete values
* no universal `anyref` payload storage for concrete `Vector<T>` / `Dict<K, V>`
* no typed-vs-erased dual world for normal monomorphized code paths

The long-term target is stronger:

* remove `anyref` even from most runtime helper boundaries by replacing
  universal helper APIs with typed helper families
* leave `anyref` only for true external interoperability constraints, if any

## Why A Dedicated Plan

Today the direction is split across:

* monomorphization docs
* stage0 type-erasure-reduction docs
* self-hosting/backend design docs
* persistent `Vector` / `Dict` runtime plans

Those documents explain parts of the story, but they do not define a single
repository-level target for where `anyref` should remain and where it should be
eliminated. This plan is that source of truth.

## Problem Statement

Monomorphization gives the backend concrete `MonoType`s, but that does not by
itself guarantee concrete runtime representation.

The current stage0 and boot designs still contain several `anyref`-centric
patterns:

* shared runtime containers whose payload slots are `anyref`
* universal helper ABIs that force boxing/unboxing at container and helper
  boundaries
* representation splits where the same semantic type can exist in both typed
  and erased forms
* fallback paths that preserve older universal layouts even when the program is
  fully monomorphized

This creates three costs:

* performance cost: boxing, casts, payload arrays, universal dispatch
* complexity cost: boundary logic, helper duplication, representation tracking
* reliability cost: typed/erased mismatches are a recurring source of backend
  bugs

## Intended End State

For a concrete monomorphized type `T`, the backend should be able to answer:

* what is the concrete Wasm layout of `T`?
* what helper family operates on values of `T`?
* what container family stores `T` without boxing?

without consulting a parallel erased fallback model.

Examples:

* `Int` stays `i64`
* `Option<Int>` lowers to a typed sum layout
* `fn(Int) Int` lowers to a typed closure family
* `Cell<String>` lowers to a typed cell family
* `Vector<Int>` lowers to a typed vector family with direct `i64` element slots
* `Dict<String, Int>` lowers to a typed dict family with `ref $String` keys and
  `i64` values

The backend should not route these through universal `anyref` payload storage
unless the path is explicitly marked as an external/interoperability boundary.

## Design Position

### 1. Monomorphization Is A Prerequisite, Not The Finish Line

Monomorphization is responsible for making backend-facing IR concrete:

* no surviving type vars
* no unresolved metavariables
* concrete specialized functions and rewritten call sites

But eliminating `anyref` is primarily a backend/runtime task:

* backend layout policy decides how concrete types map to Wasm
* runtime/helper design decides whether concrete values remain concrete through
  helper/container operations
* boundary insertion decides where boxing is still necessary

### 2. `anyref` Is Not An Acceptable End-State Fallback For Concrete Code

The final architecture should not treat “erased to `anyref`” as the normal
backup plan for concrete monomorphized values.

That means:

* no permanent “typed path plus erased fallback path” for ordinary concrete code
* no permanent universal container payloads for concrete `Vector<T>` /
  `Dict<K, V>`
* no permanent need for runtime dispatch between typed and erased forms of the
  same value in normal backend-generated code

Temporary migration fallbacks are allowed while implementing the plan, but they
are not part of the intended end state.

### 3. External ABI Boundaries Are The Only Initially Accepted `anyref` Boundary

The only hard limitation in the current architecture is the existing
host/runtime ABI shape where imported/runtime functions already expose `anyref`
slots.

Even that is not sacred long-term. The plan should eventually shrink those
boundaries too by moving from universal helper APIs toward typed helper
families.

So the rule is:

* short term: `anyref` may remain at existing runtime ABI boundaries
* medium term: those boundaries should narrow
* long term: they should disappear where typed helper families can replace them

## Scope

This plan covers:

* stage0 backend representation strategy
* self-hosted backend representation strategy
* runtime type/helper family design as required by typed layouts
* container family specialization for `Vector<T>` and `Dict<K, V>`
* shrinking or replacing universal runtime helper APIs

This plan does not attempt to redesign the Twinkle surface language.

## Non-Goals

* one-shot removal of every `anyref` use in a single phase
* changing user-visible semantics of `Vector`, `Dict`, `Option`, `Result`, etc.
* preserving old universal helper shapes forever for compatibility convenience

## Current Gaps

### Stage0

Stage0 still retains major universal container and helper surfaces:

* `Vector<T>` is backed by shared runtime array storage
* `Dict<K, V>` uses shared dict storage with erased key/value slots
* helper/runtime boundaries still box and unbox concrete values through `anyref`
* some local/control-flow paths still widen to `anyref`

### Boot

Boot has the right conceptual split in progress:

* explicit `WrapAnyref` / `UnwrapAnyref` nodes
* centralized layout planning
* a stated goal of eliminating backend-internal erased fallback

But the currently checked-in design still keeps some universal/container-level
`anyref`:

* shared `Vector` / `Dict` container refs
* `WAnyref` layouts in some places
* runtime ABI assumptions that still expose `anyref` element/key/value slots

## Workstreams

### Workstream A: Representation Policy

Create a single representation policy for the backend:

* every concrete `MonoType` maps to exactly one concrete Wasm layout
* layout choice is not deferred to ad hoc emitter heuristics
* typed and erased forms of the same concrete value do not coexist by default

Deliverables:

* explicit repository-level rules for where `anyref` is allowed
* layout-function audit for stage0 and boot
* removal of “fallback to `anyref`” language from non-boundary code paths

### Workstream B: Typed Container Families

Replace universal container payload storage with per-instantiation families:

* `Vector<T>` gets container/node/tail families derived from `T`
* `Dict<K, V>` gets container/node/leaf families derived from `(K, V)`

Examples:

* `Vector<Int>` uses direct `i64` element slots
* `Vector<String>` uses direct `ref $String` element slots
* `Dict<String, Int>` uses `ref $String` keys and `i64` values

The persistent container plans become subplans of this workstream, not the full
story by themselves:

* [persistent-vector.md](persistent-vector.md)
* [persistent-dict.md](persistent-dict.md)

### Workstream C: Typed Helper Families

Container specialization is not enough if helper calls still force values back
through universal `anyref` APIs.

We also need typed helper families for hot concrete paths:

* vector get/set/push/concat/slice families
* dict get/has/set/remove families
* typed sum/iterator/closure/cell helpers where still missing

Goal:

* concrete code uses concrete helpers by default
* universal helper APIs become transitional or strictly external

### Workstream D: Boundary Shrinking

Keep explicit boundary nodes only where still necessary.

Short term:

* `WrapAnyref` / `UnwrapAnyref` remain at true runtime ABI crossings

Medium term:

* reduce the number of helper/import sites that still require them

Long term:

* universal boundary nodes are needed only for external host interoperability,
  if at all

### Workstream E: Runtime ABI Redesign

Audit current runtime/import surfaces and replace universal `anyref`-based APIs
with typed families where practical.

Examples:

* replace generic container element APIs with typed family APIs
* remove universal payload-array conventions where concrete layouts are known
* preserve one universal external layer only where host interop truly requires it

## Staging

### Phase 1: Declare The Policy

* land this plan
* align active docs to say fallback `anyref` is temporary, not the destination
* audit stage0 and boot for places still violating the intended policy

### Phase 2: Make Container Specialization A First-Class Backend Goal

* teach layout planning to name per-instantiation `Vector<T>` / `Dict<K, V>`
  families
* update container plans to assume typed families, not shared `anyref` payload
  storage
* define fallback criteria explicitly

### Phase 3: Deliver High-Value Families First

Prioritize hot concrete families:

* `Vector<Int>`
* `Vector<String>`
* `Dict<String, Int>`
* `Dict<String, String>`
* `Dict<Int, Int>`

### Phase 4: Shrink Universal Helper Surfaces

* move hot container/helper paths off universal `anyref` APIs
* keep universal APIs only where still required during migration

### Phase 5: Remove Transitional Fallbacks

* delete temporary erased fallback paths once specialized families cover the
  supported monomorphized cases
* verify no ordinary concrete backend paths depend on `anyref`

## Success Criteria

This plan is successful when all of the following are true:

* concrete monomorphized values no longer routinely pass through `anyref`
  internally
* `Vector<T>` and `Dict<K, V>` have per-instantiation container families for
  supported concrete types
* boxing/unboxing at container element boundaries is gone for those supported
  families
* typed/erased dual-representation logic is no longer necessary for ordinary
  concrete code paths
* remaining `anyref` sites are explicitly documented as external ABI or
  transitional migration cases

## Risks

* code size growth from over-specialization
* runtime/helper-family explosion if specialization policy is too broad too early
* migration complexity while stage0 and boot coexist
* accidental retention of “temporary” fallback paths unless removal is an
  explicit phase goal

## Open Questions

* Which container/helper families should be mandatory for the first typed pass?
* Should boot and stage0 converge on the same naming scheme for specialized
  container/runtime families?
* How aggressively should runtime ABI typing be pursued before self-hosting is
  complete?

## Related Docs

* [archive/self-hosting.md](archive/self-hosting.md)
* [boot-codegen.md](boot-codegen.md)
* [persistent-vector.md](persistent-vector.md)
* [persistent-dict.md](persistent-dict.md)
* [archive/wasm-type-erasure-reduction.md](archive/wasm-type-erasure-reduction.md)
* [../internals/monomorphization.md](../internals/monomorphization.md)
