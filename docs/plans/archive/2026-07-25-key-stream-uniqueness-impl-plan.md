# Key-Stream Uniqueness Checker — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the unsound dedupe-helper recognizer shipped in `d05096e6` with the general **proof checker** specified in [2026-07-25-key-stream-uniqueness-design.md](../2026-07-25-key-stream-uniqueness-design.md), so a certified helper's output is provably duplicate-free — the key-stream-uniqueness half of the copy-carrier engine.

**Architecture:** A default-deny checker in `boot/compiler/ownership.tw` that certifies a function only with concrete evidence for obligations **O0–O4** (design §3), returning a `DedupeCertificate`. The one value-reasoning step (O3's short-circuit flag) is enabled by a minimal **analysis-only CFG-view jump-threading** simplification (§5.0) applied inside the classifier, after which O3 is a standard must-false dataflow. Driven test-first by the **adversarial negative battery** (design §7) — every counterexample from the seven design-review rounds is a failing classification test the checker must reject, while `insert_sorted`/`insert_sorted_str`/`int_keys_union` must certify.

**Tech Stack:** Twinkle boot compiler (`boot/`), self-hosted. **The design doc §5.x is the algorithm spec** — this plan references it per obligation rather than repeating code. Iterate with `target/twk run boot/tests/main.tw` (the boot-test path compiles edited source). Heavy commands (`make bundle-cli`) one at a time. After each `.tw` edit batch: `target/twk fmt <files>` then `target/twk lint boot/main.tw`.

---

## Starting state (already on branch `fixpoint-map-inplace`)

`d05096e6` shipped, and review found **unsound** (design §4, §0):
- `boot/compiler/ownership.tw`: `classify_dedupe_helpers`, `function_is_dedupe_helper`, `function_is_sort_insert_primitive`, `function_is_dedupe_combinator`, `find_set_once_flag`, `traces_to_index_of`, `block_guarded_by_not_flag`, `block_sets_flag_true`, `match_eq_operands`, `return_local_in_set`, `build_def_map`/`build_block_map`, `DictVecOps`/`dict_vec_ops`, `SummaryTable.dedupe_helpers: Dict<Int, Bool>` + `table_is_dedupe_helper`.
- `boot/compiler/summary.tw`: `with_dedupe_helpers` called at end of `compute` (full path only).
- `boot/tests/fixtures/sound_uniqueness/phase8g_dedupe_classify.tw` + a classification unit test in `boot/tests/suites/cfg_summary_suite.tw`.
- **No consumer** reads the classification yet (`table_is_dedupe_helper` unused) — nothing miscompiles today; engine-plan Task 4 is the first consumer and stays blocked until this plan's gates pass.

**Expected-red baseline (not caused by this plan).** The `phase 8A mutable decision production` suite (`boot/tests/suites/mutable_produce_suite.tw`) carries **10 deliberately-red assertions** for the copy-carrier consumer (committed pre-session in `0c8b4ce5 test: add borrow/effect copy-carrier fixtures and red assertions`; verified an ancestor of the branch tip). They fail with `ownership verdict did not certify reusable base` because the borrow/effect proof + in-place lowering are engine-plan Tasks 4–6, not built. These are **independent of the dedupe classifier**: no code in `codegen/mutable_produce.tw` or `codegen/ownership_verdicts.tw` reads `dedupe_helpers`/`table_is_dedupe_helper` (verified), so this plan's default-deny neither causes nor fixes them. They stay red until the copy-carrier consumer lands; do not disable them (they are that work's TDD target). This plan's own only-expected-red test is the positives half of the battery (green at Tasks 6–7).

This plan **rewrites** the checker. Keep the infra scaffolding (maps, `DictVecOps`) where still valid; replace all the certification logic.

---

## Cross-cutting discipline (applies to Tasks 3–7)

**ANF is non-SSA** — a local can be re-defined by `AAssign` (design §5.2). Two rules run
through every checker below and are easy to get wrong:
- **Reaching-definition, not last-write.** The function-wide `build_def_map` is trusted **only**
  for single-def locals; §5.0 threading (resolving `cond`→param) and §5.7 `!flag` freshness use
  **block-local, instruction-ordered** resolution.
- **Frozen evidence locals.** The counter, `element_local`, the `vp`-alias chain, the returned
  local, and the flag must be single-def (no `AAssign` beyond their sanctioned update);
  `alias_root` refuses links through `AAssign` targets. The `rebound_vparam` /
  `rebound_proof_local` / `rebound_alias_base` / `cross_block_stale_notflag` negatives exist to
  catch violations.

## File Structure

