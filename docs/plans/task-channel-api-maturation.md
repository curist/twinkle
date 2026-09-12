# Task / Channel API Maturation Plan

> **For agentic workers:** Use `superpowers:executing-plans` (or
> `superpowers:subagent-driven-development`) to implement this plan
> task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The
> workstreams below are **independent** — each ships on its own and can be
> sequenced or split into its own plan. WS-A is a quick doc win. WS-B was
> reclassified after diagnosis (2026-09-12) from a quick fix to a core
> type-inference change — see its section.

## Goal

Grow Twinkle's cooperative concurrency API (`Task<T>`, `Channel<T>`) from a
clean happy-path MVP into something you can build real concurrent systems with:
fix the one concrete inference bug, close the doc gap, make task failure
recoverable, and design the two missing capabilities (`select`, structured
joins/cancellation) that the current surface can't express.

## Motivation

The current API is minimal, orthogonal, and philosophically coherent (immutable
values + single-thread cooperative scheduling ⇒ no data races by construction).
It nails producer/consumer and worker-pool shapes. But building the playground
concurrency example (`playground/src/examples/concurrency.tw`) surfaced six
gaps, ordered here by priority:

1. **`Task<Void>` await is ambiguous** — a real inference bug (WS-B).
2. **No `select`** — cannot wait on "whichever channel is ready first"; the
   defining missing CSP capability (WS-E).
3. **Task failure is a trap, not a `Result`** — inconsistent with the rest of
   the language's recoverable-error story (WS-C).
4. **No structured concurrency / cancellation** — no join-all, no cancel, no
   timeout; fan-in close coordination is hand-rolled and deadlock-prone (WS-F).
5. **`send` returns a silently-ignorable `Bool` on a closed channel** — a small
   ergonomics/soundness decision (WS-D).
6. **`Channel` is missing from `docs/API.md`** — doc gap (WS-A).

---

## Current Baseline (architecture)

Task/Channel are **compiler-recognized intrinsics**, not ordinary library code.
Three layers:

* **API surface / type signatures** — placeholder-bodied prelude stubs that
  declare the Twinkle-visible signatures:
  * `boot/prelude/signatures/task.tw` — `spawn<T>(fn() T) Task<T>`,
    `await<T>(Task<T>) T`, `yield() Void`.
  * `boot/prelude/signatures/channel.tw` — `new<T>() Channel<T>`,
    `bounded<T>(Int) Channel<T>`, `send<T>(Channel<T>, T) Bool`,
    `recv<T>(Channel<T>) T?`, `close<T>(Channel<T>) Void`.
  * `boot/prelude/channel.tw` — `iter<T>(Channel<T>) Iterator<T>` (backs
    `for v in ch`, via `Iterator.unfold` over `recv()`).
  * These stubs are also embedded verbatim in `boot/lib/module/core_lib.tw`
    (generated; regenerate after editing prelude — see
    `reference_intrinsic_builtin_wiring` in agent memory).
* **Compiler recognition** — the checker/lowering treat `Task`/`Channel` as
  builtin generic types and lower their methods to runtime imports in the
  `task` / `channel_*` namespaces.
* **Runtime** — `tools/js_runtime/runtime.mjs` implements the cooperative
  scheduler (`task.*` intrinsics, JSPI `WebAssembly.promising`) and
  `channel_new` / `channel_bounded` / `channel_send` / `channel_recv` /
  `channel_recv_is_value` / `channel_recv_value` host functions. Non-JSPI
  runtimes get fail-on-use stubs so unused imports still link.

What already works and is tested:

* `spawn`/`await`/`yield`, bounded + unbuffered channels, `send`/`recv`/`close`,
  `for v in ch` drain — suites `boot/tests/suites/task_suite.tw`,
  `channel_suite.tw`; JS-side `tools/js_runtime/runtime.test.mjs`.
