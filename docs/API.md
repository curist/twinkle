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

### `Task<T>`

Cooperative task handle. Tasks run on the same program thread and switch only at explicit task points (`await`, `yield`) or task-aware host operations such as `time.sleep` / stdin reads; they are not CPU-parallel.

| Function | Signature | Description |
|----------|-----------|-------------|
| `Task.spawn` | `fn<T>(f: fn() T) Task<T>` | Start `f` as a task and return a handle for its eventual result |
| `Task.await` | `fn<T>(task: Task<T>) T` | Suspend until `task` completes, then return its result; propagates a task failure as a trap |
| `Task.yield` | `fn() Void` | Yield control to the scheduler so another runnable task can make progress |

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
| `Order.to_string` | `fn(o: Order) String` | Render as `"Lt"`/`"Eq"`/`"Gt"` |

`Order` satisfies the `Stringify` contract, so values interpolate (`"${o}"`) and
work with generic helpers that require `Stringify` (e.g. `assert.equal`).

Comparators can be passed directly as function references:
```tw
nums.sort_by(Int.compare)
names.sort_by(String.compare)
```

## Numeric (`Int`)

| Function | Signature | Description |
|----------|-----------|-------------|
| `Int.min` | `fn(a: Int, b: Int) Int` | The smaller of two integers |
| `Int.max` | `fn(a: Int, b: Int) Int` | The larger of two integers |
| `Int.clamp` | `fn(n: Int, lo: Int, hi: Int) Int` | Clamp `n` into the inclusive range `[lo, hi]` (assumes `lo <= hi`) |

