# Phase 3 Minimal Summaries — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the Phase 2 intraprocedural ownership analysis a first layer of whole-program **function summaries** so a call to a known helper stops being a blanket publication boundary — improving `twk ir --cfg` facts — plus two facts-precision items and doc/audit hygiene. Analysis-only; no codegen.

**Architecture:** Fold **param-provenance** into the Phase 2 forward `ForwardState` (a third fact threaded through the same join+fixpoint as ownership/validity). A new `summary.tw` extracts the whole-program call graph, orders it with the existing Tarjan SCC util, and computes a `SummaryTable` bottom-up (conservative seed, iterate to a cap). Summaries are consumed in `transfer_call` so borrowed args stay `Unique`, fresh returns become `Unique`, and retained/aliased origins demote to `Shared`. Two precision items (dead-merge param pruning, match-arm pattern-binding kills) and hygiene (rewrite the stale `opt/README.md`, optimizer audit) round it out.

**Tech Stack:** Twinkle (`.tw`), boot self-hosted compiler, `@std.testing` runner, hand-built multi-function `AnfModule` fixtures (stable `LocalId`/`FuncId`, no optimizer), `compiler.opt.semantics` (`call_info`), `compiler.graph_scc` (Tarjan), `make bundle-cli` for the CLI.

**Design spec (read before starting):** `docs/plans/sound-uniqueness/phase3-design.md` — the canonical design this plan implements. Every rule below traces to it. Supporting: `docs/plans/sound-uniqueness/summary-specialization.md` (summary lattice + SCC fixpoint), `docs/plans/sound-uniqueness/README.md` (Phase 3 bullets).

---

## File structure

- **Modify** `boot/compiler/ownership.tw` — summary *types*; `ForwardState.prov`; provenance propagation in `transfer_op`; transitive `publish_local`; a body-only forward (`forward_block_body`); `prov` threaded through `run_fixpoint`/joins (`FixResult.exit_prov`, `join_entry_prov`); per-function classification (`summarize_function`); call-site consumption in `transfer_call`; an `analyze_with_summaries` entry (keep the 3-arg `analyze` as an empty-table wrapper so Phase 2 call sites are untouched).
- **Create** `boot/compiler/summary.tw` — call-graph extraction, SCC-ordered fixpoint (`compute`), and the per-function `--cfg` header renderer.
- **Modify** `boot/compiler/cfg.tw` — `render_view` prints the summary header (fed a `SummaryTable`); `render_view` gains an overload/param; `CfgBlock.bound` for pattern bindings; `prune_dead_merge` view transform.
- **Modify** `boot/commands/ir.tw` — `--cfg` runs `prune_dead_merge → summary.compute → ownership.analyze_with_summaries → render_view(view, table)`.
- **Create** `boot/tests/suites/cfg_summary_suite.tw` — TDD gate over hand-built multi-function fixtures; register in `boot/tests/main.tw`.
- **Modify** `boot/tests/suites/cfg_ownership_facts_suite.tw` — pattern-binding precision + dead-merge fixtures.
- **Modify** `boot/compiler/opt/README.md` — full rewrite to the current pass set + the ANF-local peephole decision.
- **Modify** `docs/plans/sound-uniqueness/README.md` — mark Phase 3 bullets delivered.

## Conventions (read once)

