# Sound Uniqueness and Mutable Lowering Plan

**Status:** Draft

## Goal

Rebuild Twinkle's boot-compiler uniqueness optimization from scratch as a
sound, proof-driven compiler analysis that lowers ordinary immutable `Vector`,
`Dict`, and record-update code to compiler-private mutable representations when
mutation is observationally safe.

The visible language model stays immutable. `Vector.set_at`, index rebinding,
`Dict.set`, `Dict.remove`, record field rebinding, record updates, and loop
accumulator code should remain the source-level style users write. The compiler
should recover the mutation-like performance currently demonstrated only by
explicit workaround APIs such as `@std.buffer`.

This effort targets the self-hosted boot compiler in `boot/`. Rust stage0 in
`src/` can stay as it is for now; update it only if the boot compiler cannot
bootstrap or if a small reference change is required to unblock self-hosting.

## North-star performance target

The end goal is for the ordinary AWFY benchmark variants to run in the same
performance class as today's manual mutable variants:

| Ordinary source benchmark | Current workaround ceiling |
|---|---|
| `sieve` | `sieve_mut` |
| `bounce` | `bounce_mut` |
| `nbody` | `nbody_mut`, for the portion attributable to storage mutation |

When ordinary `Vector`/`Dict`/record code reliably reaches those internal mutable
paths, `Buffer` and the `*_mut` benchmark variants should be treated as removable
scaffolding rather than permanent user-facing performance APIs. Dict-backed
wrappers such as `Set<K>` should benefit from this through record-field ownership
and wrapper-aware lowering rather than needing a wholly separate optimization
family.

## Current context

The current branch intentionally removed the previous boot uniqueness,
liveness, builder-region, and escape-style optimization passes. The replacement
work should happen in the boot compiler first; stage0 should not be treated as a
parallel implementation target.

During the refactor, timing numbers are expected to be noisy and often
misleading: the work may add analysis passes, change codegen shape, and emit more
runtime support before the optimized artifacts recover that cost. Intermediate
milestones should be judged by correctness, sound printed facts, and emitted
helper/builder shape, not by compiler-throughput or AWFY timing claims. Run full
performance comparisons only when the end-to-end path is in place and the branch
is close to merging.

Existing implementation pieces still matter as historical scaffolding:

- runtime/codegen hooks for vector builders and in-place vector/dict helpers;
- persistent PVec and HAMT runtime representations;
- optimizer semantics metadata for pure/read/update/allocation behavior;
- archived plans and probes documenting earlier soundness failures.

The new work should not revive the old recognizer-heavy pass wholesale. It
should preserve what was learned, but rebuild the proof model first.

Concretely, this project is complementary to ANF, Wasm codegen, runtime helpers,
persistent PVec/HAMT representations, existing builder primitives, and
`ARecordUpdate.in_place` as a codegen slot. It intentionally replaces any
independent in-place decision path: old uniqueness/liveness/escape/builder-region
passes, recognizer-heavy rewrites, or future ad hoc passes must not independently
decide mutability. The risk to avoid is split-brain mutability decisions;
ownership legality should come from the new CFG/proof layer, while existing hooks
remain lowering mechanisms.

## Design principles

### Immutable surface, private mutation

Mutation is an implementation strategy, not a language feature. The compiler may
lower a proven-owned `Vector` or `Dict` region to mutable storage internally, but
no mutable vector or mutable HAMT type should leak into user code.

### Soundness before coverage

The optimizer must behave like a conservative type/effect system:

- if ownership is proven, destructive lowering is allowed;
- if an old version is proven observable, persistent semantics must be
  preserved;
- if the proof is unknown or incomplete, the compiler must not rewrite to
  mutation.

There should be no speculative "may be shared" runtime guess. Unknown means
fallback to the persistent path.

### Determinism for self-host stability

Analysis, specialization, and code generation must be deterministic and
order-independent. Demand-driven ownership variants are allowed, but their
creation order, names/ids, and emitted module order must not depend on hash-map
iteration or incidental traversal order. Otherwise the self-host fixed point can
wobble even when semantics are correct.

### Deep ownership, not shell freshness

A fresh wrapper record is not the same thing as ownership of the collection
storage reachable through it. Collection mutation requires proof that the
mutated backing storage is owned. Record-shell reuse, if kept, has a separate
proof obligation from vector/dict backing mutation.

### Explicit publication boundaries

Mutable internal values become ordinary persistent values only at explicit
publication points: return, value-carrying `break`, `try`/early-return exits,
storage in an escaping aggregate, storage into a `Cell<T>` (the explicit mutable
escape hatch), closure capture, module/global publication, unknown call
boundaries, task/fiber spawn captures, channel sends, or other places where the
value may be observed outside the proven region.

