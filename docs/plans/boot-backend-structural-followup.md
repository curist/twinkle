# Boot backend structural follow-up

This plan exists because the backend rewrite landed most of its intended
architecture, but real-program exercise still exposed at least one remaining
structural mismatch: some prepared-body paths are broader than neighboring
prepared-body representations assume.

The goal here is not wording cleanup. It is to audit and close the remaining
representation seams that can still produce backend failures of the form "this
value category is valid in one place, but impossible in the adjacent IR node or
consumer".

## Problem statement

The backend rewrite was meant to make backend facts explicit and mechanically
consumable. A recent failure around closure capture lowering showed that one
important class of issue can still remain after most of the rewrite is done:

- one prepared operand form allows a broader value space
- a nearby prepared op narrows that value space too aggressively
- later passes inherit the mismatch and fail when a real program uses the valid
  but unmodeled case

## Audit results (completed 2026-04-09)

A full audit of all backend passes was performed. The findings below replace the
original speculative audit focus list.

### Finding 1: AGlobalLocal verifier bypass — CONFIRMED, actionable

All 6 verification points in `verify.tw` unconditionally accept `AGlobalLocal`
with no type or repr validation:

| Function | Line | Behavior |
|---|---|---|
| `infer_atom_mono` | 547 | Returns `.None` — no type inferred |
| `verify_atom_mapped` | 583 | Returns `.Ok({})` |
| `verify_atom_use` | 594 | Returns `.Ok({})` |
| `verify_opkind_atom` | 684 | Returns `.Ok({})` — Int-as-condition not caught |
| `verify_condition_atom` | 699 | Returns `.Ok({})` — no I32 check |
| `require_local_repr_if_local` | 740 | Returns `.Ok({})` — no repr check |

The emitter derives mono/repr on-the-fly from `registry.module_globals`
(`emit.tw` lines 525-551), but the verifier never cross-references that
registry. Type mismatches (e.g., an Int-typed module global used as a branch
condition) pass verification silently and only surface during emission or Wasm
validation.

The planning pass (`wasm_plan_impl.tw:87-98`) is the only strict checkpoint —
it panics on unregistered globals. But type-level mismatches are not caught
anywhere before the emitter.

### Finding 2: AMakeClosure slot-only invariant — SAFE by construction

The original concern was that module globals could leak into closure free_vars
via `AMakeClosure(FuncId, Vector<SlotId>)`. The audit confirmed this cannot
happen:

1. `lower_core.tw:246-248` — module globals produce `GlobalLocal(gid)` in
   Core IR, a distinct variant from `Local(id)`
2. `collect_free_vars_inner` (`lower_core.tw:1198`) — only captures `.Local(id)`
   (line 1200); `GlobalLocal` falls through to the `_ =>` catch-all (line 1297)
   and is not collected
3. Free-var collection happens on Core IR **before** ANF lowering erases the
   distinction (`lower_anf.tw:256/513` converts both to `ALocal`)

The invariant holds. However, it is enforced only by a runtime trap in
`slot_assign.tw:253` (`lookup_slot` errors on missing ID) rather than by a
verifier rule. A defense-in-depth assertion would improve diagnostics.

**No prepared IR widening is needed.** `AMakeClosure(FuncId, Vector<SlotId>)`
is the correct representation — closures genuinely only capture function-local
slots.

### Finding 3: source_local ID collision — SAFE by numeric invariant

The emitter checks `module_globals[entry.source_local.id]` for every `ASlot`
in non-init functions (`emit.tw:492`). The concern was that a closure capture
param's `source_local` might numerically match a module global ID.

This is safe because IDs are allocated in guaranteed-disjoint ranges:
1. Module globals get IDs 0..N via `next_global` (`lower_core.tw:2540`)
2. Function locals start at `max(next_local, next_global)` (`lower_core.tw:2583`)
3. Closure conversion allocates `param_local` IDs above
   `max_local_id_module(anf) + 1` (`closure_convert.tw:30`)

Capture param IDs are always strictly above module global IDs. No collision is
possible under the current allocation scheme. However, this safety relies on an
implicit cross-pass numeric invariant with no assertion.

### Finding 4: repr_assign coverage — SOUND

repr_assign treats all slot categories (CaptureParam, Param, Local,
PatternLocal) identically through the same `repr_of_mono` path. Only
`DeadPlaceholder → DeadValue` is special-cased. Boundary nodes are handled
correctly:
- `AWrapAnyref` result → forced to `OpaqueAnyref`
- `AUnwrapAnyref` result → repr derived from target mono
- All others → `repr_of_mono(info.mono, env)`

The emitter has hard `require_typed_record_atom` / `require_typed_sum_atom` /
`require_closure_atom` guards that catch repr mismatches at emit time.

