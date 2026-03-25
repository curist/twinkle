# Boot Codegen — M11 Gap Closure Plan

Last updated: 2026-03-24

## Goal

Close the remaining M11 acceptance gaps after the initial end-to-end codegen
pipeline and focused integration harness landed.

This follow-up plan is specifically about the work still blocking M11 closure:

- fixing frontend/lowering/codegen failures exposed by end-to-end probing,
- expanding the current smoke-style equivalence harness into a real regression
  matrix,
- validating emitted boot WAT structurally and behaviorally across stage0 and
  boot paths.

This is a follow-up to [boot-codegen.md](boot-codegen.md), not a replacement
for it. The Phase D design and milestone definitions stay there; this document
tracks the concrete gap-closure work needed to finish M11 in practice.

---

## Current Baseline

As of 2026-03-24, the following are already in place:

- the boot pipeline exists:
  `plan_wasm_types -> insert_boundaries -> emit_module -> link -> emit_wat`
- the boot-side M11 suite is implemented and registered in `boot/tests/main.tw`
- the boot-side suite now includes compile-through-codegen repros for each of
  the main Phase 1-4 bug families
- a Rust integration harness compares stage0 vs boot behavior on the current
  green end-to-end set:
  smoke print, option boundary fixture, direct return, string return boundary,
  and top-level record field access
- emitted WAT in that Rust harness is validated by compiling it with Wasmtime

Verified commands:

- `cargo test --test boot_codegen_integration_test`
- `TWK_TEST_FILTER='codegen integration' ./target/debug/twk run -i boot/tests/main.tw`

Known limitation of the current baseline:

- the Rust harness is still a focused green matrix, not the full `tests/run/*`
  fixture set
- some named repros now compile through the boot helper but are not yet in the
  green Wasmtime-validated matrix:
  - `return` inside `if` still behaves as `no/no` instead of `yes/no`
  - `return` inside `for` still behaves as `none/none` instead of
    `found/none`
  - `tests/run/string_get.tw` and `tests/run/string_large_index_semantics.tw`
    still emit boot WAT that is not structurally valid under Wasmtime
- `tests/run/nested_field_update.tw` still fails earlier with
  `op_kind_from: unexpected type`
- some stdlib-backed control-flow fixtures still need helper-environment
  expansion before they are useful M11 regressions in the Rust harness
- `wasm-tools validate` is not yet wired; Wasmtime compilation currently serves
  as the structural validation step
- boot-runner Wasm-mode verification for the M11 suite was not completed in
  this pass

---

## Out of Scope

This plan does not include:

- Phase E multi-module/self-hosting loop work
- runtime-family specialization beyond targeted correctness fixes
- replacing Wasmtime validation with `wasm-tools validate` as a hard
  prerequisite for the first closure pass

---

## Workstreams

### Phase 1 — Sum / Variant Frontend and Lowering Correctness

Close the known failures where sum construction or destructuring does not make
it through the boot frontend cleanly.

Primary targets:

- `boot/compiler/checker.tw`
- `boot/compiler/lower_core.tw`
- `boot/compiler/resolver.tw` if qualified constructor lookup requires it

Concrete repros:

1. Option payload binding in `case` fails during lowering.
   - Shape:
     ```tw
     fn show(o: Option<Int>) {
       case o {
         .Some(n) => println("got ${n}"),
         .None => println("none"),
       }
     }
     ```
   - Observed failure:
     `lower_core: undefined variable: n`
   - Existing fixture:
     `tests/run/option_boundary_call.tw`

2. Local `Option` construction cannot determine the variant type.
   - Shape:
     ```tw
     o: Option<Int> = .Some(1)
     case o {
       .Some(_) => println("some"),
       .None => println("none"),
     }
     ```
   - Observed failure:
     `lower_core: cannot determine variant type`

3. Bare local sum constructors do not synthesize a usable type.
   - Shape:
     ```tw
     type Inner = { Hit, Miss }

     fn main() Void {
       o := .Hit
       case o {
         .Hit => println("hit"),
         .Miss => println("miss"),
       }
     }
     ```
   - Observed failure:
     `check: cannot synthesize type for this expression`

4. Qualified or nested sum constructors are not resolved correctly.
   - Shape:
     ```tw
     type Inner = { Hit, Miss }
     type Outer = { Wrap(Inner), Empty }

     println(describe(Outer.Wrap(Inner.Hit)))
     ```
   - Observed failure:
     `check: undefined variable: Outer`

Acceptance for Phase 1:

- all four shapes compile through the boot frontend
- payload bindings are available in lowered arms
- constructor resolution works for bare, contextual, and qualified forms
- at least one fixture-backed regression is added for each failure family

### Phase 2 — Control-Flow Semantics and Divergence

Fix the cases where boot-generated code validates but does not preserve stage0
behavior for early returns and branch joins.

Primary targets:

- `boot/compiler/lower_core.tw`
- `boot/compiler/codegen/emit.tw`

Concrete repros:

1. `return` inside `for` loops is lost semantically.
   - Shape:
     ```tw
     fn first_gt(xs: Vector<Int>, limit: Int) String {
       for x in xs {
         if x > limit { return "found" }
       }
       "none"
     }

     println(first_gt([1, 3, 7], 6))
     println(first_gt([1, 3], 6))
     ```
   - Stage0 output:
     `found`, `none`
   - Observed boot output:
     `none`, `none`

2. `return` inside `if` is also lost semantically.
   - Shape:
     ```tw
     fn choose(b: Bool) String {
       if b { return "yes" }
       "no"
     }

     println(choose(true))
     println(choose(false))
     ```
   - Stage0 output:
     `yes`, `no`
   - Observed boot output:
     `no`, `no`

