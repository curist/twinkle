# Tuple binding-destructuring — design

Extends tuple destructuring from `case` arms (already shipped) to **binding
positions**: `let`-style bindings, `for` loops, and `collect` comprehensions.
This is item **A** from `docs/plans/tuple-followups.md` and the headline
fast-follow sketched in `docs/plans/archive/tuples.md`.

## Motivation

The headline is multi-return ergonomics:

```tw
(q, r) := divmod(17, 5)          // q = 3, r = 2
```

Today the same requires reading `x._0` / `x._1` off a synthesized temp, which
is why `boot/compiler/*.tw` still uses explicit helper records for multi-return
rather than tuples. Binding-destructuring is the gate that lets the compiler's
own source adopt tuples. The loop forms extend the same convenience to
iteration over tuple-valued collections.

## Scope

**In scope:**

- **Let binding**, two forms:
  - `(a, b) := expr` — inferred declaration.
  - `(a, b): (Int, Int) = expr` — annotated declaration; the annotation is the
    expected type for `expr`.
- **`for` loop** element binder: `for (a, b) in xs { … }` and
  `for (a, b), i in xs { … }` (destructured element + optional index).
- **`collect` comprehension** element binder: `collect (a, b) in xs { … }` and
  `collect (a, b), i in xs { … }`.
- **Patterns** (all irrefutable): identifiers, `_` wildcards, and nested tuple
  patterns (`((a, b), c)`), arity 2–4 at each level. Reuses the existing
  `PatternKind.Tuple` / `check_pattern` machinery from case arms.

**Out of scope:**

- **Refutable patterns** in binding position — variant (`.Some(x)`),
  qualified-variant, or literal (`0`, `"s"`) sub-patterns. These are a
  **compile error** (use `case`). Making bindings total is what keeps them from
  needing a fallthrough.
- **Tuple-pattern rebind** — `(a, b) = expr` reassigning two already-bound
  names. Only *declaration* forms (`:=`, `: T =`) are supported in v1. (Adding
  rebind later is mechanical; not needed for multi-return.)
- **Function-parameter tuple patterns** (`fn f((a, b): T)`) — deferred (a
  separate `Param`-grammar edit; decided out of this design's scope).
- Arity > 4, 1-tuples, `()` unit — inherited from the base tuple design.

## Surface syntax & semantics

### Evaluation

The RHS is evaluated **once**. `(a, b) := f()` binds a temp to `f()`'s result,
then projects the elements — so a side-effecting or expensive RHS runs a single
time.

### Refutability

The binder pattern must be irrefutable: only `Ident`, `Wildcard`, and nested
`Tuple`. A `Variant` / `QualifiedVariant` / `Literal` sub-pattern anywhere in
the binder is rejected with a diagnostic pointing at the offending sub-pattern
("refutable pattern not allowed in a binding; use `case`"). Wildcards are
allowed and bind nothing (`(_, b) := p` binds only `b`); an all-wildcard binder
(`(_, _) := p`) is legal (evaluates the RHS, binds nothing).

### Arity / type errors

Arity or element-type mismatches surface against the RHS's tuple type
(`(a, b, c) := pair_of_two` → arity error; `s: String = …` bound where the
element is `Int` → type mismatch), reusing the case-arm pattern-checking
diagnostics.

### Disambiguation (parser)

At statement start, a leading `(` is ambiguous between a parenthesized
expression statement and a tuple-pattern binding LHS. Resolution: parse the
parenthesized comma-list, then peek the following token — if it is `:=` or `:`
(annotated) / the `=` of an annotated binding, reinterpret the parsed
comma-list as a binding pattern (validating each element is bindable);
otherwise it is an ordinary expression statement. After `for` / `collect`, a
leading `(` in the binder is unambiguously a tuple pattern element.

## Architecture

**Approach 1** (mirrors how tuples + case-arm destructuring were split across
the two compilers):

- **boot** carries a first-class AST node so the surface round-trips through the
  formatter, type-checks with precise diagnostics, and then lowers by
  desugaring.
- **stage0** desugars at parse time to existing constructs (it has no formatter
  and is the bootstrap/reference), exactly as it already handles tuple literals.
- **`for` / `collect` reduce to the let case**: the element is bound to a fresh
  name and the destructure is applied at the top of the loop body. The only
  genuinely new machinery is the pattern-let; the loops are thin rewrites on top.

The lowering target in both compilers is a temp binding plus positional
`._N` field-access bindings — a path that already type-checks, lowers, and
codegens for tuples (`t._0`, `t._1`, …).

### Desugaring model (shared mental model)

```tw
(a, b) := e
// ≡
__t := e
a := __t._0
b := __t._1

((a, b), c) := e
// ≡
__t := e
__t0 := __t._0
a := __t0._0
b := __t0._1
c := __t._1

(_, b) := e         // wildcard binds nothing
// ≡
__t := e
b := __t._1

for (a, b), i in xs { body }
// ≡
for __e, i in xs { (a, b) := __e   body }

collect (a, b) in xs { expr }
// ≡
collect __e in xs { (a, b) := __e   expr }
```

Annotated form pushes the annotation onto the temp:

```tw
(a, b): (Int, Int) = e
// ≡
__t: (Int, Int) = e
a := __t._0
b := __t._1
```

Fresh names (`__t`, `__e`, …) are compiler-generated and cannot collide with
user identifiers.

## AST changes

### boot (`boot/compiler/ast.tw`)

- **New `Stmt` variant** for the pattern binding, leaving the existing
  `Let(LetStmt)` path untouched (zero ripple through current single-ident
  consumers):

  ```tw
  LetPattern(LetPatternStmt)
  // LetPatternStmt = .{ pattern: Pattern, ty: TypeExpr?, value: Expr, span: Span }
  ```

  `pattern.kind` is always `.Tuple(...)` at the top level (the parser only
  produces this node for a tuple LHS). No `is_pub` / `is_rebind` — pattern-lets
  are always non-pub declarations in v1.

- **`ForStmt` / `CollectExpr`** gain an optional tuple element binder alongside
  the existing `pattern: String?`:

  ```tw
  element_pattern: Pattern?   // Some(tuple) when the element binder destructures;
                              // None → use the existing `pattern: String?`
  ```

  `index: String?` is unchanged. When `element_pattern` is `Some`, `pattern`
  is `None`. Keeping both fields avoids reworking every existing single-ident
  `for`/`collect` consumer.

### stage0 (`src/`)

No new AST nodes. The parser desugars a tuple-pattern binder directly into the
existing let / field-access / `for` / `collect` constructs (per the desugaring
model above). The stage0 parser accepts only `Ident` / `_` / nested-tuple
elements in a binder position, so refutable patterns are a structural parse
error there.

## Type checking (boot)

- **`LetPatternStmt`**: resolve the RHS type (synth `value`, or, for the
  annotated form, resolve `ty` and check `value` against it), then run the
  existing `check_pattern(pattern, rhs_ty, …)` — which validates tuple arity,
  recurses into elements, and binds each identifier's type into scope. A
  pre-pass rejects refutable sub-patterns (`.Variant` / `.QualifiedVariant` /
  `.Literal`) with a dedicated diagnostic.
- **`for` / `collect`** with `element_pattern`: check the pattern against the
  iterated element type (same `check_pattern` + irrefutability pre-pass),
  binding the destructured names into the loop-body scope; the index binder is
  unchanged.
- stage0 needs no special checker work — it only ever sees ordinary lets +
  field access after its parse-time desugar.

## Lowering (boot)

`lower_core` desugars each node into the shared model:

- **`LetPattern`**: emit `Let(temp, value)`, then walk the pattern; for each
  leaf `Ident` at positional path `p`, emit `Let(ident, <field-access chain for
  p from temp>)`. Wildcards emit nothing; nested tuples recurse (introducing an
  intermediate temp per nesting level as shown in the model). Reuses the
  existing tuple field-projection lowering — no new Core pattern-matching path.
- **`for` / `collect`** with `element_pattern`: introduce a fresh element local
  as the loop binder, then prepend the same destructure bindings to the loop
  body before lowering the user body.

No codegen changes: everything bottoms out on tuple field access, which already
codegens.

## Formatter (boot)

`boot/compiler/fmt/printer.tw` handles the new surface so `twk fmt` is
idempotent and preserves the written form:

- `LetPattern`: print `(a, b) := …` and `(a, b): (Int, Int) = …`, including
  nested patterns and `_`.
- `for` / `collect` with a tuple element binder: print `for (a, b) in …`,
  `for (a, b), i in …`, and the `collect` equivalents.

Because boot's formatter prints from the AST, the first-class node (not a
parse-time desugar) is what keeps the surface syntax from being rewritten into
`__t := …; a := __t._0` on format.

