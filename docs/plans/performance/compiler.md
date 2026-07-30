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

## Current baseline (2026-07-31)

Compiling `boot/main.tw` (~239 modules / ~3340 functions) with the bundled CLI.
Wall-clock (timing off): **~4.95–5.08s**. Representative whole-pipeline phase
timing (single instrumented run; treat as shape, not exact):

```text
compile_modules   ~1950ms   (frontend; still the largest bucket)
emit_module        ~519ms
optimize           ~474ms
verify             ~400ms
prepare_backend    ~384ms
core_link          ~280ms
link               ~216ms
emit_wasm_binary   ~209ms
plan_wasm_types    ~123ms
lower_anf          ~115ms
monomorphize        ~76ms
wasm_dce            ~57ms
closure_convert     ~23ms
```

Frontend sub-timing (the dominant bucket, spread across many small costs):

```text
typecheck    ~358ms   (bodies ~260, finalize ~101, setup ~2)
import_merge ~226ms   (module ~62, selective ~126, prelude ~35)
lower        ~215–273ms
plan_deps    ~196ms
resolve      ~164ms
load_source  ~136ms
parse        ~110ms
publish       ~65ms
env_extend    ~53ms
```

**Sound-uniqueness codegen** runs as separate late phases not in the table above.
After the summary-reuse → cold-worklist → liveness-reuse → incremental arc, per
heavy self-host build:

```text
variant_specialize (8G)   ~11.5–12.8s -> ~7.5s
produce_mutable_decisions ~10.5–12.6s -> ~5.0s
```

That arc roughly halved sound-uniqueness codegen (~23–25s → ~12.5s/build); full
`make stage2` wall ~73s.

### Shape interpretation

- The bottleneck is the **frontend** (`compile_modules`), but it is now many
  small reasonable costs across a large module graph, not one runaway stage.
  `typecheck` and `import_merge` are the top two sub-buckets.
- The backend tier (`emit_module`, `optimize`, `verify`, `prepare_backend`) is
  broad and close together — sub-timings matter, and these transform IR so they
  are more correctness-sensitive. Treat them as measure-first, not obvious wins.
- The heaviest absolute cost in a full `make stage2` is the sound-uniqueness
  codegen phases, whose remaining floor is the 8G whole-program ownership
  fixpoint (it decides clones, so it is largely inherent).

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
