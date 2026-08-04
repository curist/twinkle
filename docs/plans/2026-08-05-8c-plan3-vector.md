# 8C Plan 3 (Vector Half) — Non-Empty Vector Seeds Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the builder-region pass certify and rewrite a loop that folds a **non-empty**, locally-bound `Vector<T>` accumulator (`acc := base; for x in xs { acc = acc.append(x) }`), lowering it to `vector$builder_from(base)` → `builder_push*` → `builder_freeze` instead of leaving it as a persistent `Vector.append` loop.

**Architecture:** This is the vector sibling of the already-landed Plan 3 string half. The detector (`builder_region_detect.tw`) currently accepts only **empty** vector seeds (`acc := []` → `vector$builder_new`) and non-empty **string** seeds; it rejects non-empty vector seeds, which then surface as rejected "re-used accumulator" candidates via the refold scan. We relax `seed_family` to accept a non-empty `Vector<_>`-typed local/param seed, and thread a `from_base: Bool` flag from detection through the decision record so the rewrite chooses `builder_from(base)` (copies the seed into a private builder) over `builder_new` (fresh empty builder). The frozen accumulator stays boxed (typed routing is the separate Plan 4).

**Tech Stack:** Boot compiler (self-hosted Twinkle in `boot/`), pure-ANF detector + ANF→ANF rewrite, `target/twk` CLI, boot test suite.

## Why this is sound (the gate is already verified)

`vector$builder_from` resolves to `builder_from_fn` (`boot/compiler/codegen/runtime/arr.tw:5379`), which allocates a **fresh** `rt_BF`-sized tail array (`ArrayNew` at `:5399`) and `ArrayCopy`s the base's tail into it (`:5414`); the boxed `builder_push` (`builder_push_fn` at `arr.tw:5442` — note `:2758` is the unrelated *typed* `pvec_builder_push_fn`) does its in-place `ArraySet` only on that private tail, and when the tail fills it calls `promote_full_tail`, which is persistent path-copy (it never mutates the shared trie root). So seeding a builder from a non-empty base **never mutates any node reachable from that base** — exactly like the string half, no ownership/uniqueness proof is required. The structural conditions (single unconditional fold, no interior observation/publication before the freeze, no self/chunk alias) are unchanged and still enforced by `find_region`/`scan_main_arm`, independent of seed kind.

## Global Constraints

- **Do not change empty-vector-seed behavior.** Empty `acc := []` regions must keep emitting `vector$builder_new` (never `builder_from`); the existing empty-vector tests and boot codegen for them must stay byte-identical.
- **After editing any `.tw` file:** run `target/twk fmt <file>` then `target/twk lint boot/main.tw`. The formatter is idempotent.
- **Test commands:** `make boot-test` (boot suite) for detector/rewrite/round-trip tests; `make bundle-cli` for the self-host fixed point.
- **Heavy verification runs strictly sequentially** — never run `make bundle-cli`, `make boot-test`, and `make rust-test` concurrently or backgrounded.
- **Commit messages:** short imperative subject + what/why/how body; no line/count/test-count metrics; no `Co-Authored-By` unless actually correct for the session.
- **Backend note (already satisfied — no task needed):** `boot/compiler/backend/repr_assign.tw:52` already registers `vector$builder_from` as a creator whose result slot erases to `OpaqueAnyref`, so binding the builder handle into the `builder_local` slot is verifier-valid. (The string half had to *add* its `string$builder_from` entry; the vector one is already present.) A link smoke test in Task 2 guards this.

---

### Task 1: Make the builder-init choice data-driven (`from_base` threading) — behavior-neutral

Add a `from_base: Bool` flag carried from the detector's seed classification through the decision record, and switch the rewrite from keying on `family == "string"` to keying on `from_base`. This changes no certifications and no emitted code (string seeds keep `from_base: true` → `FromBase`; empty vector seeds keep `from_base: false` → `New`); it only makes the New-vs-FromBase decision explicit data so Task 2 can flip it per region.

