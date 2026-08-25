# Interprocedural MutVec — S4 first slice (caller-born; param-sourced later)

**Status:** Plan draft (revised twice after implementation-path review)
**Track:** Sound uniqueness & mutable lowering → **storage S4** (owned-specialized
mutable ABI across calls). The S4 *design* already exists — see
[sound-uniqueness/storage/README.md → S4](sound-uniqueness/storage/README.md#s4--owned-specialized-mutable-abi-across-calls)
and the north-star chain model there. **This doc is the concrete execution plan
for the first S4 slice**, not a re-design. It does not restate the S4 lifecycle
contract; it points at it.

> **Framing correction (post-review).** An earlier draft read as a small extension
> to `mutvec_region.scan_uses`. That is wrong. The real work is a **new
> post-specialization interprocedural storage-decision phase** with explicit
> handle-continuation and ABI validation. The high-priority sections below (region
> discovery, pass ordering, token verifier, backend ABI contract) are the load-
> bearing parts; `scan_uses` is a small piece of one of them.

## Why this exists now (the concrete driver)

The [shared-wrapper owned specialization](archive/shared-wrapper-owned-specialization.md)
lever landed (commit `dc164937`, doc archived): an owned indexed update written
through a wrapper (`xs = xs.set_at(i, v)`) now publishes an owned variant, and 8G
clones + routes it so owned callers get in-place while non-owned callers keep the
persistent generic. On AWFY `sieve` this took **35.28ms → 5.03ms (7×)** and
self-hosts, all tests green.

But it stops at **5ms, not `sieve_mut`'s 0.86ms**. The residual is a
**representation** gap, not an ownership gap:

- The owned clone `set_at__Bool$v…` emits `vector$set_in_place` — the *persistent*
  PVec in-place **trie** write (O(log n), structured storage).
- Direct index-assign in the birth function (`flags[k] = false`) instead reaches a
  flat **MutVec** region (`array.set`, O(1), no-escape scratch → no freeze) → 0.86ms.

The flat MutVec cannot span the `run → set_at`-clone boundary. This is exactly the
S4 "param-sourced / stay-low-across-a-chain" case the storage track already scoped
as open.

## Scope of "no analysis change" (reworded)

**No ownership-summary change is intended.** `summary.tw` / 8G have already done
their job: ownership is proven, the owned variant is published, and the owned
caller is routed (that is *why* `vector$set_in_place` is selected instead of a full
copy). This plan consumes those facts unchanged.

**But S4 does add new codegen/storage-selection analysis**: interprocedural region
discovery, call-chain handle-lifecycle validation, ABI-compatibility and
materialization-exit proof, and fallback. That analysis is codegen-owned and reads
existing ownership facts; it does not re-derive ownership.

## Target metric — and the freeze count

`sieve` (persistent `Vector<Bool>`, write through `.set_at()`) reaches
`sieve_mut` parity (~0.86ms) **with no source change** — the owned `set_at` chain
stays in flat `MutVec<Bool>` across the loop. The gate is *parity*, not merely
"improves" (see the per-call freeze/thaw risk).

**Crucial shape correction: `sieve` is a no-escape scratch chain, so the target is
ZERO freezes, not "freeze once."** `sieve.tw` returns `count`, not `flags` — the
flat vector is mutated, read-reduced to a scalar, then dropped. The MutVec region
already models this: `MutVecRegion.escapes == false` emits no `mutvec_freeze`
(commit `f83ecf78`; the census renders this chain as `scratch`). Slice 1 must
support **both** exit shapes, matching the existing S2 one-exit rule:

- **no-escape scratch chain → 0 `mutvec_freeze`** (`sieve`: reduce to scalar, drop);
- **escaping vector result → exactly one boundary `mutvec_freeze`** (a caller that
  returns / publishes the mutated vector).

Everywhere this doc previously said "freeze once after the loop," read "materialize
at the single boundary **if the chain escapes; otherwise not at all**."

---

## High-priority design (the load-bearing parts)

### HP-1 — Region discovery must be producer-rooted, not set-base-rooted

`mutvec_region.detect_in_func` seeds candidates from `collect_set_bases(f.body)` —
every local `vector$set_unsafe` base. In the target shape the caller's handle is
**only passed to a wrapper call**; the actual set is inside the callee clone. So
the caller handle is *never in `bases`* and is never scanned. Extending
`scan_uses` alone is insufficient — there is no candidate to scan.

**Step:** add a second candidate source — **producer-rooted call-thread regions**:
a locally-born producer (`collect`/`make`) whose handle is threaded through one or
more accepted MutVec-ABI calls (and possibly local reads), with the loop-carried
rebind coming back from those calls. This runs alongside the existing set-base
discovery; the two produce disjoint region kinds (local-write region vs
call-thread region). Determinism: producers in program order.

### HP-2 — Pass ordering: a new post-8G S4 phase

Current pipeline (`codegen/codegen.tw :: link_program`):

```
mutvec_region.rewrite_module      (line 135)   ← S2 local regions
builder_region.rewrite_module     (line 139)
variant_specialize.specialize_module_with_sem  (line 151)  ← creates owned clones + routes
convert_closures                  (line 174)
mutable_produce.produce_mutable_decisions       (line 187)
```

The current MutVec pass runs **before** `variant_specialize`, so it cannot see the
owned wrapper clone or the routed call sites this plan depends on.

**Step:** introduce an **S4 interprocedural MutVec phase after `variant_specialize`
and before `convert_closures`** (owned clones exist; closures not yet converted, so
handle threading is still visible as plain calls). Concretely, split the MutVec
work into two phases that share the family/repr machinery:

- **S2-local** (unchanged, stays at ~line 135): locally-born, locally-written
  regions.
- **S4-interprocedural** (new, after line 151): consumes
  `SpecializeResult.routes` + clone ids, discovers producer-rooted call-thread
  regions (HP-1), validates handle lifecycle (HP-3), rewrites caller region +
  selected clone ABI, or falls back.

Keep S2-local and S4 as separate passes; do not merge (the S2 path is proven /
byte-identical-critical).

### HP-3 — Interprocedural handle-continuation verifier

"Passing a handle to a MutVec-ABI callee and receiving it back counts as an
in-region use" is sound **only** with an explicit continuation proof. Current
`mutvec_region` tracks a single local handle and rejects `AInit` aliases; S4 needs
an alias/continuation model across the call-result local. For each routed call in
a candidate region, prove:

1. **Continuation identity:** the call's returned value is the storage-continuation
   token for the argument handle (the callee is the routed owned clone whose ABI
   returns the same backing), and the caller rebinds the region handle to exactly
   that result.
2. **Post-call invalidation (ANF terms):** rebinding is `AAssign`, not SSA, so this
   is not "no old-name use." It is: **no alias/copy of the pre-call value survives**,
   and the call result is **immediately assigned back to the region handle**
   (`AAssign(handle, call_result)`) before any further use of the handle. A copy of
   the pre-call value into another local (that outlives the call) rejects the region.
3. **No republication in the callee:** the clone does not freeze, publish, store
   into a record/global, capture into a closure, or fork the handle.
4. **All handle-carrying callee exits materializable or rejected.**
5. **Repr agreement:** caller and callee agree on element family and physical repr.

Any failure → the region is rejected and the site stays on the persistent
`vector$set_in_place` ABI (still correct, just 5ms not 0.86ms). This verifier is
the new codegen analysis referenced above; it is where most of the risk lives.

### HP-4 — Backend needs a NEW MutVec-ABI decision input (not an extension of current slot inference)

Correcting an earlier misstatement: `mutvec_repr.assign_mutvec_reprs` does **not**
set `phys_return` or caller-result slots. It only marks *local* handle slots
`MutVec(I64)` where a slot flows through a `mutvec_*` op (reading already-rewritten
IR; it re-proves nothing). It explicitly **leaves `phys_return` and caller-result
typing to `route_typed_vec`** — and even that is for the *frozen* `mutvec_freeze`
result (a `PVecI64` producer), **not** a MutVec function ABI (`mutvec_repr.tw`
header + `:223`).

So neither pass defines a **function-level MutVec param/return ABI**. S4 must
introduce one as a **new backend ABI-decision input**, keyed off the routed S4
clone, that the repr/route passes consume:

- **param slot repr override** on the S4 clone (`MutVec<fam>` for the threaded
  param) — new; today no pass types a param slot as MutVec;
- **MutVec return ABI** for the S4 clone (a MutVec-*returning* function) — new;
  distinct from `route_typed_vec`'s frozen-`PVec` producer handling;
- **call-result slot repr propagation** at each routed call site (caller result
  local typed `MutVec<fam>`) — new; today caller result slots are typed only from
  `mutvec_freeze`/typed-vec producers, not from a MutVec-returning callee;
- **guard:** a non-S4 caller must never reach the MutVec ABI (a routed call with one
  side still `PVec` traps `illegal cast`) — enforced by HP-5 route partitioning;
- **freeze adapters** only when leaving the S4 chain, and **only if it escapes**
  (no adapter for the no-escape scratch chain — see the freeze-count correction).

The current `mutvec_repr` (local slot handoff) and `route_typed_vec` (frozen-PVec
producer) remain; S4 adds the ABI-decision layer above them.

### HP-5 — Route partitioning: do not mutate the shared 8G clone's ABI in place

8G creates **one** owned clone per variant and routes **all** satisfying owned
caller sites to it. If S4 "upgrades" that clone's ABI to `MutVec`, any owned caller
routed to the same clone that is **not** in an S4-compatible region will call the
wrong ABI → `illegal cast`. The generic non-owned caller is fine (it stays on the
persistent generic, HP guard), but **owned-but-not-S4-compatible** callers are the
hazard.

**Rule:** never rewrite the existing 8G PVec-owned clone's ABI unless *every*
routed owned site is S4-compatible. Concretely:

- if all routed owned sites are S4-compatible → the existing clone may adopt the
  MutVec ABI;
- otherwise → **create a separate S4 MutVec-ABI sibling clone**, leave the 8G
  PVec-owned clone intact, and reroute **only** the S4-compatible sites to the
  sibling. Non-S4 owned sites keep calling the PVec-owned clone.

Deterministic sibling naming (e.g. `…$v…$mv`) and the existing 8G variant cap /
persistent fallback apply.

---

## First slice — scope

Narrowest end-to-end shape that exercises the full S4 loop:

> A **caller-born** owned `MutVec<fam>` (`collect`/`make`) threaded through **one**
> owned-specialized single-update wrapper clone that takes and returns the same
> handle, mutated across a loop, then either read-reduced to a scalar and dropped
> (no-escape scratch, **zero** freeze — the `sieve` case) or materialized at one
> boundary if it escapes.

This is precisely the landed `set_at` clone, upgraded from a persistent-PVec ABI to
a MutVec ABI. **Materialization stays at the S2 one-exit restriction for slice 1:**
either zero handle-carrying terminals (scratch → no freeze) or exactly one (escape →
one boundary freeze). Multi-exit / branch-carried handles are rejected in slice 1
(fallback to persistent), not handled.

### Accepted callee (wrapper clone) shape — precise

Slice-1 accepts a clone body that is a **trivial update-return-self** wrapper:

- **Allowed:** the threaded param `xs`; `xs[i] = v` (→ `mutvec_set`); reads
  (`xs.at(i)`, `xs.len()`); returning `xs` (the continuation).
- **Rejected (→ fallback):** `append` (growth ABI is a later slice); any branch
  that returns a *different* vector on some path; multiple returns not all carrying
  the same `xs` continuation; storing/capturing `xs` into a record/closure/global;
  freezing or publishing `xs`; calling another non-routed vector op on `xs`.

The verifier (HP-3) enforces these; the list is the human-readable summary of what
"continuation-preserving" means for slice 1.

### S4 producer inputs are post-`builder_region` ANF-prime forms

Unlike S2-local (which runs at `codegen.tw:135`, before `builder_region`), the S4
phase runs after `builder_region` (post-`:151`). So S4 does **not** see raw
`collect`/`make` — it sees the ANF-prime producer forms `builder_region` left
behind. A `collect` is already a builder chain (`builder_new` → `builder_push*` →
`builder_freeze`), and `builder_region` may have further rewritten empty-seed
accumulator loops into `builder_from`/`builder_new` regions.

**Step:** define the exact accepted producer shapes S4 discovers in ANF-prime, by
**reusing/extending the existing `classify_producer` / `CollectSeed`
`trace_collect_builder` tracing** (`mutvec_region.tw`) to recognize the
post-`builder_region` builder-chain forms — not the pre-`builder_region` shapes the
S2 pass matches. Concretely enumerate, with fixtures: the `collect` builder-chain
form, the `make` form (`MakeSeed`), and the array-literal form (`ArrayLitSeed`) as
they appear *after* `builder_region`. Any producer shape not on the list →
fallback (persistent), never a guess.

### Param-sourced vs caller-born (explicit separation)

- **Slice 1 (this doc's core):** the owned chain root is a **caller-born** handle
  (`collect`/`make`) threaded through one wrapper hop. `sieve` is this case.
- **Later slice:** the owned chain root is a **function-entry parameter** that is
  itself the thaw boundary (thaw `PVec` param → `MutVec` at entry under a proven
  owned ABI). `nbody`'s `advance(bodies, dt)` — where `bs` is a parameter
  (`base=persistent(aliased shell)` today) — is this case, and is a **follow-up
  fixture once slice 1 lands**, not slice-1 scope.

### Fallback / materialization rules (slice 1, concrete)

| Situation | Slice-1 behavior |
|---|---|
| Caller has a routed use **and** a non-routed use of the handle | reject region → persistent for both (no partial MutVec) |
| Unsupported call/op follows the loop on the handle | reject region → persistent |
| Returned vector stored into a record / captured | reject (fails HP-3.3) → persistent |
| A branch exits carrying the handle (multi-exit) | reject (S2 one-exit rule) → persistent |
| Routed call gone stale after a later rewrite | route invalidated → persistent, no MutVec signature emitted |
| No-escape scratch chain (handle read-reduced then dropped, e.g. `sieve`) | **zero** `mutvec_freeze` |
| Single escaping boundary use after the loop | **one** `mutvec_freeze` before the persistent/return use, not per call |

---

## Test plan (general, not sieve-specific)

**Positive:**
- **A (no-escape scratch, the `sieve` shape) — caller-born vector threaded through
  a MutVec-ABI wrapper, then read-reduced to a scalar and dropped.** `collect` +
  loop `xs = xs.wrap(i, v)`, function returns a scalar (not `xs`). Assert: caller
  births `MutVec`, clone param/return are `MutVec`, write is `mutvec_set` (not
  `vector$set_in_place`), **zero `mutvec_freeze`**, run result matches the boxed
  baseline.
- **A′ (escaping result) — same threading, but the function returns the mutated
  vector.** Assert: **exactly one** `mutvec_freeze` at the boundary (not per call),
  otherwise as A.
- **B — user-defined wrapper, non-Int family.** `Bool`/`Float` vector, user
  `fn bump(xs, i){ xs[i]=…; xs }` — proves no hard-coding to `set_at` or i64.
- **C — split callers (compose with the landed lever).** Owned caller routes to
  the MutVec-ABI clone; non-owned caller keeps persistent generic; both correct.

**Negative / soundness (each must fall back with no ABI mismatch):**
- wrapper **stores/captures** the handle (record field / closure / global);
- wrapper returns a **different / non-continuation vector** on some path (a fresh
  vector, or a different param) — note that returning the *same* backing on both
  paths is fine, so the fixture must use a genuinely different vector;
- caller keeps a **copy/alias of the pre-call value** that outlives the call, or
  fails to immediately assign the result back to the region handle (fails
  post-call invalidation, HP-3.2);
- routed owned caller **plus** generic non-owned caller to the same source
  function — clone MutVec ABI, generic persistent ABI, no cross-call;
- **stale/unsupported route** falls back without any `MutVec` call-signature
  mismatch;
- boundary use after loop inserts **one** freeze before the persistent use, **not**
  at each call.

**Signature-level inspection (not only call bodies):**
- verify the clone's param and result **Wasm types are `MutVec<fam>`** and the
  generic function's remain `PVec<fam>` / persistent ABI (catches HP-4 guard
  violations directly).

**Regression / perf:** `sieve` reaches `sieve_mut` parity; `queens` unchanged and
correct; `make bundle-cli` self-host fixed point + full boot-test + rust-test green
at every behavior-changing step.

## Non-goals

- **Wrapper inlining.** Inlining an owned trivial wrapper clone back into the
  caller so the *existing* intra-function MutVec detector fires is a real
  alternative for the sieve-class case, but it is a **separate concern with its own
  plan doc**, handled independently. This plan does the general interprocedural-
  storage path, which also unblocks `nbody`'s param-sourced `advance` (inlining
  would not, cleanly).
