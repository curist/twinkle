# Boxed MutVec for Field-Rebind Sugar (Follow-up) — SHIPPED

**Status:** Complete. The boxed recursive-aggregate MutVec route now fires for the
idiomatic field-rebind sugar (`rec.f = helper(rec.f)`, an `ARecordUpdate`), not
only the explicit full record reconstruction (`rec = T.{...}`, an `ARecord`). The
AWFY benchmark `examples/performance/awfy/twinkle/boxed_record_permute.tw` was
switched to the sugar form and still boxes (checksum 8696, ~8.1× over the
`TWINKLE_VARIANT_SPECIALIZE=0` persistent baseline), fulfilling the parent plan's
"add no required source rewrite" constraint. Self-host fixed point holds; full boot
suite (3777) green including the projected-field soundness guard.

## What was actually wrong (verified with sugar-only instrumentation)

The candidate variant for `permute` was proposed identically for both forms and its
SCC converged, but the sugar form's converged summary failed `variant_valid` with
empty `in_place_paths` and empty `ret_paths`. Tracing the seeded field-tier variant
analysis (`summarize_variant_resolved`, key `[shell, .people]`) isolated three
concrete divergences from the `ARecord` form — the earlier "entangled with
fixreuse / multi-pass SCC" hypothesis was a red herring caused by mixing both forms
in one instrumented run. Isolated per-form, the drops were deterministic:

1. **Field-flow lineage dropped on a fresh carrier**
   (`ownership.transfer_flow`, the `.ARecordUpdate` arm). When the base's
   whole-value origin is `< 0` (a fresh `ARecord`-built carrier), the arm returned
   `ff_none()`, discarding the per-field origins it had just computed. The `ARecord`
   arm never gates on the whole-value origin. **Fix:** in the `origin < 0` branch,
   preserve `bf`'s field origins and dirty the updated reference field when the
   written value carries a param-field origin (no shell dirty — there is no owned
   param shell). This is a requirements readout; `field_own` still proves ownership
   downstream.

2. **Projection read stayed a borrow, so the helper's result was not owned.**
   `next.people` (the `ARecordGet` feeding `swap`) could not be a last-use move
   because `next` stays live as the `ARecordUpdate` base — unlike the reconstruction
   form, where `reconstruction_projection_dead` licenses the move because the whole
   record is rebuilt. The `quartet` recognizer is built for exactly this
   read-modify-write shape, but `op_moves_one_of` only accepted a *builtin* COW
   update (`Vector.set`) as consuming the derived value, never a user helper like
   `swap`. **Fix:** in `op_moves_one_of`, also accept a summarized user call that
   takes the derived local at last use **when the callee has an owned variant that
   consumes that parameter** — checked via the already-threaded `VariantResolver`
   (`resolve(fid, [shell at pos]).is_some()` with a non-empty `in_place_paths`).
   `swap` has such a variant; `to_string` (which merely reads its argument) does
   not, so a read-only projected use stays persistent. Because `resolve` is
   `generic_only_resolver` in the generic analysis and the real resolver during
   seeded/variant-selection analysis, this never affects the generic verdict and
   fires exactly where an owned route can actually be published. `resolve` is
   threaded into `block_prep`/`recognize_quartet_moves`/`quartet_ok` (it was
   already a parameter of every enclosing analysis function).

3. **Codegen decomposition had no `ARecordUpdate` carrier arm.**
   With (1) and (2) the route was published (`permute$v…` specialized), but
   `mutvec_aggregate_rewrite.decompose_clone`'s `emit_carrier_binder` handled only
   the `.ARecord` carrier reconstruction and fell through to `.None` for
   `.ARecordUpdate`, so `build_aggregate` bailed (`ok=false`) and no `$mvagg`
   sibling was emitted. **Fix:** add an `.ARecordUpdate` arm that decomposes the
   update exactly like the `ARecord` arm — the updated field takes the new value,
   the untouched field carries over from the base carrier's matching component.

All three were necessary; only together do they box.

## Soundness

The projection-move license (2) is guarded by the callee's own owned-variant proof
(a helper that retains/borrows its argument has no such variant), and the whole
boxed route is still gated by `variant_valid`, exact-mono identity, the aggregate
verifier, and projected-borrow shell isolation. The existing negative test
`cfg field facts::read-only use of a projected field does not make the replacement
field-in-place` (a `to_string`-style read-only helper) continues to pass. Verified:
full boot suite 3777/3777, sound-uniqueness + uniqueness-census suites green, Rust
`cow_analysis` guard green, `make bundle-cli` reaches `stage3 == stage4`.

## Touched files

- `boot/compiler/ownership.tw` — field-flow `ARecordUpdate` arm; `op_moves_one_of`
  owned-variant user-call license + `callee_consumes_param`; `resolve` threaded
  through `block_prep`/`prep_blocks`/`prep_get`/`recognize_quartet_moves`/`quartet_ok`.
- `boot/compiler/codegen/mutvec_aggregate_rewrite.tw` — `emit_carrier_binder`
  `.ARecordUpdate` carrier arm.
- `boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_sugar.tw` +
  `boot/tests/suites/mutvec_call_suite.tw` — sugar-form route + boxed-op tests.
- `examples/performance/awfy/twinkle/boxed_record_permute.tw` — benchmark switched
  to idiomatic sugar.
