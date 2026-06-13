# Prelude methods returning stdlib types

Status: spike validated; ready to implement (Path 2 — reserved stdlib type, forward-declaration reuse + origin pinning).

## Spike results (2026-06-13)

A throwaway spike (seed `View`, add `Vector.as_view<T>(xs) View<Vector<T>>` →
`view.from(xs)`, cross-check identity) proved the mechanism and pinned the exact
required pieces. All spike edits were reverted; findings:

1. **Identity reuse needs two things, not one.** Seeding a forward-declared
   `View` (`def == .None`) into the base env is *necessary but not sufficient*.
   The existing `unresolved_type_binding` reuse path (`resolver.tw:1334`) **does**
   fire — `@std.view`'s own `pub type View` reuses the seeded id. But when a
   module *imports* `@std.view`, `plan_export_type_ids` (`resolver.tw:2143`)
   matches the imported `View` to an existing entry only via
   `lookup_registered_type` or `origin_index[origin]`. The seeded entry had **no
   `type_origins` entry**, so the import allocated a *fresh, divergent* id → the
   bare-`View` signature (seeded id) and the `view.from` body (imported id)
   mismatched (`expected view.View, found View`, same name, different id). **Fix:
   pin `type_origins[reserved_id] = "@std.view::View"` at base-env construction.**
   With the origin pinned, the import matched the seeded id and the mismatch
   disappeared. The canonical origin string is `"@std.view::View"` (verified).

2. **The prelude→`@std.view` cycle is NOT a blocker.** `as_view` in the prelude
   creates `prelude/vector.tw` → `@std.view` → (auto) `prelude`. The spike never
   hit the preliminary-export path for `View` (`next_preliminary_type_id` /
   `preliminary_type_exports`), so existing cycle machinery handled it. This
   downgrades the risk the earlier draft flagged here.

3. **New required work: idempotent inherent-method registration.** Once `View`'s
   id is *shared* across import boundaries, `detect_inherent_methods`
   (`resolver.tw:1526`) registers `@std.view`'s methods on the same id from
   multiple sites (`@std.view` itself, the prelude's import, the user's import)
   and raises `duplicate method: is_empty on View`. This did not occur before
   because each import previously got a distinct `View` id. **This must be made
   idempotent/deduped for shared stdlib type ids — it is the main remaining
   implementation task, beyond seeding+origin.**

4. **Bootstrap reality: stage0 must seed too.** Because the prelude references the
   reserved type by bare name, whatever compiler compiles the prelude must seed
   `View`. The Rust stage0 (`src/types/env.rs`) does not, so it can't compile the
   new prelude directly. The spike worked around this with a two-phase bootstrap
   (build a seeding-capable stage1 with the seeding-but-no-prelude-method, then
   build the full compiler with it). The real implementation should **mirror the
   seeding in stage0** rather than rely on two-phase bootstrapping.

## Goal

Unlock prelude-backed methods whose public signatures mention stdlib-defined
nominal types, starting with:

```tw
Vector.chunks<T>(xs: Vector<T>, size: Int) Vector<View<Vector<T>>>
Vector.windows<T>(xs: Vector<T>, size: Int) Vector<View<Vector<T>>>
```

so that `xs.chunks(2)` / `xs.windows(2)` resolve as ordinary inherent `Vector`
methods returning zero-copy `@std.view.View` windows over the original vector.

The collection ergonomics pass left these out because `View` is defined in
`boot/stdlib/view.tw`, while prelude method registration is bootstrapped from
`boot/prelude/*.tw` signatures in an environment that only knows builtin types.

## Background: why this is hard

The blocker is **type identity**, not parsing. Two facts from the codebase:

1. **Builtin types have fixed ids seeded into every env.** `builtin_env()` →
   `builtin_type_entries()` (`boot/compiler/base_env.tw`) assigns `Option`=0 …
   `Set`/`Order`/`Task` fixed `TypeId`s and seeds them into the starting env of
   every module. The signature loader (`signatures.load_signatures`) resolves
   prelude method annotations against `builtin_type_env()`, which contains *only*
   those builtin names. A bare `View` (or `view.View`) is `Undefined type`
   there.

