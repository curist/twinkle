# Boot Compiler — ANF Lowering & Optimization (Phase C)

Last updated: 2026-03-21

## Background

Phases A and B are complete: the self-hosted compiler has a full frontend
(lexer, parser, resolver, type checker) and Core IR lowering with
monomorphization. Phase B produces a `CoreModule` with monomorphized
`FunctionDef`s — concrete types on every node, no type variables remaining.

Phase C transforms this Core IR into ANF (Administrative Normal Form) and
runs optimization passes. ANF makes evaluation order explicit by binding
every intermediate computation to a named local, producing flat let-chains
that are straightforward to emit as Wasm instructions.

## Design Principles

From [self-hosting.md](self-hosting.md):

- **Pure pipeline**: `to_anf(core: CoreModule) -> AnfModule` and
  `optimize(anf: AnfModule) -> AnfModule` are pure functions. State
  (temp counter, op-result-mono map) is threaded functionally.
- **No `Box<T>`**: Twinkle's GC manages all record/enum payloads — recursive
  `AnfExpr` trees just work without explicit boxing.
- **Let-accumulator pattern**: identical to stage0. A `Vector<LetBinding>`
  collects bindings imperatively; `build_lets(accum, tail)` wraps them into
  nested `Let` nodes. No CPS, no closures threading continuations.

## Scope

In scope:
- ANF IR types (`boot/compiler/anf.tw`)
- Core IR → ANF lowering (`boot/compiler/lower_anf.tw`)
- ANF analysis utilities (free/bound/assigned locals, divergence)
- Optimization passes (`boot/compiler/optimize.tw`):
  - Dead let elimination
  - Copy propagation
  - Constant folding
  - Branch simplification
  - Fixed-point loop (up to 10 rounds)
  - Uniqueness rewrite (via `CowConfig` capability record)
  - Liveness analysis (for `annotate_in_place`)
- Defer elimination
- Tests for each milestone

Out of scope (Phase D+):
- `WrapAnyref`/`UnwrapAnyref` boundary insertion (Phase D)
- `plan_wasm_types` (Phase D)

## Input: CoreModule (from Phase B)

```tw
// boot/compiler/core_ir.tw (exists)
type CoreModule = .{
  functions: Vector<FunctionDef>,
  type_env: ResolvedEnv,
  init_func_id: FuncId?,
}

type FunctionDef = .{
  func_id: FuncId,
  name: String,
  params: Vector<Param>,
  body: CoreExpr,
  return_ty: MonoType,
}
```

Every `CoreExpr` carries its `MonoType` in `expr.ty`. After monomorphization,
all types are concrete (no `Var` or `MetaVar`).

---

## Milestone Plan

### M1: ANF IR Types (`boot/compiler/anf.tw`)

Define the ANF data structures, mirroring stage0's `src/ir/anf.rs` with
Twinkle idioms.

**Types to define:**

```tw
pub type OpKind = { Int, Float, Bool, Str }

pub type IndexKind = { Array, Dict, Str }

pub type Atom = {
  ALocal(LocalId),
  AGlobalFunc(FuncId),
  ALitInt(Int),
  ALitFloat(Float),
  ALitBool(Bool),
  ALitStr(String),
  ALitVoid,
}

pub type AnfExpr = {
  Let(LocalId, AnfOp, AnfExpr),
  Atom(Atom),
  Return(Atom?),
  Break(Atom?),
  Continue,
}

pub type AnfOp = {
  ACall(Atom, Vector<Atom>),
  AIf(Atom, AnfExpr, AnfExpr),
  AMatch(Atom, Vector<AnfMatchArm>),
  ALoop(AnfExpr),
  ABinOp(BinOp, Atom, Atom, OpKind),
  AUnOp(UnOp, Atom, OpKind),
  AMakeClosure(FuncId, Vector<LocalId>),
  ARecord(TypeId, Vector<FieldAtom>),
  ARecordGet(Atom, FieldId, TypeId),
  ARecordUpdate(Atom, FieldId, Atom, Bool, TypeId),
  AVariant(TypeId, VariantId, Vector<Atom>),
  AArrayLit(Vector<Atom>),
  AIndex(Atom, Atom, IndexKind, MonoType),
  AInit(Atom),
  AAssign(LocalId, Atom),
  ADefer(AnfExpr),
}

pub type FieldAtom = .{ field: FieldId, value: Atom }
pub type AnfMatchArm = .{ pattern: CorePattern, body: AnfExpr }

pub type AnfFunctionDef = .{
  func_id: FuncId,
  name: String,
  params: Vector<Param>,
  op_result_mono: Dict<Int, MonoType>,
  body: AnfExpr,
  return_ty: MonoType,
}

pub type AnfModule = .{
  functions: Vector<AnfFunctionDef>,
  init_func_id: FuncId?,
}
```

