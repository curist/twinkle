# Optimization — Stages 7, 7.5, 7.6

## Stage 7 — ANF IR (Backend-Oriented) ✅

**Goal:** Add an ANF (Administrative Normal Form) IR layer between Core IR and the WAT
backend. ANF makes evaluation order explicit and ensures every non-trivial intermediate
value is bound to a named local — a requirement for straightforward WAT/Wasm code generation.

Ordering note:

* Keep ANF at this stage (after interpreter + generics), not before Stage 5.
* This keeps execution semantics anchored by Core IR first, then introduces
  backend-oriented normalization.

**Scope decision:** The interpreter continues to use Core IR. No ANF interpreter is added
at this stage. Behavioral preservation is validated by structural invariant checks and
golden output snapshots; full equivalence testing against the interpreter is deferred to
Stage 8 where the WAT backend becomes the second execution path.

ANF IR structure (full spec in `docs/internals/ir.md §3`):

* **Atom** — trivially available values (locals or literals):
  * `ALocal(LocalId)`, `ALitInt(i64)`, `ALitFloat(f64)`, `ALitBool(bool)`,
    `ALitStr(String)`, `ALitVoid`.

* **ANFExpr** — a flat let-chain terminating in an atom:
  * `Let { local: LocalId, op: AnfOp, body: Box<ANFExpr> }`
  * `Return(Atom)` — function return (terminal).
  * `Break(Option<Atom>)` — loop break (terminal).
  * `Continue` — loop continue (terminal).

  > Note: `Break`/`Continue` are terminal `ANFExpr` variants, not `AnfOp` entries,
  > because they carry no value to bind and the body after them is unreachable.

* **AnfOp** — a single non-atomic computation whose result is bound by the enclosing `Let`:
  * `ACall { callee: Atom, args: Vec<Atom> }`
  * `AIf { cond: Atom, then_branch: ANFExpr, else_branch: ANFExpr }`
  * `AMatch { scrutinee: Atom, arms: Vec<AnfMatchArm> }`
  * `ALoop { body: ANFExpr }`
  * `ABinOp { op: BinOp, left: Atom, right: Atom }`
  * `AUnOp { op: UnOp, expr: Atom }`
  * `AMakeClosure { func_id: FuncId, free_vars: Vec<Atom> }`
  * `ARecord { type_id: TypeId, fields: Vec<(FieldId, Atom)> }`
  * `ARecordGet { target: Atom, field: FieldId }`
  * `ARecordUpdate { base: Atom, field: FieldId, value: Atom }`
  * `AVariant { type_id: TypeId, variant: VariantId, args: Vec<Atom> }`
  * `AArrayLit(Vec<Atom>)`
  * `AIndex { base: Atom, index: Atom }`
  * `AAssign { local: LocalId, value: Atom }` — maps to Wasm `local.set`.

Core → ANF lowering rules (from `docs/internals/ir.md §4`):

* **A1** — Non-atom subexpressions are let-bound to fresh temporaries before use.
  The lowering is continuation-passing: `lower_expr(expr, cont)` where `cont` is
  the rest of the computation that expects an `Atom`.
* **A2** — `If` cond is atomized; branches are lowered recursively into `ANFExpr`.
* **A3** — `Match` scrutinee is atomized; arm bodies lowered recursively.
* **A4** — `Loop` body lowered independently into `ANFExpr`.
* **A5** — `MakeClosure` free vars are already locals (atoms); lambda body lowered
  as an independent function.

Fresh temporaries: a simple counter per function, starting above the function's
existing max `LocalId`. No need for the full `LocalAllocator`.

Deliverables:

* `twk lower-anf file.tw` prints ANF IR in a readable form.
* All programs in `tests/run/` pass ANF invariant checks (see Step D).
* Golden ANF output snapshots for a representative subset of test programs.

**Execution checklist (file/module map):**

* **Step A — ANF IR type definitions (`src/ir/anf.rs`)**
  * Define `Atom`, `AnfExpr`, `AnfOp`, `AnfMatchArm` per the structure above.
  * Define `AnfFunctionDef { func_id: FuncId, params: Vec<LocalId>, body: AnfExpr, return_ty: MonoType }`.
  * Define `AnfModule { functions: Vec<AnfFunctionDef>, init_func_id: FuncId }` mirroring `CoreModule`.
  * Implement `Display` (or a `pretty_print`) for `AnfExpr` — used by `twk lower-anf`.
  * Register `pub mod anf` in `src/ir/mod.rs`; re-export `AnfModule`.

