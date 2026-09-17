# First-Class Tuples — Design

**Status:** Design approved; ready for implementation planning.

## Goal

Give Twinkle first-class tuple syntax for lightweight positional multi-return —
`(a, b)` values and `(A, B)` types with `._0` access — and remove the
`@std.tuple` library, which existed only as a workaround for the missing
language feature.

## Motivation

The driving use is **general multi-return ergonomics**: any function should be
able to return several values without declaring a named record or importing a
library.

```tw
divmod := fn(a: Int, b: Int) (Int, Int) { (a / b, a % b) }
qr := divmod(17, 5)
q := qr._0   // 3
r := qr._1   // 2
```

Today this requires `@std.tuple` — a two-line import plus `tuple.pair(a, b)`
construction and `.first`/`.second` access — which is why the pair-returning
stdlib functions (`split_once`, `zip`, `partition`) were deferred. `@std.tuple`
was always a stand-in for a language feature; with real tuples it goes away.

## Philosophy note

Twinkle is deliberately nominal ("name your data"). Tuples are the sanctioned
exception for *genuinely positional, short-lived* groupings and multi-return —
**not** for values whose fields have meaningful names. Coordinates (`x, y, z`),
colors (`r, g, b, a`), and parser state want records, and the docs must say so.
The arity cap reinforces this: past four values you are almost certainly
describing a record.

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Add tuples at all | **Yes** | Multi-return friction is real; `@std.tuple` is a workaround, not a design |
| Arity | **2–4** (Pair, Triple, Quad) | Covers realistic multi-return; hard cap steers larger cases to records (Elm caps at 3; nobody who allowed large tuples is glad they did) |
| Representation | **Compiler-synthesized nominal records** `Tuple2/3/4`, fields `_0.._3` | Zero type-system change; reuses records, monomorphization, and contract witnesses; keeps the nominal model |
| Access syntax | **`._0` … `._3`** (0-indexed) | Already valid field access with **no** lexer/parser change; identifier-shaped accessors avoid the float-lexer collision that breaks `.0`-style nested access (`t.0.1` lexes `0.1` as a float — see below) |
| Literal syntax | **`(a, b)` … `(a, b, c, d)`** | Top-level comma in the `( )` primary; disambiguation already handled (see below) |
| Type syntax | **`(A, B)` … `(A, B, C, D)`** | Bare parens in type position; `fn`-types are `fn`-prefixed, so no conflict |
| `@std.tuple` | **Delete and migrate** | It was the workaround; keeping it alongside real tuples is redundant |
| v1 destructuring | **None** | Patterns are the one real grammar edit; ship the base + migration first |

## Design

### Representation

`Tuple2<A, B>`, `Tuple3<A, B, C>`, `Tuple4<A, B, C, D>` are compiler-known
nominal records living in the prelude (auto-available, no import). Their fields
are named `_0`, `_1`, `_2`, `_3`. They are ordinary records to the type checker,
monomorphizer, and codegen — no new structural type kind, no unification
changes.

Users never write the `TupleN` names; they use the `(…)` sugar. The names exist
only as the desugaring target and for diagnostics.

### Syntax

- **Literal.** `(a, b)`, `(a, b, c)`, `(a, b, c, d)` desugar to `TupleN`
  construction. A parenthesized expression with a top-level comma is a tuple; a
  single expression stays grouping.
- **Type.** `(A, B)` … `(A, B, C, D)` desugar to `TupleN<…>`.
- **Access.** `t._0` … `t._3` is plain field access — already works today with no
  parser change (verified: a record with fields `_0`/`_1` accepts both the
  declaration and `t._0` access). Nested access `t._0._1` is safe.
- **Edges left unchanged.** `(a)` remains grouping (no 1-tuples). `()` remains a
  parse error (no unit type — `Void`/`{}` already fills that role).

### Why `._0`, not `.0`

The lexer greedily lexes `<digits> . <digits>` as a float fraction
(`scan_number`, `lexer.tw`). So `t.0.1` — intended as "field 0, then field 1 of a
nested tuple" — lexes as `t . 0.1`, with `0.1` swallowed as a float literal. This
is the classic Rust nested-tuple-access wart, and it is live in Twinkle's lexer.
Identifier-shaped `._0` accessors never enter `scan_number`, so `t._0._1` is
unambiguous and needs no lexer work. `._0` is therefore both the ergonomic and
the low-cost choice; it is 0-indexed to match Twinkle's 0-indexed vectors and
strings.

