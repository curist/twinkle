# Dataframe stress test — friction log

Companion to `docs/plans/dataframe-stress-test.md` (spec) and
`docs/plans/dataframe-stress-test-plan.md` (impl plan). Records what building a
multi-module columnar query engine (`examples/dataframe/`) revealed about Twinkle's
app-level ergonomics and collection performance — the actual deliverable of the
stress test. All findings are from the real build; nothing here is hypothetical.

The engine that produced these notes: `frame/{cell,column,table,row,query,csv,group,join,gen}.tw`,
the dataframe test suite, plus a `bench/main.tw` harness.

## Headline finding: fluent method chains don't survive module boundaries

The approved design pitched a fluent API:

```tw
table.filter(...).group_by([...]).agg([...]).order_by(...)
```

**This is not achievable across modules.** Twinkle's inherent-method (dot) sugar
`x.method(...)` resolves **only when `method` is defined in the same module that
defines `x`'s type** (documented in CLAUDE.md, but its consequence for fluent APIs
is easy to miss). `Table` is defined in module `table`; `filter`/`order_by` live in
`query`, `join` in `join`. So:

- `t.filter(...)` → `type 'Table' has no method 'filter'` (compile error)
- Must write `query.filter(t, ...)`, `query.order_by(t, ...)`, `join.join(l, r, ...)`.

The one place chaining *does* work is `group.group_by(t, ["k"]).agg([...])`: `group_by`
returns `GroupBy` and `.agg` is defined in `GroupBy`'s own module (`group`), so that
hop resolves. That asymmetry is instructive — the rule is consistent, it just doesn't
match the "objects have methods" mental model a fluent dataframe API wants.

**The constraint is twofold and was verified empirically:**
1. Inherent-method sugar resolves only in the receiver type's defining module
   (`type 'Table' has no method 'filter'` when `filter` is in `query`).
2. **Circular module imports are a hard error** — a two-file experiment with mutual
   `use` produced `Circular import detected`. So you cannot even keep the
   implementation in a separate module and add a thin delegating wrapper in
   `table.tw`: the impl module must import `Table`, and `table` importing it back is
   the cycle. To make an op an inherent method, *its whole implementation* must live
   in `Table`'s module.

**What we did (partial reorg).** We moved the row-level ops — `RowRef` +
`filter`/`with_column`/`order_by`/`Dir` — into `table.tw`, so the common chain works:
```tw
t.filter(fn(r) { r.int("amount") > 500 }).order_by("amount", Dir.Asc)   // try-wrapped, see below
```
`group_by`/`agg` and `join` stayed in their own modules as qualified calls
(`group.group_by(t, [...]).agg([...])`, `join.join(l, r, ...)`). This keeps `table.tw`
moderate (~250 lines) rather than absorbing the entire engine. Full chaining through
group-by/join would require pulling `GroupBy`/`Aggregation`/`How` and their logic into
`table.tw` too, since they all reference `Table`.

Two incidental facts learned during the reorg:
- **Inherent calls do NOT require importing the defining module.** `base.filter(...)`
  compiles with no `use frame.table` at all — resolution is purely by the value's type.
  You only `use frame.table.{Dir}` to name the `Dir` *type*. So the dot-method surface
  is import-light; it's the *definition site* that's constrained.
- **`Result`-returning ops break the visual chain.** `order_by`/`with_column`/`group_by`
  return `Result`, so a real chain needs `try`: `try t.filter(p).order_by("x", .Asc)`.
  The approved design's clean preview chain is only literally writable for the
  infallible hops.

