# Open Questions

This file tracks design questions that are still worth revisiting. Resolved items
should move into the spec or a design note rather than staying here as stale
concerns.

---

## 1. Rebinding syntax and mutation-shaped code

Twinkle intentionally uses assignment-like syntax for rebinding and value updates:

```tw
state.items = .append(item)
```

This keeps persistent-data transformations concise, but it can look like shared
mutation to programmers coming from OO/imperative languages.

**Current direction:** keep the syntax. The language model is value semantics:
updates rebuild and rebind the local root, and aliases keep seeing the old value.
`twk lint` now backs this with the `direct-rebinding` and `record-copy-helper`
rules (rebind the field/index path directly instead of writing `with_*` copy
helpers), plus `unused-must-use` for ignored `Result`/`Option` values.

**Remaining tooling question:** whether to add value-flow lints that go beyond the
current syntactic rules. Candidates that are *not* implemented yet:

- updating a value and then never reading or returning the updated binding
  (dead-store detection),
- field/index update in a statement position whose result is effectively ignored,
- suspicious aliasing patterns where code appears to expect another name to observe
  the update.

These need dataflow, not just AST shape, so they are deferred until there is
evidence they catch real mistakes.

---

## 2. Nominal records and code reuse

Twinkle records are nominal. Two record types with the same fields are still
different types, and a function cannot currently say “anything with a `name`
field”.

This keeps type identity, method resolution, and Wasm GC lowering simple, but it
can make some reusable record-field helpers awkward.

**Open question:** should Twinkle eventually add a lightweight mechanism for
field-polymorphic code?

Possible directions:

- keep nominal records only and rely on module APIs/capability records,
- add limited row-polymorphic functions,
- add explicit projection/conversion helpers,
- add a separate structural record feature for local/internal use.

This needs more thought, especially in relation to method resolution and Wasm GC
record layout.

---

## 3. Resource ownership beyond `defer`

`defer` is implemented with block-scoped, LIFO semantics and covers ordinary
manual cleanup well. It fires on normal block exit, `return`, `break`, and
`continue`; traps do not drain defers.

The remaining question is stronger ownership guarantees for external resources
such as file handles, sockets, or host objects.

**Open question:** should Twinkle add linear/unique types, or another ownership
mechanism, for resources that must be closed exactly once?

Without such a mechanism, APIs can still be written safely by convention, but the
compiler cannot prove that resource handles are not duplicated, forgotten, or used
after close.

---

## 4. FFI beyond phase-1 externs

Twinkle supports `extern` declarations for host-provided Wasm imports. Phase-1
boundary types are intentionally small: `Int`, `Float`, `Bool`, `String`, and
`Void`/`()`, plus opaque non-null extern handles and their nullable form
(`ExternType?`). Compound Twinkle values such as records, enums, `Vector`, `Dict`,
callbacks, and `Result` are still not valid extern boundary types.

Linear-memory interop has a first answer: `@std.buffer` exposes a sandboxed,
manually allocated/freed `Buffer` with `u8`/`i64`/`f64` views (see
[design/buffer.md](design/buffer.md)), and stdlib codecs/crypto/`fs` already read
and write bytes through it. That settles the "should there be an explicit
linear-memory type" question for in-module use.

**Remaining open questions:**

- How should Twinkle interoperate with *external* linear-memory Wasm modules
  (shared memory, foreign allocators)?
- Should any compound values gain a standardized ABI lowering across the extern
  boundary?
- How much marshalling should the compiler generate automatically versus requiring
  explicit library code?

The playground and JS runner already bridge some host interactions, but the
language-level FFI model should stay explicit and portable.

---

## 5. Resources plus FFI handles

External resources often appear as opaque handles returned by host APIs. In a
value-semantics language, a handle can be copied inside many immutable record
versions:

```tw
type File = .{ handle: Int, path: String }
```

All record versions may contain the same underlying handle. If one path closes the
handle, older aliases still contain the now-invalid integer.

**Open question:** what is the recommended and/or compiler-enforced model for
opaque resources?

Possible directions:

- keep handles opaque and document safe API patterns,
- represent handles as `Cell`-backed state machines,
- introduce affine/linear resource wrappers,
- require host resources to be used through callback-scoped APIs.

This overlaps with both FFI design and the broader ownership question.
