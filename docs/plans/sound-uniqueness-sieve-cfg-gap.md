# Sound Uniqueness: Vector In-Place Verdict for `sieve` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the boot compiler's CFG ownership analysis render a sound `verdict -> fN[unique:p0]` in-place decision for a loop-carried vector updated through the `set_at` wrapper (the AWFY `sieve` shape), by teaching two analysis stages about vector in-place updates the way they already understand record-field updates.

**Architecture:** The record-field-update path already works end to end (`env_main` renders `verdict -> f295[unique:p0,p0.f0]`). The vector path is the missing analog and fails at two precise, independently-testable points: (Gap B) the requirement-flow analysis (`collect_field_reqs`) never marks a vector base as dirtied, because `xs[i]=v` lowers to the `VECTOR_SET` **builtin** and builtins break the flow chain — so `set_at`'s `p0` never becomes `Consumed` with a non-empty `in_place_paths`, so `select_variant` can never key on it; (Gap A) the forward ownership transfer for a COW update **merges the stored element's provenance into the result's shell provenance** (`absorb_retained_call_args`), so `set_at`'s return is `MayAliasParams([0,2])` (two params) which blocks the Stage 4a whole-return move — so the loop-carried vector publishes to `Shared` instead of moving and staying `Unique`. Fix Gap B first (render-only, census-neutral), then Gap A (changes ownership facts → codegen → census, so it is soundness-gated), then verify real `sieve` renders the verdict.

**Tech Stack:** Twinkle boot compiler (`boot/compiler/ownership.tw`, `boot/compiler/variant_id.tw`), `target/twk ir <file>.tw --cfg`, boot test suites under `boot/tests/suites/`, fixtures under `boot/tests/fixtures/cfg/sound_uniqueness/`, the COW census guard (`cargo test --release cow_analysis`), and the self-host loop (`make stage2`).

---

## STATUS: implemented (2026-07-19) — Gaps B and A landed; loop-nesting residual remains

All six tasks executed on branch `uniqueness-rewrite-from-scratch`. Boot suite 3082 green,
COW census passing (2110 ≤ re-baselined 2200), self-host reaches a fixed point
(stage3 == stage4), lint clean of changed files.

**Done (both gaps landed exactly on target):**
- **Gap B** (`transfer_flow` / `cow_update_result_fact`, commit `d10a24be`): a COW `.Update`
  builtin now dirties its `cow_base_arg` at the shell, so `xs[i]=v` wrappers become in-place
  `Consumed`. Generalizes to `Dict.set`/`Dict.remove`/builder-push, not just vectors.
- **Gap A** (`escape_retained_call_args`, commit `ef55a010`): scoped to the `.Update` call
  site only (the shared `absorb_retained_call_args` and the `.Allocate` path are byte-identical).
  `set_at__Bool` now summarizes `p0=Consumed paths{[]} p1=Borrowed p2=Published ret=alias(p0)`,
  the Stage 4a whole-return move fires, and a unique+last-use vector stays `Unique`.
- **Verified behavior:** straight-line `set_at`, two sequential `set_at`s on a reused unique
  vector, AND a **single loop** carrying the vector all render `verdict -> fN[unique:p0]` and
  keep the vector non-`Shared`. The single-loop case converges even with an interleaved read
  (`if flags[i] { ... }`). Fixtures: `vector_replace`, `vector_once`, `vector_twice`,
  `vector_escape`, `sieve_loop_set`.

**Two deviations from this plan as originally written (both intended, both verified):**
1. **Gap A publish is gated on `!single_retention`, NOT unconditional.** The plan mandated an
   unconditional `publish_atom` on the stored operand. That over-published: it clobbered a clean
   unique move's ownership and spuriously dropped the result's `[Elem]` field fact, breaking the
   pre-existing "storing an owned inner into an all-owned vector keeps [Elem]" test — a
   pessimization (more COW), the opposite of this work's goal. The shipped code publishes only
   when the operand is NOT `single_retention` (own==Unique && last-use && single-store), the same
   predicate the `.Update` field-fact block reads to keep/drop `[Elem]`. Invariant: the result
   keeps `[Elem]` iff the operand was not published. This still publishes every wrapper param
   (params are not Unique in the generic pass, nor seeded Unique for a variant unless keyed), so
   the escape soundness the plan required is preserved; it only exempts provably-fresh unique
   moves. Task 3 Step 4's code and the `vector_escape` guard reflect the shipped form.
2. **COW census ceiling re-baselined 2000 → 2200** (commit `a088c504`). The `#[ignore]` census
   guard had drifted unnoticed: `main` itself measured 2014 (already over 2000), the branch ~2113.
   Investigation (per the Task 2/3 "stop and investigate" instruction) confirmed healthy Phase 6
   boot-source growth — comparing main→branch, in-place/builder counts grew MORE than COW
   remaining (+257 vs +99), the healthy-growth signature, not an optimizer collapse. My Gap B
   change is census-neutral (2112→2107). Re-baselined in its own commit.

