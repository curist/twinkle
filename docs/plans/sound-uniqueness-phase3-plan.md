# Phase 3 Minimal Summaries — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the Phase 2 intraprocedural ownership analysis a first layer of whole-program **function summaries** so a call to a known helper stops being a blanket publication boundary — improving `twk ir --cfg` facts — plus two facts-precision items and doc/audit hygiene. Analysis-only; no codegen.

**Architecture:** Fold **param-provenance** into the Phase 2 forward `ForwardState` (a third fact threaded through the same join+fixpoint as ownership/validity). A new `summary.tw` extracts the whole-program call graph, orders it with the existing Tarjan SCC util, and computes a `SummaryTable` bottom-up (conservative seed, iterate to a cap). Summaries are consumed in `transfer_call` so borrowed args stay `Unique`, fresh returns become `Unique`, and retained/aliased origins demote to `Shared`. Two precision items (dead-merge param pruning, match-arm pattern-binding kills) and hygiene (rewrite the stale `opt/README.md`, optimizer audit) round it out.

**Tech Stack:** Twinkle (`.tw`), boot self-hosted compiler, `@std.testing` runner, hand-built multi-function `AnfModule` fixtures (stable `LocalId`/`FuncId`, no optimizer), `compiler.opt.semantics` (`call_info`), `compiler.graph_scc` (Tarjan), `make bundle-cli` for the CLI.

**Design spec (read before starting):** `docs/plans/sound-uniqueness/phase3-design.md` — the canonical design this plan implements. Supporting: `docs/plans/sound-uniqueness/summary-specialization.md`, `docs/plans/sound-uniqueness/README.md`.

---

## File structure

- **Modify** `boot/compiler/ownership.tw` — summary *types*; `ForwardState.prov`; provenance propagation in `transfer_op`; transitive `publish_local`; body-only forward (`forward_block_body`); `prov` threaded through `run_fixpoint`/joins (`FixResult.exit_prov`, `join_entry_prov`); per-function classification (`summarize_function`); call-site consumption in `transfer_call`; an `analyze_with_summaries` entry (keep the 3-arg `analyze` as an empty-table wrapper).
- **Create** `boot/compiler/summary.tw` — table constructor, call-graph extraction, SCC-ordered fixpoint (`compute`), and the per-function `--cfg` header renderer.
- **Modify** `boot/compiler/cfg.tw` — `render_view_with_headers`; `CfgBlock.bound`; `prune_dead_merge` support.
- **Modify** `boot/commands/ir.tw` — `--cfg` runs `prune_dead_merge → summary.compute → analyze_with_summaries → summary.render_cfg`.
- **Create** `boot/tests/suites/cfg_summary_suite.tw` — TDD gate over hand-built multi-function fixtures; register in `boot/tests/main.tw`.
- **Modify** `boot/tests/suites/cfg_ownership_facts_suite.tw` — pattern-binding + dead-merge fixtures.
- **Modify** `boot/compiler/opt/README.md`, `docs/plans/sound-uniqueness/README.md`.

## Conventions (read once)

- **Soundness before coverage.** Any callee without a summary (unknown Twinkle fn, extern, indirect/closure, `Cell` op) keeps the Phase 2 conservative publish-all bucket. Never mint `Unique`/`Borrowed` without proof.
- **Two summary axes.** Caller-visible **escape** (`Borrowed`/`Retained`) is the only axis `transfer_call` acts on. **Capability** (`Consumed`) is recorded, and must **never** invalidate a caller binding in Phase 3.
- **Provenance is param-locals.** `prov[local]` = sorted `Vector<Int>` of origin **parameter local ids**. Publication is transitive over `prov`. Aggregates carry the union of their fields' `prov`.
- **Escape reads post-publish exits; return reads body-only.** Escape/capability are classified over **every block's `fx.exits`** (post-terminator-publish), so a param published on a dead branch, or an aggregate-wrapped param published by the return terminator, is captured. The return effect is classified from **`forward_block_body`** at return blocks (pre-publish), so the returned value isn't spuriously `Shared`.
- **Boot gotchas (from Phase 2):**
  - Dict `[]`-read returns `Option` — use `.get(k)` + unwrap; never `m[k].field`.
  - `for`/assignment are statements — a `case`/`if` arm whose body is a `for` must wrap it in `{ … }`.
  - `target/twk fmt` reformats aggressively (single-line `.test(...)` may become multi-line); re-locate anchors after `fmt`.
  - `target/twk lint boot/main.tw` enforces inherent-method style + `direct-rebinding`/`record-copy-helper`; `--fix` applies safe rewrites. A `record-copy-helper` finding means rebind a field (`r.f = v; r`) instead of rebuilding.
- **Determinism.** Iterate blocks/params by id/index order; `prov`/live are sorted `Vector<Int>`; never let `Dict` iteration decide output order.
- **After editing `.tw`:** `target/twk fmt <files>` then `target/twk lint boot/main.tw`.
- **Test the boot suite:** `target/twk run boot/tests/main.tw`. The CLI flag needs `make bundle-cli`.
- **Test API.** One `pub fn suite() runner.Suite`, fluent `.test("case", fn(){ …; .Ok({}) })`; assertions `try assert.equal(a,b)` / `is_true` / `is_false`; `assert.fail(msg)` returns `Err`. Register in `boot/tests/main.tw`.
- **Commits.** Short imperative subject; body for non-trivial changes.

---

## Guardrails (read before Task 1)

- **G1 — `analyze` stays the empty-table wrapper.** Phase 2's `pub fn analyze(view,b,sem)` has ~20 call sites. Do **not** change its signature. Add `pub fn analyze_with_summaries(view,b,sem,table)`; make `analyze` call it with `empty_table()`. Existing facts tests stay unchanged.
- **G2 — Phase 2 fixtures stay green throughout.** Every Phase 2 fixture uses `params: []`, so provenance is empty and transitive publish is a no-op. A Phase 2 regression means a threading bug.
- **G3 — Escape over post-publish exits; return over body-only.** (See Conventions.) If summaries come out all-`Shared`, the return classifier is reading post-publish state; if a returned wrapper's param shows `borrow`, the escape classifier is reading only the return block instead of all blocks.
- **G4 — Consumption in one place.** `transfer_call` is used by both `analyze` and `summarize_function` (via the forward pass). Implement consumption once; both benefit.
- **G5 — Dead-merge is pre-analysis.** `prune_dead_merge` runs before `compute`/`analyze` and returns a new view; the analysis runs fresh on it. Never mutate an analyzed CFG.
- **G6 — Module cycle is fine.** A `summary.tw ↔ ownership.tw` import relationship is acceptable in Twinkle (verified). Types nonetheless live in `ownership.tw`; `summary.tw` imports them. `cfg.tw` must **not** import `ownership`/`summary` — headers are passed in as a `Dict<Int,String>`.

---

## Task 1: Summary types, provenance, and escape/capability classification

Add the summary types, fold `prov` into the forward analysis, make publication transitive, and classify the **escape/capability** axes (return effect is a stub `Shared` here; Task 2 makes it real). This proves the provenance plumbing and closes the two aggregate/dead-branch escape holes.

