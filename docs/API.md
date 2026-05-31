# Twinkle API Reference

This reference documents concrete built-in and standard-library APIs. The
compiler-recognized contract reference is [contracts.md](contracts.md).

## Primitive Types

| Type | Wasm repr | Description |
|------|-----------|-------------|
| `Int` | i64 | 64-bit integer |
| `Float` | f64 | 64-bit floating-point |
| `Bool` | i32 | Boolean (`true` / `false`) |
| `Byte` | i32 | Single byte (0–255) |
| `Void` | — | Unit type |

## Built-in Types

### `Option<T>`
```tw
type Option<T> = { None, Some(T) }
```
Shorthand: `T?` is equivalent to `Option<T>`.

| Function | Signature | Description |
|----------|-----------|-------------|
| `.is_some()` | `fn<T>(opt: Option<T>) Bool` | True when `opt` is `Some` |
| `.is_none()` | `fn<T>(opt: Option<T>) Bool` | True when `opt` is `None` |
| `.unwrap()` | `fn<T>(opt: Option<T>) T` | Extract `Some(v)`; traps if `opt` is `None` |
| `.unwrap_or(default)` | `fn<T>(opt: Option<T>, default: T) T` | Extract `Some(v)` or return `default` |
| `.unwrap_or_else(f)` | `fn<T>(opt: Option<T>, f: fn() T) T` | Extract `Some(v)` or lazily compute a default |
| `.map(f)` | `fn<T, U>(opt: Option<T>, f: fn(T) U) Option<U>` | Transform `Some(v)` into `Some(f(v))`; leaves `None` unchanged |
| `.and_then(f)` | `fn<T, U>(opt: Option<T>, f: fn(T) Option<U>) Option<U>` | Chain Option-producing steps without nesting |
| `.flatten()` | `fn<T>(opt: Option<Option<T>>) Option<T>` | Remove one level of nested `Option` |
| `.filter(pred)` | `fn<T>(opt: Option<T>, pred: fn(T) Bool) Option<T>` | Keep `Some(v)` only when `pred(v)` is true |
| `.or_some(other)` | `fn<T>(opt: Option<T>, other: Option<T>) Option<T>` | Return `opt` if it is `Some`, otherwise return `other` |
| `.or_else(f)` | `fn<T>(opt: Option<T>, f: fn() Option<T>) Option<T>` | Return `opt` if it is `Some`, otherwise lazily compute another option |
| `.inspect(f)` | `fn<T>(opt: Option<T>, f: fn(T) Void) Option<T>` | Run `f(v)` for `Some(v)` and return the original option |
| `.ok_or(err)` | `fn<T, E>(opt: T?, err: E) Result<T, E>` | Convert to Result: `Some(v)` → `Ok(v)`, `None` → `Err(err)` |
| `.ok_or_else(f)` | `fn<T, E>(opt: T?, f: fn() E) Result<T, E>` | Lazy variant — `f()` is only called when `opt` is `None` |
| `.transpose()` | `fn<T, E>(opt: Option<Result<T, E>>) Result<Option<T>, E>` | Convert `Option<Result<T,E>>` into `Result<Option<T>,E>` |
| `.to_string()` | `fn<T: Stringify>(opt: Option<T>) String` | Witnesses `Stringify`: `Some(v)` → `Some(<v>)`, `None` → `None`; enables `${opt}` interpolation |

`try` on Option: `try opt` extracts the `Some` value or propagates `None` via early return.
Only valid in functions returning `Option<U>`. Use `.ok_or(err)` to bridge to `Result` contexts.

```tw
fn first_valid(a: Int?, b: Int?) Int? {
  x := try a       // returns None if a is None
  y := try b
  .Some(x + y)
}
```

### `Result<T, E>`
```tw
type Result<T, E> = { Ok(T), Err(E) }
```
Shorthand: `T!E` is equivalent to `Result<T, E>`.

Supports `try` sugar — `v := try expr` extracts the `Ok` value or propagates the `Err`.