`Cell<T>` is Twinkle's one genuine mutable box (`Cell.set` overwrites in place), so
storing a value into a Cell publishes it and a value read via `Cell.get` is not
owned. Cell is never itself a mutable-lowering target — it is already mutable; the
analysis only models its effects so the immutable-value reasoning stays sound
around it.

Return, value-carrying `break`, and `try` are all multi-exit publication edges
derived structurally from ANF: `break value` terminates a loop, and `try`
lowers to an `AMatch` whose error arm ends in `Return`. An owned handle that is
live across such an exit publishes the value on that edge; the fallthrough path
keeps the region alive. See [worked-examples.md](analysis/worked-examples.md) (Case T) for
the lowered shape.

Concurrency sinks must be explicit in the analysis. Spawning a `Task` or fiber
that captures an owned collection or record is a cross-fiber alias. Sending a
collection or record through a `Channel<T>` publishes it to another execution
context. Those cases must end the mutable region unless a future analysis proves
the value is copied or otherwise unobservable by the sender. Cross-worker sends
are different when they serialize/copy values rather than sharing Wasm-GC
references; the analysis should model that as copy/publication semantics rather
than as shared mutable aliasing. See [concurrency-publication.md](analysis/concurrency-publication.md)
for the focused subplan.

### Transient APIs are transition scaffolding

Current vector builder and dict/vector in-place APIs are the **first** codegen
targets. The first implementation should only decide between existing lowering
strategies: persistent operation, existing in-place helper, or existing builder
lowering. That keeps the ownership engine separate from runtime representation
work and lets us validate facts against today's backend hooks.

Long term, source-level or prelude-level transient APIs should not be required for
performance. Existing ad hoc transient APIs can later migrate behind a shared
internal mutable collection abstraction. That migration path is not part of the
first ownership-analysis milestone.

### Mutable collections are compiler intrinsics — later

The end shape may include a small, general compiler-private intrinsic family for
mutable collection operations. These intrinsics are not prelude APIs and not
user-callable escape hatches; they are the long-term optimizer/codegen contract
for proven mutable regions.

Do **not** make this the first implementation. The initial ownership engine should
feed the existing in-place and builder hooks. A separate later milestone can
replace those hooks with a cleaner intrinsic family once the facts are trusted.

## Proposed architecture

### Two-phase development model

Split the work into two main phases:

1. **IR foundation and analysis first.** Add a deterministic CFG ownership view
   over authoritative ANF, with SSA-style block parameters for carried values,
   migrate optimizer analysis/passes to consume that shared control-flow view,
   and make ownership facts visible in compiler IR output. This phase must not
   change generated code. The purpose is to let us manually inspect programs,
   decide whether mutation should be legal, and verify that the compiler's facts
   match that reasoning.
2. **Codegen second.** Once the printed facts are trustworthy, teach the
   optimizer/backend to use them to select between existing persistent,
   in-place, and builder lowering paths. Later milestones may replace those
   backend hooks with a shared mutable-intrinsic family. Codegen should consume
   the analysis results rather than rediscover ownership with separate
   recognizers.

This split is important for debugging. We should be able to look at ordinary
AWFY `sieve`, `bounce`, `nbody`, and compiler dict/vector/record-heavy workloads
in `twk ir` and see why a value is considered owned, borrowed, published,
rejected, or ready for a mutable region before any rewrite is emitted.

The same split applies interprocedurally, but interprocedural precision should be
staged. The first implementation can use minimal summaries and no ownership
specialization. Before generating specialized mutable function variants, the
compiler should first print the inferred ownership preconditions, postconditions,
and call-site compatibility decisions so we can verify the specialization story
manually.

### Ownership facts are the primitive

The ownership engine should infer facts only. In-place record updates, dict/vector
updates, builder lowering, transport-wrapper recovery, and future mutable-region
intrinsics are consumers of those facts; they must not embed independent ownership
logic.

The first ownership domain should be deliberately small:

- **`Unique`** — the value has a static uniqueness proof at this program point;
- **`Shared`** — the value is known observable through another live reference or
  publication boundary;
- **`Unknown`** — no proof either way, so consumers must choose the persistent
  path.

Binding validity is separate from ownership. A binding may become unavailable
because its value was moved/consumed, but `Moved` is not an ownership fact about
the value. Liveness, last-use, and binding-validity data are separate CFG facts
that ownership consumers consult alongside `Unique`/`Shared`/`Unknown`.

Recursive ownership shapes, field-path facts, publication labels, and
persistent-vs-mutable region labels are later precision layers. They are useful
for the full record/threading story, but the first implementation should not need
them to replace the old uniqueness/liveness pass and drive the existing local
rewrites.

### Borrow/read facts

