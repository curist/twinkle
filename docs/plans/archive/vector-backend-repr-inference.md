# Vector Backend Representation Inference

Last updated: 2026-03-29

## Status

Completed on 2026-03-29.

Landed scope:

- separate backend vector physical-repr tracking from semantic `MonoType`
- preserve typed `Vector<Int>` repr through local/result inference
- preserve the typed family through builder flow
- flip stage0/runtime `Vector<Int>` to a typed container/ref family
- update the runtime snapshot to the new typed-vector `rt.arr` surface

Remaining work belongs to the parent kickoff rather than this archived slice:

- `boot/lib` integration and ownership follow-through
- broader Twinkle-authored persistent vector rollout
- extension to other vector families

One implementation nuance remains intentional: emitter dispatch is repr-driven
for the implemented `Vector<Int>` paths, but a semantic fallback still exists
where repr metadata is unavailable. That does not block this plan's stage0
slice from being considered complete.

## Goal

Teach the stage0 backend to infer and preserve the physical runtime
representation of `Vector<T>` values separately from their semantic `MonoType`,
as the backend-substrate slice of the Twinkle-authored vector kickoff, so
concrete `Vector<Int>` code can move from typed helper-family routing to a real
typed container family without ad hoc emitter checks.

This plan is subordinate to:

- [backend-anyref-elimination.md](../backend-anyref-elimination.md)
- [persistent-vector.md](../persistent-vector.md)
- [twinkle-vector-kickoff.md](twinkle-vector-kickoff.md)

If this document disagrees with those plans, follow them in that order.

This document is not a standalone delivery track. It covers the stage0 backend
representation-inference work that sits inside the kickoff plan's early
milestones and must preserve that plan's `boot/lib` ownership model.

## Why This Plan Exists

The current kickoff work can already route concrete `Vector<Int>` operations to
typed helper families, but stage0 still infers most vector values as the
universal `rt_types__Array` container.

The kickoff plan already fixes the broader ownership and sequencing:

- `boot/lib` owns the persistent vector algorithm
- stage0 is the first integration environment
- the stage0-to-`boot/lib` consumption path must be explicit before typed-family
  ABI work proceeds

This document exists to isolate the missing backend representation-inference
piece inside that larger kickoff, not to defer or replace the kickoff's
integration constraint.

That is enough for:

- typed helper ABI selection
- targeted runtime import selection
- early no-boxing checks on helper boundaries

That is **not** enough for:

- switching `Vector<Int>` locals/results to a distinct typed vector ref
- making builder flow preserve typed vector families end-to-end
- replacing ad hoc `is this a Vector<Int>?` emitter checks with a general
  backend policy
- landing a real typed container family without representational drift

The missing piece is explicit backend representation inference for vectors.

## Problem Statement

Today stage0 mostly answers:

- semantic type: `Vector<Int>`

But it does not reliably answer:

- physical runtime repr: erased `rt_types__Array` or typed `Vector_i64` family?

That gap shows up in several places:

- `mono_to_valtype` maps every `MonoType::Vector(_)` to the universal array ref
- builder flow (`builder_from`, `builder_push`, `builder_freeze`) does not carry
  explicit typed-vector representation metadata
- local and call-result inference can preserve semantic `Vector<Int>` while
  still losing the intended runtime family
- the emitter currently needs special-case checks to pick typed helper families

As long as those stay coupled, the backend cannot safely flip concrete
`Vector<Int>` values to a distinct typed container ref.

## Design Position

### 1. Semantic Type And Physical Repr Must Be Separate

`MonoType::Vector(Box::new(MonoType::Int))` is the semantic type.

The backend also needs a second answer:

- erased vector repr
- typed vector family repr for `Int`
- later, typed vector family repr for other concrete element layouts

The emitter and local typing logic should consult that physical repr directly,
not reconstruct it from ad hoc pattern checks.

### 2. Typed Helper Selection Is Not Sufficient

Selecting `rt_arr__*_i64` helpers is only a first step.

The backend should eventually be able to say all of the following for concrete
`Vector<Int>` code:

- local storage uses the typed vector ref
- call results use the typed vector ref
- builder flow stays in the same typed family
- safe intrinsics and raw helper paths agree on the same family

### 3. Builder Flow Is The Critical Path

The most important inference boundary is not `Vector.make`; it is builder flow:

- `VECTOR_BUILDER_NEW`
- `VECTOR_BUILDER_FROM`
- `VECTOR_BUILDER_PUSH`
- `VECTOR_BUILDER_FREEZE`

If builder inference loses the typed family, collect/lowered loop paths will
fall back to erased assumptions even when direct vector operations are typed.

### 4. Do Not Hide Repr Choice Inside Emitter Heuristics

