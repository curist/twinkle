# Interprocedural MutVec — Slice 1 (caller-born sieve shape) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a flat `MutVec<fam>` handle flow into and out of one owned-specialized single-update wrapper clone across a loop, so `sieve` (`Vector<Bool>` written through `.set_at()`) stays flat end-to-end and reaches `sieve_mut`/flat-`MutVec` parity with no source change.

**Architecture:** A new post-`variant_specialize` interprocedural storage phase. It (1) discovers a producer-rooted call-thread region in the caller (a caller-born `collect`/`make` handle threaded through a routed owned clone call), (2) proves handle-continuation soundness with an explicit verifier, (3) upgrades the clone's param/return to a MutVec ABI (or a sibling `…$mv` clone when only some routed sites qualify), and (4) rewrites the caller region + clone body to flat `mutvec_*` ops, materializing at zero boundaries (no-escape scratch) or exactly one (escape). Any failure falls back to today's persistent `vector$set_in_place` ABI. This consumes existing ownership facts unchanged; it adds only codegen storage-selection analysis.

**Tech Stack:** Twinkle self-hosted compiler (`boot/`), boot test harness (`@std.testing`), `twk wat`/`twk ir` inspection, `make bundle-cli` self-host loop. Design source of truth: `docs/plans/interprocedural-mutvec.md` (HP-1…HP-5) and `docs/plans/sound-uniqueness/storage/README.md#s4`.

## Global Constraints

Copied verbatim from the design doc + repo house rules. Every task's requirements implicitly include this section.

- **Self-host gate at every behavior-changing step:** `make bundle-cli` self-host fixed point **+** `target/twk run boot/tests/main.tw` (full boot-test) **+** `cargo test --release` (rust-test) all green. Run these **sequentially, never concurrently or backgrounded**.
- **Non-behavior-changing tasks** (new dead-code module + tests only, phase not yet wired) require only boot-test green; note this explicitly in the task's gate step.
- **Fallback is always correct:** any rejected region / unproven condition / stale route → the site stays on the persistent `vector$set_in_place` ABI. Never emit a partial MutVec ABI. A non-S4 caller must never reach a MutVec ABI (HP-4 guard / HP-5 partitioning).
- **Reuse, don't fork, the family/repr machinery:** element family + physical repr come from the existing `PVecFamily` / `mutvec_*` op set (Int/Bool/Float/Byte already shipped). Do not hard-code to `set_at` or i64.
- **Determinism:** producer discovery in program order; sibling clone names deterministic (`…$mv`); module order stable (self-host fixed point is the proof).
- **Keep S2-local and S4 as separate passes.** Do not merge S4 into `mutvec_region.rewrite_module` (the S2 path is byte-identical-critical).
- **After editing any `.tw`:** run `target/twk fmt <file>` then `target/twk lint <entry>`; both clean before commit.
- **Materialization (slice 1):** no-escape scratch chain → **zero** `mutvec_freeze`; single escaping boundary → **exactly one** `mutvec_freeze`. Multi-exit / branch-carried handles are rejected (fallback), not handled.

---

## File Structure

- **Create** `boot/compiler/codegen/s4_region.tw` — producer-rooted call-thread region discovery (HP-1). Owns `S4Region`, `detect_call_thread_regions`, `classify_producer_prime`.
- **Create** `boot/compiler/codegen/s4_verify.tw` — interprocedural continuation verifier (HP-3). Owns `S4Verdict`, `verify_region`.
- **Create** `boot/compiler/codegen/s4_phase.tw` — the phase orchestrator (HP-2/HP-4/HP-5): consumes `SpecializeResult`, runs discovery + verify, produces `S4Decision` (ABI upgrades + route partition + rewrite plan), applies rewrite.
- **Modify** `boot/compiler/codegen/codegen.tw` — insert the S4 phase in `link_program` after `variant_specialize` (`:151`), before `convert_closures` (`:174`).
- **Modify** `boot/compiler/backend/mutvec_repr.tw` — consume `S4Decision` to type clone param/return slots + call-result slots as `MutVec<fam>` (HP-4).
- **Modify** `boot/compiler/codegen/variant_route.tw` / `variant_specialize.tw` — sibling-clone creation + route-site partition (HP-5), only if a partition is needed.
- **Create** `boot/tests/suites/s4_mutvec_suite.tw` — the S4 unit/integration suite; registered in `boot/tests/main.tw`.
- **Create** `boot/tests/fixtures/cfg/s4/*.tw` — sieve-shaped positive/negative fixtures.
- **Create** `examples/performance/awfy/twinkle/sieve_direct.tw` + register in `main.tw` — the flat-`MutVec` floor reference bench (non-buffer parity target).

---

### Task 0: S4 test harness, fixtures, and MutVec-floor reference bench

Scaffolding only — no compiler behavior change. Builds the inspection seam every later task tests against, using the **already-public** stage entrypoints so post-`builder_region`+post-`specialize` ANF and routes are inspectable before the S4 phase exists.

