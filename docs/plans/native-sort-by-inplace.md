# Native in-place `Vector.sort_by` — Design Spec (Approach A)

**Status:** Approach A measured **insufficient** (in-place writes did not materialize; `order_by` N=1M regressed ~2531ms → ~5589ms). **Escalating to Approach C** (flat dense scratch + stable merge), which is a separate plan. The Task 2 quicksort prelude change is being reverted to restore the stable merge-sort baseline; the Task 1 characterization tests are kept (they are stability-agnostic and will validate Approach C too).
**Parent plan:** [wasm-native-sort.md](wasm-native-sort.md) — this is a concrete, staged execution of that plan's "general win" direction (every `sort_by` benefits), starting with the lowest-risk variant.

## Goal

Make every idiomatic `Vector.sort_by` (and therefore `Vector.sort`, and the
dataframe `order_by` comparator path) faster by sorting **in place over a
uniquely-owned working buffer** instead of allocating a fresh persistent vector
at every merge level. Keep the closure comparator — this is a general algorithm
improvement, not a comparator-shape or dataframe-specific specialization.

## Why this and not the alternatives

The motivating hotspot is dataframe `order_by`, which lowers to
`idx.sort_by(fn(a, b) { … keys[a] … keys[b] … })`. Two costs stack inside the
current prelude merge sort:

1. **Per-merge-level PVec allocation.** `merge_sorted` builds a brand-new vector
   at every level via `append`, so the sort allocates ~`n` elements × `log n`
   levels and reads both inputs through the persistent trie.
2. **Boxed `keys[a]` reads inside the comparator closure.**

This work attacks **(1)** only, which is general to all `sort_by` calls. **(2)**
is explicitly out of scope here (it requires comparator-shape recognition or
typed `Vector<Int>` reads — see [wasm-native-sort.md](wasm-native-sort.md) and
[typed-vector-representation.md](typed-vector-representation.md)). We chose the
"every `sort_by` gets faster" lever deliberately over a dataframe-specific
kernel.

### Staging decision

- **Approach A (this spec):** in-place introsort over a uniquely-owned PVec
  buffer using existing index read/write sugar. **Zero runtime / `arr.tw` /
  stage0 changes.** Kills cost (1); still pays O(log₃₂ n) trie access per
  element (~4 hops at N = 1M).
- **Approach C (follow-up, only if A underperforms):** flat dense scratch-array
  runtime primitives + stable bottom-up merge. Kills trie access too, and
  restores stable-sort semantics. Deferred — not in this spec.

The decision gate (below) determines whether we stop at A or escalate to C.

## Design

All changes are in `boot/prelude/vector.tw`. No signature, builtin, runtime, or
stage0 changes.

### Available primitives (confirmed)

- `Vector.make(len, value) Vector<T>` — allocate a fresh (uniquely-owned) buffer.
- `buf[i]` — index read, traps on OOB, returns `T` (`IndexRead` contract).
- `buf[i] = v` — index write, backed by `set_at` → `vector$set_unsafe`; mutates
  **in place when `buf` is uniquely owned**, traps on OOB.
- existing `reverse`.

Because `make` produces a fresh buffer and we thread it linearly, the static
uniqueness analysis should compile every `buf[i] = v` to an in-place
`set_unsafe` (no copy-on-write). **Verifying this actually happens is the first
implementation step** — if writes COW, each becomes O(n) and the experiment is
invalid; that is itself a signal to expose a dedicated in-place path or jump to C.

### `sort_by<T>(xs, cmp)`

1. **Keep the existing cheap pre-scan unchanged.** Already-ascending → return
   `xs`; strictly-descending → return `xs.reverse()`. This stays outside the
   heavy path and preserves the current `sort idx id` early-out.
2. **Materialize a uniquely-owned working copy `buf`.** `buf := Vector.make(n,
   xs[0])` (safe: `n > 1` here, pre-scan returned for `n <= 1`), then a fill loop
   `buf[i] = xs[i]`.
