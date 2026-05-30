# Collections & Access Plan

## Goal

Track the cluster of plans that make **collection access** — indexing, slicing,
concatenation, traversal — both **cheap** (no needless O(n)/O(n²) copying) and
**general** (one `find`/`fold`/`region_eq`/`starts_with` that works over
`Vector`, `String`, `View`, `Stack`, …). These docs grew out of one boot-compiler
audit and share a single design spine, so they're tracked together here rather
than as loose cross-cutting entries.

The unifying constraints (see the per-doc detail):

* **Persistent value model** — every "mutating" op rebinds to a new value; ops
  return `Self`, structure is shared where possible.
* **No per-element indirection** — general access goes through *contracts*
  resolved statically and **monomorphized to a direct backing read**, not through
  captured closures.
* **Boot leads, stage0 mirrors** — runtime changes land in
  `boot/compiler/codegen/runtime/arr.tw` first, then `src/runtime/arr.rs`;
  differential tests + the self-host fixed point gate them.

---

## The thread

[slice-performance.md](slice-performance.md) audited where the boot compiler
actually pays for `slice`/`concat`. It split the problem into:

1. **LIFO drop-last / head-tail** — the real, hot pattern. Solved cheaply by an
   O(1)-amortized `drop_last` runtime op ([stack.md](stack.md)). **Shipped.** (A
   thin `Stack<T>` wrapper was tried and removed — `drop_last`/`append`/`last`
   already give a stack; the wrapper had no users.)
2. **Read-only windows / traversal** — solved by a zero-copy `View<C>`
   ([view.md](view.md)), which needs a **general access bound**
   ([access-contracts.md](access-contracts.md)) to reach elements without a
   closure.
3. **Arbitrary concat (prepend) & arbitrary-range / left-drop slice** — the
   general O(log n) fix is an RRB-tree `Vector`
   ([rrb-vector-concat.md](rrb-vector-concat.md)). **Prioritized (2026-05-30)** —
   unparked despite the Gate A red audit, as a deliberate design decision to make
   `Vector` the single universal sequence (stack/queue/deque/rope) and supply the
   cheap `drop_first`/`prepend` half.

So the live frontier is: **RRB-tree `Vector`** (the universal-sequence
investment). Access-contracts + `View` are done; `drop_last` shipped; the
`@std.stack` wrapper was tried and removed (the capability lives on `Vector`).

---

## Plan Index

| Plan | Scope | Status | Details |
|------|-------|--------|---------|
| Slice usage audit | Boot-compiler `slice`/`concat` audit + String-slice perf discussion — the evidence behind the rest | **Audit done** (Vector LIFO landed; String-slice → `View`) | [slice-performance.md](slice-performance.md) |
| `drop_last` | O(1)-amortized `Vector.drop_last` runtime op; LIFO pop sites migrated (a thin `Stack<T>` wrapper was tried and removed) | **Implemented** | [stack.md](stack.md) |
| Access contracts | Parameterized contracts `IndexRead<E>` / `IntoIterator<E>` / `IndexWrite<E>` with a `Self → E` functional dependency; write-once generic access monomorphized to direct reads; positional `v[i]` desugars to `IndexRead.at` (in scope for "done") | **Done** — all three contracts + `v[i]` + `for x in` landed; `View` is the stdlib satisfier; `Stack` deliberately excluded then removed | [access-contracts.md](access-contracts.md) |
| `Sliceable` / `[a..b]` | Range-slice indexing `foo[a..b]` → `Sliceable.slice`; Self-only contract, needs none of the parameterized-contract machinery | **Proposal — split from access-contracts** | [sliceable.md](sliceable.md) |
| `View<C>` | Zero-copy windows (backing + `start`/`count`) over any `IndexRead` backing; O(1) `drop_first`/`drop_last`/`sub` | **Shipped** (`@std.view`) | [view.md](view.md) |
| RRB-tree `Vector` | O(log n) `concat`/`slice` via relaxed radix-balanced nodes; kills O(n²) prepend-concat and left-drop loops; adds cheap `drop_first`/`prepend` (queue/deque) | **Prioritized — next (2026-05-30)**, unparked as a design decision (Vector = universal sequence) | [rrb-vector-concat.md](rrb-vector-concat.md) |

---

## Dependency & implementation order

```
slice-performance (audit) ──┬─> drop_last ..................... DONE (Stack wrapper removed)
                            ├─> access-contracts ──┬─ view .... DONE
                            │                       └─ sliceable ([a..b], Self-only; own schedule)
                            └─> rrb-vector-concat ............. NEXT (unparked — universal Vector)
```

