# Mutable Operation Catalog

**Status:** Verified against the boot compiler on `main` (2026-07-20, Phase 7A/7B).
Targets, builtin names, and ABIs below are checked against `boot/compiler/builtins.tw`
and `boot/compiler/opt/semantics.tw`; re-verify if those tables move.

This doc names the operation families that codegen decisions may target after
analysis has proven a candidate. It is not a proof system; it maps an
already-proven candidate to a persistent fallback and an existing mutable target.

**Ownership-driven emission state:** vector/dict in-place helpers, record
`can_reuse=true`, and optimizer-selected builder regions are not selected by any
current ownership decision — the old COW / builder-region rewrite pass was removed
(see [existing-hooks.md](existing-hooks.md)). Semantic builder lowering such as
`collect` still emits builder calls and stays independent of this catalog-driven
optimization path. The existing `in_place_equivalent` map in `opt/semantics.tw`
already encodes the persistent→mutable pairing for the vector/dict families and can
seed the catalog lookup.

## Source-level model

User code keeps one immutable API. For example, `xs.set_at(i, v)` is written as an
ordinary immutable update. Internally, a proven site may lower to a mutable target
instead of the persistent fallback.

## Catalog shape

Each family should define:

- persistent fallback operation/callee;
- mutable target hook/helper;
- source collection or record operand;
- result binding;
- operand mapping from persistent form to mutable target;
- proof requirements supplied by the codegen decision;
- unsupported cases that must remain persistent.

Catalog entries are data for a single selector/helper layer. Backend lowering sites
should ask that layer for “mutable target + argument mapping” or “persistent
fallback”; they should not duplicate ownership legality, staleness, or unsupported
case checks per module.

## Initial families

| Family | Persistent fallback (builtin) | First mutable target (builtin / ABI) | Operand mapping | Notes |
|---|---|---|---|---|
| Vector indexed update | `vector$set_unsafe` (`xs[i]=v` lowers here via `lower_core/lvalues.tw`) | `vector$set_in_place` — `[pvec?, i32, anyref] → [pvec]` | Identical shape → drop-in call-target swap; result binding unchanged | First emitted slice. Requires `Unique` + last-use of the vector backing. Already paired via `in_place_equivalent`. |
| Vector builder region | Persistent append/build shape selected by ownership optimization; existing semantic builder use stays separate | `vector$builder_new []→[arr]`, `builder_from [pvec?]→[arr]`, `builder_push [arr?, anyref]→[]`, `builder_freeze [arr?]→[pvec]`; typed `_i64`/`_bool` shims | Region: `from(base)` → `push*` → `freeze` | Separate from indexed update; preserve `collect` builder lowering independent of optimization. Runtime `builder_extend` exists but is not a first-cut boot builtin/shim target until cataloged. **8C first slice narrows this to empty-seed `builder_new` → `push*` → `freeze` only.** These regions stay **boxed** (the frozen accumulator is `AAssign`-rebound, which `route_typed_vec` does not follow as a copy edge → v-group escapes → boxed) — correct but not typed. Typed routing and non-empty `from(base)`/`builder_from` are deferred. See [builder-region-design.md](builder-region-design.md). |
| String builder region | Persistent `String.concat` accumulator shape | `string$builder_from [str?]→[sb]`, `builder_extend [sb?, str?]→[]`, `builder_freeze [sb?]→[str]` | Region: `from(base)` → `extend*` → `freeze` | Same region shape as vector builders but no string in-place mutation; only builder lowering is in scope. **8C first slice**: loop accumulator only, single post-loop freeze, all intra-region publication/early-exit paths rejected. See [builder-region-design.md](builder-region-design.md). |
| Dict set | `Dict.set` method | `dict$set_in_place` — `[dict?, anyref, anyref] → [dict]` | Same shape → call-target swap; result binding unchanged | Must preserve old-version observability and insertion-order behavior. Paired via `in_place_equivalent`. |
| Dict remove | `Dict.remove` method | `dict$remove_in_place` — `[dict?, anyref] → [dict]` | Same shape → call-target swap; result binding unchanged | ABI/pairing is verified; do not emit until remove semantics, ordering, and old-version observability are accepted for the slice. Paired via `in_place_equivalent`. |
| Record shell update | `ARecordUpdate(.., can_reuse=false)` → copy struct | `ARecordUpdate(.., can_reuse=true)` → `struct.set` + return same ref (`emit_record_update`) | Flip the `can_reuse` bool at the ANF node | Backend already honors the bool; no runtime helper needed. Shell reuse only; deep field collection ownership is a separate proof. |
| Ownership-specialized function variant | Generic function/callee | Cloned function keyed by canonical `VariantId` | Route exact accepted call sites from generic function id to clone id for the named `VariantId` | Required for recursive and mixed-caller cases. Generic body stays the fallback. See [worked-examples Case V](../analysis/worked-examples.md#case-v--graph_sccvisit-the-whole-compiler-idiom-real). |
| Record-backed field collection update | Persistent projected-field collection update plus persistent record update | Existing dict/vector mutable helper plus record shell slot when separately licensed | Apply field/backing decisions to the collection update and shell decisions to the record update independently | Composed family for `env.types = ...`, `Set<K>`, transport wrappers, and Case V `cur.indices[...]`/`cur.stack = ...`. Field backing and shell reuse may be accepted independently. |

## Later families

These need migration work or broader policy after the first existing-hook slices:

- compiler-private mutable intrinsic regions;
- private tagged collection/record representations;
- vector concat/extend lowering through runtime `builder_extend` once a boot builtin,
  ABI shim, and analysis decision shape are cataloged;
- multi-key ownership specialization beyond the current cap/fallback policy.

## Inspection requirement

Before backend emission uses a family, `twk ir`/census output should be able to
render the candidate as:

```text
site <debug-id>: <family> persistent=<fallback> mutable=<target> proof=<proof-id>
```

Rejected or unsupported sites should name the persistent fallback and the rejection
reason. Variant-family inspection must also render the generic callee, clone name,
canonical `VariantId`, route site, and fallback reason.
