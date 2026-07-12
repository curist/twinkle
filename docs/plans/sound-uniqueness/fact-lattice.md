# Ownership Fact Lattice and Transfer Rules

**Status:** Draft subplan

This is the semantic core of the sound-uniqueness analysis: the ownership fact
lattice, the per-op transfer function, the control-flow merge rules, and the
function-summary/specialization model. It is designed against the real ANF shapes
in [worked-examples.md](worked-examples.md) and realizes the analysis the
[architecture.md](architecture.md) umbrella and [cfg-ownership-ir.md](cfg-ownership-ir.md)
view describe.

Guiding rule throughout: **soundness before coverage.** Every default is the
conservative one (not owned), and any operation whose effect the analysis cannot
prove drops the fact to the persistent path.

## What the lattice tracks

For each ANF local at each program point, the question is: *is the value bound to
this local uniquely owned here, such that destructively mutating its backing
storage is unobservable?* Ownership is a property of a **value**, which may be
referenced by more than one local (Case C: `L7` and `L8` name the same value), so
the model is affine: **a value has at most one live owner; creating a second live
reference demotes it to shared.**

## Lattice elements (per local)

```
        Unowned  (⊤ — conservative, no mutation)
       /    |    \
 Shared  Moved   (both are non-owning terminal facts)
       \    |    /
   OwnedPersistent(shape)     ← uniquely owns a persistent value; may thaw in place
   OwnedMutable(shape)        ← uniquely owns a live mutable-region handle
        (most informative)
```

- **Unowned** — no proof. Default for parameters (unless specialized), results of
  unknown calls, values read from globals. Cannot mutate.
- **Shared** — the value is observable through another live reference (aliased,
  captured, stored, published). Persistent-only.
- **Moved** — the value was transferred out of this local (consumed by an op or
  reassigned elsewhere). The local must not be read as owned until rebound.
- **OwnedPersistent(shape)** — uniquely owns a persistent value; a mutable region
  may *begin without copying*.
- **OwnedMutable(shape)** — uniquely owns a live mutable-region handle, between
  `begin`/`thaw` and `freeze`.

`Shared` and `Moved` both sit below `Unowned` but are **not interchangeable**:
`Moved` means the value left this local and a later rebind can restore a fact,
whereas `Shared` means the value is permanently aliased/persistent. That is why
the join treats them differently (`Owned ⊔ Moved = Unowned` but
`Owned ⊔ Shared = Shared`, below).

**Publication is an event, not a lattice element.** When an owned value reaches a
publication point (return, value-carrying `break`, `try` exit, storage in an
escaping aggregate, closure/task capture, channel send, global, unknown call), it
is frozen if it was `OwnedMutable` and the local's fact becomes **`Shared`** — the
value is now observable outside the region. Transfer-table rows written “publish”
mean exactly this: fact → `Shared`, with a `freeze` inserted if it was
`OwnedMutable`.

`shape` carries structure so deep vs shell is expressible:

- `Vec { elem: Fact }` — collection with an element-ownership fact (default
  `Unowned` for nested contents until proven);
- `Dict { val: Fact }` — same, for values;
- `Record { shell: owned?, fields: FieldId → Fact }` — shell ownership plus a
  per-field fact map.

The **join** (control-flow merge) is the meet toward the conservative top:
`Owned ⊔ Owned` (same region/shape) = `Owned`; `Owned ⊔ Shared` = `Shared`;
`Owned ⊔ Moved` = `Unowned`; anything `⊔ Unowned` = `Unowned`. Record/collection
shapes join field-wise / element-wise. A fact survives a merge only if it holds on
**every** predecessor path.

## The `AInit` hinge: move vs alias

`let L = init A` is the pivotal node. Given `A = ALocal(S)`:

- **Move** if `S` is dead after this point → `L` takes `S`'s fact; `S → Moved`.
- **Alias** if `S` is still live afterward → the value now has two live
  references, so **both `L` and `S` become `Shared`**.

This single rule decides the worked negatives:

- Case B `L4 = init L9` — `L9` dead after → move → `L4` stays owned.
- Case C `L8 = init L7` — both `L7` and `L8` read later → alias → both `Shared`,
  so the later `call Fn295(L7,…)` sees a shared arg and stays persistent.

`AInit` therefore requires **liveness**: the analysis needs "is the source local
live after this point," which the CFG view supplies.

## Transfer function (per `AnfOp`)

`AnfOp` is closed, so the table is exhaustive. `let L = op`:

