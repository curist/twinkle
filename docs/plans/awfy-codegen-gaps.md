# AWFY Codegen Gaps Plan

**Status:** on branch `codegen-void-elim` (off `awfy-benchmark-suite`, not
merged). Landed: **C1** (dead-Void elim) and **native `Float.sqrt`** — the
latter is the session's big result: **nbody 6.6× faster** (~561→~85 ms), because
the real bottleneck was `math.sqrt` crossing the Wasm→JS boundary, not any of
C3–C5. Prototyped-and-backed-out: **C2** (peephole; code-size only) and **C4 via
immutable record fields** (no runtime effect — V8 already load-eliminates).

Key empirical findings this session, in order of surprise:
1. **`@std.math` functions are JS FFI calls** (`import "Math" "sqrt"`), not native
   Wasm instructions. Routing `sqrt` to the `f64.sqrt` instruction won nbody
   6.6×. Every arithmetic Math fn with a Wasm instruction (floor/ceil/trunc/
   abs/min/max/sqrt) is a free win of the same kind; transcendentals (sin/cos/
   exp/log/pow) have no Wasm instruction and must stay host calls.
2. **C1/C2 are code-size wins, not runtime wins** — V8's optimizing tier already
   DCEs dead Void stores and coalesces set/get churn in hot loops.
3. **C4 (struct.get field caching) is a non-issue** — V8 load-eliminates
   repeated `struct.get` even with mutable fields; forcing immutable fields (and
   losing in-place record update) changed no AWFY benchmark.

**Related:** `examples/awfy/` (the suite), [boot-compiler-perf.md](boot-compiler-perf.md)
(self-host compile time, a separate concern).

### Native `Float.sqrt` intrinsic (DONE — biggest win)

`math.sqrt` was `Math.sqrt(x)`, a JS import called twice per nbody inner
iteration. Added a `Float.sqrt` builtin that lowers to the `f64.sqrt` Wasm
instruction (mirrors the existing `Float.bits`/`.FloatBits` intrinsic:
`prelude/signatures/float.tw` stub + `builtins.tw` `intr(...)` appended at end +
`IntrinsicTag.FloatSqrt` + `build_intrinsic_table` + `emit_intrinsic_call` →
`F64Sqrt`), and repointed `stdlib/math.tw`'s `sqrt` at it. Boot-only (the
compiler never computes sqrt, so no stage0 parity needed).

- nbody ~561→~85 ms (**6.6×**, ~34×→~5× Node); nbody_mut ~544→~52 ms (**10.5×**).
- All other benchmarks unchanged; checksums bit-identical (IEEE-754 sqrt is
  correctly rounded in both JS and Wasm). 2950 tests + self-host fixpoint green.
- The Wasm→JS `Math.sqrt` import is fully DCE'd out of the module.

### Native `Float.abs`/`floor`/`ceil`/`trunc` (DONE — same mechanism)

Extended the intrinsic to the four other `@std.math` float ops whose semantics
match a Wasm instruction exactly: `abs_float`→`f64.abs`, `floor`→`f64.floor`,
`ceil`→`f64.ceil`, `trunc`→`f64.trunc` (added the boot-only `F64Trunc` IR
instruction, opcode `0x9D`; `abs/floor/ceil` reused the IR variants that already
existed but were unemitted). All exact IEEE `roundToIntegral`/`|x|` matches to
JS `Math.*`, so checksums are unaffected. No AWFY benchmark exercises these on a
hot path (AWFY uses only `round`, which cannot convert — see below), so the
suite is unchanged, but any float-heavy user program now avoids the JS boundary.

**What is NOT convertible (and why), completing the `@std.math` audit:**
- *No Wasm instruction exists*: `sin cos tan asin acos atan atan2 sinh cosh
  tanh exp expm1 log log10 log1p log2 pow cbrt hypot random`. WebAssembly has no
  transcendental ops; these stay host `Math.*` calls.
