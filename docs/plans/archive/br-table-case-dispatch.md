# br_table dispatch for variant case expressions

## Context

Variant `case` expressions are currently compiled as linear if-else chains. Each
arm emits a tag comparison (`struct.get` + `i32.const` + `i32.eq`) wrapped in a
wasm `if/else`, and the arms are nested recursively:

```wat
;; case x { .A => ..., .B => ..., .C => ... }
;; becomes:
struct.get $Foo 0
i32.const 0
i32.eq
if
  ;; arm A
else
  struct.get $Foo 0
  i32.const 1
  i32.eq
  if
    ;; arm B
  else
    struct.get $Foo 0
    i32.const 2
    i32.eq
    if
      ;; arm C
    else
      unreachable
    end
  end
end
```

This is O(n) in the number of arms. The tag is re-read from the struct for
every arm. For large enums (e.g. `Instr` with 60+ variants, `CoreExprKind` with
20+ variants, `TokenKind` with 50+ variants), this generates deeply nested code
with redundant work.

WebAssembly has `br_table` — a jump table that dispatches in O(1) given a dense
integer index. Twinkle's variant tags are already dense integers 0..N-1 assigned
in declaration order, and the tag is always field 0 of the sum struct. The
`BrTable` wasm IR node already exists in `wasm_ir.tw` and is correctly
serialized in both binary (`wasm.tw`) and WAT (`wat.tw`) emitters, but no
codegen pass currently produces it.

## Goal

For case expressions where all arms are variant patterns on the same sum type
(with an optional wildcard/variable catch-all), emit `br_table` dispatch instead
of nested if-else chains.

This optimization applies to the **boot compiler only** (`boot/compiler/`).
The Rust stage0 in `src/` is not changed.

Non-goal: changing how literal patterns or guard expressions are dispatched at
the top level. Those continue using the existing if-else chain. Cases with
non-trivial payload sub-patterns (e.g. `.Some(.Ok(v))`) also fall back to
if-else in the first implementation. A future per-tag bucket extension can
support these safely (see Future extensions).

## Current code path

```
emit.tw:emit_match_op (line 1468)
  → match.tw:emit_match_op (line 18)
    → match.tw:emit_arm_chain (line 25)  ← recursive if-else builder
```

`emit_arm_chain` processes one arm at a time:
1. `emit_pattern_condition` — emits tag comparison, pushes i32 bool
2. Wraps arm body + recursive else in `.If(.None, then_body, else_body)`
3. Base case: `.Unreachable`

Supporting functions in `emit.tw`:
- `emit_variant_pattern_condition` (line 1527) — `struct.get` + `i32.const` + `i32.eq`, plus inner sub-pattern checks
- `emit_pattern_bindings` (line 1582) — extracts payload fields into locals
- `can_match_variant_pattern` (line 1478) — checks if scrutinee is a sum type

## Design

### Eligibility check

A case expression is eligible for br_table dispatch when:

1. The scrutinee type is a sum type (enum), Optional, or Result.
2. Every arm's top-level pattern is either `.Variant(tid, vid, _)`, `.Wildcard`,
   or `.Var(_)`.
3. No top-level pattern is a literal (`LitInt`, `LitBool`, `LitStr`).
4. There are at least 3 explicit variant arms (below that, if-else is fine).
5. Each explicit variant tag appears **at most once** across all arms.
6. All variant payload sub-patterns are **trivial** (`.Wildcard` or `.Var(_)`
   only — no nested variant patterns, no literal sub-patterns).
7. There is at most one wildcard/variable catch-all arm, and it is the
   **last** arm.

If any condition fails, fall back to the existing `emit_arm_chain`.

Rules 5–6 are the key safety constraints. Today, nested sub-pattern matching
is performed by `emit_variant_pattern_condition`, not by `emit_pattern_bindings`.
`emit_pattern_bindings` only extracts and binds payload fields — it does not
verify that nested patterns match. If br_table dispatched to an arm with a
non-trivial sub-pattern and used only `emit_pattern_bindings`, it would skip
the nested condition check and execute the wrong arm body.

8. Each explicit variant ID must correspond to a variant in the scrutinee's sum
   layout. The type checker should guarantee this, but defensive rejection
   avoids generating a bad table from malformed IR.

Rule 7 ensures source-order semantics are preserved. A wildcard before explicit
arms would shadow them in the current if-else chain; br_table must not change
that behavior. In practice, the type checker rejects unreachable arms after a
wildcard, so this rule is defensive.

### Wasm structure

For a sum type with N variants and a case with arms covering tags
{t0, t1, ..., tk} plus an optional default:

```wat
(block $match_end
  (block $default
    (block $arm_k
      ...
      (block $arm_1
        (block $arm_0
          <scrutinee_instrs>
          struct.get $SumType 0  ;; read tag once
          br_table $arm_0 $arm_1 ... $arm_k $default
        )
        ;; arm for tag 0: bindings + body + store result
        <arm_0_bindings>
        <arm_0_body>
        local.set $result
        br $match_end
      )
      ;; arm for tag 1
      ...
    )
    ...
  )
  ;; default arm body if wildcard present, otherwise unreachable
  <default_body or unreachable>
)
;; result is in $result local
```

Key points:
- The tag is read **once** from the struct, not per-arm.
- `br_table` entries are ordered by tag value (0, 1, 2, ...), not by source arm
  order. The compiler builds a tag-to-arm mapping.
- Tags not covered by an explicit arm route to the default block.
- The default block **always** exists. If there is a wildcard arm, it becomes
  the default body. If the match is exhaustive with no wildcard, the default
  emits `unreachable` — this catches corrupt or out-of-range tag values
  defensively rather than silently routing to a valid arm.
- Each explicit arm block ends with `br $match_end` (unless it diverges).
- Diverging arms (returning, breaking, erroring) skip the result store and
  `br $match_end`, same as the current if-else logic.
- If every arm diverges, append an `unreachable` after the outer match block,
  matching the existing if-else-chain lowering. This preserves Wasm validation
  and type-flow behavior for matches whose result is never produced.
- The wildcard/default body is emitted last, after all arm blocks. It stores
  the result and falls through to `$match_end` — no explicit `br` needed
  because it is already at the end of the outer block.
- Arm bindings still re-emit `scrutinee_instrs` for payload extraction via
  `emit_pattern_bindings`. This is correct because `PreparedAtom`s are cheap
  (typically a single `local.get`), not a redundant struct read.

### Tag-to-arm mapping

Given arms in source order and a sum type with N variants:

1. Collect all explicit variant arms into a `tag → arm_index` map.
2. Identify the wildcard arm (if any) as the default.
3. Build the `br_table` label vector: for each tag 0..N-1, emit the label for
   the corresponding arm, or the default label if no explicit arm covers it.
4. If there is no wildcard and not all tags are covered, the default emits
   `unreachable` (the type checker guarantees exhaustiveness, so this is
   defensive).

### Sub-pattern restriction

The first implementation requires all variant payload sub-patterns to be trivial
(`.Wildcard` or `.Var(_)` only). This is because:

- `emit_pattern_bindings` only extracts payload fields into locals — it does
  **not** check whether nested patterns match.
- `emit_variant_pattern_condition` is responsible for nested condition checks
  (e.g. checking an inner tag for `.Some(.Ok(v))`).
- If br_table dispatched on the outer tag and then called only
  `emit_pattern_bindings`, non-trivial sub-patterns would be silently skipped.

With trivial sub-patterns, `emit_pattern_bindings` is sufficient — it just
binds the payload fields to locals, which is all that's needed after the tag
dispatch.

### Integration with existing code

The change is localized to `boot/compiler/codegen/emit/match.tw`. The entry
point `emit_match_op` gains a check: if the case is eligible for br_table
dispatch, call a new `emit_br_table_match` function; otherwise fall through to
the existing `emit_arm_chain`.

No changes needed to:
- `wasm_ir.tw` — `BrTable` node already exists
- `wasm.tw` / `wat.tw` — serialization already works
- `emit_pattern_bindings` — reused as-is inside each arm block (trivial
  sub-patterns only, so extraction without condition checking is correct)
- ANF or Core IR — no IR changes, this is purely a codegen strategy

`MatchEmitFns` already includes `match_scrutinee_mono` for accessing the
scrutinee's mono type, which is needed for the eligibility check and layout
lookup.

Generated block labels must be unique within the function. Use `result_idx`
(the local index where the match result is stored) as a disambiguator:
`$case_{result_idx}_arm_{tag}`, `$case_{result_idx}_default`,
`$case_{result_idx}_end`. Local indices are function-scoped and unique per
match expression, so this avoids collisions without adding new state to
`EmitCtx`.

## Steps

### Step 1: Add br_table eligibility check

In `boot/compiler/codegen/emit/match.tw`, add a function:

```
fn is_br_table_eligible(arms, scrutinee_mono, ctx) Bool
```

Checks all eight eligibility rules:
1. Scrutinee is a sum type.
2. All top-level patterns are Variant, Wildcard, or Var.
3. No literal patterns.
4. At least 3 explicit variant arms.
5. No duplicate top-level variant tags.
6. All variant payload sub-patterns are trivial (Wildcard or Var only).
7. Catch-all (if present) is the last arm.
8. All variant IDs are valid for the scrutinee's sum layout.

Returns false if any rule fails.

**Verify:** add a counter/log behind `TWINKLE_TIMINGS` to confirm how many
case expressions are eligible in a self-compilation. Cases with nested
sub-patterns (e.g. `.Some(.Ok(v))`) or duplicate tags should be correctly
rejected.

