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

## Current baseline: 2026-05-13

Measured with the current bundled CLI (`target/twk`) compiling `boot/main.tw`.
The boot compiler currently loads 174 modules and emits 1995 functions for this
workload.

Wall-clock self-compilation remains roughly in the same range as the previous
baseline:

```text
real: ~2.8s - 3.0s
```

Representative phase timing:

```text
compile_modules    ~1095ms
emit_module         ~339ms
optimize            ~251ms
prepare_backend     ~205ms
emit_wasm_binary    ~188ms
verify              ~175ms
link                ~117ms
core_link           ~107ms
plan_wasm_types      ~61ms
lower_anf            ~50ms
monomorphize         ~48ms
closure_convert      ~18ms
```

Frontend subphase timing is now instrumented under `TWINKLE_TIMINGS=1`:

```text
load_source       ~142ms
parse              ~56ms
plan_deps         ~114ms
dep_hashes          ~5ms
env_extend          ~8ms
import_merge      ~276ms
resolve            ~92ms
typecheck         ~168ms
publish            ~51ms
unused_imports     ~21ms
lower             ~143ms
```

The standout frontend cost is import/interface merging. Type checking, module
lowering, source loading, and dependency planning form the next tier. The
instrumented frontend buckets account for nearly all of `compile_modules`; the
remaining uninstrumented overhead in the representative run was about 20ms.

Deeper import timing shows this is cumulative rather than one pathological edge:

```text
import edges:       2316
module imports:      272, ~59ms total
selective imports:   604, ~165ms total
prelude imports:    1440, ~41ms total
export entries processed while merging: ~91500
```

Selective imports are the largest import-merge bucket despite fewer edges than
prelude imports. The current selective path registers the full imported
interface first, then binds only selected names, so many `use module.{...}` edges
still pay full-interface registration cost. No single import edge dominates;
the largest observed edges were only around one to two milliseconds, so this is
cumulative modular overhead rather than an isolated pathological dependency.

Optimizer subphase shape:

```text
dead_let       ~60 - 65ms
copy_prop      ~62 - 68ms
uniqueness     ~59 - 63ms
defer_elim      ~9 - 10ms
const_fold      ~5 - 7ms
branch_simp     ~5 - 7ms
```

Backend planning and verification details:

```text
plan_wasm_types: ~83686 slot registration calls, 737 unique types
verify:          ~81691 slots, dominated by expression walking
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
phase, yet its main buckets are spread over 174 modules and thousands of import
edges.

The next tier is broad rather than a single obvious hotspot: optimization,
module emission, backend preparation, wasm binary emission, linking, and
verification are all close enough that local sub-timings matter. `emit_wasm_binary`
serializes the 1.8 MiB compiler payload in roughly 190ms, dominated by code
section encoding; this is worth keeping efficient but is not a large enough
fraction of the build to be a primary speed lever.

The current module graph is also much larger than the historical 84-module
workload, so older absolute timings should not be used for regressions. Treat
this snapshot as the active baseline.

## Plan

### 1. Frontend: `compile_modules`

This remains the largest whole-pipeline bucket, but the latest sub-timing makes
it less likely that there is a simple broad frontend win. The main cost is not
parsing or name resolution; it is cumulative import/interface merging.

Current frontend timing shape:

```text
import_merge      ~276ms
typecheck         ~168ms
lower             ~143ms
load_source       ~142ms
plan_deps         ~114ms
resolve            ~92ms
parse              ~56ms
publish            ~51ms
unused_imports     ~21ms
env_extend          ~8ms
dep_hashes          ~5ms
```

Interpretation:

- Import merging is the best-understood frontend hotspot, but the measured cost
  is distributed across many small edges. A meaningful improvement probably
  requires a broader interface/environment representation change rather than a
  local tweak.
- Typecheck, lower, source loading, and dependency planning are each around one
  millisecond or less per module on this workload. Further digging may still
  find small fast paths, but they should not be expected to produce a large
  structural speedup.

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
