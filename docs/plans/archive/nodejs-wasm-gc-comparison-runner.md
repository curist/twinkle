# Node.js Wasm GC comparison runner

## Why this exists

Recent persistent-vector measurements strongly suggest the remaining read-heavy
slowdown may be caused less by Twinkle's vector algorithm itself and more by
**Wasmtime's current Wasm GC behavior** on large object graphs.

The clearest evidence so far:
- trie depth itself is cheap
- eliminating leaf wrappers improved large-vector reads materially
- widening nodes made performance worse, which suggests the runtime is sensitive
  to GC object shape / reference-slot scanning cost
- large vectors still carry a substantial per-access tax even on tail-only reads

That leaves an important open question:

> Is the remaining slowdown fundamentally in Twinkle's emitted Wasm/runtime
> representation, or is it mostly specific to Wasmtime's current Wasm GC
> implementation?

We should answer that with an apples-to-apples comparison using the **same
compiled Wasm module** under a second Wasm GC runtime.

## Chosen direction

Implement a **minimal Node.js runner** for stage0-emitted `.wasm` / `.wat`.

We intentionally target **only one** alternate host runtime. Node.js is the
chosen target because it is widely available and is the most practical way to
test V8's Wasm GC behavior.

This is **not** a new backend in the codegen sense. Twinkle should keep
emitting the same Wasm it emits today. The intended new piece is a host-side
runner that instantiates and executes that Wasm module under Node instead of
Wasmtime.

The key refinement is scope:
- a **benchmark-only runner** looks quite feasible and is the first target
- **full `twk run` parity** is a second question, because the broader host ABI
  includes imports that construct fresh Wasm GC values on the host side

So the plan should not start from the assumption that full parity is easy, but
it also should not treat the whole effort as high-risk from the beginning. The
actual import surface shows that the initial benchmark comparison only needs a
small subset of the current host ABI.

## Core question to answer

Using the same compiled Twinkle output:

- If Node.js is much faster on large-vector read-heavy workloads, that is strong
  evidence that Wasmtime GC is the main bottleneck.
- If Node.js is similar, then the remaining cost is more likely inherent to the
  current Twinkle runtime representation or emitted Wasm access shape.
- If Node.js is worse, then Wasmtime is probably not the limiting factor.

## Non-goals

Do **not** turn this into:
- a second codegen pipeline
- a JS-specific runtime representation
- a JS-only language mode
- a large abstraction over multiple hosts
- a Deno + Node dual-maintenance project

Do **not** change Twinkle semantics to accommodate the Node runner.

Do **not** add host features beyond what the current runtime ABI already needs
for existing `twk run` behavior and benchmark execution.

## Minimal scope

The runner only needs to do enough to instantiate and run the Wasm that current
stage0 emits.

### Required capabilities

The required host surface depends on which target we are trying to run.

#### Benchmark-only target

The current vector benchmarks compile down to a very small host import subset:
- stdout / stderr output (`print`, `println`, `error`, `eprint`, `eprintln`)
- float formatting (`f64_to_string`)

That means the initial comparison runner does **not** need fs, argv, env, or
`Result`-returning imports just to answer the Wasmtime-vs-V8 question for the
persistent-vector workloads.

#### Full `twk run` parity target

The broader current host ABI, as exercised by `boot/tests/main.tw`, includes:
- stdout / stderr output (`print`, `println`, `error`, `eprint`, `eprintln`)
- float formatting (`f64_to_string`)
- argv (`args`)
- env lookup (`env`)
- cwd (`cwd`)
- process exit (`exit`)
- file reads (`read_file`)
- file writes (`write_file`)
- directory listing (`list_dir`)
- existence checks (`exists`)
- string-to-number parsing (`parse_float`)

The full Wasmtime host implementation also supports additional imports such as:
- `write_bytes`
- `mkdirp`
- `parse_int`

The important distinction is that some of these are **easy imports** and some
are **GC-constructing imports**.

Easy imports mostly consume scalars or existing refs:
- `print`, `println`, `error`, `eprint`, `eprintln`
- `exists`
- `exit`
- likely `parse_float` / `parse_int`

GC-constructing imports must allocate fresh Twinkle runtime values as Wasm GC
objects on the host side, including:
- `f64_to_string` → runtime string
- `args` / `env` / `list_dir` → runtime arrays of runtime strings
- `cwd` → runtime string
- `read_file` → runtime `Result`-style variant wrapping runtime arrays

So the real question is not whether a benchmark-only Node runner is feasible —
it almost certainly is — but whether the **full current host ABI** is feasible
without changing Twinkle semantics or emitted Wasm.

### Nice-to-have but not required initially

- exact CLI parity with Wasmtime runner
- source maps
- fancy trap formatting
- debugger integration
- Deno support

## Success criteria

