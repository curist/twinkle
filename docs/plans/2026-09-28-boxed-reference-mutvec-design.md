# Boxed Reference MutVec Design

## Purpose

Extend Twinkle's compiler-private MutVec optimization from the existing
unboxed primitive element families to concrete `Vector<T>` instantiations whose
elements are Wasm GC references. Ordinary immutable code using types such as
`Vector<Person>` should retain a uniquely owned vector in flat mutable storage
through accepted recursive and helper-call regions, while preserving the
language's immutable value semantics and persistent fallback.

The spike in `.worktrees/boxed-mutvec-spike` demonstrated the representation
and performance potential: a write-only recursive `Vector<String>` carrier was
about eight times faster with boxed mutable storage than with persistent vector
updates. The spike is throwaway evidence, not production code. It also exposed
the main missing proof: a reference-valued element read currently aliases the
vector shell in ownership provenance, preventing realistic read-before-write
workloads such as swapping two records.

## Goals

- Optimize concrete GC-reference-valued vectors, including `Vector<MyRecord>`,
  `Vector<String>`, nested vectors, enums represented as GC references, and
  closures.
- Preserve the existing source-level `Vector<T>` API and immutable semantics;
  no public mutable collection, annotation, capability, or source rewrite is
  introduced.
- Reuse the existing owned-specialization, recursive aggregate ABI, flat-handle
  propagation, variant-cap, and publication-boundary machinery.
- Keep primitive `Int`, `Float`, `Bool`, and `Byte` elements in their existing
  unboxed MutVec families.
- Make realistic element reads followed by vector updates eligible when the
  vector shell remains uniquely owned.
- Fall back to the persistent implementation whenever shell uniqueness,
  element representation, lineage, helper ABI, or publication behavior is not
  proven.

## Non-goals

- A general mutable-vector feature in the language or standard library.
- In-place mutation of records, strings, nested vectors, or other objects held
  in vector slots.
- Optimizing representation-polymorphic code whose element representation is
  still unknown at code generation time.
- Boxing `ExternRef` or `Optional<ExternRef>` elements. Wasm extern references
  are not members of the GC `anyref` heap-type hierarchy.
- Converting an arbitrary persistent vector parameter into mutable storage.
  This slice remains rooted at vector producers already accepted by S4.
- Replacing the four specialized primitive families with boxed storage.
- Adding a distinct Wasm array or runtime operation family for every concrete
  reference type.
- Expanding the recursive aggregate optimization to multiple collection fields,
  multiple scalar results, or previously unsupported control-flow shapes.
- General-purpose borrow checking. The ownership refinement is limited to the
  shell-versus-element distinction needed for collection access.

## Decision

Add one compiler-private boxed family, `MutVecBoxed`, backed by
`array<anyref>`. Every accepted concrete `Vector<T>` whose element is a
nullable or non-null GC reference with a heap type below Wasm `any` shares this
physical mutable family. `externref` and nullable `externref` are explicitly
ineligible. Compiler IR
continues carrying the exact monomorphized `Vector<T>` type; the shared physical
family does not erase static type identity or permit values of different `T`
to mix.

The alternatives are rejected:

- A distinct mutable array type per `T` would increase Wasm types, runtime
  functions, layout bookkeeping, and ABI variants without evidence that it
  improves the target workloads.
- A universal boxed family for primitives would introduce boxing and discard
  the performance and representation benefits already delivered by the
  specialized primitive families.

## Semantic Model

Three facts must remain independent:

1. **Vector-shell ownership** determines whether an accepted vector producer
   may build into private flat storage and remain there across the region.
2. **Element-reference provenance** records where a reference loaded from a
   vector slot came from and whether that reference is borrowed, moved, or
   published.
3. **Element-object ownership** governs any later optimization of the referenced
   record, vector, closure, or other object. It is not implied by owning the
   outer vector shell.

For example:

```tw
a := people[i]
b := people[j]
people[i] = b
people[j] = a
```

The two reads produce `Person` references. They do not create aliases to the
`Vector<Person>` shell, so the shell can remain uniquely owned. The `Person`
objects may be shared and remain immutable. Assigning `people[i]` changes only
the reference stored in that private vector slot. Updating a field of `a` still
constructs a new `Person` under Twinkle's normal rebinding semantics.

An element that escapes through a return, record field, collection store,
closure capture, global, or unknown call is published as an element reference.
That publication must not publish the vector shell unless the shell itself also
escapes or survives through an alias.

## Representation and Type Classification

Element-family classification gains one closed family for GC references:

- `Int` -> existing i64 family
- `Float` -> existing f64 family
- `Bool` -> existing bool family
- `Byte` -> existing byte family
- a concrete element type whose lowered `ValType` is a GC reference below
  Wasm `any` -> boxed family
