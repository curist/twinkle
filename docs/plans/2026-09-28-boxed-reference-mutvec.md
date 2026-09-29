# Boxed Reference MutVec Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let ordinary producer-rooted immutable code using `Vector<MyRecord>` retain its uniquely owned vector in compiler-private boxed mutable storage through recursive aggregate/helper regions, while preserving persistent fallback and immutable element semantics.

**Architecture:** Add a single `array<anyref>`-backed `MutVecBoxed` family for concrete Wasm GC-reference element types, while keeping the exact `Vector<T>` mono on every region, flat edge, sibling, and ABI upgrade. Before activating that family, refine ownership with a separate rooted projected-borrow fact so reading a record reference from a vector does not alias the vector shell; reuse the existing S4 producer, variant, propagation, cap, and freeze-boundary machinery.

**Tech Stack:** Self-hosted Twinkle boot compiler, ANF/CFG ownership summaries, Wasm-GC arrays and structs, existing MutVec S4 passes, boot test harness, standalone `target/twk`, AWFY benchmark harness.

**Spec:** `docs/plans/2026-09-28-boxed-reference-mutvec-design.md`

## Global Constraints

- Keep the source-level `Vector<T>` API and immutable value semantics unchanged; add no public mutable API, annotation, capability, or required source rewrite.
- Keep `Int`, `Float`, `Bool`, and `Byte` on their existing unboxed MutVec families.
- Box only concrete element types whose lowered Wasm heap type is a GC reference below `any`; reject `ExternRef`, `Optional<ExternRef>`, `Void`, `Never`, erased `Anyref_`, unresolved `Var`/`MetaVar`, and `ErrorType`.
- Remain producer-rooted. Do not add a PVec-to-MutVec thaw and never mutate persistent trie backing.
- Treat indexed GC-reference elements as shared projected borrows. An index read cannot seed ownership of a nested vector or other element object.
- Carry and compare exact `Vector<T>` mono identity through aggregate regions, flat propagation, sibling memoization, ABI upgrades, caller rewrites, and backend verification. `ElemRepr.Boxed` alone never licenses retargeting.
- Reuse the existing ownership/variant authority, `variant_cap` partition, flat-handle verifier, and all-or-nothing rewrite. Any missing proof selects the persistent path.
- A dead vector result emits no freeze; an observed vector or carrier emits exactly one freeze at its publication boundary.
- Keep new analysis linear in module/body size. Avoid repeated full-module scans and `collect_ops` in compile-hot paths.
- Implement in the boot compiler. Change Rust stage0 only if bootstrapping the runtime declarations requires it.
- Format and lint every modified `.tw` file. Never run tree-sitter tests.
- Follow red-green-refactor for every behavior-changing task and commit each independently reviewable result.

## Review Focus

- `Vector<ExternRef>`, `Vector<ExternRef?>`, `Vector<Void>`, and erased/unresolved element types must remain persistent even though some lower through reference-shaped sentinel types; Task 2 adds classifier negatives.
- Returning or capturing `xs[i]` must publish the projected element without publishing or aliasing the `xs` shell; Task 1 adds direct and interprocedural ownership tests.
- Reading `outer[i]` from `Vector<Vector<Person>>` must not seed an owned route for the inner vector while it remains stored in `outer`; Task 1 adds the nested-vector negative.
- Two boxed helpers with the same physical family but different `Vector<T>` monos or handle slots must never share a sibling or cross-retarget; Tasks 2 and 4 add identity and fallback tests.
- A boxed route rejected after planning—cap exhaustion, capture, mixed lineage, mono loss, or unsupported op—must leave byte-valid persistent code with no partial boxed ABI; Task 5 adds end-to-end rejection fixtures.

---

### Task 1: Separate Projected Element Borrows from Shell Provenance

**Status:** Complete in `7cdfb0b6`.

**Files:**
- Create: `boot/compiler/projected_borrow.tw`
- Create: `boot/tests/suites/projected_borrow_suite.tw`
- Modify: `boot/tests/main.tw`
- Modify: `boot/compiler/ownership.tw`
- Modify: `boot/compiler/summary.tw`
- Test: `boot/tests/suites/cfg_ownership_suite.tw`
- Test: `boot/tests/suites/cfg_summary_suite.tw`

**Interfaces:**
- Produces: `ProjectedBorrow.{ roots: Vector<Int>, path: field_facts.AccessPath }`, pure equality/meet/prefix helpers, a per-local `projected_shell: ProjectedBorrow?` map in `ForwardState`, and `Summary.ret_borrow: ProjectedReturn?` where `ProjectedReturn` identifies one parameter index plus an `AccessPath`. When that value is embedded, its roots move into the container's existing path-keyed `path_prov`; projecting it back out reconstructs `projected_shell`, so one local can still contain several independently rooted fields without a second nested-provenance lattice.
- Produces: `AIndex` transfer semantics in which a GC-reference result is `.Shared`, has empty shell `prov`, and carries a rooted `[Elem]` projected borrow; primitive reads remain unchanged.
- Consumes: existing `field_facts.AccessPath`, `path_prov`, parameter-origin ids, CFG liveness/fixpoint joins, and summary SCC fallback.

