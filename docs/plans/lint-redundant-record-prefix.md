# Spec — `redundant-record-prefix` lint + auto-fix (Pattern A)

**Status: spec, ready for implementation plan.** Turns Pattern A of
[lint-rebinding-fixers.md](lint-rebinding-fixers.md) into a concrete design. Adds
a `twk lint` rule `redundant-record-prefix` with a machine-applicable auto-fix
under `--fix-redundant-record-prefix`.

## Goal

Strip a redundant named-constructor prefix from a record literal wherever the
expected record type is already known, so `TypeName.{ … }` becomes the
low-ceremony contextual form `.{ … }`.

```tw
// Before                              // After
cfg: Config = Config.{ a: 1, b: 2 }    cfg: Config = .{ a: 1, b: 2 }

fn make() Config {                     fn make() Config {
  Config.{ a: 1, b: 2 }                  .{ a: 1, b: 2 }
}                                      }
```

The anonymous `.{ … }` form is legal **only** where an expected record type is
known (CLAUDE.md, "Records"). The whole rule is therefore: fire exactly in the
positions where the compiler would already accept `.{ … }`, and nowhere else.

## Non-goals

- No inference oracle. The linter has no per-expression inferred types (it
  threads only declared nominal types); this rule stays inside what is provable
  from declared types + `ResolvedEnv`.
- No rewrite that changes observable behavior — stripping is a pure syntactic
  narrowing that leaves a well-typed program well-typed and identical.
- Patterns B and C of the umbrella doc are out of scope.

## Baseline / where this lives

Auto-fix rules live in `boot/compiler/lint.tw`; each walks the parsed AST
(`Module` / `Expr` / `Stmt` with `Span`s) plus `ResolvedEnv`. An auto-fixable
finding is a `LintFinding` carrying `edits: Vector<FixEdit>` where
`FixEdit = .{ start, end, replacement }` — source byte-range rewrites. Rule
rationale strings live in `boot/compiler/lint_rules.tw` (`describe`). The rule
hooks `lint_module` alongside the existing per-function and per-top-level-stmt
walks; it needs only the module `source` string (for span slicing and the
spelling gate), not `ResolvedEnv`.

Relevant AST facts (verified):

- `ExprKind.NamedRecord(String, Vector<RecordEntry>)` is `T.{ … }`;
  `ExprKind.Record(Vector<RecordEntry>)` is the anonymous `.{ … }`.
- `RecordEntry = .{ name, value: Expr?, span }`; `Expr = .{ id, kind, span }`.
- `LetStmt = .{ name, ty: TypeExpr?, value, is_rebind, … }` — `ty.Some` is the
  annotation anchor.
- `FunctionDecl = .{ …, return_type: TypeExpr?, body: Block, … }`;
  `Block = .{ stmts: Vector<Stmt>, tail: Expr?, span }`;
  `ReturnStmt = .{ value: Expr?, span }`.
- `inline_copy_finding_for_let` already matches `.NamedRecord(tname, entries)`
  and slices `source.slice(span.start, span.end)` for edits — confirming a
  `NamedRecord`'s `span.start` sits at the type-name prefix and that `slice`
  is byte-indexed consistent with spans.

## Core mechanism — expected-type-directed descent

**Key invariant (why no env is needed).** In well-typed input a
`NamedRecord(name, entries)` node *is* a proof that `name` is a concrete record
type — you cannot spell `SomeEnum.{ … }`, `Int.{ … }`, or a generic
`TypeVar.{ … }` (none parse/typecheck as a named record literal). And the
anonymous `.{ … }` form is legal in exactly the positions where an expected
record type is known. So the two facts the rule needs — *"is the literal a
record?"* and *"is this a position where `.{ … }` is legal?"* — are answered by
the parse node and the syntactic position, with **no `ResolvedEnv` lookup**.
Existing type-dependent lints (`inline-record-copy`, `record-copy-helper`)
already work this way: they match `NamedRecord(tname, …)` and compare declared
type-name strings, never calling `record_info`.

The walk therefore threads a single **`Bool` "expected-typed"** flag, not a
resolved `TypeId`:

```
walk_expr(ctx, expr: Expr, expected_typed: Bool) -> Vector<LintFinding>
```

`ctx` is a small threaded record `PrefixCtx = .{ source: String,
returns_typed: Bool }` (the per-function invariants), so the walk reads as
`ctx.walk_expr(expr, expected_typed)`. `expected_typed` is the varying per-node
value and stays an explicit parameter.

Behavior at each node:

1. If `expected_typed` **and** `expr.kind` is `NamedRecord(name, entries)`
   **and** the spelling gate passes → emit a strip edit for `name`.
2. Recurse into children with the right `expected_typed` for each position
   (see the four sources + flow-through below). Record-field values and call
   arguments are **always** `expected_typed` (fields and parameters always carry
   declared types), independent of the enclosing node's own flag — so a
   `NamedRecord` argument fires even inside a `:=` binding.
3. All other children are recursed with `expected_typed = false`.

### The strip edit

Delete the byte range `[expr.span.start, expr.span.start + name.len())`, leaving
`.{ … }` intact. Replacement is empty. **Spelling gate** (all must hold, else
decline):

- `source.slice(start, start + name.len()) == name`, and
- the two bytes at `start + name.len()` are `.{`.

This refuses qualified constructor paths (`pt.Point.{ … }`), and any case where
comments/whitespace sit between the name and `.{`. Sound: an un-stripped literal
is never wrong, only a missed opportunity.

## The four expected-type sources

Each sets `expected_typed = true` for the child position. No env lookup — the
`NamedRecord` node witnesses the record type if one appears there.

