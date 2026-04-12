# Boot Backend Physical Typing Plan

## Status

Done for the current stabilization scope.

## Plan Role

This is the near-term execution plan for the current boot backend
representation cleanup.

It sits between:

- [boot-selfhosted-wasm-repr-parity.md](boot-selfhosted-wasm-repr-parity.md)
  - umbrella plan tracking the self-hosting repr blockers and active sequencing
- [backend-anyref-elimination.md](backend-anyref-elimination.md)
  - longer-term plan for shrinking or removing erased helper/container families

This plan should be executed before major `anyref` elimination work. Its purpose
is to make the current erased boundaries explicit and correct so backend cleanup
stops depending on validator whack-a-mole.

## Problem

The boot backend keeps hitting Wasm validation failures of the form:

- expected `i64`, found `anyref`
- expected typed ref, found `anyref`
- expected `anyref`, found scalar

These are usually not isolated bugs in one intrinsic. They come from the same
structural issue:

- semantic type (`MonoType`)
- representation category (`ReprKind`)
- physical Wasm value type (`ValType`)

are tracked in different places, and emitter code still sometimes assumes they
all agree without checking.

The highest-risk areas are erased boundaries:

- runtime helper calls with `anyref` ABI slots
- host imports
- container ingress/egress (`PVec`, `Dict`, builder arrays)
- helper calls returning erased values
- typed field writes and constructors fed by erased inputs

The current backend has explicit `WrapAnyref` / `UnwrapAnyref` nodes and slot
repr metadata, but coercion is still partly encoded as handwritten local logic
inside individual emitters. That keeps turning representation bugs into
whack-a-mole.

## Goal

Make physical type adaptation in the boot backend systematic.

The backend should be able to answer, for every emitted edge:

- what physical Wasm type is currently on the stack?
- what physical Wasm type is expected next?
- what coercion is required, if any?

The intended result is:

- fewer ad hoc boxing/unboxing fixes in individual intrinsics
- backend verification failures that point to a specific missing coercion edge
  before WAT generation
- a cleaner path toward the broader `anyref` reduction work without requiring it
  all up front

## Non-Goals

- eliminate all `anyref` from the runtime in this phase
- redesign the Twinkle surface language or host ABI
- fully port all stage0 backend machinery into boot immediately
- patch generated WAT directly instead of fixing backend plumbing

## Core Diagnosis

Today the boot backend still relies on three partially overlapping mechanisms:

1. **Boundary insertion**
   - inserts `AWrapAnyref` / `AUnwrapAnyref`
   - good for making erased crossings explicit
   - not sufficient by itself because some runtime/container ops still need
     emitter-side adaptation

2. **Slot metadata**
   - `repr_assign` computes `repr` and `wasm_type`
   - useful for locals, but not yet enforced on every emitted operand/result edge

3. **Emitter-local coercion logic**
   - runtime calls, intrinsics, indexing, field writes, and constructors still
     contain handwritten assumptions about whether an operand is already typed or
     already erased

The missing piece is a single physical typing discipline that every emitter path
must obey.

## Design Principles

### 1. Emit from physical types, not semantic hope

`MonoType` explains how to box, unbox, or cast.

It should not by itself decide whether adaptation is needed.

Emitter decisions should instead compare:

- actual physical type of the produced operand
- expected physical type of the consumer

### 2. Every erased boundary gets a named adapter

Erased boundaries should never be “probably already handled upstream”.

If a value enters or leaves an erased runtime/container surface, emission should
use a dedicated helper for that boundary shape.

Examples:

- erased call arg
- erased call result
- erased container store
- erased container load
- typed field write from erased source

### 3. Verification should reason about edges, not only slots

Slot metadata is necessary but not enough.

The verifier should check physical compatibility at the operation edge level:

- call arg to param
- op result to destination slot
- constructor payload to field type
- `struct.set` value to field type
- container runtime result to typed consumer

### 4. Keep the short-term ABI, but stop scattering its cost

This plan does not require typed helper families yet.

It accepts that some runtime surfaces still use erased `anyref`, but it moves
all adaptation for those surfaces into centralized backend rules rather than per
site guesswork.

## Intended Invariants

After this plan lands, the backend should maintain these invariants:

