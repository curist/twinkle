# Uniqueness-Based In-Place Update Optimization

## Goal

Twinkle source semantics are fully immutable — values are never mutated.
But the compiler can safely lower "functional update" patterns to destructive
in-place mutation when it can prove the old value is never observed again.

This document describes a general ANF-level optimization pass that achieves
this without any user-visible ownership system.

## Core Invariant

For a value `x`, the compiler may reuse/mutate its storage at an update point
if and only if:

1. `x` is the **only live reference** to that object (unique)
2. `x` has **not escaped** (not captured, returned, or stored elsewhere)
3. After the update, the old version of `x` is **never observed again** (consumed)
4. The mutation is semantically equivalent to building a fresh updated value

## Uniqueness Model

Three internal states, not visible to users:

| State | Meaning |
|---|---|
| **Unique** | Compiler believes this value has exactly one owner |
| **Shared** | Multiple references may exist, or unknown origin |
| **Escaped** | Definitely not safe for in-place update |

### Fresh producers (-> Unique)

- `AArrayLit([...])` — fresh array allocation
- `ARecord { ... }` — fresh record/struct allocation
- `AVariant { ... }` — fresh variant construction
- Known-consuming update of a Unique input (see below)
- `VECTOR_BUILDER_FREEZE(b)` — fresh exact-size array
- `VECTOR_MAKE(n, fill)` — fresh preallocated array
- `DICT_NEW()` — fresh empty dict

### Uniqueness killers (-> Shared or Escaped)

- Assigning the same value to multiple live locals (aliasing)
- Closure capture (`AMakeClosure { free_vars: [..., x, ...] }`)
- Passing to unknown/external/impure function
- Storing inside another live container
- Return from function
- Branch merge where both sides produce the same local (conservative)

## Consuming Use

A use of `x` is **consuming** when:

- It is the last use of `x` in execution order
- After this point, `x`'s old value is dead

If a consuming use feeds a known COW operation, and `x` is Unique, the
compiler may lower it to a destructive in-place operation.

This is the single most important concept in the pass.

## Two Rewrite Strategies

Wasm GC arrays are fixed-size. This creates two distinct cases:

### Point rewrite (true in-place)

For operations that don't change the container's size:

| Operation | In-place variant | Wasm instruction |
|---|---|---|
| `Vector.set(xs, i, v)` | `VECTOR_SET_IN_PLACE` | `array.set` |
| Record update `{ ..r, field: v }` | `struct.set` | `struct.set` |

Rewrite is local — just replace the op at the call site.

### Region rewrite (builder wrapping)

For operations that grow the container (size changes):

| Operation | Mechanism |
|---|---|
| `Vector.push(xs, v)` | Builder: `builder_from` before loop, `builder_push` in loop, `builder_freeze` after |

Rewrite is non-local — the pass must identify the enclosing loop and wrap
it with builder init/freeze.

The builder runtime functions (`builder_new`, `builder_from`, `builder_push`,
`builder_freeze`) from the `rt.arr` module provide the amortized O(1) push
implementation backed by a doubling buffer.

## The Loop Accumulator Pattern

The most important pattern to optimize. In ANF with `AAssign`:

```
let L0 = init([])                          // fresh -> Unique
loop {
  ...
  let L25 = call VECTOR_PUSH(L0, val)     // consuming use of L0
  let _   = assign(L0 = L25)              // L0 killed, rebound to result
  continue
}
// L0 holds accumulated result
```

Key insight: `AAssign` acts as "kill old value + redefine." Between the
consuming call and the assign, the old value of `L0` is dead. The new value
inherits uniqueness from the COW operation's result.

The analysis must verify:
- `L0` is Unique at loop entry
- Every use of `L0` inside the loop follows the consume-reassign pattern
- `L0` is not used elsewhere in the loop body (between iterations)
- `L0` is not captured or escaped inside the loop

When verified, the pass rewrites:
- Before loop: `builder = builder_from(L0)` (or `builder_new()` if fresh `[]`)
- In loop: replace `call VECTOR_PUSH(L0, v) + assign(L0 = result)` with
  `call BUILDER_PUSH(builder, v)`
- After loop: `L0 = builder_freeze(builder)`

## Known COW Operations Registry

Each COW operation has optimizer metadata:

```
CowOpInfo {
    func_id: FuncId,
    kind: PointUpdate | Growth,
    base_arg_index: usize,    // which arg is the consumed collection
}
```

Initial set:

| FuncId | Operation | Kind | Base arg |
|---|---|---|---|
| `VECTOR_SET` (39) | `Vector.set(xs, i, v)` | PointUpdate | 0 |
| `VECTOR_SET_UNSAFE` (12) | `Vector.set_unsafe(xs, i, v)` | PointUpdate | 0 |
| `VECTOR_PUSH` (11) | `Vector.push(xs, v)` | Growth | 0 |
| `VECTOR_CONCAT` (25) | `Vector.concat(a, b)` | Growth | 0 |
| `DICT_SET` (13) | `Dict.set(d, k, v)` | Growth | 0 |
| `DICT_REMOVE` (29) | `Dict.remove(d, k)` | Growth | 0 |

