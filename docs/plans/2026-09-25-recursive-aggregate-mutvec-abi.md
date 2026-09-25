# Recursive Aggregate MutVec ABI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make ordinary immutable Twinkle code shaped like AWFY `permute.tw` automatically stay in flat mutable vector storage through recursive aggregate threading, reaching the `permute_mut`/LuaJIT performance class without requiring `@std.buffer` or a separate mutable source pattern.

**Architecture:** Extend the landed S4 interprocedural MutVec pipeline beyond a bare `Vector<T> -> Vector<T>` continuation. Detect an owned record carrier whose vector field preserves lineage through a recursive SCC while scalar fields are independently threaded, create a compiler-private decomposed clone ABI (`MutVec<T>` handle plus scalar parameters/results), rewrite the clone and every accepted caller/recursive edge, and freeze/reconstruct only at a proven publication boundary. The generic persistent function remains unchanged as the fallback for rejected or incompatible sites.

**Tech Stack:** Self-hosted Twinkle boot compiler, ANF ownership summaries and variant routing, `mutvec_call_*` S4 passes, typed Wasm-GC `MutVecI64`, prepared backend representation assignment, boot test suites, AWFY benchmark harness.

**Spec:** `docs/plans/sound-uniqueness/README.md` (“New S4 evidence: recursive aggregate carrier with a dead collection result”) and `docs/plans/sound-uniqueness/storage/README.md` (“S4 — Owned-specialized mutable ABI across calls”).

## Global Constraints

- Ordinary `examples/performance/awfy/twinkle/permute.tw` is the optimization target; `permute_mut.tw` is only a checksum and performance oracle.
- Do not add a public mutable-vector API, annotation, trait, source rewrite, or required `@std.buffer` usage.
- Preserve immutable value semantics. Any missing ownership, lineage, exit, representation, or ABI proof must select the existing persistent function.
- Reuse the existing `variant_specialize` route/cap discipline and `mutvec_call_phase`; do not create a second ownership authority.
- Keep the generic persistent function available for shared, aliased, unsupported, or over-cap callers.
- Keep private storage flat across every accepted recursive/SCC edge. A dead vector result emits zero freezes; an observed vector result emits exactly one freeze at its publication boundary.
- This slice accepts exactly one independently threaded scalar field in addition to the vector field. Carriers with zero or multiple threaded scalar fields remain persistent until a measured performance need justifies a different or multi-result ABI.
- Ownership evidence for post-specialization clones must be recomputed under the clone's owned seed. Pre-clone `SpecializeResult.summary` and `pruned_view` may seed analysis but must not directly license an aggregate rewrite.
- The implementation focus is the boot compiler. Update Rust stage0 only if bootstrap requires it.
- Format and lint every modified `.tw` file. Never run tree-sitter tests.
- Every behavior-changing task follows red-green-refactor and ends with a focused commit.

## Review Focus

- A caller retaining the pre-call carrier or vector must stay persistent; Task 3 adds an alias-survival negative fixture.
- A recursive branch that returns a different vector lineage must stay persistent; Task 3 adds a mixed-lineage negative fixture.
- A returned vector observed after the call must freeze exactly once, never zero or once per recursion; Tasks 3 and 5 cover this.
- Multiple callers sharing one owned clone must partition into persistent and mutable siblings without ABI crossing; Task 5 extends the existing mixed-caller test.
- Variant-cap overflow or an unsupported carrier field type must leave byte-valid persistent code; Tasks 3 and 5 test fallback.
- A same-family fresh vector substituted on any recursive edge must stay persistent even when its element representation agrees; Task 3 tests handle-identity rejection.

---

## Status (2026-09-25)

