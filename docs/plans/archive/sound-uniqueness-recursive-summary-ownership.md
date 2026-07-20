# Sound Uniqueness: Recursive Summary Ownership (graph_scc.visit) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the boot compiler's interprocedural ownership analysis prove that a state/context record threaded through a *self-recursive* call is uniquely owned, so functions like `graph_scc.visit` render owned in-place record-update verdicts instead of `persistent(aliased shell)`.

**Architecture:** The whole-program summary driver (`boot/compiler/summary.tw:run_scc`) fixed-points each call-graph SCC over **generic (unseeded)** summaries as a monotone *least* fixed point from the conservative top (`Published`). That direction cannot discover the self-supporting fact "this recursive parameter is threaded/consumed and returned owned" — the self-call publishes the argument on every round, pinning the parameter at `Published`. The ownership-specialized building blocks already exist (`ownership.summarize_variant` seeds params `Unique` at entry; `ownership.select_variant` picks the owned `VariantId` a caller demands; `variant_id.tw` canonicalizes/interns keys), but **no fixpoint ever computes a variant-seeded summary across a recursive SCC**. This plan builds that missing layer — Phase 6 **Stage 5 (SCC variant fixpoint/memo)** — as a second, `VariantId`-keyed fixpoint over the same SCC ordering, reusing `summarize_variant`/`select_variant`, and then feeds the converged owned-variant summary into `visit`'s own self-call and into the rendered verdicts.

**Tech Stack:** Twinkle boot compiler (`boot/compiler/summary.tw`, `boot/compiler/ownership.tw`, `boot/compiler/variant_id.tw`), the SCC driver `graph_scc.strongly_connected`, `target/twk ir <file>.tw --cfg`, boot fixtures under `boot/tests/fixtures/cfg/sound_uniqueness/`, boot test suite `target/twk run boot/tests/main.tw`, opt-in census `cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture`, self-host `make stage2` / `make bundle-cli`, and lint `target/twk lint boot/main.tw`.

---

## Architectural Assessment & Recommendation

You asked whether a refactor or major rewrite is warranted. Honest read after tracing the code:

- **This is a bounded extension, not a rewrite.** The analysis primitives (`summarize_variant`, `select_variant`, canonical `VariantId` keys, the Stage 4a whole-return move at `transfer_summarized_call`) are already in place and self-host-verified. What is missing is a *driver*: a `VariantId`-keyed fixpoint that mirrors the existing generic `run_scc`. Reusing the SCC ordering and the monotone-worklist shape from `run_scc` is the low-risk path.
- **The one real architectural decision** (Task 1 decision gate) is *where the variant fixpoint lives*:
  - **Option A — Parallel variant table (recommended).** Add a `VariantSummaryTable` keyed by `VariantId`, computed in a second pass after the generic `compute()`, seeded on demand from call-site `select_variant` decisions, fixed-pointed per SCC. The generic table stays exactly as-is (byte-identical for non-recursive/non-specialized code), which protects the census baseline and self-host.
  - **Option B — Generalize `run_scc` to key on `VariantId`.** More unifying but rewrites the driver that the whole compiler depends on; higher blast radius, harder to keep byte-identical. Reject unless the spike shows Option A cannot converge.
- **The genuine risk is conditional observability, not the variant summary itself.** Stage 5 summaries are only safe to consume when the consumer remains keyed by the same `VariantId` assumption. A generic function body can have both owned and unowned callers, so rendering that single body under a reachable owned seed would be a diagnostic shortcut with codegen-shaped footguns. The revised consumer in Task 5 therefore keeps the generic `fn visit` body conservative and renders owned verdicts only in a separate variant-qualified diagnostic section, e.g. `variant fn visit [unique:p0]`. **Real in-place codegen still requires Stage 6 specialization/cloning keyed by `VariantId`; generic-body rewrites remain out of scope.**

**Recommendation:** Proceed with **Option A**, gated by the Task 1 spike. Treat rendering as the acceptance signal. Keep the generic table and codegen untouched until a follow-up Stage 6 plan.

---

## Global Constraints

- Treat `docs/plans/archive/sound-uniqueness-nested-loop-ownership.md` (just-landed loop-header seeding) and the Phase 6 analysis notes under `docs/plans/sound-uniqueness/analysis/` as the evidence baseline.
- Do not change any **generic** summary. `graph_scc.visit`'s generic summary must remain `p0=Published p1=Published p2=Published ret=alias(p0)`. All new behavior lives in the variant-keyed layer.
- The census baseline (`COW_CEILING`, currently 2200; live count ~2109) must not increase unless a variant summary is actually consumed by codegen in a later plan. An analysis-only Stage 5 must keep census unchanged and the self-host byte-identical.
- Optimism must be validated: a candidate owned variant seeds a parameter `Unique`, but the seed is only sound if (a) the seeded variant fixpoint actually **converges** and proves a non-empty `in_place_paths` for that parameter (cap-hit snapshots are retracted, never published), and (b) the variant is **reachable** in the variant-reachability closure: a root discharged by an **out-of-SCC** caller with a proven-`Unique`, last-use, *occurs-once* argument, or an edge from another reachable variant's seeded analysis. Never render an owned verdict for a candidate that fails either check. Generic `select_variant` alone cannot bootstrap the recursive case, same-SCC generic calls must not root themselves, and generic-call-site-only reachability misses mutually-recursive callees.
- Backedge/recursion classification is by call-graph SCC membership (`scc_set`), consistent with `summary.tw`. This is the interprocedural analogue of the intraprocedural loop-header seeding; do not conflate the two.
- **Determinism:** variant memo keys are the canonical string `variant_id.variant_canonical_string(variant_id.canonicalize_variant(v))` (or its interned dense id), never `site_key` (a call-site pairing). The variant worklist and every iteration over demanded keys process keys in sorted canonical-string order. The generic function body renders once under the generic analysis; every owned verdict is rendered in a separate variant-qualified section keyed by the canonical variant string. Multi-specialization codegen is deferred to Stage 6.
- Never use rendered FuncIds, block ids, or local ids in durable test assertions. Assert stable text such as function names (`fn visit`, `fn scc_thread`), `verdict ->`, `reuse(unique)`, `[unique:p0]`, `Consumed paths{[]}`, and the absence of `persistent(aliased shell)` on the threaded record's updates.
- Write CFG dumps under `/tmp/twinkle-cfg-recur/`, not inside the repository.
- After editing `.tw` files, run `target/twk fmt` on changed Twinkle files, `target/twk lint boot/main.tw`, and explicit `target/twk lint` for any new fixture entries (fixtures are not compiled by `boot/main.tw`).
- Because `boot/compiler/*.tw` changes the self-hosted payload, rebuild with `make bundle-cli` (not `make quick-bundle-cli`) before running the boot suite against new behavior.
- Run verification commands one at a time, never concurrently (concurrent `twk` pegs CPU).
- **This plan is render-only by construction.** The generic ownership verdict analysis remains reached only from `boot/commands/ir.tw`'s `--cfg` path and test suites — never from the build/opt/lowering/codegen pipeline. Variant-specific verdict analysis is also called only by `--cfg` rendering/test helpers and emits diagnostics, not codegen decisions. Consequence: `make stage2` is byte-identical and the census is unchanged **by construction**, not by careful avoidance. The goal here is to close the *analysis/render* gap by making `graph_scc.visit`'s owned variant visible and testable in `--cfg`; Stage 6 must still add variant-keyed cloning/dispatch before any real in-place codegen consumes those verdicts.
- If resuming from an uncommitted spike that added `summary.per_function_seeds(...)` and `ownership.analyze_with_variants(...)` to seed the **generic** CFG body, remove or replace that shape before continuing Task 5. It is intentionally listed as an anti-goal because it can make one conditional owned caller rewrite diagnostics for all callers of the generic body.

