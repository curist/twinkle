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

## Stage 4 Cleanup

### Multi-segment module paths in type annotations — ✅ fixed

`parse_type` previously handled exactly one level of qualification (`module.Type`).
Fixed to use a `while` loop so deeper paths parse correctly.  Note: module aliases
are always single identifiers (last path segment or explicit `as alias`), so in
practice type annotations only ever need one dot (`vec.Vec2` from `use math.vec`).
The fix keeps the parser consistent with `expr_as_type_name` in record constructors.

---

### Same-named types across imported modules silently collide — ✅ fixed

`TypeEnv.type_names` is a flat `HashMap<String, TypeId>`.  When two imported
modules both declare a type with the same unqualified name (e.g. `type Point`),
the second registration silently overwrites the first.  The collision only causes
wrong behaviour if an importing module uses the bare name — which is inherently
ambiguous — but there is no error reported: the wrong `TypeId` is returned silently.

**Detection point:** The resolver, in `collect_declarations_for_context`, adds each
module's own types to the shared `TypeEnv` via `add_type`.  The right place to
detect the collision is here (or in `register_module_exports`), by tracking which
module contributed each bare name and reporting an error when a second module
tries to register the same name.

**Fix options (in order of invasiveness):**

1. *Track ownership in TypeEnv* — add a `type_name_owner: HashMap<String, String>`
   (bare name → module alias).  On `add_type`, if the name already has a different
   owner, emit an error.  The importing module must then always use `module.Type`.

2. *Scope bare names per module* — after resolving a module, remove its bare type
   names from the shared `TypeEnv` so they cannot leak.  Cross-module references
   must always go through qualified aliases.  This aligns with the language design
   (module access is always explicit).

This must be fixed before Stage 5 (the interpreter trusts that `TypeId`s are
correct; silent misidentification would produce wrong runtime dispatch for variant
patterns and record construction).

---

## Cleanup (no specific stage)

- Ensure `field_method_collision.tw` test correctly fails once inherent method
  registration lands in Stage 4.
