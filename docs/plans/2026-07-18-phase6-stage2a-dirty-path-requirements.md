# Phase 6 Part 2, Stage 2a — Dirty-Path Record-Update Requirements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A function that threads a parameter through a chain of record-update SSA results and returns the updated value classifies that parameter as `Consumed` and reports the updated field paths — via a self-contained **flow-aware dirty-path analysis** that matches real ANF lowering and ignores updates whose result never reaches the return.

**Architecture:** A small, standalone forward dataflow (independent of the ownership fixpoint) tracks per value: its single **origin** parameter (the param its record identity derives from, or none) and the set of field **paths dirtied** by in-place record updates reaching it. `ARecordUpdate(base,f,·)→result` copies `base`'s dirty set and adds `[f]`; `AAssign` copies; the returned atom's dirty set (read at each return block) yields each param's in-place field requirements. This is intraprocedural only — helper-call propagation (which unlocks `add_type`) is **Stage 2b**. Still analysis-only: `twk ir --census` stays **0 in-place**.

**Tech Stack:** Twinkle (`.tw`), boot compiler only. Tests via the boot suite. Build/verify with `make boot-test`.

---

## Why this design (context)

The earlier Stage 2 draft (now superseded) used a syntactic "base is a param local" scan. Real lowering breaks it: `RecordUpdate` lowers to a **fresh SSA local** (`lower_anf.tw:785-799`), so in a chain `env.a=…; env.b=…; env.c=…` only the first update has `base = param`; the rest have `base = the previous update's fresh result`. And checking whole-param flow (not update-*result* flow) admits `tmp := (p.f=v); return p` as a false positive.

The **dirty-path lattice** (maintainer-specified) fixes both:
- **SSA chains:** each update result carries the origin + accumulated dirty set forward, so `r2 := update(r1, b)` where `r1 := update(p0, a)` yields `r2` dirty `{[a],[b]}` origin `p0`. Reading the returned atom collects the full set.
- **Discarded updates:** `tmp := update(p, f); return p` — `tmp` is dirty, but the *returned* atom is `p` (dirty `{}`), so nothing is collected. No false positive.

Do **not** derive these from Phase-5 `ret_paths`: generic `Unknown` params need not produce owned ret-path facts, so `ret_paths` is not a reliable source here. The dirty-path lattice is a separate, self-sufficient analysis.

### Scope

**In scope (2a):** intraprocedural record-update chains on a parameter-derived record, read at the return. Worked example: a `register_type_entry`-shaped mutator (`env.type_index[k]=v; env.type_id_index[...]=...; env.types = .append(...); … ; env`) → `p0=Consumed paths{[], [.f…]}`.

**Explicitly out of scope (honest, not faked):**
- **`add_type` / helper-call propagation** → Stage 2b. A param threaded only through summarized helper calls (`env.register_type_entry(...).bind_type(...)`) yields **no** dirty paths in 2a (call results have origin *none*). `add_type` becomes an acceptance test in **2b**, not here.
- **Reference-vs-scalar field filter** → Stage 2c. `CfgFunction`/`ARecordUpdate` do not currently carry per-field type metadata (only the record `TypeId` and the op-result `MonoType`), so 2a **cannot** soundly filter scalar-field updates without new metadata. 2a therefore reports **candidate** field paths (all updated fields); scalar filtering lands before call-site specialization (Stage 2c or folded into Stage 4). This limitation is stated in the summary wording — it is not silently faked.
- **Deeper-than-`[f]` paths** (a field of a field): the lattice records only the directly-updated field; nested is a later refinement.

### File structure

- **Modify `boot/compiler/ownership.tw`:** add the dirty-path dataflow (`FlowFact` + `collect_field_reqs` + small path/flow helpers); broaden `reconcile_role` with a `has_mut` input; add `build_in_place_paths`; wire both into `summarize_function`'s param finalization. Import `path_cmp` from `variant_id` (already imported for `ParamPath`) to keep dirty-path sets canonical/deduped (reuses Stage 1).
- **Test `boot/tests/suites/cfg_summary_suite.tw`:** unit-test `collect_field_reqs` directly (localizes dataflow bugs), plus summary-level tests (real-lowering-shaped fixtures).

