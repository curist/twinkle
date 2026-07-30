# Sound-Uniqueness Stage2 Hang Investigation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Identify and fix the sound-uniqueness codegen performance cliff that makes `make stage2` appear to hang after the generic I/O change.

**Architecture:** Treat the generic I/O work as the trigger and the sound-uniqueness codegen path as the suspected failing component. Use explicit phase logs and kill switches to separate Phase 8G variant specialization, mutable-decision production, scoped summary solving, and selected ownership analysis. Fix the smallest confirmed root cause, then preserve diagnostics that are generally useful and remove noisy temporary tracing.

**Tech Stack:** Twinkle boot compiler (`boot/`), Rust stage0 bootstrap (`src/` only if needed), Deno JS runtime harness, `make stage2`, `TWINKLE_TIMINGS`, `TWINKLE_VARIANT_SPECIALIZE`, new diagnostic env flags, boot tests via `target/twk run boot/tests/main.tw`.

## Global Constraints

- Root cause before fix: do not optimize by guesswork; each change must be tied to a measured failing component.
- Keep the generic I/O implementation behavior intact while investigating unless a task explicitly tests an isolation rollback.
- Do not weaken soundness checks or silently disable mutable emission in normal builds as the final fix.
- Any diagnostic flag added for investigation must be either documented and kept intentionally, or removed before completion.
- After editing `.tw` files, run `target/twk fmt` on the exact files changed in the task and run `target/twk lint boot/main.tw`.
- Never run `tree-sitter test` from the agent.
- For timeout commands piped through `tee`, use `set -o pipefail` when the exit status matters; otherwise inspect the saved log explicitly because the pipeline status may be `tee`'s status.
- Any temporary source rollback must have an explicit backup/restore step. If a diagnostic run fails or times out, restore the source and rerun generated-file steps before continuing.

---

## Current Evidence

- `make stage2` gets through stage0 → stage1, bridge generation, and stage1 project check, then stalls during the stage1 build of `target/boot.wasm`.
- `TWINKLE_TIMINGS=1` shows frontend, core linking, monomorphization, optimization, and builder-region rewrite complete before the stall.
- With default settings, the last high-level phase marker is:

```text
[time:phase] begin variant_specialize
[time:8g:phase] begin summary_table
```

- With `TWINKLE_VARIANT_SPECIALIZE=0`, the stall moves to mutable-decision production:

```text
[time:phase] begin produce_mutable_decisions
[time:mutable:phase] begin ownership_artifacts roots=514 seeds=0
[time:mutable:artifact:phase] begin scoped_summary
```

- With both sound-uniqueness codegen paths disabled, the stage1 build completes:

```bash
TWINKLE_TIMINGS=1 \
TWINKLE_VARIANT_SPECIALIZE=0 \
TWINKLE_MUTABLE_PRODUCE=0 \
BOOT_WASM=/tmp/boot-stage1-diag.wasm \
deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build -o /tmp/stage2-no-su.wasm
```

- This points at the shared summary/ownership machinery used by codegen Phase 8G and mutable-decision production, not at stringification itself.

## Diagnostic Flags Currently Useful

These flags are part of the investigation surface and should be kept until Task 6 decides their fate.

| Flag | Effect |
|---|---|
| `TWINKLE_TIMINGS=1` | Enables coarse phase timings and new begin-phase logs. |
| `TWINKLE_VARIANT_SPECIALIZE=0` | Existing kill switch for Phase 8G variant specialization. |
| `TWINKLE_MUTABLE_PRODUCE=0` | New investigation kill switch for mutable decision production. Emits persistent fallback decisions by using an empty decision table. |
| `TWINKLE_SUMMARY_TRACE=1` | New verbose SCC trace for summary solving. Use only on focused runs; it is too noisy for ordinary logs. |

---

## File Structure

- Modify: `boot/compiler/codegen/codegen.tw`
  - Owns high-level codegen phase markers and the `TWINKLE_MUTABLE_PRODUCE` isolation switch.
