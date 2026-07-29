# Return-Field Ownership Through Calls Implementation Plan

> **DONE — archived.** Implemented and self-host-verified 2026-07-29. As-built the fix
> was **four coordinated pieces**, deeper than this plan's Task 3 anticipated. A trace
> (`twk ir --cfg` + `[dbg:*]` in `summarize_variant_resolved`/`transfer_summarized_call`)
> showed the field-tier variant summary's `ret_paths` was empty not because the
> derivation dropped a present field, but because the variant was **seeded shell-only
> (Stage 3)** — the `[.f]` requirement was ignored at seed time. The delivered fix:
> 1. **`result_ok` relaxation** — recover for a `MayAliasParams` return aliasing only
>    unique params (as Task 3 Step 2 described).
> 2. **Stage 4 field-granular seeding** — `summarize_variant_resolved` builds a
>    `field_seed` from `[.f]` reqs and threads it through `summarize_seeded` →
>    `run_fixpoint` / `collect_move_recovered_params` / the return-site replay (the
>    dormant `seed_param_field_own` helper), so the variant body enters with `.f` Unique
>    and the derivation carries `.f = from(param)`.
> 3. **Publish-time field-seeded summary** — `run_scc_variants` (summary.tw) re-analyzes
>    the full-tier variant with its field key instead of reusing the shell-seeded summary.
> 4. **Variant-aware sited scan** — `collect_groups` resolves variants so the first
>    call's `ret_paths` recover `.f`, letting a sequential/loop-carried second call prove
>    the field tier (`call_uniques_sited_with_field_seed` gained a `resolve` param).
>
> Result: `ret_field_seq`/`ret_field_loop`/`loop_set_insert` route `sites=2` full-tier;
> the five existing aliasing negatives + `leak_ret` + `loop_field_alias` stay shell-tier;
> specialize on/off runtime parity for every fixture; 3300/3300 boot tests; self-host
> fixed point (stage3 == stage4). The original task narrative below is retained for
> context; Task 3's derivation-only framing was superseded by pieces 2–4 above.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. This plan is **soundness-critical** analysis-precision work; Task 4's aliased-negative sweep and the self-host gate are non-negotiable.

> **Revised 2026-07-29 after a spike.** The first draft framed this as a *loop-carried
> / `join_entry_field_own` back-edge* problem and proposed loop-seed/join changes.
> Investigation falsified that: a **direct** loop-carried record-field update already
> lowers in place, and a *non-loop* sequential pair of calls already fails. The real
> trigger and root cause below replace the original Tasks 1–3.
>
> **Re-revised 2026-07-29 after a `twk ir --cfg` probe.** The prior revision mislocated
> the fix: it claimed the forward recovery "reads the generic summary" and "never
> consults variant summaries", and proposed making the recovery *variant-aware*. That
> is false — `transfer_call` (`ownership.tw:3879`) already resolves the callee variant
> by arg field paths and passes **that variant's** `Summary` into
> `transfer_summarized_call` as `s`. The probe (`twk ir --cfg` on `ret_field_seq`)
> showed the real gap: the field-tier variant `put [unique:p0,p0.f0]` carries an
> **empty** `ret_paths` — so there is nothing to recover, which is exactly why the
> `result_ok` spike alone did not move the cases. The fix therefore lives in the
> **variant `ret_paths` derivation** (`summarize_variant_resolved`), not in the
> forward recovery. Task 3 below is rewritten accordingly.

**Goal:** Let reference-typed field ownership survive a function return, so a value produced by a field-preserving call (`env = put(env, k, v)`, `s = s.insert(k)`) can prove its field path at a *subsequent* call and lower that call's record-backed collection update in place.

