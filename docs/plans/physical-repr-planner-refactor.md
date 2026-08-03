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
> ownership doctrine. Read it first; it fixes the invariants this plan realizes
> (single materialized `PhysPlan`; `route` the only mutator of durable reprs while
> `mutvec_repr` owns transient `MutVecI64` handles; verify on physical `ValType`
> edges via an `is_declared_coercion` capability; emit coerces-only). The substrate
> is **concrete** (`PhysRepr = { Boxed, TypedI64 }`), not generic — dict/record are
> documented, uncoded extension points there.

**Goal:** Refactor typed-vector physical representation decisions so mono/use analysis, physical repr/ABI planning, slot mutation, and emission verification have explicit ownership boundaries instead of drifting across `typed_param_abi`, `route_typed_vec`, `mutvec_repr`, and emit.

**Architecture:** Keep the current route-based tactical behavior intact while extracting a first-class `PhysPlan` layer that represents physical slot/site/ABI facts as data. Existing analyses continue to derive semantic/use eligibility; `route_typed_vec` becomes the applicator of a materialized physical plan; `mutvec_repr` remains responsible only for live `MutVecI64` handles; emit remains a coercion safety net and stops re-deriving typedness. This is the long-term cleanup following the MutVec escape-return tactical fix, which has **already landed on this branch** (`d51a9568` "keep escaping regions PVecI64 across the return (no rebox)") and proves `mutvec_freeze_i64` belongs in the typed-producer source framework. The precondition is met; this plan builds on it.

**Tech Stack:** Boot compiler (Twinkle in `boot/`), backend prepared IR (`boot/compiler/backend/`), typed-vector routing (`route_typed_vec.tw`), cross-function repr analysis (`typed_param_abi.tw`), physical verification (`verify_expr.tw` / `verify_slots.tw`), Wasm-GC emission (`boot/compiler/codegen/emit*.tw`).

## Global Constraints

- Treat `MonoType` and physical repr as related but orthogonal: `MonoType.Vector(.Int)` may be boxed `PVec`, typed `PVecI64`, or temporary `MutVecI64` depending on site.
- Do not make `Vector<Int>` globally typed. Typed vectors remain per-storage-site / per-ABI-edge decisions with explicit box/unbox at durable erased boundaries.
- Preserve the tactical MutVec escape-return fix: `route_typed_vec` recognizes `vector$__mutvec_freeze_i64` as a typed producer; `mutvec_repr` does not override freeze-result slots.
- Prefer additive refactors with behavior-preserving tests before changing routing decisions.
- **End state:** `route` remains the *decider* of typed-vector slot/return eligibility (its `compute_eligible_v` fixpoint), and `PhysPlan` becomes the *materialized, queryable, verifiable record* of those decisions — not a second independent decision engine. The goal is a single artifact every downstream stage reads, and removal of the redundant legacy map parameters (Task 9), not merely adding a plan alongside them.
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

The separation of *decision ownership* is incomplete:

- `typed_param_abi.tw` does whole-program semantic/use analysis, but emits partial physical ABI facts such as `typeable_return`, `capture_abi`, and field/payload typed sets.
- `route_typed_vec.tw` both computes physical slot eligibility and mutates prepared functions: slot retyping, builtin swaps, and `phys_return`.
- `mutvec_repr.tw` runs after route and patches live mutable handles; a stale frozen-slot override showed how late repr mutation can accidentally clobber route's physical decisions.
- `emit` correctly coerces physical mismatches, but because it is a safety net rather than an owner of repr decisions, a misplaced plan fact can move a rebuild to another boundary instead of eliminating it.

The desired long-term state is one materialized physical representation plan. Analyses produce eligibility/support facts; routing applies the plan; verification checks every physical edge; emit performs only declared coercions.

Relevant existing reference: `docs/plans/performance/vector/archive/unify-typedness-oracle-design.md`. That design partially landed earlier; this plan is the cleanup pass that makes the boundary explicit with today's code and the MutVec tactical lesson folded in.

---

## File Structure

**Create:**
- `boot/compiler/backend/phys_plan.tw` — typed-vector physical representation plan types, key helpers, ABI projections, and conversion helpers.
- `boot/tests/suites/phys_plan_suite.tw` — focused tests for key construction, default boxed behavior, projection consistency, and supported repr-to-Wasm mapping.

**Modify:**
- `boot/tests/main.tw` — register `phys_plan_suite`.
- `boot/compiler/backend/typed_param_abi.tw` — convert existing `ReprAnalysis` outputs into `PhysPlan` projections without changing behavior, then gradually query `PhysPlan` for return/capture support.
- `boot/compiler/backend/route_typed_vec.tw` — split source discovery / eligibility / application; consume `PhysPlan` projections instead of ad-hoc maps.
- `boot/compiler/backend/mutvec_repr.tw` — keep only handle override responsibility; update comments to state freeze-result typing is owned by route/plan.
- `boot/compiler/backend/prepare.tw` — thread the plan between analysis, routing, mutvec handle override, and verification.
- `boot/compiler/backend/verify_expr.tw`, `boot/compiler/backend/verify_slots.tw`, `boot/compiler/backend/verify_common.tw` — move toward physical-edge checks against the plan.
- `boot/compiler/codegen/emit*.tw` — only if a site still re-derives typedness after the plan is materialized; otherwise leave emit unchanged.
- `docs/plans/performance/vector/README.md` and `docs/plans/performance/vector/boundary-tracklist.md` — link this plan and record which old typedness-oracle work it supersedes.

