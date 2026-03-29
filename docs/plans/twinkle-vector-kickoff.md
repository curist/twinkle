# Twinkle-Authored Persistent Vector Kickoff

## Goal

Kick off the persistent vector work with a first implementation slice that keeps
the vector algorithm authored in Twinkle while integrating it through the stage0
Wasm backend first.

This plan is a delivery subplan of:

- [backend-anyref-elimination.md](backend-anyref-elimination.md)
- [persistent-vector.md](persistent-vector.md)

If this document disagrees with either parent plan, follow
`backend-anyref-elimination.md` first, then `persistent-vector.md`.

## Why This Plan Exists

The broader vector and backend plans define the target architecture, but they do
not define the first landing slice.

We need a concrete kickoff plan because there are two separate concerns:

- **backend substrate**: typed family naming, layout selection, helper ABI
  selection, optimizer compatibility
- **collection logic**: the persistent vector algorithm itself

The desired end state is:

- persistent vector logic remains on the Twinkle side
- stage0 is used first to prove the backend substrate and ABI shape
- boot follows once the substrate is stable

For this kickoff, "on the Twinkle side" has two layers:

- ordinary Twinkle in `boot/lib` for semantic vector logic
- compiler-owned Twinkle Wasm IR for low-level runtime substrate where needed

This avoids rediscovering representation problems in boot while still keeping
the long-term library ownership in Twinkle.

## Design Position

### 1. One Generic Design, Typed Instantiations

At the abstraction level, vector should be one generic persistent data
structure design.

At the backend/runtime level, monomorphization, lowering, and codegen should
materialize per-type container/helper families.

Allowed:

- one generic algorithm
- one shared set of invariants
- one schema for container/node/tail/builder/helper families

Not allowed as the end state:

- one universal runtime `Vector` container with `anyref` payload slots
- one universal helper ABI for hot concrete paths
- ordinary concrete code flowing through erased container boundaries by default

### 2. Twinkle Owns The Library Logic

The persistent vector algorithm should be authored in Twinkle source, not live
permanently as a Rust-only runtime implementation.

Stage0 remains responsible for:

- layout and type-family naming
- Wasm runtime ABI/import surface
- helper-family selection during codegen
- optimizer-only in-place ABI hooks

The Twinkle library layer remains responsible for:

- vector algorithm semantics
- structural sharing invariants
- builder semantics at the source/runtime-library layer

Low-level runtime substrate may still be authored separately in a compiler-owned
Wasm IR layer. That is compatible with self-hosting, but it does not replace
the requirement that semantic vector behavior live in ordinary `boot/lib`
Twinkle.

### 3. Stage0 Is The First Integration Environment

The first landing slice should be integrated into stage0 before boot.

Why:

- stage0 is faster to debug
- stage0 has the existing regression surface for runtime/codegen/optimizer work
- boot should inherit a proven family schema rather than become the discovery
  environment for it

This is an integration-order decision, not a statement about long-term
ownership.

## Scope

In scope:

- first typed vector family substrate in stage0 Wasm backend
- first Twinkle-authored persistent vector implementation slice
- end-to-end wiring for one concrete family
- preserving current optimizer and lowering contracts
- proving the no-boxing path for the first family

Out of scope:

- full dict/HAMT work
- full boot parity in the same change
- all vector families in the first pass
- redesigning the user-visible `Vector` API
- deleting every temporary fallback path before the first family lands

## First Target

The first concrete family is:

- `Vector<Int>`

Reasons:

- simplest typed payload (`i64`)
- highest-value proof that element boxing can be removed
- exercises helper selection, builder flow, and uniqueness rewrites without the
  extra complexity of string hashing or ref-element edge cases

`Vector<String>` is the next family after `Vector<Int>`, not part of the first
landing slice.

## Deliverable Shape

The first slice should produce all of the following:

1. A generic vector-family schema in the backend.
2. A concrete stage0-instantiated `Vector<Int>` family.
3. Twinkle-authored vector logic targeting that family shape.
4. Codegen that selects the `Vector<Int>` helper/container family for concrete
   monomorphized `Vector<Int>` values.
5. Validation that the hot `Vector<Int>` path no longer boxes elements through
   `anyref`.

## Architecture

### Backend Substrate

Stage0 needs a reusable typed-family substrate for vectors:

- family naming derived from element `MonoType`
- dedicated runtime/container/node/tail/builder type names
- typed helper names for `make`, `get`, `len`, `set`, `concat`, `slice`,
  `push`, and builder ops
- codegen selection rules that map concrete `Vector<T>` to the correct family

This substrate may be implemented partly in Rust during stage0 bring-up and
partly in Twinkle-authored Wasm IR on the boot side. The important boundary is
that it stays substrate rather than becoming the semantic owner of vectors.

Relevant stage0 files for the first slice:

- `src/runtime/types.rs`
- `src/runtime/arr.rs`
- `src/codegen/prelude.rs`
- `src/codegen/ctx.rs`
- `src/codegen/emit.rs`

### Twinkle Library Layer

The persistent vector algorithm itself should live in Twinkle-authored source in
`boot/lib`.

The implementation must satisfy all of the following:

- the algorithm is expressed in Twinkle source under `boot/lib`, rather than
  permanently embedded in Rust runtime code
- the Twinkle implementation targets the typed family substrate above
- public `Vector` semantics remain unchanged

The algorithm should not be forced down into the Wasm IR layer just because the
low-level helpers are there. Twinkle-authored Wasm IR is the right tool for raw
runtime primitives; `boot/lib` remains the right tool for persistent vector
semantics.

Planned home:

- `boot/lib` internal collection/runtime module(s)

Stage0 should integrate against that Twinkle-authored library rather than grow a
parallel long-term Rust-only vector implementation.

This remains a compiler-internal implementation detail rather than public
stdlib/prelude API surface:

- `boot/lib` is the right home for compiler-owned Twinkle libraries
- public `Vector.*` surface can stay where it is today during the kickoff
- boot compiler/runtime mirrors can adopt the same source of truth later

### Current Contracts That Must Stay Stable

The first slice must preserve:

- lowering contract for raw vector update helpers
- public `Vector` entry-point ownership:
  - `Vector.push`
  - `Vector.get`
  - `Vector.set`
  - `Vector.make`
  remain stage0 intrinsics during the kickoff slice
  - their concrete `Vector<Int>` implementations are retargeted to the typed
    family substrate and Twinkle-authored library-backed helper paths
- builder contract:
  - `VECTOR_BUILDER_NEW`
  - `VECTOR_BUILDER_FROM`
  - `VECTOR_BUILDER_PUSH`
  - `VECTOR_BUILDER_FREEZE`
- uniqueness rewrite contract for optimizer-only in-place hooks
- existing user-visible `Vector` semantics

Special care point:

- `builder_from` alias safety must remain intact. Seeding a builder from an
  existing persistent vector must not mutate shared structure unless uniqueness
  has already been proved.

### Kickoff Call Path Decision

The kickoff does **not** rehome the public `Vector.*` API out of intrinsic
dispatch.

For the first slice:

- the public surface stays where it is today: stage0 intrinsic/builtin entry
  points
- `boot/lib` owns the Twinkle-authored persistent vector logic behind that
  surface
- stage0 codegen/intrinsic lowering is responsible for routing concrete
  `Vector<Int>` operations to the typed family substrate and Twinkle-authored
  library-backed helpers

This keeps the public surface stable while still moving the vector
implementation itself onto the Twinkle side.

### Required Integration Constraint

The kickoff is not allowed to drift into a parallel Rust-owned vector
implementation.

That means:

