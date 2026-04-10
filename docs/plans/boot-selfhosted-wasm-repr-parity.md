# Boot Self-Hosted Wasm Representation Parity

## Status

In Progress.

## Problem

The self-hosted boot compiler now gets past the original multi-module type
identity bug and the follow-on boot monomorphization bug, but `boot/main.tw`
Wasm execution still fails in a series of downstream codegen paths.

Recent failures moved through:

- partial generic specialization reaching Wasm planning
- hoisted lambda `FuncId` collisions during linking
- missing closure capture params in emitted boot functions
- missing string-pool entries for pattern-only string literals
- alias-backed record fields lowering to `field#-1`
- alias-backed record / sum layouts not being recognized during emission

The current failure shape is more structural:

- the boot emitter expects typed record / sum layouts in places where the
  self-hosted pipeline currently only has `Anyref` or another erased
  representation
- impossible or dead pattern branches can still be present in ANF and reach the
  emitter with weak or placeholder local typing
- stage0 Rust codegen already has substantial flow-sensitive metadata and sum
  representation machinery, but the boot emitter does not

That makes fixing the next failing site feel like whack-a-mole. The problem is
no longer a single bad remap or one missing substitution; it is a representation
parity gap.

## Goal

Make boot self-hosted Wasm codegen use a coherent, explicit representation model
for records, closures, `Option` / `Result` / user sums, and erased `Anyref`
values so `boot/main.tw` can execute under the self-hosted Wasm compiler
without a long tail of ad hoc emitter patches.

## Non-Goals

- fully port all stage0 codegen complexity into boot immediately
- redesign the language or runtime ABI
- paper over type errors in Wasm planning or emission by silently substituting
  arbitrary defaults in semantically live paths

## Core Diagnosis

The boot backend currently mixes three different assumptions:

1. **Typed layout assumption**
   - many emitter paths call `layout_of(...)`, `get_sum_layout(...)`, or field
     lookup helpers assuming a concrete `MonoType`

2. **Erased runtime assumption**
   - some values are intentionally represented as `Anyref`, closure env arrays,
     or runtime boundary values

3. **Weak-flow assumption**
   - pattern locals and branch-local values may survive into ANF even when their
     branch is impossible or only valid under a refined scrutinee shape

These assumptions can coexist only if the backend tracks which locals still have
semantic typed meaning and when an erased runtime value can be reinterpreted as
that typed shape. Stage0 does this with explicit codegen metadata; boot mostly
tries to infer from raw `MonoType` alone.

## Plan Role And Sequencing

This document is the umbrella plan for the current self-hosted Wasm
representation cleanup.

It answers:

- what self-hosting failures are still representation-driven
- what categories of backend drift are blocking reliable `boot/main.tw`
  execution
- what sequence of work should be active now versus later

Related plans have narrower roles:

- [boot-backend-physical-typing.md](boot-backend-physical-typing.md)
  - near-term execution plan for current backend stabilization
  - centralizes actual-vs-expected `ValType` adaptation at erased boundaries
  - should drive the current emitter / verifier cleanup work
- [backend-anyref-elimination.md](backend-anyref-elimination.md)
  - longer-term architecture plan
  - removes or shrinks erased helper/container boundaries after current
    stabilization is in place

Recommended execution order:

1. use this plan to track active self-hosting blockers and repr categories
2. execute the physical typing plan to stop recurring validator mismatches in
   the current backend architecture
3. use the `anyref` elimination plan to replace remaining erased families once
   self-hosted stability is no longer advancing one boundary bug at a time

## Strategy

Address the issue in layers instead of chasing the next panic site.

### Layer 1: Make representation categories explicit

Document and enforce the categories each emitter path may consume:

- **Concrete typed value**
  - safe to pass to `layout_of`, `record_layout_of`, `get_sum_layout`
- **Erased sum value**
  - runtime variant / general anyref shape, not safe for direct typed layout
    access without reconstruction or conversion
- **Opaque erased value**
  - plain `Anyref`, closure env element, boundary-boxed value
- **Dead / impossible placeholder**
  - branch-local value that survives in IR only for structural reasons

The boot emitter should reject or explicitly branch on category mismatches,
not discover them indirectly through layout helper panics.

### Layer 2: Define where representation metadata lives

Pick one minimal source of truth for boot codegen, instead of open-coded local
heuristics in several files.

Recommended direction:

- extend boot emission context with local metadata parallel to stage0’s
  codegen context, but keep the MVP smaller:
  - local semantic `MonoType`
  - local runtime representation kind
  - optional typed-sum metadata for `Option` / `Result` / concrete named sums
  - optional closure signature / capture metadata where needed

This metadata should be established once during function setup and updated in a
small number of flow-sensitive places:

- `let`
- `assign`
- `if`
- `match`
- intrinsic calls that produce typed sums
- closure creation / closure call

### Layer 3: Split typed-sum and erased-sum paths

Any emitter helper that currently assumes “sum-like = typed layout” should be
split into:

- typed path:
  - concrete `Option<T>` / `Result<T,E>` / user sum with known layout
- erased path:
  - branch using runtime representation or refusing typed field extraction
- impossible path:
  - structurally present but semantically dead branch; emit an always-false
    condition or no bindings rather than descending into layout logic

Primary helpers to normalize:

- pattern condition emission
- pattern binding emission
- match arm lowering support
- intrinsic emitters returning `Option` / `Result`
- record field access when aliases erase to another shape

### Layer 4: Tighten earlier invariants where possible

Some current emitter failures really come from upstream IR being too weak.

Add targeted invariants in earlier passes:

- linker must preserve unique hoisted function identities
- lower-core free-var collection must not capture pattern-bound locals
- alias-backed field access must resolve to canonical fields before emission
- Wasm planning must collect all string literals used by pattern tests
- monomorphization must never manufacture partial specializations

This has already paid off and should continue, but it is not enough on its own.

## Work Plan

### Milestone 1: Representation audit

Produce a short inventory of boot emitter helpers and classify each as:

- typed-only
- erased-only
- mixed / needs split

Focus files:

- `boot/compiler/codegen/emit.tw`
- `boot/compiler/codegen/wasm_plan.tw`
- `boot/compiler/lower_anf.tw`
- `boot/compiler/anf.tw`

Deliverable:

- table of helper → expected representation → current callers → known failure
  modes

### Milestone 2: Minimal boot codegen metadata

Introduce a small representation enum and local metadata map in the boot emitter.

Suggested shape:

- `TypedRecord(MonoType)`
- `TypedSum(MonoType)`
- `Closure(MonoType)`
- `ErasedAnyref`
- `DeadValue`

This does **not** need stage0’s full richness immediately. The main purpose is
to stop layout helpers from being called on values that are only known as
`Anyref` or dead placeholders.

Deliverable:

- centralized helper for “can this local use typed layout access?”
- centralized helper for “can this local participate in typed sum patterning?”

### Milestone 3: Match / pattern normalization

Refactor boot match emission to operate on representation-aware scrutinees.

Requirements:

- impossible variant patterns on non-sum / dead scrutinees lower to false
- typed sum scrutinees keep current typed-layout path
- erased sum scrutinees use an explicit runtime path or are rejected earlier
  with a precise diagnostic
- pattern bindings only materialize locals that are semantically available in
  the arm

This milestone should remove the current `get_sum_layout(... anyref)` class of
failure.

### Milestone 4: Intrinsic sum producers and consumers

Audit boot intrinsics that create or consume `Option` / `Result` / iterator-ish
sum values.

Focus areas:

- vector / string indexing helpers
- parse helpers
- iterator / unfold helpers
- boundary conversions that box or unbox sum-like values

Goal:

- make every such intrinsic declare whether it produces a typed sum or an
  erased value, and record that in emitter metadata

### Milestone 5: Alias and canonical-layout cleanup

Consolidate alias expansion so record/sum layout helpers do not each grow their
own alias recursion rules.

Recommended helpers:

- canonical record mono
- canonical sum mono
- field lookup on canonical record mono
- variant payload lookup on canonical sum mono

This avoids repeating special cases in lowering, planning, and emission.

### Milestone 6: Self-hosted regression harness

Add a focused boot self-hosting regression matrix that runs the self-hosted Wasm
compiler through stable command slices:

- `check boot/main.tw`
- `ir boot/main.tw`
- `build boot/main.tw`
- selected smaller fixtures exercising:
  - nested variant patterns
  - alias-backed records and sums
  - closures with captures
  - higher-order generic functions
  - pattern-only string literals

The purpose is to catch representation regressions by category, not by a single
monolithic `boot/main.tw` run.

## Immediate Next Steps

1. Trace the exact `Anyref` local currently reaching `get_sum_layout` during
   self-hosted `build boot/main.tw`.
2. Add temporary instrumentation identifying:
   - current function
   - local id / atom source
   - semantic mono
   - runtime representation category
3. Implement the minimal metadata required to distinguish typed-sum from erased
   `Anyref` at that site.
4. Refactor pattern emission to use that metadata instead of raw `MonoType`
   alone.
5. Turn the discovered case into a dedicated regression test before proceeding
   to the next failure.

## Success Criteria

This plan is successful when:

- self-hosted `build boot/main.tw` no longer advances by panic-to-panic emitter
  fixes
- typed record / sum access sites are all representation-checked up front
- alias-backed records and sums are handled through shared canonical helpers
- self-hosted Wasm `build` completes for `boot/main.tw`, or failures that remain
  are ordinary compiler diagnostics rather than backend invariant panics

## Related Documents

- [self-hosting.md](self-hosting.md)
- [self-hosting-status.md](self-hosting-status.md)
- [boot-multi-module.md](boot-multi-module.md)
- [boot-backend-physical-typing.md](boot-backend-physical-typing.md)
- [backend-anyref-elimination.md](backend-anyref-elimination.md)
