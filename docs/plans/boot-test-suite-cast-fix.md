# Boot test suite illegal-cast investigation and fix plan

## Goal

Eliminate the `illegal cast` failures that appear when boot test entrypoints build
large cross-module `Vector<runner.Suite>` values, so the test entrypoints can go
back to straightforward single-shot aggregation and `tools/boot-test-fast.sh`
can use one normal `runner.run_all(...)` flow.

This plan is about fixing the compiler/runtime bug, not preserving the current
batching workaround.

---

## Problem summary

Several boot test entrypoints fail at runtime with `illegal cast` even though the
same suites pass when run individually or in smaller groups.

Observed examples from this repo state:

- `tools/boot-test-fast.sh` originally failed after compiling `boot/tests/main.tw`
  with stage0 and running the resulting Wasm directly.
- `boot/tests/test_frontend.tw` also reproduced the same failure.
- `boot/tests/main.tw` and other entrypoints already contained comments warning
  about a current boot-compiler cast hazard around large cross-module `Suite`
  vector literals or concat chains.
- `semantic_tree_stringify_suite` passes in isolation, but certain larger
  aggregations that include it can trip the cast.
- the failure is aggregation-sensitive: individual suites pass, some grouped
  batches pass, larger mixed groups fail.

This strongly suggests the bug is in how cross-module `runner.Suite` values are
materialized, passed, stored in vectors, or cast in generated Wasm — not in the
semantic correctness of the tests themselves.

---

## Current workaround

The repo currently uses pragmatic workarounds so the boot test flows stay
usable:

- `tools/boot-test-fast.sh` now builds through `tools/twk_boot.mjs` instead of
  stage0.
- several test entrypoints were rewritten to run suites in batches rather than
  constructing one large suite vector.

Those changes are useful operationally, but they are not the desired end-state.
The desired end-state is:

- one logical test run per entrypoint
- no special batching required for `runner.Suite`
- no compiler-dependent cast hazard between stage0 and boot

---

## Why this matters

This is a real correctness bug because:

- source programs that should be equivalent are not operationally equivalent
  (`[a, b, c]`, repeated `.append(...)`, and batched `run_all(...)` shapes do not
  behave consistently)
- stage0 and boot disagree on safe aggregation shapes
- test harness structure is leaking backend lowering constraints into ordinary
  source organization
- the current workaround hides the bug instead of removing it

If left unfixed, the same class of issue can affect user code that builds vectors
of cross-module records/closures, not just test harness code.

---

## Working hypothesis

The failure is most likely in one of these areas:

1. **Cross-module nominal type identity for `runner.Suite` / `runner.Test`**
   - different modules may end up materializing values whose Wasm heap types are
     not treated as the same runtime type even though source-level nominal
     identity should match.

2. **Closure/value wrapping inside suite records**
   - `runner.Test` contains a function field.
   - cross-module suite construction means many closures and function references
     are packed into records and vectors.
   - the cast may happen when those records are inserted into or read back from a
     vector, or when a function field is loaded and called.

3. **Typed container/helper mismatch in backend/codegen**
   - vector construction, append, concat, or boundary insertion may emit helper
     paths that assume a more specific heap type than the actual produced value.
   - this would explain why shape-sensitive aggregation patterns matter.

4. **Stage0/boot lowering drift**
   - stage0 and boot may no longer agree on one of the above invariants, so the
     same source shape behaves differently depending on which compiler produced
     the Wasm.

At the moment, the evidence points more toward record/vector/closure layout or
cast insertion than toward parser/checker issues.

---

## Reproduction matrix to preserve

Any real fix should keep these repro categories around:

### Repros that should pass after the fix

- a single suite entrypoint:
  - `runner.run_all([semantic_tree_stringify_suite.suite()])`
- small grouped entrypoints
- large grouped entrypoints with many imported suites
- direct vector literal aggregation of suites
- repeated `.append(...)` aggregation of suites
- `concat`-based aggregation of prebuilt suite vectors

### Cross-compiler consistency checks

For the same test entrypoint source:

- stage0-built Wasm should run successfully
- boot-built Wasm should run successfully
- both should produce the same test totals and outcomes

---

## Potential fix directions

### Option A: fix typed vector / record cast insertion

Inspect backend paths that build and consume vectors of GC-managed records with
function fields.

Likely files:

- `boot/compiler/backend/prepare.tw`
- `boot/compiler/codegen/emit.tw`
- `boot/compiler/codegen/insert_boundaries.tw`
- `boot/compiler/codegen/wasm_plan_impl.tw`
- Rust mirrors in `src/`

Questions:

- when a `runner.Suite` value crosses a construction boundary, what exact Wasm
  heap type is produced?
- when a suite is inserted into a vector, does the vector element path expect a
  different heap type and emit a narrowing cast?
- do vector literal, append, and concat share the same element-cast rules?

**Why this is a good candidate:** the failure depends heavily on aggregation
shape, which points at container construction/consumption.

### Option B: fix nominal type identity across module linking

Inspect whether imported/shared nominal types used inside `runner.Suite` and
`runner.Test` stay globally unique through module compilation, linking, backend
layout, and emitted Wasm type registration.

