# Aggregate-Field Owned Variants — Implementation Plan

> **ARCHIVED (2026-07-27).** Premise disproven (see STATUS below) and the owned-variant vtable has
> no codegen consumer. The idea belongs to [`sound-uniqueness/`](../sound-uniqueness/README.md)
> codegen **Phase 8G** (ownership-specialized function variants), not a standalone plan. Kept for the record.

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

---

## ⚠️ STATUS (2026-07-27): core premise DISPROVEN — Tasks 4–6 blocked on a missing consumer

> A review of the committed Tasks 1–3 traced the actual detection and emission paths and
> found the plan's premise is wrong, and — more fundamentally — that the entire owned-variant
> apparatus it generalizes **has no codegen consumer today**. This section is the reviewable
> writeup. Every claim cites a `file:line` you can grep at HEAD; please grill/fact-check.

### Finding 1 — `merge_targeted` was never a candidate (Task 1's verification was wrong)

Candidate detection gates each carrier param on `param_has_inplace_site`
(`boot/compiler/summary.tw:662`), whose scan `scan_inplace_op` (`:636`) sets `found = true`
**only** on `.ARecordUpdate` (record field update). Every other op — including `.ACall`,
which is what a dict `out[k]=v` lowers to (`dict$set`) and a vector `xs[i]=v` (`VECTOR_SET`) —
falls through `_ => st` (`:655`) and is invisible. Therefore:

- `aggregate_carrier_params(merge_targeted, …)` (`:690`) returns **`carriers=0`** — measured
  directly with a temporary `eprintln`. merge_targeted has a fresh-aggregate return
  (`ret=fresh ret_paths=.f0=from(p1) .f1=from(p3)`) but **no ARecordUpdate site**, so it is
  **not** proposed as a candidate.
- Task 1's commit message (`7d6b76f3`: "merge_targeted is one of them … as a SINGLE carrier
  `{p1}`") is **incorrect**. The 25 record-based `{p0}` candidates are real; the "3× `{p1}`
  merge_targeted monomorphs" never existed. The `[cand] unique:p1` lines the Task 1 step
  claimed to see do not reproduce.

**Repair for detection** (built and verified in the investigation, then reverted): extend
`scan_inplace_op` to also set `found` on a COW `.Update` builtin whose `cow_base_arg` is a
derived collection — recognized via `call_info(sem, fid).effect == .Update` + `cow_base_arg`
(`boot/compiler/opt/semantics.tw:234`, `:27`), mirroring how `ownership.tw:7397` already
dirties a COW base at the shell. With this, merge_targeted correctly detects `carriers=1`.
This repair is necessary but **not sufficient** (Findings 2–3), so it was not committed.

### Finding 2 — even seeded Unique, `merge_targeted`'s `out` is aliased at the mutation

The per-site verdict is `shell_verdict` (`boot/compiler/ownership.tw:4625`): a dict/vector
update emits in-place **iff the base's ownership fact is `.Unique`** at the mutation, else
`_ => "persistent(aliased shell)"` (`:4635`). merge_targeted (`ownership.tw:5180`) does:

```tw
out := next                                 // out aliases p1's backing
…
next_x := lat_get(next, k, default_value)   // reads `next` AFTER the loop's out[k]= writes
out[k] = join(old_x, next_x)                // in-place write to the SHARED backing
```

The forward verdict pass treats a param as Unique only if `seed_param_indices`
(`boot/compiler/codegen/ownership_verdicts.tw:328`) contains it — a set that is
**consumed-path targets (`target_params`, `:311`) ∪ copy-carrier sources
(`structural_seed_params_for_borrow_effects`, `ownership.tw:1256`)**. p1 *is* seeded (as a
copy-carrier source — the census reason literally reads `borrow-effect copy-carrier source`),
yet `out` still resolves to Shared at the write because the post-copy `lat_get(next, …)` read
keeps `next` alias-live. Hence `persistent(aliased shell)` — the "documented boundary."

A **body rewrite** (hoist the `int_keys_union(old.keys(), next.keys())` read above
`out := next`, then read values via `out` instead of `next`) is behavior-preserving (keys are
unique, so `out[k]` is untouched until its own iteration) and removes the post-copy `next`
read. It was tried in the investigation and, on its own, **still did not flip** — because with
the copy-carrier read gone, p1 also drops out of `structural_seed_params_for_borrow_effects`,
and the *base* summary never marks p1 Consumed (base analysis does not seed params Unique), so
p1 is seeded Unique by **no** path. This is what makes Finding 3 the gating prerequisite.

### Finding 3 — the owned-variant vtable has **no codegen consumer** (this is the real blocker)

`compute_variants` (`boot/compiler/summary.tw`) produces the `VariantSummaryTable`. The only
non-test reference to it in the whole tree is the **diagnostic** `twk ir --cfg` command:

```
boot/commands/ir.tw:62:  variants := summary.compute_variants(owned.view, b, s, owned.table)
```

The emission path never touches it: `compute_artifacts`
(`boot/compiler/codegen/ownership_verdicts.tw:511`) computes only the **base** summary
(`summary.compute`, `:517`) + `uniform_entry_seeds` (`:518`) and runs
`analyze_with_summaries_and_entry_seeds` (`:520`) — no variant table in scope. `mutable_produce`
/ `mutable_select` / `prepare_backend` likewise never mention it (verified by grep across
`boot/compiler/codegen/` and `boot/compiler/backend/`).

**Consequence:** the entire owned-variant apparatus is **diagnostic-only** right now. No
owned-variant — validated or not, whole-return or aggregate, record or dict — can flip *any*
emitted site, because nothing downstream reads the vtable. This is the unbuilt "**2c —
codegen-handoff**" item from `docs/plans/sound-uniqueness/`. It means the summary-side
generalization Tasks 4–6 build is inert regardless of correctness.

### Revised design — to actually flip `merge_targeted` you need ALL THREE, in this order

| # | Change | Where | Why it's required | Soundness obligation | Verify |
|---|---|---|---|---|---|
| **1. Codegen handoff** *(missing — the gate)* | Make the verdict/emission path consume validated owned-variants: for each published variant, emit a **separate, caller-guarded** variant function, and in **that** function's forward verdict pass union the variant's carrier params into the Unique seed set (extend `seed_param_indices` / thread the vtable into `compute_artifacts` → `analyze_with_summaries_and_entry_seeds`). The **base** function keeps its persistent verdict. | `codegen/ownership_verdicts.tw:328,511,518,520`; `mutable_produce`/`mutable_select` dispatch by `variant_key` (the field already exists, set `.None` today); call-site selection already recovers `arg_unique` via `transfer_summarized_call`. | Without a consumer the vtable is inert (Finding 3). This is the ONLY component that turns a validated variant into a real in-place site. | Seeding a carrier Unique is sound **only inside the guarded variant** — a variant body may assume its key params are Unique because the variant is emitted/selected exclusively when the caller passes them Unique + last-use. The base body must stay persistent. `TWINKLE_FIXVERIFY` must stay clean (note: the pre-existing `analyze:unique_analysis_diags` mismatch is the tracked-red baseline, present even before Task 1 — measure the delta, not the absolute). | A fixture where caller passes a fresh dict Unique: `selected_decision_count(produce_for(fx), fn, "dict_set") > 0`. |
| **2. Body rewrite of `merge_targeted`** | Hoist `keys := int_keys_union(old.keys(), next.keys())` above `out := next`; read `next_x := lat_get(out, k, …)` instead of `lat_get(next, k, …)`. | `ownership.tw:5180` | Even a Unique seed gives `persistent(base still live)` while `next` is read after `out := next` (Finding 2). The rewrite makes `out` genuinely Unique at the write. | Behavior-preserving: union keys are unique, so `out[k]` is unread/unwritten until its own iteration; `out == next` backing at that point. Must be covered by existing `merge_targeted`/fixpoint tests + the boot suite (self-host uses this fn). | Self-host `stage3 == stage4`; boot suite unchanged except the target marker. |
| **3. Summary generalization** *(this plan)* | Aggregate-field candidate detection (Task 1) **+ the dict/vector-carrier repair from Finding 1** + kind-dispatched SCC driver (Task 4) + `variant_valid_key` (Task 3). | `summary.tw:636,662,690,713` + driver | Produces the *validated variant* that #1 consumes and that #2 makes consumable. Load-bearing only once #1 exists. | Validation (`variant_valid_key`) already gates: carrier stays Consumed w/ in-place path AND returned `OwnedFromParam` under `OwnedFresh`. | `twk ir --cfg` shows the published variant; then #1's fixture flips. |

**Sequencing correction:** this plan built #3 first, but **#1 is the prerequisite** and does not
exist. #3 and #2 change nothing observable until #1 lands. Recommended: pause Tasks 4–6, author
`docs/plans/owned-variant-codegen-handoff.md` for #1 with body-rewritten `merge_targeted` as its
first customer, and fold #3's detection repair (Finding 1) into that plan so the first end-to-end
flip is provable.

### Points worth grilling (open questions for the reviewer)

- **Is a per-variant verdict pass the right shape, or should `seed_param_indices` just union the
  vtable's carriers for the base pass?** The latter is simpler but unsound unless every caller is
  uniform-Unique — the "mixed-caller guard" (`ownership_verdicts.tw:325`) already exists for the
  copy-carrier case; does it extend to variant carriers, or must the variant be a distinct emitted
  function? (Leaning: distinct function, because a non-uniform caller must still hit the base.)
