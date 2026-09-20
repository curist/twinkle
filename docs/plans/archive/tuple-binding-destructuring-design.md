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

  > **Scope reversal, acknowledged:** `docs/plans/archive/tuples.md` listed
  > `for (a, b) in …` as *out of scope*, preferring the bare `for a, b in xs`
  > (element+index) idiom. This design reverses that: `(a, b)` and `a, b` are
  > syntactically distinct (they mean destructure-element vs element+index), so
  > there is no conflict, and the loop forms fall out almost for free once
  > `LetPattern` exists (they reduce to the let case). The reversal is
  > deliberate, not an oversight.
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
  rebind later is mechanical; not needed for multi-return.) Today `(a, b) = e`
  parses as `Binary(Assign, Tuple, Tuple)` and falls through
  `lower_lvalue_assign` (`lower_core/lvalues.tw`, no `.Tuple` arm) to a confusing
  generic error; add a **clean checker-side rejection** ("tuple-pattern rebind
  not supported; use `:=` or `case`") as a small in-scope courtesy.
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

### Disambiguation (parser) — the fiddly part

At statement start, a leading `(` is ambiguous between a parenthesized/tuple
expression and a tuple-pattern binding LHS. **The naive fix is wrong**: boot's
`is_stmt_leader` (`parser.tw:89-110`) gates `parse_block`'s dispatch, and the
leader branch *always* appends to `stmts` — it has no path to the block's
`tail`. Promoting `.LParen` to a statement leader would therefore route every
tuple **tail expression** through the statement path and prevent it from ever
becoming the block's implicit-return `tail` — silently breaking working code
like `fn divmod(a, b) (Int, Int) { (a / b, a % b) }` (verified working today).

Correct approach: do the peek-and-reinterpret **inside `parse_block`'s existing
generic-expression fallback** (the path that already produces `tail`), not by
adding `.LParen` to `is_stmt_leader`. Parse the parenthesized expression
normally (`parse_expr_bp` naturally stops before `:=`/`:`, which are not infix
operators), then peek: if the next token is `:=` or the `:`/`=` of an annotated
binding, reinterpret the parsed tuple *expression* as a binding *pattern*
(validating each element is bindable); otherwise keep it as the expression it
already parsed as. **Module-level bindings** (`parse_top_level_stmt`, which
calls `parse_stmt` directly and has no block-tail fallback) need the equivalent
reinterpretation wired in separately — module-level `(a, b) := …` is a real
case. After `for` / `collect`, a leading `(` in the binder is unambiguously a
tuple pattern element (no tail conflict there). **Spike the parser
disambiguation first — on _both_ compilers**: boot's dual-path
(`parse_block` fallback + `parse_top_level_stmt`) handling, and stage0's
`is_let_binding` `(`-led lookahead + `ColonEq`-BP removal. These are the one
genuinely fiddly change on each side.

**stage0 has the analogous let-LHS disambiguation to solve too** — it is not
"already parsing." Today `is_let_binding` (`src/syntax/parser.rs:2194`) only
recognizes a binding when the *first* token is an `Ident` followed by `:=`/`:`,
so a `(`-led LHS is never routed to `parse_let_stmt` (which *does* call
`parse_pattern`, which *does* handle tuple patterns). Instead `(a, b) := e`
falls to the expression path, parses `(a, b)` as a tuple expression, treats `:=`
as an infix operator (`infix_binding_power` gives `ColonEq` a BP,
`parser.rs:2660`), and calls `token_to_binop(ColonEq)` → `unreachable!()`
(`parser.rs:2732`) — the ICE.

The fix is **not** to add a `ColonEq` arm to `token_to_binop`: that would stop
the panic but produce a `Binary` *expression* (there is no `BinOp` for `:=`),
which never becomes `Stmt::Let { pattern: Pattern::Tuple }` and so never reaches
the checker/lowering work below. The correct fix is to **extend `is_let_binding`
to detect a `(`-led tuple-pattern LHS** (lookahead scanning past the balanced
`)` for a trailing `:=`/`:`), and to **remove or gate the `ColonEq` infix BP**
so the let path claims it first. `for`/`collect` binders genuinely already parse
(their `parse_pattern` path handles `(a, b)`); only the let LHS needs this.
Unlike boot, stage0's `parse_block` collects only statements (no
implicit-return tail), so there is no tail-routing hazard — the stage0 lookahead
is simpler than boot's, but non-zero.

## Architecture

**Approach 1** (mirrors how tuples + case-arm destructuring were split across
the two compilers):

- **boot** carries a first-class AST node so the surface round-trips through the
  formatter, type-checks with precise diagnostics, and then lowers by
  desugaring to positional `._N` field-access bindings.
