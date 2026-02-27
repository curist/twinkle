# Twinkle — To Be Confirmed

Design areas that need more discussion before committing to an implementation
approach. These are not yet accepted work items.

---

## TBC-001: `@stdlib` module imports

**Spec §8.2, module.md D-003:** The `@` sigil prefix (`use @array`, `use @std.json`)
is reserved for standard library modules that live outside the project tree.

**Current state:** The syntax is parsed and `is_stdlib: bool` is set on `ImportDecl`.
The module loader returns a "not yet implemented" error when `is_stdlib` is true.

**Open questions:**

* What is the stdlib module path? An embedded directory in the binary? A path
  resolved from the `twinkle` install prefix? A directory next to `twinkle.toml`?
* What modules exist? The prelude already covers `Array`, `Dict`, `String`, `Range`,
  `Option`, `Result`. What would additional `@` modules add?
* Does the stdlib ship as `.tw` source or pre-compiled IR?
* How does the module system cache and version stdlib modules?

**Needs:** A concrete stdlib scope decision before implementation begins.