- `boot/lib` is the source of truth for the persistent vector algorithm
- stage0 may use a thin adapter/shim layer to call or embed that logic
- stage0 may not grow a second independent copy of the vector algorithm in Rust
  beyond temporary glue needed to consume the Twinkle-authored artifact

Before Milestones 1-2 proceed, the implementation must choose and write down
one concrete consumption path from `boot/lib` into stage0. Accepted shapes
include:

- compile `boot/lib` Twinkle code into an artifact that stage0 links or imports
- generate stage0-consumable runtime/codegen artifacts from the `boot/lib`
  source of truth
- another equivalent path with the same ownership rule

Whichever path is chosen, the invariant is the same: algorithm ownership stays
in `boot/lib`, and Rust-side code remains integration glue rather than a second
maintained implementation.

Compiler-owned runtime modules authored in Twinkle Wasm IR are allowed within
this model. They count as Twinkle implementation of substrate, not as the
semantic vector library itself.

## Milestones

### Milestone 0: Consumption Path Decision

Decide the exact mechanism by which stage0 intrinsic/runtime code consumes the
Twinkle-authored implementation in `boot/lib`.

Tasks:

- choose one concrete stage0 consumption path from `boot/lib`
- identify the exact stage0 change sites required for that path
- write down the ownership boundary between Twinkle-authored algorithm code and
  Rust-side integration glue

Acceptance:

- the stage0-to-`boot/lib` call/link/import path is explicit before typed family
  ABI work proceeds
- Milestones 1-2 are scoped against a stage0 consumption path that actually
  exists, rather than a deferred future mechanism

### Milestone 1: Family Schema In Stage0

Define the generic typed vector-family schema and teach stage0 to name one
concrete family.

Tasks:

- define family naming/key derivation for `Vector<T>`
- add first typed vector/container/node/tail type definitions for `Vector<Int>`
- choose and create the `boot/lib` module home for the Twinkle-authored vector
  implementation
- keep old erased path working only as migration support

Acceptance:

- stage0 can refer to a dedicated `Vector<Int>` family in runtime/type planning
- no user-visible behavior changes yet

### Milestone 2: Typed Helper Selection

Wire codegen and stage0 runtime/helper selection to target the `Vector<Int>`
family for concrete monomorphized values.

Tasks:

- update stage0 runtime/helper entries to support typed helper family selection
- rewrite intrinsic emission in stage0 for typed vector families where current
  `Vector.make/get/set` lowering is hard-coded in the emitter
- route concrete `Vector<Int>` `make/get/len/set/concat/slice/push` helper
  calls to typed family symbols
- retarget builder helper families as part of the same typed-family switch
- keep stage0 `Vector.push/get/set/make` intrinsics as the public entry points,
  but change their concrete implementation path to use the typed family
- keep erased helpers only for unsupported or still-migrating cases

Acceptance:

- emitted code for concrete `Vector<Int>` no longer uses the generic hot-path
  helper ABI where typed families are available
- `Vector.get` and `Vector.make` for concrete `Vector<Int>` are included in this
  first typed path, not deferred

### Milestone 3: Twinkle-Authored `Vector<Int>` Algorithm

Land the first Twinkle-authored persistent vector implementation against the new
family schema.

Tasks:

- implement the persistent vector logic in Twinkle source under `boot/lib`
- keep structural sharing semantics
- keep builder semantics compatible with the existing lowering/optimizer
  contracts

Acceptance:

- `Vector<Int>` behavior matches existing tests
- the algorithm is Twinkle-authored, not Rust-only
- `boot/lib` is in scope for this kickoff and contains the first persistent
  vector implementation slice
- the stage0 integration path is explicit and does not require maintaining a
  parallel Rust-owned copy of the vector algorithm

### Milestone 4: Builder + Optimizer Compatibility

Validate that builder and uniqueness behavior still works with the first typed
family.

Tasks:

- verify `builder_from` does not mutate aliases
- verify uniqueness rewrites still target the correct helper family
- keep `vector_set_in_place` as an optimizer-only ABI hook

