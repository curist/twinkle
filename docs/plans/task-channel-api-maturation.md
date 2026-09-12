# Task / Channel API Maturation Plan

> **For agentic workers:** Use `superpowers:executing-plans` (or
> `superpowers:subagent-driven-development`) to implement this plan
> task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The
> workstreams below are **independent** — each ships on its own and can be
> sequenced or split into its own plan. Do WS-A/WS-B (quick wins) before the
> design-first workstreams.

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
| 1 | WS-A: Document `Channel` in API.md | Doc | Yes |
| 2 | WS-B: Fix `Task<Void>` await inference | Bug | Yes |
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

## WS-B: Fix `Task<Void>` await inference

**Why:** `a := Task.spawn(fn() { side_effect() })` then `a.await()` fails
typecheck with `"cannot infer type / ambiguous type"` when the awaited result is
`Void` and discarded — `T` in `Task<T>` never gets pinned. Fire-and-forget and
"spawn a side-effecting task then await it for completion" are normal patterns.
Today they're only expressible by giving the spawned fn a bogus return value
(the playground example had to do this; `task_suite.tw` sidesteps it by always
annotating `fn() Int`/`fn() String`).

**Repro (should compile after the fix):**
```tw
fn side_effect(id: Int) {
  println("task ${id}")
}
a := Task.spawn(fn() { side_effect(1) })
a.await()
```
Actual today: `cannot infer type for this expression ... ambiguous type` at
`a.await()`.

**Files (diagnosis-gated):**
- Investigate: `boot/compiler/checker.tw` (bidirectional inference / meta-var
  resolution for generic builtin calls). The likely root cause is that
  `await<T>` returns `T`, and with the result discarded and the closure body
  `Void`, `T`'s meta-var is never constrained to `Void` and is reported
  ambiguous instead of defaulting. Compare how a discarded `T`-returning generic
  call is defaulted elsewhere.
- Test: `boot/tests/suites/task_suite.tw`.

- [ ] **Step 1: Add a failing test** to `task_suite.tw`'s `suite()` chain:
  ```tw
  .test(
    "spawn and await a void task",
    fn() {
      log: Cell<Int> = Cell.new(0)
      t := Task.spawn(fn() { log.set(7) })
      t.await()
      try assert.equal(log.get(), 7)
      .Ok({})
    },
  )
  ```

- [ ] **Step 2: Run it and confirm it fails at typecheck.**
  Run: `target/twk run boot/tests/main.tw` (or the narrower suite entry).
  Expected: compile error `ambiguous type` at `t.await()`.

- [ ] **Step 3: Diagnose.** Use `target/twk ir /tmp/repro.tw` and the checker to
  find where `T` for the discarded `await` is left as an unresolved meta-var.
  Determine whether the fix belongs in: (a) defaulting an unconstrained
  return-position meta-var to `Void` when the value is discarded at statement
  position, or (b) propagating the spawned closure's `Void` body type into
  `Task<T>`'s `T` at `spawn`. Prefer (b) if `spawn`'s closure already pins `T` in
  the non-void case — that means the void case is the outlier.

- [ ] **Step 4: Implement the minimal fix** in `boot/compiler/checker.tw` at the
  location found in Step 3. (Exact edit depends on diagnosis — do not guess
  before Step 3.)

- [ ] **Step 5: Verify** the new test passes and existing task/channel suites
  still pass: `make boot-test`.

- [ ] **Step 6: Self-host check.** The fix is in the boot compiler's checker
  (`boot/compiler/checker.tw`); the failing test lives in `boot/tests/` and is
  not compiled by stage0. stage0 (Rust, `src/`) has its own independent checker
  — it needs the same fix **only if** boot source reachable from `boot/main.tw`
  relies on void-task await, which it does not today (existing call sites
  annotate `fn() Int` etc.). So `make stage2` should pass unchanged; run it to
  confirm the checker change didn't regress the self-host fixed point. Mirror the
  fix in stage0 only if `make stage2` breaks.

- [ ] **Step 7: Commit.**
  ```bash
  git add boot/compiler/checker.tw boot/tests/suites/task_suite.tw
  git commit -m "fix(check): infer Void for discarded Task await result"
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
- **No fabricated steps:** WS-A/WS-B/WS-C have concrete steps because their shape
  is known; WS-D/WS-E/WS-F are explicitly design-gated because their
  implementation depends on an unmade decision — writing step-by-step code there
  would be placeholder guesswork, which this plan deliberately avoids.
- **Type consistency:** signatures (`try_await<T>(Task<T>) Result<T, String>`,
  `join_all`) are proposed, not yet locked — flagged as design decisions, not
  presented as final APIs to code against blindly.
