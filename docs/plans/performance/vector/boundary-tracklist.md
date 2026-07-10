# Typed-vector boundary tracklist

**Purpose:** one glanceable map of every boundary a typed `PVecI64` vector must
cross to stay fast, so "how many more rabbit holes?" has an answer. The end goal is
the dataframe `order_by` win (drop the ~7× gap vs Clojure ~0.34s @ 1M).

**Where we are (2026-07-10, branch `typed-vector-crossfn-abi`):** C2 (oracle
unification) typed the captured key column into the comparator. A separate B6
attempt — a buffer-backed sort kernel (`@std.sort.ints_by`) — was built, measured
(~35% on `order_by`), then **REVERTED** (`bec1bd5c`) as a compiler-special-cased
point solution. Its findings stand and reset the priorities:

> **Findings (2026-07-10, survive the revert):**
> - Full `order_by` is ~1.2s, **not** the ~1.84s previously recorded here.
> - The merge floor is ~490ms of *boxed idx reads* out of a ~510ms floor; mechanics
>   (closures/Order/recursion) are ~17ms. The old "floor is mechanics" framing is
>   **stale**.
> - Typed *return* and typed *capture* ABIs exist for named functions/closures, but
>   a typed **parameter** ABI does **not** exist anywhere — every named function
>   takes boxed `PVec` params. The merge reads through params, so B6 the principled
>   way needs general typed-parameter ABI emission (the "Extend" project), not a
>   kernel.
> - Secondary: `route_typed_vec` types `collect` builders but not manual `.append`
>   accumulators (they seed via `builder_from`, unknown to the router).

**B6 remains open** (pursue via typed representation / typed-param ABI, not a
special-cased kernel). The other remaining lever is **B8** (typed `take`/gather for
non-Int columns); all storage/ABI/capture boundaries for the sort itself are ✅.

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
| B5 | Typed **field-read** copy propagation | ✅ | `1d815c37` (2b now group-aware; fixed a live `zs := b.xs` invalid-Wasm miscompile + verifier edge) |
| B6 | Direct-call typed **argument** ABI (`fn(xs: PVecI64)`) | ⬜ | analysis-only — the typed-repr analysis computes a `typeable_params` set, but emission still passes every param boxed; no physical typed-arg pass-through yet (the "Extend" project) |
| B7 | Generic runtime-helper ABI — typed `gather` | ✅ | `gather_i64` routing (Stage 1) |
| B8 | Generic runtime-helper ABI — `take` / other helpers | 🟡 | `gather` done; `take` and friends box → unbox (~830ms in full order_by) |

## C. Closure capture — typed vectors captured into a comparator/closure

| # | Boundary | Status | Notes |
|---|----------|--------|-------|
| C1 | Captured **builder-candidate** (`collect`) → typed env + `get_i64` reads | ✅ | M1b local-capture (proven end-to-end) |
| C2 | Captured **payload / typed-return call-result** → typed env | ✅ | oracle unification (`fd3da98f`…`5776e82b`); the dataframe key column now types end-to-end (`sort idx by amount` ~1400→~775ms) |
| C3 | Captured field-read → typed env | 🟡 | unblocked by B5 + the oracle unification (field-read result is typed and flows through the same capture fixpoint); not specifically exercised/benched |

## D. Boundaries that MUST stay boxed (the safety invariant — do NOT cross)

| Boundary | Why |
|----------|-----|
| 🚫 anyref / universal `ClosureEnv` (generic/untyped closures) | O(n)-per-read pathology if a typed vector lands here and is read in a loop (uniform typing was reverted for this — `m1a-anyref-readback-investigation.md`) |
| 🚫 Generic container (`Vector<Vector<Int>>`, dict values, `array<anyref>`) | element type is erased |
| 🚫 Untyped variant payload (boxed producer) | source isn't genuinely `PVecI64` |

---

## The dataframe `order_by` critical path (the headline)

`build IntCol (A3 ✅) → amounts := as_ints(col) (B2 ✅) → comparator captures amounts, reads amounts[a] (C2 ✅) → take/gather (B7 ✅ / B8 🟡)`

- **`sort idx by amount` (~1343ms → ~775ms):** C2 landed via the oracle
  unification, so the captured `amounts` column now stays typed into the
  comparator (`get_i64` reads) instead of re-boxing at the call site. **Done.**
- **Full `order_by` (~2.3s → ~1.84s):** still wants **B8** (typed `take`) so the
  gather/take steps stop boxing — this is the remaining headline lever.

## What "finishing the chain" means right now

1. ~~**C2**~~ — ✅ landed (oracle unification, `fd3da98f`…`5776e82b`).
2. **B8** — typed `take`/helper ABI, for the full `order_by` number. ← current
   remaining lever on the headline path.
3. **C3** — captured field-read → typed env (🟡, unblocked by B5 + the
   unification; verify/bench if a real path wants it).
4. Deferred oracle-unification refinements (not blocking the win): full
   `PhysPlan` materialization for field/payload/param/return ABI products +
   dirty-tracking cost lever; the *coercing* verifier edges (call args/returns/
   variant payloads). See `unify-typedness-oracle-design.md` §3/§4/§6.

Everything else in this folder is either landed (A1–A3, B1–B5, B7, C1–C2) or a
rejected/measured approach kept for the record.

## Spike finding (2026-07-07) — RESOLVED by the oracle unification (2026-07-08)

The spike below proved the C2 mechanism but surfaced a structural blocker: the
router had **two** "is this slot `PVecI64`" oracles (`slot_typed_after_route` for
the analyses vs `route_func`'s `eligible_v` for the actual retyping) that diverged
on the dataframe's multi-use column group, so emit dropped a box while the slot
stayed boxed → invalid Wasm.

**Resolution:** `unify-typedness-oracle-design.md`, implemented in five commits
(`fd3da98f`…`5776e82b`). `compute_eligible_v` is now the single ground-truth
eligibility; `slot_typed_after_route` delegates to it; capture typing is a
greatest fixpoint over a materialized `slot_repr` (so the support check sees
relaxed captures), fail-closed on non-convergence; and a post-route verifier edge
guards the capture store. C2 now types the real dataframe column end-to-end with
valid Wasm (`sort idx by amount` ~775ms). Two group-awareness gaps the verifier
caught along the way (B5 field-read copies, gather-result copies) were fixed as
part of the same work.

<details>
<summary>Original spike finding (kept for the record)</summary>

A throwaway spike wired capture typing for a call-result source (alias-aware
`slot_typed_after_route` + `relaxed` seeded from Condition-1 candidates, threaded
into `analyze_typed_captures`). Result:

- **Isolated: it works.** A captured typed-return call-result *and its copy* type
  into the comparator — `PVecI64` env param, `get_i64` reads, valid, runs. The
  `typed_payload_capture_guard` bench stays fast (~5ms, no per-read-unbox
  miscompile).
- **Dataframe: invalid Wasm, sort number NOT measurable.** On the real
  `bench_n` (the key column captured into *two* comparators + gathered + copied),
  the module fails validation: `route_func` skipped the `box_i64` after `as_ints`
  (it now believes the column is typed) but did **not** retype the destination slot
  → a `PVecI64`→`PVec` store mismatch.
- **Root cause = the two-analyses divergence.** They agree on simple code but
  diverge on the dataframe's complex multi-use group, so emit drops a box while
  the slot stays boxed.

</details>
