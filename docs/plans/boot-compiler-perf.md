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

