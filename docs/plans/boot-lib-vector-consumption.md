# `boot/lib` Vector Consumption Boundary

## Goal

Define the concrete ABI and artifact boundary by which stage0 consumes a
Twinkle-authored `Vector<Int>` implementation from `boot/lib`, so vector
semantics can move to the Twinkle side without requiring a parallel Rust-owned
algorithm.

This plan is subordinate to:

- [twinkle-vector-kickoff.md](twinkle-vector-kickoff.md)
- [persistent-vector.md](persistent-vector.md)
- [backend-anyref-elimination.md](backend-anyref-elimination.md)
- [boot-foundation-libs.md](boot-foundation-libs.md)

If this document disagrees with those plans, follow them in that order.

### Prerequisites

This plan depends on infrastructure that does not exist yet:

- [twinkle-runtime-import-boundary.md](twinkle-runtime-import-boundary.md)
  provides the mechanism by which `boot/lib` Twinkle code can import
  `rt.arr` substrate helpers. Without it, there is no way for a `.tw` file
  to bind runtime symbols, and the artifact-consumption boundary described
  here is not implementable.

Until that prerequisite lands, stage0 vector specialization work (typed
family dispatch, builder metadata tracking) remains on the existing
`rt.arr`-direct path. That work is useful substrate preparation but does not
constitute progress on this plan's milestones.

## Why This Plan Exists

The current stage0 work has been building backend substrate:

- typed helper/container families
- vector/backend representation inference
- builder-family preservation

That work is necessary, but it is not the long-term ownership model.

The intended end state is closer to a host-contract style boundary:

- `boot/lib` defines the vector implementation
- stage0 lowers calls to an agreed ABI surface
- the linker consumes the compiled artifact like any other module
- Rust remains substrate and integration glue, not algorithm owner

This plan makes that boundary explicit so implementation can pivot away from
indefinite Rust-side specialization.

## Design Position

### 1. `boot/lib` Owns Vector Semantics

The following belong on the Twinkle side:

- persistent vector algorithm
- structural sharing rules
- builder semantics above optimizer-only mutation hooks
- safe `Vector.make/get/set/push/concat/slice/len` behavior

Rust/stage0 remains responsible for:

- typed family naming and Wasm repr policy
- runtime substrate helpers in `rt.arr`
- codegen selection of the correct library ABI symbol
- optimizer-only unique/in-place hooks
- linking the compiled `boot/lib` artifact

### 2. Stage0 Should Consume An Artifact, Not Source-Rewrite It

The preferred shape is:

1. compile the relevant `boot/lib` vector module(s) with stage0
2. obtain a normal Wasm module artifact with named exports/imports
3. link that module alongside runtime modules and the user module
4. have stage0 intrinsics/runtime-prelude selection target the exported ABI

Not allowed:

- generating Rust copies of vector logic from Twinkle source
- embedding a second maintained Rust implementation of the algorithm
- source-to-source inlining of `boot/lib` code into user modules as the primary
  consumption path

### 3. `rt.arr` Becomes Substrate-Only

`rt.arr` should not remain the semantic home of vectors.

Its role in this design is limited to:

- raw typed container allocation/access helpers
- builder substrate helpers
- optimizer-only hooks where mutation is still a backend concern

The Twinkle library implementation may import `rt.arr` helpers, but user code
should not need to know that shape directly.

### 4. Public `Vector.*` Surface Can Stay Intrinsic During Migration

For the first landing slice, user-visible `Vector.*` calls may still enter
through stage0 intrinsics.

But their concrete implementation path changes:

- before: intrinsic lowering directly targets `rt.arr`
- after: intrinsic lowering targets the `boot/lib` vector ABI

That keeps user-facing API stability while moving implementation ownership to
Twinkle.

## Scope

In scope:

- first ABI for `Vector<Int>` library consumption from `boot/lib`
- stage0 import/link model for that compiled artifact
- ownership split between `boot/lib`, stage0, and `rt.arr`
- first migration path for safe vector operations and builder operations

Out of scope:

- all vector families at once
- final public stdlib rehome of `Vector.*`
- dict/HAMT boundary design
- removing every temporary direct `rt.arr` call in the same change

## First Family

The first consumption boundary is for:

- `Vector<Int>`

Reasons:

- existing backend substrate already proves the `i64` family path
- it is the simplest concrete ABI to stabilize
- it exercises both direct ops and builder-driven collect paths

## Boundary Shape

### Layer 1: Substrate Helpers (`rt.arr`)

These remain low-level typed-family helpers owned by stage0/runtime.

Examples for `Vector<Int>`:

- `rt_arr__make_i64`
- `rt_arr__get_i64`
- `rt_arr__set_i64`
- `rt_arr__len_i64`
- `rt_arr__concat_i64`
- `rt_arr__slice_i64`
- `rt_arr__push_i64`
- `rt_arr__builder_new`
- `rt_arr__builder_from_i64`
- `rt_arr__builder_push_i64`
- `rt_arr__builder_freeze_i64`

These are substrate, not the semantic library ABI.

### Layer 2: Library ABI (`boot/lib`)

`boot/lib` exports the semantic `Vector<Int>` operation surface that stage0
consumes.

Initial required exports:

- `vector_i64_make`
- `vector_i64_get`
- `vector_i64_set`
- `vector_i64_len`
- `vector_i64_concat`
- `vector_i64_slice`
- `vector_i64_push`
- `vector_i64_builder_new`
- `vector_i64_builder_from`
- `vector_i64_builder_push`
- `vector_i64_builder_freeze`

