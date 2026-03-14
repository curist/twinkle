# Pre-Self-Hosting Cleanup

Refactoring and cleanup tasks to address before Stage 10 (self-hosting).

---

## High Impact (codegen cleanup)

- [x] **Remove dead `_func_table` parameter from `emit_user_module`**
  Remove the unused `_func_table: &HashMap<String, FuncId>` parameter from
  `emit_user_module` in `emit.rs` and all call sites (`cli/build.rs`, unit tests).
  Always passed as empty `HashMap`.

- [x] **Deduplicate helpers between emit.rs and ctx.rs**
  Remove duplicate functions that exist in both files:
  - `atom_iterator_state` — identical in both
  - `iterator_state_from_unfold_args` — identical in both
  - `atom_mono` (emit.rs) vs `infer_atom_mono` (ctx.rs) — same logic
  - Flow-binding push/restore free functions in emit.rs just delegate to ctx
    methods — remove the wrappers

- [x] **Fix `needs_iterator_next_helper` stub**
  The function body is `ctx.imports().iter().any(|_| false) || { true }` which
  unconditionally returns `true`. Either implement a proper check or inline
  `true` and remove the function.

- [ ] **Remove vestigial `concrete_func_sigs.is_empty()` guards**
  Scattered through `ctx.rs` and `emit.rs` — these were safety nets from before
  typed closures became the only path. Now that `emit_user_module` always
  populates `concrete_func_sigs`, these guards are vestigial. Evaluate and
  remove where safe.

## Optimization

- [x] **Optimize `collect_concrete_func_signatures` linear scan**
  Inside `maybe_insert_concrete_sig`,
  `anf.functions.iter().find(|f| f.func_id == func_id)` is a linear scan called
  for every `AMakeClosure`/`AGlobalFunc` atom. Build a
  `HashMap<FuncId, &AnfFunctionDef>` upfront instead.

- [ ] **Simplify `collect_capture_mono_by_func` fixed-point loop**
  Uses a fixed-point loop with full `HashMap` clone per iteration. Since ANF
  functions are topologically ordered, a single forward pass in dependency order
  should suffice.

- [ ] **Parameterize typed variant pattern-condition code**
  Three near-identical blocks in `emit_pattern_condition` for
  `typed_iter_option`, `typed_unfold_step`, and `typed_general_option`. Only
  variation is which struct symbol is used and how `field_monos` are extracted.
  Extract a shared helper.

- [x] **Avoid cloning in `ctx.imports()` call**
  `ctx.imports()` returns `.values().cloned().collect()`, allocating a new `Vec`.
  One call site at `emit.rs` just checks `.any(...)` — can check the map
  directly. The main call could use `.values().cloned()` directly without the
  intermediate `Vec`.

## Structural / Architecture

- [ ] **Extract generic ANF tree visitor**
  ~6 different `collect_*_expr`/`collect_*_op` families all manually walk the
  same `AnfExpr`/`AnfOp` structure with different per-leaf actions. A generic
  visitor/fold combinator would eliminate the repeated boilerplate.

- [ ] **Deduplicate opt pass structural recursion**
  `dead_let_elim_op`, `copy_propagate_op`, `constant_fold_op`,
  `branch_simplify_op` all repeat the same `AIf`/`AMatch`/`ALoop`/`ADefer`
  structural recursion (~100 lines duplicated). Extract a shared traversal
  helper.

- [x] **Simplify `prioritize_specialized_iterator_types`**
  Currently empties `module.types` into 7 temp `Vec`s then concatenates in
  order. A single `sort_by_key` with integer priority per type-name prefix
  would be cleaner.

## Small Cleanup

- [x] **Remove `pub fn parse` stub in `syntax/mod.rs`**
  Marked with `/// TODO: Remove once integration tests are updated`. Legacy
  stub that wraps `parse_source` and discards the result.

- [x] **Move `ref_user_record_null` to `#[cfg(test)]`**
  Function in `emit.rs` has `#[allow(dead_code)]` — only used in one test.
  Move it into the test module.

- [x] **Fix stale "Stage 8c" panic message in `emit_let_binding`**
  Panic message references historical "Stage 8c". Update to reflect current
  state.

- [x] **Avoid unnecessary clone in `build_user_sig_map_typed`**
  `.cloned().unwrap_or_default()` on `capture_locals` produces a new `Vec` per
  function when only the length is needed. Use `.map_or(0, Vec::len)` instead.

- [x] **Clean up surviving TODO/FIXME comments**
  - `ir/core.rs:5` — TODO about serde dependency
  - `types/check.rs:2750` — TODO about method call vs field access
