# Concurrency Publication and Uniqueness

**Status:** Draft subplan

## Purpose

Define concurrency-related publication sinks for sound uniqueness analysis.

The optimizer must not mutate a value after it has been made observable by
another execution context. This matters for Task/fiber work, `Channel<T>`, and
future cross-worker communication.

## Baseline rule

Concurrency sinks are publication boundaries unless proven otherwise.

A mutable region must end before:

- spawning a `Task` or fiber that captures an owned collection or record;
- sending a collection or record through a `Channel<T>`;
- storing an owned value into shared task/channel state;
- passing an owned value to an unknown concurrency helper.

These are cross-fiber aliases: the sender may continue while the receiver or
spawned task observes the value.

## Channel sends

`Channel<T>` send publishes the sent value to another execution context. For
ownership analysis, treat it like storing into an escaping aggregate plus an
unknown reader.

A future analysis may recover precision only if it proves the send copies the
value, the sender cannot observe the old version, or the channel is statically
local and drained before mutation resumes. Those are optional future extensions;
the first implementation should block.

## Task/fiber capture

A spawned task or fiber that captures a value makes that value observable outside
the current sequential region. Capturing an owned vector, dict, record, or a
record field containing a collection must publish that value.

The analysis should distinguish:

- direct capture of the collection/record;
- capture of a wrapper record such as `Set<K>`;
- capture of a closure that itself captured the value;
- capture of an old version that remains observable after an update.

## Cross-worker sends

Cross-worker communication may serialize/copy values instead of sharing Wasm-GC
references. That distinction matters:

- shared reference transfer is publication/aliasing;
- serialized copy is publication of a copy, not mutable aliasing of the original.

The analysis should model cross-worker sends explicitly rather than lumping them
into ordinary unknown calls. If the runtime guarantees serialization, mutation of
the sender's original value may remain sound after the copy boundary, subject to
ordinary old-version observability.

## IR/debug expectations

`twk ir` ownership output should show concurrency sinks as first-class reasons:

- `published: task capture`
- `published: channel send`
- `published: cross-worker shared transfer`
- `copied: cross-worker serialized send`
- `blocked: unknown concurrency call`

## Relationship to main architecture

This doc expands the publication-boundary section in
[architecture.md](architecture.md). The first implementation should be
conservative for Task/fiber and Channel paths; cross-worker serialized-copy
precision can be added when the runtime contract is explicit enough.
