# Native in-place `Vector.sort_by` — Approach A result

**Status:** rejected (archived). See the consolidated performance plan: [wasm-native-sort.md](../vector-perf/wasm-native-sort.md).

## What was tried

Approach A attempted to replace the prelude `Vector.sort_by` merge sort with an in-place quicksort-style algorithm over a freshly allocated, uniquely owned `Vector` buffer.

The hope was that:

```tw
buf[i] = v
```

would compile to in-place vector writes throughout the sort, avoiding persistent-vector allocation at every merge level while keeping the public API unchanged.

## Result

This did not work well enough to keep. The generated sort crossed helper and recursive call boundaries (`swap`, range sort helpers, insertion-sort helpers), and the uniqueness analysis did not preserve the required in-place guarantee through those calls. Writes fell back to the persistent copy-on-write path.

That made the dataframe `order_by` benchmark substantially slower rather than faster. The important lesson is qualitative: if the algorithm relies on many vector writes, those writes must be guaranteed dense/in-place by construction, not merely hoped for through uniqueness analysis.

## Decision

Do not continue this direction for `order_by` unless the uniqueness model changes substantially. The follow-up direction is Approach C: sort over an explicit dense scratch buffer, where writes are real mutable array writes and cannot fall back to persistent COW.

See: [native-sort-dense-merge.md](native-sort-dense-merge.md).
