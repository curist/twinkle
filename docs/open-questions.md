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

**Open tooling question:** which patterns should produce warnings?

Likely lint candidates:

- updating a value and then never reading or returning the updated binding,
- field/index update in a statement position whose result is effectively ignored,
- suspicious aliasing patterns where code appears to expect another name to observe
  the update.

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

## 3. Circular modules and recursive type groups

The module system rejects circular imports. This keeps compilation order and
incremental analysis straightforward, but large programs sometimes have mutually
recursive domain concepts.

**Open question:** is acyclic module structure enough in practice, or does Twinkle
need an explicit mechanism for mutually recursive type groups across files?

**Proposed direction:** [plans/recursive-module-groups.md](plans/recursive-module-groups.md)
— condense the module graph into SCCs and resolve each strongly-connected group
with the two-phase (signatures-then-bodies) pass already used *within* a module,
allowing type/function cycles while rejecting top-level value-initialization
cycles. The restriction is architectural, not semantic; this also unblocks
blanket prelude-into-prelude injection.

Possible directions considered:

- keep cycles rejected and encourage colocating mutually recursive types,
- allow type-only cycles with restrictions ← the proposed plan's MVP,
- add explicit forward declarations,
- add package-level recursive type groups.

---

## 4. Resource ownership beyond `defer`

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

## 5. FFI beyond phase-1 externs

Twinkle supports `extern` declarations for host-provided Wasm imports. Phase-1
boundary types are intentionally small: `Int`, `Float`, `Bool`, `String`, and
`Void`/`()`. Compound Twinkle values such as records, enums, `Vector`, `Dict`,
callbacks, and `Result` are not valid extern boundary types today.

This avoids committing too early to a large interop model, but several questions
remain.

**Open questions:**

- How should Twinkle interoperate with linear-memory Wasm modules?
- Should there be explicit `Buffer`, `ByteView`, or linear-memory types?
- Should any compound values have standardized ABI lowering?
- How should opaque host handles be represented safely?
- How much marshalling should the compiler generate automatically versus requiring
  explicit library code?

The playground and JS runner already bridge some host interactions, but the
language-level FFI model should stay explicit and portable.

---

## 6. Resources plus FFI handles

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
