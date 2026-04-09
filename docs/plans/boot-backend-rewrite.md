# Boot Backend Rewrite Plan

## Status

Phase 1 complete. Boundary locked, architecture decisions settled, `prepare_backend()` wired into pipeline.

## Summary

The boot compiler's current Wasm backend has reached the point where local fixes
no longer buy real confidence. Self-hosted execution keeps advancing into new
backend paths and then failing on the next missing invariant:

- closure capture params missing from emitted functions
- string-pool entries missing for pattern-only literals
- alias-backed record and sum layouts failing during emission
- typed-layout helpers being called on erased `Anyref` values
- structurally present but semantically dead locals reaching codegen
- emitter panics like `lookup_local: unknown LocalId L136`

These are not independent bugs. They are symptoms of a backend architecture that
still reconstructs too much codegen truth on demand.

The rewrite proposed here changes the backend contract:

- optimized ANF remains the semantic optimization IR
- a new prepared backend layer becomes the codegen IR
- the verifier becomes the place where invalid backend states fail
- emission becomes a mostly mechanical lowering from prepared IR to Wasm IR

## Problem

Today the boot pipeline effectively asks the emitter to answer questions that
should already be settled before emission:

- Is this local physically present in the current function?
- Is it a declared param, a closure capture param, a let-bound local, or a
  module-global proxy?
- Does this value still have a concrete typed record/sum layout, or has it
  already been erased to `Anyref`?
- Is this match arm semantically live, or only structurally present?
- Is this closure ABI consistent with all closure creation sites?

The current backend answers those questions with a mix of:

- local-body pre-scans
- `MonoType` heuristics
- special cases in `emit.tw`
- side tables built in `wasm_plan.tw`
- flow-sensitive assumptions that are not explicit in the IR contract

That is fundamentally brittle. A backend should not discover missing locals,
invalid captures, or representation mismatches for the first time while
emitting Wasm instructions.

## Diagnosis

The current architecture conflates three different notions of identity:

1. **Semantic local identity**
   - ANF/Core `LocalId` as a source-level or lowering-level name
2. **Closure capture identity**
   - which outer local is captured by a hoisted function
3. **Physical backend slot identity**
   - which Wasm param/local slot is used in the emitted function body

As long as these stay partially implicit, the backend remains vulnerable to:

- missing or inconsistent capture layouts
- references to semantic locals that were never materialized as backend slots
- dead-path locals surviving into codegen without a valid runtime
  representation
- typed-layout helpers being applied to values that are only known as erased
  references

The current `lookup_local: unknown LocalId ...` failure is one manifestation of
that deeper issue, not the issue itself.

## Goal

Redesign the boot backend so that:

- closure conversion is explicit and stable
- backend slot allocation is explicit and stable
- value representation categories are explicit and stable
- impossible/dead paths are modeled explicitly rather than handled by emitter
  guesswork
- emission is a mostly mechanical lowering from backend-ready IR to Wasm IR
- verifier failures happen before emission, with actionable diagnostics

The result should be robust enough that new language features extend explicit
backend contracts rather than adding more emitter heuristics.

## Non-Goals

- port stage0 codegen line-for-line into boot
- preserve every current boot backend internal data structure
- optimize for minimal diff size
- hide invariant failures by auto-creating fallback locals or default values
- redesign the source language or user-visible runtime ABI without need

## Design Principles

### 1. Emission must not infer backend facts

If emission needs to know whether something is a capture param, typed sum,
opaque anyref, or dead placeholder, that fact should already exist in the input
IR/metadata.

### 2. Closure capture must become an explicit ABI rewrite

Captured values should not remain ambient free-variable knowledge. Hoisted
functions should receive explicit capture params, and their bodies should refer
to those rewritten backend params.

### 3. Backend slot identity must be separate from semantic local identity

`LocalId` can remain the semantic identity used by earlier IR, but the backend
needs its own slot model for physical params/locals.

### 4. Representation categories must be first-class

The backend must distinguish at least:

- primitive unboxed values
- concrete typed refs
- concrete typed sums
- erased sum-like values
- opaque `Anyref`
- closure refs / closure env payloads
- dead or impossible placeholders

### 5. Verification is a phase, not an afterthought

