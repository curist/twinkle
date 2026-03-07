# Module System Design

This document describes the design of Twinkle's module system: how modules are
imported, resolved, named, and composed.

---

## Import Syntax

Modules are imported with `use` followed by a dot-separated identifier path:

```tw
use foo.bar
```

The dot path maps directly to the filesystem: `foo.bar` → `<root>/foo/bar.tw`.
Single-segment imports are valid: `use utils` → `<root>/utils.tw`.

The `use` keyword was chosen over `import` because it is shorter and because
`import` in other languages (Python, JS) often implies bringing names directly
into scope, which is not what Twinkle does — imported modules are always accessed
through qualified names (`bar.fn`).

---

## Project Root Resolution

The project root is resolved by walking up from the entry file's directory until
a `twinkle.toml` file is found. The `TWINKLE_ROOT` environment variable overrides
this with an absolute path. If neither is found, the entry file's directory is
treated as the root (for single-file scripts).

Walking up to a manifest is the established convention (Cargo, go.mod, package.json).
The env var override is useful for CI, scripts, and editor integrations. The
no-manifest fallback keeps single-file programs friction-free.

`twinkle.toml` may initially be empty or contain only a project name — its presence
is what matters for root detection, not its contents.

---

## Standard Library Imports

Stdlib modules use a `@` sigil prefix:

```tw
use @std.fs
use @std.path
use @std.proc
```

The `@` sigil is visually distinct, unambiguous (no user module can start with `@`),
and familiar from Node.js scoped packages. It makes provenance immediately obvious
at the use site.

**Prelude scope:** Builtin types (`Int`, `Float`, `Bool`, `Void`, `String`,
`Array<T>`, `Dict<K,V>`, `Option<T>`, `Result<T,E>`), functions (`print`,
`println`, `error`), and stdlib modules (`Array`, `Dict`, `String`, `Range`)
remain implicitly in scope. Only richer future stdlib modules require an explicit
`use @...`.

---

## Module Aliasing

Module aliasing is supported: `use foo.bar as baz` binds the module under `baz`
instead of the default last-segment name `bar`.

Without aliasing, any project that imports two modules with the same filename
(`use math.vector` and `use graphics.vector`) would be stuck. Aliasing is the
minimal fix.

Importing two modules that resolve to the same identifier without `as` is a
compile-time error:

```tw
use math.vector
use graphics.vector   // error: module identifier "vector" is already bound
                      // help: use an alias, e.g. `use graphics.vector as gvec`
```

---

## No Destructuring Imports (MVP)

Destructuring imports (`use foo.bar.{a, b}`) are deferred past MVP. The spec
enforces qualified access (`bar.fn`) as the default, which is readable and
grep-friendly.

Future syntax (when added):

```tw
use math.vector.{translate, scale}
use math.vector.{translate as tr, scale}
```

To both alias a module and destructure, two separate statements are required —
there is no combined form. Wildcard imports (`use foo.bar.*`) will never be
supported.

---

## Re-exports

There is no special re-export syntax. A module re-exports a name by binding it
at the top level with `pub`:

```tw
use math.vector
pub translate := vector.translate
```

Dedicated re-export forms (`pub use ...`) were rejected because they create
invisible name aliases that are hard to trace. Explicit `pub` rebinding is
consistent with the rest of the language.

---

## Circular Imports

Circular imports are a compile-time error. The compiler detects cycles in the
import graph during module loading and reports them.

Cycle resolution requires either lazy initialization or forward declarations,
both adding complexity. For MVP, cycles are almost always a design mistake. This
can be revisited if a real use case emerges.