- Modify: `boot/compiler/codegen/variant_specialize.tw`
  - Owns Phase 8G subphase markers and any fix to variant-specialization prefiltering or summary reuse.
- Modify: `boot/compiler/codegen/mutable_produce.tw`
  - Owns mutable candidate counts and producer subphase markers.
- Modify: `boot/compiler/codegen/ownership_verdicts.tw`
  - Owns scoped artifact phase markers and any fix to scoped summary/ownership artifact computation.
- Modify: `boot/compiler/summary.tw`
  - Owns SCC trace output, dependency closure, summary fixpoint behavior, and summary table post-processing.
- Possibly modify: `boot/compiler/ownership.tw`
  - Owns `with_dedupe_helpers` and any temporary subphase markers or scoped helper variants.
- Temporarily modify: `boot/prelude/io.tw`
  - Used only for the diagnostic string-only rollback in Task 3. Always back it up and restore the generic wrapper source before proceeding.
- Regenerate when prelude sources change: `boot/lib/module/core_lib.tw`
  - Produced by `python3 tools/generate_core_lib.py`; do not edit by hand.
- Modify or add tests under `boot/tests/suites/`
  - Add regression coverage for whichever root cause is confirmed.
- No planned changes to `tree-sitter-twinkle/`.

---

## Task 1: Lock in the diagnostic harness

**Files:**
- Modify: `boot/compiler/codegen/codegen.tw`
- Modify: `boot/compiler/codegen/variant_specialize.tw`
- Modify: `boot/compiler/codegen/mutable_produce.tw`
- Modify: `boot/compiler/codegen/ownership_verdicts.tw`
- Modify: `boot/compiler/summary.tw`

**Interfaces:**
- Consumes: existing `TWINKLE_TIMINGS` and `TWINKLE_VARIANT_SPECIALIZE` behavior.
- Produces: repeatable phase logs, `TWINKLE_MUTABLE_PRODUCE=0`, and `TWINKLE_SUMMARY_TRACE=1`.

**Starting state note:** This task can be executed either by adding the diagnostic hooks from scratch or by validating already-applied diagnostic changes. If the hooks already exist in the working tree, treat Task 1 as a compile-and-reproduction lock-in rather than a request to duplicate them.

- [x] **Step 1: Confirm phase logs compile.**

Run:

```bash
target/twk fmt boot/compiler/codegen/codegen.tw boot/compiler/codegen/variant_specialize.tw boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/summary.tw
./target/release/twk check boot/main.tw
```

Expected: type checking succeeds.

- [x] **Step 2: Build a diagnostic stage1 compiler.**

Run:

```bash
./target/release/twk build boot/main.tw -o /tmp/boot-stage1-diag.wasm
```

Expected: `/tmp/boot-stage1-diag.wasm` is produced.

- [x] **Step 3: Verify the sound-uniqueness bypass completes.**

Run:

```bash
TWINKLE_TIMINGS=1 \
TWINKLE_VARIANT_SPECIALIZE=0 \
TWINKLE_MUTABLE_PRODUCE=0 \
BOOT_WASM=/tmp/boot-stage1-diag.wasm \
deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build -o /tmp/stage2-no-su.wasm
```

Expected: the build reaches `WASM output: /tmp/stage2-no-su.wasm`.

- [x] **Step 4: Verify default still reproduces the stall with phase evidence.**

Run:

```bash
set -o pipefail
TWINKLE_TIMINGS=1 \
BOOT_WASM=/tmp/boot-stage1-diag.wasm \
timeout 180s deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build -o /tmp/stage2-default-trace.wasm 2>&1 | tee /tmp/stage2-default-trace.log
```

Expected: the log reaches `begin variant_specialize` and `begin summary_table`, then does not reach `variant_specialize:` before the timeout.

- [x] **Step 5: Verify the mutable-only stall shape.**

Run:

```bash
set -o pipefail
TWINKLE_TIMINGS=1 \
TWINKLE_VARIANT_SPECIALIZE=0 \
BOOT_WASM=/tmp/boot-stage1-diag.wasm \
timeout 180s deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build -o /tmp/stage2-mutable-only-trace.wasm 2>&1 | tee /tmp/stage2-mutable-only-trace.log
```

Expected: the log reaches `begin produce_mutable_decisions`, `begin ownership_artifacts`, and `begin scoped_summary`, then does not reach `end ownership_artifacts` before the timeout.

---

## Investigation Results

| Run | I/O wrapper shape | 8G | Mutable producer | Candidate counts | Root count | Last phase |
|---|---|---|---|---|---|---|
| bypass | generic | off | off | no mutable candidates collected | no mutable roots collected | completed: `WASM output: /tmp/stage2-no-su.wasm` |
| default trace | generic | on | on | not collected; 8G stalls before mutable producer | not collected | timeout after `[time:8g:phase] begin summary_table`; no `variant_specialize:` completion |
| mutable-only current | generic | off | on | `calls=791 records=638 fields=191` from `/tmp/stage2-candidate-current.log` | `roots=514 seeds=0` from `/tmp/stage2-candidate-current.log` | timeout after `[time:summary:scc] first=643`; no `end ownership_artifacts` |
| string-only diagnostic | string-only | off | on | `calls=791 records=638 fields=191` from `/tmp/stage2-candidate-string-io.log` | `roots=514 seeds=0` from `/tmp/stage2-candidate-string-io.log` | timeout after `[time:summary:scc] first=643`; no `end ownership_artifacts` |
| summary trace | generic | on | on | not collected; 8G stalls before mutable producer | not collected | timeout inside SCC solving before first round for `set_prelude_function_origins` |

Task 1 evidence is recorded in `/Users/curist/playground/rust/twinkle/.superpowers/sdd/2026-07-30-sound-uniqueness-stage2-hang-investigation/task-1-report.md`. The diagnostic stage1 compiler used for these runs is `/tmp/boot-stage1-diag.wasm`; trace logs are `/tmp/stage2-default-trace.log` and `/tmp/stage2-mutable-only-trace.log`.

Task 2 evidence is recorded in `/Users/curist/playground/rust/twinkle/.superpowers/sdd/2026-07-30-sound-uniqueness-stage2-hang-investigation/task-2-report.md`. The default summary trace timed out after:

```text
[trace:summary:scc:end] first=732 first_name=register_methods_from_groups members=1 blocks=9 insts=15 edges=10 rounds=1 visits=1 changed=1 remaining=0
[trace:summary:scc:start] first=733 first_name=set_prelude_function_origins members=1 blocks=11 insts=36 edges=12
```

No `trace:summary:scc:round`, `trace:summary:scc:end`, or `time:summary:scc` marker appeared for `first=733` before timeout. This localizes the default stall to pre-round summary work for the base-env/prelude-origin function `set_prelude_function_origins`; `with_dedupe_helpers` was not reached, so Task 2 added no dedupe markers and required no stage1 rebuild.

Task 3 evidence is recorded in `/Users/curist/playground/rust/twinkle/.superpowers/sdd/2026-07-30-sound-uniqueness-stage2-hang-investigation/task-3-report.md`. The diagnostic source-only rollback to string I/O produced the same mutable candidate counts and root count as the generic I/O run (`calls=791 records=638 fields=191`, `roots=514 seeds=0`). Both runs timed out in scoped summary after the last completed SCC marker `first=643`, so the measured data rejects simple candidate/root growth as the trigger and points toward analysis behavior on an unchanged root set.

---

## Task 2: Localize the default stall inside `summary.compute`

**Files:**
- Modify: `boot/compiler/summary.tw`
- Possibly modify: `boot/compiler/codegen/variant_specialize.tw`
- Possibly modify: `boot/compiler/ownership.tw`

