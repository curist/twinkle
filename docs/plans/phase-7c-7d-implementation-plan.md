# Codegen Phase 7C/7D Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the Phase 7C/7D decision seam: a typed mutable-codegen decision table, one selector/fallback helper, prepared-site extraction for emission, and backend plumbing that consults the helper while always emitting persistent code.

**Architecture:** Phase 7C defines the decision and selector contracts. Phase 7D threads those contracts through `PreparedModule` and `EmitCtx`, then uses a small emit-side helper to translate prepared slots/args into selector inputs. The selector can recognize valid, stale, unsupported, or ambiguous decisions and record the would-be mutable target, but every Phase 7C/7D code path returns the persistent target for emission. Phase 8 must deliberately change the selector contract and Wasm planning before any mutable target can be emitted.

**Tech Stack:** Twinkle boot compiler (`boot/compiler/**/*.tw`), optimized ANF, prepared backend IR, Wasm codegen, boot tests (`boot/tests/**/*.tw`).

## Why 7C and 7D share one plan

7C without 7D would leave unused records; 7D without 7C would invite ad-hoc backend checks. This plan lands one vertical seam: data model, centralized selector, prepared-site extraction, backend threading, persistent-only consumption, and tests proving no optimized mutable output appears.

## Global Constraints

- Treat the boot compiler in `boot/` as the primary implementation.
- Codegen consumes decisions; it does not re-prove uniqueness, last-use, field ownership, or escape.
- Absence, ambiguity, staleness, unsupported family, or target mismatch emits the persistent operation.
- Existing runtime/compiler hooks are implementation targets, not independent legality sources.
- Mutable-target lookup and persistent fallback stay centralized; backend families do not hand-roll ownership legality or stale-decision checks.
- No optimized mutable emission in this plan: no decision may emit `vector$set_in_place`, `dict$set_in_place`, `dict$remove_in_place`, or record `can_reuse=true`.
- Semantic builder lowering such as `collect` remains independent of ownership decisions.
- After each task that edits `.tw` files, run `target/twk fmt <changed .tw files>`, `target/twk lint boot/main.tw`, and the named tests for that task.

---

## File structure

- Create `boot/compiler/codegen/mutable_select.tw`
  - Owns the decision table, ambiguity representation, staleness checks, stable call-shape checks, and persistent-only target selection.
  - Uses `Dict<Int, Vector<MutableDecision>>` so duplicate decisions for one site become observable ambiguity, not silent overwrites.

- Create `boot/compiler/codegen/emit/mutable_sites.tw`
  - Owns prepared-IR site extraction for emission: current function/result local site, call base source local from `PreparedAtom.ASlot(SlotId)`, family classification from persistent builtin ids, and type-qualified record field-path key creation.
  - This helper is used by `.ACall` / `.ARecordUpdate` emission and directly unit-tested. Because Phase 7D always emits persistent output, tests can prove helper correctness and persistent-output guards, but not distinguish emit wiring by output alone; Task 4 therefore includes an explicit code-review gate for the `emit.tw` delegation points.

- Modify `boot/compiler/backend/prepare.tw`
  - Add `mutable_decisions` to `PreparedModule`.
  - Keep `prepare_backend(...)` as the existing compatibility wrapper with an empty table.
  - Add `prepare_backend_with_mutable_decisions(...)` for tests and future analysis producers.

- Modify `boot/compiler/codegen/emit/context.tw`
  - Add `mutable_decisions` to `EmitCtx`.

- Modify `boot/compiler/codegen/emit.tw`
  - Add `use compiler.codegen.mutable_select` and `use compiler.codegen.emit.mutable_sites`.
  - Initialize `base_ctx.mutable_decisions` and copy it into each per-function `EmitCtx` created by `emit_func`.
  - Pass result `SlotId` into op emission so `.ACall` and `.ARecordUpdate` can ask `mutable_sites` for selector inputs.
  - Consult `mutable_select` through `mutable_sites` while emitting persistent output.

- Leave `boot/compiler/codegen/emit/calls.tw` behavior unchanged.
  - ABI adaptation stays inside `emit_call`.
  - `emit.tw` passes an already-selected callee atom; in this plan that callee is always the persistent fallback.

- Create `boot/tests/suites/mutable_select_suite.tw`
  - Unit-tests ambiguity, staleness, unsupported family, fallback reasons, and record field/path checks.

- Modify `boot/tests/main.tw`
  - Register the new suite.

- Modify tests with manual `PreparedModule.{ ... }` constructors:
  - `boot/tests/suites/codegen_emit_suite.tw`
  - `boot/tests/suites/backend_verify_suite.tw`

---

## Boundary-insertion constraint

Phase 7C/7D does **not** validate non-base argument literal identity across the optimized-ANF → prepared-IR boundary. `prepare_backend` runs `insert_boundaries(...)`; for runtime helpers such as `vector$set_unsafe`, value arguments can be wrapped into temporary slots before emission. A pre-prepare decision that sees `LitInt(9)` may become a prepared call argument `Slot(wrap_temp)`. Until a producer runs in the prepared domain or records boundary-origin metadata, the selector validates only stable call-shape fields: site, family, persistent target, source/result locals, base argument index, and argument count.

## Emit-wiring review constraint

Phase 7D intentionally emits the same persistent WAT whether the selector is consulted or not. Tests in this plan therefore prove helper behavior, persistent-output guards, and Wasm-planning isolation, but they cannot prove identical-output `.ACall` / `.ARecordUpdate` branches actually delegated to `mutable_sites`. Task 4 includes a required review checkpoint that inspects the `emit.tw` diff and confirms both emission branches call the selector helpers before the task may be committed.

## Real-source survival scope

Task 2 proves the vector indexed-update site identity survives optimized ANF → prepared IR. Task 3 must also prove one real compiled `Dict.set` source site survives through `mutable_sites.select_call_for_emit(...)`, because dict lowering uses a real persistent builtin call family but a different source form from vector indexed assignment. Record update receives helper and hand-built integration coverage in this phase; a real-source record-update survival proof is deferred to Phase 8F unless the implementer can add it without expanding this seam.

## No-output-change baseline

Before starting Task 1 implementation, capture the default compiler output baseline:

```bash
target/twk build boot/main.tw -o /tmp/twinkle-phase-7d-before.wasm
```

After Task 4 emission plumbing, rebuild and compare:

```bash
target/twk build boot/main.tw -o /tmp/twinkle-phase-7d-after.wasm
cmp -s /tmp/twinkle-phase-7d-before.wasm /tmp/twinkle-phase-7d-after.wasm
```

Expected: `cmp` exits 0. If it differs, inspect before continuing; Phase 7C/7D must not change default emitted compiler output.

## Non-goals for this plan

- Do not implement the ownership-analysis producer that populates real decisions from `ownership.tw`. That is a follow-up after the seam exists.
- Do not clone ownership-specialized variants.
- Do not emit mutable vector/dict helpers or `can_reuse=true` from decisions.
- Do not add a user-facing dry-run renderer; the selector returns would-be target data for internal tests only.

---

### Task 1: Add the centralized mutable selector data model

**Files:**
- Create: `boot/compiler/codegen/mutable_select.tw`
- Create: `boot/tests/suites/mutable_select_suite.tw`
- Modify: `boot/tests/main.tw`

