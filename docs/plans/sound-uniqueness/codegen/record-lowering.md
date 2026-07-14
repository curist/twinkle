# Record Lowering

**Status:** Draft slice plan.

Record lowering is split into shell reuse and deep field/backing-storage mutation.
This codegen track should start with simple shell reuse only; deep field ownership
waits for later analysis precision.

## Shell reuse

A record shell update may reuse the shell when analysis proves the record shell is
owned and the old shell is unobservable. The backend consumes an explicit decision
and may set the existing in-place record-update slot or equivalent hook.

## Deep field ownership

A fresh or owned shell does not prove ownership of reference-typed fields. Mutating
`env.types` or a `Set<K>`'s backing dict requires a separate field-path proof from
[../analysis/records-fields.md](../analysis/records-fields.md).

## Slice order

1. Dry-run simple shell-update decisions.
2. Backend lookup/fallback for record-update decisions.
3. Emit one simple shell reuse through the existing record hook.
4. Add branch/loop cases only when proof ids make the accepted path inspectable.
5. Defer field-sensitive collection mutation and wrapper projection.

## Fallbacks

Emit persistent record update when shell ownership, last-use, operation mapping, or
field-path proof is missing.
