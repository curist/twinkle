# Stage0 illegal-cast bootstrap failure

Status: completed and archived.

## Summary

A refactor in `boot/compiler/query/definition.tw` exposed a stage0 bug that made
`make stage2` / `make bundle-cli` fail while starting the stage1 compiler. The
stage0-produced wasm trapped with:

```text
RuntimeError: illegal cast
```

The bad WAT was in `boot/main.tw` top-level initialization. The CLI command
vector was constructed as a `PVec`, but the destination local was typed as
`Array`, so codegen emitted an invalid `PVec -> Array` cast.

## Root cause

Stage0 inferred module globals by intersecting:

- locals referenced free outside `__init__`; and
- every local bound anywhere inside `__init__`.

That is unsafe because ordinary `LocalId`s are function-scoped. A free local in
one function can numerically collide with an ANF temporary in `__init__`.

When the query refactor changed function/local numbering, such a collision made
the temporary holding the CLI command vector look like a module global. Then
`assign_expr_locals` intentionally ignored preserved mono metadata for that
local and fell back to `infer_op_valtype(AArrayLit)`, which returns `Array`.
Vector literals semantically construct `PVec`, so the emitted cast was invalid.

## Fix

The fix is to distinguish source-level init bindings from compiler temporaries:
module-global and module-global-pinning analyses now consider only `AInit`
bindings in `__init__`.

Implemented in:

- `src/ir/anf/analysis.rs`: added `collect_init_binding_locals`
- `src/codegen/emit.rs`: module-global discovery uses init bindings
- `src/opt/pipeline.rs`: optimizer pinning uses init bindings
- `boot/compiler/anf_analysis.tw`: boot parity helper
- `boot/compiler/opt/pipeline.tw`: boot optimizer parity fix

Regression coverage:

- `module_global_collection_ignores_init_temporaries_with_colliding_local_ids`
- `collect_init_binding_locals_excludes_non_init_temporaries`

## Validation

Validated with:

```sh
cargo test --release
make stage2
./target/release/twk check boot/main.tw
```

The original stage1 startup `RuntimeError: illegal cast` is resolved. Applying
the previous `definition.tw` refactor no longer produces the bad stage0 startup
trap; any remaining failures from that refactor are separate boot-backend issues.

## Follow-up guidance

Avoid using bare `LocalId` as a whole-module identity unless the value is known
to be from the reserved module-global range or the key is scoped by function.
Whole-module analyses should either carry function identity or restrict matches
to explicit global/source-level binding shapes.
