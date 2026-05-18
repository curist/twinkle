# Stage0 Builtin and Contract Alignment Plan

Status: accepted

## Context

Stage0 exists to bootstrap the boot compiler, but its builtin and contract model is
still partly hardcoded in Rust. Recent use of `Vector.sort()` in boot code exposed
that stage0 could typecheck a primitive `Ord.compare` path without being able to
lower it correctly. The immediate fix added stage0 intrinsic lowering for
primitive compare methods and tightened the `Stringify` fallback, but the larger
risk remains: builtin method knowledge is spread across several places.

The goal of this plan is to make stage0 boring and predictable: every builtin
path accepted by stage0 is exercised by tests, every visible intrinsic has a
validated signature and lowering path, and contract method lookup is explicit
rather than fallthrough-based.

## Problems to Address

### Duplicated builtin definitions

Builtin methods and intrinsic functions are currently described across multiple
systems:

- prelude implementation sources: `prelude/*.tw`
- prelude signature stubs: `prelude/signatures/*.tw`
- Rust intrinsic registry: `src/intrinsics/registry.rs`
- Rust intrinsic signature loader: `src/intrinsics/signatures.rs`
- Rust type environment method table: `src/types/env.rs`
- Rust monomorphizer contract resolution: `src/ir/monomorphize.rs`
- Rust codegen intrinsic lowering: `src/codegen/emit.rs`
- generated boot core library embedding: `boot/lib/module/core_lib.tw`

When one of these changes without the others, stage0 can accept code that later
fails during monomorphization or codegen.

### Ad hoc contract resolution

Stage0 handles `Stringify` and `Ord` with special cases. Fallback logic that was
intended for `Stringify.to_string` was able to catch `Ord.compare`, which turned
a missing compare target into an invalid `String.to_string` call.

`Eq` is a near-term motivating case too. As Eq contract support expands, it will
exercise the same contract-resolution and builtin-drift paths as `Ord`, but for
`eq`-style methods rather than ordering methods.

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

Default assumption for this plan: primitive `compare` methods stay as Rust
instruction-level intrinsics in stage0 until stage0 can reliably compile and link
prelude implementation modules for bootstrapping. That gives Phase 4 a concrete
task: document and validate the intrinsic policy, not remove the intrinsics
immediately.

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

## Ordering and Priority

The phases are intentionally separable:

- Phase 1 should happen first because it locks in current behavior and catches
  regressions cheaply.
- Phase 3 can proceed before Phase 2 and is the highest-value cleanup after
  tests, because it removes the cross-contract fallback class directly.
- Phase 2 and Phase 4 can proceed in parallel once Phase 1 tests exist.
- The bootstrap fixture from Phase 1 should be kept in sync with later phases;
  there is no separate validation phase.

## Proposed Work

### Phase 1: Make drift visible

Add focused Rust tests that compile small programs through stage0 for all
primitive contract methods used by boot code.

Suggested location:

- `tests/stage0_builtin_contract_test.rs`, or
- additional cases in an existing stage0-oriented integration test if that keeps
  fixture helpers simpler.

Coverage:

- `Vector<Int>.sort()`
- `Vector<Float>.sort()`
- `Vector<String>.sort()`
- `Vector<Byte>.sort()`
- direct `Int.compare`, `Float.compare`, `String.compare`, and `Byte.compare`
- generic functions with `T: Ord` calling `a.compare(b)`
- generic functions with `T: Stringify` calling `x.to_string()`
- Eq smoke coverage once Eq contract calls are lowered through the same stage0
  path. Direct primitive Eq method tests are deferred unless/until stage0 exposes
  concrete primitive Eq methods analogous to `Int.compare` / `String.compare`.

These tests should build or run through Rust stage0, not only typecheck. They
should fail with diagnostics before codegen panics when a builtin path is
missing.

Done criteria:

- Each primitive contract method accepted by stage0 has a build/run test.
- The tests fail if a method is present in `src/types/env.rs` but absent from the
  intrinsic registry or codegen lowering.
- The tests cover both direct primitive calls and generic contract-bound calls.

### Phase 2: Centralize intrinsic metadata checks

Extend the intrinsic registry validation so each `include_in_signature_registry`
entry in `src/intrinsics/registry.rs` is checked against parsed `.tw` signature
stubs loaded by `src/intrinsics/signatures.rs`.

Implementation pointers:

- Build on the existing validation in `src/intrinsics/validate.rs`.
- Compare registry entries against signatures parsed from
  `prelude/signatures/*.tw`.
- Keep `.tw` stubs as the source of user-visible signature shape; Rust registry
  entries should be checked against them rather than silently diverging.

Validate:

- canonical name
- type parameter names and bounds
- parameter types
- return type
- dispatch kind expectations where applicable

