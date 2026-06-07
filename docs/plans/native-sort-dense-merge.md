# Native dense-buffer stable merge sort — Design Spec (Approach C)

**Status:** approved design, ready for implementation plan.
**Parent plan:** [wasm-native-sort.md](wasm-native-sort.md) — this realizes that plan's
Phase 2/3 "runtime-native sort over a dense mutable working set" direction.
**Supersedes:** [native-sort-by-inplace.md](native-sort-by-inplace.md) (Approach A),
which was measured insufficient — its `buf[i] = v` writes fell to the persistent
copy-on-write path across `swap`/recursion call boundaries, regressing `order_by`
(N=1M) ~2531ms → ~5589ms. That prelude change has been reverted; the stable merge
sort is the current baseline again.

## Goal

Make every idiomatic `Vector.sort_by` (and therefore `Vector.sort`, and the
dataframe `order_by` comparator path) faster by sorting over a **flat, mutable,
dense Wasm-GC array** instead of allocating a fresh persistent vector at every
merge level and reading through the persistent trie on every access. Keep the
generic closure comparator — this is a general algorithm improvement that every
`sort_by` benefits from, not a comparator-shape or dataframe-specific kernel.

## Why this works where Approach A didn't

The current prelude merge sort pays three costs per `sort_by`:

1. **Per-merge-level PVec allocation** — `merge_sorted` builds a brand-new vector
   at every level.
2. **Trie reads** — every element access is an O(log₃₂ n) trie traversal.
3. **A closure call per comparison.**

Approach A tried to kill (1) and (2) by sorting in place over a uniquely-owned
PVec buffer, but the uniqueness analysis did not keep `buf[i] = v` in place
across the `swap`/recursion call boundaries, so every write became a persistent
copy-on-write — a net regression.

Approach C kills (1) and (2) **definitively** by doing the sort over a flat
`array (mut eqref)` whose element writes are genuine `array.set` instructions
(O(1), no trie, no copy-on-write). The buffer lives *outside* the PVec value
model, so the uniqueness analysis is never involved. Cost (3), the closure call,
is retained deliberately so the win is general; eliminating it is a separate,
later effort (see Out of scope).

## Design

### Data flow

`Vector.sort_by` keeps its current outer shape; only the heavy path changes:

```
sort_by(xs, cmp):
  # pre-scan — UNCHANGED, kept outside the heavy path
  n = xs.len()
  if n <= 1: return xs
  detect ascending / strictly_descending in one pass
  if ascending:          return xs
  if strictly_descending: return xs.reverse()

  # heavy path — dense merge
  src = scratch_from_vector(xs)             # PVec -> flat array (one pass)
  aux = scratch_new(n)                      # merge auxiliary, same length
  src = merge_sort_dense(src, aux, n, cmp)  # stable bottom-up, ping-pong
  scratch_freeze(src)                       # flat array -> PVec (builder)
```

The pre-scan is retained verbatim: it preserves the cheap already-ordered /
strictly-descending early-outs (e.g. the dataframe `sort idx id` fast path) and
lets the dense kernel assume a genuinely unsorted working set.

### Runtime primitives (new `rt.arr` ops)

Five small intrinsics, each a handful of Wasm instructions, operating over the
**existing** `rt_types__Array` GC type (a `array (mut eqref)`, already used as the
PVec tail) — so **no new GC type** is added:

| op | signature | implementation |
|---|---|---|
| `scratch_from_vector` | `(vec) → Scratch<T>` | `array.new` of `len`, copy elements in |
| `scratch_new` | `(len) → Scratch<T>` | `array.new` null/zero-filled (the aux buffer) |
| `scratch_get` | `(buf, i) → T` | `array.get` |
| `scratch_set` | `(buf, i, v)` | `array.set` (mutating; no COW) |
| `scratch_freeze` | `(buf) → Vector<T>` | `builder_new` → push each → `builder_freeze` |

`scratch_len` is intentionally omitted — the Twinkle merge already carries `n`.

Two optimizations are deferred to follow-ups, not part of the first cut:
- **Leaf-walk copy** in `scratch_from_vector` (iterate PVec leaves rather than
  per-index `get`) to make the input copy a clean O(n) instead of O(n log₃₂ n).
- **Bulk-leaf freeze** (assemble PVec leaves directly from array chunks) instead
  of the element-by-element builder loop.
Both are correctness-neutral speedups; the first cut uses the simple forms.

### Type-system surface

A new **internal, non-exported** opaque builtin type `Scratch<T>`:
- phantom-generic — `T` is erased; the runtime stores boxed `eqref` exactly as
  PVec does, so `scratch_get`/`scratch_set` are generic and zero-cost over `T`;
- maps to the existing `rt_types__Array` GC type (no new GC type);
- prelude-only — it is never exported, and users never see or name it. It exists
  solely so the prelude merge sort and the intrinsic signatures typecheck.

### The stable bottom-up merge (Twinkle, in `boot/prelude/vector.tw`)

