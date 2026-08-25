# Shared-wrapper owned specialization

**Status:** Implemented / archived
**Track:** Sound uniqueness & mutable lowering → codegen (Phase 8G variant
specialization). See [sound-uniqueness/codegen/README.md](../sound-uniqueness/codegen/README.md)
and [performance/README.md](../performance/README.md) (runtime priority #1).

## One-line problem

Ordinary persistent-`Vector` code that performs its owned indexed update **through
a prelude/user wrapper function** (`xs = xs.set_at(i, v)`, `xs = xs.append(v)`)
does **not** get in-place mutable lowering when that wrapper's monomorphization is
**shared** with a non-owned caller — even though the same update written as
index-assign sugar (`xs[i] = v`) does. The result is a large, silent performance
cliff for idiomatic code.

## Why this matters (impact)

The mutable-lowering substrate (MutVec + `*_in_place` hooks) already works. The
gap is purely *reachability of ownership across a shared wrapper*. Measured on the
AWFY `sieve` benchmark (persistent `Vector<Bool>`, `flags = flags.set_at(k, false)`):

| Variant | Time (40 iters) |
|---|---|
| `sieve` as written (`.set_at()` wrapper) | **35.28 ms** |
| `sieve` with the one line changed to `flags[k] = false` sugar | **0.89 ms** |
| `sieve_mut` (manual `@std.buffer`, the workaround ceiling) | 0.92 ms |

So a one-line, still-fully-persistent, no-`Buffer` change closes a **~38×** gap and
reaches the manual-Buffer ceiling. The only thing standing between idiomatic code
and that speed is the wrapper-call boundary.

This is squarely the project north star: *"`Vector.set_at`, index rebinding …
should remain the source-level style users write; the compiler should recover the
mutation-like performance currently demonstrated only by `@std.buffer`"*
([sound-uniqueness/architecture.md](sound-uniqueness/architecture.md)).

**General framing (do not special-case sieve).** `set_at` is just the first
witnessed instance. The real defect is: *any* wrapper of the shape "take a
collection param, update it in place, return it" loses ownership when its
monomorphization is shared between an owned caller and a non-owned caller. The fix
and its tests must target that general shape — `set_at`, `append`, and arbitrary
user wrappers `fn f(xs, …) { xs[i] = …; xs }` — not the sieve line.

## What we know (evidence chain)

All observations via `target/twk ir --census --sites <entry>` and
`TWINKLE_TIMINGS=1 target/twk build …` (the `[time:8g:*]` lines).

1. **The substrate works when the write is visible in the owning function.**
   Rewriting sieve's write to index-assign sugar makes the MutVec region detector
   claim the region (`collect@L… MutVec… scratch yes yes`) and the ownership
   producer emit `reuse(unique)`. Runtime drops to Buffer parity. So neither the
   region detector, the freeze path, nor the in-place hooks are the problem.

2. **The wrapper form loses it only under caller conflict.**
   - In isolation, a function with the exact sieve shape whose `set_at` write is
     the *only* use of that monomorphization **does** fire: `set_at__Bool` shows
     `base=reuse(unique)` → `vector$set_in_place` selected. (The generic
     whole-program summary proves the param owned because every caller is owned.)
   - Add a **second, non-owned** caller of the same monomorphization (base stays
     live) and the single `set_at__Bool` verdict collapses to
     `base=persistent(aliased shell)` → `absent_fallback`. **Even the owned caller
     now loses.** One verdict serves one monomorphization, so the conservative
     (non-owned) caller wins.

3. **This is exactly the AWFY multi-benchmark situation.** `set_at__Bool` is shared
   between:
   - `sieve.tw`: `flags = flags.set_at(k, false)` — owned (old `flags` dies).
   - `queens.tw`: `fr := free_rows.set_at(r, false)` — **non-owned** (`free_rows`
     stays live as the per-row base across the loop and is re-read next iteration).

   Because both share `set_at__Bool`, the generic summary is forced conservative
   and sieve stays persistent → the 38× gap.

4. **Phase 8G variant specialization — which exists precisely to resolve this — is
   producing no variant for the wrapper.** With an owned + non-owned fixture the
   trace shows:

   ```
   [time:8g] table=… variants=… (no variants)
   … clones=0 …
   ```

   `variant_specialize.tw` (8G) is designed to *clone* a callee under an owned
   entry seed and route only owned callers to the clone, leaving non-owned callers
   on the generic persistent body — the exact remedy for a shared-monomorphization
   conflict. It is not firing here: **`compute_variants` publishes no owned variant
   for `set_at`, so 8G has nothing to route.**

## Where the failure localizes

Pipeline of the relevant machinery (all in `boot/compiler/`):

- `summary.tw :: candidate_variants` — scans every function's params; a param is a
  candidate owned variant iff it (a) *threads to an in-place update opportunity*
  (`scan_param_threading().found`) **and** (b) *flows to the return*
  (`ret_aliases_exactly_param || scan.ret_derived`).
- `summary.tw :: run_scc_variants` — seeds the candidate optimistically, runs the
  per-SCC fixpoint, validates (`variant_valid`), and **publishes** survivors into
  the `VariantSummaryTable`. It handles singleton (non-recursive) SCCs, so
  recursion is *not* a structural requirement.
- `codegen/variant_specialize.tw` — `published_callee_set(vt)` gates which callees
  are considered; `collect_groups` matches caller sites whose argument uniqueness
  satisfies a published variant, clones, filters (`updatable_funcs`), and routes.

`set_at(xs, index, value) { xs[index] = value; xs }` *looks* like a textbook
candidate: `xs` threads to an in-place update **and** is returned. Yet no variant
is published. So the break is at **candidate generation or publication for this
wrapper shape**, upstream of the 8G routing that everyone assumed was covering it.

## Theory (ranked hypotheses to confirm)

The leading question: **why does `candidate_variants` / `run_scc_variants` yield no
owned variant for `set_at`?** Ranked:

- **H1 — `scan_param_threading` doesn't flag the update as `found`.** The wrapper's
  body lowers `xs[index] = value` to a `vector$set_unsafe` rebind
  (census: `L11 = update L7`). If `scan_inplace_op` recognizes the mutable
  producer's update forms but not this particular builtin/op shape in the *scan*
  (it is a deliberately separate, ownership-agnostic pre-filter), `found` stays
  false and no candidate is generated. **Most likely.** The mutable-decision
  producer clearly *does* see the site (it prints the verdict), but that is a
  different scan than `scan_param_threading`.

- **H2 — candidate generated but retracted.** `run_scc_variants` seeds the
  optimistic hypothesis, then `variant_valid` retracts it. Would show a candidate
  that fails validation. Distinguish from H1 by instrumenting both counts.

- **H3 — routing/`updatable_funcs` declines a published variant.** If a variant is
  in fact published, `collect_groups` may fail to match the owned caller site
  (argument-uniqueness at the loop-carried call not proven the same way the intra
  -function loop-carried fixpoint proves it), or `updatable_funcs` filters the
  wrapper out. Less likely given the trace already says `(no variants)`, but keep
  as a fallback once H1/H2 are resolved, because fixing publication just moves the
  question here.

- **H4 — determinism / whole-program dilution.** Something about the multi-caller
  whole-program summary suppresses the candidate that appears in isolation. Lowest
  priority; the isolation vs. conflict experiments already point at H1/H2.

## Confirming the theory (first diagnostics)

1. Add a temporary trace (or reuse `TWINKLE_TIMINGS` gating) inside
   `candidate_variants` printing, per function/param: `found`, `ret_derived`,
   `ret_aliases_exactly_param`, and whether a candidate was emitted. Run the owned
   +non-owned fixture. Confirms H1 vs H2 immediately.
2. If a candidate *is* emitted, trace `run_scc_variants` publish/retract
   (`variant_valid`) for that key.
3. If published, enable a `collect_groups` trace and check the owned caller site's
   `call_uniques_sited` argument-uniqueness against the published variant
   (surfaces H3).

## Fix direction (general, not sieve-specific)

Goal: **an owned collection parameter that is updated in place and returned should
publish an owned variant, and owned callers should route to an owned clone while
non-owned callers keep the persistent generic — for arbitrary wrappers, recursive
or not.** Concretely, depending on which hypothesis holds:

- **If H1:** teach `scan_param_threading` / `scan_inplace_op` to recognize the same
  update-site forms the mutable-decision producer already recognizes (the
  `vector$set_unsafe` / index-assign rebind, `Dict.set`, record-field update),
  keeping it a sound *over*-approximation (a hypothesis the fixpoint later proves
  or retracts). Then the singleton-SCC publish path already covers non-recursive
  wrappers.

- **If H3:** align the call-site argument-uniqueness proof used by 8G routing with
  the intra-function loop-carried ownership fixpoint, so a loop-carried owned base
  passed to a wrapper is recognized as owned at the call.

Keep the soundness discipline from `architecture.md`: unknown ⟹ persistent; the
non-owned caller (queens) must keep the persistent generic; determinism of
variant ids / clone names / module order must not depend on hash-map iteration.

## Test plan (must prove the general case)

Per the "general success" requirement, tests target the *shape*, not sieve:

- **Fixture A — shared wrapper, split callers (the core case).** One owned caller
  (`acc = acc.set_at(i, v)` with the base dying) + one non-owned caller (base stays
  live) sharing one monomorphization. Assert: owned caller emits
  `vector$set_in_place` (via an owned clone/route), non-owned caller stays
  persistent, run result unchanged, self-host fixed point holds.
- **Fixture B — user-defined wrapper.** Same shape with a user `fn bump(xs, i) { xs[i] = xs[i] + 1; xs }` rather than the prelude `set_at`, to prove it is not
  hard-coded to prelude names.
- **Fixture C — `append` wrapper.** `acc = acc.append(x)` owned vs non-owned split,
  to cover the second common wrapper family.
- **Fixture D — negative / soundness.** A wrapper whose returned collection is also
  aliased out (escapes) must **not** lower in place for either caller.
- **Regression:** AWFY `sieve` reaches `sieve_mut` parity while `queens` result and
  codegen stay correct; boot-test + `make bundle-cli` self-host fixed point green.

## Non-goals

- `bounce` / `nbody`. Both also use `.set_at()`, but their gap is dominated by
  per-step **record allocation** (`Vector<Ball>` / `Vector<Body>`), not the vector
  write — confirmed: with index-assign sugar the ownership decision fires
  (`would_use true`) yet runtime does not move. Those belong to the deferred
  boxed/record MutVec family (mutvec-checklist Phase 7), not this work.
- New storage representation. This is a *reachability of existing ownership*
  problem; no new mutable representation is required.

## Pointers

- `boot/compiler/summary.tw` — `candidate_variants`, `scan_param_threading`,
  `run_scc_variants`, `compute_variants`.
- `boot/compiler/codegen/variant_specialize.tw` — publish/route/clone (8G).
- `boot/prelude/vector.tw` — `set_at` (`{ xs[index] = value; xs }`), `append`.
- `examples/performance/awfy/twinkle/{sieve,queens,sieve_mut}.tw` — the live
  shared-monomorphization conflict and the perf ceiling.
- Verdict strings & fixtures: `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw`,
  `cfg_ownership_facts_suite.tw`.
