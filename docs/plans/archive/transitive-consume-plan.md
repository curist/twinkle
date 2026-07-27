# Transitive-Consume Delegation Implementation Plan

> **STATUS (2026-07-27): LANDED, across two plans.** The **delegated-consume** and
> **mixed local-update+delegation** shapes (Tasks 1–5 here) landed: the resolver spine,
> variant resolvers, render pipeline, and delegated-consume candidacy compose
> `build → resolve → … → add` end to end. The **transport-wrapper** shape (this plan's
> Task 6/7 transport portions) is delivered separately by
> `transport-wrapper-role-recovery-plan.md` (candidacy consumption gate +
> `collect_move_recovered_params` → `cap=Consumed`), including the Phase 6 boundary
> decision recorded in `transport-wrapper-phase6-conflict-brief.md`. The Task 6–9 steps
> below are superseded for the transport shape; kept for history.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let owned in-place threading compose through delegating call hops of arbitrary depth, so chains such as `build → resolve → resolve_decls → resolve_one → add` reach the leaf in-place update instead of collapsing to persistent at the first forwarding function.

**Architecture:** This is an analysis-precision change, not a codegen change. The implementation adds a per-call-site variant resolver to the ownership forward analysis, exposes already-validated variants during later SCC validation, and broadens variant candidacy to recognize delegation shapes. The resolver is injected into `ownership.tw`; `ownership.tw` must not import `summary.tw`, preserving the current module direction.

**Tech Stack:** Boot compiler (`boot/compiler/summary.tw`, `boot/compiler/ownership.tw`), Twinkle CFG fixtures (`boot/tests/fixtures/cfg/sound_uniqueness/*.tw`), fixture suite (`boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`), CFG render harness (`target/twk ir <file> --cfg`), boot suite (`target/twk run boot/tests/main.tw`), self-host (`make stage2`), census (`target/twk ir boot/main.tw --census --sites`).

## Global Constraints

- Treat `boot/` as the primary implementation. Do not update Rust stage0 unless boot no longer builds.
- Preserve module direction: `summary.tw` may import `ownership.tw`; `ownership.tw` must not import `summary.tw`.
- Selection must be **per call site**, gated by `arg_unique`, with generic fallback. Do not globally replace a callee's generic summary with a variant summary.
- The change is analysis/render-only. Do not implement variant cloning/dispatch or emitted-code specialization here.
- After editing `.tw` files, run `target/twk fmt <file>` and `target/twk lint <entry-or-file>` as appropriate.
- Run heavy builds one at a time. Use `make quick-bundle-cli` only when `target/boot.wasm` is known fresh; otherwise use `make bundle-cli`.
- Keep RED/NEG fixture assertions semantic: match `variant fn`, `verdict -> f`, and `reuse(unique)`, not hard-coded numeric `FuncId`s.

---

## Quick orientation for a fresh worker

### What problem is being fixed?

The current analysis can prove a direct call to a leaf consumer owns the threaded shell, but it loses that proof through pure forwarders:

```tw
fn add(env: Env, k: String, v: Int) Env {
  env.types = .set(k, v) // leaf update
  env
}

fn resolve_one(env: Env, k: String, v: Int) Env {
  env = add(env, k, v) // pure delegator
  env
}
```

`add` gets an owned variant because it contains an update site. `resolve_one` and later forwarders do not currently become candidates, and their generic summaries publish `env` because generic analysis cannot prove the param is unique + last-use at the delegated call.

The same issue appears in transport-wrapper form:

```tw
fn synth(ctx: Ctx, name: String) Out {
  ctx.syms = .set(name, ctx.count)
  Out.{ ctx, ty: ctx.count }
}

fn check(ctx: Ctx, a: String, b: String) Ctx {
  o1 := synth(ctx, a)
  ctx = o1.ctx // ownership arrives through ret_paths=.f0=from(p0)
  o2 := synth(ctx, b)
  ctx = o2.ctx
  ctx
}
```

Here `synth` returns a fresh wrapper, with the threaded `ctx` recoverable from a return field (`ret_paths=.f0=from(p0)`). The candidacy scan currently does not track this path, so `check` is not proposed as an owned variant.

### Current code map

Use these locations as the starting points; line numbers may drift, so search by function name.

