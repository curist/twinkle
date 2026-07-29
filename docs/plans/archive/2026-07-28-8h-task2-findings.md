# Phase 8H — Task 2 implementation findings (PAUSED for plan revision)

Status: Task 1 landed (commit `analysis: expose structured field-backing verdicts`).
Task 2 implemented as a WIP commit on branch `codegen-8h-record-backed-field-collections`
but **paused**: it passes the 3 new field-path tests yet breaks 4 existing 8G tests,
exposing two design gaps that need the plan revised before continuing.

## What was built (works, keep the ideas)

1. **Path-granular publication/selection** (`summary.tw`): `variant_for_paths`
   re-keys published variants from the converged `in_place_paths`;
   `select_variant_for_arg_paths` + `variant_paths_satisfied` +
   `PathSet.contains`; shell-only `variant_args_satisfied`/`select_variant_for_args`
   kept as compat wrappers.
2. **Per-arg proven paths** (`ownership.tw`): `SitedCallUniq.arg_paths` +
   `call_arg_paths` (shell from whole-arg last-use, plus depth-1 reference field
   paths from `atom_field_own`).
3. **Reachability/routing** switched to `arg_paths`; `reachable_variant_keys_for_test`
   hook added.
4. **Reference-typed field filtering** (the sound fix for the primitive-field bug):
   - `OptimizerSemantics.ref_fields: Dict<Int, Dict<Int, Bool>>` +
     `with_ref_fields` + `sem_field_is_primitive` (conservative default: only an
     explicit "primitive" entry drops a field path; unknown stays kept).
   - `resolver.build_ref_field_table(env)` — structural: non-scalar `MonoType` = GC
     reference; enumerated from `env.types` records.
   - Gate in `ownership.transfer_flow` `.ARecordUpdate`: only add `[.f]` to the
     dirty set when the field is not proven primitive.
   - Env-aware `sem` wired into the two test helpers that assert field content
     (`variants_of`, `render_entry`).

## Gap 1 — primitive fields (RESOLVED, keep)

`transfer_flow` added every record-update field to `in_place_paths` unconditionally,
so `setb`'s `Int` field `b` became a variant requirement no shell-only caller could
prove. Fixed by the reference-typed filter above. The ownership-only signals
(`field_own`/`field_backing_moved`) are **not** a sound filter — they are strictly
narrower than "reference-typed" (miss shared-value / pointer-replacement stores);
dropping such entries would relax the variant key (unsound). The type predicate is
required. Two parallel investigations converged on this; it is the correct approach.

## Gap 2 — re-keying REPLACES the shell-only variant (NEEDS PLAN DECISION)

`red_delegate_chain`: `add` does `env.types = .set(k,v)` — a Dict-in-a-field update.
Re-keying makes `add`'s only variant *require* field `[.types]` uniqueness. But:

- Field-uniqueness does **not** propagate through delegation params the way
  shell-uniqueness does. In `build → resolve → resolve_decls → resolve_one → add`,
  the intermediates receive `env` as a param with no field-ownership proof, so
  `add`'s field-path variant is **unreachable** in the generic analysis (summary
  shows `p0=Consumed paths{[],[.f0]}` but no `variant fn add` section renders).
- Worse: replacing the shell-only key **destroys the 8G shell-reuse win** that did
  compose through delegation. The variant doesn't just get stricter — it vanishes
  until Task 3's clone field-seeds exist.

A field-backed update legitimately supports two wins at different proof levels:
shell-unique caller → record-shell reuse (8G, composes); field-unique caller →
shell reuse + collection in-place (8H, needs Task 3 seeds). The plan's Task 2 Step 3
("construct the final key from `in_place_paths`", replacing) discards the first.

Likely resolution: **publish both** a shell-only and a shell+field variant per member
(`by_func` already holds multiple keys; `variant_specificity` already prefers the
strongest a caller proves). Also reconcile Task 2 Step 7's internal tension:
it says make reachability use strict `variant_paths_satisfied` **and** "must not drop
field-path variants from reachability" — those conflict under shell-only diagnostic
seeds. Reachability (for render/enumeration) likely needs to be shell-permissive
while routing stays field-strict.

## Failing tests under the current WIP (all pre-existing 8G contracts)

- `variant_specialize::setb …` — fixed by the filter once its `variants_of` is
  env-aware (already done); listed because the WIP snapshot predates a clean run.
- `cfg sound uniqueness … delegating call chain …` — Gap 2 (variant unreachable).
- `cfg sound uniqueness … mixed local update plus delegation …` — Gap 2.
- `cfg sound uniqueness … Cell-backed dict update stays conservative` — re-verify
  under the revised model.

## Verification note

`target/twk run boot/tests/main.tw` recompiles boot source, so compiler-source edits
take effect there without a rebuild. `twk ir` / `twk wat` use the bundled
`target/boot.wasm` and need `make bundle-cli` to reflect source changes.
