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
   O(1)-amortized `drop_last` runtime op + a `Stack<T>` wrapper
   ([stack.md](stack.md)). **Shipped.**
2. **Read-only windows / traversal** — solved by a zero-copy `View<C>`
   ([view.md](view.md)), which needs a **general access bound**
   ([access-contracts.md](access-contracts.md)) to reach elements without a
   closure.
3. **Arbitrary concat (prepend) & arbitrary-range / left-drop slice** — the
   general O(log n) fix is an RRB-tree `Vector`
   ([rrb-vector-concat.md](rrb-vector-concat.md)). **Parked** — the audit found
   no hot caller for it (Gate A red).

So the live frontier is: **access-contracts (foundation) → view (consumer)**,
with stack already done and RRB parked until a workload justifies it.

---

## Plan Index

| Plan | Scope | Status | Details |
|------|-------|--------|---------|
| Slice usage audit | Boot-compiler `slice`/`concat` audit + String-slice perf discussion — the evidence behind the rest | **Audit done** (Vector LIFO landed; String-slice → `View`) | [slice-performance.md](slice-performance.md) |
| `drop_last` + `Stack<T>` | O(1)-amortized `Vector.drop_last` runtime op + thin `Stack<T>` wrapper; LIFO pop sites migrated | **Implemented** | [stack.md](stack.md) |
| Access contracts | Parameterized contracts `IndexRead<E>` / `IntoIterator<E>` / `IndexWrite<E>` with a `Self → E` functional dependency; write-once generic access monomorphized to direct reads | **Proposal — next** (resolver gap scoped) | [access-contracts.md](access-contracts.md) |
| `View<C>` | Zero-copy windows (backing + `start`/`len`) over any `IndexRead` backing; O(1) `drop_first`/`drop_last`/`sub` | **Proposal — blocked on access-contracts** | [view.md](view.md) |
| RRB-tree `Vector` | O(log n) `concat`/`slice` via relaxed radix-balanced nodes; kills O(n²) prepend-concat and left-drop loops | **Parked — Gate A red (2026-05-29)** | [rrb-vector-concat.md](rrb-vector-concat.md) |

---

## Dependency & implementation order

```
slice-performance (audit) ──┬─> stack/drop_last ............... DONE
                            ├─> access-contracts ── view ...... NEXT (foundation → consumer)
                            └─> rrb-vector-concat ............. PARKED (revisit on demand)
```

1. ~~**`drop_last` + `Stack<T>`**~~ — the audit's real hot path. **Done.**
2. **Access contracts** — the foundation `View` and write-once `find`/`fold`/
   `region_eq` all sit on. Implementation-ready: the functional dependency is free
   under *determined conformance*; the work is extending the requirement model in
   `boot/compiler/contracts.tw` + `checker.tw` (per-arg shape vocabulary +
   `Elem`/`Self`/`Iterator<Elem>` return shapes + bind-and-thread `Elem`), then
   mirroring to stage0.
3. **`View<C>`** — pure stdlib once access-contracts lands; first consumer is the
   inline `slice(...) == lit` sites and head/tail recursion (e.g. `emit/match.tw`).
4. **RRB-tree `Vector`** — only revisit if a prepend-`concat` accumulator loop or a
   left-drop / `drop_first` dequeue loop becomes dominant. Start with Gate B
   benchmarks (`boot/bench/`) to confirm the quadratic curve before any
   relaxed-node code.

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

## Open decisions (gating access-contracts implementation)

From [access-contracts.md](access-contracts.md)'s open questions — mostly have
"Lean:" defaults, listed here so they're tracked in one place:

* **Bound syntax** — does `C: IndexRead<E>` auto-introduce `E`, or must it be
  declared (`fn f<C: IndexRead<E>, E>`)? Lean: declared explicitly.
* **`len` placement** — on `IndexRead`, or a separate `Countable`? Lean: keep on
  `IndexRead`.
* **Naming** — `IndexRead`/`IndexWrite` vs `Indexable`; `Sliceable` vs `Slice`
  (avoid collision with the old type name, now `View`).
