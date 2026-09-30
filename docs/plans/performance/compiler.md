# Compiler Performance Plan

Compiler-throughput side of the performance effort — the shape of the
self-hosted boot compiler and the levers worth chasing. Generated-program
runtime performance lives in [compiled-programs.md](compiled-programs.md).

This file keeps the **current baseline plus durable lessons**, not a timeline.
Older dated snapshots have been collapsed into the lessons below; the compiler,
runtime data structures, module graph, and codegen shape have all changed enough
that the raw April–June numbers are no longer useful as baselines.

## How to measure

Build with the bundled CLI and enable compiler timings:

```bash
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/twinkle-boot.wasm
```

For wall-clock, run the same build without timing output:

```bash
/usr/bin/time -p target/twk build boot/main.tw -o /tmp/twinkle-boot.wasm
```

Use **same-session A/B comparisons** for optimization work. Whole-pipeline
timings are noisy (±15% on `lower`/`emit_module`), so a single sample never
justifies a change on its own. For codegen/ownership work, gate every change on
**byte-identical output** (A/B diff) plus the `make stage2` fixed point
(stage3 == stage4).

## Current baseline (2026-09-30, after recursive-aggregate + boxed-reference MutVec)

Compiling `boot/main.tw` (277 modules / ~4752 functions, `wanted` 4155) with the
bundled CLI, sound-uniqueness codegen enabled (the default). Three timing-disabled
wall-clock samples were **29.65–31.69s, median ~30.85s**. The two sound-uniqueness
ownership phases dominate even harder than before — **~20.8s of the ~30.9s
median**:

```text
variant_specialize         15.30–15.62s median ~15.4s
produce_mutable_decisions  5.31–5.62s   median ~5.4s
compile_modules            3.23–3.38s   median ~3.3s
emit_module                1.47–1.53s   median ~1.5s
emit_wasm_binary           0.74–0.76s   median ~0.75s
verify                     ~0.68s
prepare_backend            ~0.56s
core_link                  ~0.42s
link                       ~0.32s
run_mutvec_call            ~0.28s
plan_wasm_types            ~0.20s
lower_anf                  ~0.17s
monomorphize               ~0.12s
wasm_dce                   ~0.10s
optimize                   ~0.05s
builder_region_rewrite     ~0.05s
closure_convert            ~0.03s
```

Sub-breakdown of the two dominant phases:

```text
variant_specialize:        table ~8.1s
                           variants ~5.1s
                           groups ~1.9s; filter ~0.06s
produce_mutable_decisions: summary ~4.2s
                           summary:reuse ~2.0s
                           cfg ~0.15s; ownership ~0.97s
```

Frontend medians remain a secondary cost, up slightly with the larger module
graph: `typecheck` ~0.58s (`bodies` ~0.41s), `import_merge` ~0.36s, `lower`
~0.36s, `resolve` ~0.30s.

### What changed since the 2026-08-29 baseline

The build is ~51% slower wall (20.4s → ~30.9s). Two things moved together: the
module graph grew (265 → 277 modules, ~4123 → ~4752 functions, ~15%), and the
recursive-aggregate + boxed-reference MutVec features landed
(`docs/plans/archive/2026-09-28-boxed-reference-mutvec.md` and siblings), which
thread projected-borrow facts through the whole-program ownership summary and add
boxed-family classification + owned-variant eligibility into variant
specialization. The regression is concentrated exactly there: `variant_specialize`
grew 9.99s → ~15.4s, and within it the **`variants` subphase jumped ~813–866ms →
~5.1s (~6×)** — far more than the ~15% function growth alone explains, so the new
per-variant ownership/classification work is the primary driver.
`produce_mutable_decisions` grew more modestly (4.53s → ~5.4s; `summary` ~3.5 →
~4.2s, `summary:reuse` ~1.65 → ~2.0s), tracking the larger program.

