# AWFY-style cross-language benchmark suite

An [are-we-fast-yet](https://github.com/smarr/are-we-fast-yet)-style suite that
runs the same benchmarks in **Twinkle**, **Node/JS (V8)**, and **Go**, then
prints a normalized timing table.

## Purpose: compiler perf gap-finding

The point is **not** an absolute cross-language ranking. It is to surface where
Twinkle's Wasm-GC codegen and runtime are slow relative to fast reference
implementations, so the gaps guide compiler optimization work. Node is the
closest apples-to-apples answer to "why is my Wasm slower than V8" (Twinkle's
output runs under a JS host); Go stands in for "what a decent native compiler
achieves".

## Running

```bash
make awfy                 # or: examples/awfy/run.sh
```

`run.sh` runs each language, checks that every benchmark's checksum agrees
across all three, and **fails the run if any disagree** before printing the
table. To run a single language directly:

```bash
target/twk run examples/awfy/twinkle/main.tw
node examples/awfy/node/main.mjs
(cd examples/awfy/go && go run -gcflags=all=-d=fmahash=1111111111111111 .)
```

There is no per-benchmark filter flag yet — to isolate one benchmark, comment
out the others in the three `main` files (`twinkle/main.tw`, `node/main.mjs`,
`go/main.go`).

## What each row means

`run.sh` emits one row per (language, benchmark):

```
lang   bench   iters   ms   checksum   us_per_op
```

- `iters` — number of timed iterations of `run(size)`.
- `ms` — wall time for those iterations.
- `checksum` — the integer `run(size)` returns; doubles as the anti-DCE sink and
  the correctness check. Identical across languages by construction.
- `us_per_op` — `ms * 1000 / iters`.

## The honest-baseline caveat

Node and Go use **native, mutable** stdlib arrays. Twinkle values are immutable;
array/record updates go through **persistent GC structures** (`Vector.set_at`,
record rebinding). So on the array-write-heavy benchmarks — **Sieve, Bounce,
NBody** (per-step `set_at`) and to a lesser extent Queens/Towers — a large gap is
**expected and is the point of the exercise**. Conversely, the allocation- and
recursion-heavy benchmarks (**List, Storage, Json**) show Twinkle's GC-struct
allocation is competitive.

## Floating point determinism

The cross-language checksum diff requires bit-identical float results. Two
things make that hold:

- **Go FMA is disabled** in `run.sh` via
  `-gcflags=all=-d=fmahash=1111111111111111`. Without it, Go fuses `a*b+c` into a
  single fused-multiply-add on arm64, which rounds differently than the separate
  multiply/add that V8 and Wasm perform, and Mandelbrot/NBody diverge by a ULP.
  Disabling it puts all three languages on identical strict IEEE-754 arithmetic —
  a *fairer* baseline, since none of them fuse.
- **NBody's energy** is scaled to an integer checksum by `round(energy * 1e8)`.
  The scale factor is small enough that a hypothetical last-bit energy difference
  could not flip the rounded integer, and large enough to be a meaningful check.

## Benchmarks and canonical config

Each benchmark exposes `run(size) -> Int` (returns the checksum) plus
`warmup`/`iters`/`size`/`expected` constants. The **same** `(warmup, iters,
size)` triple is used in all three languages. For the recursion/simulation
benchmarks, `size` is a repeat count, so it does not affect the checksum — only
the amount of timed work.

| bench | size | warmup | iters | expected | exercises |
|---|---|---|---|---|---|
| mandelbrot | 500 | 10 | 20 | 191 | pure `Float` loops, no heap |
| sieve | 5000 | 10 | 40 | 669 | boolean array, index writes (persistent `Vector<Bool>`) |
| queens | 1000 | 10 | 40 | 1000 | recursion + boolean guard arrays |
| permute | 300 | 10 | 20 | 8660 | recursion + int array swaps |
| towers | 200 | 10 | 20 | 8191 | recursion + stack pegs |
| list | 1000 | 10 | 40 | 499500 | enum cons-list build + traverse |
| bounce | 400 | 10 | 20 | 47174 | `Int` sim + persistent ball vector + LCG |
| storage | 120 | 10 | 20 | 27881 | GC-throughput nested-vector tree + LCG |
| nbody | 20000 | 5 | 20 | -16908926 | 5-body `Float` sim over `Vector<Body>` |
| json | 1000 | 20 | 100 | 25280 | hand-written recursive-descent parser (string/`Byte`) |

Determinism note: Bounce and Storage share AWFY's exact linear-congruential
PRNG (`seed = (seed*1309 + 13849) & 65535`, initial seed 74755), defined once in
`twinkle/bounce.tw` and reused by `twinkle/storage.tw`.

## Adding a benchmark

1. Add `<name>.{tw,mjs,go}` exposing `run`, `warmup`, `iters`, `size`,
   `expected` (the Go file also declares a `Bench{...}` value).
2. Register it in the three `main` files.
3. Run one language to obtain the checksum, paste it into all three `expected`
   (and the table above).
4. `make awfy` — it fails if the languages disagree.