**Interfaces:**
- Consumes: `TWINKLE_SUMMARY_TRACE=1` from Task 1.
- Produces: a specific phase boundary inside `summary.compute`: SCC solving, `with_dedupe_helpers`, or variant table input preparation.

- [x] **Step 1: Run a bounded default trace with SCC details.**

Run:

```bash
set -o pipefail
TWINKLE_TIMINGS=1 \
TWINKLE_SUMMARY_TRACE=1 \
BOOT_WASM=/tmp/boot-stage1-diag.wasm \
timeout 180s deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build -o /tmp/stage2-summary-trace.wasm 2>&1 | tee /tmp/stage2-summary-trace.log
```

Expected: the log shows repeated `[trace:summary:scc:start]`, `[trace:summary:scc:round]`, and `[trace:summary:scc:end]` lines until the timeout.

- [x] **Step 2: Identify the last completed and first incomplete summary marker.**

Run:

```bash
rg "trace:summary:(compute|scc|roots)|time:8g:phase|time:summary:scc" /tmp/stage2-summary-trace.log | tail -n 200
```

Expected: one of these outcomes is clear:

- last line is `trace:summary:scc:start` without a matching end, so a specific SCC is hot;
- all SCCs end and last line is `trace:summary:compute:dedupe:start`, so `with_dedupe_helpers` is hot;
- all summary markers end and 8G does not reach `begin variant_table`, so code between markers is hot.

- [x] **Step 3: If a specific SCC is hot, capture its identity.**

Record the line containing:

```text
[trace:summary:scc:start] first=643 first_name=parse_expr_in members=25 blocks=1440 insts=2083 edges=1972
```

Expected: the plan notes the function name and whether the SCC is parser-related, resolver-related, base-env-related, or prelude-generated.

- [x] **Step 4: If dedupe is hot, add temporary subphase markers in `with_dedupe_helpers`.**

In `boot/compiler/ownership.tw`, locate `pub fn with_dedupe_helpers(...)` and add `TWINKLE_TIMINGS`-guarded markers around each major loop. Use this format:

```twinkle
if timings_enabled() {
  eprintln("[time:own:dedupe:phase] begin classify_helpers")
}
```

Expected: the next trace identifies the hot dedupe loop without changing normal behavior when `TWINKLE_TIMINGS` is unset.

- [x] **Step 5: Rebuild stage1 after any added markers.**

Run:

```bash
target/twk fmt boot/compiler/ownership.tw
./target/release/twk check boot/main.tw
./target/release/twk build boot/main.tw -o /tmp/boot-stage1-diag.wasm
```

Expected: the diagnostic stage1 compiler is refreshed.

---

## Task 3: Measure candidate/root growth caused by the generic I/O trigger

**Files:**
- Modify: `boot/compiler/codegen/mutable_produce.tw`
- Modify: `boot/compiler/codegen/variant_specialize.tw`
- Possibly modify: `boot/compiler/summary.tw`
- Temporarily modify: `boot/prelude/io.tw`
- Regenerate: `boot/lib/module/core_lib.tw`

**Interfaces:**
- Consumes: candidate counts from Task 1 logs.
- Produces: a comparison table that separates “small trigger” from “analysis blow-up.”

- [x] **Step 1: Capture current generic-I/O candidate counts.**

Run:

```bash
set -o pipefail
TWINKLE_TIMINGS=1 \
TWINKLE_VARIANT_SPECIALIZE=0 \
BOOT_WASM=/tmp/boot-stage1-diag.wasm \
timeout 120s deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build -o /tmp/stage2-candidate-current.wasm 2>&1 | tee /tmp/stage2-candidate-current.log
rg "time:mutable:phase|scope_roots|trace:summary:roots" /tmp/stage2-candidate-current.log
```

Expected: the log includes concrete counts in the form `calls=791 records=638 fields=191` and a concrete root count such as `roots=514`.

- [x] **Step 2: Create a diagnostic source-only rollback of `boot/prelude/io.tw`.**

