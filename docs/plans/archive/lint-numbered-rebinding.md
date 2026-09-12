# Spec — `numbered-rebinding` lint + auto-fix (Pattern B)

> **For agentic workers:** implement the "Implementation plan (brief)" section
> task-by-task; each step is independently shippable and self-host-verified.

**Status: IMPLEMENTED (2026-09-12), then renamed + broadened.** Turns Pattern B
of [lint-rebinding-fixers.md](lint-rebinding-fixers.md) into a concrete design.

> **Post-landing revision (2026-09-12):** the rule was renamed
> `numbered-rebinding` → **`redundant-rebinding`** (flag
> `--fix-redundant-rebinding`) and its scope broadened. The insight: the defect
> is *unnecessary* aliasing/rebinding, not numbering. A **pure alias**
> (`foo := bar`) now fires regardless of naming — it adds no information — while
> a **computed advance** (`acc2 := f(acc)`) still requires the numbered/suffixed
> sibling heuristic so meaningful renames (`sorted := sort(items)`) are left
> alone. Everything below is the original spec; read `numbered-rebinding` as
> `redundant-rebinding` and note gate 4 now applies only to the computed shape.

- **Task 1 — detection (report-only):** landed `e39c88e6` (gates 1–6, Shape 2
  detect-only, unit fixtures).
- **Task 2 — Shape 1 auto-fix:** `same_source_type`, gate-7/8 edit builder
  (`numbered_rebinding_edits`), `--fix-numbered-rebinding` flag wired across the
  six touch points, fixer + command-selection fixtures.
- **Task 3 — boot self-application:** dogfooded on `boot/`; the eight real hits
  all stay **report-only** (none carry the byte-identical annotations gate 7
  requires), so the fixer is a safe no-op on boot source and `make boot-test`
  self-host stays a byte-identical fixed point. Dogfooding note: the open
  underscore-suffix heuristic surfaces one mild false-ceremony match
  (`arm_body`/`arm` in `codegen/emit/match.tw`); left as-is because it stays
  report-only and tightening the suffix set risks dropping legitimate
  `state_next`-style successors.

## Goal

Collapse a freshly-named shadow accumulator that only threads a value forward
into an in-place rebind of the name it advances, so the numbered/suffixed
sibling disappears.

```tw
// Before                          // After
acc2 := f(op, acc)                 acc = f(op, acc)
g(rest, acc2)                      g(rest, acc)
```

The name `acc2` is ceremony: it holds "the next `acc`" and is threaded forward
once, after which the old `acc` is never read again. Rebinding `acc` in place
says the same thing without the numbered sibling.

## The two shapes

Both appeared in the motivating manual tidy-up (commit `c2410fa4`).

```tw
// Shape 1 — straight-line thread  (AUTO-FIX in v1)
acc2 := f(op, acc)             →    acc = f(op, acc)
g(rest, acc2)                       g(rest, acc)

// Shape 2 — loop seed            (DETECT-ONLY in v1, auto-fix deferred)
cur := acc                          for arm in arms {
for arm in arms {             =>      acc = h(arm.body, acc)
  cur = h(arm.body, cur)            }
}                                    acc
cur
```

Shape 2's auto-fix must rename an assignment **target** (`cur = …` inside the
loop) as well as reads, and delete the seed line; that lands as a follow-up
(see "Out of scope for v1"). Shape 2 detection ships in v1 so the report-only
pass surfaces both, which is where the naming heuristic (below) gets calibrated.

## Non-goals

- No cross-block dataflow. The rule reasons within a single `Block`'s statement
  list plus its tail — the same scope `direct-rebinding` already works in.
- No inference oracle. Like the sibling rules, it threads declared names and
  structure only, never `record_info` / per-expression inferred types.
- No rewrite that changes observable behavior.
- Patterns A and C of the umbrella doc are out of scope (A landed; C partly).

## Baseline / where this lives

