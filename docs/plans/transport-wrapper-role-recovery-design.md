# Transport-wrapper role recovery — design

**Status:** design (approved), pending implementation plan.

**Extends:** `docs/plans/transitive-consume-plan.md` (Task 6). Root-cause evidence:
`docs/plans/sound-uniqueness/analysis/transport-wrapper-blocker.md`.

## Goal

Let owned in-place threading compose through the **transport-wrapper** delegation
shape, so a function that threads a param through a callee returning a fresh
wrapper (`ret_paths=.fN=from(pK)`) and projects the param back out earns an owned
`[unique:pK]` variant instead of collapsing to persistent.

Worked shape:

```tw
fn synth(ctx: Ctx, name: String) Out {
  ctx.syms = .set(name, ctx.count)
  ctx.count = ctx.count + 1
  Out.{ ctx, ty: ctx.count }          // synth: p0=Consumed paths{[]} ret=fresh ret_paths=.f0=from(p0)
}

fn check(ctx: Ctx, a: String, b: String) Ctx {
  o1 := synth(ctx, a)
  ctx = o1.ctx                         // projection recovers ctx (transport=move [.f0] from p0)
  o2 := synth(ctx, b)
  ctx = o2.ctx
  ctx
}
```

DESIRED: `check` earns `p0=Consumed paths{[]} ret=alias(p0)`, after which
`build → check` composes through the already-shipped delegated-consume path.

## What already works (do not rebuild)

- **Candidacy** (uncommitted, from Task 6): `scan_param_threading` proposes `check`
  as a candidate via the `ret_paths` field marking + `ARecordGet` projection +
  `ret_derived` return gate. Keep this.
- **Seeded recovery**: under a `[unique:p0]` seed, the forward analysis already
  selects synth's owned variant at each call and recovers `ctx` as
  `transport=move([.f0] from p0)`, yielding check's variant `ret=alias(p0)`.

The single remaining gap: check's `p0` classifies as **`Borrowed`**, not
`Consumed`, so `variant_valid` retracts it.

## Root cause (one sentence)

`reconcile_role(esc, cap, has_mut, flows_to_return)` needs `has_mut` or
`cap==Consumed` to reach `Consumed`; the transport wrapper has neither, because
synth's in-place path is shell-only (no `has_mut` field path) and the param's
consume is masked by the immediate rebind (`cap` is derived from *exit*-validity,
which the rebind restores). See the blocker note for the full chain.

## The model: move-recovery ⇒ Consumed

A param that flows to the return is `Consumed` (in-place threaded) when it reaches
the return through a **move-recovery** — consumed by a callee and handed back via
that callee's `ret_paths` projection — and stays `Borrowed` when it reaches the
return through a plain **borrow** (`fn id(x){ x }`). The move-vs-borrow fact is
already computed at the projection (`transport=move` vs `transport=borrow`); this
design surfaces it into the role.

Concretely: extend the role derivation so that, for a param with
`flows_to_return`, being the origin of a move-recovery projection sets the
"consumed" input to `reconcile_role` (via `cap`, or an equivalent signal), giving
`Consumed`. `in_place_paths` then follows for free: `build_in_place_paths = shell ∪
dirty`, so a Consumed param has a non-empty (shell) path even with empty `dirty`,
which satisfies `variant_valid`.

## Soundness

The refinement only touches the `esc=Borrowed` branch of `reconcile_role`. The
unsound cases are fenced *before* it:

- **Read-after NEG** (`red_transport_read_after`): the caller reads `ctx` after
  `synth`, so the arg is not last-use; the transfer's `ret_paths` `OwnedFromParam`
  recovery is gated on `arg_unique` and fails; `ctx` is published; `esc=Retained`;
  `reconcile_role` returns `Published` — unchanged by this design.
- **Borrow-return** (`fn id(x){ x }`): no move-recovery, so the consumed signal is
  absent → stays `Borrowed`.
- **Over-proposal**: candidacy is a hypothesis; `run_scc_variants` re-analyzes and
  `variant_valid` retracts any variant whose seeded summary is not
  `Consumed`-with-in-place-path + `ret` aliasing the param.

This milestone is analysis/render-only: no codegen dispatch consumes these
variants yet, so a misclassification affects CFG diagnostics, not emitted code.

## Mechanism: `move_recovered_params` → `cap = Consumed`