- `ExternRef` and `Optional<ExternRef>` -> no mutable family
- `Void`, `Never`, erased `Anyref_`, unresolved `Var`/`MetaVar`, and
  `ErrorType` -> no mutable family, regardless of any sentinel Wasm type used
  elsewhere during lowering
- an unknown, erased, unsupported, or non-reference representation -> no
  mutable family

The boxed classification is based on the fully lowered Wasm heap-type class,
an explicit semantic-type allow/deny check, and not nominal identity. It must
not be implemented as merely "`val_type_of_mono(T)` returns `.Ref`" because
semantic non-values such as `Void` may use a reference-shaped lowering
sentinel. One `MutVecBoxed` therefore serves every eligible record and other GC
reference type.
The exact `Vector<T>` `MonoType` must travel with each detected region,
flat-handle fact, ABI upgrade, and producer/freeze boundary. Reconstructing it from
the physical-family name is forbidden because the boxed name cannot identify
`T`.

`MutVecBoxed` contains a mutable `array<anyref>` backing and a logical length,
matching the shape and bounds behavior of the primitive MutVec types. Loads
cast from `anyref` to the statically known element reference type at the typed
boundary; stores upcast that type to `anyref`. A cast failure is not an expected
runtime path: only compiler-generated, statically typed operations can access
the private handle. The backend verifier must reject a boxed operation whose
exact vector mono and value/result type disagree.

The private runtime operation family provides the same seven operations and
behavior as existing MutVec families: new, make, push, get, set, length, and
freeze. There is no thaw operation. As in existing S4, accepted producers are
rewritten so their values are built directly in private flat storage before a
persistent PVec trie is materialized. This design never mutates or reuses a
persistent vector's backing nodes; arbitrary PVec parameters remain persistent.
Bounds checks, logical length, capacity growth, and trap behavior remain
consistent with persistent `Vector<T>` and the primitive mutable families.

## Ownership and Provenance Refinement

The ownership model gains a separate projected-borrow channel:

```text
ProjectedBorrow = { roots: Vector<Int>, path: AccessPath }
```

`roots` stores the same canonical parameter-origin ids already used by shell
provenance, while `path` uses `field_facts.AccessPath`. The projection is
kept separately from a local's shell `prov`; it is not encoded by putting the
base's roots into the result's shell provenance. Dynamic vector indices
collapse to the conservative path `[Elem]` because this scope does not prove
per-index disjointness.

The ownership transfer for `AIndex(base, index, ..., elem_ty)` changes only for
GC-reference-valued vector elements:

- The result is a borrowed element reference by default.
- Its `ProjectedBorrow` contains the collection's canonical roots plus `[Elem]`
  (or the composed outer path plus `[Elem]`), while its shell provenance is
  empty.
- Creating the element result does not demote or publish the vector shell.
- The result is `Shared`/borrowed even when the base and `[Elem]` field fact are
  unique. This slice never transfers an element object's ownership from an
  index read and therefore never seeds an inner owned-vector route.
- Publishing the result publishes only the projected element path and any
  object reachable through that result, not the collection shell.

`ProjectedBorrow` is copied by `AInit` and assignment, conservatively met at
branches and loops, and cleared when roots or paths disagree. Record, variant,
and collection construction graft it under the destination field, payload, or
element path in the same way existing `path_prov` preserves nested origins.
Unknown calls conservatively publish the projected borrow. Known-call summaries
gain an explicit borrowed-projection return classification equivalent to
`BorrowedFromPath(param, path)`; unlike `OwnedFromParam` and whole-return alias
forms, it must never qualify the source shell for an owned route. Call transfer
re-roots that returned projection through the actual argument and preserves the
empty shell provenance.

If the collection base is moved, rebound, or has the indexed slot replaced
while an element local remains live, the element local remains a valid shared
GC reference. It does not borrow vector storage. Replacement may invalidate or
remove deep `[Elem]` ownership facts on the new collection value, but it does
not retroactively turn the loaded element into a shell alias.

Primitive element reads keep their existing behavior and carry no reference
provenance. Reads from dictionaries, records, tuples, variants, and other
containers are unchanged unless they already use the same collection-element
path abstraction.

The implementation must use the existing `field_facts.AccessPath` vocabulary
and ownership authority rather than introduce a second alias analysis. If the
current path/provenance lattice cannot express a collection element separately
from its shell at all relevant joins, it should be extended with that meaning
at the ownership layer. A local special case inside MutVec detection is not
sufficient because variant specialization and helper summaries run earlier and
must see the same distinction.

Joins are conservative: conflicting shell origins, projected-borrow roots or
paths, unknown calls, or unsupported path transformations lose eligibility
rather than merging into an optimistic fact. Losing projected detail may
publish the element object but must neither manufacture shell uniqueness nor
demote an otherwise independent shell. Fixpoint fallback and iteration caps
must produce persistent code.

