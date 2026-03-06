# Stage 10.1 Interim Baseline (Before 10.2)

Date: 2026-03-06  
Command:

```sh
cargo bench --bench wasm_exec -- bench_closure --noplot
```

## bench_closure (tests/run/bench_closure.tw)

- `no_mono`: `[343.29 µs 344.96 µs 346.88 µs]`
- `with_mono`: `[343.24 µs 344.09 µs 345.23 µs]`
- `with_typed_closure`: `[181.77 µs 183.39 µs 185.01 µs]`

## Median-based ratios

- `with_typed_closure` vs `with_mono`: `344.09 / 183.39 = 1.88x` faster
- `with_typed_closure` vs `no_mono`: `344.96 / 183.39 = 1.88x` faster
- `with_mono` vs `no_mono`: `344.96 / 344.09 = 1.00x` (roughly neutral)

## Notes

- Criterion printed very large "change" percentages because it compared against an old stored baseline from a different workload scale; ignore those for cross-stage interpretation.
- Use this file as the checkpoint baseline for Stage 10.2 comparison.