- [x] **Step 1: Write pure projected-borrow lattice tests**

  Add `projected_borrow_suite` cases asserting that identical roots/path survive `meet`, conflicting paths return `.None`, roots are canonicalized, prefixing `[Field(f)]` with `[Elem]` yields a representable composed path, and paths beyond the supported depth return `.None` rather than truncating optimistically. Register the suite in `boot/tests/main.tw`.

- [x] **Step 2: Run the new suite and verify it fails**

  Run: `target/twk test --filter "projected borrow"`

  Expected: FAIL because `compiler.projected_borrow` and its interfaces do not exist.

- [x] **Step 3: Implement the pure projected-borrow value module**

  In `boot/compiler/projected_borrow.tw`, define:

  ```tw
  pub type ProjectedBorrow = .{ roots: Vector<Int>, path: AccessPath }
  pub fn make(roots: Vector<Int>, path: AccessPath) ProjectedBorrow
  pub fn same(a: ProjectedBorrow, b: ProjectedBorrow) Bool
  pub fn meet(a: ProjectedBorrow?, b: ProjectedBorrow?) ProjectedBorrow?
  pub fn prefix(seg: PathSeg, b: ProjectedBorrow) ProjectedBorrow?
  ```

  Canonicalize roots with sorted de-duplication. `meet` preserves a fact only when roots and path agree exactly. `prefix` uses only paths representable by the existing `field_facts` codec and returns `.None` on depth overflow.

- [x] **Step 4: Run the pure suite and verify it passes**

  Run: `target/twk test --filter "projected borrow"`

  Expected: PASS.

- [x] **Step 5: Write failing ownership and summary tests**

  Extend `cfg_ownership_suite.tw` and `cfg_summary_suite.tw` with fixtures/assertions for:

  - `item := xs[i]` leaves a seeded-unique `xs` shell unique while `item` is shared;
  - assigning and joining the same projected borrow preserves it, while conflicting roots/paths lose it conservatively;
  - rebinding or replacing `xs[i]` while `item` remains live keeps `item` valid and does not turn it into a shell alias;
  - returning `xs[i]` summarizes as a borrowed projection of parameter `xs`, not `MayAliasParams([xs])`;
  - a caller receiving that return keeps its vector shell eligible but treats the result as shared;
  - storing or closure-capturing that result publishes the element object without publishing the vector shell;
  - reading an inner vector from `Vector<Vector<Person>>` never gives the inner result unique ownership.

- [x] **Step 6: Run focused ownership tests and verify the old transfer fails**

  Run: `target/twk test --filter "projected element"`

  Expected: FAIL because current reference-valued `AIndex` copies base shell origins into the result and summaries have no borrowed-projection return.

- [x] **Step 7: Thread projected borrows through ownership state and joins**

  Add `projected_shell: LocalMap<ProjectedBorrow>` to `ForwardState` and the corresponding block exit/fixpoint state. Seed it empty; copy it through `AInit`/assignment; meet it at CFG joins. When constructing a record, variant, or collection, graft the projected roots into the destination's existing `path_prov` under the destination path; when projecting that path, reconstruct `projected_shell`. Use the same representable-depth policy as `path_prov` and clear the fact at ownership choke points without changing unrelated shell facts. Update warm-cache/fixpoint equality and conservative non-convergence fallback so missing or conflicting projected information cannot create ownership.

- [x] **Step 8: Implement `AIndex` and publication transfer**

  For GC-reference array/vector results, set result ownership to `.Shared`, shell provenance to `[]`, and projected borrow to the base's canonical shell roots composed with `.Elem`. If the base already is a projected borrow, compose its path with `.Elem` only when representable. Publishing such a local invalidates/publishes only the corresponding deep path/object evidence and leaves the root shell ownership unchanged. Primitive index results keep the existing empty-provenance behavior.

- [x] **Step 9: Add borrowed-projection summaries and call transfer**

  Add:

  ```tw
  pub type ProjectedReturn = .{ param: Int, path: AccessPath }
  // field on Summary
  ret_borrow: ProjectedReturn?
  ```

  A function returning one consistent projected borrow emits `ret_borrow`; conflicting return sites and summary-fixpoint non-convergence yield `.None`. Include it in summary equality/rendering. At known calls, re-root the projection through the actual argument, set the call result shared with empty shell provenance, and never use `ret_borrow` to set `flows_to_return`, `ret_exact_param`, `ReturnOwn`, or variant ownership eligibility.

