# Physical Representation Ownership — Master Design

**Status:** Design approved 2026-08-03. The doctrine below is the durable artifact;
the only implementor is the typed-vector sub-plan
([physical-repr-planner-refactor.md](physical-repr-planner-refactor.md)).

The goal is clean SoC across the three layers `MonoType → ReprKind → ValType`, with
**no compromise**: one physical-repr vocabulary (`ReprKind`, whose `TypedVec(ElemRepr)`
names a typed vector), `ValType` a pure function of it, and the standing invariant
`wasm_type == wasm_type_of_repr(repr, mono)` at every slot. `PhysPlan` is the
concrete (non-generic) record of the per-site repr *overrides* for func-id-keyed
sites; it carries the family as an `ElemRepr` tag. Typed vectors span four families
(`I64` / `I32` / `F64` / `Byte` → `PVecI64` / `PVecBool` / `PVecF64` / `PVecByte`).

## Problem

"MonoType / repr / wasm-type separation of concerns" is the real subject. The three
layers should be a pure pipeline; today they are not, and fixing that — not merely
tidying ownership — is the endeavor.

The three layers are `MonoType` (semantic) → `ReprKind` (physical representation) →
`ValType` (concrete Wasm type). `SlotInfo` (`prepared_ir.tw:74`) carries all three,
`{ mono, repr, wasm_type }`, for every slot. Good SoC means each is a pure function
of the last: **`wasm_type == wasm_type_of_repr(repr, mono)` for every slot, always**
(`wasm_type_of_repr` actually threads `env` too — `(repr, mono)` is the shorthand used
throughout this doc; see the `ValType` doctrine rule for the real signature).

**Layer 1 — the default `MonoType → ReprKind → ValType` — is *mostly* clean but has
a live leak.** `repr_assign.tw` maps each `MonoType` to a `ReprKind` (`repr_of_mono`,
`repr_assign.tw:229`) and each `ReprKind` to a `ValType` (`wasm_type_of_repr`). For
records, dicts, strings, closures this is a clean pure function. But the vector case
is broken: `ReprKind` already has a `TypedVec(ElemRepr)` variant meant to name a
typed vector, yet it is **inert** — `wasm_type_of_repr(.TypedVec(_))` ignores the
`ElemRepr` and routes through `val_type_of_mono(mono)` (`repr_assign.tw:348`, and its
twin `verify_common.tw:27`), which yields the *boxed* `PVec`. So the repr layer cannot
express "typed vector of family X" as a `ValType`.

**Layer 2 — per-*site* physical specialization for vectors — routes *around* Layer
1, breaking the invariant.** The optimization "*this* site uses an unboxed typed PVec
(`PVecI64` / `PVecBool` / `PVecF64` / `PVecByte`) or a transient `MutVec*` handle
instead of the boxed `PVec`" is applied by `route_typed_vec.tw` +
`mutvec_repr.tw` — but because `TypedVec` is inert, route **keeps `repr` at its boxed
default and overwrites only `wasm_type`** (`with_repr_wasm(info, info.repr,
.Ref(…PVecI64))`, `route_typed_vec.tw:627`). After routing a typed slot, `repr` says
boxed and `wasm_type` says `PVecI64` — **the invariant is broken at exactly the typed
sites.** That is the real mess: not just scattered ownership, but a middle layer
(`repr`) that downstream cannot trust, forcing `emit` to re-derive typedness as a
safety net and the verifier to work on raw `ValType`. A stale frozen-slot override
already showed how late `wasm_type` mutation can clobber an earlier decision.

The fix restores the invariant: make `TypedVec(ElemRepr)` **live** (family-aware
`wasm_type_of_repr`), have route set `repr = TypedVec(elem)` and *derive* `wasm_type`
from it, and record the per-site decision in one queryable place. Then the three
layers are a pure pipeline again and nothing downstream re-derives anything.

## Why this is not "one refactor covering vector, dict, record"

The thing being refactored — Layer 2 — **only exists for vectors**:

- **Records** already map to a nominal GC `struct` with per-field physical types
  (an `Int` field is already `i64` in the struct). There is no boxed-vs-unboxed
  *choice per site* to own — no analog of `PVecI64` waiting to be unlocked.