---

## File Map

- `boot/compiler/summary.tw` (owns the variant layer — it imports `ownership.tw`, never the reverse)
  - Add the `VariantId`-keyed variant summary table, the second SCC fixpoint pass (`compute_variants`), the variant-reachability pass (`reachable_variants`), and variant-qualified render assembly.
  - Change `render_cfg` to accept the variant table and append `variant:` header lines. Generic per-op verdicts remain generic; owned verdicts render only in separate `variant fn ... [unique:...]` sections assembled by `summary.tw`.
  - Reuse `order_sccs`, `scc_callers`, `insert_sorted`, and the monotone-worklist shape. Same-SCC generic calls are never reachability roots.
- `boot/compiler/ownership.tw` (**seedable per-function helpers only — no `VariantSummaryTable` reference, or it would create a `summary ↔ ownership` import cycle**)
  - Expose whatever `summarize_variant` / `select_variant` glue the variant driver needs; `summary.tw` owns the syntactic candidate scan.
  - Factor the validated loop-header seed cycle out of `ownership_stage` into a shared `run_fixpoint_validated(...)` helper and use it from generic analysis, seeded summaries, and call-site uniqueness scans. `graph_scc.visit` has local loops; the variant summary layer must not be weaker than the final verdict layer.
  - Expose seedable single-function diagnostics (`analyze_function_with_seed`) and call-site scans (`call_uniques`) that accept a `SummaryTable` overlay. Do **not** add an API that rewrites the generic body verdicts under a seed.
  - Harden the shared `arg_unique` with the occurs-once guard (Task 5 Step 1a).
- `boot/commands/ir.tw` (orchestration — has `view`, `b`, `sem`, and the generic `table`)
  - In `render_cfg_artifacts`: compute `variants := summary.compute_variants(...)`, render the generic analyzed view with `ownership.analyze_with_summaries(...)`, and let `summary.render_cfg(...)` append reachable variant-qualified diagnostic sections. Do not call a generic-body `analyze_with_variants` pipeline.
- `boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_state.tw`
  - Minimal self-recursive record-threading positive fixture (the `visit` shape without graph noise).
- `boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_param.tw`
  - Negative fixture: the threaded record enters from a parameter with no owning caller; no owned verdict may render.
- `boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_alias.tw`
  - Negative fixture: an observable alias of the record survives across the recursive call; must stay conservative.
- `boot/tests/fixtures/cfg/sound_uniqueness/multi_param_return_alias.tw`
  - Negative fixture: a function returns one of two params (`ret` aliases two params), so the exact-single-alias candidate gate must reject it — no owned verdict may render.
- `boot/tests/fixtures/cfg/sound_uniqueness/mutual_recursion_thread.tw`
  - Positive fixture: two mutually-recursive functions thread the record (exercises multi-member SCCs, not just self-loops).
- `boot/compiler/cfg.tw`
  - Add `render_function_with_header(func, title, header)` so `summary.tw` can append variant-qualified diagnostic bodies without teaching `cfg.tw` about summaries or variants.
- `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`
  - Positive + negative regression tests. `render_entry` duplicates the CLI pipeline (`compile → build_view → prune → generic analyze → render`); it must mirror `ir.tw` by computing variants and letting `summary.render_cfg` append variant-qualified sections, **or** be refactored to call `commands.ir.render_cfg_for_entry` so there is one pipeline.
- `docs/plans/sound-uniqueness/analysis/README.md` and `.../sieve-cfg-gap-notes.md`
  - Record `graph_scc.visit` status after the fix (or after an explicit analysis-only landing).

---

### Task 1: Decision spike — confirm the mechanism and pick Option A vs B

**Files:**
- Read: `boot/compiler/summary.tw`, `boot/compiler/ownership.tw` (`transfer_summarized_call`, `summarize_variant`, `select_variant`, `summarize_seeded`)
- Create: `/tmp/twinkle-cfg-recur/spike-notes.md`

**Interfaces:**
- Consumes: current generic summary behavior for `graph_scc.visit`.
- Produces: a written decision (Option A vs B), a confirmed seed representation, and a go/split call on render observability. **No production code changes in this task.**

- [ ] **Step 1: Capture the generic-fixpoint-stuck-at-top evidence**

```bash
mkdir -p /tmp/twinkle-cfg-recur
target/twk ir boot/compiler/graph_scc.tw --cfg > /tmp/twinkle-cfg-recur/graph_scc-before.cfg
rg -n "^fn visit|summary:|verdict ->|reuse\(unique\)|persistent\(aliased shell\)|: Shared" /tmp/twinkle-cfg-recur/graph_scc-before.cfg | head -40
```

Expected: `visit` summary `p0=Published p1=Published p2=Published ret=alias(p0)`; every `record_update` verdict on the threaded state renders `shell=persistent(aliased shell)`.

- [ ] **Step 2: Confirm the self-call is the pin**

Read `transfer_summarized_call` (`boot/compiler/ownership.tw:1165`). Confirm in writing that for the `MayAliasParams` self-call the Stage 4a move only fires when `arg_unique[k]` is true, and that in the generic pass the recursive argument enters `Unknown` (never seeded), so the self-call takes the publish branch (`ownership.tw:1214`) and pins `p0` at `Published`.

- [ ] **Step 3: Confirm `summarize_variant`/`select_variant` are sufficient primitives**

Read `summarize_variant` (`ownership.tw:3915`) and `select_variant` (`ownership.tw:3941`). Record whether seeding `visit`'s `p0` `Unique` at entry (via `summarize_variant` with key `{(param:0, path:[])}`) plus a *recursive* variant summary at the self-call would let the Stage 4a move fire. Note the chicken-and-egg: the self-call needs `visit`'s own owned-variant summary, i.e. a variant-level fixpoint.

- [ ] **Step 4: Decide Option A vs B and observability**

Write `/tmp/twinkle-cfg-recur/spike-notes.md` answering:
1. Can a parallel `VariantSummaryTable` (Option A) reach a fixed point for the `{visit}` self-loop SCC using the existing monotone-worklist shape? (Expected: yes — seed the demanded variant, iterate the self-call against the current iterate, stop on `same_summary`.)
2. Confirm the render/verdict seeding path: `analyze_with_summaries → analyze_function → ownership_stage → run_fixpoint(..., unique_seed, seeds)`. Verify that a seeded **variant-qualified diagnostic analysis** can make `block_verdicts` render `reuse(unique)` for `visit`, and that `ownership_stage` reaches codegen nowhere (so this is render-only). Do not plan to overwrite the generic body verdicts under a seed.
3. Final recommendation: Option A (default) or Option B (only if A cannot converge).

- [ ] **Step 5: Commit spike notes only if a repo doc changed**

The spike writes to `/tmp`. Do not commit `/tmp`. If you refined `docs/plans/sound-uniqueness/analysis/*.md` wording, commit that alone:

```bash
git status --short
git add docs/plans/sound-uniqueness/analysis
git commit -m "docs: record recursive-summary ownership spike findings"
```

---

### Task 2: Add the minimal self-recursive positive fixture (preflight only)

**Files:**
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_state.tw`

**Interfaces:**
- Consumes: nothing (an inert fixture file — `boot/main.tw` does not compile fixtures).
- Produces: the minimal `visit`-shaped fixture and recorded evidence of today's conservative render. **No suite test is added here** — the positive test would be red until Task 5's consumer lands, and every commit must stay green (the test is added, green, in Task 5).

- [ ] **Step 1: Create the minimal fixture (the `visit` shape without graph noise)**

Create `boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_state.tw`:

```tw
pub type Acc = .{ total: Int, depth: Int }

