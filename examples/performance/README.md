# Performance Examples and Benchmarks

This directory gathers benchmark suites, probes, and app-scale stress tests used
to understand and improve Twinkle performance.

These programs are diagnostics: they are meant to expose bottlenecks and guard
against regressions, not to demonstrate that Twinkle is already fast.

## Contents

| Directory | Role |
|-----------|------|
| [awfy/](awfy/README.md) | AWFY-style cross-language suite for broad generated-code/runtime gaps. |
| [compiler/](compiler/README.md) | Compiler/runtime microbenchmarks that used to live under `boot/bench/`: PVec, Dict, Set, heap, and JSPI probes. |
| [crypto-bench/](crypto-bench/README.md) | Crypto/base64 byte-boundary benchmarks across Twinkle and native runtimes. |
| [dataframe/](dataframe/README.md) | App-scale columnar dataframe stress test and order-by benchmark context. |
| [sort-bench/](sort-bench/README.md) | Focused vector/read/sort probes spun out of the dataframe order-by bottleneck. |

## Related docs

- Runtime/user-program performance roadmap:
  [docs/plans/performance/compiled-programs.md](../../docs/plans/performance/compiled-programs.md)
- Compiler throughput roadmap:
  [docs/plans/performance/compiler.md](../../docs/plans/performance/compiler.md)
- Vector/order-by subtrack:
  [docs/plans/performance/vector/](../../docs/plans/performance/vector/README.md)
- Dataframe stress-test notes:
  [docs/plans/performance/dataframe/](../../docs/plans/performance/dataframe/README.md)

## Common commands

```bash
make awfy
make bench
make bench-guard
examples/performance/crypto-bench/run.sh
target/twk run examples/performance/dataframe/bench/main.tw
target/twk run examples/performance/sort-bench/sort_by_component_probe.tw
```