Reads from a uniquely owned collection should not automatically defeat mutation.
AWFY `sieve` and `bounce` both interleave reads and updates of the same logical
collection. The analysis should distinguish non-escaping reads from publication
or aliasing sinks.

A local read such as `xs[i]` can be a temporary borrow inside the region. A read
that stores `xs`, captures `xs`, returns `xs`, passes `xs` to an unknown callee,
or publishes an older version is a different event and must constrain or end the
mutable region.

### Loop-carried ownership across back-edges

Loop-carried ownership is a named hard case, not just another join. The headline
patterns all thread an owned handle through a back-edge while interleaving reads
and writes:

- `flags = flags.set_at(k, false)` in `sieve`;
- `balls = balls.set_at(j, updated_ball)` in `bounce`;
- vector append/build accumulators;
- dict/env accumulator updates.

The analysis must model loop entry facts, facts after each iteration, and the
back-edge merge explicitly. A value may remain owned across the loop only if the
body's reads are non-escaping borrows and every path through the body preserves
the ownership invariant for the next iteration. Any path that publishes the
current or previous value, stores it into an escaping aggregate, captures it in
an escaping closure/task, sends it through a channel, or aliases it beyond the
region must break the loop-carried mutable region.

The IR/debug output should show loop-carried facts separately from ordinary
branch facts: loop candidate, carried locals, borrow sites, update sites,
back-edge merge result, and the reason a loop was accepted or rejected.

### Closure captures: blocker first, recoverable later

Closure capture is a publication sink by default. If a closure captures an owned
collection or record, the mutable region must end unless the compiler proves the
closure is non-escaping and called in a scope where the borrow remains temporary.

That leaves a future coverage path: non-escaping closures, inlined closures, or
closure summaries may eventually recover ownership precision. The first sound
analysis should still treat unknown or escaping closure captures as hard
blockers, and the IR should distinguish "blocked by escaping capture" from
"temporary non-escaping closure borrow" if the latter is introduced. See
[closure-capture.md](analysis/closure-capture.md) for the focused subplan.

### Interprocedural uniqueness specialization — later

Some profitable cases require caller-dependent code generation, but this should
come after the core CFG ownership engine and minimal summaries are stable. The
same source function may eventually need both:

- a normal immutable/persistent variant for callers that pass shared or unknown
  values;
- a mutable-specialized variant for callers that pass proven-owned values and do
  not need older versions to remain observable.

This is analogous to monomorphization, but keyed by ownership/uniqueness facts
rather than type arguments. The caller should statically select the variant; this
should not introduce runtime uniqueness tests or dynamic dispatch.

The analysis should therefore produce function summaries and call-site decisions,
for example:

- parameter ownership requirements for a mutable-specialized variant;
- whether a parameter is consumed, borrowed, published, or returned;
- whether the return value is owned, persistent, or published;
- which call sites can use a mutable-specialized callee;
- which call sites must remain on the generic immutable callee.

The IR needs a way to represent these variants without abandoning ANF as the
structured backend spine. The preferred shape is cloned/specialized ANF functions
plus CFG-derived summaries and ANF-keyed annotations/side tables. The important
requirement is that the artifact can contain both variants of the same logical
function, with names/ids and printed IR output that make the specialization key
visible.

Concrete example:

```tw
fn clear_at(flags: Vector<Bool>, i: Int) Vector<Bool> {
  flags.set_at(i, false)
}

fn owned_case(n: Int, i: Int) Vector<Bool> {
  flags := Vector.make(n, true)
  clear_at(flags, i)
}

fn shared_case(n: Int, i: Int) Bool {
  flags := Vector.make(n, true)
  old := flags
  updated := clear_at(flags, i)
  old[i] and !updated[i]
}
```

`owned_case` can eventually call a mutable-specialized `clear_at` because `flags`
is freshly created and the old version is not observed after the call. That
variant can use the existing in-place helper first; a later mutable-region model
could begin a mutable vector region, write the slot, and freeze/publish the
result.

`shared_case` must call the ordinary persistent variant because `old` observes
the pre-update value. If it reused the mutable variant, `old[i]` would see the
write and the program would be wrong.

A record/dict example has the same shape:

```tw
type Env = .{ types: Dict<String, Int>, values: Dict<String, Int> }

fn add_type(env: Env, name: String, id: Int) Env {
  env.types = env.types.set(name, id)
  env
}

fn build_env() Env {
  env := Env.{ types: Dict.new(), values: Dict.new() }
  env = add_type(env, "A", 1)
  env = add_type(env, "B", 2)
  env
}

fn branch_env(env: Env) Int {
  before := env
  after := add_type(env, "A", 1)

  if before.types.has("A") {
    0
  } else if after.types.has("A") {
    1
  } else {
    2
  }
}
```

