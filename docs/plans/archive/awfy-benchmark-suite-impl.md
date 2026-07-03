# AWFY Benchmark Suite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up an are-we-fast-yet-style benchmark suite (9 AWFY micros + Json macro) in Twinkle, Node/JS, and Go, wired to a `run.sh` that prints a normalized timing table and fails on cross-language checksum disagreement — so codegen/runtime perf gaps become reproducible numbers.

**Architecture:** Hybrid layout — per-benchmark files inside per-language directories, orchestrated by a crypto-bench-style `run.sh`. Each benchmark exposes `run(size) -> Int` returning an integer checksum that doubles as the anti-DCE sink. A per-language harness runs warmup + timed iterations, asserts every checksum equals a declared `expected`, and emits one TSV row per benchmark. `run.sh` normalizes rows to `us_per_op` and diffs checksums across languages before printing timings.

**Tech Stack:** Twinkle (`target/twk run`, `@std.date`, `@std.math`, persistent `Vector`), Node ≥ current (ESM `.mjs`, `performance.now()`), Go (`go run`, `time.Now()`), bash + awk.

**Design source:** `docs/plans/awfy-benchmark-suite.md` (approved design). Reference implementations for the algorithms: the official are-we-fast-yet JavaScript sources at https://github.com/smarr/are-we-fast-yet (`benchmarks/JavaScript/`). This plan restates each algorithm concretely so the engineer does not need network access, but the AWFY JS is the source of truth for the exact numeric behavior and the expected checksums.

---

## Conventions used throughout

