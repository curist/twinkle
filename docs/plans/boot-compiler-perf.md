# Boot Compiler Performance Plan

This document tracks the current performance shape of the self-hosted boot
compiler and the next investigations worth doing. Older measurements from the
April compiler are intentionally collapsed into lessons learned: the compiler,
runtime data structures, module graph, and generated code shape have changed
enough that those raw numbers are no longer useful as baselines.

## How to measure

Build with the bundled CLI and enable compiler timings:

```bash
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/twinkle-boot.wasm
```

For wall-clock checks, run the same build without timing output:

```bash
/usr/bin/time -p target/twk build boot/main.tw -o /tmp/twinkle-boot.wasm
```

Use same-session A/B comparisons for optimization work. Whole-pipeline timings
are noisy enough that a single sample should not justify a change by itself.

## Current baseline: 2026-06-20

Measured with the current bundled CLI (`target/twk`) compiling `boot/main.tw`,
two same-session timing runs plus three wall-clock samples. The boot compiler now
loads 215 modules and emits 2939 functions for this workload — a much larger
graph than the May snapshot (174 modules / 1995 functions), reflecting the
channels, task-concurrency, terminal-framework, and LSP work that has landed
since.

Wall-clock self-compilation has grown roughly in proportion to the larger graph:

```text
real: ~4.07s - 4.11s
```

Representative phase timing (range across the two runs):

```text
compile_modules    ~1690 - 1850ms
emit_module         ~386 - 409ms
optimize            ~331 - 373ms
prepare_backend     ~316 - 337ms
verify              ~290 - 295ms
emit_wasm_binary    ~236 - 256ms
core_link           ~232 - 243ms
link                ~185 - 197ms
plan_wasm_types     ~103 - 107ms
lower_anf            ~86 - 92ms
monomorphize         ~68 - 72ms
wasm_dce             ~44 - 54ms
closure_convert      ~22 - 25ms
```

`wasm_dce` is a new phase since the May baseline. The biggest relative growth is
in the frontend (see below) and in `verify` (~175ms → ~290ms), tracking the
larger function count.

Frontend subphase timing (range across the two runs):

```text
import_merge      ~408 - 443ms
typecheck         ~369 - 406ms
lower             ~248 - 257ms
plan_deps         ~189 - 204ms
resolve           ~130 - 144ms
load_source       ~123 - 139ms
parse              ~93 - 106ms
publish            ~56 - 62ms
env_extend         ~17 - 20ms
unused_imports     ~14 - 17ms
dep_hashes          ~6ms
```

Import/interface merging is still the standout frontend cost, but `typecheck`
(~168ms → ~390ms) and `lower` (~143ms → ~250ms) have grown the most in absolute
terms and are now firmly in the same tier. The instrumented frontend buckets
account for nearly all of `compile_modules`.

Deeper import timing shows this is cumulative rather than one pathological edge:

```text
import edges:       3426
module imports:      357, ~77ms total
selective imports:   717, ~180ms total
prelude imports:    2352, ~180ms total
export entries processed while merging: ~144900
```

Selective and prelude imports are now tied as the largest import-merge buckets.
Prelude edges grew the most (1440 → 2352) as the prelude surface widened, so
their cumulative cost has caught up to selective imports despite each prelude
edge being individually tiny. The selective path still registers the full
imported interface first, then binds only selected names, so many `use
module.{...}` edges still pay full-interface registration cost. No single import
edge dominates; the largest observed edges were only a few microseconds, so this
remains cumulative modular overhead rather than an isolated pathological
dependency.

Optimizer subphase shape:

```text
funcs=2939  total_rounds=6271  avg_rounds=2.13  at_cap=24

uniqueness     ~96ms
dead_let       ~90ms
copy_prop      ~90ms
defer_elim     ~18ms
const_fold     ~12ms
branch_simp    ~11ms
```

Backend planning and verification details:

```text
plan_wasm_types: ~120358 slot registration calls, 1022 unique types
verify:          ~117419 slots; expr_walk ~194ms dominates slot_checks ~99ms
```

## What changed since the old plan

The old investigation started from a much slower compiler where associative-list
`Dict`, flat copy-on-write vectors, repeated layout derivation, and temporary
code-section copies dominated large parts of the pipeline. Those specific
bottlenecks have already been addressed or made less central by later compiler
changes.

Important historical lessons that still apply:

- Replacing the linear `Dict` with a persistent HAMT changed the shape of nearly
  every phase by removing O(n) environment and symbol-table lookups.
- Accumulator-style emission helped where code repeatedly built small temporary
  vectors and concatenated them into larger buffers.
- Reusing per-pass facts was often better than structural rewrites:
  - emission reuses layout caches instead of repeatedly deriving record/sum
    layouts;
  - repr assignment caches mono-derived representation, value-type, and layout
    facts;
  - wasm code-section emission caches name-to-index lookups and writes sections
    directly into the final output buffer.
- The most reliable optimization workflow has been: instrument the hot subphase,
  identify repeated derivation or copying, then remove that repeated work with a
  small targeted cache or accumulator change.

## Current interpretation

The bottleneck has moved back to the frontend, but the frontend profile is now
mostly many small reasonable costs across a large module graph rather than one
obvious runaway stage. `compile_modules` is larger than any single backend
phase, yet its main buckets are spread over 215 modules and thousands of import
edges.

