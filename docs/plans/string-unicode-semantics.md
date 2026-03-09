# String Unicode Semantics Plan

**Status:** Proposed  
**Last updated:** 2026-03-09

## Goal

Make `String` behavior consistent and predictable across interpreter and Wasm by
defining and implementing a single language-level model:

- Twinkle strings are valid UTF-8.
- String indexing/iteration operate on Unicode scalar values, not raw bytes.
- Byte-level operations are explicit APIs, not implicit language behavior.

This plan is focused on language ergonomics and backend parity for self-hosting work.

---

## Problem Statement

Today there is a semantic mismatch between interpreter and Wasm for non-ASCII strings:

- Interpreter string operations use Rust `chars()` in key paths, so they are scalar-based.
- Wasm runtime string operations use `array<i8>` length/get/copy directly, so they are byte-based.
- `for c in s` and `s[i]` therefore disagree across backends for non-ASCII input.
- In Wasm, slicing/indexing can split UTF-8 sequences and later fail when host decodes runtime strings.

This mismatch is now user-visible and will become more painful as Twinkle codebases grow.

---

## Language Direction (Normative)

### 1. Core string model

- `String` is a sequence of Unicode scalar values encoded as UTF-8.
- All observable string APIs must preserve valid UTF-8 invariants.

### 2. Iteration and indexing semantics

- `for c in s` iterates scalar values.
- `c` has type `String` and represents one scalar value.
- `s[i]` indexes by scalar index and traps on OOB.
- `String.get(s, i)` indexes by scalar index and returns `Option<String>`.
- `String.substring(s, start, end)` uses scalar indices `[start, end)`.
- `String.len()` returns scalar count.

### 3. Explicit byte APIs

If byte-level behavior is needed (lexer/parser/compiler internals), it must be explicit:

- Proposed additions:
  - `String.utf8_bytes(s) Vector<Int>`
  - `String.from_utf8(bytes: Vector<Int>) Option<String>`
  - `String.byte_len(s) Int`
- These APIs are intentionally separate from scalar indexing/iteration.

### 4. Non-goals

- Grapheme-cluster semantics (user-perceived characters) are out of scope.
- Unicode normalization/case-folding/collation are out of scope.

---

## Why This Direction

- Matches user expectations for high-level language string loops/indexing.
- Removes interpreter vs Wasm backend behavior drift.
- Keeps low-level byte access available for compiler-style code.
- Prevents invalid UTF-8 strings from being created through ordinary string operations.

---

## API Naming Decision Draft

This section proposes concrete names so implementation can proceed without
repeated naming debates.

### Canonical scalar APIs (recommended)

- `String.len(s) Int` and `s.len()`  
  Scalar count.
- `String.get(s, i) String?` and `s.get(i)`  
  Safe scalar indexing.
- `s[i] String`  
  Trap-on-OOB scalar indexing.
- `String.substring(s, start, end) String` and `s.substring(start, end)`  
  Scalar range slicing.
- `String.iter(s) Iterator<String>` and `s.iter()`  
  Scalar iterator for `for`/`collect` lowering.
- `String.code_point_at(s, i) Int?` and `s.code_point_at(i)`  
  Safe scalar code-point lookup.
- `String.from_code_point(n: Int) String?`  
  Unicode scalar to one-scalar string.

### Canonical byte APIs (recommended)

- `String.byte_len(s) Int`
- `String.utf8_bytes(s) Vector<Int>`
- `String.from_utf8(bytes: Vector<Int>) String?`
- `String.byte_at(s, i) Int?`

These names make byte intent explicit and avoid overloading scalar APIs.

### Compatibility and deprecation policy (recommended)

- Keep `char_code_at` and `from_char_code` temporarily as compatibility aliases.
- Alias mapping:
  - `char_code_at(s, i)` -> trap-on-OOB wrapper over `String.code_point_at`
  - `from_char_code(n)` -> alias to `String.from_code_point(n)`
- Mark both as deprecated in docs once new names land.
- Remove aliases in a later cleanup milestone after compiler/stdlib migration.

### Rationale for naming choices

- `code_point` is unambiguous; `char_code` is historically ambiguous.
- `utf8_bytes` and `from_utf8` are explicit about encoding boundary.
- Keeping `get`/`[]` aligned with scalar semantics preserves ergonomic surface.

---

## Implementation Plan

