# Rich Out-of-Bounds Trap Messages (Phase 3) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give a user's out-of-bounds `xs[i]` read and `xs[i] = v` write a source-mapped `index N out of bounds for length L` trace instead of the bare `unreachable`.

**Architecture:** A hidden `__panic_oob(index, len)` helper formats the message and traps through the existing `error` string-sink; an explicit `if idx u>= len` guard at the entry of the user-facing `rt.arr` get/set family functions calls it. The formatted message flows through Phase 2's existing `twinkleMessage` capture to the renderer as the headline (no renderer/host change), and Phase 4.1 already puts the caret on the `xs[i]` site.

**Tech Stack:** Twinkle (`.tw`, boot compiler), `boot/compiler/codegen/runtime/arr.tw` (`PVecFamily` builders), `boot/compiler/builtins.tw` (FuncId registry), Rust stage0 (`src/`, bootstrap only), `make stage2`/`bundle-cli`, `tools/js_runtime/cli.test.mjs`.

**Design:** `docs/plans/runtime-trace-rich-oob-messages.md` (read it — this plan implements it).

## Global Constraints

- Boot compiler in `boot/` is primary; touch Rust stage0 (`src/`) only as far as `make stage2` bootstrapping requires (Task 1/Task 2 verify).
- After editing any `.tw`: `target/twk fmt <file>` then `target/twk lint boot/main.tw` (the ~4 pre-existing findings in `section.tw`/`builtins.tw`/`census.tw` are unrelated — introduce no new ones).
- Heavy commands (`make stage2`, `make bundle-cli`) run **sequentially, foreground, never backgrounded/concurrent**.
- Never run full `cargo test`; targeted filters only. Boot suite: `target/twk run boot/tests/main.tw`.
- Fresh compiler after a stage rebuild is `target/twk` only after `make bundle-cli`; before that drive the just-built compiler with `BOOT_WASM=target/boot.wasm deno run -A tools/js_runtime/deno_main.mjs <args>`, and `cp target/boot.wasm tools/js_runtime/boot.wasm` before JS tests.
- **Scope:** user `xs[i]` read + `xs[i] = v` write only. NON-GOALS (do not implement): div0/rem0 guard, renderer kind-headline/ANSI polish, name demangling, `--strip-debug`, dict/string/slice/builder OOB, internal `rt.arr` invariant checks, any public `rt.panic` surface.
- **Message text (exact):** `index ${index} out of bounds for length ${len}`.
- **Comparison is unsigned** (`.I32GeU`) so a negative index (arriving as a large unsigned i32) is caught by the same single check.
- Commit style: imperative subject, what/why/how, no line/count metrics. Trailers on every commit:
  ```
  Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01FmurxQ6UfDKcPb5T9fWjfY
  ```

## File Structure

- `boot/compiler/codegen/runtime/arr.tw` — **modify.** Add the `__panic_oob` runtime helper (if mechanism ii) and the `if idx u>= len { __panic_oob(idx, len) }` guards in the `PVecFamily` get/set builders.
- `boot/compiler/builtins.tw` — **modify (only if mechanism i).** Reserve a FuncId for `__panic_oob`.
- A hidden prelude/runtime module for `__panic_oob` — **create (only if mechanism i).** Task 1 decides the exact home.
- `src/` (Rust stage0) — **modify only if `make stage2` requires it** (FuncId parity).
- `tools/js_runtime/cli.test.mjs` — **modify.** Add OOB read / write / negative-index e2e cases.
- `boot/tests/suites/…` (a codegen/arr suite) — **modify.** Assert the guard emits the `__panic_oob` call at the user get/set sites.
- `docs/plans/runtime-stack-traces.md` + `docs/plans/runtime-trace-rich-oob-messages.md` — **modify (Task 3).** Mark Phase 3 (OOB) done.

## Reference: the exact guard shape (established from `arr.tw`)

The mutvec read already carries the pattern to mirror (`arr.tw:2466-2495`): it computes `idx < 0 || idx >= len` and does `.If(.None, [.Unreachable], [])`. Two forms this plan uses:

- **Replace an existing cold arm** (mutvec get/set): swap `[.Unreachable]` for `[.LocalGet(<idx>), .LocalGet(<len>), <call __panic_oob>, .Unreachable]`.
- **Add an entry guard** (pvec get/set, which currently trap natively): after `len` is available in a local, insert
  ```tw
  .LocalGet(<idx_local>),
  .LocalGet(<len_local>),
  .I32GeU,
  .If(.None, [.LocalGet(<idx_local>), .LocalGet(<len_local>), <call __panic_oob>, .Unreachable], []),
  ```
  The trailing `.Unreachable` keeps the `.If` arm well-typed after the noreturn call. `<call __panic_oob>` is the call form Task 1 establishes (a `.Call("__panic_oob")`-style instruction).

---

## Task 1: Spike + land `__panic_oob` at one site

Prove the panic-helper call path end-to-end and land it at ONE representative user site (the `Vector<Int>` indexed read), so the wiring mechanism is decided and proven before broad rollout. This task's deliverable is **kept** code (not throwaway) plus a recorded mechanism decision.

**Files:** `boot/compiler/codegen/runtime/arr.tw`; maybe `boot/compiler/builtins.tw` + a hidden module (mechanism i); maybe `src/` (stage0 parity). Design doc (record the decision).

**Interfaces:**
- Produces: a callable `__panic_oob(index_i32, len_i32) -> (never)` reachable from `rt.arr` function bodies via a `.Call(...)` instruction; the exact instruction form + definition site are this task's recorded output, consumed by Task 2.

- [ ] **Step 1: Map the lowering.** Determine exactly which `rt.arr` function a `Vector<Int>` `xs[i]` **read** lowers to (expected: the `family_i64().pvec_get_fn` output, i.e. `get_i64`), and which functions `xs[i] = v` **write** lowers to (candidates: `pvec_set_fn` / `pvec_set_in_place_fn` / `get_leaf`, plus the typed-write promotion path). Record the precise function names in the design doc's Component 2 (Task 2 guards exactly these). Use `target/twk wat some/file.tw --func get_i64 --calls` and grep `arr.tw` to confirm.