**Key differences from stage0:**
- `Param` reused from `core_ir.tw` (has `local: LocalId` + `ty: MonoType`)
  instead of parallel `params` + `param_tys` vectors.
- `FieldAtom` record instead of tuple `(FieldId, Atom)`.
- `ARecordUpdate` carries `can_reuse: Bool` positionally (always `false`
  at lowering time; set by liveness `annotate_in_place` and uniqueness).
- No `all_init_func_ids` (multi-module is Phase E).
- `CowConfig` and related types also defined here — shared between
  `opt/uniqueness.tw` (consumer) and Phase D (provider).

**Validation:** types compile, can construct and pattern-match each variant.

---

### M2: Core ANF Lowering — Atoms & Simple Ops (`boot/compiler/lower_anf.tw`)

Implement the lowering scaffold and handle the straightforward cases.

**Functions:**

```tw
pub fn lower_module(core: CoreModule) AnfModule
fn lower_func(func: FunctionDef) AnfFunctionDef

// State threaded through lowering
type LowerState = .{
  next_temp: Int,
  op_result_mono: Dict<Int, MonoType>,
}

fn fresh(state: LowerState) (LocalId, LowerState)

type LetAccum = Vector<LetBinding>
type LetBinding = .{ local: LocalId, op: AnfOp }

fn build_lets(accum: LetAccum, tail: AnfExpr) AnfExpr
fn push_accum(accum: LetAccum, state: LowerState, local: LocalId, op: AnfOp, ty: MonoType) (LetAccum, LowerState)

fn lower_expr_top(expr: CoreExpr, state: LowerState) (AnfExpr, LowerState)
fn lower_expr(expr: CoreExpr, state: LowerState, accum: LetAccum) (AnfExpr, LowerState, LetAccum)
fn atomize(expr: CoreExpr, state: LowerState, accum: LetAccum) (Atom, LowerState, LetAccum)
```

**Cases handled in M2:**
- Literals → `Atom` directly
- `Local` / `GlobalLocal` → `ALocal`
- `GlobalFunc` → `AGlobalFunc`
- `BinOp` / `UnOp` → atomize operands, push op
- `Call` → atomize callee and all args, push `ACall`
- `MakeClosure` → push `AMakeClosure`
- `Record` / `RecordGet` / `RecordUpdate` → atomize, push op
- `Variant` / `ArrayLit` / `Index` → atomize, push op

**Helper functions:**
- `op_kind_from(ty: MonoType) OpKind` — maps `MonoType` to `OpKind`.
  Handles `Int`, `Byte` (→ `Int`), `Float`, `Bool`, `String`.
  Note: `Byte` maps to `Int` because byte arithmetic uses integer Wasm ops.
- `index_kind_from(ty: MonoType) IndexKind` — maps base type to `IndexKind`
- `type_id_from(ty: MonoType) TypeId` — extracts TypeId from Named type
- `max_local_id(func: FunctionDef) Int` — walk Core IR to find max local ID

**Threading pattern:** Since Twinkle has no `&mut`, state is threaded
explicitly. Each function takes `LowerState` and returns updated
`LowerState`. The accumulator `LetAccum` is similarly threaded.

**Test:** Lower a function with `let x = 1 + 2; x * 3` and verify the
ANF output has three let-bindings terminating in an atom.

---

### M3: Control Flow Lowering

Handle `If`, `Match`, `Loop`, `Break`, `Continue`, `Return`.

**Cases:**
- `If(cond, then, else)` → atomize cond, lower branches independently
  via `lower_expr_top`, push `AIf`, bind to fresh temp
- `Match(scrutinee, arms)` → atomize scrutinee, lower each arm body
  independently, push `AMatch`