**Architecture:** Field-backed in-place emission already works when the receiver's field is proven owned at the call (fresh construction gives this directly, so a *first* call from a freshly-built record routes to the full-tier clone). It breaks on the *next* call because the value now comes from a return, and **no `ret_paths` field entry exists to recover from**. Two things are true, both verified (see the probe in Orientation): (a) the forward recovery already receives the **variant** summary — `transfer_call` resolves the callee variant by arg field paths at `ownership.tw:3879` and passes it as `s` to `transfer_summarized_call`, so it is *not* stuck on the generic summary; but (b) even the field-tier variant summary carries an **empty** `ret_paths`, because `summarize_variant_resolved`'s derivation drops the record field across the in-place `e.types[k]=v` update, so there is nothing to recover. On top of that, recovery is gated to `OwnedFresh` returns via `result_ok`, and `put`/`insert` return `MayAliasParams`. The fix therefore has two parts: **populate the field-tier variant's `ret_paths`** in the derivation so the returned record carries `.types`, and **relax `result_ok`** for a result that may-alias only **unique** (consumed) params.

**Tech Stack:** Twinkle boot compiler (`boot/`), self-hosted. `make quick-bundle-cli` only re-bundles the CLI from the existing `boot.wasm`, so it suffices for test-only tasks (Task 1/2). **Task 3 edits the compiler (`ownership.tw`), so it needs `make bundle-cli`** (which runs `stage2` to recompile `boot.wasm`) before `twk run`/`twk ir`/`twk wat` reflect the change — those all run the *embedded* `boot.wasm`. No Rust stage0 changes.

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

Root cause, two parts:

1. **The field-tier variant summary carries no field `ret_path` (primary).** The forward recovery in `transfer_summarized_call` reads `s.ret_paths`, where `s` is **already the resolved variant summary** — `transfer_call` (`ownership.tw:3879-3882`) computes `paths := st.call_arg_paths(args, au)`, calls `resolve(fid.id, paths)`, and passes the selected variant `vs` (or the generic summary when no variant is selected) as `s`. A field `ret_path` is only emitted when the *re-analyzed variant body proves the field owned* (`body.field_own_get(aid)` / `path_prov_get(aid)` classified in the derivation at `ownership.tw:8380-8423`). For `put`'s field-tier variant that derivation currently yields **nothing** — see the probe — so `s.ret_paths` is empty and there is nothing to recover. The gap is in the *derivation*, not in which summary the recovery reads.
2. **`result_ok` gates to `OwnedFresh`.** Even once a field `ret_path` exists, recovery is skipped unless the callee's `ret` is `OwnedFresh` (`ownership.tw:4087`). `put`/`insert` return their parameter, so `ret = MayAliasParams([0])`, and recovery is blocked.

**Probe (`twk ir --cfg` on `ret_field_seq.tw`, recorded here — this is the authoritative `entry.summary` the forward recovery reads):**

| `put` summary | rendered header |
|---|---|
| generic | `p0=Consumed paths{[],[.f0]} … ret=alias(p0)` |
| variant `[unique:p0]` (shell) | `p0=Consumed paths{[]} … ret=alias(p0)` |
| variant `[unique:p0,p0.f0]` (field tier) | `p0=Consumed paths{[],[.f0]} … ret=alias(p0)` |

`render_ret_paths` prints `ret_paths=…` whenever non-empty; **no** header shows it, so **every** `put` summary — including the field-tier variant the fix depends on — has an empty `ret_paths`.

**Spike result (do not re-run; recorded here):** relaxing `result_ok` to accept `MayAliasParams(ks)` when every `k` is `arg_unique[k]` compiled and kept all 3294 tests green, but the sequential/loop/Set cases still proved shell-tier — because the field-tier variant summary's `ret_paths` is empty (probe above), so there is nothing to recover regardless of `result_ok`. So the core work is **populating the field-tier variant's `ret_paths` in the derivation**, with the `result_ok` relaxation as a necessary companion.

## Fix approach

Two coordinated changes in `boot/compiler/ownership.tw`. **No change to `transfer_call`'s variant resolution** (it already passes the variant summary as `s`) and **no threading of `resolve` into `transfer_summarized_call`** — the prior draft's variant-aware-recovery step is deleted.

