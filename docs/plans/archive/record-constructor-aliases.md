# Record Constructor Alias Plan

## Goal

Allow named record constructors to accept type aliases that resolve to record
types, so `P.{ ... }` works when `type P = Point`.

## Current State

`P.{ ... }` is parsed successfully, but type checking fails when `P` is an
alias to a record type.

Root cause:

- `synth_record_lit` looks up `P` and gets `TypeId(P)` (an alias `TypeDef`).
- Record-field checking expects a `TypeDef::Record` at that same `TypeId`.
- Alias expansion is not applied in the named-record-constructor path.

This creates a behavior gap versus the existing "aliases are transparent"
semantics used elsewhere.

## Target Semantics

Given:

```tw
type Point = .{ x: Int, y: Int }
type P = Point
```

both forms should work and produce the same type:

```tw
a := Point.{ x: 1, y: 2 }
b := P.{ x: 1, y: 2 }
```

Additional expectations:

- Alias chains work: `type Q = P`, then `Q.{ ... }` works.
- Concrete aliases to generic records work:
  `type IntBox = Box<Int>`, then `IntBox.{ value: 1 }` works.
- Constructor names that do not resolve to record types still fail with a clear
  error.

## Non-Goals

- Introducing nominal newtype behavior for aliases.
- Changing alias syntax or parser rules.
- Adding full generic type-alias arguments (already restricted elsewhere).

## Design

### 1. Canonicalize record constructor target in type checker

In `src/types/check.rs` (`synth_record_lit`), when `name: Some(type_name)`:

1. Resolve `type_name` in `TypeEnv`.
2. Canonicalize through alias targets until reaching a concrete constructor
   shape:
   - Accept only final `MonoType::Named { type_id, args }` whose `type_id`
     points to `TypeDef::Record`.
   - Reject primitives/sums/functions/non-record targets.
3. Run existing field checking with canonical `(record_type_id, args)`.
4. Return canonical record type (`MonoType::Named { record_type_id, args }`),
   not the alias `TypeId`.

Why return canonical type:

- Keeps alias transparency consistent with the rest of type resolution.
- Avoids introducing pseudo-nominal behavior from constructor syntax.
- Prevents unification failures between `P.{ ... }` and expected `Point`.

### 2. Diagnostics for non-record constructor targets

Improve the current failure mode for `Alias.{ ... }` where alias target is not a
record (for example `type MyInt = Int`).

Preferred behavior:

- Emit a direct diagnostic that named record constructors require a record type.
- Include the resolved target type in the note.

Implementation options:

- Add a dedicated `TypeError` variant (preferred for stable diagnostics).
- Or emit `TypeMismatch` with a focused note if we want minimal enum churn.

### 3. No parser/lowering/runtime changes required

- Parser already captures constructor name paths (`Type.{ ... }`,
  `module.Type.{ ... }`).
- Lowering uses typed expression info; once type checking returns the canonical
  record type, lowering stays unchanged.
- Runtime/codegen behavior is unchanged.

## Implementation Tasks

### Task A: Add constructor-target canonicalization helper

- Add a helper in type checker (or `TypeEnv`) to resolve constructor names to:
  - canonical record `TypeId`
  - concrete type args (possibly empty)
  - optional display info for diagnostics

Must handle alias chains and concrete alias targets like `Box<Int>`.

### Task B: Update named record literal path

- Refactor `synth_record_lit` to use canonical target data.
- Ensure `check_record_lit_fields` receives canonical record type/args.
- Ensure the resulting expression type is canonical.

### Task C: Improve diagnostics

- Add/adjust error reporting for "constructor target is not a record".
- Keep source span on the constructor expression.

### Task D: Add typecheck pass tests

Add new tests under `tests/typecheck/pass/`:

- alias-to-record constructor:
  - `type Point = .{ ... }`
  - `type P = Point`
  - `p := P.{ ... }`
- alias-chain constructor (`Q = P`).
- concrete alias to generic record (`IntBox = Box<Int>`).
- module-qualified alias constructor (`mod.P.{ ... }` where exported alias
  resolves to a record).

### Task E: Add typecheck fail tests

Add tests under `tests/typecheck/fail/`:

- alias-to-primitive constructor (`type MyInt = Int`; `MyInt.{ ... }`).
- alias-to-sum constructor (`type S = Option<Int>`; `S.{ ... }`).

Assert message quality (clear "not a record constructor target" guidance).

### Task F: Add runtime regression coverage

Add a `tests/run/*.tw` case that prints values constructed via both
`Point.{ ... }` and `P.{ ... }` to confirm end-to-end parity.

## Validation

- `cargo test` passes with new pass/fail cases.
- Existing alias tests continue to pass.
- Existing record constructor tests continue to pass.
- No changes in runtime snapshots other than new fixture outputs.

## Rollout

1. Land type checker canonicalization + diagnostics.
2. Land tests (pass/fail/run).
3. Update docs:
   - `docs/spec.md` record constructor section: aliases to record types are
     valid constructor names.
   - `docs/design/records.md` add alias constructor example.

## Exit Criteria

- `P.{ ... }` works for aliases resolving to records.
- Constructor behavior is transparent with direct record constructors.
- Non-record alias constructors fail with clear diagnostics.
