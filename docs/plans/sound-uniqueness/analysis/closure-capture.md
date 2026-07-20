# Closure Capture and Uniqueness

**Status:** Draft subplan

## Purpose

Define how closure capture interacts with sound ownership analysis for mutable
lowering.

Closure capture is both a soundness hazard and a future coverage opportunity:
escaping captures must block mutation, but non-escaping closures may eventually
be treated as temporary borrows or summarized callees.

## Baseline rule

Closure capture is a publication sink by default.

If a closure captures an owned collection or record, the mutable region must end
unless the compiler proves the closure is non-escaping and called in a scope
where the borrow remains temporary.

Hard blockers in the initial analysis:

- closure value is returned;
- closure value is stored in a record, variant, vector, dict, global, task, or
  channel;
- closure is passed to an unknown callee;
- closure may outlive the current mutable region;
- closure captures an old version that remains observable after an update.

## Recoverable future cases

The first sound implementation may reject all captures. Later work can recover
coverage for:

- immediately-called closures that do not escape;
- closures inlined before ownership analysis;
- closure summaries that state captured values are only read/borrowed;
- higher-order helpers whose callback is known not to escape.

The analysis must distinguish these states in printed IR:

- `blocked: escaping closure capture`
- `blocked: unknown closure escape`
- `borrow: non-escaping closure capture`
- `summary: closure reads only`

## Resolving indirect call targets (control-flow analysis)

The recovery cases above ("closure summaries that state captured values are only
read", "callback known not to escape") name a *summary* the analysis wants to
borrow at an indirect call site — but they leave out the step that *produces* an
applicable summary. This section is that missing mechanism.

There are **two distinct edges**, and only one is covered by the baseline rule:

- **Closure-as-a-value-that-captures** — `let f = fn() { …xs… }`. Handled above:
  if `f` escapes, `xs` publishes.
- **The indirect call itself** — `let g = pick(); let L = call g(xs)`. The
  *callee* is a runtime funcref, so with no target information the argument `xs`
  hits the generic `ACall(unknown/unsummarized)` rule
  ([fact-lattice.md](fact-lattice.md)) → every arg `Shared`, `L ← Unknown`. This
  is the edge the baseline over-approximates, and the one CFA recovers.

**The recovery step:** a control-flow / defunctionalization pass computes, for each
indirect call site, the **finite set of concrete functions the funcref can point
to**. When that set is known and non-empty, the call borrows the **meet (most
conservative merge) of those targets' summaries** instead of falling to
`unknown` — exactly the summary machinery in
[summary-specialization.md](summary-specialization.md), sourced from a resolved
target set rather than a syntactic callee. A `Borrowed`-in-all-targets parameter
then stays a borrow (arg fact preserved); a parameter `Published`/`Consumed` in
*any* possible target must take the conservative merge (publish). If the target
set is unknown, partial, or escapes analysis, the call **stays** at the generic
`unknown` rule — sound fallback, no coverage lost relative to today.

Twinkle's own lowering makes this tractable: after monomorphization a closure is a
typed struct (`$closure_<sig>` with a funcref in field 0), so indirect sites are
already grouped by concrete Wasm signature, and many resolve to a small,
statically-enumerable target set (often a singleton). Harvesting that set is a
standard 0-CFA / defunctionalization analysis — **prior art, not new theory**
(see the reading list in [design-rationale.md](design-rationale.md)).

Scope discipline matches the rest of the plan: the **first** implementation may
skip CFA entirely and leave every indirect call at `unknown → publish`
(sound, zero coverage on this edge). CFA is a *coverage* extension layered on once
the printed facts are trusted — never a soundness precondition.

Printed IR should distinguish the resolution outcome:

- `resolved: indirect call → {f, g} (merged summary)`
- `blocked: unresolved indirect call (unknown target set)`

## IR/debug expectations

`twk ir` ownership output should show:

- which locals are captured;
- whether the closure escapes;
- whether captured values are borrowed, published, or returned;
- why a mutable candidate was rejected or kept live across the closure boundary.

## Relationship to main architecture

This doc expands the closure-capture section in [architecture.md](../architecture.md).
The default implementation should be conservative; recoverable closure coverage is
optional and should only follow after the printed facts are trusted.
