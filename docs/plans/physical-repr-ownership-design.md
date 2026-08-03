# Physical Representation Ownership — Master Design

**Status:** Design approved 2026-08-03, with amendment: the substrate is **concrete**
(a vector `PhysPlan`), not generic `PhysPlan<R>`. The doctrine below is the durable
artifact; the only implementor is the typed-vector sub-plan
([physical-repr-planner-refactor.md](physical-repr-planner-refactor.md)).

## Problem

"MonoType / repr / wasm-type separation of concerns" names **two** distinct layers,
and only one is messy. Conflating them is what makes a "general refactor" look
bigger than it is.

**Layer 1 — semantic type → default physical representation. Already unified.**
`repr_assign.tw` is the single place mapping every `MonoType` to a `ReprKind`
(`repr_of_mono`, `repr_assign.tw:229`) and then to a Wasm `ValType`
(`wasm_type_of_repr`). `SlotInfo` (`prepared_ir.tw:74`) carries
`{ mono, repr, wasm_type }` for every slot. This covers vector, dict, record,
string, closure — all of it — and is a pure function of `MonoType`, so it is
site-independent. It is **clean and out of scope**; this endeavor does not touch it.

**Layer 2 — per-*site* physical specialization. Exists only for vectors, and that
is the mess.** The optimization "*this particular* storage site / ABI edge uses a
non-default physical representation" (unboxed `PVecI64` / `MutVecI64` instead of
boxed `PVec`) is bolted on *after* Layer 1 by `route_typed_vec.tw` +
`mutvec_repr.tw`, which **override** `SlotInfo.repr`/`wasm_type`. That override —
whole-program eligibility analysis in one file, application in another, late
patching in a third, plus emit re-deriving typedness as a safety net — has no
explicit ownership boundary. A stale frozen-slot override already demonstrated how
late repr mutation can clobber an earlier decision.

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

Type-agnostic contract every current and future repr customer obeys:

- `MonoType` is **semantic**. It never proves a storage site is physically
  specialized.
- `repr_assign` owns the **default** `MonoType → ReprKind → ValType`. Unchanged by
  this endeavor.
- A **materialized `PhysPlan`** is the single source of truth for every
  *site-dependent override*: which slot / field / payload / return / capture uses a
  non-default physical representation.
- **`route` is the only mutator of plan-owned (durable) reprs** — prepared slot
  `repr`/`wasm_type` for durable storage sites and ABI edges. It *decides*
  eligibility (its own analysis) and *records* the decision into the plan; the plan
  is the queryable, verifiable record, not a second decision engine. **`mutvec_repr`
  owns transient `MutVecI64` handle reprs**, which are **not** `PhysPlan` variants:
  a handle never reaches a durable ABI edge, so it never enters the plan. The
  doctrine reads "only mutator of *durable* reprs," not "only mutator" — the two
  ownerships do not overlap.
- **`verify` checks every physical edge** against actual Wasm `ValType`s before Wasm
  validation.
- **`emit` inserts coercions only at declared representation boundaries** and never
  re-derives typedness from `MonoType`.
- Any future per-site specialization, for any type, plugs into this substrate — it
  does **not** add a new bespoke route/patch pass.

## Substrate (built by the vector sub-plan)

Concrete, not generic. One real customer, dict rejected, record has no alternate
form — so the substrate is a plain vector `PhysPlan`. The durable value is the
doctrine, which needs zero generics. Re-generalize to `PhysPlan<R>` only when a
second customer lands; doing it now buys nothing and forces `R?`-accessor +
per-customer-default ceremony in a traitless language for no payoff.

### `phys_plan.tw` — concrete vector plan container
```
pub type PhysRepr = { Boxed, TypedI64 }

pub type PhysPlan = .{
  slot_repr:    Dict<String, PhysRepr>,
  field_repr:   Dict<String, PhysRepr>,
  payload_repr: Dict<String, PhysRepr>,
  return_repr:  Dict<String, PhysRepr>,
  capture_repr: Dict<String, PhysRepr>,
}
```
- Point-query accessors return `PhysRepr` with the **default baked in**: an absent
  entry is `.Boxed`. No `Option` ceremony, no per-customer wrapper — that ceremony
  existed only to keep a generic `R` honest, and it is gone with the generic.
- Site-key helpers (`slot_key(func,slot)` / `field_key(type,field)` /
  `payload_key(type,variant,i)` / `abi_key(func,i)`) live here for now. They are the
  one genuinely type-agnostic piece; extract them to a shared module the day a
  second customer needs them, not speculatively.
- `MutVecI64` is **not** a `PhysRepr` variant — it is a `mutvec_repr`-owned transient
  handle repr (see doctrine).
- `set_*` helpers record typed sites; `empty()` constructs the all-empty plan.