**Interfaces:**
- Produces: `empty_decision_table() MutableDecisionTable`
- Produces: `site_key(site: Site) Int`
- Produces: `with_decision(self: MutableDecisionTable, d: MutableDecision) MutableDecisionTable`
- Produces: `selection_reason_tag(reason: SelectionReason) Int`
- Produces: `record_reason_tag(reason: SelectionReason) Int`
- Produces: `select_call(table, site, expected_family, persistent_fallback, expected_base_arg_index, source_local, result_local, arg_count) CallSelection`
- Produces: `select_record_update(table, site, source_local, result_local, field_path_key, original_can_reuse) RecordReuseSelection`
- Consumes later: `PreparedModule.mutable_decisions`, `EmitCtx.mutable_decisions`, `mutable_sites` helpers

- [ ] **Step 1: Write the failing selector suite**

Create `boot/tests/suites/mutable_select_suite.tw`. The tests must include these cases and reason tags:

```tw
// Required reason tags:
// Absent=0, PersistentOnly=1, WrongFamily=2, WrongPersistentTarget=3,
// MissingMutableTarget=4, SourceLocalMismatch=5, ResultLocalMismatch=6,
// ArgumentShapeMismatch=7, BaseArgMismatch=8, FieldPathMismatch=9,
// Ambiguous=10, UnsupportedFamily=11.
```

Use this helper shape in the test file:

```tw
use @std.testing.assert as assert
use @std.testing as runner

use compiler.codegen.mutable_select
use compiler.core_ir.{FuncId, LocalId}

fn fid(id: Int) FuncId { FuncId.{ id } }
fn lid(id: Int) LocalId { LocalId.{ id } }

fn decision(
  site: mutable_select.Site,
  family: mutable_select.OperationFamily,
  persistent: FuncId?,
  mutable: FuncId?,
) mutable_select.MutableDecision {
  mutable_select.MutableDecision.{
    site,
    family,
    source_local: lid(1),
    result_local: site.local,
    arg_count: 3,
    base_arg_index: .Some(0),
    persistent_func: persistent,
    mutable_func: mutable,
    variant_key: .None,
    field_path_key: "1:f7",
    proof_debug_id: "selector-test",
  }
}
```

Add tests with these exact assertions:

```tw
// absent
selected := mutable_select.select_call(
  mutable_select.empty_decision_table(),
  mutable_select.Site.{ func: fid(10), local: lid(20) },
  mutable_select.OperationFamily.VectorSet,
  fid(100),
  .Some(0),
  lid(1),
  lid(20),
  3,
)
try assert.equal(mutable_select.selection_reason_tag(selected.reason), 0)
try assert.equal(selected.emit_func.id, 100)
try assert.equal(selected.would_func.id, 100)

// live persistent-only
site := mutable_select.Site.{ func: fid(10), local: lid(20) }
table := mutable_select.empty_decision_table().with_decision(
  decision(site, mutable_select.OperationFamily.VectorSet, .Some(fid(100)), .Some(fid(101))),
)
selected = mutable_select.select_call(
  table,
  site,
  mutable_select.OperationFamily.VectorSet,
  fid(100),
  .Some(0),
  lid(1),
  lid(20),
  3,
)
try assert.equal(mutable_select.selection_reason_tag(selected.reason), 1)
try assert.equal(selected.emit_func.id, 100)
try assert.equal(selected.would_func.id, 101)
```

Also add separate tests for:

```tw
// wrong family -> reason 2
// wrong persistent target -> reason 3
// missing mutable target -> reason 4
// source local mismatch -> reason 5
// result local mismatch -> reason 6
// arg_count mismatch -> reason 7
// base_arg_index mismatch -> reason 8
// duplicate decisions for the same site -> reason 10 and persistent fallback
// unsupported family, e.g. VectorBuilderRegion in this Phase 7D selector -> reason 11
// type-qualified record field path match -> reason 1, emit_can_reuse remains false, would_reuse true
// record selector with VectorSet/DictSet decision at same site -> reason 2
// record field id match but type id mismatch (e.g. "10:f0" vs "11:f0") -> reason 9,
//   emit_can_reuse remains false, would_reuse false
// duplicate record decisions for the same site -> reason 10
```

Do not test non-base literal identity in Phase 7C/7D. Boundary insertion can wrap value arguments into temporaries before emission, so optimized-ANF literal identity and prepared-call argument identity are not stable across the backend boundary. Phase 7C/7D validates only stable call shape: site, family, persistent target, source/result locals, base argument index, and argument count.

Register the suite in `boot/tests/main.tw`.

- [ ] **Step 2: Run the suite and verify the expected failure**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: compile failure naming `compiler.codegen.mutable_select` or `mutable_select_suite` as missing.

- [ ] **Step 3: Implement `boot/compiler/codegen/mutable_select.tw`**

Create the module with this shape. Keep all fallback checks here; emitters must not duplicate them.

```tw
//! Central mutable-codegen decision selector for Codegen Phase 7C/7D.
//!
//! This module owns the decision/fallback contract. It does not prove ownership
//! and never changes emitted code in Phase 7C/7D.

use compiler.core_ir.{FuncId, LocalId}
use compiler.variant_id as own_variant

pub type OperationFamily = {
  VectorSet,
  DictSet,
  DictRemove,
  RecordShellUpdate,
  VectorBuilderRegion,
  StringBuilderRegion,
  FunctionVariant,
  RecordBackedFieldCollection,
}

pub type Site = .{ func: FuncId, local: LocalId }

pub type MutableDecision = .{
  site: Site,
  family: OperationFamily,
  source_local: LocalId,
  result_local: LocalId,
  arg_count: Int,
  base_arg_index: Int?,
  persistent_func: FuncId?,
  mutable_func: FuncId?,
  variant_key: own_variant.VariantId?,
  field_path_key: String,
  proof_debug_id: String,
}

pub type MutableDecisionTable = .{ by_site: Dict<Int, Vector<MutableDecision>> }

pub type SelectionReason = {
  Absent,
  PersistentOnly,
  WrongFamily,
  WrongPersistentTarget,
  MissingMutableTarget,
  SourceLocalMismatch,
  ResultLocalMismatch,
  ArgumentShapeMismatch,
  BaseArgMismatch,
  FieldPathMismatch,
  Ambiguous,
  UnsupportedFamily,
}

pub type CallSelection = .{ emit_func: FuncId, would_func: FuncId, reason: SelectionReason }
pub type RecordReuseSelection = .{ emit_can_reuse: Bool, would_reuse: Bool, reason: SelectionReason }

pub fn selection_reason_tag(r: SelectionReason) Int {
  case r {
    .Absent => 0,
    .PersistentOnly => 1,
    .WrongFamily => 2,
    .WrongPersistentTarget => 3,
    .MissingMutableTarget => 4,
    .SourceLocalMismatch => 5,
    .ResultLocalMismatch => 6,
    .ArgumentShapeMismatch => 7,
    .BaseArgMismatch => 8,
    .FieldPathMismatch => 9,
    .Ambiguous => 10,
    .UnsupportedFamily => 11,
  }
}

pub fn record_reason_tag(r: SelectionReason) Int {
  selection_reason_tag(r)
}

pub fn site_key(site: Site) Int {
  own_variant.site_key(site.func.id, site.local.id)
}

pub fn empty_decision_table() MutableDecisionTable {
  MutableDecisionTable.{ by_site: Dict.new() }
}

pub fn with_decision(self: MutableDecisionTable, d: MutableDecision) MutableDecisionTable {
  key := d.site.site_key()
  existing := case self.by_site.get(key) {
    .Some(v) => v,
    .None => [],
  }
  self.by_site[key] = existing.append(d)
  self
}
```