---

## Task 1: Introduce `PhysPlan` as a behavior-preserving projection layer

**Files:**
- Create: `boot/compiler/backend/phys_plan.tw`
- Create: `boot/tests/suites/phys_plan_suite.tw`
- Modify: `boot/tests/main.tw`

**Interfaces:**
- Produces:
  - `pub type PhysRepr = { Boxed, TypedI64 }`
  - `pub type PhysPlan = .{ slot_repr: Dict<String, PhysRepr>, field_repr: Dict<String, PhysRepr>, payload_repr: Dict<String, PhysRepr>, return_repr: Dict<String, PhysRepr>, capture_repr: Dict<String, PhysRepr> }`
  - `pub fn empty() PhysPlan`
  - `pub fn slot_key(func_id: Int, slot_id: Int) String`
  - `pub fn field_key(type_id: Int, field_id: Int) String`
  - `pub fn payload_key(type_id: Int, variant_id: Int, index: Int) String`
  - `pub fn abi_key(func_id: Int, index: Int) String`
  - `pub fn slot_repr(plan: PhysPlan, func_id: Int, slot_id: Int) PhysRepr`
  - `pub fn field_repr(plan: PhysPlan, type_id: Int, field_id: Int) PhysRepr`
  - `pub fn payload_repr(plan: PhysPlan, type_id: Int, variant_id: Int, index: Int) PhysRepr`
  - `pub fn return_repr(plan: PhysPlan, func_id: Int) PhysRepr`
  - `pub fn capture_repr(plan: PhysPlan, func_id: Int, index: Int) PhysRepr`
  - `pub fn is_typed_i64(r: PhysRepr) Bool`
  - `pub fn to_pvec_val_type(r: PhysRepr) ValType`

- [ ] **Step 1: Write the plan module skeleton.**

Create `boot/compiler/backend/phys_plan.tw`:

```tw
//! Materialized physical representation plan for typed-vector sites.
//!
//! MonoType answers "what Twinkle type is this?". PhysPlan answers "which Wasm
//! representation does this storage site / ABI edge use?". `Boxed` is the default
//! erased PVec ABI. `TypedI64` is the PVecI64 physical representation for a
//! semantic Vector<Int> site. MutVecI64 is intentionally not a durable plan repr:
//! it is a temporary region handle owned by mutvec_repr.

use compiler.codegen.wasm_ir.{ValType}

pub type PhysRepr = { Boxed, TypedI64 }

pub type PhysPlan = .{
  slot_repr: Dict<String, PhysRepr>,
  field_repr: Dict<String, PhysRepr>,
  payload_repr: Dict<String, PhysRepr>,
  return_repr: Dict<String, PhysRepr>,
  capture_repr: Dict<String, PhysRepr>,
}

pub fn empty() PhysPlan {
  .{
    slot_repr: Dict.new(),
    field_repr: Dict.new(),
    payload_repr: Dict.new(),
    return_repr: Dict.new(),
    capture_repr: Dict.new(),
  }
}

pub fn slot_key(func_id: Int, slot_id: Int) String { "${func_id}:${slot_id}" }
pub fn field_key(type_id: Int, field_id: Int) String { "${type_id}:${field_id}" }
pub fn payload_key(type_id: Int, variant_id: Int, index: Int) String { "${type_id}:${variant_id}:${index}" }
pub fn abi_key(func_id: Int, index: Int) String { "${func_id}:${index}" }

fn repr_or_boxed(m: Dict<String, PhysRepr>, key: String) PhysRepr {
  case m[key] {
    .Some(r) => r,
    .None => .Boxed,
  }
}

pub fn slot_repr(plan: PhysPlan, func_id: Int, slot_id: Int) PhysRepr {
  repr_or_boxed(plan.slot_repr, slot_key(func_id, slot_id))
}

pub fn field_repr(plan: PhysPlan, type_id: Int, field_id: Int) PhysRepr {
  repr_or_boxed(plan.field_repr, field_key(type_id, field_id))
}

pub fn payload_repr(plan: PhysPlan, type_id: Int, variant_id: Int, index: Int) PhysRepr {
  repr_or_boxed(plan.payload_repr, payload_key(type_id, variant_id, index))
}

pub fn return_repr(plan: PhysPlan, func_id: Int) PhysRepr {
  repr_or_boxed(plan.return_repr, "${func_id}")
}

pub fn capture_repr(plan: PhysPlan, func_id: Int, index: Int) PhysRepr {
  repr_or_boxed(plan.capture_repr, abi_key(func_id, index))
}

pub fn is_typed_i64(r: PhysRepr) Bool {
  case r {
    .TypedI64 => true,
    .Boxed => false,
  }
}

pub fn to_pvec_val_type(r: PhysRepr) ValType {
  case r {
    .TypedI64 => .Ref(true, .Named("rt_types__PVecI64")),
    .Boxed => .Ref(true, .Named("rt_types__PVec")),
  }
}
```

