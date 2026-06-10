# Typed `Vector<Int>` representation — de-risking spike

**Status:** active (2026-06-10). First concrete step of the
[typed-vector-representation.md](typed-vector-representation.md) master track,
chosen over a full upfront plan to *measure the premise before committing
multi-week effort*. Boot-only (no stage0 parity — routing only changes what
boot emits; stage0 compiles the new boot source as ordinary Twinkle and the
self-host fixed point is between two boot-compiled stages).

## The premise being tested

`xs[i]` on `Vector<Int>` today: bounds-check → `rt_arr__get` (trie walk →
`anyref` leaf element) → `ref.cast BoxedInt` → `struct.get i64`. The boxed leaf
holds an `anyref` pointer to a **separate, scattered `BoxedInt` heap object**, so
each read is a pointer-chase + cast on top of the trie walk. A typed `PVecI64`
walks the **same trie** but its leaves store `i64` inline → one `array.get i64`,
no chase, no cast. The spike measures whether removing that per-read scattered
pointer-chase is worth the typed-storage machinery. (The random boxed read is
~16 ns; the trie walk is shared, so the saving is the leaf-egress delta.)

## Design

- New GC struct `PVecI64` = same shape as `PVec` but `tail: ref ArrayI64`.
- **Reuse** `VecInternal`/`VecChildren` for the trie: children are `ref null eq`,
  which hold `ArrayI64` leaves (all GC arrays are `eq` subtypes). Only the leaves
  and tail are typed; internal nodes are shared.
- `ArrayI64` already exists (from the `sort_i64` kernel).
- Typed ops mirror `rt.arr` with ~5 substitutions (struct type → `PVecI64`, leaf
  array → `ArrayI64`, result/elem `anyref` → `i64`).

## Steps