| Area | File/function | Why it matters |
|---|---|---|
| Summary data types | `boot/compiler/ownership.tw`: `ParamSummary`, `ReturnEffect`, `ReturnPathOwn`, `Summary`, `SummaryTable`, `summary_get` | `summary.tw` imports these types from `ownership.tw`; keep it that way. |
| User-call transfer | `boot/compiler/ownership.tw`: `transfer_call`, `transfer_summarized_call` | A direct user callee currently reads only `table.summary_get(fid.id)` and transfers that summary. Variant-aware selection hooks here. |
| Whole-return move guard | `boot/compiler/ownership.tw`: `transfer_summarized_call`, `arg_unique`, `MayAliasParams` arm | The soundness guard for delegated-consume: only a single returned param with a unique + last-use argument is moved. |
| Return-path recovery guard | `boot/compiler/ownership.tw`: `transfer_summarized_call`, `ret_paths` loop | The soundness guard for transport-wrapper: `OwnedFromParam(k)` recovery only applies when `arg_unique[k]` is true; otherwise it publishes conservatively. |
| Verdict rendering | `boot/compiler/ownership.tw`: `block_verdicts`, `render_call_decision`, `select_variant` | CFG output's `verdict -> f...` comes from here; transfer and verdict rendering must use the same selected summary. |
| Per-function analysis | `boot/compiler/ownership.tw`: `analyze_function`, `analyze_with_summaries*`, `analyze_function_with_seed`, `summarize_variant`, `summarize_seeded` | Resolver threading must reach these paths. |
| Variant table | `boot/compiler/summary.tw`: `VariantSummaryTable`, `VariantEntry`, `vtable_put`, `variant_get`, `variant_args_satisfied` | `summary.tw` owns variant table details and builds resolver closures. |
| Candidate scan | `boot/compiler/summary.tw`: `scan_inplace_op`, `param_has_inplace_site`, `candidate_variants` | Broaden this from local update sites to delegated-consume and transport-wrapper sites. |
| Variant validation | `boot/compiler/summary.tw`: `run_scc_variants`, `build_overlay`, `compute_variants` | Later SCCs must see variants already validated in earlier SCCs. |
| Render pipeline | `boot/commands/ir.tw`: `render_cfg_artifacts`; `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`: `render_entry` | These must analyze rendered CFGs with the variant resolver after `compute_variants`. |
| Codegen artifacts | `boot/compiler/codegen/ownership_verdicts.tw`: `compute_artifacts` | Current production artifacts analyze generically before variants. This plan changes CFG rendering/analysis diagnostics; do not assume emitted code dispatch exists. |

### Existing RED fixtures

The currently uncommitted RED fixtures should already exist before implementation:

- `boot/tests/fixtures/cfg/sound_uniqueness/red_delegate_chain.tw`
- `boot/tests/fixtures/cfg/sound_uniqueness/red_transport_wrapper_chain.tw`
- `boot/tests/fixtures/cfg/sound_uniqueness/red_mixed_delegate_update.tw`

Their current locks live in `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`. They intentionally assert conservative behavior. After implementation, flip them to owned assertions.

### Soundness invariant

A variant may be proposed optimistically, but it only survives if `run_scc_variants` re-analyzes the function body and `variant_valid` still sees:

- a non-empty in-place path for the candidate param; and
- a return that aliases exactly that param.

For delegated-consume (`ret=alias(pK)`), `transfer_summarized_call` performs the whole-return move only when `arg_unique[K]` is true. `arg_unique` includes Unique ownership, binding validity, last-use, and single occurrence among call args.

For transport-wrapper (`ret=fresh ret_paths=.fN=from(pK)`), return-path recovery similarly gates `OwnedFromParam(K)` on `arg_unique[K]`. If the caller reads the original param after the delegated call, the argument is not last-use; recovery fails and the candidate must retract.

This plan adds negative fixtures for both shapes. These locks are not optional: they prove candidacy broadening did not weaken acceptance guards.

### Implementation strategy in one sentence

Add `ownership.VariantResolver = fn(Int, Vector<Bool>) Summary?`, pass it through every ownership analysis path that transfers or renders user calls, have `summary.tw` build resolver closures from `VariantSummaryTable`, and broaden candidacy so validated variants can be selected through forwarding and wrapper hops.

---

## Task 1: Baseline and snapshots

**Files:** none.

**Interfaces:** none.

- [ ] **Step 1: Confirm current RED locks pass**

Run:
```bash
target/twk run boot/tests/main.tw 2>&1 | tail -1
```

Expected: `Ran <N> tests: <N> passed`.

- [ ] **Step 2: Snapshot current RED fixture renders**

Run:
```bash
for f in red_delegate_chain red_transport_wrapper_chain red_mixed_delegate_update; do
  target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/$f.tw --cfg > /tmp/before_$f.cfg 2>&1
done
```

Expected:
- `/tmp/before_red_delegate_chain.cfg` contains no `variant fn ` and no `verdict ->`.
- `/tmp/before_red_transport_wrapper_chain.cfg` contains `ret_paths=.f0=from(p0)`, but no `variant fn check` and no `verdict ->`.
- `/tmp/before_red_mixed_delegate_update.cfg` contains `variant fn outer [unique:p0]` with a downstream `verdict -> f`, but `fn build [` has no `verdict ->`.

- [ ] **Step 3: Keep these snapshots for Task 3 only**

Do not use the snapshots after Task 4; from Task 4 onward renders are expected to change.

---

## Task 2: Add negative soundness fixtures first