**Files:**
- Modify: `boot/compiler/ownership.tw`.
- Create: `boot/compiler/summary.tw` (table constructor only).
- Create + register: `boot/tests/suites/cfg_summary_suite.tw`; `boot/tests/main.tw`.

- [ ] **Step 1: Write the failing test (single-function, escape axis)**

Create `boot/tests/suites/cfg_summary_suite.tw`:

```tw
use @std.testing.assert as assert
use @std.testing as runner

use compiler.anf.{AnfExpr, AnfFunctionDef, AnfModule, AnfOp}
use compiler.builtins
use compiler.cfg
use compiler.core_ir.{FuncId, GlobalId, LocalId, Param}
use compiler.mono_type.{MonoType, TypeId}
use compiler.opt.semantics as semantics
use compiler.opt.semantics.{make_prelude_optimizer_semantics}
use compiler.ownership
use compiler.summary

fn lid(id: Int) LocalId {
  LocalId.{ id }
}

fn b_reg() builtins.BuiltinRegistry {
  builtins.make_builtin_registry()
}

fn sem() semantics.OptimizerSemantics {
  make_prelude_optimizer_semantics(b_reg())
}

fn dict_new_call(b: builtins.BuiltinRegistry) AnfOp {
  .ACall(.AGlobalFunc(b.method_id("Dict", "new")), [])
}

fn wrapper_record(field_val: LocalId) AnfOp {
  // TypeId/FieldId are structural placeholders; build_view/summary do not need a
  // real type table. FieldAtom.{ field, value }.
  .ARecord(TypeId.{ id: 0 }, [.{ field: FieldId.{ id: 0 }, value: .ALocal(field_val) }])
}

fn fdef(id: Int, name: String, nparams: Int, body: AnfExpr) AnfFunctionDef {
  params: Vector<Param> = collect i in range(nparams) {
    Param.{ local: lid(i), ty: MonoType.Int }
  }
  AnfFunctionDef.{
    func_id: FuncId.{ id },
    name,
    is_init: false,
    params,
    op_result_mono: Dict.new(),
    body,
    return_ty: MonoType.Int,
  }
}

fn module_of(funcs: Vector<AnfFunctionDef>) AnfModule {
  AnfModule.{
    functions: funcs,
    init_func_id: .None,
    extern_imports: Dict.new(),
    global_monos: Dict.new(),
    lib_exports: [],
  }
}

fn summ1(name: String, nparams: Int, body: AnfExpr) ownership.Summary {
  b := b_reg()
  m := module_of([fdef(1, name, nparams, body)])
  v := cfg.build_view(m, b)
  case cfg.function_named(v, name) {
    .Some(f) => ownership.summarize_function(f, summary.empty_table(), b, sem()),
    .None => error("missing ${name}"),
  }
}

fn escape_tag(e: ownership.EscapeEffect) Int {
  case e {
    .Borrowed => 0,
    .Retained => 1,
  }
}

fn cap_tag(c: ownership.ParamCapability) Int {
  case c {
    .NoCap => 0,
    .Consumed => 1,
  }
}

fn p_escape(s: ownership.Summary, i: Int) Int {
  escape_tag(s.params[i].escape)
}

pub fn suite() runner.Suite {
  runner
    .suite("cfg summary")
    .test("t1 borrow: fn f(x) { Dict.new() } -> p0 borrow", fn() {
      b := b_reg()
      s := summ1("f", 1, .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1)))))
      try assert.equal(p_escape(s, 0), escape_tag(.Borrowed))
      .Ok({})
    })
    .test("t1 retain: fn f(x) { global_set G0 = x; 0 } -> p0 retain", fn() {
      body: AnfExpr = .Let(lid(1), .AGlobalSet(GlobalId.{ id: 0 }, .ALocal(lid(0))), .Atom(.ALitInt(0)))
      s := summ1("f", 1, body)
      try assert.equal(p_escape(s, 0), escape_tag(.Retained))
      .Ok({})
    })
    .test("t1 aggregate-escape: fn f(x) { Wrapper.{x} } -> p0 retain (blocker 1)", fn() {
      b := b_reg()
      body: AnfExpr = .Let(lid(1), wrapper_record(lid(0)), .Atom(.ALocal(lid(1))))
      s := summ1("f", 1, body)
      try assert.equal(p_escape(s, 0), escape_tag(.Retained))
      .Ok({})
    })
    .test("t1 dead-branch publish: fn f(x){ if c { global_set G=x }; 0 } -> p0 retain (blocker 2)", fn() {
      // then-arm publishes x; x is dead at the return. Escape must still be Retained.
      then_e: AnfExpr = .Let(lid(2), .AGlobalSet(GlobalId.{ id: 0 }, .ALocal(lid(0))), .Atom(.ALitInt(0)))
      else_e: AnfExpr = .Atom(.ALitInt(0))
      body: AnfExpr = .Let(lid(1), .AIf(.ALitBool(true), then_e, else_e), .Atom(.ALitInt(0)))
      s := summ1("f", 1, body)
      try assert.equal(p_escape(s, 0), escape_tag(.Retained))
      .Ok({})
    })
}
```

Register in `boot/tests/main.tw` (`use .suites.cfg_summary_suite` + `cfg_summary_suite.suite()`).

> Note: confirm `FieldId` is importable (`use compiler.core_ir.{…, FieldId}`) — grep `pub type FieldId`. Add it to the imports.

- [ ] **Step 2: Run it to verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — no `compiler.summary` / no `ownership.summarize_function` / no summary types.

- [ ] **Step 3: Add summary types + `summary.tw` constructor**

In `ownership.tw`, after the `Ownership` enum:

```tw
pub type EscapeEffect = { Borrowed, Retained }
pub type ParamCapability = { NoCap, Consumed }
pub type ParamSummary = .{ escape: EscapeEffect, capability: ParamCapability }
pub type ReturnEffect = { OwnedFresh, MayAliasParams(Vector<Int>), Shared }
pub type Summary = .{ params: Vector<ParamSummary>, ret: ReturnEffect }
pub type SummaryTable = .{ by_func: Dict<Int, Summary> }

pub fn summary_get(t: SummaryTable, func_id: Int) Summary? {
  t.by_func.get(func_id)
}
```

Create `boot/compiler/summary.tw`:

```tw
//! Phase 3 interprocedural summary driver. Task 1: table constructor only;
//! Task 3 adds the call-graph SCC fixpoint; Task 5 adds the --cfg header.
use compiler.ownership.{SummaryTable}

pub fn empty_table() SummaryTable {
  SummaryTable.{ by_func: Dict.new() }
}
```

- [ ] **Step 4: Add `prov` to `ForwardState` + helpers**

```tw
type ForwardState = .{ own: Dict<Int, Int>, valid: Dict<Int, Bool>, prov: Dict<Int, Vector<Int>> }
```

Update every `ForwardState.{ … }` construction to add `prov: Dict.new()` (grep `ForwardState.{`). Add:

```tw
fn prov_of(prov: Dict<Int, Vector<Int>>, a: Atom) Vector<Int> {
  case atom_local_id(a) {
    .Some(id) => case prov.get(id) {
      .Some(v) => v,
      .None => [],
    },
    .None => [],
  }
}

fn set_prov_st(st: ForwardState, id: Int, origins: Vector<Int>) ForwardState {
  st.prov[id] = origins
  st
}
```

