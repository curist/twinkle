# Physical Representation Planner Refactor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Where this sits:** this plan lives at the `docs/plans/` top level because it is
> a compiler-backend structural refactor, but it belongs to the vector-performance
> endeavor. Context, prior landed work, and the boundary tracklist are under
> [`docs/plans/performance/vector/README.md`](performance/vector/README.md) (see its
> "Living docs" table for the back-link). Read that README and
> [`typed-vector-representation.md`](performance/vector/typed-vector-representation.md)
> before starting.

> **Governing doctrine:** this is the sole implementor of
> [`physical-repr-ownership-design.md`](physical-repr-ownership-design.md) — the master
> ownership doctrine. Read it first; it fixes the invariants this plan realizes:
> `ReprKind` is the single physical-repr vocabulary (`TypedVec(ElemRepr)` is **live**);
> `ValType` is a pure function of `ReprKind` via `wasm_type_of_repr`, so
> **`wasm_type == wasm_type_of_repr(repr, mono)` holds at every slot**; `PhysPlan`
> records per-site `ElemRepr` overrides for func-id-keyed sites; `route` sets
> `repr = TypedVec(elem)` and *derives* `wasm_type` (never writes it independently);
> `mutvec_repr` sets `MutVec(elem)` the same way; verify asserts the invariant + edge
> classes; emit reads trustworthy `repr` and re-derives **nothing**.

> **Four vector families.** `route_func` iterates `all_families()`
> (`boot/compiler/elem_family.tw`) — `vec_i64` / `vec_bool` / `vec_f64` / `vec_byte`,
> physically `PVecI64` / `PVecBool` / `PVecF64` / `PVecByte`, tagged by
> `ElemRepr = { I64, I32, F64, Byte }` (`repr_policy.tw`). Every layer carries the
> family through `ElemRepr`: the plan value, `ReprKind.TypedVec(ElemRepr)`, and
> `pvec_wasm_type(elem)`. No family is ever flattened to i64.

**Goal:** Make `MonoType → ReprKind → ValType` a clean pure pipeline for typed vectors: revive `ReprKind.TypedVec` so the repr layer can name a typed vector, restore the invariant `wasm_type == wasm_type_of_repr(repr, mono)` by having `route` set `repr` and derive `wasm_type`, and record the per-site decision in one queryable `PhysPlan`. Ownership of the mono/use analysis, repr/ABI planning, slot mutation, and emission then falls out cleanly instead of drifting across `typed_param_abi`, `route_typed_vec`, `mutvec_repr`, and emit.

**Architecture:** The behavior of the emitted Wasm is preserved; what changes is *where the physical family lives*. Today `route` keeps `repr` at its boxed default (`TypedRef`) and overwrites only `SlotInfo.wasm_type`, because `ReprKind.TypedVec` is inert — so `repr` and `wasm_type` disagree at typed sites and emit must re-derive typedness as a safety net. This plan makes `TypedVec(ElemRepr)` live (`pvec_wasm_type` + a family-aware `wasm_type_of_repr` arm), has `route` set `repr = TypedVec(elem)` and derive `wasm_type` from it, records the decision in `PhysPlan`, and then removes the emit safety net because `repr` is now trustworthy. Builds on the MutVec escape-return tactical fix already landed on this branch (`d51a9568` "keep escaping regions PVecI64 across the return (no rebox)").

**Tech Stack:** Boot compiler (Twinkle in `boot/`), backend prepared IR (`boot/compiler/backend/`), typed-vector routing (`route_typed_vec.tw`), cross-function repr analysis (`typed_param_abi.tw`), physical verification (`verify_expr.tw` / `verify_slots.tw`), Wasm-GC emission (`boot/compiler/codegen/emit*.tw`).

## Global Constraints

- Treat `MonoType` and physical repr as orthogonal: `MonoType.Vector(.Int)` maps to `ReprKind` `TypedRef` (boxed `PVec`), `TypedVec(.I64)` (typed `PVecI64`), or `MutVec(.I64)` (transient handle) depending on site — same per family for Bool/Float/Byte. `ValType` is always `wasm_type_of_repr(repr, mono)`.
- **The invariant is the correctness spine:** `SlotInfo.wasm_type == wasm_type_of_repr(SlotInfo.repr, SlotInfo.mono)` for every slot, after every pass. Never write `wasm_type` without setting `repr` to match. Verify asserts it (Task 8). (The real signature is `wasm_type_of_repr(repr, mono, env)` — `repr_assign.tw:324`; `env` resolves nominal struct types. `(repr, mono)` is the shorthand used throughout this plan; verify calls the three-arg form.)
- Do not make `Vector<Int>` globally typed. Typed vectors remain per-storage-site / per-ABI-edge decisions with explicit box/unbox at durable erased boundaries.
- Preserve the tactical MutVec escape-return fix: `route_typed_vec` recognizes `vector$__mutvec_freeze_i64` as a typed producer; `mutvec_repr` does not override freeze-result slots.
- Behavior-preserving: same emitted Wasm at every task. The repr-layer change (reviving `TypedVec`) is a *representation* change, not a codegen change — `wasm_type_of_repr(.TypedVec(elem))` must yield exactly the `ValType` route used to write directly.
- **End state:** `route` remains the *decider* of typed-vector slot/return/capture eligibility (its `compute_eligible_v` fixpoint); it *applies* by setting `repr` and deriving `wasm_type`; `PhysPlan` is the *queryable record* of those func-id-keyed decisions (an `ElemRepr` per site) — not a second decision engine. Return/capture facts flow through the plan (Task 9 drops the redundant maps). Downstream reads `SlotInfo.repr`/`wasm_type` (consistent by the invariant); field/payload family stays with `wasm_layout`.
- **Capture a baseline first.** Before Task 1, record current `TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/stage2.wasm 2>&1 | grep '^\[time'` numbers (esp. `analyze_typed_repr` / `route_typed_vectors` from the prepare sub-timings) into the tracklist. Every `PhysPlan` lookup is a string-interpolated `Dict<String, _>` access (per-key alloc + FNV-1a); the perf guard in the final task diffs against this baseline.
- After editing `.tw` files, run `target/twk fmt <changed .tw files>` and `target/twk lint boot/main.tw`; for fixture/test files outside project formatting reachability, format/lint them explicitly.
- After compiler-source edits, rebuild with `make bundle-cli`; `make quick-bundle-cli` is valid only when `target/boot.wasm` is already fresh.
- Validation must include self-host fixed point, boot tests, and targeted WAT checks for typed-return and boxed-boundary behavior.

---

## Context and current pain

The current compiler already has the right *data fields*:

- `SlotInfo.mono` records semantic type.
- `SlotInfo.repr` / `SlotInfo.wasm_type` record physical storage type.
- `PreparedFunc.phys_return` records a physical return ABI override.

The root problem is a broken invariant. `ReprKind` already has a `TypedVec(ElemRepr)`
variant meant to name a typed vector, but it is **inert**: `wasm_type_of_repr(.TypedVec(_))`
ignores the `ElemRepr` and returns the boxed `PVec` (`repr_assign.tw:348`,
`verify_common.tw:27`). So `route` cannot express a typed vector *as a repr* — it
keeps `repr = TypedRef(mono)` (boxed) and overwrites only `SlotInfo.wasm_type`
(`with_repr_wasm(info, info.repr, .Ref(…PVecI64))`, `route_typed_vec.tw:627`). After
routing, `repr` says boxed and `wasm_type` says `PVecI64` — the two disagree at every
typed site. Consequences:

- `emit` cannot trust `repr`, so it re-derives typedness from `MonoType` as a safety net.
- `mutvec_repr` patches `wasm_type` the same way; a stale frozen-slot override showed how a late `wasm_type` write clobbers an earlier decision.
- `typed_param_abi.tw` emits `typeable_return` / `capture_abi` / field-payload sets as loose maps threaded separately.

The fix makes `TypedVec(ElemRepr)` live and has `route` set `repr` (deriving
`wasm_type`), restoring `wasm_type == wasm_type_of_repr(repr, mono)`. Then `PhysPlan`
records the per-site decision, verify asserts the invariant, and emit's safety net is
dead code to delete.

Relevant existing reference: `docs/plans/performance/vector/archive/unify-typedness-oracle-design.md`.

---

## File Structure

**Create:**
- `boot/compiler/backend/phys_plan.tw` — `PhysPlan` (`slots`/`returns`/`captures` over `ElemRepr`), key helpers, ABI projection, accessors.
- `boot/tests/suites/phys_plan_suite.tw` — key construction, default-boxed, projection, and repr→ValType tests.

**Modify — repr layer (makes the invariant expressible):**
- `boot/compiler/codegen/wasm_layout.tw` — add `pvec_wasm_type(elem: ElemRepr) ValType` beside its twin `mutvec_wasm_type` (which lives here, *not* in `repr_policy.tw`).
- `boot/compiler/backend/repr_assign.tw` (`.TypedVec` arm at `repr_assign.tw:348`) + `boot/compiler/backend/verify_common.tw` (twin at `verify_common.tw:27`) — point the `.TypedVec(elem)` arm of `wasm_type_of_repr` (and its twin) at `pvec_wasm_type(elem)`.

