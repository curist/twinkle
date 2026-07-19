# Sound Uniqueness Sieve CFG Gap Investigation-Plus-Fix Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Investigate why the real AWFY sieve source does not currently render the unique-specialized `set_at` decision described in `docs/plans/sound-uniqueness/analysis/worked-examples.md`, add a failing test for each proven compiler gap before changing behavior, fix the smallest proven gap, and record the analogous conservative `graph_scc.visit` evidence as a secondary case.

**Architecture:** Start from real on-disk sources and current `target/twk ir --cfg` output, not synthetic snippets. First preserve the observed mismatch as evidence, then isolate which analysis stage loses the proof: thin-wrapper summary, collect freeze introduction, loop-carried fact merge, call-site ownership propagation, or CFG verdict rendering. Once a stage is proven wrong by a focused failing test, make the narrow production fix for that stage before continuing; passing isolation tests classify that stage as not the root cause. Treat `graph_scc.visit` as a related but lower-priority recursive/threaded-state case after sieve is understood.

**Tech Stack:** Twinkle boot compiler, `target/twk ir <file>.tw --cfg`, CFG ownership render, summary render, source search with `rg`, focused boot tests under `boot/tests/suites/` and fixtures under `boot/tests/fixtures/` only if the investigation needs executable regression coverage.

## Global Constraints

- Keep investigation evidence grounded in real source paths:
  - `examples/performance/awfy/twinkle/sieve.tw`
  - `boot/compiler/graph_scc.tw`
- Do not use unstable rendered function-id numbers as durable assertions.
- Preserve documentation-only evidence before changing production compiler behavior.
- This is investigation-plus-fix work: once a compiler gap is isolated, add or tighten a failing test first, then make the smallest production analysis change that makes that test pass.
- Do not use rendered `FuncId`, local (`LNN`), or block (`BNN`) numbers in durable test assertions. Local/block numbers are acceptable only in temporary archaeology commands and evidence notes that are explicitly regenerated from the current tree.
- Prefer stable CFG fragments in tests and reports: function names (`fn replace`, `fn loop_set_count`), `summary:`, role text (`Consumed paths{[]}`, `ret=alias(p0)`), `facts.in`, `facts.out`, `terminator: loop-back-edge`, `record_update`, `transport=`, and `verdict ->`/`unique:`.
- Before committing any fixture assertion, run `target/twk ir <fixture>.tw --cfg` once and compare the current render shape against the assertion text. If the render shape differs, update the assertion to match stable current text before adding the production fix.
- Assertions that reject `: Shared` must inspect only CFG fact lines, not an entire function section, so unrelated explanatory text or verdict details cannot cause false failures.
- Each task that changes tracked files ends with `git status --short` and a commit using the task's suggested message after its verification command passes.
- The primary question is not whether sieve has the right source shape; it does. The question is why current CFG analysis does not retain/render the unique proof.

---

## Current evidence snapshot

### Primary gap: real sieve `set_at` wrapper

Source:

```text
examples/performance/awfy/twinkle/sieve.tw
```

Relevant source shape:

```tw
flags = .set_at(k, false)
```

Regeneration command:

```bash
mkdir -p /tmp/twinkle-cfg-gap
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg > /tmp/twinkle-cfg-gap/sieve.cfg
```

Observed stable CFG fragments from the real source:

```text
fn run
  summary: p0=Borrowed ret=shared
  ...
  anf L55: call Fn33(L5)
  anf L13: init L55
  ...
  block B19 loop.header(L13, L16)
    facts.in={L13: Unknown, L16: Unknown} facts.out={L13: Unknown, L16: Unknown}
  ...
  anf L64: call Fn297(L13, L16, false)
  anf L65: assign L13 = L64
  ...
  block B24 if.join(L13, L16)
    facts.in={L13: Shared, L16: Unknown} facts.out={L13: Shared, L16: Unknown}
  ...
fn set_at__Bool
  summary: p0=Borrowed p1=Borrowed p2=Consumed paths{[]} ret=alias(p0,p2)
```

Expected worked-example intent:

```text
loop-carried flags should remain unique after collect freeze;
set_at__Bool should summarize the vector receiver as the consumed/returned value;
the call site should be eligible to render an owned-specialized verdict such as
verdict -> set_at[unique:...]
```

Concrete mismatch:

```text
No stable `verdict -> ...[unique:...]` is rendered for the real sieve call site.
The loop-carried vector local is `Unknown` at loop entry and later `Shared` at the join.
The rendered `set_at__Bool` summary attributes `Consumed paths{[]}` to p2, even though
source-level `set_at(xs, index, value)` should consume the vector receiver path, not the
Bool value argument.
```

### Secondary case: real `graph_scc.visit`

Source:

```text
boot/compiler/graph_scc.tw
```

Relevant source shapes:

```tw
cur.indices[node] = idx
cur.lowlinks[node] = idx
cur.stack = .append(node)
cur.on_stack[node] = true
cur = .visit(dep, edges)
cur.components = .append(component)
```

Regeneration command:

```bash
mkdir -p /tmp/twinkle-cfg-gap
target/twk ir boot/compiler/graph_scc.tw --cfg > /tmp/twinkle-cfg-gap/graph_scc.cfg
```

Observed stable CFG fragments from the real source:

```text
fn visit
  summary: p0=Published p1=Published p2=Published ret=alias(p0)
  ...
  record_get ... transport=borrow(... published)
  record_update ... shell=persistent(aliased shell) field=persistent(insufficient deep ownership)
  ...
  anf ... call Fn296(...)
  ...
  terminator: loop-back-edge ...
```

Interpretation:

```text
The real source has the expected recursive threaded-state shape: dict field updates,
vector field appends, recursion, branch/match joins, and loop back-edges. Current CFG
rendering is conservative/persistent for the generic function body. This is related to
sieve because both cases need ownership to survive through loops and call summaries, but
sieve is the smaller and more direct thin-wrapper failure.
```

---

### Task 1: Preserve focused evidence for the real sieve mismatch

**Files:**
- Read: `examples/performance/awfy/twinkle/sieve.tw`
- Read: `docs/plans/sound-uniqueness/analysis/worked-examples.md`
- Create if absent, otherwise modify: `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md`

**Interfaces:**
- Consumes: Current real-source CFG output for sieve.
- Produces: A short evidence note that future implementation work can cite without redoing the initial archaeology.

- [ ] **Step 1: Regenerate the current sieve CFG dump**

Run:

```bash
mkdir -p /tmp/twinkle-cfg-gap
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg > /tmp/twinkle-cfg-gap/sieve.cfg
```

Expected: command exits successfully and writes `/tmp/twinkle-cfg-gap/sieve.cfg`.

- [ ] **Step 2: Extract the stable mismatch fragments**

Run:

```bash
rg -n "^fn run|^fn set_at__Bool|summary:|loop\.header|facts\.in=.*(Unknown|Shared)|anf .*call .*Fn|anf .*assign .*|loop-back-edge|verdict ->" /tmp/twinkle-cfg-gap/sieve.cfg
```

Expected: output includes the loop-carried facts, the `set_at__Bool` summary, and the call/assign pair. Current regenerated evidence includes these temporary local ids:

```text
anf L64: call ...
anf L65: assign L13 = L64
facts.in={L13: Unknown, ...}
facts.in={L13: Shared, ...}
summary: p0=Borrowed p1=Borrowed p2=Consumed paths{[]} ret=alias(p0,p2)
```

- [ ] **Step 3: Compare against the worked-example claim**

Read the Case A section in:

```text
docs/plans/sound-uniqueness/analysis/worked-examples.md
```

Record this exact discrepancy in the note:

```markdown
The real sieve source still lowers to the expected loop-carried `set_at` wrapper
call, but current CFG ownership rendering does not prove the worked-example target
verdict. `flags` enters the inner loop as `Unknown`, becomes `Shared` at the join,
and `set_at__Bool` currently summarizes the Bool value parameter as consumed rather
than the vector receiver.
```

- [ ] **Step 4: Verify this task made no compiler behavior changes**

Run:

```bash
git diff -- boot/compiler src examples/performance/awfy/twinkle/sieve.tw
```

Expected: no production compiler or sieve source diff for this documentation-only task.

- [ ] **Step 5: Commit the preserved evidence**

Run:

```bash
git status --short
git add docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md
git commit -m "docs: preserve sieve CFG gap evidence"
```

Expected: only the evidence note is staged for this task's commit.

---

### Task 2: Isolate whether the thin-wrapper summary is wrong

