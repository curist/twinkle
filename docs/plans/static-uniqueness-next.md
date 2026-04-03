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

## Proposed Rollout

### Phase A: Semantics Metadata

Add explicit result-freshness metadata to optimizer semantics, separating:

- reuses base when unique
- returns fresh result when shared

Do this first so the rest of the pass stops relying on implicit assumptions.

### Phase B: Fresh-After-COW Tracking

Teach the forward walk to mark a result unique when:

- the op guarantees a fresh result in the shared case, and
- the result has not itself escaped/been tainted yet

This is the highest-value precision improvement with the smallest structural
change.

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

- aliased base, then `set`, then further linear updates on the fresh result
- both branches produce a unique updated value; update continues after the join
- negative branch case where one path captures/stores the value, suppressing the
  rewrite after merge
- helper-function wrappers around `append`, `set`, and record update
- negative helper case where the callee stores the argument in an aggregate or
  captures it in a closure

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