## Optimization Pipeline

The boxed family enters through the same pipeline as primitive families:

1. Ownership analysis and variant specialization prove an owned route. The
   shell-versus-element refinement allows element reads without destroying
   shell uniqueness.
2. MutVec region detection classifies the exact `Vector<T>` as boxed and records
   that mono on the region.
3. Recursive aggregate verification applies the existing carrier, lineage,
   recursion, alias, exit, and publication checks without boxed-specific
   exceptions.
4. Flat-handle propagation creates edge-specific helper siblings and retargets
   boxed get/set calls. Existing rules continue to forbid a helper clone from
   being shared across incompatible handle slots or ABIs.
5. Backend representation assignment maps the accepted handle to
   `MutVecBoxed`, while typed operands and results retain their exact reference
   types.
6. The accepted vector producer builds directly into the private handle. The
   accepted clone graph operates on that handle. A dead vector result performs
   no freeze; an observed vector or carrier performs exactly one freeze at its
   publication boundary.

Exact mono is part of the proof identity, not just diagnostic metadata.
`AggregateMutVecRegion`, each flat edge and sibling, propagation memoization,
`MutVecCallAbiUpgrade`, caller rewrite, and backend verification all carry and
compare `vector_mono`. A helper sibling is identified by at least
`(callee, handle_param, vector_mono)`; an existing sibling may be reused only
when all three agree. `ElemRepr.Boxed` alone is never sufficient to retarget a
call or select a memoized sibling.

This feature does not create an independent boxed-vector pass. Boxed regions
share the existing variant-cap partition, rewrite orchestration, and fallback
path. Cap exhaustion or failure anywhere in a proposed clone graph rejects the
whole mutable chain before rewriting.

## Nested and Generic Types

For `Vector<Vector<Person>>`, the outer vector stores references to persistent
inner vectors. Proving the outer shell unique permits in-place replacement of
outer slots but says nothing about an inner vector's uniqueness. In this slice,
loading an inner vector through an outer index always produces a shared
projected borrow and cannot seed an owned route for the inner vector. An inner
vector may receive MutVec only from an independent accepted producer; moving it
uniquely out of an outer slot would require a future consume/replace proof. The
two handles must never be inferred to share ownership merely because one was
loaded from the other.

Monomorphized generic functions may use boxed MutVec when their selected clone
has a concrete reference-valued `T`. A function compiled with representation-
polymorphic or erased `T` remains persistent. This design does not add runtime
type dispatch between primitive and boxed families.

## Rejection and Fallback Rules

The persistent implementation remains authoritative when any of these occurs:

- the caller retains an alias to the vector or its carrier field;
- the recursive route returns or substitutes a different vector lineage;
- the same physical handle reaches multiple callee slots;
- a helper is shared across persistent and mutable ABIs without an edge-specific
  clone;
- a helper captures the handle, republishes it, or passes it to an unsupported
  callee;
- the element representation is unknown or not supported by a MutVec family;
- the element lowers to `externref` rather than a GC reference below `any`;
- exact `Vector<T>` metadata is lost or conflicts across an edge or join;
- variant-cap reservation, ownership analysis, or a dataflow fixpoint fails;
- an unrecognized ANF operation touches the handle or carrier;
- backend verification cannot prove the typed cast/store relationship.

Fallback is all-or-nothing for the affected flat-handle chain. The compiler
must not emit a partially rewritten chain that alternates boxed and persistent
ABIs through implicit conversions.

## Runtime and Backend Safety

The boxed runtime is compiler-private and cannot be named by Twinkle source.
Every operation performs the same null, bounds, and logical-length checks as its
primitive counterpart. `set` mutates only the private backing array represented
by the handle. `freeze` returns a normal persistent `Vector<T>` and ends the
mutable region at that boundary.

The builtin ABI registry, runtime type declarations, Wasm layout registry,
representation policy, and prepared-IR MutVec assignment must agree on the
boxed handle type and `anyref` element ABI. The verifier must catch mismatched
families and monomorphizations before Wasm emission. Generated WAT must contain
typed casts only at boxed operation boundaries, not repeated boxing wrappers
around already-reference-valued elements.

## Diagnostics

Existing MutVec census and route diagnostics should render the boxed family and
the exact vector mono, so `Vector<Person>` and `Vector<String>` routes remain
distinguishable even though they share physical storage. Rejection messages
should distinguish at least:

- aliased vector shell;
- element provenance conflict;
- unsupported or unknown element representation;
- incompatible boxed mono across a helper edge;
- ordinary aggregate or flat-propagation rejection.

No new user-facing language diagnostic is required; this remains an optional
optimization.

