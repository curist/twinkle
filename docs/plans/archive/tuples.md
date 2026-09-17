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

A concrete case tuples uniquely unlock: **value-and-rest returns**. A
`Vector.pop() -> (T, Vector<T>)?` (or `drop_first` returning the dropped head
*and* the tail) is impossible to express cleanly today — you must return one or
the other, or reach for `@std.tuple`. With tuples it is the natural signature.
This is a prelude function, which is why **stage0 parity is required** (see
below): the prelude is compiled by the Rust stage0 bootstrap compiler.

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
| Representation | **Compiler-known prelude records** `Tuple2/3/4`, fields `_0.._3` | Zero type-system change; ordinary records to checker/mono/codegen; keeps the nominal model |
| Contract witnesses | **Generic prelude functions** (`Eq` automatic; 6 hand-written `compare`/`to_string`) | Review-verified: no synthesizer needed; matches how `Vector`/`Option`/`Pair` already do it |
| stage0 parity | **In-scope, required** | Prelude tuple defs + tuple-typed APIs are compiled by Rust stage0; `make stage2` fixed point is the gate |
| Access syntax | **`._0` … `._3`** (0-indexed) | Already valid field access with **no** lexer/parser change; identifier-shaped accessors avoid the float-lexer collision that breaks `.0`-style nested access (`t.0.1` lexes `0.1` as a float — see below) |
| Literal syntax | **`(a, b)` … `(a, b, c, d)`** | New parser work: comma-loop in the `(` primary (array-literal `[` is the template). Newline/call-gluing is already handled; comma parsing is not |
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

- **Literal.** `(a, b)`, `(a, b, c)`, `(a, b, c, d)` produce a tuple; a single
  parenthesized expression stays grouping. **This is new parser work** — the `(`
  primary case (`parser.tw`) currently parses exactly one inner expression and
  requires `)` (so `(1, 2)` errors today). It must grow a comma-loop, using the
  sibling array-literal `[` case as the template. (Only the *newline/call-gluing*
  disambiguation is already handled — see below; comma parsing is not.)
- **Type.** `(A, B)` … `(A, B, C, D)` produce a tuple type. Also new parser work:
  `parse_type_expr_base` has no `(` case today, so there is nothing to conflict
  with — it is entirely new code, not a cleared collision.
- **Access.** `t._0` … `t._3` is plain field access — already works today with no
  parser change (verified: a record with fields `_0`/`_1` accepts both the
  declaration and `t._0` access). Nested access `t._0._1` is safe.
- **Edges left unchanged.** `(a)` remains grouping (no 1-tuples). `()` remains a
  parse error (no unit type — `Void`/`{}` already fills that role).

### AST representation

Tuple literals parse to a **dedicated `ExprKind.Tuple(Vector<Expr>)` variant**,
and tuple types to a `TypeExprKind.Tuple(Vector<TypeExpr>)` variant — the nodes
survive to the printer rather than being desugared into `NamedRecord`/`TupleN.{…}`
at parse time. Precedent: `!E` parses straight into `TypeExprKind.Result(...)`
and the formatter re-renders it as `ok!err` (`fmt/printer.tw`). The formatter
prints `.Tuple(xs)` back as `(a, b)` directly, so `twk fmt` stays idempotent with
no resugar heuristic.

Cost: a `.Tuple` arm must be added to every exhaustive `ExprKind`/`TypeExprKind`
match (lint, fmt, checker, lower_core, and any other pass that matches without a
`_` wildcard). Twinkle's exhaustiveness checking flags each missing arm at
compile time, so this is mechanical, not a hunt. Lowering maps `.Tuple(xs)` to a
`TupleN` record construction; from that point on the pipeline sees an ordinary
record. (The rejected alternative — desugaring to `NamedRecord("Tuple2", …)` at
parse time — touches fewer files but forces the formatter to *detect* tuple-shaped
records and resugar them, or it expands `(a, b)` into `Tuple2.{ _0: a, _1: b }` on
save, breaking the ergonomics and idempotence.)

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