pub fn scc_thread(a: Acc, n: Int) Acc {
  cur := a
  cur.total = cur.total + n
  if n > 0 {
    cur.depth = cur.depth + 1
    cur = .scc_thread(n - 1)
  }
  cur
}

pub fn run(n: Int) Int {
  start := Acc.{ total: 0, depth: 0 }
  end := start.scc_thread(n)
  end.total
}
```

`cur` is fresh-or-owned at entry, updated in place (`cur.total = ...`, `cur.depth = ...`), threaded through the self-call `cur = .scc_thread(n - 1)` (which rebinds `cur` to the result), and returned. This is the `graph_scc.visit` ownership shape with no dict/vector noise. The `run` entry gives the recursion an owning caller (a fresh, unique `start`) so the candidate owned variant is caller-dischargeable. Every call uses inherent-method syntax (`start.scc_thread(n)`, `cur = .scc_thread(...)`) so the fixture lints clean under the R1 inherent-call rule.

- [ ] **Step 2: Preflight the current render (expected: conservative) and lint**

```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_state.tw --cfg \
  | rg -n "^fn scc_thread|summary:|verdict ->|reuse\(unique\)|persistent\(aliased shell\)|unique:"
target/twk lint boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_state.tw
```

Expected before the fix: `scc_thread` summary shows `p0=Published` and `ret=alias(p0)` (the `Int` param `p1` renders `Borrowed`, not `Published` — do not rely on primitive param roles); the two `record_update` verdicts render `persistent(aliased shell)`; no `[unique:p0]`; lint reports no findings.

- [ ] **Step 3: Commit the fixture (inert; suite stays green)**

```bash
target/twk fmt boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_state.tw
git add boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_state.tw
git commit -m "test(fixture): add minimal self-recursive threaded-record shape"
```

---

### Task 3: Build the variant summary table and a per-SCC variant fixpoint (Option A)

**Files:**
- Modify: `boot/compiler/summary.tw`
- Modify (accessors only, if the spike found a gap): `boot/compiler/ownership.tw`

**Interfaces:**
- Consumes: `order_sccs`, `scc_callers`, `insert_sorted`, the monotone-worklist shape (`run_scc`), `ownership.summarize_variant`, `ownership.select_variant`, `variant_id` canonicalization/interner.
- Produces: `compute_variants(view, b, sem, generic_table) VariantSummaryTable` — a `VariantId`-keyed table of owned summaries fixed-pointed per SCC, seeded on demand from call-site `select_variant` decisions, leaving the generic table byte-identical.

> **Grounding note for the implementer:** the exact seed set and stop condition are pinned by Task 1's spike notes. The structure below mirrors the generic driver in `summary.tw` (seed → `order_sccs` → per-SCC monotone worklist → `same_summary` stop), but optimistic variant cap handling is **fail-closed**: if the worklist hits its cap before convergence, retract the active variants instead of publishing the optimistic snapshot. Reuse `same_summary`, `insert_sorted`, `int_set`, `in_set`, `build_func_index`, and `order_sccs` verbatim rather than re-deriving them.

- [ ] **Step 1: Add the variant table type keyed by canonical `VariantId`**

In `boot/compiler/summary.tw`, key by the **canonical VariantId string** — `variant_id.variant_canonical_string(variant_id.canonicalize_variant(v))` — so equal owned keys collapse and the fixpoint is order-independent. Do **not** use `variant_id.site_key`: that is a Szudzik pairing of a `(func_id, local_id)` *call site* for the CallDecision table, not a variant identity.

A bare `Dict<String, Summary>` is insufficient: the render needs to (a) recover the `VariantId` from a stored entry (to re-seed the analysis) and (b) enumerate a function's surviving variants to pick the canonical-least. So store an entry record and maintain a `func_id -> sorted canonical keys` index alongside `by_key`. All inserts/lookups go through one `variant_memo_key` helper:

```tw
fn variant_memo_key(v: vid.VariantId) String {
  vid.variant_canonical_string(vid.canonicalize_variant(v))
}

pub type VariantEntry = .{ variant: vid.VariantId, summary: Summary }

pub type VariantSummaryTable = .{
  by_key: Dict<String, VariantEntry>,      // canonical key -> { variant, summary }
  by_func: Dict<Int, Vector<String>>,      // func_id -> its canonical keys, sorted
}

pub fn empty_variant_table() VariantSummaryTable {
  VariantSummaryTable.{ by_key: Dict.new(), by_func: Dict.new() }
}

fn vtable_get(t: VariantSummaryTable, v: vid.VariantId) VariantEntry? {
  t.by_key.get(variant_memo_key(v))
}

fn vtable_put(t: VariantSummaryTable, v: vid.VariantId, s: Summary) VariantSummaryTable {
  key := variant_memo_key(v)
  t.by_key[key] = VariantEntry.{ variant: vid.canonicalize_variant(v), summary: s }
  prev := case t.by_func.get(v.func) {
    .Some(ks) => ks,
    .None => [],
  }
  t.by_func[v.func] = insert_sorted_str(prev, key) // dedup + sorted for canonical-least
  t
}

fn vtable_drop(t: VariantSummaryTable, v: vid.VariantId) VariantSummaryTable {
  key := variant_memo_key(v)
  t.by_key = t.by_key.remove(key)
  case t.by_func.get(v.func) {
    .Some(ks) => t.by_func[v.func] = collect k in ks { k }.filter(fn(k) { k != key }),
    .None => {},
  }
  t
}
```

> `insert_sorted_str` is the `String` analogue of the existing `insert_sorted` (dedup + ascending). Add it next to `insert_sorted`. `Dict.remove` and `Vector.filter` exist in the prelude; if `filter` is unavailable in boot at this call site, rebuild the vector with a `collect`+guard. Retraction (Task 3 Step 3) uses `vtable_drop` so both indices stay consistent.

- [ ] **Step 2: Seed *candidate* owned variants optimistically (not via generic `select_variant`)**

**Why not `select_variant` against the generic table:** `ownership.select_variant` only emits reqs for params whose generic summary already has a non-empty `in_place_paths` (`ownership.tw:3944`). But the recursion pins the threaded param at `Published`, and a `Published` param has *empty* `in_place_paths`. So generic `select_variant` returns the empty (generic) key for `scc_thread`/`visit` — no caller, recursive or outer, can ever demand their owned variant from the generic summary. The demand must be **bootstrapped optimistically**, then validated by the fixpoint (this is the interprocedural analogue of loop-header assume/validate/retract).

Seed a candidate owned variant `{(param k, path [])}` for every `(function f, param k)` where the **generic** summary shows the structural threading signal: `f`'s return aliases **exactly** `k` (`ret = MayAliasParams([k])` — a single-param alias set, not merely *containing* `k`) **and** `k` has at least one in-place-eligible update site in `f`'s body (a `record_update`/COW-update whose base traces to `k`). The *exactly-one* requirement is a soundness gate: the optimistic hypothesis (Step 3) rewrites the return to `alias([k])`, and Stage 4a only moves a return that aliases exactly one param. If the real function may alias several params (`ret = MayAliasParams([j,k])`, e.g. `return cond ? a : b`), forcing `alias([k])` would under-approximate the aliasing and could render an unsound owned move — so multi-param return aliases are **rejected as candidates**. This signal is present in the *pinned* generic summary — `scc_thread`'s generic `ret=alias(p0)` is already exactly `[0]`.

```tw
// A param that the generic summary returns as the SOLE alias AND updates in place is a
// candidate for an owned variant. The seed is a hypothesis; run_scc_variants proves
// or retracts it. Determinism: iterate params in index order, functions in ascending
// func_id (order_sccs already yields deterministic member order).
fn candidate_variants(view: CfgView, generic: SummaryTable) Vector<vid.VariantId> {
  cands: Vector<vid.VariantId> = []
  for f in view.functions {
    s := table_get(generic, f.func_id)
    for ps, k in s.params {
      if ret_aliases_exactly_param(s.ret, k) and param_has_inplace_site(f, k) {
        v := vid.VariantId.{
          func: f.func_id,
          unique: [vid.UniqueReq.{ param: k, path: vid.shell() }],
        }
        cands = .append(vid.canonicalize_variant(v))
      }
    }
  }
  cands
}