**Files:**
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/red_delegate_read_after.tw`
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/red_transport_read_after.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: existing `render_entry`, `section_from`, `assert` helpers in `cfg_sound_uniqueness_fixtures_suite.tw`.
- Produces: NEG locks that must remain green through Tasks 3-8.

### Task 2A: delegated-consume read-after-delegate NEG

- [ ] **Step 1: Write `red_delegate_read_after.tw`**

Create `boot/tests/fixtures/cfg/sound_uniqueness/red_delegate_read_after.tw`:

```tw
// NEGATIVE soundness fixture for delegated-consume.
// `tap` has the same return shape as a forwarding candidate: it returns the
// delegated Env result. But it reads the ORIGINAL `env` after delegating, so
// `env` is not last-use at the `add` call. A seeded owned variant for `tap`
// must therefore retract: no `variant fn tap`, no `verdict ->`, no reuse.
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

- [ ] **Step 2: Format it**

Run:
```bash
target/twk fmt boot/tests/fixtures/cfg/sound_uniqueness/red_delegate_read_after.tw
```

Expected: formatter succeeds.

### Task 2B: transport-wrapper read-after-delegate NEG

- [ ] **Step 3: Write `red_transport_read_after.tw`**

Create `boot/tests/fixtures/cfg/sound_uniqueness/red_transport_read_after.tw`:

```tw
// NEGATIVE soundness fixture for transport-wrapper delegation.
// `check` returns ctx through a fresh wrapper projection shape, but reads the
// ORIGINAL `ctx` after calling `synth`. That makes ctx non-last-use at the
// wrapper call, so ret_paths OwnedFromParam recovery must fail and the candidate
// must retract.
type Ctx = .{ syms: Dict<String, Int>, count: Int }

type Out = .{ ctx: Ctx, ty: Int }

fn synth(ctx: Ctx, name: String) Out {
  ctx.syms = .set(name, ctx.count)
  ctx.count = ctx.count + 1
  Out.{ ctx, ty: ctx.count }
}

fn check(ctx: Ctx, name: String) Ctx {
  out := synth(ctx, name)
  println(ctx.count.to_string())
  out.ctx
}

fn build() Ctx {
  ctx := Ctx.{ syms: Dict.new(), count: 0 }
  check(ctx, "x")
}

println(build().count.to_string())
```

- [ ] **Step 4: Format it**

Run:
```bash
target/twk fmt boot/tests/fixtures/cfg/sound_uniqueness/red_transport_read_after.tw
```

Expected: formatter succeeds.

### Task 2C: lock both negatives in the suite

- [ ] **Step 5: Add suite tests before the closing `}`**

Append these tests to `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw` near the existing RED locks:

```tw
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
    .test(
      "NEG: transport wrapper then original read stays persistent",
      fn() {
        out := try render_entry("red_transport_read_after")
        check := try section_from(out, "fn check ")
        try assert.str_contains(out, "ret_paths=.f0=from(p0)")
        try assert.is_false(check.contains("verdict ->"))
        try assert.is_false(out.contains("variant fn check"))
        try assert.is_false(check.contains("reuse(unique)"))
        .Ok({})
      },
    )
```

- [ ] **Step 6: Format and run the suite**

Run:
```bash
target/twk fmt boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
target/twk run boot/tests/main.tw 2>&1 | tail -1
```

Expected: all tests pass. These NEG locks may be vacuously green until candidacy broadening, but they become load-bearing after Tasks 5 and 6.

---

## Task 3: Inject a variant resolver into ownership analysis without changing behavior

**Files:**
- Modify: `boot/compiler/ownership.tw`

**Interfaces:**
- Produces: `pub type VariantResolver = fn(Int, Vector<Bool>) Summary?`
- Produces: `pub fn generic_only_resolver(_: Int, _: Vector<Bool>) Summary?`
- Produces: resolver-aware entry points with distinct names; existing public entry points remain generic wrappers.
- Must not mention `VariantSummaryTable`, `VariantEntry`, or any `summary.tw` type in `ownership.tw`.

### Task 3A: define resolver and arg-unique helper

- [ ] **Step 1: Add resolver type near `SummaryTable`**

In `boot/compiler/ownership.tw`, near the `Summary`/`SummaryTable` definitions, add:

```tw
pub type VariantResolver = fn(Int, Vector<Bool>) Summary?

pub fn generic_only_resolver(_: Int, _: Vector<Bool>) Summary? {
  .None
}
```

- [ ] **Step 2: Factor shared call-site uniqueness helper**

Near `transfer_summarized_call`, add:

```tw
fn call_arg_unique(st: ForwardState, args: Vector<Atom>, last: Vector<Int>) Vector<Bool> {
  collect a in args {
    case atom_local_id(a) {
      .Some(id) => local_reusable(st.own, st.valid, id, last) and store_count(args, id) == 1,
      .None => false,
    }
  }
}
```

Then replace the local `arg_unique` construction inside `transfer_summarized_call` with:

```tw
arg_unique := call_arg_unique(ForwardState.{ own: pre_own, valid: pre_valid, prov: pre_prov, field_own: st.field_own, path_prov: st.path_prov }, args, last)
```

If constructing a temporary state is awkward, make the helper accept the `own` and `valid` maps directly instead. The direct-map form should return the same `collect a in args { ... }` result shown above, reading ownership from the supplied maps rather than from a `ForwardState`.

