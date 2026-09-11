# Stack-safety transform: auto-rewrite deep recursion to a heap-stack driver

Status: **Design (spiked scalar-only; representation + soundness open)**. Supersedes
the per-pass rewrite in [compiler-stack-safety.md](compiler-stack-safety.md) as the
*primary* direction. **A scalar-only spike validated the technique and the
scalar-frame perf; the reference-frame representation — which the primary use case
actually needs — is unresolved and unmeasured. Phase 0 resolves it before any other
work.**

## Goal

A compiler pass that automatically rewrites recursion the host stack can't absorb
into a driver loop over a **heap-allocated shadow stack**, so deeply recursive
programs stop overflowing V8's fixed Wasm execution stack — at runtime *and* at
compile time.

## Why this over the combinator (stated strategic bet)

The demonstrated pain today is entirely **compile-time** overflow (the deep `cond`
/ long side-effecting sequence / deep `if/else` reproductions in
[compiler-stack-safety.md](compiler-stack-safety.md)). A deep stack-safe traversal
combinator (Option 2 there; the shallow `archive/fold-core-expr.md` rebuilt as a
deep worklist driver) fixes exactly that, with **no** shadow-stack representation
problem — a compile-time worklist runs once per compile and is perf-insensitive,
single-threaded, and owns its own storage.

We are choosing the auto-transform over the combinator **on the explicit bet that
user *runtime* deep recursion is a real target we want a language-level guarantee
for** — not merely to fix the compiler's compile-time walkers. The compiler being a
user program means one mechanism covers both, but that convenience is not the
justification; the runtime guarantee is. If that bet is wrong, the combinator is the
cheaper fix and this plan should be reconsidered. The combinator remains the
documented fallback for shapes this transform cannot reach.

## The technique (validated)

Defunctionalize recursion: a recursive function becomes a loop that carries its own
call frames on the heap. Only **non-tail** recursive calls (work pending after
return) consume host stack — `return_call` / `return_call_ref` already make tail
calls safe — so only those are reified. **ANF** is the right level: it has named
every intermediate as a `let`-binding, so "what is live across this call" and "what
is the continuation" are syntactic.

**Output shape** (validated by scalar spike): a value register, an integer
stack-pointer, a growable shadow stack, phase as control flow. Descend pushing
frames until a leaf; unwind popping frames and combining. The frame is
`{tag, live-vars}`; tier-2's tag spans the resume points of every function in the
SCC.

## Open question #1 (Phase 0 blocker): frame representation for reference-typed frames

The scalar spike stored frames in a `@std.buffer` (linear memory) and measured
**~1.05×** on integer `sum`. **That result is scalar-only and does not describe the
primary use case.** `@std.buffer` is linear memory — `u8`/`i64`/`f64` only. Wasm-GC
references (records, strings, `array<T>`, closures, dicts, and the compiler's own
`CoreExpr`/`PreparedExpr` nodes + child `Vector`s + envs) **cannot be stored in it.**
The compiler's own walkers — the stated primary target — have ref-dominated frames.

So the shipped representation must be **selected by frame content**:

- **All-scalar frames** → `@std.buffer` fast path. Measured ~1.05× (spike).
- **Reference-bearing frames** (the primary target) → a **mutable, growable GC array
  of generated frame-struct** (`array.set` in place; scalars as struct fields, refs
  as struct fields), OR a hybrid scalar-Buffer + `array<anyref>` ref-stack. **Perf
  UNMEASURED.**

Two facts that make this a real blocker, not a detail:

1. **No primitive exists yet.** `mutvec_families()` is scalar-only
   (`i64`/`bool`/`float`/`byte`); a mutable growable GC ref/struct array with
   in-place set is not an emit surface today — it is adjacent to the deferred MutVec
   Phase 5 (boxed) work and `rt_types__Array`. This support must be built and
   validated first.
2. **The honest baseline is not the spike's strawman.** The doc's earlier "Vector of
   frames: 22–49×" was a *persistent* `Vector`. For reference frames the honest
   comparison is a *mutable* GC array with in-place writes, likely near the
   in-place-`Vector<Int>` ~1.3× row — not 49×. The real ref-frame tax must be
   **measured**, not asserted.

