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
rationale strings live in `boot/compiler/lint_rules.tw` (`describe`). The rule is
threaded through `lint_module`'s existing per-function walk (alongside
`inline-record-copy`), reusing the `LintCtx` that already carries `source` and
`env`.

Relevant AST / env facts (verified):

- `ExprKind.NamedRecord(String, Vector<RecordEntry>)` is `T.{ … }`;
  `ExprKind.Record(Vector<RecordEntry>)` is the anonymous `.{ … }`.
- `RecordEntry = .{ name, value: Expr?, span }`.
- `record_info(env, tid) RecordInfo?` returns `.Some(.{ name, fields })` with
  `ResolvedField = .{ name, ty: MonoType }` **iff** `tid` is a record (`.None`
  for enums, aliases, unknowns) — this is the "is a concrete record" oracle.
- `lookup_registered_type(env, name) TypeEntry?` maps a type name to its
  `TypeId`; `lookup_registered_function(env, name) FunctionSig?` and
  `lookup_method(env, type_name, method_name) String?` give call targets.
- `MonoType.Named(TypeId, Vector<MonoType>)` is a nominal instantiation; generic
  positions surface as `MonoType.Var(_)` and never resolve to a record.

## Core mechanism — expected-type-directed descent

A single recursive walk over expressions carrying a **resolved expected record
type**:

```
check_expr(expr: Expr, expected: TypeId?, ctx) -> accumulate LintFindings
```

`expected` is `.Some(tid)` only when the position's expected type resolves to a
**concrete record** (`record_info(env, tid)` is `.Some`); otherwise `.None`.

Behavior at each node:

1. If `expected` is `.Some(_)` **and** `expr.kind` is `NamedRecord(name, entries)`
   **and** the spelling gate passes → emit a strip edit for `name`.
2. Whether or not it stripped, if `expr` is a record literal (named or
   anonymous) whose **own** record type is known, recurse into each `entries[i]`
   with that field's expected record type (rule 3 below). A `NamedRecord` always
   knows its own type via `name`, so it supplies field types for its children
   even when it could not itself be stripped.
3. Recurse structurally into all other children with `expected = .None`, except
   the flow-through positions (below), which forward `expected` unchanged.

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

All resolve a declared type to a record `TypeId?` via
`name → lookup_registered_type → record_info`. A non-record or generic result
yields `.None` and suppresses firing.

1. **Annotated let** — `let x: T = value`: `value` is checked with `expected` =
   resolved `T`. (`LintCtx` already resolves annotation names for
   `inline-record-copy`; reuse `type_name` + the lookup.)
2. **Return position** — a function whose `decl.return_type` resolves to record
   `R`: the body's tail expression and every `.Return(e)` in the body are
   checked with `expected = R`. Nested blocks/`if`/`case` in tail position
   inherit `R` through flow-through.
3. **Record-field value** — when descending into a record literal whose type `E`
   is known, field `f`'s value is checked with `expected` = the record tid of
   `record_info(E)`'s field-`f` `MonoType` (a `Named(tid, _)` whose `record_info`
   is `.Some`; anything else → `.None`).
4. **Call argument** — `Call(callee, args)`: resolve `callee` to a `FunctionSig`
   (`lookup_registered_function`, or the inherent-method form via
   `lookup_method` on the receiver's type), then check each `args[i]` with
   `expected` = param `i`'s record tid. Arity/overload mismatch or an
   unresolved callee → all args checked with `.None`.

### Flow-through positions

`expected` passes to these sub-expressions unchanged (they occupy the same
type slot as their parent): `if`/`else` branch tails, `case` arm bodies, `cond`
arm bodies, and block tail expressions.

## Safety argument

Each gate is independently sound; a failed gate only declines a fix.

- **Expected type is a concrete record.** Stripping to `.{ … }` is legal exactly
  where a concrete record type is expected. Generic (`Var`) positions never
  resolve to a record, so we never strip against a type variable.
- **No name-equality requirement.** We do *not* require `name` to string-match
  the expected type's name — that would miss type aliases
  (`type Foo = Bar`; `let x: Foo = Bar.{ … }`). Stripping stays sound because a
  `.{ … }` in a position expecting `Foo` resolves to `Foo` regardless of the
  original prefix, and the program was already well-typed.
- **Spelling gate** guarantees the edit removes exactly the prefix and nothing
  load-bearing (qualified paths, interspersed trivia are refused).
- **Well-typed input assumption.** Lint runs on a resolved/typechecked module
  (env is post-resolve, as `record-copy-helper` already relies on). Confirm this
  ordering during implementation; if lint can run pre-typecheck, gate the rule
  on typecheck success.

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

- **Unit fixtures** for each of the four sources plus each decline case: `:=`
  unannotated position (no fire), generic return/param (`Var` → no fire), enum /
  alias expected type edge (alias fires, enum does not), qualified path
  `pt.Point.{ … }` (spelling gate declines), and nested field/arg positions.
- **Boot self-application.** Boot source is dense with `T.{ … }` in annotated and
  return positions. Run the fixer across `boot/`, then:
  - `target/twk fmt` the touched files (expect no additional churn),
  - `make boot-test` green,
  - `make stage2` / self-host stays green and the rebuilt compiler is
    byte-identical (a behavior-preserving rewrite must not change codegen).
- Follow the `reference` for regenerating any bundled lint payload if the rule
  wiring requires it; otherwise this is boot-only.

## Sequencing (for the implementation plan)

1. `check_expr` skeleton + strip edit + spelling gate, wired for the **annotated
   let** source only; unit fixtures; report + fix flag.
2. Add **return position** (tail + `.Return`) with flow-through into arms/blocks.
3. Add **record-field** recursion (needs `record_info` field-type resolution).
4. Add **call-argument** resolution (`lookup_registered_function` +
   inherent-method `lookup_method`).
5. Boot self-application, fmt/boot-test/stage2 byte-identical validation.

Each step is independently shippable behind the same flag; the walk grows one
expected-type source at a time.