Use one helper in both `transfer_call` and `transfer_summarized_call`; do not duplicate the logic.

### Task 3B: thread resolver through transfer and analysis

- [ ] **Step 3: Add `resolve: VariantResolver` to `transfer_call`**

In the direct user-callee branch, select the chosen summary per call site:

```tw
.None => {
  au := call_arg_unique(st, args, last)
  chosen := case resolve(fid.id, au) {
    .Some(vs) => vs,
    .None => case summary_get(table, fid.id) {
      .Some(s) => s,
      .None => return st.publish_call(result, args, cc_suppress),
    },
  }
  st.transfer_summarized_call(result, chosen, args, last, suppress, fid.id)
}
```

Use `summary_get(table, fid.id)` rather than `table.summary_get(fid.id)` if that is the local style in the edited block.

- [ ] **Step 4: Thread resolver to every `transfer_op` path**

Add `resolve` parameters to these functions and pass them through to `transfer_call`:

- `transfer_op`
- `forward_block_body`
- `forward_block`
- `run_fixpoint`
- `run_fixpoint_validated`
- `summarize_seeded`
- `summarize_function` / `summarize_function_cached` wrappers
- `analyze_function`
- `analyze_function_with_seed`
- `call_uniques` only if it needs transfer effects from selected summaries for later reachability; otherwise keep it generic and document that decision in code.

Important: if `call_uniques` remains generic, final variant reachability may still miss downstream selections. The safest implementation is to add a resolver-aware `call_uniques_resolved(...)` and have `summary.reachable_variants` use it when scanning reachable variant bodies.

- [ ] **Step 5: Add distinct resolver-aware public entry points**

Twinkle has no function overloading. Add new names instead of trying to overload existing functions. The resolver-aware entry points and their implementations should mirror the existing generic entry points, with only the additional `resolve` parameter threaded through:

- `pub fn analyze_with_summaries_resolved(view: CfgView, b: BuiltinRegistry, sem: OptimizerSemantics, table: SummaryTable, resolve: VariantResolver) CfgView`
  - Body: the current `analyze_with_summaries` body, but call `analyze_with_summaries_and_entry_seeds_resolved(view, b, sem, table, Dict.new(), resolve)`.
- `pub fn analyze_with_summaries_and_entry_seeds_resolved(view: CfgView, b: BuiltinRegistry, sem: OptimizerSemantics, table: SummaryTable, entry_seeds: Dict<Int, Dict<Int, Bool>>, resolve: VariantResolver) CfgView`
  - Body: the current `analyze_with_summaries_and_entry_seeds` body, but call `analyze_function(..., resolve)` for each function.
- `pub fn analyze_function_with_seed_resolved(f: CfgFunction, table: SummaryTable, b: BuiltinRegistry, sem: OptimizerSemantics, unique_seed: Dict<Int, Bool>, resolve: VariantResolver) CfgFunction`
  - Body: the current `analyze_function_with_seed` body, but pass `resolve` into `analyze_function`.
- `pub fn summarize_variant_resolved(f: CfgFunction, key: vid.UniqueKey, table: SummaryTable, b: BuiltinRegistry, sem: OptimizerSemantics, suppress: Dict<Int, Bool>, resolve: VariantResolver) Summary`
  - Body: the current `summarize_variant` body, building `unique_seed` from `key`, then calling resolver-aware `summarize_seeded`.

Keep existing public functions as wrappers using `generic_only_resolver` so current callers remain source-compatible.

### Task 3C: make verdict rendering resolver-aware

- [ ] **Step 6: Thread resolver into `block_verdicts`**

`block_verdicts` renders `verdict -> f...` by computing `au` and calling `select_variant(fid.id, s, au)` from the generic summary. Add `resolve: VariantResolver` to `block_verdicts`; for user callees, choose:

```tw
case summary_get(table, fid.id) {
  .Some(base) => {
    au := call_arg_unique(pre, args, last)
    selected := case resolve(fid.id, au) {
      .Some(vs) => vs,
      .None => base,
    }
    dec := render_call_decision(fid.id, select_variant(fid.id, selected, au))
    if dec.len() > 0 {
      verdicts[inst.anf_local.id] = dec
    }
  },
  .None => {},
}
```

The analysis transfer and rendered call verdict must consult the same resolver; otherwise CFG output can show stale generic decisions.

- [ ] **Step 7: Rebuild and verify unchanged behavior**

Run:
```bash
make quick-bundle-cli 2>&1 | tail -2
target/twk run boot/tests/main.tw 2>&1 | tail -1
for f in red_delegate_chain red_transport_wrapper_chain red_mixed_delegate_update; do
  diff <(target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/$f.tw --cfg 2>&1) /tmp/before_$f.cfg \
    && echo "$f: unchanged"
done
```

Expected: all pass; the three original RED fixture renders are byte-identical to the snapshots because every path still uses `generic_only_resolver`.

---

## Task 4: Build variant resolvers in `summary.tw` and use them during validation/rendering

