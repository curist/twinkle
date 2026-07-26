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

## The central axis (revised after Task 1's measurement)

**Task 1 measured the reality and it changes the framing.** The axis is **candidate *kind***,
not carrier *arity*:

- **Whole-return carrier** (existing): `ret = .MayAliasParams([k])` — the whole return *is* one
  param. Handled by `optimistic_hypothesis`/`variant_valid` today.
- **Aggregate-field carrier** (new, this plan): `ret = .OwnedFresh` with `ret_paths` naming
  param carriers. Needs the OwnedFresh-preserving hypothesis (Task 2) and `OwnedFresh`-gated
  validation (Task 3), dispatched by the driver (Task 4).

**`merge_targeted` is a SINGLE-carrier aggregate `{p1}`, not `{p1,p3}`** (Task 1 finding):
`out := next`(p1) is updated in place via `out[k]=`, but `next_locked` (from p3) is rebuilt with
pure `insert_sorted` — *no* in-place site — so `param_has_inplace_site` correctly excludes p3.
**No 2+-carrier aggregate exists anywhere in the compiler** (all 28 detected candidates are
single-carrier). So:

- The immediate `merge_targeted` flip needs only **single-carrier `OwnedFresh` support** — the
  KIND dispatch, not the arity generalization.
- The N-carrier machinery (multi-req key, `variant_valid_key` over a set, the `member_pidx`→full-
  key driver) is kept for **generality/correctness** but has **no real customer**; it is exercised
  only by the synthetic 2-carrier fixture in Task 5, not by boot/main. Do not treat "N-carrier
  soundness" as a live risk for the self-host flip.