Verified anchors (current branch): `AnfOp.ARecordUpdate(Atom, FieldId, Atom, Bool, TypeId)` (`anf.tw:50`), `AnfOp.AAssign(LocalId, Atom)` (`anf.tw:55`), `atom_local_id` (`ownership.tw:80`), `return_atom(blk)` reads `.Return(.Some(a))` (`ownership.tw` near `:3294`), `CfgBlock.{ instructions, terminator, preds: Vector<CfgEdge> }` (`cfg.tw:59`), `CfgEdge.target` holds the **predecessor** id in a `preds` edge (`ownership.tw:3314`), `run_fixpoint`'s `for changed { for blk in blocks { … } }` join-to-fixpoint pattern (`ownership.tw:2596`), `variant_id.path_cmp(a,b) Order` (Stage 1), Part-1 `reconcile_role`/finalization collect in `summarize_function`.

---

## Task 1: The dirty-path dataflow (`collect_field_reqs`)

**Files:**
- Modify: `boot/compiler/ownership.tw` (new dataflow + helpers)
- Test: `boot/tests/suites/cfg_summary_suite.tw` (direct unit tests + a CfgFunction builder helper)

- [ ] **Step 1: Add a CfgFunction test helper + write failing unit tests**

In `cfg_summary_suite.tw`, add a helper that builds a `CfgFunction` from a body (mirrors `summ1` but returns the CFG function), and import `variant_id` is already present. Add:

```tw
fn cfg_func_of(name: String, nparams: Int, body: AnfExpr) cfg.CfgFunction {
  b := b_reg()
  v := cfg.build_view(module_of([fdef(1, name, nparams, body)]), b)
  case cfg.function_named(v, name) {
    .Some(f) => f,
    .None => error("missing ${name}"),
  }
}

// Does param `k` have exactly the given sorted field ids as its dirty [.f] paths
// (plus the analysis does NOT add the shell here — that is build_in_place_paths).
fn dirty_fields(reqs: Dict<Int, Vector<variant_id.ParamPath>>, k: Int) Vector<Int> {
  case reqs.get(k) {
    .Some(paths) => {
      out: Vector<Int> = []
      for p in paths {
        if p.len() == 1 {
          out = .append(p[0])
        }
      }
      out
    },
    .None => [],
  }
}
```

Then the failing tests:

```tw
    .test(
      "collect_field_reqs: SSA update chain returns full dirty set",
      fn() {
        b := b_reg()
        // fn f(env) { r1 := (env.f0 = d0); r2 := (r1.f1 = d1); r2 }   -- REAL lowering shape:
        // second update's base is r1 (the prior fresh result), NOT env.
        u0: AnfOp = .ARecordUpdate(.ALocal(lid(0)), FieldId.{ id: 0 }, .ALocal(lid(1)), false, TypeId.{ id: 0 })
        u1: AnfOp = .ARecordUpdate(.ALocal(lid(2)), FieldId.{ id: 1 }, .ALocal(lid(3)), false, TypeId.{ id: 0 })
        body: AnfExpr = .Let(
          lid(1), dict_new_call(b),
          .Let(lid(2), u0,
          .Let(lid(3), dict_new_call(b), .Let(lid(4), u1, .Atom(.ALocal(lid(4)))))),
        )
        reqs := ownership.collect_field_reqs(cfg_func_of("f", 1, body))
        try assert.is_true(same_ints(dirty_fields(reqs, 0), [0, 1])) // both fields, param 0
        .Ok({})
      },
    )
    .test(
      "collect_field_reqs: discarded update, original param returned -> no dirty",
      fn() {
        b := b_reg()
        // fn f(env) { tmp := (env.f0 = d); env }   -- update result discarded, return original
        u0: AnfOp = .ARecordUpdate(.ALocal(lid(0)), FieldId.{ id: 0 }, .ALocal(lid(1)), false, TypeId.{ id: 0 })
        body: AnfExpr = .Let(
          lid(1), dict_new_call(b), .Let(lid(2), u0, .Atom(.ALocal(lid(0)))),
        )
        reqs := ownership.collect_field_reqs(cfg_func_of("f", 1, body))
        try assert.equal(dirty_fields(reqs, 0).len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify failure (function undefined)**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -iE 'collect_field_reqs|undefined|error' | tail -5`
