# Worked ANF Examples — Ownership Design Anchor

**Status:** Draft (design anchor, not a spec)

This doc grounds the sound-uniqueness analysis in **real ANF shapes** the boot
compiler actually produces, so the fact lattice, transfer rules, CFG view, and
function summaries are designed against concrete cases instead of hypotheticals.
It applies the plan's own discipline — print the facts and check they match the
reasoning — at design time.

Regenerate any dump with:

```bash
target/twk ir <file>.tw --anf
```

> Numbering note: `twk ir` prints **boot** FuncIds; the census harness
> (`tests/cow_analysis.rs`) uses **stage0** FuncIds. They are different numbering
> schemes — do not cross-map them. In boot ANF below, `Fn39` = `dict.set`,
> `Fn24` = `vector.append`, `Fn38` = `Dict.new` (observed, not memorized). All
> `FnNNN` values here are **illustrative snapshots that drift as boot changes** —
> re-run the regen command to confirm current ids; do not treat them as stable
> references.

## Op → ownership-event mapping (transfer-rule skeleton)

`AnfOp` is a closed enum, so this table is exhaustive over the events the
analysis cares about. It is the skeleton of the per-op transfer function.

| ANF op | Ownership event |
|---|---|
| `ACall` to a known constructor (`Dict.new`, `Vector.make`, builder freeze), `ARecord`, `AArrayLit`, `AVariant` | **introduce** owned |
| `AIndex`, `ARecordGet` | **borrow** (non-escaping read; does not kill ownership) |
| `ACall` to a consuming op/wrapper (`dict.set`, `vector.append`, `Vector.set`, thin wrappers) | **consume** arg, **produce** owned result |
| `ARecordUpdate(_, f, v, in_place, _)` | shell update; `in_place` is the ready decision slot |
| `AAssign(local, atom)` (rebind) | **transfer** ownership atom → local |
| `AInit(atom)` | **move** if source dead afterward, else **alias** (linearity hinge) |
| `AMakeClosure(_, captured)` | **publish** each captured owned local |
| `ARecord`/`AVariant`/`AArrayLit`/`FieldAtom` storing an owned local | **publish** (escaping aggregate) |
| `AGlobalSet` | **publish** (module/global) |
| `Return(atom?)` | **publish** at exit |
| `Break(atom?)` (value-carrying) | **publish** at loop exit |
| `AMatch` arm body ending in `Return`/`Break` (e.g. `try` error arm) | **publish** at early exit |
| `ACall` to `cell$new`/`cell$set` (store into a `Cell`) | **publish** the stored value (mutable box, aliasable/readable anytime) |
| `ACall` to `cell$update` (read-modify-write) | closure param is **Unowned** (old contents, like `get`); result is **published** (like `set`) |
| `ACall` to `cell$get` | result is **Unowned** (contents stay aliased through the live cell) |
| `ACall` to an unknown/non-summarized target | **publish** (conservative) |
| `ALoop` + `Continue` | back-edge; loop-carried facts must reconverge |

## Case A — `sieve`/`run`: loop-carried vector, update via thin wrapper

`L13` = frozen `flags` (from the `collect` builder), loop-carried through two
nested `ALoop`s.

```
let L55 = call Fn33(L5)            introduce owned (freeze collect-builder)
let L13 = init L55                 move L55 → L13
loop                              outer loop; carries L13, i, count
  let L58 = index[array] L13, L15  BORROW L13 (read flags[i]); ownership survives
  loop                            inner loop; carries L13, k
    let L64 = call Fn297(L13,L16,false)  CONSUME L13 → owned L64  (needs Fn297 summary)
    assign L13 = L64                     TRANSFER back across back-edge
    continue
L14                               return count; L13 never published
```

Wrapper `set_at__Bool` (`Fn297`): `call Fn25(v,i,x); assign v; return v` →
**summary: consumes param0, returns owned = set(param0,…)**.

- **Verdict: owned-mutable.** Borrow-only reads + consume-produce updates + no
  publication.
- **Finding:** even "intraprocedural" sieve updates *through a call*. Thin-wrapper
  summaries are on the critical path, not an interprocedural afterthought.

