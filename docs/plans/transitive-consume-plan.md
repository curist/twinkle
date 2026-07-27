# Transitive-Consume Delegation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let owned in-place threading compose through delegating (forwarder / transport-wrapper) call hops of arbitrary depth, so chains like `build → resolve → resolve_decls → resolve_one → add` reach in-place instead of collapsing to persistent at the first delegation.

**Architecture:** Three cooperating obstacles must be removed together (a review of an earlier candidacy-only draft proved candidacy-broadening alone is insufficient). (1) Make callee-summary selection **variant-aware** at the forward-analysis call site, per-call-site and gated by `arg_unique`, with generic fallback. (2) Make validated variants **visible during variant validation**, not only at final rendering, so a delegator validated in an earlier SCC is seen by its caller's SCC. (3) Broaden variant **candidacy** to include delegated-consume and transport-wrapper shapes. A resolver is injected into `ownership.tw` so it never imports `summary.tw` (no cycle).

**Tech Stack:** Boot compiler (`boot/compiler/summary.tw`, `boot/compiler/ownership.tw`), Twinkle test suite (`target/twk run boot/tests/main.tw`), CFG render harness (`twk ir <file> --cfg`), self-host (`make stage2`), census (`twk ir boot/main.tw --census --sites`).

---

## Background — three obstacles (verified against code)

The RED fixtures in `boot/tests/fixtures/cfg/sound_uniqueness/` (`red_delegate_chain`,
`red_transport_wrapper_chain`, `red_mixed_delegate_update`) and their locks in
`cfg_sound_uniqueness_fixtures_suite.tw` document the conservative behavior.

1. **Generic-pass publish** (`ownership.tw:4033`). The `MayAliasParams` whole-return
   move only fires for a *proven Unique + last-use* argument (`arg_unique`,
   `ownership.tw:4002`). In the generic pass a threaded param is `Unknown`, so it
   publishes → a delegating hop's generic summary becomes `p0=Published`.
2. **Candidacy excludes delegators** (`summary.tw:694`, `param_has_inplace_site`
   `:662`, `scan_inplace_op` `:636`). Candidacy requires an in-place site **inside the
   function's own body**; a pure delegator's only update is inside its callee, and a
   transport wrapper's ownership arrives via a fresh return field, so neither is a
   candidate.
3. **Validation reads generic-only summaries** (`summary.tw:825`,
   `build_overlay(generic, member_iter)`). During `run_scc_variants`, out-of-SCC
   callees are resolved through the **generic** table, never through already-validated
   variants in `vtable`. So `resolve_one` can validate against `add`'s clean generic
   (`ret=alias(p0)`), but `resolve_decls` validates against `resolve_one`'s *polluted*
   generic (`p0=Published`) and is retracted. Candidacy-broadening flips exactly one
   hop from a leaf, never a chain.

**Selection point** for a user callee's summary in the forward analysis:
`ownership.tw:3841-3849` — `table.summary_get(fid.id)` yields the generic `s`, then
`st.transfer_summarized_call(result, s, args, last, suppress, fid.id)`. This is where
variant-aware selection hooks in.

**Module direction:** `summary.tw:21` imports `compiler.ownership`; `ownership.tw` does
**not** import `summary.tw`. `VariantSummaryTable`/`VariantEntry`/`variant_get` live in
`summary.tw:505-552`. `variant_args_satisfied` (`summary.tw:925`) and `callee_func_id`
(`ownership.tw:3489`) are private. Therefore ownership must stay ignorant of the variant
table type — inject a resolver closure instead.

**Soundness argument.** Variant validation re-runs the guarded forward analysis; the
whole-return move (`ownership.tw:4033`) still fires only when the delegated argument is
genuinely last-use (`arg_unique`, `:4002`). A "delegator" that reads its param after the
delegated call keeps a live use, so `arg_unique` is false, the move declines, the
candidate does not converge, and it is **retracted** (`variant_valid`, `summary.tw:743`).
Broadening candidacy and enabling variant-aware selection only propose/propagate
hypotheses; they never weaken an acceptance guard. Task 2 encodes this as an unconditional
negative fixture that must stay persistent.

---

## Task 1: Baseline

**Files:** none (verification).

- [ ] **Step 1: Confirm the red locks currently pass**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -1`
Expected: `Ran <N> tests: <N> passed`.

- [ ] **Step 2: Snapshot current renders for later diffing**

Run:
```bash
for f in red_delegate_chain red_transport_wrapper_chain red_mixed_delegate_update; do
  target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/$f.tw --cfg > /tmp/before_$f.cfg 2>&1