| Function | Signature | Description |
|----------|-----------|-------------|
| `.is_ok()` | `fn<T, E>(res: Result<T, E>) Bool` | True when `res` is `Ok` |
| `.is_err()` | `fn<T, E>(res: Result<T, E>) Bool` | True when `res` is `Err` |
| `.unwrap()` | `fn<T, E: Stringify>(res: Result<T, E>) T` | Extract `Ok(v)`; traps with the error value if `res` is `Err(e)` |
| `.unwrap_or(default)` | `fn<T, E>(res: Result<T, E>, default: T) T` | Extract `Ok(v)` or return `default` |
| `.unwrap_or_else(f)` | `fn<T, E>(res: Result<T, E>, f: fn(E) T) T` | Extract `Ok(v)` or map the error to a fallback value |
| `.map(f)` | `fn<T, U, E>(res: Result<T, E>, f: fn(T) U) Result<U, E>` | Transform `Ok(v)` into `Ok(f(v))`; leaves `Err(e)` unchanged |
| `.map_err(f)` | `fn<T, E, F>(res: Result<T, E>, f: fn(E) F) Result<T, F>` | Transform `Err(e)` into `Err(f(e))`; leaves `Ok(v)` unchanged |
| `.and_then(f)` | `fn<T, U, E>(res: Result<T, E>, f: fn(T) Result<U, E>) Result<U, E>` | Chain Result-producing steps without nested Results |
| `.or_else(f)` | `fn<T, E, F>(res: Result<T, E>, f: fn(E) Result<T, F>) Result<T, F>` | Recover from an error by lazily producing another Result |
| `.ok()` | `fn<T, E>(res: Result<T, E>) Option<T>` | Convert `Ok(v)` to `Some(v)` and discard errors |
| `.err()` | `fn<T, E>(res: Result<T, E>) Option<E>` | Convert `Err(e)` to `Some(e)` and discard success values |
| `.transpose()` | `fn<T, E>(res: Result<Option<T>, E>) Option<Result<T, E>>` | Convert `Result<Option<T>,E>` into `Option<Result<T,E>>` |
| `.to_string()` | `fn<T: Stringify, E: Stringify>(res: Result<T, E>) String` | Witnesses `Stringify`: `Ok(v)` → `Ok(<v>)`, `Err(e)` → `Err(<e>)`; enables `${res}` interpolation |

### `Cell<T>`
Mutable reference cell for imperative state.

| Function | Signature | Description |
|----------|-----------|-------------|
| `Cell.new` | `fn<T>(v: T) Cell<T>` | Create a cell containing `v` |
| `.get()` | `fn<T>(c: Cell<T>) T` | Read current value |
| `.set(v)` | `fn<T>(c: Cell<T>, v: T) Void` | Overwrite value |
| `.update(f)` | `fn<T>(c: Cell<T>, f: fn(T) T) Void` | Apply `f` to current value and store result |

### `Range`
Record with fields `{ start: Int, end: Int, step: Int }`. Iterable in `for` loops.

| Function | Signature | Description |
|----------|-----------|-------------|
| `range` | `fn(n: Int) Range` | Range `[0, n)` with step 1 |
| `range_from` | `fn(start: Int, end: Int) Range` | Range `[start, end)` with step 1 |
| `range_step` | `fn(start: Int, end: Int, step: Int) Range` | Range with custom step |

### `Iterator<T>`
Lazy iterator type.

