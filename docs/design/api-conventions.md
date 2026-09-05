# API Conventions

This document records the naming and return-type conventions that the prelude and
standard library follow, so that a new API is *predictable*: a reader who has not
seen a particular function should be able to guess how it fails and what its
forcing/unchecked form is called. These are conventions for library design, not
compiler-enforced rules.

---

## Fallible operations: `Option`, `Result`, or trap

A function that can fail chooses one of three shapes based on **what the caller can
do with the failure**, not on how the function is implemented.

### `Option<T>` — failure is a single, information-free "no"

The value either forms or it does not, and there is nothing worth reporting about
why. The caller's only question is presence.

* Primitive parsers and constructors: `Int.from_string`, `Float.from_string`,
  `Byte.from_int`, `String.from_utf8`, `String.from_code_point`,
  `String.from_char_code`, `String.from_byte`.
* Safe accessors: `Vector.get`, `String.get`, `Dict.get`, `Vector.first`/`last`.

### `Result<T, E>` — failure carries actionable detail

The failure has a cause the caller might branch on, surface to a user, or recover
from. `E` is a **domain enum** when callers distinguish causes, or `String` when a
human-readable reason is enough.

* Typed-error domains: `@std.fs` (`FsError`), `@std.regexp` compile (`RegexError`).
* Codecs with a diagnosable reason: `crypto.hex_decode`, `crypto.base64_decode`
  (`Result<_, String>`).

### trap — failure means a broken precondition, not runtime data

The failure indicates a bug in the calling code, so there is no value to hand back
and nothing to recover; the program traps (see spec §6). A trapping expression has
type `Never`.

* Out-of-bounds positional access (`v[i]`, `s[i]`, `v[i] = x`), division by zero.
* Explicit `error(...)`, `@std.proc`'s `exit(...)`.

**Rule of thumb.** Can the caller do something meaningful with the reason? →
`Result`. Is "it didn't work" the whole story? → `Option`. Is a failure a bug in
the caller? → trap.

---

## Naming the forcing and unchecked forms

The user-facing surface has two forcing forms, plus operator sugar for unchecked
positional access. They are not interchangeable — the name tells you which
situation you are in.

| Form | Situation | Behavior | Examples |
|---|---|---|---|
| `.unwrap()` / `.unwrap_or*()` | you already hold an `Option`/`Result` | force it (trap) or supply a fallback | `opt.unwrap()`, `res.unwrap_or(0)` |
| `.must` / `must_*` | build directly from raw input | parse/compile-or-trap (construction + unwrap in one) | `regexp.must(pattern)` |
| `c[i]` (operator sugar) | unchecked positional access | traps on out of bounds; the safe form is `.get(i) -> Option` | `v[i]`, `s[i]` |

Guidelines:

* Reach for `must` only when a construction that normally returns `Result` should
  trap at the call site. When you already hold an `Option`/`Result`, the general
  forcing form is `.unwrap()`; there is no separate `must` for that.
* There is deliberately **no user-facing `_unsafe` method surface.** Unchecked
  operations (`vector$set_unsafe`, `dict$get_unsafe`, `dict$get`) exist only as
  internal compiler intrinsics that back operator sugar and loop lowering; they are
  not exposed as callable names. If a future unchecked operation ever needs to be
  user-callable, it should be a **type-qualified method with a checked sibling**
  (e.g. `Dict.get_unsafe` next to `Dict.get`), never a bare snake_case free
  function.

---

## Positional vs keyed indexing

`c[i]` and `d[k]` deliberately differ in safety, following the rule above:

* **Positional** access (`Vector`, `String`) — `v[i]` / `s[i]` **trap** on an
  out-of-bounds index. A positional index asserts a precondition (the index is in
  range); violating it is a caller bug. The safe form is `.get(i) -> Option`.
* **Keyed** access (`Dict`) — `d[k]` returns **`Option<V>`**. A keyed lookup is
  inherently a presence test, so absence is ordinary runtime data, not a bug.

This is intentional: the two brackets read the same but answer different questions,
so they carry different failure shapes.