// EXACTLY k: the alias set is the single element [k]. `contains k` is unsound here
// (see Step 3 hypothesis rewrite); a multi-param return alias is not a candidate.
fn ret_aliases_exactly_param(ret: ReturnEffect, k: Int) Bool {
  case ret {
    .MayAliasParams(idxs) => idxs.len() == 1 and idxs[0] == k,
    _ => false,
  }
}
```

> `param_has_inplace_site(f, k)` is a **purely syntactic** scan of `f`'s ANF for a record/COW update whose base local traces to param `k`. Do not reuse `ownership.collect_field_reqs`: that analysis is ownership-gated, and recursive publication is exactly the pin this candidate pre-filter must defeat. Seed the derived-local set with `f.params[k].id`; propagate through `AInit`, `AAssign`, direct local copies, record-update results whose base is derived, and block/loop edge arguments that feed block params. `vid.shell()` is the empty (`[]`) `ParamPath` (`variant_id.tw`).

**Reachability is enforced in Task 5, not here.** A candidate that converges to a real owned variant (Step 3) is still only *rendered* if it is reachable in the variant-reachability closure (Task 5 Steps 4-5). Seeding broadly here is sound because nothing consumes the candidate until that check runs.

- [ ] **Step 3: Per-SCC variant fixpoint mirroring `run_scc`**

For each SCC (in the same `order_sccs` order), run an **assume/validate/retract** fixpoint over the *candidate variant keys whose function is in this SCC*, processed in sorted `variant_memo_key` order for determinism.

**Initialize each candidate iterate optimistically, NOT from the generic summary.** Seeding from generic is the bug the reviewer caught: a generic-`Published` param makes `transfer_summarized_call` *publish* the self-call argument (section 1, `.Published => publish_atom`) before Stage 4a can move it, so the seeded analysis can never observe a unique recursive result and the fixpoint stays pinned. Instead seed each candidate key `{(k, [])}` with the **hypothesis** summary — param `k` `Consumed paths{[]}`, `ret = MayAliasParams([k])`, other params from generic — so that at the self-call section 1 leaves the arg alone (`.Consumed => {}`) and section 2 takes the Stage 4a move (the seeded entry makes the argument `arg_unique`). This is a greatest-fixed-point seed; soundness comes from validation + retraction, exactly like loop-header seeding.

```tw
fn optimistic_hypothesis(generic_s: Summary, k: Int) Summary {
  params: Vector<ParamSummary> = collect ps, i in generic_s.params {
    if i == k {
      ParamSummary.{ base_role: .Consumed, in_place_paths: vid.shell_set(), flows_to_return: true }
    } else {
      ps
    }
  }
  Summary.{ params, ret: .MayAliasParams([k]), ret_paths: [] }
}

fn run_scc_variants(
  scc: Vector<Int>,
  index: Dict<Int, CfgFunction>,
  user_ids: Dict<Int, Bool>,
  generic: SummaryTable,
  candidates: Dict<Int, Vector<vid.VariantId>>, // func_id -> its candidate keys (canonical)
  vtable: VariantSummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
) VariantSummaryTable {
  // 1. SEED each candidate (member, key) with optimistic_hypothesis(generic[member], k)
  //    -- NOT the generic summary. vid.shell_set() is the {[]} PathSet.
  // 2. Worklist over keys in sorted variant_memo_key order; recompute a key when an in-SCC
  //    callee variant it depends on changed (scc_callers gives caller edges; a self-loop is
  //    its own caller). summarize_variant(f, key, overlay, ...) reads `overlay` = generic
  //    with the current in-SCC variant iterates layered on top, so the self-call resolves to
  //    the owned key. Stop only when same_summary reaches a real fixed point. If the inner
  //    loop hits its safety cap with `changed == true`, retract every still-active candidate
  //    in this SCC and publish no cap-time optimistic snapshot.
  // 3. VALIDATE + RETRACT: after convergence, drop (via vtable_drop) any key whose converged
  //    summary lost the hypothesis. A key survives ONLY if its converged summary has BOTH:
  //      (a) the seeded param `k` Consumed with non-empty in_place_paths (ownership proven), AND
  //      (b) ret STILL exactly alias([k]) -- ret_aliases_exactly_param(conv.ret, k). If the
  //          seeded analysis widened the return to alias more params (or to Shared), the move
  //          hypothesis no longer holds -> retract. An alias forcing a publish fails (a); a
  //          multi-param return widening fails (b).
  //    Dropping a key may invalidate a co-member that depended on it, so re-run the SCC until
  //    the surviving key set is stable (each pass only removes keys, so this terminates).
  vtable
}
```

Note `vid.shell_set()` (the `{[]}` `PathSet`, `variant_id.tw:52`) for the hypothesis's `in_place_paths`, matching how the fixpoint reports a shell consume.

> The overlay ("generic + variant-iterate at the self-call") is the crux. Task 1 Step 3 records whether `transfer_summarized_call` can be pointed at a variant summary for an in-SCC callee via the existing `table` argument (a `SummaryTable` whose `by_func` entry for the recursive callee is temporarily the current variant iterate), or whether a small variant-lookup hook is needed next to `summary_get`. Implement whichever the spike confirms; do not change generic-table reads for out-of-SCC callees. **Multi-key caveat:** `SummaryTable.by_func` holds one summary per `func_id`, so the overlay can represent only one variant per in-SCC callee at a time. That is exact for this plan's fixtures and `graph_scc.visit` (each threaded function has a single candidate key). If a real in-SCC callee has several candidate keys, do not silently pick `keys[0]`: assert/retract for now, or explicitly select the demanded key through `select_variant` before broadening the plan.

- [ ] **Step 4: Add the `compute_variants` entry point**

```tw
fn candidates_by_func(view: CfgView, generic: SummaryTable) Dict<Int, Vector<vid.VariantId>> {
  by_func: Dict<Int, Vector<vid.VariantId>> = Dict.new()
  for v in candidate_variants(view, generic) {
    prev := case by_func.get(v.func) {
      .Some(vs) => vs,
      .None => [],
    }
    by_func[v.func] = .append(v)
  }
  by_func
}

