# Dataframe Performance and Stress-Test Notes

> **Retired (2026-09-03).** This stress test delivered its findings and has been
> archived — there is nothing left to *build* in the plan itself. Its two
> outcomes now live where they belong: the **ergonomics friction** (fluent method
> chains don't cross module boundaries, circular imports are a hard error,
> `Result`-returning ops break chaining) is durable and captured in
> [friction-log.md](friction-log.md); the **performance finding** (`order_by` is
> sort-bound) drove `sort-bench` and the still-active vector/sort track at
> [../../performance/vector/](../../performance/vector/README.md). The working
> engine and benchmarks remain a live regression/bench asset at
> `examples/performance/dataframe/` — only the plan docs are archived.

This folder keeps the dataframe stress-test docs near the performance track they
inform. The working code lives in `examples/performance/dataframe/`.

## Docs

| Doc | Role |
|-----|------|
| [stress-test.md](stress-test.md) | Original dataframe/query-engine stress-test design. |
| [stress-test-plan.md](stress-test-plan.md) | Implementation plan for the stress-test project. |
| [friction-log.md](friction-log.md) | Findings from building the dataframe engine: language ergonomics and performance cliffs. |

## Performance role

The dataframe project is the app-scale workload that ties together several
runtime performance tracks:

- typed `Vector<Int>` / `Vector<Float>` columns;
- variant-held column data such as `IntCol(Vector<Int>)`;
- `sort_by` over row-id vectors for `order_by`;
- random key reads and gather/take paths;
- HAMT-backed grouping and joins.

The active vector/order-by work is tracked in [../vector/](../vector/README.md),
with the umbrella runtime roadmap in [../compiled-programs.md](../compiled-programs.md).