Record updates (`ARecordUpdate`) are already handled by the existing
`annotate_in_place` pass in `src/opt/liveness.rs` and would be subsumed
by the general pass.

## Analysis: What to Compute on ANF

### 1. Use-count and last-use analysis

For each ANF binding:
- Collect all use sites
- Determine whether each use is the last use in execution order
- Handle `AAssign` as "kill + redefine" — the old value's last use is the
  one immediately before the assign

Existing `src/opt/use_count.rs` provides `count_uses` and
`collect_assigned_locals`. Extend with last-use tracking.

### 2. Escape analysis

For each binding, determine whether the value may outlive the local scope.

Escapes if:
- Returned from the function
- Captured by `AMakeClosure`
- Passed to an unknown/external/impure function (any `ACall` not in the
  known-safe set)
- Stored into a non-unique container (e.g., `AArrayLit` containing the local,
  record field)
- Written to a mutable cell

Conservative: if uncertain, mark as Escaped.

### 3. Uniqueness propagation

Walk ANF bindings in order, maintaining a map `LocalId -> UniquenessState`.

Transfer rules:
- Fresh allocation -> `Unique`
- Consuming use of Unique input through known COW op -> result is `Unique`
- Any uniqueness killer -> `Shared` or `Escaped`
- `AAssign(r = v)` -> kill old state of `r`, inherit state from `v`
- Branch merge -> conservative: `Shared` unless both sides produce the same
  uniqueness (for a first version, just mark `Shared`)

For loops: iterate to fixed point, or conservatively assume all assigned
locals may be `Shared` unless the consume-reassign pattern is detected.

## Pipeline Integration

```
parse -> resolve -> typecheck -> lower (Core IR) -> monomorphize
  -> lower (ANF) -> peephole opts -> UNIQUENESS PASS -> emit (WAT)
```

The uniqueness pass runs after peephole optimization (constant folding, DCE,
copy propagation, branch simplification) because those passes can expose
last-use opportunities and simplify the ANF shape.

The pass produces rewritten ANF where eligible COW operations are replaced
with their in-place variants. The emitter already knows how to emit these
(e.g., `VECTOR_SET_IN_PLACE` emits `array.set`).

### Location in codebase

New file: `src/opt/uniqueness.rs`
Integrated into the pipeline in `src/opt/pipeline.rs`, called after the
existing peephole fixed-point loop and before `annotate_in_place`.

Eventually `annotate_in_place` (record update reuse) can be folded into
the uniqueness pass, since it's a special case of point-rewrite on a
Unique record.

## Staging

| Phase | Work |
|---|---|
| 1 | `src/opt/uniqueness.rs`: uniqueness state map, fresh-producer recognition, escape check, last-use detection |
| 2 | Point rewrites: `VECTOR_SET` -> `VECTOR_SET_IN_PLACE` when unique + consumed |
| 3 | Region rewrites: `VECTOR_PUSH` in loop-accumulator pattern -> builder wrapping |
| 4 | Extend to `DICT_SET`, `DICT_REMOVE`, `VECTOR_CONCAT` |
| 5 | Subsume `annotate_in_place` for records into the general pass |
| 6 | Verification tests: aliasing boundaries, closure capture, branch merges, fallback behavior |

Phase 1-2 are the minimal useful version. Phase 3 handles the benchmark
case (`xs = xs.push(v)` in a loop). Phases 4-6 are incremental extensions.

## What NOT to attempt in v1

- Branch-merge precision (just mark Shared)
- Interprocedural analysis
- Alias tracking through containers
- FFI boundary reasoning
- Values stored in general object graphs

Mark these as Shared/Escaped and move on. Sound and conservative first.

## Relationship to Existing Optimizations

### Collect specializations (Stage 10.1/10.2)

The `collect` syntax is a language construct that the lowerer routes directly
to optimized paths:
- Range-collect: preallocate + fill + slice (size known upfront)
- Vector/iterator-collect: `builder_new` + `builder_push` + `builder_freeze`

These are lowerer-level specializations, not optimizer rewrites. They remain
as-is. The uniqueness pass handles the *user-written* equivalent:
`for x in iter { xs = xs.push(f(x)) }`.

### Record update in-place (`annotate_in_place`)

The existing pass in `src/opt/liveness.rs` sets `can_reuse_in_place` on
`ARecordUpdate` when the base local is dead in the continuation. This is
a correct special case of point-rewrite-on-unique-value. Phase 5 subsumes it.

### Typed closure specialization (Stage 9.6)

Orthogonal. Typed closures eliminate anyref boxing at call sites.
Uniqueness optimization eliminates unnecessary copying at update sites.
They compose cleanly.