Tasks 0–3 landed. Task 2 required a foundation the plan under-specified: owned
specialization only proved whole-record aliasing, so a reconstructed carrier
lost its vector-field lineage. That is now a separate prerequisite commit,
`feat(ownership): track vector field lineage through record reconstruction`
(`ReturnOwn.OwnedFromField`, `Summary.ret_exact_param`, a per-field flow
lattice, and field-tier variant validation), which makes owned reconstruction
routes preserve the vector field. `detect_aggregate_regions` and the census
section sit on top of it. The scratch fixture and the real AWFY Permute both
report a `dead_field` `vec_i64` region (carrier `p0`, vector `.f0`, scalar
`.f1`). Task 3 adds `verify_aggregate_region` as the aggregate-ABI acceptance gate
(single-authority: routing already discharges lineage, so alias/mixed/fresh
shapes yield no region; the verifier judges scalar arity/type, recursion, exit)
and joins the verdict into the census. Remaining: the Task 4–5 decomposed-ABI
rewrite (the four `permute$mvagg` route/WAT tests are still tracked-red by
design).

**Task 4–5 paused for design.** Scoping surfaced that both the scratch fixture
and the real `permute.tw` write the vector inside a `swap` helper, and no
inliner runs before S4 — so the in-place `mutvec_set` must land *through* the
helper, which the committed WAT test (expecting it inside `permute$mvagg`) does
not anticipate. The decomposition design (scalar-only return, in-place mutable
handle, control flow carries only the scalar), the recommended helper-write
approach (compose with the existing bare-vector S4 rather than add an inliner),
caller thaw/freeze rules, and bail conditions are captured in
`docs/plans/2026-09-26-aggregate-abi-rewrite-design.md`. The open question was
resolved by prototype (2026-09-26): bare-vector S4 does **not** flatten a
param-sourced handle threaded into `swap` (it roots regions only on
`collect`/`make` producers), so Option A does not compose for free. The design
doc now recommends **Option B** — a narrow, self-contained `swap` beta-reduce
inside the decomposed clone: single flat clone, matches the committed
`permute$mvagg` WAT test verbatim, tightest hot loop.

## File Map

- Create `boot/compiler/codegen/mutvec_aggregate_region.tw`: detect record-carried vector lineage and describe decomposed ABI requirements without rewriting code.
- Create `boot/compiler/codegen/mutvec_aggregate_verify.tw`: validate ownership, recursive/SCC continuity, exits, aliases, and scalar-field independence.
- Modify `boot/compiler/codegen/variant_specialize.tw`: close owned recursive routes across every member of a demanded SCC before aggregate analysis runs.
- Modify `boot/compiler/codegen/mutvec_call_phase.tw`: merge accepted aggregate regions into S4 planning, generate mutable siblings, and apply the ANF signature/body/caller rewrite.
- Modify `boot/compiler/codegen/mutvec_call_verify.tw`: expose the existing clone/site/representation helpers shared by aggregate verification; do not duplicate them.
- Modify `boot/compiler/backend/mutvec_repr.tw`: consume decomposed ABI upgrades and type the new vector parameter/call arguments as `MutVec(fam)` while scalar slots retain their ordinary representations.
- Modify `boot/compiler/codegen/codegen.tw`: thread aggregate decision/audit data through the existing S4 stage without adding a second pass ordering point.
- Modify `boot/commands/ir.tw`, `boot/compiler/census.tw`, and `boot/compiler/codegen/mutable_audit.tw`: render aggregate-carrier decisions, exits, and fallback reasons under `twk ir --census --sites`.
- Create fixtures under `boot/tests/fixtures/cfg/mutvec_call/`: positive dead-result Permute shape, positive observed-result shape, and negative alias/mixed-lineage/unsupported shapes.
- Modify `boot/tests/suites/mutvec_call_suite.tw`: analysis, rewrite, WAT, fallback, and freeze-count tests.
- Modify `examples/performance/awfy/twinkle/permute.tw` only if a benchmark correctness guard is needed; do not change its algorithm or source-level data model.
- Modify `docs/plans/sound-uniqueness/README.md`, `docs/plans/sound-uniqueness/storage/README.md`, and `examples/performance/awfy/README.md` after the ordinary benchmark meets the gate.

### Task 0: Close Owned Variant Routes Across Recursive SCCs

**Files:**
- Modify: `boot/compiler/codegen/variant_specialize.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_mutual.tw`
- Modify: `boot/tests/suites/mutvec_call_suite.tw`