Implement selector internals with these rules:

```tw
// 1. Missing site -> Absent.
// 2. More than one decision at the site -> Ambiguous.
// 3. Unsupported families for Phase 7D call selection -> UnsupportedFamily.
//    Supported call families in this plan: VectorSet, DictSet, DictRemove.
// 4. Family mismatch -> WrongFamily. `select_record_update` has implicit expected
//    family `RecordShellUpdate`; any other decision family returns WrongFamily.
// 5. Source/result local mismatch -> SourceLocalMismatch / ResultLocalMismatch.
// 6. arg_count mismatch -> ArgumentShapeMismatch.
// 7. base_arg_index mismatch -> BaseArgMismatch.
// 8. persistent_func mismatch or absent for calls -> WrongPersistentTarget.
// 9. mutable_func absent for calls -> MissingMutableTarget.
// 10. type-qualified field_path_key mismatch for records -> FieldPathMismatch.
// 11. Valid decision -> PersistentOnly, emit persistent fallback/original can_reuse,
//     preserve would_func/would_reuse for tests and later dry-run rendering.
```

Implement `selection_reason_tag(...)` exactly as specified by the test suite: Absent=0 through UnsupportedFamily=11. `select_call(...)` must always return `emit_func: persistent_fallback`, including valid decisions. `select_record_update(...)` must always return `emit_can_reuse: original_can_reuse`, including valid decisions.

- [ ] **Step 4: Format, lint, and run tests**

Run:

```bash
target/twk fmt boot/compiler/codegen/mutable_select.tw boot/tests/suites/mutable_select_suite.tw boot/tests/main.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: the new selector tests pass.

- [ ] **Step 5: Commit Task 1**

```bash
git add boot/compiler/codegen/mutable_select.tw boot/tests/suites/mutable_select_suite.tw boot/tests/main.tw
git commit -m "codegen: add mutable decision selector"
```

---

### Task 2: Thread the decision table through backend preparation

**Files:**
- Modify: `boot/compiler/backend/prepare.tw`
- Modify: `boot/tests/suites/codegen_emit_suite.tw`
- Modify: `boot/tests/suites/backend_verify_suite.tw`
- Test: `boot/tests/suites/backend_prepare_suite.tw`

**Interfaces:**
- Consumes: `mutable_select.MutableDecisionTable`
- Produces: `PreparedModule.mutable_decisions`
- Produces: `prepare_backend_with_mutable_decisions(...) PreparedModule`
- Preserves: existing `prepare_backend(...) PreparedModule` call sites

- [ ] **Step 1: Add failing preparation-boundary tests**

In `boot/tests/suites/backend_prepare_suite.tw`, merge these names into the existing `use` lists; do not paste duplicate imports if the file already imports the same module or symbol:

```tw
use compiler.anf.{AnfExpr, AnfModule, Atom}
use compiler.backend.prepare.{PreparedModule, prepare_backend, prepare_backend_with_mutable_decisions}
use compiler.backend.prepared_ir.{PreparedExpr, PreparedFunc, SlotId}
use compiler.builtins.{BuiltinRegistry}
use compiler.codegen.mutable_select
use compiler.core_ir.{FuncId, LocalId}
use compiler.pipeline
```

Add one storage test and one real-site identity test.

Storage test:

```tw
.test(
  "prepare_backend carries mutable decision table through the backend boundary",
  fn() {
    b := empty_builtins()
    anf := mk_module([
      mk_func_def(1, "f", [], MonoType.Int, AnfExpr.Atom(.ALitInt(1)), Dict.new()),
    ])
    site := mutable_select.Site.{ func: fid(1), local: lid(99) }
    decisions := mutable_select.empty_decision_table().with_decision(
      mutable_select.MutableDecision.{
        site,
        family: mutable_select.OperationFamily.VectorSet,
        source_local: lid(1),
        result_local: lid(99),
        arg_count: 3,
        base_arg_index: .Some(0),
        persistent_func: .Some(fid(100)),
        mutable_func: .Some(fid(101)),
        variant_key: .None,
        field_path_key: "",
        proof_debug_id: "prepared-boundary",
      },
    )
    prepared := prepare_backend_with_mutable_decisions(anf, empty_env(), b, no_captures(), decisions)
    selected := mutable_select.select_call(
      prepared.mutable_decisions,
      site,
      mutable_select.OperationFamily.VectorSet,
      fid(100),
      .Some(0),
      lid(1),
      lid(99),
      3,
    )
    try assert.equal(mutable_select.selection_reason_tag(selected.reason), 1)
    .Ok({})
  },
)
```

Real-site identity test: add small helpers in the suite to walk a prepared expression until it finds the `vector$set_unsafe` call and returns the prepared result slot plus base argument slot.

```tw
type VectorSetSlots = .{ result: SlotId, base: SlotId }
type VectorSetSite = .{ func: PreparedFunc, result: SlotId, base: SlotId }

fn find_vector_set_slots_expr(expr: PreparedExpr, set_id: FuncId) VectorSetSlots? {
  case expr {
    .Let(result, .ACall(.AGlobalFunc(fid), args), body) => {
      if fid.id == set_id.id and args.len() == 3 {
        case args[0] {
          .ASlot(base) => return .Some(VectorSetSlots.{ result, base }),
          _ => {},
        }
      }
      find_vector_set_slots_expr(body, set_id)
    },
    .Let(_, _, body) => find_vector_set_slots_expr(body, set_id),
    _ => .None,
  }
}

fn find_vector_set_site(prepared: PreparedModule, builtins: BuiltinRegistry) VectorSetSite {
  set_id := builtins.id("vector$set_unsafe")
  for pf in prepared.funcs {
    if pf.name == "set_one" {
      case find_vector_set_slots_expr(pf.body, set_id) {
        .Some(slots) => return VectorSetSite.{ func: pf, result: slots.result, base: slots.base },
        .None => {},
      }
    }
  }
  error("missing vector set site")
}
```

Then add helpers to capture the same decision shape from optimized/closure-converted ANF before backend preparation:

```tw
type AnfVectorSetSite = .{
  func: FuncId,
  result: LocalId,
  base: LocalId,
  arg_count: Int,
}

fn find_vector_set_anf_expr(func_id: FuncId, expr: AnfExpr, set_id: FuncId) AnfVectorSetSite? {
  case expr {
    .Let(result, .ACall(.AGlobalFunc(fid), args), body) => {
      if fid.id == set_id.id and args.len() == 3 {
        case args[0] {
          .ALocal(base) => return .Some(AnfVectorSetSite.{
            func: func_id,
            result,
            base,
            arg_count: args.len(),
          }),
          _ => {},
        }
      }
      find_vector_set_anf_expr(func_id, body, set_id)
    },
    .Let(_, _, body) => find_vector_set_anf_expr(func_id, body, set_id),
    _ => .None,
  }
}

