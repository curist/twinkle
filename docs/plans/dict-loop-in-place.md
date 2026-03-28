# Dict Loop In-Place Optimization

## Goal

Extend the uniqueness optimization pass to rewrite `DICT_SET` and `DICT_REMOVE` calls
inside loops to their in-place variants (`DICT_SET_IN_PLACE`, `DICT_REMOVE_IN_PLACE`)
when the dict local follows the consume-reassign pattern. Apply the fix in both the
stage0 (Rust) and boot (Twinkle) compilers.

## Motivation

The current uniqueness pass handles two categories:

1. **Point rewrite** (non-loop): `DICT_SET → DICT_SET_IN_PLACE` when the dict is provably
   unique and consumed. This works today in both compilers.
2. **Loop rewrite**: Only handles `VECTOR_PUSH → builder_from/builder_push/builder_freeze`.
   Dict operations inside loops are left as COW copies.

Dict's COW `set` copies the entire backing structure on every call. Inside a loop doing
N insertions, this is O(N²) total work. The boot compiler (self-hosted, runs via `twk run`
through the Wasm backend) has ~72 `Dict.new()` sites and heavy dict mutation in loops
throughout:

- **checker.tw**: `subst` dict updated on every unification
- **resolver.tw**: type/function registration loops, fixed-point convergence loops
- **monomorphize.tw**: `spec_map`, `processed` tracking in BFS queue processing

Both `twk run` (Wasm backend) and `twk build` compile through ANF and run the optimization
passes — so this rewrite benefits both user programs and boot compiler performance.

## Current State in Both Compilers

### Stage0 (Rust) — `src/opt/uniqueness.rs`

- `analyze_loop_expr` hardcodes `prelude::VECTOR_PUSH` (line 299)
- `rewrite_loop_expr` hardcodes `VECTOR_PUSH → VECTOR_BUILDER_PUSH` (line 364–370)
- Dict COW ops are registered in `cow_op_info` with `in_place_rewrite: Some(...)`, but
  never consulted by the loop analysis path
- Point rewrite at line 688–706 handles dict ops outside loops correctly

### Boot (Twinkle) — `boot/compiler/opt/`

- `analysis.tw`: `analyze_loop_push_sites` is parameterized by `push_id` but only called
  with the vector push ID
- `loop_builder.tw`: `rewrite_loop_expr` assumes builder lifecycle (push → builder_push)
- `uniqueness.tw`: `try_loop_rewrite` calls `rewrite_loop_region` which requires a
  `BuilderConfig` — dict ops have no builder equivalent
- `semantics.tw`: `DICT_SET` and `DICT_REMOVE` registered as COW ops with
  `in_place_equivalent`, but never reached by the loop rewrite path

Both compilers have the same structural gap: the loop rewrite path is vector-builder-specific
and has no generic mechanism for "just swap the callee" rewrites.

## Current ANF Shape

`d[k] = v` inside a loop lowers to:

```
let L20 = call GlobalFunc(13) (L2, "k", L7)   // DICT_SET(d, k, v)
let L21 = assign(L2 = L20)                     // d = result
```

This is exactly the consume-reassign pattern (`call COW_OP(base, ...) + assign(base = result)`)
that the point rewrite already recognizes. The gap is that the loop analysis only matches
`VECTOR_PUSH` and rejects any other use of the base local.

## Approach

Dict doesn't need a builder lifecycle. Unlike vectors (which need `builder_from` before
the loop and `builder_freeze` after), dict's in-place variant (`DICT_SET_IN_PLACE`) is a
drop-in replacement — same signature, same return value, just mutates instead of copying.
This makes the loop rewrite simpler than the vector case: no wrapping, just a callee swap.

### Phase 1: Stage0 — extend loop analysis and rewrite

**File: `src/opt/uniqueness.rs`**

