# Boot first-class builtin function support

## Context

The boot backend already has several pieces of higher-order function support, but
those pieces do not currently line up around one explicit model of a global
function value.

Today it already knows how to handle:

- direct calls to user functions
- direct calls to builtin / prelude functions
- closure conversion for user functions with prepared bodies
- some higher-order call edges where `AGlobalFunc(fid)` is passed to a
  function-typed parameter

The missing part is not "higher-order support from scratch". The missing part
is consistent ownership for this case:

- builtin / prelude functions flowing through closure-typed boundaries as
  first-class values

This shows up in examples like:

```tw
xs.map(Int.to_string)
Iterator.unfold(0, step)
c.update(inc)
```

The immediate failure is currently:

```text
lookup_func_sym: unknown FuncId 5
```

where `FuncId 5` is `int_to_string`.

That crash happens because emission still assumes that any closure-capable
`FuncId` behaves like a prepared user function with a normal function symbol and
prepared body.

## What already exists

The current backend is closer to supporting this than the failing crash might
suggest.

### Planner-side higher-order tracking already exists

`boot/compiler/codegen/wasm_plan_scan.tw` already tracks:

- `closure_funcs` from explicit `AMakeClosure`
- `ho_global_funcs` when an `AGlobalFunc(fid)` is passed to a function-typed
  user parameter or closure-shaped builtin ABI parameter
- `builtin_calls` for direct builtin calls

`boot/compiler/codegen/wasm_plan_impl.tw` also already tries to register
higher-order function signatures via:

- `register_func_sig_by_id(...)`
- `collect_higher_order_global_func_sigs_op(...)`

The real gap is that this registry path only works for prepared user functions.
When the target is a builtin `FuncId`, the planner has no equivalent source of
semantic function signature metadata.

### Verifier support is partially there

`boot/compiler/backend/verify.tw` already permits `AGlobalFunc(_)` at closure ABI
edges in `verify_atom_coercible_to_val_type(...)` when the expected ABI type is
`rt_types__Closure`.

But the verifier still treats explicit closure construction as user-only:

- `AMakeClosure(fid, free_vars)` currently rejects targets that are not present
  in `funcs_by_id`
- `infer_atom_mono(.AGlobalFunc(fid), ...)` only knows how to infer a function
  mono for prepared user functions

So the verifier already understands one important call-edge exception, but it
still does not own a general rule for "this global function value is allowed to
be materialized as a closure".

### Emission is still the hard failure point

`boot/compiler/codegen/emit.tw` is where the user-only assumption remains most
visible:

- `emit_make_closure(...)` always calls `lookup_func_sym(...)`
- trampoline emission is driven from `PreparedFunc`
- several call sites still perform ad hoc `AGlobalFunc -> closure` wrapping
- plain `emit_atom(.AGlobalFunc(fid), ...)` still emits raw `ref.func(sym)`

That last point matters beyond call arguments: if a function value is stored in a
record, array, option, result, or returned directly, a pure emit-time strategy
has no reliable expected-type hook to decide that `AGlobalFunc(fid)` must become
a closure value rather than a raw function reference.

## Additional constraints from the current backend

The current failure is only the first visible symptom. The backend also has
three deeper constraints that the implementation plan must address explicitly.

### 1. Function-valued storage is currently typed as a concrete closure subtype

Today function monos do not lower to base `rt_types__Closure` in general.
Instead:

- `repr_assign.tw` maps `Function(...)` to `ClosureRef`
- `wasm_layout.tw` lowers `Function(params, ret)` to `ref null $closure_<sig>`
- `emit_make_closure(...)` only falls back to base `rt_types__Closure` when no
  concrete signature is registered

That means the old idea of "builtin closures can be universal-only base
closures, and we only need to fix indirect calls" is incomplete.

Even if indirect calls use the universal path, a builtin closure materialized as
plain `rt_types__Closure` still cannot be safely stored in a local, field,
variant payload, or array element whose Wasm type is the narrower typed closure
subtype.

Any correctness-first plan therefore needs an explicit representation rule for
function values, not only a call rule.

### 2. Builtin ABI metadata is not enough

`BuiltinEntry` currently carries:

- builtin kind
- runtime import info when applicable
- Wasm ABI contract

That is not the same thing as the semantic Twinkle function signature.

For first-class builtin closures, the backend also needs the semantic parameter
and return monos for tasks such as:

- inferring the mono of `AGlobalFunc(fid)` in verifier logic
- deciding where `AGlobalFunc(fid)` must become `AMakeClosure(fid, [])`
- boxing and unboxing arguments/results in universal trampolines
- optionally registering typed closure layouts later

Prepared user functions already provide that information through `PreparedFunc`.
Builtin targets currently do not have a backend-owned equivalent.

### 3. Direct-call lowering and closure lowering are not the same thing

Some builtins have bespoke call lowering that goes beyond their ABI contract.
Examples already present in `emit.tw` include:

- builder-region casts for `vector_builder_*`
- host call argument/result shims such as `host_write_bytes`
- intrinsic lowering such as `cell_update` and `iterator_unfold`

So a generic rule like "emit builtin universal trampolines from builtin ABI
metadata" is only sound for a subset of builtins.

The backend needs to distinguish:

- builtins that are direct-callable
- builtins that are closure-materializable
- builtins that can use a generic universal closure trampoline
- builtins that require a dedicated closure wrapper trampoline
- builtins that are not closure-materializable yet

Without that distinction, the plan risks replacing one user-only assumption with
a too-broad builtin-only assumption.

## Problem statement

The backend has the ingredients for higher-order support, but they are split
across incompatible ownership models:

- planner can notice some higher-order global function flows
- verifier allows some closure-ABI edges
- emitter still assumes closure materialization is a prepared-user-function
  feature
- function-valued storage is currently typed as a concrete closure subtype
- builtin semantic signatures are not owned anywhere in backend data
- builtin direct-call shims are currently separate from closure lowering