- `Loop(body)` → lower body independently, push `ALoop`
- `Break(expr?)` → if value, atomize it first (into current accum), then
  return `Break(atom?)` terminal
- `Continue` → return `Continue` terminal
- `Return(expr?)` → same pattern as Break

**Structural forms in atomize position:** When `If`/`Match`/`Loop` appear
as subexpressions (e.g., `foo(if c { a } else { b })`), they must be
atomized. The approach:
1. Lower the structural form via `lower_expr_top` → full `AnfExpr`
2. `splice_atom_bind`: rewrite the tail atom to a fresh local binding
3. `flatten_into_accum`: extract all `Let` nodes into the current accum

**Test:** Lower `if true { 1 } else { 2 }` in both tail and operand
positions. Lower a loop with break. Lower a match with two arms.

---

### M4: Let & Assign Lowering

Handle `Let(local, value, body)` and `Assign(local, value)`.

**`Let` lowering:**
1. Lower `value` in a separate `value_accum`
2. If value reduces to an atom: push `(orig_local, AInit(atom))` into outer
   accum, then continue lowering `body` with the same outer accum
3. If value reduces to a terminal (diverges): the body is unreachable —
   return `build_lets(value_accum, terminal)` without processing body

**`Assign` lowering:**
1. Atomize `value` into current accum
2. Push `(fresh_discard, AAssign(local, atom))` to accum
3. Return `Atom(ALitVoid)`

**`Defer` lowering:**
1. Lower inner expression independently via `lower_expr_top`
2. Push `(fresh_discard, ADefer(inner_anf))` to accum
3. Return `Atom(ALitVoid)`

**Test:** Lower `let x = 1; let y = x + 2; y` and verify correct binding
order. Lower an assign-in-loop pattern.

---

### M5: ANF Analysis Utilities (`boot/compiler/opt/analysis.tw`)

Implement the canonical analysis functions, reusable by the optimizer and
(later) the codegen.

**Functions:**

```tw
// Collect locals referenced but not declared within expr
fn collect_free_locals(expr: AnfExpr, declared: Dict<Int, Bool>) Dict<Int, Bool>

// Collect locals declared (let-bound) within expr
fn collect_bound_locals(expr: AnfExpr) Dict<Int, Bool>

// Collect locals that are targets of AAssign within expr
fn collect_assigned_locals(expr: AnfExpr) Dict<Int, Bool>

// Collect locals bound by a pattern
fn collect_pattern_bindings(pattern: CorePattern) Dict<Int, Bool>

// Does the expression always diverge (Return/Break/Continue on every path)?
fn expr_always_diverges(expr: AnfExpr) Bool
fn op_always_diverges(op: AnfOp) Bool
```

**Dict<Int, Bool> as set:** Twinkle has no `HashSet`; use `Dict<Int, Bool>`
with helper functions `set_has(s, k)` / `set_add(s, k)` / `set_union(a, b)`.

**Test:** Verify free-local analysis on a closure capturing two vars.
Verify divergence detection on a function with early return.

---

### M6: Dead Let Elimination & Copy Propagation (`opt/use_count.tw`, `opt/dead_let.tw`, `opt/copy_prop.tw`)

First two peephole passes.

**Use counting — two distinct functions (critical):**

```tw
// Count ALL references to each local, including AMakeClosure.free_vars
// Used by dead-let elimination (a local with zero total uses is dead)
fn count_uses(expr: AnfExpr) Dict<Int, Int>

// Count references EXCLUDING AMakeClosure.free_vars positions
// Used by copy propagation (free_var slots hold LocalIds and cannot
// accept literal substitution — a local used only as a free_var must
// not be propagated away)
fn count_uses_excluding_free_vars(expr: AnfExpr) Dict<Int, Int>

// Is an AnfOp pure (safe to eliminate if unused)?
fn is_pure(op: AnfOp) Bool
```

Purity: `AInit`, `ABinOp` (except int div/mod), `AUnOp`, `ARecord`,
`ARecordGet`, `ARecordUpdate`, `AVariant`, `AArrayLit`, `AMakeClosure`
are pure. `ACall`, `AAssign`, `AIndex`, `ADefer` are impure.
`AIf`/`AMatch`/`ALoop` are conservatively impure.

**Dead let elimination:**
Remove `Let(t, op, body)` where `count_uses[t] == 0` and `is_pure(op)`.
Returns `(AnfExpr, Bool)` — the rewritten expr and whether anything changed.