| Function | Signature | Description |
|----------|-----------|-------------|
| `Iterator.next` | `fn<T>(it: Iterator<T>) Option<IterItem<T>>` | Advance iterator |
| `Iterator.unfold` | `fn<T,S>(seed: S, step: fn(S) UnfoldStep<T,S>) Iterator<T>` | Build iterator from seed + step function |
| `Iterator.map` | `fn<T,U>(it: Iterator<T>, f: fn(T) U) Iterator<U>` | Lazy adapter that transforms each yielded value |
| `Iterator.filter` | `fn<T>(it: Iterator<T>, pred: fn(T) Bool) Iterator<T>` | Lazy adapter that yields only matching values |
| `Iterator.take` | `fn<T>(it: Iterator<T>, n: Int) Iterator<T>` | Lazy adapter that yields at most `n` values (`n <= 0` yields empty) |
| `Iterator.drop` | `fn<T>(it: Iterator<T>, n: Int) Iterator<T>` | Lazy adapter that drops the first `n` values (`n <= 0` drops none) |
| `Iterator.take_while` | `fn<T>(it: Iterator<T>, pred: fn(T) Bool) Iterator<T>` | Lazy adapter that yields values until `pred` returns false |
| `Iterator.drop_while` | `fn<T>(it: Iterator<T>, pred: fn(T) Bool) Iterator<T>` | Lazy adapter that drops values while `pred` returns true, then yields the rest |
| `Iterator.to_vector` | `fn<T>(it: Iterator<T>) Vector<T>` | Materialize iterator into a vector. Equivalent to `collect x in it { x }`. Traverses the full iterator — infinite iterators will not terminate. O(n) memory. The iterator itself is persistent and can be reused. |

Supporting types:
```tw
type IterItem<T> = .{ value: T, rest: Iterator<T> }
type UnfoldStep<T, S> = { Done, Yield(T, S) }
```

### `Order`
```tw
type Order = { Lt, Eq, Gt }
```
Comparison result type used by `sort_by` and primitive comparators.

| Function | Signature | Description |
|----------|-----------|-------------|
| `Int.compare` | `fn(a: Int, b: Int) Order` | Compare two integers |
| `Float.compare` | `fn(a: Float, b: Float) Order` | Compare two floats |
| `String.compare` | `fn(a: String, b: String) Order` | Lexicographic byte-order comparison |
| `Byte.compare` | `fn(a: Byte, b: Byte) Order` | Compare two bytes by numeric value |

Comparators can be passed directly as function references:
```tw
nums.sort_by(Int.compare)
names.sort_by(String.compare)
```

## I/O

| Function | Signature | Description |
|----------|-----------|-------------|
| `print` | `fn(s: String) Void` | Print to stdout (no newline) |
| `println` | `fn(s: String) Void` | Print to stdout with newline |
| `eprint` | `fn(s: String) Void` | Print to stderr (no newline) |
| `eprintln` | `fn(s: String) Void` | Print to stderr with newline |
| `error` | `fn(s: String) Void` | Trap with error message (unrecoverable) |

## Type Conversions

| Function | Signature | Description |
|----------|-----------|-------------|
| `Int.to_string` | `fn(n: Int) String` | Convert `Int` to `String` |
| `Float.to_string` | `fn(f: Float) String` | Convert `Float` to `String` |
| `Bool.to_string` | `fn(b: Bool) String` | Convert `Bool` to `String` |
| `String.to_string` | `fn(s: String) String` | Identity (returns input) |
| `Int.from_string` | `fn(s: String) Option<Int>` | Parse string to `Int` |
| `Float.from_string` | `fn(s: String) Option<Float>` | Parse string to `Float` |
| `Int.to_float` | `fn(n: Int) Float` | Convert `Int` to `Float` |
| `Float.to_int` | `fn(f: Float) Int` | Convert integral `Float` to `Int` (traps if not integral) |
| `Float.bits` | `fn(f: Float) Int` | Return the IEEE 754 bit pattern of the float as an `Int` |
| `String.from_char_code` | `fn(n: Int) Option<String>` | Single-char string from integer code (ASCII range) |
| `String.from_byte` | `fn(b: Byte) Option<String>` | Single-char string from byte value (ASCII range) |
| `String.from_code_point` | `fn(n: Int) Option<String>` | String from Unicode code point (full range) |

Conversion functions can be used as first-class function references (e.g. `nums.map(Int.to_string)`). The dot-call form `.to_string()` also works on values directly.

`Float.bits` is a low-level operation mainly useful for binary encoding, hashing, and other bit-exact work.

## Byte

Primitive type representing a single byte (0–255). Returned by string indexing (`s[i]`) and byte iteration (`for b in s`).

