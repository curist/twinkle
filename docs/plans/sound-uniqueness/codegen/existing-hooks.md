# Existing Mutable Hook Inventory

**Status:** Draft inventory; verify against the boot compiler before codegen work
starts.

The first codegen implementation should reuse current mechanisms. This doc names
the hooks to verify and catalog before any ownership decision emits mutable code.

## Hook categories

### Vector indexed update

- Existing unsafe/in-place vector set helper used by previous optimization work.
- Persistent fallback remains ordinary vector set/update lowering.
- First target for emitted-code changes because the argument/result shape is small
  and easy to inspect in WAT.

### Vector builders

- Existing `vector$builder_*` family.
- Some builder use is semantic, notably `collect`, and must keep working without
  an ownership optimization decision.
- Optimizer-selected builder regions should be treated separately from semantic
  builder lowering.

### Dict in-place helpers

- Existing runtime/helper path for dict set/update.
- Remove support must be verified separately from set/update.
- The catalog must record insertion-order and old-version observability
  requirements before emission uses these helpers.

### Record shell update

- Existing `ARecordUpdate.in_place` slot or backend equivalent.
- Current branch keeps the slot but does not set it from an ownership proof.
- Shell reuse is distinct from ownership of vector/dict storage stored in fields.

## Verification before use

For each hook, record in [operation-catalog.md](operation-catalog.md):

- exact helper/op name as emitted by the boot compiler;
- expected operand order;
- whether the helper returns the updated immutable value or mutates through an
  internal handle;
- type restrictions or monomorphized forms;
- persistent fallback used when the hook is unavailable;
- WAT/call-inspection signature that proves the hook was selected.

## Non-hooks

`@std.buffer` is user-facing workaround scaffolding, not an internal hook for this
track. Its retirement policy lives in
[../migration/buffer-cleanup.md](../migration/buffer-cleanup.md).
