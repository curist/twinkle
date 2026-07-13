# Phase 1 CFG Ownership View — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the Phase 1 structural CFG ownership view over optimized, defer-free ANF, expose it through `twk ir --cfg`, and gate graph shape plus determinism without ownership facts or codegen changes.

**Architecture:** ANF remains authoritative. `compiler/cfg.tw` derives a deterministic CFG view from `artifacts.opt` and preserves optimized-ANF identity through let-result `LocalId` mappings; block parameters name only syntactic carried values, while entry/exit fact maps exist as empty Phase 2 scaffolding. The CLI flag and tests mirror the Phase 0 census pattern: `twk ir --cfg` builds from `artifacts.opt`, prints a stable debug view, and returns before other IR printers.

**Tech Stack:** Twinkle (`.tw`), boot self-hosted compiler, `@std.testing` runner, `pipeline.compile_source` for in-process optimized-ANF fixtures, `make bundle-cli` for CLI-flag verification.

**Design specs:** `docs/plans/sound-uniqueness/cfg-ownership-ir.md`, `docs/plans/sound-uniqueness/architecture.md` section “Phase 1A — 1A-view”, and `docs/plans/sound-uniqueness/README.md` section “Phase 1”. This plan implements only the structural view; ownership `Unique`/`Shared`/`Unknown` facts start in Phase 2.

---

## File structure

- **Create** `boot/compiler/cfg.tw` — data model, deterministic builder, render helpers, and structural query helpers used by tests.
- **Create** `boot/tests/suites/cfg_ownership_suite.tw` — gate tests over inline fixtures compiled via `pipeline.compile_source(src) -> artifacts.opt`.
- **Modify** `boot/tests/main.tw` — register the CFG ownership suite.
- **Modify** `boot/main.tw` — register `--cfg` on `ir_cmd`.
- **Modify** `boot/commands/ir.tw` — handle `--cfg` over `artifacts.opt` with an early return.
- **Modify** `docs/plans/sound-uniqueness/README.md` — mark Phase 1 delivered only after the implementation and verification gates pass.

## Conventions (read once)

- **No ownership facts.** `entry_facts` and `exit_facts` are empty `Dict<Int, String>` values in every block. Do not add `Unique`, `Shared`, `Unknown`, candidate verdicts, proof ids, borrow facts, or rejection reasons in this phase.
- **No codegen changes.** Do not modify backend, prepare, runtime, optimizer rewrites, or ANF output. The CFG is a derived view and printer only.
- **Input is optimized ANF.** The builder is called with `artifacts.opt`. It asserts defer-free input and traps with `error("cfg: ADefer reached CFG builder; expected optimized defer-free ANF")` if an `ADefer` appears.
- **Identity is optimized ANF identity.** Instruction mappings use optimized `Let` result `LocalId` values. Do not key anything on pre-optimization/source locals.
- **Block params are syntactic carried values only.** Include every `AAssign` target in branch arms and loop bodies, every `AIf`/`AMatch`/`ALoop` result binding, and every `Break` payload local. Order deterministic unions by `LocalId.id`.
- **Partial rebinds have structural join params now; fact-level forwarding later.** Join params are the union of arm `AAssign` targets. Because optimized ANF rebinds the same `LocalId` rather than creating SSA names, Phase 1 edge args are target-param arity placeholders; Phase 2 facts distinguish a rebinding predecessor from one that forwards the incoming value.
- **Determinism rule.** Walk optimized ANF order and append vectors in that order. Never iterate a `Dict` to decide block order, output order, params, preds, or succs.
- **CLI rebuild rule.** Tests compile boot source directly with `target/twk run boot/tests/main.tw`; the CLI flag requires `make bundle-cli`.
- **After editing `.tw`.** Run `target/twk fmt <files>` and `target/twk lint boot/main.tw` or the relevant entry.
- **Commits.** Use short imperative subjects and include a body for non-trivial changes.

---

## Task 1: CFG data model, straight-line builder, and render shell

**Files:**
- Create: `boot/compiler/cfg.tw`
- Create: `boot/tests/suites/cfg_ownership_suite.tw`
- Modify: `boot/tests/main.tw`

**Interfaces:**
- Consumes: `compiler.anf.{AnfModule, AnfFunctionDef, AnfExpr, AnfOp, Atom}`, `compiler.builtins.BuiltinRegistry`.
- Produces:
  - `cfg.build_view(m: AnfModule, builtins: BuiltinRegistry) CfgView`
  - `cfg.render_view(view: CfgView) String`
  - structural query helpers: `cfg.function_named`, `cfg.count_blocks`, `cfg.count_terminator`, `cfg.max_param_count`, `cfg.empty_fact_blocks`, `cfg.count_edge_arity_mismatches`

- [ ] **Step 1: Write the failing suite file**

Create `boot/tests/suites/cfg_ownership_suite.tw`:

```tw
use @std.testing.assert as assert
use @std.testing as runner

use compiler.cfg
use compiler.pipeline

fn view(src: String) Result<cfg.CfgView, String> {
  artifacts := try pipeline.compile_source(src)
  .Ok(cfg.build_view(artifacts.opt, artifacts.builtins))
}

fn rendered(src: String) Result<String, String> {
  v := try view(src)
  .Ok(cfg.render_view(v))
}

fn function(src: String, name: String) Result<cfg.CfgFunction, String> {
  v := try view(src)
  case cfg.function_named(v, name) {
    .Some(f) => .Ok(f),
    .None => .Err("missing CFG function ${name}"),
  }
}

pub fn suite() runner.Suite {
  runner
    .suite("cfg ownership view")
    .test(
      "straight-line function has entry block, return terminator, and empty facts",
      fn() {
        f := try function("fn id(x: Int) Int {\n  y := x + 1\n  y\n}\n", "id")
        try assert.equal(cfg.count_blocks(f), 1)
        try assert.equal(cfg.count_terminator(f, "return"), 1)
        try assert.equal(cfg.empty_fact_blocks(f), cfg.count_blocks(f))
        try assert.equal(cfg.count_edge_arity_mismatches(f), 0)
        out := try rendered("fn id(x: Int) Int {\n  y := x + 1\n  y\n}\n")
        try assert.is_true(out.contains("// CFG ownership view"))
        try assert.is_true(out.contains("facts.in={} facts.out={}"))
        try assert.is_true(out.contains("terminator: return"))
        .Ok({})
      },
    )
}
```

- [ ] **Step 2: Register the suite before creating `compiler.cfg`**

In `boot/tests/main.tw`, add the import near the other suite imports:

```tw
use .suites.cfg_ownership_suite
```

Add the suite entry near the existing uniqueness suites:

```tw
  cfg_ownership_suite.suite(),
```

- [ ] **Step 3: Run the red test**

Run:

```bash
target/twk run boot/tests/main.tw 2>&1 | grep -iE 'cfg ownership|compiler.cfg|error'
```

Expected: a compile error because `compiler.cfg` does not exist. This proves the suite is registered and cannot pass accidentally.

- [ ] **Step 4: Create `boot/compiler/cfg.tw` with the data model and straight-line implementation**