**Files:**
- Modify: `boot/compiler/builder_region_detect.tw` (`SeedFamily` struct ~line 38, `RegionCandidate` struct ~line 25, `seed_family` ~lines 292-313, the primary `RegionCandidate.{` at ~line 459, the refold `RegionCandidate.{` at ~line 593)
- Modify: `boot/compiler/codegen/builder_region_produce.tw` (`BuilderRegionDecision` struct ~line 23, `allocate_locals` `BuilderRegionDecision.{` at ~line 265)
- Modify: `boot/compiler/codegen/builder_region.tw` (`rewrite_one_decision` `init` choice at ~lines 309-313)
- Modify: `boot/tests/suites/builder_region_suite.tw` (the three hand-built literals: `fu1_candidate` ~line 106, `mk_candidate` ~line 124, the stale `BuilderRegionDecision.{` ~line 760)

**Interfaces:**
- Produces: `SeedFamily.{ push_id: FuncId, family: String, from_base: Bool }`; `RegionCandidate` gains `from_base: Bool`; `BuilderRegionDecision` gains `from_base: Bool`. Task 2 consumes `from_base` in `seed_family`; the rewrite consumes `d.from_base`.

- [ ] **Step 1: Add the field to the three structs and update `seed_family` (all existing arms, no new seed accepted yet).**

In `builder_region_detect.tw`, change the `SeedFamily` type:

```tw
type SeedFamily = .{ push_id: FuncId, family: String, from_base: Bool }
```

Add `from_base: Bool,` as the last field of `pub type RegionCandidate = .{ ... }`.

Update `seed_family`'s existing arms to carry `from_base` (string always copies → `true`; empty vector uses a fresh builder → `false`):

```tw
  case op {
    .AInit(.ALitStr(_)) => .Some(SeedFamily.{ push_id: str_push, family: "string", from_base: true }),
    .AInit(.ALocal(v)) => case empty_arr.get(v.id) {
      .Some(_) => .Some(SeedFamily.{ push_id: vec_push, family: "vector", from_base: false }),
      .None => case acc_mono {
        .Some(.String) => .Some(SeedFamily.{ push_id: str_push, family: "string", from_base: true }),
        _ => .None,
      },
    },
    _ => .None,
  }
```

- [ ] **Step 2: Set `from_base` at both `RegionCandidate` construction sites.**

At the primary constructor in `detect_in_expr` (~line 459, the `.Some(fr) => cands.append(RegionCandidate.{ ... })`), add `from_base: sf.from_base,`.

At the refold constructor in `scan_refold_loops` (~line 593), add `from_base: false,` (refold candidates are always rejected, so the value is inert; `false` is the safe default).

- [ ] **Step 3: Thread `from_base` through the decision record.**

In `builder_region_produce.tw`, add `from_base: Bool,` as the last field of `pub type BuilderRegionDecision = .{ ... }`. In `allocate_locals` (~line 265), add `from_base: c.from_base,` to the `BuilderRegionDecision.{ ... }` literal.

- [ ] **Step 4: Switch the rewrite to key on `from_base`.**

In `builder_region.tw` `rewrite_one_decision`, replace:

```tw
      init: BuilderInit = if d.family == "string" {
        .FromBase(d.accumulator)
      } else {
        .New
      }
```

with:

```tw
      init: BuilderInit = if d.from_base {
        .FromBase(d.accumulator)
      } else {
        .New
      }
```

- [ ] **Step 5: Fix the three hand-built test literals so the suite compiles.**

In `builder_region_suite.tw`: add `from_base: true,` to both `fu1_candidate` (~line 106) and `mk_candidate` (~line 124) `RegionCandidate.{ ... }` literals (both are string candidates). Add `from_base: d.from_base,` to the stale `BuilderRegionDecision.{ ... }` literal (~line 760).

- [ ] **Step 6: Format and lint the edited files.**

Run:
```bash
target/twk fmt boot/compiler/builder_region_detect.tw boot/compiler/codegen/builder_region_produce.tw boot/compiler/codegen/builder_region.tw boot/tests/suites/builder_region_suite.tw
target/twk lint boot/main.tw
```
Expected: fmt idempotent (a second run is a no-op), lint reports no new violations.

- [ ] **Step 7: Run the builder-region suite to prove behavior-neutrality.**

Run: `make boot-test`
Expected: PASS. In particular the existing empty-vector emit tests still hold — `"rewrite emits vector builder_new/push/freeze and drops append"` (still 1 `vector$builder_new`, 0 `vector$builder_from`) and `"typed Vector<Int> accumulator loop stays boxed"` — and every string test is unchanged. No test asserts a new outcome yet.

- [ ] **Step 8: Commit.**

