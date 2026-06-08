# Native dense-buffer stable merge sort — Approach C

**Status:** implementation direction for generic `Vector.sort_by`; not the final dataframe `order_by` performance lever by itself. See the consolidated performance plan: [wasm-native-sort.md](wasm-native-sort.md).

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
