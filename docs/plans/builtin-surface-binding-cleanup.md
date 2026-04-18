# Builtin Surface Binding Cleanup Plan

Last updated: 2026-04-18

## Goal

Refactor the boot builtin environment so public and internal callable names are
separated by construction rather than by post-hoc cleanup.

Target state:

- only bind canonical/public names into the user-visible free namespace when
  that is actually intended,
- keep internal helpers explicitly internal (`__...`) or entirely unbound from
  the user-visible namespace,
- remove the current bind-then-hide cleanup step from
  `boot/compiler/base_env.tw`.

This plan is specifically about boot builtin namespace construction. It is not a
full builtin-registry redesign.

---

## Why This Plan Exists

Recent boot changes fixed an immediate user-facing bug by removing internal
builtin names such as `bool_to_string` from the visible free-function namespace.
That fix works, but it currently does so by:

1. building a broad function binding table,
2. registering methods and internal helpers,
3. explicitly removing bindings that should never have been user-visible.

That is a containment fix, not the desired architecture.

Stage0 points toward a better model:

- public surface names are canonical (`Bool.to_string`, `String.len`,
  `Dict.new`),
- internal helpers are explicitly internal (`__host_read_file`,
  `__vector_builder_new`, etc.),
- legacy aliases are isolated as compatibility machinery rather than treated as
  the main surface model.

Boot should move toward that shape.

---

## Current Baseline

Today boot has three relevant pieces:

- `boot/compiler/signatures.tw`
  - derives internal names from signature groups, for example
    `Bool.to_string -> bool_to_string`
- `boot/compiler/base_env.tw`
  - loads builtin/prelude signatures into the environment,
  - registers builtin methods,
  - currently removes some names afterward via explicit hiding helpers
- `boot/compiler/builtins.tw`
  - owns builtin execution identity (`FuncId`), ABI, runtime/intrinsic
    classification, and canonical-name mappings

Current problems:

### B1 — User-visible binding names are not modeled explicitly

`with_functions(...)` seeds free bindings for all loaded function signatures when
starting from an empty function-binding map. That means many internal execution
names become visible automatically unless they are removed later.

### B2 — Signature-derived internal names leak into the free namespace

Receiver-based signature groups currently derive internal names like:

- `bool_to_string`
- `string_len`
- `dict_new`
- `vector_append`

Those names are useful as internal execution identities, but they are not the
intended user-facing surface.

### B3 — Internal helpers are inconsistent in spelling and visibility

Some helpers are clearly internal already (`__host_read_file` in stage0,
optimizer-only helpers, builder helpers), while boot still exposes or registers
some equivalent helpers under plain names like:

- `host_read_file`
- `string_substring`
- `vector_builder_new`

Some of these should be explicitly internal; others should simply be unbound
from user code.

### B4 — Bind-then-hide obscures intent

The current cleanup step works, but it makes namespace policy hard to reason
about because visibility is determined by subtraction rather than by
construction.

---

## Desired End State

### Public/user-visible free names

Only these should be free names in boot when intended by the language surface:

- true free builtins such as `print`, `println`, `error`, `eprint`, `eprintln`
- genuine free functions such as `range`, `range_from`, `range_step`
- any future public free builtin explicitly designated as part of the language
  surface

### Public/user-visible method and qualified names

Method-backed builtin surface should remain reachable through:

- `x.to_string()`
- `Bool.to_string(true)`
- `String.len(s)` if module-qualified/canonical form is supported by the checker
- `Dict.new()`
- `Cell.new()`
- `Iterator.unfold(...)`

The important point is that the surface should be canonical, not raw internal
execution names.

### Internal names

Internal callable identities should remain available to compiler internals via:

- builtin registry lookup,
- method-entry targets,
- lowering/codegen/optimizer references,
- imported-function identity/origin tracking where needed.

But they should **not** automatically become user-visible free functions.

---

## Non-Goals

This plan does not attempt to:

- change builtin `FuncId` allocation policy,
- redesign the whole builtin registry,
- remove all legacy alias support from stage0,
- change runtime ABI naming,
- decide every canonical-vs-internal naming detail for stage0 and boot in one
  pass.

---

## Design Direction

The core change is to separate two concepts that are currently coupled:

1. **registered callable identity**
2. **user-visible free binding**

A builtin can be registered and callable internally without being bound as a
free user name.

That means boot environment construction should answer three separate questions:

- What callables exist?
- What methods point to those callables?
- Which names are actually bound in the free user namespace?

Today these answers are partially conflated.

---

## Implementation Plan

### P1 — Inventory builtin names by visibility class

Create a small classification table for current boot builtins/signature-backed
functions:

#### Class A — public free names

Examples:

- `print`
- `println`
- `error`
- `eprint`
- `eprintln`
- `range`
- `range_from`
- `range_step`

#### Class B — public method/qualified surface only

Examples:

- `Bool.to_string`
- `Int.to_string`
- `Float.to_string`
- `String.len`
- `Dict.new`
- `Vector.append`
- `Cell.new`
- `Iterator.unfold`

These need registered callable identities, but should not have raw internal free
bindings like `bool_to_string` or `string_len`.

#### Class C — internal-only helpers

Examples:

- vector builder helpers,
- host helpers,
- optimizer/in-place helpers,
- bridge helpers,
- internal-only runtime shims.

For each class, write down:

- visible free name (if any),
- canonical public name (if any),
- internal execution name,
- whether it should exist in signature files, `builtins.tw`, or both.

Deliverable:

- either embed the classification directly in this plan or add a nearby table in
  implementation comments before refactoring.

### P2 — Stop using `with_functions(...)` as an implicit visibility policy

Refactor boot builtin environment construction so it no longer relies on the
initial empty `function_bindings` case in `with_functions(...)` to decide what
becomes visible.

Possible directions:

1. build the registered function table first without populating user bindings,
2. add an explicit helper for binding only selected free names,
3. split `with_functions(...)` into:
   - register all callable signatures,
   - bind selected public names.

The plan does not require a specific API shape, but the resulting code should
make user-visible binding policy explicit.

### P3 — Make method-backed builtin signatures public only through canonical names

For signature-backed receiver groups, keep the callable identities needed by boot
internals, but expose them to users through the method/qualified surface only.

This likely means one of:

- store canonical names as the public binding names while keeping internal names
  as registry identities,
- or bind no free name at all for method-backed builtins and rely entirely on
  method resolution plus canonical imported names.

Important invariant:

- `Bool.to_string` works,
- `true.to_string()` works,
- `bool_to_string(true)` does not.

### P4 — Make internal helpers explicitly internal

Normalize internal-only helper naming so boot reflects the intended boundary more
clearly.

Candidates include:

- `host_*` helpers
- `vector_builder_*`
- in-place optimizer targets
- bridge/runtime-only helper functions

Preferred rule:

- if a helper is not part of the user language surface, either:
  - name it explicitly internal (`__...`), or
  - keep it unbound from the user namespace entirely.

This plan does not require renaming every helper immediately, but visibility and
intent should become unambiguous.

### P5 — Remove bind-then-hide cleanup from `builtin_env()`

Once P2–P4 are in place, delete the explicit post-processing steps that remove
bindings after the fact.

Desired result:

- `builtin_env()` constructs the correct namespace directly,
- there is no cleanup pass like `hide_bound_method_targets(...)` or
  `hide_internal_free_targets(...)`.

### P6 — Add guardrail tests around namespace policy

Add tests proving both positive and negative visibility rules.

#### Positive

- `Bool.to_string(true)` resolves
- `true.to_string()` resolves
- `Int.to_string(42)` resolves
- `Dict.new()` resolves
- `range_from(1, 10)` resolves as a free function

#### Negative

- `bool_to_string(true)` is undefined
- `string_len("x")` is undefined
- `dict_new()` is undefined
- internal vector builder helpers are undefined
- internal host helpers are undefined

Also preserve internal lookup coverage so compiler-facing identity is not lost:

- builtin registry still contains the internal entries,
- method resolution still targets the intended internal callable identity,
- lowering/tests that inspect builtin ids continue to work.

---

## Suggested Refactoring Shape

One likely clean direction is:

- keep `builtins.tw` as the execution/identity registry,
- keep signature files as the source of public callable signatures,
- add explicit boot environment helpers along the lines of:
  - register callable signatures,
  - bind free public names,
  - register methods,
  - register origins.

In other words, make namespace construction an explicit sequence rather than an
emergent side effect of `with_functions(...)`.

---

## Risks

### R1 — Breaking compiler-internal lookups

If the refactor accidentally removes registered callable identities instead of
just removing free bindings, lowering/codegen/optimizer paths may break.

Mitigation:

- keep tests that verify builtin registry ids and method-entry targets,
- distinguish clearly between registration and free binding in code.

### R2 — Drift between boot and stage0 naming policy

Boot may move in a cleaner direction but still differ from stage0 in temporary
ways.

Mitigation:

- use stage0 as the direction of travel,
- document any intentionally temporary boot-only compatibility behavior.

### R3 — Canonical names and internal names may remain partially duplicated

Some duplication may persist if signatures, methods, and registry all encode the
same surface in different forms.

Mitigation:

- keep this plan focused on namespace policy first,
- leave full registry/signature unification to separate plans if needed.

---

## Exit Criteria

This plan is complete when:

- boot no longer relies on bind-then-hide cleanup in `builtin_env()`,
- user-visible free names are explicitly chosen rather than implicitly seeded,
- method-backed builtins are reachable through canonical surface forms but not
  through raw internal free names,
- internal helpers are either explicitly internal (`__...`) or unbound from the
  user namespace,
- guardrail tests clearly lock down both allowed and forbidden builtin names.
