# Static Uniqueness Precision: Next Steps

## Goal

Extend Twinkle's current uniqueness optimizer to catch more linear-update cases
without introducing runtime refcounts, runtime uniqueness flags, or user-visible
ownership annotations.

This is a follow-up to
[`deferred-persistence.md`](./deferred-persistence.md), which defines the
current semantic contract and documents the implementation that already exists.

## Why This Exists

The current pass is intentionally conservative. That keeps it easy to reason
about, but it leaves performance on the table in cases that are still tractable
with ordinary static analysis.

The main missed opportunities today are:

- values that become fresh again after a guaranteed copy-on-write update
- values that remain unique on all incoming control-flow paths after a merge
- helper functions that consume and return an updated value linearly, but whose
  parameters are conservatively tainted at function entry

These are static-analysis precision issues, not evidence that Twinkle needs
runtime tracking.

## Non-Goals

- No runtime reference counting or runtime uniqueness checks
- No user-visible uniqueness types, borrow syntax, or `@noescape` annotations
- No whole-program theorem-proving or exponential path enumeration
- No requirement to catch every dynamic case that a runtime-tracked language
  such as Roc may optimize

The goal is to recover the high-value, still-predictable cases while keeping the
optimizer local, understandable, and cheap.

## Current Conservative Limits

The current pass in [`src/opt/uniqueness.rs`](../../src/opt/uniqueness.rs):

- taints all function parameters at entry
- treats unknown calls as retaining unless listed as known COW or known
  read-only operations
- does not propagate uniqueness out of `if`/`match`/`loop` regions
- only lets a COW call result inherit uniqueness when the base was already
  unique and the update consumed it

Those rules are safe, but they miss cases where the result of a shared update is
now fresh, or where all branches preserve uniqueness independently.

## Proposed Extensions

### 1. Fresh-After-COW Results

When an update operation is known to allocate a fresh result if the base is not
uniquely reusable, the result should be treated as a new freshness boundary.

Example:

```tw
y := xs
xs2 := Dict.set(xs, k, v)
```

Even though `xs` is aliased, `xs2` should be trackable as a fresh value if
`Dict.set` is summarized as:

- consumes the logical value of `xs`
- returns a result not aliased with prior mutable state
- safe for subsequent uniqueness tracking

This lets the optimizer resume normal uniqueness reasoning after the forced
copy/path-copy step.

#### Required metadata

Extend optimizer semantics for update builtins with a stronger result-freshness
classification:

- **ReuseIfUnique:** current behavior; may reuse base if provably unique
- **FreshIfShared:** if uniqueness proof fails, result is still a new logical
  value suitable for further tracking

This is especially relevant for:

- `VECTOR_SET_UNSAFE`
- `DICT_SET`
- `DICT_REMOVE`
- future persistent operations such as `VECTOR_CONCAT`

### 2. Path-Sensitive Merge Rule

The current pass drops uniqueness at control-flow joins. Instead, use a normal
forward dataflow meet.

Target rule:

- a local is unique after a merge iff it is unique on every incoming path and
  no path introduces an escaping alias

This remains polynomial. It does not require enumerating path combinations
beyond the standard branch dataflow merge.

#### Initial scope

Start with `if` and `match` only. Keep loops conservative until the merge rules
are well-tested.

#### Important distinction

We do not need to prove that two different locals from different branches are
"the same object." We only need a safe merge rule for locals that are already
part of ANF control-flow state and survive the join.

### 3. Function-Boundary Precision

The biggest risk in staying purely intraprocedural is accidental quadratic
behavior across small helper functions.

Example:

```tw
step(xs, v) = xs.append(v)

build(items) =
  xs := []
  for item in items {
    xs = step(xs, item)
  }
  xs
```

If `step`'s parameter is always treated as tainted, Twinkle may miss the linear
update pattern entirely.

#### Proposed static remedies

- **Call summaries:** infer whether a function argument is retained, captured,
  stored, or only consumed into a returned update result
- **Selective inlining:** inline tiny wrappers around known update ops
- **Specialization:** clone a function for "consuming unique arg" call sites

#### Summary shape

Keep the first version simple. For each parameter, summarize:

- retained/captured: yes or no
- may flow into aggregate storage: yes or no
- may flow to unknown call: yes or no
- consumed into returned update result: yes or no

This is enough to unblock many wrappers around `set`, `remove`, `append`, and
record update helpers.

### 4. `VECTOR_CONCAT` and Similar Multi-Base Ops

Some updates involve more than one collection input. These need stronger alias
reasoning than today's single-base rules.

Example:

```tw
zs := xs.concat(ys)
```

Potential staged rule:

- if `xs` is unique
- and `ys` is proven not to alias `xs` or a view into `xs`
- then allow a destructive fast path on `xs`

