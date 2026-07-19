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