There is no "non-negotiable Buffer" constraint. The Buffer is one specialization for
all-scalar SCCs; the reference path is the load-bearing case and is where the perf
story is currently blank.

## Open question #2 (Phase 0 blocker): shadow-stack lifetime and reentrancy

The stack must be **allocated per driver entry** and held in a local (fresh on entry,
or save/restore `sp`), never a module-global. A global stack is silently corrupted —
producing **wrong results, not an overflow** — by:

- a **reentrant back-edge through a closure** (`A` calls `map(xs, f)`, `f` calls
  `A`), and
- **`Task` concurrency** (two stackful JSPI fibers running the same transformed SCC
  share one stack).

Per-entry lifetime makes the driver reentrant and Task-safe, at the cost of one
allocation per top-level SCC call — which **moves the perf number** and must be
included in the Phase 0 re-spike.

## Soundness invariants (must hold; each gets a red fixture in Phase 1)

1. **Reentrancy:** per-entry shadow stack (Open question #2). No transformed SCC may
   share stack state across invocations or fibers.
2. **`defer` ordering:** reifying a continuation across a non-tail call inside an
   `ADefer`-protected region must still run the deferred code at the correct point
   during unwind. Requires an explicit ordering argument and a defer-bearing
   overflowing fixture.
3. **Tail/`return_call` correspondence:** every call the analysis classifies as
   *tail* (and therefore skips) MUST be emitted downstream as `return_call` /
   `return_call_ref`. If any tail-position call (e.g. a typed-closure/indirect path)
   is not emitted as a real tail call, the analysis skips it and it still overflows.
   State as an invariant; test that the classifier and the emitter agree.
4. **Value-register typing:** for an SCC whose functions have *different* return
   types, the unified driver needs one value register per return type (or an
   explicit boxing story). Enumerate the SCC's return types when synthesizing the
   driver.
5. **Hidden back-edges detected, not just untransformed:** an SCC reachable via a
   back-edge the static analysis cannot see (through a closure / `fn` value / dynamic
   dispatch) must be **detected and left entirely untransformed** — never partially
   transformed. Partial coverage of such an SCC is at best useless and, with any
   shared state, unsound.

## Policy: automatic on statically-detected unbounded-recursion SCCs

Apply automatically to functions in a call-graph SCC with a non-tail back-edge, when
the SCC has **no** back-edge the analysis cannot see (invariant #5). No opt-in
annotation.

**Hybrid depth-threshold (safety valve, ships with the policy):** emit the fast
native function guarded by a depth counter; switch to the driver only past a budget.
Shallow recursion stays native at ~0×; only genuinely-deep recursion pays the driver
tax. This makes automatic application defensible even for a hot-and-shallow SCC, and
means the driver is a **cold path** — which is why a reference-capable ~1.3×
representation may be the right trade over a scalar-only ~1.05× one.

## Phases

Each phase is gated by: the reproduction's depth is raised and the overflow wall
confirmed to move; **self-host fixed point** (`make bundle-cli` stage3==stage4); the
**full boot suite** green; and **byte-identical WAT for modules with no transformed
SCC** (capture `shasum` before, compare after).

### Phase 0 — representation + re-spike (BLOCKER, do first)

Resolve Open questions #1 and #2 with measurement, not assertion:

1. Build (or expose from `rt.arr`) a **mutable growable GC frame-struct array** with
   in-place set — the reference-capable shadow-stack primitive.
2. Hand-write the driver for a **reference-heavy** mutual recursion (a tree of
   records / a mini `CoreExpr`-shaped walker whose frames carry refs + `Vector`s),
   with a **per-entry** stack, and measure vs natural recursion. Also measure the
   all-scalar Buffer path with per-entry lifetime (re-confirm ~1.05× or correct it).
3. Deliverable: a measured ref-frame tax and a decision — uniform GC frame-struct
   array vs hybrid scalar-Buffer + ref-array — recorded here, replacing the blank
   perf story. If the ref-frame tax is unacceptable, this plan stops here.

### Phase 1 — differential harness

An equivalence oracle: compile a function untransformed and transformed, assert
identical results over (a) the boot corpus and (b) randomly-generated deep IR. **Red
fixtures must be reference-heavy AND defer-bearing from day one** (soundness
invariants #2, #4) — a `sum`-like fixture would pass while missing every hole. Keep
the untransformed path until a green soak, then it becomes the regression guard.

Files: new `boot/tests/suites/stack_safety_suite.tw`; a transform entry callable from
tests.

### Phase 2 — tier 1: direct self-recursion

Detect a self-recursive non-tail function; synthesize its `{tag, live-vars}` frame
type (typed per invariant #4); rewrite to descend/unwind over the Phase-0
representation with a per-entry stack. Emit the fixture red tests for reentrancy,
`defer`, and tail-correspondence (invariants #1–#3) and make them pass. Acceptance: a
1000-deep self-recursive fixture that overflows natural recursion compiles and runs;
harness green; measured tax matches Phase 0.

Files (new): `boot/compiler/backend/stack_safety.tw` (SCC + non-tail back-edge +
hidden-back-edge detection, live-vars-at-call), and the ANF rewrite. Wire into the
pipeline after monomorphize/lower_anf, before backend prepare.

### Phase 3 — tier 2: mutual / SCC recursion

Generalize to a whole SCC: one unified frame type enumerating resume points across
every function in the SCC, one combined driver. This is what makes the compiler's own
`lower_expr↔lower_block` / `emit_expr` / `encode_instrs↔encode_instr` safe.
Acceptance: a deep mutual-recursion fixture (ref-bearing) runs; the boot compiler
compiles the deep-`cond` / long side-effecting statement sequence reproductions
without overflow; self-host holds.

### Phase 4 — hybrid depth-threshold

Emit native-with-counter + driver, switch at a budget. Acceptance: shallow recursion
shows ~0× tax (verify via WAT the native path is taken; timing unchanged from
pre-transform); deep recursion still safe.

## Verification methodology

- **Differential harness** (Phase 1) is the primary equivalence proof, over
  ref-heavy + defer-bearing fixtures.
- **Byte-identical WAT** for untouched modules + **self-host fixed point** guard
  against perturbing the compiler itself.
- **The reproductions** are the moving acceptance test: raise N each phase, confirm
  the wall moved. Final acceptance: a 1000-arm `cond` / 1000-statement side-effecting
  function *and* a depth-1M mutual recursion (ref-bearing) compile and run.

## Adjacent fix — LANDED (not a prerequisite)

The transform's shadow stack needs none of the typed-`Vector` machinery, so the
typed-repr grow-rebind fix is an **adjacent** correctness fix, not a prerequisite —
but it removes a real verifier crash on legal growable-`Vector<Int>` user code and is
the closest existing analog to the mutable-array support Phase 0 needs. Landed on
`main` as `a5d15078`: `mutvec_region.tw`'s handle-use scan treated any `AAssign(h, _)`
as a supported rebind; a handle rebound from a fresh producer now rejects the region
(persistent fallback) instead of typing a `PVec` into a `MutVecI64` slot. Test:
`mutvec_region_suite` "handle rebound to a fresh collect is NOT claimed".

## Non-goals

- **Recursion through closures / unknown `fn` values / dynamic dispatch.** A static
  pass can't see the back-edge; such SCCs are **detected and left untransformed**
  (soundness invariant #5), so they behave exactly as today (may overflow) — never
  partially transformed. Whole-program CPS (which would cover them) is rejected: it
  allocates a continuation on every call and defeats the unboxed, fast-by-default
  runtime.
- **Replacing the combinator entirely.** For shapes this transform can't reach,
  [compiler-stack-safety.md](compiler-stack-safety.md) remains the fallback; its
  `prepare.tw` MutVec bailout soundness fix is independent and still required.
- Raising the host stack (verified non-viable in `compiler-stack-safety.md`;
  JSPI/growable-stack routes out of scope by decision).
