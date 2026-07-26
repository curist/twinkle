# Aggregate-Field Owned Variants — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use `superpowers:subagent-driven-development`
> or `superpowers:executing-plans` to implement this task-by-task. Steps use checkbox
> (`- [ ]`) syntax. This is a compiler-analysis change: the gates are census probes,
> `TWINKLE_FIXVERIFY`, the self-host fixed point, and the boot suite. Read
> `docs/plans/fixpoint-map-inplace.md` ("Primary lever") first for the diagnosis, and
> `docs/plans/sound-uniqueness/` for the owned-variant (Phase 6) design this extends.
>
> **This is the general, reusable lever** — it makes the *analysis* more precise, so any
> function that returns a fresh aggregate whose fields are owned carriers gets in-place
> mutation **at call sites that pass those inputs uniquely** (Unique + last-use). That is a
> real class (builder/transform functions returning a record or tuple of collections), but
> it is gated on the uniqueness precondition — not "every program." It supersedes the dropped
> one-off `run_fixpoint` cold/warm-split workaround.

**Goal:** Teach the owned-variant machinery to recognize a function that returns a **fresh
aggregate whose fields are owned carriers** — `ret=OwnedFresh` with
`ret_paths=.f0=from(p1) .f1=from(p3)` — as an owned-variant candidate, so that at call
sites passing those params Unique, the function's internal collection updates emit in-place
(`dict$set_in_place` / `vector$set_unsafe`) instead of persistent rebuilds.

**Architecture:** Owned variants today (Phase 6) only fire for a **single whole-return
carrier** — `candidate_variants` requires `ret_aliases_exactly_param` (`.MayAliasParams([k])`,
the whole return *is* param k). This plan generalizes the candidate → hypothesis → validate
→ select pipeline from *one whole-return param* to *a set of aggregate-field carrier params*.
The variant identity (`vid.UniqueKey = Vector<UniqueReq>`), the computed `ret_paths`, and the
caller-side multi-carrier recovery (`transfer_summarized_call`) already support multiple
carriers; the work is concentrated in three summary-side functions plus the SCC driver.

**Tech stack:** Twinkle self-hosted compiler (`boot/`), `make bundle-cli` self-host loop,
`target/twk ir --census/--cfg` ownership probes, `TWINKLE_FIXVERIFY` guard, boot test suite.

---

## The central design decision (settle before Task 1)

The real risk is **validation soundness for N independent carriers.** Two sub-decisions:

1. **One multi-param variant vs several single-param variants.** For a 2-carrier return
   `{f0=from(p1), f1=from(p3)}`, we could propose one variant keyed on `{p1,p3}` (both must
   be Unique at the call) or two variants keyed on `{p1}` and `{p3}` separately.
   **Decision (MVP): one variant requiring ALL carriers Unique.** Rationale: it matches the
   copy-carrier "all-or-nothing" nature (a caller either owns the inputs or takes the generic
   persistent path), keeps the variant count linear, and is the conservative superset. Per-
   carrier subset variants are a later precision refinement, not MVP.
2. **What "still a valid carrier variant" means after convergence.** Each carrier param must
   *independently* remain (a) `Consumed` with a non-empty in-place path, AND (b) still
   returned as its aggregate field (its `ret_paths` entry stays `.OwnedFromParam(k)`, not
   widened to Shared). If any carrier fails either, the whole variant is retracted (MVP
   all-or-nothing). Carriers do not interact through shared state in the copy-carrier shape
   (each `out_i := param_i; out_i[k]=…`), which is *why* all-or-nothing is sound — **verify
   this assumption holds on the `merge_targeted__` body before generalizing further.**

---

## Ground truth (verify before starting)

