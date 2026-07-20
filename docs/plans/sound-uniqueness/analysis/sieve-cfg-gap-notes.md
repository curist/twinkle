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

## nested loop-carried merge gap (closed)

The real `examples/performance/awfy/twinkle/sieve.tw` now renders the in-loop
`verdict -> ...[unique:p0]` for the inner `set_at` call. The fix is conservative
provisional loop-header seeding: live loop-header locals may start as `Unique`
during iteration, but an assumption is kept only when every entry and backedge
predecessor contribution validates as both `Unique` and binding-valid after convergence.
Function-parameter and fresh-alias nested-loop fixtures stay conservative.

## graph_scc.visit classification (analysis-resolved)

`graph_scc.visit` now keeps its **generic** summary/body conservative while rendering a
separate owned diagnostic body for the reachable recursive variant. The generic section
still reports `p0=Published p1=Published p2=Published ret=alias(p0)` and its record
updates may still say `shell=persistent(aliased shell)`: that is the correct generic
body because `visit` can be called without an owned precondition.

The owned precondition is rendered separately as `variant fn visit [unique:p0]`, with
`variant: p0=Consumed paths{[]} ... ret=alias(p0)`. In that variant-qualified body,
the threaded state record's shell updates render `shell=reuse(unique)`, while dict/vector
field backings remain conservative with `field=persistent(insufficient deep ownership)`.
This separation is intentional: field-value publication must not publish the enclosing
record shell, but deep field mutation still needs its own proof.

This is still analysis/debug output only. Real in-place codegen remains the Stage 6
handoff in the codegen track: codegen must clone/specialize by `VariantId` before it can
consume these variant-qualified verdicts as mutation decisions.