fn find_vector_set_anf(anf: AnfModule, builtins: BuiltinRegistry) AnfVectorSetSite {
  set_id := builtins.id("vector$set_unsafe")
  for f in anf.functions {
    if f.name == "set_one" {
      case find_vector_set_anf_expr(f.func_id, f.body, set_id) {
        .Some(site) => return site,
        .None => {},
      }
    }
  }
  error("missing pre-prepare vector set site")
}
```

Then add the source-local continuity test. It builds the decision from the pre-prepare ANF site, passes that explicit table through `prepare_backend_with_mutable_decisions`, and verifies the same source/result locals are still available after slot assignment. This is not the final prepared-helper survival proof; Task 3 adds that proof once `mutable_sites.select_call_for_emit(...)` exists.

```tw
.test(
  "pre-prepare update decision matches prepared source locals",
  fn() {
    src := "fn set_one(xs: Vector<Int>) Vector<Int> {\n  xs[0] = 9\n  xs\n}\n"
    artifacts := try pipeline.compile_source(src)
    closure_conversion := convert_closures(artifacts.opt, artifacts.env)
    pre := find_vector_set_anf(closure_conversion.anf, artifacts.builtins)
    site := mutable_select.Site.{ func: pre.func, local: pre.result }
    decisions := mutable_select.empty_decision_table().with_decision(
      mutable_select.MutableDecision.{
        site,
        family: mutable_select.OperationFamily.VectorSet,
        source_local: pre.base,
        result_local: pre.result,
        arg_count: pre.arg_count,
        base_arg_index: .Some(0),
        persistent_func: .Some(artifacts.builtins.id("vector$set_unsafe")),
        mutable_func: .Some(artifacts.builtins.id("vector$set_in_place")),
        variant_key: .None,
        field_path_key: "",
        proof_debug_id: "prepared-real-site",
      },
    )
    prepared := prepare_backend_with_mutable_decisions(
      closure_conversion.anf,
      artifacts.env,
      artifacts.builtins,
      closure_conversion.captures,
      decisions,
    )
    site_slots := find_vector_set_site(prepared, artifacts.builtins)
    pf := site_slots.func
    base_info := case pf.slots[site_slots.base.id] {
      .Some(info) => info,
      .None => error("missing base slot after decision threading"),
    }
    result_info := case pf.slots[site_slots.result.id] {
      .Some(info) => info,
      .None => error("missing result slot after decision threading"),
    }
    selected := mutable_select.select_call(
      prepared.mutable_decisions,
      mutable_select.Site.{ func: pf.func_id, local: result_info.source_local },
      mutable_select.OperationFamily.VectorSet,
      artifacts.builtins.id("vector$set_unsafe"),
      .Some(0),
      base_info.source_local,
      result_info.source_local,
      pre.arg_count,
    )
    try assert.equal(mutable_select.selection_reason_tag(selected.reason), 1)
    .Ok({})
  },
)
```

- [ ] **Step 2: Run the tests and verify the expected failure**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: compile failure naming `mutable_decisions` or `prepare_backend_with_mutable_decisions`.

- [ ] **Step 3: Extend `PreparedModule` and preparation wrappers**

In `boot/compiler/backend/prepare.tw`, add:

```tw
use compiler.codegen.mutable_select
```

Extend `PreparedModule`:

```tw
mutable_decisions: mutable_select.MutableDecisionTable,
```

Keep existing callers stable:

```tw
pub fn prepare_backend(
  anf: AnfModule,
  env: ResolvedEnv,
  builtins: BuiltinRegistry,
  closure_captures: Dict<Int, Vector<CaptureParam>>,
) PreparedModule {
  prepare_backend_with_mutable_decisions(
    anf,
    env,
    builtins,
    closure_captures,
    mutable_select.empty_decision_table(),
  )
}
```

Move the old body to:

```tw
pub fn prepare_backend_with_mutable_decisions(
  anf: AnfModule,
  env: ResolvedEnv,
  builtins: BuiltinRegistry,
  closure_captures: Dict<Int, Vector<CaptureParam>>,
  mutable_decisions: mutable_select.MutableDecisionTable,
) PreparedModule {
  // Existing prepare_backend body, with mutable_decisions added to each return.
}
```

Add `mutable_decisions` to both the normal return and the `prepared_funcs_depth_exceeds` fallback return.

- [ ] **Step 4: Fix explicit prepared-module constructors**

Run:

```bash
rg -n "PreparedModule\.\{" boot/tests boot/compiler -g '*.tw'
```

Then broaden the search for typed anonymous `PreparedModule`-returning records that do not spell the type name:

```bash
rg -n "typed_vector_fields|typed_vector_payloads|PreparedModule" boot/tests boot/compiler -g '*.tw'
```

For every manual constructor outside `prepare.tw`, including typed anonymous returns, add:

```tw
mutable_decisions: mutable_select.empty_decision_table(),
```

Add `use compiler.codegen.mutable_select` to each modified test file.

- [ ] **Step 5: Format, lint, and run tests**

Run:

```bash
target/twk fmt boot/compiler/backend/prepare.tw boot/tests/suites/backend_prepare_suite.tw boot/tests/suites/codegen_emit_suite.tw boot/tests/suites/backend_verify_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: backend preparation and existing codegen tests pass with no behavior change.

- [ ] **Step 6: Commit Task 2**

```bash
git add boot/compiler/backend/prepare.tw boot/tests/suites/backend_prepare_suite.tw boot/tests/suites/codegen_emit_suite.tw boot/tests/suites/backend_verify_suite.tw
git commit -m "codegen: thread mutable decisions through prepare"
```

---

### Task 3: Add prepared-site extraction helpers for emission

**Files:**
- Create: `boot/compiler/codegen/emit/mutable_sites.tw`
- Modify: `boot/tests/suites/codegen_emit_suite.tw`
- Modify: `boot/tests/suites/wasm_plan_suite.tw`

**Interfaces:**
- Consumes: `EmitCtx`, `PreparedAtom`, `LocalEntry`, `SlotId`, `FuncId`, `FieldId`, `TypeId`
- Produces: `site_for_result(ctx, result_entry) Site?`
- Produces: `call_family_for_persistent(fid, ctx) OperationFamily?`
- Produces: `base_source_local_for_args(args, family, ctx) LocalId?`
- Produces: `record_base_source_local(base, ctx) LocalId?`
- Produces: `field_path_key(tid, fid) String`
- Produces: `select_call_for_emit(table, persistent_fid, args, result_entry, ctx) CallSelection?`

- [ ] **Step 1: Write failing helper tests**

In `boot/tests/suites/codegen_emit_suite.tw`, merge these names into the existing `use` lists; do not paste duplicate imports if the file already imports the same module or symbol:

```tw
use compiler.codegen.emit.mutable_sites
use compiler.codegen.mutable_select
use compiler.codegen.emit.context.{EmitCtx, LocalEntry}
use compiler.backend.prepared_ir.{PreparedAtom, SlotId}
use compiler.core_ir.{FieldId, FuncId, LocalId}
use compiler.mono_type.{TypeId}
```

