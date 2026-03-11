# Byte-First String Semantics Plan

**Status:** Complete
**Last updated:** 2026-03-10

## Goal

Define a coherent byte-first string model for Twinkle:

- `String` remains always-valid UTF-8
- Core `String` operations are byte-based (len, index, slice, iteration)
- Unicode/code-point traversal is explicit via `chars()`
- `Byte` is a dedicated primitive type used by byte-oriented APIs

This keeps the language explicit and predictable while preserving safe text invariants.

---

## Problem Statement

Today there is a semantic mismatch between interpreter and Wasm for non-ASCII strings:

- Interpreter string operations use Rust `chars()` in key paths, so they are scalar-based.
- Wasm runtime string operations use `array<i8>` length/get/copy directly, so they are byte-based.
- `for c in s` and `s[i]` therefore disagree across backends for non-ASCII input.
- In Wasm, slicing/indexing can split UTF-8 sequences and later fail when host decodes runtime strings.

The byte-first direction resolves this by making the Wasm behavior the canonical model
and adding boundary checks to prevent invalid UTF-8 production.

---

## Language Direction (Normative)

### 1. `Byte` primitive type

`Byte` is a primitive value type representing an unsigned 8-bit value (range `0..255`).
It is distinct from `Int`.

**Purpose:** byte-oriented APIs including string byte indexing, UTF-8 conversion,
file/network data, and compiler internals. It is not the start of a larger integer
tower — it is a focused primitive for raw 8-bit data.

**Conversions:**

```tw
Byte.to_int(b: Byte) Int       // always succeeds
Byte.from_int(n: Int) Byte?    // None if n outside 0..255
```

**Arithmetic:** operations on `Byte` promote to `Int`. This avoids wraparound
surprises and keeps `Byte` primarily a data-unit type.

```tw
b1 + b2    // Int
b + 1      // Int
```

**Examples:**

```tw
b := Byte.from_int(0xFF)?   // Byte
i := Byte.to_int(b)         // 255
b + 1                        // Int
```

### 2. Core string model

- `String` is an immutable sequence of bytes that is always valid UTF-8.
- The primary observable sequence unit of `String` is the **byte**.
- `len`, `get`, `[]`, `for in`, and slicing are defined in terms of byte offsets.
- Unicode scalar traversal is provided through explicit APIs (`chars()`).

**UTF-8 validity invariant:** The following operations always produce valid UTF-8:
string literals, concatenation, `slice` (with boundary check), `from_code_point`,
`chars()` reconstruction. The only fallible entry point from raw bytes is `from_utf8`,
which returns `Option<String>`. The runtime must never construct invalid UTF-8
through any string operation.

### 3. Non-goals

- Grapheme-cluster semantics (user-perceived characters).
- Unicode normalization, case-folding, locale-aware collation.
- Arbitrary invalid UTF-8 string values.

---

## API Surface

### Core byte-based APIs

**Length:**

```tw
String.len(s: String) Int     // UTF-8 byte length, O(1)
s.len() Int
```

Examples: `"abc".len() == 3`, `"é".len() == 2`, `"你".len() == 3`, `"👍".len() == 4`.

**Indexing:**

```tw
s[i] Byte                         // byte offset, traps on OOB
String.get(s: String, i: Int) Byte?   // byte offset, None on OOB
s.get(i) Byte?
```

Examples: `"é"[0] == 0xC3`, `"é"[1] == 0xA9`.

**Iteration:**

```tw
for b in s { ... }    // b: Byte, iterates UTF-8 bytes
```

When the iterable expression has type `String`, the compiler lowers iteration to
a byte-by-byte walk over the string's UTF-8 storage. This is a normative lowering
rule — `for b in s` must not implicitly call `chars()`.

**Slicing:**

```tw
String.slice(s: String, start: Int, end: Int) String
s.slice(start, end) String
```

- `start` and `end` are byte offsets, range is `[start, end)`.
- Traps if indices are out of bounds.
- Traps if `start` or `end` is not on a valid UTF-8 scalar boundary.
- Returns a new `String` containing the specified byte range (always a copy, not a view).

**UTF-8 scalar boundary definition:** A byte offset is a scalar boundary if it is 0,
equal to the byte length of the string, or points to a byte that is not a UTF-8
continuation byte (`10xxxxxx`). Equivalently, it is the start of a UTF-8 encoded
scalar value or one past the end.

Examples: `"é".slice(0, 2)` succeeds; `"é".slice(0, 1)` traps (invalid boundary).

### Explicit Unicode APIs

**Scalar iteration:**

```tw
String.chars(s: String) Iterator<String>
s.chars() Iterator<String>
```

Iterates Unicode scalar values in decode order. Each yielded element is a one-scalar
`String` value. Implementations may allocate new string values for each element.

```tw
for ch in "aé你👍".chars() {
  // yields "a", "é", "你", "👍"
}
```

The iterator maintains an internal byte offset, advancing by the UTF-8 byte length
of each decoded scalar. This ensures each `.next()` is O(1) amortized.

**Optional scalar helpers:**

```tw
String.char_len(s: String) Int              // Unicode scalar count, O(n)
String.code_point_at(s: String, i: Int) Int?  // scalar index (not byte index), O(n)
String.from_code_point(n: Int) String?      // None for invalid scalar values
```

Note: `code_point_at` uses a **scalar index**, not a byte index. Docs must clearly
distinguish byte-index APIs from scalar-index APIs.

### UTF-8 conversion boundary

```tw
String.utf8_bytes(s: String) Vector<Byte>
String.from_utf8(bytes: Vector<Byte>) String?
```

