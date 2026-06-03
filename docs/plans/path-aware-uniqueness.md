# Uniqueness Optimizer: Value-Semantics Soundness Fix (+ path-aware future work)

**Status: the soundness work is DONE** (branch `fix/uniqueness-alias-inplace`,
commit `e658523`). This document records what was actually found and fixed, and
what remains as *optional, measured-as-low-value* future optimization.

The original framing of this plan — "the uniqueness analysis is sound but too
conservative, extend its coverage" — turned out to be backwards for the cases
that mattered. Investigation (prompted by a code review of the draft plan)
revealed the analysis was **unsound** in two places: it rewrote `m[k] = v` to an
in-place dict mutation while the dict was still observed through a live alias,
breaking the language's immutable value semantics.

## What was wrong (and is now fixed)

### Bug 1 — copy-bind ownership transfer (both compilers)

A copy-bind `b := a` transferred ownership to `b` unconditionally. When `a` was
updated first (so it carried ownership), then aliased, then `b` updated, the
update on `b` mutated the dict still aliased by the live `a`:

```tw
a: Dict<String, Int> = Dict.new()
a["x"] = 1
b := a
b["y"] = 2
// before the fix: a.len() == 2 (corrupted); now: a.len() == 1
```

Fix: guard the `AInit`/`AAssign` copy-bind transfer with source liveness — only
move ownership when the source is dead after the bind; a live source is a true
alias, so neither side uniquely owns the value. Landed in both
`boot/compiler/opt/uniqueness.tw` and the stage0 reference
`src/opt/uniqueness.rs`.

### Bug 2 — field-borrow on a shell-fresh record (boot only)

Boot's field-borrow grant fired on a record with mere **shell** ownership. A
record built from a parameter has a fresh shell but fields that still alias the
caller's dicts, so the field-in-place corrupted the caller:

```tw
fn record_via_param(ctx: Ctx, v: Int) Ctx {
  ctx.types["k"] = v   // first update: COW (correct)
  ctx.spans["k"] = v   // before the fix: in-place on a field aliasing the caller
  ctx
}
```

This was the documented "shell-fresh ≠ deep-owned" gap
(`archive/boot-uniqueness-deep-ownership.md`). **Stage0 Rust never had it** — it
keeps these field updates COW and only reuses the record shell. The boot R4b
grant was uniquely more aggressive, and that extra aggression was the bug.

Fix: track **deep ownership** in boot — a record is `Deep` only when *all* its
field values are deep-owned (`ARecord` / `ARecordUpdate` propagate `Deep`
conditionally), and the field-borrow grant requires `Deep`, not shell. This
aligns boot with the already-sound stage0 behavior.

## The invariant these fixes enforce

**Deep ownership, not shell freshness, licenses in-place field mutation**, and a
copy-bind only moves ownership when the source is provably dead. A fresh record
shell whose fields alias the caller is never eligible for field in-place.

## Regression guards (must stay green)

- `boot/tests/suites/value_semantics_suite.tw` — runtime: dict alias, multi-alias,
  record-field-via-parameter, plus vector / alias-before controls.
- `tests/opt/dict_alias_live_update_not_rewritten.tw` + the stage0 structural
  test `opt_dict_alias_live_update_not_rewritten` (with the new dict-in-place
  detection helper `count_call_to`).
- `tests/opt/field_borrow_dict.tw` — helper-returned record stays COW at the dict
  level; runtime result correct.

## Measured cost (boot/main.tw, pre-fix vs fixed)

| | pre-fix | fixed | Δ |
|---|---:|---:|---|
| dict in-place | 107 | 75 | −32 |
| dict COW | 365 | 397 | +32 |
| record-update shell reuse | 132 | 132 | 0 |
| `optimize` phase | 320.6 ms | ~313 ms | none (noise) |
| wall-clock (`hyperfine`) | — | 3.619 s ± 0.015 | — |

The fix converted 32 unsound dict-in-place rewrites to COW with **no measurable
compile-time impact**; record-shell reuse is fully preserved.

## Optional future work (data shows low value — revisit only with numbers)

1. **Deep-fresh-helper ("rule 3").** Mark a helper's result `Deep` when it
   returns a record whose fields are all deep-fresh (strengthen
   `tail_is_fresh_record` to verify field provenance), so helper-returned records
   like `make_ctx()` regain dict-field-in-place soundly. This would recover *at
   most* the ~32 dict ops above — measured as zero compile-time benefit — at the
   cost of re-introducing boot-vs-stage0 divergence and the complexity that
   caused Bug 2. **Not worth it now.** If a future workload shows it mattering,
   do it here with the census harness below.

2. **Restore the auditable COW census harness.** `tests/cow_analysis.rs` existed
   and was deleted in `639ef58`; recover it (`git show 639ef58^:tests/cow_analysis.rs`),
   add the R3 `VECTOR_BUILDER_EXTEND` family and a downward-only ceiling
   assertion. It compiles through the backend pipeline (not the slow
   interpreter) and gives any future optimizer work a before/after number.

3. **Broader path-aware coverage.** The original idea of generalizing the
   field-borrow/last-use recognizers into one access-path analysis remains
   available, but is a *performance* lever, not a correctness one. Gate it on a
   measured compile-time need; keep the deep-ownership invariant above as the
   non-negotiable soundness boundary.

## Note on the draft implementation plan

The earlier task-by-task plan in this file (restore census → add deep-ownership
coverage → mirror to Rust) assumed the analysis was sound-but-conservative. The
code review correctly flagged that several of its positive fixtures would have
required exactly the unsound rewrite that turned out to be a live bug. That plan
has been superseded by the soundness fix above; its salvageable parts are items
1–3 here.