## Verification Strategy

Tests must prove both semantic correctness and route selection.

### Ownership tests

- A reference element read does not publish or demote a uniquely owned vector
  shell.
- Returning, storing, or capturing the element publishes the element reference
  but not the vector shell.
- A surviving alias to the vector still demotes the shell and prevents MutVec.
- Conflicting element paths at a branch join conservatively reject ownership
  transfer without corrupting shell facts.
- Nested-vector element ownership remains independent from the outer shell.

### Region and ABI tests

- Recursive `Vector<Person>` swap accepts a boxed aggregate route and executes
  correctly.
- `Vector<String>` and another nominal record type share the physical boxed
  runtime family while retaining distinct exact monos.
- Mixed boxed and primitive helper routes never retarget across incompatible
  ABIs.
- A persistent caller and mutable caller of the same source helper receive
  separate compatible siblings or the mutable route falls back.
- Lost mono metadata, unsupported generic `T`, handle duplication, capture,
  republish, mixed lineage, and cap exhaustion stay persistent.
- `Vector<ExternRef>` and `Vector<ExternRef?>` remain persistent and never
  select boxed storage.
- vectors over `Void`, erased `Anyref_`, unresolved types, and error sentinels
  are rejected by the family classifier before route construction.
- A dead vector result emits no freeze; an observed vector or containing record
  emits exactly one freeze.
- Boxed loads and stores use the boxed runtime operations, and the accepted hot
  clone graph contains no persistent vector update.

### Runtime tests

- Empty, singleton, growth, get, set, push, and freeze behavior matches
  persistent vectors.
- Negative, logical-length, and oversized indices trap consistently.
- Null-capable reference values, if they are legal for the concrete Twinkle
  element type, round-trip without changing nullability.
- Records containing reference fields, nested vectors, strings, enums, and
  closures round-trip as references without object mutation.

### Integration gates

- Format and lint every modified Twinkle file.
- Run focused ownership, MutVec-call, recursive-record, and boxed fixtures while
  iterating.
- Rebuild through the self-host fixed point and run the complete Rust and boot
  suites before completion.
- Never run tree-sitter tests.

## Performance Gate

Use an ordinary immutable, producer-rooted recursive workload that swaps
user-defined records in `Vector<Person>` through the same aggregate/helper
shape that production code is expected to use. Confirm route selection in WAT
before accepting timings.

The reproducible persistent baseline is the identical benchmark source compiled
from the parent commit immediately before boxed route activation. Record both
compiler commit ids, build both standalone compilers, and run them in the same
benchmark session. Before timing, the baseline WAT must show persistent vector
updates and the candidate WAT must show boxed MutVec operations. No feature flag
or deliberately pessimized source variant is added solely for benchmarking.

Run at least three samples from each recorded compiler build in the same
benchmark session and compare medians. The feature passes when:

- ordinary optimized source is at least `3.0x` faster than the parent-commit
  persistent baseline;
- the optimized hot clone graph uses boxed MutVec get/set operations and no
  persistent vector update;
- the result checksum matches the persistent implementation; and
- dead and published result variants retain their zero-freeze and one-freeze
  invariants.

The spike's roughly eightfold improvement is evidence, not the required result.
If the production read-before-write benchmark misses `3.0x`, profile and open a
focused follow-up rather than weakening ownership or ABI checks.

## Expected Implementation Areas

The implementation plan should map exact edits after re-inspecting current
interfaces, but responsibility is expected to remain in these existing units:

- `boot/compiler/elem_family.tw` and backend representation policy for boxed
  family classification;
- `boot/compiler/ownership.tw` and `field_facts` access paths for independent
  element provenance;
- runtime type, array-operation, layout, and builtin registries for
  `MutVecBoxed`;
- aggregate region/rewrite and MutVec backend representation code for carrying
  exact `Vector<T>` monos;
- `boot/tests/suites/mutvec_call_suite.tw` and focused ownership suites for
  route, fallback, and WAT assertions;
- an AWFY-style immutable `Vector<Person>` benchmark fixture for the performance
  gate.

Rust stage0 changes are required only if the boot compiler cannot bootstrap the
new runtime or representation declarations without them.

## Delivery Boundaries

Land the feature in independently reviewable stages:

1. ownership semantics and negative tests for shell-versus-element provenance;
2. boxed runtime representation and typed ABI;
3. boxed region selection and exact-mono propagation through aggregate/helper
   rewrites;
4. integration fallback coverage and self-host verification;
5. representative record-swap performance gate and documentation.

Each stage must leave the compiler correct and persistent fallback functional.
The implementation plan may combine mechanically coupled runtime and backend
steps, but it must not combine the ownership proof change with route activation
before the ownership tests are independently green.
