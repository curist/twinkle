# Permute MutVec Hot-Path Overhead Follow-up

**Status:** Resolved on 2026-09-28 by removing redundant caller-side index
narrowing guards from the private MutVec ABI. The rebuilt ordinary benchmark
passed the recursive aggregate performance gate.

**Goal:** Remove enough residual codegen overhead from ordinary immutable AWFY
Permute to make its three-sample median no slower than `1.50×` LuaJIT, without
changing the benchmark algorithm, weakening bounds semantics, or introducing a
source-level mutable API.

## Evidence

The aggregate-storage objective is already met. The selected route is
`permute$mvagg → swap$mv → set_at$mv`; it performs flat
`mutvec_get_i64`/`mutvec_set_i64` operations, emits no freeze for the dead vector
field, and runs slightly faster than the manual `@std.buffer` oracle.

Three same-build samples measured these medians:

| row | median µs/op | ordinary ratio |
|---|---:|---:|
| Twinkle ordinary `permute` | 4869.31 | 1.000 |
| Twinkle `permute_mut` | 4948.20 | 0.984 |
| LuaJIT `permute` | 3206.45 | 1.519 |

The LuaJIT ceiling is 4809.68 µs/op, so the measured gap is 59.64 µs/op, or
about 1.24% above the gate. Because the oracle is not faster, further storage or
aggregate-ABI work is not justified by this result.

Current WAT exposes two plausible residual costs:

- `swap$mv` calls `set_at$mv` twice and returns the unchanged mutable handle;
- each read and write repeats an `Int → i32` range guard before the runtime
  mutable-vector operation;
- record explosion leaves numerous handle aliases and local-to-local moves in
  `permute$mvagg`, even though the handle has one physical identity.

The first hypothesis was confirmed. Changing private MutVec get/set indices from
`i32` to `i64` lets the runtime logical-length guard prove safe narrowing once;
the caller no longer emits its own `i32::MAX` guard. Ordinary Permute's median
fell from 4869.31 to 4349.85 µs/op while OOB trap probes remained intact, so no
helper inliner or broader optimizer change was needed.

## Constraints

- Preserve Twinkle's trapping behavior for negative, oversized, and
  out-of-bounds indices.
- Keep the optimization compiler-private and representation-driven.
- Do not add a general-purpose inliner merely to move this benchmark.
- Preserve persistent fallback for every route rejected by the aggregate or
  flat-handle verifiers.
- Measure with the same bundled compiler build and the existing AWFY harness.

## Investigation Plan

### Task 1: Isolate the residual cost

Add benchmark-only probes with the same recursion and checksum that vary one
dimension at a time:

1. current propagated helper graph;
2. equivalent inline `mutvec_get/set` shape with helper calls removed;
3. helper graph with statically proven small indices, retaining required runtime
   OOB checks but avoiding redundant `i64 → i32` range guards where legal;
4. scalar recursion with swaps removed, to establish the recursion/local-copy
   floor.

Inspect WAT for each probe and record median timings. Select a compiler change
only if one probe recovers the required margin repeatably.

### Task 2: Implement the smallest evidenced optimization

Prefer, in order:

1. a post-propagation cleanup that removes identity handle aliases and folds
   trivial return-the-receiver `$mv` wrappers at their dedicated clone sites;
2. representation-aware elimination of redundant index conversion guards when
   range is already proven on the same path;
3. a narrowly scoped flat-helper inliner only if the first two probes show that
   the call boundary itself is the remaining cost.

Add WAT regression tests for the exact removed shape and runtime tests proving
negative and out-of-bounds indices still trap. Follow red-green-refactor and
keep each optimization independently revertible.

### Task 3: Re-run the gate

Rebuild through the self-host fixed point, run the complete correctness suites,
and take three same-session AWFY samples. The change lands only when ordinary
`permute` remains no slower than `1.25× permute_mut` and reaches `≤1.50×`
LuaJIT. If the result remains within benchmark noise of the boundary, increase
the sample count rather than weakening the gate.
