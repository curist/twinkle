# AWFY-style cross-language benchmark suite

An [are-we-fast-yet](https://github.com/smarr/are-we-fast-yet)-style suite that
runs the same benchmarks in **Twinkle**, **Node/JS (V8)**, **Go**, **Lua 5.4
hosted by Redbean**, and **LuaJIT**, then prints a normalized timing table.

## Purpose: compiler perf gap-finding

The point is **not** an absolute cross-language ranking. It is to surface where
Twinkle's Wasm-GC codegen and runtime are slow relative to fast reference
implementations, so the gaps guide compiler optimization work. Node is the
closest apples-to-apples answer to "why is my Wasm slower than V8" (Twinkle's
output runs under a JS host); Go stands in for "what a decent native compiler
achieves". Redbean's Lua 5.4 interpreter adds a compact dynamic-language
baseline, while LuaJIT shows what tracing compilation can extract from the same
Lua sources. Neither changes the suite's gap-finding purpose.

**Persistent-structure peers.** Node and Go use native *mutable* arrays, so on
the array-write-heavy benchmarks they answer a different question than "how good
is Twinkle's persistent `Vector`". For that, **Clojure** (persistent vectors — a
32-way trie) and **Racket** (treelists — an immutable RRB tree) run the
write-heavy subset (Sieve, Bounce, NBody) using the *same broad representation
family* as Twinkle's `Vector<T>`: indexed persistent collections with functional
update. They are the fair yardstick for those benchmarks — and after the MutVec
work Twinkle's persistent path now *leads* that yardstick (see the results
snapshot below).

## Running

```bash
make awfy                 # or: examples/performance/awfy/run.sh
```

`run.sh` runs each language, checks that every benchmark's checksum agrees
across all languages that implement it, and **fails the run if any disagree**
before printing the table. To run a single language directly:

```bash
target/twk run examples/performance/awfy/twinkle/main.tw
node examples/performance/awfy/node/main.mjs
(cd examples/performance/awfy/go && go run -gcflags=all=-d=fmahash=1111111111111111 .)
redbean.com -i examples/performance/awfy/lua/main.lua
AWFY_LANG=luajit luajit examples/performance/awfy/lua/main.lua
clojure -M examples/performance/awfy/clojure/main.clj      # Sieve, Bounce, NBody only
racket examples/performance/awfy/racket/main.rkt           # Sieve, Bounce, NBody only
```

Redbean, LuaJIT, Clojure, and Racket are **optional**. `run.sh` finds
`redbean.com` or `redbean` on `PATH`; set `REDBEAN=/path/to/redbean.com` to
select an explicit download. It finds `luajit` on `PATH`, with
`LUAJIT=/path/to/luajit` as an override. It skips optional runtimes that are
unavailable. Both Lua runtimes cover the full base suite from the same source
files. Clojure and Racket cover only the write-heavy subset (Sieve, Bounce,
NBody) in both persistent and unlocked (`*_mut`) forms, so the checksum diff
compares each benchmark across just the languages that emit a row for it.

