# Boot Signature Source of Truth Plan

## Goal

Make [`prelude/signatures/*.tw`](../../prelude/signatures/) the source of truth
for the boot compiler's user-visible builtin signatures and builtin method
shapes, while keeping
[`boot/compiler/builtins.tw`](../../boot/compiler/builtins.tw) as the execution
identity and dispatch registry.

In practice, this means:

- parse + resolve [`prelude/signatures/*.tw`](../../prelude/signatures/) in a
  signature-only bootstrap path
- seed boot `ResolvedEnv` from those resolved signatures instead of duplicating
  them in [`boot/compiler/base_env.tw`](../../boot/compiler/base_env.tw)
- derive builtin method registrations from the same signature sources instead of
  maintaining a separate hand-written method table in
  [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw)
- keep [`boot/compiler/builtins.tw`](../../boot/compiler/builtins.tw)
  responsible for `FuncId` allocation,
  runtime/intrinsic classification, ABI metadata, and internal-only helpers

This is the boot-side counterpart to the stage0 direction already implemented in
[`src/intrinsics/signatures.rs`](../../src/intrinsics/signatures.rs).

## Status

All phases complete.

### Phase 0 (complete)
Guardrail test suite added in `boot/tests/suites/base_env_guardrail_suite.tw`.
Locks current `builtin_env()` behaviour before the refactor.

### Phase 1 (complete)
Signature-only loader in `boot/compiler/signatures.tw`.
`load_signatures(dir, type_env)` parses and resolves `prelude/signatures/*.tw`
without running normal typecheck/lower/codegen.
Test suite: `boot/tests/suites/signature_loader_suite.tw`.

