# Typed Vector Representation — Implementation Plan

**Goal:** Make monomorphic numeric/vector-heavy Twinkle code fast without requiring user code to avoid idiomatic `Vector<T>` operations. Today `Vector<Int>` is semantically typed but physically still a generic persistent vector of `anyref` elements. Reading an `Int` element requires a PVec trie lookup, `anyref` load, cast to `BoxedInt`, and `struct.get i64`. In hot loops and sort comparators this boxed egress is multiplied millions of times.

**Thesis:** The compiler should use monomorphization information to choose more precise physical vector representations for common element types, starting with `Vector<Int>`, while preserving the source-level `Vector<T>` abstraction.

**Related plan:** [wasm-native-sort.md](wasm-native-sort.md) attacks the immediate `order_by` hotspot by sorting over dense runtime working sets. This plan is the broader representation fix: make typed vector access faster everywhere so idiomatic numeric collection code has better baseline performance.

---

## Problem statement

The current `Vector<T>` implementation is a generic PVec whose leaves store `anyref`. For primitive values this means boxing at container boundaries:

```text
Vector<Int> read:
  PVec trie lookup
  -> anyref element
  -> ref.cast BoxedInt
  -> struct.get i64
```

This is acceptable for ordinary application use, but expensive in hot numeric paths:

```tw
idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })
```

At `N = 1000000`, comparison sort calls the comparator many times, so the boxed vector read path is repeated tens of millions of times.

The same issue affects:

- numeric sort/map/filter/fold loops;
- matrix/array-style algorithms;
- dataframe key/index workloads;
- any future standard-library numeric algorithms.

---

## Baseline metrics and symptoms

From [wasm-native-sort.md](wasm-native-sort.md):

```text
N = 1000000
sort values : 828.89ms
sort idx key: 1674.20ms
```

`sort idx key` isolates repeated `keys[a]` / `keys[b]` reads from a `Vector<Int>` key column inside a comparator. This cost includes PVec traversal and boxed `Int` egress. A dense native representation in Rust/Go/Clojure is much faster, showing that the workload itself is not inherently multi-second.

### Cross-runtime calibration: Clojure reference vectors vs dense longs

Read probe: `examples/sort-bench/ref_vector_read_clojure.clj`.

Shape: `N = 1_000_000`, `M = 10_000_000` random reads with the same multiplicative
index pattern used by the Twinkle typed-vector probe.

```text
JVM long[]                    ~9 ms
Clojure persistent Vector<Long> ~167–170 ms
Clojure Vector<String>          ~210–266 ms
Clojure Vector<deftype Row>     ~267–273 ms
Clojure Vector<map row>         ~850–950 ms
```

Sort probe: `examples/sort-bench/long_array_sort_clojure.clj`.

Shape: `N = 1_000_000`, same LCG-generated values as the Clojure value-sort
reference. Clojure's ordinary `sort` uses value-style collection semantics by
sorting an object-array copy; primitive `long[]` sorting is Java interop and
mutates, so the probe clones the array before `Arrays/sort` to model
value-preserving use.

```text
long[] clone + Arrays/sort      ~41–42 ms
Clojure persistent Vector sort  ~169–182 ms
```

Takeaways:

- Dense primitive arrays are a separate performance tier; Clojure `long[]` is
  roughly an order of magnitude faster than boxed persistent-vector reads in the
  random-read shape, and roughly 4× faster than persistent-vector sort even when
  cloning first.
- Boxed primitive payloads in a reference vector are very expensive, which
  supports the `Vector<Int> -> PVecI64` direction.
- Reference payload vectors (`String`, nominal/`deftype`-like rows) do not show
  the same cliff as boxed primitives, though map-as-record payloads are much
  slower. This suggests `VectorAnyref` may remain acceptable for reference
  payloads as the default, while primitive monomorphic vectors need typed leaf
  storage and hot kernels may still need dense working sets.

---

## Design direction

Use monomorphization and backend representation analysis to distinguish semantic type from physical representation:

```text
semantic type: Vector<Int>
physical repr: PVecAnyref today
future repr:   PVecI64 or DenseI64 working-set where valid
```

A staged approach avoids needing to solve fully representation-polymorphic generics upfront.

### Representation families

Candidate physical families:

| Semantic type | Physical family | Leaf/storage shape |
|---|---|---|
| `Vector<Int>` | `PVecI64` | i64 elements |
| `Vector<Float>` | `PVecF64` | f64 elements |
| `Vector<Bool>` / `Vector<Byte>` | `PVecI31` or byte-specific | compact scalar |
| `Vector<String>` / records / closures | `PVecAnyref` | existing anyref |
| generic `Vector<T>` where `T` unknown | `PVecAnyref` | existing anyref |

Start with `Vector<Int>` only. It is the measured hotspot and simplest scalar layout.

---

## Two levels of improvement

### Level 1 — Typed dense working sets inside kernels