We should add a backend verifier over prepared backend IR instead of letting the
emitter trap on bad states.

### 6. Prefer explicit rewrites over dynamic emitter cleverness

When in doubt, rewrite the IR so the emitter becomes simpler.

### 7. Code emission should be machinery

The emitter should be a mostly mechanical translation layer, not a secondary
analysis pass. Its job is to consume prepared backend facts and lower them to
Wasm IR. If emission needs to rediscover scoping, capture, liveness, or runtime
representation facts, the design boundary is wrong.

## Governance and Review Criteria

This plan is structured so progress can be audited at review time without
reconstructing intent from scattered commits.

Each implementation phase should produce:

- a committed design or interface change in the planned target files
- verifier or unit coverage for the new invariant being introduced
- at least one integration check proving the phase works in the real pipeline
- deletion or isolation of superseded heuristics where practical
- an update to this plan's phase checklist and decision log

A phase is not complete just because new code exists. It is complete when the
new contract is explicit, verified, tested, and the old competing mechanism is
removed or clearly quarantined.

Working rules:

- Do not add new emitter-side inference as part of the rewrite.
- Do not silently preserve invalid states just to keep self-hosting moving.
- Prefer explicit prepared IR fields over additional side maps.
- Prefer verifier failures over fallback code paths.
- Keep semantic ANF optimization-friendly; move backend obligations into the new
  prepared layer.
- Every transitional adapter should be temporary and called out explicitly.

## Current Surface Area and Rewrite Targets

Initial audit map of the backend surface to be rewritten or absorbed into new
phases:

| Current area | Current role | Structural problem | Rewrite target |
|---|---|---|---|
| `boot/compiler/lower_core.tw` | hoisting, free-var collection, closure creation setup | capture identity still leaks forward as ambient semantic local knowledge | keep semantic closure lowering, but move final capture ABI rewrite into dedicated backend pass |
| `boot/compiler/lower_anf.tw` | semantic ANF lowering | backend assumptions still implicit in ANF shape | keep as semantic ANF producer only |
| `boot/compiler/opt/*` | ANF optimization | optimized ANF is still not backend-safe by itself | keep optimizer contract unchanged; add prepared backend layer after optimization |
| `boot/compiler/codegen/wasm_plan.tw` | type/layout, capture, and string-pool side tables | rediscovers facts that should already be explicit in prepared backend IR | rewrite to consume prepared backend IR | ~881 lines |
| `boot/compiler/codegen/insert_boundaries.tw` | representation boundary insertion | currently sits in an ambiguous relationship with later repr inference | either absorb into preparation or make its postconditions explicit input to repr assignment | significant supporting pass |
| `boot/compiler/codegen/emit.tw` | Wasm emission plus backend reasoning | too much semantic inference and failure discovery during emission | reduce to mechanical lowering from prepared backend IR | ~2741 lines |
| `boot/compiler/codegen/codegen.tw` | pipeline wiring | currently wires semantic ANF too directly into planning/emission | rewire around closure conversion, preparation, verifier, then planning/emission | ~63 lines |

## Proposed Architecture

### Pipeline shape

| Current path | Proposed path |
|---|---|
| lower to Core | lower to Core |
| lower to ANF | lower to ANF |
| optimize ANF | optimize ANF |
| Wasm planning | final closure conversion / capture ABI rewrite |
| boundary insertion | backend preparation / slot assignment / representation assignment |
| emit Wasm | backend verification |
|  | Wasm planning over prepared backend IR |
|  | boundary insertion only if still needed in that form |
|  | emit Wasm from prepared backend IR |

Key change: raw optimized ANF is no longer the direct contract for emission.

### New backend IR

Use a distinct **prepared backend IR** as the codegen contract.

Naming decision:

- the new layer should not continue to be called plain `ANF`
- use `Prepared*` names for its top-level types unless implementation work
  reveals a stronger alternative

Suggested top-level names:

- `PreparedModule`
- `PreparedFunc`
- `PreparedExpr`

The exact syntax can stay ANF-like if that keeps implementation simple. The key
change is the contract, not whether every node shape changes.

### Core data model

#### Slot identity

Add a physical slot identity separate from `LocalId`.

Suggested shape:

```tw
pub type SlotId = .{ id: Int }
```