### Verify edge-skeleton — on `ValType` + a coercion capability
The shared verifier helper operates on **physical `ValType` edges plus a coercion
predicate capability**, never on a semantic repr tag:
```
// capability record
.{ is_declared_coercion: fn(from: ValType, to: ValType) Bool }
```
At a copy / assign / result edge it compares producer `ValType` to destination
`ValType`: equal is accepted; a mismatch is accepted only if
`is_declared_coercion(from, to)` holds; otherwise it reports a physical-repr
mismatch. This keeps the existing `verify_expr.tw` intact — we **add** a helper
rather than rework the verifier — and is already type-agnostic (it never mentions
`PhysRepr`), so a future customer reuses it by supplying its own coercion predicate
(vectors declare `PVec ↔ PVecI64`; `MutVecI64` edges are produced by the freeze
producer). This is the one substrate piece that stays reusable without any
genericity.

### Staging pattern in `prepare.tw`
Canonical, documented order:
`analysis (facts) → materialize PhysPlan → apply (route) → verify edges → emit (coerce-only)`.

## Vector sub-plan alignment

The reviewed vector plan is already concrete (its Task 1 defines
`PhysRepr = { Boxed, TypedI64 }` and a concrete `PhysPlan`), so this amendment
*removes* churn rather than adding it — there is no generic rebase to do. Two seams
still align to the doctrine when the plan is finalized:
- Its verifier Task adopts the `ValType` + `is_declared_coercion` edge-skeleton above
  (instead of an ad-hoc classifier), so the skeleton lands reusable.
- The emit "stop re-deriving typedness" change is sequenced **last**, gated on the
  edge-skeleton being proven complete — it is the only step that removes a safety
  net (see Handoff below).
Everything else in that plan stands, behavior-preserving and per-task.

## Parked extension points (no code)

- **Dict.** A future unboxed/typed-dict customer would define its own repr set
  (e.g. `{ Boxed, TypedIntKeys, ... }`), reuse the site-key helpers (extracted to a
  shared module at that point), add a dict eligibility analysis, and supply a dict
  coercion predicate to the verify skeleton — which is already type-agnostic and
  needs no change. Landing this second customer is also the trigger to generalize
  the container to `PhysPlan<R>`. Not built — prior investigation rejected typed
  dict on cost grounds; revisit only with a measured win.
- **Record.** No alternate physical form exists today (fields are already typed in
  the struct). A future packed / specialized-layout customer would plug in the same
  way. Not built.

## Files

All code is created/modified by the **vector sub-plan**; this master doc ships only
the doctrine.

**Create:**
- `boot/compiler/backend/phys_plan.tw` — concrete `PhysPlan` + `PhysRepr` + site-key
  helpers + accessors (default `.Boxed`) + `set_*` + `empty`.
- `boot/tests/suites/phys_plan_suite.tw` — concrete container + site-key +
  edge-skeleton tests.
- shared verify edge-skeleton helper in `verify_common.tw` (added, not a rewrite).

**Modify:**
- `boot/compiler/backend/prepare.tw` — document + wire the canonical staging order.
- `docs/plans/README.md`, `docs/plans/performance/vector/README.md` — link this
  umbrella doctrine and the sub-plan.

## Handoff to the implementation plan

Constraints the impl-plan agent must carry (these gate the sub-plan):

1. **Concrete, not generic.** Build the vector `PhysPlan` (`PhysRepr = { Boxed,
   TypedI64 }`) above — no `PhysPlan<R>` container and no generic test suite.
   Dict/record stay parked as docs only.
2. **Preserve the tactical invariant byte-for-byte.** `route` recognizes
   `vector$__mutvec_freeze_i64` as a typed producer; `mutvec_repr` does not override
   freeze-result slots. Self-host fixed point is the gate on **every** task.
3. **Sequence the emit "stop re-deriving typedness" change last**, gated on the
   verify edge-skeleton being proven complete — it is the only step that removes a
   safety net, so it carries the real risk.
4. **Baseline the sub-timings first.** Record `analyze_typed_repr` /
   `route_typed_vectors` numbers before Task 1 (every `PhysPlan` lookup is a
   `Dict<String,_>` string-key alloc); the perf guard diffs against this baseline.

## Non-goals

- Touching Layer 1 (`repr_assign` default MonoType→repr→ValType). It is already
  unified.
- A universal "accept all collection types" injection interface. The repr *set* is
  per-customer by design.
- A generic `PhysPlan<R>` container. Concrete-only until a second real customer
  lands; genericity now is speculative against this design's own findings.
- Building dict or record specialization. Parked until a measured customer exists.
- Making `Vector<Int>` (or any type) globally specialized. Overrides stay
  per-storage-site / per-ABI-edge.
- Replacing emit coercions with an explicit prepared-IR coercion op.
