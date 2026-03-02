# Twinkle Design Decisions

This document records design decisions that are not self-evident from the spec.
Each entry captures what was decided, the alternatives considered, and why we
chose the way we did.

---

## D-001: Module Import Syntax — `use foo.bar`

**Decision:** Use `use foo.bar` (dot-separated identifier path) as the import syntax.

**Alternatives considered:**

* `import "path/to/file"` — string-literal path, filesystem-grounded, no ambiguity.
  Rejected because quotes feel low-level, and path strings create no natural
  connection to the qualified access syntax.
* `import foo.bar` — same as chosen but with `import` keyword. Rejected: `use` is
  shorter, and `import` in other languages often implies bringing names directly
  into scope (Python, JS), which is not what this does.

**Rationale:** `use foo.bar` is concise, mirrors qualified access (`bar.fn`), and
enforces clean module naming by using identifiers rather than arbitrary strings.
The dot-to-slash mapping (`foo.bar` → `foo/bar.tw`) is unambiguous and familiar
from Go and Python package conventions.

---

## D-002: Project Root Resolution

**Decision:** Project root is resolved by walking up from the entry file's directory
until a `twinkle.toml` file is found. The `TWINKLE_ROOT` environment variable
overrides this with an absolute path. If neither is found, the entry file's
directory is treated as the root (for single-file scripts).

**Alternatives considered:**

* **CWD** — wherever `twk` is invoked. Breaks when invoked from a subdirectory.
* **Entry file's directory always** — simple, but breaks for libraries (a module
  deep in the tree can't import siblings at the root level).
* **Manifest only (no fallback)** — correct but too strict for quick scripts.

**Rationale:** Walking up to a manifest is the established convention (Cargo,
go.mod, package.json). The env var override is useful for CI, scripts, and
editor integrations that know the root explicitly. The no-manifest fallback keeps
single-file programs friction-free.

**`twinkle.toml`:** Initially may be empty or contain only a project name. Its
presence is what matters for root detection, not its contents.

---

## D-003: Stdlib Modules — `@` Sigil

**Decision:** Stdlib (non-prelude) modules are imported with a `@` sigil prefix:
`use @std.fs`, `use @std.path`, `use @std.proc`.

**Alternatives considered:**

* **`use std.array`** — reserved `std` prefix. Ambiguous with a user directory
  named `std/`; reserves a common name in user project space.
* **No sigil, separate root** — resolve `use array` from stdlib path first, fall
  back to project root. Implicit, hard to understand resolution order, surprising
  failures.

**Rationale:** The `@` sigil is visually distinct, unambiguous (no user module can
start with `@`), and familiar from the Node.js scoped package convention. It makes
the provenance of a module immediately obvious at the use site.

**Prelude scope:** Builtin types (`Int`, `Float`, `Bool`, `Void`, `String`,
`Array<T>`, `Dict<K,V>`, `Option<T>`, `Result<T,E>`), functions (`print`,
`println`, `error`), and stdlib modules (`Array`, `Dict`, `String`, `Range`)
remain implicitly in scope. Only richer future stdlib modules require an explicit
`use @...`.

---

## D-004: Module Aliasing

**Decision:** Module aliasing is supported in MVP: `use foo.bar as baz` binds the
module under the name `baz` instead of the default last-segment name `bar`.

**Alternatives considered:**

* **No aliasing in MVP** — rejected because last-segment collision (`use math.vector`
  and `use graphics.vector` both bind `vector`) is a realistic problem with no
  other resolution mechanism.

**Rationale:** Without aliasing, any project that imports two modules with the same
filename is stuck. Aliasing is the minimal fix and has a simple, conventional
syntax. The `as` keyword is consistent with how aliasing works in most languages
that support it.

---

## D-005: Module Identifier Collision

**Decision:** Importing two modules that resolve to the same last-segment identifier
without using `as` is a compile-time error.

```tw
use math.vector
use graphics.vector   // error: module identifier "vector" is already bound
                      // help: use an alias, e.g. `use graphics.vector as gvec`
```

**Rationale:** Silent shadowing would make code hard to reason about. The error
message should guide the user directly to the solution.

---

## D-006: No Destructuring in MVP

**Decision:** Destructuring imports (`use foo.bar.{a, b}`) are not supported in MVP.

**Future syntax (when added):**

```tw
// bring specific names directly into scope
use math.vector.{translate, scale}

// per-name alias within destructuring
use math.vector.{translate as tr, scale}

// if you also want the module bound, use two separate statements
use math.vector as vec
use math.vector.{translate, scale}
```

There is no combined single-statement form for both aliasing the module and
destructuring; two statements are required (explicit is better here).

**No wildcard:** `use foo.bar.*` is not and will never be supported. It makes it
impossible to know where a name comes from without reading all imports.

**Rationale:** Destructuring adds surface area without being necessary for
correctness. The spec already enforces qualified access (`bar.fn`) as the default,
which is readable and grep-friendly. Destructuring is useful ergonomics for
frequently-used names; deferring it keeps Stage 4 focused.

---

## D-007: Re-exports

**Decision:** There is no special re-export syntax. A module can naturally re-export
a name by binding it at the top level with `pub`:

```tw
use math.vector
pub translate := vector.translate
```

**Rejected:** `pub use math.vector` or `pub use math.vector.{translate}` as
dedicated re-export forms. These are implicit and create invisible name aliases
that are hard to trace.

**Rationale:** Explicit `pub` rebinding is consistent with the rest of the language
(explicit over implicit, no magic). The re-export is visible at the point of
definition.

---

## D-008: Circular Imports

**Decision:** Circular imports are a compile-time error. The compiler detects cycles
in the import graph during module loading and reports the cycle.

**Rationale:** Cycle resolution requires either lazy initialization (complex) or
forward declarations (more surface area). For MVP, cycles are almost always a
design mistake. This can be revisited if a real use case emerges.

---

## D-009: Single-Segment Imports

**Decision:** `use utils` is valid and resolves to `<root>/utils.tw`. It is the
degenerate case of the dot-path rule (`use a.b.c` → `<root>/a/b/c.tw`).

---