**Files:**
- Create: `boot/tests/suites/s4_mutvec_suite.tw`
- Create: `boot/tests/fixtures/cfg/s4/sieve_scratch.tw` (positive A), `sieve_escape.tw` (positive A′), `wrap_capture.tw` (negative: stores handle), `wrap_diff_return.tw` (negative: returns a different vector), `caller_alias.tw` (negative: pre-call alias survives), `wrap_multi_exit.tw` (negative: branch-carried handle → Task 3 condition 4), `split_callers.tw` (mixed owned + generic caller)
- Create: `examples/performance/awfy/twinkle/sieve_direct.tw`
- Modify: `boot/tests/main.tw` (register suite), `examples/performance/awfy/twinkle/main.tw` (register bench)

**Interfaces:**
- Consumes (verified present): `pipeline.compile_entry_path(path) Result<PipelineArtifacts, CompileError>`; `PipelineArtifacts{opt: AnfModule, env, builtins}` (`artifacts.tw:19-27`); `mutvec_region.rewrite_module(anf, builtins) AnfModule`; `builder_region.rewrite_module(anf, builtins) AnfModule`; `variant_specialize.specialize_module_with_sem(anf, builtins, sem) SpecializeResult`; **`SpecializeResult{anf, routes: Vector<VariantRoute>, ...}`** — the post-clone module field is `.anf` (NOT `.module`); `variant_route.VariantRoute{generic_func, clone_func, clone_name, route_sites: Vector<Int>}`; `pipeline.emit_wat(a) String`; `wat_func_body(wat, marker) String?` (copy from `variant_specialize_suite.tw:39`).
- Produces (for later tasks): `s4_stage(path) -> .{ prime: AnfModule, spec: SpecializeResult, builtins }` — a harness that reproduces `link_program`'s order up to and including `variant_specialize`, returning the ANF-prime module + routes + the builtin registry (later tasks need `builtins`). `count_op_calls(wat, marker, op_name) Int` — WAT op counter scoped to one function.

- [ ] **Step 0: Late-cliff discovery (cheap, do it first)** — grep boot's own compiler source for an owned-wrapper `collect`/`make` → routed-clone shape (a `xs = xs.<wrapper>(...)` in a loop over a locally-born vector). Record in the suite header comment whether any exist. If **none**, Task 7 is the first time S4 fires on real (non-fixture) code and its self-host fixed point is walking into unknown territory — flag that in Task 7's gate step so a convergence failure there isn't a surprise. If **some** exist, note them as the real self-host exercise.

- [ ] **Step 1: Write the reference bench** — `sieve_direct.tw`: a copy of `sieve.tw` with the inner write as a direct index-assign (`flags[k] = false`) instead of `flags = .set_at(k, false)`, and a header comment `// Flat-MutVec floor reference — keep in sync with sieve.tw (same body, direct write).` Same `warmup/iters/size/expected` constants.

- [ ] **Step 2: Register + run the reference bench**

Run: `target/twk fmt examples/performance/awfy/twinkle/sieve_direct.tw && target/twk run examples/performance/awfy/twinkle/main.tw 2>&1 | grep -E 'sieve(_direct)?\b'`
Expected: `sieve_direct` runs, checksum 669, and its ms is at/below `sieve` (this is the flat-`MutVec` floor; it should roughly match `sieve_mut`).

- [ ] **Step 3: Write the harness in the suite** — in `s4_mutvec_suite.tw`, implement `s4_stage(path)` by calling, in order: `compile_entry_path` → take `.opt` + `.builtins` → `mutvec_region.rewrite_module` → `builder_region.rewrite_module` → build `sem` exactly as `link_program` does at **`codegen.tw:125`**: `make_prelude_optimizer_semantics(builtins).with_ref_fields(build_ref_field_table(env))` (all three are `pub`: `opt/semantics.tw`, `resolver.tw`) → `variant_specialize.specialize_module_with_sem`. Return `.{ prime, spec, builtins }`. Also copy `wat_func_body` and add `count_op_calls`.

- [ ] **Step 4: Write the fixtures** — `sieve_scratch.tw` = the spike `run_wrap` shape returning `count` (no escape). `sieve_escape.tw` = same but `pub fn run(...) Vector<Bool>` returning `flags`. `wrap_capture.tw` = wrapper stores `xs` into a `Cell` before returning. `wrap_diff_return.tw` = wrapper returns a freshly-`collect`ed vector on one branch. `caller_alias.tw` = caller binds `snapshot := flags` before the loop and reads `snapshot` after. `split_callers.tw` = one owned caller + one generic (non-owned) caller of the same `set_at`.

- [ ] **Step 5: Write the harness smoke test** — a `.test("s4_stage compiles sieve_scratch", fn() { ... })` that calls `s4_stage(fixture_path("sieve_scratch"))` and asserts `spec.routes.len() >= 1` (the `set_at` route exists) and returns `.Ok({})`.