- [x] **Step 10: Run ownership, summary, and sound-uniqueness suites**

  Run: `target/twk test --filter "projected element"`

  Run: `target/twk test --filter "summary"`

  Run: `target/twk test --filter "sound uniqueness"`

  Expected: PASS with existing ownership tests unchanged.

- [x] **Step 11: Format, lint, and commit**

  Run: `target/twk fmt boot/compiler/projected_borrow.tw boot/compiler/ownership.tw boot/compiler/summary.tw boot/tests/suites/projected_borrow_suite.tw boot/tests/suites/cfg_ownership_suite.tw boot/tests/suites/cfg_summary_suite.tw boot/tests/main.tw`

  Run: `target/twk lint boot/main.tw`

  Commit:

  ```bash
  git add boot/compiler/projected_borrow.tw boot/compiler/ownership.tw boot/compiler/summary.tw boot/tests/suites/projected_borrow_suite.tw boot/tests/suites/cfg_ownership_suite.tw boot/tests/suites/cfg_summary_suite.tw boot/tests/main.tw
  git commit -m "feat(ownership): separate vector elements from shell provenance"
  ```

### Task 2: Add Safe Boxed-Family Classification and Exact-Mono ABI Identity

**Status:** Complete in `da2f5efa`.

**Files:**
- Modify: `boot/compiler/elem_family.tw`
- Modify: `boot/compiler/backend/repr_policy.tw`
- Modify: `boot/compiler/codegen/mutvec_call_phase.tw`
- Modify: `boot/compiler/codegen/mutvec_propagate.tw`
- Modify: `boot/compiler/codegen/mutvec_aggregate_region.tw`
- Modify: `boot/compiler/codegen/mutvec_aggregate_rewrite.tw`
- Modify: `boot/compiler/codegen/mutvec_aggregate_phase.tw`
- Test: `boot/tests/suites/repr_policy_suite.tw`
- Test: `boot/tests/suites/mutvec_call_suite.tw`
- Create: `examples/performance/awfy/twinkle/boxed_record_permute.tw`

**Interfaces:**
- Consumes: Task 1's ownership summaries, `MonoType`, `wasm_layout.val_type_of_mono`, and existing family registry.
- Produces: `ElemRepr.Boxed`, `boxed_mutvec_family_of(vector_mono: MonoType) ElemFamily?`, and exact `vector_mono: MonoType` fields on `AggregateMutVecRegion`, `MutVecCallAbiUpgrade`, `FlatSibling`, flat propagation observations, and memo identity. The semantic classifier is exhaustive; the backend separately confirms the resolved Wasm heap type is below `any`.
- Produces: helper sibling identity `(callee_func, handle_param, vector_mono)` and rejection on any mono mismatch before ANF rewriting.

- [x] **Step 1: Write classifier and exact-mono failing tests**

  In `repr_policy_suite.tw`, assert boxed eligibility for concrete record, string, nested-vector, enum, and closure element monos, while primitive behavior remains unchanged. Assert rejection for `ExternRef`, `Optional<ExternRef>`, `Void`, `Never`, `Anyref_`, `Var`, `MetaVar`, and `ErrorType`. In `mutvec_call_suite.tw`, add synthetic propagation tests showing same callee/slot with different boxed monos cannot reuse a sibling, and same callee/mono through different handle slots is rejected or receives a distinct sibling according to the existing slot rule.

- [x] **Step 2: Run focused tests and verify they fail**

  Run: `target/twk test --filter "boxed family"`

  Run: `target/twk test --filter "exact boxed mono"`

  Expected: FAIL because no boxed classification or mono-bearing propagation identity exists.

- [x] **Step 3: Implement the closed boxed classifier**

  Add `ElemRepr.Boxed` and exhaustive mappings in `repr_policy.tw`. Add one boxed `ElemFamily` in `elem_family.tw`, but do not use the spike's `.Vector(_)` catch-all. The selector must first reject semantic sentinels and externref-family types, then accept only `.Vector(element)` whose resolved Wasm type is a GC reference below `.Any`; keep `candidate_typed_vec_family` primitive-only so normal persistent boxed vectors continue using `PVec` rather than becoming a new typed-vector representation.

- [x] **Step 4: Carry exact mono through aggregate decomposition**

  Add `vector_mono: MonoType` to `AggregateMutVecRegion`, populate it from the record field mono, and use it directly in `decompose_clone` and caller reconstruction. Remove primitive-only reconstruction via `mono_of_family`/`elem_of_suffix` wherever boxed routing would erase `T`.

