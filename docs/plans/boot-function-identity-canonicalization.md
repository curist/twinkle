# Boot Function Identity Canonicalization Plan

## Status

Steps 1–3 implemented. Step 4 (linker hardening) pending.

## Problem

The boot compiler’s multi-module pipeline still treats imported function
identity as something that can be reconstructed from whichever spelling happens
to be visible at each stage.

That is fragile because one semantic function may appear under several names:

- direct import names
- module-qualified names
- selective-import local bindings
- prelude-visible names
- prelude internal receiver-prefixed names such as `vector_filter`
- method-resolution names derived from the first visible module path

The current failure mode follows that shape:

- lowering can allocate or reuse a `FuncId` under one visible spelling
- `module_compiler` can record external origin data under another spelling
- linker metadata is then reconstructed with name matching plus
  `env.function_bindings` fallbacks
- if one alias spelling was not registered, a phantom imported `FuncId` can be
  left unresolved and later collide or remap incorrectly during linking

This is not just a missing-case problem. The boot compiler currently lacks one
explicit, canonical notion of imported function identity.

## Goal

Make cross-module linking depend on one canonical imported-function identity
model rather than on best-effort string reconciliation.

Desired end state:

- every imported or exported function has one canonical link identity
- all visible aliases point to that identity
- module alias spelling differences stop mattering once resolution reaches a
  registered target and canonical origin
- lowering reuses one imported phantom `FuncId` per canonical external symbol
- linker metadata is built from canonical identities, not exact string-name
  coincidence
- prelude internal names, direct imports, selective imports, and method-call
  spellings all converge on the same link target

## Non-Goals

This plan does not propose:

- redesigning Twinkle’s user-visible import syntax
- removing method sugar or prelude internal helper names from the resolver
- changing diagnostic naming policy as part of this fix
- a one-shot rewrite of the whole boot compiler pipeline

## Current Code Reality

The current boot implementation spreads function identity across several
structures.

### Resolver and import projection

In [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw):

- `functions: Vector<FunctionSig>` stores registered function signatures
- `func_index: Dict<String, Int>` indexes registered function names
- `function_bindings: Dict<String, String>` maps visible names to registered
  target names
- `merge_module_exports(...)` registers qualified import names
- `merge_selective_imports(...)` binds local aliases to registered qualified
  names
- `merge_prelude_exports(...)` binds visible prelude names and receiver-
  prefixed internal names like `vector_filter`

This already gives the resolver a partial canonicalization layer, but not an
explicit imported-origin model.

### Export extraction

Also in [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw):

- `ExportedFunction` currently carries:
  - `name`: importing-facing export name
  - `source_name`: the function’s defining-module name

That distinction is useful, but it still does not carry full imported-origin
metadata across re-exports.

The implementation will likely need explicit export-side origin carriage as
well, for example by extending `ExportedFunction` with canonical origin metadata
or an equivalent mechanism. Otherwise the resolver cannot reliably preserve the
original defining module across export extraction and later import projection.

### Lowering

In [`boot/compiler/lower_core.tw`](../../boot/compiler/lower_core.tw):

- `func_table: Dict<String, FuncId>` maps visible spellings to `FuncId`
- builtins seed that table directly
- imported aliases are later rebound to canonical target ids via
  `env.function_bindings`
- recent fixes avoid allocating fresh phantom ids for some alias-only names

This helped, but imported identity is still effectively mediated by names.

### Module compiler linker metadata

In [`boot/compiler/module_compiler.tw`](../../boot/compiler/module_compiler.tw):

- `track_func_origins(...)` reconstructs name → `ExternalRef`
- `build_external_refs(...)` scans phantom `FuncId`s and tries to match them
  back to those names
- if direct matching fails, it falls back through `env.function_bindings`
- prelude alias naming is duplicated here to repair linker metadata

This is reconstructive rather than authoritative.

### Core linker

In [`boot/compiler/core_linker.tw`](../../boot/compiler/core_linker.tw):

- imported remapping already uses `ExternalRef { module_path, func_name }`
- this part is conceptually sound
- the fragility happens earlier, when the compiler tries to discover which
  phantom imported `FuncId` should point at which `ExternalRef`

## Diagnosis: Three Identity Layers

The boot compiler currently mixes three identity layers.

### Layer 1: visible names

Examples:

- `filter`
- `vector_filter`
- `command.add_required_positional`
- `app.command.add_required_positional`
- local selective-import aliases

These are source-facing spellings.

### Layer 2: registered target names

Examples:

- names stored in `env.functions`
- names that `env.function_bindings` points to
- names chosen during import projection into the current env

These are more canonical than visible spellings, but still import-context-
dependent.

### Layer 3: defining-site link identities

Examples:

- `(module_path = prelude/vector.tw, func_name = filter)`
- `(module_path = app/command.tw, func_name = add_required_positional)`

This is the real cross-module identity needed by linking.

Today the compiler mostly stores layer 1 and layer 2 directly, then tries to
reconstruct layer 3 later. That is why new alias shapes keep reopening the same
bug class.

## Chosen Data Model