This should stay behind a dedicated plan gate because the alias proof is more
subtle than the current one-base consume-reassign cases.

## Measurement Baseline (2026-04-03)

Before describing the rollout, here is the measured state of the boot compiler
(`boot/tests/main.tw`, 2738 functions). Reproduce with:

```bash
cargo test --release --test cow_analysis -- --nocapture
```

| Operation | Pre-opt | Post-opt | Rewritten |
|---|---|---|---|
| VECTOR_APPEND | 427 | 391 | 36 → builder |
| DICT_SET | 227 | 170 | 57 → DICT_SET_IN_PLACE |
| VECTOR_CONCAT | 106 | 106 | (no rewrite exists) |
| REC_UPDATE | 98 | 98 COW | (none rewritten) |
| VECTOR_SET_UNSAFE | — | 0 | 14 → VECTOR_SET_IN_PLACE |
| DICT_REMOVE | 6 | 6 | (none rewritten) |

**Total COW remaining: 777.** Optimization rate ≈ 14%.

### Why the biggest buckets remain un-optimized

**Pattern analysis of the top remaining DICT_SET functions:**

| Function | COW ops | Pattern |
|---|---|---|
| `make_prelude_optimizer_semantics` | 11 DICT_SET | sequential `d[k] = v` on local `Dict.new()` |
| `mock_semantics` | 10 DICT_SET | sequential `d[k] = v` on local `Dict.new()` |
| `scan_tainted_op` | 8 DICT_SET | sequential `d[k] = v` on local `Dict.new()` |
| `link` | 12 DICT_SET | mixed sequential + loop |

All three top functions share the same shape: a local dict is created via
`Dict.new()` (a fresh producer), updated sequentially with `d[k] = v` (which
lowers to `DICT_SET(d, k, v)` + `assign(d = result)`), and then stored into a
record field or returned at the end of the function.

The current optimizer **should** handle this — `Dict.new()` is a fresh producer,
and each `DICT_SET` + reassign is the consume-reassign pattern. But it doesn't,
because the pre-scan `collect_tainted` sees the final record construction
`MyRecord.{ field: d, ... }` and taints `d` **globally** for the entire
function. Every intermediate version of `d` (which is dead before the escape
point) is poisoned by the final use.

This is the single highest-impact precision gap in the current optimizer.

## Proposed Rollout

### Phase A: Reassign-Aware Taint (pre-scan refinement)

**Problem.** The pre-scan (`collect_tainted`) is flow-insensitive: if a local
escapes anywhere in the function, it is tainted everywhere. For reassigned
locals (`d = Dict.new(); d[k1] = v1; d[k2] = v2; return .{ f: d }`), this is
too conservative. Each `assign(d = result)` kills the previous version of `d`.
Only the *final* version — the one live at the escape point — is truly aliased.

**Observation.** In ANF, `d[k] = v` lowers to:

```
let r = DICT_SET(d, k, v)   // COW: allocates fresh if d is shared
let _ = assign(d = r)        // kill old d, d now points to r
```

The `assign(d = r)` means the old value of `d` is dead — no reference to it
survives. If `d` is later stored into a record, only the final value of `d`
(after all reassignments) escapes. Prior values were consumed by the
DICT_SET→assign chain.

**Fix.** Refine `collect_tainted` to track which locals are reassigned
(have `AAssign` targeting them). For a reassigned local, an escape point
(stored in record/array/variant, passed to non-COW call, captured by closure)
only taints the local if the escape is reachable without an intervening
reassignment that kills the current version.

Concretely, for straight-line code this means: walk the ANF in order. When we
see `assign(d = ...)`, reset `d`'s "escaped" status. When we see `d` used in
an escaping position, mark it escaped. At the end, only locals that are escaped
*without* a subsequent reassign-then-COW chain are tainted.

**Scope.** Start with straight-line reassign chains only. If `d` is reassigned
inside a branch or loop, keep the current conservative behavior (taint it).

**Why this is safe.** The key invariant: `assign(d = r)` makes `d` point to `r`.
The old value of `d` is unreachable through `d` after the assignment. If `r`
came from `DICT_SET(d, k, v)` where `d` was the sole reference, the old backing
storage was either reused (in-place) or copied (COW) — either way, `r` is the
only live reference to the result. Subsequent operations on `d` (now pointing to
`r`) are safe to treat as unique until the next escape.

**Expected impact.** This directly unblocks the sequential-dict-build pattern
that accounts for ~40+ DICT_SET operations across the top functions. It also
unblocks the equivalent VECTOR_SET_UNSAFE chains and REC_UPDATE chains where
the base is fresh but escapes at function end.

**What this does NOT help.** Cases where the base truly is a function parameter
(tainted at entry, never reassigned from a fresh source). Those need Phase B.