| Function | Signature | Description |
|----------|-----------|-------------|
| `Byte.to_int` | `fn(b: Byte) Int` | Convert byte to integer |
| `Byte.from_int` | `fn(n: Int) Option<Byte>` | Convert integer in range 0..255 to `Byte` |
| `Byte.to_string` | `fn(b: Byte) String` | Convert byte to string representation |
| `Byte.compare` | `fn(a: Byte, b: Byte) Order` | Compare two bytes by numeric value |
| `.in_range(lo, hi)` | `fn(b: Byte, lo: Int, hi: Int) Bool` | Whether byte is in the inclusive ASCII range `[lo, hi]` |
| `.is_upper()` | `fn(b: Byte) Bool` | Whether byte is an ASCII uppercase letter |
| `.is_lower()` | `fn(b: Byte) Bool` | Whether byte is an ASCII lowercase letter |
| `.is_digit()` | `fn(b: Byte) Bool` | Whether byte is an ASCII decimal digit |
| `.is_hex_digit()` | `fn(b: Byte) Bool` | Whether byte is an ASCII hexadecimal digit |
| `.is_alpha()` | `fn(b: Byte) Bool` | Whether byte is an ASCII letter |
| `.is_alnum()` | `fn(b: Byte) Bool` | Whether byte is an ASCII letter or decimal digit |
| `.is_newline()` | `fn(b: Byte) Bool` | Whether byte is LF (`0x0A`) |
| `.is_space()` | `fn(b: Byte) Bool` | Whether byte is an ASCII whitespace character recognized by the lexer |

## String

Strings are immutable, UTF-8 encoded, and GC-managed. String interpolation: `"hello ${name}"`.

**Byte-oriented model:** Lengths, indices, and slicing all operate on **byte offsets**, not characters. Indexing (`s[i]`) returns a `Byte`. This is efficient but means multi-byte UTF-8 characters occupy multiple index positions. Use the Unicode helpers below for character-level operations.

### Core (builtin)

| Method | Signature | Description |
|--------|-----------|-------------|
| `.len()` | `fn(s: String) Int` | Length in **bytes** |
| `.is_empty()` | `fn(s: String) Bool` | Whether the string has no bytes |
| `s[i]` | — | Byte at byte offset `i` (returns `Byte`, traps on OOB) |
| `.get(i)` | `fn(s: String, i: Int) Option<Byte>` | Safe byte lookup at byte offset |
| `.slice(start, end)` | `fn(s: String, start: Int, end: Int) String` | Substring by **byte offsets** `[start, end)`. Out-of-range indices are clamped to `[0, len]`. Traps if a clamped index falls mid-codepoint |
| `.concat(other)` | `fn(s: String, other: String) String` | Concatenate two strings |
| `.char_code_at(i)` | `fn(s: String, i: Int) Int` | Byte value at byte offset `i` (same as `Byte.to_int(s[i])`) |
| `.utf8_bytes()` | `fn(s: String) Vector<Byte>` | Copy string bytes into a vector |
| `String.from_utf8` | `fn(bytes: Vector<Byte>) Option<String>` | Validate UTF-8 and create string |
| `String.from_byte` | `fn(b: Byte) Option<String>` | Create a one-character string from an ASCII byte; returns `None` for non-ASCII bytes |

### Prelude (auto-imported, no import needed)

| Method | Signature | Description |
|--------|-----------|-------------|
| `.index_of(needle)` | `fn(s: String, needle: String) Option<Int>` | First byte offset of `needle` |
| `.contains(needle)` | `fn(s: String, needle: String) Bool` | Whether `s` contains `needle` |
| `.starts_with(prefix)` | `fn(s: String, prefix: String) Bool` | Prefix check |
| `.ends_with(suffix)` | `fn(s: String, suffix: String) Bool` | Suffix check |
| `.split(sep)` | `fn(s: String, sep: String) Vector<String>` | Split on separator (empty sep returns `[s]`) |
| `.lines()` | `fn(s: String) Vector<String>` | Split on newlines (handles both `\n` and `\r\n`) |
| `.trim()` | `fn(s: String) String` | Strip leading/trailing ASCII whitespace |
| `.strip_prefix(prefix)` | `fn(s: String, prefix: String) Option<String>` | Remove prefix and return remainder, or `None` |
| `.strip_suffix(suffix)` | `fn(s: String, suffix: String) Option<String>` | Remove suffix and return remainder, or `None` |
| `.count(needle)` | `fn(s: String, needle: String) Int` | Count non-overlapping occurrences of `needle` |
| `.replace(old, new)` | `fn(s: String, old: String, new_s: String) String` | Replace all non-overlapping occurrences |

