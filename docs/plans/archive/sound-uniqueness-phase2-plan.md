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
- **Test API (real shape — every snippet below uses it).** A suite file exports one `pub fn suite() runner.Suite`, built fluently: `runner.suite("name").test("case", fn() { ...; .Ok({}) }).test(...)`. Each `.test` callback returns `Result<Void, String>` and ends in `.Ok({})`. Assertions return `Result<Void, String>` and are `try`'d: `try assert.equal(a, b)` (needs `Eq + Stringify`; compare ownership by its `Int` tag), `try assert.is_true(cond)`, `try assert.is_false(cond)`; `assert.fail(msg)` / `assert.ok(cond, msg)` return an `Err`/`Result` (use as `return assert.fail(msg)` or the tail). There is **no** `assert.true`/`assert.eq_int`/top-level `runner.test(...)`. Helpers that yield `Result` (e.g. `function(src, name)`) are `try`'d inside the callback. Register a new suite in `boot/tests/main.tw` exactly like the existing `cfg_ownership_suite` (a `use .suites.<name>` line plus its `<name>.suite()` in the run list).
- **Commits.** Short imperative subject; body for non-trivial changes. Add a `Co-Authored-By` trailer **only when it is actually correct for your session/tooling** (AGENTS.md:116) — do not add it unconditionally. The commit-message bodies below omit the trailer for that reason; add it yourself if appropriate.

---

## Execution guardrails (read before Task 1)

These four rules pin down the highest-churn and most-surprising spots so they can't
be gotten subtly wrong. They are **authoritative**: where a task's inline snippet
differs in *structure* (not logic) from a rule here, follow the rule.

### G1 — Freeze `analyze_function` at Task 5; evolve only `ownership_stage`

Tasks 4–7 all touch the per-function driver. Rewriting the same function four times
is how a stray half gets left behind. Split it once so the churny tasks touch a
single helper body:

- **Task 4** writes `analyze_function` with the liveness half only (the
  `compute_liveness` + `collect` that fills `entry.live`/`exit.live`).
- **Task 5** introduces `fn ownership_stage(blocks: Vector<CfgBlock>, b: BuiltinRegistry, sem: OptimizerSemantics) Vector<CfgBlock>` and adds exactly **one** line to `analyze_function` — `blocks = ownership_stage(blocks, b, sem)` after the liveness `collect`. From here on `analyze_function` is **frozen**:

  ```tw
  fn analyze_function(f: CfgFunction, b: BuiltinRegistry, sem: OptimizerSemantics) CfgFunction {
    live := compute_liveness(f.blocks)
    blocks := collect blk in f.blocks {
      bl := live[blk.id.id]
      blk.entry.live = bl.live_in
      blk.exit.live = bl.live_out
      blk
    }
    blocks = ownership_stage(blocks, b, sem)   // Task 5+ only; Task 4 omits this line
    CfgFunction.{ func_id: f.func_id, name: f.name, params: f.params, blocks }
  }
  ```

- **Task 6 / Task 7** replace **`ownership_stage`'s body only** (Task 6: ownership
  fixpoint; Task 7: combined ownership+validity fixpoint). Do not touch
  `analyze_function`.

Whenever a task says "replace the ownership stage of `analyze_function`", it means
replace `ownership_stage`'s body. After Task 7, grep the module: there must be
exactly one `analyze_function` and one live fixpoint driver — no orphaned
`fixpoint_ownership` left beside `run_fixpoint`.

### G2 — Task 2 edit checklist (every edge-arg site, not just the three helpers)

`edge_args_for` feeds args at more sites than the `wire_*` helpers; miss one and the
graph desyncs silently while still typechecking. Retype/replace **all** of these
(function names are the stable anchors; the line refs are pre-edit and drift a few
lines after Task 1):

- `wire_fallthrough_to_join` (def `~:401`) → sig `(ctx, ft: FallThrough, join, params, result_local)`. Callers: `build_match` (`~:471`), `build_if` (`~:513`, `~:517`) — pass **`ft`** (not `ft.block`) and `result_local`.
- `build_match`: retype `falling: Vector<BlockId>` → `Vector<FallThrough>` (`.append(ft.block)` → `.append(ft)`, `for block in falling` → `for ft in falling`). `build_match` already has `result_local` in scope — thread it.
- `wire_backedge` (def `~:524`) → sig `(ctx, ft, header, params)`; compute `edge_args_forward(params)` inside. Caller `~:579`: pass `params`.
- `wire_break_exit` (def `~:537`) → sig `(ctx, block, exit, params, result_local)`. Caller `~:588`: pass `params, result_local`.
- **Inline (non-helper) edge builders in `build_loop`** — easy to miss: the initial `back_args := edge_args_for(params)` (`~:568`) → `edge_args_forward(params)`, and the `for cb in body_out.continues` back-edge (`~:581-587`) → build its edge args with `edge_args_forward(params)`.
- **Delete `edge_args_for`** once nothing calls it — the linter flags the unused fn otherwise.

After Task 2, run the **structural** suite (`cfg_ownership_suite`) *before* any analysis
task. A Phase 1 shape regression here is always a Task 2 wiring bug — the analysis
never mutates structure, so don't compensate for it downstream.

### G3 — Match-arm pattern bindings are invisible defs (sound, imprecise, and locked by a fixture)

A `case` arm binds its payload locals through the **pattern**
(`CorePattern.Var(LocalId)`, extracted by `anf_analysis.collect_pattern_bindings`),
**not** through any `AnfOp` and **not** as a block param — `build_match` builds arm
blocks with `[]` params (`cfg.tw:440`). So `op_defs` never lists them: a
pattern-bound local used in an arm is, to this analysis, *undefined*, and backward
liveness leaks it as live-in up to function entry.

This is **sound and cannot trap**: over-approximating liveness only biases the
`AInit` hinge toward *alias* (`Shared`) instead of *move* — it never mints a bogus
`Unique`; match→arm edges carry `args: []` into `[]`-param arm blocks so no
positional index can go out of bounds; and `fact_of` on an absent local is
`Unknown`. It costs only `--cfg` precision, which no codegen consumes in Phase 2.
**Phase 2 accepts this and does not special-case patterns.** Two guards keep it a
*known* property rather than a latent surprise:

- Add a facts-suite fixture (with the Task 6 control-flow tests) that hand-builds an
  `AMatch` with a `.Variant(_, _, [.Var(lid(k))])` arm whose body reads `lid(k)`,
  then asserts `analyze` does **not** trap and the bound local's arm-exit ownership
  is `Shared` **or** `Unknown` — never `Unique` (the alias bias yields `Shared`, an
  absent fact yields `Unknown`; the invariant is only that it is never `Unique`).
- Record the precision upgrade in the **Phase 3** README row (Task 9): carry each
  arm's `collect_pattern_bindings(arm.pattern)` onto the arm block (a `bound` set
  killed at block entry in `scan_block_backward`, seeded `Unknown` forward). Do
  **not** build it in Phase 2.

### G4 — Tasks 6–7 convergence debug ladder

The generalized live-in join + skip-unprocessed fixpoint is the one place a wrong
answer is subtle. If the loop-carried test (Task 6 Step 7) reports `Unknown` where
`Unique` is expected, check in order:

1. `join_entry_ownership` iterates `blk.entry.live` (**not** `blk.params`) — a
   paramless loop body must join the carried local by same id (the
   `param_index → .None` branch). If it only handles params, the live-through local
   is dropped to `Unknown`.
2. The fixpoint `continue`s on `!is_processed(pred)` — a first-pass back-edge must
   contribute *nothing*, not `Unknown`. A `Unique ⊔ Unknown` at the header means the
   skip is missing.
3. `blk.entry.live` was materialized by the liveness stage **before**
   `ownership_stage` runs (G1 ordering). Empty `entry.live` inside the join means the
   stages are out of order.

Never advance past Task 6 or Task 7 with these tests deferred — they are the only
end-to-end check for the live-through join and the back-edge skip.

---

## Task 1: Carry the raw op and function params (additive, keeps everything green)

Phase 1 dropped the raw `AnfOp` after `op_text(op)` and never recorded a function's own parameters. Add both fields; the transfer and liveness need them. This task is purely additive — `op_text` still renders the same strings, so the Phase 1 structural suite stays green.

**Files:**
- Modify: `boot/compiler/cfg.tw:15` (`CfgInstruction`), `:42` (`CfgFunction`), the instruction push and `build_function`.
- Test: `boot/tests/suites/cfg_ownership_suite.tw` (extend an existing test).

- [ ] **Step 1: Write the failing test**