`utf8_bytes` returns a copy of the UTF-8 byte sequence (not a view into the string's
internal storage). `from_utf8` is the explicit checked entry point from raw bytes to
`String`.

### Compatibility and deprecation

- Keep `char_code_at` and `from_char_code` temporarily as aliases.
- Mark both as deprecated once new names land.
- Remove in a later cleanup milestone after compiler/stdlib migration.

---

## Complexity Model

### Byte-oriented (predictable, fast)

| API | Complexity |
|-----|-----------|
| `len` | O(1) |
| `s[i]` / `get` | O(1) |
| `for b in s` | O(n) over byte length |
| `slice` | O(k) copy of sliced bytes (k = end - start) |

### Unicode-oriented (explicit, may decode)

| API | Complexity |
|-----|-----------|
| `chars()` | O(n) over byte length |
| `char_len` | O(n) |
| `code_point_at` | O(n) unless optimized |

This split is intentional: Unicode-aware costs are visible in the API surface.

---

## Implementation Plan

### Phase 0: Semantics Lock and Spec Updates

**Work:**

- Update `docs/spec.md` and `docs/API.md` to declare byte-first semantics and UTF-8 invariants.
- Document `Byte` type in the spec.
- Add compatibility note: previous scalar-based behavior in the interpreter was the
  unintended side; byte-based Wasm behavior is now canonical.

**Exit criteria:** spec and API docs agree on byte-based `len/get/index/slice/for-in` semantics.

---

### Phase 1: `Byte` Primitive Type

**Work:**

- Add `Byte` to the type system (`MonoType::Byte`).
- Wasm representation: `i32` (same as `Bool`).
- Interpreter representation: `Value::Byte(u8)`.
- Implement `Byte.to_int`, `Byte.from_int`.
- Implement arithmetic promotion: `Byte + Byte -> Int`, `Byte + Int -> Int`, etc.

**Exit criteria:** `Byte` type works end-to-end in both backends with conversion and arithmetic.

---

### Phase 2: Byte-Based String Core APIs

**Work:**

- Change `String.len` to return byte length (interpreter currently returns scalar count).
- Change `s[i]` and `String.get` to return `Byte` at byte offset.
- Introduce `String.slice` with UTF-8 boundary validation (traps on invalid boundary).
- Update `for b in s` to iterate bytes (`b: Byte`).

**Exit criteria:**

- `len`, `get`, `[]`, `slice`, `for-in` are byte-based in both backends.
- `slice` traps on non-boundary indices.
- Dual-backend fixture matrix passes.

---

### Phase 3: `String.chars()` Unicode Iterator

**Work:**

- Implement `String.chars(s) -> Iterator<String>` in both backends.
- Iterator state is a byte offset; each `.next()` decodes one scalar and advances.
- Add `String.char_len` as scalar count helper.
- Add tests for ASCII, multi-byte (`é`, `你`, `👍`), mixed strings.

**Exit criteria:** `chars()` parity tests pass on both backends.

---

### Phase 4: Scalar Helper APIs

**Work:**

- Add `String.code_point_at(s, i)` (scalar index, not byte index) and `String.from_code_point(n)`.
- Migrate `char_code_at` / `from_char_code` as deprecated aliases.
- Ensure docs clearly distinguish byte-index vs scalar-index APIs.

**Exit criteria:** scalar helpers work correctly; deprecated aliases mapped.

---

### Phase 5: UTF-8 Conversion APIs

**Work:**

- Add `String.utf8_bytes(s) -> Vector<Byte>`.
- Add `String.from_utf8(bytes: Vector<Byte>) -> String?`.
- Ensure compiler/stdlib use byte APIs where byte-precise behavior is intended.

**Exit criteria:** explicit byte-to-string boundary tested in both backends.

---

## Testing Strategy

### Fixture matrix

For each relevant API (`for-in`, `len`, `get`, `[]`, `slice`, `chars`, `char_len`):

- ASCII-only (`"abc"`).
- Single multi-byte scalar (`"é"`, `"你"`, `"👍"`).
- Mixed ASCII + multi-byte (`"aé你👍"`).
- Boundary/OOB cases.
- `slice` boundary validation (valid and invalid split points).

Run each fixture in both backends:

- Core interpreter (`twk run -i`)
- Wasm runtime (`twk run`)

### Invariant tests

- String operations never produce invalid UTF-8.
- `slice` traps on non-boundary indices, never silently produces invalid UTF-8.
- Host decode path never fails for valid Twinkle programs.

### Regression tests

- Preserve `and` short-circuit trap behavior.
- Preserve `s[i]` OOB trap behavior.
- Preserve `String.get` safe access behavior.

---

## Risks and Mitigations

- **Breaking change:** `len` and `for-in` semantics change for existing code.
  Mitigation: this is pre-1.0; document the change; existing codebase is small.

- **`slice` boundary traps:** users may find it surprising that `s.slice(0, 1)` traps
  for multi-byte strings. Mitigation: clear docs and error messages; `chars()` is the
  obvious alternative for character-level work.

- **Implementation drift risk:** interpreter and Wasm diverge again.
  Mitigation: dual-backend fixture matrix as required CI path for string tests.

---

## Success Criteria

This plan is successful when:

- `String` core operations (`len`, `[]`, `get`, `slice`, `for-in`) are byte-based in both backends.
- `chars()` provides explicit Unicode scalar iteration with identical behavior across backends.
- `Byte` is a working primitive type.
- No ordinary string operation can produce invalid UTF-8.
- String semantics are clearly documented as byte-first with explicit Unicode APIs.