- [ ] **Step 5: Transitive `publish_local`**

```tw
fn publish_local(st: ForwardState, id: Int) ForwardState {
  st = set_own_st(st, id, .Shared)
  case st.prov.get(id) {
    .Some(origins) => {
      for o in origins {
        st = set_own_st(st, o, .Shared)
      }
      st
    },
    .None => st,
  }
}
```

- [ ] **Step 6: Provenance propagation in `transfer_op`**

Update the arms below (keep existing own/valid logic; add the `prov` side):

```tw
    .ARecord(_, fields) => {
      origins: Vector<Int> = []
      for fa in fields {
        origins = union_sorted(origins, prov_of(st.prov, fa.value))
        st = field_store(st, fa.value, last)
      }
      st = set_result(st, result, .Unique)
      set_prov_st(st, result, origins)
    },
    .AVariant(_, _, args) => {
      origins: Vector<Int> = []
      for a in args {
        origins = union_sorted(origins, prov_of(st.prov, a))
        st = field_store(st, a, last)
      }
      st = set_result(st, result, .Unique)
      set_prov_st(st, result, origins)
    },
    .AArrayLit(elems) => {
      origins: Vector<Int> = []
      for a in elems {
        origins = union_sorted(origins, prov_of(st.prov, a))
        st = field_store(st, a, last)
      }
      st = set_result(st, result, .Unique)
      set_prov_st(st, result, origins)
    },
    .ARecordGet(base, _, _) => {
      st = set_result(st, result, .Unknown)
      set_prov_st(st, result, prov_of(st.prov, base))
    },
    .AIndex(base, _, _, _) => {
      st = set_result(st, result, .Unknown)
      set_prov_st(st, result, prov_of(st.prov, base))
    },
    .ARecordUpdate(base, _, v, _, _) => {
      origins := union_sorted(prov_of(st.prov, base), prov_of(st.prov, v))
      st = consume_base(st, result, base, last)
      st = field_store(st, v, last)
      set_prov_st(st, result, origins)
    },
    .AInit(a) => {
      st = init_hinge(st, result, a, last)
      set_prov_st(st, result, prov_of(st.prov, a))
    },
    .AAssign(local, a) => {
      o := fact_of(st.own, a)
      st = set_own_st(st, local.id, o)
      st = set_prov_st(st, local.id, prov_of(st.prov, a))
      set_valid(st, local.id, true)
    },
    .AWrapAnyref(a, _) => {
      st = init_hinge(st, result, a, last)
      set_prov_st(st, result, prov_of(st.prov, a))
    },
    .AUnwrapAnyref(a, _) => {
      st = init_hinge(st, result, a, last)
      set_prov_st(st, result, prov_of(st.prov, a))
    },
```

`AGlobalSet`/`AMakeClosure` keep publishing operands (now transitive) and set result `Unknown` (empty prov). `ACall` prov is set in Task 4's `transfer_call`; leave empty here.

- [ ] **Step 7: Thread `prov` through the fixpoint + seed params**

Add the prov twins (`prov_map_get`, `same_prov_map` — reuse `same_live` for the inner vector compare, `join_entry_prov`, `prov_of_in`, `seed_param_prov`) exactly as in the design; extend `FixResult` with `exit_prov` and `run_fixpoint` to (a) take the function `params`, (b) seed block 0's entry prov via `seed_param_prov`, (c) join entry prov via `join_entry_prov`, (d) compare prov in change-detection, (e) store `exit_prov[blk] = st.prov`.

```tw
fn prov_map_get(m: Dict<Int, Dict<Int, Vector<Int>>>, id: Int) Dict<Int, Vector<Int>> {
  case m.get(id) {
    .Some(v) => v,
    .None => Dict.new(),
  }
}

fn same_prov_map(a: Dict<Int, Vector<Int>>, b: Dict<Int, Vector<Int>>) Bool {
  if a.keys().len() != b.keys().len() {
    return false
  }
  for k in a.keys() {
    case a.get(k) {
      .Some(av) => case b.get(k) {
        .Some(bv) => if !same_live(av, bv) {
          return false
        },
        .None => return false,
      },
      .None => {},
    }
  }
  true
}

fn prov_of_in(pv: Dict<Int, Vector<Int>>, a: Atom) Vector<Int> {
  case atom_local_id(a) {
    .Some(id) => case pv.get(id) {
      .Some(v) => v,
      .None => [],
    },
    .None => [],
  }
}

fn join_entry_prov(blk: CfgBlock, exit_prov: Dict<Int, Dict<Int, Vector<Int>>>, processed: Dict<Int, Bool>) Dict<Int, Vector<Int>> {
  entry: Dict<Int, Vector<Int>> = Dict.new()
  for lid in blk.entry.live {
    pidx := param_index(blk, lid)
    acc: Vector<Int> = []
    for pe in blk.preds {
      if is_processed(processed, pe.target.id) {
        pv := prov_map_get(exit_prov, pe.target.id)
        src := case pidx {
          .Some(i) => if i < pe.args.len() {
            prov_of_in(pv, pe.args[i])
          } else {
            []
          },
          .None => case pv.get(lid) {
            .Some(v) => v,
            .None => [],
          },
        }
        acc = union_sorted(acc, src)
      }
    }
    if acc.len() > 0 {
      entry[lid] = acc
    }
  }
  entry
}

fn seed_param_prov(entry: Dict<Int, Vector<Int>>, params: Vector<LocalId>) Dict<Int, Vector<Int>> {
  for p in params {
    entry[p.id] = [p.id]
  }
  entry
}
```

`run_fixpoint(blocks, params, table, b, sem)` per-block step (mirrors the own/valid handling):

```tw
      entry_prov := join_entry_prov(blk, exit_prov, processed)
      if blk.id.id == 0 {
        entry_prov = seed_param_prov(entry_prov, params)
      }
      st := ForwardState.{ own: entry_own, valid: entry_valid, prov: entry_prov }
      st = forward_block(blk, st, table, b, sem)
      if !already
        or !same_own_map(own_map_get(exits, blk.id.id), st.own)
        or !same_valid_map(valid_map_get(exit_valid, blk.id.id), st.valid)
        or !same_prov_map(prov_map_get(exit_prov, blk.id.id), st.prov) {
        changed = true
        exits[blk.id.id] = st.own
        exit_valid[blk.id.id] = st.valid
        exit_prov[blk.id.id] = st.prov
      }
```

`FixResult = .{ exits, exit_valid, exit_prov }`. `run_fixpoint`, `forward_block`, `ownership_stage`, `analyze_function`, `transfer_op`, `transfer_call` all gain the `table: SummaryTable` param (threaded; `transfer_call` ignores it until Task 4). `analyze_function`'s materialize `collect` seeds block 0's entry prov before the final `forward_block`. Keep `analyze(view,b,sem)` as the wrapper (G1) calling `analyze_with_summaries(view,b,sem, summary.empty_table())`.

- [ ] **Step 8: `forward_block_body` (no terminator publish)**