done
```
Expected: three files; none contains `verdict ->` except inside `red_mixed`'s
`variant fn outer` section.

---

## Task 2: Unconditional negative soundness fixture (write first)

**Files:**
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/red_delegate_read_after.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

- [ ] **Step 1: Write the negative fixture — delegated update, then original read**

`tap` must be shaped so the **only** thing keeping it persistent is the read-after-delegate
guard — not an unrelated candidacy miss. So `tap` returns the delegated result (`Env`, a
candidacy shape whose `ret` aliases `p0` via the delegated call), and reads the *original*
`env` after delegating. The read is routed through `println` so DCE cannot drop it (the
analysis runs over optimized ANF — a dead pure read would be removed, making `env` last-use
again and defeating the fixture):

```
// NEGATIVE soundness fixture. `tap` returns the delegated result (candidacy shape), but
// reads the ORIGINAL `env` after the delegated call. That live use means `env` is not
// last-use at the `add` call, so the seeded owned variant must fail to converge and be
// RETRACTED -- no owned variant, no reuse -- even once delegated-consume candidacy (Task 5)
// and variant-aware selection (Tasks 3-4) land. The println keeps the read live through DCE.
type Env = .{ types: Dict<String, Int> }

fn add(env: Env, k: String, v: Int) Env {
  env.types = .set(k, v)
  env
}

fn tap(env: Env, k: String, v: Int) Env {
  next := add(env, k, v)
  println(env.types.len().to_string())
  next
}

fn build() Env {
  env := Env.{ types: Dict.new() }
  tap(env, "a", 1)
}

println(build().types.len().to_string())
```

Note: this lock is vacuously green until Task 5 makes `tap` a candidate; from Task 5 on it
is load-bearing (Task 5 Step 3 re-checks it). It is the concrete encoding of the soundness
argument above.

- [ ] **Step 2: Format and capture its current (already-correct) render**

Run:
```bash
target/twk fmt boot/tests/fixtures/cfg/sound_uniqueness/red_delegate_read_after.tw
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/red_delegate_read_after.tw --cfg 2>&1 \
  | grep -E "^fn |variant fn |verdict ->|reuse\(unique\)"
```
Expected: `fn tap` present; **no** `variant fn tap`, **no** `verdict ->`, **no**
`reuse(unique)` tied to `tap`.

- [ ] **Step 3: Add the negative lock to the suite (before the closing `}`)**

```
.test(
  "NEG: delegated update then original read stays persistent",
  fn() {
    out := try render_entry("red_delegate_read_after")
    tap := try section_from(out, "fn tap ")
    try assert.is_false(tap.contains("verdict ->"))
    try assert.is_false(out.contains("variant fn tap"))
    try assert.is_false(tap.contains("reuse(unique)"))
    .Ok({})
  },
)
```

- [ ] **Step 4: Run the suite**

Run: `target/twk fmt boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw && target/twk run boot/tests/main.tw 2>&1 | tail -1`
Expected: all pass. This lock must remain green through every later task.

---

## Task 3: Inject a variant resolver into the forward analysis (refactor, no behavior change)

Add a resolver parameter that defaults to "always generic", so this task is a pure
refactor. `ownership.tw` must not name any variant type.

**Files:**
- Modify: `boot/compiler/ownership.tw` — `transfer_call` (`:3826`), `transfer_summarized_call`
  (`:3988`), `analyze_with_summaries` (`:6412`), `summarize_variant` (`:7645`).

- [ ] **Step 1: Define the resolver type as a function alias in `ownership.tw`**

The resolver answers "for this callee and this call site's per-arg uniqueness, is there a
selected non-generic summary?" — returning `.None` means use the generic summary.

```
pub type VariantResolver = fn(Int, Vector<Bool>) Summary?
```

- [ ] **Step 2: Thread the resolver through the call transfer**

In `transfer_call` (`:3826`) add a `resolve: VariantResolver` parameter. At the user-callee
branch (`:3844-3845`), compute the per-site `arg_unique` (reuse the exact logic currently
in `transfer_summarized_call:4002-4007` — factor it into a shared helper
`fn call_arg_unique(st, args, last) Vector<Bool>` so both sites use one definition), then
select:

```
.None => {
  au := call_arg_unique(st, args, last)
  chosen := case resolve(fid.id, au) {
    .Some(vs) => vs,
    .None => case table.summary_get(fid.id) {
      .Some(s) => s,
      .None => return st.publish_call(result, args, cc_suppress),
    },
  }
  st.transfer_summarized_call(result, chosen, args, last, suppress, fid.id)
},
```

- [ ] **Step 3: Add resolver-carrying variants of the analysis entry points**

Twinkle has no function overloading, so do **not** reuse a name. Add new named functions
`analyze_with_summaries_resolved(view, b, sem, table, resolve)` and a
`summarize_variant`-with-resolver form, threading `resolve: VariantResolver` down to every
`transfer_call`. Keep the existing public `analyze_with_summaries` and `summarize_variant`
as thin wrappers that call the resolved forms with a shared `generic_only_resolver =
fn(_, _) { .None }` (this mirrors the existing distinct-name precedent
`analyze_with_summaries_and_entry_seeds`, `ownership.tw:6421`). Find callers to leave on the
generic path with `grep -rn "analyze_with_summaries\b" boot/`.

- [ ] **Step 4: Rebuild and verify byte-identical behavior**

Run:
```bash
make quick-bundle-cli 2>&1 | tail -2
target/twk run boot/tests/main.tw 2>&1 | tail -1
for f in red_delegate_chain red_transport_wrapper_chain red_mixed_delegate_update red_delegate_read_after; do
  diff <(target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/$f.tw --cfg 2>&1) /tmp/before_$f.cfg \
    && echo "$f: unchanged"
