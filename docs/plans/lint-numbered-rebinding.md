# Spec — `numbered-rebinding` lint + auto-fix (Pattern B)

> **For agentic workers:** implement the "Implementation plan (brief)" section
> task-by-task; each step is independently shippable and self-host-verified.

**Status: DRAFT / not started (review-hardened 2026-09-12).** Turns Pattern B of
[lint-rebinding-fixers.md](lint-rebinding-fixers.md) into a concrete design.
Adds a `twk lint` rule `numbered-rebinding` (report-only first) with a
machine-applicable auto-fix under `--fix-numbered-rebinding`.

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

**Selecting `base`.** Strip the sibling affix from `tmp` per the naming
heuristic (gate 4) to get a candidate name, then require `expr_uses_name(rhs,
base)` (gate 2) and same-block declaration (gate 3). If the strip is ambiguous
or no resulting name satisfies gates 2–3, there is no candidate. `base` is the
name we rebind onto.

The collapse `tmp := rhs` → `base = rhs` + rename(`tmp` → `base`) is
**behavior-preserving** exactly when every gate below holds. Each gate only
declines a fix when it fails; a declined fix is never wrong.

### Gates (all required)

1. **Fresh binding.** `stmts[i]` is `.Let(ls)`, `ls.is_rebind == false`,
   `ls.name == tmp`, and `tmp` is not `pub`.
2. **`base` is read by `rhs`.** `base` is a bare in-scope local with
   `expr_uses_name(ls.value, base) == true`. This makes `base = rhs` a genuine
   self-advance rather than an unrelated overwrite. (Sub-case: `rhs` is exactly
   `base` — a pure alias `tmp := base`; then no rebind is emitted, only the
   rename + seed deletion. See edits.)
3. **`base` is declared *earlier in this same block*** — there is a `.Let(bls)`
   at some index `k < i` in `block.stmts` with `bls.name == base` (`base` is
   not `pub`). **This gate is what actually makes the rewrite sound; gate 4
   alone does not** (see S1 below). `source_used_after` scans only the
   candidate's own block, but rebinding `base` inside a nested block
   *propagates the new value to the enclosing scope*. So if `base` were an
   outer-scope local or a function parameter, an outer read after this block —
   invisible to gate 4 — would silently change. Requiring `base` to be
   same-block-declared means every read of `base` that could observe the
   rewrite lives in this block and is therefore covered by gate 4. Rejects
   parameters and all outer-scope names.
4. **Naming heuristic — `tmp` is a numbered/suffixed sibling of `base`.** This
   is the precision knob that makes the rule a *ceremony* lint, not an
   aggressive rewrite (without it, every `y := f(x)` with dead `x` would fire).
   v1 predicate `is_numbered_sibling(tmp, base) Bool` fires when any holds:
   - `tmp == base + <digits>` (`acc`→`acc2`, `cache`→`cache2`), or
   - `tmp == base + "_" + <suffix>` (`state`→`state_next`), or
   - `tmp` is `base` with a leading `new_` / `next_` / `cur_` (`slots`→
     `new_slots`, and Shape 2's `acc`→`cur` via the `cur`-family list).

   Keep this predicate a single named function; the report-only pass exists to
   tune it against real boot hits before any edit is emitted.
5. **`base` is dead after the binding.** `source_used_after(block, i, base) ==
   false`. `rhs` reads `base` on line `i` itself, which this helper does not
   count (it scans from `i+1`). Combined with gate 3, this is airtight: every
   read that could observe the rewrite is in this block, and this gate proves
   none survives. `source_used_after` is itself conservative/fail-closed.
6. **`tmp` is forward-threaded, enumerable, not rebound.** Every occurrence of
   `tmp` after the binding is a *read* the span collector (below) can fully
   enumerate, and `tmp` is never itself rebound (`tmp = …`) in the forward
   region. Any `tmp` use inside a form the collector can't descend (closure,
   nested block) → decline. This makes a partial rename impossible.

