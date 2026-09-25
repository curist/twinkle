# Aggregate MutVec ABI Rewrite — Design Notes (Tasks 4–5)

Design capture before implementing the decomposed-ABI rewrite in
`docs/plans/2026-09-25-recursive-aggregate-mutvec-abi.md`. Tasks 0–3 landed
(ownership field-lineage foundation, `detect_aggregate_regions`,
`verify_aggregate_region`, census). This doc records the rewrite design and the
one open decision surfaced during scoping, so Task 4–5 can be implemented in a
focused follow-up without re-deriving it.

## Where the tracked-red tests stand

Four tests are red **by design** until this rewrite lands
(`boot/tests/suites/mutvec_call_suite.tw`):

- `recursive record scratch gets an aggregate MutVec route` — expects
  `run_mutvec_call` to produce a `permute…$mvagg` sibling that `run` calls.
- `recursive record scratch emits mutvec_set_i64 with zero freezes`
- `recursive record escape emits exactly one boundary freeze`
- `recursive record field escape emits exactly one boundary freeze`

The last three compile the fixture to WAT and assert the `permute…$mvagg` clone
`contains("mutvec_set_i64")`, drops `rt_arr__set`, and freezes 0× (dead) / 1×
(observed).

## Core decomposition design (settled)

The owned reconstruction clone `permute(state: State, n) State` becomes a
private sibling `permute$mvagg(vhandle: MutVec, count: Int, n) Int`:

- **Return the scalar only.** The vector is *not* returned. It rides as an
  in-place mutable handle parameter. The caller passed the handle in and still
  holds it after the call (mutated in place), so it never needs to come back
  through the return.
- **Control flow carries only the scalar.** Because the handle is threaded by
  mutation through a stable local, any `if`/`loop` that used to yield the
  `State` carrier now yields just the scalar component. This is what makes the
  transform tractable — no need to return a pair or restructure control flow to
  thread two values.
- **This is uniform across all three exits.** Dead-field, publish-record, and
  publish-field differ *only* at the caller boundary (how/whether the handle is
  frozen), never inside the clone. The clone body is identical for all three.

### Record-explosion rules (ANF → ANF on the clone body)

Maintain `explode: LocalId → (vec_local, scalar_local)` for every
carrier-typed (`State`) local. Per op:

| Op on a carrier | Rewrite |
|---|---|
| `record State { vf=V, sf=S }` → `LR` | map `LR→(V, S)`; drop the record op |
| `init LX` (LX carrier) → `LR` | map `LR→explode[LX]`; drop |
| `record_get LC .vf` → `LR` | `LR = init vec(LC)` (vector-typed alias) |
| `record_get LC .sf` → `LR` | `LR = init scalar(LC)` |
| `assign LC = LX` (both carrier) | two lets: `assign vec(LC)=vec(LX)`; `assign scalar(LC)=scalar(LX)` |
| `call permute(LC, …args)` → `LR` | `scalar(LR) = call permute$mvagg(vec(LC), scalar(LC), …args)`; `vec(LR) = init vec(LC)` (same handle, mutated in place) |
| `if … then …Lc else …Lc` → `LR` (carrier) | if yields `scalar(Lc)` per branch; `LR` is scalar-typed; drop vector |
| tail `return LC` / `Atom(LC)` | `return scalar(LC)` |

The **carrier param** `state (L5)` is replaced by two params
`vhandle (fresh), count (fresh)`; `explode[L5] = (vhandle, count)`. Fresh locals
allocated from `max_local+1`. Mutable threaded carriers (e.g. `st`/`L7`, which
is re-`assign`ed) get one stable `(vec, scalar)` pair per local id so the
`assign` rule updates them consistently. `op_result_mono` must gain entries for
every new local (vector param → `Vector<Int>`; scalar locals → the scalar mono).

## The open decision: where the helper write becomes `mutvec_set`

**Both the scratch fixture and the real `examples/performance/awfy/twinkle/permute.tw`
write the vector inside a `swap` helper** (`swap(st.v, n-1, i)`), not directly
in `permute`. `swap(v: Vector<Int>, i, j) Vector<Int>` does
`v.set_at(i,b).set_at(j,a)`. No inliner runs before S4 (pipeline order:
`mutvec_region → builder_region → variant_specialize → run_mutvec_call`). So the
`set_at` is never in `permute`'s own body.

This means "retarget the clone's vector writes to `mutvec_set`" (plan Task 4
Step 4) cannot fire on `permute` — the write lives in `swap`. Two ways to make
the write in-place:

### Option A — compose with bare-vector S4 (recommended)

Thread the flat handle from `permute$mvagg` into `swap`, and let the **existing**
`mutvec_call` bare-vector path flatten `swap` to an in-place clone
(`swap` is exactly the `Vector<Int> → Vector<Int>` owned-continuation shape that
S4 already handles). `swap`'s `set_at` → `mutvec_set_i64` in **swap's** flat
clone; `permute$mvagg` calls it and keeps the same handle.

