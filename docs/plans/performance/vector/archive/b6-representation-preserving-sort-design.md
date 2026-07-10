# B6 — Typed `sort_by` kernel (typed idx through the merge)

> ⚠️ **Outcome: the kernel was built and then REVERTED (2026-07-10, `bec1bd5c`)** as
> a compiler-special-cased point solution. This design doc's *validation findings*
> (below) are the durable result and motivate the typed-representation "Extend"
> path; the kernel itself is not the chosen direction. See
> [boundary-tracklist.md](../boundary-tracklist.md) "Findings".

**Status:** design (2026-07-10), kernel reverted. The validation that typed
*parameter* ABI for named functions does not exist (only returns/captures) is the
key durable finding; "Extend" (general typed-parameter ABI) is the real path.

**Where this sits:** the headline lever for the dataframe `order_by` sort, re-scoped
from the tracklist's stale "B8 typed take." See
[boundary-tracklist.md](../boundary-tracklist.md) (B6) and
[generic-sort-by-vector-read-perf.md](../generic-sort-by-vector-read-perf.md).

## Goal

Sort a `Vector<Int>` index vector with a custom comparator while reading the index
and the merge's intermediate buffers as `PVecI64` (typed `get_i64`) instead of
boxed `PVec` (`get`). Achieve this with a purpose-built typed merge-sort kernel
recognized at lowering, rather than a general typed-parameter ABI.

## The win (spike-measured, N = 1M)

The merge floor is almost entirely boxed vector reads, not mechanics:

| Layer (from `b6_size_probe`) | Time |
|---|---|
| R: reads + compare + recursion (boxed idx reads) | ~511ms |
| N: compare + recursion, **no reads** | ~17ms |
| → boxed idx reads alone (R − N) | **~494ms** |
| seq typed vs seq boxed reads (20M) | 102ms vs 262ms → ratio ~0.39 |

Cross-check against the live benchmark: `sort idx by amount` = 745ms ≈ 494ms boxed
idx reads + 254ms already-typed `amounts` reads (C2) + 17ms mechanics.

Typing the idx reads (×0.39) saves **~200–300ms** on the sort:
`sort idx by amount` ~745 → ~450–545ms; full `order_by` ~1.2s → ~0.9–1.0s.

