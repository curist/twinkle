# Dedicated Set Type Plan

## Goal

Add a first-class `Set<K>` collection for unique values.

Twinkle users can already model sets as `Dict<K, Void>` or `Dict<K, Bool>`, but
that exposes an implementation trick at call sites and makes common operations
less readable. A dedicated `Set` makes intent explicit while reusing the existing
persistent dict machinery.

## Motivation

Text-processing and compiler-style code often needs to track membership:

```tw
seen: Dict<String, Void> = Dict.new()
seen["name"] = {}
if seen.has("name") { ... }
```

A set API expresses the same idea directly:

```tw
seen := Set.new()
seen = seen.insert("name")
if seen.contains("name") { ... }
```

This matches the direction of nearby languages:

- Rust exposes dedicated `HashSet<T>` and `BTreeSet<T>` types, even though a
  hash set is conceptually map-to-unit storage.
- Gleam exposes `gleam/set` as an opaque wrapper around `gleam/dict`.

Twinkle should follow the Gleam-like path: a small, immutable API backed by the
existing dictionary implementation.

## Target API

Initial prelude surface:

```tw
Set.new()              // Set<K>
set.insert(k)          // Set<K>
set.remove(k)          // Set<K>
set.contains(k)        // Bool
set.len()              // Int
set.is_empty()         // Bool
Set.from_vector(xs)    // Set<K>
set.to_vector()        // Vector<K>

set.union(other)        // Set<K>
set.intersection(other) // Set<K>
set.difference(other)   // Set<K>
set.is_subset(other)    // Bool
```

Suggested full signatures:

```tw
pub fn new<K>() Set<K>
pub fn insert<K>(s: Set<K>, k: K) Set<K>
pub fn remove<K>(s: Set<K>, k: K) Set<K>
pub fn contains<K>(s: Set<K>, k: K) Bool
pub fn len<K>(s: Set<K>) Int
pub fn is_empty<K>(s: Set<K>) Bool
pub fn from_vector<K>(xs: Vector<K>) Set<K>
pub fn to_vector<K>(s: Set<K>) Vector<K>

pub fn union<K>(a: Set<K>, b: Set<K>) Set<K>
pub fn intersection<K>(a: Set<K>, b: Set<K>) Set<K>
pub fn difference<K>(a: Set<K>, b: Set<K>) Set<K>
pub fn is_subset<K>(a: Set<K>, b: Set<K>) Bool
```

All functions should be available through qualified form (`Set.insert(s, k)`) and
method sugar (`s.insert(k)`) where applicable.

## Semantics

### Key types

`Set<K>` should support the same key types as `Dict<K, V>`:

- `Int`
- `String`
- `Byte`

Do not expand supported key types as part of this work. If `Dict` later supports
more key types, `Set` can inherit that support.

### Immutability

Sets are persistent values. Operations return a new set and leave the old one
usable:

```tw
s1 := Set.new().insert("a")
s2 := s1.insert("b")
// s1 contains only "a"; s2 contains "a" and "b"
```

Assignment sugar is not required for sets in the first version. Users can rebind:

```tw
seen = seen.insert(k)
```

### Duplicate insertion

Inserting an existing member is idempotent:

```tw
Set.new().insert("a").insert("a").len() == 1
```

### Removal

Removing a missing member is a no-op.

### Ordering

`Set.to_vector` should use the same observable ordering as the backing `Dict`:

- first insertion order is preserved
- reinserting an existing member keeps its position
- removing a member removes it from the order
- removing and later inserting the same member appends it as a fresh insertion

This keeps set behavior predictable and consistent with `Dict.keys()`.

Set-theory operations should have deterministic ordering:

- `a.union(b)`: start with `a`'s order, then append members from `b` not already
  present
- `a.intersection(b)`: preserve the order from `a`, keeping only members also in
  `b`
- `a.difference(b)`: preserve the order from `a`, removing members present in
  `b`

`is_subset(a, b)` returns true when every member of `a` is contained in `b`.

## Representation

Use a nominal record backed by `Dict<K, Void>`:

```tw
pub type Set<K> = .{ entries: Dict<K, Void> }
```

`Void` is preferable to `Bool` because the value carries no information. Current
Twinkle supports `Dict<K, Void>` and dict assignment with `{}` values.

Implementation sketch:

```tw
pub fn new<K>() Set<K> {
  Set.{ entries: Dict.new() }
}

pub fn insert<K>(s: Set<K>, k: K) Set<K> {
  s.entries = s.entries.set(k, {})
  s
}

pub fn remove<K>(s: Set<K>, k: K) Set<K> {
  s.entries = s.entries.remove(k)
  s
}

pub fn contains<K>(s: Set<K>, k: K) Bool {
  s.entries.has(k)
}

pub fn to_vector<K>(s: Set<K>) Vector<K> {
  s.entries.keys()
}
```

The final code should follow the actual prelude/module conventions used by the
compiler, but the implementation should stay this thin unless a performance
issue appears.

## Placement

Likely files to update:

- `boot/prelude/set.tw` — main implementation
- prelude module registration/import wiring so `Set` is available like `Dict`
- boot signature/core-lib snapshots, if required by the current prelude pipeline
- Rust stage0 equivalents only if needed for bootstrapping or API parity
- `docs/API.md` — document the new `Set<K>` section
- `docs/plans/scripting-ergonomics.md` — mark Set as the chosen Phase 2 path

Before implementation, inspect how `Vector`, `Dict`, and other prelude nominal
APIs are registered so `Set` follows the same pattern.

## Tests

Add boot tests covering:

- create empty set
- insert and contains
- duplicate insert does not increase length
- remove existing member
- remove missing member is a no-op
- `from_vector` deduplicates
- `to_vector` preserves insertion order
- union ordering and membership
- intersection ordering and membership
- difference ordering and membership
- subset true/false cases
- supported key types: `String`, `Int`, `Byte`

If stage0 gets a mirror implementation, add corresponding Rust/compiler tests as
needed to keep bootstrapping reliable.

## Non-goals

- No mutable set API in the first version
- No hash/ordering contracts exposed to users
- No expansion beyond current `Dict` key types
- No set literal syntax
- No ordered-vs-hash set distinction yet

## Open Questions

### Should sets be iterable directly?

Nice to have, but not required for the first version. `set.to_vector()` is enough
for explicit iteration:

```tw
for item in set.to_vector() { ... }
```

Direct `for item in set` can be considered later if the iterable-lowering path is
straightforward for nominal prelude types.

### Should there be aliases for naming familiarity?

The initial API uses `contains` to match existing `Vector.contains` and
membership phrasing. We can add aliases later if useful, but the first version
should avoid duplicating names such as `has`, `member`, or `delete`.