**Still-load-bearing soundness note:** a carrier is valid only if it stays `Consumed` with an
in-place path AND its return field stays `.OwnedFromParam(k)` under an `OwnedFresh` return
(Task 3's `OwnedFresh` gate); caller-side recovery only applies `ret_paths` under `OwnedFresh`.
Where multiple carriers exist (fixture only), all-or-nothing retraction is used and is sound
because the copy-carrier shape gives each carrier an independent `out_i := param_i; out_i[k]=…`.

---

## Ground truth (verify before starting)

```bash
# merge_targeted__ returns a fresh aggregate (MergeOut) whose f0 (map) is an in-place
# carrier from p1 (out := next; out[k]=…); its f1 (locked) is from p3 but rebuilt via pure
# insert_sorted (NO in-place site), so the SINGLE carrier is {p1}. Its dict$set is still
# persistent — the target to flip.
target/twk ir boot/main.tw --cfg \
  | grep -A1 -E '^fn merge_targeted__Int' | grep -E '^fn |summary:'
# → summary: p0=Borrowed p1=Published p2=Borrowed p3=Published … ret=fresh
#            ret_paths=.f0=from(p1) .f1=from(p3)   (f1=from(p3) is returned, not a carrier)

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

## Task 1 — Aggregate-carrier candidate detection ✅ DONE (commit `7d6b76f3`)

**Files:** Modify `boot/compiler/summary.tw` (`candidate_variants`).

**Outcome:** `aggregate_carrier_params` + the aggregate branch in `candidate_variants` landed.
28 aggregate candidates detected in boot/main (25 `{p0}`, 3 `{p1}` = the merge_targeted
monomorphs). Self-host byte-identical, boot suite still at the one known-red marker (the new
candidates are inert — the current MayAliasParams driver retracts them; Tasks 2–4 make them
validate). **Gate note for future tasks:** `compute_variants` runs on the `twk ir --cfg` path
(`boot/commands/ir.tw:62`), **not** `--census --sites` — instrument/observe candidates via
`target/twk ir boot/main.tw --cfg 2>LOG`.

Steps below are retained as the record of what was implemented.

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

- [ ] **Step 3: Gate on candidate *generation* via temporary instrumentation (no committed
  test at this task).** `--cfg` only renders variants that survive into `VariantSummaryTable`;
  after Task 1 the still-singular driver (`run_scc_variants`, Task 4) will retract or mis-seed
  the `{p1,p3}` candidate, so a `--cfg` grep shows nothing — the wrong gate. And
  `candidate_variants`/`render_variant_key` are **private `fn`s** in `summary.tw`, so a suite in
  another module cannot call them directly — do not claim a committed unit test here. Instead add
  a **temporary** `eprintln("[cand] ${render_variant_key(v)}")` at the point `candidate_variants`
  appends the aggregate candidate, rebuild, and confirm it prints for `merge_targeted__`:

```bash
make bundle-cli 2>&1 | tail -3   # Fixed point reached: stage3 == stage4
# compute_variants runs on the --cfg path (ir.tw:62), NOT --census. Capture stderr:
target/twk ir boot/main.tw --cfg >/dev/null 2>/tmp/cands.log
grep -o '\[cand\] unique:[^ ]*' /tmp/cands.log | sort | uniq -c
# → 3× [cand] unique:p1   (the merge_targeted monomorphs; 25× unique:p0 for other builders)
```
  **Remove the `eprintln` before committing.** Durable coverage comes at Task 5 via the fixture
  census once the driver (Task 4) threads the full key; the *visible-in-`--cfg`* and *census-flip*
  gates are deferred to Task 4.

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
    if in_set(cset, i) {
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

- [ ] **Step 2: Confirm `summarize_variant` already seeds the whole key (no change expected).**
  It is defined in `boot/compiler/ownership.tw` (`pub fn summarize_variant`, ~`:7645`; `summary.tw`
  imports it). It **directly loops `for req in key { unique_seed[f.params[req.param].id] = true }`**
  then calls `summarize_seeded` — so it already seeds every param in the key, not just `unique[0]`.
  (`unique_seed_for_variant` is a *separate* private helper in `summary.tw` used for
  rendering/reachability, not by `summarize_variant`.) No edit needed here; this step just records
  that the seeding side is already multi-param — the singular bottleneck is the driver (Task 4).

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

- [ ] **Step 1: Add a per-carrier-set validator for the AGGREGATE case only.** `variant_valid_key`
  requires `OwnedFresh` and validates every carrier; it is **not** a replacement for the existing
  `variant_valid(s, k)` (the whole-return `.MayAliasParams([k])` path). Both coexist — Task 4
  dispatches between them per member. All carriers must stay in-place AND still be returned as
  their aggregate field.

```tw
// A carrier is still returned iff the return is a FRESH aggregate (OwnedFresh) whose
// ret_paths still carry k via OwnedFromParam. The OwnedFresh gate is load-bearing: caller-side
// recovery (transfer_summarized_call, result_ok = OwnedFresh) ONLY applies ret_paths under an
// OwnedFresh return, so a widened `ret=Shared` with stale/surviving ret_paths must NOT validate.
fn carrier_returned_owned_fresh(s: Summary, k: Int) Bool {
  case s.ret {
    .OwnedFresh => {
      for rp in s.ret_paths {
        case rp.own {
          .OwnedFromParam(j) => if j == k { return true },
          .OwnedFresh => {},
        }
      }
      false
    },
    _ => false, // MayAliasParams / Shared: not an aggregate-carrier return
  }
}

// A converged aggregate variant survives iff EVERY carrier param still (a) has a non-empty
// in-place path and (b) is returned as an OwnedFresh aggregate field.
fn variant_valid_key(s: Summary, key: vid.UniqueKey) Bool {
  if key.len() == 0 { return false }
  // Aggregate variants require a fresh-aggregate return; a whole-return single carrier uses
  // the existing variant_valid(s, k) path instead.
  case s.ret {
    .OwnedFresh => {},
    _ => return false,
  }
  for req in key {
    k := req.param
    in_place_ok := k >= 0 and k < s.params.len() and !s.params[k].in_place_paths.is_empty()
    if !(in_place_ok and carrier_returned_owned_fresh(s, k)) {
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

- [ ] **Step 1: Replace `member_pidx: Dict<Int, Int>` with the full key, dispatching by candidate
  kind so the existing whole-return path is preserved.** Use the existing
  `member_key: Dict<Int, vid.VariantId>` as the source of truth; derive the carrier list from
  `member_key[m].unique` via `carriers_of(v) = collect req in v.unique { req.param }`. The
  candidate kind is fixed by the member's **generic** return: `.OwnedFresh` ⇒ aggregate,
  `.MayAliasParams([k])` ⇒ whole-return. At each site the driver currently calls
  `optimistic_hypothesis(gs, k)` / `variant_valid(conv, k)` (the seeding loop ~`:806`, the
  prev-fallback ~`:829`/`:850`, the validation loop ~`:852`, and the publish-survivors loop
  ~`:872`), dispatch:

```tw
  is_agg := case table_get(generic, m).ret {
    .OwnedFresh => true,
    _ => false,   // .MayAliasParams / .Shared → whole-return (or no) variant
  }
  // seed:
  seeded := if is_agg {
    optimistic_hypothesis_multi(table_get(generic, m), carriers_of(member_key[m]))
  } else {
    optimistic_hypothesis(table_get(generic, m), seed_param_of(member_key[m]))
  }
  // validate:
  ok := if is_agg {
    variant_valid_key(conv, member_key[m].unique)
  } else {
    variant_valid(conv, seed_param_of(member_key[m]))
  }
```
  Keep the "active member set is nonempty" guard by testing `member_key.keys().len()` instead of
  `member_pidx`. **Do not blanket-replace `variant_valid` with `variant_valid_key`** — that would
  break whole-return variants (they are not `OwnedFresh`).

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
(`dict$set_in_place … MutableSelected`), keyed to the single-carrier `unique:p1` variant. If it
stays `persistent(aliased shell)`, dump `--cfg` for the variant summary and check the carrier
against `variant_valid_key` / the caller's `arg_unique` for p1.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/summary.tw
git commit -m "ownership: drive the SCC variant fixpoint over the aggregate carrier key

merge_targeted__ now selects an in-place dict decision for its {p1} OwnedFresh aggregate
carrier when the caller passes p1 Unique. Driver dispatches by kind (OwnedFresh vs
MayAliasParams) and threads the full key (multi-carrier path exercised by the Task 5 fixture)."
```

---

## Task 5 — Micro-fixture + behavioral gates

**Files:** Add a fixture mirroring the `phase8d_*` dict fixtures; verify.

- [ ] **Step 1: Add two fixtures** under `boot/tests/fixtures/sound_uniqueness/`, mirroring the
  `phase8d_*` dict fixtures, each `.test(`ed in `boot/tests/suites/mutable_produce_suite.tw` via
  `selected_decision_count(produce_for("<fixture>"), "<fn>", "dict_set") > 0` (see the existing
  `phase8d_dict_merge_targeted_min` test at ~`:351`):
  - **Single-carrier** (the real shape — matches `merge_targeted`): `out := a; out[k] = v;
    return .{ f0: out, f1: some_other }`, called once with `a` fresh (Unique). This is the
    capability that actually flips the compiler.
  - **Synthetic 2-carrier** (exercises the multi-carrier code, which has NO real customer in
    boot/main): `a2 := a; a2[k] = v; b2 := b; b2[k] = w; return .{ f0: a2, f1: b2 }`, called with
    both dicts fresh. Without this fixture the `member_pidx`→full-key path and
    `variant_valid_key`'s N-carrier loop are untested.
  The `merge_targeted__` census flip (Task 4) is the integration gate; these fixtures are the
  unit-level coverage.

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
the `run_fixpoint` beneficiary (the E-DRY follow-up plan).

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
  the remaining tracked-red target (it lands via the E-DRY beneficiary follow-up), and let the
  `merge_targeted__` assertion pass. Re-point the comment to the E-DRY follow-up.

- [ ] **Step 2: fmt + suite.**

```bash
target/twk fmt boot/tests/suites/mutable_produce_suite.tw
target/twk test 2>&1 | tail -3
```
Expected: exactly one known failure remains — the `run_fixpoint` in-place assertion — until
the E-DRY follow-up lands. Exit 1 is expected; the failing test name must be the
`run_fixpoint` one.

- [ ] **Step 3: Commit.**

```bash
git add boot/tests/suites/mutable_produce_suite.tw
git commit -m "test: land merge_targeted in-place marker; run_fixpoint half awaits E-DRY"
```

---

## Follow-up (separate plan): `run_fixpoint` beneficiary via E-DRY

> **Not task-by-task ready — a sketch, not an executable task.** Do NOT attempt this from
> the outline below; author a dedicated plan (`docs/plans/fixpoint-edry-beneficiary.md`)
> **after Tasks 1–6 are green.** Note `fixpoint_iterate` would be the **first real
> multi-carrier customer** (14 carriers) — the multi-carrier machinery built in Tasks 4–5 is
> exercised for real here, not just by the synthetic fixture. The full
> `fixpoint_iterate` signature is large (14 map params plus `run_fixpoint`'s read-only
> context — `blocks`, `succ`, `preps`, `table`, `b`, `sem`, `suppress`, `cc_suppress`,
> `unique_seed`, `seeds`, `dirty`, `all_dirty`, `label`) and its exact shape depends on how
> Tasks 1–6 land, so pinning it now would be a placeholder, not a plan.

**Idea.** Once aggregate-field owned variants exist, extract a helper
`fixpoint_iterate(<14 maps> , <read-only ctx>) FixState` from `run_fixpoint` that updates
each map param in place and returns them all in the `FixState` — the exact
`ret=OwnedFresh` + per-field carrier shape this plan enables. The two statically-cold
callers (`ownership.tw:6249`, `:6370`) pass fresh `Dict.new()` maps (Unique + last-use), so
the 14-carrier variant is selected and the map writes flip in-place. This is the 14-carrier
stress case for the capability proven on `merge_targeted` (single-carrier) plus the synthetic
2-carrier fixture.

**Acceptance target for that plan:** `fixpoint_iterate`'s `dict_set` census rows flip to
`true`, the last (`run_fixpoint`) half of the tracked marker goes green, `TWINKLE_FIXVERIFY`
clean, self-host stable, and a real `summary:roots run` improvement — after which both this
plan and `docs/plans/fixpoint-map-inplace.md` move to `docs/plans/archive/` (rows removed
from `docs/plans/README.md`), per the house rule.

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
- Revised axis (Task 1 finding): the distinction is candidate *kind* (`OwnedFresh` aggregate vs
  `MayAliasParams` whole-return), not carrier arity. `merge_targeted` is single-carrier `{p1}`;
  no 2+-carrier aggregate exists in boot/main, so the multi-carrier machinery is covered by a
  synthetic fixture (T5) and first used for real by the E-DRY `fixpoint_iterate` (14 carriers).
- Types are consistent: `aggregate_carrier_params` → `Vector<Int>`; `optimistic_hypothesis_multi`
  / `variant_valid_key` take the carrier list / `vid.UniqueKey`; `carriers_of(v)` bridges the
  `VariantId` to the list. `carrier_returned_owned_fresh` reuses the `ReturnPathOwn.own` /
  `.OwnedFromParam(k)` shape from `ownership.tw:74`.
