# Static Uniqueness Plan

## Goal

Extend Twinkle's static uniqueness optimizer so it catches more realistic
linear-update cases without changing the runtime model.

This plan keeps Twinkle's current design constraints:

- no runtime refcounts
- no runtime uniqueness flags
- no user-visible ownership or borrow annotations
- no whole-program alias theorem proving

It focuses on improving `src/opt/uniqueness.rs` while keeping the optimizer
local, understandable, and cheap.

This plan consolidates the earlier:

- [`deferred-persistence.md`](./deferred-persistence.md)
- the previous static uniqueness next/follow-up notes

## Current Status

Several planned phases are already complete.

### Completed

#### Phase A: reassign-aware taint refinement

The pre-scan no longer globally over-taints some reassigned locals just because
their final value escapes later.

This unlocked sequential dict-build patterns such as:

```tw
fn build() Dict<String, Int> {
  d: Dict<String, Int> = Dict.new()
  d["a"] = 10
  d["b"] = 20
  d
}
```

when the only escape is after the update chain.

#### Phase B: fresh-after-COW results

Known COW operations with in-place variants now mark their result as fresh when
forced to copy from a shared base. This lets later updates on the copied result
rewrite in place.

#### Phase B½: refreshed tracking

The pass now tracks a `refreshed` set for tainted locals that have been
reassigned from a fresh unique source. This removed a major asymmetry where
record updates could regain precision only in limited cases.

#### Phase F1: shared reusable-base predicate

The pass now uses a shared base predicate for:

- `ARecordUpdate`
- `DICT_SET`
- `DICT_REMOVE`
- `VECTOR_SET_UNSAFE`

So tainted-but-refreshed locals are handled consistently.

#### Phase F2: relaxed consume-reassign recognition

`is_consume_reassign` no longer requires exact adjacency. It now handles
transparent forwarding bind chains such as:

```tw
tmp := Dict.set(d, "a", 10)
tmp2 := tmp
d = tmp2
```

#### Phase F4: branch merge propagation

For `AIf` and `AMatch`, the pass now intersects post-branch facts for:

- `unique`
- `known_empty`
- `refreshed`

This allows branch-local freshness to remain useful after the join.

#### Phase F5/F6: tiny wrapper support

A summary-based tiny-wrapper path now recognizes direct wrappers around known
update operations.

Current coverage includes caller-side optimization through wrappers for:

- `VECTOR_APPEND`
- `DICT_SET`
- `DICT_REMOVE`

including:

- straight-line cases
- loop dict rewrites
- transparent forwarding-bind wrappers

The helper body itself remains unchanged; the caller-side sites are what rewrite.

## Why Another Consolidated Plan Is Needed

The optimizer now catches several high-value patterns, but the implementation is
still somewhat recognizer-heavy and uneven across operation families.

The main remaining issues are:

- some logic still depends on ANF shape more than semantics
- vector and dict rewrites still use different permissiveness rules
- straight-line and loop append rewrites are still narrow relative to real code
- helper support is good for tiny wrappers, but not yet a general function
  summary system
- `VECTOR_CONCAT` and other multi-base ops still have no dedicated strategy

These are precision and architecture issues, not soundness bugs.

## Non-Goals

- No runtime uniqueness metadata
- No borrow checker or ownership syntax
- No requirement to optimize every persistent-data-structure case
- No exponential path enumeration
- No immediate whole-program interprocedural alias analysis

## Current Characterization Baseline

The following fixtures should remain as explicit guardrails.

### Refresh / consume-reassign / join propagation

- `tests/opt/dict_set_param_assign_back_chain.tw`
- `tests/opt/dict_set_param_forward_bind_chain.tw`
- `tests/opt/dict_set_after_if_join.tw`

### Tiny-wrapper coverage

- `tests/opt/vector_append_helper_wrapper.tw`
- `tests/opt/vector_append_helper_forward_wrapper.tw`
- `tests/opt/dict_set_helper_wrapper.tw`
- `tests/opt/dict_set_helper_forward_wrapper.tw`
- `tests/opt/dict_remove_helper_wrapper.tw`
- `tests/opt/dict_remove_helper_forward_wrapper.tw`
- `tests/opt/dict_set_loop_helper_wrapper.tw`
- `tests/opt/dict_set_loop_helper_forward_wrapper.tw`
- `tests/opt/dict_remove_loop_helper_wrapper.tw`
- `tests/opt/dict_remove_loop_helper_forward_wrapper.tw`

### Important negative case

Keep this as a deliberate non-optimization:

