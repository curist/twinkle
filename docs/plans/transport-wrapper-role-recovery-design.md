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

## The model: consuming-callee recovery ⇒ Consumed

A param that flows to the return is `Consumed` (in-place threaded) when it reaches
the return through a **consuming-callee recovery** — passed to a callee that
consumes it (`base_role == .Consumed`) and hands it back via that callee's
`ret_paths=.fN=from(pK)`, then move-projected back out of the returned wrapper. It
stays `Borrowed` otherwise. The distinguishing fact is *callee consumption*, not the
`transport=move` license alone: a move-projection only proves the wrapper field is
uniquely owned, which is also true for non-recovery projections (see Mechanism).

Concretely: extend the role derivation so that, for a param with `flows_to_return`,
being recovered by such a move-projection sets the "consumed" input to
`reconcile_role` (via `cap`), giving `Consumed`. `in_place_paths` then follows for
free: `build_in_place_paths = shell ∪ dirty`, so a Consumed param has a non-empty
(shell) path even with empty `dirty`, which satisfies `variant_valid`.

## Soundness

The refinement only touches the `esc=Borrowed` branch of `reconcile_role`. The
unsound and irrelevant cases are fenced elsewhere:

- **Read-after NEG** (`red_transport_read_after`): the caller reads `ctx` after
  `synth`, so the arg is not last-use; the transfer's `ret_paths` `OwnedFromParam`
  recovery is gated on `arg_unique` and fails; `ctx` is published; `esc=Retained`;
  `reconcile_role` returns `Published` — unchanged by this design.
- **Non-recovery move-projection** (a param's own field projected out of a
  still-owned record, not a consuming-callee `ret_paths` recovery): excluded by the
  consuming-callee gate on the field linkage, so it never enters
  `move_recovered_params`.
- **Plain borrow-return / non-candidates** (`fn id(x){ x }`): never proposed —
  candidacy requires `scan.found`, so they never reach validation.
- **Over-proposal**: candidacy is a hypothesis; `run_scc_variants` re-analyzes and
  `variant_valid` retracts any variant whose seeded summary is not
  `Consumed`-with-in-place-path + `ret` aliasing the param.

This milestone is analysis/render-only: no codegen dispatch consumes these
variants yet, so a misclassification affects CFG diagnostics, not emitted code.

## Mechanism: `move_recovered_params` → `cap = Consumed`

**Why not a return-witness flag.** The move-recovery fact is *not* persisted where
the return classifier can read it: `RetWitness` records only returned shape
(`WDirect`/`WVariant`/`WUnknown`), and transport-move eligibility is **block-local**
(`BlockPrep.transport`, built per block by `recognize_transport_moves` and consumed
during `ARecordGet` transfer/rendering). So the signal must be *collected*, not read
off an existing summary fact.