Add tests that construct a minimal `EmitCtx` with a slot map containing a base slot whose `source_local` differs from its `SlotId`. Assert the helper returns the semantic `source_local`, not the slot id. For any helper test that expects `select_call_for_emit(...)` or `site_for_result(...)` to return `.Some(...)`, set `ctx.current_func_id` to `.Some(site.func)`; leaving it `.None` is the explicit skip path and should be tested only when the expected result is `.None`.

Required assertions:

```tw
base := mutable_sites.base_source_local_for_args(
  [PreparedAtom.ASlot(SlotId.{ id: 42 }), PreparedAtom.ALitInt(0), PreparedAtom.ALitInt(9)],
  mutable_select.OperationFamily.VectorSet,
  ctx,
)
case base {
  .Some(local) => try assert.equal(local.id, 7),
  .None => return .Err("missing base source local"),
}
try assert.equal(mutable_sites.field_path_key(TypeId.{ id: 10 }, FieldId.{ id: 3 }), "10:f3")
```

Also add helper tests for `Dict.set` and `Dict.remove`: `call_family_for_persistent` returns `.DictSet` / `.DictRemove`, `base_source_local_for_args` extracts argument zero's source local, and `select_call_for_emit(...)` returns reason `PersistentOnly` with the persistent dict helper as `emit_func` and the in-place helper as `would_func`.

Also add a helper-level selector test that calls `mutable_sites.select_call_for_emit(...)` with a non-empty decision table and asserts reason `PersistentOnly`, `would_func` is the mutable helper, and `emit_func` is the persistent fallback. This pins the logic emission is intended to delegate to.

Add the actual pre-prepare-to-prepared survival proofs here:

1. Compile vector update source:

```tw
fn set_one(xs: Vector<Int>) Vector<Int> {
  xs[0] = 9
  xs
}
```

Capture the decision from optimized/closure-converted ANF before `prepare_backend_with_mutable_decisions`, prepare with that table, find the prepared `vector$set_unsafe` call, build an `EmitCtx` whose `current_func_id` is `.Some(pf.func_id)`, and pass the prepared `args`, result entry, and `EmitCtx` to `mutable_sites.select_call_for_emit(...)`. Assert reason `PersistentOnly`, `would_func == vector$set_in_place`, and `emit_func == vector$set_unsafe`. This test must not call `mutable_select.select_call(...)` directly for the final assertion; it must derive the prepared base source local and prepared arg count through `mutable_sites`.

2. Compile dict update source:

```tw
fn set_one_dict(m: Dict<Int, Int>) Dict<Int, Int> {
  m[1] = 9
  m
}
```

Capture the pre-prepare ANF call whose callee is `artifacts.builtins.method_id("Dict", "set")`, build a `MutableDecision` with `family: mutable_select.OperationFamily.DictSet`, `persistent_func: .Some(artifacts.builtins.method_id("Dict", "set"))`, and `mutable_func: .Some(artifacts.builtins.id("dict$set_in_place"))`, then prepare with that table. Find the prepared `Dict.set` call, build an `EmitCtx` whose `current_func_id` is `.Some(pf.func_id)`, and assert `mutable_sites.select_call_for_emit(...)` returns `PersistentOnly`, `would_func == dict$set_in_place`, and `emit_func == Dict.set`. This proves at least one real dict source site preserves the same source/result locals and base argument through preparation.

- [ ] **Step 2: Run tests and verify the expected failure**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: compile failure naming `compiler.codegen.emit.mutable_sites`.

- [ ] **Step 3: Implement `boot/compiler/codegen/emit/mutable_sites.tw`**

Create the helper module. It may depend on `compiler.codegen.emit.context`, `compiler.codegen.mutable_select`, prepared IR types, and `compiler.mono_type.{TypeId}`. Implement:

```tw
pub fn site_for_result(ctx: EmitCtx, result_entry: LocalEntry) mutable_select.Site? {
  case ctx.current_func_id {
    .Some(fid) => .Some(mutable_select.Site.{ func: fid, local: result_entry.source_local }),
    .None => .None,
  }
}

pub fn call_family_for_persistent(fid: FuncId, ctx: EmitCtx) mutable_select.OperationFamily? {
  if fid.id == ctx.builtins.id("vector$set_unsafe").id {
    return .Some(.VectorSet)
  }
  if fid.id == ctx.builtins.method_id("Dict", "set").id {
    return .Some(.DictSet)
  }
  if fid.id == ctx.builtins.method_id("Dict", "remove").id {
    return .Some(.DictRemove)
  }
  .None
}

fn base_arg_index_for_family(family: mutable_select.OperationFamily) Int? {
  case family {
    .VectorSet => .Some(0),
    .DictSet => .Some(0),
    .DictRemove => .Some(0),
    _ => .None,
  }
}

pub fn base_source_local_for_args(
  args: Vector<PreparedAtom>,
  family: mutable_select.OperationFamily,
  ctx: EmitCtx,
) LocalId? {
  base_idx := case base_arg_index_for_family(family) {
    .Some(i) => i,
    .None => return .None,
  }
  if base_idx < 0 or base_idx >= args.len() {
    return .None
  }
  case args[base_idx] {
    .ASlot(sid) => .Some(lookup_slot(ctx, sid).source_local),
    _ => .None,
  }
}

pub fn record_base_source_local(base: PreparedAtom, ctx: EmitCtx) LocalId? {
  case base {
    .ASlot(sid) => .Some(lookup_slot(ctx, sid).source_local),
    _ => .None,
  }
}

pub fn field_path_key(tid: TypeId, fid: FieldId) String {
  "${tid.id}:f${fid.id}"
}
```

Implement `select_call_for_emit(table, persistent_fid, args, result_entry, ctx)` by combining `site_for_result`, `call_family_for_persistent`, `base_arg_index_for_family`, `base_source_local_for_args`, and `mutable_select.select_call(...)`. It must pass `expected_base_arg_index` and the stable prepared arg count into the selector. Return `.None` when any prepared-site requirement is unavailable so `emit.tw` can emit the persistent call directly.

- [ ] **Step 4: Add a Wasm planning guard test**

In `boot/tests/suites/wasm_plan_suite.tw`, first inspect the existing planning-test API and registry assertions:

```bash
rg -n "plan_wasm_types|direct_builtin_calls|runtime_imports" boot/tests/suites/wasm_plan_suite.tw boot/compiler/codegen -g '*.tw'
```

Then add a test that prepares a module whose body only calls the persistent vector or dict helper, attaches a decision naming the corresponding `*_in_place` helper, runs `plan_wasm_types(prepared, env, builtins)`, and asserts the registry does not register the mutable helper unless it appears in the prepared body. Use the nearby `direct_builtin_calls` / `runtime_imports` assertion style discovered by the `rg` command; the assertion should fail if planning starts looking at would-be decision targets during Phase 7D.

- [ ] **Step 5: Format, lint, and run tests**

Run:

```bash
target/twk fmt boot/compiler/codegen/emit/mutable_sites.tw boot/tests/suites/codegen_emit_suite.tw boot/tests/suites/wasm_plan_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: helper tests pass, the Wasm planning guard passes, and no emitted code changes yet.

- [ ] **Step 6: Commit Task 3**

```bash
git add boot/compiler/codegen/emit/mutable_sites.tw boot/tests/suites/codegen_emit_suite.tw boot/tests/suites/wasm_plan_suite.tw
git commit -m "codegen: add mutable site extraction helpers"
```

---

### Task 4: Expose the decision table to emission without changing output

**Files:**
- Modify: `boot/compiler/codegen/emit/context.tw`
- Modify: `boot/compiler/codegen/emit.tw`
- Test: `boot/tests/suites/codegen_emit_suite.tw`

**Interfaces:**
- Consumes: `PreparedModule.mutable_decisions`
- Consumes: `mutable_sites.select_call_for_emit(...)`
- Produces: `EmitCtx.mutable_decisions`
- Produces: emission that consults `mutable_select` and still emits persistent targets

- [ ] **Step 1: Add a failing persistent-output integration test**

In `boot/tests/suites/codegen_emit_suite.tw`, add a hand-built `PreparedModule` test with a known result slot `source_local`. Attach a decision for that site and emit the module. Assert persistent output remains and the mutable helper is absent. Add equivalent persistent-output checks for `Dict.set` and `Dict.remove`: WAT/call inspection should show the persistent dict helper and not the `_in_place` helper.

The test must assert both the selector-helper result and emitted WAT/calls:

```tw
selected := mutable_sites.select_call_for_emit(
  prepared.mutable_decisions,
  persistent_set_id,
  args,
  result_entry,
  ctx,
)
case selected {
  .Some(s) => {
    try assert.equal(mutable_select.selection_reason_tag(s.reason), 1)
    try assert.equal(s.would_func.id, in_place_set_id.id)
    try assert.equal(s.emit_func.id, persistent_set_id.id)
  },
  .None => return .Err("selector helper did not inspect call site"),
}
try assert.is_true(wat.contains("rt_arr__set"))
try assert.is_false(wat.contains("rt_arr__set_in_place"))
```

This fails if the helper logic is wrong or if a future change accidentally emits the mutable helper during 7D; output alone does not prove `emit.tw` called the helper.

Add a second integration test for `.ARecordUpdate`: construct or compile a record update with `can_reuse=false`, attach a matching `RecordShellUpdate` decision whose selector result has `would_reuse=true`, and emit WAT. Assert the selector reports `PersistentOnly`, the emitted path remains the copy/`struct.new` path, and no decision flips the operation to the `struct.set` reuse path. Also add a cross-record-type mismatch case: same `FieldId`, different `TypeId`, and assert `FieldPathMismatch`. Because Phase 7D output is intentionally persistent either way, this test proves selector/helper correctness plus the persistent-output guard; it does not by itself prove emit wiring.

- [ ] **Step 2: Run the test and verify the expected failure**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: compile failure for missing `EmitCtx.mutable_decisions` or missing emission plumbing.

- [ ] **Step 3: Add decisions to `EmitCtx`**

In `boot/compiler/codegen/emit/context.tw`:

```tw
use compiler.codegen.mutable_select

pub type EmitCtx = .{
  registry: WasmTypeRegistry,
  builtins: BuiltinRegistry,
  env: ResolvedEnv,
  mutable_decisions: mutable_select.MutableDecisionTable,
  // existing fields continue unchanged
}
```

In `boot/compiler/codegen/emit.tw`, add:

```tw
use compiler.codegen.mutable_select
use compiler.codegen.emit.mutable_sites
```

Initialize `base_ctx` with:

```tw
mutable_decisions: prepared.mutable_decisions,
```

In `emit_func`, when constructing the per-function `EmitCtx`, include:

```tw
mutable_decisions: base_ctx.mutable_decisions,
```

Find every `EmitCtx.{ ... }` literal before editing:

```bash
rg -n "EmitCtx\.\{" boot -g '*.tw'
```

Update every literal outside production emission as well, especially the minimal helper-test contexts added to `boot/tests/suites/codegen_emit_suite.tw` in Task 3. Use `mutable_decisions: mutable_select.empty_decision_table()` for neutral test contexts, or the relevant non-empty table for selector-consultation tests. For selector-consultation tests, also set `current_func_id: .Some(site.func)` so `site_for_result(...)` does not take the intentional `.None` skip path.

- [ ] **Step 4: Pass result slot identity into op emission**

Change `emit_op` to receive the result `SlotId`. Inside `emit_op`, keep existing behavior by deriving `result_idx` from `lookup_slot(ctx, result_slot)`.

```tw
fn emit_op(
  result_slot: SlotId,
  op: PreparedOp,
  result_vt: ValType?,
  result_mono: MonoType,
  ctx: EmitCtx,
  buf: Vector<Instr>,
) Vector<Instr> {
  entry := lookup_slot(ctx, result_slot)
  result_idx := entry.index
  // existing op dispatch continues here
}
```

Update every `emit_op(...)` call in `emit.tw` to pass the current `SlotId`. Search all call sites with:

```bash
rg -n "emit_op\(" boot/compiler/codegen/emit.tw
```

Expected call-site groups to update: ordinary let-spine emission, `Void`/`Never` let handling, tail/nested helper paths that currently pass only the Wasm local index.

- [ ] **Step 5: Consult the selector for calls**

In the `.ACall` branch of `emit_op`, replace the direct call with this concrete shape:

```tw
callee2 := case callee {
  .AGlobalFunc(fid) => case mutable_sites.select_call_for_emit(
    ctx.mutable_decisions,
    fid,
    args,
    entry,
    ctx,
  ) {
    .Some(selected) => PreparedAtom.AGlobalFunc(selected.emit_func),
    .None => callee,
  },
  _ => callee,
}
emit_call(callee2, args, result_idx, result_vt, result_mono, ctx, buf)
```

`select_call_for_emit` derives the site internally from `ctx.current_func_id` and `result_entry.source_local`. When `current_func_id` is absent, it returns `.None`.

- [ ] **Step 6: Consult the selector for record updates**

In the `.ARecordUpdate` branch, use `mutable_sites.site_for_result(...)`, `mutable_sites.record_base_source_local(base, ctx)`, and `mutable_sites.field_path_key(tid, fid)`. When `site` or base source local is unavailable, emit the persistent record update directly.

Selector call shape:

```tw
selected := case mutable_sites.site_for_result(ctx, entry) {
  .Some(site) => case mutable_sites.record_base_source_local(base, ctx) {
    .Some(base_source) => .Some(mutable_select.select_record_update(
      ctx.mutable_decisions,
      site,
      base_source,
      entry.source_local,
      mutable_sites.field_path_key(tid, fid),
      can_reuse,
    )),
    .None => .None,
  },
  .None => .None,
}
can_reuse2 := case selected {
  .Some(s) => s.emit_can_reuse,
  .None => can_reuse,
}
buf2 := emit_record_update(base, fid, value, can_reuse2, tid, result_mono, ctx, buf)
buf2.append(.LocalSet(result_idx))
```

Because the selector never flips `emit_can_reuse` in Phase 7D, emitted code stays persistent.

- [ ] **Step 7: Review the identical-output emit wiring**

Run:

```bash
git diff -- boot/compiler/codegen/emit.tw
rg -n "select_call_for_emit|select_record_update|ARecordUpdate|ACall" boot/compiler/codegen/emit.tw
```

Expected review result before continuing: the `.ACall` branch delegates global persistent callees through `mutable_sites.select_call_for_emit(...)`, and the `.ARecordUpdate` branch delegates base/result/field checks through `mutable_sites.site_for_result(...)`, `mutable_sites.record_base_source_local(...)`, `mutable_sites.field_path_key(...)`, and `mutable_select.select_record_update(...)`. The reviewer must explicitly confirm this wiring, because emitted WAT is intentionally identical to persistent fallback in Phase 7D and cannot prove the delegation happened.

- [ ] **Step 8: Format, lint, and run tests**

Run:

```bash
target/twk fmt boot/compiler/codegen/mutable_select.tw boot/compiler/codegen/emit/mutable_sites.tw boot/compiler/codegen/emit/context.tw boot/compiler/codegen/emit.tw boot/tests/suites/codegen_emit_suite.tw boot/tests/suites/mutable_select_suite.tw boot/tests/suites/wasm_plan_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
target/twk build boot/main.tw -o /tmp/twinkle-phase-7d-after.wasm
cmp -s /tmp/twinkle-phase-7d-before.wasm /tmp/twinkle-phase-7d-after.wasm
```

Expected: helper tests prove vector/dict/record decisions can be recognized through the emit helper, emitted WAT remains persistent, Wasm planning ignores would-be mutable targets, the before/after compiler Wasm comparison exits 0, and the required code review has confirmed the identical-output `emit.tw` selector wiring. Do not claim WAT alone proves `emit.tw` consulted the selector in Phase 7D.

- [ ] **Step 9: Commit Task 4**

```bash
git add boot/compiler/codegen/emit/context.tw boot/compiler/codegen/emit.tw boot/tests/suites/codegen_emit_suite.tw
git commit -m "codegen: consume mutable selector as persistent fallback"
```

---

### Task 5: Update docs to name the implemented 7C/7D seam

**Files:**
- Modify: `docs/plans/sound-uniqueness/codegen/README.md`
- Modify: `docs/plans/sound-uniqueness/codegen/handoff-contract.md`

**Interfaces:**
- Consumes: implemented selector/API names from Tasks 1–4
- Produces: docs that name the actual helper, persistent-only `emit_*` behavior, `would_*` dry-run data, and the deferred analysis producer

- [ ] **Step 1: Update `codegen/README.md` status and Phase 7C/7D checklists**

In `docs/plans/sound-uniqueness/codegen/README.md`, replace the opening status paragraph with:

```md
**Status:** In progress. Phases 7A (hook inventory), 7B (operation catalog),
and the 7C/7D backend decision seam are verified against `main` (2026-07-20).
The surviving mutable hooks and their persistent→mutable mappings are cataloged,
and codegen now has a persistent-only selector/plumbing seam. **Next: Phase 7E
or the analysis-producer follow-up** — dry-run rendering or extracting the
`ownership.tw` render-only decisions into `MutableDecisionTable`. The full
analysis track is complete through Phase 6 (record/field ownership,
transport-wrapper / `Result`-payload return-path summaries,
ownership-specialization decision facts, and recursive SCC variant-qualified
diagnostics), so codegen consumes a trustworthy fact set rather than
rediscovering ownership. (These "Codegen Phase 7A/…" labels are the codegen
track's own local numbering; see the phase-numbering note in
[../analysis/README.md](../analysis/README.md).)
```

Then replace the Phase 7C and Phase 7D checklist blocks with:

```md
## Codegen Phase 7C — Decision records and handoff contract ✅ seam done (2026-07-20)