- [ ] **Step 2: Add focused tests for keys/defaults.**

Create `boot/tests/suites/phys_plan_suite.tw`:

```tw
use @std.testing.assert as assert
use @std.testing as runner
use compiler.backend.phys_plan

fn test_keys_are_stable() Result<Void, String> {
  try assert.equal(phys_plan.slot_key(7, 3), "7:3")
  try assert.equal(phys_plan.field_key(10, 4), "10:4")
  try assert.equal(phys_plan.payload_key(10, 2, 1), "10:2:1")
  try assert.equal(phys_plan.abi_key(8, 0), "8:0")
  .Ok({})
}

fn test_absent_entries_are_boxed() Result<Void, String> {
  p := phys_plan.empty()
  try assert.is_false(phys_plan.is_typed_i64(phys_plan.slot_repr(p, 1, 2)))
  try assert.is_false(phys_plan.is_typed_i64(phys_plan.field_repr(p, 1, 2)))
  try assert.is_false(phys_plan.is_typed_i64(phys_plan.payload_repr(p, 1, 2, 3)))
  try assert.is_false(phys_plan.is_typed_i64(phys_plan.return_repr(p, 1)))
  try assert.is_false(phys_plan.is_typed_i64(phys_plan.capture_repr(p, 1, 0)))
  .Ok({})
}

fn test_typed_i64_val_type_names_pvec_i64() Result<Void, String> {
  vt := phys_plan.to_pvec_val_type(.TypedI64)
  case vt {
    .Ref(_, ht) => case ht {
      .Named(name) => try assert.equal(name, "rt_types__PVecI64"),
      _ => return .Err("expected named heap type"),
    },
    _ => return .Err("expected ref valtype"),
  }
  .Ok({})
}

pub fn suite() runner.Suite {
  runner
    .suite("physical representation plan")
    .test("keys are stable", test_keys_are_stable)
    .test("absent entries are boxed", test_absent_entries_are_boxed)
    .test("TypedI64 maps to PVecI64", test_typed_i64_val_type_names_pvec_i64)
}
```

- [ ] **Step 3: Register the suite.**

Modify `boot/tests/main.tw`:

```tw
use .suites.phys_plan_suite
```

and add `phys_plan_suite.suite()` in the suite list near other backend suites.

- [ ] **Step 4: Format and run the focused suite through boot tests.**

Run:

```bash
target/twk fmt boot/compiler/backend/phys_plan.tw boot/tests/suites/phys_plan_suite.tw boot/tests/main.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point, no lint findings, boot tests pass.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/backend/phys_plan.tw boot/tests/suites/phys_plan_suite.tw boot/tests/main.tw
git commit -m "backend: introduce physical representation plan data model"
```

---

## Task 2: Project existing typed-field/payload/return/capture facts into `PhysPlan`

**Files:**
- Modify: `boot/compiler/backend/typed_param_abi.tw`
- Modify: `boot/tests/suites/phys_plan_suite.tw`

**Interfaces:**
- Consumes: `phys_plan.PhysPlan`, `phys_plan.empty`, `phys_plan.*_key`.
- Produces: `pub fn phys_plan_from_analysis(r: ReprAnalysis) phys_plan.PhysPlan`.

- [ ] **Step 1: Add projection helper.**

In `boot/compiler/backend/typed_param_abi.tw`, import `phys_plan` and add after `pub type ReprAnalysis`:

```tw
use compiler.backend.phys_plan

pub fn phys_plan_from_analysis(r: ReprAnalysis) phys_plan.PhysPlan {
  p := phys_plan.empty()

  // Project only truthy entries: `typed_fields` / `typed_payloads` are presence
  // maps, but iterate k,v and guard on the value so a stray `false` never
  // mis-projects as typed.
  for key, typed in r.typed_fields {
    if typed {
      p.field_repr[key] = .TypedI64
    }
  }
  for key, typed in r.typed_payloads {
    if typed {
      p.payload_repr[key] = .TypedI64
    }
  }
  for func_key, fam_key in r.param_abi.typeable_return {
    if fam_key == "vec_i64" {
      p.return_repr[func_key] = .TypedI64
    }
  }
  for func_key, captures in r.capture_abi {
    for idx, fam_key in captures {
      if fam_key == "vec_i64" {
        p.capture_repr["${func_key}:${idx}"] = .TypedI64
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

fn test_analysis_projection_sets_abi_and_layout_entries() Result<Void, String> {
  typed_fields: Dict<String, Bool> = Dict.new()
  typed_fields["10:4"] = true
  typed_payloads: Dict<String, Bool> = Dict.new()
  typed_payloads["11:2:0"] = true
  typeable_return: Dict<String, String> = Dict.new()
  typeable_return["7"] = "vec_i64"
  captures: Dict<Int, String> = Dict.new()
  captures[0] = "vec_i64"
  capture_abi: Dict<String, Dict<Int, String>> = Dict.new()
  capture_abi["8"] = captures

  r := ReprAnalysis.{
    typed_fields,
    typed_payloads,
    param_abi: ParamAbi.{ typeable_params: Dict.new(), typeable_return },
    capture_abi,
  }
  p := phys_plan_from_analysis(r)

  try assert.is_true(phys_plan.is_typed_i64(phys_plan.field_repr(p, 10, 4)))
  try assert.is_true(phys_plan.is_typed_i64(phys_plan.payload_repr(p, 11, 2, 0)))
  try assert.is_true(phys_plan.is_typed_i64(phys_plan.return_repr(p, 7)))
  try assert.is_true(phys_plan.is_typed_i64(phys_plan.capture_repr(p, 8, 0)))
  try assert.is_false(phys_plan.is_typed_i64(phys_plan.return_repr(p, 9)))
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
_ := plan.slot_repr.keys().len()
```

