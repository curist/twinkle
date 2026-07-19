# Sound Uniqueness Analysis Gap Audit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce a durable gap-audit document for `docs/plans/sound-uniqueness/analysis/worked-examples.md` that records which examples are grounded by current `target/twk ir --cfg` output, which are only partially grounded, and what follow-up investigation each gap needs.

**Architecture:** Add a focused documentation artifact beside the existing sound-uniqueness plan docs. The artifact should map each worked-example case to its source origin, current CFG command, observed evidence, current confidence level, and a concrete follow-up path. This is documentation-only; it should not change compiler behavior or test expectations.

**Tech Stack:** Markdown docs, `target/twk ir <file>.tw --cfg`, repository source search with `rg`, optional temp CFG dumps under `/tmp`.

## Global Constraints

- Keep this documentation-only unless a later task explicitly asks for tests or compiler changes.
- Treat `docs/plans/sound-uniqueness/analysis/worked-examples.md` as a design anchor, not as proof that current boot analysis already emits every target verdict.
- Distinguish three statuses clearly: `confirmed`, `partial`, and `gap`.
- Do not depend on unstable `FuncId` values as durable evidence.
- Prefer exact source paths and function names over prose-only descriptions.
- When citing CFG output, record stable rendered fragments such as `summary:`, `ret_paths=`, `record_update`, `transport=`, `terminator: match`, `terminator: loop-back-edge`, and `verdict ->`.

---

### Task 1: Create the worked-example gap audit document

**Files:**
- Create: `docs/plans/sound-uniqueness/analysis/gap-audit.md`
- Read: `docs/plans/sound-uniqueness/analysis/worked-examples.md`
- Read: `docs/plans/sound-uniqueness/analysis/README.md`
- Read: `docs/plans/sound-uniqueness/README.md`

**Interfaces:**
- Consumes: Current worked-example case names and claims.
- Produces: A stable audit document that later workers can update as analysis precision improves.

- [ ] **Step 1: Re-read the canonical analysis docs**

Run:
```bash
target/twk run boot/tests/main.tw
```
Expected: The boot tests pass before documenting the audit baseline.

Then read these files completely:
```text
docs/plans/sound-uniqueness/README.md
docs/plans/sound-uniqueness/analysis/README.md
docs/plans/sound-uniqueness/analysis/worked-examples.md
```

- [ ] **Step 2: Create the audit file with status vocabulary**

Create `docs/plans/sound-uniqueness/analysis/gap-audit.md` with this header and status key:

```markdown
# Sound Uniqueness Worked-Example Gap Audit

**Status:** Current CFG evidence audit

This document tracks how well current boot-side CFG ownership rendering supports
`worked-examples.md`. It is intentionally separate from the worked examples:
that file remains a design anchor, while this file records what current tooling
can prove today and where follow-up investigation is still needed.

## Status key

- **confirmed** — current on-disk source plus `target/twk ir --cfg` renders the
  expected source shape and the relevant analysis evidence.
- **partial** — current CFG renders the important source shape, but not the full
  target verdict claimed or implied by the worked example.
- **gap** — no current on-disk source or stable CFG evidence has been identified.

## Regeneration commands

Use temp dumps when investigating large files:

```bash
mkdir -p /tmp/twinkle-cfg-gap-audit
target/twk ir <path>.tw --cfg > /tmp/twinkle-cfg-gap-audit/<name>.cfg
rg -n "^fn <function>|summary:|ret_paths=|verdict ->|record_update|transport=|terminator: match|terminator: loop-back-edge" /tmp/twinkle-cfg-gap-audit/<name>.cfg
```

Avoid using `FuncId(...)` values as durable audit evidence.
```

- [ ] **Step 3: Add the initial case matrix**

Add this table to `gap-audit.md`:

```markdown
## Case matrix

| Worked example | Source origin | Current status | Current evidence | Follow-up |
|---|---|---|---|---|
| Case A — sieve / `set_at` wrapper | `examples/performance/awfy/twinkle/sieve.tw` | partial | CFG shows loop-carried `flags` and `call Fn...(... false)` into `set_at__Bool`; current loop facts render `Unknown`/`Shared`, not a stable owned call-site verdict. | Investigate why the current boot CFG analysis does not render `verdict -> set_at[unique]` for this source. Decide whether this is an analysis precision gap, a render gap, or the worked-example target getting ahead of implementation. |
| Case B — `build_env` owned threading | `boot/tests/fixtures/cfg/sound_uniqueness/env_main.tw`, `env_lib.tw`; related historical synthetic suites | confirmed | Fixture CFG renders owned call-site evidence such as `verdict -> ...[unique:p0,p0.f0]` and `...[unique:p0,p0.f1]`. | Optionally find or add a real boot-compiler `ResolvedEnv` fixture if exact production-source grounding is desired. |
| Case C — `branch_env` old version observable | `boot/tests/fixtures/cfg/sound_uniqueness/env_main.tw` | partial | Fixture CFG renders shared/borrowed branch behavior and avoids an owned call-site verdict, but the source shape is fixture-adjusted rather than the exact `before := e; after := add_type(e)` shape in the doc. | Add an exact negative fixture if current analysis can represent the alias-read-after-call shape conservatively, or document why current render needs a same-block old-version read to expose the behavior. |
| Record case — `advance` scalar shell | `boot/stdlib/regexp/parse.tw` | confirmed | CFG for `advance` renders `summary: p0=Consumed paths{[],[.f1]} ...` and a scalar-field `record_update`. | Record the exact stable fragments; no compiler work implied. |
| Record case — `push_scope` nested vector/dict append | `boot/compiler/checker.tw`; `boot/compiler/lower_core/context.tw` | confirmed | CFG renders `push_scope` summaries consuming the context and `.locals` path, plus `record_get`, `Vector.append`, and `record_update`. | Capture both checker and lower-core versions because their field indices differ. |
| Case V — `graph_scc.visit` | `boot/compiler/graph_scc.tw`; simplified fixture in `boot/tests/fixtures/cfg/sound_uniqueness/visit_lib.tw` | partial | CFG renders recursive call, loop back-edges, match joins, dict/vector field updates, and persistent record-update verdicts; it does not currently render the unique-specialized variant target described in the worked example. | Investigate summary/SCC specialization versus generic fallback. Decide whether to update the worked example wording or add a follow-up analysis/codegen task. |
| Case W — transport-wrapper threading | Broad idiom in checker/lowering/resolver/query; representative evidence in existing summaries | partial | Current CFG often renders `ret_paths=.fN=from(pM)` and field-projection moves, but no single audit fixture covers all named wrapper families. | Add a representative matrix of real helper functions: one checker helper, one lowering helper, one resolver/query helper. |
| Case R — Result-wrapped state transport | `boot/compiler/query/analyze.tw`; fixture in `boot/tests/fixtures/cfg/sound_uniqueness/result_*` | confirmed | Real query CFG renders variant payload return paths such as `V0[0].f0=from(p0)` and `V1[0].f0=from(p0)` for `load_source`/`parse_cached`; fixture covers handled-result flow. | Keep both real-source and small-fixture evidence; confirm handled `.Err` versus early-return distinction remains clear. |
| Case T — `try` early-return publication | `boot/stdlib/crypto.tw`; `boot/stdlib/regexp/parse.tw` | confirmed | CFG renders `terminator: match`, error-variant construction, and `terminator: return` on error arms. | Optionally add a tiny dedicated fixture if the large stdlib parse/crypto dumps are too noisy. |
| Case Cell — `Cell<Dict>` get/set | `boot/compiler/unused_imports.tw`; `boot/compiler/opt/defer_elim.tw`; fixture in `boot/tests/fixtures/cfg/sound_uniqueness/cell_*` | confirmed | CFG renders `cell$get`, dict `set`, and `cell$set`; summaries publish/borrow conservatively. | Preserve as conservative baseline; do not chase in-place optimization for Cell contents. |
| Census baseline | Stage0 Rust test `cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture` | gap | Current boot-side CFG audit is not the same as the stage0 census; no deterministic boot-side census is documented here. | Later create a boot-side deterministic census/audit if needed. |
```

- [ ] **Step 4: Add interpretation guidance**

Add this section after the matrix:

```markdown
## Interpretation guidance

The main current gaps are not missing source shapes. They are places where the
current boot CFG renderer proves the shape but not the final target verdict from
`worked-examples.md`:

1. **Sieve / thin `set_at` wrapper:** the loop and wrapper call are present, but
   current facts do not show stable owned specialization.
2. **Real `graph_scc.visit`:** recursion, loops, joins, and record-field updates
   are present, but current rendered verdicts are conservative/persistent.
3. **Exact Case C alias negative:** fixture coverage exists, but the exact prose
   shape should be rechecked or added as its own fixture.
4. **Case W breadth:** return-path transport is visible, especially in query
   analysis, but the audit should identify one source example per major wrapper
   family rather than relying on broad prose.

When updating this audit, prefer adding the command used, the source path, the
function name, and one or two stable CFG fragments. Avoid copying large CFG dumps
into this document.
```

- [ ] **Step 5: Verify documentation formatting and links**

Run:
```bash
rg -n "TODO|TBD|FuncId\(" docs/plans/sound-uniqueness/analysis/gap-audit.md
```
Expected: No matches.

Run:
```bash
target/twk run boot/tests/main.tw
```
Expected: Tests still pass; this task is documentation-only.

---

### Task 2: Link the gap audit from the analysis README

**Files:**
- Modify: `docs/plans/sound-uniqueness/analysis/README.md`
- Read: `docs/plans/sound-uniqueness/analysis/gap-audit.md`

**Interfaces:**
- Consumes: `gap-audit.md` created in Task 1.
- Produces: Discoverable navigation from the analysis plan index.

- [ ] **Step 1: Find the existing document index section**

Run:
```bash
rg -n "worked-examples|summary|phase|README|Documents|docs" docs/plans/sound-uniqueness/analysis/README.md
```
Expected: Identify the list or paragraph where analysis supporting documents are linked.

- [ ] **Step 2: Add the gap-audit link**

Add a link near the existing `worked-examples.md` reference:

```markdown
- [`gap-audit.md`](gap-audit.md) records which worked examples are currently
  confirmed by boot `--cfg` output, which are partial, and which need follow-up
  investigation.
```

If the README does not use a bullet list, add one short sentence instead:

```markdown
For the current evidence status of each worked example, see
[`gap-audit.md`](gap-audit.md).
```

- [ ] **Step 3: Verify link target exists**

Run:
```bash
test -f docs/plans/sound-uniqueness/analysis/gap-audit.md
```
Expected: exit code 0.

Run:
```bash
target/twk run boot/tests/main.tw
```
Expected: Tests still pass.

---

### Task 3: Optional follow-up fixture plan for partial cases

**Files:**
- Create only if requested later: `docs/plans/sound-uniqueness-analysis-partial-fixtures.md`
- Read: `docs/plans/sound-uniqueness/analysis/gap-audit.md`
- Read: `docs/plans/sound-uniqueness-fixture-tests.md`

**Interfaces:**
- Consumes: The gap list from `gap-audit.md`.
- Produces: A separate implementation plan for adding or refining executable fixtures.

- [ ] **Step 1: Decide whether documentation is enough**

Do not create more tests automatically. First classify the partial cases:

```markdown
- Sieve: real source exists; likely analysis/render precision gap.
- Graph SCC visit: real source exists; likely summary/SCC specialization gap.
- Exact Case C: likely fixture-shape gap.
- Case W breadth: likely documentation/audit granularity gap.
```

- [ ] **Step 2: If tests are requested, write a separate fixture plan**

The separate plan should propose one independently reviewable task per partial case:

```markdown
1. Add exact Case C negative fixture.
2. Add source-backed AWFY sieve CFG assertion, marked with the current conservative evidence if not yet expected to be unique.
3. Add graph_scc source-backed assertion for loops/recursion/update shape, not unique-specialized verdict unless implementation changes first.
4. Add transport-wrapper fixture or real-source audit assertions for one checker, one lowering, and one query helper.
```

- [ ] **Step 3: Keep compiler behavior unchanged unless explicitly requested**

Any plan that changes analysis/codegen must be separate from fixture/documentation work and should start with a new design decision: whether the worked-example target verdict is intended to be implemented now or deferred.

---

## Self-review checklist

- [ ] Every worked-example family from `worked-examples.md` has a row in the audit matrix.
- [ ] Rows distinguish real source evidence from fixture evidence.
- [ ] Partial rows explain what is proven today and what is not proven today.
- [ ] No row depends on `FuncId` as stable evidence.
- [ ] The analysis README links to the new audit.
- [ ] Final verification command is recorded with its actual output before claiming completion.
