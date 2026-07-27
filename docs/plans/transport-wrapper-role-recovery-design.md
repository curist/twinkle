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

## Insertion point — decide by spike

Two viable places to surface the move-recovery signal. A focused, read-only spike
(instrument the seeded summary derivation for `check` vs a borrow-return control
like `id`) picks the one that reads an **existing** fact with the least new state:

- **(i) `cap` via mid-block consume** — record that the param's entry value was
  consumed at some instruction even if the local is later rebound, and feed it
  into `cap`. Simple signal; may require threading a small "consumed params" fact
  through/alongside the forward pass.
- **(ii) return-witness move flag** *(lean)* — extend the return classification so a
  param recovered via a `transport=move … from(pK)` projection marks `pK`
  Consumed. Localized to the summary derivation (`summarize_seeded` /
  `reconcile_role` inputs); no new state in the hot forward pass — **if** the
  move-vs-borrow fact is reachable from the return-block ForwardState the
  classifier already holds.

Decision rule: prefer (ii) if the move-recovery is derivable from facts the return
classifier already reads; otherwise (i).

## Testing

- **Keep green:** `red_transport_read_after` (NEG) stays persistent — no
  `variant fn check`, no `verdict ->`, no `reuse(unique)` in `check`. This is the
  load-bearing soundness lock.
- **Add a borrow control** (small fixture: `fn id(x){ x }` threaded from a caller)
  asserting it stays `Borrowed`/persistent, so the move/borrow distinction is
  pinned, not just asserted for the positive.
- **Flip to owned:** `red_transport_wrapper_chain` composes — `variant fn check
  [unique:p0]`, `verdict -> f`, `reuse(unique)`, and `build` selects check.
- **Regression:** boot census in-place counts hold or rise; `make stage2` fixed
  point; `make test` green.

## Scope / non-goals

- **In:** role recovery for the transport-wrapper shape; the up-chain
  (`build → check`) composes via the existing delegated-consume machinery once
  check is Consumed — no new work there.
- **Out:** deep field-backing in-place; codegen dispatch for owned variants;
  changing the shell-only in-place-path representation; any general
  shell-as-mutation change to `has_mut` (explicitly avoided — too broad).
