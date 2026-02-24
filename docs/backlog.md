# Twinkle Compiler — Backlog

Work items that need to be retrofitted into already-completed stages,
or built as part of upcoming stages.

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
  (Canonical surface names are `String.of_*`; `int_to_string`/friends are intrinsic aliases.)

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

- Ensure `field_method_collision.tw` test correctly fails once inherent method
  registration lands in Stage 4.
