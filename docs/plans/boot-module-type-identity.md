# Boot Module Type Identity Plan

## Goal

Make imported nominal types in the boot compiler keep a single semantic
identity across module boundaries, regardless of import topology.

In practice, this means:

- full imports and selective imports must refer to the same imported type
- exported function signatures must carry enough type metadata to remain valid
  when re-imported elsewhere
- hidden selective-import machinery must not allocate semantic duplicate
  `TypeId`s
- imported-type identity must not depend on the importer's local type-count
  layout

This plan addresses the structural issue behind recent boot multi-module bugs:

- mixed full + selective import of the same module producing duplicate type IDs
- selective-only imported sum types failing variant resolution after crossing
  module boundaries

## Status

Phase 0 complete. Phases 1–5 planned.

## Why Now

The current multi-module boot pipeline is functional, but its type-identity
model is still projection-based rather than canonical:

- module exports are re-materialized into each importing environment
- import merging allocates fresh `TypeId`s based on importer-local state
- selective imports rely on hidden aliases as an implementation trick
- exported signatures can reference imported support types that are not modeled
  explicitly as part of a module interface

That combination is workable for simple cases, but fragile for real compiler
modules like:

- [`boot/compiler/lexer.tw`](../../boot/compiler/lexer.tw)
- [`boot/compiler/cursor.tw`](../../boot/compiler/cursor.tw)
- [`boot/compiler/parser.tw`](../../boot/compiler/parser.tw)

Those modules mix:

- public and non-public types
- full and selective imports
- public functions whose signatures mention imported nominal types
- downstream shorthand variant use on selectively imported sum types

Recent fixes removed the immediate failures, but they were still repairs inside
the current projection model. The root problem is that imported nominal type
identity is implicit and recreated ad hoc instead of being represented
explicitly.

## Current State

### What Boot Does Today

The current boot multi-module path lives primarily in:

- [`boot/compiler/module_compiler.tw`](../../boot/compiler/module_compiler.tw)
- [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw)
- [`boot/compiler/checker.tw`](../../boot/compiler/checker.tw)

Current behavior:

1. compile each dependency against the base builtin env
2. extract a `ModuleExports` payload from the dependency env
3. merge those exports into the importer env
4. remap imported `TypeId`s into importer-local IDs during merge
5. use hidden aliases like `$import$.alias.Type` for standalone selective
   imports

This works when every relevant imported type is explicitly visible and merged in
exactly one way. It becomes brittle when:

- two projection paths refer to the same source type
- a public signature mentions a support type that is not itself a public
  declaration of the current module
- downstream checking expects variant/type-definition lookup through the merged
  ID

### Current Structural Smells

#### Type identity is recreated per importer

`merge_module_exports(...)` computes new `TypeId`s from `env.types.len()`. That
means identity depends on importer-local history instead of the source module.

#### Hidden names are doing identity work

Standalone selective imports merge under `$import$.alias.*` and then alias out
selected names. Those hidden names are currently more than a lookup trick:
they indirectly determine which `TypeId` survives.

#### Public API closure is incomplete

A module's public function signatures can mention imported nominal types.
Those support types are part of the semantic surface of the module interface,
but the current `ModuleExports` shape does not model that cleanly.

#### Method visibility is keyed by type names

The boot resolver currently tracks methods under string type names. This makes
aliasing/import projection more delicate than it should be, because name choice
and type identity are coupled.

## Root Cause

The root cause is that boot multi-module compilation treats imported nominal
types as importer-local rebindings instead of canonical external identities.

Names, IDs, and visibility are conflated:

- a type's local env slot
- the name used to refer to it in the current module
- the identity used by checker/lowerer/method lookup
- whether the type is public or merely required to interpret public signatures

Because those concerns are collapsed together, import projection has to
"reconstruct" type identity by cloning `TypeEntry`s and remapping their IDs.
That is why subtle changes in import topology can create duplicate or
incompatible nominal types.