- *Semantics differ from any instruction*: `round` (JS rounds half toward +∞;
  `f64.nearest` is half-to-even, e.g. `round(2.5)=3` vs `nearest(2.5)=2`),
  `sign` (returns −1/±0/1/NaN — no single instruction).
- *Genuinely host-bound*: `@std.{date,io,proc,fs,time}` externs (clock, I/O) —
  no Wasm equivalent, must stay FFI.

### Native `Float` methods: `min`/`max`/`round`/`from_bits` (DONE — new APIs)

Added inherent `Float` methods that lower to Wasm rather than crossing to JS
(these are new API surface, not replacing existing boundary crossings):
- `Float.min`/`Float.max` → `f64.min`/`f64.max` (single instr; NaN/±0 semantics
  match JS `Math.min`/`max`). Callable as `a.min(b)`.
- `Float.from_bits(n)` → `f64.reinterpret_i64` — inverse of `Float.bits`.
- `Float.round` → round half up toward +∞ (`0.5→1`, `1.5→2`, `2.5→3`, `-0.5→-0`),
  matching JS `Math.round`. No single Wasm instruction does this (`f64.nearest`
  is half-to-even, `0.5→0`), so it lowers to `f64.floor` + compare + `select`
  (comparing the exact fractional part, not `floor(x+0.5)`), all native. Note it
  differs from `math.round`'s *identity* only at the sub-ULP boundary because
  the float **literal parser** rounds e.g. `0.49999999999999994` up to `0.5`
  (a pre-existing parser precision issue, unrelated to `round`).

**Considered and dropped:** `math.fround`/`fmin`/`fmax` wrappers (kept the plain
`Float.min`/`max` inherent methods instead; `math.fround` left as-is on the host
`Math.fround`); `Int.popcount`/`leading_zeros`/`trailing_zeros` (`i64.popcnt`/
`clz`/`ctz`) — no consumer in the codebase, so the speculative API and its IR
were removed. (The HAMT dict has its own internal wasm `popcount` helper that
could later use `i64.popcnt` natively — a separate, internal change.)

## Landed on branch `codegen-void-elim` (2026-07-02)

Same-session A/B (`target/twk run examples/awfy/twinkle/main.tw`, 3 rounds,
`ms` column; ±15% noise). Correctness: 2950 boot tests + self-host fixpoint +
five-language AWFY checksum diff all green for both changes.

### C1 — dead Void materialization eliminated (DONE)

Dropped the `i32.const 0; ref.i31; local.set $dead` sentinel at its four emit
sites: `AAssign`, `AGlobalSet` (`emit.tw`), and the Void cases of
`append_result_store` + `emit_loop_op` (`emit/control_flow.tw`). The result slot
of a Void op is never inspected, so the store was pure dead code.

- mandelbrot `run` WAT: ref.i31 52→18, local.set 152→118.
- **Runtime:** permute ~1197→~1163 ms (**~3%, consistent**); mandelbrot/nbody/
  towers/etc **unchanged** — V8 DCEs the dead stores inside hot loops.
- Code size (AWFY suite wasm): 34167→31648 bytes (−7.4%).

### C2 — peephole coalescing of ANF temporaries (PROTOTYPED, BACKED OUT)

A peephole over each function's emitted `Instr` stream (recursing into
if/block/loop) with three semantics-preserving rules — `set x; get x` with x
read once → drop both (stack-thread); read>1 → `local.tee`; `get x; set x` →
drop both (self-copy no-op) — was implemented and validated (self-host fixpoint,
2950 tests, checksums all green), then **removed**. Measurements that justify
the removal:

- mandelbrot `run` WAT: local.set 118→83, local.get 135→100 (~35 set/get pairs
  removed; only 2 needed a tee → most temps were single-use).
- Code size: AWFY suite wasm 31648→28499 bytes (−10% more, −16.6% vs baseline);
  boot.wasm 3214291→3023183 (−6%).