```tw
fn forward_block_body(blk: CfgBlock, entry: ForwardState, table: SummaryTable, b: BuiltinRegistry, sem: OptimizerSemantics) ForwardState {
  scan := scan_block_backward(blk, blk.exit.live)
  st := entry
  for inst, i in blk.instructions {
    last := last_use_at(inst.op, scan.live_after[i])
    st = transfer_op(st, inst.anf_local.id, inst.op, last, table, b, sem)
  }
  st
}

fn forward_block(blk: CfgBlock, entry: ForwardState, table: SummaryTable, b: BuiltinRegistry, sem: OptimizerSemantics) ForwardState {
  st := forward_block_body(blk, entry, table, b, sem)
  case blk.terminator {
    .Some(.Return(.Some(a))) => {
      st = publish_atom(st, a)
    },
    .Some(.ValueBreak(a)) => {
      st = publish_atom(st, a)
    },
    _ => {},
  }
  st
}
```

- [ ] **Step 9: `summarize_function` — escape/capability over ALL block exits (return stubbed)**

```tw
fn own_is_shared(own: Dict<Int, Int>, id: Int) Bool {
  case own.get(id) {
    .Some(tag) => tag == 1,
    .None => false,
  }
}

pub fn summarize_function(f: CfgFunction, table: SummaryTable, b: BuiltinRegistry, sem: OptimizerSemantics) Summary {
  live := compute_liveness(f.blocks)
  blocks := collect blk in f.blocks {
    bl := live_get(live, blk.id.id)
    blk.entry.live = bl.live_in
    blk.exit.live = bl.live_out
    blk
  }
  fx := run_fixpoint(blocks, f.params, table, b, sem)

  // Escape/capability: scan EVERY block's post-publish exit (fx.exits/fx.exit_valid).
  // A param published on a dead branch, or wrapped into a returned aggregate
  // (published by the return terminator), shows own==Shared in some block's exit.
  params: Vector<ParamSummary> = collect p in f.params {
    esc: EscapeEffect = .Borrowed
    cap: ParamCapability = .NoCap
    for blk in blocks {
      if own_is_shared(own_map_get(fx.exits, blk.id.id), p.id) {
        esc = .Retained
      }
      case valid_map_get(fx.exit_valid, blk.id.id).get(p.id) {
        .Some(v) => if !v {
          cap = .Consumed
        },
        .None => {},
      }
    }
    ParamSummary.{ escape: esc, capability: cap }
  }

  Summary.{ params, ret: .Shared } // return effect: Task 2
}
```

- [ ] **Step 10: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — borrow/retain/aggregate/dead-branch escape tests **and** all Phase 2 facts tests (G2).

- [ ] **Step 11: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw boot/tests/main.tw
git commit -m "ownership: param provenance + escape/capability classification

Fold a param-origin provenance map into ForwardState (threaded through the
join+fixpoint), make publish transitive over provenance, and classify each
param's escape/capability by scanning every block's post-publish exit — so a
param published on a dead branch or wrapped into a returned aggregate is Retained.
Return effect stubbed Shared (Task 2). analyze stays the empty-table wrapper."
```

---

## Task 2: Return-effect classification

Compute the real return effect (`OwnedFresh` / `MayAliasParams(set)` / `Shared`) from the **body-only** state at return blocks, joined across returns.

**Files:**
- Modify: `boot/compiler/ownership.tw` (`summarize_function`).
- Test: `boot/tests/suites/cfg_summary_suite.tw`.

- [ ] **Step 1: Write the failing tests (payload asserts — blocker 4)**

Add helpers + tests:

```tw
fn ret_tag(r: ownership.ReturnEffect) Int {
  case r {
    .OwnedFresh => 0,
    .MayAliasParams(_) => 1,
    .Shared => 2,
  }
}

// Exact alias set, or [] for non-alias returns.
fn ret_alias_set(r: ownership.ReturnEffect) Vector<Int> {
  case r {
    .MayAliasParams(s) => s,
    _ => [],
  }
}

fn same_ints(a: Vector<Int>, b: Vector<Int>) Bool {
  if a.len() != b.len() {
    return false
  }
  for x, i in a {
    if x != b[i] {
      return false
    }
  }
  true
}

    .test("t2 fresh: fn f(x) { Dict.new() } -> ret fresh", fn() {
      b := b_reg()
      s := summ1("f", 1, .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1)))))
      try assert.equal(ret_tag(s.ret), ret_tag(.OwnedFresh))
      .Ok({})
    })
    .test("t2 alias payload: fn f(x) { x } -> ret alias([0])", fn() {
      s := summ1("f", 1, .Atom(.ALocal(lid(0))))
      try assert.equal(ret_tag(s.ret), ret_tag(.MayAliasParams([])))
      try assert.is_true(same_ints(ret_alias_set(s.ret), [0]))
      .Ok({})
    })
    .test("t2 multi-origin: fn f(x,y) { if c { x } else { y } } -> ret alias([0,1])", fn() {
      then_e: AnfExpr = .Atom(.ALocal(lid(0)))
      else_e: AnfExpr = .Atom(.ALocal(lid(1)))
      body: AnfExpr = .Let(lid(2), .AIf(.ALitBool(true), then_e, else_e), .Atom(.ALocal(lid(2))))
      s := summ1("f", 2, body)
      try assert.is_true(same_ints(ret_alias_set(s.ret), [0, 1]))
      .Ok({})
    })
    .test("t2 wrap return: fn f(x) { Wrapper.{x} } -> ret alias([0])", fn() {
      b := b_reg()
      body: AnfExpr = .Let(lid(1), wrapper_record(lid(0)), .Atom(.ALocal(lid(1))))
      s := summ1("f", 1, body)
      try assert.is_true(same_ints(ret_alias_set(s.ret), [0]))
      .Ok({})
    })
```

- [ ] **Step 2: Run to verify they fail**

Expected: FAIL — return is stubbed `.Shared`.

- [ ] **Step 3: Implement return-effect classification (body-only)**

Add helpers + replace the stub in `summarize_function`:

```tw
fn return_atom(blk: CfgBlock) Atom? {
  case blk.terminator {
    .Some(.Return(.Some(a))) => .Some(a),
    _ => .None,
  }
}

fn param_pos(params: Vector<LocalId>, local_id: Int) Int {
  for p, i in params {
    if p.id == local_id {
      return i
    }
  }
  -1
}

fn prov_to_indices(params: Vector<LocalId>, origins: Vector<Int>) Vector<Int> {
  out: Vector<Int> = []
  for o in origins {
    idx := param_pos(params, o)
    if idx >= 0 {
      out = insert_sorted(out, idx)
    }
  }
  out
}