- [ ] **Step 2: Thread plan from prepare.**

In `prepare.tw`, after `repr := analyze_typed_repr(...)`, add:

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

## Task 4: Make route consume `PhysPlan` projections for fields, payloads, returns, and captures

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw`
- Modify: `boot/tests/suites/route_typed_vec_suite.tw`
- Modify: `boot/tests/suites/typed_record_fields_suite.tw`
- Modify: `boot/tests/suites/typed_param_abi_suite.tw`

**Interfaces:**
- Consumes: `phys_plan.PhysPlan` from Task 3.
- Produces: route internals that query `plan` instead of separately passed physical maps where behavior is already represented in the projection.

> **Transitional scaffolding warning.** The `*_from_plan` adapters below reconstruct
> the exact maps analysis already produced (a `map → PhysPlan → map` round-trip),
> and `capture_abi_from_plan` re-parses the interpolated key with `split`/`parse_int`
> — a fragile string-format coupling with an `unwrap_or(-1)` failure path. This is a
> **stepping stone**, not the target shape. The preferred end state is route doing
> **point queries** (`phys_plan.field_repr(plan, tid, fid)`,
> `phys_plan.return_repr(plan, fid)`, `phys_plan.capture_repr(plan, fid, idx)`) at
> its decision sites, and dropping both the reconstructed maps *and* the redundant
> legacy parameters in **Task 9**. If a decision site can be migrated to a point
> query directly here without the map detour, prefer that and skip the corresponding
> adapter.

- [ ] **Step 1: Add local projection adapters inside route.**

In `route_typed_vec.tw`, add helpers near `route_func`:

```tw
fn typed_fields_from_plan(plan: phys_plan.PhysPlan) Dict<String, Bool> {
  out: Dict<String, Bool> = Dict.new()
  for key, repr in plan.field_repr {
    if phys_plan.is_typed_i64(repr) {
      out[key] = true
    }
  }
  out
}

fn typed_payloads_from_plan(plan: phys_plan.PhysPlan) Dict<String, Bool> {
  out: Dict<String, Bool> = Dict.new()
  for key, repr in plan.payload_repr {
    if phys_plan.is_typed_i64(repr) {
      out[key] = true
    }
  }
  out
}

fn typeable_return_from_plan(plan: phys_plan.PhysPlan) Dict<String, String> {
  out: Dict<String, String> = Dict.new()
  for key, repr in plan.return_repr {
    if phys_plan.is_typed_i64(repr) {
      out[key] = "vec_i64"
    }
  }
  out
}

fn capture_abi_from_plan(plan: phys_plan.PhysPlan) Dict<String, Dict<Int, String>> {
  out: Dict<String, Dict<Int, String>> = Dict.new()
  for key, repr in plan.capture_repr {
    if phys_plan.is_typed_i64(repr) {
      parts := key.split(":")
      if parts.len() == 2 {
        func_key := parts[0]
        idx := parts[1].parse_int().unwrap_or(-1)
        if idx >= 0 {
          cur := case out[func_key] {
            .Some(m) => m,
            .None => Dict.new(),
          }
          cur[idx] = "vec_i64"
          out[func_key] = cur
        }
      }
    }
  }
  out
}
```

If `String.split` / `parse_int` names differ, use the existing string helpers used elsewhere in the compiler; keep this adapter local and covered by tests.

- [ ] **Step 2: Route from plan-derived maps.**

At the top of `route_typed_vectors`, derive:

```tw
route_fields := typed_fields_from_plan(plan)
route_payloads := typed_payloads_from_plan(plan)
route_capture_abi := capture_abi_from_plan(plan)
route_typeable_return := typeable_return_from_plan(plan)
```

Use those values for route computation. Keep the old explicit parameters temporarily and assert they match the plan-derived projections in debug style:

```tw
if !bool_key_maps_equal(route_fields, typed_fields) {
  eprintln("[phys-plan] typed field projection differs from legacy typed_fields; using PhysPlan")
}
```

Do the same for payloads. For return/capture maps, add small equality helpers or skip warning in this task; behavior must come from `plan`.

- [ ] **Step 3: Update tests to prove route honors plan.**

In route/typed-record/typed-param tests, add one focused test where the legacy explicit maps are empty but `plan` contains a typed field/return. Example route test shape:

```tw
fn test_route_uses_phys_plan_return_projection() Result<Void, String> {
  // Compile a function `g` returning Vector<Int>` and a caller reading `g()`.
  // Build a PhysPlan with return_repr["<g func id>"] = .TypedI64.
  // Call route_typed_vectors(..., plan, Dict.new(), Dict.new(), Dict.new(), Dict.new()).
  // Assert caller call-result slot is PVecI64 and no legacy typeable_return map was needed.
  .Ok({})
}
```

Use existing helper patterns in `route_typed_vec_suite.tw` to find function ids and inspect slot wasm types. Do not rely on WAT substring matching in this unit test.

- [ ] **Step 4: Format, rebuild, test.**

Run:

```bash
target/twk fmt boot/compiler/backend/route_typed_vec.tw boot/tests/suites/route_typed_vec_suite.tw boot/tests/suites/typed_record_fields_suite.tw boot/tests/suites/typed_param_abi_suite.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point, no lint findings, boot tests pass.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/backend/route_typed_vec.tw boot/tests/suites/route_typed_vec_suite.tw boot/tests/suites/typed_record_fields_suite.tw boot/tests/suites/typed_param_abi_suite.tw
git commit -m "backend: route typed vectors from PhysPlan projections"
```

---

## Task 5: Materialize `slot_repr` from route eligibility before applying it

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw`
- Modify: `boot/compiler/backend/phys_plan.tw`
- Modify: `boot/tests/suites/route_typed_vec_suite.tw`