**A move-projection alone is not enough (evidence).** `transport=move` means only
"the wrapper field may be moved out" (the field is uniquely owned) — it does *not*
by itself mean the callee consumed the source param. So the signal must also carry
callee-consumption evidence. What supplies it: storing a param into a fresh wrapper
**consumes** it, so a wrapper callee that returns the param already summarizes the
source as `base_role == .Consumed` with `ret_paths=.fN=from(pK)`. (Confirmed:
`fn peek(ctx){ Out.{ ctx, ty: ctx.count } }` summarizes `p0=Consumed
ret_paths=.f0=from(p0)`.) The candidacy scan already tracks exactly this linkage —
`mark_ret_path_field` marks a call result's field derived-from-`pK` from the
callee's `ret_paths=OwnedFromParam(k)`; gate that marking on `cs.params[k].base_role
== .Consumed` so the linkage means "recovered from a *consuming* callee," never a
bare field projection.

**The signal.** Introduce `move_recovered_params: Dict<Int, Bool>` — params `pK`
recovered by a licensed move-projection of a consuming-callee wrapper field. Collect
it (resolver-aware, under the same seed the summary uses, so the callee's owned
variant is selected and its `ret_paths` recovery fires) by mirroring the **real
`ARecordGet` move branch**, not a looser proxy: at each `ARecordGet(base, field) ->
result` where

- `projection_move_licensed(...)` succeeds and the projected `pr.shell` exists (the
  same predicate the transfer uses to actually move), **and**
- the projected shell provenance resolves to a source param `pK`, **and**
- `field` is a consuming-callee-derived field for `pK` (the gated candidacy linkage
  above),

add `pK`. This needs per-site forward facts, so it is a small dedicated collection
over blocks (paralleling the `collect_field_reqs` replay), not a read of block-exit
state.

**Feeding it in.** In the `raw_params` loop, set `cap = .Consumed` for a param in
`move_recovered_params` (in addition to the existing exit-validity check).
`reconcile_role` is unchanged: for `check`, `esc=Borrowed`, `cap=Consumed`,
`flows_to_return=true` → `Consumed`; `in_place_paths = shell ∪ dirty` is then
non-empty (shell), so `variant_valid` accepts. No new state rides the hot forward
fixpoint; the collection is one localized pass in the summary derivation.

**Left to the plan** (not a design blocker): the exact provenance query that
resolves a projected shell to a source param, and whether the collection can reuse
the return-block replay `summarize_seeded` already performs or needs its own block
sweep. A short read-only spike confirms the provenance resolution before wiring
`cap` — a wiring detail, not a mechanism fork.

## What a borrow control actually is (evidence)

The intuitive "borrow transport-wrapper" control does **not** exist for this shape:
storing a param into a fresh wrapper *consumes* it, so any callee that returns the
param via `ret_paths=.fN=from(pK)` already summarizes `pK` as `Consumed`. Confirmed:
`fn peek(ctx){ Out.{ ctx, ty: ctx.count } }` → `p0=Consumed ret_paths=.f0=from(p0)`,
and its caller's projection under the resolver renders `transport=move([.f0] from
p1)` (recovered from the param), **not** `from(fresh)`. So `peek`/`thread` is a
*positive* case (a single-hop transport wrapper), not a borrow control.

Because ret-path recovery already implies consumption, the real over-fire risk for
`move_recovered_params` is **not** a borrow wrapper — it is a licensed
move-projection of a param's own field that is *not* a consuming-callee recovery
(e.g. projecting a field out of a still-owned record param). The consuming-callee
gate exists precisely to exclude that; the control below targets it.

## Testing

Render assertions are **section-scoped**. Note `section_from` runs to end-of-output
(unbounded), so it must not be used to bound a single function's section for a
negative check — use `section_between(out, "fn X [", "fn <next> [")` or add a
bounded `function_section` helper. This includes fixing the existing
`red_transport_read_after` lock, which currently uses `section_from(out, "fn check
")`.

- **Keep green (NEG), bounded:** `red_transport_read_after` stays persistent —
  bound the `check` section with `section_between`, then assert no `verdict ->`, no
  `reuse(unique)` inside it, and no `variant fn check` in the output. Load-bearing
  soundness lock (the read-after arg is not last-use → recovery fails → `ctx`
  published → `esc=Retained` → `Published`).
- **Over-fire control — RESOLVED: unreachable-by-construction (no test shipped).**
  The intended over-fire control was a candidate-shaped licensed move-projection that
  is *not* a consuming-callee recovery (a plain owned-field projection). No such
  candidate can be constructed under the current candidacy: transport-wrapper
  candidacy fires only through `mark_ret_path_field`, which requires the callee to
  return the param via `ret_paths=OwnedFromParam(k)` with `base_role == .Consumed`
  (a consuming recovery); a bare `x := r.field` projection of a still-owned record
  param produces no `ret_paths`-derived candidacy and no `scan.found`, so it is never
  proposed. Delegated-consume candidacy (the `.ACall` ret-alias arm) likewise
  requires `ret_aliases_exactly_param`. So a non-recovery move-projection cannot reach
  variant validation, and `collect_move_recovered_params` cannot over-fire on one.
  The soundness locks are therefore the read-after NEG (`red_transport_read_after`,
  arg_unique gate) plus the `base_role == .Consumed` candidacy gate. Per the "no
  vacuous green" rule, no over-fire test is added.

  Note (Phase 6 boundary): a *pure* transport wrapper (consuming-via-wrap but
  non-mutating) IS a legitimate candidate and now classifies `Consumed` — this is the
  intended, sound outcome, not an over-fire. See
  `transport-wrapper-phase6-conflict-brief.md`.
- **Flip to owned (positive), section- and id-scoped.** Resolve `check`'s FuncId
  from the render (or match the concrete `call Fn<check_id>` site) rather than a
  bare `verdict -> f`:
  - `variant_section(out, "check", "unique:p0")` contains the recovered role
    `p0=Consumed` **and** the synth-call `verdict -> f` selection. Note
    `reuse(unique)` is **not** a signal here: `check` does no `record_update` (it
    recovers ctx via projection), and the leaf `synth` earns no variant (its
    generic summary is already `Consumed`; its return is a fresh wrapper, not
    `ret_derived`). So no `reuse(unique)` appears for this shape.
  - `section_between(out, "fn build [", "fn $init")` contains
    `verdict -> f<check_id>[unique:p0]` (or the `build`→`check` call site).
  - Add `peek`/`thread` as a **single-hop positive** so the fix is covered at
    minimum depth, not only the two-hop `check`.
- **Regression:** boot census in-place counts hold or rise; `make stage2` fixed
  point; `make test` green.

## Scope / non-goals

- **In:** role recovery for the transport-wrapper shape; the up-chain
  (`build → check`) composes via the existing delegated-consume machinery once
  check is Consumed — no new work there.
- **Out:** deep field-backing in-place; codegen dispatch for owned variants;
  changing the shell-only in-place-path representation; any general
  shell-as-mutation change to `has_mut` (explicitly avoided — too broad).
