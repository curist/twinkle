# Internal Borrow/Effect Framework

**Status:** Framework landing page — analysis work is complete; active work is now on the
codegen and storage tracks (see the [top-level README](../README.md)). The completed Phases 0-6
ownership/provenance work is preserved as the historical phase log in
[phases-0-6-history.md](phases-0-6-history.md). The loan/effect checker that layered on top of it
has also **landed** (the copy-carrier borrow/effect engine + key-stream dedupe checker); its plans
are archived (see the references below).

## Purpose

This directory documents the proof-producing half of Twinkle's mutable-lowering
pipeline. The compiler keeps Twinkle's surface language immutable, but it may lower
ordinary immutable vector, dict, and record-update code to private mutable storage
when the framework proves that mutation is observationally safe.

The right mental model is an **internal borrow/effect framework** made of concrete
proof engines. It is not a user-visible borrow checker and it is not part of source
type checking: failed proofs do not reject programs. They explain why a candidate
must use persistent lowering.

## Framework shape

Typed ANF and its derived CFG are the shared substrate. The framework then combines
proof engines:

| Proof engine | Responsibility | Current status |
|---|---|---|
| CFG/liveness/validity | Block structure, loop/back-edge flow, binding validity, last-use, path death | Implemented in Phases 1-3; see [phases-0-6-history.md](phases-0-6-history.md) |
| Ownership/provenance | `Unique` / `Shared` / `Unknown`, fresh values, move-vs-alias, publication | Implemented in Phases 2-3; design in [fact-lattice.md](fact-lattice.md) |
| Path/region ownership | Record shell vs field backing, nested containers, return-path/payload transport | Implemented in Phases 4-5; see [records-fields.md](records-fields.md) and [summary-specialization.md](summary-specialization.md) |
| Summary/specialization | Borrowed/consumed/published params, return ownership, owned-entry variant decisions | Implemented as analysis facts in Phases 3 and 6; emitted variants remain codegen work |
| Loan/effect conflicts | Non-escaping reads as loans, writes as effects, compatibility/rejection proofs | In progress; see [../../archive/2026-07-24-ownership-borrow-effect-checker-plan.md](../../archive/2026-07-24-ownership-borrow-effect-checker-plan.md) |
| Diagnostic rendering | Accepted proof tokens and active rejection reasons for codegen decisions | Implemented incrementally by each proof engine |

Codegen consumes framework proof artifacts mechanically. It must not rediscover
ownership or borrow legality through a second recognizer path.

## Invariants

- Generated code changes only in codegen phases, not while adding analysis facts.
- ANF remains authoritative; CFG is a derived analysis/debug view over optimized
  ANF.
- Proof facts are the only source of mutability legality.
- Missing proof means `Unknown` or `Shared`, never speculative mutation.
- A failed borrow/effect proof is an optimization failure, not a type error.
- Runtime/storage-specific compatibility rules belong in internal effect proofs,
  not in the surface type checker.

## Why this is broader than ownership analysis

The original analysis track was named around sound uniqueness because the first
proof engines had to answer whether a value was uniquely owned. That remains
necessary, but it is not sufficient. Some safe mutable-lowering cases need a second
question:

> Given an owned value, are the temporary reads from it compatible with the later
> writes we want to perform in place?

For example, a dict copy-carrier shape can read from `next` and write through
`out := next`. Treating every read result as publication demotes the carrier to
`Shared`; treating every read as harmless is unsound because `Dict.keys` may expose
the runtime order vector. The loan/effect engine models `keys()` and `get()` as
loans, dict updates as write effects, and accepts only explicitly safe pairs.
`Dict.set_in_place` can be compatible with a precomputed ordered key stream, while
`Dict.remove_in_place` conflicts with a live `Dict.keys` loan under the current
runtime.

That makes borrow/effect checking the framework layer that coordinates the earlier
ownership, liveness, path, and summary proofs with operation-specific read/write
compatibility.

## Historical / reference plans (landed, archived)

These built the loan/effect layer on top of Phases 0-6. Both landed and are archived; kept here
as the design record. No active analysis work remains — see the codegen/storage tracks.

- [Internal borrow/effect framework implementation](../../archive/2026-07-24-ownership-borrow-effect-checker-plan.md)
  — built the first loan/effect conflict engine and integrated accepted proofs
  into ownership transfer.
- [Merge-targeted owned dict acceptance slice](../../archive/2026-07-24-merge-targeted-owned-dict-plan.md)
  — verified the motivating copy-carrier pattern without rewriting boot compiler
  source.

## Historical phase log

[phases-0-6-history.md](phases-0-6-history.md) records the completed ownership /
provenance phase ledger as of 2026-07-19, including the post-completion lattice
generalization cleanup. Keep that file as implementation history and regression
context. New architectural guidance should be added to this README or focused
subplans rather than extending the old phase ledger.

## Related design documents

- [../architecture.md](../architecture.md) — canonical sound mutable-lowering
  architecture.
- [fact-lattice.md](fact-lattice.md) — ownership/provenance lattice and transfer
  rules.
- [summary-specialization.md](summary-specialization.md) — function summaries and
  owned-entry specialization facts.
- [worked-examples.md](worked-examples.md) — real ANF cases that anchor the proof
  engines.
- [sound-analysis.md](sound-analysis.md) — required positive/negative coverage.
- [closure-capture.md](closure-capture.md) and
  [concurrency-publication.md](concurrency-publication.md) — focused publication
  refinements.
- [../codegen/README.md](../codegen/README.md) — codegen consumers of proof facts.
