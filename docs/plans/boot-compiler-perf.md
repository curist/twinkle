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

## Current baseline: 2026-05-12

Measured with the current bundled CLI (`target/twk`) compiling `boot/main.tw`.
The boot compiler currently loads 173 modules and emits 1956 functions for this
workload.

Wall-clock self-compilation:

```text
real: 2.88s - 2.93s
```

Representative phase timings from three timed runs:

```text
compile_modules    1073 - 1104ms
optimize            255 - 273ms
emit_module         255 - 271ms
prepare_backend     221 - 241ms
emit_wasm_binary    193 - 219ms
  code_section      141 - 157ms
  small_sections     34 - 40ms
link                184 - 215ms
verify              181 - 190ms
core_link           108 - 116ms
plan_wasm_types      67 - 69ms
lower_anf            53 - 58ms
monomorphize         49 - 54ms
closure_convert      19 - 20ms
```

Optimizer subphase shape:

```text
dead_let       ~64 - 66ms
copy_prop      ~67 - 72ms
uniqueness     ~59 - 68ms
defer_elim      ~9 - 10ms
const_fold      ~5 - 7ms
branch_simp     ~5 - 7ms
```

Backend planning and verification details:

```text
plan_wasm_types: 82027 slot registration calls, 729 unique types
verify:          80071 slots, dominated by expression walking
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

The bottleneck has moved back to the frontend. `compile_modules` is now far
larger than any single backend phase. The next tier is broad rather than a
single obvious hotspot: optimization, module emission, backend preparation,
wasm binary emission, linking, and verification are all close enough that local
sub-timings matter.

The current module graph is also much larger than the historical 84-module
workload, so older absolute timings should not be used for regressions. Treat
this snapshot as the active baseline.

## Plan

### 1. Frontend: `compile_modules`

This is the clearest whole-pipeline target. Start with sub-timing inside module
compilation before changing algorithms.

Questions to answer:

- Which frontend stages dominate now: parsing, resolving imports, name
  resolution, type checking, alias expansion, or lowering?
- Are large modules paying repeated type-alias expansion, `zonk`, substitution,
  instantiation, or environment lookup costs?
- Are non-generic functions and types still flowing through generic machinery
  that can fast-path empty maps or empty type-argument lists?
- Are imported module environments rebuilt or merged repeatedly when they could
  be shared or cached for the compilation session?

Likely useful probes:

- per-module and per-stage timing within `compile_modules`;
- counters for substitution calls with empty maps;
- counters for alias expansion / zonking of the same type shapes;
- resolver/checker environment lookup counts for the largest modules.

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
  subphase, but small sections have grown enough to inspect too.
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
