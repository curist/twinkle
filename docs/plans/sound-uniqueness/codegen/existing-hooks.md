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

### Ownership-specialized function variants

- No runtime hook is required: this is compiler cloning plus call-site rewriting.
- The original function remains the generic/persistent fallback.
- Clone names should encode or otherwise link to the canonical `VariantId` for
  inspection, while preserving stable internal ids for codegen.
- Recursive and mutually-recursive clones must route in-SCC calls to the matching
  owned clone/peer clone, not accidentally to the generic function.
- [Worked-examples Case V](../analysis/worked-examples.md#case-v--graph_sccvisit-the-whole-compiler-idiom-real)
  is the anchor: `graph_scc.visit`'s generic body stays conservative, and the
  `visit[unique:p0]` clone may consume the variant-qualified shell-reuse verdicts.

### Record-backed field collection updates

- This is a composed use of existing hooks, not a new runtime primitive: project a
  field, call the existing dict/vector mutable helper when field backing ownership
  is licensed, then use the record-shell slot only when shell reuse is separately
  licensed.
- Shell-only acceptance is valid: a record update may reuse the shell even when the
  projected dict/vector update stays persistent.
- Field backing acceptance is path-sensitive and must preserve sibling reads,
  old-version observability, and nested-value publication rules.

## Verification before use

For each hook, record in [operation-catalog.md](operation-catalog.md):

- exact helper/op name as emitted by the boot compiler;
- expected operand order;
- whether the helper returns the updated immutable value or mutates through an
  internal handle;
- type restrictions or monomorphized forms;
- persistent fallback used when the hook is unavailable;
- WAT/call-inspection signature that proves the hook was selected.

For ownership-specialized variants, also record:

- clone naming and symbol policy;
- call-site rewrite point;
- generic fallback path;
- recursive/mutual-recursive routing behavior;
- cap/fallback behavior when a variant key is unavailable.

## Non-hooks

`@std.buffer` is user-facing workaround scaffolding, not an internal hook for this
track. Its retirement policy lives in
[../migration/buffer-cleanup.md](../migration/buffer-cleanup.md).