**Files:**
- Read: `boot/compiler/summary.tw`
- Read: `boot/compiler/ownership.tw`
- Read: `boot/compiler/opt/semantics.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/vector_replace.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: `set_at__Bool` summary from `/tmp/twinkle-cfg-gap/sieve.cfg`.
- Produces: A focused fixture test proving whether vector index assignment summaries consume the receiver/base collection parameter.

- [ ] **Step 1: Locate summary construction for calls and consuming paths**

Run:

```bash
rg -n "Consumed paths|cow_base_arg|base_arg|IndexWrite|set_at|summary|ret_paths|alias\(" boot/compiler/summary.tw boot/compiler/ownership.tw boot/compiler/opt/semantics.tw
```

Expected: identify the code path that classifies a wrapper call and maps consumed paths to parameters.

- [ ] **Step 2: Add the focused vector replacement fixture**

Create `boot/tests/fixtures/cfg/sound_uniqueness/vector_replace.tw`:

```tw
pub fn replace(xs: Vector<Bool>, i: Int, value: Bool) Vector<Bool> {
  xs[i] = value
  xs
}
```

Expected: the fixture compiles and lowers to a thin wrapper shape with an index-write call, assignment back to `xs`, and return of `xs`.

- [ ] **Step 3: Preflight the fixture render shape before adding durable assertions**

Run:

```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/vector_replace.tw --cfg > /tmp/twinkle-cfg-gap/vector-replace-pre.cfg
rg -n "^fn replace|^fn set_at__Bool|summary:|anf .*call .*Fn|anf .*assign .*" /tmp/twinkle-cfg-gap/vector-replace-pre.cfg
```

Expected current pre-fix render includes:

```text
fn replace
summary: p0=Borrowed p1=Borrowed p2=Consumed paths{[]} ret=alias(p0,p2)
fn set_at__Bool
```

If this command does not compile, fix the fixture syntax before editing the test suite. If the summary already consumes `p0`, still add the regression test and helper below, but record that Task 2 is a coverage/classification task rather than a failing-test-first production fix.

- [ ] **Step 4: Add fact-line assertion helpers and the failing summary assertion**

In `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`, add this helper immediately after `section_from`:

```tw
fn assert_fact_lines_unique_without_shared(section: String, context: String) Result<Void, String> {
  saw_unique := false
  for line in section.lines() {
    if line.contains("facts.in=") or line.contains("facts.out=") {
      if line.contains(": Unique") {
        saw_unique = true
      }
      try assert.is_false(line.contains(": Shared"))
    }
  }
  try assert.ok(saw_unique, "expected Unique fact in ${context}")
  .Ok({})
}
```

Then add this test to `suite()` immediately before the existing `"Cell-backed dict update stays conservative"` test:

```tw
    .test(
      "vector index assignment summary consumes receiver not value",
      fn() {
        out := try render_entry("vector_replace")
        replace := try section_between(out, "fn replace", "fn set_at__Bool")
        try assert.str_contains(
          replace,
          "summary: p0=Consumed paths{[]} p1=Borrowed p2=Borrowed ret=alias(p0)",
        )
        try assert.is_false(replace.contains("p2=Consumed paths{[]}"))
        .Ok({})
      },
    )
```

Expected pre-fix failure: the rendered `fn replace` summary currently contains:

```text
summary: p0=Borrowed p1=Borrowed p2=Consumed paths{[]} ret=alias(p0,p2)
```

That failure proves the summary assigns consumption to the stored Bool value instead of the vector receiver.

- [ ] **Step 5: Run the focused test before changing implementation**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected before the production fix: the new `vector index assignment summary consumes receiver not value` test fails with a missing expected summary string or with the negative `p2=Consumed paths{[]}` assertion.

- [ ] **Step 6: Fix only the summary mapping after the failing test exists**

Change the summary logic so an index-write or vector `set_at` wrapper consumes the receiver/base collection parameter, not the stored value parameter.

Expected post-fix summary shape for both the fixture and real sieve wrapper:

```text
summary: p0=Consumed paths{[]} p1=Borrowed p2=Borrowed ret=alias(p0)
```

The exact surrounding function ids may differ, but the vector receiver must be the consumed parameter and the Bool value parameter must not be consumed.

- [ ] **Step 7: Re-run the focused test and sieve CFG after the summary fix**

Run:

```bash
target/twk run boot/tests/main.tw
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg > /tmp/twinkle-cfg-gap/sieve-after-summary.cfg
rg -n "^fn set_at__Bool|summary:|verdict ->|facts\.in=" /tmp/twinkle-cfg-gap/sieve-after-summary.cfg
```

Expected: the new test passes and `set_at__Bool` summary is corrected. If the real sieve still lacks a `verdict -> ...unique:` line or still shows the loop-carried vector degrading to `Unknown`/`Shared`, continue to Task 3.

- [ ] **Step 8: Commit the thin-wrapper summary classification or fix**

Run:

```bash
git status --short
git add boot/tests/fixtures/cfg/sound_uniqueness/vector_replace.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw \
  boot/compiler/summary.tw boot/compiler/ownership.tw boot/compiler/opt/semantics.tw
