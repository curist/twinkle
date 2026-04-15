# Boot Compiler Performance Investigation

## How to run

```bash
# 1. Build the boot compiler with stage0
./target/release/twk build boot/main.tw -o /tmp/boot.wasm

# 2. Run with timings enabled (compiles boot/main.tw with the compiled wasm)
TWINKLE_TIMINGS=1 node tools/run_wasm_node.mjs /tmp/boot.wasm -- build boot/main.tw -o /tmp/stage2.wasm 2>&1 | grep '^\[time'
```

## Historical baseline (boot/main.tw, 84 modules, Node/Wasm)

```
compile_modules   3735ms   (84 modules — parse/resolve/check/lower per file)
optimize          3383ms   (fixed-point opt pipeline over all functions)
verify            3086ms   (backend IR verifier over all PreparedFuncs)
plan_wasm_types   1397ms
emit_wasm_binary   743ms
emit_module        616ms
link               612ms
prepare_backend    327ms
lower_anf           49ms
core_link           63ms
monomorphize        21ms
closure_convert     11ms
─────────────────────────
total            ~14 000ms
```

This was the original investigation baseline. Several items below have since
been fixed, so use the current snapshot for prioritization.

## Snapshot (after wasm.tw accumulator refactor, 2026-04-13)

```
compile_modules   3558ms
optimize           512ms  (uniqueness=78ms, dead_let=218ms, copy_prop=181ms)
emit_module        601ms
link               671ms
emit_wasm_binary   453ms
prepare_backend    351ms
verify             188ms
plan_wasm_types    158ms
lower_anf           66ms
core_link           68ms
monomorphize        24ms
closure_convert     12ms
```

Changes since previous snapshot:
- Skipped merge_mono_maps in with_metadata_from when next_local unchanged: uniqueness
  1629ms → 85ms (~95% reduction); only loop-rewrite functions pay the merge cost
- Added instantiate() fast path for non-generic functions (the common case in all
  large modules): skips empty var_map allocation and params vector copy
  compile_modules: 3815ms → 3615ms (~5% reduction)
- Refactored wasm.tw encode_instr/encode_instrs to accumulator pattern (buf-first):
  eliminates one temporary Vector<Byte> per instruction and per control-flow nesting
  level; section encoders updated to call encode_instrs(buf, ...) directly instead
  of emit_bytes(buf, encode_instrs(...))
  emit_wasm_binary: 802ms → 453ms (~44% reduction)

Current priority order:

1. `compile_modules` (3558ms) — type-checker dominates large modules
2. `emit_module` + `link` (~1272ms combined) — next codegen targets
3. `optimize` (512ms) — no longer a top bottleneck

`verify`, `plan_wasm_types`, and `optimize` are no longer top-tier bottlenecks.

---

## Snapshot (2026-04-15, after type-encoding cleanup)

```
compile_modules   3465ms
optimize           487ms  (uniqueness=77ms, dead_let=207ms, copy_prop=174ms)
emit_module        583ms
link               628ms
emit_wasm_binary   401ms
prepare_backend    329ms
verify             169ms
plan_wasm_types    143ms
lower_anf           50ms
core_link           64ms
monomorphize        22ms
closure_convert     11ms
```

Changes since previous snapshot:
- `sort_by` in prelude/vector.tw replaced with merge sort (was insertion sort, O(n²))
- wasm.tw: added `_into` accumulator variants for type section encoding
  (`encode_storage_type_into`, `encode_field_type_into`, `encode_func_comptype_into`,
  `encode_comptype_into`, `encode_subtype_into`); all type section callers updated
- wasm.tw: `compress_locals` now uses structural `val_type_eq` instead of string keys
- wasm.tw: `collect_ref_func_syms` called once in `emit_wasm_parts`, result passed into
  `encode_elem_section_payload_with_refs` (eliminates second full instruction traversal)
- Sub-timing added to `emit_wasm_binary` for section breakdown

Net effect on `emit_wasm_binary`: 453ms → 401ms (~11% reduction). The type-section
cleanup paths were cold; most remaining time is in `ctx_build` and `code_section`.

