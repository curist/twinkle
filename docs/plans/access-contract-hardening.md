# Access-contract hardening and cleanup

Status: **In progress** — Findings 1 (arity validation), 2 (proof-cache
soundness), and 6 (`View.sub` clamping) are landed and self-host green;
Findings 3–5 (lowering/monomorphization refactors) are pending.

## Goal

Tighten the implementation that landed with the archived collection-access plan
cluster ([archive/access-contracts.md](archive/access-contracts.md),
[archive/view.md](archive/view.md), [archive/stack.md](archive/stack.md)) so the
new generic access machinery is correct by construction and easier to maintain.

The shipped feature works for the intended `IndexRead<E>` / `IndexWrite<E>` /
`IntoIterator<E>` paths, but the implementation still has a few sharp edges:
contract-bound arity is not validated, parameterized proof caching can record the
wrong fact, generic iteration lowering depends on side-channel maps, and
monomorphization recovers bound-only element types by scanning function bodies.
This plan addresses those issues without changing the public contract model.

## Non-goals

- No new contract syntax.
- No trait/instance search or associated-type machinery.
- No change to the `Self -> E` determined-conformance model.
- No expansion of the `View<C>` public surface beyond documenting or enforcing
  its existing invariants.

## Findings to address

### 1. Validate builtin contract type-argument arity — **DONE**

Landed: `contracts.type_arg_count`, a `ContractArityMismatch` diagnostic wired
through all diag sites, and an arity check in `resolver.tw`'s bound resolution
that drops the malformed bound instead of storing it. Stage0 (Rust) left
untouched — boot self-hosts cleanly and no gate exercises the rejected shapes.

Today the resolver accepts any number of type arguments for any builtin contract.
That lets invalid bounds through with two distinct bad outcomes (confirmed by
repro, not Wasm-validation failures):

- **Missing args on a parameterized contract** surface as a checker-stage
  internal error once the element type is actually used. `fn g<C: IndexRead>(c:
  C) Int { c.at(0) }` fails with `compiler bug: uninstantiated type variable 'C'
  reached unification` — an ICE with a "please report this" link, never reaching
  codegen. (Without the `at` call the body type-checks and emits fine, so the
  failure is latent until `E` is forced.)
- **Extra args on a nullary contract** are silently ignored. `fn f<T: Eq<Int>>(x:
  T) Bool { x == x }` compiles and runs, returning `true`.

Both should be clean resolver diagnostics instead. Examples:

```tw
fn f<T: Eq<Int>>(x: T) Bool { x == x }      // should be rejected (silently accepted today)
fn g<C: IndexRead>(c: C) C { c.at(0) }      // should be rejected (ICEs today)
```

Required arities:

| Contract | Type args |
|----------|-----------|
| `Stringify` | none |
| `Eq` | none |
| `Ord` | none |
| `IndexRead` | `E` |
| `IndexWrite` | `E` |
| `IntoIterator` | `E` |

Implementation sketch:

- Add arity metadata in `boot/compiler/contracts.tw` (for example
  `type_arg_count(contract) Int`).
- In `boot/compiler/resolver.tw`'s type-parameter bound resolution, compare the
  parsed bound args against that metadata.
- Emit a normal resolver diagnostic for wrong arity and avoid storing malformed
  bound refs.
- Keep hover/formatting behavior unchanged for valid bounds.

Diagnostic shape:

- Use a dedicated diagnostic kind rather than a generic resolver error, so CLI,
  LSP, snapshots, and future quick-fixes can present the same message.
- Point the primary span at the malformed contract bound, not the whole type
  parameter list.
- Explain both what the contract expects and how to fix the spelling.
- For parameterized access contracts, name the missing element type in the help
  text (`E`) so users understand why the argument exists.

Suggested messages:

```text
error: contract `Eq` does not take type arguments
 --> example.tw:1:9
  |
1 | fn f<T: Eq<Int>>(x: T) Bool { x == x }
  |         ^^^^^^^ remove `<Int>`
  |
  = help: write `T: Eq`
```

