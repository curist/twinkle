# Contract Call Resolution Plan

## Goal

Make builtin contract method calls resolve through exact function identity instead
of rediscovering targets later by method name. Contract calls such as
`x.to_string()` for `T: Stringify` should become exact calls to the inherent
method attached to the concrete receiver type after monomorphization, or fail
with a clear compiler error if no unique target exists.

## Motivation

Formatting `boot/tests/main.tw` reordered imports and exposed a codegen bug:
contract fallback selected an unrelated `to_string` loaded earlier than the
receiver type's actual inherent method. Recent fixes tightened and ranked the
fallback search, but this is still not the right correctness boundary. The
compiler should not have to scan functions named `to_string` to rediscover the
method belonging to a nominal type.

Twinkle records are nominal and inherent methods are module functions whose first
parameter is the receiver type. That means the compiler should be able to resolve
contract method implementations by receiver type identity, not by import order or
name-shape heuristics.

## Current failure mode

The checker/lowerer can produce generic contract calls like:

```tw
ContractCall(Stringify, "to_string", receiver, args)
```

That node preserves the contract and method name, but not an exact target. During
monomorphization, once the receiver type becomes concrete, the pass attempts to
recover a target by:

1. looking up a method name in the resolved environment,
2. looking up the returned name in the linked function table,
3. falling back to scanning function definitions by method-shaped names.

Step 3 is the design smell. It makes correctness depend on what functions happen
to be linked and in which order they are encountered.

`boot/compiler/core_linker/contract_resolve.tw` already contains
`exact_contract_ref(...)`, but that API is only "exact" after it has converted a
receiver type to a function name and then looked that name up in `func_table`.
This plan replaces that name-oriented seam with a resolver whose semantic source
of truth is method metadata keyed by receiver type identity.

Follow-up investigation found a separate prerequisite bug: linked Core IR kept
per-module `TypeId`s in function params, expression types, record/variant nodes,
and match patterns, while the linked `ResolvedEnv` used the entry/linker's
remapped type identities. That made a real receiver method such as
`Box<T>.to_string` appear incompatible with `Box<Int>` after linking, so fallback
could reject the correct method and later call an unrelated same-named function.
Moving DCE after monomorphization did not fix this by itself; the linker must
first canonicalize type identities across module boundaries.

## Current implementation status

Steps 1–7 are complete. The method table is the primary contract resolution
path; the fallback name-scan (`fallback_contract_target_id`) has been removed
from monomorphization; DCE uses method-table-based contract retention instead
of broad same-name matching; and `build_type_remap` errors on unmapped nominal
TypeIds.

Implemented:

- linked Core IR now canonicalizes nominal `TypeId`s across modules before DCE,
  monomorphization, ANF, and codegen see it,
- a regression test guards that cross-module `Box.to_string` uses the linked
  canonical `Box` type identity,
- `CoreModule` now carries `method_table: Dict<String, FuncId>` mapping
  `(receiver_type, method_name)` to exact linked `FuncId`,
- the linker builds the method table from ALL modules' method metadata during
  the linking loop, remapping TypeIds and FuncIds, then filters after DCE,
- linker emits `eprintln` diagnostics if a method metadata entry cannot be
  resolved in the module's `func_table` (currently silent in practice),
- `resolve_contract_target_id` uses the method table as primary path via
  `method_table_key()`, with env-based name lookup as secondary (no fallback
  scan of all functions),
- `method_table_key` handles `ExternRef` types alongside `Named` types,
- `fallback_contract_target_id` and all its helpers (`contract_candidate_for_func`,
  `match_receiver_type`, `receiver_specificity`, `better_contract_candidate`,
  `name_matches_method`, `contract_expected_return`, `TypeMatch`,
  `ContractCandidate`) have been removed,
- monomorphization functions use a `MonoCtx` record to bundle immutable context
  (`generic_ids`, `func_map`, `env`, `func_table`, `method_table`) instead of
  threading five separate parameters,
- a test verifies the method table contains the correct `Box.to_string` FuncId
  after cross-module linking,
- `build_type_remap` errors on unmapped nominal TypeIds with origin-based
  lookup falling back to name-based lookup before erroring,
- DCE uses method-table suffix matching for generic receivers instead of broad
  same-name function retention; removed `fallback_contract_refs`,
  `fallback_method_names_for_func`, `append_unique_int`, `append_unique_str`.

All steps complete.

## Desired invariant

After monomorphization, there should be no unresolved `ContractCall` on the
correctness path. Every contract method call should lower to an exact function
call:

```tw
Call(GlobalFunc(target_func_id), [receiver, ...args])
```

The target function must be chosen from authoritative method metadata for the
receiver type. Fallback name scans may remain temporarily for diagnostics or
legacy reachability, but should not be required to compile valid programs.

## Design direction

### 1. Replace the current contract resolver with exact method implementation lookup

Extend or replace `boot/compiler/core_linker/contract_resolve.tw` so the central
query resolves:

```tw
(contract, method_name, concrete_receiver_type) -> FuncId?
```

The lookup should use authoritative metadata:

- primitives and builtin containers use registered builtin method mappings,
- prelude/builtin aliases use the same canonical names/identities as
  `lower_core/calls.tw::prelude_method_alias` and base-env method registration,
- `Named(type_id, args)` uses the nominal type's inherent method table,
- imported methods resolve through canonical function identity after module
  linking,
- generic receiver methods return the generic `FuncId`; monomorphization then
  specializes that exact function.

The resolver may temporarily carry names alongside `FuncId`s while the linked
method table is introduced, but name lookup must be a compatibility layer, not
the semantic source of truth. This API should not scan all function names.

### 2. Preserve enough metadata and type identity through linking

Audit the module linker to ensure method metadata survives function-id remapping.
For nominal types, the linked program should have a post-link method table keyed
by `(receiver type identity, method_name)` with linked `FuncId` values.

The linker must also remap every `TypeId` embedded in linked Core IR into the
same canonical identities used by the linked `ResolvedEnv`. This includes
function params/returns, expression `ty` fields, `Record`/`Variant` nodes,
`ExternRef`/`Named` types nested inside containers/functions/results/options, and
variant patterns in `Match` arms. Without this, receiver compatibility checks can
compare a module-local nominal id against the entry-env id for the same type and
incorrectly reject the true inherent method.

Current groundwork remaps known type identities but leaves unmapped entries in
place. Tighten this before considering the invariant complete: after linking,
nominal IDs embedded in IR should either be owned by the linked `ResolvedEnv` or
produce an internal linker error with the module path, original type id, and IR
location/category being remapped.

If the current environment stores only names, either:

- update it to store `FuncId` where linking can remap it, or
- derive a post-link method table from `ResolvedEnv` plus `func_table`.

The second option is likely lower risk because it avoids changing frontend
resolver APIs immediately. It is transitional: the long-term invariant is that
contract resolution depends on linked function identity, not on a name that
happens to resolve.

### 3. Enforce uniqueness instead of first-match behavior

The exact resolver must reject ambiguous method metadata. If the linked method
table contains multiple entries for the same receiver type identity and method
name, compilation should fail with an internal error instead of selecting the
first entry.

This matters because `ResolvedEnv` currently exposes lookup helpers that can
return the first matching method entry. The new resolver must use a path that can
observe duplicates and report ambiguity.

### 4. Resolve contract calls during monomorphization

Do not blindly trust a name-table hit for named receivers. If method metadata
returns a function name that resolves in `func_table`, validate that the selected
function is compatible with the concrete receiver type before accepting it. If it
is not compatible, the final behavior should be a targeted internal error once
exact metadata is expected.

Current groundwork still uses a receiver-compatible fallback scan as a temporary
bridge for name-based method tables. This fallback remains correctness-sensitive
and is therefore pending work: a missing/stale method table must not silently
select another method-shaped function, even if that function is receiver-compatible
by type.

When monomorphization sees:

```tw
ContractCall(contract, method_name, receiver, args)
```

it should:

1. rewrite/specialize the receiver and arguments,
2. use the concrete receiver type to find an exact implementation `FuncId`,
3. if the implementation is generic, enqueue/specialize that exact function,
4. rewrite the expression to `Call(GlobalFunc(resolved_or_specialized_id), ...)`.

If resolution fails, report an internal compiler error that includes:

- receiver type,
- receiver category: builtin/container/named/other,
- contract and method name,
- source span,
- whether method-table metadata was missing, ambiguous, or present but failed
  `func_table`/`FuncId` resolution.

Do not continue with a broad fallback scan once exact linked method metadata is
available. During the transitional phase, any fallback must be receiver-compatible
and should exist only to bridge current name-based method tables. The next step is
to convert fallback use into diagnostics/assertions so valid programs compile via
exact metadata only.

### 5. Remove fallback from correctness-sensitive paths

Moving DCE after monomorphization is not sufficient on its own. It may still be a
reasonable cleanup later, but the observed regression was caused primarily by
non-canonical linked `TypeId`s plus same-name function collisions. Fix linker
TypeId canonicalization first, then tighten contract resolution.

Once exact resolution is in place:

- remove `fallback_contract_target_id` from monomorphization, or restrict it to a
  temporary assertion-only compatibility path,
- first prove no post-monomorphization `ContractCall` reaches DCE/codegen,
- then update `core_linker/dce.tw` to remove broad same-name retention and rely
  on exact resolved calls/references,
- add a guard test that no `ContractCall` reaches ANF lowering/codegen.

## TDD plan

### Existing regression fixtures

Keep these fixtures as permanent coverage:

- `boot/tests/fixtures/multi/contract_fallback_main.tw`
  - unrelated non-generic `to_string` loaded earlier must not be selected.
- `boot/tests/fixtures/multi/contract_generic_fallback_main.tw`
  - unrelated generic `to_string<T>` loaded earlier must not beat the receiver's
    actual `Box<T>` method.

### New red tests to add before implementation

1. **Ambiguous fallback should not silently pick by order**

   Construct two unrelated imported generic `to_string<T>` functions and a
   receiver with its own nominal method. The correct implementation must be the
   nominal receiver method, independent of import order.