1. **Annotated let** — `let x: T = value` (`LetStmt.ty` is `.Some`): `value` is
   walked with `expected_typed = true`. An unannotated `x := value` (`ty` is
   `.None`) walks with `false`. (A bare rebind `x = value` with no annotation is
   left `false` in v1 — resolving the rebound name's type needs scope tracking;
   conservative, sound.)
2. **Return position** — a function whose `decl.return_type` is `.Some` sets
   `ctx.returns_typed = true`; the body's tail expression and every `.Return(e)`
   in tail-reachable position are walked with `expected_typed = true`. Nested
   `if`/`case`/`cond`/block tails inherit it through flow-through. A function
   with no declared return type walks its body tail with `false`.
3. **Record-field value** — when walking a record literal (named *or* anonymous),
   every field value is walked with `expected_typed = true`. Fields always carry
   a declared type, so this holds regardless of whether the enclosing literal was
   itself in an expected-typed position.
4. **Call argument** — `Call(callee, args)`: the callee sub-expression is walked
   with `false`; every argument is walked with `expected_typed = true`. Function
   parameters always carry declared types, so any argument position is
   expected-typed — no callee resolution needed.

### Flow-through positions

`expected` passes to these sub-expressions unchanged (they occupy the same
type slot as their parent): `if`/`else` branch tails, `case` arm bodies, `cond`
arm bodies, and block tail expressions.

## Safety argument

Each gate is independently sound; a failed gate only declines a fix.

- **The literal is a record.** A `NamedRecord` node only parses/typechecks when
  its name is a concrete record type. Generic type variables, enums, and
  primitives cannot appear as `X.{ … }`, so acting only on `NamedRecord` nodes
  can never strip against a non-record or a type variable.
- **The position is expected-typed.** We strip only in the four positions where
  the language already accepts `.{ … }` (annotated bindings, function params,
  return expressions, record fields) plus flow-through. This is CLAUDE.md's
  verbatim anchor list, so a stripped literal stays legal.
- **No name-equality requirement.** We do *not* compare `name` to any expected
  type name — there is no expected type name in hand, and comparing would miss
  type aliases (`type Foo = Bar`; `let x: Foo = Bar.{ … }`). Stripping stays
  sound because a `.{ … }` in an expected-typed position resolves to that
  position's type regardless of the original prefix.
- **Spelling gate** guarantees the edit removes exactly the prefix and nothing
  load-bearing (qualified paths, interspersed trivia are refused).
- **Well-typed input assumption.** The witness invariant needs the module to
  have typechecked. `run_lint_command` builds findings from `analyze.analyze_module`
  (resolve + typecheck) before linting, and unit tests parse hand-written
  well-typed snippets — both satisfy it. The rule performs no `ResolvedEnv`
  lookup itself, so it also runs correctly under the plain `builtin_env()` test
  harness.

Because the output is a strict syntactic narrowing of an already-accepted
program, no observable behavior changes.

## Interaction with `twk fmt`

Verified: `fmt` neither adds nor removes the constructor prefix — it left both an
annotated `.{ … }` and an unannotated `Config.{ … }` untouched. The fixer's
output is therefore stable under `fmt` (idempotent pipeline: lint-fix then fmt
produces no churn), and the danger position — unannotated `d := Config.{ … }`,
where no expected type exists — is correctly never fired on.

## Rule identity & rollout

- Rule name: `redundant-record-prefix` (kebab, matching `direct-rebinding`,
  `inline-record-copy`, `record-copy-helper`).
- Detection is always on (report). Edits are applied only when
  `--fix-redundant-record-prefix` is passed, matching the existing
  `--fix-inline-record-copy` convention.
- Rationale string added to `lint_rules.tw` `describe`, surfaced by
  `twk lint --explain`.

## Testing & validation

- **Unit fixtures** (via the existing `findings(src)` / `builtin_env()` harness in
  `lint_pass_suite.tw`) for each of the four sources plus each decline case: `:=`
  unannotated position (no fire), function with no declared return type (tail no
  fire), qualified path `pt.Point.{ … }` (spelling-gate decline), already-anonymous
  `.{ … }` (no fire), and nested field/arg positions inside a `:=` value (fire).
- **Boot self-application.** Boot source is dense with `T.{ … }` in annotated and
  return positions. Run the fixer across `boot/`, then:
  - `target/twk fmt` the touched files (expect no additional churn),
  - `make boot-test` green,
  - `make stage2` / self-host stays green and the rebuilt compiler is
    byte-identical (a behavior-preserving rewrite must not change codegen).
- Boot-only: no Rust/stage0 change and no bundled-payload regen — this is a pure
  boot-compiler lint rule.

## Out of scope for v1 (sound omissions, possible follow-ups)

- Variant payload positions (`Some(Config.{ … })`) — not anchored.
- Array-element positions (`xs: Vector<Config> = [Config.{ … }]`) — not anchored.
- Bare rebind anchoring (`p = Config.{ … }` where `p` is a known-typed local).
- Closure return positions — a closure body's tail is walked with
  `expected_typed = false` (nested field/arg anchors inside still fire).

## Sequencing (for the implementation plan)

1. `PrefixCtx` + `walk_*` skeleton + strip edit + spelling gate, wired for the
   **annotated let** source (function bodies and top-level stmts); unit fixtures.
2. Add **return position** (tail + `.Return`) with flow-through into arms/blocks.
3. Add **record-field** and **call-argument** anchors (both just set
   `expected_typed = true` on child positions — no env work).
4. Wire the `--fix-redundant-record-prefix` flag and `describe` rationale.
5. Boot self-application, fmt/boot-test/stage2 byte-identical validation.

Each step is independently shippable behind the same flag; the walk grows one
expected-type source at a time.
