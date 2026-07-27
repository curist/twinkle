# Transport-wrapper Role Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Classify a param that is consumed by a callee and recovered through that callee's `ret_paths` wrapper projection as `Consumed`, so transport-wrapper delegation (`build → check → synth`) composes owned in-place threading instead of collapsing to persistent.

**Architecture:** Analysis/render-only. Gate the transport-wrapper candidacy on callee consumption, then add a small resolver-aware collection pass (`collect_move_recovered_params`) that mirrors the real `ARecordGet` move branch to find params recovered by a licensed move-projection whose projected shell provenance names the param; feed that set into `cap = .Consumed` in the summary derivation. `ownership.tw` stays free of `summary.tw` types.

**Tech Stack:** Boot compiler (`boot/compiler/summary.tw`, `boot/compiler/ownership.tw`), Twinkle CFG fixtures (`boot/tests/fixtures/cfg/sound_uniqueness/*.tw`), fixture suite (`boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`), `target/twk ir <file> --cfg`, `target/twk run boot/tests/main.tw`, `make bundle-cli`, `make stage2`.

**Design:** `docs/plans/transport-wrapper-role-recovery-design.md`. Blocker analysis: `docs/plans/sound-uniqueness/analysis/transport-wrapper-blocker.md`.

---

## Global constraints

- Treat `boot/` as the primary implementation. Do not touch Rust stage0 unless boot no longer builds.
- Module direction: `summary.tw` imports `ownership.tw`; never the reverse.
- After editing a `.tw` file, run `target/twk fmt <file>` then `target/twk lint <file>` (apply `--fix` for inherent-call findings, then re-fmt).
- `twk ir --cfg` runs the **compiled binary's** analysis: boot-source analysis edits require `make bundle-cli` before `twk ir` reflects them. For fast inner-loop checks that must run new source without a full rebuild, use a scratch driver run via `target/twk run boot/scratch_*.tw` (compiles current source with the existing binary). Delete scratch files before committing.
- Run heavy builds one at a time (never concurrently): `make bundle-cli`, `make stage2`, `make test`.
- Keep RED/NEG assertions semantic (`variant fn`, `verdict -> f`, `reuse(unique)`), not numeric FuncIds, except where the plan asks for a resolved `f<id>`.

## Starting state

`boot/compiler/summary.tw` has **uncommitted** transport-wrapper candidacy from the parent plan's Task 6:
- `InplaceScan` extended with `derived_fields`, helpers `mark_derived_field` / `atom_has_derived_field` / `copy_derived_fields`;
- `scan_inplace_op` has an `.ACall` arm (delegated-consume + a `for rp in cs.ret_paths { st = .mark_ret_path_field(rp, args, dst) }` loop) and an `.ARecordGet` projection arm;
- `mark_ret_path_field(st, rp, args, dst)` marks a call result's field derived from `OwnedFromParam(k)`;
- `scan_param_threading` replaces `param_has_inplace_site` and returns `.{ found, ret_derived }`;
- `candidate_variants` gates on `scan.found and (ret_aliases_exactly_param(...) or scan.ret_derived)`.

This proposes `check`/`thread` as candidates but they retract today (role stays `Borrowed`). This plan gates that candidacy on callee consumption and adds the role recovery that makes them validate.

Verify the starting state before beginning:

```bash
git -C /Users/curist/playground/rust/twinkle status --short   # expect: only  M boot/compiler/summary.tw
```

---

## Task 1: Gate transport-wrapper candidacy on callee consumption

**Files:**
- Modify: `boot/compiler/summary.tw` (`mark_ret_path_field` and its `.ACall`-arm caller)

The candidacy `ret_paths` marking currently fires for any `OwnedFromParam(k)` return path. Gate it on the callee actually consuming param `k` (`cs.params[k].base_role == .Consumed`), so the linkage means "recovered from a consuming callee," never a bare field projection. This is the robustness gate from the design's soundness section.

- [ ] **Step 1: Pass the callee param summaries into `mark_ret_path_field`**