**Possible language change (still the #1 ask):** allow inherent-method resolution for
`pub fn`s in *any* module whose first parameter is the receiver type (UFCS-style), or an
opt-in `extend Table { ... }` block in another module. Either would let the engine keep
its clean module split *and* offer full fluent chaining — the one thing this project
could not have both of.

## Ergonomic findings

- **GAP — `Cell` is a reserved type name.** `pub type Cell = {...}` fails with
  `type name 'Cell' is reserved` (it's the builtin mutable reference cell). The scalar
  enum had to be renamed `Scalar`. Minor, but reserved-name collisions aren't
  discoverable until you hit them; there's no list in the docs of reserved type names
  (`Cell`, and presumably `Option`/`Result`/`Range`/`Iterator`/`Array`...).

- **MOSTLY USER-FIXABLE — `assert.equal` needs `Stringify`, which enums don't auto-derive.**
  `assert.equal<T: Eq + Stringify>` requires `Stringify`, and **enums get auto-derived
  structural `Eq` but NOT auto `Stringify`.** So `assert.equal(column.dtype(c), DType.DFloat)`
  first failed with `type DType does not satisfy Stringify`. The correct framing (thanks
  to review feedback): `assert.equal` is ordinary user-level code that legitimately asks
  for `Stringify`, so the fix is to **give the enum a `to_string`**, not to work around
  the assert. We did exactly that — `DType`, `Dir`, and `How` each got a one-line
  `pub fn to_string(...)` in their module, and the `DType` assertions now use
  `assert.equal` directly. This is the idiomatic resolution and it's cheap.
  - **Residual real gap: builtin `Order` has no Stringify witness.** `Order` is a
    *compiler builtin* with no prelude `.tw` module, so there is nowhere user code can
    attach an `Order.to_string`. Its assertion still uses the `== Order.Lt`/`is_true`
    workaround. That part is a genuine (small) **stdlib gap**: builtins returned by core
    APIs (`Int.compare → Order`) should ship a `to_string`/Stringify witness so they're
    usable with the same `assert.equal`/interpolation as everything else.
  - **Secondary nicety:** auto-derivable `Stringify` for simple (payload-free) enums
    would remove even the one-liners — but that's an enhancement, not the gap.

- **NOT A GAP (corrected) — nested patterns DO compile.** During the build a subagent
  reported that `case opt { .Some(.CInt(_)) => ... }` was rejected and rewrote it to a
  two-level `case opt { .Some(s) => case s { .CInt(_) => ... } }`. That was a
  **misdiagnosis** (same failure mode as the historical `known_empty` episode): nested
  constructor patterns work in both compilers. Verified by direct repro
  (`.Some(.CInt(_))` and binding-inner `.Some(.CInt(n)) => "${n}"` both run), by the
  boot compiler's own source using them (`base_env.tw`: `.Some(.Record(name, _, _))`,
  `prelude/result.tw`: `.Some(.Ok(v))`), by the dedicated regression test
  `boot/repros/nested_variant_pattern_repro.tw`, and by the fact that stage0 must
  compile that boot source to bootstrap. `from_cells` has been simplified back to the
  nested form. **Lesson for this log: treat single-subagent "X doesn't compile" claims
  as hypotheses until reproduced minimally.**

- **TRADEOFF — the two-import-line rule bites within your own project, not just stdlib.**
  `use frame.cell.{Scalar}` brings the *type* into scope but NOT the module alias, so
  `cell.to_string(...)` is undefined until you also add `use frame.cell`. The
  CLAUDE.md note frames this as a stdlib-generic-type thing (`@std.view`), but it
  applies to any first-party module that exports both a type and functions you call
  qualified. Every `frame/*` consumer needed the doubled import. Predictable once
  learned, but it's boilerplate that reads like a mistake.

- **POSITIVE — function-valued record fields are first-class and clean.**
  `Aggregation = .{ name: String, apply: fn(Table, Vector<Int>) Scalar }` with
  `(a.apply)(t, idx)` compiled on the first try. This is what makes the no-trait
  "capability record" pattern actually pleasant (see below).

- **POSITIVE — immutable rebind + index/field-assign sugar.** `d[key] = idxs.append(row)`,
  `raw[c] = raw[c].append(f)` (auto-formatted to `.append(f)`), `out = .append(x)`
  read like mutation while staying persistent. Building columns, buckets, and output
  tables via rebind never once produced an aliasing surprise — the Python `[[]]*n`
  footgun simply can't happen.

- **POSITIVE — inference + `collect` + `for x,i in`.** `collect i in range(t.nrows) { i }`
  for an index vector, `for c, i in t.cols` for column+position, `idx.sort_by(fn(a, b) { ... })`
  with inferred comparator param types, and contextual closures whose return type comes
  from the expected `fn(...) Scalar` field — all worked without annotation ceremony.

- **POSITIVE — `Dict` first-insertion order is a quiet win.** Group-by output order is
  deterministic for free because `Dict<String, Vector<Int>>` preserves first-seen key
  order; no separate sort of group keys needed. (We still keep a parallel
  `order: Vector<String>`, but only to iterate without re-hashing.)

- **MINOR — `String.split("\n")` blank-line semantics.** A trailing newline yields a
  trailing `""` element, and a blank *interior* line is a real empty row (a null cell
  in a 1-column CSV). The naive "drop all empty lines" loses null rows; the loader had
  to drop only a single trailing empty element. Not a language gap, but a sharp edge
  in the most obvious CSV-splitting code.

## Capability-record / no-trait observations

The uniform `Aggregation = .{ name, apply: fn(Table, Vector<Int>) Scalar }` record is
the project's stand-in for a `trait Aggregator`. Verdict: **it works and it's honest,
but it pushes type erasure onto the value boundary.**

- Heterogeneous aggregations (`[sum("amount"), mean("score"), count()]`) coexist in one
  `Vector<Aggregation>` precisely because `apply` erases each one's internal accumulator
  type down to `fn(Table, Vector<Int>) Scalar`. With traits you'd keep the accumulator
  typed; here the accumulator lives captured inside the closure and the *result* is the
  boxed `Scalar`. That's a reasonable trade, and closures-over-config made the
  constructors (`sum(col)`, `extreme(col, prefix, keep)`) compact.
- The cost is at the **column edges**, not the algorithm: every aggregation produces a
  `Scalar`, then `from_cells` re-infers a dtype to repack a typed `Column`. So a
  group-by does unbox→box→re-infer per group cell. For the analytic core this is fine
  (few groups), but it means the "unboxed columnar" performance story has a boxed seam
  exactly at aggregation output.
- **Enum-tag dispatch on `ColData` is the repetitive part.** Nearly every column
  operation is a 4-arm `case c.data { .IntCol(v) => ..., .FloatCol(v) => ..., ... }`
  with near-identical bodies (`gather`, `compare_at`, `cell_at`, `from_cells` packers,
  agg `sum`). Without higher-kinded types or a generic-over-primitive mechanism there's
  no way to write the body once. This is the no-traits + no-HKT cost made concrete: the
  4× duplication is the price of unboxed primitive columns.

## Null-mask ergonomics

Carrying a parallel `Vector<Bool>` mask alongside dense unboxed data was **cheaper than
feared and read cleanly** in most places:

- `gather` threads the mask by indexing it with the same `idx` — one extra loop, no
  special cases.
- Aggregations skip nulls with a plain `if !column.is_null(c, i)` guard inside the fold.
- `cell_at` collapses a masked cell to `Scalar.CNull`, which is the single choke point
  where "missing" becomes visible at the value boundary — nice.
- The one awkward spot is **`from_cells` on an all-null input**: with no non-null scalar
  to infer a dtype from, it must *guess* (defaults to `FloatCol`). This surfaces in a
  left join where a right column's matched rows are all null — the output column's dtype
  is then arbitrary. A real engine would carry the source dtype through the join rather
  than round-tripping through `Scalar`; our index-vector + `gather_nullable` shortcut
  trades that correctness corner for simplicity (documented MVP limitation).
- `RowRef` typed accessors (`r.int("age")`) **trap on null** by design, so predicates
  over nullable columns must pre-check `r.is_null(...)`. Ergonomic for clean data, a
  sharp edge for dirty data — an `r.int_opt("age") Int?` companion would soften it.

## Performance at scale

From `bench/main.tw` (`/tmp/dataframe-bench.txt`), times in ms, synthetic data
(`gen.table`), key cardinality 64 for group-by, near-unique right keys for join:

```
── N = 10000 ──
filter      : 2.18ms    (checksum 4912)
order_by    : 11.69ms   (checksum 10000)
group_by/agg: 4.76ms    (checksum 64)
join        : 7.12ms    (checksum 8597)

── N = 100000 ──
filter      : 17.54ms   (checksum 49735)
order_by    : 150.84ms  (checksum 100000)
group_by/agg: 27.99ms   (checksum 64)
join        : 110.80ms  (checksum 78120)

── N = 1000000 ──
filter      : 210.77ms  (checksum 498802)
order_by    : 2808.96ms (checksum 1000000)
group_by/agg: 339.99ms  (checksum 64)
join        : 1552.94ms (checksum 937500)
```

Observations:

- **Nothing trapped or OOM'd at 1M rows.** PVec (32-way trie) and Dict (HAMT) both held
  up across two orders of magnitude. This is the strongest positive: a from-scratch
  columnar engine over a million rows runs in single-digit seconds with no tuning.
- **`filter` is clean O(n)** (~2 → 17.5 → 211ms, ≈10× per 10× rows). The per-row
  `RowRef` allocation + `r.int(...)` (column lookup by name, mask check, `as_ints`,
  index) costs ~0.2µs/row — acceptable, though the column-name `position` scan inside
  every `r.int` is redundant work that a row-cursor caching the column could remove.
- **`order_by` is the cost center: ~12 → 151 → 2809ms**, growing faster than linear
  (≈13× then ≈18.6× per 10× rows) — the `n log n` comparison sort plus a **full `take`
  gather of all N rows** through PVec random access (O(log32 n) per element). This is
  the predicted gather cliff: at 1M, `order_by` is ~13× slower than `filter` even though
  both end with a `take`, because filter gathers ~half the rows in original order while
  order_by gathers all N in shuffled order (cache-hostile trie walks). **Highest-value
  perf target:** a specialized sort-and-gather, or a primitive `Vector.gather(idx)` /
  `Vector.permute` that walks the trie once, would directly cut the dominant cost.
- **`group_by/agg` scales ~linearly and stays cheap** (~5 → 28 → 340ms) despite the
  unbox→box→re-infer seam, because only 64 groups are produced — the HAMT inserts
  dominate and they're fast. The boxed-`Scalar` aggregation seam did **not** show up as
  a bottleneck at these group counts (it would for high-cardinality group-bys).
- **`join` ~linear** (~7 → 111 → 1553ms) with near-unique keys: HAMT build over the
  right side + probe + two `take`s + `gather_nullable`'s `Scalar` round-trip. The
  `gather_nullable` boxing is extra work the inner join doesn't need (no -1 indices) —
  a fast path that calls plain `column.gather` when `how == Inner` would help.

### Phase 1 gather-path optimization results

After rewriting `column.gather` to use `collect`, routing join null-fill through typed
`column.gather_or_null`, and switching `head` to structural `column.slice`, the same bench
harness produced:

```
── N = 10000 ──
filter      : 2.39ms    (checksum 4912)
order_by    : 12.19ms   (checksum 10000)
group_by/agg: 4.77ms    (checksum 64)
join        : 6.58ms    (checksum 8597)

── N = 100000 ──
filter      : 17.53ms   (checksum 49735)
order_by    : 151.68ms  (checksum 100000)
group_by/agg: 28.54ms   (checksum 64)
join        : 83.97ms   (checksum 78120)

── N = 1000000 ──
filter      : 213.08ms  (checksum 498802)
order_by    : 2823.07ms (checksum 1000000)
group_by/agg: 333.73ms  (checksum 64)
join        : 1380.43ms (checksum 937500)
```

The clear movement is join: removing the `Scalar` round-trip from right-column null-fill
cuts a meaningful chunk of the large left-join case. `filter` and `order_by` are essentially
flat, which matches the cost model: Phase 1 removes append-loop overhead but still does one
indexed vector read per gathered cell, while `order_by` remains comparator-bound. `group_by`
is roughly unchanged within benchmark noise.

A follow-up added the v1 `Vector.gather` runtime primitive and routed `column.gather`
through it. That run produced:

```
── N = 10000 ──
filter      : 2.12ms    (checksum 4912)
order_by    : 12.03ms   (checksum 10000)
group_by/agg: 4.82ms    (checksum 64)
join        : 6.67ms    (checksum 8597)

── N = 100000 ──
filter      : 17.89ms   (checksum 49735)
order_by    : 154.77ms  (checksum 100000)
group_by/agg: 26.76ms   (checksum 64)
join        : 84.40ms   (checksum 78120)

── N = 1000000 ──
filter      : 214.92ms  (checksum 498802)
order_by    : 2625.39ms (checksum 1000000)
group_by/agg: 339.00ms  (checksum 64)
join        : 1398.56ms (checksum 937500)
```

As expected, the v1 runtime gather is mostly flat against Phase 1: it consolidates the loop
into the runtime but still performs one trie lookup per requested index. The `order_by` number
moved in this run, but that workload is dominated by sort-comparator behavior and shows enough
run-to-run variance that this should not be attributed to gather alone without a targeted
microbenchmark.

A further `order_by` pass moved the `ColData` dispatch outside the sort comparator and uses
per-dtype comparator closures over the typed key vector. The latest run was:

```
── N = 10000 ──
filter      : 2.13ms    (checksum 4912)
order_by    : 12.06ms   (checksum 10000)
group_by/agg: 4.75ms    (checksum 64)
join        : 6.40ms    (checksum 8597)

── N = 100000 ──
filter      : 17.83ms   (checksum 49735)
order_by    : 147.35ms  (checksum 100000)
group_by/agg: 27.60ms   (checksum 64)
join        : 85.41ms   (checksum 78120)

── N = 1000000 ──
filter      : 209.49ms  (checksum 498802)
order_by    : 2530.79ms (checksum 1000000)
group_by/agg: 337.20ms  (checksum 64)
join        : 1481.51ms (checksum 937500)
```

This is directionally better for `order_by`, but still modest: the comparator still performs
random vector reads and null checks for each comparison. Larger gains likely require a deeper
sort strategy change (for example materializing null ranks/keys into a representation with
cheaper repeated reads, or a specialized typed/key sort), plus a later trie-aware gather path.

**Conclusion.** The gather-path plan is complete and archived in
`docs/plans/archive/vector-gather.md`. It delivered API/implementation cleanup, removed the
join `Scalar` round-trip, made `head` structural, and added the reusable `Vector.gather` API.
It did **not** deliver a meaningful `filter` win because v1 gather is still an independent
lookup loop; the promised monotonic-index benefit remains future work for a trie-aware gather
implementation. `order_by` improved only modestly after comparator specialization and should
be treated as a separate typed/key-sort problem rather than a gather problem.

## Recommendations (ranked)

1. **Cross-module inherent methods (UFCS or `extend`).** Without it, fluent
   library APIs force everything into one module — verified hard by the circular-import
   error. We shipped a *partial* reorg (row-level ops into `table.tw`) to get
   `t.filter(...).order_by(...)`, but full chaining through group-by/join still can't
   coexist with a clean module split. This is the #1 finding and it's architectural.
2. **[DONE] `Stringify` witness for builtin `Order`.** Shipped on `main`
   (`Order.to_string`, wired like `option`/`result`). User enums are fixable by adding
   `to_string` (we did, for `DType`/`Dir`/`How`); builtins with no module to attach to
   weren't, hence the language fix. Remaining: audit other builtins returned by core
   APIs, and optionally auto-derive `Stringify` for payload-free enums.
3. **Typed/key sort strategy for `order_by`, plus a later trie-aware gather.** The comparator
   dominates sorted workloads because it performs random vector reads on every comparison;
   moving `ColData` dispatch out of the comparator helped only modestly. A trie-aware
   `Vector.gather(idx)` would still help shared `take` paths, especially monotonic selections,
   but it is not the main `order_by` lever by itself.
4. **A way to write `ColData` 4-arm dispatch once** (generic-over-primitive, or codegen).
   The 4× duplication in `gather`/`compare_at`/`cell_at`/`from_cells`/agg is the
   concrete tax of no-traits + no-HKT for unboxed columns.
5. **Reserved-name documentation.** List reserved type names (`Cell`, `Option`,
   `Result`, ...) in the docs so collisions are designed around, not discovered.
6. **Nullable accessors on the row view** (`RowRef.int_opt`), for predicates over dirty
   data without trap-or-precheck.

## What held up without complaint

Generics (`from_cells<…>`-style monomorphic helpers, `Vector<Column>`,
`Dict<String, Vector<Int>>`), `Result`/`try` propagation through every fallible op,
`case`/`cond`, closures with inferred params, `collect`/`range`, record field-pun and
named-constructor literals, and the persistent collections under load. The language was
productive; the friction was concentrated in the findings above, and only the first
(cross-module methods) changed the *shape* of the result rather than just its verbosity.