```bash
# merge_targeted__ returns a fresh aggregate with two param-field carriers, yet its
# internal dict$set is persistent — the target to flip.
target/twk ir boot/main.tw --cfg \
  | grep -A1 -E '^fn merge_targeted__Int' | grep -E '^fn |summary:'
# → summary: p0=Borrowed p1=Published p2=Borrowed p3=Published … ret=fresh
#            ret_paths=.f0=from(p1) .f1=from(p3)

target/twk ir boot/main.tw --census --sites \
  | grep -E '^merge_targeted__Int' | grep dict_set | head -1
# → merge_targeted__Int  dict_set  dict$set  dict$set_in_place  false  L… = update L… base=persistent(aliased shell) …

# Boot suite currently exits 1 on the tracked marker (asserts merge_targeted__ AND run_fixpoint).
target/twk test 2>&1 | tail -1
# → Ran 3258 tests: 3257 passed, 1 failed   (exit 1)
```

**Current single-carrier code (all in `boot/compiler/summary.tw`; re-grep line numbers):**

- `candidate_variants` (~`:689`): for each param `k`, if
  `ret_aliases_exactly_param(s.ret, k) and param_has_inplace_site(f, k)` → propose
  `VariantId{func, unique:[{k, shell}]}`.
- `optimistic_hypothesis(gs: Summary, k: Int)` (~`:721`): sets param `k` `.Consumed`+shell,
  and rewrites the return to `ret: .MayAliasParams([k]), ret_paths: []`.
- `variant_valid(s: Summary, k: Int)` (~`:743`): `!s.params[k].in_place_paths.is_empty()
  and ret_aliases_exactly_param(s.ret, k)`.
- `run_scc_variants` (~`:767`): drives the SCC fixpoint using
  `member_pidx: Dict<Int, Int>` (**one seed param per member**) and `seed_param_of(v)`
  (returns `v.unique[0].param`). Seeds via `optimistic_hypothesis(gs, k)`; validates via
  `variant_valid(conv, k)`.
- **Already set-based (no change needed):** `variant_args_satisfied` (~`:925`, iterates
  `v.unique`), `unique_seed_for_variant` (~`:973`, seeds every `req` in `v.unique`),
  `mark_site_variants` (~`:951`, key-based). `summarize_variant` seeds all key params Unique
  (verify at Task 2).
- `ReturnPathOwn = .{ via: RetVia, field: Int?, own: ReturnOwn }` (`ownership.tw:74`); a
  carrier is a `ret_paths` entry whose `own` is `.OwnedFromParam(k)`.

---

## Task 1 — Aggregate-carrier candidate detection

**Files:** Modify `boot/compiler/summary.tw` (`candidate_variants`). Test:
`boot/tests/suites/cfg_summary_suite.tw` (or the owned-variant fixture suite).

- [ ] **Step 1: Add a helper that extracts aggregate-field carriers from a summary.**

```tw
// Params that are returned as an OwnedFresh aggregate field AND have an in-place site:
// the carrier set for a candidate variant. Sorted+deduped for determinism.
fn aggregate_carrier_params(f: CfgFunction, s: Summary) Vector<Int> {
  case s.ret {
    .OwnedFresh => {
      out: Vector<Int> = []
      for rp in s.ret_paths {
        case rp.own {
          .OwnedFromParam(k) => if param_has_inplace_site(f, k) {
            out = insert_sorted(out, k)   // dedupe-sorted; a param may back >1 field
          },
          .OwnedFresh => {},
        }
      }
      out
    },
    _ => [],
  }
}
```

- [ ] **Step 2: Extend `candidate_variants` to also propose the aggregate variant.** Keep the
  existing single whole-return candidate; add: if `aggregate_carrier_params(f, s)` is
  non-empty, propose ONE variant keyed on all carriers.

```tw
    // existing whole-return single-carrier candidate loop stays …
    carriers := aggregate_carrier_params(f, s)
    if carriers.len() > 0 {
      reqs: Vector<vid.UniqueReq> = collect k in carriers {
        vid.UniqueReq.{ param: k, path: vid.shell() }
      }
      v := vid.VariantId.{ func: f.func_id, unique: reqs }
      cands = .append(v.canonicalize_variant())
    }
```

- [ ] **Step 3: Rebuild + confirm `merge_targeted__` now has a candidate.**