**Remaining gap (out of scope here; follow-up plans):**
- **Real sieve does NOT yet render the in-loop verdict.** The residual cause is isolated to
  **loop nesting** (not the interleaved read): a vector carried by an OUTER loop and mutated in
  an INNER loop degrades to `Shared` — the inner-loop write flows `Shared` out through the outer
  back-edge, so the pessimistic fixpoint (headers start from the join of PROCESSED preds only)
  never bootstraps the nested headers to `Unique`. Needs optimistic loop-header seeding (assume
  Unique at headers, verify, retract on refutation). A single loop already works. See
  `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md` (## residual loop-carried merge gap).
- **`graph_scc.visit` stays fully conservative** (`p0/p1/p2=Published`, every `record_update`
  persistent / `[in_place=false]`, no owned verdict). It is a SEPARATE recursive/SCC-summary
  specialization gap: the threaded record flows into the self-recursive `visit` call, so the
  generic SCC summary publishes it. Needs variant seeding across the SCC — not the non-recursive
  `.Update` wrapper mechanism fixed here. See the same notes file (## graph_scc.visit classification).

The task-by-task sections below are the AS-EXECUTED plan and remain accurate EXCEPT where noted
above (Task 3's unconditional-publish framing is superseded by the `!single_retention` gate;
the census gate ceiling is 2200).

---

## Background: the verified mechanism (read before starting)

This replaces the previous plan, whose Task 2 diagnosis ("`p2=Consumed` is a misattribution; rewrite the summary to `p0=Consumed p1=Borrowed p2=Borrowed ret=alias(p0)`") was wrong. That change is **unsound**: dropping `p2` from `ret=alias` erases the fact that the stored element escapes into the returned vector, which for reference-typed elements (`set_at<Vector<Int>>`) would let a later in-place mutation corrupt an alias now living inside the returned vector.

The current real state (regenerate to confirm; do not trust ids):

```bash
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg | \
  rg -n "call Fn|assign L13|loop\.header|if\.join|facts\.in=\{L13|verdict ->|unique:|fn set_at__Bool|summary: p0"
```

Current findings:
- `set_at__Bool` summary: `p0=Borrowed p1=Borrowed p2=Consumed paths{[]} ret=alias(p0,p2)`.
- The inner-loop call `... call Fn297(L13, L16, false)` then `assign L13 = L64`; the carried vector `L13` is `Unknown` at the loop header and `Shared` at the `if.join`.
- **No `verdict -> ...[unique:...]` line renders anywhere** — confirmed even for a fresh, unique, straight-line vector (no loop), which proves the summary itself is the primary blocker, not the loop.

Why (traced in `boot/compiler/ownership.tw`):
- `select_variant` (≈3658) keys an owned variant only for a param with **non-empty `in_place_paths`** whose argument is `arg_unique` at the call. It reads only `s.params[i].in_place_paths`; it never reads `s.ret`.
- `in_place_paths` is non-empty only when `reconcile_role` (≈3325) returns `.Consumed`, which for a `Borrowed`+`flows_to_return` param requires `has_mut = !dirty.is_empty()`.
- `dirty` comes from `collect_field_reqs` (≈3549). Its per-op transfer `transfer_flow` (≈3442) grows `dirty` for `.ARecordUpdate` (adds `[.f]`) but routes every `.ACall` through `call_result_fact`, which **returns `ff_none()` for builtins** (comment ≈3411: *"Builtins and unknown callees break the chain"*). `xs[i]=v` is an `.ACall` to the `VECTOR_SET` builtin, so `set_at`'s `p0` is never dirtied → `has_mut=false` → `p0=Borrowed` → empty `in_place_paths` → no verdict. **This is Gap B.**
- Separately, at the call site `transfer_summarized_call` (≈1122) handles `ret=MayAliasParams(idxs)`: the Stage 4a whole-return move (result takes the arg's unique shell) fires **only when `idxs.len() == 1`**. `set_at`'s return is `MayAliasParams([0,2])` because `absorb_retained_call_args` (≈957, line ≈970) unions the stored element's prov into the result's **shell** prov. With two params it can't move, so it publishes both origins and the result is `Shared` → the loop-carried `flags` degrades. **This is Gap A.** ⚠️ `absorb_retained_call_args` is **shared** with the `.Allocate` branch of `transfer_builtin_call` (≈1054), and Allocate-effect builtins carry retained args too (`Vector.make` fill `[1]`, `builder_from` `[0]`, `builder_freeze` `[0]`). Rewriting it in place would silently publish those accumulators and drop them from the fresh container's shell prov — an unaudited behavior change that muddies the census gate. The Gap A fix is therefore **scoped to `.Update` only** via a dedicated `escape_retained_call_args` function; `absorb_retained_call_args` and the `.Allocate` path stay byte-identical (Task 3).

The sound targets:
- After Gap B: `set_at__Bool` summary becomes `p0=Consumed paths{[]} p1=Borrowed p2=Consumed paths{[]} ret=alias(p0,p2)`. **At this stage escape tracking rides on `p2` staying in `ret=alias`:** because `p2` is in the shell alias set it `flows_to_return`, so it classifies `Consumed` and every caller in the multi-param `MayAliasParams` branch publishes the stored arg (`transfer_summarized_call` ≈1172). A straight-line unique-vector `set_at` renders `verdict -> fN[unique:p0]`. Render-only: `base_role` `Borrowed`→`Consumed` is identical for escape transfer at call sites (both are no-ops in `transfer_summarized_call`), and `in_place_paths` is read only by `select_variant` (render). Census must be unchanged.
- After Gap A: `set_at__Bool` summary becomes `p0=Consumed paths{[]} p1=Borrowed p2=Published ret=alias(p0)`. **The escape-tracking mechanism deliberately changes** from "p2 in `ret=alias`" (Gap B stage) to "`p2=Published`" (Gap A): the stored element leaves the result's shell alias set (so `ret=MayAliasParams([0])` and the Stage 4a single-param move can fire), and is instead published directly (`esc=Retained → base_role=Published`), so callers still publish the stored arg via `transfer_summarized_call`'s section-1 `Published` branch (≈1148). A vector element sits at `.Elem`, which is **not** a ret_path candidate, so once `p2` leaves the shell alias set it no longer `flows_to_return` — without the explicit publish it would silently degrade to `Borrowed` (a no-op at call sites, i.e. an **unsound** unpublished escape); the mandatory `publish_atom` in Task 3 is what keeps it `Published`. With both gaps the Stage 4a move fires when the vector arg is unique+last-use and the loop-carried `flags` stays `Unique`. This changes ownership facts, so census may change and must be re-baselined only if the new census is proven sound. The change is applied **only** to the `.Update` call site (new `escape_retained_call_args`); the `.Allocate` path keeps calling `absorb_retained_call_args` unchanged, so any census delta must be attributable solely to `.Update` sites (`set_at`/`append`/`Dict.set`), never to `Vector.make`/`builder_*`.
- **Known non-goal (out of scope):** `call_result_fact` skips shell paths when propagating a callee's `in_place_paths` transitively (`if !p.is_shell()` ≈3435), so a wrapper *of* a vector wrapper (a summarized user call whose only in-place path is the shell `[]`) does not inherit the in-place requirement. The sieve case calls `set_at` directly, so this does not block it; a double wrapper over a pure vector shell update is a separate, non-soundness follow-up.

## Global Constraints

- Grounded in real sources: `examples/performance/awfy/twinkle/sieve.tw` and `boot/compiler/graph_scc.tw`.
- Never use rendered `FuncId`, local (`LNN`), or block (`BNN`) numbers in durable assertions. Prefer stable text: function names (`fn replace`, `fn once`), `summary:`, role text (`Consumed paths{[]}`, `ret=alias(p0,p2)`), `verdict ->`, `[unique:p0]`, `facts.in`/`facts.out`, `terminator: loop-back-edge`. `verdict ->` and `[unique:p0]` **are** stable render tokens (verified against `env_main`).
- Before committing any fixture assertion, run `target/twk ir <fixture>.tw --cfg` once and reconcile the assertion text against the current render.
- Write all `--cfg` dumps under `/tmp/twinkle-cfg-gap/` (outside the repo).
- Each task that changes tracked files ends with `git status --short` and a commit using the task's suggested message after its verification command passes.
- **Soundness gating:** Gap B (Task 2) must leave the COW census unchanged. Gap A (Task 3) may change the census; it must pass `make stage2` (self-host) and any census delta must be justified as sound (fewer or equal live aliases, never more in-place mutation of a still-live value). Because Gap A is scoped to the `.Update` path, every changed census site must correspond to a COW **update** (`set_at`/`append`/`Dict.set`/`Dict.remove`), never to a `Vector.make`/`builder_from`/`builder_freeze` allocation — an allocation-site delta means the scoping leaked and must be investigated before committing.
- **`select_variant` is currently render/test-only:** it is read by fixtures and the CFG dump, not consumed by codegen (the "4c-recording pass" that would drive specialization is future work). This is *why* Gap B can create new owned variants for every `.Update` wrapper without moving the census. If anyone wires `select_variant` output into compute before this lands, Task 2 stops being census-neutral — re-check that assumption if the Task 2 census gate trips.
- Do not run the full `cargo test`; run only the targeted `cow_analysis` census plus the boot suite.

---

### Task 1: Preserve corrected evidence for the vector in-place gap

**Files:**
- Read: `examples/performance/awfy/twinkle/sieve.tw`
- Read: `boot/prelude/vector.tw` (the `set_at` definition)
- Read: `docs/plans/sound-uniqueness/analysis/worked-examples.md` (Case A)
- Create if absent, otherwise modify: `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md`

**Interfaces:**
- Consumes: current real-source CFG output for `sieve`.
- Produces: an evidence note recording the two proven gaps so downstream tasks can cite it without redoing archaeology.

- [ ] **Step 1: Regenerate the current sieve CFG dump**

Run:

```bash
mkdir -p /tmp/twinkle-cfg-gap
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg > /tmp/twinkle-cfg-gap/sieve.cfg
```

Expected: exit 0, file written.

- [ ] **Step 2: Extract the stable evidence fragments**

Run:

```bash
rg -n "^fn run|^fn set_at__Bool|summary:|loop\.header|if\.join|facts\.in=.*(Unknown|Shared)|assign L13|verdict ->|unique:" /tmp/twinkle-cfg-gap/sieve.cfg
```

Expected: `set_at__Bool` summary shows `p0=Borrowed ... p2=Consumed paths{[]} ret=alias(p0,p2)`; the carried vector is `Unknown` at the loop header and `Shared` at the `if.join`; **no `verdict ->` line appears**.

- [ ] **Step 3: Confirm the summary itself is the blocker with a straight-line probe**

Run:

```bash
cat > /tmp/twinkle-cfg-gap/straight.tw <<'EOF'
pub fn once() Bool {
  flags: Vector<Bool> = collect _ in range(10) { true }
  flags = flags.set_at(0, false)
  flags[1]
}
EOF
target/twk ir /tmp/twinkle-cfg-gap/straight.tw --cfg | rg -n "fn once|summary:|verdict ->|unique:"
```

Expected: no `verdict ->` line even though `flags` is fresh, unique, and single-use before reassign. This proves the missing `in_place_paths` on `set_at`'s `p0` (Gap B) — not the loop — is the primary blocker.

- [ ] **Step 4: Record the corrected two-gap finding**

Write `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md` (create the file if absent) with exactly this content under a heading `## sieve vector in-place gap (verified)`:

```markdown
## sieve vector in-place gap (verified)

The real sieve source lowers to the expected loop-carried `set_at` wrapper call.
Current CFG ownership analysis renders no `verdict -> ...[unique:...]` decision for
it, for two independent reasons:

- Gap B (summary / requirement-flow): `xs[i]=v` inside `set_at` lowers to the
  VECTOR_SET builtin. `collect_field_reqs` routes builtin calls through
  `call_result_fact`, which breaks the flow chain for builtins, so the vector base
  is never dirtied. Result: `set_at`'s p0 stays `Borrowed` with empty
  `in_place_paths`, and `select_variant` (which reads only `in_place_paths`, never
  `ret`) can never key on it. Even a fresh, unique, straight-line vector renders no
  verdict.

- Gap A (forward ownership transfer): `absorb_retained_call_args` unions the stored
  element's provenance into the result's SHELL provenance, so `set_at`'s return is
  `MayAliasParams([0,2])`. The Stage 4a whole-return move only fires for a single
  aliased param, so the call publishes both origins and the loop-carried vector
  degrades to `Shared`. Note `absorb_retained_call_args` is shared with the
  `.Allocate` branch (`Vector.make`/`builder_from`/`builder_freeze` retain args too),
  so the fix is scoped to a new `.Update`-only `escape_retained_call_args` rather than
  rewriting the shared function.

Escape tracking for the stored reference element is required, but the mechanism
changes across the two fixes:
- After Gap B only: `p2` stays Consumed and stays in `ret=alias(p0,p2)`; callers
  publish it via the multi-param `MayAliasParams` branch.
- After Gap A: `p2` leaves the shell alias set (`ret=alias(p0)`, enabling the
  single-param move) and instead escapes via `p2=Published`; callers publish it via
  the section-1 `Published` branch. This relies on a MANDATORY `publish_atom` on the
  stored operand — a vector element is at `.Elem`, not a ret_path, so without the
  explicit publish `p2` would silently degrade to `Borrowed` (an unsound, unpublished
  escape). The `vector_escape` fixture guards exactly this.

The previously-planned "p0=Consumed p1=Borrowed p2=Borrowed ret=alias(p0)" target was
unsound (it dropped the escape entirely) and is rejected.
```

- [ ] **Step 5: Verify no compiler behavior changed**

Run:

```bash
git diff -- boot/compiler src examples/performance/awfy/twinkle/sieve.tw
```

Expected: empty (documentation-only task).

- [ ] **Step 6: Commit the evidence note**

Run:

```bash
git status --short
git add docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md docs/plans/sound-uniqueness-sieve-cfg-gap.md
git commit -m "docs: record verified sieve vector in-place ownership gaps"
```

Expected: only the evidence note and this rewritten plan are staged.

---

### Task 2: Gap B — dirty the vector base so `set_at`'s `p0` becomes an in-place `Consumed` param

**Files:**
- Read: `boot/compiler/ownership.tw` (`collect_field_reqs` ≈3549, `transfer_flow` ≈3442, `call_result_fact` ≈3412, `reconcile_role` ≈3325, `select_variant` ≈3658)
- Read: `boot/compiler/opt/semantics.tw` (`CallSemantics`, `call_info`, `.Update` effect, `cow_base_arg`)
- Read: `boot/compiler/variant_id.tw` (`shell_set` ≈52, `is_empty`, `union`)
- Modify: `boot/compiler/ownership.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/vector_replace.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/vector_once.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: the requirement-flow (`FlowFact{origin, dirty}`) dataflow and `CallSemantics` for the COW `VECTOR_SET` builtin.
- Produces: `set_at`'s `p0` classified `Consumed` with `in_place_paths={[]}`, so `select_variant` renders `verdict -> fN[unique:p0]` for a unique vector argument.

- [ ] **Step 1: Add the two fixtures**

Create `boot/tests/fixtures/cfg/sound_uniqueness/vector_replace.tw`:

```tw
pub fn replace(xs: Vector<Bool>, i: Int, value: Bool) Vector<Bool> {
  xs[i] = value
  xs
}
```

Create `boot/tests/fixtures/cfg/sound_uniqueness/vector_once.tw`:

```tw
pub fn once() Bool {
  flags: Vector<Bool> = collect _ in range(10) { true }
  flags = flags.set_at(0, false)
  flags[1]
}
```

- [ ] **Step 2: Preflight the current (pre-fix) render shape**

Run:

```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/vector_replace.tw --cfg | rg -n "^fn replace|summary:|verdict ->|unique:"
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/vector_once.tw --cfg | rg -n "^fn once|^fn set_at__Bool|summary:|verdict ->|unique:"
```

Expected (pre-fix): `vector_replace` defines its own wrapper `fn replace` (it does `xs[i]=v` inline, so there is **no** `fn set_at__Bool` in its dump); its summary shows `p0=Borrowed ... p2=Consumed paths{[]} ret=alias(p0,p2)`. `vector_once` calls `flags.set_at(...)`, so `fn set_at__Bool` **is** present there with the same `p0=Borrowed` summary, and **no `verdict ->` line** appears in `once`. If either file fails to compile, fix the fixture syntax before continuing.

- [ ] **Step 3: Add the failing tests**

In `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`, add these two tests to `suite()` immediately before the existing `"Cell-backed dict update stays conservative"` test:

```tw
    .test(
      "vector index-write wrapper summarizes the base as in-place consumed",
      fn() {
        out := try render_entry("vector_replace")
        replace := try section_from(out, "fn replace")
        // p0 (the vector) is now an in-place consumed param...
        try assert.str_contains(replace, "p0=Consumed paths{[]}")
        // ...and at the Gap-B stage p2 (the stored value) is escape-tracked by
        // STAYING in ret=alias. NOTE: Task 3 (Gap A) deliberately replaces this
        // assertion — after Gap A the return is ret=alias(p0) and p2 escapes via
        // p2=Published instead. This `ret=alias(p0,p2)` line is updated in Task 3
        // Step 5; it is the correct intermediate shape, not the final one.
        try assert.str_contains(replace, "ret=alias(p0,p2)")
        .Ok({})
      },
    )
    .test(
      "unique straight-line vector set_at renders an owned in-place verdict",
      fn() {
        out := try render_entry("vector_once")
        once := try section_between(out, "fn once", "fn set_at__Bool")
        try assert.str_contains(once, "verdict ->")
        try assert.str_contains(once, "[unique:p0]")
        .Ok({})
      },
    )
```

- [ ] **Step 4: Run the tests to confirm they fail**

Run:

```bash
target/twk run boot/tests/main.tw 2>&1 | rg -n "vector index-write wrapper|unique straight-line vector|FAIL|fail"
```

Expected: both new tests fail (the summary shows `p0=Borrowed`; no `verdict ->`).

- [ ] **Step 5: Thread `sem` into the requirement-flow analysis**

In `boot/compiler/ownership.tw`, change `transfer_flow`'s signature and its `.ACall` branch. Replace the existing `transfer_flow` (the block starting `fn transfer_flow(st: Dict<Int, FlowFact>, table: SummaryTable, op: AnfOp, result: Int)`) with:

```tw
fn transfer_flow(
  st: Dict<Int, FlowFact>,
  table: SummaryTable,
  sem: OptimizerSemantics,
  op: AnfOp,
  result: Int,
) Dict<Int, FlowFact> {
  case op {
    .ARecordUpdate(base, fld, _, _, _) => {
      bf := flow_get(st, base)
      st[result] = if bf.origin >= 0 {
        FlowFact.{ origin: bf.origin, dirty: bf.dirty.add(vid.field(fld.id)) }
      } else {
        ff_none()
      }
      st
    },
    .AAssign(local, a) => {
      st[local.id] = flow_get(st, a)
      st
    },
    .ACall(callee, args) => {
      // A COW in-place update builtin (`xs[i]=v` -> VECTOR_SET, `Dict.set`, ...)
      // dirties its base collection at the SHELL, exactly like an ARecordUpdate
      // dirties [.f]; this is what lets a vector wrapper's base param become an
      // in-place Consumed param. A summarized USER callee still routes through
      // call_result_fact (Stage 2b transitive propagation); non-Update builtins
      // and unknown callees break the chain (origin none).
      st[result] = case callee_func_id(callee) {
        .Some(fid) => case call_info(sem, fid) {
          .Some(cs) => cow_update_result_fact(st, cs, args),
          .None => call_result_fact(st, table, callee, args),
        },
        .None => call_result_fact(st, table, callee, args),
      }
      st
    },
    _ => st,
  }
}

// A COW `.Update` builtin dirties its cow_base_arg at the shell: the base value's
// backing is reused in place and handed back. Mirrors how ARecordUpdate grows dirty,
// but with no field path — the whole shell is the dirtied region. Non-Update builtins
// (reads, allocations) or a missing base break the chain.
fn cow_update_result_fact(
  st: Dict<Int, FlowFact>,
  cs: CallSemantics,
  args: Vector<Atom>,
) FlowFact {
  case cs.effect {
    .Update => case cs.cow_base_arg {
      .Some(k) => if k < args.len() {
        base := flow_get(st, args[k])
        if base.origin >= 0 {
          FlowFact.{ origin: base.origin, dirty: base.dirty.union(vid.shell_set()) }
        } else {
          ff_none()
        }
      } else {
        ff_none()
      },
      .None => ff_none(),
    },
    _ => ff_none(),
  }
}
```

**Note — Gap B is not vector-specific.** `cow_update_result_fact` fires for *any* `.Update` builtin with a `cow_base_arg`, so `Dict.set`/`Dict.remove`/builder-push wrappers also start dirtying their base and may newly render `verdict -> ...[unique:...]`. This is the intended generalization (a COW update is a COW update), but it means a `Dict`-wrapper verdict appearing in some unrelated suite dump is **expected**, not a regression. The new fixtures cover only vectors; do not treat non-vector verdicts as failures.

Then update the single caller inside `collect_field_reqs` (the line `st = transfer_flow(st, table, inst.op, inst.anf_local.id)`) to pass `sem`:

```tw
        st = transfer_flow(st, table, sem, inst.op, inst.anf_local.id)
```

- [ ] **Step 6: Thread `sem` into `collect_field_reqs` and its caller**

Change `collect_field_reqs`'s signature (the line `pub fn collect_field_reqs(f: CfgFunction, table: SummaryTable) Dict<Int, vid.PathSet>`) to:

```tw
pub fn collect_field_reqs(f: CfgFunction, table: SummaryTable, sem: OptimizerSemantics) Dict<Int, vid.PathSet> {
```

Then update its call site inside `summarize_seeded` (the line `field_reqs := collect_field_reqs(f, table)`) to:

```tw
  field_reqs := collect_field_reqs(f, table, sem)
```

`summarize_seeded` already has `sem` in scope as a parameter.

- [ ] **Step 7: Fix any other `collect_field_reqs`/`transfer_flow` callers**

Run:

```bash
rg -n "collect_field_reqs\(|transfer_flow\(" boot/compiler boot/tests
```

Expected: update every call to pass `sem`. If a unit test calls `collect_field_reqs` with a raw `CfgFunction`, construct `sem` there via `semantics.make_prelude_optimizer_semantics(b)` (see `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw:25` for the pattern). Do not leave any caller on the old arity.

- [ ] **Step 8: Rebuild the CLI and re-run the tests**

Run:

```bash
make quick-bundle-cli
target/twk run boot/tests/main.tw 2>&1 | rg -n "vector index-write wrapper|unique straight-line vector|FAIL|fail|passed"
```

Expected: both new tests pass; no other suite regresses. If `make quick-bundle-cli` reports a stale `target/boot.wasm`, run `make bundle-cli` instead.

- [ ] **Step 9: Confirm census is unchanged (render-only guarantee)**

Run:

```bash
cargo test --release cow_analysis 2>&1 | tail -20
```

Expected: the census total is unchanged from its committed baseline. Gap B only affects `select_variant`/render, so any census delta means an unintended consumer of `in_place_paths`/`base_role` was hit — stop and investigate before committing.

- [ ] **Step 10: Format and commit**

Run:

```bash
target/twk fmt \
  boot/compiler/ownership.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/vector_replace.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/vector_once.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git status --short
git add boot/compiler/ownership.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/vector_replace.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/vector_once.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git commit -m "ownership: model vector in-place update as base in-place consumption

xs[i]=v lowers to the VECTOR_SET builtin, which the requirement-flow analysis
previously ignored (builtins break the flow chain), so a vector wrapper's base
param never became Consumed and select_variant could not render an in-place
verdict. Teach transfer_flow to dirty a COW .Update builtin's base at the shell,
mirroring ARecordUpdate. The stored value stays Consumed and stays in ret=alias,
so escape tracking is preserved. Render-only; census unchanged."
```

Expected: only the summary logic, the two fixtures, and the suite are staged.

---

### Task 3: Gap A — keep the stored element out of the result's shell provenance so the whole-return move can fire

**Files:**
- Read: `boot/compiler/ownership.tw` (`absorb_retained_call_args` ≈957, `transfer_builtin_call` ≈1043 — note the `.Allocate` call at ≈1054 and the `.Update` call at ≈1065, `field_store`, `publish_atom`/`publish_local`, `transfer_summarized_call` ≈1122 including the Stage 4a move at ≈1166)
- Read: `boot/compiler/opt/semantics.tw` (the Allocate-effect builtins with retained args: `Vector.make` `[1]`, `builder_from` `[0]`, `builder_freeze` `[0]` — these must stay unchanged)
- Modify: `boot/compiler/ownership.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/vector_twice.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/vector_escape.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: the COW-update forward transfer (`transfer_builtin_call`'s `.Update` branch only).
- Produces: `set_at`'s return classified `MayAliasParams([0])` (rendered `ret=alias(p0)`), with the stored element soundly published, so a unique vector argument MOVES and the result stays `Unique`.
- **Scope guard:** the fix is a new `escape_retained_call_args` used only by the `.Update` branch. `absorb_retained_call_args` and the `.Allocate` branch are left byte-identical, so `Vector.make`/`builder_from`/`builder_freeze` summaries do not move.

- [ ] **Step 1: Confirm the escape-publish requirement in `field_store` (already verified — do not re-open)**

Read `field_store`, `publish_atom`/`publish_local`, and `consume_base` to ground the fix:

```bash
rg -n "fn field_store|fn publish_atom|fn publish_local|fn consume_base" boot/compiler/ownership.tw
```

**Confirm before writing the fix (one line):** `publish_local` records the escape (`esc=Retained`) *independently of* the local's validity flag — i.e. calling `publish_atom` on an operand that `field_store` just invalidated (`set_valid=false`) still publishes it. The Step 4 fix runs `field_store` then `publish_atom` in that order and relies on the publish landing regardless. If `publish_local` were guarded by validity (it is not, per the current source), the ordering would need to flip; verify, don't assume.

Verified fact — treat as a requirement, **not** an open question: `field_store` (≈868)
publishes the stored operand **only on a NON-last-use** path; on a **LAST-use** store it
calls `set_valid(src, false)` (consume/invalidate), which records `cap=Consumed` but leaves
`esc=Borrowed`. `set_at` stores `value` at its last use. Once Gap A removes `value` from the
result's shell alias set, `value` no longer `flows_to_return` — a vector element sits at
`.Elem`, which is **not** a ret_path candidate (see `summarize_seeded` ≈3837) — so
`reconcile_role` would classify it `Borrowed`, and `Borrowed` is a **no-op** at every call
site (`transfer_summarized_call` ≈1149). That would leave a stored reference element
**unpublished at the caller**: the caller keeps it unique and can mutate it in place,
corrupting the copy now living inside the collection. **Unsound.**

Therefore the Step 4 fix **MUST** publish each retained (stored) operand **that actually
escapes**. Publishing forces `esc=Retained → base_role=Published`
(ownership.tw:3719-3721, 3327), the *only* mechanism that publishes the stored argument at
callers after `p2` leaves the shell alias set. For the wrapper-param case (`set_at`/`stash`)
this is unconditional in effect, because a param is not Unique in the generic pass.

> **AS SHIPPED:** publish is gated on `!single_retention` (see Step 4). This is not a "drop
> the publish" escape hatch for the escaping case — a wrapper param is never `single_retention`
> in the generic pass, so it still publishes. The gate only exempts a provably-fresh unique
> single-use move (own==Unique && last-use && single-store), which has no surviving caller
> alias to protect and whose `[Elem]` fact must be preserved. Without the gate, unconditional
> publish drops `[Elem]` on every fresh-owned element store (a pessimization) and broke the
> pre-existing "storing an owned inner into an all-owned vector keeps [Elem]" test. The
> `vector_escape` fixture (Step 2) guards the escaping (param) case; the `[Elem]` test in
> `cfg_field_facts_suite` guards the exempted move case.

- [ ] **Step 2: Add the two fixtures**

Create `boot/tests/fixtures/cfg/sound_uniqueness/vector_twice.tw` (a unique vector reused across two sequential `set_at`s — the second call must still see it unique, which requires the first call to MOVE rather than publish):

```tw
pub fn twice() Bool {
  flags: Vector<Bool> = collect _ in range(10) { true }
  flags = flags.set_at(0, false)
  flags = flags.set_at(1, false)
  flags[2]
}
```

Create `boot/tests/fixtures/cfg/sound_uniqueness/vector_escape.tw` (the soundness guard: a reference-typed element stored into a vector at its **last use** must be published, NOT treated as unique afterward). Note the shape deliberately mirrors `set_at` exactly — the element `v` is stored and then **not** returned (the container `xs` is returned), so the store is `v`'s last use and hits `field_store`'s invalidate-not-publish branch. **Do not have the function return `v`**: returning it makes the store a non-last-use, which `field_store` already publishes, so the guard would pass vacuously and miss the exact bug:

```tw
pub fn stash(xs: Vector<Vector<Int>>, i: Int, v: Vector<Int>) Vector<Vector<Int>> {
  xs[i] = v
  xs
}
```

- [ ] **Step 3: Preflight the current (pre-fix) render shape**

Run:

```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/vector_twice.tw --cfg | rg -n "^fn twice|^fn set_at__Bool|summary:|verdict ->|unique:"
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/vector_escape.tw --cfg | rg -n "^fn stash|summary:"
```

Expected (pre-fix, i.e. Gap B landed but Gap A not yet): `set_at__Bool` still shows `ret=alias(p0,p2)`; `twice` renders a verdict for the FIRST call only (the first call publishes `flags`, so the second no longer sees it unique). `vector_escape` does `xs[i]=v` inline, so there is **no** `fn set_at` in its dump — inspect `fn stash`'s own summary, which pre-Gap-A reads `p0=Consumed paths{[]} p1=Borrowed p2=Consumed paths{[]} ret=alias(p0,p2)`. Record that exact `stash` summary as the escape baseline; after Gap A it must become `... p2=Published ret=alias(p0)`.

- [ ] **Step 4: Make the stored element a deep escape, not a shell alias (`.Update` only)**

Do **not** edit `absorb_retained_call_args` — it is shared with the `.Allocate` branch (`Vector.make`/`builder_from`/`builder_freeze`), which must keep absorbing its retained args into the fresh container's shell prov. Instead, add a new `.Update`-only function and route only the `.Update` call site to it.

First add `escape_retained_call_args` next to `absorb_retained_call_args` in `boot/compiler/ownership.tw`.

> **AS SHIPPED (supersedes the original unconditional-publish design):** the publish is gated on
> `!single_retention`, not unconditional. Unconditional publish clobbered a clean unique move's
> ownership and dropped the result's `[Elem]` fact (pessimization; broke the pre-existing
> "storing an owned inner into an all-owned vector keeps [Elem]" test). Gating on
> `single_retention` (own==Unique && last-use && single-store — the SAME predicate the `.Update`
> field-fact block reads to keep/drop `[Elem]`) publishes exactly the escaping operands and
> exempts provably-fresh unique moves. Invariant: the result keeps `[Elem]` iff the operand was
> NOT published. This still publishes every wrapper param (params are not Unique in the generic
> pass, nor seeded Unique for a variant unless keyed), so the escape soundness below is preserved.

```tw
// COW `.Update` ONLY (see transfer_builtin_call's .Update branch). The stored
// operands escape into the collection as DEEP elements. They must NOT join the
// result's SHELL provenance: carry_base_prov (run immediately before this in the
// .Update chain) already set the result's shell prov to the COW base's origins only,
// so a single-param whole-return move can fire. This is a SEPARATE function from
// absorb_retained_call_args on purpose: the .Allocate path still absorbs its retained
// args into the fresh container's shell and must stay unchanged.
//
// publish_atom is REQUIRED for an ESCAPING operand, not redundant with field_store:
// field_store publishes only a NON-last-use operand; on a LAST-use store it
// invalidates instead (set_valid=false -> cap=Consumed, esc stays Borrowed). set_at
// stores its `value` PARAM at its last use, so without this publish `value` classifies
// Borrowed once it leaves the shell alias set (it no longer flows_to_return: an element
// is at .Elem, not a ret_path) and is NEVER published at the caller -- an unsound escape.
// The publish forces esc=Retained -> base_role=Published.
//
// But publish ONLY when the operand actually escapes. The publish and the result's
// [Elem] field fact are two faces of one decision: keep [Elem] <=> the operand is a
// clean unique move into the container <=> it must NOT be published. `single_retention`
// (own==Unique && last-use && single-store) is exactly that predicate, and it is what
// the .Update field-fact block reads to keep/drop [Elem]. A single-retained unique
// operand (e.g. a fresh local stored at last use) is genuinely consumed into the
// result -- no surviving caller alias to protect -- so publishing it would only clobber
// its ownership and spuriously drop [Elem] (a pessimization, more COW). A param or
// aliased operand is NOT single-retained (params are not Unique in the generic pass,
// nor seeded Unique for a variant unless keyed), so it still publishes and escapes
// soundly. Invariant: result keeps [Elem] iff the operand was not published here.
fn escape_retained_call_args(
  st: ForwardState,
  args: Vector<Atom>,
  retained: Vector<Int>,
  last: Vector<Int>,
) ForwardState {
  operands := retained_atoms(retained, args)
  for i in retained {
    if i < args.len() {
      moved := st.single_retention(args[i], last, operands)
      st = .field_store(args[i], last)
      st = if moved {
        st
      } else {
        st.publish_atom(args[i])
      }
    }
  }
  st
}
```

Then, in `transfer_builtin_call`'s `.Update` branch (≈1065), retarget only that call. Change:

```tw
      st = .consume_call_base(result, cs, args, last).carry_base_prov(result, cs, args).absorb_retained_call_args(
        result,
        args,
        ri,
        last,
      )
```

to:

```tw
      st = .consume_call_base(result, cs, args, last).carry_base_prov(result, cs, args).escape_retained_call_args(
        args,
        ri,
        last,
      )
```

Leave the `.Allocate` branch's `absorb_retained_call_args(result, args, ri, last)` call (≈1054) **untouched**. `escape_retained_call_args` drops the `result` parameter deliberately: it never writes result prov, because `carry_base_prov` already established the base-only shell prov for the `.Update` result.

- [ ] **Step 5: Add the tests**

In `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`, add to `suite()` immediately after the Task 2 tests:

```tw
    .test(
      "vector set_at return aliases only the base shell",
      fn() {
        out := try render_entry("vector_twice")
        set_at := try section_from(out, "fn set_at__Bool")
        try assert.str_contains(set_at, "ret=alias(p0)")
        try assert.is_false(set_at.contains("ret=alias(p0,p2)"))
        .Ok({})
      },
    )
    .test(
      "reused unique vector stays unique across sequential set_at",
      fn() {
        out := try render_entry("vector_twice")
        twice := try section_between(out, "fn twice", "fn set_at__Bool")
        // Both sequential set_at calls render an owned in-place verdict: the first
        // MOVES flags (result unique), so the second still sees it unique.
        try assert.str_contains(twice, "[unique:p0]")
        occurrences := twice.split("[unique:p0]")
        try assert.ok(occurrences.len() >= 3, "expected two owned verdicts in twice")
        .Ok({})
      },
    )
    .test(
      "reference element stored into a vector is published not owned",
      fn() {
        out := try render_entry("vector_escape")
        stash := try section_from(out, "fn stash")
        // v (p2) is stored into xs at its LAST use and never returned, so it must be
        // PUBLISHED (escaped): the caller cannot keep treating it as unique and later
        // mutate it in place, which would corrupt the copy now inside xs. This is the
        // exact soundness property the whole-return move must not break -- if the
        // mandatory publish_atom is dropped, p2 degrades to Borrowed and this fails.
        try assert.str_contains(stash, "p2=Published")
        // ...and the return aliases ONLY the base shell, enabling the single-param move.
        try assert.str_contains(stash, "ret=alias(p0)")
        .Ok({})
      },
    )
```

Then **update the Task 2 test** `"vector index-write wrapper summarizes the base as in-place consumed"`: Gap A deliberately changes `replace`'s summary from `ret=alias(p0,p2)` (with `p2` Consumed) to `ret=alias(p0)` with `p2=Published`. Replace that test's final assertion accordingly:

```tw
        try assert.str_contains(replace, "p0=Consumed paths{[]}")
        // After Gap A: the base still consumes in place, the stored value escapes
        // via p2=Published (not via ret=alias), and the return aliases only the base.
        try assert.str_contains(replace, "p2=Published")
        try assert.str_contains(replace, "ret=alias(p0)")
```

- [ ] **Step 6: Rebuild and run the boot suite**

Run:

```bash
make quick-bundle-cli
target/twk run boot/tests/main.tw 2>&1 | rg -n "vector set_at return aliases|reused unique vector|reference element stored|FAIL|fail|passed"
```

Expected: the three new tests pass. Existing tests may need re-baselining if a summary they assert changed shape — inspect each failure and update only if the new shape is the intended `ret=alias(p0)`/published-element behavior, never to hide a regression.

- [ ] **Step 7: Soundness gate — census and self-host**

Run these one at a time (never concurrently):

```bash
cargo test --release cow_analysis 2>&1 | tail -20
make stage2
```

Expected: `make stage2` (self-host) succeeds. The census may change: a sound change only ever removes live aliases (enabling more moves), never adds in-place mutation of a value that is still live. If the census total moved, diff the per-site census against the baseline and confirm every changed site corresponds to a vector that is provably unique + last-use at the call. If any site now elides a copy for a still-aliased vector, the change is unsound — revert and reconsider. Record the justified new baseline number in the commit body.

**Scope check (the change touches only `.Update`):** every changed census site must be a COW **update** (`set_at`/`append`/`Dict.set`/`Dict.remove`). Because Step 4 left `absorb_retained_call_args` and the `.Allocate` branch untouched, a moved site at a `Vector.make`/`builder_from`/`builder_freeze` allocation means the scoping leaked — stop and investigate rather than re-baselining. If unsure a delta is `.Update`-only, dump a fixture that uses `Vector.make(n, x)` / `builder_*` and confirm its summary is byte-identical to pre-Gap-A.

- [ ] **Step 8: Format and commit**

Run:

```bash
target/twk fmt boot/compiler/ownership.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/vector_twice.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/vector_escape.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git status --short
git add boot/compiler/ownership.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/vector_twice.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/vector_escape.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git commit -m "ownership: COW-update store escapes deep, not into the result shell

A stored element previously joined the result's shell provenance, forcing
set_at's return to MayAliasParams([base,value]) and blocking the Stage 4a
whole-return move, so a unique vector published to Shared. Add a .Update-only
escape_retained_call_args that publishes the stored element as a deep escape and
keeps only the base in the shell alias set, so a unique+last-use vector argument
moves and the result stays Unique. The shared absorb_retained_call_args and the
.Allocate path (Vector.make/builder_*) are left unchanged, so allocation
summaries do not move. Escape tracking preserved (vector_escape guard). Census
re-baselined; self-host green."
```

Expected: only the transfer logic, the two fixtures, and the suite are staged.

---

### Task 4: Verify the real `sieve` renders the in-place verdict

**Files:**
- Read: `examples/performance/awfy/twinkle/sieve.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/sieve_loop_set.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: Gap B (Task 2) and Gap A (Task 3).
- Produces: a regression fixture proving the loop-carried vector case renders the verdict, plus a real-sieve confirmation.

- [ ] **Step 1: Regenerate the real sieve CFG and inspect the loop call site**

Run:

```bash
target/twk ir examples/performance/awfy/twinkle/sieve.tw --cfg > /tmp/twinkle-cfg-gap/sieve-after.cfg
rg -n "^fn run|^fn set_at__Bool|summary:|loop\.header|if\.join|facts\.in=.*(Unknown|Shared|Unique)|verdict ->|unique:" /tmp/twinkle-cfg-gap/sieve-after.cfg
```

Expected if both gaps are fixed: `set_at__Bool` summary shows `p0=Consumed paths{[]} ... ret=alias(p0)`; the inner-loop `set_at` call renders `verdict -> fN[unique:p0]`; the carried vector no longer shows `: Shared` at the `if.join`.

Note on the loop fixpoint: the requirement-flow and ownership fixpoints only let PROCESSED predecessors contribute at a block entry, so a loop header is first visited with only its pre-loop predecessor (unique from the `collect` freeze). If the back-edge now also produces `Unique` (because the Stage 4a move fires), the header join stays `Unique` and converges — no separate optimistic-seeding stage should be required. If it does NOT converge to `Unique`, do not patch it here; record the residual loop-merge gap in `sieve-cfg-gap-notes.md` and treat it as a follow-up (Step 4 below).

- [ ] **Step 2: Add the minimal loop-carried fixture**

Create `boot/tests/fixtures/cfg/sound_uniqueness/sieve_loop_set.tw` (returns `Int` so the vector is never published at the function boundary, matching sieve's `flags`-is-private property):

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

- [ ] **Step 3: Add the loop-carried regression test**

In `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`, add to `suite()` immediately after the Task 3 tests:

```tw
    .test(
      "loop-carried vector set_at renders an owned in-place verdict",
      fn() {
        out := try render_entry("sieve_loop_set")
        loop_set := try section_between(out, "fn loop_set_count", "fn set_at__Bool")
        try assert.str_contains(loop_set, "terminator: loop-back-edge")
        try assert.str_contains(loop_set, "verdict ->")
        try assert.str_contains(loop_set, "[unique:p0]")
        // The carried vector must not degrade to Shared on any fact line.
        for line in loop_set.lines() {
          if line.contains("facts.in=") or line.contains("facts.out=") {
            try assert.is_false(line.contains(": Shared"))
          }
        }
        .Ok({})
      },
    )
```

- [ ] **Step 4: Run the fixture; branch on the outcome**

Run:

```bash
make quick-bundle-cli
target/twk run boot/tests/main.tw 2>&1 | rg -n "loop-carried vector set_at|FAIL|fail|passed"
```

Expected: the test passes — the loop-carried case renders the verdict and the vector stays non-`Shared`.

If it fails with the verdict present at the call but a `: Shared` fact on the carried vector at the join/back-edge, the residual gap is the loop-carried ownership merge (not the summary or the move). In that case: mark this test `.skip(...)` if the suite supports it (or comment it with a `// pending: loop-carried merge` note and a passing weaker assertion on just `verdict ->`), append a `## residual loop-carried merge gap` section to `sieve-cfg-gap-notes.md` describing the observed header/back-edge facts, and stop — that merge is a separate follow-up plan, not part of this one.

- [ ] **Step 5: Commit**

Run:

```bash
target/twk fmt boot/tests/fixtures/cfg/sound_uniqueness/sieve_loop_set.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git status --short
git add boot/tests/fixtures/cfg/sound_uniqueness/sieve_loop_set.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw \
  docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md
git commit -m "test: cover loop-carried vector in-place verdict (sieve shape)"
```

Expected: the fixture, the suite, and any notes update are staged.

---

### Task 5: Classify the `graph_scc.visit` secondary case

**Files:**
- Read: `boot/compiler/graph_scc.tw`
- Modify: `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md`
- Modify when classification requires wording changes: `docs/plans/sound-uniqueness/analysis/worked-examples.md`

**Interfaces:**
- Consumes: the sieve fixes from Tasks 2–4.
- Produces: a recorded classification of whether `graph_scc.visit` is the same class or a separate recursive-SCC gap.

- [ ] **Step 1: Regenerate the graph SCC CFG**

Run:

```bash
target/twk ir boot/compiler/graph_scc.tw --cfg > /tmp/twinkle-cfg-gap/graph_scc.cfg
rg -n "^fn visit|summary:|record_update|transport=|terminator: loop-back-edge|verdict ->|unique:" /tmp/twinkle-cfg-gap/graph_scc.cfg
```

Expected: `visit`'s summary and its record/vector field-update verdicts. Note whether any `verdict -> ...[unique:...]` now renders.

- [ ] **Step 2: Classify**

`visit` threads `cur` (a record of dicts/vectors) through recursion, dict field writes (`cur.indices[node]=idx`), vector field appends (`cur.stack = .append(node)`), match joins, and a loop back-edge. Apply this rule:
- If `visit` now renders an in-place verdict for its threaded state after the sieve fixes, it is the same ownership-propagation class.
- If it stays conservative (params `Published`, `field=persistent(...)`), it is a separate recursive/SCC-summary specialization gap: the state is threaded through a self-recursive call whose summary is still being fixed by the SCC driver, which the whole-value vector fix does not address.

- [ ] **Step 3: Record the classification**

Append the matching bullet to `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md` under a `## graph_scc.visit classification` heading:

```markdown
- `graph_scc.visit` renders in-place verdicts after the sieve vector fix, so it is
  the same ownership-propagation class.
```

or:

```markdown
- `graph_scc.visit` stays conservative after the sieve vector fix. It is a separate
  recursive/SCC-summary specialization gap: the threaded state passes through a
  self-recursive call whose summary is still generic, and its field updates target
  dict/record fields inside a record shell that is never proven uniquely owned across
  the recursion. Track as a follow-up.
```

- [ ] **Step 4: Reconcile the worked example wording if needed**

If `worked-examples.md` states a `graph_scc.visit` target the implementation still does not reach, add one clarifying line distinguishing the observed shape today from the target after recursive/SCC specialization. Do not weaken the source-shape finding.

- [ ] **Step 5: Commit**

Run:

```bash
git status --short
git add docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md docs/plans/sound-uniqueness/analysis/worked-examples.md
git commit -m "docs: classify graph_scc.visit relative to the sieve vector fix"
```

Expected: documentation-only commit.

---

### Task 6: Final verification and reporting

**Files:**
- Any files changed by Tasks 1–5.

**Interfaces:**
- Consumes: all prior tasks.
- Produces: an evidence-backed final status.

- [ ] **Step 1: Format every changed Twinkle file**

Run:

```bash
target/twk fmt \
  boot/compiler/ownership.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/vector_replace.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/vector_once.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/vector_twice.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/vector_escape.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/sieve_loop_set.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
```

Expected: formatter succeeds and is idempotent (a second run is a no-op).

- [ ] **Step 2: Full boot suite, census, self-host, lint — one at a time**

Run each separately (never concurrently):

```bash
target/twk run boot/tests/main.tw
cargo test --release cow_analysis
make stage2
target/twk lint boot/main.tw
```

Expected: boot suite green; census at the justified baseline; self-host green; lint reports no new house-rule violations in the changed files (record any pre-existing unrelated violations as unrelated and leave them).

- [ ] **Step 3: Confirm no stray artifacts**

Run:

```bash
git status --short
ls /tmp/twinkle-cfg-gap 2>/dev/null
```

Expected: only intentional source/test/doc files modified; all `--cfg` dumps live under `/tmp/twinkle-cfg-gap` (outside the repo).

- [x] **Step 4: Report the outcome**

**ACTUAL RESULT — Outcome B (with the single-loop case working):**

```text
B. Straight-line + move fixed, loop-carried residual: Gaps B and A landed; the
   loop-carried merge is documented as a separate follow-up.
```

Refinement of B: the STRAIGHT-LINE, sequential-reuse, and SINGLE-LOOP carried cases all
render `verdict -> fN[unique:p0]` and keep the vector non-`Shared` (pinned by `vector_once`,
`vector_twice`, `sieve_loop_set`). `set_at__Bool` summarizes the sound target
`p0=Consumed paths{[]} p1=Borrowed p2=Published ret=alias(p0)`; self-host reaches a fixed
point; census passing at re-baselined 2200. The REAL sieve does not yet render the in-loop
verdict — the residual is isolated to **loop NESTING** (not the interleaved read): an
inner-loop write flows `Shared` through the outer back-edge, so the pessimistic fixpoint
never bootstraps the nested headers to `Unique`. Follow-up = optimistic loop-header seeding.

For `graph_scc.visit`:

```text
- separate recursive/SCC-summary specialization gap (documented follow-up).
```

`visit` stays fully conservative (`p0/p1/p2=Published`, every `record_update` persistent /
`[in_place=false]`, no owned verdict): its threaded record flows into the self-recursive
`visit` call, so the generic SCC summary publishes it. Needs variant seeding across the SCC,
not the non-recursive `.Update` wrapper mechanism fixed here.