## Design Direction

### Principle: Names Are Views, Identity Is Canonical

Imported nominal types should have a canonical identity that does not change
when:

- imported fully
- imported selectively
- re-exported through function signatures
- brought into scope under multiple names
- referenced through hidden selective-import support machinery

Names should be bindings to that identity, not the source of that identity.

### Principle: Module Interfaces Must Be Closed Over Referenced Types

A module interface is not just:

- public type declarations
- public function names

It must also include the nominal support types reachable from public signatures
and method surfaces.

That closure should be explicit in the export model rather than discovered
later by ad hoc hidden-name logic.

### Principle: Selective Import Should Not Change Semantics

`use .m`
and
`use .m.{T}`
and
`use .m` plus `use .m.{T}`

must all describe the same imported type identity. The only difference should
be which names become available in the local scope.

## Proposed End State

### 1. Enrich `ModuleExports` With Interface-Type Closure

Extend the export model so that a module's exported interface includes:

- visible/public type bindings
- support types reachable from exported function signatures and method
  receivers/results
- a clear distinction between:
  - visible bindings
  - support-only types

Support-only types must be importable into the importing env for signature
interpretation, but they do not become user-visible names unless explicitly
bound.

### 2. Give Imported Types Canonical External Identity

Introduce an explicit type-origin key for imported nominal types.

Possible shape:

- `TypeOrigin = Local | External(module_path, exported_name_or_key)`

or equivalent.

The key requirement is:

- the same source type imported through different paths resolves to the same
  semantic identity

This may continue to use integer `TypeId`s internally, but those IDs should be
allocated/reused by origin, not by local env length alone.

### 3. Split "Bring Support Types Into Env" From "Bind Local Names"

Import processing should happen in two conceptual steps:

1. register or reuse canonical imported type identities required by the module
   interface closure
2. bind local names for:
   - qualified module access
   - selective imports
   - aliases

That makes hidden selective-import support types an implementation detail only.
They should never create new semantic type identities once the source module
interface is known.

### 4. Move Method Lookup Toward Identity-Based Registration

Today methods are keyed by string type names in the boot resolver. That makes
aliasing more fragile than necessary.

Target direction:

- register methods by canonical receiver type identity
- optionally retain name-based compatibility helpers during migration

This reduces dependence on whichever name happened to be used during import
projection.

### 5. Make Export/Merge Semantics Symmetric With Stage0 Direction

The stage0 side already models module exports around canonical identities rather
than importer-local clones. The boot side should converge toward the same
semantic model even if the data structures differ.

This plan does not require a literal copy of the Rust implementation, but it
should preserve the same invariants.

## Non-Goals

- rewriting the whole boot module compiler in one change
- replacing the current boot env representation in a single step
- solving builtin signature duplication here
- changing user-facing import syntax
- introducing re-export syntax in the same plan

## Implementation Plan

### Phase 0: Characterization and Invariants

Files:

- [`boot/tests/suites/multi_module_suite.tw`](../../boot/tests/suites/multi_module_suite.tw)
- [`boot/compiler/lexer.tw`](../../boot/compiler/lexer.tw)
- [`boot/compiler/cursor.tw`](../../boot/compiler/cursor.tw)
- [`boot/compiler/parser.tw`](../../boot/compiler/parser.tw)

Add/keep characterization coverage for:

1. mixed full + selective import of the same type-bearing module
2. selective-only imported sum type used locally with shorthand variants
3. selective-only imported type flowing through a public function signature and
   then consumed downstream
4. transitive imported record/sum types used in method and field resolution
5. canary checks for boot compiler modules with real import topology

Required invariant checks:

- no topology-dependent `TypeId` splits for the same source nominal type
- no variant lookup failures caused by import form alone
- no method/field lookup drift caused by alias choice

Exit criteria:

- the suite encodes the failing topologies directly
- at least one real boot compiler module is used as an integration canary