- [x] **Step 5: Carry exact mono through flat propagation and ABI upgrades**

  Add `vector_mono` to `MutVecCallAbiUpgrade`, `ThreadObservation`, `FlatSibling`, propagation arguments/accumulator identity, and backend-facing route records. Replace callee-only memoization with an explicit key or validated tables covering callee, handle parameter, and mono. Before reusing or retargeting a sibling, compare exact monos; mismatch sets the propagation result to rejected before any rewrite is committed.

- [x] **Step 6: Run focused classifier and propagation tests**

  Run: `target/twk test --filter "boxed family"`

  Run: `target/twk test --filter "exact boxed mono"`

  Run: `target/twk test --filter "flatten_helper"`

  Expected: PASS; existing primitive route tests remain green.

- [x] **Step 7: Add the benchmark source while boxed routing is still inactive**

  Create `boxed_record_permute.tw` with a producer-built `Vector<Person>`, recursive carrier, helper-based read-before-write swap, and deterministic checksum. Verify it runs persistently with the current compiler and record this commit as the source-compatible pre-activation baseline candidate; Task 6 will rebuild this exact revision and confirm its WAT before timing.

- [x] **Step 8: Format, lint, and commit**

  Run: `target/twk fmt boot/compiler/elem_family.tw boot/compiler/backend/repr_policy.tw boot/compiler/codegen/mutvec_call_phase.tw boot/compiler/codegen/mutvec_propagate.tw boot/compiler/codegen/mutvec_aggregate_region.tw boot/compiler/codegen/mutvec_aggregate_rewrite.tw boot/compiler/codegen/mutvec_aggregate_phase.tw boot/tests/suites/repr_policy_suite.tw boot/tests/suites/mutvec_call_suite.tw examples/performance/awfy/twinkle/boxed_record_permute.tw`

  Run: `target/twk lint boot/main.tw`

  Commit:

  ```bash
  git add boot/compiler/elem_family.tw boot/compiler/backend/repr_policy.tw boot/compiler/codegen/mutvec_call_phase.tw boot/compiler/codegen/mutvec_propagate.tw boot/compiler/codegen/mutvec_aggregate_region.tw boot/compiler/codegen/mutvec_aggregate_rewrite.tw boot/compiler/codegen/mutvec_aggregate_phase.tw boot/tests/suites/repr_policy_suite.tw boot/tests/suites/mutvec_call_suite.tw examples/performance/awfy/twinkle/boxed_record_permute.tw
  git commit -m "refactor(mutvec): preserve exact vector mono through flat ABIs"
  ```

### Task 3: Add the Compiler-Private Boxed MutVec Runtime

**Status:** Complete in `1232332f`.

**Files:**
- Modify: `boot/compiler/codegen/runtime/types.tw`
- Modify: `boot/compiler/codegen/runtime/arr.tw`
- Modify: `boot/compiler/codegen/wasm_layout.tw`
- Modify: `boot/compiler/builtins.tw`
- Test: `boot/tests/suites/builtins_suite.tw`
- Test: `boot/tests/suites/wasm_layout_suite.tw`
- Test: `boot/tests/suites/runtime_suite.tw`

**Interfaces:**
- Consumes: Task 2's `ElemRepr.Boxed` and boxed family suffix/name.
- Produces: Wasm types `rt_types__MutVecBoxed` and its `Array<anyref>` backing; private builtin/runtime operations `vector$__mutvec_{new,make,push,set,get,len,freeze}` mapped to `rt_arr__mutvec_*`; `mutvec_wasm_type(.Boxed)` and `pvec_wasm_type(.Boxed)`.
- Produces: the same logical-length, capacity-growth, bounds, and freeze behavior as existing primitive MutVec families; no thaw operation.

- [x] **Step 1: Write runtime registry, layout, and generator tests**

  Assert the boxed handle maps to `MutVecBoxed`, freezes to ordinary `PVec`, and has seven registered private operations using `.Anyref` element ABI. Inspect the generated runtime module to verify each operation exists with the expected parameter/result types, set/get perform logical-length guards, growth replaces only private flat backing, and freeze materializes ordinary PVec nodes. Executing these operations through optimized source is deferred to Task 4, when boxed route activation exists.

- [x] **Step 2: Run focused tests and verify they fail**

  Run: `target/twk test --filter "boxed mutvec runtime"`

  Expected: FAIL because the type and private runtime symbols do not exist.

- [x] **Step 3: Define `MutVecBoxed` and layout mappings**

  Add `MutVecBoxed { data: ref Array, len: i32 }` beside the primitive handles. Extend `mutvec_wasm_type` and `pvec_wasm_type` exhaustively so `.Boxed` maps to `MutVecBoxed` and ordinary `PVec` respectively.

- [x] **Step 4: Generate boxed runtime operations from `PVecFamily`**

  Reuse `family_boxed()` with a boxed mutable handle name rather than adding identity box/unbox runtime shims. Instantiate the existing seven-operation MutVec generator with `.Anyref`, null default, ordinary boxed leaf operations, and the existing capacity/bounds algorithms. Ensure freeze materializes an ordinary persistent `PVec` and never aliases mutable backing as persistent trie storage.