pub fn compute_variants(
  view: CfgView,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  generic: SummaryTable,
) VariantSummaryTable {
  index := build_func_index(view)
  user_ids := user_id_set(view)
  candidates := candidates_by_func(view, generic)
  vtable := empty_variant_table()
  for scc in order_sccs(view, index, user_ids) {
    vtable = run_scc_variants(scc, index, user_ids, generic, candidates, vtable, b, sem)
  }
  vtable
}
```

- [ ] **Step 5: Build and self-host with NO consumer yet (must be byte-identical)**

At this point nothing reads `compute_variants`, so it must be dead and change nothing.

```bash
make bundle-cli
target/twk run boot/tests/main.tw 2>&1 | rg -n "passed|FAIL"
make stage2
cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture 2>&1 | rg -n "TOTAL COW remaining|test result"
```

Expected: suite passes (no positive test exists yet — it is added, green, in Task 5); self-host reaches a fixed point; census unchanged (~2109). If census moved, `compute_variants` is not actually dead — find the accidental call before continuing.

- [ ] **Step 6: Commit the analysis layer (still unconsumed)**

```bash
target/twk fmt boot/compiler/summary.tw boot/compiler/ownership.tw
git add boot/compiler/summary.tw boot/compiler/ownership.tw
git commit -m "ownership: compute variant-keyed summaries over recursive SCCs"
```

---

### Task 4: Prove the self-loop fixpoint converges to the owned variant

**Files:**
- Modify: `boot/compiler/summary.tw` (widen `render_cfg` to take the variant table; render `variant:` header lines)
- Modify: `boot/commands/ir.tw` (`render_cfg_artifacts` computes the variant table and passes it to `render_cfg`)
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw` (`render_entry` mirrors the same two lines)

**Interfaces:**
- Consumes: `compute_variants` from Task 3.
- Produces: direct evidence that `scc_thread`'s owned variant summary converges to `p0=Consumed paths{[]} ret=alias(p0)` before any *verdict* change, isolating the fixpoint from the render-verdict wiring. This step only prints a summary line — it does not yet change any materialized `reuse(unique)`/`persistent(...)` verdict (that is Task 5).

- [ ] **Step 1: Widen `render_cfg` and compute the variant table at the call sites**

`summary.render_cfg` is currently `(view: CfgView, table: SummaryTable)` and has no `b`/`sem`, so it cannot call `compute_variants` itself. Two coordinated changes:

1. In `summary.tw`, change the signature to `render_cfg(view, table, variants: VariantSummaryTable)` and, for each function with a surviving owned variant (`variants.by_func[func_id]` non-empty), append a `variant: ...` header line per key via the existing `render_summary` (look the summary up through `variants.by_key`). Header-only — the per-op verdicts stay generic in this task.
2. At **both** call sites, compute the variant table (they already have `b`/`sem` and the generic `table`) and pass it in:
   - `boot/commands/ir.tw::render_cfg_artifacts` (`ir.tw:55-57`): after `table := summary.compute(view, b, s)`, add `variants := summary.compute_variants(view, b, s, table)` and call `summary.render_cfg(analyzed, table, variants)`.
   - `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw::render_entry` (`suite:17-28`) duplicates that pipeline — add the same `variants := summary.compute_variants(...)` line and pass it to `render_cfg`. (If you prefer one pipeline, refactor `render_entry` to call `commands.ir.render_cfg_for_entry`; either is fine, but the suite helper MUST be updated or the added tests render without variants.)

- [ ] **Step 2: Assert convergence on the minimal fixture**

```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_state.tw --cfg \
  | rg -n "^fn scc_thread|summary:|variant:"
```

Expected: the **generic** `summary:` line is unchanged; a new **`variant:`** line for `scc_thread` shows `p0=Consumed paths{[]}` with `ret=alias(p0)`. This proves the self-call's Stage 4a move fired under the seeded, fixed-pointed variant. (Per-op `reuse(unique)` verdicts are still absent — that is Task 5.) **Do not assert the `Int` param's role** — primitive params render `Borrowed`, not `Published`; assert only `p0=Consumed paths{[]}` and `ret=alias(p0)`.

- [ ] **Step 3: Commit the convergence probe**

```bash
target/twk fmt boot/compiler/summary.tw boot/commands/ir.tw boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git add boot/compiler/summary.tw boot/commands/ir.tw boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git commit -m "ownership: surface converged recursive variant summary in --cfg header"
```

---

### Task 5: Derisk the consumer and render variant-qualified owned verdicts

**Files:**
- Modify: `boot/compiler/ownership.tw` — harden `arg_unique`, factor validated loop-seed fixpoint reuse, expose seedable single-function analysis and call-site uniqueness scans. **No `VariantSummaryTable` reference here** (avoids the `summary ↔ ownership` cycle).
- Modify: `boot/compiler/summary.tw` — owns `VariantSummaryTable`; add external-SCC reachability, variant-summary overlays, and variant-qualified render sections.
- Modify: `boot/compiler/cfg.tw` — add a tiny render helper that can render one already-analyzed `CfgFunction` with a caller-supplied title/header, keeping `cfg.tw` free of summary/ownership imports.
- Modify: `boot/commands/ir.tw::render_cfg_artifacts` — keep the generic analyzed view generic; compute variants and pass them to `summary.render_cfg(...)` so it can append variant diagnostic sections.
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw` — mirror the CLI pipeline and add positive tests against variant-qualified sections.

**Module-layering rule (blocker):** `summary.tw` imports `ownership.tw`, never the reverse (`summary.tw` header comment). So anything touching `VariantSummaryTable` lives in `summary.tw` / `boot/commands/ir.tw`, not `ownership.tw`. `ownership.tw` only accepts ordinary `SummaryTable` overlays and ordinary `unique_seed` maps.

**Interfaces:**
- Consumes: the converged, retraction-filtered `VariantSummaryTable` from Task 3/4.
- Produces: separate `variant fn ... [unique:...]` diagnostic sections whose record-update verdicts render `reuse(unique)` for `scc_thread`, `ping`/`pong`, and `graph_scc.visit`; the generic `fn ...` sections and generic summaries remain conservative/unchanged.

> **Resume note for implementers:** If a prior uncommitted spike added `per_function_seeds(...)` plus `ownership.analyze_with_variants(...)` that rewrites the generic body under a seed, do not land that shape. Keep useful pieces (occurs-once guard, seedable helpers), but route owned verdicts into variant-qualified sections only.

- [ ] **Step 0: Remove the seeded generic-body spike before adding the new consumer**

If the working tree contains the earlier Task 5 spike, delete the generic-body seeding path before writing the new RED tests:

- In `boot/commands/ir.tw` and `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`, stop computing `seeds := summary.per_function_seeds(...)` and stop calling `ownership.analyze_with_variants(...)` for the normal view. The normal view must be analyzed with `ownership.analyze_with_summaries(view, b, sem, table)`.
- In `boot/compiler/ownership.tw`, either remove `analyze_with_variants(...)` or leave it unused/private only if later refactoring immediately replaces it with `analyze_function_with_seed(...)`. No public API should encourage seeding the generic function body.
- In `boot/compiler/summary.tw`, remove `per_function_seeds(...)` if its only consumer is generic-body seeding. Reachability should feed variant-section rendering, not a generic `func_id -> seed` map.

Focused verification:

```bash
target/twk build boot/main.tw -o /tmp/newboot.wasm
BOOT_WASM=/tmp/newboot.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs ir boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_state.tw --cfg \
  | rg -n "^fn scc_thread|summary:|variant:|reuse\(unique\)|persistent\(aliased shell\)"
