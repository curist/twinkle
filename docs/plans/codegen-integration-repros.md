# Codegen Integration Repro Cleanup Plan

## Goal

Restore the remaining disabled codegen integration repros in
`boot/tests/suites/codegen_integration_suite.tw` without masking real compiler
bugs. The suite is now wired into the main boot test runner; the remaining
problem cases are kept as `repro_*` helpers so they stay easy to find while not
breaking the active suite.

## Current status

The active codegen integration suite covers the stable end-to-end codegen path,
runtime linking, closure ABI basics, match emission, and structural linked-module
checks. Stale expectations for runtime module membership and startup export shape
have been updated.

A first cleanup pass fixed the source-string compiler harness so it lowers and
links every analyzed module, not just the virtual entry module. That gives
prelude-backed methods real module origins in codegen integration tests, matching
entry-file compilation. With that structural fix, the iterator `to_vector` repro
that passes a named step function now runs as an active test.

The remaining repro helpers fall into a few bug clusters. Each structural issue
has a focused follow-up plan. `Cell.update` and `Iterator.unfold` builtin method
values are active again after boundary insertion and wasm planning were taught to
carry the expected concrete function mono through closure materialization.

| Repro helper | Symptom | Likely area | Follow-up plan |
|--------------|---------|-------------|----------------|
| `repro_dict_index_materializes_typed_option` | Link identity is fixed, but dict indexing in a prelude generic-call argument currently goes through the erased `rt_dict__get_option` path instead of materializing the typed `Option<Int>` in user code | boundary insertion / typed-vs-erased container egress for prelude generic call arguments | [Dict index typed option boundary](codegen-repro-dict-index-typed-option.md) |
| `repro_builtin_returned_from_function_then_called` | Codegen lookup fails for the returned builtin function id | function-return boundary closure materialization for builtins | [Builtin function return closure materialization](codegen-repro-builtin-return-closure.md) |
| `repro_user_function_returned_from_function_then_called` | Generated WAT returns a raw `ref.func` instead of allocating the expected closure | function-return boundary closure materialization for user functions | [User function return closure materialization](codegen-repro-user-return-closure.md) |

## Investigation order

### Builtin export/link identity for prelude methods

The unresolved `option$unwrap_or` and `iterator$to_vector` link failures were
caused by the source-string compiler path only lowering/linking the virtual entry
module. That path now lowers every analyzed module and builds the same external
reference metadata as entry-file compilation, so prelude-backed methods can link
through their real module origins.

`Iterator.to_vector` with a named unfold step is active again. The dict indexing
case still stays as a repro because it exposes a separate typed/erased boundary
issue; continue that work in
[codegen-repro-dict-index-typed-option.md](codegen-repro-dict-index-typed-option.md).

Tasks:

- Trace how builtin method calls become `FuncId`s and import names in lowering.
- Compare the failing symbols with entries in the builtin registry and signature
  loader.
- Decide whether these helpers should:
  - be emitted inline,
  - lower to another runtime helper,
  - be exported by a runtime/prelude module, or
  - remain unavailable in codegen tests because they are not runtime-level
    helpers.
- Once the symbol identity is correct, re-enable the affected repros as normal
  `.test(...)` entries.

Expected result:

- Dict indexing followed by `unwrap_or` links successfully and still constructs a
  typed `Option<Int>` at the dict boundary, even when the value flows into a
  prelude generic call.
- Iterator `to_vector` examples link through the boot codegen path.

### Function return boundary closure materialization

Returning a function value should produce the same closure representation as
storing a function value in a local or record field. Local and record-field cases
are active and passing; return-position cases are not. Track the builtin and user
function variants in
[codegen-repro-builtin-return-closure.md](codegen-repro-builtin-return-closure.md)
and [codegen-repro-user-return-closure.md](codegen-repro-user-return-closure.md).

Tasks:

- Compare boundary insertion for `AInit`, record construction, and return
  expressions.
- Check whether `AMakeClosure` insertion is missing for return-position function
  values.
- Cover both builtin functions and user functions.
- Ensure returned function values use the universal closure ABI and call through
  `call_ref $rt_types__ClosureFunc`.

Expected result:

- Returning `Int.to_string` from a function emits a builtin trampoline closure.
- Returning a user function emits a user closure allocation.
- Both returned closures can be called indirectly after assignment to a local.

## Re-enabling policy

For each fixed repro:

- Rename the helper back from `repro_*` to `test_*`.
- Add it back to `suite()` near the related active test.
- Prefer a focused lower/backend test when a bug is fixed below full codegen.
- Run `target/twk fmt` on edited `.tw` files.
- Run `target/twk run boot/tests/main.tw` before considering the cleanup done.

## Non-goals

- Do not weaken backend verifier checks to make a repro pass.
- Do not add runtime exports solely to satisfy stale internal names if the helper
  should instead be inlined or lowered differently.
- Do not reintroduce a Wasm start section; linked programs intentionally export
  `__twinkle_start` for host-controlled startup.