3. **Sort `buf` in place** with **3-way (Dutch-flag) quicksort + insertion-sort
   cutoff (~16)**:
   - compare via `cmp(buf[i], buf[j])`;
   - swap via index read/write sugar;
   - median-of-three pivot;
   - 3-way partition is deliberate — the dataframe `amount` key is
     low-cardinality (many duplicates), where plain quicksort degrades to O(n²);
     3-way handles duplicate-heavy input well.
4. **Return `buf`.**
5. Remove the now-dead `merge_sorted` and `sort_by_range`.

`sort<T: Ord>` is unchanged (still `xs.sort_by(fn(a, b) { a.compare(b) })`).

### Recursion

Quicksort recursion depth is O(log n) typical but unbounded worst-case (no
heapsort fallback in A). The pre-scan handles sorted/reverse input; 3-way
handles duplicates; median-of-three handles typical adversarial shapes. If large
inputs trap on stack depth, that is a signal to add a heapsort fallback or move
to C. Acceptable risk for the experiment.

## Semantic change: stability

Approach A is **unstable**; today's merge sort is **stable**.

- Existing suites use distinct keys (including the dataframe null-ordering test
  `query_suite.tw`, keys `[3,1,2]` with one null), so they should pass.
- This is a real behavior change for duplicate keys / equal-comparator ties
  (the dataframe comparator returns `Order.Eq` for equal keys and two nulls).
- If A is kept, document that `Vector.sort_by` no longer guarantees stable
  output. A genuine requirement for stability is itself a trigger to escalate to
  Approach C (stable merge).

## Verification & decision gate

Baselines to beat (from `wasm-native-sort.md`, N = 1,000,000):

| microbench (`order_by_micro.tw`) | current |
|---|---|
| `sort values` | ~829 ms |
| `sort idx key` | ~1674 ms |

| dataframe (`main.tw`) | current |
|---|---|
| `order_by` | ~2531 ms |

Commands:

```bash
target/twk run examples/dataframe/bench/order_by_micro.tw
target/twk run examples/dataframe/bench/main.tw
make boot-test            # incl. dataframe query_suite + api_vector_suite
```

Expected: `sort values` and the merge-allocation portion of `sort idx key` drop
materially; `order_by` drops from the ~2.5s range. All suites green (especially
the null-ordering test).

**Decision gate:** if `order_by` does not improve materially, escalate to
Approach C (flat dense scratch + stable merge). If it improves but stability is
required by some consumer, also escalate to C.

### Measured results (gate: FAIL → escalate to C)

In-place writes did **not** materialize. The generated WAT emits `swap`,
`quicksort_range`, and `insertion_sort_range` as separate functions, so the
working buffer crosses call boundaries on every swap and the uniqueness analysis
falls back to the persistent copy-on-write path (`rt_arr__set` + `array.copy`,
two O(log₃₂ n) COW writes per swap). No `set_in_place`/`set_unsafe` writes were
emitted in the sort path. Net effect is roughly O(n·log²n) work — a
constant-factor *regression*, not an O(n²) hang.

| benchmark (N = 1,000,000) | baseline (merge sort) | Approach A (quicksort) |
|---|---|---|
| `sort values` (`order_by_micro.tw`) | ~829 ms | ~3526 ms (~4.3× slower) |
| `sort idx key` (`order_by_micro.tw`) | ~1674 ms | ~4490 ms (~2.7× slower) |
| `order_by` (`main.tw`) | ~2531 ms | ~5589 ms (~2.2× slower) |

Boot suite stayed green (2565 tests) — correctness is fine; the problem is
purely that the COW write path makes the in-place algorithm slower than the
merge sort it replaced. The plan's Step 5 inline-fallback was **not** pursued:
even a fully-inlined single-body sort that achieved in-place writes would still
pay the per-element O(log₃₂ n) trie-access cost that Approach C eliminates, and
A would remain unstable. Approach C (flat dense scratch-array runtime primitives
+ stable bottom-up merge) is the correct next step and is its own plan.

## Out of scope

- Comparator-shape recognition / typed argsort kernels.
- Typed `Vector<Int>` physical representation.
- Flat dense scratch-array runtime primitives (that's Approach C).
- stage0 parity changes (none needed — pure prelude).
