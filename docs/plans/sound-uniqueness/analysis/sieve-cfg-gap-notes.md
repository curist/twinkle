## sieve vector in-place gap (verified)

The real sieve source lowers to the expected loop-carried `set_at` wrapper call.
Current CFG ownership analysis renders no `verdict -> ...[unique:...]` decision for
it, for two independent reasons:

- Gap B (summary / requirement-flow): `xs[i]=v` inside `set_at` lowers to the
  VECTOR_SET builtin. `collect_field_reqs` routes builtin calls through
  `call_result_fact`, which breaks the flow chain for builtins, so the vector base
  is never dirtied. Result: `set_at`'s p0 stays `Borrowed` with empty
  `in_place_paths`, and `select_variant` (which reads only `in_place_paths`, never
  `ret`) can never key on it. Even a fresh, unique, straight-line vector renders no
  verdict.

- Gap A (forward ownership transfer): `absorb_retained_call_args` unions the stored
  element's provenance into the result's SHELL provenance, so `set_at`'s return is
  `MayAliasParams([0,2])`. The Stage 4a whole-return move only fires for a single
  aliased param, so the call publishes both origins and the loop-carried vector
  degrades to `Shared`.

Escape tracking for the stored reference element is required, but the mechanism
changes across the two fixes:
- After Gap B only: `p2` stays Consumed and stays in `ret=alias(p0,p2)`; callers
  publish it via the multi-param `MayAliasParams` branch.
- After Gap A: `p2` leaves the shell alias set (`ret=alias(p0)`, enabling the
  single-param move) and instead escapes via `p2=Published`; callers publish it via
  the section-1 `Published` branch. This relies on a MANDATORY `publish_atom` on the
  stored operand — a vector element is at `.Elem`, not a ret_path, so without the
  explicit publish `p2` would silently degrade to `Borrowed` (an unsound, unpublished
  escape). The `vector_escape` fixture guards exactly this.

The previously-planned "p0=Consumed p1=Borrowed p2=Borrowed ret=alias(p0)" target was
unsound (it dropped the escape entirely) and is rejected.

## post-fix status (both gaps landed)

After Gap B + Gap A, `set_at__Bool` summarizes exactly the sound target:
`p0=Consumed paths{[]} p1=Borrowed p2=Published ret=alias(p0)`. Self-host reaches a
fixed point (stage3 == stage4), so the whole-value move is not a miscompilation.

Verified behavior by shape:
- Straight-line unique vector `set_at` renders `verdict -> fN[unique:p0]`.
- A unique vector reused across two sequential `set_at`s stays unique (the first
  MOVES, so the second still sees it unique).
- A SINGLE loop carrying the vector converges: the Stage 4a move keeps it Unique on
  the back-edge, so the loop-header join stays Unique and the in-loop `set_at` renders
  the verdict. This holds even with an interleaved read (`if flags[i] { ... }`) in the
  same loop body — a borrow does not degrade the carried vector.

### Gap A publish is gated on `!single_retention` (refinement of the plan)

The plan mandated an UNCONDITIONAL `publish_atom` on the stored operand. That
over-published: it clobbered a clean unique move's ownership and spuriously dropped the
result's `[Elem]` field fact (broke the pre-existing "storing an owned inner into an
all-owned vector keeps [Elem]" fact — a pessimization, more COW). The shipped
`escape_retained_call_args` publishes only when the operand is NOT `single_retention`
(own==Unique && last-use && single-store) — the same predicate the `.Update` field-fact
block reads to keep/drop `[Elem]`. Invariant: the result keeps `[Elem]` iff the operand
was not published. This still publishes every wrapper param (params are not Unique in
the generic pass, nor seeded Unique for a variant unless keyed), so the escape soundness
the plan required is preserved; it only exempts provably-fresh unique moves.

## residual loop-carried merge gap (NESTED loops) — follow-up

The real `examples/performance/awfy/twinkle/sieve.tw` still does NOT render the in-loop
verdict: the carried vector `flags` shows `: Shared` at the outer loop header and the
`if.join`. The cause is isolated to **loop nesting**, not the read:
- single loop + interleaved read of the vector: CONVERGES (verdict renders, stays Unique).
- nested loops (vector carried by an OUTER loop, mutated by an INNER loop, as in sieve):
  does NOT converge. The inner loop header enters `Unknown`, the inner back-edge produces
  `Shared`, and that Shared flows out through the OUTER back-edge, so the outer header
  can never prove Unique — the pessimistic fixpoint (headers start from the join of
  PROCESSED preds only) never bootstraps the nested headers to Unique.

This is a separate loop-merge/optimistic-seeding gap (assume Unique at loop headers,
verify, retract if the back-edge refutes), independent of the summary/move fixes in this
plan. Track as a follow-up. The `sieve_loop_set` fixture pins the single-loop case that
DOES work; a nested-loop fixture and the optimistic-seeding merge are the next plan.
