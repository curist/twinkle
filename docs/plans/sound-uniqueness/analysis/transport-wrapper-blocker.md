# Transport-wrapper composition — blocker analysis (findings)

Context: the transitive-consume work (delegated-consume, delegate-chain) composes
end to end. The **transport-wrapper** shape does not yet, and this note records
exactly why, so the fix can be designed rather than re-derived.

## The shape

```tw
fn synth(ctx: Ctx, name: String) Out {
  ctx.syms = .set(name, ctx.count)   // in-place field updates
  ctx.count = ctx.count + 1
  Out.{ ctx, ty: ctx.count }         // FRESH wrapper carrying ctx in field 0
}

fn check(ctx: Ctx, a: String, b: String) Ctx {
  o1 := synth(ctx, a)                // ctx consumed into synth
  ctx = o1.ctx                       // projection: recover ctx from wrapper field 0
  o2 := synth(ctx, b)
  ctx = o2.ctx
  ctx                                // returns ctx (aliases p0)
}
```

`synth`'s summary: `p0=Consumed paths{[]}  ret=fresh  ret_paths=.f0=from(p0)`
(shell-only in-place path; ctx handed back through return field 0, not `ret`).

DESIRED: `check` earns an owned `[unique:p0]` variant (ctx threaded in place).

## Current state after candidacy broadening

Candidacy now fires correctly. `scan_param_threading` (summary.tw) detects:
`synth` call marks the `Out` result's field 0 derived-from-ctx (via `ret_paths`),
the `ARecordGet` projects it back to a whole-derived `ctx`, and the returned
`ctx` is derived → `ret_derived`. So `check` is proposed as a candidate, and its
seeded variant analysis even recovers **`ret=alias(p0)`**.

But validation retracts it: check's `p0` classifies as **`Borrowed`**, not
`Consumed`, and `variant_valid` requires a Consumed param with a non-empty
in-place path.

## Root cause (why p0 stays Borrowed)

`reconcile_role(esc, cap, has_mut, flows_to_return)` yields `Consumed` only when
`(has_mut OR cap==Consumed) AND flows_to_return`. Here `flows_to_return=true`, but
both other inputs are false:

1. **`has_mut = !dirty.is_empty()` is false.** `dirty` (field paths) comes from the
   FlowFact dataflow (`collect_field_reqs`/`call_result_fact`). Three reasons it
   stays empty:
   - `synth`'s in-place path is **shell-only** `{[]}` — the field updates do not
     survive being moved into the fresh `Out` wrapper.
   - `call_result_fact` only handles a callee that returns the carrier **directly**
     (`ret` alias / `flows_to_return`). `synth` returns via `ret_paths` field, so
     the propagation does not fire at all through the wrapper + projection.
   - Even if it did, `call_result_fact` **skips shell paths** (`if !p.is_shell()`),
     so a shell-only in-place path contributes nothing to `dirty`.

2. **`cap = Consumed` is false — the consume is masked by rebind.** `cap` is set to
   `Consumed` iff the param's **exit-validity** goes false in some block. `ctx` is
   consumed by `synth` (validity false mid-block) but **immediately rebound**
   (`ctx = o1.ctx`) to the recovered value, restoring exit-validity. So the
   mid-block consume never shows at block exit → `cap=NoCap`.

## Why delegated-consume avoided this

The leaf `add` has a real field update `[.f0]`, so its in-place path is
`{[],[.f0]}` (non-shell). `call_result_fact` propagates `[.f0]` to the caller →
`has_mut=true` → `Consumed`, independent of `cap`. The shell-only transport case
has no such field-path signal, so it falls through to the masked `cap`.

## The semantic truth

Threading `ctx` through `synth` **is** an in-place reuse: `synth` mutates ctx's
backing and hands the same region back (`ret_paths=.f0=from(p0)`). Classifying
check's `p0` as `Consumed` is sound and desired. The analysis simply lacks a
signal for "consumed-then-recovered-via-wrapper-projection".

## Design surface (two facts must become right)

- **`base_role = Consumed`** for the threaded param (from `reconcile_role`).
- A **non-empty `in_place_paths`** — note `build_in_place_paths = shell ∪ dirty`,
  so once `role=Consumed` the shell alone makes `in_place_paths` non-empty; the
  field-path `dirty` is not strictly required for validity.

So the crux is producing a sound `Consumed` classification for a param that is
consumed by a callee and recovered through that callee's `ret_paths` projection.
Candidate levers (to be brainstormed):

- **A. FlowFact field-level tracking through the wrapper projection** — mirror the
  `InplaceScan.derived_fields` extension in the FlowFact dataflow, carry the
  callee's in-place paths on the wrapper return-field, project them at
  `ARecordGet`, and let shell count toward the mutation signal.
- **B. Capability recovery via `ret_paths`** — unmask the rebind: recognize that a
  param consumed at a call and recovered via that call's `ret_paths=from(pK)`
  projection was consumed, so `cap=Consumed`.
- **C. Key off the existing transport-move fact** — the forward analysis already
  emits `transport=move([.f] from(...))` at the projection; when the moved value's
  provenance is the seeded param, treat the param as in-place threaded.

All three are analysis/render-only under the current milestone (no codegen
dispatch); an over-broad classification affects CFG diagnostics, not emitted code,
and unsound variants are still retracted by `variant_valid` re-analysis.

## Resolved (2026-07-27)

Shipped via `docs/plans/transport-wrapper-role-recovery-plan.md`. Mechanism:

- **Candidacy consumption gate** — `mark_ret_path_field` gates on
  `cs.params[k].base_role == .Consumed`, so the ret-path linkage is a consuming
  recovery, not a bare field projection.
- **`collect_move_recovered_params`** (`ownership.tw`) — replays each block from the
  seeded fixpoint and records params recovered by a licensed move-projection whose
  projected shell provenance names the param, mirroring the real `.ARecordGet` move
  branch (`projection_move_licensed` + `pr.shell` + `project_path_prov` →
  `param_index_of`). Fed into `cap = .Consumed` in `summarize_seeded`.

Transport-wrapper chains (`build → check → synth`) and the single-hop `wrap/thread`
now compose; the read-after NEG stays persistent (`esc=Retained`). Levers A and C
from the design surface above were **not** needed — the `cap` route sufficed.

**Phase 6 boundary decision (Option A):** a pure (non-mutating) transport wrapper is
indistinguishable from a mutating one once the param is moved into a fresh wrapper
(both summarize `p0=Consumed paths{[]}`), so a pure-transport recovered param now also
classifies `Consumed`. Sound (arg_unique-gated); the `phase6 stage3` test was updated
accordingly. Full record: `../../transport-wrapper-phase6-conflict-brief.md`.

Validation: boot census `dict_set` in-place 396→397 (no regress), self-host fixed
point holds (stage3 == stage4), `make test` green.
