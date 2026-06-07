# Dataframe / query engine — Twinkle stress test

**Date:** 2026-06-07
**Status:** design approved, plan pending

## Purpose

`tools/leetcode/` proved Twinkle's ergonomics on small, self-contained problems and
converged on essentially one recurring gap (the now-solved tuple/pair type). LeetCode
problems are single-file, no I/O, int-heavy, and small-input — they push the language
only so far. The self-hosted boot compiler is the *deepest* stress test we have, but it
stresses **compiler correctness**, not application-level ergonomics.

This project fills the gap: a real, multi-module **columnar dataframe / query engine**
written in Twinkle and exercised over large synthetic datasets. It targets two
dimensions explicitly:

1. **App ergonomics & stdlib breadth** — multi-module architecture, capability records
   as the trait substitute, generics + closures, pattern matching, `Result`/`try`, and
   the immutable-rebind threading idiom, all at app scale.
2. **Performance at scale** — unboxed `Vector<Int>`/`Vector<Float>` columns (PVec),
   `Dict`-HAMT group keys, `sort_by` over index vectors, hash joins, and PVec
   random-access gather, over ~1M-row inputs.

**Deliverables are two things:** the working engine, *and* a friction-log document
(mirroring the leetcode friction log in memory) capturing ergonomic gaps and perf
cliffs discovered along the way. The friction log is the actual product of the stress
test.

## Architecture

Its own project root `examples/dataframe/` (parallels `tools/leetcode/`, which has its own
`twinkle.toml`). Copies `assert.tw` + `runner.tw` from `boot/tests/` like leetcode does.
Library modules live under `frame/`, correctness tests under `tests/`, benchmark harness
under `bench/`.

```
examples/dataframe/
  twinkle.toml            name = "dataframe"
  assert.tw  runner.tw    copied from boot/tests (same pattern as leetcode)
  frame/
    column.tw   ColData enum + Column record (data + null mask); typed builders; getters; dtype; map
    table.tw    Table record; from_columns; schema (names/dtypes/nrows); select/drop/rename; head/slice; display; take (gather)
    row.tw      RowRef view + typed accessors (.int/.float/.str/.bool/.is_null) for predicates
    csv.tw      CSV string -> Table, per-column type inference
    query.tw    filter, with_column (derive), order_by
    group.tw    Aggregation capability record + builtins (count/sum/mean/min/max) + group_by().agg()
    join.tw     hash join (inner + left)
    gen.tw      seeded PRNG synthetic-data generator for benches
  tests/        per-module correctness suites + main.tw runner.run_all([...])
  bench/main.tw generate N rows, time group_by/sort/join via date.now()
```

Module paths resolve from the root, e.g. `use frame.column`, `use frame.table`.

### Note on `json.tw` reuse

`boot/lib/json.tw` lives under the `boot/` project root, so a separate
`examples/dataframe/` root **cannot import it**. CSV is therefore the primary loader
(and exercises `String`/`Byte`/utf8 handling on its own). JSON ingest is deferred; if
added later it means vendoring (copying) `json.tw` into the project — itself a
cross-module-reuse data point worth noting.

## Core types

```tw
type ColData = { IntCol(Vector<Int>), FloatCol(Vector<Float>), StrCol(Vector<String>), BoolCol(Vector<Bool>) }
type Column  = .{ data: ColData, nulls: Vector<Bool> }      // nulls[i]=true => missing
type Table   = .{ names: Vector<String>, cols: Vector<Column>, nrows: Int }
type Cell    = { CInt(Int), CFloat(Float), CStr(String), CBool(Bool), CNull }   // scalar at API edges
```

Storage is columnar and unboxed (`Vector<Int>` is a dense i64 PVec). `Cell` is the boxed
scalar only at API boundaries: aggregation outputs, display, and single-value access.

### Null mask