Immutability makes gates 2/5 clean: Twinkle values are persistent, so rebinding
`base` cannot alias-mutate anything `rhs` captured — the only question is
whether the *old* `base` value is still read, which gates 3+5 together settle.

### Why the scary failure modes are loud, not silent

`checker.tw:5247` — a rebind `x = value` checks `value` against the **existing**
binding's type (locals are stored monomorphic, so there is no let-generalization
crack). That turns two of three failure modes into compile errors:

| Failure mode | Result | Loud? |
|---|---|---|
| Missed a `tmp` occurrence (partial rename) | dangling `tmp` → undefined-var | **compile error** |
| `rhs` type ≠ `base` type | rebind fails type check | **compile error** |
| Old `base` read after rebind (gates 3+5 wrong) | overwrote a live value | **silent** |

Only the third is silent, and it is what gates **3 + 5** jointly guard —
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
   (`rhs` is bare `base`), instead **delete the whole statement**: one edit over
   `[ls.span.start, next_stmt_or_tail.span.start)` with replacement `""`.
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
forms it can't descend), this returns `.Uncertain`.

**A pure expr-level mirror is not enough** — it would wrongly collect
assignment *targets* as renameable reads (`expr_uses_name` descends into *both*
sides of `.Binary(.Assign, …)`, so a forward `tmp = g(tmp)` would yield the LHS
`tmp` as a "read" and silently retarget the rebind). The collector is therefore
**statement-role-aware**, driven by a block-level wrapper over `stmts[i+1..]`
plus `block.tail` with these per-`Stmt` rules:

- `.Let(is_rebind, name==tmp)` or `.Expr(.Binary(.Assign, Ident(tmp), _))` in
  the forward region → **`.Uncertain`** (a forward rebind of `tmp`; gate 6
  declines). This is what actually enforces gate 6 — do not rely on the
  expr-level mirror for it.
- assignment/rebind **target** idents are never collected as reads; only the
  RHS and other read positions feed `expr_ident_spans`.
- `.For` / `.Defer` / any statement containing a nested `Block` or closure that
  mentions `tmp` → `.Uncertain` (mirrors the `_ => true` decline).

Any `.Uncertain` → the finding declines its edits (stays report-only). This
fail-closed discipline is what upholds gate 6. Because v1 never collects
assignment targets, Shape 2 — whose `cur` is an assignment target inside the
loop — is detect-only until a target-aware collector lands.

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
  - decline: `base` read after binding (gate 5); `tmp` used inside a closure
    (gate 6 `.Uncertain`); `tmp` rebound in forward region (gate 6); name not a
    numbered sibling (gate 4); `tmp` is `pub` (gate 1); **candidate in a nested
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

1. **`is_numbered_sibling` + candidate scan (report-only).** Add
   `lint_numbered_rebinding(block: Block) Vector<LintFinding>` covering Shape 1
   gates 1–6 (edits empty) and the Shape-2 loop-seed detector; wire it into the
   block-walk driver next to `lint_direct_rebinding`. **Gate 3 (same-block
   `base`) must be in from the start** — without it the report-only corpus
   includes unsound nested candidates that would later auto-fix wrongly. Add the
   `describe` rationale. Unit fixtures for every fire/decline row above,
   including the S1 nested-candidate regression. Ship, then dogfood on boot
   source and tune `is_numbered_sibling` against real hits.
2. **`expr_ident_spans` fail-closed collector** + block-level wrapper returning
   `IdentSpans`. Unit-test enumeration completeness and the `.Uncertain`
   decline path (closure/nested-block cases).
3. **Shape 1 auto-fix.** Emit the binding-rewrite edit (or whole-statement
   delete for the pure-alias sub-case) + the per-occurrence rename edits, gated
   on `IdentSpans.Complete`. Wire `--fix-numbered-rebinding` — this is **six
   coordinated touch points**, not one: `boot/main.tw` `.add_flag(...)`, and in
   `commands/lint.tw` the `FixFlags` record, `no_fixes()`, `wants()`,
   `fix_flags_from_args()`, and `any()`.
4. **Boot self-application + fmt/boot-test/stage2 byte-identical validation.**

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
