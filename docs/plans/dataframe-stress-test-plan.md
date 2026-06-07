# Dataframe / Query Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a multi-module columnar dataframe / query engine in Twinkle (`examples/dataframe/`), exercised over large synthetic datasets, to stress-test app-level ergonomics + stdlib breadth and collection performance — producing both a working engine and a friction-log document.

**Architecture:** Columnar storage — each `Column` is a tagged `ColData` enum (`IntCol`/`FloatCol`/`StrCol`/`BoolCol`) over unboxed primitive `Vector`s, plus a parallel `Vector<Bool>` null mask. A `Table` is parallel named columns. `filter`, `order_by`, and `join` all reduce to "compute an index vector, then `take` (gather)". Aggregations are uniform capability records (`fn(Table, Vector<Int>) Cell`) so heterogeneous aggregations coexist in one `Vector`. Fluent API only (no SQL frontend).

**Tech Stack:** Twinkle (`.tw`), `@std.date` for benchmark timing, the copied `assert.tw` + `runner.tw` test harness (same pattern as `tools/leetcode/`). Run with `target/twk` (built via `make bundle-cli`).

**Design reference:** `docs/plans/dataframe-stress-test.md`.

---

## Deviation log (discovered during execution)

- **`Cell` → `Scalar` (Task 2):** `Cell` is a reserved builtin type name in Twinkle (the mutable reference cell). The scalar enum is therefore named **`Scalar`**, living in module **`cell`** (file `frame/cell.tw`). All later tasks use `use frame.cell.{Scalar}`, construct `Scalar.CInt(...)`, and call `cell.to_string(...)` / `cell.from_cells(...)`. Wherever the task text below says `Cell`/`Cell.CInt`, read `Scalar`/`Scalar.CInt`.

## Conventions for every task

- **Project root:** `examples/dataframe/` has its own `twinkle.toml`, so module paths resolve from there: `use frame.cell`, `use frame.column`, `use tests.cell_suite`, `use runner`, `use assert`.
- **Test entrypoint:** `examples/dataframe/main.tw` imports each suite module and calls `runner.run_all([...])`. Each task that adds a suite also wires it into `main.tw`.
- **Run all tests:** `target/twk run examples/dataframe/main.tw`
- **Run one suite (TDD inner loop):** `TWK_TEST_FILTER="<suite name>" target/twk run examples/dataframe/main.tw`
- **Red state:** Twinkle compiles the whole program, so a test referencing a not-yet-written function fails as a **compile error** ("unknown function/module"). That counts as the failing-test step. After implementing, the same run must pass.
- **Format after editing:** `target/twk fmt examples/dataframe/<file>.tw` (formatter is idempotent).
- **Anonymous `.{...}` literals** need a known expected type; otherwise use the named form (`Column.{...}`, `RowRef.{...}`, `Cell.CInt(n)`).
- **Two import lines** when a module both supplies constructors and a type you name: `use frame.row` (alias) + `use frame.row.{RowRef}` (type). Inherent-method dot sugar (`t.select(...)`) resolves via the receiver type regardless.
- **Commit** after each task with a short imperative subject. Do **not** add a `Co-Authored-By` trailer unless actually correct for the session. End-of-task commits only; never commit on `main` if the repo policy says branch first — assume work happens on a feature branch created at execution time.

---

## File structure (locked decomposition)

```
examples/dataframe/
  twinkle.toml            name = "dataframe"
  assert.tw               copied verbatim from boot/tests/assert.tw
  runner.tw               copied verbatim from boot/tests/runner.tw
  main.tw                 imports all suites + runner.run_all([...])
  frame/
    cell.tw     Cell enum (scalar at API edges); to_string; from_cells
    column.tw   ColData, Column, DType; constructors; len/dtype/is_null; as_* ; gather; compare_at; cell_at
    table.tw    Table; from_columns; col_index/column/ncols; select/drop/rename; head; take; display
    row.tw      RowRef view + typed accessors (int/float/str/bool/is_null)
    csv.tw      parse CSV string -> Table with per-column type inference
    query.tw    filter; with_column; Dir; order_by
    group.tw    Aggregation; count/sum/mean/min/max; GroupBy; group_by; agg
    join.tw     How enum; hash join (inner + left)
    gen.tw      seeded PRNG synthetic-data generator
  tests/
    cell_suite.tw  column_suite.tw  table_suite.tw  row_suite.tw
    csv_suite.tw   query_suite.tw   group_suite.tw  join_suite.tw
  bench/
    main.tw     generate N rows; time filter/order_by/group_by/join via date.now()
```

---

## Task 1: Project scaffolding

**Files:**
- Create: `examples/dataframe/twinkle.toml`
- Create: `examples/dataframe/assert.tw` (copy of `boot/tests/assert.tw`)
- Create: `examples/dataframe/runner.tw` (copy of `boot/tests/runner.tw`)
- Create: `examples/dataframe/main.tw`
- Create: `examples/dataframe/tests/cell_suite.tw`

- [ ] **Step 1: Create the project root and copy the harness**

```bash
mkdir -p examples/dataframe/frame examples/dataframe/tests examples/dataframe/bench
printf 'name = "dataframe"\n' > examples/dataframe/twinkle.toml
cp boot/tests/assert.tw examples/dataframe/assert.tw
cp boot/tests/runner.tw examples/dataframe/runner.tw
```

(If `boot/tests/assert.tw` / `runner.tw` differ from the `tools/leetcode/` copies, prefer the `boot/tests/` originals; they are the canonical harness.)

- [ ] **Step 2: Write a trivial smoke suite**

Create `examples/dataframe/tests/cell_suite.tw`:

```tw
use assert
use runner

pub fn suite() {
  runner.suite("cell")
    .test(
      "harness smoke test",
      fn() {
        assert.equal(1 + 1, 2)
      },
    )
}
```

- [ ] **Step 3: Wire the entrypoint**

Create `examples/dataframe/main.tw`:

```tw
use tests.cell_suite
use runner

runner.run_all([
  cell_suite.suite(),
])
```

- [ ] **Step 4: Run to verify the harness works**

Run: `target/twk run examples/dataframe/main.tw`
Expected: PASS — output ends with `Ran 1 tests: 1 passed`.

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe
git commit -m "dataframe: scaffold project root and test harness"
```

---

## Task 2: `Cell` scalar type

`Cell` is the boxed scalar used only at API edges (aggregation results, display, single-value access). It needs auto-derived structural `Eq` (for `assert.equal`) and a hand-written `to_string` (Stringify is not auto-derived for user types). `from_cells` packs a `Vector<Cell>` back into a typed `Column` (used by `group.agg`), inferring dtype from the first non-null cell.

**Files:**
- Create: `examples/dataframe/frame/cell.tw`
- Modify: `examples/dataframe/tests/cell_suite.tw`

- [ ] **Step 1: Write the failing tests**

Replace `examples/dataframe/tests/cell_suite.tw` with:

```tw
use assert
use runner
use frame.cell
use frame.cell.{Cell}