**Copy propagation:**
Inline `Let(t, AInit(lit), body)` where `lit` is a non-local atom (literal
or global func ref) and `count_uses_excluding_free_vars[t] == 1`.
This ensures locals whose only use is inside `AMakeClosure.free_vars` are
never propagated away (the free_var slot cannot hold a literal).

Substitution: walk the body replacing `ALocal(t)` with the literal atom.

**Test:** Verify dead-let removes `let t = 1 + 2` when t is unused.
Verify copy-prop inlines `let t = 42; ... t ...` when t is used once.
Verify copy-prop does NOT inline a local whose only use is as a
closure free_var.

---

### M7: Constant Folding & Branch Simplification (`opt/const_fold.tw`, `opt/branch_simp.tw`)

**Constant folding:**
Evaluate `ABinOp(op, lit_left, lit_right, kind)` and
`AUnOp(op, lit, kind)` when all operands are literals. Replace with
`AInit(result_literal)`. Leave int div/mod by zero as-is (runtime trap).

Covers: int/float arithmetic, comparisons, bool and/or/eq/ne, unary
neg/not. Note: `BitNot` is absent from the boot `UnOp` type (only `Neg`
and `Not` exist). If `BitNot` is added later, constant folding and
`op_kind_from` must be updated to handle it.

**Branch simplification:**
Replace `Let(t, AIf(ALitBool(b), then, else), body)` with the known
branch. The selected branch's tail atom becomes `AInit(atom)` bound to `t`.

`splice_branch` helper: if the selected branch terminates
(Return/Break/Continue), the continuation `body` is unreachable — drop it.

**Test:** Verify `1 + 2` folds to `3`. Verify `if true { a } else { b }`
simplifies to `a`. Verify `if false { return 1 } else { 2 }; x` simplifies
to `let t = 2; x`.

---

### M8: Fixed-Point Optimization Loop (`opt/pipeline.tw`)

Wire the four passes into a fixed-point loop with module-level orchestration.

```tw
pub fn optimize_module(module: AnfModule) AnfModule
fn optimize_func(func: AnfFunctionDef, pinned: Dict<Int, Bool>) AnfFunctionDef
```

**Pinned locals:** For `__init__` functions, compute the set of locals
that are referenced as free variables by other functions. These must not
be dead-eliminated or propagated away.

```tw
// Intersect bound locals of __init__ with free locals of all other functions.
// Important: seed collect_free_locals with each function's own param locals
// as the initial declared set, so params are not incorrectly counted as free.
fn compute_pinned(module: AnfModule) Dict<Int, Bool>
```

**Fixed-point loop:** Run dead-let → copy-prop → const-fold → branch-simp
up to 10 rounds. Break early when no pass reports `changed = true`.

**Post-loop passes (run once, in order):**
1. `annotate_in_place` (liveness-based record update optimization)
2. `uniqueness_rewrite` (COW elimination, takes `CowConfig`)
3. `eliminate_defers` (defer inlining at scope exits)

**Test:** Verify `let x = 1; let y = x + 2; y * 3` optimizes to
`let t = 9` (const-fold chains through copy-prop across rounds).

---

### M9: Liveness Analysis (`opt/liveness.tw`)

Backward dataflow analysis computing which locals are live at a given
point. Used by uniqueness rewrite and `annotate_in_place`.

**Functions:**

```tw
// Compute the set of locals live immediately after this expression
fn live_after(expr: AnfExpr, live_out: Dict<Int, Bool>) Dict<Int, Bool>

// Walk ARecordUpdate nodes; set can_reuse_in_place = true when the
// base local is dead in the continuation
fn annotate_in_place(func: AnfFunctionDef) AnfFunctionDef
```

**Semantics:** Backward walk from the tail of the function. At each
`Let(t, op, body)`: compute `live_out` of the op from `live_in` of the
body, minus `t`, plus locals referenced in `op`. Loops are conservative:
all locals read anywhere in the loop body are treated as live at loop
entry (fixed-point not needed — single conservative pass).

**`annotate_in_place`:** For each `ARecordUpdate { base, ... }`, check
if the base local (when `base` is `ALocal(id)`) is absent from
`live_after(body)`. If dead, set `can_reuse_in_place = true` — the WAT
emitter can use `struct.set` instead of allocating a new struct.

