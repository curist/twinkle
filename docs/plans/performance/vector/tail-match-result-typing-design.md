# Tail-match result typing for typed vectors

**Status:** design (approved 2026-07-07, review folded in), not yet implemented
**Branch:** `typed-vector-crossfn-abi`
**Depends on:** the landed `Expected{vt, mono}` emit-coercion foundation
(`expected-vt-coercion-design.md`, commits 7b91f250..c51ce496)

## Goal

Let a dataframe-style accessor carry a physical `PVecI64` (raw i64 leaves) through
its return, so callers of it are typed and downstream reads use `get_i64`:

```tw
pub fn as_ints(c: Column) Vector<Int> {
  case c.data {
    .IntCol(v) => v,
    _ => error("column is not Int"),
  }
}
```

Concretely: make `return_is_typed(as_ints)` fire so the function gets a `PVecI64`
physical return ABI, `keys := as_ints(col)` is typed, and the `order_by` comparator
reading `keys` uses `get_i64` — dropping the dataframe `order_by` sort (~1343ms).

**Scope: tail-position match/if results only.** The match/if whose result is the
function's returned value. This is exactly the accessor shape (`as_ints`,
`as_floats`, `as_strs`, ...). General non-tail match-result typing (result stored
to a field, passed onward, or bound mid-function) is explicitly deferred — see
*Deferred*.

## Why the naive "just reuse `.Return => false`" is not enough

Ground-truth investigation (2026-07-07):

- `as_ints` does **not** lower to a tail-match. `lower_anf.tw:553-568` unconditionally
  materializes every `.Match` into a result slot + `Atom(ALocal tmp)`, even in tail
  position. The WAT has a real result slot `p1`: the `IntCol` arm does
  `p2 = struct.get; p1 = p2; br`, and the tail is `local.get p1`.
- Writing the arm as `.IntCol(v) => return v` **does** make the arm `return`
  directly, but the function *still* returns boxed `$rt_types__PVec` — typed routing
  did not fire. `return_atom_slots` (typed_param_abi.tw:306) collects two poison
  atoms besides `v`:
  1. the **diverging `error` arm's** value slot `t` (an `error()` call result — not a
     typed vector), and
  2. the **dead trailing `Atom(ALocal r)`** (unreachable once every arm returns/traps).
- `expr_always_diverges` (anf_analysis.tw:508) does **not** flag `error()` as
  diverging: in the IR an `error` call is a normal value-producing op; its trap is a
  runtime effect, not a structural terminal. Divergence-of-`error` is a *semantic*
  fact (it is a noreturn builtin), not a structural one.

So typing the accessor requires excluding both poison atoms from the return-atom set.
The store-based arm (`.IntCol(v) => v`) additionally makes `v` escape via the store
into `r` (`v_group_escapes` treats a bare arm `Atom(v)` as an escape).

## Design

Three cooperating pieces, all in the boot backend (boot-only; no stage0 parity
needed — semantics-preserving optimization).

### 1. `BuiltinRegistry.is_noreturn` — noreturn as builtin metadata

The one required semantic input: "`error`/`trap` never return." This is **not** a
RouteIds workaround. The fact already exists upstream (the checker's
`call_diverges` / `expr_diverges`, checker.tw:5070) and its absence from the IR is a
latent imprecision affecting `expr_always_diverges` and any divergence consumer.

Its natural home is builtin metadata: the backend already threads
`builtins: BuiltinRegistry` through every relevant function. Expose
`is_noreturn(reg, func_id) Bool`, keyed off the builtin (canonical `trap`, surfaced
as `error`). Implementation detail (field on `BuiltinEntry` vs. canonical-name check)
is left to the plan; either way it is a single source of truth in `builtins.tw`, not
scattered fids.

### 2. Tailify canonicalization (pre-route `PreparedExpr → PreparedExpr`)