```bash
make bundle-cli 2>&1 | tail -3   # Fixed point reached: stage3 == stage4
target/twk ir boot/main.tw --cfg | grep -iE 'variant.*merge_targeted|merge_targeted.*unique:p1,p3'
```
Expected: a variant keyed `unique:p1,p3` appears for `merge_targeted__`. (It will not yet
flip in-place — the hypothesis/validation are still single-carrier; Tasks 2–4.)

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/summary.tw
git commit -m "ownership: propose owned-variant candidates for aggregate-field carriers"
```

---

## Task 2 — Multi-carrier optimistic hypothesis

**Files:** Modify `boot/compiler/summary.tw` (`optimistic_hypothesis` + callers).

- [ ] **Step 1: Add a carrier-set hypothesis** that marks every carrier `Consumed`+shell and
  **preserves** the aggregate return (`gs.ret` / `gs.ret_paths`) rather than rewriting to
  `.MayAliasParams([k])`.

```tw
// Optimistic seed for a variant keyed on `carriers`: each carrier Consumed with an in-place
// shell path; the return is left as the generic aggregate (OwnedFresh + ret_paths), which
// is the correct recursive-call assumption for an aggregate-returning carrier.
fn optimistic_hypothesis_multi(gs: Summary, carriers: Vector<Int>) Summary {
  cset := int_set(carriers)
  params: Vector<ParamSummary> = collect ps, i in gs.params {
    if has_int(cset, i) {
      ParamSummary.{ base_role: .Consumed, in_place_paths: vid.shell_set(), flows_to_return: true }
    } else {
      ps
    }
  }
  Summary.{ params, ret: gs.ret, ret_paths: gs.ret_paths }
}
```
Keep the existing single-`k` `optimistic_hypothesis` for the whole-return path, or make it
`optimistic_hypothesis_multi(gs, [k])` returning `.MayAliasParams([k])` — pick one; do not
leave two divergent seeders for the same case.

- [ ] **Step 2: Verify `summarize_variant` seeds all key params Unique.** Read
  `summarize_variant` (grep in `summary.tw`); confirm it drives `unique_seed_for_variant`
  (already set-based) so every param in the key is seeded, not just `unique[0]`. If it reads
  a single seed param, fix it to consume the whole key.

- [ ] **Step 3: Rebuild (no census change expected yet — driver is Task 4).**

```bash
make bundle-cli 2>&1 | tail -1
```
Expected: self-host green. (Behavioral flip comes after Tasks 3–4 wire the driver.)

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/summary.tw
git commit -m "ownership: multi-carrier optimistic hypothesis preserving the aggregate return"
```

---

## Task 3 — Multi-carrier validation

**Files:** Modify `boot/compiler/summary.tw` (`variant_valid` + a carrier predicate).

- [ ] **Step 1: Add a per-carrier-set validator.** All carriers must stay in-place AND still
  be returned as their aggregate field.

```tw
// A converged aggregate variant survives iff EVERY carrier param still (a) has a non-empty
// in-place path and (b) is still returned (its ret_paths entry stays OwnedFromParam).
fn param_still_returned(s: Summary, k: Int) Bool {
  case s.ret {
    .MayAliasParams(idxs) => idxs.len() == 1 and idxs[0] == k,
    _ => {
      for rp in s.ret_paths {
        case rp.own {
          .OwnedFromParam(j) => if j == k { return true },
          .OwnedFresh => {},
        }
      }
      false
    },
  }
}

fn variant_valid_key(s: Summary, key: vid.UniqueKey) Bool {
  if key.len() == 0 { return false }
  for req in key {
    k := req.param
    in_place_ok := k >= 0 and k < s.params.len() and !s.params[k].in_place_paths.is_empty()
    if !(in_place_ok and param_still_returned(s, k)) {
      return false
    }
  }
  true
}
```

- [ ] **Step 2: Rebuild.**

```bash
make bundle-cli 2>&1 | tail -1
```
Expected: self-host green.

- [ ] **Step 3: Commit.**