**Interfaces:**
- Consumes: the existing call graph, ownership-published variants, `seed_for_variant`, `field_seed_for_variant`, and `variant_cap`.
- Produces: a `SpecializeResult` in which a demanded owned route for one SCC member either has compatible owned clones and recursive routes for every member, or the whole SCC demand remains generic.

- [x] **Step 1: Add a mutually recursive carrier fixture and failing route test**

Create two functions that alternate while threading `State.{ values: Vector<Int>, count: Int }`. Assert that an owned entry demand routes both functions to owned clones and that every in-SCC call targets the corresponding clone. Also add a cap-pressure case and assert that it leaves the entire SCC generic rather than cloning only a prefix.

- [x] **Step 2: Run the focused tests and verify RED**

Run `target/twk test --filter "recursive SCC route closure"`.

Expected: the entry-demanded member is cloned, but a peer demanded only from inside the SCC remains generic.

- [x] **Step 3: Form and close demanded SCC clone groups**

Compute SCC membership before clone allocation. For each demanded owned variant, derive compatible owned seeds for all reachable members of the same SCC, reserve the complete clone group under the per-generic `variant_cap`, then create and route the group atomically. If any member lacks a compatible published variant, supported update, route, or cap slot, create none of the group.

- [x] **Step 4: Recompute recursive routes under each member's seed**

Analyze each created clone with its whole-value and field seed, and require every in-SCC edge to resolve to the peer clone belonging to the same closed group. Record all new clones and rewritten callers in `changed_funcs` so downstream ownership production rebuilds their CFG facts.

- [x] **Step 5: Run specialization and existing MutVec tests**

```bash
target/twk test --filter "recursive SCC route closure"
target/twk test --filter "variant specialize"
target/twk test --filter "mutvec call"
```

Expected: mutually recursive owned routes are all-or-nothing, self-recursive behavior is unchanged, and cap overflow produces persistent code.

- [x] **Step 6: Commit the prerequisite**

```bash
git add boot/compiler/codegen/variant_specialize.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_mutual.tw boot/tests/suites/mutvec_call_suite.tw
git commit -m "feat(variants): close owned routes across recursive SCCs"
```

### Task 1: Pin the Ordinary Recursive-Carrier Shape

**Files:**
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_scratch.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_escape.tw`
- Modify: `boot/tests/suites/mutvec_call_suite.tw`

**Interfaces:**
- Consumes: existing `mutvec_call_stage`, `compile_fixture_wat`, `wat_func_body`, and `count_op_calls` test helpers.
- Produces: stable positive fixtures whose carrier is `State.{ values: Vector<Int>, count: Int }`; the scratch fixture returns only `count`, while the escape fixture returns `State` and observes `values`.

- [x] **Step 1: Add the scratch fixture using normal immutable Twinkle**

```tw
type State = .{ values: Vector<Int>, count: Int }

fn swap(values: Vector<Int>, i: Int, j: Int) Vector<Int> {
  a := values[i]
  b := values[j]
  values.set_at(i, b).set_at(j, a)
}

fn permute(state: State, n: Int) State {
  next := State.{ values: state.values, count: state.count + 1 }
  if n == 0 {
    next
  } else {
    next = permute(next, n - 1)
    i := n - 1
    for i >= 0 {
      next = State.{ values: swap(next.values, n - 1, i), count: next.count }
      next = permute(next, n - 1)
      next = State.{ values: swap(next.values, n - 1, i), count: next.count }
      i = i - 1
    }
    next
  }
}

