# Typed Vector Representation — Implementation Plan

**Goal:** Make monomorphic numeric/vector-heavy Twinkle code fast without requiring user code to avoid idiomatic `Vector<T>` operations. Today `Vector<Int>` is semantically typed but physically still a generic persistent vector of `anyref` elements. Reading an `Int` element requires a PVec trie lookup, `anyref` load, cast to `BoxedInt`, and `struct.get i64`. In hot loops and sort comparators this boxed egress is multiplied millions of times.

**Thesis:** The compiler should use monomorphization information to choose more precise physical vector representations for common element types, starting with `Vector<Int>`, while preserving the source-level `Vector<T>` abstraction.

**Architecture parent:** [../backend-anyref-elimination.md](../backend-anyref-elimination.md) — this plan delivers the `Vector<Int>` container family of that broader "make `anyref` exceptional" effort; the representation-boundary policy it defines governs how far this routing can safely extend.

**Related plan:** [wasm-native-sort.md](archive/wasm-native-sort.md) attacks the immediate `order_by` hotspot by sorting over dense runtime working sets. This plan is the broader representation fix: make typed vector access faster everywhere so idiomatic numeric collection code has better baseline performance.

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

From [wasm-native-sort.md](archive/wasm-native-sort.md):

```text
N = 1000000
sort values : 828.89ms
sort idx key: 1674.20ms
```

`sort idx key` isolates repeated `keys[a]` / `keys[b]` reads from a `Vector<Int>` key column inside a comparator. This cost includes PVec traversal and boxed `Int` egress. A dense native representation in Rust/Go/Clojure is much faster, showing that the workload itself is not inherently multi-second.

### Cross-runtime calibration: Clojure reference vectors vs dense longs

Read probe: `examples/performance/sort-bench/ref_vector_read_clojure.clj`.

Shape: `N = 1_000_000`, `M = 10_000_000` random reads with the same multiplicative
index pattern used by the Twinkle typed-vector probe.

```text
JVM long[]                    ~9 ms
Clojure persistent Vector<Long> ~167–170 ms
Clojure Vector<String>          ~210–266 ms
Clojure Vector<deftype Row>     ~267–273 ms
Clojure Vector<map row>         ~850–950 ms
```

Sort probe: `examples/performance/sort-bench/long_array_sort_clojure.clj`.

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

This is the near-term bridge used by [wasm-native-sort.md](archive/wasm-native-sort.md).

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

> **Progress (landed on `main`; last re-measured 2026-07-04).** The `Vector<Int>`
> track is well underway; per-phase status is tagged on each header below.
> Landed: typed `PVecI64` family + intra-function routing (S1/S2.0, see
> [typed-vector-spike.md](archive/typed-vector-spike.md)), boxed-boundary adapters for
> return + direct-call args (S2.1), and typed **record fields** (S2.2, see
> [../../archive/typed-record-fields.md](../../archive/typed-record-fields.md)). The
> native value-sort kernel ([native-typed-value-sort.md](archive/native-typed-value-sort.md))
> realizes the Phase-2 dense working set.
>
> **Update (2026-07-05).** Two things happened since. (1) A **uniform-typing**
> attempt (make `Vector<Int>` physically `PVecI64` *everywhere*) was built and
> **reverted** — it made captured-vector reads O(n) per access via the `anyref`
> closure env (post-mortem: [m1a-anyref-readback-investigation.md](archive/m1a-anyref-readback-investigation.md)).
> The corrected model is **storage-site typing**: `TypedVec`/`PVecI64` is a
> per-site optimization, never a global property
> ([../representation-boundary-policy.md](../representation-boundary-policy.md)).
> (2) **Typed variant payloads landed** (Milestone A, branch `typed-vector-repr-m1a`,
> [storage-site-typed-vectors.md](archive/storage-site-typed-vectors.md)) — capture-safe,
> no pathology. **But `order_by` is still unchanged**: the conservative producer
> eligibility doesn't type the *real* dataframe `IntCol` columns.
>
> **⚡ UPDATE (2026-07-08): both of the below LANDED; the `order_by` sort win is
> in.** On branch `typed-vector-crossfn-abi`: cross-fn ABI (B2 accessor returns,
> B3/B4 copy propagation) broadened producer eligibility, and the two typedness
> oracles were unified so captured columns type without invalid Wasm (C2 — the
> [unify-typedness-oracle-design.md](archive/unify-typedness-oracle-design.md) work,
> `fd3da98f`…`5776e82b`). `sort idx by amount` ~1400→~775ms, full `order_by`
> ~2.3s→~1.84s @ 1M. **The current per-boundary status of record is now
> [boundary-tracklist.md](boundary-tracklist.md)** — the per-phase notes below this
> line are pre-C2 history. Remaining headline lever: **B8** (typed `take`).
>
> **Open next, in priority order (pre-2026-07-08 — items 1 & 2 now done):**
> 1. ~~**Broaden producer eligibility**~~ ✅ (cross-fn ABI).
> 2. ~~**M1b — typed closure environments**~~ ✅ (local capture C1 + captured
>    columns C2).
> Typed combinators (Phase 5) remain useful but secondary.

