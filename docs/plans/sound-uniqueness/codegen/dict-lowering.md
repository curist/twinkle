# Dict Lowering

**Status:** Draft slice plan.

Dict lowering should reuse the same decision/handoff path as vector lowering, but
it has its own runtime semantics risks: HAMT update behavior, removal, insertion
order, and nested value ownership.

## Slice order

1. Catalog persistent `Dict.set` and the existing in-place set target.
2. Dry-run accepted/rejected decisions with persistent fallback names.
3. Emit proven-owned `Dict.set` through the existing helper.
4. Catalog and only then enable `Dict.remove`.
5. Defer record-backed wrappers such as `Set<K>` until field-path ownership is
   available.

## Required proof

- Source dict backing is `Unique` at the update/remove site.
- Source dict is at last use for the old persistent value.
- No older dict version remains observable through locals, records, variants,
  closures, tasks/channels, cells, globals, unknown callees, or branch exits.

## Semantic constraints

- Lookup behavior must match the persistent HAMT path.
- Insertion-order iteration must remain correct.
- Ownership of dict backing does not imply ownership of reference-typed values
  stored inside the dict.

## Fallbacks

Emit persistent `Dict.set`/`Dict.remove` for missing proof, stale decisions,
unsupported helper shapes, nested-value uncertainty, or unverified remove/order
semantics.