This causes four concrete problems.

### 1. Closure-capable is still conflated with prepared user function

A builtin `FuncId` can be callable and valid at a closure boundary even when it
has:

- no `PreparedFunc`
- no entry in `prepared_funcs`
- no entry in `func_sym_map`

The emitter still treats those conditions as if they were prerequisites for
closure construction.

### 2. Closure materialization is too implicit

Today `AGlobalFunc(fid)` is left in the IR and several emit-time call sites try
to wrap it opportunistically when they notice a closure-shaped boundary.

That works only for a subset of argument-passing sites. It does not provide a
clear path for:

- storing a builtin function value in data
- returning a builtin function value
- moving a builtin function value through locals before use

### 3. The current typed-storage representation conflicts with universal-only builtin closures

The tempting minimal fix is to emit builtin closures only as base
`rt_types__Closure` values. But the backend currently represents
`Function(...)` values as typed closure subtype refs in storage and slot types.

So a builtin closure emitted only as base `rt_types__Closure` would still be
incompatible with existing Wasm field/local signatures, even before any indirect
call happens.

### 4. Builtin closure support needs semantic signature ownership and lowering policy

The backend cannot correctly materialize or call builtin closures using only:

- prepared-user metadata
- raw builtin ABI metadata

It needs one shared model that also answers:

- what semantic function signature this builtin has
- whether it is closure-materializable at all
- whether generic universal trampoline lowering is sound
- whether a dedicated wrapper trampoline is required

## Goal

Make builtin / prelude functions first-class values in the boot backend with one
explicit backend rule for when a global function becomes a closure.

Concretely:

- passing a builtin function where a closure value is expected must work
- storing or returning builtin function values must work through an explicit IR
  path rather than emit-time guesswork
- planner, verifier, and emitter must agree on which `FuncId`s are
  closure-materializable
- closure construction must not rely on user-only symbol maps
- the backend must own semantic function signatures for builtin targets used as
  values
- the correctness path must use a closure representation that can actually flow
  through storage and indirect calls safely
- builtin support must start from a deliberately supported subset rather than
  assuming every builtin ABI is closure-safe

## Non-goals

This plan does not aim to:

- redesign the surface function type system
- replace prepared backend IR with stage0's architecture
- solve every typed-closure optimization case up front
- unify all builtin and intrinsic lowering in one patch
- commit to a final specialized representation for builtin closures
- make every runtime helper or intrinsic closure-materializable in the first
  pass

Correctness and ownership come first.

## Design decisions

## Decision 1: use explicit closure materialization, not emit-time guessing

The backend should stop treating `AGlobalFunc(fid)` as a value that emission can
silently reinterpret later.

Instead, closure boundaries should be made explicit in IR by materializing a
closure value before codegen reaches raw emission.

### Recommended representation

Use the existing explicit closure op:

- `AMakeClosure(fid, free_vars)`

and extend its meaning from:

- "make a closure for a prepared user function with captures"

to:

- "make a closure for any closure-materializable callable target"

For builtin globals, that becomes:

- `AMakeClosure(fid, [])`

This is preferable to keeping `AGlobalFunc(fid)` all the way to emission
because it gives one explicit IR marker for:

- call-argument adaptation
- storage in records / variants / arrays
- assignment through locals
- return / break of function values

It also lets existing planner logic around `AMakeClosure` become more central
instead of adding more emit-time special cases.

## Decision 2: introduce one callable-target query shared by planner, verifier, and emitter

The backend needs a single query layer for `FuncId` classification.

Conceptually it should answer:

- is this a prepared user function?
- is this a builtin / prelude entry?
- what is its semantic function signature?
- is it direct-callable?
- is it closure-materializable?
- if materialized as a closure, what trampoline family exists?
- can it use the generic universal trampoline path?
- does it require a dedicated closure wrapper trampoline?
- does it have typed closure support, or universal-only support?

A conceptual shape is:

- `UserFunc(PreparedFunc)`
- `BuiltinFunc(BuiltinCallableInfo)`

Where `BuiltinCallableInfo` owns at least:

- builtin entry
- semantic parameter monos
- semantic return mono
- closure materialization policy
- wrapper lowering kind for closure emission when needed
- typed-closure support policy
- direct-call lowering policy

A useful refinement, matching the direction stage0 suggests, is to keep these as
separate backend facts rather than collapsing them into one broad
"wrapper-needed" bit. In particular, the backend should be able to distinguish:

- whether a callable may be materialized as a closure at all
- whether closure lowering is generic or wrapper-backed
- which wrapper lowering kind owns that closure lowering
- whether typed closure specialization is available
- how direct calls to the same builtin are lowered

The important outcome is not the tag type itself. The important outcome is that
planner, verifier, and emitter stop re-deriving callable facts from unrelated
maps such as `prepared_funcs`, `func_sym_map`, or builtin ABI tables.

## Decision 3: use a stage0-style hybrid closure strategy

Stage0 is the right reference point here.

It does **not** solve first-class builtin functions by forcing every function
value through one universal-only representation forever. Instead, it combines:

- a universal base closure ABI for correctness and interoperability
- typed closure subtypes and typed call paths where a concrete signature is
  known and worth specializing
- explicit wrapper/trampoline ownership for builtin and prelude function values

Boot should follow that same direction.

For the correctness path:

- every closure value must remain valid as a base `rt_types__Closure`
- builtin function values must be materializable without assuming a prepared
  user body
- typed closure specialization should remain available where the backend can
  prove the concrete closure signature and representation
- the universal closure ABI should be the fallback interoperability path, not
  the only intended long-term path

This avoids the current boot bug without baking in an unnecessary performance
regression relative to stage0.

### Consequence for the correctness pass

The backend should stop conflating these three questions:

- can this callable be materialized as a closure at all?
- can this callable use the universal closure ABI?
- can this callable use a typed closure specialization?

The end state should allow:

- storage and interop through the base closure ABI when needed
- typed closure structs and typed indirect calls where representation proof is
  explicit
- builtin wrappers/trampolines that can participate in either path according to
  declared policy

## Decision 4: semantic function signatures for builtins must become backend-owned data

The backend needs a first-class source of semantic signatures for builtin
callables.

That source should be available anywhere `PreparedFunc` signatures are currently
used, especially in:

- planner signature registration
- verifier mono inference
- closure-boundary rewriting
- trampoline boxing and unboxing

A practical first implementation is:

- resolve builtin semantic signatures from `ResolvedEnv` using the builtin's
  registered function name or canonical mapping
- normalize them into backend-owned callable metadata before planning/emission

The important point is ownership: once backend preparation finishes, later
passes should not have to rediscover builtin semantic signatures ad hoc.

## Decision 5: direct-call lowering and closure lowering are separate capabilities

A builtin being callable directly does not imply that it is immediately safe to
materialize as a first-class closure.

For each builtin target, the backend should track closure-facing policy as
separate data, for example:

- closure materializable vs not materializable
- generic closure lowering vs wrapper-backed closure lowering
- wrapper lowering kind such as `ByteToString`, `CellUpdate`, `IteratorUnfold`
- typed closure support availability

And separately a direct-call policy such as:

- runtime import call
- intrinsic inline lowering
- bespoke ABI shimmed runtime call

This separation matters because stage0's shape is not "one special-case bit per
builtin". It is "one central callable model, plus explicit lowering kinds for
exceptional callable families".

This lets the implementation start with a small safe set of builtin function
values, such as the `to_string` family, while explicitly deferring more exotic
entries until their wrapper story is defined.

## Decision 6: planner should track explicit closure materialization requirements

Planner support should pivot from "notice some higher-order edges" to
"understand explicit closure materialization sites".

Once closure-boundary rewriting inserts `AMakeClosure(fid, [])` for global
function values at closure boundaries, the planner can use that as the primary
signal for trampoline requirements.

Existing `ho_global_funcs` scanning can remain temporarily as compatibility or
assertion support during the migration, but the end state should be:

- closure trampoline requirements come from explicit closure materialization
- direct-call builtin tracking remains separate
- typed closure support and closure support in general are not treated as the
  same fact

## Decision 7: verifier should validate closure-materializable targets, not just prepared funcs

`AMakeClosure` validation should no longer mean "target must be present in
`funcs_by_id`".

Instead it should mean:

- the target `FuncId` is known through the shared callable query
- the target is closure-materializable under that shared policy
- the capture list matches the callable kind

That implies:

- user functions still validate against closure capture metadata
- builtin globals require zero captures
- unsupported builtin targets are rejected explicitly with backend terms

This moves verifier ownership to the real rule rather than a user-function
proxy.

## Closure boundary insertion strategy

A dedicated rewrite should make closure boundaries explicit before prepared IR is
emitted.

This may extend `insert_boundaries.tw` or land as a new nearby preparation pass,
but it should be treated as a distinct responsibility from anyref wrap/unwrap
insertion.

Its key job is:

> whenever a value with atom form `AGlobalFunc(fid)` flows into a place whose
> semantic type is `Function(...)` or whose backend storage / ABI contract
> requires a closure value, rewrite that flow into `AMakeClosure(fid, [])`.

That rewrite must cover more than direct call arguments.

### Boundaries to rewrite

At minimum:

- arguments to user functions whose parameter mono is `Function(...)`
- arguments to builtins / intrinsics whose semantic parameter mono is
  `Function(...)`
- arguments to builtins / intrinsics whose ABI param type is `rt_types__Closure`
  when semantic param metadata is not otherwise attached
- `AInit` and `AAssign` into locals whose semantic mono is `Function(...)`
- record / variant / array construction where a field or element mono is
  `Function(...)`
- return / break paths whose semantic type is `Function(...)`

The preferred source of truth is semantic mono information, not raw ABI shape.
ABI-only closure detection should remain a compatibility fallback for places
that have not yet been converted to semantic callable metadata.

## Representation strategy

## Initial correctness strategy: hybrid base-closure interoperability with typed specialization

For the first correctness pass:

- explicit closure materialization still inserts `AMakeClosure(fid, [])` at real
  closure boundaries
- every materialized closure must remain interoperable through base
  `rt_types__Closure`
- typed closure structs should remain available where the backend already has a
  concrete closure signature and a sound typed path
- indirect closure calls should prefer a typed path when representation proof is
  explicit and otherwise fall back to the universal closure ABI

Why this is the right first step:

- it matches stage0 more closely
- it preserves a path to good indirect-call performance
- it avoids treating universal-only lowering as the permanent representation
  contract
- it still makes builtin closure support explicit and backend-owned

### Important note on semantic typing

This does **not** change the semantic mono of function values.

The semantic type remains `Function(params, ret)`. What changes in the
correctness pass is how the backend decides between:

- base closure interoperability
- typed closure specialization
- wrapper-based builtin closure support

## Initial builtin support scope: a safe, explicit subset

The first implementation should not claim all builtins are closure-safe.

Start with builtins whose direct-call and closure-call stories are already simple
and whose universal trampolines can be derived without bespoke wrapper logic.
Examples include:

- `Int.to_string`
- `Float.to_string`
- `Bool.to_string`
- `String.to_string`
- likely `Byte.to_string` only if its current intrinsic lowering is given an
  explicit wrapper policy rather than silently reused

Entries with bespoke call-lowering contracts should remain explicitly excluded
until their closure wrapper story is implemented, for example:

- `vector_builder_*`
- host helpers with argument/result shims
- intrinsics like `Cell.update` and `Iterator.unfold` if they need dedicated
  closure wrapper behavior rather than generic ABI lowering

## Later optimization: typed closure specialization

After correctness lands, the backend can reintroduce typed closure
specialization where it has enough proof.

That may include:

- restoring typed storage for function values proven to stay within one closure
  layout family