#### Slot roles

```tw
pub type SlotRole = {
  Param,
  CaptureParam,
  Local,
  PatternLocal,
  DeadPlaceholder,
}
```

Module globals are not slots in prepared backend IR. They should appear as
explicit prepared operands or nodes.

#### Representation categories

```tw
pub type ReprKind = {
  I64,
  F64,
  I32,
  Byte,
  TypedRef(MonoType),
  TypedSum(MonoType),
  ClosureRef(FuncId),
  ErasedSum,
  OpaqueAnyref,
  DeadValue,
}
```

This is illustrative, not final. The exact taxonomy should be refined during
implementation, but the backend must stop collapsing all refs into vague
`MonoType`-driven assumptions.

#### Slot info

```tw
pub type SlotInfo = .{
  slot: SlotId,
  source_local: LocalId,
  role: SlotRole,
  mono: MonoType,
  repr: ReprKind,
  wasm_type: ValType,
}
```

Invariant:

- `mono` preserves the semantic type for diagnostics, planning, and debugging
- `repr` records the runtime representation actually available at this program
  point
- `wasm_type` is the physical storage/calling type derived from `repr`

#### Function environment

Each prepared function should carry an explicit environment record such as:

```tw
pub type PreparedFunc = .{
  func_id: FuncId,
  name: String,
  params: Vector<SlotId>,
  captures: Vector<SlotId>,
  locals: Vector<SlotId>,
  slots: Dict<Int, SlotInfo>,
  body: PreparedExpr,
  return_mono: MonoType,
}
```

Phase 1 must also settle what `PreparedExpr` is:

- a distinct prepared AST, or
- an ANF-shaped IR rewritten to use prepared operands/slots explicitly

What is not acceptable is leaving `PreparedExpr` as raw semantic `AnfExpr` plus
implicit side metadata.

Again, the important point is not the exact record fields; it is that physical
params, locals, and captures become explicit and queryable without inference.

## Major Rewrite Areas

### 1. Final closure conversion

#### Why

The current pipeline still treats closure capture as partly implicit. That is
why missing capture-layout synchronization turns into emitter panics.

#### Rewrite target

Make closure conversion a distinct, final, backend-facing transformation:

- compute each hoisted function's complete capture set
- canonicalize capture ordering
- add explicit capture params to the hoisted function ABI
- rewrite body references to captured outer locals so they use those capture
  params
- rewrite closure construction sites to pass captured values in the same
  canonical order

#### Result

After this pass:

- a prepared function body never refers to ambient outer locals
- every capture is just another explicit backend param with role
  `CaptureParam`
- closure construction and closure call ABI become mechanically checkable

#### Why this is better than patching capture-layout tables

Because it removes the hidden contract. A function either has an explicit
capture param or it does not. There is no later `lookup_local` guesswork.

### 2. Backend slot assignment

#### Why

Right now the emitter maps semantic locals directly to Wasm locals, while also
trying to inject capture params and pre-allocate let-bound locals. That mixes
concerns and makes failures hard to reason about.

#### Rewrite target

Introduce a dedicated slot assignment phase that:

- allocates physical backend slots for every live prepared-function value
- classifies each slot by role
- determines the final Wasm value type for each slot
- records the mapping from semantic local uses to physical slot uses

#### Result

The emitter no longer owns local existence. It just consumes assigned slots.

### 3. Representation assignment

#### Why

Many boot backend failures come from using semantic type as a proxy for runtime
representation.

That is insufficient for:

- erased boundaries
- typed vs erased sums
- closure env payloads
- impossible branches
- values intentionally held as `Anyref`

#### Rewrite target

Add a representation assignment phase that computes per-slot and per-expression
representation categories.

It should answer questions like:

- can this value safely use typed record layout helpers?
- can this value safely use typed sum layout helpers?
- is this closure env payload only recoverable as opaque anyref?
- is this branch-local semantically dead?

This phase should not silently guess. If a live value's representation cannot be
established, it should fail verification.

#### Result

Emitter helpers become representation-driven instead of `MonoType`-driven.

### 4. Backend verifier

#### Why

The boot backend currently discovers bad states by trapping deep in emission.
That is too late.

#### Rewrite target