- **Runtime:** **no measurable change on any AWFY benchmark** — V8 already
  coalesces set/get copies in the optimizing tier.
- **Compile time:** the pass adds ~4% to a boot self-compile (5.1→5.3 s), since
  it runs on every emitted function.

Net: a code-size/baseline-tier cleanup with zero optimizing-tier runtime benefit
and a compile-time cost. Not worth carrying for a runtime-focused effort; if
module size / cold-start ever becomes the priority it can be revived (or gated
behind an opt flag). **The compute-bound benchmarks need C3 (inline
`rt_arr__get`), C4 (`struct.get` field caching), and C5 (typed Vector).**

## Goal

The AWFY suite (`examples/awfy/`) exists to surface where Twinkle's Wasm-GC
codegen and runtime are slow versus fast reference implementations (Node/V8, Go),
so the gaps guide compiler work. This document records the current gaps, the
pinpointed root causes (from reading generated WAT), and the ordered attack
vectors. It is the runtime/codegen counterpart to `boot-compiler-perf.md`, which
tracks self-host *compile* time.

## How to measure

Run the whole suite (fails if any cross-language checksum disagrees, then prints
the normalized table):

```bash
make awfy            # or: examples/awfy/run.sh
```

To inspect codegen for one benchmark, build it through a probe entry that forces
the module's `run` to be retained (building the module alone DCE-strips it), then
read the WAT:

```bash
printf 'use .mandelbrot\nprintln("${mandelbrot.run(500)}")\n' > examples/awfy/twinkle/probe.tw
target/twk build examples/awfy/twinkle/probe.tw -o /tmp/mand.wat
rm examples/awfy/twinkle/probe.tw
# then grep/sed the WAT (prefer grep over reading whole file)
```

Per-benchmark times are ms for the timed batch (`iters` × internal `size`);
compare *within* a benchmark across languages, not across benchmarks.

## Current baseline: 2026-07-02

Environment: Darwin arm64; `target/twk` (bundled CLI); Node v26.0.0, Go 1.26.4,
Racket v9.2, Clojure 1.12.5. Single-sample suite run — treat ±15% as noise;
the ratios (not absolute ms) are the signal.

Times in ms (lower = faster). "T/Node" is Twinkle ÷ Node.

| benchmark | Twinkle | Node | Go | Clojure | Racket | T/Node | category |
|---|--:|--:|--:|--:|--:|--:|---|
| list       | 0.47 | 0.19 | 0.39 |  –   |  –   | 2.5× | competitive |
| storage    | 854  | 429  | 439  |  –   |  –   | 2.0× | competitive |
| json       | 134  | 54   | 57   |  –   |  –   | 2.5× | competitive |
| queens     | 292  | 78   | 28   |  –   |  –   | 3.7× | recursion+rebind |
| towers     | 881  | 143  | 63   |  –   |  –   | 6.2× | recursion+rebind |
| mandelbrot | 2635 | 337  | 465  |  –   |  –   | 7.8× | **pure numeric loop** |
| permute    | 1274 | 159  | 75   |  –   |  –   | 8.0× | recursion+rebind |
| bounce     | 1954 | 72   | 58   | 2099 | 1235 | 27×  | persistent write |
| nbody      | 579  | 17   | 13   | 365  | 369  | 34×  | **float compute** |
| sieve      | 37   | 0.9  | 0.26 | 30   | 27   | 41×  | persistent write |
| *sieve_mut*  | **0.86** | – | – | 18   | 0.90 | ≈node | buffer escape hatch |
| *bounce_mut* | **76**   | – | – | 5268 | 328  | ≈node | buffer escape hatch |
| *nbody_mut*  | 540      | – | – | 722  | 278  | (barely) | buffer escape hatch |

Storage-cost isolation (persistent − `_mut`, i.e. how much of each benchmark is
the persistent-structure write):