Auto-fix rules live in `boot/compiler/lint.tw`; each walks the parsed AST
(`Module` / `Expr` / `Stmt` with `Span`s). An auto-fixable `LintFinding` carries
`edits: Vector<FixEdit>` where `FixEdit = .{ start, end, replacement }` (the type
is defined in `boot/lib/source/report.tw:33`) — source byte-range rewrites. Rule
rationale strings live in `boot/compiler/lint_rules.tw` (`describe`). This rule
is a per-block lint alongside `lint_direct_rebinding(block)` and hooks the same
block-walk driver — note that driver recurses into **nested** blocks (if/case
arms, `for` bodies, closures), so candidates can appear inside them; gate 3
(same-block `base`) is what keeps that sound.

Relevant existing helpers this rule **reuses verbatim**:

- `source_used_after(block, i, name) Bool` (lint.tw:1581) — "is `name` read
  anywhere in `block` after statement `i`?" Leans on `expr_uses_name`, whose
  `_ => true` fallback (lint.tw:1311) treats any compound form it can't fully
  descend as a use. This is the liveness gate, already fail-closed.
- `expr_uses_name` / `stmt_mentions_name` — conservative "does this expr/stmt
  read `name`" traversals.
- `is_bare_ident`, `path_root`, `is_ident_named`, `opt_str_eq`.

Relevant AST facts (verified in existing rules):

- `Stmt` variants used: `.Let(LetStmt)`, `.Expr(ExprStmt)`, `.For(ForStmt)`.
- `LetStmt = .{ name, name_span: Span, ty: TypeExpr?, value: Expr, is_rebind,
  is_pub, span }` (`ast.tw:120`). A new binding is `is_rebind == false`; a
  rebind `x = v` is `true`. `name_span` is the binding-rewrite edit anchor.
- `ExprStmt` exposes `.expr` (lint.tw:1368 does `case es.expr.kind`).
- `ForStmt = .{ pattern: String?, index: String?, iter: Expr?, condition:
  Expr?, body: Block, … }` (also `pattern_span`/`index_span`/`span`; used by
  `for_is_update_region`, lint.tw:1415).
- `Block = .{ stmts: Vector<Stmt>, tail: Expr?, span }`.
- `Expr = .{ id, kind, span }`; `ExprKind.Ident(String)`.

## Core mechanism — forward-thread collapse

For a candidate binding at statement index `i`:

```
tmp := <rhs>         // .Let, !is_rebind, name == tmp
```