Create `boot/compiler/cfg.tw`:

```tw
//! Structural CFG ownership view over optimized ANF.
//!
//! Phase 1 builds a deterministic view only: blocks, carried block parameters,
//! terminators, predecessor/successor lists, and optimized-ANF instruction
//! mappings. Ownership facts are Phase 2; `entry_facts` and `exit_facts` stay
//! empty in this module. ANF remains authoritative and codegen does not consume
//! this view.

use compiler.anf.{AnfExpr, AnfFunctionDef, AnfMatchArm, AnfModule, AnfOp, Atom}
use compiler.builtins.{BuiltinRegistry}
use compiler.core_ir.{LocalId}

pub type BlockId = .{ id: Int }

pub type CfgInstruction = .{ anf_local: LocalId, text: String }

pub type CfgEdge = .{ target: BlockId, args: Vector<LocalId> }

pub type Terminator = {
  Branch(BlockId, Vector<LocalId>),
  CondBranch(Atom, BlockId, Vector<LocalId>, BlockId, Vector<LocalId>),
  Match(Atom, Vector<CfgEdge>),
  LoopBackEdge(BlockId, Vector<LocalId>),
  Return(Atom?),
  ValueBreak(Atom),
  VoidBreak,
  ContinueIfPresent,
}

pub type CfgBlock = .{
  id: BlockId,
  name: String,
  params: Vector<LocalId>,
  instructions: Vector<CfgInstruction>,
  terminator: Terminator?,
  preds: Vector<CfgEdge>,
  succs: Vector<CfgEdge>,
  entry_facts: Dict<Int, String>,
  exit_facts: Dict<Int, String>,
}

pub type CfgFunction = .{
  func_id: Int,
  name: String,
  blocks: Vector<CfgBlock>,
}

pub type CfgView = .{ functions: Vector<CfgFunction> }

type BuildCtx = .{ next_block_id: Int, blocks: Vector<CfgBlock> }

type NewBlockOut = .{ ctx: BuildCtx, id: BlockId }

type BuildExprOut = .{ ctx: BuildCtx, current: BlockId }

fn bid(id: Int) BlockId {
  BlockId.{ id }
}

fn atom_text(atom: Atom) String {
  case atom {
    .ALocal(local) => "L${local.id}",
    .AGlobalLocal(gid) => "G${gid.id}",
    .AGlobalFunc(fid) => "Fn${fid.id}",
    .ALitInt(n) => n.to_string(),
    .ALitFloat(f) => f.to_string(),
    .ALitBool(v) => if v { "true" } else { "false" },
    .ALitStr(s) => "\"${s}\"",
    .ALitVoid => "void",
  }
}

fn local_text(local: LocalId) String {
  "L${local.id}"
}

fn local_list_text(locals: Vector<LocalId>) String {
  parts := collect local in locals {
    local_text(local)
  }
  parts.join(", ")
}

fn edge_text(edge: CfgEdge) String {
  "B${edge.target.id}(${local_list_text(edge.args)})"
}

fn sorted_insert_local(locals: Vector<LocalId>, local: LocalId) Vector<LocalId> {
  out: Vector<LocalId> = []
  inserted := false
  for existing in locals {
    if existing.id == local.id {
      return locals
    }

    if !inserted and local.id < existing.id {
      out = .append(local)
      inserted = true
    }

    out = .append(existing)
  }

  if !inserted {
    out = .append(local)
  }

  out
}

fn sorted_union(a: Vector<LocalId>, b: Vector<LocalId>) Vector<LocalId> {
  out := a
  for local in b {
    out = sorted_insert_local(out, local)
  }
  out
}

fn empty_block(id: BlockId, name: String, params: Vector<LocalId>) CfgBlock {
  CfgBlock.{
    id,
    name,
    params,
    instructions: [],
    terminator: .None,
    preds: [],
    succs: [],
    entry_facts: Dict.new(),
    exit_facts: Dict.new(),
  }
}

fn new_ctx() BuildCtx {
  BuildCtx.{ next_block_id: 0, blocks: [] }
}

fn new_block(ctx: BuildCtx, name: String, params: Vector<LocalId>) NewBlockOut {
  id := bid(ctx.next_block_id)
  ctx.next_block_id = ctx.next_block_id + 1
  ctx.blocks = .append(empty_block(id, name, params))
  NewBlockOut.{ ctx, id }
}

fn replace_block(ctx: BuildCtx, block: CfgBlock) BuildCtx {
  for existing, i in ctx.blocks {
    if existing.id.id == block.id.id {
      ctx.blocks[i] = block
      return ctx
    }
  }

  error("cfg: unknown block B${block.id.id}")
}

fn get_block(ctx: BuildCtx, id: BlockId) CfgBlock {
  for block in ctx.blocks {
    if block.id.id == id.id {
      return block
    }
  }

  error("cfg: unknown block B${id.id}")
}

fn push_instruction(ctx: BuildCtx, id: BlockId, inst: CfgInstruction) BuildCtx {
  block := get_block(ctx, id)
  block.instructions = .append(inst)
  replace_block(ctx, block)
}

fn add_pred(ctx: BuildCtx, target: BlockId, from: BlockId, args: Vector<LocalId>) BuildCtx {
  block := get_block(ctx, target)
  block.preds = .append(CfgEdge.{ target: from, args })
  replace_block(ctx, block)
}

fn finish_block(ctx: BuildCtx, id: BlockId, term: Terminator, succs: Vector<CfgEdge>) BuildCtx {
  block := get_block(ctx, id)
  block.terminator = .Some(term)
  block.succs = succs
  ctx = replace_block(ctx, block)
  for edge in succs {
    ctx = add_pred(ctx, edge.target, id, edge.args)
  }
  ctx
}

fn op_text(op: AnfOp) String {
  case op {
    .ACall(callee, args) => {
      rendered := collect arg in args { atom_text(arg) }
      "call ${atom_text(callee)}(${rendered.join(", ")})"
    },
    .AIf(cond, _, _) => "if ${atom_text(cond)}",
    .AMatch(scrutinee, _) => "match ${atom_text(scrutinee)}",
    .ALoop(_) => "loop",
    .ABinOp(_, left, right, _) => "binop ${atom_text(left)}, ${atom_text(right)}",
    .AUnOp(_, atom, _) => "unop ${atom_text(atom)}",
    .AMakeClosure(fid, free_vars) => {
      captures := collect local in free_vars { local_text(local) }
      "make_closure Fn${fid.id} [${captures.join(", ")}]"
    },
    .ARecord(_, fields) => {
      values := collect field in fields { atom_text(field.value) }
      "record { ${values.join(", ")} }"
    },
    .ARecordGet(atom, field, _) => "record_get ${atom_text(atom)} .${field.id}",
    .ARecordUpdate(atom, field, value, in_place, _) =>
      "record_update ${atom_text(atom)} .${field.id} = ${atom_text(value)} [in_place=${if in_place { "true" } else { "false" }}]",
    .AVariant(_, vid, args) => {
      rendered := collect arg in args { atom_text(arg) }
      "variant #${vid.id}(${rendered.join(", ")})"
    },
    .AArrayLit(elements) => {
      rendered := collect elem in elements { atom_text(elem) }
      "[${rendered.join(", ")}]"
    },
    .AIndex(base, index, _, _) => "index ${atom_text(base)}, ${atom_text(index)}",
    .AInit(atom) => "init ${atom_text(atom)}",
    .AAssign(local, atom) => "assign ${local_text(local)} = ${atom_text(atom)}",
    .AGlobalSet(gid, atom) => "global_set G${gid.id} = ${atom_text(atom)}",
    .ADefer(_) => error("cfg: ADefer reached CFG builder; expected optimized defer-free ANF"),
    .AWrapAnyref(atom, _) => "wrap_anyref ${atom_text(atom)}",
    .AUnwrapAnyref(atom, _) => "unwrap_anyref ${atom_text(atom)}",
  }
}

fn build_expr(ctx: BuildCtx, current: BlockId, expr: AnfExpr) BuildExprOut {
  case expr {
    .Let(local, op, body) => {
      ctx = push_instruction(ctx, current, CfgInstruction.{ anf_local: local, text: op_text(op) })
      build_expr(ctx, current, body)
    },
    .Atom(atom) => {
      ctx = finish_block(ctx, current, .Return(.Some(atom)), [])
      BuildExprOut.{ ctx, current }
    },
    .Return(atom_opt) => {
      ctx = finish_block(ctx, current, .Return(atom_opt), [])
      BuildExprOut.{ ctx, current }
    },
    .Break(.Some(atom)) => {
      ctx = finish_block(ctx, current, .ValueBreak(atom), [])
      BuildExprOut.{ ctx, current }
    },
    .Break(.None) => {
      ctx = finish_block(ctx, current, .VoidBreak, [])
      BuildExprOut.{ ctx, current }
    },
    .Continue => {
      ctx = finish_block(ctx, current, .ContinueIfPresent, [])
      BuildExprOut.{ ctx, current }
    },
  }
}

fn build_function(func: AnfFunctionDef) CfgFunction {
  nb := new_block(new_ctx(), "entry", [])
  built := build_expr(nb.ctx, nb.id, func.body)
  CfgFunction.{ func_id: func.func_id.id, name: func.name, blocks: built.ctx.blocks }
}

pub fn build_view(m: AnfModule, _builtins: BuiltinRegistry) CfgView {
  // Structural Phase 1 does not inspect builtins. Keep the parameter so Phase 2
  // ownership facts can share the same call surface as the census-style CLI/test
  // wiring without changing every caller.
  functions := collect f in m.functions {
    build_function(f)
  }
  CfgView.{ functions }
}

pub fn function_named(view: CfgView, name: String) CfgFunction? {
  for func in view.functions {
    if func.name == name {
      return .Some(func)
    }
  }

  .None
}

pub fn count_blocks(func: CfgFunction) Int {
  func.blocks.len()
}

pub fn count_terminator(func: CfgFunction, name: String) Int {
  n := 0
  for block in func.blocks {
    case block.terminator {
      .Some(term) => if terminator_name(term) == name {
        n = n + 1
      },
      .None => {},
    }
  }
  n
}

pub fn max_param_count(func: CfgFunction) Int {
  n := 0
  for block in func.blocks {
    if block.params.len() > n {
      n = block.params.len()
    }
  }
  n
}

pub fn empty_fact_blocks(func: CfgFunction) Int {
  n := 0
  for block in func.blocks {
    if block.entry_facts.keys().len() == 0 and block.exit_facts.keys().len() == 0 {
      n = n + 1
    }
  }
  n
}

fn target_param_count(func: CfgFunction, id: BlockId) Int {
  for block in func.blocks {
    if block.id.id == id.id {
      return block.params.len()
    }
  }

  -1
}

fn edge_arity_mismatch(func: CfgFunction, target: BlockId, args: Vector<LocalId>) Bool {
  args.len() != target_param_count(func, target)
}

fn terminator_arity_mismatches(func: CfgFunction, term: Terminator) Int {
  case term {
    .Branch(target, args) => if edge_arity_mismatch(func, target, args) { 1 } else { 0 },
    .CondBranch(_, then_target, then_args, else_target, else_args) => {
      n := 0
      if edge_arity_mismatch(func, then_target, then_args) {
        n = n + 1
      }
      if edge_arity_mismatch(func, else_target, else_args) {
        n = n + 1
      }
      n
    },
    .Match(_, edges) => {
      n := 0
      for edge in edges {
        if edge_arity_mismatch(func, edge.target, edge.args) {
          n = n + 1
        }
      }
      n
    },
    .LoopBackEdge(target, args) => if edge_arity_mismatch(func, target, args) { 1 } else { 0 },
    _ => 0,
  }
}

pub fn count_edge_arity_mismatches(func: CfgFunction) Int {
  n := 0
  for block in func.blocks {
    for edge in block.succs {
      if edge_arity_mismatch(func, edge.target, edge.args) {
        n = n + 1
      }
    }
    case block.terminator {
      .Some(term) => n = n + terminator_arity_mismatches(func, term),
      .None => {},
    }
  }
  n
}

fn terminator_name(term: Terminator) String {
  case term {
    .Branch(_, _) => "branch",
    .CondBranch(_, _, _, _, _) => "condbranch",
    .Match(_, _) => "match",
    .LoopBackEdge(_, _) => "loop-back-edge",
    .Return(_) => "return",
    .ValueBreak(_) => "value-break",
    .VoidBreak => "void-break",
    .ContinueIfPresent => "continue-if-present",
  }
}

fn terminator_text(term: Terminator) String {
  case term {
    .Branch(target, args) => "branch B${target.id}(${local_list_text(args)})",
    .CondBranch(cond, then_target, then_args, else_target, else_args) =>
      "condbranch ${atom_text(cond)} then B${then_target.id}(${local_list_text(then_args)}) else B${else_target.id}(${local_list_text(else_args)})",
    .Match(scrutinee, edges) => {
      rendered := collect edge in edges { edge_text(edge) }
      "match ${atom_text(scrutinee)} ${rendered.join(", ")}"
    },
    .LoopBackEdge(target, args) => "loop-back-edge B${target.id}(${local_list_text(args)})",
    .Return(.Some(atom)) => "return ${atom_text(atom)}",
    .Return(.None) => "return",
    .ValueBreak(atom) => "value-break ${atom_text(atom)}",
    .VoidBreak => "void-break",
    .ContinueIfPresent => "continue-if-present",
  }
}

fn render_block(lines: Vector<String>, block: CfgBlock) Vector<String> {
  lines = .append("  block B${block.id.id} ${block.name}(${local_list_text(block.params)})")
  pred_text := collect edge in block.preds { edge_text(edge) }
  succ_text := collect edge in block.succs { edge_text(edge) }
  lines = .append("    preds=[${pred_text.join(", ")}] succs=[${succ_text.join(", ")}]")
  lines = .append("    facts.in={} facts.out={}")
  for inst in block.instructions {
    lines = .append("    anf L${inst.anf_local.id}: ${inst.text}")
  }
  case block.terminator {
    .Some(term) => lines = .append("    terminator: ${terminator_text(term)}"),
    .None => lines = .append("    terminator: <missing>"),
  }
  lines
}

fn join_lines(lines: Vector<String>) String {
  if lines.len() == 0 {
    ""
  } else {
    "${lines.join("\n")}\n"
  }
}

pub fn render_view(view: CfgView) String {
  lines: Vector<String> = ["// CFG ownership view", "// ${view.functions.len()} function(s)", ""]
  for func in view.functions {
    lines = .append("fn ${func.name} [FuncId(${func.func_id})]")
    for block in func.blocks {
      lines = render_block(lines, block)
    }
    lines = .append("")
  }
  join_lines(lines)
}
```

