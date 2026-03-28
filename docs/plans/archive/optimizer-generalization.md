# Optimizer Generalization Plan

## Goal

Evolve the boot optimizer from a small set of effective but shape-sensitive passes
into a more generally useful optimization layer that can improve:

- compiler-lowered persistent collection operations
- library-authored persistent collection implementations written in Twinkle
- future backend/runtime work that wants semantic optimization without depending on
  exact source or ANF let-chain shape

The intent is not to throw away the current passes. The intent is to keep the
current ANF pipeline and pragmatic wins, while moving optimization legality and
rewrite power onto shared semantic contracts.

## Why This Plan Exists

Today the optimizer is not fundamentally broken, but it is narrow:

- peephole passes are mostly syntax-driven
- semantic knowledge is split across pass-local logic and ad hoc config
- collection optimization depends heavily on exact lowering shape
- builder/in-place rewrites are useful, but encoded as special cases rather than
  first-class optimization concepts

That is good enough for the current compiler. It is not a strong long-term base for
Twinkle-authored persistent collection libraries or broader backend work.

## Current State

### Pipeline shape

`boot/compiler/opt/pipeline.tw` runs:

1. fixed-point peepholes:
   - `dead_let`
   - `copy_prop`
   - `const_fold`
   - `branch_simp`
2. post-loop passes:
   - `liveness` record-update annotation
   - `uniqueness` COW-to-in-place rewrite
   - `defer_elim`

This is a tree-walking ANF optimizer with no CFG.

### What already works well

- `use_count.tw` provides a simple purity/use-count base for dead let elimination
  and copy propagation.
- `liveness.tw` computes a conservative backward live set and annotates
  `ARecordUpdate` with in-place reuse when the base is dead.
- `uniqueness.tw` already proves enough single-ownership to:
  - rewrite COW collection updates to in-place variants
  - detect a loop accumulator pattern
  - rewrite that pattern to vector builder ops
- `defer_elim.tw` is structurally separate and correctly stays last.
- `collect` already lowers directly through `vector_builder_new` /
  `vector_builder_push` / `vector_builder_freeze` in `lower_core.tw`. The
  remaining optimizer gap is not "make collect use builders", but "give
  non-collect library/authored shapes an equally canonical transient target".
- `anf_analysis.tw` already centralizes some ANF tree utilities
  (free/bound/assigned locals, pattern bindings, divergence). The remaining
  gap is optimizer-specific effect/alias/operand analysis, not analysis sharing
  from absolute zero.

### Where semantic knowledge lives today

Optimization legality is currently spread across several unrelated mechanisms:

- hardcoded `AnfOp` pattern checks
- `CowConfig` in `uniqueness.tw`
- prelude-specific `make_prelude_cow_config` wiring in `pipeline.tw`
- builtins/lowering choices upstream
- conservative container/call taint rules in `uniqueness.tw`

The optimizer does have semantic knowledge today, but it is encoded indirectly.

## Main Limitations

### 1. Call semantics are name/config based, not IR-native

Collection reasoning depends on matching `ACall(.AGlobalFunc(func_id), args)` and then
consulting side tables such as:

- `cow_ops`
- `fresh_producer_ids`
- `read_only_ids`
- builder func ids

That works for a small prelude surface, but it does not scale well to:

- more collection helpers
- library-level persistent structures
- backend-specific helper families
- future optimizer consumers beyond collections

### 2. Optimization depends too much on exact shape

The current uniqueness and loop-region rewrites expect very particular patterns:

- consume-reassign shape after a COW call
- exact `push(base, elem)` plus `assign(base, result)` loops
- base local must appear only in allowed positions

Semantically equivalent library code that lowers differently may miss the
optimization entirely.

### 3. Analyses are duplicated and pass-local

Several passes re-walk ANF with their own local logic:

- use counting
- free/bound/assigned local collection
- liveness
- taint/escape detection
- local-use scanning inside loop rewrite

This makes the optimizer harder to extend consistently.

