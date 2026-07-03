# Examples

Three kinds of examples live here.

## Single-file demos

The loose `*.tw` files each showcase one feature or idiom — records, enums,
closures, pattern matching, immutability, FFI, and so on. Run any of them
directly:

```bash
target/twk run examples/<name>.tw
```

They are intentionally small; browse the directory to see what is available.

## Stress-test projects

Self-contained projects (each with its own `twinkle.toml`) that exercise
Twinkle at scale to **surface** weaknesses in API ergonomics and performance and
drive fixes back into the compiler and standard library. They are diagnostics,
not claims that Twinkle is already good at these things.

- **`aoc/`** — a scaffolded Advent of Code workspace: a Makefile fetches puzzle
  inputs, generates the per-year/all-years test aggregators, and drives a
  TDD-style solving loop on the sample then the real input.
- **`leetcode/`** — API ergonomics on small, self-contained algorithm problems.
See each project's own README for details. They share the `assert.tw` /
`runner.tw` test harness via symlinks to `boot/tests` when needed.

## Performance suites and probes

Performance work lives under [`performance/`](performance/README.md):

- **`performance/awfy/`** — broad generated-code/runtime benchmark suite.
- **`performance/compiler/`** — compiler/runtime microbenchmarks formerly under
  `boot/bench/`.
- **`performance/crypto-bench/`** — byte/crypto boundary benchmarks.
- **`performance/dataframe/`** — a columnar query engine that stresses fluent APIs
  at application scale; it exposed generic `sort_by` as a bottleneck.
- **`performance/sort-bench/`** — focused probes spun out of the dataframe
  finding (sorting, dict, typed vector reads), with cross-language baselines.