**Interfaces:**
- Produces:
  - `pub fn set_slot_repr(plan: PhysPlan, func_id: Int, slot_id: Int, repr: PhysRepr) PhysPlan`
  - route-local `materialize_slot_plan_for_func(plan: phys_plan.PhysPlan, func_id: Int, eligible_v: Dict<Int, Bool>) phys_plan.PhysPlan` that writes `slot_repr` entries before slot mutation.

- [ ] **Step 1: Add a setter helper.**

In `phys_plan.tw`:

```tw
pub fn set_slot_repr(plan: PhysPlan, func_id: Int, slot_id: Int, repr: PhysRepr) PhysPlan {
  if is_typed_i64(repr) {
    plan.slot_repr[slot_key(func_id, slot_id)] = repr
  }
  plan
}
```

Absent entries remain boxed; this helper only records typed sites.

- [ ] **Step 2: Split route into compute/apply inside `route_func`.**

In `route_typed_vec.tw`, keep `compute_eligible_v` as the source of truth, but make `route_func` first materialize the slot plan for the current function:

```tw
func_plan := plan
for sid in eligible_v.keys() {
  func_plan = phys_plan.set_slot_repr(func_plan, pf.func_id.id, sid, .TypedI64)
}
```

Then retype slots by looking up `func_plan.slot_repr`, not directly from `eligible_v`:

```tw
typed_here := phys_plan.is_typed_i64(phys_plan.slot_repr(func_plan, pf.func_id.id, info.slot.id))
```

Keep `eligible_b` for builder handles because builder slots are not durable typed-vector value slots.

- [ ] **Step 3: Return the materialized plan from route.**

Extend the existing `RoutedModule` record type (`route_typed_vec.tw:95`, currently `.{ funcs, swapped_sites }`) to include `plan: phys_plan.PhysPlan`:

```tw
pub type RoutedModule = .{ funcs: Vector<PreparedFunc>, swapped_sites: Vector<SwappedSetSite>, plan: phys_plan.PhysPlan }
```

There is no `RouteResult` type — `route_typed_vectors` returns `RoutedModule`, and `prepare.tw` already reads `routed.funcs` / `routed.swapped_sites`. Thread the updated plan through the fold over functions so the returned literal (line 148) carries the accumulated `plan`. `prepare.tw` should bind `routed.plan` even if later tasks do not consume it yet.

- [ ] **Step 4: Test that route result plan matches slot mutation.**