> Note: the old README/tracklist decomposition ("the floor is closure calls, Order
> allocation, recursion, append mechanics") is stale — the hoisted-global `Order`
> and ~1ns closure calls already crushed those to ~17ms. The floor is **reads.**

## Root cause & validation (why the kernel, not the general fix)

`sort_by__Int`, `sort_by_range__Int`, `merge_sorted__Int` all take/return boxed
`PVec`; every merge read is boxed. Two layers of blocker, both confirmed by spike:

1. **Producer gap.** `merge_sorted` builds its output with a manual `.append`
   accumulator (it interleaves two sorted sources under control flow — it cannot be
   a `collect`). The `loop_builder` optimizer rewrites that to a builder seeded with
   **`builder_from(base)`**, but `route_typed_vec` only knows
   `builder_new`/`push`/`freeze` (the `collect` path) — it is unaware of
   `builder_from`, so the accumulator never types. (Repro: `collect` read
   typed-only → `get_i64`; manual `.append` read typed-only → boxed `get`.)

2. **No typed-parameter ABI for named functions (the deciding factor).** Even if
   the producer typed, the merge reads through *parameters* (`merge_sorted`'s
   `a[i]`/`b[j]`, `sort_by_range`'s `xs[lo]`). A WAT audit of the whole `order_by`
   build shows:

   | ABI kind | Named user functions | Evidence |
   |---|---|---|
   | Typed **return** | ✅ exists | `as_ints` → `R: PVecI64`, `no_nulls` → `R: PVecBool` |
   | Typed **capture** (closures) | ✅ exists | `__lambda`/`typed_tramp_*` typed env params (C1/C2) |
   | Typed **parameter** | ❌ **does not exist anywhere** | every named func has `P: box` on every param |

   `typed_param_abi`'s `typeable_params` is analyzed but consumed only to decide
   whether a param *feeds a typed payload* — it never emits a typed-signature
   function variant. So a typed vector re-boxes crossing into any named function,
   and the merge's parameter reads stay boxed regardless of typed returns.

Capturing the win therefore requires typed parameters through the recursive merge.
Doing that *generally* means building a specialize-by-representation subsystem
(typed-param function variants + call-site coercion) — the historically riskiest
area. A dedicated kernel gets typed parameters **by construction** and avoids all
of it.

## Primary approach — dedicated typed `sort_by` kernel

Recognize the monomorphic `sort_by` instance for a primitive element and emit a
self-contained typed merge sort. **No new runtime helpers** — every typed op
already exists (`builder_new_i64`/`push_i64`/`freeze_i64`, `get_i64`, `len_i64`,
`box_i64`/`unbox_i64`; and the `_bool`/`_f64` families where present). This is a
pure lowering/codegen change.

### Recognition

At lowering, detect a call to the `sort_by` prelude instance whose element type is
a typed-vector family (`Int` first; `Bool`/`Float` where their families exist).
Route it to the typed kernel instead of the generic `sort_by__T` monomorph.

### The kernel (typed merge sort over `PVecI64`)

- Input: the index vector as `PVecI64`. If the argument is a typed local (e.g. the
  dataframe `idx` built by `collect`), pass it typed directly; if boxed, `unbox`
  once at kernel entry (one-time, negligible vs the sort).
- Reads: `get_i64` / `len_i64` throughout (the pre-scan, leaves, and the merge).
- Intermediates: build each merge output with a fresh `builder_new_i64` +
  `push_i64` + `freeze_i64` (the merge output is not seeded from a base, so
  `builder_from` is not needed). Singleton leaf `[xs[lo]]` becomes a one-element
  typed build.
- Comparator: the closure is invoked with **unboxed** element values (`i64`). The
  elements are already unboxed primitives; only the container changes. The
  comparator itself may capture a typed column (C2, already landed) and reads it
  typed independently.
- `strictly_descending` / `ascending` fast paths: return the input (typed) or a
  typed reverse.
- Output: a `PVecI64`, `box`ed once at the boundary to hand back as `Vector<Int>`.

### Element families

`Int` (`PVecI64`) is the target and the dataframe case. `Bool` (`PVecBool`) and
`Float` (`PVecF64`) get the same kernel shape *iff* their runtime families are
present; otherwise the generic `sort_by` remains for those element types (correct,
just boxed). No new families are introduced by this work.

## Future direction — general typed-parameter ABI ("Extend")

The kernel is sort-specific. The general win (any recursive/helper function over
`Vector<Int>` — `map`/`filter`/`take`/a user's own merge) needs the two pieces the
validation exposed, kept here as the roadmap, not this project's scope:

- **Typed-parameter ABI emission**: consume `typed_param_abi`'s `typeable_params`
  to emit typed-signature (`PVecI64`) function variants and coerce at call sites
  (specialize-by-representation vs adapt-at-call — the open fork). This is the
  substantial piece.
- **`builder_from` producer recognition**: add `builder_from` to
  `RouteIds`/`FamilyIds` + typed `builder_from_i64`/`_bool` variants, so manual
  `.append` accumulators type like `collect` does. Needed so general functions
  (not just the hand-written kernel) can produce typed intermediates.

If/when typed-parameter ABI lands, the sort kernel can be retired in favor of the
plain generic `sort_by` typing through it.

## Testing & validation

- **Commit `b6_size_probe`** as `examples/performance/sort-bench/b6_size_probe.tw`
  (the ablation sizing the boxed-read floor; regression guard).
- **WAT assertions on the kernel:** the typed sort path reads `get_i64` and builds
  with `builder_*_i64`; no generic `get`/`box_i64` inside the merge; the kernel's
  own params are `PVecI64`.
- **Benchmark gate:** `order_by_breakdown.tw` at N = 1M — `sort idx by amount`
  ~745 → ~450–545ms; full `order_by` ~1.2s → ~0.9–1.0s (signal, not oracle).
  `sort_by_component_probe` / `merge_attribution_probe` for the floor.
- **Correctness:** identical sort results vs the generic `sort_by` on randomized
  and adversarial inputs (already-sorted, reverse-sorted, all-equal, single, empty,
  duplicates); full `make boot-test` green; self-host (`make bundle-cli`
  fixed-point) — the compiler itself sorts, so a miscompile breaks the bootstrap.
- **Boundary coercion:** a boxed-source `Vector<Int>` sorted via the kernel
  (unbox-once path) returns a correct boxed `Vector<Int>`; a typed-local source
  passes typed with no per-element boxing.

## Risks & mitigations

- **Kernel ↔ generic divergence.** The kernel must match `sort_by`'s exact
  semantics (stability, comparator contract, the ascending/descending fast paths).
  *Mitigation:* mirror the prelude algorithm; differential test against the generic
  `sort_by` across the input matrix above.
- **Recognition precision.** Mis-recognizing a non-typed `sort_by` instance, or
  missing the intended one. *Mitigation:* gate strictly on the element family; fall
  through to generic `sort_by` on any doubt (correctness-preserving default).
- **Comparator boundary.** The kernel calls the closure with unboxed `i64`; ensure
  the closure-call path matches the comparator's expected ABI (it already takes
  unboxed `Int`). *Mitigation:* reuse the existing typed non-tail closure-call path
  (T3.2); verify against a comparator that captures a typed column (C2) and one
  that does not.
- **Stage0 parity.** Boot-codegen perf opt; per the no-stage0-parity rule for
  boot-only codegen opts, stage0 needs the kernel only if boot source relies on it
  to compile. Boxed correctness is preserved (generic `sort_by` still exists), so
  boot should still bootstrap; flag if `make stage2` disagrees.