These are provided the **idiomatic Twinkle way — ordinary generic prelude witness
functions**, not a compiler synthesizer. A design review verified (with working
repros against the current compiler) that no new derivation machinery is needed:

- `Eq` / `==` is **already automatic**. Records auto-derive `Eq` when all fields
  satisfy `Eq` (`checker.tw` `try_auto_derive_eq` is a compile-time proof, and at
  runtime `==` on any non-primitive dispatches to the single generic host routine
  `rt.core.eq` — there is no per-type codegen). So `TupleN` gets `==`/`!=` for
  free just by being a record.
- `Ord` / `compare` and `Stringify` / `to_string` are **hand-written generic
  functions**, exactly how `Vector<T>`, `Option<T>`, `Result<T,E>`, and today's
  `@std.tuple` `Pair` already provide them (`boot/stdlib/tuple.tw`'s `to_string`
  is three lines). At the arity-4 cap that is **six small functions** total —
  `compare` and `to_string`, one per arity — e.g.:

  ```tw
  fn compare<A: Ord, B: Ord>(x: (A, B), y: (A, B)) Order {
    case x._0.compare(y._0) { .Eq => x._1.compare(y._1), other => other }
  }
  fn to_string<A: Stringify, B: Stringify>(t: (A, B)) String {
    "(${t._0}, ${t._1})"
  }
  ```

The language rule "user records don't auto-derive `Ord`/`Stringify`" stays
unchanged; tuples get them because these witness functions exist for the
compiler-owned `TupleN` types, resolved through the same generic
`prove_contract_method` path the other conditional types use.

**Checker proof wiring — verify in the spike:** conditional-satisfaction proofs
for some builtin containers are hardcoded per container in
`checker.tw` (`try_builtin_container_contract`, e.g. `.Ord => case ty { .Vector … }`).
The repro suggests `TupleN`'s `Ord`/`Stringify` resolve purely via generic method
lookup with **no** table entry needed, but confirm this early rather than assume.

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

A deliberate sequencing insight: the `TupleN` records and their witness functions
can be written with **no tuple sugar** (`Tuple2<A, B>` type name, `Tuple2.{ _0: …,
_1: … }` construction, `._0` access) — all of which are ordinary generic-record
syntax that both boot **and** stage0 already compile. So the records/witnesses
layer lands first and keeps `make stage2` green with zero parser change; the
`(…)` sugar is added to both compilers only afterward.

0. **`Tuple2`-only spike (boot, de-risk).** Prove the hard/unknown parts on one
   arity before generalizing: the comma-in-parens parser change, the
   `ExprKind.Tuple` variant, the `twk fmt` round-trip printing `(a, b)` back
   unchanged, and whether `TupleN`'s `Ord`/`Stringify` proof resolves via generic
   method lookup or needs a `try_builtin_container_contract` entry.
1. **Records + witnesses, sugar-free (boot + stage0 both stay green).**
   `Tuple2/3/4` generic records (fields `_0.._3`) in the prelude + the six
   generic witness functions (`compare`/`to_string` per arity), written in
   explicit record syntax so stage0 compiles them unchanged. `==` is automatic.
2. **Tuple sugar in the boot compiler.** `(a, b, …)` → `ExprKind.Tuple` →
   `TupleN` construction on lowering; `(A, B, …)` → `TypeExprKind.Tuple` →
   `TupleN<…>`; add `.Tuple` arms across exhaustive matches; fmt prints `(a, b)`.
   `._0.._3` access already works — tests only.
3. **Tuple sugar in stage0 (`src/`).** Mirror step 2 in Rust: parser comma-loops
   (expr + type), the `Tuple` AST node(s), type-checking, and lowering to the
   same `TupleN` record shape (happy path only). Gate: `make stage2` reaches a
   fixed point.
4. **Migrate `@std.tuple`.** Rewrite consumers to sugar, delete the library,
   regenerate `core_lib`.