Likely files:

- `boot/compiler/resolver.tw`
- `boot/compiler/module_compiler.tw`
- `boot/compiler/core_linker.tw`
- `boot/compiler/codegen/wasm_layout.tw`
- `boot/compiler/codegen/wasm_plan_impl.tw`
- Rust mirrors in `src/module/`, `src/types/`, `src/ir/`

Questions:

- does every module importing `tests.runner` agree on the exact same nominal
  type identity for `Suite` and `Test`?
- are typed container families keyed by a stable nominal identity, or are they
  accidentally duplicated per import path/module environment snapshot?

**Why this is a good candidate:** cross-module values are the common factor.

### Option C: fix closure packaging inside test records

Inspect whether the `run: fn() Result<Void, String>` field inside `runner.Test`
gets wrapped consistently when a `Suite` is created in one module and consumed in
another.

Questions:

- does the test function field have one stable closure representation?
- are wrapper trampolines/casts inserted consistently for named functions,
  lambdas, and imported functions inside record fields?

**Why this is a good candidate:** `Suite` values are not just plain data; they
contain function fields, which are a common cast hazard in Wasm GC pipelines.

---

## Preferred fix strategy

Pursue the issue in this order:

1. **Minimize to the smallest cross-module repro**
   - keep only `tests.runner`, one or two suite modules, and one entrypoint
   - find the smallest aggregation shape that fails

2. **Instrument the emitted Wasm/backend IR for the failing repro**
   - inspect record type names, vector element helper types, closure field types,
     and any emitted `ref.cast`/`ref.test`-like behavior or equivalent typed
     assumptions

3. **Check stage0 vs boot output for the same repro**
   - identify where the produced type/layout/cast path diverges

4. **Fix the invariant at the compiler level**
   - prefer a fix in type/layout/boundary handling over further source-level
     workarounds

5. **Remove batching workarounds**
   - restore `boot/tests/main.tw`, `boot/tests/test_frontend.tw`, and
     `boot/tests/test_codegen.tw` to normal single-run aggregation once safe

---

## Implementation phases

### Phase 1: isolate a minimal repro

- [ ] Add a dedicated tiny repro entrypoint under `boot/tests/fixtures/` or a
      focused suite helper that builds a small `Vector<runner.Suite>` from two
      imported suite modules
- [ ] Record which source shapes fail:
      literal / append / concat / local alias / batched run
- [ ] Confirm whether the failure still requires `semantic_tree_stringify_suite`
      specifically or whether any additional cross-module suite is enough

### Phase 2: capture backend evidence

- [ ] Dump Core / mono / ANF / prepared backend IR for the minimal repro
- [ ] Dump WAT for both stage0-built and boot-built outputs
- [ ] Compare the Wasm type names and helper calls for:
      - `runner.Suite`
      - `runner.Test`
      - vectors containing them
      - closure fields inside `Test`

### Phase 3: identify the broken invariant

- [ ] Determine whether the bug is:
      - nominal type duplication,
      - container helper mismatch,
      - closure packaging mismatch,
      - or another backend cast insertion bug
- [ ] Confirm whether the same root cause explains both
      `boot/tests/main.tw` and `boot/tests/test_frontend.tw`

### Phase 4: implement the real fix

- [ ] Fix the compiler/backend invariant in boot
- [ ] Mirror the same fix into Rust stage0 if the drift is there too
- [ ] Add focused regression tests for the exact failing aggregation shape

### Phase 5: remove workarounds

- [ ] Collapse `boot/tests/main.tw` back to one `runner.run_all(...)`
- [ ] Collapse `boot/tests/test_frontend.tw` batching
- [ ] Collapse `boot/tests/test_codegen.tw` batching
- [ ] Keep only regression coverage, not workaround comments

---

## Regression tests to add

At minimum, add targeted tests covering:

- [ ] vector literal of imported `runner.Suite` values
- [ ] repeated `.append(...)` of imported `runner.Suite` values
- [ ] `concat` of two `Vector<runner.Suite>` values built in different modules
- [ ] direct `runner.run_all(...)` over a large cross-module suite list
- [ ] stage0-built and boot-built execution of the same repro entrypoint

A useful extra regression would be a non-test-harness version using user-defined
records with function fields stored in vectors across multiple modules. That
would confirm the fix is general and not overfit to `tests.runner`.

---

## Success criteria

This plan is complete when all of the following are true:

- `boot/tests/main.tw` runs as one logical aggregated run without batching
- `boot/tests/test_frontend.tw` runs without batching
- `boot/tests/test_codegen.tw` runs without batching
- `tools/boot-test-fast.sh` succeeds without relying on source-level suite
  splitting to avoid casts
- stage0 and boot agree on the same repro entrypoints
- new regression tests pin the bug so it cannot silently return

---

## Notes for implementation

The current batching changes are acceptable as temporary operational support, but
should be treated as scaffolding around a compiler bug.

Do not normalize the workaround into permanent test-runner structure unless the
team explicitly decides that large cross-module suite aggregation is outside the
language/runtime contract. Right now the evidence suggests it should work and the
compiler is the thing that needs to improve.
