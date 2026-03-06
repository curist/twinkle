# Stage 10.2 Collect Builder Benchmark Comparison

Date: 2026-03-06  
Command:

```sh
cargo bench --bench wasm_exec -- bench_collect_strategy --noplot
```

## Workloads

- `collect_builder`: `tests/run/bench_collect_iterator.tw`  
  Uses `collect x in my_range(1000)` (routes through Stage 10.2 builder path).
- `manual_push`: `tests/run/bench_manual_push_iterator.tw`  
  Uses `xs = xs.push(...)` inside a loop (persistent concat-per-append growth, representative of the pre-10.2 asymptotic pattern).

Both programs produce the same output (`999000`).

## Results

- `collect_builder`: `[756.73 µs 760.04 µs 764.56 µs]`
- `manual_push`: `[57.015 ms 57.110 ms 57.229 ms]`

## Ratio

- Median ratio: `57.110 ms / 760.04 µs = 75.14x` faster for `collect_builder`.

## Notes

- This is the most direct available A/B in the same codebase without checking out pre-10.2 sources.
- The earlier `bench_closure` fixture is range-collect dominated, so it is not a sensitive indicator for Stage 10.2 (general vector/iterator collect path).
