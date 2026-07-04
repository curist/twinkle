# Performance Plans

This directory is the home for active performance tracking. It splits work by the
outcome being improved, while keeping focused subtracks nearby instead of
scattered across `docs/plans/`.

## Umbrella tracks

| Track | Goal | Start here |
|------|------|------------|
| Compiled-program performance | Make Wasm produced by Twinkle run faster. This is the primary runtime/user-program performance track. | [compiled-programs.md](compiled-programs.md) |
| Compiler performance | Make the self-hosted Twinkle compiler build programs faster. | [compiler.md](compiler.md) |

## Focused subtracks

| Subtrack | Role |
|---------|------|
| [vector/](vector/README.md) | `Vector<T>`, `sort_by`, typed vector representation, and dataframe `order_by` performance. |
| [dataframe/](dataframe/README.md) | Dataframe stress-test design, friction log, and app-level benchmark context. |

## Current runtime priority stack

1. **Typed container representation.** Continue moving hot monomorphic containers
   away from erased `anyref` storage and universal helper APIs. The current lead
   is `Vector<Int>` through the boundaries that real programs use. The
   architecture-parent plan is
   [backend-anyref-elimination.md](backend-anyref-elimination.md); the concrete
   `Vector<Int>` family lives in [vector/](vector/README.md).
2. **Vector/read/order-by path.** Dataframe `order_by` and generic key-index
   `sort_by` are still dominated by random boxed vector reads. The vector folder
   tracks the active probes and rejected approaches.
3. **Persistent runtime cleanup.** Incremental PVec and HAMT improvements remain
   useful when they remove repeated trie walks, per-element boxing, or avoidable
   helper work without changing semantics.
4. **Numeric/codegen improvements.** Keep AWFY-style numeric loops as the guard
   for generated-code quality, but do not repeat already-measured non-levers.
5. **Byte/string/Buffer paths.** Use linear-memory `Buffer` and allocation-free
   views where they remove a real boundary; avoid adding Buffer copies just to
   get faster interior reads.

## Measurement rules

- Prefer same-session A/B comparisons over isolated timings.
- Keep benchmark context with the plan that explains it.
- Record rejected approaches with the evidence, so future work does not retry
  shapes that were already measured.
- Treat archived perf docs as evidence, not as the active queue.

## Historical evidence

Completed or rejected investigations remain in [../archive/](../archive/). The
most relevant archived references are linked from the focused subtrack docs that
supersede them.
