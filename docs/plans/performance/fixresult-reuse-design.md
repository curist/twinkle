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
analyze pass will actually consume, so its size is bounded to ~the analyzed set
(~460), not the full summarized closure.

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

### Threading (signature changes)

Summary side:

- `summarize_seeded(...)` returns `.{ summary: Summary, fx: FixResult, reusable: Bool }`.
  Its existing callers `summarize_function` and `summarize_variant` take
  `.summary`; their external behavior is unchanged.
- The SCC driver (`run_scc`) accumulates `fx` into a `FixCache` for members that
  are `reusable` and in the candidate-root set. For a multi-member SCC, a
  `reusable` member's `fx` is stable across driver iterations (it depends on no
  in-SCC sibling), so caching the final value is correct.
- A new `compute_for_roots_cached(view, b, sem, candidate_funcs)` returns
  `.{ table: SummaryTable, fix_cache: FixCache }`. Existing `compute_for_roots`
  remains as a thin wrapper returning only the table, for other callers.

Analyze side:

- `analyze_selected_with_summaries(view, b, sem, table, selected, cache: FixCache)`
  gains the cache parameter.
- `analyze_function(f, table, b, sem, unique_seed, cached_fx: FixResult?)` and
  `ownership_stage(..., cached_fx: FixResult?)` gain an optional cached fx. When
  `.Some(fx)`, `ownership_stage` **skips `run_fixpoint_validated`** and proceeds
  directly to materialization; when `.None`, it computes as today.
- `analyze_with_summaries` (the full / test-suite path) passes an empty cache, so
  `cached_fx` is always `.None` there and tests continue to exercise the real
  fixpoint.

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

The cache holds one `FixResult` per eligible candidate root (~460) from
summary-end to analyze-consumption, then is freed. This is genuinely new peak
memory: today each fx is computed and immediately discarded in both passes. The
whole `view` (all blocks) is already resident, so ~460 fx is expected to be
acceptable, but it is the one measured risk.

**Fallback if memory bites:** cap the cache with a predicate (e.g. skip caching
functions above some block/instruction count). The largest functions are few and
still save the most time, so even a partial cache captures most of the win. This
is a predicate change only, no architectural change.

## Acceptance and Testing

- **Mutable census byte-identical** — the hard accept gate. Run the census on the
  candidate build before and after; `diff` must be clean.
- **Self-host stable** — stage2 == stage3 byte-identical output.
- **Full boot test suite green.**
- **`TWINKLE_FIXVERIFY=1` build of `boot/main.tw`** completes without trapping —
  exercises the guard on the real hot functions (`link` et al.), not just
  fixtures.
- **Timing** — `[time] produce_mutable_decisions` and the `[time:mutable:artifacts]
  ownership=` sub-timing drop materially. Record before/after in
  `docs/plans/performance/compiler.md`.

## Out of Scope (YAGNI)

- No reuse for multi-member-SCC members that call siblings — they recompute.
- No caching of variant/seeded runs (`summarize_variant`).
- No persistence of the cache across builds.
- No change to the full `analyze_with_summaries` / test-suite analysis path.
- No change to the loop-seed collector or `validate_loop_seed` proof gate.