**Selecting `base` (Shape 1).** Derive every lexical candidate allowed by gate
4: remove the maximal trailing digit run, take the prefix before each
underscore as the possible base for a suffixed sibling, and strip each matching
leading `new_` / `next_` / `cur_`. Deduplicate those strings, then filter them
through gates 2–3. Proceed only when exactly one distinct candidate remains;
zero or multiple surviving bases decline the finding. `base` is that unique
name. For example, `new_slots` produces lexical candidates `new` and `slots`;
if both are same-block locals read by the RHS, the candidate is ambiguous and
declined. Shape 2 does not use this selection algorithm: its seed must be the
bare alias `cur := base`, so the RHS selects `base` directly (see "Shape 2
detection").

The collapse `tmp := rhs` → `base = rhs` + rename(`tmp` → `base`) is
**behavior-preserving** exactly when every detection gate below and the
auto-fix type/trivia gates hold. A candidate can remain report-only when its
structure is clear but the fixer cannot prove a safe source rewrite.

### Gates (all required)

1. **Fresh binding.** `stmts[i]` is `.Let(ls)`, `ls.is_rebind == false`,
   `ls.name == tmp`, and `tmp` is not `pub`.
2. **`base` is read by `rhs`, exactly.** Run the same expression-role-aware,
   fail-closed identifier-span collector used for the forward region over
   `ls.value` for `base` and require `.Complete` with at least one span.
   Assignment targets never count as reads; v1 conservatively returns
   `.Uncertain` for any assignment expression in the candidate RHS. Do not use
   `expr_uses_name` for this proof: its fallback returns `true` for unknown
   compound forms even when the name is absent. This makes `base = rhs` a
   genuine self-advance rather than an unrelated overwrite.
   (Sub-case: `rhs` is exactly `base` — a pure alias `tmp := base`; then no
   rebind is emitted, only the rename + seed deletion. See edits.)
3. **`base` is declared *earlier in this same block*** — the nearest preceding
   `.Let(bls)` whose name is `base` has `!bls.is_rebind` and `!bls.is_pub`.
   Search backward from `i - 1` and use that exact declaration for the type gate
   too. An earlier rebind of a parameter or outer local is not a declaration
   and must not satisfy this gate. **This gate is what
   actually makes the rewrite sound; gate 4 alone does not** (see S1 below).
   `source_used_after` scans only the
   candidate's own block, but rebinding `base` inside a nested block
   *propagates the new value to the enclosing scope*. So if `base` were an
   outer-scope local or a function parameter, an outer read after this block —
   invisible to gate 5 — would silently change. Requiring `base` to be
   same-block-declared means every read of `base` that could observe the
   rewrite lives in this block and is therefore covered by gate 5. Rejects
   parameters and all outer-scope names.
4. **Naming heuristic — `tmp` is a numbered/suffixed sibling of `base`.** This
   is the precision knob that makes the rule a *ceremony* lint, not an
   aggressive rewrite (without it, every `y := f(x)` with dead `x` would fire).
   v1 predicate `is_numbered_sibling(tmp, base) Bool` fires when any holds:
   - `tmp == base + <digits>` (`acc`→`acc2`, `cache`→`cache2`), or
   - `tmp == base + "_" + <suffix>` (`state`→`state_next`), or
   - `tmp` is `base` with a leading `new_` / `next_` / `cur_` (`slots`→
     `new_slots`).

   Keep this predicate a single named function; the report-only pass exists to
   tune it against real boot hits before any edit is emitted. Exact `cur` is
   deliberately not part of this predicate: it has no lexical relationship to
   an arbitrary `base` and belongs only to Shape 2's stricter alias/loop/tail
   detector.
5. **`base` is dead after the binding.** `source_used_after(block, i, base) ==
   false`. `rhs` reads `base` on line `i` itself, which this helper does not
   count (it scans from `i+1`). Combined with gate 3, this is airtight: every
   read that could observe the rewrite is in this block, and this gate proves
   none survives. `source_used_after` is itself conservative/fail-closed.
6. **`tmp` is forward-threaded, enumerable, not rebound.** Every occurrence of
   `tmp` after the binding is a *read* the span collector (below) can fully
   enumerate, and `tmp` is never itself rebound (`tmp = …`) in the forward
   region. Any `tmp` use inside a form the collector can't descend (closure,
   nested block, record-field shorthand) → decline. At least one forward
   read is required; an entirely unused binding belongs to `unused-bindings`,
   not this rule. This makes a partial rename impossible.

Gates 1–6 define a sound Shape 1 recommendation. The span collector lands with
the report-only detector so the first implementation never has a looser,
short-lived interpretation of gates 2 or 6. Initially the complete spans prove
the candidate but are not attached as edits; auto-fix is enabled in the
following task. An `.Uncertain` result declines Shape 1 entirely.

### Additional auto-fix gates

Shape 1 receives edits only when both gates below also pass:

7. **The replacement preserves the binding type.** A pure alias (`rhs` is the
   bare `base`) needs no type gate because it deletes the alias rather than
   rebinding `base`. For a computed RHS, both the earlier `base` declaration
   and `tmp` binding must carry explicit type annotations whose source slices
   are byte-identical. This deliberately conservative syntactic proof means
   the original program already checked `rhs` at exactly the type the new
   rebind requires. Add `same_source_type(a: TypeExpr, b: TypeExpr, source:
   String) Bool` for this comparison; do not rely on a later build to catch a
   mismatched rebind. Unannotated or differently spelled-but-equivalent types
   remain report-only.
