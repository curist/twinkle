# Mutable Operation Catalog

**Status:** Draft catalog for first existing-hook lowering.

This doc names the operation families that codegen decisions may target after
analysis has proven a candidate. It is not a proof system; it maps an
already-proven candidate to a persistent fallback and an existing mutable target.

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

## Initial families

| Family | Persistent fallback | First mutable target | Notes |
|---|---|---|---|
| Vector indexed update | `Vector.set_at` / index rebinding lowered to persistent vector update | Existing vector in-place set helper | First emitted slice. Requires `Unique` + last-use of the vector backing. |
| Vector builder region | Persistent append/build shape or existing non-optimizer builder use | Existing `vector$builder_*` hooks | Separate from indexed update; preserve `collect` builder lowering independent of optimization. |
| Dict set | Persistent HAMT `Dict.set` | Existing dict in-place set helper | Must preserve old-version observability and insertion-order behavior. |
| Dict remove | Persistent HAMT `Dict.remove` | Existing dict in-place remove helper, if semantics are cataloged | Do not bundle with `set` until remove semantics and ordering are verified. |
| Record shell update | Persistent record update / field rebinding | Existing `ARecordUpdate.in_place` slot or equivalent backend hook | Shell reuse only; deep field collection ownership is a separate proof. |
| Ownership-specialized function variant | Generic function/callee | Cloned function keyed by canonical `VariantId` | Required for recursive and mixed-caller cases. Generic body stays the fallback. See [worked-examples Case V](../analysis/worked-examples.md#case-v--graph_sccvisit-the-whole-compiler-idiom-real). |
| Record-backed field collection update | Persistent projected-field collection update plus persistent record update | Existing dict/vector mutable helper plus record shell slot when separately licensed | Composed family for `env.types = ...`, `Set<K>`, transport wrappers, and Case V `cur.indices[...]`/`cur.stack = ...`. Field backing and shell reuse may be accepted independently. |

## Later families

These need migration work or broader policy after the first existing-hook slices:

- compiler-private mutable intrinsic regions;
- private tagged collection/record representations;
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