### Canonical imported origin lives in `ResolvedEnv`

Extend `ResolvedEnv` with imported-function origin metadata, conceptually:

```text
function_origins: Dict<String, ExternalRef>
```

Meaning:

- key: registered target name in `env.functions`
- value: canonical defining-site symbol for imported functions only

This keeps resolver authoritative for both:

- visible name → registered target (`function_bindings`)
- registered target → defining-site origin (`function_origins`)

Canonical imported origins must use the same canonical module-path form already
used by import planning, `CompiledModule.path`, and linker lookups. Alias
canonicalization is not complete if path spelling can still diverge.

Design rule:

- `function_bindings` answers: “what registered function does this visible name
  mean?”
- `function_origins` answers: “if that registered function is imported, where is
  it actually defined?”

Local functions and builtins should have no `function_origins` entry.

### Module alias canonicalization is included, but only for function identity

Yes: the plan does address module alias canonicalization **for linker-relevant
function identity**.

For example, these should converge once resolved:

- `command.add_required_positional`
- `app.command.add_required_positional`
- a selective local alias of the same function

They may still differ as source-level spellings, but once they bind to the same
registered target and canonical imported origin they should become
indistinguishable to lowering and linking.

What this plan does **not** attempt is a broader rewrite of all module alias
canonicalization behavior across the compiler. It only canonicalizes module alias
variation insofar as it affects imported function identity.

### Re-exports preserve original origin

Canonical imported origin must point at the original defining module, not merely
the immediate module that re-exported the function.

If:

- module `A` defines `foo`
- module `B` re-exports `foo`
- module `C` imports `foo` from `B`

then `C` should still see canonical origin `(A, foo)`, not `(B, foo)`.

Operationally:

- exported function metadata should preserve any existing imported origin from
  the exporting module’s env
- only functions truly local to that module should default their origin to the
  current module path
- the same rule must apply to support/export-helper functions, not only visible
  exports, wherever those functions can participate in cross-module references

### Monomorphization is downstream of this plan

This plan only covers pre-monomorphization cross-module linking.

Current pipeline order is:

- multi-module compile
- core linking
- monomorphization

So canonical imported identity at this stage only needs:

- `module_path`
- `func_name`

Specialized monomorphized copies are out of scope because they do not yet exist
when phantom imported `FuncId`s are allocated and linked.

### Lowering may use a private cache key, but not as the semantic model

Lowering will likely need a cache to deduplicate imported phantom `FuncId`s.
That cache may use a local serialized key if Twinkle dict ergonomics make that
simplest.

Important constraint:

- the semantic identity remains `ExternalRef { module_path, func_name }`
- any string serialization used in lowering is only a private implementation
  detail for cache lookup
- it must not become a second authoritative identity layer

## Implementation Steps

### Step 1: Add canonical imported origins in the resolver

Files:

- [`boot/compiler/resolver.tw`](../../boot/compiler/resolver.tw)

Changes:

- extend `ResolvedEnv` with `function_origins`
- add helper APIs for reading and writing imported origins
- update import projection paths so imported functions preserve canonical origin:
  - `merge_module_exports(...)`
  - `merge_selective_imports(...)`
  - `merge_prelude_exports(...)`
- ensure export extraction carries canonical origin metadata forward for both
  visible exports and support functions
- keep `function_bindings` as a binding map only
- ensure re-exported functions preserve original defining origin rather than
  resetting to the immediate dependency module
- define shadowing behavior explicitly: when a local declaration replaces an
  imported/prelude registered target, stale imported origin metadata must be
  removed or overwritten
- ensure method-table entries continue to point at registered function names
  that participate in the same canonical-origin model

Acceptance criteria:

- imported registered targets have one canonical `ExternalRef`
- visible aliases bind through `function_bindings` without needing duplicate
  origin entries
- prelude visible names and receiver-prefixed aliases converge on the same
  imported origin
- module alias spelling differences converge through resolver metadata into one
  canonical imported origin
- local functions and builtins do not acquire external origins
- shadowing a prelude/imported function does not leave stale imported-origin
  metadata behind
- re-exports preserve the original defining module path
- support functions use the same origin-propagation rules wherever they can be
  linked across modules
- method-resolution names still resolve through registered targets carrying the
  correct imported origin

### Step 2: Make lowering deduplicate phantom imported `FuncId`s by origin

Files:

- [`boot/compiler/lower_core.tw`](../../boot/compiler/lower_core.tw)
- relevant lowering tests

Changes:

- add lowering-side imported-symbol cache keyed by canonical imported origin
  semantics
- consult resolver origin metadata when allocating imported phantom `FuncId`s
- reuse one phantom imported `FuncId` per canonical external symbol per module
- expose imported-symbol ownership in `LowerResult`, conceptually:

```text
imported_func_symbols: Dict<Int, ExternalRef>
```

- land characterization tests in the same change so CI never spends a commit in
  a knowingly failing state

Acceptance criteria:

- `filter` and `vector_filter` reuse one imported `FuncId` when they refer to
  the same prelude function
- aliased qualified spellings for one external function reuse one imported
  `FuncId`