Expected: unresolved-name errors for `ownership.collect_field_reqs`.

- [ ] **Step 3: Implement the dirty-path lattice + dataflow in `ownership.tw`**

Add near the other summary helpers (above `summarize_function`). Uses `variant_id.path_cmp` for canonical dirty sets:

```tw
// ── Requirement-flow (dirty-path) dataflow ──────────────────────────
// Self-contained forward analysis (independent of the ownership fixpoint): per
// value, which single parameter its RECORD IDENTITY derives from (origin; -1 =
// none/ambiguous) and the field paths dirtied by in-place record updates reaching
// it. Reading the RETURNED atom's dirty set yields each param's in-place field
// requirements — correctly ignoring an update whose result never reaches the return.
type FlowFact = .{ origin: Int, dirty: Vector<ParamPath> }

fn ff_none() FlowFact {
  FlowFact.{ origin: 0 - 1, dirty: [] }
}

fn path_eq(a: ParamPath, b: ParamPath) Bool {
  case variant_id.path_cmp(a, b) {
    .Eq => true,
    _ => false,
  }
}

// Insert `p` into a canonical-sorted, deduped path set.
fn add_path(paths: Vector<ParamPath>, p: ParamPath) Vector<ParamPath> {
  out: Vector<ParamPath> = []
  inserted := false
  for q in paths {
    if !inserted {
      case variant_id.path_cmp(p, q) {
        .Eq => return paths,          // already present
        .Lt => {
          out = .append(p)
          inserted = true
        },
        .Gt => {},
      }
    }
    out = .append(q)
  }
  if !inserted {
    out = .append(p)
  }
  out
}

fn union_paths(a: Vector<ParamPath>, b: Vector<ParamPath>) Vector<ParamPath> {
  out := a
  for p in b {
    out = add_path(out, p)
  }
  out
}

fn paths_eq(a: Vector<ParamPath>, b: Vector<ParamPath>) Bool {
  if a.len() != b.len() {
    return false
  }
  for p, i in a {
    if !path_eq(p, b[i]) {
      return false
    }
  }
  true
}

fn flow_get(st: Dict<Int, FlowFact>, a: Atom) FlowFact {
  case atom_local_id(a) {
    .Some(id) => case st.get(id) {
      .Some(ff) => ff,
      .None => ff_none(),
    },
    .None => ff_none(),
  }
}

// Per-op transfer: only ARecordUpdate (record->record, add [f]) and AAssign (rebind)
// carry a fact; every other result defaults to none (chain breaks — ARecordGet is a
// field value, ACall is a call result handled in Stage 2b).
fn transfer_flow(st: Dict<Int, FlowFact>, op: AnfOp, result: Int) Dict<Int, FlowFact> {
  case op {
    .ARecordUpdate(base, fld, _, _, _) => {
      bf := flow_get(st, base)
      st[result] = if bf.origin >= 0 {
        FlowFact.{ origin: bf.origin, dirty: add_path(bf.dirty, [fld.id]) }
      } else {
        ff_none()
      }
      st
    },
    .AAssign(local, a) => {
      st[local.id] = flow_get(st, a)
      st
    },
    _ => st,
  }
}

fn join_fact(a: FlowFact, b: FlowFact) FlowFact {
  org := if a.origin == b.origin {
    a.origin
  } else {
    0 - 1
  }
  FlowFact.{ origin: org, dirty: union_paths(a.dirty, b.dirty) }
}

// Join predecessors' exit facts at a block entry. Mirrors join_entry_prov EXACTLY
// (ownership.tw): a block PARAMETER (SSA merge target) is sourced from each pred
// edge's arg `pe.args[i]`; a non-parameter live local carries over from the pred's
// exit by the same id (dominance). Only PROCESSED preds contribute (fixpoint
// correctness for back-edges). This is what makes branches/merges/loops correct —
// e.g. register_type_entry's case arms each update a different field of env and the
// merge unions their dirty sets. `param_index(blk, lid)` and `is_processed` are the
// same helpers join_entry_prov uses.
fn flow_join_entry(
  blk: CfgBlock,
  exits: Dict<Int, Dict<Int, FlowFact>>,
  processed: Dict<Int, Bool>,
) Dict<Int, FlowFact> {
  entry: Dict<Int, FlowFact> = Dict.new()
  for lid in blk.entry.live {
    pidx := param_index(blk, lid)
    acc := ff_none()
    seen := false
    for pe in blk.preds {
      if is_processed(processed, pe.target.id) {
        pex := case exits.get(pe.target.id) {
          .Some(m) => m,
          .None => Dict.new(),
        }
        src := case pidx {
          .Some(i) => if i < pe.args.len() {
            flow_get(pex, pe.args[i])
          } else {
            ff_none()
          },
          .None => case pex.get(lid) {
            .Some(x) => x,
            .None => ff_none(),
          },
        }
        acc = if seen {
          join_fact(acc, src)
        } else {
          src
        }
        seen = true
      }
    }
    if seen {
      entry[lid] = acc
    }
  }
  entry
}

fn flow_map_eq(a: Dict<Int, FlowFact>, b: Dict<Int, FlowFact>) Bool {
  if a.keys().len() != b.keys().len() {
    return false
  }
  for k in a.keys() {
    af := case a.get(k) {
      .Some(x) => x,
      .None => ff_none(),
    }
    bf := case b.get(k) {
      .Some(x) => x,
      .None => return false,
    }
    if af.origin != bf.origin or !paths_eq(af.dirty, bf.dirty) {
      return false
    }
  }
  true
}

// Forward dataflow to a fixed point (dirty grows, origin only degrades -> terminates),
// modeled on run_fixpoint's `for changed { for blk }` shape with the same processed
// guard. Computes its own liveness so it is self-contained (unit-testable on a raw
// CfgFunction), exactly as summarize_function annotates blocks. Returns, per
// parameter index, the union of dirty [.f] paths carried by the value returned at
// any return block.
pub fn collect_field_reqs(f: CfgFunction) Dict<Int, Vector<ParamPath>> {
  live := compute_liveness(f.blocks)
  blocks := collect blk in f.blocks {
    bl := live_get(live, blk.id.id)
    blk.entry.live = bl.live_in
    blk.exit.live = bl.live_out
    blk
  }
  exits: Dict<Int, Dict<Int, FlowFact>> = Dict.new()
  processed: Dict<Int, Bool> = Dict.new()
  for blk in blocks {
    exits[blk.id.id] = Dict.new()
    processed[blk.id.id] = false
  }
  changed := true
  for changed {
    changed = false
    for blk in blocks {
      entry := flow_join_entry(blk, exits, processed)
      if blk.id.id == 0 {
        for p, i in f.params {
          entry[p.id] = FlowFact.{ origin: i, dirty: [] }
        }
      }
      st := entry
      for inst in blk.instructions {
        st = transfer_flow(st, inst.op, inst.anf_local.id)
      }
      prev := case exits.get(blk.id.id) {
        .Some(m) => m,
        .None => Dict.new(),
      }
      if !flow_map_eq(prev, st) {
        exits[blk.id.id] = st
        changed = true
      }
      processed[blk.id.id] = true
    }
  }
  reqs: Dict<Int, Vector<ParamPath>> = Dict.new()
  for blk in blocks {
    case return_atom(blk) {
      .Some(a) => {
        fexit := case exits.get(blk.id.id) {
          .Some(m) => m,
          .None => Dict.new(),
        }
        rf := flow_get(fexit, a)
        if rf.origin >= 0 and rf.dirty.len() > 0 {
          prev := case reqs.get(rf.origin) {
            .Some(v) => v,
            .None => [],
          }
          reqs[rf.origin] = union_paths(prev, rf.dirty)
        }
      },
      .None => {},
    }
  }
  reqs
}
```