Add a route suite test that compiles a typed-vector producer, runs route, and asserts every `PVecI64` slot in the routed function has a corresponding `TypedI64` `slot_repr` entry in `routed.plan`.

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
- Produces: comments and checks that `mutvec_repr` only changes `MutVecI64` handle slots and never changes a durable typed-vector slot recorded by route.

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
route_typed := phys_plan.is_typed_i64(phys_plan.slot_repr(plan, pf.func_id.id, info.slot.id))
```

Then keep the current handle override behavior, but if a slot is both `route_typed` and a handle candidate, fail with a clear diagnostic:

```tw
if route_typed and handle.has(info.slot.id) {
  error("mutvec_repr: slot S${info.slot.id.to_string()} is both route-owned TypedI64 and MutVec handle in ${pf.name}")
}
```

A freeze-result slot should be `route_typed` and not a handle after the tactical fix. A live handle should be `handle` and not `route_typed`.

- [ ] **Step 3: Thread from prepare.**

In `prepare.tw`, pass `routed.plan` into `assign_mutvec_reprs`.

- [ ] **Step 4: Add or extend a regression test.**

Use an existing mutvec fixture shape and assert after prepare that:

- at least one slot has wasm type `rt_types__MutVecI64` for the live handle;
- the freeze-result / return slot has wasm type `rt_types__PVecI64` when route claims it;
- no slot marked as route-owned `TypedI64` is rewritten to `MutVecI64`.

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

- [ ] **Step 1: Replace `has_phys_return` boolean use in route application.**

Keep `compute_eligible_v` returning `has_phys_return` for source discovery in this task, but when setting the prepared function ABI, use:

```tw
if phys_plan.is_typed_i64(phys_plan.return_repr(func_plan, pf.func_id.id)) {
  pf.phys_return = .Some(.Ref(true, .Named(fi.fam.pvec_type)))
}
```

If the plan currently lacks a typed return entry but `has_phys_return` is true, add it before applying:

```tw
if has_phys_return {
  func_plan.return_repr["${pf.func_id.id}"] = .TypedI64
}
```

- [ ] **Step 2: Add consistency test.**

Add a route suite assertion that whenever a routed function has `phys_return = PVecI64`, `routed.plan.return_repr[func_id]` is `TypedI64`.

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

## Task 8: Add physical-edge verifier checks for non-coercing vector edges

**Files:**
- Modify: `boot/compiler/backend/verify_expr.tw`
- Modify: `boot/compiler/backend/verify_common.tw`
- Modify: `boot/tests/suites/typed_record_fields_suite.tw`
- Modify: `boot/tests/suites/backend_repr_suite.tw`

**Interfaces:**
- Consumes: prepared slot `wasm_type`, `phys_return`, field/payload layout, and eventually `PhysPlan`.
- Produces: verifier failures for uncoerced `PVecI64` ↔ boxed `PVec` mismatches on local/copy/result edges.

> **Highest-risk task — land report-only first.** This is the only task that can
> *reject currently-valid modules*: the verifier has no direct signal for whether
> emit will insert a coercion at a given edge, so a strict rejection can trip on an
> existing edge that relies on emit's coercion safety net and break `make boot-test`
> / self-host on landing. Therefore: (1) first land the check as **report-only**
> (accumulate a diagnostic / `eprintln`, do **not** `error`), gated behind an env
> flag (e.g. `TWINKLE_VERIFY_VEC_REPR=1`); (2) run the full suite + self-host and
> inventory every edge that fires — confirm each is genuinely a bug and not an
> emit-coerced edge; (3) only then flip the confirmed non-coercing edge classes to
> hard `error`. Do not enable strict rejection by default in the same commit that
> introduces the check.

The verifier check is built as the **doctrine's reusable edge-skeleton**: it operates
on physical `ValType` edges plus an `is_declared_coercion` capability, and never
mentions `PhysRepr`. This is the one substrate piece that stays reusable for a future
dict/record customer without any genericity — they supply their own coercion
predicate and reuse the same skeleton.

- [ ] **Step 1: Add the physical-repr edge-skeleton + vector coercion capability.**

In `verify_common.tw`, add a classifier (for readable diagnostics) and the
type-agnostic edge check driven by a capability:

```tw
pub type VecPhys = { NotVector, BoxedPVec, TypedPVecI64, MutVecI64 }

pub fn vec_phys_of_val_type(vt: ValType) VecPhys {
  case vt {
    .Ref(_, ht) => case ht {
      .Named(n) => cond {
        n == "rt_types__PVec" => .BoxedPVec,
        n == "rt_types__PVecI64" => .TypedPVecI64,
        n == "rt_types__MutVecI64" => .MutVecI64,
        _ => .NotVector,
      },
      _ => .NotVector,
    },
    _ => .NotVector,
  }
}

// The reusable edge-skeleton. `is_declared_coercion` is the per-customer capability:
// the vector customer declares PVec <-> PVecI64 coercible (emit boxes/unboxes there);
// MutVecI64 edges are produced by the freeze producer, not by a copy edge.
pub type ReprEdgeCap = .{ is_declared_coercion: fn(from: ValType, to: ValType) Bool }

pub fn vec_repr_edge_cap() ReprEdgeCap {
  .{
    is_declared_coercion: fn(from, to) {
      f := vec_phys_of_val_type(from)
      t := vec_phys_of_val_type(to)
      // PVec <-> PVecI64 is a declared coercion (emit inserts box_i64/unbox_i64).
      (is_boxed_or_typed(f) and is_boxed_or_typed(t))
    },
  }
}

fn is_boxed_or_typed(p: VecPhys) Bool {
  case p {
    .BoxedPVec => true,
    .TypedPVecI64 => true,
    _ => false,
  }
}

