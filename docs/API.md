# Twinkle API Reference

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
| `.unwrap_or(default)` | `fn<T>(opt: Option<T>, default: T) T` | Extract `Some(v)` or return `default` |
| `.map(f)` | `fn<T, U>(opt: Option<T>, f: fn(T) U) Option<U>` | Transform `Some(v)` into `Some(f(v))`; leaves `None` unchanged |
| `.and_then(f)` | `fn<T, U>(opt: Option<T>, f: fn(T) Option<U>) Option<U>` | Chain Option-producing steps without nesting |
| `.ok_or(err)` | `fn<T, E>(opt: T?, err: E) Result<T, E>` | Convert to Result: `Some(v)` → `Ok(v)`, `None` → `Err(err)` |
| `.ok_or_else(f)` | `fn<T, E>(opt: T?, f: fn() E) Result<T, E>` | Lazy variant — `f()` is only called when `opt` is `None` |
| `.transpose()` | `fn<T, E>(opt: Option<Result<T, E>>) Result<Option<T>, E>` | Convert `Option<Result<T,E>>` into `Result<Option<T>,E>` |

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
| `.map(f)` | `fn<T, U, E>(res: Result<T, E>, f: fn(T) U) Result<U, E>` | Transform `Ok(v)` into `Ok(f(v))`; leaves `Err(e)` unchanged |
| `.and_then(f)` | `fn<T, U, E>(res: Result<T, E>, f: fn(T) Result<U, E>) Result<U, E>` | Chain Result-producing steps without nested Results |
| `.transpose()` | `fn<T, E>(res: Result<Option<T>, E>) Option<Result<T, E>>` | Convert `Result<Option<T,E>>` into `Option<Result<T,E>>` |

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
| `String.from_char_code` | `fn(n: Int) Option<String>` | Single-char string from byte value (ASCII range) |
| `String.from_code_point` | `fn(n: Int) Option<String>` | String from Unicode code point (full range) |

Conversion functions can be used as first-class function references (e.g. `nums.map(Int.to_string)`). The dot-call form `.to_string()` also works on values directly.

## Byte

Primitive type representing a single byte (0–255). Returned by string indexing (`s[i]`) and byte iteration (`for b in s`).

| Function | Signature | Description |
|----------|-----------|-------------|
| `Byte.to_int` | `fn(b: Byte) Int` | Convert byte to integer |
| `Byte.from_int` | `fn(n: Int) Option<Byte>` | Convert integer in range 0..255 to `Byte` |
| `Byte.to_string` | `fn(b: Byte) String` | Convert byte to string representation |
| `Byte.compare` | `fn(a: Byte, b: Byte) Order` | Compare two bytes by numeric value |

## String

Strings are immutable, UTF-8 encoded, and GC-managed. String interpolation: `"hello ${name}"`.

**Byte-oriented model:** Lengths, indices, and slicing all operate on **byte offsets**, not characters. Indexing (`s[i]`) returns a `Byte`. This is efficient but means multi-byte UTF-8 characters occupy multiple index positions. Use the Unicode helpers below for character-level operations.

### Core (builtin)

| Method | Signature | Description |
|--------|-----------|-------------|
| `.len()` | `fn(s: String) Int` | Length in **bytes** |
| `s[i]` | — | Byte at byte offset `i` (returns `Byte`, traps on OOB) |
| `.get(i)` | `fn(s: String, i: Int) Option<Byte>` | Safe byte lookup at byte offset |
| `.slice(start, end)` | `fn(s: String, start: Int, end: Int) String` | Substring by **byte offsets** `[start, end)`, traps if indices fall mid-codepoint |
| `.concat(other)` | `fn(s: String, other: String) String` | Concatenate two strings |
| `.char_code_at(i)` | `fn(s: String, i: Int) Int` | Byte value at byte offset `i` (same as `Byte.to_int(s[i])`) |
| `.utf8_bytes()` | `fn(s: String) Vector<Byte>` | Copy string bytes into a vector |
| `String.from_utf8` | `fn(bytes: Vector<Byte>) Option<String>` | Validate UTF-8 and create string |

