# Module-global type tracking by linked global id

## Status

Proposed. Motivated by an attempt to expose LSP `CompletionItemKind` values as
module-level `pub` value bindings (`kinds.constant`). That refactor shipped as
functions (`kinds.constant()`) instead, because adding a module of `pub` value
constants triggers the bug described here. See `boot/lib/lsp/kinds.tw`.

## Symptom

Adding a new module whose only exports are module-level `pub` value bindings
(e.g. a dozen `pub constant := 21`) can make a previously-building program fail
to compile, with an error about an **unrelated** global:

```
backend verifier: function run_help_command (F310x): call arg 0 to format_help
  has mono Void, expected Named(T328)  [arg=AGlobalLocal(L59, mono=Void)]
```

`L59` here is the top-level `cli` record in `boot/main.tw` — nothing to do with
the new module. Adding the new globals shifted the global-id layout enough to
expose a latent defect.

## Root cause

Two distinct id spaces are conflated:

- **Linked GlobalLocal ids** — program-wide unique, assigned by `core_linker`
  (`core_linker.tw`, "Step 2b: Assign globally unique GlobalLocal IDs"). These
  are what `AGlobalLocal(lid)` operands carry after linking.
- **Per-module source LocalIds** — unique only *within* a module, carried by
  each prepared slot as `SlotInfo.source_local`.

All module-level initializers are merged into a **single `$init`** function.
Each module numbers its locals from zero, so within `$init` two unrelated slots
can share the same `source_local.id`. Observed: two slots, both
`source_local.id = 59`, both `role = Local`, both `mono = Void`, neither of
which is the real `cli` global (whose true type is `Named(T328)`).

The backend nonetheless keys module globals by `source_local.id` everywhere:

- `wasm_plan_impl.tw` builds `reg.module_globals[k]` (the codegen registry),
  resolving each global's mono via `facts.module_global_assign_monos` plus a
  fallback that scans init slots by `source_local.id`.
- `emit.tw` (~"For module globals in `$init`, also set the global") emits the
  `GlobalSet` keyed by `source_local.id`.
- `verify.tw` `build_module_globals` resolves global monos the same way.

When ids collide, the wrong slot wins (dict iteration order), so the global
resolves to an imprecise type (`Void`). Codegen tolerates this because module
globals are stored **anyref**: a `Void`/`i31` declared type still holds a record
via boxing, and reads cast back. The verifier is the only strict consumer, so it
is the first to reject — but the imprecise mono also drives the global's repr to
`OpaqueAnyref`, so the problem is not merely verifier strictness (see Rejected
alternatives).

### Latent soundness concern

Because global identity is `source_local.id`, two *genuinely distinct* module
globals that collide on `source_local.id` would map to the same
`global_local_<n>` Wasm global and alias at runtime. We did not observe data
corruption (the colliding slots above were not both live globals), but the
identity scheme does not preclude it.

## Why the obvious shallow fixes fail

- **Relax the verifier** (skip the mono check for `AGlobalLocal` operands): the
  immediately-following repr check still fails (`OpaqueAnyref` arg into a
  `TypedRef` param, "missing AUnwrapAnyref?"), because codegen derives repr from
  the same wrong mono. Relaxing both would mask genuinely malformed codegen.
- **Disambiguate by preferring non-`Void` slots**: in the observed case *both*
  colliding slots are `Void`; the real type is in neither.
- **Functions instead of values** (`kinds.constant()`): the current workaround.
  It sidesteps module globals entirely, but does not fix the underlying defect
  and does not help genuine module-level `pub` value constants elsewhere.

## Proposed fix

Track each module global's true type by its **linked** global id, from an
authoritative source, and use that single map in both codegen and the verifier.

1. **Build the authoritative map at link time.** `core_linker` already computes
   `global_value_ids: Dict<module_path, Dict<value_name, linked_gid>>`. Join it
   with each exported value's type to produce
   `linked_global_types: Dict<Int /*linked gid*/, MonoType>`. The value types
   come from the type-checked module envs / `pub_values`; confirm the linker has
   access (or thread the needed types in).
2. **Carry it through the pipeline.** Add the map to the linked module output and
   thread it `link → monomorphize → lower_anf → prepare` so it lands on
   `PreparedModule`. Module-global types are concrete, so monomorphization should
   pass them through unchanged; verify this.
3. **Consume it in one place.** Replace the `source_local.id`-based resolution in
   `wasm_plan_impl.tw` (`reg.module_globals`) and `verify.tw`
   (`build_module_globals`) with direct lookups into `linked_global_types`,
   keyed by the linked gid (= the `AGlobalLocal` operand id). `emit.tw`'s
   `GlobalSet` should key off the same identity.
4. **Make global identity the linked gid, not `source_local.id`.** This closes
   the aliasing soundness gap and removes the merged-`$init` collision hazard.

### Shared-logic note

When this lands, the codegen planner and the verifier should resolve module
globals through one shared helper (the original `kinds` review feedback asked for
this). The earlier attempt extracted `facts.module_global_monos` +
`collect_referenced_globals` and had both callers use them — but that only shares
the *structure*; it still resolves via `source_local.id` and so resolves the
wrong type. The missing piece is the authoritative type data above, not shared
control flow. Reintroduce the shared helper once it reads `linked_global_types`.

## Files (expected)

- `boot/compiler/core_linker.tw` — build `linked_global_types`.
- `boot/compiler/core_ir.tw` — carry it on the linked module type.
- `boot/compiler/lower_anf.tw`, `boot/compiler/backend/prepare.tw` — thread to
  `PreparedModule`.
- `boot/compiler/codegen/wasm_plan_impl.tw`, `boot/compiler/codegen/emit.tw` —
  resolve/emit globals by linked gid.
- `boot/compiler/backend/verify.tw` (+ `facts.tw` for the shared helper) — same.
- Rust stage0 (`src/`) — only if needed to keep `boot/main.tw` bootstrapping;
  stage0 currently compiles `boot/main.tw` with `pub` value constants, so it may
  need no change.

## Validation

- Reproduce first: switch `boot/lib/lsp/kinds.tw` back to `pub <name> := <n>`
  value bindings and update `completion.tw` to `kinds.constant` (no parens);
  confirm `target/twk build boot/main.tw` fails as above. This is the regression
  gate.
- After the fix: full self-host loop (`make bundle-cli`) to fixed point, plus the
  boot suite (`make boot-test`) and Rust suite.
- Add a focused test: a small multi-module program that reads a module-level
  `pub` value of a *record* type from another module's top level, exercising the
  linked-gid path end to end.
- Once green with value bindings, convert `kinds` back to the value form.

## Bootstrap note

`target/twk`'s embedded verifier rejects the value-binding program, so it cannot
build a fixed compiler from the new source (chicken-and-egg). Bootstrap through
Rust stage0: `cargo build --release` then
`./target/release/twk build boot/main.tw -o stage1.wasm` produces a compiler
carrying the fix, which then drives the self-host loop.
