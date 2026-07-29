# Return-Field Ownership Through Calls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. This plan is **soundness-critical** analysis-precision work; Task 4's aliased-negative sweep and the self-host gate are non-negotiable.

> **Revised 2026-07-29 after a spike.** The first draft framed this as a *loop-carried
> / `join_entry_field_own` back-edge* problem and proposed loop-seed/join changes.
> Investigation falsified that: a **direct** loop-carried record-field update already
> lowers in place, and a *non-loop* sequential pair of calls already fails. The real
> trigger and root cause below replace the original Tasks 1–3.

**Goal:** Let reference-typed field ownership survive a function return, so a value produced by a field-preserving call (`env = put(env, k, v)`, `s = s.insert(k)`) can prove its field path at a *subsequent* call and lower that call's record-backed collection update in place.

**Architecture:** Field-backed in-place emission already works when the receiver's field is proven owned at the call (fresh construction gives this directly, so a *first* call from a freshly-built record routes to the full-tier clone). It breaks on the *next* call because the value now comes from a return: the forward call-result recovery (`ownership.tw`) reads the callee's **generic** summary `ret_paths`, which carry no field ownership for a parameter-returning function (the generic body can't prove the field owned without a seed), and the recovery is additionally gated to `OwnedFresh` returns via `result_ok`. The fix makes return-field recovery **variant-aware** — resolve the callee's variant by the current call's argument field paths and read *that* variant's `ret_paths` — and relaxes `result_ok` for a result that may-alias only **unique** (consumed) params.

**Tech Stack:** Twinkle boot compiler (`boot/`), self-hosted. Iterate with `make quick-bundle-cli` for the boot suite; use `make bundle-cli` before any `twk ir`/`twk wat` inspection (those run the *embedded* `boot.wasm`, which `quick-bundle-cli` does not rebuild). No Rust stage0 changes.

**Design source:** `docs/plans/sound-uniqueness/codegen/README.md` §"Codegen Phase 8H"; `docs/plans/sound-uniqueness/analysis/records-fields.md`; the as-built forward ownership transfer and variant summaries in `boot/compiler/ownership.tw` / `summary.tw`.

---

## Global Constraints

- **Soundness first.** Field ownership may cross a return **only** when the callee's return exclusively owns the field — i.e. it may-alias only params that are unique (whole-argument last-use) at the call, and the callee's field-tier variant proves that field owned. Any shared/aliased source must stay persistent. The negative fixtures (`field_dict_alias_old`, `field_vector_alias_old`, `red_*`, `visit_aliased`) must remain persistent and keep old-handle observability.
- Field-path claims stay **depth-one** (`[.f]`), matching `call_arg_paths`, `ret_paths` (`field: Int?`), and `VariantId`. No `Elem`/`Val`/payload-field composition beyond what `ret_paths` already models.
- No new operation families, no runtime-helper ABI changes, no source-semantics changes.
- Byte-identical output for every fixture whose behavior does not involve return-carried field ownership (verify against the `sound_uniqueness` WAT set).
- Watch perf: the forward recovery and `call_result_fact` are on a hot, cascade-sensitive path (see commit `3631c29b` "stop shell paths cascading through call_result_fact"). Do not widen what flows through `call_result_fact`'s `dirty`; scope changes to the field-path recovery.
- After editing `.tw`: `target/twk fmt <files>` then `target/twk lint boot/main.tw`. Do not run tree-sitter tests.

## Orientation — the grounded gap (with spike evidence)

Boundary map (all `Env = .{ types: Dict, spare: Dict }`, updating `.types`; `put(e,k,v){ e.types[k]=v; e }`):

| Case | Result | Why |
|---|---|---|
| Direct in-loop `env.types[i]=v` | ✅ in-place | no call boundary; `go` proves the field directly |
| `env = put(env,0,10)` (single) | ✅ in-place, full-tier clone | arg `env` is freshly built → field owned at the call |
| `env=put(...); env=put(...)` (sequential 2) | ❌ 2nd persistent, shell tier | 2nd arg came from a return with no recoverable field |
| loop of `env=put(...)`, `Set.insert` in a loop | ❌ persistent, shell tier | iteration ≥2 is the sequential-2 case |

Root cause, two parts (both confirmed by spike):

1. **Forward recovery reads the generic summary.** In `ownership.tw` the ACall forward transfer recovers the result's `field_own` from `s.ret_paths` where `s := table.summary_get(fid)` is the **generic** summary (around line 4093). A field `ret_path` is only emitted when the *body proves the field owned* (`body.field_own_get(aid)` in the derivation around line 8391). A parameter-returning function's generic body cannot prove its field owned (no seed), so its generic `ret_paths` have **no** field entry. Only the field-tier **variant** summary (`put [unique:p0,p0.f0]`) proves and carries it — and the forward recovery never consults variant summaries.
2. **`result_ok` gates to `OwnedFresh`.** Even given a field `ret_path`, recovery is skipped unless the callee's `ret` is `OwnedFresh` (line ~4087). `put`/`insert` return their parameter, so `ret = MayAliasParams([0])`, and recovery is blocked.

**Spike result (do not re-run; recorded here):** relaxing `result_ok` to accept `MayAliasParams(ks)` when every `k` is `arg_unique[k]` compiled and kept all 3294 tests green, but the sequential/loop/Set cases still proved shell-tier — because part (1) (generic summary lacks the field `ret_path`) is the dominant blocker. So the core work is **variant-aware recovery**, with the `result_ok` relaxation as a necessary companion.

## Fix approach

In the ACall forward recovery, before reading `ret_paths`:
- compute this call's argument field paths (`pre.call_arg_paths(args, arg_unique)` is already available in the transfer),
- resolve the callee variant with those paths (`resolve(fid, arg_paths)` — the same `VariantResolver` the transfer already threads), and
- read **that variant's** `ret_paths` (falling back to the generic summary when no variant is selected).

Then a first call from a fresh record resolves the full variant → its `ret_paths` carry `.types` → the result's `field_own` gets `.types`; the second call's arg now has `.types` → resolves the full variant again → composes. Pair with the `result_ok` relaxation so a `MayAliasParams(unique)` return is eligible.

## File Structure

- **Create** fixtures under `boot/tests/fixtures/cfg/sound_uniqueness/`:
  - `ret_field_seq.tw` — two sequential `put` calls (minimal reproducer; positive).
  - `ret_field_loop.tw` — loop of `put` calls (positive).
  - `loop_set_insert.tw` — loop-carried `Set.insert` (positive, the wrapper case).
  - `ret_field_alias.tw` — the source param is aliased before the returning call (negative; must stay persistent).
  - `loop_field_dict_update.tw` — direct in-loop update (regression guard: already works, must keep working in place).
- **Modify** `boot/compiler/ownership.tw` — variant-aware return-field recovery in the ACall forward transfer; relax `result_ok` for `MayAliasParams(unique)`.
- **Create** `boot/tests/suites/return_field_ownership_suite.tw`; **Modify** `boot/tests/main.tw`.

---

## Task 1: Fixtures — the working boundary and the failing cases

**Files:** Create the five fixtures above.

- [ ] **Step 1: Regression guard (already works).** `loop_field_dict_update.tw`:

```twinkle
pub type Env = .{ types: Dict<Int, Int>, spare: Dict<Int, Int> }

pub fn go() Int {
  env := Env.{ types: Dict.new(), spare: Dict.new() }
  for i in range(5) {
    env.types[i] = i * 10
  }
  case env.types.get(3) {
    .Some(v) => v,
    .None => 0,
  }
}

println(go().to_string())
```

- [ ] **Step 2: Positive reproducers.** `ret_field_seq.tw`:

```twinkle
pub type Env = .{ types: Dict<Int, Int>, spare: Dict<Int, Int> }

pub fn put(e: Env, k: Int, v: Int) Env {
  e.types[k] = v
  e
}

pub fn go() Int {
  env := Env.{ types: Dict.new(), spare: Dict.new() }
  env = put(env, 0, 10)
  env = put(env, 1, 20)
  case env.types.get(1) {
    .Some(v) => v,
    .None => 0,
  }
}

println(go().to_string())
```

`ret_field_loop.tw` is the same with `for i in range(5) { env = put(env, i, i * 10) }` (output `30` via `.get(3)`). `loop_set_insert.tw`:

```twinkle
pub fn go() Int {
  s: Set<Int> = Set.new()
  for i in range(5) {
    s = s.insert(i)
  }
  s.len()
}

println(go().to_string())
```

- [ ] **Step 3: Negative (aliased source).** `ret_field_alias.tw`:

```twinkle
pub type Env = .{ types: Dict<Int, Int>, spare: Dict<Int, Int> }

pub fn put(e: Env, k: Int, v: Int) Env {
  e.types[k] = v
  e
}

pub fn go() Bool {
  shared := Dict.new().set(0, 0)
  env := Env.{ types: shared, spare: shared }
  env = put(env, 1, 20)
  shared.has(0)
}

println(go().to_string())
```

`types` and `spare` alias `shared`, so `put`'s field update must NOT mutate in place. Output `true`.

- [ ] **Step 4: Confirm runtime values compile.** `target/twk run <each fixture>` prints `30`/`20`/`5`/`true` (and `30` for `loop_field_dict_update`). Commit the fixtures.

```bash
git add boot/tests/fixtures/cfg/sound_uniqueness/ret_field_*.tw boot/tests/fixtures/cfg/sound_uniqueness/loop_set_insert.tw boot/tests/fixtures/cfg/sound_uniqueness/loop_field_dict_update.tw
git commit -m "test(ret-field): fixtures for return-carried field ownership (positive, negative, regression)"
```

---

## Task 2: Failing suite — positives lower in place, negative stays persistent

**Files:** Create `boot/tests/suites/return_field_ownership_suite.tw`; Modify `boot/tests/main.tw`.

- [ ] **Step 1: Write the suite** (reuse the helper shape from `field_backed_collection_suite.tw`: `compile_fixture_wat`, `wat_func_body_result`, `wat_has_instr`, `ir_sites_text`). The positive signal is the callee clone routing to the full tier and its body emitting the in-place helper; check both a user-function case and the Set case, plus the regression guard, plus the aliased negative:

```twinkle
.test(
  "sequential returning calls compose full-tier field ownership",
  fn() Result<Void, String> {
    try assert.str_contains(ir_sites_text("ret_field_seq"), "|0:;0:0")
    .Ok({})
  },
)
.test(
  "loop of returning calls stays in place",
  fn() Result<Void, String> {
    try assert.str_contains(ir_sites_text("ret_field_loop"), "|0:;0:0")
    .Ok({})
  },
)
.test(
  "loop-carried Set.insert reaches the full-tier clone",
  fn() Result<Void, String> {
    try assert.str_contains(ir_sites_text("loop_set_insert"), "|0:;0:0")
    .Ok({})
  },
)
.test(
  "direct in-loop field update still lowers in place (regression)",
  fn() Result<Void, String> {
    body := try wat_func_body_result(try compile_fixture_wat("loop_field_dict_update"), "go")
    try assert.ok(wat_has_instr(body, "rt_dict__set_in_place"), "direct loop field update must stay in place")
    .Ok({})
  },
)
.test(
  "aliased returning-call source stays persistent",
  fn() Result<Void, String> {
    try assert.ok(
      !ir_sites_text("ret_field_alias").contains("|0:;0:0"),
      "an aliased field backing must not publish/route a full-tier field variant",
    )
    .Ok({})
  },
)
```

- [ ] **Step 2: Register in `boot/tests/main.tw`, run red.**

```bash
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: the three positive `|0:;0:0` tests FAIL; the regression and aliased-negative tests PASS already.

- [ ] **Step 3: Commit the red suite.**

```bash
git add boot/tests/suites/return_field_ownership_suite.tw boot/tests/main.tw
git commit -m "test(ret-field): red tests for return-carried field ownership"
```

---

## Task 3: Variant-aware return-field recovery + `result_ok` relaxation

**Files:** Modify `boot/compiler/ownership.tw`.

- [ ] **Step 1: Confirm the forward-recovery context.** In the ACall forward transfer (the block ending around line 4122 that does `st.set_field_own(result, result_fields)`), verify `resolve: VariantResolver`, `args`, and `arg_unique` are in scope, and that `pre.call_arg_paths(args, arg_unique)` (or the already-computed `paths`) is available. If `arg_unique` is not yet computed here, compute it exactly as the sited-call scan does (`local_reusable(...) and store_count(...) == 1`).

- [ ] **Step 2: Resolve the callee variant by arg field paths.** Before the `eff_ret_paths` read, select the summary whose `ret_paths` to use:

```twinkle
arg_paths := pre.call_arg_paths(args, arg_unique)
eff_summary := case resolve(callee_id, arg_paths) {
  .Some(vs) => vs,
  .None => s,
}
```

Change `eff_ret_paths` to read `eff_summary.ret_paths` (still blanked to `[]` when `in_set(suppress, callee_id)`), and use `eff_summary.ret` for the `result_ok` classification below.

- [ ] **Step 3: Relax `result_ok` for unique may-alias returns.** Replace the `OwnedFresh`-only gate (line ~4087) with:

```twinkle
result_ok := case eff_summary.ret {
  .OwnedFresh => true,
  .MayAliasParams(ks) => {
    ok := ks.len() > 0
    for k in ks {
      if !(k < args.len() and arg_unique[k]) {
        ok = false
      }
    }
    ok
  },
  .Shared => false,
}
```

The existing `own_here` check (`OwnedFromParam(k)` requires `arg_unique[k]`, else publish-on-fail) already keeps a shared source from recovering — the aliased negative (`ret_field_alias`) exits with `shared` not unique, so `own_here`/`result_ok` both refuse it.

- [ ] **Step 4: Format, lint, build, run the Task 2 suite green.**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

Expected: the three positive tests pass; regression + aliased-negative still pass; all other suites green. If a positive still fails, re-open Step 2 — confirm the field-tier variant summary actually carries the `.types` `ret_path` (inspect via a temporary `render` of `eff_summary.ret_paths`); if the variant summary lacks it, the gap is in `summarize_variant`'s `ret_paths` propagation and must be fixed there first.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/ownership.tw
git commit -m "analysis(ret-field): variant-aware return-field ownership recovery

Resolve the callee variant by the call's argument field paths and recover the
selected variant's ret_paths field ownership; relax result_ok for a return that
may-alias only unique (consumed) params. Composes field ownership through
parameter-returning calls (put/Set.insert) so a second/loop call proves the full
tier. Aliased sources stay persistent (own_here still gates on arg_unique)."
```

---

## Task 4: Soundness sweep, byte-identical fallback, self-host gate

**Files:** Modify `boot/tests/suites/return_field_ownership_suite.tw` (runtime-parity assertions).

- [ ] **Step 1: Runtime correctness + specialize on/off parity** for `ret_field_seq` (20), `ret_field_loop` (30), `loop_set_insert` (5), `ret_field_alias` (true), `loop_field_dict_update` (30). `on == off` for each.

- [ ] **Step 2: Negative-aliasing sweep.** Re-run the 8I scoped helper inspection over `field_dict_alias_old`, `field_vector_alias_old`, `red_delegate_read_after`, `red_transport_read_after`, `visit_aliased` and confirm each user function still emits the persistent helper (no new `set_in_place`). This is the primary soundness guard — return-carried ownership must not leak through an alias.

- [ ] **Step 3: Byte-identical fallback.** Baseline the `sound_uniqueness` WAT set before Task 3; after, only fixtures that actually carry field ownership through a return (`ret_field_seq`, `ret_field_loop`, `loop_set_insert`) may change. Every other fixture must be byte-identical.

- [ ] **Step 4: Self-host gate.**

```bash
target/twk lint boot/main.tw
make bundle-cli
target/twk run boot/tests/main.tw
make stage2
```

Expected: lint clean; bundle succeeds; boot suite green; self-host fixed point (stage3 == stage4). Watch the `make stage2` timing — if the recovery change regresses compiler throughput (the `call_result_fact` cascade risk), narrow the variant resolution to calls whose args actually carry a non-shell path.

- [ ] **Step 5: Commit and update docs.**

```bash
git add boot/tests/suites/return_field_ownership_suite.tw
git commit -m "test(ret-field): runtime parity, aliasing sweep, byte-identical fallback"
```

Then in `docs/plans/sound-uniqueness/codegen/README.md`, note that loop-carried/threaded and `Set.insert`-in-a-loop field updates now lower in place via return-carried field ownership. Remove this plan's row from `docs/plans/README.md` and move this file to `docs/plans/archive/`.

---

## Scope Boundary

Delivers depth-one field ownership crossing a return, composing through parameter-returning calls (user functions and `Set.insert`/`remove`). It does **not** deliver:

- Path-level consume-dead with post-call sibling reads (the `field_transport_ctx` transport shape) — still a separate deferral.
- `Elem`/`Val` nested-value ownership (dict/vector element values).
- Return-field ownership for `OwnedFresh` wrappers that *construct* a new record around a borrowed field (only the may-alias-unique and fresh cases).
- Multi-carrier returns (a return that aliases more than one param) beyond the all-unique gate.

## Self-Review Checklist

1. **Spec coverage:** fixtures incl. regression + negative (Task 1), red positives/negative (Task 2), variant-aware recovery + `result_ok` (Task 3), soundness sweep + byte-identical + self-host (Task 4). ✓
2. **Grounded, not guessed:** root cause and fix location confirmed by the recorded spike (result_ok necessary-but-insufficient; generic summary lacks the field ret_path; variant summary carries it). ✓
3. **Soundness anchor:** recovery gated on `arg_unique` (unique source) and the callee's proven field-tier variant; `ret_field_alias` + the existing negative sweep are the guards. ✓
4. **Type consistency:** `result_ok`, `eff_summary`, `ret_paths`, `MayAliasParams`, `arg_unique`, `call_arg_paths`, and full-tier `|0:;0:0` are used consistently. ✓
5. **Perf awareness:** variant resolution is added to the forward recovery, not to `call_result_fact`'s cascade; Task 4 Step 4 watches self-host timing and offers a narrowing fallback. ✓