## Case B — `build_env`: positive record + dict, single publication

```
add_type(env):
  record_get env .types            project field (borrow)
  call Fn39(.., name, id)           consume field → owned (dict.set)
  record_update env .types = .. [in_place=false]   shell write (flippable if env owned)
  assign env; return env            summary: consumes param0, returns owned

build_env:
  L7 = call Fn38(); L8 = call Fn38()   introduce owned dicts
  L9 = record Env{types=L7,…}          introduce owned shell (L7,L8 moved in)
  L4 = init L9                          move
  L10 = call Fn295(L4,…); assign L4     consume-produce (add_type)
  L12 = call Fn295(L4,…); assign L4     consume-produce
  L4                                    publish at return (freeze once)
```

- **Verdict: owned throughout; publish only at return.** No prior version read.
  `add_type`'s `record_update` flips to in-place because `build_env` supplies the
  ownership proof — the interprocedural specialization case, made concrete.

## Case C — `branch_env`: negative, old version stays observable

```
L8 = init L7                       ALIAS: before := e (L7 and L8 name same value)
L10 = call Fn295(L7,…); L9 = init L10   after := add_type(e,…)
record_get L8 .types               OBSERVE OLD VERSION via alias L8  ← poison
… record_get L9 .types …
```

- **Verdict: persistent.** `init L7` is an **alias** because both `L7` and `L8`
  are read later; mutating `L7`'s dict in place would corrupt `L8`'s view.
- **The linearity hinge:** `AInit` is a *move* iff the source is dead afterward
  (Case B) and an *alias* otherwise (Case C). The whole negative reduces to this.

## Cases B ∩ C — one callee, two caller shapes (interprocedural specialization)

Cases B and C call the **same callee** `add_type` (`Fn295`) with opposite caller
ownership, so it is the concrete realization of the specialization scenario:

| Call site | Argument fact | Required variant |
|---|---|---|
| `build_env`: `call Fn295(L4,…)` (×2) | `L4` owned, reassigned, old never read | **mutable-specialized** (shell + `.types` in-place) |
| `branch_env`: `call Fn295(L7,…)` | `L7` aliased by `L8`, `L8.types` read later | **persistent** |

`add_type` therefore needs two codegen variants, and the choice is made
**per call site from the caller's ownership fact on the argument atom** — not from
anything inside `add_type`. This mirrors the `clear_at`/`owned_case`/`shared_case`
example in [architecture.md](architecture.md), but observed in real lowered ANF:
three call sites, two required variants.

Contrast `visit` (Case V): recursive plus called from `strongly_connected`, but
**every caller passes an owned `State`**, so it collapses to a single
owned-specialized variant with no persistent variant — and the recursion makes its
summary a fixpoint over itself. The specialization key is driven entirely by the
set of caller argument facts, which the census confirms is usually uniform (few
functions will actually need both variants).

## Record cases — shell reuse vs field ownership

`advance` (single scalar field — pure shell):

```
record_get c .pos                  read Int field (unboxed; no ownership concern)
int.add ..
record_update c .pos = .. [in_place=false]   SHELL update (flippable if c owned)
assign c; c                        publish at return
```

Only the *shell* is in question; sibling field `tokens` is untouched and safely
shared.

`push_scope` (field is an owned collection — nested):

```
record_get ctx .locals             PROJECT owned-collection field (borrow)
call Fn38()                         introduce owned Dict
call Fn24(locals, dict)             CONSUME field → owned (vector.append)
record_update ctx .locals = .. [in_place=false]   SHELL write of owned field
assign ctx; ctx                    publish
```

**Two independent decisions on one statement:** shell reuse (needs `ctx` owned)
and field-backing in-place (needs `.locals` deeply owned + no alias on the old
`.locals`). Sibling fields (`depth`, `tokens`) need not be owned — field
sensitivity matters. The inner `Dict`s already in `.locals` stay shared (we only
append) — nested shell-vs-deep in miniature.

## Case V — `graph_scc`/`visit`: the whole compiler idiom, real

`visit` threads `State` (`L7`, `cur := st`) through recursion. The **canonical
quartet** repeats per field:

```
record_get L7 .indices                            project field (borrow)
call Fn39(L43, L5, L8)                             consume → owned (dict.set)
record_update L7 .indices = L44 [in_place=false]   shell write
assign L7 = L45                                    rebind cur (L7 → L7)
```

…for `.indices`, `.lowlinks`, `.next_index`, `.stack`, `.on_stack`. It also has:

- **Index-update through a field** (`cur.indices[node] = idx`) = the same quartet
  with a `dict.set` — not a separate case.
- **Recursion** (`cur = .visit(dep,edges)` → `call Fn296(L7); assign L7`) ⇒
  visit's summary is **self-referential**; summaries need an SCC-level fixpoint.
- **Branch/match-join of an owned record**: `if … { record_update .lowlinks;
  assign L7 } else { }` — one arm mutates `L7`, the other doesn't; the join must
  merge *mutated* and *untouched* to *still owned*. This is the dominant join.
- **Loop-carried owned record** across `continue` back-edges (the `for dep` and
  `for !done` loops).
- **Nested collection field**: `cur.components = .append(component)` writes a
  locally-built `Vector<String>` into a `Vector<Vector>` field.
- **Publication** only at `return cur`.

`visit` alone exercises shell reuse, field ownership, nested collections,
match-join, loop-carried ownership, recursion/self-summary, and dict/vector
field updates — the real shape the analysis must handle, not a toy.

## Case T — `try`/`chain`: early-return publication

```
L8 = call Fn295(a)
match L8
  Ok(L2)  => L2                    fallthrough: region continues
  Err(L3) => variant Err(L3); return   EARLY EXIT: publish on error arm
L4 = init L10
… second try, same shape …
```

`try` is an `AMatch` whose error arm ends in `Return`. It is a **multi-exit
publication point** (structurally like value-carrying `break`), derived from the
match arm structure — no new node. An owned handle spanning a `try` gains an
extra publication/exit edge on the error arm; the fallthrough (`Ok`) arm keeps
the region alive.

## Case Cell — `mark`: mutable collection through a `Cell` (the un-optimizable corner)

`unused_imports.mark` accumulates used names into a `Cell<Dict>` threaded (by
reference) through the entire recursive walk; `defer_elim.register_snapshot_types`
has the identical shape over `Cell<Dict<Int, MonoType>>`. Source is
`u := used.get(); u[name] = true; used.set(u)` — the real lowered ANF
(`Fn55` = `cell$get`, `Fn56` = `cell$set`, `Fn39` = `dict.set`):

```
let L99 = call Fn55(L95)             cell$get → UNOWNED (contents of the live cell)
let L97 = init L99                   name the borrowed contents
let L100 = call Fn39(L97,L96,true)   dict.set on an UNOWNED dict ⇒ forced COW
assign L97 = L100                    rebind
let L102 = call Fn56(L95, L97)       cell$set → PUBLISH (store back into the cell)
```

- **Verdict: persistent, and correctly so.** `cell$get` yields Unowned because the
  Cell is a live alias of that same dict at `cell$set` time; mutating it in place
  could corrupt any other live `get` of the same Cell. This `dict.set` can **never**
  join the census's 267 dict-in-place sites.
- **This is the tightest sound rule, not timidity.** Beating it needs either a
  runtime refcount (the RC-clan `refcount==1` trick — off the menu on Wasm GC) or a
  static escape/liveness proof over the Cell's contents (full alias analysis — the
  exact hazard the `Cell` quarantine buys us out of; immutability gives no leverage
  here, because the Cell *is* the one mutable location). Both are foreclosed by
  prior design choices, so store-publishes / read-yields-Unowned is the *minimal*
  sound treatment. `Cell` is not an optimization target by design.
- **The cost is ~2 self-inflicted sites, already discouraged.** Only `used` and
  `snap_types` route a mutable *collection* through a Cell; every other boot Cell
  holds an unboxed scalar (nothing to optimize) or a wholesale-rebuilt record (no
  prior version reused). The idiomatic alternative — thread the Dict through return
  values (Case B/V) — *is* fully optimizable, which is exactly what the "avoid Cell,
  thread state through returns" guidance steers toward. This case is that guidance's
  cost, made concrete.
