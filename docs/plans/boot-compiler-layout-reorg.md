# Boot Compiler Layout Reorganization

## Goal

Reorganize `boot/compiler/` into clearer subdirectories so syntax, semantic
analysis, IR definitions, lowering, module orchestration, optimization, backend,
and debug tooling stop living side by side in one crowded root.

This is a structural refactor only. The target is a cleaner end-state import
layout, not a behavior change.

---

## Why This Needs a Plan First

This refactor is broad even though it is mostly file moves:

* many modules import each other by path
* boot tests import compiler modules directly
* naming changes and directory changes should land consistently
* some files sit on boundary lines (`core_linker`, `artifacts`, `builtins`)

Without an explicit move plan, it is easy to end up with an inconsistent
half-reorganized tree or a pile of compatibility shims that keep the old
structure alive indefinitely.

---

## Non-Goals

* No parser/checker/lowering/codegen behavior changes
* No large internal decomposition of `parser.tw`, `checker.tw`, `lower_core.tw`,
  `lower_anf.tw`, or `codegen/emit.tw`
* No public compatibility layer that preserves both old and new import paths
  long-term
* No attempt to redesign the compiler architecture in this plan

---

## Target Layout

```text
boot/compiler/
  pipeline.tw
  frontend/
    ast.tw
    tokens.tw
    cursor.tw
    lexer.tw
    parser.tw
  sema/
    resolver.tw
    checker.tw
  bootstrap/
    base_env.tw
    signatures.tw
    builtins.tw
  ir/
    core.tw
    anf.tw
    anf_analysis.tw
  lower/
    ast_to_core.tw
    monomorphize.tw
    core_to_anf.tw
  module/
    imports.tw
    compiler.tw
    linker.tw
    artifacts.tw
    fs_util.tw
  debug/
    ir_print.tw
  opt/
    ...
  codegen/
    pipeline.tw
    wasm/
      ir.tw
      layout.tw
      plan.tw
      emit.tw
      linker.tw
      wat.tw
    runtime/
      ...
```

---

## Naming Decisions

These names should be treated as part of the refactor, not optional cleanup.

| Old path | New path | Reason |
|------|------|------|
| `boot/compiler/core_ir.tw` | `boot/compiler/ir/core.tw` | IR definition file should live under `ir/` and `core_ir` is redundant there |
| `boot/compiler/lower_core.tw` | `boot/compiler/lower/ast_to_core.tw` | Makes the transform direction explicit |
| `boot/compiler/lower_anf.tw` | `boot/compiler/lower/core_to_anf.tw` | Makes the transform direction explicit |
| `boot/compiler/module_compiler.tw` | `boot/compiler/module/compiler.tw` | This is module orchestration, not a top-level compiler concept |
| `boot/compiler/core_linker.tw` | `boot/compiler/module/linker.tw` | Links Core modules across module boundaries, not part of IR definition |
| `boot/compiler/util.tw` | `boot/compiler/module/fs_util.tw` | Current file is too generic; its only current purpose is FS-related error text |
| `boot/compiler/codegen/codegen.tw` | `boot/compiler/codegen/pipeline.tw` | `codegen/codegen.tw` is tautological |
| `boot/compiler/codegen/wasm_ir.tw` | `boot/compiler/codegen/wasm/ir.tw` | Wasm-specific backend internals should live under `codegen/wasm/` |
| `boot/compiler/codegen/wasm_layout.tw` | `boot/compiler/codegen/wasm/layout.tw` | Same reason |
| `boot/compiler/codegen/wasm_plan.tw` | `boot/compiler/codegen/wasm/plan.tw` | Same reason |
| `boot/compiler/codegen/linker.tw` | `boot/compiler/codegen/wasm/linker.tw` | Wasm module linker, backend-specific |

The following keep their current basename but move directories only:

* `ast.tw`, `tokens.tw`, `cursor.tw`, `lexer.tw`, `parser.tw`
* `resolver.tw`, `checker.tw`
* `base_env.tw`, `signatures.tw`, `builtins.tw`
* `anf.tw`, `anf_analysis.tw`
* `imports.tw`, `artifacts.tw`, `ir_print.tw`
* `codegen/emit.tw`, `codegen/insert_boundaries.tw`, `codegen/wat.tw`

---

## Desired Root Surface

After the reorg, `boot/compiler/` root should be intentionally small.

Files expected to remain at root:

* `pipeline.tw`

Everything else should live in a focused subdirectory unless a strong reason
appears during implementation.

---

## Work Plan

### Phase 0: Freeze the target map

- [ ] Confirm the target directory layout and rename map in this document.
- [ ] Confirm that the root should keep only `pipeline.tw`.
- [ ] Decide whether `builtins.tw` belongs under `bootstrap/` or should later move
      to a backend/shared folder. For this plan, keep it in `bootstrap/`.

### Phase 1: Prepare directory structure

- [ ] Create `boot/compiler/frontend/`
- [ ] Create `boot/compiler/sema/`
- [ ] Create `boot/compiler/bootstrap/`
- [ ] Create `boot/compiler/ir/`
- [ ] Create `boot/compiler/lower/`
- [ ] Create `boot/compiler/module/`
- [ ] Create `boot/compiler/debug/`
- [ ] Create `boot/compiler/codegen/wasm/`

