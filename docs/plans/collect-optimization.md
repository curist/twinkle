# Collect Optimization — Eliminating O(N²) Array Building

## Problem

`collect i in range(n) { body }` currently compiles to three builder intrinsics:

```
VECTOR_BUILDER_NEW()           → Cell (a 1-element outer array wrapping an empty inner array)
loop:
  VECTOR_BUILDER_PUSH(b, x)   → concat(b[0], [x]); b[0] = result
VECTOR_BUILDER_FREEZE(b)       → b[0]
```

`VECTOR_BUILDER_PUSH` is O(N): it calls `rt_arr__concat` which allocates a fresh N+1 element
array and copies all existing elements every iteration. Total cost: O(N²) allocations + copies.

Measured impact: 1 000-element `collect` + `fold` takes ~55 ms — dominated entirely by array
building, not by the closure dispatch the benchmark was meant to measure.

## Root Cause

WebAssembly GC arrays are **fixed-size**. There is no `array.push`. Wasm does provide:

| Instruction | Meaning |
|---|---|
| `array.new_fixed $t n` | n-element array, values from stack (compile-time n) |
| `array.new $t`          | n-element array, default value + length from stack (runtime n) |
| `array.set $t i v`      | mutate element in place |

The builder exploits `array.set` only for the outer Cell slot (index 0). The inner array itself
is never mutated — it is replaced wholesale by `rt_arr__concat` each push.

## Proposed Fix: Two-Phase Strategy

### Phase 1 — Range-collect specialization (common case, high impact)

**Target pattern:** `collect i in range(n) { body }`
(covers `range(n)`, `range_from(a,b)`, `range_step(a,b,s)`)

When the source of `collect` is a `Range` record, the iteration count is fully known at the
start of execution (even if not at compile time). We can:

1. Compute `len = (range.end - range.start + range.step - 1) / range.step` (ceiling division).
2. Emit `ref.null none; local.get len; array.new $Array` — **one allocation**.
3. In the loop body, emit `array.set $Array idx result` — **zero extra allocations**.
4. Return the pre-allocated array directly — no freeze needed.

Resulting WAT sketch for `collect i in range(n) { i * 2 }`:

```wat
;; allocate
ref.null none
local.get $len        ;; i32 length
array.new $Array      ;; -> ref (Array of len nulls)
local.set $arr

;; fill loop  (idx = 0 .. len)
loop $cont
  local.get $idx
  local.get $len
  i32.ge_u
  br_if $break

  ;; body: i * 2 (boxed as anyref)
  local.get $idx
  i64.extend_i32_s
  i64.const 2
  i64.mul
  struct.new $BoxedInt

  ;; array.set $arr[$idx] = body_result
  local.get $arr
  local.get $idx
  ;; swap so stack is (arr, idx, val) for array.set
  ...
  array.set $Array

  local.get $idx
  i32.const 1
  i32.add
  local.set $idx
  br $cont
end $break

local.get $arr
```

**Where to implement:** The lowering pass (`src/ir/lower.rs`) already has
`lower_collect_range` and `lower_collect_iterator` branches. Add a third branch that detects
a Range source and emits a `VectorInit { size_expr, body_expr }` Core IR node (or inline the
specialization directly into the ANF lowering).

Alternative: add a **post-ANF rewrite** in the optimizer (`src/opt/`) that recognises the
`NEW → LOOP(PUSH) → FREEZE` pattern when the loop bound is a Range and replaces it with a
`PREALLOC → LOOP(SET)` pattern. This keeps the lowerer simpler.

### Phase 2 — Exponential-growth builder (general case)

For `collect x in some_vector { f(x) }` or `collect x in some_iterator { g(x) }` the count
is not known upfront. Replace the current O(N²) builder with a doubling-buffer scheme:

State: `(buf: anyref, len: i32, cap: i32)` stored in a 3-element `$Array`.

- `builder_new`: allocate buf of capacity 8, len = 0, cap = 8.
- `builder_push(b, v)`:
  - if `len < cap`: `array.set buf[len] v; len++` — **O(1), no allocation**.
  - else: allocate `new_buf` of size `cap * 2`; copy old elements; `buf = new_buf; cap *= 2`.
- `builder_freeze(b)`: `array.copy new_arr[0..len] from buf` — one final allocation of exact size.

Total cost: O(N) allocations (amortised), O(N) copies.

This requires adding a Wasm `array.copy` instruction to the IR (already in the GC spec) and
implementing the builder in the codegen emit layer (or as a runtime Wasm module).

## Recommended Sequencing