* **Step B — Core → ANF lowering pass (`src/ir/lower_anf.rs`)**
  * Entry point: `pub fn lower_module(module: &CoreModule) -> AnfModule`.
  * Per-function: `lower_func(func: &FunctionDef) -> AnfFunctionDef`.
  * Core expression lowering via CPS: `lower_expr(expr: &CoreExpr, cont: impl FnOnce(Atom) -> AnfExpr) -> AnfExpr`.
    * Atomic cases (`LitInt`, `LitBool`, `Local`, etc.) call `cont` directly with the atom.
    * Non-atomic cases (e.g. `BinOp`, `Call`, `Record`) recursively atomize their subexpressions,
      allocate a fresh `LocalId`, emit `Let(tmp, AnfOp, cont(ALocal(tmp)))`.
    * Terminal cases (`Break`, `Continue`, `Return`) emit the terminal `ANFExpr` variant directly
      (ignore `cont` — unreachable after a terminal).
    * Structural cases (`If`, `Match`, `Loop`) atomize their guard/scrutinee and recurse into branches.
  * Fresh temp counter: track `next_temp: u32` per function, initialized to `max(params) + 1` or
    the function's local count.

* **Step C — CLI command (`src/cli/lower_anf.rs`)**
  * Implement `pub fn cmd_lower_anf(path: &Path) -> anyhow::Result<()>` using the same pipeline
    as `twk lower`: parse → resolve → typecheck → lower (Core IR) → `lower_anf::lower_module`.
  * Wire as `twk lower-anf <file>` in `src/cli/mod.rs` and `src/main.rs`.
  * Fix stale comment in `src/codegen/mod.rs`: change `// WAT/Wasm backend - Stage 7` to
    `// WAT/Wasm backend - Stage 8`.

* **Step D — Tests (`tests/anf_test.rs`)**
  * **Invariant checker** (`fn check_anf_invariants(module: &AnfModule)`): walk `AnfExpr` and assert:
    * All `ACall` args are `Atom` (no nested expressions).
    * All `ARecord` field values are `Atom`.
    * All `AVariant` args are `Atom`.
    * All `ABinOp`/`AUnOp` operands are `Atom`.
    * `Let` body is never immediately another `Let` wrapping the same op (no redundant nesting).
  * Run the invariant checker on every `tests/run/*.tw` program as part of `cargo test`.
  * **Golden snapshot tests**: pick a handful of simple programs (e.g. `hello.tw`, `arithmetic.tw`,
    `closures.tw`, `records.tw`) and snapshot their `twk lower-anf` output; fail on diff.

---

## Stage 7.5 — Dataflow Analysis & ANF Optimization ✅

**Goal:** Introduce a dataflow-aware optimization pass over ANF IR — computing use-def
information and applying peephole rewrites — to reduce redundant computation before WAT
emission. Also provides liveness-based last-use proof for safe functional-update annotation,
consumed by the Stage 8 WAT backend.

**Scope decision:**

* Optimizations operate directly on ANF IR. No separate CFG IR is introduced.
* **Why no flat basic-block CFG:** WAT uses structured control flow (`block`/`loop`/`if`),
  not arbitrary jumps. A flat CFG would require a re-structuring pass before WAT emission,
  making it wasted work for this target. ANF's `AIf`/`ALoop`/`AMatch` structure already maps
  directly to WAT constructs. Dataflow analysis (use counting, liveness) is equally expressible
  as a tree-walk over structured ANF — no flattening needed.
* The same reasoning applies to `defer` (Stage 7.6): defer elimination can be implemented as
  an ANF tree-walk pass that threads scope-aware defer lists, rather than CFG edge insertion.
  See Stage 7.6 for details.
* CFG construction is deferred indefinitely; if advanced whole-function analysis is ever needed
  (e.g. alias analysis for array/dict in-place rewriting), it can be added on top of ANF at
  that point.
* The Core IR interpreter is unchanged. Semantic correctness is validated by structural invariant
  checks (ANF invariants still hold post-optimization) and by formal argument per rewrite rule;
  runtime differential testing awaits the Stage 8 WAT backend.
* Functional-update annotation (Step C) only sets flags on ANF nodes; no evaluation semantics
  change in Stage 7.5. The WAT backend reads the flags.

**Pipeline addition:**

```text
Core IR → ANF IR → [Stage 7.5] optimized ANF IR → WAT/Wasm backend
```

**Optimization passes (concrete rules):**