| benchmark | persistent | buffer | storage share |
|---|--:|--:|--:|
| sieve  | 37   | 0.86 | ~98% |
| bounce | 1954 | 76   | ~96% |
| nbody  | 579  | 540  | ~7%  |

Interpretation: two independent problems.
- **sieve/bounce** are ~pure persistent-write cost; `@std.buffer` already drops
  them to the native league, so the compiler lever is a *default-path* typed
  `Vector` representation.
- **mandelbrot/nbody** are compute-bound; the buffer barely helps nbody (7% was
  storage). The lever is numeric/loop codegen. `list`/`storage`/`json` are
  already competitive (GC allocation + recursion are Twinkle strengths).

## Root causes (from generated WAT)

Instruction histograms of the hot functions (op counts per function body; Wasm
loops are emitted once and executed many times, so these are per-iteration
proportions):

```text
mandelbrot run  : 6 f64.mul, 3 f64.add, 3 f64.sub, 2 f64.div | 118 local.set, 81 local.get, 42 ref.i31
nbody advance   : 19 f64.mul, 8 f64.add, 6 f64.sub, 1 f64.div | 202 local.get, 142 local.set, 21 ref.i31,
                  39 struct.get, 3 struct.new, 3 call rt_arr__get, 1 call rt_arr__len
permute permute : 3 struct.new, 6 struct.get, 4 call user__, 11 ref.i31 | 44 local.get, 43 local.set
```

### C1 — Dead `Void` materialization (pervasive, every function)

After **every assignment statement** the backend emits
`i32.const 0; ref.i31; local.set $dead` — the statement's `Void` result boxed as
an i31ref and stored to a throwaway local. Mandelbrot's inner escape loop pays
~10 of these per iteration; `run` has 42 total, nbody `advance` 21, permute 11.
They compute nothing. Even if V8 lowers `ref.i31` to a tagged immediate, it is
still instructions + a local write in the hottest loops.

Representative WAT (mandelbrot escape loop, `zi = 2.0 * zr * zi + ci`):

```wat
f64.const 2
local.get $p19
f64.mul
local.set $p41
local.get $p41
local.get $p21
f64.mul
local.set $p42
local.get $p42
local.get $p13
f64.add
local.set $p43
local.get $p43
local.set $p21          ;; write result back to zi
i32.const 0
ref.i31
local.set $p44          ;; <-- dead: the statement's Void value
i32.const 0
ref.i31
local.set $p45          ;; <-- dead again
```

### C2 — Un-coalesced ANF temporaries (pervasive)

Every subexpression gets a fresh local that is `set` then immediately `get`,
instead of being threaded on the Wasm operand stack or reusing a slot. Hence
118/81 and 202/142 local set/get for ~20 and ~34 real ops. Copy chains
(`local.set $p37; local.get $p37; …`) and even literal self-copies
(`local.get $p34; local.set $p34`) appear. Wasm locals are cheap, but this is
2–4× the necessary instruction count in the loops that matter, and it obscures
values V8 could otherwise keep in registers.

### C3 — Un-inlined runtime `Vector` ops in hot loops

`bs[i]` / `bs[j]` lower to `call $rt_arr__get` — a real function call per index,
not an inlined `array.get`. nbody `advance` calls `rt_arr__get` 3× per inner
iteration plus `rt_arr__len`. For a 5-element vector the lookup is cheap but the
non-inlined call (argument marshaling, no scalar-replacement across it) is not.
Sieve/bounce persistent paths pay the same for `set_at`.

### C4 — Per-field `struct.get` with no caching across the loop

Body fields are re-read via `struct.get` on every use (39 in nbody `advance`);
nothing hoists `bj.x/y/z/mass` into locals for the duration of an inner
iteration, and nothing keeps the 5 bodies register-resident across the step the
way V8/Go do. This is the bulk of nbody's residual ~34× after storage is removed.

### C5 — Per-step record/vector allocation for rebinding (the persistence cost)

