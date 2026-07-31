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

## Current baseline (2026-07-31, sound-uniqueness on by default)

Compiling `boot/main.tw` (~257 modules / ~4123 functions) with the bundled CLI,
sound-uniqueness codegen enabled (the default). Wall-clock (timing off):
**~20s**. The two sound-uniqueness ownership phases dominate everything —
**~13.5s of the ~20s**:

```text
variant_specialize        ~7.7s   ← 8G whole-program ownership summary + variants
produce_mutable_decisions  ~5.9s   ← scoped summary reuse + ownership analyze
compile_modules            ~2.5s   (frontend)
emit_module                ~0.95s
prepare_backend            ~0.52s
verify                     ~0.48s
optimize                   ~0.47s
core_link                  ~0.31s
link                       ~0.26s
emit_wasm_binary           ~0.25s
plan_wasm_types            ~0.15s
lower_anf                  ~0.14s
monomorphize               ~0.09s
```

Sub-breakdown of the two dominant phases:

```text
variant_specialize:        table (summary.compute) ~5806ms   groups ~1616ms   variants ~671ms
produce_mutable_decisions: summary (reuse path)    ~4786ms   cfg ~987ms       ownership ~813ms
```

The frontend numbers (`compile_modules ~2.5s`, `typecheck` ~358ms, `import_merge`
~226ms, etc.) are unchanged from the pre-sound-uniqueness measurements below and
are now a small fraction of the build. Older frontend phase/sub-timing tables
were measured with sound-uniqueness codegen off (`TWINKLE_VARIANT_SPECIALIZE=0`)
and so omit these two phases; their shape still holds for the front half.

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

- The frontend (`compile_modules`) is no longer the bottleneck — it is a small
  fraction once sound-uniqueness codegen is on. Its cost is still many small
  reasonable costs across a large module graph (`typecheck`, `import_merge` top).
- The backend tier (`emit_module`, `optimize`, `verify`, `prepare_backend`) is
  broad and close together — sub-timings matter, and these transform IR so they
  are more correctness-sensitive. Treat them as measure-first, not obvious wins.
- The heaviest absolute cost is the sound-uniqueness codegen phases, whose
  remaining floor is the 8G whole-program ownership
  fixpoint (it decides clones, so it is largely inherent).

### Inconclusive: reuse 8G's FixResults in the mutable producer (revert was misattributed)

The apparent redundancy — 8G computes a FixResult per function and discards it,
then the mutable producer re-runs the fixpoint over the candidate roots — was
prototyped (carry 8G's per-candidate-root `FixCache` in `SpecializeResult`,
pre-seed the mutable producer's cache, skip re-running the all-safe root SCCs).
The win was real (`summary:reuse` ~2999 → ~1339ms, `produce_mutable_decisions`
~5.9 → ~4.6s, wall ~20 → ~19s) and **output was byte-identical** on the boot
self-build. It was reverted after `TWINKLE_FIXVERIFY` reported
`analyze:unique_analysis_diags`.

**Correction (2026-07-31):** that reasoning was wrong.
`analyze:unique_analysis_diags` is a **pre-existing, tracked-red FIXVERIFY
baseline** — it mismatches on plain `main` too (with the lever's flag off *and*
with `TWINKLE_SUMMARY_REUSE=0`), and the archived owned-variant plans already
name it as "the tracked-red baseline, present even before Task 1 — measure the
delta, not the absolute" (`archive/owned-variant-codegen-handoff.md`,
`archive/aggregate-field-owned-variants.md`). FIXVERIFY errors on the **first**
mismatch, so seeing that name proves nothing about the lever; the correct gate is
**no NEW mismatch beyond the baseline** (a delta / census), which was never run.
Combined with the byte-identical output, the fix-reuse lever is **inconclusive,
not disproven** — quite possibly sound.

The right way to settle it is the census in
[2026-07-31-8g-fixcache-reuse.md](2026-07-31-8g-fixcache-reuse.md) Phase 0: turn
FIXVERIFY into a full mismatch list and compare the set with the flag on vs off.
**Durable lesson:** `analyze:unique_analysis_diags` is a known FIXVERIFY red;
acceptance for any ownership change is *delta against that baseline* (plus
byte-identity + stage2), never "FIXVERIFY prints nothing."

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
