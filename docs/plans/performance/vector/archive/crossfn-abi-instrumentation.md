# Cross-function typed-vector ABI — instrumentation findings

**Branch:** `typed-vector-crossfn-abi`. Written 2026-07-05, before any design work.
This is the "instrument WHY before designing blind" step from
[typed-vector-continue-here.md](typed-vector-continue-here.md) §1.

## Method

Built the dataframe bench to WAT and traced an `IntCol` column from construction
through the `order_by` hot path:

```bash
target/twk build examples/performance/dataframe/bench/main.tw -o /tmp/df.wat
```

## Storage IS typed (Milestone A works)

The variant payload is genuinely unboxed — no ambiguity here:

```wat
(type $user__$ColData_t22 (sub (struct
  (field $tag i32)
  (field $IntCol_0 (ref null $rt_types__PVecI64))   ;; <- typed, not PVec
  (field $FloatCol_0 (ref null $rt_types__PVec)) ...)))
```

So every box we see below comes from **crossing a boundary**, never from storage.

## Three boundaries box the typed column

All three are boxed-only ABIs; the typed `PVecI64` boxes the instant it is
*consumed* across any of them.

### 1. Return-as-`Vector<Int>` ABI — `as_ints` (`column.tw:76`)

`.IntCol(v) => v` returns `Vector<Int>`, whose default ABI is boxed `PVec`:

```wat
struct.get $user__$ColData_t22 1     ;; extract PVecI64
call $rt_arr__box_i64                 ;; box to return
```

### 2. Generic runtime-helper ABI — `gather` (`column.tw:105`), also `take`

`.IntCol(v) => ColData.IntCol(v.gather(idx))`: extract typed → box → call the
**generic boxed** `rt_arr__gather` → unbox back into the new IntCol:

```wat
struct.get $user__$ColData_t22 1
call $rt_arr__box_i64
call $rt_arr__gather                  ;; runs entirely on boxed PVec
call $rt_arr__unbox_i64               ;; store result back typed
```

The gather loop itself operates on boxed `PVec`, so Milestone A's typed storage
buys **zero** read speedup here. This is the ~830ms direct-read half of
`order_by` (`gather`/`take`).

### 3. Closure-env capture ABI — `sort_indices_by_column` (`table.tw:244`)

The comparator half (~1317ms). `idx.sort_by(fn(a,b){ keys[a] … })` captures the
key column into the `anyref ClosureEnv`. The column is boxed *before* capture:

```wat
struct.get $user__$ColData_t22 1       ;; extract PVecI64 key column
call $rt_arr__box_i64                   ;; box it
array.new_fixed $rt_types__ClosureEnv 2 ;; capture BOXED into anyref env
struct.new $user__$closure_fn_i64_i64_t7
```

And the comparator body (`f342`) reads it with the **boxed** routine, twice per
compare, inside the O(n log n) loop:

```wat
struct.get $rt_types__PVec 0
call $rt_arr__get                       ;; boxed read, NOT rt_arr__get_i64
```

`order_by` (`f340`) itself builds the index vector typed
(`rt_arr__builder_freeze_i64`) then boxes it to call `sort_indices_by_column`,
whose param ABI is boxed (`functype_105` param = `PVec`).

## Build side (same root cause)

`gen.table`'s `amounts` local is boxed for the **entire append loop** — every
local in `f351_table` is `(ref null $rt_types__PVec)`. Reason: `amounts` escapes
to `int_col`, whose param ABI is boxed (`functype_87` param = `PVec`), so
`route_typed_vec`'s escape guard leaves it boxed. The typed payload only
materializes via `unbox_i64` *inside* `int_col` at the storage crossing. So the
build never benefits from the typed rep either — same boxed-param-ABI root cause.

## Root cause (one sentence)

Function params/returns and the closure env are **boxed-only ABIs**
(`int_col` param, `sort_indices_by_column` param+result, `ClosureEnv = array
anyref`); the typed rep is confined to storage sites and boxes the instant it
crosses any of them.

## Implication for the design fork

- Boundaries #1 and #2 (return + generic-helper ABI) are **step 1** — a
  cross-function monomorphic typed ABI (specialize-by-representation vs
  adapt-at-call). Fixing them wins the ~830ms gather/take path and lets the
  column stay typed end-to-end from `int_col` build through `gather`/`take`.
- Boundary #3 (closure capture) is **step 2 / M1b** — needs a per-capture-repr
  closure env, and depends on #1 (nothing typed to capture until the column
  stops boxing at the `int_col` / `sort_indices` param boundary).