Acceptance:

- vector optimization tests still pass for the first family
- no aliasing regression from builder seeding

### Milestone 5: No-Boxing Proof

Add representation-focused checks that `Vector<Int>` no longer boxes element
payloads on the hot path.

Tasks:

- add or update WAT/snapshot assertions
- inspect emitted helper signatures and typed loads/stores for `make/get/len`,
  update paths, and builder-driven append paths
- verify builder-family retargeting for `collect` and loop-rewritten append
  cases

Acceptance:

- concrete `Vector<Int>` `make/get`/element read/write paths do not route
  through `anyref`
- concrete `Vector<Int>` builder-driven hot paths (`collect`, loop-rewritten
  append, builder freeze flow) do not route through `anyref`

## File Checklist For The First Slice

Expected stage0 touch points:

- `src/runtime/types.rs`
- `src/runtime/arr.rs`
- `src/codegen/prelude.rs`
- `src/codegen/ctx.rs`
- `src/codegen/emit.rs`
- tests and snapshots covering runtime/codegen/optimizer behavior

Expected `boot/lib` touch points in the kickoff itself:

- `boot/lib` Twinkle-authored internal vector implementation module(s)
- any wiring needed to let stage0 consume that implementation while keeping the
  public `Vector` surface stable

Expected boot compiler touch points later, after stage0 shape is proven:

- `boot/compiler/codegen/runtime/arr.tw`
- boot codegen/type-planning mirrors that adopt the same family schema

## Validation

Behavioral validation:

- existing vector runtime tests still pass
- existing vector optimizer tests still pass
- builder behavior remains correct
- structural sharing remains correct under branching versions

Representation validation:

- `Vector<Int>` hot path does not box through `anyref`
- typed helper families are selected for concrete `Vector<Int>`
- the concrete `Vector<Int>` intrinsic path for `Vector.get`, `Vector.make`, and
  `Vector.set` uses the typed family rather than erased ABI assumptions
- builder-driven `Vector<Int>` hot paths use typed builder/helper families rather
  than erased ABI assumptions
- erased fallback, if still present during migration, is not used for the
  supported first family

Migration validation:

- unsupported families still compile during migration
- no new permanent typed-vs-erased dual-path logic is introduced for ordinary
  concrete `Vector<Int>` code

## Exit Criteria

This kickoff plan is complete when all are true:

1. Stage0 has a real typed-family substrate for vectors.
2. `Vector<Int>` is wired end-to-end through that substrate.
3. `boot/lib` contains the first Twinkle-authored persistent vector
   implementation slice.
4. Public `Vector.push/get/set/make` remain intrinsic entry points for the
   kickoff, but their concrete `Vector<Int>` path is routed through the typed
   family substrate.
5. Builder and optimizer contracts still hold.
6. The `Vector<Int>` `make/get/set`/element update hot path no longer boxes
   element payloads via `anyref`.
7. The `Vector<Int>` builder-driven hot path (`collect`, loop append rewrites,
   freeze flow) no longer boxes through `anyref`.
8. Boot compiler/runtime mirrors have not yet been migrated unless doing so is
   trivial follow-through; the focus of this plan is stage0 integration plus the
   `boot/lib` implementation slice.

## Follow-On Work

After this kickoff plan lands:

1. Extend the same schema to `Vector<String>`.
2. Decide whether additional scalar/ref families should be mandatory before boot
   migration.
3. Port the proven family model to boot.
4. Use the same playbook for dict/HAMT work.

## Resolved Direction

- The permanent home for the Twinkle-authored internal vector implementation is
  `boot/lib`.
- During migration, the vector library pieces should be rebuilt from the
  Twinkle side rather than preserved as a long-term handwritten Rust runtime
  surface.
- Stage0 remains the first integration target, but it should be integrating the
  backend substrate with the Twinkle-authored library, not replacing that
  library with a Rust-owned implementation.
