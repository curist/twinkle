# Boot first-class builtin function support

## Context

The boot backend currently handles three closely related cases with different
levels of maturity:

- direct calls to user functions
- direct calls to builtins / prelude functions
- first-class user functions flowing through closure-typed boundaries

The missing piece is:

- first-class builtin / prelude functions flowing through closure-typed boundaries

This shows up in examples like:

```tw
xs.map(Int.to_string)
Iterator.unfold(0, step)
c.update(inc)
```

where the function value being passed is a global `FuncId`, but not a user
function with a prepared body.

A fresh self-hosted boot compiler currently fails on this class of input with:

```text
lookup_func_sym: unknown FuncId 5
```

where `FuncId 5` is `int_to_string`.

That failure is only the most visible symptom. The deeper issue is that the boot
backend still treats closure-capable callables as if they were synonymous with
prepared user functions.

## Problem statement

The boot backend has a split callable model:

- direct call lowering knows how to distinguish user functions from builtins
- closure lowering and trampoline emission are still primarily user-function
  oriented
- planner and verifier contain partial higher-order special handling, but there
  is no single backend-level notion of a builtin function as a first-class
  callable target

This creates architectural drift:

- some passes know that a builtin `FuncId` is callable
- some passes know that a user `FuncId` can be wrapped as a closure
- no pass owns the general rule for when a global function value should be
  materialized as a closure and what trampoline family must exist for it

As a result, builtin function values can survive planning and verification but
still crash in emission when closure construction assumes the target has a user
function symbol or a prepared body.

## Why stage0 matters

Stage0 already handles this class of input correctly, but the goal of this plan
is not to copy stage0 literally.

What stage0 does provide is a useful behavioral reference:

- builtin / prelude function references are recognized as first-class values
- dedicated closure trampolines are emitted for those references
- closure construction does not assume every callable target has a user body
  symbol

Boot has a different and, in several respects, cleaner backend structure:

- explicit closure conversion
- prepared backend IR
- slot assignment and repr assignment
- backend verification
- Wasm planning as a separate phase

The plan should preserve those structural advantages while recovering the
missing callable coverage.

## Goal

Make builtin / prelude functions first-class values in the boot backend with the
same semantic reliability currently expected for user functions.

In concrete terms:

- passing a builtin function where a closure value is expected must work
- storing or returning builtin function values must use a defined backend path
- planning, verification, and emission must agree on which `FuncId`s are
  closure-capable and what support code they require
- closure construction must no longer rely on user-only symbol maps for builtin
  targets

## Non-goals

This plan does not aim to:

- redesign the surface function type system
- replace prepared backend IR with a stage0-style architecture
- require typed builtin closures as the initial solution
- unify all builtin and intrinsic lowering in one patch
- optimize closure representation before correctness is restored

Correctness and explicit ownership come first.

## Current symptoms

### 1. Closure construction assumes a user function symbol

`boot/compiler/codegen/emit.tw` currently routes `emit_make_closure(...)`
through user-oriented machinery and performs a user symbol lookup even when the
callee is a builtin `FuncId`.

That is structurally wrong because builtin function values:

- may be valid closure targets
- may not have `PreparedFunc`
- may not have entries in `func_sym_map`

### 2. Trampoline emission is user-function only

Boot currently emits closure trampolines from prepared user functions.
There is no explicit parallel path for builtin / prelude function references
used as values.

### 3. Higher-order handling is scattered

Today the backend contains several partial adaptations:

- user higher-order call wrapping
- runtime arg wrapping for closure-shaped builtin ABI params
- intrinsic-specific closure argument handling
- planner-side detection for some higher-order builtin edges

This works for some cases, but the ownership is fragmented.

## Desired invariants

The backend should converge on the following rules.

### Invariant 1: callable-target kind is explicit

For any `FuncId` that reaches codegen, backend logic should be able to answer:

- is this a user function target?
- is this a builtin / prelude target?
- is it direct-callable?
- is it closure-capable?
- if closure-capable, what trampoline family must exist?

No pass should need to infer this ad hoc from unrelated data structures.

### Invariant 2: closure-capable is not synonymous with prepared user function

A callable target may be closure-capable even if it has:

- no prepared body
- no user function symbol
- no entry in the prepared-function map