No import rewrites yet. This phase is just scaffold creation.

### Phase 2: Move leaf files first

Move files with relatively local dependency surfaces before moving central
coordinator files.

- [ ] Move frontend files:
      `ast.tw`, `tokens.tw`, `cursor.tw`, `lexer.tw`, `parser.tw`
- [ ] Move semantic files:
      `resolver.tw`, `checker.tw`
- [ ] Move bootstrap files:
      `base_env.tw`, `signatures.tw`, `builtins.tw`
- [ ] Move IR files:
      `core_ir.tw -> ir/core.tw`,
      `anf.tw -> ir/anf.tw`,
      `anf_analysis.tw -> ir/anf_analysis.tw`
- [ ] Move lowering files:
      `lower_core.tw -> lower/ast_to_core.tw`,
      `monomorphize.tw -> lower/monomorphize.tw`,
      `lower_anf.tw -> lower/core_to_anf.tw`
- [ ] Move debug file:
      `ir_print.tw -> debug/ir_print.tw`

Update imports after each coherent group, not one file at a time.

### Phase 3: Move orchestration files

- [ ] Move `imports.tw -> module/imports.tw`
- [ ] Move `artifacts.tw -> module/artifacts.tw`
- [ ] Move `core_linker.tw -> module/linker.tw`
- [ ] Move `module_compiler.tw -> module/compiler.tw`
- [ ] Move `util.tw -> module/fs_util.tw`

This phase rewrites the highest-churn coordinator imports:

* `boot/compiler/pipeline.tw`
* `boot/compiler/module/compiler.tw`
* boot tests that import these modules directly

### Phase 4: Normalize backend naming

- [ ] Move `codegen/codegen.tw -> codegen/pipeline.tw`
- [ ] Create `codegen/wasm/`
- [ ] Move `codegen/wasm_ir.tw -> codegen/wasm/ir.tw`
- [ ] Move `codegen/wasm_layout.tw -> codegen/wasm/layout.tw`
- [ ] Move `codegen/wasm_plan.tw -> codegen/wasm/plan.tw`
- [ ] Move `codegen/linker.tw -> codegen/wasm/linker.tw`
- [ ] Decide whether `codegen/emit.tw`, `codegen/insert_boundaries.tw`,
      and `codegen/wat.tw` also move under `codegen/wasm/`.

Recommendation: yes, move them too, so Wasm backend internals live together.

Preferred end-state:

- [ ] `codegen/emit.tw -> codegen/wasm/emit.tw`
- [ ] `codegen/insert_boundaries.tw -> codegen/wasm/insert_boundaries.tw`
- [ ] `codegen/wat.tw -> codegen/wasm/wat.tw`

### Phase 5: Rewrite imports to the new stable layout

- [ ] Rewrite all boot compiler internal imports
- [ ] Rewrite `boot/main.tw`
- [ ] Rewrite helper modules under `boot/tests/helpers/`
- [ ] Rewrite all suites under `boot/tests/suites/`
- [ ] Search for stale `compiler.<old_name>` imports

No compatibility re-exports should remain unless a specific cycle forces a
temporary shim during the refactor.

### Phase 6: Validation and cleanup

- [ ] Run boot tests
- [ ] Run any relevant Rust-side tests that reference boot compiler paths
- [ ] Remove any dead comments or path references that still describe the old layout
- [ ] Update any design/internal docs that mention the old locations directly

---

## Import Rewrite Strategy

To keep the refactor reviewable and low-risk:

1. Move one subsystem at a time.
2. Rewrite all imports for that subsystem immediately.
3. Run targeted search after each phase for stale paths.
4. Keep file contents unchanged unless an import or comment must change.

Avoid mixing directory reorg with helper extraction or behavior cleanup in the
same commit.

---

## Acceptance Criteria

1. `boot/compiler/` root contains only `pipeline.tw`.
2. Syntax, sema, bootstrap, IR, lowering, module orchestration, debug tooling,
   optimization, and backend files each live in focused directories.
3. No old import paths remain in boot compiler code or boot tests.
4. The refactor does not change compiler behavior.
5. The resulting layout makes it obvious where a new file belongs without
   adding more root-level clutter.

---

## Open Questions

### Should `builtins.tw` live in `bootstrap/`?

Short term: yes. It is tightly coupled to boot compiler bootstrap setup and is
imported by lowering, optimization, and backend code as shared metadata.

Longer term, if builtin registry logic grows into a stage-neutral subsystem, it
may deserve a `shared/` or `runtime_contract/` home. That is out of scope here.

### Should all Wasm backend files move under `codegen/wasm/`?

Yes. The clean end-state is stronger if Wasm-specific internals stop sitting next
to the backend entrypoint.

Suggested split:

* keep `boot/compiler/codegen/pipeline.tw` as the backend entrypoint
* move backend implementation details into `boot/compiler/codegen/wasm/`
* keep runtime support under `boot/compiler/codegen/runtime/`