We consider this effort successful if it gives us a reliable runtime comparison
for existing Twinkle programs and benchmarks with no codegen changes.

Specifically:
- it can run the vector microbench programs already in `benches/tw/`
- results are easy to compare with Wasmtime on the same machine
- if feasible, it can later run the broader current stage0 compiler output using
  the existing host ABI
- if full parity is not feasible, the blocker is identified clearly

We also consider the effort successful if it produces a clear negative answer
for **full host parity**. If Node can run the benchmark subset but cannot
satisfy the broader GC-shaped host ABI without changing Twinkle's runtime
contract, that is still valuable and still answers the original performance
question for the vector workloads.

## Implementation shape

There are two acceptable entrypoints. Prefer the first because it is smaller.

### Option A: standalone Node runner script

Add a script such as:
- `tools/run_wasm_node.mjs`

Workflow:
1. `twk build input.tw -o out.wasm`
2. `node tools/run_wasm_node.mjs out.wasm [program args...]`

Advantages:
- no immediate CLI churn
- easier to iterate
- explicitly keeps the comparison runner separate from main UX

### Option B: integrated CLI backend switch

Example shape:
- `twk run --backend wasmtime file.tw`
- `twk run --backend node file.tw`

This is nicer long-term but should only come after the standalone runner works.

## Recommended implementation order

### Phase 1: benchmark-only feasibility spike

Goal:
- prove we can load emitted Wasm under Node
- implement the small host subset needed by the vector benchmarks
- answer the Wasmtime-vs-V8 question for the current persistent-vector repros

Known benchmark import subset:
- `print`
- `println`
- `error`
- `eprint`
- `eprintln`
- `f64_to_string`

Checklist:
- build `.wasm` from a vector benchmark module
- instantiate it in Node
- implement string decoding for print/error paths
- implement at least one GC-constructing import (`f64_to_string`) that returns a
  runtime string
- run the vector benchmark modules successfully

Exit criteria:
- if benchmark modules run, proceed to performance comparison immediately
- if they do not, stop and summarize the concrete blocker

### Phase 2: collect benchmark comparison data

Goal:
- run the existing vector benchmark modules under Node and compare with Wasmtime

Primary workloads:
- `benches/tw/vector_get_tiny.tw`
- `benches/tw/vector_get_1025.tw`
- `benches/tw/vector_get_deep.tw`
- `benches/tw/vector_get_deep_tail_only.tw`
- `benches/tw/vector_iter_sum.tw`

At this phase, correctness matters more than polished UX.

The benchmark runner should avoid conflating runtime behavior with process
startup and one-time compilation cost. The comparison should therefore load the
same compiled `.wasm` once per measured session, warm up, and then execute it
multiple times in-process before summarizing median or representative runtime.

### Phase 3: full host-ABI feasibility

Goal:
- determine whether Node can support the broader current `twk run` host surface
- execute `boot/tests/main.tw` under Node if that broader surface is feasible

This phase covers imports such as:
- `args`
- `cwd`
- `env`
- `exists`
- `list_dir`
- `read_file`
- `write_file`
- `exit`
- `parse_float`

This is the phase most likely to hit ABI-shape or Wasm-GC-construction issues.
If it does, document that clearly rather than widening scope.

### Phase 4: collect broader comparative measurements

Run the same modules under:
- Wasmtime (`twk run` / current benchmark harness)
- Node.js runner

Capture both:
- total runtime
- normalized per-access cost for the vector probes

Measurement harness rules:
- do not spawn a fresh host process for every sample if avoidable
- do not include front-end compilation from `.tw` during the timed loop
- compile or load the module once, warm up, then measure repeated execution
- report clearly whether numbers include only execution or execution plus
  instantiation

## Repro plan

All comparison runs should be reproducible from a clean checkout.

### Build artifacts

For each benchmark / program:
1. compile once to `.wasm`
2. run that exact `.wasm` under both Wasmtime and Node

This avoids mixing codegen variance into runtime comparisons.

For performance measurement, prefer a harness that also mirrors the existing
Wasmtime execution-only benchmark style:
1. compile once to `.wasm`
2. load / compile the module once in the host runtime
3. perform warmup runs
4. measure repeated executions in-process

This avoids mixing host startup and one-time compilation variance into the core
runtime comparison.

### Target comparison workloads

#### Vector-focused probes

These are the highest-priority repro set because they motivated the work:

| Program | Purpose |
|---|---|
| `vector_get_tiny.tw` | tail-only small-vector baseline |
| `vector_get_1024.tw` | depth-1 boundary case |
| `vector_get_1025.tw` | depth-2 boundary case |
| `vector_get_deep.tw` | large-vector trie reads |
| `vector_get_deep_tail_only.tw` | large-vector tail-only reads |
| `vector_iter_sum.tw` | iterator traversal on large vector |

