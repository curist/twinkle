# Deferred Persistence via Uniqueness Tracking

## Goal

Define the strategy by which Twinkle preserves immutable value semantics while
allowing the compiler to use destructive mutation when provably unobservable.

Collections remain internally mutable while uniquely owned and transition to
persistent (copy-on-write or structurally shared) behavior only at aliasing
points. This avoids pathological O(N^2) copy-on-write in linear update patterns
while preserving the language-level immutability contract.

## Existing Implementation

The core of this strategy is already implemented across both compilers:

- **Stage0 (Rust):**
  - `src/opt/uniqueness.rs` — taint analysis, forward uniqueness tracking,
    point rewrites, loop region (builder) rewrites, loop dict callee-swap
    rewrites, record update annotation
  - `src/opt/liveness.rs` — `live_after` used by the taint pre-scan
  - `src/opt/pipeline.rs` — integrates `uniqueness_rewrite` after peephole
    fixed-point, before `eliminate_defers`
  - `src/codegen/emit.rs` — emits `array.set` / `struct.set` for in-place
    variants, builder runtime calls for region rewrites

- **Boot compiler (Twinkle):**
  - `boot/compiler/opt/uniqueness.tw` — taint analysis, point rewrites
  - `boot/compiler/opt/loop_builder.tw` — vector builder loop rewrite
  - `boot/compiler/opt/builder_region.tw` — builder region detection
  - `boot/compiler/opt/pipeline.tw` — pass integration
  - `boot/compiler/opt/liveness.tw` — liveness analysis

- **Prior plans (archived):**
  - `docs/plans/archive/uniqueness-optimization.md` — original detailed design
  - `docs/plans/archive/dict-loop-in-place.md` — dict loop swap extension

This document consolidates the rationale and design into a single reference.

## Semantic Contract

### What users see

All ordinary values in Twinkle are immutable (spec S2). No program can observe
mutation of a previously constructed value. Updates appear as new values bound
to (possibly the same) names. Shared mutable state is explicit and only
available through `Cell<T>`.

### What the compiler may do

Mutate data structures in place, reuse memory, or delay copying — provided no
observable alias can detect it. This freedom is exercised entirely at compile
time through static analysis; the Wasm GC runtime provides no refcount access
or header flags.

## Value Ownership States

Each heap-backed value (`Vector`, `Dict`, `String`, records) is conceptually in
one of two states, invisible to users:

| State | Meaning |
|---|---|
| **Unique** | Exactly one live reference; safe for in-place mutation |
| **Tainted** | May be aliased, captured, or escaped; COW required on mutation |

A value enters the Unique state when it is a **fresh producer** — a new
allocation with no prior observers:

- Array literal (`AArrayLit`)
- Record construction (`ARecord`)
- Variant construction (`AVariant`)
- `VECTOR_MAKE(n, fill)`, `VECTOR_BUILDER_FREEZE(b)`, `DICT_NEW()`

These states are inferred statically by the uniqueness pass
(`src/opt/uniqueness.rs`). There is no runtime tracking — Wasm GC objects are
opaque to the host and carry no user-controlled metadata headers.

## Pivot Points (Uniqueness Breakers)

A value transitions from Unique to Tainted when the compiler cannot prove
single ownership. The existing `collect_tainted` pre-scan identifies these
points:

### Binding alias

```tw
y := xs
```

If `xs` remains live after this point, both `xs` and `y` may observe the same
object. The analysis taints `xs` when the source is still live in the
continuation (liveness check).

### Closure capture

```tw
fn() { xs }
```

Any local captured by `AMakeClosure` is tainted unconditionally — the closure
may outlive the current scope and retain a reference.

### Storage into aggregates

```tw
rec := MyRecord.{ field: xs }
ys := [xs, a, b]
```

Values stored into arrays (`AArrayLit`), records (`ARecord`), or variants
(`AVariant`) are tainted — the container holds a second reference.

### Function call (non-COW, non-read-only)

```tw
foo(xs)
```

If `foo` is not a known COW operation (`Vector.set`, `Dict.set`, etc.) or a
known read-only operation (`Vector.len`, `Dict.has`, etc.), all local arguments
are tainted conservatively. The callee might retain the reference internally.