git commit -m "fix: classify vector index assignment ownership"
```

Expected: the commit contains the new failing-then-passing fixture test plus only the narrow production files needed for the proven summary fix. If the test proved this stage was already correct and no production fix was made, use `git commit -m "test: cover vector index assignment ownership"` instead.

---

### Task 3: Isolate whether collect freeze introduces a unique vector fact

**Files:**
- Read: `boot/compiler/ownership.tw`
- Read: `boot/compiler/cfg.tw`
- Read: `boot/compiler/summary.tw`
- Read: `boot/compiler/opt/semantics.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/collect_carry_loop.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: Corrected summary evidence from Task 2 and current facts around `anf ... call` / `init` for the sieve collect freeze.
- Produces: A concrete yes/no answer: does a collect-builder freeze become a `Unique` vector fact when moved into a loop-carried local before any mutation wrapper is involved?

- [ ] **Step 1: Record the real sieve pre-loop collect area without using ids as assertions**

Run:

```bash
rg -n "call Fn33|init L55|loop.header|facts\.in|facts\.out" /tmp/twinkle-cfg-gap/sieve-after-summary.cfg /tmp/twinkle-cfg-gap/sieve.cfg
```

Expected current archaeology: the real sieve dump shows the collect freeze call and the later loop headers, but it does not provide a stable, direct assertion point that proves the `flags` local is unique immediately after `init`. Treat this command as evidence gathering only, not as a regression assertion.

- [ ] **Step 2: Determine whether collect freeze is modeled as a fresh producer**

Run:

```bash
rg -n "builder|freeze|collect|fresh|Unique|introduce" boot/compiler/ownership.tw boot/compiler/summary.tw boot/compiler/opt/semantics.tw
```

Expected: identify whether collect-builder freeze is represented in optimizer semantics or ownership transfer as fresh/unique.

- [ ] **Step 3: Add a stronger collect-freshness fixture with an observable loop-carried vector fact**

Create `boot/tests/fixtures/cfg/sound_uniqueness/collect_carry_loop.tw`:

```tw
pub fn carry_flags(n: Int) Int {
  flags: Vector<Bool> = collect _ in range(n) { true }
  i := 0
  for i < n {
    flags = flags
    i = i + 1
  }
  i
}
```

Why this fixture exists: a simple `make_flags() Vector<Bool>` only proves the function summary can render `ret=fresh`; it does not expose the post-freeze local fact at a loop boundary. The self-assignment in `carry_flags` forces `flags` to be a loop-carried local while avoiding `set_at`, wrapper summaries, and mutation decisions. That isolates collect freeze plus move/loop transport.

- [ ] **Step 4: Preflight the collect-carry fixture render shape**

Run:

```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/collect_carry_loop.tw --cfg > /tmp/twinkle-cfg-gap/collect-carry-pre.cfg
rg -n "^fn carry_flags|summary:|loop.header|facts\.in=|facts\.out=|terminator: loop-back-edge|: Unique|: Shared" /tmp/twinkle-cfg-gap/collect-carry-pre.cfg
```

Expected current render includes at least one loop-carried fact line with `: Unique` and no fact line with `: Shared`. The fixture may also include a collect-builder lowering loop with `Unknown` facts before the post-freeze carry loop; `Unknown` in that earlier builder loop is not a collect-freeze failure.

- [ ] **Step 5: Add the collect-carry assertion**

In `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`, add this test to `suite()` immediately after the Task 2 vector replacement test:

```tw
    .test(
      "collect freeze remains unique when carried through a borrow-free loop",
      fn() {
        out := try render_entry("collect_carry_loop")
        carry := try section_from(out, "fn carry_flags")
        try assert.str_contains(carry, "terminator: loop-back-edge")
        try assert_fact_lines_unique_without_shared(carry, "carry_flags")
        .Ok({})
      },
    )
```