**Test:** Verify liveness correctly identifies dead locals after their
last use. Verify `annotate_in_place` marks record updates where the
base is consumed.

---

### M10: Uniqueness Rewrite (`opt/uniqueness.tw`)

Eliminate unnecessary COW allocations by proving single-ownership.
Decoupled from codegen via a `CowConfig` capability record.

**Bridge layer — `CowConfig`:**

```tw
// Describes a COW-aware call: base argument position and optional
// in-place rewrite target
pub type CowOpEntry = .{
  base_arg: Int,
  in_place_id: FuncId?,
}

// Describes a builder rewrite for the loop accumulator pattern
pub type BuilderConfig = .{
  push_id: FuncId,          // VECTOR_PUSH
  builder_new_id: FuncId,   // VECTOR_BUILDER_NEW
  builder_from_id: FuncId,  // VECTOR_BUILDER_FROM
  builder_push_id: FuncId,  // VECTOR_BUILDER_PUSH
  builder_freeze_id: FuncId, // VECTOR_BUILDER_FREEZE
}

// All FuncId knowledge the uniqueness pass needs, provided by the caller
pub type CowConfig = .{
  // FuncId → CowOpEntry for COW-aware calls
  cow_ops: Dict<Int, CowOpEntry>,
  // FuncIds that produce fresh (unique) values
  fresh_producer_ids: Dict<Int, Bool>,
  // FuncIds that are read-only and don't retain references
  read_only_ids: Dict<Int, Bool>,
  // Builder rewrite config (for loop region rewrite)
  builder: BuilderConfig,
}
```

Phase D provides the concrete `CowConfig` wired to actual prelude FuncIds.
Phase C implements the pass logic and can be tested with a mock config.

**Algorithm (same as stage0):**

Phase 1 — Pre-scan: build `tainted` set of locals that cannot be unique
(params, aliased, captured in closures, stored in containers, passed to
non-COW calls). COW-aware calls (looked up via `config.cow_ops`) taint
only non-base args.

Phase 2 — Forward rewrite: track `unique: Dict<Int, Bool>`. Fresh
producers (`AArrayLit`, `ARecord`, `AVariant`, calls to
`config.fresh_producer_ids`) make their result unique. COW ops on a
unique non-tainted base are rewritten to their `in_place_id`.

Phase 3 — Loop region rewrite: detect the accumulator pattern
(`xs = []; for x in coll { xs = push(xs, x) }`) using
`config.builder.push_id`, rewrite to builder new/push/freeze.

```tw
fn uniqueness_rewrite(func: AnfFunctionDef, config: CowConfig) AnfFunctionDef
```

**Test:** Build a mock `CowConfig` with test FuncIds. Verify:
- `set_unsafe(unique_vec, i, v)` rewrites to `set_in_place`
- Tainted locals are not rewritten
- Loop accumulator pattern rewrites to builder

---

### M11: Defer Elimination (`opt/defer_elim.tw`)

Transform `ADefer` nodes into inline execution at scope exits.

**Ordering:** Defer elimination must run as a final post-loop pass, after
the fixed-point optimization loop completes. It restructures terminal nodes
(Return/Break/Continue/Atom) irreversibly and is not idempotent with
respect to the peephole passes. In stage0, it runs after uniqueness
rewrite; defer elimination is the last pass in the pipeline.

**Algorithm:**
- Thread two defer lists: `fn_defers` (outside loops, fired on Return and
  function tail) and `loop_defers` (inside loops, fired on Break/Continue
  and loop tail).
- When encountering `ADefer(body)`: snapshot free locals into fresh locals
  (capture semantics — see values at defer-time, not exit-time), add to
  the current defer list.
- At each exit point (Return/Break/Continue/tail Atom), prepend deferred
  expressions in LIFO order before the exit.
- **`in_sub_expr` flag:** When descending into `AIf`/`AMatch` arm bodies
  (via the op-level elimination function), set `in_sub_expr = true`. When
  descending into `ALoop` bodies, keep `in_sub_expr = false` — loop body
  tail IS a real scope exit. When `in_sub_expr` is true, terminal `Atom`
  nodes do NOT fire defers (they are value-producing positions, not scope
  exits). Only `Return`/`Break`/`Continue` and true function/loop tail
  atoms fire defers.