**A — Use counting**
Walk `AnfExpr` recursively and count `ALocal(id)` appearances in *operand* position.
Exclude `Let.local` (the binding site, not a use) and `AAssign.local` (a write target, not a read).
Include everything else: atoms in `ABinOp`, `ACall.args`, `ACall.callee`, `ARecordGet.target`, etc.
Result: `HashMap<LocalId, usize>` (absent key = 0 uses).

**B — Pure-op predicate**
`is_pure(op) = true` for: `AInit`, `ABinOp`, `AUnOp`, `ARecord`, `ARecordGet`, `ARecordUpdate`,
`AVariant`, `AArrayLit`, `AMakeClosure`.
`is_pure(op) = false` for: `ACall` (may I/O or trap), `AAssign` (mutates state),
`AIf`/`AMatch`/`ALoop` (contain arbitrary sub-expressions; treated conservatively).

**C — Dead let elimination (DLE)**
```
Let(t, pure_op, body)   where   uses[t] == 0   →   body
```
Pure lets with no uses are dropped. Repeated until stable.

**D — Literal copy propagation**
```
Let(t, AInit(atom), body)
  where  atom ∈ { ALitInt, ALitFloat, ALitBool, ALitStr, ALitVoid, AGlobalFunc }
  and    uses[t] <= 1
  →  body with every ALocal(t) replaced by atom
```
Only non-local atoms are propagated; `ALocal(u)` atoms are not, because `u` could be
reassigned between the init and the single use. Literals are always safe regardless of
intermediate mutations. After substitution the let becomes dead; DLE eliminates it next round.

**E — Constant folding**
```
Let(t, ABinOp(op, ALitInt(a),   ALitInt(b)),   body)  →  Let(t, AInit(ALitInt(eval(op,a,b))),   body)
Let(t, ABinOp(op, ALitFloat(a), ALitFloat(b)), body)  →  Let(t, AInit(ALitFloat(eval(op,a,b))), body)
Let(t, AUnOp(Not, ALitBool(b)),                body)  →  Let(t, AInit(ALitBool(!b)),             body)
Let(t, AUnOp(Neg, ALitInt(a)),                 body)  →  Let(t, AInit(ALitInt(-a)),              body)
```
Integer division / modulo by zero literals: leave as-is (runtime trap is intentional).
After folding to `AInit`, copy propagation eliminates `t` in the next round.

**F — Branch simplification**
```
Let(t, AIf { cond: ALitBool(true),  then_branch, _ }, body)
  →  splice then_branch: if it ends Atom(a), rewrite to Let(t, AInit(a), body)
Let(t, AIf { cond: ALitBool(false), _, else_branch }, body)
  →  same for else_branch
```
After splicing, copy propagation eliminates the `Let(t, AInit(a), ...)` wrapper.

**Fixed-point iteration:** Repeat: count-uses → DLE → copy-prop → constant-fold →
branch-simplify → until no change (or max 10 rounds).

**Liveness analysis:**
Backward walk computing `live(body)` = set of locals that may be read at or after each point.

```
live(Atom(ALocal(t)))          = {t}
live(Atom(_non-local_))        = {}
live(Return(Some(ALocal(t))))  = {t}
live(Return(_) | Break(_) | Continue)  = {}
live(Let(t, op, body))         = (live(body) \ {t}) ∪ locals_in_atoms(op)
```

For ops containing sub-expressions (`AIf`, `AMatch`, `ALoop`), `locals_in_atoms` includes
live sets of those sub-expressions unioned together (conservative).

**Functional-update annotation:**
For `Let(t, ARecordUpdate { base: ALocal(r), field, value }, body)`:
- If `r ∉ live(body)` → set `can_reuse_in_place = true` on the `ARecordUpdate` node.
- Meaning: the record referenced by `r` has no further observable readers; the WAT backend
  may emit `struct.set` (in-place mutation) instead of allocating a new struct.

Safety invariants preserved:
- No observable alias: `r` is dead in `body`, so no later code can observe the pre-update value.
- Evaluation order unchanged: the update expression is still evaluated; only the allocation strategy changes.
- Trap behavior unchanged: out-of-bounds struct field access traps identically either way.

**IR change:** Add `can_reuse_in_place: bool` to `AnfOp::ARecordUpdate`. Default `false`.
Set `true` only by the liveness pass.

Deliverables:

* `twk opt file.tw` prints optimized ANF IR; `--show-original` flag also prints the unoptimized form.
* `tests/opt_test.rs`:
  * ANF invariants hold on the optimized module for every `tests/run/*.tw` program.
  * Node-count reduction: a dedicated `tests/opt/constant_folding.tw` fixture with compile-time
    constants produces fewer `Let` nodes after optimization.
  * Golden snapshot tests for `tests/opt/constant_folding.tw` and `tests/opt/dead_let.tw`.
  * Liveness annotation tests: `tests/opt/record_in_place.tw` (base local dies at update → annotated
    `can_reuse_in_place = true`); `tests/opt/record_aliased.tw` (base reused after → `false`).