- restoring typed indirect calls where representation guarantees are explicit
- emitting typed builtin closure structs when their signatures are concrete and
  worth specializing

That work should be treated as follow-up optimization, not part of the minimal
correctness fix.

## Proposed implementation phases

## Phase 0: inventory callable targets and choose the supported builtin subset

Primary work:

- audit builtin entries by closure feasibility
- separate direct-callability from closure-materializability
- mark an initial supported subset for first-class builtin function values
- document excluded builtins and why they are excluded for now

Exit criteria:

- the plan names which builtin entries are in scope for the first correctness
  pass
- unsupported builtin closures fail explicitly rather than accidentally

## Phase 1: establish callable-target ownership including builtin semantic signatures

Primary work:

- introduce shared callable-target queries for `FuncId`
- classify user functions and builtin entries through one backend helper
- attach semantic function signatures to builtin callable metadata
- define closure-materializable vs direct-callable as explicit backend facts
- define closure policy separately from direct-call lowering policy

Likely data owned here:

- callable kind
- semantic params / return
- direct-call lowering mode
- closure lowering mode
- runtime import metadata when relevant

Exit criteria:

- backend code stops using `prepared_funcs` membership as a proxy for closure
  support
- planner, verifier, and emitter can all ask the same callable questions
- builtin semantic signatures are available without ad hoc rediscovery in later
  passes

## Phase 2: switch the correctness representation of function values to base closure

Primary work:

- change backend representation assignment so `Function(...)` values store as
  base `rt_types__Closure` in the correctness path
- update `val_type_of_mono` / related helpers or add backend-specific helpers so
  prepared slot and field Wasm types reflect that rule where needed
- align verifier physical-edge checks with the widened closure representation
- keep direct calls to `AGlobalFunc(fid)` unchanged

Primary files:

- `boot/compiler/backend/repr_assign.tw`
- `boot/compiler/codegen/wasm_layout.tw`
- `boot/compiler/backend/verify.tw`
- any planner helpers that assume `Function(...)` implies typed closure subtype

Exit criteria:

- function-valued locals, params, fields, and payloads can hold base closure
  values safely
- universal-only builtin closures are no longer rejected by representation
  mismatch before they are even called

## Phase 3: make closure materialization explicit in IR

Primary work:

- add a dedicated rewrite for closure boundaries
- rewrite `AGlobalFunc(fid)` to `AMakeClosure(fid, [])` at function-valued and
  closure-storage boundaries
- cover storage, assignment, and return boundaries in addition to call
  arguments
- prefer semantic parameter/result monos over ABI-only inference

Migration note:

- existing emit-time call-site wrapping may remain temporarily, but only as
  compatibility scaffolding while the explicit rewrite lands

Exit criteria:

- function-to-closure adaptation is represented explicitly in IR
- plain storage / return of builtin function values no longer depends on
  emitter guesswork

## Phase 4: extend verifier and planner to the new ownership model

Primary work:

- make `AMakeClosure` verifier logic accept supported builtin targets
- require zero captures for builtin globals
- use shared callable metadata for `AGlobalFunc` mono inference where possible
- pivot planner closure support to explicit `AMakeClosure` usage
- keep direct builtin-call planning separate from closure trampoline planning
- stop treating typed closure support and closure support in general as the same
  fact

Exit criteria:

- verifier accepts supported builtin closure materialization and rejects
  unsupported targets clearly
- planner can answer which builtin and user `FuncId`s need closure trampoline
  support from explicit materialization sites

## Phase 5: add builtin universal closure trampolines for the supported subset

Primary work:

- emit builtin universal trampolines from callable metadata, not `PreparedFunc`
- construct builtin closures without `lookup_func_sym(...)`
- keep builtin closure env null / empty
- box and unbox arguments/results from semantic signature metadata
- reject or defer builtin targets whose closure mode requires wrappers not yet
  implemented

Implementation note:

- runtime builtins in the initial supported subset should lower through owned
  import symbol lookup
- intrinsic or shimmed builtins should not be force-fit into the generic path;
  they either need a dedicated wrapper mode or must remain out of scope

Exit criteria:

- builtin closure construction no longer relies on user symbol maps or prepared
  bodies
- `AMakeClosure(fid, [])` works for the explicitly supported builtin subset

## Phase 6: make universal indirect calls the correctness path

Primary work:

- stop assuming every function-typed callee value has a typed closure layout
- route indirect closure calls through the universal path by default
- keep typed closure calls only where representation proof is explicit

Exit criteria:

- builtin closures stored in function-typed slots can be called safely later
- typed closure layout assumptions no longer determine correctness

## Phase 7: add wrapper-based closure support for harder builtin targets

Primary work:

- extend callable metadata with dedicated wrapper trampoline modes
- add closure wrappers for builtins whose direct-call lowering uses bespoke ABI
  shims or intrinsic lowering
- widen the supported builtin subset deliberately, one family at a time

Examples of likely later candidates:

- `Byte.to_string`
- `Cell.update`
- `Iterator.unfold`
- host helpers with argument/result shims

Exit criteria:

- builtin closure support grows through explicit wrapper ownership rather than
  accidental emitter reuse
- supported and unsupported builtin targets remain clearly classified

## Phase 8: remove bespoke wrapping paths and harden coverage

Primary work:

- replace call-site-specific `AGlobalFunc -> closure` wrapping with the explicit
  closure-boundary rewrite as the primary model
- simplify emission helpers around runtime and intrinsic closure arguments
- add focused regression coverage for stored, returned, and indirectly called
  builtin function values

Exit criteria:

- closure-boundary adaptation has one primary implementation path
- remaining special cases are documented as intentional ABI shims rather than
  callable-model patches

## Tests to add

### 1. User higher-order call with builtin function value

Examples:

```tw
xs.map(Int.to_string)
xs.map(Float.to_string)
```

These confirm that builtin globals passed to user higher-order functions are
materialized explicitly and survive indirect call emission.

