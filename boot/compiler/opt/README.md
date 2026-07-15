# ANF Optimization Pipeline

The boot compiler's optimizer transforms ANF IR (Administrative Normal Form) to
remove dead bindings, propagate copies, fold constants, simplify constant-condition
branches, and eliminate `defer` nodes. All passes operate directly on the structured
ANF tree.

There is **no** uniqueness/COW-to-in-place rewrite here anymore, and no builder-region
rewrite. Those passes (and the standalone liveness/ownership analyses that fed them)
were removed in the sound-uniqueness rebuild. The optimizer emits no in-place mutation
today; every collection op stays copy-on-write through codegen. Ownership facts now
live outside `opt/` (see "CFG ownership facts" below) and are analysis-only — they
drive no codegen yet.

## Pipeline Overview

`pipeline.tw` orchestrates, per function:

1. **`eliminate_defers` first** — runs before the peephole loop so that
   `branch_simplify` cannot hoist `ADefer` nodes out of constant-condition branches
   and break block-scoped defer semantics.
2. **Fixed-point peephole loop** (max 10 rounds): dead let elimination, copy
   propagation, constant folding, branch simplification — repeated until no pass
   reports a change (or the round cap is hit).

```
AnfModule
  -> per function:
       eliminate_defers          (runs first; see note above)
       loop {
         dead_let_elim -> copy_propagate -> constant_fold -> branch_simplify
       } until stable (or 10 rounds)
  -> optimized AnfModule
```

Functions whose ANF nesting exceeds `max_stack_safe_opt_depth` (512) skip the
peephole loop (only `eliminate_defers` runs) to avoid deep recursion.

### Pinned locals

Module-level `$init` functions may bind locals that other functions reference
(module-scope variables). `compute_pinned` identifies these cross-function locals so
that dead let elimination and copy propagation never remove or inline them.

## Passes

### use_count.tw — Use Counting

Foundation for dead let elimination and copy propagation. Walks the ANF tree and
counts how many times each `LocalId` appears in operand/atom position.

- `count_uses(expr)` — counts all references including `AMakeClosure.free_vars`
- `count_uses_excluding_free_vars(expr)` — excludes closure free var positions
  (used by copy propagation, since inlining a literal into a free var slot is invalid)
- purity is decided via `is_pure_with_semantics` (from `semantics.tw`), so calls stay
  non-eliminable

Let binders and `AAssign` targets are not counted as uses.

### dead_let.tw — Dead Let Elimination

```
Let(t, pure_op, body)  where  uses[t] == 0  and  t not assigned  ->  body
```

Removes let-bindings whose bound local is never referenced and whose right-hand side
is pure (no side effects). Locals that appear as `AAssign` targets are preserved even
at zero use count, since the assignment itself may be meaningful. Pinned locals are
merged into the assigned set so they are never eliminated. `dead_let_elim_with_pinned_and_semantics`
consults the optimizer semantics for the purity decision; `dead_let_elim_with_pinned`
is the semantics-free fallback.

Recurses into `AIf`, `AMatch`, `ALoop`, and `ADefer` sub-expressions.

### copy_prop.tw — Copy Propagation

```
Let(t, AInit(atom), body)  where  can_propagate(atom, uses[t])  ->  body[t := atom]
```

Inlines the initializer atom directly at use sites and drops the let-binding.
Propagation rules:

- **Literals and global funcs** (`ALitInt`, `ALitFloat`, `ALitBool`, `ALitStr`,
  `ALitVoid`, `AGlobalFunc`): always safe to duplicate, propagated at any use count >= 1.
- **Local-to-local** (`ALocal(u)`): propagated only when `t` is used exactly once and
  neither `t` nor `u` is reassigned (`AAssign` target), to avoid observing stale values.

Uses `count_uses_excluding_free_vars` so that closure free var positions (which cannot
accept arbitrary atoms) don't inflate the use count. `ARecordUpdate.can_reuse` is a
structural field carried through unchanged — copy propagation does not compute or set it.

Provides `subst_atom(expr, target, replacement)` as a general-purpose atom substitution
utility, with a shadow-stop guard: if a nested `Let` rebinds the target, substitution
stops in that scope.

### const_fold.tw — Constant Folding

Evaluates `ABinOp` and `AUnOp` with literal operands at compile time, rewriting the
result to `AInit(literal)`.

Supported folds:
- **Int**: `+`, `-`, `*`, `/` (skip if b=0), `%` (skip if b=0), `&`, `|`, `^`, `<<`,
  `>>`, `==`, `!=`, `<`, `<=`, `>`, `>=`
- **Float**: `+`, `-`, `*`, `/`, `==`, `!=`, `<`, `<=`, `>`, `>=`
- **Bool**: `and`, `or`, `==`, `!=`
- **Unary**: `-` on Int/Float, `!` on Bool