> **Notes.** (1) `compute_liveness`/`live_get`/`param_index`/`is_processed` are the same helpers `summarize_function` and `join_entry_prov` use — verify their exact signatures against those call sites (the plan copies their usage verbatim). (2) `f.params` on a `CfgFunction` is `Vector<LocalId>`, so `p.id` is the param's local id and `i` its index. (3) The join mirrors `join_entry_prov` so merges/loops are handled with the same proven logic — no simplified same-id join.

- [ ] **Step 4: Run the unit tests to verify they pass**

Run: `make boot-test 2>&1 | grep -E 'Ran [0-9]+ tests|FAIL' | tail -3` (see Task 3 for the pipefail-safe form)
Expected: `Ran N tests: N passed` (the chain test yields `[0,1]`; the discarded test yields `[]`).

- [ ] **Step 5: Add a branch-join unit test (register_type_entry shape)**

```tw
    .test(
      "collect_field_reqs: field updates across a branch join both reach return",
      fn() {
        b := b_reg()
        // fn f(env, c) {
        //   if c { r1 := (env.f0 = d) ; ret r1 } else { r2 := (env.f1 = d) ; ret r2 }
        // }  -- each arm updates a different field on env and returns it.
        // `.AIf(cond, then, else)` is an AnfOp bound in a Let; its result (lid5) is the
        // SSA merge block param, fed by each arm's value (lid3/lid4) as edge args.
        d0: AnfOp = dict_new_call(b)
        thenb: AnfExpr = .Let(lid(3), .ARecordUpdate(.ALocal(lid(0)), FieldId.{ id: 0 }, .ALocal(lid(2)), false, TypeId.{ id: 0 }), .Atom(.ALocal(lid(3))))
        elseb: AnfExpr = .Let(lid(4), .ARecordUpdate(.ALocal(lid(0)), FieldId.{ id: 1 }, .ALocal(lid(2)), false, TypeId.{ id: 0 }), .Atom(.ALocal(lid(4))))
        body: AnfExpr = .Let(
          lid(2), d0,
          .Let(lid(5), .AIf(.ALocal(lid(1)), thenb, elseb), .Atom(.ALocal(lid(5)))),
        )
        reqs := ownership.collect_field_reqs(cfg_func_of("f", 2, body))
        // env (lid0) carries into both arms by dominance; each arm dirties a different
        // field; the merge (lid5) unions the arms' dirty sets -> {f0, f1} for param 0.
        try assert.is_true(same_ints(dirty_fields(reqs, 0), [0, 1]))
        .Ok({})
      },
    )
```

