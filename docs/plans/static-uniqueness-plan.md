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

- [archive/deferred-persistence.md](archive/deferred-persistence.md)
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

The main remaining issues are now narrower than when this plan started:

- some logic still depends on ANF shape more than semantics
- vector and dict rewrites still use different permissiveness rules
- straight-line and loop append rewrites are still narrow relative to real code
- helper support is good for tiny wrappers, but not yet for fresh-wrapper
  destructuring or richer bookkeeping-shaped helpers
- branchy env/registry flows still lose precision more quickly than their
  straight-line equivalents

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

### Fresh-wrapper / field-borrow coverage

- `tests/opt/field_borrow_dict.tw`
- `tests/opt/fresh_wrapper_destructure_dict.tw`
- `tests/opt/fresh_wrapper_destructure_reread_not_rewritten.tw`

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
  accumulator patterns from that earlier snapshot
- at that point `VECTOR_CONCAT` was still the next global blocker

### Current snapshot (2026-04-21)

Current `cow_analysis` on `boot/tests/main.tw` now reports:

| Operation | Pre-opt | Post-opt | Rewritten |
|---|---:|---:|---|
| VECTOR_APPEND | 480 | 210 | 270 → builder |
| VECTOR_CONCAT | 109 | 25 | 84 → builder-extend path |
| DICT_SET | 391 | 134 | 257 → DICT_SET_IN_PLACE |
| DICT_REMOVE | 22 | 22 | none |
| REC_UPDATE | 109 | 74 COW | 35 → REC_UPDATE_IN_PLACE |
| VECTOR_SET (safe) | 7 | 7 | none |
| VECTOR_SET_IN_PLACE | — | 14 | lowered unsafe-set rewrites |

**Total COW remaining: 472**

This means the optimizer still removes well over half of the original COW
traffic in this workload. The biggest structural shift relative to the earlier
versions of this plan is that `VECTOR_CONCAT` is no longer the dominant unsolved
family; the remaining work is now mostly a mix of:

- residual vector append regions with awkward control flow
- dict/record threading through branchy env/registry code
- a smaller set of branchy helper-shaped context-update patterns

## Current Remaining Hotspots

Recent `cow_analysis` output points to three main buckets.

### 1. Dict + record threaded context updates

The heaviest remaining functions are now mostly env/registry/context code rather
than pure vector construction:

- `lower_module`
- `register_type_entry`
- `extend_types_from`
- `plan_wasm_types_impl`
- `rewrite_index`
- `scan_op` / `scan_atom`
- `collect_local_types`
- `transfer_owned_local`

These functions all mix record shell updates with dict-field updates. The common
failure mode is no longer "the collection is obviously shared"; it is that the
fresh current state is transported through wrappers, branches, or helper-shaped
control flow that the optimizer still treats conservatively.

### 2. Residual vector-append regions with awkward control flow

`VECTOR_APPEND` still has the largest raw remaining count, but the remaining
sites are narrower than the original emitter/parser bottleneck. Current examples
include:

- `parse_prefix`
- `synth_assign_op`
- `emit_instr`
- `parse_type_expr_base`
- `parse_use`
- `parse_function` / `parse_type`
- `lex`
- `unify`

These are mostly builder-eligible in spirit, but they sit behind branchy spines,
helper boundaries, or loop/early-return shapes that the current builder-region
matcher intentionally does not overfit.

### 3. Concat stragglers, not concat as a blocker

`VECTOR_CONCAT` is now down to a small residual set instead of being the main
architectural blocker. The remaining sites are mostly:

- shared-operand concat where copying is fundamentally required
- control-flow-heavy parser cases such as `parse_prefix`
- shapes that would require more aggressive builder-region extraction than is
  justified by the remaining payoff

## Remaining Problems

## 1. Branchy env/registry flows still destroy too much precision

The remaining dict/record hotspots are often not straight-line update chains.
They thread state through:

- conditional branches
- early exits
- helper calls that are semantically transparent but not syntactically tiny
- alternating record-update and dict-update steps

This is less about adding new ownership concepts and more about keeping the
existing facts alive in realistic control-flow shapes.

## 2. Vector builder rewriting should stay conservative

There are still vector append cases that look optimizable, but the plan should
not regress into the earlier unsoundness where logical-accumulator reads inside
rewritten regions observed the wrong semantics.

So the remaining vector work should stay narrow:

- preserve the explicit negative cases
- prefer targeted builder-region widening over broad "read-only is probably fine"
  relaxations
- stop once the residual sites require non-local alias reasoning

## 3. Some low-volume families are not worth turning into major phases

At current counts, these are real but not roadmap-defining:

- `DICT_REMOVE`
- safe `VECTOR_SET`
- fully general helper summaries
- general interprocedural no-retain / alias analysis

They should be handled only if they fall out naturally from focused work on the
higher-value hotspots above.

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