This is a coincidence-of-timing attribution, not an isolated A/B: the numbers
were not re-measured against the pre-MutVec compiler on identical source, and the
module-graph growth is confounded in. The next probe should A/B the `variants`
subphase with the boxed/projected-borrow classification paths bypassed on
identical source to confirm the ~4s delta is the MutVec ownership work rather than
program growth, before treating it as a lever.

**What the `variants` subphase is, and where the cost is NOT.** `variants` is
`summary.compute_variants` (variant_specialize.tw:500), which runs an ownership
fixpoint (`run_scc_variants`) once per candidate variant per SCC. A first guess was
that the projected-borrow state (`ForwardState.projected_shell`, added by the MutVec
Task 1 foundation) threaded through every such run was the overhead. **Reading the
code rules that out as the primary cost:** `merge_projected_exit`
(ownership.tw:6155) iterates only `old.keys()`, and `projected_shell` is non-empty
only for a function that reads a GC-reference vector element (`xs[i]`). For the
~99% of functions that never do, the map is empty and the per-join merge is an
empty loop already — gating it would recover ~nothing. Do **not** start with a
"gate `projected_shell`" change; it is very likely a null result.

**The actual next probe (measure first, no plan doc needed).** The ~6× jump is
more likely *more work of the same kind*: the boxed/owned-variant eligibility
producing **more candidate variants** (so the per-variant fixpoint runs more
times), or heavier per-run summaries, amplified across `4155` wanted functions.
Instrument `compute_variants`/`run_scc_variants` to report the candidate-variant
count, the number of SCC-variant fixpoint runs, and per-part timing, and compare
against the pre-MutVec compiler on identical source (build both CLIs, or bisect the
recursive-mutvec-abi range). Only once the dominant term is identified does a lever
follow — and acceptance is byte-identical output + the `make stage2` fixed point
regardless of which lever it turns out to be.

### The dominant redundancy: the whole-program summary is computed ~twice

`variant_specialize`'s `summary.compute` (5806ms) runs the ownership fixpoint over
**all ~4123 functions** to build the summary + variant tables, producing a
`FixResult` per function but passing `no_cache_ids` — so **every FixResult is
discarded**. `produce_mutable_decisions` then reuses the summary *table* (landed
summary-reuse) but re-runs ~588 SCCs, mostly to reconstruct the FixResults for the
~548 mutation-candidate roots (`has_root` forces a re-run to fill the cache). The
giant functions are summarized twice at near-identical cost — `link` 428+422ms,
`analyze_copy_carriers` 161+147ms, `run_fixpoint` 146+141ms,
`extract_exports_for_module` 107+102ms. This is the primary algorithmic lever (see
the FixCache-reuse update below).

### Shape interpretation

- The frontend (`compile_modules`) is not the bottleneck — it is about 13% of
  wall time once sound-uniqueness codegen is on. Its cost is still many small
  reasonable costs across a large module graph (`typecheck`, `import_merge` top).
- The backend tier (`emit_module`, `optimize`, `verify`, `prepare_backend`) is
  broad and close together — sub-timings matter, and these transform IR so they
  are more correctness-sensitive. Treat them as measure-first, not obvious wins.
- The heaviest absolute cost is the sound-uniqueness codegen phases, now about
  71% of wall time. The next measurement should decompose 8G's ~7.4s `table`
  cost by function/SCC and by liveness versus fixpoint work before attempting a
  data-structure change; the MutDict census does not match this workload.

### Landed: reuse 8G's FixResults in the mutable producer (`TWINKLE_8G_FIXREUSE`)

8G's `summary.compute` produces a FixResult per function and discarded them all;
the producer then re-ran the ownership fixpoint over the ~548 candidate roots to
reconstruct them. Now 8G caches the candidate roots' fixes (`compute_cached`),
carries them in `SpecializeResult`, and the producer pre-seeds its scoped-summary
cache so the safe-root SCCs are skipped. **summary:reuse ~3105 → ~1513ms;
produce_mutable_decisions ~5923 → ~4256ms; wall ~21.45 → ~19.97s median.**