- [ ] **Step 5b: Write an inliner-fragility guard test (independent of S4)** — S4's entire premise is that the routed `set_at` call *survives as a plain call* post-`variant_specialize`; the just-landed ANF inliner (and the in-progress closure-devirtualizer) sit near this pipeline slot and could later eat that call shape, silently turning S4 into a no-op with **no test failure** (it'd just "safely" fall back and the whole feature goes dark). Add a standing test — not gated on S4 — asserting the sieve fixture's `set_at` clone call is still present post-`variant_specialize` (e.g. `wat_func_body(compile_fixture_wat("sieve_scratch"), "_run")` contains a `call ` to a `set_at__Bool` clone). If a future inliner change breaks the premise, this fails loudly instead of regressing a benchmark nobody watches.

- [ ] **Step 6: Register + run**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -3`
Expected: all boot tests pass, including the new smoke test.

- [ ] **Step 7: Gate (non-behavior-changing: boot-test only) + commit**

```bash
target/twk lint boot/tests/main.tw
git add boot/tests/suites/s4_mutvec_suite.tw boot/tests/fixtures/cfg/s4 boot/tests/main.tw examples/performance/awfy/twinkle/sieve_direct.tw examples/performance/awfy/twinkle/main.tw
git commit -m "test(s4): sieve fixtures, post-specialize inspection harness, flat-MutVec reference bench"
```

---

### Task 1: HP-1a — post-`builder_region` producer recognizer

The current `classify_producer` (`mutvec_region.tw:139`) runs **before** `builder_region`, so it matches a raw `collect`. S4 runs after, where the `collect` is a `builder_new → builder_push* → builder_freeze` chain. This task adds a recognizer for that post-`builder_region` form.

**Files:**
- Create: `boot/compiler/codegen/s4_region.tw`
- Test: `boot/tests/suites/s4_mutvec_suite.tw`

**Interfaces:**
- Consumes: `s4_stage(path).prime` (Task 0); ANF op vocabulary from `compiler.anf` (`AnfOp`, `AnfExpr`, `LocalId`); the builder-chain op names as emitted by `builder_region` — **discover these first** (Step 1).
- Produces: `s4_region.classify_producer_prime(func: AnfFunctionDef, handle: LocalId) MutVecProducer?` — returns `.CollectSeed`/`.MakeSeed`/`.ArrayLitSeed` when `handle` is a caller-born producer in ANF-prime, else `.None` (parameter, call-result, or unrecognized). Reuses `mutvec_region.MutVecProducer`.

- [ ] **Step 1: Discovery (record, don't guess)** — read `builder_region.rewrite_module` and its detect module (`builder_region_detect.tw`) and record, in a comment block at the top of `s4_region.tw`, the exact op sequence a `collect` becomes post-`builder_region` (builder_new/push/freeze op constructor names + how the frozen result binds to the handle local). Confirm against `s4_stage(fixture).prime` by dumping the `flags` def chain in a scratch test.

- [ ] **Step 2: Write the failing test**

```twinkle
.test("classify_producer_prime recognizes a post-builder_region collect", fn() {
  st := s4_stage(fixture_path("sieve_scratch"))
  run_fn := find_func(st.prime, "run")          // helper: locate the run function
  handle := run_fn.flags_handle()                // helper: the local bound to the frozen collect
  prod := s4_region.classify_producer_prime(run_fn, handle)
  try assert.equal(producer_name(prod.unwrap_or(.MakeSeed)), "collect")
  .Ok({})
})
```

- [ ] **Step 3: Run to verify it fails**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -i 'classify_producer_prime'`
Expected: FAIL (function/module not found).

- [ ] **Step 4: Implement `classify_producer_prime`** — walk the def of `handle` in `func.body`; accept when it is (a) a `builder_freeze` whose builder lineage traces back to a `builder_new` with no foreign aliasing (CollectSeed), (b) a `Vector.make` call (MakeSeed), or (c) an array literal (ArrayLitSeed). Reject when `handle` is a function parameter or a call result. Reuse the existing CollectSeed builder-chain tracing logic in `mutvec_region.tw` (the `builder_new_local` + `builder_aliases` + `freeze_temp` lineage around `mutvec_region.tw:455-456` — **note there is no function literally named `trace_collect_builder`; find the real internal helper**). Do not import the pipeline (avoid the cycle — keep the harness in the suite).