values: Vector<Int> = collect _i in range(6) { 0 }
result := permute(State.{ values, count: 0 }, 6)
println(result.count.to_string())
```

Use the same definitions in `recursive_record_escape.tw`, but return the whole `State` from a public `run` and read both `count` and `values[0]` at top level so `.PublishRecord` is observable. Add `recursive_record_field_escape.tw`, whose caller observes only the returned `values` projection, to pin `.PublishField` separately.

- [x] **Step 2: Add failing structural tests**

Add tests named:

```tw
"recursive record scratch gets an aggregate MutVec route"
"recursive record scratch emits mutvec_set_i64 with zero freezes"
"recursive record escape emits exactly one boundary freeze"
"recursive record field escape emits exactly one boundary freeze"
```

The first test must inspect the S4 decision rather than grep source. The WAT tests must assert `mutvec_set_i64` is present in the private clone, `rt_arr__set` is absent from that clone, and freeze counts are zero/one respectively.

- [x] **Step 3: Run the tests and verify RED**

Run:

```bash
target/twk test --filter "recursive record"
```

Expected: the fixtures compile and return the correct checksum, but the decision/WAT assertions fail because current S4 only recognizes a bare vector continuation.

- [x] **Step 4: Capture the current diagnostic baseline**

Run:

```bash
target/twk ir boot/tests/fixtures/cfg/mutvec_call/recursive_record_scratch.tw --census --sites
```

Record in the test comment that the current update reaches persistent `set_at__Int`/`rt_arr__set` and no aggregate MutVec region is reported.

- [x] **Step 5: Commit the red characterization**

```bash
git add boot/tests/fixtures/cfg/mutvec_call/recursive_record_*.tw boot/tests/suites/mutvec_call_suite.tw
git commit -m "test(mutvec): characterize recursive aggregate carrier"
```

### Task 2: Detect Aggregate Carrier Lineage Without Rewriting

**Files:**
- Create: `boot/compiler/codegen/mutvec_aggregate_region.tw`
- Modify: `boot/compiler/codegen/mutvec_call_phase.tw`
- Modify: `boot/commands/ir.tw`
- Modify: `boot/compiler/census.tw`
- Modify: `boot/compiler/codegen/mutable_audit.tw`
- Modify: `boot/tests/suites/mutvec_call_suite.tw`

**Interfaces:**
- Consumes: `variant_specialize.SpecializeResult`, ownership return-path/field facts, `mutvec_call_region.route_site_targets`, `mutvec_region.collect_defs`, `elem_family_of`.
- Produces:

```tw
pub type AggregateMutVecExit = { DeadField, PublishRecord, PublishField }

pub type AggregateScalarField = .{
  field: FieldId,
  mono: MonoType,
}

pub type AggregateMutVecRegion = .{
  caller_func: FuncId,
  generic_func: FuncId,
  clone_func: FuncId,
  carrier_param: Int,
  carrier_type: TypeId,
  vector_field: FieldId,
  family: String,
  scalar_fields: Vector<AggregateScalarField>,
  entry_call: LocalId,
  recursive_calls: Vector<LocalId>,
  exit: AggregateMutVecExit,
  proof_id: String,
}

