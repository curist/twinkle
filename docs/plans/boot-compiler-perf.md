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

---

## Snapshot (2026-04-16, emit.tw accumulator refactor)

```
compile_modules    493ms
optimize           178ms
prepare_backend    205ms
emit_module        283ms   (was 528ms — 46% reduction)
verify             103ms
emit_wasm_binary   211ms
  code_section     169ms
plan_wasm_types     58ms
link                62ms
lower_anf           33ms
core_link           28ms
monomorphize        22ms
closure_convert     11ms
─────────────────────────
total             ~1690ms
```

Changes since previous snapshot:
- `sanitize_name` in emit.tw: replaced O(n²) `"${out}${b}"` string concat loop
  with `collect` + `join("")`
- `emit_let`: replaced tail-recursive Let-chain with iterative loop, eliminating
  O(N²·log N) `small.concat(large_tail)` pattern (each level appended small op
  result onto growing buf instead of concating large tail onto small op result)
- Full accumulator refactor of all `emit_*` functions in emit.tw: added
  `buf: Vector<Instr>` parameter; functions push directly into buf via `.append()`
  instead of returning small intermediate vectors and concating them. Eliminates
  O(m·log n) per-concat and reduces GC pressure from ephemeral vector allocations.
  `emit_module`: 528ms → 283ms (~46% reduction)

### Remaining hot spots

`compile_modules` (493ms) is now the dominant cost, followed by `emit_wasm_binary`
(211ms, mostly `code_section` at 169ms). `emit_module` is no longer a top target.

---

## Snapshot (2026-04-17)

```
compile_modules    442ms
optimize           190ms
prepare_backend    210ms
emit_module        273ms
verify             115ms
emit_wasm_binary   212ms
plan_wasm_types     38ms
link                53ms
lower_anf           31ms
core_link           26ms
monomorphize        23ms
closure_convert     11ms
─────────────────────────
total             ~1624ms
```

Changes since previous snapshot:
- No targeted changes; numbers reflect noise-level variation from run to run.
  `compile_modules` improved ~10% (493→442ms), `plan_wasm_types` -34% (58→38ms).

---

## Snapshot (2026-04-18, emit_module helper collection fixes)

```
compile_modules    442ms
optimize           194ms
prepare_backend    211ms
emit_module        216ms   (was 273ms — 21% reduction)
verify             116ms
emit_wasm_binary   198ms
plan_wasm_types     39ms
link                54ms
lower_anf           31ms
core_link           26ms
monomorphize        22ms
closure_convert     11ms
─────────────────────────
total             ~1560ms
```

Changes since previous snapshot:
- `collect_sum_variant_helpers`: added `is_sum: Dict<String, Bool>` cache — avoids
  calling `is_sum_mono` (which calls `layout_of` → O(|env.types|) linear scan in
  `find_type_entry`) more than once per unique MonoType. ~67k slot-level calls
  reduced to ~N_unique_types calls.
- `collect_iterator_next_helpers_expr`: converted recursive Let-chain traversal to
  iterative loop, eliminating O(N_let_bindings) recursive calls per function.
  (Same pattern as the `emit_let` iterative refactor.)
  `emit_module`: 273ms → 216ms (~21% reduction).

### Remaining hot spots

`compile_modules` (442ms) and `prepare_backend` (211ms) are the top two targets.
`emit_wasm_binary` (198ms) and `verify` (116ms) follow.

---

## Snapshot (2026-04-25, emission layout-cache reuse)

Observed shape during this follow-up:

```
compile_modules    ~619–664ms
optimize           ~238–250ms
prepare_backend    ~216–234ms
emit_module        ~185–188ms   (was ~243–250ms before this change)
emit_wasm_binary   ~238–250ms
verify             ~127–137ms
plan_wasm_types    ~41–46ms
link               ~59–77ms
```

Representative stable reruns after the change:

- `emit_module`: `185ms`, `185ms`, `188ms`
- `emit_func` body sub-timing: `137ms`, `136ms`, `140ms`

### Investigation notes

The initial suspicion was that match lowering itself was expensive because of the
recursive else-chain shape in `emit_arm_chain()`. Temporary sub-timing inside
`emit_module` showed the real cost was in function-body emission, not helper
collection or local-map setup. Deeper control-flow timing then showed:

- `match` dominated `emit_op`
- within `match`, most time was attributed to arm lowering rather than
  scrutinee setup
- within pattern emission, `emit_variant_pattern_condition()` spent most of its
  time in `get_sum_layout()` / `layout_of()` rather than tag checks, inner
  checks, or binding construction

A trial iterative rewrite of `emit_arm_chain()` did not produce a clear win and
was reverted.