- **stage0** is **already pattern-generic in its AST** and needs no *new* AST
  nodes and no parse-time desugar. Its AST holds patterns — `Stmt::Let { pattern:
  Pattern }`, `Stmt::For { pattern, index_pattern: Option<Pattern> }`,
  `Collect { pattern, index_pattern }` (`src/syntax/ast.rs`) — `parse_let_stmt`
  and the `for`/`collect` binders already run `parse_pattern` (which handles
  `Pattern::Tuple`). But there is real stage0 **parser** work for the let LHS: a
  `(`-led binding is not currently recognized as a let and ICEs (see
  Disambiguation) — fix `is_let_binding` + the `ColonEq` BP. `for`/`collect`
  binders already parse. The bulk of the remaining stage0 work is **checker and
  lowering**, which have explicit `TypeError::UnsupportedFeature { note: "…for
  now" }` / `LowerError` placeholders on the pattern branches of
  `check_let_stmt`/`check_for_stmt` (`src/types/check.rs`) and `src/ir/lower.rs`
  — deliberate stubs awaiting this feature. So stage0's task is: the let-LHS
  parser fix, then implement pattern handling in those checker branches (note:
  `check_for_stmt` branches per iterable kind — Vector/String/Range/Iterator —
  so it is repeated; and `check_let_stmt`'s placeholder is a combined
  `Variant | Literal | Tuple` arm at `check.rs:2749` — split out `Tuple`) and in
  `lower.rs`'s `Stmt::Let`/`Stmt::For` lowering, mirroring boot's temp+`._N`
  mechanism. This makes the two compilers structurally *closer* (both do real
  pattern type-checking in place), not divergent.
- **`for` / `collect` reduce to the let case**: the element is bound to a fresh
  name and the destructure is applied at the top of the loop body. The only
  genuinely new machinery is the pattern-let; the loops are thin rewrites on top.

The lowering target in both compilers is a temp binding plus positional
`._N` field-access bindings — a path that already type-checks, lowers, and
codegens for tuples (`t._0`, `t._1`, …). This is a *third*, already-correct
codegen path (ordinary `emit_record_get` / `record_get_expr`), distinct from
the write path and the pattern-match read path touched by the recent Byte
tuple-field boxing fix — so this design does not reopen that issue.

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

No new AST nodes — stage0's AST is already pattern-generic (`Stmt::Let.pattern`,
`Stmt::For.pattern`/`index_pattern`, `Collect.pattern`/`index_pattern`), and
`parse_pattern` already produces `Pattern::Tuple`. The work is: the **let-LHS
parser fix** (`is_let_binding` `(`-led lookahead + `ColonEq`-BP removal — see
Disambiguation; `for`/`collect` binders already parse), plus **checker +
lowering** (see below). No new AST node and no parse-time desugar.

**Note the deliberate two-compiler encoding asymmetry:** boot adds a *new*
`LetPattern` Stmt variant (widening boot's `LetStmt.name: String` to a pattern
would ripple through ~dozens of `ls.name` consumers in `lint.tw`, `query/*.tw`,
etc.), while stage0 *already* generalized its `Stmt::Let` to carry a `Pattern`.
The two compilers therefore represent the "same" node differently; this is an
accepted, stated decision, not accidental drift.

## Type checking

### boot

- **`LetPatternStmt`**: resolve the RHS type (synth `value`, or, for the
  annotated form, resolve `ty` and check `value` against it), then run the
  existing `check_pattern(pattern, rhs_ty, …)` (`checker.tw:3573`) — which
  validates tuple arity, recurses into elements, and binds each identifier's
  type into scope. Guard with the existing `pattern_is_irrefutable`
  (`checker.tw:3735`) — the same Wildcard/Ident/Tuple-of-irrefutable check case
  exhaustiveness uses — rejecting refutable sub-patterns with a dedicated
  diagnostic. **Scope caveat:** call `check_pattern` the way `check_let`
  (`checker.tw:5349`) does — binding directly into the current frame, with **no**
  `push_scope`/`pop_scope`. Do *not* copy `check_case_arm`'s (`checker.tw:3446`)
  push/pop wrapper, or the destructured bindings would be discarded after the
  statement.
- **`for` / `collect`** with `element_pattern`: `check_for` already pushes one
  scope around the whole loop, so call `check_pattern` against the iterated
  element type directly (no extra scope), binding the destructured names into
  the loop-body scope; the index binder is unchanged.

### stage0

Replace the `Pattern::Tuple` placeholders in `check_let_stmt` and
`check_for_stmt` (`src/types/check.rs`) — currently
`TypeError::UnsupportedFeature { note: "…for now" }` — with real handling:
recurse the tuple pattern against the RHS / element type, bind each identifier,
and enforce irrefutability. `check_for_stmt` handles this per iterable-kind
branch (Vector/String/Range/Iterator), so the pattern logic is repeated (factor
a helper). This mirrors boot's checker, keeping the two in parity.

## Lowering

### boot

`lower_core` desugars each node into the shared model:

- **`LetPattern`**: emit `Let(temp, value)`, then walk the pattern; for each
  leaf `Ident` at positional path `p`, emit `Let(ident, <field-access chain for
  p from temp>)`. Wildcards emit nothing; nested tuples recurse (introducing an
  intermediate temp per nesting level as shown in the model). Build the field
  accesses with `record_get_expr` (`lower_core/helpers.tw:19`) +
  `tuple_field_types` (`lower_core/patterns.tw:7`, already imported) — the same
  primitive ordinary `.field` access uses. This is **new lowering code on an
  existing primitive**, deliberately *not* reusing `lower_pattern`'s `.Tuple`
  arm (which builds a `CorePattern.Variant` match). Bypassing the match avoids a
  wasted runtime tag check for an always-single-variant `TupleN` — a legitimate,
  slightly better choice than "the same lowering as case arms."