2. **A prelude method's signature is resolved once, at base-env construction.**
   `Vector.chunks`'s return `Vector<View<...>>` bakes in whatever `TypeId` `View`
   has at that moment. Regular analysis of `@std.view`
   (`boot/compiler/query/analyze.tw`) allocates type ids **sequentially after the
   builtins** (`next_preliminary_type_id` / `resolver.next_available_type_id`).
   Unless forced, those two `View` ids differ → the plan's "duplicate `View`
   identity" hazard → unification silently misbehaves.

   (`type_registry.tw` was built to make stdlib ids stable but is **not yet
   authoritative** — see the comment at `analyze.tw:1122`: "TypeIds still come
   from the resolver/checker's sequential allocation." We do not depend on it
   here.)

So `View` needs a single fixed id that is (a) known to the signature loader and
(b) reused — not reallocated — when `@std.view` is analyzed normally.

## Relation to other work

- `boot/compiler/builtin_refs.tw` + `docs/plans/boot-typed-builtin-type-refs.md`
  (commit `058e3d8`) established the convention we follow here: *source-level
  builtin/well-known type names live only at the bootstrap boundary, resolved by
  name from the `ResolvedEnv` at the use site, never by raw `TypeId.{ id: N }`.*
  That work explicitly scoped itself **out** of prelude signature loading and the
  stdlib-type question; this plan picks that up. It is a companion pattern, not a
  prerequisite.
- `Set` is the existing precedent for a generic container that is compiler-known
  with methods in prelude source, but `Set` has **no source type decl** (it is a
  pure builtin). We deliberately do *not* promote `View` to a builtin (that was
  the rejected "Path 1"): `View` must stay an importable `@std.view` type that
  *satisfies* the access contracts, which was the whole point of the
  access-contracts design.

## Key enabling mechanism (existing, reused)

`resolver.collect_declarations` (`boot/compiler/resolver.tw:1320`) already has an
**in-place reuse path** for forward-declared types. When it encounters
`.Type(decl)` and the env already binds `decl.name` with `def == .None`
(`unresolved_type_binding`, line 1334), it does **not** allocate a new id — it
lets the module fill that existing binding's definition. This is how
preliminary/cycle interfaces are reconciled.

Path 2 rides this: seed `View` as a **forward declaration** (`def == .None`) at a
fixed reserved id, and `@std.view`'s own `pub type View<C> = ...` fills the def
in place, keeping the id. No new id-reservation subsystem is required.

## Design

### 1. Reserved stdlib-type registry (single source of truth)

Add a small table at the bootstrap boundary (new section in
`boot/compiler/base_env.tw`, or a dedicated `reserved_stdlib_types.tw` that
`base_env` and the analyze guard both import). Each row:

```
{ name: "View", arity: 1, origin: "@std.view::View" }
```

The reserved `TypeId` is assigned by appending these rows to
`builtin_type_entries()` **after** the existing builtins, so they get the next
contiguous ids. (The exact number does not matter as long as it is fixed and
seeded before any user/stdlib type is allocated.) The entry is created with
`def: .None` and `is_extern: false`.

### 2. Seed forward declarations into both bootstrap envs (+ pin origin)

- `builtin_type_env()` — so the **signature loader** can resolve bare `View` in
  prelude annotations. Resolution only needs id + arity; `def == .None` is fine
  (the resolver already builds `MonoType.Named(id, args)` from forward-declared
  entries during cycle breaking).
