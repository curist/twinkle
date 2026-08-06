# Lint enhancement: `record-copy-helper` should catch copy-with-updated-field

**Status:** LANDED (2026-08-06). Implemented and self-host-verified in
`feat(lint): extend record-copy-helper to copy-with-update` (81796dc3), with the
compiler-wide sweep in `refactor(boot): rebind copy-with-update builders
directly` (7b873fdc).

**Outcome vs. this plan.** Detection was generalized past the plan's method-call
scope to *any* field updated from its own prior value — method call, binary/unary
op, or index — via a `spine_base` walk to the leftmost operand (the user asked to
cover general update patterns, not just `.set(...)`). The accept predicate flags
when every entry is a copy, an update, or a trivial pass-through and at least one
field is copied-or-updated, so single-field wrappers are caught and fresh
constructors are not. The R1 autofix handles the unambiguous shape (exactly one
updated field, all siblings verbatim copies, tail-position result), emitting the
`.method` rebinding shorthand for method updates and full text otherwise;
multi-update and pass-through-sibling shapes stay report-only. The
`--fix-record-copy-helper` flag was wired (it was advertised by the finding but
never registered, so even `--fix` had skipped this rule). All four wrapper modules
plus six other builder helpers now rebind directly.

---

*Original design below.*

## Problem

The L3 `record-copy-helper` lint (`boot/compiler/lint.tw`, ~1584) flags a function
whose result rebuilds a record by **copying** a source value's fields, nudging the
author toward direct field rebinding (`p.field = v`). But it only recognizes
*verbatim* copies. It misses the far more common — and more wasteful — shape: a
record literal that copies the source but **updates one field with an expression
derived from that field**.

Concrete miss (surfaced while wrapping the fixpoint maps): the four semantic wrapper
types define

```tw
pub fn set<T>(m: BlockMap<T>, k: Int, v: T) BlockMap<T> {
  BlockMap.{ inner: m.inner.set(k, v) }        // ← should be flagged
}
```

The canonical, lint-preferred form is direct rebinding:

```tw
pub fn set<T>(m: BlockMap<T>, k: Int, v: T) BlockMap<T> {
  m.inner = .set(k, v)
  m
}
```

`block_map.tw` was hand-fixed to the second form, but `local_map`/`local_set`/
`block_set` were **not flagged** by `twk lint --fix`, so they stayed inconsistent —
proof the detector has a blind spot, not that those three are fine.

## Root cause

`classify_entry(pname, entry)` returns one of `CopyFromP` (`f: p.f`), `Trivial`
(bare ident / shorthand / other field access), or `NonTrivial` (anything else). A
method call like `m.inner.set(k, v)` hits the catch-all `_ => .NonTrivial`
(`lint.tw:1624`). `is_copy_rebuild` then returns `false` on the first `NonTrivial`
entry (`lint.tw:1638`), so the record is never reported. The detector therefore
treats "update field `inner` from `m.inner`" as unrelated "real work" instead of the
copy-with-update it is.

There is also a structural gap for **single-field records** (all four wrappers are
`.{ inner: … }`): `is_copy_rebuild` requires `copies >= 1` verbatim `CopyFromP`
entries, which a one-field update never has — so even reclassifying the entry isn't
enough on its own.

## Goal

Extend L3 to flag a result record that reconstructs a source value `base` (a
param/local of the record's type) where every entry is a verbatim copy, a
same-field update, or a trivial pass-through, and **at least one entry updates a
field from `base`'s same field** — including the single-field wrapper case. Provide
an R1 autofix for the unambiguous shape.

## Design

### Detection

1. Add an `EntryKind` case `UpdatedCopy` (field derived from `base`'s same field).
   In `classify_entry`, before the `_ => .NonTrivial` catch-all, recognize an entry
   `f: <expr>` as `UpdatedCopy` when the expression's **root receiver/operand is
   `base.f`** — i.e. `base.f.method(...)` (method call on the same field) or
   `base.f <op> …` (arithmetic on the same field). Walk to the leftmost
   receiver/operand and check it is `Field(Ident(pname), f)` with `f == entry.name`.
2. Rework the accept predicate: flag when all entries ∈ {`CopyFromP`, `UpdatedCopy`,
   `Trivial`} **and** `UpdatedCopy count >= 1` (drop the `CopyFromP >= 1` requirement,
   which excluded single-field wrappers; require at least one *real* copy-or-update
   so genuinely-fresh constructors — all `Trivial`/literal — are not flagged).
3. Genuinely-fresh constructors (no entry references `base`) stay unflagged: no
   `CopyFromP` and no `UpdatedCopy` ⇒ predicate false.

### Autofix (R1) — unambiguous shape only

Rewrite is safe to auto-apply when: exactly **one** `UpdatedCopy` entry, all other
entries are `CopyFromP` verbatim copies of the same `base`, `base` is a simple
param/local, and the function result is exactly that record literal (tail
expression). Then:

```
T.{ …verbatim copies…, f: base.f.method(args) }   ⇒   base.f = .method(args)
                                                        base
```

using the `.method` rebinding shorthand (`spec.md:423`, "valid only at the head of a
rebinding RHS"). For an arithmetic update (`f: base.f + 1`) the shorthand does not
apply; emit `base.f = base.f + 1` only if still shorter/clearer, else leave
**report-only**. **Multi-field updates**, non-tail results, or a `base` that is not a
plain local ⇒ **report-only** (L-level), not auto-fixed — the direct-rebinding chain
(`base.a = …; base.b = …; base`) is correct but its safe synthesis is out of scope
for the first slice.

### Files

- `boot/compiler/lint.tw` — `classify_entry` (add `UpdatedCopy` + root-receiver
  walk), `is_copy_rebuild` (new predicate), `copy_finding` (message may name the
  updated field), and the fixer branch that emits the rebinding rewrite.
- `boot/compiler/lint_rules.tw` — no new rule id (extends `record-copy-helper`);
  update its description if it enumerates what it catches.
- Fixer (R1) wiring for the new autofix edit.

## Testing

Lint fixtures under the existing lint suite:

- **Positive (flag + autofix):** the wrapper `set`/`insert` shape
  (`T.{ inner: m.inner.set(k,v) }`), a multi-field record copying all fields and
  updating one (`P.{ x: p.x, y: p.y, z: p.z + 1 }` → `p.z = p.z + 1; p`).
- **Positive (flag, report-only):** two updated fields; `base` not a plain local.
- **Negative (no flag):** genuinely fresh constructor (`T.{ a: 0, b: mk() }`);
  update from a *different* value than the copies; a field updated from a *different*
  field (`f: base.g.op()`).
- **Regression:** re-run `twk lint --fix` on the four wrapper modules → all four
  converge to the direct-rebinding `set`/`insert`, byte-identical behavior.

## Acceptance

- `twk lint boot/main.tw` (and the type modules via the test entry) reports the
  three currently-missed wrapper modules; `--fix` rewrites them to match
  `block_map`.
- Existing `record-copy-helper` positives/negatives unchanged (no new false
  positives on fresh constructors).
- Boot suite green; lint fixer output idempotent.

## Notes / relationship

Extends L3 `record-copy-helper`; distinct from L5 `direct-rebinding` (the *alias*
form, `acc := prefix; acc = …`). See [project_linter_impl] history and
`docs/plans/archive/linter.md`.