- [x] **S1 — typed runtime family + direct microbench (the measurement). DONE — gate passed decisively (2026-06-10).**
  Built `PVecI64` (struct in `runtime/types.tw`) + `rt.arr` ops `len_i64`,
  `get_i64`, `promote_full_tail_i64`, `builder_new_i64`/`push_i64`/`freeze_i64`
  + `empty_pvec_i64`/`empty_leaf_i64` globals. The trie-build helpers are
  shared: `new_path`/`push_tail` treat leaves as opaque `ref eq` (widened
  `wrap_leaf`'s param `ref Array` → `ref eq` to match). **Key correction:** the
  RRB `concat_trees` is *not* leaf-agnostic — it casts leaves to boxed `Array`
  to rebalance — so the typed builder uses a **radix** append (`push_tail` +
  manual root-overflow growth), which is correct since builders only produce
  strict vectors. A/B microbench A/B'd via two temp internal builtins
  (`bench_read_i64`/`bench_read_boxed`, identical LCG index sequence → matching
  checksums verify correctness across all trie depths n=33…100000…1M).

  **Result at N=1M, 10M random reads (quiet machine, stable across runs):**

  | path | time | per read |
  |---|---:|---:|
  | boxed `rt.arr` get + `BoxedInt` cast/deref | ~610–628 ms | ~61 ns |
  | typed `PVecI64` `get_i64` (raw i64 leaf) **incl. one-time build** | ~90 ms | ~9 ns |

  **~6.8× faster** — and the typed number *includes* building the `PVecI64`
  from the boxed input once (~1M boxed reads + typed pushes), so the pure typed
  read is even cheaper. Confirms the premise: the scattered `BoxedInt`
  pointer-chase was the dominant per-read cost; the shared trie walk is cheap.
  Typed `Vector<Int>` storage is validated as the master lever. **Proceed to S2.**
- [ ] **S2 — source-level repr routing.** Recognize intra-function
  typed-eligible `Vector<Int>` and route `collect`/literal → typed builder,
  `xs[i]` → `get_i64`, `xs.len()` → `len_i64`; erase `PVecI64 → PVecAnyref` at
  every call boundary (coercion: box each `i64`). Verifier must forbid feeding a
  `PVecI64` slot to a generic anyref-vector helper without the coercion.

  **S2.0 design (first increment — IR rewrite, conservative no-boundary).**
  Lowest blast radius: a post-prepare IR rewrite pass, *no* change to existing
  slots' `ReprKind`. Conservative eligibility = the typed vector never reaches a
  boundary, so **no coercion needed yet** (the hard part is deferred).
  - **Wire typed ops as internal builtins** (`vector$get_i64`/`len_i64`/
    `builder_new_i64`/`push_i64`/`freeze_i64`; abi + `rt`, `.None` canonical, no
    prelude stub) so the rewrite can reference them by FuncId.
  - **Eligibility:** for each `Let(v, ACall(builder_freeze, [b]), body)` where
    `v: Vector<Int>`, classify every use of `v` in `body`: OK iff it is an
    `AIndex(base=v, …, .Array)` read or the arg of `ACall(vector$len, [v])`.
    Any other use (call arg, record field, return, capture, append, sort, …)
    disqualifies `v`. Trace `b`'s lineage (its `builder_new` + all
    `builder_push(b, …)`).
  - **Rewrite (eligible only):** `builder_new`→`builder_new_i64`;
    `builder_push(b, elem)`→`builder_push_i64(b, raw_i64(elem))` (drop the
    element's box/`AWrapAnyref`); `builder_freeze(b)`→`builder_freeze_i64(b)`
    bound to a fresh `PVecI64`-typed slot `v64`; `v`'s index/len uses rewritten
    to `get_i64(v64,i)` / `len_i64(v64)`. `v` becomes dead.
  - Emit needs **no changes** — it already lowers `ACall(GlobalFunc(builtin))`
    to the rt op; only `v64`'s slot carries the `PVecI64` wasm type.
  - **Gate:** a `collect` + random-`xs[i]` + scalar-return bench routes to typed
    and matches the S1 ~7× (S3). Note: conservative eligibility rarely fires on
    real code (dataframe columns cross boundaries) — S2.0 *proves the routing
    mechanism + measures idiomatic source*; broad reach needs the coercion +
    cross-function increments (deferred).

  **S2.0 status: WORKING (2026-06-10).** Idiomatic `collect` + random `xs[i]` +
  scalar return now routes to PVecI64 and gets the full win: at N=1M / 10M reads,
  **boxed ~540–577 ms vs typed ~82 ms (~6.8×)**, matching the S1 internal-op
  number; checksums match the boxed path. Self-host converges with routing
  enabled and all 2571 tests pass; dataframe `order_by` unaffected (its columns
  escape, so they don't route — the conservative no-boundary eligibility holds).

  Getting there from the gated commit took three fixes (the builder is threaded
  through copies, and routing after boundary insertion must reproduce the boxed
  builder's slot erasure):
  1. **Builder lineage across copies.** `builder_new`'s result goes to a temp
     `p2`, then `p3 := p2` (an `AInit` copy), and `p3` is the builder threaded to
     push/freeze. Matching `builder_new` by the freeze-arg slot (`p3`) missed
     `p2`, leaving it boxed → illegal cast. **Fix:** a copy-map
     (`Let(dst, AInit(ASlot(src)))`), traced backward from the freeze arg so
     every builder slot (incl. the `builder_new` result) is in `eligible_b`.
  2. **Builder slot erasure.** The boxed builder slots are `OpaqueAnyref`/anyref
     (so `builder_new` returning `Array` fits and the arg shim casts to `Array`).
     route_func receives the threaded copy as `TypedRef`/`PVec`, so it must
     re-erase the `eligible_b` slots to `OpaqueAnyref`/anyref itself.
  3. **Builder-arg shim for the `_i64` ops.** `is_builder_buffer_arg`
     (`runtime_abi.tw`) must include `builder_push_i64`/`builder_freeze_i64` so
     their arg-0 builder is cast `anyref → Array` (not to the mono-derived PVec).

  The earlier-feared "erased index-result re-boxing" turned out not to hurt — the
  measured ~6.8× shows the read win is fully realized (the optimizer cleans up).
- [x] **S3 — confirm idiomatic path.** Done: the `collect`+`xs[i]` bench above
  matches S1's ~7× at the source level.

## Deferred past the spike (the rest of the track)

Cross-function typed ABIs, typed `gather`/`sort`/`map`/`filter`, contract
awareness (`Stringify`/`Eq`/iteration), `Float`/`Bool`/`Byte` families,
`set`/`slice`/`concat` over typed storage. Only pursue if the spike wins.