- [ ] **Step 5: Format, lint, and run the straight-line gate**

Run:

```bash
target/twk fmt boot/compiler/cfg.tw boot/tests/suites/cfg_ownership_suite.tw boot/tests/main.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw 2>&1 | grep -iE 'cfg ownership|FAIL|error'
```

Expected: the "cfg ownership view" suite passes the straight-line test, with no `FAIL` or compile error in the filtered output.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/cfg.tw boot/tests/suites/cfg_ownership_suite.tw boot/tests/main.tw
git commit -m "cfg: add structural CFG view shell" -m "Build the first CFG ownership view data model over optimized ANF, with straight-line blocks, optimized let-local instruction mappings, stable rendering, and empty fact-map scaffolding for Phase 2."
```

---

## Task 2: Branch joins and syntactic carried block parameters

**Files:**
- Modify: `boot/compiler/cfg.tw`
- Modify: `boot/tests/suites/cfg_ownership_suite.tw`

**Interfaces:**
- Consumes: Task 1 `cfg.build_view`, `CfgBlock.params`, `CfgEdge.args`, `cfg.count_edge_arity_mismatches`.
- Produces: `condbranch` terminators, paramless arm blocks, join blocks with sorted union params, and predecessor edges whose `args.len()` always matches the target block's params.

- [ ] **Step 1: Add failing branch tests**

In `boot/tests/suites/cfg_ownership_suite.tw`, add these `.test` blocks after the straight-line test:

```tw
    .test(
      "if expression creates then else and join blocks",
      fn() {
        f := try function(
          "fn choose(b: Bool, a: Int, c: Int) Int {\n  x := 0\n  if b {\n    x = a\n  } else {\n    x = c\n  }\n  x\n}\n",
          "choose",
        )
        try assert.equal(cfg.count_terminator(f, "condbranch"), 1)
        try assert.is_true(cfg.max_param_count(f) >= 1)
        try assert.equal(cfg.empty_fact_blocks(f), cfg.count_blocks(f))
        try assert.equal(cfg.count_edge_arity_mismatches(f), 0)
        .Ok({})
      },
    )
    .test(
      "partial branch rebind has join params and valid edge arity",
      fn() {
        out := try rendered(
          "fn partial(b: Bool) Int {\n  x := 0\n  if b {\n    x = 1\n  } else {\n  }\n  x\n}\n",
        )
        try assert.is_true(out.contains("terminator: condbranch"))
        try assert.is_true(out.contains("join"))
        try assert.is_true(out.contains("succs=[B"))
        try assert.is_true(out.contains("preds=[B"))
        try assert.is_true(out.contains("facts.in={} facts.out={}"))
        f := try function(
          "fn partial(b: Bool) Int {\n  x := 0\n  if b {\n    x = 1\n  } else {\n  }\n  x\n}\n",
          "partial",
        )
        try assert.equal(cfg.count_edge_arity_mismatches(f), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run the red branch tests**

Run:

```bash
target/twk run boot/tests/main.tw 2>&1 | grep -iE 'if expression creates|partial branch|FAIL|error'
```

Expected: at least one new CFG ownership test fails because Task 1 records `AIf` as a straight-line instruction instead of creating a `condbranch` and join.

- [ ] **Step 3: Add branch-carried local collection helpers**

In `boot/compiler/cfg.tw`, insert these helpers after `sorted_union`:

```tw
fn atom_local(atom: Atom) LocalId? {
  case atom {
    .ALocal(local) => .Some(local),
    _ => .None,
  }
}

fn collect_assign_targets(expr: AnfExpr) Vector<LocalId> {
  case expr {
    .Let(_, op, body) => {
      found: Vector<LocalId> = case op {
        .AAssign(local, _) => [local],
        .AIf(_, then_e, else_e) => sorted_union(collect_assign_targets(then_e), collect_assign_targets(else_e)),
        .AMatch(_, arms) => {
          acc: Vector<LocalId> = []
          for arm in arms {
            acc = sorted_union(acc, collect_assign_targets(arm.body))
          }
          acc
        },
        .ALoop(loop_body) => collect_assign_targets(loop_body),
        .ADefer(_) => error("cfg: ADefer reached CFG builder; expected optimized defer-free ANF"),
        _ => [],
      }
      sorted_union(found, collect_assign_targets(body))
    },
    _ => [],
  }
}

fn collect_break_payloads(expr: AnfExpr) Vector<LocalId> {
  case expr {
    .Let(_, op, body) => {
      found: Vector<LocalId> = case op {
        .AIf(_, then_e, else_e) => sorted_union(collect_break_payloads(then_e), collect_break_payloads(else_e)),
        .AMatch(_, arms) => {
          acc: Vector<LocalId> = []
          for arm in arms {
            acc = sorted_union(acc, collect_break_payloads(arm.body))
          }
          acc
        },
        .ALoop(loop_body) => collect_break_payloads(loop_body),
        .ADefer(_) => error("cfg: ADefer reached CFG builder; expected optimized defer-free ANF"),
        _ => [],
      }
      sorted_union(found, collect_break_payloads(body))
    },
    .Break(.Some(atom)) => case atom_local(atom) {
      .Some(local) => [local],
      .None => [],
    },
    _ => [],
  }
}

fn expr_rebound_targets(expr: AnfExpr) Vector<LocalId> {
  collect_assign_targets(expr)
}

fn branch_join_params(result_local: LocalId, left: AnfExpr, right: AnfExpr) Vector<LocalId> {
  sorted_insert_local(sorted_union(expr_rebound_targets(left), expr_rebound_targets(right)), result_local)
}

fn edge_args_for(params: Vector<LocalId>) Vector<LocalId> {
  // Phase 1 does not rename values into full SSA. Edge args are target-param
  // placeholders so the printed graph has the same arity shape as later fact
  // maps. The forward-vs-rebound distinction for partial rebinds is represented
  // by Phase 2 facts, not by distinct Phase 1 local ids.
  params
}
```

- [ ] **Step 4: Replace `build_expr` with branch-aware logic**

In `boot/compiler/cfg.tw`, replace the entire `build_expr` function with this version:

```tw
fn build_expr(ctx: BuildCtx, current: BlockId, expr: AnfExpr) BuildExprOut {
  case expr {
    .Let(local, .AIf(cond, then_e, else_e), body) => build_if(ctx, current, local, cond, then_e, else_e, body),
    .Let(local, op, body) => {
      ctx = push_instruction(ctx, current, CfgInstruction.{ anf_local: local, text: op_text(op) })
      build_expr(ctx, current, body)
    },
    .Atom(atom) => {
      ctx = finish_block(ctx, current, .Return(.Some(atom)), [])
      BuildExprOut.{ ctx, current }
    },
    .Return(atom_opt) => {
      ctx = finish_block(ctx, current, .Return(atom_opt), [])
      BuildExprOut.{ ctx, current }
    },
    .Break(.Some(atom)) => {
      ctx = finish_block(ctx, current, .ValueBreak(atom), [])
      BuildExprOut.{ ctx, current }
    },
    .Break(.None) => {
      ctx = finish_block(ctx, current, .VoidBreak, [])
      BuildExprOut.{ ctx, current }
    },
    .Continue => {
      ctx = finish_block(ctx, current, .ContinueIfPresent, [])
      BuildExprOut.{ ctx, current }
    },
  }
}
```

Add `build_if` immediately above `build_expr`:

```tw
fn build_if(
  ctx: BuildCtx,
  current: BlockId,
  result_local: LocalId,
  cond: Atom,
  then_e: AnfExpr,
  else_e: AnfExpr,
  body: AnfExpr,
) BuildExprOut {
  params := branch_join_params(result_local, then_e, else_e)
  then_nb := new_block(ctx, "if.then", [])
  ctx = then_nb.ctx
  else_nb := new_block(ctx, "if.else", [])
  ctx = else_nb.ctx
  join_nb := new_block(ctx, "if.join", params)
  ctx = join_nb.ctx

  ctx = finish_block(
    ctx,
    current,
    .CondBranch(cond, then_nb.id, [], else_nb.id, []),
    [
      CfgEdge.{ target: then_nb.id, args: [] },
      CfgEdge.{ target: else_nb.id, args: [] },
    ],
  )

  then_out := build_expr(ctx, then_nb.id, then_e)
  ctx = then_out.ctx
  then_block := get_block(ctx, then_out.current)
  case then_block.terminator {
    .Some(.Return(_)) => {},
    .Some(.ValueBreak(_)) => {},
    .Some(.VoidBreak) => {},
    .Some(.ContinueIfPresent) => {},
    _ => ctx = finish_block(
      ctx,
      then_out.current,
      .Branch(join_nb.id, edge_args_for(params)),
      [CfgEdge.{ target: join_nb.id, args: edge_args_for(params) }],
    ),
  }

  else_out := build_expr(ctx, else_nb.id, else_e)
  ctx = else_out.ctx
  else_block := get_block(ctx, else_out.current)
  case else_block.terminator {
    .Some(.Return(_)) => {},
    .Some(.ValueBreak(_)) => {},
    .Some(.VoidBreak) => {},
    .Some(.ContinueIfPresent) => {},
    _ => ctx = finish_block(
      ctx,
      else_out.current,
      .Branch(join_nb.id, edge_args_for(params)),
      [CfgEdge.{ target: join_nb.id, args: edge_args_for(params) }],
    ),
  }

  build_expr(ctx, join_nb.id, body)
}
```

- [ ] **Step 5: Format, lint, and run the branch gate**

Run:

```bash
target/twk fmt boot/compiler/cfg.tw boot/tests/suites/cfg_ownership_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw 2>&1 | grep -iE 'cfg ownership|FAIL|error'
```

Expected: the CFG ownership branch tests pass, no `FAIL` in the filtered output.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/cfg.tw boot/tests/suites/cfg_ownership_suite.tw
git commit -m "cfg: model branch joins with carried params" -m "Derive branch CFG structure from optimized ANF, using deterministic LocalId-ordered join params for AAssign targets and AIf result bindings while keeping fact maps empty."
```

---

## Task 3: Match joins, value breaks, and deterministic arm edges

**Files:**
- Modify: `boot/compiler/cfg.tw`
- Modify: `boot/tests/suites/cfg_ownership_suite.tw`

**Interfaces:**
- Consumes: Task 2 sorted param helpers, `CfgEdge` rendering, and `cfg.count_edge_arity_mismatches`.
- Produces: `match` terminators, one paramless arm block per optimized-ANF arm, deterministic arm order, join params including `AMatch` result local and arm `AAssign` targets.

- [ ] **Step 1: Add failing match and value-break tests**

In `boot/tests/suites/cfg_ownership_suite.tw`, add these tests after the branch tests:

```tw
    .test(
      "match creates arm blocks and a join with carried params",
      fn() {
        f := try function(
          "fn pick(o: Int?, seed: Int) Int {\n  acc := seed\n  case o {\n    .Some(v) => {\n      acc = v\n    },\n    .None => {\n      acc = seed + 1\n    },\n  }\n  acc\n}\n",
          "pick",
        )
        try assert.equal(cfg.count_terminator(f, "match"), 1)
        try assert.is_true(cfg.max_param_count(f) >= 1)
        try assert.equal(cfg.empty_fact_blocks(f), cfg.count_blocks(f))
        try assert.equal(cfg.count_edge_arity_mismatches(f), 0)
        .Ok({})
      },
    )
    .test(
      "value-carrying break is printed as a terminator",
      fn() {
        out := try rendered(
          "fn first(n: Int) Int {\n  x := 0\n  for i in range(n) {\n    if i == 1 {\n      break i\n    }\n    x = i\n  }\n  x\n}\n",
        )
        try assert.is_true(out.contains("terminator: value-break"))
        try assert.is_true(out.contains("facts.in={} facts.out={}"))
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run the red match tests**

Run:

```bash
target/twk run boot/tests/main.tw 2>&1 | grep -iE 'match creates|value-carrying break|FAIL|error'
```

Expected: at least one new test fails because Task 2 still treats `AMatch` as a straight-line instruction.

- [ ] **Step 3: Add match param helper**

In `boot/compiler/cfg.tw`, insert after `branch_join_params`:

```tw
fn match_join_params(result_local: LocalId, arms: Vector<AnfMatchArm>) Vector<LocalId> {
  params: Vector<LocalId> = [result_local]
  for arm in arms {
    params = sorted_union(params, expr_rebound_targets(arm.body))
  }
  params
}
```

- [ ] **Step 4: Extend `build_expr` for `AMatch`**

In `boot/compiler/cfg.tw`, change the first cases in `build_expr` to include `AMatch`:

```tw
  case expr {
    .Let(local, .AIf(cond, then_e, else_e), body) => build_if(ctx, current, local, cond, then_e, else_e, body),
    .Let(local, .AMatch(scrutinee, arms), body) => build_match(ctx, current, local, scrutinee, arms, body),
```

Add `build_match` immediately after `build_if`:

```tw
fn build_match(
  ctx: BuildCtx,
  current: BlockId,
  result_local: LocalId,
  scrutinee: Atom,
  arms: Vector<AnfMatchArm>,
  body: AnfExpr,
) BuildExprOut {
  params := match_join_params(result_local, arms)
  arm_ids: Vector<BlockId> = []
  edge_list: Vector<CfgEdge> = []

  for _, i in arms {
    nb := new_block(ctx, "match.arm.${i}", [])
    ctx = nb.ctx
    arm_ids = .append(nb.id)
    edge_list = .append(CfgEdge.{ target: nb.id, args: [] })
  }

  join_nb := new_block(ctx, "match.join", params)
  ctx = join_nb.ctx
  ctx = finish_block(ctx, current, .Match(scrutinee, edge_list), edge_list)

  for arm, i in arms {
    arm_out := build_expr(ctx, arm_ids[i], arm.body)
    ctx = arm_out.ctx
    arm_block := get_block(ctx, arm_out.current)
    case arm_block.terminator {
      .Some(.Return(_)) => {},
      .Some(.ValueBreak(_)) => {},
      .Some(.VoidBreak) => {},
      .Some(.ContinueIfPresent) => {},
      _ => ctx = finish_block(
        ctx,
        arm_out.current,
        .Branch(join_nb.id, edge_args_for(params)),
        [CfgEdge.{ target: join_nb.id, args: edge_args_for(params) }],
      ),
    }
  }

  build_expr(ctx, join_nb.id, body)
}
```

- [ ] **Step 5: Format, lint, and run the match gate**

Run:

```bash
target/twk fmt boot/compiler/cfg.tw boot/tests/suites/cfg_ownership_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw 2>&1 | grep -iE 'cfg ownership|FAIL|error'
```

Expected: all CFG ownership tests pass, no `FAIL` in the filtered output. If the value-break fixture lowers to a nested `AIf` inside an `ALoop` and the break is not reachable yet, keep the test red and complete Task 4 before committing Task 3; do not weaken the value-break requirement.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/cfg.tw boot/tests/suites/cfg_ownership_suite.tw
git commit -m "cfg: add match arms and value-break structure" -m "Represent AMatch as deterministic arm edges with a LocalId-ordered join parameter set, preserving value-carrying break terminators for later ownership publication facts."
```

---

## Task 4: Loops, back-edges, continue, and defer-free assertion coverage

**Files:**
- Modify: `boot/compiler/cfg.tw`
- Modify: `boot/tests/suites/cfg_ownership_suite.tw`

**Interfaces:**
- Consumes: Task 2/3 carried-local helpers.
- Produces: loop header/body/exit blocks, loop-carried params from body `AAssign` targets plus `ALoop` result local plus break payload locals, `loop-back-edge` terminators for continuing paths, `continue-if-present` terminators for explicit `Continue`.

- [ ] **Step 1: Add failing loop and defer-free tests**

In `boot/tests/suites/cfg_ownership_suite.tw`, add these tests after the match tests:

```tw
    .test(
      "loop carries assign targets and has a loop-back-edge",
      fn() {
        f := try function(
          "fn sum_to(n: Int) Int {\n  acc := 0\n  for i in range(n) {\n    acc = acc + i\n  }\n  acc\n}\n",
          "sum_to",
        )
        try assert.is_true(cfg.count_terminator(f, "loop-back-edge") >= 1)
        try assert.is_true(cfg.max_param_count(f) >= 1)
        try assert.equal(cfg.empty_fact_blocks(f), cfg.count_blocks(f))
        try assert.equal(cfg.count_edge_arity_mismatches(f), 0)
        .Ok({})
      },
    )
    .test(
      "optimized defer-free input means cfg output never contains ADefer",
      fn() {
        out := try rendered(
          "fn use_defer() Int {\n  x := 1\n  defer {\n    y := x + 1\n  }\n  x\n}\n",
        )
        try assert.is_true(!out.contains("ADefer"))
        try assert.is_true(!out.contains("defer"))
        try assert.is_true(out.contains("// CFG ownership view"))
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run the red loop tests**

Run:

```bash
target/twk run boot/tests/main.tw 2>&1 | grep -iE 'loop carries|defer-free|FAIL|error'
```

Expected: the loop-carried test fails because `ALoop` is not yet structural.

- [ ] **Step 3: Add loop param helper**

In `boot/compiler/cfg.tw`, insert after `match_join_params`:

```tw
fn loop_params(result_local: LocalId, body: AnfExpr) Vector<LocalId> {
  params := sorted_insert_local(expr_rebound_targets(body), result_local)
  sorted_union(params, collect_break_payloads(body))
}
```

- [ ] **Step 4: Extend `build_expr` for `ALoop`**

In `boot/compiler/cfg.tw`, change the first cases in `build_expr` to include `ALoop`:

```tw
  case expr {
    .Let(local, .AIf(cond, then_e, else_e), body) => build_if(ctx, current, local, cond, then_e, else_e, body),
    .Let(local, .AMatch(scrutinee, arms), body) => build_match(ctx, current, local, scrutinee, arms, body),
    .Let(local, .ALoop(loop_body), body) => build_loop(ctx, current, local, loop_body, body),
```

Add `build_loop` after `build_match`:

```tw
fn build_loop(ctx: BuildCtx, current: BlockId, result_local: LocalId, loop_body: AnfExpr, body: AnfExpr) BuildExprOut {
  params := loop_params(result_local, loop_body)
  header_nb := new_block(ctx, "loop.header", params)
  ctx = header_nb.ctx
  body_nb := new_block(ctx, "loop.body", [])
  ctx = body_nb.ctx
  exit_nb := new_block(ctx, "loop.exit", params)
  ctx = exit_nb.ctx

  ctx = finish_block(
    ctx,
    current,
    .Branch(header_nb.id, edge_args_for(params)),
    [CfgEdge.{ target: header_nb.id, args: edge_args_for(params) }],
  )
  ctx = finish_block(
    ctx,
    header_nb.id,
    .Branch(body_nb.id, []),
    [CfgEdge.{ target: body_nb.id, args: [] }],
  )

  body_out := build_expr(ctx, body_nb.id, loop_body)
  ctx = body_out.ctx
  final_body := get_block(ctx, body_out.current)
  case final_body.terminator {
    .Some(.ValueBreak(_)) => {
      ctx = finish_block(
        ctx,
        body_out.current,
        .Branch(exit_nb.id, edge_args_for(params)),
        [CfgEdge.{ target: exit_nb.id, args: edge_args_for(params) }],
      )
    },
    .Some(.VoidBreak) => {
      ctx = finish_block(
        ctx,
        body_out.current,
        .Branch(exit_nb.id, edge_args_for(params)),
        [CfgEdge.{ target: exit_nb.id, args: edge_args_for(params) }],
      )
    },
    .Some(.ContinueIfPresent) => {
      ctx = finish_block(
        ctx,
        body_out.current,
        .LoopBackEdge(header_nb.id, edge_args_for(params)),
        [CfgEdge.{ target: header_nb.id, args: edge_args_for(params) }],
      )
    },
    .Some(.Return(_)) => {},
    _ => {
      ctx = finish_block(
        ctx,
        body_out.current,
        .LoopBackEdge(header_nb.id, edge_args_for(params)),
        [CfgEdge.{ target: header_nb.id, args: edge_args_for(params) }],
      )
    },
  }

  build_expr(ctx, exit_nb.id, body)
}
```

- [ ] **Step 5: Preserve value-break labels while adding exit edges**

The `build_loop` code above rewrites a body block ending in `ValueBreak` to a branch so the exit edge exists. That hides the value-break terminator from output, which violates the Phase 1 printer contract. Fix it by replacing this branch in `build_loop`:

```tw
    .Some(.ValueBreak(_)) => {
      ctx = finish_block(
        ctx,
        body_out.current,
        .Branch(exit_nb.id, edge_args_for(params)),
        [CfgEdge.{ target: exit_nb.id, args: edge_args_for(params) }],
      )
    },
```

with this exact code:

```tw
    .Some(.ValueBreak(atom)) => {
      break_exit := new_block(ctx, "loop.value-break.exit", [])
      ctx = break_exit.ctx
      ctx = finish_block(
        ctx,
        break_exit.id,
        .Branch(exit_nb.id, edge_args_for(params)),
        [CfgEdge.{ target: exit_nb.id, args: edge_args_for(params) }],
      )
      block := get_block(ctx, body_out.current)
      block.succs = [CfgEdge.{ target: break_exit.id, args: [] }]
      block.terminator = .Some(.ValueBreak(atom))
      ctx = replace_block(ctx, block)
      ctx = add_pred(ctx, break_exit.id, body_out.current, [])
    },
```

- [ ] **Step 6: Format, lint, and run the CFG gate**

Run:

```bash
target/twk fmt boot/compiler/cfg.tw boot/tests/suites/cfg_ownership_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw 2>&1 | grep -iE 'cfg ownership|FAIL|error'
```

Expected: all CFG ownership tests pass, no `FAIL` in the filtered output. The rendered loop fixture contains `terminator: loop-back-edge`; the break fixture contains `terminator: value-break`; every block prints `facts.in={} facts.out={}`.

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/cfg.tw boot/tests/suites/cfg_ownership_suite.tw
git commit -m "cfg: represent loop-carried structural edges" -m "Add loop header/body/exit blocks, deterministic carried params from syntactic rebinds and result locals, value-break exit structure, and defer-free optimized-ANF coverage."
```

---

## Task 5: `twk ir --cfg` CLI flag and byte-identical determinism gate

**Files:**
- Modify: `boot/main.tw`
- Modify: `boot/commands/ir.tw`
- Modify: `boot/tests/suites/cfg_ownership_suite.tw`

**Interfaces:**
- Consumes: `cfg.build_view(artifacts.opt, artifacts.builtins)`, `cfg.render_view`.
- Produces: canonical `twk ir --cfg` output over optimized ANF with an early return.

- [ ] **Step 1: Add the in-process determinism test**

In `boot/tests/suites/cfg_ownership_suite.tw`, add this final test to the suite chain:

```tw
    .test(
      "rendering is byte-identical across two builds",
      fn() {
        src := "fn choose(b: Bool, n: Int) Int {\n  acc := 0\n  for i in range(n) {\n    if b {\n      acc = acc + i\n    } else {\n      acc = acc + 1\n    }\n  }\n  acc\n}\n"
        first := try rendered(src)
        second := try rendered(src)
        try assert.equal(first, second)
        try assert.is_true(first.contains("terminator: condbranch"))
        try assert.is_true(first.contains("terminator: loop-back-edge"))
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run the in-process determinism gate**

Run:

```bash
target/twk fmt boot/tests/suites/cfg_ownership_suite.tw
target/twk run boot/tests/main.tw 2>&1 | grep -iE 'rendering is byte-identical|cfg ownership|FAIL|error'
```

Expected: the new determinism test passes, no `FAIL` in the filtered output.

- [ ] **Step 3: Register the CLI flag**

In `boot/main.tw`, extend `ir_cmd` by adding the concrete `.add_flag("cfg", "Print the structural CFG ownership view (optimized ANF)")` call after the existing `--census` / `--sites` flags:

```tw
ir_cmd := file_command("ir", "Compile and print compiler IR")
  .add_flag("core", "Print lowered Core IR")
  .add_flag("mono", "Print monomorphized Core IR")
  .add_flag("anf", "Print unoptimized ANF IR")
  .add_flag("opt", "Print optimized ANF IR")
  .add_flag("wat", "Print final linked WAT")
  .add_flag("all", "Print all available IR stages")
  .add_flag("census", "Print the candidate-op census table (optimized ANF)")
  .add_flag("sites", "With --census, also list each candidate site")
  .add_flag("cfg", "Print the structural CFG ownership view (optimized ANF)")
```

- [ ] **Step 4: Handle the flag in `run_ir_command`**

In `boot/commands/ir.tw`, add the import next to `compiler.census`:

```tw
use compiler.cfg
```

Then insert this early return immediately after the existing `--census` block and before `first := true`:

```tw
  if parsed.has_flag("cfg") {
    view := cfg.build_view(artifacts.opt, artifacts.builtins)
    print(cfg.render_view(view))
    return
  }
```

Leave the `--census` block first so `twk ir file.tw --census --cfg` preserves the existing Phase 0 census behavior.

- [ ] **Step 5: Format, lint, rebuild, and verify the CLI flag**

Run:

```bash
printf 'fn choose(b: Bool, n: Int) Int {\n  acc := 0\n  for i in range(n) {\n    if b {\n      acc = acc + i\n    } else {\n      acc = acc + 1\n    }\n  }\n  acc\n}\n' > /tmp/cfg_probe.tw
target/twk fmt boot/main.tw boot/commands/ir.tw boot/compiler/cfg.tw boot/tests/suites/cfg_ownership_suite.tw
target/twk lint boot/main.tw
make bundle-cli
target/twk ir /tmp/cfg_probe.tw --cfg > /tmp/cfg_probe.1
target/twk ir /tmp/cfg_probe.tw --cfg > /tmp/cfg_probe.2
cmp /tmp/cfg_probe.1 /tmp/cfg_probe.2
head -40 /tmp/cfg_probe.1
```

Expected:

```text
// CFG ownership view
```

The `cmp` command exits successfully with no output. The `head` output includes `terminator: condbranch`, `terminator: loop-back-edge`, `preds=[`, `succs=[`, and `facts.in={} facts.out={}`. It does not include ownership facts such as `Unique`, `Shared`, or `Unknown`.

- [ ] **Step 6: Verify the CLI reads optimized ANF and codegen still runs**

Run:

```bash
printf 'fn use_defer() Int {\n  x := 1\n  defer {\n    y := x + 1\n  }\n  x\n}\n' > /tmp/cfg_defer_probe.tw
target/twk ir /tmp/cfg_defer_probe.tw --cfg | grep -E 'CFG ownership view|defer|ADefer'
target/twk build /tmp/cfg_probe.tw -o /tmp/cfg_probe.wat
```

Expected: the grep output shows the `CFG ownership view` header and no `defer` / `ADefer` lines; the build command succeeds. This verifies the flag uses `artifacts.opt` after `eliminate_defers` and that Phase 1 did not change codegen.

- [ ] **Step 7: Commit**

```bash
git add boot/main.tw boot/commands/ir.tw boot/tests/suites/cfg_ownership_suite.tw
git commit -m "ir: add twk ir --cfg structural view" -m "Wire the Phase 1 CFG ownership view into the IR command over optimized ANF, with deterministic rendering checks and an early-return CLI path matching the census pattern."
```

---

## Task 6: Final Phase 1 gates and tracking docs

**Files:**
- Modify: `docs/plans/sound-uniqueness/README.md`

**Interfaces:**
- Consumes: Completed `compiler.cfg` implementation, test suite registration, and `twk ir --cfg` flag.
- Produces: Phase 1 tracking status updated only after verification passes.

- [ ] **Step 1: Run the focused boot suite gate**

Run:

```bash
target/twk run boot/tests/main.tw 2>&1 | grep -iE 'cfg ownership|uniqueness|FAIL|error'
```

Expected: CFG ownership, uniqueness census, and uniqueness guard suites report no `FAIL` or compile error in the filtered output.

- [ ] **Step 2: Run the full boot suite summary**

Run:

```bash
target/twk run boot/tests/main.tw 2>&1 | tail -5
```

Expected: the test runner summary reports all boot suites passing.

- [ ] **Step 3: Run lint after all `.tw` edits**

Run:

```bash
target/twk lint boot/main.tw
```

Expected: no direct-rebinding, record-copy-helper, unused-import, or inherent-call lint that requires source changes. If lints are reported, fix them before continuing and rerun `target/twk fmt` on the edited `.tw` files.

- [ ] **Step 4: Verify the canonical flag against a real compiler entry**

Run:

```bash
make bundle-cli
target/twk ir boot/main.tw --cfg > /tmp/boot-main.cfg.1
target/twk ir boot/main.tw --cfg > /tmp/boot-main.cfg.2
cmp /tmp/boot-main.cfg.1 /tmp/boot-main.cfg.2
grep -E 'CFG ownership view|terminator: condbranch|terminator: loop-back-edge|facts.in=\{\} facts.out=\{\}' /tmp/boot-main.cfg.1 | head -20
```

Expected: `cmp` exits successfully with no output. The grep output includes the CFG header, structural terminators, and empty facts. It does not include populated ownership fact names.

- [ ] **Step 5: Mark README Phase 1 items delivered**

In `docs/plans/sound-uniqueness/README.md`, under `### Phase 1 — CFG ownership view, no codegen changes`, change the four Phase 1 boxes from `- [ ]` to `- [x]` and append ` Done: 2026-07-13.` to each delivered item:

```markdown
- [x] **Build CFG ownership view over ANF.** Done: 2026-07-13. ANF remains authoritative; the CFG is
  a derived analysis view with deterministic block ids, successors, and mappings
  back to ANF lets/ops. Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [x] **Add SSA-style block parameters for carried values.** Done: 2026-07-13. Use block parameters
  for values crossing joins/back-edges; keep ownership facts as separate maps.
  Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [x] **Represent value-carrying breaks.** Done: 2026-07-13. Treat `break value` as both a control
  edge and a possible publication/region-exit edge; explicit freeze handling is a
  later mutable-region concern. Details:
  [cfg-ownership-ir.md](cfg-ownership-ir.md).
- [x] **Print the structural CFG.** Done: 2026-07-13. `twk ir --cfg` shows blocks, carried block
  parameters, terminators (including value-carrying break edges), and per-block
  ANF mapping, with the entry/exit fact maps shown empty. Populated ownership
  facts, candidates, accepted/rejected reasons, and proof ids arrive with Phase 2.
  Details: [cfg-ownership-ir.md](cfg-ownership-ir.md).
```

- [ ] **Step 6: Commit the tracking update**

```bash
git add docs/plans/sound-uniqueness/README.md
git commit -m "docs/sound-uniqueness: mark Phase 1 CFG view delivered" -m "Record that the structural CFG ownership view, carried params, value-break structure, and canonical --cfg printer are in place with empty fact maps for Phase 2."
```

---

## Self-review

**Spec coverage:**
- Structural view only, no ownership facts → Tasks 1–6 keep `entry_facts` / `exit_facts` empty and tests assert empty maps; no `Unique` / `Shared` / `Unknown` strings appear in acceptance. ✓
- Build from defer-free `artifacts.opt` and assert `ADefer` failure → Task 5 wires CLI over `artifacts.opt`; Task 4 adds optimized-defer-free coverage; `op_text`, `collect_assign_targets`, and `collect_break_payloads` fail loud on `ADefer`. ✓
- Block parameters = carried values only → Tasks 2–4 add syntactic params from `AAssign` targets, `AIf` / `AMatch` / `ALoop` result bindings, and `Break` payload locals. ✓
- Partial rebind join shape → Task 2 uses deterministic union params at the join and preserves predecessor edges for every arm. Phase 1 edge args are only target-param arity placeholders because optimized ANF is non-SSA; the forward-vs-rebound distinction is populated by Phase 2 facts. ✓
- Optimized-ANF identity → Task 1 `CfgInstruction.anf_local` records optimized let-result `LocalId`; all callers pass `artifacts.opt`. ✓
- Determinism → Tasks 1–5 use vector traversal order and sorted `LocalId` params; Task 5 and Task 6 compare byte-identical output across repeated runs. ✓
- `twk ir --cfg` canonical flag → Task 5 registers and verifies the flag with `make bundle-cli`. ✓
- Gate suite under `boot/tests/suites/`, registered in `boot/tests/main.tw` → Task 1 creates and registers `cfg_ownership_suite.tw`. ✓
- No codegen changes / no CFG-to-ANF round trip → File structure and all tasks touch only `compiler/cfg.tw`, IR CLI, tests, and tracking docs; Task 5 verifies a normal build still succeeds. ✓

**Placeholder scan:** No placeholder markers or ellipsis placeholders are used. Every code-edit step includes concrete code and every run step includes exact commands with expected output shape.

**Type consistency:** Public names are consistent across tasks: `build_view`, `render_view`, `function_named`, `count_blocks`, `count_terminator`, `max_param_count`, `empty_fact_blocks`, and `count_edge_arity_mismatches`. `CfgBlock.params`, `CfgEdge.args`, `CfgInstruction.anf_local`, `entry_facts`, and `exit_facts` are defined in Task 1 and reused by later tasks. Terminator names in tests match `terminator_name`: `condbranch`, `match`, `loop-back-edge`, `return`, `value-break`, `void-break`, and `continue-if-present`.

**Known residual risks for implementers to verify while executing:**
- If a fixture lowers differently than expected, inspect it with `target/twk ir <fixture> --opt` and keep the acceptance tied to optimized ANF shape, not source syntax.
- If stage0 rejects a planned helper style, preserve the same interface and behavior while rewriting into simpler Twinkle constructs already used elsewhere in `boot/`.
- The Phase 1 printer is an acceptance surface, not a stable user-facing format; keep it deterministic and readable, but do not add ownership-analysis content before Phase 2.