Add a verifier over the prepared backend form. It should check at least:

- every referenced semantic local maps to a valid slot in scope
- every prepared slot has a role, mono, repr, and wasm type
- every closure body references captures only through explicit capture params
- every closure construction site matches the prepared capture layout
- every typed-layout operation consumes a compatible typed representation
- every erased-layout operation avoids typed-only helpers
- dead/impossible placeholders are only used in dead/impossible contexts
- all branch merges preserve compatible runtime representation categories
- module-global access paths are explicit and consistent

#### Result

Failures become localized compiler diagnostics instead of Wasm execution traps.

### 5. Emitter simplification

#### Why

`emit.tw` currently does too much backend reasoning.

#### Rewrite target

Reduce emitter responsibilities to machinery:

- translate prepared slots to Wasm locals/params/globals
- translate prepared repr-aware operations to Wasm IR
- select among already-validated lowering forms
- trust the backend verifier rather than recover from bad input

It should not:

- infer capture layouts
- infer local existence
- infer typed-vs-erased representation state
- decide whether a path is semantically live
- manufacture fallback semantics for inconsistent inputs

#### Result

The emitter becomes smaller, easier to test, and less likely to accrete future
special cases. More importantly, it stops being a hidden semantic pass.

### 6. Wasm planning alignment

#### Why

Type/layout planning currently collaborates with emission through side tables
that are not always aligned with function-local reality.

#### Rewrite target

Move Wasm planning to consume prepared backend functions and explicit capture
layouts rather than rediscover them from raw ANF.

This includes:

- closure layouts
- concrete function signatures
- string-pool needs
- module-global typing
- typed record/sum registrations

#### Result

Planning and emission share the same prepared source of truth.

## Proposed File and Module Layout

The exact names can change, but the rewrite should converge on a layout where
responsibilities are obvious from file names.

Suggested additions:

- `boot/compiler/backend/prepared_ir.tw`
  - prepared module/function/expr/slot types
- `boot/compiler/backend/closure_convert.tw`
  - final closure ABI rewrite over optimized ANF
- `boot/compiler/backend/slot_assign.tw`
  - physical slot allocation and role classification
- `boot/compiler/backend/repr_assign.tw`
  - runtime representation assignment and branch joins
- `boot/compiler/backend/verify.tw`
  - verifier over prepared backend IR
- `boot/compiler/backend/prepare.tw`
  - orchestration from optimized ANF to prepared backend module

Likely survivors with narrower contracts:

- `boot/compiler/codegen/wasm_plan.tw`
  - consumes prepared backend IR only
- `boot/compiler/codegen/emit.tw`
  - consumes prepared backend IR only
- `boot/compiler/codegen/codegen.tw`
  - orchestrates prepare → verify → plan → emit → link

## Transitional Strategy

The rewrite should not require a flag day where all backend stages change at
once. The risky part is not the end-state architecture; it is the coexistence
period while old planning/emission still exist.

### Migration unit

Use **whole-module migration**, not per-function mixed mode.

A given module should be either:

- on the legacy semantic-ANF backend path, or
- on the new prepared-backend path

within a single compile. Do not allow one function in a module to be prepared
while another is still interpreted by legacy planner/emitter logic. That would
introduce mixed closure ABI and representation assumptions inside one module.

### Adapter boundary

The only supported transitional adapter should be:

- optimized semantic ANF
- → `prepare_backend(...)`
- → prepared backend IR
- → temporary lowering adapter back into a legacy-emittable shape **only when
  necessary to keep the pipeline running during migration**

This adapter should be module-wide, temporary, and explicitly tracked for later
removal. It should not become a second long-term backend contract.

### Green-suite strategy during migration

For each phase, keep both of these green where applicable:

- structural tests for the new prepared backend IR layer
- existing end-to-end source → WAT / Wasm execution tests via the current
  production path

Until Phase 6 is complete, the new backend path may validate mostly through:

- prepared IR snapshots
- backend verifier tests
- selected adapter-backed integration tests

After Phase 6, the production path should flip to prepared IR and the adapter
should move onto the deletion path.

## Proposed Phase Breakdown

The table below is the quick audit view. Detailed phase descriptions follow.