The end state should not depend on scattered checks like “if the atom is
`Vector<Int>` then call this helper”.

Instead:

- repr inference decides the runtime family
- valtype selection follows that repr
- emission dispatches from that repr

That keeps the policy auditable and makes later boot mirroring tractable.

## Scope

In scope:

- stage0 vector repr metadata and inference
- local/call-result/backend valtype plumbing for typed vector families
- builder-flow representation tracking
- replacing ad hoc vector-family emitter checks with repr-driven dispatch
- enough runtime type naming to let `Vector<Int>` use a distinct backend family
- the backend-side assumptions needed to stay compatible with the kickoff
  plan's explicit stage0-to-`boot/lib` consumption path

Out of scope:

- redesigning the persistent vector algorithm itself
- redesigning the public `Vector` API
- fully removing erased fallback for unsupported vector families
- dict/HAMT repr inference in the same change

This plan may still require adjacent `boot/lib` or integration-wiring updates
when the chosen kickoff consumption path makes them necessary. What remains out
of scope here is specifying the algorithm internals as a separate design track.

## Current State

Current stage0 behavior is split:

- helper-family selection for some concrete `Vector<Int>` paths can already use
  typed `*_i64` runtime symbols
- semantic inference still treats vectors as `MonoType::Vector(T)`
- physical valtype inference still defaults `Vector<T>` to `rt_types__Array`
- builder results do not yet preserve an explicit typed vector family repr

This means the backend can route helper calls, but it cannot yet safely switch
the runtime container ref for concrete vectors.

## Target State

For a concrete vector value, stage0 should infer all of the following together:

- semantic type: `Vector<Int>`
- physical repr: typed vector family for `Int`
- Wasm valtype: ref to the typed vector family
- helper family: `Vector_i64` helper set

That inference should survive:

- literals
- `Vector.make`
- `Vector.push`
- `Vector.set_unsafe`
- `Vector.concat`
- `Vector.slice`
- indexing / `Vector.get`
- builder-new/from/push/freeze flow
- safe `Vector.set`

Unsupported or not-yet-specialized families may still use erased fallback
during migration.

## Proposed Backend Shape

### Kickoff Integration Constraint

This backend work must be executed under the kickoff plan's ownership rule:

- `boot/lib` remains the source of truth for the persistent vector algorithm
- stage0 repr/container/helper changes here are substrate and integration glue,
  not a parallel Rust-owned vector implementation
- any typed container/helper flip described below must be compatible with the
  explicit stage0-to-`boot/lib` consumption path chosen by kickoff Milestone 0

So this document can refine backend representation policy, but it cannot be used
to justify advancing typed vector-family runtime work as an independent track
that postpones the `boot/lib` integration decision.

### Repr Metadata

Add explicit vector physical representation metadata in stage0 codegen, parallel
to existing typed-sum handling.

Expected shape:

- a new vector repr enum or `ValueRepr::TypedVector { elem_ty }`
- helpers to answer whether a local/atom/result lives in a typed vector family
- symbol helpers for the vector family name derived from concrete element type

### Valtype Mapping

`mono_to_valtype_specialized` should stop treating all vectors identically.

For concrete supported families:

- `Vector<Int>` -> typed vector family ref

For unsupported or erased cases:

- `Vector<T>` -> `rt_types__Array`

### Call/Builder Result Inference

Call-result inference must preserve vector repr through:

- runtime vector helpers
- intrinsic `Vector.make/get/set/push`
- builder calls, especially `builder_freeze`

This is the backend step that lets later container-family flips stay coherent.

### Emission

Once repr metadata exists, emission should dispatch from that representation.

That affects:

- array/vector literals
- vector indexing
- vector safe intrinsics
- runtime prelude calls
- builder fast paths

The emitter should not need to recover family choice by peeking at semantic
types alone.

## Milestones

### Milestone 0: Kickoff Dependency

Confirm the prerequisite sequencing inherited from
`twinkle-vector-kickoff.md`.

Tasks:

- treat kickoff Milestone 0's stage0-to-`boot/lib` consumption-path decision as
  a prerequisite for repr work that changes typed-family ABI/runtime ownership
- state the ownership boundary for any Rust-side changes this plan introduces
- scope the repr work as stage0 substrate for the kickoff rather than a separate
  implementation track

Acceptance:

- this plan's typed-family work is explicitly tied to the kickoff's
  stage0-to-`boot/lib` integration path
- no milestone in this document implies a parallel long-term Rust-owned vector
  implementation

### Milestone 1: Repr Metadata In Stage0

Introduce explicit vector physical repr metadata in codegen context/state.

Tasks:

- add a vector repr concept alongside existing typed closure/cell/sum metadata
- add family symbol helpers for typed vectors
- define which concrete vector families are recognized in this phase
- keep the chosen kickoff ownership/integration path explicit in the
  representation metadata and naming scheme

Acceptance:

- stage0 can represent “semantic `Vector<Int>`, physical typed vector family”
  explicitly in backend metadata

### Milestone 2: Local And Call-Result Inference

Teach local/call-result inference to preserve vector repr instead of collapsing
all vectors to erased array refs.

Tasks:

- thread vector repr through let-binding inference
- teach call-result inference for vector helpers/intrinsics
- keep erased fallback for unsupported vector families
- preserve compatibility with the kickoff's public-intrinsic entry points and
  `boot/lib`-owned algorithm boundary

Acceptance:

- concrete `Vector<Int>` locals/results can infer a typed vector-family repr
- generic/unsupported vectors still compile via erased fallback

### Milestone 3: Builder Repr Preservation

Make builder flow preserve the typed vector family.

Tasks:

- define backend repr behavior for `builder_new`, `builder_from`,
  `builder_push`, and `builder_freeze`
- ensure collect/loop-rewrite paths do not lose the typed vector family
- keep alias-safety semantics unchanged
- keep builder repr policy compatible with the kickoff's `boot/lib`-owned
  builder semantics

Acceptance:

- `collect` and loop-builder rewrites for `Vector<Int>` preserve the same typed
  vector-family repr through freeze

### Milestone 4: Repr-Driven Emitter Dispatch

Replace ad hoc vector-family checks in the emitter with repr-driven dispatch.

Tasks:

- route vector literals/indexing/intrinsics/runtime calls from vector repr
- remove temporary “is this `Vector<Int>`?” dispatch where the repr now answers
  the question
- keep emitted code unchanged for unsupported/erased families
- preserve the kickoff model where public `Vector.*` entry points stay intrinsic
  while their concrete path targets typed family substrate/helpers

Acceptance:

- typed helper/container selection for `Vector<Int>` is driven by backend repr
  metadata rather than local emitter heuristics

### Milestone 5: Typed Container Flip

After repr inference is stable, switch concrete `Vector<Int>` values to the
typed vector-family ref/container.

Tasks:

- add the typed vector family type(s)
- update runtime/helper signatures to return the typed vector ref
- update stage0 valtype mappings and coercions to match
- land the container/ref flip in a way that remains compatible with the chosen
  stage0-to-`boot/lib` integration path rather than introducing a parallel
  Rust-owned algorithm track

Acceptance:

- concrete `Vector<Int>` values use a distinct typed vector-family ref in
  stage0
- helper-family selection, local inference, and builder flow all agree on that
  representation

## File Checklist

Expected primary stage0 touch points:

- `src/codegen/ctx.rs`
- `src/codegen/emit.rs`
- `src/runtime/types.rs`
- `src/runtime/arr.rs`
- targeted tests covering codegen/runtime/optimizer behavior

Possible adjacent touch points under the kickoff plan:

- intrinsic ABI/result metadata if typed vector-family refs become visible there
- `boot/lib` integration wiring or module stubs required by the chosen
  consumption path
- boot mirrors after stage0 behavior is proven

## Validation

Behavioral validation:

- existing vector runtime tests still pass
- existing optimizer tests still pass
- collect/loop-builder paths still preserve semantics

Representation validation:

- concrete `Vector<Int>` locals/results infer the typed vector family where
  supported
- builder-driven `Vector<Int>` paths preserve typed family choice through
  freeze
- emitted user WAT uses the typed helper family because repr inference chose it,
  not because of ad hoc emitter-only checks

Migration validation:

- unsupported vector families still compile
- erased fallback remains limited to unsupported/non-specialized families

## Exit Criteria

This plan is complete when all are true:

1. Stage0 tracks vector physical repr separately from semantic `MonoType`.
2. Concrete `Vector<Int>` locals/results preserve a typed vector-family repr.
3. Builder flow preserves the typed vector family for `Vector<Int>`.
4. The emitter dispatches vector lowering from backend repr metadata rather than
   temporary semantic-type checks.
5. Stage0 can safely switch `Vector<Int>` to a real typed container ref without
   representational drift between helper selection, local typing, and builder
   flow.
6. The work remains aligned with the kickoff plan's explicit `boot/lib`
   ownership and stage0 integration path.

## Follow-On Work

After this plan lands:

1. Complete any remaining `Vector<Int>` runtime/layout work that sits beyond
   this repr-inference slice.
2. Continue the kickoff by wiring or extending the Twinkle-authored
   `Vector<Int>` implementation through the chosen `boot/lib` consumption path.
3. Extend the same repr-inference model to other vector families.
4. Reuse the same backend policy for dict/HAMT specialization.