8. **The binding-prefix edit preserves trivia.** Slice
   `[ls.name_span.start, ls.value.span.start)` from `ctx.source`; if it contains
   `//`, keep the finding report-only. Line comments are Twinkle's only comment
   syntax. This prevents the coarse replacement used to remove an annotation
   from deleting a comment. Whitespace-only prefixes and comment-free explicit
   annotations are fixable.

### Shape 2 detection

Shape 2 is recognized by a separate, detect-only scanner rather than by
weakening Shape 1's sibling-name rule. It requires all of the following:

- a fresh, non-public, unannotated bare alias `cur := base`, with the temporary
  named exactly `cur` and `base` a bare identifier declared earlier in the same
  block;
- `base` dead after the seed, using `source_used_after`;
- one immediately following `for` whose header does not mention or shadow
  `cur`/`base`, whose body has no tail, and whose statements are exclusively
  rebinds of `cur` whose RHS reads `cur` but not `base`;
- the enclosing block tail is exactly `cur`, with no intervening statement.

This deliberately narrow form makes exact `cur` useful without treating every
`cur := x` alias as a ceremony finding. It reports with empty `edits` in v1.

Immutability makes gates 2/5 clean: Twinkle values are persistent, so rebinding
`base` cannot alias-mutate anything `rhs` captured — the only question is
whether the *old* `base` value is still read, which gates 3+5 together settle.

### Why the scary failure modes are loud, not silent

`checker.tw:5247` — a rebind `x = value` checks `value` against the **existing**
binding's type (locals are stored monomorphic, so there is no let-generalization
crack). The fixer must prove type compatibility up front; compiler failures are
defense in depth, not a safety gate:

| Failure mode | Result | Loud? |
|---|---|---|
| Missed a `tmp` occurrence (partial rename) | dangling `tmp` → undefined-var | **compile error** |
| Type gate implemented incorrectly | rebind fails type check | **compile error** |
| Old `base` read after rebind (gates 3+5 wrong) | overwrote a live value | **silent** |

The old-`base` case is silent, and it is what gates **3 + 5** jointly guard —
gate 3 confines every observing read to this block, gate 5 (`source_used_after`,
fail-closed) proves none survives. (An earlier draft credited gate 5 alone;
review found that unsound for candidates in a nested block whose `base` is an
outer local — the rebind escapes the block and gate 5 never sees the outer
read. Gate 3 closes exactly that hole.) The "loud" nets are **post-hoc**: the
fixer writes edits without recompiling (`apply_to_files` just `write_text`), so
a compile error surfaces only on the *next* build. The mandatory self-host
re-verify (below) is that build for boot self-application; a user who fixes and
does not rebuild would ship broken source at fix time if a gate were buggy —
which is why the silent gate (3+5) must be right, not merely the loud ones.
Gate 7 prevents the known type-changing case before edits are offered.

### The edits (Shape 1)

`FixEdit = .{ start, end, replacement }` (defined in
`boot/lib/source/report.tw:33`) — byte-range rewrites. Multi-edit findings are
already an established shape: `constant_fn_edits` emits disjoint edits, and
`apply_edits` (`commands/lint.tw`) applies a finding's edits atomically
(sorted descending by `start`, dropping the whole finding on any overlap). Two
kinds:

1. **Rewrite the binding** `tmp := ` → `base = `. One edit over
   `[ls.name_span.start, ls.value.span.start)` with replacement `"${base} = "`.
   Use `ls.name_span` (a real field on `LetStmt`) as the anchor — for an
   annotated candidate `tmp: T = rhs` this range also swallows the `: T`, which
   is correct (the rebind carries no annotation). For the pure-alias sub-case
   (`rhs` is bare `base`), instead **delete only the statement span**: one edit
   over `[ls.span.start, ls.span.end)` with replacement `""`. Leaving
   surrounding whitespace to `twk fmt` preserves comments between statements
   and also works when the binding is the final statement and the block has no
   tail.
