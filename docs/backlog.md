# Twinkle Compiler — Backlog

Work items that need to be retrofitted into already-completed stages,
or built as part of upcoming stages.

---

## Stage 2 — Type Checker

### Type alias expansion
**Status:** Parser produces `TypeDef::Alias { ty }` correctly. Type checker silently
ignores it (`TypeDef::Alias { .. } => {}` in `env.rs` lines 54 & 87).

**Work:**
- Register alias in `TypeEnv` so the alias name resolves to its underlying type.
- Expand aliases transparently during type checking (alias names unify with the
  target type; they are not new nominal types).

### Dict index assignment type checking
**Status:** `synth_assign` handles `ExprKind::Index` but only for `Array<T>`.
`m[k] = v` on a `Dict<K,V>` falls to the type mismatch error path.

**Work:**
- Add a `MonoType::Dict(k, v)` arm in `synth_assign`'s `ExprKind::Index` branch.
- Check: index type matches `K`, value type matches `V`, result is `Void`.

### Compound assignment cleanup
**Status:** `BinOp::AddAssign/SubAssign/MulAssign/DivAssign/ModAssign` and
`synth_compound_assign` still exist even though compound assignment was dropped
from the spec.

**Work:**
- Remove `PlusEq/MinusEq/StarEq/SlashEq/PercentEq` tokens from lexer.
- Remove `AddAssign/SubAssign/MulAssign/DivAssign/ModAssign` from `BinOp` in AST.
- Remove `synth_compound_assign` from type checker.
- Remove the `PlusEq =>` mapping in parser.

---

## Stage 3 — Lowering

### Lvalue assignment desugaring
**Status:** `extract_simple_assign` (lower.rs:1479) only handles `Ident` targets.
`r.field = expr` and `arr[i] = val` parse correctly as
`BinOp(Assign, FieldAccess/Index, rhs)` but fall through the lowerer unhandled.

**Work:**
- `r.field = expr` → `Assign(r_local, RecordUpdate(Local(r_local), field, expr))`.
  Needs a `RecordUpdate` Core IR node (or lower as a call to a record-copy helper).
- `arr[i] = expr` → `Assign(arr_local, Call(Array.set, [Local(arr_local), i, expr]))`.
- `m[k] = expr` → `Assign(m_local, Call(Dict.set, [Local(m_local), k, expr]))`.

Note: For field and index lvalue targets, the lowerer must resolve the root local
(e.g., for `a.b.c = x`, the root is `a`) and re-assign it.

### Dict `for k, v in dict` iteration
**Status:** AST `Stmt::For { index_pattern, .. }` supports the two-pattern form,
but the lowerer's `for x in coll` path likely only handles `Array`. Dict iteration
is unimplemented.

**Work:**
- Detect when the iterator expression has type `Dict<K,V>`.
- Lower to a loop over `Dict.keys(d)`, binding key and looking up value per iteration,
  or use whatever dict iteration primitive is defined in the stdlib.

---

## Stage 5 — Interpreter

### Full stdlib as native builtins
**Status:** Prelude FuncIds 1–11 are dispatched natively. The rest of the stdlib
has no native implementation.

**Work — Array module:**
- `Array.set(arr, i, val) Array<T>` — return new array with element replaced.
- `Array.concat(arr1, arr2) Array<T>` — concatenate two arrays.
- `Array.slice(arr, start, end) Array<T>` — subset.

**Work — Dict module:**
- `Dict.new() Dict<K,V>` — empty dict.
- `Dict.set(m, k, v) Dict<K,V>` — return new dict with key set.
- `Dict.remove(m, k) Dict<K,V>` — return new dict without key.
- `Dict.get(m, k) Option<V>` — safe lookup.
- `Dict.has(m, k) Bool` — membership test.
- `Dict.keys(m) Array<K>` — key list.
- `Dict.len(m) Int` — entry count.

**Work — String module:**
- `String.substring(s, start, end) String`.
- `String.of_int(n) String`, `String.of_float(f) String`, `String.of_bool(b) String`.
  (Note: `int_to_string` etc. at FuncId 4–6 are the same things under different names;
  resolve naming consistency.)

**Work — Range:**
- `range(n) Array<Int>` — 0..n-1.
- `range_from(a, b) Array<Int>` — a..b-1.
- `range_step(a, b, step) Array<Int>`.

### Dict `Value` representation
**Status:** `Value::Dict(...)` is listed in the plan but has no concrete type.

**Work:**
- Decide representation: `HashMap<Value, Value>` (requires `Value: Hash + Eq`)
  or `Vec<(Value, Value)>` for simplicity first.
- Implement all Dict builtins on top of chosen representation.

---

## Cleanup (no specific stage)

- Remove compound assignment from lexer/parser/AST/type checker (see Stage 2 item above).
- Ensure `field_method_collision.tw` test correctly fails once inherent method
  registration lands in Stage 4.