- **TSV row shape (every language, every benchmark):**
  `lang<TAB>bench<TAB>iters<TAB>ms<TAB>checksum`
  (crypto-bench's row with `sink` renamed to `checksum`.) `run.sh` appends the derived `us_per_op` column.
- **Checksum = sink.** `run(size)` returns an `Int` derived from the final state. The harness XOR/uses it so the optimizer cannot dead-code the work.
- **Shared config table (canonical).** Every language uses the *same* `(warmup, iters, size)` per benchmark. The canonical values live in this document (and in `examples/performance/awfy/README.md`); each language hard-codes the same literals in its per-benchmark file. `size` and `expected` disagreements are caught automatically by the checksum diff; `warmup`/`iters` are documented and kept in sync by hand.

### Canonical config table

| bench | size | warmup | iters | expected (checksum) |
|---|---|---|---|---|
| mandelbrot | 500 | 10 | 30 | 191 |
| nbody | 250000 | 5 | 20 | *(fill from node run)* |
| bounce | 1500 | 10 | 40 | *(fill from node run)* |
| sieve | 5000 | 10 | 40 | *(fill from node run)* |
| queens | 1000 | 10 | 40 | *(fill from node run)* |
| towers | 600 | 10 | 40 | *(fill from node run)* |
| permute | 1000 | 10 | 40 | *(fill from node run)* |
| storage | 1000 | 10 | 30 | *(fill from node run)* |
| list | 1000 | 10 | 40 | *(fill from node run)* |
| json | 1 | 20 | 100 | *(fill from node run)* |

> `size` here is the AWFY "inner iteration" count where AWFY parameterizes it that way (queens/towers/permute/list/bounce/storage/json repeat a fixed-shape unit `size` times); mandelbrot/sieve/nbody take a genuine problem size. Values chosen to land a timed batch roughly in 50–500 ms; **retune during Task N if a batch is wildly off** (adjust `iters`, keep `size` fixed so checksums stay comparable).

> **`expected` procedure:** the checksum is defined by the algorithm, not chosen. For every benchmark after mandelbrot, the plan's final step is: run the *node* implementation once, read the printed checksum, and paste that integer into all three languages' `expected` (and into the table above). Then the checksum diff proves the other two languages agree.

---

## File structure

```
examples/performance/awfy/
  README.md                 # honest-baseline caveat + canonical config table + how to run
  run.sh                    # orchestrates all langs, normalizes TSV, diffs checksums
  twinkle/
    twinkle.toml            # project root for the .tw files (so `use .mandelbrot` works)
    harness.tw              # Benchmark record + run_all: warmup, time, assert, TSV emit
    main.tw                 # imports each bench module, builds Vector<Benchmark>, runs
    mandelbrot.tw           # pub fn run(size) Int + pub warmup/iters/size/expected consts
    nbody.tw
    bounce.tw
    sieve.tw
    queens.tw
    towers.tw
    permute.tw
    storage.tw
    list.tw
    json.tw
  node/
    harness.mjs             # runBench(bench): warmup, time, assert, TSV emit
    main.mjs                # imports each bench, runs
    mandelbrot.mjs ... json.mjs
  go/
    go.mod                  # module examples/performance/awfy/go, go 1.21
    harness.go              # Bench struct + RunBench
    main.go                 # builds []Bench, runs
    mandelbrot.go ... json.go   # (same package main)
```

Notes:
- `twinkle.toml` lives in `examples/performance/awfy/twinkle/` so that its parent namespace is that directory and `main.tw` can import siblings as `use .mandelbrot`. (Confirmed pattern: `examples/aoc/*` uses `use .solution`.)
- Go files all share `package main` in one directory; `go run examples/performance/awfy/go` compiles the whole dir.
- Node uses ESM (`"type":"module"` is unnecessary since files are `.mjs`).

---

## Task 1: Scaffolding + harnesses (no benchmarks yet)

Establish the three harnesses and the orchestrator with a single trivial "smoke" benchmark so the full pipeline (run + normalize + checksum diff) is proven before any real port.

**Files:**
- Create: `examples/performance/awfy/twinkle/twinkle.toml`
- Create: `examples/performance/awfy/twinkle/harness.tw`
- Create: `examples/performance/awfy/twinkle/smoke.tw`
- Create: `examples/performance/awfy/twinkle/main.tw`
- Create: `examples/performance/awfy/node/harness.mjs`
- Create: `examples/performance/awfy/node/smoke.mjs`
- Create: `examples/performance/awfy/node/main.mjs`
- Create: `examples/performance/awfy/go/go.mod`
- Create: `examples/performance/awfy/go/harness.go`
- Create: `examples/performance/awfy/go/smoke.go`
- Create: `examples/performance/awfy/go/main.go`
- Create: `examples/performance/awfy/run.sh`

- [ ] **Step 1: Twinkle project root**

`examples/performance/awfy/twinkle/twinkle.toml`:
```toml
name = "awfy"
```

- [ ] **Step 2: Twinkle harness**

`examples/performance/awfy/twinkle/harness.tw`:
```tw
use @std.date

pub type Benchmark = .{
  name: String,
  warmup: Int,
  iters: Int,
  size: Int,
  expected: Int,
  run: fn(Int) Int,
}

fn print_row(name: String, iters: Int, ms: Float, checksum: Int) {
  println("twinkle\t${name}\t${iters}\t${ms}\t${checksum}")
}

fn run_one(b: Benchmark) {
  // Warmup: results discarded, but guard against DCE.
  w := 0
  warm_sink := 0
  for w < b.warmup {
    warm_sink = warm_sink ^ b.run(b.size)
    w = w + 1
  }
  if warm_sink == 0x7fffffffffffffff {
    println("unreachable ${warm_sink}")
  }

  start := date.now()
  i := 0
  checksum := 0
  for i < b.iters {
    checksum = b.run(b.size)
    i = i + 1
  }
  elapsed := date.now() - start

  if checksum != b.expected {
    error("${b.name}: checksum ${checksum} != expected ${b.expected}")
  }
  print_row(b.name, b.iters, elapsed, checksum)
}

pub fn run_all(benches: Vector<Benchmark>) {
  for b in benches {
    run_one(b)
  }
}
```

- [ ] **Step 3: Twinkle smoke benchmark**

`examples/performance/awfy/twinkle/smoke.tw`:
```tw
pub warmup := 1
pub iters := 1
pub size := 3
pub expected := 6

pub fn run(size: Int) Int {
  sum := 0
  i := 1
  for i <= size {
    sum = sum + i
    i = i + 1
  }
  sum
}
```

- [ ] **Step 4: Twinkle main**

`examples/performance/awfy/twinkle/main.tw`:
```tw
use .harness.{Benchmark}
use .harness
use .smoke

harness.run_all([
  Benchmark.{ name: "smoke", warmup: smoke.warmup, iters: smoke.iters, size: smoke.size, expected: smoke.expected, run: smoke.run },
])
```

- [ ] **Step 5: Run the Twinkle smoke and verify a TSV row**

Run: `target/twk run examples/performance/awfy/twinkle/main.tw`
Expected output (exactly one row; `ms` is a float that varies):
```
twinkle	smoke	1	<ms>	6
```
If you see `checksum ... != expected`, the harness math is wrong — fix before proceeding.

- [ ] **Step 6: Node harness**

`examples/performance/awfy/node/harness.mjs`:
```js
export function runBench(b) {
  let warmSink = 0;
  for (let w = 0; w < b.warmup; w++) warmSink ^= b.run(b.size);
  if (warmSink === 0x7fffffff) console.log("unreachable", warmSink);

  const start = performance.now();
  let checksum = 0;
  for (let i = 0; i < b.iters; i++) checksum = b.run(b.size);
  const elapsed = performance.now() - start;

  if (checksum !== b.expected) {
    throw new Error(`${b.name}: checksum ${checksum} != expected ${b.expected}`);
  }
  console.log(`node\t${b.name}\t${b.iters}\t${elapsed}\t${checksum}`);
}
```

- [ ] **Step 7: Node smoke + main**

`examples/performance/awfy/node/smoke.mjs`:
```js
export const warmup = 1, iters = 1, size = 3, expected = 6;
export function run(size) {
  let sum = 0;
  for (let i = 1; i <= size; i++) sum += i;
  return sum;
}
```

`examples/performance/awfy/node/main.mjs`:
```js
import { runBench } from "./harness.mjs";
import * as smoke from "./smoke.mjs";

const benches = [
  { name: "smoke", warmup: smoke.warmup, iters: smoke.iters, size: smoke.size, expected: smoke.expected, run: smoke.run },
];
for (const b of benches) runBench(b);
```

- [ ] **Step 8: Run the Node smoke**

Run: `node examples/performance/awfy/node/main.mjs`
Expected: `node\tsmoke\t1\t<ms>\t6`

- [ ] **Step 9: Go module + harness**

`examples/performance/awfy/go/go.mod`:
```
module awfy

go 1.21
```

`examples/performance/awfy/go/harness.go`:
```go
package main

import (
	"fmt"
	"time"
)

type Bench struct {
	Name     string
	Warmup   int
	Iters    int
	Size     int
	Expected int
	Run      func(int) int
}

func RunBench(b Bench) {
	warmSink := 0
	for w := 0; w < b.Warmup; w++ {
		warmSink ^= b.Run(b.Size)
	}
	if warmSink == 0x7fffffff {
		fmt.Println("unreachable", warmSink)
	}

	start := time.Now()
	checksum := 0
	for i := 0; i < b.Iters; i++ {
		checksum = b.Run(b.Size)
	}
	elapsedMs := float64(time.Since(start).Nanoseconds()) / 1e6

	if checksum != b.Expected {
		panic(fmt.Sprintf("%s: checksum %d != expected %d", b.Name, checksum, b.Expected))
	}
	fmt.Printf("go\t%s\t%d\t%g\t%d\n", b.Name, b.Iters, elapsedMs, checksum)
}
```

- [ ] **Step 10: Go smoke + main**

`examples/performance/awfy/go/smoke.go`:
```go
package main

func smokeRun(size int) int {
	sum := 0
	for i := 1; i <= size; i++ {
		sum += i
	}
	return sum
}

var smokeBench = Bench{Name: "smoke", Warmup: 1, Iters: 1, Size: 3, Expected: 6, Run: smokeRun}
```

`examples/performance/awfy/go/main.go`:
```go
package main

func main() {
	benches := []Bench{
		smokeBench,
	}
	for _, b := range benches {
		RunBench(b)
	}
}
```

- [ ] **Step 11: Run the Go smoke**

Run: `go run examples/performance/awfy/go`
Expected: `go\tsmoke\t1\t<ms>\t6`

- [ ] **Step 12: run.sh (orchestrate + normalize + checksum diff)**

`examples/performance/awfy/run.sh`:
```bash
#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../.."

raw="$(mktemp)"
trap 'rm -f "$raw"' EXIT

target/twk run examples/performance/awfy/twinkle/main.tw >> "$raw"
node examples/performance/awfy/node/main.mjs             >> "$raw"
go run examples/performance/awfy/go                       >> "$raw"

# Checksum agreement: for each bench, all langs must report the same checksum.
mismatch="$(awk -F '\t' '
  NF >= 5 {
    key = $2
    if (key in seen) {
      if (sum[key] != $5) bad[key] = 1
    } else {
      seen[key] = 1; sum[key] = $5
    }
  }
  END { for (k in bad) print k, "checksums disagree" }
' "$raw")"

if [ -n "$mismatch" ]; then
  echo "CHECKSUM MISMATCH:" >&2
  echo "$mismatch" >&2
  echo "--- raw rows ---" >&2
  sort -t$'\t' -k2,2 "$raw" >&2
  exit 1
fi

printf 'lang\tbench\titers\tms\tchecksum\tus_per_op\n'
awk -F '\t' 'NF >= 5 { printf "%s\t%s\t%s\t%s\t%s\t%.6f\n", $1, $2, $3, $4, $5, ($4 * 1000.0) / $3 }' "$raw" \
  | sort -t$'\t' -k2,2 -k1,1
```

- [ ] **Step 13: Make run.sh executable and run the full smoke pipeline**

Run:
```bash
chmod +x examples/performance/awfy/run.sh
examples/performance/awfy/run.sh
```
Expected: no CHECKSUM MISMATCH; a header row plus three `smoke` rows (go, node, twinkle) all with checksum `6`.

- [ ] **Step 14: Prove the checksum diff actually fails**

Temporarily change `expected`/`run` in `node/smoke.mjs` so its checksum differs (e.g. `return sum + 1;`), run `examples/performance/awfy/run.sh`, confirm it exits non-zero with `CHECKSUM MISMATCH`. Then revert the change and confirm it passes again. (Do not commit the broken state.)

- [ ] **Step 15: Commit**

```bash
git add examples/performance/awfy
git commit -m "awfy: scaffold benchmark harness + orchestrator with smoke bench

Three per-language harnesses (Twinkle/Node/Go) emitting the crypto-bench TSV
row shape, plus run.sh that normalizes to us_per_op and fails the run when a
benchmark's checksum disagrees across languages. A trivial 'smoke' benchmark
proves the pipeline end to end."
```

---

## Task 2: Mandelbrot (worked template for all remaining ports)

This task is the reference for every later benchmark: add `<bench>.tw`/`.mjs`/`.go`, register it in the three `main` files, run, and confirm the checksum agrees across all three languages.

**Algorithm (AWFY Mandelbrot, restated):** render an `size`×`size` mandelbrot bitmap, packing escape bits MSB-first into bytes; the checksum is the XOR of all packed bytes. Escape test: iterate `z = z² + c` up to 50 times; a point "escapes" (bit = 1) if `zr² + zi² > 4.0`. Pixel `(x,y)` maps to `cr = 2·x/size − 1.5`, `ci = 2·y/size − 1.0`. The inner recurrence, in order:
```
zr = zrzr - zizi + cr
zi = 2.0 * zr * zi + ci      // uses the just-updated zr
zrzr = zr * zr
zizi = zi * zi
```
For `size = 500` the checksum is `191`.

**Files:**
- Create: `examples/performance/awfy/twinkle/mandelbrot.tw`
- Create: `examples/performance/awfy/node/mandelbrot.mjs`
- Create: `examples/performance/awfy/go/mandelbrot.go`
- Modify: `examples/performance/awfy/twinkle/main.tw`, `examples/performance/awfy/node/main.mjs`, `examples/performance/awfy/go/main.go`

- [ ] **Step 1: Twinkle mandelbrot**

`examples/performance/awfy/twinkle/mandelbrot.tw`:
```tw
pub warmup := 10
pub iters := 30
pub size := 500
pub expected := 191

pub fn run(size: Int) Int {
  sum := 0
  byte_acc := 0
  bit_num := 0
  y := 0
  for y < size {
    ci := (2.0 * Int.to_float(y) / Int.to_float(size)) - 1.0
    x := 0
    for x < size {
      zr := 0.0
      zrzr := 0.0
      zi := 0.0
      zizi := 0.0
      cr := (2.0 * Int.to_float(x) / Int.to_float(size)) - 1.5
      z := 0
      not_done := true
      escape := 0
      for not_done && z < 50 {
        zr = zrzr - zizi + cr
        zi = 2.0 * zr * zi + ci
        zrzr = zr * zr
        zizi = zi * zi
        if zrzr + zizi > 4.0 {
          not_done = false
          escape = 1
        }
        z = z + 1
      }
      byte_acc = (byte_acc << 1) + escape
      bit_num = bit_num + 1
      if bit_num == 8 {
        sum = sum ^ byte_acc
        byte_acc = 0
        bit_num = 0
      } else if x == size - 1 {
        byte_acc = byte_acc << (8 - bit_num)
        sum = sum ^ byte_acc
        byte_acc = 0
        bit_num = 0
      }
      x = x + 1
    }
    y = y + 1
  }
  sum
}
```

> **Verify `Int.to_float` exists** with `grep -rn "fn to_float" boot/prelude boot/stdlib`. If the conversion spelling differs (e.g. `y.to_float()` or `float(y)`), use the spelling the prelude actually provides; the arithmetic is unchanged.

- [ ] **Step 2: Node mandelbrot**

`examples/performance/awfy/node/mandelbrot.mjs`:
```js
export const warmup = 10, iters = 30, size = 500, expected = 191;

export function run(size) {
  let sum = 0, byteAcc = 0, bitNum = 0, y = 0;
  while (y < size) {
    const ci = (2.0 * y / size) - 1.0;
    let x = 0;
    while (x < size) {
      let zr = 0.0, zrzr = 0.0, zi = 0.0, zizi = 0.0;
      const cr = (2.0 * x / size) - 1.5;
      let z = 0, notDone = true, escape = 0;
      while (notDone && z < 50) {
        zr = zrzr - zizi + cr;
        zi = 2.0 * zr * zi + ci;
        zrzr = zr * zr;
        zizi = zi * zi;
        if (zrzr + zizi > 4.0) { notDone = false; escape = 1; }
        z += 1;
      }
      byteAcc = (byteAcc << 1) + escape;
      bitNum += 1;
      if (bitNum === 8) { sum ^= byteAcc; byteAcc = 0; bitNum = 0; }
      else if (x === size - 1) { byteAcc <<= (8 - bitNum); sum ^= byteAcc; byteAcc = 0; bitNum = 0; }
      x += 1;
    }
    y += 1;
  }
  return sum;
}
```

- [ ] **Step 3: Go mandelbrot**

`examples/performance/awfy/go/mandelbrot.go`:
```go
package main

func mandelbrotRun(size int) int {
	sum, byteAcc, bitNum, y := 0, 0, 0, 0
	fsize := float64(size)
	for y < size {
		ci := (2.0*float64(y)/fsize) - 1.0
		x := 0
		for x < size {
			zr, zrzr, zi, zizi := 0.0, 0.0, 0.0, 0.0
			cr := (2.0*float64(x)/fsize) - 1.5
			z, notDone, escape := 0, true, 0
			for notDone && z < 50 {
				zr = zrzr - zizi + cr
				zi = 2.0*zr*zi + ci
				zrzr = zr * zr
				zizi = zi * zi
				if zrzr+zizi > 4.0 {
					notDone = false
					escape = 1
				}
				z++
			}
			byteAcc = (byteAcc << 1) + escape
			bitNum++
			if bitNum == 8 {
				sum ^= byteAcc
				byteAcc, bitNum = 0, 0
			} else if x == size-1 {
				byteAcc <<= (8 - bitNum)
				sum ^= byteAcc
				byteAcc, bitNum = 0, 0
			}
			x++
		}
		y++
	}
	return sum
}

var mandelbrotBench = Bench{Name: "mandelbrot", Warmup: 10, Iters: 30, Size: 500, Expected: 191, Run: mandelbrotRun}
```

- [ ] **Step 4: Register in all three mains**

In `examples/performance/awfy/twinkle/main.tw` add `use .mandelbrot` and append to the vector:
```tw
Benchmark.{ name: "mandelbrot", warmup: mandelbrot.warmup, iters: mandelbrot.iters, size: mandelbrot.size, expected: mandelbrot.expected, run: mandelbrot.run },
```
In `examples/performance/awfy/node/main.mjs` add `import * as mandelbrot from "./mandelbrot.mjs";` and append:
```js
{ name: "mandelbrot", warmup: mandelbrot.warmup, iters: mandelbrot.iters, size: mandelbrot.size, expected: mandelbrot.expected, run: mandelbrot.run },
```
In `examples/performance/awfy/go/main.go` append `mandelbrotBench,` to the `benches` slice.

- [ ] **Step 5: Verify each language independently produces checksum 191**

Run each and confirm the mandelbrot row's checksum column is `191`:
```bash
target/twk run examples/performance/awfy/twinkle/main.tw
node examples/performance/awfy/node/main.mjs
go run examples/performance/awfy/go
```
If Twinkle disagrees, the most likely cause is `<<` on `Int` vs JS 32-bit `<<`; because `byte_acc` never exceeds 8 bits here, 64-bit and 32-bit shifts agree — but confirm. If a language throws the `checksum != expected` error, the port is wrong; debug with systematic-debugging before moving on.

- [ ] **Step 6: Full pipeline + checksum agreement**

Run: `examples/performance/awfy/run.sh`
Expected: no mismatch; a `mandelbrot` row for each of go/node/twinkle, all checksum `191`, plus the smoke rows.

- [ ] **Step 7: Commit**

```bash
git add examples/performance/awfy
git commit -m "awfy: add mandelbrot benchmark across Twinkle/Node/Go

Pure-float escape-time render packing bits into a bytewise XOR checksum;
checksum 191 at size 500 agrees across all three languages."
```

> **Every remaining benchmark task follows Steps 1–7 of this task**: write the three files, register in the three mains, verify each language independently, then confirm `run.sh` agreement, then commit. Each task below gives the algorithm and the concrete Twinkle port; write the Node and Go versions as direct transliterations of the stated algorithm (Node is also your `expected`-checksum oracle).

---

## Task 3: Sieve

**Algorithm (AWFY Sieve of Eratosthenes):** a boolean array `flags` of length `size+1`, all `true`. For each `i` from 2..=size, if `flags[i]` is true, count it and mark every multiple `2i, 3i, ...` false. AWFY's checksum is the count of primes found. Repeat the whole sieve `size`… no — AWFY runs a fixed `5000`-length sieve and returns the prime count (`669` for length 5000). Here `size = 5000`, `run` does one sieve, checksum = prime count.

**Twinkle port note:** boolean array → `Vector<Bool>`; `flags[i] = false` → `flags = flags.set_at(i, false)` (persistent — this is the *wanted* array-write hotspot signal). Build the initial vector with a `collect` or an append loop.

**Files:** `examples/performance/awfy/{twinkle/sieve.tw, node/sieve.mjs, go/sieve.go}` + register in three mains.

- [ ] **Step 1: Twinkle sieve**

`examples/performance/awfy/twinkle/sieve.tw`:
```tw
pub warmup := 10
pub iters := 40
pub size := 5000
pub expected := 669

pub fn run(size: Int) Int {
  // flags[0..=size], index 0 and 1 unused for counting.
  flags: Vector<Bool> = collect i in range(size + 1) { true }
  count := 0
  i := 2
  for i <= size {
    if flags[i] {
      count = count + 1
      k := i + i
      for k <= size {
        flags = flags.set_at(k, false)
        k = k + i
      }
    }
    i = i + 1
  }
  count
}
```

> Verify `range`/`collect` produce a 0-based `Vector` of length `size+1`; `range(n)` yields `0..n-1`, so `range(size+1)` gives indices `0..size`. Confirm `collect` binds `i` correctly (value unused).

- [ ] **Step 2: Node sieve** — `flags = new Array(size+1).fill(true)`, same loops, `flags[k] = false`, return count. `expected` from this run.
- [ ] **Step 3: Go sieve** — `flags := make([]bool, size+1)` with a fill loop, same logic.
- [ ] **Step 4: Register in three mains.**
- [ ] **Step 5: Run each language; paste node's checksum into all three `expected` and the config table.** For length 5000 expect `669` (AWFY-known); if node prints something else, trust node and update.
- [ ] **Step 6: `examples/performance/awfy/run.sh` — confirm agreement.**
- [ ] **Step 7: Commit** `awfy: add sieve benchmark (persistent Vector<Bool> writes)`.

---

## Task 4: Queens

**Algorithm (AWFY Queens):** solve the 8-queens problem `size` times (fixed 8×8 board), using three boolean guard arrays — `freeRows[8]`, `freeMaxs[15]` (↗ diagonals, index `r+c`), `freeMins[15]` (↘ diagonals, index `c-r+7`) — via backtracking `placeQueen(column)`. AWFY's `queens()` returns a boolean (solved). Repeat `size` times; checksum = number of successful solves (all succeed, so = `size`). To make the checksum sensitive to correctness, fold the solved-boolean: `checksum = checksum * 2 + (solved ? 1 : 0)` per repeat is overkill; simpler: **checksum = count of repeats that produced a valid full placement.** Keep it identical across languages.

**Twinkle port note:** the three guard arrays are `Vector<Bool>`; each placement rebinds them. Because they're threaded through recursion and rebound on backtrack, pass them into a recursive helper and return the updated triple. Simplest faithful port: a recursive `place(row, free_rows, free_maxs, free_mins) Bool` that tries each column, using `set_at` to mark/unmark. Return whether a full solution was found (AWFY stops at the first solution).

**Files:** `examples/performance/awfy/{twinkle/queens.tw, node/queens.mjs, go/queens.go}` + mains.

- [ ] **Step 1: Twinkle queens**

`examples/performance/awfy/twinkle/queens.tw`:
```tw
fn get_row_column(free_rows: Vector<Bool>, free_maxs: Vector<Bool>, free_mins: Vector<Bool>, r: Int, c: Int) Bool {
  free_rows[r] && free_maxs[c + r] && free_mins[c - r + 7]
}

// Returns true if a full solution is found from column `c`.
fn place(free_rows: Vector<Bool>, free_maxs: Vector<Bool>, free_mins: Vector<Bool>, c: Int) Bool {
  if c == 8 {
    true
  } else {
    solved := false
    r := 0
    for !solved && r < 8 {
      if get_row_column(free_rows, free_maxs, free_mins, r, c) {
        fr := free_rows.set_at(r, false)
        fx := free_maxs.set_at(c + r, false)
        fn2 := free_mins.set_at(c - r + 7, false)
        if place(fr, fx, fn2, c + 1) {
          solved = true
        }
      }
      r = r + 1
    }
    solved
  }
}

pub warmup := 10
pub iters := 40
pub size := 1000
pub expected := 1000

pub fn run(size: Int) Int {
  count := 0
  n := 0
  for n < size {
    free_rows: Vector<Bool> = collect i in range(8) { true }
    free_maxs: Vector<Bool> = collect i in range(15) { true }
    free_mins: Vector<Bool> = collect i in range(15) { true }
    if place(free_rows, free_maxs, free_mins, 0) {
      count = count + 1
    }
    n = n + 1
  }
  count
}
```

> `fn2` avoids shadowing the `fn` keyword. Confirm `fn` is reserved and pick non-keyword local names.

- [ ] **Step 2: Node queens** (recursive `place` returning bool; same guard indexing). `expected` = `size` (1000).
- [ ] **Step 3: Go queens** (same; `[]bool` guards, recursion returns bool).
- [ ] **Step 4: Register in mains.**
- [ ] **Step 5: Run each; agree on checksum (expect 1000).**
- [ ] **Step 6: run.sh agreement.**
- [ ] **Step 7: Commit** `awfy: add queens benchmark (backtracking over persistent guards)`.

---

## Task 5: Permute

**Algorithm (AWFY Permute):** generate all permutations of `[0..n)` (AWFY uses n=6), incrementing a global `count` on each swap-out; checksum = total permutation count. AWFY:
```
permute(n):
  count += 1
  if n != 0:
    permute(n-1)
    for i in [n-1 .. 0]:
      swap(v, n-1, i); permute(n-1); swap(v, n-1, i)
```
Run over an int array `v` of length 6. `count` for n=6 is `8660`. Repeat `size` times; checksum = final count from the last repeat (deterministic, equals `8660`). Keep the exact AWFY recursion so the count matches.

**Twinkle port note:** `v` is `Vector<Int>`; swaps rebind via `set_at`. Thread `count` as a returned accumulator (avoid `Cell`). The recursion returns the running count and the (possibly reordered) vector: `permute(v, n, count) -> (Vector<Int>, Int)`. Twinkle tuples: return a record or a 2-tuple if supported; **check whether tuples exist** (`grep -rn "tuple\|(.*,.*):" ` or look for `.0`/`.1`). If no tuples, use a small record `.{ v: Vector<Int>, count: Int }`.

**Files:** `examples/performance/awfy/{twinkle/permute.tw, node/permute.mjs, go/permute.go}` + mains.

- [ ] **Step 1: Twinkle permute**

`examples/performance/awfy/twinkle/permute.tw`:
```tw
type PState = .{ v: Vector<Int>, count: Int }

fn swap(v: Vector<Int>, i: Int, j: Int) Vector<Int> {
  a := v[i]
  b := v[j]
  v.set_at(i, b).set_at(j, a)
}

fn permute(s: PState, n: Int) PState {
  st := PState.{ v: s.v, count: s.count + 1 }
  if n == 0 {
    st
  } else {
    st = permute(st, n - 1)
    i := n - 1
    for i >= 0 {
      st = PState.{ v: swap(st.v, n - 1, i), count: st.count }
      st = permute(st, n - 1)
      st = PState.{ v: swap(st.v, n - 1, i), count: st.count }
      i = i - 1
    }
    st
  }
}

pub warmup := 10
pub iters := 40
pub size := 1000
pub expected := 8660

pub fn run(size: Int) Int {
  count := 0
  rep := 0
  for rep < size {
    v: Vector<Int> = collect i in range(6) { 0 }
    result := permute(PState.{ v: v, count: 0 }, 6)
    count = result.count
    rep = rep + 1
  }
  count
}
```

> `expected` 8660 is AWFY-known for n=6; confirm against node.

- [ ] **Step 2: Node permute** (mutable array + module-scope count reset per repeat).
- [ ] **Step 3: Go permute** (same).
- [ ] **Step 4–7:** register, run, agree (expect 8660), run.sh, commit `awfy: add permute benchmark`.

---

## Task 6: Towers

**Algorithm (AWFY Towers of Hanoi):** move a stack of 13 disks across 3 pegs, counting moves; checksum = move count (`8191` for 13 disks). AWFY models pegs as linked `TowersDisk` nodes with a `size` and `next`, pushing/popping and asserting legality; the observable result is `movesDone`. A faithful-enough port that preserves the checksum: pegs as `Vector<Int>` stacks (disk sizes), `moveDisks(n, from, to, via)` recursion incrementing a threaded move counter. Repeat `size` times; checksum = moves from last run = `8191`.

**Twinkle port note:** three peg stacks as `Vector<Int>` (top = last element). `push` = `.append`, `pop` = read `xs[len-1]` then `xs.drop_last()` (`grep -rn "drop_last" boot/prelude` — it's a runtime builtin per project memory). Thread the three pegs + move count through the recursion via a record `TState`. Since only the move count feeds the checksum and the recursion structure is fixed (`2^13 - 1` moves), you may simplify to just counting moves recursively; **but** to exercise the stack rebinds (the point of Towers), keep the peg vectors and actually move disks.

**Files:** `examples/performance/awfy/{twinkle/towers.tw, node/towers.mjs, go/towers.go}` + mains.

- [ ] **Step 1: Twinkle towers**

`examples/performance/awfy/twinkle/towers.tw`:
```tw
type Pegs = .{ a: Vector<Int>, b: Vector<Int>, c: Vector<Int>, moves: Int }

fn peg(p: Pegs, which: Int) Vector<Int> {
  cond {
    which == 0 => p.a,
    which == 1 => p.b,
    _ => p.c,
  }
}

fn set_peg(p: Pegs, which: Int, v: Vector<Int>) Pegs {
  cond {
    which == 0 => Pegs.{ a: v, b: p.b, c: p.c, moves: p.moves },
    which == 1 => Pegs.{ a: p.a, b: v, c: p.c, moves: p.moves },
    _ => Pegs.{ a: p.a, b: p.b, c: v, moves: p.moves },
  }
}

fn move_top(p: Pegs, from: Int, to: Int) Pegs {
  src := peg(p, from)
  disk := src[src.len() - 1]
  p2 := set_peg(p, from, src.drop_last())
  dst := peg(p2, to)
  p3 := set_peg(p2, to, dst.append(disk))
  Pegs.{ a: p3.a, b: p3.b, c: p3.c, moves: p3.moves + 1 }
}

fn move_disks(p: Pegs, n: Int, from: Int, to: Int, via: Int) Pegs {
  if n == 1 {
    move_top(p, from, to)
  } else {
    p1 := move_disks(p, n - 1, from, via, to)
    p2 := move_top(p1, from, to)
    move_disks(p2, n - 1, via, to, from)
  }
}

pub warmup := 10
pub iters := 40
pub size := 600
pub expected := 8191

pub fn run(size: Int) Int {
  moves := 0
  rep := 0
  for rep < size {
    // Peg a holds disks 13..1 (largest at bottom = index 0).
    a: Vector<Int> = collect i in range(13) { 13 - i }
    empty: Vector<Int> = []
    start := Pegs.{ a: a, b: empty, c: empty, moves: 0 }
    result := move_disks(start, 13, 0, 2, 1)
    moves = result.moves
    rep = rep + 1
  }
  moves
}
```

> Confirm `drop_last` is exposed on `Vector` (project memory: `Vector.drop_last` runtime builtin). If the spelling is `.drop_last()` vs `.pop()`, use the real one.

- [ ] **Step 2: Node towers** — arrays as stacks (`push`/`pop`), recursive `moveDisks`, count moves. `expected` from node (8191).
- [ ] **Step 3: Go towers** — slices as stacks, same recursion.
- [ ] **Step 4–7:** register, run, agree (8191), run.sh, commit `awfy: add towers benchmark`.

---

## Task 7: List

**Algorithm (AWFY List):** builds three linked lists and does a length-based tail-recursive comparison; AWFY's `makeList(len)` builds a list `len..1`, and `isShorterThan`/`tail` recursion. The observable result is the length of a rebuilt list. Simpler faithful probe preserving the spirit (allocation + traversal): build a cons list `0..len-1`, then sum its elements via tail recursion; checksum = sum. With `len` fixed, checksum is deterministic. Use AWFY's `size` as list length repeated internally; here **build a list of length `size` and return its length via traversal**, so both allocation and traversal are exercised and the checksum (`size`) is trivially cross-checkable — but a length-only checksum is a weak correctness signal. Strengthen it: checksum = sum of all elements = `size*(size-1)/2`.

**Twinkle port note:** `type List = { Nil, Cons(Int, List) }`; build with a loop prepending, traverse with tail recursion (or a loop) accumulating the sum. This is the enum-allocation + recursion probe.

**Files:** `examples/performance/awfy/{twinkle/list.tw, node/list.mjs, go/list.go}` + mains.

- [ ] **Step 1: Twinkle list**

`examples/performance/awfy/twinkle/list.tw`:
```tw
type List = { Nil, Cons(Int, List) }

fn build(n: Int) List {
  acc: List = .Nil
  i := 0
  for i < n {
    acc = .Cons(i, acc)
    i = i + 1
  }
  acc
}

fn sum_list(xs: List, acc: Int) Int {
  case xs {
    .Nil => acc
    .Cons(v, rest) => sum_list(rest, acc + v)
  }
}

pub warmup := 10
pub iters := 40
pub size := 1000
pub expected := 499500

pub fn run(size: Int) Int {
  xs := build(size)
  sum_list(xs, 0)
}
```

> `expected` = `size*(size-1)/2` = `1000*999/2` = `499500`. Confirm the enum literal spelling (`.Nil`, `.Cons(...)`) matches the parser rules for new-line vs same-line variant access.

- [ ] **Step 2: Node list** — `{v, next}` cons cells (or `null`), build + traverse sum. `expected` 499500.
- [ ] **Step 3: Go list** — `type node struct { v int; next *node }`, build + traverse.
- [ ] **Step 4–7:** register, run, agree (499500), run.sh, commit `awfy: add list benchmark (enum cons-list build + traverse)`.

---

## Task 8: Bounce

**Algorithm (AWFY Bounce):** `size`… no — AWFY Bounce simulates a fixed set of 100 balls in a box for 50 steps, using AWFY's `Random` LCG to init positions/velocities; checksum = total number of wall-bounces across all steps (`1331` for the standard config). The LCG: `seed = 74755; next = () => { seed = (seed * 1309 + 13849) & 65535; return seed; }`. Each ball: `x,y ∈ [0,500)`, `xVel,yVel ∈ [-2..2]` derived from the PRNG; each step moves and reflects at walls counting a bounce.

**Determinism is mandatory:** port the LCG *exactly* (`(seed*1309 + 13849) & 65535`, initial seed 74755, integer arithmetic) so all three languages produce identical bounce counts.

**Twinkle port note:** balls as `Vector<Ball>` where `type Ball = .{ x: Int, y: Int, xv: Int, yv: Int }` (AWFY uses ints for position via the PRNG). Thread the PRNG seed as a returned accumulator (record `Rng = .{ seed: Int }` with `fn next(r) -> .{ value, rng }`, or fold into a state record). Each step rebinds the ball vector and accumulates the bounce count. `size` = number of simulation repeats.

**Files:** `examples/performance/awfy/{twinkle/bounce.tw, node/bounce.mjs, go/bounce.go}` + mains.

- [ ] **Step 1: Twinkle bounce**

`examples/performance/awfy/twinkle/bounce.tw`:
```tw
type Rng = .{ seed: Int }
type RngStep = .{ value: Int, rng: Rng }

fn rng_next(r: Rng) RngStep {
  s := (r.seed * 1309 + 13849) & 65535
  RngStep.{ value: s, rng: Rng.{ seed: s } }
}

type Ball = .{ x: Int, y: Int, xv: Int, yv: Int }
type BallStep = .{ ball: Ball, rng: Rng }

// AWFY Ball ctor: x,y in [0,500); xVel,yVel = (next % 5) - 2, scaled.
fn new_ball(r: Rng) BallStep {
  s1 := rng_next(r)
  s2 := rng_next(s1.rng)
  s3 := rng_next(s2.rng)
  s4 := rng_next(s3.rng)
  b := Ball.{
    x: s1.value % 500,
    y: s2.value % 500,
    xv: (s3.value % 5) - 2,
    yv: (s4.value % 5) - 2,
  }
  BallStep.{ ball: b, rng: s4.rng }
}

// One bounce step; returns updated ball and whether it bounced.
type BounceResult = .{ ball: Ball, bounced: Bool }
fn bounce_step(b: Ball) BounceResult {
  x_limit := 500
  y_limit := 500
  bounced := false
  nx := b.x + b.xv
  ny := b.y + b.yv
  xv := b.xv
  yv := b.yv
  if nx > x_limit { nx = x_limit; xv = 0 - xv; bounced = true }
  if nx < 0 { nx = 0; xv = 0 - xv; bounced = true }
  if ny > y_limit { ny = y_limit; yv = 0 - yv; bounced = true }
  if ny < 0 { ny = 0; yv = 0 - yv; bounced = true }
  BounceResult.{ ball: Ball.{ x: nx, y: ny, xv: xv, yv: yv }, bounced: bounced }
}

pub warmup := 10
pub iters := 40
pub size := 1500
pub expected := 0  // fill from node

pub fn run(size: Int) Int {
  ball_count := 100
  total := 0
  rep := 0
  for rep < size {
    // init balls with a fresh seed each repeat for determinism.
    rng := Rng.{ seed: 74755 }
    balls: Vector<Ball> = []
    k := 0
    for k < ball_count {
      bs := new_ball(rng)
      balls = balls.append(bs.ball)
      rng = bs.rng
      k = k + 1
    }
    // 50 steps.
    step := 0
    bounces := 0
    for step < 50 {
      j := 0
      for j < ball_count {
        res := bounce_step(balls[j])
        balls = balls.set_at(j, res.ball)
        if res.bounced { bounces = bounces + 1 }
        j = j + 1
      }
      step = step + 1
    }
    total = bounces
    rep = rep + 1
  }
  total
}
```

> **Match AWFY's exact Ball ctor and step math** against the official `Bounce.js` — the `%`/scaling and wall-limit constants must be identical or the checksum won't match the historical AWFY value. The code above is the standard AWFY shape; verify field-by-field against the source before trusting `expected`. Because all three languages run *the same* ported math, the checksum diff still validates cross-language agreement even if it differs from AWFY's canonical number.

- [ ] **Step 2: Node bounce** — same LCG + ball math; this run defines `expected`.
- [ ] **Step 3: Go bounce** — same.
- [ ] **Step 4–7:** register, run, **paste node's checksum into all three `expected` + the table**, run.sh agreement, commit `awfy: add bounce benchmark (deterministic LCG, persistent ball vector)`.

---

## Task 9: Storage

**Algorithm (AWFY Storage):** GC-stress — recursively build a tree of arrays using the AWFY `Random` LCG to decide branching, counting the number of nodes created; checksum = node count (`5461` for the standard config). AWFY `buildTreeDepth(depth, random)`: if depth == 1, return an array of `random.next() % 10 + 1` nulls; else return an array of length 4 whose elements are `buildTreeDepth(depth-1, random)`. Count every array allocated. Standard depth = 7.

**Determinism:** same LCG as Bounce (seed 74755).

**Twinkle port note:** the tree is `Vector<Node>` where `type Node = { Leaf, Branch(Vector<Node>) }` or simply return the running allocation count (the *result* is the count, and the allocation is the point). To actually stress GC, build real vectors: `build(depth, rng) -> .{ node: Tree, rng: Rng, count: Int }`. `size` = repeats.

**Files:** `examples/performance/awfy/{twinkle/storage.tw, node/storage.mjs, go/storage.go}` + mains.

- [ ] **Step 1: Twinkle storage**

`examples/performance/awfy/twinkle/storage.tw`:
```tw
use .bounce.{Rng}   // reuse the exact same LCG
use .bounce

type Tree = { Leaf, Branch(Vector<Tree>) }
type Built = .{ node: Tree, rng: Rng, count: Int }

fn build(depth: Int, rng: Rng, count: Int) Built {
  c := count + 1
  if depth == 1 {
    step := bounce.rng_next(rng)
    n := (step.value % 10) + 1
    // allocate a leaf vector of length n (contents irrelevant).
    kids: Vector<Tree> = collect i in range(n) { .Leaf }
    Built.{ node: .Branch(kids), rng: step.rng, count: c }
  } else {
    kids: Vector<Tree> = []
    r := rng
    cc := c
    i := 0
    for i < 4 {
      b := build(depth - 1, r, cc)
      kids = kids.append(b.node)
      r = b.rng
      cc = b.count
      i = i + 1
    }
    Built.{ node: .Branch(kids), rng: r, count: cc }
  }
}

pub warmup := 10
pub iters := 30
pub size := 1000
pub expected := 0  // fill from node

pub fn run(size: Int) Int {
  count := 0
  rep := 0
  for rep < size {
    rng := Rng.{ seed: 74755 }
    b := build(7, rng, 0)
    count = b.count
    rep = rep + 1
  }
  count
}
```

> If `use .bounce.{Rng}` cross-module reuse causes friction (per project notes, cross-module inherent methods don't resolve — but plain type/function imports do), fall back to a local `Rng` copy. Verify the two import lines (`use .bounce` for functions, `use .bounce.{Rng}` for the type) compile together.

- [ ] **Step 2: Node storage** — nested arrays, count allocations, same LCG. Defines `expected`.
- [ ] **Step 3: Go storage** — nested slices, count.
- [ ] **Step 4–7:** register, run, paste checksum, run.sh, commit `awfy: add storage benchmark (GC-throughput tree build)`.

---

## Task 10: NBody

**Algorithm (AWFY NBody):** the classic 5-body (Sun + 4 planets) simulation with fixed initial conditions; `size` = number of `advance(0.01)` steps; checksum-equivalent = the system energy after `size` steps. AWFY verifies energy to a tolerance; for a cross-language *integer* checksum, scale the final energy: `checksum = round(energy * 1e9)` (or `floor(energy * 1e8)`) — pick one and use it identically in all three languages. Initial conditions and the `advance`/`energy` math are the canonical NBody (from the Computer Language Benchmarks Game, as AWFY uses).

**Determinism:** no RNG; the fixed constants must be byte-identical across languages (copy the exact planet mass/position/velocity literals from AWFY `NBody.js`). Floating-point results should match across V8/Go/Wasm for this step count, but if the scaled-integer checksum diverges in the last digits, reduce the scale factor (e.g. `1e6`) until all three agree — document the chosen scale in the README.

**Twinkle port note:** bodies as `Vector<Body>` with `type Body = .{ x,y,z,vx,vy,vz,mass: Float }`; each `advance` step rebinds the vector. Uses `@std.math.sqrt`. This is the per-step Vector-copy-cost probe.

**Files:** `examples/performance/awfy/{twinkle/nbody.tw, node/nbody.mjs, go/nbody.go}` + mains.

- [ ] **Step 1: Node nbody first** (reference for constants + checksum). Port AWFY `NBody.js`: `advance(dt)` does pairwise velocity updates then position updates; `energy()` sums kinetic + potential. `run(size)`: init system, `for i in size: advance(0.01)`, return `Math.round(energy() * 1e9)` (or chosen scale). Record the checksum → `expected`.
- [ ] **Step 2: Twinkle nbody**

`examples/performance/awfy/twinkle/nbody.tw` (structure; fill the exact constants from the node port):
```tw
use @std.math

type Body = .{ x: Float, y: Float, z: Float, vx: Float, vy: Float, vz: Float, mass: Float }

solar_mass := 4.0 * math.pi * math.pi
days_per_year := 365.24

fn advance(bodies: Vector<Body>, dt: Float) Vector<Body> {
  n := bodies.len()
  bs := bodies
  i := 0
  for i < n {
    bi := bs[i]
    vx := bi.vx
    vy := bi.vy
    vz := bi.vz
    j := i + 1
    for j < n {
      bj := bs[j]
      dx := bi.x - bj.x
      dy := bi.y - bj.y
      dz := bi.z - bj.z
      d2 := dx * dx + dy * dy + dz * dz
      mag := dt / (d2 * math.sqrt(d2))
      vx = vx - dx * bj.mass * mag
      vy = vy - dy * bj.mass * mag
      vz = vz - dz * bj.mass * mag
      bs = bs.set_at(j, Body.{ x: bj.x, y: bj.y, z: bj.z, vx: bj.vx + dx * bi.mass * mag, vy: bj.vy + dy * bi.mass * mag, vz: bj.vz + dz * bi.mass * mag, mass: bj.mass })
      j = j + 1
    }
    bs = bs.set_at(i, Body.{ x: bi.x, y: bi.y, z: bi.z, vx: vx, vy: vy, vz: vz, mass: bi.mass })
    i = i + 1
  }
  // position update
  k := 0
  for k < n {
    b := bs[k]
    bs = bs.set_at(k, Body.{ x: b.x + dt * b.vx, y: b.y + dt * b.vy, z: b.z + dt * b.vz, vx: b.vx, vy: b.vy, vz: b.vz, mass: b.mass })
    k = k + 1
  }
  bs
}

fn energy(bodies: Vector<Body>) Float {
  n := bodies.len()
  e := 0.0
  i := 0
  for i < n {
    bi := bodies[i]
    e = e + 0.5 * bi.mass * (bi.vx * bi.vx + bi.vy * bi.vy + bi.vz * bi.vz)
    j := i + 1
    for j < n {
      bj := bodies[j]
      dx := bi.x - bj.x
      dy := bi.y - bj.y
      dz := bi.z - bj.z
      dist := math.sqrt(dx * dx + dy * dy + dz * dz)
      e = e - (bi.mass * bj.mass) / dist
      j = j + 1
    }
    i = i + 1
  }
  e
}

// init_bodies builds the 5-body system with AWFY's exact literals and
// offset-momentum correction for the sun. Fill from the node port.
fn init_bodies() Vector<Body> {
  // ... exact sun/jupiter/saturn/uranus/neptune constants ...
  []  // REPLACE with the concrete 5-body vector (see node port).
}

pub warmup := 5
pub iters := 20
pub size := 250000
pub expected := 0  // fill from node

pub fn run(size: Int) Int {
  bodies := init_bodies()
  i := 0
  for i < size {
    bodies = advance(bodies, 0.01)
    i = i + 1
  }
  // checksum: scale energy to an integer (must match node/go scale factor).
  Float.to_int(math.round(energy(bodies) * 1000000.0))
}
```

> The `init_bodies` constants and the checksum scale factor are copied verbatim from the node port completed in Step 1 — do not invent them. Verify `Float.to_int`/`math.round` spellings (`grep -rn "fn to_int" boot/prelude boot/stdlib`).

- [ ] **Step 3: Go nbody** — same constants, same scale factor.
- [ ] **Step 4–7:** register, run each, confirm the scaled-integer checksum agrees across all three (reduce scale if the last digits diverge), run.sh, commit `awfy: add nbody benchmark (5-body float sim over Vector<Body>)`.

---

## Task 11: Json

**Algorithm (AWFY Json):** a hand-written recursive-descent JSON parser over AWFY's fixed input string, producing an object graph; checksum = a structural fold of the parsed value. AWFY's input is a fixed JSON document (`rapidjson` sample); the observable result is a value derived from the parse (AWFY checks specific field values). For a cross-language integer checksum: parse the fixed string and compute `checksum = f(parsed)` — e.g. sum of all integer literals encountered + total number of parsed nodes. Define this fold identically in all three languages.

**Twinkle port note:** parse over the string/`Byte`s (string/Byte codegen diagnostic). Use `@std.buffer` or string indexing; a recursive-descent parser returning a tagged `Json` value:
```tw
type Json = { JNull, JBool(Bool), JNum(Int), JStr(String), JArr(Vector<Json>), JObj(Vector<Entry>) }
type Entry = .{ key: String, value: Json }
```
Thread a parse cursor (`.{ pos: Int }`) through the recursion; the checksum folds `JNum` values and counts nodes. Keep the input string a shared constant (embed the same literal in all three languages).

**Files:** `examples/performance/awfy/{twinkle/json.tw, node/json.mjs, go/json.go}` + mains.

- [ ] **Step 1: Choose a fixed input + define the checksum fold.** Use a compact fixed JSON string (embed the *identical* literal in all three languages). Define `checksum = (sum of all integer numbers) * 31 + (count of all parsed nodes)` — restate this exactly in each language. Start with the node implementation as the oracle.

Suggested shared input (a self-contained subset that exercises objects, arrays, strings, numbers, bools, null):
```json
{"widget":{"debug":"on","window":{"title":"Sample","width":500,"height":300},"items":[1,2,3,4,5],"enabled":true,"data":null}}
```
Fold: numbers `500,300,1,2,3,4,5` sum = 815; node count = however many `Json` nodes the parser builds — let node compute it. `expected` comes from the node run.

- [ ] **Step 2: Node json** — recursive-descent parser over the fixed string, build the value graph, fold to checksum. Defines `expected`.
- [ ] **Step 3: Twinkle json** — port the parser with the `Json`/`Entry` types above and a `Cursor = .{ pos: Int }` threaded through; same fold. Prefer string indexing/`@std.buffer` per whichever is ergonomic; note in the commit which byte/string ops dominated.
- [ ] **Step 4: Go json** — hand-written parser (do **not** use `encoding/json`; the point is comparable parser codegen), same fold.
- [ ] **Step 5–7:** register, run each, confirm agreement (paste node checksum), run.sh, commit `awfy: add json benchmark (hand-written recursive-descent parser)`.

---

## Task 12: Integration — `make awfy` + README

**Files:**
- Modify: `Makefile`
- Create: `examples/performance/awfy/README.md`

- [ ] **Step 1: Add the `make awfy` target**

In `Makefile`, add `awfy` to the `.PHONY` line and a target mirroring `bench` (it needs `target/twk`):
```make
# Run the AWFY-style cross-language benchmark suite. See examples/performance/awfy/README.md.
awfy: target/twk
	@examples/performance/awfy/run.sh
```
Also add a help line near the other bench help prints:
```make
	@printf '  make awfy              Run the AWFY-style cross-language benchmark suite\n'
```

- [ ] **Step 2: Verify `make awfy` runs the whole suite**

Run: `make awfy`
Expected: no CHECKSUM MISMATCH; a normalized table with header `lang bench iters ms checksum us_per_op` and one row per (language, benchmark) for all 10 benchmarks × 3 languages (+ smoke), sorted by bench then lang.

- [ ] **Step 3: README**

`examples/performance/awfy/README.md` covering:
- **Purpose:** compiler perf gap-finding, not absolute cross-language ranking.
- **How to run:** `make awfy` (or `examples/performance/awfy/run.sh` directly); how to run a single language (`target/twk run examples/performance/awfy/twinkle/main.tw`, `node examples/performance/awfy/node/main.mjs`, `go run examples/performance/awfy/go`); note there is no per-benchmark filter yet — comment out rows in the `main` files to isolate one.
- **Honest-baseline caveat:** Node/Go stdlib arrays are native and mutable; Twinkle uses persistent GC structures, so gaps on array-write-heavy benchmarks (**Sieve, Storage, NBody**) are expected and are the diagnostic point.
- **The canonical config table** (copied from this plan, with the filled-in `expected` values).
- **Checksum contract:** each `run(size)` returns the checksum; `run.sh` fails if languages disagree; how to add a benchmark (add `<name>.{tw,mjs,go}` exposing the contract, register in the three mains, run to fill `expected`).
- **NBody checksum scaling note:** state the chosen scale factor and why (float last-digit divergence across runtimes).

- [ ] **Step 4: Format Twinkle sources + lint**

Run:
```bash
target/twk fmt examples/performance/awfy/twinkle/main.tw
for f in examples/performance/awfy/twinkle/*.tw; do target/twk fmt "$f"; done
target/twk lint examples/performance/awfy/twinkle/main.tw
```
Fix any lint findings (expect `direct-rebinding` guidance to already be satisfied since ports use `x = x.set_at(...)`). Re-run `make awfy` to confirm formatting didn't break anything.

- [ ] **Step 5: Commit**

```bash
git add Makefile examples/performance/awfy/README.md examples/performance/awfy/twinkle
git commit -m "awfy: add make target + README documenting the honest-baseline caveat"
```

---

## Task 13: Finalize plan bookkeeping

- [ ] **Step 1:** Per project convention, when the work is complete remove this plan's row (and the design doc's row) from `docs/plans/README.md` and move the docs to `docs/plans/archive/`. (See the "Plans README remove when done" convention.) Do this only after `make awfy` is green end to end.
- [ ] **Step 2:** Final commit `docs(plans): archive AWFY benchmark suite plan (shipped)`.

---

## Self-review notes (for the executor)

- **Spec coverage:** all 10 benchmarks (Task 2–11), per-benchmark contract + harness contract (Task 1), cross-language checksum diff (Task 1 Step 12/14), shared config table (this doc + README, Task 12), `make awfy` + README caveat (Task 12), no compiler/`src` changes (all tasks are `.tw`/`.mjs`/`.go`/bash/Make only). ✔
- **Determinism:** Bounce & Storage share one exact LCG (Task 8 defines `Rng`, Task 9 imports it); NBody has fixed constants; Json has a fixed input. ✔
- **Type consistency:** `Benchmark` record fields (`name,warmup,iters,size,expected,run`) are identical across Twinkle/Node/Go harnesses and every registration site. The `run(size) -> Int` signature is uniform. ✔
- **Known unknowns to verify while executing (each flagged inline):** `Int.to_float`/`Float.to_int`/`math.round` spellings; `Vector.drop_last` exposure; `collect`/`range` bounds; enum literal new-line rules; tuple availability (avoided via records); cross-module type import ergonomics (`use .bounce.{Rng}`). Resolve each with a quick `grep` before leaning on it, as noted in the relevant step.
- **`expected` values:** mandelbrot (191), sieve (669), queens (1000), permute (8660), towers (8191), list (499500) are algorithmically fixed; nbody, bounce, storage, json are filled from the node reference run and then enforced across languages by the checksum diff.