In `boot/compiler/summary.tw`, change the signature and body of `mark_ret_path_field` to accept the callee's `params` and gate on `.Consumed`:

```tw
fn mark_ret_path_field(
  st: InplaceScan,
  rp: ReturnPathOwn,
  args: Vector<Atom>,
  dst: Int,
  callee_params: Vector<ParamSummary>,
) InplaceScan {
  case rp.via {
    .Direct => case rp.field {
      .Some(fid) => case rp.own {
        .OwnedFromParam(k) => if k >= 0 and k < args.len() and k < callee_params.len()
          and atom_is_derived(st.derived, args[k]) and param_is_consumed(callee_params[k]) {
          st.mark_derived_field(dst, fid)
        } else {
          st
        },
        _ => st,
      },
      .None => st,
    },
    _ => st,
  }
}

fn param_is_consumed(ps: ParamSummary) Bool {
  case ps.base_role {
    .Consumed => true,
    _ => false,
  }
}
```

`ParamSummary` is already imported from `ownership` in `summary.tw`. `ParamRole` is too (used by `render_role`), so the `case ps.base_role` compiles.

- [ ] **Step 2: Update the caller to pass `cs.params`**

In `scan_inplace_op`'s `.ACall` arm, the `ret_paths` loop currently reads:

```tw
          // Transport wrapper: mark any return field that recovers a derived param.
          for rp in cs.ret_paths {
            st = .mark_ret_path_field(rp, args, dst)
          }
```

Change it to pass `cs.params`:

```tw
          // Transport wrapper: mark a return field only when the callee CONSUMES the
          // ret-path source param (so the linkage is a consuming recovery, not a bare
          // field projection).
          for rp in cs.ret_paths {
            st = .mark_ret_path_field(rp, args, dst, cs.params)
          }
```

- [ ] **Step 3: Format, lint, typecheck**

```bash
cd /Users/curist/playground/rust/twinkle
target/twk fmt boot/compiler/summary.tw
target/twk lint boot/compiler/summary.tw   # apply --fix if it reports inherent-call findings, then re-fmt
target/twk build boot/main.tw -o /tmp/check.wasm 2>&1 | tail -3
```

Expected: `WASM output: /tmp/check.wasm` (compiles clean).

- [ ] **Step 4: Confirm the gate still proposes the consuming wrapper (non-vacuous)**

Write a scratch driver that runs the current source and shows `check` is still a candidate under the gate (peek/thread and synth/check callees are `Consumed`, so the gate passes). Create `/tmp/probe_gate.tw`:

```tw
type Ctx = .{ syms: Dict<String, Int>, count: Int }
type Out = .{ ctx: Ctx, ty: Int }
fn synth(ctx: Ctx, name: String) Out {
  ctx.syms = .set(name, ctx.count)
  ctx.count = ctx.count + 1
  Out.{ ctx, ty: ctx.count }
}
fn check(ctx: Ctx, a: String, b: String) Ctx {
  o1 := synth(ctx, a)
  ctx = o1.ctx
  o2 := synth(ctx, b)
  ctx = o2.ctx
  ctx
}
fn build() Ctx {
  ctx := Ctx.{ syms: Dict.new(), count: 0 }
  check(ctx, "x", "y")
}
println(build().count.to_string())
```

Create `boot/scratch_gate.tw`:

```tw
use commands.common.{format_compile_error}
use compiler.cfg
use compiler.opt.semantics as semantics
use compiler.ownership
use compiler.pipeline
use compiler.summary

fn main() {
  artifacts := case pipeline.compile_entry_path("/tmp/probe_gate.tw") {
    .Ok(r) => r,
    .Err(e) => {
      println(format_compile_error(e))
      return
    },
  }
  b := artifacts.builtins
  view := cfg.build_view(artifacts.opt, b)
  view = ownership.prune_dead_merge(view)
  sem := semantics.make_prelude_optimizer_semantics(b)
  table := summary.compute(view, b, sem)
  variants := summary.compute_variants(view, b, sem, table)
  println("variant keys: ${variants.by_key.keys().len()}")
}

main()
```