**Execution checklist (file/module map):**

* **Step A — Module skeleton + use counting (`src/opt/`)**
  * `src/opt/mod.rs`:
    * `pub mod use_count; pub mod passes; pub mod liveness; pub mod pipeline;`
    * Re-export `pipeline::optimize_module` for use by CLI and future WAT backend.
  * `src/opt/use_count.rs`:
    * `pub fn count_uses(body: &AnfExpr) -> HashMap<LocalId, usize>` — recursive walk;
      count `ALocal(id)` in all atom-position fields; skip `Let.local` binder and
      `AAssign.local` target.
    * `pub fn is_pure(op: &AnfOp) -> bool` — pure set as specified above.
    * `fn locals_in_op(op: &AnfOp) -> Vec<LocalId>` — private helper returning all `ALocal`
      references in operand positions of `op` (used by liveness).

* **Step B — Peephole passes (`src/opt/passes.rs`)**
  * `pub fn dead_let_elim(body: AnfExpr, uses: &HashMap<LocalId, usize>) -> (AnfExpr, bool)` —
    walk `AnfExpr`; on `Let(t, pure_op, inner)` where `uses.get(&t) == None or 0`, return
    `(inner, changed=true)`; recurse into sub-expressions of other nodes.
  * `pub fn copy_propagate(body: AnfExpr, uses: &HashMap<LocalId, usize>) -> (AnfExpr, bool)` —
    on `Let(t, AInit(lit), inner)` where `lit` is non-local and `uses[t] <= 1`, call
    `subst_atom(inner, t, lit)` and return `(result, true)`.
  * `pub fn constant_fold(body: AnfExpr) -> (AnfExpr, bool)` — on `Let(t, ABinOp/AUnOp
    with literal atoms, inner)`, compute the result literal, rewrite to `Let(t, AInit(result), inner)`.
  * `pub fn branch_simplify(body: AnfExpr) -> (AnfExpr, bool)` — on `Let(t, AIf(ALitBool(b),
    then_e, else_e), inner)`, select the known branch and splice it into `inner`.
  * `fn subst_atom(body: AnfExpr, target: LocalId, replacement: Atom) -> AnfExpr` — recursive
    substitution of `ALocal(target)` → `replacement` everywhere in `body`. Only called with
    non-local `replacement` atoms, so mutation-safety is not a concern.

* **Step C — Liveness + in-place annotation (`src/opt/liveness.rs`, `src/ir/anf.rs`)**
  * `src/ir/anf.rs`:
    * Add `can_reuse_in_place: bool` field to `AnfOp::ARecordUpdate` (default `false`).
    * Update `Display` impl for `ARecordUpdate` to show `[in-place]` when set.
  * `src/opt/liveness.rs`:
    * `pub fn live_after(body: &AnfExpr) -> HashSet<LocalId>` — backward liveness walk;
      returns the set of locals live at the *entry* of `body`.
    * `pub fn annotate_in_place(func: &mut AnfFunctionDef)` — walk the function body;
      at each `Let(t, ARecordUpdate { base: ALocal(r), .. }, inner)`, call `live_after(inner)`;
      if `r` is absent from the live set, set `can_reuse_in_place = true`.

* **Step D — Pipeline driver + CLI + tests**
  * `src/opt/pipeline.rs`:
    * `pub fn optimize_func(func: AnfFunctionDef) -> AnfFunctionDef` — fixed-point loop
      (max 10 rounds): count-uses → DLE → copy-prop → constant-fold → branch-simplify
      → repeat if changed; then call `annotate_in_place`.
    * `pub fn optimize_module(module: AnfModule) -> AnfModule` — map `optimize_func` over
      all functions.
  * `src/cli/opt.rs`:
    * `pub fn cmd_opt(path: &Path, show_original: bool) -> anyhow::Result<()>` — full pipeline
      through `lower_anf::lower_module`; optionally print original ANF; then `optimize_module`;
      print optimized ANF.
  * Wire `twk opt <file> [--show-original]` in `src/cli/mod.rs` and `src/main.rs`.
  * `tests/opt_test.rs`:
    * Invariant tests: for each `tests/run/*.tw`, lower to ANF, optimize, run
      `check_anf_invariants` — must pass (reuse the checker from `anf_test.rs`).
    * Node-count tests: `tests/opt/constant_folding.tw` fixture; count `Let` nodes before
      and after — optimized count must be strictly smaller.
    * Snapshot tests: golden ANF output for `tests/opt/constant_folding.tw` and
      `tests/opt/dead_let.tw`.
    * Liveness annotation tests: `tests/opt/record_in_place.tw` asserts at least one
      `can_reuse_in_place = true`; `tests/opt/record_aliased.tw` asserts none.