| Op | Effect on facts |
|---|---|
| `ACall(constructor)` — `Dict.new`, `Vector.make`, builder freeze | `L ← OwnedPersistent` with a fresh, deeply-owned shape |
| `ARecord(fields)` | `L ← OwnedPersistent(Record{shell:owned, fields})`; each field arg **moved in** (its fact becomes the field fact; local → `Moved`) |
| `AArrayLit` / `AVariant(args)` | `L ← OwnedPersistent` with a fresh shape; args moved in; if an arg cannot be moved (shared), the container’s element fact is `Shared` |
| `ARecordGet(base, f)` | `L ← borrow` of field `f`. Base ownership **preserved** (read is not a consume). If `L` later flows only to reads → transient borrow; if `L` is stored/returned/captured → publishes field `f` and demotes base’s field fact |
| `AIndex(base, i)` | `L ← borrow` (element read). Same borrow-vs-publish rule as `ARecordGet` |
| `ACall(consuming)` — `dict.set`, `vector.append`, `Vector.set`, summarized wrapper: *consumes p0, returns owned* | `L ← OwnedPersistent(result)`; arg p0 → `Moved`. **In-place begin licensed iff p0 was `Owned` at the call**; otherwise the result is still owned but `begin` copies (persistent path) |
| `ARecordUpdate(base, f, v, in_place, _)` | `L ← base.record-fact with field f ← v.fact`. **`in_place` licensed iff base was `Owned`**; if in-place, base → `Moved` |
| `AAssign(local, A)` | `local ← A.fact` (ownership transfer; the loop-carried rebind) |
| `AInit(A)` | move/alias hinge above |
| `AMakeClosure(_, captured)` | each captured local → `Shared` (published), unless the closure is proven non-escaping (future; see closure-capture.md) |
| store into an **escaping** aggregate | stored local → `Shared` |
| `AGlobalSet(_, A)` | `A → Shared` |
| `ACall(Cell.new / Cell.set / Cell.update)` — store into a `Cell` | **publish** the stored value → `Shared` (a `Cell` is a mutable box, aliasable and readable at arbitrary times); the returned `Cell` handle is owned but its contents are `Shared`. `Cell.update` reads-then-writes, so — like `Cell.get` — the value handed to the update function is `Unowned` |
| `ACall(Cell.get)` | `L ← Unowned` (contents stay aliased through the live cell). `Cell` is not an optimization target — already mutable by design; these rows only keep the analysis sound around it |
| `Return(A)` / `Break(A)` / match-arm body ending in `Return` (`try`) | publish `A` → `A` becomes `Shared`; insert `freeze` on this edge if `A` was `OwnedMutable` |
| `ACall(unknown/unsummarized)` | every reference arg → `Shared`; `L ← Unowned` |
| `ABinOp`/`AUnOp`/scalar ops | no reference-ownership effect |

Two consequences worth stating explicitly:

- **A consuming op always produces an owned *result***, even on a shared input —
  because e.g. `dict.set` semantically returns a new value. What the input’s
  ownership gates is only whether `begin` is in-place vs copying. Do not conflate
  "result owned" with "input owned."
- **The record quartet carries two independent in-place decisions.** For
  `record_get .f` → consuming call → `record_update .f` → `assign`: the *field
  backing* in-place is licensed by field `f` being deeply owned; the *shell*
  in-place is licensed by the record being owned. They are annotated separately
  (worked-examples Case V).

## Control flow

- **Branch/match join.** Local facts merge by the lattice join. Case V’s
  `if … { record_update .lowlinks; assign L7 } else { }`: the updating arm ends
  with `L7` owned (consumed then reassigned) and the empty arm leaves `L7` owned →
  join `Owned`. If one arm published `L7` and the other did not, the join is
  `Shared`.
- **Loop back-edge.** The loop-header entry fact for a carried local is
  `join(pre-loop fact, back-edge fact)`, iterated to fixpoint. A local stays
  `Owned` across the loop iff every body path ends with it `Owned`
  (borrow-only reads, or consume-then-reassign) and no path publishes/aliases it.
  Monotone over the finite lattice ⇒ terminates.
- **Multi-exit publication.** `Return`, value-carrying `Break`, and `try` error
  arms are all exit edges (worked-examples Case T); each publishes the exiting
  value and forces a `freeze` if it was `OwnedMutable`. The fallthrough path keeps
  the region alive.

## Function summaries

> The summary schema, SCC-ordered computation, variant identity, call-site
> decision, caps, and determinism are specified in full in
> [summary-specialization.md](summary-specialization.md). This section states just
> the fact-lattice-level shape.

Interprocedural facts, computed per function (schema authoritative in
[summary-specialization.md](summary-specialization.md); `ParamRole` has **three**
values, with "returned" factored out into `flows_to_return` + `ReturnOwn`):

- **per parameter `base_role`** ∈ { `Borrowed` (read-only, ownership-neutral),
  `Consumed` (mutated/moved; caller’s value dead after if in-place taken),
  `Published` (escapes inside the callee) };
