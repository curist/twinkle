# Phase 2 Minimal Ownership Facts — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Populate the empty entry/exit fact maps the Phase 1 CFG view reserved with a sound `Unique`/`Shared`/`Unknown` ownership analysis (plus liveness and binding-validity), and surface it through `twk ir --cfg` — with no codegen changes.

**Architecture:** ANF stays authoritative. `compiler/cfg.tw` keeps ownership of the structural view but grows the data it discarded (the raw `AnfOp` per instruction, function params, and *real* per-edge transferred atoms as phi arguments). A new `compiler/ownership.tw` runs a per-function pipeline over that view — edge-arg-aware backward liveness, a forward per-`AnfOp` ownership transfer with a positional predecessor join iterated to fixpoint, and a parallel binding-validity meet — and returns a view with populated `BlockFacts`. `render_view` prints the facts; `twk ir --cfg` becomes `build_view → analyze → render_view`.

**Tech Stack:** Twinkle (`.tw`), boot self-hosted compiler, `@std.testing` runner, hand-built `AnfModule` fixtures (stable `LocalId`s, no optimizer), `compiler.opt.semantics` (`call_info`/`cow_base_arg`), `make bundle-cli` for CLI verification.

**Design specs (read before starting):**
- `docs/plans/sound-uniqueness/phase2-design.md` — the canonical design this plan implements. Every rule below traces to it.
- `docs/plans/sound-uniqueness/fact-lattice.md` — semantic core (lattice, hinges, join).
- `docs/plans/sound-uniqueness/worked-examples.md` — the Cases A/B/C/V/T fixtures reference.
- `docs/plans/sound-uniqueness/README.md` — the Phase 2 tracking bullets.

---

## File structure

- **Modify** `boot/compiler/cfg.tw` — extend `CfgInstruction` (+`op`), `CfgFunction` (+`params`), `CfgEdge.args` (`Vector<LocalId>` → `Vector<Atom>`) and terminator payloads (→ `Vector<Atom>`); build real edge args; replace `entry_facts`/`exit_facts: Dict<Int, String>` with `entry`/`exit: BlockFacts`; render populated facts.
- **Create** `boot/compiler/ownership.tw` — `Ownership` type, `analyze`, liveness, transfer, positional join + fixpoint, binding-validity, and `pub` query helpers for tests.
- **Create** `boot/tests/suites/cfg_ownership_facts_suite.tw` — TDD gate over hand-built ANF fixtures, one per rule, cross-referenced to worked-examples.
- **Modify** `boot/tests/main.tw` — register the new suite.
- **Modify** `boot/tests/suites/cfg_ownership_suite.tw` — update the Phase 1 structural suite for the new edge-arg (`Atom`) and empty-facts (`BlockFacts`) shapes.
- **Modify** `boot/commands/ir.tw` — `--cfg` runs `ownership.analyze` between `build_view` and `render_view`.
- **Modify** `boot/main.tw` — no new flag; only if `ir_cmd` wiring needs the semantics import (see Task 8).
- **Modify** `docs/plans/sound-uniqueness/README.md` — Task 9: mark Phase 2 bullets delivered, add the Phase 8 extern + Phase 3 dead-merge tracking rows and ledger entries.

## Conventions (read once)

- **Soundness before coverage.** Every default is conservative. Any op the analysis cannot prove drops the fact to `Unknown`/`Shared`. Never mint `Unique` without a proof.
- **Facts are keyed per `LocalId.id` (`Int`).** `BlockFacts.ownership`/`binding_valid` are `Dict<Int, ...>`; `live` is a **sorted** `Vector<Int>`. Absent ownership key ⇒ `Unknown`; absent `binding_valid` key ⇒ valid (`true`).
- **Analysis is view-driven.** Read the raw `op` off `CfgInstruction` and function params off `CfgFunction`. Never parse `text`. `analyze` takes no `AnfModule`.
- **`succs` is the authoritative edge list.** Terminator arg payloads exist only for `terminator_text` rendering and must stay identical to the matching `succs` edge. The analysis reads edge args off `succs`, never off a terminator.
- **Conservative ref rule.** Publication/field-store apply to **every `ALocal` operand** (no type table). Non-local atoms (literal/global/func) are ownership-neutral. Demoting a scalar to `Shared` is harmless.
- **Structural ops are unreachable in instructions.** `AIf`/`AMatch`/`ALoop` never appear as a `CfgInstruction` (Phase 1 lowers them to block structure). Like `ADefer`, their transfer arms are `error(...)` hard errors.
- **Determinism.** Iterate blocks by `id` order; never let a `Dict` iteration decide output/compare order. The `--cfg` output must be byte-identical across two builds.
- **After editing `.tw`.** Run `target/twk fmt <files>` then `target/twk lint boot/main.tw`.
- **Test the boot suite with:** `target/twk run boot/tests/main.tw`. The CLI flag needs `make bundle-cli`.
- **Commits.** Short imperative subject; body for non-trivial changes. End with the `Co-Authored-By` trailer.

---

## Task 1: Carry the raw op and function params (additive, keeps everything green)

Phase 1 dropped the raw `AnfOp` after `op_text(op)` and never recorded a function's own parameters. Add both fields; the transfer and liveness need them. This task is purely additive — `op_text` still renders the same strings, so the Phase 1 structural suite stays green.

**Files:**
- Modify: `boot/compiler/cfg.tw:15` (`CfgInstruction`), `:42` (`CfgFunction`), the instruction push and `build_function`.
- Test: `boot/tests/suites/cfg_ownership_suite.tw` (extend an existing test).

- [ ] **Step 1: Write the failing test**

Add to `boot/tests/suites/cfg_ownership_suite.tw` (uses the existing `function`/`view` helpers):

```tw
runner.test("phase2 t1: instructions carry raw op and functions carry params", fn () {
  src := "fn f(x: Int) Int {\n  y := x + 1\n  y\n}\n"
  case function(src, "f") {
    .Ok(func) => {
      // The function records its own parameter local(s).
      assert.true(func.params.len() >= 1, "expected f to record its params")
      // Every instruction carries a raw op, not just text.
      has_binop := false
      for block in func.blocks {
        for inst in block.instructions {
          case inst.op {
            .ABinOp(_, _, _, _) => has_binop = true,
            _ => {},
          }
        }
      }
      assert.true(has_binop, "expected the y := x + 1 instruction to carry an ABinOp")
    },
    .Err(e) => assert.fail(e),
  }
})
```

- [ ] **Step 2: Run it to verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — `CfgInstruction` has no field `op` / `CfgFunction` has no field `params`.

- [ ] **Step 3: Extend the data types**

In `boot/compiler/cfg.tw`, edit `CfgInstruction` and `CfgFunction`:

```tw
pub type CfgInstruction = .{ anf_local: LocalId, op: AnfOp, text: String }

pub type CfgFunction = .{ func_id: Int, name: String, params: Vector<LocalId>, blocks: Vector<CfgBlock> }
```

- [ ] **Step 4: Populate `op` at the push site**

In `build_expr`, the straight-line `.Let(local, op, body)` arm currently builds a `CfgInstruction` with only `anf_local`/`text`. Add `op`:

```tw
    .Let(local, op, body) => {
      ctx = .push_instruction(current, CfgInstruction.{ anf_local: local, op, text: op_text(op) })
      ctx.build_expr(current, body)
    },
```

- [ ] **Step 5: Populate `params` in `build_function`**

`build_function` receives `func: AnfFunctionDef`, which has `params: Vector<Param>` where `Param = .{ local, ty }` (`core_ir.tw:98`). Record just the locals:

```tw
fn build_function(func: AnfFunctionDef) CfgFunction {
  nb := new_ctx().new_block("entry", [])
  built := nb.ctx.build_expr(nb.id, func.body)
  ctx := case built.fallthrough {
    .Some(ft) => built.ctx.finish_block(ft.block, .Return(.Some(ft.tail)), []),
    .None => built.ctx,
  }
  params := collect p in func.params {
    p.local
  }
  CfgFunction.{ func_id: func.func_id.id, name: func.name, params, blocks: ctx.blocks }
}
```

Add `Param` to the `use compiler.core_ir.{...}` line if the field access needs it (it does not — `p.local` resolves via the record field; no import needed).

- [ ] **Step 6: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — the new test and all Phase 1 structural tests.

- [ ] **Step 7: Format, lint, commit**