// Returns .Some(message) when the edge is an illegal non-coercing physical mismatch.
pub fn check_repr_edge(from: ValType, to: ValType, cap: ReprEdgeCap, ctx: String) String? {
  fp := vec_phys_of_val_type(from)
  tp := vec_phys_of_val_type(to)
  cond {
    // Not both vector physical types → not this check's concern.
    fp == .NotVector or tp == .NotVector => .None,
    // Equal physical class (incl. MutVecI64 == MutVecI64) is fine. Compare the
    // classified VecPhys (a plain enum), not raw ValType, so no ValType `==` is needed.
    fp == tp => .None,
    cap.is_declared_coercion(from, to) => .None,
    _ => .Some("physical vector repr mismatch at ${ctx}: producer ${from.to_string()} vs destination ${to.to_string()}"),
  }
}
```

- [ ] **Step 2: Check local copy/storage edges.**

In `verify_expr.tw`, extend existing slot/value verification so `AInit(atom)` and
`AAssign(target, atom)` call `check_repr_edge(atom_vt, dest_slot_wasm_type,
vec_repr_edge_cap(), "AInit"/"AAssign")`. A returned `.Some(msg)` is a candidate
defect: local copy/assign is non-coercing, so a `BoxedPVec` vs `TypedPVecI64`
mismatch with no declared coercion is a bug. **Report it (report-only mode) rather
than `error` on first landing** (see the task-level warning above), gated behind the
`TWINKLE_VERIFY_VEC_REPR=1` flag; only escalate to a hard failure once the
suite/self-host inventory confirms no valid edge relies on it.

- [ ] **Step 3: Check record-get result edge.**

For `ARecordGet(base, field, type_id)`, call `check_repr_edge(field_layout_vt,
result_slot_vt, vec_repr_edge_cap(), "ARecordGet")`. This catches the field-read copy
class before Wasm validation, through the same skeleton.

- [ ] **Step 4: Add negative tests using existing verifier test patterns.**

Create a prepared fixture in an existing backend suite that forces a `PVecI64` producer into a boxed destination slot without a coercion edge and assert verification rejects with a message containing `physical vector repr mismatch`.

- [ ] **Step 5: Format, rebuild, test.**

Run:

```bash
target/twk fmt boot/compiler/backend/verify_expr.tw boot/compiler/backend/verify_common.tw boot/tests/suites/typed_record_fields_suite.tw boot/tests/suites/backend_repr_suite.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point, no lint findings, boot tests pass.

- [ ] **Step 6: Commit.**

```bash
git add boot/compiler/backend/verify_expr.tw boot/compiler/backend/verify_common.tw boot/tests/suites/typed_record_fields_suite.tw boot/tests/suites/backend_repr_suite.tw
git commit -m "verify: reject uncoerced vector physical repr mismatches"
```

---