Done criteria:

- Adding an intrinsic registry entry without a matching signature stub fails a
  Rust test.
- Changing a `.tw` signature without updating the intrinsic metadata fails a
  Rust test.
- Primitive compare signatures are covered by this validation.

### Phase 3: Make contract target lookup explicit

Avoid introducing a large resolver framework for now. There are only three
builtin contracts (`Stringify`, `Eq`, `Ord`) and each has one method. The root
bug was not lack of a general framework; it was shared fallback logic leaking
between contracts.

Replace the fallback chain in `src/ir/monomorphize.rs` with explicit lookup keyed
by `(contract, method)`.

Implementation pointers:

- Keep the logic near `resolve_contract_method_target` in
  `src/ir/monomorphize.rs` unless it grows enough to deserve a separate module.
  This function name exists as of this plan; if the monomorphizer is refactored,
  keep the replacement contract-target helper as the owner for this work.
- Introduce a small helper such as `resolve_builtin_contract_method_target` that
  matches exact pairs:
  - `("Stringify", "to_string")`
  - `("Ord", "compare")`
  - `("Eq", "eq")` or the final Eq method name when Eq lowering lands
- Do not fall back from one pair to another.
- Preserve the existing user/prelude method target lookup where it is correct;
  make the contract-specific part decide only which lookup strategy is allowed.

Done criteria:

- No contract resolution path uses a generic `.or_else(resolve_stringify_target)`
  fallback for non-`to_string` methods.
- Unsupported `(contract, method)` pairs remain unresolved and fail in the normal
  compiler path instead of being rewritten to the wrong builtin.
- Tests cover that `Ord.compare` cannot resolve to `String.to_string`.
- Eq contract tests are added when Eq reaches monomorphization/codegen.

### Phase 4: Document and reduce primitive special cases

Audit primitive methods and classify each one as one of:

- runtime import intrinsic
- instruction-level intrinsic
- ordinary prelude function compiled from Twinkle source

Implementation pointers:

- Registry and policy live in `src/intrinsics/registry.rs`.
- Instruction-level lowering lives in `src/codegen/emit.rs`.
- Signature shape lives in `prelude/signatures/*.tw` and is checked by Phase 2.
- Typechecker method visibility lives in `src/types/env.rs`.

For each instruction-level intrinsic, document why it cannot currently be an
ordinary prelude function in stage0. Where there is no ABI or bootstrap reason,
prefer ordinary prelude functions.

Default policy for now:

- Keep primitive `compare` methods as instruction-level intrinsics in stage0.
- Require semantic parity tests against the Twinkle prelude behavior, especially
  for `Float.compare` NaN ordering. Per `prelude/float.tw`, NaN compares as
  greater than all non-NaN values, and NaN compared with NaN yields Eq.

Done criteria:

- Every primitive intrinsic has an explicit classification.
- Every instruction-level primitive intrinsic has a short rationale.
- Primitive compare behavior is tested for representative Lt/Eq/Gt outcomes.
- `Float.compare` has NaN behavior coverage matching `prelude/float.tw`.

### Phase 5: Keep bootstrap-sensitive APIs in the smoke fixture

This is not a separate test category from Phase 1. It is the maintenance rule for
Phase 1's fixture: when boot starts using a newer prelude API, add a small stage0
smoke case for that API before or with the boot change.

Done criteria:

- The fixture covers recently added prelude APIs used by boot sources.
- The fixture is cheaper than `make stage2` but catches missing stage0 builtin
  metadata or lowering.
- `make stage2` remains required before landing changes that affect boot sources,
  prelude signatures, intrinsic metadata, or contract resolution.

## Open Questions

- Can primitive `compare` methods eventually become ordinary Twinkle prelude
  functions in stage0? Default for this plan: no; keep them as intrinsics until
  stage0 can compile/link prelude implementation modules reliably during
  bootstrap.
- Should contract definitions (`Stringify`, `Eq`, `Ord`) have a single machine-
  readable source shared by stage0 and boot? Revisit after Phase 3, once the
  explicit contract lookup shape is known.
- Can the generated `boot/lib/module/core_lib.tw` be eliminated or replaced with
  a more direct embedding step to reduce sync points? Revisit after Phase 2,
  when signature/registry drift checks show which generated artifacts still
  create operational risk.
- How much of stage0's type environment method table can be derived from parsed
  signature modules instead of handwritten entries? Revisit after Phase 2 and
  Phase 4 classify the remaining handwritten primitive method entries.

## Non-Goals

- Replacing stage0 with the boot compiler.
- Adding a trait system.
- Changing Twinkle's explicit contract model.
- Reworking the Wasm runtime ABI beyond what is needed for builtin consistency.
- Building a general-purpose contract resolver framework before the current
  contract set needs one.

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