| Phase | Goal | Size | Primary output |
|---|---|---|---|
| 1 | Lock boundary decisions | S | settled architecture blockers and `prepare_backend(...)` boundary |
| 2 | Final closure conversion | M | explicit capture ABI rewrite + first prepared backend IR slice |
| 3 | Slot assignment | M | slot-based prepared functions |
| 4a | Boundary semantics | M | settled architectural home for boundary insertion + prepared boundary form |
| 4b | Representation assignment | L | repr-classified prepared backend IR with branch/join rules |
| 5 | Backend verifier | M | authoritative prepared-backend-IR verifier |
| 6a | Planning migration | M | `wasm_plan.tw` on prepared backend IR |
| 6b | Emitter slot migration | M | slot-based emitter |
| 6c | Emitter repr migration | M | mechanical repr-driven emitter |
| 7 | Delete legacy heuristics | S | single production backend contract |

### Phase 1: Lock the boundary and settle unresolved architecture choices

Size: S

Target files:

- `docs/plans/boot-backend-rewrite.md`
- `boot/compiler/codegen/codegen.tw`
- `boot/compiler/backend/prepare.tw`

Deliverables:

- finalize the major boundary decisions needed before implementation starts
- decide whether boundary insertion is absorbed into preparation or remains a
  distinct pass over prepared IR
- decide the migration adapter boundary and make it explicit in pipeline wiring
- define the entry API for backend preparation

Required decisions:

- module globals are **not** modeled as ordinary locals in prepared IR
- migration unit is whole-module, not per-function mixed mode
- verifier will be authoritative before emission once Phase 5 lands
- boundary insertion has one explicit architectural home before Phase 4 starts

Exit criteria:

- `prepare_backend(...)` boundary is named in code and docs
- module globals decision is no longer an open question
- transitional adapter strategy is documented in code comments or stubs
- this plan's open questions are reduced to details, not architecture blockers

Why first:

- prevents Phase 2 from building on unresolved global/capture assumptions
- avoids spending a phase on speculative placeholder types

### Phase 2: Implement final closure conversion and the first prepared IR slice

Size: M

Target files:

- `boot/compiler/backend/prepared_ir.tw`
- `boot/compiler/backend/closure_convert.tw`
- `boot/compiler/lower_core.tw`
- `boot/compiler/codegen/codegen.tw`
- new tests under `boot/tests/suites/`

Deliverables:

- first real prepared IR types, introduced only as needed for closure rewrite
- explicit capture-set computation
- canonical capture ordering
- hoisted function ABI rewrite
- closure construction rewrite
- tests for nested and transitive captures

Exit criteria:

- prepared functions no longer rely on ambient free-variable lookup
- closure creation sites and hoisted function ABIs agree on capture order
- module-global references are explicit backend operands or explicit prepared
  nodes, not disguised locals
- the current capture side-table mechanism is either removed from hot paths or
  clearly marked transitional

Why next:

- closure capture is the sharpest current structural failure
- it removes one of the biggest ambient assumptions from codegen

### Phase 3: Implement prepared slot assignment

Size: M

Target files:

- `boot/compiler/backend/prepared_ir.tw`
- `boot/compiler/backend/slot_assign.tw`
- `boot/compiler/backend/prepare.tw`
- new tests under `boot/tests/suites/`

Deliverables:

- `SlotId`
- slot allocation pass
- role classification
- semantic-local to slot mapping
- prepared-function shape

Exit criteria:

- all live prepared-function references resolve through slots, not raw semantic
  locals
- slot roles are explicit in test fixtures
- emitter code can begin consuming slots without needing local pre-scan logic

Why next:

- every later backend phase should consume slots rather than raw locals

### Phase 4a: Settle boundary semantics

Size: M

Target files:

- `boot/compiler/backend/prepared_ir.tw`
- `boot/compiler/codegen/insert_boundaries.tw` or its replacement
- `boot/compiler/backend/prepare.tw`
- new tests under `boot/tests/suites/`

Deliverables:

- explicit architectural home for boundary insertion
- explicit prepared form for boundary-crossing operations
- decision on whether repr transitions create new prepared values/slots or
  update existing annotations

Exit criteria:

- boundary insertion is no longer architecturally ambiguous
- prepared backend IR has an explicit representation-transition story
- follow-on repr work no longer depends on unresolved boundary questions

