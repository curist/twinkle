# FixResult Reuse Across the Summary and Analyze Passes — Design

**Status:** Design (approved 2026-07-23). Implementation plan to follow.

## Goal

Cut `produce_mutable_decisions` time by eliminating the redundant ownership
fixpoint that mutable-decision production runs **twice** per function: once in
the summary pass and once in the analyze pass. Cache the summary pass's
`FixResult` and reuse it in the analyze pass for functions where the two runs
are provably identical.

## Background and Evidence

Mutable-decision production (`compute_candidate_artifacts`,
`boot/compiler/codegen/ownership_verdicts.tw`) runs two ownership passes over the
same CFG view, both scoped to the mutable candidate roots:

```
table    := summary.compute_for_roots(view, b, sem, candidate_funcs)
analyzed := ownership.analyze_selected_with_summaries(view, b, sem, table, candidate_funcs)
```

Both call the same `run_fixpoint_validated(...)` (combined ownership + validity +
provenance fixpoint) on each function, then extract *different projections* from
the resulting `FixResult`:

- the **summary** pass derives whole-function param roles / return effects;
- the **analyze** pass materializes per-block facts and mutable verdicts.

On a full self-build the fixpoint dominates: `[time:mutable:artifacts]
summary≈11.6s ownership≈8.3s`, and the worst function `link` is run through the
fixpoint twice (~3.3s + ~3.4s) with 14 validation reruns each.

A prior loop-seed-filtering attempt was **reverted as a null result** — the cost
is `reruns × cost(run_fixpoint)`, and reruns is bound by invalid-seed
dependency-chain depth, not seed count, so trimming seeds did not move it. The
real redundancy is the double fixpoint itself.

**Byte-identical experiment (2026-07-23).** Temporary instrumentation serialized
and hashed the full `FixResult` (all five fields, sorted keys) for every generic
run. Over a full self-build, **456 of 461 analyze-pass FixResults were
byte-identical** to a summary-pass FixResult for the same function. The only 5
mismatches were all multi-member SCC members (`parse_prefix`, `synth_call`,
`check_closure`, `lower_expr`, `atomize`), where in-SCC suppression is
non-inert. This confirms the reuse is viable and sound-gateable.

## Why the two runs match (and when they don't)

The generic summary run and the analyze run of a function `f` differ in only two
inputs:

1. **`suppress`**: the summary pass passes `suppress = scc_set` (the function's
   SCC member set — `summary.tw` `run_scc`); the analyze pass passes an empty
   suppress (`ownership_stage`'s `no_suppress`). `suppress` only affects direct
   calls to func-ids in the set, breaking recursion during summary computation.
2. **`table` completeness**: at summary time the SCC being processed has only
   provisional summaries for its own members; the analyze-time table is complete.

Both differences vanish under a single condition:

> **`f` makes no direct call to any func-id in its own SCC** (equivalently, no
> direct call to any id in `suppress`).

If that holds, `suppress` is inert for `f`, and all of `f`'s direct callees sit
in earlier, already-finalized SCCs (topological order), so the `table`
projection `f` depends on is identical in both passes. Therefore the `FixResult`
is byte-identical, and reusing it is sound. Indirect/closure calls do not consult
per-id `suppress`, so they never disqualify `f`.

`unique_seed` is the third input; the analyze pass always uses an empty
`unique_seed`, so only the **generic** summary run (empty `unique_seed`) is a
reuse candidate. Variant/seeded runs (`summarize_variant`) are naturally
excluded.

### Coverage: this gate is exactly "singleton, non-self-recursive"

Under the current direct-call SCC graph, **every multi-member SCC member makes at
least one direct call to another member** (strong connectivity: the first hop of
any intra-SCC path stays in-SCC). So `calls_suppressed` is always true for
multi-member members — they are never cached. The gate therefore reduces to
"the function is a singleton SCC and does not call itself." The
`calls_suppressed(blocks, suppress)` scan is kept as the implementation because it
tests the soundness condition directly (robust to graph changes), but the
expected cached set is the **singleton, non-self-recursive** functions.

In the byte-identical experiment this is ~420 of the 461 analyzed functions
(including `link` and the dominant cost). The ~36 multi-member functions whose
fx happened to match despite non-inert suppression are **not** cached by this
gate — they make in-SCC direct calls, so they are conservative (safe) misses,
recomputed as today. The 5 genuine mismatches are likewise not cached.

## Architecture

All changes localize to the `compute_candidate_artifacts` pair. The cache is a
plain value produced by the summary step and consumed by the analyze step — no
global state, no lifecycle beyond these two adjacent calls:

```
table, fix_cache := summary.compute_for_roots_cached(view, b, sem, candidate_funcs)
analyzed         := ownership.analyze_selected_with_summaries(
  view, b, sem, table, candidate_funcs, fix_cache,
)
```

### The cache

```
type FixCache = Dict<Int, FixResult>   // func_id -> generic, analyze-equivalent fx
```

Keyed by func-id, holding the generic `FixResult`. Populated only for functions
that are (a) `reusable` (see gate) and (b) in the candidate-root set that the
analyze pass will actually consume, so its size is bounded to the reusable subset
of the analyzed set (~420 of ~460 in the experiment; the ~40 non-reusable
candidate roots recompute), not the full summarized closure.

### The eligibility gate (local, no call-graph plumbing)

`summarize_seeded` already holds the function's blocks and its `suppress` set, so
it computes eligibility locally:

```
reusable = unique_seed is empty
       and not calls_suppressed(blocks, suppress)
```

`calls_suppressed(blocks, suppress)` scans every `.ACall` instruction and returns
true if any direct callee's func-id is in `suppress`. This scan is cheap relative
to the fixpoint and runs once per function.

### Public API boundary (concrete)

Today `FixResult` and `summarize_seeded` are **private** to
`boot/compiler/ownership.tw`, and the SCC driver in `summary.tw` calls the public
`summarize_function(...)`, which returns only `Summary`. The design must open a
concrete boundary so `summary.tw` can build a cache:

- Export the result type: `pub type FixResult` (already a plain record; no shape
  change).
- Add the cache type + accessors in `ownership.tw`, all public:
  ```
  pub type FixCache = .{ by_func: Dict<Int, FixResult> }
  pub fn empty_fix_cache() FixCache
  pub fn fix_cache_put(c: FixCache, func_id: Int, fx: FixResult) FixCache
  pub fn fix_cache_get(c: FixCache, func_id: Int) FixResult?   // .None when absent
  ```
- Add a public cached summarizer that exposes the fx and eligibility:
  ```
  pub fn summarize_function_cached(
    f: CfgFunction, table: SummaryTable, b: BuiltinRegistry,
    sem: OptimizerSemantics, suppress: Dict<Int, Bool>,
  ) .{ summary: Summary, fx: FixResult, reusable: Bool }
  ```
  Internally this is `summarize_seeded` with an empty `unique_seed`, returning the
  fx it already computes plus `reusable = !calls_suppressed(f.blocks, suppress)`.
  Existing `summarize_function` becomes `summarize_function_cached(...).summary`
  (behavior unchanged for its other callers); `summarize_variant` is untouched
  (seeded ⇒ never reusable).

### Threading (signature changes)

Summary side:

- The SCC driver (`run_scc`) calls `summarize_function_cached(...)` for the
  generic pass, takes `.summary` for the table, and accumulates `.fx` into a
  `FixCache` via `fix_cache_put` **iff** `.reusable` **and** the member is in the
  candidate-root set (`candidate_funcs`). For a multi-member SCC a `reusable`
  member's `fx` is stable across driver iterations (it depends on no in-SCC
  sibling — but per the coverage note, multi-member members are not reusable
  anyway), so caching the final value is correct.
- A new `pub fn compute_for_roots_cached(view, b, sem, candidate_funcs)` returns
  `.{ table: SummaryTable, fix_cache: FixCache }`. Existing `compute_for_roots`
  remains as a thin wrapper returning only the table, for other callers.

Analyze side:

- `analyze_selected_with_summaries(view, b, sem, table, selected, cache: FixCache)`
  gains the cache parameter. **Both** the untimed branch (currently
  `analyze_function(..., Dict.new())` at ownership.tw:3205) and the timed branch
  (ownership.tw:3236) must look up `fix_cache_get(cache, f.func_id)` and pass the
  result down **identically** — the timed path must not diverge in reuse
  behavior, only in instrumentation.
- `analyze_function(f, table, b, sem, unique_seed, cached_fx: FixResult?)` and
  `ownership_stage(..., cached_fx: FixResult?)` gain an optional cached fx. When
  `.Some(fx)`, `ownership_stage` **skips `run_fixpoint_validated`** and proceeds
  directly to materialization; when `.None`, it computes as today.
- **Soundness invariant:** `ownership_stage` consults `cached_fx` **only when
  `unique_seed` is empty**. Materialization still reads `unique_seed`, so reusing
  a generic fx under a seeded analysis would be unsound. The analyze pass always
  passes an empty `unique_seed`, so this is an assertion guarding against future
  callers, not a live branch: if `cached_fx` is `.Some` while `unique_seed` is
  non-empty, ignore the cache and compute (optionally trap under the verify flag).