**Modify — planning/application:**
- `boot/tests/main.tw` — register `phys_plan_suite`.
- `boot/compiler/backend/typed_param_abi.tw` — project `ReprAnalysis` return/capture facts into `PhysPlan`.
- `boot/compiler/backend/route_typed_vec.tw` — set `repr = TypedVec(elem)` and derive `wasm_type`; materialize + return the plan; consume return/capture from the plan.
- `boot/compiler/backend/mutvec_repr.tw` — set `repr = MutVec(elem)` and derive `wasm_type`; keep only handle responsibility.
- `boot/compiler/backend/prepare.tw` — thread the plan analysis → route → mutvec → verify.
- `boot/compiler/backend/verify_expr.tw`, `verify_slots.tw`, `verify_common.tw` — assert the invariant + edge-class checks.
- `boot/compiler/codegen/emit.tw` — remove the `MonoType`-driven typedness re-derivation once `repr` is trustworthy.
- `docs/plans/performance/vector/README.md` and `boundary-tracklist.md` — link this plan.

---

## Task 1: Revive `ReprKind.TypedVec` and introduce `PhysPlan`

**Files:**
- Modify: `boot/compiler/codegen/wasm_layout.tw` (add `pvec_wasm_type` beside `mutvec_wasm_type`)
- Modify: `boot/compiler/backend/repr_assign.tw`, `boot/compiler/backend/verify_common.tw` (make the `.TypedVec` arm live)
- Create: `boot/compiler/backend/phys_plan.tw`
- Create: `boot/tests/suites/phys_plan_suite.tw`
- Modify: `boot/tests/main.tw`

> **Two things, both foundational.** (1) The repr layer: `ReprKind.TypedVec(ElemRepr)`
> is currently inert — make it name its family PVec, so `wasm_type_of_repr(.TypedVec(elem))`
> yields `pvec_wasm_type(elem)`. This is safe in isolation because **nothing constructs
> `TypedVec` yet** (route does that in Task 5), so changing its mapping is inert-until-used.
> (2) The plan: `PhysPlan` records per-site `ElemRepr` overrides, func-id-keyed only
> (slots / returns / captures — each single-family because functions monomorphize).
> Field/payload family is **not** in the plan (per-instantiation, owned by
> `wasm_layout`); their eligibility stays in the `typed_fields` / `typed_payloads`
> presence maps. There is no `PhysRepr` enum — `ElemRepr` + `ReprKind` are the only
> repr vocabularies.

- [ ] **Step 1: Make `TypedVec` live — add `pvec_wasm_type`, fix `wasm_type_of_repr`.**

In `codegen/wasm_layout.tw`, beside `mutvec_wasm_type` (its exact mirror — that
function lives here, not in `repr_policy.tw`), add:

```tw
pub fn pvec_wasm_type(elem: ElemRepr) ValType {
  name := case elem {
    .I64 => "rt_types__PVecI64",
    .I32 => "rt_types__PVecBool",
    .F64 => "rt_types__PVecF64",
    .Byte => "rt_types__PVecByte",
  }
  .Ref(true, .Named(name))
}
```

Then repoint the `.TypedVec` arm in **both** repr→ValType functions from the inert
`val_type_of_mono(mono)` to `pvec_wasm_type(elem)`:

```tw
// repr_assign.tw wasm_type_of_repr_cached, ~line 348:
.TypedVec(e) => .{ val_type: pvec_wasm_type(e), cache },
// verify_common.tw ~line 27 (its twin repr->valtype):
.TypedVec(e) => pvec_wasm_type(e),
```

Update the `verify_expr.tw:1293` `.TypedVec(_) => case actual { .TypedVec(_) => ... }`
arm to compare the `ElemRepr` (or rely on the invariant check added in Task 8), and
drop the "inert" wording in `mutvec_repr.tw` comments. No slot carries `TypedVec` yet,
so this commit is behavior-neutral; a targeted test locks the mapping (Step 3).

- [ ] **Step 2: Write the plan module skeleton.**

Create `boot/compiler/backend/phys_plan.tw`. The plan value is the typed-family
override `ElemRepr`; an absent entry means the boxed default:

```tw
//! Per-site physical-repr OVERRIDES for typed-vector ABI sites, keyed by a
//! monomorphized function id. An entry is the typed family (ElemRepr); absent means
//! "no override -> the MonoType default (boxed PVec)". The site's ReprKind is
//! TypedVec(elem) and its ValType is pvec_wasm_type(elem). Record fields / variant
//! payloads are NOT here (per-instantiation family, owned by wasm_layout); MutVec*
//! handles are ReprKind.MutVec owned by mutvec_repr.

use compiler.backend.repr_policy.{ElemRepr}

// Field names differ from the accessor names below on purpose: a `pub fn
// slot_repr(plan: PhysPlan, ...)` whose first param is PhysPlan auto-registers as an
// inherent method, and a same-named field would be a FieldMethodCollision.
pub type PhysPlan = .{
  slots: Dict<String, ElemRepr>,
  returns: Dict<String, ElemRepr>,
  captures: Dict<String, ElemRepr>,
}

pub fn empty() PhysPlan {
  .{ slots: Dict.new(), returns: Dict.new(), captures: Dict.new() }
}

pub fn slot_key(func_id: Int, slot_id: Int) String { "${func_id}:${slot_id}" }
pub fn abi_key(func_id: Int, index: Int) String { "${func_id}:${index}" }

// Accessors return ElemRepr? directly: .Some(elem) is a typed override, .None the
// boxed default. Callers derive ReprKind via .TypedVec(elem) and ValType via
// pvec_wasm_type(elem).
pub fn slot_repr(plan: PhysPlan, func_id: Int, slot_id: Int) ElemRepr? {
  plan.slots[slot_key(func_id, slot_id)]
}

pub fn return_repr(plan: PhysPlan, func_id: Int) ElemRepr? {
  plan.returns["${func_id}"]
}

pub fn capture_repr(plan: PhysPlan, func_id: Int, index: Int) ElemRepr? {
  plan.captures[abi_key(func_id, index)]
}
```

- [ ] **Step 3: Add focused tests.**

Create `boot/tests/suites/phys_plan_suite.tw`:

```tw
use @std.testing.assert as assert
use @std.testing as runner
use compiler.backend.phys_plan
use compiler.backend.repr_policy
use compiler.codegen.wasm_layout.{pvec_wasm_type}

fn test_keys_are_stable() Result<Void, String> {
  try assert.equal(phys_plan.slot_key(7, 3), "7:3")
  try assert.equal(phys_plan.abi_key(8, 0), "8:0")
  .Ok({})
}

fn test_absent_entries_are_none() Result<Void, String> {
  p := phys_plan.empty()
  try assert.is_true(phys_plan.slot_repr(p, 1, 2) == .None)
  try assert.is_true(phys_plan.return_repr(p, 1) == .None)
  try assert.is_true(phys_plan.capture_repr(p, 1, 0) == .None)
  .Ok({})
}

fn assert_pvec_name(elem: repr_policy.ElemRepr, want: String) Result<Void, String> {
  case pvec_wasm_type(elem) {
    .Ref(_, ht) => case ht {
      .Named(name) => assert.equal(name, want),
      _ => .Err("expected named heap type"),
    },
    _ => .Err("expected ref valtype"),
  }
}

fn test_pvec_wasm_type_per_family() Result<Void, String> {
  try assert_pvec_name(.I64, "rt_types__PVecI64")
  try assert_pvec_name(.I32, "rt_types__PVecBool")
  try assert_pvec_name(.F64, "rt_types__PVecF64")
  try assert_pvec_name(.Byte, "rt_types__PVecByte")
  .Ok({})
}

pub fn suite() runner.Suite {
  runner
    .suite("physical representation plan")
    .test("keys are stable", test_keys_are_stable)
    .test("absent entries are none", test_absent_entries_are_none)
    .test("pvec_wasm_type per family", test_pvec_wasm_type_per_family)
}
```

(`import compiler.backend.repr_policy` too for the `ElemRepr` type annotation.)

- [ ] **Step 4: Register the suite.**

Modify `boot/tests/main.tw`: `use .suites.phys_plan_suite` and add `phys_plan_suite.suite()` near other backend suites.

- [ ] **Step 5: Format, rebuild, test.**

```bash
target/twk fmt boot/compiler/backend/repr_policy.tw boot/compiler/backend/repr_assign.tw boot/compiler/backend/verify_common.tw boot/compiler/backend/verify_expr.tw boot/compiler/backend/mutvec_repr.tw boot/compiler/backend/phys_plan.tw boot/tests/suites/phys_plan_suite.tw boot/tests/main.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point (behavior-neutral — `TypedVec` still unconstructed), no lint findings, boot tests pass.

