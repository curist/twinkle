# Vector Lowering

**Status:** Draft slice plan.

Vector lowering is the first emitted-code target because indexed update has a
small operation shape and directly exercises `Unique` + last-use decisions.

## Slice order

1. Dry-run decision renders `Vector.set_at` persistent target and existing
   in-place target.
2. Backend lookup finds the decision but still emits persistent code.
3. One local owned indexed update emits the existing in-place helper.
4. Loop-carried vector updates emit the helper only when back-edge facts certify
   ownership preservation.
5. Builder regions are enabled separately from indexed updates. The same builder
   infrastructure also covers string-concat regions; vector concat/extend through
   runtime `builder_extend` is not first-cut until its boot builtin/shim path is
   cataloged.

## Required proof

- Source vector backing is `Unique` at the update site.
- Source vector is at last use for the persistent value being replaced.
- Any interleaved reads are non-escaping borrows explicitly represented in facts.
- Branch/loop joins preserve ownership on all paths that reach the mutable site.

## Fallbacks

Emit the persistent vector update when any required proof or mapping is missing,
when the helper target is unavailable, or when the source/result shape does not
match the cataloged family.