> **Typed-return ABI bridge landed (2026-07-06, branch `typed-vector-crossfn-abi`).**
> `PreparedFunc.phys_return: ValType?` lets a function returning a typed slot
> expose a physical `PVecI64` result ABI (no boundary box); emit routes such a
> function's body through `emit_tail_expr` so returns coerce to the physical type
> (a no-op). `typeable_return` is computed via the unified `slot_typed_after_route`
> predicate (payload/field read, builder candidate, or typed-return call result);
> route_func sets `phys_return`, types typed-return call results (`keys :=
> as_ints(col) → PVecI64`), and `analyze_typed_captures` supports capturing them.
> Self-host fixed point; 2973 tests. **Sound but currently inert on the dataframe:
> `as_ints` returns a `case`-result slot, not the payload binding, and route_func
> does not yet type match/if results.** The remaining piece to activate the
> ~1343ms dataframe sort is **control-flow-result typing**: type a match/if result
> whose arms all return typed slots (linking the arm-result and result slots),
> after which `as_ints` returns typed, `keys` is typed, and M1b's already-landed
> capture routing fires on the comparator.
>
> **Control-flow-result-typing attempt (2026-07-06) — routing works, blocked on an
> emit coercion; reverted to keep the branch green.** A full routing pass was
> written and self-host-clean: `slot_typed_after_route` recognises a match/if
> result whose arms yield typed sources (so `return_is_typed(as_ints)` fires);
> `route_func` retypes the result slot AND the typed arm atoms to `PVecI64`; the
> escape check relaxes result-position atoms. It correctly types `as_ints`'s `v`
> (payload read) and its `case`-result slot. **But it produces invalid wasm:** the
> match/if *arm* store coerces the arm value to the result **mono** (`Vector<Int>`
> → boxed `PVec`) via `emit_expr(arm, result_mono)`, boxing the already-`PVecI64`
> arm value and storing it into the now-`PVecI64` result local (a type mismatch;
> the dataframe run traps). This is the **4th** emit site (after function return,
> gather result, and match arm) that coerces to a mono-derived physical type
> instead of the retyped slot's `wasm_type`. The fix is an **`expected_vt`
> physical-override on the emit tail-coercion path** (`emit_if_op`/`emit_match_op`
> already receive `result_vt`; thread it into the arm's tail
> `emit_atom_for_expected` so a `PVecI64` arm coerces to `PVecI64` — a no-op —
> instead of boxing). Once that lands, re-apply the control-flow-result routing and
> the dataframe sort should drop (as_ints→typed, keys→typed, M1b capture fires).