### 4. There is no first-class optimizer-level transient region concept

The compiler already has an operational vector-builder family and lowers
`collect` through it, but the optimizer still knows about that transient shape
only through specific builtin/rewrite logic. There is no explicit optimizer
concept for:

- begin mutable transient
- apply local updates
- freeze back to persistent value

Without that, the compiler must rediscover transient opportunities by pattern
matching immutable-looking code.

### 5. Record reuse is special-cased, not generalized

`ARecordUpdate` has a dedicated `can_reuse_in_place` bit. Collection updates use a
different mechanism. Both are really instances of the same problem:

- a persistent update on a provably unique base may reuse storage

Today that idea is split across separate representations.

## Non-Goals

- Replacing ANF with SSA immediately
- Building a full CFG optimizer before improving current passes
- Eliminating all pattern matching; some local peepholes will remain useful
- Solving every alias/effect problem in one step
- Blocking current optimizer work until the generalized design is complete

## Target State

Keep ANF and the current pipeline shape, but make optimization operate on:

- shared operation/effect contracts
- shared analysis helpers
- canonical collection/transient forms

The long-term optimizer should answer questions like:

- is this op pure?
- does it allocate a fresh result?
- does the result alias an argument?
- which arguments are read-only, consumed, or escape?
- is there an in-place equivalent?
- is this region a transient builder/update/freeze region?

Those answers should come from one optimizer-facing contract layer, not from
independent pass-specific heuristics.

## Proposed Architecture

### Layer A: Shared optimizer semantics

Introduce a new optimizer-facing semantics module, for example:

- `boot/compiler/opt/semantics.tw`

This should classify both builtin calls and structured ANF ops using records such as:

```tw
pub type EffectKind = { Pure, ReadOnly, Update, Allocate, Control }

pub type AliasSummary = .{
  fresh_result: Bool,
  result_aliases_args: Dict<Int, Bool>,
  consumes_args: Dict<Int, Bool>,
  escapes_args: Dict<Int, Bool>,
}

pub type RewriteSummary = .{
  in_place_equivalent: FuncId?,
  transient_role: TransientRole?,
}

pub type OpSemantics = .{
  effect: EffectKind,
  alias: AliasSummary,
  rewrite: RewriteSummary,
}
```

The exact record shape can change, but the optimizer should be able to ask semantic
questions without embedding builtin names in every pass.

### Layer B: Canonical call forms

Short term:

- keep `ACall(.AGlobalFunc(...), args)`
- teach passes to consult the shared semantics layer instead of pass-local tables

Mid term:

- add a more explicit ANF form for optimizer-visible builtins or collection ops, such as:
  - `ABuiltinCall(FuncId, Vector<Atom>)`
  - or dedicated collection/transient ops

This reduces repeated pattern matching on `ACall(.AGlobalFunc(...), ...)`.

### Layer C: Shared analyses

Consolidate reusable analysis utilities under `boot/compiler/opt/analysis/` or expand
`anf_analysis.tw` with optimizer-oriented helpers:

- use counts
- assigned locals
- live-in/live-out maps
- free-local and escape summaries
- alias transfer helpers
- per-op operand walkers

The goal is to stop rewriting "walk every op and inspect locals" independently in
each pass.

### Layer D: Explicit transient/builder regions

Introduce first-class transient concepts to ANF and the optimizer:

- transient/builder begin
- transient update
- transient freeze

This can start as a semantic layer over existing builder functions, then become
explicit IR later.

That gives the optimizer a general model for:

- `collect`
- loop accumulators
- bulk map/filter/build operations
- future dict builders

Instead of reconstructing a transient region from repeated immutable updates.

### Layer E: Unified "reusable update" concept

Generalize the current record and collection reuse logic so that:

- record update reuse
- vector in-place update
- dict in-place update

are all instances of the same legality check:

- base is unique
- base does not escape
- result does not need the old version afterward
- backend/runtime supplies a reuse-capable implementation

## Staged Plan