Each `Column` keeps its dense `ColData` *plus* a parallel `Vector<Bool>` null mask
(`true` = missing), matching how Arrow/pandas separate values from validity. This
preserves the columnar perf win and forces real null propagation: aggregations skip
masked rows; `filter`/`order_by`/`join` carry the mask through `take`.

## The key primitive: `take`

```tw
fn take(t: Table, idx: Vector<Int>) Table   // gather every column + its null mask by index
```

`filter`, `order_by`, and `join` all reduce to **compute an index vector, then `take`**:

- `filter(pred)` — scan rows, collect matching indices, `take`.
- `order_by(col, dir)` — `sort_by` an index vector by the keyed column, `take`.
- `join(other, on, how)` — build a hash map of probe-side keys, produce paired index
  vectors, `take` from each side, concat columns.

`take` is the main perf-critical path (PVec random access is O(log32 n) per element), so
it is the most likely place to surface a perf cliff — which is a desired outcome, not a
problem to design around.

## The capability-record centerpiece (no-trait stress)

An aggregation is a **uniform record** so heterogeneous aggregations coexist in one
`Vector` (the interesting consequence of having no traits and no existentials):

```tw
type Aggregation = .{ name: String, apply: fn(Column) Cell }   // closes over source column + reduction; nulls skipped
// builtins are constructors:
agg.sum("spend")   agg.mean("age")   agg.count()   agg.min("x")   agg.max("x")
```

`group_by([keys]).agg([...])` builds a composite group key per row into a `Dict`-HAMT,
then runs each `Aggregation` over each group's column slice. This is the explicit-
capability idiom used in anger, and `mean` exercises float math. Comparator closures for
`order_by` are the second capability-record surface.

## Data flow (example)

```
from_csv  ->  filter (idx -> take)  ->  group_by (HAMT keys)  ->  agg (Aggregation records)
          ->  order_by (sort_by -> take)  ->  display
```

## Error handling

`from_csv` returns `Result<Table, String>` and uses `try` for malformed rows and
type-inference conflicts. Column-name lookups and dtype mismatches in ops return
`Result`. Out-of-range/dtype errors that indicate a program bug may trap.

## Stress mapping

**App ergonomics & stdlib breadth**
- ~8 interdependent modules sharing nominal `Column`/`Table`/`Cell` types across boundaries.
- Capability records: `Aggregation`, comparator closures.
- Generics + closures: `filter`/`with_column`/`order_by` take closures; `RowRef` accessors.
- Pervasive enum-tag pattern matching over `ColData`/`Cell`.
- `Result`/`try` for ingest and op errors.
- Immutable rebind threading the `Table` through the fluent chain.

**Performance at scale**
- Unboxed `Vector<Int>`/`Vector<Float>` columns (PVec).
- `group_by` -> Dict-HAMT keyed by composite group key over ~1M rows.
- `order_by` -> `sort_by` over an index vector.
- Hash join -> Dict build + probe.
- `take`/gather -> PVec random access (cliff-hunting).
- Benchmarked with `date.now()` at scaling `N`, run via `target/twk run bench/main.tw`.

## Phasing

1. **Core** — Column/ColData/Cell + null mask; Table + from_columns/select/drop/rename/
   display + `take`. Tests.
2. **Ingest & derive** — `csv.tw` with per-column type inference; `filter`; `with_column`;
   `RowRef`. Tests.
3. **Sort & group/agg** — `order_by`; `Aggregation` + builtins; `group_by().agg()`. Tests.
4. **Join** — hash inner/left join. Tests.
5. **Bench & friction log** — `gen.tw`; `bench/main.tw` timing group_by/sort/join across
   scaling N; write the friction-log doc capturing ergonomic gaps + perf cliffs.

**Optional later:** JSON ingest (vendor `json.tw`), `stddev`/quantile aggs, multi-key joins.

## Out of scope (YAGNI)

- SQL/string query frontend (fluent API only).
- Real file I/O plumbing beyond reading a CSV string; data is generated or embedded.
- Concurrency, lazy/streaming execution, query optimization/planning.