### Phase 2 (complete)
`builtin_env()` in `boot/compiler/base_env.tw` now loads FunctionSigs from
`prelude/signatures/*.tw` via `load_signatures` instead of a hardcoded table.
Method registrations for the signature-backed surface are derived from the same
groups and registered first (they win under `register_methods`'s dedup rule).
The hardcoded list in `builtin_env()` is reduced to entries not covered by any
signature file: I/O (`print/println/error/eprint/eprintln`), `string_substring`,
vector builders, and host ops.
`dict.tw` had a missing `get` entry — added as part of this phase.
`register_methods` was made `pub` in `resolver.tw` so `base_env.tw` can call it.

## Why Now

The boot compiler is far enough along that signature duplication is now
noticeable friction:

- [`boot/compiler/base_env.tw`](../../boot/compiler/base_env.tw) hardcodes a
  large `builtin_env()` signature table
- [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw) separately
  hardcodes builtin method registration
- the repository already has canonical signature stubs in
  [`prelude/signatures/*.tw`](../../prelude/signatures/)
- stage0 already consumes those files as signature-authoritative input

The boot compiler does **not** currently use those signature files. It still
seeds signatures from hardcoded `builtin_sig(...)` calls and method mappings.

This makes simple API edits expensive and drift-prone: adding or changing a
builtin method can require touching the signature stub, boot `base_env.tw`, boot
`resolver.tw`, and sometimes boot lowering/dispatch metadata.

## Current State

### What Boot Uses Today

- [`boot/compiler/base_env.tw`](../../boot/compiler/base_env.tw)
  - seeds builtin named types directly
  - seeds user-visible builtin functions directly via `builtin_sig(...)`
  - calls `register_builtin_methods()` at the end of `builtin_env()`
- [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw)
  - registers builtin methods with a separate hardcoded table
- [`boot/compiler/builtins.tw`](../../boot/compiler/builtins.tw)
  - assigns `FuncId`s
  - classifies runtime vs intrinsic calls
  - stores ABI metadata
  - stores internal-only callable names used by lower/codegen/optimizer

### What Stage0 Uses Today

- [`src/intrinsics/signatures.rs`](../../src/intrinsics/signatures.rs)
  - embeds [`prelude/signatures/*.tw`](../../prelude/signatures/)
  - parses and resolves them in a signature-only path
  - builds the stage0 builtin signature table from those resolved declarations

### Important Constraint

[`prelude/signatures/*.tw`](../../prelude/signatures/) are **signature stubs**,
not normal executable modules. Their bodies are placeholders and are not
suitable for normal boot typecheck/lower/codegen execution. The boot side must
therefore mirror stage0's approach:

- parse them
- resolve signatures/types from them
- do **not** typecheck/lower/codegen them as normal modules

### Coverage Boundary

[`prelude/signatures/*.tw`](../../prelude/signatures/) cover the user-visible
builtin/intrinsic callable surface such as:

- `String.len`
- `Vector.push`
- `Dict.new`
- `Cell.set`
- `range_from`
- `Iterator.unfold`

They do **not** cover all boot-side callable identities. Boot still needs
separate handling for:

- internal-only helpers like `vector_builder_*`
- optimizer-only/in-place variants like `vector_set_in_place`
- host bindings like `host_read_file`
- lowering/runtime bridge names that are not part of the user-visible API

Those should stay in
[`boot/compiler/builtins.tw`](../../boot/compiler/builtins.tw).

## Design

### Split Responsibilities Clearly

After this plan lands, the boot compiler should have two distinct bootstrap
sources:

1. Signature source: [`prelude/signatures/*.tw`](../../prelude/signatures/)
2. Dispatch/identity source:
   [`boot/compiler/builtins.tw`](../../boot/compiler/builtins.tw)

The signature source answers:

- what user-visible builtin functions exist
- what their type parameters, parameters, and return types are
- which method names are attached to builtin receiver types

The dispatch/identity source answers:

- what `FuncId` each builtin identity gets
- whether that identity is runtime-backed or intrinsic-backed
- what ABI/runtime metadata it needs
- what internal-only builtin identities exist

### Keep `builtins.tw` as the Mapping Layer

There is still a mapping problem even after deduplicating signatures:

- signature files expose canonical Twinkle names like `String.len`,
  `Vector.push`, `Dict.new`
- the current boot compiler internals still use legacy/internal builtin names
  like `string_len`, `vector_push`, `dict_new`

Instead of inventing a second registry file, keep this bridge in
[`boot/compiler/builtins.tw`](../../boot/compiler/builtins.tw).

Target shape:

- every builtin entry may carry both:
  - a canonical public name used by resolver/checker/language-facing bootstrap
  - an internal dispatch name used by lower/codegen/runtime glue
- internal-only builtins may omit a public name entirely
- boot lower/codegen/optimizer continue to target internal dispatch names or
  `FuncId`s, whichever is already less disruptive

This keeps identity and execution metadata centralized while removing signature
duplication from unrelated files.

### Do Not Load Signature Stubs Through Normal Prelude Injection

[`prelude/signatures/*.tw`](../../prelude/signatures/) should **not**
participate in ordinary prelude module auto-injection.

They are bootstrap metadata, not runtime/user modules. The existing multi-module
prelude injector should continue to work with normal `prelude/*.tw` modules
only.

## Non-Goals

- Replacing [`boot/compiler/builtins.tw`](../../boot/compiler/builtins.tw)
  entirely
- Making [`prelude/signatures/*.tw`](../../prelude/signatures/) executable boot
  modules
- Solving all remaining boot multi-module pipeline wiring in the same change
- Removing internal-only builtin identities from boot codegen/optimizer
- Converting host ABI metadata to come from `.tw` signature stubs

## Implementation Plan

### Phase 0: Inventory and Guardrails

Files:

- [`boot/compiler/base_env.tw`](../../boot/compiler/base_env.tw)
- [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw)
- [`boot/compiler/builtins.tw`](../../boot/compiler/builtins.tw)
- [`prelude/signatures/*.tw`](../../prelude/signatures/)

Changes:

1. Inventory the boot builtin signatures currently hardcoded in
   `base_env.tw`.
2. Inventory the builtin method registrations currently hardcoded in
   `resolver.tw`.
3. Classify each item into one of:
   - signature-backed and should come from
     [`prelude/signatures/*.tw`](../../prelude/signatures/)
   - internal-only and should stay in
     [`builtins.tw`](../../boot/compiler/builtins.tw)
   - pure-prelude and should come from normal prelude modules, not the
     signature bootstrap
4. Add characterization tests for the currently supported builtin call forms:
   - module-qualified builtin calls
   - dot-method builtin calls
   - first-class builtin method references where already supported

Notes:

- This phase should explicitly call out any existing resolver method entries
  that do not correspond to a boot-visible `FunctionSig` today.
- The inventory is a prerequisite for not accidentally deleting behavior during
  the swap.

Exit criteria:

- a checked-in inventory comment/doc exists
- tests lock current behavior before refactor

### Phase 1: Signature-Only Bootstrap Loader

Files:

- [`boot/compiler/signatures.tw`](../../boot/compiler/signatures.tw) (new)
- [`boot/compiler/base_env.tw`](../../boot/compiler/base_env.tw)

Changes:

1. Add a boot-side helper that:
   - locates [`prelude/signatures/*.tw`](../../prelude/signatures/)
   - parses each file
   - resolves only declarations/signatures against the builtin type seed
   - returns resolved `FunctionSig`s plus enough metadata to derive method
     registrations
2. Mirror stage0's bootstrap rule: parse + resolve only, no body typechecking
   and no lowering/codegen.
3. Decide method ownership by signature module:
   - [`prelude/signatures/string.tw`](../../prelude/signatures/string.tw) =>
     receiver `String`
   - [`prelude/signatures/vector.tw`](../../prelude/signatures/vector.tw) =>
     receiver `Vector`
   - [`prelude/signatures/dict.tw`](../../prelude/signatures/dict.tw) =>
     receiver `Dict`
   - [`prelude/signatures/cell.tw`](../../prelude/signatures/cell.tw) =>
     receiver `Cell`
   - [`prelude/signatures/iterator.tw`](../../prelude/signatures/iterator.tw) =>
     receiver `Iterator`
   - [`prelude/signatures/int.tw`](../../prelude/signatures/int.tw) =>
     receiver `Int`
   - [`prelude/signatures/float.tw`](../../prelude/signatures/float.tw) =>
     receiver `Float`
   - [`prelude/signatures/bool.tw`](../../prelude/signatures/bool.tw) =>
     receiver `Bool`
   - [`prelude/signatures/byte.tw`](../../prelude/signatures/byte.tw) =>
     receiver `Byte`
   - [`prelude/signatures/range.tw`](../../prelude/signatures/range.tw) =>
     free functions (`range`, `range_from`, `range_step`)

Exit criteria:

- boot has a reusable signature loader module
- loader output is deterministic
- signature stubs are never passed into normal boot typecheck/lower/codegen

### Phase 2: Replace Hardcoded Boot Signature Seeding

Files:

- [`boot/compiler/base_env.tw`](../../boot/compiler/base_env.tw)
- [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw)

Changes:

1. Replace the hardcoded user-visible builtin `FunctionSig` list in
   `builtin_env()` with signatures loaded from
   [`prelude/signatures/*.tw`](../../prelude/signatures/).
2. Replace builtin method registrations that correspond to those signatures with
   data derived from the signature loader.
3. Keep builtin named type seeding (`Option`, `Result`, `Cell`, `Range`,
   `Iterator`, `IterItem`, `UnfoldStep`, `Order`) as explicit boot bootstrap
   data for now.
4. Leave internal-only helpers and host functions out of the signature-derived
   env unless they intentionally belong to the language-visible builtin API.

Notes:

- This phase should shrink `base_env.tw` substantially.
- `resolver.tw` should stop hand-maintaining method registrations for the
  signature-backed builtin surface.

Exit criteria:

- changing a signature in [`prelude/signatures/*.tw`](../../prelude/signatures/)
  updates boot builtin env without editing
  [`base_env.tw`](../../boot/compiler/base_env.tw)
- changing a builtin method in
  [`prelude/signatures/*.tw`](../../prelude/signatures/) updates boot method
  shape without editing [`resolver.tw`](../../boot/compiler/resolver.tw)

### Phase 4 (complete)
Drift test suite added in `boot/tests/suites/signature_drift_suite.tw`.
Three bidirectional drift tests close the loop between `prelude/signatures/*.tw`,
`builtin_env()`, and `make_builtin_registry().by_canonical`.
Drift detection caught that `Int.from_string` and `Float.from_string` were missing
from the canonical registry — fixed in `builtins.tw`.
Regression coverage added for module-qualified, dot-method, and free-function
builtin call forms.

### Phase 3: Canonical-to-Internal Mapping in `builtins.tw`

Files:

- [`boot/compiler/builtins.tw`](../../boot/compiler/builtins.tw)
- [`boot/compiler/lower_core.tw`](../../boot/compiler/lower_core.tw)
- [`boot/compiler/codegen/emit.tw`](../../boot/compiler/codegen/emit.tw)
- [`boot/compiler/opt/pipeline.tw`](../../boot/compiler/opt/pipeline.tw)

Changes:

1. Extend `BuiltinEntry` with the minimum metadata needed to relate:
   - canonical public name, for example `String.len`
   - internal dispatch name, for example `string_len`
2. Add lookup helpers for:
   - canonical name -> builtin entry / `FuncId`
   - internal dispatch name -> builtin entry / `FuncId`
3. Update boot lowering/bootstrap sites so they resolve builtin identities
   through this shared mapping rather than assuming only legacy names.
4. Keep internal-only entries legal:
   - no public name required
   - still fully registered for optimizer/codegen use

Notes:

- This phase is the core reason a separate registry file is unnecessary.
- The bridge belongs next to `FuncId` ownership and ABI/runtime metadata.

Exit criteria:

- user-visible builtin identities can be addressed canonically
- boot lowering/codegen still has stable access to internal dispatch names
- canonical and internal names do not drift independently

### Phase 4: Validation and Drift Tests

Files:

- `boot/tests/suites/*`

Changes:

1. Add tests proving the boot compiler seeds builtin signatures from
   [`prelude/signatures/*.tw`](../../prelude/signatures/).
2. Add drift tests that fail if a signature-backed builtin exists in boot
   hardcoded tables but not in the signature sources.
3. Add mapping validation tests:
   - every public builtin signature used by boot resolves to a `BuiltinEntry`
   - internal-only builtins are explicitly marked as such
4. Add regression coverage for:
   - module-qualified builtin calls
   - dot-method builtin calls
   - free builtin functions like `range_from`
   - mixed canonical/internal lookup paths where boot lowering still depends on
     internal names

Exit criteria:

- signature drift is caught by tests
- boot and stage0 agree on the intended builtin signature surface

## Suggested Landing Order

1. Add Phase 0 inventory + guardrails.
2. Land the signature-only loader.
3. Switch `builtin_env()` to signature-derived seeding.
4. Switch builtin method registration to signature-derived data.
5. Add canonical/internal mapping in `builtins.tw`.
6. Clean up residual duplicated boot tables.

This order keeps the refactor low-risk: bootstrap visibility first, dispatch
identity second.

## Open Questions

1. Should boot derive builtin receiver ownership from filename alone, or should
   the loader carry explicit module alias metadata like stage0 does?
2. Which current `register_builtin_methods()` entries are actually meant to come
   from normal prelude modules rather than builtin bootstrap?
3. Do we want canonical names to become the primary names in boot lowering, or
   is it better to keep internal names there and only bridge at bootstrap/env
   boundaries for now?

## Completion Criteria

This plan is complete when:

1. Boot user-visible builtin signatures come from
   [`prelude/signatures/*.tw`](../../prelude/signatures/).
2. Boot builtin method shape for that signature-backed surface comes from the
   same source.
3. [`boot/compiler/base_env.tw`](../../boot/compiler/base_env.tw) no longer
   duplicates that signature surface by hand.
4. [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw) no longer
   duplicates that method surface by hand.
5. [`boot/compiler/builtins.tw`](../../boot/compiler/builtins.tw) remains the
   sole source of `FuncId`/dispatch/ABI identity, including the
   canonical-to-internal mapping.