#### Boot compiler repro

If Phase 3 succeeds, also compare:
- `cargo run --release --bin twk -- run boot/tests/main.tw`
- equivalent Node-runner execution of the compiled module

This matters because the real question is not just synthetic vector behavior,
but whether the same runtime gap appears in the actual compiler workload.

### Measurement rules

To keep summaries credible:
- run both Wasmtime and Node on the **same machine**
- use the **same compiled `.wasm` artifact** where possible
- separate correctness validation from performance measurement
- measure multiple times and use a median or stable representative value
- report both total runtime and normalized units when meaningful
- state explicitly whether each timing includes only execution, or execution
  plus instantiation, or full process startup
- prefer the same measurement shape on both sides: load once, warm up, then run
  many times in-process

## What to summarize from the comparison

Every summary should answer these questions directly:

### 1. Did the module run correctly under Node?

For each workload, record:
- ran successfully / failed
- whether it was a benchmark-subset run or a full-host-parity run
- if failed, which import or runtime feature was missing
- whether the failure was a generic Node limitation or specifically a blocker in
  creating / returning Wasm GC runtime values
- whether the output matched Wasmtime

### 2. How did Node compare on large-vector reads?

This is the main performance question.

Minimum metrics to report:
- Wasmtime runtime
- Node runtime
- relative difference
- whether the gap is small, moderate, or large

### 3. Did Node narrow the large-vector penalty specifically?

Do not only compare totals. Also compare whether Node changes the shape of the
slowdown across:
- tiny vectors
- depth boundary vectors
- large tail-only reads
- large trie reads

The most important signal is whether Node materially reduces:
- `vector_get_deep`
- `vector_get_deep_tail_only`
- `vector_iter_sum`

### 4. What does that imply for next steps?

Use the following interpretation guide.

## Interpretation guide

### Outcome A: Node is much faster on large vectors

This suggests Wasmtime's Wasm GC implementation is the main bottleneck.

Implications:
- avoid overfitting Twinkle's runtime around Wasmtime-specific weakness
- keep runtime representation changes conservative
- prioritize alternate-host support as a practical execution path
- focus future runtime work on low-complexity wins, not major redesigns

### Outcome B: Node is only slightly faster or similar

This suggests the remaining cost is in the Twinkle runtime representation or the
emitted Wasm access shape more generally, not primarily in Wasmtime.

Implications:
- continue optimizing the runtime representation
- consider further GC-object-count reductions or compact representations
- a Node runner is still useful, but it does not change the optimization target

### Outcome C: Node is worse

This suggests Wasmtime is not the main issue and may already be competitive.

Implications:
- keep optimizing Twinkle's representation
- deprioritize host-runtime diversification as a performance lever

## Recommended summary format

When results are available, summarize them in this structure:

### Execution status

| Workload | Wasmtime | Node | Notes |
|---|---|---|---|
| vector_get_tiny | pass | pass | |
| vector_get_1025 | pass | pass | |
| vector_get_deep | pass | pass | |
| vector_get_deep_tail_only | pass | pass | |
| vector_iter_sum | pass | pass | |
| boot/tests/main.tw | pass | fail/pass | missing import / success note |

### Performance comparison

| Workload | Wasmtime | Node | Relative difference | Interpretation |
|---|---|---|---|---|
| vector_get_tiny | ... | ... | ... | baseline |
| vector_get_1025 | ... | ... | ... | depth boundary |
| vector_get_deep | ... | ... | ... | large trie reads |
| vector_get_deep_tail_only | ... | ... | ... | large tail-only reads |
| vector_iter_sum | ... | ... | ... | iterator at scale |

### Conclusion

A short conclusion should explicitly answer:
- Was the **benchmark-only** Node comparison feasible?
- Was **full current host parity** feasible?
- If the benchmark comparison worked, is Wasmtime likely the main GC bottleneck?
- Does the Node comparison change the persistent-vector optimization roadmap?
- Is the Node runner worth keeping as a maintained execution path?

## Suggested follow-up decisions after results

If Node clearly outperforms Wasmtime on the problematic workloads:
1. keep the Node runner in-tree
2. document it as an experimental runtime option
3. avoid large representation redesigns until we know how much was runtime-
   specific vs representation-specific
4. optionally add a lightweight benchmark command that runs the same `.wasm`
   under both hosts

If Node does not outperform Wasmtime materially:
1. keep the runner only if the maintenance cost is low
2. continue runtime representation work in `src/runtime/arr.rs`
3. investigate further GC-object-count reductions or more compact vector forms

## Current recommendation

Proceed with a **minimal standalone Node.js runner** first.

That keeps the project focused, gives a strong answer to the Wasmtime-vs-V8
question, and avoids prematurely committing to a larger multi-runtime strategy.