This lever was **reverted once as "FIXVERIFY-unsound," which was wrong** — a
misdiagnosis worth its own lesson. `analyze:unique_analysis_diags` (and the
`merge_targeted__{Bool,Int,Vec_Int}` monomorphizations) are a **pre-existing,
tracked-red FIXVERIFY baseline**: they mismatch on plain `main` too (lever off,
even with `TWINKLE_SUMMARY_REUSE=0`), documented in the archived owned-variant
plans as "measure the delta, not the absolute." `TWINKLE_FIXVERIFY` errors on the
**first** mismatch, so seeing that name proves nothing. A `TWINKLE_FIXVERIFY_CENSUS`
mode (list the full mismatch set, don't trap) showed the lever-on set is
byte-for-byte the same four functions as lever-off — **zero new mismatches** — and
output is byte-identical over the whole self-build.

**Durable lesson:** acceptance for any ownership/fix change is the **FIXVERIFY
delta** (census set with the change vs without) plus byte-identity + `make stage2`,
never "`TWINKLE_FIXVERIFY` prints nothing." A first-mismatch trap over a codebase
with a known-red baseline will misattribute an unrelated failure to your change —
exactly what sank this lever's first attempt.

### Null result: scope 8G's whole-program `summary.compute`

The largest single sound-uniqueness cost is 8G's `summary.compute` (~5.8s over all
~4106 funcs). Scoping it to a "variant-relevant closure" was investigated and
**rejected at the measurement gate**: the required scope is **93.9% of the
program** (`down_wanted=3235` ≈ 79%, `up_callers=873`, `union=3857/4106`).
`candidate_variants` reads every function's summary, and `collect_groups` needs
accurate summaries for the *upward caller closure* of published callees (873 funcs
outside the downward `wanted` closure) to judge caller arg-uniqueness. On top of
that, 8G's summary is the producer's reuse base, so it must stay accurate for the
producer's `wanted` (~79%) regardless — a hard floor. Scoping to 94% saves <7%
(~350ms) while risking a dropped variant (codegen change). Below the 0.85
stop-gate; not pursued past Phase 1. *Lesson: a whole-program analysis whose
consumers include an upward caller scan can't be scoped to a downward closure —
the caller closure drags the scope back to ~whole-program.*

## Landed wins (durable lessons)

Grouped by area. Each is a "stop doing unnecessary work / defer until needed"
change, validated self-host-stable + boot-suite-green, byte-identical where the
change touches codegen.

**Frontend**

- **Import merge — lazy origin index + skip identity TypeId remaps**
  (~485 → ~226ms, ~53%). `plan_export_type_ids` rebuilt a full inverted
  `origin → TypeId` index eagerly per edge though it's only read on a name-lookup
  miss (the minority), and `remap_function_sig`/`remap_type_def` walked and
  reallocated every signature/type-def to apply id→**itself** no-op remaps.
  Build the index lazily on first miss; omit identity mappings and short-circuit
  the remap when the map is empty. *Lesson: no-op remaps still walk and realloc
  trees; build derived indices lazily on the first real consumer.*