2. **Rename each forward `tmp`** → `base`. One edit per occurrence:
   `FixEdit.{ start: occ.span.start, end: occ.span.end, replacement: base }`.

`twk fmt` runs after fixes and normalizes any spacing, so replacement spacing
need not be exact.

### The one new helper

`expr_uses_name` returns `Bool`; the rename needs spans:

```
fn expr_ident_spans(e: Expr, name: String) IdentSpans
type IdentSpans = { Complete(Vector<Span>), Uncertain }
```

Same traversal as `expr_uses_name`, collecting the `span` of every
`.Ident(name)` — but where `expr_uses_name` has `_ => true` (the compound
forms it can't descend), this returns `.Uncertain`. Record shorthand is also
`.Uncertain`: `.{ tmp }` stores the read as `RecordEntry.{ name: "tmp", value:
.None, … }`, so there is no identifier-expression span to rename. Rewriting the
entry would require synthesizing `tmp: base` to preserve the field name; v1
declines instead.

**A pure expr-level mirror is not enough** — it would wrongly collect
assignment *targets* as renameable reads (`expr_uses_name` descends into *both*
sides of `.Binary(.Assign, …)`, so a forward `tmp = g(tmp)` would yield the LHS
`tmp` as a "read" and silently retarget the rebind). The collector is therefore
**statement-role-aware**, driven by a block-level wrapper over `stmts[i+1..]`
plus `block.tail` with these per-`Stmt` rules:

- Any later `.Let` with `name == tmp`, whether a rebind or a fresh shadowing
  declaration, returns `.Uncertain`. This prevents the collector from crossing
  into occurrences owned by a different lexical binding.
- Any `.Binary(.Assign, _, _)` encountered at any expression depth returns
  `.Uncertain`, regardless of its target. This conservative v1 rule covers
  whole, field, and index rebinds of `tmp`, prevents assignment-path roots from
  being miscounted as reads, and applies equally to the candidate RHS and the
  forward region. A future role-aware collector may admit unrelated assignment
  expressions by walking only their true read positions.
- `.For` / `.Defer` / any statement containing a nested `Block` or closure that
  mentions `tmp` → `.Uncertain` (mirrors the `_ => true` decline).

Any `.Uncertain` declines the Shape 1 finding. A `.Complete` result must contain
at least one span. This fail-closed discipline is what upholds gate 6. Because
v1 never collects assignment targets, Shape 2 — whose `cur` is an assignment
target inside the loop — is handled by its separate detector and remains
detect-only until a target-aware collector lands.

## Rule identity & rollout

- Rule name `numbered-rebinding` (kebab, parallels `direct-rebinding`).
- Detection always on (report). Edits applied only under
  `--fix-numbered-rebinding`, matching the `--fix-inline-record-copy` /
  `--fix-redundant-record-prefix` convention.
- Rationale added to `lint_rules.tw` `describe`, surfaced by `twk lint --explain`.

## Relationship to existing rules

Sibling of `direct-rebinding`: that rule collapses a temp that **aliases** an
existing field path and gets path-updates + copy-back; this one collapses a temp
that holds a **freshly computed** advance of a name and threads it forward. They
share the per-block driver and the `source_used_after` / `expr_uses_name`
helpers. Shape 2's loop-seed form overlaps `direct-rebinding` structurally
(a `cur := base` seed, loop updates, tail return) but with free-function
accumulator updates (`cur = h(…, cur)`) rather than method-chain receiver
updates; generalizing `direct-rebinding`'s `expr_rooted_at` is an alternative
home for Shape 2's auto-fix and is called out in the follow-ups.

## Testing & validation

- **Unit fixtures** via the existing `findings(src)` / `builtin_env()` harness
  (`lint_pass_suite.tw`):
  - fire: Shape 1 straight-line thread; pure-alias sub-case; multiple forward
    reads of `tmp`.
  - report-only: computed RHS with missing or non-source-identical annotations
    (gate 7); comment in the replaced binding prefix (gate 8).
  - decline: RHS does not contain an exactly enumerated `base` read (gate 2);
    `base` read after binding (gate 5); no forward `tmp` read; `tmp`
    used inside a closure or record shorthand (gate 6 `.Uncertain`); `tmp`
    shadowed or rebound directly or through a field/index path in the forward
    region, or appears in any assignment expression (gate 6); name not a
    numbered sibling, or resolves to multiple sibling bases (gate 4);
    `tmp` is `pub` (gate 1); **candidate in a nested
    block whose `base` is a parameter or outer-scope local (gate 3)** — the S1
    regression test: original vs. rewritten must return the same value.
  - detect-only: Shape 2 loop seed reports a finding but carries no `edits`.
- **Boot self-application.** Run `--fix-numbered-rebinding` across `boot/`, then:
  - `target/twk fmt` the touched files (expect no extra churn),
  - `make boot-test` green,
  - `make stage2` / self-host stays green and byte-identical (a
    behavior-preserving rewrite must not change codegen).
- Boot-only: no Rust/stage0 change, no bundled-payload regen.

## Implementation plan (brief)

Each step is independently shippable; detection ships before any edit.

1. **Candidate scans and fail-closed span collector (report-only).** Add
   `is_numbered_sibling`, `IdentSpans`, the statement-role-aware
   `expr_ident_spans`/forward-region collector, and
   `lint_numbered_rebinding(block: Block, source: String) Vector<LintFinding>`.
   Shape 1 must satisfy gates 1–6, including exact non-empty `.Complete` span
   results for the RHS `base` read and forward `tmp` reads, but its
   finding carries empty `edits` in this task. Add the separate narrow Shape-2
   loop-seed detector specified above; wire both into the
   block-walk driver next to `lint_direct_rebinding`. **Gate 3 (same-block
   `base`) must be in from the start** — without it the report-only corpus
   includes unsound nested candidates that would later auto-fix wrongly. Add the
   `describe` rationale. Unit fixtures for every fire/decline row above,
   including the S1 nested-candidate regression and collector completeness /
   `.Uncertain` cases. Ship, then dogfood on boot source and tune
   `is_numbered_sibling` against real hits.
2. **Shape 1 auto-fix.** Add `same_source_type` and the comment-free prefix
   check. Emit the binding-rewrite edit (or whole-statement delete for the
   pure-alias sub-case) + the per-occurrence rename edits only when gates 7–8
   pass. Wire `--fix-numbered-rebinding` — this is **six
   coordinated touch points**, not one: `boot/main.tw` `.add_flag(...)`, and in
   `commands/lint.tw` the `FixFlags` record, `no_fixes()`, `wants()`,
   `fix_flags_from_args()`, and `any()`. Add fixer-output fixtures for computed
   annotated rewrites, pure-alias deletion in a no-tail block where the alias
   is read by a following final expression statement, intervening-comment
   preservation, multiple occurrence renames, and Shape 2 remaining
   report-only. Add command-selection fixtures proving the dedicated flag
   selects this rule, `--fix` includes it, and overlap handling drops a finding
   atomically rather than applying a partial rename.
3. **Boot self-application + fmt/boot-test/stage2 byte-identical validation.**

## Out of scope for v1 (sound omissions, possible follow-ups)

- **Shape 2 auto-fix** — needs a target-aware span collector (rename the
  `cur = …` assignment target inside the loop) and seed-line deletion; or fold
  into a generalized `direct-rebinding` `expr_rooted_at`. Detect-only in v1.
- **Cross-block threads** — a `tmp` bound in one block and read in a nested
  child block. Out; single-block only.
- **Non-sibling temps** — any single-use forward thread whose name is unrelated
  to the value it advances; deliberately excluded by gate 4 to keep this a
  ceremony lint.
- **Outer-scope / parameter `base`** — a candidate whose `base` is declared
  outside its own block (excluded by gate 3). Sound to handle only with an
  enclosing-scope liveness proof the per-block driver can't supply today.
