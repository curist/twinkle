# Sound Uniqueness Sieve CFG Gap Investigation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Investigate why the real AWFY sieve source does not currently render the unique-specialized `set_at` decision described in `docs/plans/sound-uniqueness/analysis/worked-examples.md`, and record the analogous conservative `graph_scc.visit` evidence as a secondary case.

**Architecture:** Start from real on-disk sources and current `target/twk ir --cfg` output, not synthetic snippets. First preserve the observed mismatch as evidence, then isolate which analysis stage loses the proof: collect freeze introduction, loop-carried fact merge, thin-wrapper summary, call-site ownership propagation, or CFG verdict rendering. Treat `graph_scc.visit` as a related but lower-priority recursive/threaded-state case after sieve is understood.

**Tech Stack:** Twinkle boot compiler, `target/twk ir <file>.tw --cfg`, CFG ownership render, summary render, source search with `rg`, focused boot tests under `boot/tests/suites/` and fixtures under `boot/tests/fixtures/` only if the investigation needs executable regression coverage.

## Global Constraints

- Keep investigation evidence grounded in real source paths:
  - `examples/performance/awfy/twinkle/sieve.tw`
  - `boot/compiler/graph_scc.tw`
- Do not use unstable rendered function-id numbers as durable assertions.
- Do not change production compiler behavior while documenting the gap.
- If production analysis changes become necessary, add failing tests first.
- Prefer stable CFG fragments: `summary:`, `facts.in`, `facts.out`, `anf ... call`, `terminator: loop-back-edge`, `record_update`, `transport=`, and `verdict ->`.
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
- Create or modify if requested by this task: `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md`

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
rg -n "^fn run|^fn set_at__Bool|summary:|facts\.in=\{L13|facts\.in=\{L13: Shared|anf L55: call|anf L13: init|anf L64: call|anf L65: assign|loop-back-edge|verdict ->" /tmp/twinkle-cfg-gap/sieve.cfg
```

Expected: output includes the loop-carried `L13` facts, the `set_at__Bool` summary, and the call/assign pair. Current expected evidence includes:

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
git diff -- boot compiler src examples/performance/awfy/twinkle/sieve.tw
```

Expected: no production compiler or sieve source diff for this documentation-only task.

---

### Task 2: Isolate whether the thin-wrapper summary is wrong

**Files:**
- Read: `boot/compiler/summary.tw`
- Read: `boot/compiler/ownership.tw`
- Read: `boot/compiler/opt/semantics.tw`
- Optional test target: `boot/tests/suites/cfg_summary_suite.tw`
- Optional test target: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: `set_at__Bool` summary from `/tmp/twinkle-cfg-gap/sieve.cfg`.
- Produces: A yes/no answer: is the summary assigning consumption to the wrong parameter?

- [ ] **Step 1: Locate summary construction for calls and consuming paths**

Run:

```bash
rg -n "Consumed paths|cow_base_arg|base_arg|IndexWrite|set_at|summary|ret_paths|alias\(" boot/compiler/summary.tw boot/compiler/ownership.tw boot/compiler/opt/semantics.tw
```

Expected: identify the code path that classifies a wrapper call and maps consumed paths to parameters.

- [ ] **Step 2: Add or identify a minimal wrapper summary check**

If no existing test directly checks a vector receiver wrapper, add a failing test that compiles a small fixture equivalent to:

```tw
pub fn replace(xs: Vector<Bool>, i: Int, value: Bool) Vector<Bool> {
  xs[i] = value
  xs
}
```

Expected failing assertion before any fix:

```text
summary should mention p0=Consumed paths{[]} and ret=alias(p0), not p2=Consumed paths{[]}
```

- [ ] **Step 3: Run the focused test before changing implementation**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: the new focused test fails if the summary bug is real and unhandled.

- [ ] **Step 4: Fix only the summary mapping if the test proves it is wrong**

Change the summary logic so an index-write or vector `set_at` wrapper consumes the receiver/base collection parameter, not the stored value parameter.

Expected post-fix `set_at__Bool` summary shape:

```text
summary: p0=Consumed paths{[]} p1=Borrowed p2=Borrowed ret=alias(p0)
```

The exact order/format may differ, but the vector receiver must be the consumed parameter.

- [ ] **Step 5: Re-run sieve CFG after the summary fix**

Run:

```bash
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg > /tmp/twinkle-cfg-gap/sieve-after-summary.cfg
rg -n "^fn set_at__Bool|summary:|anf L64: call|verdict ->|facts\.in=\{L13" /tmp/twinkle-cfg-gap/sieve-after-summary.cfg
```

Expected: `set_at__Bool` summary is corrected. If `L13` is still `Unknown`/`Shared`, continue to Task 3.

---

### Task 3: Isolate whether collect freeze introduces a unique vector fact

**Files:**
- Read: `boot/compiler/ownership.tw`
- Read: `boot/compiler/cfg.tw`
- Read: `boot/compiler/summary.tw`
- Optional test target: `boot/tests/suites/cfg_ownership_facts_suite.tw`

**Interfaces:**
- Consumes: Current facts around `anf L55: call ...` and `anf L13: init L55` in sieve.
- Produces: A yes/no answer: does the collect-builder freeze result become `Unique` before the loop?

- [ ] **Step 1: Extract the pre-loop collect freeze area**

Run:

```bash
rg -n "anf L55: call|anf L13: init|block B2|block B12|facts\.in|facts\.out" /tmp/twinkle-cfg-gap/sieve.cfg
```