## Phase 0: Semantics Lock and Spec Updates

### Work

- Update spec and API docs to declare scalar semantics and UTF-8 invariants.
- Document byte APIs as explicit low-level operations.
- Add a short compatibility note: previous byte-like behavior for some Wasm paths was unintended.

### Exit criteria

- `docs/spec.md` and `docs/API.md` agree on `len/get/index/substring/for-in` semantics.

---

## Phase 1: Runtime UTF-8 Decode Primitives (Wasm)

### Work

- Add internal runtime helpers to advance by codepoint boundaries over UTF-8 bytes.
- Add helper(s) for scalar count and scalar-range slicing.
- Keep runtime representation as UTF-8 byte array (`array<i8>`), but stop treating byte index as scalar index.

### Exit criteria

- Runtime can:
  - count scalars,
  - slice by scalar range,
  - extract scalar at index,
  without producing invalid UTF-8.

---

## Phase 2: String Iterator Surface

### Work

- Introduce `String.iter(s) -> Iterator<String>`.
- Implement in Wasm and interpreter with identical scalar semantics.
- Add tests for ASCII + non-ASCII (`é`, `你`, `👍`) and mixed strings.

### Exit criteria

- `String.iter` parity tests pass on both backends.

---

## Phase 3: Lower `for c in s` Through Iterator

### Work

- Change lowering of string `for`/`collect` to iterator path instead of `len + index`.
- Keep source syntax unchanged (`for c in s` remains ergonomic).

### Exit criteria

- `for c in s` produces identical output in interpreter and Wasm for Unicode fixtures.

---

## Phase 4: Align String Core APIs

### Work

- Move `String.len`, `String.substring`, `String.get`, and `s[i]` to scalar semantics in Wasm.
- Ensure interpreter matches exactly (including edge cases and traps).
- Introduce canonical scalar naming:
  - `String.code_point_at` and `String.from_code_point`
- Keep `char_code_at`/`from_char_code` as temporary deprecated aliases to the
  new scalar APIs.

### Exit criteria

- No backend divergence on string core API behavior.
- All string fixtures pass in both interpreters with same expected output/trap class.

---

## Phase 5: Add Explicit Byte APIs

### Work

- Add `utf8_bytes`, `from_utf8`, `byte_len` (or equivalent final naming).
- Ensure compiler/stdlib use byte APIs where byte-precise behavior is intended.

### Exit criteria

- No user-facing features depend on implicit byte indexing of `String`.
- Byte operations are explicit and tested.

---

## Testing Strategy

### Fixture matrix

For each relevant API (`for-in`, `collect`, `len`, `get`, `index`, `substring`):

- ASCII-only fixtures.
- Single multibyte scalar fixtures.
- Mixed ASCII + multibyte fixtures.
- Emoji / 4-byte scalar fixtures.
- Boundary/OOB fixtures.

Run each fixture in:

- Core interpreter (`twk run -i`)
- Wasm runtime (`twk run`)

### Invariant tests

- String operations never produce invalid UTF-8.
- Host decode path never fails for valid Twinkle programs that avoid explicit byte APIs.

### Regression tests

- Preserve `and` short-circuit trap behavior.
- Preserve `s[i]` OOB trap behavior.
- Preserve `String.get` safe access behavior.

---

## Risks and Mitigations

- **Performance risk**: scalar indexing over UTF-8 is O(n).  
  Mitigation: document complexity; optimize later with optional indexing caches if needed.

- **Compatibility risk**: existing code assuming byte indexing semantics may change behavior.  
  Mitigation: provide explicit byte APIs and migration notes.

- **Implementation drift risk**: interpreter and Wasm diverge again.  
  Mitigation: dual-backend fixture matrix as required CI path for string tests.

---

## Success Criteria

This plan is successful when all are true:

- `for c in s` and `s[i]` have identical behavior in interpreter and Wasm.
- Non-ASCII strings work without runtime UTF-8 decode failures in normal string operations.
- String semantics are clearly documented as scalar-based.
- Byte-level operations are explicit and intentionally named.

---

## Suggested Execution Order

1. Phase 0 (docs/spec lock)
2. Phase 1 (runtime decode primitives)
3. Phase 2 (String.iter)
4. Phase 3 (`for c in s` lowering to iterator)
5. Phase 4 (core API alignment)
6. Phase 5 (explicit byte APIs and migration cleanup)