```text
error: contract `IndexRead` needs an element type argument
 --> example.tw:1:13
  |
1 | fn first<C: IndexRead>(c: C) C { c.at(0) }
  |             ^^^^^^^^^ expected one type argument
  |
  = help: write `IndexRead<E>` and declare `E`, for example `fn first<C: IndexRead<E>, E>(c: C) E`
```

```text
error: contract `IndexRead` takes one type argument, but got multiple
 --> example.tw:1:9
  |
1 | fn f<C: IndexRead<Int, String>>(c: C) Int { c.at(0) }
  |         ^^^^^^^^^^^^^^^^^^^^^^ keep only the element type
  |
  = help: write `IndexRead<Int>`
```

For `IndexWrite` and `IntoIterator`, use the same wording pattern as
`IndexRead`, with contract-specific examples:

```text
help: write `IndexWrite<E>` and declare `E`, for example `fn push<C: IndexWrite<E>, E>(c: C, x: E) C`
help: write `IntoIterator<E>` and declare `E`, for example `fn collect<C: IntoIterator<E>, E>(c: C) Vector<E>`
```

Regression coverage:

- Reject extra args on nullary contracts (`Eq<Int>`, `Stringify<Int>`).
- Reject missing args on parameterized contracts (`IndexRead`, `IndexWrite`,
  `IntoIterator`).
- Reject too many args on parameterized contracts (`IndexRead<Int, String>`).
- Keep valid concrete and variable args working (`IndexRead<E>`,
  `IndexRead<Int>`).

### 2. Make parameterized contract proof caching sound — **DONE**

Landed: `prove_contract` now caches parameterized contracts under a per-element
key (`ty::contract::elem`), keeps the bare key for nullary contracts and the
cycle-detection `active` set, and skips the cache entirely (read and write) when
the hinted element is still an unresolved meta. Positive and negative entries use
the same key construction.

`prove_contract` skips *reading* the proof cache when an element hint is present,
because hinted proofs need the side effect of unifying the proof's `Elem` with the
bound's declared element type. However, hinted proofs still write to the same
cache key as unhinted proofs:

```tw
"${ty}::${contract}"
```

That key does not distinguish `IndexRead<Int>` from `IndexRead<String>`. A failed
or successful hinted proof can therefore cache an imprecise fact for later
unhinted checks.

Chosen implementation:

Use parameterized cache keys when the contract arguments are stable, and skip the
cache only for unresolved inference-state-dependent arguments:

```text
if contract has no type args:
  cache by ty + contract
else if all contract args are fully zonked/concrete:
  cache by ty + contract + args
else:
  skip cache
```

Details:

- Nullary contracts keep the existing key shape (`Int::Eq`,
  `String::Stringify`).
- Parameterized contracts include the normalized element argument in the key, for
  example `Seq::IndexRead<Int>` vs `Seq::IndexRead<String>`.
- Do not cache keys containing unresolved metavars. Those are local to the
  current inference session and may solve differently later.
- Apply the same key construction to both positive and negative cache entries.
- Keep the current "skip cache read when a hint must bind" behavior only when the
  hint is not fully resolved. If the hinted argument is concrete after zonking,
  the parameterized key is precise enough to read safely.

Regression coverage:

- A type satisfying `IndexRead<Int>` must not be poisoned by a failed
  `IndexRead<String>` proof.
- Existing successful generic access tests remain unchanged.

### 3. Replace generic-iteration lowering side channels with one explicit record

`for x in c` over generic access contracts currently relies on several maps and
reconstruction steps:

- `for_elem_types` records the element type.
- `method_calls` records `IntoIterator.iter` wrapping for some type-variable
  receivers.
- Lowering infers indexed-contract iteration from `Var(_)` and separate method
  lookup behavior.

This is correct but fragile. The checker already knows the iteration mode; it
should record that decision directly.

Implementation sketch:

Add one lowering-info record keyed by the iterable expression id, for example:

```tw
type IterableLoweringKind = {
  ConcreteIndexed,
  IndexReadContract,
  IntoIteratorContract,
  Iterator,
  Range,
  Dict,
}

type IterableLoweringInfo = .{
  elem_ty: MonoType,
  secondary_ty: MonoType?,
  secondary_allowed: Bool,
  kind: IterableLoweringKind,
}
```

Then:

- Have `iterable_binding_info_of` / `bind_iterable_vars` produce and store this
  record.
- Have `lower_core/iteration.tw` consume the record rather than re-deriving the
  mode from `Var(_)` and `method_calls`.
- Keep concrete `Vector`/`String`/`Dict`/`Range` behavior byte-for-byte equivalent
  where possible.
- Keep `IndexRead` preferred over `IntoIterator` when both are present.
- Build the checker-side decision on the contract-identity helper from Finding 4
  (`lookup_scoped_contract_bound`), not a fresh round of method-name probing.

This must *replace* the existing side channels, not sit alongside them. Remove
`for_elem_types` (the `Dict<Int, MonoType>` threaded through `InferCtx` at
`checker.tw:36/59/5028`) and fold its `elem_ty` into the new record; likewise
drop the `method_calls`-based `IntoIterator.iter` reconstruction once lowering
reads `kind`. The exit criterion "one explicit decision" is only met if no
parallel map survives.

Regression coverage:

- `for x in c` over `C: IndexRead<E>` lowers through indexed `len`/`at` contract
  calls.
- `for x in c` over `C: IntoIterator<E>` lowers through `iter()` and the iterator
  loop.
- `for x, i in c` keeps the existing index behavior for indexed collections and
  iterator-backed collections.

### 4. Prefer contract-specific lookup helpers over method-name probing

Several paths ask whether a type variable has a contract by searching for a
method name such as `"at"` or `"iter"`. That is brittle: future contracts could
reuse method names, and the call sites really care about contract identity.

Implementation sketch:

- Add a helper such as:

```tw
fn lookup_scoped_contract_bound(
  base_ty: MonoType,
  contract: BuiltinContract,
  scope: Vector<ResolvedTypeParam>,
) ScopedContractMatch?
```

- Use it in:
  - `synth_index` (`IndexRead` specifically),
  - iterable analysis (`IndexRead` first, then `IntoIterator`),
  - any contract-backed lowering decisions.
- Keep method lookup by name only where resolving an actual method call.

This is mostly cleanup, but it makes later contracts safer to add.

### 5. Move monomorphization FD recovery away from body scanning

`boot/compiler/monomorphize.tw` currently recovers bound-only type parameters by
walking the callee body and inspecting contract/inherent calls. This handles the
current access-contract cases but couples type substitution to implementation
shape: a function's monomorphic type arguments should be recoverable from its
signature and bounds once receiver types are concrete.

Target direction:

- During `infer_call_subst`, after matching value parameters and return type,
  inspect the callee's type-parameter bounds.
- For a bound like `C: IndexRead<E>`, if `C` is known concrete and `E` is still
  unresolved, resolve the contract method required to determine `E` (for
  `IndexRead`, `at`; for `IntoIterator`, `iter`; for `IndexWrite`, `set_at` or
  `append`) and match the concrete method signature back to the bound arg.
- Keep the existing body-scan recovery temporarily as a fallback, then remove it
  once tests cover the signature/bound path.

Regression coverage:

- Bound-only `E` in return types, local types, and delegated generic calls is
  resolved without depending on a particular contract call in the function body.
- Existing `View<C>` nested-bound cases still monomorphize.
- Recursive generic functions do not trigger unbounded recovery.

### 6. Make `View.sub` total by clamping — **DONE**

Landed: `View.sub` clamps `a` into `[0, count]` and `b` into `[start, count]`
via the new `Int.clamp`, so out-of-range or reversed endpoints collapse to a
valid (possibly empty) window. Added `Int.min` / `Int.max` / `Int.clamp` to the
prelude (`boot/prelude/int.tw`) and documented all four in `docs/API.md`.

`View.sub(v, a, b)` currently adjusts `start` and `count` directly:

```tw
v.start = v.start + a
v.count = b - a
```

That makes invalid windows representable (negative count, out-of-bounds start,
end before start). `View` window operations should prefer total behavior when it
is reasonable, matching `drop_first` / `drop_last` returning an empty view instead
of trapping on empty input.

Chosen behavior: **clamp `sub` to the current view bounds**.

Suggested semantics:

```tw
start := clamp(a, 0, v.count)
end := clamp(b, start, v.count)
v.start = v.start + start
v.count = end - start
v
```

This means:

- `v.sub(-5, 3)` becomes `v.sub(0, 3)`.
- `v.sub(2, 999)` becomes `v.sub(2, v.len())`.
- `v.sub(8, 3)` becomes an empty window at the clamped start.
- Returned views always preserve `0 <= count` and remain within the source
  window.

Tradeoffs:

- **Pros:** preserves `View` invariants, keeps the operation total, avoids delayed
  traps from invalid window records, and fits the slice/window API expectation
  that out-of-range endpoints can collapse to a valid empty or shortened window.
- **Cons:** can hide caller index bugs that a trapping API would expose, and adds
  a small amount of endpoint normalization work per `sub` call.

If strict range validation becomes useful later, add a separate checked helper
such as `try_sub` / `sub_checked`; keep plain `sub` total and ergonomic.

Regression coverage:

- Negative starts clamp to zero.
- Ends beyond the view length clamp to the view length.
- Reversed ranges produce an empty view, not a negative-count view.
- Chained `sub` calls over an already-windowed view stay within the original
  source and materialize the expected elements.

## Suggested implementation order

1. Contract arity validation. This turns the current ICE / silent-accept
   behaviors into clean diagnostics and gives malformed programs early
   feedback.
2. Parameterized proof-cache safety. Small change, low risk.
3. Contract-specific scoped-bound lookup helper. Mechanical cleanup that reduces
   risk before touching iteration.
4. Explicit iterable lowering info. Refactor with behavior-preserving tests.
5. Bound-driven monomorphization FD recovery. Larger semantic cleanup; keep the
   existing body scan as a fallback until confidence is high.
6. Clamp `View.sub` and add invariant/edge-case tests.

## Stage0 (Rust reference) parity

All work here lands in the boot compiler (`boot/`), which is canonical. Findings
1 and 2 change which programs are *accepted* (arity rejection) and which proofs
are *cached*, so the Rust stage0 in `src/` could diverge on validity. Decide per
slice whether stage0 needs the same change:

- If `boot/main.tw` itself never exercises the rejected shapes, stage0 can stay
  as-is and the divergence is benign (boot is the authority).
- If a self-host gate or a shared fixture depends on the behavior, mirror the
  minimal change into stage0.

Default to *not* touching stage0 unless a gate forces it; note the decision in
the slice's commit message so the divergence is intentional and traceable.

## Validation

Run the normal project gates after each behavior-changing slice. For the inner
loop prefer the fast boot gate and targeted Rust filters; reserve the full
`cargo test --release` / `make test` for end-of-slice:

```bash
target/twk run boot/tests/main.tw
cargo test --release
make test
```

For monomorphization changes, also inspect generated WAT for representative
contract calls when debugging:

```bash
target/twk build some/file.tw -o /tmp/debug.wat
```

## Exit criteria

- Malformed contract arities are rejected with a resolver diagnostic during
  resolution, instead of ICE-ing in the checker (missing args) or being silently
  accepted (extra args on nullary contracts).
- Parameterized contract proofs do not cache facts that ignore their element
  argument.
- Generic iteration lowering consumes one explicit checker-produced decision.
- Scoped contract checks are keyed by contract identity, not incidental method
  names.
- Monomorphization can recover `Self -> E` substitutions from bounds/signatures,
  with body scanning removed or kept only as a documented fallback.
- `View.sub` is total: out-of-range endpoints clamp to a valid window, reversed ranges produce empty, and invalid negative-count views are no longer representable through `sub`.
