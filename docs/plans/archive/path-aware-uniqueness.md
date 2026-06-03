# Uniqueness Optimizer: Value-Semantics Soundness Fix (+ path-aware future work)

**Status: COMPLETE / ARCHIVED.** The soundness work is DONE (commit `e658523`).
Both optional perf levers (dict path-aware, vector-append builder-threading) were
measured and rejected — see "Measurement check" and "Closeout" below. This
document records what was actually found and fixed, and why no further
optimization is warranted absent a proving workload.

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

## Measurement check (2026-06-03): the numbers say stop

Before opening item 1 or 2, we gathered the data the plan demands. Verdict:
**no measured compile-time justification for either.**

Census (stage0 optimizer over `boot/main.tw`): `TOTAL COW remaining = 1695`
(passes the downward ratchet; one below the 1696 baseline, within noise).

Boot-side optimizer split — stable at the post-fix numbers, confirming boot has
not drifted: record-shell reuse `in_place=true` = 132, dict in-place (Fn46) = 75,
dict COW (Fn38) = 397.

Compile-time breakdown (boot compiling `boot/main.tw`, ~3.6 s wall):

| phase | ms |
|---|---:|
| compile_modules (frontend) | 1404 |
| emit_module | 365 |
| optimize (of which **uniqueness = 82**) | 313 |
| prepare_backend | 269 |
| verify | 244 |
| emit_wasm_binary | 229 |

Two reasons the path-aware items don't pay:

1. The uniqueness pass is **~2 % of wall-clock** (82 ms / 3.6 s); the frontend's
   `import_merge` alone (339 ms) dwarfs it. There's no bottleneck here to attack.
2. The remaining COW is dominated by `VECTOR_APPEND`, **not** the dict/record
   copies items 1 & 2 target. The COW-heaviest functions
   (`emit_resume_segment`, `anyref.emit_unbox_from_anyref`, the `closure_emit.*`
   trampolines) are almost pure `VECTOR_APPEND`, which is already O(1)-amortized.
   The dict/record copies are scattered thin; item 1 recovers *at most* ~32 of
   them — a count this census confirms is on no hot concentration.

The plan's "don't do it without a workload that proves it matters" guidance is
therefore confirmed by measurement, not just asserted. If COW ever does start to
hurt, the lever shows up in the per-function census as a dict/record-heavy
function climbing the list — that breakdown is the instrument to watch.

### Follow-up spike: the `VECTOR_APPEND` case also fails the cost test

Since `VECTOR_APPEND` dominates the remaining COW count, we spiked whether it
was worth its own optimization pass. It is not.

**Why the cheap appends are already gone, and the rest are unreachable.**
`VECTOR_APPEND` has no in-place variant — outside the builder family it always
COWs the tail leaf, so the count can only drop by converting append chains to
builders. The straight-line and simple-loop chains are *already* rewritten
(`builder_region.tw` / `loop_builder.tw`); e.g. `emit_resume_segment` shows
`BUILDER_NEW/PUSH/FREEZE` firing alongside the 38 appends it can't reach. Those
remaining appends are a **threaded-accumulator** pattern: `buf` is passed into
helper calls and returned (`buf = emit_unbox_from_anyref(…, buf)`,
`buf = save_hoisted_to_frame(…, buf)`) and built across loops and `case`/`if`
arms. That violates every rewriter gate (base-read-in-gap, single straight-line
run, intra-function). Capturing it would mean threading a mutable builder through
all ~727 codegen append sites — a large, inter-procedural, high-risk refactor.

**What it would buy (microbench, 1M ops, `hyperfine`).** 1M COW helper-appends =
196.9 ms vs 1M builder pushes = 182.3 ms — a ~14.6 ms delta, ~15 ns per append
(the rest is fixed compile+startup). Compiling `boot/main.tw` emits on the order
of ~1M instructions total (2.8 MB wasm), much of it already builder-ized, so the
**absolute ceiling** — every emitted instruction being an un-rewritten COW
append, which it is not — is ~15 ms of 3600 ms (**<0.5 %**), under the phase
noise we already measure. High risk × cross-cutting effort for a sub-0.5 %
ceiling: not worth it.

## Closeout

Soundness is fixed and merged; the census harness is restored as the regression
instrument. Both remaining perf levers — dict path-aware coverage and
vector-append builder-threading — were measured and **neither survives the cost
test**. This plan is archived as complete. Reopen only with a concrete workload
where the per-function census shows a hot path climbing the COW list.

## Note on the draft implementation plan

The earlier task-by-task plan in this file (restore census → add deep-ownership
coverage → mirror to Rust) assumed the analysis was sound-but-conservative. The
code review correctly flagged that several of its positive fixtures would have
required exactly the unsound rewrite that turned out to be a live bug. That plan
has been superseded by the soundness fix above; its salvageable parts are items
1–3 here.