Run:

```bash
target/twk run boot/scratch_gate.tw 2>&1 | tail -3
rm -f boot/scratch_gate.tw
```

Expected: it prints a `variant keys:` line and does not error. (`check` still enters validation; it retracts, so its key may be absent — that is fine, this step only confirms the gate compiles and the pipeline runs. Role recovery in Task 3 makes the key appear.)

- [ ] **Step 5: Regression-verify candidacy base and commit**

Run the boot suite to confirm the candidacy + gate change nothing observable yet:

```bash
target/twk run boot/tests/main.tw 2>&1 | tail -1
```

Expected: `Ran <N> tests: <N> passed`.

Then rebuild and re-run so `twk ir` reflects the source, and confirm the transport fixtures are still persistent (no accidental composition, no regressions):

```bash
make bundle-cli 2>&1 | tail -2
target/twk run boot/tests/main.tw 2>&1 | tail -1
for f in red_transport_wrapper_chain red_transport_read_after red_delegate_chain red_mixed_delegate_update; do
  echo -n "$f: "; target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/$f.tw --cfg 2>&1 | grep -cE "variant fn check|variant fn thread"
done
```

Expected: boot suite passes; `red_transport_wrapper_chain` and `red_transport_read_after` print `0` (still persistent); the delegate/mixed fixtures are unaffected.

Commit:

```bash
git add boot/compiler/summary.tw
git commit -m "analysis: transport-wrapper candidacy + callee-consumption gate

Broaden candidacy to the transport-wrapper shape (ret_paths field marking +
ARecordGet projection + ret_derived return gate), and gate the ret-path field
marking on the callee consuming the source param (cs.params[k].base_role ==
.Consumed) so the linkage is a consuming recovery, not a bare field projection.
Candidates are proposed but still retract (role recovery follows); transport
fixtures stay persistent."
```

---

## Task 2: Bounded section helper; fix the read-after NEG lock

**Files:**
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`

`section_from` runs to end-of-output, so using it to bound `check`'s section for a negative assertion also matches later sections. Add a bounded helper and switch the read-after NEG lock to it. Behavior-preserving (NEG stays green).

- [ ] **Step 1: Add a bounded `function_section` helper**

In `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`, near `section_from`, add:

```tw
// Bounded slice of one function's section: from "fn <name> [" up to the next
// "\nfn " (or EOF). Unlike section_from, it does not run past the function.
fn function_section(out: String, name: String) Result<String, String> {
  start_marker := "fn ${name} ["
  start := try out.index_of(start_marker).ok_or("missing function ${name}")
  tail := out.substring(start, out.len())
  rest := tail.substring(1, tail.len())
  next := case rest.index_of("\nfn ") {
    .Some(i) => i + 1,
    .None => tail.len(),
  }
  .Ok(tail.substring(0, next))
}
```

- [ ] **Step 2: Switch the transport read-after NEG lock to the bounded helper**

Replace the existing NEG test body:

```tw
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

with:

```tw
    .test(
      "NEG: transport wrapper then original read stays persistent",
      fn() {
        out := try render_entry("red_transport_read_after")
        check := try function_section(out, "check")
        try assert.str_contains(out, "ret_paths=.f0=from(p0)")
        try assert.is_false(check.contains("verdict ->"))
        try assert.is_false(out.contains("variant fn check"))
        try assert.is_false(check.contains("reuse(unique)"))
        .Ok({})
      },
    )
```

- [ ] **Step 3: Format, run, commit**

```bash
target/twk fmt boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
target/twk run boot/tests/main.tw 2>&1 | tail -1
```

Expected: all tests pass.

```bash
git add boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git commit -m "test: bound the transport read-after NEG to one function section

section_from runs to EOF, so it cannot bound a single function's section for a
negative check. Add function_section (bounded by the next \"\\nfn \") and use it
for the read-after NEG lock."
```

---