- [ ] **Step 2: Prove the call path (pick mechanism i or ii).** No `rt.arr` function calls the string/error ops today, so establish one:
  - **Prefer (i):** define `__panic_oob(index: Int, len: Int) Never { error("index ${index} out of bounds for length ${len}") }` as a hidden Twinkle function, reserve a FuncId in `builtins.tw` (append at the end per FuncId-stability discipline), and emit `.Call("__panic_oob")` from one `rt.arr` site, letting the linker's `rename_func` (`linker.tw:146`) resolve it. Verify the emitted module links and validates.
  - **Fall back to (ii)** if (i) can't resolve across the runtime→prelude boundary or forces disproportionate stage0 work: emit `__panic_oob` as a `FuncDef` in the runtime layer (in `arr.tw` or a new `panic.tw` runtime module) whose body calls `int_to_string`, `string_concat`, and `__error_string` by name via the same `.Call("name")` mechanism `rt.arr` uses for its siblings.
  - **Record which mechanism won and why** in the design doc, plus the exact `<call __panic_oob>` instruction form and the helper's definition site — Task 2 reuses them verbatim.
  - If NEITHER path works without disproportionate effort, STOP and report BLOCKED with findings — do not force a shaky mechanism (the design's documented escape hatch is to re-scope to a renderer-only headline).

- [ ] **Step 3: Land the guard at the read site.** Add the entry guard (per "Reference: the exact guard shape") to the `Vector<Int>` read function only (`family_i64` `pvec_get_fn`), calling `__panic_oob`. Leave all other families/paths for Task 2.

- [ ] **Step 4: Bootstrap.** Sequentially, foreground:
  ```
  make stage2
  make bundle-cli
  cp target/boot.wasm tools/js_runtime/boot.wasm
  ```
  If `make stage2` fails on FuncId/arity parity (mechanism i), apply the minimal stage0 (`src/`) change so it bootstraps (the shipped compiler is boot-built; stage0 may keep emitting `.Unreachable` in its own `arr.rs` as long as stage2 converges), then re-run. Gate: `target/twk run boot/tests/main.tw` green.

- [ ] **Step 5: Prove e2e.** Create `/tmp/twk_p3_read.tw`:
  ```tw
  xs := [10, 20, 30]
  println(xs[5])
  ```
  Run `target/twk run /tmp/twk_p3_read.tw; echo "exit=$?"`. Expected: stderr shows `error: index 5 out of bounds for length 3`, a `-->` snippet + caret on the `xs[5]` line, and `exit=1`. Capture the output.

- [ ] **Step 6: fmt + lint + commit.**
  ```bash
  target/twk fmt boot/compiler/codegen/runtime/arr.tw   # + any new module / builtins.tw touched
  target/twk lint boot/main.tw
  git add -A   # include tracked rebuild byproducts (tools/bridge.wasm, bridge_bytes.mjs) if changed; NOT gitignored target/*, tools/js_runtime/boot.wasm, core_lib.tw
  git commit -m "feat(runtime): rich OOB message for Vector<Int> indexed read

Add a hidden __panic_oob(index,len) helper and an unsigned bounds guard at
the Vector<Int> read path so out-of-bounds xs[i] traps with
\"index N out of bounds for length L\" and a source-mapped snippet. Wiring
mechanism recorded in the design doc; broad rollout follows."
  ```

---

## Task 2: Extend guards to all user get/set paths + tests

Roll the proven guard out to every user-reachable indexed read/write across element families and the write paths, replace the mutvec cold arms, and add the codegen + e2e tests.

**Files:** `boot/compiler/codegen/runtime/arr.tw`; `tools/js_runtime/cli.test.mjs`; a codegen/arr boot suite.

**Interfaces:**
- Consumes: Task 1's recorded `<call __panic_oob>` instruction form and helper definition.

- [ ] **Step 1: Read guards for all families.** Apply the entry guard (same shape as Task 1) to the `pvec_get_fn` output for every family that a user read reaches — boxed / i64 / bool / float / byte — by adding it once in the family `pvec_get_fn` builder so it applies across families (Task 1 already covered i64; generalize the edit so all families emit it, not just i64). Confirm via `target/twk wat` that `get`/`get_bool`/`get_float`/`get_byte`/`get` (boxed) each contain the `__panic_oob` call.

- [ ] **Step 2: Write guards.** Add the guard to the write functions a user `xs[i] = v` reaches (the exact set recorded in Task 1 Step 1 — expected `pvec_set_fn` and `pvec_set_in_place_fn`; `set_in_place` must first load `len` via `StructGet(f.pvec_ty, pv_LEN)` since it lacks it). Use the same `idx u>= len` unsigned guard calling `__panic_oob(idx, len)`.

- [ ] **Step 3: mutvec cold arms.** In `pvec_mutvec_get_fn` (`arr.tw:2466`) and `pvec_mutvec_set_fn` (`arr.tw:2431`), replace the `[.Unreachable]` arm with `[.LocalGet(<idx>), .LocalGet(<len>), <call __panic_oob>, .Unreachable]` (free — the compare already exists).

- [ ] **Step 4: Codegen test.** In a codegen/arr boot suite, add an assertion that the emitted WAT for the user get and set functions contains a call to `__panic_oob` (or the recorded call target). Follow the existing `codegen_emit_suite`/`arr` suite pattern for building + scanning emitted WAT. Run `target/twk run boot/tests/main.tw` → PASS.

- [ ] **Step 5: Rebuild + e2e cases.** Sequentially: `make stage2 && make bundle-cli && cp target/boot.wasm tools/js_runtime/boot.wasm`; gate `target/twk run boot/tests/main.tw` green. Then add to `tools/js_runtime/cli.test.mjs` three cases via the existing `execFileSync("node", [entry, "run", trapPath], …)` harness (RENDERER_WASM via `buildRenderer`), each in a temp dir, asserting exit != 0 and a `-->…:N:` snippet + caret:
  - **read OOB:** `xs := [10,20,30]` then `xs[5]` → stderr matches `/index 5 out of bounds for length 3/`.
  - **write OOB:** a `Vector<Int>` write past the end (`xs[5] = 1`, in a form that reaches the guarded write path) → matches `/index 5 out of bounds for length 3/`.
  - **negative index:** `xs[-1]` read → matches `/out of bounds for length 3/` (proves the unsigned compare catches it).
  - **in-bounds control:** a program that indexes validly still exits 0.
  Run `deno test -A tools/js_runtime/cli.test.mjs` → PASS.

- [ ] **Step 6: Verify + benchmark.** `target/twk run boot/tests/main.tw` green; `cp target/boot.wasm tools/js_runtime/boot.wasm`; `deno test -A tools/js_runtime/` green (the known `web.test.mjs` env quirk aside). Run an existing indexing-heavy benchmark in `boot/bench/` before/after (compare against the pre-Task-1 `main`) and record that the read guard adds no meaningful regression. If a regression appears, report it with numbers.

- [ ] **Step 7: fmt + lint + commit.**
  ```bash
  target/twk fmt boot/compiler/codegen/runtime/arr.tw <suite files>
  target/twk lint boot/main.tw
  git add -A   # tracked byproducts only; no gitignored artifacts
  git commit -m "feat(runtime): rich OOB messages for all user vector index read/write

Extend the __panic_oob bounds guard across every element family's read and
the xs[i]=v write paths, and swap the mutvec cold arms to the rich message.
Adds codegen + twk-run e2e coverage (read/write/negative-index)."
  ```

---

## Task 3: Docs + status

**Files:** `docs/plans/runtime-stack-traces.md`, `docs/plans/runtime-trace-rich-oob-messages.md`.

- [ ] **Step 1: Mark Phase 3 (OOB) done.** In `docs/plans/runtime-stack-traces.md`, update the Status line and the `### Phase 3 — Rich messages via rt.panic` section to reflect that OOB rich messages landed (via the hidden `__panic_oob` guard, mechanism recorded in the design doc) and that div0/rem0 was deliberately skipped (native trap retained; documented future option). In `docs/plans/runtime-trace-rich-oob-messages.md`, set `Status: Complete (landed on <branch>)` and record the chosen wiring mechanism from Task 1.

- [ ] **Step 2: Commit.**
  ```bash
  git add docs/plans/runtime-stack-traces.md docs/plans/runtime-trace-rich-oob-messages.md
  git commit -m "docs(plans): mark Phase 3 rich OOB messages complete"
  ```

---

## Self-Review

**Spec coverage** (against `docs/plans/runtime-trace-rich-oob-messages.md`):
- Hidden `__panic_oob(index, len)` helper, spike-decided wiring → Task 1 Steps 2. ✓
- Explicit `idx u>= len` unsigned entry guard at user get/set → Task 1 Step 3 (read) + Task 2 Steps 1–2 (all families + write). ✓
- Mutvec cold-arm enrichment (free) → Task 2 Step 3. ✓
- Message flows through existing `error`/`twinkleMessage` capture; no renderer/host change → no renderer task present (correct). ✓
- Stage0 parity handled only as bootstrap requires → Task 1 Step 4. ✓
- div0 skipped, other Phase-4 items out of scope → Global Constraints NON-GOALS; no such tasks. ✓
- Tests: spike e2e, codegen assertion, read/write/negative e2e, benchmark → Task 1 Step 5, Task 2 Steps 4–6. ✓
- Escape hatch if wiring non-viable → Task 1 Step 2 BLOCKED path. ✓
- Docs/status → Task 3. ✓

**Placeholder scan:** The only intentionally-deferred specifics (the exact `__panic_oob` definition site and call instruction, and the exact write-path function set) are a spike's legitimate output — Task 1 records them and Task 2 consumes them verbatim; every other step has concrete code, commands, and acceptance criteria. The guard shape, unsigned compare (`.I32GeU`), noreturn-arm form, message text, and e2e assertions are all spelled out.

**Type consistency:** `__panic_oob(index_i32, len_i32) -> never` is defined in Task 1 and consumed by Task 2 with the same call form. Guard locals reference the family builders' existing `idx` param and `len` local (`pv_LEN`), consistent with the `arr.tw` `PVecFamily` shape. `.I32GeU`/`.If`/`.Unreachable`/`.LocalGet`/`.Call` are the `wasm_ir.tw` `Instr` variants used verbatim elsewhere in `arr.tw`.
