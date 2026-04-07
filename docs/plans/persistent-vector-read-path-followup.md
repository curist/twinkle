# Persistent vector read-path follow-up

## Current state

Persistent vectors stay in place for the stage0 Wasm backend.

We also keep the runtime-side read-path cleanup in `src/runtime/arr.rs`:
- `rt_arr__get` is monolithic again
- small vectors (`len <= 32`) use a direct tail-only fast path
- tail reads are handled directly in `get`
- the read path no longer depends on `get -> get_leaf -> final array.get`

An experimental stage0 codegen optimization pass was tried and then reverted.
That experiment included:
- chunked lowering for `for x in xs`
- inline stage0 lowering for `xs[i]` and `Vector.get`
- extra Wasm-local machinery to support those paths

The experiment made the backend more complex but did not materially improve the
observed read-heavy regressions, so it is not the direction to keep pursuing.

## What we learned

The persistent vector algorithm itself is still the right family of data structure.
The remaining regression appears more likely to come from the **Wasm GC access
shape** than from the bit-partitioned trie design itself.

The current evidence points more toward:
- dynamic `ref.cast` overhead during trie descent
- nullable/non-null transitions in hot reads
- GC object-graph traversal cost through `VecInternal` / `VecLeaf`
- Wasmtime optimization limits around current Wasm GC patterns

and less toward:
- helper call layering alone
- generic loop lowering alone
- the persistent vector algorithm being fundamentally wrong for the use case

## Path forward

### Phase 1: Measure the remaining read cost more directly

Add focused microbenchmarks or targeted WAT inspection around:
- tiny vectors (`len <= 32`)
- larger vectors hitting the tail path
- trie reads with `shift == 5`
- trie reads with `shift == 10`

Goal:
- separate small-vector behavior from true trie-descent behavior
- identify whether the cost is mostly in casts, nullability, or depth

### Phase 2: Reduce cast pressure in the runtime read path

Investigate ways to cut the number of dynamic type checks on steady-state reads.
Promising directions:
- avoid routing through a generic `VecNode`-style dynamic path where possible
- keep traversal in statically-known `VecInternal` form for as long as possible
- make the final leaf step cheaper if possible
- move proof obligations outward instead of paying repeated dynamic checks per read

Goal:
- remove `ref.cast` and `ref.as_non_null` operations from the hottest part of
  trie traversal wherever the invariants already justify it

### Phase 3: Consider physical layout changes if casts remain dominant

If cast-heavy traversal still dominates after runtime cleanup, prototype a more
uniform node representation.

Candidate direction:
- a single node layout that reduces or removes internal/leaf subtype casting

This is less elegant than the current nominal node hierarchy, but may better fit
current Wasm GC performance characteristics.

Goal:
- preserve persistent-vector semantics while simplifying the runtime read path

## Explicit non-goals for now

Do **not** rush into changing the representation family to something unrelated,
such as:
- RRB trees
- finger trees
- ropes
- HAMT hybrids for vectors

Those solve different problems and are not justified by the current evidence.

Also avoid reintroducing complex stage0-only codegen special cases unless a new
measurement clearly shows that runtime representation costs are no longer the
main bottleneck.

## Recommended next implementation target

The next practical step should be:

1. keep the current runtime `get` cleanup
2. add targeted measurements for small/tail/trie cases
3. prototype one cast-reduction change in `src/runtime/arr.rs`
4. compare again against `795d1c8`

That keeps the codebase simpler while continuing to attack the most plausible
remaining cause of the read-heavy regression.
