# Method Resolution Spec-Alignment Plan

Last updated: 2026-03-21

## Goal

Align compiler method resolution with the language spec:

- dot syntax resolves record fields first, then inherent methods,
- inherent methods come from the type's defining module (plus curated builtins),
- arbitrary "extension-method-by-first-parameter" behavior is removed.

---

## Drift Summary

Current implementation registers methods broadly from function signatures where
the first parameter is a receiver type. In practice this allows calls like:

```tw
fn zap(n: Int) Int { n + 1 }
x := 1
x.zap()   // currently accepted
```

This is broader than spec intent and makes method availability import-sensitive.

Spec reference:

- `docs/spec.md` ("dot syntax only resolves record fields and inherent methods from the defining module")

---

## Why Fix

Current behavior has concrete downsides:

1. Import-order/non-local effects on method availability.
2. Silent method replacement risk for `(receiver_type, method_name)` collisions.
3. Surprise for users because docs and behavior disagree.
4. Semver/API fragility: adding helper functions can accidentally add methods.

---

## Target Semantics

### User-defined nominal types (`Named`)

A function `fn m(x: T, ...)` is an inherent method only when:

1. `T` is defined in module `M`,
2. function `m` is defined in module `M`.

This should hold both inside `M` and from importing modules.

### Builtin receiver types (`Int`, `Float`, `Bool`, `Byte`, `String`, `Vector`, `Dict`, etc.)

Only curated builtin/prelude method registrations are allowed.
User modules must not create new dot methods on builtin types.

### Non-goals

- No trait/typeclass extension system in this plan.
- No syntax changes.
- No change to field-vs-method precedence rules.

---

## Implementation Plan

## Phase 1 — Freeze Behavior with Tests (Red/Green)

Add tests that encode desired semantics:

1. Allowed:
   - type-defined-in-module method works in same module.
   - same method works cross-module after import.
2. Rejected:
   - arbitrary extension-style methods on primitives (`Int`, `String`, etc.).
   - arbitrary extension-style methods on named types from non-defining modules.
3. Stability:
   - conflicting method names from unrelated imports do not affect method lookup.

Update both stage0 Rust tests and boot tests (where applicable) to prevent
future drift.

## Phase 2 — Method Registration Model in Stage0

Replace implicit first-parameter auto-registration in import plumbing with
explicit method registration sourced from module ownership.

### 2.1 Track/export method metadata per module

Extend module export data to include explicit method entries:

- receiver type id
- method name
- resolved function name
- ownership/source kind (builtin vs module-defined)

This avoids re-deriving method-ness at import sites.

### 2.2 Restrict registration at module definition time

When compiling module `M`:

- register methods for named types only if receiver type is defined in `M`,
- register builtin methods only from curated builtin/prelude sources.

### 2.3 Import path uses exported method table only

During `register_module_exports`, register methods from explicit exported method
entries, not by "first parameter" inference.

### 2.4 Diagnostics

Improve error text for previously-accepted extension patterns:

- "type X has no method 'm'" plus hint:
  - use `m(x, ...)` function call, or
  - move method definition to type-defining module.

## Phase 3 — Boot Compiler Alignment

Mirror the same rule in boot resolver/checker:

- keep builtin registrations explicit,
- ensure user-defined method detection matches defining-module semantics.

Even if boot is currently less multi-module-complete, lock rule parity now to
avoid reintroducing drift during self-hosting.

## Phase 4 — Docs + Examples Sync

Update:

- `docs/spec.md` method section wording (if any ambiguity remains),
- API docs/examples that currently imply extension-style behavior,
- migration notes for users with extension-style code.

---

## Affected Areas (Expected)

Stage0:

- `src/module/mod.rs`
- `src/module/context.rs`
- module export types (where `ModuleExports` is defined)
- `src/types/resolve.rs`
- `src/types/env.rs` (method table + helpers)
- method lookup/call diagnostics in `src/types/check.rs`

Boot:

- `boot/compiler/resolver.tw`
- `boot/compiler/checker.tw`

Tests:

- `tests/typecheck_*`, `tests/modules_*`, and/or new dedicated method-resolution tests
- boot suites under `boot/tests/suites/`

---

## Compatibility / Migration

Breaking change for code relying on implicit extension-style dot methods.

Migration patterns:

1. `x.foo(y)` -> `foo(x, y)` when `foo` is just a helper.
2. Move `foo` into the receiver type's defining module if dot syntax is desired.
3. For primitive helpers, keep module-qualified or free-function style.

Optional rollout approach:

- one release cycle with warning + suggestion,
- then enforce as hard error.

---

## Exit Criteria

This plan is complete when:

1. Stage0 and boot both enforce defining-module inherent method semantics.
2. No import-order sensitivity remains for method availability.
3. Extension-style dot calls are rejected with actionable diagnostics.
4. Docs/spec/tests all match implemented behavior.