## Task 3: Collect `move_recovered_params` and feed into `cap` (the core fix)

**Files:**
- Modify: `boot/compiler/ownership.tw` (`summarize_seeded`; add `collect_move_recovered_params`)
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw` (flip the transport lock, add single-hop positive)
- Create: `boot/tests/fixtures/cfg/sound_uniqueness/transport_wrapper_single.tw`

### Task 3A: Write the failing composition tests first

- [ ] **Step 1: Add a single-hop positive fixture**

Create `boot/tests/fixtures/cfg/sound_uniqueness/transport_wrapper_single.tw`:

```tw
// Single-hop transport wrapper. `wrap` consumes ctx (stores it into a fresh Out)
// and returns it via ret_paths=.f0=from(p0); `thread` projects it back and returns
// it. DESIRED: thread earns [unique:p0], build selects it.
type Ctx = .{ syms: Dict<String, Int>, count: Int }

type Out = .{ ctx: Ctx, ty: Int }

fn wrap(ctx: Ctx, name: String) Out {
  ctx.syms = .set(name, ctx.count)
  Out.{ ctx, ty: ctx.count }
}

fn thread(ctx: Ctx, name: String) Ctx {
  o := wrap(ctx, name)
  ctx = o.ctx
  ctx
}

fn build() Ctx {
  ctx := Ctx.{ syms: Dict.new(), count: 0 }
  thread(ctx, "a")
}

println(build().count.to_string())
```

- [ ] **Step 2: Format the fixture**

```bash
target/twk fmt boot/tests/fixtures/cfg/sound_uniqueness/transport_wrapper_single.tw
```

- [ ] **Step 3: Flip the transport RED lock and add the single-hop positive**

In `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`, replace the existing RED lock:

```tw
    .test(
      "RED: transport-wrapper chain keeps fresh-field move facts but drops ownership",
      fn() {
        out := try render_entry("red_transport_wrapper_chain")

        // The wrapper's return-path summary is correct (Phase 5 machinery works).
        try assert.str_contains(out, "ret_paths=.f0=from(p0)")

        // RED: `check` still summarizes the threaded ctx as Borrowed/shared, so
        // `build` never threads it as owned.
        try assert.str_contains(out, "p0=Borrowed p1=Published p2=Published ret=shared")
        try assert.is_false(out.contains("verdict ->"))
        try assert.is_false(out.contains("variant fn "))
        .Ok({})
      },
    )
```

with two tests (multi-hop flip + single-hop positive):

```tw
    .test(
      "transport-wrapper chain composes owned threading through returned ctx",
      fn() {
        out := try render_entry("red_transport_wrapper_chain")
        try assert.str_contains(out, "ret_paths=.f0=from(p0)")

        // check earns an owned variant: p0 recovered to Consumed, and it selects
        // synth's owned behavior across the two hops. NOTE: `reuse(unique)` is NOT
        // a signal here — check does no record_update (it recovers ctx via
        // projection); the leaf synth's generic summary is already Consumed and it
        // earns no variant (its return is a fresh wrapper, not ret_derived). The
        // success signals are the recovered role + the build-boundary selection.
        variant := try variant_section(out, "check", "unique:p0")
        try assert.str_contains(variant, "p0=Consumed")
        try assert.str_contains(variant, "verdict -> f")

        // build selects check's owned [unique:p0] variant across the boundary.
        build_sec := try section_between(out, "fn build [", "fn $init")
        try assert.str_contains(build_sec, "verdict -> f")
        try assert.str_contains(build_sec, "[unique:p0")
        .Ok({})
      },
    )
    .test(
      "single-hop transport wrapper composes owned threading",
      fn() {
        out := try render_entry("transport_wrapper_single")
        variant := try variant_section(out, "thread", "unique:p0")
        try assert.str_contains(variant, "p0=Consumed")
        try assert.str_contains(variant, "verdict -> f")
        build_sec := try section_between(out, "fn build [", "fn $init")
        try assert.str_contains(build_sec, "verdict -> f")
        try assert.str_contains(build_sec, "[unique:p0")
        .Ok({})
      },
    )