### Invariant 3: planner owns trampoline requirements

If a global function value can cross a closure boundary, planning should record
that requirement explicitly.

Emission should not be forced to rediscover closure support needs from local
syntax alone.

### Invariant 4: verifier understands closure-capable global functions

The verifier should explicitly permit `AGlobalFunc(fid)` at closure boundaries
when `fid` is a known closure-capable target.

### Invariant 5: closure materialization is centralized

There should be one primary helper for adapting a function value to a closure
value at ABI boundaries instead of separate bespoke logic for each builtin or
intrinsic call site.

## Proposed design

## 1. Introduce a backend callable-target abstraction

Add a small shared abstraction used by planning, verification, and emission.

A possible shape is conceptually:

- `UserFunc(PreparedFunc)`
- `BuiltinFunc(BuiltinEntry)`

or a query API that answers the same questions without allocating a new tagged
value everywhere.

This abstraction should provide at least:

- callable kind
- direct-call support
- closure-capable support
- closure trampoline mode
- direct-call symbol / runtime dispatch ownership

The exact data shape is flexible, but the backend needs a single source of truth
for callable identity.

## 2. Add explicit planning for closure-capable builtin refs

The planning scan should record not only builtin direct calls, but also builtin
`FuncId`s that appear in positions where a closure value may need to be
materialized.

This includes at least:

- `AGlobalFunc` passed to function-typed params of user functions
- `AGlobalFunc` passed to closure-taking builtins / intrinsics
- `AGlobalFunc` stored in data or returned through function-typed paths
- explicit closure-making sites if they exist for globals

The result should be an explicit planned set of closure-capable global targets,
not merely a side effect of direct-call analysis.

## 3. Add builtin closure trampolines

Add a dedicated emission path for builtin / prelude closure trampolines.

Recommended first step:

- support a universal closure trampoline for closure-capable builtins
- reuse builtin ABI metadata for parameter/result adaptation
- emit null / empty env because builtin globals do not capture user locals

This is enough to restore correctness without immediately committing to typed
builtin closures.

## 4. Dispatch closure construction by callable kind

Refactor `emit_make_closure(...)` so it branches by callable target kind.

### User function target

Keep the existing user path:

- user universal trampoline
- typed closure path when available

### Builtin target

Use builtin closure trampoline symbols and construct a base closure value
without assuming a user function symbol exists.

As part of this phase, remove any unconditional user-symbol lookup from closure
construction.

## 5. Centralize function-value-to-closure adaptation

Create a single helper responsible for this operation:

> given a function-valued atom and an expected closure boundary, emit the
> correct closure materialization for either user or builtin targets

Use that helper from:

- user direct call arg adaptation
- runtime builtin arg adaptation
- intrinsic lowering that accepts closures
- any closure-valued storage / return path that needs explicit materialization

This should replace call-site-specific wrapping logic as the primary model.

## 6. Make verifier closure-aware for builtin globals

Strengthen backend verification so it can say:

- this `AGlobalFunc(fid)` is closure-capable and valid here
- this `AGlobalFunc(fid)` is not closure-capable and should be rejected here

Do not encode this as a silent special case. Treat it as a first-class backend
rule.

## Representation strategy

## Initial recommendation: universal builtin closures only

For the initial correctness fix, support builtin function values through the
universal closure ABI only.

Why:

- it restores semantics for the failing class of programs
- it avoids immediate typed-closure complexity for builtins
- it keeps the first implementation aligned with the existing runtime closure
  representation

Typed builtin closures can be added later if code quality or performance makes
that worthwhile.

## Alternative later extension: typed builtin closures

Once the abstraction above exists, the backend may later decide to emit typed
builtin closure structs for builtins with fully concrete ABI.

That should be treated as a follow-up optimization / specialization plan, not a
prerequisite for correctness.

## Materialization boundary strategy

Two broad approaches are possible.

### Option A: emit-time materialization

Keep `PreparedAtom::AGlobalFunc(fid)` as the IR form and materialize closures in
emission when a closure boundary requires it.

Pros:

- smallest immediate refactor
- fits current prepared-IR shape

Cons:

- planner and verifier must separately understand closure-capable globals

### Option B: explicit prepared closure-materialization op

