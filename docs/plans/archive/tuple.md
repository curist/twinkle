# Tuples — ad-hoc multi-value grouping

Status: proposal.

Twinkle has no tuple type. When code needs to carry two or three values together
without naming a domain type, today it reaches for one of three workarounds, each
with a cost:

- **A one-off named record** per call site. Clean to read, but the declaration is
  ceremony when the grouping is incidental — e.g. a stack `pop` that must return
  both the popped value and the remaining vector, or a post-order tree walk that
  returns both a subtree height and a running best.
- **A `Vector<T>` as a fixed-size pair**, e.g. `[start, end]` for an interval or
  `[x, y]` for a point. This loses the names (`p[0]`/`p[1]` is positional and
  easy to transpose), loses the arity (nothing stops a 3-element vector), and
  forces a single element type (no `Pair<Int, String>`).
- **Two out-params threaded as separate returns** — impossible without a grouping
  type, so this collapses back to one of the above.

This came out of an ergonomics pass solving a spread of algorithm problems
(DP, grids, graphs, trees, sorting, hashing, and a few recursive-descent
parsers). The codebase held up well across the board; the **one** recurring gap
was exactly this — grouping a handful of values that don't deserve a domain name.
The clearest instances:

```tw
// Returning value + remainder from an immutable pop (appeared in 3 parsers):
type Pop<T> = .{ value: T, rest: Vector<T> }
fn pop<T>(stack: Vector<T>) Result<Pop<T>, String> { ... }

// Returning two accumulators from a recursive walk:
type Probe = .{ height: Int, diameter: Int }
fn walk(t: Tree) Probe { ... }
```

`Pop<T>` and `Probe` are pure plumbing — the kind of type a tuple exists to avoid
declaring. That said, field names are still better when they communicate API
semantics. A tuple should be for local/incidental grouping, not for domain values
or public APIs where `.value` / `.rest` / `.height` carry meaningful information.

## Design tension

A classic tuple `(Int, String)` is a **structural, anonymous** type, and Twinkle
is deliberately nominal: records map to distinct Wasm GC structs, anonymous
`.{ ... }` literals are only allowed where an expected record type is known, and
type aliases don't create new nominal types (see `docs/spec.md`). Adding
structural tuples would cut against that grain and add a second, parallel notion
of "anonymous aggregate" to the type system, the pattern matcher, the backend
struct layout, and the formatter.

The project's track record favors **library-first** answers to ergonomics gaps —
`@std.view` (zero-copy windows), the proposed `@std.queue`, and capabilities-as-
records all add expressiveness with no new syntax or compiler primitive. A tuple
fits the same mold if we resist the urge to give it syntax.

## Goals

- Remove the per-call-site ceremony of declaring an incidental 2-value grouping
  type.
- Stay nominal and library-only: no new syntax, no parser change, no backend or
  monomorphization change.
- Compose with what records already get for free — conditional structural
  `==`/`!=` when all fields satisfy `Eq`, generic functions, pattern-free field
  access.
- Keep the surface tiny and predictable.

## Non-goals

- Do **not** add `(a, b)` literal syntax or `.0`/`.1` positional access in the
  first version (see *Alternative* below — it is the structural-type path and is
  explicitly deferred).
- Do **not** replace domain records. An interval is still better as
  `.{ start, end }`; a tuple is for groupings that genuinely have no good names.
- Do **not** put the type in the prelude initially (mirrors `@std.queue`); revisit
  promotion only if it proves load-bearing.
- Do not add arity beyond `Pair` up front. `Pair` covers the observed cases;
  wider tuples are a smell that wants a record unless repeated real usage proves
  otherwise.

## Recommended: `@std.tuple` with `Pair`

A pure stdlib module — one generic record type plus a terse constructor. Records
already give conditional structural equality when all fields satisfy `Eq`, so no
manual equality witness is needed.

```tw
// boot/stdlib/tuple.tw
pub type Pair<A, B> = .{ first: A, second: B }

/// Terse constructor: pair(x, y) instead of Pair.{ first: x, second: y }.
pub fn pair<A, B>(first: A, second: B) Pair<A, B> {
  .{ first, second }
}

/// Swap the two components.
pub fn swap<A, B>(p: Pair<A, B>) Pair<B, A> {
  .{ first: p.second, second: p.first }
}

/// Stringify witness, so `${p}` works when both components are Stringify.
pub fn to_string<A: Stringify, B: Stringify>(p: Pair<A, B>) String {
  "(${p.first}, ${p.second})"
}
```