1. **Populate the field-tier variant's `ret_paths` (primary fix).** In `summarize_variant_resolved`, the return-path derivation (`ownership.tw:8380-8423`) classifies single-segment `.Field` paths from the returned atom's `path_prov`/`field_own`. For `put`'s field-tier variant it yields nothing. The graft machinery *can* register a single-segment `[.Field(types)]` shell key in both maps (`graft` at `field_facts.tw:192`; `graft_path_prov` at `ownership.tw:4258`), and `classify_path_own` (`ownership.tw:7338`) would emit it — but the `ARecordUpdate` transfer for `e.types[k]=v` (`ownership.tw:4563-4591`) removes `.types` from the returned record and only re-grafts it when the result shell stays `Unique` **and** `value_moved` (single-retention of the new dict) holds; when that graft is skipped, the returned `e` has no classifiable `.types` key. Trace which guard drops it (Task 3 Step 3) and fix so the returned record carries a single-segment `[.Field(types)]` key whose origins classify as `OwnedFromParam(0)` (`shell_origins == [p0]`) or `OwnedFresh` (`shell_origins == []` with the field unique). Either classification is recoverable, and both are sound because the param is `Consumed`.

2. **Relax `result_ok` (necessary companion).** Recovery is gated to `OwnedFresh` returns (`ownership.tw:4087`); `put`/`insert` return `MayAliasParams`, so the populated `ret_paths` would still be ignored. Accept `MayAliasParams(ks)` when every `k` is `arg_unique[k]`, reading `s.ret` (the summary already passed in) — no re-resolution.

With both, the first call from a fresh record resolves the field-tier variant → its now-non-empty `ret_paths` carry `.types` → the moved result's `field_own` gets `.types`; the second call's arg now has `.types` → resolves the field-tier variant again → composes.

## File Structure

- **Create** fixtures under `boot/tests/fixtures/cfg/sound_uniqueness/`:
  - `ret_field_seq.tw` — two sequential `put` calls (minimal reproducer; positive).
  - `ret_field_loop.tw` — loop of `put` calls (positive).
  - `loop_set_insert.tw` — loop-carried `Set.insert` (positive, the wrapper case).
  - `ret_field_alias.tw` — the source param is aliased before the returning call (negative; must stay persistent).
  - `leak_ret.tw` — a returning callee that **leaks** its field before returning the param (callee-side negative; the derivation must NOT claim the returned field owned).
  - `loop_field_alias.tw` — a live pre-loop handle aliases the loop-entry record (loop-side runtime negative; the first iteration must stay persistent).
  - `loop_field_dict_update.tw` — direct in-loop update (regression guard: already works, must keep working in place).
- **Modify** `boot/compiler/ownership.tw` — populate the field-tier variant's `ret_paths` in the `summarize_variant_resolved` derivation (fix the `ARecordUpdate` field-drop); relax `result_ok` for `MayAliasParams(unique)`.
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

`types` and `spare` alias `shared`, so `put`'s field update must NOT mutate in place. Output `true`. Note this fixture resolves to the **shell** variant (`env.types` isn't uniquely owned at the call), so it guards the shell path but does **not** enter the field-tier recovery — hence the two extra negatives below.

- [ ] **Step 3b: Negative (callee-side field leak).** `leak_ret.tw` — enters the field-tier path with a *fresh* arg, but the callee leaks the field, so the derivation must not carry it back:

```twinkle
pub type Env = .{ types: Dict<Int, Int>, spare: Dict<Int, Int> }

pub fn put(e: Env, k: Int, v: Int) Env {
  e.types[k] = v
  e
}

pub fn leak_ret(e: Env, sink: Cell<Dict<Int, Int>>) Env {
  sink.set(e.types)
  e
}

pub fn go() Int {
  sink: Cell<Dict<Int, Int>> = Cell.new(Dict.new())
  env := Env.{ types: Dict.new(), spare: Dict.new() }
  env = leak_ret(env, sink)
  env = put(env, 0, 10)
  sink.get().len()
}

println(go().to_string())
```

