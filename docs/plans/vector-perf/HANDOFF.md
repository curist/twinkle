# Typed Vector representation — handoff & review note

**Purpose:** let another session pick up the typed-`Vector<Int>` work, and let a
reviewer check it later. Written 2026-06-10. Branch: `native-typed-value-sort`
(not yet merged to `main`).

This note is the entry point. Deeper detail lives in:
- [typed-vector-spike.md](typed-vector-spike.md) — the spike plan + S1/S2.0 status (read this second).
- [typed-vector-representation.md](typed-vector-representation.md) — the long-term umbrella plan.
- [README.md](README.md) — the whole vector/sort perf endeavor index.

---

## TL;DR — where it stands

The "master lever" for numeric/dataframe perf is giving `Vector<Int>` typed `i64`
leaf storage so random reads skip the boxed-`BoxedInt` pointer-chase. Two pieces
have landed and **work** on this branch:

- **S1 — typed `PVecI64` runtime family.** Validated ~6.8× faster random reads.
- **S2.0 — source-level routing.** Idiomatic `xs := collect …; … xs[i] …` (where
  `xs` never escapes) now compiles to typed storage and gets the full ~6.8×.

**Everything is committed, self-hosts, and all 2571 boot tests pass.** It is a
real, working, conservative optimization — not a prototype. It just doesn't yet
cover vectors that *escape* (passed to functions, stored in records) — notably
dataframe columns — which is the next increment.

## Commit trail (review in order)

| commit | what |
|---|---|
| `c1e90fe` | S1: typed `PVecI64` family (struct + `len_i64`/`get_i64`/`promote`/builder ops + roundtrip test) |
| `7baa23c` | S2.0 infrastructure, committed **gated off** with a bug diagnosis (historical; superseded by next) |
| `fc93bec` | S2.0 **activated** — builder-lineage fix + cascade; idiomatic `collect`+`xs[i]` ~6.8× |

Reviewing `fc93bec` on top of `7baa23c` shows the full S2.0 change. (The gated
commit exists only so the WIP wasn't lost mid-debug; `fc93bec` is the real one.)

## Verify the current state (no code changes needed)

```bash
make bundle-cli      # must print "Fixed point reached" (self-host with routing on)
make boot-test       # must print "Ran 2571 tests: 2571 passed"

# The headline result (quiet machine — kill background CPU first; numbers
# degrade several-fold under load):
target/twk run examples/sort-bench/typed_vec_read_probe.tw
#   match=true ; boxed ~540ms ; typed ~82ms  (~6.8x)

# S1's runtime-level number, for comparison:
target/twk run examples/sort-bench/value_sort_micro.tw   # native xs.sort()
```

If `match=false`, or the typed/boxed gap collapses, the routing regressed.

## What works, and what doesn't (scope)

**Works:** a `Vector<Int>` built by `collect` (or `[…]` literal) and used **only**
by `xs[i]` indexed reads and `xs.len()`, all within one function. Such a vector
never crosses a representation boundary, so it is stored as `PVecI64` end-to-end
with **no coercion** needed.

**Does NOT route (stays boxed, unchanged):** any `Vector<Int>` that escapes —
passed to a function, returned, stored in a record/variant, captured by a
closure, `.append`/`.sort`/`.map`'d, stringified, etc. The escape analysis is
deliberately conservative; if in doubt it leaves the vector boxed. This is why
**dataframe `order_by` is unaffected** (its `IntCol(Vector<Int>)` columns cross
boundaries) — correct, but it means the headline dataframe win is still pending.

## Architecture map (the S2.0 touch points)

Routing runs **after** boundary insertion + repr assignment
(`backend/prepare.tw` calls `route_typed_vectors` last). The pass therefore has
to reproduce how the boxed builder is already represented — that's where the
subtlety is.

- `backend/route_typed_vec.tw` — **the pass.** Per function: find a
  `collect`-built `Vector<Int>` (`v = builder_freeze(b)`), escape-analyze `v`
  (only `xs[i]`/`len` allowed), trace the builder lineage backward through
  `AInit` copies (copy-map), then: swap `builder_new/push/freeze`/`len` →
  `_i64`, retype `v`'s slot to `PVecI64`, and **re-erase** the builder-lineage
  slots to `OpaqueAnyref`/anyref.
- `runtime/arr.tw` + `runtime/types.tw` — the `PVecI64` family (S1).
- `builtins.tw` — the `_i64` builtins (abi + `rt`, `.None` canonical).
- `emit/arrays.tw` — `xs[i]` routes to `get_i64` when the base wasm type is
  `PVecI64` (`is_pvec_i64`).
- `emit/runtime_abi.tw` + `emit/calls.tw` — the `_i64` builder ops skip the
  mono-driven result adaption and get the `anyref→Array` builder-arg cast
  (`is_builder_buffer_arg`, `is_builder_seed`, `is_typed_vec_freeze`).
- `backend/verify_slots.tw` — verifier accepts a `PVecI64` wasm type for a
  `Vector<Int>` slot (`is_typed_vec_i64`).

## Gotchas / landmines (things that bit, don't re-trip them)

1. **`concat_trees` is not leaf-agnostic** — it casts leaves to boxed `Array`
   for RRB rebalancing. The typed builder uses a **radix** append (`push_tail` +
   manual root-overflow growth) instead; correct because builders only produce
   strict vectors. Don't route the typed builder through `concat_trees`.
2. **The builder is threaded through copies** (`builder_new`→temp, then
   `AInit` copy→builder local). Match the lineage by the copy-map, not a single
   slot id, or `builder_new` stays boxed → illegal cast.
3. **Routing after boundary insertion** means the typed slots must mimic the
   boxed slots' erasure (builder slots = `OpaqueAnyref`/anyref). This "boundary
   churn" is the tax of the post-repr approach; the alternative (route before
   boundary insertion) was not taken.