Runtime kernels can accept the existing generic boxed PVec, then immediately materialize a typed dense buffer:

```text
PVecAnyref<Int> -> dense i64 array -> sort/fold/etc -> output Vector
```

Pros:

- Easier and lower risk.
- No public ABI change for `Vector<Int>`.
- Directly supports the native sort plan.

Cons:

- Still pays one boxed read per input element during materialization.
- Does not improve arbitrary user indexing outside the kernel.

This is the near-term bridge used by [wasm-native-sort.md](wasm-native-sort.md).

### Level 2 — True typed PVec representation

Represent `Vector<Int>` itself with typed leaves so element reads do not cross `anyref`:

```text
PVecI64 read:
  trie lookup
  -> i64 array.get
```

Pros:

- Improves all idiomatic `Vector<Int>` access.
- Reduces boxing pressure.
- Gives better baseline performance before specialized kernels.

Cons:

- Requires backend representation tracking.
- Generic function boundaries may need conversion or specialization.
- Runtime helpers must exist per typed family.
- Equality/stringify/iteration/indexing contracts need typed-family awareness.

---

## Representation-boundary policy

A typed vector can safely remain typed when all uses are monomorphic and representation-known:

```tw
fn sum(xs: Vector<Int>) Int { ... }      // can use PVecI64
fn sort(xs: Vector<Int>) Vector<Int> { ... } // can use PVecI64
```

It may need to erase to generic `PVecAnyref` when crossing a representation-polymorphic boundary:

```tw
fn id<T>(xs: Vector<T>) Vector<T> { xs }
fn stringify<T: Stringify>(xs: Vector<T>) String { xs.to_string() }
```

Possible policies:

1. **Specialize generic functions by representation.**
   Monomorphization produces separate backend instances for `Vector<Int>` vs `Vector<String>`. Preferred long term.

2. **Erase at generic boundaries.**
   Convert `PVecI64` to `PVecAnyref` when passing to code that expects an erased vector. Easier, but can lose performance and allocate.

3. **Use adapter shims.**
   Keep function ABI generic but generate typed helper paths for known operations. Useful as an intermediate approach.

The project already has monomorphization and backend representation metadata; this plan extends those mechanisms rather than adding user-visible syntax.

---

## Runtime work

For `PVecI64`, mirror existing `rt.arr` operations where needed:

- `len`
- `get` / index read
- `set` / index write if needed
- `append` / builder push
- `builder_new`, `builder_push_i64`, `builder_freeze_i64`
- `gather` or typed gather later
- `slice` eventually, if structural sharing remains important

Do not port every generic PVec operation at once. Start with the operations required by benchmarks and tests.

Possible implementation approaches:

1. **Separate typed runtime module/family**
   - e.g. `rt.arr_i64` with `PVecI64`/typed leaf arrays.
   - Clear and fast, but duplicates runtime logic.

2. **Parameterized code generation for runtime helpers**
   - Generate `arr_i64`, `arr_f64`, etc. from a template.
   - Reduces drift, but adds tooling complexity.

3. **Dense vector only for hot kernels first**
   - Avoid full persistent typed PVec initially.
   - Useful stepping stone, but not the final representation fix.

---

## Compiler/backend work

### Representation analysis

Extend backend representation facts so locals/results can distinguish:

```text
VectorAnyref(T)
VectorI64
VectorF64
VectorI31
```

This must flow through:

- literals (`[1, 2, 3]` can become `VectorI64` when context is `Vector<Int>`);
- `collect` over `Int` body;
- function parameters/results after monomorphization;
- record fields containing typed vectors;
- closures capturing typed vectors;
- intrinsic calls (`len`, index, append, sort, gather).

### Intrinsic/prelude dispatch

Route operations by physical representation:

```text
Vector.len(VectorI64)      -> rt.arr_i64.len
Vector.get/index i64       -> rt.arr_i64.get_i64
Vector.append i64          -> rt.arr_i64.push_i64
Vector.sort<Int>           -> typed/native sort kernel
Vector.gather<Int>         -> typed gather when available
```

Generic/unknown representation keeps using existing `rt.arr` anyref helpers.

### Boundary coercions

Add explicit coercion helpers where representation changes are unavoidable:

```text
VectorI64 -> VectorAnyref   // box each i64 into BoxedInt
VectorAnyref -> VectorI64   // unbox/cast each element, trap on mismatch; use only when semantically safe
```

These should be visible in backend IR/planning, not hidden ad hoc in emitters.

---

## Implementation phases