`leak_ret` publishes `e.types` into `sink` and returns the record; the field is now aliased by `sink`. If the derivation wrongly claimed `leak_ret`'s returned `.types` owned, `put(env, 0, 10)` would update that leaked dict in place and `sink.get().len()` would be `1`. Correct output is `0`.

- [ ] **Step 3c: Negative (loop-carried aliased source, runtime).** `loop_field_alias.tw` — a live pre-loop handle aliases the loop-entry record:

```twinkle
pub type Env = .{ types: Dict<Int, Int>, spare: Dict<Int, Int> }

pub fn put(e: Env, k: Int, v: Int) Env {
  e.types[k] = v
  e
}

pub fn go() Int {
  env := Env.{ types: Dict.new(), spare: Dict.new() }
  keep := env
  for i in range(5) {
    env = put(env, i, i * 10)
  }
  case keep.types.get(0) {
    .Some(_) => 999,
    .None => 0,
  }
}

println(go().to_string())
```

`keep` aliases the initial record, so the **first** iteration's `put` must stay persistent (later iterations operating on non-aliased results may legitimately go in place — that is sound). If the first iteration mutated the initial dict in place, `keep.types.get(0)` would be `.Some` → `999`. Correct output is `0`. This is a **runtime/parity** guard only — do not assert absence of `|0:;0:0` for it (a precise post-fix compiler may route later iterations full-tier).

- [ ] **Step 4: Confirm runtime values compile.** `target/twk run <each fixture>` prints `30`/`20`/`5`/`true`/`0`/`0` (and `30` for `loop_field_dict_update`). Commit the fixtures.

```bash
git add boot/tests/fixtures/cfg/sound_uniqueness/ret_field_*.tw boot/tests/fixtures/cfg/sound_uniqueness/loop_set_insert.tw boot/tests/fixtures/cfg/sound_uniqueness/leak_ret.tw boot/tests/fixtures/cfg/sound_uniqueness/loop_field_alias.tw boot/tests/fixtures/cfg/sound_uniqueness/loop_field_dict_update.tw
git commit -m "test(ret-field): fixtures for return-carried field ownership (positive, negatives, regression)"
```

---

## Task 2: Failing suite — positives lower in place, negative stays persistent

**Files:** Create `boot/tests/suites/return_field_ownership_suite.tw`; Modify `boot/tests/main.tw`.