pub fn detect_aggregate_regions(
  spec: variant_specialize.SpecializeResult,
  builtins: BuiltinRegistry,
) Vector<AggregateMutVecRegion>
```

- [x] **Step 1: Add detector unit tests before implementation**

Assert that the scratch fixture yields one region with the `values` field, `Int` family, `count` scalar field, recursive call sites, and `.DeadField` exit. Assert that the record-escape and field-escape fixtures yield `.PublishRecord` and `.PublishField` respectively. Assert that the mutually recursive fixture from Task 0 reports every in-SCC call in its closed recursive route set.

- [x] **Step 2: Run detector tests and verify RED**

Run `target/twk test --filter "aggregate carrier detector"`.

Expected: compile failure because `mutvec_aggregate_region` and its public API do not exist.

- [x] **Step 3: Implement sound-by-rejection lineage discovery**

Walk only ownership-routed clones. Identify one record parameter whose projected vector field:

1. has a registered MutVec family;
2. flows into every replacement `ARecord`/`ARecordUpdate` at the same field;
3. is returned through the same record field on every recursive SCC edge;
4. has no second collection field competing for the same private handle;
5. reaches either no observed exit, one returned field, or one returned carrier.

Use ownership summary/field-path evidence for lineage and liveness. Syntax matching may locate candidate record operations, but it must not license mutation by itself. Detection may use pre-clone summaries only to find candidates; acceptance is deferred until Task 3 recomputes clone-local evidence under the routed owned seed.

- [x] **Step 4: Render decisions before enabling rewriting**

Add a `mutvec aggregate regions:` census section with columns:

```text
func clone carrier vector_field family scalars recursion exit would_use reason proof
```

At this task, `would_use` remains `false` and reason is `analysis-only`.

- [x] **Step 5: Run focused tests and inspect the real Permute report**

```bash
target/twk test --filter "aggregate carrier detector"
target/twk ir examples/performance/awfy/twinkle/main.tw --census --sites | rg -C 3 "permute|mutvec aggregate"
```

Expected: the fixture and ordinary AWFY Permute are detected; generated WAT is unchanged.

- [x] **Step 6: Commit analysis-only detection**

```bash
git add boot/compiler/codegen/mutvec_aggregate_region.tw boot/compiler/codegen/mutvec_call_phase.tw boot/commands/ir.tw boot/compiler/census.tw boot/compiler/codegen/mutable_audit.tw boot/tests/suites/mutvec_call_suite.tw
git commit -m "feat(mutvec): detect recursive aggregate carriers"
```

### Task 3: Verify Safety and Pin Persistent Fallbacks

**Files:**
- Create: `boot/compiler/codegen/mutvec_aggregate_verify.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_alias.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_mixed_lineage.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_fresh_lineage.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_unsupported.tw`
- Create: `boot/tests/fixtures/cfg/mutvec_call/recursive_record_multi_scalar.tw`
- Modify: `boot/compiler/codegen/mutvec_call_verify.tw`
- Modify: `boot/tests/suites/mutvec_call_suite.tw`

**Interfaces:**
- Consumes: `AggregateMutVecRegion`, `SpecializeResult`, ownership summaries, and shared helpers `clone_site_targets`, `func_def_of`, `repr_agrees`.
- Produces:

```tw
pub type AggregateRejectReason = {
  CarrierAliasSurvives,
  FieldAliasSurvives,
  MixedFieldLineage,
  RecursiveRouteMissing,
  CalleeRepublishes,
  ExitUnclassified,
  ReprDisagree,
  UnsupportedScalarField,
  UnsupportedScalarArity,
}

pub type AggregateVerdict = { Accept, Reject(AggregateRejectReason) }