```

- [ ] **Step 4: Run to confirm the new tests FAIL (red)**

```bash
target/twk fmt boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
target/twk run boot/tests/main.tw 2>&1 | tail -4
```

Expected: the two new tests fail (`missing variant section variant fn check [unique:p0]` / `thread`), because role recovery is not implemented yet. Everything else passes.

### Task 3B: Implement the collection and cap feed

- [ ] **Step 5: Add `collect_move_recovered_params` to `ownership.tw`**

Add this function next to `collect_field_reqs` in `boot/compiler/ownership.tw`. It reconstructs each block's entry `ForwardState` from the fixpoint result `fx` (mirroring the ret-classification loop in `summarize_seeded`), replays instructions with the resolver, and records params recovered by the **same** licensed move-projection branch the real `ARecordGet` transfer uses (`projection_move_licensed` + `pr.shell` present + projected shell provenance names a single param).

It takes the **liveness-annotated** `blocks` and `params` that `summarize_seeded` already built (not `f`), because `block_prep`'s `scan.live_after` requires the per-block liveness `summarize_seeded` set on those blocks:

```tw
// Params recovered by a licensed move-projection of a consuming-callee wrapper
// field. Mirrors the real `.ARecordGet` move branch (see transfer_op): the same
// projection_move_licensed gate, the same `pr.shell` deep-owned requirement, and
// the projected shell provenance resolving to exactly one param. Runs under the
// summary's seed + resolver (so the callee's owned variant is selected and its
// ret_paths recovery has set the wrapper field's provenance to the param).
fn collect_move_recovered_params(
  blocks: Vector<CfgBlock>,
  params: Vector<LocalId>,
  fx: FixResult,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  suppress: Dict<Int, Bool>,
  unique_seed: Dict<Int, Bool>,
  resolve: VariantResolver,
) Dict<Int, Bool> {
  out: Dict<Int, Bool> = Dict.new()
  done := all_processed(blocks)
  for blk in blocks {
    entry_own := join_entry_ownership(blk, fx.exits, done)
    entry_valid := join_entry_valid(blk, fx.exit_valid, done)
    entry_prov := join_entry_prov(blk, fx.exit_prov, done)
    if blk.id.id == 0 {
      entry_prov = seed_param_prov(entry_prov, params)
      entry_own = seed_param_own(entry_own, unique_seed, params)
    }
    entry_field := join_entry_field_own(blk, fx.exit_field_own, entry_own, done)
    entry_pp := join_entry_path_prov(blk, fx.exit_path_prov, done)
    st := ForwardState.{
      own: entry_own,
      valid: entry_valid,
      prov: entry_prov,
      field_own: entry_field,
      path_prov: entry_pp,
    }
    pred_id := case blk.preds.len() == 1 {
      true => blk.preds[0].target.id,
      false => 0 - 1,
    }
    st = if pred_id >= 0 {
      st.seed_payload_binding(
        blk,
        nested_get(fx.exit_field_own, pred_id),
        nested_get(fx.exit_path_prov, pred_id),
        nested_get(fx.exit_prov, pred_id),
      )
    } else {
      st
    }
    prep := block_prep(blk, sem)
    scan := prep.scan
    for inst, i in blk.instructions {
      last := last_use_at(inst.op, scan.live_after[i])
      case inst.op {
        .ARecordGet(base, fld, _) => case atom_local_id(base) {
          .Some(bid) => if st.projection_move_licensed(
            inst.anf_local.id,
            base,
            last,
            prep.moves,
            prep.transport,
          ) {
            pr := st.atom_field_own(base).project(ff.PathSeg.Field(fld.id))
            case pr.shell {
              .Some(_) => {
                proj_pp := project_path_prov(st.path_prov_get(bid), ff.PathSeg.Field(fld.id))
                case proj_pp.shell {
                  .Some(os) => if os.len() == 1 {
                    case param_index_of(params, os[0]) {
                      .Some(pk) => out[pk] = true,
                      .None => {},
                    }
                  },
                  .None => {},
                }
              },
              .None => {},
            }
          },
          .None => {},
        },
        _ => {},
      }
      st = .transfer_op(
        inst.anf_local.id,
        inst.op,
        last,
        prep.moves,
        prep.transport,
        table,
        b,
        sem,
        suppress,
        Dict.new(),
        resolve,
      )
    }
  }
  out
}
```

- [ ] **Step 6: Add the `move_recovered_has` helper**

Next to `collect_move_recovered_params`, add:

```tw
fn move_recovered_has(m: Dict<Int, Bool>, k: Int) Bool {
  case m.get(k) {
    .Some(v) => v,
    .None => false,
  }
}
```

- [ ] **Step 7: Call it in `summarize_seeded` and feed `cap`**

In `summarize_seeded`, the `raw_params` collect (which builds `esc`/`cap`) comes *before* `field_reqs` in the current source. Place the `move_recovered` call right after `fx` is computed (the `run_fixpoint_validated` result) and immediately before the `raw_params` collect, so `cap` can read it:

```tw
  move_recovered := collect_move_recovered_params(blocks, f.params, fx, table, b, sem, suppress, unique_seed, resolve)