| Stage | Work | Impact |
|---|---|---|
| 10.1 | Range-collect specialization in lowerer/optimizer | Eliminates O(N²) for the common case; 10–100× speedup on range-based benchmarks |
| 10.2 | Exponential-growth builder for general collect | O(N log N) → near-O(N) for vector/iterator sources |
| 10.3 | `array.copy` instruction in Wasm IR + linker | Prerequisite for Phase 2 freeze step |

Stage 10.1 alone fixes the benchmark regression and requires no new IR nodes or runtime
changes — just a codegen specialization in `emit.rs` (or a rewrite pass in `opt/`).

### Status (2026-03-06)

- Stage 10.1: implemented (range collect prealloc + set + final slice).
- Stage 10.2: implemented with runtime builder functions in `rt.arr`:
  `builder_new`, `builder_push`, `builder_freeze`.
- Lowering now routes vector/iterator collect accumulation through
  `VECTOR_BUILDER_*` so growth is amortized instead of concat-per-push.

### Benchmark Snapshot (2026-03-06)

- Closure-focused checkpoint (Stage 10.1 baseline):
  `docs/benchmarks/stage10-1-baseline-2026-03-06.md`
- Stage 10.2 collect-path comparison:
  `docs/benchmarks/stage10-2-collect-comparison-2026-03-06.md`

Key number from Stage 10.2 collect-path benchmark:
- `collect_builder` vs persistent `manual_push`: **~75.14x** median speedup
  on iterator collect workload (`N=1000`).

## Generalization Path (Beyond Collect)

The same idea can be lifted into a broader optimization pass:
"internally mutable when unique, externally immutable by semantics."

### Pass concept: uniqueness-based mutability elision

1. Mark fresh allocations (`array.new`, record construction, builder state) as
   **unique** while they have a single owning local and do not escape.
2. Rewrite copy-on-write updates (`rt_arr__set`, `concat`-style growth, record
   rebuilds) to in-place writes when the target is provably unique.
3. If uniqueness is lost (aliasing, capture into closure, return/store), fall
   back to the existing persistent/copying behavior.

### Why this is better than one-off collect rewrites

- Covers `collect`, indexed vector updates, staged record construction, and
  future dict/string builders under one framework.
- Preserves language-level immutable value semantics.
- Keeps correctness simple via "prove-unique or fall back" discipline.

### Suggested staging

| Stage | Work |
|---|---|
| 11.1 | Add local uniqueness/escape analysis over ANF/Core locals |
| 11.2 | Introduce internal in-place intrinsics (array/record set) gated by uniqueness |
| 11.3 | Rewrite COW update patterns to in-place form when safe |
| 11.4 | Add verification tests for aliasing boundaries and fallback behavior |

### General Optimization Pattern (Design Note)

Use this as the default template for immutable-semantic performance rewrites:

1. Detect a persistent-update pattern:
   `x1 = alloc(...)`; `x2 = cow_set(x1, ...)`; `x3 = cow_set(x2, ...)` ...
2. Prove uniqueness for the backing value through the update region:
   single owner local, no capture, no store to aggregate/global, no unknown calls
   that may retain aliases.
3. Rewrite to internal mutating ops:
   `x = alloc(...)`; `set_in_place(x, ...)`; `set_in_place(x, ...)`; ...; `freeze_or_return(x)`.
4. Insert deopt fallback when proof fails:
   keep or restore the original persistent (copying) path.

Safety invariants:

- Observable behavior must remain value-immutable at language boundaries.
- In-place writes are permitted only while uniqueness proof is valid.
- Any potential alias boundary invalidates uniqueness immediately.
- Optimization must be monotonic-safe: if analysis is uncertain, do not rewrite.

Canonical rewrite targets:

- `Vector.push` growth loops (`concat` chains) -> prealloc/set or amortized builder.
- Repeated `Vector.set` on fresh vectors -> `set_in_place`.
- Record/dict staged construction -> internal mutating builder + final immutable value.

## Implementation Notes

- `array.new $t` in the Wasm IR is `Instr::ArrayNew(TypeSym)`. It pops `(default: anyref,
  len: i32)` from the stack and pushes a fresh array. Already in the IR.
- `array.set $t` is `Instr::ArraySet(TypeSym)`. Pops `(arr, idx: i32, val)`. Already in IR.
- Range field access (start/end/step) uses `ARecordGet` with `RANGE_TYPE_ID` and field indices
  0/1/2.  The length expression is `(end - start) / step` (assuming step > 0; sign handling
  needed for negative step).
- The body expression of `collect` is already isolated as a single `AnfExpr` after ANF
  lowering; reusing it in the pre-alloc loop requires no structural change to the ANF shape.
- The `$Array` type is `anyref`-element; `array.new` default value is `ref.null none`
  (heap type `None`).