### Prelude (auto-imported, no import needed)

| Method | Signature | Description |
|--------|-----------|-------------|
| `.index_of(needle)` | `fn(s: String, needle: String) Option<Int>` | First byte offset of `needle` |
| `.contains(needle)` | `fn(s: String, needle: String) Bool` | Whether `s` contains `needle` |
| `.starts_with(prefix)` | `fn(s: String, prefix: String) Bool` | Prefix check |
| `.ends_with(suffix)` | `fn(s: String, suffix: String) Bool` | Suffix check |
| `.split(sep)` | `fn(s: String, sep: String) Vector<String>` | Split on separator (empty sep returns `[s]`) |
| `.trim()` | `fn(s: String) String` | Strip leading/trailing ASCII whitespace |

### Unicode helpers (prelude)

| Method | Signature | Description |
|--------|-----------|-------------|
| `.chars()` | `fn(s: String) Iterator<String>` | Iterate Unicode scalars (each as a 1–4 byte `String`) |
| `.char_len()` | `fn(s: String) Int` | Number of Unicode scalars |
| `.code_point_at(i)` | `fn(s: String, i: Int) Option<Int>` | Code point at **scalar index** `i` (O(n)) |
| `.graphemes()` | `fn(s: String) Iterator<String>` | Iterate extended grapheme clusters (user-perceived characters). Handles combining marks, ZWJ emoji sequences, and regional indicator flags via a simplified UAX #29 implementation. |

**Iteration:** `for b in s { ... }` iterates **bytes** (`b: Byte`). Use `for ch in s.chars() { ... }` to iterate Unicode scalars, or `for g in s.graphemes() { ... }` for user-perceived characters (grapheme clusters).

Qualified forms (`String.len`, `String.trim`, etc.) also work.

## Vector\<T\>

Persistent (copy-on-write) vector. Literal syntax: `[1, 2, 3]`.

| Method | Signature | Description |
|--------|-----------|-------------|
| `.len()` | `fn<T>(v: Vector<T>) Int` | Number of elements |
| `.push(elem)` | `fn<T>(v: Vector<T>, elem: T) Vector<T>` | Append element, return new vector |
| `.get(i)` | `fn<T>(v: Vector<T>, i: Int) Option<T>` | Safe index lookup |
| `.set(i, val)` | `fn<T>(v: Vector<T>, i: Int, val: T) Option<Vector<T>>` | Safe update at index |
| `.concat(other)` | `fn<T>(v: Vector<T>, other: Vector<T>) Vector<T>` | Concatenate two vectors |
| `.slice(start, end)` | `fn<T>(v: Vector<T>, start: Int, end: Int) Vector<T>` | Subvector `[start, end)` |
| `.map(f)` | `fn<A,B>(xs: Vector<A>, f: fn(A) B) Vector<B>` | Transform each element |
| `.filter(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Vector<A>` | Keep elements where `f` returns true |
| `.fold(init, f)` | `fn<A,B>(xs: Vector<A>, init: B, f: fn(B,A) B) B` | Left fold |
| `.find(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Option<A>` | First element matching predicate |
| `.any(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Bool` | True if any element matches |
| `.all(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Bool` | True if all elements match |
| `.contains(elem)` | `fn<A>(xs: Vector<A>, elem: A) Bool` | True if `elem` is in the vector |
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
| `.set(key, value)` | `fn<K,V>(d: Dict<K,V>, key: K, value: V) Dict<K,V>` | Insert/replace key-value pair, return new dict |
| `.len()` | `fn<K,V>(d: Dict<K,V>) Int` | Number of entries |
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

Only non-prelude stdlib modules require explicit imports: `use @std.path`, `use @std.fs`, `use @std.proc`.

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
| `write_bytes` | `fn(path: String, bytes: Vector<Int>) !FsError` | Write bytes to file |
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