- `builtin_env()` — so every module's starting env carries `View` at the reserved
  id. **Critically, also set `type_origins[reserved_id] = "@std.view::View"`**
  (spike finding #1). Without this, `plan_export_type_ids` cannot match a module's
  *import* of `@std.view::View` to the seeded entry and allocates a divergent id.
  The seeded entry has `def == .None`; the origin is what ties the import, the
  in-module definition, and the seeded signature reference to one id.

`View` must **not** be added to `is_reserved_type_name` (`resolver.tw:1263`) —
that would make `@std.view`'s own `pub type View` an error. Protection against
*other* modules shadowing it is handled by the origin guard below, not by the
reserved-name list.

### 2b. Make inherent-method registration idempotent for shared ids

Spike finding #3: with `View`'s id shared across import boundaries,
`detect_inherent_methods` (`resolver.tw:1526`) re-registers `@std.view`'s methods
on the same id from each import site and raises `duplicate method: …`. Before
shipping the prelude method, `detect_inherent_methods` (and/or the import-time
`register_imported_interface_types` path) must treat re-registration of an
identical `(type_id, method_name, function_name)` as a **no-op** rather than a
duplicate-definition error — scoped to reserved/shared stdlib type ids so genuine
user duplicate-method errors are preserved.

### 3. Reference `View` unqualified in the prelude signature

`view.View` is module-qualified and the signature loader does not process
imports, so the annotation must use the **bare** seeded name:

```tw
// boot/prelude/vector.tw
use @std.view   // needed for the function BODY (view.from), not the annotation

pub fn chunks<T>(xs: Vector<T>, size: Int) Vector<View<Vector<T>>> {
  view.from(xs).chunks(size)
}

pub fn windows<T>(xs: Vector<T>, size: Int) Vector<View<Vector<T>>> {
  view.from(xs).windows(size)
}
```

Bare `View` resolves to the reserved id in **both** passes:
- signature scan (against `builtin_type_env()` with the seeded `View`), and
- normal compilation of `prelude/vector.tw` (against `builtin_env()` + the
  `@std.view` import, whose `View` is the same reserved id after step 2's reuse).

`prelude/vector.tw` is compiled as a real module (bodies included) on top of the
signature scan, so the `use @std.view` import is required and legitimate.

### 4. Origin guard against accidental shadowing (soundness)

Because the reserved `View` binding is `def == .None` in the global base env, the
resolver reuse path would also fire for a *user* module that declares
`type View` — silently giving the user's `View` the reserved id and (worse)
filling the prelude-referenced id with the user's field layout. That is a real
soundness hole and must be guarded.

The guard belongs in the **analyze layer**, where the module's canonical path is
known (the resolver's `collect_declarations` does not currently receive it).
When a module defines a type whose name is in the reserved registry:

- if the module's canonical origin **equals** the registry `origin` → allowed
  (this is `@std.view` filling its own `View`);
- otherwise → emit a diagnostic (reuse `ReservedTypeName`, or a dedicated
  "cannot redefine reserved stdlib type `View`") and do **not** let the fill
  propagate.

Concretely, hook the check where local types are captured per module
(`capture_local_types`, `analyze.tw:1105`, which already has `canonical`) and/or
`preliminary_type_exports` (`analyze.tw:535`). The dependency-ordering edge — a
user `type View` analyzed *before* `@std.view` — is covered because the guard
fires on the offending module regardless of order, before its (wrong) def is
shared.

## Implementation steps

The spike already validated steps 1–2 and isolated the additional work (2b, and
stage0 parity). Order:

1. **Reserved registry + seeding + origin.** Add the registry and append `View`
   (`arity: 1`, `def: .None`) to the builtin type vector in `base_env.tw`; seed
   into both `builtin_type_env()` and `builtin_env()`. **Set
   `type_origins[reserved_id] = "@std.view::View"` in `builtin_env()`** (the
   spike's load-bearing fix). Keep names confined to this boundary
   (builtin_refs-style). Note `builtin_type_names` requires `def == .Some`, so the
   reserved entry/name must be concatenated *separately* from
   `builtin_type_entries()` / `builtin_type_names()`.
2. **Stage0 parity.** Mirror the seeding (and origin) in the Rust stage0
   (`src/types/env.rs`, alongside `Set`/`Order`/`Task`), so stage0 can compile the
   bare-`View` prelude method directly. Without this the prelude won't bootstrap
   except via the throwaway two-phase trick. Confirm stage0's import/method
   model needs the equivalent of the origin pin and the dedup.
3. **Idempotent method registration (step 2b).** Dedup identical method
   re-registration for the shared `View` id in `detect_inherent_methods` /
   `register_imported_interface_types`. This is the gating fix — without it the
   prelude fails with `duplicate method: … on View` for *every* program.
4. **Prelude method.** Add `chunks` / `windows` to `boot/prelude/vector.tw` (bare
   `View` return, `use @std.view` for bodies). `receiver_for_stem("vector")` /
   `to_internal_name` already route `Vector` methods — no signature-map change.
5. **Origin guard.** Add the reserved-stdlib shadowing check in the analyze layer
   (`capture_local_types` / `preliminary_type_exports`) so a non-`@std.view`
   module defining `type View` is rejected.
6. **Tests.** Boot tests under `boot/tests/suites/` covering: `xs.chunks(2)` /
   `xs.windows(2)` shapes and contents; invalid sizes → empty vector; a `View`
   produced by `chunks` is the same nominal type as one from `view.from`
   (identity unification); existing `api_view_suite` still green (no duplicate
   methods); and a negative test for the origin guard.
7. **Docs.** Add `Vector.chunks` / `Vector.windows` to `docs/API.md`; note the
   reserved-stdlib-type mechanism in the design notes if appropriate.
8. **Rebuild + verify** (below).

## Touch points (file:line)

- `boot/compiler/base_env.tw` — `builtin_type_env` (150), `builtin_env` (241):
  add reserved registry + seeding + **`type_origins` pin** (spike-confirmed
  essential).
- `boot/compiler/resolver.tw` — reuse path `collect_declarations` (1334) and
  `unresolved_type_binding` (1311): relied upon, fire correctly as-is.
  `plan_export_type_ids` (2143): the import-merge matcher whose `origin_index`
  lookup makes the origin pin necessary (no code change, but this is *why*).
  `detect_inherent_methods` (1526): **dedup identical method re-registration**
  for shared ids (step 3). `is_reserved_type_name` (1263): leave `View` out.
- `boot/compiler/query/analyze.tw` — `capture_local_types` (1105),
  `preliminary_type_exports` (535): origin guard (step 5).
- `src/types/env.rs` — mirror the seeding + origin alongside `Set`/`Order`/`Task`
  (step 2); check the Rust import/method-registration path for the same dedup.
- `boot/prelude/vector.tw` — add `chunks` / `windows`.
- `boot/compiler/signatures.tw` — `receiver_for_stem` (27), `to_internal_name`
  (54): `Vector` routing already present (spike-confirmed no change).
- `boot/tests/suites/api_view_suite.tw` (or a vector suite) — tests.

## Acceptance criteria

- `Vector.chunks` and `Vector.windows` resolve as inherent vector methods.
- Their return type is `Vector<View<Vector<T>>>`, sharing the original vector via
  `View` windows; invalid sizes return an empty vector.
- `View` has the **same nominal identity** whether referenced from a prelude
  method result or imported directly from `@std.view` (proven by a unification
  test, not by inspection).
- A non-`@std.view` module defining `type View` is rejected (origin guard).
- `make stage2` reaches a **deterministic fixed point** and
  `target/twk run boot/tests/main.tw` passes.

## Expectations / caveats

- **Not byte-identical to the current self-host.** Unlike the `builtin_refs`
  work (which preserved ids), seeding `View` shifts every dynamically-allocated
  user `TypeId` by the number of newly reserved entries. The required signal is a
  deterministic fixed point (stage2 == stage3), **not** byte-identity with the
  pre-change compiler.
- **The remaining risk is method-registration dedup (step 3), not identity.** The
  spike confirmed identity unifies with seeding + origin pin; the cycle is handled
  by existing machinery. The open question is scoping the dedup so it suppresses
  *only* re-registration of the same `(type_id, method_name, function_name)` for
  shared stdlib ids, without masking genuine user duplicate-method errors. If the
  dedup proves hard to scope cleanly, reassess against promoting `View` to a
  builtin (Path 1), which sidesteps both import-merge and multi-site method
  registration.
- Keep all source-level reserved names confined to the bootstrap boundary, per
  the `builtin_refs.tw` convention.
```