```bash
git add boot/compiler/summary.tw
git commit -m "ownership: validate multi-carrier aggregate variants over the full key"
```

---

## Task 4 — Generalize the SCC driver from single param to full key

**Files:** Modify `boot/compiler/summary.tw` (`run_scc_variants`).

- [ ] **Step 1: Replace `member_pidx: Dict<Int, Int>` with the full key.** Use the existing
  `member_key: Dict<Int, vid.VariantId>` as the source of truth; derive the carrier list from
  `member_key[m].unique`. Everywhere the driver currently calls `optimistic_hypothesis(gs, k)`
  or `variant_valid(conv, k)` (the seeding loop ~`:806`, the prev-fallback ~`:829`/`:850`, the
  validation loop ~`:852`, and the publish-survivors loop ~`:872`), pass the carrier list /
  key instead:
  - seed: `optimistic_hypothesis_multi(table_get(generic, m), carriers_of(member_key[m]))`
  - validate: `variant_valid_key(conv, member_key[m].unique)`
  where `carriers_of(v)` returns `collect req in v.unique { req.param }`.
  Keep the "active member set is nonempty" guard by testing `member_key.keys().len()` instead
  of `member_pidx`.

- [ ] **Step 2: fmt + lint.**

```bash
target/twk fmt boot/compiler/summary.tw
target/twk lint boot/main.tw
```

- [ ] **Step 3: Rebuild + census gate — the primary result.**

```bash
make bundle-cli 2>&1 | tail -3
target/twk ir boot/main.tw --census --sites \
  | grep -E '^merge_targeted__Int' | grep dict_set | head -3
```
Expected: `merge_targeted__Int`'s `dict_set` row flips to a selected in-place decision
(`dict$set_in_place … MutableSelected`), keyed to the `unique:p1,p3` variant. If it stays
`persistent(aliased shell)`, dump `--cfg` for the variant summary and check which carrier
failed `variant_valid_key`.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/summary.tw
git commit -m "ownership: drive the SCC variant fixpoint over the full carrier key