- [x] **Step 5: Register the boxed private builtin ABI**

  Extend `MutVecFamilySpec` data with the empty suffix boxed family after all established families. Register its seven runtime definitions without changing existing builtin identities unexpectedly; update identity/canonical registry tests if the append-only table changes expected terminal ids.

- [x] **Step 6: Run runtime, layout, and builtin tests**

  Run: `target/twk test --filter "boxed mutvec runtime"`

  Run: `target/twk test --filter "wasm layout"`

  Run: `target/twk test --filter "builtins"`

  Expected: PASS.

- [x] **Step 7: Format, lint, and commit**

  Run: `target/twk fmt boot/compiler/codegen/runtime/types.tw boot/compiler/codegen/runtime/arr.tw boot/compiler/codegen/wasm_layout.tw boot/compiler/builtins.tw boot/tests/suites/builtins_suite.tw boot/tests/suites/wasm_layout_suite.tw boot/tests/suites/runtime_suite.tw`

  Run: `target/twk lint boot/main.tw`

  Commit:

  ```bash
  git add boot/compiler/codegen/runtime/types.tw boot/compiler/codegen/runtime/arr.tw boot/compiler/codegen/wasm_layout.tw boot/compiler/builtins.tw boot/tests/suites/builtins_suite.tw boot/tests/suites/wasm_layout_suite.tw boot/tests/suites/runtime_suite.tw
  git commit -m "feat(runtime): add boxed mutable vector storage"
  ```

### Task 4: Activate Boxed Recursive Aggregate and Helper Routes

**Status:** Complete. Checkpoint `c1239483` activated the dead-result
`Vector<Person>` recursive/helper route; `1f21ef39` added the publication,
runtime-execution, and backend-repr test surface; `9ebf550e` fixed a stage0
self-host trap the projected-borrow join (Task 1) had latently introduced. With
that fix the boxed route runs end to end: `mutvec_boxed.tw` executes through
boxed `mutvec_get`/`mutvec_set`/`mutvec_freeze` over records, strings, nested
vectors, and closures and reproduces its checksum (84), the boxed OOB fixtures
trap on the logical length, and the self-host fixed point (stage3 == stage4)
holds with the full boot suite green.

- [x] Detect and flatten a producer-rooted recursive `Vector<Person>` carrier.
- [x] Retarget boxed helper reads/writes to empty-suffix `mutvec_get`/`mutvec_set`.
- [x] Preserve exact boxed mono identity and reject cross-mono helper reuse.
- [x] Assign every activated helper handle/result slot as `MutVecBoxed` without an i64 cross-cast.
- [x] Verify the dead-result route emits no freeze and keeps persistent updates out of the clone graph.
- [x] Add whole-carrier and projected-field publication fixtures with exactly one freeze.
- [x] Add the boxed runtime execution/OOB/type-round-trip fixture matrix.
- [x] Add explicit backend rejection tests for boxed value/element mono mismatches.
- [x] Run Task 4's complete focused verification, format/lint, and final task commit.

**Self-host note:** The projected-borrow CFG join (`ownership.join_entry_projected`)
threads an `Option<ProjectedBorrow>` accumulator sourced from a generic
`LocalMap.get` and `pb.meet`. Matching that slot directly leaves its variant tag
misrepresented under stage0 codegen, tripping stage0's non-exhaustive-match
fallback the first time the join runs on real self-host input. Re-materializing
the accumulator through a typed-return boundary (`reproject`) before the match
restores it — the same discipline `join_entry_prov` already uses.

**Files:**
- Modify: `boot/compiler/codegen/mutvec_region.tw`
- Modify: `boot/compiler/codegen/mutvec_call_region.tw`
- Modify: `boot/compiler/codegen/mutvec_call_verify.tw`
- Modify: `boot/compiler/codegen/mutvec_propagate.tw`
- Modify: `boot/compiler/codegen/mutvec_aggregate_phase.tw`
- Modify: `boot/compiler/backend/mutvec_repr.tw`
- Modify: `boot/compiler/backend/verify_expr.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_swap.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_escape.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_field_escape.tw`
- Create: `boot/tests/suites/fixtures/mutvec_boxed.tw`
- Create: `boot/tests/suites/fixtures/mutvec_boxed_oob.tw`
- Test: `boot/tests/suites/mutvec_call_suite.tw`
- Test: `boot/tests/suites/backend_repr_suite.tw`