### Fix

Reused `WasmTypeRegistry.layout_cache` directly during emission in `emit.tw`.
Added cached lookup helpers for emission-time layout queries and switched the
hot record/sum/intrinsic paths to use them instead of recomputing layouts from
`layout_of()` on every call.

This reduced repeated layout work in:

- `get_sum_layout()`-style sum emission paths
- record layout lookup for record literal/get/update
- intrinsic emitters that materialize typed record/sum results
- variant literal emission

### Result

This was a real codegen win:

- `emit_module`: roughly `~245ms -> ~186ms` (~24% reduction)
- `emit_func` body work: roughly `~200ms -> ~138ms` (~31% reduction)

### Updated priority order

1. `prepare_backend` — especially `repr_assign`
2. `emit_wasm_binary` / `code_section`
3. residual emit-time type/value-type lookup cleanup

---

## Snapshot (2026-04-25, repr_assign cache reuse)

After cleaning up the temporary diagnostics above, the next optimization reused
the same memoization idea in backend preparation.

### Investigation notes

Temporary sub-timing in `prepare_backend` showed:

- `insert_boundaries` was moderate
- `slot_assign` was moderate
- `repr_assign` was the dominant subphase

Code inspection showed `repr_assign` repeatedly recomputed the same facts for
many slots across the whole module graph:

- `repr_of_mono(mono, env)`
- `val_type_of_mono(mono, env)`
- `layout_of(mono, env)` through named-type repr derivation

With ~64k slots in the workload, the same monotypes were being re-derived many
times.

### Fix

Added a shared cache threaded through `assign_repr_for_module()` and
`assign_repr_for_func()` for:

- mono → `ReprKind`
- mono → `ValType`
- mono → `WasmLayout`

The public helper behavior stayed the same; the optimization is internal to the
repr-assignment pass. Boundary overrides (`AWrapAnyref`, `AUnwrapAnyref`) still
apply exactly as before.

### Result

Representative reruns after the cache landed:

```
compile_modules    ~562–583ms
prepare_backend    ~136–137ms   (was ~216–234ms before this change)
emit_module        ~136ms
emit_wasm_binary   ~210–211ms
  code_section     ~182–183ms
```

This was a large backend-preparation win:

- `prepare_backend`: roughly `~225ms -> ~136ms` (~40% reduction)

### Updated priority order

1. `emit_wasm_binary` / `code_section`
2. `emit_module` and other residual codegen lookup cleanup
3. any remaining preparation hotspots after cache reuse

---

## Current snapshot (2026-04-25, after repr_assign cache reuse)

Fresh whole-pipeline timing after the cleanup and repr cache work:

```
compile_modules    559ms
core_link           41ms
monomorphize        26ms
lower_anf           51ms
optimize           223ms
closure_convert     13ms
prepare_backend    137ms
verify             125ms
plan_wasm_types     40ms
emit_module        136ms
link                53ms
emit_wasm_binary   210ms
  type_order         4ms
  build_ctx          7ms
  type_section       3ms
  small_sections    13ms
  code_section     182ms
```

### What this means now

The earlier cache-reuse work changed the shape of the compiler substantially.
The main remaining heavy phases are now:

1. `compile_modules`
2. `optimize`
3. `code_section` inside `emit_wasm_binary`
4. `prepare_backend`, `verify`, and `emit_module` in a tight second tier

Within backend/codegen specifically, the single clearest isolated target is now
still `code_section`.

### Reusable optimization pattern

Two recent wins had the same structure:

- `emit.tw`: reuse precomputed `layout_cache` instead of repeatedly calling
  `layout_of()` in hot emission paths
- `repr_assign.tw`: thread a shared cache for repeated mono-derived facts
  (`ReprKind`, `ValType`, `WasmLayout`) across all functions and slots

The common lesson is:

> when a hot pass repeatedly recomputes facts from the same monotypes or names,
> a shared per-pass cache can beat local refactors by a wide margin.

So the next investigations should actively look for repeated derivation and
lookup churn before attempting structural rewrites.

### Current plan

#### 1. `emit_wasm_binary` / `code_section`

Check whether the same cache-reuse pattern applies to Wasm binary encoding.
Likely candidates:

- repeated `type_idx_of(ctx, name)` calls during instruction encoding
- repeated `func_idx_of(ctx, name)` / `global_idx_of(ctx, name)` lookups for
  call/global/ref-func heavy bodies
- repeated `find_label_depth()` scans over the same label stacks in dense
  control-flow regions
- payload assembly patterns that repeatedly append temporary function bodies