### Function parameters

Parameters arrive from outside the analysis scope. All function parameters are
tainted unconditionally at entry.

### Branch-boundary escape

If an alias `y := xs` occurs in a nested scope and `y` escapes (e.g., assigned
outward), then `xs` is tainted when live in the outer continuation.
Conservative: the analysis does not attempt to prove uniqueness across branch
merges.

### What about return?

Returning a value does not taint it within the callee's analysis — the returned
value flows to the caller's fresh binding. Uniqueness of the result is
determined at the call site, not the return site.

## COW Operations and Rewrite Strategies

### Known COW operations

The pass recognizes operations that consume a collection and produce an updated
version:

| FuncId | Operation | Rewrite kind | In-place variant |
|---|---|---|---|
| `VECTOR_SET_UNSAFE` | `xs[i] = v` | Point | `VECTOR_SET_IN_PLACE` |
| `DICT_SET` | `Dict.set(d, k, v)` | Point / Loop swap | `DICT_SET_IN_PLACE` |
| `DICT_REMOVE` | `Dict.remove(d, k)` | Point / Loop swap | `DICT_REMOVE_IN_PLACE` |
| `VECTOR_APPEND` | `xs.append(v)` | Loop region (builder) | builder wrapping |

### Known read-only operations (no taint)

`VECTOR_LEN`, `DICT_LEN`, `DICT_HAS`, `DICT_GET`, `DICT_GET_UNSAFE`,
`DICT_KEYS`. Passing a local to these does not taint it.

### Point rewrite (same-size update)

When a COW operation's base argument is Unique and consumed (dead after use, or
immediately reassigned via `assign(base = result)`), the compiler replaces the
COW call with its in-place variant:

```
// Before
let L5 = VECTOR_SET_UNSAFE(L0, idx, val)
let _  = assign(L0 = L5)

// After
let L5 = VECTOR_SET_IN_PLACE(L0, idx, val)
let _  = assign(L0 = L5)
```

The Wasm emitter lowers `VECTOR_SET_IN_PLACE` to `array.set` (true in-place
mutation on the GC heap).

### Region rewrite (growth in loop)

For operations that change container size (`Vector.append` in a loop), the pass
wraps the enclosing loop with a builder lifecycle:

```
// Before (empty initial vector)
let L0 = []
loop {
  let L5 = VECTOR_APPEND(L0, val)
  let _  = assign(L0 = L5)
  continue
}

// After (empty base → VECTOR_BUILDER_NEW)
let L0 = []
let builder = VECTOR_BUILDER_NEW()
loop {
  let _ = VECTOR_BUILDER_PUSH(builder, val)
  continue
}
let L0 = VECTOR_BUILDER_FREEZE(builder)
```

When the base vector is non-empty (e.g., a parameter or prior computation),
`VECTOR_BUILDER_FROM(base)` is used instead of `VECTOR_BUILDER_NEW()` to seed
the builder with existing elements.

Builder functions (`VECTOR_BUILDER_NEW`, `VECTOR_BUILDER_FROM`,
`VECTOR_BUILDER_PUSH`, `VECTOR_BUILDER_FREEZE`) provide amortized O(1) append
via a doubling buffer internally.

### Loop callee-swap rewrite (dict)

Dict operations inside loops use a simpler strategy than builder wrapping: the
consume-reassign pattern `DICT_SET(d, k, v); assign(d = result)` is rewritten
by swapping the callee to `DICT_SET_IN_PLACE`. No builder lifecycle is needed
because dict mutation does not change container size in the Wasm GC sense.

Read-only operations on the dict base (`Dict.has`, `Dict.get`, `Dict.len`) are
allowed inside the loop without breaking the rewrite.

### Record update

`ARecordUpdate` reuse is inferred by the uniqueness pass. When the base record
is Unique, non-escaped, and consumed, the emitter lowers to `struct.set`
(in-place field mutation).

## Analysis Structure

The pass operates on ANF after peephole optimization.

### Phase 1: Pre-scan (`collect_tainted`)

Walk the entire function body and collect all locals that are provably
non-unique (aliased, captured, stored, or passed to unknown functions). Function
parameters are unconditionally tainted.