5. **Payoff APIs.** Add the value-and-rest prelude functions tuples unlock —
   e.g. `Vector.pop() -> (T, Vector<T>)?` — proving the feature end-to-end.
6. **Grammar + tooling.** Update `docs/grammar.ebnf` (`PrimaryExpr` and the type
   grammar), then the tree-sitter grammar + regenerated artifacts.
7. **Docs.** `docs/API.md` tuple section (replacing the `@std.tuple` section),
   including the "name it when the fields have names" guidance and `Vector.pop`.

### stage0 parity (required, in-scope)

The Rust stage0 compiler in `src/` gets full tuple support in this work — this is
**not** deferred. Two forcing reasons:

- The `TupleN` records and their witness functions live in the **prelude**, which
  stage0 lowers eagerly on every `make stage2`. Even before any tuple-returning
  API, stage0 must at least compile those definitions.
- The motivating APIs (`Vector.pop`, value-and-rest helpers) are prelude
  functions whose *signatures use tuple types* — so stage0 must parse `(A, B)`
  types, `(a, b)` literals, `._n` access, and lower them, or `make stage2` fails
  (the established "feature used in boot source needs stage0 support" rule).

Practically this means mirroring the boot-compiler work in `src/`: lexer already
has the tokens; add the `(` comma-loop in the expression and type parsers, the
`Tuple` AST node(s), type-checking, and lowering to the same `TupleN` record
shape. stage0 only needs the **happy path** (no diagnostics polish). The
self-host loop (`make stage2` reaching a fixed point) is the acceptance gate.

To keep the spike honest, sequence stage0 parity **before** putting any tuple
syntax into prelude source that stage0 must compile — i.e. the `Tuple2` spike can
be boot-only, but the witness functions and any `Vector.pop`-style prelude API
land only once stage0 parses tuples.

## Fast-follow (next, after v1 base + migration land)

**Destructuring patterns.** `(a, b)` in `case` arms (reuses the existing
`PatternList`) and `(a, b) :=` in `let` bindings (the one structural grammar
edit: widen `LetBinding` to accept a pattern LHS — in both boot and stage0). This
is the immediate next step once tuples exist and `@std.tuple` is gone — call it
out in the eventual plan so it is not lost.

**Adoption inside `boot/compiler/*.tw` is gated on destructuring.** Even though
stage0 parses tuples after v1, the compiler's *own* source will not use tuples
until destructuring lands — reading `x._0`/`x._1` for multi-return in the
compiler is worse than the current explicit helper structs, so it waits for
`(a, b) :=`. This is a style rule, not a stage0 constraint. v1 tuple usage stays
in the prelude, stdlib, examples, and user code.

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

- **The real work is parsing + two-compiler AST/lowering, not contract
  machinery.** A design review (with working repros) confirmed the witnesses are
  six small generic functions (`Eq` is already automatic), so no synthesizer is
  needed. The genuine effort is: the comma-in-parens parser change and the
  `ExprKind.Tuple`/`TypeExprKind.Tuple` variant, done in **both** boot and stage0
  (`src/`), plus threading `.Tuple` arms through every exhaustive match
  (compiler-caught, mechanical).
- **stage0 parity is a hard gate.** `make stage2` reaching a fixed point is the
  acceptance test; the prelude witness/records layer is deliberately sugar-free
  so it lands before the parser work and never breaks the bootstrap.
- **Spike first.** The `Tuple2`-only spike de-risks the two underspecified
  unknowns (comma parsing, fmt round-trip / representation) before generalizing
  and migrating.
- **Migration is a breaking change.** Acceptable: `@std.tuple` was a stopgap, and
  the in-repo surface is small and mechanical (one production consumer,
  `boot/stdlib/regexp/parse.tw`, plus tests/examples).
- **Tree-sitter regen needs Docker + a human test run** (project rule); also
  update `docs/grammar.ebnf`.