1. In `analyze_loop_expr`: alongside the `VECTOR_PUSH` check, also accept calls to
   COW ops (via `cow_op_info`) that have `in_place_rewrite: Some(...)` and follow the
   consume-reassign pattern on `base`. These are "in-place-swappable" ops.

2. Add a new rewrite path in `rewrite_expr` (or alongside `rewrite_loop_expr`): when a
   loop body contains only in-place-swappable dict ops on a unique base (no vector push),
   skip the builder wrapping and just swap callee IDs inside the loop.

3. For mixed loops (vector push on one local + dict set on another), the two rewrites
   are independent — apply each to its own base local.

Key difference from vector rewrite:
- Vector: needs `builder_from` before loop, `builder_freeze` after, and callee change
  from `VECTOR_PUSH` to `VECTOR_BUILDER_PUSH`
- Dict: just swap `DICT_SET → DICT_SET_IN_PLACE` (or `DICT_REMOVE → DICT_REMOVE_IN_PLACE`)
  in the call site, keep the assign-back as-is

### Phase 2: Boot compiler — same pattern

**Files: `boot/compiler/opt/uniqueness.tw`, `boot/compiler/opt/analysis.tw`**

1. In `uniqueness.tw` `try_loop_rewrite`: before falling through to `rewrite_loop_region`
   (which requires builder config), check if the loop body contains dict COW ops on the
   candidate base. If so, apply the simpler in-place swap rewrite.

2. In `analysis.tw`: add `analyze_loop_dict_sites` (or generalize `analyze_loop_push_sites`)
   to recognize `DICT_SET`/`DICT_REMOVE` consume-reassign patterns.

3. Add a `rewrite_loop_dict_expr` (or extend `rewrite_loop_expr`) that swaps callee IDs
   without builder wrapping.

### Phase 3: Read-op passthrough

Dict reads (`dict.get`, `dict.has`, `dict.len`, `dict.keys`) may appear in the same loop
alongside dict mutations. These don't consume the base, so they should not block the
in-place rewrite. Ensure both `analyze_loop_expr` (stage0) and `analyze_loop_push_sites`
(boot) treat read-only dict ops as safe non-consuming uses of the base local.

In stage0: extend `is_no_retain_read_only` to include dict read ops.
In boot: extend the `ReadOnly` effect classification in `semantics.tw` to cover dict reads
(some may already be classified correctly).

## Validation

### Tests to add (both compilers)

- Simple dict accumulation loop → in-place rewrite applied
- Dict remove in loop → in-place rewrite applied
- Multiple dict ops per iteration on same base → all rewritten
- Dict set + dict read in same loop → rewrite applied (reads don't block)
- Dict passed to non-COW function in loop → rewrite blocked (tainted)
- Dict aliased before loop → rewrite blocked (tainted)
- Mixed vector push + dict set on different locals → both rewritten

### Existing test suites

- Stage0: `cargo test` (includes opt uniqueness tests)
- Boot: `cargo run --release -- run boot/tests/main.tw`
- WAT output: verify `rt_dict__set_in_place` appears instead of `rt_dict__set`

## Risks

- **Semantic correctness**: In-place mutation is only safe when the dict is uniquely owned.
  The existing taint analysis and consume-reassign checks guard this, but extending to
  loops adds surface area. Dict-specific edge cases:
  - Dict referenced by closure captured in the loop
  - Dict stored in a record field and accessed through the field
  - Dict passed as argument to another function inside the loop
- **Interaction with persistent dict (HAMT)**: The persistent-dict plan preserves the
  `DICT_SET_IN_PLACE` / `DICT_REMOVE_IN_PLACE` contract. This optimization is compatible
  with the future HAMT implementation and becomes more valuable with it (in-place HAMT
  path mutation vs full path-copy).
- **`analyze_loop_expr` generalization**: Changing from VECTOR_PUSH-specific to generic
  COW-op matching could accidentally accept ops that aren't safe to rewrite. Guard by
  checking `cow_op_info` and verifying that `in_place_rewrite` is `Some`.
