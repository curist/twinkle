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

## First-cut decision record

Each accepted mutable-lowering candidate should carry:

| Field | Purpose |
|---|---|
| ANF key | The optimized-ANF site the decision applies to. |
| Operation family | Vector indexed update, vector builder, dict set/remove, record shell update, etc. |
| Source value | The collection/record value whose storage or shell may be reused. |
| Result binding | The local that receives the post-update immutable value. |
| Proof requirements | Required ownership fact, last-use proof, and any loop/branch proof id. |
| Persistent fallback | The ordinary immutable operation to emit when the decision is absent or rejected. |
| Mutable target | The existing helper/hook selected by the operation catalog. |
| Argument mapping | How persistent-call operands map to mutable-target operands. |
| Debug id | Stable id for `twk ir`, census, and WAT/call inspection. |

## Analysis obligations

Analysis certifies facts; codegen consumes them without re-checking:

- Existing in-place/helper decisions require both `Unique` and last-use. `Unique`
  alone is not sufficient.
- Builder decisions must name the region boundaries and the helper sequence that
  is legal to emit.
- Record decisions must distinguish shell reuse from deep field/backing-storage
  mutation.
- Future field-path and return-path handoffs must be explicit; codegen must not
  infer ownership transfer from record or variant shape.

## Codegen obligations

Given a live, well-formed decision, codegen may select the named mutable target.
It must not:

- re-run uniqueness, field-ownership, liveness, last-use, or escape analysis;
- promote a site to mutation without a decision;
- silently reinterpret a decision for a different operation family;
- emit mutation for stale keys or mismatched operand shapes.

## Staleness checks

A decision is stale or unusable if:

- its ANF key no longer resolves in the artifact being lowered;
- the resolved op family differs from the recorded operation family;
- the source/result locals do not match the recorded shape;
- argument mapping cannot be applied exactly;
- the mutable target is unavailable for the monomorphized types involved.

Every stale/unusable case falls back to the persistent target and should be
visible in debug output.

## Dry-run before emission

Before any optimized helper is emitted, the backend should be able to run in a
mode where it looks up decisions, reports whether they would be used, and still
emits persistent code. This isolates side-table plumbing from semantic lowering.