Expected current evidence includes:

```text
anf L55: call ...
anf L13: init L55
block B12 loop.header(L14, L15)
facts.in={L14: Unknown, L15: Unknown}
```

- [ ] **Step 2: Determine whether the freeze call is recognized as a fresh vector producer**

Run:

```bash
rg -n "builder|freeze|collect|fresh|Unique|introduce" boot/compiler/ownership.tw boot/compiler/summary.tw boot/compiler/opt/semantics.tw
```

Expected: identify whether collect-builder freeze is represented in optimizer semantics as fresh/unique.

- [ ] **Step 3: Add a tiny collect-freeze ownership test if missing**

Use a source shape equivalent to:

```tw
pub fn make_flags(n: Int) Vector<Bool> {
  flags := collect _ in 0..n { true }
  flags
}
```

Expected assertion after CFG rendering:

```text
The vector result from collect freeze should be treated as fresh/unique until published at return.
```

- [ ] **Step 4: Fix freshness introduction only if the test proves it is missing**

Adjust ownership transfer for the collect-builder freeze call so the result is introduced as unique/fresh.

- [ ] **Step 5: Re-run sieve CFG after the freshness fix**

Run:

```bash
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg > /tmp/twinkle-cfg-gap/sieve-after-freeze.cfg
rg -n "anf L55: call|anf L13: init|block B19|facts\.in=\{L13|anf L64: call|verdict ->" /tmp/twinkle-cfg-gap/sieve-after-freeze.cfg
```

Expected: `L13` should no longer enter the inner loop as `Unknown` solely because the collect result was not introduced as unique. If it still degrades, continue to Task 4.

---

### Task 4: Isolate whether loop-carried merge loses uniqueness

**Files:**
- Read: `boot/compiler/ownership.tw`
- Read: `boot/compiler/cfg.tw`
- Optional test target: `boot/tests/suites/cfg_ownership_facts_suite.tw`
- Optional fixture target: `boot/tests/fixtures/cfg/sound_uniqueness/`

**Interfaces:**
- Consumes: Corrected wrapper summary and collect freshness evidence from Tasks 2 and 3.
- Produces: A yes/no answer: does the loop fixpoint/merge preserve uniqueness across consume-produce assignment back to the same loop-carried local?

- [ ] **Step 1: Build a minimal loop-carried vector fixture if the real sieve is still too noisy**

Create a fixture equivalent to:

```tw
pub fn loop_set(n: Int) Vector<Bool> {
  flags := collect _ in 0..n { true }
  i := 0
  for i < n {
    flags = .set_at(i, false)
    i = i + 1
  }
  flags
}
```

Expected behavior after Tasks 2 and 3:

```text
The loop-carried vector remains unique across the back-edge, and the `set_at` call site can select an owned variant.
```

- [ ] **Step 2: Run the minimal fixture and real sieve side by side**

Run:

```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/sieve_loop_set.tw --cfg > /tmp/twinkle-cfg-gap/minimal-loop-set.cfg
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg > /tmp/twinkle-cfg-gap/sieve-loop-check.cfg
```

Expected: if the minimal fixture passes but real sieve fails, the remaining issue is a sieve-specific branch/join or nested-loop shape. If both fail, the loop-carried merge is the likely issue.

- [ ] **Step 3: Inspect merge behavior for consume-produce self assignment**

Run:

```bash
rg -n "merge|join|loop|back-edge|facts\.in|facts\.out|assign" boot/compiler/ownership.tw boot/compiler/cfg.tw
```

Expected: identify where facts from the loop body and loop header are joined.

- [ ] **Step 4: Add a failing test for the loop-carried merge if needed**

The test should assert stable CFG evidence, not `FuncId` values:

```text
- loop header for the vector local does not render `Unknown`
- call site renders `verdict -> ...[unique:...]`
```

- [ ] **Step 5: Fix merge only after the failing test exists**

Update the loop fact merge so a local that is consumed, replaced by a unique result, and carried through the back-edge can remain unique when no old alias is live.

---

### Task 5: Document the `graph_scc.visit` secondary case after sieve is classified

**Files:**
- Read: `boot/compiler/graph_scc.tw`
- Read: `docs/plans/sound-uniqueness/analysis/worked-examples.md`
- Modify only if requested: `docs/plans/sound-uniqueness/analysis/worked-examples.md`
- Modify only if requested: `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md`

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

- [ ] **Step 4: Update worked-example wording only if implementation remains conservative**

If current implementation intentionally does not yet support the unique-specialized
`graph_scc.visit` target, update `worked-examples.md` to distinguish:

```text
- observed real source shape today
- target verdict expected after recursive/SCC summary specialization
```

Do not weaken the source-shape finding; only clarify implementation status.

---

### Task 6: Final verification and reporting

**Files:**
- Any tests/docs changed by Tasks 1-5.

**Interfaces:**
- Consumes: Investigation changes.
- Produces: Evidence-backed final status.

- [ ] **Step 1: Run formatter if any `.tw` files changed**

If Task 4 added the optional fixture, run:

```bash
target/twk fmt boot/tests/fixtures/cfg/sound_uniqueness/sieve_loop_set.tw
```

Expected: formatter succeeds. If no `.tw` files changed, skip this step and record that it was skipped because the work was documentation-only.

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

- [ ] **Step 4: Report one of these concrete outcomes**

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