No optimized emission yet. This phase makes the analysis→backend seam explicit
and fail-safe.

- [x] **Define first-cut backend decision records.** Implemented as
  `compiler.codegen.mutable_select.MutableDecision`: operation family, ANF site,
  source/result locals, stable argument shape, persistent fallback, would-be
  mutable target, optional `VariantId`, type-qualified field/path key, and
  `proof_debug_id`. The real analysis proof-payload producer is deliberately
  deferred; 7C/7D proves the backend seam with explicit tables first.
- [x] **Define one catalog-driven selection helper/layer.** Implemented as
  `compiler.codegen.mutable_select` plus prepared-site extraction in
  `compiler.codegen.emit.mutable_sites`. Backend lowering asks the selector for
  call or record selections and receives `emit_func` / `emit_can_reuse` values
  that remain persistent in Phase 7D, while `would_func` / `would_reuse` preserve
  the would-be mutable target for tests and later dry-run rendering.
- [x] **Attach decisions as ANF-keyed side tables.** `PreparedModule` carries
  `MutableDecisionTable` across backend preparation; `prepare_backend(...)`
  supplies an empty table by default and
  `prepare_backend_with_mutable_decisions(...)` exists for tests and future
  producers.
- [x] **Define staleness handling.** The selector reports absent, ambiguous,
  unsupported, wrong-family, wrong-persistent-target, source/result mismatch,
  argument-shape mismatch, base-argument mismatch, and field-path mismatch cases,
  and all such cases emit the persistent fallback.

## Codegen Phase 7D — Backend lookup and persistent fallback plumbing ✅ done (2026-07-20)

Still no optimized emission. This phase wires the backend to consume an empty or
ignored side table while deliberately returning the persistent target for every site.

- [x] **Thread the decision table to backend/codegen entry points.**
  `PreparedModule.mutable_decisions` is copied into `EmitCtx`; default empty-table
  behavior preserves persistent output.
- [x] **Validate lookup/fallback paths.** Selector and prepared-site tests cover
  present, absent, stale/mismatched, ambiguous, and unsupported decisions; emission
  tests guard that vector/dict calls and record updates still use persistent output.
- [x] **Keep fallback centralized.** Emission delegates prepared call/record site
  extraction to `compiler.codegen.emit.mutable_sites` and decision classification
  to `compiler.codegen.mutable_select`; family-specific emit code does not
  re-prove ownership or hand-roll stale-decision checks.
```

- [ ] **Step 2: Update `handoff-contract.md` with the concrete Phase 7D selector rule**

In `docs/plans/sound-uniqueness/codegen/handoff-contract.md`, replace the “Central selection layer” section with:

```md
## Central selection layer

Phase 7C/7D defines one selector/helper layer before any family starts emitting
mutable code. Backend lowering passes the prepared site, operation family,
decision table, stable operand shape, and persistent fallback to that layer.
The layer returns:

- `emit_func` / `emit_can_reuse`: the target that normal emission must use now;
  in Phase 7D these are always the persistent fallback / original record-reuse
  value, even for a live mutable decision;
- `would_func` / `would_reuse`: the mutable target that a later dry-run renderer
  or Phase 8 emission slice can report or enable deliberately; and
- a reason such as absent, persistent-only, wrong family, wrong persistent target,
  missing mutable target, source/result mismatch, argument-shape mismatch,
  base-argument mismatch, field-path mismatch, ambiguous, or unsupported.

No normal Phase 7D build can activate a mutable target. Phase 8 must extend Wasm
planning and deliberately change the selector consumption contract before
`would_*` data may become emitted code.

Per-family backend sites must not duplicate ownership legality, last-use,
staleness, unsupported-family, or fallback checks. They may perform only the local
mechanical emission for the target the selector returned.
```

- [ ] **Step 3: Update the handoff decision-record table for the explicit-table seam**

In `docs/plans/sound-uniqueness/codegen/handoff-contract.md`, update the first-cut decision-record text so it distinguishes the implemented backend seam from the deferred proof producer:

```md
## First-cut decision record