`build_env` should be able to call an owned-specialized `add_type` that reuses
the record shell when safe and mutates the owned `types` HAMT field. `branch_env`
must use the persistent variant because `before` keeps the old environment
observable. This is why caller-dependent variants are not theoretical; they are
required for both performance and soundness.

The boot compiler also has a more common transport-wrapper form: helpers return
`.{ ..., ctx/state/env }` records (`SynthOut`, `CheckOut`, `ExprOut`, `LocalOut`,
`FuncIdOut`, ANF lowering state/accum results, resolver env results,
boundary-rewrite results, query-analysis state results, etc.), then the caller
immediately continues with `ctx = out.ctx`, `state = out.state`, or `env = out.env`
while reading sibling result fields. Covering the actual source therefore
requires **return-path ownership summaries** such as `returns[.ctx]` or
`returns[.state]` being `OwnedFromParam(0)`, plus a field-projection move when
that path is not observed through the wrapper afterward. Query analysis
also wraps these records in `Result`, so return paths need variant/payload
segments such as `Ok[0].state` and `Err[0].state`. Treating `.{ ctx, ... }`,
`.{ state, ... }`, or locally handled `Result` payloads as ordinary aggregate
publication would be sound but would miss dominant checker/lowering/resolver/query
threading idioms.

#### Controlling specialization explosion

Naive ownership specialization can generate too many variants. A function with
many reference-typed parameters — including vectors, dicts, and records with
reference-typed fields — could theoretically need every owned/shared combination,
especially if different callers provide different ownership shapes. That must be
handled explicitly; unbounded ownership-monomorphization is not an acceptable
default. This also composes with existing type-argument monomorphization: the
real code-size budget is type-specialized clones multiplied by ownership-shape
variants, not ownership variants in isolation.

Possible strategies:

1. **Demand-driven specialization with caps.** Generate variants only for
   call-site shapes that actually occur and only when the ownership fact changes
   codegen. Read-only reference parameters should not participate in the
   specialization key. Parameters that are never consumed, updated, or published
   in a way that affects mutable lowering should stay generic. Each function
   should have a variant cap and fall back to the ordinary persistent version
   when the cap is reached.
2. **Private tagged/boxed representation.** Use an internal representation such
   as `Persistent(PVec)` vs `Mutable(MutVec)`, `Persistent(HAMT)` vs
   `Mutable(MutHAMT)`, or record shells that carry private mutable-field handles
   for proven-owned fields, with generic functions dispatching on the tag where
   needed. This may simplify implementation and reduce code duplication, but the
   tag is only a representation tag. It must not be treated as proof that
   mutation is safe. Soundness still comes from static ownership facts, or else
   the runtime would need refcounts/borrow flags/COW checks, which this plan does
   not want.
3. **Hybrid.** Use static analysis to decide when a value may enter a private
   mutable region, generate specialized variants only for hot or consuming
   parameter/field shapes, and use private representation abstraction to keep
   codegen manageable. For example, if a function has many reference parameters
   but only mutates parameter 2, it should need at most a generic variant and an
   `owned_param_2` variant, not every combination of all parameters. If a record
   parameter has many fields but only one field is updated or projected into a
   mutable dict/vector region, the specialization key should mention that field's
   ownership rather than every field combination.

The likely default is the hybrid strategy: static proof remains the source of
truth; specialization is demand-driven and capped; representation tags may reduce
implementation friction but must not introduce runtime "maybe unique" behavior.
This applies to record shell reuse and field-sensitive ownership just as much as
to vector/dict backing storage. Any dispatch introduced by tagged
representations should happen outside hot inner loops when possible, because the
AWFY goal is to approach the manual `*_mut` performance class.

### CFG ownership view with SSA-style block parameters

The compiler does not currently have a reusable CFG or SSA IR. Add a derived CFG
ownership view early for this work: a deterministic CFG over authoritative ANF,
with SSA-style block parameters for values that cross branch joins or loop
back-edges.

ANF remains the program spine and backend-facing structured IR. The CFG view is
for analysis, printing, and decision production. Current optimizer
analysis/passes should migrate to consume this shared control-flow view or facts
derived from it, rather than each pass rediscovering liveness and joins by
walking the ANF tree independently.

The first version does not need full machine-oriented SSA for every value, but it
should provide the minimal facts needed to replace the old local uniqueness view:

- explicit basic blocks or equivalent join points;
- block parameters for carried/merged values;
- `Unique`/`Shared`/`Unknown` ownership facts as separate maps at block entry and
  exit;
- binding-validity, liveness, and last-use facts separate from ownership;
- loop-carried back-edge facts;
- value-carrying `break` edges and their ownership/publication effect;
- candidate update verdicts for existing persistent/in-place/builder hooks;
- ANF-keyed side-table decisions for accepted first-cut hook lowerings;
- printable analysis facts for each block and candidate update site.

