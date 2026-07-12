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

## IR/debug expectations

`twk ir` ownership output should show:

- which locals are captured;
- whether the closure escapes;
- whether captured values are borrowed, published, or returned;
- why a mutable candidate was rejected or kept live across the closure boundary.

## Relationship to main architecture

This doc expands the closure-capture section in [architecture.md](architecture.md).
The default implementation should be conservative; recoverable closure coverage is
optional and should only follow after the printed facts are trusted.