Why next:

- it narrows the hardest part of representation work before full classification

### Phase 4b: Implement representation assignment

Size: L

Target files:

- `boot/compiler/backend/repr_assign.tw`
- `boot/compiler/backend/prepared_ir.tw`
- new tests under `boot/tests/suites/`

Deliverables:

- `ReprKind`
- representation classification pass
- join/merge rules for branches
- explicit handling of dead/impossible branches
- tests covering typed refs, typed sums, erased sums, opaque anyrefs

Exit criteria:

- typed-layout operations can be statically gated by prepared repr metadata
- branch joins that previously relied on emitter guesswork are explicit
- impossible/dead-path placeholders are represented intentionally and tested
- valid representation flows are covered by positive verifier-facing tests

Why next:

- this is the foundation for removing emitter heuristics around layout access

### Phase 5: Build the backend verifier

This is also the first major payoff checkpoint. Even before full planner/emitter
migration, an authoritative verifier over prepared backend IR should catch the
main invalid-state bug classes earlier and more clearly than the current path.

Size: M

Target files:

- `boot/compiler/backend/verify.tw`
- `boot/compiler/backend/prepare.tw`
- `boot/compiler/codegen/codegen.tw`
- new tests under `boot/tests/suites/`

Deliverables:

- verifier over prepared backend IR
- high-quality diagnostics with function/local/slot context
- CI integration before Wasm emission

Exit criteria:

- invalid prepared backend states fail before Wasm planning/emission
- at least the current known bad classes have verifier coverage:
  - missing local/slot mapping
  - missing explicit capture param
  - typed-layout op on erased/opaque repr
  - invalid branch repr merge
- the pipeline can be configured to stop at verifier failure with readable
  diagnostics

Why next:

- locks in the contract before emitter rewrite proceeds too far

### Phase 6: Migrate Wasm planning and emission to prepared IR

Size: L

This is the highest-risk implementation phase and should be tracked as three
sub-phases rather than one undifferentiated rewrite.

### Phase 6a: planning migration

This phase includes string-pool population. In the new architecture, planning
should derive string-pool needs from prepared backend IR rather than depending
on emitter-path discovery.

Target files:

- `boot/compiler/codegen/wasm_plan.tw`
- `boot/compiler/codegen/codegen.tw`

Deliverables:

- planning consumes prepared functions and explicit capture/global/repr facts
- capture/layout facts are no longer rediscovered from raw ANF in the production
  path

Exit criteria:

- planning uses prepared IR as its source of truth
- planning-side capture/layout side tables derived from raw ANF are removed or
  quarantined

### Phase 6b: emitter slot migration

Target files:

- `boot/compiler/codegen/emit.tw`
- `boot/compiler/codegen/codegen.tw`

Deliverables:

- emitter local handling rewritten around `SlotId`
- old local pre-scan and semantic-local lookup removed from the production path

Exit criteria:

- emitter local lookup is slot-based and verifier-backed
- the `lookup_local: unknown LocalId ...` class of failure becomes structurally
  impossible in emission

### Phase 6c: emitter representation migration

Target files:

- `boot/compiler/codegen/emit.tw`
- `boot/compiler/codegen/codegen.tw`
- targeted backend/codegen tests

Deliverables:

- repr-sensitive emitter helpers consume prepared repr metadata
- typed-layout access is gated by verified typed repr information
- old typed-vs-erased inference logic is removed from the production path

Exit criteria:

- emitter helper responsibilities are visibly smaller and mechanical
- repr-sensitive codegen bugs move from runtime traps toward verifier failures

Why next:

- this is where architectural payoff appears in production code

### Phase 7: Delete obsolete compatibility heuristics

Size: S

Target files:

- whichever old backend files still carry transitional logic
- this plan doc and active status docs

Deliverables:

- remove transitional fallback logic
- remove redundant side tables that duplicate prepared IR facts
- remove dead helper paths that only existed to compensate for weak invariants

Exit criteria:

- only one backend contract remains in the production path
- old inference-based helpers are deleted, not merely bypassed
- documentation reflects the new architecture rather than both architectures

Why last:

- keeps migration practical while preventing permanent dual systems

## Implementation Checklist