### Phase 1: Make Interface-Type Closure Explicit

Files:

- [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw)
- [`boot/compiler/module_compiler.tw`](../../boot/compiler/module_compiler.tw)

Changes:

1. extend `ModuleExports` to distinguish:
   - visible exported types
   - support/interface types referenced by exported signatures
2. make export extraction compute the closure of nominal support types
3. preserve deterministic order for exported support types
4. document which support types are user-visible bindings vs merge-only support

Exit criteria:

- exported signatures can always be interpreted by an importer without relying
  on hidden local accidents
- export payload shape makes support-type presence explicit

### Phase 2: Canonicalize Imported Type Identity

Files:

- [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw)

Changes:

1. add an explicit imported-type origin model
2. teach merge logic to reuse/import by origin instead of always allocating new
   IDs from `env.types.len()`
3. make `merge_module_exports(...)`, `merge_selective_imports(...)`, and
   `merge_prelude_exports(...)` share the same identity rules
4. keep hidden selective-import aliases as lookup plumbing only, not as
   identity-bearing names

Exit criteria:

- the same source nominal type imported through any supported topology resolves
  to one canonical semantic identity

### Phase 3: Decouple Method Registration From Type Names

Files:

- [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw)
- [`boot/compiler/checker.tw`](../../boot/compiler/checker.tw)

Changes:

1. introduce method registration keyed by receiver identity rather than only
   by string type name
2. adapt imported-type method projection to reuse the canonical receiver
   identity
3. retain compatibility shims only where necessary during migration

Exit criteria:

- method lookup is invariant under aliasing/import topology

### Phase 4: Simplify Selective Import Implementation

Files:

- [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw)
- [`boot/compiler/module_compiler.tw`](../../boot/compiler/module_compiler.tw)

Changes:

1. remove identity-sensitive behavior from the hidden selective-import path
2. make selective import a pure binding operation over already-registered
   imported identities
3. keep any hidden namespace only for internal lookup convenience, if still
   needed

Exit criteria:

- selective import no longer has a unique semantic path
- full and selective import differ only in scope exposure

### Phase 5: Parity/Regression Sweep

Files:

- [`boot/tests/suites/multi_module_suite.tw`](../../boot/tests/suites/multi_module_suite.tw)
- [`boot/tests/suites/resolver_suite.tw`](../../boot/tests/suites/resolver_suite.tw)
- targeted Rust/CLI integration tests where useful

Checks:

1. boot compiler modules that previously exposed the bug now typecheck past the
   old failure site
2. multi-module import tests all pass in both interpreter and Wasm boot modes
3. no regressions in method resolution for imported types
4. no regressions in Core IR lowering/linking caused by canonicalized imported
   identities

## Risks

### Risk: Too Much State Migration At Once

Changing `TypeEntry`, method registration, and export payloads in one step can
destabilize the boot compiler.

Mitigation:

- land characterization tests first
- make the export-closure change incremental
- add compatibility layers while the old and new identity models coexist

### Risk: Hidden Support Types Leak Into User Scope

Making support types explicit must not accidentally expose them as normal user
bindings.

Mitigation:

- represent visibility separately from presence in the interface closure
- test that selective imports still do not bring parent modules or support-only
  names into scope

### Risk: Divergence From Stage0 Semantics

The boot fix could become another boot-only special case.

Mitigation:

- compare each phase against the stage0 module export/import invariants
- prefer semantic parity even if the concrete representation differs

## Success Criteria

This plan is complete when:

- import topology no longer changes nominal type identity
- public signatures referencing imported types remain valid across downstream
  imports
- selective import is semantically equivalent to full import plus fewer local
  bindings
- method and variant resolution for imported nominal types are stable under
  aliasing
- boot compiler modules like
  [`boot/compiler/parser.tw`](../../boot/compiler/parser.tw) no longer fail for
  structural import-identity reasons