Back up the current generic wrapper source before replacing it:

```bash
cp boot/prelude/io.tw /tmp/twinkle-prelude-io.generic.backup.tw
```

Temporarily replace `boot/prelude/io.tw` with string-only wrappers that still call hidden sinks:

```twinkle
//! Diagnostic string-only I/O wrappers.

/// Print a string to stdout.
pub fn print(value: String) Void {
  __print_string(value)
}

/// Print a string to stdout followed by a newline.
pub fn println(value: String) Void {
  __println_string(value)
}

/// Trap with an unrecoverable error message.
pub fn error(value: String) Never {
  __error_string(value)
}

/// Print a string to stderr.
pub fn eprint(value: String) Void {
  __eprint_string(value)
}

/// Print a string to stderr followed by a newline.
pub fn eprintln(value: String) Void {
  __eprintln_string(value)
}
```

Then regenerate the embedded prelude:

```bash
python3 tools/generate_core_lib.py
./target/release/twk build boot/main.tw -o /tmp/boot-stage1-string-io-diag.wasm
```

If any command after the replacement fails or times out, immediately restore the backup and rerun `python3 tools/generate_core_lib.py` before investigating anything else.

Expected: a diagnostic stage1 compiler with ordinary `io.tw` wrappers but no generic `Stringify` calls.

- [x] **Step 3: Capture string-only I/O candidate counts.**

Run:

```bash
set -o pipefail
TWINKLE_TIMINGS=1 \
TWINKLE_VARIANT_SPECIALIZE=0 \
BOOT_WASM=/tmp/boot-stage1-string-io-diag.wasm \
timeout 120s deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build -o /tmp/stage2-candidate-string-io.wasm 2>&1 | tee /tmp/stage2-candidate-string-io.log
rg "time:mutable:phase|scope_roots|trace:summary:roots" /tmp/stage2-candidate-string-io.log
```

Expected: candidate/root counts can be compared to Step 1.

- [x] **Step 4: Restore generic `boot/prelude/io.tw`.**

Restore from the backup created in Step 2:

```bash
cp /tmp/twinkle-prelude-io.generic.backup.tw boot/prelude/io.tw
python3 tools/generate_core_lib.py
target/twk fmt boot/prelude/io.tw
```

Expected: the working tree is back to the intended generic I/O behavior.

- [x] **Step 5: Record the comparison in this plan or a follow-up note.**

Add an “Investigation Results” section with:

```markdown
## Investigation Results

| Run | I/O wrapper shape | 8G | Mutable producer | Candidate counts | Root count | Last phase |
|---|---|---|---|---|---|---|
| current | generic | on | on | measured from `/tmp/stage2-candidate-current.log` | measured from `/tmp/stage2-candidate-current.log` | measured from `/tmp/stage2-candidate-current.log` |
| mutable-only | generic | off | on | measured from `/tmp/stage2-candidate-current.log` | measured from `/tmp/stage2-candidate-current.log` | measured from `/tmp/stage2-candidate-current.log` |
| string-only diagnostic | string-only | off | on | measured from `/tmp/stage2-candidate-string-io.log` | measured from `/tmp/stage2-candidate-string-io.log` | measured from `/tmp/stage2-candidate-string-io.log` |
| bypass | generic | off | off | no mutable candidates collected | no mutable roots collected | completed |
```

Expected: the table supports or rejects the hypothesis that the trigger is candidate/root growth.

---

## Task 4: Test focused fixes for confirmed polynomial hotspots

**Files:**
- Modify: `boot/compiler/summary.tw`
- Possibly modify: `boot/compiler/ownership.tw`
- Possibly modify: `boot/compiler/codegen/variant_specialize.tw`
- Possibly modify: `boot/compiler/codegen/ownership_verdicts.tw`
- Test: add or update a focused suite under `boot/tests/suites/`

**Interfaces:**
- Consumes: hot component identified in Tasks 2 and 3.
- Produces: one minimal fix with an automated regression guard.