1. ~~**`drop_last`**~~ — the audit's real hot path. **Done.** (The `Stack<T>` wrapper
   it once carried was removed — no users.)
2. **Access contracts** — the foundation `View` and write-once `find`/`fold`/
   `region_eq` all sit on. Implementation-ready: the functional dependency is free
   under *determined conformance*; the work is extending the requirement model in
   `boot/compiler/contracts.tw` + `checker.tw` (per-arg shape vocabulary +
   `Elem`/`Self`/`Iterator<Elem>` return shapes + bind-and-thread `Elem`), then
   mirroring to stage0. **Completion includes wiring positional `v[i]` through
   `IndexRead.at`** (routing `synth_index`'s hardcoded `Vector`/`String` arms;
   keyed `Dict[K] -> V?` stays special-cased) — backing `[]` is a motivation, not
   a follow-on.
3. ~~**`View<C>`**~~ — **Done.** Shipped as `@std.view`; satisfies the access
   contracts itself, so views compose.
4. **RRB-tree `Vector`** — **next.** Unparked as a design decision (make `Vector`
   the single universal sequence: stack/queue/deque/rope), *not* gated on a
   measured hot loop. Start with Gate B baselines (`boot/bench/`) to quantify
   today's curves and set the bar, then the relaxed-node implementation.

---

## Shared design notes

* **Determined conformance** ([design/contracts.md](../design/contracts.md)) —
  conformance is determined by the receiver, never searched: each contract method
  resolves by name to exactly one function per type. Parameterized contracts just
  add a receiver-determined parameter (`Self → E`); no instance search, no
  associated-type machinery, no dynamic dispatch.
* **Monomorphize to direct reads** — `c.get(i)` compiles to the concrete backing's
  `array.get` / byte read after monomorphization, so generality costs nothing per
  element. This is why the contract route is preferred over a closure capability.
* **No new public compare primitive** — the inline `slice(...) == lit` allocations
  become ordinary generic loops over the bound; `region_eq` stays private.
* **Totality** — window/shrink ops (`drop_first`/`drop_last`) are total: an empty
  input returns empty, never traps, matching `Vector.drop_last`.
* **Backing retention caveat** ([view.md](view.md)) — a long-lived `View` over a
  large backing keeps the whole backing alive; that's the deliberate V1 trade
  (localized retention vs reshaping `String`).

---

## Decisions (all locked 2026-05-29)

Full rationale in [access-contracts.md](access-contracts.md#decisions-locked-2026-05-29);
summarized here:

* **Read accessor** — `IndexRead.at(self, Int) E`, unchecked (traps OOB), matching
  `v[i]`; `get -> E?` stays the ergonomic surface outside the contract.
* **`[i]` scope** — positional element indexing `v[i]` desugars through
  `IndexRead.at` and is part of access-contracts "done"; keyed `Dict[K] -> V?`
  stays special-cased (future `KeyedRead<K,V>`).
* **`[a..b]` scope** — range-slice syntax is a **separate plan**
  ([sliceable.md](sliceable.md)); `Sliceable` is Self-only, no machinery needed.
* **Bound syntax** — `E` declared explicitly (`fn f<C: IndexRead<E>, E>`).
* **Method naming** — match existing builtin names (`slice`); the new methods are the
  unchecked positional accessors `at` (read) / `set_at` (write), distinct from the
  checked `get`/`set` (`-> E?`/`-> Self?`) since a type can't have two same-named
  methods with different returns. No other aliases.
* **Contract names** — `IndexRead`/`IndexWrite`/`IntoIterator`/`Sliceable` (kept).
* **`len` placement** — on `IndexRead` (no separate `Countable`).
* **`IndexWrite`** — `set_at` + `append`, both return `Self` (`set_at` unchecked,
  traps OOB; checked `set -> Self?` stays outside the contract).
* **Iteration layering (locked 2026-05-30)** — `for x in c` over `C: IndexRead<E>`
  lowers to the existing **indexed loop** (no iterator/closure allocation);
  `IntoIterator` is reserved for **non-indexable** iterables, and `Iterator.unfold`
  stays the iterator constructor that `iter` is built on. See
  [access-contracts.md](access-contracts.md#decisions-locked-2026-05-29).
* **`skip`→`drop`** — already shipped; not part of this work.

First-commit scope (checker foundation + `Vector`/`String`, deferring `[i]` wiring
and `View`/`Stack` registration) is in
[access-contracts.md](access-contracts.md#first-commit-scope).