- **Typecheck — Pass 0 rebuild skip + finalize no-meta guard** (~425 → ~358ms).
  Pass 0 only mutates function `ret` types, so calling `with_functions` (which
  re-filters every visible function's index/bindings/origins) was pure waste —
  swap the ret-updated vector in directly (setup ~52 → ~2ms). The finalize sweep
  zonked ~157k `type_map` entries and rebuilt every type tree even when nothing
  resolved — skip `zonk` on meta-free entries (~114 → ~101ms). `bodies` (~260ms,
  the bidirectional inference walk) is the irreducible core. *Lesson: don't
  rebuild an index that doesn't depend on what the pass mutates; a meta-free type
  is unaffected by substitution.*
- **Linker — hoist `ns_prefix`** (`link` ~320 → ~227ms). Phase 4 recomputed the
  per-module `ns_prefix` (an O(len²) char-by-char string build) on every renamed
  instruction. Compute once per module and thread it through. *Lesson: hoist any
  per-item recompute that only depends on the enclosing scope.*
- **LSP — skip the occurrence index on occurrence-free requests.** Every editor
  snapshot eagerly built the file's occurrence index (a full AST walk on cache
  miss, on every keystroke), but hover/completion/signature-help never read it.
  Gated behind `with_occurrences` + `_lite` snapshot paths. Interactive-latency
  win, not a batch-build metric. *Lesson: gate expensive per-snapshot derivations
  on the consumers that actually need them.*

**Backend / codegen**

- **`prepare_backend` — typed-vector analysis scope filter**
  (`analyze_typed_repr` ~226 → ~39ms; `prepare_backend` ~565 → ~384ms). The joint
  typed-repr fixpoint did ~4 whole-body walks per element-family per function over
  **all** 3340 functions, but only **155** have a `Vector<Int>`/`Vector<Bool>`
  slot; a function with no family slot contributes nothing to any fixpoint dict.
  Filter to the family-slot subset once at the top. *Lesson: gate a whole-program
  analysis to the functions it can actually classify — output is identical, skipped
  funcs contribute nothing.*
- **Reuse 8G's pruned CFG in the mutable producer** (`TWINKLE_CFG_REUSE`; producer
  `cfg` phase ~845 → ~49ms). The mutable-decision producer rebuilt the whole
  post-clone CFG view (`build_view |> prune_dead_merge`) from scratch, though
  Phase 8G already built and pruned a per-function view over the pre-clone module
  and specialization only edits clone bodies + rewrites a few callers' call
  targets (in the boot build only ~70 of ~4127 functions change). Track exactly
  which functions the routing rewrite changed, carry 8G's pruned view + that set
  in `SpecializeResult`, and rebuild only the changed functions
  (`build_view_reusing` + `prune_dead_merge_selective`), reusing the rest.
  Byte-identical because `build_view`/`prune_dead_merge` are purely per-function
  and `prune_function` is idempotent, so a reused already-pruned `CfgFunction`
  equals a fresh rebuild. *Lesson: unlike FixResults (scope-dependent), a
  `CfgFunction` is a purely structural per-function fact — safe to carry across
  the 8G→producer boundary for every function specialization didn't touch.*