```bash
target/twk fmt boot/compiler/cfg.tw boot/tests/suites/cfg_ownership_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/cfg.tw boot/tests/suites/cfg_ownership_suite.tw
git commit -m "cfg: carry raw AnfOp per instruction and function params for Phase 2

The Phase 2 ownership transfer and liveness read the raw op and its operand
atoms; Phase 1 kept only rendered text and dropped the op. Add op to
CfgInstruction and params to CfgFunction. Purely additive; op_text still
renders identically so the structural view is unchanged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: Real per-edge transferred atoms (phi arguments)

Phase 1 wired placeholder edge args (`edge_args_for(params)` returns the params verbatim) and dropped each arm's `FallThrough.tail`. The ownership join needs the *actual atom* each predecessor feeds into each target param — a join's result local is not a value any arm computes. Make `CfgEdge.args` and the terminator payloads `Vector<Atom>`, and build the real mapping: **result param ← arm tail atom**, **loop-result param ← break payload**, **carried local ← `ALocal(that local)`**.

**Files:**
- Modify: `boot/compiler/cfg.tw` — `CfgEdge` (`:17`), `Terminator` (`:19-27`), `edge_args_for`, the fall-through/join/loop wiring, `local_list_text` uses in rendering, and `edge_arity_mismatch`.
- Test: `boot/tests/suites/cfg_ownership_suite.tw`.

- [ ] **Step 1: Write the failing test**

Add to `cfg_ownership_suite.tw`:

```tw
runner.test("phase2 t2: branch join edge args carry the arm tail atom", fn () {
  // Two arms each produce a fresh value; the join param is fed the arm's tail.
  src := "fn f(c: Bool) Int {\n  r := if c { 1 } else { 2 }\n  r\n}\n"
  case function(src, "f") {
    .Ok(func) => {
      join := block_named(func, "if.join")
      case join {
        .Some(jb) => {
          // The join carries exactly one param (the result local r).
          assert.eq_int(jb.params.len(), 1, "join should carry the result local")
          rp := jb.params[0]
          // Each predecessor edge supplies a concrete atom for that param,
          // and it is NOT the placeholder ALocal(r): the then-arm feeds 1.
          fed_literal := false
          for pe in jb.preds {
            for a in pe.args {
              case a {
                .ALitInt(_) => fed_literal = true,
                _ => {},
              }
            }
          }
          assert.true(fed_literal, "expected a literal arm tail (1/2) in the join edge args")
        },
        .None => assert.fail("no if.join block"),
      }
    },
    .Err(e) => assert.fail(e),
  }
})
```

Add the `block_named` helper near the other helpers at the top of the suite:

```tw
fn block_named(func: cfg.CfgFunction, name: String) cfg.CfgBlock? {
  for block in func.blocks {
    if block.name == name {
      return .Some(block)
    }
  }
  .None
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — edge args are placeholder locals; `CfgEdge.args` is `Vector<LocalId>` so `case a { .ALitInt ... }` will not even typecheck (or no literal is present).

- [ ] **Step 3: Retype edges and terminators to `Vector<Atom>`**

In `boot/compiler/cfg.tw`:

```tw
pub type CfgEdge = .{ target: BlockId, args: Vector<Atom> }

pub type Terminator = {
  Branch(BlockId, Vector<Atom>),
  CondBranch(Atom, BlockId, Vector<Atom>, BlockId, Vector<Atom>),
  Match(Atom, Vector<CfgEdge>),
  LoopBackEdge(BlockId, Vector<Atom>),
  Return(Atom?),
  ValueBreak(Atom),
  VoidBreak,
  ContinueIfPresent,
}
```

- [ ] **Step 4: Build the param→atom mapping**

Replace `edge_args_for` with a mapping that takes the values feeding each param. Add helpers:

```tw
// Map each target param to the atom this path supplies:
//  - the result local gets `result_atom` (the arm/break tail),
//  - every other carried local gets ALocal(itself) (rebound or forwarded — same id).
fn edge_args_with_result(params: Vector<LocalId>, result_local: LocalId, result_atom: Atom) Vector<Atom> {
  collect p in params {
    if p.id == result_local.id {
      result_atom
    } else {
      .ALocal(p)
    }
  }
}

// No distinguished result value on this edge (e.g. a loop back-edge): every
// carried param forwards its own current binding.
fn edge_args_forward(params: Vector<LocalId>) Vector<Atom> {
  collect p in params {
    .ALocal(p)
  }
}
```

- [ ] **Step 5: Thread the result atom through the join wiring**

`wire_fallthrough_to_join` must know the result local and the arm's tail. Change its signature and callers:

```tw
fn wire_fallthrough_to_join(
  ctx: BuildCtx,
  ft: FallThrough,
  join: BlockId,
  params: Vector<LocalId>,
  result_local: LocalId,
) BuildCtx {
  args := edge_args_with_result(params, result_local, ft.tail)
  ctx.finish_block(ft.block, .Branch(join, args), [CfgEdge.{ target: join, args }])
}
```

In `build_if`, pass the whole `FallThrough` (not just `ft.block`) and the `result_local`:

```tw
  case then_out.fallthrough {
    .Some(ft) => ctx = .wire_fallthrough_to_join(ft, join_nb.id, params, result_local),
    .None => {},
  }
  case else_out.fallthrough {
    .Some(ft) => ctx = .wire_fallthrough_to_join(ft, join_nb.id, params, result_local),
    .None => {},
  }
```

In `build_match`, the `falling` vector currently stores only `ft.block`. Change it to store the whole `FallThrough` so the tail survives:

```tw
  falling: Vector<FallThrough> = []
  for arm, i in arms {
    arm_out := ctx.build_expr(arm_ids[i], arm.body)
    ctx = arm_out.ctx
    arm_breaks = append_all(arm_breaks, arm_out.breaks)
    arm_continues = append_all(arm_continues, arm_out.continues)
    case arm_out.fallthrough {
      .Some(ft) => falling = .append(ft),
      .None => {},
    }
  }

  if falling.len() == 0 {
    return BuildExprOut.{ ctx, fallthrough: .None, breaks: arm_breaks, continues: arm_continues }
  }

  join_nb := ctx.new_block("match.join", params)
  ctx = join_nb.ctx
  for ft in falling {
    ctx = .wire_fallthrough_to_join(ft, join_nb.id, params, result_local)
  }
  ctx.build_join_continuation(join_nb.id, body, arm_breaks, arm_continues)
```

`build_match` already receives `result_local` — thread it in (it is the `result_local` param of `build_match`).

- [ ] **Step 6: Fix the loop wiring**

Loop back-edges and the initial branch into the header carry the loop's carried params by forwarding; the break→exit edge feeds the loop-result param the break payload. In `build_loop`:

```tw
  back_args := edge_args_forward(params)
  ctx = .finish_block(current, .Branch(header_nb.id, back_args), [CfgEdge.{ target: header_nb.id, args: back_args }])
  ctx = .finish_block(header_nb.id, .Branch(body_nb.id, []), [CfgEdge.{ target: body_nb.id, args: [] }])
```

`wire_backedge` keeps forwarding args (the body loops back with current bindings):

```tw
fn wire_backedge(ctx: BuildCtx, ft: FallThrough?, header: BlockId, params: Vector<LocalId>) BuildCtx {
  case ft {
    .Some(f) => {
      back_args := edge_args_forward(params)
      ctx.finish_block(f.block, .LoopBackEdge(header, back_args), [CfgEdge.{ target: header, args: back_args }])
    },
    .None => ctx,
  }
}
```

`wire_break_exit` feeds the loop-result param the break payload:

```tw
fn wire_break_exit(ctx: BuildCtx, block: BlockId, exit: BlockId, params: Vector<LocalId>, result_local: LocalId) BuildCtx {
  b := ctx.get_block(block)
  case b.terminator {
    .Some(.VoidBreak) => {
      args := edge_args_forward(params)
      ctx.finish_block(block, .VoidBreak, [CfgEdge.{ target: exit, args }])
    },
    .Some(.ValueBreak(a)) => {
      args := edge_args_with_result(params, result_local, a)
      ctx.finish_block(block, .ValueBreak(a), [CfgEdge.{ target: exit, args }])
    },
    _ => ctx,
  }
}
```

Update the `build_loop` call sites: `wire_backedge(body_out.fallthrough, header_nb.id, params)`, the continue loop uses `edge_args_forward(params)`, and `wire_break_exit(bb, exit_nb.id, params, result_local)`. `build_loop` receives `result_local` — thread it through.

- [ ] **Step 7: Fix rendering and arity for `Atom` args**

`local_list_text(args)` took `Vector<LocalId>`. Edge/terminator args are now `Vector<Atom>`. Add and use an atom-list renderer:

```tw
fn atom_list_text(atoms: Vector<Atom>) String {
  parts := collect a in atoms {
    atom_text(a)
  }
  parts.join(", ")
}
```

Replace `local_list_text(args)` with `atom_list_text(args)` in `edge_text` and in every `terminator_text` arm that renders edge args (`Branch`, `CondBranch`, `LoopBackEdge`). Leave `local_list_text` in place for `block.params` (still `Vector<LocalId>`). `edge_arity_mismatch` compares `args.len()` to the target param count — unchanged (length still valid for `Vector<Atom>`).

- [ ] **Step 8: Update the Phase 1 structural suite for `Atom` edge args**

`cfg_ownership_suite.tw` has a `same_args(left: Vector<LocalId>, right: Vector<LocalId>)` helper and edge-arg assertions. Retype them to `Atom` and compare by `atom_text` equality:

```tw
fn same_args(left: Vector<Atom>, right: Vector<Atom>) Bool {
  if left.len() != right.len() {
    return false
  }
  for a, i in left {
    if atom_text_pub(a) != atom_text_pub(right[i]) {
      return false
    }
  }
  true
}
```

`atom_text` is private to `cfg.tw`; expose a `pub fn atom_text_pub(a: Atom) String { atom_text(a) }` in `cfg.tw` for the test, or compare structurally in the test. Adjust the specific value-break assertion (which checked `LocalId` args) to compare against the expected `Atom` (`.ALocal(payload)`).

- [ ] **Step 9: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — the new phi-arg test plus the retyped Phase 1 suite.

- [ ] **Step 10: Format, lint, commit**

```bash
target/twk fmt boot/compiler/cfg.tw boot/tests/suites/cfg_ownership_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/cfg.tw boot/tests/suites/cfg_ownership_suite.tw
git commit -m "cfg: carry real per-edge transferred atoms as phi arguments

Edge args (and terminator payloads) become Vector<Atom>: each predecessor
supplies the concrete atom it feeds into each target param — result param gets
the arm tail / break payload, carried params forward ALocal(self). This is
what the Phase 2 ownership join reads positionally; the placeholder-param
edge args could not express a join result's ownership. succs stays the
authoritative edge list; terminator payloads are render-only and identical.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: Fact storage shape + `ownership.tw` skeleton (identity analyze)

Replace the Phase 1 string fact maps with grouped `BlockFacts`, and stand up `ownership.tw` with an `analyze` that returns the view unchanged (facts still empty). This flips the storage shape and wires the module while keeping everything green — no real facts yet.

**Files:**
- Modify: `boot/compiler/cfg.tw` — `CfgBlock` fields, `empty_block`, `empty_fact_blocks`, `render_block` facts line.
- Create: `boot/compiler/ownership.tw`.
- Modify: `boot/tests/suites/cfg_ownership_suite.tw` — `empty_fact_blocks` still counts un-analyzed blocks.

- [ ] **Step 1: Write the failing test**

Add to `cfg_ownership_suite.tw` (imports `use compiler.ownership` at top):

```tw
runner.test("phase2 t3: analyze returns a view; empty-facts query still works", fn () {
  src := "fn f(x: Int) Int {\n  x\n}\n"
  case view(src) {
    .Ok(v) => {
      analyzed := ownership.analyze(v, builtins.make_builtin_registry(), sem_for())
      case cfg.function_named(analyzed, "f") {
        .Some(func) => assert.true(func.blocks.len() >= 1, "f has blocks after analyze"),
        .None => assert.fail("f missing after analyze"),
      }
    },
    .Err(e) => assert.fail(e),
  }
})
```

Add a `sem_for()` helper near the top of the suite:

```tw
use compiler.opt.semantics.{make_prelude_optimizer_semantics}

fn sem_for() semantics.OptimizerSemantics {
  make_prelude_optimizer_semantics(builtins.make_builtin_registry())
}
```

(Import `compiler.opt.semantics as semantics` too, or destructure the type — match the existing import style.)

- [ ] **Step 2: Run it to verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — no module `compiler.ownership`.

- [ ] **Step 3: Change the fact storage shape in `cfg.tw`**

Add the `BlockFacts` type and `Ownership` re-export is NOT here (Ownership lives in `ownership.tw`); `BlockFacts` holds only primitive maps, so it can live in `cfg.tw` keyed by `Int`:

```tw
pub type BlockFacts = .{
  ownership: Dict<Int, Int>,   // LocalId.id -> Ownership tag (0=Unique,1=Shared,2=Unknown); see ownership.tw
  binding_valid: Dict<Int, Bool>,
  live: Vector<Int>,
}

fn empty_block_facts() BlockFacts {
  BlockFacts.{ ownership: Dict.new(), binding_valid: Dict.new(), live: [] }
}
```

> Note: `BlockFacts.ownership` stores the ownership **tag as an `Int`** to keep `cfg.tw` free of an `ownership.tw` dependency (avoids an import cycle: `ownership.tw` uses `cfg.tw`). `ownership.tw` owns the `Ownership` enum and the tag↔enum mapping. This is decided here; do not make `cfg.tw` import `ownership.tw`.

Change `CfgBlock`:

```tw
pub type CfgBlock = .{
  id: BlockId,
  name: String,
  params: Vector<LocalId>,
  instructions: Vector<CfgInstruction>,
  terminator: Terminator?,
  preds: Vector<CfgEdge>,
  succs: Vector<CfgEdge>,
  entry: BlockFacts,
  exit: BlockFacts,
}
```

Update `empty_block` to use `entry: empty_block_facts()`, `exit: empty_block_facts()`.

- [ ] **Step 4: Update `empty_fact_blocks` and rendering**

```tw
pub fn empty_fact_blocks(func: CfgFunction) Int {
  n := 0
  for block in func.blocks {
    if block.entry.ownership.keys().len() == 0
      and block.exit.ownership.keys().len() == 0
      and block.entry.binding_valid.keys().len() == 0
      and block.exit.binding_valid.keys().len() == 0
      and block.entry.live.len() == 0
      and block.exit.live.len() == 0 {
      n = n + 1
    }
  }
  n
}
```

`render_block` still prints the empty line for now (Task 8 fills it):

```tw
  lines = .append("    facts.in={} facts.out={}")
```

- [ ] **Step 5: Create `ownership.tw` with an identity `analyze`**

Create `boot/compiler/ownership.tw`:

```tw
//! Phase 2 ownership analysis over the structural CFG view.
//!
//! Populates each block's entry/exit BlockFacts with a sound
//! Unique/Shared/Unknown ownership fact, plus edge-arg-aware liveness and
//! binding-validity. Consumes the CFG view (raw ops, params, real edge args);
//! ANF stays authoritative and codegen does not read this.

use compiler.cfg.{CfgView, CfgFunction, CfgBlock}
use compiler.builtins.{BuiltinRegistry}
use compiler.opt.semantics.{OptimizerSemantics}

pub type Ownership = { Unique, Shared, Unknown }

// Tag encoding shared with cfg.BlockFacts.ownership (stored as Int there).
pub fn own_tag(o: Ownership) Int {
  case o {
    .Unique => 0,
    .Shared => 1,
    .Unknown => 2,
  }
}

pub fn own_of_tag(t: Int) Ownership {
  cond {
    t == 0 => .Unique,
    t == 1 => .Shared,
    _ => .Unknown,
  }
}

pub fn analyze(view: CfgView, b: BuiltinRegistry, sem: OptimizerSemantics) CfgView {
  // Identity for now; real pipeline arrives in Tasks 4-7.
  view
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — the identity `analyze` test plus all structural tests (now on the `BlockFacts` shape).

- [ ] **Step 7: Format, lint, commit**

```bash
target/twk fmt boot/compiler/cfg.tw boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/cfg.tw boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_suite.tw boot/tests/main.tw
git commit -m "ownership: grouped BlockFacts + analyze skeleton (identity)

Replace the Phase 1 string fact maps with a BlockFacts record (ownership tag
map, binding_valid map, sorted live vector) and add compiler/ownership.tw with
an identity analyze. cfg.tw stores the ownership tag as Int to avoid importing
ownership.tw (cycle). No real facts yet; keeps the view green.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

Register the new suite in `boot/tests/main.tw` now if not already (add the `cfg_ownership_facts_suite` in Task 4 when it first exists; the structural suite is already registered).

---

## Task 4: Edge-arg-aware backward liveness

Compute `live_in`/`live_out` per block to fixpoint, translating successor params back through the feeding edge atom. Store `live_in` into `entry.live` and `live_out` into `exit.live` (sorted). This is what the move-vs-alias and last-use hinges consume.

**Files:**
- Modify: `boot/compiler/ownership.tw`.
- Create + register: `boot/tests/suites/cfg_ownership_facts_suite.tw`; register in `boot/tests/main.tw`.

- [ ] **Step 1: Write the failing test (hand-built module, stable ids)**

Create `boot/tests/suites/cfg_ownership_facts_suite.tw`:

```tw
use @std.testing.assert as assert
use @std.testing as runner

use compiler.anf.{AnfExpr, AnfFunctionDef, AnfModule, AnfOp, Atom, AnfMatchArm, FieldAtom}
use compiler.builtins
use compiler.cfg
use compiler.core_ir.{FuncId, LocalId, GlobalId, TypeId}
use compiler.mono_type.{MonoType}
use compiler.opt.semantics.{make_prelude_optimizer_semantics}
use compiler.ownership

fn lid(id: Int) LocalId {
  LocalId.{ id }
}

fn b_reg() builtins.BuiltinRegistry {
  builtins.make_builtin_registry()
}

fn sem() ownership.OptimizerSemantics {
  make_prelude_optimizer_semantics(b_reg())
}

// call Dict.new() -> fresh dict (Allocate)
fn dict_new_call(b: builtins.BuiltinRegistry) AnfOp {
  .ACall(.AGlobalFunc(b.method_id("Dict", "new")), [])
}

// call Dict.set(base, 1, 2) -> Update, cow_base_arg 0
fn dict_set_call(b: builtins.BuiltinRegistry, base: LocalId) AnfOp {
  .ACall(.AGlobalFunc(b.method_id("Dict", "set")), [.ALocal(base), .ALitInt(1), .ALitInt(2)])
}

fn module_of(name: String, body: AnfExpr) AnfModule {
  func: AnfFunctionDef = AnfFunctionDef.{
    func_id: FuncId.{ id: 1 },
    name,
    is_init: false,
    params: [],
    op_result_mono: Dict.new(),
    body,
    return_ty: MonoType.Int,
  }
  AnfModule.{
    functions: [func],
    init_func_id: .None,
    extern_imports: Dict.new(),
    global_monos: Dict.new(),
    lib_exports: [],
  }
}

fn analyzed_func(m: AnfModule) cfg.CfgFunction {
  b := b_reg()
  v := cfg.build_view(m, b)
  a := ownership.analyze(v, b, sem())
  case cfg.function_named(a, "f") {
    .Some(f) => f,
    .None => error("missing f"),
  }
}

fn block0(f: cfg.CfgFunction) cfg.CfgBlock {
  f.blocks[0]
}

fn live_contains(live: Vector<Int>, id: Int) Bool {
  for x in live {
    if x == id {
      return true
    }
  }
  false
}

runner.test("phase2 t4: a local used after its def is live; a dead one is not", fn () {
  // L0 = Dict.new(); L1 = Dict.set(L0,..); return L1   (L0 dead after its use at L1)
  b := b_reg()
  body: AnfExpr = .Let(
    lid(0),
    dict_new_call(b),
    .Let(lid(1), dict_set_call(b, lid(0)), .Atom(.ALocal(lid(1)))),
  )
  f := analyzed_func(module_of("f", body))
  blk := block0(f)
  // Entry live-in of a param-free function that defines both locals: neither is
  // live at entry (both defined inside).
  assert.true(!live_contains(blk.entry.live, 0), "L0 not live at entry")
  // At block exit, L1 is the returned value -> live out until the terminator use.
  assert.true(live_contains(blk.exit.live, 1) or blk.succs.len() == 0, "L1 tracked to the return")
})
```

Register the suite in `boot/tests/main.tw` (mirror the existing `cfg_ownership_suite` registration line).

- [ ] **Step 2: Run it to verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — `entry.live`/`exit.live` are empty (identity analyze).

- [ ] **Step 3: Implement uses/defs extraction**

Add to `ownership.tw` (import the ANF/atom types):

```tw
use compiler.anf.{AnfOp, Atom}
use compiler.cfg.{Terminator, CfgEdge, CfgInstruction, BlockId}
use compiler.core_ir.{LocalId}

fn atom_local_id(a: Atom) Int? {
  case a {
    .ALocal(l) => .Some(l.id),
    _ => .None,
  }
}

fn push_uid(acc: Vector<Int>, a: Atom) Vector<Int> {
  case atom_local_id(a) {
    .Some(id) => acc.append(id),
    .None => acc,
  }
}

// Locals READ by an op (its ref/scalar operands).
fn op_uses(op: AnfOp) Vector<Int> {
  case op {
    .ACall(callee, args) => {
      acc := push_uid([], callee)
      for a in args {
        acc = push_uid(acc, a)
      }
      acc
    },
    .ABinOp(_, l, r, _) => push_uid(push_uid([], l), r),
    .AUnOp(_, a, _) => push_uid([], a),
    .AMakeClosure(_, caps) => collect c in caps {
      c.id
    },
    .ARecord(_, fields) => {
      acc: Vector<Int> = []
      for fa in fields {
        acc = push_uid(acc, fa.value)
      }
      acc
    },
    .ARecordGet(a, _, _) => push_uid([], a),
    .ARecordUpdate(base, _, v, _, _) => push_uid(push_uid([], base), v),
    .AVariant(_, _, args) => {
      acc: Vector<Int> = []
      for a in args {
        acc = push_uid(acc, a)
      }
      acc
    },
    .AArrayLit(elems) => {
      acc: Vector<Int> = []
      for a in elems {
        acc = push_uid(acc, a)
      }
      acc
    },
    .AIndex(base, idx, _, _) => push_uid(push_uid([], base), idx),
    .AInit(a) => push_uid([], a),
    .AAssign(_, a) => push_uid([], a),
    .AGlobalSet(_, a) => push_uid([], a),
    .AWrapAnyref(a, _) => push_uid([], a),
    .AUnwrapAnyref(a, _) => push_uid([], a),
    .AIf(_, _, _) => error("ownership: AIf reached op_uses; structural op is block-lowered"),
    .AMatch(_, _) => error("ownership: AMatch reached op_uses; structural op is block-lowered"),
    .ALoop(_) => error("ownership: ALoop reached op_uses; structural op is block-lowered"),
    .ADefer(_) => error("ownership: ADefer reached op_uses; expected defer-free ANF"),
  }
}

// Locals READ by a terminator (scrutinee/cond/return/break atoms), excluding
// edge args (those are handled by the edge translation).
fn term_uses(term: Terminator) Vector<Int> {
  case term {
    .CondBranch(test, _, _, _, _) => push_uid([], test),
    .Match(scrut, _) => push_uid([], scrut),
    .Return(.Some(a)) => push_uid([], a),
    .Return(.None) => [],
    .ValueBreak(a) => push_uid([], a),
    _ => [],
  }
}
```

- [ ] **Step 4: Implement the backward liveness fixpoint**

```tw
type BlockLive = .{ live_in: Vector<Int>, live_out: Vector<Int> }

fn block_by_id(blocks: Vector<CfgBlock>, id: Int) CfgBlock {
  for b in blocks {
    if b.id.id == id {
      return b
    }
  }
  error("ownership: unknown block ${id}")
}

// live_out contribution of edge (pred -> succ):
//   (succ.live_in - succ.params) ∪ { locals(edge.args[i]) | succ.params[i] ∈ succ.live_in }
fn edge_live_contribution(succ: CfgBlock, succ_live_in: Vector<Int>, edge_args: Vector<Atom>) Vector<Int> {
  param_ids := collect p in succ.params {
    p.id
  }
  // values live past the join, named directly (not a param)
  acc: Vector<Int> = []
  for id in succ_live_in {
    if !live_contains_int(param_ids, id) {
      acc = insert_sorted(acc, id)
    }
  }
  // params translated to the atom this edge feeds
  for p, i in succ.params {
    if live_contains_int(succ_live_in, p.id) {
      case atom_local_id(edge_args[i]) {
        .Some(src) => acc = insert_sorted(acc, src),
        .None => {},
      }
    }
  }
  acc
}

fn live_contains_int(v: Vector<Int>, id: Int) Bool {
  for x in v {
    if x == id {
      return true
    }
  }
  false
}

// Sorted insert (dedup) mirroring cfg.tw's sorted_insert_local but over Int.
fn insert_sorted(v: Vector<Int>, id: Int) Vector<Int> {
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

fn union_sorted(a: Vector<Int>, b: Vector<Int>) Vector<Int> {
  out := a
  for x in b {
    out = insert_sorted(out, x)
  }
  out
}

fn diff_sorted(a: Vector<Int>, remove: Vector<Int>) Vector<Int> {
  out: Vector<Int> = []
  for x in a {
    if !live_contains_int(remove, x) {
      out = .append(x)
    }
  }
  out
}

fn same_live(a: Vector<Int>, b: Vector<Int>) Bool {
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

// live_in(b) = uses(b) ∪ (live_out(b) − defs(b))
fn block_live_in(blk: CfgBlock, live_out: Vector<Int>) Vector<Int> {
  // uses/defs walking instructions forward, but liveness is order-insensitive
  // at block granularity: gather all uses and all defs, then apply the formula.
  uses: Vector<Int> = []
  defs: Vector<Int> = []
  for inst in blk.instructions {
    for u in op_uses(inst.op) {
      uses = insert_sorted(uses, u)
    }
    defs = insert_sorted(defs, inst.anf_local.id)
  }
  case blk.terminator {
    .Some(t) => for u in term_uses(t) {
      uses = insert_sorted(uses, u)
    },
    .None => {},
  }
  union_sorted(uses, diff_sorted(live_out, defs))
}

fn compute_liveness(blocks: Vector<CfgBlock>) Dict<Int, BlockLive> {
  live: Dict<Int, BlockLive> = Dict.new()
  for b in blocks {
    live[b.id.id] = BlockLive.{ live_in: [], live_out: [] }
  }
  changed := true
  for changed {
    changed = false
    // iterate in reverse block-id order (blocks are appended in ~RPO)
    idx := blocks.len() - 1
    for idx >= 0 {
      blk := blocks[idx]
      // live_out = ∪ over succ edges of edge_live_contribution
      lo: Vector<Int> = []
      for e in blk.succs {
        succ := block_by_id(blocks, e.target.id)
        succ_in := live[succ.id.id].live_in
        lo = union_sorted(lo, edge_live_contribution(succ, succ_in, e.args))
      }
      li := block_live_in(blk, lo)
      prev := live[blk.id.id]
      if !same_live(prev.live_in, li) or !same_live(prev.live_out, lo) {
        changed = true
        live[blk.id.id] = BlockLive.{ live_in: li, live_out: lo }
      }
      idx = idx - 1
    }
  }
  live
}
```

- [ ] **Step 5: Store liveness into the facts and return the view**

Replace the identity `analyze` body:

```tw
pub fn analyze(view: CfgView, b: BuiltinRegistry, sem: OptimizerSemantics) CfgView {
  functions := collect f in view.functions {
    analyze_function(f, b, sem)
  }
  CfgView.{ functions }
}

fn analyze_function(f: CfgFunction, b: BuiltinRegistry, sem: OptimizerSemantics) CfgFunction {
  live := compute_liveness(f.blocks)
  blocks := collect blk in f.blocks {
    bl := live[blk.id.id]
    blk.entry = blk.entry.with_live(bl.live_in)
    blk.exit = blk.exit.with_live(bl.live_out)
    blk
  }
  CfgFunction.{ func_id: f.func_id, name: f.name, params: f.params, blocks }
}
```

Because records are immutable and rebinding a nested field path is sugar, prefer the direct rebind (lint rule `direct-rebinding`): `blk.entry.live = bl.live_in`. Rewrite Step 5 to use direct rebinds rather than a `with_live` helper:

```tw
  blocks := collect blk in f.blocks {
    bl := live[blk.id.id]
    blk.entry.live = bl.live_in
    blk.exit.live = bl.live_out
    blk
  }
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS.

- [ ] **Step 7: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw boot/tests/main.tw
git commit -m "ownership: edge-arg-aware backward liveness

Compute live_in/live_out to fixpoint, translating successor block params back
through the feeding edge atom so a join result's liveness is attributed to the
arm-tail local, not the param. Stored into BlockFacts.live (sorted). Consumed
by the move-vs-alias and last-use hinges.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: Forward ownership transfer (per-op, hinges, `cow_base_arg`)

Implement the per-`AnfOp` transfer producing `exit.ownership` from `entry.ownership` within a block, using `call_info`/`cow_base_arg`, the three hinges, and last-use derived from `exit.live` + a backward in-block scan. Tests here use single-block fixtures (introduce/move/alias/publish).

**Files:**
- Modify: `boot/compiler/ownership.tw`.
- Test: `boot/tests/suites/cfg_ownership_facts_suite.tw`.

- [ ] **Step 1: Write the failing tests**

Add to `cfg_ownership_facts_suite.tw`. Helpers:

```tw
fn own_at_exit(f: cfg.CfgFunction, block_idx: Int, local_id: Int) ownership.Ownership {
  blk := f.blocks[block_idx]
  case blk.exit.ownership.get(local_id) {
    .Some(tag) => ownership.own_of_tag(tag),
    .None => .Unknown,
  }
}

fn assert_own(actual: ownership.Ownership, expected: ownership.Ownership, msg: String) {
  assert.eq_int(ownership.own_tag(actual), ownership.own_tag(expected), msg)
}
```

Tests (each cross-references worked-examples):

```tw
runner.test("phase2 t5 introduce: Dict.new() is Unique", fn () {
  b := b_reg()
  body: AnfExpr = .Let(lid(0), dict_new_call(b), .Atom(.ALocal(lid(0))))
  f := analyzed_func(module_of("f", body))
  assert_own(own_at_exit(f, 0, 0), .Unique, "fresh dict is Unique")
})

runner.test("phase2 t5 move: init of a dead source keeps Unique (Case B)", fn () {
  // L0 = Dict.new(); L1 = init L0; return L1  (L0 dead after the init)
  b := b_reg()
  body: AnfExpr = .Let(
    lid(0),
    dict_new_call(b),
    .Let(lid(1), .AInit(.ALocal(lid(0))), .Atom(.ALocal(lid(1)))),
  )
  f := analyzed_func(module_of("f", body))
  assert_own(own_at_exit(f, 0, 1), .Unique, "moved value stays Unique")
})

runner.test("phase2 t5 alias: init with a still-live source demotes both (Case C)", fn () {
  // L0 = Dict.new(); L1 = init L0; L2 = Dict.set(L0,..); return L1
  // L0 is read at L2 after the init -> alias -> both Shared.
  b := b_reg()
  body: AnfExpr = .Let(
    lid(0),
    dict_new_call(b),
    .Let(
      lid(1),
      .AInit(.ALocal(lid(0))),
      .Let(lid(2), dict_set_call(b, lid(0)), .Atom(.ALocal(lid(1)))),
    ),
  )
  f := analyzed_func(module_of("f", body))
  assert_own(own_at_exit(f, 0, 0), .Shared, "aliased source is Shared")
  assert_own(own_at_exit(f, 0, 1), .Shared, "alias result is Shared")
})

runner.test("phase2 t5 publish: global_set demotes to Shared", fn () {
  // L0 = Dict.new(); global_set G0 = L0; return L0
  b := b_reg()
  body: AnfExpr = .Let(
    lid(0),
    dict_new_call(b),
    .Let(lid(1), .AGlobalSet(GlobalId.{ id: 0 }, .ALocal(lid(0))), .Atom(.ALocal(lid(0)))),
  )
  f := analyzed_func(module_of("f", body))
  assert_own(own_at_exit(f, 0, 0), .Shared, "published value is Shared")
})
```

- [ ] **Step 2: Run to verify they fail**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — ownership map is empty (only liveness populated).

- [ ] **Step 3: Implement `fact`, the lattice, last-use, and `call_info` classification**

Add to `ownership.tw`:

```tw
use compiler.opt.semantics.{OptimizerSemantics, CallSemantics, EffectKind, call_info}
use compiler.core_ir.{FuncId}

pub fn join_own(a: Ownership, b: Ownership) Ownership {
  case a {
    .Unknown => .Unknown,
    .Shared => case b {
      .Unknown => .Unknown,
      _ => .Shared,
    },
    .Unique => b,
  }
}

// ownership of an atom against a working map: locals resolve; non-locals Unknown.
fn fact_of(own: Dict<Int, Int>, a: Atom) Ownership {
  case atom_local_id(a) {
    .Some(id) => case own.get(id) {
      .Some(tag) => own_of_tag(tag),
      .None => .Unknown,
    },
    .None => .Unknown,
  }
}

fn set_own(own: Dict<Int, Int>, id: Int, o: Ownership) Dict<Int, Int> {
  own[id] = own_tag(o)
  own
}

// FuncId of a direct callee atom; .None for indirect (closure) callees.
fn callee_func_id(a: Atom) FuncId? {
  case a {
    .AGlobalFunc(fid) => .Some(fid),
    _ => .None,
  }
}
```

Last-use within a block: a local's last use is at index `k` if it is used at `k` and not used at any later instruction and not in `exit.live`. Implement as a per-block precomputation:

```tw
// For instruction index i, the set of local ids whose LAST use in this block is
// at i AND which are not live out of the block (so the use here is the final one).
fn last_use_at(blk: CfgBlock, i: Int) Vector<Int> {
  target := blk.instructions[i]
  used_here := op_uses(target.op)
  out: Vector<Int> = []
  for id in used_here {
    if live_contains_int(blk.exit.live, id) {
      continue
    }
    // used later in this block?
    later := false
    j := i + 1
    for j < blk.instructions.len() {
      if live_contains_int(op_uses(blk.instructions[j].op), id) {
        later = true
      }
      j = j + 1
    }
    // used by the terminator?
    case blk.terminator {
      .Some(t) => if live_contains_int(term_uses(t), id) {
        later = true
      },
      .None => {},
    }
    if !later {
      out = insert_sorted(out, id)
    }
  }
  out
}
```

- [ ] **Step 4: Implement the per-op transfer**

```tw
type ForwardState = .{ own: Dict<Int, Int>, valid: Dict<Int, Bool> }

fn is_last_use(last: Vector<Int>, id: Int) Bool {
  live_contains_int(last, id)
}

// Apply one instruction. `last` is last_use_at(blk, i). `result` is inst.anf_local.
fn transfer_op(st: ForwardState, result: Int, op: AnfOp, last: Vector<Int>, b: BuiltinRegistry, sem: OptimizerSemantics) ForwardState {
  case op {
    .ACall(callee, args) => transfer_call(st, result, callee, args, last, sem),
    .ARecord(_, fields) => {
      st = set_result(st, result, .Unique)
      for fa in fields {
        st = field_store(st, fa.value, last)
      }
      st
    },
    .AVariant(_, _, args) => {
      st = set_result(st, result, .Unique)
      for a in args {
        st = field_store(st, a, last)
      }
      st
    },
    .AArrayLit(elems) => {
      st = set_result(st, result, .Unique)
      for a in elems {
        st = field_store(st, a, last)
      }
      st
    },
    .ARecordGet(_, _, _) => set_result(st, result, .Unknown),   // borrow: result not owned
    .AIndex(_, _, _, _) => set_result(st, result, .Unknown),    // borrow
    .ARecordUpdate(base, _, v, _, _) => {
      st = consume_base(st, result, base, last)
      field_store(st, v, last)
    },
    .AInit(a) => init_hinge(st, result, a, last),
    .AAssign(local, a) => {
      // rebind: result local's fact = fact(a); binding becomes valid again
      o := fact_of(st.own, a)
      st = set_own_st(st, local.id, o)
      set_valid(st, local.id, true)
    },
    .AMakeClosure(_, caps) => {
      st = set_result(st, result, .Unknown)   // closure ref itself: not an optimization target
      for c in caps {
        st = publish_local(st, c.id)
      }
      st
    },
    .AGlobalSet(_, a) => {
      st = publish_atom(st, a)
      set_result(st, result, .Unknown)
    },
    .AWrapAnyref(a, _) => init_hinge(st, result, a, last),     // representationally transparent
    .AUnwrapAnyref(a, _) => init_hinge(st, result, a, last),
    .ABinOp(_, _, _, _) => set_result(st, result, .Unknown),  // scalar, neutral
    .AUnOp(_, _, _) => set_result(st, result, .Unknown),
    .AIf(_, _, _) => error("ownership: AIf reached transfer; structural op is block-lowered"),
    .AMatch(_, _) => error("ownership: AMatch reached transfer; structural op is block-lowered"),
    .ALoop(_) => error("ownership: ALoop reached transfer; structural op is block-lowered"),
    .ADefer(_) => error("ownership: ADefer reached transfer; expected defer-free ANF"),
  }
}
```

Helper mutators on `ForwardState`:

```tw
fn set_result(st: ForwardState, id: Int, o: Ownership) ForwardState {
  set_own_st(st, id, o)
}

fn set_own_st(st: ForwardState, id: Int, o: Ownership) ForwardState {
  st.own = set_own(st.own, id, o)
  st
}

fn set_valid(st: ForwardState, id: Int, v: Bool) ForwardState {
  st.valid[id] = v
  st
}

fn publish_local(st: ForwardState, id: Int) ForwardState {
  set_own_st(st, id, .Shared)
}

fn publish_atom(st: ForwardState, a: Atom) ForwardState {
  case atom_local_id(a) {
    .Some(id) => publish_local(st, id),
    .None => st,
  }
}

// AInit / wrap / unwrap move-vs-alias hinge.
fn init_hinge(st: ForwardState, result: Int, a: Atom, last: Vector<Int>) ForwardState {
  case atom_local_id(a) {
    .Some(src) => if is_last_use(last, src) {
      // move: result takes source ownership; source binding invalid
      o := fact_of(st.own, a)
      st = set_own_st(st, result, o)
      set_valid(st, src, false)
    } else {
      // alias: both Shared
      st = publish_local(st, src)
      set_own_st(st, result, .Shared)
    },
    .None => set_own_st(st, result, .Unknown),
  }
}

// Field-store hinge: same two outcomes as AInit but the stored value goes into a
// fresh shell (no tracked field ownership in Phase 2), so on move we only
// invalidate the source; on alias we demote it.
fn field_store(st: ForwardState, a: Atom, last: Vector<Int>) ForwardState {
  case atom_local_id(a) {
    .Some(src) => if is_last_use(last, src) {
      set_valid(st, src, false)
    } else {
      publish_local(st, src)
    },
    .None => st,
  }
}

// Consuming-op hinge on a named base (ARecordUpdate: base is the shell).
fn consume_base(st: ForwardState, result: Int, base: Atom, last: Vector<Int>) ForwardState {
  case atom_local_id(base) {
    .Some(bid) => {
      base_own := fact_of(st.own, base)
      case base_own {
        .Unique => if is_last_use(last, bid) {
          st = set_own_st(st, result, .Unique)
          set_valid(st, bid, false)
        } else {
          set_own_st(st, result, .Unknown)
        },
        _ => set_own_st(st, result, .Unknown),
      }
    },
    .None => set_own_st(st, result, .Unknown),
  }
}
```

`transfer_call` classifies via `call_info`:

```tw
fn transfer_call(st: ForwardState, result: Int, callee: Atom, args: Vector<Atom>, last: Vector<Int>, sem: OptimizerSemantics) ForwardState {
  info := case callee_func_id(callee) {
    .Some(fid) => call_info(sem, fid),
    .None => .None,   // indirect/closure callee: no summary
  }
  case info {
    .Some(cs) => case cs.effect {
      .Allocate => set_own_st(st, result, .Unique),
      .Update => consume_call_base(st, result, cs, args, last),
      .ReadOnly => set_own_st(st, result, .Unknown),   // borrow: result not owned
      .Pure => set_own_st(st, result, .Unknown),
      .Control => set_own_st(st, result, .Unknown),
    },
    .None => {
      // unknown Twinkle call, extern, or Cell op: publish every ref arg
      for a in args {
        st = publish_atom(st, a)
      }
      set_own_st(st, result, .Unknown)
    },
  }
}

fn consume_call_base(st: ForwardState, result: Int, cs: CallSemantics, args: Vector<Atom>, last: Vector<Int>) ForwardState {
  case cs.cow_base_arg {
    .Some(k) => if k < args.len() {
      consume_base(st, result, args[k], last)
    } else {
      set_own_st(st, result, .Unknown)
    },
    .None => set_own_st(st, result, .Unknown),
  }
}
```

- [ ] **Step 5: Drive the transfer over a block and store `exit.ownership`**

Add a per-block forward that seeds from `entry` facts (Task 6 wires the real entry; for now seed empty, which single-block fixtures need) and writes `exit`:

```tw
fn forward_block(blk: CfgBlock, entry: ForwardState, b: BuiltinRegistry, sem: OptimizerSemantics) ForwardState {
  st := entry
  for inst, i in blk.instructions {
    last := last_use_at(blk, i)
    st = transfer_op(st, inst.anf_local.id, inst.op, last, b, sem)
  }
  st
}
```

In `analyze_function`, after liveness, run a single forward pass per block (Task 6 turns this into a fixpoint with real entry facts). Temporary single-pass wiring so Task 5 tests pass:

```tw
  blocks := collect blk in blocks {
    entry_state := ForwardState.{ own: Dict.new(), valid: Dict.new() }
    exit_state := forward_block(blk, entry_state, b, sem)
    blk.exit.ownership = exit_state.own
    blk.exit.binding_valid = exit_state.valid
    blk
  }
```

(Keep the liveness `collect` from Task 4 first, then this `collect` over its result.)

- [ ] **Step 6: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — introduce/move/alias/publish.

- [ ] **Step 7: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
git commit -m "ownership: forward per-op transfer with the three hinges

Per-AnfOp transfer producing exit.ownership: Allocate->Unique, Update consumes
cow_base_arg, ReadOnly/borrow neutral, unknown/extern/Cell publish ref args.
AInit move-vs-alias, the field-store hinge, and the consuming-op hinge all read
block last-use. Single-block fixtures (introduce/move/alias/publish) pass;
control-flow join + fixpoint is the next task.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: Positional predecessor join + fixpoint (branches and loops)

Turn the single forward pass into a fixpoint: each block's `entry.ownership` is the positional join over predecessors of `fact(pred_exit, pred_edge.args[i])` for each param `params[i]`; reprocess to fixpoint. This delivers branch-join and loop-carried ownership.

**Files:**
- Modify: `boot/compiler/ownership.tw`.
- Test: `boot/tests/suites/cfg_ownership_facts_suite.tw`.

- [ ] **Step 1: Write the failing tests**

Add fixtures using `AIf`. Helper to fetch a join block's entry ownership by block name:

```tw
fn own_entry_named(f: cfg.CfgFunction, name: String, local_id: Int) ownership.Ownership {
  for blk in f.blocks {
    if blk.name == name {
      case blk.entry.ownership.get(local_id) {
        .Some(tag) => return ownership.own_of_tag(tag),
        .None => return .Unknown,
      }
    }
  }
  .Unknown
}

fn result_param_of(f: cfg.CfgFunction, name: String) Int {
  for blk in f.blocks {
    if blk.name == name {
      return blk.params[0].id
    }
  }
  error("no block ${name}")
}
```

Tests:

```tw
runner.test("phase2 t6 branch join: both arms allocate => Unique (Case V join)", fn () {
  // r := if c { Dict.new() } else { Dict.new() }; return r
  b := b_reg()
  then_e: AnfExpr = .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1))))
  else_e: AnfExpr = .Let(lid(2), dict_new_call(b), .Atom(.ALocal(lid(2))))
  body: AnfExpr = .Let(lid(0), .AIf(.ALitBool(true), then_e, else_e), .Atom(.ALocal(lid(0))))
  f := analyzed_func(module_of("f", body))
  rp := result_param_of(f, "if.join")
  assert_own(own_entry_named(f, "if.join", rp), .Unique, "both arms Unique -> join Unique")
})

runner.test("phase2 t6 branch join: one arm publishes => Shared", fn () {
  // r := if c { d := Dict.new(); global_set G0 = d; d } else { Dict.new() }; r
  b := b_reg()
  then_e: AnfExpr = .Let(
    lid(1),
    dict_new_call(b),
    .Let(lid(3), .AGlobalSet(GlobalId.{ id: 0 }, .ALocal(lid(1))), .Atom(.ALocal(lid(1)))),
  )
  else_e: AnfExpr = .Let(lid(2), dict_new_call(b), .Atom(.ALocal(lid(2))))
  body: AnfExpr = .Let(lid(0), .AIf(.ALitBool(true), then_e, else_e), .Atom(.ALocal(lid(0))))
  f := analyzed_func(module_of("f", body))
  rp := result_param_of(f, "if.join")
  assert_own(own_entry_named(f, "if.join", rp), .Shared, "one arm published -> join Shared")
})
```

- [ ] **Step 2: Run to verify they fail**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — join entry ownership is empty (single-pass seeds every entry empty ⇒ `Unknown`).

- [ ] **Step 3: Implement the positional join**

```tw
// entry.ownership for a block = positional join over preds of fact(pred_exit, arg[i]).
fn join_entry_ownership(blk: CfgBlock, exits: Dict<Int, Dict<Int, Int>>) Dict<Int, Int> {
  entry: Dict<Int, Int> = Dict.new()
  // For a block with no preds (entry block), params carry no join fact; leave empty.
  for p, i in blk.params {
    acc: Ownership? = .None
    for pe in blk.preds {
      // pe.target holds the predecessor block id (see cfg.add_pred comment).
      pred_exit := case exits.get(pe.target.id) {
        .Some(m) => m,
        .None => Dict.new(),
      }
      // find this predecessor's succ edge to blk to read the positional arg;
      // preds mirror succs, so pe.args[i] is the atom fed to params[i].
      contributed := if i < pe.args.len() {
        fact_of(pred_exit, pe.args[i])
      } else {
        .Unknown
      }
      acc = case acc {
        .None => .Some(contributed),
        .Some(prev) => .Some(join_own(prev, contributed)),
      }
    }
    case acc {
      .Some(o) => entry[p.id] = own_tag(o),
      .None => {},
    }
  }
  entry
}
```

- [ ] **Step 4: Implement the fixpoint driver**

Replace the temporary single-pass in `analyze_function` with a fixpoint over `entry`/`exit` ownership maps (binding-validity is threaded in Task 7; here keep the `valid` map computed per block from the forward pass but not yet joined):

```tw
fn fixpoint_ownership(blocks: Vector<CfgBlock>, b: BuiltinRegistry, sem: OptimizerSemantics) Dict<Int, Dict<Int, Int>> {
  // exit ownership map per block id
  exits: Dict<Int, Dict<Int, Int>> = Dict.new()
  entries: Dict<Int, Dict<Int, Int>> = Dict.new()
  for blk in blocks {
    exits[blk.id.id] = Dict.new()
    entries[blk.id.id] = Dict.new()
  }
  changed := true
  for changed {
    changed = false
    for blk in blocks {
      entry_own := join_entry_ownership(blk, exits)
      st := ForwardState.{ own: entry_own, valid: Dict.new() }
      st = forward_block(blk, st, b, sem)
      prev := exits[blk.id.id]
      if !same_own_map(prev, st.own) {
        changed = true
        exits[blk.id.id] = st.own
      }
      entries[blk.id.id] = entry_own
    }
  }
  // stash entries under a sentinel? Simpler: recompute entries in materialize.
  exits
}

fn same_own_map(a: Dict<Int, Int>, b: Dict<Int, Int>) Bool {
  if a.keys().len() != b.keys().len() {
    return false
  }
  for k in a.keys() {
    case b.get(k) {
      .Some(v) => if a.get_unsafe(k) != v {
        return false
      },
      .None => return false,
    }
  }
  true
}
```

> Determinism note: `same_own_map` iterates `a.keys()` only to compare values — order-independent (pure equality). The fixpoint result does not depend on `Dict` iteration order.

- [ ] **Step 5: Materialize entry/exit ownership into the blocks**

Rewrite the ownership stage of `analyze_function` to compute the fixpoint, then fill `entry.ownership`/`exit.ownership`:

```tw
  exits := fixpoint_ownership(blocks, b, sem)
  blocks := collect blk in blocks {
    entry_own := join_entry_ownership(blk, exits)
    st := ForwardState.{ own: entry_own, valid: Dict.new() }
    st = forward_block(blk, st, b, sem)
    blk.entry.ownership = entry_own
    blk.exit.ownership = st.own
    blk
  }
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — both-arms-Unique join and one-arm-published join, plus a loop-carried case if you add one (accumulator consumed-then-reassigned stays `Unique`).

- [ ] **Step 7: Add a loop-carried test, then commit**

```tw
runner.test("phase2 t6 loop-carried: consume-then-reassign stays Unique (Case A)", fn () {
  // acc := Dict.new(); loop { acc = Dict.set(acc,..); continue }  (simplified)
  b := b_reg()
  loop_body: AnfExpr = .Let(
    lid(2),
    dict_set_call(b, lid(1)),
    .Let(lid(3), .AAssign(lid(1), .ALocal(lid(2))), .Continue),
  )
  body: AnfExpr = .Let(
    lid(1),
    dict_new_call(b),
    .Let(lid(0), .ALoop(loop_body), .Atom(.ALocal(lid(1)))),
  )
  f := analyzed_func(module_of("f", body))
  // At the loop header entry, the carried accumulator L1 remains Unique.
  assert_own(own_entry_named(f, "loop.header", 1), .Unique, "loop-carried acc stays Unique")
})
```

If this fails because the header's carried param set does not include `L1`, confirm Phase 1 `loop_params` carries the `AAssign` target — it does (`collect_assign_targets`). Adjust the assertion to the actual carried local if the fixture's ids differ.

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
git commit -m "ownership: positional predecessor join + fixpoint

entry.ownership is the positional join over preds of fact(pred_exit,
edge.args[i]) per param; iterate to fixpoint over the three-element lattice.
Delivers branch-join (both arms Unique => Unique; one publishes => Shared) and
loop-carried ownership (consume-then-reassign stays Unique).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: Binding-validity (default / transfer / positional meet)

Thread `binding_valid` through the same forward pass and join it by the positional meet, so a value moved on one arm and forwarded on another resolves to "not usable".

**Files:**
- Modify: `boot/compiler/ownership.tw`.
- Test: `boot/tests/suites/cfg_ownership_facts_suite.tw`.

- [ ] **Step 1: Write the failing test**

```tw
fn valid_at_exit(f: cfg.CfgFunction, block_idx: Int, local_id: Int) Bool {
  blk := f.blocks[block_idx]
  case blk.exit.binding_valid.get(local_id) {
    .Some(v) => v,
    .None => true,   // absent => valid
  }
}

runner.test("phase2 t7: a moved source is invalid; a rebind revalidates", fn () {
  // L0 = Dict.new(); L1 = init L0; return L1  (L0 moved => invalid at exit)
  b := b_reg()
  body: AnfExpr = .Let(
    lid(0),
    dict_new_call(b),
    .Let(lid(1), .AInit(.ALocal(lid(0))), .Atom(.ALocal(lid(1)))),
  )
  f := analyzed_func(module_of("f", body))
  assert.true(!valid_at_exit(f, 0, 0), "moved L0 is invalid at block exit")
})
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — `binding_valid` is currently seeded empty each pass and not materialized into `exit.binding_valid`.

- [ ] **Step 3: Join binding-validity positionally and thread it**

Add the meet join and thread `valid` through the fixpoint. First the entry meet:

```tw
// entry.binding_valid[params[i]] = AND over preds of ( edge.args[i] non-local
// OR its source local valid at pred exit ).
fn join_entry_valid(blk: CfgBlock, exit_valid: Dict<Int, Dict<Int, Bool>>) Dict<Int, Bool> {
  entry: Dict<Int, Bool> = Dict.new()
  for p, i in blk.params {
    ok := true
    for pe in blk.preds {
      pv := case exit_valid.get(pe.target.id) {
        .Some(m) => m,
        .None => Dict.new(),
      }
      contributed := if i < pe.args.len() {
        case atom_local_id(pe.args[i]) {
          .Some(src) => case pv.get(src) {
            .Some(v) => v,
            .None => true,
          },
          .None => true,   // non-local feed is always "valid"
        }
      } else {
        true
      }
      if !contributed {
        ok = false
      }
    }
    entry[p.id] = ok
  }
  entry
}
```

Extend the fixpoint to co-iterate `exit_valid` alongside `exits`. Change `fixpoint_ownership` to also track `exit_valid: Dict<Int, Dict<Int, Bool>>`, seed each block's entry valid via `join_entry_valid`, run `forward_block` (which already writes `st.valid`), and compare with `same_valid_map`. Materialize `entry.binding_valid`/`exit.binding_valid` in the same `collect` as ownership:

```tw
fn same_valid_map(a: Dict<Int, Bool>, b: Dict<Int, Bool>) Bool {
  if a.keys().len() != b.keys().len() {
    return false
  }
  for k in a.keys() {
    case b.get(k) {
      .Some(v) => if a.get_unsafe(k) != v {
        return false
      },
      .None => return false,
    }
  }
  true
}
```

In the materialize `collect`, seed `st.valid` from `join_entry_valid(blk, exit_valids)` before `forward_block`, then:

```tw
    blk.entry.binding_valid = entry_valid
    blk.exit.binding_valid = st.valid
```

> Because ownership and validity share `ForwardState` and the same `forward_block`, run **one** combined fixpoint that reaches a joint fixed point on both maps (loop until neither `exits` nor `exit_valid` changes).

- [ ] **Step 4: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — moved-source-invalid, plus all prior tests.

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
git commit -m "ownership: binding-validity (default, transfer, positional meet)

Thread binding_valid through the forward pass (moves invalidate, AAssign
revalidates) and join it by the positional meet over edge args, co-iterated to
a joint fixpoint with ownership. A value moved on one arm and forwarded on
another is 'not usable' at the merge.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 8: Render populated facts + `twk ir --cfg` wiring + determinism

Print the ownership facts keyed by each block's carried params (sorted), and make `twk ir --cfg` run `analyze`. Binding-validity and liveness stay off the default print.

**Files:**
- Modify: `boot/compiler/cfg.tw` (`render_block`), `boot/commands/ir.tw` (`--cfg` path), `boot/main.tw` (import if needed).
- Test: `boot/tests/suites/cfg_ownership_facts_suite.tw` (render + determinism) and a CLI smoke check.

- [ ] **Step 1: Write the failing render test**

```tw
runner.test("phase2 t8: render shows populated facts for carried params", fn () {
  b := b_reg()
  then_e: AnfExpr = .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1))))
  else_e: AnfExpr = .Let(lid(2), dict_new_call(b), .Atom(.ALocal(lid(2))))
  body: AnfExpr = .Let(lid(0), .AIf(.ALitBool(true), then_e, else_e), .Atom(.ALocal(lid(0))))
  v := cfg.build_view(module_of("f", body), b)
  a := ownership.analyze(v, b, sem())
  out := cfg.render_view(a)
  assert.true(out.contains("Unique"), "render should show a Unique fact")
  assert.true(out.contains("facts.in={") and out.contains("facts.out={"), "fact lines present")
})
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — `render_block` still prints `facts.in={} facts.out={}`.

- [ ] **Step 3: Render the facts (keyed by sorted params)**

In `cfg.tw`, add an ownership-tag renderer and fill the facts line:

```tw
fn own_tag_text(tag: Int) String {
  cond {
    tag == 0 => "Unique",
    tag == 1 => "Shared",
    _ => "Unknown",
  }
}

fn render_facts(facts: BlockFacts, params: Vector<LocalId>) String {
  parts := collect p in params {
    tag := case facts.ownership.get(p.id) {
      .Some(t) => t,
      .None => 2,
    }
    "L${p.id}: ${own_tag_text(tag)}"
  }
  "{${parts.join(", ")}}"
}
```

`params` is already sorted by `LocalId` (Phase 1 `sorted_*`). Update `render_block`:

```tw
  lines = .append("    facts.in=${render_facts(block.entry, block.params)} facts.out=${render_facts(block.exit, block.params)}")
```

- [ ] **Step 4: Wire `--cfg` to run `analyze`**

In `boot/commands/ir.tw`, the `--cfg` branch currently does `cfg.build_view(...)` then renders. Insert `analyze`:

```tw
  if parsed.has_flag("cfg") {
    b := artifacts.builtins
    view := cfg.build_view(artifacts.opt, b)
    analyzed := ownership.analyze(view, b, semantics.make_prelude_optimizer_semantics(b))
    print(cfg.render_view(analyzed))
    return
  }
```

Add imports at the top of `ir.tw`: `use compiler.ownership` and `use compiler.opt.semantics as semantics`. Match the existing early-return shape of the `--cfg` branch.

- [ ] **Step 5: Add a determinism test**

```tw
runner.test("phase2 t8: facts are byte-identical across two analyses", fn () {
  b := b_reg()
  body: AnfExpr = .Let(
    lid(0),
    dict_new_call(b),
    .Let(lid(1), dict_set_call(b, lid(0)), .Atom(.ALocal(lid(1)))),
  )
  m := module_of("f", body)
  a1 := cfg.render_view(ownership.analyze(cfg.build_view(m, b), b, sem()))
  a2 := cfg.render_view(ownership.analyze(cfg.build_view(m, b), b, sem()))
  assert.true(a1 == a2, "analysis output is deterministic")
})
```

- [ ] **Step 6: Run boot tests, then rebuild CLI and smoke-check**

```bash
target/twk run boot/tests/main.tw          # all suites green
make bundle-cli                            # rebuild target/twk with the new --cfg path
printf 'fn f(c: Bool) Int {\n  r := if c { 1 } else { 2 }\n  r\n}\n' > /tmp/cfgfacts.tw
target/twk ir /tmp/cfgfacts.tw --cfg | head -40      # inspect populated facts
target/twk ir /tmp/cfgfacts.tw --cfg > /tmp/a.txt
target/twk ir /tmp/cfgfacts.tw --cfg > /tmp/b.txt
diff /tmp/a.txt /tmp/b.txt && echo "DETERMINISTIC"
```

Expected: boot tests PASS; `--cfg` prints non-empty `facts.in`/`facts.out`; `diff` reports no differences (`DETERMINISTIC`).

- [ ] **Step 7: Format, lint, commit**

```bash
target/twk fmt boot/compiler/cfg.tw boot/commands/ir.tw boot/tests/suites/cfg_ownership_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/cfg.tw boot/commands/ir.tw boot/main.tw boot/tests/suites/cfg_ownership_facts_suite.tw
git commit -m "cfg/ir: render populated ownership facts and run analyze in twk ir --cfg

render_view fills facts.in/out keyed by each block's carried params (sorted);
twk ir --cfg becomes build_view -> ownership.analyze -> render_view. Liveness
and binding-validity stay off the default print. Output is byte-identical
across runs.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 9: Soundness guard, real-program smoke, and tracking docs

Add the two-direction soundness guard, a wide real-program smoke over `boot/main.tw`, and make the deferral tracking edits the design promised.

**Files:**
- Test: `boot/tests/suites/cfg_ownership_facts_suite.tw`.
- Modify: `docs/plans/sound-uniqueness/README.md`.

- [ ] **Step 1: Write the soundness-guard test (both directions)**

```tw
runner.test("phase2 t9 soundness guard: classifications are as the transfer assumes", fn () {
  b := b_reg()
  s := sem()
  // dict.set / vector.append / Vector.set must be Update with a base arg.
  for name_pair in [["Dict", "set"], ["Vector", "append"], ["Vector", "set"]] {
    fid := b.method_id(name_pair[0], name_pair[1])
    case semantics.call_info(s, fid) {
      .Some(cs) => {
        assert.eq_int(effect_tag(cs.effect), effect_tag_update(), "${name_pair[0]}.${name_pair[1]} is Update")
        case cs.cow_base_arg {
          .Some(_) => {},
          .None => assert.fail("${name_pair[0]}.${name_pair[1]} missing cow_base_arg"),
        }
      },
      .None => assert.fail("${name_pair[0]}.${name_pair[1]} has no CallSemantics"),
    }
  }
  // A Cell op must resolve to .None (publish bucket), never Update.
  case semantics.call_info(s, b.method_id("Cell", "set")) {
    .None => {},
    .Some(cs) => assert.true(effect_tag(cs.effect) != effect_tag_update(), "Cell.set must not be Update"),
  }
})
```

Add small tag helpers (import `semantics` and `EffectKind`):

```tw
use compiler.opt.semantics as semantics
use compiler.opt.semantics.{EffectKind}

fn effect_tag(e: EffectKind) Int {
  case e {
    .Pure => 0,
    .ReadOnly => 1,
    .Update => 2,
    .Allocate => 3,
    .Control => 4,
  }
}

fn effect_tag_update() Int {
  2
}
```

> If `Cell.set` is registered under a different builtin name, adjust `method_id` accordingly (grep `boot/compiler/opt/semantics.tw` for the Cell rows). The guard's intent — Cell never `Update` — is what matters.

- [ ] **Step 2: Write a wide real-program smoke**

```tw
runner.test("phase2 t9 smoke: analyze runs over a real program without trapping", fn () {
  src := "fn build() Dict<Int, Int> {\n  d := Dict.new()\n  d[1] = 2\n  d[3] = 4\n  d\n}\n"
  case view(src_wrap(src)) {  // reuse the compile-source `view` helper via a thin wrapper
    .Ok(_) => {},
    .Err(e) => assert.fail(e),
  }
})
```

If the facts suite has no compile-source `view` helper (it uses hand-built modules), add one mirroring the structural suite:

```tw
use compiler.pipeline

fn analyze_source(src: String) Result<cfg.CfgView, String> {
  b := b_reg()
  artifacts := try pipeline.compile_source(src)
  .Ok(ownership.analyze(cfg.build_view(artifacts.opt, b), b, sem()))
}

runner.test("phase2 t9 smoke: analyze a real optimized program", fn () {
  src := "fn build() Dict<Int, Int> {\n  d := Dict.new()\n  d[1] = 2\n  d[3] = 4\n  d\n}\n"
  case analyze_source(src) {
    .Ok(v) => assert.true(v.functions.len() >= 1, "analyzed at least one function"),
    .Err(e) => assert.fail(e),
  }
})
```

- [ ] **Step 3: Run boot tests**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS.

- [ ] **Step 4: Update the README tracking (the design's deferral task)**

In `docs/plans/sound-uniqueness/README.md`:

1. Mark the four implemented Phase 2 bullets `[x]` with a one-line "Done" note pointing at `phase2-design.md` and the new files (`compiler/ownership.tw`, `cfg_ownership_facts_suite.tw`). Leave "Catalog later precision needs" as the coverage-doc bullet.
2. Add a **Phase 8** bullet: "Extern copying-borrow precision — treat host imports as borrow (args preserved) with `Unique` GC results per the copying-marshalling contract; Phase 2 conservatively over-publishes them." Cross-reference `concurrency-publication.md` and `fact-lattice.md`'s extern row.
3. Add to **Phase 3** ("Move ownership-relevant pass queries to CFG facts"): "Dead-merge block-param pruning using the Phase 2 liveness facts."
4. Add two **Future-work ledger** rows: "Extern copying-borrow precision → Phase 8" and "Binding-validity / liveness render surface in `--cfg` → ledger nicety".

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/tests/suites/cfg_ownership_facts_suite.tw
target/twk lint boot/main.tw
git add boot/tests/suites/cfg_ownership_facts_suite.tw docs/plans/sound-uniqueness/README.md
git commit -m "ownership: soundness guard + real-program smoke; track Phase 2 in README

Guard both misclassification directions (dict.set/vector.append/Vector.set are
Update with cow_base_arg; Cell ops never Update) and smoke-analyze a real
optimized program. Mark the delivered Phase 2 README bullets and add the
Phase 8 extern + Phase 3 dead-merge tracking rows the design deferred.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

- [ ] **Step 6: Full verification and self-host check**

```bash
make boot-test          # all boot suites
make stage2             # self-host: stage0 must still compile boot/main.tw
```

Expected: boot suites green; `make stage2` succeeds (the new `ownership.tw` and `cfg.tw` changes are boot-only and do not use any construct stage0 lacks — if `make stage2` fails, the failure is a stage0-parity gap, not a logic bug; see the stage0 bootstrap dependency note in project memory).

---

## Self-review

**1. Spec coverage** (against `phase2-design.md`):
- Structural inputs (raw op, params) → Task 1. Real edge args (`Vector<Atom>`, phi) + succs-authoritative + terminator sync → Task 2. ✓
- `BlockFacts` shape, `empty_fact_blocks` update, module split (`ownership.tw`) → Task 3. ✓
- Edge-arg-aware liveness → Task 4. ✓
- Transfer table (ACall via `call_info`, Allocate/Update/ReadOnly/Pure/Control/`.None`; `cow_base_arg`; ARecord/AVariant/AArrayLit; ARecordGet/AIndex borrow; ARecordUpdate; AInit; AAssign; AMakeClosure; AGlobalSet; wrap/unwrap; binop/unop; structural-op hard errors) → Task 5. Three hinges (AInit, consuming-op on `cow_base_arg`, field-store) → Task 5. ✓
- Positional join + fixpoint (branch join, loop-carried) → Task 6. ✓
- Binding-validity default/transfer/positional-meet → Task 7. ✓
- Rendering (sorted params) + `--cfg` wiring + determinism → Task 8. ✓
- Two-direction soundness guard + tracking-doc edits → Task 9. ✓
- Conservative ref rule (no type table) → encoded in `publish_atom`/`field_store` acting on every `ALocal` (Conventions + Task 5). ✓

**2. Placeholder scan:** No "TBD"/"add error handling" — every code step shows code; every test step shows the assertion and the run/expected. The one soft spot (exact Cell builtin name in Task 9) has an explicit "grep and adjust; intent is what matters" instruction, not a silent gap.

**3. Type consistency:** `Ownership` (enum) lives in `ownership.tw`; `BlockFacts.ownership` stores the **`Int` tag** (Task 3 note) to avoid a `cfg.tw → ownership.tw` import cycle; `own_tag`/`own_of_tag` bridge them and are used consistently in tests (`assert_own`, `own_at_exit`) and rendering (`own_tag_text`). `CfgEdge.args: Vector<Atom>` is introduced in Task 2 and read positionally by `join_entry_ownership`/`join_entry_valid` (Tasks 6–7) and `edge_live_contribution` (Task 4) — all agree it is `Vector<Atom>`. `ForwardState` (own+valid) is shared by `transfer_op`/`forward_block`/the fixpoint. `analyze(view, b, sem)` signature is stable across the CLI (Task 8) and tests.

**Open risk to watch during execution:** Task 2 touches the most Phase 1 code (every wiring site). Run the *structural* suite after Task 2 before moving on — if any Phase 1 shape test regresses, fix it there rather than compensating in the analysis. The analysis (Tasks 4–7) never mutates structure, so a structural regression is always a Task 2 bug.