- **Dict** is a HAMT of anyref slots. A typed/unboxed dict was investigated and
  **rejected** (dict cost is allocation / insertion-order-bound, not key-handling).

Generalizing the plan machinery to all three now would be building an interface
against **one real implementor and zero other customers**, one of which is already
ruled out. That is the over-abstraction to avoid. What *is* general is the
**decision-ownership doctrine** and a thin substrate of provably type-agnostic
parts; vectors are its first exerciser.

## Decomposition

- **Master (this endeavor):** the ownership **doctrine** — the durable artifact that
  governs all present and future physical-repr work. It owns no code of its own; its
  invariants are realized by the sub-plan.
- **Implementation — typed vectors:**
  [physical-repr-planner-refactor.md](physical-repr-planner-refactor.md). Builds the
  **concrete** vector `PhysPlan`, the type-agnostic verify edge-skeleton, and the
  `prepare` staging order — instantiating the doctrine. It is the only customer
  today. Stays at its current location; this master doc links to it, and the
  vector-performance endeavor
  ([performance/vector/README.md](performance/vector/README.md)) links to it too.
- **Parked extension points — dict, record:** documented below as "plug in here when
  a customer exists," explicitly **not built** and **not pre-generalized**.
  Re-derive a generic `PhysPlan<R>` the day a second real customer lands — not
  before.

## The ownership doctrine (the durable artifact)

The three layers are a pure pipeline. Each rule below keeps it that way.

- **`MonoType` is semantic.** It never proves a storage site is physically
  specialized. (It *is* still read to pick a family from a concrete element type —
  that is structural derivation, not a specialization decision, and is permitted, in
  `repr_assign` / `wasm_layout` only.)
- **`ReprKind` is the single physical-representation vocabulary.** A typed vector is
  `TypedVec(ElemRepr)`; the boxed default is `TypedRef(mono)`; a transient handle is
  `MutVec(ElemRepr)`. There is **no second repr enum** — the plan, route, verify, and
  emit all speak `ReprKind` (and its `ElemRepr` family tag). `TypedVec` is **live**,
  not inert.
- **`ValType` is a pure function of `ReprKind`.** `wasm_type_of_repr(repr, mono, env)`
  (its real signature — `repr_assign.tw:324`; `env` resolves nominal struct types, and
  is elided as `wasm_type_of_repr(repr, mono)` in the invariant shorthand) is the
  *only* producer of a slot's `wasm_type`, and `wasm_type_of_repr(.TypedVec(elem), …)`
  yields that family's PVec. The standing invariant, checkable at any point:
  **`SlotInfo.wasm_type == wasm_type_of_repr(SlotInfo.repr, SlotInfo.mono, env)`.**
  Nobody writes a slot's `wasm_type` independently of its `repr`. The **one**
  `ValType`-without-`ReprKind` site is `PreparedFunc.phys_return` (the return-ABI
  override): it has no companion slot `repr`, is *derived* from the plan's `returns`
  `ElemRepr` via `pvec_wasm_type(elem)`, and is checked by the coercing-edge verify —
  not the per-slot invariant.
- **`PhysPlan` records the per-*function* repr *overrides*** — the func-id-keyed ABI
  sites (function returns, closure captures), each single-family (a function is
  monomorphized to a distinct id per instantiation). An entry names the override as
  an `ElemRepr` (absent = the boxed default). It is the queryable decision record,
  not a second decision engine. Local-slot overrides are **not** in `PhysPlan` — they
  live authoritatively on `SlotInfo.repr` itself (route sets `repr := TypedVec(elem)`
  and derives `wasm_type` from it in place); a slot side-table (`PhysPlan.slots`) was
  built, then dropped as redundant once route wrote straight to `SlotInfo.repr` — see
  the note in `phys_plan.tw` below.
- **`route` decides eligibility (its own analysis), records it in the plan, and
  *applies* it as a pure recompute:** `repr := TypedVec(elem)`, then
  `wasm_type := wasm_type_of_repr(repr, mono)`. It never overwrites `wasm_type`
  independently — so the invariant holds by construction and there is no stale-slot
  clobber to fear. **`mutvec_repr`** sets `repr := MutVec(elem)` for transient handle
  slots the same way (derive `wasm_type` from `repr`); handles never reach a durable
  ABI edge, so they never enter the plan. The two ownerships do not overlap.