Expected current outcome: this test should pass if collect freeze introduction is already sound. If it fails by rendering no fact-line `: Unique` or by rendering fact-line `: Shared` for the carried vector, collect freshness/move/loop transport is a proven root cause and must be fixed before Task 4.

- [ ] **Step 6: Fix freshness introduction only if the collect-carry test fails**

If the new collect-carry test fails, adjust ownership transfer for the collect-builder freeze call and the subsequent move into the loop-carried local so the carried vector is introduced as unique/fresh. Do not change loop consume-produce merge logic in this task.

Expected post-fix evidence in `fn carry_flags`:

```text
terminator: loop-back-edge ...
facts.in={..., <flags local>: Unique, ...}
```

The local id is intentionally not asserted in the test; the durable assertion is that the `carry_flags` section contains a `: Unique` loop fact and no `: Shared` fact.

- [ ] **Step 7: Re-run sieve CFG after the collect-freshness classification**

Run:

```bash
target/twk run boot/tests/main.tw
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg > /tmp/twinkle-cfg-gap/sieve-after-freeze.cfg
rg -n "^fn run|^fn set_at__Bool|summary:|loop.header|facts\.in=|verdict ->" /tmp/twinkle-cfg-gap/sieve-after-freeze.cfg
```

Expected: the collect-carry test passes. If real sieve still lacks a unique `set_at` verdict after Tasks 2 and 3, continue to Task 4 because collect freeze has been isolated away from the remaining failure.

- [ ] **Step 8: Commit the collect-freshness classification or fix**

Run:

```bash
git status --short
git add boot/tests/fixtures/cfg/sound_uniqueness/collect_carry_loop.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw \
  boot/compiler/ownership.tw boot/compiler/cfg.tw boot/compiler/summary.tw boot/compiler/opt/semantics.tw
git commit -m "fix: preserve collect vector ownership facts"
```

Expected: the commit contains the collect-carry fixture plus only the narrow production files needed if the test exposed a freshness gap. If the test passed without a production fix, use `git commit -m "test: cover collect vector ownership facts"` instead.

---

### Task 4: Isolate whether loop-carried merge loses uniqueness

**Files:**
- Read: `boot/compiler/ownership.tw`
- Read: `boot/compiler/cfg.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/sieve_loop_set.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: Corrected wrapper summary from Task 2 and collect-carry uniqueness evidence from Task 3.
- Produces: A focused failing test, or a passing classification, for consume-produce assignment back to the same loop-carried vector local.

- [ ] **Step 1: Add the minimal loop-carried `set_at` fixture**

Create `boot/tests/fixtures/cfg/sound_uniqueness/sieve_loop_set.tw`:

```tw
pub fn loop_set_count(n: Int) Int {
  flags: Vector<Bool> = collect _ in range(n) { true }
  i := 0
  for i < n {
    flags = .set_at(i, false)
    i = i + 1
  }
  i
}
```

Why this fixture returns `Int`: returning the vector would publish it at function exit and add an unrelated source of conservatism. The fixture keeps the updated vector private, matching the real sieve property that `flags` is not returned.

- [ ] **Step 2: Preflight the loop-carried `set_at` fixture render shape**

Run:

```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/sieve_loop_set.tw --cfg > /tmp/twinkle-cfg-gap/sieve-loop-set-pre.cfg
rg -n "^fn loop_set_count|^fn set_at__Bool|summary:|loop.header|facts\.in=|facts\.out=|terminator: loop-back-edge|verdict ->|unique:|: Shared" /tmp/twinkle-cfg-gap/sieve-loop-set-pre.cfg
```

Expected current pre-fix render includes `fn loop_set_count`, `fn set_at__Bool`, a loop back-edge, and either no `verdict -> ...unique:` decision or a fact-line `: Shared` on the loop-carried vector. If this command does not compile, fix the fixture syntax before editing the test suite.

- [ ] **Step 3: Add the loop-carried consume-produce assertion**

In `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`, add this test to `suite()` immediately after the Task 3 collect-carry test:

```tw
    .test(
      "loop-carried vector set_at keeps unique decision when old value is dead",
      fn() {
        out := try render_entry("sieve_loop_set")
        loop_set := try section_between(out, "fn loop_set_count", "fn set_at__Bool")
        try assert.str_contains(loop_set, "terminator: loop-back-edge")
        try assert.str_contains(loop_set, "verdict ->")
        try assert.str_contains(loop_set, "unique:")
        try assert_fact_lines_unique_without_shared(loop_set, "loop_set_count")
        .Ok({})
      },
    )
