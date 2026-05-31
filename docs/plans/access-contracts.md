# Access contracts — `IndexRead` / `IntoIterator` / `IndexWrite`

Status: **decisions locked, ready to implement** (2026-05-29). The general-access
foundation under [view.md](view.md) (the zero-copy window), [stack.md](stack.md)
(LIFO), and [slice-performance.md](slice-performance.md) (the audit). Needs one
contract-model extension: **parameterized contracts with a functional
dependency**. Locked design choices are recorded inline and summarized in
[Decisions](#decisions-locked-2026-05-29). Tracked under
[collections-access.md](collections-access.md).

## Why

We want one **general access pattern** — write `find` / `position` / `fold` /
`region_eq` / `starts_with` once and have it work over `Vector`, `String`,
`View`, `Stack`, and anything else indexable. Today's contracts can't express
that: `Stringify` / `Eq` / `Ord` are all **Self-only** (`eq(self, other: Self)`),
so there's no way to name "the element type you get when you index into Self."

Two mechanisms could give general access (see [slice-performance.md](slice-performance.md)):

- a **capability record** — a closure `at: fn(Int) T` carried in a record. This
  works over any backing but pays an **indirect call per element** and leans on
  the records-of-functions style.
- a **contract bound** — a named inherent-method requirement, resolved statically
  and **monomorphized to a direct backing read** (no indirection). This is the
  inherent-method model the language already favors.

We take the contract route. The win: generality *without* per-element indirection
(`c.get(i)` compiles to the concrete backing's `array.get` / byte read after
monomorphization), and the inline `slice(...) == lit` allocations from the audit
disappear with **no public compare primitive** added — they become ordinary
generic loops over the bound.

## The gap, and how we close it

Access needs an **element type determined by Self**, which Self-only contracts
can't carry. Associated types are an explicit non-goal
([design/contracts.md](../design/contracts.md)). We close the gap with the
smallest sufficient extension:

**Parameterized contracts, kept under *determined conformance*.**

Twinkle contracts follow **determined conformance** (see
[design/contracts.md](../design/contracts.md)): conformance is *determined by the
receiver, not searched*. A contract names a set of required inherent methods, each
found by name (one function per type — no candidate set), and the receiver
determines everything. Parameterized contracts just extend that one step:

- A contract may take type parameters: `IndexRead<E>`.
- The receiver **determines** the parameter (a **functional dependency `Self →
  E`**): `Vector<T>` determines `IndexRead<T>`; `String` determines
  `IndexRead<Byte>`. A type satisfies `IndexRead<E>` for *at most one* `E`, so
  given a concrete `Self` the checker recovers `E` with no search and no ambiguity.

This is associated-type *behavior* (one element type per container) expressed as a
determined parameter — no projection syntax, no associated-type machinery. It
stays inside the existing non-goals: no impl blocks, no instance search, no
dynamic dispatch, fully monomorphized.

**Checking rule.** At a bound `C: IndexRead<E>`:
- `C` concrete → resolve the required methods by name, recover `E` from whichever
  ones mention it, and check the rest for consistency (determinacy guarantees one
  `E`).
- `C` generic → `E` is carried as another parameter of the enclosing generic, and
  the call site discharges it when `C` is made concrete.

## The contracts

```tw
contract IndexRead<E>    { len(self) Int        at(self, Int) E }   // backs `v[i]`; unchecked, traps OOB
contract IntoIterator<E> { iter(self) Iterator<E> }                 // non-indexable iterables; see below
contract IndexWrite<E>   { set_at(self, Int, E) Self   append(self, E) Self }  // unchecked, traps OOB
contract Sliceable       { slice(self, Int, Int) Self }             // Self-only; `foo[a..b]` syntax → sliceable.md
```

(Contract syntax is illustrative — these are compiler-recognized, like the
existing three. `Sliceable` introduces no element type, so it already fits the
current Self-only model; only the `<E>` contracts need the extension above.)

**`at` vs `get` (locked).** `IndexRead`'s accessor is the **unchecked
`at(self, Int) E`** — it returns the element directly and **traps on OOB**, exactly
matching today's `v[i]` / `s[i]` semantics. The safe `get(self, Int) E?` stays the
ergonomic surface on `Vector` but is **not** part of the contract; generic
algorithms iterate over `range(len())`, so they never go OOB and need no
unwrapping. This is what makes positional `[]` desugar straight to `IndexRead.at`
(below) — the operator and the contract accessor are the same thing.

**`set_at` vs `set` (locked).** `IndexWrite`'s mutator is the **unchecked
`set_at(self, Int, E) Self`** (traps on OOB), the write dual of `at`: read `at`,
write `set_at`, both positional and "…at [index]". The existing checked
`set(self, Int, E) Self?` stays the ergonomic surface on `Vector` (it is *not* the
contract method — they cannot share a name, since method resolution is by name and
`set` already returns `Self?`). `append(self, E) Self` already matches the builtin.
`put` was rejected — its associative/map connotation belongs to the future keyed
contract, not positional write.

**Positional `[]` is in scope, not optional.** Backing the `[]` access syntax is a
*motivation* for this contract, not a follow-on: the plan is **done only when
`v[i]` for any `IndexRead<E>` satisfier desugars to `c.at(i)`** (so `View` gets
bracket indexing for free). `synth_index` (checker.tw:4159), which today
hardcodes `Vector`/`String`/`Dict`, routes positional indexing through the
contract. **Keyed `[]` stays separate** — `Dict<K,V>[K] -> V?` remains
special-cased (associative, a future `KeyedRead<K,V>`, see below); only the
positional, `Int`-indexed, trap-on-OOB `[]` is unified under `IndexRead`.

## Satisfiers

**Builtin (compiler-registered inherent methods):**

| Type | Satisfies |
|---|---|
| `Vector<T>` | `IndexRead<T>`, `IntoIterator<T>`, `IndexWrite<T>`, `Sliceable` |
| `String` | `IndexRead<Byte>`, `IntoIterator<Byte>`, `Sliceable` (`sub` = substring, O(m)) |
| `Range` | `IntoIterator<Int>` |
| `Dict<K,V>` | `IntoIterator<…>` (entries) — later |

**Stdlib (ordinary inherent methods):**

- `View<C>` ([view.md](view.md)) — `IndexRead<E>` (`at` delegates to `source.at`),
  all O(1) window ops. **Landed** as the first stdlib satisfier.
- `Stack<T>` — was never made a satisfier (a LIFO abstraction shouldn't expose
  positional access), and the `@std.stack` wrapper itself was later **removed**
  (2026-05-30) as unused — see [stack.md](stack.md). `Vector.drop_last` is the
  lasting LIFO primitive.

`View` satisfies the same contracts as the builtins, so it plugs straight into the
generic algorithms below — and views compose (a `View` over a `View`).

### Conformance audit (what actually has to change)

Satisfaction is **structural through inherent methods**, not `impl` blocks: a
builtin satisfies a contract when its existing inherent methods match the required
name *and* signature, plus a compiler-registered satisfaction rule. So conforming
`Vector`/`Dict` is reconciliation, not a rewrite — and it is **not symmetric**.

**`Vector` — almost conforms; two real gaps.** It already exposes `len`, `get`,
`set`, `slice`, `push` as builtins (`boot/compiler/codegen/runtime/arr.tw`). Blockers:

- **`get` returns `A?`, not `A`** (confirmed by `vector_get_optional_return.tw`:
  `xs.get(pos)` yields `Int?`). **Resolved:** `IndexRead` requires the unchecked
  `at(self, Int) E` (traps on OOB, like `xs[i]`), and `get -> A?` stays the
  ergonomic surface outside the contract. `Vector`/`String` need an `at` inherent
  method registered (the trap-on-OOB read that `v[i]` already lowers to). This is
  also the desugaring target for positional `[]`.
- **Naming**: `Sliceable` requires `sub` but `Vector` has `slice`; `IntoIterator`
  requires `iter` but for-in is a builtin today. Reconcile by either naming the
  contract methods `slice`/(existing iteration hook) or adding `sub`/`iter`
  aliases. Pure naming, no runtime change.

**`Dict` — positional contracts do not apply.** `IndexRead`/`IndexWrite`/`Sliceable`
are **positional** (`get(self, Int)`); `Dict` is **associative** (keyed by `K`,
HAMT). Forcing `Dict` to satisfy `IndexRead<Int → V>` is a category error. `Dict`
naturally satisfies **only `IntoIterator<E>`** (entries/keys/values). Generic
*associative* access, if ever wanted, is a **separate future contract** —
e.g. `KeyedRead<K, V>` with `get(self, K) V?` — explicitly **not** one of these
four. Keeping positional and keyed access as distinct contracts is a deliberate
non-goal of this plan.

Sequencing: define the contracts and the requirement-model extension first
(below), then a per-type registration pass; the data structures themselves don't
change.

## Write-once generic algorithms

```tw
fn starts_with<C: IndexRead<E>, E: Eq>(hay: C, needle: C) Bool {
  if needle.len() > hay.len() { return false }
  for i in range(needle.len()) {
    if hay.get(i) != needle.get(i) { return false }
  }
  true
}

fn position<C: IndexRead<E>, E: Eq>(xs: C, target: E) Int? { … }
fn fold<C: IndexRead<E>, A, E>(xs: C, init: A, f: fn(A, E) A) A { … }
```

Each monomorphizes to direct backing access per `C`. The audit's
`s.slice(a,b) == lit` and `trimmed.slice(0,3) == "///"` sites become
`region_eq` / `starts_with` calls over the bound — allocation-free, and with no
parallel public compare API ([slice-performance.md](slice-performance.md) Tier 1).

## Syntax hooks

Backing these surface syntaxes is a primary motivation for the contracts — each is
part of "done", not optional polish:

- `v[i]` (positional, `Int`-indexed) → `IndexRead<E>` (`c.at(i)`, unchecked, traps
  OOB). Replaces the hardcoded `Vector`/`String` arms in `synth_index`
  (checker.tw:4159); **keyed `Dict<K,V>[K] -> V?` stays special-cased** (future
  `KeyedRead<K,V>`).
- `for x in c` → **`IndexRead<E>` for indexable bounds, `IntoIterator<E>` otherwise.**
  A generic `C: IndexRead<E>` already has `len`+`at`, so `for x in c` lowers to the
  **same indexed loop** the concrete collections use — no iterator/closure allocation
  (the existing fast path is preserved; only generic *non-indexable* receivers go
  through `IntoIterator.iter`). See the iteration decision below.
- `v[a..b]` (range-slice) → `Sliceable.slice` — **tracked in a separate plan**
  ([sliceable.md](sliceable.md)); `Sliceable` is Self-only and needs none of this
  doc's parameterized-contract machinery, so it lands on its own schedule.

Add these rows to the syntax-hook table in
[../contracts.md](../contracts.md) once implemented.

## Naming: the `drop` family

With `drop_first`/`drop_last` settled on `Vector`/`View` ([view.md](view.md),
[stack.md](stack.md)), **"drop" is Twinkle's verb for removing elements from an
end/position**, and `take`/`drop` is the complementary pair. The iterator
combinators **`drop`/`drop_while`** keep one mental model:

- `iter.drop(n)` / `iter.drop_while(p)` — drop from the front.
- `drop_first()` is conceptually `drop(1)`; `drop_last()` is its back equivalent.
- `take` / `take_while` are the natural complement.

This follows the FP lineage (Scala/Kotlin/Clojure/Haskell/Elixir use `drop`)
rather than the stream lineage (`skip` in Rust/LINQ/Java Streams).

**Done** — `prelude/iterator.tw` already exposes `drop`/`drop_while` (no `skip`),
and no `.skip(` call sites remain. This rename is *not* part of the
access-contracts work; it shipped separately.

## How it fits the family

- **Access contracts** (this doc) — the general bound; write-once generic access,
  monomorphized to direct reads.
- **`View<C>`** ([view.md](view.md)) — concrete zero-copy window; a satisfier.
- **`Stack<T>` / `drop_last`** ([stack.md](stack.md)) — LIFO; a satisfier plus the
  runtime shrink op.
- **RRB** ([rrb-vector-concat.md](archive/rrb-vector-concat.md)) — makes `Vector`'s own
  `sub`/`concat` O(log n).
- **Tier 1** ([slice-performance.md](slice-performance.md)) — the hot byte loop
  stays direct `s[i]`.

## Resolver findings — determinacy is free; the real gap is representational

Checked the boot checker's contract machinery (`boot/compiler/contracts.tw`,
`boot/compiler/checker.tw`, `boot/compiler/core_linker/contract_resolve.tw`).