---

## Stage 7.6 — Defer ✅

**Goal:** Implement `defer` end-to-end: interpreter execution and ANF-level elimination,
leaving no `Defer` nodes for the WAT backend.

> **Full design:** See [docs/design/defer.md](../design/defer.md).

`defer expr` is a block-scoped statement that schedules an expression to run when the
enclosing block exits. Semantics: LIFO ordering, capture-by-value, triggers on normal
exit / `return` / `break` / `continue` / `try`-propagated `Err`, does **not** trigger on traps.

**Why no CFG for defer:** defer elimination is naturally a structured-scope problem. Since
ANF already encodes scope structure via nested `Let`-chains, and WAT requires structured
control flow anyway, an ANF tree-walk pass with scope-aware defer lists is sufficient and
simpler than CFG edge insertion. The tree structure *is* the scope structure.

**ANF defer elimination — scope threading:**

The elimination pass walks `AnfExpr` recursively, threading two lists:

* `fn_defers` — defers active between the current point and the enclosing function boundary;
  these run on `Return`.
* `loop_defers` — defers active within the current loop iteration; these run on `Break` and
  `Continue` (which exit only the current loop, not the function).

Rewrite rules:

```
Let(_, ADefer(d), body)        →  eliminate_defers(body, fn_defers=[..d], loop_defers=[..d])
Let(t, ALoop { body }, rest)   →  ALoop body' where body' = eliminate_defers(body,
                                       fn_defers=fn_defers++loop_defers, loop_defers=[])
                                   then eliminate_defers(rest, fn_defers, loop_defers)
Return(v)                      →  prepend (fn_defers ++ loop_defers) LIFO, then Return(v)
Break(v)                       →  prepend loop_defers LIFO, then Break(v)
Continue                       →  prepend loop_defers LIFO, then Continue
Atom(a) at end of deferred scope →  prepend own-scope defers LIFO, then Atom(a)
```

The nested-loop case works correctly: entering `ALoop` folds the current `loop_defers` into
`fn_defers` (so inner `Return` still unwinds outer defers) and resets `loop_defers` to empty
(so inner `Break`/`Continue` do not run outer loop's defers).

**Work items:**

* **Grammar & parser** — `defer` keyword and `defer expr` statement form (already in grammar
  from tree-sitter work; verify parser handles it).
* **AST** — `StmtKind::Defer(ExprId)`.
* **Type checker** — type-check the deferred expression in the current scope; result type
  is discarded. Any expression type is accepted — function calls, block expressions, etc.
  Expressions with type `Never` (i.e. those that diverge: `return`, `break`, `continue`,
  `error(...)`) are rejected at the type-check level, because a defer body that itself
  performs a non-local exit would silently swallow the surrounding control flow and is
  almost certainly a bug.
* **Core IR** — `CoreExprKind::Defer(ExprId)` as an opaque pass-through node; lowerer emits it
  directly without desugaring.
* **Interpreter** — maintain a defer stack (a `Vec<Vec<CoreExpr>>`) alongside the eval frame;
  push a new scope on block entry, drain LIFO on any `Signal` except `Trap`.
* **ANF IR** — add `AnfOp::ADefer(Box<AnfExpr>)` to preserve deferred expressions through
  linearization; `lower_anf` emits it as-is.
* **ANF elimination pass** (`src/opt/defer_elim.rs`) — `pub fn eliminate_defers(func: AnfFunctionDef)
  -> AnfFunctionDef`: tree-walk as described above; after this pass no `ADefer` ops remain;
  run as the final step in `optimize_module` (after all peephole passes).

**Deliverables:**

* `defer` works correctly in `twk run` (interpreter path).
* ANF elimination pass removes all `ADefer` nodes; WAT backend sees no `Defer` nodes.
* Tests covering:
  * Basic LIFO ordering within a block.
  * `return` unwinds all active defer scopes (function-level).
  * `break` / `continue` unwind only the current loop's defer scope.
  * Nested loops: inner `break` does not run outer loop's defers.
  * `try`-propagated `Err` triggers defers (same as return).
  * Trap does not trigger defers.
  * Capture-by-value at declaration time.