- **Does emitting a second function per variant interact with monomorphization** (one clone per
  (func, type-args))? A variant is a third axis (func, type-args, unique-key) — dedup/interning is
  in `variant_id.tw` but the emission/naming path is unbuilt.
- **Is merge_targeted even called Unique + last-use by anyone?** If no caller passes `next` fresh,
  the variant is never selected and the whole chain is moot for this specific function — worth
  confirming against `run_fixpoint`'s call sites before investing (the E-DRY beneficiary assumed
  the cold callers pass fresh `Dict.new()`).
- **The pre-existing `analyze:unique_analysis_diags` fixverify mismatch** — is it the
  merge_targeted/run_fixpoint boundary, or an unrelated red? #1's acceptance must define the
  expected post-flip fixverify state, not just "clean."

---

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

## Task 1 — Aggregate-carrier candidate detection ✅ CODE LANDED (`7d6b76f3`), ⚠️ OUTCOME CORRECTED

**Files:** Modify `boot/compiler/summary.tw` (`candidate_variants`).

> **⚠️ The original outcome below is WRONG — see Finding 1 in the STATUS section.** The 25
> record-based `{p0}` candidates are real, but the "3 `{p1}` merge_targeted monomorphs" **never
> existed**: `param_has_inplace_site` only detects `.ARecordUpdate`, so merge_targeted's dict
> carrier scores `carriers=0` and is not proposed. Detecting it needs the COW-`.Update` repair
> (Finding 1), which is necessary-but-not-sufficient (Findings 2–3). The committed code is
> sound (it just never fires for dict carriers); the *claim* that it detected merge_targeted is
> the defect.

**Outcome (as originally recorded — retained for the record, do not trust the merge_targeted
count):** `aggregate_carrier_params` + the aggregate branch in `candidate_variants` landed.
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

## Task 2 — Multi-carrier optimistic hypothesis ✅ DONE

**Files:** Modify `boot/compiler/summary.tw` (`optimistic_hypothesis` + callers) and
`boot/tests/suites/cfg_summary_suite.tw`.

**Outcome:** `pub fn optimistic_hypothesis_multi` landed with a summary-suite regression that
pins the aggregate-preserving contract. It marks keyed carriers `Consumed paths{[]}` and preserves
`ret=OwnedFresh` plus `ret_paths`; the existing whole-return `optimistic_hypothesis` remains for
`.MayAliasParams([k])`. `summarize_variant` was confirmed to already seed every `req in key`.
Self-host reaches `stage3 == stage4`; boot suite still has exactly the one known-red marker; no
census flip yet (expected until Tasks 3–4).

- [x] **Step 1: Add a carrier-set hypothesis** that marks every carrier `Consumed`+shell and
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
  gs.params = params
  gs
}
```
Keep the existing single-`k` `optimistic_hypothesis` for the whole-return path, or make it
`optimistic_hypothesis_multi(gs, [k])` returning `.MayAliasParams([k])` — pick one; do not
leave two divergent seeders for the same case.

- [x] **Step 2: Confirm `summarize_variant` already seeds the whole key (no change expected).**
  It is defined in `boot/compiler/ownership.tw` (`pub fn summarize_variant`, ~`:7645`; `summary.tw`
  imports it). It **directly loops `for req in key { unique_seed[f.params[req.param].id] = true }`**
  then calls `summarize_seeded` — so it already seeds every param in the key, not just `unique[0]`.
  (`unique_seed_for_variant` is a *separate* private helper in `summary.tw` used for
  rendering/reachability, not by `summarize_variant`.) No edit needed here; this step just records
  that the seeding side is already multi-param — the singular bottleneck is the driver (Task 4).

- [x] **Step 3: Rebuild (no census change expected yet — driver is Task 4).**

```bash
make bundle-cli 2>&1 | tail -1
```
Expected: self-host green. (Behavioral flip comes after Tasks 3–4 wire the driver.)

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/summary.tw boot/tests/suites/cfg_summary_suite.tw docs/plans/aggregate-field-owned-variants.md
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

> **⚠️ BLOCKED / INERT until the codegen handoff (#1) exists — see STATUS Finding 3.** This
> driver change was implemented and self-host-verified in the investigation (kind-dispatched
> seed/validate over the full `member_key`; it correctly publishes record-aggregate variants
> the old MayAliasParams driver retracted). It was **reverted** because the published vtable has
> no codegen consumer, so it flips nothing emitted. Re-apply it only alongside #1, and gate on a
> real `selected_decision_count` flip — **not** the `--census --sites merge_targeted` row (that
> renders the base body, which stays persistent regardless; see Finding 2). The Task 4 census
> gate as originally written checks the wrong thing.

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