merge_targeted__ now selects an in-place dict decision for its p1/p3 aggregate-field
carriers when the caller passes both Unique."
```

---

## Task 5 — Micro-fixture + behavioral gates

**Files:** Add a fixture mirroring the `phase8d_*` dict fixtures; verify.

- [ ] **Step 1: Add a minimal 2-carrier fixture** under
  `boot/tests/fixtures/sound_uniqueness/` — a function that takes two dicts, does
  `a2 := a; a2[k] = v; b2 := b; b2[k] = w; return .{ f0: a2, f1: b2 }`, called once with two
  fresh (Unique) dict arguments. Add a `.test(` in
  `boot/tests/suites/mutable_produce_suite.tw` asserting
  `selected_decision_count(produce_for("<fixture>"), "<fn>", "dict_set") > 0`, mirroring the
  existing `phase8d_dict_merge_targeted_min` test at ~`:351`.

- [ ] **Step 2: Behavioral equivalence + self-host + suite.**

```bash
TWINKLE_FIXVERIFY=1 target/twk build boot/main.tw -o /tmp/fixverify.wasm 2>&1 | tail -5
make bundle-cli 2>&1 | tail -3
target/twk test 2>&1 | tail -3
cargo test --release ownership 2>&1 | tail -20
```
Expected: no `fixverify` mismatch; self-host `stage3 == stage4`; the new fixture test green;
the boot suite failure count **drops by one or stays at the tracked marker** (the
`merge_targeted__` half of the marker now passes — see Task 6); Rust ownership tests pass.

- [ ] **Step 3: Perf check (the compiler-speed payoff is downstream of `run_fixpoint`, but
  measure now for a baseline).**

```bash
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/stage2.wasm 2>&1 \
  | grep -E 'summary:roots|own:fixpoint'
```
Record; `merge_targeted__` alone is a modest win — the large `summary:roots` drop lands with
the `run_fixpoint` beneficiary (Task 7 / follow-up).

- [ ] **Step 4: Commit.**

```bash
git add boot/tests/
git commit -m "test: aggregate-field owned-variant fixture flips 2-carrier dict updates in-place"
```

---

## Task 6 — Retire the merge_targeted half of the marker

**Files:** Modify `boot/tests/suites/mutable_produce_suite.tw`.

- [ ] **Step 1: Update the tracked marker (~`:380`).** With `merge_targeted__` now flipping,
  its assertion should go green. Split the marker: keep `run_fixpoint`'s in-place assertion as
  the remaining tracked-red target (it lands via the E-DRY beneficiary, Task 7), and let the
  `merge_targeted__` assertion pass. Re-point the comment to the E-DRY follow-up.

- [ ] **Step 2: fmt + suite.**

```bash
target/twk fmt boot/tests/suites/mutable_produce_suite.tw
target/twk test 2>&1 | tail -3
```
Expected: exactly one known failure remains — the `run_fixpoint` in-place assertion — until
Task 7. Exit 1 is expected; the failing test name must be the `run_fixpoint` one.

- [ ] **Step 3: Commit.**

```bash
git add boot/tests/suites/mutable_produce_suite.tw
git commit -m "test: land merge_targeted in-place marker; run_fixpoint half awaits E-DRY"
```

---

## Task 7 — `run_fixpoint` beneficiary (E-DRY), as a follow-up

Now that aggregate-field owned variants exist, the returned-carrier helper that was NOT
viable before becomes viable. This is the 14-carrier stress case — do it only after Tasks 1–6
are green and the 2-carrier soundness is confirmed.

**Files:** Modify `boot/compiler/ownership.tw`.

- [ ] **Step 1: Extract `fixpoint_iterate(exits, exit_valid, …14 maps…, <read-only ctx>) FixState`**
  from `run_fixpoint` — each of the 14 map params updated in place and returned in the
  `FixState`. Keep `run_fixpoint` as the thin cold/warm map-builder + `FixRun` projector; the
  cold callers (`:6249`, `:6370`) pass fresh `Dict.new()` maps (Unique + last-use).

- [ ] **Step 2: Rebuild + census gate.**

```bash
make bundle-cli 2>&1 | tail -3
target/twk ir boot/main.tw --census --sites \
  | awk -F'\t' '$1=="fixpoint_iterate" && $2=="dict_set"{print $5}' | sort | uniq -c
```
Expected: the core map updates flip to `true` (the 14-carrier variant selected at the cold
callers). If the variant is proposed but not selected, check `variant_args_satisfied` against
the cold callers' `arg_unique` (all 14 fresh maps must be Unique + last-use).

- [ ] **Step 3: Behavioral gates (as Task 5 Step 2) + the `summary:roots run` win**, then flip
  the last marker half green and commit. Move both this plan and
  `docs/plans/fixpoint-map-inplace.md` to `docs/plans/archive/`, and remove their rows from
  `docs/plans/README.md`, per the house rule.

---

## Out of scope

- Per-carrier *subset* variants (a caller with only some inputs Unique) — a later precision
  refinement; MVP is all-carriers-Unique.
- Field-granular (`[f]`) in-place paths beyond the shell — Phase 6 Part 2, unrelated.
- The `run_fixpoint` cold/warm source split — dropped as a non-universal workaround.

## Self-review notes

- Coverage: candidate detection (T1), hypothesis (T2), validation (T3), driver (T4), fixture
  + gates (T5), marker (T6), beneficiary (T7) — the full candidate→hypothesis→validate→select
  pipeline plus proof.
- The central soundness risk (N-carrier all-or-nothing validity) is called out as the design
  decision and gated on the 2-carrier `merge_targeted__` proof before the 14-carrier case.
- Types are consistent: `aggregate_carrier_params` → `Vector<Int>`; `optimistic_hypothesis_multi`
  / `variant_valid_key` take the carrier list / `vid.UniqueKey`; `carriers_of(v)` bridges the
  `VariantId` to the list. `param_still_returned` reuses the `ReturnPathOwn.own` /
  `.OwnedFromParam(k)` shape from `ownership.tw:74`.
