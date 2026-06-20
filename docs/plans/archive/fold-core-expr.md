# `fold_core_expr` — a child-traversal combinator for Core IR analyses

Status: **Design** (2026-06-20)

## Problem

Several Core IR passes are hand-written structural recursions shaped like:

```tw
case expr.kind {
  .Local(id)  => ...special...,
  .Call(c, a) => ...recurse...,
  .If(...)    => ...recurse...,
  // ~15 more arms that just recurse into children
  _ => .{ ... }          // catch-all that does NOTHING
}
```

Adding a `CoreExprKind` variant is **non-breaking** — every `_ =>` still
compiles — which is exactly the hazard: a pass that *should* handle the new node
silently doesn't. This caused a real miscompile: `collect_free_vars_inner`
(`lower_core/closures.tw`) had no `ContractCall` arm, so a closure-captured
`IntoIterator` iterable (`for v in ch` over a captured channel inside
`Task.spawn`) was never recorded as a capture and degraded to a bogus
`GlobalLocal(0)` reference, tripping the backend verifier
("unknown module global"). Fixed in commit `0579db04` by adding the missing arm —
but the *class* of bug remains: every such pass is an independent place to forget.

The root cause is that these passes mix two concerns under one `case`:
**structural recursion** (descend into children — pure boilerplate) and **genuine
special cases** (capture a local, collect a reference). The `_ =>` no-op makes the
boilerplate silently fail for new variants.

## Goal & scope

Provide one generic child-traversal combinator so the "descend into all children"
knowledge lives in a **single, exhaustive, checker-guarded** place; passes keep
only their genuine special cases and delegate the rest to a **sound** default.

Scope (v1): **Core IR analysis passes** — folds that collect an accumulator and
today carry unsound no-op `_ =>` arms. Transforms (rewrites) and other IRs
(ANF, Prepared) are out of scope.

## The combinator

Home: a new module `boot/compiler/core_fold.tw` (keeps `core_ir.tw` lean).

```tw
pub fn fold_children<A>(expr: CoreExpr, acc: A, f: fn(A, CoreExpr) A) A
```

Semantics: apply `f(acc, child)` to each **immediate** sub-expression of
`expr.kind`, in source order, threading `acc`; return the final `acc`. One level
only — `f` drives deeper recursion.

- **Leaves** (`LitInt/LitFloat/LitBool/LitStr/LitVoid`, `Local`, `GlobalLocal`,
  `GlobalFunc`, `Continue`) → no children → return `acc` unchanged.
- **Compound** → fold over `CoreExpr` children in order:
  - `Let(_, value, body)` → value, body
  - `Assign(_, value)` / `GlobalSet(_, value)` → value
  - `BinOp(_, l, r)` → l, r; `UnOp(_, inner)` → inner
  - `Call(callee, args)` → callee, then each arg
  - `ContractCall(_, _, recv, args)` → recv, then each arg
  - `If(c, t, e)` → c, t, e
  - `Match(scrut, arms)` → scrut, then each `arm.body`
  - `Loop(body)` → body; `Defer(inner)` → inner
  - `Break(Some v)` / `Return(Some v)` → v; `None` forms → no child
  - `Record(_, fields)` → each `field.value`
  - `RecordGet(target, _)` → target; `RecordUpdate(base, _, value)` → base, value
  - `Variant(_, _, args)` → each arg; `ArrayLit(elems)` → each elem
  - `Index(base, idx)` → base, idx
- **`MakeClosure(_, free_vars)`** → **no `CoreExpr` children** (free_vars are
  `LocalId`s) → returns `acc`. Passes that care read `free_vars` directly.

**Linchpin:** `fold_children` contains the *only* exhaustive `case expr.kind` over
every `CoreExprKind` variant, with **no `_ =>`**. Adding a `CoreExprKind` variant breaks only
this function until its children are wired — the compiler-enforced checklist. A
header comment states: never add a `_ =>` here.

## Migration pattern (free-vars pilot)

`collect_free_vars_inner` collapses to explicit arms for the nodes it actually
reasons about, plus a sound default:

```tw
case expr.kind {
  .Local(id)            => ...record capture...,
  .GlobalLocal(_)       => state,                 // globals aren't captured
  .Assign(local, value) => ...capture local, then recurse value...,
  .Let(local, value, body)  => ...recurse value; recurse body with bound+local...,
  .Match(scrut, arms)       => ...recurse scrut; per arm recurse body with bound+patternvars...,
  .MakeClosure(_, free_vars) => ...record each free_var...,
  _ => fold_children(expr, state, fn(st, child) {
         collect_free_vars_inner(child, st.bound, st.captured, st.result)
       }),
}
```

The ~15 boilerplate "just recurse" arms (Call, If, BinOp, Record, Index, Variant,
ArrayLit, ContractCall, GlobalSet, RecordGet/Update, UnOp, Loop, Break, Return,
Defer) collapse into the one `_ =>`, which now **recurses** instead of no-op'ing.
`ContractCall` — and any future non-binder — is handled for free.

### Other candidates

After the pilot, migrate other scope-insensitive analysis folds **only if they fit
the fold shape cleanly**: DCE reachability (`core_linker/dce.tw`) and the planner
scan's collectors. `monomorphize` is mostly a transform (out of scope); migrate
only its pure collector sub-passes, if any fit. Don't force a pass that doesn't.
Each migration is verified independently.

## Binding handling (the one footgun)

`fold_children` is **binding-unaware** — it folds `Let` body and `Match` arms with
no context change. So **scope-sensitive** passes (those threading a `bound` set)
must keep explicit `Let`/`Match` arms and must *not* delegate binders to the
default. **Scope-insensitive** passes (DCE, reachability) may delegate everything.
This is documented on `fold_children` and on each scope-sensitive pass.

## Guarantees & residual risk

- A new **non-binding** variant is handled automatically by the sound `_ =>` (no
  silent miscompile), and `fold_children` won't compile until its children are
  wired (forced acknowledgement at one site).
- **Residual:** a future **binding-introducing** variant would over-capture under
  the default — a **loud, sound** failure (captures too much, not the silent
  under-capture that caused the original bug). Covered by a documented convention:
  "adding a binding-introducing variant? audit scope-sensitive passes." A far
  smaller rule than "audit every pass."

## Testing

1. **Unit tests for `fold_children`**: construct a `CoreExpr` per node kind and
   assert it visits exactly the expected children (e.g. collect child count / child
   kinds), explicitly covering `ContractCall` and the zero-child `MakeClosure`.
2. **Regression**: the channel fan-in/out test already exercises free-vars over a
   `ContractCall` inside a closure — keep it as the guard for the original bug.
3. **Net**: `make bundle-cli` self-host fixed point + full boot suite after each
   pass migration.

## Non-goals

- Transforms / rewriting combinators (`map_children`), ANF/Prepared IR traversals.
- Removing `_ =>` from passes that aren't structural folds.
- A binding-aware traversal framework (over-couples scope into the combinator).

## Related

- The original bug + fix: `lower_core/closures.tw` (commit `0579db04`), surfaced
  via channels `for v in ch` in a closure.
- Other no-op-wildcard candidates: `core_linker/dce.tw`, `monomorphize.tw`, the
  planner scan (`codegen/wasm_plan_scan.tw`).