### Stage 1: Centralize semantics without changing IR shape

- Add optimizer semantics records for current builtins and relevant ANF ops.
- Replace `CowConfig` with a more general semantics/config layer, or make `CowConfig`
  a subset of it.
- Derive builtin-facing semantics from `BuiltinRegistry` / canonical builtin
  metadata instead of creating a third independent builtin source of truth.
- Update `use_count`, `liveness`, and `uniqueness` to consult the shared semantics
  layer where possible.
- Keep current behavior unchanged.

Success criteria:

- no pass needs to hardcode collection builtin roles independently
- `make_prelude_cow_config` becomes a general prelude optimizer semantics builder

Recommended kickoff slice:

- add `boot/compiler/opt/semantics.tw` with a deliberately small API:
  - effect classification for current ANF ops
  - builtin semantics lookup by `FuncId`
  - helpers for "fresh result", "read-only call", and "COW/in-place equivalent"
- make `pipeline.tw` build optimizer semantics from `BuiltinRegistry`
- migrate `dead_let` / `use_count.is_pure` first, then `uniqueness`
- add parity tests that assert semantics-backed behavior matches current
  `CowConfig` and `is_pure` behavior on existing optimizer fixtures

### Stage 2: Consolidate analysis helpers

- Move repeated operand/local scanning into shared helpers.
- Add reusable live-in/live-out support rather than recomputing `live_after(body)`
  in many places.
- Introduce an escape/alias helper layer used by uniqueness and future passes.

Current status:

- done: shared optimizer helpers in `boot/compiler/opt/analysis.tw` for
  local-operand scanning, global call-site counting, next-local allocation,
  taint/escape pre-scan, loop push-site legality, and continuation-side
  base-reuse legality
- done: loop-region builder rewrite construction moved out of `uniqueness.tw`
  into `boot/compiler/opt/loop_builder.tw`
- remaining: decide whether any additional pass-local legality/state helpers in
  `uniqueness.tw` are stable enough to share before starting Stage 3

Success criteria:

- fewer pass-local tree walkers
- easier to add new rewrite legality checks consistently

### Stage 3: Generalize uniqueness into ownership/effect rewrite

- Recast `uniqueness.tw` around semantic properties:
  - fresh result
  - read-only arg
  - consumed base
  - in-place equivalent
  - escape behavior
- Keep the current tainted/unique model initially, but derive legality from shared
  semantics instead of builtin-name categories.

Success criteria:

- collection rewrites no longer depend on prelude-specific logic scattered across passes
- new persistent helpers can become optimizable by registering semantics, not by adding
  custom pass logic

### Stage 4: Introduce canonical transient regions

- Model existing vector builder flow as the first transient region.
- Lower `collect` directly into canonical transient operations or a canonical builder
  op family recognized by the optimizer.
- Extend the model to dict builders if needed.

Current status:

- done: `boot/compiler/opt/builder_region.tw` now defines a small
  optimizer-facing `BuilderRegion` plus lowering back to the existing
  `vector_builder_*` runtime calls
- done: `boot/compiler/opt/loop_builder.tw` now emits that canonical region
  shape instead of assembling the final nested builder-call ANF directly
- done: `boot/compiler/builder_family.tw` now provides a shared builder-family
  config derived from `BuiltinRegistry`, used by both optimizer semantics and
  front-end collect lowering
- done: `boot/compiler/lower_core.tw` now shares one collect-builder lowering
  helper for both iterator and condition-form `collect`, so front-end collect
  lowering also goes through a single canonical builder-call scaffold
- decision: treat this as sufficient Stage 4 completion for now
- deferred: do not add explicit transient IR forms unless a concrete optimization
  or a second transient family shows that the shared builder-family boundary is
  no longer enough

Success criteria:

- loop-region rewrite becomes a transform into a transient region, not a vector-only
  ad hoc pattern
- Twinkle-authored bulk collection code has a clearer optimization target

### Stage 5: Optional IR refinement