- **`for` / `collect`** with `element_pattern`: introduce a fresh element local
  as the loop binder, then prepend the same destructure bindings to the loop
  body before lowering the user body.

### stage0

Replace the `Pattern::Tuple` lowering placeholders in `src/ir/lower.rs`'s
`Stmt::Let` / `Stmt::For` handling (currently `LowerError::UnsupportedFeature`)
with the same temp+`._N` field-projection expansion.

No codegen changes in either compiler: everything bottoms out on tuple field
access (`emit_record_get` in stage0, the record-get path in boot), which already
codegens correctly and is unaffected by the Byte tuple-field boxing fix.

## Wiring checklist (boot `Stmt.LetPattern`)

Adding a new `Stmt` variant is only *partly* "compiler-caught, mechanical."
Exhaustive matches fail to compile until updated (good — they force the edit);
but several sites end in `_ => …` and would **silently no-op** on the new
variant, so they must be wired deliberately, not trusted to error:

- **Fail-to-compile (exhaustive) — will be caught:** `fmt/printer.tw`
  `format_stmt` / `stmt_span` (reuse the existing `format_pattern`,
  `printer.tw:2052`).
- **Silent `_ =>` fallthrough — MUST wire explicitly:** `checker.tw`
  `check_stmt` / `check_top_level_stmt` (`~5296`) and `query/hover.tw` (`~348`).
  A missed wire here type-checks with no diagnostic and no bindings, surfacing
  only later as the generic `"unsupported statement kind"` net in
  `lower_core/statements.tw:143`.
- **Other touch points:** `resolver.tw` (`~902`), `lower_core.tw` (module-let
  collection, `~274`/`~381`), `lower_core/statements.tw` (`~32`), `lint.tw`
  (~15 `Stmt` sites — rebinding/redundant-binding rules), `module_compiler.tw`
  (`~446`), `unused_imports.tw` (`~332`), and LSP `query/*.tw`.

The implementation plan should carry this as an explicit checklist rather than
relying on the compiler to flag every site.

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
  runtime — no wasmtime): the same programs, proving stage0's checker + lowering
  implementation matches boot.
- **fmt idempotence** (`fmt_suite.tw`): each surface form round-trips unchanged.
- **Disambiguation regression guards:** a tuple **tail expression**
  (`fn f() (Int, Int) { (1, 2) }`) still type-checks/runs after the parser
  change; a **module-level** `(a, b) := …` works; and the stage0 `(a, b) := …`
  ICE is gone (a stage0 parse/check test).
- **Self-host fixed point** (`make stage2`) is the hard acceptance gate — the
  two compilers must agree, and a later commit adopting `(a, b) :=` inside
  `boot/compiler/*.tw` for multi-return (a follow-on, not part of this plan)
  exercises it in anger.

## Risks / notes

- **Disambiguation is the one genuine risk** (`(…)` at statement start): it must
  live in `parse_block`'s tail-producing fallback, not `is_stmt_leader`, or it
  regresses tuple tail-expressions like the `divmod` example. Plus the separate
  module-level (`parse_top_level_stmt`) path and the stage0 `ColonEq` ICE. Spike
  all three first — see the Disambiguation section.
- **Two-compiler work is not symmetric.** boot: new AST node + checker + lowering
  + printer + the wiring checklist above. stage0: **a let-LHS parser fix**
  (`is_let_binding` `(`-led lookahead + `ColonEq`-BP removal — do *not* just add
  a `token_to_binop` arm, which mis-parses as an expression) plus filling in the
  checker + lowering pattern placeholders (the checker branch is per-iterable-
  kind, so more than one site). No new AST node either side; the compilers end up
  structurally closer than a "boot node vs stage0 desugar" framing suggests.
- **Formatter fidelity** is why boot uses a first-class node; a parse-time
  desugar would make `twk fmt` rewrite the surface into `__t := …; a := __t._0`.
- **Not affected: the Byte tuple-field boxing fix.** The design uses the ordinary
  field-read path (`emit_record_get` / `record_get_expr`), a third path distinct
  from the write and pattern-match-read paths that fix touched.
- **`make stage2` is the parity gate** but only proves *behavioral* agreement;
  the checker/lowering must be implemented on both sides for it to even reach a
  fixed point (the stage0 placeholders currently hard-error).
- **Tree-sitter regen needs Docker + a human test run** (project rule).
- **Follow-on adoption** (rewriting compiler multi-return call sites to
  `(a, b) :=`) is deliberately *not* in this plan; it is the motivating payoff
  and a natural next step once this lands.

Follow-on: adopt `(a, b) :=` at multi-return call sites in `boot/compiler/*.tw` to replace helper records and exercise the self-hosted compiler on its own new syntax.