Choose exactly one fix path based on evidence. If none of Fix Paths A-C match the evidence, use Fix Path D first to localize the pre-round ownership summarization hotspot and then either return to A-C or apply the minimal confirmed source/analysis fix.

### Fix Path D: pre-round ownership summarization hotspot

Use this path if `TWINKLE_SUMMARY_TRACE=1` stops at `trace:summary:scc:start` for a specific SCC without a matching first `trace:summary:scc:round`, and Task 3 does not show candidate/root growth. The goal is to identify whether the hot work is liveness/prep, ownership fixpoint, classification, field-requirement finishing, or a particular source-level callee pattern.

- [ ] **Step D1: Add temporary per-function summary markers around `summarize_function_cached`.**

In `boot/compiler/summary.tw`, inside `run_scc`, add `TWINKLE_TIMINGS`-guarded markers immediately before and after each `summarize_function_cached` call. Include the function name, id, block count, instruction count, and elapsed time. Keep the output off unless `TWINKLE_TIMINGS` is set.

Expected: the next trace says whether the hot function enters ownership summarization and whether it returns.

- [ ] **Step D2: If the function does not return, add temporary intra-function ownership markers.**

In `boot/compiler/ownership.tw`, instrument the major sections of the summarization path already summarized by `[time:own:summarize]`: liveness, fixpoint, classify, field requirements, and finish. If existing timing markers only print after return, add begin/end markers around the same sections so a timeout reveals the active subsection.

Expected: the next trace identifies the non-returning subsection for `set_prelude_function_origins` or another hot function.

- [ ] **Step D3: Rebuild the diagnostic stage1 after markers.**

Run:

```bash
target/twk fmt boot/compiler/summary.tw boot/compiler/ownership.tw
./target/release/twk check boot/main.tw
./target/release/twk build boot/main.tw -o /tmp/boot-stage1-diag.wasm
```

Expected: the diagnostic stage1 compiler is refreshed.

- [ ] **Step D4: Run a bounded trace with the new markers.**

Run:

```bash
set -o pipefail
TWINKLE_TIMINGS=1 \
TWINKLE_SUMMARY_TRACE=1 \
BOOT_WASM=/tmp/boot-stage1-diag.wasm \
timeout 180s deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build -o /tmp/stage2-hot-function-trace.wasm 2>&1 | tee /tmp/stage2-hot-function-trace.log
```

Expected: the log identifies the exact active subsection for the hot SCC. Record the decisive marker lines in Investigation Results.

- [ ] **Step D5: Choose the smallest confirmed fix.**

Based on D4, choose one:

- If the hotspot is duplicate worklist growth in scoped roots, return to Fix Path A.
- If the hotspot is whole-view dedupe, return to Fix Path B.
- If the hotspot is unneeded 8G publication for non-updatable functions, continue with Fix Path C.
- If the hotspot is a source-level compiler helper that is semantically simple but analysis-hostile, rewrite that helper only after proving the rewrite preserves behavior and add regression coverage for the helper behavior or the stage2 path.

Expected: the plan names the selected root cause and fix before implementation.

### Fix Path A: `dependency_closure` queue blow-up

Use this path only if traces show `trace:summary:roots:closure:start` without a prompt matching `closure:end`, or if root/candidate counts reveal repeated queued callees.

- [ ] **Step A1: Add a regression test for duplicate queued callees.**

Create a test in an existing summary/ownership suite that constructs or compiles a call graph where many roots share callees. The assertion should compare closure membership, not timing:

```twinkle
// Shape: roots a(), b(), c() all call shared(), and shared() calls leaf().
// dependency_closure should return each reachable function once.
```

Expected before fix: the test may pass functionally, so use trace/counter instrumentation to prove the duplicate queue behavior before changing code. Add a temporary counter around `dependency_closure` that reports enqueue attempts, duplicate enqueue skips, and final closure size; remove or intentionally retain the counter during Task 6 cleanup.

