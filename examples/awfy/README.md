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

**Persistent-structure peers.** Node and Go use native *mutable* arrays, so on
the array-write-heavy benchmarks they answer a different question than "how good
is Twinkle's persistent `Vector`". For that, **Clojure** (persistent vectors — a
32-way trie) and **Racket** (treelists — an immutable RRB tree) run the
write-heavy subset (Sieve, Bounce, NBody) using the *same broad representation
family* as Twinkle's `Vector<T>`: indexed persistent collections with functional
update. They are the fair yardstick for those benchmarks.

## Running

```bash
make awfy                 # or: examples/awfy/run.sh
```

`run.sh` runs each language, checks that every benchmark's checksum agrees
across all languages that implement it, and **fails the run if any disagree**
before printing the table. To run a single language directly:

```bash
target/twk run examples/awfy/twinkle/main.tw
node examples/awfy/node/main.mjs
(cd examples/awfy/go && go run -gcflags=all=-d=fmahash=1111111111111111 .)
clojure -M examples/awfy/clojure/main.clj      # Sieve, Bounce, NBody only
racket examples/awfy/racket/main.rkt           # Sieve, Bounce, NBody only
```

Clojure and Racket are **optional** — `run.sh` skips them if the `clojure` /
`racket` commands are not on `PATH`. They cover only the write-heavy subset
(Sieve, Bounce, NBody) in both persistent and unlocked (`*_mut`) forms, so the
checksum diff compares each benchmark across just the languages that emit a row
for it.

There is no per-benchmark filter flag yet — to isolate one benchmark, comment
out the others in the `main` files.

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

The Clojure and Racket rows put a number on "how much of that gap is Twinkle vs
how much is persistence itself": on Sieve/Bounce/NBody, Twinkle lands in the
same order of magnitude as those mature persistent-collection runtimes (and
beats Clojure on Bounce), while all three sit far behind native mutable arrays.
So the write-heavy gaps are largely the cost of the persistent representation,
not a Twinkle-specific defect — the levers are typed/specialized `Vector`
representations, not micro-optimizing `set_at`.

## The unlocked / native tier (`*_mut`)

Every one of those languages has an escape hatch out of persistent structures
into native mutable storage. The `sieve_mut` / `bounce_mut` / `nbody_mut`
benchmarks exercise it, so the table shows both tiers side by side (same
checksums enforced):

| language | escape hatch used |
|---|---|
| Twinkle | `@std.buffer` — manually-managed linear memory with u8/i64/f64 views + `free()` |
| Clojure | Java primitive arrays (`boolean-array` / `long-array` / `double-array`) |
| Racket | mutable `vector` and unboxed `flvector` |

What the numbers show:

- **Twinkle's `@std.buffer` reaches the native league on write-bound kernels.**
  `bounce` 2024 ms → `bounce_mut` ~80 ms — matching Node's native array — and
  `sieve` 36 ms → ~1.8 ms. This is the concrete answer to "can Twinkle escape the
  persistence tax": yes, when you opt into manual linear memory.
- **NBody barely moves** (`nbody` 600 ms → `nbody_mut` 570 ms; Racket sees only a
  modest gain too). NBody is *compute*-bound, not storage-bound — the 5-body
  vector is tiny, so `set_at` copies were already cheap. Its remaining ~30× gap
  to Node is float codegen and per-access `get_f64`/`set_f64` call overhead, not
  persistence. Different lever entirely.
- **Clojure's idiomatic array ports do *not* beat its persistent vectors**
  (`bounce_mut` is actually slower). Clojure's persistent vectors are extremely
  optimized, while naive `long-array`/`double-array` code boxes at every loop
  boundary; reaching the native league needs aggressive primitive-type discipline
  (`*unchecked-math*`, `^long`/`^double` everywhere, no boxing across closures)
  that ordinary array code doesn't get for free. A useful reminder that "drop to
  arrays" is not automatically fast.

Takeaway for Twinkle: the persistent `Vector` is competitive with peer
persistent collections, and `@std.buffer` provides a real, native-speed path for
the write-bound cases that need it. The open compiler lever is typed/specialized
`Vector` representations (to narrow the default-path gap) and float codegen (for
the compute-bound cases), not `set_at` itself.

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
| sieve_mut | 5000 | 10 | 40 | 669 | unlocked tier: native mutable storage (Twinkle `@std.buffer`) |
| bounce_mut | 400 | 10 | 20 | 47174 | unlocked tier: native mutable storage |
| nbody_mut | 20000 | 5 | 20 | -16908926 | unlocked tier: native mutable storage |

The `*_mut` variants exist only for the three persistent-write languages
(Twinkle, Clojure, Racket) — Node/Go are already native, so their base
`sieve`/`bounce`/`nbody` rows *are* the native-tier reference.

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

## Attribution

The benchmark designs (Mandelbrot, NBody, Bounce, Sieve, Queens, Towers,
Permute, Storage, List, Json) are adapted from the
[are-we-fast-yet](https://github.com/smarr/are-we-fast-yet) suite (MIT licensed,
© Stefan Marr et al.). The implementations here are fresh ports written for this
comparison, not copies of the upstream sources; the NBody initial conditions are
the classic 5-body constants from the Computer Language Benchmarks Game. This
code is covered by the repository's top-level MIT `LICENSE`.
