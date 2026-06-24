# Twinkle `Vector` vs Racket `treelist`

Cross-language microbenchmarks comparing Twinkle's `Vector<Int>` against
[Racket's `treelist`](https://docs.racket-lang.org/reference/treelist.html) — the
RRB-tree sequence that backs **Rhombus's `[a, b, c]` list type**. Both are 32-way
RRB structures (`BITS = 5`), so this isolates the design differences:

- **Twinkle** is an RRB trie **with a tail buffer** (Clojure/Scala lineage) →
  amortized-O(1) end push/pop, plus an unboxed `PVecI64` family for `Vector<Int>`.
- **Racket treelist** is a **pure RRB tree, no tail** → end ops are O(log₃₂ N),
  but it has long-shipped relaxed concat / `drop` / `take` / sublist.

## What this does and does not measure

Twinkle runs as AOT WebAssembly on Deno/V8; Racket treelist runs on Racket CS.
Absolute milliseconds therefore compare **two whole stacks**, not data structures
in a vacuum. Trust:

1. the **scaling curve** (ms per doubling of N) — a property of the algorithm, and
2. the **relative op cost within each stack**.

Do **not** read the absolute ratio as "language X is faster".

## Running

```bash
boot/bench/racket/run.fish      # side-by-side table: N | twk ms | racket ms | racket/twk
```

Needs `target/twk` (run `make bundle-cli` first) and `racket` on PATH. Each
workload runs one discarded warmup pass + 5 timed passes per side, keeping the
**min** ms per N (both V8 and Racket CS need warmup, so this is the fair
protocol). The `racket/twk` column is `racket_ms / twk_ms`: `< 1.0` means Racket
is faster, `> 1.0` means Twinkle is faster.

## Workloads

Each `*.rkt` mirrors the matching `boot/bench/*.tw` exactly — same `N` values,
build phase outside the timed region, same sink to defeat dead-code elimination.

| File | Workload | treelist op |
|---|---|---|
| `concat_prepend` | `acc = [i] ++ acc` (right-operand accumulator) | `(treelist-append (treelist i) acc)` |
| `concat_append`  | `acc = acc ++ [i]` (left-base) | `(treelist-append acc (treelist i))` |
| `append_push`    | `acc = acc.append(i)` (one element at a time) | `(treelist-add acc i)` |
| `get_regular`    | N strided `v[idx]` on an append-built vector | `(treelist-ref v idx)` |
| `get_relaxed`    | N strided `v[idx]` on a concat-built (17-elem chunk) vector | `(treelist-ref v idx)` |
| `slice_dropfirst`| `acc = acc.slice(1, len)` (dequeue / left-drop) | `(treelist-drop acc 1)` |

## Findings (2026-06-24, Racket CS v9.2)

- **append_push / concat_append:** Twinkle wins (up to ~3.3× @32k) — the tail
  buffer pays off where the pure-RRB treelist must descend the right spine.
- **concat_prepend / get_relaxed:** roughly even, both near-linear — Twinkle's
  recently-landed RRB concat and relaxed-node navigation behave correctly.
- **get_regular:** Racket ~1.6× faster (native CS vs wasm/V8), both linear.
- **slice_dropfirst:** both O(log N) per op (loop is O(N log N), not quadratic).
  The general `slice` originally folded the tail into the trie and re-split it on
  every call, leaving Twinkle ~14× behind `treelist-drop`. After adding a
  left-drop fast path to `slice` (trim only the left spine, share the tail) the
  gap is ~2×, which is essentially the `get_regular` runtime baseline — i.e. the
  algorithmic deficit is gone. The fast path covers the `end == len` family
  (`drop_first`, `drop`, `slice(k, len)`).
- **take (`slice(0, n)` right-trim):** had the same fold tax (~14× behind
  `treelist-take`). A right-trim fast path (`start == 0`: truncate the tail when
  `end` reaches it, else trim the trie right and drop the tail — never fold) now
  makes `take` ~1.45× *faster* than `treelist-take` and on par with Twinkle's
  `drop_last` op. Mid-range one-shot `slice(a, b)` (both ends interior) still
  uses the general fold path.