**Files:**
- Modify: `boot/compiler/summary.tw`
- Modify: `boot/commands/ir.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: `ownership.VariantResolver`, `ownership.*_resolved(...)` APIs from Task 3.
- Produces: resolver helpers inside `summary.tw`.

### Task 4A: resolver helpers

- [ ] **Step 1: Implement `variant_summary_for`**

In `boot/compiler/summary.tw`, near `variant_args_satisfied`, add a helper with this behavior:

```tw
fn variant_summary_for(vtable: VariantSummaryTable, callee_id: Int, arg_unique: Vector<Bool>) Summary? {
  best_key := ""
  best_score := 0 - 1
  best: Summary? = .None
  case vtable.by_func.get(callee_id) {
    .Some(keys) => {
      for key in keys {
        case vtable.by_key.get(key) {
          .Some(entry) => if variant_args_satisfied(entry.variant, arg_unique) {
            score := variant_specificity(entry.variant)
            if score > best_score or (score == best_score and (best_key.len() == 0 or key < best_key)) {
              best_key = key
              best_score = score
              best = .Some(entry.summary)
            }
          },
          .None => {},
        }
      }
      best
    },
    .None => .None,
  }
}
```

Also add:

```tw
fn variant_specificity(v: vid.VariantId) Int {
  score := 0
  for req in v.unique {
    score = score + 1 + req.path.segs.len()
  }
  score
}
```

Use exact Twinkle syntax as required by the compiler/formatter.

- [ ] **Step 2: Implement public/internal resolver builders**

Add:

```tw
fn make_variant_resolver(vtable: VariantSummaryTable) ownership.VariantResolver {
  fn(callee_id: Int, arg_unique: Vector<Bool>) Summary? {
    variant_summary_for(vtable, callee_id, arg_unique)
  }
}

fn make_outscc_resolver(vtable: VariantSummaryTable, scc_set: Dict<Int, Bool>) ownership.VariantResolver {
  fn(callee_id: Int, arg_unique: Vector<Bool>) Summary? {
    if in_set(scc_set, callee_id) {
      .None
    } else {
      variant_summary_for(vtable, callee_id, arg_unique)
    }
  }
}
```

`in_set` already exists in `summary.tw` for integer sets; reuse it.

### Task 4B: validation-time resolver

- [ ] **Step 3: Use out-of-SCC resolver in `run_scc_variants`**

In `run_scc_variants`, keep `overlay := build_overlay(generic, member_iter)` for in-SCC Gauss-Seidel summaries. Change the call from generic `summarize_variant` to resolver-aware `summarize_variant_resolved`:

```tw
overlay := build_overlay(generic, member_iter)
resolve := make_outscc_resolver(vtable, scc_set)
next := ownership.summarize_variant_resolved(f, key, overlay, b, sem, scc_set, resolve)
```

Rationale: in-SCC calls continue to read the member-iterate overlay. Out-of-SCC calls can now select already-validated variants from earlier SCCs, which is what lets chains compose beyond one hop.

- [ ] **Step 4: Rebuild and check mixed fixture first**

Run:
```bash
make quick-bundle-cli 2>&1 | tail -2
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/red_mixed_delegate_update.tw --cfg 2>&1 \
  | grep -E "fn build|variant fn outer|verdict ->|reuse\(unique\)"