```

`blocks` and `f.params` are both in scope in `summarize_seeded` (`blocks` is the liveness-annotated local; `f.params` is the `Vector<LocalId>`). Then in the `raw_params` loop, extend the `cap` derivation so a move-recovered param is `Consumed`. The current loop is:

```tw
  raw_params: Vector<RawParam> = collect p in f.params {
    esc: EscapeEffect = .Borrowed
    cap: ParamCapability = .NoCap
    for blk in blocks {
      if own_is_shared(nested_get(fx.exits, blk.id.id), p.id) {
        esc = .Retained
      }
      case nested_get(fx.exit_valid, blk.id.id).get(p.id) {
        .Some(v) => if !v {
          cap = .Consumed
        },
        .None => {},
      }
    }
    RawParam.{ esc, cap }
  }
```

Change the `cap` initialization line so it starts `Consumed` when the param is move-recovered:

```tw
  raw_params: Vector<RawParam> = collect p, pi in f.params {
    esc: EscapeEffect = .Borrowed
    cap: ParamCapability = if move_recovered_has(move_recovered, pi) {
      .Consumed
    } else {
      .NoCap
    }
    for blk in blocks {
      if own_is_shared(nested_get(fx.exits, blk.id.id), p.id) {
        esc = .Retained
      }
      case nested_get(fx.exit_valid, blk.id.id).get(p.id) {
        .Some(v) => if !v {
          cap = .Consumed
        },
        .None => {},
      }
    }
    RawParam.{ esc, cap }
  }
```

`collect p, pi in f.params` binds the param index `pi` (0-based), which matches `move_recovered_params`' key domain (param indices from `param_index_of`). The `move_recovered_has` helper was added in Step 6.

- [ ] **Step 8: Format, lint, typecheck**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/compiler/ownership.tw   # apply --fix for inherent-call findings, re-fmt
target/twk build boot/main.tw -o /tmp/check.wasm 2>&1 | tail -3
```

Expected: compiles clean.

- [ ] **Step 9: Fast-verify composition with a scratch driver (no full rebuild)**

Create `boot/scratch_role.tw`:

```tw
use commands.common.{format_compile_error}
use compiler.cfg
use compiler.opt.semantics as semantics
use compiler.ownership
use compiler.pipeline
use compiler.summary

fn dump(path: String) {
  artifacts := case pipeline.compile_entry_path(path) {
    .Ok(r) => r,
    .Err(e) => {
      println(format_compile_error(e))
      return
    },
  }
  b := artifacts.builtins
  view := cfg.build_view(artifacts.opt, b)
  view = ownership.prune_dead_merge(view)
  sem := semantics.make_prelude_optimizer_semantics(b)
  table := summary.compute(view, b, sem)
  variants := summary.compute_variants(view, b, sem, table)
  resolver := variants.make_variant_resolver_for_render()
  analyzed := ownership.analyze_with_summaries_resolved(view, b, sem, table, resolver)
  println(summary.render_cfg(view, analyzed, b, sem, table, variants))
}

fn main() {
  dump("/tmp/probe_gate.tw")
}

main()
```