- local aliases plus qualified names converge on the same imported id when they
  share canonical origin
- host builtins and local functions still bypass the imported-symbol path
- lowering result exposes enough metadata for downstream linker wiring without
  reconstructive name matching

### Step 3: Remove module-compiler reconstruction of imported identity

Files:

- [`boot/compiler/module_compiler.tw`](../../boot/compiler/module_compiler.tw)

Changes:

- build `CompiledModule.external_refs` directly from lowering-owned imported
  symbol metadata
- remove or collapse reconstructive helpers such as:
  - `track_func_origins(...)`
  - `build_external_refs(...)`
- remove duplicated prelude naming logic used only to repair linker metadata

Acceptance criteria:

- `module_compiler.tw` no longer infers imported identity from name matching
- `CompiledModule.external_refs` is a direct projection of lowering metadata
- no private prelude alias canonicalization remains here for linker purposes

### Step 4: Harden linker invariants

Files:

- [`boot/compiler/core_linker.tw`](../../boot/compiler/core_linker.tw)

Changes:

- fail loudly on unresolved phantom imported `FuncId`s
- treat missing imported `ExternalRef`s as invariant violations
- keep linker remapping keyed by `(module_path, func_name)`

Acceptance criteria:

- imported phantom ids cannot silently pass through without external refs
- linker errors identify the broken module/imported id precisely
- post-link collisions from missing imported remaps are no longer possible

## Test Plan

The structural fix needs tests that assert identity convergence, not only end
behavior.

### Resolver tests

Add coverage showing that these visible spellings share a canonical external
origin when appropriate:

- direct full import qualified name
- selective import local binding
- method-desugared path that resolves through a module-qualified function
- prelude visible name and prelude internal helper name
- nested alias path such as `app.command.add_required_positional` versus the
  direct-import path used by the actual export owner
- re-exported imports preserving original defining origin
- canonical module-path normalization for imported origins
- local shadowing clearing stale imported-origin metadata
- support-function origin propagation where applicable

### Lowering tests

Add focused tests for `lower_core.lower_module(...)` ensuring:

- imported aliases that target the same canonical symbol reuse one `FuncId`
- prelude helper aliases such as `filter` / `vector_filter` reuse one imported
  `FuncId`
- host builtin aliases keep reusing builtin ids rather than allocating phantom
  ids

### Module compiler / linker tests

Add end-to-end boot tests covering:

- cross-module method calls through re-exported or first-seen module paths
- prelude-backed method calls like `xs.filter(...)`
- selective imports plus qualified imports referring to the same dependency
- a case where several alias spellings appear in one module and still link to
  one external target

## Risks

### Re-export origin propagation

If canonical origin propagation is implemented incorrectly, imported functions
can be rebound to the immediate re-exporting module rather than the original
defining module.

Guardrail:

- export extraction and import projection must preserve existing imported origin
  metadata whenever present

### Path canonicalization drift

If `ExternalRef.module_path` is not stored in the same canonical path form used
by import planning and linker lookup, alias convergence can still fail even when
name canonicalization is correct.

Guardrail:

- imported origins must always store canonical module paths

### User shadowing of prelude functions

Resolver registration already allows user functions to shadow prelude names.
Origin metadata must follow the surviving registered target, not a stale prelude
entry.

### Duplicated authority

If resolver and module compiler both remain authoritative for imported identity,
the refactor will not remove the current architectural seam.

Guardrail:

- resolver owns imported origin semantics
- lowering owns imported phantom-id allocation
- module compiler only forwards lowering metadata

### Overuse of serialized cache keys

A serialized `ExternalRef` key may be acceptable as a local lowering cache
implementation detail, but it must not become a public or semantic identity
layer.

## Transitional Delivery

This should land as a small sequence, not a large rewrite.

### Land together

- Step 1 and Step 2 are the main fix
- characterization tests should land with Step 2, not in a separate knowingly-
  failing change

### Land next

- Step 3 removes the old reconstruction path once the new path is proven
- Step 4 hardens invariants after the metadata flow is simplified
- after Step 2, do not add new name-matching fallbacks in `module_compiler.tw`
  for future cases; fix missing canonical-origin propagation instead

## Audit Checklist

Do not consider this plan implemented until all boxes are true in code review:

- [ ] `ResolvedEnv` distinguishes visible function bindings from canonical
      imported origins
- [ ] module alias spelling differences converge through resolver metadata into
      one canonical imported origin
- [ ] prelude helper aliases converge through resolver metadata rather than
      duplicate naming logic in `module_compiler.tw`
- [ ] canonical imported origins use canonical module-path spelling compatible
      with linker lookups
- [ ] lowering reuses one phantom imported `FuncId` per canonical symbol per
      module
- [ ] `LowerResult` exposes imported-symbol metadata directly
- [ ] `CompiledModule.external_refs` is built from lowering metadata, not from
      string reconciliation
- [ ] unresolved phantom imported ids fail as invariant violations
- [ ] regression tests cover prelude aliases, selective imports, qualified
      imports, re-exports, support-function cases where applicable, path
      normalization, and cross-module method resolution through aliased paths