- **Containment:** publication hits the Cell *contents* only. A Cell-typed record
  field (`FileRegistry.data: Cell<RegistryData>`) does not lose shell reuse of the
  enclosing record — Cell-ness does not leak outward to the aggregate holding it.

## Census baseline (stage0 over `boot/main.tw`)

`cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture`
runs **stage0** (whose uniqueness optimizer is intact) over the real boot
compiler, giving the *achievable* in-place distribution:

| Op | Pre-opt (all COW) | COW remaining | In-place/builder |
|---|---|---|---|
| Record update | 515 | 344 | **171 in-place** |
| DICT_SET | 533 | 266 | **267 in-place** |
| VECTOR_SET (index) | 5 | 5 | 13 in-place |
| DICT_REMOVE | 19 | 19 | 0 |
| Vector builders | — | 1191 append | 706 new / 915 push / 962 freeze / 298 extend |

- **Record + dict in-place are the compiler wins (171 + 267 sites); vector
  index-mutation is negligible (13).** Vector `set_at` in-place is an AWFY
  (sieve/bounce) concern — a different population from the compiler.
- The new boot analysis, once built, should reproduce roughly this distribution.
- **Gate reconciled 2026-07-12** (was red: 1696 ceiling set 2026-06-03 vs current
  total). Two facts surfaced fixing it:
  - the total is **non-deterministic** — stage0's optimizer makes a run-to-run
    varying number of in-place conversions (observed ~1962–1970), so the ceiling
    is coarse (headroom to 2000) and can only catch large regressions. This is the
    determinism risk the plan flags, observed in the existing optimizer.
  - the total is **absolute**, so it also grows with boot source size; re-baseline
    upward for legitimate growth, downward only for genuine optimizer gains.
- **Scope caveat:** this census measures **stage0** compiling boot, not the new
  boot analysis (which runs in the boot pipeline). It is a *reference for the
  achievable distribution*, not a regression gate for this project's work — the
  new analysis will need its own, deterministic, boot-side census.

## Cross-cutting design findings

1. **Analysis runs over desugared ANF.** `collect`/`for-in` are gone; loops are
   `ALoop` + `assign` + `break`/`continue`; ranges are `record_get`. Rules target
   lowered ops.
2. **The universal update shape is `let Ln = <consuming op>; assign Lc = Ln`**,
   with ownership `Lc → Ln → Lc`. Borrow reads (`AIndex`/`ARecordGet`) are
   distinguishable from consuming updates by op kind alone.
3. **`AInit` is the linearity hinge** (move vs alias by source liveness). Every
   negative in these cases reduces to it.
4. **The record quartet** (`record_get` → consuming call → `record_update` →
   `assign`) is the dominant compiler idiom and carries **two independent
   in-place decisions** (field backing on the call, shell on the `record_update`).
   The 171 record + 267 dict in-place census sites are these quartets.
5. **Branch/match-join of an owned record is pervasive** (census: 2898 `case`
   sites) — the join merge (mutated-on-one-arm vs untouched) is table stakes.
6. **Thin-wrapper and recursive summaries are on the critical path.** Sieve
   (`set_at`), record threading (`add_type`), and `visit` (self-recursive) all
   update through calls. Minimal summary = per-parameter consumed/borrowed/
   returned + return ownership; recursion needs an SCC fixpoint on summaries.
7. **Multi-exit publication** unifies `Return`, value-carrying `Break`, and `try`
   error arms — all are exit edges derived structurally, all publish the exiting
   value.

## Reprioritization for the plan

- **Record-threading first, not raw dict.** Source census: 246 record rebinds vs
  43 dict updates; stage0 census: 171 record + 267 dict in-place. The compiler
  win is owned record shells whose fields are dicts/vectors (Case B/V), reached
  through the quartet. Phase 2C's "dict-heavy" framing should read
  "record-threading-heavy with dict/vector-valued fields."
- **Match-arm joins are first-class**, not an edge case — the CFG view's join
  merge must be right early.
- **`try`/early-return is a publication sink** and should be listed alongside
  return/break in the architecture doc's publication boundaries.
- **Reconcile the census ceiling** before adopting it as the baseline gate.