- `tests/opt/vector_append_loop_reads_acc_not_rewritten.tw`

Reason: allowing in-loop reads of the logical accumulator without rewriting
those reads to builder-aware operations caused a real Wasm semantic bug.

So vector builder rewriting must remain stricter than dict loop rewriting until
read semantics are handled explicitly.

## Measurements

Reproduce with:

```bash
cargo test --release --test cow_analysis -- --nocapture
```

### Pre-optimizer baseline

| Operation | Pre-opt | Post-opt | Rewritten |
|---|---:|---:|---|
| VECTOR_APPEND | 427 | 391 | 36 → builder |
| DICT_SET | 227 | 170 | 57 → DICT_SET_IN_PLACE |
| VECTOR_CONCAT | 106 | 106 | none |
| REC_UPDATE | 98 | 98 COW | none |
| VECTOR_SET_UNSAFE | — | 0 | 14 → VECTOR_SET_IN_PLACE |
| DICT_REMOVE | 6 | 6 | none |

**Total COW remaining: 777**

### After A/B + refreshed-set work

| Operation | Pre-opt | Post-opt | Rewritten |
|---|---:|---:|---|
| VECTOR_APPEND | 427 | 391 | 36 → builder |
| DICT_SET | 227 | 141 | 86 → DICT_SET_IN_PLACE |
| VECTOR_CONCAT | 106 | 106 | none |
| REC_UPDATE | 98 | 75 COW | 23 → REC_UPDATE_IN_PLACE |
| VECTOR_SET_UNSAFE | — | 0 | 14 → VECTOR_SET_IN_PLACE |
| DICT_REMOVE | 6 | 6 | none |

**Total COW remaining: 725**

### After the recent follow-up work

From current `cow_analysis` comparison against `d2dba2f`:

- total remaining COW: `777 -> 723`
- `DICT_SET`: `170 -> 139`
- `DICT_SET_IN_PLACE`: `57 -> 88`
- `REC_UPDATE_COW`: `98 -> 75`
- `REC_UPDATE_IN_PLACE`: `0 -> 23`

### After R1 + R2 (known-empty builder relaxation)

| Operation | Pre-opt | Post-opt | Rewritten |
|---|---:|---:|---|
| VECTOR_APPEND | 427 | 237 | 190 → builder |
| DICT_SET | 227 | 139 | 88 → DICT_SET_IN_PLACE |
| VECTOR_CONCAT | 106 | 106 | none |
| REC_UPDATE | 98 | 75 COW | 23 → REC_UPDATE_IN_PLACE |
| VECTOR_SET_UNSAFE | — | 0 | 14 → VECTOR_SET_IN_PLACE |
| DICT_REMOVE | 6 | 6 | none |

**Total COW remaining: 569** (was 723 pre-R2, 777 at baseline)

Interpretation:

- dict and record improvements continue showing up in the boot compiler
- the R2 known-empty relaxation unlocked the largest remaining vector
  accumulator patterns (parser, emitter, resolver)
- `VECTOR_APPEND` dropped from 391 to 237 (−154)
- biggest remaining pain is now `VECTOR_CONCAT` (106 remaining, untouched)

## Current Remaining Hotspots

Recent `cow_analysis` output points to three main buckets.

### 1. Vector-heavy parser / emitter code

After R2, many previously dominant VECTOR_APPEND hotspots were resolved.
Remaining hotspots are now dominated by VECTOR_CONCAT:

- `emit_wat_parts` (15 VECTOR_APPEND remaining)
- `parse_prefix` (10 VECTOR_APPEND + 4 VECTOR_CONCAT)
- `parse_type_expr_base` (7 VECTOR_APPEND + 5 VECTOR_CONCAT)
- `parse_function` / `parse_type` (5 VECTOR_APPEND + 4 VECTOR_CONCAT each)
- `parse_if_expr` / `parse_collect_expr` / `parse_expr_bp` / `parse_for_stmt`
  (pure VECTOR_CONCAT, 5 each)

VECTOR_CONCAT (106 remaining, 0 optimized) is now the single largest
remaining operation family. The next global win is a dedicated concat strategy.

### 2. Dict-building linker / registry code

Examples:

- `boot/compiler/codegen/linker.tw::link`
- `boot/compiler/opt/analysis.tw::scan_tainted_op`

These still contain substantial dict update traffic and are good indicators for
further dict precision improvements.

### 3. Record-update-heavy env / registry code

Examples:

- `register_layout_type_def`
- `register_type_entry`
- `lower_module`