Usage at the motivating sites:

```tw
use @std.tuple
use @std.tuple.{Pair}

fn pop<T>(stack: Vector<T>) Result<Pair<T, Vector<T>>, String> {
  case stack.last() {
    .Some(v) => .Ok(tuple.pair(v, stack.drop_last())),
    .None => .Err("stack underflow"),
  }
}

top := try pop(stack)
value := top.first
rest := top.second
```

The two-import-line shape (`use @std.tuple` for the `tuple.pair` constructor,
`use @std.tuple.{Pair}` to name the type in annotations) is the same split already
documented for `@std.view`.

### What this fixes, and what it doesn't

- **Fixes** the multi-value-return ceremony directly: local `pop` and `walk`-like
  helpers can stop needing bespoke record declarations; access stays named
  (`.first`/`.second`, never `[0]`/`[1]`); mixed element types work
  (`Pair<Int, String>`); arity is fixed by the type.
- **Does not** make `Vector<Vector<Int>>` collection literals (LeetCode-style
  `[[1,3],[2,6]]`) prettier — building `Vector<Pair<Int,Int>>` still means
  `tuple.pair(1, 3)` per element rather than `(1, 3)`. That is the literal-syntax
  problem, which only the deferred alternative addresses. In hand-written Twinkle
  this case is usually better served by a domain record (`Interval`) anyway, so
  the win there is smaller than it looks from test fixtures.

### Stringify and future arities

`Stringify` is worth providing for `Pair`: it keeps debug output and string
interpolation consistent with `Option`, `Result`, and `Vector`.

Do **not** promise `Triple.to_string` in the same module yet. Twinkle has a single
value namespace per module and no overloaded function names, while `Stringify`
requires an inherent method named `to_string`. If `Triple` lands later, it needs a
clear answer first: split arities into separate modules, allow only `Pair` to
satisfy `Stringify`, introduce an overload story, or use some other explicitly
chosen design. Until then, keeping the initial module to `Pair` avoids a naming
trap.

### Wiring

Pure-Twinkle stdlib module, the lightest add path: `boot/stdlib/tuple.tw` →
regenerate `core_lib` → `make bundle-cli` → `stdlib_tuple_suite` + docs. No
Rust/stage0 change (nothing in `boot/main.tw` imports it). Same recipe as
`@std.math`.

## Alternative considered: tuple syntax `(a, b)` + `.0`

A real structural tuple — `(a, b)` literals, `(A, B)` type syntax, `.0`/`.1`
access, pattern matching `case p { (x, y) => ... }`. This is what fixes the
collection-literal case and reads the most naturally.

Rejected for the first version because the cost is out of proportion to the
remaining gap:

- It introduces Twinkle's first **structural** aggregate, against the nominal
  design. Either tuples become a special structural carve-out, or `(a, b)`
  desugars to canonical generated records — a non-trivial compiler feature.
- `( ... )` already means grouping; `(a, b)` vs a parenthesized expression vs the
  unit/`Void` case (`()`) needs careful parser disambiguation, and `expr.0`
  collides with nothing today but adds a new postfix form to the
  first-character/`.field` rules.
- Touches parser, grammar (tree-sitter), checker, pattern matcher, backend struct
  layout, and formatter — versus the stdlib option's single `.tw` file.

If, after `Pair` lands and sees use, the collection-literal ergonomics still bite
hard, revisit this as a sugar layer **over** the nominal `Pair` (so `(a, b)` is
sugar for `tuple.pair(a, b)` and `Pair` stays the one runtime type) rather than a
parallel structural system.

## Rollout

1. Add `boot/stdlib/tuple.tw` (`Pair`, `pair`, `swap`, `to_string`). Regenerate
   `core_lib`, `make bundle-cli`.
2. Add `boot/tests/suites/stdlib_tuple_suite.tw`; wire into `boot/tests/main.tw`.
3. Document under `docs/API.md` Standard Library (`@std.tuple`) and add to the
   `use @std.*` list.
4. Migrate the few in-tree bespoke pairs that are pure plumbing; leave genuine
   domain records alone.

## Open questions

- Prelude vs `@std.tuple`? Conservative default is the module; promote only on
  evidence. `Pair` is fundamental enough that promotion is plausible.
- Do we want `map_first`/`map_second`/`map_both` adapters, or is that scope creep
  better left until a caller needs them? Lean toward omitting until asked.
- Should `Triple` exist later? Wait for repeated real 3-value sites, and resolve
  the `Stringify`/namespace issue before committing to its API shape.
