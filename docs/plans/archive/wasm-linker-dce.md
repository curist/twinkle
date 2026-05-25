# Wasm Linker DCE Plan

**Status: DONE** — Implemented and landed. Self-host loop converges, all tests pass.

## Results

Playground example (`twinkle.tw`): **21,611 → 9,098 bytes (58% reduction), 150 → 68 functions**.

## What was implemented

### Step 1: Hide codegen.intrinsics exports — DONE

`retain_final_exports` in `boot/compiler/codegen/linker.tw` now also filters
`codegen.intrinsics`, removing 5 internal helper exports from the final module.

### Step 2: Wasm reachability analysis — DONE

New file: `boot/compiler/codegen/linker_dce.tw`

- DFS worklist traversal seeded from exports, element segments, and global initializers
- Follows `Call`, `ReturnCall`, `RefFunc` edges
- Recurses into `If`, `Block`, `Loop` bodies
- Threads `Dict<String, Bool>` through return values (no Cell)

### Step 3: Filter functions and imports — DONE

- Dead `FuncDef`s and `ImportDef`s removed via `collect ... continue`
- Types, globals, tables, elems, data preserved (conservative)

### Step 4: Conservative first version — DONE

Only functions and imports are eliminated. Follow-up can add:
- Unreferenced types, globals, data segments, empty tables/elements

### Step 5: Wire into pipeline — DONE

Called from `link_program` in `boot/compiler/codegen/codegen.tw` after linking,
not from inside `link()` (avoids circular import between linker and linker_dce).
Timing reported under `TWINKLE_TIMINGS=1` as `wasm_dce`.

### Step 6: Tests — DONE

- Existing test fixtures updated to have reachable top-level calls
- Negative assertion added: unused `lib.json` import is verified absent after DCE

## Follow-up: Narrow Core DCE Roots

The existing Core IR DCE currently roots every function in the entry module. That is conservative and can keep private helper functions that are not reachable from top-level execution.

Consider refining executable-mode Core DCE roots to include only:

- init/top-level execution
- explicit public exports required by the selected build mode
- any host/bridge roots that must be public

This is a separate change from Wasm linker DCE because it changes source-level linking policy.