### Phase 2: Forward walk

Walk ANF bindings in order, maintaining a `unique: HashSet<LocalId>`:
- Fresh allocation -> insert into unique set
- `AInit(y = x)`: transfer uniqueness from `x` to `y` if `x` not tainted
- `AAssign(r = v)`: transfer uniqueness if applicable
- COW consume-reassign: result inherits uniqueness from consumed base
- Branches/loops: conservative (uniqueness does not propagate out)

### Phase 3: Rewrite

At each COW call site, check whether the base local is in the unique set and
consumed. Apply the appropriate rewrite (point, region, or callee-swap).

## Pipeline Position

```
parse -> resolve -> typecheck -> lower (Core IR) -> monomorphize
  -> lower (ANF) -> peephole opts
  -> UNIQUENESS PASS -> eliminate_defers -> emit (WAT)
```

**Invariant:** No pass running after uniqueness may introduce new aliasing.
Currently `eliminate_defers` does not introduce aliasing.

## Current Implementation Status

| Component | Status |
|---|---|
| Pre-scan taint analysis | Complete (`collect_tainted`) |
| Point rewrite: `VECTOR_SET_UNSAFE -> SET_IN_PLACE` | Complete |
| Point rewrite: `DICT_SET -> DICT_SET_IN_PLACE` | Complete |
| Point rewrite: `DICT_REMOVE -> DICT_REMOVE_IN_PLACE` | Complete |
| Loop region rewrite: `VECTOR_APPEND -> builder` | Complete |
| Loop callee-swap: `DICT_SET/REMOVE -> in-place` | Complete |
| Record update in-place | Complete |
| Boot compiler mirror (Twinkle) | Partial (vector builder done; dict loop in progress) |

## Follow-up Precision Plan

This document is the canonical reference for the semantic contract and the
current implementation. A separate follow-up plan,
[`static-uniqueness-next.md`](./static-uniqueness-next.md), covers the next
round of static-analysis precision work:

- regaining uniqueness after guaranteed-fresh COW results
- path-sensitive propagation through branches and merges
- function-boundary summaries/specialization without runtime tracking

That follow-up plan is intentionally static-only. Runtime refcounts or runtime
uniqueness flags remain out of scope.

## Baseline Measurements

See [`docs/reports/cow-analysis-baseline.md`](../reports/cow-analysis-baseline.md)
for a snapshot of COW operation counts across the full boot compiler
(`boot/tests/main.tw`, 2887 functions, measured 2026-04-02).

Reproduce with: `cargo test --release --test cow_analysis -- --nocapture`

Key takeaway: **1144 COW operations remain** after optimization (~8% rewrite
rate). The three biggest contributors are `VECTOR_APPEND` (751), `DICT_SET`
(177), and `VECTOR_CONCAT` (106). Record updates (98) are entirely un-optimized.

## Testing Strategy

Tests live in `tests/opt_test.rs` with fixtures in `tests/opt/*.tw`. Each
fixture is a self-contained Twinkle program with a comment describing the
expected optimization behavior and runtime output.

### Level 1: Structural verification (ANF inspection)

Compile the fixture through the full pipeline including optimization, then
inspect the resulting ANF for specific callee FuncIds. This is the primary way
to verify the pass did its job.

- **Positive (rewrite happened):** assert `has_call_to(module, SET_IN_PLACE)`
  and `!has_call_to(module, SET_UNSAFE)`.
- **Negative (rewrite correctly suppressed):** assert the COW callee remains
  and no in-place variant appears.
- **Counting:** `count_calls_to` verifies exact rewrite counts when a fixture
  has multiple update sites (e.g., chain of two sets → both rewritten).

Helpers: `compile_opt` (returns optimized ANF), `compile_anf` (returns
pre-optimization ANF for comparison), `has_call_to`, `count_calls_to`,
`has_in_place_update` (for record `can_reuse_in_place` flag).

### Level 2: Runtime correctness (interpreter)

Run the fixture with `assert_runtime_output` and check printed values match
expectations. This catches cases where the rewrite is structurally present but
semantically wrong (e.g., in-place mutation observable through an alias the
analysis missed).

### Level 3: Runtime correctness (Wasm)