> **Merge shape:** `.AIf` produces then/else blocks feeding a join block whose parameter (`lid5`) is sourced from each arm's edge arg. `flow_join_entry`'s block-param branch (`param_index` → `pe.args[i]`) unions the arms' facts, so `lid5` carries `{[f0],[f1]}` origin `p0`. `env` (`lid0`) is a non-param live local carried into each arm by the dominance branch. If the built CFG differs (e.g. `.AIf` shape isn't as assumed), first confirm by printing `--cfg` for the fixture; the assertion (`{0,1}` for param 0) holds under any correct SSA merge.

- [ ] **Step 6: Run + commit**

Run (pipefail-safe): `set -o pipefail; make boot-test 2>&1 | tail -40 | grep -E 'Ran [0-9]+ tests|FAIL'`
Expected: `Ran N tests: N passed`.

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "phase6 stage2a: flow-aware dirty-path record-update requirement analysis

Self-contained forward dataflow (origin + dirtied field paths per value) that
follows real SSA update chains and reads the returned atom, so chained updates
collect fully and a discarded update whose result is not returned is ignored.
Not yet wired into base_role/in_place_paths (Task 2). Helper-call propagation is
Stage 2b; scalar-field filtering is Stage 2c."
```

---

## Task 2: Wire dirty-paths into `base_role` + `in_place_paths`

**Files:**
- Modify: `boot/compiler/ownership.tw` (broaden `reconcile_role`, add `build_in_place_paths`, wire finalization)
- Test: `boot/tests/suites/cfg_summary_suite.tw` (summary-level)

- [ ] **Step 1: Write the failing summary test — chain mutator classifies Consumed with field paths**

```tw
    .test(
      "phase6 stage2a: param threaded through update chain -> Consumed with field paths",
      fn() {
        b := b_reg()
        // fn f(env) { r1 := (env.f0 = d0); r2 := (r1.f1 = d1); r2 }  (real SSA chain)
        u0: AnfOp = .ARecordUpdate(.ALocal(lid(0)), FieldId.{ id: 0 }, .ALocal(lid(1)), false, TypeId.{ id: 0 })
        u1: AnfOp = .ARecordUpdate(.ALocal(lid(2)), FieldId.{ id: 1 }, .ALocal(lid(3)), false, TypeId.{ id: 0 })
        body: AnfExpr = .Let(
          lid(1), dict_new_call(b),
          .Let(lid(2), u0,
          .Let(lid(3), dict_new_call(b), .Let(lid(4), u1, .Atom(.ALocal(lid(4)))))),
        )
        s := summ1("f", 1, body)
        try assert.equal(p_role(s, 0), role_tag(.Consumed))
        try assert.is_true(s.params[0].flows_to_return)
        // in_place_paths == [ [], [0], [1] ]  (shell + both fields)
        try assert.equal(s.params[0].in_place_paths.len(), 3)
        try assert.equal(s.params[0].in_place_paths[0].len(), 0)
        try assert.equal(s.params[0].in_place_paths[1][0], 0)
        try assert.equal(s.params[0].in_place_paths[2][0], 1)
        .Ok({})
      },
    )
    .test(
      "phase6 stage2a: discarded update, original returned -> Borrowed, no paths",
      fn() {
        b := b_reg()
        u0: AnfOp = .ARecordUpdate(.ALocal(lid(0)), FieldId.{ id: 0 }, .ALocal(lid(1)), false, TypeId.{ id: 0 })
        body: AnfExpr = .Let(lid(1), dict_new_call(b), .Let(lid(2), u0, .Atom(.ALocal(lid(0)))))
        s := summ1("f", 1, body)
        try assert.equal(p_role(s, 0), role_tag(.Borrowed))
        try assert.equal(s.params[0].in_place_paths.len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify failure**

Run: `set -o pipefail; target/twk run boot/tests/main.tw 2>&1 | grep -A6 'update chain -> Consumed' | tail -10`
Expected: FAIL — the param is still `Borrowed` / `in_place_paths` empty (dirty-paths not yet wired into the role/paths).

- [ ] **Step 3: Broaden `reconcile_role` and add `build_in_place_paths`**

Replace the Part-1 `reconcile_role` (add `has_mut`), and add the builder:

```tw
// Canonical would-mutate model: a param with a dirty (in-place-updated) path that
// flows to the return is Consumed; else the escape/move classification stands.
fn reconcile_role(
  esc: EscapeEffect,
  cap: ParamCapability,
  has_mut: Bool,
  flows_to_return: Bool,
) ParamRole {
  case esc {
    .Retained => .Published,
    .Borrowed => {
      consumes := has_mut or case cap {
        .Consumed => true,
        .NoCap => false,
      }
      if consumes and flows_to_return {
        .Consumed
      } else {
        .Borrowed
      }
    },
  }
}

// Shell [] (downward-closed under []) plus the collected dirty field paths, which
// are already canonical-sorted/deduped by add_path.
fn build_in_place_paths(dirty: Vector<ParamPath>) Vector<ParamPath> {
  out: Vector<ParamPath> = [[]]
  for p in dirty {
    out = .append(p)
  }
  out
}
```

- [ ] **Step 4: Wire into `summarize_function` finalization**

Right after the `raw_params` collect (where `f` is unshadowed), add:

```tw
  field_reqs := collect_field_reqs(f)
```

Replace the Part-1 finalization collect (`params: Vector<ParamSummary> = collect rp, i in raw_params { … }`) with:

```tw
  params: Vector<ParamSummary> = collect rp, i in raw_params {
    flows := param_flows_to_return(i, ret, ret_paths_final)
    dirty := case field_reqs.get(i) {
      .Some(v) => v,
      .None => [],
    }
    has_mut := dirty.len() > 0
    role := reconcile_role(rp.esc, rp.cap, has_mut, flows)
    ipp: Vector<ParamPath> = case role {
      .Consumed => build_in_place_paths(dirty),
      _ => [],
    }
    ParamSummary.{ base_role: role, in_place_paths: ipp, flows_to_return: flows }
  }
```

(Move-consumed params from Part 1 have empty `dirty`, so `build_in_place_paths([]) = [[]]` — their coarse shell path is preserved. The single `reconcile_role` call site inside this collect gets the new 4-arg form.)

- [ ] **Step 5: Run the summary tests to verify green**

Run (pipefail-safe): `set -o pipefail; make boot-test 2>&1 | tail -40 | grep -E 'Ran [0-9]+ tests|Fixed point|FAIL'`
Expected: `Fixed point reached: stage3 == stage4` and `Ran N tests: N passed`. If a pre-existing Part-1 role/render test now fails because a param correctly reclassifies `Consumed` (a direct flowing update chain), update that expectation and note it in the commit.

- [ ] **Step 6: Add a render test**

```tw
    .test(
      "phase6 stage2a: render shows field paths",
      fn() {
        s := ownership.Summary.{
          params: [
            ownership.ParamSummary.{ base_role: .Consumed, in_place_paths: [[], [0], [1]], flows_to_return: true },
          ],
          ret: .OwnedFresh,
          ret_paths: [],
        }
        try assert.equal(summary.render_summary(s), "summary: p0=Consumed paths{[],[.f0],[.f1]} ret=fresh")
        .Ok({})
      },
    )
```

- [ ] **Step 7: Run + commit**

Run (pipefail-safe): `set -o pipefail; make boot-test 2>&1 | tail -40 | grep -E 'Ran [0-9]+ tests|FAIL'` → `Ran N tests: N passed`.

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "phase6 stage2a: classify update-chain params Consumed with field paths

Wire the dirty-path analysis into base_role (has_mut) and in_place_paths: a param
threaded through a flowing record-update chain is Consumed and carries its field
paths ([], [.f]...). Move-consumed params keep their shell [] path. Analysis only;
census stays 0. add_type (helper-call chain) still gets nothing -> Stage 2b."
```

---

## Task 3: Stage 2a verification

**Files:** none (verification only). All commands use `set -o pipefail` so a `make` failure is not masked by a trailing `grep`/`tail`.

- [ ] **Step 1: Census unchanged (analysis-only invariant)**

Run: `set -o pipefail; target/twk ir boot/main.tw --census 2>&1 | awk 'NR>1 && NF>=3 {s+=$NF} END{print "total in_place:", s+0}'`
Expected: `total in_place: 0`.

- [ ] **Step 2: Full boot suite + self-host fixed point (unmasked)**

Run: `make boot-test`
Expected (read the tail): `Fixed point reached: stage3 == stage4` and `Ran N tests: N passed`. Run alone; no parallel heavy `twk`. Do NOT pipe into `grep|tail` for the authoritative check — inspect the real exit / full tail.

- [ ] **Step 3: Lint clean**

Run: `target/twk lint boot/main.tw`
Expected: no new findings introduced by this change (report-only; `--explain` for rationale).

- [ ] **Step 4: Real-code sanity — a `register_type_entry`-shaped mutator renders field paths**

Run: `set -o pipefail; target/twk ir boot/main.tw --cfg 2>&1 | grep -m5 'Consumed paths{\[\],\[\.f'`
Expected: at least one real function that threads a param through a record-update chain and returns it (e.g. `register_type_entry`) now renders `p0=Consumed paths{[],[.f…]}`. If none appear, verify by building a boot-suite fixture of that exact shape — the suite tests are authoritative.

- [ ] **Step 5: Byte-identical builds (determinism)**

Run: `set -o pipefail; target/twk build boot/main.tw -o /tmp/p6s2a_x.wasm && target/twk build boot/main.tw -o /tmp/p6s2a_y.wasm && cmp /tmp/p6s2a_x.wasm /tmp/p6s2a_y.wasm && echo IDENTICAL`
Expected: `IDENTICAL` (the dirty-path sets are canonical-sorted via `path_cmp`, so no ordering nondeterminism).

---

## Deferred (tracked)

| Item | Home |
|---|---|
| Helper-call summary propagation (unlocks `add_type`) | **Stage 2b** — own plan after 2a lands |
| Reference-vs-scalar field filter (needs per-field type metadata not on `CfgFunction`) | **Stage 2c** — add metadata explicitly or filter before call-site specialization |
| Deeper-than-`[f]` nested paths | later refinement |
| `ConsumedPaths` (6th `ForwardState` field) + `consume_dead` + acceptance #5/#7/#8 | **Stage 4** (call-site decision) |
| Owned-entry re-analysis validating these candidate paths | **Stage 3** |

## Self-Review

- **Real-lowering fidelity:** fixtures use the true SSA shape — a chained update's second `ARecordUpdate` has `base = the prior result local` (`lid(2)`), not the param — matching `lower_anf.tw:785`. The discarded-update negative (`return` the original param) pins the flow-of-*result* semantics.
- **False-positive killed:** `collect_field_reqs` reads only the returned atom's dirty set, so `tmp := (p.f=v); return p` yields no path (Task 1 + Task 2 negative tests).
- **No faked filter:** scalar-vs-reference filtering is explicitly deferred to Stage 2c with the metadata reason stated; 2a reports candidate paths and says so.
- **Verification not masked:** every `make boot-test` check uses `set -o pipefail` or is run unpiped; `twk lint` is run after edits (project requirement).
- **Type consistency:** `FlowFact`, `collect_field_reqs`, `build_in_place_paths`, and `reconcile_role(esc,cap,has_mut,flows)` are used consistently; `ParamPath`/path helpers reuse `variant_id.path_cmp` (Stage 1). The single `reconcile_role` call site is updated in Task 2.
- **Merges/loops handled, not corner-cut:** `flow_join_entry` mirrors `join_entry_prov` exactly — SSA block parameters sourced from `pe.args[i]`, non-params carried by dominance, only processed preds contribute, over real `compute_liveness` — so branches (`register_type_entry`'s case), joins, and loop back-edges are correct with the same proven logic as the ownership fixpoint. (Chosen over threading a 6th `ForwardState` field: a self-contained pass keeps blast radius off `run_fixpoint` while replicating its join faithfully — full capability either way.)
- **Termination:** the dataflow is monotone (dirty grows via `union_paths`; origin only degrades to −1), bounded by (#locals × #fields), so `for changed` converges — same argument class as `run_fixpoint`.
- **Honest acceptance:** `add_type` is **not** claimed here (helper-call chain → no dirty paths in 2a); it is Stage 2b's acceptance test. 2a's worked example is a `register_type_entry`-shaped direct mutator.