### Unicode helpers (prelude)

| Method | Signature | Description |
|--------|-----------|-------------|
| `.substring(start, end)` | `fn(s: String, start: Int, end: Int) String` | Substring by **character indices** `[start, end)`. Safe for multi-byte UTF-8. Indices clamped to `[0, char_len]`. O(n) |
| `.chars()` | `fn(s: String) Iterator<String>` | Iterate Unicode scalars (each as a 1–4 byte `String`) |
| `.char_len()` | `fn(s: String) Int` | Number of Unicode scalars |
| `.code_point_at(i)` | `fn(s: String, i: Int) Option<Int>` | Code point at **scalar index** `i` (O(n)) |
| `.graphemes()` | `fn(s: String) Iterator<String>` | Iterate extended grapheme clusters (user-perceived characters). Handles combining marks, ZWJ emoji sequences, and regional indicator flags via a simplified UAX #29 implementation. |

**Iteration:** `for b in s { ... }` iterates **bytes** (`b: Byte`). Use `for ch in s.chars() { ... }` to iterate Unicode scalars, or `for g in s.graphemes() { ... }` for user-perceived characters (grapheme clusters).

Qualified forms (`String.len`, `String.trim`, etc.) also work.

## Vector\<T\>

Persistent vector with structural sharing. Literal syntax: `[1, 2, 3]`.

| Method | Signature | Description |
|--------|-----------|-------------|
| `.len()` | `fn<T>(v: Vector<T>) Int` | Number of elements |
| `.is_empty()` | `fn<T>(v: Vector<T>) Bool` | Whether the vector has no elements |
| `.append(elem)` | `fn<T>(v: Vector<T>, elem: T) Vector<T>` | Append element, return new vector. O(log n) amortized, effectively constant with the tail buffer |
| `.get(i)` | `fn<T>(v: Vector<T>, i: Int) Option<T>` | Safe index lookup. O(log n) |
| `.set(i, val)` | `fn<T>(v: Vector<T>, i: Int, val: T) Option<Vector<T>>` | Safe update at index. O(log n) |
| `.concat(other)` | `fn<T>(v: Vector<T>, other: Vector<T>) Vector<T>` | Concatenate two vectors structurally. O(log n), sharing both operands' trees |
| `.slice(start, end)` | `fn<T>(v: Vector<T>, start: Int, end: Int) Vector<T>` | Subvector `[start, end)`. O(log n), sharing the source tree except boundary spines |
| `.map(f)` | `fn<A,B>(xs: Vector<A>, f: fn(A) B) Vector<B>` | Transform each element |
| `.filter(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Vector<A>` | Keep elements where `f` returns true |
| `.fold(init, f)` | `fn<A,B>(xs: Vector<A>, init: B, f: fn(B,A) B) B` | Left fold |
| `.first()` | `fn<A>(xs: Vector<A>) Option<A>` | First element, or `.None` if empty |
| `.last()` | `fn<A>(xs: Vector<A>) Option<A>` | Last element, or `.None` if empty |
| `.drop_first()` | `fn<A>(xs: Vector<A>) Vector<A>` | Vector without its first element, or empty if already empty. O(log n) via structural slice |
| `.drop_last()` | `fn<A>(xs: Vector<A>) Vector<A>` | Vector without its last element, or empty if already empty. O(log n) worst-case, O(1) when shrinking the tail |
| `.find(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Option<A>` | First element matching predicate |
| `.any(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Bool` | True if any element matches |
| `.all(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Bool` | True if all elements match |
| `.contains(elem)` | `fn<A>(xs: Vector<A>, elem: A) Bool` | True if `elem` is in the vector |
| `.position(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Option<Int>` | Index of first element matching predicate |
| `.flat_map(f)` | `fn<A,B>(xs: Vector<A>, f: fn(A) Vector<B>) Vector<B>` | Map each element to a vector and flatten |
| `.reverse()` | `fn<A>(xs: Vector<A>) Vector<A>` | Reverse order |
| `.sort_by(cmp)` | `fn<T>(xs: Vector<T>, cmp: fn(T,T) Order) Vector<T>` | Return a new sorted vector using comparator (e.g. `xs.sort_by(Int.compare)`) |
| `.join(sep)` | `fn(xs: Vector<String>, sep: String) String` | Join strings with separator |
| `Vector.make` | `fn<T>(size: Int, fill: T) Vector<T>` | Create vector of `size` filled with `fill` |

