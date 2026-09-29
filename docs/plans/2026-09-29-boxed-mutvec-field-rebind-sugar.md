# Boxed MutVec for Field-Rebind Sugar (Follow-up)

**Status:** Not started. Scoped out of the boxed-reference-MutVec work
(`2026-09-28-boxed-reference-mutvec.md`) after Task 4/5 landed, because the fix is
feature-sized rather than a bounded extension.

## Problem

The boxed recursive-aggregate MutVec route activates for the explicit
record-rebuild carrier form:

```tw
next = State.{ people: swap(next.people, i, j), count: next.count }   // ARecord → boxes
```

but **not** for the idiomatic field-rebind sugar the language otherwise
encourages (see `CLAUDE.md`, "Immutability and Rebinding"):

```tw
next.people = swap(next.people, i, j)   // ARecordUpdate → stays persistent
```

Both compile and produce identical results; only the first takes the boxed
route. This violates the parent plan's Global Constraint "add no required source
rewrite": to get the optimization today, ordinary code must be rewritten to the
non-idiomatic full-reconstruction form.

Reproduce: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_escape.tw`
boxes (`mutvec_get`/`set`/`freeze` in its WAT); change its loop's
`next = State.{ people: swap(...), count: next.count }` to
`next.people = swap(next.people, ...)` and the boxed ops disappear.

## Precise drop point (deepened investigation)

Traced end to end with sugar-only instrumentation. The variant candidate for
`permute` is proposed, its SCC converges, but the converged summary **fails
`summary.variant_valid`** (retracted at the SCC validate step → `member_key`
emptied → nothing published → `spec.routes` empty). `variant_valid` fails because
the converged variant summary has **empty `in_place_paths` (`param_ok=false`) and
empty `ret_paths`**. Both trace to three *separate* analyses that each treat
`ARecordUpdate` weaker than `ARecord`, and all three must line up in the
non-monotone field-flow/SCC fixpoint:

1. **Field-flow (`ownership.transfer_flow` → `collect_field_flow_with_live` →
   `reqs`).** `ARecordUpdate` on a base whose whole-value origin is `< 0` (a fresh
   `ARecord`-built carrier) returned `ff_none()`, dropping the per-field origins
   and the written value's dirty. **Candidate fix (verified to work in isolation):**
   in the `origin < 0` branch, keep the base's field origins and, for a dirtied
   exact param-field projection, add `vid.field(field)` to `bf.dirty` — mirroring
   the `ARecord` arm. With this, one field-flow run yields `rf.fields=[people<-state]`
   and `reqs=[state:{people}]` (`has_mut=true`). **But it is unstable:** the fact
   only appears in the run where `swap` is resolved to its owned variant; other
   rounds see `swap` generic, the reconstructed field origin differs from the
   carrier's, and the exact-map `join_fact` guard (documented non-monotone) drops
   it — the fixpoint settles on empty.
2. **Main ownership body (`path_prov` / `field_own`, the `ARecordUpdate` arm near
   `ownership.tw:8322`).** `ret_paths` is built (around `ownership.tw:9364`) from
   the returned atom's `body.path_prov_get` + `body.field_own_get` (NOT the
   field-flow). For `people` to become `OwnedFromField(state, people)` the returned
   `next` needs a single-segment `.Field(people)` in `path_prov` AND
   `field_own.is_unique(field_path(people))`. The `ARecordUpdate` field-lineage in
   this analysis does not establish that for a fresh-base update, so `ret_paths`
   stays empty even when the field-flow run is good.
3. **Role gate (`reconcile_role`, `ownership.tw:8227`).** `Consumed` requires
   `(has_mut or cap==Consumed) and flows_to_return`. `flows_to_return`
   (`param_flows_to_return`) is true only for `MayAliasParams(k)` or a ret_path
   `OwnedFromParam(k)` — it does **not** count `OwnedFromField`. So even the good
   run (`has_mut=true`) yields role `Borrowed` because `ret_paths` is empty →
   `in_place_paths` empty → `variant_valid` fails. Fixing (1) alone is
   insufficient; (2) must also populate `ret_paths` (and possibly
   `param_flows_to_return`/`reconcile_role` must accept an `OwnedFromField` return
   as flowing-to-return).

The rebuild (`ARecord`) form succeeds because all three analyses treat a full
reconstruction as owned-field-preserving and its field origins are stable across
the loop join.

## Root-cause map (from the Task 4 investigation)

The divergence is upstream of the mutvec codegen, in ownership/variant
specialization (`spec.routes` is empty for the sugar form, so
`detect_aggregate_regions` sees nothing):

1. **Op shape:** field-rebind sugar lowers to `ARecordUpdate(base, field, value)`;
   the rebuild form lowers to `ARecord(tid, fields)`.
2. **Route demand:** `variant_specialize.specialize_module_with_cap_sem` produces
   a live owned route (`permute$v438`) for the rebuild form and **zero** routes
   for the sugar form. `updatable_funcs` marks the function updatable in both
   (initial `next := State.{...}` is an `ARecord`), so the coarse filter is not
   the gate.
3. **Candidate parity:** `summary.candidate_variants` proposes the **same**
   field-path candidate (`variant_for_paths(permute, state, {people})`) for both
   forms — confirmed by instrumentation (`found=false ttr=false recon=true
   nonshell=true npaths=2`). So the drop is in validation/convergence, not
   proposal.
4. **Convergence + validation parity:** both SCCs converge (`changed=false`
   within cap), and where `summary.variant_valid` is reached it sees **identical
   passing** data for both (`ret=OwnedFresh`, `ret_paths=[OwnedFromField(state,
   people)]`, `in_place=2`). Yet `compute_variants().by_func` ends up **without**
   the sugar variant. At that point the observed facts are internally
   inconsistent with a single-pass reading of `run_scc_variants` — the real
   behavior is entangled with `summary.compute_cached` / `TWINKLE_8G_FIXREUSE`
   reuse and/or multi-pass SCC re-seeding, which the investigation did not fully
   isolate.

## Concrete asymmetries found (candidate fixes, individually insufficient)

Both are real `ARecord` vs `ARecordUpdate` asymmetries in `boot/compiler/ownership.tw`.
Patching either or both did **not** flip the route on its own, so they are
necessary-looking but not sufficient — the primary blocker is deeper in step 4.

- **Field-fact drop on fresh base** (`flow_op`, the `.ARecordUpdate` arm): the
  result keeps the base's per-field origin facts only when `bf.origin >= 0`. A
  fresh `ARecord`-built base has whole-value origin `-1` but valid per-field
  facts, so updating one field of it returns `ff_none()` and loses the
  `OwnedFromField(state, people)` lineage. `ARecord` never gates on `origin`. Fix
  direction: in the `origin < 0` else-branch, preserve `bf`'s field facts (no
  shell/param-consumption dirty, since there is no param base).
- **`ret_witness` missing arm** (`ownership.tw`): the return-value witness scan
  maps `.ARecord => WDirect` and `.AVariant => WVariant` but has no
  `.ARecordUpdate` arm, so a record-update return falls to `WUnknown` and "drops
  every claim it could produce" unless field facts already pinned `WDirect`. Add
  `.ARecordUpdate(_, _, _, _, _) => .WDirect`.

## Suggested approach

1. Reproduce with a minimal fixture; add a red boxed-route test asserting the
   sugar form yields a live route + `mutvec_get`/`set`/`freeze`.
2. Land the two `ownership.tw` asymmetry fixes with their own ownership/summary
   unit coverage (they are defensible independently).
3. Instrument `run_scc_variants` end to end (candidate → seed → converge →
   validate → publish) with fixreuse **off** and **on** to isolate why the sugar
   variant is absent from `by_func` despite converging and validating. This is
   the crux and the highest-risk part — it touches soundness-critical code, so
   pair every change with the sound-uniqueness suite and a self-host fixed-point
   check.
4. Guard heavily: full `make test`, `make stage2` fixed point, and the COW/
   sound-uniqueness census must stay clean — a false owned route here corrupts
   live aliases.

## Non-goals / cautions

- Do not weaken exact-mono, projected-borrow, or fallback isolation checks to
  force the route.
- Do not introduce a source-level annotation or API; the whole point is that
  idiomatic sugar optimizes with no rewrite.