Run:

```bash
target/twk run boot/scratch_role.tw 2>&1 | grep -E "variant fn check|verdict ->|p0=Consumed" | head
rm -f boot/scratch_role.tw
```

Expected: `variant fn check [unique:p0]` renders, its header shows `p0=Consumed`, and `build` shows a `verdict -> f...[unique:p0...]`. If not, re-check Steps 5–7 before rebuilding.

- [ ] **Step 10: Rebuild and run the suite**

```bash
make bundle-cli 2>&1 | tail -2
target/twk run boot/tests/main.tw 2>&1 | tail -1
```

Expected: all tests pass, including the two composition tests from Task 3A and the read-after NEG.

- [ ] **Step 11: Confirm the NEG still holds and the delegate/mixed fixtures are unaffected**

```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/red_transport_read_after.tw --cfg 2>&1 \
  | awk '/^fn check \[/{p=1} /^fn build \[/{p=0} p' | grep -cE "verdict ->|reuse\(unique\)"
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/red_transport_read_after.tw --cfg 2>&1 | grep -c "variant fn check"
for f in red_delegate_chain red_mixed_delegate_update; do
  echo -n "$f: "; target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/$f.tw --cfg 2>&1 | grep -cE "variant fn (resolve|outer)"
done
```

Expected: the read-after checks print `0` and `0` (NEG persistent); the delegate/mixed fixtures still show their variants (unchanged).

- [ ] **Step 12: Commit**

```bash
git add boot/compiler/ownership.tw \
  boot/tests/fixtures/cfg/sound_uniqueness/transport_wrapper_single.tw \
  boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw
git commit -m "analysis: recover Consumed role for transport-wrapper params

Add collect_move_recovered_params: replay each block from the seeded fixpoint and
record params recovered by a licensed move-projection whose projected shell
provenance names the param, mirroring the real ARecordGet move branch
(projection_move_licensed + pr.shell + project_path_prov). Feed the set into
cap=Consumed in summarize_seeded so a consumed-then-recovered param that flows to
return classifies as Consumed, with a non-empty (shell) in-place path.

Transport-wrapper chains now compose (build -> check -> synth); the single-hop
wrapper composes too. The read-after NEG stays persistent (esc=Retained fences
it); delegate/mixed fixtures are unchanged."
```

---

## Task 4: Over-fire control — verify the gate is not vacuously safe

**Files:**
- Possibly create: `boot/tests/fixtures/cfg/sound_uniqueness/transport_borrow_control.tw`
- Modify: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw` (only if a valid control exists)

The design's over-fire risk is a licensed move-projection that is **not** a consuming-callee recovery (a plain owned-field projection). Establish whether such a case can even reach candidacy; ship a real control if it can, otherwise record why it cannot and rely on the read-after NEG.

- [ ] **Step 1: Attempt a candidate-shaped non-recovery move-projection**

Reason from the code: transport-wrapper candidacy fires only via `mark_ret_path_field`, which now requires the callee to return the param through `ret_paths=OwnedFromParam(k)` with `base_role == .Consumed` — i.e. a consuming recovery. A bare `x := r.field` projection of a still-owned record param produces no `ret_paths`-derived candidacy and no `scan.found`, so it is never proposed. Delegated-consume candidacy (the `.ACall` ret-alias arm) requires `ret_aliases_exactly_param`, also not a bare projection.

Try to construct a counterexample fixture where a param's own field is move-projected yet the function still becomes a candidate. If you find one, add it as `transport_borrow_control.tw` and a suite test asserting its variant is **retracted** (param stays `Borrowed`, empty in-place paths), using `function_section` for bounded assertions. Verify it genuinely enters validation (its candidate is proposed) so the control is non-vacuous.

- [ ] **Step 2: If no such candidate exists, record it**

If (as the reasoning predicts) no candidate-shaped non-recovery move-projection can be constructed, add a short note to the design doc's Testing section stating the over-fire gate is unreachable-by-construction under the current candidacy (ret-path recovery implies consumption), so the read-after NEG plus the `base_role == .Consumed` gate are the soundness locks. Do **not** add a vacuous green test.

```bash
# after editing docs or adding a real control + test:
target/twk run boot/tests/main.tw 2>&1 | tail -1   # must stay green
git add -A && git commit -m "test|docs: over-fire control for transport role recovery