fn join_return(a: ReturnEffect, b: ReturnEffect) ReturnEffect {
  case a {
    .Shared => .Shared,
    .OwnedFresh => b,
    .MayAliasParams(sa) => case b {
      .Shared => .Shared,
      .OwnedFresh => .MayAliasParams(sa),
      .MayAliasParams(sb) => .MayAliasParams(union_sorted(sa, sb)),
    },
  }
}
```

In `summarize_function`, after building `params`, compute `ret` from body-only return-block state (reuse `join_entry_*` with an all-processed map + `seed_param_prov` for block 0):

```tw
  done := all_processed(blocks)
  ret: ReturnEffect = .OwnedFresh
  seen := false
  for blk in blocks {
    case return_atom(blk) {
      .Some(a) => {
        entry_own := join_entry_ownership(blk, fx.exits, done)
        entry_valid := join_entry_valid(blk, fx.exit_valid, done)
        entry_prov := join_entry_prov(blk, fx.exit_prov, done)
        if blk.id.id == 0 {
          entry_prov = seed_param_prov(entry_prov, f.params)
        }
        st := ForwardState.{ own: entry_own, valid: entry_valid, prov: entry_prov }
        body := forward_block_body(blk, st, table, b, sem)
        idxs := prov_to_indices(f.params, prov_of(body.prov, a))
        r: ReturnEffect = if idxs.len() > 0 {
          .MayAliasParams(idxs)
        } else {
          case fact_of(body.own, a) {
            .Unique => .OwnedFresh,
            _ => .Shared,
          }
        }
        ret = if seen {
          join_return(ret, r)
        } else {
          r
        }
        seen = true
      },
      .None => {},
    }
  }
  Summary.{ params, ret }
```

(Replace the Task 1 `Summary.{ params, ret: .Shared }` tail with this.)

- [ ] **Step 4: Run tests to verify they pass**

Expected: PASS — fresh / alias([0]) / multi-origin([0,1]) / wrap alias([0]); Phase 2 green.

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "ownership: return-effect classification (body-only, multi-origin)

Classify each function's return from the body-only state at return blocks:
MayAliasParams(origins) when the return atom carries param provenance (full set),
OwnedFresh for a genuinely fresh Unique, else Shared; joined across return blocks."
```

---

## Task 3: Call graph + SCC fixpoint driver (`summary.tw`)

Compute the whole-program `SummaryTable` bottom-up over call-graph SCCs.

**Files:**
- Modify: `boot/compiler/summary.tw`.
- Test: `boot/tests/suites/cfg_summary_suite.tw`.

- [ ] **Step 1: Write the failing tests**

```tw
fn compute_of(funcs: Vector<AnfFunctionDef>) ownership.SummaryTable {
  b := b_reg()
  v := cfg.build_view(module_of(funcs), b)
  summary.compute(v, b, sem())
}

fn summ_of(t: ownership.SummaryTable, func_id: Int) ownership.Summary {
  case ownership.summary_get(t, func_id) {
    .Some(s) => s,
    .None => error("no summary ${func_id}"),
  }
}

    .test("t3 leaf summaries computed for every function", fn() {
      b := b_reg()
      f := fdef(1, "f", 1, .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1)))))
      g := fdef(2, "g", 1, .Atom(.ALocal(lid(0))))
      t := compute_of([f, g])
      try assert.equal(escape_tag(summ_of(t, 1).params[0].escape), escape_tag(.Borrowed))
      try assert.equal(ret_tag(summ_of(t, 1).ret), ret_tag(.OwnedFresh))
      try assert.is_true(same_ints(ret_alias_set(summ_of(t, 2).ret), [0]))
      .Ok({})
    })
    .test("t3 recursion terminates: fn f(x){ f(x) }", fn() {
      call_self: AnfExpr = .Let(
        lid(1),
        .ACall(.AGlobalFunc(FuncId.{ id: 1 }), [.ALocal(lid(0))]),
        .Atom(.ALocal(lid(1))),
      )
      t := compute_of([fdef(1, "f", 1, call_self)])
      try assert.equal(t.by_func.keys().len(), 1)
      .Ok({})
    })
```

- [ ] **Step 2: Run to verify it fails**

Expected: FAIL — no `summary.compute`.

- [ ] **Step 3: Implement the driver**

First inspect the SCC API: `grep -n "pub fn\|pub type" boot/compiler/graph_scc.tw`. Then in `summary.tw` implement `compute` per the design: build user-id set, seed each function's summary conservatively (`params Retained`, `ret Shared`), order by call-graph SCCs (callee-first), and for each SCC iterate `ownership.summarize_function` until stable or a `members*4+1` cap, replacing changed summaries. Provide `conservative_summary`, `same_summary` (compare escape/cap tags + return tag + alias-set), `callee_ids` (scan `ACall(AGlobalFunc(fid))` for user ids), `func_by_id`, and `order_sccs` (adjacency `fid → callee_ids`, Tarjan via `graph_scc.tw`, callee-first order). Full code as in the design's Task-2 sketch; adapt `order_sccs` to the real `graph_scc` API.

- [ ] **Step 4: Run tests to verify they pass**

Expected: PASS — leaf summaries + recursion terminates; Phase 2 green.

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "summary: whole-program call-graph SCC fixpoint (compute)"
```

---

## Task 4: Consume summaries in `transfer_call`

Make call sites read the callee summary so borrowed args stay `Unique`, fresh returns become `Unique`, and retained/aliased origins demote.

**Files:**
- Modify: `boot/compiler/ownership.tw`.
- Test: `boot/tests/suites/cfg_summary_suite.tw`.

- [ ] **Step 1: Write the failing tests (concrete fixtures — blocker 3)**

Add the caller-analysis helper and all consumption fixtures:

```tw
fn analyzed_caller(funcs: Vector<AnfFunctionDef>, caller: String) cfg.CfgFunction {
  b := b_reg()
  v := cfg.build_view(module_of(funcs), b)
  t := summary.compute(v, b, sem())
  a := ownership.analyze_with_summaries(v, b, sem(), t)
  case cfg.function_named(a, caller) {
    .Some(f) => f,
    .None => error("no ${caller}"),
  }
}

// exit ownership of a local in the caller's entry block (block 0).
fn caller_own(f: cfg.CfgFunction, local_id: Int) Int {
  case f.blocks[0].exit.ownership.get(local_id) {
    .Some(tag) => tag,
    .None => 2,
  }
}

fn own_unique() Int {
  ownership.own_tag(.Unique)
}