The next tier is broad rather than a single obvious hotspot: optimization,
module emission, backend preparation, wasm binary emission, linking, and
verification are all close enough that local sub-timings matter. `emit_wasm_binary`
serializes the ~2.7 MiB compiler payload in roughly 240ms, dominated by code
section encoding; this is worth keeping efficient but is not a large enough
fraction of the build to be a primary speed lever.

The current module graph (215 modules / 2939 functions) is much larger than both
the historical 84-module workload and the May 174-module snapshot, so older
absolute timings should not be used for regressions. Treat this snapshot as the
active baseline.

## Plan

### 1. Frontend: `compile_modules`

This remains the largest whole-pipeline bucket, but the latest sub-timing makes
it less likely that there is a simple broad frontend win. The main cost is not
parsing or name resolution; it is cumulative import/interface merging.

Current frontend timing shape:

```text
import_merge      ~408 - 443ms
typecheck         ~369 - 406ms
lower             ~248 - 257ms
plan_deps         ~189 - 204ms
resolve           ~130 - 144ms
load_source       ~123 - 139ms
parse              ~93 - 106ms
publish            ~56 - 62ms
unused_imports     ~14 - 17ms
env_extend         ~17 - 20ms
dep_hashes          ~6ms
```

Interpretation:

- Import merging is the best-understood frontend hotspot, but the measured cost
  is distributed across many small edges. A meaningful improvement probably
  requires a broader interface/environment representation change rather than a
  local tweak.
- Typecheck has grown faster than the module count (~1.7ms/module now, up from
  ~1ms in May) and has joined import merging as a top frontend bucket; it is now
  worth its own subphase instrumentation. Lower, source loading, and dependency
  planning are each still around one millisecond or less per module. Further
  digging may find small fast paths, but outside typecheck they should not be
  expected to produce a large structural speedup.

Possible future probes, if frontend work resumes:

- split selective import registration internally into type registration,
  function registration, value registration, method registration, and final
  binding work;
- prototype a selective-import fast path only if we are willing to compute the
  needed support-entry closure for selected exports;
- consider caching/remapping an imported interface view per `(dependency,
  alias, import kind/items)` within one compilation session;
- add typechecker counters for empty substitution, alias expansion, and zonk;
- measure whether `load_source` is real file I/O cost or source hashing / path
  canonicalization / overlay lookup overhead.

Prefer small repeated-work eliminations over parser or checker rewrites unless
instrumentation proves the structural cost is real.

### 2. Optimizer: `optimize`

The optimizer remains a top-tier phase, but its cost is spread across a few
passes rather than one runaway pass.

Next checks:

- `dead_let`, `copy_prop`, and `uniqueness` should each get direct subphase A/B
  timing before optimization work.
- Look for repeated traversals over the same ANF body that can be fused without
  making pass behavior harder to reason about.
- Check whether use-count, free-variable, purity, or uniqueness facts can be
  shared within one optimization round.
- Investigate the functions hitting the optimization round cap; confirm whether
  they represent real missed simplification or just harmless churn.

Avoid broad optimizer restructuring until a specific repeated traversal or fact
recomputation is identified.

### 3. Code generation and wasm emission

`emit_module`, `prepare_backend`, `emit_wasm_binary`, and `link` are now in a
similar range. Work here should be driven by sub-timings, not by the old
assumption that code-section encoding is always the only target.

Areas to probe:

- `emit_module`: residual layout/type/value-type lookup churn, helper discovery,
  and instruction-vector building in large functions.
- `prepare_backend`: remaining slot/repr assignment scans and repeated
  mono-derived facts not covered by the existing cache.
- `emit_wasm_binary`: code-section body encoding is still the largest wasm
  subphase. The whole binary emission phase is only a modest share of the build,
  so even a strong local win here is useful but not transformative.
- `link`: current timings are higher than the older post-HAMT snapshots; measure
  symbol resolution, map merges, and final module assembly separately.

### 4. Verification and wasm type planning

These are not the first targets, but they are large enough to watch for obvious
repeated work.

Checks:

- `verify` is dominated by expression walking; look for avoidable rewalking of
  unchanged bodies or repeated slot-entry lookups.
- `plan_wasm_types` performs many slot registration calls for a much smaller set
  of unique types; confirm whether repeated registrations are cheap cache hits or
  still doing unnecessary work.

### 5. Runtime data-structure follow-ups

The compiler now runs on the erased persistent `PVec` runtime described in
[`persistent-vector.md`](persistent-vector.md). Keep measuring vector-heavy
compiler paths before changing vector layout.

Potential runtime investigations:

- typed vector families to reduce `anyref` traffic in hot homogeneous vectors;
- RRB-style concat/slice improvements if instruction-buffer concatenation still
  appears in profiles;
- CHAMP-style HAMT layout improvements if dictionary allocation or iteration
  locality shows up again.

These should be justified by compiler profiles rather than implemented as
standalone runtime cleanups.

## Working rules for future updates

- Keep only the current baseline plus durable lessons in this file.
- Move obsolete raw snapshots out of the main narrative instead of appending a
  long timeline.
- Record ranges or representative same-session A/B results, not isolated single
  numbers.
- State what changed, why it matters, and what the next measurement should prove.