- [ ] **Step 6: Commit.**

```bash
git add boot/compiler/backend/repr_policy.tw boot/compiler/backend/repr_assign.tw boot/compiler/backend/verify_common.tw boot/compiler/backend/verify_expr.tw boot/compiler/backend/mutvec_repr.tw boot/compiler/backend/phys_plan.tw boot/tests/suites/phys_plan_suite.tw boot/tests/main.tw
git commit -m "backend: make ReprKind.TypedVec live; introduce PhysPlan over ElemRepr"
```

---

## Task 2: Project typed-return/capture ABI facts into `PhysPlan`

**Files:**
- Modify: `boot/compiler/backend/typed_param_abi.tw` (add the projection helper)
- Modify: `boot/tests/suites/phys_plan_suite.tw`

**Interfaces:**
- Consumes: `phys_plan.PhysPlan`, `phys_plan.empty`.
- Produces: `pub fn phys_plan_from_analysis(r: ReprAnalysis) phys_plan.PhysPlan`.

> **Only the func-id-keyed ABI facts project.** `typeable_return` / `capture_abi`
> values already ARE the family `mono_key` and key on a monomorphized function id, so
> they project their real family straight through — every family, never filtered to
> `vec_i64`. Record fields and variant payloads do **not** project: `typed_fields` /
> `typed_payloads` are `(TypeId, field)`-keyed presence maps whose physical family is
> per-instantiation (owned by `wasm_layout`), so no single `Typed(fam)` entry can
> name it. They stay as presence maps that route keeps consuming. Slot reprs are not
> a projection — route materializes them from its own eligibility fixpoint (Task 5).

- [ ] **Step 1: Add projection helper.**

The ABI maps carry the family as a `mono_key` string; the plan carries `ElemRepr`. Add
the small bridge in `repr_policy.tw` (next to `candidate_typed_vec_family`):

```tw
pub fn elem_repr_of_mono_key(key: String) ElemRepr? {
  cond {
    key == "vec_i64" => .Some(.I64),
    key == "vec_bool" => .Some(.I32),
    key == "vec_f64" => .Some(.F64),
    key == "vec_byte" => .Some(.Byte),
    _ => .None,
  }
}
```

Then in `boot/compiler/backend/typed_param_abi.tw`, import `phys_plan` + `repr_policy` and add after `pub type ReprAnalysis`:

```tw
use compiler.backend.phys_plan
use compiler.backend.repr_policy.{elem_repr_of_mono_key}

pub fn phys_plan_from_analysis(r: ReprAnalysis) phys_plan.PhysPlan {
  p := phys_plan.empty()

  // The map value is the family mono_key — map it to ElemRepr, every family.
  for func_key, fam_key in r.param_abi.typeable_return {
    case elem_repr_of_mono_key(fam_key) {
      .Some(elem) => p.returns[func_key] = elem,
      .None => {},
    }
  }
  for func_key, captures in r.capture_abi {
    for idx, fam_key in captures {
      case elem_repr_of_mono_key(fam_key) {
        .Some(elem) => p.captures["${func_key}:${idx}"] = elem,
        .None => {},
      }
    }
  }

  p
}
```

This is a projection only: no routing behavior changes yet.

- [ ] **Step 2: Test projection defaults and typed entries.**

Add to `phys_plan_suite.tw`:

```tw
use compiler.backend.typed_param_abi.{ParamAbi, ReprAnalysis, phys_plan_from_analysis}

fn test_analysis_projection_sets_abi_entries() Result<Void, String> {
  // A typed non-i64 return + capture must survive projection, not be dropped.
  typeable_return: Dict<String, String> = Dict.new()
  typeable_return["7"] = "vec_f64"
  captures: Dict<Int, String> = Dict.new()
  captures[0] = "vec_bool"
  capture_abi: Dict<String, Dict<Int, String>> = Dict.new()
  capture_abi["8"] = captures

  r := ReprAnalysis.{
    typed_fields: Dict.new(),
    typed_payloads: Dict.new(),
    param_abi: ParamAbi.{ typeable_params: Dict.new(), typeable_return },
    capture_abi,
  }
  p := phys_plan_from_analysis(r)

  try assert.is_true(phys_plan.return_repr(p, 7) == .Some(.F64))
  try assert.is_true(phys_plan.capture_repr(p, 8, 0) == .Some(.I32))
  try assert.is_true(phys_plan.return_repr(p, 9) == .None)
  .Ok({})
}
```

Register it in `suite()`.

- [ ] **Step 3: Format, rebuild, test.**

Run:

```bash
target/twk fmt boot/compiler/backend/typed_param_abi.tw boot/tests/suites/phys_plan_suite.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point, no lint findings, boot tests pass.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/backend/typed_param_abi.tw boot/tests/suites/phys_plan_suite.tw
git commit -m "backend: project typed-vector ABI facts into PhysPlan"
```

---

## Task 3: Thread `PhysPlan` through prepare without behavior changes

**Files:**
- Modify: `boot/compiler/backend/prepare.tw`
- Modify: `boot/compiler/backend/route_typed_vec.tw`
- Modify: direct test callers of `route_typed_vectors` under `boot/tests/suites/`

**Interfaces:**
- Consumes: `typed_param_abi.phys_plan_from_analysis`.
- Produces: widened `route_typed_vectors(..., plan: phys_plan.PhysPlan, ...)` while still accepting existing maps for behavior.

- [ ] **Step 1: Widen route entry point with inert `plan` argument.**

In `route_typed_vec.tw`, import `compiler.backend.phys_plan` and change the public signature from:

```tw
pub fn route_typed_vectors(
  funcs: Vector<PreparedFunc>,
  builtins: BuiltinRegistry,
  typed_fields: Dict<String, Bool>,
  typed_payloads: Dict<String, Bool>,
  capture_abi: Dict<String, Dict<Int, String>>,
  typeable_return: Dict<String, String>,
) RoutedModule {
```

to:

```tw
pub fn route_typed_vectors(
  funcs: Vector<PreparedFunc>,
  builtins: BuiltinRegistry,
  plan: phys_plan.PhysPlan,
  typed_fields: Dict<String, Bool>,
  typed_payloads: Dict<String, Bool>,
  capture_abi: Dict<String, Dict<Int, String>>,
  typeable_return: Dict<String, String>,
) RoutedModule {
```

Keep `plan` unused in this task. If the linter complains about an unused binding, read a harmless value once near the top:

```tw
_ := plan.returns.keys().len()
```

- [ ] **Step 2: Thread plan from prepare.**

In `prepare.tw`, after `repr := analyze_typed_repr(...)`, project:

```tw
phys := typed_param_abi.phys_plan_from_analysis(repr)
```

Then pass `phys` into `route_typed_vectors` before the existing typed maps.

- [ ] **Step 3: Update all direct callers.**

Search:

```bash
rg -n "route_typed_vectors\(" boot
```

For each test/helper caller that does not already import `compiler.backend.phys_plan`, add that import and pass `phys_plan.empty()` unless the caller already has a `ReprAnalysis` result to project.

- [ ] **Step 4: Format, rebuild, test.**

Run:

```bash
target/twk fmt boot/compiler/backend/prepare.tw boot/compiler/backend/route_typed_vec.tw boot/tests/suites/*.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point, no lint findings, boot tests pass.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/backend/prepare.tw boot/compiler/backend/route_typed_vec.tw boot/tests/suites
git commit -m "backend: thread PhysPlan through typed-vector routing"
```

---

## Task 4: Make route consume `PhysPlan` for returns and captures

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw`
- Modify: `boot/tests/suites/route_typed_vec_suite.tw`
- Modify: `boot/tests/suites/typed_param_abi_suite.tw`

**Interfaces:**
- Consumes: `phys_plan.PhysPlan` from Task 3.
- Produces: route internals that read return/capture facts from `plan` instead of the separately-passed `typeable_return` / `capture_abi` maps.

> **Scope: returns and captures only.** These are the func-id-keyed facts the plan
> owns. Record-field / variant-payload typing continues to read the `typed_fields` /
> `typed_payloads` presence maps unchanged — their family is resolved per-store-site /
> per-instantiation as today, and is not in the plan. Slot reprs are materialized in
> Task 5, not here.

- [ ] **Step 1: Reconstruct the return/capture maps from the plan.**

In `route_typed_vec.tw`, add helpers near `route_func` that rebuild exactly the two
maps route already consumes, so behavior comes from `plan`:

The plan stores `ElemRepr`; route's maps are keyed by `mono_key`, so add the inverse
of `elem_repr_of_mono_key` in `repr_policy.tw`:

```tw
pub fn mono_key_of_elem_repr(elem: ElemRepr) String {
  case elem {
    .I64 => "vec_i64",
    .I32 => "vec_bool",
    .F64 => "vec_f64",
    .Byte => "vec_byte",
  }
}
```

Then, in `route_typed_vec.tw`, add helpers near `route_func` that rebuild exactly the
two maps route already consumes, so behavior comes from `plan`:

```tw
fn typeable_return_from_plan(plan: phys_plan.PhysPlan) Dict<String, String> {
  out: Dict<String, String> = Dict.new()
  for key, elem in plan.returns {
    out[key] = mono_key_of_elem_repr(elem)
  }
  out
}