fn own_shared() Int {
  ownership.own_tag(.Shared)
}

    .test("t4 borrow: borrowing callee leaves caller arg Unique", fn() {
      b := b_reg()
      // g(y) { Dict.new() }  borrows y, returns fresh
      g := fdef(2, "g", 1, .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1)))))
      // f() { a := Dict.new(); r := g(a); 0 }   (literal tail so a's fact is observable)
      f_body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), .ACall(.AGlobalFunc(FuncId.{ id: 2 }), [.ALocal(lid(0))]), .Atom(.ALitInt(0))),
      )
      f := analyzed_caller([fdef(1, "f", 0, f_body), g], "f")
      try assert.equal(caller_own(f, 0), own_unique())
      .Ok({})
    })
    .test("t4 retain: retaining callee demotes caller arg to Shared", fn() {
      b := b_reg()
      // g(y) { global_set G = y; 0 }  retains y
      g := fdef(2, "g", 1, .Let(lid(1), .AGlobalSet(GlobalId.{ id: 0 }, .ALocal(lid(0))), .Atom(.ALitInt(0))))
      f_body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), .ACall(.AGlobalFunc(FuncId.{ id: 2 }), [.ALocal(lid(0))]), .Atom(.ALitInt(0))),
      )
      f := analyzed_caller([fdef(1, "f", 0, f_body), g], "f")
      try assert.equal(caller_own(f, 0), own_shared())
      .Ok({})
    })
    .test("t4 return-alias: aliasing callee demotes the origin arg", fn() {
      b := b_reg()
      // g(y) { y }  returns alias of y
      g := fdef(2, "g", 1, .Atom(.ALocal(lid(0))))
      f_body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), .ACall(.AGlobalFunc(FuncId.{ id: 2 }), [.ALocal(lid(0))]), .Atom(.ALitInt(0))),
      )
      f := analyzed_caller([fdef(1, "f", 0, f_body), g], "f")
      try assert.equal(caller_own(f, 0), own_shared())
      .Ok({})
    })
    .test("t4 aggregate wrapper: wrap(xs) demotes the arg", fn() {
      b := b_reg()
      // g(y) { Wrapper.{y} }
      g := fdef(2, "g", 1, .Let(lid(1), wrapper_record(lid(0)), .Atom(.ALocal(lid(1)))))
      f_body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), .ACall(.AGlobalFunc(FuncId.{ id: 2 }), [.ALocal(lid(0))]), .Atom(.ALitInt(0))),
      )
      f := analyzed_caller([fdef(1, "f", 0, f_body), g], "f")
      try assert.equal(caller_own(f, 0), own_shared())
      .Ok({})
    })
    .test("t4 multi-origin: both origin args demoted", fn() {
      b := b_reg()
      // g(y, z) { if c { y } else { z } }  returns alias of y or z
      then_e: AnfExpr = .Atom(.ALocal(lid(0)))
      else_e: AnfExpr = .Atom(.ALocal(lid(1)))
      g := fdef(2, "g", 2, .Let(lid(2), .AIf(.ALitBool(true), then_e, else_e), .Atom(.ALocal(lid(2)))))
      f_body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(
          lid(1),
          dict_new_call(b),
          .Let(lid(2), .ACall(.AGlobalFunc(FuncId.{ id: 2 }), [.ALocal(lid(0)), .ALocal(lid(1))]), .Atom(.ALitInt(0))),
        ),
      )
      f := analyzed_caller([fdef(1, "f", 0, f_body), g], "f")
      try assert.equal(caller_own(f, 0), own_shared())
      try assert.equal(caller_own(f, 1), own_shared())
      .Ok({})
    })
    .test("t4 fresh result is Unique at the call", fn() {
      b := b_reg()
      g := fdef(2, "g", 1, .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1)))))
      // f() { a := Dict.new(); r := g(a); 0 }  -> r is Unique (fresh return)
      f_body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), .ACall(.AGlobalFunc(FuncId.{ id: 2 }), [.ALocal(lid(0))]), .Atom(.ALitInt(0))),
      )
      f := analyzed_caller([fdef(1, "f", 0, f_body), g], "f")
      try assert.equal(caller_own(f, 1), own_unique())
      .Ok({})
    })
    .test("t4 capability recorded, not acted on: consumed arg stays valid", fn() {
      b := b_reg()
      // g(y) { z := <consume y via Dict.set>; z }  (y last-used, moved) -> capability Consumed, escape Borrowed
      g_body: AnfExpr = .Let(
        lid(1),
        .ACall(.AGlobalFunc(b.method_id("Dict", "set")), [.ALocal(lid(0)), .ALitInt(1), .ALitInt(2)]),
        .Atom(.ALitInt(0)),
      )
      g := fdef(2, "g", 1, g_body)
      f_body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), .ACall(.AGlobalFunc(FuncId.{ id: 2 }), [.ALocal(lid(0))]), .Atom(.ALocal(lid(0)))),
      )
      f := analyzed_caller([fdef(1, "f", 0, f_body), g], "f")
      // a's binding stays valid at the caller (Consumed is not acted on in Phase 3).
      case f.blocks[0].exit.binding_valid.get(0) {
        .Some(v) => try assert.is_true(v),
        .None => {}, // absent => valid
      }
      .Ok({})
    })
    .test("t4 conservative: unknown callee publishes the arg", fn() {
      b := b_reg()
      // call FuncId 99 which is NOT in the module -> no summary -> conservative publish
      f_body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), .ACall(.AGlobalFunc(FuncId.{ id: 99 }), [.ALocal(lid(0))]), .Atom(.ALitInt(0))),
      )
      f := analyzed_caller([fdef(1, "f", 0, f_body)], "f")
      try assert.equal(caller_own(f, 0), own_shared())
      .Ok({})
    })
```

- [ ] **Step 2: Run to verify they fail**

Expected: FAIL — with no consumption, a borrowing call still publishes the arg → borrow test expects Unique but gets Shared, and the fresh-result test gets Unknown.

- [ ] **Step 3: Implement consumption in `transfer_call`**

Replace the user-call handling; keep builtins + the `.None` bucket:

```tw
fn transfer_call(st: ForwardState, result: Int, callee: Atom, args: Vector<Atom>, last: Vector<Int>, table: SummaryTable, sem: OptimizerSemantics) ForwardState {
  case callee_func_id(callee) {
    .Some(fid) => case call_info(sem, fid) {
      .Some(cs) => transfer_builtin_call(st, result, cs, args, last),
      .None => case summary_get(table, fid.id) {
        .Some(s) => transfer_summarized_call(st, result, s, args),
        .None => publish_call(st, result, args),
      },
    },
    .None => publish_call(st, result, args),
  }
}

fn publish_call(st: ForwardState, result: Int, args: Vector<Atom>) ForwardState {
  for a in args {
    st = publish_atom(st, a)
  }
  set_own_st(st, result, .Unknown)
}

fn transfer_builtin_call(st: ForwardState, result: Int, cs: CallSemantics, args: Vector<Atom>, last: Vector<Int>) ForwardState {
  case cs.effect {
    .Allocate => set_own_st(st, result, .Unique),
    .Update => consume_call_base(st, result, cs, args, last),
    .ReadOnly => set_own_st(st, result, .Unknown),
    .Pure => set_own_st(st, result, .Unknown),
    .Control => set_own_st(st, result, .Unknown),
  }
}

fn transfer_summarized_call(st: ForwardState, result: Int, s: Summary, args: Vector<Atom>) ForwardState {
  for ps, i in s.params {
    if i < args.len() {
      case ps.escape {
        .Retained => st = publish_atom(st, args[i]),
        .Borrowed => {},
      }
    }
  }
  case s.ret {
    .OwnedFresh => set_own_st(st, result, .Unique),
    .Shared => set_own_st(st, result, .Unknown),
    .MayAliasParams(idxs) => {
      origins: Vector<Int> = []
      for k in idxs {
        if k < args.len() {
          st = publish_atom(st, args[k])
          origins = union_sorted(origins, prov_of(st.prov, args[k]))
        }
      }
      st = set_own_st(st, result, .Shared)
      set_prov_st(st, result, origins)
    },
  }
}
```

(`consume_call_base`, `call_info`, `CallSemantics` are the existing Phase 2 pieces; `summary_get` from Task 1.)

- [ ] **Step 4: Run tests to verify they pass**

Expected: PASS — borrow stays Unique; retain/alias/aggregate/multi-origin demote; fresh result Unique; capability not acted on; unknown conservative. Phase 2 green.

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "ownership: consume summaries in transfer_call

Direct user calls with a summary: Borrowed args unchanged, Retained args publish,
OwnedFresh -> Unique result, MayAliasParams -> publish every origin + Shared
result. Consumed is recorded only. Builtins keep CallSemantics; unknown/extern/
indirect keep the conservative publish bucket."
```

