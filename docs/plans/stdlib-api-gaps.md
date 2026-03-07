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

### P1 — Writeable in Twinkle, needed for ergonomic compiler code

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

---

### P2 — Nice to have

#### Numeric conversions

```tw
Int.to_float(n: Int) -> Float
Float.to_int(f: Float) -> Int
```

#### Dict extras

```tw
fn values<K, V>(d: Dict<K, V>) Vector<V>
```

---

## Infrastructure: inherent methods for builtin types

Today, inherent methods (via `TypeEnv::add_method`) only work for
`MonoType::Named` types. Builtin types (`Vector`, `String`, `Dict`) have
their methods hard-coded in the type checker and lowerer.

To register Twinkle-defined functions as inherent methods on builtins:

1. Give builtin types a synthetic TypeId (or key) for method lookup
2. Add a fallback in `synth_method_call`'s builtin arms: before erroring on
   unknown method, check `TypeEnv::get_method_function`
3. Write the functions in a stdlib `.tw` module and register them during
   module loading

This unblocks all P1 items above without adding more Rust builtins.

---

## Status

| Item | Priority | Status |
|------|----------|--------|
| String ordering (`<`, `>`, `<=`, `>=`) | P0 | Not started |
| `char_code_at` / `from_char_code` | P0 | Not started |
| `Int.from_string` / `Float.from_string` | P0 | Not started |
| Inherent methods for builtins (infra) | P1 | Not started |
| Vector combinators | P1 | Not started |
| String utilities | P1 | Not started |
| Numeric conversions | P2 | Not started |
| Dict extras | P2 | Not started |