- [ ] **Step 1: Write the suite** (reuse the helper shape from `field_backed_collection_suite.tw`: `compile_fixture_wat`, `wat_func_body_result`, `wat_has_instr`, `ir_sites_text`). The positive signal is the callee clone routing to the full tier and its body emitting the in-place helper; check both a user-function case and the Set case, plus the regression guard, plus the caller- and callee-side negatives. (`loop_field_alias` is a runtime-only guard — it lives in Task 4 Step 1, not here, since a precise compiler may legitimately route later iterations full-tier.)

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
.test(
  "leaked-then-returned field stays persistent",
  fn() Result<Void, String> {
    try assert.ok(
      !ir_sites_text("leak_ret").contains("|0:;0:0"),
      "a callee that leaks its field must not carry field ownership back through the return",
    )
    .Ok({})
  },
)
```

`leak_ret`'s only field-backed candidate is the downstream `put` on the leaked dict, so absence of `|0:;0:0` is a valid invariant (unlike `loop_field_alias`). All negatives here are already green pre-fix; they must stay green through Task 3.

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

## Task 3: Populate the field-tier variant's `ret_paths` + `result_ok` relaxation

**Files:** Modify `boot/compiler/ownership.tw`.

> The forward recovery already receives the resolved variant summary as `s`
> (`transfer_call`, `ownership.tw:3879-3882`) — do **not** add variant resolution or
> thread `resolve` into `transfer_summarized_call`. The gap is upstream, in the
> derivation that produces the variant's `ret_paths`.

- [ ] **Step 1: Ground the gap on the current compiler.** With the *unmodified* embedded compiler (no rebuild needed — `--cfg` runs the current `target/twk`), dump the variant summaries for the reproducer:

```bash
target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/ret_field_seq.tw --cfg | grep -E "variant fn put|variant:"
```

Confirm the `variant fn put [unique:p0,p0.f0]` header shows `ret=alias(p0)` with **no** `ret_paths=` (empty). This is the exact value the forward recovery reads. This is the signal to drive Step 3 toward.

- [ ] **Step 2: Relax `result_ok` (isolated, do first).** In `transfer_summarized_call`, replace the `OwnedFresh`-only gate (`ownership.tw:4087`) with the version below, reading `s.ret` (the summary already passed in — already the resolved variant; **not** a re-resolved `eff_summary`). `arg_unique` is already in scope (`ownership.tw:4041`).

```twinkle
result_ok := case s.ret {
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

The existing `own_here` check (`OwnedFromParam(k)` requires `arg_unique[k]`, else publish-on-fail) still keeps a shared source from recovering — the aliased negative (`ret_field_alias`) resolves to the shell variant with `env.types` not unique, so `own_here`/`result_ok` both refuse it. On its own this change moves nothing (Step 1 confirmed the field-tier `ret_paths` is empty); Step 3 is what makes it fire.

- [ ] **Step 3: Populate the field-tier variant's `ret_paths` (the primary fix).** The returned `e` in `put`'s field-tier variant must expose a single-segment `[.Field(types)]` path that `classify_path_own` (`ownership.tw:7338`) turns into `OwnedFromParam(0)` (origins `[p0]`) or `OwnedFresh` (origins `[]` with the field unique). Diagnose where it is lost, then fix:
  - **Trace.** Temporarily `render` the returned atom's `field_own_get(aid)` and `path_prov_get(aid)` in the derivation (`ownership.tw:8380-8423`), and/or `rf`/`pp` at the end of the `ARecordUpdate` transfer (`ownership.tw:4563-4591`). The likely culprit is the `if own_is_unique(...) { … if value_moved { graft } }` guard: when `value_moved` (single-retention of the new dict `v`) is false, `.types` is removed (`remove_prefix`) and never re-grafted, so no shell key survives. Follow the `.types` key through `graft`/`graft_path_prov` (`field_facts.tw:192` / `ownership.tw:4258`) and `classify_path_own` to see which of {key absent, origins multi-origin, `fm.is_unique` false} kills it.
  - **Fix** the identified step so the field survives for the in-place field-tier update. Keep it depth-one and keep it scoped to the case where the field is genuinely retained in place.
  - **Verify the derivation output** (requires a compiler rebuild — `--cfg` runs the *embedded* `boot.wasm`):

    ```bash
    target/twk fmt boot/compiler/ownership.tw && make bundle-cli
    target/twk ir boot/tests/fixtures/cfg/sound_uniqueness/ret_field_seq.tw --cfg | grep -E "variant fn put|variant:"
    ```

    The `variant fn put [unique:p0,p0.f0]` header must now show `ret_paths=.f0=from(p0)` (or `.f0=fresh`).
  - **SOUNDNESS GUARD (mandatory).** In the same `--cfg` output, the **shell** variant `[unique:p0]` and the **generic** `put` summary must **still** show empty `ret_paths`. Those are the summaries an aliased caller resolves to; if the field ret_path leaks onto them, the aliased negative can recover a field it does not own. If either gained a `ret_paths`, the fix is too broad — narrow it to the field-tier (field-seeded) analysis.

- [ ] **Step 4: Format, lint, build, run the Task 2 suite green.**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
make bundle-cli
target/twk run boot/tests/main.tw
```

Expected: the three positive tests pass; regression + aliased-negative still pass; all other suites green. (Use `make bundle-cli`, not `make quick-bundle-cli`, so the ownership change is compiled into `boot.wasm`.)

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/ownership.tw
git commit -m "analysis(ret-field): carry field ownership through parameter-returning calls

Populate the field-tier variant's ret_paths so a returned record whose field was
updated in place exposes that field as owned, and relax result_ok for a return
that may-aliases only unique (consumed) params. Composes field ownership through
parameter-returning calls (put/Set.insert) so a second/loop call proves the full
tier. The forward recovery already reads the resolved variant summary; the fix is
in the derivation. Shell/generic summaries keep empty ret_paths, so aliased
sources stay persistent (own_here still gates on arg_unique)."
```

---

## Task 4: Soundness sweep, byte-identical fallback, self-host gate

**Files:** Modify `boot/tests/suites/return_field_ownership_suite.tw` (runtime-parity assertions).

- [ ] **Step 1: Runtime correctness + specialize on/off parity** for `ret_field_seq` (20), `ret_field_loop` (30), `loop_set_insert` (5), `ret_field_alias` (true), `leak_ret` (0), `loop_field_alias` (0), `loop_field_dict_update` (30). `on == off` for each. `leak_ret` and `loop_field_alias` are the sharp soundness cases — a wrong-in-place would flip them to `1` and `999` respectively, and the on/off parity is what proves specialization didn't introduce the corruption.

- [ ] **Step 2: Negative-aliasing sweep.** Re-run the 8I scoped helper inspection over `field_dict_alias_old`, `field_vector_alias_old`, `red_delegate_read_after`, `red_transport_read_after`, `visit_aliased`, plus the new `leak_ret`, and confirm each user function still emits the persistent helper (no new `set_in_place`). This is the primary soundness guard — return-carried ownership must not leak through an alias. Also confirm via `twk ir --cfg` that `leak_ret`'s variants (and the shell/generic `put` summaries) keep empty `ret_paths`.

- [ ] **Step 3: Byte-identical fallback.** Baseline the `sound_uniqueness` WAT set before Task 3. After, the target positives (`ret_field_seq`, `ret_field_loop`, `loop_set_insert`) will change, and `loop_field_alias` may change (later iterations may go in place — sound). Any **other** change is unexpected: investigate each one and confirm the changed function genuinely carries a field through a return (a legitimately improved codegen) rather than a soundness regression. Do not treat a diff as pass/fail by count — `make stage2` self-host convergence (Step 4) is the correctness backstop for the wider set.

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

1. **Spec coverage:** fixtures incl. regression + negative (Task 1), red positives/negative (Task 2), field-tier `ret_paths` derivation + `result_ok` (Task 3), soundness sweep + byte-identical + self-host (Task 4). ✓
2. **Grounded, not guessed:** root cause and fix location confirmed by the recorded spike **and** the `twk ir --cfg` probe (the forward recovery already receives the resolved variant summary as `s`; the field-tier variant's `ret_paths` is empty, so the fix is in the `summarize_variant_resolved` derivation, not in the recovery). ✓
3. **Soundness anchor:** recovery gated on `arg_unique` (unique source); the field `ret_path` is populated **only** on the field-tier variant, while the shell `[unique:p0]` and generic summaries an aliased caller resolves to keep empty `ret_paths` (Step 3 soundness guard); `ret_field_alias` + the existing negative sweep are the runtime guards. ✓
4. **Type consistency:** `result_ok`, `s.ret` (no re-resolved `eff_summary`), `ret_paths`, `MayAliasParams`, `arg_unique`, `classify_path_own`, and full-tier `|0:;0:0` are used consistently. ✓
5. **Perf awareness:** no variant resolution is added to the forward recovery (it already happens once in `transfer_call`); the change is confined to the `summarize_variant_resolved` derivation and the local `result_ok` gate, off `call_result_fact`'s cascade. Task 4 Step 4 watches self-host timing. ✓
