# `Sliceable` — `foo[a..b]` range-slice syntax

Status: proposal. Splits off from [access-contracts.md](access-contracts.md) (which
defines the parameterized read/write/iterate contracts and wires *element*
indexing `foo[i]`). This plan covers **range-slice indexing** `foo[a..b]` over the
`Sliceable` contract. Tracked under [collections-access.md](collections-access.md).

## Why a separate plan

`Sliceable` is **Self-only** (`sub`/`slice(self, Int, Int) Self`), so unlike
`IndexRead`/`IndexWrite`/`IntoIterator` it needs **none** of the
parameterized-contract / functional-dependency machinery that access-contracts
builds. Its only real work is a new **`[]` operator arm** that dispatches on a
`Range` index. Keeping it separate keeps the access-contracts foundation small and
lets the slice syntax land (or wait) on its own schedule.

It depends on access-contracts only for the contract-registration plumbing (the
per-type satisfaction pass); the `Sliceable` contract shape itself is plain.

## The mapping

`foo[a..b]` desugars to `foo.slice(a, b)` — the `Sliceable.slice` inherent method.
This makes `[]` overload on the **index expression's type**, the Rust model:

| `foo[idx]` where idx: | Contract | Method | Returns |
|---|---|---|---|
| `Int` | `IndexRead<E>` ([access-contracts.md](access-contracts.md)) | `at(self, Int) E` | element, traps OOB |
| `Range` | `Sliceable` (this plan) | `slice(self, Int, Int) Self` | sub-sequence |
| `K` (Dict) | future `KeyedRead<K,V>` | keyed get | `V?` |

The pieces already exist:

- `a..b` is already a first-class **`Range` value** — `synth_range_op`
  (`boot/compiler/checker.tw`) types `a..b` as `Range` (TypeId 3) with both ends
  unified to `Int`. Same `..` used in `for i in 0..5`.
- `synth_index` (`boot/compiler/checker.tw`, the `.Index(base, idx)` arm) currently
  handles only an `Int` index (`Vector`/`String`) plus the `Dict` keyed arm. The
  change is to add a **`Range`-typed index arm** that resolves `Sliceable.slice` on
  `base` and returns `Self`.

## The contract

```tw
contract Sliceable { slice(self, Int, Int) Self }
```

**Method naming (locked, matching access-contracts):** the contract method is
`slice` (the name `Vector`/`String` already expose), not `sub` — no aliases.

## Satisfiers

- `Vector<T>` — `slice` builtin already present (O(m) today; O(log n) post-RRB, see
  [rrb-vector-concat.md](rrb-vector-concat.md), parked).
- `String` — `slice` = substring (O(m)).
- `View<C>` ([view.md](view.md)) — `slice` = O(1) window adjust (`sub`).

So once this lands, `view[1..n]`, `vec[1..n]`, `str[1..n]` all work uniformly.

## Caveats (locked)

1. **Contiguous only.** `Range` can carry a step (`range_step`), but
   `slice(self, Int, Int)` is step-1. A stepped range in slice position is rejected
   (strided slice is out of scope).
2. **Both bounds required.** The grammar's `range_expression` is `expr '..' expr`,
   so `foo[1..]` / `foo[..n]` / `foo[..]` do not parse today. Open-ended slices
   would need grammar work — out of scope; it is always `foo[a..b]`.
3. **Half-open `[a, b)`** — matches `0..5` iterating 0–4 and `slice` semantics. No
   surprise.

## Scope / done

- Add the `Range`-index arm to `synth_index` → resolve `Sliceable.slice`, return
  `Self`; reject stepped ranges in slice position.
- Register the `Sliceable` contract + `Vector`/`String`/`View` satisfiers
  (`slice`). Mirror to stage0.
- Lower `foo[a..b]` to the `slice` call in codegen (both compilers).
- Tests: `vec[a..b]`, `str[a..b]`, `view[a..b]`, OOB/empty edges, stepped-range
  rejection. Green on `make boot-test` + the self-host fixed point.

## Open questions

- **Lowering path** — does `foo[a..b]` lower through the same desugaring as an
  explicit `foo.slice(a, b)` call (preferred — one code path), or get a dedicated
  index node? Lean: desugar to the method call early so existing `slice` codegen +
  the RRB upgrade apply unchanged.
- **Stepped range in slice position** — hard compile error vs runtime trap. Lean:
  compile error (the type is known: a `Range` literal with a step).
- **Assignment form** `foo[a..b] = xs` (splice) — out of scope here; would need an
  `IndexWrite`-style range setter. Note and defer.