### 2. Builtin function stored then called later

Add at least one case where a builtin function value is:

- bound to a local
- stored in a record / option / result / collection
- later called through a function-typed path

This is the key characterization test for why explicit closure materialization
and the widened closure representation are both necessary.

### 3. Return-path characterization

Add a case where a function returns a builtin function value and another
function consumes it through a higher-order path.

This guards the return-boundary rewrite directly.

### 4. Verifier characterization

Add tests that distinguish:

- supported builtin globals accepted at `AMakeClosure`
- unsupported builtin targets rejected explicitly
- builtin `AMakeClosure` with non-empty captures rejected explicitly

### 5. User closures remain correct under the universal representation path

Add tests showing that existing user closures still work when the correctness
path uses base closure storage and universal indirect calls.

This protects against regressions while typed closure specialization is deferred.

### 6. Deferred wrapper-mode coverage

When wrapper-based builtin closure support lands, add targeted tests for each new
family rather than broad generic coverage.

Examples:

```tw
Iterator.unfold(0, step)
Cell.update(c, inc)
```

These should be enabled only when those builtin families are explicitly moved
into the supported subset.

### 7. Self-host regression checks

Use the preferred fast self-host workflow from the repository guidance:

```bash
cargo run --release -- build boot/main.tw -o target/boot-main.wasm
node tools/run_wasm_node.mjs target/boot-main.wasm -- build boot/main.tw
node tools/run_wasm_node.mjs target/boot-main.wasm -- run boot/tests/main.tw
```

For targeted example regressions, keep using the same runner style:

```bash
node tools/run_wasm_node.mjs target/boot-main.wasm -- run examples/api_ergonomics.tw
```

The goal is to verify that the current failure class disappears and that later
failures, if any, represent new blockers rather than callable-model drift.

## Success criteria

This plan is complete when:

- builtin / prelude functions can cross closure-typed boundaries without backend
  crashes
- closure materialization happens through explicit IR rather than emit-time
  guesswork
- planner, verifier, and emitter share one explicit model of callable targets
  and closure-materializable targets
- builtin semantic function signatures are owned by backend callable metadata
- the correctness representation allows function values to be stored, returned,
  and called safely through the base closure ABI
- builtin closure construction no longer assumes a prepared user symbol or body
- unsupported builtin targets fail explicitly according to declared policy
- self-hosted boot compilation advances through the supported first-class
  builtin cases
- regression coverage protects stored, returned, and indirectly called builtin
  function values

## Implementation checklist by file

This section translates the revised plan into a concrete worklist so
implementation can proceed without rediscovering ownership boundaries.

### 1. Shared callable-target queries and builtin semantic signatures

**Primary files**

- `boot/compiler/codegen/emit.tw`
- `boot/compiler/backend/verify.tw`
- `boot/compiler/codegen/wasm_plan_impl.tw`
- `boot/compiler/codegen/wasm_plan_scan.tw`
- `boot/compiler/builtins.tw`
- `boot/compiler/resolver.tw` usage sites
- possibly a new shared helper file such as `boot/compiler/backend/callable_targets.tw`

**Add / refactor**

- introduce one helper API for `FuncId` classification
- centralize queries such as:
  - `callable_target(fid, prepared_funcs, builtins, env)`
  - `callable_semantic_sig(fid, ...)`
  - `is_closure_materializable(fid, ...)`
  - `closure_policy(fid, ...)`
  - `direct_call_policy(fid, ...)`
  - `has_typed_closure_support(fid, registry, ...)`
  - `builtin_requires_wrapper_trampoline(fid, ...)`

**Current code to replace or simplify**

- ad hoc `bi.entry(...)` branching in `emit_call(...)`
- verifier logic that uses `funcs_by_id` presence as a proxy for closure support
- planner logic that silently drops builtin `FuncId`s when
  `register_func_sig_by_id(...)` cannot find a prepared func

**Done when**

- the backend has one obvious place to ask what kind of callable a `FuncId`
  represents
- builtin semantic signatures are available to planner, verifier, and emitter
- no pass relies on `prepared_funcs` membership alone to decide closure support

### 2. Correctness representation: hybrid base-closure interoperability with typed specialization

**Primary files**

- `boot/compiler/backend/repr_assign.tw`
- `boot/compiler/codegen/wasm_layout.tw`
- `boot/compiler/backend/verify.tw`
- `boot/compiler/codegen/wasm_plan_impl.tw`
- `boot/compiler/codegen/emit.tw`

**Add / refactor**

- keep `Function(...)` semantically precise while making sure every materialized
  closure remains interoperable through base `rt_types__Closure`
- preserve typed closure structs and typed call paths where a concrete
  signature is known and the representation proof is explicit
- audit helpers that currently assume either:
  - every function value lowers to a typed closure subtype, or
  - every indirect closure call should use the universal path
- align verifier, planner, and emitter with that hybrid rule

**Done when**

- builtin and user closures can always interoperate through the base closure
  ABI when needed
- typed closure specialization remains available where it is proven safe
- builtin closure support no longer depends on collapsing all function-valued
  storage to universal-only representation

### 3. Explicit closure-boundary rewrite

**Primary files**

- `boot/compiler/codegen/insert_boundaries.tw`
- `boot/compiler/anf.tw`
- `boot/compiler/backend/prepare.tw`
- `boot/compiler/backend/prepared_ir.tw`
- `boot/compiler/backend/slot_assign.tw`
- `boot/compiler/backend/repr_assign.tw`
- `boot/compiler/backend/verify.tw`

**Add / refactor**

- add a dedicated rewrite that turns `AGlobalFunc(fid)` into explicit closure
  materialization when the destination semantic type is `Function(...)` or the
  destination backend representation requires a closure value
- prefer reusing existing closure machinery via `AMakeClosure(fid, [])`
  instead of inventing a separate builtin-only op
