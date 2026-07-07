# Typed-vector boundary tracklist

**Purpose:** one glanceable map of every boundary a typed `PVecI64` vector must
cross to stay fast, so "how many more rabbit holes?" has an answer. The end goal is
the dataframe `order_by` win (drop the ~7× gap: ~1343ms `sort idx by amount`,
~2.3s full `order_by` @ 1M vs Clojure ~0.34s).

**The model (do not violate):** `PVecI64` is a *per-storage-site* optimization,
never a global property of `Vector<Int>`. A typed vector that reaches a *durable
erased* boundary (anyref, the universal `ClosureEnv`, a generic container, an
untyped variant) must be **boxed** — boxed once at the crossing, read boxed. See
`../representation-boundary-policy.md`. The whole game is: keep a vector typed along
its *actual path* through the program, and box it only where it genuinely erases.

Status: ✅ done · 🟡 partial · ⬜ open · 🚫 must-stay-boxed (by design)

---

## A. Storage sites — where a `PVecI64` can physically live

| # | Boundary | Status | Where |
|---|----------|--------|-------|
| A1 | Non-escaping local (`collect` + index) | ✅ | main (S2.0) |
| A2 | Typed record field | ✅ | main (S2.2) |
| A3 | Typed variant payload (`IntCol(Vector<Int>)`) | ✅ | Milestone A (this branch; `as_ints` types `IntCol`) |

## B. Cross-function ABI — typed vectors flowing through calls

| # | Boundary | Status | Notes |
|---|----------|--------|-------|
| B1 | Direct-call result coercion to destination vt | ✅ | `expected-vt-coercion` (this branch) |
| B2 | Return-as-`Vector<Int>` ABI (`as_ints` returns `PVecI64`) | ✅ | this session (tail-match result typing) |
| B3 | Typed call-result **copy/alias** propagation (`keys := as_ints(...)`) | ✅ | this session (group-aware 2c/2c'') |
| B4 | Typed **payload** copy/alias propagation | ✅ | this session (group-aware 2c) |
| B5 | Typed **field-read** copy propagation | ⬜ | invalid-Wasm bug (2b not group-aware); off dataframe critical path |
| B6 | Direct-call typed **argument** ABI (`fn(xs: PVecI64)`) | 🟡 | boxing adapters exist; verify true typed-arg pass-through |
| B7 | Generic runtime-helper ABI — typed `gather` | ✅ | `gather_i64` routing (Stage 1) |
| B8 | Generic runtime-helper ABI — `take` / other helpers | 🟡 | `gather` done; `take` and friends box → unbox (~830ms in full order_by) |

## C. Closure capture — typed vectors captured into a comparator/closure

| # | Boundary | Status | Notes |
|---|----------|--------|-------|
| C1 | Captured **builder-candidate** (`collect`) → typed env + `get_i64` reads | ✅ | M1b local-capture (proven end-to-end) |
| C2 | Captured **payload / typed-return call-result** → typed env | ⬜ | mechanism proven (spike below); **blocked on unifying the two typedness analyses** (see spike finding) |
| C3 | Captured field-read → typed env | ⬜ | blocked on B5 (same copy bug) |

## D. Boundaries that MUST stay boxed (the safety invariant — do NOT cross)

| Boundary | Why |
|----------|-----|
| 🚫 anyref / universal `ClosureEnv` (generic/untyped closures) | O(n)-per-read pathology if a typed vector lands here and is read in a loop (uniform typing was reverted for this — `m1a-anyref-readback-investigation.md`) |
| 🚫 Generic container (`Vector<Vector<Int>>`, dict values, `array<anyref>`) | element type is erased |
| 🚫 Untyped variant payload (boxed producer) | source isn't genuinely `PVecI64` |

---

## The dataframe `order_by` critical path (the headline)

`build IntCol (A3 ✅) → amounts := as_ints(col) (B2 ✅) → comparator captures amounts, reads amounts[a] (C2 ⬜) → take/gather (B7 ✅ / B8 🟡)`

- **`sort idx by amount` (~1343ms):** blocked solely on **C2** (comparator capture). B2 landed this session but the win doesn't show until C2 lands — the captured `amounts` re-boxes at the call site today.
- **Full `order_by` (~2.3s):** also wants **B8** (typed `take`) so the gather/take steps stop boxing.

## What "finishing the chain" means right now

1. **C2** — type captured payload/call-result columns (analysis-only; validation-spike-gated: prove the sort number moves before building the full analysis). ← current focus
2. **B8** — typed `take`/helper ABI, for the full `order_by` number (after C2).
3. **B5 / C3** — field-read copy bug (correctness; latent invalid-Wasm), independent of the sort win.

Everything else in this folder is either landed (A1–A3, B1–B4, B7, C1) or a
rejected/measured approach kept for the record.

## Spike finding (2026-07-07) — C2 mechanism works, but exposes a structural blocker

A throwaway spike wired capture typing for a call-result source (alias-aware
`slot_typed_after_route` + `relaxed` seeded from Condition-1 candidates, threaded
into `analyze_typed_captures`). Result:

- **Isolated: it works.** A captured typed-return call-result *and its copy* type
  into the comparator — `PVecI64` env param, `get_i64` reads, valid, runs. The
  `typed_payload_capture_guard` bench stays fast (~5ms, no per-read-unbox
  miscompile). So the C2 mechanism and the model (typed comparator reads) are
  confirmed.
- **Dataframe: invalid Wasm, sort number NOT measurable.** On the real
  `bench_n` (the key column captured into *two* comparators + gathered + copied),
  the module fails validation: `route_func` skipped the `box_i64` after `as_ints`
  (it now believes the column is typed) but did **not** retype the destination slot
  → a `PVecI64`→`PVec` store mismatch.
- **Root cause = the two-analyses divergence (the deferred "question 2").** The
  capture/typedness *analysis* (`slot_typed_after_route` / `capture_abi`) and the
  actual slot-*retyping* (`route_func`'s `eligible_v`) are two parallel
  implementations of "is this slot `PVecI64`". They agree on simple code but
  **diverge on the dataframe's complex multi-use group**, so emit drops a box while
  the slot stays boxed. Making one side more aggressive without the other produces
  invalid Wasm.

**Conclusion:** the blocker to the `order_by` win is **not "one more boundary"** —
it is that the typed-vector router has two "is-typed" oracles that must be unified
into one source of truth before an escaping, multi-use column can be typed
end-to-end without invalid Wasm. **C2 depends on that unification.** This is the
real structural next step, and it is bigger than the analysis-only fix C2 first
appeared to be.
