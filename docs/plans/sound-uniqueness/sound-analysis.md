# Sound Ownership Analysis — Required Coverage

**Status:** Coverage matrix (the algorithm lives in
[fact-lattice.md](fact-lattice.md) + [summary-specialization.md](summary-specialization.md))

## Purpose

Define the **required coverage** for the sound ownership analysis: the positive
patterns it should eventually optimize and the negative patterns it must reject.
This is the checklist the algorithm is measured against.

The algorithm itself is now designed elsewhere:

- [fact-lattice.md](fact-lattice.md) — the affine ownership lattice, the
  per-`AnfOp` transfer function, the `AInit` move/alias hinge, and control-flow
  merges (branch/match joins, loop back-edges, multi-exit publication).
- [summary-specialization.md](summary-specialization.md) — function summaries,
  SCC-ordered computation, and field-path-granular call-site variant selection.
- [worked-examples.md](worked-examples.md) — the real ANF cases both are validated
  against.

This document stays focused on *what must be covered*, not *how*.

## Relationship to the CFG ownership view

The analysis is expected to rely on the CFG ownership view for local control-flow
structure:

- branch joins;
- loop back-edges;
- loop-carried values;
- value-carrying `break` exits;
- publication edges;
- ANF-node mappings for debug output and codegen decisions.

The CFG view does not by itself solve whole-program ownership. It provides the
intra-function control-flow substrate. The sound analysis still needs function
summaries, call-site reasoning, and a deterministic whole-program fixpoint or
worklist for interprocedural cases.

## Required proof outcomes

For each candidate value/update, the analysis should classify:

- **owned / mutable-eligible** — destructive lowering is sound;
- **borrowed temporarily** — reads are allowed but do not publish the value;
- **published / escaped** — mutable region must end;
- **old version observable** — persistent operation required;
- **unknown** — fallback to persistent operation;
- **rejected with reason** — debug output explains why mutation was not licensed.

Unknown must never be treated as maybe-owned.

## Vector patterns to cover

Initial positive patterns:

- fresh vector updated with `set_at` and returned/published only after updates;
- loop-carried vector updated across back-edges;
- interleaved non-escaping element reads, as in `sieve` and `bounce`;
- append/build accumulator loops;
- method-wrapper forms such as `xs.set_at(i, v)`;
- owned vector passed to a callee that has an owned-specialized variant.

Required negative patterns:

- old vector alias remains observable after update;
- vector shared through `slice`, `concat`, view/window, or other sharing op;
- vector stored in record/variant/vector/dict/global/`Cell` before update;
- vector captured by escaping closure/task/fiber;
- vector sent through `Channel<T>`;
- vector passed to unknown or publishing callee;
- inner vector projected from `Vector<Vector<T>>` without proof that the inner
  vector is owned.

## Dict patterns to cover

Initial positive patterns:

- fresh `Dict.new()` threaded through `Dict.set` / `Dict.remove` updates;
- loop-carried dict accumulator updates;
- env/registry-style dict fields updated through owned records;
- dict returned/published only after update region completes;
- owned dict passed to a callee that has an owned-specialized variant.

Required negative patterns:

- old dict alias remains observable after update;
- dict stored in record/variant/vector/dict/global/`Cell` before update;
- dict captured by escaping closure/task/fiber;
- dict sent through `Channel<T>`;
- dict passed to unknown or publishing callee;
- dict value projected from `Dict<K, Dict<...>>` or `Dict<K, Vector<...>>`
  without proof that the inner value is owned;
- updates that would break insertion-order semantics.

## Record patterns to cover

Records need both shell ownership and field-sensitive ownership.

Initial positive patterns:

- fresh record shell updated in place when the old shell is unobservable;
- owned record with owned collection fields projected for mutation;
- record field rebinding that updates a deeply owned field;
- transport-wrapper records (`.{ ..., ctx/state/env }`) whose returned fields
  carry ownership from consumed input accumulators and are projected by the
  caller;
- variant-wrapped transport records (`Result<SourceLoad, AnalysisError>`-style)
  whose locally handled payload fields carry state ownership through match arms;
- wrapper records such as `Set<K>` projecting to owned `Dict<K, Void>`;
- owned record passed to a callee that has an owned-specialized variant.

Required negative patterns:

- fresh record shell containing shared collection fields;
- record update that preserves shared fields but then treats the result as deeply
  owned;
- old record alias remains observable after shell or field update;
- field projected from shared or unknown record;
- record captured by escaping closure/task/fiber;
- record sent through `Channel<T>`;
- record stored in escaping aggregate/global/`Cell` before mutation.

