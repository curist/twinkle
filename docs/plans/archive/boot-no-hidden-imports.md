# Boot No-Hidden-Imports Plan

Last updated: 2026-03-28

## Goal

Remove the hidden `$import$...` selective-import path from the boot compiler and
replace it with a clearer import model where:

- imported nominal type identity is registered independently of local names
- support/interface types can exist in the env without synthetic user-facing
  bindings
- full import and selective import share the same semantic import path
- method lookup does not depend on whichever alias name happens to win

This is a follow-on architecture plan to
[boot-module-type-identity.md](boot-module-type-identity.md), not a replacement
for the already-landed type-identity fixes.

## Status

Completed on 2026-03-28.

Phases 0 through 6 are now landed:

- import topology bugs are fixed
- `ResolvedEnv` separates canonical type/function storage from visible bindings
- standalone selective imports register interface closure first, then bind only
  selected local names
- support/interface types can exist without user-visible local bindings
- selective-imported values no longer require hidden linker-origin names
- method lookup follows receiver identity
- hidden selective-import type-name compatibility helpers are removed

## Why Now

The current resolver/module-import path is functionally much better than it was,
but the implementation is still carrying compatibility structure from the older
projection model.

Earlier revisions relied on hidden names like `$import$.alias.Type` or
`$import$.alias.func` to bridge the gap between:

- canonical imported identity
- support-type closure needed for public signatures
- local names that the current module should actually see
- method/value lookup infrastructure that still assumes names are the authority

That is not just cosmetic debt.

It keeps several concepts coupled that should be separate:

- type existence
- type identity
- type visibility in the current module
- import form (full vs selective)
- method registration key choice

As long as those concerns remain partially collapsed together, every future
change in module/import behavior will keep paying a "compatibility plumbing"
tax.

## Why This Is Worth Doing

This plan is worth doing only because it improves the architecture in a real
way, not because the current code looks inelegant.

Concrete benefits:

- clearer invariants:
  - a type can exist in the env without being bound to a local user name
- simpler import semantics:
  - selective import becomes "register interface closure, then bind fewer names"
- lower alias-order fragility:
  - method and type lookup stop depending on whichever name `find_type_name(...)`
    returns first
- cleaner future extension points:
  - re-exports, richer module interfaces, and stage0 parity become easier to
    reason about

This plan is not justified if we only care about preserving today's fixed
behavior forever. It is justified if boot multi-module/import behavior will keep
evolving.

## Non-Goals

- rewriting the entire boot resolver in one step
- replacing all `ResolvedEnv` storage at once
- changing user-facing import syntax
- adding re-export syntax in the same plan
- redoing already-landed type-identity fixes just for stylistic purity

## Current State

The relevant code lives primarily in:

- [`boot/compiler/resolver.tw`](../../../boot/compiler/resolver.tw)
- [`boot/compiler/module_compiler.tw`](../../../boot/compiler/module_compiler.tw)
- [`boot/compiler/checker.tw`](../../../boot/compiler/checker.tw)

The env now represents:

- "this imported support type exists"
- without also representing:
- "this imported support type has some concrete local name binding"

## Design Direction

### Principle: Identity Store And Binding Store Are Different Things

Imported types should be registered in a canonical identity store, while local
bindings should be modeled separately.

The compiler must be able to represent all of these independently:

- the type exists
- the type has a canonical `TypeId`
- the current module binds zero, one, or many names to it
- one of those names may be preferred for diagnostics

### Principle: Interface Closure Registration Happens Before Binding

Import processing should be split into:

1. register/reuse all imported interface types required for typechecking
2. bind the names that the current import form exposes

That should hold for:

- full imports
- selective imports
- aliases
- prelude imports

### Principle: Method Lookup Should Follow Identity

Method resolution for imported nominal types should be keyed by receiver
identity, not whichever surface name happened to be registered first.

### Principle: Hidden Names Should Not Be Required For Correctness

If any hidden namespace remains temporarily during migration, it must be
debugging/compatibility scaffolding only. Correctness and semantic identity must
not depend on it.

## Proposed End State

### 1. `ResolvedEnv` Separates Type Storage From Type Bindings

Target shape, conceptually:

- `types: Vector<TypeEntry>` remains the storage for canonical type entries
- `type_bindings: Dict<String, TypeId>` maps local names to canonical IDs
- optional diagnostic-preferred-name metadata may exist separately

This lets support-only imported types exist in the env without any local user
binding.

### 2. `ModuleExports` Distinguishes Visible Types From Support Types

The export payload should model:

- visible exported type bindings
- support/interface types reachable from exported signatures and methods

Importers must be able to register both, while binding names only for the
visible set requested by the import form.

### 3. Import Merge Becomes Two Explicit Steps

Importing a module should conceptually become:

1. `register_imported_interface_types(...)`
2. `bind_imported_names(...)`

Full import binds:

- qualified module-visible type names
- qualified module-visible function names
- module-qualified method dispatch surface

Selective import binds:

- only the selected type/value names
- optional aliases for those selected names

Neither mode should require hidden names for correctness.

### 4. Selective Imported Values Also Lose Hidden Origin Names

Linker origin tracking should no longer need hidden names like
`$import$.alias.func`.

Selective-imported values should carry enough direct origin metadata that the
lowering/linker path can resolve them without a synthetic hidden binding.

### 5. Method Registration Moves To Receiver Identity

Target direction:

- `methods: Dict<Int, Vector<MethodEntry>>` or equivalent keyed by `TypeId`
- checker method lookup starts from receiver type identity
- name-based helpers remain only for builtins or migration shims where needed