### Phase checklist

- [x] Phase 1 complete: backend boundary settled and architecture blockers closed
- [x] Phase 2 complete: final closure conversion implemented and tested
- [x] Phase 3 complete: slot assignment implemented and tested
- [ ] Phase 4a complete: boundary semantics settled and implemented
- [ ] Phase 4b complete: representation assignment implemented and tested
- [ ] Phase 5 complete: backend verifier enforced in pipeline
- [ ] Phase 6a complete: Wasm planning consumes prepared backend IR
- [ ] Phase 6b complete: emitter local handling is slot-based
- [ ] Phase 6c complete: emitter repr handling is mechanical and prepared-IR-driven
- [ ] Phase 7 complete: legacy inference heuristics removed

### PR review checklist

Use this at review time for any backend rewrite PR:

- [ ] Does this change move responsibility out of the emitter rather than into it?
- [ ] Does the new fact live in prepared backend IR or verifier-checked metadata?
- [ ] Is the invariant named and tested?
- [ ] Does the pipeline fail earlier and more clearly than before?
- [ ] Was any obsolete heuristic removed or quarantined?
- [ ] Does this make future backend features easier to add mechanically?

## Testing Strategy

### Unit tests by phase

#### Closure conversion

Add tests covering:

- single capture
- multiple captures with stable order
- nested closure recapture
- transitive capture through intermediate closures
- shadowing and pattern-bound locals
- captured module globals vs ordinary locals

#### Slot assignment

Add tests covering:

- params
- capture params
- let locals
- pattern locals
- dead placeholders
- dead-placeholder and non-global slot roles only

#### Representation assignment

Add tests covering:

- typed records
- typed sums
- erased boundary outputs
- closure env payloads
- impossible match branches
- branch merges with compatible and incompatible representations

#### Verifier

Add tests for both valid and invalid prepared backend IR.

Valid cases should cover acceptance of representative prepared programs across
major representation categories.

Invalid cases should include:

- missing slot
- capture used without capture param
- typed sum operation on opaque anyref
- invalid branch merge
- dead placeholder used in live context

### Integration tests

Add a dedicated backend-preparation and self-hosting matrix.

| Check kind | Purpose | Required examples |
|---|---|---|
| Structural pipeline checks | validate the new backend layer before emission | source → optimized ANF → prepared backend IR → verifier |
| Runtime pipeline checks | validate end-to-end codegen behavior | source → full codegen → WAT/Wasm → execution |

Required coverage:

- verify prepared IR for `boot/main.tw`
- build WAT for `boot/main.tw`
- run selected self-hosting slices through Wasm
- targeted fixtures for:
  - nested closures
  - higher-order functions
  - alias-backed records/sums
  - match-heavy code
  - pattern-only strings
  - module-global capture/reference cases

### Regression policy

Any backend crash that currently appears as an emission panic should gain:

- one verifier regression if it is an invalid-state bug
- one integration regression if it is a valid-state codegen bug

## Migration Notes

### Keep semantic ANF stable if possible

The rewrite does not require replacing ANF as the optimizer's IR. The cleaner
boundary is to keep semantic ANF for optimization and add a distinct prepared
backend IR layer after optimization.

### Avoid half-implicit transitional designs

During migration, prefer temporary adapters over making the new prepared IR
optional. Optionality would recreate the same split-brain architecture we are
trying to remove.

### Preserve user-visible ABI only where intentional

Internal boot backend function layouts can change if needed. If any public or
runtime ABI must remain stable, document that separately and encode it in tests.

## Risks and Failure Modes

### Risk: the rewrite preserves semantic locals too deep into backend code

Mitigation:

- require slot-based APIs in new emitter/planner code
- forbid new raw-`LocalId` lookup helpers in backend emission

### Risk: prepared IR becomes just another bag of side tables

Mitigation:

- prefer explicit fields on prepared functions/slots/exprs
- use side maps only for clearly global facts

### Risk: transitional adapters become permanent

Mitigation:

- every adapter must be listed in Phase 7 deletion work
- do not mark a phase complete while the old path remains authoritative

### Risk: verifier is added but not made authoritative

Mitigation:

- wire verifier into the production path before emission
- treat verifier failure as a hard pipeline stop