Later versions add record shell and field-sensitive ownership facts, transport
return paths, call-site specialization records, mutable-region annotations, and
richer codegen-ready decisions with proof/debug ids.

Codegen should consume explicit ANF-keyed decisions from this view and stay
mechanical: it should not re-prove uniqueness, rediscover record-field ownership,
or repeat escape analysis. The plan does not aim to remove ANF, build a CFG-to-ANF
de-SSA/tree-reconstruction pass, or add a relooper for Wasm structured control
flow. See [cfg-ownership-ir.md](analysis/cfg-ownership-ir.md) for the focused subplan.

### Mutable collection intrinsic family — later

Define a backend-independent, compiler-private intrinsic family for mutable
collection regions after the first ownership engine is stable. The optimizer
should initially target existing runtime/codegen hooks; later lowering can map a
clean intrinsic family to Wasm-GC runtime helpers, direct Wasm operations, or
future backend representations.

The shared shape should include:

- begin/thaw from a proven-owned persistent value or known-empty collection;
- read through the mutable handle;
- write/update through the mutable handle;
- append/extend for vector-like collections;
- remove for map-like collections;
- freeze/publish back to the ordinary immutable value.

These intrinsics should carry enough type information for `Vector<T>`,
`Dict<K,V>`, and record shell lowering without becoming public signatures in
`@std` or the prelude.

### Mutable vector lowering

First, drive the existing builder and `set_in_place` hooks from ownership facts.
Later, add or formalize a compiler-private mutable vector target using the shared
mutable-collection intrinsic family, with operations such as:

- create from a fresh vector or known-empty vector;
- read element;
- write element;
- append/extend where applicable;
- freeze/publish to ordinary `Vector<T>`.

The first high-value patterns are:

- `flags = flags.set_at(k, false)` in loops;
- `balls = balls.set_at(j, updated_ball)` in loops;
- vector accumulator append/build loops;
- method-wrapper forms such as `xs.set_at(i, v)` where the wrapper is equivalent
  to the primitive indexed update.

### Mutable dict lowering

First, drive the existing dict in-place helpers from ownership facts. Later, add
or formalize a compiler-private mutable HAMT target for `Dict<K,V>` using the
same mutable-collection intrinsic family. The mutable dict path must preserve the
language-level behavior, including lookup semantics and insertion-order
iteration.

High-value patterns include threaded env, registry, seen-name, and map-building
flows in the compiler itself. Dict-backed wrappers such as `Set<K>` should ride
this path through record-field ownership and wrapper-aware lowering, since the
builtin `Set` representation is a thin record wrapper around `Dict<K, Void>`
(`entries`).

The dict path should not be a special-case sibling of vector lowering. Both
should share the same ownership, escape, publication, branch-join, and
loop-carried ownership framework, with collection-specific operation metadata
layered on top.

### Record shell reuse and field ownership

Records are the obvious adjacent case to collection uniqueness. A record update
has two separate optimization questions:

- can the record shell itself be updated/reused in place;
- can collection values stored in or projected from record fields be treated as
  deeply owned.

Those questions must stay separate. A fresh record shell around shared fields is
not proof that the reachable `Vector` or `Dict` storage is owned. Conversely, a
proven-owned record whose relevant fields are also proven owned should allow
field-sensitive propagation, so wrapper types such as `Set<K>` can inherit the
right dict ownership story without a bespoke Set optimizer.

The IR/debug output should make this visible: shell-owned records, deeply-owned
fields, field projections, and rejected record updates should all be explainable
before any codegen rewrite is enabled. Accepted record/field decisions should be
codegen-ready: codegen should see whether to reuse a record shell, project an
owned field, freeze/publish a field, or emit the persistent fallback without
making new soundness decisions.

### Nested collection ownership

Nested collections need the same shell-vs-deep split as records. A fresh outer
collection is not proof that inner collections are owned:

- `Vector<Vector<T>>` may have an owned outer vector whose elements point at
  shared inner vectors;
- `Dict<K, Vector<V>>` may have an owned HAMT whose values point at shared
  vectors;
- records, variants, and array literals can all wrap shared collection values in
  freshly allocated outer shells.

Mutating the outer structure only needs ownership of the outer backing. Mutating
an inner collection reached through an element/value/field projection requires a
separate proof for that inner backing. Field-sensitive ownership for records and
element/value-sensitive ownership for nested collections should share the same
principle: fresh shell allocation does not imply deep ownership of reachable
reference-typed contents.

The first implementation can be conservative and reject most nested inner
mutation. The important requirement is that the IR/debug output names the reason:
outer owned but inner unknown/shared, projected inner ownership proven, or nested
publication detected.