fn capture_abi_from_plan(plan: phys_plan.PhysPlan) Dict<String, Dict<Int, String>> {
  out: Dict<String, Dict<Int, String>> = Dict.new()
  for key, elem in plan.captures {
    parts := key.split(":")
    if parts.len() == 2 {
      func_key := parts[0]
      idx := parts[1].parse_int().unwrap_or(-1)
      if idx >= 0 {
        cur := case out[func_key] {
          .Some(m) => m,
          .None => Dict.new(),
        }
        cur[idx] = mono_key_of_elem_repr(elem)
        out[func_key] = cur
      }
    }
  }
  out
}
```

> **Transitional scaffolding.** These two adapters reconstruct maps analysis already
> produced (a `map → PhysPlan → map` round-trip), and `capture_abi_from_plan`
> re-parses the interpolated key with `split`/`parse_int` — fragile, with an
> `unwrap_or(-1)` path. It is a **stepping stone**: the target is route querying
> `phys_plan.return_repr(plan, fid)` / `phys_plan.capture_repr(plan, fid, idx)` at
> its decision sites and dropping both the adapters and the legacy parameters in
> **Task 9**. If a decision site can move to a point query directly here, prefer that
> and skip the corresponding adapter. If `String.split` / `parse_int` names differ,
> use the existing compiler string helpers; keep the adapter local and covered by
> tests.

- [ ] **Step 2: Route from plan-derived maps.**

At the top of `route_typed_vectors`, derive the two maps from `plan` and use them for
route computation instead of the passed `capture_abi` / `typeable_return`:

```tw
route_capture_abi := capture_abi_from_plan(plan)
route_typeable_return := typeable_return_from_plan(plan)
```

Keep the old explicit `capture_abi` / `typeable_return` parameters temporarily; add a
small equality assertion (or skip in this task) — behavior must come from `plan`.
`typed_fields` / `typed_payloads` are untouched: route reads them exactly as before.

- [ ] **Step 3: Test that route honors the plan.**

Add a focused test where the legacy `typeable_return` / `capture_abi` maps are empty
but `plan` carries a typed return:

```tw
fn test_route_uses_phys_plan_return_projection() Result<Void, String> {
  // Compile a function `g` returning Vector<Int> and a caller reading `g()`.
  // Build a PhysPlan with returns["<g func id>"] = .I64.
  // Call route_typed_vectors(..., plan, typed_fields, typed_payloads, Dict.new(), Dict.new()).
  // Assert the caller's call-result slot is PVecI64 and no legacy typeable_return was needed.
  .Ok({})
}
```

Use existing helper patterns in `route_typed_vec_suite.tw` to find function ids and inspect slot wasm types. Do not rely on WAT substring matching in this unit test.

- [ ] **Step 4: Format, rebuild, test.**

Run:

```bash
target/twk fmt boot/compiler/backend/route_typed_vec.tw boot/tests/suites/route_typed_vec_suite.tw boot/tests/suites/typed_param_abi_suite.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point, no lint findings, boot tests pass.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/backend/route_typed_vec.tw boot/tests/suites/route_typed_vec_suite.tw boot/tests/suites/typed_param_abi_suite.tw
git commit -m "backend: route typed vectors from PhysPlan projections"
```

---

## Task 5: Route sets `repr = TypedVec`, derives `wasm_type`, records the slot plan

This is the invariant-restoration task. Route stops writing `wasm_type` independently;
it sets `repr = TypedVec(elem)` and derives `wasm_type = wasm_type_of_repr(repr, mono)`
(= `pvec_wasm_type(elem)`), so `SlotInfo.repr` and `SlotInfo.wasm_type` agree at typed
sites for the first time. This is the one behavior-adjacent change — but it is
byte-for-byte identical because `pvec_wasm_type(elem)` equals the `ValType` route used
to write directly (`fi.fam.pvec_type`).

> **Byte-identical needs a reader audit, not just a matching `wasm_type`.** Flipping a
> typed slot's `repr` from `TypedRef`→`TypedVec` is behavior-neutral only if **no
> existing reader of `SlotInfo.repr` branches differently** when a vector slot's repr
> becomes `TypedVec` instead of the boxed `TypedRef` default. Before Step 2, grep every
> `case … repr` / `SlotInfo.repr` reader (`rg -n 'case .*repr|\.repr\b' boot/compiler`)
> and confirm each either ignores `TypedVec` or already handles it as it did the boxed
> default. One such reader is already known — `verify_expr.tw:1293` has a `.TypedVec`
> arm (patched in Task 1 Step 1) — so readers exist; the audit is finding the rest.
> Emit is *supposed* to still re-derive from `MonoType` here (its safety net isn't
> removed until Task 11), so it is not a concern for this step.

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw`
- Modify: `boot/compiler/backend/phys_plan.tw`
- Modify: `boot/tests/suites/route_typed_vec_suite.tw`

**Interfaces:**
- Produces:
  - `pub fn set_slot_repr(plan: PhysPlan, func_id: Int, slot_id: Int, elem: ElemRepr) PhysPlan`
  - route-local slot-plan materialization that writes `slots` entries as route applies `TypedVec`.

- [ ] **Step 1: Add a setter helper.**

In `phys_plan.tw`:

```tw
pub fn set_slot_repr(plan: PhysPlan, func_id: Int, slot_id: Int, elem: ElemRepr) PhysPlan {
  plan.slots[slot_key(func_id, slot_id)] = elem
  plan
}
```

- [ ] **Step 2: Apply `TypedVec` and derive `wasm_type` in the per-family routine.**

Slot eligibility is computed per family: `route_func` iterates
`for fi in families_ids(builtins)` and calls `route_func_family(pf, fi, ids, builtins, ...)`,
where `compute_eligible_v` runs against the active family `fi`. Each eligible slot's `ElemRepr` comes from its own type via
`candidate_typed_vec_family(info.mono)` (pure, `repr_policy.tw`, `.Some` for any
typed-vector slot). The retype site walks the slots with `info` in hand, so record the
plan entry and set the repr there in one pass. Replace the current retype site
(`route_typed_vec.tw:627`, `with_repr_wasm(info, info.repr, .Ref(true, .Named(fi.fam.pvec_type)))`)
with a version that sets `repr = TypedVec(elem)` and derives `wasm_type = pvec_wasm_type(elem)`:

```tw
eligible_v.has(info.slot.id) => case candidate_typed_vec_family(info.mono) {
  .Some(elem) => .Some(with_repr_wasm(info, .TypedVec(elem), pvec_wasm_type(elem))),
  .None => .Some(info),
},
```

and, threading `func_plan` through the same slot walk, record each such slot:
`func_plan = phys_plan.set_slot_repr(func_plan, pf.func_id.id, info.slot.id, elem)`.
`pvec_wasm_type(elem)` is the same `ValType` route used to write directly
(`fi.fam.pvec_type`), so this is byte-identical — but now `repr` and `wasm_type` agree,
and verify (which has `env`) can assert `wasm_type == wasm_type_of_repr(repr, mono, env)`.
Keep `eligible_b` for builder handles (not durable typed-vector value slots).

- [ ] **Step 3: Return the materialized plan from route.**

Extend the existing `RoutedModule` record type (`route_typed_vec.tw:96`, currently `.{ funcs, swapped_sites }`) to include `plan: phys_plan.PhysPlan`:

```tw
pub type RoutedModule = .{ funcs: Vector<PreparedFunc>, swapped_sites: Vector<SwappedSetSite>, plan: phys_plan.PhysPlan }
```

There is no `RouteResult` type — `route_typed_vectors` returns `RoutedModule`, and `prepare.tw` already reads `routed.funcs` / `routed.swapped_sites`. Thread the accumulated `func_plan` through the fold over functions so the returned literal (line 149) carries `plan`. `prepare.tw` binds `routed.plan` even if later tasks do not consume it yet.

- [ ] **Step 4: Test the invariant holds after route.**

Add a route suite test that compiles a typed-vector producer, runs route, and asserts every routed slot satisfies `wasm_type == wasm_type_of_repr(repr, mono)` — in particular that a typed slot has `repr = TypedVec(elem)` (not `TypedRef`) and `wasm_type = pvec_wasm_type(elem)`, and a matching `slots` entry in `routed.plan`. Cover an i64 producer at minimum; add a non-i64 family producer if a fixture exists.

- [ ] **Step 5: Format, rebuild, test.**

Run:

```bash
target/twk fmt boot/compiler/backend/phys_plan.tw boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/prepare.tw boot/tests/suites/route_typed_vec_suite.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point, no lint findings, boot tests pass.

- [ ] **Step 6: Commit.**

```bash
git add boot/compiler/backend/phys_plan.tw boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/prepare.tw boot/tests/suites/route_typed_vec_suite.tw
git commit -m "backend: materialize typed-vector slot repr plan during routing"
```

---

## Task 6: Make `mutvec_repr` consume route's physical ownership explicitly

**Files:**
- Modify: `boot/compiler/backend/mutvec_repr.tw`
- Modify: `boot/compiler/backend/prepare.tw`
- Modify: `boot/tests/suites/mutvec_region_suite.tw` or add a focused backend repr test

**Interfaces:**
- Consumes: routed `PhysPlan` from Task 5.
- Produces: comments and checks that `mutvec_repr` only changes `MutVec*` handle slots and never changes a durable typed-vector slot recorded by route.

> **Handle reprs are already derived.** `mutvec_repr` sets a handle slot's `repr` to
> `ReprKind.MutVec(elem)`, whose `wasm_type_of_repr` arm is already live
> (`mutvec_wasm_type`). So the invariant already holds for handle slots — this task
> only adds the guard that mutvec never touches a route-owned `TypedVec` slot. If any
> mutvec site still writes `wasm_type` directly, switch it to set `repr = MutVec(elem)`
> and derive, same as route.

- [ ] **Step 1: Update `assign_mutvec_reprs` signature to accept the route plan.**

Change:

```tw
pub fn assign_mutvec_reprs(funcs: Vector<PreparedFunc>, env: ResolvedEnv, builtins: BuiltinRegistry) Vector<PreparedFunc>
```

to:

```tw
pub fn assign_mutvec_reprs(funcs: Vector<PreparedFunc>, env: ResolvedEnv, builtins: BuiltinRegistry, plan: phys_plan.PhysPlan) Vector<PreparedFunc>
```

Import `compiler.backend.phys_plan`.

- [ ] **Step 2: Guard against clobbering route-owned typed slots.**

Inside the slot loop, compute:

```tw
route_typed := phys_plan.slot_repr(plan, pf.func_id.id, info.slot.id) != .None
```

Then keep the current handle override behavior, but if a slot is both `route_typed` and a handle candidate, fail with a clear diagnostic:

```tw
if route_typed and handle.has(info.slot.id) {
  error("mutvec_repr: slot S${info.slot.id.to_string()} is both route-owned Typed and MutVec handle in ${pf.name}")
}
```

A freeze-result slot should be `route_typed` and not a handle after the tactical fix. A live handle should be `handle` and not `route_typed`.

- [ ] **Step 3: Thread from prepare.**

In `prepare.tw`, pass `routed.plan` into `assign_mutvec_reprs`.

- [ ] **Step 4: Add or extend a regression test.**

Use an existing mutvec fixture shape and assert after prepare that:

- at least one slot has wasm type `rt_types__MutVecI64` for the live handle;
- the freeze-result / return slot has wasm type `rt_types__PVecI64` when route claims it;
- no slot marked as route-owned `Typed` is rewritten to `MutVec*`.

Prefer a prepared-IR inspection test over WAT substring matching for this task.

- [ ] **Step 5: Format, rebuild, test.**

Run:

```bash
target/twk fmt boot/compiler/backend/mutvec_repr.tw boot/compiler/backend/prepare.tw boot/tests/suites/mutvec_region_suite.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
TWINKLE_MUTVEC=1 target/twk wat boot/tests/suites/fixtures/mutvec_escape_return.tw --func 'esc' --calls 2>&1 | grep -cE 'box_i64|unbox_i64'
```

Expected: fixed point, no lint findings, boot tests pass, WAT count is `0` for the typed `esc` boundary.

- [ ] **Step 6: Commit.**

```bash
git add boot/compiler/backend/mutvec_repr.tw boot/compiler/backend/prepare.tw boot/tests/suites/mutvec_region_suite.tw
git commit -m "backend: make mutvec repr respect route-owned typed slots"
```

---

## Task 7: Move `phys_return` materialization behind `PhysPlan.return_repr`

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw`
- Modify: `boot/tests/suites/typed_param_abi_suite.tw`
- Modify: `boot/tests/suites/route_typed_vec_suite.tw`

**Interfaces:**
- Consumes: `phys_plan.return_repr(plan, func_id)`.
- Produces: `phys_return` set only by applying `PhysPlan.return_repr`, not by a separate `has_phys_return` boolean.

> **`phys_return` is the one `ValType`-without-`ReprKind` site — the invariant does not
> reach it.** `PreparedFunc.phys_return` is a raw return-ABI `ValType` override with no
> companion slot `repr`, so the per-slot invariant (Task 8 Step 1) does not and cannot
> assert it. Consistency instead comes from *derivation*: it is set to
> `pvec_wasm_type(elem)` for the plan's `returns` `ElemRepr`, and its correctness at the
> call/return boundary is checked by the **coercing-edge** verify (Task 8), not the
> invariant. Keep it derived from the plan entry — never a hand-written `ValType` — so
> "one repr vocabulary" still holds for returns even though the field itself carries no
> `repr`.

- [ ] **Step 1: Replace `has_phys_return` boolean use in route application.**

Keep `compute_eligible_v` returning `has_phys_return` for source discovery in this task, but when setting the prepared function ABI, drive both the decision **and** the
physical type off the plan entry — so a non-i64 typed return gets its own family PVec:

```tw
case phys_plan.return_repr(func_plan, pf.func_id.id) {
  .Some(elem) => pf.phys_return = .Some(pvec_wasm_type(elem)),
  .None => {},
}
```

If the plan currently lacks a typed return entry but `has_phys_return` is true, add it
before applying, deriving the family from the return type:

```tw
if has_phys_return {
  case candidate_typed_vec_family(pf.return_mono) {
    .Some(elem) => func_plan.returns["${pf.func_id.id}"] = elem,
    .None => {},
  }
}
```

- [ ] **Step 2: Add consistency test.**

Add a route suite assertion that whenever a routed function has a typed `phys_return`, `phys_plan.return_repr(routed.plan, func_id)` is `.Some(elem)` and `pvec_wasm_type(elem)` equals the `phys_return` `ValType` (same family PVec). Cover at least one non-i64 family so the family round-trips, not just i64.

- [ ] **Step 3: Format, rebuild, test.**

Run:

```bash
target/twk fmt boot/compiler/backend/route_typed_vec.tw boot/tests/suites/typed_param_abi_suite.tw boot/tests/suites/route_typed_vec_suite.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point, no lint findings, boot tests pass.

- [ ] **Step 4: Commit.**

```bash
git add boot/compiler/backend/route_typed_vec.tw boot/tests/suites/typed_param_abi_suite.tw boot/tests/suites/route_typed_vec_suite.tw
git commit -m "backend: materialize typed return ABI from PhysPlan"
```

---

## Task 8: Verify the invariant, then physical-edge checks

**Files:**
- Modify: `boot/compiler/backend/verify_slots.tw` (invariant)
- Modify: `boot/compiler/backend/verify_expr.tw`, `boot/compiler/backend/verify_common.tw` (edge skeleton)
- Modify: `boot/tests/suites/typed_record_fields_suite.tw`
- Modify: `boot/tests/suites/backend_repr_suite.tw`

**Interfaces:**
- Consumes: `SlotInfo.{repr, mono, wasm_type}` (invariant); physical `ValType`s at each edge (edge skeleton — never reads the `PhysPlan` object).
- Produces: the standing invariant assertion + verifier failures for uncoerced physical mismatches on non-coercing edges.

- [ ] **Step 1: Assert the invariant `wasm_type == wasm_type_of_repr(repr, mono)`.**

In `verify_slots.tw`, for every slot, assert
`slot.wasm_type == wasm_type_of_repr(slot.repr, slot.mono, env)`. This is the primary,
type-agnostic guarantee: once route derives `wasm_type` from `repr` (Task 5) it holds
for typed-vector slots by construction, and thereafter catches *any* pass that writes
one without the other (the entire stale-slot / clobber class). A `ValType` equality
helper already exists (`val_type_eq` / structural compare in `verify_common.tw`); reuse
it.

> **The invariant is asserted over EVERY slot — audit the direct `wasm_type` writers
> first.** It is only "low risk / land hard directly" if *every* pass that writes
> `wasm_type` independently today already produces a value that equals
> `wasm_type_of_repr(repr, mono, env)` for that slot's `repr`. Direct writers exist
> outside the typed-vector route path — at minimum `slot_assign.tw` (`.Anyref` builder/
> placeholder slots, ~lines 68/91/129), `route_typed_vec.tw:629` (`eligible_b`
> `OpaqueAnyref`/`.Anyref`), and any record/dict/closure slot construction in
> `repr_assign.tw`. Before landing the hard check: (1) `rg -n 'wasm_type\s*[:=]'
> boot/compiler` and confirm each writer's `repr` maps back to the same `ValType`; and
> (2) run the assertion **report-only for one full suite + self-host pass** and inventory
> every slot that fires. Only flip to a hard `error` once that inventory is empty — a
> single mismatched pre-existing writer would otherwise break `make boot-test` on
> landing, which is not "passes immediately."

- [ ] **Step 2: Add the physical-repr edge-skeleton + vector coercion capability.**

> **Report-only first.** Unlike the invariant, the edge-skeleton can *reject
> currently-valid modules*: a strict rejection can trip on an edge that relies on
> emit's coercion. So: (1) land it **report-only** (`eprintln`, not `error`) behind
> `TWINKLE_VERIFY_VEC_REPR=1`; (2) run the full suite + self-host and inventory every
> edge that fires — confirm each is a real bug, not an emit-coerced edge; (3) only
> then flip the confirmed non-coercing edge classes to hard `error`. Not in the same
> commit that introduces the check.

The edge check operates on physical `ValType` edges plus an edge-class-aware
`is_declared_coercion` capability — never on `ReprKind`. A future dict/record customer
reuses it with its own predicate.

In `verify_common.tw`, add a classifier (for readable diagnostics) and the
type-agnostic edge check driven by a capability:

```tw
use compiler.elem_family.{family_by_pvec_name}