---

## Task 5: Fold the summary into `--cfg` + wire the CLI

**Files:**
- Modify: `boot/compiler/summary.tw` (renderer), `boot/compiler/cfg.tw` (`render_view_with_headers`), `boot/commands/ir.tw`.
- Test: `boot/tests/suites/cfg_summary_suite.tw`.

- [ ] **Step 1: Write the failing test**

```tw
    .test("t5 render: --cfg header shows the summary + determinism", fn() {
      b := b_reg()
      g := fdef(2, "g", 1, .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1)))))
      v := cfg.build_view(module_of([fdef(1, "f", 0, .Atom(.ALitInt(0))), g]), b)
      t := summary.compute(v, b, sem())
      out1 := summary.render_cfg(v, t)
      out2 := summary.render_cfg(v, t)
      try assert.is_true(out1.contains("summary:"))
      try assert.is_true(out1 == out2)
      .Ok({})
    })
```

- [ ] **Step 2: Run to verify it fails**

Expected: FAIL — no `summary.render_cfg` / no `cfg.render_view_with_headers`.

- [ ] **Step 3: Header renderer in `summary.tw`**

```tw
pub fn render_summary(s: Summary) String {
  parts: Vector<String> = []
  for ps, i in s.params {
    esc := case ps.escape {
      .Borrowed => "borrow",
      .Retained => "retain",
    }
    tag := case ps.capability {
      .Consumed => "${esc}(consumed)",
      .NoCap => esc,
    }
    parts = .append("p${i}=${tag}")
  }
  ret := case s.ret {
    .OwnedFresh => "fresh",
    .Shared => "shared",
    .MayAliasParams(idxs) => {
      ps := collect k in idxs {
        "p${k}"
      }
      "alias(${ps.join(",")})"
    },
  }
  "summary: ${parts.join(" ")} ret=${ret}"
}

pub fn render_cfg(view: CfgView, table: SummaryTable) String {
  headers: Dict<Int, String> = Dict.new()
  for f in view.functions {
    case ownership.summary_get(table, f.func_id) {
      .Some(s) => headers[f.func_id] = render_summary(s),
      .None => {},
    }
  }
  cfg.render_view_with_headers(view, headers)
}
```

- [ ] **Step 4: `render_view_with_headers` in `cfg.tw`**

Add `pub fn render_view_with_headers(view: CfgView, headers: Dict<Int, String>) String` — identical to `render_view` but each `fn …` header line appends, on the next indented line, `case headers.get(func.func_id) { .Some(h) => "  ${h}", .None => "" }` (skip the empty line when absent). `cfg.tw` gains **no** `ownership`/`summary` import (G6).

- [ ] **Step 5: Wire `twk ir --cfg`**

In `boot/commands/ir.tw` `--cfg` branch:

```tw
  if parsed.has_flag("cfg") {
    b := artifacts.builtins
    view := cfg.build_view(artifacts.opt, b)
    view = ownership.prune_dead_merge(view)   // enabled in Task 6; until then omit this line
    s := semantics.make_prelude_optimizer_semantics(b)
    table := summary.compute(view, b, s)
    analyzed := ownership.analyze_with_summaries(view, b, s, table)
    print(summary.render_cfg(analyzed, table))
    return
  }
```

Add `use compiler.summary`. Leave the `prune_dead_merge` line omitted until Task 6.

- [ ] **Step 6: Boot tests, rebuild CLI, smoke + determinism**

```bash
target/twk run boot/tests/main.tw
make bundle-cli
printf 'fn helper(xs: Vector<Int>) Int {\n  xs.len()\n}\nfn top() Int {\n  a := [1, 2, 3]\n  helper(a)\n}\n' > /tmp/sum.tw
target/twk ir /tmp/sum.tw --cfg | grep -E "summary:|facts" | head
target/twk ir /tmp/sum.tw --cfg > /tmp/a.txt
target/twk ir /tmp/sum.tw --cfg > /tmp/b.txt
diff /tmp/a.txt /tmp/b.txt && echo DETERMINISTIC
```

Expected: PASS; header shows `summary:`; `diff` clean.

- [ ] **Step 7: Format, lint, commit**

```bash
target/twk fmt boot/compiler/cfg.tw boot/compiler/summary.tw boot/commands/ir.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/cfg.tw boot/compiler/summary.tw boot/commands/ir.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "cfg/ir: fold function summaries into twk ir --cfg"
```

---

## Task 6: Dead-merge param pruning (minimal, pre-analysis)

**Files:**
- Modify: `boot/compiler/ownership.tw` (`prune_dead_merge`), `boot/commands/ir.tw` (enable the line).
- Test: `boot/tests/suites/cfg_ownership_facts_suite.tw`.

- [ ] **Step 1: Write the failing test**

```tw
    .test("phase3 prune: a dead carried join param is pruned; arity stays consistent", fn() {
      b := b_reg()
      then_e: AnfExpr = .Atom(.ALitInt(1))
      else_e: AnfExpr = .Atom(.ALitInt(2))
      body: AnfExpr = .Let(lid(0), .AIf(.ALitBool(true), then_e, else_e), .Atom(.ALitInt(0)))
      v := cfg.build_view(module_of("f", body), b_reg())
      pruned := ownership.prune_dead_merge(v)
      case cfg.function_named(pruned, "f") {
        .Some(f) => {
          jb := case block_named(f, "if.join") {
            .Some(x) => x,
            .None => return assert.fail("no if.join"),
          }
          try assert.equal(jb.params.len(), 0)
          try assert.equal(cfg.count_edge_arity_mismatches(f), 0)
          .Ok({})
        },
        .None => assert.fail("no f"),
      }
    })
```

(`module_of` here is the Phase 2 single-function helper already in `cfg_ownership_facts_suite.tw`.)

- [ ] **Step 2: Run to verify it fails**

Expected: FAIL — no `prune_dead_merge` / join still carries the dead param.

- [ ] **Step 3: Implement `prune_dead_merge`**

In `ownership.tw`, per the design: compute liveness; per block, the param positions to drop are those whose param local is not in `live_in`; rebuild each block dropping those params, and drop the positionally-aligned atom in every edge targeting a pruned block and in the terminator payload (keep `succs`/terminator identical). Provide `drop_positions_local`, `drop_positions_atom`, `prune_terminator`. Removing a dead param cannot change other locals' liveness, so no re-liveness is needed.

- [ ] **Step 4: Enable in `ir.tw`**

Uncomment `view = ownership.prune_dead_merge(view)` (Task 5 Step 5).

- [ ] **Step 5: Run tests + CLI smoke**