> **M1b typed closure captures — local-capture increment landed (2026-07-06,
> branch `typed-vector-crossfn-abi`).** A `Vector<Int>` captured into a closure
> and read via index/len only now flows typed through the (trampoline-private)
> anyref env: the trampoline downcasts it `anyref→PVecI64` (O(1), no rebuild —
> the m1a pathology was uniform-typing's `unbox_i64`, avoided here), so a sort
> comparator reads the captured key column via `get_i64`. Gated on a two-part
> soundness check (`analyze_typed_captures`): the capture is read typed-only in
> the lambda AND at every construction site the enclosing free var is a typed
> LOCAL producer (`free_var_typed_local`) — a boxed free var (function return /
> param / combinator result) must not type the capture or the downcast traps.
> Reuses the param-ABI machinery (a capture is a param across the `AMakeClosure`
> edge) plus a `relaxed` escape-guard threaded through the classifiers.
> **Win:** a typed-local key sort @ N=1M ~1276→**748ms** (~41%). Self-host fixed
> point; 2973 tests. **Not yet the dataframe order_by:** its `keys :=
> column.as_ints(amount_col)` is a boxed function return, so the capture stays
> boxed (correctly). Closing that needs the **typed-return bridge** (`as_ints`
> returns `PVecI64`) — the next increment — after which the column reaches the
> comparator typed and the ~1343ms dataframe sort drops.

> **Cross-fn typed-vector ABI — Stage 2 joint fixpoint landed (2026-07-06,
> branch `typed-vector-crossfn-abi`).** The plan's Stage 2 as written could not
> type the real dataframe `ColData.IntCol` (payload typing and param typing are
> mutually circular). Resolved with a co-inductive **greatest fixpoint**
> (`analyze_typed_repr` in typed_param_abi.tw): a typeable param stored into a
> payload is a clean typed producer, so `int_col(values)` → `.IntCol(values)`
> types the payload even with no direct `collect` producer. Fields stay
> builder-only (a field store inserts no coercion; only variant payloads coerce).
> Self-host green, 2971 tests.
>
> **Effect:** the dataframe Int column now gathers typed (`rt_arr__gather_i64`).
> gather_compare @ N=1M: native gather 3 columns ~467→391ms, table.take
> ~471→418ms. order_by breakdown @ N=1M: full order_by ~2403→2304ms; `table.take`
> ~471→418ms. **The sort (1343ms) dominates order_by and is unchanged** — it needs
> the typed closure env (M1b), unblocked by this work but a separate effort. One
> residual cost: `int_col` still `unbox_i64`s its boxed param into the typed
> payload once per column build (build path, not the order_by metric); the pending
> `f$i64` variant emission (plan Tasks 2.3–2.5) removes it by passing typed args.

> **Cross-fn typed-vector ABI — Stage 1 landed + Checkpoint A (2026-07-06,
> branch `typed-vector-crossfn-abi`).** Stage 1 of
> [crossfn-typed-vector-abi-plan.md](archive/crossfn-typed-vector-abi-plan.md) added a
> typed `gather_i64` runtime op (+ `builder_push_i64_raw`) and routes
> `gather(v, idx)` → `gather_i64` when the receiver `v` is already a typed
> `PVecI64` (escape whitelist + gather-result eligibility fixpoint + an emit fix
> so the typed result is not re-boxed). **Proven end-to-end** by
> `examples/performance/sort-bench/typed_gather_probe.tw` (a `collect`→`.IntCol`
> payload gathered → `rt_arr__gather_i64`, no boxed gather, correct result) and a
> routing suite (`boot/tests/suites/route_typed_vec_suite.tw`); 2968 boot tests +
> self-host green.
>
> **Checkpoint A measurement: the dataframe gather did NOT move — as predicted by
> the note above.** Building `examples/performance/dataframe/bench/gather_compare.tw`
> to WAT shows **0 `rt_arr__gather_i64`, 7 boxed `rt_arr__gather`**. Root cause
> (verified in code): the real `ColData.IntCol` payload is never typed, because
> every column is built via `column.int_col(values: Vector<Int>)` (`frame/column.tw:25`)
> — a **boxed param** producer — and `gen.table` feeds it `column.int_col(amounts)`
> (`frame/gen.tw:31`). `analyze_typed_payloads` only marks a payload typed from a
> clean `builder_freeze`→`.IntCol` producer; there is none, so `column.gather`'s
> `v` is not `eligible_v` and the swap never fires. **Stage 1's dataframe win is
> therefore gated on Stage 2** (typed param/return ABI): once `int_col`'s `values`
> param can be a typed `PVecI64` clean producer, the payload types and Stage 1's
> gather routing fires. Stage 1 is correct, self-contained, and a necessary
> prerequisite (`gather_i64` must exist for Stage 2 to route to), but yields no
> standalone dataframe delta. gather_compare @ N=1M (unchanged, boxed): native
> gather amount ~78ms, native gather 3 columns ~468ms, native table.take ~471ms.

### Implementation map (where the landed routing lives)

Routing runs **after** boundary insertion + repr assignment
(`boot/compiler/backend/prepare.tw` calls `route_typed_vectors` last), so the
pass must reproduce how the boxed builder is already represented — that is where
the subtlety is (see the "three fixes" gotchas in
[typed-vector-spike.md](archive/typed-vector-spike.md)).

- `boot/compiler/backend/route_typed_vec.tw` — **the pass.** Per function: find a
  `collect`-built `Vector<Int>` (`v = builder_freeze(b)`), escape-analyze `v`
  (only `xs[i]`/`len` allowed), trace the builder lineage backward through
  `AInit` copies, then swap `builder_new/push/freeze`/`len` → `_i64`, retype
  `v`'s slot to `PVecI64`, and re-erase the builder-lineage slots to
  `OpaqueAnyref`/anyref. Also hosts `analyze_typed_fields` (S2.2): whole-program
  record-field inference — a `Vector<Int>` field is typed only if every producer
  is typed-routable and every consumer reads it via index/len.
- `boot/compiler/codegen/runtime/{types,arr}.tw` — the `PVecI64` family (S1) and
  the `box_i64` boxed-boundary adapter (S2.1); `codegen/emit/coercions.tw` emits
  the `box_i64` coercion.
- `boot/compiler/builtins.tw` — the `_i64` builtins (abi + `rt`, `.None`
  canonical).
- `boot/compiler/codegen/emit/arrays.tw` — `xs[i]` routes to `get_i64` when the
  base wasm type is `PVecI64` (`is_pvec_i64`).
- `boot/compiler/codegen/emit/{runtime_abi,calls}.tw` — the `_i64` builder ops
  skip mono-driven result adaption and get the `anyref→Array` builder-arg cast;
  direct-call args coerce to callee param slot types so `PVecI64` boxes to `PVec`
  at user-function boundaries (S2.1).
- `boot/compiler/backend/verify_slots.tw` — verifier accepts a `PVecI64` wasm
  type for a `Vector<Int>` slot (`is_typed_vec_i64`);
  `backend/verify_expr.tw` rejects a `PVecI64`-value-into-`PVec`-field mismatch
  (S2.2 `pvec_repr_mismatch`).

**Design question resolved.** The pass runs *after* boundary insertion, so it
pays an "erasure-mimicry tax" (typed slots must re-reproduce the boxed builder's
slot erasure). The route-before-vs-after question is settled in
[../representation-boundary-policy.md](../representation-boundary-policy.md):
representation is a pure function of the monomorphized type and is made
first-class, and Milestone 2 moves the decision into `insert_boundaries` and
retires this post-pass. Milestone 1 keeps the coercion as a principled
(repr-diff-driven) post-pass while the family + aggregate-layout work lands.

### Phase 1 — Measure boxed vector read cost directly — ✅ done

Add microbenchmarks that isolate:

- linear `Vector<Int>` sum/index loop;
- repeated random `Vector<Int>` reads;
- same shape after materializing to a dense runtime buffer once, if available;
- sort comparator reads (`order_by_micro.tw` already covers this indirectly).

Record numbers in this plan and `docs/plans/performance/dataframe/friction-log.md` where relevant.

### Phase 2 — Dense i64 working-set helper for sort kernels — ✅ done (native value-sort kernel)

As part of [wasm-native-sort.md](archive/wasm-native-sort.md), implement helpers that materialize `Vector<Int>` into a dense i64 working array inside the runtime sort. This gives immediate value and validates unboxing/fill loops.

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