Iterative (no recursion → no depth concerns), ping-ponging `src`↔`aux` by
rebinding the two handles each pass:

```
width = 1
for width < n {
  lo = 0
  for lo < n {
    mid = min(lo + width, n)
    hi  = min(lo + 2 * width, n)
    # merge src[lo, mid) and src[mid, hi) into aux[lo, hi), stably
    ...
    lo = lo + 2 * width
  }
  tmp = src;  src = aux;  aux = tmp     # ping-pong
  width = width * 2
}
return src                              # buffer holding the final sorted result
```

**Stable-merge rule:** when both runs have elements, take the **right** element
only when `cmp(left, right) == .Gt`; otherwise take the left element. Equal keys
(`.Eq`) therefore keep their input order. Written with the existing Twinkle
idioms — no `break`, no `+=`: boolean guards and explicit `x = x + 1` increments.

`Vector.sort<T: Ord>` is unchanged (`xs.sort_by(fn(a, b) { a.compare(b) })`) and
inherits the new path automatically.

## Semantics: stability restored

Approach C is a **stable** sort, restoring the guarantee the original merge sort
had and that Approach A gave up. Concretely:

- The `sort_by` doc comment documents stable order again.
- The dataframe `order_by` null-ordering test (`query_suite.tw`) and any
  duplicate-key ordering keep working.
- New tests assert stability directly (see Testing).

This removes the "unstable sort" caveat entirely; there is no behavior
regression for duplicate keys or equal-comparator ties.

## Verification & success gate

Baselines to beat (N = 1,000,000, current stable merge sort):

| benchmark | current |
|---|---|
| `sort values` (`order_by_micro.tw`) | ~829 ms |
| `sort idx key` (`order_by_micro.tw`) | ~1674 ms |
| `order_by` (`main.tw`) | ~2531 ms |

Commands:

```bash
target/twk run examples/dataframe/bench/order_by_micro.tw
target/twk run examples/dataframe/bench/order_by_breakdown.tw
target/twk run examples/dataframe/bench/main.tw
make bundle-cli      # self-host fixed point — also the codegen correctness gate
make boot-test       # incl. dataframe query_suite + api_vector_suite
```

**Success gate:** `sort values` (N=1M) drops materially from ~829ms and
`order_by` (main, N=1M) drops materially from ~2531ms, with all suites green and
`make bundle-cli` reaching its fixed point. `filter`/`join`/`group_by` must not
regress materially.

**If the closure-call cost dominates** and the dataframe `order_by` target is
still not met after the dense path lands, the next step is a typed Int-key
argsort kernel + comparator-shape recognition (parent plan Phase 4) — a separate
plan, not this one.

## Cross-compiler scope

Touches **both compilers** (boot primary, stage0 the correctness mirror), per the
runtime-builtin wiring discipline. Expected touch points:

- **Runtime ops:** `boot/compiler/codegen/runtime/arr.tw` ↔ `src/runtime/arr.rs`
  (the five `scratch_*` ops, same instruction sequence in both host syntaxes).
- **Builtin registration:** `boot/compiler/builtins.tw` (`builtin_specs` + ABI
  arms) ↔ stage0 `src/ir/lower.rs`, `src/intrinsics/registry.rs`,
  `src/intrinsics/signatures.rs`, `src/codegen/prelude.rs`,
  `src/types/env.rs` (incl. the easy-to-miss method-resolution entry).
- **`Scratch<T>` type registration:** boot type env + stage0 type env (new
  builtin TypeId; reuses the existing `rt_types__Array` GC type — no new GC
  type in `types.tw`/`types.rs`).
- **Prelude surface:** signature stubs in `boot/prelude/signatures/vector.tw`;
  the merge sort body in `boot/prelude/vector.tw`.
- **Tests:** `boot/tests/suites/api_vector_suite.tw` (existing robustness tests
  from the Approach-A branch, plus new stability tests).

FuncIds are internal per-compiler and reconcile by canonical name — append new
ops, never renumber; pick free 1000+ ids in stage0.

## Testing

- **Keep** the existing robustness tests: large, duplicate-heavy, adversarial,
  already-sorted/reverse, all-equal, and input-immutability.
- **Add stability tests:** sort items by a key that produces ties (e.g. records
  `{key, seq}` sorted by `key`) and assert the original relative order of equal
  keys is preserved.
- **Dataframe:** `query_suite.tw` null-ordering test stays green.
- **Perf:** the three benchmark harnesses above as the gate.

## Out of scope

- Comparator-shape recognition / typed Int-key argsort kernels (parent plan
  Phase 4) — the deliberate next step *if* closure-call cost dominates.
- Typed `Vector<Int>` physical representation
  ([typed-vector-representation.md](typed-vector-representation.md)).
- Leaf-walk copy and bulk-leaf freeze (correctness-neutral speedups, deferred).
- Any change to the public `Vector` / `sort` / `sort_by` / dataframe API.
