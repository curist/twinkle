# Stack-safety transform: auto-rewrite deep recursion to a heap-stack driver

Status: **Design (spiked, prerequisite landed)**. Supersedes the per-pass
rewrite in [compiler-stack-safety.md](compiler-stack-safety.md) as the *primary*
direction. The empirical floor and the representation constraint below are
established by spikes; the one blocking correctness bug (typed-repr grow-rebind)
is fixed and merged.

## Goal

A compiler pass that automatically rewrites recursion the host stack can't
absorb into a driver loop over a **heap-allocated shadow stack**, so deeply
recursive programs stop overflowing V8's fixed Wasm execution stack — at runtime
*and* at compile time.

## Why this over the per-pass rewrite

The self-hosted compiler is itself an ordinary Twinkle program. So "the compiler
overflows while walking deep IR" and "a user program overflows while recursing
deeply" are the **same** problem. Fixing the *generated code* fixes both with one
mechanism, instead of hand-converting each compiler pass (`lower_core`, `opt/*`,
`monomorphize`, `lower_anf`, `prepare`, `emit`, the serializers) to an explicit
stack one at a time. The per-pass approach remains a valid fallback for shapes
this transform can't reach (see Non-goals), and its `prepare.tw` MutVec
soundness item is independent and still needed regardless.

## The technique

Defunctionalize recursion: a recursive function becomes a loop that carries its
own call frames on the heap. Non-tail-recursive calls (calls with work pending
after they return) are the only ones that consume host stack — `return_call` /
`return_call_ref` already make tail calls safe — so only those need reifying.

The right IR level is **ANF**: it has already named every intermediate as a
`let`-binding, so "what is live across this call" and "what is the continuation"
are syntactic, which is exactly what the transform needs to build frames. The
pass is an ANF→ANF rewrite; the existing backend lowers its output unchanged.

**Output shape** (validated by spike; a mutual-recursion `sum` reduced to this):
a value register, an integer stack-pointer, a growable flat shadow stack, and
phase expressed as control flow. Descend pushing frames until a leaf, then unwind
popping frames and combining. The frame is a `{tag, live-vars}` record; for
tier-2 the tag spans the resume points of every function in the SCC.

## The one non-negotiable design constraint: shadow-stack representation

The spike measured, on mutual recursion with realistic per-node work (natural
recursion overflows at depth ~8k–16k; guards identical across all variants):

| Shadow-stack representation | Tax vs natural recursion |
|---|---|
| Persistent `Vector` of frames (naive output) | **22–49×** — unshippable |
| Pre-sized flat `Vector<Int>` + `sp`, in-place `xs[sp]=v` | ~1.3× |
| **Growable `@std.buffer` + `sp` + mode-as-control-flow** | **~1.05×** — essentially native |

Two consequences the implementation MUST honor:

1. **Emit a `@std.buffer`-backed shadow stack**, not a `Vector` of frames.
   Linear-memory `set_i64`/`get_i64` are single load/store; a persistent `Vector`
   used as a hot push/pop stack churns trie nodes and is 20×+ slower. Buffer also
   grows without fighting the ownership analysis (it is unconditionally mutable),
   and at scale it beat the in-place `Vector` (96 ms vs 127 ms at depth 4M).
2. V8 does **not** rescue the naive form — escape analysis cannot elide a frame
   that escapes into the stack (that escape is the point). The win comes entirely
   from the data structure, so the representation is a correctness-of-perf
   requirement, not an optimization to add later.

## Policy: automatic on statically-detected unbounded-recursion SCCs

Apply automatically to functions in a call-graph SCC with a non-tail back-edge
(statically unbounded recursion). No opt-in annotation.

**Hybrid depth-threshold (safety valve, ships with the policy):** emit the fast
native function guarded by a depth counter; switch to the driver only past a
budget. Shallow recursion then pays ~0× (stays native) and only genuinely-deep
recursion pays ~1.05×. This makes "automatic on all SCCs" defensible even for an
SCC that turns out hot-and-shallow.

## Prerequisite — LANDED