- ensure the rewrite covers:
  - user direct-call arguments with function-typed params
  - builtin / intrinsic call arguments with function-typed params
  - closure ABI call arguments where semantic metadata is not yet attached
  - local init / assignment into function-typed locals
  - returns / breaks of function-typed values
  - record / variant / array construction when a field or payload is
    function-typed

**Likely concrete helper additions**

- `atom_is_global_func(atom)`
- `mono_is_function(mono)`
- `rewrite_function_value_boundary(...)`
- `rewrite_atoms_for_expected_monos(...)`
- `rewrite_atom_for_expected_closure_storage(...)`

**Important constraint**

- plain `AGlobalFunc(fid)` should remain valid for direct calls
- explicit closure materialization should only appear where the value is
  actually crossing a closure boundary

**Done when**

- function-valued storage / return paths no longer rely on emit-time wrapping
- `AMakeClosure(fid, [])` appears in prepared IR for builtin function values at
  closure boundaries

### 4. Verifier updates

**Primary file**

- `boot/compiler/backend/verify.tw`

**Add / refactor**

- update `AMakeClosure` verification so builtin targets can be accepted
- stop requiring every `AMakeClosure` target to exist in `funcs_by_id`
- validate by callable kind instead:
  - prepared user func: validate captures against closure metadata
  - builtin func: require zero captures and explicit closure-materializable
    support
- infer `AGlobalFunc` monos for supported builtin targets from callable metadata
- add helper-level diagnostics such as:
  - unknown callable target
  - builtin target is not closure-materializable
  - builtin closure must not capture locals

**Current hot spots**

- `verify_expr(...)` branch for `.AMakeClosure(fid, free_vars)`
- `verify_atom_coercible_to_val_type(...)`
- `infer_atom_mono(...)`
- any helper that infers `AGlobalFunc(_)` is always `.ClosureRef` without
  distinguishing direct vs materialized uses

**Done when**

- the verifier can describe builtin closure failures explicitly in backend terms
- verifier behavior matches the explicit closure-boundary rewrite and widened
  closure representation

### 5. Planner updates

**Primary files**

- `boot/compiler/codegen/wasm_plan_scan.tw`
- `boot/compiler/codegen/wasm_plan_impl.tw`
- `boot/compiler/codegen/wasm_plan_types.tw`

**Add / refactor**

- make explicit closure materialization the primary source of trampoline demand
- extend planner data so builtin closure targets are not lost when no
  `PreparedFunc` exists
- separate closure support from typed closure support
- register semantic signatures for builtin closure targets from callable
  metadata, not only from prepared funcs
- add planned metadata for closure-materializable builtin targets, for example:
  - `builtin_closure_funcs: Dict<Int, Bool>`
  - or a more general callable-trampoline requirement set

**Current hot spots**

- `scan_module_bodies(...)`
- `scan_op(...)` handling for `.AMakeClosure(...)`
- `register_func_sig_by_id(...)`
- `register_higher_order_global_func_sigs(...)`
- `reg.concrete_func_sigs[...]`

**Done when**

- planner can report all user and builtin targets that need closure trampolines
- builtin closure targets survive planning even without prepared bodies
- typed closure metadata is optional optimization data rather than a correctness
  prerequisite

### 6. Builtin universal closure trampoline emission

**Primary files**

- `boot/compiler/codegen/emit.tw`
- `boot/compiler/builtins.tw`
- possibly `boot/compiler/backend/callable_targets.tw`
- possibly `boot/compiler/codegen/wasm_plan_impl.tw` if planner-owned symbols or
  metadata are needed

**Add / refactor**

- split `emit_make_closure(...)` by callable kind
- add builtin trampoline emission independent of `PreparedFunc`
- emit empty / null env for builtin global closures
- derive argument/result boxing from semantic signature metadata
- route only supported builtin closure kinds through the generic universal path
- reject or defer wrapper-required builtin targets explicitly

**Current hot spots**

- `lookup_func_sym(...)`
- `emit_make_closure(...)`
- `emit_trampolines_for_func(...)`
- `emit_universal_trampoline(...)`
- top-level module emission where trampoline `FuncDef`s are collected

**Likely additions**

- `lookup_builtin_callable_sym(...)` or equivalent import/runtime symbol lookup
- `emit_builtin_universal_trampoline(...)`
- `emit_trampolines_for_callable_target(...)`
- `callable_tramp_sym(fid, kind)` helpers

**Design caution**

- runtime builtins and intrinsics may need different trampoline stories
- if some builtin cannot yet be soundly re-entered through a generic universal
  trampoline, mark it non-closure-materializable first rather than hiding the
  limitation

**Done when**

- `emit_make_closure(fid, [], ...)` works for supported builtin targets
- builtin closure creation no longer calls `lookup_func_sym(...)`

### 7. Universal indirect-call correctness path

**Primary file**

- `boot/compiler/codegen/emit.tw`

**Add / refactor**

- change indirect closure calling so typed call emission is optional
  specialization, not the default implied by callee mono alone
- ensure every materialized closure can be called through the universal closure
  ABI
- keep typed closure path only when representation proof is explicit

**Current hot spots**

- `emit_closure_call(...)`
- `emit_universal_closure_call(...)`
- `require_closure_atom(...)`
- closure layout lookups based on `ctx.registry.concrete_func_sigs`

**Practical first step**

- prefer the universal call path for all indirect closure calls until builtin
  closure support is stable
- then selectively restore typed call specialization if benchmarks justify it

**Done when**

- a builtin closure stored in a function-typed slot can be called safely later
- typed closure layout assumptions no longer cause correctness failures

### 8. Wrapper-based builtin closure follow-up

**Primary files**

- `boot/compiler/codegen/emit.tw`
- `boot/compiler/builtins.tw`
- callable-target metadata helpers

**Add / refactor**

- add dedicated wrapper trampoline modes for builtin families that need bespoke
  closure lowering
- document the wrapper contract per builtin family
- expand the supported builtin subset deliberately

**Done when**

