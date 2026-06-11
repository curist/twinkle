# Native dense-buffer stable merge sort — Approach C

**Status:** rejected (archived) — implemented and measured; the dense path did **not** beat the old recursive merge sort and regresses the pure-`Vector<Int>` sort case. It is correct and stable, but is not a performance win on its own. The real lever remains a typed dense key-index argsort (see [wasm-native-sort.md](../vector-perf/wasm-native-sort.md), "Main vector to attack"). See the [benchmark gate](#benchmark-gate-results) below for the keep/revert evaluation.

## Goal

Make generic `Vector.sort_by` stop doing its heavy merge work through persistent vectors. The public API stays unchanged:

```tw
xs.sort_by(cmp)
xs.sort()
```

Internally, the heavy path copies elements into a flat mutable scratch array, runs a stable bottom-up merge over dense buffers, then freezes the result back to `Vector<T>`.

## Why this is sound

Approach A tried to sort in-place over a `Vector`, but vector writes fell back to persistent copy-on-write. Approach C avoids that failure mode by leaving the persistent vector value model during the hot sort body:

```text
Vector<T> -> Scratch<T> -> dense stable merge -> Vector<T>
```

Scratch writes are real Wasm-GC `array.set` operations. They do not depend on uniqueness analysis and cannot accidentally become persistent COW writes.

This attacks real generic `sort_by` costs:

- allocation of fresh persistent vectors at every merge level;
- trie reads/writes in the merge body;
- recursion overhead from the old prelude merge implementation.

It also keeps stable-sort semantics, which Approach A would not have preserved.

## Expected impact

This is a good foundation, but it should not be judged only against the old Twinkle baseline. The dataframe `order_by` benchmark is still dominated by comparator work: each comparison repeatedly reads keys and null masks through persistent vectors.

So Approach C can improve generic sorting mechanics, but a large dataframe `order_by` win requires the next layer: recognizing/routing key-index sorts to dense key/null/row-id working sets.

## Shape

The sort keeps the existing cheap pre-scan:

```text
already ascending        -> return input
strictly descending      -> reverse
otherwise                -> dense merge path
```

The dense path is:

```text
src = scratch_from_vector(xs)
aux = scratch_new(n)
src = merge_sort_dense(src, aux, n, cmp)
return scratch_freeze(src)
```

The merge is stable: when keys compare equal, it takes the left element first.

## Runtime surface

The implementation uses an internal opaque `Scratch<T>` type backed by the existing mutable Wasm-GC array representation. It is not exported to users.

The load-bearing runtime operations are the scratch allocation/read/write primitives. Vector-to-scratch copy and scratch-to-vector freeze may be implemented in Twinkle first and optimized later with leaf-walk copy / bulk-leaf freeze.

## Next work after this

The main vector to attack is still:

```tw
idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })
```

and the nullable dataframe variant. The end game is to keep that idiom while lowering it to dense key-index sort internally.

## Benchmark gate results

Measured as a controlled A/B on one machine: the committed Approach C (`HEAD`)
vs. the pre-C recursive merge sort (`boot/prelude/vector.tw` from `2dedf33`),
each rebuilt via full `make bundle-cli`. Stability was separately verified by a
new tie-heavy record sort test in `boot/tests/suites/api_vector_suite.tw`
(equal keys keep input order); the full boot suite is green.

N = 1,000,000:

| metric | old recursive merge | Approach C dense | delta |
|---|---:|---:|---:|
| `sort values` (sort `Vector<Int>` directly) | ~808 ms | ~935 ms | +16% (regression) |
| `sort idx key` (`Int.compare(keys[a], keys[b])`) | ~1652 ms | ~1628 ms | ~flat (noise) |
| `order_by` (dataframe `main.tw`) | ~2637 ms | ~2670 ms | ~flat (noise) |

**Verdict: FAIL.** This does not even reach the "WEAK" bar (which required the
dense path to help). The case Approach C should have helped most — sorting a
plain `Vector<Int>`, where the comparator is trivial and only sort mechanics
remain — is the one that regresses. The extra full copy in/out
(`scratch_from_vector` + `scratch_freeze`) plus per-element un-inlined
`scratch_get`/`scratch_set` runtime calls (each an `anyref` cast + bounds-checked
`array.get`/`array.set`) cost more than the old PVec-builder merge saved. On
dataframe `order_by` the opaque comparator's repeated persistent-vector key/null
reads dominate, so swapping the merge mechanics is neutral — exactly as the
consolidated plan predicted.

**Why this matters for direction:** Approach C confirms (like Approach A before
it) that improving generic `sort_by` mechanics is not the lever. The win has to
come from removing the repeated persistent-vector reads inside the comparator,
i.e. lowering key-index sorts to typed dense key/null/row-id working sets
(parent plan Phase 4). The `Scratch<T>` infrastructure built here (Tasks 1–2) is
reusable for that dense-working-set kernel; the `sort_by` rewrite (Task 3) is
the part that earns no current benefit.