```

Expected: `fn build [` now contains a `verdict -> f...` selection for `outer`'s existing variant. `red_delegate_chain` may still be persistent until Task 5.

### Task 4C: render-time resolver

- [ ] **Step 5: Update CFG render pipeline**

Current `boot/commands/ir.tw:render_cfg_artifacts` computes generic `owned.analyzed` before variants and passes that to `summary.render_cfg`. Change the pipeline shape so the CFG render uses a resolver-aware analyzed view after variants are known:

```tw
owned := ownership_verdicts.compute_artifacts(artifacts.opt, b, s)
variants := summary.compute_variants(owned.view, b, s, owned.table)
resolver := summary.make_variant_resolver_for_render(variants)
analyzed := ownership.analyze_with_summaries_resolved(owned.view, b, s, owned.table, resolver)
summary.render_cfg(owned.view, analyzed, b, s, owned.table, variants, resolver)
```

Expose the resolver builder from `summary.tw` if needed, for example:

```tw
pub fn make_variant_resolver_for_render(vtable: VariantSummaryTable) ownership.VariantResolver {
  make_variant_resolver(vtable)
}
```

If `render_cfg` cannot accept first-class function values cleanly, it is also acceptable for `summary.render_cfg` to build the resolver internally from `variants`; the key requirement is that generic rendered sections and variant sections both analyze with variant-aware call selection.

- [ ] **Step 6: Update suite `render_entry` similarly**

In `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`, update `render_entry` to mirror `twk ir --cfg`:

```tw
table := summary.compute(view, b, sem)
variants := summary.compute_variants(view, b, sem, table)
resolver := summary.make_variant_resolver_for_render(variants)
analyzed := ownership.analyze_with_summaries_resolved(view, b, sem, table, resolver)
.Ok(summary.render_cfg(view, analyzed, b, sem, table, variants, resolver))
```

Keep `twk ir --cfg` and test rendering byte-equivalent in structure.

- [ ] **Step 7: Update variant sections**

`summary.variant_sections` currently calls `analyze_function_with_seed(...)` against a reachable overlay. Switch it to `analyze_function_with_seed_resolved(...)` with a resolver built from reachable variants, or keep the overlay only if you verify downstream `verdict ->` selections still render correctly in variant sections.

Preferred shape:

```tw
resolver := make_variant_resolver_for_render(variants)
analyzed := ownership.analyze_function_with_seed_resolved(f, generic, b, sem, seed, resolver)
```

If this makes unreachable variants visible in variant bodies, use a reachable-filtered resolver built from `reachable` instead of the whole table.

- [ ] **Step 8: Run focused validation**

Run:
```bash
target/twk run boot/tests/main.tw 2>&1 | tail -1
for f in red_mixed_delegate_update red_delegate_read_after red_transport_read_after; do
  echo "== $f =="
  target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/$f.tw --cfg 2>&1 \
    | grep -E "^fn |variant fn |verdict ->|reuse\(unique\)"
done
```

Expected:
- `red_mixed_delegate_update`: `fn build [` shows a `verdict -> f...` for `outer`.
- `red_delegate_read_after`: no `variant fn tap`, no `verdict ->` in `fn tap`, no `reuse(unique)` in `fn tap`.
- `red_transport_read_after`: no `variant fn check`, no `verdict ->` in `fn check`, no `reuse(unique)` in `fn check`.

- [ ] **Step 9: Leaf-validation probe (gate before Task 5)**

Task 5 stacks transitive composition on top of the assumption that a single-hop
leaf update variant still **validates and renders its own in-place body** through
the new resolver-aware render pipeline. Confirm that spine is intact *before*
broadening candidacy, so a Task 5 failure is unambiguously about composition, not
about Task 4 having broken reachable-variant rendering.

Use `red_mixed_delegate_update`, whose `outer` variant is reachable in the
baseline (`variant fn outer [unique:p0]` renders today). Run:
```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/red_mixed_delegate_update.tw --cfg 2>&1 \
  | awk '/variant fn outer \[unique:p0\]/{p=1} p{print} /^fn |^variant fn (check|synth|tap|resolve)/{if(!/outer/)p=0}'
```

Expected: the `variant fn outer [unique:p0]` section still renders after the
Task 4 pipeline rewiring, and its body contains `reuse(unique)` — the leaf update
executes in place inside the validated, reachable variant. If this section is
missing or shows no `reuse(unique)`, stop: Task 4's resolver-aware render broke
leaf-variant rendering, and Task 5 cannot be diagnosed on top of it.

---

## Task 5: Broaden candidacy for delegated-consume (`ret=alias(pK)`)

**Files:**
- Modify: `boot/compiler/summary.tw`

**Interfaces:**
- Consumes: generic `SummaryTable` and `summary_get`.
- Produces: pure delegators returning their delegated param as sole alias become candidates.

- [ ] **Step 1: Thread the generic summary table into the scan**

Change signatures:

```tw
fn scan_inplace_op(op: AnfOp, dst: Int, st: InplaceScan, generic: SummaryTable) InplaceScan
fn param_has_inplace_site(f: CfgFunction, k: Int, generic: SummaryTable) Bool
```

Update callers in `candidate_variants` accordingly.

- [ ] **Step 2: Add delegated-consume call case**

In `scan_inplace_op`, add an `.ACall` arm. Use `summary_get`, not `table_get`; globals without user summaries must be ignored, not trapped.

```tw
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
}
```

- [ ] **Step 3: Rebuild and inspect delegate chain**

Run:
```bash
make quick-bundle-cli 2>&1 | tail -2
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/red_delegate_chain.tw --cfg 2>&1 \
  | grep -E "variant fn resolve|verdict ->|reuse\(unique\)"
```

Expected: `resolve_one`, `resolve_decls`, and `resolve` earn reachable `[unique:p0]` variants; each variant selects the downstream owned variant; `build` selects `resolve`.

- [ ] **Step 4: Re-check delegated negative**

Run:
```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/red_delegate_read_after.tw --cfg 2>&1 \
  | grep -E "^fn tap|variant fn tap|verdict ->|reuse\(unique\)"
target/twk run boot/tests/main.tw 2>&1 | tail -1
```

Expected: no `variant fn tap`, no `verdict ->` in `tap`, no `reuse(unique)` in `tap`; suite passes.

---

## Task 6: Broaden candidacy for transport-wrapper (`ret_paths=.fN=from(pK)`)

**Files:**
- Modify: `boot/compiler/summary.tw`

**Interfaces:**
- Produces: a path-aware candidacy scan capable of following `call -> record_get -> assign/init` for fresh wrapper returns.

### Task 6A: extend scan state

- [ ] **Step 1: Add field-derived tracking to `InplaceScan`**

Current scan state tracks only whole-local derivedness. Extend it with a map from result local to return fields that carry derived ownership:

```tw
type InplaceScan = .{
  derived: Dict<Int, Bool>,
  derived_fields: Dict<Int, Dict<Int, Bool>>,
  found: Bool,
  changed: Bool,
}
```

Add helpers:

```tw
fn mark_derived_field(st: InplaceScan, id: Int, field: Int) InplaceScan
fn atom_has_derived_field(st: InplaceScan, a: Atom, field: Int) Bool
```

`mark_derived_field` should set `changed = true` when it adds a new field fact.

- [ ] **Step 2: Preserve existing whole-derived propagation**

Update `AAssign` and `AInit` arms so whole-derived values still mark their destination. If the source carries derived fields, copy those field facts to the destination too. This keeps wrapper temporaries stable through simple rebinding.

### Task 6B: recognize return-path fields and projections

- [ ] **Step 3: Mark call result fields from `ret_paths`**

In the `.ACall` arm, after delegated-consume handling, inspect the callee summary's `ret_paths`:

- only consider `rp.via == .Direct`;
- only consider `rp.field == .Some(field_id)`;
- only consider `rp.own == .OwnedFromParam(k)`;
- require `k < args.len()` and `atom_is_derived(st.derived, args[k])`.

For each match, call `mark_derived_field(st, dst, field_id)`. Do **not** set `found` yet just because the wrapper result carries a field. The opportunity becomes relevant when the function actually projects and threads that field.

- [ ] **Step 4: Propagate projected fields through `ARecordGet`**

Add an `.ARecordGet(base, field, _)` arm:

```tw
.ARecordGet(base, field, _) => if atom_has_derived_field(st, base, field.id) {
  st.found = true
  st.mark_derived(dst)
} else {
  st
}
```

This says: a param-derived owned value arrived through a wrapper field and is now being threaded as a local value, so the wrapper function is a candidate.

- [ ] **Step 5: Rebuild and inspect transport wrapper**

Run:
```bash
make quick-bundle-cli 2>&1 | tail -2
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/red_transport_wrapper_chain.tw --cfg 2>&1 \
  | grep -E "variant fn check|verdict ->|reuse\(unique\)"
```

Expected: `check` earns a reachable `[unique:p0]` variant, selects `synth`'s owned path, and `build` selects `check`.

- [ ] **Step 6: Re-check transport negative**

Run:
```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/red_transport_read_after.tw --cfg 2>&1 \
  | grep -E "^fn check|variant fn check|verdict ->|reuse\(unique\)"
target/twk run boot/tests/main.tw 2>&1 | tail -1
```

Expected: no `variant fn check`, no `verdict ->` in `check`, no `reuse(unique)` in `check`; suite passes.

---

## Task 7: Flip RED locks to owned assertions

**Files:**
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

**Interfaces:**
- Consumes: post-fix actual CFG render strings.
- Produces: former RED tests become regression locks for owned composition.

- [ ] **Step 1: Capture post-fix render snippets**

Run:
```bash
for f in red_delegate_chain red_transport_wrapper_chain red_mixed_delegate_update; do
  echo "== $f =="
  target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/$f.tw --cfg 2>&1 \
    | grep -E "^fn |variant fn |verdict ->|reuse\(unique\)|ret_paths=.f0=from\(p0\)"
done
```

Use this output to choose exact, stable assertion strings.

- [ ] **Step 2: Rewrite `red_delegate_chain` test**

Change title to:

```tw
"delegating call chain composes owned threading end to end"
```

Assert at least:

```tw
out := try render_entry("red_delegate_chain")
try assert.str_contains(out, "variant fn resolve_one [unique:p0]")
try assert.str_contains(out, "variant fn resolve_decls [unique:p0]")
try assert.str_contains(out, "variant fn resolve [unique:p0]")
try assert.str_contains(out, "verdict -> f")
try assert.str_contains(out, "reuse(unique)")
```

- [ ] **Step 3: Rewrite `red_transport_wrapper_chain` test**

Change title to:

```tw
"transport-wrapper chain composes owned threading through returned ctx"
```

Assert at least:

```tw
out := try render_entry("red_transport_wrapper_chain")
try assert.str_contains(out, "ret_paths=.f0=from(p0)")
try assert.str_contains(out, "variant fn check [unique:p0]")
try assert.str_contains(out, "verdict -> f")
try assert.str_contains(out, "reuse(unique)")
```

- [ ] **Step 4: Rewrite `red_mixed_delegate_update` test**

Change title to:

```tw
"mixed local update plus delegation composes across build boundary"
```

Assert at least:

```tw
out := try render_entry("red_mixed_delegate_update")
outer := try variant_section(out, "outer", "unique:p0")
try assert.str_contains(outer, "reuse(unique)")
build_sec := try section_between(out, "fn build [", "fn $init")
try assert.str_contains(build_sec, "verdict -> f")
```

- [ ] **Step 5: Keep NEG tests unchanged**

Do not weaken `red_delegate_read_after` or `red_transport_read_after` assertions.

- [ ] **Step 6: Format, lint, run**

Run:
```bash
target/twk fmt boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
target/twk lint boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
target/twk run boot/tests/main.tw 2>&1 | tail -1
```

Expected: formatter succeeds, lint reports no findings, and all tests pass.

---

## Task 8: Regression guard — census, self-host, full suites

**Files:** none.

**Interfaces:** none.

- [ ] **Step 1: Boot-side census must not regress**

Run:
```bash
target/twk ir boot/main.tw --census --sites 2>&1 | grep -E "dict_set|vector_set"
```

Expected: in-place counts hold or increase vs the "Boot-side census" table in `docs/plans/sound-uniqueness/analysis/worked-examples.md`. If counts drop, investigate before proceeding.

- [ ] **Step 2: Self-host fixed point**

Run:
```bash
make stage2 2>&1 | tail -5
```

Expected: stage2 builds and self-host verification passes. Run alone, not concurrently with other heavy builds.

- [ ] **Step 3: Full suites**

Run:
```bash
make test 2>&1 | tail -5
```

Expected: all pass.

---

## Task 9: Documentation and commit

**Files:**
- Modify: `docs/plans/sound-uniqueness/analysis/worked-examples.md`
- Modify: `docs/plans/sound-uniqueness/README.md`
- Modify: `docs/plans/transitive-consume-plan.md` if implementation discoveries changed the plan notes

**Interfaces:** none.

- [ ] **Step 1: Update worked examples**

In `docs/plans/sound-uniqueness/analysis/worked-examples.md`, update the "Boot-side census" note. Replace the old `run_fixpoint … transitively-published sub-class` caveat with the observed post-fix behavior:

- delegated consume chains now compose through forwarders;
- transport-wrapper `ret_paths=.fN=from(pK)` chains now compose when the original is last-use;
- read-after-delegate cases stay persistent by design;
- deep field-backing remains out of scope.

- [ ] **Step 2: Update sound-uniqueness README**

In `docs/plans/sound-uniqueness/README.md`, link to the moved plan as `../transitive-consume-plan.md` and narrow/remove any transitively-published caveat based on actual results.

- [ ] **Step 3: Commit**

Run:
```bash
git add boot/compiler/summary.tw boot/compiler/ownership.tw \
  boot/commands/ir.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/ \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw \
  docs/plans/transitive-consume-plan.md \
  docs/plans/sound-uniqueness/
git commit -m "analysis: compose owned threading through delegating call hops

Make callee-summary selection variant-aware at the forward-analysis call site
(per call site, gated by arg_unique, generic fallback) and make validated
variants visible during variant validation, so owned threading composes through
forwarder and transport-wrapper hops instead of collapsing to persistent at the
first delegation. Candidacy is broadened for delegated-consume and
transport-wrapper shapes. ownership.tw stays free of variant-table types via an
injected resolver. Read-after-delegate negative fixtures lock the soundness
boundary; census and self-host hold."
```

---

## Acceptance criteria

- `red_delegate_chain`, `red_transport_wrapper_chain`, and `red_mixed_delegate_update` render owned in-place threading end to end, and their suite tests assert owned facts by semantic tokens rather than numeric FuncIds.
- `red_delegate_read_after` remains persistent: no `variant fn tap`, no `verdict ->` in `tap`, no `reuse(unique)` in `tap`.
- `red_transport_read_after` remains persistent: no `variant fn check`, no `verdict ->` in `check`, no `reuse(unique)` in `check`.
- Variant-aware selection is per-call-site and `arg_unique`-gated, with generic fallback.
- Validated variants from earlier SCCs are visible during later SCC variant validation.
- In-SCC recursive/mutually recursive validation continues to use the existing member-iterate overlay path.
- `ownership.tw` does not import `summary.tw`; variant selection reaches ownership via `VariantResolver`.
- Boot-side census in-place counts do not regress.
- `make stage2` fixed point holds and `make test` passes.

## Out of scope

- Deep field-backing in-place (`field=persistent(insufficient deep ownership)`) remains a separate ceiling; this plan composes shell/collection threading only.
- Emitted codegen for newly-owned variants (variant cloning/dispatch, Phase 8G) is not part of this plan; CFG analysis/rendering changes do not imply generated-code specialization.
- A general trait/capability system or new language syntax is not part of this plan.

## Common pitfalls

- **Import cycle:** do not import `summary.tw` from `ownership.tw`; resolver callbacks are the boundary.
- **Global overlay misuse:** do not replace the generic table wholesale for all calls. A function can be called once with a unique last-use arg and elsewhere with a shared/live arg.
- **Missing render update:** transfer can select a variant while `block_verdicts` still renders generic decisions. Keep transfer and verdict rendering resolver-aware together.
- **Vacuous negative tests:** NEG fixtures must have candidate-shaped returns. Returning `Int` from a read-after-delegate test does not prove variant retraction.
- **Trap on missing summary:** candidate scan must use `summary_get`, not `table_get`, for callees found in arbitrary `AGlobalFunc` atoms.
- **In-SCC gating:** do not let the resolver return in-SCC member iterates without checking call-site requirements. The plan avoids this by using resolver only for out-of-SCC variants during validation.
