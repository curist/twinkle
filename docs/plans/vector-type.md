# Vector<T>: Sequence Type Design

## Decision

Replace `Array<T>` with a single sequence type `Vector<T>`.

---

## How We Got Here

### Starting point

`Array<T>` was the only sequence type. It had `Array.append` which returned a new array with
an element added. But the underlying wasm `array` type is fixed-size at allocation — `append`
was secretly O(N) copy-per-call, and the name implied O(1) amortized growth. Misleading.

### Round 1: Array + Vector split

First instinct: keep `Array<T>` as fixed-size (no `append`), add `Vector<T>` as the growable
type. Clean separation in principle.

Problem: `arr[i] = val` on Array desugars to `Array.set(arr, i, val)` which is also O(N) COW.
This defeats the stated purpose of Array as the "fast" type. The `[]` write sugar looks like
O(1) in-place mutation but isn't.

### Round 2: Just Vector

Remove Array entirely. But this raised the read/write asymmetry question: if `v[i]` is total
(returns `Option<T>`), what does `v[i] = val` mean? And COW set on Vector is expected — that's
what you signed up for with a growable type.

### Round 3: Array as read-only

Make Array truly read-only after construction — no write sugar, no `Array.set`. But then
Vector already does everything Array does plus growth. No reason to have both.

### Reference check: Gleam, Elm, Clojure

None of these languages expose raw unsafe arrays as the default user-facing type:

- **Elm**: `Array` is a tree structure; `Array.get` returns `Maybe a`.
- **Clojure**: `[1 2 3]` is a bit-partitioned trie (persistent vector); `(get v i)` returns
  nil on OOB (safe); `(nth v i)` throws (unsafe escape hatch).
- **Gleam**: functional collections; no raw mutable array exposed.

Takeaway: "fast raw array as a user type" is not the norm in immutable/functional languages.
The perceived need was premature optimization.

### Conclusion

One type. Consistent with Twinkle's immutable value semantics. Backed by flat COW wasm array
now, RRB tree (bit-partitioned trie) later. Expectation is explicitly set: updates are O(N)
today; the RRB optimization does not change the API.

---

## API

```tw
// Construction
[1, 2, 3]                                // Vector<Int> literal
Vector.make(size: Int, fill: T) Vector<T> // fixed-size init with fill value

// Growth
v.push(elem) Vector<T>                   // returns new Vector

// Length
v.len() Int

// Unsafe access — OOB panics
v[i]                                     // T
v[i] = val                               // desugars to v = Vector.set_unsafe(v, i, val)

// Safe access — total
v.get(i) Option<T>                       // None if OOB
v.set(i, val) Option<Vector<T>>          // None if OOB

// Bulk operations
v.concat(other) Vector<T>
v.slice(start, end) Vector<T>
```

`[]` is the "I know what I'm doing" fast path. `.get`/`.set` are the safe path.

`v.set` returns `Option<Vector<T>>` rather than `Result<Vector<T>, String>` because:
- OOB is the only failure mode, so `None` is unambiguous.
- `String` errors are appropriate for diagnostics, not core container APIs.
- A structured `IndexError` type adds overhead for a single failure case.

---

## collect

`collect` continues to produce `Vector<T>`:

```tw
squares := collect x in range(10) { x * x }  // Vector<Int>
```

Internally uses the mutable builder intrinsics (allocated as a mutable wasm array, filled
during iteration, then frozen). This makes N pushes cost O(N) total, not O(N²).

---

## Dict and String — unchanged

- `m[k]` → `Option<V>` (total, as before)
- `m[k] = v` → desugars to `m = Dict.set(m, k, v)` (as before)
- `s[i]` → single-char `String`, OOB panics (as before)

---

## TypeId spacing

User types start at `TypeId(256)` to leave room for future built-in types.

Built-in TypeIds (0–255):

| TypeId | Type |
|--------|------|
| 0 | Option |
| 1 | Result |
| 2 | Cell |
| 3 | Range |
| 4 | Iterator |
| 5 | IterItem |
| 6 | UnfoldStep |
| 7 | Vector |
| 8–255 | reserved for future built-ins |
| 256+ | user-defined types |

---

## Implementation plan

1. Add `MonoType::Vector(Box<MonoType>)` to `src/types/ty.rs`; assign `TypeId(7)`; shift user
   types to `TypeId(256)`.
2. Update `src/types/check.rs`: remove `Array` methods (`append`, `set`, `concat`, `slice`);
   add `synth_vector_call` and `synth_method_call` cases for `MonoType::Vector`.
3. Update `src/ir/lower.rs`: add Vector prelude FuncIds; update method dispatch; update
   `collect` to produce `Vector<T>`; shift user type TypeId start to 256.
4. Update `src/codegen/prelude.rs`: add Vector prelude entries; remove `Array.append` /
   `Array.set`; update `Array.concat` → `Vector.concat` etc.
5. Rename `src/runtime/arr.rs` operations to serve Vector semantics; expose `make` as
   `Vector.make`.
6. Update `src/interp/eval.rs`: rename `Value::Arr` → `Value::Vec` (or keep internal name,
   update surface); update all call_builtin cases.
7. Update spec `docs/spec.md` section 14: remove Array, add Vector.
8. Update tests: `tests/run/arrays.tw` → `tests/run/vectors.tw`; update all references.

The wasm `array` type remains the runtime backing — it is an implementation detail not visible
to users.