pub fn suite() {
  runner.suite("cell")
    .test(
      "to_string renders each variant",
      fn() {
        ok1 := try assert.equal(cell.to_string(Cell.CInt(42)), "42")
        ok2 := try assert.equal(cell.to_string(Cell.CFloat(1.5)), "1.5")
        ok3 := try assert.equal(cell.to_string(Cell.CStr("hi")), "hi")
        ok4 := try assert.equal(cell.to_string(Cell.CBool(true)), "true")
        assert.equal(cell.to_string(Cell.CNull), "null")
      },
    )
    .test(
      "cells are structurally comparable",
      fn() {
        assert.equal(Cell.CInt(7), Cell.CInt(7))
      },
    )
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run examples/dataframe/main.tw`
Expected: FAIL — compile error, unknown module `frame.cell`.

- [ ] **Step 3: Implement `cell.tw`**

Create `examples/dataframe/frame/cell.tw`:

```tw
/// Cell is the boxed scalar used at API edges: aggregation results, display,
/// and single-value access. Columnar storage uses unboxed Vectors instead.
pub type Cell = {
  CInt(Int),
  CFloat(Float),
  CStr(String),
  CBool(Bool),
  CNull,
}

/// Render a cell for display / Stringify. (Eq is auto-derived structurally.)
pub fn to_string(c: Cell) String {
  case c {
    .CInt(n) => n.to_string(),
    .CFloat(f) => f.to_string(),
    .CStr(s) => s,
    .CBool(b) => if b {
      "true"
    } else {
      "false"
    },
    .CNull => "null",
  }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `TWK_TEST_FILTER="cell" target/twk run examples/dataframe/main.tw`
Expected: PASS — `cell` suite green.

Then format: `target/twk fmt examples/dataframe/frame/cell.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/cell.tw examples/dataframe/tests/cell_suite.tw
git commit -m "dataframe: add Cell scalar type with to_string"
```

---

## Task 3: `Column` — columnar storage with null mask

**Files:**
- Create: `examples/dataframe/frame/column.tw`
- Create: `examples/dataframe/tests/column_suite.tw`
- Modify: `examples/dataframe/main.tw`

- [ ] **Step 1: Write the failing tests**

Create `examples/dataframe/tests/column_suite.tw`:

```tw
use assert
use runner
use frame.cell.{Cell}
use frame.column
use frame.column.{Column, ColData, DType}

pub fn suite() {
  runner.suite("column")
    .test(
      "int_col has length and no nulls",
      fn() {
        c := column.int_col([1, 2, 3])
        ok1 := try assert.equal(column.len(c), 3)
        assert.is_false(column.is_null(c, 1))
      },
    )
    .test(
      "dtype reflects the variant",
      fn() {
        c := column.float_col([1.0, 2.0])
        assert.equal(column.dtype(c), DType.DFloat)
      },
    )
    .test(
      "as_ints returns the backing vector",
      fn() {
        c := column.int_col([10, 20, 30])
        assert.equal(column.as_ints(c), [10, 20, 30])
      },
    )
    .test(
      "gather reorders data and nulls",
      fn() {
        c := column.with_nulls(ColData.IntCol([5, 6, 7]), [false, true, false])
        g := column.gather(c, [2, 1, 0])
        ok1 := try assert.equal(column.as_ints(g), [7, 6, 5])
        ok2 := try assert.is_true(column.is_null(g, 1))
        assert.is_false(column.is_null(g, 0))
      },
    )
    .test(
      "cell_at yields CNull for masked rows",
      fn() {
        c := column.with_nulls(ColData.StrCol(["a", "b"]), [false, true])
        ok1 := try assert.equal(column.cell_at(c, 0), Cell.CStr("a"))
        assert.equal(column.cell_at(c, 1), Cell.CNull)
      },
    )
    .test(
      "compare_at orders ints with nulls last",
      fn() {
        c := column.with_nulls(ColData.IntCol([3, 1, 0]), [false, false, true])
        ok1 := try assert.equal(column.compare_at(c, 1, 0), Order.Lt)
        assert.equal(column.compare_at(c, 0, 2), Order.Lt)
      },
    )
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run examples/dataframe/main.tw`
Expected: FAIL — compile error, unknown module `frame.column`.

- [ ] **Step 3: Implement `column.tw`**

Create `examples/dataframe/frame/column.tw`:

```tw
use frame.cell.{Cell}

/// The unboxed primitive backing store for a column.
pub type ColData = {
  IntCol(Vector<Int>),
  FloatCol(Vector<Float>),
  StrCol(Vector<String>),
  BoolCol(Vector<Bool>),
}

/// A column: dense data plus a parallel null mask (nulls[i] == true => missing).
pub type Column = .{ data: ColData, nulls: Vector<Bool> }

/// Logical element type of a column.
pub type DType = { DInt, DFloat, DStr, DBool }

fn no_nulls(n: Int) Vector<Bool> {
  Vector.make(n, false)
}

pub fn with_nulls(data: ColData, nulls: Vector<Bool>) Column {
  Column.{ data, nulls }
}

pub fn int_col(values: Vector<Int>) Column {
  Column.{ data: ColData.IntCol(values), nulls: no_nulls(values.len()) }
}

pub fn float_col(values: Vector<Float>) Column {
  Column.{ data: ColData.FloatCol(values), nulls: no_nulls(values.len()) }
}

pub fn str_col(values: Vector<String>) Column {
  Column.{ data: ColData.StrCol(values), nulls: no_nulls(values.len()) }
}

pub fn bool_col(values: Vector<Bool>) Column {
  Column.{ data: ColData.BoolCol(values), nulls: no_nulls(values.len()) }
}

pub fn len(c: Column) Int {
  case c.data {
    .IntCol(v) => v.len(),
    .FloatCol(v) => v.len(),
    .StrCol(v) => v.len(),
    .BoolCol(v) => v.len(),
  }
}

pub fn dtype(c: Column) DType {
  case c.data {
    .IntCol(_) => DType.DInt,
    .FloatCol(_) => DType.DFloat,
    .StrCol(_) => DType.DStr,
    .BoolCol(_) => DType.DBool,
  }
}

pub fn is_null(c: Column, i: Int) Bool {
  c.nulls[i]
}

pub fn as_ints(c: Column) Vector<Int> {
  case c.data {
    .IntCol(v) => v,
    _ => error("column is not Int"),
  }
}

pub fn as_floats(c: Column) Vector<Float> {
  case c.data {
    .FloatCol(v) => v,
    _ => error("column is not Float"),
  }
}

pub fn as_strs(c: Column) Vector<String> {
  case c.data {
    .StrCol(v) => v,
    _ => error("column is not Str"),
  }
}

pub fn as_bools(c: Column) Vector<Bool> {
  case c.data {
    .BoolCol(v) => v,
    _ => error("column is not Bool"),
  }
}

/// Gather rows by index, carrying the null mask along.
pub fn gather(c: Column, idx: Vector<Int>) Column {
  new_nulls: Vector<Bool> = []

  for i in idx {
    new_nulls = .append(c.nulls[i])
  }

  new_data := case c.data {
    .IntCol(v) => {
      out: Vector<Int> = []

      for i in idx {
        out = .append(v[i])
      }

      ColData.IntCol(out)
    },
    .FloatCol(v) => {
      out: Vector<Float> = []

      for i in idx {
        out = .append(v[i])
      }

      ColData.FloatCol(out)
    },
    .StrCol(v) => {
      out: Vector<String> = []

      for i in idx {
        out = .append(v[i])
      }

      ColData.StrCol(out)
    },
    .BoolCol(v) => {
      out: Vector<Bool> = []

      for i in idx {
        out = .append(v[i])
      }

      ColData.BoolCol(out)
    },
  }

  Column.{ data: new_data, nulls: new_nulls }
}

/// Read a single element as a boxed Cell (CNull when masked).
pub fn cell_at(c: Column, i: Int) Cell {
  if c.nulls[i] {
    return Cell.CNull
  }

  case c.data {
    .IntCol(v) => Cell.CInt(v[i]),
    .FloatCol(v) => Cell.CFloat(v[i]),
    .StrCol(v) => Cell.CStr(v[i]),
    .BoolCol(v) => Cell.CBool(v[i]),
  }
}

/// Compare two rows of this column for sorting. Nulls sort last (greater).
pub fn compare_at(c: Column, i: Int, j: Int) Order {
  ai := c.nulls[i]
  aj := c.nulls[j]

  if ai and aj {
    return Order.Eq
  }

  if ai {
    return Order.Gt
  }

  if aj {
    return Order.Lt
  }

  case c.data {
    .IntCol(v) => Int.compare(v[i], v[j]),
    .FloatCol(v) => Float.compare(v[i], v[j]),
    .StrCol(v) => String.compare(v[i], v[j]),
    .BoolCol(v) => Int.compare(bool_rank(v[i]), bool_rank(v[j])),
  }
}

fn bool_rank(b: Bool) Int {
  if b {
    1
  } else {
    0
  }
}
```

- [ ] **Step 4: Wire the suite and run**

Add to `examples/dataframe/main.tw` — import `use tests.column_suite` and add `column_suite.suite(),` to the `run_all` list. The file becomes:

```tw
use tests.cell_suite
use tests.column_suite
use runner

runner.run_all([
  cell_suite.suite(),
  column_suite.suite(),
])
```

Run: `TWK_TEST_FILTER="column" target/twk run examples/dataframe/main.tw`
Expected: PASS — `column` suite green.

Then format: `target/twk fmt examples/dataframe/frame/column.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/column.tw examples/dataframe/tests/column_suite.tw examples/dataframe/main.tw
git commit -m "dataframe: add columnar Column type with null mask, gather, compare_at"
```

---

## Task 4: `Table` — named columns, select/drop/rename, take, display

**Files:**
- Create: `examples/dataframe/frame/table.tw`
- Create: `examples/dataframe/tests/table_suite.tw`
- Modify: `examples/dataframe/main.tw`

- [ ] **Step 1: Write the failing tests**

Create `examples/dataframe/tests/table_suite.tw`:

```tw
use assert
use runner
use frame.column
use frame.table
use frame.table.{Table}

fn sample() Table {
  cols := [
    column.int_col([1, 2, 3]),
    column.str_col(["a", "b", "c"]),
  ]

  case table.from_columns(["id", "name"], cols) {
    .Ok(t) => t,
    .Err(e) => error(e),
  }
}

pub fn suite() {
  runner.suite("table")
    .test(
      "from_columns sets nrows and rejects ragged columns",
      fn() {
        t := sample()
        ok1 := try assert.equal(t.nrows, 3)
        bad := table.from_columns(["a", "b"], [column.int_col([1]), column.int_col([1, 2])])
        assert.is_err(bad)
      },
    )
    .test(
      "column looks up by name",
      fn() {
        t := sample()
        c := try t.column("id")
        assert.equal(column.as_ints(c), [1, 2, 3])
      },
    )
    .test(
      "select keeps and orders the requested columns",
      fn() {
        t := try sample().select(["name"])
        ok1 := try assert.equal(t.names, ["name"])
        assert.equal(column.as_strs(try t.column("name")), ["a", "b", "c"])
      },
    )
    .test(
      "drop removes a column",
      fn() {
        t := try sample().drop(["id"])
        assert.equal(t.names, ["name"])
      },
    )
    .test(
      "rename changes a column name",
      fn() {
        t := try sample().rename("id", "key")
        assert.equal(t.names, ["key", "name"])
      },
    )
    .test(
      "take gathers rows across all columns",
      fn() {
        t := sample().take([2, 0])
        ok1 := try assert.equal(column.as_ints(try t.column("id")), [3, 1])
        ok2 := try assert.equal(column.as_strs(try t.column("name")), ["c", "a"])
        assert.equal(t.nrows, 2)
      },
    )
    .test(
      "display renders a header and rows",
      fn() {
        s := sample().display()
        ok1 := try assert.str_contains(s, "id")
        assert.str_contains(s, "name")
      },
    )
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run examples/dataframe/main.tw`
Expected: FAIL — compile error, unknown module `frame.table`.

- [ ] **Step 3: Implement `table.tw`**

Create `examples/dataframe/frame/table.tw`:

```tw
use frame.cell
use frame.column
use frame.column.{Column}

/// A table is parallel named columns of equal length.
pub type Table = .{ names: Vector<String>, cols: Vector<Column>, nrows: Int }

pub fn from_columns(names: Vector<String>, cols: Vector<Column>) Result<Table, String> {
  if names.len() != cols.len() {
    return .Err("names/columns length mismatch: ${names.len()} vs ${cols.len()}")
  }

  if cols.len() == 0 {
    return .Ok(Table.{ names, cols, nrows: 0 })
  }

  nrows := column.len(cols[0])

  for c, i in cols {
    if column.len(c) != nrows {
      return .Err("column '${names[i]}' has length ${column.len(c)}, expected ${nrows}")
    }
  }

  .Ok(Table.{ names, cols, nrows })
}

pub fn ncols(t: Table) Int {
  t.cols.len()
}

pub fn col_index(t: Table, name: String) Int? {
  t.names.position(fn(n) { n == name })
}

pub fn column(t: Table, name: String) Result<Column, String> {
  case t.col_index(name) {
    .Some(i) => .Ok(t.cols[i]),
    .None => .Err("no such column '${name}'"),
  }
}

pub fn select(t: Table, names: Vector<String>) Result<Table, String> {
  out_cols: Vector<Column> = []

  for name in names {
    out_cols = .append(try t.column(name))
  }

  from_columns(names, out_cols)
}

pub fn drop(t: Table, names: Vector<String>) Result<Table, String> {
  keep: Vector<String> = []

  for n in t.names {
    if !names.contains(n) {
      keep = .append(n)
    }
  }

  t.select(keep)
}

pub fn rename(t: Table, from: String, to: String) Result<Table, String> {
  case t.col_index(from) {
    .None => .Err("no such column '${from}'"),
    .Some(i) => {
      new_names := case t.names.set(i, to) {
        .Some(ns) => ns,
        .None => { return .Err("rename index out of range") },
      }
      .Ok(Table.{ names: new_names, cols: t.cols, nrows: t.nrows })
    },
  }
}

pub fn head(t: Table, n: Int) Table {
  limit := if n < t.nrows {
    n
  } else {
    t.nrows
  }
  idx := collect i in range(limit) { i }
  t.take(idx)
}

/// Gather rows by index across every column (the core primitive).
pub fn take(t: Table, idx: Vector<Int>) Table {
  out_cols: Vector<Column> = []

  for c in t.cols {
    out_cols = .append(column.gather(c, idx))
  }

  Table.{ names: t.names, cols: out_cols, nrows: idx.len() }
}

/// Render a simple text table (header + every row). Intended for small results.
pub fn display(t: Table) String {
  lines: Vector<String> = [t.names.join(" | ")]

  for r in range(t.nrows) {
    fields: Vector<String> = []

    for c in t.cols {
      fields = .append(cell.to_string(column.cell_at(c, r)))
    }

    lines = .append(fields.join(" | "))
  }

  lines.join("\n")
}
```

- [ ] **Step 4: Wire the suite and run**

Add `use tests.table_suite` and `table_suite.suite(),` to `examples/dataframe/main.tw`.

Run: `TWK_TEST_FILTER="table" target/twk run examples/dataframe/main.tw`
Expected: PASS — `table` suite green.

Then format: `target/twk fmt examples/dataframe/frame/table.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/table.tw examples/dataframe/tests/table_suite.tw examples/dataframe/main.tw
git commit -m "dataframe: add Table with select/drop/rename/take/display"
```

---

## Task 5: `RowRef` — typed row view for predicates

**Files:**
- Create: `examples/dataframe/frame/row.tw`
- Create: `examples/dataframe/tests/row_suite.tw`
- Modify: `examples/dataframe/main.tw`

- [ ] **Step 1: Write the failing tests**

Create `examples/dataframe/tests/row_suite.tw`:

```tw
use assert
use runner
use frame.column
use frame.table
use frame.table.{Table}
use frame.row
use frame.row.{RowRef}

fn sample() Table {
  cols := [
    column.int_col([10, 20]),
    column.str_col(["x", "y"]),
  ]

  case table.from_columns(["age", "name"], cols) {
    .Ok(t) => t,
    .Err(e) => error(e),
  }
}

pub fn suite() {
  runner.suite("row")
    .test(
      "typed accessors read the right cell",
      fn() {
        t := sample()
        r := RowRef.{ table: t, idx: 1 }
        ok1 := try assert.equal(r.int("age"), 20)
        assert.equal(r.str("name"), "y")
      },
    )
    .test(
      "is_null reports the mask",
      fn() {
        c := column.with_nulls(column.ColData.IntCol([1, 2]), [true, false])
        t := case table.from_columns(["v"], [c]) {
          .Ok(tt) => tt,
          .Err(e) => error(e),
        }
        r := RowRef.{ table: t, idx: 0 }
        assert.is_true(r.is_null("v"))
      },
    )
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run examples/dataframe/main.tw`
Expected: FAIL — compile error, unknown module `frame.row`.

- [ ] **Step 3: Implement `row.tw`**

Create `examples/dataframe/frame/row.tw`:

```tw
use frame.column
use frame.column.{Column}
use frame.table
use frame.table.{Table}

/// A lightweight view of one row, used by predicates: r.int("age") > 30.
/// Typed accessors trap on type mismatch or null — null-tolerant access
/// goes through is_null first. (This sharp edge is intentional friction-log material.)
pub type RowRef = .{ table: Table, idx: Int }

fn lookup(r: RowRef, name: String) Column {
  case r.table.column(name) {
    .Ok(c) => c,
    .Err(e) => error(e),
  }
}

pub fn is_null(r: RowRef, name: String) Bool {
  column.is_null(lookup(r, name), r.idx)
}

pub fn int(r: RowRef, name: String) Int {
  c := lookup(r, name)

  if column.is_null(c, r.idx) {
    error("null at ${name}[${r.idx}]")
  }

  column.as_ints(c)[r.idx]
}

pub fn float(r: RowRef, name: String) Float {
  c := lookup(r, name)

  if column.is_null(c, r.idx) {
    error("null at ${name}[${r.idx}]")
  }

  column.as_floats(c)[r.idx]
}

pub fn str(r: RowRef, name: String) String {
  c := lookup(r, name)

  if column.is_null(c, r.idx) {
    error("null at ${name}[${r.idx}]")
  }

  column.as_strs(c)[r.idx]
}

pub fn bool(r: RowRef, name: String) Bool {
  c := lookup(r, name)

  if column.is_null(c, r.idx) {
    error("null at ${name}[${r.idx}]")
  }

  column.as_bools(c)[r.idx]
}
```

- [ ] **Step 4: Wire the suite and run**

Add `use tests.row_suite` and `row_suite.suite(),` to `examples/dataframe/main.tw`.

Run: `TWK_TEST_FILTER="row" target/twk run examples/dataframe/main.tw`
Expected: PASS — `row` suite green.

Then format: `target/twk fmt examples/dataframe/frame/row.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/row.tw examples/dataframe/tests/row_suite.tw examples/dataframe/main.tw
git commit -m "dataframe: add RowRef typed row view for predicates"
```

---

## Task 6: `filter` and `with_column`

**Files:**
- Create: `examples/dataframe/frame/query.tw`
- Create: `examples/dataframe/tests/query_suite.tw`
- Modify: `examples/dataframe/main.tw`

- [ ] **Step 1: Write the failing tests**

Create `examples/dataframe/tests/query_suite.tw`:

```tw
use assert
use runner
use frame.column
use frame.table
use frame.table.{Table}
use frame.query

fn people() Table {
  cols := [
    column.int_col([25, 35, 45]),
    column.str_col(["ann", "bob", "cid"]),
  ]

  case table.from_columns(["age", "name"], cols) {
    .Ok(t) => t,
    .Err(e) => error(e),
  }
}

pub fn suite() {
  runner.suite("query")
    .test(
      "filter keeps matching rows",
      fn() {
        t := people().filter(fn(r) { r.int("age") > 30 })
        ok1 := try assert.equal(t.nrows, 2)
        assert.equal(column.as_strs(try t.column("name")), ["bob", "cid"])
      },
    )
    .test(
      "with_column appends a derived column",
      fn() {
        base := people()
        doubled := collect a in column.as_ints(try base.column("age")) { a * 2 }
        t := try base.with_column("age2", column.int_col(doubled))
        ok1 := try assert.equal(t.names, ["age", "name", "age2"])
        assert.equal(column.as_ints(try t.column("age2")), [50, 70, 90])
      },
    )
    .test(
      "with_column rejects a length mismatch",
      fn() {
        bad := people().with_column("x", column.int_col([1]))
        assert.is_err(bad)
      },
    )
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run examples/dataframe/main.tw`
Expected: FAIL — compile error, unknown module `frame.query`.

- [ ] **Step 3: Implement `filter` and `with_column` in `query.tw`**

Create `examples/dataframe/frame/query.tw`:

```tw
use frame.column
use frame.column.{Column}
use frame.table
use frame.table.{Table}
use frame.row.{RowRef}

/// Keep rows for which the predicate returns true.
pub fn filter(t: Table, pred: fn(RowRef) Bool) Table {
  idx: Vector<Int> = []

  for i in range(t.nrows) {
    if pred(RowRef.{ table: t, idx: i }) {
      idx = .append(i)
    }
  }

  t.take(idx)
}

/// Add or replace a column. The new column must match the table's row count.
pub fn with_column(t: Table, name: String, c: Column) Result<Table, String> {
  if t.nrows != 0 and column.len(c) != t.nrows {
    return .Err("column '${name}' has length ${column.len(c)}, expected ${t.nrows}")
  }

  case t.col_index(name) {
    .Some(i) => {
      new_cols := case t.cols.set(i, c) {
        .Some(cs) => cs,
        .None => { return .Err("with_column index out of range") },
      }
      .Ok(Table.{ names: t.names, cols: new_cols, nrows: t.nrows })
    },
    .None => from_columns_append(t, name, c),
  }
}

fn from_columns_append(t: Table, name: String, c: Column) Result<Table, String> {
  nrows := if t.ncols() == 0 {
    column.len(c)
  } else {
    t.nrows
  }
  .Ok(Table.{ names: t.names.append(name), cols: t.cols.append(c), nrows })
}
```

- [ ] **Step 4: Wire the suite and run**

Add `use tests.query_suite` and `query_suite.suite(),` to `examples/dataframe/main.tw`.

Run: `TWK_TEST_FILTER="query" target/twk run examples/dataframe/main.tw`
Expected: PASS — `query` suite green.

Then format: `target/twk fmt examples/dataframe/frame/query.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/query.tw examples/dataframe/tests/query_suite.tw examples/dataframe/main.tw
git commit -m "dataframe: add filter and with_column"
```

---

## Task 7: `order_by`

**Files:**
- Modify: `examples/dataframe/frame/query.tw`
- Modify: `examples/dataframe/tests/query_suite.tw`

- [ ] **Step 1: Add the failing tests**

Append these two `.test(...)` calls to the `query` suite chain in `examples/dataframe/tests/query_suite.tw` (insert before the closing of the chain). Also add `use frame.query.{Dir}` to the imports at the top of the file.

```tw
    .test(
      "order_by ascending sorts by the keyed column",
      fn() {
        t := people().take([2, 0, 1])
        sorted := try t.order_by("age", Dir.Asc)
        assert.equal(column.as_ints(try sorted.column("age")), [25, 35, 45])
      },
    )
    .test(
      "order_by descending reverses the order",
      fn() {
        sorted := try people().order_by("age", Dir.Desc)
        assert.equal(column.as_ints(try sorted.column("age")), [45, 35, 25])
      },
    )
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run examples/dataframe/main.tw`
Expected: FAIL — compile error, unknown type `Dir` / unknown function `order_by`.

- [ ] **Step 3: Implement `order_by` in `query.tw`**

Append to `examples/dataframe/frame/query.tw`:

```tw
pub type Dir = { Asc, Desc }

fn flip(o: Order) Order {
  case o {
    .Lt => Order.Gt,
    .Gt => Order.Lt,
    .Eq => Order.Eq,
  }
}

/// Sort rows by a single column. Sorts an index vector, then gathers.
pub fn order_by(t: Table, name: String, dir: Dir) Result<Table, String> {
  col := try t.column(name)
  idx := collect i in range(t.nrows) { i }

  sorted := idx.sort_by(fn(a, b) {
    base := column.compare_at(col, a, b)

    case dir {
      .Asc => base,
      .Desc => flip(base),
    }
  })

  .Ok(t.take(sorted))
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `TWK_TEST_FILTER="query" target/twk run examples/dataframe/main.tw`
Expected: PASS — `query` suite green (now including order_by tests).

Then format: `target/twk fmt examples/dataframe/frame/query.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/query.tw examples/dataframe/tests/query_suite.tw
git commit -m "dataframe: add order_by (index-sort then gather)"
```

---

## Task 8: CSV loader with type inference

`csv.tw` parses a simple CSV string (comma-separated, newline-delimited, no quoted-field/escape handling in MVP — that is a deliberate scope cut and friction-log note) into a `Table`. Each column's dtype is inferred by scanning its cells: all-empty-or-int → `IntCol`; else all-empty-or-number → `FloatCol`; `"true"`/`"false"` (case-sensitive) → `BoolCol`; otherwise `StrCol`. Empty cells become nulls.

**Files:**
- Create: `examples/dataframe/frame/csv.tw`
- Create: `examples/dataframe/tests/csv_suite.tw`
- Modify: `examples/dataframe/main.tw`

- [ ] **Step 1: Write the failing tests**

Create `examples/dataframe/tests/csv_suite.tw`:

```tw
use assert
use runner
use frame.column
use frame.column.{DType}
use frame.csv

pub fn suite() {
  runner.suite("csv")
    .test(
      "infers int and string columns",
      fn() {
        t := try csv.parse("id,name\n1,ann\n2,bob\n")
        ok1 := try assert.equal(t.nrows, 2)
        ok2 := try assert.equal(column.dtype(try t.column("id")), DType.DInt)
        ok3 := try assert.equal(column.dtype(try t.column("name")), DType.DStr)
        assert.equal(column.as_ints(try t.column("id")), [1, 2])
      },
    )
    .test(
      "infers float when a column mixes int and decimal",
      fn() {
        t := try csv.parse("x\n1\n2.5\n")
        ok1 := try assert.equal(column.dtype(try t.column("x")), DType.DFloat)
        assert.equal(column.as_floats(try t.column("x")), [1.0, 2.5])
      },
    )
    .test(
      "empty cells become nulls",
      fn() {
        t := try csv.parse("x\n1\n\n3\n")
        ok1 := try assert.is_true(column.is_null(try t.column("x"), 1))
        assert.is_false(column.is_null(try t.column("x"), 0))
      },
    )
    .test(
      "ragged rows are an error",
      fn() {
        assert.is_err(csv.parse("a,b\n1\n"))
      },
    )
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run examples/dataframe/main.tw`
Expected: FAIL — compile error, unknown module `frame.csv`.

- [ ] **Step 3: Implement `csv.tw`**

Create `examples/dataframe/frame/csv.tw`:

```tw
use frame.column
use frame.column.{Column, ColData}
use frame.table
use frame.table.{Table}

/// Parse a simple CSV (comma fields, newline rows, no quoting) into a Table,
/// inferring each column's dtype. Empty cells become nulls.
pub fn parse(input: String) Result<Table, String> {
  rows := split_rows(input)

  if rows.len() == 0 {
    return .Err("empty CSV input")
  }

  header := rows[0].split(",")
  ncols := header.len()

  // Collect raw string cells per column.
  raw: Vector<Vector<String>> = []

  for _ in range(ncols) {
    raw = .append([])
  }

  body := rows.drop_first()

  for line, r in body {
    fields := line.split(",")

    if fields.len() != ncols {
      return .Err("row ${r + 1} has ${fields.len()} fields, expected ${ncols}")
    }

    for f, c in fields {
      raw[c] = .append(f)
    }
  }

  cols: Vector<Column> = []

  for c in range(ncols) {
    cols = .append(infer_column(raw[c]))
  }

  table.from_columns(header, cols)
}

fn split_rows(input: String) Vector<String> {
  out: Vector<String> = []

  for line in input.split("\n") {
    if line != "" {
      out = .append(line)
    }
  }

  out
}

fn is_int(s: String) Bool {
  case Int.from_string(s) {
    .Some(_) => true,
    .None => false,
  }
}

fn is_float(s: String) Bool {
  case Float.from_string(s) {
    .Some(_) => true,
    .None => false,
  }
}

fn is_bool(s: String) Bool {
  s == "true" or s == "false"
}

fn infer_column(cells: Vector<String>) Column {
  nulls: Vector<Bool> = []

  for s in cells {
    nulls = .append(s == "")
  }

  all_int := true
  all_float := true
  all_bool := true
  any_value := false

  for s in cells {
    if s != "" {
      any_value = true

      if !is_int(s) {
        all_int = false
      }

      if !is_float(s) {
        all_float = false
      }

      if !is_bool(s) {
        all_bool = false
      }
    }
  }

  if any_value and all_int {
    data: Vector<Int> = []

    for s in cells {
      data = .append(parse_int_or(s, 0))
    }

    Column.{ data: ColData.IntCol(data), nulls }
  } else if any_value and all_float {
    data: Vector<Float> = []

    for s in cells {
      data = .append(parse_float_or(s, 0.0))
    }

    Column.{ data: ColData.FloatCol(data), nulls }
  } else if any_value and all_bool {
    data: Vector<Bool> = []

    for s in cells {
      data = .append(s == "true")
    }

    Column.{ data: ColData.BoolCol(data), nulls }
  } else {
    Column.{ data: ColData.StrCol(cells), nulls }
  }
}

fn parse_int_or(s: String, fallback: Int) Int {
  case Int.from_string(s) {
    .Some(n) => n,
    .None => fallback,
  }
}

fn parse_float_or(s: String, fallback: Float) Float {
  case Float.from_string(s) {
    .Some(f) => f,
    .None => fallback,
  }
}
```

- [ ] **Step 4: Wire the suite and run**

Add `use tests.csv_suite` and `csv_suite.suite(),` to `examples/dataframe/main.tw`.

Run: `TWK_TEST_FILTER="csv" target/twk run examples/dataframe/main.tw`
Expected: PASS — `csv` suite green.

Then format: `target/twk fmt examples/dataframe/frame/csv.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/csv.tw examples/dataframe/tests/csv_suite.tw examples/dataframe/main.tw
git commit -m "dataframe: add CSV loader with per-column type inference"
```

---

## Task 9: `from_cells` — pack Cells back into a typed Column

`group.agg` produces one `Cell` per group per aggregation; those must become a typed `Column`. `from_cells` infers dtype from the first non-null cell and packs (CNull → masked, with a type-appropriate placeholder value).

**Files:**
- Modify: `examples/dataframe/frame/column.tw`
- Modify: `examples/dataframe/tests/column_suite.tw`

- [ ] **Step 1: Add the failing tests**

Add `use frame.cell.{Cell}` is already imported in `column_suite.tw`. Append to the `column` suite chain:

```tw
    .test(
      "from_cells infers Int and packs nulls",
      fn() {
        c := try column.from_cells([Cell.CInt(1), Cell.CNull, Cell.CInt(3)])
        ok1 := try assert.equal(column.dtype(c), DType.DInt)
        ok2 := try assert.is_true(column.is_null(c, 1))
        assert.equal(column.as_ints(c), [1, 0, 3])
      },
    )
    .test(
      "from_cells infers Float",
      fn() {
        c := try column.from_cells([Cell.CFloat(1.5), Cell.CFloat(2.5)])
        assert.equal(column.dtype(c), DType.DFloat)
      },
    )
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run examples/dataframe/main.tw`
Expected: FAIL — compile error, unknown function `from_cells`.

- [ ] **Step 3: Implement `from_cells` in `column.tw`**

Append to `examples/dataframe/frame/column.tw`:

```tw
/// Pack a vector of Cells into a typed Column, inferring dtype from the first
/// non-null cell. CNull cells become masked with a type-appropriate placeholder.
/// An all-null input defaults to a FloatCol (matches mean-of-empty results).
pub fn from_cells(cells: Vector<Cell>) Result<Column, String> {
  nulls: Vector<Bool> = []

  for c in cells {
    nulls = .append(is_null_cell(c))
  }

  case first_non_null(cells) {
    .None => .Ok(Column.{ data: ColData.FloatCol(Vector.make(cells.len(), 0.0)), nulls }),
    .Some(.CInt(_)) => pack_ints(cells, nulls),
    .Some(.CFloat(_)) => pack_floats(cells, nulls),
    .Some(.CStr(_)) => pack_strs(cells, nulls),
    .Some(.CBool(_)) => pack_bools(cells, nulls),
    .Some(.CNull) => .Err("unreachable: first_non_null returned CNull"),
  }
}

fn is_null_cell(c: Cell) Bool {
  case c {
    .CNull => true,
    _ => false,
  }
}

fn first_non_null(cells: Vector<Cell>) Cell? {
  for c in cells {
    case c {
      .CNull => {},
      _ => { return .Some(c) },
    }
  }

  .None
}

fn pack_ints(cells: Vector<Cell>, nulls: Vector<Bool>) Result<Column, String> {
  data: Vector<Int> = []

  for c in cells {
    case c {
      .CInt(n) => data = .append(n),
      .CNull => data = .append(0),
      _ => { return .Err("mixed cell types in from_cells (expected Int)") },
    }
  }

  .Ok(Column.{ data: ColData.IntCol(data), nulls })
}

fn pack_floats(cells: Vector<Cell>, nulls: Vector<Bool>) Result<Column, String> {
  data: Vector<Float> = []

  for c in cells {
    case c {
      .CFloat(f) => data = .append(f),
      .CInt(n) => data = .append(n.to_float()),
      .CNull => data = .append(0.0),
      _ => { return .Err("mixed cell types in from_cells (expected Float)") },
    }
  }

  .Ok(Column.{ data: ColData.FloatCol(data), nulls })
}

fn pack_strs(cells: Vector<Cell>, nulls: Vector<Bool>) Result<Column, String> {
  data: Vector<String> = []

  for c in cells {
    case c {
      .CStr(s) => data = .append(s),
      .CNull => data = .append(""),
      _ => { return .Err("mixed cell types in from_cells (expected Str)") },
    }
  }

  .Ok(Column.{ data: ColData.StrCol(data), nulls })
}

fn pack_bools(cells: Vector<Cell>, nulls: Vector<Bool>) Result<Column, String> {
  data: Vector<Bool> = []

  for c in cells {
    case c {
      .CBool(b) => data = .append(b),
      .CNull => data = .append(false),
      _ => { return .Err("mixed cell types in from_cells (expected Bool)") },
    }
  }

  .Ok(Column.{ data: ColData.BoolCol(data), nulls })
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `TWK_TEST_FILTER="column" target/twk run examples/dataframe/main.tw`
Expected: PASS — `column` suite green.

Then format: `target/twk fmt examples/dataframe/frame/column.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/column.tw examples/dataframe/tests/column_suite.tw
git commit -m "dataframe: add from_cells to pack Cells into a typed Column"
```

---

## Task 10: Aggregations (`count`/`sum`/`mean`/`min`/`max`)

Aggregations are uniform capability records: `Aggregation = .{ name: String, apply: fn(Table, Vector<Int>) Cell }`. `apply` receives the source table and the group's row indices, so heterogeneous aggregations live in one `Vector` despite the no-trait system. All aggregations skip nulls.

**Files:**
- Create: `examples/dataframe/frame/group.tw`
- Create: `examples/dataframe/tests/group_suite.tw`
- Modify: `examples/dataframe/main.tw`

- [ ] **Step 1: Write the failing tests (aggregations over an explicit index set)**

Create `examples/dataframe/tests/group_suite.tw`:

```tw
use assert
use runner
use frame.cell.{Cell}
use frame.column
use frame.table
use frame.table.{Table}
use frame.group

fn numbers() Table {
  cols := [column.int_col([1, 2, 3, 4])]

  case table.from_columns(["x"], cols) {
    .Ok(t) => t,
    .Err(e) => error(e),
  }
}

pub fn suite() {
  runner.suite("group")
    .test(
      "count counts the index set",
      fn() {
        a := group.count()
        assert.equal((a.apply)(numbers(), [0, 1, 2]), Cell.CInt(3))
      },
    )
    .test(
      "sum adds the selected int rows",
      fn() {
        a := group.sum("x")
        assert.equal((a.apply)(numbers(), [0, 1, 3]), Cell.CInt(7))
      },
    )
    .test(
      "mean returns a float average",
      fn() {
        a := group.mean("x")
        assert.equal((a.apply)(numbers(), [0, 1, 2, 3]), Cell.CFloat(2.5))
      },
    )
    .test(
      "min and max pick extremes",
      fn() {
        amin := group.min("x")
        amax := group.max("x")
        ok1 := try assert.equal((amin.apply)(numbers(), [1, 3]), Cell.CInt(2))
        assert.equal((amax.apply)(numbers(), [1, 3]), Cell.CInt(4))
      },
    )
}
```

> Note: `(a.apply)(...)` parenthesizes the field access so it is called as a function, not parsed as an inherent method on `a`.

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run examples/dataframe/main.tw`
Expected: FAIL — compile error, unknown module `frame.group`.

- [ ] **Step 3: Implement the aggregations in `group.tw`**

Create `examples/dataframe/frame/group.tw`:

```tw
use frame.cell.{Cell}
use frame.column
use frame.column.{Column}
use frame.table
use frame.table.{Table}

/// A uniform aggregation: given the source table and a group's row indices,
/// produce one scalar Cell. Uniformity lets heterogeneous aggregations share
/// one Vector despite the no-trait type system.
pub type Aggregation = .{ name: String, apply: fn(Table, Vector<Int>) Cell }

fn source_column(t: Table, name: String) Column {
  case t.column(name) {
    .Ok(c) => c,
    .Err(e) => error(e),
  }
}

pub fn count() Aggregation {
  Aggregation.{
    name: "count",
    apply: fn(t: Table, idx: Vector<Int>) {
      Cell.CInt(idx.len())
    },
  }
}

pub fn sum(col: String) Aggregation {
  Aggregation.{
    name: "sum_${col}",
    apply: fn(t: Table, idx: Vector<Int>) {
      c := source_column(t, col)

      case c.data {
        .IntCol(v) => {
          acc := 0

          for i in idx {
            if !column.is_null(c, i) {
              acc = acc + v[i]
            }
          }

          Cell.CInt(acc)
        },
        .FloatCol(v) => {
          acc := 0.0

          for i in idx {
            if !column.is_null(c, i) {
              acc = acc + v[i]
            }
          }

          Cell.CFloat(acc)
        },
        _ => error("sum requires a numeric column: ${col}"),
      }
    },
  }
}

pub fn mean(col: String) Aggregation {
  Aggregation.{
    name: "mean_${col}",
    apply: fn(t: Table, idx: Vector<Int>) {
      c := source_column(t, col)
      total := 0.0
      n := 0

      for i in idx {
        if !column.is_null(c, i) {
          n = n + 1
          total = total + numeric_at(c, col, i)
        }
      }

      if n == 0 {
        Cell.CNull
      } else {
        Cell.CFloat(total / n.to_float())
      }
    },
  }
}

pub fn min(col: String) Aggregation {
  extreme(col, "min", Order.Lt)
}

pub fn max(col: String) Aggregation {
  extreme(col, "max", Order.Gt)
}

fn extreme(col: String, prefix: String, keep: Order) Aggregation {
  Aggregation.{
    name: "${prefix}_${col}",
    apply: fn(t: Table, idx: Vector<Int>) {
      c := source_column(t, col)
      best := -1

      for i in idx {
        if !column.is_null(c, i) {
          if best < 0 or column.compare_at(c, i, best) == keep {
            best = i
          }
        }
      }

      if best < 0 {
        Cell.CNull
      } else {
        column.cell_at(c, best)
      }
    },
  }
}

fn numeric_at(c: Column, col: String, i: Int) Float {
  case c.data {
    .IntCol(v) => v[i].to_float(),
    .FloatCol(v) => v[i],
    _ => error("expected a numeric column: ${col}"),
  }
}
```

- [ ] **Step 4: Wire the suite and run**

Add `use tests.group_suite` and `group_suite.suite(),` to `examples/dataframe/main.tw`.

Run: `TWK_TEST_FILTER="group" target/twk run examples/dataframe/main.tw`
Expected: PASS — `group` suite green.

Then format: `target/twk fmt examples/dataframe/frame/group.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/group.tw examples/dataframe/tests/group_suite.tw examples/dataframe/main.tw
git commit -m "dataframe: add aggregations (count/sum/mean/min/max)"
```

---

## Task 11: `group_by(...).agg(...)`

Groups rows by a composite key string built from the key columns (using `Cell.to_string` joined with a separator), tracked in a `Dict<String, Vector<Int>>` — insertion order gives deterministic group order. The output has the key columns (one row per group, taken from each group's first row) plus one column per aggregation (via `from_cells`).

**Files:**
- Modify: `examples/dataframe/frame/group.tw`
- Modify: `examples/dataframe/tests/group_suite.tw`

- [ ] **Step 1: Add the failing tests**

Append to the `group` suite chain in `examples/dataframe/tests/group_suite.tw` (and add `use frame.column.{DType}` to imports if a dtype assertion is wanted; not required below):

```tw
    .test(
      "group_by then agg sums per group",
      fn() {
        cols := [
          column.str_col(["a", "b", "a", "b"]),
          column.int_col([10, 1, 20, 2]),
        ]
        t := case table.from_columns(["k", "v"], cols) {
          .Ok(tt) => tt,
          .Err(e) => error(e),
        }
        g := try t.group_by(["k"]).agg([group.sum("v"), group.count()])
        ok1 := try assert.equal(g.nrows, 2)
        ok2 := try assert.equal(column.as_strs(try g.column("k")), ["a", "b"])
        ok3 := try assert.equal(column.as_ints(try g.column("sum_v"), ), [30, 3])
        assert.equal(column.as_ints(try g.column("count")), [2, 2])
      },
    )
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run examples/dataframe/main.tw`
Expected: FAIL — compile error, unknown function `group_by` / `agg`.

- [ ] **Step 3: Implement `group_by` and `agg` in `group.tw`**

Append to `examples/dataframe/frame/group.tw`:

```tw
pub type GroupBy = .{ table: Table, keys: Vector<String> }

pub fn group_by(t: Table, keys: Vector<String>) GroupBy {
  GroupBy.{ table: t, keys }
}

fn group_key(t: Table, keys: Vector<String>, row: Int) String {
  parts: Vector<String> = []

  for k in keys {
    c := source_column(t, k)
    parts = .append(cell.to_string(column.cell_at(c, row)))
  }

  parts.join("\u{1f}")
}

pub fn agg(g: GroupBy, aggs: Vector<Aggregation>) Result<Table, String> {
  t := g.table

  // Build group buckets, preserving first-seen order.
  buckets: Dict<String, Vector<Int>> = Dict.new()
  order: Vector<String> = []

  for row in range(t.nrows) {
    key := group_key(t, g.keys, row)

    case buckets[key] {
      .Some(idxs) => buckets[key] = idxs.append(row),
      .None => {
        buckets[key] = [row]
        order = .append(key)
      },
    }
  }

  // First row index of each group, in first-seen order.
  firsts: Vector<Int> = []

  for key in order {
    case buckets[key] {
      .Some(idxs) => firsts = .append(idxs[0]),
      .None => { return .Err("internal: missing bucket ${key}") },
    }
  }

  // Output columns: key columns gathered by `firsts`, then one per aggregation.
  out_names: Vector<String> = []
  out_cols: Vector<Column> = []

  for k in g.keys {
    out_names = .append(k)
    out_cols = .append(column.gather(source_column(t, k), firsts))
  }

  for a in aggs {
    cells: Vector<Cell> = []

    for key in order {
      idxs := case buckets[key] {
        .Some(v) => v,
        .None => { return .Err("internal: missing bucket ${key}") },
      }
      cells = .append((a.apply)(t, idxs))
    }

    out_names = .append(a.name)
    out_cols = .append(try column.from_cells(cells))
  }

  table.from_columns(out_names, out_cols)
}
```

> The `\u{1f}` unit-separator in `group_key` avoids collisions between concatenated key fields. If string-escape syntax differs, use a sentinel unlikely to appear in data (e.g. `"\u{1f}"` → fall back to `"||"`), and note the limitation in the friction log.

- [ ] **Step 4: Run to verify it passes**

Run: `TWK_TEST_FILTER="group" target/twk run examples/dataframe/main.tw`
Expected: PASS — `group` suite green.

Then format: `target/twk fmt examples/dataframe/frame/group.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/group.tw examples/dataframe/tests/group_suite.tw
git commit -m "dataframe: add group_by().agg() with HAMT-keyed grouping"
```

---

## Task 12: Hash join (inner + left)

`join.tw` builds a `Dict<String, Vector<Int>>` from the right table's key column (string-keyed for MVP — keys are stringified via `Cell.to_string`), then probes with the left rows. Produces left+right index vectors, gathers both sides, and concatenates columns (right key column dropped to avoid duplication; right non-key columns prefixed on name collision).

**Files:**
- Create: `examples/dataframe/frame/join.tw`
- Create: `examples/dataframe/tests/join_suite.tw`
- Modify: `examples/dataframe/main.tw`

- [ ] **Step 1: Write the failing tests**

Create `examples/dataframe/tests/join_suite.tw`:

```tw
use assert
use runner
use frame.column
use frame.table
use frame.table.{Table}
use frame.join
use frame.join.{How}

fn left() Table {
  cols := [
    column.int_col([1, 2, 3]),
    column.str_col(["ann", "bob", "cid"]),
  ]

  case table.from_columns(["id", "name"], cols) {
    .Ok(t) => t,
    .Err(e) => error(e),
  }
}

fn right() Table {
  cols := [
    column.int_col([2, 3]),
    column.str_col(["x", "y"]),
  ]

  case table.from_columns(["id", "tag"], cols) {
    .Ok(t) => t,
    .Err(e) => error(e),
  }
}

pub fn suite() {
  runner.suite("join")
    .test(
      "inner join keeps only matching keys",
      fn() {
        j := try join.join(left(), right(), "id", How.Inner)
        ok1 := try assert.equal(j.nrows, 2)
        ok2 := try assert.equal(column.as_strs(try j.column("name")), ["bob", "cid"])
        assert.equal(column.as_strs(try j.column("tag")), ["x", "y"])
      },
    )
    .test(
      "left join keeps unmatched left rows with null fill",
      fn() {
        j := try join.join(left(), right(), "id", How.Left)
        ok1 := try assert.equal(j.nrows, 3)
        ok2 := try assert.equal(column.as_strs(try j.column("name")), ["ann", "bob", "cid"])
        assert.is_true(column.is_null(try j.column("tag"), 0))
      },
    )
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run examples/dataframe/main.tw`
Expected: FAIL — compile error, unknown module `frame.join`.

- [ ] **Step 3: Implement `join.tw`**

Create `examples/dataframe/frame/join.tw`:

```tw
use frame.cell
use frame.column
use frame.column.{Column}
use frame.table
use frame.table.{Table}

pub type How = { Inner, Left }

fn key_string(c: Column, i: Int) String {
  cell.to_string(column.cell_at(c, i))
}

/// Hash join `l` and `r` on a shared key column name. Right key column is
/// dropped from the output; right non-key columns whose names collide with
/// left columns are prefixed with "r_". Left join fills unmatched right rows
/// with nulls (a sentinel index of -1, materialized as a null row).
pub fn join(l: Table, r: Table, on: String, how: How) Result<Table, String> {
  lkey := try l.column(on)
  rkey := try r.column(on)

  // Build the right-side index map: key string -> right row indices.
  rmap: Dict<String, Vector<Int>> = Dict.new()

  for ri in range(r.nrows) {
    if !column.is_null(rkey, ri) {
      k := key_string(rkey, ri)

      case rmap[k] {
        .Some(idxs) => rmap[k] = idxs.append(ri),
        .None => rmap[k] = [ri],
      }
    }
  }

  // Produce paired index vectors. -1 on the right means "no match".
  lidx: Vector<Int> = []
  ridx: Vector<Int> = []

  for li in range(l.nrows) {
    matches := if column.is_null(lkey, li) {
      empty_indices()
    } else {
      case rmap[key_string(lkey, li)] {
        .Some(idxs) => idxs,
        .None => empty_indices(),
      }
    }

    if matches.len() == 0 {
      case how {
        .Left => {
          lidx = .append(li)
          ridx = .append(-1)
        },
        .Inner => {},
      }
    } else {
      for ri in matches {
        lidx = .append(li)
        ridx = .append(ri)
      }
    }
  }

  build_output(l, r, on, lidx, ridx)
}

fn empty_indices() Vector<Int> {
  []
}

fn build_output(
  l: Table,
  r: Table,
  on: String,
  lidx: Vector<Int>,
  ridx: Vector<Int>,
) Result<Table, String> {
  out_names: Vector<String> = []
  out_cols: Vector<Column> = []

  // All left columns, gathered by lidx.
  for c, i in l.cols {
    out_names = .append(l.names[i])
    out_cols = .append(column.gather(c, lidx))
  }

  // Right columns except the join key; gathered by ridx, null-filling -1.
  for c, i in r.cols {
    name := r.names[i]

    if name != on {
      out_name := if l.names.contains(name) {
        "r_${name}"
      } else {
        name
      }
      out_names = .append(out_name)
      out_cols = .append(gather_nullable(c, ridx))
    }
  }

  table.from_columns(out_names, out_cols)
}

/// Like column.gather, but a -1 index produces a null cell.
fn gather_nullable(c: Column, idx: Vector<Int>) Column {
  cells: Vector<cell.Cell> = []

  for i in idx {
    if i < 0 {
      cells = .append(cell.Cell.CNull)
    } else {
      cells = .append(column.cell_at(c, i))
    }
  }

  case column.from_cells(cells) {
    .Ok(col) => col,
    .Err(e) => error(e),
  }
}
```

> `gather_nullable` round-trips through `Cell`/`from_cells` so a `-1` (no-match) row becomes a null. If a column's matched rows are all null (so `from_cells` can't infer a dtype), it defaults to `FloatCol`; note this as a known MVP limitation in the friction log.

- [ ] **Step 4: Wire the suite and run**

Add `use tests.join_suite` and `join_suite.suite(),` to `examples/dataframe/main.tw`.

Run: `TWK_TEST_FILTER="join" target/twk run examples/dataframe/main.tw`
Expected: PASS — `join` suite green.

Then format: `target/twk fmt examples/dataframe/frame/join.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/join.tw examples/dataframe/tests/join_suite.tw examples/dataframe/main.tw
git commit -m "dataframe: add hash join (inner + left)"
```

---

## Task 13: Full-suite green checkpoint

**Files:**
- None (verification only)

- [ ] **Step 1: Run the entire test suite**

Run: `target/twk run examples/dataframe/main.tw`
Expected: PASS — every suite (`cell`, `column`, `table`, `row`, `query`, `csv`, `group`, `join`) green, final line `Ran N tests: N passed`.

- [ ] **Step 2: Confirm the formatter is idempotent across the project**

Run: `target/twk fmt examples/dataframe/frame/cell.tw examples/dataframe/frame/column.tw examples/dataframe/frame/table.tw examples/dataframe/frame/row.tw examples/dataframe/frame/query.tw examples/dataframe/frame/csv.tw examples/dataframe/frame/group.tw examples/dataframe/frame/join.tw`
Expected: no diff on a second run.

- [ ] **Step 3: Commit any formatting changes**

```bash
git add -A examples/dataframe
git commit -m "dataframe: full-suite green checkpoint" || echo "nothing to commit"
```

---

## Task 14: Synthetic data generator

A seeded LCG PRNG generates a deterministic `Table` of N rows with a low-cardinality key column (for group/join stress), an int measure, and a float measure. Deterministic so benchmarks are reproducible.

**Files:**
- Create: `examples/dataframe/frame/gen.tw`
- Create: `examples/dataframe/tests/gen_suite.tw`
- Modify: `examples/dataframe/main.tw`

- [ ] **Step 1: Write the failing tests**

Create `examples/dataframe/tests/gen_suite.tw`:

```tw
use assert
use runner
use frame.table
use frame.gen

pub fn suite() {
  runner.suite("gen")
    .test(
      "generates the requested shape",
      fn() {
        t := gen.table(100, 8)
        ok1 := try assert.equal(t.nrows, 100)
        assert.equal(t.names, ["key", "amount", "score"])
      },
    )
    .test(
      "is deterministic for a fixed size",
      fn() {
        a := gen.table(50, 4).display()
        b := gen.table(50, 4).display()
        assert.equal(a, b)
      },
    )
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run examples/dataframe/main.tw`
Expected: FAIL — compile error, unknown module `frame.gen`.

- [ ] **Step 3: Implement `gen.tw`**

Create `examples/dataframe/frame/gen.tw`:

```tw
use frame.column
use frame.table
use frame.table.{Table}

/// Deterministic LCG step (Numerical Recipes constants), masked to 31 bits.
fn next_seed(seed: Int) Int {
  (seed * 1664525 + 1013904223) % 2147483648
}

/// Generate an N-row table with `key_cardinality` distinct string keys, an Int
/// `amount` column, and a Float `score` column. Deterministic given (n, cardinality).
pub fn table(n: Int, key_cardinality: Int) Table {
  keys: Vector<String> = []
  amounts: Vector<Int> = []
  scores: Vector<Float> = []

  seed := 12345

  for _ in range(n) {
    seed = next_seed(seed)
    k := seed % key_cardinality
    keys = .append("k${k}")

    seed = next_seed(seed)
    amounts = .append(seed % 1000)

    seed = next_seed(seed)
    scores = .append((seed % 10000).to_float() / 100.0)
  }

  cols := [
    column.str_col(keys),
    column.int_col(amounts),
    column.float_col(scores),
  ]

  case table.from_columns(["key", "amount", "score"], cols) {
    .Ok(t) => t,
    .Err(e) => error(e),
  }
}
```

- [ ] **Step 4: Wire the suite and run**

Add `use tests.gen_suite` and `gen_suite.suite(),` to `examples/dataframe/main.tw`.

Run: `TWK_TEST_FILTER="gen" target/twk run examples/dataframe/main.tw`
Expected: PASS — `gen` suite green.

Then format: `target/twk fmt examples/dataframe/frame/gen.tw`

- [ ] **Step 5: Commit**

```bash
git add examples/dataframe/frame/gen.tw examples/dataframe/tests/gen_suite.tw examples/dataframe/main.tw
git commit -m "dataframe: add deterministic synthetic data generator"
```

---

## Task 15: Benchmark harness

A standalone entrypoint that generates large tables and times `filter`, `order_by`, `group_by().agg()`, and `join` with `@std.date.now()`, printing elapsed milliseconds per op across scaling N. This is the perf-at-scale instrument (and the cliff-finder for `take`/gather).

**Files:**
- Create: `examples/dataframe/bench/main.tw`

- [ ] **Step 1: Implement the benchmark entrypoint**

Create `examples/dataframe/bench/main.tw`:

```tw
use @std.date
use frame.column
use frame.table
use frame.table.{Table}
use frame.query
use frame.query.{Dir}
use frame.group
use frame.join
use frame.join.{How}
use frame.gen
use frame.row.{RowRef}

fn time_ms(label: String, work: fn() Int) Void {
  start := date.now()
  guard := work()
  elapsed := date.now() - start
  println("${label}: ${elapsed}ms (checksum ${guard})")
}

fn bench_n(n: Int) Void {
  println("── N = ${n} ──")
  base := gen.table(n, 64)

  time_ms("filter      ", fn() {
    base.filter(fn(r) { r.int("amount") > 500 }).nrows
  })

  time_ms("order_by    ", fn() {
    case base.order_by("amount", Dir.Asc) {
      .Ok(t) => t.nrows,
      .Err(_) => -1,
    }
  })

  time_ms("group_by/agg", fn() {
    case base.group_by(["key"]).agg([group.sum("amount"), group.mean("score"), group.count()]) {
      .Ok(t) => t.nrows,
      .Err(_) => -1,
    }
  })

  time_ms("self-join   ", fn() {
    keyed := gen.table(n, 64)

    case join.join(base, keyed, "key", How.Inner) {
      .Ok(t) => t.nrows,
      .Err(_) => -1,
    }
  })

  println("")
}

bench_n(10000)
bench_n(100000)
bench_n(1000000)
```

> The `self-join` on a low-cardinality key intentionally explodes row counts; if `N = 1_000_000` traps or runs too long, lower the largest `bench_n` argument and record the ceiling in the friction log — that ceiling *is* a finding.

- [ ] **Step 2: Run the benchmark at the two smaller sizes first**

Run: `target/twk run examples/dataframe/bench/main.tw`
Expected: prints `filter`/`order_by`/`group_by/agg`/`self-join` timings for `N = 10000` and `100000`. Observe whether `N = 1000000` completes; if it traps or hangs, note where.

- [ ] **Step 3: Capture a timing snapshot**

Save the printed output to a scratch file for the friction log:

```bash
target/twk run examples/dataframe/bench/main.tw | tee /tmp/dataframe-bench.txt
```

- [ ] **Step 4: Commit**

```bash
git add examples/dataframe/bench/main.tw
git commit -m "dataframe: add benchmark harness (filter/order_by/group_by/join)"
```

---

## Task 16: Friction-log document

The actual deliverable of the stress test: a written log of ergonomic gaps and perf cliffs found while building and benchmarking the engine, mirroring the leetcode friction log.

**Files:**
- Create: `docs/plans/dataframe-friction-log.md`

- [ ] **Step 1: Write the friction log**

Create `docs/plans/dataframe-friction-log.md` with these sections, filled in from real experience during Tasks 1–15 (not invented):

```markdown
# Dataframe stress test — friction log

Companion to `docs/plans/dataframe-stress-test.md`. Records what building a
multi-module columnar query engine revealed about Twinkle ergonomics and
collection performance.

## Ergonomic findings
- (one bullet per real friction or pleasant surprise, with a concrete code example
  and whether it was a GAP, a POSITIVE, or a TRADEOFF — same style as the leetcode log)

## Capability-record / no-trait observations
- How did uniform `Aggregation` records + `fn(Table, Vector<Int>) Cell` feel vs. wanting
  real traits/existentials? Where did the enum-tag dispatch on `ColData` get repetitive?

## Null-mask ergonomics
- Did carrying the parallel `Vector<Bool>` through every op cost more than expected?
  Where did null propagation read cleanly vs. awkwardly?

## Performance at scale
- Paste the `bench/main.tw` timings (from /tmp/dataframe-bench.txt).
- Call out the `take`/gather cost (PVec random access) at 1e5 / 1e6 rows.
- Note any size at which an op trapped, hung, or blew up (especially the self-join).
- Compare group_by (Dict-HAMT) vs order_by (sort_by) vs join scaling.

## Recommendations
- Concrete language/stdlib changes this project motivates (ranked), e.g. missing Vector
  bulk ops, a typed-column abstraction, faster gather, etc.
```

- [ ] **Step 2: Fill every section from real observations**

Go back through the implemented tasks and the benchmark output and replace each
bullet placeholder with concrete findings. Do **not** leave any parenthetical
prompt text. Every claim must trace to something actually observed.

- [ ] **Step 3: Commit**

```bash
git add docs/plans/dataframe-friction-log.md
git commit -m "dataframe: add stress-test friction log"
```

---

## Self-Review (completed during planning)

**Spec coverage:** Every spec section maps to tasks — core types/null mask (Tasks 2–4, 9), `take` primitive (Task 4), capability-record aggregations (Tasks 10–11), CSV ingest (Task 8), filter/with_column/order_by (Tasks 6–7), hash join (Task 12), bench harness + friction log (Tasks 14–16). `RowRef` (Task 5) backs the filter predicate API. JSON ingest and `stddev`/quantile/multi-key joins are explicitly out of scope per the spec and are not tasked.

**Type consistency:** `Cell`, `Column`/`ColData`/`DType`, `Table`, `RowRef`, `Aggregation`/`GroupBy`, `Dir`, `How` are defined once and referenced with the same field/variant names throughout (`apply: fn(Table, Vector<Int>) Cell` is used identically in Tasks 10 and 11; `from_cells` defined in Task 9 is consumed in Tasks 11 and 12; `take`/`gather` names are stable).

**Placeholder scan:** Code steps contain complete implementations. The only intentionally-unfilled content is the friction log body (Task 16), which by nature must be written from real observations — Step 2 of that task explicitly requires replacing every prompt bullet.

**Known risks to watch during execution (verify against the live compiler, fix inline):**
1. **`(a.apply)(...)` call syntax** — calling a record field that holds a function. If the parser rejects the parenthesized form, introduce a tiny `fn run_agg(a: Aggregation, t: Table, idx: Vector<Int>) Cell { (a.apply)(t, idx) }` helper or whatever call form the compiler accepts, and use it everywhere an aggregation is invoked.
2. **`\u{1f}` string escape** (Task 11 `group_key`) — if unsupported, fall back to a `"||"` separator and note the collision caveat.
3. **`for _ in range(n)` / `collect i in range(n) { i }`** — if a bare `_` loop binder or `range` is unavailable, switch to a `j := 0; for j < n { ...; j = j + 1 }` counter.
4. **`Vector.make(n, fill)` for `Bool`** — confirmed in `docs/API.md`; if a typed-fill issue arises, build the mask with an append loop.
These are surfaced here so the executor checks them early rather than discovering them mid-task.
