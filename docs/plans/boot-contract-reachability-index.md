# Boot Contract Reachability Index

## Goal

Make contract-call reachability in the Core linker more explicit and cheaper by
pre-indexing fallback method targets.

---

## Motivation

`boot/compiler/core_linker.tw` performs lightweight DCE after linking. When it
encounters a contract call and cannot resolve an exact target, it falls back to
scanning functions for method-name matches. That conservative fallback is useful,
but scanning all functions from expression traversal is both imprecise and harder
to reason about.

---

## Non-Goals

* No change to contract syntax or capability design
* No new trait system
* No removal of conservative fallback behavior in the first phase
* No change to normal exact method resolution

---

## Target Shape

Build fallback indexes once before reachability BFS:

```tw
type ReachabilityIndex = .{
  funcs_by_id: Dict<Int, FunctionDef>,
  fallback_methods: Dict<String, Vector<Int>>,
}
```

Then expression traversal can resolve fallback contract refs with a dictionary
lookup rather than scanning all functions repeatedly.

---

## Work Plan

### Phase 1: Introduce an index without behavior changes

- [x] Add a `ReachabilityIndex` helper type near linker reachability code.
- [x] Build `funcs_by_id` and `fallback_methods` once from linked functions.
- [x] Replace `fallback_contract_refs` scans with index lookups.
- [x] Preserve current matching rules exactly.

### Phase 2: Make exact vs fallback resolution visible

- [ ] Separate exact contract target resolution from fallback resolution in code.
- [ ] Add comments explaining why fallback exists and when it is expected.
- [ ] Add tests around contract calls that rely on exact resolution and fallback
      resolution.

### Phase 3: Tighten fallback over time

- [ ] Audit cases where fallback is still required.
- [ ] Prefer exact typed method resolution where enough type/env information is
      available.
- [ ] Consider turning unexpected fallback into verifier diagnostics once the
      exact path is complete.

---

## Validation

- [x] Core linker suite
- [x] Contract/stringify-related tests
- [x] Codegen integration suite
- [x] Boot self-build

---

## Risks

* The existing fallback may be masking missing exact resolution cases.
* Tightening too soon can make valid programs disappear from DCE roots.
* Name-based matching must stay conservative until contract lowering/linking is
  fully explicit.
