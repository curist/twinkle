# Transparent Native Key-Index Argsort for Dataframe `order_by`

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** optional transparent fast path, not the primary `order_by` performance plan. The main plan is [generic-sort-by-vector-read-perf.md](generic-sort-by-vector-read-perf.md), which targets generic callback sorting and indexed vector reads so idiomatic side-effecting comparators remain competitive. This plan applies the native dense typed working-set model only to conservatively recognized pure key-index `sort_by` shapes.

**Goal:** Make idiomatic Twinkle dataframe code fast without asking users or dataframe authors to call a performance-specialized `argsort` API. The source should remain normal comparator-based code:

```tw
idx.sort_by(fn(a, b) {
  // compare null rank, then keys[a] vs keys[b]
})
```

The compiler recognizes a conservative, side-effect-free key-index comparator shape and lowers that call to an internal native argsort kernel. Non-matching `sort_by` closures keep the generic comparator path exactly as today.

**Non-goal:** Expose `Vector.argsort*` as a public/ergonomic API just to recover performance. Other languages get good performance from their normal idioms; Twinkle should too. Any native argsort runtime functions introduced by this plan are compiler-internal implementation details.

**Primary benchmark:** `target/twk run examples/dataframe/bench/main.tw` and `target/twk run examples/dataframe/bench/order_by_breakdown.tw` at N = 1,000,000.

Current same-machine signal:

| component | current |
|---|---:|
| full dataframe `order_by` | ~2.5–2.7s |
| null-aware index sort | ~2.1–2.2s |
| `table.take(sorted)` gather | ~0.4s |
| Clojure persistent-vector reference | ~0.38s total |

The sort phase dominates. The value-sort kernels do not affect this path because dataframe `order_by` sorts row ids with an opaque comparator that repeatedly reads `keys[a]`, `keys[b]`, `nulls[a]`, and `nulls[b]` from persistent vectors.

## Scope

**In scope:**

- Transparent compiler lowering for recognized `idx.sort_by(fn(a, b) { ... })` key-index comparators.
- Native argsort kernels for primitive keys: `Int`, `Float`, and `Bool`.
- Null-aware ordering matching current dataframe semantics:
  - Ascending: nulls sort last.
  - Descending: nulls sort first.
  - Null/null compares equal and preserves row-id order without inspecting keys.
  - Equal non-null keys preserve row-id order.
- Boot compiler implementation first, then Rust stage0 parity.
- Dataframe source remains idiomatic; `examples/dataframe/frame/table.tw::sort_indices_by_column` should not call a public `argsort` helper.
- Benchmarks and docs recording before/after `order_by` metrics.

**Out of scope:**

- Public `Vector.argsort` / `Vector.argsort_nulls` API design.
- `String` key argsort. Keep the current `String` comparator path until a separate string-key strategy exists.
- Arbitrary `Vector.sort_by(closure)` optimization. Comparators with side effects, traps, nonstandard ordering, or unrecognized structure must keep the generic path.
- `table.take(sorted)` / gather optimization. The gather phase is visible but smaller; tackle it after the sort phase moves.
- Counting/radix sort. The first kernel should be a general stable comparison merge over dense buffers; range-aware strategies can be added after the path is proven.

## Internal runtime shape, not public API

The compiler may introduce internal builtins such as:

```text
vector$argsort_i64_by_idx(idx: Vector<Int>, keys: Vector<Int>, descending: Bool) -> Vector<Int>
vector$argsort_i64_nulls_by_idx(idx: Vector<Int>, keys: Vector<Int>, nulls: Vector<Bool>, descending: Bool) -> Vector<Int>
vector$argsort_f64_nulls_by_idx(...)
vector$argsort_bool_nulls_by_idx(...)
```

These are not user-facing methods. Wire them like internal compiler targets (`rt(..., .None)`) and route only through the compiler recognizer. If tests need to exercise them, do it through idiomatic `sort_by` source programs and verify the emitted WAT contains the internal runtime symbol.

Why accept `idx`? Because the source idiom is `idx.sort_by(...)`; preserving an arbitrary input index vector avoids proving the receiver is exactly `0..n`. The current dataframe `order_by` does use the full range, and a later range-specialized helper can remove that small index-build cost, but it is not the main bottleneck.

## Recognized comparator shapes

Start narrow and explicit. Only optimize closure literals whose resolved body matches one of these shapes with no extra statements or calls beyond the compare/index operations.

### Simple key-index compare

Ascending:

```tw
idx.sort_by(fn(a, b) { Int.compare(keys[a], keys[b]) })
idx.sort_by(fn(a, b) { Float.compare(keys[a], keys[b]) })
idx.sort_by(fn(a, b) { Int.compare(bool_rank_for_sort(keys[a]), bool_rank_for_sort(keys[b])) })
```

Descending:

```tw
idx.sort_by(fn(a, b) { Int.compare(keys[b], keys[a]) })
idx.sort_by(fn(a, b) { Float.compare(keys[b], keys[a]) })
```

### Dataframe null-aware compare

Match the current `examples/dataframe/frame/table.tw::sort_indices_by_column` primitive arms:

```tw
ai := col.nulls[a]
bi := col.nulls[b]

if ai and bi { return Order.Eq }
if ai { return Order.Gt }  // Asc; Desc swaps Lt/Gt
if bi { return Order.Lt }

Int.compare(keys[a], keys[b])
```

and the corresponding descending form using `Int.compare(keys[b], keys[a])` and swapped null ordering. Float and Bool variants mirror the same structure.

This is intentionally conservative. For example, this must **not** optimize:

```tw
idx.sort_by(fn(a, b) {
  println("${a}, ${b}")
  Int.compare(keys[a], keys[b])
})
```

because the comparator has observable side effects and the native sort would change call count/order.

## Architecture

Reuse the dense typed runtime storage proven by native value sort:

```text
idx/keys/nulls persistent vectors
  -> dense row-id buffer: ArrayI64, materialized from idx
  -> dense key buffer: ArrayI64 / ArrayF64 / ArrayI64 bool-rank
  -> dense null-rank buffer when null-aware
  -> stable merge row ids by (null rank, key), with special null/null tie behavior
  -> freeze row ids back to Vector<Int>
```

Comparison rules:

- Null rank is compared first when present.
- For Asc: non-null rank < null rank.
- For Desc: null rank < non-null rank.
- If both rows are null, take the left row id immediately. This matches the current comparator, which returns `Order.Eq` for null/null and does **not** inspect keys.
- If both rows are non-null:
  - Asc takes left when `key[a] <= key[b]`.
  - Desc takes left when `key[b] <= key[a]`.
- Equal non-null keys take the left row id to preserve stability.
- Float must match `Float.compare`: NaN compares greater than every non-NaN, and NaN/NaN compares equal. Do not use raw `f64.le` alone for Float argsort; add explicit NaN branches before numeric comparison.

## Task 1: Internal `Int` argsort kernel

**Files:**

- Modify: `boot/compiler/builtins.tw`
- Modify: `boot/compiler/codegen/runtime/arr.tw`
- Modify: `boot/tests/suites/api_vector_suite.tw` or add a focused sort lowering suite

- [ ] Register internal ABI + runtime mappings for `vector$argsort_i64_by_idx` and `vector$argsort_i64_nulls_by_idx` with canonical `.None`.
- [ ] Implement stable dense kernels over `ArrayI64` row ids and `ArrayI64` keys. ABI shapes:
  - no-null: `[pvec_n(), pvec_n(), i32] -> [pvec_()]` (idx, keys, descending)
  - null-aware: `[pvec_n(), pvec_n(), pvec_n(), i32] -> [pvec_()]` (idx, keys, nulls, descending)
- [ ] Validate inputs consistently with current behavior: index OOB traps through vector reads; length mismatches may trap rather than return recoverable errors.
- [ ] Add direct behavior tests through source-level `sort_by` once Task 2 routing exists; until then, keep a small temporary probe if needed and remove it before commit.

## Task 2: Compiler recognizer and routing for `Int`

**Files:**

- Modify: `boot/compiler/monomorphize.tw` or the earliest resolved-IR pass that still has enough closure body structure
- Modify: tests / WAT probes for routing evidence

- [ ] Detect calls to prelude `Vector.sort_by` where the comparator is a closure literal or closure function generated from a literal.
- [ ] Recognize the simple `Int.compare(keys[a], keys[b])` and descending `Int.compare(keys[b], keys[a])` shapes.
- [ ] Recognize the dataframe null-aware `Int` shape from `table.tw`.
- [ ] Rewrite matching calls to internal argsort builtins; all other `sort_by` calls fall through unchanged.
- [ ] Add a routing-proof program that includes both a recognized comparator and a side-effecting comparator; verify WAT contains the internal argsort only for the recognized one.

Acceptance:

```bash
make bundle-cli
make boot-test
target/twk run examples/dataframe/main.tw
target/twk build /tmp/argsort_probe/main.tw -o /tmp/argsort_probe/out.wat
grep "argsort_i64" /tmp/argsort_probe/out.wat
```

## Task 3: Dataframe benchmark integration without source-level specialized API

**Files:**

- Keep: `examples/dataframe/frame/table.tw` comparator source idiomatic unless small refactors are needed to make the shape recognizable.
- Modify: `examples/dataframe/bench/order_by_breakdown.tw`

- [ ] If needed, make minimal no-semantics-change cleanup to `sort_indices_by_column` so its `Int` comparator matches the recognizer exactly.
- [ ] Update `order_by_breakdown.tw`: its current `sort idx + nulls` probe manually calls `idx.sort_by(...)`, so ensure it uses the same recognized shape or add a labelled native-recognized probe.
- [ ] Measure:

```bash
target/twk run examples/dataframe/bench/order_by_breakdown.tw
target/twk run examples/dataframe/bench/main.tw
```

Expected: the recognized null-aware sort phase should drop materially from ~2.1–2.2s at N = 1M. If it does not, inspect whether persistent-vector reads or closure calls remain in the hot merge loop.

## Task 4: Float and Bool recognition + kernels

**Files:**

- Modify: `boot/compiler/builtins.tw`
- Modify: `boot/compiler/codegen/runtime/arr.tw`
- Modify: compiler recognizer pass
- Modify: vector/dataframe tests

- [ ] Add `f64` no-null and null-aware kernels using dense `ArrayF64` keys and explicit NaN handling matching `Float.compare`.
- [ ] Add Bool kernels using dense integer ranks for Bool keys.
- [ ] Recognize the Float and Bool dataframe comparator shapes.
- [ ] Keep String on the generic comparator fallback.
- [ ] Test ascending/descending/null behavior for all primitive columns, including Float NaN placement.

## Task 5: Stage0 parity

**Files:**

- Modify: `src/runtime/arr.rs`
- Modify: `src/runtime/types.rs` if new runtime array helpers are needed
- Modify: `src/codegen/prelude.rs`
- Modify: `src/intrinsics/registry.rs`
- Modify: `src/intrinsics/signatures.rs` only if internal runtime names require contracts
- Modify: `src/ir/lower.rs`
- Modify: `src/ir/monomorphize.rs` or the matching recognizer pass

- [ ] Mirror the boot kernels in Rust stage0.
- [ ] Mirror the comparator-shape recognizer/routing.
- [ ] Verify stage0 can run the dataframe benchmark and boot compiler still self-hosts:

```bash
cargo build --release
cargo run --release -- run examples/dataframe/bench/main.tw
make bundle-cli
```

## Task 6: Benchmark gate and documentation

**Files:**

- Modify: `docs/plans/wasm-native-sort.md`
- Modify: `docs/plans/dataframe-friction-log.md`
- Optionally modify/add dataframe bench helpers

- [ ] Record before/after numbers for `order_by_breakdown.tw` and `bench/main.tw`.
- [ ] Compare against the Clojure persistent-vector reference:

```bash
clojure examples/dataframe/bench/order_by_clojure_persistent.clj
```

- [ ] Update `wasm-native-sort.md` with the result and the next bottleneck.
- [ ] If sort falls near or below gather, open a follow-up plan for trie-aware bulk gather / `table.take`.

## Success criteria

- Dataframe `order_by("amount", Dir.Asc)` at N = 1M improves materially from the current ~2.5–2.7s range with no public argsort call in dataframe source.
- `sort idx + nulls` no longer dominates at ~2.1–2.2s; the next bottleneck should be `table.take(sorted)` or remaining boxing at kernel boundaries.
- Current query tests continue to pass, including null ordering and descending order.
- Side-effecting or otherwise unrecognized `sort_by` comparators continue to execute through the generic comparator path.
- `String` columns continue to work via the existing comparator fallback.
- Boot and stage0 stay in parity; `make bundle-cli` reaches a fixed point.

## Notes and open questions

- The recognizer is the highest-risk part of this plan. If matching after closure conversion loses too much source structure, move the pass earlier, after resolution/typechecking but before closure lowering.
- The first implementation should optimize only exact, tested shapes. Broaden later based on real code, not speculation.
- A range-specialized helper that generates row ids internally can remove the current `idx := collect i in range(t.nrows) { i }` cost, but that is small relative to comparator sorting and should not complicate the first pass.
