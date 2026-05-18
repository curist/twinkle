# Stage0 Builtin and Contract Alignment Plan

Status: draft

## Context

Stage0 exists to bootstrap the boot compiler, but its builtin and contract model is
still partly hardcoded in Rust. Recent use of `Vector.sort()` in boot code exposed
that stage0 could typecheck a primitive `Ord.compare` path without being able to
lower it correctly. The immediate fix added stage0 intrinsic lowering for
primitive compare methods and tightened the `Stringify` fallback, but the larger
risk remains: builtin method knowledge is spread across several places.

The goal of this plan is to make stage0 boring and predictable while preserving
its role as a bootstrap compiler.

## Problems to Address

### Duplicated builtin definitions

Builtin methods and intrinsic functions are currently described across multiple
systems:

- prelude implementation sources
- prelude signature stubs
- Rust intrinsic registry
- Rust intrinsic signature loader
- Rust type environment method table
- Rust monomorphizer contract resolution
- Rust codegen intrinsic lowering
- generated boot core library embedding

When one of these changes without the others, stage0 can accept code that later
fails during monomorphization or codegen.

### Ad hoc contract resolution

Stage0 handles `Stringify` and `Ord` with special cases. Fallback logic that was
intended for `Stringify.to_string` was able to catch `Ord.compare`, which turned
a missing compare target into an invalid `String.to_string` call.

### Primitive methods are special in too many places

Primitive methods can be both normal prelude functions and Rust intrinsics. This
is useful for bootstrap performance and ABI reasons, but it means primitive
methods need a single policy for:

- type signatures
- method table entries
- contract satisfaction
- monomorphization target resolution
- codegen lowering
- semantic parity with the Twinkle prelude implementation

### Bootstrap-only failures are easy to miss

Boot compiler tests can pass with an existing self-hosted `target/twk`, while
`make stage2` fails when Rust stage0 compiles the same source. The validation
matrix needs explicit coverage for stage0 bootstrap-sensitive builtin paths.

## Desired Invariants

1. Every builtin method visible to the typechecker has a valid lowering path in
   stage0, either as a user/prelude function or as a Rust intrinsic.
2. Contract method resolution is method-specific. A fallback for one contract
   method must never apply to another method.
3. Primitive intrinsic signatures come from the same `.tw` signature stubs used
   for boot-side builtin signatures whenever possible.
4. Stage0 and boot agree on which primitive and prelude types satisfy builtin
   contracts.
5. `make stage2` remains the authoritative bootstrap validation, but narrower
   tests catch drift earlier.

## Proposed Work

### Phase 1: Make drift visible

Add focused tests that compile small programs through Rust stage0 for all
primitive contract methods used by boot code:

- `Vector<Int>.sort()`
- `Vector<Float>.sort()`
- `Vector<String>.sort()`
- `Vector<Byte>.sort()`
- direct `Int.compare`, `Float.compare`, `String.compare`, and `Byte.compare`
- generic functions with `T: Ord` calling `a.compare(b)`
- generic functions with `T: Stringify` calling `x.to_string()`

These should run in the Rust test suite and fail before codegen panics. The tests
should assert successful build/run behavior rather than only typecheck success.

### Phase 2: Centralize intrinsic metadata checks

Extend the intrinsic registry validation so each `include_in_signature_registry`
entry is checked against the parsed `.tw` signature stubs for:

- canonical name
- type parameter names and bounds
- parameter types
- return type
- dispatch kind expectations

This keeps Rust registry entries honest without making Rust the canonical source
for user-visible signatures.

### Phase 3: Normalize contract target resolution

Replace contract-specific fallback chains with a resolver that takes:

- contract name
- method name
- receiver type
- argument types

The resolver should return an explicit result:

- resolved builtin intrinsic target
- resolved user/prelude function target
- unsatisfied contract
- ambiguous or malformed contract implementation

`Stringify.to_string` and `Ord.compare` should go through the same resolver
shape, with method-specific matching only where the contract definition requires
it.

### Phase 4: Reduce primitive special cases

Audit primitive methods and classify each one as one of:

- runtime import intrinsic
- instruction-level intrinsic
- ordinary prelude function compiled from Twinkle source

For each instruction-level intrinsic, document why it cannot currently be an
ordinary prelude function in stage0. Where there is no ABI or bootstrap reason,
prefer ordinary prelude functions.

### Phase 5: Strengthen bootstrap validation

Add a lightweight bootstrap smoke target that uses Rust stage0 to build a small
fixture exercising recently added prelude APIs. This should be cheaper than a
full `make stage2` but catch the same class of stage0 drift.

The full `make stage2` fixed-point check remains required before landing changes
that affect boot sources, prelude signatures, intrinsic metadata, or contract
resolution.

## Open Questions

- Should primitive `compare` methods remain Rust intrinsics, or should stage0
  compile their Twinkle implementations once stage0 can reliably load prelude
  implementation modules?
- Should contract definitions (`Stringify`, `Eq`, `Ord`) have a single machine-
  readable source shared by stage0 and boot?
- Can the generated `boot/lib/module/core_lib.tw` be eliminated or replaced with
  a more direct embedding step to reduce sync points?
- How much of stage0's type environment method table can be derived from parsed
  signature modules instead of handwritten entries?

## Non-Goals

- Replacing stage0 with the boot compiler.
- Adding a trait system.
- Changing Twinkle's explicit contract model.
- Reworking the Wasm runtime ABI beyond what is needed for builtin consistency.

## Practical Guidance Until This Lands

When adding or changing a builtin/prelude method, update and validate all relevant
surfaces:

1. prelude implementation source
2. prelude signature stub
3. Rust intrinsic registry, if the method is intrinsic
4. Rust intrinsic signature contract, if the method is intrinsic
5. Rust codegen lowering, if instruction-level intrinsic
6. boot core library generation
7. stage0 bootstrap via `make stage2`
