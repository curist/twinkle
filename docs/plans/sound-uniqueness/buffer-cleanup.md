# Buffer Cleanup After Sound Mutable Lowering

**Status:** Draft subplan

## Purpose

Define when and how to remove or shrink the explicit `@std.buffer` escape hatch
after ordinary immutable Twinkle code can compile to private mutable collection
paths.

`Buffer` was added as a workaround for workloads where persistent `Vector`/`Dict`
code could not yet reach mutation-like performance. The long-term goal is not to
make users choose `Buffer` for ordinary local collection updates; the compiler
should prove when mutation is safe and lower normal code to private mutable
internals.

## Relationship to sound uniqueness

This cleanup is a follow-up, not an early milestone. Do not remove or deprecate
`Buffer` until the sound-uniqueness work has an end-to-end path:

- printed ownership facts for relevant programs;
- mutable-intrinsic lowering for proven local update regions;
- ordinary AWFY `sieve`/`bounce`/`nbody` in the same performance class as the
  current `*_mut` variants where storage mutation is the bottleneck;
- correctness guards for aliasing, publication, closure capture, concurrency,
  records, and nested collections.

## What Buffer currently represents

`@std.buffer` is a manually managed linear-memory API. It is useful today because
it provides:

- flat mutable storage;
- unboxed primitive reads/writes;
- performance comparable to native mutable arrays for some write-heavy kernels;
- an escape hatch around persistent `Vector` path-copy costs.

It is also intentionally low-level:

- manual lifetime with `free()`;
- unchecked logical indexing;
- visible mutation and aliasing;
- separate semantics from ordinary immutable Twinkle values.

That makes it acceptable as temporary performance scaffolding, but undesirable as
the main user-facing answer for local collection-update performance.

## Cleanup policy

Buffer cleanup should happen in stages.

### Stage 1 — Stop recommending Buffer for ordinary local updates

Once normal `Vector`/`Dict`/record code reaches the mutation-like path for proven
owned regions, documentation should stop presenting `Buffer` as the recommended
solution for ordinary local update loops.

Keep `Buffer` documented only for cases that are still genuinely outside the
collection optimizer's scope, such as raw byte-oriented interop or deliberately
manual linear-memory work. Crypto should not be treated as a permanent Buffer
justification: `Vector<Byte>` should become fast enough for the standard crypto
workloads.

### Stage 2 — Retire benchmark workaround variants

The `*_mut` AWFY variants should remain until the ordinary variants are the
primary performance target and reliably reach the same class. Then:

- keep historical numbers in the plan/archive docs;
- remove or archive `sieve_mut`, `bounce_mut`, and `nbody_mut` as active
  benchmark rows;
- make ordinary AWFY rows the sole merge gate for those workloads.

Do not remove the variants while they are still needed as the practical upper
bound for compiler work.

### Stage 3 — Narrow or hide Buffer API surface

After ordinary collection code no longer needs Buffer for performance, decide
whether `@std.buffer` should be:

- kept as a low-level expert API for raw linear-memory use;
- moved behind an unstable/internal namespace;
- reduced to a smaller byte-buffer interop API;
- removed entirely.

This decision should be based on remaining real use cases, not on the old AWFY
workarounds.

## Non-goals

- Do not remove `Buffer` before ordinary immutable code reaches the target class.
- Do not use Buffer cleanup to avoid implementing sound uniqueness.
- Do not keep `Buffer` as the primary answer for local mutable vectors/dicts once
  the compiler can prove those regions itself.
- Do not conflate private compiler mutable intrinsics with the public Buffer API.

## Verification before cleanup

Before deprecating or removing any Buffer-facing API or benchmark variant, run:

```bash
target/twk test
make stage2
examples/performance/awfy/run.sh
```

Also compare ordinary and workaround variants from the same machine/session while
the workaround variants still exist:

```bash
target/twk run examples/performance/awfy/twinkle/main.tw
```

The cleanup gate is not exact equality. The ordinary variants should be in the
same performance class for the portions where storage mutation is the bottleneck,
and all checksums must match.

## Open questions

- Which Buffer use cases remain after ordinary collection updates optimize?
- What `Vector<Byte>` representation/codegen work is needed so standard crypto
  workloads no longer need Buffer for performance?
- Should raw byte interop workloads keep a small linear-memory API even if crypto
  and collection-update workloads no longer need it?
- Should the Buffer docs move from standard API documentation to an unstable or
  low-level internals section before full removal?
- How long should the `*_mut` AWFY variants remain archived as regression
  references after they stop being active benchmark rows?
