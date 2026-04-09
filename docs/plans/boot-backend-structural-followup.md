# Boot backend structural follow-up

This plan exists because the backend rewrite landed most of its intended
architecture, but real-program exercise still exposed at least one remaining
structural mismatch: some prepared-body paths are broader than neighboring
prepared-body representations assume.

The goal here is not wording cleanup. It is to audit and close the remaining
representation seams that can still produce backend failures of the form “this
value category is valid in one place, but impossible in the adjacent IR node or
consumer”.

## Problem statement

The backend rewrite was meant to make backend facts explicit and mechanically
consumable. A recent failure around closure capture lowering showed that one
important class of issue can still remain after most of the rewrite is done:

- one prepared operand form allows a broader value space
- a nearby prepared op narrows that value space too aggressively
- later passes inherit the mismatch and fail when a real program uses the valid
  but unmodeled case

A concrete example is closure construction payloads:

- prepared operands already distinguish ordinary function-local values from
  non-slot operands such as explicit module-global references
- but closure payload representation may still assume every captured value is a
  slot
- that makes some valid captures impossible to represent even though the rest of
  the prepared IR already admits them

This plan is a targeted audit for remaining mismatches of that shape.

## Goals

- find prepared/backend representations that are narrower than the real value
  space accepted by adjacent passes
- unify operand/value categories across lowering, verifier, planner, and emitter
- add regression tests that stress category boundaries rather than only happy
  paths
- make future backend bugs fail in the verifier or in narrow, obvious lowering
  code rather than during end-to-end boot execution

## Audit focus

Search for places where the code assumes a value is:

- always a `SlotId`
- always function-local
- always representable as a prepared slot operand rather than a general prepared
  operand
- always typed / never erased
- always discoverable from syntax instead of explicit prepared metadata
- always unique by `source_local` in a way that excludes valid multiple roles or
  bridge cases

High-signal patterns to inspect:

- `lookup_slot(...)`
- `find_slot_by_source_local(...)`
- `source_local` reverse-lookup logic
- `PreparedOp` fields typed as `Vector<SlotId>` or `SlotId` where a broader
  operand type may be more correct
- `AGlobalLocal(...)` bridge handling
- verifier helpers that treat non-slot operands as automatically valid
- planner scans that infer backend facts from syntax instead of prepared facts
- emitter helpers that assume every closure/env/member/value source is a slot

## Work plan

### 1. Audit prepared operand categories for structural mismatches

Target:
- `boot/compiler/backend/prepared_ir.tw`
- `boot/compiler/backend/slot_assign.tw`
- `boot/compiler/backend/verify.tw`
- `boot/compiler/codegen/emit.tw`
- `boot/compiler/codegen/wasm_plan_impl.tw`
- `boot/compiler/codegen/wasm_plan_scan.tw`

Deliverables:
- a list of prepared ops whose operand/result categories are too narrow
- a decision for each case: widen representation, prove invariant, or reject in
  verifier
- explicit handling rules for bridge operands such as module globals

Exit criteria:
- every prepared op has an intentional operand category design
- no value category is accepted in one pass but structurally unrepresentable in
  the next pass

### 2. Widen underspecified prepared ops where necessary

Examples of likely candidates:
- closure payload operands
- bridge cases involving `AGlobalLocal(...)`
- any op whose payload is currently slot-only but semantically may include a
  non-slot prepared operand

Deliverables:
- prepared IR changes where needed
- lowering updates in slot assignment
- verifier/planner/emitter updates aligned to the new contract

Exit criteria:
- prepared IR can represent all valid backend cases exercised by the language
- slot-only assumptions remain only where they are truly semantic invariants

### 3. Tighten verifier boundaries around bridge categories

Deliverables:
- verifier rules for any intentionally non-slot operands
- verifier rejection for category combinations that remain intentionally illegal
- diagnostics that name the precise prepared op / operand mismatch

Exit criteria:
- category mismatches fail in verifier or early lowering, not deep in codegen
- bridge operands have explicit rules instead of implicit tolerance

### 4. Add structural regression suites

The main value here is not more tests in general; it is tests that force values
across representation-category boundaries where structural mismatches tend to
hide.

## Test patterns to add

These patterns are meant to surface bugs like the recent closure-capture/module-
global mismatch.

### A. Closure capture category matrix

For each captured value kind, construct a closure and then exercise planning,
verification, and emission:

- captured declared param
- captured let-bound local
- captured pattern-bound local
- captured reassigned local (`AAssign` target and later use)
- captured module global
- captured value flowing through wrap/unwrap boundaries
- captured closure value (closure captures closure)

Why this helps:
- exposes places where closure payloads assume “slot only” or “typed only”
- catches mismatches between closure conversion, slot lowering, verifier, and
  emitter

### B. Use-site matrix for non-slot operands

For every non-slot prepared operand category currently allowed, test whether it
appears in each place it might plausibly flow:

- direct call arg
- closure payload
- record field
- variant payload
- array literal element
- branch condition / match scrutinee if applicable
- return / break value if applicable

Why this helps:
- surfaces asymmetric IR designs where one op accepts a value category but an
  adjacent op cannot represent it

### C. Higher-order bare function matrix

Test direct bare global-function use in all higher-order positions:

- user function parameter of function type
- nested higher-order call
- passed through another function before invocation
- used in a branch before being passed onward
- mixed with closure-wrapped functions in the same call family

Why this helps:
- catches planner/emitter gaps around function signature/trampoline planning
- finds places where closure and bare-function paths diverge structurally

### D. Bridge operand persistence tests

Where a compatibility bridge exists, verify that it survives every relevant pass
without accidental narrowing:

- lower to prepared body
- verify prepared body
- plan Wasm types
- emit Wasm module

Good examples:
- `AGlobalLocal(...)`
- erased/opaque anyref values crossing explicit boundaries

Why this helps:
- catches bugs where a bridge is modeled in one pass but forgotten in another

### E. Duplicate-source and alias-role stress tests

Construct cases where a semantic origin participates in more than one backend
role or flows through rewritten identities:

- captured local rewritten to capture param and also referenced at construction
  site
- pattern-local and ordinary local interactions in nested matches
- reassigned value reused after branch/loop boundaries

Why this helps:
- surfaces over-strong uniqueness assumptions around `source_local`
- catches verifier logic that accidentally assumes a one-to-one identity where
  the backend contract now has a rewritten or bridged form

### F. Real-program end-to-end stress fixtures

Keep a small set of boot-style fixtures that deliberately combine the risky
categories above in one program:

- module globals + closures + higher-order calls
- pattern-bound values captured by closures
- boundary wrap/unwrap temps captured or returned through branches
- container literals carrying mixed operand categories

Why this helps:
- some structural mismatches only show up once several individually-valid
  bridges compose in the same function/module

## Recommended execution order

1. audit prepared operand categories
2. widen underspecified prepared ops
3. tighten verifier boundaries for bridge categories
4. add the structural regression suites above
5. rerun boot self-hosted paths and keep the new fixtures in the regular boot
   test path

## Success criteria

This follow-up is successful when:

- no prepared/backend value category is broader in one pass than the next pass
  can represent
- slot-only assumptions are explicit semantic invariants, not accidental shape
  restrictions
- bridge operands have clear verifier-backed rules
- category-mismatch bugs are caught by targeted tests or verifier failures
  before they become boot-runtime crashes