**Why not a return-witness flag.** The move-vs-borrow fact is *not* persisted where
the return classifier can read it: `RetWitness` records only returned shape
(`WDirect`/`WVariant`/`WUnknown`), and transport-move eligibility is **block-local**
(`BlockPrep.transport`, built per block by `recognize_transport_moves` and consumed
during `ARecordGet` transfer/rendering). So the signal must be *collected*, not read
off an existing summary fact.

**The signal.** Introduce `move_recovered_params: Dict<Int, Bool>` — the params `pK`
recovered by a licensed move-projection. Collect it by replaying the function's
blocks (resolver-aware, under the same seed the summary uses, so the callee's owned
variant is selected and its `ret_paths` recovery fires): at each `ARecordGet(base,
field) -> result` site where the projection is a licensed move
(`transport_has(prep.transport, result)`) **and** the recovered value's path
provenance resolves to param `pK`, add `pK`. This parallels the existing
`collect_field_reqs` replay pass — it needs per-site forward facts, so it is a small
dedicated collection over blocks, not a read of block-exit state.

**Feeding it in.** In the `raw_params` loop, set `cap = .Consumed` for a param in
`move_recovered_params` (in addition to the existing exit-validity check).
`reconcile_role` is unchanged: for `check`, `esc=Borrowed`, `cap=Consumed`,
`flows_to_return=true` → `Consumed`; `in_place_paths = shell ∪ dirty` is then
non-empty (shell), so `variant_valid` accepts. No new state rides the hot forward
fixpoint; the collection is one localized pass in the summary derivation.

**Left to the plan** (not a design blocker): the exact provenance query that
resolves a projected value to a source param, and whether the collection can reuse
the return-block replay `summarize_seeded` already performs or needs its own block
sweep. A short read-only spike confirms the provenance resolution before wiring
`cap` — this is a wiring detail, not the two-way mechanism fork the prior draft
implied.

## Testing

All render assertions are **section-scoped** (`section_between` / `variant_section`
/ `section_from`) so a broad token cannot match an unrelated function's section —
mirroring the delegate/mixed locks already flipped.

- **Keep green (NEG):** `red_transport_read_after` stays persistent — in the `check`
  section: no `verdict ->`, no `reuse(unique)`; and no `variant fn check` in the
  output. Load-bearing soundness lock.
- **Borrow control — must be non-vacuous.** `fn id(x){ x }` is useless here: it is
  never proposed (candidacy needs `scan.found`), so it never enters validation and
  proves nothing about role recovery. Instead add a **candidate-shaped** borrow
  transport-wrapper whose inner callee *borrows* rather than consumes the threaded
  param, e.g.:

  ```tw
  fn peek(ctx: Ctx) Out { Out.{ ctx, ty: ctx.count } }   // reads ctx, does NOT mutate/consume
  fn thread(ctx: Ctx) Ctx { o := peek(ctx); ctx = o.ctx; ctx }
  ```

  `peek` still carries `ret_paths=.f0=from(p0)`, so `thread` *is* proposed as a
  candidate and enters variant validation — but the projection is a
  `transport=borrow`, not a move, so `p0 ∉ move_recovered_params` → stays
  `Borrowed`. Assert: `thread` enters validation yet its variant is **retracted**
  (no `variant fn thread`, `thread`'s generic summary keeps `p0=Borrowed` with
  empty in-place paths). The plan must *verify* this fixture genuinely reaches
  validation (candidacy fires) so the control is not silently vacuous; a direct
  unit assertion on `thread`'s seeded summary classification is an acceptable
  substitute if the fixture proves awkward.
- **Flip to owned (positive), section-scoped:**
  - `variant_section(out, "check", "unique:p0")` contains a synth-call
    `verdict -> f` selection **and** `reuse(unique)`;
  - `section_between(out, "fn build [", "fn $init")` contains a `verdict -> f`
    selecting `check`.
- **Regression:** boot census in-place counts hold or rise; `make stage2` fixed
  point; `make test` green.

## Scope / non-goals

- **In:** role recovery for the transport-wrapper shape; the up-chain
  (`build → check`) composes via the existing delegated-consume machinery once
  check is Consumed — no new work there.
- **Out:** deep field-backing in-place; codegen dispatch for owned variants;
  changing the shell-only in-place-path representation; any general
  shell-as-mutation change to `has_mut` (explicitly avoided — too broad).