- [ ] **Step A2: Add a queued set to `dependency_closure`.**

Change `dependency_closure` to mark items when enqueued, not only when closed. Preserve sorted deterministic processing. Use a `queued: Dict<Int, Bool>` and only append a callee when it is neither closed nor queued.

- [ ] **Step A3: Verify closure semantics.**

Run the focused test and:

```bash
./target/release/twk check boot/main.tw
```

Expected: same reachable set, fewer duplicate worklist entries in trace.

### Fix Path B: `with_dedupe_helpers` whole-view post-processing is hot

Use this path only if all SCCs complete and the last marker is `trace:summary:compute:dedupe:start` or `trace:summary:roots:dedupe:start`.

- [ ] **Step B1: Confirm whether scoped summary needs whole-view dedupe helpers.**

Read `boot/compiler/ownership.tw` around `with_dedupe_helpers` and every caller of `summary.compute_for_roots_cached`.

Expected: identify whether scoped artifacts require helper summaries for every function or only functions in the dependency closure plus helper dependencies.

- [ ] **Step B2: Add a scoped dedupe helper variant.**

Introduce a new function only if the proof is clear:

```twinkle
pub fn with_dedupe_helpers_for_roots(
  table: SummaryTable,
  view: CfgView,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  roots: Dict<Int, Bool>,
) SummaryTable
```

It must preserve conservative summaries outside the scoped closure.

- [ ] **Step B3: Use the scoped helper only from `compute_for_roots_cached`.**

Keep whole-program `summary.compute` using the existing full helper behavior.

- [ ] **Step B4: Add equivalence coverage.**

Add a test comparing verdicts for mutable candidate keys under full artifacts and scoped artifacts.

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: scoped/full equivalence still holds.

### Fix Path C: 8G summary table repeats expensive work unnecessarily

Use this path if default 8G is hot in `summary.compute`, but mutable-only scoped summary is also hot enough to require shared improvements.

- [ ] **Step C1: Add phase-level measurement around 8G summary reuse candidates.**

Record the timing for:

```twinkle
view := ownership.prune_dead_merge(cfg.build_view(anf, b))
table := summary.compute(view, b, sem)
vt := summary.compute_variants(view, b, sem, table)
```

Expected: the summary table dominates 8G.

- [ ] **Step C2: Evaluate an early updatable prefilter before variant publication.**

Use `updatable_funcs(anf, b)` before expensive caller scans where sound. Do not skip summary publication for callees that can influence updatable roots unless the dependency proof is explicit.

- [ ] **Step C3: If safe, scope 8G variant publication to updatable callees and their recursive SCCs.**

The scoped variant table must still support recursive self-routing for cloned functions.

- [ ] **Step C4: Add a regression fixture for recursive field-tier routing.**

Run existing focused suites that cover 8G/8H:

```bash
target/twk run boot/tests/main.tw
```

Expected: existing variant routing and field-backed collection tests continue to pass.

---

## Task 5: Confirm the fix on the stage2 path

**Files:**
- Modify: implementation files from Task 4 only.
- No new source files unless Task 4 added tests.

**Interfaces:**
- Consumes: selected fix from Task 4.
- Produces: restored `make stage2` progress with sound-uniqueness codegen enabled.

- [ ] **Step 1: Rebuild stage1 with the fix.**

Run:

```bash
./target/release/twk build boot/main.tw -o /tmp/boot-stage1-fixed.wasm
```

Expected: stage1 is produced.

- [ ] **Step 2: Run default stage1 build with timings.**

Run:

```bash
set -o pipefail
TWINKLE_TIMINGS=1 \
BOOT_WASM=/tmp/boot-stage1-fixed.wasm \
deno run --allow-read --allow-write --allow-env tools/js_runtime/deno_main.mjs build -o /tmp/stage2-fixed.wasm 2>&1 | tee /tmp/stage2-fixed.log
```