**No changes needed.**

### Finding 5: Emitter silent fallbacks — low risk, poor hygiene

`atom_mono` returns `.Void` and `atom_repr` returns `.OpaqueAnyref` when an
`AGlobalLocal` isn't in the registry (`emit.tw` lines 528, 549). In practice
these are unreachable because `emit_atom` (line 504) panics first. But the
fallbacks mask errors if call ordering ever changes.

## Remaining work plan

### 1. Tighten verifier for AGlobalLocal (primary deliverable)

This is the only confirmed structural gap with real risk. The verifier should
resolve `AGlobalLocal` mono/repr from registry data and validate it the same
way it validates `ASlot` operands.

Concrete changes in `boot/compiler/backend/verify.tw`:

- **`infer_atom_mono` (line 547):** Look up `AGlobalLocal(lid)` in a
  module_globals map and return `Some(mono)` instead of `None`. This requires
  threading module_globals data into the verifier (currently it only receives
  `closure_captures` and `funcs`).
- **`verify_opkind_atom` (line 684):** Resolve the global's repr and check it
  against the allowed set for the op kind, same as the `ASlot` path.
- **`verify_condition_atom` (line 699):** Resolve the global's repr and check
  for I32 compatibility.
- **`require_local_repr_if_local` (line 740):** Resolve the global's repr and
  check against the allowed set.
- **`verify_atom_use` (line 594):** Confirm the global exists in the registry
  (catch unregistered globals before they reach the emitter).

The verifier's `verify_prepared_module` function will need to accept a
module_globals map (or a broader registry reference) as an additional parameter.

Exit criteria:
- `AGlobalLocal` operands are validated for mono/repr compatibility
- Unregistered globals are rejected with a clear diagnostic
- No `.Ok({})` bypass remains for `AGlobalLocal` in validation paths

### 2. Defense-in-depth assertions (low priority)

These address invariants that are currently safe but rely on implicit
cross-pass properties:

- **Closure capture assertion:** Add a verifier check that every `SlotId` in
  `AMakeClosure` free_vars has role `CaptureParam`, `Param`, or `Local` — not
  `DeadPlaceholder`. (The current verifier at line 279 already checks capture
  count and source_local alignment, but doesn't check roles.)
- **source_local disjointness:** Consider an assertion in `wasm_plan_impl.tw`
  that no `module_globals` key appears as a `source_local.id` in any non-init
  function's slot map.
- **Emitter fallback cleanup:** Replace the silent `.Void` / `.OpaqueAnyref`
  fallbacks in `atom_mono` / `atom_repr` (lines 528, 549) with `error()` calls,
  since `emit_atom` already panics on missing globals.

### 3. Regression tests

Focus on the confirmed gap (AGlobalLocal validation) and defense-in-depth
cases. The original test matrix has been trimmed to match actual findings.

#### A. Module global usage matrix

Test `AGlobalLocal` in every position where verifier validation was previously
skipped:

- module global as arithmetic operand (Int binop)
- module global as if-condition (Bool)
- module global as record-get target
- module global as match scrutinee (sum type)
- module global as call argument
- module global as closure call callee (function-typed global)
- module global as return value

These directly exercise the new verifier rules from step 1.

#### B. Closure capture category matrix

Verify that all capture value kinds round-trip through the pipeline:

- captured declared param
- captured let-bound local
- captured pattern-bound local
- captured reassigned local
- captured closure value (closure captures closure)

Note: "captured module global" is intentionally excluded — the audit confirmed
this is impossible by construction. A test that a module global referenced
inside a closure body compiles correctly (via `AGlobalLocal`, not capture) is
useful instead.

#### C. Higher-order bare function matrix

Test bare `AGlobalFunc` in higher-order positions:

- user function parameter of function type
- nested higher-order call
- mixed with closure-wrapped functions in the same call

#### D. End-to-end stress fixture

One fixture combining module globals + closures + higher-order calls +
pattern-bound captures in a single module, exercising all the above paths
together.

## Recommended execution order

1. Thread module_globals into verifier; add `AGlobalLocal` validation rules
2. Add module global usage matrix tests (A) to confirm verifier catches errors
3. Clean up emitter silent fallbacks
4. Add defense-in-depth assertions
5. Add remaining regression tests (B, C, D)

## Success criteria

- `AGlobalLocal` operands are validated by the verifier for type/repr
  compatibility, matching the same rigor applied to `ASlot` operands
- Unregistered module globals are caught by the verifier, not by emitter panics
- The closure slot-only invariant and source_local disjointness invariant have
  explicit assertions rather than relying on implicit numeric properties
- Category-mismatch bugs are caught by targeted tests or verifier failures
  before they become boot-runtime crashes