2. **No fallback for missing method metadata**

   This is better expressed as an internal compiler test than as a normal `.tw`
   fixture. Build or inject a broken linked environment where the checker has
   accepted a contract through a known inherent method, but the post-link method
   metadata is missing. Monomorphization should fail with a targeted internal
   error instead of choosing another method-shaped function.

3. **Cross-module generic inherent method specialization**

   Define `Box<T>` and `to_string<T: Stringify>(Box<T>)` in one module. Call a
   generic `show<T: Stringify>` from another module. Verify generated WAT calls
   the specialized `Box<Int>` method, not any unrelated `to_string`. This also
   guards the linker invariant that the `Box` `TypeId` inside linked function
   params/bodies has been remapped to the canonical entry-env type identity.

4. **Builtin/container contracts still resolve**

   Verify `Vector<Int>.to_string()` and primitive interpolation still compile
   after fallback removal. Include both direct method calls and contract-backed
   generic calls so prelude alias handling stays covered.

5. **Duplicate method metadata is rejected**

   Add an internal resolver/linker test that constructs duplicate method metadata
   for the same receiver type identity and method name. The exact resolver should
   report ambiguity rather than returning the first entry.

6. **Exact target identity is asserted in IR**

   Add a Core/mono IR-level regression that finds the contract-backed call inside
   the specialized `show(Box<Int>)` body and asserts it targets the `Box.to_string`
   function identity, not merely a same-named `to_string`. WAT substring checks are
   not sufficient because an unrelated generic fallback can have a plausible symbol
   name.

7. **Unmapped linked nominal IDs are rejected**

   Add a linker test or fixture that would leave a module-local `TypeId` embedded
   in linked IR. Linking should fail with a targeted internal error instead of
   allowing later resolver/codegen phases to observe an unknown nominal id.

## Implementation steps

1. Canonicalize linked Core IR `TypeId`s in `core_linker.tw` so each module's
   local nominal ids are remapped to the same ids used by the linked `ResolvedEnv`.
   Remap function signatures, expression types, record/variant constructors, and
   match patterns. **Done.**
2. Tighten type remapping so unmapped nominal IDs embedded in linked IR become a
   targeted internal linker error instead of being left in place. **Done.**
   `build_type_remap` now errors on unmapped nominal TypeIds, with origin-based
   lookup falling back to name-based lookup before erroring.
3. Replace/extend `core_linker/contract_resolve.tw` with an exact resolver. It
   may temporarily return names plus `FuncId`s, but name lookup must be a
   transitional compatibility layer, not the semantic source of truth. **Done.**
4. Add or derive a post-link method table keyed by receiver type identity and
   method name. The table must reject ambiguous entries. **Done.** `CoreModule`
   now carries `method_table: Dict<String, FuncId>` keyed by
   `"${type_key}::${method_name}"` where type_key is the builtin name or
   `"t${tid.id}"` for Named types. Built in `core_linker.tw` during the linking
   loop using each module's `methods_by_type` and `methods` with remapped
   TypeIds and FuncIds, then filtered after DCE.
5. Update monomorphization to resolve `ContractCall` through exact metadata.
   **Done.** `resolve_contract_target_id` uses method table as primary path
   via `method_table_key()`, with env-based name lookup as secondary.
6. Remove `fallback_contract_target_id` from monomorphization's correctness path.
   **Done.** The fallback scan and all supporting helpers have been removed.
   Monomorphization uses a `MonoCtx` record to bundle context parameters.
7. After proving `ContractCall` cannot reach DCE/codegen, remove DCE's broad
   same-name retention. Consider moving DCE after monomorphization as a cleanup,
   not as the primary correctness fix. **Done.** DCE now uses method-table
   suffix matching for generic receivers instead of broad same-name retention.
8. Add tests proving import order does not affect contract method target choice
   and tests asserting exact IR target identity. **Done.** Added three tests:
   import-order invariance (reversed import fixture produces same WAT output),
   no `ContractCall` survives monomorphization, and specialized `show(Box<Int>)`
   calls the exact `Box.to_string` specialization by FuncId in mono IR.
9. Run the boot test suite and self-host loop. **Done.** All 2040 boot tests
   pass, Rust tests pass, stage2 is up to date.

## Non-goals

- Do not introduce a trait system.
- Do not change source-level contract syntax.
- Do not make imports order-sensitive or preserve import order in the formatter as
  a workaround.
- Do not rely on WAT symbol names as the only long-term assertion mechanism;
  prefer IR-level target identity assertions where practical.

## Success criteria

- Contract method calls are resolved by nominal receiver type and exact function
  identity.
- Missing or stale method metadata fails with a targeted internal error instead
  of falling back to a global same-name scan.
- Monomorphization no longer scans all functions by method name to find a target.
- Existing and new fixtures pass regardless of import order.
- IR-level tests assert the exact `FuncId` target for contract-backed calls.
- Ambiguous method metadata is rejected rather than first-matched.
- Linked Core IR uses canonical linked `TypeId`s, so receiver compatibility is
  stable across module boundaries.
- DCE does not need broad same-name retention for contract calls.
- Formatting `boot/tests/main.tw` does not change test behavior.