- harder builtin families gain first-class closure support through explicit
  ownership
- support expansion does not reintroduce ad hoc emitter patches

### 9. Tests and regressions

**Primary areas**

- backend tests covering prepared IR / verifier / emitter behavior
- boot tests and example-based self-host checks

**Add now**

- user higher-order call with builtin function value:
  - `xs.map(Int.to_string)`
  - `xs.map(Float.to_string)`
- stored builtin function value later called indirectly
- returned builtin function value later consumed indirectly
- verifier characterization for supported / unsupported builtin closure targets
- negative test for builtin `AMakeClosure(fid, non_empty_captures)`
- user-closure regression coverage under universal closure representation

**Add later with wrapper-mode support**

- builtin / intrinsic closure entry coverage:
  - `Iterator.unfold(0, step)`
  - `Cell.update(c, inc)`

**Regression commands**

```bash
cargo run --release -- build boot/main.tw -o target/boot-main.wasm
node tools/run_wasm_node.mjs target/boot-main.wasm -- build boot/main.tw
node tools/run_wasm_node.mjs target/boot-main.wasm -- run boot/tests/main.tw
node tools/run_wasm_node.mjs target/boot-main.wasm -- run examples/api_ergonomics.tw
```

## Current implementation status

Legend:

- `✅` done
- `🟡` in progress
- `⬜` not done yet

### 1. Shared callable-target ownership

**Goal**

One backend-owned way to answer:

- what a `FuncId` is
- what its semantic signature is
- whether it can be materialized as a closure
- whether it needs wrappers

**Files**

- `boot/compiler/backend/callable_targets.tw`
- `boot/compiler/backend/verify.tw`
- `boot/compiler/codegen/wasm_plan_impl.tw`
- `boot/compiler/codegen/insert_boundaries.tw`
- `boot/compiler/codegen/emit.tw`

**Status**

- ✅ shared callable-target helper module exists
- ✅ builtin semantic signatures are resolved from the environment
- ✅ closure-materializable policy exists
- ✅ wrapper-needed policy exists
- ✅ verifier uses `callable_target` for all call-shape and closure-support decisions — `funcs_by_id` proxy removed from `verify_call_shape`
- ✅ planner uses shared callable metadata — `ho_global_funcs` compat scan removed, `AMakeClosure` is the sole trampoline source
- ✅ boundary insertion uses semantic mono exclusively — conservative ABI fallback (`materialize_global_func_atom`) removed
- ✅ emitter uses shared callable metadata: wrapper_kind drives trampoline dispatch, no more name-based re-derivation
- ✅ closure materialization policy and wrapper policy are explicit backend data
- ✅ wrapper lowering kind (`WrapperKind`), typed-closure support policy (`typed_closure_support`), and direct-call policy (`DirectCallPolicy`) are now fully separate fields in `CallableTargetInfo`

### 2. Correctness representation: hybrid base-closure interoperability with typed specialization

**Goal**

Match stage0 more closely:

- every closure remains valid as base `rt_types__Closure` for interoperability
- typed closure structs and typed call paths remain available where concrete
  signatures are known and proven safe

**Files**

- `boot/compiler/backend/repr_assign.tw`
- `boot/compiler/codegen/wasm_layout.tw`
- `boot/compiler/backend/verify.tw`
- `boot/compiler/codegen/wasm_plan_impl.tw`
- `boot/compiler/codegen/emit.tw`

**Status**

- ✅ `val_type_of_mono(Function(...))` returns `rt_types__Closure` (base type) — propagates to slots, fields, payloads, repr_assign, verify
- ✅ all `Function(...)` valued locals, params, fields, and payloads now use `rt_types__Closure` as their Wasm type
- ✅ typed closure struct creation (`$closure_${key}`) and typed trampolines removed from emission — universal path is the correctness baseline
- ✅ builtin and user closures are interoperable through the base closure ABI

### 3. Explicit closure-boundary rewrite

**Goal**

Make closure materialization explicit in IR via `AMakeClosure(fid, [])` instead of
emit-time guessing.

**Files**

- `boot/compiler/codegen/insert_boundaries.tw`
- `boot/tests/suites/insert_boundaries_suite.tw`

**Status**

- ✅ rewrites function-typed user-call arguments
- ✅ rewrites function-valued returns
- ✅ rewrites `AInit`
- ✅ rewrites `AAssign`
- ✅ rewrites function-valued array literals
- ✅ rewrites function-valued record fields
- ✅ rewrites function-valued record updates
- ✅ rewrites function-valued variant payloads
- ✅ semantic mono lookup for record and variant boundaries is complete — `Option`/`Result` payload monos preserved, conservative ABI fallback removed
- 🟡 emitter-side adaptation has been reduced so the rewrite is more primary than before

### 4. Verifier ownership of builtin closure materialization

**Goal**

Verifier should validate supported and unsupported builtin `AMakeClosure`
explicitly.

**Files**

- `boot/compiler/backend/verify.tw`
- `boot/tests/suites/backend_verify_suite.tw`

**Status**

- ✅ supported builtin `AMakeClosure` is accepted
- ✅ unsupported builtin closure targets are rejected explicitly
- ✅ builtin closure captures are rejected explicitly
- ✅ callable metadata is used for builtin `AGlobalFunc` semantic mono inference
- ✅ verifier uses `target.closure_materializable` directly (no more `is_closure_materializable_policy` indirection)
- ✅ all call-shape and closure-support decisions route through `callable_target` — no remaining `funcs_by_id` proxy checks

### 5. Planner support for builtin higher-order/global functions

**Goal**

Planner should retain builtin callable signatures and closure needs without
requiring a `PreparedFunc`.

**Files**

- `boot/compiler/codegen/wasm_plan_impl.tw`
- `boot/compiler/codegen/wasm_plan_scan.tw`
- `boot/compiler/codegen/wasm_plan_types.tw`
- `boot/tests/suites/wasm_plan_suite.tw`