`bs = bs.set_at(j, Body.{…})` and `PState.{…}` emit `struct.new` + the vector
copy each step. This *is* the intended persistent-update cost. It dominates
sieve/bounce (large or heavily-rewritten collections) and is negligible for
nbody (5 elements). Addressed by representation work + the uniqueness optimizer,
not by removing the semantics.

## Attack vectors (ordered)

Ordering favors pervasive, low-risk wins first (C1/C2 touch every benchmark),
then targeted numeric and representation work. Measure each in same-session A/B
on the affected AWFY benchmarks; keep boot self-host green.

### 1. Eliminate dead `Void` materialization (C1)

A statement whose value is `Void` and is discarded should emit nothing (or just
its side effects), not `ref.i31` + `local.set`. Likely a lowering/ANF or emit
rule that always materializes a statement's result value. This is the single
most pervasive win (every loop body) and should be low-risk.
- Expected: largest single improvement on mandelbrot; helps every benchmark.
- Watch: statements whose value *is* used (last expr of a block) must be
  unaffected; `Void`-typed calls with side effects keep the call, drop the
  boxing.

### 2. Coalesce / stack-thread ANF temporaries (C2)

Reduce the set-then-immediately-get churn: thread single-use subexpressions on
the operand stack, reuse local slots for non-overlapping temporaries, and drop
self-copies (`local.get $x; local.set $x`) and copy chains via copy-propagation
into the local layout. Pairs naturally with #1.
- Expected: broad instruction-count reduction in every hot loop.
- Watch: correctness of evaluation order and shared/aliased locals; verify with
  the checksum diff (already enforced across five languages).

### 3. Inline / intrinsic-ify hot `Vector` element ops (C3)

Make `xs[i]` read (and ideally `set_at`) not bottom out in a non-inlined
`rt_arr__get` call on the hot path — inline the small-vector fast path or emit a
direct `array.get` for the trie leaf. Benefits every indexed-collection loop
(nbody, sieve, bounce, permute, towers).
- Watch: keep the persistent semantics; only the *access* is inlined.

### 4. Typed / specialized `Vector` representation on the default path (C5)

The `@std.buffer` `_mut` results prove the ceiling: `sieve` 37→0.86ms, `bounce`
1954→76ms (≈ Node). Bring a slice of that to idiomatic code via typed
`Vector<Int>`/`Vector<Float>` (e.g. `PVecI64`, unboxed leaves) and/or wider use
of the uniqueness optimizer to update uniquely-owned vectors in place. See the
`vector-perf/` endeavor — this benchmark is a concrete driver for it.
- Target: narrow sieve/bounce without forcing users to `@std.buffer`.

### 5. Numeric/loop codegen for the compute-bound cases (C4)

For nbody/mandelbrot after #1/#2: hoist repeatedly-read `struct.get` fields into
locals for an iteration, keep small fixed structs scalar-resident across a loop,
and confirm no boxing sneaks into the f64 path. This is what stands between
Twinkle and V8/Go on `nbody` (~34×) once storage and Void/temp waste are gone.
- Note: Twinkle already stores record `Float` fields unboxed (unlike Racket's
  boxed-flonum persistent vectors), so this is scalar *residency/reload*, not
  unboxing.

### Non-goals

- Micro-optimizing `set_at` itself — it is already on par with Clojure/Racket
  persistent collections; representation (#4) is the lever.
- The OO-macro benchmarks (DeltaBlue/Richards/CD/Havlak) remain out of scope for
  the suite.

## Working rules for future updates

- Re-record the baseline table (with date + environment) when numbers move
  materially; keep the ratios, they are the durable signal.
- Attribute each landed optimization to the benchmark(s) it moved and by how
  much (same-session A/B), the way `boot-compiler-perf.md` tracks phase timings.
- The five-language checksum diff in `run.sh` is the correctness guard for any
  codegen change — a mismatch means the optimization broke semantics.