Phase 7C/7D implements the backend-facing record first, populated by explicit
test tables and future producers. Each accepted mutable-lowering candidate should
carry:

| Field | Purpose |
|---|---|
| ANF key | The optimized-ANF site the decision applies to. |
| Operation family | Vector indexed update, dict set/remove, record shell update, and later vector/string builder regions, record-backed field update, ownership-specialized function variant, etc. |
| Source value | The collection/record local whose storage or shell may be reused. |
| Result binding | The local that receives the post-update immutable value. |
| Stable call shape | Argument count and base-argument index fields that survive optimized-ANF → prepared-IR boundary insertion. |
| Persistent fallback | The ordinary immutable operation or generic callee to emit when the decision is absent or rejected. |
| Mutable target | The existing helper/hook or cloned function selected by the operation catalog, stored as would-be data until Phase 8 enables emission. |
| Variant key | The exact `VariantId` when the decision depends on an owned-specialized callee/body. Empty for purely local decisions. |
| Field/path key | The type-qualified consumed shell/field path for record-backed or record-shell decisions. Empty for whole-value call decisions. |
| Debug id | Stable id for `twk ir`, census, and WAT/call inspection. |

Follow-up after 7D: extract the existing render-only call-decision logic from
`ownership.tw` into a producer that populates `MutableDecisionTable` over
optimized ANF. That producer must attach the actual required ownership fact,
last-use proof, and any loop/branch/SCC proof id to the debug/proof trail, but it
is not part of 7C/7D backend plumbing because the seam must be usable and
testable with explicit tables first.
```

- [ ] **Step 4: Run verification commands**

Run:

```bash
target/twk fmt boot/compiler/codegen/mutable_select.tw boot/compiler/codegen/emit/mutable_sites.tw boot/compiler/backend/prepare.tw boot/compiler/codegen/emit/context.tw boot/compiler/codegen/emit.tw boot/tests/suites/mutable_select_suite.tw boot/tests/suites/backend_prepare_suite.tw boot/tests/suites/codegen_emit_suite.tw boot/tests/suites/backend_verify_suite.tw boot/tests/suites/wasm_plan_suite.tw boot/tests/main.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
target/twk build boot/main.tw -o /tmp/twinkle-phase-7d-after.wasm
cmp -s /tmp/twinkle-phase-7d-before.wasm /tmp/twinkle-phase-7d-after.wasm
make bundle-cli
make boot-test
git diff --check
```

Expected: formatter is idempotent after the first run, linter reports no blocking house-rule violations, boot tests pass, default compiler Wasm output matches the pre-Task-1 baseline, `make bundle-cli` and `make boot-test` pass sequentially, and `git diff --check` prints no whitespace errors.

- [ ] **Step 5: Commit Task 5**

```bash
git add docs/plans/sound-uniqueness/codegen/README.md docs/plans/sound-uniqueness/codegen/handoff-contract.md
git commit -m "docs: mark codegen decision seam implemented"
```

---

## Acceptance criteria

- `compiler.codegen.mutable_select` is the only place that classifies a decision as valid, stale, ambiguous, unsupported, or persistent-only.
- `MutableDecisionTable` preserves duplicate site decisions and reports them as `Ambiguous`.
- The selector records would-be mutable targets but returns persistent emission targets unconditionally in this phase.
- Existing default codegen passes an empty decision table and emits the same persistent operations as before.
- `compiler.codegen.emit.mutable_sites` is the only place emission translates prepared slots/args/fields into selector inputs.
- Task 4 includes a code-review checkpoint confirming `.ACall` and `.ARecordUpdate` in `emit.tw` delegate to the selector helpers, because identical persistent output cannot prove that wiring by WAT inspection alone.
- Tests exercise absent, live persistent-only, wrong-family, wrong-persistent-target, missing-mutable-target, source-local mismatch, result-local mismatch, argument-shape mismatch, base-arg mismatch, field-path mismatch, ambiguous, unsupported-family, and record-shell fallback cases.
- Prepared backend plumbing carries decision tables without forcing emitters or wasm planners to re-prove ownership.
- Emission helper tests prove selector consultation with a real `EmitCtx`/slot map whose `current_func_id` is populated for live-site assertions, including vector, dict set/remove, record base extraction, and type-qualified record field keys.
- Real-source survival tests prove prepared-site lookup for vector indexed update and dict set; record-update real-source survival is deferred to Phase 8F unless implemented opportunistically in this seam.
- Emission integration tests prove WAT/calls still use the persistent target for vector/dict calls and record updates. They are persistent-output guards, not proof that identical-output emit paths consulted the selector.
- Wasm planning tests prove would-be mutable targets in decisions do not register runtime imports or direct builtin calls during Phase 7D.
- No Phase 7C/7D code path emits `vector$set_in_place`, `dict$set_in_place`, `dict$remove_in_place`, or `can_reuse=true` because of a decision.

## Verification commands

Run before claiming the implementation is complete:

```bash
target/twk fmt boot/compiler/codegen/mutable_select.tw boot/compiler/codegen/emit/mutable_sites.tw boot/compiler/backend/prepare.tw boot/compiler/codegen/emit/context.tw boot/compiler/codegen/emit.tw boot/tests/suites/mutable_select_suite.tw boot/tests/suites/backend_prepare_suite.tw boot/tests/suites/codegen_emit_suite.tw boot/tests/suites/backend_verify_suite.tw boot/tests/suites/wasm_plan_suite.tw boot/tests/main.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
target/twk build boot/main.tw -o /tmp/twinkle-phase-7d-after.wasm
cmp -s /tmp/twinkle-phase-7d-before.wasm /tmp/twinkle-phase-7d-after.wasm
make bundle-cli
make boot-test
git diff --check
```

`boot/tests/main.tw` imports the compiler and exercises the modified preparation/emission paths from source-level integration tests, so it catches Twinkle type errors in the new modules and default codegen regressions. The final `make bundle-cli` + `make boot-test` sanity check verifies the self-hosted payload and boot suite after the seam lands.

## Follow-up plan after 7D

Create a separate analysis-producer plan that extracts the current render-only call-decision logic from `ownership.tw:block_verdicts` into data that populates `MutableDecisionTable`. That follow-up should reuse `select_variant`, skip generic/empty variant keys, preserve ambiguity rather than overwriting duplicate sites, and translate only through the 7C/7D selector seam. If it needs non-base argument identity, it must either run after boundary insertion / preparation or carry explicit boundary-origin metadata; it must not compare optimized-ANF literal operands directly against prepared call operands.

## Self-review notes

- Spec coverage: Phase 7C decision records, centralized selector, ambiguity/stale fallback checks, and Phase 7D backend lookup/fallback plumbing are covered.
- Scope: Phase 8 emission and the real ownership-analysis producer are excluded deliberately.
- Risk handled: prepared emission reconstructs site keys from `current_func_id` plus the result slot's `source_local`; it skips selection when the function id or base slot source is unavailable.
- Planner/emitter mismatch handled for this phase: persistent-only selection means no new mutable runtime import is required. Phase 8 must extend Wasm planning before enabling mutable emission.
