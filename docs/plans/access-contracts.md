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
contract IndexRead<E>    { len(self) Int        at(self, Int) E }  // backs `v[i]`; unchecked, traps OOB
contract IntoIterator<E> { iter(self) Iterator<E> }              // backs `for x in`
contract IndexWrite<E>   { set(self, Int, E) Self   append(self, E) Self }
contract Sliceable       { slice(self, Int, Int) Self }          // Self-only; `foo[a..b]` syntax → sliceable.md
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

**Positional `[]` is in scope, not optional.** Backing the `[]` access syntax is a
*motivation* for this contract, not a follow-on: the plan is **done only when
`v[i]` for any `IndexRead<E>` satisfier desugars to `c.at(i)`** (so `View`/`Stack`
get bracket indexing for free). `synth_index` (checker.tw:4159), which today
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

- `View<C>` ([view.md](view.md)) — `IndexRead<E>`, `IntoIterator<E>`, `Sliceable`;
  all O(1) window ops, `get` delegates to `source.get`.
- `Stack<T>` ([stack.md](stack.md)) — `IndexRead<T>` (`top` = `get(len-1)`),
  `IndexWrite<T>`.

Because `View` and `Stack` satisfy the same contracts as the builtins, they plug
straight into the generic algorithms below — and views compose (a `View` over a
`View`).

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
- `for x in v` → `IntoIterator<E>` — generalizes today's builtin iteration to any
  satisfier (currently only the builtin collections iterate).
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
- **RRB** ([rrb-vector-concat.md](rrb-vector-concat.md)) — makes `Vector`'s own
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
- **Method naming** — contract methods **match the names builtins already expose**:
  `IntoIterator` wired to the existing `for`-in hook; `Sliceable.slice` (not `sub`).
  The only new method is `at` (the unchecked read). No duplicate aliases.
- **Contract names** — `IndexRead` / `IndexWrite` / `IntoIterator` / `Sliceable`
  (kept; not collapsed to `Indexable`).
- **`len` placement** — kept on `IndexRead` (no separate `Countable`/`Sized`).
- **`IntoIterator` element** — reuses the builtin `Iterator<E>`.
- **`IndexWrite` shape** — `set` + `append`, both returning `Self` (persistent
  rebinding). `drop_last` stays the dedicated runtime op ([stack.md](stack.md)),
  not part of the contract.
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
4. ⬜ Write-once `find`/`position`/`fold`/`region_eq`/`starts_with` over the bound;
   `IntoIterator`/`IndexWrite` specs; register `String` (`IndexRead<Byte>`) and
   `View`/`Stack`; the `[i]` syntax wiring through `IndexRead.at`.

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
element-index syntax wiring** through `IndexRead.at`, and **`View`/`Stack`
registration** as satisfiers. (`[a..b]` slicing is out — see
[sliceable.md](sliceable.md).)
