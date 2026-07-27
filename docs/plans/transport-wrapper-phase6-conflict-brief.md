# Decision brief: transport-wrapper role recovery vs. Phase 6 pure-transport semantics

**Audience:** an agent (or engineer) with fresh context who must decide how to resolve
a conflict surfaced while implementing transport-wrapper role recovery. This brief is
self-contained: read it plus the linked files and you have enough to decide.

**Related docs:**
- Design: `docs/plans/transport-wrapper-role-recovery-design.md`
- Implementation plan: `docs/plans/transport-wrapper-role-recovery-plan.md` (Task 3 is the one blocked)
- Root-cause of the original blocker: `docs/plans/sound-uniqueness/analysis/transport-wrapper-blocker.md`

## One-paragraph summary

We are teaching the ownership analysis to classify a parameter that is threaded through
a "transport wrapper" callee and recovered as `Consumed`, so chains like
`build → check → synth` compose owned in-place threading. The implemented fix works for
the target. But it also flips an **existing, deliberate** Phase 6 test that requires a
param threaded through a *non-mutating* wrapper to stay `Borrowed`. The mutating and
non-mutating wrappers are **indistinguishable at the summary level**, so we cannot have
both without deeper work. The classification change is **sound** (arg_unique-gated); the
question is whether to move that Phase 6 precision boundary, do deeper work to preserve
it, or defer transport-wrapper composition.

## What the fix does (already implemented, uncommitted)

Task 3 added `collect_move_recovered_params` to `boot/compiler/ownership.tw` and fed its
result into `cap = .Consumed` in `summarize_seeded`. It records params recovered by a
**licensed move-projection** whose projected shell provenance resolves to exactly one
param — mirroring the real `.ARecordGet` move branch (`projection_move_licensed` +
`pr.shell` + `project_path_prov` → `param_index_of`). A `Consumed`-with-`flows_to_return`
param under `esc=Borrowed` then classifies `Consumed` (non-empty shell in-place path),
which lets its owned variant validate.

**Target that now composes (correct, desired):**
```tw
fn synth(ctx: Ctx, name: String) Out {
  ctx.syms = .set(name, ctx.count)   // MUTATES ctx in place, then wraps it
  Out.{ ctx, ty: ctx.count }         // ret_paths=.f0=from(p0)
}
fn check(ctx, a, b) Ctx { o1 := synth(ctx,a); ctx = o1.ctx; o2 := synth(ctx,b); ctx = o2.ctx; ctx }
```
`check` earns `variant fn check [unique:p0]` with `p0=Consumed`; `build` selects it.

## The conflict

**Failing test:** `boot/tests/suites/cfg_return_paths_suite.tw`, test titled
`"phase6 stage3: owned-entry re-analysis recovers a param-threaded transport"`
(around line 637–668). Fixture `case_w_param_fixture()` (line 214):
```
helper(x) = Wrapper.{ f0: x }          // PURE wrapper — no mutation
caller(ctx) = { out := helper(ctx); g := out.f0; g }
```
Under an owned-entry seed (ctx shell-Unique) it asserts:
```tw
try assert.is_true(is_borrowed(owned, 0))         // <-- FAILS after the fix (now Consumed)
try assert.is_true(owned.params[0].flows_to_return)
```
Test comment: *"Recovered as a Borrowed transport (NOT Consumed -- the param is threaded,
not mutated)."*

**Why the fix flips it:** `synth` (mutates then wraps) and `helper` (pure wrap) produce
**identical** generic summaries — both `p0=Consumed paths{[]}`. Storing a param into a
fresh record is what makes it `Consumed` (last-use move into the wrapper), *not* the
field mutation; and the mutation's field paths are erased once ctx is moved into the
fresh `Out`. So `collect_move_recovered_params` sees the same shape for `check`'s and
`caller`'s projections and marks both `Consumed`. (Verified empirically by the
implementer: both callees summarize `p0=Consumed flows=true`, shell-only in-place paths.)

## Is it sound? Yes.

- `Consumed(p0)` means an owned variant may reuse p0's buffer; it is **selected only when
  `arg_unique[0]` holds at the call site** (p0 Unique + last-use + sole occurrence). The
  read-after-call case (`red_transport_read_after`) fails the gate → p0 published →
  `esc=Retained` → `Published`. That NEG still passes.
- For the pure wrapper, no buffer reuse actually happens, so `Consumed` is a harmless
  over-approximation of "owned/reusable," which is true because the input is unique.