### Promotion/freeze model — later

A future mutable-region representation may need explicit freeze/publish operations
when a mutable region must produce an ordinary persistent value. That model should
not be part of the first implementation, which reuses existing in-place and
builder helpers.

When this later model is introduced, a freeze is required before observable
publication, but should not be inserted between internal updates in the same
proven region. If a persistent value is provably owned, mutation can begin without
copying. If a persistent value is not proven owned, the optimizer must leave the
operation on the persistent path unless a future explicit clone-to-owned operation
is proven semantically necessary and profitable.

## Milestone plan

The future work is organized into three owned tracks: first build the CFG
ownership view, migrate analysis/passes to consume it, and print minimal
`Unique`/`Shared`/`Unknown` ownership facts; then consume those ANF-keyed
facts/decisions through existing persistent/in-place/builder lowering hooks; then
clean up the successful hook-based lowering behind compiler-private intrinsics.
Baseline work is a precondition, and later path precision, specialization,
mutable intrinsics, cleanup, and Buffer retirement are follow-on milestones rather
than prerequisites for the first working ownership engine. The detailed track
checklists live in [analysis/README.md](analysis/README.md),
[codegen/README.md](codegen/README.md), and
[migration/README.md](migration/README.md).

### Precondition — Baseline and guardrails

- Keep a same-session baseline script for comparing current branch, historical
  optimized revisions, and new work.
- Track ordinary AWFY variants against their `*_mut` counterparts.
- Keep negative aliasing cases as first-class guardrails: slices, record fields,
  variant payloads, array literals, nested collection elements/values, closure
  captures, task/fiber captures, channel sends, globals, and unknown calls.
- Prefer WAT/codegen checks that show which helper was emitted, alongside runtime
  checksum checks.

Exit criteria: the team can quickly answer whether a change improved ordinary
AWFY performance, preserved checksums, and avoided known aliasing corruptions.

### Phase 1A — CFG ownership view and printable facts, no rewrites

This phase lands in two slices, matching the analysis README's Phase 1 / Phase 2 split:
**1A-view** builds the structural CFG view first — verifiable on its own terms,
running no analysis — and **1A-facts** populates the minimal ownership facts on
top of it. Keeping them separate lets the greenfield CFG infrastructure be tested
for graph correctness and determinism before any lattice transfer is layered on.

**1A-view — structural CFG view (README Phase 1):**

- Add the deterministic CFG ownership view with SSA-style block parameters for
  carried values only, identified from ANF syntax: every `AAssign` target (loop
  and branch-arm rebinds), `AIf`/`AMatch`/`ALoop` result bindings, and `Break`
  payloads — see [cfg-ownership-ir.md](analysis/cfg-ownership-ir.md).
- Build the CFG view from the defer-free `artifacts.opt` and preserve mappings
  back to the optimized-ANF lets/ops (keyed by `artifacts.opt` `LocalId`), not
  pre-optimization source ANF.
- Extend `twk ir` with a way to print the structural CFG (e.g. `twk ir --cfg`):
  block graph, carried block parameters, terminators (including value-carrying
  break edges), and per-block ANF mapping, with the entry/exit fact maps shown
  empty. Output must be designed for manual debugging.
- Keep generated code unchanged.

**1A-facts — minimal ownership facts (README Phase 2):**

- Keep the first executable ownership facts simple (`Unique`/`Shared`/`Unknown`),
  while leaving room in the view for later record shell and field-sensitive facts.
- Model operation effects through optimizer semantics rather than hardcoded source
  names where possible (candidate detection reuses the Phase 0 census's
  `OptimizerSemantics` approach).
- Rebuild local ownership, binding-validity, liveness, and escape facts on the CFG
  view without emitting any mutable rewrites.
- Print facts at the level where decisions are made: local ownership state,
  binding validity, last-use/liveness, borrow/read observations,
  publication/escape events, branch joins, loop-carried/back-edge ownership facts,
  and candidate update-site verdicts. Later debug modes should add record
  shell/field ownership, return-path ownership, transport-wrapper projections,
  summaries, specialization choices, and richer codegen decisions.
- For every rejected mutable candidate, print the reason in terms of the proof
  model, such as old value observable, unknown call boundary, captured by
  closure, stored in aggregate, branch fact mismatch, or insufficient deep
  ownership.
- Validate branch joins and loop-carried back-edge ownership conservatively
  before using the facts for code generation.
- Keep generated code unchanged while the CFG view and printed facts stabilize.

Exit criteria: candidate sites and call sites can be classified and explained in
CFG/ownership output, but generated code is unchanged. We can manually analyze
AWFY and compiler workloads, then verify that the printed facts match the
expected first-domain ownership story; later debug modes can add the
specialization story.