**Status**

- ✅ builtin higher-order/global function signatures can be registered in planner data
- ✅ builtin concrete function signatures can survive planning without prepared bodies
- ✅ planner tests cover builtin higher-order function signature registration
- ✅ compatibility-era `ho_global_funcs` scanning removed — `AMakeClosure` is now the sole source of closure-trampoline demand
- ✅ planner scan separates closure-materialized FuncIds from direct builtin call sites, and typed closure support is tracked explicitly in planner data

### 6. Builtin closure and trampoline emission

**Goal**

Emitter should materialize supported builtin closures without assuming a prepared
user body or user-only symbol lookup.

**Files**

- `boot/compiler/codegen/emit.tw`
- `boot/compiler/backend/callable_targets.tw`

**Status**

- ✅ builtin trampoline emission exists for a supported subset
- ✅ builtin closure creation no longer depends entirely on prepared user function bodies
- ✅ simple builtin targets such as the `to_string` family have a first-class closure path
- ✅ `Byte.to_string` now has explicit wrapper-backed closure trampoline support
- ✅ `Cell.update` now has explicit wrapper-backed closure trampoline support
- ✅ `Iterator.unfold` now has explicit wrapper-backed closure trampoline support
- 🟡 builtin support is still intentionally narrow
- ✅ wrapper dispatch now uses `target.wrapper_kind` field — no more `builtin_wrapper_policy(name)` name-check re-derivation
- ⬜ additional wrapper trampolines are still needed for other harder builtin targets such as host shimmed helpers

### 7. Emitter-side opportunistic wrapping cleanup

**Goal**

Make emission downstream of explicit closure materialization rather than
silently patching `AGlobalFunc` at call sites.

**Files**

- `boot/compiler/codegen/emit.tw`

**Status**

- ✅ removed direct-call argument wrapping in `emit_direct_call(...)`
- ✅ removed generic runtime ABI closure-arg opportunism in `emit_runtime_arg(...)`
- ✅ removed local opportunistic adaptation in intrinsic helpers such as `cell_update` and `iterator_unfold`
- ✅ removed the generic `emit_global_func_as_closure_arg(...)` helper
- ✅ the explicit boundary rewrite is now more primary than before
- ✅ remaining emitter assumptions around closure construction and typed closure layouts have been cleaned up

### 8. Universal indirect-call correctness path

**Goal**

Make indirect closure calls correct by default through the universal closure ABI.

**Files**

- `boot/compiler/codegen/emit.tw`

**Status**

- ✅ universal indirect-call lowering is now the default correctness path
- ✅ `emit_closure_call` always routes through `emit_universal_closure_call`
- ✅ typed closure call path removed — no longer part of active correctness seam
- ✅ stored builtin closures can be called safely through the universal path
- ✅ user closures have explicit regression coverage under the universal-first path

### 9. Wrapper-mode builtin support

**Goal**

Support builtins whose direct-call lowering is not enough for closure lowering.

**Files**

- `boot/compiler/backend/callable_targets.tw`
- `boot/compiler/codegen/emit.tw`
- `boot/compiler/builtins.tw`

**Status**

- ✅ some builtins are already classified as wrapper-needed or not yet generic-closure-safe
- ✅ wrapper trampolines implemented for `byte_to_string`, `cell_update`, and `iterator_unfold`
- ✅ wrapper contracts exist as explicit `WrapperKind` enum in `CallableTargetInfo`
- ✅ `WrapperKind`, `typed_closure_support`, and `DirectCallPolicy` are now fully separate fields
- ⬜ host shimmed helpers and other bespoke builtin families still need explicit wrapper-mode support

### 10. Tests and regression coverage

**Files**

- `boot/tests/suites/insert_boundaries_suite.tw`
- `boot/tests/suites/backend_verify_suite.tw`
- `boot/tests/suites/wasm_plan_suite.tw`

**Status**

- ✅ boundary insertion tests cover call arguments, return, init, assign, array literals, record fields, record updates, and variant payloads
- ✅ verifier tests cover supported builtin `AMakeClosure`, wrapper-backed builtin `AMakeClosure`, unsupported builtin `AMakeClosure`, and builtin captures
- ✅ planner tests cover builtin higher-order function signature registration
- ✅ codegen coverage includes direct first-class builtin closure emission for `Byte.to_string`, `Cell.update`, and `Iterator.unfold`
- ✅ stored-builtin-function then indirect-call regression coverage
- ✅ returned-builtin-function then indirect-call regression coverage
- ✅ stronger user-closure regression coverage under widened storage and universal indirect calls

### Suggested implementation order

1. inventory builtin closure support and declare the safe initial subset
2. add callable-target query helpers with builtin semantic signatures
3. refine the closure representation strategy around stage0-style hybrid interop and typed specialization
4. teach verifier about builtin-capable `AMakeClosure`
5. add explicit closure-boundary rewrite to produce `AMakeClosure(fid, [])`
6. update planner to record builtin closure trampoline requirements
7. implement builtin trampolines for the supported subset
8. split wrapper lowering kind, typed-closure support policy, and direct-call policy more cleanly in callable metadata
9. make indirect closure calling choose typed specialization when proven and universal fallback otherwise
10. remove ad hoc wrapping from emitters
11. add wrapper-mode support for harder builtin families
12. expand tests and rerun the self-host loop after each step

## Notes on scope discipline

The tempting short fix is still to patch individual seams like:

- `map(Int.to_string)`
- `Iterator.unfold`
- `Cell.update`

with more emitter-side wrapping logic.

That would reduce the visible failures, but it would preserve the real problem:
closure materialization would still be implicit, scattered, and user-function
biased.

This plan instead prefers:

- one callable-target model
- one backend-owned semantic signature story for builtin callables
- one explicit closure-materialization story
- one correctness representation for function-valued storage
- universal indirect-call correctness before typed-call optimization
- explicit support policies for builtin families

over a growing set of per-builtin exceptions.