- **`MutDict` / S5.** Dict stay-low is a different storage target (flat `MutDict`,
  three-tier publication); out of scope.
- **Multi-hop chains, recursive/SCC edges carrying MutVec, branch/multi-exit
  handles, `append` growth ABI.** Covered by the north-star chain model; these are
  the next S4 slices once single-hop is proven.
- **Boxed/record element MutVec.** Tracked separately (mutvec-checklist Phase 7).

## Dependencies & risks

- **Depends on:** the landed shared-wrapper lever (owned variant + route already
  produced — the ABI change rides that clone). MutVec runtime ops for the target
  family already exist (Int/Bool/Float/Byte shipped).
- **Risk — per-call freeze/thaw regression.** Materializing at the call boundary
  instead of staying `MutVec` is O(n) per call — *slower* than the trie write. The
  perf gate (sieve must reach ~0.86ms, not merely improve) catches this.
- **Risk — ABI mismatch / illegal cast** (HP-4 guard). A routed call with one side
  still `PVec` traps. Covered by the self-host fixed point (boot threads owned
  vectors) and the signature-level fixtures.
- **Risk — unsound continuation acceptance** (HP-3). Accepting a
  non-token-preserving wrapper corrupts aliased state. The negative fixtures and
  the explicit verifier conditions are the guard.
