# Stage 9.7 — Standard Library & API Gaps

**Goal:** Fill API gaps needed for ergonomic compiler development in Twinkle,
preparing the language for self-hosting (Stage 10).

**Principle:** Prefer implementing in Twinkle (as stdlib or inherent methods)
over adding Rust builtins, wherever possible.

---

## Priorities

### P0 — Blocks self-hosting ✅ DONE

#### String ordering comparisons ✅

`<`, `>`, `<=`, `>=` on strings. Lexicographic byte comparison via `rt_str__cmp`.
Works in both interpreter and Wasm codegen.

#### `char_code_at` / `from_char_code` ✅

```tw
char_code_at(s: String, i: Int) -> Int      // byte value at index
from_char_code(n: Int) -> Option<String>    // ASCII (0-127) to 1-char string
```

#### `int_from_string` / `float_from_string` ✅

```tw
int_from_string(s: String) -> Option<Int>    // pure Wasm, no host call
float_from_string(s: String) -> Option<Float> // delegates to host
```

`int_from_string` is implemented entirely in Wasm (digit loop with sign handling).
`float_from_string` delegates to a host import (`host.parse_float`).

---

### P1 — Writeable in Twinkle, needed for ergonomic compiler code ✅ DONE

These can be implemented as Twinkle functions and registered as inherent
methods on builtin types (requires the inherent-method-for-builtins
infrastructure — see below).

#### Vector combinators

```tw
fn map<A, B>(xs: Vector<A>, f: fn(A) B) Vector<B>
fn filter<A>(xs: Vector<A>, f: fn(A) Bool) Vector<A>
fn fold<A, B>(xs: Vector<A>, init: B, f: fn(B, A) B) B
fn find<A>(xs: Vector<A>, f: fn(A) Bool) Option<A>
fn any<A>(xs: Vector<A>, f: fn(A) Bool) Bool
fn all<A>(xs: Vector<A>, f: fn(A) Bool) Bool
fn contains<A>(xs: Vector<A>, elem: A) Bool
fn reverse<A>(xs: Vector<A>) Vector<A>
fn join(xs: Vector<String>, sep: String) String
```

#### String utilities

```tw
fn contains(s: String, needle: String) Bool
fn index_of(s: String, needle: String) Option<Int>
fn starts_with(s: String, prefix: String) Bool
fn ends_with(s: String, suffix: String) Bool
fn split(s: String, sep: String) Vector<String>
fn trim(s: String) String
```

Implemented in:
- `@std.vector` (`stdlib/vector.tw`)
- `@std.string_ext` (`stdlib/string_ext.tw`)

---

### P2 — Nice to have ✅ DONE

#### Numeric conversions

```tw
Int.to_float(n: Int) -> Float
Float.to_int(f: Float) -> Int
```

#### Dict extras

```tw
fn values<K, V>(d: Dict<K, V>) Vector<V>
```

Implemented in:
- `@std.numeric` (`stdlib/numeric.tw`)
- `@std.dict_ext` (`stdlib/dict_ext.tw`)

---

## Infrastructure: inherent methods for builtin types

Completed:
1. Builtin receiver types use synthetic method-lookup TypeIds
2. `synth_method_call` builtin arms now fall back to `TypeEnv::get_method_function`
3. Lowering has matching builtin-method fallback
4. Module registration now records builtin receiver methods and exposes
   `Vector.*` / `String.*` / `Dict.*` qualified entries when available

---

## Status

| Item | Priority | Status |
|------|----------|--------|
| String ordering (`<`, `>`, `<=`, `>=`) | P0 | Done |
| `char_code_at` / `from_char_code` | P0 | Done |
| `int_from_string` / `float_from_string` | P0 | Done |
| Inherent methods for builtins (infra) | P1 | Done |
| Vector combinators | P1 | Done |
| String utilities | P1 | Done |
| Numeric conversions | P2 | Done |
| Dict extras | P2 | Done |