`target/twk run boot/tests/main.tw` (PASS); `make bundle-cli`; re-run the `--cfg` determinism diff.

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/commands/ir.tw boot/tests/suites/cfg_ownership_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/commands/ir.tw boot/tests/suites/cfg_ownership_facts_suite.tw
git commit -m "ownership: minimal pre-analysis dead-merge param pruning"
```

---

## Task 7: Match-arm pattern-binding precision (retire G3)

**Files:**
- Modify: `boot/compiler/cfg.tw` (`CfgBlock.bound`, `build_match`), `boot/compiler/ownership.tw` (`scan_block_backward` kills `bound`).
- Test: `boot/tests/suites/cfg_ownership_facts_suite.tw`.

- [ ] **Step 1: Write the failing test**

```tw
    .test("phase3 patbind: pattern-bound local is killed at arm entry (not live-in)", fn() {
      b := b_reg()
      arm: AnfMatchArm = AnfMatchArm.{
        pattern: .Variant(TypeId.{ id: 0 }, VariantId.{ id: 0 }, [.Var(lid(5))]),
        body: .Let(lid(6), .AInit(.ALocal(lid(5))), .Atom(.ALitInt(0))),
      }
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(7), .AMatch(.ALocal(lid(0)), [arm]), .Atom(.ALitInt(0))),
      )
      f := analyzed_func(module_of("f", body))
      blk := case block_named(f, "match.arm.0") {
        .Some(x) => x,
        .None => return assert.fail("no match.arm.0"),
      }
      try assert.is_false(live_contains(blk.entry.live, 5))
      .Ok({})
    })
```

- [ ] **Step 2: Run to verify it fails**

Expected: FAIL — L5 leaks into `arm.entry.live` (Phase 2 G3).

- [ ] **Step 3: Add `CfgBlock.bound`; populate in `build_match`**

`cfg.tw`: add `bound: Vector<Int>` to `CfgBlock` (default `[]` in `empty_block`). In `build_match`, populate each arm block's `bound` from the arm pattern via a new `pattern_binding_ids(arm.pattern)` (wrap `anf_analysis.collect_pattern_bindings` → sorted `Vector<Int>`) and a `set_block_bound(ctx, id, bound)` helper.

- [ ] **Step 4: Kill `bound` in `scan_block_backward`**

`ownership.tw`, at the end of `scan_block_backward` before returning:

```tw
  cur = diff_sorted(cur, blk.bound)
  BlockScan.{ live_in: cur, live_after }
```

(Absent locals already read Unknown/∅ in the forward pass, so no forward-seed change is needed.)

- [ ] **Step 5: Run tests to verify they pass**

Expected: PASS — L5 not live-in; the earlier G3 "no trap / never Unique" fixture still holds; Phase 2 green.

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/cfg.tw boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/cfg.tw boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
git commit -m "cfg/ownership: match-arm pattern-binding precision (retire Phase 2 G3)"
```

---

## Task 8: Doc + audit hygiene, tracking, full verification

**Files:**
- Modify: `boot/compiler/opt/README.md`, `docs/plans/sound-uniqueness/README.md`.

- [ ] **Step 1: Optimizer audit**

```bash
grep -rniE "uniqueness|liveness|ownership|in.?place|cow|reuse" boot/compiler/opt/*.tw | grep -v README
```

Expected: only `semantics.tw` (builtin `CallSemantics`) and comments — **no** independent liveness/ownership/legality pass. Record the command + result in the commit body and `opt/README.md`.

- [ ] **Step 2: Rewrite `opt/README.md`**

Replace the stale content (it still documents deleted `uniqueness.tw`/`liveness.tw`/`loop_builder.tw`/builder-region and "no CFG is constructed"). Document the **current** pipeline (`defer_elim` + fixed-point `dead_let`/`copy_prop`/`const_fold`/`branch_simp`), the support modules (`analysis.tw`/`use_count.tw`/`semantics.tw`), and a "CFG ownership facts" note pointing at `cfg.tw`/`ownership.tw`/`summary.tw`. Record the **peephole decision**: these peepholes stay ANF-local because they consult no ownership/control-flow facts.

- [ ] **Step 3: `--census` still shows 0 in-place**

```bash
printf 'fn build() Dict<Int, Int> {\n  d := Dict.new()\n  d[1] = 2\n  d[3] = 4\n  d\n}\n' > /tmp/cen.tw
target/twk ir /tmp/cen.tw --census
```

Expected: `dict_set` candidates ≥ 1, **in_place = 0**.

- [ ] **Step 4: Mark the README bullets**

In `docs/plans/sound-uniqueness/README.md` Phase 3 section, mark delivered: summaries, the two precision bullets, and the "move queries"/peephole bullet (reframed — add "Done — CFG facts already the single source; optimizer audited"). Leave genuinely-deferred items unchecked.

- [ ] **Step 5: Full verification**

```bash
make boot-test
make stage2
```

Expected: boot suites green; `make stage2` reaches `stage3 == stage4`.

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk lint boot/main.tw
git add boot/compiler/opt/README.md docs/plans/sound-uniqueness/README.md
git commit -m "opt/docs: rewrite stale opt/README, optimizer audit, track Phase 3"
```

---

## Self-review

**1. Spec coverage** (against `phase3-design.md`):
- Types + provenance + transitive publish + escape/capability (all-blocks scan) → Task 1. ✓ (fixes aggregate-escape blocker 1 and dead-branch blocker 2 via post-publish all-block scan)
- Return effect (body-only, multi-origin `MayAliasParams`, payloads) → Task 2. ✓
- SCC fixpoint (conservative seed, cap) → Task 3. ✓
- Consumption in `transfer_call`; borrow stays Unique; unknown/extern/indirect conservative; capability recorded-not-acted → Task 4. ✓ (concrete fixtures, blocker 3; payload asserts, blocker 4)
- Fold summary into `--cfg` + determinism → Task 5. ✓
- Dead-merge pruning pre-analysis → Task 6. ✓
- Pattern-binding precision → Task 7. ✓
- `opt/README.md` rewrite + audit + peephole + census-0 + tracking + `make stage2` → Task 8. ✓

**2. Placeholder scan:** Task 3 Step 3 and Task 6 Step 3 describe the driver/pruning helpers by contract and point at the design's full code rather than repeating it; the novel/tricky code (provenance, classification, consumption, rendering) is shown in full. No "TBD"/"add error handling".

**3. Type consistency:** `Summary`/`ParamSummary`/`EscapeEffect`/`ParamCapability`/`ReturnEffect`/`SummaryTable`/`summary_get`/`summarize_function`/`analyze_with_summaries`/`render_view_with_headers` used consistently across tasks; `ForwardState` gains `prov` in Task 1 and every construction updated; escape reads `fx.exits` (post-publish), return reads `forward_block_body` (pre-publish).

**Open risks to watch during execution:**
- **Task 1 threads `table` + `prov` through many signatures** (`run_fixpoint`/`forward_block`/`ownership_stage`/`analyze_function`/`transfer_op`/`transfer_call`). Grep and update all call sites in the same task to keep it compiling.
- **G3 direction:** escape from post-publish exits, return from body-only. Swapping them reintroduces blockers 1/2 or makes all returns `Shared`.
- **G6:** never import `ownership`/`summary` into `cfg.tw`; pass headers as a `Dict`.
- **Determinism:** `prov` sorted `Vector<Int>`; SCC order from Tarjan over deterministically-enumerated edges; `--cfg` gated by the Task 5 determinism test.