These already improved, but still contain mixed record/dict/vector update paths,
often under branchy control flow.

## Remaining Problems

## 1. Vector and dict analyses still differ too much

Current situation:

- dict loop analysis allows read-only uses such as `len`, `get`, `has`
- vector builder rewrites must still reject some analogous in-loop reads for
  soundness reasons

This asymmetry may still be too broad, but it cannot be relaxed casually.

### Consequence

There is still no principled shared operation model across:

- taint pre-scan
- point rewrites
- loop rewrites
- straight-line builder rewrites

## 2. Straight-line append rewrite is still too spine-shaped

The current straight-line builder rewrite handles important emitter-like code,
but it still depends on a narrow top-level spine pattern.

### Consequence

It misses:

- more control-flow-shaped append regions
- helper patterns beyond the tiny-wrapper subset
- likely some of the heaviest parser/emitter cases reported by `cow_analysis`

## 3. Tiny-wrapper summaries are useful but intentionally narrow

Current summary support handles tiny, direct, local wrappers well.

It does not yet aim to cover:

- arbitrary parameter reordering
- wrappers with internal control flow
- wrappers with harmless bookkeeping beyond simple forwarding binds
- general interprocedural no-retain reasoning

## 4. Multi-base ops still have no dedicated strategy

`VECTOR_CONCAT` remains a major source of residual COW, and it needs more than
a one-base consume-reassign proof.

## Proposed Remaining Rollout

## Phase R1: Shared operation semantics table ✅

Replaced the scattered `CowOpInfo`/`cow_op_info()`/`is_no_retain_read_only()`/
`InPlaceSwapInfo`/`in_place_swap_info()` with a unified `OpSemantics` struct
and `op_semantics()` lookup.

Each known intrinsic is now classified once with:

- `base_arg: Option<usize>` — which arg is the COW-updated collection
- `in_place_id: Option<FuncId>` — direct callee-swap in-place variant
- `fresh_if_copied: bool` — COW copy produces fresh result (Phase B)
- `no_retain_read_only: bool` — op never retains/aliases args
- `fresh_producer: bool` — op always produces a fresh unique value
- `builder_rewritable: bool` — participates in vector builder lifecycle

A derived `CowOpInfo` view (via `as_cow_info()`) gives call sites a guaranteed
`usize` base_arg for COW ops. All analysis paths — taint pre-scan, point
rewrites, loop analyses, wrapper summaries, straight-line builder rewrite —
now query this single table.

Pure refactor: cow_analysis numbers unchanged (723 total COW remaining).

## Phase R2: Broader vector builder recognition via known-empty relaxation ✅

Relaxed the taint guard for both loop and straight-line vector builder rewrites.

Previously, only `unique && !tainted` locals were candidates for builder
rewriting. Now, `unique && known_empty` locals also qualify even if tainted.
This is sound because:

- `known_empty` proves the local was just allocated (e.g. `xs: Vector<T> = []`)
- any taint is from a future escape on the spine (stored in record, returned)
- `builder_new()` is used (not `builder_from`), so no shared alias is mutated
- after `builder_freeze`, the local is reassigned to the frozen result
- subsequent code (including the escape) sees the frozen vector

Critical safety fix: after processing a loop in `rewrite_op`, the pass now
invalidates `known_empty` for any locals assigned inside the loop body.
Without this, a local that was `[]` at creation but modified by a preceding
loop could be incorrectly treated as empty by a later builder rewrite.

Measured impact on boot compiler:

- total remaining COW: 723 → 569 (−154, −21.3%)
- VECTOR_APPEND: 391 → 237 (−154)
- `lex`: 29 → 5 COW
- `unescape_string`, `rewrite_calls_kind`, `parse_block`, `monomorphize`,
  `lower_expr`, `parse_import_items` all dropped out of top-30 hotspots

## Phase R3: Dedicated `VECTOR_CONCAT` plan 🚧

Partial progress is now implemented via a gated builder-based path:

- new internal helper: `VECTOR_BUILDER_EXTEND(builder, vec)`
- straight-line concat chains now rewrite to builder setup + extend + freeze
- single straight-line concat consume-reassign sites on eligible bases now
  also rewrite to builder setup + extend + freeze
- builder-region eligibility now also accepts refreshed unique bases, not only
  plain untainted bases or tainted-known-empty ones
- mixed straight-line append+concat regions now rewrite through one builder
  region (`push` + `extend` + `freeze`)
- loop concat consume-reassign patterns now rewrite conservatively to builder
  setup + extend + freeze
