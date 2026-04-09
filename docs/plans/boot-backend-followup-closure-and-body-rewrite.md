# Boot backend follow-up plan

This plan covers the architectural gaps left after the backend rewrite landed its
prepared metadata, verifier, planner migration, and slot-backed emission.

The remaining work is not about comment cleanup or wording. It is about making
the backend contract fully explicit in the code.

## Goals

- make closure conversion rewrite hoisted bodies to explicit capture params
- make prepared function bodies backend-native instead of semantic ANF plus metadata
- remove remaining planner rediscovery/fallback behavior that should be explicit
- expand verifier coverage as the stronger backend contract becomes representable

## Current gaps

- closure capture metadata exists, but hoisted bodies still rely on semantic
  `LocalId` references rather than explicit capture params
- `PreparedFunc.body` is still semantic `AnfExpr`
- planner still scans `prepared.anf` for several concerns and retains some
  transitional behavior around higher-order global function cases
- emitter allocates from prepared slots, but body lowering still resolves
  semantic `LocalId`s through a map
- verifier is authoritative for current prepared metadata, but not yet for the
  full operand/repr story

## Work plan

### 1. Rewrite hoisted functions to explicit capture params

Target:
- `boot/compiler/backend/closure_convert.tw`
- prepared/backend tests

Deliverables:
- hoisted function signatures include explicit leading capture params
- hoisted function bodies are rewritten to read those params directly
- capture use no longer depends on ambient semantic-local identity

Exit criteria:
- closure ABI is explicit in both metadata and rewritten body shape
- planner/emitter/verifier no longer need to treat captures as special ambient locals

### 2. Introduce a backend-native prepared body form

Target:
- `boot/compiler/backend/prepared_ir.tw`
- preparation/rewrite passes
- emitter/verifier tests

Deliverables:
- prepared body operands refer to backend identities such as slots rather than
  semantic `LocalId`
- repr-sensitive operations remain explicit in the prepared body form
- `PreparedFunc.body` is no longer just semantic `AnfExpr`

Exit criteria:
- emitter lowers prepared bodies without semantic-local reconstruction
- prepared IR stands on its own as a backend contract, not ANF plus side tables

### 3. Migrate emitter from slot-backed to slot-native lowering

Target:
- `boot/compiler/codegen/emit.tw`
- codegen tests

Deliverables:
- body lowering reads slot-native prepared operands directly
- semantic `LocalId` lookup maps disappear from expression emission
- remaining local-shape compatibility logic is deleted

Exit criteria:
- emission is mechanical over prepared backend IR
- `lookup_local`-style semantic-local failure modes are gone entirely

### 4. Remove planner rediscovery leftovers

Target:
- `boot/compiler/codegen/wasm_plan.tw`
- `boot/compiler/codegen/wasm_plan_impl.tw`
- planner tests

Deliverables:
- planner facts that matter for lowering are carried in prepared metadata
- higher-order global function cases no longer rely on fallback-style empty
  capture layout synthesis
- body scanning is limited to facts that genuinely belong to syntax traversal

Exit criteria:
- planner is prepared-IR-driven for backend facts
- rediscovery logic is deleted rather than merely bypassed

### 5. Expand verifier scope to the stronger contract

Target:
- `boot/compiler/backend/verify.tw`
- verifier test suite

Deliverables:
- verifier checks slot-native body operands
- verifier checks repr-sensitive ops against prepared operands directly
- closure-call and branch/join invariants move from emitter assumptions into
  verifier-enforced rules

Exit criteria:
- invalid prepared backend states fail before planning/emission
- the verifier describes the actual backend contract rather than a subset of it

## Recommended order

1. explicit capture-param body rewrite
2. backend-native prepared body form
3. slot-native emitter lowering
4. planner cleanup
5. verifier expansion

## Why this order

Closure body rewriting removes the largest remaining source of ambient semantic
meaning. Once captures and local operands are explicit in the prepared body,
planner, emitter, and verifier can all become simpler and more mechanical.