After this, `find_type_name(...)` becomes a diagnostics/display helper rather
than part of semantic resolution.

## Implementation Plan

### Phase 0: Characterization For The No-Hidden Invariant

Files:

- [`boot/tests/suites/resolver_suite.tw`](../../../boot/tests/suites/resolver_suite.tw)
- [`boot/tests/suites/multi_module_suite.tw`](../../../boot/tests/suites/multi_module_suite.tw)

Add/keep tests that make the architecture target observable:

1. standalone selective type import does not retain hidden type bindings
2. selective-only imported support types remain usable in public signatures
   without becoming local user-visible names
3. selective import still does not expose the parent module namespace
4. method/variant/field resolution still works after removing hidden selected
   type bindings
5. no `$import$...` selective-import value names are required by linker-visible
   execution paths

Exit criteria:

- at least one resolver-level test fails if hidden selective bindings are
  reintroduced as correctness machinery

### Phase 1: Introduce Separate Type-Binding Infrastructure

Files:

- [`boot/compiler/resolver.tw`](../../../boot/compiler/resolver.tw)

Changes:

1. add a binding map for type names separate from raw `types` storage
2. convert `lookup_type(...)` and related helpers to read bindings rather than
   assuming `type_names` is the source of truth
3. keep compatibility helpers so existing code can migrate incrementally
4. preserve deterministic preferred-name behavior for diagnostics

Exit criteria:

- the env can represent a type entry without requiring a corresponding local
  user-visible binding

### Phase 2: Make Export Shapes Explicit

Files:

- [`boot/compiler/resolver.tw`](../../../boot/compiler/resolver.tw)
- [`boot/compiler/module_compiler.tw`](../../../boot/compiler/module_compiler.tw)

Changes:

1. split `ModuleExports.types` into:
   - visible exported type bindings
   - support/interface-only imported types
2. preserve deterministic ordering for both groups
3. document and test that support-only types are importable but not automatically
   bound into local scope

Exit criteria:

- export payloads state clearly which imported types are bindable surface names
  versus merge-only interface support

### Phase 3: Split Registration From Binding

Files:

- [`boot/compiler/resolver.tw`](../../../boot/compiler/resolver.tw)

Changes:

1. add a registration step that imports/reuses canonical type identities for the
   full interface closure
2. add separate binding steps for:
   - full import type/value names
   - selective import type/value names
   - aliases
3. remove hidden-name dependence from selective imported type correctness

Exit criteria:

- full import and selective import share the same type-registration path
- selective import differs only in which names are bound locally

### Phase 4: Remove Hidden Selective-Import Value Plumbing

Files:

- [`boot/compiler/module_compiler.tw`](../../../boot/compiler/module_compiler.tw)
- [`boot/compiler/lower_core.tw`](../../../boot/compiler/lower_core.tw)

Status: completed on 2026-03-28.

Landed result:

1. selective imported values are registered canonically and bound separately
2. linker origin tracking uses canonical qualified names plus selected local
   bindings
3. external refs stay stable without `$import$.alias.func` metadata entries

Exit criteria met.

### Phase 5: Move Method Lookup To Identity

Files:

- [`boot/compiler/resolver.tw`](../../../boot/compiler/resolver.tw)
- [`boot/compiler/checker.tw`](../../../boot/compiler/checker.tw)

Status: completed on 2026-03-28.

Landed result:

1. `ResolvedEnv` now carries receiver-identity method storage keyed by `TypeId`
2. checker method resolution for nominal receivers follows receiver identity
   rather than `find_type_name(...)`
3. name-keyed method tables remain only for builtins, module-qualified lookup,
   and compatibility surfaces

Exit criteria met.

### Phase 6: Delete The Hidden Selective Namespace

Files:

- [`boot/compiler/resolver.tw`](../../../boot/compiler/resolver.tw)
- targeted tests/docs

Status: completed on 2026-03-28.

Landed result:

1. selective imported types now register under exact canonical qualified names
2. hidden selective-import compatibility helpers are removed from resolver logic
3. tests/docs now assert the absence of hidden selective names directly

Exit criteria met.

## Risks

### Risk: Too Much Env Churn At Once

`ResolvedEnv` is widely used. Changing its binding model can create broad
mechanical fallout.

Mitigation:

- add the new binding layer alongside current structures first
- migrate APIs before deleting compatibility fields
- land each phase with dedicated focused tests

### Risk: Preferred Diagnostic Names Become Unstable

When multiple bindings point at the same imported type, diagnostics may become
harder to read if there is no stable preferred name.

Mitigation:

- keep explicit preferred-name metadata for diagnostics
- treat display names as presentation state, not semantic identity

### Risk: Support-Only Types Accidentally Leak Into Scope

Once support types can exist without ordinary bindings, bugs may accidentally
bind them into user scope during migration.

Mitigation:

- test both "support type available for checking" and "support type not visible
  as a local name"

### Risk: Half-Migrated Method Lookup Reintroduces Topology Bugs

If type binding separation lands before method registration is identity-based,
partial compatibility layers may hide gaps.

Mitigation:

- keep explicit regression tests for mixed full/selective/transitive import
  method calls during the migration

## Success Criteria

This plan is complete when all are true:

- support/interface imported types can exist in the env without synthetic local
  bindings
- full import and selective import share one registration path and differ only
  in bound names
- selective-imported values no longer require hidden origin names for linking
- method lookup follows receiver identity rather than surface-name accidents
- no active boot compiler import logic relies on a `$import$...` selective
  namespace