4. **Benchmark hygiene:** numbers degrade 3–5× under background CPU load
   (a running game bit this repeatedly). Check `ps aux | sort -k3` first.
5. **Boot-only, no stage0 parity:** all of this is codegen/runtime in boot's
   Twinkle source; stage0 just compiles it. Self-host convergence is the gate.

## What's next (the decision for the next session)

S2.0 proves the mechanism but only helps non-escaping local vectors. To reach the
real dataframe `order_by` win, a typed vector must survive **crossing a boundary**.
That needs, roughly in increasing order of effort:

1. **Boundary coercion** `PVecI64 ↔ PVecAnyref` (box/unbox each element), emitted
   where an escaping typed vector meets boxed-expecting code — so eligibility can
   relax beyond "never escapes". The plan's `typed-vector-representation.md`
   "Representation-boundary policy" section frames the choices (erase-at-boundary
   vs specialize-by-representation vs adapter-shim).
2. **Cross-function monomorphic typed ABIs** — let a monomorphized `fn(xs:
   Vector<Int>)` take `PVecI64`, so the column flows typed through `gather`/the
   comparator without re-boxing.
3. Typed `gather`/`sort`/`map`, then `Float`/`Bool`/`Byte` families.

Open question worth resolving early: **route after boundary insertion (current,
with the erasure-mimicry tax) or before it (cleaner typing, but re-do the pass on
a different IR).** S2.0 chose "after" and made it work; "before" may be cleaner
for the coercion work.

## Review checklist

- [ ] `make bundle-cli` reaches fixed point; `make boot-test` 2571/2571.
- [ ] `typed_vec_read_probe.tw` shows `match=true` and a ~6× gap.
- [ ] Read `fc93bec`'s diff against `7baa23c`; sanity-check the escape analysis
      in `route_typed_vec.tw` (does any escaping use slip through?).
- [ ] Confirm dataframe (`examples/dataframe/bench/main.tw`) still behaves.
- [ ] Decide the boundary-coercion policy before extending eligibility.