## Task 9: Retire redundant legacy map parameters (reach the single-plan end state)

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw`
- Modify: `boot/compiler/backend/mutvec_repr.tw`
- Modify: `boot/compiler/backend/prepare.tw`
- Modify: direct test callers under `boot/tests/suites/`

**Interfaces:**
- Produces: `route_typed_vectors(funcs, builtins, plan)` and `assign_mutvec_reprs(funcs, env, builtins, plan)` — the physical facts now flow *only* through `plan`.

Without this task the refactor stops at dual-carrying (plan **plus** the four legacy
maps), and the headline goal — one materialized plan that downstream stages read —
is not actually reached. This task removes the redundancy once route consumes the
plan.

- [ ] **Step 1: Confirm route reads only the plan.**

Verify (from Task 4's migration) that every route decision site queries `plan` —
either via a point query (`phys_plan.field_repr` / `return_repr` / `capture_repr` /
`payload_repr`) or via a plan-derived local — and that the `typed_fields`,
`typed_payloads`, `capture_abi`, `typeable_return` parameters are no longer read
anywhere in `route_typed_vec.tw`. Grep for each parameter name to prove non-use.
If any site still reads a legacy map, migrate it to a plan query before proceeding.

- [ ] **Step 2: Narrow the signatures.**

Remove the four now-dead map parameters from `route_typed_vectors`, leaving:

```tw
pub fn route_typed_vectors(
  funcs: Vector<PreparedFunc>,
  builtins: BuiltinRegistry,
  plan: phys_plan.PhysPlan,
) RoutedModule {
```

Delete the transitional `*_from_plan` reconstruction adapters that are no longer
needed (any decision site kept on the map shape must first move to a point query).
Remove the debug `eprintln` projection-equality warnings added in Task 4.

- [ ] **Step 3: Update `prepare.tw` and callers.**

`prepare.tw` passes only `phys := typed_param_abi.phys_plan_from_analysis(repr)` into
route; `repr`'s individual maps are no longer forwarded to routing (they remain
available to analysis). Update every direct test caller found via
`rg -n "route_typed_vectors\(" boot` to the three-argument form, building a
`PhysPlan` (via `phys_plan.empty()` or `phys_plan_from_analysis`) as needed.

- [ ] **Step 4: Format, rebuild, test.**

Run:

```bash
target/twk fmt boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/mutvec_repr.tw boot/compiler/backend/prepare.tw boot/tests/suites/*.tw
make bundle-cli
target/twk lint boot/main.tw
make boot-test
```

Expected: fixed point, no lint findings, boot tests pass. Byte-identical self-host
output vs the pre-narrowing commit confirms the removal was purely structural.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/mutvec_repr.tw boot/compiler/backend/prepare.tw boot/tests/suites
git commit -m "backend: route/mutvec repr consume only PhysPlan; drop legacy maps"
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

- `MonoType` is semantic. It never proves a storage site is physically typed.
- `route_typed_vec` **decides** durable typed-vector physical sites (its `compute_eligible_v` fixpoint) and applies them to prepared functions: slot `wasm_type`, typed builtin routing, `phys_return`.
- `PhysPlan` is the **materialized record** of those decisions — the single queryable, verifiable artifact every downstream stage reads. It records what route decided; it is not a second, independent decision engine.
- `typed_param_abi` computes use/support facts and PhysPlan projections; it does not mutate slots.
- `mutvec_repr` owns only temporary `MutVecI64` handles. MutVec freeze results are typed producers in route, not frozen-slot overrides in mutvec_repr.
- `emit` consumes prepared physical types and inserts coercions only at declared representation boundaries.
- `verify_expr` rejects non-coercing physical mismatches before Wasm validation.
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
- Consumes: prepared `SlotInfo.wasm_type` / `phys_return` / `PhysPlan`, and the
  strict verifier edge-skeleton from Task 8.
- Produces: an emit that reads prepared physical types and never re-derives typed-vector
  eligibility from `MonoType`.

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
rg -n "PVecI64|val_type_of_mono|Vector\(\.Int\)|is_typed|TypedVec" boot/compiler/codegen/emit.tw
```

For each hit, classify it: **(a) reads prepared type** (`SlotInfo.wasm_type`,
`phys_return`, atom val type) → leave as-is; **(b) applies a declared coercion**
(`box_i64`/`unbox_i64` at a physical mismatch) → leave as-is, the doctrine keeps it;
**(c) re-derives eligibility** (recomputes typed-vs-boxed from `MonoType`/mono keys
where a prepared physical type is already available) → this is the removal target.
Record the classification inline in a comment at each (c) site.

- [ ] **Step 2: Replace each re-derivation with a prepared-type read.**

For every class-(c) site, replace the `MonoType`-driven decision with a read of the
already-materialized physical type (the slot's `wasm_type`, the callee `phys_return`,
or `PhysPlan.*_repr`). If the audit found no class-(c) sites, skip to Step 4 and
record the confirmation.

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

- [ ] **Step 3: Run vector performance smoke checks.**

Run:

```bash
target/twk run examples/performance/sort-bench/typed_vec_read_probe.tw
target/twk run examples/performance/sort-bench/merge_attribution_probe.tw
target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw
```

Expected: checks complete successfully; timings remain in the same performance class as the pre-refactor route-based implementation. Record any material regression in `docs/plans/performance/vector/boundary-tracklist.md` before continuing.

- [ ] **Step 4: Final diff review.**

Run:

```bash
git diff --stat
git diff -- boot/compiler/backend/phys_plan.tw boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/typed_param_abi.tw boot/compiler/backend/mutvec_repr.tw boot/compiler/backend/verify_expr.tw boot/compiler/backend/verify_common.tw boot/compiler/backend/prepare.tw
```

Check that:

- route still owns durable `PVecI64` typing;
- mutvec_repr only owns `MutVecI64` handles;
- emit code was not changed to re-derive typedness;
- verifier errors mention physical producer/destination reprs.

- [ ] **Step 5: Commit final validation notes if docs changed.**

```bash
git add docs/plans/performance/vector
if ! git diff --cached --quiet; then
  git commit -m "docs: record physical repr planner validation"
fi
```

---

## Risks & rollback

- **Scope creep into typed parameter ABI.** This plan clarifies and materializes current return/capture/field/payload/local slot decisions. It does not add named-function typed parameter ABI. If a task starts specializing function params, stop and split a separate typed-param ABI plan.
- **Fixpoint performance.** Folding more facts into a materialized plan can increase prepare cost. This plan first projects and applies existing facts, then materializes route's result. Do not move all source categories into a multi-round `PhysPlan` fixpoint until boot compile timings are measured.
- **Verifier false positives.** Add checks only for edges whose emitter behavior is known. Non-coercing local/copy/result edges are first; direct-call and variant/record construction coercing edges need exact declared-coercion modeling before strict rejection.
- **MutVec regression.** The tactical fix depends on route recognizing `mutvec_freeze_i64` and mutvec_repr not clobbering freeze slots. Keep the WAT checks in Task 12 as a regression gate.
- **Emit safety-net removal (Task 11).** Removing typedness re-derivation is gated on the Task 8 verifier being strict and green; if the audit is uncertain whether a site is a declared coercion (keep) vs a re-derivation (remove), leave it and record it — a missed removal is harmless, a wrong one drops a coercion. Never run Task 11 while the verifier is still report-only.
- **Stopping at dual-carrying.** Tasks 2–7 deliberately run the plan *alongside* the legacy maps for behavior-preserving safety. That is a transitional state, not the goal. If Task 9 (legacy-map removal) is skipped or deferred, the "single materialized plan" objective is not met and the round-trip scaffolding from Task 4 becomes permanent debt. Treat Task 9 as load-bearing, not optional cleanup.
- **Verifier can reject valid code.** Task 8 is the only task that can break a currently-passing build. It must land report-only behind a flag and be inventoried before any hard rejection is enabled by default (see the Task 8 warning).
- **Rollback.** Each task is behavior-preserving or tightly gated. Revert the latest task commit if a verifier or projection refactor rejects valid current code. Reverting Task 1 removes only the unused plan model; reverting later tasks should restore the previous route map plumbing without changing language semantics.

## Out of scope

- Adding typed ABI for normal named-function parameters.
- Making `Vector<Int>` globally `PVecI64` by default.
- Adding typed helper ABIs for every vector combinator.
- Replacing emit coercions with an explicit prepared-IR `ACoerce` op.
- Reworking non-vector physical representations.