```tw
lo.max(0).min(width)   // clamp via chained dot-calls
i.clamp(0, xs.len())   // or directly
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
| `.slice(start, end)` | `fn(s: String, start: Int, end: Int) String` | Substring by **byte offsets** `[start, end)`. Out-of-range indices are clamped to `[0, len]`. Traps if a clamped index falls mid-codepoint. Also written `s[start..end]` (the `Sliceable` contract) |
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
| `.slice(start, end)` | `fn<T>(v: Vector<T>, start: Int, end: Int) Vector<T>` | Subvector `[start, end)`. O(log n), sharing the source tree except boundary spines. Also written `v[start..end]` (the `Sliceable` contract) |
| `.gather(idx)` | `fn<T>(xs: Vector<T>, idx: Vector<Int>) Vector<T>` | Bulk index: `result[k] = xs[idx[k]]`, length `idx.len()`. Traps on OOB index. Permute/select/duplicate in one op |
| `.map(f)` | `fn<A,B>(xs: Vector<A>, f: fn(A) B) Vector<B>` | Transform each element |
| `.filter(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Vector<A>` | Keep elements where `f` returns true |
| `.fold(init, f)` | `fn<A,B>(xs: Vector<A>, init: B, f: fn(B,A) B) B` | Left fold |
| `.first()` | `fn<A>(xs: Vector<A>) Option<A>` | First element, or `.None` if empty |
| `.last()` | `fn<A>(xs: Vector<A>) Option<A>` | Last element, or `.None` if empty |
| `.drop_first()` | `fn<A>(xs: Vector<A>) Vector<A>` | Vector without its first element, or empty if already empty. O(log n) via structural slice |
| `.drop_last()` | `fn<A>(xs: Vector<A>) Vector<A>` | Vector without its last element, or empty if already empty. O(log n) worst-case, O(1) when shrinking the tail |
| `.take(n)` | `fn<A>(xs: Vector<A>, n: Int) Vector<A>` | First `n` elements; clamps negative counts to zero and too-large counts to length |
| `.drop(n)` | `fn<A>(xs: Vector<A>, n: Int) Vector<A>` | Skip first `n` elements; clamps negative counts to zero and too-large counts to length |
| `.take_while(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Vector<A>` | Longest prefix whose elements satisfy `f` |
| `.drop_while(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Vector<A>` | Suffix after the prefix whose elements satisfy `f` |
| `.find_map(f)` | `fn<A,B>(xs: Vector<A>, f: fn(A) Option<B>) Option<B>` | First `.Some` produced by `f`, short-circuiting |
| `.count_where(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Int` | Count elements satisfying `f` |
| `.zip_with(other, f)` | `fn<A,B,C>(a: Vector<A>, b: Vector<B>, f: fn(A,B) C) Vector<C>` | Combine elementwise, stopping at the shorter input |
| `.chunks(size)` | `fn<T>(xs: Vector<T>, size: Int) Vector<View<Vector<T>>>` | Non-overlapping zero-copy windows over the vector; invalid sizes return empty |
| `.windows(size)` | `fn<T>(xs: Vector<T>, size: Int) Vector<View<Vector<T>>>` | Sliding zero-copy windows over the vector; invalid or too-large sizes return empty |
| `.find(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Option<A>` | First element matching predicate |
| `.any(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Bool` | True if any element matches |
| `.all(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Bool` | True if all elements match |
| `.contains(elem)` | `fn<A>(xs: Vector<A>, elem: A) Bool` | True if `elem` is in the vector |
| `.position(f)` | `fn<A>(xs: Vector<A>, f: fn(A) Bool) Option<Int>` | Index of first element matching predicate |
| `.flat_map(f)` | `fn<A,B>(xs: Vector<A>, f: fn(A) Vector<B>) Vector<B>` | Map each element to a vector and flatten |
| `.compact()` | `fn<A>(xs: Vector<Option<A>>) Vector<A>` | Drop `.None` entries and unwrap `.Some` values |
| `.dedup()` | `fn<A: Eq>(xs: Vector<A>) Vector<A>` | Remove adjacent duplicate elements |
| `.intersperse(sep)` | `fn<A>(xs: Vector<A>, sep: A) Vector<A>` | Insert `sep` between elements |
| `.reverse()` | `fn<A>(xs: Vector<A>) Vector<A>` | Reverse order |
| `.sort_by(cmp)` | `fn<T>(xs: Vector<T>, cmp: fn(T,T) Order) Vector<T>` | Return a new sorted vector using comparator (e.g. `xs.sort_by(Int.compare)`) |
| `.join(sep)` | `fn(xs: Vector<String>, sep: String) String` | Join strings with separator |
| `Vector.make` | `fn<T>(size: Int, fill: T) Vector<T>` | Create vector of `size` filled with `fill` |

**Indexing syntax:** `v[i]` — unsafe, traps on out-of-bounds.

**Index assignment:** `v[i] = x` — sets element at index `i`.

Vectors are iterable: `for x in v { ... }` and `for x, i in v { ... }`.
Qualified forms (`Vector.map`, `Vector.filter`, etc.) also work.

## `@std.queue`

Persistent double-ended queue. Import with `use @std.queue` for constructors and
qualified calls, and `use @std.queue.{Queue}` when naming the type directly.
The queue is immutable: push and pop operations return a new queue while sharing
structure where possible.

`Queue<T>` is designed for efficient operations at both ends. It uses vectors
internally and normally pushes/pops against vector tails; when one side empties,
it rebalances by reversing/splitting the remaining elements. Prefer conservative
amortized-performance assumptions until benchmark data is available. For tiny
collections, raw `Vector<T>` may still be faster due to lower overhead.

| Function / Method | Signature | Description |
|-------------------|-----------|-------------|
| `queue.new()` | `fn<T>() Queue<T>` | Create an empty queue |
| `queue.singleton(value)` | `fn<T>(value: T) Queue<T>` | Create a queue with one value |
| `queue.from_vector(xs)` | `fn<T>(xs: Vector<T>) Queue<T>` | Build a queue preserving vector order |
| `.to_vector()` | `fn<T>(q: Queue<T>) Vector<T>` | Materialize values from front to back |
| `.len()` | `fn<T>(q: Queue<T>) Int` | Number of values |
| `.is_empty()` | `fn<T>(q: Queue<T>) Bool` | Whether the queue has no values |
| `.push_front(value)` | `fn<T>(q: Queue<T>, value: T) Queue<T>` | Return a queue with `value` added at the front |
| `.push_back(value)` | `fn<T>(q: Queue<T>, value: T) Queue<T>` | Return a queue with `value` added at the back |
| `.peek_front()` | `fn<T>(q: Queue<T>) Option<T>` | Front value without removing it |
| `.peek_back()` | `fn<T>(q: Queue<T>) Option<T>` | Back value without removing it |
| `.pop_front()` | `fn<T>(q: Queue<T>) Option<Pop<T>>` | Remove the front value, returning `{ value, rest }` |
| `.pop_back()` | `fn<T>(q: Queue<T>) Option<Pop<T>>` | Remove the back value, returning `{ value, rest }` |

Qualified forms (`queue.push_back(q, x)`) and method-call forms
(`q.push_back(x)`) both work.

## `@std.heap`

Persistent priority queue, implemented as a pairing heap. Import with
`use @std.heap` for constructors and qualified calls, and
`use @std.heap.{Heap}` when naming the type directly. The heap is immutable:
every operation returns a new heap while sharing structure where possible.

Priority is defined by a comparator stored in the heap. The value for which the
comparator returns `.Lt` against all others is dequeued first, so a comparator
of `Int.compare` gives a **min-heap** and a reversed comparator
(`fn(a, b) Order { b.compare(a) }`) gives a **max-heap**. `new_ord` is a
shorthand for the `Ord`-contract min-heap. Because the comparator rides with the
heap, `push` / `pop` / `merge` take no comparator argument; only mix heaps built
with the same ordering.

Performance is amortized: `push` and `merge` are O(1), `pop` is O(log n).
The `boot/bench/heap_*` benchmarks confirm both a build-and-drain heapsort and a
mixed push/pop workload scale as n·log n with no quadratic blowup. For a
*one-shot* sort, `Vector.sort_by` is ~3–5× faster than build-then-drain; reach
for the heap when priorities arrive incrementally or you only need the top few.

| Function / Method | Signature | Description |
|-------------------|-----------|-------------|
| `heap.new(cmp)` | `fn<T>(cmp: fn(T, T) Order) Heap<T>` | Empty heap ordered by `cmp` |
| `heap.new_ord()` | `fn<T: Ord>() Heap<T>` | Empty min-heap using the `Ord` contract |
| `heap.singleton(value, cmp)` | `fn<T>(value: T, cmp: fn(T, T) Order) Heap<T>` | Heap of one value |
| `heap.from_vector(xs, cmp)` | `fn<T>(xs: Vector<T>, cmp: fn(T, T) Order) Heap<T>` | Build a heap from a vector |
| `.to_vector()` | `fn<T>(h: Heap<T>) Vector<T>` | Drain into a vector in priority order |
| `.len()` | `fn<T>(h: Heap<T>) Int` | Number of values |
| `.is_empty()` | `fn<T>(h: Heap<T>) Bool` | Whether the heap has no values |
| `.push(value)` | `fn<T>(h: Heap<T>, value: T) Heap<T>` | Return a heap with `value` inserted |
| `.peek()` | `fn<T>(h: Heap<T>) Option<T>` | Highest-priority value without removing it |
| `.pop()` | `fn<T>(h: Heap<T>) Option<Pop<T>>` | Remove the highest-priority value, returning `{ value, rest }` |
| `.merge(other)` | `fn<T>(a: Heap<T>, b: Heap<T>) Heap<T>` | Combine two heaps (uses the left comparator) |

Qualified forms (`heap.push(h, x)`) and method-call forms (`h.push(x)`) both
work.

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

## Set\<K\>

Persistent set backed by a `Dict<K, Void>`. Elements must be `Int`, `String`, or
`Byte` (the same key constraint as `Dict`). Insertion order of first insertion is
preserved, so `to_vector` and iteration are deterministic. No import needed.

| Method | Signature | Description |
|--------|-----------|-------------|
| `Set.new()` | `fn<K>() Set<K>` | Create an empty set |
| `.insert(k)` | `fn<K>(s: Set<K>, k: K) Set<K>` | Return a new set with `k` added (no-op if present) |
| `.remove(k)` | `fn<K>(s: Set<K>, k: K) Set<K>` | Return a new set with `k` removed |
| `.contains(k)` | `fn<K>(s: Set<K>, k: K) Bool` | Whether `k` is a member |
| `.len()` | `fn<K>(s: Set<K>) Int` | Number of elements |
| `.is_empty()` | `fn<K>(s: Set<K>) Bool` | Whether the set has no elements |
| `.to_vector()` | `fn<K>(s: Set<K>) Vector<K>` | Elements as a vector, in insertion order |
| `.iter()` | `fn<K>(s: Set<K>) Iterator<K>` | Iterate elements in insertion order |
| `Set.from_vector(xs)` | `fn<K>(xs: Vector<K>) Set<K>` | Build a set from a vector, deduplicating |
| `.union(other)` | `fn<K>(a: Set<K>, b: Set<K>) Set<K>` | Elements in either set (order: `a` then new from `b`) |
| `.intersection(other)` | `fn<K>(a: Set<K>, b: Set<K>) Set<K>` | Elements in both sets (order from `a`) |
| `.difference(other)` | `fn<K>(a: Set<K>, b: Set<K>) Set<K>` | Elements in `a` but not `b` (order from `a`) |
| `.is_subset(other)` | `fn<K>(a: Set<K>, b: Set<K>) Bool` | Whether every element of `a` is in `b` |

Sets support `==`/`!=` using membership equality, so insertion order does not
affect equality. Sets are directly iterable in insertion order; `.to_vector()`
returns the same order when a materialized vector is needed. Qualified forms
(`Set.union`, etc.) also work.

```tw
seen: Set<Int> = Set.new()
seen = seen.insert(3).insert(7)
seen.contains(7)                   // true
Set.from_vector([1, 1, 2]).len()   // 2
for k in seen { ... }              // iterate directly
```

## Operators

### Arithmetic (`Int` and `Float`)
`+`, `-`, `*`, `/`, `%`, unary `-`

Division by zero traps.

### Comparison
`==`, `!=`, `<`, `<=`, `>`, `>=`

### Logical
`and`, `or`, `!` (prefix not)

### Bitwise (`Int`)
`&` (and), `|` (or), `^` (xor), `<<` (shift left), `>>` (shift right)

Operate on the full 64-bit two's-complement representation. `>>` is an
**arithmetic** shift — it sign-extends, so `-8 >> 1` is `-4`. Precedence follows
C: `&` binds tighter than `^`, which binds tighter than `|`, and all three bind
looser than the shifts. Parenthesize when mixing with comparisons.

```tw
fn single_number(nums: Vector<Int>) Int {
  acc := 0
  for n in nums { acc = acc ^ n }   // pairs cancel, unique value remains
  acc
}
```

### String interpolation
`"text ${expr} more"` — calls `.to_string()` on interpolated expressions.

---

## Standard Library

Everything above (primitives, built-in types, I/O, type conversions, String/Vector/Dict methods, operators) is available as **prelude** — no import needed.

Only non-prelude stdlib modules require explicit imports: `use @std.path`, `use @std.fs`, `use @std.io`, `use @std.proc`, `use @std.date`, `use @std.time`, `use @std.view`, `use @std.math`, `use @std.tuple`, `use @std.regexp`, `use @std.crypto`, `use @std.buffer`.

### `@std.crypto`

Digest, MAC, and binary encoding helpers. The module is an umbrella over pure
Twinkle implementations. MD5 and SHA-1 are legacy digest algorithms, included
for compatibility with old protocols, checksums, and programming puzzles; do not
use them for passwords, signatures, or collision-resistant security decisions.
Prefer SHA-256 or HMAC-SHA-256 for new digest/MAC use cases.

The `*_bytes` digests (and the `String` entry points) route through a transient
linear-memory `Buffer` scratch internally — faster than the functional path even
counting the copy-in, so any program that hashes pulls in a Wasm memory section.
The `*_buf` variants skip the copy when the bytes already live in a reused `Buffer`.

```tw
type Digest = .{ bytes: Vector<Byte> }
```

| Function | Signature | Description |
|----------|-----------|-------------|
| `crypto.md5` | `fn(input: String) Digest` | MD5 digest of a UTF-8 string |
| `crypto.md5_bytes` | `fn(input: Vector<Byte>) Digest` | MD5 digest of raw bytes |
| `crypto.md5_buf` | `fn(input: Buffer) Digest` | MD5 digest of the whole `Buffer` (`buf.len()` bytes); pair with `buffer.from_bytes` to amortize the copy when hashing reused data |
| `crypto.sha1` | `fn(input: String) Digest` | SHA-1 digest of a UTF-8 string |
| `crypto.sha1_bytes` | `fn(input: Vector<Byte>) Digest` | SHA-1 digest of raw bytes |
| `crypto.sha1_buf` | `fn(input: Buffer) Digest` | SHA-1 digest of the whole `Buffer` (`buf.len()` bytes) |
| `crypto.sha256` | `fn(input: String) Digest` | SHA-256 digest of a UTF-8 string |
| `crypto.sha256_bytes` | `fn(input: Vector<Byte>) Digest` | SHA-256 digest of raw bytes |
| `crypto.sha256_buf` | `fn(input: Buffer) Digest` | SHA-256 digest of the whole `Buffer` (`buf.len()` bytes) |
| `crypto.hmac_sha256` | `fn(key: String, message: String) Digest` | HMAC-SHA-256 over UTF-8 key/message strings |
| `crypto.hmac_sha256_bytes` | `fn(key: Vector<Byte>, message: Vector<Byte>) Digest` | HMAC-SHA-256 over raw key/message bytes |
| `crypto.hex_encode` | `fn(bytes: Vector<Byte>) String` | Lowercase hexadecimal encoding |
| `crypto.hex_decode` | `fn(text: String) Result<Vector<Byte>, String>` | Decode uppercase/lowercase hexadecimal text |
| `crypto.base64_encode` | `fn(bytes: Vector<Byte>) String` | Standard Base64 with `=` padding |
| `crypto.base64_decode` | `fn(text: String) Result<Vector<Byte>, String>` | Decode standard padded Base64 |
| `.to_bytes()` | `fn(d: Digest) Vector<Byte>` | Raw digest bytes |
| `.hex()` | `fn(d: Digest) String` | Lowercase hexadecimal representation |
| `.base64()` | `fn(d: Digest) String` | Standard Base64 representation |
| `.to_string()` | `fn(d: Digest) String` | Stringify witness; renders `.hex()` |

```tw
use @std.crypto

digest := crypto.sha256("abc")
println(digest.hex()) // ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
```

### `@std.math`

Math helpers. `Int.min`, `Int.max`, and `Int.clamp` already live in the prelude;
this module adds number-theory, exponent, and floating-point helpers. Call
qualified, e.g. `math.gcd(a, b)`.

The integer helpers are pure Twinkle. The constants are Twinkle literals. The
floating-point helpers bridge to the host's `Math` object via `extern`, so they
require the JS/Deno runner that backs `target/twk run`; those host imports are
tree-shaken away when only the integer helpers and constants are used. JS
`Math` constants are exported with lowercase Twinkle value names.

| Constant | Type | Description |
|----------|------|-------------|
| `e` | `Float` | Euler's number (`Math.E`) |
| `pi` | `Float` | Pi (`Math.PI`) |

| Function | Signature | Description |
|----------|-----------|-------------|
| `abs` | `fn(n: Int) Int` | Absolute integer value (`abs(min_i64)` overflows) |
| `sign` | `fn(n: Int) Int` | `-1`, `0`, or `1` by integer sign |
| `gcd` | `fn(a: Int, b: Int) Int` | Greatest common divisor (non-negative; `gcd(0,0)=0`) |
| `lcm` | `fn(a: Int, b: Int) Int` | Least common multiple (`0` when either is `0`) |
| `pow` | `fn(base: Int, exp: Int) Int` | Integer exponentiation; traps on negative `exp`; `pow(0,0)=1` |
| `isqrt` | `fn(n: Int) Int` | Floor of the integer square root; traps when `n < 0` |
| `abs_float` | `fn(x: Float) Float` | Floating-point absolute value (host `Math.abs`) |
| `sign_float` | `fn(x: Float) Float` | Floating-point sign (host `Math.sign`) |
| `pow_float` | `fn(base: Float, exp: Float) Float` | Floating-point exponentiation (host `Math.pow`) |
| `sqrt` | `fn(x: Float) Float` | Square root (host `Math.sqrt`) |
| `cbrt` | `fn(x: Float) Float` | Cube root (host `Math.cbrt`) |
| `hypot` | `fn(x: Float, y: Float) Float` | Hypotenuse for two values (host `Math.hypot`) |
| `floor` | `fn(x: Float) Float` | Largest integer ≤ `x` (host `Math.floor`) |
| `ceil` | `fn(x: Float) Float` | Smallest integer ≥ `x` (host `Math.ceil`) |
| `round` | `fn(x: Float) Float` | Nearest integer, ties toward `+∞` (host `Math.round`) |
| `trunc` | `fn(x: Float) Float` | Integer part, removing fractional digits (host `Math.trunc`) |
| `fround` | `fn(x: Float) Float` | Round to nearest 32-bit float value (host `Math.fround`) |
| `sin` / `cos` / `tan` | `fn(x: Float) Float` | Trigonometric functions, radians (host `Math.*`) |
| `asin` / `acos` / `atan` | `fn(x: Float) Float` | Inverse trigonometric functions, radians (host `Math.*`) |
| `atan2` | `fn(y: Float, x: Float) Float` | Quadrant-aware arc-tangent (host `Math.atan2`) |
| `sinh` / `cosh` / `tanh` | `fn(x: Float) Float` | Hyperbolic functions (host `Math.*`) |
| `asinh` / `acosh` / `atanh` | `fn(x: Float) Float` | Inverse hyperbolic functions (host `Math.*`) |
| `exp` / `expm1` | `fn(x: Float) Float` | `e^x` and `e^x - 1` (host `Math.*`) |
| `log` / `log1p` | `fn(x: Float) Float` | Natural log and `ln(1+x)` (host `Math.*`) |
| `log2` / `log10` | `fn(x: Float) Float` | Base-2 and base-10 logarithms (host `Math.*`) |
| `random` | `fn() Float` | Pseudorandom number in `[0, 1)` (host `Math.random`) |

```tw
use @std.math

math.gcd(12, 18)       // 6
math.pow(2, 10)        // 1024
math.isqrt(99)         // 9
math.pi                // 3.141592653589793
math.sin(math.pi / 2.0) // 1.0
```

### `@std.regexp`

Pure-Twinkle regular expressions for structured text parsing. Compile once with
`regexp.compile`/`regexp.must` (or `compile_with`/`must_with` for options), then
use the inherent methods on `Regexp`. Matches use Unicode scalar offsets;
captures are 1-based (`group(0)` is the whole match). Matching is linear-time
(Pike VM): there is no backtracking, so patterns never blow up on adversarial
input.

| Type / Function | Signature | Description |
|-----------------|-----------|-------------|
| `RegexError` | `.{ pos: Int, message: String }` | Compile error; `pos` is a scalar offset in the pattern |
| `CompileOptions` | `.{ ignore_case: Bool, multiline: Bool, dotall: Bool }` | Global flags; also expressible inline (`(?i)`/`(?m)`/`(?s)`) |
| `Match` | `.{ start: Int, end: Int, groups: Vector<String?>, group_names: ... }` | Successful match with pre-materialized capture text |
| `regexp.compile(pattern)` | `fn(pattern: String) Result<Regexp, RegexError>` | Compile a pattern (all flags off) |
| `regexp.compile_with(pattern, opts)` | `fn(pattern: String, opts: CompileOptions) Result<Regexp, RegexError>` | Compile with global flags |
| `regexp.must(pattern)` | `fn(pattern: String) Regexp` | Compile or trap with `regexp:${pos}: ...` |
| `regexp.must_with(pattern, opts)` | `fn(pattern: String, opts: CompileOptions) Regexp` | Compile with flags or trap |
| `.test(s)` | `fn(re: Regexp, s: String) Bool` | True if the pattern matches anywhere |
| `.find(s)` | `fn(re: Regexp, s: String) Match?` | Leftmost match, if any |
| `.find_all(s)` | `fn(re: Regexp, s: String) Iterator<Match>` | Non-overlapping matches, including empty-match progress |
| `.replace(s, repl)` | `fn(re: Regexp, s: String, repl: String) String` | Replace the first match |
| `.replace_all(s, repl)` | `fn(re: Regexp, s: String, repl: String) String` | Replace every match |
| `Match.group(i)` | `fn(m: Match, i: Int) String?` | Capture text by index, or `.None` if absent/unmatched |
| `Match.group_named(name)` | `fn(m: Match, name: String) String?` | Capture text by group name, or `.None` |
| `Match.text()` | `fn(m: Match) String` | Whole-match text |

Supported syntax: literals, `.`, character classes and ranges (`[abc]`, `[a-z]`,
`[^...]`), predefined classes (`\d \w \s` and uppercase negations), greedy and
lazy quantifiers (`* + ? {m} {m,} {m,n}`, each with a trailing `?` for the lazy
form), capturing, non-capturing, and named groups, alternation, `^`/`$`, and
escapes (`\n \t \r \f \v \\` and escaped metacharacters). Replacement templates
expand `$0`..`$9`, with `$$` for a literal dollar.

**Flags** are ASCII-only and can be set globally (via `CompileOptions`) or inline:

- `ignore_case` / `(?i)` — case-insensitive matching.
- `multiline` / `(?m)` — `^` also matches after `\n`, `$` also before `\n`.
- `dotall` / `(?s)` — `.` also matches `\n`.

Inline flags apply to the rest of the pattern when leading (`(?im)abc`), or to a
scoped group: `(?i:abc)` enables, `(?-i:abc)` disables, `(?i-m:abc)` combines.
Scoped flags override global options within their extent.

**Named groups** use `(?<name>…)` or `(?P<name>…)`; names must be `snake_case`
(lowercase first) and unique. A named group is also a numbered group in source
order, so `group(1)` and `group_named("id")` can refer to the same capture.

**Not supported** (by design, to keep matching linear-time): backreferences
(`\1`, `\k<name>`), lookaround (`(?=…)`, `(?!…)`, `(?<=…)`, `(?<!…)`), and other
backtracking-engine features (atomic groups, possessive quantifiers, conditional
patterns).

Raw string literals (`r"…"`) avoid doubling backslashes in patterns; both
spellings compile identically (`r"\d+"` is the same pattern as `"\\d+"`).

```tw
use @std.regexp

re := regexp.must(r"(?<n>\d+) (?<color>red|green|blue)")
for m in re.find_all("Game 1: 3 blue, 4 red") {
  n := Int.from_string(m.group_named("n").unwrap()).unwrap()
  color := m.group_named("color").unwrap()
  println("${n} ${color}")
}

// Case-insensitive via options, or inline (?i):
regexp.must_with("error", .{ ignore_case: true, multiline: false, dotall: false }).test("ERROR")  // true
regexp.must("(?i)error").test("ERROR")                                                            // true

// Lazy quantifier stops at the first match:
regexp.must("<.*?>").find("<a><b>").unwrap().text()  // "<a>"

regexp.must(r"mul\((\d+),(\d+)\)").replace_all("mul(2,4)", "$1*$2")  // "2*4"
```

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
| `read_buffer` | `fn(path: String) Buffer!FsError` | Read raw file bytes directly into a linear-memory buffer |
| `read_text` | `fn(path: String) String!FsError` | Read UTF-8 text (`read_bytes` + decode) |
| `write_text` | `fn(path: String, content: String) !FsError` | Write string to file |
| `write_bytes` | `fn(path: String, bytes: Vector<Byte>) !FsError` | Write bytes to file |
| `write_buffer` | `fn(path: String, buf: Buffer) !FsError` | Write bytes directly from a linear-memory buffer |
| `mkdirp` | `fn(path: String) !FsError` | Create directory (and parents) |
| `list_dir` | `fn(path: String) Vector<DirEntry>!FsError` | List directory entries |
| `exists` | `fn(path: String) Bool` | Check if path exists |

### `@std.io`

Standard input and output helpers. Stdin reads may suspend cooperatively under a task-capable async runtime, but they remain I/O APIs rather than `Task` APIs.

| Function | Signature | Description |
|----------|-----------|-------------|
| `read_stdin_chunk` | `fn(max_bytes: Int) Vector<Byte>` | Read up to `max_bytes` from stdin; returns an empty vector at EOF |
| `read_stdin_timeout` | `fn(max_bytes: Int, timeout_ms: Int) Vector<Byte>` | Read up to `max_bytes`, waiting at most `timeout_ms`; returns empty on timeout or EOF |
| `stdin_eof` | `fn() Bool` | True after stdin reaches EOF |
| `write_stdout_bytes` | `fn(bytes: Vector<Byte>) Void` | Write raw bytes to stdout |
| `write_stdout_text` | `fn(text: String) Void` | Write UTF-8 text to stdout |

### `@std.proc`

Process and environment.

| Function | Signature | Description |
|----------|-----------|-------------|
| `args` | `fn() Vector<String>` | Command-line arguments |
| `env` | `fn(name: String) Option<String>` | Environment variable lookup |
| `cwd` | `fn() String` | Current working directory |
| `exit` | `fn(code: Int)` | Exit process (never returns) |

### `@std.date`

Compatibility timing utilities. Prefer `@std.time.now()` for elapsed/runtime timing; `date` is reserved for calendar/date APIs over time.

| Function | Signature | Description |
|----------|-----------|-------------|
| `now` | `fn() Float` | Current time as milliseconds since the time origin (`performance.now()` in Node/browser; ms since Unix epoch in the interpreter) |

### `@std.time`

Runtime timing utilities.

| Function | Signature | Description |
|----------|-----------|-------------|
| `now` | `fn() Float` | Monotonic-ish milliseconds since the runtime time origin |
| `sleep` | `fn(ms: Int) Void` | Suspend for at least `ms` milliseconds under the async/JSPI runtime |

### `@std.view`

A read-only, zero-copy window `View<C>` over any `IndexRead` backing (a `Vector`,
a `String`, or another `View`). Element reads delegate through the contract, so
`at` is a direct backing read plus an integer add; the window ops are O(1) and
share the one backing. A `View` itself satisfies `IndexRead<E>`. Requires
`use @std.view` (and `use @std.view.{View}` to name the type). The element type
`E` follows from the backing via the functional dependency. See
[plans/view.md](plans/archive/view.md).

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
| `.sub(a, b)` | `fn<C>(v: View<C>, a: Int, b: Int) View<C>` | Relative sub-window `[a, b)` (O(1), shares backing; total — endpoints clamp into `[0, len()]`, so out-of-range or reversed args yield a valid/empty window) |
| `.slice(a, b)` | `fn<C>(v: View<C>, a: Int, b: Int) View<C>` | Alias for `.sub`; the `Sliceable` satisfier backing `v[a..b]` |
| `.to_vector()` | `fn<C: IndexRead<E>, E>(v: View<C>) Vector<E>` | Materialize the window into an owned `Vector` |
| `.fold(init, f)` | `fn<C: IndexRead<E>, E, B>(v: View<C>, init: B, f: fn(B, E) B) B` | Left-fold over the window |
| `.take(n)` | `fn<C>(v: View<C>, n: Int) View<C>` | First `n` elements (O(1), shares backing; clamps like `sub`) |
| `.drop(n)` | `fn<C>(v: View<C>, n: Int) View<C>` | Skip first `n` elements (O(1), shares backing; clamps like `sub`) |
| `.take_while(f)` | `fn<C: IndexRead<E>, E>(v: View<C>, f: fn(E) Bool) View<C>` | Prefix whose elements satisfy `f` (O(1) result, shares backing) |
| `.drop_while(f)` | `fn<C: IndexRead<E>, E>(v: View<C>, f: fn(E) Bool) View<C>` | Suffix after the prefix whose elements satisfy `f` (O(1) result, shares backing) |
| `.find_map(f)` | `fn<C: IndexRead<E>, E, U>(v: View<C>, f: fn(E) Option<U>) Option<U>` | First `.Some` produced by `f`, short-circuiting |
| `.count_where(f)` | `fn<C: IndexRead<E>, E>(v: View<C>, f: fn(E) Bool) Int` | Count elements satisfying `f` |
| `.zip_with(other, f)` | `fn<A: IndexRead<EA>, EA, B: IndexRead<EB>, EB, R>(a: View<A>, b: View<B>, f: fn(EA, EB) R) Vector<R>` | Combine elementwise, stopping at the shorter input |
| `.chunks(size)` | `fn<C>(v: View<C>, size: Int) Vector<View<C>>` | Non-overlapping contiguous windows; invalid sizes return empty |
| `.windows(size)` | `fn<C>(v: View<C>, size: Int) Vector<View<C>>` | Sliding contiguous windows; invalid or too-large sizes return empty |
| `.map(f)` | `fn<C: IndexRead<E>, E, U>(v: View<C>, f: fn(E) U) Vector<U>` | Transform each element, materializing |
| `.filter(f)` | `fn<C: IndexRead<E>, E>(v: View<C>, f: fn(E) Bool) Vector<E>` | Keep matching elements, materializing |
| `.find(f)` | `fn<C: IndexRead<E>, E>(v: View<C>, f: fn(E) Bool) Option<E>` | First element matching predicate |
| `.any(f)` | `fn<C: IndexRead<E>, E>(v: View<C>, f: fn(E) Bool) Bool` | True if any element matches |
| `.all(f)` | `fn<C: IndexRead<E>, E>(v: View<C>, f: fn(E) Bool) Bool` | True if all elements match |
| `.position(f)` | `fn<C: IndexRead<E>, E>(v: View<C>, f: fn(E) Bool) Option<Int>` | Index of first matching element |
| `.flat_map(f)` | `fn<C: IndexRead<E>, E, U>(v: View<C>, f: fn(E) Vector<U>) Vector<U>` | Map each element to a vector and flatten |

### `@std.tuple`

Ad-hoc grouping of a few values without declaring a domain record. `Pair<A, B>`
is the common case; `Triple<A, B, C>` is the escalation. Both are nominal
records, so they get conditional structural `==`/`!=` for free when their fields
satisfy `Eq`, and both satisfy `Stringify` (`(a, b)` / `(a, b, c)`). Use a named
record instead when the fields carry meaningful API names.

Like `@std.view`, the full surface is two import lines: `use @std.tuple` for the
`tuple.pair` / `tuple.triple` constructors, and `use @std.tuple.{Pair, Triple}`
to name the types in annotations. `Triple` is transparently re-exported from a
submodule (so each arity can own its `to_string`); you never need to import
`@std.tuple.triple` directly.

| Function | Signature | Description |
|----------|-----------|-------------|
| `tuple.pair(a, b)` | `fn<A, B>(first: A, second: B) Pair<A, B>` | Construct a `Pair` |
| `tuple.triple(a, b, c)` | `fn<A, B, C>(first: A, second: B, third: C) Triple<A, B, C>` | Construct a `Triple` |
| `.swap()` | `fn<A, B>(p: Pair<A, B>) Pair<B, A>` | Swap the two components |
| `.to_string()` | `fn<A: Stringify, B: Stringify>(p: Pair<A, B>) String` | Render as `(a, b)`; backs `Stringify` |
| `.to_string()` | `fn<A: Stringify, B: Stringify, C: Stringify>(t: Triple<A, B, C>) String` | Render as `(a, b, c)`; backs `Stringify` |

Fields are named `first` / `second` (and `third` for `Triple`):

```tw
use @std.tuple
use @std.tuple.{Pair, Triple}

fn pop<T>(stack: Vector<T>) Result<Pair<T, Vector<T>>, String> {
  case stack.last() {
    .Some(v) => .Ok(tuple.pair(v, stack.drop_last())),
    .None => .Err("stack underflow"),
  }
}

top := try pop(stack)
value := top.first
rest := top.second

t: Triple<Int, Int, Bool> = tuple.triple(1, 2, true)
"${t}"   // "(1, 2, true)"
```

### `@std.buffer`

Sandboxed, low-level, manually-managed linear-memory buffers — Twinkle's second
mutate-in-place reference type alongside `Cell`. `Buffer` is an opt-in escape
hatch for workloads where GC-managed `Vector<Byte>` is too slow (e.g. byte codecs,
dense numeric arrays). Correctness — calling `free`, not using after free — is the
**programmer's responsibility, like C**. The only safety floor Wasm provides for
free is that all access stays within linear memory, so the worst case is corrupting
another buffer's bytes or trapping, never an escape from the sandbox. See
[docs/plans/buffer-linear-memory.md](plans/buffer-linear-memory.md) for the
design rationale.

Like `@std.view` and `@std.tuple`, two import lines give the full surface:

```tw
use @std.buffer                                    // buffer.new(n), buf.view_i64(..)
use @std.buffer.{Buffer, U8View, I64View, F64View} // type names for annotations
```

The constructors are module-qualified (`buffer.new`, not `Buffer.new`).

**`Buffer` type:** `pub type Buffer = .{ ptr: Int, size: Int }` — a GC record
whose `ptr` is the linear-memory offset and `size` is the allocation size in
bytes. Both fields are public but raw; treat them as opaque outside `@std.buffer`.

**Lifetime:**

| Function | Signature | Description |
|----------|-----------|-------------|
| `buffer.new(nbytes)` | `fn(nbytes: Int) Buffer` | Allocate an uninitialized region. Traps on a negative or oversized request (linear-memory pointers are 32-bit) |
| `buffer.from_bytes(bytes)` | `fn(bytes: Vector<Byte>) Buffer` | Allocate and copy a byte vector into linear memory |
| `buf.free()` | `fn(b: Buffer) Void` | Release the region. Double-free corrupts allocator bookkeeping |
| `buf.len()` | `fn(b: Buffer) Int` | Byte length of the region |
| `buf.to_bytes()` | `fn(b: Buffer) Vector<Byte>` | Copy the bytes back out into an owned `Vector<Byte>` |

**Raw byte-addressed accessors** (byte `off`, little-endian, unaligned OK, **unchecked against `len`** — only the whole-memory bound traps):

| Function | Signature | Description |
|----------|-----------|-------------|
| `buf.get_u8(off)` | `fn(b: Buffer, off: Int) Byte` | Read one byte at byte offset `off` |
| `buf.set_u8(off, v)` | `fn(b: Buffer, off: Int, v: Byte) Void` | Write one byte |
| `buf.get_u32(off)` | `fn(b: Buffer, off: Int) Int` | Read 4 bytes as an unsigned little-endian word, zero-extended to `Int` |
| `buf.set_u32(off, v)` | `fn(b: Buffer, off: Int, v: Int) Void` | Write the low 32 bits of `v` as a little-endian word |
| `buf.get_i64(off)` | `fn(b: Buffer, off: Int) Int` | Read 8 bytes as a little-endian i64 |
| `buf.set_i64(off, v)` | `fn(b: Buffer, off: Int, v: Int) Void` | Write 8 bytes as a little-endian i64 |
| `buf.get_f64(off)` | `fn(b: Buffer, off: Int) Float` | Read 8 bytes as a little-endian f64 |
| `buf.set_f64(off, v)` | `fn(b: Buffer, off: Int, v: Float) Void` | Write 8 bytes as a little-endian f64 |

**Element-indexed views** (handles over a slice of the buffer; O(1) construction, no extra allocation; element index `i`, **unchecked against `count`**):

| Function | Signature | Description |
|----------|-----------|-------------|
| `buf.view_u8(byte_off, count)` | `fn(b: Buffer, byte_off: Int, count: Int) U8View` | Byte-element view over `[byte_off, byte_off + count)` |
| `buf.view_i64(byte_off, count)` | `fn(b: Buffer, byte_off: Int, count: Int) I64View` | i64-element view; `count` is in elements, `byte_off` in bytes |
| `buf.view_f64(byte_off, count)` | `fn(b: Buffer, byte_off: Int, count: Int) F64View` | f64-element view; `count` is in elements, `byte_off` in bytes |

Each view type (`U8View`, `I64View`, `F64View`) exposes the same interface:

| Method | Element type | Description |
|--------|-------------|-------------|
| `v.len()` | — | Element count |
| `v.at(i)` | `Int` / `Int` / `Float` | Element at index `i` (unchecked) |
| `v.set(i, x)` | — | Write element at index `i` in place (unchecked) |
| `v.slice(lo, hi)` | same view type | Sub-window `[lo, hi)` — O(1), shares backing, endpoints clamped |
| `v.iter()` | `Iterator<Int>` / `Iterator<Int>` / `Iterator<Float>` | Iterate elements for `for x in v.iter()` |

`U8View` elements are raw byte values in the `Int` domain (0–255), avoiding the
`Byte.from_int` round-trip in hot loops. `I64View` elements are `Int`; `F64View`
elements are `Float`.

```tw
use @std.buffer
use @std.buffer.{Buffer, I64View}

// alloc, write, read back, free
buf := buffer.new(64)
v := buf.view_i64(0, 8)   // 8-element i64 window over the first 64 bytes
i := 0
for i < 8 {
  v.set(i, i * 10)
  i = i + 1
}
total := 0
for x in v.iter() { total = total + x }
println("${total}")   // 280
buf.free()
```

**Manual-lifetime caveat.** There is no automatic `free`, no use-after-free
detection, and no double-free guard. These are programmer responsibilities. Inside a
function or block body the idiomatic pattern is `defer buf.free()` at the allocation
site (a `defer` runs when its enclosing `{ }` block exits); a top-level script scope
has no enclosing block, so there you must call `buf.free()` explicitly. A freed region
may be reused by the next `buffer.new` call (address-ordered free-list coalescing), so
use-after-free silently reads or corrupts the next allocation.