Add this `.test(...)` clause to the existing `pub fn suite()` in `boot/tests/suites/cfg_ownership_suite.tw` (it uses the file's `function` helper, which returns `Result`):

```tw
    .test("phase2 t1: instructions carry raw op and functions carry params", fn() {
      f := try function("fn f(x: Int) Int {\n  y := x + 1\n  y\n}\n", "f")
      try assert.is_true(f.params.len() >= 1)
      has_binop := false
      for block in f.blocks {
        for inst in block.instructions {
          case inst.op {
            .ABinOp(_, _, _, _) => has_binop = true,
            _ => {},
          }
        }
      }
      try assert.is_true(has_binop)
      .Ok({})
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
renders identically so the structural view is unchanged."
```

---

## Task 2: Real per-edge transferred atoms (phi arguments)

> **Follow [G2](#g2--task-2-edit-checklist-every-edge-arg-site-not-just-the-three-helpers) as you go** — it lists every edge-arg site (including the two inline `build_loop` spots and the now-dead `edge_args_for`), and mandates running the structural suite before moving on. The steps below give the code; G2 is the completeness checklist.

Phase 1 wired placeholder edge args (`edge_args_for(params)` returns the params verbatim) and dropped each arm's `FallThrough.tail`. The ownership join needs the *actual atom* each predecessor feeds into each target param — a join's result local is not a value any arm computes. Make `CfgEdge.args` and the terminator payloads `Vector<Atom>`, and build the real mapping: **result param ← arm tail atom**, **loop-result param ← break payload**, **carried local ← `ALocal(that local)`**.

**Files:**
- Modify: `boot/compiler/cfg.tw` — `CfgEdge` (`:17`), `Terminator` (`:19-27`), `edge_args_for`, the fall-through/join/loop wiring, `local_list_text` uses in rendering, and `edge_arity_mismatch`.
- Test: `boot/tests/suites/cfg_ownership_suite.tw`.

- [ ] **Step 1: Write the failing test**

Add to `cfg_ownership_suite.tw`:

```tw
    .test("phase2 t2: branch join edge args carry the arm tail atom", fn() {
      // Two arms each produce a fresh value; the join param is fed the arm's tail.
      f := try function("fn f(c: Bool) Int {\n  r := if c { 1 } else { 2 }\n  r\n}\n", "f")
      jb := case block_named(f, "if.join") {
        .Some(b) => b,
        .None => return assert.fail("no if.join block"),
      }
      // The join carries exactly one param (the result local r).
      try assert.equal(jb.params.len(), 1)
      // Each predecessor edge supplies a concrete atom for that param, and it is
      // NOT the placeholder ALocal(r): the then-arm feeds the literal 1.
      fed_literal := false
      for pe in jb.preds {
        for a in pe.args {
          case a {
            .ALitInt(_) => fed_literal = true,
            _ => {},
          }
        }
      }
      try assert.is_true(fed_literal)
      .Ok({})
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
authoritative edge list; terminator payloads are render-only and identical."
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
    .test("phase2 t3: analyze returns a view; empty-facts query still works", fn() {
      v := try view("fn f(x: Int) Int {\n  x\n}\n")
      analyzed := ownership.analyze(v, builtins.make_builtin_registry(), sem_for())
      case cfg.function_named(analyzed, "f") {
        .Some(func) => assert.is_true(func.blocks.len() >= 1),
        .None => assert.fail("f missing after analyze"),
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
ownership.tw (cycle). No real facts yet; keeps the view green."
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

use compiler.anf.{AnfExpr, AnfFunctionDef, AnfModule, AnfOp, Atom}
use compiler.builtins
use compiler.cfg
use compiler.core_ir.{FuncId, LocalId, GlobalId}
use compiler.mono_type.{MonoType}
use compiler.opt.semantics as semantics
use compiler.opt.semantics.{make_prelude_optimizer_semantics}
use compiler.ownership

fn lid(id: Int) LocalId {
  LocalId.{ id }
}

fn b_reg() builtins.BuiltinRegistry {
  builtins.make_builtin_registry()
}

fn sem() semantics.OptimizerSemantics {
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

// The suite is one exported builder. Task 4 creates it with the first test;
// Tasks 5-9 append more `.test(...)` clauses. Helper `fn`s go above `suite()`.
pub fn suite() runner.Suite {
  runner
    .suite("cfg ownership facts")
    .test("phase2 t4: a local used after its def is live; a dead one is not", fn() {
      // L0 = Dict.new(); L1 = Dict.set(L0,..); return L1  (L0 dead after its use at L1)
      b := b_reg()
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), dict_set_call(b, lid(0)), .Atom(.ALocal(lid(1)))),
      )
      f := analyzed_func(module_of("f", body))
      blk := block0(f)
      // Neither local is live at entry (both are defined inside the block).
      try assert.is_false(live_contains(blk.entry.live, 0))
      // L1 is the returned value, tracked to the terminator use.
      try assert.is_true(live_contains(blk.exit.live, 1) or blk.succs.len() == 0)
      .Ok({})
    })
}
```

Register the suite in `boot/tests/main.tw`: add `use .suites.cfg_ownership_facts_suite` and include `cfg_ownership_facts_suite.suite()` in the run list, exactly like `cfg_ownership_suite`.

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

// Locals a straight-line op DEFINES. AAssign defines both its let-result and
// its rebind target; every other op defines only its let-result. This matters:
// a def must be killed before its own block's uses so a def-before-use does NOT
// leak the local into live_in (the bug the naive gather-all formula had).
fn op_defs(inst_result: Int, op: AnfOp) Vector<Int> {
  case op {
    .AAssign(target, _) => insert_sorted(insert_sorted([], inst_result), target.id),
    _ => insert_sorted([], inst_result),
  }
}

// Backward per-instruction scan of one block. Returns live_in AND, for each
// instruction index i, the set of locals live *immediately after* i
// (`live_after[i]`). live_after is what Task 5 uses for last-use: a use of L at
// i is a last-use iff L ∉ live_after[i] — which correctly handles a later
// AAssign that redefines L (loop consume-then-reassign) and a block-live-out L.
type BlockScan = .{ live_in: Vector<Int>, live_after: Vector<Vector<Int>> }

fn scan_block_backward(blk: CfgBlock, live_out: Vector<Int>) BlockScan {
  // Seed with live_out plus the terminator's direct uses (scrutinee/cond/
  // return/break). Terminator EDGE args are handled by the edge translation in
  // edge_live_contribution, not here.
  cur := live_out
  case blk.terminator {
    .Some(t) => for u in term_uses(t) {
      cur = insert_sorted(cur, u)
    },
    .None => {},
  }
  n := blk.instructions.len()
  after_rev: Vector<Vector<Int>> = []
  i := n - 1
  for i >= 0 {
    after_rev = .append(cur)                                     // live AFTER instruction i
    inst := blk.instructions[i]
    cur = diff_sorted(cur, op_defs(inst.anf_local.id, inst.op))  // kill defs first
    for u in op_uses(inst.op) {                                  // then add uses
      cur = insert_sorted(cur, u)
    }
    i = i - 1
  }
  // after_rev is high->low; reverse so live_after[i] indexes by instruction i.
  live_after: Vector<Vector<Int>> = []
  j := after_rev.len() - 1
  for j >= 0 {
    live_after = .append(after_rev[j])
    j = j - 1
  }
  BlockScan.{ live_in: cur, live_after }
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
      li := scan_block_backward(blk, lo).live_in
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
by the move-vs-alias and last-use hinges."
```

---

> **Apply [G1](#g1--freeze-analyze_function-at-task-5-evolve-only-ownership_stage) here:** introduce `ownership_stage` and add its single call to `analyze_function`, then freeze `analyze_function`. Tasks 6–7 rewrite only `ownership_stage`'s body. The Step 5 "temporary single-pass wiring" below is `ownership_stage`'s first body — put it there, not inline in `analyze_function`.

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

fn assert_own(actual: ownership.Ownership, expected: ownership.Ownership) Result<Void, String> {
  assert.equal(ownership.own_tag(actual), ownership.own_tag(expected))
}
```

Tests, appended as `.test(...)` clauses to the facts `suite()` (each cross-references worked-examples).

> **Fixture rule for positive (`Unique`) cases.** `forward_block` publishes the
> terminator value (`Return(A)`), so a fixture that *returns the value under
> test* would see it demoted to `Shared` at `exit.ownership` — correctly, because
> returning it publishes it. To observe the pre-publication `Unique` fact at the
> block boundary, the positive fixtures end in a **literal tail** (`.ALitInt(0)`)
> so the tracked local is not the returned value. The negative fixtures (alias,
> publish) already end `Shared`, so they can return the value.

```tw
    .test("phase2 t5 introduce: Dict.new() is Unique", fn() {
      // L0 = Dict.new(); return 0   (L0 not returned, so not published)
      b := b_reg()
      body: AnfExpr = .Let(lid(0), dict_new_call(b), .Atom(.ALitInt(0)))
      f := analyzed_func(module_of("f", body))
      try assert_own(own_at_exit(f, 0, 0), .Unique)
      .Ok({})
    })
    .test("phase2 t5 move: init of a dead source keeps Unique (Case B)", fn() {
      // L0 = Dict.new(); L1 = init L0; return 0   (L0 dead after the init; L1 not published)
      b := b_reg()
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), .AInit(.ALocal(lid(0))), .Atom(.ALitInt(0))),
      )
      f := analyzed_func(module_of("f", body))
      try assert_own(own_at_exit(f, 0, 1), .Unique)
      .Ok({})
    })
    .test("phase2 t5 alias: init with a still-live source demotes both (Case C)", fn() {
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
      try assert_own(own_at_exit(f, 0, 0), .Shared)
      try assert_own(own_at_exit(f, 0, 1), .Shared)
      .Ok({})
    })
    .test("phase2 t5 publish: global_set demotes to Shared", fn() {
      // L0 = Dict.new(); global_set G0 = L0; return L0
      b := b_reg()
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), .AGlobalSet(GlobalId.{ id: 0 }, .ALocal(lid(0))), .Atom(.ALocal(lid(0)))),
      )
      f := analyzed_func(module_of("f", body))
      try assert_own(own_at_exit(f, 0, 0), .Shared)
      .Ok({})
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

Last-use is derived from the per-instruction `live_after` sets produced by
`scan_block_backward` (Task 4), which correctly account for a later `AAssign`
redefining the local (loop consume-then-reassign) and for block-live-out: a use
of `L` at instruction `i` is a **last-use** iff `L ∉ live_after[i]`. The forward
pass precomputes `live_after` once per block and consults it by index:

```tw
// The locals used at instruction i whose value is dead immediately after i.
fn last_use_at(op: AnfOp, live_after_i: Vector<Int>) Vector<Int> {
  out: Vector<Int> = []
  for id in op_uses(op) {
    if !live_contains_int(live_after_i, id) {
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

Add a per-block forward that seeds from `entry` facts (Task 6 wires the real entry; for now seed empty, which single-block fixtures need), applies the per-instruction transfer using the block's `live_after` sets, and **then applies terminator publication** for `Return(A)` / value-carrying `ValueBreak(A)` (the exit-edge publication the design requires — without it a returned/broken value is not demoted on its exit edge):

```tw
fn forward_block(blk: CfgBlock, entry: ForwardState, b: BuiltinRegistry, sem: OptimizerSemantics) ForwardState {
  scan := scan_block_backward(blk, blk.exit.live)   // exit.live was filled by the liveness stage
  st := entry
  for inst, i in blk.instructions {
    last := last_use_at(inst.op, scan.live_after[i])
    st = transfer_op(st, inst.anf_local.id, inst.op, last, b, sem)
  }
  // Terminator publication: publish the value leaving on this block's exit edge.
  case blk.terminator {
    .Some(.Return(.Some(a))) => st = publish_atom(st, a),
    .Some(.ValueBreak(a)) => st = publish_atom(st, a),
    _ => {},
  }
  st
}
```

`scan_block_backward` needs `blk.exit.live`, so the liveness stage must run and be materialized into the blocks **before** the ownership stage (`ownership_stage` receives the liveness-filled blocks; Task 4's `collect` in `analyze_function` runs first).

Per [G1](#g1--freeze-analyze_function-at-task-5-evolve-only-ownership_stage), introduce `ownership_stage` now and wire it in with the single frozen line, then never edit `analyze_function` again — Tasks 6–7 change only this helper's body. Its Task 5 body is a single forward pass per block (Task 6 turns it into a fixpoint with real entry facts). Temporary single-pass wiring so Task 5 tests pass:

```tw
fn ownership_stage(blocks: Vector<CfgBlock>, b: BuiltinRegistry, sem: OptimizerSemantics) Vector<CfgBlock> {
  collect blk in blocks {
    entry_state := ForwardState.{ own: Dict.new(), valid: Dict.new() }
    exit_state := forward_block(blk, entry_state, b, sem)
    blk.exit.ownership = exit_state.own
    blk.exit.binding_valid = exit_state.valid
    blk
  }
}
```

Then add the one call to `analyze_function` (after the liveness `collect`, exactly as the frozen form in G1 shows):

```tw
  blocks = ownership_stage(blocks, b, sem)
```

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
control-flow join + fixpoint is the next task."
```

---

> **Two guardrails apply here:** [G1](#g1--freeze-analyze_function-at-task-5-evolve-only-ownership_stage) (replace `ownership_stage`'s body only — leave `analyze_function` alone), and [G4](#g4--tasks-67-convergence-debug-ladder) (the debug ladder for the loop-carried test). Also add the [G3](#g3--match-arm-pattern-bindings-are-invisible-defs-sound-imprecise-and-locked-by-a-fixture) match-binding fixture alongside these control-flow tests.

## Task 6: Positional predecessor join + fixpoint (branches and loops)

Turn the single forward pass into a fixpoint: each block's `entry.ownership` joins predecessors over **every live-in local** — block params via positional `fact(pred_exit, pred_edge.args[i])`, live-through non-param locals by same id — skipping not-yet-processed predecessors so back-edges do not poison loop headers; reprocess to fixpoint. This delivers branch-join and loop-carried ownership.

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
    .test("phase2 t6 branch join: both arms allocate => Unique (Case V join)", fn() {
      // r := if c { Dict.new() } else { Dict.new() }; return r
      b := b_reg()
      then_e: AnfExpr = .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1))))
      else_e: AnfExpr = .Let(lid(2), dict_new_call(b), .Atom(.ALocal(lid(2))))
      body: AnfExpr = .Let(lid(0), .AIf(.ALitBool(true), then_e, else_e), .Atom(.ALocal(lid(0))))
      f := analyzed_func(module_of("f", body))
      rp := result_param_of(f, "if.join")
      try assert_own(own_entry_named(f, "if.join", rp), .Unique)
      .Ok({})
    })
    .test("phase2 t6 branch join: one arm publishes => Shared", fn() {
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
      try assert_own(own_entry_named(f, "if.join", rp), .Shared)
      .Ok({})
    })
```

- [ ] **Step 2: Run to verify they fail**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — join entry ownership is empty (single-pass seeds every entry empty ⇒ `Unknown`).

- [ ] **Step 3: Implement the positional join**

The join must cover **every local live at the block boundary**, not just block
params. A local can flow through a block without being a param (Phase 1 is not
full SSA): a loop-body block has *no* params, yet the loop-carried collection is
live-through it. For such a local the join is by **same id** across predecessor
exits; only genuine block params use the positional edge-arg translation. And to
avoid a not-yet-processed back-edge poisoning a loop header
(`Unique ⊔ Unknown = Unknown`), the join **skips predecessors that have not been
processed yet** — an unprocessed predecessor contributes nothing (the `⊔`
identity). At fixpoint every reachable predecessor is processed, so the result is
the true join.

```tw
fn param_index(blk: CfgBlock, lid: Int) Int? {
  for p, i in blk.params {
    if p.id == lid {
      return .Some(i)
    }
  }
  .None
}

fn fact_of_local(own: Dict<Int, Int>, id: Int) Ownership {
  case own.get(id) {
    .Some(tag) => own_of_tag(tag),
    .None => .Unknown,
  }
}

fn is_processed(processed: Dict<Int, Bool>, id: Int) Bool {
  case processed.get(id) {
    .Some(v) => v,
    .None => false,
  }
}

// entry.ownership for every local live-in at blk:
//  - a block param: positional join of fact(pred_exit, pred_edge.args[i]);
//  - a live-through non-param local: same-id join of pred_exit.ownership[lid].
// Only processed predecessors contribute (unprocessed back-edges are skipped).
fn join_entry_ownership(blk: CfgBlock, exits: Dict<Int, Dict<Int, Int>>, processed: Dict<Int, Bool>) Dict<Int, Int> {
  entry: Dict<Int, Int> = Dict.new()
  for lid in blk.entry.live {
    pidx := param_index(blk, lid)
    acc: Ownership? = .None
    for pe in blk.preds {
      // pe.target holds the predecessor block id (see cfg.add_pred comment).
      if !is_processed(processed, pe.target.id) {
        continue
      }
      pred_exit := case exits.get(pe.target.id) {
        .Some(m) => m,
        .None => Dict.new(),
      }
      contributed := case pidx {
        .Some(i) => if i < pe.args.len() {
          fact_of(pred_exit, pe.args[i])
        } else {
          .Unknown
        },
        .None => fact_of_local(pred_exit, lid),
      }
      acc = case acc {
        .None => .Some(contributed),
        .Some(prev) => .Some(join_own(prev, contributed)),
      }
    }
    case acc {
      .Some(o) => entry[lid] = own_tag(o),
      .None => {},
    }
  }
  entry
}
```

The function entry block (block 0) has no predecessors, so its live-in
function-parameter locals get no join fact → absent → read as `Unknown` (the
correct default for an un-summarized parameter). No explicit param seeding is
needed.

- [ ] **Step 4: Implement the fixpoint driver**

Add the fixpoint driver `fixpoint_ownership` (`analyze_function` stays frozen — per [G1](#g1--freeze-analyze_function-at-task-5-evolve-only-ownership_stage) you replace only `ownership_stage`'s body, in Step 5). The driver iterates over the exit ownership maps, tracking which blocks have been processed so the join can skip unprocessed back-edges. Iterate blocks in id order (≈RPO), so forward-edge predecessors are processed before their targets and back-edges converge over subsequent rounds. Binding-validity is threaded into this same loop in Task 7:

```tw
fn fixpoint_ownership(blocks: Vector<CfgBlock>, b: BuiltinRegistry, sem: OptimizerSemantics) Dict<Int, Dict<Int, Int>> {
  exits: Dict<Int, Dict<Int, Int>> = Dict.new()
  processed: Dict<Int, Bool> = Dict.new()
  for blk in blocks {
    exits[blk.id.id] = Dict.new()
    processed[blk.id.id] = false
  }
  changed := true
  for changed {
    changed = false
    for blk in blocks {
      entry_own := join_entry_ownership(blk, exits, processed)
      st := ForwardState.{ own: entry_own, valid: Dict.new() }
      st = forward_block(blk, st, b, sem)
      already := is_processed(processed, blk.id.id)
      if !already or !same_own_map(exits[blk.id.id], st.own) {
        changed = true
        exits[blk.id.id] = st.own
      }
      processed[blk.id.id] = true
    }
  }
  exits
}

// After the fixpoint every reachable block is processed, so materialize/join
// with an all-true processed map.
fn all_processed(blocks: Vector<CfgBlock>) Dict<Int, Bool> {
  done: Dict<Int, Bool> = Dict.new()
  for blk in blocks {
    done[blk.id.id] = true
  }
  done
}

fn same_own_map(a: Dict<Int, Int>, b: Dict<Int, Int>) Bool {
  if a.keys().len() != b.keys().len() {
    return false
  }
  for k in a.keys() {
    case a.get(k) {
      .Some(av) => case b.get(k) {
        .Some(bv) => if av != bv {
          return false
        },
        .None => return false,
      },
      .None => {},
    }
  }
  true
}
```

> Determinism note: `same_own_map` iterates `a.keys()` only to compare values — order-independent (pure equality). The fixpoint result does not depend on `Dict` iteration order.

- [ ] **Step 5: Materialize entry/exit ownership into the blocks**

Replace `ownership_stage`'s body (the Task 5 single-pass `collect`) with the fixpoint + materialize — `analyze_function` is untouched (per [G1](#g1--freeze-analyze_function-at-task-5-evolve-only-ownership_stage)):

```tw
fn ownership_stage(blocks: Vector<CfgBlock>, b: BuiltinRegistry, sem: OptimizerSemantics) Vector<CfgBlock> {
  exits := fixpoint_ownership(blocks, b, sem)
  done := all_processed(blocks)
  collect blk in blocks {
    entry_own := join_entry_ownership(blk, exits, done)
    st := ForwardState.{ own: entry_own, valid: Dict.new() }
    st = forward_block(blk, st, b, sem)
    blk.entry.ownership = entry_own
    blk.exit.ownership = st.own
    blk
  }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — both-arms-Unique join and one-arm-published join, plus a loop-carried case if you add one (accumulator consumed-then-reassigned stays `Unique`).

- [ ] **Step 7: Add a loop-carried test, then commit**

```tw
    .test("phase2 t6 loop-carried: consume-then-reassign stays Unique (Case A)", fn() {
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
      try assert_own(own_entry_named(f, "loop.header", 1), .Unique)
      .Ok({})
    })
```

This test is the end-to-end check for the **live-through join** and the
**skip-unprocessed fixpoint**: `loop.body` has *no* params, so `L1` is a
live-through non-param local inside it — the consuming `Dict.set(L1)` only proves
`Unique` if `join_entry_ownership` propagates `L1`'s fact by same id (blocker 2),
and the loop header only stays `Unique` if the not-yet-processed back-edge does
not poison it to `Unknown` on the first pass (blocker 3). If it fails as
`Unknown`, those two are the cause. (If it fails because the header's carried
param set does not include `L1`, confirm Phase 1 `loop_params` carries the
`AAssign` target — it does, via `collect_assign_targets`; adjust the asserted id
if the fixture's ids differ.)

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
git commit -m "ownership: predecessor join + fixpoint over live-in locals

entry.ownership joins every live-in local across processed predecessors: block
params via positional edge args, live-through non-param locals by same id. The
fixpoint skips not-yet-processed predecessors so an uninitialized back-edge does
not poison a loop header (Unique join Unknown = Unknown). Delivers branch-join
(both arms Unique => Unique; one publishes => Shared) and loop-carried ownership
(consume-then-reassign stays Unique through the paramless loop body)."
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

// (facts suite copy of the structural suite's helper — add if not already present)
fn block_named(f: cfg.CfgFunction, name: String) cfg.CfgBlock? {
  for blk in f.blocks {
    if blk.name == name {
      return .Some(blk)
    }
  }
  .None
}

    .test("phase2 t7: a moved source is invalid at block exit", fn() {
      // L0 = Dict.new(); L1 = init L0; return L1  (L0 moved => invalid at exit)
      b := b_reg()
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), .AInit(.ALocal(lid(0))), .Atom(.ALocal(lid(1)))),
      )
      f := analyzed_func(module_of("f", body))
      try assert.is_false(valid_at_exit(f, 0, 0))
      .Ok({})
    })
    .test("phase2 t7: a valid live-through non-param local stays valid across a boundary", fn() {
      // L0 = Dict.new(); r := if c { 1 } else { 2 }; L2 = Dict.set(L0,..); return 0
      // L0 is live-through the if blocks WITHOUT being a join param; it is never
      // moved, so the generalized validity merge must keep it valid where read.
      b := b_reg()
      then_e: AnfExpr = .Atom(.ALitInt(1))
      else_e: AnfExpr = .Atom(.ALitInt(2))
      after: AnfExpr = .Let(lid(2), dict_set_call(b, lid(0)), .Atom(.ALitInt(0)))
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), .AIf(.ALitBool(true), then_e, else_e), after),
      )
      f := analyzed_func(module_of("f", body))
      // Find the continuation block that reads L0 and assert L0 valid at its entry.
      blk := case block_named(f, "if.join") {
        .Some(jb) => jb,
        .None => return assert.fail("no if.join block"),
      }
      case blk.entry.binding_valid.get(0) {
        .Some(v) => assert.is_true(v),
        .None => .Ok({}),   // absent => valid (also acceptable)
      }
    })
```

> **On the requested "invalid live-through, non-param" test.** Under the Phase 2
> hinges, *every* invalidation is gated on last-use (`AInit` move, consuming-op
> move, field-store move all require the source to be dead here). "Moved" therefore
> implies "dead", and a dead local is not live across any out-edge — so an
> **invalid live-through** local is unreachable in this domain, and a test asserting
> it cannot be constructed with real ops. The generalized merge above is still the
> correct, sound design (it mirrors ownership and is defensive against future
> rules that could invalidate a still-live local), and the live-through *valid*
> case above guards that the merge does not spuriously flip a live-through local to
> invalid. If a later phase adds an invalidate-while-live rule, add the negative
> test then.

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — `binding_valid` is currently seeded empty each pass and not materialized into `exit.binding_valid`.

- [ ] **Step 3: Join binding-validity positionally and thread it**

The validity meet covers **every live-in local**, mirroring the generalized
ownership join (blocker parity): block params meet through the positional edge
arg's source-local validity; live-through non-param locals meet the same id's
validity across processed predecessor exits. Absent pred validity means valid.

```tw
fn valid_of_local(pv: Dict<Int, Bool>, id: Int) Bool {
  case pv.get(id) {
    .Some(v) => v,
    .None => true,   // absent => valid
  }
}

fn join_entry_valid(blk: CfgBlock, exit_valid: Dict<Int, Dict<Int, Bool>>, processed: Dict<Int, Bool>) Dict<Int, Bool> {
  entry: Dict<Int, Bool> = Dict.new()
  for lid in blk.entry.live {
    pidx := param_index(blk, lid)
    ok := true
    for pe in blk.preds {
      if !is_processed(processed, pe.target.id) {
        continue
      }
      pv := case exit_valid.get(pe.target.id) {
        .Some(m) => m,
        .None => Dict.new(),
      }
      contributed := case pidx {
        .Some(i) => if i < pe.args.len() {
          case atom_local_id(pe.args[i]) {
            .Some(src) => valid_of_local(pv, src),
            .None => true,   // non-local feed is always valid
          }
        } else {
          true
        },
        .None => valid_of_local(pv, lid),
      }
      if !contributed {
        ok = false
      }
    }
    entry[lid] = ok
  }
  entry
}

fn same_valid_map(a: Dict<Int, Bool>, b: Dict<Int, Bool>) Bool {
  if a.keys().len() != b.keys().len() {
    return false
  }
  for k in a.keys() {
    case a.get(k) {
      .Some(av) => case b.get(k) {
        .Some(bv) => if av != bv {
          return false
        },
        .None => return false,
      },
      .None => {},
    }
  }
  true
}
```

Now generalize the Task 6 `fixpoint_ownership` into a **combined** fixpoint that
co-iterates ownership and validity to a joint fixed point (they share
`ForwardState` and `forward_block`), and update `ownership_stage`'s body to fill
both maps. This **replaces** the ownership-only `fixpoint_ownership` (delete it —
per [G1](#g1--freeze-analyze_function-at-task-5-evolve-only-ownership_stage) there
must be exactly one live driver) and `ownership_stage`'s Task 6 body;
`analyze_function` stays frozen:

```tw
type FixResult = .{ exits: Dict<Int, Dict<Int, Int>>, exit_valid: Dict<Int, Dict<Int, Bool>> }

fn run_fixpoint(blocks: Vector<CfgBlock>, b: BuiltinRegistry, sem: OptimizerSemantics) FixResult {
  exits: Dict<Int, Dict<Int, Int>> = Dict.new()
  exit_valid: Dict<Int, Dict<Int, Bool>> = Dict.new()
  processed: Dict<Int, Bool> = Dict.new()
  for blk in blocks {
    exits[blk.id.id] = Dict.new()
    exit_valid[blk.id.id] = Dict.new()
    processed[blk.id.id] = false
  }
  changed := true
  for changed {
    changed = false
    for blk in blocks {
      entry_own := join_entry_ownership(blk, exits, processed)
      entry_valid := join_entry_valid(blk, exit_valid, processed)
      st := ForwardState.{ own: entry_own, valid: entry_valid }
      st = forward_block(blk, st, b, sem)
      already := is_processed(processed, blk.id.id)
      if !already
        or !same_own_map(exits[blk.id.id], st.own)
        or !same_valid_map(exit_valid[blk.id.id], st.valid) {
        changed = true
        exits[blk.id.id] = st.own
        exit_valid[blk.id.id] = st.valid
      }
      processed[blk.id.id] = true
    }
  }
  FixResult.{ exits, exit_valid }
}
```

`ownership_stage`'s final body (replacing its Task 6 body; `analyze_function` unchanged):

```tw
fn ownership_stage(blocks: Vector<CfgBlock>, b: BuiltinRegistry, sem: OptimizerSemantics) Vector<CfgBlock> {
  fx := run_fixpoint(blocks, b, sem)
  done := all_processed(blocks)
  collect blk in blocks {
    entry_own := join_entry_ownership(blk, fx.exits, done)
    entry_valid := join_entry_valid(blk, fx.exit_valid, done)
    st := ForwardState.{ own: entry_own, valid: entry_valid }
    st = forward_block(blk, st, b, sem)
    blk.entry.ownership = entry_own
    blk.exit.ownership = st.own
    blk.entry.binding_valid = entry_valid
    blk.exit.binding_valid = st.valid
    blk
  }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — moved-source-invalid, plus all prior tests.

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_ownership_facts_suite.tw
git commit -m "ownership: binding-validity (default, transfer, generalized meet)

Thread binding_valid through the forward pass (moves invalidate, AAssign
revalidates) and join it by the meet over live-in locals — params via edge args,
live-through non-param locals by same id — co-iterated to a joint fixpoint with
ownership. A value moved on one arm and forwarded on another is 'not usable' at
the merge."
```

---

## Task 8: Render populated facts + `twk ir --cfg` wiring + determinism

Print the ownership facts keyed by each block's carried params (sorted), and make `twk ir --cfg` run `analyze`. Binding-validity and liveness stay off the default print.

**Files:**
- Modify: `boot/compiler/cfg.tw` (`render_block`), `boot/commands/ir.tw` (`--cfg` path), `boot/main.tw` (import if needed).
- Test: `boot/tests/suites/cfg_ownership_facts_suite.tw` (render + determinism) and a CLI smoke check.

- [ ] **Step 1: Write the failing render test**

```tw
    .test("phase2 t8: render shows populated facts for carried params", fn() {
      b := b_reg()
      then_e: AnfExpr = .Let(lid(1), dict_new_call(b), .Atom(.ALocal(lid(1))))
      else_e: AnfExpr = .Let(lid(2), dict_new_call(b), .Atom(.ALocal(lid(2))))
      body: AnfExpr = .Let(lid(0), .AIf(.ALitBool(true), then_e, else_e), .Atom(.ALocal(lid(0))))
      v := cfg.build_view(module_of("f", body), b)
      a := ownership.analyze(v, b, sem())
      out := cfg.render_view(a)
      try assert.is_true(out.contains("Unique"))
      try assert.is_true(out.contains("facts.in={") and out.contains("facts.out={"))
      .Ok({})
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
    .test("phase2 t8: facts are byte-identical across two analyses", fn() {
      b := b_reg()
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), dict_set_call(b, lid(0)), .Atom(.ALocal(lid(1)))),
      )
      m := module_of("f", body)
      a1 := cfg.render_view(ownership.analyze(cfg.build_view(m, b), b, sem()))
      a2 := cfg.render_view(ownership.analyze(cfg.build_view(m, b), b, sem()))
      try assert.is_true(a1 == a2)
      .Ok({})
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
across runs."
```

---

## Task 9: Soundness guard, real-program smoke, and tracking docs

Add the two-direction soundness guard, a wide real-program smoke over `boot/main.tw`, and make the deferral tracking edits the design promised.

**Files:**
- Test: `boot/tests/suites/cfg_ownership_facts_suite.tw`.
- Modify: `docs/plans/sound-uniqueness/README.md`.

- [ ] **Step 1: Write the soundness-guard test (both directions)**

First add the tag helper and the effect-name imports at the top of the suite (`EffectKind` lives in `compiler.opt.semantics`):

```tw
use compiler.opt.semantics.{EffectKind}
use compiler.pipeline

fn effect_is_update(e: EffectKind) Bool {
  case e {
    .Update => true,
    _ => false,
  }
}

// Assert a builtin FuncId classifies as Update with a cow_base_arg.
fn assert_update_with_base(s: semantics.OptimizerSemantics, fid: FuncId, label: String) Result<Void, String> {
  case semantics.call_info(s, fid) {
    .Some(cs) => {
      try assert.is_true(effect_is_update(cs.effect))
      case cs.cow_base_arg {
        .Some(_) => .Ok({}),
        .None => assert.fail("${label} missing cow_base_arg"),
      }
    },
    .None => assert.fail("${label} has no CallSemantics"),
  }
}
```

The guard `.test` clause. Note the three Update builtins use **mixed accessors**: `Dict.set`/`Vector.append` are `method_id`, but the vector index-set update path is registered as `vector$set_unsafe` via `b.id(...)` — **not** `method_id("Vector", "set")`:

```tw
    .test("t9 soundness guard: classifications match the transfer's assumptions", fn() {
      b := b_reg()
      s := sem()
      try assert_update_with_base(s, b.method_id("Dict", "set"), "Dict.set")
      try assert_update_with_base(s, b.method_id("Vector", "append"), "Vector.append")
      try assert_update_with_base(s, b.id("vector$set_unsafe"), "vector$set_unsafe")
      // A Cell op must resolve to .None (publish bucket) or at least never Update.
      case semantics.call_info(s, b.method_id("Cell", "set")) {
        .None => .Ok({}),
        .Some(cs) => assert.is_false(effect_is_update(cs.effect)),
      }
    })
```

> If `Cell.set` is registered under a different builtin name, grep `boot/compiler/opt/semantics.tw` for the Cell rows and adjust. The guard's intent — Cell never `Update` — is what matters.

- [ ] **Step 2: Write a wide real-program smoke**

Add a compile-source helper (the facts suite otherwise uses hand-built modules) and a smoke `.test`:

```tw
fn analyze_source(src: String) Result<cfg.CfgView, String> {
  b := b_reg()
  artifacts := try pipeline.compile_source(src)
  .Ok(ownership.analyze(cfg.build_view(artifacts.opt, b), b, sem()))
}
```

```tw
    .test("t9 smoke: analyze a real optimized program without trapping", fn() {
      src := "fn build() Dict<Int, Int> {\n  d := Dict.new()\n  d[1] = 2\n  d[3] = 4\n  d\n}\n"
      v := try analyze_source(src)
      try assert.is_true(v.functions.len() >= 1)
      .Ok({})
    })
```

- [ ] **Step 3: Run boot tests**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS.

- [ ] **Step 4: Update the README tracking (the design's deferral task)**

In `docs/plans/sound-uniqueness/README.md`:

1. Mark the four implemented Phase 2 bullets `[x]` with a one-line "Done" note pointing at `phase2-design.md` and the new files (`compiler/ownership.tw`, `cfg_ownership_facts_suite.tw`). Leave "Catalog later precision needs" as the coverage-doc bullet.
2. Add a **Phase 8** bullet: "Extern copying-borrow precision — treat host imports as borrow (args preserved) with `Unique` GC results per the copying-marshalling contract; Phase 2 conservatively over-publishes them." Cross-reference `concurrency-publication.md` and `fact-lattice.md`'s extern row.
3. Add to **Phase 3** ("Move ownership-relevant pass queries to CFG facts"): "Dead-merge block-param pruning using the Phase 2 liveness facts." Also add: "Match-arm pattern-binding precision — carry `collect_pattern_bindings(arm.pattern)` onto arm blocks so pattern-bound locals are killed at block entry (Phase 2 soundly over-approximates them as live-in; see plan guardrail G3)."
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
Phase 8 extern + Phase 3 dead-merge tracking rows the design deferred."
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
- Join + fixpoint over **all live-in locals** (params via edge args, live-through non-param locals by same id), skipping unprocessed back-edges (branch join, loop-carried) → Task 6. ✓
- Binding-validity default/transfer/generalized meet (same live-in coverage) → Task 7. ✓
- Rendering (sorted params) + `--cfg` wiring + determinism → Task 8. ✓
- Two-direction soundness guard + tracking-doc edits → Task 9. ✓
- Conservative ref rule (no type table) → encoded in `publish_atom`/`field_store` acting on every `ALocal` (Conventions + Task 5). ✓

**2. Placeholder scan:** No "TBD"/"add error handling" — every code step shows code; every test step shows the assertion and the run/expected. The one soft spot (exact Cell builtin name in Task 9) has an explicit "grep and adjust; intent is what matters" instruction, not a silent gap.

**3. Type consistency:** `Ownership` (enum) lives in `ownership.tw`; `BlockFacts.ownership` stores the **`Int` tag** (Task 3 note) to avoid a `cfg.tw → ownership.tw` import cycle; `own_tag`/`own_of_tag` bridge them and are used consistently in tests (`assert_own`, `own_at_exit`) and rendering (`own_tag_text`). `CfgEdge.args: Vector<Atom>` is introduced in Task 2 and read positionally by `join_entry_ownership`/`join_entry_valid` (Tasks 6–7) and `edge_live_contribution` (Task 4) — all agree it is `Vector<Atom>`. `ForwardState` (own+valid) is shared by `transfer_op`/`forward_block`/the fixpoint. `analyze(view, b, sem)` signature is stable across the CLI (Task 8) and tests.

**Open risks to watch during execution:**

- **Task 2** touches the most Phase 1 code (every wiring site). Run the
  *structural* suite after Task 2 before moving on — if any Phase 1 shape test
  regresses, fix it there rather than compensating in the analysis. The analysis
  (Tasks 4–7) never mutates structure, so a structural regression is always a
  Task 2 bug.
- **Tasks 6 and 7** are the intricate dataflow (generalized live-in join,
  skip-unprocessed fixpoint, combined ownership/validity). Do **not** defer their
  tests — run the branch-join and loop-carried tests the moment each task's code
  lands. The loop-carried test in particular is the load-bearing check for the
  live-through join *and* the back-edge skip; if it passes, both are working.