Introduce an explicit prepared representation for converting a global function
into a closure value.

Pros:

- makes closure boundaries more explicit in prepared IR
- can simplify later verification and emission

Cons:

- larger refactor across more passes

Recommended approach for now: start with Option A, but keep the helper / query
APIs clean enough that migrating to Option B later would be straightforward.

## Implementation phases

## Phase 1: establish callable ownership

Primary work:

- introduce shared callable-target queries or abstraction
- remove closure-construction assumptions that require user-only symbols
- make `emit_make_closure(...)` branch by callable kind

Exit criteria:

- builtin globals no longer crash merely because they lack a user symbol
- callable-target classification is shared rather than duplicated

## Phase 2: planner support for first-class builtin refs

Primary work:

- explicitly record closure-capable builtin refs in planning
- separate direct builtin call tracking from closure support requirements

Exit criteria:

- planning can answer which builtin `FuncId`s require closure trampoline
  support

## Phase 3: builtin closure trampoline emission

Primary work:

- emit universal builtin closure trampolines
- hook them into module emission and symbol ownership
- ensure runtime/intrinsic adaptation is correct through builtin ABI metadata

Exit criteria:

- builtin function values can be wrapped as closures and called indirectly

## Phase 4: centralize closure-boundary adaptation

Primary work:

- introduce a single helper for function-value-to-closure adaptation
- route direct-call, runtime-call, and intrinsic closure-taking sites through it
- retire bespoke site-specific wrapping paths where possible

Exit criteria:

- closure-boundary logic is centralized and reused

## Phase 5: verifier parity

Primary work:

- verifier recognizes closure-capable builtin globals
- verifier rejects unsupported builtin closure flows explicitly
- verifier diagnostics describe unsupported callable edges in backend terms

Exit criteria:

- verifier and emitter agree on closure-capable builtin targets

## Phase 6: hardening and parity coverage

Primary work:

- add focused coverage for builtin first-class function flows
- confirm self-host loop advances past the current failure class
- document any remaining unsupported builtin closure shapes explicitly

Exit criteria:

- failing examples like `xs.map(Int.to_string)` compile and run through the
  self-host path
- backend tests cover user and builtin function values through the same closure
  boundaries

## Tests to add

### 1. User higher-order call with builtin function value

Examples:

```tw
xs.map(Int.to_string)
xs.map(Float.to_string)
```

These should confirm builtin globals passed to user higher-order functions are
materialized as closures correctly.

### 2. Builtin / intrinsic higher-order entry with named user function

Examples:

```tw
Iterator.unfold(0, step)
Cell.update(c, inc)
```

These are already close to the current bug seam and should remain covered.

### 3. Builtin function stored then called later

Add at least one case where a builtin function value is:

- bound to a local
- stored in a record / option / result / collection
- later called through a function-typed path

This ensures the fix is not limited to direct argument adaptation.

### 4. Verifier characterization tests

Add tests that explicitly distinguish:

- closure-capable builtin globals accepted at closure boundaries
- unsupported builtin globals rejected with clear diagnostics

### 5. Self-host regression checks

After each phase, rerun the preferred self-host path:

```bash
./target/release/twk build boot/main.tw -o /tmp/boot.wasm
BOOT_WASM=/tmp/boot.wasm tools/run.sh examples/api_ergonomics.tw
```

The objective is to ensure the current failure class is removed and the next
failure, if any, reflects a new blocker rather than callable-model drift.

## Success criteria

This plan is complete when:

- builtin / prelude functions can flow through closure-typed boundaries without
  backend crashes
- planner, verifier, and emitter share one explicit model of closure-capable
  callable targets
- closure construction no longer assumes that every closure-capable target has a
  user function symbol or prepared body
- the boot self-host path advances through current higher-order builtin cases
- new backend coverage protects against regressions in both user and builtin
  first-class function handling

## Notes on scope discipline

The tempting short fix is to patch individual sites like:

- `Iterator.unfold`
- `Cell.update`
- `map(Int.to_string)`

with more ad hoc wrapping logic.

That would keep the backend moving, but it would not solve the real ownership
problem.

This plan intentionally prefers:

- one callable-target model
- one planner story for closure-capable globals
- one main closure adaptation helper

over a growing set of per-builtin exceptions.