Division and modulo by zero are intentionally left as-is (runtime trap is the correct
behavior).

After folding to `AInit`, the next copy propagation round eliminates the wrapper.

### branch_simp.tw — Branch Simplification

```
Let(t, AIf(ALitBool(true),  then_e, _), body)  ->  splice(then_e, t, body)
Let(t, AIf(ALitBool(false), _, else_e), body)  ->  splice(else_e, t, body)
```

When an `AIf` condition is a literal bool, selects the known branch and splices it
into the continuation. Splicing walks the branch's let-chain:

- If it ends in `Atom(a)`, rewrites to `Let(t, AInit(a), body)`.
- If it ends in a terminal (`Return`/`Break`/`Continue`), drops the unreachable
  continuation.

### defer_elim.tw — Defer Elimination

Removes all `ADefer` nodes by rewriting exit points to execute deferred expressions
in LIFO order before transferring control. Runs **first** in the per-function order
(see Pipeline Overview) so the peephole passes never see `ADefer`.

Threads two defer lists through the walk:
- `fn_defers` — active between current point and function boundary; fired on `Return`
  and normal function exit
- `loop_defers` — active within current loop iteration; fired on `Break`, `Continue`,
  and end-of-iteration

Rewrite rules:
- `ADefer(d)`: registers `d` into `loop_defers` and continues
- `ALoop(body)`: folds `loop_defers` into `fn_defers`, resets `loop_defers = []` for
  the loop body
- `Return`: prepends all defers (fn + loop, LIFO)
- `Break`/`Continue`: prepends only loop defers (LIFO)
- Terminal `Atom`: prepends scope-appropriate defers depending on context

**Capture-by-value**: at registration time, free locals in the deferred expression are
snapshot-bound to fresh locals (`let snap = init(src)`). The deferred body is remapped
to use the snapshots, ensuring it observes values at declaration time, not execution
time. Fresh locals come from `analysis.tw`'s `next_local_id`.

## Support modules

### analysis.tw — Fresh local minting

The uniqueness/liveness analysis that once lived here was removed in the rebuild. The
only survivor the optimizer still needs is `next_local_id(func)`, which `defer_elim`
uses to mint fresh locals for snapshot captures.

### semantics.tw — Shared Optimizer Semantics

Central optimizer-facing metadata for builtin calls. Exposes per-builtin
`CallSemantics` (effect classification `Pure`/`ReadOnly`/`Update`/`Allocate`/`Control`,
`fresh_result`, and COW metadata such as `cow_base_arg`/`in_place_equivalent`/
`retained_args`), plus builder-family config derived from
`compiler/builder_family.tw` and `BuiltinRegistry`.

This is **metadata only** — a static table describing builtins, not an analysis pass.
The `in_place_equivalent` / `cow_base_arg` fields exist so that the census (and, later,
the codegen track) can classify candidate COW ops; the optimizer itself does not emit
any in-place rewrite. `is_pure_with_semantics` here backs the purity check used by
dead-let and copy-prop. `CowConfig` remains a compatibility wrapper for older call
sites/tests, with converters in both directions.

`make_prelude_optimizer_semantics(reg)` builds the prelude semantics from
`BuiltinRegistry`; `optimize_module_with_semantics` is the semantics-first entry point.

## CFG ownership facts (outside opt/)

The sound-uniqueness Phase 1–3 analysis lives in `compiler/`, not `compiler/opt/`:

- `compiler/cfg.tw` — structural CFG view over optimized ANF (blocks, carried block
  params, terminators, predecessor/successor edges, ANF instruction mappings).
- `compiler/ownership.tw` — Phase 2 ownership facts (`Unique`/`Shared`/`Unknown`),
  edge-arg-aware liveness, binding validity, and per-function summaries.
- `compiler/summary.tw` — bottom-up interprocedural summary driver over call-graph
  SCCs.

This analysis is **the single source of truth** for ownership/liveness/publication
facts; the old ownership-consuming optimizer passes were deleted in the rebuild, so
there is no competing legality pass in `opt/`. It is **analysis-only**: it emits no
codegen. Inspect it with `twk ir --cfg`, `twk ir --census`, and `twk ir --sites`.

### Peephole decision: why these stay ANF-local

`dead_let`, `copy_prop`, `const_fold`, and `branch_simp` remain plain ANF-local
peepholes because none of them consult ownership or control-flow facts. Dead-let and
copy-prop reason only about local use counts and syntactic purity; const-fold reasons
only about literal operands; branch-simp reasons only about literal conditions. None
needs the CFG/ownership facts, so there is no reason to route them through the CFG or
to hold them back for the codegen track.