If Stage 1-4 still leave too much syntactic matching:

- add optimizer-facing builtin/transient op variants to ANF
- keep codegen lowering straightforward by preserving a simple mapping back to runtime
  helpers

This stage is optional and should only happen if the semantics layer alone is not
enough.

Current decision:

- skipped for now
- revisit only if the current builder-family abstraction causes repeated awkward
  pattern matching, blocks a desired optimization, or proves too narrow for
  another transient family such as dict builders

## Impact on Current Passes

### `use_count.tw`

Keep it small, but stop making purity the only semantic predicate it exposes.
Purity/effect summaries should move to the shared semantics layer.

### `dead_let.tw`

Can remain mostly unchanged, but should eventually use a shared effect predicate rather
than `use_count.is_pure`.

### `copy_prop.tw`

Should continue to be a local peephole pass. It does not need deep semantic analysis,
but it should benefit from shared operand walkers and clearer atom duplication rules.

### `const_fold.tw`

Can remain mostly as-is. This is a good example of a pass that is naturally
pattern-driven and does not need major architectural change.

### `branch_simp.tw`

Can remain mostly as-is. It should continue to be a local canonicalization pass.

### `liveness.tw`

Should evolve from a single-purpose helper into part of a broader shared analysis
foundation. The current record-specific annotation should become one client of a more
general reuse legality framework.

### `uniqueness.tw`

This is the main beneficiary of the plan. It should become less prelude-specific and
less tied to exact call forms. Stage 2 has already started shrinking it by moving
shared legality analysis into `analysis.tw` and loop-region rewrite construction
into `loop_builder.tw`.

### `defer_elim.tw`

Should stay structurally separate and late. It is not the main target of this plan.
The main improvement here is to ensure earlier passes expose cleaner shared analysis
helpers rather than to redesign defer elimination itself.

## Library Authoring Implications

If this plan succeeds, Twinkle-authored persistent collection libraries should be able
to get low-level optimization by:

- using canonical persistent update/transient primitives
- registering optimizer semantics for those primitives
- relying less on exact source-code shape

That is the key improvement over the current design.

The optimizer still will not optimize arbitrary clever immutable code magically.
But it should stop requiring one-off pass logic for every new collection helper family.

## Testing Strategy

Testing should be staged the same way as the implementation. The goal is to catch:

- semantic regressions in existing optimizations
- mismatches between shared optimizer contracts and actual rewrites
- missed optimization opportunities caused by harmless ANF shape changes
- backend/codegen drift once more optimizer-visible semantics move into shared layers

### 1. Pass-Level Unit Tests

Each pass should keep focused tests over small ANF inputs:

- `dead_let`
  - removes unused pure lets
  - preserves effectful lets
  - preserves pinned locals
- `copy_prop`
  - propagates literals/globals safely
  - preserves semantics around reassigned locals
  - stops correctly at shadowing boundaries
- `const_fold`
  - folds supported ops
  - preserves divide/mod-by-zero traps
- `branch_simp`
  - splices literal branches correctly
  - preserves diverging branch behavior
- `liveness`
  - computes stable liveness for straight-line, branch, and loop forms
  - annotates record reuse only when the base is dead
- `uniqueness`
  - rewrites only when ownership proof holds
  - refuses rewrite when values escape, alias, or remain live
- `defer_elim`
  - preserves LIFO behavior
  - preserves capture-by-value semantics

### 2. Shared-Semantics Contract Tests

Once optimizer semantics are centralized, add tests that validate the contract layer
itself independently of any one pass.

Examples:

- a helper marked `Pure` is removable when unused
- a helper marked `ReadOnly` does not taint uniqueness
- a helper marked `fresh_result` produces a unique result candidate
- a helper with `in_place_equivalent` rewrites only when ownership conditions hold
- a helper marked as transient begin/update/freeze is recognized consistently across
  all optimizer clients

These tests should fail if semantics metadata and pass behavior drift apart.

### 3. Cross-Pass Integration Tests