* **Deadlock is already detected**, not hung: the scheduler traps with
  `"task deadlock: remaining tasks are all blocked on channels/awaits"` /
  `"...blocked awaiting each other"` (`runtime.mjs` ~line 817). Repro:
  `boot/repros/channel_deadlock.tw`.
* **A spawned task that fails and is never awaited** surfaces as the program's
  failure (`runtime.mjs` ~line 825).
* `Channel.bounded(0)` traps with a clear message
  (`boot/repros/channel_bounded_zero.tw`).

---

## Global Constraints

* **Self-host fixed point.** Any prelude/compiler change must survive
  `make stage2` (rebuild `target/boot.wasm`) and keep `make boot-test` green
  (3549 boot tests at time of writing).
* **stage0 dependency is a COMPILE-time concern, not runtime.** The self-host
  loop's first step is `./target/release/twk build boot/main.tw` — stage0 (the
  Rust compiler in `src/`) compiles the entire `boot/main.tw` import graph to
  produce stage1. That graph includes `boot/commands/lsp.tw`, which uses
  `Channel<Vector<Byte>>` / `Channel<Bool>` (`lsp.tw:30-31`) — so stage0 must
  *typecheck and lower* `Channel`, even though the LSP is never *run* during
  bootstrap and the channel scheduler never *executes* under stage0. Since
  `make stage2` passes today, stage0 already handles `Channel` compilation.
  **A *new* primitive (`select`, `try_await`) is a stage0 concern only if boot
  source reachable from `boot/main.tw` (notably `boot/compiler/*`,
  `boot/commands/*`) adopts it — and then only for compilation, not execution**
  (see `reference_stage0_bootstrap_dependency`). Simplest path: keep new
  primitives out of boot compiler/command source (tests, stdlib, and the
  playground don't cross stage0), so no stage0 work is needed. If boot source
  must adopt one, verify with `make stage2` (which exercises the stage0→stage1
  build) and add stage0 support only if that build breaks.
* **Runtime wiring touches two runtimes.** New intrinsics need the host
  function in `runtime.mjs` **and** a non-JSPI fail-on-use stub, plus a Safari
  extern-metadata regression note per the comment at `runtime.mjs` ~line 387.
* **Heavy verification runs sequentially**, never concurrently or backgrounded
  (`feedback_sequential_heavy_verification`): `make boot-test`, then
  `make stage2`, then `make bundle-cli`.
* **Format + lint every edited `.tw`**: `target/twk fmt <file>` and
  `target/twk lint <entry>`.
* Commit style: short imperative subject, what/why/how body, no line-count
  metrics.

---

## Workstream Priority

| Order | Workstream | Type | Ship independently? |
|-------|------------|------|---------------------|
| 1 | WS-A: Document `Channel` in API.md | Doc | Yes — **DONE** |
| 2 | WS-B: Fix inferred-return propagation through closures | Bug (core inference) | Yes — **DONE** |
| 3 | WS-C: Recoverable task failure (`try_await`) | Feature | Yes |
| 4 | WS-D: `send`-on-closed ergonomics | Decision + small change | Yes |
| 5 | WS-E: `select` over channels | Design-first feature | Yes (after design) |
| 6 | WS-F: Structured concurrency / cancellation | Design-first feature | Yes (after design) |

---

## WS-A: Document `Channel` in `docs/API.md`

**Why:** `docs/API.md` has the `Task.*` rows but no `Channel` section, even
though `Channel` is in `docs/spec.md` §15. Contributors (and the earlier
analysis for this plan) were surprised it was absent.

**Files:**
- Modify: `docs/API.md` (add a `Channel` subsection next to the `Task` rows,
  ~line 95).

- [ ] **Step 1: Add the `Channel` API table.** Mirror the `Task` table style.
  Copy signatures verbatim from `boot/prelude/signatures/channel.tw` and the
  `iter` from `boot/prelude/channel.tw`:

  | Op | Signature | Notes |
  |----|-----------|-------|
  | `Channel.new` | `fn<T>() Channel<T>` | Unbuffered rendezvous channel |
  | `Channel.bounded` | `fn<T>(capacity: Int) Channel<T>` | Buffered; `capacity >= 1` (0/negative traps) |
  | `ch.send` | `fn<T>(Channel<T>, T) Bool` | Suspends under backpressure; `false` if closed |
  | `ch.recv` | `fn<T>(Channel<T>) T?` | `.None` once closed and drained |
  | `ch.close` | `fn<T>(Channel<T>) Void` | Idempotent |
  | `for v in ch` | — | Iterates until closed and drained (via `iter`) |

- [ ] **Step 2: Cross-check against spec.** Confirm wording matches
  `docs/spec.md` §15 (backpressure, close idempotency, drain semantics). Fix any
  drift in whichever doc is wrong.

- [ ] **Step 3: Commit.**
  ```bash
  git add docs/API.md
  git commit -m "docs(api): document Channel in the API reference"
  ```

---

## WS-B: Fix inferred-return-type propagation through closure arguments

> **Reclassified (2026-09-12): NOT a quick win.** Originally scoped as a small
> `Task<Void>` await-defaulting fix. Diagnosis showed the true root cause is a
> general type-inference ordering bug in `checker.tw`, not Task-specific.
> Touching core inference requires full self-host re-verification and carries
> real regression risk — treat this as its own careful workstream, not a
> drive-by patch.

**Why:** `a := Task.spawn(fn() { side_effect() })` then `a.await()` fails
typecheck with `"cannot infer type / ambiguous type"` when `side_effect` itself
has an inferred return type. Fire-and-forget and "spawn a side-effecting task
then await it for completion" are normal patterns. Calls whose return type is
already concrete work, which is why the existing `Cell.set`- and
`Task.yield`-tailed task tests do not expose the bug.

**Verified root cause (2026-09-12).** The bug is **not** about `Void`, `await`,
or `Task` specifically. It reproduces with a plain user generic:
```tw
fn side(id: Int) { println("t${id}") }   // NO return annotation → inferred Void
fn run<T>(f: fn() T) T { f() }
run(fn() { side(1) })                     // ← "ambiguous type" on the closure
```
The trigger is precise: **a closure whose *tail expression* is a call to a
function with an *inferred* (unannotated) return type, passed to a generic where
that return drives instantiation, leaves the type variable unsolved.** Pass 0
gives the callee a shared return meta-var. The call links that meta-var to the
generic call's expected-return meta-var, but `check_function` later resolves the
callee by calling `set_subst` on the original id directly. That destructive
write replaces the link instead of resolving its terminal meta-var, so the
call-site meta remains unsolved.

This is also a checker soundness bug, not just an ambiguity bug. An earlier
concrete constraint can be erased the same way:
```tw
fn inferred_int() { 1 }
x: String = inferred_int()
```
The call first constrains the inferred return meta to `String`; the direct
`set_subst` later overwrites it with `Int`, allowing the inconsistency to reach
the backend verifier instead of producing a checker type mismatch.

Confirmed by differential testing — all of these **work** (so they define the
workarounds and bound the bug):
- Annotate the callee's return: `fn side(...) Void { ... }` ✓
- Annotate the closure: `fn() Void { side(1) }` ✓
- Callee with an already-annotated return (e.g. `Task.yield()`) ✓
- Closure body with **no tail expression** (a `for`-loop or a trailing
  statement, e.g. `fn() { side(1) 0 }`) — the no-tail path unifies `Void` with
  the expected type directly ✓ (this is why the worker-pool example's
  loop-bodied worker closures compiled)
- An int-literal / any concrete-typed tail ✓

And these **fail** (same root cause): `fn() { side(1) }` (single void-call
tail), `fn() { side(1) side(2) }` (void-call tail after a statement),
`Task.spawn(fn(){side(1)}).await()` (chained), `collect ... { Task.spawn(fn(){
side(id) }) }` then `for t in ts { t.await() }`.

**Where it lives (traced):** `boot/compiler/checker.tw`. The relevant chain is
`check_closure` (no-annotation branch, ~3056/3135) → `check_block` tail
(~5025) → `check_expr` `.Call` path (~2988) → `synth_call` `.Ident` non-local
branch (~1745) → `pre_unify_return` (~569). For a non-generic callee,
`instantiate` (~1462) returns `sig.ret` directly; when `sig.ret` is an
unresolved return meta (unannotated function), `pre_unify_return` unifies it
with the closure's expected meta. The bug is in `check_function` (~5338): its
direct `set_subst(mid, inferred_ret)` overwrites any existing substitution for
`mid`, severing meta-to-meta links or erasing concrete constraints before the
finalize sweep (~5567).

**Chosen fix:** replace that destructive assignment with a dedicated
substitution-preserving inferred-return binding step. Ordinary inferred returns
must unify the Pass-0 signature return with the synthesized body return, which
follows existing links and reports incompatible earlier constraints. `Never`
needs a narrow special case because normal unification intentionally treats it
as compatible with every type: resolve the signature return through the current
substitution and bind only an unresolved terminal meta to `Never`. Keep writing
the synthesized return into `env.functions` so later calls and lowering see the
actual signature. Do not reorder functions or introduce dependency/SCC passes;
that would add recursion complexity without repairing the unsafe overwrite.

**Files (diagnosis and fix design complete):**
- `boot/compiler/checker.tw` — substitution-preserving inferred-return binding.
- Test: `boot/tests/suites/task_suite.tw` (Task-facing case) **and**
  `boot/tests/suites/checker_suite.tw` (the general user-generic case, since the
  bug is not Task-specific).

- [x] **Step 1: Add failing tests** — a Task-facing inferred-`Void` propagation
  case in `task_suite.tw`, plus propagation and constraint-preservation cases in
  `checker_suite.tw`:
  ```tw
  // task_suite.tw
  fn set_log(log: Cell<Int>) {
    log.set(7)
  }

  .test(
    "spawn and await a void task",
    fn() {
      log: Cell<Int> = Cell.new(0)
      t := Task.spawn(fn() { set_log(log) })
      t.await()
      try assert.equal(log.get(), 7)
      .Ok({})
    },
  )
  ```
  Add a `checker_suite.tw` case asserting `run(fn() { side(1) })`
  (inferred-`Void` callee) type-checks clean, matching the suite's existing
  check-success harness. Add a negative case asserting that assigning an
  inferred-`Int` call to `String` produces a checker type mismatch; it must not
  survive until backend verification. Include a forward-reference propagation
  shape and preserve coverage for inferred `Never` returns.

- [x] **Step 2: Run and confirm the propagation cases fail** with `ambiguous
  type` and the constraint-preservation case fails because the checker emits no
  diagnostic: `target/twk run boot/tests/main.tw`.

- [x] **Step 3: Design the fix** against the traced root cause above. Preserve
  the existing substitution graph by unifying ordinary inferred returns and
  binding only the terminal unresolved meta for `Never`. This directly repairs
  both lost meta propagation and erased concrete constraints without changing
  function-check order.

- [x] **Step 4: Implement** the substitution-preserving inferred-return binding
  in `boot/compiler/checker.tw`.

- [x] **Step 5: Verify** the new tests pass and nothing regresses:
  `make boot-test` (full suite — a change this central must run all of it).

- [x] **Step 6: Self-host check.** Run `make stage2` — the fixed point
  (stage3 == stage4) is the real guard that a central inference change didn't
  perturb codegen. The fix changes the boot compiler's own checker; existing boot
  source doesn't rely on the buggy pattern (unannotated-return closures into
  generics currently wouldn't compile), so the stage0 (Rust) → stage1 build
  should be unaffected. stage0's Rust checker likely has the analogous gap, but
  it only matters if boot source *adopts* the pattern — mirror the fix in `src/`
  only if `make stage2` breaks.

- [x] **Step 7: Commit.**
  ```bash
  git add docs/plans/task-channel-api-maturation.md docs/plans/README.md \
    boot/compiler/checker.tw boot/tests/suites/checker_suite.tw \
    boot/tests/suites/task_suite.tw
  git commit -m "fix(check): preserve inferred return constraints"
  ```

---

## WS-C: Recoverable task failure (`try_await`)

**Why:** `Task.await` "propagates a task failure as a trap" — one task erroring
kills the whole program with no per-task recovery. That's inconsistent with the
language's `Result`/`try` story for recoverable errors. Add a way to await a task
and recover from its failure.

**Design decision to settle first:** the error type. Twinkle traps carry a
string message (see runtime stack-trace work). Options:
- `try_await<T>(Task<T>) Result<T, String>` — simplest; failure message as
  `String`. Recommended for a first cut.
- A structured `TaskError` record (message + optional source span). More work;
  defer unless the trap machinery already surfaces structured info cheaply.

Note: **only recoverable `error(...)` failures should be catchable.** Hard traps
(OOB, div0) must stay uncatchable — mirror `defer`'s "does not trigger on traps"
rule (spec §12) and the panic/OOB behavior from the runtime-stack-traces work.
Decide explicitly whether `try_await` catches only `error(...)`-originated
failures or all non-fatal task failures, and document it.

**Files:**
- Modify: `boot/prelude/signatures/task.tw` (add `try_await` signature stub).
- Regenerate: `boot/lib/module/core_lib.tw` (embedded prelude copy).
- Modify: `tools/js_runtime/runtime.mjs` (new `task` intrinsic returning a
  success/failure discriminant + value/message; the scheduler already tracks
  `state === "failed"` and `t.error` at ~line 827 — expose it instead of
  rejecting).
- Modify: non-JSPI stub for the new intrinsic (`runtime.mjs` ~line 727 region).
- Modify: stage0 `src/` if boot source will use `try_await` (see Global
  Constraints). If boot source does not adopt it, a stage0 fail-stub is enough
  to keep the module linking.
- Test: `boot/tests/suites/task_suite.tw`, `tools/js_runtime/runtime.test.mjs`.

- [ ] **Step 1: Write the design note** capturing the error-type decision and
  the catch-scope (error-only vs all-non-fatal) at the top of this workstream or
  in `docs/design/`. This is the gate — do not implement before it's settled.

- [ ] **Step 2: Add a failing boot test** for the success and failure paths:
  ```tw
  .test(
    "try_await recovers from a failed task",
    fn() {
      good := Task.spawn(fn() Int { 1 })
      bad := Task.spawn(fn() Int { error("boom") })
      try assert.equal(good.try_await(), .Ok(1))
      case bad.try_await() {
        .Ok(_) => .Err("expected failure"),
        .Err(msg) => assert.contains(msg, "boom"),
      }
    },
  )
  ```
  (Confirm `assert.contains` exists in `@std.testing.assert`; if not, match on
  the message with `.index_of`.)

- [ ] **Step 3: Add the JS-side failing test** in `runtime.test.mjs` mirroring
  the two paths, so the runtime intrinsic is covered independently of the boot
  compiler.

- [ ] **Step 4: Implement** the signature stub, the runtime intrinsic + non-JSPI
  stub, regenerate `core_lib.tw`, and wire compiler recognition (follow
  `reference_intrinsic_builtin_wiring` / `reference_runtime_builtin_wiring`).

- [ ] **Step 5: Verify** — `make boot-test`, the JS runtime tests
  (`node --test tools/js_runtime/runtime.test.mjs` or the repo's runner), then
  `make stage2`.

- [ ] **Step 6: Document** `try_await` in `docs/API.md` and `docs/spec.md` §15,
  including the catch-scope rule (traps not caught).

- [ ] **Step 7: Commit** (one commit for the feature + tests + docs, or split
  runtime/boot/docs if a reviewer would gate them separately).

---

## WS-D: `send`-on-closed ergonomics

**Why:** `send` returns `Bool` (`false` if closed), which is silently
ignorable — easy to drop the check and lose data. This is a small,
self-contained API-taste decision.

**Design decision to settle first (pick one):**
- **Keep `Bool`, add lint.** Add a `twk lint` rule flagging a `ch.send(...)`
  whose result is discarded (like an unused `Result`). Lowest churn; preserves
  the current API. Recommended.
- **Trap on send-after-close.** Matches Go's panic-on-closed-send; makes the bug
  loud. Breaking change to current semantics; `send` becomes `Void`.
- **Return `Result<Void, SendError>`.** Consistent with `try`, but heavier for
  the common always-open case.

**Files (if "keep Bool + lint" chosen):**
- Modify: `boot/compiler/lint.tw` (new rule; follow the existing rule structure
  and `--explain` rationale convention).
- Test: `boot/tests/suites/lint_*` (match the existing lint suite naming).

- [ ] **Step 1: Settle the decision** with the API owner; record it here.
- [ ] **Step 2..N:** flesh out steps once the direction is chosen (lint rule vs
  semantic change vs Result). Each path is small; do not pre-write both.

> Left intentionally shallow: the right steps depend entirely on Step 1's
> outcome, and writing three divergent step-lists would be speculative.

---

## WS-E: `select` over channels (design-first)

**Why:** The defining missing CSP capability. You cannot currently wait on
"whichever of these channels is ready first," nor do a non-blocking try-recv.
This blocks timeouts (race a work channel against a timer), multiplexing
(fan-in from N channels without a dedicated task each), and cancellation
patterns (WS-F depends on this).

**This is a design workstream — produce a design doc before any code.** Do NOT
write implementation tasks until the design is reviewed; fabricating steps here
would be guesswork.

**Open design questions to resolve in the design doc:**
- **Surface syntax.** A `select { ch1.recv() => ..., ch2.send(v) => ..., _ => ...
  }` block (like `case`)? Or a library form (`select.recv([ch1, ch2]) (Int, Int)`
  returning which-index + value)? A syntax form reads best but is a parser +
  checker + lowering change; a library form is intrinsics-only. Weigh against
  the naming/parser rules (§16) and the existing `case`/`cond` machinery.
- **Send arms.** Does `select` support send-arms (ready-to-send) or recv-only in
  v1? Recv-only is much simpler and covers most needs; defer send-arms.
- **Default / non-blocking.** A `_ =>` (or `default`) arm that makes `select`
  non-blocking (returns immediately if nothing ready) — this is the try-recv
  primitive. Include in v1.
- **Timeout.** Express timeout as a `select` arm over a timer channel, or a
  first-class `select` timeout clause? Prefer composition (timer channel) if a
  timer-channel primitive is cheap; otherwise a clause.
- **Fairness.** If multiple arms are ready, which wins — first-listed,
  round-robin, or random? Go randomizes to avoid starvation. Decide and document;
  cooperative single-thread makes this a pure scheduler-policy choice.
- **Scheduler support.** What does the scheduler need? Today `recv` parks a task
  on one channel's waiter list. `select` needs a task registered as a waiter on
  *several* channels simultaneously, woken by whichever fires first, with the
  others de-registered atomically. Scope this against the waiter-list model in
  `runtime.mjs` (`waiters`, `blockedOnTask`).
- **stage0.** If any boot-source concurrency adopts `select`, stage0 needs it
  (Global Constraints). Likely keep `select` out of boot source initially.

- [ ] **Step 1: Write `docs/plans/select-design.md`** (or `docs/design/`)
  answering every question above with a chosen option + rationale, plus a
  worked example and the runtime waiter-registration sketch.
- [ ] **Step 2: Review the design** with the API owner. Only then split the
  implementation into its own plan (parser? checker? lowering? runtime
  intrinsic? stage0?) with bite-sized tasks.

---

## WS-F: Structured concurrency / cancellation (design-first)

**Why:** No cancel, no timeout, no scope that joins its children on exit. The
fan-in close coordination is hand-rolled: the playground example needed a
dedicated "closer" task that awaits all workers then closes the results channel.
Forget it and you get a **deadlock trap** (the scheduler detects it — not a
silent hang — but it's still a bug the API invites). A join/scope primitive and
cancellation would remove this footgun class.

**This is a design workstream — produce a design doc before any code.** It also
likely **depends on WS-E** (cancellation is naturally expressed as a select over
a cancel channel).

**Open design questions to resolve in the design doc:**
- **Join primitive.** `Task.join_all(tasks: Vector<Task<T>>) Vector<T>`? Does it
  short-circuit on first failure (with WS-C's recoverable variant
  `try_join_all` returning `Result`)? This directly removes the "closer task"
  boilerplate.
- **Structured scope.** A `scope { ... }` block that auto-joins (or cancels)
  child tasks spawned within it when the block exits — the structured-concurrency
  model (nursery / task group). Interaction with `defer` and with `return`/`try`
  unwinding needs care (both already unwind blocks at the CFG level).
- **Cancellation model.** Cooperative cancel (a cancel token/channel a task polls
  at task points) vs. forced. Cooperative fits the "switch only at task points"
  invariant; forced does not. Almost certainly cooperative — specify how a task
  observes cancellation (a `CancelToken` checked at `yield`/`recv`, or a
  select-over-cancel-channel pattern from WS-E).
- **Timeout.** `Task.await_timeout(t, ms) T?` or express via WS-E select + timer.
  Prefer composition if the timer-channel primitive lands.
- **Deadlock ergonomics.** The scheduler already traps on deadlock
  (`runtime.mjs` ~line 817). Should a join/scope primitive convert common
  deadlock shapes into a cleaner diagnostic, or is the existing trap message
  enough? Decide.

- [ ] **Step 1: Write `docs/plans/structured-concurrency-design.md`** answering
  every question above, sequenced after WS-E, with worked examples that replace
  the playground example's manual closer-task pattern.
- [ ] **Step 2: Review**, then split into its own implementation plan.

---

## Cross-Cutting Notes

* **Playground is not blocked by any of this.** The shipped concurrency example
  works around WS-B (spawned fns return values) and WS-F (manual closer task).
  None of these workstreams are prerequisites for it.
* **Verification order for any code workstream:** `target/twk fmt` + `twk lint`
  on edited `.tw` → `make boot-test` → `make stage2` → `make bundle-cli`, run
  sequentially.
* **After completion**, per repo lifecycle: move this doc to
  `docs/plans/archive/` and remove its row from the plan index in
  `docs/plans/README.md` (don't mark it "Done" in place).

## Self-Review

- **Coverage:** all six analysis items map to a workstream (1→WS-B, 2→WS-E,
  3→WS-C, 4→WS-F, 5→WS-D, 6→WS-A). ✓
- **No fabricated steps:** WS-A (done) and WS-C have concrete steps; WS-B has a
  complete, verified diagnosis but its *fix design* is left open (a core
  inference change that must not be guessed); WS-D/WS-E/WS-F are design-gated
  because their implementation depends on an unmade decision. Writing
  step-by-step code for the open items would be placeholder guesswork, which this
  plan deliberately avoids.
- **Type consistency:** signatures (`try_await<T>(Task<T>) Result<T, String>`,
  `join_all`) are proposed, not yet locked — flagged as design decisions, not
  presented as final APIs to code against blindly.