Expected: the build reaches `WASM output: /tmp/stage2-fixed.wasm` without disabling 8G or mutable production.

- [ ] **Step 3: Run `make stage2`.**

Run:

```bash
make stage2
```

Expected: fixed point is reached.

- [ ] **Step 4: Run boot tests.**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: boot tests pass.

- [ ] **Step 5: Run lint.**

Run:

```bash
target/twk lint boot/main.tw
```

Expected: no new relevant lint findings.

---

## Task 6: Cleanup diagnostics and document retained flags

**Files:**
- Modify: `boot/compiler/codegen/codegen.tw`
- Modify: `boot/compiler/codegen/variant_specialize.tw`
- Modify: `boot/compiler/codegen/mutable_produce.tw`
- Modify: `boot/compiler/codegen/ownership_verdicts.tw`
- Modify: `boot/compiler/summary.tw`
- Possibly modify: `docs/plans/sound-uniqueness/codegen/README.md`

**Interfaces:**
- Consumes: confirmed fix and diagnostic evidence.
- Produces: clean final tree with intentional diagnostics only.

- [ ] **Step 1: Decide whether to keep `TWINKLE_MUTABLE_PRODUCE=0`.**

Keep it only if it is useful as a documented emergency isolation flag. If kept, document that it forces persistent fallback by skipping mutable decision production.

- [ ] **Step 2: Decide whether to keep `TWINKLE_SUMMARY_TRACE=1`.**

Keep it only if the output is guarded and valuable for future ownership debugging. If kept, ensure it is off by default and not implied by `TWINKLE_TIMINGS`.

- [ ] **Step 3: Remove noisy temporary phase markers not worth keeping.**

Normal `TWINKLE_TIMINGS=1` should remain readable. Prefer keeping high-level begin/end phase markers and removing overly granular markers that were only useful for this investigation.

- [ ] **Step 4: Update sound-uniqueness codegen docs if flags remain.**

In `docs/plans/sound-uniqueness/codegen/README.md`, add a short diagnostic section describing retained flags and when to use them.

- [ ] **Step 5: Format and lint.**

Run:

```bash
target/twk fmt boot/compiler/codegen/codegen.tw boot/compiler/codegen/variant_specialize.tw boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/summary.tw
target/twk lint boot/main.tw
```

Expected: formatting is stable and lint reports no new relevant findings.

---

## Task 7: Final verification and handoff

**Files:**
- Modify: this plan with final investigation results.
- Modify: tests/docs touched by the chosen fix.

**Interfaces:**
- Consumes: final fixed implementation.
- Produces: a concise record of root cause, fix, and verification evidence.

- [ ] **Step 1: Add final results to this plan.**

Add:

```markdown
## Final Results

- Root cause:
- Fix:
- Diagnostic flags kept:
- Diagnostic flags removed:
- Verification commands:
```

Fill each item with concrete evidence from the completed tasks.

- [ ] **Step 2: Run final verification.**

Run:

```bash
make stage2
target/twk run boot/tests/main.tw
target/twk lint boot/main.tw
```

Expected: all commands complete successfully.

- [ ] **Step 3: Report residual risks.**

Call out any remaining performance deferrals in the sound-uniqueness track, especially if the fix restores stage2 but does not fully optimize the summary algorithm.

- [ ] **Step 4: Commit.**

Use a short imperative commit subject. Example:

```bash
git add docs/plans/2026-07-30-sound-uniqueness-stage2-hang-investigation.md
git add boot/compiler/codegen/codegen.tw boot/compiler/codegen/variant_specialize.tw boot/compiler/codegen/mutable_produce.tw boot/compiler/codegen/ownership_verdicts.tw boot/compiler/summary.tw
git add boot/tests/suites docs/plans/sound-uniqueness/codegen/README.md
git commit -m "Diagnose sound-uniqueness stage2 hang"
```

Expected: commit includes the plan, fix, retained diagnostics, and regression coverage.