## Phase R3: Dedicated `VECTOR_CONCAT` plan ✅

Implemented via a gated builder-based path:

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

Residual cases after R3:

- single conditional appends in branches (e.g., `if ref_func_syms.len() > 0
  { lines = lines.append(...) }`) — only worth addressing if a multi-step
  builder threshold lowering is accepted
- early-return-inside-loop patterns where the loop body has return statements
  that reference the accumulator (e.g., `parse_prefix` StringStart)
- concat of two shared/parameter-derived vectors (fundamentally COW)

At this point R3 should be considered complete enough for this plan. The
remaining concat cases are either deliberate conservatism or genuinely shared
operands where a copy is the right behavior.

## Phase R4: Broader helper summaries

After the tiny-wrapper path, consider a slightly richer summary form.

Possible additions:

- direct parameter reordering
- a small number of harmless read-only or bookkeeping operations around the
  update
- explicit summary bits for retained/captured/stored/unknown-call behavior

This is still meant to stay local and cheap, not become full interprocedural
analysis.

## Phase R4b: Field-borrow optimization ✅

Implemented `is_field_borrow_and_update` helper and hooked it into the
`ARecordGet` handler.

When a struct's field is extracted but the struct base is still live (so the
simple "base dies immediately" path doesn't apply), the pass now checks whether
the extracted field is consumed by a COW op (DICT_SET, VECTOR_SET_UNSAFE) whose
result is then stored back via ARecordUpdate. The matcher now tolerates small
runs of transparent bookkeeping lets between those steps, as long as they do
not use or capture the borrowed field. In that shape the field has no other
live references at the COW site, so it is treated as unique and the COW op can
use its in-place variant.

Pattern that fires:
```tw
fn multi_update(ctx: Ctx, k1: String, k2: String, k3: String, v: Int) Ctx {
  ctx := make_ctx()
  ctx.table[k1] = v      // first: ctx not unique → COW, ctx becomes unique
  ctx.cache[k2] = v      // second: ctx unique + field-borrow → DICT_SET_IN_PLACE
  ctx.extra[k3] = v      // third: ctx unique + field-borrow → DICT_SET_IN_PLACE
  ctx
}
```

Impact on boot compiler: 2 additional DICT_SET → DICT_SET_IN_PLACE rewrites
(for functions with 2+ sequential compound dict updates where the base is
already unique/refreshed after the first update).

Why coverage is limited to 2 fires: most compound dict updates in hot functions
are either (a) the first update in a branch where the base is a param (not yet
unique), (b) each branch has at most 1 compound update, or (c) the base was
reset from an opaque function call (not unique). The `VECTOR_APPEND` path also
cannot benefit since VECTOR_APPEND has no direct in-place variant.

New test fixtures:
- `tests/opt/field_borrow_dict.tw`

## Phase R4c: Fresh-wrapper destructure summaries ✅

Recent hotspot inspection shows a recurring shape in boot compiler code such as:

- `boot/compiler/lower_core.tw::alloc_func_id`
- `boot/compiler/codegen/insert_boundaries.tw::alloc_local`

These helpers return a freshly-allocated record wrapper like:

```tw
fn alloc_func_id(ctx: LowerCtx) FuncIdOut {
  id := FuncId.{ id: ctx.next_func }
  ctx.next_func = ctx.next_func + 1
  FuncIdOut.{ func_id: id, ctx }
}
```

and callers then immediately unpack it:

```tw
r := ctx.alloc_func_id()
ctx = r.ctx
ctx.func_table[sig.name] = r.func_id
```

This is a real contributor to remaining boot-compiler COW. Current
`cow_analysis` shows the main examples as:

- `lower_module` — `DICT_SET=8`, `REC_UPDATE_COW=8`
- `rewrite_call` — `DICT_SET=4`, `REC_UPDATE_COW=4`
- `rewrite_index` — `DICT_SET=3`, `REC_UPDATE_COW=3`

### Implemented approach

The optimizer now records a `returns_fresh_record` summary bit for tiny helper
functions that tail-return a freshly-built record. Examples include:

- `boot/compiler/lower_core.tw::alloc_func_id`
- `boot/compiler/codegen/insert_boundaries.tw::alloc_local`

Calls to such helpers are marked fresh at the call site. Then, in the
`ARecordGet` handler, a narrow destructuring rule transfers deep uniqueness only
when:

1. the base is still `source_fresh`
2. the remaining uses of the wrapper are limited to disjoint non-escaping
   `ARecordGet`s
3. the moved field is never re-read from the wrapper

After that first deep field move, `source_fresh` is removed from the wrapper so
no second field can gain the same treatment accidentally.

This is deliberately narrower than a generic live-base `ARecordGet` ownership
rule, but it is enough for the `{ ctx, local }` / `{ ctx, func_id }` transport
pattern that shows up throughout the boot compiler.

### Why this narrower rule was needed

### What is **not** valid as a general rule