- **`flows_to_return`** — whether the parameter’s value flows into the return;
- **return ownership** (`ReturnOwn`) ∈ `OwnedFresh` | `OwnedFromParam(k)` |
  `Shared`;
- **`in_place_paths`** — the field paths a `Consumed`+`flows_to_return` parameter
  mutates in place if the caller proves them owned (`[]` = shell/whole value).

Worked summaries:

- `set_at` wrapper — `p0: Consumed`, `flows_to_return`, `in_place_paths {[]}`;
  return `OwnedFromParam(0)`.
- `add_type` — `p0: Consumed`, `flows_to_return`, `in_place_paths {[], [.types]}`,
  `p1,p2: Borrowed`; return `OwnedFromParam(0)`.
- `visit` — `p0(State): Consumed`, `flows_to_return`; return `OwnedFromParam(0)`;
  **recursive**, so its summary depends on itself.

Summaries are computed **bottom-up over call-graph SCCs** (the compiler already
has Tarjan SCC). Within a recursive SCC, iterate summaries to fixpoint. The
iteration must stay sound at every step — start from the conservative assumption
(parameters `Borrowed`, no in-place capability) and only promote a parameter to
in-place-capable when every recursive path supports it, so a partial fixpoint is
never unsound.

## Interprocedural specialization

For a callee with in-place-capable parameter `k`, a call site emits the
**mutable-specialized variant** iff, at that site, the argument’s fact is `Owned`
**and** the old version is not observed after the call; otherwise it emits the
**persistent variant**. This is exactly Cases B ∩ C: `add_type` gets a mutable
variant at `build_env`’s owned call sites and the persistent variant at
`branch_env`’s shared call site.

- **Specialization key** = the subset of the callee's in-place `(parameter,
  field-path)` requirements that are `Owned` at the site (the shell path `[]` and
  each mutated field independently — so a shared sibling field does not block a
  field's in-place update). Read-only/`Borrowed` parameters and fields never
  participate. Full schema in [summary-specialization.md](summary-specialization.md).
- **Cap.** Bound the number of ownership variants per function (composing with
  type-monomorphization, per architecture.md); on overflow, fall back to the
  persistent variant. The census suggests most functions need at most one extra
  variant (uniform caller shapes, e.g. `visit`), so the cap should rarely bind.
- No runtime uniqueness test: the caller statically selects the variant.

## Validation trace (against the anchors)

- **Case B `build_env`:** `Dict.new ⇒ Owned`; `record Env ⇒ Owned(shell + owned
  fields)`; `init L9 ⇒ move` (owned); each `call Fn295 ⇒` owned arg + summary
  in-place-capable ⇒ mutable variant; `assign` transfers owned back; `return ⇒`
  publish once. **Verdict: owned-mutable, in-place add_type.** ✓
- **Case C `branch_env`:** `init L7 ⇒ alias` (both `Shared`); `call Fn295(L7)`
  sees `Shared` arg ⇒ persistent variant; later `record_get L8 .types` reads the
  old version — consistent with the demotion. **Verdict: persistent.** ✓
- **Case A `sieve`:** freeze ⇒ Owned; `index` ⇒ borrow (ownership preserved);
  `call Fn297` with owned arg + wrapper summary ⇒ in-place; `assign` back across
  the back-edge; loop fixpoint holds Owned; never published. **Verdict:
  owned-mutable.** ✓
- **Case V `visit`:** quartet keeps `L7` owned across each field update; match-arm
  join merges to Owned; recursion resolved by the SCC summary fixpoint (all
  callers owned ⇒ single owned variant); nested `components` write moves an owned
  `Vector` into the field. **Verdict: owned-mutable, both in-place decisions per
  quartet.** ✓

## Open questions

- Exact representation of `shape` for records vs nested collections — inline
  recursive fact, or interned shape ids to bound size?
- Element/value ownership for nested collections: track precisely, or default
  `Unowned` and only refine on freshly-introduced-and-never-shared contents?
- Recursive-SCC summary fixpoint direction and termination proof — confirm the
  "start conservative, promote monotonically" schedule is both sound and precise
  enough for `visit`.
- How much liveness precision does the `AInit` hinge need (per-local last-use vs
  full liveness), and can the CFG view supply it cheaply?
- Does `OwnedMutable` need to appear in the fact domain during Phase 1 analysis,
  or only at codegen once regions are formed?

## Non-goals

- No runtime uniqueness flags, refcounts, or COW checks — soundness is static.
- No speculative "may be shared" mutation.
- No lattice element that permits mutation without a proof on **every** path.
- No abandoning conservative defaults for coverage.
