# Module System Design

This document describes the design of Twinkle's module system: how modules are
imported, resolved, named, and composed.

---

## Import Syntax

Modules are imported with `use` followed by either:

- an absolute dot-separated identifier path (`foo.bar`)
- a relative path with a leading dot (`.foo`, `.foo.bar`)

Examples:

```tw
use foo.bar
use .arg
```

Absolute paths map from project root: `foo.bar` → `<root>/foo/bar.tw`.
Single-segment absolute imports are valid: `use utils` → `<root>/utils.tw`.

The `use` keyword was chosen over `import` because it is shorter and because
`import` in other languages (Python, JS) often implies bringing names directly
into scope, which is not what Twinkle does — imported modules are always accessed
through qualified names (`bar.fn`).

---

## Relative Imports (Submodules)

Relative imports are for sibling/submodule access inside a namespace. A leading
dot means "resolve from the current module's parent namespace" (not from project
root).

Example with file `<root>/lib/argparse/app.tw` (module `lib.argparse.app`):

```tw
use .arg      // => lib.argparse.arg
use .command  // => lib.argparse.command
use .style    // => lib.argparse.style
```

Relative imports avoid repeating long namespace prefixes while keeping canonical
module identity rooted at project root.

Rules:

- `use foo` is always absolute (root-relative).
- `use .foo` is always relative to the importing module's parent namespace.
- Relative import paths are lexical; no fallback probing between relative and
  absolute resolution.
- `use @std.*` is unchanged and is never relative.

Nested module structure is still fully supported. Relative imports are intended
for local namespace access; absolute imports are used when jumping to a
different namespace.

Example project layout:

```text
<root>/
  lib/
    argparse/
      app.tw
      arg.tw
      command.tw
      style.tw
      parser/
        token.tw
        reader.tw
```

Example imports from `lib/argparse/app.tw`:

```tw
use .arg
use .command
use .parser.token
use lib.argparse.style  // explicit absolute import also valid
```

Example imports from `lib/argparse/parser/reader.tw`:

```tw
use .token              // => lib.argparse.parser.token
use lib.argparse.style  // cross-namespace jump stays absolute
```

MVP scope: only single leading-dot relative imports are supported (`.foo`,
`.foo.bar`). Parent traversal (`..foo`) is not part of MVP.

---

## Project Root Resolution

The project root is resolved by walking up from the entry file's directory until
a `twinkle.toml` file is found. If none is found, the entry file's directory is
treated as the root (for single-file scripts).

Walking up to a manifest is the established convention (Cargo, go.mod, package.json).
The no-manifest fallback keeps single-file programs friction-free.

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

Circular imports are allowed when the cycle is through type and function
interfaces only. The compiler breaks these cycles with preliminary module
interfaces, then checks each module body once its dependencies are available.

Cycles involving top-level value initialization remain compile-time errors:
top-level statements execute, and Twinkle does not define an initialization order
inside an import cycle. The diagnostic points at the import edge that would make
that value-initialization cycle unavoidable.