Exact final symbol spelling can follow Twinkle module export conventions, but
the ABI must distinguish:

- semantic library exports from `boot/lib`
- raw substrate helpers from `rt.arr`

### Layer 3: Public Entry Surface

The public entry surface remains:

- `Vector.make`
- `Vector.get`
- `Vector.set`
- `Vector.push`
- `Vector.len`
- `Vector.concat`
- `Vector.slice`
- internal builder intrinsics

Stage0 lowers these to the library ABI, not directly to the substrate, once the
consumption path is active for `Vector<Int>`.

## ABI Rules

### Safe Operations

The `boot/lib` ABI owns safe semantics:

- bounds behavior
- persistence semantics
- builder freeze semantics
- alias safety for builder-from

That means `Vector.get` / `Vector.set` safe behavior belongs in Twinkle code,
not in Rust-only helper logic.

### Optimizer-Only Hooks

Optimizer-specific mutation hooks may remain outside the semantic ABI for the
first pass.

Examples:

- `VECTOR_SET_IN_PLACE`
- any future unique-builder mutation helpers

These hooks are allowed to keep targeting substrate/runtime helpers directly as
long as:

- they are compiler-internal only
- they preserve the same semantic contract as the library layer
- the Twinkle library is still the single semantic source of truth

### Builder ABI

Builder operations are part of the semantic consumption boundary, not just
raw runtime details.

The library ABI must own:

- builder creation semantics
- builder seeding semantics
- builder freeze semantics

The substrate may still provide raw builder storage helpers, but stage0 should
prefer library-owned builder entry points for semantic collect paths.

## Integration Path

### Chosen Shape

The first implementation should use this path:

1. author the vector implementation in `boot/lib`
2. compile that module with stage0 as part of the build pipeline
3. include its Wasm module in the linked module set
4. route `Vector<Int>` intrinsic/runtime calls to its exports

This requires:

- normal module linking
- export/import name agreement
- minimal stage0 selection logic

This does **not** require:

- source rewriting of `boot/lib` into user code
- a Rust mirror of vector semantics
- special-purpose generated Rust shims

### Build Pipeline Requirement

Stage0 needs a notion of compiler-owned library artifacts alongside runtime
modules.

For the first slice, it is acceptable if the pipeline explicitly compiles a
small fixed `boot/lib` vector module set rather than introducing the full boot
multi-module world immediately.

But the pipeline must still treat that output as a compiled artifact, not as
manually duplicated Rust logic.

## File Targets

Expected primary implementation touch points:

- `boot/lib/...` new internal vector module(s)
- `src/cli/build.rs`
- `src/codegen/prelude.rs`
- `src/codegen/emit.rs`
- `src/runtime/mod.rs` or equivalent runtime/module assembly path
- linker/module assembly code that includes runtime + `boot/lib` artifacts

Likely supporting docs/tests:

- `docs/plans/twinkle-vector-kickoff.md`
- tests that inspect linked WAT for calls to `boot/lib` vector exports
- compatibility tests comparing stage0 behavior before/after the boundary flip

## Milestones

### Milestone 1: Freeze The ABI

Write down the exact `Vector<Int>` library export surface and its ownership
split versus `rt.arr`.

Acceptance:

- every first-slice vector operation has a named library ABI entry
- optimizer-only hooks are explicitly separated from semantic ABI entries

### Milestone 2: Stage0 Artifact Consumption

Teach the build pipeline to compile and link the `boot/lib` vector module as an
artifact.

Acceptance:

- the linked module contains the compiled `boot/lib` vector artifact
- stage0 can import its exports by stable symbol name

### Milestone 3: Retarget Intrinsic Dispatch

Retarget `Vector<Int>` intrinsic/runtime lowering from substrate helpers to the
library ABI.

Acceptance:

- concrete `Vector<Int>` user code calls `boot/lib` exports for semantic ops
- direct `rt.arr` calls remain only in substrate or optimizer-only paths

### Milestone 4: Move First Semantic Slice To Twinkle

Implement the first real `Vector<Int>` semantic layer in `boot/lib`.

Acceptance:

- no parallel Rust semantic implementation is required for the covered ops
- tests prove behavior is still correct through the linked artifact path

## Validation

Behavioral validation:

- existing `Vector<Int>` runtime/optimizer tests still pass
- collect and direct method calls still behave the same

Boundary validation:

- linked user WAT shows semantic vector calls going through `boot/lib` exports
- `rt.arr` remains visible as substrate, not as the final semantic entry layer

Ownership validation:

- the covered semantic operations have one Twinkle-authored implementation path
- no maintained Rust duplicate of those semantics remains

## Exit Criteria

This plan is complete when all are true:

1. `boot/lib` exports an agreed `Vector<Int>` semantic ABI.
2. Stage0 links the compiled `boot/lib` vector artifact as part of normal build.
3. Concrete `Vector<Int>` semantic ops are lowered to that ABI rather than
   directly to `rt.arr`.
4. `rt.arr` remains substrate-only for the covered operations.
5. The first Twinkle-authored vector semantic slice is consumed end-to-end by
   stage0 through the artifact boundary.

## Follow-On Work

After this boundary lands:

1. Move more of the vector algorithm from temporary substrate assumptions into
   `boot/lib`.
2. Extend the same pattern to additional vector families.
3. Reuse the same artifact-consumption model in the boot compiler proper.
4. Apply the same ABI-first approach to persistent dict/HAMT work.