**The functional dependency `Self → E` is guaranteed by construction — no new
coherence machinery needed**, because *determined conformance* already holds for
the existing contracts. Each required method resolves by *name* to *exactly one*
function per type: `prove_contract_method` (checker.tw:679) calls
`resolve_method_func_name` (checker.tw:1610), which is just
`env.lookup_method(TypeName, name) String?` — a single name, no candidate set, no
overlapping-instance search (instance search is already a categorical non-goal),
and duplicate inherent method names on a type are illegal. So `E`, recovered from
whichever requirement mentions it (e.g. `get`'s return), is necessarily unique:
the receiver determines it. There is nothing extra to enforce.

**What actually needs building is the requirement model, which is hardcoded
Self-only today:**

- `ContractMethodRequirement.ret` is a **closed enum** `ContractReturnShape = {
  String, Bool, Order }` (contracts.tw:11) — only fixed concrete returns.
  `IndexRead.get` must return the FD-bound `E`, `Sliceable.sub` must return `Self`,
  `IntoIterator.iter` returns `Iterator<E>`. → needs new return shapes (`Self`,
  `Elem`, `Iterator<Elem>`).
- `prove_contract_method` unifies **every non-receiver parameter with the receiver
  type** (checker.tw:713-723) — correct for `Eq.eq(self, other: Self)`, but wrong
  for `IndexRead.get(self, i: Int)` (arg is `Int`) and `IndexWrite.set(self, Int,
  E)` (args `Int`, `E`). → needs a per-parameter *shape* vocabulary (`Self`,
  `Int`, `Elem`) instead of "all args are Self".
- The genuinely new logic: instead of checking `ret == fixed_type` (checker.tw:728),
  **bind** `Elem :=` the resolved method's actual return type and thread it so the
  call site's `E` is determined and sibling methods (`set`/`iter`) are checked
  against the same `Elem`. This stays determined conformance — bind-and-thread, not
  search — and it mirrors the existing element recursion in
  `try_builtin_container_contract` (checker.tw:773, which already destructures
  `.Vector(elem)` and recurses).

So: the FD is safe; implementation = extend `ContractMethodRequirement` (per-arg
shapes + `Elem`/`Self`/`Iterator<Elem>` returns) and make `prove_contract_method`
bind `Elem` rather than assuming Self-typed args and a fixed return.

## Decisions (locked 2026-05-29)

- **Read accessor** — `IndexRead` requires the unchecked **`at(self, Int) E`**
  (traps on OOB, matching `v[i]`); `get -> E?` stays the ergonomic surface outside
  the contract. Generic algorithms iterate over `range(len())`, so no unwrapping.
- **`[i]` element-indexing syntax is in scope** — positional `v[i]` desugars to
  `IndexRead.at`; the plan is "done" only once `synth_index` routes positional
  indexing through the contract (`View`/`Stack` then get `[i]` for free). Keyed
  `Dict[K] -> V?` stays special-cased — associative access is a future
  `KeyedRead<K, V>`, not unified.
- **`[a..b]` range-slice syntax is a SEPARATE plan** ([sliceable.md](sliceable.md)).
  `Sliceable` is Self-only and needs none of this doc's machinery; it tracks its
  own `synth_index` `Range`-index arm and satisfiers there.
- **Bound syntax** — `E` is **declared explicitly** (`fn f<C: IndexRead<E>, E>`),
  inferred at call sites. No implicit-introduction machinery.
- **Method naming** — contract methods **match the names builtins already expose**
  where possible: `Sliceable.slice` (not `sub`). The new methods are the **unchecked
  positional accessors** `at` (read) and `set_at` (write), which intentionally differ
  from the checked `get`/`set` (`-> E?`/`-> Self?`) because a type cannot expose two
  methods of the same name with different return types. `append` matches the builtin.
- **Contract names** — `IndexRead` / `IndexWrite` / `IntoIterator` / `Sliceable`
  (kept; not collapsed to `Indexable`).
- **`len` placement** — kept on `IndexRead` (no separate `Countable`/`Sized`).
- **`IntoIterator` element** — reuses the builtin `Iterator<E>`.
- **Iteration layering (locked 2026-05-30)** — `for x in c` over a generic
  `C: IndexRead<E>` lowers to **indexed iteration** (`range(len())` + `at(i)`), the
  same allocation-free path the concrete collections already use; routing it through
  `IntoIterator.iter` (which returns an `Iterator<E>`) would force an iterator +
  closure allocation per loop, so it is **not** used for indexable receivers.
  `IntoIterator` earns its keep for genuinely **non-indexable** iterables (lazy
  streams, `Dict` entries, ranges-as-streams). `Iterator.unfold` stays the low-level
  iterator **constructor**: index-backed types can derive `iter` generically via
  `unfold` over `range(len())`, and custom iterables implement `iter` with `unfold`
  directly. `IntoIterator` is layered on the iterator machinery, not a replacement
  for `unfold`.
- **`IndexWrite` shape** — `set_at` + `append`, both returning `Self` (persistent
  rebinding); `set_at` is unchecked (traps on OOB), the write dual of `at`. The
  checked `set(self, Int, E) Self?` stays the ergonomic surface, outside the
  contract. `drop_last` stays the dedicated runtime op ([stack.md](stack.md)), not
  part of the contract.
- **`skip`→`drop` rename** — already shipped, *not* part of this work (see
  [Naming: the `drop` family](#naming-the-drop-family)).

### Implementation progress

Track A — the requirement-model / proof-side foundation:

1. ✅ **Per-parameter shapes** (contracts.tw) — `arg_shapes: Vector<ContractArgShape>`
   replaces the bare count (`Receiver`/`Int`/`Elem`); `ContractReturnShape` gains
   `Int`/`Receiver`/`Elem`. (commits `eb28842`, `6b2681f`)
2. ✅ **`Elem` binding** (checker.tw `prove_contract_method`) — fresh element meta
   per proof, threaded through arg/return shapes; return check switched from strict
   equality to unification so `Elem`/`Receiver` returns bind against the satisfier's
   actual type. The three builtin contracts exercise none of the new paths, so it's
   behavior-preserving; full suite green + fixed point. (commit `6b2681f`)
3. ✅ **`IndexRead<E>` contract + parameterized bound** (`len(self) Int`,
   `at(self, Int) E`). Wired through every `BuiltinContract` switch; the bound's
   declared `E` threads through `ScopedContractMatch` so `c.at(i)` types as `E` and
   `c.len()` as `Int`. `Vector.at` (unchecked `xs[index]`) added to the prelude →
   `Vector<T>` satisfies `IndexRead<T>`. Proof recovers `Elem` from the satisfier's
   actual `at` return. Tested: checker-level typing/proof/rejection + a runtime test
   that `Vector` satisfies *and executes* generic `at`/`len` over the bound.
   (commits `9fd1273` plumbing, `631a7c8` contract). **Bonus:** contract-call
   codegen already handles arbitrary contract methods generically — no
   monomorphization change needed for `at`/`len`.
4. 🔶 Write-once `find`/`position`/`fold`/`region_eq`/`starts_with` over the bound.
   **Landed:** the direct algorithms work and are tested
   (`position_via_index_read`/`region_eq_via_index_read`/`starts_with_via_index_read`
   in `api_vector_suite`).
   - **Checker FD fix:** a parameterized bound (`C: IndexRead<E>`) now threads its
     declared `E` into the contract proof so the functional dependency binds it
     *before* a sibling `E: Eq` is checked. Previously the Eq check saw an unbound
     meta. (`checker.tw`: `prove_contract`/`prove_contract_method` gained an
     `elem_hint`; `check_*_bounds` compute it via `bound_elem_hint`; proof cache is
     skipped when a hint must bind.)
   - **Monomorphization FD fix:** a type param that appears *only* in a contract
     bound (e.g. `E` in `region_eq<C: IndexRead<E>, E: Eq>(a: C, b: C, n: Int)` — `E`
     is in no value parameter) is invisible to signature-only `infer_call_subst`, so
     the specialized body kept a free `Var(E)` and crashed codegen
     (`val_type_of_mono called on Var`) whenever both `==`/`!=` operands were
     contract-method calls. Now `infer_call_subst` walks the callee body and, for
     each contract call whose receiver is concrete under the partial subst, resolves
     the target method and matches its concrete return type back against the call's
     declared result type — the mono-time analogue of the checker's `Self -> Elem`
     recovery. (`monomorphize.tw`: `augment_subst_from_contract_calls`.)
   - **Bound-forwarding fix (checker):** *forwarding* a bound-only `E: Eq` to
     another generic over a **type-variable receiver** now works — e.g.
     `starts_with<C: IndexRead<E>, E: Eq>` delegating to `region_eq(hay, needle, n)`.
     Previously it reported `type ?N does not satisfy Eq`: proving `Var(C): IndexRead`
     via the receiver's in-scope bound returned `Ok` without binding the proof's
     element hint, so the callee's element meta stayed unbound and the sibling Eq
     check failed. Fix: when a type variable carries the contract via an in-scope
     bound (`scoped_bound_for_contract`), unify the proof's `elem_hint` with that
     bound's declared element type (`bind_elem_hint_to_scoped_bound`), tying the
     callee's element to the caller's `E`. Tested end-to-end (checker proof in
     `checker_suite`; runtime delegation in `api_vector_suite`).

   - **`String` satisfies `IndexRead<Byte>`:** added an unchecked
     `String.at(self, Int) Byte` (same trap-on-OOB semantics as `s[index]`) so the
     contract proof recovers `E = Byte` from `at`'s return; the checked form stays
     `get(s, i) Byte?`. `region_eq`/`starts_with` over the bound now run on `String`
     allocation-free — the substitution target for the slice-perf `s.slice(a,b) == lit`
     sites. (`prelude/string.tw`.)
   - **`[i]` syntax → `IndexRead.at`:** `c[i]` on a type variable bounded
     `IndexRead<E>` now type-checks to `E` and lowers to a `ContractCall(IndexRead,
     "at", c, [i])`. `synth_index` records the contract call (keyed by the `Index`
     expr id) for a `.Var` receiver with an in-scope `at`; `lower_index` reads that
     record and emits the contract call. Concrete `Vector`/`String`/`Dict` receivers
     carry no record and keep the direct `Index` op (so `String.at`'s `s[index]`
     body does not recurse). Tested: checker typing (`c[0]` is `Var(E)`) + runtime
     (`index_via_bracket`, and `a[i] == b[i]` composing with the mono FD fix).

   - **`IndexWrite<E>` (done):** `set_at(self, Int, E) Self` (unchecked write dual of
     `at`) + `append(self, E) Self`, both returning `Self`. Wired through every
     `BuiltinContract` switch; `Vector.set_at` added (rebinds via the unchecked
     index-assignment lowering) so `Vector<T>` satisfies `IndexWrite<T>`. The existing
     `Receiver` return shape covered both methods — no new shape needed. The checked
     `Vector.set -> Vector<T>?` is untouched. Tested with generic `set_at`/`append`
     over the bound. Multi-bound `C: IndexRead<E> + IndexWrite<E>` composes.

   - **`for x in c` over `C: IndexRead<E>` (done).** Lowers to the existing indexed
     loop — no iterator/closure allocation — driven by the contract's `len`/`at`,
     which monomorphize to the satisfier's direct ops. `iterable_binding_info_of`
     gained a `.Var` arm binding the element to the bound's `E`; since the element
     can't be recovered at lowering from the receiver `Var(C)`, the checker records
     it per iterable (`for_elem_types`, threaded `CheckResult` → `LowerCtx`) and
     `build_indexed_loop` emits `IndexRead.at`/`len` contract calls for a `Var` iter
     (concrete receivers unchanged). Verified index-based lowering (no
     `Iterator.next`) and the `for x, i in c` indexed form over `Vector`/`String`.

   - **`IntoIterator<E>` (done).** `iter(self) Iterator<E>`, for non-indexable
     iterables. Added the `IteratorElem` return shape (`Iterator<E>`, the builtin
     `Iterator` TypeId 4); wired through every `BuiltinContract` switch. `for x in c`
     over a generic `IntoIterator<E>` (and not `IndexRead`) lowers to `c.iter()` (an
     `IntoIterator.iter` contract call typed `Iterator<E>`) and reuses the iterator
     loop; `IndexRead` is preferred when a Var has both (indexed, allocation-free).
     `iterable_binding_info_of` resolves the element via `iterable_var_match`
     (`at` then `iter`); `bind_iterable_vars` records the `iter` contract call so
     `setup_indexed_iter` wraps the receiver. Tested with a `Countdown` user type
     via both `c.iter().to_vector()` and `for x in c`.
     - **Known limitation:** a builtin type's *prelude-Twinkle* methods are not in the
       linker's contract method table (only signature/codegen methods and
       `is_builtin_method_type` prelude methods register under the `t{tid}` key the
       contract resolver uses). So `Iterator` itself can't yet satisfy `IntoIterator`
       via an identity `iter`. User-type and signature-backed satisfiers resolve fine;
       this is the contract receiver counterpart of the `is_builtin_method_type` list.

   `View<C>` ✅ landed as the first stdlib satisfier (`@std.view`, see
   [view.md](view.md)): a zero-copy window whose own `len`/`at` make it satisfy
   `IndexRead<E>`, confirmed by passing a `View<Vector<Int>>` through a generic
   `<C: IndexRead<E>, E>` bound (the element `E` is recovered through the *nested*
   backing bound, no registration code). `Stack` was considered and **deliberately
   excluded** (2026-05-30): a LIFO abstraction shouldn't expose positional access —
   see [stack.md](stack.md). **This plan is now complete**: all three contracts, the
   `[i]`/`for x in` syntax, and the sole stdlib satisfier (`View`) have landed.

   - **Concrete bound type args (done).** `resolve_ast_type_params` previously mapped
     *every* bound type arg to `MonoType.Var(name)`, so a concrete `C: IndexRead<Int>`
     became the rigid `Var("Int")` and unifying a satisfier's real `Int` `at` return
     against it failed ("uninstantiated type variable 'Int' reached unification") —
     only the type-variable spelling worked. Fixed by threading `env` and resolving
     each bound arg with `resolve_type_expr` against the sibling type-param names: a
     declared param resolves to `Var`, anything else to a concrete type. `IndexRead<E>`
     still stores `Var("E")` (no regression); `IndexRead<Int>` now stores `Int`. This
     unblocks element-monomorphic algorithms (e.g. `IndexRead<Int> + IndexWrite<Int>`
     "double in place"), since there is no numeric contract to bound an abstract `E`.

**Boundary finding (the doc's Resolver findings under-billed this).** Steps 3–4 are
not testable end-to-end without a sliver of Track B: a contract proof is only
*triggered* from a `<C: IndexRead<E>>` bound, but today the parser reads a bound as
a single identifier (parser.tw:268, no `<args>`) and `ResolvedContractRef =
{ Builtin(BuiltinContract) }` carries no type arg. So the next unit must also:
**(B-sliver)** parse a parameterized bound and store its type arg on the bound, so
`c.at(i)` types as the declared `E`. That confirms Track B (parameterized bounds)
is the general foundation, not a mechanical add-on.

**stage0:** untouched and correct — the generic contract model is boot-only; stage0
hardcodes Stringify/Eq/Ord (`src/types/check.rs`) and only needs work once the
*bootstrapped* sources (prelude/boot) adopt the new bounds. Boot tests run on
`target/twk`, so the feature is validated boot-first.

Deferred to follow-up commits (still part of "done" for *this* plan): **`[i]`
element-index syntax wiring** through `IndexRead.at`, and **`View`
registration** as satisfiers. (`[a..b]` slicing is out — see
[sliceable.md](sliceable.md).)