**Interfaces:**
- Consumes: Task 1 projected-borrow ownership, Task 2 exact-mono ABI records/classifier, and Task 3 boxed runtime operations.
- Produces: accepted producer-rooted `Vector<Person>` aggregate routes, boxed `$mv` helper siblings, boxed `$mvagg` recursive siblings, and backend slots physically represented as `MutVecBoxed` while retaining exact logical mono.
- Produces: verifier rejection before emission when boxed get/set value/result types disagree with the handle's `Vector<T>` mono.

- [ ] **Step 1: Add realistic record-swap route and WAT tests**

  Create a fixture whose producer-built `Vector<Person>` is carried in a recursive record, whose helper reads two records then writes them back swapped, and whose scalar result supplies a checksum. Add dead-field, publish-record, and publish-field variants. Add runtime fixtures covering empty/singleton construction, growth by push, reference get/set, freeze, records/strings/nested vectors/closures round-tripping, negative index, logical-length OOB, and oversized index traps. Assert route acceptance, correct execution, boxed get/set calls in the propagated clone graph, absence of persistent `rt_arr__set` in that graph, and zero/one freeze boundaries.

- [ ] **Step 2: Run the boxed aggregate tests and verify they fail**

  Run: `target/twk test --filter "boxed recursive record"`

  Run: `target/twk test --filter "boxed mutvec runtime"`

  Expected: FAIL because boxed family activation and backend representation assignment are not wired.

- [ ] **Step 3: Enable boxed region and helper verification**

  Route eligible boxed producers through existing `classify_producer_prime`, region detection, and aggregate verification. Retarget index reads/writes to the empty-suffix boxed runtime operations only after the verifier confirms the handle's exact `Vector<T>` mono and the operation result/value mono equals `T`. Preserve exhaustive reject-by-default matching for unsupported ANF ops.

- [ ] **Step 4: Enable boxed backend representation assignment**

  In `mutvec_repr.tw`, derive `.MutVec(.Boxed)` from the ABI upgrade's exact `vector_mono` and validated boxed family rather than defaulting an unknown family to `.I64`. Type handle params/results as `MutVecBoxed`; leave loaded/stored logical values at their exact GC reference type. Extend backend verification to reject missing mono, externref, sentinel, or value/element mismatch before Wasm emission.

- [ ] **Step 5: Wire caller freeze and reconstruction with the exact mono**

  Resolve empty-suffix boxed builtins, use `region.vector_mono` for the producer handle and frozen result, and reconstruct the original nominal carrier without substituting `.Vector(.Anyref_)`. Keep the existing all-or-nothing plan reservation so a boxed failure leaves the original persistent caller and clone graph untouched.

- [ ] **Step 6: Run boxed route, backend, and primitive regression tests**

  Run: `target/twk test --filter "boxed recursive record"`

  Run: `target/twk test --filter "backend repr"`

  Run: `target/twk test --filter "mutvec call"`

  Expected: PASS, including existing primitive recursive aggregate cases.

- [ ] **Step 7: Format, lint, and commit**

  Run: `target/twk fmt boot/compiler/codegen/mutvec_region.tw boot/compiler/codegen/mutvec_call_region.tw boot/compiler/codegen/mutvec_call_verify.tw boot/compiler/codegen/mutvec_propagate.tw boot/compiler/codegen/mutvec_aggregate_phase.tw boot/compiler/backend/mutvec_repr.tw boot/compiler/backend/verify_expr.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_swap.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_escape.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_field_escape.tw boot/tests/suites/fixtures/mutvec_boxed.tw boot/tests/suites/fixtures/mutvec_boxed_oob.tw boot/tests/suites/mutvec_call_suite.tw boot/tests/suites/backend_repr_suite.tw`

  Run: `target/twk lint boot/main.tw`

  Commit:

  ```bash
  git add boot/compiler/codegen/mutvec_region.tw boot/compiler/codegen/mutvec_call_region.tw boot/compiler/codegen/mutvec_call_verify.tw boot/compiler/codegen/mutvec_propagate.tw boot/compiler/codegen/mutvec_aggregate_phase.tw boot/compiler/backend/mutvec_repr.tw boot/compiler/backend/verify_expr.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_swap.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_escape.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_field_escape.tw boot/tests/suites/fixtures/mutvec_boxed.tw boot/tests/suites/fixtures/mutvec_boxed_oob.tw boot/tests/suites/mutvec_call_suite.tw boot/tests/suites/backend_repr_suite.tw
  git commit -m "feat(mutvec): optimize recursive reference vectors"
  ```

### Task 5: Prove Boxed Fallback and ABI Isolation End to End