```

Expected before the new variant-section renderer: the generic `fn scc_thread` body is conservative (`persistent(aliased shell)`), even though the Task 4 `variant:` header may still appear.

- [ ] **Step 1: Add RED tests for variant-qualified rendering, not generic-body rewriting**

In `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`, add a helper that extracts a variant section by stable text. Variant sections may be appended after the whole generic CFG, so do not bound them with generic function markers like `fn run`; bound them by the next `variant fn ` marker or EOF:

```tw
fn variant_section(out: String, func_name: String, key: String) Result<String, String> {
  start_marker := "variant fn ${func_name} [${key}]"
  start := try out.index_of(start_marker).ok_or("missing variant section ${start_marker}")
  tail := out.substring(start, out.len())
  next := case tail.substring(1, tail.len()).index_of("\nvariant fn ") {
    .Some(i) => i + 1,
    .None => tail.len(),
  }
  .Ok(tail.substring(0, next))
}
```

Add the minimal fixture test after the nested-loop tests. It must assert the generic body is still conservative and the variant body is owned:

```tw
    .test(
      "self-recursive threaded record renders an owned variant verdict",
      fn() {
        out := try render_entry("recursive_thread_state")
        generic := try section_between(out, "fn scc_thread", "fn run")
        try assert.str_contains(generic, "persistent(aliased shell)")
        variant := try variant_section(out, "scc_thread", "unique:p0")
        try assert.str_contains(variant, "variant: p0=Consumed paths{[]}")
        try assert.str_contains(variant, "reuse(unique)")
        try assert.is_false(variant.contains("persistent(aliased shell)"))
        .Ok({})
      },
    )
```

Add a `graph_scc.visit` assertion to the existing visit-like fixture test (or a new test in the same suite if `visit_main` is the fixture entry):

```tw
        variant := try variant_section(out, "visit", "unique:p0")
        try assert.str_contains(variant, "variant: p0=Consumed paths{[]}")
        try assert.str_contains(variant, "reuse(unique)")
```

Run the suite with the current generic-only render:

```bash
target/twk run boot/tests/main.tw 2>&1 | rg -n "self-recursive threaded record|visit-like threaded|FAIL|passed"
```

Expected: FAIL because no `variant fn ...` section exists yet. If the generic body already renders `reuse(unique)`, the seeded generic-body spike has not been removed.

- [ ] **Step 2: Make optimistic variant cap handling fail closed**

In `boot/compiler/summary.tw::run_scc_variants`, track whether the inner fixpoint actually converged:

```tw
converged := false
for changed and rounds < cap_inner {
  changed = false
  ...
  rounds = rounds + 1
}
converged = !changed
if !converged {
  // The iterate started optimistic. A cap-time snapshot is not conservative.
  // Retract every active member in this SCC and publish no survivors from this pass.
  member_pidx = Dict.new()
  member_key = Dict.new()
  stable = true
}
```

Only run `variant_valid` and publish survivors when `converged` is true. This is a blocker: no optimistic cap snapshot may render, even if it happens to satisfy `variant_valid`.

Focused verification:

```bash
target/twk build boot/main.tw -o /tmp/newboot.wasm
BOOT_WASM=/tmp/newboot.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs ir boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_state.tw --cfg \
  | rg "^fn scc_thread|summary:|variant:"
```

Expected: `scc_thread` still has the Task 4 `variant:` header. If it disappears, the cap handling is retracting converged SCCs by mistake.

Also inspect `run_scc_variants` before committing this task: every call to `vtable_put` must be dominated by the `converged` branch. This structural check is required because a deterministic cap-hit fixture would depend on the current widening cap and could become flaky when iteration order or caps change.

- [ ] **Step 3: Factor validated loop-seed fixpoint reuse**

`graph_scc.visit` carries `cur` through local `for` loops. The final ownership pass already has loop-header assume/validate/retract in `ownership_stage`, but `summarize_seeded` and call-site scans must use the same validated loop seeds or the variant summary layer is weaker than the verdict layer.

In `boot/compiler/ownership.tw`, factor this helper next to `run_fixpoint`:

```tw
fn run_fixpoint_validated(
  blocks: Vector<CfgBlock>,
  params: Vector<LocalId>,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
  unique_seed: Dict<Int, Bool>,
) FixResult {
  seeds := collect_loop_seed_candidates(blocks)
  fx := run_fixpoint(blocks, params, table, b, sem, suppress, unique_seed, seeds)
  stable_seeds := false
  for !stable_seeds {
    kept := retain_valid_loop_seeds(blocks, fx, seeds)
    if same_loop_seed_set(kept, seeds) {
      stable_seeds = true
    } else {
      seeds = kept
      fx = run_fixpoint(blocks, params, table, b, sem, suppress, unique_seed, seeds)
    }
  }
  fx
}
```

Then replace the duplicated loop-seed block in `ownership_stage` with `run_fixpoint_validated(...)`, and replace the `empty_seeds` calls in `summarize_seeded` and `call_uniques` with `run_fixpoint_validated(...)` using the same `table`, `suppress`, and `unique_seed` arguments.

Focused verification:

```bash
target/twk build boot/main.tw -o /tmp/newboot.wasm
BOOT_WASM=/tmp/newboot.wasm deno run --allow-read --allow-write --allow-env \
  tools/js_runtime/deno_main.mjs ir boot/compiler/graph_scc.tw --cfg \
  | rg -n "^fn visit|summary:|variant:" | head -20
```

Expected: `visit`'s generic `summary:` remains `p0=Published p1=Published p2=Published ret=alias(p0)`, and a `variant:` line for `visit` appears with `p0=Consumed paths{[]}` and `ret=alias(p0)`. If the variant still retracts, inspect the converged seeded summary before changing validity; do not weaken validation blindly.

- [ ] **Step 4: Harden call-site uniqueness and reachability roots**

In `ownership.tw`, fold the occurs-once guard into every call `arg_unique` computation used by Stage 4a moves, call-decision rendering, and public call scans:

```tw
fn atom_occurs_once(args: Vector<Atom>, id: Int) Bool {
  count := 0
  for a in args {
    case atom_local_id(a) {
      .Some(other) => if other == id { count = count + 1 },
      .None => {},
    }
  }
  count == 1
}
```

Use `local_reusable(..., id, last) and atom_occurs_once(args, id)`.

In `summary.tw::reachable_variants`, compute a `func_id -> scc_id` map from `order_sccs(...)`. Root a candidate only from a call site whose caller's SCC differs from the callee's SCC. Same-SCC calls propagate only after the caller variant is already reachable. This prevents `recursive_thread_param` from bootstrapping itself through its self-call.

Add or expose this ownership helper without importing variants into `ownership.tw`:

```tw
pub type CallUniq = .{ callee: Int, arg_unique: Vector<Bool> }