### Risk: representation assignment is too vague to be useful

Mitigation:

- require concrete yes/no gating for typed-layout operations
- add branch-join and impossible-path tests before emitter rewrite proceeds

### Risk: closure conversion forces deeper semantic-IR changes than planned

Mitigation:

- if closure conversion requires semantic ANF or Core IR changes beyond explicit
  capture-param injection, closure ordering, and explicit global/capture
  operands, stop and re-evaluate the backend boundary before continuing
- do not quietly expand the rewrite scope by embedding semantic-lowering changes
  into backend migration work

## Decision Log

Update this section as implementation proceeds.

- Initial direction: use prepared backend IR after optimization rather than
  replacing semantic ANF as the optimizer IR.
- Initial direction: treat code emission as mechanical lowering, not semantic
  recovery.
- Initial direction: separate semantic local identity from physical backend slot
  identity.
- Initial direction: make closure capture an explicit ABI rewrite.
- Settled direction: module globals are explicit backend operands/nodes in
  prepared backend IR, not ordinary locals.

### Phase 1 settled decisions

- **prepare_backend() boundary**: `boot/compiler/backend/prepare.tw` defines
  `PreparedModule` and `prepare_backend(anf) PreparedModule`. This is the only
  sanctioned entry into the backend pipeline. `codegen.tw` calls it before any
  other backend pass.
- **Transitional adapter**: `PreparedModule.anf` carries the raw semantic
  `AnfModule` for use by the legacy planner/emitter during migration. It is
  explicitly marked transitional and must be removed after Phase 6.
- **Migration unit**: whole-module. A given module is either on the legacy
  semantic-ANF path or the new prepared-backend path within a single compile.
  No per-function mixed mode.
- **Module globals**: not modeled as ordinary locals in prepared IR. They will
  be explicit backend operands or prepared nodes in Phase 2+.
- **Boundary insertion**: currently lives between plan_wasm_types and
  emit_module in the legacy pipeline. Its architectural home is preparation;
  it will be absorbed into prepare_backend() in Phase 4a. No other placement
  is acceptable for new work.
- **Verifier will be authoritative**: once Phase 5 lands, verifier failure is a
  hard pipeline stop before Wasm planning and emission. The verifier is not
  optional.
- **No new inference in wasm_plan.tw / emit.tw**: any new backend fact must
  live in PreparedModule or a sub-record, not in a new emitter heuristic.

## Open Questions

### How much stage0 machinery should be mirrored?

Mirror the principles, not necessarily the implementation:

- explicit metadata
- strong verification
- stable closure ABI
- representation-aware codegen

Boot has an opportunity to adopt a cleaner architecture instead of replaying the
same historical accretion.

## Checkpoint Value Before Full Migration

Phases 1 through 5 already provide meaningful value even if Phase 6 takes longer
than expected:

- closure ABI and slot/repr facts become explicit in prepared backend IR
- invalid backend states fail in the verifier instead of surfacing late in
  emission
- the rewrite gains a stable checkpoint before planner/emitter migration is
  fully complete

That checkpoint should be treated as a legitimate stopping point for reassessing
scope, sequencing, and residual risk before continuing with full migration.

## Success Criteria

The rewrite is successful when all of the following are true:

- `emit.tw` no longer performs ad hoc local existence reconstruction
- closure captures are explicit prepared-function params, not ambient knowledge
- typed-layout helpers only operate on verified typed representations
- invalid backend states fail in the backend verifier, not in Wasm execution
- the emitter is recognizably mechanical: it lowers prepared backend IR instead
  of re-analyzing program meaning
- self-hosted `boot/main.tw` codegen no longer advances via one-off emitter
  patches
- adding a new backend feature requires extending explicit prepared backend IR
  or verifier rules, not adding more implicit emitter heuristics

## Relationship to Existing Plans

This plan supersedes the implementation strategy of
[`boot-selfhosted-wasm-repr-parity.md`](boot-selfhosted-wasm-repr-parity.md)
where that document assumes the existing emitter/context architecture remains
fundamentally intact.

The representation-parity problem statement still stands, but the proposed fix
here is broader: replace the backend boundary with explicit, verifier-checked
prepared backend IR instead of continuing incremental emitter hardening.
