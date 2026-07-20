# String Lowering

**Status:** Draft slice plan.

String lowering in this track means builder-region lowering for `String.concat`
accumulator loops. There is no string in-place mutation target: accepted sites
lower to the existing string builder hooks, while unsupported shapes keep ordinary
persistent `String.concat`.

## Slice order

1. Dry-run decisions render `String.concat` persistent target and the existing
   `string$builder_from/extend/freeze` target sequence.
2. Backend lookup finds the decision but still emits persistent code.
3. One local owned string-concat accumulator loop emits the existing builder
   sequence.
4. Loop-carried string accumulators emit builder code only when back-edge facts
   certify ownership preservation.
5. Keep string builder lowering separate from vector indexed update and vector
   builder work, even though the ABI shim path is shared.

## Required proof

- Source string accumulator is `Unique` at the concat/update site.
- Source accumulator is at last use for the persistent value being replaced.
- Interleaved reads are non-escaping borrows explicitly represented in facts.
- Branch/loop joins preserve ownership on all paths that reach the builder region.

## Hook targets

- Persistent fallback: `String.concat` / `string$concat`.
- Builder seed: `string$builder_from`.
- Builder step: `string$builder_extend`.
- Builder finalization: `string$builder_freeze`.
- Builder buffer ABI: erased builder local cast back to `rt_types__StrBuilder` by
  the existing builder argument shim.

## Non-targets

- No string in-place mutation helper exists or should be invented in this track.
- `String.slice`, indexing, UTF-8 conversion, and parsing helpers are read/share or
  conversion operations, not standalone mutable-lowering targets.
- Runtime/private string builder representation changes belong to migration or a
  later codegen design, not the first existing-hook slice.

## Fallbacks

Emit persistent `String.concat` when any required proof or mapping is missing, when
the builder target is unavailable, when a stale decision is detected, or when the
source/result shape does not match the cataloged family.
