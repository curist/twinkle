# COW Operation Analysis — Baseline (2026-04-02)

## How to reproduce

```bash
cargo test --release --test cow_analysis -- --nocapture
```

The test lives at `tests/cow_analysis.rs`. It compiles `boot/tests/main.tw` (the
full boot compiler + test suites) through the backend pipeline, then counts
COW-related FuncId calls in the ANF before and after the uniqueness optimization
pass.

## Results

Total functions in module: **2887**

### Aggregate counts

| Operation | Pre-opt | Post-opt | Rewritten to |
|---|---|---|---|
| VECTOR_APPEND | 792 | 751 | 41 -> builder (NEW/PUSH/FREEZE) |
| DICT_SET | 234 | 177 | 57 -> DICT_SET_IN_PLACE |
| VECTOR_CONCAT | 106 | 106 | (no rewrite exists) |
| REC_UPDATE | 98 | 98 COW, 0 in-place | (none rewritten) |
| VECTOR_SET_UNSAFE | (lowered from VECTOR_SET) | 0 | 14 -> VECTOR_SET_IN_PLACE |
| VECTOR_SET (safe) | 6 | 6 | (not targeted by opt) |
| DICT_REMOVE | 6 | 6 | (not rewritten) |

Post-opt in-place/builder summary:

| Optimized operation | Count |
|---|---|
| VECTOR_SET_IN_PLACE | 14 |
| DICT_SET_IN_PLACE | 57 |
| BUILDER_NEW | 161 |
| BUILDER_FROM | 10 |
| BUILDER_PUSH | 179 |
| BUILDER_FREEZE | 171 |

**Total COW operations remaining after optimization: 1144**

Optimization rate: ~8% of COW operations eliminated.

### Top 30 COW-heaviest functions (post-opt)

```
  1. lex                                                cow= 29  [VECTOR_APPEND=29]
  2. link                                               cow= 23  [VECTOR_APPEND=11, DICT_SET=12]
  3. emit_index_op                                      cow= 19  [VECTOR_APPEND=19]
  4. emit_op                                            cow= 18  [VECTOR_APPEND=18]
  5. register_layout_type_def                           cow= 17  [VECTOR_APPEND=5, DICT_SET=4, REC_UPDATE_COW=8]
  6. emit_str_binop                                     cow= 16  [VECTOR_APPEND=16]
  7. emit_wat_parts                                     cow= 15  [VECTOR_APPEND=15]
  8. parse_prefix                                       cow= 15  [VECTOR_APPEND=11, VECTOR_CONCAT=4]
  9. emit_intrinsic_string_slice                        cow= 15  [VECTOR_APPEND=15]
 10. emit_universal_closure_call                        cow= 14  [VECTOR_APPEND=14]
 11. unescape_string                                    cow= 13  [VECTOR_APPEND=13]
 12. emit_intrinsic_vector_get                          cow= 13  [VECTOR_APPEND=13]
 13. emit_intrinsic_vector_set                          cow= 13  [VECTOR_APPEND=13]
 14. emit_intrinsic_string_get                          cow= 13  [VECTOR_APPEND=13]
 15. emit_utf8_boundary_check                           cow= 13  [VECTOR_APPEND=13]
 16. emit_intrinsic_from_char_code                      cow= 13  [VECTOR_APPEND=13]
 17. lower_module                                       cow= 13  [VECTOR_APPEND=2, VECTOR_CONCAT=1, DICT_SET=4, REC_UPDATE_COW=6]
 18. parse_type_expr_base                               cow= 12  [VECTOR_APPEND=7, VECTOR_CONCAT=5]
 19. emit_intrinsic_byte_from_int                       cow= 12  [VECTOR_APPEND=12]
 20. emit_intrinsic_char_code_at                        cow= 11  [VECTOR_APPEND=11]
 21. emit_universal_trampoline                          cow= 11  [VECTOR_APPEND=11]
 22. make_prelude_optimizer_semantics                   cow= 11  [DICT_SET=11]
 23. emit_intrinsic_call                                cow= 10  [VECTOR_APPEND=10]
 24. rewrite_calls_kind                                 cow= 10  [VECTOR_APPEND=10]
 25. mock_semantics                                     cow= 10  [DICT_SET=10]
 26. parse_function                                     cow=  9  [VECTOR_APPEND=5, VECTOR_CONCAT=4]
 27. parse_type                                         cow=  9  [VECTOR_APPEND=5, VECTOR_CONCAT=4]
 28. emit_intrinsic_cell_update                         cow=  9  [VECTOR_APPEND=9]
 29. emit_closure_call                                  cow=  9  [VECTOR_APPEND=9]
 30. emit_string_pool_getters                           cow=  9  [VECTOR_APPEND=9]
```

### Analysis: where the pain is

**1. VECTOR_APPEND (751 remaining, 95% un-optimized)**

The dominant cost. Most are in the WAT emitter (`emit_*` functions) and parser,
where WAT instruction lists / token lists are built by appending one element at
a time. The uniqueness pass fails to optimize these because:
- The accumulator vector is typically passed to helper functions (taints it)
- Or threaded through branches/match arms (conservative analysis)
- Or the append is not in a simple loop but in straight-line code

**2. VECTOR_CONCAT (106, 0% optimized)**

No rewrite strategy exists for concat. Used in the parser for merging sub-lists
(e.g., `parse_prefix` building node children). Each concat copies both operands.

**3. DICT_SET (177 remaining, 24% optimized)**

Used in the resolver, linker, and checker for building environment maps. Many
are in straight-line code or threaded through function calls, preventing the
uniqueness pass from proving single ownership.

**4. REC_UPDATE (98, 0% optimized)**

Record field updates (e.g., `InferCtx` in the type checker) are all COW. The
checker threads `ctx` through every call, updating fields like `subst`,
`next_meta`, `locals`. Each update copies the entire record struct. This is the
most architecturally concerning pattern: the checker does O(N) record updates
where N is the number of AST nodes, and each update is O(fields) — giving
O(N * fields) total copy work. With nested data (dict-valued fields), the
effective cost is higher.

### What would help most

1. **Interprocedural escape analysis** — most VECTOR_APPEND calls fail because
   the vector is passed to a helper. If the pass could prove the helper doesn't
   retain the reference, many more appends could use builders.

2. **Record update in-place** — the checker's `InferCtx` threading pattern
   (`ctx = ctx.with_field(new_val)`) is the classic case. The analysis currently
   never rewrites record updates because the base record is often a function
   parameter (tainted by default).

3. **VECTOR_CONCAT rewrite** — either prove one operand is dead (consume it as
   the base) or use a builder that accepts bulk appends.