## Nested collection patterns

The analysis must distinguish outer ownership from inner ownership:

- `Vector<Vector<T>>` outer vector owned, inner vectors unknown/shared;
- `Dict<K, Vector<V>>` dict owned, values unknown/shared;
- records/variants/array literals wrapping shared collections;
- field/element/value projections that borrow vs transfer ownership.

The first implementation may reject most inner mutation. It must still explain
that the outer shell is owned while the projected inner value is not proven owned.

## Caller-shape patterns

The same function may need different outcomes depending on caller facts.

Required caller distinctions:

- caller passes fresh owned value and never observes old version;
- caller passes a value that is aliased locally before the call;
- caller passes a function parameter whose ownership is unknown;
- caller passes an owned record but only one field is relevant;
- caller passes owned value to a callee that consumes it and returns owned result;
- caller receives ownership through a returned record field (`out.ctx`,
  `out.state`, `out.env`, etc.) while reading sibling fields from the same
  wrapper;
- caller receives ownership through a locally handled variant payload field
  (`Ok[0].state`, `Err[0].state`), with branch joins preserving ownership only
  when every continuing arm carries it;
- caller passes value to a callee that may publish it;
- multiple call sites need both generic persistent and owned-specialized variants.

The analysis must support demand-driven, deterministic, capped specialization.
The cap must account for type monomorphization multiplied by ownership variants.

## Whole-program requirements

The analysis must eventually reason across the linked, monomorphized program:

- build function summaries for ownership requirements and effects;
- propagate call-site facts into callee variant selection;
- compute summaries/variants with deterministic worklist order;
- avoid relying on hash-map iteration order for decisions or emitted ids;
- keep generic persistent fallback variants available;
- explain why each call site selected an owned-specialized or persistent callee.

Whole-program analysis does not mean whole-program theorem proving. Unknown,
recursive, mutually recursive, external, host, or effect-opaque cases can fall
back to persistent behavior.

## Publication sinks to model

At minimum:

- return or value-carrying `break` that publishes outside the mutable region;
- storage in record/variant/vector/dict/global;
- storage into a `Cell<T>` (the explicit mutable escape hatch) via
  `Cell.new`/`set`/`update` — the value escapes into a mutable, arbitrarily-
  aliasable, arbitrarily-timed-read box; correspondingly, `Cell.get` yields a
  non-owned value;
- closure capture;
- task/fiber capture;
- `Channel<T>` send;
- unknown *Twinkle* callee (no summary);
- module/global publication;
- cross-worker shared transfer;
- old-version alias that remains observable.

Cross-worker serialized copy should be modeled separately from shared transfer
when the runtime contract guarantees copying.

**Synchronous extern/host calls are *not* in this list.** Unlike an unknown
Twinkle callee, the extern boundary is a closed allow-list marshalled by a copying
bridge (`Vector<Byte>`/`Vector<String>` args copied out, results host-constructed
fresh, no retained GC reference), so a GC-typed extern argument is a **borrow** and
an extern result is **owned-fresh** — not a publication. See the extern row in
[fact-lattice.md](fact-lattice.md) and the resolution in
[concurrency-publication.md](concurrency-publication.md). Only an extern that
retains a GC argument past the call falls back to publication.

## Debug output requirements

For every candidate, `twk ir` ownership output should show:

- current ownership class;
- relevant borrow/read sites;
- publication sinks;
- loop-carried facts if inside a loop;
- field/inner projection facts if relevant;
- call-site summary or specialization decision;
- return-path ownership facts for record and variant transport wrappers and the
  field/payload projection move/borrow verdict;
- accept/reject verdict;
- proof/debug id used later by codegen.

## Design references

The algorithm that satisfies this coverage is specified in
[fact-lattice.md](fact-lattice.md) (lattice, transfer function, control-flow
merges) and [summary-specialization.md](summary-specialization.md) (summaries,
SCC fixpoint, call-site specialization), validated against
[worked-examples.md](worked-examples.md).

The **coverage patterns below are Twinkle-specific** — they come from the census
of real boot code, not from the literature. But the *techniques* for the
sub-problems (escape analysis, region/uniqueness inference, borrow-style
loop-carried lifetimes) are proven prior art; see the **Prior art and further
reading** section of [design-rationale.md](design-rationale.md) for the reading
list. The takeaway matches the gradient framing: no single paper is a blueprint,
but each piece we assemble has a canonical algorithm worth borrowing rather than
reinventing.
