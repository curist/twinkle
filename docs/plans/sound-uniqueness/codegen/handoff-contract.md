# Analysis-to-Codegen Handoff Contract

**Status:** Draft codegen contract.

This doc owns the first implementation seam between ownership analysis and
backend lowering. The later intrinsic family in
[../migration/mutable-intrinsics.md](../migration/mutable-intrinsics.md) builds on
this seam, but the first codegen track should target existing hooks.

## Governing rule

**Absence, ambiguity, or staleness of a decision emits the persistent operation.**
Codegen never guesses mutation. A mutable path is opt-in per ANF site and must be
licensed by a live decision.

## Central selection layer

Phase 7C should define one catalog-driven selector/helper before any family starts
emitting mutable code. Backend lowering passes the ANF site, operation family,
decision table, catalog entry, and persistent fallback to that layer. The layer
returns either:

- the exact mutable target plus argument mapping licensed by a live decision; or
- the ordinary persistent fallback with a reason such as absent, stale,
  unsupported, or dry-run.

Per-family backend sites must not duplicate ownership legality, last-use,
staleness, unsupported-family, or fallback checks. They may perform only the local
mechanical emission for the target the selector returned.

## First-cut decision record

Each accepted mutable-lowering candidate should carry:

| Field | Purpose |
|---|---|
| ANF key | The optimized-ANF site the decision applies to. |
| Operation family | Vector indexed update, vector/string builder region, dict set/remove, record shell update, record-backed field update, ownership-specialized function variant, etc. |
| Source value | The collection/record value whose storage or shell may be reused. |
| Result binding | The local that receives the post-update immutable value. |
| Proof requirements | Required ownership fact, last-use proof, and any loop/branch/SCC proof id. |
| Persistent fallback | The ordinary immutable operation or generic callee to emit when the decision is absent or rejected. |
| Mutable target | The existing helper/hook or cloned function selected by the operation catalog. |
| Argument mapping | How persistent-call operands map to mutable-target operands. |
| Variant key | The exact `VariantId` when the decision depends on an owned-specialized callee/body. Empty for purely local decisions. |
| Field/path key | The consumed shell/field path for record-backed collection decisions. Empty for whole-value decisions. |
| Debug id | Stable id for `twk ir`, census, and WAT/call inspection. |

## Analysis obligations

Analysis certifies facts; codegen consumes them without re-checking:

- Existing in-place/helper decisions require both `Unique` and last-use. `Unique`
  alone is not sufficient.
- Builder decisions must name the region boundaries and the helper sequence that
  is legal to emit.
- Record decisions must distinguish shell reuse from deep field/backing-storage
  mutation.
- Field-path and return-path handoffs must be explicit; codegen must not infer
  ownership transfer from record or variant shape.
- Variant-qualified diagnostics certify only the named `VariantId`; they do not
  certify the generic function body or any other ownership key.

## Codegen obligations

Given a live, well-formed decision, codegen may select the named mutable target.
It must not:

- re-run uniqueness, field-ownership, liveness, last-use, or escape analysis;
- promote a site to mutation without a decision;
- silently reinterpret a decision for a different operation family;
- emit mutation for stale keys or mismatched operand shapes;
- route a call to an ownership-specialized clone unless the decision names the
  exact `VariantId` and the clone was generated for that key.

## Staleness checks

A decision is stale or unusable if:

- its ANF key no longer resolves in the artifact being lowered;
- the resolved op family differs from the recorded operation family;
- the source/result locals do not match the recorded shape;
- argument mapping cannot be applied exactly;
- the mutable target is unavailable for the monomorphized types involved;
- the recorded `VariantId` no longer matches an accepted reachable variant;
- a field/path decision is applied to a different projected field, shell, or result
  local than the recorded ANF shape.

Every stale/unusable case falls back to the persistent target and should be
visible in debug output.

## Variant routing records

Ownership-specialized function codegen needs a second record shape in addition to
local mutable-op decisions:

| Field | Purpose |
|---|---|
| Generic function id | The original function that remains the fallback. |
| VariantId | The canonical owned key, e.g. `visit[unique:p0]`. |
| Clone function id/name | The generated function body for that key. |
| Route sites | ANF call keys that may call the clone. |
| Recursive routes | In-SCC calls that must route to the same clone/peer clone. |
| Fallback reason | Why a site stayed generic: absent proof, stale key, over cap, unsupported family, ambiguous key. |
| Proof/debug id | Link back to the `twk ir --cfg` variant-qualified diagnostic section. |

For [worked-examples Case V](../analysis/worked-examples.md#case-v--graph_sccvisit-the-whole-compiler-idiom-real), this means `fn visit` stays generic, while
`variant fn visit [unique:p0]` can become a cloned function whose recursive calls
route through the same owned key. Applying those shell-reuse verdicts to the
generic `visit` body would be a stale/ambiguous decision and must fall back.

## Dry-run before emission

Before any optimized helper or cloned variant is emitted, the backend should be
able to run in a mode where it looks up decisions, reports whether they would be
used, and still emits persistent code. This isolates side-table plumbing from
semantic lowering.