> **Progress (2026-06-11, branch `native-typed-value-sort`).** The `Vector<Int>`
> track is well underway; per-phase status is tagged on each header below.
> Landed: typed `PVecI64` family + intra-function routing (S1/S2.0, see
> [typed-vector-spike.md](typed-vector-spike.md)), boxed-boundary adapters for
> return + direct-call args (S2.1), and typed **record fields** (S2.2, see
> [../archive/typed-record-fields.md](../archive/typed-record-fields.md)). The
> native value-sort kernel ([native-typed-value-sort.md](native-typed-value-sort.md))
> realizes the Phase-2 dense working set. **Open next:** typed combinators
> (Phase 5) and variant-payload routing (a Phase-6 boundary) — the latter is the
> dataframe `order_by` unlock, since columns are `IntCol(Vector<Int>)`.

### Phase 1 — Measure boxed vector read cost directly — ✅ done

Add microbenchmarks that isolate:

- linear `Vector<Int>` sum/index loop;
- repeated random `Vector<Int>` reads;
- same shape after materializing to a dense runtime buffer once, if available;
- sort comparator reads (`order_by_micro.tw` already covers this indirectly).

Record numbers in this plan and `docs/plans/dataframe-friction-log.md` where relevant.

### Phase 2 — Dense i64 working-set helper for sort kernels — ✅ done (native value-sort kernel)

As part of [wasm-native-sort.md](wasm-native-sort.md), implement helpers that materialize `Vector<Int>` into a dense i64 working array inside the runtime sort. This gives immediate value and validates unboxing/fill loops.

### Phase 3 — Backend representation enum for typed vectors — ✅ done (S2.0 repr tags + S2.2 verifier check)

Introduce backend representation tags for typed vectors, initially behind a conservative gate:

- only `Vector<Int>`;
- only within a single function after monomorphization;
- erase at uncertain boundaries.

Add verifier checks so typed vector locals cannot be consumed by generic anyref-vector helpers without an explicit coercion.

### Phase 4 — Typed `Vector<Int>` literals, collect, index, and len — ✅ done (S2.0)

Make the smallest useful `Vector<Int>` path typed:

- annotated/lowered literals;
- `collect` with `Int` body;
- `.len()`;
- `xs[i]` index read;
- simple iteration if needed for benchmarks.

Run existing vector/API tests plus new numeric read microbenchmarks.

### Phase 5 — Typed append/builder and common combinators — ◐ partial (typed builder done; combinators open)

Add typed builder support so loops building `Vector<Int>` do not box each append. Route `collect` to the typed builder where possible.

Then consider:

- `Vector.gather<Int>`;
- `Vector.sort<Int>`;
- `Vector.map`/`filter` specializations if optimizer can recognize them.

### Phase 6 — Cross-function monomorphic typed vectors — ◐ partial (boundary-by-boundary)

Let monomorphized function ABIs use typed vector representations when all call sites agree or when the monomorphized instance is representation-specific.

This is where the feature becomes broadly useful, rather than a local optimization. Landed so far as conservative per-boundary steps rather than full typed ABIs: S2.1 (return + direct-call argument boxing adapters) and S2.2 (typed record fields via whole-program field inference). Still open: variant payloads (the dataframe-column boundary), closures/closure-call boundaries, builtin/vector combinators, and genuinely representation-specialized cross-function ABIs.

### Phase 7 — Extend to Float/Bool/Byte if Int succeeds — ◐ partial (Float value-sort kernel only; typed Float/Bool/Byte vectors open)

Add typed families only when motivated by benchmarks and use cases.

---

## Success criteria

Language-level metrics:

- direct random `Vector<Int>` read microbench improves substantially;
- `Vector<Int>.sort()` improves beyond the current ~829ms at `N = 1000000`;
- `idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })` improves when paired with sort-shape lowering;
- no semantic changes to source-level `Vector<T>`.

Dataframe metrics:

- `order_by` improves without requiring dataframe users to call a special API;
- `filter`, `join`, and `group_by` do not regress;
- null ordering behavior remains covered by tests.

Engineering guardrails:

- typed and erased vector representations are explicit in backend facts;
- verifier catches representation mismatches;
- boot and stage0 remain in parity;
- runtime helper duplication is contained or generated.

---

## Risks and open questions

- **Runtime duplication:** typed PVec families may duplicate a lot of `rt.arr` logic.
- **Boundary churn:** erasing/retyping vectors at generic boundaries can erase gains if too frequent.
- **Wasm GC typed arrays:** confirm the exact array/value representation constraints for i64/f64 arrays in the current emitter/runtime type model.
- **Code size:** monomorphized typed helpers may grow Wasm output.
- **Optimization interaction:** static uniqueness and builder rewrites must understand typed builders.
- **Testing matrix:** every typed family multiplies vector operation coverage.

---

## Relationship to Wasm-native sort

The native-sort plan is the near-term path to make `order_by` fast by using dense typed working sets inside sort kernels. This plan is the broader representation track that makes `Vector<Int>` access cheaper even outside sort kernels. They should proceed together:

1. native sort proves the dense typed working-set performance model;
2. typed vector representation reduces the cost of getting into those working sets and improves idiomatic numeric code generally;
3. optimizer/lowering connects idiomatic source to the fast representation without user-visible escape hatches.