- **This milestone is analysis/render-only** — no codegen dispatch consumes these
  variants yet, so a classification change affects CFG diagnostics, not emitted code today.
- The failing test's comment is a *semantic/precision* statement. Note its neighbor at
  ~line 624 ("param-threaded state is NOT recovered ... Loosening the gate would be
  UNSOUND ... Do not fix this test") **is a different test** (the generic gate-fail case),
  and is *not* the one failing. The failing one is a precision boundary, not a soundness guard.

## The options

### A. Close the precision boundary (accept `Consumed` for pure transport)
Update the failing test to expect `Consumed` and rewrite its comment to record the moved
boundary and the soundness rationale (arg_unique-gated).
- The suite has `is_published` (line 104) and `is_borrowed` (line 111) but **no
  `is_consumed`** helper — add one (mirror `is_borrowed`, matching `.Consumed`) or assert
  `owned.params[0].base_role` is `.Consumed` directly.
- Then finish plan Task 4 (over-fire control) and Task 5 (census + `make stage2` +
  `make test`). **Self-host byte-identical (`make stage2`) and the boot census are the
  soundness validators** — if the broadened `Consumed` classification changed any real
  in-place decision unsoundly, they would catch it.
- Pro: ships transport-wrapper composition now; sound. Con: loses the mutated-vs-threaded
  precision distinction; someone deliberately encoded that distinction, so confirm nothing
  else depends on pure-transport = `Borrowed`.

### B. Distinguish mutation (keep both)
Make `synth`'s summary preserve its field in-place-paths **through** the wrapping (so a
mutating wrapper reports non-shell paths, e.g. `{[],[.f0]}`, while a pure wrapper reports
`{[]}`), then gate `collect_move_recovered_params` on the callee summary having a
**non-shell** in-place path.
- Pro: keeps `check → Consumed` and `helper → Borrowed`; most precise.
- Con: this is the "deep field-backing" the design explicitly scoped **out**. It needs its
  own spike/design: today the field-mutation info is lost when ctx moves into the fresh
  `Out`. Uncertain size; may not be cleanly possible without broader Phase 4/6 changes.

### C. Defer transport-wrapper composition
Revert Task 3's three uncommitted files; keep transport persistent; preserve Phase 6.
- The delegated-consume and mixed shapes already shipped (committed) and are unaffected.
- Pro: zero risk to Phase 6 semantics. Con: transport-wrapper stays a documented ceiling.

## Current repo state (branch `fixpoint-map-inplace`)

Uncommitted (the Task 3 fix, not yet committed because it's blocked):
- `boot/compiler/ownership.tw` — `collect_move_recovered_params` + `move_recovered_has` +
  `cap` feed in `summarize_seeded`.
- `boot/tests/suites/cfg_sound_uniqueness_fixtures_suite.tw` — RED transport lock flipped
  to two composition tests (both pass).
- `boot/tests/fixtures/cfg/sound_uniqueness/transport_wrapper_single.tw` — new single-hop
  positive fixture.

Committed earlier in this effort (do not revisit): Task 1 (candidacy + consumption gate,
`f3ad3d6d`), Task 2 (bounded `function_section` helper, `3dea707e`).

With the uncommitted changes: `check`/`thread` compose and their tests pass; the full boot
suite is `3261 passed, 1 failed` — the single failure is the Phase 6 test above.

## How to verify the core claim yourself (5 min)

The whole decision hinges on "synth (mutating) and helper (pure) are indistinguishable."
Confirm by dumping both generic summaries. A scratch driver (run via `target/twk run
boot/scratch_x.tw`, which compiles current source with the existing binary — delete the
file after) that `summary.compute`s a module containing a mutating wrapper and a pure
wrapper and prints `render_summary` for each will show both as `p0=Consumed paths{[]}`.
The failing test's fixture (`case_w_param_fixture`) is the pure wrapper; the transport
fixtures (`red_transport_wrapper_chain`, `transport_wrapper_single`) are the mutating ones.

## Recommendation (non-binding)

If nothing else in the analysis relies on pure-transport = `Borrowed`, **Option A** is the
pragmatic, sound choice — the classification is arg_unique-gated and the milestone is
render-only, and Task 5's self-host + census are strong soundness validators. Reserve
**Option B** for when the mutated-vs-threaded distinction is actually needed by codegen
dispatch (a later milestone). **Option C** if there is any doubt about moving the Phase 6
boundary right now.
