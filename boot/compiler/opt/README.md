# ANF Optimization Pipeline

The boot compiler's optimization pipeline transforms ANF IR (Administrative Normal Form)
to reduce redundant computation, eliminate dead code, and convert copy-on-write operations
to in-place mutations where safe. All passes operate directly on the structured ANF tree
-- no CFG is constructed.

## Pipeline Overview

`pipeline.tw` orchestrates two phases:

1. **Fixed-point loop** (max 10 rounds): dead let elimination, copy propagation,
   constant folding, branch simplification -- repeated until no pass reports a change.
2. **Post-loop passes** (single shot each): liveness-based record update annotation,
   uniqueness/COW rewrite, defer elimination.

```
AnfModule
  -> per function:
       loop {
         dead_let_elim -> copy_propagate -> constant_fold -> branch_simplify
       } until stable (or 10 rounds)
       -> annotate_in_place   (liveness)
       -> uniqueness_rewrite  (COW elimination)
       -> eliminate_defers    (must be last)
  -> optimized AnfModule
```

### Pinned locals

Module-level `__init__` functions may bind locals that other functions reference
(module-scope variables). `compute_pinned` identifies these cross-function locals so
that dead let elimination and copy propagation never remove or inline them.

## Passes

### use_count.tw -- Use Counting

Foundation for dead let elimination and copy propagation. Walks the ANF tree and counts
how many times each `LocalId` appears in operand/atom position.

- `count_uses(expr)` -- counts all references including `AMakeClosure.free_vars`
- `count_uses_excluding_free_vars(expr)` -- excludes closure free var positions
  (used by copy propagation, since inlining a literal into a free var slot is invalid)
- `is_pure(op)` -- pure-op predicate: true for `AInit`, `ABinOp` (except int div/mod),
  `AUnOp`, `ARecord`, `ARecordGet`, `ARecordUpdate`, `AVariant`, `AArrayLit`,
  `AMakeClosure`, and structurally pure `AIf`/`AMatch`. False for `ACall`, `AAssign`,
  `ALoop`, `ADefer`.

Let binders and `AAssign` targets are not counted as uses.

### dead_let.tw -- Dead Let Elimination

```
Let(t, pure_op, body)  where  uses[t] == 0  and  t not assigned  ->  body
```

Removes let-bindings whose bound local is never referenced and whose right-hand side
is pure (no side effects). Locals that appear as `AAssign` targets are preserved even
at zero use count, since the assignment itself may be meaningful. Pinned locals are
merged into the assigned set so they are never eliminated.

Recurses into `AIf`, `AMatch`, `ALoop`, and `ADefer` sub-expressions.

### copy_prop.tw -- Copy Propagation

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
accept arbitrary atoms) don't inflate the use count.

Provides `subst_atom(expr, target, replacement)` as a general-purpose atom substitution
utility, with a shadow-stop guard: if a nested `Let` rebinds the target, substitution
stops in that scope.

### const_fold.tw -- Constant Folding

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

### branch_simp.tw -- Branch Simplification

```
Let(t, AIf(ALitBool(true),  then_e, _), body)  ->  splice(then_e, t, body)
Let(t, AIf(ALitBool(false), _, else_e), body)  ->  splice(else_e, t, body)
```

When an `AIf` condition is a literal bool, selects the known branch and splices it
into the continuation. Splicing walks the branch's let-chain:

- If it ends in `Atom(a)`, rewrites to `Let(t, AInit(a), body)`.
- If it ends in a terminal (`Return`/`Break`/`Continue`), drops the unreachable
  continuation.

### liveness.tw -- Liveness Analysis & Record Update Annotation

**Liveness**: backward dataflow walk computing `live_after(expr)` -- the set of locals
that may be read at or after each program point.

- `Let(t, op, body)`: start from `live(body)`, kill `t`, add locals read by `op`
- `AIf`/`AMatch`: conservative union of all branch live sets
- `ALoop`: conservative -- all locals read anywhere in the loop body are live
- `AAssign(target, value)`: kills `target`, adds locals in `value`

**annotate_in_place**: walks `ARecordUpdate` nodes and sets `can_reuse_in_place = true`
when the base local is dead in the continuation. This tells the WAT backend it may emit
`struct.set` (in-place mutation) instead of allocating a new struct.

### semantics.tw -- Shared Optimizer Semantics

Central optimizer-facing metadata for builtin calls and structured ANF ops.
The current stage exposes:

- effect classification (`Pure`, `ReadOnly`, `Update`, `Allocate`, `Control`)
- fresh-result metadata
- COW/in-place rewrite metadata
- builder-family metadata

Builder-family ids now come from the shared
`compiler/builder_family.tw` helper so optimizer code and front-end
lowering derive the same builder family from `BuiltinRegistry`.

`pipeline.tw` now builds prelude optimizer semantics from `BuiltinRegistry`,
and `uniqueness.tw` consumes those semantics directly. `CowConfig` remains as a
compatibility wrapper for older call sites/tests.

### uniqueness.tw -- Uniqueness Rewrite (COW Elimination)

Proves single-ownership of collection values to rewrite copy-on-write operations to
in-place mutations. The active path now consumes shared optimizer semantics
rather than hardcoded builtin tables. It now relies on shared analysis helpers
in `analysis.tw`, and delegates loop-region builder construction to
`loop_builder.tw`.

**Phase 1 -- Pre-scan**: builds a `tainted` set of locals that can never be unique:
- Function parameters (come from outside)
- Locals captured by closures (`AMakeClosure` free vars)
- Locals stored in containers (array literals, record fields, variant args)
- Locals passed to non-COW, non-read-only calls
- Alias copies (`let y = x` or `y = x`) where the source is still live after

**Phase 2 -- Forward rewrite**: tracks a `unique` set of locals known to have sole
ownership. Forward walk through the let-chain:
- Fresh producers (`vector_make`, `dict_new`, array/record/variant literals) make
  their result unique
- `AInit(ALocal(src))` transfers uniqueness from source to target (source loses it)
- `AAssign(target, ALocal(src))` similarly transfers
- COW ops (`vector_set_unsafe`, `dict_set`, `dict_remove`) on a unique, non-tainted
  base with a consume-reassign or dead-base pattern are rewritten to their in-place
  counterparts (`vector_set_in_place`, `dict_set_in_place`, `dict_remove_in_place`)
- `ARecordUpdate` on unique base gets `can_reuse_in_place = true`
- Branches/loops are conservative: unique sets are not propagated out

**Phase 3 -- Loop region rewrite**: transforms the accumulator pattern
`v = []; for ... { v = v.push(x) }` into the builder pattern
`b = builder_new(); for ... { builder_push(b, x) }; v = builder_freeze(b)`.

Loop legality analysis lives in `analysis.tw`; the builder-region rewrite itself
lives in `loop_builder.tw`, which now emits an optimizer-facing `BuilderRegion`
and lowers that canonical region shape through `builder_region.tw`. Analysis
validates that the base local is only used in push+reassign patterns within the
loop body. Rewriting introduces three fresh locals (builder, freeze result,
assign) and replaces `vector_push` calls with `builder_push`. A safety check
verifies the rewritten site count matches the analysis.

Also handles `builder_from` for non-empty initial vectors vs `builder_new` for
initially-empty ones (tracked via `known_empty` set).

### loop_builder.tw -- Loop Builder Region Rewrite

Owns loop-accumulator candidate rewriting once legality has already been
established. It rewrites push sites inside the loop body, chooses the builder
region seed (`builder_new` vs `builder_from`), and constructs a canonical
`BuilderRegion` for lowering.

### builder_region.tw -- Canonical Builder Region Lowering

Defines a small optimizer-facing transient builder region abstraction and lowers
it to the current runtime builder call family. This is the Stage 4 bridge:
passes can target a stable builder-region concept without directly assembling the
final nested ANF `Let` shape around `vector_builder_*` calls.

This is the current intended stop point. The optimizer does not yet introduce
explicit transient IR nodes; Stage 5 IR refinement is deferred unless the shared
builder-family boundary stops being sufficient.

### defer_elim.tw -- Defer Elimination

Removes all `ADefer` nodes by rewriting exit points to execute deferred expressions
in LIFO order before transferring control.

Threads two defer lists through the walk:
- `fn_defers` -- active between current point and function boundary; fired on `Return`
  and normal function exit
- `loop_defers` -- active within current loop iteration; fired on `Break`, `Continue`,
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
time.

Must run last in the pipeline -- after all peephole passes, since it changes control
flow structure in ways the peephole passes aren't designed to handle.

## Configuration

`make_prelude_optimizer_semantics` in `semantics.tw` builds optimizer semantics
for Twinkle's prelude from `BuiltinRegistry`.

`optimize_module_with_semantics` in `pipeline.tw` is the semantics-first entry
point. `optimize_module_with_config` remains available as a compatibility layer
and internally converts `CowConfig` to optimizer semantics.