- **Risk — variant/determinism explosion.** MutVec-ABI variants multiply the 8G
  clone set. Keep demand-driven + capped with persistent fallback (existing 8G
  discipline); deterministic clone names / module order (self-host fixed point).

## Pointers

- `boot/compiler/codegen/codegen.tw` — `link_program` pass order (MutVec `:135`,
  variant_specialize `:151`, closure `:174`, mutable_produce `:187`); the new S4
  phase slots after `:151`.
- `boot/compiler/codegen/mutvec_region.tw` — `detect_in_func` / `collect_set_bases`
  (`:389`/`:354`, set-base-rooted seeding — HP-1 adds a producer-rooted source),
  `classify_producer` + `MutVecProducer` (`CollectSeed`/`MakeSeed`/`ArrayLitSeed`,
  locally-born gate `:148`) and `trace_collect_builder` (the builder-chain tracer S4
  must extend for post-`builder_region` forms — producer-shape section), `eligible`
  / `scan_uses` (every-use + one-exit rules).
- `boot/compiler/codegen/variant_specialize.tw` — owned clone + `routes` (the ABI
  rides this; source of routed sites for HP-1/HP-2; HP-5 partitions these routes).
- `boot/compiler/backend/mutvec_repr.tw` — `assign_mutvec_reprs` (`:261`) marks
  **local** MutVec handle slots only and **defers `phys_return`/caller-result to
  `route_typed_vec`** (header + `:223`). HP-4 adds a **new** MutVec-ABI decision
  input above both passes; it does not extend `assign_mutvec_reprs`.
- `boot/compiler/codegen/builder_region_detect.tw` — runs before the S4 phase;
  defines the ANF-prime producer forms S4 sees.
- `docs/plans/sound-uniqueness/storage/README.md` — S4 design + lifecycle contract
  + north-star chain model (source of truth; update it first on divergence).
- `docs/plans/mutvec-later-slices.md` — S4 listed as explicitly deferred there;
  this plan executes that deferral.
- `examples/performance/awfy/twinkle/{sieve,sieve_mut,nbody}.tw` — driving
  customers + perf ceiling.