```bash
git add boot/compiler/builder_region_detect.tw boot/compiler/codegen/builder_region_produce.tw boot/compiler/codegen/builder_region.tw boot/tests/suites/builder_region_suite.tw
git commit -m "8c: make builder-init New-vs-FromBase data-driven via from_base

Carry a from_base flag from seed classification through the decision
record; the rewrite now chooses builder_from vs builder_new from that
flag instead of family=='string'. Behavior-neutral: string seeds stay
from_base=true (FromBase), empty vector seeds from_base=false (New).
Prepares the Plan 3 vector-half seed relaxation."
```

---

### Task 2: Accept non-empty vector local/param seeds and rewrite them via `builder_from`

Add the one `seed_family` arm that certifies a non-empty `Vector<_>`-typed `AInit(.ALocal)` seed (`acc := base`) with `from_base: true`. This is the behavior change: such regions now certify and, via Task 1's data-driven rewrite, lower to `vector$builder_from(base)`.

**Files:**
- Modify: `boot/compiler/builder_region_detect.tw` (`seed_family` inner `case acc_mono`, ~lines 303-308; and the doc comment above `seed_family`, ~lines 280-291)
- Modify: `boot/tests/suites/builder_region_suite.tw` (flip the deferred test ~lines 358-366; add detection/certification, emit, and link tests)

**Interfaces:**
- Consumes: `SeedFamily`/`RegionCandidate`/`BuilderRegionDecision.from_base` and the data-driven `init` from Task 1.
- Produces: certified vector regions for non-empty local/param seeds, rewritten to `vector$builder_from` → `vector$builder_push` → `vector$builder_freeze`.

- [ ] **Step 1: Write the failing detection + certification test.**

Add to `builder_region_suite.tw` (mirrors the string `"string param/local seed is detected and certified"` test):

```tw
    .test(
      "non-empty vector param seed is detected and certified (Plan 3 vector half)",
      fn() Result<Void, String> {
        src := "fn build(base: Vector<Int>) Vector<Int> {\n  acc := base\n  for x in [1, 2] { acc = acc.append(x) }\n  acc\n}\nprintln(build([9]).len().to_string())\n"
        try assert.equal(clean_count(src), 1)
        try assert.equal(certified_count(src), 1)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run it to confirm it fails.**

Run: `make boot-test`
Expected: FAIL — `clean_count` is `0` today (the non-empty vector seed is rejected by `seed_family` and only surfaces as a rejected refold candidate).

- [ ] **Step 3: Add the vector arm to `seed_family`.**

In `builder_region_detect.tw`, extend the inner `case acc_mono` so a non-empty `Vector`-typed local seed certifies (sound because `builder_from` copies — see the plan's soundness section):

```tw
      .None => case acc_mono {
        .Some(.String) => .Some(SeedFamily.{ push_id: str_push, family: "string", from_base: true }),
        .Some(.Vector(_)) => .Some(SeedFamily.{ push_id: vec_push, family: "vector", from_base: true }),
        _ => .None,
      },
```

Update the doc comment above `seed_family` (~lines 288-291) to state that non-empty vector local/param seeds are now accepted and lower to `vector$builder_from(acc)`, whose runtime deep-copies the seed's tail (`ArrayCopy` at `arr.tw:5414`) so no ownership proof is needed — replacing the old "deferred to Plan 3's vector half" wording. (Optional: the stale rejection-reason string at `builder_region_detect.tw:603` still says "deferred to Plan 3"; it only applies to genuine refolds now, so refreshing its wording is harmless but not required.)

- [ ] **Step 4: Run the detection test to confirm it passes.**

Run: `make boot-test`
Expected: the new detection/certification test PASSES.

- [ ] **Step 5: Flip the now-stale "deferred" test.**

Replace the existing test (currently `"non-empty vector seed stays deferred (Plan 3 vector half not landed)"`, ~lines 358-366, which asserts `clean_count == 0`) with an assertion that it is now certified. Note the seed here is a non-empty **array-literal** local (`acc: Vector<Int> = [1]`) rather than a param — it still routes through the `AInit(.ALocal)` + `Vector` mono path:

```tw
    .test(
      "non-empty vector array-literal seed is now certified (Plan 3 vector half)",
      fn() Result<Void, String> { assert.equal(
        clean_count(
          "fn m() Void {\n  acc: Vector<Int> = [1]\n  for x in [2, 3] { acc = acc.append(x) }\n  println(acc.len().to_string())\n}\nm()\n",
        ),
        1,
      ) },
    )