**Indexing syntax:** `v[i]` — unsafe, traps on out-of-bounds.

**Index assignment:** `v[i] = x` — sets element at index `i`.

Vectors are iterable: `for x in v { ... }` and `for x, i in v { ... }`.
Qualified forms (`Vector.map`, `Vector.filter`, etc.) also work.

## Dict\<K, V\>

Persistent hash map. Keys must be `Int`, `String`, or `Byte`. `keys()`, dict iteration,
and helpers built on top of them preserve insertion order of first insertion; updating an
existing key keeps its position, and remove+reinsert appends it at the end.

| Method | Signature | Description |
|--------|-----------|-------------|
| `Dict.new()` | `fn<K,V>() Dict<K,V>` | Create empty dict |
| `.get(key)` | `fn<K,V>(d: Dict<K,V>, key: K) Option<V>` | Look up value by key |
| `.set(key, value)` | `fn<K,V>(d: Dict<K,V>, key: K, value: V) Dict<K,V>` | Insert/replace key-value pair, return new dict |
| `.len()` | `fn<K,V>(d: Dict<K,V>) Int` | Number of entries |
| `.is_empty()` | `fn<K,V>(d: Dict<K,V>) Bool` | Whether the dict has no entries |
| `.has(key)` | `fn<K,V>(d: Dict<K,V>, key: K) Bool` | Check if key exists |
| `.keys()` | `fn<K,V>(d: Dict<K,V>) Vector<K>` | All keys as a vector |
| `.values()` | `fn<K,V>(d: Dict<K,V>) Vector<V>` | All values as a vector |
| `.remove(key)` | `fn<K,V>(d: Dict<K,V>, key: K) Dict<K,V>` | Remove key, return new dict |

**Lookup syntax:** `d[key]` — returns `Option<V>`.

**Assignment syntax:** `d[key] = value` — sets key-value pair (sugar for `Dict.set`).

The free functions `dict_get(d, key)` and `dict_get_unsafe(d, key)` also exist.

Dicts are iterable: `for k, v in d { ... }`.
Qualified forms (`Dict.values`, etc.) also work.

## Operators

### Arithmetic (`Int` and `Float`)
`+`, `-`, `*`, `/`, `%`, unary `-`

Division by zero traps.

### Comparison
`==`, `!=`, `<`, `<=`, `>`, `>=`

### Logical
`and`, `or`, `!` (prefix not)

### String interpolation
`"text ${expr} more"` — calls `.to_string()` on interpolated expressions.

---

## Standard Library

Everything above (primitives, built-in types, I/O, type conversions, String/Vector/Dict methods, operators) is available as **prelude** — no import needed.

Only non-prelude stdlib modules require explicit imports: `use @std.path`, `use @std.fs`, `use @std.proc`, `use @std.date`, `use @std.view`.

### `@std.path`

Path manipulation (string-based, no I/O).