- **Field/payload physical family is Layer-1 structural.** A record/variant type is
  not monomorphized (one `TypeId`, `Var("T")` fields), so a `Vector<T>` field's family
  is a function of the concrete instantiation `(TypeId, type args)`, which
  `wasm_layout` derives per-instantiation. A `(TypeId, field)` key cannot name it, so
  the plan does not carry field/payload family — only their *eligibility* (the
  existing presence gate). `wasm_layout` still owns *which* family a field is.
- **`verify` asserts the invariant** (`wasm_type == wasm_type_of_repr(repr, mono)`)
  per slot, plus per-edge-class coercion checks on `ValType` edges (see below).
- **`emit` reads `repr` / `wasm_type`** (guaranteed consistent) and inserts coercions
  only at declared boundaries. Because `repr` is trustworthy, emit has **nothing to
  re-derive** — the old `MonoType`-driven typedness safety net is removed, not merely
  audited.
- Any future per-site specialization, for any type, adds a `ReprKind` variant and
  plugs into this substrate — it does **not** add a new bespoke route/patch pass.

## Substrate (built by the vector sub-plan)

Concrete, not generic. One real customer, dict rejected, record has no alternate
form — so the substrate is a plain vector `PhysPlan`. The durable value is the
doctrine, which needs zero generics. Re-generalize to `PhysPlan<R>` only when a
second customer lands; doing it now buys nothing and forces `R?`-accessor +
per-customer-default ceremony in a traitless language for no payoff.

### The repr layer: revive `TypedVec`, add `pvec_wasm_type`
The substrate's foundation is making `ReprKind.TypedVec(ElemRepr)` a live physical
repr (`ElemRepr = { I64, F64, I32, Byte }`, `repr_policy.tw`). Add the family →
`ValType` map — the exact mirror of the existing `mutvec_wasm_type`, placed beside it
in `codegen/wasm_layout.tw` (that is where `mutvec_wasm_type` lives — *not*
`repr_policy.tw`):
```
pub fn pvec_wasm_type(elem: ElemRepr) ValType {
  name := case elem {
    .I64  => "rt_types__PVecI64",
    .I32  => "rt_types__PVecBool",
    .F64  => "rt_types__PVecF64",
    .Byte => "rt_types__PVecByte",
  }
  .Ref(true, .Named(name))
}
```
and point `wasm_type_of_repr(.TypedVec(elem))` (and the twin in `verify_common.tw`) at
it instead of `val_type_of_mono(mono)`. That single change makes the invariant
`wasm_type == wasm_type_of_repr(repr, mono)` *expressible* for typed vectors; route
setting `repr := TypedVec(elem)` makes it *hold*.