- [ ] **Step 5: Run to verify it passes**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -i 'classify_producer_prime'`
Expected: PASS.

- [ ] **Step 6: Gate (non-behavior-changing: boot-test only) + commit**

```bash
target/twk fmt boot/compiler/codegen/s4_region.tw && target/twk lint boot/main.tw
git add boot/compiler/codegen/s4_region.tw boot/tests/suites/s4_mutvec_suite.tw
git commit -m "codegen(s4): recognize post-builder_region caller-born producers (HP-1a)"
```

---

### Task 2: HP-1b — producer-rooted call-thread region discovery

**Files:**
- Modify: `boot/compiler/codegen/s4_region.tw`
- Test: `boot/tests/suites/s4_mutvec_suite.tw`

**Interfaces:**
- Consumes: `classify_producer_prime` (Task 1); `SpecializeResult.routes` + `VariantRoute.route_sites` (Task 0).
- Produces: `s4_region.S4Region = .{ handle: LocalId, producer: MutVecProducer, thread_calls: Vector<Int>, exit_kind: S4Exit }` where `S4Exit = { Scratch, EscapeReturn }`; and `s4_region.detect_call_thread_regions(func: AnfFunctionDef, spec: SpecializeResult) Vector<S4Region>` — for each caller-born producer handle threaded through one-or-more routed owned calls with the loop-carried rebind coming back from those calls, emit one `S4Region`. Program order; disjoint from S2 local-write regions.

- [ ] **Step 1: Write the failing test**

```twinkle
.test("detect_call_thread_regions finds the sieve set_at thread", fn() {
  st := s4_stage(fixture_path("sieve_scratch"))
  run_fn := find_func(st.prime, "run")
  regions := s4_region.detect_call_thread_regions(run_fn, st.spec)
  try assert.equal(regions.len(), 1)
  r := regions[0]
  try assert.equal(producer_name(r.producer), "collect")
  try assert.equal(r.thread_calls.len(), 1)     // the single set_at clone call
  try assert.true(exit_is_scratch(r.exit_kind)) // returns count, not flags
  .Ok({})
})
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -i 'detect_call_thread_regions'`
Expected: FAIL.

- [ ] **Step 3: Implement `detect_call_thread_regions`** — collect candidate producer handles via `classify_producer_prime`; for each, scan `func.body` for `AAssign(handle, call_result)` where the call target's site key is in some `route.route_sites` (a routed owned clone); require every other use of `handle` to be an in-region read (`.at`/`.len`) or such a rebind; classify exit as `Scratch` if `handle` never reaches a terminal/return and `EscapeReturn` if it is returned exactly once; reject (emit nothing) on any other use or multi-exit.

- [ ] **Step 4: Run to verify it passes**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -i 'detect_call_thread_regions'`
Expected: PASS.

- [ ] **Step 5: Add the escape-shape test** — same as Step 1 but `fixture_path("sieve_escape")`, asserting `regions.len() == 1` and `exit_is_scratch(r.exit_kind) == false`.

- [ ] **Step 6: Run + Gate (boot-test only) + commit**

```bash
target/twk fmt boot/compiler/codegen/s4_region.tw && target/twk lint boot/main.tw
target/twk run boot/tests/main.tw 2>&1 | tail -3
git add boot/compiler/codegen/s4_region.tw boot/tests/suites/s4_mutvec_suite.tw
git commit -m "codegen(s4): producer-rooted call-thread region discovery (HP-1b)"
```

---

### Task 3: HP-3 — interprocedural continuation verifier

Builds the verifier condition-by-condition (TDD per condition). Positive first, then one negative fixture per HP-3 condition. This is the risk-bearing task.

**Files:**
- Create: `boot/compiler/codegen/s4_verify.tw`
- Test: `boot/tests/suites/s4_mutvec_suite.tw`

**Interfaces:**
- Consumes: `S4Region` (Task 2); `SpecializeResult` (clone bodies + routes); the ANF vocabulary.
- Produces: `s4_verify.S4Verdict = { Accept, Reject(S4RejectReason) }`; `S4RejectReason = { NonContinuationReturn, AliasSurvives, CalleeRepublishes, MultiExit, ReprDisagree }`; `s4_verify.verify_region(region: S4Region, func: AnfFunctionDef, spec: SpecializeResult) S4Verdict`.

- [ ] **Step 1: Write the accept-path test**

```twinkle
.test("verify_region accepts the sieve scratch region", fn() {
  st := s4_stage(fixture_path("sieve_scratch"))
  run_fn := find_func(st.prime, "run")
  r := s4_region.detect_call_thread_regions(run_fn, st.spec)[0]
  try assert.true(verdict_is_accept(s4_verify.verify_region(r, run_fn, st.spec)))
  .Ok({})
})
```

- [ ] **Step 2: Run (fail), then implement the accept path** — `verify_region` returns `.Accept` when all five HP-3 conditions hold: (1) each thread call's returned value is the continuation token for the arg handle (callee is the routed owned clone whose ABI returns the same backing) and the caller rebinds the region handle to exactly that result; (2) no alias/copy of the pre-call value survives and the result is immediately `AAssign`ed back before any further handle use; (3) the clone body does not freeze/publish/store-into-record-or-global/capture/fork the handle; (4) all handle-carrying callee exits are materializable or the region is rejected; (5) caller and callee agree on element family + physical repr. Read the clone body via `spec` (look up `route.clone_func` in **`spec.anf`** — the post-clone module field).

