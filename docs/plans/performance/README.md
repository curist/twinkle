# Performance Plans

This directory is the home for active performance tracking. It splits work by the
outcome being improved, while keeping focused subtracks nearby instead of
scattered across `docs/plans/`.

## Umbrella tracks

| Track | Goal | Start here |
|------|------|------------|
| Compiled-program performance | Make Wasm produced by Twinkle run faster. This is the primary runtime/user-program performance track. | [compiled-programs.md](compiled-programs.md) |
| Sound uniqueness and mutable lowering | Rebuild the boot compiler's proof-driven mutation optimization so ordinary immutable `Vector`/`Dict`/record code reaches the performance class currently requiring Buffer/transient workarounds. | [../sound-uniqueness/](../sound-uniqueness/) |
| Compiler performance | Make the self-hosted Twinkle compiler build programs faster. | [compiler.md](compiler.md) |

## Focused subtracks

| Subtrack | Role |
|---------|------|
| [vector/](vector/README.md) | `Vector<T>`, `sort_by`, typed vector representation, and dataframe `order_by` performance. |

The dataframe stress test that motivated much of the vector/order-by work has
been **retired** (findings delivered); its docs live at
[../archive/dataframe/](../archive/dataframe/README.md), and the working engine
remains a bench asset at `examples/performance/dataframe/`.

## Current runtime priority stack

1. **Sound uniqueness and mutable lowering.** Rebuild the boot compiler's
   proof-driven optimization so normal immutable collection and record-update code can use private
   mutable `Vector`/`Dict`/record representations when ownership is proven. **The
   integer/bool write-heavy case has largely landed** (MutVec for `Int`/`Bool`/
   `Float`/`Byte`, plus interprocedural S4): AWFY `sieve` on the *persistent*
   path now matches — and slightly beats — its manual `*_mut` Buffer variant
   (~0.6 ms vs ~0.8 ms total), and `bounce`/`nbody` have closed much of the gap.
   Remaining: `Float`/`Byte` `set_in_place` write-routing and param-sourced thaw;
   `nbody`'s residual gap is float codegen, not persistence. Net direction holds —
   Buffer is no longer the recommended workaround for ordinary local collection
   updates, only for FFI / shared-memory / dense byte codecs.
2. **Typed container representation.** Continue moving hot monomorphic containers
   away from erased `anyref` storage and universal helper APIs. The current lead
   is `Vector<Int>` through the boundaries that real programs use. The
   architecture-parent plan is
   [backend-anyref-elimination.md](backend-anyref-elimination.md); the concrete
   `Vector<Int>` family lives in [vector/](vector/README.md).
3. **Vector/read/order-by path.** Dataframe `order_by` and generic key-index
   `sort_by` are still dominated by random boxed vector reads. The vector folder
   tracks the active probes and rejected approaches.
4. **Persistent runtime cleanup.** Incremental PVec and HAMT improvements remain
   useful when they remove repeated trie walks, per-element boxing, or avoidable
   helper work without changing semantics.
5. **Numeric/codegen improvements.** Keep AWFY-style numeric loops as the guard
   for generated-code quality, but do not repeat already-measured non-levers.
6. **Byte/string/Buffer paths.** Use linear-memory `Buffer` and allocation-free
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