```

Expected pre-fix failure if loop merge or call-site specialization is still wrong: the fixture section contains a `set_at` call/assign pair but no `verdict -> ...unique:` decision, or it contains fact-line `: Shared` for the loop-carried vector fact.

- [ ] **Step 4: Run the minimal fixture and real sieve side by side**

Run:

```bash
target/twk run boot/tests/main.tw
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/sieve_loop_set.tw --cfg > /tmp/twinkle-cfg-gap/minimal-loop-set.cfg
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg > /tmp/twinkle-cfg-gap/sieve-loop-check.cfg
rg -n "^fn loop_set_count|^fn set_at__Bool|summary:|loop.header|facts\.in=|verdict ->|unique:" /tmp/twinkle-cfg-gap/minimal-loop-set.cfg
rg -n "^fn run|^fn set_at__Bool|summary:|loop.header|facts\.in=|verdict ->|unique:" /tmp/twinkle-cfg-gap/sieve-loop-check.cfg
```

Expected: if `sieve_loop_set` passes but real sieve fails, the remaining issue is a sieve-specific branch/join or nested-loop shape. If both fail, the loop-carried consume-produce merge or call-site ownership propagation is the likely issue.

- [ ] **Step 5: Inspect merge and call-site decision behavior for consume-produce self assignment**

Run:

```bash
rg -n "merge|join|loop|back-edge|facts\.in|facts\.out|assign|select_variant|verdict|unique" boot/compiler/ownership.tw boot/compiler/cfg.tw boot/compiler/summary.tw
```

Expected: identify where facts from the loop body and loop header are joined, and where a summarized call with a consumed receiver selects a `unique:` verdict from caller facts.

- [ ] **Step 6: Fix only the proven loop/call-site gap**

If the Task 4 test fails, update only the narrow failing stage:

```text
- If the receiver fact is `Unique` before the `set_at` call but the rendered decision is missing, fix call-site variant selection or CFG verdict rendering.
- If the receiver fact is `Unique` before the body and becomes `Shared`/`Unknown` only at the back-edge, fix the loop fact merge so a local that is consumed, replaced by a unique result, and carried through the back-edge can remain unique when no old alias is live.
- If the receiver is already non-unique before the call despite Tasks 2 and 3 passing, inspect branch/join transfer before changing the merge.
```

Expected post-fix: the new `loop-carried vector set_at keeps unique decision when old value is dead` test passes, and the real sieve CFG is ready for final verification in Task 6.

- [ ] **Step 7: Commit the loop-carried ownership classification or fix**

Run:

```bash
git status --short
git add boot/tests/fixtures/cfg/sound_uniqueness/sieve_loop_set.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw \
  boot/compiler/ownership.tw boot/compiler/cfg.tw boot/compiler/summary.tw
git commit -m "fix: preserve loop-carried vector ownership"
```

Expected: the commit contains the loop-carried fixture plus only the narrow production files needed for the proven loop/call-site fix. If the test passed without a production fix, use `git commit -m "test: cover loop-carried vector ownership"` instead.

---

### Task 5: Document the `graph_scc.visit` secondary case after sieve is classified

**Files:**
- Read: `boot/compiler/graph_scc.tw`
- Read: `docs/plans/sound-uniqueness/analysis/worked-examples.md`
- Modify when classification requires wording changes: `docs/plans/sound-uniqueness/analysis/worked-examples.md`
- Modify when classification requires the investigation note: `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md`

**Interfaces:**
- Consumes: Sieve root-cause classification from Tasks 2-4.
- Produces: Clear documentation of whether `graph_scc.visit` is the same class of gap or a separate recursion/SCC-specialization gap.

- [ ] **Step 1: Regenerate current graph SCC CFG**

Run:

```bash
mkdir -p /tmp/twinkle-cfg-gap
target/twk ir boot/compiler/graph_scc.tw --cfg > /tmp/twinkle-cfg-gap/graph_scc.cfg
```

Expected: command exits successfully.

- [ ] **Step 2: Extract stable `visit` evidence**

Run:

```bash
rg -n "^fn visit|summary:|record_update|transport=|anf .*call .*Fn|terminator: match|terminator: loop-back-edge|verdict ->" /tmp/twinkle-cfg-gap/graph_scc.cfg
```

Expected current evidence includes:

```text
fn visit
summary: p0=Published p1=Published p2=Published ret=alias(p0)
record_update ... shell=persistent(aliased shell) field=persistent(insufficient deep ownership)
terminator: loop-back-edge ...
```

- [ ] **Step 3: Classify graph SCC relative to sieve**

Use this classification rule:

```text
If fixing sieve summary/freshness/loop merge also improves graph_scc.visit, document
it as the same ownership-propagation class. If sieve improves but graph_scc.visit
remains conservative, document graph_scc.visit as a separate recursive SCC-summary
specialization gap.
```

- [ ] **Step 4: Record graph SCC classification in the investigation note**

Append one of these exact bullets to `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md` under a `## graph_scc.visit classification` heading:

```markdown
- `graph_scc.visit` improved after the sieve fix, so it belongs to the same ownership-propagation class as the sieve gap.
```

or:

```markdown
- `graph_scc.visit` remains conservative after the sieve fix. Treat it as a separate recursive/SCC-summary specialization gap: the real source has the threaded-state shape, but current summaries still publish the state parameters and render persistent field updates.
```

or:

```markdown
- `graph_scc.visit` was documentation-only in this pass. No compiler behavior changed for this case.
```

- [ ] **Step 5: Update worked-example wording only if implementation remains conservative**

If current implementation intentionally does not yet support the unique-specialized
`graph_scc.visit` target, update `worked-examples.md` to distinguish:

```text
- observed real source shape today
- target verdict expected after recursive/SCC summary specialization
```

Do not weaken the source-shape finding; only clarify implementation status.

- [ ] **Step 6: Commit the graph SCC documentation classification**

Run:

```bash
git status --short
git add docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md \
  docs/plans/sound-uniqueness/analysis/worked-examples.md
git commit -m "docs: classify graph SCC ownership evidence"
```

Expected: the commit contains only documentation updates for the secondary case.

---

### Task 6: Final verification and reporting

**Files:**
- Any tests/docs changed by Tasks 1-5.

**Interfaces:**
- Consumes: Investigation changes.
- Produces: Evidence-backed final status.

- [ ] **Step 1: Run formatter on changed Twinkle files**

Run this command if Tasks 2-4 added the planned fixtures or changed any boot compiler/test source:

```bash
target/twk fmt \
  boot/tests/fixtures/cfg/sound_uniqueness/vector_replace.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/collect_carry_loop.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/sieve_loop_set.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw \
  boot/compiler/summary.tw \
  boot/compiler/ownership.tw \
  boot/compiler/cfg.tw
```

Expected: formatter succeeds. If a listed production compiler file was not changed, it is still safe to pass it to the formatter.

- [ ] **Step 2: Run focused CFG commands**

Run:

```bash
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg > /tmp/twinkle-cfg-gap/sieve-final.cfg
target/twk ir boot/compiler/graph_scc.tw --cfg > /tmp/twinkle-cfg-gap/graph_scc-final.cfg
```

Expected: both commands succeed.

- [ ] **Step 3: Run boot tests**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: tests pass.

- [ ] **Step 4: Run the linter on the boot test entry**

Run:

```bash
target/twk lint boot/tests/main.tw
```

Expected: linter completes. If it reports house-rule violations in files changed by this plan, fix them before reporting completion; if it reports pre-existing unrelated violations, record that they are unrelated and leave them unchanged.

- [ ] **Step 5: Check final tracked-file state**

Run:

```bash
git status --short
```

Expected: only intentional investigation/test/compiler/doc files are modified. There should be no untracked `/tmp/twinkle-cfg-gap` artifacts because all CFG dumps were written outside the repository.

- [ ] **Step 6: Report one of these concrete outcomes**

Report exactly which outcome applies:

```text
A. Sieve fixed: real sieve now renders a stable owned-specialized `set_at` verdict.
B. Sieve classified but not fixed: root cause identified with a failing test or doc note.
C. Sieve still unknown: evidence preserved, but root cause remains unresolved.
```

Also report graph SCC status separately:

```text
- graph_scc.visit improved with sieve fix
- graph_scc.visit remains a separate recursive/SCC-specialization gap
- graph_scc.visit was documentation-only and unchanged
```
