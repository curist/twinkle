# dataframe

An **app-scale stress test** for Twinkle. It pushes on API ergonomics and, by
running a realistic workload, surfaces where performance falls short — it is a
diagnostic, not a demonstration that Twinkle is fast.

It is a small columnar query engine — typed cells and columns, tables, CSV
parsing, filtering, group-by, joins, and order-by — built the way a real
application would be. At this scale the interesting pressure is different from
the leetcode set: fluent method chaining across modules, persistent collections
threaded through transformations, and hot paths like sort and gather.

This is the project that exposed Twinkle's generic `sort_by` as a bottleneck
(order-by is sort-bound), which is **why [`sort-bench`](../sort-bench) exists** —
to isolate that cost and drive the optimization work, which is still ongoing.
Ergonomics findings here also drove concrete fixes (e.g. the `Order` `Stringify`
fix).

## Layout

- `frame/` — the engine modules (`cell`, `column`, `table`, `csv`, `group`,
  `join`, `gen`).
- `tests/` — suites exercising the engine; run via `main.tw`.
- `bench/` — performance probes, with cross-language baselines (Clojure, Go,
  Rust) for the order-by / sort hot path.

## Run

```bash
target/twk run examples/dataframe/main.tw        # test suites
target/twk run examples/dataframe/bench/main.tw  # benchmarks
```

`assert.tw` and `runner.tw` are symlinks to the canonical harness in
`boot/tests/`.