pub fn call_uniques(
  f: CfgFunction,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
  unique_seed: Dict<Int, Bool>,
) Vector<CallUniq>
```

`call_uniques` must use `run_fixpoint_validated(...)`, not a raw `run_fixpoint(..., empty_seeds)`. Both root scans and variant-propagation scans pass the caller's SCC set as `suppress`, matching summary SCC handling and preventing in-SCC speculative return-path details from leaking into the scan.

- [ ] **Step 5: Use variant summary overlays for reachable variant analysis**

For a reachable variant body, recursive calls must transfer through converged variant summaries, not the generic table. In `summary.tw`, add a helper that overlays the canonical reachable variant summary for each function onto the generic table:

```tw
fn reachable_overlay(generic: SummaryTable, variants: VariantSummaryTable, reachable: Dict<String, Bool>) SummaryTable {
  overlay := generic
  for func_id, keys in variants.by_func {
    picked := false
    for key in keys {
      if !picked and reach_has(reachable, key) {
        case variants.by_key.get(key) {
          .Some(entry) => {
            overlay = table_put(overlay, func_id, entry.summary)
            picked = true
          },
          .None => {},
        }
      }
    }
  }
  overlay
}
```

This helper intentionally supports one reachable variant per function for this plan. If a function has more than one reachable key, keep the canonical-least for render determinism and emit only that diagnostic section; Stage 6 cloning can generalize multi-specialization. Do not render every reachable key with a single canonical-least overlay, because non-picked variants could be analyzed through the wrong recursive summary.

Reachability propagation is a fixed point over both the reachable set and the overlay derived from it:
1. seed roots from out-of-SCC generic callers;
2. build `overlay := reachable_overlay(generic, variants, reachable)`;
3. rescan every currently reachable canonical-least variant body with `ownership.call_uniques(f, overlay, b, sem, scc_set, unique_seed_for_variant(...))`;
4. mark newly demanded variants;
5. repeat from step 2 until no new keys are marked.

This rescan is required because discovering `ping` can add `pong` to the overlay, and already-reachable bodies may then preserve ownership through calls that were previously generic.

- [ ] **Step 6: Render variant-qualified function sections**

In `cfg.tw`, add a helper that renders one analyzed function with caller-supplied title/header text:

```tw
pub fn render_function_with_header(func: CfgFunction, title: String, header: String) String {
  lines: Vector<String> = [title]
  if header.len() > 0 {
    for line in header.lines() {
      lines = .append("  ${line}")
    }
  }
  for block in func.blocks {
    lines = render_block(lines, block)
  }
  join_lines(lines)
}
```

In `ownership.tw`, expose a seedable single-function analyzer:

```tw
pub fn analyze_function_with_seed(
  f: CfgFunction,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  unique_seed: Dict<Int, Bool>,
) CfgFunction
```

This should share the same internal `analyze_function` implementation used by `analyze_with_summaries`, including `run_fixpoint_validated(...)`.

Change `summary.render_cfg` in Task 5 to receive both the pruned source view and the generic analyzed view, plus `b`/`sem` for variant diagnostics:

```tw
pub fn render_cfg(
  source: CfgView,
  generic_analyzed: CfgView,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  generic: SummaryTable,
  variants: VariantSummaryTable,
) String
```

Inside `render_cfg`, keep the generic output first:

```tw
out := generic_analyzed.render_view_with_headers(headers)
```

Then append at most one section per function: the canonical-least reachable variant for that function. Move Task 4's generic-header `variant:` lines into these reachable variant sections (or stop rendering them under generic function headers) so unreachable converged candidates cannot look consumable.

```text
variant fn visit [unique:p0]
  variant: p0=Consumed paths{[]} p1=Published p2=Published ret=alias(p0)
  block ...
    verdict ... shell=reuse(unique) ...
```

Build each variant section by:
1. computing `reachable := reachable_variants(...)` and `overlay := reachable_overlay(generic, variants, reachable)` after the reachability fixed point stabilizes;
2. for each function, picking only its canonical-least reachable variant key;
3. looking up the original `CfgFunction` in `source` (the un-analyzed pruned view);
4. computing `unique_seed_for_variant(f, entry.variant)`;
5. analyzing that one function with `ownership.analyze_function_with_seed(f, overlay, b, sem, seed)`;
6. rendering it with `cfg.render_function_with_header(...)` and the title `variant fn ${f.name} [${render_variant_key(entry.variant)}]`.

Do not change the generic function body's verdicts under a seed.

- [ ] **Step 7: Run the focused positive checks**

```bash
make bundle-cli
target/twk run boot/tests/main.tw 2>&1 | rg -n "self-recursive threaded record|visit-like threaded|FAIL|passed"
target/twk ir boot/compiler/graph_scc.tw --cfg > /tmp/twinkle-cfg-recur/graph_scc-after.cfg
rg -n "^fn visit|^variant fn visit|summary:|variant:|reuse\(unique\)|persistent\(aliased shell\)" \
  /tmp/twinkle-cfg-recur/graph_scc-after.cfg | head -80
```

Expected:
- Generic `fn visit` summary remains `p0=Published p1=Published p2=Published ret=alias(p0)`.
- Generic `fn visit` body may still show `persistent(aliased shell)`.
- `variant fn visit [unique:p0]` exists and shows `variant: p0=Consumed paths{[]}` plus `reuse(unique)` record-update verdicts.
- No unreachable-only function (for example `recursive_thread_param` once Task 6 lands) shows a generic-header `variant:` line that could be mistaken for a reachable owned body.
- The minimal fixture's generic body stays conservative and its variant body renders `reuse(unique)`.

- [ ] **Step 8: Commit the derisked consumer + positives**

```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/summary.tw boot/compiler/cfg.tw \
  boot/commands/ir.tw boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/compiler/summary.tw boot/compiler/cfg.tw \
  boot/commands/ir.tw boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git commit -m "ownership: render recursive owned verdicts as variant diagnostics"
```

---


### Task 6: Negative fixtures — parameter, alias, and retraction safety

**Files:**
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_param.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_alias.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/multi_param_return_alias.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/mutual_recursion_thread.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: Task 5's rendering.
- Produces: guards that an unowned parameter, a live alias, a multi-param return alias, and mutual recursion behave correctly (conservative where they must be, owned where sound).

- [ ] **Step 1: Parameter negative fixture (no external caller can discharge)**

Create `boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_param.tw`:

```tw
pub type Acc = .{ total: Int, depth: Int }

pub fn thread_param(a: Acc, n: Int) Acc {
  cur := a
  cur.total = cur.total + n
  if n > 0 {
    cur = .thread_param(n - 1)
  }
  cur
}
```

The candidate variant for `thread_param` may even *converge* (the body would consume a unique `p0`), but its only call site is the self-call. Same-SCC calls are not reachability roots, and no external caller passes a proven-unique record, so no `variant fn thread_param [unique:p0]` section may render. This isolates the reachability gate from the fixpoint. There is no bare-call to a same-module-typed function, so it lints clean (the recursion uses inherent `cur = .thread_param(...)`).

- [ ] **Step 2: Alias negative fixture (live alias across the recursive call)**

Create `boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_alias.tw`:

```tw
pub type Acc = .{ total: Int, depth: Int }

pub fn thread_alias(cur: Acc, alias: Acc, n: Int) Acc {
  c := cur
  c.total = c.total + alias.total
  if n > 0 {
    c = .thread_alias(alias, n - 1)
  }
  c
}

pub fn run_alias(n: Int) Int {
  start := Acc.{ total: 0, depth: 0 }
  end := start.thread_alias(start, n)
  end.total
}
```

`run_alias` passes the same fresh `start` as both `cur` and `alias`, so inside `thread_alias` the update `c.total = ...` mutates a record that `alias` still observes (and `alias` is passed live into the recursive call). No owned variant section may render — the live alias blocks ownership, whether via fixpoint retraction or via the dischargeability gate (`start` occurs twice in one call, so it is not `arg_unique`). Either sound path keeps the update conservative. Every call is inherent (`start.thread_alias(start, n)`, `c = .thread_alias(...)`) so it lints clean; unlike the earlier draft, `c = .thread_alias(alias, n - 1)` rebinds `c: Acc` (not `c.total: Int`), so it typechecks.

- [ ] **Step 3: Multi-param return-alias negative fixture (candidate gate)**

Create `boot/tests/fixtures/cfg/sound_uniqueness/multi_param_return_alias.tw`:

```tw
pub type Acc = .{ total: Int, depth: Int }

pub fn pick(a: Acc, b: Acc, cond: Bool) Acc {
  r := if cond { a } else { b }
  r.total = r.total + 1
  r
}

