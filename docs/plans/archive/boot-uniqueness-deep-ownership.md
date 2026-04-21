# Boot uniqueness deep-ownership model

## Context

Self-hosting exposed a soundness gap in the boot optimizer's uniqueness rewrite.

The immediate failure shape was in the module compiler's environment threading:

- a fresh `ResolvedEnv` record value was produced from an existing environment
- some fields inside that new record still pointed at the caller's dict objects
- the uniqueness pass treated the updated record result as unique
- later dict updates inside `extend_types_from` became eligible for
  `dict_set_in_place`
- under the Wasm GC runtime this mutated shared heap state and poisoned the
  caller's environment

Stage1 did not expose the bug because the Rust execution path does not preserve
that aliasing pattern in the same way. Stage2 did, because record values and
container fields are separate heap objects and aliasing survives record updates.

The current `clone_env` / field-copy helpers avoid the failure by eagerly
breaking sharing before mutation. They are a useful workaround, but they are not
an optimizer-level fix.

## Problem statement

The boot uniqueness pass currently conflates two different properties:

- **fresh outer value**: a new record/variant/array shell was allocated
- **deep ownership**: mutable reference-typed state reachable through that value
  is unaliased and can be safely mutated in place

For persistent collections on Wasm GC, only the second property is sufficient
for destructive rewrites such as:

- `dict_set -> dict_set_in_place`
- `dict_remove -> dict_remove_in_place`
- `vector_set_unsafe -> vector_set_in_place`

A fresh wrapper record around shared dict fields is not enough.

## Goal

Refine the boot uniqueness model so in-place rewrites require proof of
**deep ownership** of the mutated collection base, not merely freshness of a
wrapper value.

If this lands cleanly, correctness should no longer depend on ad hoc environment
cloning helpers such as `clone_env`.

## Non-goals

This plan does not aim to:

- add user-visible ownership or borrow syntax
- add runtime refcounts or runtime uniqueness flags
- solve whole-program aliasing
- maximize rewrite coverage in the first patch
- redesign Twinkle's persistent runtime structures

The first implementation should prefer soundness and conservatism over
aggressive optimization.

## Root cause summary

The key unsound transition is:

1. a function parameter or other externally sourced value contains aliased dicts
2. a record update produces a fresh result record
3. the optimizer marks that result as "unique"
4. field-derived or subsequently threaded collection values inherit that fact too
   broadly
5. a COW update on one of those collections rewrites to an in-place mutation

The missing distinction is between owning the outer record cell and owning the
reachable collection contents.

## Design principles

### 1. Separate shell freshness from deep ownership

The optimizer should model at least two ownership strengths:

- values whose outer container is fresh but whose interior may still alias
- values whose reachable mutable state is safe for destructive update

Naming is flexible. The implementation may use:

- an enum such as `NotOwned | ShallowOwned | DeepOwned`
- or equivalent structured facts carried in the rewrite state

The important part is semantic, not nominal: collection in-place rewrites must
be gated on deep ownership.

### 2. Keep first-pass rules conservative

The initial pass should only infer deep ownership from obviously safe producers,
for example:

- `Dict.new`
- vector/array builders that allocate a fresh backing store
- collection operations already known to return a fresh copied result
- transfer from another deeply owned local via `AInit(ALocal(...))` or
  `AAssign(..., ALocal(...))`

Fresh record construction should not automatically imply deep ownership unless
all relevant reference fields are themselves proven deeply owned.

### 3. Treat reusable record-shell updates separately

`ARecordUpdate(..., can_reuse_in_place = true)` has a different proof obligation
from `dict_set_in_place`.

Reusing the outer record shell may be sound under weaker conditions than
mutating a shared dict/vector reachable from that record. The model should keep
those proof obligations distinct rather than collapsing both into one `unique`
bit.

### 4. Do not derive ownership from field extraction by default

A field read such as `ARecordGet` should not produce a deeply owned value just
because the source record was freshly allocated.

Any propagation through field extraction must be justified by deep ownership of
that field value itself, not merely freshness of the outer shell.

## Proposed model

## Ownership lattice

Introduce an ownership lattice for locals in `boot/compiler/opt/uniqueness.tw`.

Initial suggested shape:

- `None`
- `Shallow`
- `Deep`

Interpretation:

- `None`: no ownership claim; never eligible for in-place collection mutation
- `Shallow`: fresh outer aggregate may be reused as a shell, but reachable
  reference fields may alias
- `Deep`: safe to consume for collection in-place rewrites

The exact representation can be:

- `Dict<Int, OwnershipKind>`
- or split maps if that is simpler for Twinkle code generation and pattern
  matching

Recommended direction: use one map keyed by local id so the state transition
logic remains centralized.

## Producer rules

### Deep producers

Mark results as `Deep` when they come from operations that allocate fresh,
non-aliased mutable storage or return a fresh copied collection result.

Expected examples:

- empty dict/vector constructors
- collection literals whose element insertion does not preserve reusable mutable
  backing state from aliased inputs
- known COW operations that must allocate because the base was shared
- ownership transfer from an already-`Deep` local

### Shallow producers

Mark results as `Shallow` when the outer value is fresh but interior fields may
alias.

Expected examples:

- record literals containing local/reference fields not proven `Deep`
- record updates over an existing base unless the base and preserved fields are
  proven `Deep`
- wrapper values around external references

### Transfer rules

`AInit(ALocal(src))` and `AAssign(target, ALocal(src))` should transfer the full
ownership class, not just a boolean unique flag.

That means:

- `Deep -> Deep`
- `Shallow -> Shallow`
- `None -> None`

with the source losing ownership when the transfer consumes it.

## Rewrite rules

### Collection COW elimination

Require the base local to be `Deep` before rewriting:

- `dict_set`
- `dict_remove`
- `vector_set_unsafe`
- any future similar collection mutation primitive

The existing consume-reassign / dead-base checks remain necessary, but they are
not sufficient on their own.

### Record-update shell reuse

Allow `ARecordUpdate(... can_reuse_in_place = true)` under a separate predicate.

Initially, the safest path is:

- keep current shell-reuse behavior only when the base local is at least
  `Shallow`
- tighten further if backend/runtime details show stronger requirements

This keeps the plan focused on the proven bug source: collection mutation
through aliased interior fields.

## Analysis and semantics touch points

Primary files likely affected:

- `boot/compiler/opt/uniqueness.tw`
- `boot/compiler/opt/analysis.tw`
- `boot/compiler/opt/semantics.tw`
- uniqueness-focused tests in `boot/tests/suites/opt_uniqueness_suite.tw`

Potential follow-up touch points:

- optimizer README/docs describing freshness and reusable updates
- any helper logic that assumes `op_has_fresh_result` implies deep ownership

A likely cleanup is to stop treating `op_has_fresh_result` as a direct synonym
for "safe for in-place mutation later". It should instead feed ownership-class
assignment logic.

## Implementation phases

### Phase 1: Introduce ownership classes without widening optimization

Refactor rewrite state from a single `unique` set to ownership-class tracking.

Goals:

- preserve existing pass structure
- keep behavior conservative
- require `Deep` for collection in-place rewrites
- allow `Shallow` to represent fresh record shells

This phase may intentionally reduce some previously accepted rewrites if they
were relying on the unsound conflation.

### Phase 2: Reclassify core producers

Audit the producers currently routed through `op_has_fresh_result` and classify
which ones should yield:

- `Deep`
- `Shallow`
- no ownership fact

Particular attention should go to:

- `ARecord`
- `ARecordUpdate`
- collection literals
- known COW operations returning copied results

### Phase 3: Tighten propagation through wrappers and field reads

Audit helper paths that may accidentally upgrade ownership too far, including:

- wrapper-style helper rewrites
- `ARecordGet`-mediated flows
- any metadata paths that assume a fresh aggregate implies fresh contents

The first patch can simply avoid propagating through `ARecordGet` unless a
specific safe case is established.

### Phase 4: Remove workaround cloning from boot compiler call sites

Once stage2 is stable with the refined uniqueness model:

- remove `clone_env` from `module_compiler.tw`
- remove any targeted clone helpers added solely as optimizer workarounds
- confirm that environment threading remains correct under the self-hosted Wasm
  runtime

This should happen only after the optimizer-level fix is validated.

## Testing plan

### Unit / optimizer tests

Add focused tests covering the ownership distinction itself.

#### 1. Fresh wrapper around shared dict does not enable in-place dict mutation

Shape:

- start from a non-owned or tainted dict local
- place it inside a fresh record
- produce a fresh record update result
- extract/thread the dict field
- verify `dict_set` does not rewrite to `dict_set_in_place`

This is the direct guardrail for the bug class.

#### 2. Deep-owned direct dict still rewrites

Keep a positive control:

- `d = Dict.new()`
- `d2 = dict_set(d, k, v)`
- verify rewrite still happens when consume-reassign or dead-base conditions are
  satisfied

#### 3. Ownership transfer preserves class

Verify:

- `Deep` transfers stay `Deep`
- `Shallow` transfers do not become `Deep`

#### 4. Record shell reuse remains separately controlled

Add tests showing:

- fresh record/record-update values can still use `can_reuse_in_place` when that
  is intended
- collection mutation inside shared fields does not piggyback on that fact

### End-to-end self-host regression

Add or document a regression around multi-module boot compilation where the same
base environment is reused across dependency compilation.

Success condition:

- no duplicate type registration from poisoned `type_index`
- no dependence on manual environment cloning for correctness

Suggested command path:

```bash
cargo run --release -- build boot/main.tw -o target/boot-main.wasm
node tools/run_wasm_node.mjs target/boot-main.wasm -- build boot/main.tw
```

Also rerun the boot compiler test entry path used during self-host iteration.

## Success criteria

This plan is complete when:

- collection in-place rewrites require deep ownership rather than bare freshness
- fresh wrapper records around shared mutable fields no longer trigger unsound
  rewrites
- stage2 self-host no longer requires `clone_env`-style workaround cloning for
  correctness
- existing safe uniqueness wins on directly owned collections still work
- the model is documented clearly enough that future optimizer work does not
  reintroduce shallow-vs-deep ownership confusion

## Open questions

### Should `ARecordUpdate` ever produce `Deep` directly?

Probably yes in some cases, but the first implementation should be conservative.
A safe initial rule is:

- `ARecordUpdate` produces `Shallow` by default
- only upgrade to `Deep` once the base and preserved field values are modeled
  precisely enough

### How much of this should be shared with Rust stage0?

Long term, the ownership semantics should converge between Rust and boot so both
pipelines optimize the same patterns for the same reasons. The immediate need,
however, is to restore soundness in the boot optimizer and unblock removal of
workaround cloning.

### Should `op_has_fresh_result` survive as-is?

Possibly, but only as a lower-level freshness fact. Callers should not equate it
with deep ownership. If that distinction remains awkward in code, split the API
into separate freshness and ownership classification helpers.