**Files:**
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_alias.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_capture.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_mixed_lineage.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_nested_borrow.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_mixed_mono.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_externref.tw`
- Modify: `boot/tests/suites/mutvec_call_suite.tw`

**Interfaces:**
- Consumes: the complete boxed route from Task 4.
- Produces: integration evidence that every rejected boxed shape remains on the persistent ABI and that no partial boxed sibling/upgrade survives rejection.

- [ ] **Step 1: Add end-to-end fallback fixtures and assertions**

  Cover a surviving vector alias, element closure capture, fresh/mixed recursive lineage, nested-vector indexed borrow used as an attempted inner mutation root, `Person`/`String` calls reaching one source helper, `ExternRef` elements, handle duplication across callee slots, and a deliberately exhausted variant cap. For each, assert the documented accepted or rejected outer route, exact sibling identity, absence of incompatible retargeting, byte-valid compilation, and correct runtime result.

- [ ] **Step 2: Run fallback tests and inspect failures**

  Run: `target/twk test --filter "boxed fallback"`

  Expected: new tests either pass immediately through conservative behavior or expose a partial-rewrite/identity bug; any failure must be fixed without weakening the fixture.

- [ ] **Step 3: Make rejection transactional where needed**

  If a failing fixture exposes mutation before full graph acceptance, change planning to reserve every sibling/ABI upgrade and validate all exact monos first, then append rewritten functions and caller changes only when the whole chain is accepted. Do not add boxed-specific ownership exceptions.

- [ ] **Step 4: Run focused and full boot tests**

  Run: `target/twk test --filter "boxed fallback"`

  Run: `target/twk test --filter "mutvec call"`

  Run: `target/twk test`

  Expected: PASS.

- [ ] **Step 5: Format, lint, and commit**

  Run: `target/twk fmt boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_alias.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_capture.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_mixed_lineage.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_nested_borrow.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_mixed_mono.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_externref.tw boot/tests/suites/mutvec_call_suite.tw`

  Run: `target/twk lint boot/main.tw`

  Commit:

  ```bash
  git add boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_alias.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_capture.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_mixed_lineage.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_nested_borrow.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_boxed_mixed_mono.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_externref.tw boot/tests/suites/mutvec_call_suite.tw
  git commit -m "test(mutvec): guard boxed reference fallbacks"
  ```

### Task 6: Establish the Record-Vector Performance Gate

**Status:** PASS (6.9×, gate 3.0×).

**Result.** `examples/performance/awfy/twinkle/boxed_record_permute.tw`
(producer-built `Vector<Person>`, recursive `State` carrier, helper read-before-write
swap, checksum 8696) self-times via the AWFY `harness` (warmup 10, iters 20, size 300).

- **Boxed candidate** (route active): 167.94 / 167.51 / 167.13 ms → **median 167.5 ms**.
- **Persistent baseline** (`TWINKLE_VARIANT_SPECIALIZE=0`): 1158.89 / 1139.87 / 1165.99 ms → **median 1158.9 ms**.
- **Ratio: 6.9×** faster, ≥ the 3.0× gate. Checksums agree (8696) on both paths.
- **Route evidence (WAT).** Boxed path emits `mutvec_get`/`mutvec_set`/`mutvec_freeze`
  and no persistent set in the hot clone graph; the persistent path emits zero
  `mutvec_*` ops. Toggled by the single flag, same compiler, same source.

**Methodology note (deviation from the two-worktree recipe below).** The baseline
is the *same* candidate compiler with `TWINKLE_VARIANT_SPECIALIZE=0`, the in-code
sanctioned kill-switch that makes the compiler "fall back to the generic
persistent path" (`codegen.tw`). This is a stricter control than a separate
pre-activation CLI — it holds compiler version and source **identical**, isolating
exactly the boxed route — and it sidesteps that the recorded pre-activation
commit (Task 3, `1232332f`) cannot self-host without the Task-4 self-host fix
`9ebf550e`. The benchmark uses the explicit record-rebuild carrier form because
the idiomatic field-rebind sugar (`rec.f = ...`) does not yet box; that gap is
tracked in `docs/plans/2026-09-29-boxed-mutvec-field-rebind-sugar.md`.

**Files:**
- Test: `examples/performance/awfy/twinkle/boxed_record_permute.tw`
- Modify: `docs/plans/2026-09-28-boxed-reference-mutvec.md`

**Interfaces:**
- Consumes: Task 4's ordinary optimized `Vector<Person>` route and the parent commit immediately before Task 4 route activation as the persistent compiler baseline.
- Produces: a deterministic checksum workload, recorded compiler revisions/commands, route-selection evidence, three-sample medians, and a pass/fail decision against the `3.0x` gate.

- [ ] **Step 1: Validate the benchmark's recorded pre-activation revision**

  Use the fixture committed in Task 2, before Task 4's activation commit, so identical source compiles with both compilers. Confirm it uses a producer-built `Vector<Person>`, recursive record carrier, helper-based read-before-write swap, and observable checksum. Record the Task 3 commit as the pre-activation compiler revision when its WAT confirms persistent vector updates.

- [ ] **Step 2: Prove correctness and representation for both compilers**

  Build standalone CLIs from the recorded pre-activation commit and the completed candidate in separate worktrees. Run the benchmark once with each and assert identical checksum. Emit WAT: baseline must call persistent vector update; candidate hot clone graph must call boxed `mutvec_get`/`mutvec_set`, contain no persistent update, and preserve zero/one freeze behavior in its dead/published fixture variants.

- [ ] **Step 3: Run three benchmark rounds for each compiler**

  Use the repository AWFY runner and the same host/session settings used for the existing Permute gate. Alternate baseline and candidate runs to reduce drift, retain every sample, and compare medians. Do not compare the write-only spike or a deliberately pessimized source variant.

- [ ] **Step 4: Apply the performance decision**

  PASS when candidate median time is at least `3.0x` faster than the persistent parent-compiler baseline, checksums agree, and WAT proves the intended route. If it misses, profile the residual and open a focused follow-up; do not weaken ownership, exact-mono, or ABI checks and do not mark this plan complete.

- [ ] **Step 5: Record evidence, format, lint, and commit**

  Add compiler commit ids, exact commands, samples, medians, ratio, checksum, and WAT route/freeze evidence to this task's status section without changing the prescribed gate.

  Run: `target/twk fmt examples/performance/awfy/twinkle/boxed_record_permute.tw`

  Run: `target/twk lint examples/performance/awfy/twinkle/boxed_record_permute.tw`

  Commit:

  ```bash
  git add examples/performance/awfy/twinkle/boxed_record_permute.tw docs/plans/2026-09-28-boxed-reference-mutvec.md
  git commit -m "perf(mutvec): verify boxed record vector speedup"
  ```

### Task 7: Self-Host, Full Verification, and Branch Review

**Status:** Verification complete; independent review (Step 4) not yet run.

- **Self-host fixed point:** `make bundle-cli` then `make stage2` both reach
  `Fixed point reached: stage3 == stage4`. The projected-borrow join needed a
  self-host fix (`9ebf550e`, see the Task 4 self-host note) — it was the first
  self-host of that code.
- **Full verification:** `target/twk test` → `Ran 3775 tests: 3775 passed`;
  `target/twk lint boot/main.tw` → no findings; `git diff --check` clean.
- **WAT audit:** boxed benchmark emits `mutvec_get`/`set`/`freeze` with no
  persistent set in its clone graph; the persistent path
  (`TWINKLE_VARIANT_SPECIALIZE=0`) emits none; publication fixtures emit exactly
  one boundary freeze; all six fallback fixtures stay on the persistent ABI with
  no cross-mono retarget.
- **Remaining:** Step 4 independent branch review (soundness focus areas below);
  and the deferred idiomatic field-rebind-sugar boxing route
  (`docs/plans/2026-09-29-boxed-mutvec-field-rebind-sugar.md`).

**Files:**
- Modify only files required by failures attributable to this feature.
- Modify: `docs/plans/2026-09-28-boxed-reference-mutvec.md` with final status and any durable implementation notes.

**Interfaces:**
- Consumes: all prior task commits and performance evidence.
- Produces: a self-host fixed point, complete test/lint evidence, and an independent soundness review with all findings resolved or explicitly blocking completion.

- [ ] **Step 1: Rebuild and verify the self-host fixed point**

  Run: `make bundle-cli`

  Run: `make stage2`

  Expected: both complete and the repository's stage comparison reports a fixed point.

- [ ] **Step 2: Run complete verification**

  Run: `make test`

  Run: `target/twk lint boot/main.tw`

  Run: `target/twk lint examples/performance/awfy/twinkle/boxed_record_permute.tw`

  Run: `git diff --check`

  Expected: all suites pass; lint has no new actionable findings; no whitespace errors. Never run tree-sitter tests.

- [ ] **Step 3: Audit generated WAT and fallback output**

  Use `target/twk wat` on the accepted record-swap benchmark and representative fallback fixtures. Confirm boxed ops and freeze counts only on accepted graphs, ordinary PVec ABI on rejected graphs, and no `Vector<Person>`/`Vector<String>` cross-retarget.

- [ ] **Step 4: Request an independent branch review**

  Ask the reviewer to focus on projected-borrow summary soundness, externref/sentinel exclusions, exact-mono identity, transactional fallback, persistent-backing isolation, self-host stability, and the benchmark's fairness. Fix every confirmed issue with focused regression coverage and rerun the affected verification.

- [ ] **Step 5: Record final evidence and commit**

  Update this plan with the final implementation commits, verification commands/results, performance conclusion, and any intentionally deferred scope such as consume/replace ownership for nested vectors.

  Commit:

  ```bash
  git add docs/plans/2026-09-28-boxed-reference-mutvec.md
  git commit -m "docs(mutvec): record boxed reference verification"
  ```