```
permute$mvagg(handle, count, n):
  ...
  call swap$mv(handle, i, j)      // in-place; returns same handle
  count2 = call permute$mvagg(handle, count, n-1)
  ...
  return count                    // scalar only

swap$mv(handle, i, j):
  mutvec_set_i64(handle, i, b)    // the write lives here
  mutvec_set_i64(handle, j, a)
```

- **Pros:** reuses machinery, reaches permute_mut/LuaJIT-class perf (the win is
  eliminating the persistent COW alloc in `set_at`, achieved either way; the
  extra call is cheap), lowest miscompilation risk, honors "do not create a
  second authority."
- **Cost:** the committed WAT tests must be relaxed to find `mutvec_set_i64` in
  `swap`'s flat clone rather than inside `permute$mvagg` (still assert no
  `rt_arr__set` in the hot path + 0/1 `mutvec_freeze`). The
  `permute$mvagg`-specific assertion is the only thing that changes.
- **Integration risk to resolve:** the bare-vector region detector is
  producer-rooted (`collect`/`make`). Here the handle originates as the
  decomposed `vhandle` **param**, not a producer. Confirm whether bare-vector S4
  fires on a param-sourced flat handle threaded into `swap`, or whether the
  aggregate pass must hand `swap`'s sites to the S4 planner explicitly. This is
  the main unknown for Option A.

### Option B — inline `swap` into the decomposed clone

Add a small helper-inliner so `swap`'s body folds into `permute$mvagg`;
`mutvec_set_i64` then appears literally inside `permute$mvagg` and the committed
tests pass verbatim.

- **Pros:** matches the tests as written; no cross-function handle threading.
- **Cons:** a new inliner surface (which owned helpers to inline, arity/local
  remapping, all-or-nothing bail), more moving parts, higher risk. Larger than
  the decomposition itself.

**Recommendation:** Option A. The performance goal (in-place write, no COW
alloc) is met by either, and A reuses the verified S4 path. Adjusting one test
assertion is cheaper and safer than a new inliner. Decide the Option-A
integration-risk question first (param-sourced bare-vector S4 firing on `swap`).

## Caller boundary rules (thaw + freeze)

At an accepted entry site `permute(State.{ v, count }, n)`:

1. **Thaw the producer.** The caller-born `v` (`collect`/`make`) flattens to a
   mutvec builder via the existing producer rewrite → a flat `handle`.
2. **Call decomposed.** `permute(State.{v,count}, n)` →
   `count' = permute$mvagg(handle, count, n)`; the record construction is dropped
   by the ordinary dead-let pass.
3. **Boundary per exit:**
   - `.DeadField` (scratch): no freeze. The scalar result feeds the observed
     `.count`; `handle` dies. **0 freezes total.**
   - `.PublishRecord` (escape): freeze `handle` once, reconstruct
     `State.{ v: frozen, count: count' }` at the boundary. **1 freeze.**
   - `.PublishField` (field_escape): freeze `handle` once, substitute the frozen
     vector at the observed `.values` projection. **1 freeze.**

Freezes never appear inside the clone or the recursion — only at the caller
exit named by the verified region. This is what the escape/field-escape tests
check (`count_op_calls(_run, "mutvec_freeze_i64") == 1` and module-wide `== 1`).

## All-or-nothing bail conditions (never partially rewrite)

Reject the whole upgrade (leave the persistent clone) if any holds:
- a recursive edge passes a handle that is not `vec(current carrier)` (a
  substituted handle);
- any carrier op cannot be mapped by the table above (unexpected shape);
- the single scalar field is not present / not ordinary-scalar repr (already
  gated by `verify_aggregate_region`);
- (Option A) `swap`'s flat clone cannot be produced for every write site;
- the variant cap is exhausted for the `$mvagg` (and `$mv`) siblings.

Fallback is always safe: the generic persistent clone stays. Reuse
`variant_specialize.variant_cap` and the existing `$mv` partition discipline so
a site belongs to exactly one physical ABI class (persistent / bare MutVec /
aggregate MutVec) before any sibling is allocated (plan Task 4 Step 6).

## Suggested implementation order for the follow-up

1. Resolve the Option-A integration question (does bare-vector S4 flatten a
   param-sourced handle threaded into `swap`? — prototype on the scratch clone).
2. New module `mutvec_aggregate_rewrite.tw`: pure `build_decomposed_sibling`
   (record-explosion, `.None` bail), unit-tested on the real specialized clone
   body before any wiring.
3. Wire the aggregate plan into `mutvec_call_phase` (sibling gen, unified
   partition, caller thaw + call rewrite) reusing the `$mv` scaffolding.
4. `mutvec_repr.apply_mutvec_call_abi_upgrades`: type `vhandle` + call args
   MutVec, `phys_return` = scalar repr; no PVec↔MutVec `ref.cast`.
5. Boundary freeze/reconstruct for `.PublishRecord` / `.PublishField` (Task 5).
6. Adjust the WAT test assertions per the chosen option; run the full suite,
   then `make bundle-cli` / `make stage2` fixed-point + AWFY perf gate (Task 6).