Add integration tests for the full optimization pipeline rather than isolated passes.

Important cases:

- fixed-point cleanup enables later uniqueness rewrite
- copy propagation and dead-let elimination do not destroy ownership information
  needed by later passes
- liveness/reuse annotations remain valid after earlier simplifications
- defer elimination still works after all earlier rewrites

These tests should run against optimized ANF snapshots, not just final runtime output.

### 4. Library-Shape Regression Tests

The main risk in this area is "same semantics, slightly different shape, optimization
silently disappears". Add tests that encode families of equivalent source/library
patterns and require comparable optimization outcomes.

Examples:

- vector update written through direct consume-reassign vs an equivalent helper layer
- loop accumulator expressed with small refactors that should still reach builder form
- persistent dict update helper expressed in a few equivalent ANF shapes
- record/collection update code that should continue to reuse in place after benign
  refactors

These are the tests most likely to make the optimizer generally useful instead of
merely clever on one source shape.

### 5. Runtime/Backend Characterization Tests

When optimizer contracts drive in-place or transient rewrites, keep backend-facing
tests that confirm the optimized form still maps to the intended runtime behavior.

Examples:

- optimized vector builder flow still lowers to builder runtime helpers
- optimized in-place collection rewrites still preserve persistent semantics
- record reuse still emits the intended in-place backend path
- future transient ops still map cleanly to runtime helpers or backend intrinsics

### 6. Snapshot Strategy

Use two complementary snapshot layers:

- ANF optimizer snapshots
  - best for verifying whether a rewrite happened
  - should cover ownership, builder, and reuse transformations
- final WAT/backend snapshots
  - best for verifying that optimizer-visible annotations still affect codegen

Do not rely only on final runtime behavior, because many optimization regressions
preserve correctness while silently losing the intended rewrite.

### 7. Negative Tests

Add tests that prove the optimizer refuses illegal rewrites.

Critical negative cases:

- value captured by closure
- value stored into another container
- aliased source still live after update
- read after update when old version must remain observable
- helper marked with the wrong semantics and rejected by validation checks

These tests are as important as the positive ones, because the entire point of the
generalized optimizer is to widen applicability without weakening correctness.

### 8. Migration-Gated Tests

For each stage in this plan, add tests before switching existing passes over:

- Stage 1: contract-layer parity with current `CowConfig` behavior
- Stage 2: shared-analysis parity with current liveness/taint behavior, plus
  direct tests for extracted loop-region helpers
- Stage 3: uniqueness rewrite parity on existing vector/dict/record tests
- Stage 4: transient-region parity with current builder rewrite behavior
- Stage 5: ANF-form parity if explicit builtin/transient ops are introduced

This keeps the refactor incremental and prevents "big bang" optimizer regressions.

## Validation

### Keep current behavior green

- existing `tests/opt/*`
- vector and dict runtime characterization tests
- defer tests

### Add new optimizer-contract tests

- a helper registered as read-only should not taint uniqueness
- a helper registered as fresh should produce a unique result
- a helper with an in-place equivalent should rewrite based on ownership proof, not
  based on ad hoc name checks
- transient region rewrites should verify site counts and semantic equivalence

### Add library-shape tests

- persistent vector helper code written in Twinkle still optimizes when expressed
  through canonical transient/update forms
- semantically equivalent helper refactors do not accidentally lose optimization just
  because the ANF shape changed slightly

## Risks

- Over-designing a generic effect system before current needs are clear
- Making the optimizer harder to debug if semantics become too implicit
- Splitting semantics between lowering and optimizer in inconsistent ways
- Adding explicit transient ops too early, before the minimal shared semantics layer
  proves useful
- Trying to solve CFG-grade problems in a tree-ANF pipeline without clear payoff

## Recommended Order

1. Shared semantics layer
2. Shared analysis helpers
3. Uniqueness rewrite migration
4. Canonical transient region support
5. Optional ANF refinement

That order improves generality quickly without forcing an immediate IR rewrite.