- mixed loop append+concat accumulator regions now also rewrite through one
  builder region (`push` + `extend` + `freeze`)
- dead-base concat sites (`ys := xs.concat(rhs)` where `xs` dies immediately)
  now also rewrite to builder setup + extend + freeze
- taint pre-scan now treats the left concat base more precisely: it is only
  conservatively tainted when it remains live after the concat call, which
  unlocks the dead-base path without changing the existing `vector_set_after_concat`
  negative behavior
- self-concat is still rejected (`xs = xs.concat(xs)`) to avoid alias hazards
- concat is still **not** treated as a general one-base COW op in the shared
  semantics table; keeping it local to the rewrite avoids regressing existing
  negative cases like `vector_set_after_concat`

Current staged rule:

- left base is unique
- or left base is tainted-but-known-empty, so `builder_new()` is used
- right side does not syntactically alias the left base
- body shape is either consume-reassign (`tmp = concat(base, rhs); base = tmp`)
  or dead-base (`tmp = concat(base, rhs)` with no later use of `base`)
- accumulator reads remain disallowed inside the rewritten region

Measured impact so far:

- `VECTOR_APPEND`: `237 -> 193`
- `VECTOR_CONCAT`: `99 -> 26`
- total remaining COW: `562 -> 445`

Additional R3 extensions landed in sessions after the initial plan:

1. **Concat-result freshness propagation**: After `ys = xs.concat(rhs)`, the
   result local is marked unique/fresh so downstream concat ops can use
   `builder_from` on it. This unlocked many parser-heavy concat chains.

2. **Early-return-aware if-join**: When one branch of an if always terminates
   (return/break/continue), the continuation is only reachable via the other
   branch. The join now uses the non-terminating branch's state directly
   instead of intersecting, preserving `known_empty` and `builder_safe` facts
   through early-return guard patterns. This unblocked all the parser functions
   with `if !valid { append error; return }` guards before the first concat.

3. **`source_fresh` tracking for non-empty array literals**: A new
   `source_fresh` set tracks locally-allocated but non-empty array literals
   moved to tainted locals via init. This acts as an additional bypass in
   `base_can_start_builder_region` (alongside `known_empty` and
   `builder_safe`), enabling loop/spine builder rewrites for initializers like
   `lines := ["(module"]` that grow across multiple loops. The bypass is
   intentionally NOT wired into `can_preserve_builder_uniqueness` in the
   regular COW single-step path to prevent unsafe in-place rewrites after
   opaque calls.

4. **consume-reassign in `detect_dead_concat_base`**: The dead-base concat
   detector now allows `xs = xs.concat(rhs)` (consume-reassign) patterns in
   addition to the original `ys := xs.concat(rhs)` (dead-base) case.

What remains for full R3:

- single conditional appends in branches (e.g., `if ref_func_syms.len() > 0
  { lines = lines.append(...) }`) — only worth addressing if a multi-step
  builder threshold lowering is accepted
- early-return-inside-loop patterns where the loop body has return statements
  that reference the accumulator (e.g., `parse_prefix` StringStart)
- concat of two shared/parameter-derived vectors (fundamentally COW)

## Phase R4: Broader helper summaries

After the tiny-wrapper path, consider a slightly richer summary form.

Possible additions:

- direct parameter reordering
- a small number of harmless read-only or bookkeeping operations around the
  update
- explicit summary bits for retained/captured/stored/unknown-call behavior

This is still meant to stay local and cheap, not become full interprocedural
analysis.

## Phase R5: Branchy env/registry precision cleanup

Use the current hotspot list to target functions like:

- `register_layout_type_def`
- `register_type_entry`
- `link`
- `scan_tainted_op`

Goal:

- make record/dict/vector update reasoning more uniform in mixed branchy code
- convert another measurable slice of boot-compiler COW traffic

## Testing Strategy

For each new precision gain, keep the same four guardrails:

- structural ANF checks
- interpreter runtime checks
- Wasm runtime checks
- differential opt vs no-opt correctness

Also keep the characterization fixtures as explicit before/after evidence.

For future work, prefer adding fixtures that mirror boot-compiler hotspot shapes
rather than only minimal synthetic examples.

## Stopping Rule

This plan is successful if it:

- preserves the static-only runtime model
- removes the biggest accidental COW hotspots in real Twinkle code
- keeps the optimizer understandable and local
- improves measured boot-compiler COW counts, not just isolated fixtures

It is acceptable to stop before solving fully general multi-base alias problems
or full interprocedural alias reasoning.