1. Every local slot has a single source of truth for its physical Wasm type.
2. Every emitted operand position has a known expected physical Wasm type.
3. Every mismatch between actual and expected types goes through a centralized
   coercion helper.
4. No intrinsic/runtime/container emitter writes directly into typed fields or
   typed constructors from erased values without an explicit coercion step.
5. No scalar value is passed directly into an erased `anyref` ABI slot without
   explicit boxing.
6. No erased runtime/container result is consumed as typed data without explicit
   unboxing or ref cast.

## Workstreams

### Workstream A: Central stack coercion

Create one canonical stack adaptation helper in boot codegen.

Inputs:

- actual `ValType`
- expected `ValType`
- semantic `MonoType` for boxing/unboxing details

Responsibilities:

- scalar ↔ `anyref` boxing/unboxing
- typed ref casts
- nullable ↔ non-nullable coercions where valid
- `i32` ↔ `i64` adaptation where ABI requires it

Rule:

- if an emitter path needs adaptation and does not use this helper, that path is
  suspicious

### Workstream B: Operand/consumer physical typing helpers

Add centralized helpers for the most common edge shapes:

- emit runtime arg for ABI param `i`
- adapt runtime result to destination slot type
- emit erased container element/key/value store
- adapt erased container load to typed destination
- adapt constructor/field payload to expected field `ValType`

This should replace repeated ad hoc logic in:

- runtime call emission
- indexing emission
- vector/dict intrinsics
- cell/iterator helpers
- typed record and sum construction paths

### Workstream C: Physical-type-aware emitter plumbing

Strengthen emitter context helpers so codegen can always ask:

- actual physical type of atom/local/global
- semantic mono of atom/local/global
- repr category of atom/local/global

Then update emitter sites to compare actual vs expected instead of branching on
semantic type alone.

Priority sites:

- runtime calls
- intrinsic calls touching erased storage
- `AIndex`
- `struct.new`
- `struct.set`
- sum constructors
- host import adaptation

### Workstream D: Edge verification in the prepared backend

Extend the backend verifier so it catches missing coercions before WAT
validation.

Checks to add:

- call argument physical compatibility against callee ABI
- constructor payload compatibility against field layout
- `struct.set` value compatibility
- direct use of erased container/runtime loads in typed consumers without an
  explicit adapter
- direct use of scalar values in erased ABI slots without boxing

The verifier should fail with messages framed in backend terms, not raw Wasm
validator offsets.

### Workstream E: Erased surface inventory

Produce and maintain a short inventory of every erased ABI surface still in use.

Initial categories:

- runtime helpers taking/returning `anyref`
- host imports using erased values
- `rt_arr__*` payload APIs
- `rt_dict__*` key/value APIs
- closure universal call path
- iterator state fields storing erased values

For each surface record:

- producer/consumer shape
- required coercion direction
- current adapter helper
- future typed-family replacement candidate

This inventory becomes the checklist for validation cleanup and later `anyref`
reduction work.

## Milestones

### Milestone 1: Canonical coercion helper

Land a single boot helper for stack coercion and move existing scattered runtime
result/argument adaptation onto it.

Exit criteria:

- runtime call argument adaptation and runtime result adaptation no longer use
  bespoke scalar-vs-anyref checks at each site

### Milestone 2: Erased container boundary helpers

Wrap all erased container ingress/egress behind dedicated helpers.

Targets:

- vector element store/load
- dict key/value store/load
- builder element ingress/egress
- iterator seed/yield storage where erased

Exit criteria:

- `rt_arr__get` / `rt_arr__set` / `rt_dict__*` are not called directly from
  emitter paths that also hand-roll boxing or unboxing

### Milestone 3: Typed field/constructor adaptation

Normalize all field writes and constructors so payloads are coerced against the
actual field `ValType` before emission.

Targets:

- `emit_record_literal`
- `emit_record_update`
- sum/option/result constructors
- cell helpers
- iterator helper construction

Exit criteria:

- no typed field or constructor path relies on “semantic mono probably already
  matches physical stack shape”

### Milestone 4: Verifier edge checks

Teach `verify_prepared` to reject missing adapters before WAT emission.

Exit criteria:

- a representative missing boxing/unboxing bug fails in backend verification
  with a precise edge-level message