| Function | Signature | Description |
|----------|-----------|-------------|
| `is_absolute` | `fn(path: String) Bool` | Starts with `/` |
| `join` | `fn(base: String, part: String) String` | Join two path segments |
| `join_all` | `fn(parts: Vector<String>) String` | Join multiple segments |
| `dirname` | `fn(path: String) String` | Directory component |
| `basename` | `fn(path: String) String` | Filename component |
| `stem` | `fn(path: String) String` | Filename without extension |
| `extension` | `fn(path: String) String` | Extension including dot |
| `normalize` | `fn(path: String) String` | Canonicalize (resolve `.` and `..`) |

### `@std.fs`

File system operations. Functions return `Result` types with `FsError`.

```tw
type FsError = { NotFound, PermissionDenied, InvalidUtf8, Other(String) }
type EntryKind = { File, Dir, Other }
type DirEntry = .{ name: String, kind: EntryKind }
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `read_bytes` | `fn(path: String) Vector<Byte>!FsError` | Read raw file bytes |
| `read_text` | `fn(path: String) String!FsError` | Read UTF-8 text (`read_bytes` + decode) |
| `write_text` | `fn(path: String, content: String) !FsError` | Write string to file |
| `write_bytes` | `fn(path: String, bytes: Vector<Byte>) !FsError` | Write bytes to file |
| `mkdirp` | `fn(path: String) !FsError` | Create directory (and parents) |
| `list_dir` | `fn(path: String) Vector<DirEntry>!FsError` | List directory entries |
| `exists` | `fn(path: String) Bool` | Check if path exists |

### `@std.proc`

Process and environment.

| Function | Signature | Description |
|----------|-----------|-------------|
| `args` | `fn() Vector<String>` | Command-line arguments |
| `env` | `fn(name: String) Option<String>` | Environment variable lookup |
| `cwd` | `fn() String` | Current working directory |
| `exit` | `fn(code: Int)` | Exit process (never returns) |

### `@std.date`

Timing utilities.

| Function | Signature | Description |
|----------|-----------|-------------|
| `now` | `fn() Float` | Current time as milliseconds since the time origin (`performance.now()` in Node/browser; ms since Unix epoch in the interpreter) |

### `@std.view`

A read-only, zero-copy window `View<C>` over any `IndexRead` backing (a `Vector`,
a `String`, or another `View`). Element reads delegate through the contract, so
`at` is a direct backing read plus an integer add; the window ops are O(1) and
share the one backing. A `View` itself satisfies `IndexRead<E>`. Requires
`use @std.view` (and `use @std.view.{View}` to name the type). The element type
`E` follows from the backing via the functional dependency. See
[plans/view.md](plans/view.md).

| Function | Signature | Description |
|----------|-----------|-------------|
| `view.from(c)` | `fn<C: IndexRead<E>, E>(c: C) View<C>` | Wrap a whole backing in a window (O(1)) |
| `.len()` | `fn<C>(v: View<C>) Int` | Number of elements in the window |
| `.is_empty()` | `fn<C>(v: View<C>) Bool` | True when the window has no elements |
| `.at(i)` | `fn<C: IndexRead<E>, E>(v: View<C>, i: Int) E` | Element at window-relative index (traps OOB); backs `IndexRead` |
| `.first()` | `fn<C: IndexRead<E>, E>(v: View<C>) Option<E>` | First element, or `.None` if empty |
| `.last()` | `fn<C: IndexRead<E>, E>(v: View<C>) Option<E>` | Last element, or `.None` if empty |
| `.drop_first()` | `fn<C>(v: View<C>) View<C>` | Drop the first element (O(1), shares backing; total) |
| `.drop_last()` | `fn<C>(v: View<C>) View<C>` | Drop the last element (O(1), shares backing; total) |
| `.sub(a, b)` | `fn<C>(v: View<C>, a: Int, b: Int) View<C>` | Relative sub-window `[a, b)` (O(1), shares backing) |
| `.to_vector()` | `fn<C: IndexRead<E>, E>(v: View<C>) Vector<E>` | Materialize the window into an owned `Vector` |
| `.fold(init, f)` | `fn<C: IndexRead<E>, E, B>(v: View<C>, init: B, f: fn(B, E) B) B` | Left-fold over the window |