### `phys_plan.tw` — concrete vector plan container
The plan records a per-*function* repr **override**. The only override is boxed →
typed vector, whose payload is the family `ElemRepr`; an absent entry means "no
override → the `MonoType` default". So the plan carries `ElemRepr`, not a second
repr enum:
```
// Field names differ from the accessor names below on purpose: a `pub fn
// return_repr(plan: PhysPlan, ...)` whose first param is PhysPlan auto-registers as
// an inherent method, and a same-named field would be a FieldMethodCollision.
pub type PhysPlan = .{
  returns:  Dict<String, ElemRepr>,  // "${func}"          -> typed family override
  captures: Dict<String, ElemRepr>,  // "${func}:${index}" -> typed family override
}
```
**Superseded: no `slots` side-table.** An earlier iteration also carried
`slots: Dict<String, ElemRepr>` keyed by `"${func}:${slot}"`. It was dropped — local
slots don't need a func-id-keyed lookup table because `route` writes the decision
straight onto the slot it already holds: `repr := TypedVec(elem)`, `wasm_type`
derived from it, both on `SlotInfo` in place. The side-table was pure redundancy
once that direct write existed, so it was removed rather than kept in sync. Only
`returns` and `captures` are true out-of-band ABI facts — a function's return value
and a closure's captures aren't `SlotInfo`s the function body owns, so they need
their own queryable record; a local slot already has one (itself).
- **Only func-id-keyed ABI sites.** Returns and captures belong to a monomorphized
  function id, so each has one concrete family — the plan records it authoritatively.
  Record fields and variant payloads are **not** in the plan: their family is
  per-instantiation and owned by `wasm_layout` (see doctrine). Their *eligibility*
  stays in the existing `typed_fields` / `typed_payloads` presence maps, which
  `route` keeps consuming.
- Point-query accessors return `ElemRepr?`: `.Some(elem)` is a typed override,
  `.None` is the boxed default. The site's `ReprKind` is `TypedVec(elem)` and its
  `ValType` is `pvec_wasm_type(elem)` — never hardcoded to `PVecI64`.
- The `abi_key(func,i)` site-key helper lives here for now. It is the one genuinely
  type-agnostic piece; extract it to a shared module the day a second customer needs
  it, not speculatively.
- `MutVec*` handles are `ReprKind.MutVec(elem)` reprs owned by `mutvec_repr`, never a
  plan override (see doctrine).
- `set_*` helpers record typed capture/return sites; `empty()` constructs the
  all-empty plan.

### Verify — the invariant first, then edge classes
The primary check is structural and cheap: for every slot,
**`wasm_type == wasm_type_of_repr(repr, mono)`**. Once route derives `wasm_type` from
`repr`, this can only fail if some pass wrote one without the other — so it catches
the entire class of "stale slot / clobbered override" bugs directly, and it is
type-agnostic (no vector knowledge). The edge-skeleton below is the second line: it
checks producer/destination `ValType`s across copy/call boundaries.

The shared edge helper operates on **physical `ValType` edges plus a coercion
predicate capability**, never on a repr tag. **Whether a `boxed PVec ↔ typed PVec`
pair is a legal coercion depends on the edge class**, so the capability takes the
edge class, not just the two `ValType`s:
```
// edge class: does the emitter insert a coercion here?
type EdgeClass = { Coercing, NonCoercing }
// capability record
.{ is_declared_coercion: fn(from: ValType, to: ValType, edge: EdgeClass) Bool }
```
- **Coercing edges** — call args, function returns, variant construct/extract: the
  emitter inserts that family's `box`/`unbox` (`route_typed_vec.tw` records that
  variant construction and extraction both coerce; call/return go through
  `emit_coerce_stack`). A boxed↔typed-family pair here is a **declared coercion**.
- **Non-coercing edges** — record-field read/store, closure-capture store, local
  copy/assign: the emitter inserts **nothing** (fields carry no coercion). A
  boxed↔typed mismatch here is a **real defect**, exactly what the existing
  `verify_expr.tw` `pvec_vt_mismatch` check rejects.
- **Cross-family typed↔typed** (`PVecI64` vs `PVecF64`) is **never** a declared
  coercion, on any edge class.

At an edge the helper compares producer `ValType` to destination `ValType`: equal is
accepted; otherwise accepted only if `is_declared_coercion(from, to, edge)` holds;
else it reports a physical-repr mismatch. Reuse the existing per-edge split — the
non-coercing sites already error via `pvec_vt_mismatch`, the coercing sites already
gate on `can_emit_coerce_stack` — rather than collapsing both into one `(from,to)`
predicate. The vector classifier enumerates every family PVec (derive it from
`family_by_pvec_name` in `elem_family.tw`, not hardcoded `PVec`/`PVecI64`). It works
purely on `ValType`, so a future customer reuses it with its own predicate — the one
substrate piece reusable without genericity.

### Staging pattern in `prepare.tw`
Canonical, documented order:
`analysis (facts) → materialize PhysPlan → apply (route: set repr, derive wasm_type) → verify (invariant + edges) → emit (coerce-only, no re-derivation)`.

## Parked extension points (no code)

- **Dict.** A future unboxed/typed-dict customer adds its own `ReprKind` variant(s)
  (e.g. `TypedDict(...)`), a `wasm_type_of_repr` arm for it, a dict eligibility
  analysis, and a dict coercion predicate for the verify skeleton (already
  `ValType`-only, needs no change). Not built — prior investigation rejected typed
  dict on cost grounds; revisit only with a measured win.
- **Record.** No alternate physical form exists today (fields are already typed in
  the struct). A future packed / specialized-layout customer would add a `ReprKind`
  variant the same way. Not built.

## Files

All code is created/modified by the **vector sub-plan**; this master doc ships only
the doctrine.

**Create:**
- `boot/compiler/backend/phys_plan.tw` — concrete `PhysPlan` (returns / captures
  over `ElemRepr`) + site-key helper + accessors (default `.None`) + `set_*` +
  `empty`.
- `boot/tests/suites/phys_plan_suite.tw` — container + site-key + invariant +
  edge-skeleton tests.
- shared verify edge-skeleton helper in `verify_common.tw` (added, not a rewrite).

**Modify (the repr-layer fix that makes SoC possible):**
- `boot/compiler/codegen/wasm_layout.tw` — add `pvec_wasm_type(elem: ElemRepr)` beside its twin `mutvec_wasm_type`.
- `boot/compiler/backend/repr_assign.tw` (`.TypedVec` arm at `repr_assign.tw:348`) + `verify_common.tw` (twin at `verify_common.tw:27`) — point `wasm_type_of_repr(.TypedVec(elem), …)` (and its twin) at `pvec_wasm_type(elem)`, retiring the inert `val_type_of_mono(mono)` route.
- `boot/compiler/backend/prepare.tw` — document + wire the canonical staging order.
- `docs/plans/README.md`, `docs/plans/performance/vector/README.md` — link this
  umbrella doctrine and the sub-plan.

**Explicitly out of the plan's authority (leave as-is):**
- `boot/compiler/codegen/wasm_layout.tw`'s **existing per-instantiation field/payload
  derivation** — it derives each record field / variant payload physical family
  per-instantiation from the concrete substituted element type. This is Layer-1
  structural derivation on a concrete type, *not* a re-derivation the emit-audit
  removes. A future worker must not "fix" it into a `PhysPlan` read — a `(TypeId,
  field)` key cannot name a per-instantiation family. (This is orthogonal to *adding*
  `pvec_wasm_type` to the same file, which the plan does do — adding a new helper is
  fine; rewriting the field/payload derivation is not.)

## Handoff to the implementation plan

Constraints the impl-plan agent must carry (these gate the sub-plan):

1. **One repr vocabulary; the invariant holds by construction.** Revive
   `ReprKind.TypedVec(ElemRepr)` (add `pvec_wasm_type`, fix `wasm_type_of_repr`); do
   **not** introduce a second repr enum. The `PhysPlan` carries `ElemRepr` overrides
   over `returns` / `captures` — func-id-keyed only, all four families (`I64` / `I32`
   / `F64` / `Byte`). Local-slot overrides are not plan-keyed; route writes them
   straight onto `SlotInfo.repr`. Route sets `repr = TypedVec(elem)` and derives
   `wasm_type`; it never writes `wasm_type` independently. Field/payload family is
   **not** the plan's to own — it is `wasm_layout`'s per-instantiation derivation.
   Dict/record stay parked as docs only.
2. **Preserve the tactical invariant byte-for-byte.** `route` recognizes
   `vector$__mutvec_freeze_i64` as a typed producer; `mutvec_repr` does not override
   freeze-result slots. Self-host fixed point is the gate on **every** task.
3. **Sequence the emit "stop re-deriving typedness" removal last**, after the
   invariant (`wasm_type == wasm_type_of_repr(repr, mono)`) and the edge-skeleton are
   green. Once `repr` is trustworthy the re-derivation is dead code, but it is the
   only step that deletes a safety net, so it lands last and carries the real risk.
4. **Baseline the sub-timings first.** Record `analyze_typed_repr` /
   `route_typed_vectors` numbers before Task 1 (every `PhysPlan` lookup is a
   `Dict<String,_>` string-key alloc); the perf guard diffs against this baseline.

## Non-goals

- Changing the **default** `MonoType → ReprKind` assignments, or the boxed default for
  vectors. Making `TypedVec → ValType` live (the inert-leak fix) *is* in scope — it is
  what the invariant needs — but nothing else in Layer 1 moves.
- A universal "accept all collection types" injection interface. The repr *set* is
  per-customer by design (each customer adds a `ReprKind` variant).
- A generic `PhysPlan<R>` container. Concrete-only until a second real customer lands.
- Building dict or record specialization. Parked until a measured customer exists.
- Making `Vector<Int>` (or any type) globally specialized. Overrides stay
  per-storage-site / per-ABI-edge.
- Replacing emit coercions with an explicit prepared-IR coercion op.
