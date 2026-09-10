# MutDict Evidence Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the Candidate M representation gate using boot-compiler workload evidence and complete, same-session arena-versus-persistent measurements.

**Architecture:** Extend the existing throwaway dual-arena benchmark rather than creating a second harness. Keep Candidate H as a diagnostic control, add the missing phase boundaries and persistent trace, and base the decision on Candidate M versus persistent `Dict` for the insert-heavy and low-overwrite workloads found in the optimized boot compiler.

**Tech Stack:** Twinkle boot compiler, Wasm GC runtime IR, `target/twk ir`, `target/twk wat`

**Spec:** `docs/plans/mutdict-dense-freeze-input.md`

## Global Constraints

- The boot runtime is authoritative; do not add stage0 parity for this spike.
- Hash and trace preparation, correctness scans, printing, and source-level Vector preparation stay outside timed regions.
- Candidate allocations and GC stay inside timed regions.
- Candidate M remains provisional; Candidate H does not reopen boxed retained storage without a separate reviewed design decision.
- Do not begin the open-addressing runtime unless the completed evidence explicitly selects Candidate M.
- Do not run tree-sitter tests.

---

### Task 1: Record the boot-compiler workload census

**Files:**
- Modify: `docs/plans/mutdict-dense-freeze-input.md`
- Modify: `docs/plans/sound-uniqueness/storage/README.md`

**Interfaces:**
- Consumes: optimized ANF candidate census and CFG ownership view from `target/twk ir boot/main.tw --opt --census --sites` and `--cfg`
- Produces: a static workload classification that identifies representative benchmark rows without claiming dynamic execution frequencies

- [ ] Run the optimized census and record the `dict_set`/`dict_remove` totals and selected-site character.
- [ ] Inspect representative selected loop-carried functions in the CFG ownership view and their source.
- [ ] Classify insertion, overwrite, removal, publication, and flat-clone behavior conservatively.
- [ ] Document why the census makes construction and low-density updates decision-critical.

### Task 2: Add correctness coverage for the complete trace

**Files:**
- Modify: `boot/bench/mutdict_arena_layout_spike.tw`
- Modify: `boot/compiler/codegen/runtime/dict.tw`
- Modify: `boot/compiler/builtins.tw`
- Modify: `boot/prelude/signatures/dict.tw`

**Interfaces:**
- Consumes: existing stable-ID Candidate M/H arena operations and `freeze_dense`
- Produces: benchmark results for Candidate M, Candidate H, and persistent `Dict`, plus named phase timings

- [ ] Extend the benchmark assertions first to require three observationally identical publications and the expanded timing schema.
- [ ] Run `target/twk run boot/bench/mutdict_arena_layout_spike.tw` and confirm the assertion fails because the persistent result/timings are absent.
- [ ] Add the minimal persistent trace implementation using identical logical keys, values, overwrite order, and churn order.
- [ ] Add post-churn clone isolation checks for both arena candidates before timing integration.
- [ ] Run the focused benchmark at warmup scale and confirm all correctness guards pass.

### Task 3: Add missing timing boundaries and rows

**Files:**
- Modify: `boot/compiler/codegen/runtime/dict.tw`
- Modify: `boot/bench/mutdict_arena_layout_spike.tw`

**Interfaces:**
- Consumes: the correctness-complete three-way trace from Task 2
- Produces: construction, overwrite, churn, dense/churned clone, seam, HAMT, order, publication, and total timings

- [ ] Time Candidate M/H construction while keeping trace/hash preparation outside the timer.
- [ ] Clone the completed 1.25n churned physical arena and time deep versus shallow clone behavior.
- [ ] Add timing boundaries around the bottom-up HAMT builder and bulk order builder without changing `freeze_dense` semantics.
- [ ] Report complete Candidate M/H workload totals and same-session persistent totals.
- [ ] Add `k/n = 1/8` rows at 65K and 1M while retaining 1x, 4x, and churn rows.
- [ ] Rotate candidate/control order and retain warmup plus three raw samples per row.

### Task 4: Verify emitted behavior and collect measurements

**Files:**
- Modify: `docs/plans/mutdict-dense-freeze-input.md`
- Modify: `docs/plans/sound-uniqueness/storage/README.md`
- Modify: `docs/plans/sound-uniqueness/storage/spike-tier0-dict.md`

**Interfaces:**
- Consumes: expanded benchmark output and emitted WAT
- Produces: reviewed evidence and an explicit select-M or stop verdict

- [ ] Format every changed `.tw` file twice and confirm the second pass is clean.
- [ ] Lint `boot/main.tw` and confirm no new finding from the spike.
- [ ] Rebuild the boot compiler and standalone CLI as required by changed runtime/prelude surfaces.
- [ ] Inspect WAT calls and relevant function bodies against the representation shape guards.
- [ ] Run all reported rows and preserve raw ranges and medians without outlier subtraction.
- [ ] Compare Candidate M end-to-end against persistent `Dict` on construction-heavy, 1/8×, 1×, 4×, and churn workloads.
- [ ] Record the workload census, phase results, cost interpretation, and explicit reviewed verdict in the plan and storage documents.
- [ ] Run the focused boot tests and relevant Rust verification before claiming completion.