- **Soundness before coverage.** Any callee without a summary (unknown Twinkle fn, extern, indirect/closure callee, `Cell` op) keeps the Phase 2 conservative publish-all bucket. Never mint `Unique`/`Borrowed` without proof.
- **Two summary axes.** Caller-visible **escape** (`Borrowed`/`Retained`) is the only axis `transfer_call` acts on. **Capability** (`Consumed`) is recorded for later phases and must **never** invalidate a caller binding in Phase 3.
- **Provenance is param-locals.** `prov[local]` = sorted `Vector<Int>` of origin **parameter local ids**. Publication is transitive over `prov`. Aggregates carry the union of their fields' `prov`. Convert to positional indices only when building `MayAliasParams`.
- **Boot gotchas (learned in Phase 2):**
  - Dict `[]`-read returns an `Option` — use `.get(k)` and unwrap; do **not** write `m[k].field`.
  - `for`/assignment are statements — a `case`/`if` arm whose body is a `for` loop must wrap it in a block `{ … }`.
  - `target/twk fmt` reformats aggressively (single-line `.test("…", fn(){…})` may become multi-line). After `fmt`, re-locate anchors before further edits.
  - `target/twk lint boot/main.tw` enforces inherent-method call style (`x.f(a)` over `mod.f(x,a)` when `x`'s type is same-module) and `direct-rebinding`/`record-copy-helper`. Prefer writing in that style; `twk lint boot/main.tw --fix` applies the safe rewrites. A `record-copy-helper` finding means: rebind a field (`r.f = v; r`) instead of rebuilding the record.
- **Determinism.** Iterate blocks/params by id/index order; `prov` and live are sorted `Vector<Int>`; never let `Dict` iteration decide output order. `--cfg` must be byte-identical across two builds.
- **After editing `.tw`:** `target/twk fmt <files>` then `target/twk lint boot/main.tw`.
- **Test the boot suite with:** `target/twk run boot/tests/main.tw`. The CLI flag needs `make bundle-cli`.
- **Test API (real shape).** A suite file exports one `pub fn suite() runner.Suite`, built fluently: `runner.suite("name").test("case", fn() { …; .Ok({}) }).test(…)`. Each `.test` callback returns `Result<Void, String>` ending in `.Ok({})`. `try assert.equal(a, b)` (needs `Eq + Stringify`; compare enums by an `Int` tag), `try assert.is_true(c)`, `try assert.is_false(c)`; `assert.fail(msg)` returns an `Err` (use `return assert.fail(...)` or as a tail). Register a suite in `boot/tests/main.tw` with a `use .suites.<name>` line plus `<name>.suite()` in the run list.
- **Commits.** Short imperative subject; body for non-trivial changes. Add a `Co-Authored-By` trailer only if correct for your session.

---

## Guardrails (read before Task 1)

- **G1 — `analyze` stays the empty-table wrapper.** Phase 2's `pub fn analyze(view, b, sem)` has ~20 call sites (the whole `cfg_ownership_facts_suite`). Do **not** change its signature. Add `pub fn analyze_with_summaries(view, b, sem, table)` and make `analyze` call it with `empty_summary_table()`. Existing facts tests keep passing unchanged (empty table ⇒ Phase 2 behavior).
- **G2 — Phase 2 fixtures must stay green throughout.** Every Phase 2 fixture uses `params: []`, so provenance is empty everywhere and transitive publish is a no-op. After each task run the full suite; a Phase 2 regression means a provenance/threading bug, not a summary-logic bug.
- **G3 — Classify from the body-only state, before terminator publication.** `forward_block` publishes the returned value (`Return(a) → publish a`), so reading a return block's *post-publish* exit would make every return look `Shared`. `summarize_function` must read the return atom's `own`/`prov` from **`forward_block_body`** (instructions only, no terminator publish). Param escape is also read from the body-only state (body publishes like `AGlobalSet` are already applied there).
- **G4 — Consumption lives in one place.** `transfer_call` is used by both `analyze` (facts) and `summarize_function` (via the forward pass). Implement summary consumption once in `transfer_call`; both benefit automatically.
- **G5 — Dead-merge pruning is pre-analysis.** `prune_dead_merge` runs *before* `summary.compute`/`analyze` and returns a new view; the analysis then runs fresh on the pruned view. Never mutate an already-analyzed CFG (stale fact maps).

---

## Task 1: Summary types, provenance, and per-function classification

Add the summary types, fold `prov` into the forward analysis (a third threaded fact), and expose `summarize_function` that classifies one function against a callee table. This is the analysis core; consumption at call sites is Task 3.

**Files:**
- Modify: `boot/compiler/ownership.tw`.
- Create + register: `boot/tests/suites/cfg_summary_suite.tw`; register in `boot/tests/main.tw`.

- [ ] **Step 1: Write the failing test (single-function fixtures)**

Create `boot/tests/suites/cfg_summary_suite.tw`:

```tw
use @std.testing.assert as assert
use @std.testing as runner

use compiler.anf.{AnfExpr, AnfFunctionDef, AnfModule, AnfOp}
use compiler.builtins
use compiler.cfg
use compiler.core_ir.{FuncId, GlobalId, LocalId, Param, TypeId as CoreTypeId}
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

// Build a function def: params are locals 0..nparams-1; body uses higher locals.
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

// Summarize a single function `f` (id 1) with an empty callee table.
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

fn ret_tag(r: ownership.ReturnEffect) Int {
  case r {
    .OwnedFresh => 0,
    .MayAliasParams(_) => 1,
    .Shared => 2,
  }
}

fn p0_escape(s: ownership.Summary) Int {
  escape_tag(s.params[0].escape)
}

pub fn suite() runner.Suite {
  runner
    .suite("cfg summary")
    .test("t1 borrow+fresh: fn f(x) { Dict.new() }", fn() {
      b := b_reg()
      s := summ1("f", 1, .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1)))))
      try assert.equal(p0_escape(s), escape_tag(.Borrowed))
      try assert.equal(ret_tag(s.ret), ret_tag(.OwnedFresh))
      .Ok({})
    })
    .test("t1 returns-alias: fn f(x) { x }", fn() {
      s := summ1("f", 1, .Atom(.ALocal(lid(0))))
      try assert.equal(p0_escape(s), escape_tag(.Borrowed))
      try assert.equal(ret_tag(s.ret), ret_tag(.MayAliasParams([])))
      .Ok({})
    })
    .test("t1 retain: fn f(x) { global_set G0 = x; 0 }", fn() {
      body: AnfExpr = .Let(lid(1), .AGlobalSet(GlobalId.{ id: 0 }, .ALocal(lid(0))), .Atom(.ALitInt(0)))
      s := summ1("f", 1, body)
      try assert.equal(p0_escape(s), escape_tag(.Retained))
      .Ok({})
    })
}
```

Register in `boot/tests/main.tw`: add `use .suites.cfg_summary_suite` and `cfg_summary_suite.suite()` in the run list.

- [ ] **Step 2: Run it to verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — no module `compiler.summary` / no `ownership.summarize_function` / no `ownership.Summary`.

- [ ] **Step 3: Add the summary types to `ownership.tw`**

After the `Ownership` enum:

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

(`summary.empty_table()` lives in `summary.tw`, Task 2; for Task 1 the test calls it — so add a temporary `pub fn empty_table() SummaryTable { SummaryTable.{ by_func: Dict.new() } }` **in `summary.tw`** now. Create `summary.tw` minimally in this task with just that function + imports; the driver arrives in Task 2.)

Create `boot/compiler/summary.tw`:

```tw
//! Phase 3 interprocedural summary driver. Task 1: table constructor only;
//! Task 2 adds call-graph extraction + the SCC fixpoint + the --cfg header.

use compiler.ownership.{SummaryTable}

pub fn empty_table() SummaryTable {
  SummaryTable.{ by_func: Dict.new() }
}
```

- [ ] **Step 4: Add `prov` to `ForwardState` and provenance helpers**

Change `ForwardState`:

```tw
type ForwardState = .{ own: Dict<Int, Int>, valid: Dict<Int, Bool>, prov: Dict<Int, Vector<Int>> }
```

Update **every** `ForwardState.{ … }` construction to include `prov: Dict.new()` (grep `ForwardState.{`). Add helpers near `set_own_st`:

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

- [ ] **Step 5: Make `publish_local` transitive over provenance**

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

(Origins are parameter locals; publishing them directly via `set_own_st` — not `publish_local` — avoids re-recursing.)

- [ ] **Step 6: Propagate provenance in `transfer_op`**

Update these arms (keep the existing own/valid logic; add the `prov` side). Show the full replacements:

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

`AMakeClosure`/`AGlobalSet` keep publishing their operands (now transitive via `publish_local`) and set the result `Unknown` with empty `prov` (no `set_prov_st` needed — absent ⇒ `∅`). `ACall` provenance is set inside `transfer_call` in Task 3; for Task 1 leave the call result with empty `prov`.

- [ ] **Step 7: Thread `prov` through the fixpoint and seed params**

`FixResult` gains an exit-prov map and `run_fixpoint` tracks it in parallel with `own`/`valid`. Add the prov twins:

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

// Entry prov for a block: positional join of predecessor exit prov (params via
// edge args, live-through locals by same id), mirroring join_entry_ownership.
fn join_entry_prov(
  blk: CfgBlock,
  exit_prov: Dict<Int, Dict<Int, Vector<Int>>>,
  processed: Dict<Int, Bool>,
) Dict<Int, Vector<Int>> {
  entry: Dict<Int, Vector<Int>> = Dict.new()
  for lid in blk.entry.live {
    pidx := param_index(blk, lid)
    acc: Vector<Int> = []
    for pe in blk.preds {
      if is_processed(processed, pe.target.id) {
        pv := prov_map_get(exit_prov, pe.target.id)
        src_prov := case pidx {
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
        acc = union_sorted(acc, src_prov)
      }
    }
    if acc.len() > 0 {
      entry[lid] = acc
    }
  }
  entry
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

// Seed a function's parameter locals: each param's prov is itself.
fn seed_param_prov(entry: Dict<Int, Vector<Int>>, params: Vector<LocalId>) Dict<Int, Vector<Int>> {
  for p in params {
    entry[p.id] = [p.id]
  }
  entry
}
```

Extend `FixResult` and `run_fixpoint` to carry `exit_prov`. `run_fixpoint` takes the function's `params` so block 0's entry prov is seeded. In the per-block step:

```tw
      entry_prov := join_entry_prov(blk, exit_prov, processed)
      if blk.id.id == 0 {
        entry_prov = seed_param_prov(entry_prov, params)
      }
      st := ForwardState.{ own: entry_own, valid: entry_valid, prov: entry_prov }
      st = forward_block(blk, st, table, b, sem)
      // change-detection also compares prov:
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

`run_fixpoint` and `forward_block`/`ownership_stage`/`analyze_function` now take the callee `table` param (Task 3 uses it; for Task 1 thread it as `SummaryTable`, unused by `transfer_call` yet — pass `empty_table()` internally where needed). Keep `analyze`'s 3-arg wrapper (G1): `analyze(view,b,sem)` calls `analyze_with_summaries(view,b,sem, summary.empty_table())`.

> Note: `run_fixpoint` currently seeds params nowhere; block 0 has no preds so `join_entry_prov` is empty for it — that's why `seed_param_prov` is applied to block 0. `analyze_function`'s materialize `collect` must likewise seed block 0's entry prov before the final `forward_block`.

- [ ] **Step 8: Add `forward_block_body` (no terminator publish) for classification**

Split the terminator publication out of `forward_block`:

```tw
fn forward_block_body(
  blk: CfgBlock,
  entry: ForwardState,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
) ForwardState {
  scan := scan_block_backward(blk, blk.exit.live)
  st := entry
  for inst, i in blk.instructions {
    last := last_use_at(inst.op, scan.live_after[i])
    st = transfer_op(st, inst.anf_local.id, inst.op, last, table, b, sem)
  }
  st
}

fn forward_block(
  blk: CfgBlock,
  entry: ForwardState,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
) ForwardState {
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

(`transfer_op` and `transfer_call` gain the `table` param now; `transfer_call` ignores it until Task 3.)

- [ ] **Step 9: Implement `summarize_function` (classification)**

```tw
fn is_return_block(blk: CfgBlock) Bool {
  case blk.terminator {
    .Some(.Return(_)) => true,
    _ => false,
  }
}

fn return_atom(blk: CfgBlock) Atom? {
  case blk.terminator {
    .Some(.Return(.Some(a))) => .Some(a),
    _ => .None,
  }
}

// Positional index of a param local, or -1 if not a param.
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

pub fn summarize_function(
  f: CfgFunction,
  table: SummaryTable,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
) Summary {
  live := compute_liveness(f.blocks)
  blocks := collect blk in f.blocks {
    bl := live_get(live, blk.id.id)
    blk.entry.live = bl.live_in
    blk.exit.live = bl.live_out
    blk
  }
  fx := run_fixpoint(blocks, f.params, table, b, sem)
  done := all_processed(blocks)

  // Per param, combine across return blocks using the BODY-ONLY exit state (G3).
  params: Vector<ParamSummary> = collect p in f.params {
    esc: EscapeEffect = .Borrowed
    cap: ParamCapability = .NoCap
    for blk in blocks {
      if is_return_block(blk) {
        entry_own := join_entry_ownership(blk, fx.exits, done)
        entry_valid := join_entry_valid(blk, fx.exit_valid, done)
        entry_prov := join_entry_prov(blk, fx.exit_prov, done)
        if blk.id.id == 0 {
          entry_prov = seed_param_prov(entry_prov, f.params)
        }
        st := ForwardState.{ own: entry_own, valid: entry_valid, prov: entry_prov }
        body := forward_block_body(blk, st, table, b, sem)
        if fact_of_local(body.own, p.id) tag_is_shared() {
          esc = .Retained
        }
        case body.valid.get(p.id) {
          .Some(v) => if !v {
            cap = .Consumed
          },
          .None => {},
        }
      }
    }
    ParamSummary.{ escape: esc, capability: cap }
  }

  // Return effect: join over return blocks, from the body-only state.
  ret: ReturnEffect = .OwnedFresh
  seen_return := false
  for blk in blocks {
    case return_atom(blk) {
      .Some(a) => {
        seen_return = true
        entry_own := join_entry_ownership(blk, fx.exits, done)
        entry_valid := join_entry_valid(blk, fx.exit_valid, done)
        entry_prov := join_entry_prov(blk, fx.exit_prov, done)
        if blk.id.id == 0 {
          entry_prov = seed_param_prov(entry_prov, f.params)
        }
        st := ForwardState.{ own: entry_own, valid: entry_valid, prov: entry_prov }
        body := forward_block_body(blk, st, table, b, sem)
        origins := prov_to_indices(f.params, prov_of(body.prov, a))
        r: ReturnEffect = if origins.len() > 0 {
          .MayAliasParams(origins)
        } else {
          case fact_of(body.own, a) {
            .Unique => .OwnedFresh,
            _ => .Shared,
          }
        }
        ret = if seen_return {
          join_return(ret, r)
        } else {
          r
        }
      },
      .None => {},
    }
  }

  Summary.{ params, ret }
}
```

Replace the pseudo-calls `tag_is_shared()`/`fact_of_local(...) tag_is_shared()` with a real check:

```tw
fn own_is_shared(own: Dict<Int, Int>, id: Int) Bool {
  case own.get(id) {
    .Some(tag) => tag == 1,
    .None => false,
  }
}
```

and use `if own_is_shared(body.own, p.id) { esc = .Retained }`. (Also fix the `seen_return` initial-join: initialize `ret` from the first return, then join subsequent ones — the code above already gates with `seen_return`; ensure the first assignment sets `ret = r` and flips `seen_return` before the join path.)

- [ ] **Step 10: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — the three Task 1 summary tests **and** all Phase 2 facts tests (empty-table `analyze` unchanged; G2).

- [ ] **Step 11: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw boot/tests/main.tw
git commit -m "ownership: param provenance + per-function summary classification

Fold a param-origin provenance map into ForwardState (a third fact threaded
through the same join+fixpoint as ownership/validity), make publish transitive
over provenance, and classify each function's Summary (escape/capability per
param; OwnedFresh/MayAliasParams/Shared return) from the body-only exit state.
No consumption yet; analyze stays the empty-table wrapper (Phase 2 green)."
```

---

## Task 2: Call graph + SCC fixpoint driver (`summary.tw`)

Compute the whole-program `SummaryTable` bottom-up over call-graph SCCs.

**Files:**
- Modify: `boot/compiler/summary.tw`.
- Test: `boot/tests/suites/cfg_summary_suite.tw`.

- [ ] **Step 1: Write the failing tests (multi-function fixtures)**

Add helpers + tests to `cfg_summary_suite.tw`:

```tw
fn compute_of(funcs: Vector<AnfFunctionDef>) ownership.SummaryTable {
  b := b_reg()
  m := module_of(funcs)
  v := cfg.build_view(m, b)
  summary.compute(v, b, sem())
}

fn summ_named(t: ownership.SummaryTable, func_id: Int) ownership.Summary {
  case ownership.summary_get(t, func_id) {
    .Some(s) => s,
    .None => error("no summary for ${func_id}"),
  }
}

    .test("t2 driver: leaf summaries computed for every function", fn() {
      b := b_reg()
      // Fn1 f(x) { Dict.new() } ; Fn2 g(y) { y }
      f := fdef(1, "f", 1, .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1)))))
      g := fdef(2, "g", 1, .Atom(.ALocal(lid(0))))
      t := compute_of([f, g])
      try assert.equal(escape_tag(summ_named(t, 1).params[0].escape), escape_tag(.Borrowed))
      try assert.equal(ret_tag(summ_named(t, 1).ret), ret_tag(.OwnedFresh))
      try assert.equal(ret_tag(summ_named(t, 2).ret), ret_tag(.MayAliasParams([])))
      .Ok({})
    })
    .test("t2 recursion terminates: fn f(x){ f(x) }", fn() {
      // self-recursive; must terminate and yield a sound (conservative) summary.
      call_self: AnfExpr = .Let(
        lid(1),
        .ACall(.AGlobalFunc(FuncId.{ id: 1 }), [.ALocal(lid(0))]),
        .Atom(.ALocal(lid(1))),
      )
      t := compute_of([fdef(1, "f", 1, call_self)])
      // sound: conservative-or-better; the assertion is only that compute returns.
      try assert.is_true(t.by_func.keys().len() == 1)
      .Ok({})
    })
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — no `summary.compute`.

- [ ] **Step 3: Implement the call graph + SCC fixpoint**

Inspect `boot/compiler/graph_scc.tw` for its public API first:

```bash
grep -n "pub fn\|pub type" boot/compiler/graph_scc.tw
```

Then in `summary.tw` (adapt the SCC call to the real API found above):

```tw
use compiler.anf.{AnfExpr, AnfModule, AnfOp, Atom}
use compiler.builtins.{BuiltinRegistry}
use compiler.cfg.{CfgView, CfgFunction}
use compiler.core_ir.{FuncId}
use compiler.opt.semantics.{OptimizerSemantics}
use compiler.ownership.{Summary, SummaryTable, ParamSummary, ReturnEffect, summarize_function}

// Direct user-function callees referenced anywhere in a function's blocks.
fn callee_ids(f: CfgFunction, user_ids: Dict<Int, Bool>) Vector<Int> {
  out: Vector<Int> = []
  for blk in f.blocks {
    for inst in blk.instructions {
      case inst.op {
        .ACall(callee, _) => case callee {
          .AGlobalFunc(fid) => if is_user(user_ids, fid.id) {
            out = insert_sorted_int(out, fid.id)
          },
          _ => {},
        },
        _ => {},
      }
    }
  }
  out
}

fn is_user(user_ids: Dict<Int, Bool>, id: Int) Bool {
  case user_ids.get(id) {
    .Some(_) => true,
    .None => false,
  }
}

fn insert_sorted_int(v: Vector<Int>, id: Int) Vector<Int> {
  out: Vector<Int> = []
  inserted := false
  for x in v {
    if x == id {
      return v
    }
    if !inserted and id < x {
      out = .append(id)
      inserted = true
    }
    out = .append(x)
  }
  if !inserted {
    out = .append(id)
  }
  out
}

fn conservative_summary(f: CfgFunction) Summary {
  params: Vector<ParamSummary> = collect _ in f.params {
    ParamSummary.{ escape: .Retained, capability: .NoCap }
  }
  Summary.{ params, ret: .Shared }
}

fn same_summary(a: Summary, b: Summary) Bool {
  if a.params.len() != b.params.len() {
    return false
  }
  for pa, i in a.params {
    pb := b.params[i]
    if escape_int(pa.escape) != escape_int(pb.escape) or cap_int(pa.capability) != cap_int(pb.capability) {
      return false
    }
  }
  ret_int(a.ret) == ret_int(b.ret) and ret_alias_same(a.ret, b.ret)
}

// (escape_int/cap_int/ret_int/ret_alias_same: small tag comparators; ret_alias_same
//  compares the MayAliasParams sets when both are MayAliasParams, else true.)

pub fn compute(view: CfgView, b: BuiltinRegistry, sem: OptimizerSemantics) SummaryTable {
  // user function ids
  user_ids: Dict<Int, Bool> = Dict.new()
  for f in view.functions {
    user_ids[f.func_id] = true
  }
  // seed table conservatively
  table: SummaryTable = SummaryTable.{ by_func: Dict.new() }
  for f in view.functions {
    table.by_func[f.func_id] = conservative_summary(f)
  }
  // Order by SCCs of the call graph (see graph_scc.tw API). Process each SCC
  // reverse-topologically; iterate the SCC to a fixpoint or a cap.
  sccs := order_sccs(view, user_ids)   // Vector<Vector<Int>> of func ids, callee-first
  for scc in sccs {
    cap := scc.len() * 4 + 1
    round := 0
    changed := true
    for changed and round < cap {
      changed = false
      round = round + 1
      for fid in scc {
        f := func_by_id(view, fid)
        s := summarize_function(f, table, b, sem)
        case table.by_func.get(fid) {
          .Some(prev) => if !same_summary(prev, s) {
            changed = true
            table.by_func[fid] = s
          },
          .None => {
            changed = true
            table.by_func[fid] = s
          },
        }
      }
    }
  }
  table
}
```

Implement `order_sccs` on top of `graph_scc.tw` (build adjacency `fid → callee_ids`, run Tarjan, return SCCs in callee-first order; if `graph_scc` already returns reverse-topological order, use it directly). Implement `func_by_id` (linear scan of `view.functions`) and the small tag comparators.

- [ ] **Step 4: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — leaf summaries + recursion terminates; Phase 2 green.

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "summary: whole-program call-graph SCC fixpoint (compute)

Extract the linked-module call graph, order it by Tarjan SCCs (graph_scc),
and compute summaries bottom-up: seed conservative (params Retained, return
Shared), recompute each SCC member until stable or a members*4 cap, keeping
current sound summaries on cap. No consumption at call sites yet."
```

---

## Task 3: Consume summaries in `transfer_call`

Make call sites read the callee summary (via the threaded `table`) so borrowed args stay `Unique`, fresh returns become `Unique`, and retained/aliased origins demote. This improves both `summarize_function` (caller summaries) and `analyze` facts.

**Files:**
- Modify: `boot/compiler/ownership.tw`.
- Test: `boot/tests/suites/cfg_summary_suite.tw`.

- [ ] **Step 1: Write the failing tests**

Add a facts-through-summaries helper and the escape/return/aggregate fixtures:

```tw
// Analyze a multi-function module with computed summaries; return caller function.
fn analyzed_caller(funcs: Vector<AnfFunctionDef>, caller: String) cfg.CfgFunction {
  b := b_reg()
  m := module_of(funcs)
  v := cfg.build_view(m, b)
  t := summary.compute(v, b, sem())
  a := ownership.analyze_with_summaries(v, b, sem(), t)
  case cfg.function_named(a, caller) {
    .Some(f) => f,
    .None => error("no ${caller}"),
  }
}

fn own_exit_block0(f: cfg.CfgFunction, local_id: Int) ownership.Ownership {
  blk := f.blocks[0]
  case blk.exit.ownership.get(local_id) {
    .Some(tag) => ownership.own_of_tag(tag),
    .None => .Unknown,
  }
}

    .test("t3 borrow stays Unique: caller arg not demoted by a borrowing callee", fn() {
      b := b_reg()
      // Fn2 g(y){ Dict.new() }  (borrows y, returns fresh)
      g := fdef(2, "g", 1, .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1)))))
      // Fn1 f(){ a := Dict.new(); r := g(a); a }   -> a stays Unique
      f_body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(
          lid(1),
          .ACall(.AGlobalFunc(FuncId.{ id: 2 }), [.ALocal(lid(0))]),
          .Atom(.ALocal(lid(0))),
        ),
      )
      f := analyzed_caller([fdef(1, "f", 0, f_body), g], "f")
      try assert.equal(ownership.own_tag(own_exit_block0(f, 0)), ownership.own_tag(.Shared))
      // NOTE: a is returned (published) here, so exit is Shared; assert instead the
      // pre-return fact via entry.ownership of the return, or use a literal tail:
      .Ok({})
    })
```

> Fixture caution (as in Phase 2): the terminator publishes the returned value, so end positive fixtures in a **literal tail** to observe the pre-publication fact. Rewrite the borrow test to `f(){ a := Dict.new(); r := g(a); 0 }` and assert `own_at_exit(f,0, a)` is `Unique` (a not demoted by the borrowing call). Add the retain, returns-alias, multi-origin, and aggregate (`wrap`) fixtures analogously, asserting: retain ⇒ caller arg `Shared`; returns-alias ⇒ arg `Shared`; aggregate `wrap(xs)=Wrapper.{xs}` ⇒ arg `Shared` and result not `Unique`; unknown callee (no summary, e.g. call an id not in the module) ⇒ arg `Shared` (conservative).

(Write each `.test` with a concrete fixture and assertion following the design's consumption table.)

- [ ] **Step 2: Run to verify they fail**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — with the empty consumption, a borrowing call still publishes the arg (conservative bucket), so `a` is `Shared` not `Unique`.

- [ ] **Step 3: Implement consumption in `transfer_call`**

Replace the user-call handling; keep builtins (`call_info`) and the `.None` bucket:

```tw
fn transfer_call(
  st: ForwardState,
  result: Int,
  callee: Atom,
  args: Vector<Atom>,
  last: Vector<Int>,
  table: SummaryTable,
  sem: OptimizerSemantics,
) ForwardState {
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

// Existing conservative bucket (Phase 2 .None): publish every ref arg, result Unknown.
fn publish_call(st: ForwardState, result: Int, args: Vector<Atom>) ForwardState {
  for a in args {
    st = publish_atom(st, a)
  }
  set_own_st(st, result, .Unknown)
}

// Builtins keep the Phase 2 CallSemantics path (Allocate/Update/ReadOnly/...).
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
  // Per-param escape.
  for ps, i in s.params {
    if i < args.len() {
      case ps.escape {
        .Retained => st = publish_atom(st, args[i]),
        .Borrowed => {},
      }
    }
  }
  // Return effect drives result ownership + prov, and demotes aliased origins.
  case s.ret {
    .OwnedFresh => {
      st = set_own_st(st, result, .Unique)
      st
    },
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

`consume_call_base` and `call_info`/`CallSemantics` are the existing Phase 2 pieces; `summary_get` is from Task 1. Thread `table` into `transfer_op` → `transfer_call` (already added the param in Task 1 Step 8).

- [ ] **Step 4: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — borrow stays `Unique`; retain/alias/aggregate/unknown behave per the design; Phase 2 green.

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "ownership: consume summaries in transfer_call

Direct user calls with a summary resolve per the two-axis schema: Borrowed args
stay unchanged, Retained args publish, OwnedFresh returns are Unique, and
MayAliasParams returns publish every origin arg + result Shared. Consumed is
recorded only (never invalidates a caller binding in Phase 3). Builtins keep the
CallSemantics path; unknown/extern/indirect keep the conservative publish bucket."
```

---

## Task 4: Fold the summary into `--cfg` + wire the CLI

Render each function's summary as a `--cfg` header line and make `twk ir --cfg` compute + consume summaries.

**Files:**
- Modify: `boot/compiler/summary.tw` (header renderer), `boot/compiler/cfg.tw` (`render_view` takes a table), `boot/commands/ir.tw`.
- Test: `boot/tests/suites/cfg_summary_suite.tw`.

- [ ] **Step 1: Write the failing test**

```tw
    .test("t4 render: --cfg header shows the summary + determinism", fn() {
      b := b_reg()
      g := fdef(2, "g", 1, .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1)))))
      m := module_of([fdef(1, "f", 0, .Atom(.ALitInt(0))), g])
      v := cfg.build_view(m, b)
      t := summary.compute(v, b, sem())
      out1 := cfg.render_view_with_summaries(v, t)
      out2 := cfg.render_view_with_summaries(v, t)
      try assert.is_true(out1.contains("summary:"))
      try assert.is_true(out1.contains("ret=fresh") or out1.contains("ret="))
      try assert.is_true(out1 == out2)
      .Ok({})
    })
```

- [ ] **Step 2: Run to verify it fails**

Expected: FAIL — no `cfg.render_view_with_summaries` / no summary header.

- [ ] **Step 3: Implement the header renderer (`summary.tw`)**

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
```

- [ ] **Step 4: Render it in `cfg.tw`**

Add `render_view_with_summaries(view, table)` that renders like `render_view` but, per function, appends the summary string to the `fn …` header line (look up `table` by `func.func_id`; if absent, omit). Keep `render_view(view)` as a wrapper passing `summary.empty_table()`... **but** `cfg.tw` must not import `summary.tw` (cycle: `summary → ownership → cfg`, and `summary → cfg`). Instead have `render_view_with_summaries` take the already-built `SummaryTable` (a `cfg`-visible type via `ownership`? No — `SummaryTable` lives in `ownership.tw`, and `cfg.tw` must not import `ownership.tw`). **Resolution:** render the header in `summary.tw` (which imports both), not in `cfg.tw`. So add `pub fn render_view(view, table)` **in `summary.tw`** that calls `cfg.render_view(view)` for the body and splices the header lines, OR pass a pre-rendered `Dict<Int,String>` of headers into `cfg.render_view`. Choose the latter: `cfg.render_view_with_headers(view, headers: Dict<Int, String>)` where `headers` maps `func_id → summary string`; `summary.tw` builds `headers` and calls it. `cfg.tw` stays free of `ownership`/`summary` imports.

```tw
// cfg.tw
pub fn render_view_with_headers(view: CfgView, headers: Dict<Int, String>) String {
  // identical to render_view, but each function header line appends
  //   case headers.get(func.func_id) { .Some(h) => "  ${h}", .None => "" }
}
```

```tw
// summary.tw
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

Update the Task 4 test to call `summary.render_cfg(v, t)` instead of the placeholder `cfg.render_view_with_summaries`.

- [ ] **Step 5: Wire `twk ir --cfg`**

In `boot/commands/ir.tw`, `--cfg` branch:

```tw
  if parsed.has_flag("cfg") {
    b := artifacts.builtins
    view := cfg.build_view(artifacts.opt, b)
    view = ownership.prune_dead_merge(view)   // Task 5 adds this; until then, omit this line
    table := summary.compute(view, b, semantics.make_prelude_optimizer_semantics(b))
    analyzed := ownership.analyze_with_summaries(view, b, semantics.make_prelude_optimizer_semantics(b), table)
    print(summary.render_cfg(analyzed, table))
    return
  }
```

Add `use compiler.summary` to `ir.tw`. (Leave the `prune_dead_merge` line commented/omitted until Task 5.)

- [ ] **Step 6: Boot tests, rebuild CLI, smoke + determinism**

```bash
target/twk run boot/tests/main.tw
make bundle-cli
printf 'fn helper(xs: Vector<Int>) Int {\n  xs.len()\n}\nfn main2() Int {\n  a := [1, 2, 3]\n  helper(a)\n}\n' > /tmp/sum.tw
target/twk ir /tmp/sum.tw --cfg | grep -E "summary:|facts" | head
target/twk ir /tmp/sum.tw --cfg > /tmp/a.txt
target/twk ir /tmp/sum.tw --cfg > /tmp/b.txt
diff /tmp/a.txt /tmp/b.txt && echo DETERMINISTIC
```

Expected: boot tests PASS; header shows `summary:`; `diff` clean.

- [ ] **Step 7: Format, lint, commit**

```bash
target/twk fmt boot/compiler/cfg.tw boot/compiler/summary.tw boot/commands/ir.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/cfg.tw boot/compiler/summary.tw boot/commands/ir.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "cfg/ir: fold function summaries into twk ir --cfg

render_view_with_headers appends a per-function summary header; summary.render_cfg
builds the headers and runs build_view -> compute -> analyze_with_summaries ->
render. cfg.tw stays free of ownership/summary imports (headers passed in as a
Dict). Byte-identical across runs."
```

---

## Task 5: Dead-merge param pruning (minimal, pre-analysis)

Drop join/loop carried params that are dead across the boundary, realigning edge args, as a view transform run **before** analysis.

**Files:**
- Modify: `boot/compiler/ownership.tw` (`prune_dead_merge`), `boot/commands/ir.tw` (enable the line).
- Test: `boot/tests/suites/cfg_ownership_facts_suite.tw`.

- [ ] **Step 1: Write the failing test**

Add to `cfg_ownership_facts_suite.tw` a fixture where an `if` carries a param that is dead after the join, assert the join block loses that param and arity stays consistent:

```tw
    .test("phase2 t-prune: a dead carried join param is pruned; arity stays consistent", fn() {
      // r := if c { 1 } else { 2 }; return 0   (r is dead after the join)
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

(`module_of` here is the Phase 2 single-function helper in `cfg_ownership_facts_suite.tw`.)

- [ ] **Step 2: Run to verify it fails**

Expected: FAIL — `no field prune_dead_merge` / join still carries the dead param.

- [ ] **Step 3: Implement `prune_dead_merge` in `ownership.tw`**

```tw
pub fn prune_dead_merge(view: CfgView) CfgView {
  functions := collect f in view.functions {
    prune_function(f)
  }
  CfgView.{ functions }
}

fn prune_function(f: CfgFunction) CfgFunction {
  live := compute_liveness(f.blocks)
  // For each block, the set of param positions to drop = params not in live_in.
  drops: Dict<Int, Vector<Int>> = Dict.new()   // block id -> sorted param positions to drop
  for blk in f.blocks {
    li := live_get(live, blk.id.id).live_in
    dropped: Vector<Int> = []
    for p, i in blk.params {
      if !live_contains_int(li, p.id) {
        dropped = .append(i)
      }
    }
    if dropped.len() > 0 {
      drops[blk.id.id] = dropped
    }
  }
  // Rewrite: drop params at each block; drop the aligned atom in every edge whose
  // target is a pruned block (preds carry the source id; succs carry the target).
  blocks := collect blk in f.blocks {
    kept_params := drop_positions_local(blk.params, drops.get(blk.id.id))
    new_succs := collect e in blk.succs {
      CfgEdge.{ target: e.target, args: drop_positions_atom(e.args, drops.get(e.target.id)) }
    }
    new_preds := collect e in blk.preds {
      CfgEdge.{ target: e.target, args: drop_positions_atom(e.args, drops.get(blk.id.id)) }
    }
    new_term := prune_terminator(blk.terminator, drops)
    blk.params = kept_params
    blk.succs = new_succs
    blk.preds = new_preds
    blk.terminator = new_term
    blk
  }
  f.blocks = blocks
  f
}
```

Add `drop_positions_local(v, opt_positions)` / `drop_positions_atom(v, opt_positions)` (keep index `i` iff `i` not in the positions vector; `.None` ⇒ keep all) and `prune_terminator` (rewrite `Branch`/`CondBranch`/`LoopBackEdge`/`Match` edge-arg payloads by the target block's drops, keeping them identical to `succs` per the Phase 2 render-only contract). Removing a *dead* param cannot change any other local's liveness, so no re-liveness is needed.

- [ ] **Step 4: Enable in `ir.tw`**

Uncomment the `view = ownership.prune_dead_merge(view)` line added in Task 4 Step 5.

- [ ] **Step 5: Run tests + CLI smoke**

Run: `target/twk run boot/tests/main.tw` (PASS); `make bundle-cli`; re-run the `--cfg` determinism diff.

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/commands/ir.tw boot/tests/suites/cfg_ownership_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/commands/ir.tw boot/tests/suites/cfg_ownership_facts_suite.tw
git commit -m "ownership: minimal pre-analysis dead-merge param pruning

Drop join/loop carried params dead across the boundary (BlockFacts.live) and
realign every edge/terminator payload, as a view->view transform run before the
ownership/summary analysis so no fact map goes stale. Removing a dead param
cannot change other locals' liveness. Guarded by edge-arity reciprocity."
```

---

## Task 6: Match-arm pattern-binding precision (retire G3)

Kill pattern-bound locals at arm entry so they no longer leak live-in.

**Files:**
- Modify: `boot/compiler/cfg.tw` (`CfgBlock.bound`, `build_match`), `boot/compiler/ownership.tw` (`scan_block_backward` kills `bound`; forward pass seeds them `Unknown`).
- Test: `boot/tests/suites/cfg_ownership_facts_suite.tw` (tighten the G3 fixture).

- [ ] **Step 1: Write the failing test**

Tighten the existing G3 fixture (or add one) asserting the bound local is **not** live-in at the arm block:

```tw
    .test("phase2 t-patbind: pattern-bound local is killed at arm entry (not live-in)", fn() {
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
      // L5 is bound by the pattern -> defined at arm entry -> NOT live-in.
      try assert.is_false(live_contains(blk.entry.live, 5))
      .Ok({})
    })
```

(Needs `AnfMatchArm`, `TypeId`, `VariantId` imports — already present in the facts suite from Phase 2 Task 6.)

- [ ] **Step 2: Run to verify it fails**

Expected: FAIL — L5 currently leaks into `arm.entry.live` (G3 over-approximation).

- [ ] **Step 3: Add `CfgBlock.bound` and populate in `build_match`**

`cfg.tw`: add `bound: Vector<Int>` to `CfgBlock` (default `[]` in `empty_block`). In `build_match`, when creating each arm block, populate its `bound` from the arm pattern:

```tw
use compiler.anf_analysis.{collect_pattern_bindings}
// ...
  for arm, i in arms {
    nb := ctx.new_block("match.arm.${i}", [])
    ctx = nb.ctx
    // record pattern-bound locals so liveness kills them at arm entry
    bound := pattern_binding_ids(arm.pattern)
    ctx = ctx.set_block_bound(nb.id, bound)
    arm_ids = .append(nb.id)
    edge_list = .append(CfgEdge.{ target: nb.id, args: [] })
  }
```

Add `pattern_binding_ids(p) Vector<Int>` (wrap `collect_pattern_bindings` → sorted `Vector<Int>` of ids) and `set_block_bound(ctx, id, bound)` (fetch block, set `bound`, replace). Update `empty_block` to include `bound: []`.

- [ ] **Step 4: Kill `bound` in liveness; seed `Unknown` in the forward pass**

`ownership.tw` `scan_block_backward`: after the backward instruction scan, remove `blk.bound` from `cur` (they are defined at block entry):

```tw
  // pattern-bound locals are defined at block entry: kill them from live_in.
  cur = diff_sorted(cur, blk.bound)
  BlockScan.{ live_in: cur, live_after }
```

Forward pass: when building a block's entry `ForwardState`, seed each `bound` local `own = Unknown` (absent already reads Unknown, so this is a no-op for ownership) and `prov = ∅` (absent ⇒ ∅). No explicit seeding needed beyond ensuring the kill removes them from live-in. (The key change is the liveness kill; the forward pass already treats absent locals as Unknown/∅.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — L5 no longer live-in; the earlier G3 "no trap / never Unique" fixture still holds.

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/cfg.tw boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/cfg.tw boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
git commit -m "cfg/ownership: match-arm pattern-binding precision (retire Phase 2 G3)

build_match records each arm's collect_pattern_bindings on CfgBlock.bound;
scan_block_backward kills bound at block entry so pattern-bound locals no longer
leak live-in. Removes the Phase 2 sound-but-imprecise over-approximation."
```

---

## Task 7: Doc + audit hygiene, tracking, and full verification

Rewrite the stale `opt/README.md`, record the ANF-local peephole decision, audit the optimizer, mark the README bullets, and run the full gates.

**Files:**
- Modify: `boot/compiler/opt/README.md`, `docs/plans/sound-uniqueness/README.md`.
- Test: acceptance gates (commands).

- [ ] **Step 1: Optimizer audit**

```bash
grep -rniE "uniqueness|liveness|ownership|in.?place|cow|reuse" boot/compiler/opt/*.tw | grep -v README
```

Expected: only `semantics.tw` (builtin `CallSemantics`, consumed by ownership/census) and comments — **no** independent liveness/ownership/legality pass. Record the command + result in the commit body and in `opt/README.md`.

- [ ] **Step 2: Rewrite `opt/README.md`**

Replace the stale content (it still documents deleted `uniqueness.tw`/`liveness.tw`/`loop_builder.tw`/builder-region and "no CFG is constructed"). New content documents the **current** pipeline: `defer_elim` + the fixed-point peepholes (`dead_let`, `copy_prop`, `const_fold`, `branch_simp`) in `pipeline.tw`; `analysis.tw`/`use_count.tw`/`semantics.tw` (builtin call semantics); and a "CFG ownership facts" note pointing at `cfg.tw`/`ownership.tw`/`summary.tw` for the ownership analysis. Record the **peephole decision**: these peepholes stay ANF-local because they consult no ownership/control-flow facts; ownership-dependent work lives on the CFG facts.

- [ ] **Step 3: `--census` still shows 0 in-place**

```bash
printf 'fn build() Dict<Int, Int> {\n  d := Dict.new()\n  d[1] = 2\n  d[3] = 4\n  d\n}\n' > /tmp/cen.tw
target/twk ir /tmp/cen.tw --census
```

Expected: `dict_set` candidates ≥ 1, **in_place = 0** (Phase 3 changed no codegen).

- [ ] **Step 4: Mark the README bullets**

In `docs/plans/sound-uniqueness/README.md` Phase 3 section, mark delivered: the summaries bullet, the two precision bullets, and the peephole/"move queries" bullet (reframed — see the design's "vacuous bullet" note; add a one-line "Done — CFG facts already the single source; optimizer audited"). Keep any genuinely-deferred item unchecked.

- [ ] **Step 5: Full verification**

```bash
make boot-test        # all boot suites green
make stage2           # self-host fixed point (boot-only; no stage0-parity construct)
```

Expected: boot suites green; `make stage2` reaches `stage3 == stage4`.

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/opt/README.md 2>/dev/null || true
target/twk lint boot/main.tw
git add boot/compiler/opt/README.md docs/plans/sound-uniqueness/README.md
git commit -m "opt/docs: rewrite stale opt/README, optimizer audit, track Phase 3

Rewrite opt/README.md to the current pass set (the deleted uniqueness/liveness/
builder-region passes are gone; a CFG now exists) and record the ANF-local
peephole decision. Audit confirms no optimizer pass has an independent
ownership/liveness/legality path — the CFG facts are the single source. Mark the
delivered Phase 3 README bullets; --census still shows 0 in-place."
```

---

## Self-review

**1. Spec coverage** (against `phase3-design.md`):
- Two-axis summaries (escape acted-on; capability recorded) → Task 1 (types + classify) + Task 3 (consume). ✓
- Provenance folded into `ForwardState`, transitive publish, aggregates carry union → Task 1 Steps 4–7. ✓
- Return `MayAliasParams` (multi-origin) demotes all origins → Task 1 classify + Task 3 consume. ✓
- SCC fixpoint, conservative seed, iteration cap → Task 2. ✓
- Consumption in `transfer_call`; borrowed stay Unique; unknown/extern/indirect conservative → Task 3. ✓
- Fold summary into `--cfg`; determinism → Task 4. ✓
- Dead-merge pruning pre-analysis → Task 5. ✓
- Match-arm pattern-binding precision → Task 6. ✓
- `opt/README.md` rewrite + optimizer audit + peephole decision + census-0 + README tracking + make stage2 → Task 7. ✓
- Aggregate-escape soundness (`wrap`) → Task 1 (prov union + transitive publish) + Task 3 fixture. ✓

**2. Placeholder scan:** The consumption fixtures in Task 3 Step 1 are described-then-shown (borrow shown in full; retain/alias/multi-origin/aggregate/unknown described with exact expected facts and the design's consumption table). If executing, write each as a concrete `.test` mirroring the borrow fixture. The `graph_scc.tw` API (`order_sccs`) is adapted to the real API discovered in Task 2 Step 3 (grep first). No "TBD"/"add error handling".

**3. Type consistency:** `SummaryTable = .{ by_func: Dict<Int, Summary> }` and `summary_get`/`summarize_function` used consistently across Tasks 1–4; `EscapeEffect`/`ParamCapability`/`ReturnEffect` tag comparators match the enums; `ForwardState` gains `prov` in Task 1 and every construction updated; `analyze_with_summaries(view,b,sem,table)` is the consuming entry, `analyze(view,b,sem)` the wrapper (G1); `render_view_with_headers(view, headers: Dict<Int,String>)` keeps `cfg.tw` import-cycle-free.

**Open risks to watch during execution:**
- **Task 1 is the largest** (provenance threading + classification). Run the full suite after each step; G2 says a Phase 2 regression is a threading bug.
- **Import cycles:** `summary → ownership → cfg` and `summary → cfg` are fine (acyclic). Never import `ownership`/`summary` from `cfg.tw` — pass the header `Dict` in (Task 4 Step 4).
- **`run_fixpoint`/`forward_block` signatures gain `table`** in Task 1; update all call sites (grep) in the same task to keep it compiling.
- **G3 (body-only classification):** if summaries come out all-`Shared`, the classifier is reading post-terminator-publish state — use `forward_block_body`.