### Phase 1B — Migrate existing optimizer decisions to CFG facts

- Move ownership-relevant liveness/use/control-flow queries onto the CFG view.
- Keep simple local peephole passes ANF-based only where they do not need
  control-flow facts.
- Ensure dead-let/copy-prop/branch decisions that depend on liveness or joins use
  the shared CFG-derived facts instead of independent recursive walks.
- Preserve output equivalence while this migration happens.

Exit criteria: the optimizer has one shared derived view for liveness, joins,
loop back-edges, and ownership facts, while ANF remains authoritative and
generated code remains unchanged.

### Phase 2A — First codegen from ownership facts via existing hooks

This architecture milestone corresponds to the finer codegen-track slices in
[codegen/README.md](codegen/README.md): operation catalog, decision handoff,
backend lookup/fallback, then narrow emitted lowering families.

- Teach the optimizer/backend to consume Phase 1 facts rather than rediscovering
  ownership with separate recognizers.
- Enable mutation lowering only through existing persistent/in-place/builder
  strategies: no new mutable runtime representation, `begin_mutable`, or `freeze`
  model in the first implementation.
- Support fresh/proven-unique vector and dict updates where the simple domain and
  last-use facts are sufficient.
- Keep all publication and aliasing sinks as hard blockers.
- Preserve the Phase 1 IR/debug output so each generated in-place helper or
  builder lowering can be traced back to the proof/debug id that licensed it.
- Keep codegen mechanical: consume accepted decisions from ANF-keyed CFG analysis
  annotations/side tables, and do not add a second uniqueness/escape proof in
  backend code.

Exit criteria: existing in-place/builder hooks are selected from CFG ownership
facts, negative aliasing cases remain persistent and correct, and every emitted
optimized helper has an inspectable ownership proof.

### Phase 2B — Existing vector builders and append/extend regions

- Generalize from indexed updates to append/build loops using the existing builder
  hooks.
- Preserve existing `collect` behavior while allowing the optimizer to choose the
  existing builder path for hand-written accumulator loops when facts justify it.

Exit criteria: vector build/update patterns consume the shared ownership facts
instead of separate recognizers for builders and in-place `set_at`.

### Phase 2C — Mutable HAMT dict regions and record-backed wrappers

- Apply the shared ownership/publication framework to `Dict.set` and
  `Dict.remove`.
- Preserve dict insertion-order iteration and observable key behavior.
- Preserve the nested-collection rule: ownership of a dict node/value slot does
  not imply ownership of a reference-typed value stored in that slot.
- Target compiler env/registry-style threaded dict flows as realistic analysis
  and correctness stress cases.
- Handle record-backed wrappers, including `Set<K>`, through field-sensitive
  ownership rather than a separate Set-only pass.
- Keep old-version observability as the main blocker: if an older dict value can
  be read, the update must remain persistent.

Exit criteria: dict-heavy compiler code gets in-place HAMT updates only when the
analysis proves the old version is unobservable. Dict-backed record wrappers
benefit later, when field-path precision is added.

### Phase 2D — Later mutable-intrinsic cleanup

- After the fact engine and existing-hook lowering are stable, introduce explicit
  optimizer IR nodes or annotations for mutable-region intrinsics if the
  builder/in-place helper surface becomes too implicit.
- Move codegen toward intrinsic region operations: begin, read, write, append,
  remove, freeze.
- Route existing vector builder hooks and vector/dict in-place helpers through the
  shared mutable-collection intrinsic model.
- Keep runtime helper names private and backend-specific.
- Stop treating prelude-visible transient forms as optimization targets.

Exit criteria: the optimizer targets a stable mutable-region concept rather than
ad hoc helper-call rewrites, and existing transient hooks are either hidden behind
that concept or identified as removable compatibility scaffolding.

### Follow-up — Buffer retirement path

- Compare ordinary AWFY variants to the manual Buffer variants in regular perf
  runs.
- Once ordinary code reaches the same performance class for workloads where
  mutation is the bottleneck, stop treating `Buffer` as the recommended user
  solution for those cases.
- Deprecate and then remove explicit Buffer APIs and benchmark variants only
after ordinary code has a proven replacement path.

Exit criteria: `Buffer` is no longer needed to express high-performance local
collection updates in user code. See [buffer-cleanup.md](migration/buffer-cleanup.md) for
the focused cleanup policy.

## Testing and verification

Correctness gates:

```bash
target/twk test
make stage2
target/twk run boot/tests/main.tw
```

End-of-track performance and checksum gates:

```bash
target/twk run examples/performance/awfy/twinkle/main.tw
examples/performance/awfy/run.sh
make awfy
```

Focused codegen inspection:

```bash
target/twk build examples/performance/awfy/twinkle/main.tw -o /tmp/awfy.wat
target/twk wat examples/performance/awfy/twinkle/main.tw --func sieve --calls
target/twk wat examples/performance/awfy/twinkle/main.tw --func bounce --calls
```

Expected verification style:

- during the refactor, prefer correctness tests and IR/codegen inspection over
  timing interpretation;
- when preparing to merge, record ordinary-vs-mut timings from the same
  machine/session;
- verify AWFY checksums before interpreting performance;
- inspect generated calls when a benchmark moves unexpectedly;
- keep aliasing negatives in the boot test suite, not only as ad hoc repros.

## Non-goals

- No user-visible borrow checker or ownership annotations.
- No runtime uniqueness flags or refcounts.
- No speculative runtime "maybe shared" COW decision for optimizer correctness.
- No requirement that every persistent update optimize.
- No whole-program alias theorem prover as a first milestone.
- No Rust stage0 port unless bootstrapping or reference parity requires it.
- No Buffer removal before ordinary immutable code reaches the target class.
- Do not remove ANF or make CFG the authoritative codegen IR as part of this
  project.

## Open design questions

- Should record shell reuse use the same mutable-region intrinsic family, or a
  smaller record-specific intrinsic/annotation such as today's `ARecordUpdate`
  in-place bit?
- How should field-sensitive ownership be represented so wrapper records such as
  `Set<K>` naturally project to owned `Dict<K, Void>` when sound?
- Should the first mutable vector target reuse the current PVec shape, add a new
  private mutable PVec, or introduce typed/flat storage at the same time?
- How much interprocedural summary information is needed beyond thin wrappers?
- Should mutable-collection decisions be represented as ANF annotations, an
  ANF-keyed side table, or both?
- What should the specialization key look like for ownership-monomorphized
  function variants, and which parameters or record fields are allowed to
  participate?
- What variant cap should prevent type-monomorphized clones multiplied by
  ownership-specialization variants from exploding, and how should the compiler
  choose fallback sites?
- Should private tagged collection/record representations be used to reduce
  variant count, and where is runtime dispatch acceptable without losing the
  AWFY target?
- Should freeze/publish be explicit ANF, a codegen annotation, or a separate
  post-ANF lowering phase?
- How should branch-local mutable regions merge when both branches update the
  same owned collection and publish one joined value?
- How should loop-carried ownership facts be represented at back-edges, and what
  proof should license an owned handle to remain mutable across iterations with
  interleaved reads?
- How should concurrency publication sinks be represented for Task/fiber capture,
  Channel sends, and serialized cross-worker messages?
- How much nested collection ownership should Phase 1 model beyond conservative
  rejection of inner mutation through projected elements/values?
- Which non-escaping closure cases are worth recovering after the initial
  hard-blocker treatment for closure capture?
- Which compiler workloads should become analysis and correctness stress cases
  for dict and record-wrapper regions?

## Relationship to existing docs

This plan supersedes the active intent of the boot uniqueness design represented
by `docs/plans/archive/boot-uniqueness-deep-ownership.md` and the removed boot
optimizer passes. `docs/plans/archive/static-uniqueness-plan.md` remains useful
historical evidence for patterns, measurements, and prior pitfalls, but it
described an incremental extension of an older optimizer rather than the current
from-scratch sound mutable-lowering design.

Sibling subplans:

- `docs/plans/sound-uniqueness/analysis/design-rationale.md`
- `docs/plans/sound-uniqueness/analysis/worked-examples.md`
- `docs/plans/sound-uniqueness/analysis/fact-lattice.md`
- `docs/plans/sound-uniqueness/analysis/summary-specialization.md`
- `docs/plans/sound-uniqueness/analysis/cfg-ownership-ir.md`
- `docs/plans/sound-uniqueness/analysis/sound-analysis.md`
- `docs/plans/sound-uniqueness/analysis/closure-capture.md`
- `docs/plans/sound-uniqueness/analysis/concurrency-publication.md`
- `docs/plans/sound-uniqueness/analysis/records-fields.md`
- `docs/plans/sound-uniqueness/codegen/handoff-contract.md`
- `docs/plans/sound-uniqueness/codegen/operation-catalog.md`
- `docs/plans/sound-uniqueness/codegen/existing-hooks.md`
- `docs/plans/sound-uniqueness/migration/mutable-intrinsics.md`
- `docs/plans/sound-uniqueness/migration/buffer-cleanup.md`

Relevant references:

- `docs/internals/persistent-runtime.md`
- `docs/design/buffer.md`
- `docs/plans/archive/awfy-c5-inplace-vector.md`
- `docs/plans/archive/boot-uniqueness-deep-ownership.md`
- `docs/plans/archive/static-uniqueness-plan.md`