**Capture semantics:**
```tw
// At defer registration:
let snap_x = init(x)    // snapshot current value of x
// ... more code that may mutate x ...
// At scope exit:
<deferred body with snap_x substituted for x>
```

**Test:** Verify defer executes at function return. Verify defer inside
a loop executes at break and continue. Verify defer captures values at
registration time, not exit time.

---

### M12: Integration & End-to-End Testing

Wire the full pipeline: `CoreModule → lower_module → optimize_module → AnfModule`.

**Integration tests:**
- Round-trip: lower → optimize → verify the ANF output is well-formed
  (every referenced local is declared, no `ADefer` nodes survive after
  optimize)
- Comparison tests: compile the same `.tw` programs through both stage0
  and boot, compare pretty-printed ANF output after normalization (local
  IDs may differ, but structure should match). Note: boot `AnfModule`
  cannot be fed directly to stage0's WAT emitter due to layout differences
  (`Vector<Param>` vs parallel vectors, no `all_init_func_ids`).
  Behavioral equivalence is verified in Phase D via Wasm execution output.

**Pretty-printer:** Implement `anf_to_string(module: AnfModule) String`
for debugging and snapshot tests.

---

## File Layout

```
boot/compiler/
  anf.tw              # M1: ANF IR types + CowConfig
  lower_anf.tw        # M2–M4: Core IR → ANF lowering
  opt/
    analysis.tw       # M5: free/bound/assigned locals, divergence
    use_count.tw      # M6: count_uses, count_uses_excluding_free_vars, is_pure
    dead_let.tw       # M6: dead let elimination
    copy_prop.tw      # M6: copy propagation
    const_fold.tw     # M7: constant folding
    branch_simp.tw    # M7: branch simplification
    liveness.tw       # M9: liveness analysis, annotate_in_place
    uniqueness.tw     # M10: uniqueness rewrite (takes CowConfig)
    defer_elim.tw     # M11: defer elimination
    pipeline.tw       # M8: fixed-point loop, optimize_module/optimize_func
```

Each pass is a self-contained module with a clear single responsibility.
`pipeline.tw` imports all passes and wires them into the optimization
pipeline. This mirrors stage0's `src/opt/` directory structure.

## Rust ↔ Twinkle Mapping

| Rust concept | Twinkle equivalent |
|---|---|
| `HashMap<LocalId, MonoType>` | `Dict<Int, MonoType>` |
| `HashSet<LocalId>` | `Dict<Int, Bool>` |
| `Vec<(LocalId, AnfOp)>` (LetAccum) | `Vector<LetBinding>` record |
| `&mut u32` (next_temp) | `Int` field in `LowerState`, threaded |
| `Box<AnfExpr>` | `AnfExpr` (GC-managed, no Box needed) |
| `clone()` | implicit (GC references share freely) |
| `(body, changed)` return | `(AnfExpr, Bool)` or a result record |
| Pattern matching on `&CoreExprKind` | `case expr.kind { ... }` |
| `MAX_ROUNDS: usize = 10` | `max_rounds := 10` |

## Dependencies

- `boot/compiler/core_ir.tw` (Phase B — exists)
- `boot/compiler/resolver.tw` for `MonoType`, `TypeId`, `ResolvedEnv`
- `boot/compiler/ast.tw` for `BinOp`, `UnOp`

## Risks & Mitigations

**Deep recursion on large ANF trees:** Twinkle compiles to Wasm, which has
a limited call stack. The let-accumulator pattern keeps recursion shallow
for the lowering pass (linear in let-chain depth, not expression depth).
Optimization passes walk the tree recursively but each `Let` node has
bounded branching. Monitor stack depth on large boot compiler self-tests.

**Performance of Dict<Int, Bool> as sets:** Dict is currently a linear
scan. For the optimization fixed-point loop, this could be slow on
functions with many locals. Acceptable for Phase C — the persistent HAMT
plan ([persistent-dict.md](persistent-dict.md)) will address this if
needed.

**State threading verbosity:** Every function returns updated state,
making signatures longer than stage0's `&mut` style. This is a deliberate
trade-off for purity. Consider a thin `with_fresh(state, fn)` helper if
the pattern becomes unwieldy.