- **Deep-IR traversal shape (the worklist tax).** A stack-safety pass once
  converted many backend/optimizer IR walks to `Vector`-backed worklists that box
  every child into a GC vector (~2–9× slower per node than native frames),
  regressing every post-monomorphize phase. Recovery shape: **iterate the deep
  direction — the linear `Let` spine — and recurse only into control-flow branch
  bodies, whose nesting is shallow.** Reserve an explicit worklist / depth-gated
  fallback only for the one or two walks where nesting itself is genuinely
  unbounded (`slot_assign`'s else-if chains). *Lesson: a Vector worklist over IR
  nodes is a real per-node tax; iterate the unbounded spine, recurse the bounded
  nesting.*

**Interprocedural MutVec** (`mutvec_call_*` — storage-tier "S4")

- **`run_mutvec_call` discovery scope — gate to route-caller functions**
  (`run_mutvec_call` ~1.95s → ~0.17s, ~11×). The interprocedural-MutVec phase ran
  `detect_call_thread_regions` over **all ~4100 functions** on every compile,
  and `detect` rebuilt the whole `route_site_targets` table + a builtin lookup
  per call. On `boot/main.tw` (which claims zero regions) that was ~1.95s — ~9%
  of wall — of pure overhead. Two output-identical fixes: **(a)** hoist the
  spec-invariant inputs (route table + `vector$len` id) out of the ~4100-call
  loop (`detect_call_thread_regions_with`); **(b)** only a function that *calls a
  routed clone* can host a region, so gate the scan to the route-caller subset,
  recovering each `route_site`'s caller func-id by unpairing the Szudzik
  `vid.site_key` (self-contained integer `isqrt` — importing `@std.math` would
  drag `math.tw` into the compiler's bootstrap graph, which fails on `Float.abs`
  at the self-host stage). Sound for over-cap routes (`clone_func = -1`) too,
  since it keys on the site's caller, not on whether a clone exists. Byte-
  identical (self-host fixed point holds); a `site_key_func` round-trip test
  across both pairing branches guards the inverse. *Lesson (same as the
  `prepare_backend` typed-vector filter): a whole-program scan should be gated to
  the functions it can actually classify — here, decode the classifiable set
  straight out of the phase's own route table instead of walking every body.*

**Sound-uniqueness codegen** (roughly halved the two dominant late phases)

- **Summary-reuse.** The mutable-decision producer seeds its scoped summary from
  8G's carried whole-program ownership table, recomputing only the
  clone-affected closure instead of running the fixpoint a second time.
- **Cold ownership fixpoint worklist.** The cold pass swept every block every
  round (~63% no-op re-visits on `summary:link`). Seed all blocks dirty at a cold
  start and drive the existing successor-based worklist, re-processing a block only
  when a predecessor's exit changed. **Order-preserving only** — a skipped visit is
  a provable no-op, so block order/rounds/change-sequence are unchanged and output
  stays byte-identical. *Lesson: skipping no-op visits is safe; reordering is not —
  widening is visit-order-sensitive, so a priority/SCC worklist could diverge.*
- **Liveness reuse in `field_reqs`.** `summarize_function` computed liveness for
  its own fixpoint, then `collect_field_reqs` recomputed it over the identical
  blocks — thread the computed liveness in. Byte-identical by construction.
- **Loop-seed incremental re-propagation.** Big functions rerun the validated
  fixpoint up to ~14× to validate optimistic loop-carried `Unique` seeds. A rerun
  only ever *removes* seeds, changing exactly the dropped-seed blocks' entries.
  Carry the **full** solver state (exit maps + widening state) across reruns and
  process only a dirty set. `summary:link` rerun ~3443 → ~660ms, no cold fallback.
  *Lesson: when reruns only shrink the input, re-propagate incrementally from the
  changed blocks instead of restarting.*

**Perf-neutral / measured-and-rejected (don't re-try without new evidence)**

- **Loop-seed warm-start-by-restart** — net regression. Warm-start seeds all exit
  maps and marks blocks processed, so early rounds run full meets over large maps
  (cold's early rounds are cheap and grow), and the dominant widening function
  falls back to cold anyway. Superseded by incremental re-propagation above.
- **`call_uniques` fixpoint reuse** — not viable. `summary.compute` runs the
  generic resolver; `call_uniques` runs the render resolver, which is strictly
  *more precise* (owned-variant selection changes call effects). Reusing the
  summary `fx` **loses clones** — a codegen change, not a transparent speedup.
- **Empty-`subst` fast path in `zonk_with_meta`** — perf-neutral; finalize cost
  concentrates in meta-bearing modules the fast path doesn't accelerate.
- **awfy-c5 in-place `set_at`** — perf-neutral for self-compilation. The lever
  that gives user programs large wins doesn't apply: the compiler's hot loops
  accumulate via `Vector.append` (builder) and `Dict`, with only ~9 `.set_at`
  sites total, all off the compile hot path.
- **Ownership-fixpoint map backing swap (HAMT → sorted-pair-array)** — rejected on
  measurement. The dominant analysis phases thread `Dict<Int,T>` maps of width
  32–60; the intuition was that HAMT hash+traverse+`.keys()`-union overhead could be
  cut with a width-compact flat backing. Round-1 spike
  (`boot/bench/fixpoint_map_spike.tw`) showed 13–19× — but only for fork-everything
  (M=W) and no reads. Round-2 spike (`fixpoint_map_divergence_spike.tw`) measured the
  real mix: **the HAMT wins point reads 7–12×** (at these widths it is 1–2 levels
  deep, so a get is ≈ one hash + a hop, vs 5–6 branchy binary-search iterations) and
  wins low-divergence merges (M/W ≲ 20%, the convergence common case — O(M·log W)
  structural sharing beats an O(W) array rebuild). Only fresh builds favor the array,
  and one backing can't win builds *and* reads. *Lesson: at small/shallow widths a
  persistent HAMT get is near-constant, not a traverse tax — a flat/sorted backing
  only pays off for build-heavy, read-light, high-divergence maps, which these are
  not.* (The named-type refactor over these maps proceeds for readability, keeping the
  HAMT backing — `docs/plans/fixpoint-map-intmap.md`.)

## Historical lessons (still apply)

The old investigation started from a much slower compiler where associative-list
`Dict`, flat copy-on-write vectors, repeated layout derivation, and temporary
code-section copies dominated. Those specific bottlenecks are gone, but the
lessons transfer:

- Replacing the linear `Dict` with a persistent HAMT reshaped nearly every phase
  by removing O(n) environment/symbol-table lookups.
- Accumulator-style emission helps where code repeatedly builds small temporary
  vectors and concatenates them.
- **Reusing per-pass facts beats structural rewrites**: emission reuses layout
  caches; repr assignment caches mono-derived repr/value-type/layout facts; wasm
  code-section emission caches name→index and writes directly into the output
  buffer.
- The most reliable workflow: instrument the hot subphase, find repeated
  derivation or copying, remove it with a small targeted cache or accumulator —
  not a parser/checker rewrite.

## Open levers / next probes

Diminishing and all measure-first; prefer small repeated-work eliminations over
broad rewrites unless instrumentation proves the structural cost is real.

- **Frontend — import merge representation.** Cost is cumulative across many tiny
  edges (largest single edge is single-digit µs), so the lever is a
  representation change, not an edge tweak. The `selective` bucket (~126ms) still
  registers the full imported interface before binding selected names; a
  per-selected-item support closure would need the exporter's method fixpoint
  re-run per edge (deferred — cleaner as an exporter-side per-visible-export
  closure). `typecheck` `bodies` (~260ms) is the irreducible inference walk;
  `finalize`'s remaining lever is subtree-sharing inside `zonk_with_meta` (needs a
  change-tracking return shape).
- **Backend.** `emit_module` is ~407ms in the per-function `emit_func` loop
  (~0.12ms/func) — the irreducible codegen walk, not a broad local win.
  `optimize`'s cleanest identified lever is a `count_uses`/`collect_assigned_locals`
  fusion in `dead_let` (~25ms, modest, COW-correctness-sensitive). `verify` is
  dominated by per-node type checks, not the pre-walk.
- **Sound-uniqueness floor.** 8G's whole-program summary fixpoint is inherent (it
  decides clones). Seed-validation reruns are largely exhausted (remaining
  headroom means a soundness-critical batched/dependency-ordered seed-retraction
  algorithm). Cross-pass liveness sharing for `call_uniques`/`analyze` is
  ~0.3–0.5s but needs a threaded per-view liveness cache.
- **Runtime data structures** — justify by compiler profiles, not standalone
  cleanups: typed vector families to cut `anyref` traffic in hot homogeneous
  vectors; RRB-style concat/slice if instruction-buffer concat reappears;
  CHAMP-style HAMT layout if dict allocation/iteration locality resurfaces.

## Working rules for future updates

- Keep only the current baseline plus durable lessons in this file.
- Collapse obsolete snapshots into lessons instead of appending a timeline.
- Record ranges or representative same-session A/B results, not isolated numbers.
- State what changed, why it matters, and what the next measurement should prove.