### Milestone 5: Erased surface inventory and shrink plan

Record the remaining erased surfaces and link each to either:

- its permanent external-ABI reason
- or a follow-up typed-family elimination plan

Exit criteria:

- active backend cleanup work can be driven from the inventory instead of from
  validator offsets alone

## Outcome

This plan's current Milestones 1–4 are complete enough for the original
problem scope.

What landed in this pass:

- canonical runtime argument/result coercion centered on `emit_coerce_stack`
- named ABI shims for runtime/host surfaces that reshape representations
  without mixing that logic into coercion rules
- erased container ingress/egress cleanup across the main vector/dict/runtime
  surfaces used by self-hosting
- explicit field/payload coercion in typed record, variant, and update emission
- verifier checks for missing erased-boundary adaptation at call edges plus
  field/payload compatibility checks tied to typed layouts

Result:

- the recurring `expected X, found anyref` / `expected anyref, found scalar`
  class of self-hosted backend failures is no longer the active blocker
- self-hosted build/validate work gets past the original physical typing
  failures that motivated this plan

The current self-hosting blocker has moved elsewhere: sum match/pattern
lowering for user sums is now the active failure source. That work belongs back
under the umbrella repr parity plan rather than extending this plan's scope.

## Reproduction And Validation Loop

Use the repository's preferred self-host loop so backend changes can be checked
quickly and consistently.

Primary commands:

```bash
cargo run --release --bin twk -- build boot/main.tw -o /tmp/boot.wasm
node tools/run_wasm_node.mjs /tmp/boot.wasm -- build boot/main.tw
wasm-tools validate --generate-dwarf full --features all boot/main.wat
cargo run --release -- run boot/main.wat
```

What each step is for:

1. `cargo run --release --bin twk -- build ...`
   - rebuild the boot compiler Wasm with stage0 Rust codegen
2. `node tools/run_wasm_node.mjs /tmp/boot.wasm -- build boot/main.tw`
   - run the boot compiler itself and regenerate `boot/main.wat`
3. `wasm-tools validate --generate-dwarf full --features all boot/main.wat`
   - catch physical type mismatches early with function/offset/source location
     information
4. `cargo run --release -- run boot/main.wat`
   - exercise the runtime/import side after validation succeeds

Working conventions:

- prefer the Node runner path above over slower `cargo run --release -- run ...`
  based self-host loops when iterating on boot backend fixes
- treat `boot/main.wat` as a generated debugging artifact and do not commit it
- when validation reports a byte offset, use the generated-dwarf location first;
  if needed, inspect the nearby WAT and use `wasm-tools` around the reported
  offset to find the exact instruction edge

## Immediate Next Steps

1. Consolidate the recently added runtime/container coercion fixes under one
   named coercion helper instead of leaving them as partially duplicated local
   logic.
2. Audit `emit_record_literal`, `emit_variant_literal`, and `emit_record_update`
   for the same actual-vs-expected physical type mismatch pattern that showed up
   in cells and container ops.
3. Add verifier checks for:
   - scalar passed to erased ABI slot without boxing
   - erased result written into typed slot without unboxing
   - typed field write fed by erased source
4. Document the current erased runtime surfaces in one table under this plan.
5. Continue the `host.read_file` import mismatch separately; it is related to
   runtime/result adaptation, but it also reflects a host ABI typing issue.

## Relationship To Other Plans

- [boot-selfhosted-wasm-repr-parity.md](boot-selfhosted-wasm-repr-parity.md)
  tracks the broader self-hosting representation gap that exposed these bugs.
  This plan was the implementation discipline for one major slice of that work:
  physical backend typing at erased boundaries.
- [backend-anyref-elimination.md](backend-anyref-elimination.md) defines the
  longer-term destination where many of these erased boundaries disappear
  entirely. This plan remains the nearer-term stabilization step that made the
  current erased boundaries explicit and correct before they are removed.

In sequence:

1. parity plan identifies the blocker category and owns current status
2. this plan stabilized the current backend by centralizing coercion and
   verification for erased-boundary physical typing
3. current self-hosted failures now move back to the parity plan's remaining
   repr/match-lowering work
4. `anyref` elimination removes the remaining erased surfaces structurally
