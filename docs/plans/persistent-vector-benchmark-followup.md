# Persistent vector benchmark follow-up

## Context

The stage0 Wasm backend now lowers `Vector<T>` to the persistent `PVec`
representation introduced in `src/runtime/arr.rs`.

This improved update-heavy workloads dramatically, especially persistent
`set`, but the first benchmark pass also exposed large regressions in
read-heavy workloads.

This note records:
- how the current benchmark setup works
- which regressions were observed
- how to reproduce them
- the current hypotheses
- likely follow-up directions

## Benchmark setup

We intentionally do **not** keep the old flat vector backend in-tree just for
benchmarking.

Instead, benchmarks compare two git refs directly using temporary worktrees.
That lets us compare:
- a baseline ref with the old flat COW vector backend
- a newer ref with `PVec`

### Current benchmark inputs

Twinkle benchmark programs live in:
- `benches/tw/vector_append_chain.tw`
- `benches/tw/vector_append_indirect.tw`
- `benches/tw/vector_collect_sum.tw`
- `benches/tw/vector_iter_sum.tw`
- `benches/tw/vector_get_sum.tw`
- `benches/tw/vector_set_chain.tw`

### Current benchmark scripts

#### End-to-end process timing

`scripts/bench_compare_refs.sh`

This script measures full process wall time for repeated:
- `twk run <bench.tw>`

So it includes:
- Twinkle compile-to-Wasm
- Wasm instantiation
- Wasm execution
- process startup overhead

Example:

```bash
scripts/bench_compare_refs.sh 795d1c8 HEAD 3
```

#### Execution-only timing

`scripts/bench_exec_compare_refs.sh`

This script is more useful for runtime analysis.

It:
- creates temporary worktrees for two refs
- copies the same benchmark inputs into both
- builds `bench_exec` in release mode in each worktree
- compiles each benchmark to Wasm once
- measures repeated execution of the already-built Wasm module

This removes most compile-time noise and focuses on Wasm runtime behavior.

Example:

```bash
scripts/bench_exec_compare_refs.sh 795d1c8 HEAD 3
```

### Bench driver

Execution-only timing uses:
- `src/bin/bench_exec.rs`

That binary uses:
- `twinkle::cli::build::build_wat`
- `twinkle::cli::run_wasm::build_engine`
- `twinkle::cli::run_wasm::execute_module`

so the measured execution path matches the normal stage0 Wasm runner.

## Important benchmarking note

All benchmark scripts use **Rust release builds**.

Specifically, they build:

```bash
cargo build --release
cargo build --release --bin bench_exec
```

and then run:
- `target/release/twk`
- `target/release/bench_exec`

So the benchmark numbers are from the optimized Rust host binary, not from a
debug build.

## Baseline ref

The most useful baseline for the persistent vector change is:
- `795d1c8` — just before `7eafa07`

That gives a clean comparison between:
- old flat COW vector behavior
- new persistent `PVec` behavior

## Observed results

The first meaningful execution-only comparison was:

```bash
scripts/bench_exec_compare_refs.sh 795d1c8 3e4019c 3
```

Representative result:

```text
benchmark                           before(s)   after(s)     delta   speedup
------------------------------------------------------------------------------
vector_append_chain.tw               0.017310   0.022982 +0.005672      0.75x
vector_append_indirect.tw            0.017304   0.022953 +0.005649      0.75x
vector_collect_sum.tw                0.007704   1.174760 +1.167056      0.01x
vector_iter_sum.tw                   0.012778   1.217107 +1.204329      0.01x
vector_get_sum.tw                    0.013124   1.224346 +1.211222      0.01x
vector_set_chain.tw                287.852068   2.150542 -285.701526    133.85x
```

## What these numbers seem to mean

### Clear win: persistent set

`vector_set_chain.tw` improved massively.

That matches the representation change:
- old backend: `set` copied the full array
- new backend: `set` path-copies only the affected trie spine/leaf

This is the result the persistent vector design was expected to improve.

### Clear regression: read-heavy workloads

Three workloads regressed by roughly two orders of magnitude:
- `vector_collect_sum.tw`
- `vector_iter_sum.tw`
- `vector_get_sum.tw`

The important pattern is that these three now cluster together.
That suggests the regression is mainly in **reading from vectors**, not in
vector construction.

## Current suspicion

The main suspected cause is that vector reads are no longer direct Wasm array
reads.

### Old behavior

Before `PVec`, vector access in the stage0 Wasm backend was effectively:
- `array.len`
- `array.get`

`emit_vector_get_intrinsic` used direct Wasm GC array operations.

### New behavior

After `PVec`, vector access goes through:
- `rt_arr__get`
- which may call `rt_arr__get_leaf`
- which may walk internal trie nodes
- plus `ref.cast`, `struct.get`, branchy tail/trie logic, and only then the
  final `array.get`

So a hot loop that used to perform a direct `array.get` per element now pays a
substantially larger constant factor.

## Why `collect` likely regressed too

`collect` still lowers through the builder path, so construction itself is not
necessarily the main problem.

But `vector_collect_sum.tw` immediately iterates over the collected vector.
If vector iteration lowers to repeated `len/get`, then the post-build read cost
can dominate the benchmark.

The fact that:
- `vector_collect_sum`
- `vector_iter_sum`
- `vector_get_sum`

all land in nearly the same range strongly supports this explanation.

## Likely current lowering problem

The main suspicion is:
- `for x in xs` is effectively implemented as repeated vector `get`
- rather than as a chunk/leaf-aware traversal over the persistent structure

If true, this means both:
- direct indexing, and
- iterator-style loops

share the same expensive read path.

## How to reproduce

These repros are intentionally documented but not expected to be run often,
since they take a long time.

### End-to-end comparison

```bash
scripts/bench_compare_refs.sh 795d1c8 3e4019c 3
```

### Execution-only comparison

```bash
scripts/bench_exec_compare_refs.sh 795d1c8 3e4019c 3
```

For a more stable result, increase the run count, for example:

```bash
scripts/bench_exec_compare_refs.sh 795d1c8 3e4019c 7
```

## What should be investigated next

### 1. Inspect vector loop lowering

Determine whether `for x in xs` lowers to repeated:
- `rt_arr__len`
- `rt_arr__get`

If yes, that is the first optimization target.

### 2. Add a fast vector iteration path

Potential options:
- lower vector loops to a dedicated traversal helper
- iterate leaf-by-leaf instead of element-by-element through `get`
- special-case tail-only vectors

This is likely the highest-value follow-up if read-heavy performance matters.

### 3. Add a cheaper read fast path

Potential options:
- inline a tail-only or small-vector path
- reduce `ref.cast` / helper-call overhead in the common case
- keep `rt_arr__get` for the general path but specialize common shapes

### 4. Re-check optimizer interaction

`append`-heavy source patterns may already be rewritten into builder-style or
other optimized forms. That means append benchmarks alone do not tell the whole
story about the persistent representation.

This is why the read-heavy benchmarks are especially useful: they expose costs
that survive optimization.

## Bottom line

The current evidence suggests:
- persistent vectors **successfully fix update-heavy workloads**
- persistent vectors currently cause **major regressions in read-heavy Wasm
  workloads**
- the most likely culprit is the shift from direct `array.get` to runtime
  `PVec` access on hot loop paths

The most promising next step is to optimize **vector iteration and read
lowering**, not to revisit persistent `set`.
