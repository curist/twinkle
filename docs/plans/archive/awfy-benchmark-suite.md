# AWFY-style cross-language benchmark suite for Twinkle

**Date:** 2026-07-01
**Status:** Design approved, pending implementation plan

## Goal

Stand up an [are-we-fast-yet](https://github.com/smarr/are-we-fast-yet)-style
benchmark suite whose primary purpose is **compiler perf gap-finding**: surface
where Twinkle's Wasm-GC codegen and runtime are slow relative to fast reference
implementations, so that gaps guide compiler optimization work. Absolute
cross-language rankings matter less than isolating diagnosable hotspots.

## Reference implementations

- **Node/JS (V8)** — fast JIT, and Twinkle targets Wasm run under a JS host, so
  it is the closest apples-to-apples answer to "why is my Wasm slower than V8".
- **Go** — AOT-compiled, GC'd; a stand-in for "what a decent native compiler
  achieves".

Both are already used by `examples/performance/crypto-bench`, so the tooling exists.

## Scope

First cut: all 9 AWFY micros plus the Json macro.

Included: Mandelbrot, NBody, Bounce, Sieve, Queens, Towers, Permute, Storage,
List, Json.

Explicitly out of scope (first cut): the OO-heavy macros DeltaBlue, Richards,
CD, Havlak — a faithful port requires re-expressing class hierarchies and
virtual dispatch as tagged unions / capability records, which is significant
effort and raises fairness questions with low diagnostic payoff. Also out of
scope: ReBench integration — a plain `run.sh` is sufficient for gap-finding.

## Approach: per-benchmark files, per-language directories (hybrid)

Chosen over (A) a crypto-bench-style monolith-per-language and (B) a fully
AWFY-faithful shared harness. The hybrid keeps the repo's proven `run.sh` + TSV
+ `sink` machinery while giving **per-benchmark isolation** (run just Mandelbrot
across all languages and drill in — exactly what gap-finding wants) and adds a
**cross-language checksum diff** that keeps the numbers honest.

### Directory layout

```
examples/performance/awfy/
  README.md
  run.sh                 # orchestrates all langs, normalizes TSV, diffs checksums
  twinkle.toml           # project root for the .tw files
  twinkle/
    harness.tw           # loop, timing (@std.date), TSV emit, checksum compare
    main.tw              # registers all benchmarks, runs the suite
    mandelbrot.tw ... json.tw   # one file per benchmark
  node/
    harness.mjs
    main.mjs
    mandelbrot.mjs ... json.mjs
  go/
    harness.go
    main.go
    mandelbrot.go ... json.go
```

## Contracts

### Per-benchmark contract

Each benchmark file exposes:

- `run(size: Int) Int` — does the work and returns an integer **checksum**
  derived from the result (e.g. a folded/summed final state). This value is
  simultaneously the anti-DCE `sink` and the correctness checksum.
- a declared `expected` checksum constant for the standard problem size.

### Harness / runner contract

The per-language runner iterates the registered benchmarks. For each:

1. run `warmup` iterations (results discarded),
2. time `iters` iterations of `run(size)` — `@std.date` in Twinkle,
   `performance.now()` in Node, `time.Now()` in Go,
3. assert every iteration's checksum `== expected`; fail loudly on mismatch,
4. emit one TSV row: `lang\tbench\titers\tms\tchecksum`
   (the crypto-bench row shape, with `sink` renamed to `checksum`).

### Methodology

- Each benchmark has a fixed `(warmup, iters, size)` triple defined in **one
  shared table**, identical across all three languages, so the comparison is
  apples-to-apples. AWFY's own sizes are the starting values; tune each so a
  timed run lands roughly in the 50–500 ms range.
- Warmup matters because both the Node baseline and Twinkle's emitted Wasm run
  under V8's JIT; steady-state is the quantity of interest.
- `run.sh` extends crypto-bench's `normalize` to also collect each
  `(bench, checksum)` and **fail the run if checksums disagree across
  languages** before printing the timing table. A faithful-but-wrong port must
  not silently produce meaningless timings.

## Benchmark ports

The recurring theme: AWFY's mutable object graphs become immutable records +
rebinding; in-place array mutation becomes persistent `Vector` rebinds, or —
where hot — a mutable accumulator threaded through return values.

| Benchmark | Exercises | Twinkle port note |
|---|---|---|
| Mandelbrot | pure `Float` loops, no heap | Direct port. Cleanest numeric-codegen diagnostic. |
| NBody | `Float` math + fixed body array | Bodies as `Vector<Body>` of records; each step rebinds. Watch per-step Vector copy cost. |
| Bounce | `Float` + small mutable ball array + RNG | Balls as records in a `Vector`; port AWFY's deterministic PRNG exactly. |
| Sieve | boolean array, index writes | `Vector<Bool>` with persistent `set` — a likely Twinkle hotspot vs native arrays; that gap is a wanted signal. |
| Queens | recursion + boolean arrays | Recursive; boolean rows/diagonals as Vectors. |
| Towers | recursion + linked stack | Disk stack as a tagged union or `Vector`; recursion-heavy. |
| Permute | recursion + int array | Direct recursive port. |
| Storage | allocation / GC stress | Builds a nested tree of Vectors; PRNG-driven. Pure GC-throughput probe. |
| List | linked-list build + traverse | `type List = { Nil, Cons(Int, List) }`; allocation + tail recursion. |
| Json | string parsing | Hand-written recursive-descent parser over a fixed JSON string; string/`Byte` diagnostic. Reuse AWFY's fixed input. |

**Determinism:** the RNG-driven benchmarks (Bounce, Storage, and AWFY's
`Random`) must port AWFY's exact linear-congruential PRNG so all three languages
produce identical checksums — otherwise the cross-language checksum diff cannot
validate the ports.

## Integration

- `make awfy` target (mirroring `make bench`) shelling `examples/performance/awfy/run.sh`.
- `examples/performance/awfy/README.md` documenting the honest-baseline caveat: Node/Go
  stdlib arrays are native and mutable; Twinkle uses persistent GC structures,
  so gaps on array-write-heavy benchmarks (Sieve, Storage) are expected and are
  the point of the exercise.
- No compiler / `src` changes. These are standalone `.tw` programs run via
  `target/twk run`, so no `make bundle-cli` / `make stage2` rebuild is needed to
  edit or add a benchmark.

## Success criteria

- `examples/performance/awfy/run.sh` runs all 10 benchmarks in all three languages, prints a
  normalized TSV timing table, and fails if any cross-language checksum disagrees.
- Each benchmark is independently runnable so a single feature can be drilled
  into across languages.
- The suite reveals at least the expected persistent-structure gaps (Sieve,
  Storage) as concrete, reproducible numbers to guide compiler work.