pub fn verify_aggregate_region(
  region: AggregateMutVecRegion,
  spec: variant_specialize.SpecializeResult,
  builtins: BuiltinRegistry,
) AggregateVerdict
```

- [x] **Step 1: Add negative fixtures and failing verdict tests**

The alias fixture saves `state.values` before recursion and reads it afterward. The mixed-lineage fixture returns a newly constructed unrelated vector on one branch. The fresh-lineage fixture constructs a new `Vector<Int>` on one recursive edge, proving that matching element family is not handle identity. The unsupported fixture uses `Vector<String>`. The multi-scalar fixture independently updates two scalar fields. Tests must assert a named rejection reason and ordinary persistent WAT.

- [x] **Step 2: Run tests and verify RED**

Run `target/twk test --filter "aggregate carrier verifier"`.

Expected: compile failure because the verifier API is absent.

- [x] **Step 3: Implement the verifier using ownership facts**

Locate the route's owned seed and recompute ownership/field-path facts over the post-specialization clone body rather than trusting the pre-clone summary. Require deep ownership of the vector field at entry, deadness of the old field after every replacement, exact handle identity across every recursive edge, consistent closed-SCC clone routing, one supported family, no publication inside the SCC, and a classified exit. Accept exactly one independently threaded scalar field whose MonoType has an ordinary scalar Wasm representation and whose dataflow never contains the vector handle. Reject zero or multiple threaded scalar fields with `UnsupportedScalarArity`; the scalar may be dead at a particular caller boundary, but the private ABI remains uniform across all accepted sites.

- [x] **Step 4: Make rejection visible and leave codegen unchanged**

Join verdicts into the census rows. Accepted rows still say `would_use=false (rewrite not landed)`; rejected rows print the exact `AggregateRejectReason`.

- [x] **Step 5: Run positive, negative, and existing S4 tests**

```bash
target/twk test --filter "aggregate carrier"
target/twk test --filter "mutvec call"
```

Expected: positives accept, negatives reject with the expected reason, and existing bare-vector S4 behavior remains unchanged.

- [x] **Step 6: Commit the safety gate**

```bash
git add boot/compiler/codegen/mutvec_aggregate_verify.tw boot/compiler/codegen/mutvec_call_verify.tw boot/tests/fixtures/cfg/mutvec_call/recursive_record_*.tw boot/tests/suites/mutvec_call_suite.tw
git commit -m "feat(mutvec): verify recursive aggregate carriers"
```

### Task 4: Rewrite the Private Clone to a Decomposed ABI

**Files:**
- Modify: `boot/compiler/codegen/mutvec_call_phase.tw`
- Modify: `boot/compiler/backend/mutvec_repr.tw`
- Modify: `boot/compiler/codegen/codegen.tw`
- Modify: `boot/tests/suites/mutvec_call_suite.tw`

**Interfaces:**
- Consumes: accepted `AggregateMutVecRegion` records.
- Produces an extension of the existing ABI decision:

```tw
pub type AggregateMutVecAbiUpgrade = .{
  clone_func: Int,
  carrier_param: Int,
  vector_param: Int,
  scalar_param: Int,
  scalar_return: Int,
  family: ElemRepr,
  exit: AggregateMutVecExit,
}
```

`MutVecCallDecision` gains `aggregate_upgrades: Vector<AggregateMutVecAbiUpgrade>`. This slice deliberately uses the backend's existing single `phys_return`: the mutable handle remains live in the caller and the clone returns the one updated scalar. Multi-scalar returns are persistent fallback, not an implicit multi-value ABI.

- [ ] **Step 1: Add failing ANF-shape tests**

For the accepted scratch clone, assert:

- the mutable sibling has a vector parameter plus scalar parameters, not a `State` parameter;
- recursive calls target that sibling with decomposed arguments;
- `ARecord`/`ARecordGet` operations for the carrier disappear from the sibling;
- the sibling returns the scalar `count` directly;
- the original generic/persistent clone remains present.

- [ ] **Step 2: Run tests and verify RED**

Run `target/twk test --filter "decomposed aggregate ABI"`.

Expected: no `aggregate_upgrades` and the existing record-shaped clone remains.

- [ ] **Step 3: Generate a capped mutable sibling**

Reuse `variant_specialize.variant_cap` and the existing `$mv` sibling partitioning. Create a deterministic sibling name ending in `$mvagg`. Allocate fresh parameter locals for the projected vector and the single scalar field, copy their MonoTypes into `op_result_mono`, and set the sibling return type to the scalar result type.

- [ ] **Step 4: Rewrite the sibling body**

Replace carrier field projections with the corresponding decomposed parameter/current scalar local. Replace carrier reconstruction with local rebinding of those components. Rewrite recursive calls to pass the same handle plus the current scalar and consume the direct scalar result. Retarget vector reads/writes to family `mutvec_get/set/len` operations. Reject the whole upgrade if a recursive edge substitutes any other handle or if any record operation cannot be mapped exactly; never partially rewrite a clone.

- [ ] **Step 5: Rewrite accepted entry call sites**

Flatten the caller-born vector producer using the existing MutVec producer rewrite. Replace `permute(State.{ values, count }, n)` with the `$mvagg(values_handle, count, n)` call. Replace the result’s scalar projection with the direct call result. For `.DeadField`, emit no freeze and remove dead carrier construction through the ordinary dead-let pass.

- [ ] **Step 6: Unify bare-vector and aggregate route partitioning**

Build one per-route site partition before allocating any `$mv` or `$mvagg` sibling. A site may belong to at most one physical ABI class: persistent, bare MutVec, or aggregate MutVec. All-accepted sites of one class may upgrade the owned clone directly; mixed classes receive distinct capped siblings. Reserve all required siblings before rewriting, count them against the same per-generic `variant_cap`, and fall back every affected site if the complete partition cannot be allocated. Feed the final site-to-clone map to the existing all-members-of-a-region survival gate.

- [ ] **Step 7: Assign physical representations**

Teach `apply_mutvec_call_abi_upgrades` to mark only `vector_param` and matching call arguments as `ReprKind.MutVec(family)`, and set the clone's `phys_return` to the ordinary representation of `scalar_return`. Scalar parameters/results otherwise retain `repr_assign`'s normal representation. There must be no generic `ref.cast` between `PVec` and `MutVec`.

- [ ] **Step 8: Run focused ANF and verifier tests**

```bash
target/twk test --filter "decomposed aggregate ABI"
target/twk test --filter "mutvec call"
target/twk wat boot/tests/fixtures/cfg/mutvec_call/recursive_record_scratch.tw --func permute --calls
```

Expected: the private clone calls `mutvec_get_i64`/`mutvec_set_i64`, contains no persistent vector set, constructs no carrier records, and validates as Wasm-GC.

- [ ] **Step 9: Commit the dead-result rewrite**

```bash
git add boot/compiler/codegen/mutvec_call_phase.tw boot/compiler/backend/mutvec_repr.tw boot/compiler/codegen/codegen.tw boot/tests/suites/mutvec_call_suite.tw
git commit -m "feat(mutvec): decompose recursive aggregate ABI"
```

### Task 5: Materialize Observed Results and Partition Mixed Callers

**Files:**
- Modify: `boot/compiler/codegen/mutvec_call_phase.tw`
- Modify: `boot/compiler/backend/mutvec_repr.tw`
- Modify: `boot/tests/suites/mutvec_call_suite.tw`

**Interfaces:**
- Consumes: `.PublishRecord` and `.PublishField` aggregate upgrades from Tasks 2–4.
- Produces: one boundary adapter per observed exit; persistent callers continue to target the original clone.

- [ ] **Step 1: Add failing freeze/reconstruction tests**

Assert that `recursive_record_escape` emits exactly one `mutvec_freeze_i64`, reconstructs one ordinary `State` at the boundary, and contains no freeze in the recursive clone. Assert that `recursive_record_field_escape` also freezes exactly once but substitutes only the published vector projection. Add a mixed-caller test where one caller’s vector dies and another retains an alias; assert only the first routes to `$mvagg`.

Add a direct planning test with the generic function already at
`variant_specialize.variant_cap`; assert no `$mvagg` sibling or aggregate ABI
upgrade is produced and both callers remain on the persistent clone.

- [ ] **Step 2: Run tests and verify RED**

Run `target/twk test --filter "aggregate boundary"`.

Expected: dead-result path works from Task 4, but observed-result reconstruction is absent and/or the mixed caller is not partitioned.

- [ ] **Step 3: Implement boundary materialization**

For `.PublishField`, freeze the handle once and substitute the frozen vector at the observed projection. For `.PublishRecord`, freeze once and reconstruct the source record from the frozen vector plus the single scalar output. Insert adapters only at the caller exit named by the verified decision.

- [ ] **Step 4: Apply the unified partition to observed and mixed callers**

Use Task 4's unified partition for observed aggregate sites alongside existing bare-vector sites. A strict accepted subset gets `$mvagg`; rejected sites keep the original persistent clone. Confirm the all-members-of-a-region survival gate sees the final sibling-aware targets so no call crosses physical ABIs.

- [ ] **Step 5: Run all boundary and negative tests**

```bash
target/twk test --filter "aggregate boundary"
target/twk test --filter "aggregate carrier verifier"
target/twk test --filter "mutvec call"
```

Expected: zero freezes for dead results, one for observed results, persistent fallback for every negative fixture, and no verifier errors.

- [ ] **Step 6: Commit publication adapters**

```bash
git add boot/compiler/codegen/mutvec_call_phase.tw boot/compiler/backend/mutvec_repr.tw boot/tests/suites/mutvec_call_suite.tw
git commit -m "feat(mutvec): materialize recursive carrier boundaries"
```

### Task 6: Make Ordinary AWFY Permute the Performance Gate

**Files:**
- Modify: `examples/performance/awfy/README.md`
- Modify: `docs/plans/sound-uniqueness/README.md`
- Modify: `docs/plans/sound-uniqueness/storage/README.md`
- Modify: `docs/plans/mutvec-checklist.md`

**Interfaces:**
- Consumes: optimized ordinary `permute.tw`, retained `permute_mut.tw` oracle, LuaJIT row.
- Produces: reproducible correctness/codegen/performance evidence and updated roadmap status.

- [ ] **Step 1: Rebuild the self-hosted compiler and prove a fixed point**

```bash
make bundle-cli
shasum -a 256 target/boot.wasm
make stage2
shasum -a 256 target/boot.wasm
```

Expected: the two SHA-256 values match. Do not commit the ignored generated payload.

- [ ] **Step 2: Run correctness suites**

```bash
make boot-test
make rust-test
target/twk run examples/performance/awfy/twinkle/permute_mut_test.tw
```

Expected: all pass.

- [ ] **Step 3: Inspect ordinary Permute WAT and census**

```bash
target/twk wat examples/performance/awfy/twinkle/main.tw --func permute --calls
target/twk ir examples/performance/awfy/twinkle/main.tw --census --sites | rg -C 4 "permute|mutvec aggregate"
```

Acceptance:

- ordinary Permute’s selected `$mvagg` clone calls `mutvec_get_i64`/`mutvec_set_i64`;
- it contains no `rt_arr__set`, `set_at__Int`, or carrier `struct.new` in the recursive hot body;
- it emits zero `mutvec_freeze` calls because only `count` is observed;
- the census prints the ownership proof, recursive routes, dead exit, and consumed rewrite.

- [ ] **Step 4: Run three same-session AWFY samples**

```bash
examples/performance/awfy/run.sh
examples/performance/awfy/run.sh
examples/performance/awfy/run.sh
```

Record medians for `twinkle permute`, `twinkle permute_mut`, and `luajit permute`. Do not mix results from different compiler builds.

- [ ] **Step 5: Apply the performance gate**

The ordinary `permute` median must:

- be no slower than `1.25 × permute_mut` median; and
- be no slower than `1.50 × LuaJIT permute` median.

If correctness and WAT shape pass but either performance condition fails, do not weaken the gate or require users to write `permute_mut`. Profile the residual ordinary-vs-oracle delta, add a separate follow-up plan for the identified codegen cost, and leave this plan incomplete.

- [ ] **Step 6: Update documentation with measured results**

Mark the recursive aggregate S4 slice landed only after the gate passes. Explain that normal immutable source now receives the optimization automatically; keep `permute_mut` documented as an oracle, not recommended source style.

- [ ] **Step 7: Commit verification and documentation**

```bash
git add examples/performance/awfy/README.md docs/plans/sound-uniqueness/README.md docs/plans/sound-uniqueness/storage/README.md docs/plans/mutvec-checklist.md
git commit -m "docs(mutvec): record recursive aggregate performance gate"
```

### Task 7: Final Regression and Branch Review

**Files:**
- Review all files changed by Tasks 1–6.

**Interfaces:**
- Consumes: complete aggregate S4 implementation.
- Produces: merge-ready branch with verified persistent fallbacks and ordinary-source performance.

- [ ] **Step 1: Run the complete project verification**

```bash
make test
```

Expected: Rust, boot, and JavaScript runtime suites pass.

- [ ] **Step 2: Re-run formatting and lint on reachable Twinkle sources**

```bash
target/twk fmt
target/twk lint boot/main.tw
target/twk lint examples/performance/awfy/twinkle/main.tw
```

Expected: formatting is idempotent. Investigate new lint findings; do not mechanically apply `.inc()`/`.dec()` in the benchmark hot recursion until those methods inline equivalently.

- [ ] **Step 3: Audit fallback WAT**

Compile the alias, mixed-lineage, unsupported-family, and variant-cap fixtures. Confirm each uses persistent vector operations and contains no MutVec/PVec cast bridge.

- [ ] **Step 4: Review the final diff against the spec**

Confirm normal `permute.tw` is unchanged in algorithm and data model, no public API was added, dead results freeze zero times, observed results freeze once, and every uncertain path falls back.

- [ ] **Step 5: Commit any review-only corrections**

```bash
git add boot/compiler/codegen boot/compiler/backend boot/tests/fixtures/cfg/mutvec_call boot/tests/suites/mutvec_call_suite.tw docs/plans examples/performance/awfy
git commit -m "fix(mutvec): close recursive aggregate review findings"
```

Skip this commit when review finds no changes.