- `analyze_with_summaries` (the full / test-suite path) passes
  `empty_fix_cache()`, so `cached_fx` is always `.None` there and tests continue
  to exercise the real fixpoint.

Materialization (`join_entry_ownership` etc. → `blk.entry`/`blk.exit` facts and
verdicts) is unchanged and still runs on every analyzed function. Because the
reused `fx` is byte-identical, materialization output — and therefore the mutable
census — is bit-for-bit identical.

## Verify mode (`TWINKLE_FIXVERIFY`, off by default)

A standing, gated guard against future changes silently breaking the gate's
assumptions. On a cache **hit** with the flag set, `ownership_stage` still runs
`run_fixpoint_validated` and compares the fresh fx against the cached one; any
divergence traps via `error("fixverify mismatch: <func>")`.

Comparison uses a canonical serializer `render_fix_result(fx) String` (sorted
keys over all five `FixResult` fields — the same form the experiment
prototyped), kept permanently but invoked only under the flag. Release cost is a
single env read per analyzed function.

## Memory

The cache holds one `FixResult` per eligible candidate root (~420) from
summary-end to analyze-consumption, then is freed. This is genuinely new peak
memory: today each fx is computed and immediately discarded in both passes. The
whole `view` (all blocks) is already resident, so ~420 fx is expected to be
acceptable, but it is the one measured risk (see Acceptance for the RSS
measurement).

**Fallback if memory bites:** cap the cache with a predicate (e.g. skip caching
functions above some block/instruction count). The largest functions are few and
still save the most time, so even a partial cache captures most of the win. This
is a predicate change only, no architectural change.

## Acceptance and Testing

The cache lives only in `compute_candidate_artifacts` (the build / mutable
decision-production path). The `--census` command uses `compute_artifacts` (the
**full** path — `boot/commands/ir.tw:141`), which has no cache, so a census diff
**does not exercise the optimization**. Acceptance must hit the candidate path.

Correctness (hard accept gates):

- **Build output byte-identical** — build `boot/main.tw` before and after the
  change; the emitted `.wasm` must be byte-for-byte identical. The build path runs
  `produce_mutable_decisions → compute_candidate_artifacts`, so this is the
  end-to-end test on the cached path. (Also verify **self-host stable**:
  stage2 == stage3.)
- **`TWINKLE_FIXVERIFY=1` build of `boot/main.tw`** completes without trapping —
  exercises the recompute-and-compare guard on the real hot functions (`link` et
  al.), not just fixtures.
- **Full boot test suite green.**
- **Mutable census byte-identical** — kept as a secondary check. It validates the
  full path, not the cache, so it guards against collateral damage but is not the
  primary gate.

Directed gate tests (make the local `calls_suppressed` gate regression-resistant).
Add boot tests over small fixtures asserting the `reusable` flag / cache
membership for each case:

- non-recursive, no in-SCC direct call → **cache hit** (`reusable == true`);
- direct self-recursive singleton → **cache miss** (`reusable == false`);
- mutual-recursive SCC member with in-SCC direct calls → **cache miss**;
- seeded / variant summary (`summarize_variant`, non-empty `unique_seed`) →
  **cache miss**.

  These use the public `summarize_function_cached` / `summarize_variant` and
  `FixCache` accessors, so no new test-only hooks are needed beyond the API above.

Performance (attributable, not just "faster"):

- Add a gated counter line, e.g.
  `[time:mutable:fixcache] hits=<n> misses=<n> verify_reruns=<n>`, emitted under
  `TWINKLE_TIMINGS`, so the win is attributable to cache reuse rather than noise.
- Report **repeated A/B** timings (several runs each) of `[time]
  produce_mutable_decisions` and the `[time:mutable:artifacts] ownership=`
  sub-timing, before vs after. Record in `docs/plans/performance/compiler.md`.

Memory (the one measured risk):

- Report **before/after peak RSS** for `target/twk build boot/main.tw` (e.g.
  `/usr/bin/time -l` on macOS). If the retained ~420 `FixResult`s push peak RSS
  beyond an acceptable bound, apply the block-count-cap fallback (below) and
  re-measure.

## Out of Scope (YAGNI)

- No reuse for multi-member-SCC members that call siblings — they recompute.
- No caching of variant/seeded runs (`summarize_variant`).
- No persistence of the cache across builds.
- No change to the full `analyze_with_summaries` / test-suite analysis path.
- No change to the loop-seed collector or `validate_loop_seed` proof gate.
