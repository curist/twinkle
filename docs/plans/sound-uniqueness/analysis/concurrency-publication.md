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

## Synchronous extern / FFI calls (the copy-vs-share distinction, resolved)

A synchronous `extern` call looks like an unknown callee, so the naive rule would
publish every argument. Fact-checking the actual boundary makes this **strictly
tighter** — for synchronous FFI the copy-vs-share question above is not
open, it is *resolved to copy*:

- **The boundary allow-list is closed and tiny** (`is_extern_safe_type`,
  `boot/compiler/resolver.tw`): only `Int`, `Float`, `Bool`, `String`, `Void`,
  `ExternRef`/`ExternRef?`, `Vector<Byte>`, `Vector<String>`, and
  `Result<Vector<Byte>,String>` cross. Arbitrary records/dicts/user vectors/
  closures are rejected at resolve time, so **no analyzed record/dict/generic
  vector can escape through a synchronous extern at all.**
- **The marshalling copies in both directions** (`tools/js_runtime/runtime.mjs`):
  a GC-typed *argument* is decoded into a host-owned copy synchronously
  (`decodeByteArray`/`decodeStringArray`, whose `.slice()` copy is deliberate) and
  the host retains **no** Twinkle GC reference; a GC-typed *result* is
  host-constructed fresh (`makeByteArray`/`makeStringArray`).

Consequences for the analysis (see the extern row in
[fact-lattice.md](fact-lattice.md)):

- a `Vector<Byte>`/`Vector<String>` **argument is a read-only borrow**, not a
  publication — the caller's value stays owned and mutable after the call;
- a GC-typed **return is `Unique`** (fresh, deeply-owned, unaliased),
  fully mutable-eligible downstream;
- scalars are ownership-neutral; `ExternRef` handles are host-owned and never
  alias a Twinkle collection.

**Soundness contract.** This borrow/fresh model holds *because* the auto-bridge
does the copying — it is a property of the marshalling layer, not the language. A
future extern that stashed a GC-typed argument (retained the reference past the
synchronous call, e.g. into a JS closure/global, or across a JSPI suspension
before decoding) would break it; such an extern must be modeled as publication.
Because the bridge marshals the closed boundary set uniformly, the contract holds
structurally for every extern today, but the analysis should key the borrow/fresh
treatment on "recognized copying extern" and fall back to publish for anything
outside that set.

**So the residual copy-vs-share ambiguity is confined to cross-worker / `Channel`
transport above**, where whole GC values genuinely move between execution contexts
and the copy is a runtime *guarantee to reason about* rather than an observed
synchronous fact. Synchronous FFI does not need that reasoning — it is already
copy by construction.

## IR/debug expectations

`twk ir` ownership output should show concurrency sinks as first-class reasons:

- `published: task capture`
- `published: channel send`
- `published: cross-worker shared transfer`
- `copied: cross-worker serialized send`
- `blocked: unknown concurrency call`
- `borrow: extern arg (copying marshaller)`
- `owned-fresh: extern result (host-constructed)`

## Relationship to main architecture

This doc expands the publication-boundary section in
[architecture.md](../architecture.md). The first implementation should be
conservative for Task/fiber and Channel paths; cross-worker serialized-copy
precision can be added when the runtime contract is explicit enough.