<Either: add transport_borrow_control.tw + retraction assertion (non-vacuous),
or: document that a candidate-shaped non-recovery move-projection is
unreachable-by-construction and rely on the read-after NEG + base_role gate.>"
```

---

## Task 5: Regression guard

**Files:** none.

- [ ] **Step 1: Boot census must not regress**

```bash
target/twk ir boot/main.tw --census --sites 2>&1 | grep -E "dict_set|vector_set"
```

Expected: in-place counts hold or increase versus the "Boot-side census" table in `docs/plans/sound-uniqueness/analysis/worked-examples.md`. If they drop, investigate before proceeding.

- [ ] **Step 2: Self-host fixed point (run alone)**

```bash
make stage2 2>&1 | tail -5
```

Expected: stage2 builds and `Fixed point reached: stage3 == stage4`.

- [ ] **Step 3: Full suites (run alone)**

```bash
make test 2>&1 | tail -5
```

Expected: all pass.

---

## Task 6: Documentation and finalize

**Files:**
- Modify: `docs/plans/sound-uniqueness/analysis/transport-wrapper-blocker.md` (mark resolved)
- Modify: `docs/plans/transitive-consume-plan.md` (note transport-wrapper landed via the role-recovery plan)
- Modify: `docs/plans/README.md` if it lists these plans

- [ ] **Step 1: Update the blocker note**

Append a "Resolved" section to `transport-wrapper-blocker.md` summarizing the shipped mechanism (candidacy consumption gate + `collect_move_recovered_params` → `cap=Consumed`) and pointing to the design doc.

- [ ] **Step 2: Reconcile the parent plan**

In `docs/plans/transitive-consume-plan.md`, note that the transport-wrapper shape (its Task 6/7 transport portions) is delivered by `transport-wrapper-role-recovery-plan.md`, and that the delegated-consume + mixed shapes already landed.

- [ ] **Step 3: Commit docs**

```bash
git add docs/plans/
git commit -m "docs: transport-wrapper role recovery landed

Mark the blocker analysis resolved and reconcile the transitive-consume plan:
the transport-wrapper shape composes via the candidacy consumption gate plus
collect_move_recovered_params -> cap=Consumed."
```

---

## Acceptance criteria

- `red_transport_wrapper_chain` and `transport_wrapper_single` render owned in-place threading: `variant fn check`/`variant fn thread` `[unique:p0]` with a recovered `p0=Consumed` role and a `verdict -> f...`, and `build` selects the outer `[unique:p0]` variant — asserted section-scoped. (`reuse(unique)` is not a signal for this shape; see the Task 3A note.)
- `red_transport_read_after` stays persistent: no `verdict ->`/`reuse(unique)` in the bounded `check` section, no `variant fn check`.
- Candidacy gates on callee consumption (`base_role == .Consumed`); `move_recovered_params` is collected from the real `ARecordGet` move branch and feeds `cap`.
- The over-fire risk is covered by a non-vacuous control or documented as unreachable-by-construction.
- `ownership.tw` does not import `summary.tw`; the resolver remains the boundary.
- Boot census in-place counts do not regress; `make stage2` fixed point holds; `make test` passes.

## Out of scope

- Deep field-backing in-place; codegen dispatch for owned variants; any general shell-as-mutation change to `has_mut`.
- The delegated-consume and mixed shapes (already landed).