### Newline / call disambiguation (already handled)

A `(` (or `[`) at the start of a line is **not** glued to the previous line as a
call, because `parse_postfix` breaks on any newline-preceded token except `.`
(`parser.tw`). So tail-position tuple returns and future `(a, b) :=` statements
are safe from accidental call-parsing. The only rule to document: a `(` on the
*same* line right after an expression is still a call (`f (a, b)` means
`f(a, b)`) — the existing, consistent behavior.

### Contracts

`Tuple2/3/4` carry, conditional on their element types satisfying the relevant
contract:

- `==` / `!=` (structural equality),
- `compare` (`Ord`, lexicographic by position),
- `to_string` / `Stringify`, rendering `(a, b)` / `(a, b, c)` / `(a, b, c, d)`
  (so `${t}` interpolation works).

These are provided the same way `@std.tuple` provides them today, now capped at
four arities and moved into the prelude/compiler surface.

### Migration (part of v1)

Delete `boot/stdlib/tuple.tw` and its triple submodule, then migrate every call
site:

- `tuple.pair(a, b)` / `tuple.triple(a, b, c)` → `(a, b)` / `(a, b, c)`
- `Pair<A, B>` / `Triple<A, B, C>` type annotations → `(A, B)` / `(A, B, C)`
- `.first` / `.second` / `.third` → `._0` / `._1` / `._2`
- `.swap()` → dropped entirely (no `TupleN` helper methods in v1); the sole
  call site lives in the deleted tuple test suite. Callers that need it write
  `(p._1, p._0)`
- remove the `use @std.tuple` / `use @std.tuple.{Pair, Triple}` import lines

Known live sites: `boot/stdlib/regexp/parse.tw` (the one production stdlib
consumer), several `boot/tests/suites/*`, `boot/repros/*`, leetcode examples
under `examples/`, and `docs/API.md`. Regenerate `core_lib` after prelude
changes. This is a breaking change for any external `@std.tuple` user, but the
rewrite is mechanical.

### Tree-sitter

`tree-sitter-twinkle/grammar.js` gains tuple literal and tuple type rules.
Regenerate `src/` and the wasm; per project rule, **hand `tree-sitter test` off
to the human** rather than running it from the agent.

## v1 scope (ordered)

1. Synthesized `Tuple2/3/4` nominal records (fields `_0.._3`) in the prelude,
   with `==`, `compare`, and `to_string` witnesses.
2. `(a, b, …)` literal sugar → `TupleN` construction.
3. `(A, B, …)` type sugar → `TupleN<…>`.
4. `._0.._3` access — already works; add tests only.
5. Migrate all `@std.tuple` consumers; delete the library; regenerate
   `core_lib`.
6. Tree-sitter grammar + regenerated artifacts.
7. Docs: `docs/API.md` tuple section (replacing the `@std.tuple` section),
   including the "name it when the fields have names" guidance.

## Fast-follow (next, after v1 base + migration land)

**Destructuring patterns.** `(a, b)` in `case` arms (reuses the existing
`PatternList`) and `(a, b) :=` in `let` bindings (the one structural grammar
edit: widen `LetBinding` to accept a pattern LHS). This is the immediate next
step once tuples exist and `@std.tuple` is gone — call it out in the eventual
plan so it is not lost.

```tw
// fast-follow target
(q, r) := divmod(17, 5)
case lookup(k) { .Some((key, val)) => use(key, val), .None => ... }
```

Once destructuring lands, revisit the deferred pair-returning stdlib
(`split_once`, `zip`, `partition`) — they become natural to add and consume.

## Out of scope

- Arity beyond 4 (use a record).
- 1-tuples and a unit `()` type.
- `.first`/`.second` named accessors (positional `._N` only).
- Variadic-generic operations over "any tuple" (rank-1 HM has no such facility;
  each arity is an independent monomorphic shape).
- `for (a, b) in …` tuple-pattern loops — the existing bare `for a, b in …`
  stays the idiom.

## Risks / notes

- **Contract-witness generation is the arity cost.** Capping at 4 bounds it to
  four hand-provided (or four generated) witness sets, mirroring why Rust caps
  its tuple trait impls.
- **Migration is a breaking change.** Acceptable: `@std.tuple` was a stopgap, and
  the in-repo surface is small and mechanical.
- **Tree-sitter regen needs Docker + a human test run** (project rule).
