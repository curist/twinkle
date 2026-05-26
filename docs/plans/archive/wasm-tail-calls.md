# Wasm Tail Calls Plan

## Goal

Teach the boot compiler to emit WebAssembly tail-call instructions for calls in
true tail position. Tail-call support is a required target-runtime capability for
Twinkle-generated Wasm, not an optional compatibility mode.

The immediate target is direct monomorphic calls that currently lower to
`call` followed by `return`. Later phases can extend this to closure calls and
other indirect call paths.

## Current State

Phases 2–4 are complete. The boot compiler emits `return_call` for direct calls
and `return_call_ref` for closure calls in tail position, including propagation
through `if` branches and `case` arms (both if-chain and br-table dispatch paths).

Backend plumbing (pre-existing):

- `boot/compiler/codegen/wasm_ir.tw` defines `ReturnCall` and `ReturnCallRef`.
- `boot/compiler/codegen/wat.tw` prints them as `return_call` and
  `return_call_ref`.
- `boot/compiler/codegen/wasm.tw` serializes them in the binary emitter.
- `boot/compiler/codegen/linker.tw` rewrites them during symbol/type renaming.

Tail-call emission (new):

- `emit.tw`: `try_emit_tail_op` detects `Let(slot, op, Return(ASlot(slot)))`
  patterns and emits tail calls for `ACall`, `AIf`, and `AMatch` ops.
  `emit_tail_expr` / `emit_tail_let` propagate return context so nested
  expressions can also emit tail calls.
- `emit/calls.tw`: `emit_direct_tail_call` pushes args then emits
  `ReturnCall(sym)` instead of `Call(sym)` + result store.
- `emit/match.tw`: `emit_tail_match_op` dispatches to either
  `emit_tail_br_table_match` (3+ variant patterns) or `emit_tail_arm_chain`
  (if-chain fallback), both using `emit_tail_expr` for arm bodies.
- `emit/closures.tw`: `emit_closure_tail_call` uses `ref.test` to try the typed
  fast path (`return_call_ref`) and falls back to the universal erased path.
  `emit_typed_trampoline` generates concrete trampolines for user functions.
  `emit_materialized_closure` creates 3-field concrete closure structs for
  eligible user functions.

All planned phases are complete. Phase 5 (adapter shims) was declined as
low-impact after measurement.

## Non-Goals

- Do not change Twinkle source semantics.
- Do not provide a compatibility mode for runtimes without Wasm tail-call
  support.
- Do not use tail calls across cleanup/finalization boundaries such as future
  `defer` lowering.
- Do not introduce Wasm exception handling as part of this work.

## Design Principles

### 1. Tail calls are part of the required Wasm target

The compiler may emit `return_call` / `return_call_ref` whenever a call is in a
valid Wasm tail position. Runtimes that cannot validate tail-call instructions
are unsupported targets for Twinkle-generated Wasm.

### 2. Tail calls remain semantics-preserving

Tail-call emission must not change Twinkle source semantics, modulo stack usage
and stack traces. The compiler should still emit normal `call` / `call_ref` for
calls that are not in tail position or are not ABI-compatible with the enclosing
return continuation.

### 3. Only emit when the Wasm call signature matches

A tail call is valid only when the callee ABI result matches the enclosing
function continuation exactly.

Initial rule:

- callee params are already prepared as usual
- callee results exactly match the enclosing function results
- no result boxing, unboxing, cast, struct construction, or adapter call remains
  after the call

If result adaptation is needed, keep the old non-tail sequence. A later phase
can introduce typed tail-call adapter shims when that becomes valuable.

### 4. Respect evaluation order

Argument evaluation and side effects must remain identical. The optimization
should replace only the terminal call/return shape, not move computations across
other effects.

## Tail Position Definition

A call is in tail position when its result is immediately returned from the
current function with no remaining work.

Source-level examples that should eventually qualify:

```tw
fn loop(n: Int, acc: Int) Int {
  if n == 0 { acc } else { loop(n - 1, acc + n) }
}
```

```tw
fn dispatch(x: Int) Int {
  case x {
    0 => zero(),
    _ => dispatch(x - 1),
  }
}
```

Implementation should not rely on source syntax directly. Prefer recognizing
the shape after lowering, where function bodies have explicit terminal forms.

## Implementation Plan

### Phase 1: Declare tail calls as a target requirement

- Document tail-call support as part of Twinkle's required Wasm target feature
  set alongside Wasm GC / typed references.
- Update CLI/help/runtime documentation to state that runtimes without tail-call
  support are unsupported.
- Keep existing non-tail call emission for calls that are not in valid tail
  position.

Acceptance criteria:

- Project docs clearly describe tail calls as required target support.
- There is no `--no-wasm-tail-calls` compatibility path in the design.

### Phase 2: Direct-call tail emission ✓

Implemented via `try_emit_tail_op` in `emit.tw`. Instead of an explicit
`EmitContinuation` enum, tail detection works by pattern-matching the ANF shape:
`Let(slot, ACall(f, args), Return(Some(ASlot(slot))))`. When the let-body
immediately returns the same slot the call stores into, and the callee is a
non-builtin user function, `emit_direct_tail_call` emits `ReturnCall(sym)`.

Void-returning and Never-returning calls are excluded (they need result
boxing/adaptation after the call).

Tests in `boot/tests/suites/codegen_emit_suite.tw`:
- tail-position direct call emits `return_call`
- non-tail call does not emit `return_call`
- void-result tail call does not emit `return_call`

### Phase 3: Tail-position propagation through control flow ✓

`emit_tail_expr` and `emit_tail_let` propagate return context through nested
expressions. `try_emit_tail_op` handles three op kinds:

- **AIf**: emits condition, then each branch via `emit_tail_expr`
- **AMatch (if-chain)**: `emit_tail_arm_chain` emits pattern conditions and
  bodies in tail context
- **AMatch (br-table)**: `emit_tail_br_table_match` uses `StructGet + BrTable`
  dispatch with tail-aware arm bodies (3+ variant patterns)

The `atom_is_return` flag allows `Atom(ASlot(s))` to count as a return when
called from `emit_tail_let`, where trailing atoms are semantically returns.

Tests:
- tail call in if branches emits `return_call`
- tail call in case arms emits `return_call` (if-chain path)
- tail call in br-table match emits `return_call` + `br_table`

### Phase 4: Closure and function-reference tail calls ✓

Implemented via `emit_closure_tail_call` in `closures.tw`. For closure calls in
tail position (`ACall(ASlot(closure_slot), args)`), the emitter:

1. Attempts a typed fast path using `ref.test` to check if the closure is a
   concrete typed closure struct (3-field with typed funcref). If so, extracts
   the typed funcref and emits `return_call_ref` with the concrete function type.
2. Falls back to the universal erased path: extracts the universal funcref from
   the base closure struct, boxes arguments, and emits a normal `call_ref` +
   `return` (cannot use `return_call_ref` on the erased path because result
   unboxing is needed after the call).

Supporting changes:

- `emit_materialized_closure` now creates 3-field concrete closure structs for
  user functions that appear in `concrete_func_sigs`, with a typed funcref
  pointing to `$typed_tramp_N`.
- `emit_typed_trampoline` generates typed trampolines that take concrete
  (unboxed) parameters, extract captures from the closure environment, and call
  the real function body directly — no boxing/unboxing overhead.
- `emit_trampolines_for_func` emits both universal and typed trampolines for
  eligible user functions.
- `typed_closure_mono_for_func` guards typed closure creation to user functions
  only (builtins use the base 2-field closure struct).

Tests:
- tail-position closure call emits `ref.test` + `return_call_ref`
- non-tail closure call does not emit `return_call_ref`
- user function closures use concrete struct (`struct.new $closure_*`)

### Phase 5: Optional adapter shims — declined

Many currently non-eligible calls may fail only because of a small result
adapter, such as a cast or typed/erased boundary conversion. The idea was to
generate adapter wrapper functions that perform the call + adaptation, then
tail-call the adapter from the original call site.

After completing Phases 2–4 and measuring the boot compiler output, the
remaining missed tail-call opportunities are too few to justify the added
complexity. The boot compiler emits ~40k calls total, of which 124 are already
`return_call` / `return_call_ref`. The remaining candidates are ~30 builtin
calls in tail position (mostly generated eq-comparison helpers), each needing
trivial result adaptation (i32→Bool cast or similar). Generating adapter shims
for these would add code-size overhead and compilation complexity for negligible
stack-usage or performance benefit.

If a future workload shows meaningful tail-call opportunities behind result
adaptation, this phase can be revisited.

## Validation Strategy

### Static/WAT tests

Add compiler tests that inspect emitted WAT/IR for:

- direct tail recursion uses `return_call`
- direct non-tail recursion does not
- tail calls under `if` and `case` use `return_call`
- non-tail-position calls do not emit `return_call` / `return_call_ref`

### Runtime tests

Add a recursion-depth test that would normally overflow the host/Wasm stack but
succeeds on the required target runtime because tail calls are supported.

### Binary/validator tests

Update the binary subset reference and ensure the emitted binary validates with
the chosen engine/toolchain when tail calls are enabled.

## Runtime Requirement

Tail calls are part of Twinkle's required WebAssembly target. The compiler may
emit tail-call instructions without probing for fallback support.

Required policy:

- document the minimum supported runtimes/engines that validate Wasm GC plus
  tail calls
- fail early or clearly when invoking a bundled runner that lacks tail-call
  support
- do not add a portable fallback mode whose purpose is supporting runtimes
  without tail calls

## Expected Impact On The Current Boot Compiler

A snapshot of the current boot compiler output suggests tail calls are useful but
not likely to be a large immediate throughput win by themselves.

The compiler source already uses explicit `for` loops for many hot traversals,
so tail recursion is not the dominant looping idiom. In the generated boot WAT,
there are many ordinary calls, but only a small subset are in obvious terminal
call shapes. Most direct opportunities are wrapper/delegation functions,
runtime helper exits, and closure trampoline tails. Direct self-tail recursion is
rare in the current emitted module; one clear example is a recursive HAMT lookup
path in the dictionary runtime, whose recursion depth is bounded by the trie
shape.

So the expected near-term value is:

- improved stack behavior for future tail-recursive compiler algorithms
- small wins from replacing terminal delegation calls
- cleaner support for functional/control-flow-heavy library code
- not a major standalone speedup for the current boot compiler pipeline

Larger performance impact would likely require combining tail calls with other
backend work, especially reducing erased adapter chains and enabling
`return_call_ref` for closure/trampoline-heavy paths.

## Risks

- Engine support may lag behind Wasm GC support in some environments.
- Tail calls can make stack traces less informative.
- Incorrectly marking a call as tail could skip required result adaptation or
  cleanup work.
- `return_call_ref` combines tail calls with typed function references, so it
  should be validated separately from direct `return_call`.

## Suggested Milestones

1. ✓ Feature flag and no-op plumbing.
2. ✓ Direct self-recursive `return_call` in simple function tails.
3. ✓ Tail-position propagation through `if` and `case` (including br-table).
4. ✓ Runtime-gated deep recursion test.
5. ✓ Closure `return_call_ref` support.
6. ~~Optional adapter shims~~ — declined (low impact after measurement).
