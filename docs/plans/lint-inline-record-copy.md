# Lint rule: `inline-record-copy`

## Motivation

Reconstructing a record by copying most of its fields from a single source value
is a maintenance hazard: every future field of that record type must be threaded
by hand, and a forgotten field is a silent bug. This was exactly the shape of the
`emit_loop_op` `EmitCtx` reconstruction — `loop_ctx := EmitCtx.{ registry:
ctx.registry, …, label_stack: new, loop_depth: ctx.loop_depth + 1 }` — where
adding `current_pf` to `EmitCtx` required remembering to thread it through the
manual copy. The idiomatic Twinkle form is to rebind the source directly
(`ctx.label_stack = …`), since records are immutable and assignment is rebinding.

The existing `record-copy-helper` rule only catches this when it is the **return
expression of a `with_*` function**, and it bails on any computed field (its
`is_copy_rebuild` treats arithmetic/literals as `NonTrivial`). The inline
binding form (`x := T.{ …copy of src… }`) with a computed changed field is
uncovered — and is the shape that actually occurred.

## Scope (v1)

Flag a **local binding** `x := T.{ … }` (or `x: T = .{ … }`) whose record literal
rebuilds a single source value by copying a **majority** of its fields verbatim.

Out of scope (deferred): record literals in return/argument positions (they drag
in the liveness reasoning of the separate "alias-then-mutate-then-pass" pattern),
and any auto-fix (`twk lint` is report-only; `twk fix` is separate).

## Detection (pure AST — no `env`, no type information)

Trigger: a `Let` statement whose value is a `Record` or `NamedRecord` literal with
**≥ 2 entries**.

Classify each entry against a candidate source identifier `S`:

- **copy of `S`**: the entry is `field: S.field` where `S` is a bare identifier
  and the read field name equals the entry name (the `CopyFromP` shape the
  `record-copy-helper` already uses, generalized from "the param `pname`" to "any
  identifier").
- **changed / other**: everything else — a computed value, a literal, a
  cross-field read (`f: s.g`), a shorthand pun (`{ field }`), or a copy from a
  *different* source.

Pick the dominant source `S*` = the identifier that appears most often as a
verbatim copy. **Flag** when its copies are a strict majority of all entries:

```
copies(S*) * 2 > total_entries
```

This ignores small literals that merely reuse one field of `src` while genuinely
building something new, and multi-source merges (no single dominant source).

## Why no type check

The copies are same-name (`f: src.f`). For the literal to typecheck, `src.f` must
have the type of `T`'s field `f`; holding across a majority of fields already
implies `src` has `T`'s shape. A different type coincidentally sharing every
copied field name and type is essentially never real code, and the lint is
report-only — a rare miss is a harmless spurious hint, never a bad rewrite. So the
rule needs no `ResolvedEnv` and no local-type tracking.

## Message

```
`x` rebuilds `S*` by copying its fields; rebind `S*` directly (`S*.field = v`)
instead of constructing a copy
```

Rule id `inline-record-copy`, span at the binding. Add a `describe()` entry in
`lint_rules.tw` with an Avoid/Prefer example.

## Implementation notes

- Extract "is this entry a verbatim same-name copy of identifier `X`" out of
  `classify_entry` (currently hardwired to the param name `pname`) into a shared
  helper, so `record-copy-helper` and the new rule use one definition. No behavior
  change to the existing rule.
- Hook the new detection where `Let` statements are visited (`lint_stmt` /
  `lint_block`), alongside `lint_direct_rebinding`.

## Tests (lint suite)

Positives:

- majority copies from one source plus a computed changed field
  (`loop_depth: ctx.loop_depth + 1`).
- all entries are copies from one source (pure alias reconstruction).

Negatives:

- minority copies (only one field of `src` reused while building something new).
- multi-source merge (`{ a: x.a, b: y.b }`).
- cross-field shuffle (`{ f: s.g }`).
- all-computed / no verbatim copies.
- single-entry record.