Run the same fixture through the Wasm backend with `assert_runtime_output_wasm`.
This catches emitter bugs where the ANF is correct but the generated WAT
miscompiles the in-place variant.

### Level 4: Differential (opt vs no-opt)

Some tests use `assert_runtime_matrix` to run the same fixture in multiple
configurations and assert identical output. This is the strongest correctness
property: the optimization must not change observable behavior.

### Coverage structure

Each rewrite category (vector point, vector loop, dict point, dict loop, record)
has both positive and negative fixtures:

| Category | Positive fixtures | Negative fixtures |
|---|---|---|
| Vector point (`SET_IN_PLACE`) | `vector_set_unique`, `vector_set_from_make`, `vector_set_after_len`, chain/branch variants | `vector_set_aliased`, `vector_set_captured`, `vector_set_param`, `vector_set_stored_in_*`, escape variants |
| Vector loop (builder) | `vector_append_loop_unique`, `vector_append_loop_seeded` | `vector_append_loop_captured`, `vector_append_loop_reads_acc` |
| Dict point (`SET/REMOVE_IN_PLACE`) | `dict_set_unique`, `dict_remove_unique`, `dict_chain_unique` | `dict_set_aliased`, `dict_remove_captured`, `dict_after_user_call`, `dict_stored_in_array` |
| Dict loop (callee swap) | `dict_set_loop_unique`, `dict_remove_loop_unique`, `dict_set_loop_multiple_ops`, `dict_set_loop_with_read` | `dict_set_loop_aliased`, `dict_set_loop_captured` |
| Record (`can_reuse_in_place`) | `record_in_place`, `record_unique_in_place` | `record_aliased`, `record_alias_escape`, `record_capture_escape` |

### Gaps to fill

- **Nested loops:** inner loop builds a vector, outer loop accumulates results
- **Mixed ops in one loop:** vector append + dict set on different locals
- **Function-return fresh producer:** caller updates result of a user function
  that returns a fresh literal (not just `Vector.make`)
- **Branch-then-update:** both branches produce fresh values, update follows
  (currently conservative — worth a negative test confirming no rewrite)

## Interaction with Persistent Data Structures

This optimization is orthogonal to persistent data structures (PVec, HAMT) but
composes well:

- **Without PDS (current):** Shared values use full-copy COW on mutation.
  Uniqueness optimization eliminates copies in the common linear-update case.
- **With PDS (planned):** Shared values use structural sharing (O(log N)
  path-copy). Uniqueness optimization still eliminates even that overhead for
  linear patterns, yielding true O(1) amortized updates.

See `persistent-vector.md` and `persistent-dict.md` for PDS plans.

## What's Not Attempted

- **Runtime uniqueness flags or refcounts:** Wasm GC objects are opaque. No
  runtime tracking is possible or needed.
- **Interprocedural analysis:** Functions are black boxes unless in the known
  COW/read-only sets.
- **Branch-merge precision:** Both sides of a branch conservatively yield
  Tainted. No phi-node reasoning.
- **Alias tracking through containers:** A value stored in a container is
  Tainted permanently.
- **User-visible ownership annotations:** No `@noescape`, no borrow markers.
  The optimization is fully automatic.
- **`VECTOR_CONCAT` rewrite:** Requires proving the two operands don't alias,
  which needs deeper analysis.

## Mental Model (for Users)

Users can think of it as:

> If I keep transforming a value linearly (`xs = xs.append(v)` in a loop), it's
> fast — the compiler reuses the buffer. If I branch it (`ys = xs; xs =
> xs.append(v)`), it becomes persistent and copies as needed.

This is not a guarantee exposed in the spec, but a performance property of the
current compiler.

## Future Work

Detailed next-step analysis work lives in
[`static-uniqueness-next.md`](./static-uniqueness-next.md). The short version:

- **`VECTOR_CONCAT` uniqueness:** Add alias analysis sufficient to prove
  `concat(a, b)` is safe when `b` is not a view into `a`.
- **Loop-carried uniqueness across branches:** Tighten the conservative
  branch-merge rule when both sides provably produce the same unique local.
- **Function-boundary precision:** Add static summaries/specialization for
  non-retaining helper functions, without introducing runtime tracking.