```

- [ ] **Step 6: Write the emit test (proves `builder_from`, not `builder_new`).**

Add (mirrors the string `"string param seed rewrites to builder calls"` test, but asserts the vector family and that `builder_new` is *not* used):

```tw
    .test(
      "non-empty vector param seed rewrites to builder_from/push/freeze (Plan 3 vector half)",
      fn() Result<Void, String> {
        a := compile_or_fail(
          "fn build(base: Vector<Int>) Vector<Int> {\n  acc := base\n  for x in [1, 2] { acc = acc.append(x) }\n  acc\n}\nprintln(build([9]).len().to_string())\n",
        )
        anf2 := builder_region.rewrite_module(a.opt, a.builtins)
        fn_b := find_func(anf2, "build")
        try assert.equal(count_calls_to(fn_b.body, a.builtins.method_id("Vector", "append")), 0)
        try assert.equal(count_calls_to(fn_b.body, a.builtins.id("vector$builder_from")), 1)
        try assert.equal(count_calls_to(fn_b.body, a.builtins.id("vector$builder_push")), 1)
        try assert.equal(count_calls_to(fn_b.body, a.builtins.id("vector$builder_freeze")), 1)
        try assert.equal(count_calls_to(fn_b.body, a.builtins.id("vector$builder_new")), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 7: Write the link smoke test (guards the `repr_assign` erasure for `vector$builder_from`).**

Add (confirms the rewritten module passes the backend verifier — the builder handle bound into the erased `builder_local` slot must not be typed `Vector<Int>`):

```tw
    .test(
      "non-empty vector seed loop links through the backend (Plan 3 vector half)",
      fn() Result<Void, String> {
        assert.is_true(
          links_ok(
            "fn build(base: Vector<Int>) Vector<Int> {\n  acc := base\n  for x in [1, 2] { acc = acc.append(x) }\n  acc\n}\nprintln(build([9]).len().to_string())\n",
          ),
        )
      },
    )
```

- [ ] **Step 8: Assert empty-vector regions are unaffected (regression guard).**

Confirm the existing `"rewrite emits vector builder_new/push/freeze and drops append"` test (empty `acc: Vector<Int> = []`) still asserts `vector$builder_new == 1` and is green — it exercises the `from_base: false` branch. No edit needed; just verify it passes in the next run. (If you want an explicit guard, add an assertion `count_calls_to(fn_m.body, a.builtins.id("vector$builder_from")) == 0` to that test.)

- [ ] **Step 9: Format, lint, run the suite.**

```bash
target/twk fmt boot/compiler/builder_region_detect.tw boot/tests/suites/builder_region_suite.tw
target/twk lint boot/main.tw
make boot-test
```
Expected: all builder-region tests PASS, including the flipped test, the new detection/emit/link tests, and the unchanged empty-vector + string tests.

- [ ] **Step 10: Commit.**

```bash
git add boot/compiler/builder_region_detect.tw boot/tests/suites/builder_region_suite.tw
git commit -m "8c: certify non-empty vector accumulator seeds (Plan 3 vector half)

seed_family now accepts a non-empty Vector-typed local/param seed
(acc := base), lowering it to vector\$builder_from(base) -> push ->
freeze. Sound without an ownership proof: builder_from deep-copies the
seed's tail (arr.tw:5414) and push/promotion are persistent, so the
base is never mutated. Regions stay boxed (typed routing is Plan 4).
Empty-seed vectors keep using builder_new."
```

---

### Task 3: End-to-end correctness (round-trip + base-unchanged), self-host fixed point, census, docs

Prove the rewrite is observationally correct on real execution (result correct AND the seed base is unmodified), confirm the boot compiler still self-hosts with the newly-claimed regions, measure the census shift, and update the tracking docs.

**Files:**
- Modify: `boot/tests/suites/builder_region_suite.tw` (a round-trip helper fn + a round-trip/base-unchanged test)
- Modify: `docs/plans/sound-uniqueness/codegen/builder-region-design.md` (Plan 3 vector bullet → LANDED)
- Modify: `docs/plans/sound-uniqueness/codegen/README.md` (orientation + roadmap: Plan 3-vector done, Plan 4 next)
- Modify: `/Users/curist/.claude/projects/-Users-curist-playground-rust-twinkle/memory/project_8c_builder_region_followups.md` and `MEMORY.md` (mark Plan 3-vector landed, Plan 4 now next)

- [ ] **Step 1: Write the round-trip + base-unchanged test.**

Add a helper fn near the top of `builder_region_suite.tw` (next to `seeded_concat`):

```tw
// A non-empty vector param seed (`acc := base`, an `AInit(.ALocal)` seed),
// exercised end-to-end by the Plan 3 vector-half round-trip test. When compiled
// by a twk that has the vector-half rewrite, `acc := base` lowers to
// vector$builder_from(base); the base value must remain observably unchanged.
fn seeded_append(base: Vector<Int>, xs: Vector<Int>) Vector<Int> {
  acc := base
  for x in xs {
    acc = .append(x)
  }
  acc
}
```

Add the test:

```tw
    .test(
      "non-empty vector seed round-trips and leaves the base unchanged (Plan 3 vector half)",
      fn() Result<Void, String> {
        base: Vector<Int> = [1, 2, 3]
        out := seeded_append(base, [4, 5])
        try assert.equal(out.len(), 5)
        try assert.equal(out[0] + out[1] + out[2] + out[3] + out[4], 15)
        // The rewrite must copy, not alias: base is still [1, 2, 3].
        try assert.equal(base.len(), 3)
        try assert.equal(base[0] + base[1] + base[2], 6)
        .Ok({})
      },
    )
```

Format + lint + run:
```bash
target/twk fmt boot/tests/suites/builder_region_suite.tw
target/twk lint boot/main.tw
make boot-test
```
Expected: PASS under the current `target/twk`. (This binary does not yet contain the rewrite, so `seeded_append` compiles persistently here — the test still passes because the persistent path is trivially correct. Step 3 re-runs it under a bundled twk that actually applies the rewrite, which is the real soundness gate.)

- [ ] **Step 2: Self-host fixed point — rebuild the CLI with the new rewrite.**

Run: `make bundle-cli`
Expected: completes and reaches a fixed point (the self-host loop's stage comparison succeeds). This is the whole-program soundness gate: the boot compiler now claims its own non-empty vector accumulator loops and compiles itself through them. A miscompile here fails the build.

- [ ] **Step 3: Re-run the boot suite under the freshly bundled twk (the real round-trip gate).**

Run: `make boot-test`
Expected: PASS. `target/twk` now contains the vector-half rewrite, so `seeded_append` is lowered to `vector$builder_from(base)` + `builder_push` when the suite is compiled — the round-trip/base-unchanged test now genuinely exercises the new path. A wrong (aliasing) `builder_from` would corrupt `base` and fail the `base.len()`/sum assertions.

- [ ] **Step 4: Measure the census shift.**

Run:
```bash
target/twk ir boot/main.tw --census --sites 2>&1 | grep -c "re-used accumulator, not a fresh seed"
target/twk ir boot/main.tw --census --sites 2>&1 | grep -E "vector\s+true\s" | grep -c "phase8c-region"
```
Expected: the "re-used accumulator" count is **lower** than the pre-change baseline of 452 (fresh non-empty vector seeds moved from that rejected bucket into certified), and certified vector builder regions (`linearly_folded = true`) increased. Record the two before/after numbers in the commit body. (Exact magnitudes depend on how many boot regions are fresh-seed vs genuine refold; both directions of change are the expected signal, not a specific target.)

- [ ] **Step 5: Update the design + tracker docs.**

In `builder-region-design.md`, change the Plan 3 vector bullet from "GO / confirmed safe" to **LANDED** with the commit hash, noting the census delta and that regions stay boxed pending Plan 4. Update the header status line if it enumerates landed plans.

In `codegen/README.md`, update the orientation block: Plan 3 (vector half) is now **landed**; the verified next step becomes **Plan 4 (typed routing)** — teach `route_typed_vec` to follow the freeze→accumulator `AAssign` copy edge (or the preferred `Let`-binding rewrite) so `Vector<Int>` accumulators unbox to flat i64. Move the "re-used accumulator ~452" row's status accordingly.

- [ ] **Step 6: Update memory.**

In `project_8c_builder_region_followups.md`, change "Next step = Plan 3 (vector half). GO." to "Plan 3 (vector half) LANDED <hash>; next = Plan 4 typed routing." Update the matching `MEMORY.md` one-line pointer.

- [ ] **Step 7: Run the Rust suite for a full green bar, then commit.**

Run (sequentially, after bundle-cli/boot-test have finished): `make rust-test`
Expected: PASS (no Rust stage0 change was made; this confirms nothing regressed).

```bash
git add boot/tests/suites/builder_region_suite.tw docs/plans/sound-uniqueness/codegen/builder-region-design.md docs/plans/sound-uniqueness/codegen/README.md
git commit -m "8c: land Plan 3 vector half + round-trip/self-host gate

Add an end-to-end round-trip test asserting the seed base is unchanged
after the builder rewrite, confirm the self-host fixed point with boot's
own non-empty vector loops now claimed, and record the census shift
(re-used-accumulator bucket down, certified vector regions up). Update
the design/README/memory to mark the vector half landed and Plan 4
(typed routing) as the next step."
```

---

## Self-Review

**Spec coverage** (against `builder-region-design.md` Plan 3 vector bullet + the earlier verification):
- "confirm `builder_push` never mutates a shared trie node" → verified in the plan's soundness section (runtime evidence with line refs); guarded at runtime by Task 3's base-unchanged assertion.
- "relax the vector seed to `builder_from(base)` (stays boxed)" → Task 2 Step 3 (seed_family arm) + Task 1 (data-driven `FromBase`); boxed-ness is inherited unchanged (the rewrite still `AAssign`-rebinds, which `route_typed_vec` does not follow — asserted indirectly by the emit test using `vector$builder_*`, not `*_i64`).
- "Test: non-empty vector accumulator round-trips and the base value is observably unchanged" → Task 3 Step 1/Step 3.
- "one runtime read + detector relax" / "no analysis dependency" → no analysis-track or `repr_assign` file is touched (the `vector$builder_from` creator entry already exists, `repr_assign.tw:52`).

**Placeholder scan:** every code step contains the exact literal to add/replace and the exact command + expected result. No "add error handling"/"similar to"/"TBD".

**Type consistency:** `from_base: Bool` is added to `SeedFamily`, `RegionCandidate`, and `BuilderRegionDecision`, and set at every constructor enumerated (detect.tw:459 primary, detect.tw:593 refold, produce.tw:265, suite.tw:106/124/760); the rewrite reads `d.from_base`. `seed_family` returns `SeedFamily?` throughout. The new match arm uses `MonoType.Vector(_)` (confirmed present at `mono_type.tw:19`). Builtin ids used in tests (`vector$builder_from`, `vector$builder_push`, `vector$builder_freeze`, `vector$builder_new`, `method_id("Vector","append")`) match the names used by the existing empty-vector emit test.

## Testing strategy (summary)

- **Detector/rewrite tests** run via `pipeline.compile_source` / `builder_region.rewrite_module` inside the suite — they exercise the freshly-compiled compiler source on each `make boot-test`, no bundle required.
- **Round-trip correctness** is only meaningful once `target/twk` itself carries the rewrite — hence Task 3 runs `make bundle-cli` then re-runs `make boot-test`. The base-unchanged assertion is the aliasing-soundness gate.
- **Self-host fixed point** (`make bundle-cli`) is the whole-program soundness gate: boot compiling itself through its own now-claimed vector regions.
- **Census** confirms the change actually moved regions from rejected → certified.

## Risks

- **A non-empty vector seed that is genuinely a re-fold (folded in an earlier loop) must stay rejected.** It does: `seed_family` only fires on the seed *binding* (`AInit(.ALocal)`/`AInit(.ALitStr)`); a re-folded accumulator's second loop has no fresh seed binding, so it is still surfaced only by `scan_refold_loops` as rejected. Covered structurally; the existing FU-2 re-fold test remains green.
- **Optimizer folding the `acc := base` seed away.** The detection/emit tests use a **param** seed (`fn build(base: Vector<Int>)`) to avoid literal copy-propagation, exactly as the landed string-param test does; the array-literal variant is covered separately by the flipped test.
- **Backend verifier rejecting the builder handle slot.** Prevented by the pre-existing `repr_assign.tw:52` `vector$builder_from` creator entry and guarded by Task 2's link smoke test.