pub fn run_pick(n: Int) Int {
  x := Acc.{ total: 0, depth: 0 }
  y := Acc.{ total: n, depth: 0 }
  end := x.pick(y, n > 0)
  end.total
}
```

`pick` returns `a` or `b`, so its generic `ret = MayAliasParams([0, 1])` — two params. The exact-single-alias candidate gate (`ret_aliases_exactly_param`, Task 3 Step 2) must **reject** both `(pick, 0)` and `(pick, 1)`: forcing the hypothesis `alias([0])` would drop the alias to `b` and could render an unsound move. So `pick`'s `r.total = ...` update must stay `persistent`. This guards the exact-single rule against regression to a `contains k` check. All calls are inherent (`x.pick(y, ...)`) so it lints clean.

- [ ] **Step 4: Mutual-recursion positive fixture (multi-member SCC)**

Create `boot/tests/fixtures/cfg/sound_uniqueness/mutual_recursion_thread.tw`:

```tw
pub type Acc = .{ total: Int, depth: Int }

pub fn ping(a: Acc, n: Int) Acc {
  cur := a
  cur.total = cur.total + n
  if n > 0 {
    cur = .pong(n - 1)
  }
  cur
}

pub fn pong(a: Acc, n: Int) Acc {
  cur := a
  cur.depth = cur.depth + 1
  if n > 0 {
    cur = .ping(n - 1)
  }
  cur
}

pub fn run_mut(n: Int) Int {
  start := Acc.{ total: 0, depth: 0 }
  end := start.ping(n)
  end.total
}
```

`ping`/`pong` form a two-member SCC; both should acquire owned variant sections and render `reuse(unique)` there while their generic bodies stay conservative if they also have unowned entries. This exercises variant reachability: `run_mut` is an external root discharging `ping`'s owned variant, and `ping`'s seeded variant analysis then discharges `pong`'s owned variant via the `cur = .pong(...)` edge — `pong` is **not** reachable from any generic external caller, so a generic-only dischargeability check would wrongly leave it conservative.

- [ ] **Step 5: Add the tests**

```tw
    .test(
      "recursive threaded parameter stays conservative",
      fn() {
        out := try render_entry("recursive_thread_param")
        // thread_param is the only user function; section_from avoids any
        // end-marker prefix hazard.
        body := try section_from(out, "fn thread_param")
        try assert.is_false(body.contains("reuse(unique)"))
        try assert.is_false(out.contains("variant fn thread_param"))
        try assert.is_false(body.contains("variant: p0=Consumed"))
        .Ok({})
      },
    )
    .test(
      "recursive threaded aliased record stays conservative",
      fn() {
        out := try render_entry("recursive_thread_alias")
        // End marker "fn run_alias" is NOT a prefix of "fn thread_alias", so the
        // section is bounded correctly (the earlier "fn thread_alias_go"/"fn
        // thread_alias" pair self-overlapped and yielded an empty section).
        body := try section_between(out, "fn thread_alias", "fn run_alias")
        try assert.is_false(body.contains("reuse(unique)"))
        try assert.is_false(out.contains("variant fn thread_alias"))
        try assert.is_false(body.contains("variant: p0=Consumed"))
        .Ok({})
      },
    )
    .test(
      "multi-param return alias stays conservative",
      fn() {
        out := try render_entry("multi_param_return_alias")
        body := try section_between(out, "fn pick", "fn run_pick")
        try assert.is_false(body.contains("reuse(unique)"))
        try assert.is_false(out.contains("variant fn pick"))
        try assert.is_false(body.contains("variant: p0=Consumed"))
        try assert.is_false(body.contains("variant: p1=Consumed"))
        .Ok({})
      },
    )
    .test(
      "mutually recursive threaded record renders owned in-place verdicts",
      fn() {
        out := try render_entry("mutual_recursion_thread")
        ping := try variant_section(out, "ping", "unique:p0")
        try assert.str_contains(ping, "reuse(unique)")
        pong := try variant_section(out, "pong", "unique:p0")
        try assert.str_contains(pong, "reuse(unique)")
        .Ok({})
      },
    )
```

- [ ] **Step 6: Run, lint, and commit**

```bash
target/twk run boot/tests/main.tw 2>&1 | rg -n "recursive threaded parameter|recursive threaded aliased|multi-param return alias|mutually recursive threaded|self-recursive threaded record|FAIL|passed"
target/twk lint boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_param.tw
target/twk lint boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_alias.tw
target/twk lint boot/tests/fixtures/cfg/sound_uniqueness/multi_param_return_alias.tw
target/twk lint boot/tests/fixtures/cfg/sound_uniqueness/mutual_recursion_thread.tw
```

Expected: all five positive/negative tests pass; lint clean.

```bash
target/twk fmt \
  boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_param.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_alias.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/multi_param_return_alias.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/mutual_recursion_thread.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git add \
  boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_param.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/recursive_thread_alias.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/multi_param_return_alias.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/mutual_recursion_thread.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git commit -m "test: guard recursive uniqueness against params, aliases, multi-param returns, mutual recursion"
```

---

### Task 7: Final verification and docs

**Files:**
- Modify: `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md`
- Modify: `docs/plans/sound-uniqueness/analysis/README.md`
- Modify: `docs/plans/README.md` (remove this plan's row on completion, per repo convention)

**Interfaces:**
- Consumes: the landed implementation and fixtures.
- Produces: recorded status and full-suite validation.

- [ ] **Step 1: Full verification, one command at a time**

```bash
target/twk run boot/tests/main.tw
cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture
make stage2
target/twk lint boot/main.tw
```

Expected:
- Boot suite passes.
- Census **unchanged** (~2109) — this plan only touches the `--cfg` render path, never codegen, so the count cannot move. Any change at all means something is wired incorrectly (e.g. variant diagnostic analysis leaked into the build pipeline) — investigate before re-baselining.
- Self-host **byte-identical** (`make stage2` reaches a fixed point) for the same reason.
- Lint clean.

- [ ] **Step 2: Update the analysis notes**

In `docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md` and `.../README.md`, record `graph_scc.visit` as **analysis-resolved**: the owned variant now renders in `--cfg`; the codegen handoff that turns the variant-qualified diagnostic decision into a real in-place mutation is the separate Stage 6. Remove `graph_scc.visit` from any active-deferral list where it now overstates the *analysis* gap, but keep a Stage 6 (codegen consumption) follow-up note.

- [ ] **Step 3: Retire this plan and archive**

Per repo convention (`docs/plans/README.md`), on completion remove this plan's row from `docs/plans/README.md` and move the doc to `docs/plans/archive/`:

```bash
git mv docs/plans/sound-uniqueness-recursive-summary-ownership.md docs/plans/archive/
# edit docs/plans/README.md to delete this plan's row
git add docs/plans/README.md docs/plans/archive/sound-uniqueness-recursive-summary-ownership.md \
  docs/plans/sound-uniqueness/analysis/sieve-cfg-gap-notes.md docs/plans/sound-uniqueness/analysis/README.md
git commit -m "docs: close recursive-summary ownership gap (graph_scc.visit)"
```

- [ ] **Step 4: Confirm no stray artifacts**

```bash
git status --short
find . -path './.git' -prune -o -name '*.cfg' -print
ls /tmp/twinkle-cfg-recur 2>/dev/null
```

Expected: no `.cfg` dumps inside the repo; temporary dumps only under `/tmp/twinkle-cfg-recur`.

Final report should state:
- whether `graph_scc.visit` (and the minimal/mutual fixtures) render `reuse(unique)`, or whether the plan landed analysis-only with Stage 6 deferred;
- that parameter/alias negatives stayed conservative;
- that the **generic** summary table and census baseline are unchanged (or census dropped via a real codegen consumer);
- boot suite, self-host, and lint outcomes;
- any residual: mutual-recursion depth limits, dict-valued state fields in `visit`, and the Stage 6 codegen handoff if still open.
```