done
```
Expected: all tests pass; `red_delegate_chain`/`red_transport_wrapper_chain` unchanged
(`red_delegate_read_after` has no snapshot; skip its diff). Behavior is identical because
the resolver still returns `.None` everywhere. (If `target/boot.wasm` is stale,
`make quick-bundle-cli` will not pick up source edits — run `make bundle-cli`.)

---

## Task 4: Populate the resolver from the validated variant table

Now make the resolver actually select variants, both at render time and during variant
validation.

**Files:**
- Modify: `boot/compiler/summary.tw` — `render_cfg` path, `compute_variants` (`:883`),
  `run_scc_variants` (`:767`), and a new resolver-builder.

- [ ] **Step 1: Add a resolver builder in `summary.tw`**

`summary.tw` knows both the variant table and `variant_args_satisfied` (`:925`). Build a
resolver that, for `(callee_id, arg_unique)`, looks up a validated variant whose
`UniqueReq`s are satisfied by `arg_unique` and returns its summary, else `.None`:

```
fn make_variant_resolver(vtable: VariantSummaryTable) ownership.VariantResolver {
  fn(callee_id, arg_unique) {
    // scan validated variants for this callee; pick the one whose unique-param
    // requirements are all satisfied by arg_unique (variant_args_satisfied), else None.
    variant_summary_for(vtable, callee_id, arg_unique)
  }
}
```

Implement `variant_summary_for` over `vtable.by_key` using `variant_args_satisfied`. When
multiple variants for the same callee are satisfied by `arg_unique`, rank by **specificity
first** (the variant requiring the most unique params / deepest paths), then by canonical
key as a deterministic tie-break. A single-key MVP is fine given `run_scc_variants` seeds one
key per member.

- [ ] **Step 2: Use the resolver at render time (analysis AND verdict rendering)**

`render_cfg` (`summary.tw:1222`) does **not** re-analyze; it renders per-function from the
generic `table` (`summary_get(table, ...)` at `:1232`), and the call-decision renderer reads
`summary_get(table, ...)` at `:469`. So two things must become resolver-aware, or `build`
will transfer correctly yet still render no `verdict ->`:
  1. In the harness/entry render path (`render_entry` in the suite, and any production
     caller), after `variants := compute_variants(...)`, analyze with
     `analyze_with_summaries_resolved(view, b, sem, table, make_variant_resolver(variants))`
     instead of the generic `analyze_with_summaries`.
  2. Thread the resolver into `render_cfg` and down to the call-decision renderer (the
     function around `:469`) so a rendered call verdict consults the selected variant
     summary, not the generic one.

- [ ] **Step 3: Use the resolver DURING validation (the C1 fix)**

In `run_scc_variants` (`:767`), make `summarize_variant` resolver-aware for **out-of-SCC
callees only**. Pass `make_outscc_resolver(vtable, scc_set)` into the resolved
`summarize_variant`: for a callee **not** in `scc_set`, return its `vtable` variant summary
when `arg_unique` satisfies it, else `.None`. For a callee **in** `scc_set`, always return
`.None` so the existing member_iter overlay / Gauss-Seidel path handles it unchanged (this is
how recursion and mutual recursion already validate, and it avoids having to gate in-SCC
iterates by `arg_unique`). `compute_variants` processes SCCs callee-first (`:891-894`), so
earlier-SCC survivors are already in `vtable` when a caller SCC validates.

- [ ] **Step 4: Rebuild and observe the first flips**

Run: `make quick-bundle-cli 2>&1 | tail -2` then re-render the four fixtures.
Expected: `red_mixed_delegate_update` now shows a `verdict ->` inside `fn build [`
(build selects `outer`'s existing variant). `red_delegate_chain` may still be persistent
(delegators are not yet candidates — Task 5). `red_delegate_read_after` must stay
persistent. Run `target/twk run boot/tests/main.tw 2>&1 | tail -1`; the NEG lock must hold.

---

## Task 5: Broaden candidacy — delegated consume (`MayAliasParams`)

**Files:**
- Modify: `boot/compiler/summary.tw` — `scan_inplace_op` (`:636`), `param_has_inplace_site`
  (`:662`), `candidate_variants` (`:689`), threading `generic` down; add `b`/`generic` params
  as needed from `compute_variants` (`:883`).

- [ ] **Step 1: Give the scan access to the generic table**

Thread the generic `SummaryTable` into `scan_inplace_op` and `param_has_inplace_site` (they
are called from `candidate_variants`, which receives `generic`). Do **not** add a
`BuiltinRegistry` unless needed — the delegated-consume test uses only the generic table.

- [ ] **Step 2: Add the delegated-consume case to `scan_inplace_op`**

A derived atom passed to a user callee that returns exactly that arg position as its sole
alias is an in-place opportunity; mark `found` and continue the derived thread onto the
result. Use the **Optional** getter `summary_get` (`ownership.tw:114`, already imported into
`summary.tw:24`) — **not** `table_get`, which traps on a missing id (`summary.tw:341`,
`error("...no seeded summary...")`) and would blow up on builtin/global callees that have no
user summary. Use the verified callee-match idiom (`.AGlobalFunc(fid)` + `fid.id`, as at
`summary.tw:243`):

```
.ACall(callee, args) => case callee {
  .AGlobalFunc(fid) => case summary_get(generic, fid.id) {
    .Some(cs) => {
      hit := false
      for a, j in args {
        if atom_is_derived(st.derived, a) and ret_aliases_exactly_param(cs.ret, j) {
          hit = true
        }
      }
      if hit {
        st.found = true
        st.mark_derived(dst)
      } else {
        st
      }
    },
    .None => st,
  },
  _ => st,
},
```

- [ ] **Step 3: Rebuild and verify the chain composes**

Run: `make quick-bundle-cli 2>&1 | tail -2` then:
```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/red_delegate_chain.tw --cfg 2>&1 \
  | grep -E "variant fn resolve|verdict ->|reuse\(unique\)"
```
Expected: `resolve_one`, `resolve_decls`, `resolve` all earn `[unique:p0]` variants, each
selecting the downstream owned variant, and `build` selects `resolve`. This works only
because Task 4 Step 3 made validated variants visible during validation. Re-run the suite;
the NEG lock must still hold.

---

## Task 6: Broaden candidacy — transport-wrapper (path-sensitive)

`red_transport_wrapper_chain` needs a different shape: `synth` returns `OwnedFresh` with
`ret_paths=.f0=from(p0)`, and `check` does `record_get .f0` then `assign`. This is not a
`MayAliasParams` return, so Task 5's case does not fire.

**Files:**
- Modify: `boot/compiler/summary.tw` — `scan_inplace_op` (`:636`).

- [ ] **Step 1: Track ownership arriving through a fresh return field**

Extend the scan so that a call whose callee summary has `ret_paths` carrying a consumed
param to a return field (e.g. `.f0=from(pK)`) where the passed arg at `pK` is derived,
marks the **result** as carrying that field's ownership; then an `ARecordGet(result, .f0)`
propagates `derived` onto its destination, and the subsequent `AAssign`/`AInit` continues
the thread (existing arms at `:638-654`). Read the `ret_paths` shape from the `Summary`
type (grep `ret_paths` in `summary.tw`/`ownership.tw` to confirm the field/segment
representation before writing the match).

- [ ] **Step 2: Rebuild and verify**

Run: `make quick-bundle-cli 2>&1 | tail -2` then:
```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/red_transport_wrapper_chain.tw --cfg 2>&1 \
  | grep -E "variant fn check|verdict ->|reuse\(unique\)"
```
Expected: `check` earns a `[unique:p0]` variant selecting `synth`'s owned variant, and
`build` selects `check`. NEG lock still holds.

---

## Task 7: Flip the RED locks to green

**Files:**
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`.

- [ ] **Step 1: Rewrite each of the three `RED:` tests to assert the owned outcome**

Drive assertions from the **actual** post-fix renders (do not hand-invent strings). Match
selection tokens, not FuncIds (ids drift). Example for `red_delegate_chain`:

```
out := try render_entry("red_delegate_chain")
try assert.str_contains(out, "variant fn resolve ")
try assert.str_contains(out, "verdict -> f")
try assert.str_contains(out, "reuse(unique)")
```

Apply matching green flips to the transport-wrapper and mixed tests, and drop the `RED:`
prefixes from their titles. Leave the Task 2 NEG lock unchanged.

- [ ] **Step 2: Format, lint, run**

Run:
```bash
target/twk fmt boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
target/twk lint boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
target/twk run boot/tests/main.tw 2>&1 | tail -1
```
Expected: `No findings.` and all tests pass.

---

## Task 8: Regression guard — census and self-host

**Files:** none.

- [ ] **Step 1: Boot-side census must not regress**

Run: `target/twk ir boot/main.tw --census --sites 2>&1 | grep -E "dict_set|vector_set"`
Expected: in-place counts hold or increase vs the "Boot-side census" table in
`docs/plans/sound-uniqueness/analysis/worked-examples.md`. A **drop** means a delegator was
validated then wrongly demoted, or an unsound variant was selected — investigate first.

- [ ] **Step 2: Self-host fixed point**

Run: `make stage2 2>&1 | tail -5`
Expected: stage2 builds and self-host verification passes. Run alone (never concurrently
with other heavy builds).

- [ ] **Step 3: Full suites**

Run: `make test 2>&1 | tail -5`
Expected: all pass.

---

## Task 9: Docs and commit

**Files:**
- Modify: `docs/plans/sound-uniqueness/analysis/worked-examples.md` (census note),
  `docs/plans/sound-uniqueness/README.md` (narrow the transitively-published caveat).

- [ ] **Step 1: Update the worked-examples "Boot-side census" note**

Replace the `run_fixpoint … transitively-published sub-class` sentence with the observed
post-fix behavior: which delegating/transport chains now compose, and which remain
persistent and why (e.g. deep field-backing, read-after-delegate).

- [ ] **Step 2: Commit**

```bash
git add boot/compiler/summary.tw boot/compiler/ownership.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/ \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw \
  docs/plans/sound-uniqueness/
git commit -m "analysis: compose owned threading through delegating call hops

Make callee-summary selection variant-aware at the forward-analysis call site
(per call site, gated by arg_unique, generic fallback) and make validated
variants visible during variant validation, so owned threading composes through
forwarder and transport-wrapper hops of arbitrary depth instead of collapsing to
persistent at the first delegation. Candidacy is broadened for delegated-consume
and transport-wrapper shapes. ownership.tw stays free of variant-table types via
an injected resolver (no import cycle). A read-after-delegate negative fixture
locks the soundness boundary; census and self-host hold."
```

---

## Acceptance criteria

- `red_delegate_chain`, `red_transport_wrapper_chain`, `red_mixed_delegate_update` render
  owned in-place threading end to end; their suite tests assert owned facts by selection
  token (no hard-coded FuncIds).
- `red_delegate_read_after` (read-after-delegate) stays persistent — no owned variant, no
  reuse — throughout.
- Variant-aware selection is per-call-site and `arg_unique`-gated, with generic fallback;
  no global overlay swap that would mis-serve a callee called both uniquely and shared.
- `ownership.tw` does not import `summary.tw`; variant selection reaches it via the injected
  `VariantResolver`.
- Boot-side census in-place counts do not regress; `make stage2` fixed point holds;
  `make test` green.

## Out of scope

- Deep field-backing in-place (`field=persistent(insufficient deep ownership)`) — a
  separate ceiling; only shell/collection threading composes here.
- Emitted codegen for newly-owned variants (variant cloning/dispatch, Phase 8G) — this plan
  is analysis-only; renders change, generated code does not.