- `boot/compiler/ownership.tw` — the checker (types, threading, obligation checkers, classifier).
- `boot/compiler/summary.tw` — populate certificates on **all** summary paths (§5.9).
- `boot/compiler/codegen/ownership_verdicts.tw` — ensure the scoped/production path also gets a certified table.
- `boot/tests/fixtures/sound_uniqueness/` — the adversarial battery + positive fixtures.
- `boot/tests/suites/cfg_summary_suite.tw` — classification unit tests (positives certify; every negative rejected).

---

## Task 1: Land the full adversarial battery + positives as failing tests

**Files:**
- Create fixtures under `boot/tests/fixtures/sound_uniqueness/`:
  - **One self-contained battery file** `phase8h_ksu_battery.tw` (not the earlier `_pos`/`_neg_*` split): the compositional combinators need `insert_sorted` / `merge_two` in the same compilation unit, and the test classifies a single compiled unit — matching the `phase8g` precedent. It supersedes and removes `phase8g_dedupe_classify.tw`.
  - Positives: `insert_sorted` (verbatim from the real compiler, design §2); the `String`-element `insert_sorted_str` (design §5.10 gate 2 — pins that O1's OpKind-wildcarded equality evidence covers `Int` **and** `String`, both lowering to `ABinOp(.Eq/.Lt, …, opkind)`); the `union_via_insert` / `union_keys` combinators (the latter over `merge_two` at its certified arg-0 position); and `int_keys_union` under its production name. `merge_two` is a supporting certified combinator (unasserted).
  - Negatives, one function per design §7 battery item: `bad` (repeated id), `two_id_one_block`, `bad2` (fixed-index append), `nested_loop_append`, `fixed_index_eq_guard`, `second_in_loop_exit`, `continue_before_eq`, `unchanged_counter_backedge`, `join_side_effect` (threading must not fire), `rebound_vparam`, `rebound_proof_local`, `stale_notflag`, `rebound_alias_base`, `cross_block_stale_notflag`, `short_circuit_reconvergence_trap`, `wrong_arg_position` (combinator), `flag_reset_in_loop`, plus O0/O4: `param_rebind_combinator`, `wrong_return`, `extra_acc_mutation`, `bare_append_union`, `passthrough`.
- Modify `boot/tests/suites/cfg_summary_suite.tw`: **two** tests over the battery — one asserting **every** negative carries no certificate (must stay green from Task 2's default-deny on), one asserting **every** positive is certified with the right `kind` (expected red until Tasks 6–7).

- [ ] **Step 1:** Write the battery as a compilable `.tw` (each helper called from `main` so it isn't DCE'd). Design §7 gives the exact shape of each. `target/twk fmt` + `target/twk run` to confirm it compiles and runs.
- [ ] **Step 2:** Write the two classification tests (mirror the existing `phase8g` test in `cfg_summary_suite.tw`: `pipeline.compile_entry_path` → `cfg.build_view` → `ownership.classify_dedupe_helpers`). Split so the negatives-rejected safety posture and the positives-certify acceptance target are separately observable (before Task 2's type exists, assert via `table_is_dedupe_helper`; after, via certificate kind).
- [ ] **Step 3:** `target/twk run boot/tests/main.tw`. Expected: the negatives test is red against the current unsound checker (it wrongly certifies `bad`/`bad2`, design §4); the positives test passes. This red set is the target.
- [ ] **Step 4:** Commit (`test: adversarial battery for key-stream uniqueness certification`).

---

## Task 2: `DedupeCertificate` + default-deny classifier skeleton

**Files:** `boot/compiler/ownership.tw`, `boot/tests/suites/cfg_summary_suite.tw`, `boot/tests/suites/mutable_produce_suite.tw` (and `cfg_summary_suite` SummaryTable literals).

- [ ] **Step 1:** Add `DedupeKind`/`DedupeCertificate` (design §5.1). Change `SummaryTable.dedupe_helpers` to `Dict<Int, DedupeCertificate>` (design §5.9); update `empty_summary_table`, `table_is_dedupe_helper` (now: has-certificate), and the two `cfg_summary_suite` `SummaryTable.{…}` literals.
- [ ] **Step 2:** Replace `classify_dedupe_helpers`/`function_is_dedupe_helper` bodies with a **default-deny stub**: return an empty `Dict` (certify nothing). Delete the old unsound `function_is_sort_insert_primitive`/`function_is_dedupe_combinator`/`find_set_once_flag`/`block_sets_flag_true`/`match_eq_operands` bodies (keep `build_def_map`/`build_block_map`/`DictVecOps`/`dict_vec_ops`/`call_target_of`/`call_args_of`/`atom_local_id` helpers).
- [ ] **Step 3:** Update the two Task-1 tests to assert on the certificate (kind). `fmt` + `lint` + `target/twk run boot/tests/main.tw`. Expected: the **negatives test is green** (default-deny rejects everything), the **positives test is red** (nothing certified yet). This is the clean TDD baseline.
- [ ] **Step 4:** Commit (`ownership: DedupeCertificate + default-deny classifier skeleton`).

---

## Task 3: CFG-view jump-threading (§5.0), centralized in the classifier

**Files:** `boot/compiler/ownership.tw`, `boot/tests/suites/cfg_summary_suite.tw`.

- [ ] **Step 1:** Write a failing unit test: threading `insert_sorted`'s CFG rethreads the constant `B10` edge (the `if.join` fed `false`) to the false-target; a non-constant join is untouched; a `join_side_effect` block (instructions before the branch) is **not** threaded. Assert on block preds/edges of the threaded view.
- [ ] **Step 2:** Implement `thread_const_branches(view) CfgView` per design §5.0 (the exact pattern: `CondBranch(cond, T, [], F, [])`, `cond` a trivial-alias of a block param `p`, `B` has no non-alias instructions; rethread each pred edge supplying `ALitBool(c)` for `p`). Call it at the **top of `classify_dedupe_helpers`** so all callers share it (§5.9). Expose the test seam required by gate 5 (design §5.0): `thread_const_branches` as `pub`, **and an unthreaded classification mode** — a `pub fn classify_dedupe_helpers_unthreaded(...)` (or a `skip_threading` option threaded into `classify_dedupe_helpers`) that runs the identical certifier over the *un*-threaded view. Exposing `thread_const_branches` alone is insufficient — Task 9's verdict-equivalence check needs a real no-threading path through the full classifier.
- [ ] **Step 3:** `fmt`+`lint`+run. Threading test passes; classification still all-positives-red (checker still stub). Commit (`ownership: analysis-only CFG-view const-branch threading`).

---

## Task 4: Induction-variable identification (§5.3)

**Files:** `boot/compiler/ownership.tw`, `boot/tests/suites/cfg_summary_suite.tw`.

- [ ] **Step 1:** Unit test `find_induction` on `insert_sorted`: returns the counter `L6` and element `L7`; returns `.None` on `bad2` (fixed index), `unchanged_counter_backedge`, and a wrong-`bound` fixture.
- [ ] **Step 2:** Implement `find_induction` per §5.3 exactly: sole init pred (`ctr=0`); **every** back-edge advances `ctr` by one (follow the `ctr := t` source atom to a local `t` whose *def op* is `ABinOp(.Add, ctr, 1)` — real ANF shape, §5.3 "accepted advance"); `ctr >= len(vp)` (or `< len` inverse) polarity; element = `AIndex(alias_root==vp, ctr)`.
- [ ] **Step 3:** `fmt`+`lint`+run. Unit test passes. Commit.

---

## Task 5: Primitive O0 + O1 + O2 (design §5.4–§5.6)

**Files:** `boot/compiler/ownership.tw`, `boot/tests/suites/cfg_summary_suite.tw`.

- [ ] **Step 1:** Extend `certify_sort_insert_primitive` (returns `DedupeCertificate?`) to check, in order, **O0** (§5.4: fresh-`[]` accumulator; approved appends only; no `AAssign` to any param or `element_local`; returns are `vp` or lineage), **O1** (§5.5: equality early-return of `vp` on `element == id`; the equality false-edge dominates the induction increment; only in-loop return), **O2** (§5.6: exactly one static `append(element_local)`, not inside an inner loop/SCC). Still return `.None` (O3 pending) so positives stay red but the primitive negatives among the battery flip to *rejected-for-the-right-reason*.
- [ ] **Step 2:** Assert (unit-level, on the intermediate result) that `bad2`, `nested_loop_append`, `fixed_index_eq_guard`, `second_in_loop_exit`, `continue_before_eq`, `rebound_vparam`, `rebound_proof_local`, `wrong_return`, `extra_acc_mutation`, `unchanged_counter_backedge` are rejected by O0–O2. `fmt`+`lint`+run. Commit.

---

## Task 6: Primitive O3 flag dataflow (§5.7) — primitive certifies

**Files:** `boot/compiler/ownership.tw`, `boot/tests/suites/cfg_summary_suite.tw`.

- [ ] **Step 1:** Implement the **set-once flag** check (§5.7: exactly one `false` definition — counting `AInit(false)` AND `AAssign(flag,false)` — that dominates the loop header; assigned only `true` elsewhere), the **must-false dataflow** (`flag_false_entry`/`_exit` with the entry→exit transfer and the *current-not-stale* `!flag`-true-edge refinement), and the **instruction-sensitive acceptance** (intra-block walk; each id-append flag-false at its instruction; tail rule: sets flag after, or terminal post-loop). On success return the `SortInsertPrimitive` certificate (`vector_param`, `induction_local`, `element_local`, `flag_local`).
- [ ] **Step 2:** `fmt`+`lint`+`target/twk run boot/tests/main.tw`. Expected: **`insert_sorted` and `insert_sorted_str` both certify** (element-type-agnostic via the OpKind wildcard); `bad`, `two_id_one_block`, `stale_notflag`, `flag_reset_in_loop` are rejected. Positive `insert_sorted`/`insert_sorted_str` tests green; all primitive negatives green.
- [ ] **Step 3:** Commit (`ownership: sort-insert primitive certifier (O0-O3), proof-backed`).

---

## Task 7: Combinator O4 (§5.8) — `int_keys_union` certifies

**Files:** `boot/compiler/ownership.tw`, `boot/tests/suites/cfg_summary_suite.tw`.

- [ ] **Step 1:** Implement `certify_combinator` per §5.8 (seed from a vector param; every accumulator update is `acc = dh(…)` with `dh ∈ known` and the accumulator at `known[dh].vector_param`; no `AAssign` to vector params; return in lineage; ≥1 extension). Wire `classify_dedupe_helpers` to run primitive-then-combinator to a bounded fixpoint (§5.9).
- [ ] **Step 2:** `fmt`+`lint`+run. Expected: `union_via_insert`/`int_keys_union` certify as `Combinator`; `bare_append_union`, `passthrough`, `param_rebind_combinator`, `wrong_arg_position` rejected. **Entire battery green** now (all positives certify, all negatives rejected).
- [ ] **Step 3:** Commit (`ownership: dedupe combinator certifier (O4)`).

---

## Task 8: Populate certificates on all summary paths (§5.9)

**Files:** `boot/compiler/summary.tw`, `boot/compiler/codegen/ownership_verdicts.tw`.

- [ ] **Step 1:** Ensure `with_dedupe_helpers` (or an equivalent centralization) runs at the end of **both** `summary.compute` and the scoped `compute_for_roots_cached` path, so the production candidate path (`ownership_verdicts.tw`) gets a certified table (design §5.9 / review round 5 medium). Add a test compiling a fixture through the scoped path and asserting the certificate is present.
- [ ] **Step 2:** `fmt`+`lint`+`target/twk run boot/tests/main.tw`. Full suite green. Commit.

---

## Task 9: Rebuild + boot-main positives + threading equivalence (gates 2 & 5)

**Files:** none (verification), possibly a note in the design doc.

- [ ] **Step 1:** `make bundle-cli` (one at a time). Then a test asserting the boot compiler's own `insert_sorted`/`insert_sorted_str`/`int_keys_union` certify (compile `boot/main.tw`, classify, assert). This is design gate 2 (boot-main positives).
- [ ] **Step 2:** Gate 5 — verdict-equivalence: a test comparing the ownership analysis's rendered verdicts on a fixture *with* vs. *without* the §5.0 threading — the threaded default path vs. the **unthreaded classification mode** exposed in Task 3 (`classify_dedupe_helpers_unthreaded` / `skip_threading`) — asserting identical except at intended copy-carrier sites; and `target/twk run boot/tests/main.tw` unchanged (regression coverage).
- [ ] **Step 3:** Commit.

---

## Task 10: Gate 4 — independent review, then unblock the consumer

- [ ] **Step 1:** Dispatch an independent review (subagent + human) of the *implemented* checker against design §3 (O0–O4) and the battery, specifically hunting for a `.tw` function that certifies but produces duplicates. Address findings (each becomes a new battery fixture). Repeat until clean.
- [ ] **Step 2:** Only then unblock engine-plan Task 4 (its consumer reads `dedupe_helpers` for the stream-uniqueness fact). Update [2026-07-24-copy-carrier-engine-impl-plan.md](2026-07-24-copy-carrier-engine-impl-plan.md)'s Task-3/Step-4 banner to "done — see key-stream-uniqueness checker" and note the consumer is unblocked.
- [ ] **Step 3:** Commit.

---

## Self-Review

- **Spec coverage:** Every obligation O0–O4 and the §5.0 threading has a task (3–7), driven by the full §7 adversarial battery (Task 1) with the primitive/combinator theorems (§5.11) as the soundness target. Wiring on all summary paths (Task 8), boot-main + threading gates (Task 9), independent review before consumer (Task 10).
- **TDD order:** battery + positives red first (Task 1–2), then each obligation flips its negatives, positives certify at Task 6–7, battery fully green at Task 7.
- **Soundness posture:** default-deny throughout; the checker only ever *adds* certificates when evidence is complete; missing evidence ⇒ persistent (safe). No consumer until Task 10.
- **Placeholder scan:** algorithm detail lives in design §5.x (the spec); this plan pins files, fixtures, TDD order, and gates. Where a step says "per §5.x," that section contains the complete, review-hardened algorithm.