### emit_wasm_binary sub-timing breakdown

```
type_order (Tarjan SCC, ~408 types)   13ms
build_ctx                              80ms   ← second target
type_section                            6ms
small_sections (import/func/table/
  global/export/start/elem)            24ms
code_section (1354 functions)         274ms   ← primary target
```

### Root cause: Dict is an O(n) unsorted association list

`rt.dict` was implemented as a linear-scan association list. Every `dict_set`,
`dict_get`, `dict_has` scanned all entries calling `core_eq` per entry. This
dominated every hot path in the compiler: type-env lookups in the checker,
symbol tables in the resolver, `WasmCtx` lookups during codegen and linking.

**Fix (2026-04-15): replaced with persistent HAMT + insertion-order PVec.**
See commit "Replace O(n) assoc-list Dict with persistent HAMT + insertion-order PVec".

---

## Current snapshot (2026-04-15, after HAMT Dict)

```
compile_modules    442ms   (was 3465ms — 87% reduction)
optimize           150ms   (was 487ms  — 69% reduction)
emit_module        528ms
prepare_backend    221ms   (was 329ms  — 33% reduction)
emit_wasm_binary   200ms   (was 401ms  — 50% reduction)
  type_order         6ms
  build_ctx          6ms
  type_section       3ms
  small_sections    13ms
  code_section      170ms
link                54ms   (was 628ms  — 91% reduction)
verify              98ms   (was 169ms  — 42% reduction)
plan_wasm_types     57ms   (was 143ms  — 60% reduction)
lower_anf           31ms
core_link           27ms
monomorphize        21ms
closure_convert     10ms
─────────────────────────
total             ~1840ms  (was ~7600ms — 76% reduction)
```

Wall-clock comparison (boot/main.tw, 84 modules):
- stage0 Rust:  1.19s
- stage1 Wasm:  1.98s  (~1.66× — within 2× of native)

The HAMT replaced the bottleneck in essentially every phase simultaneously:
checker env lookups, resolver symbol tables, wasm ctx func/type index lookups,
link symbol resolution. The compound effect drove a 76% overall reduction.

### Remaining hot spots

`emit_module` (528ms) and `code_section` (170ms) are the new top targets.
`compile_modules` (442ms) is healthy — the large modules (checker.tw at 47ms,
lower_core.tw at 41ms, emit.tw at 31ms) are now proportional to their size.

`optimize` (150ms) breakdown: dead_let=43ms, copy_prop=37ms, uniqueness=43ms.
No longer a priority.

### Potential further improvements

**RRB-Trees for PVec** — current PVec concat is O(m·log n); RRB-Trees
(Bagwell & Rompf 2011) allow O(log n) concat/slice via relaxed internal nodes
with a size table. Would mainly benefit emit.tw's instruction-vector building.

**CHAMP for HAMT** — Steindorfer 2015 improvement: two bitmaps per node
(data vs sub-trie) to inline key-value pairs directly in the node array,
eliminating the separate HamtEntry struct allocation per entry. Fewer
allocations, better iteration locality. Scala 2.13's HashMap uses CHAMP.

---

## Findings (post fine-grained instrumentation)

```
[time:mod] boot/compiler/checker.tw    total=701ms  check=411ms  lower=61ms
[time:mod] boot/compiler/lower_core.tw total=468ms  check=329ms  lower=57ms
[time:mod] boot/compiler/emit.tw       total=637ms  check=518ms  lower=76ms
[time:mod] boot/compiler/parser.tw     total=409ms  check=308ms
[time:mod] boot/compiler/pipeline.tw   total=3167ms deps=3158ms  (almost all dep resolution time)
[time:mod] boot/compiler/codegen.tw    total=1485ms deps=1475ms  (same)

[time:opt] funcs=1354  total_rounds=3063  avg_rounds=2.26  at_cap=13
[time:opt] dead_let=202ms  copy_prop=167ms  const_fold=4ms  branch_simp=3ms
[time:opt] annotate_in_place=0.9ms  uniqueness=3001ms  defer_elim=3ms
```

These numbers were captured before the later ownership-model cleanup that:

- removed `clone_env` from module compilation
- deleted the resolver clone helpers
- moved record-update reuse decisions under `uniqueness.tw`
- removed `annotate_in_place` from the production optimization pipeline

So the detailed timings below remain useful for historical root-cause analysis,
but some hypotheses are now resolved and should not be treated as current work.

```
[time:verify] funcs=1354  total_slots=62185  slot_entries=62185
[time:verify] slot_checks=2971ms  expr_walk=95ms

[time:plan] funcs=1354  slot_reg_calls=63539  unique_types=402  strings=1484
[time:plan] scan=45ms  register_slots=58ms  register_sigs=1362ms  topo_sort=4ms
```

Key findings:
- **compile_modules** still concentrates in a handful of large modules, and their cost is still mostly `check`: checker.tw (~454ms), emit.tw (~576ms), parser.tw (~347ms), lower_core.tw (~333ms).
- **optimize** is still dominated by `uniqueness` (~2946ms of ~3401ms). dead_let+copy_prop are secondary (~418ms combined). avg rounds remains low (~2.26), so the fixed-point loop is still not the problem.
- **verify** is no longer a major issue (~199ms total after the earlier slot-check fix).
- **plan_wasm_types** is no longer a major issue (~164ms total after the earlier signature-registration fix).

---

## 1. `compile_modules` — 3735ms (84 modules)

Average ~44ms per module.  Each module runs:
parse → resolve → check → lower_core

### Hypotheses

**H1.1 — Per-module env cloning is expensive.**
This was a plausible hypothesis when `compile_module` still called
`clone_env(base_env)` before every `extend_types_from`.

**Status:** resolved structurally. The clone workaround was removed after the
boot optimizer was fixed to distinguish shallow wrapper freshness from deep
ownership, so this is no longer a current optimization target.

**H1.2 — Prelude modules re-compiled per entry point.**
The 40+ prelude files are loaded and compiled as dependencies for every user
module.  If prelude modules are individually cached but their
parse/resolve/check results are not, each entry point re-does that work.

**H1.3 — Type checker cost grows super-linearly.**
HM unification with MetaVar is amortized O(n) in theory, but the checker's
env lookup (Dict<String, _>) may hash large strings many times, or the
constraint solving may traverse deep type trees repeatedly.

**H1.4 — Hot modules are disproportionately expensive.**
A few large modules (resolver.tw, checker.tw, emit.tw) may dominate.  We
cannot tell yet because `compile_modules` is a single aggregate timing.

### Findings

Per-module sub-step timing shows `check` dominates in the large modules:
checker.tw (411ms), emit.tw (518ms), lower_core.tw (329ms), parser.tw (308ms).
The `deps` column for top-level files like pipeline.tw is just recursively
accumulated cost from those modules, not new work.

All four large modules have ZERO generic (parameterized) functions. This means
every `instantiate()` call in the type checker was allocating an empty
`var_map: Dict<String, MonoType>` and copying all params through `subst_vars`
with no substitutions to apply — pure waste.

Fix: added fast path in `instantiate()` for `sig.type_params.len() == 0`:
return `sig.params` directly and unwrap `sig.ret`, skipping the Dict alloc and
`collect p in params { subst_vars(p, {}) }` traversal.
Result: compile_modules 3815ms → 3615ms (~200ms, ~5% reduction).

The checker's check time is still O(N) in total expressions with a moderate
constant factor from InferCtx struct allocations and growing type_map/expr_spans
dicts. No O(n²) pattern was found — the bottleneck is proportional work.

### Next investigation targets for compile_modules

- `check` in checker.tw (411ms), emit.tw (519ms), lower_core.tw (302ms),
  parser.tw (340ms) still dominate. The checker threads InferCtx (12 fields)
  functionally through every expression, creating struct copies on each update.
  Cell-wrapping the write-only accumulators (type_map, expr_spans, method_calls)
  would eliminate ~2 InferCtx allocations per expression — potentially a 20-40%
  reduction in check time.
- `lower_core.tw` and `parser.tw` also have non-trivial `lower` times (49ms,
  54ms) — might be worth looking at once check time is further reduced.

