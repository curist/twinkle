# Boot Compiler: Pub Binding Rebinding Rule

Align the boot checker (`boot/compiler/checker.tw`) with the stage0 rule change: only public bindings are restricted from rebinding at module scope; private top-level bindings may be freely rebound.

## Background

The stage0 Rust compiler previously rejected all rebinding at module scope (`at_module_scope` flag in `src/types/check.rs`). This was relaxed so that only `pub` bindings are restricted — private bindings follow normal rebinding rules (spec &sect;7.3&ndash;7.4, &sect;8).

The boot checker still enforces the old blanket rule via `ctx.locals.len() == 1` in two places:
- `synth_assign_op` (line 1650)
- `check_let` for `is_rebind` (line 2312)

## Steps

### 1. Add `is_pub` to `LetStmt`

**File:** `boot/compiler/ast.tw`

Add `is_pub: Bool` field to the `LetStmt` record type.

### 2. Thread `is_pub` through the parser

**File:** `boot/compiler/parser.tw`

Currently `parse_item_at` routes `Pub` followed by non-`type`/non-`fn` to `parse_top_level_stmt`, which calls `parse_stmt` &rarr; `parse_binding_stmt`. The `pub` token is never consumed for let statements.

Options (pick one):
- **(a)** In `parse_item_at`, when `Pub` is followed by `Ident`, advance past `Pub` and call a variant of `parse_binding_stmt` that sets `is_pub = true`.
- **(b)** Add a `Pub` case in `parse_stmt` that delegates to `parse_binding_stmt` with an `is_pub` parameter.

Option (a) is simpler since pub lets are only valid at item level, not inside blocks.

### 3. Track pub names in the checker

**File:** `boot/compiler/checker.tw`

Add a `pub_names: Dict<String, Bool>` (or a dedicated set type) to `InferCtx`. In `check_let`, when processing a non-rebind let with `is_pub == true`, insert the name into `pub_names`.

### 4. Replace the blanket module-scope check

**File:** `boot/compiler/checker.tw`

In both `synth_assign_op` and `check_let` (the `is_rebind` path), replace:

```
if ctx.locals.len() == 1 {
  // reject
}
```

with:

```
if ctx.pub_names.has(name) {
  // reject with "cannot rebind public binding 'name'"
}
```

### 5. Update error message

Change the diagnostic from `"assignment at module scope is not allowed"` to `"cannot rebind public binding '${name}'"` to match the stage0 error.

### 6. Add tests

Add boot checker test cases:
- **Pass:** private top-level rebinding (`x := 1; x = 2`)
- **Pass:** rebinding in top-level for loop (`sum := 0; for ... { sum = sum + i }`)
- **Fail:** pub binding rebinding (`pub x := 1; x = 2`)