Redbean documents `-i` as interpreter mode: it disables the web server and runs
the first positional argument as a Lua script. The runtime currently identifies
this bundled interpreter as Lua 5.4. See [Redbean's interpreter-mode
documentation](https://redbean.dev/#repl). A small compatibility module selects
Lua 5.4 native bit operations or LuaJIT's `bit` module; benchmark bodies and
workload constants remain shared.

To check only the Redbean integration smoke path:

```bash
examples/performance/awfy/test_lua.sh
```

There is no suite-wide per-benchmark filter flag yet. The Lua entry point accepts
`AWFY_BENCH=<name>` for focused checks; for other languages, comment out the
unwanted entries in their `main` files.

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

## Representative results (µs/op)

A single-session snapshot (2026-09-03, arm64 macOS; `us_per_op`, lower is
better). Absolute numbers are noisy across runs and machines — read the **ratios
and tiers**, not the digits. This is a gap-finder, not a leaderboard.

| bench | twinkle | node | go | racket | clojure | what it tells us |
|---|---|---|---|---|---|---|
| sieve | **15** | 20 | 6 | 636 (`_mut` 20) | 719 (`_mut` 430) | persistent `Vector<Bool>` beats Node and both peers' native escape hatches |
| bounce | 10 830 | 3 508 | 2 894 | 60 867 | 115 440 | persistent beats peer-persistent 5–11×; behind Node/Go on native |
| bounce_mut | 3 762 | — | — | 16 079 | 261 459 | Twinkle `@std.buffer` reaches the Node/Go native tier |
| nbody | 4 322 | 848 | 651 | 17 942 | 18 592 | float-codegen bound (not persistence) |
| nbody_mut | 2 631 | — | — | 13 575 | 37 156 | Buffer helps ~1.7×; residual gap is float codegen |
| mandelbrot | 138 701 | 16 929 | 23 383 | — | — | pure-float loop; the float-codegen gap in isolation |
| list | 11 | 5 | 10 | — | — | GC-struct allocation is competitive |
| json | 1 168 | 535 | 517 | — | — | recursive-descent parse (string/`Byte`) |
| queens / permute / towers / storage | 6 040 / 58 004 / 40 169 / 36 022 | 1 914 / 7 973 / 7 114 / 21 901 | 707 / 3 587 / 3 134 / 21 978 | — | — | recursion + GC throughput |

Reproduce with `make awfy`. The write-heavy headline: MutVec closed the
persistence tax on integer/bool kernels, so the persistent peers are now the ones
behind and the remaining Node/Go distance is compute/float codegen, not `set_at`.

## The honest-baseline caveat (largely closed for write-heavy kernels)

Node and Go use **native, mutable** stdlib arrays. Twinkle values are immutable;
array/record updates go through **persistent GC structures** (`Vector.set_at` /
the `arr[i] = v` rebinding sugar). Historically this cost a large gap on the
array-write-heavy benchmarks (**Sieve, Bounce, NBody**) — that gap was "the point
of the exercise."

**That gap has largely closed on the write-heavy integer/bool kernels.** The
compiler's sound-uniqueness / MutVec work now stores owned, locally-born
`Vector<Int>` / `Vector<Bool>` regions as unboxed mutable arrays and freezes them
back, so `set_at`/`arr[i]=v` in these loops no longer copies a trie per step. On
a representative run (below):

- **Sieve** (persistent `Vector<Bool>`) runs at **~15 µs/op** — it now *beats
  Node* (~20 µs), and beats both Clojure's and Racket's *native-array* escape
  hatches (Racket `sieve_mut` ~20 µs, Clojure ~430 µs). Only Go (~6 µs) is ahead.
  There is no persistence tax left to escape here; Twinkle no longer ships a
  Sieve `*_mut` variant.
- **Bounce** (persistent ball `Vector`) runs at **~10.8 ms/op** — **5–11× faster
  than the persistent peers** (Racket ~61 ms, Clojure ~115 ms), though still
  ~3–3.7× behind Node/Go's native arrays.

So the write-heavy gaps are no longer "the cost of persistence." The persistent
peers (Clojure/Racket) are now the ones far behind, and the remaining distance to
Node/Go is compute/codegen, not `set_at`. The allocation/recursion benchmarks
(**List, Storage, Json**) remain a GC-throughput signal, and the pure-float loops
(**Mandelbrot, NBody**) are float-codegen bound — a different lever entirely.

## The unlocked / native tier (`*_mut`)

Each persistent-write language has an escape hatch into native mutable storage.
The `*_mut` benchmarks exercise it (same checksums enforced), so the table can
show what manual mutable storage still buys *on top of* the now-fast persistent
path:

| language | escape hatch used |
|---|---|
| Twinkle | `@std.buffer` — manually-managed linear memory with u8/i64/f64 views + `free()` |
| Clojure | Java primitive arrays (`boolean-array` / `long-array` / `double-array`) |
| Racket | mutable `vector` and unboxed `flvector` |

What the numbers show now that persistent storage is fast:

- **Sieve no longer needs an escape hatch — dropped for Twinkle.** The
  `@std.buffer` version was *slower* than the persistent `Vector<Bool>` (~21 µs vs
  ~15 µs): one byte-per-flag linear-memory buffer with per-access `get_u8`/
  `set_byte` calls loses to the unboxed persistent leaves. Buffer bought nothing,
  so `sieve_mut` was removed from the Twinkle suite. (Clojure/Racket keep their
  `sieve_mut` rows as the native-array comparison anchor — which Twinkle's
  *persistent* sieve beats.)
- **Bounce still gains from Buffer.** `bounce` ~10.8 ms → `bounce_mut` ~3.8 ms
  (~2.9×), reaching Node/Go's native tier (~2.9–3.5 ms). Kept.
- **NBody still gains modestly.** `nbody` ~4.3 ms → `nbody_mut` ~2.6 ms (~1.7×).
  NBody is *compute*-bound — the residual ~3× gap to Node is float codegen and
  per-access `get_f64`/`set_f64` overhead, not persistence. Kept.
- **Permute exposes recursive aggregate threading.** `permute` ~60.6 ms/op →
  `permute_mut` ~4.9 ms/op (~12×). The ordinary version recursively returns a
  `PState` containing a persistently updated `Vector<Int>` and scalar count; the
  unlocked version keeps six integers in Buffer and returns only the count. This
  is evidence for an owned-specialized mutable ABI that carries private vector
  storage through recursive calls and materializes only if the vector escapes.
- **Clojure's idiomatic array ports do *not* beat its persistent vectors**
  (`bounce_mut` is actually slower). Reaching the native league needs aggressive
  primitive-type discipline (`*unchecked-math*`, `^long`/`^double`, no boxing
  across closures) that ordinary array code doesn't get for free — a reminder that
  "drop to arrays" is not automatically fast.

Takeaway for Twinkle: the persistent `Vector` is now *faster* than peer
persistent collections on write-heavy integer/bool kernels, and `@std.buffer` is
no longer a general workaround for the persistence tax — it earns its place only
where it still wins (Bounce's native-tier reach, NBody's compute loop) or where
GC arrays fundamentally can't go. Buffer's durable value — FFI, `SharedArrayBuffer`
/ cross-Worker transport, truly-contiguous hottest-loop codecs — and the forward
work to make `Vector<Byte>` ↔ `Buffer` crossings cheap are tracked in
[`docs/plans/vector-byte-buffer-interop.md`](../../../docs/plans/vector-byte-buffer-interop.md).
The open default-path compiler lever is typed/specialized `Vector` reads for the
sort/`order_by` read-wall; float codegen is the lever for the compute-bound
cases.

## Floating point determinism

The cross-language checksum diff requires bit-identical float results. Two
things make that hold:

- **Go FMA is disabled** in `run.sh` via
  `-gcflags=all=-d=fmahash=1111111111111111`. Without it, Go fuses `a*b+c` into a
  single fused-multiply-add on arm64, which rounds differently than the separate
  multiply/add that V8 and Wasm perform, and Mandelbrot/NBody diverge by a ULP.
  Disabling it puts the full-suite languages on identical strict IEEE-754
  arithmetic — a *fairer* baseline, since none of them fuse.
- **NBody's energy** is scaled to an integer checksum by `round(energy * 1e8)`.
  The scale factor is small enough that a hypothetical last-bit energy difference
  could not flip the rounded integer, and large enough to be a meaningful check.

## Benchmarks and canonical config

Each benchmark exposes `run(size) -> Int` (returns the checksum) plus
`warmup`/`iters`/`size`/`expected` constants. The **same** `(warmup, iters,
size)` triple is used in every full-suite language. For the recursion/simulation
benchmarks, `size` is a repeat count, so it does not affect the checksum — only
the amount of timed work.

| bench | size | warmup | iters | expected | exercises |
|---|---|---|---|---|---|
| mandelbrot | 500 | 10 | 20 | 191 | pure `Float` loops, no heap |
| sieve | 5000 | 10 | 40 | 669 | boolean array, index writes via `.set_at` (persistent `Vector<Bool>`) |
| sieve_direct | 5000 | 10 | 40 | 669 | same as `sieve` but with `flags[k] = false` rebinding sugar — confirms the sugar gets the same unboxed-MutVec treatment (Twinkle-only) |
| queens | 1000 | 10 | 40 | 1000 | recursion + boolean guard arrays |
| permute | 300 | 10 | 20 | 8660 | recursion + int array swaps |
| permute_mut | 300 | 10 | 20 | 8660 | unlocked tier: recursive Buffer swaps with a separately threaded scalar count (Twinkle only) |
| towers | 200 | 10 | 20 | 8191 | recursion + stack pegs |
| list | 1000 | 10 | 40 | 499500 | enum cons-list build + traverse |
| bounce | 400 | 10 | 20 | 47174 | `Int` sim + persistent ball vector + LCG |
| storage | 120 | 10 | 20 | 27881 | GC-throughput nested-vector tree + LCG |
| nbody | 20000 | 5 | 20 | -16908926 | 5-body `Float` sim over `Vector<Body>` |
| json | 1000 | 20 | 100 | 25280 | hand-written recursive-descent parser (string/`Byte`) |
| sieve_mut | 5000 | 10 | 40 | 669 | unlocked tier: native mutable storage — **Clojure/Racket only** (Twinkle's `@std.buffer` version was dropped: its persistent `sieve` is faster) |
| bounce_mut | 400 | 10 | 20 | 47174 | unlocked tier: native mutable storage (Twinkle `@std.buffer`) |
| nbody_mut | 20000 | 5 | 20 | -16908926 | unlocked tier: native mutable storage (Twinkle `@std.buffer`) |

The `*_mut` variants exist only where a persistent or aggregate-threaded baseline
has a useful unlocked comparison — Node/Go/LuaJIT already use mutable arrays or
tables, so their base rows are the native-tier reference. Twinkle emits
`permute_mut`, `bounce_mut`, and `nbody_mut` (where Buffer wins), but not
`sieve_mut` (where its persistent path already wins); Clojure and Racket retain
their write-heavy unlocked rows.

Determinism note: Bounce and Storage share AWFY's exact linear-congruential
PRNG (`seed = (seed*1309 + 13849) & 65535`, initial seed 74755), defined once in
`twinkle/bounce.tw` and reused by `twinkle/storage.tw`.

## Adding a benchmark

1. Add `<name>.{tw,mjs,go,lua}` exposing `run`, `warmup`, `iters`, `size`,
   `expected` (the Go file also declares a `Bench{...}` value).
2. Register it in the four full-suite `main` files.
3. Run one language to obtain the checksum, paste it into every `expected`
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