---

## 2. `optimize` — 3383ms

`optimize_module` runs a fixed-point loop (cap 10 rounds) of four passes over
every function, then three post-loop passes.

### Hypotheses

**H2.1 — Many functions run close to the 10-round cap.**
If most functions need 8–10 rounds to converge, the total work is ~10×
the single-pass cost.  The loop exits early only when all four passes
report `changed = false`.

**H2.2 — `collect_free_locals` / `collect_bound_locals` are called on every round.**
`compute_pinned` runs once per module, but each round of `copy_prop` and
`dead_let_elim` may do their own internal AST traversals that are O(AST size).
For large functions (e.g. the boot compiler's own codegen helpers), this
could be the dominant term.

**H2.3 — `uniqueness_rewrite` is expensive on large functions.**
The uniqueness pass (Phase 1–2 complete) does a pre-scan + forward walk with
a `HashSet` of unique locals.  On the fully monomorphized module it sees
all specialised copies, so the total work scales with the number of
monomorphic function variants.

**H2.4 — `annotate_in_place` runs on every function unconditionally.**
Even functions with no record-update ops pay the traversal cost.

**Status:** resolved structurally. Record-update reuse decisions now live under
`uniqueness.tw`, and `annotate_in_place` is no longer in the production
pipeline.

### Findings

`uniqueness_rewrite` is 3001ms out of 3383ms total — it alone IS the
optimize bottleneck.  The fixed-point loop is cheap: avg 2.26 rounds,
only 13 functions hit the cap.  dead_let+copy_prop together are 370ms,
a secondary target.

### Investigation

Recent wins:

- Fused collect_tainted + live_after_by_binding into single backward pass
  (collect_tainted_and_live in analysis.tw) — eliminates one O(n) traversal
  per function for functions with COW ops
- Added taint-based second pre-check (has_rewritable_cow_op_in_expr): after
  taint analysis, if all COW-op base locals are tainted, skip rewrite_expr
  entirely. This is the dominant win (~44% uniqueness reduction) because many
  functions in large modules (checker.tw, emit.tw) operate on dict/vector
  parameters that are always tainted, so rewrite_expr was wasting time finding
  no rewrites.

Current state: uniqueness=85ms. No longer a priority.

Additional win (after uniqueness reduction):
- Skip merge_mono_maps in with_metadata_from when next_local is unchanged:
  loop rewrites are the only path that adds new monos entries, and they always
  advance next_local by 3. When next_local is equal on both sides the two maps
  are identical so the O(|monos|) merge can be skipped.
  uniqueness: 1629ms → 85ms (~95% reduction)

dead_let+copy_prop at ~200ms each are now secondary targets, but optimize
(534ms total) is no longer a top priority.

---

## 3. `verify` — historical 3086ms, current ~199ms

`verify_prepared_module` walks every `PreparedFunc`'s full body expression
tree, checking slot membership, metadata, and expression shapes.

### Hypotheses

**H3.1 — Verify does redundant work that prepare_backend already enforces.**
`prepare_backend` constructs the PreparedModule.  If its invariants are strong
enough, many verify checks are guaranteed-true by construction and pay the
traversal cost for no diagnostic benefit in a correct build.

**H3.2 — `slot_info` / `verify_slot_membership` is O(slots) per call.**
If `pf.slots` is a `Dict<Int, SlotInfo>` these lookups should be O(1), but
if the dict is implemented as a sorted vector or if key hashing is slow for
integer keys, it could be O(n) or have high constant factor.

**H3.3 — `verify_unique_source_local` builds a reverse map on every call.**
If the uniqueness check scans `pf.slots` linearly for each slot to prove
no two slots share the same source local, that is O(slots²) per function.

**H3.4 — Verify is O(total IR nodes), which is large after monomorphization.**
After mono, each generic function is expanded into N copies.  Verify sees all
of them.  The total IR node count may be large enough that even a fast linear
pass takes seconds.

### Findings

The split is decisive: slot_checks=2971ms vs expr_walk=95ms.
With 62185 slots across 1354 functions (avg 45 slots/func), and
slot_entries=62185 (1:1 ratio of slots to entries), the cost is inside
the slot-check loop, not the expression walk.

`verify_unique_source_local` is the prime suspect: it likely scans all
slot entries to prove no two slots share the same source local, giving
O(slots²) per function.  For avg 45 slots, that is ~2000 comparisons
per function × 1354 functions = ~2.7M comparisons.

### Investigation

- Confirm `verify_unique_source_local` is O(slots²) by reading its body.
- Fix: build a `seen_source_locals: Dict<Int, Bool>` once per function
  (O(slots)) instead of re-scanning for each slot.
- Secondary: consider making verify opt-in behind `TWINKLE_VERIFY=1` for
  release builds.

---

## 4. `plan_wasm_types` — historical 1397ms, current ~164ms

`plan_wasm_types` scans all prepared bodies and builds the `WasmTypeRegistry`:
registers mono types, string pool entries, runtime imports, closure func sigs.

### Hypotheses

**H4.1 — `register_mono` is called for every slot in every function.**
For a large module with many monomorphic copies, this means O(functions ×
avg_slots) calls.  Each `register_mono` recursively walks the `MonoType` tree
and inserts into `WasmTypeRegistry` dicts.

**H4.2 — `register_runtime_import` scans `runtime_imports` linearly.**
The deduplication check iterates the whole imports vector on every call.
If there are many distinct builtin call sites this is O(builtins²).
(Already uses `as_sym` string comparison — likely not the dominant cost but
worth confirming.)

**H4.3 — `topo_sort_type_defs` is O(n²).**
The topo sort in `wasm_plan_impl.tw` may use a naive dependency traversal
that is quadratic in the number of struct types registered.

**H4.4 — String interning in `register_string` hashes large WAT strings.**
String pool entries include a getter symbol name built by interpolation, which
is hashed on every `dict.set` call.

### Findings

`register_sigs` takes 1362ms of 1471ms total (93%).  This covers
`register_func_sig_by_id` for closure/ho-global funcs and
`register_higher_order_global_func_sigs` + `analyze_builtin_call`.
Slot registration (63539 calls) takes only 58ms — MonoType registration
itself is cheap.  topo_sort is 4ms (not a problem).

### Investigation

- Add sub-timers inside register_sigs to split: closure_funcs registration
  vs ho_global_funcs vs `register_higher_order_global_func_sigs` vs
  `analyze_builtin_call`.
- `register_higher_order_global_func_sigs` likely iterates all functions
  looking for higher-order usages — check if it is O(funcs²) or uses a Dict.
- `analyze_builtin_call` calls `register_runtime_import` which has a linear
  dedup scan; with many distinct builtins this could be O(builtins²).

---

## 5. Remaining steps (< 750ms each)

`emit_wasm_binary` (743ms), `emit_module` (616ms), `link` (612ms) are
significant but secondary.  Investigation can wait until the top four are
addressed.  Quick notes:

- **emit_wasm_binary**: encodes the full linked Wasm binary.  Cost is
  proportional to module size (instruction count × LEB128 encoding).  Likely
  hard to improve without shrinking the module first.
- **emit_module**: emits WAT instructions from PreparedIR.  One pass over all
  functions; cost follows IR size.
- **link**: DCE + renumber + merge.  May have O(n²) in symbol resolution if
  the import/export tables are scanned linearly per reference.

---

## Suggested investigation order

1. Re-run per-module + per-sub-step timings on the current tree to measure the
   post-`clone_env` baseline and confirm whether type checking still dominates
   `compile_modules`.
2. Re-run per-pass timings on `optimize_module` after removing pipeline
   `annotate_in_place`, then profile `uniqueness_rewrite` under the ownership
   model rather than the older unique-bit implementation.
3. Add slot/node count logging to `verify_prepared_module` — determines whether
   3s is unavoidable given IR size or whether specific checks are pathological.
4. Add type-registration count and topo-sort timer to `plan_wasm_types`.

All four can be added behind `TWINKLE_TIMINGS=1` incrementally without
changing semantics.  Once the root cause per step is confirmed, a separate
fix plan can be drafted for each.