### Step 2: Implement `emit_br_table_match`

New function in `match.tw`:

```
fn emit_br_table_match(
  scrutinee_instrs, scrutinee_mono, arms, result_idx, result_mono, ctx, buf, fns
) Vector<Instr>
```

Logic:
1. Get the sum layout via `get_sum_layout_ctx`.
2. Count total variants N from the layout.
3. Build tag-to-arm-index map and identify default arm.
4. Generate block labels using `result_idx` for uniqueness:
   - One label per explicit arm: `$case_{result_idx}_arm_{tag}`
   - `$case_{result_idx}_default`
   - `$case_{result_idx}_end`
   Only explicit arms and the default need blocks. The `br_table` label vector
   has N entries (one per tag), with uncovered tags pointing to the default label.
5. Build each explicit arm's body: `emit_pattern_bindings` + `emit_expr` +
   `append_result_store` + `Br($case_{result_idx}_end)` (unless divergent).
6. Build the default body: if a catch-all arm exists,
   `emit_pattern_bindings(default_pattern, scrutinee_instrs, scrutinee_mono, ctx)`
   + `emit_expr` + `append_result_store` (no `br` needed — falls through).
   If no catch-all, emit `unreachable`. The bindings step is required for
   `.Var(sid)` catch-alls (e.g. `other => use(other)`) — without it the
   variable would read an unset local.
7. If `all_arms_diverge(arms)` is true, append an `unreachable` after the
   outer match block, just as `emit_arm_chain` does when both sides of the
   generated if/else tree diverge.
8. Build the `br_table` label vector ordered by tag 0..N-1.
9. Assemble the nested block structure with `BrTable` at the innermost level.

### Step 3: Wire into `emit_match_op`

Replace the current unconditional call to `emit_arm_chain` with:

```
if is_br_table_eligible(arms, scrutinee_mono, ctx) {
  emit_br_table_match(...)
} else {
  emit_arm_chain(...)
}
```

### Step 4: Bootstrap verification

Run the full self-host loop to verify correctness:

```bash
make stage2
target/twk run boot/tests/main.tw
```

The stage2 compiler must produce identical output (or at least pass all tests).
Compare stage1 and stage2 wasm binaries — they may differ in instruction
encoding but must be semantically equivalent.

### Step 5: Measure

A/B timing comparison with `TWINKLE_TIMINGS=1`:

```bash
# Before (current)
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/before.wasm

# After (with br_table)
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/after.wasm
```

Expected impact areas:
- `emit_module` should decrease (fewer instructions emitted per case, fewer
  `struct.get` instructions in output)
- Generated wasm binary size should decrease slightly (br_table is more compact
  than nested if-else for large matches)
- Runtime performance of the compiled compiler may improve (O(1) dispatch vs
  O(n) for large enums), which could show up in `compile_modules` if the
  compiler's own case matches over large enums run faster

## Risks and constraints

- **Nested variant pattern bug** (see `boot-nested-variant-pattern-lowering.md`):
  this change does not alter pattern matching semantics — it only changes the
  dispatch mechanism from if-else to br_table. The first implementation
  explicitly rejects non-trivial sub-patterns (rule 6), so the nested pattern
  bug cannot be triggered through this path.

- **Block nesting depth**: wasm engines have implementation limits on nesting
  depth. A br_table with N arms creates N+2 nested blocks. For the largest enum
  in the compiler (~60 variants for `Instr`), this is 62 levels — well within
  typical engine limits (V8 allows thousands).

- **Small matches**: br_table has overhead (block setup, label vector). The
  eligibility check requires at least 3 explicit variant arms.

- **Wildcard-only or variable-only arms**: a case with only `_ => ...` should
  not generate br_table. The eligibility check handles this by requiring at
  least 3 explicit variant patterns.

## Future extensions

- **Per-tag arm buckets**: lift rules 5–6 by grouping arms that share the same
  top-level tag into a bucket. br_table dispatches to the bucket, then the
  bucket runs the existing `emit_arm_chain` (with `emit_pattern_condition` for
  sub-pattern checks) over the arms within that tag. This enables br_table for
  cases like `case x { .Some(.Ok(v)) => ..., .Some(.Err(e)) => ..., .None => ... }`
  — the outer dispatch is O(1), the inner `.Some` bucket uses if-else over 2
  arms.

- **Nested br_table**: once per-tag buckets work, inner nested variant matches
  could themselves use br_table recursively.

- **Literal switch**: integer case expressions with dense ranges could also
  use br_table. This is a separate optimization with different eligibility
  rules.

- **Bool case optimization**: `case bool_expr { true => ..., false => ... }`
  should just be an `if/else`, not br_table. This is already handled correctly
  since bool patterns are `LitBool`, not `Variant`.