3. Plain `if` expressions can still produce invalid join code.
   - Shape:
     ```tw
     fn sign(n: Int) String {
       if n > 0 { "pos" } else { "nonpos" }
     }
     ```
   - Observed failure:
     Wasm validation error `uninitialized local: 2`

Acceptance for Phase 2:

- branch joins are emitted with valid result/local discipline
- `return` semantics inside `if` and `for` match stage0
- interpreter output, stage0 Wasm output, and boot Wasm output agree on the
  regression cases

### Phase 3 — Match Emission and Stack Discipline

Fix the remaining `case`-driven backend issues where scalar/reference handling
still emits invalid WAT.

Primary targets:

- `boot/compiler/codegen/emit.tw`
- `boot/compiler/codegen/emit_pattern.tw`

Concrete repros:

1. Matching on `String.get(...)` results still miscompiles.
   - Shape:
     ```tw
     case String.get("ab", 4294967297) {
       .Some(_) => println("some"),
       .None => println("none"),
     }
     ```
   - Observed failure:
     Wasm validation error `expected anyref but nothing on stack`
   - Important nuance:
     the intrinsic call itself works when its result is ignored, so the bug is
     in consuming the `Option<Byte>` value via `case`

2. Matching on scalar `Bool` emits the wrong stack/value representation.
   - Shape:
     ```tw
     b := true
     case b {
       true => println("t"),
       false => println("f"),
     }
     ```
   - Observed failure:
     Wasm validation error `expected anyref, found i32`

Acceptance for Phase 3:

- `case` works over both scalar and reference-backed scrutinees
- match compilation preserves stack discipline and result typing
- large-index `String.get` remains covered as both a narrowing regression and a
  match-consumption regression

### Phase 4 — Record Representation Parity

Fix the remaining places where record values created or stored at the top level
take the wrong representation path during access or update.

Primary targets:

- `boot/compiler/codegen/wasm_layout.tw`
- `boot/compiler/codegen/emit.tw`
- `boot/compiler/lower_core.tw`

Concrete repros:

1. Top-level record field access does not use the expected layout path.
   - Shape:
     ```tw
     type Pair = .{ x: Int, y: Int }
     p := Pair.{ x: 41, y: 1 }
     println("${p.x}")
     ```
   - Observed failure:
     `emit_record_get: expected Record layout`

2. Nested record update through a top-level binding still breaks.
   - Shape:
     ```tw
     type Inner = .{ val: Int }
     type Outer = .{ inner: Inner, name: String }

     x: Outer = Outer.{ inner: Inner.{ val: 42 }, name: "hello" }
     x.inner.val = 99
     ```
   - Observed failure:
     `op_kind_from: unexpected type`
   - Existing fixture:
     `tests/run/nested_field_update.tw`

Acceptance for Phase 4:

- top-level record construction, get, and nested update all lower and emit
  through the same representation rules as local/parameter records
- the existing nested-field-update fixture passes through boot codegen

### Phase 5 — Harness Expansion and M11 Closure Matrix

Move from the current focused smoke harness to the actual M11 acceptance
matrix.

Primary targets:

- `boot/tests/helpers/codegen_harness.tw`
- `boot/tests/helpers/emit_boot_wat.tw`
- `boot/tests/suites/codegen_integration_suite.tw`
- `tests/boot_codegen_integration_test.rs`

Work items:

1. Expand the builtin/test environment in the boot harness so richer stdlib
   programs compile in the boot path.
2. Promote current inline repros into a fixture-backed regression matrix where
   practical.
3. Add explicit end-to-end regressions for the Phase D bug categories named in
   [boot-codegen.md](boot-codegen.md):
   - sum boundary
   - never / early return
   - i64 to i32 narrowing
   - struct field access/update
   - nested pattern matching with payload extraction
4. Finish verifying the boot test runner in both modes:
   - interpreter mode: `twk run -i ...`
   - Wasm mode: `twk run ...`
5. Decide whether to keep Wasmtime-only validation or add `wasm-tools validate`
   as a second validation layer.

Known harness-limited area to revisit after Phases 1-4:

- iterator / `Iterator.unfold` / richer stdlib-backed cases were intentionally
  omitted from the Rust M11 matrix because the helper builtin environment is
  still narrow. That omission is not yet proof of a codegen bug.

Acceptance for Phase 5:

- the Rust integration test covers real fixture-backed cases for each bug
  category that now compiles through boot
- the boot-side M11 suite runs from `boot/tests/main.tw`
- both the Rust harness and boot suite exercise behavior, not just text output
- every boot-emitted module in the matrix passes structural validation

---

## Recommended Execution Order

1. Fix sum/variant frontend and lowering issues first.
   - Several omitted M11 programs do not reach codegen at all yet.
2. Fix control-flow semantics next.
   - These are behavior mismatches even when structural validation passes.
3. Fix match/codegen stack discipline.
   - These are concentrated invalid-WAT failures with small repros.
4. Fix record representation parity.
   - This closes the remaining record-shaped regressions called out by the
     Phase D plan.
5. Expand the harness only after the above shapes are green.
   - Otherwise the matrix grows faster than failures can be diagnosed.

---

## Exit Criteria

M11 is closed when all of the following are true:

1. The boot integration suite is registered and executed from
   `boot/tests/main.tw`.
2. The concrete repro families in Phases 1-4 each have end-to-end regression
   coverage.
3. Stage0 and boot produce the same runtime behavior for the M11 matrix.
4. Every emitted module in that matrix passes structural validation
   (Wasmtime compilation at minimum; `wasm-tools validate` optional unless
   adopted as policy).
5. The remaining intentional omissions in the current harness are either fixed
   and covered, or explicitly moved to a separate follow-up plan.