// TypedPVec carries the family mono_key so cross-family edges (PVecI64 vs PVecF64)
// are distinguishable. MutVec covers every rt_types__MutVec* handle type.
pub type VecPhys = { NotVector, BoxedPVec, TypedPVec(String), MutVec }

// Whether the emitter inserts a coercion at this edge. Boxed <-> typed is legal on a
// Coercing edge (emit boxes/unboxes) and a DEFECT on a NonCoercing one (no coercion
// is emitted — the pair reaches the Wasm validator as a type error).
pub type EdgeClass = { Coercing, NonCoercing }

pub fn vec_phys_of_val_type(vt: ValType) VecPhys {
  case vt {
    .Ref(_, ht) => case ht {
      .Named(n) => cond {
        // MutVec* first: "rt_types__PVec" is a prefix of the typed PVec names, so
        // check the exact boxed name, not a prefix.
        n.starts_with("rt_types__MutVec") => .MutVec,
        n == "rt_types__PVec" => .BoxedPVec,
        _ => case family_by_pvec_name(n) {
          .Some(f) => .TypedPVec(f.mono_key),
          .None => .NotVector,
        },
      },
      _ => .NotVector,
    },
    _ => .NotVector,
  }
}

// The reusable edge-skeleton. `is_declared_coercion` takes the edge class: a
// boxed <-> typed pair is a declared coercion ONLY on a Coercing edge (call arg /
// return / variant construct+extract — emit inserts that family's box/unbox). On a
// NonCoercing edge (record-field read/store, capture store, local copy/assign) the
// emitter inserts nothing, so boxed <-> typed is a real defect. Two *different* typed
// families are never coercible on any edge (PVecI64 vs PVecF64 is a bug), and MutVec*
// edges are produced by the freeze producer, not by a copy edge.
pub type ReprEdgeCap = .{ is_declared_coercion: fn(from: ValType, to: ValType, edge: EdgeClass) Bool }

pub fn vec_repr_edge_cap() ReprEdgeCap {
  .{
    is_declared_coercion: fn(from, to, edge) {
      case edge {
        .NonCoercing => false,
        .Coercing => {
          f := vec_phys_of_val_type(from)
          t := vec_phys_of_val_type(to)
          // exactly one boxed and one typed family PVec
          (is_boxed(f) and is_typed_pvec(t)) or (is_typed_pvec(f) and is_boxed(t))
        },
      }
    },
  }
}

fn is_boxed(p: VecPhys) Bool {
  case p {
    .BoxedPVec => true,
    _ => false,
  }
}

fn is_typed_pvec(p: VecPhys) Bool {
  case p {
    .TypedPVec(_) => true,
    _ => false,
  }
}

// Two VecPhys are the same physical class iff same variant AND, for typed, same family.
fn vec_phys_eq(a: VecPhys, b: VecPhys) Bool {
  case a {
    .NotVector => case b { .NotVector => true, _ => false },
    .BoxedPVec => case b { .BoxedPVec => true, _ => false },
    .MutVec => case b { .MutVec => true, _ => false },
    .TypedPVec(fa) => case b {
      .TypedPVec(fb) => fa == fb,
      _ => false,
    },
  }
}

// Returns .Some(message) when the edge is an illegal physical mismatch for its class.
// `val_type_name` (already in verify_common.tw) stringifies a ValType — it is NOT
// Stringify, so do not use `${vt}` / `vt.to_string()`.
pub fn check_repr_edge(from: ValType, to: ValType, edge: EdgeClass, cap: ReprEdgeCap, ctx: String) String? {
  fp := vec_phys_of_val_type(from)
  tp := vec_phys_of_val_type(to)
  cond {
    // Not both vector physical types → not this check's concern.
    is_not_vector(fp) or is_not_vector(tp) => .None,
    // Equal physical class (same family, incl. MutVec == MutVec) is fine.
    vec_phys_eq(fp, tp) => .None,
    cap.is_declared_coercion(from, to, edge) => .None,
    _ => .Some("physical vector repr mismatch at ${ctx}: producer ${val_type_name(from)} vs destination ${val_type_name(to)}"),
  }
}

fn is_not_vector(p: VecPhys) Bool {
  case p {
    .NotVector => true,
    _ => false,
  }
}
```

> **This skeleton generalizes the existing per-edge checks; it does not replace
> them.** `verify_expr.tw` already rejects non-coercing field mismatches
> (`pvec_vt_mismatch`) and gates coercing edges (`can_emit_coerce_stack`). The
> skeleton's job is to give those a single edge-class-aware classifier and to cover
> the local copy/assign edges that have no check yet — passing `.NonCoercing` where
> the existing code hard-errors and `.Coercing` where it already coerces. Do not
> route an edge through the skeleton with the wrong class, and do not weaken an
> existing hard error into a skeleton `.Coercing` acceptance.

- [ ] **Step 3: Check local copy/storage edges (NonCoercing).**

In `verify_expr.tw`, extend existing slot/value verification so `AInit(atom)` and
`AAssign(target, atom)` call `check_repr_edge(atom_vt, dest_slot_wasm_type,
.NonCoercing, vec_repr_edge_cap(), "AInit"/"AAssign")`. A returned `.Some(msg)` is a
candidate defect: local copy/assign is non-coercing, so a `BoxedPVec` vs
`TypedPVec(fam)` mismatch — or a cross-family `TypedPVec` vs `TypedPVec` mismatch — is
a bug. **Report it (report-only mode) rather than `error` on first landing** (see the
Step 2 warning), gated behind `TWINKLE_VERIFY_VEC_REPR=1`; only escalate to a hard
failure once the suite/self-host inventory confirms no valid edge relies on it.

- [ ] **Step 4: Check record-get result edge (NonCoercing).**

For `ARecordGet(base, field, type_id)`, call `check_repr_edge(field_layout_vt,
result_slot_vt, .NonCoercing, vec_repr_edge_cap(), "ARecordGet")` — fields insert no
coercion, so this must stay non-coercing, matching the existing `pvec_vt_mismatch`
hard error. (Call-arg / return / variant construct+extract edges are `.Coercing`;
they already gate on `can_emit_coerce_stack` and need no new check.)

- [ ] **Step 5: Add negative tests using existing verifier test patterns.**

Create a prepared fixture in an existing backend suite that forces a `PVecI64` producer into a boxed destination slot without a coercion edge and assert verification rejects with a message containing `physical vector repr mismatch`. Also add a positive test that the invariant check (Step 1) passes on a normal typed-vector module.

- [ ] **Step 6: Format, rebuild, test.**

Run:

```bash
target/twk fmt boot/compiler/backend/verify_expr.tw boot/compiler/backend/verify_common.tw boot/tests/suites/typed_record_fields_suite.tw boot/tests/suites/backend_repr_suite.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point, no lint findings, boot tests pass.

- [ ] **Step 7: Commit.**

```bash
git add boot/compiler/backend/verify_slots.tw boot/compiler/backend/verify_expr.tw boot/compiler/backend/verify_common.tw boot/tests/suites/typed_record_fields_suite.tw boot/tests/suites/backend_repr_suite.tw
git commit -m "verify: assert wasm_type==f(repr,mono) invariant + edge-class repr checks"
```

---

