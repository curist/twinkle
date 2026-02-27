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

---

## TBC-002: `Iterator<T>` type and user-extensible iteration

**Spec §12:** `for x in coll` is spec'd to support `Iterator<T>` as one of the
supported collection types, enabling user types to be iterable:

```tw
// tree.tw
pub fn iter<T>(t: Tree<T>) Iterator<T> { ... }

fn sum_tree(t: Tree<Int>) Int {
  acc := 0
  for x in t.iter() { acc = acc + x }
  acc
}
```

**Current state:** `for x in` supports `Array<T>`, `Range`, and `Dict<K,V>`.
`Iterator<T>` does not exist as a type.

**Open questions:**

* What is the representation of `Iterator<T>`? A closure over a mutable index?
  A linked list of values? A built-in struct with a `next: fn() Option<T>` field?
* If `Iterator<T>` is a record with a `next` function field, does it need `Cell<T>`
  internally to track state? (Iterators are inherently stateful.)
* How does the lowerer lower `for x in iter { }` when the type is `Iterator<T>`?
  Repeated `iter.next()` calls until `None`? That requires the iterator to be
  mutable, which conflicts with immutable-by-default semantics.
* Should `Iterator<T>` be a built-in nominal type (like `Cell<T>`) or user-definable?
* Is this worth the complexity, given `collect` + explicit `Array`/`Range` covers
  most practical iteration needs?

**Needs:** A decision on the mutability/state model for iterators before implementation.