- [ ] **Step 3: Run to verify accept-path passes**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -i 'verify_region accepts'`
Expected: PASS.

- [ ] **Step 4: Condition 3 negative — callee republishes**

```twinkle
.test("verify_region rejects a wrapper that captures the handle", fn() {
  st := s4_stage(fixture_path("wrap_capture"))
  run_fn := find_func(st.prime, "run")
  rs := s4_region.detect_call_thread_regions(run_fn, st.spec)
  // Either discovery rejects it outright, or the verifier does — both are correct.
  ok := rs.len() == 0 || reject_reason_is(s4_verify.verify_region(rs[0], run_fn, st.spec), "CalleeRepublishes")
  try assert.true(ok)
  .Ok({})
})
```

- [ ] **Step 5: Run (fail), implement condition-3 guard, run (pass).**

- [ ] **Step 6: Condition 1 negative — non-continuation return** — same shape with `wrap_diff_return`, expecting `NonContinuationReturn` (or discovery rejection).

- [ ] **Step 7: Run (fail), implement condition-1 guard, run (pass).**

- [ ] **Step 8: Condition 2 negative — pre-call alias survives** — same shape with `caller_alias`, expecting `AliasSurvives` (or discovery rejection).

- [ ] **Step 9: Run (fail), implement condition-2 guard, run (pass).**

- [ ] **Step 10: Condition 4 — materializable exits (make the discharge explicit, don't wave it off)** — condition 4 (every handle-carrying callee exit is materializable or the region is rejected) is discharged for slice 1 by the single-exit discovery rule in Task 2 (multi-exit / branch-carried handle → rejected at discovery). Add a `wrap_multi_exit.tw` fixture (a wrapper that returns `xs` on one branch and a different vector on another) and a test asserting `detect_call_thread_regions` yields **zero** regions for it (rejected at discovery). Keep a defensive `MultiExit` guard in `verify_region` too. This turns "folds into the accept path" into a literal passing test.

- [ ] **Step 11: Condition 5 — repr agreement (resolve, don't assert away; this one can TRAP)** — HP-4's backstop for a caller/callee repr mismatch is a runtime `illegal cast` **trap**, not a quiet fallback, so condition 5 must be positively discharged before this task is "done." Do **one** of:
  - (a) **Add a repr-mismatch negative fixture** — a caller of element family X routed against a clone specialized for family Y — and assert `verify_region` returns `Reject(ReprDisagree)` (never Accept). If such a shape is constructible in slice-1 scope, this is the required path.
  - (b) **If it is structurally unconstructible** (8G clones are per-monotype, so a routed owned caller and its clone always share element family — verify this against `elem_family.tw` + `variant_specialize`'s per-monotype cloning), write that justification as a citation-backed comment in `s4_verify.tw`, **and still keep the `ReprDisagree` guard in code** as a defensive assertion (cheap; guards against a future non-monotype clone path). Record which of (a)/(b) was taken in the commit message.

- [ ] **Step 12: Gate (boot-test only) + commit**

```bash
target/twk fmt boot/compiler/codegen/s4_verify.tw && target/twk lint boot/main.tw
target/twk run boot/tests/main.tw 2>&1 | tail -3
git add boot/compiler/codegen/s4_verify.tw boot/tests/suites/s4_mutvec_suite.tw
git commit -m "codegen(s4): interprocedural continuation verifier with negative guards (HP-3)"
```

---

### Task 4: HP-2 — wire the S4 phase into `link_program` (decision-only, inert)

Insert the phase so it discovers + verifies on every module, records an `S4Decision`, but performs **no ABI change and no rewrite yet** — every site falls back. This proves the phase is safe (byte-identical output) before it touches codegen. **First behavior-adjacent step → full self-host gate.**

**Files:**
- Create: `boot/compiler/codegen/s4_phase.tw`
- Modify: `boot/compiler/codegen/codegen.tw` (`link_program`, after `:151`, before `:174`)
- Test: `boot/tests/suites/s4_mutvec_suite.tw`

**Interfaces:**
- Consumes: `s4_region.detect_call_thread_regions`, `s4_verify.verify_region`, `SpecializeResult`.
- Produces: `s4_phase.S4Decision = .{ accepted: Vector<S4Region>, module: AnfModule, spec: SpecializeResult }`; `s4_phase.run_s4(spec: SpecializeResult, builtins) S4Decision` — in this task `module`/`spec` pass through unchanged; `accepted` is populated but unused downstream.

- [ ] **Step 1: Write the inertness test**

```twinkle
.test("run_s4 leaves the module byte-identical when nothing is applied", fn() {
  st := s4_stage(fixture_path("sieve_scratch"))
  dec := s4_phase.run_s4(st.spec, st.builtins)   // st.builtins threaded from Task 0 harness
  try assert.equal(anf_fingerprint(dec.module), anf_fingerprint(st.spec.anf))
  try assert.true(dec.accepted.len() >= 1)        // discovery still fires
  .Ok({})
})
```

- [ ] **Step 2: Run (fail), implement `run_s4`** — iterate module functions, run discovery + verify, collect accepted regions, but return `module`/`spec` unchanged. Add `anf_fingerprint` (stable string hash of the ANF) to the suite.

- [ ] **Step 3: Wire into `link_program`** — the current hand-off is `anf_spec := spec.anf` (`codegen.tw:164`) feeding `convert_closures` (`:174`). Insert `decision := s4_phase.run_s4(spec, builtins)` right after the `spec` block, and replace the `anf_spec := spec.anf` source with `anf_spec := decision.module` (== `spec.anf` unchanged in this task). Keep the existing variable flow otherwise identical.

- [ ] **Step 4: Inertness proof** — the in-process `anf_fingerprint` test (Step 1) *is* the correct and sufficient proof that `run_s4` changes nothing: it compares `dec.module` against `st.spec.anf` for the fixture. **Do NOT try to prove inertness by diffing `target/twk build` output before/after a `git stash`** — `target/twk` is the *frozen old* self-hosted binary (`target/boot.wasm`, see `Makefile` `stage2`/`bundle-cli`); toggling `.tw` source under it never runs the newly-wired S4 phase, and the new `s4_*` modules referenced from `link_program` shift function bodies/indices so such a diff spuriously fails (or falsely passes under DCE). The real end-to-end proof that the *new self-hosted* pipeline still converges is the `make bundle-cli` self-host fixed point in Step 5; if the phase is truly inert, boot's own output is unchanged and the fixed point holds on the first rebuild.

- [ ] **Step 5: Full self-host gate (sequential) + commit**

```bash
target/twk fmt boot/compiler/codegen/s4_phase.tw boot/compiler/codegen/codegen.tw && target/twk lint boot/main.tw
make bundle-cli
target/twk run boot/tests/main.tw 2>&1 | tail -3
cargo test --release 2>&1 | tail -5
git add boot/compiler/codegen/s4_phase.tw boot/compiler/codegen/codegen.tw boot/tests/suites/s4_mutvec_suite.tw
git commit -m "codegen(s4): wire inert interprocedural MutVec phase after variant_specialize (HP-2)"
```

---

### Task 5: HP-4 — MutVec-ABI decision input (clone param/return + call-result repr)

Now let the accepted decision type the clone's param + return as `MutVec<fam>` and propagate the caller call-result slot repr. Still no caller-region rewrite of the *writes* (Task 7) — this task lands the ABI typing so signatures flip, verified at the WAT signature level. Because it changes emitted types, it rides the self-host gate.

**Cross-stage plumbing (do this first):** `mutvec_repr` (`assign_mutvec_reprs`) runs in the backend **prepare** stage over `PreparedFunc`/`PreparedIR`, which is *downstream* of `link_program` where `S4Decision` is computed. So `S4Decision.abi_upgrades` must be **threaded from codegen into the prepare stage** — trace how `link_program`'s output reaches `prepare_codegen`/the backend and carry the upgrade table alongside it (a new field on the codegen→prepare hand-off, keyed by `clone_func`). This is real plumbing the design understates; budget a step for it.

**Files:**
- Modify: `boot/compiler/backend/mutvec_repr.tw` (new MutVec-ABI decision input above `assign_mutvec_reprs`; do not extend the local-slot pass)
- Modify: `boot/compiler/codegen/s4_phase.tw` (emit the ABI-upgrade records into `S4Decision`)
- Test: `boot/tests/suites/s4_mutvec_suite.tw`

**Interfaces:**
- Consumes: `S4Decision.accepted`; `route.clone_func`/`clone_name`.
- Produces: `s4_phase.S4AbiUpgrade = .{ clone_func: Int, param_slots: Vector<Int>, return_mutvec: Bool, fam: ElemRepr }`, carried on `S4Decision.abi_upgrades`; consumed by `mutvec_repr` to type those slots `MutVec<fam>` and the routed call-result slots at each caller site.

- [ ] **Step 1: Discovery** — read `mutvec_repr.tw` header + `:223`/`:261` and record how `route_typed_vec` types a frozen-`PVec` producer result, so the new MutVec-return path is a sibling of that, not an edit to it.

- [ ] **Step 2: Write the signature-level failing test**

```twinkle
.test("accepted sieve clone gets a MutVecBool param and result", fn() {
  wat := compile_fixture_wat("sieve_scratch")   // emit_wat through the full pipeline
  clone := wat_func_body(wat, "set_at__Bool").unwrap_or("")
  try assert.true(clone.contains("MutVecBool"))          // param/result typed MutVec
  try assert.true(!clone.contains("rt_arr__set_in_place")) // no persistent trie ABI
  .Ok({})
})
```

- [ ] **Step 3: Run (fail), implement the ABI-upgrade emission + `mutvec_repr` consumption** — for each accepted region's clone, mark param slots + return as `MutVec<fam>`; at each routed caller site, type the call-result slot `MutVec<fam>`. Guard: if any routed owned site for that clone is not in `accepted`, do **not** upgrade in place (defer to Task 6 partitioning) — for now, only upgrade when the clone's every routed site is accepted; otherwise leave persistent (fallback).

- [ ] **Step 4: Run to verify the signature test passes.**

- [ ] **Step 5: Full self-host gate (sequential) + commit**

```bash
target/twk fmt boot/compiler/backend/mutvec_repr.tw boot/compiler/codegen/s4_phase.tw && target/twk lint boot/main.tw
make bundle-cli
target/twk run boot/tests/main.tw 2>&1 | tail -3
cargo test --release 2>&1 | tail -5
git add boot/compiler/backend/mutvec_repr.tw boot/compiler/codegen/s4_phase.tw boot/tests/suites/s4_mutvec_suite.tw
git commit -m "backend(s4): MutVec-ABI param/return/call-result decision input (HP-4)"
```

---

### Task 6: HP-5 — route partitioning + sibling clone

When only some routed owned sites are S4-compatible, create a deterministic `…$mv` sibling clone with the MutVec ABI and reroute only the compatible sites; leave the 8G PVec-owned clone intact for the rest.

**Files:**
- Modify: `boot/compiler/codegen/variant_specialize.tw` / `variant_route.tw` (sibling creation + route-site partition)
- Modify: `boot/compiler/codegen/s4_phase.tw` (request the partition)
- Test: `boot/tests/suites/s4_mutvec_suite.tw`

**Interfaces:**
- Consumes: `VariantRoute.route_sites`; `S4Decision.accepted`.
- Produces: `s4_phase` requests a partition; a new `…$mv` clone id + a rerouted subset of `route_sites`. Non-S4 sites keep the original clone. Deterministic name `${clone_name}$mv`.

- [ ] **Step 1: Write the split-callers failing test**

```twinkle
.test("mixed callers: only the S4-compatible owned site gets the MutVec sibling", fn() {
  wat := compile_fixture_wat("split_callers")
  try assert.true(wat.contains("$mv"))                 // sibling exists
  mv := wat_func_body(wat, "$mv").unwrap_or("")
  try assert.true(mv.contains("MutVecBool"))
  base := wat_func_body(wat, "set_at__Bool_v").unwrap_or("")
  try assert.true(base.contains("rt_arr__set_in_place")) // original PVec clone intact
  .Ok({})
})
```

- [ ] **Step 2: Run (fail), implement partition + sibling** — when `accepted` covers a strict subset of a clone's `route_sites`: clone the owned clone as `${clone_name}$mv`, apply the MutVec ABI to the sibling only, move the accepted sites' route targets to the sibling, keep the rest. Respect the existing 8G variant cap + persistent fallback.

- [ ] **Step 3: Run to verify the split test passes.**

- [ ] **Step 4: Full self-host gate (sequential) + commit**

```bash
target/twk fmt boot/compiler/codegen/variant_specialize.tw boot/compiler/codegen/variant_route.tw boot/compiler/codegen/s4_phase.tw && target/twk lint boot/main.tw
make bundle-cli
target/twk run boot/tests/main.tw 2>&1 | tail -3
cargo test --release 2>&1 | tail -5
git add boot/compiler/codegen/variant_specialize.tw boot/compiler/codegen/variant_route.tw boot/compiler/codegen/s4_phase.tw boot/tests/suites/s4_mutvec_suite.tw
git commit -m "codegen(s4): sibling-clone route partitioning for mixed callers (HP-5)"
```

---

### Task 7: End-to-end rewrite + parity gate

Turn on the caller-region + clone-body rewrite: `flags[i]`→`mutvec_get`, `flags = flags.set_at(...)`→ threaded `mutvec_set` across the call, `collect`→ stays `MutVec` (no freeze). Enforce zero in-loop `mutvec_freeze` (scratch) / exactly one (escape), correct results, and `sieve` reaching the `sieve_direct` floor.

**Files:**
- Modify: `boot/compiler/codegen/s4_phase.tw` (apply the region + clone-body rewrite)
- Test: `boot/tests/suites/s4_mutvec_suite.tw`

**Interfaces:**
- Consumes: everything above. Produces: `S4Decision.module` now carries the rewritten caller region + clone body.

- [ ] **Step 1: Write the scratch parity + freeze-count test**

```twinkle
.test("sieve scratch stays flat: mutvec_set, zero freeze, correct result", fn() {
  wat := compile_fixture_wat("sieve_scratch")
  run_fn := wat_func_body(wat, "_run").unwrap_or("")
  clone := wat_func_body(wat, "$mv").unwrap_or(wat_func_body(wat, "set_at__Bool").unwrap_or(""))
  try assert.true(clone.contains("mutvec_set_bool"))
  // Count CALLS, not raw substrings: a surviving `rt_arr__mutvec_freeze_*` runtime
  // DEFINITION would inflate a substring count even with zero call sites. Use a
  // call-scoped counter (the `wat_has_call`/`rewritten_calls` idiom from existing suites).
  try assert.equal(count_op_calls(wat, "_run", "mutvec_freeze"), 0)   // no-escape scratch → zero calls
  try assert.true(run_fn.contains("mutvec_get_bool"))       // read went flat too
  .Ok({})
})
```

- [ ] **Step 2: Run (fail), implement the rewrite** — apply the flat rewrite to the accepted region in the caller and the clone body; the `collect` producer stays a `MutVec` builder (no freeze) for a Scratch exit; for `EscapeReturn`, insert exactly one `mutvec_freeze` at the boundary.

- [ ] **Step 3: Run to verify the scratch test passes.**

- [ ] **Step 4: Add the escape freeze-count test** — `compile_fixture_wat("sieve_escape")`, assert `count_op_calls(wat, "_run", "mutvec_freeze") == 1` (call-scoped, per Step 1's note).

- [ ] **Step 5: `mutable_produce` non-interaction (make the safety explicit, like S2 got)** — `mutable_produce.produce_mutable_decisions_seeded_with_sem` runs *after* S4's rewrite (`codegen.tw:188`) and joins on `mutable_catalog` FuncIds (`vector$set_unsafe` etc.). S4's rewritten clone body uses different FuncIds (`mutvec_set*`), so it *should* be inert there — but the analogous S2-vs-`builder_region` safety got an explicit justifying comment at `codegen.tw:135` and S4 has none. Add (a) a comment in `mutable_produce.tw` (or at the S4 call site) stating why an S4-claimed local yields no `ProducedDecision` (its writes are `mutvec_*`, not catalog set ops), and (b) a test asserting `produce_mutable_decisions` emits **zero** decisions for the sieve fixture's S4-rewritten `run`/clone.

- [ ] **Step 6: Correctness + parity check**

Run: `target/twk run examples/performance/awfy/twinkle/main.tw 2>&1 | grep -E 'sieve\b|sieve_direct|sieve_mut'`
Expected: `sieve` checksum 669 and its ms now at/near `sieve_direct` (the flat floor) — i.e. no longer the ~5× slower persistent path. If `sieve` regressed vs the pre-Task-7 number, a per-call freeze slipped in — fix before proceeding (this is the doc's headline risk).

- [ ] **Step 7: `queens` unchanged + full self-host gate (sequential)**

```bash
target/twk fmt boot/compiler/codegen/s4_phase.tw && target/twk lint boot/main.tw
make bundle-cli
target/twk run boot/tests/main.tw 2>&1 | tail -3
cargo test --release 2>&1 | tail -5
target/twk run examples/performance/awfy/twinkle/main.tw 2>&1 | grep -E 'queens|sieve'
```
Expected: all green; `queens` unchanged; `sieve` at parity.

- [ ] **Step 8: Commit + update the design doc**

```bash
git add boot/compiler/codegen/s4_phase.tw boot/tests/suites/s4_mutvec_suite.tw
git commit -m "codegen(s4): end-to-end flat-MutVec across the sieve set_at boundary — parity (HP-1..HP-5 slice 1)"
```
Then update `docs/plans/interprocedural-mutvec.md` (mark slice 1 landed) and, per repo convention, remove its row from `docs/plans/README.md` only when the *whole* S4 plan is done — for slice 1, leave the row and note "slice 1 landed; param-sourced `nbody` next".

---

## Self-Review

**Spec coverage (HP-1…HP-5 + test plan):**
- HP-1 (producer-rooted discovery, post-`builder_region`): Tasks 1–2. ✅
- HP-2 (post-`variant_specialize` phase, pass ordering): Task 4. ✅
- HP-3 (continuation verifier, all 5 conditions + negatives): Task 3. ✅ Conditions 1–3 have negative fixtures (Steps 4–9); condition 4 (materializable exits) has an explicit multi-exit rejection test (Step 10); condition 5 (repr agreement) is positively discharged — fixture or citation-backed justification + defensive guard (Step 11) — because its failure backstop is a runtime trap, not a silent fallback.
- HP-4 (MutVec-ABI decision input, param/return/call-result): Task 5, including the codegen→prepare cross-stage plumbing of the upgrade table. ✅
- HP-5 (route partitioning + sibling clone): Task 6. ✅
- Test plan A / A′ / B / C: A+A′ = Tasks 2/7; C = Task 6. **B (non-Int family, user wrapper): still no dedicated fixture** — the family-generality claim rests on "reuse family/repr, no i64 hard-coding" plus condition 5's repr check; add a `Float`/`Bool` user-`bump` fixture + test as an explicit Task 7 follow-up before claiming family-generality *proven*. (Left as the one acknowledged, deliberately-scheduled gap.)
- Signature-level inspection: Task 5. `mutable_produce` non-interaction: Task 7 Step 5. Perf/regression (sieve parity, queens unchanged, self-host): Tasks 5–7 gates. ✅
- Derisking order: inert-phase-first (Task 4) proven by an **in-process `anf_fingerprint` equality** (not a `target/twk` stash-diff, which cannot exercise the new phase); real self-host convergence rides the `make bundle-cli` gate. A Task-0 grep establishes whether Task 7 is the first real S4 firing on boot source. ✅

**Placeholder scan:** implementation steps that touch compiler internals (Tasks 1/3/5/6) open with an explicit **Discovery step** to read + record the real internal signatures rather than fabricate them; the tests and inspection commands are concrete and runnable. No "handle edge cases"/"TBD" left.

**Type consistency:** `S4Region`, `S4Exit`, `S4Verdict`, `S4RejectReason`, `S4Decision`, `S4AbiUpgrade`, `classify_producer_prime`, `detect_call_thread_regions`, `verify_region`, `run_s4` are used consistently across tasks; `MutVecProducer`/`ElemRepr`/`VariantRoute.route_sites` reuse verified existing types.

**Open item for the executor:** the harness helpers `find_func`, `flags_handle`, `exit_is_scratch`, `verdict_is_accept`, `reject_reason_is`, `anf_fingerprint`, `count_op_calls` (call-scoped, modeled on the existing `wat_has_call`/`rewritten_calls` idiom), `producer_name` are small suite-local utilities — write them in Task 0/Step 3 as needed; each is a few lines over the ANF/WAT the harness already returns.