## Task 9: Retire the return/capture legacy parameters (reach the plan-owned end state)

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw`
- Modify: `boot/compiler/backend/prepare.tw`
- Modify: direct test callers under `boot/tests/suites/`

**Interfaces:**
- Produces: `route_typed_vectors(funcs, builtins, plan, typed_fields, typed_payloads)` — the func-id-keyed facts (returns, captures) now flow *only* through `plan`.

Without this task the return/capture facts stay dual-carried (plan **plus** the two
legacy maps) and the plan-owned goal for ABI sites isn't reached. `typed_fields` /
`typed_payloads` **stay** — they are the field/payload eligibility gate, whose family
is per-instantiation and not in the plan (Option A). Only the two func-id-keyed maps
the plan subsumes are removed.

- [ ] **Step 1: Confirm route reads the plan for returns/captures.**

Verify (from Task 4) that every return/capture decision site queries `plan` — via a
point query (`phys_plan.return_repr` / `phys_plan.capture_repr`) or a plan-derived
local — and that the `capture_abi` and `typeable_return` parameters are no longer read
anywhere in `route_typed_vec.tw`. Grep both names to prove non-use. `typed_fields` /
`typed_payloads` are still read (unchanged) — that is expected, not a miss.

- [ ] **Step 2: Narrow the signature.**

Remove the two now-dead map parameters from `route_typed_vectors`, leaving:

```tw
pub fn route_typed_vectors(
  funcs: Vector<PreparedFunc>,
  builtins: BuiltinRegistry,
  plan: phys_plan.PhysPlan,
  typed_fields: Dict<String, Bool>,
  typed_payloads: Dict<String, Bool>,
) RoutedModule {
```

Delete the transitional `typeable_return_from_plan` / `capture_abi_from_plan`
adapters (any decision site kept on the map shape must first move to a point query).
Remove any debug projection-equality warnings added in Task 4.

- [ ] **Step 3: Update `prepare.tw` and callers.**

`prepare.tw` passes `phys := typed_param_abi.phys_plan_from_analysis(repr)` plus the
still-needed `repr.typed_fields` / `repr.typed_payloads`; `repr.param_abi.typeable_return`
and `repr.capture_abi` are no longer forwarded to routing (they remain available to
analysis). Update every direct test caller found via `rg -n "route_typed_vectors\(" boot`
to the new form, building a `PhysPlan` (via `phys_plan.empty()` or
`phys_plan_from_analysis`) as needed.

- [ ] **Step 4: Format, rebuild, test.**

Run:

```bash
target/twk fmt boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/prepare.tw boot/tests/suites/*.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point, no lint findings, boot tests pass. Byte-identical self-host
output vs the pre-narrowing commit confirms the removal was purely structural.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/prepare.tw boot/tests/suites
git commit -m "backend: route consumes PhysPlan for returns/captures; drop those legacy maps"
```

---

## Task 10: Document ownership boundaries and retire stale comments

**Files:**
- Modify: `docs/plans/performance/vector/typed-vector-representation.md`
- Modify: `docs/plans/performance/vector/boundary-tracklist.md`
- Modify: `docs/plans/performance/vector/README.md`
- Modify: comments in `boot/compiler/backend/prepare.tw`, `route_typed_vec.tw`, `mutvec_repr.tw`, `typed_param_abi.tw`

**Interfaces:**
- Produces: human-readable ownership contract for future work.

- [ ] **Step 1: Add the ownership contract to typed-vector docs.**

Append a section:

```md
## Physical representation ownership contract

- Three layers, one pipeline: `MonoType` (semantic) → `ReprKind` (physical repr) → `ValType`. `ReprKind` is the single repr vocabulary — `TypedVec(ElemRepr)` is live, and `ValType` is a pure function of it via `wasm_type_of_repr`. The invariant **`SlotInfo.wasm_type == wasm_type_of_repr(repr, mono)`** holds at every slot.
- `route_typed_vec` **decides** the durable typed-vector physical sites (its `compute_eligible_v` fixpoint) and **applies** them by setting `repr = TypedVec(elem)` and deriving `wasm_type`; it never writes `wasm_type` independently.
- `PhysPlan` is the **prepare-time decision record** (an `ElemRepr` per func-id-keyed site: slots, returns, captures — each single-family because functions monomorphize). Downstream reads `SlotInfo.repr` / `wasm_type` (consistent by the invariant), not the plan object.
- Record-field / variant-payload physical family is **not** in `PhysPlan`. It is Layer-1 structural, owned by `wasm_layout`, derived per-instantiation from the concrete `(TypeId, type args)` — a `(TypeId, field)` key cannot name it. Field/payload *eligibility* lives in the `typed_fields` / `typed_payloads` presence maps.
- `typed_param_abi` computes use/support facts and the return/capture PhysPlan projection; it does not mutate slots.
- `mutvec_repr` owns only temporary `MutVec(ElemRepr)` handles (every family in `mutvec_families()`), setting `repr` and deriving `wasm_type` the same way. MutVec freeze results are typed producers in route, not frozen-slot overrides in mutvec_repr.
- `emit` reads `repr` / `wasm_type` (trustworthy by the invariant) and inserts coercions only at declared boundaries. It re-derives **nothing** from `MonoType`; picking a family from a concrete element type in `wasm_layout` is permitted Layer-1 structural derivation.
- `verify` asserts the invariant per slot, and rejects physical mismatches per edge class (non-coercing edges hard-fail; coercing edges must have an emitter coercion) before Wasm validation.
```

- [ ] **Step 2: Update vector README.**

Add this plan to the living docs table with role: “Long-term cleanup for mono/use vs physical repr/ABI decision ownership.”

- [ ] **Step 3: Remove stale comments.**

Search:

```bash
rg -n "TypedVec is inert|freeze result|phys_return|route owns|mutvec_repr|PVecI64" boot/compiler/backend docs/plans/performance/vector
```

Update comments that imply `mutvec_repr` owns freeze-result `PVecI64` or that `route_typed_vec` is merely a bolt-on without a plan.

- [ ] **Step 4: Format docs-adjacent Twinkle files and lint.**

Run:

```bash
target/twk fmt boot/compiler/backend/prepare.tw boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/mutvec_repr.tw boot/compiler/backend/typed_param_abi.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point, no lint findings, boot tests pass.

- [ ] **Step 5: Commit.**

```bash
git add docs/plans/performance/vector/typed-vector-representation.md docs/plans/performance/vector/boundary-tracklist.md docs/plans/performance/vector/README.md boot/compiler/backend/prepare.tw boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/mutvec_repr.tw boot/compiler/backend/typed_param_abi.tw
git commit -m "docs: define typed-vector physical repr ownership boundaries"
```

---

## Task 11: Emit typedness-audit and gate (the safety-net removal)

**Files:**
- Modify: `boot/compiler/codegen/emit.tw` (only where an audit finds a re-derivation)
- Modify: `boot/tests/suites/backend_repr_suite.tw`

**Interfaces:**
- Consumes: prepared `SlotInfo.wasm_type` / `phys_return` (the materialized physical
  facts — emit does not read the `PhysPlan` object), and the strict verifier
  edge-skeleton from Task 8.
- Produces: an emit that reads prepared physical types and never re-derives typed-vector
  *eligibility* from `MonoType`.

This is sequenced **last on purpose**: it is the only step that touches the coercion
safety net, and the doctrine's target — "emit inserts coercions only at declared
boundaries and never re-derives typedness" — is what lets the verifier be trusted.
**Gate:** do not start until Task 8's edge-skeleton has been flipped to strict
`error` and is green across `make boot-test` + self-host fixed point. With the
verifier strict, any emit change that drops a needed coercion is caught immediately;
without it, this task is flying blind.

Note: emit today is mostly the coercion *applier* (it inserts `box_i64`/`unbox_i64`
at physical mismatches) and mostly *reads* physical types. Keep the coercion
insertion — the doctrine wants it. The audit targets only sites that recompute
*whether a vector site is typed* from `MonoType` instead of reading the prepared
`SlotInfo`/`PhysPlan`. The task may legitimately conclude "no re-derivation found"
and close as a documented confirmation.

- [ ] **Step 1: Audit emit for typedness re-derivation.**

Search emit for sites that decide typed-vector physical form from semantic type
rather than prepared physical type:

```bash
rg -n "PVec(I64|Bool|F64|Byte)|val_type_of_mono|Vector\(\.(Int|Bool|Float|Byte)\)|elem_family_of|is_typed|TypedVec" boot/compiler/codegen/emit.tw
```

For each hit, classify it: **(a) reads prepared type** (`SlotInfo.wasm_type`,
`phys_return`, atom val type) → leave as-is; **(b) applies a declared coercion**
(`box_i64`/`unbox_i64` at a physical mismatch) → leave as-is, the doctrine keeps it;
**(c) re-derives eligibility** (recomputes typed-vs-boxed from `MonoType`/mono keys
where a prepared physical type is already available) → this is the removal target.
Record the classification inline in a comment at each (c) site.

`emit/runtime_abi.tw` carries per-family builder lists (one hardcoded list per
`_i64`/`_bool`/`_f64`/`_byte` family). These are legitimate family dispatch, **not**
a re-derivation target — leave them. But keep them in view during the audit: if a
site indexes those lists off a `MonoType`-derived family where a prepared physical
type (slot `wasm_type`) is already available, that lookup is a class-(c) site like any
other and moves to the prepared-type read.

**`wasm_layout.tw` is out of scope — do not audit or "fix" it.** Its per-instantiation
`elem_family_of(subst_type_params(...))` for record fields / variant payloads is
Layer-1 structural derivation on a concrete type, and is the *only* place that gets a
generic multi-family vector field right (a `(TypeId, field)` key cannot). Rewriting it
into a `PhysPlan`/`SlotInfo` read would break generics. The audit is `emit.tw` only.

- [ ] **Step 2: Replace each re-derivation with a `repr` / `wasm_type` read.**

For every class-(c) site, replace the `MonoType`-driven decision with a read of the
slot's `repr` / `wasm_type` (or the callee `phys_return`) — now trustworthy by the
invariant. Unlike the additive version of this plan, there *is* a re-derivation to
remove: emit re-derives today precisely because `repr` was untrustworthy before
Task 5. With the invariant in place these sites are dead branches; delete them. If a
site turns out to already read the prepared type, leave it.

- [ ] **Step 3: Add a regression fixture.**

In `backend_repr_suite.tw`, add a prepared-IR fixture where a typed producer flows to
a consumer and assert emit selects the typed op purely from prepared physical types
(no dependence on a re-derived mono decision). Prefer prepared-IR inspection over WAT
substring matching.

- [ ] **Step 4: Format, rebuild, validate under the strict verifier.**

Run (with the Task 8 verifier strict, i.e. no report-only flag needed):

```bash
target/twk fmt boot/compiler/codegen/emit.tw boot/tests/suites/backend_repr_suite.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point, no lint findings, boot tests pass, and the strict verifier
raises no new physical-repr mismatch (proving no coercion was dropped).

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/codegen/emit.tw boot/tests/suites/backend_repr_suite.tw
git commit -m "emit: read prepared vector physical types, stop re-deriving typedness"
```

---

## Task 12: Final validation and performance guard

**Files:**
- Modify only if validation reveals stale docs or comments.

- [ ] **Step 1: Run full compiler validation.**

Run:

```bash
make bundle-cli
make boot-test
make rust-test
```

Expected: self-host fixed point, boot tests pass, Rust tests pass.

- [ ] **Step 2: Run typed-vector WAT boundary checks.**

Use the committed MutVec escape-return fixture from the tactical fix:

```bash
TWINKLE_MUTVEC=1 target/twk wat boot/tests/suites/fixtures/mutvec_escape_return.tw --func 'esc' --calls 2>&1 | grep -cE 'box_i64|unbox_i64'
TWINKLE_MUTVEC=1 target/twk wat boot/tests/suites/fixtures/mutvec_escape_return.tw --func 'read_typed' --calls 2>&1 | grep -E 'esc|len_i64|box_i64|unbox_i64'
TWINKLE_MUTVEC=1 target/twk wat boot/tests/suites/fixtures/mutvec_escape_return.tw --func 'via_sink' --calls 2>&1 | grep -cE 'box_i64'
TWINKLE_MUTVEC=1 target/twk wat boot/tests/suites/fixtures/mutvec_escape_return.tw --func 'via_sink' --calls 2>&1 | grep -cE 'unbox_i64'
```

Expected:

- `esc` box/unbox count is `0`.
- `read_typed` has no whole-vector `box_i64` or `unbox_i64`.
- `via_sink` has exactly one `box_i64` and zero `unbox_i64`.

- [ ] **Step 3: Multi-family correctness (self-host does not cover this).**

Self-host output is predominantly i64, so a wrong bool/f64/byte family tag or a
`pvec_wasm_type` mismatch would not perturb the fixed point. The multi-family unit
tests (Tasks 2/5/7) are the real guard. Add **one runnable** end-to-end fixture that
actually executes — a `Vector<Float>` (or `Vector<Bool>`) typed return threaded
through a caller, plus a typed capture of the same — and assert it computes the right
result *and* emits the family PVec (`PVecF64`/`PVecBool`), not `PVecI64` and not a
reboxed `PVec`. A prepared-IR/WAT assertion alone is not enough here; run the program.

- [ ] **Step 4: Run vector performance smoke checks.**

Run:

```bash
target/twk run examples/performance/sort-bench/typed_vec_read_probe.tw
target/twk run examples/performance/sort-bench/merge_attribution_probe.tw
target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw
```

Expected: checks complete successfully; timings remain in the same performance class as the pre-refactor route-based implementation. Record any material regression in `docs/plans/performance/vector/boundary-tracklist.md` before continuing.

- [ ] **Step 5: Final diff review.**

Run:

```bash
git diff --stat
git diff -- boot/compiler/backend/phys_plan.tw boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/typed_param_abi.tw boot/compiler/backend/mutvec_repr.tw boot/compiler/backend/verify_expr.tw boot/compiler/backend/verify_common.tw boot/compiler/backend/prepare.tw
```

Check that:

- route sets `repr = TypedVec(elem)` and derives `wasm_type` (no independent `wasm_type` write);
- the invariant `wasm_type == wasm_type_of_repr(repr, mono)` is asserted in verify and holds;
- mutvec_repr only owns `MutVec(elem)` handles;
- emit no longer re-derives typedness from `MonoType` (the safety net is gone);
- verifier errors mention physical producer/destination reprs.

- [ ] **Step 6: Commit final validation notes if docs changed.**

```bash
git add docs/plans/performance/vector
if ! git diff --cached --quiet; then
  git commit -m "docs: record physical repr planner validation"
fi
```

---

## Risks & rollback

- **The repr-layer change must be byte-identical.** Reviving `TypedVec` means `wasm_type_of_repr(.TypedVec(elem))` = `pvec_wasm_type(elem)` must equal exactly the `ValType` route wrote directly before (`fi.fam.pvec_type`). Confirm the `ElemRepr → PVec name` map matches `ElemFamily.pvec_type` for all four families before Task 5. If they ever diverge, every typed slot's `wasm_type` shifts and output is no longer identical.
- **Scope creep into typed parameter ABI.** This plan restores the invariant and materializes current slot/return/capture decisions (and leaves field/payload eligibility where it is). It does not add named-function typed parameter ABI. If a task starts specializing function params, stop and split a separate typed-param ABI plan.
- **Field/payload family is not the plan's.** Do not fold record-field / variant-payload physical family into `PhysPlan`. It is per-instantiation, owned by `wasm_layout`; a `(TypeId, field)` key cannot name it, and a projection that tried would mis-tag generic multi-family types. `PhysPlan` is func-id-keyed only.
- **Fixpoint performance.** Folding more facts into a materialized plan can increase prepare cost. This plan projects the return/capture facts and materializes route's slot result. Do not move source categories into a multi-round `PhysPlan` fixpoint until boot compile timings are measured.
- **Verifier false positives / edge-class.** Add checks only for edges whose emitter behavior is known, and always pass the right `EdgeClass`. Non-coercing local/copy/record-get edges hard-fail on a boxed↔typed mismatch; call/return/variant edges are coercing. A miscategorized edge either masks a real bug or rejects valid code.
- **MutVec regression.** The tactical fix depends on route recognizing `mutvec_freeze_i64` and mutvec_repr not clobbering freeze slots. Keep the WAT checks in Task 12 as a regression gate.
- **Emit safety-net removal (Task 11).** Deleting the typedness re-derivation is gated on the invariant (Task 5) and the edge verifier (Task 8) being green. With `repr` trustworthy the re-derivation is dead code; still, if a site is ambiguous (declared coercion to keep vs re-derivation to remove), leave it — a missed removal is harmless, a wrong one drops a coercion. Never run Task 11 while the edge verifier is still report-only. `wasm_layout.tw` is explicitly out of the audit.
- **Stopping at dual-carrying.** Tasks 2–7 run the plan *alongside* the legacy return/capture maps for behavior-preserving safety. That is transitional. If Task 9 (legacy-map removal) is skipped, the plan-owned objective for ABI sites is not met and the round-trip scaffolding from Task 4 becomes permanent debt. Treat Task 9 as load-bearing.
- **Edge verifier can reject valid code.** The Task 8 *edge-skeleton* (not the invariant check) is the only piece that can break a currently-passing build; it lands report-only behind a flag and is inventoried before any hard rejection. The invariant check is low-risk and lands hard directly.
- **Rollback.** Each task is behavior-preserving or tightly gated. Revert the latest task commit if a check rejects valid current code. Reverting Task 1 leaves `TypedVec` live but unconstructed (still inert in practice) plus an unused plan model; reverting Task 5 restores the direct `wasm_type` write. No revert changes language semantics.

## Out of scope

- Adding typed ABI for normal named-function parameters.
- Making `Vector<Int>` globally `PVecI64` by default.
- Adding typed helper ABIs for every vector combinator.
- Replacing emit coercions with an explicit prepared-IR `ACoerce` op.
- Reworking non-vector physical representations.