A broad rule of the form:

- call result is `unique + source_fresh`
- `ARecordGet(base, field)` from that fresh record may transfer deep uniqueness
  to the field even when `base` remains live

is too aggressive.

Reason: once the base record stays live, it is still an alias path to the
extracted field. Without additional tracking, later uses of the base could:

- pass the wrapper to an opaque call
- store/return the wrapper
- re-read the same field

Any of those would make `DICT_SET_IN_PLACE` / `VECTOR_SET_IN_PLACE` on the
extracted field unsound.

So the earlier "just propagate `source_fresh` through live `ARecordGet` and
remove it from the base" idea is **not** a safe generalization of the current
analysis.

### Example unlocked by the implementation

```tw
r := ctx.alloc_func_id()
ctx = r.ctx                 // ctx becomes unique/refreshed via fresh-wrapper unpack
ctx.func_table[k] = v       // field-borrow can now fire
ctx.origin_cache[k2] = v
ctx.imported_func_origins[k3] = v
```

### Measured impact

On the stage0 `cow_analysis` workload that first motivated this follow-up, the
fresh-wrapper rule reduced remaining COW by:

- total COW: `513 -> 483`
- `DICT_SET`: `155 -> 136`
- `REC_UPDATE_COW`: `99 -> 88`
- `DICT_SET_IN_PLACE`: `239 -> 258`
- `REC_UPDATE_IN_PLACE`: `36 -> 47`

After mirroring the same idea into the boot optimizer, absolute head-of-tree
`cow_analysis` totals moved slightly because the boot optimizer implementation
itself adds more dict-heavy compiler code to the measured workload. The local
R4c effect above is still the meaningful before/after for this change.

Notable hotspot movement:

- `lower_module`: `17 -> 8` remaining COW
- `rewrite_index`: `6 -> 4`
- `rewrite_call` dropped out of the top-30 list

Characterization added:

- `tests/opt/fresh_wrapper_destructure_dict.tw`
- `tests/opt/fresh_wrapper_destructure_reread_not_rewritten.tw`

This makes R4c complete enough for the purposes of this plan. The same narrow
fresh-wrapper destructuring rule is now also mirrored in the boot optimizer,
with characterization coverage for both the positive case and the same-field
re-read negative case.

## Phase R5: Branchy env/registry precision cleanup 🚧

A first parity step has now landed in the boot optimizer as well:

- `AIf` joins use the non-terminating branch directly when the other branch
  always terminates
- `AMatch` joins ignore terminating arms and intersect only the reachable-arm
  post-state
- boot uniqueness state now carries `refreshed` / fresh-wrapper facts through
  those joins instead of dropping all branch-local precision

This is enough to cover focused regression shapes such as an early-return guard
that refreshes a tainted accumulator in the surviving branch before a later COW
update.

A first hotspot-oriented source cleanup also landed in boot compiler code:

- `register_layout_type_def` / `register_closure_func_type` now batch
  `type_defs` + `type_index` updates through one final registry reconstruction
  helper instead of repeatedly updating the registry record in place
- `scan_tainted_op` now shares small taint-atom helpers, reducing local
  bookkeeping noise in the analysis code

Measured effect of the latest R5 follow-up on head `cow_analysis`:

- total COW: `496 -> 472`
- `DICT_SET`: `140 -> 134`
- `REC_UPDATE_COW`: `88 -> 74`
- `register_layout_type_def` dropped out of the top-30 hotspot list
- `scan_tainted_op` also dropped out of the top-30 hotspot list
- follow-up batching in `register_type_entry`, `extend_types_from`, and
  `plan_wasm_types_impl` removed additional registry-shell churn even though
  those functions still remain on the hotspot list

Use the current hotspot list to target functions like:

- `register_layout_type_def`
- `register_type_entry`
- `link`
- `scan_tainted_op`

Goal:

- make record/dict/vector update reasoning more uniform in mixed branchy code
- convert another measurable slice of boot-compiler COW traffic

## Completion Assessment

The foundational work in this plan is now largely done:

- refresh-aware record/dict reuse is in place
- shared semantics-table refactoring is in place
- known-empty / builder-safe / source-fresh vector builder widening is in place
- concat rewriting is in place and has removed most concat traffic
- field-borrow for immediate update-back patterns is in place
- fresh-wrapper destructuring for `{ ctx, local }` / `{ ctx, func_id }` helper
  returns is in place

What remains is no longer a broad architectural gap. It is a short list of
bounded follow-ups, led by:

1. branchy env/registry precision cleanup in a handful of hotspot functions
2. conservative, case-driven widening of residual vector append regions
3. only-if-cheap cleanup of residual dict/record hotspots exposed by current
   `cow_analysis`

That means this document can now serve as a closeout plan rather than an open-
ended roadmap: future work should be tracked as focused hotspot follow-ups, not
as another large uniqueness redesign.

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