For a function tail of shape `Let(r, AMatch|AIf, Atom(ALocal r))` where `r`'s only
use is that trailing atom, rewrite each *value-producing* arm's tail
`Atom(a) → Return(Some(a))` (recursing through nested if/match/let-spines to reach
each arm's tail). Diverging arms are left untouched. An arm already ending in
`Return(Some(a))` is left as-is.

This is semantics-preserving (returning the match result per-arm ≡ storing-then-
returning it) whenever the precondition holds — `r` used only as the immediate
trailing `Atom(ALocal r)`.

**Gate: apply tailify only when the function's `return_mono` is `Vector<Int>`.**
Although the rewrite is a semantics-preserving canonicalization in general, running
it unconditionally rewrites the IR shape of *every* tail `Let(r, AMatch|AIf, Atom r)`
in the whole compiler and perturbs the `try_emit_tail_op` pattern (the trailing
becomes `Atom(r)`, not `Return(Some(r))`) — a broad blast radius for a tiny general
win. Gating to `Vector<Int>` returns confines the change to exactly the accessor
shape this feature targets. Generalize later only if a measured reason appears.

Its payoff: the live arm's `v` now flows into a **`Return`**, so the existing
`v_group_escapes` rule `.Return(.Some(_)) => false` (route_typed_vec.tw:872) already
makes `v` consumed-typed-only. **No store-escape relaxation is introduced.** The
now-dead trailing `Atom(ALocal r)` is left in place — exactly the shape the compiler
already produces for source-level `return`-in-arm matches (see the Wasm-validation
assumption below, which R1 in the plan promotes to a step-0 spike).

Nested-spine recursion (reaching each arm's true tail through let-spines / nested
if/match, skipping diverging sub-arms, leaving already-`Return` tails alone) is a
correctness surface that needs its own unit coverage.

### 3. Divergence-aware `return_atom_slots` (typed_param_abi.tw)

When a `Let`'s op is a match/if that **all-diverges** under a noreturn-aware
divergence predicate (so the `error` arm counts via `is_noreturn`), skip the dead
body (excludes `r`) and skip diverging arms' atoms (excludes `t`). Net collected
return-atoms = the live `Return` atoms (`v`) only.

This change is not `as_ints`-specific: it alters `return_is_typed` for *any*
`Vector<Int>`-returning function with diverging tail arms. It stays sound because
every collected slot must still pass `slot_typed_after_route` and the return type
must be `Vector<Int>` — but it needs regression coverage (a multi-arm typed
accessor; one with a genuinely non-typed arm that must stay untyped; `error` in a
non-tail position).

Then the *existing* `slot_typed_after_route` payload-read case types `v`,
`return_is_typed(as_ints)` fires, `typeable_return` gives the function a `PVecI64`
phys return ABI, and the already-landed function-return emit gate coerces the arm
returns. `route_func`'s `eligible_v` types `v` via its existing
`collect_typed_payload_reads` path — the `eligible_v` ↔ `slot_typed_after_route`
agreement is **not** disturbed.

The divergence predicate is a `PreparedExpr`/`PreparedOp` mirror of
`expr_always_diverges`. **Note:** the existing `PreparedExpr` version
(`emit/helpers.tw:41`) does *not* inspect the op — its `Let` case is
`.Let(_, _, body) => diverges(body)`. The mirror must add **both** (a) the
`AIf`/`AMatch` op recursion (a match/if all of whose arms diverge is diverging, as
in the `AnfExpr` version `anf_analysis.tw:508`) **and** (b) the `is_noreturn`-call
case. Adding only the `error`-call case is insufficient.

### Why the payload is typed (producer-driven — no circular dependency)

`analyze_typed_payloads` (route_typed_vec.tw:1456) marks a variant payload field
typed iff `has_ok_producer and !bad_producer and !bad_consumer`, and its only scan —
`scan_func_payload_producers` — sets solely `has_ok_producer`/`bad_producer` from the
**construction** sites (`.IntCol(xs)` where `xs` is a clean typed source). It never
sets `bad_consumer` for payloads (that flag is the *record-field* path,
`scan_consumers` on `.ARecordGet`). So the `IntCol` field's typedness is decided
entirely by where `IntCol` is *built*, independent of `as_ints`. There is **no**
circular payload↔return dependency; `as_ints` only needs tailify so its *read* of
`v` is consumed-typed-only (otherwise the escaping payload binding is coerced back to
boxed `PVec` at extraction).

**Contingency this exposes:** the perf win requires **every** `IntCol` producer
program-wide to be clean. If any construction site feeds a boxed/messy source,
`bad_producer` fires → the field is boxed → `as_ints` still compiles *correctly*
(the return coerces boxed `PVec` → `PVecI64` via `unbox`, O(n)) but wins nothing. A
test should assert the dataframe's column producers are clean.

## Why this is sound (and beats approach 1)

Every step only *excludes provably-dead/diverging code* from the return-atom set, or
converts a store into an equivalent `Return`. We never newly type a **live** boxed
result slot, and we never relax the escape check on a **reachable** store — the two
things that could produce a mismatched physical type at an un-gated store site
(loop rebind, record-field store, dict/vector element store, closure capture, Cell
store), which would be invalid Wasm / a trap. The landed emit gates cover only four
sites, so soundness rests entirely on this analysis being conservatively correct;
this design is conservative by construction.

## Wasm-validation assumption — the make-or-break risk (plan step 0)

After retyping, emit still emits the dead trailing `Atom(ALocal r)` as
`local.get $r; <unbox to PVecI64>; return` on a never-assigned boxed `r`, relying on
that code being unreachable (it sits after a match whose every arm returns/traps) so
Wasm's polymorphic-stack rule accepts the operand-type mismatch. Confirmed
structurally in the explicit-`return` WAT experiment on the *boxed* function — but
that is not the same as the *retyped* function.

**This is the make-or-break assumption, so the plan verifies it first (a step-0
spike): hand-construct / force the retyped `as_ints`, then confirm the module
validates and runs before building anything else.** If dead-code validation proves
fragile, the fallback is to have tailify (or emit) replace the dead trailing
`Atom(ALocal r)` with an explicit `unreachable`/trap terminal when the preceding
op all-diverges, so validity never depends on the polymorphic-stack rule.

## Deferred (documented, not built)

- **General non-tail match-result typing** — result stored to a field / passed
  onward / bound mid-function. This is the approach-1 path: a Match/If case in
  `slot_typed_after_route` with a soundness-critical escape relaxation (arm→result
  flow as intra-group, only in tail/result positions) plus reconciling `route_func`'s
  `eligible_v` with `slot_typed_after_route` (two parallel "is slot typed"
  implementations that must agree). Pursue only if a real non-tail workload demands
  it.
- **Root-cause structural fix** — lower `error`/`trap` to a real IR terminal
  (`unreachable`/trap node) instead of a value-producing call, so divergence is
  structural and no metadata lookup is needed anywhere (`expr_always_diverges` just
  works). Correct long-term, but touches shared frontend lowering, exhaustiveness,
  DCE, and emit (stage0 parity) — a much larger blast radius than this feature
  warrants. `is_noreturn` metadata is the pragmatic interim.

## Verify loop

```
make bundle-cli          # self-host fixed point
make boot-test           # boot test suite (currently ~2973)
target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw
                         # "sort idx by amount" ~1343ms must drop, must not trap
```

Minimal repro: `/tmp/asints.tw` (`Col` enum with `IntCol(Vector<Int>)` + `as_ints` +
a sum-over-keys caller). WAT: `target/twk build <file> -o /tmp/x.wat`.
`as_ints` lives at `examples/performance/dataframe/frame/column.tw:74`.