## Grammar & tree-sitter

- **`tree-sitter-twinkle/grammar.js`**: the let-binding LHS accepts a tuple
  pattern; the `for` / `collect` element binder accepts a tuple pattern.
  Regenerate `src/parser.c`, `grammar.json`, `node-types.json`, and rebuild
  `tree-sitter-twinkle.wasm` (Docker + a **human** `tree-sitter test` run — per
  project rule, the agent does not run tree-sitter tests). Commit `grammar.js`,
  the regenerated `src/`, and the wasm together.
- **`docs/grammar.ebnf`**: update the `LetStmt`, `ForStmt`/for-binder, and
  `Collect` productions to admit a tuple pattern LHS/binder.
- **`docs/spec.md`**: document the binding, `for`, and `collect` forms and the
  irrefutability rule.

## Testing & acceptance

- **boot checker suite** (`checker_suite.tw`): arity match/mismatch, nested
  patterns, wildcards, element-type flow, refutable-sub-pattern rejection, the
  annotated form.
- **boot codegen integration** (`codegen_integration_suite.tw`, via
  `run_exit_code`): end-to-end execution of `:=` (flat + nested + wildcard),
  `for (a, b) in`, `for (a, b), i in`, and the `collect` forms — asserting
  computed values, including single-evaluation of a side-effecting RHS.
- **stage0 run-tests** (`tests/tuple_pattern_run_test.rs`, now on the Deno
  runtime — no wasmtime): the same programs, proving stage0's parse-desugar
  matches.
- **fmt idempotence** (`fmt_suite.tw`): each surface form round-trips unchanged.
- **Self-host fixed point** (`make stage2`) is the hard acceptance gate — the
  two compilers must agree, and a later commit adopting `(a, b) :=` inside
  `boot/compiler/*.tw` for multi-return (a follow-on, not part of this plan)
  exercises it in anger.

## Risks / notes

- **Two-compiler parity is the real work**, not new semantics: the parser
  changes (disambiguation + binder patterns), the boot AST node + checker +
  lowering + printer, and the stage0 parse-desugar, all threaded through the
  exhaustive matches the new node touches (compiler-caught, mechanical).
- **Formatter fidelity** is why boot uses a first-class node rather than a
  parse-time desugar; a desugar would make `twk fmt` rewrite the surface.
- **Disambiguation** (`(…)` at statement start) is the one genuinely fiddly
  parser bit — spike it first (parse-comma-list-then-peek, converting expr→pattern
  on a trailing bind token).
- **Tree-sitter regen needs Docker + a human test run** (project rule).
- **Follow-on adoption** (rewriting compiler multi-return call sites to
  `(a, b) :=`) is deliberately *not* in this plan; it is the motivating payoff
  and a natural next step once this lands.