**Typed-repr grow-rebind correctness fix** (uncommitted on `main`, verified):
before this, an owned `Vector<Int>` rebound to a freshly-built vector (the
grow/double-on-full idiom) either silently de-typed to boxed (~5×) or tripped the
backend verifier ("physical vector repr mismatch at AAssign: PVec vs
MutVecI64"). Root cause: `mutvec_region.tw`'s handle-use scan treated any
`AAssign(h, _)` as a supported threading rebind, ignoring the RHS. Fix:
`handle_has_foreign_rebind` in `eligible` rejects a region whose handle is rebound
from a producer not derived from the handle → persistent (boxed) fallback, no
crash; threading rebinds (`xs = xs.append(v)`) still claimed. This unblocks the
transform (its Buffer shadow stack needs none of the typed-`Vector` machinery,
but user growable-`Vector` code no longer miscompiles). Test:
`mutvec_region_suite` "handle rebound to a fresh collect is NOT claimed".

## Phases

Each phase is gated by: the reproduction's depth is raised and the overflow wall
confirmed to move; **self-host fixed point** (`make bundle-cli` stage3==stage4);
the **full boot suite** (`target/twk run boot/tests/main.tw`) green; and
**byte-identical WAT for modules with no transformed SCC** (capture `shasum`
before, compare after) — the transform must not perturb untouched code.

### Phase 0 — differential harness (do first)

Build the equivalence oracle before touching codegen. A test seam that compiles a
function both untransformed and transformed and asserts identical results over
(a) the boot corpus and (b) randomly-generated deep IR. This is what lets every
later phase land without silent regressions. Deliverable: harness + a red test
for a known-overflowing direct-recursive fixture.

Files: new `boot/tests/suites/stack_safety_suite.tw`; a transform entry callable
from tests.

### Phase 1 — tier 1: direct self-recursion

Detect a self-recursive function with a non-tail call; synthesize its
`{tag, live-vars}` frame type; rewrite the body to descend/unwind over a Buffer
shadow stack with `sp`. Acceptance: a 1000-deep self-recursive fixture that
overflows natural recursion compiles and runs; differential harness green; the
~1.05× steady-state tax reproduced on the realistic-work fixture.

Files (new): `boot/compiler/backend/stack_safety.tw` (analysis: SCC + non-tail
back-edge detection, live-vars-at-call), and the ANF rewrite (frame synthesis +
driver emission). Wire into the pipeline after monomorphize/lower_anf, before
backend prepare.

### Phase 2 — tier 2: mutual / SCC recursion

Generalize Phase 1 to a whole SCC: one unified frame type enumerating resume
points across every function in the SCC, one combined driver dispatching to any
of them. This is the phase that actually makes the compiler's own
`lower_expr↔lower_block` / `encode_instrs↔encode_instr` safe. Acceptance: a deep
mutual-recursion fixture compiles and runs; the boot compiler compiles a
pathological deep-`cond` / long side-effecting statement sequence (the
reproductions from `compiler-stack-safety.md`) without overflow; self-host holds.

### Phase 3 — hybrid depth-threshold

Emit native-with-counter + driver, switch at a budget. Acceptance: shallow
recursion shows ~0× tax (stays native — verify via WAT that the native path is
taken and timing is unchanged from pre-transform); deep recursion still safe.

## Verification methodology

- **Differential harness** (Phase 0) is the primary equivalence proof — stronger
  than "looks right"; keep the untransformed path until a green soak, then it
  becomes the regression guard.
- **Byte-identical WAT** for untouched modules + **self-host fixed point** guard
  against perturbing the compiler itself.
- **The reproductions** (deep `cond`, long side-effecting sequence, deep mutual
  recursion) are the moving acceptance test: raise N each phase, confirm the wall
  moved. Final acceptance: a 1000-arm `cond` / 1000-statement side-effecting
  function *and* a depth-1M mutual recursion compile and run.

## Deferred (not in this plan) — perf-preserving typed-`Vector` grow

Making a growable owned `Vector<Int>` (rather than Buffer) *fast* needs
multi-producer regions (re-produce the fresh `collect` as a mutvec producer too)
plus grow-reads-old-handle soundness. It is larger and riskier, and the transform
does not need it (Buffer is ~1.05×). Build it only if a real workload demands fast
growable `Vector<Int>`; the landed reject in `mutvec_region.tw` is the exact hook
where it would slot in (turn reject into re-produce). See
[performance/](performance/README.md) typed-vector track.

## Non-goals

- **Recursion through closures / unknown `fn` values / dynamic dispatch.** A
  static pass can't see a back-edge through a runtime function value. Left as a
  documented depth-trap; whole-program CPS (which would cover it) is rejected —
  it allocates a continuation on every call and defeats the unboxed,
  fast-by-default runtime.
- **Replacing the per-pass rewrite entirely.** For shapes this transform can't
  reach, `compiler-stack-safety.md` remains the fallback; its `prepare.tw` MutVec
  bailout soundness fix is independent and still required.
- Raising the host stack (verified non-viable in `compiler-stack-safety.md`,
  including that JSPI/growable-stack routes are out of scope by decision).