#### Implementation sketch

In `collect_tainted` (`src/opt/uniqueness.rs`), use a two-pass approach:

**Pass 1 (existing):** Run the current `collect_tainted` as-is to produce the
baseline `tainted` set. Also collect the set of locals that are targets of
`AAssign` (reassigned locals).

**Pass 2 (new, straight-line only):** For each reassigned local `d` that is in
`tainted`, walk the top-level `Let` chain (the straight-line spine of the
function body, NOT recursing into `AIf`/`AMatch`/`ALoop` sub-expressions).
Track a boolean `escaped_since_last_reassign` per candidate local:

- When we see `AAssign { local: d, ... }`: reset `escaped_since_last_reassign`
  to `false` for `d`.
- When we see `d` used in an escaping position (stored in record/array/variant,
  passed to non-COW call, captured by closure): set `escaped_since_last_reassign`
  to `true` for `d`.
- When we see `d` used in any position inside a branch or loop sub-expression
  (i.e., `d` appears in an `AIf`/`AMatch`/`ALoop` body): conservatively set
  `escaped_since_last_reassign` to `true` for `d` (bail out for this local).

At the end: if `escaped_since_last_reassign` is `false` for `d`, remove `d`
from `tainted`. This means every escape of `d` was followed by a reassignment
that killed the escaped version before any further use.

**Why the two-pass approach.** The existing `scan_tainted_expr` recurses into
branches via `scan_tainted_op`. The second pass deliberately does NOT recurse —
it only walks the top-level `Let` chain. This avoids the ambiguity of
cancelling branch-scoped escapes with outer-level reassigns. If `d` appears
anywhere inside a nested branch/loop, we bail out and keep the conservative
taint from pass 1.

This is a localized change to the pre-scan. The forward walk and rewrite logic
remain unchanged.

#### Test fixtures

Positive (rewrite should happen):
- `dict_set_chain_escape_at_end` — `d = Dict.new(); d[k1]=v1; d[k2]=v2;`
  `return .{f: d}` → both sets rewritten to in-place
- `vector_set_chain_escape_at_end` — same pattern with vector
- `record_update_chain_escape_at_end` — `r = MyRec.{...}; r.f1 = v1;`
  `r.f2 = v2; return .{nested: r}` → both updates in-place

Negative (rewrite must NOT happen):
- `dict_set_chain_alias_mid` — `d = Dict.new(); y = d; d[k]=v; return .{f:d}`
  → `d` aliased to `y` before the set, NOT safe
- `dict_set_chain_escape_in_branch` — escape inside a branch, reassign only
  on one path → conservative, no rewrite

### Phase B: Fresh-After-COW Results

**Problem.** When a COW operation runs on a tainted base (e.g., a function
parameter), the current pass does not rewrite the call AND does not mark the
result as unique. But COW operations guarantee a fresh allocation when the base
is shared — the result is a new object with no prior observers.

**Example:**

```tw
fn update_twice(d: Dict<String, Int>, k1: String, v1: Int, k2: String, v2: Int) Dict<String, Int> {
  d1 := Dict.set(d, k1, v1)   // d is param (tainted), COW copies → d1 is fresh
  d2 := Dict.set(d1, k2, v2)  // d1 is fresh+unique → could be in-place
  d2
}
```

Today: neither set is rewritten. `d` is tainted (parameter), so the first set
stays COW. `d1` is not marked unique because the current code only propagates
uniqueness when the base was already unique. The second set also stays COW.

With Phase B: the first set still COWs (correct — `d` might be aliased by the
caller). But `d1` is recognized as fresh (COW allocated a new dict). The second
set sees `d1` as unique and rewrites to `DICT_SET_IN_PLACE`.

**Fix.** In the forward walk (`rewrite_let`), after the existing COW rewrite
block (lines 887–911 of `uniqueness.rs`): when a COW op's base is tainted but
the operation is a known COW with a fresh-if-shared guarantee, mark the result
local as unique even though no in-place rewrite happened on this call.

**Scope.** Only COW operations that have an `in_place_rewrite` (i.e.,
`DICT_SET`, `DICT_REMOVE`, `VECTOR_SET_UNSAFE`) participate in fresh-after-COW
marking. `VECTOR_APPEND` is in `cow_op_info` but has `in_place_rewrite: None`
— its result freshness depends on the underlying array implementation and
future persistent vector changes, so it is excluded from Phase B. (The loop
builder rewrite already handles `VECTOR_APPEND` via a separate mechanism.)

The key addition to the forward walk:

```
if let Some(info) = cow_op_info(*func_id) {
    if let Some(Atom::ALocal(base)) = args.get(info.base_arg) {
        let base = *base;
        if unique.contains(&base) && !tainted.contains(&base) {
            // ... existing rewrite logic (unchanged) ...
        } else if info.in_place_rewrite.is_some() {
            // NEW: base is tainted or not unique, but this COW op
            // guarantees a fresh result. Mark the result as unique
            // for downstream ops — but only if the result itself
            // is not already tainted (escapes later in the function).
            let is_consumed = is_consume_reassign(body, base, bind_local)
                || !live_after(body).contains(&base);
            if is_consumed && !tainted.contains(&bind_local) {
                unique.insert(bind_local);
            }
        }
    }
}
```

**Why this is safe.** Three guards protect correctness:

1. `info.in_place_rewrite.is_some()` — restricts to ops with well-defined
   full-copy COW semantics (`DICT_SET`, `DICT_REMOVE`, `VECTOR_SET_UNSAFE`).
   These always allocate a new backing structure when the base is shared.

2. `is_consumed` — ensures the old base doesn't survive as a potential alias
   to the result's internal state. Without consumption, the old base might
   share structure with the result (in a future persistent-data-structure
   world), so we conservatively require the base to die.

3. `!tainted.contains(&bind_local)` — ensures the result itself is not already
   known to escape (stored in a record, passed to a non-COW call, captured by
   a closure) later in the function. This is the same guard used by the
   existing `AInit` transfer path and fresh-producer detection. Without it, a
   result that escapes downstream could be incorrectly treated as unique,
   enabling an unsafe in-place rewrite on a subsequent operation.

**Interaction with Phase A.** These two phases are independent and compose:
- Phase A fixes the pre-scan so fresh locals aren't over-tainted by late escapes
- Phase B fixes the forward walk so tainted-base COW results gain uniqueness

A function like `make_prelude_optimizer_semantics` benefits from Phase A (the
base is `Dict.new()`, just over-tainted). A function like `update_twice` above
benefits from Phase B (the base is a genuine parameter). Both phases together
cover both patterns.

**Expected impact.** Moderate — primarily helps functions that receive a
collection parameter and do multiple sequential updates before returning.
Less common than the Phase A pattern in the current boot compiler, but
important for user-written code.

#### Implementation sketch

In `rewrite_let` (`src/opt/uniqueness.rs`), extend the COW check block with an
`else if` branch as shown above, gated on `in_place_rewrite.is_some()` and
`!tainted.contains(&bind_local)`. No changes to `collect_tainted` or
`CowOpInfo`.

#### Test fixtures

Positive (rewrite should happen):
- `dict_set_param_then_set` — `fn f(d) { d1 = Dict.set(d,k,v); d2 =`
  `Dict.set(d1,k2,v2); d2 }` → second set rewritten to in-place
- `vector_set_param_chain` — same pattern with VECTOR_SET_UNSAFE

Negative (rewrite must NOT happen):
- `dict_set_param_result_aliased` — `d1 = Dict.set(d,k,v); y = d1;`
  `d2 = Dict.set(d1,k2,v2)` → `d1` aliased to `y`, second set stays COW
- `dict_set_param_result_captured` — `d1 = Dict.set(d,k,v);`
  `f := fn() { d1 }; d2 = Dict.set(d1,k2,v2)` → `d1` captured, stays COW

### Phase C: Branch/Merge Dataflow

Replace the "do not propagate out of branches" rule with intersection-style
merge of uniqueness facts for `if` and `match`.

Keep loops conservative for the first iteration.

### Phase D: Function Summaries

Infer and cache simple no-retain/consume summaries for local functions and use
them at call sites. Apply only to direct known callees first.

### Phase E: Selective Specialization or Inlining

Only if needed after measurement. This is likely the most invasive step and
should be justified by benchmark wins rather than aesthetic completeness.

## Testing Strategy

Add focused fixtures for each new precision class:

- **Phase A:** sequential update chains on fresh locals that escape at function
  end; negative cases with mid-chain aliasing or branch-scoped escapes
- **Phase B:** sequential updates on parameter-sourced COW results; negative
  cases with aliasing or capture of intermediate results
- **Phase C:** both branches produce a unique updated value; update continues
  after the join; negative case where one path captures/stores the value
- **Phase D/E:** helper-function wrappers around `append`, `set`, and record
  update; negative case where callee stores argument in aggregate or closure

Testing should remain at the same four levels as the current plan:

- structural ANF checks
- interpreter correctness
- Wasm correctness
- differential opt vs no-opt

## Stopping Rule

Twinkle does not need to catch every case that a runtime-tracked implementation
could optimize.

This plan is successful if it:

- preserves the simple static-only runtime model
- removes the most important accidental `O(N^2)` cases across helpers
- regains uniqueness after forced COW boundaries
- improves branch precision without making the optimizer opaque

If later cases require significantly more complex alias analysis for marginal
wins, it is acceptable to stop here.