The likely highest-ROI version of the same pattern here is **pre-resolving or
memoizing instruction-adjacent indices**, not a large serializer rewrite.

---

## Snapshot (2026-04-25, wasm code-section follow-up)

This follow-up started from `emit_wasm_binary` with `code_section` as the
clearest isolated backend hotspot and applied the same “remove repeated work
first” heuristic in three steps.

### 1. Reuse instruction-adjacent index lookups

Added a shared `CodeSectionCache` threaded through the hot code-section encoder
for repeated lookups of:

- type name → type index
- func symbol → func index
- global name → global index
- data segment name → data index

Named heap-type encoding for `ref.null` / `ref.test` / `ref.cast` now reuses
that same cached type-index path.

### 2. Replace label-stack scans with a compact label context

Replaced the cached-path `Vector<String>` label-stack threading with `LabelCtx`,
which carries:

- current nesting depth
- named label → definition depth

This lets the hot encoder resolve branch depths directly instead of rescanning a
stack vector and avoids repeated `label_stack.append(...)` churn in nested
control flow.

### 3. Remove large temporary section copies

Temporary detail timing inside `encode_code_section_payload()` showed that a
large part of the remaining `code_section` cost was not just instruction
encoding but payload assembly.

The main issue was that `emit_wasm_parts()` still built a full section payload,
then wrapped it with `encode_section(...)` into another temporary vector, then
appended that vector into the final module buffer. For the code section, that
meant copying a large payload again.

Replaced `encode_section(...)` with `emit_section_into(...)` so section headers
and payload bytes are appended directly into the final module buffer.

### Result

Representative reruns after the full follow-up landed:

```
compile_modules    ~556–560ms
optimize           ~218–220ms
prepare_backend    ~136–137ms
verify             ~124–126ms
plan_wasm_types     ~39ms
emit_module        ~134ms
link                ~53–54ms
emit_wasm_binary   ~169–170ms
  type_order         ~2ms
  build_ctx          ~7ms
  type_section       ~3ms
  small_sections    ~13.5–13.7ms
  code_section     ~142–143ms
```

Compared to the previous whole-pipeline snapshot:

- `emit_wasm_binary`: roughly `~210ms -> ~170ms`
- `code_section`: roughly `~182ms -> ~143ms`

Compared to the start of this follow-up before any wasm changes:

- `emit_wasm_binary`: roughly `~221ms -> ~170ms`
- `code_section`: roughly `~193ms -> ~143ms`

### Interpretation

The first two fixes confirmed that repeated name lookup and repeated label-depth
reconstruction still mattered, but only incrementally at this stage.

The largest remaining backend win came from payload assembly: the code section
was paying for an extra full-section temporary copy. Removing that copy changed
`code_section` much more than the smaller lookup/context cleanups.

### Updated priority order

With this follow-up in place, the main remaining heavy phases are now:

1. `compile_modules`
2. `optimize`
3. `emit_wasm_binary` / `code_section` and `emit_module`
4. `prepare_backend` / `verify`

#### 2. `compile_modules`

Now that backend work is healthier, `compile_modules` is again the largest whole
phase. Apply the same question there:

- are there repeated monotype substitutions, instantiations, or env lookups
  that can be cached per module or per checker pass?
- are there repeated layout/type-entry lookups in checker/lowering code that can
  reuse already-derived facts instead of rediscovering them?

The parser-specific `Cursor` threading suspicion is currently deprioritized; the
better heuristic is still to hunt repeated derivation/lookup hot spots first.

#### 3. `optimize`

`optimize` is no longer dramatic, but at ~223ms it is large enough that the same
memoization question is still worth asking for the dominant subpasses once the
current codegen target is exhausted.

#### 4. `verify` / residual codegen cleanup

These are now medium-sized. Continue applying the same approach opportunistically
when a probe shows repeated fact derivation rather than true algorithmic work.

### Notes on temporary instrumentation

The follow-up used temporary sub-timings in `prepare_backend`, `emit_module`,
`code_section`, and several match/pattern helpers. Those probes were useful for
isolating the real hotspot and were removed after the investigation.

### Potential further improvements

**RRB-Trees for PVec** — current PVec concat is O(m·log n); RRB-Trees
(Bagwell & Rompf 2011) allow O(log n) concat/slice via relaxed internal nodes
with a size table. Would benefit sub-body instruction-vector building in codegen.

**CHAMP for HAMT** — Steindorfer 2015 improvement: two bitmaps per node
(data vs sub-trie) to inline key-value pairs directly in the node array,
eliminating the separate HamtEntry struct allocation per entry. Fewer
allocations, better iteration locality. Scala 2.13's HashMap uses CHAMP.

