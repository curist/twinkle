# Milestone A — Storage-Site Typed Vectors — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the conservative typed-vector storage policy to typed sum/variant payloads (`IntCol(Vector<Int>)` → `PVecI64` in the variant struct) so variant-held columns get typed *direct* reads — without reintroducing the M1a per-read pathology.

**Architecture:** Mirror the proven S2.2 typed-record-field mechanism for variant payloads. A whole-program analysis marks `(TypeId, VariantId, payloadIdx)` sites typed; `wasm_layout` emits `PVecI64` for those payload slots; `route_typed_vectors` retypes the match-extracted read slots (or boxes on escape); `emit_coerce_stack` places one `box_i64`/`unbox_i64` at each `PVecI64↔PVec` crossing (including the erased-Variant bridge). `repr_of_mono(Vector<Int>)` stays `TypedRef`; `PVecI64` is site-driven only. Escape analysis keeps captured/escaping vectors boxed, so no typed vector reaches the `anyref` closure env.

**Tech Stack:** Boot compiler (`boot/`). Verify: `make bundle-cli` (self-host to fixed point), `make boot-test`, and `.tw` probes built to `.wat` then grepped. Boot-only.

**Spec:** [storage-site-typed-vectors.md](storage-site-typed-vectors.md).

---

## Canonical identifiers (use verbatim everywhere)

- **Payload site key:** `"${tid.id}:${vid.id}:${payloadIdx}"` — `TypeId.id`,
  `VariantId.id`, payload index. (Record-field keys are 2-part `"${tid.id}:${fieldIdx}"`;
  payload keys are 3-part, so they never collide.)
- **Env/module field:** `typed_vector_payloads: Dict<String, Bool>` (mirrors
  `typed_vector_fields`).
- **Classifier:** `candidate_typed_vec_family(mono) ElemRepr?` (renamed from
  `elem_repr_of_vector`).

## Key facts (do not re-derive)

- **S2.2 record-field template:** `env.typed_vector_fields` keyed
  `"${tid.id}:${fieldIdx}"`; `wasm_layout.tw:264` emits `PVecI64` when the key is
  present **and** the field type is `Vector<Int>`; produced by
  `analyze_typed_fields` (`route_typed_vec.tw:799`); threaded
  `PreparedModule` (`prepare.tw:37,89`) → `env` (`codegen.tw:86`) → layout.
- **Field-read retyping (mirror for payloads):** `route_func`
  (`route_typed_vec.tw:82`) calls `collect_typed_field_reads` (:91) to gather the
  slots that read a typed field, then retypes them to `PVecI64` (:141).
- **Sum layout:** `layout_of_sum_def` (`wasm_layout.tw:296`) builds
  `payload_types := collect f in v.fields { val_type_of_mono(subst_type_params(f,…)) }`.
- **Erased bridge:** `emit_sum_to_variant_helper` (`emit/bridge_funcs.tw:19`)
  `StructGet`s each payload then `emit_box_to_anyref`; `emit_variant_to_sum_helper`
  (:57) reads them back expecting the sum's `payload_types`. A `PVecI64` payload
  must be `box_i64`'d into the `anyref` Variant array and `unbox_i64`'d back.
- **Three repr paths** in `repr_assign.tw` return `TypedRef` for vectors:
  `repr_of_mono` (:231), `repr_of_named_cached` (:260), `cached_repr_of_mono` (:315).
- **Coercion** (`emit/coercions.tw`) already does `PVecI64↔PVec` both ways.

## Note on granularity

Plumbing/layout/bridge tasks carry complete-enough code and concrete gates. The
two analysis-and-routing tasks (T4, T5) extend an ~800-line producer/consumer/
alias analysis; each is split into focused sub-steps with exact targets and a
concrete WAT/probe gate, since the internal code is emergent.

## File / task map

| Task | Responsibility | Files |
|------|----------------|-------|
| T1 | Classifier unification (candidate ≠ actual) | `repr_policy.tw`, `repr_assign.tw` |
| T2 | `typed_vector_payloads` plumbing + fix test fixtures | `resolver.tw`, `prepare.tw`, `codegen.tw`, 3 test suites |
| T3 | Typed variant payload layout (guarded) | `wasm_layout.tw`, `wasm_layout_suite.tw` |
| T4 | Analysis: mark typed payload sites | `route_typed_vec.tw`, `prepare.tw` |
| T5 | Route/coerce the extracted payload reads | `route_typed_vec.tw` |
| T6 | Audit payload emission incl. erased bridge | `emit/bridge_funcs.tw`, `emit/variants.tw`, `emit/calls.tw` |
| T7 | Guards: scoped positive probe, capture tripwire, order_by | `examples/…`, `boot/tests/…` |

---

## Task 1: Classifier unification (candidate ≠ actual)

**Files:** `boot/compiler/backend/repr_policy.tw`, `boot/compiler/backend/repr_assign.tw`, `boot/tests/suites/repr_policy_suite.tw`

- [ ] **Step 1: Rename the classifier**

In `repr_policy.tw`, rename `elem_repr_of_vector` → `candidate_typed_vec_family`
(same body/signature), doc: "the *candidate* typed element family; whether a
*site* is actually typed is decided elsewhere." Update the call in
`repr_policy_suite.tw`.

- [ ] **Step 2: One shared vector-default helper**

```tw
// backend/repr_assign.tw — single source of truth for the *default* vector repr.
// Vectors are TypedRef (boxed PVec ABI) by default; PVecI64 comes only from
// site-aware sources (route_typed_vec slots, typed-storage layout), never here.
fn vector_default_repr(mono: MonoType) ReprKind {
  .TypedRef(mono)
}
```

Replace the vector arms at `:231`, `:260`, `:315` to call `vector_default_repr(mono)`
(wrapping in `.{ repr: …, cache }` where the cached paths require it).

- [ ] **Step 3: Self-host + commit**

```bash
make bundle-cli && make boot-test
git add boot/compiler/backend/repr_policy.tw boot/compiler/backend/repr_assign.tw boot/tests/suites/repr_policy_suite.tw
git commit -m "backend: unify vector repr default; rename candidate classifier (no behavior change)"
```
Expected: fixed point; green; all three still return `TypedRef`.

---

## Task 2: `typed_vector_payloads` plumbing (inert) + fixtures

**Files:** `resolver.tw`, `backend/prepare.tw`, `codegen/codegen.tw`, and the test
suites `typed_record_fields_suite.tw`, `backend_verify_suite.tw`, `codegen_emit_suite.tw`.

- [ ] **Step 1: Add to `ResolvedEnv`**

In `resolver.tw` beside `typed_vector_fields` (:129): add
`typed_vector_payloads: Dict<String, Bool>`; in the constructor (:149):
`typed_vector_payloads: Dict.new()`.

- [ ] **Step 2: Add to `PreparedModule` + thread**

`prepare.tw`: add `typed_vector_payloads: Dict<String, Bool>` to `PreparedModule`
(:37); both returns (:84, :89) add `typed_vector_payloads: Dict.new()`.
`codegen.tw` (:86): add `env2.typed_vector_payloads = prepared.typed_vector_payloads`.

- [ ] **Step 3: Fix every `PreparedModule` test fixture**

```bash
grep -rn "PreparedModule.{\|typed_vector_fields:" boot/tests/suites/typed_record_fields_suite.tw boot/tests/suites/backend_verify_suite.tw boot/tests/suites/codegen_emit_suite.tw
```
Add `typed_vector_payloads: Dict.new()` to each `PreparedModule.{ … }` literal that
sets `typed_vector_fields`. (Missing-field construction is a compile error, so the
build tells you which remain.)

- [ ] **Step 4: Self-host + commit**

```bash
make bundle-cli && make boot-test
git add boot/compiler/resolver.tw boot/compiler/backend/prepare.tw boot/compiler/codegen/codegen.tw boot/tests/suites/*.tw
git commit -m "backend: add typed_vector_payloads plumbing (empty, inert) + fixtures"
```

---

## Task 3: Typed variant payload layout override (guarded)

**Files:** `boot/compiler/codegen/wasm_layout.tw` (`layout_of_sum_def`), `boot/tests/suites/wasm_layout_suite.tw`

- [ ] **Step 1: Failing test**

Mirror `test_layout_record_vector_int_field_is_pveci64`: build an env with a sum
type (variant index 0, one `Vector<Int>` payload), set
`base.typed_vector_payloads["${tid.id}:0:0"] = true`, `layout_of`, assert the
variant `payload_types[0]` is `.Ref(_, .Named("rt_types__PVecI64"))`. Run → FAIL.

- [ ] **Step 2: Implement the guarded override**

In `layout_of_sum_def`'s payload loop, compute the canonical key
`"${tid.id}:${vid.id}:${payloadIdx}"` and emit
`.Ref(true, .Named("rt_types__PVecI64"))` **only when** the key is present in
`env.typed_vector_payloads` **and** the (substituted) field type is `Vector<Int>`
— otherwise the existing `val_type_of_mono(field_ty, env)`. (Mirror the two-part
guard at `wasm_layout.tw:264`.)

- [ ] **Step 3: Pass; self-host; commit**

```bash
target/twk run boot/tests/main.tw   # new test PASS
make bundle-cli && make boot-test
git add boot/compiler/codegen/wasm_layout.tw boot/tests/suites/wasm_layout_suite.tw
git commit -m "codegen: typed Vector<Int> variant payloads in layout_of_sum_def (guarded)"
```

---

## Task 4: Analysis — mark typed payload sites

Split into focused sub-steps. Extends `analyze_typed_fields` machinery in
`route_typed_vec.tw`; each sub-step compiles and self-hosts before the next.

**Files:** `boot/compiler/backend/route_typed_vec.tw`, `boot/compiler/backend/prepare.tw`

- [ ] **Step 1: Payload-site stat + key helper**

Add a `payload_key(tid, vid, idx) String` producing the canonical 3-part key, and
a per-key `PayloadStat` (mirror `FieldStat`: `has_ok_producer`, `bad_producer`,
`bad_consumer`). Add a `payload_stats: Dict<String, PayloadStat>` alongside the
field stats in `analyze_typed_fields`.

- [ ] **Step 2: Producer scan — typed vector into variant construction**

In the producer classification (`classify_v_group`), recognize a typed `v`-group
whose sole escape is a **variant construction** placing `v` in payload site
`(tid, vid, idx)` (the variant analogue of a single typed-field store). Mark that
key's `has_ok_producer`; a `v` that also escapes elsewhere sets `bad_producer`.

- [ ] **Step 3: Consumer scan — match-arm payload reads (egress-once allowed)**

Recognize `case … .Variant(x) => …` payload bindings. A consumer that reads `x`
only via typed index/`len`, or transfers it into another typed site, is
typed-compatible. A consumer that lets `x` **escape to anyref/closure** is a
**one-time boxed egress — allowed** (do NOT set `bad_consumer`; the extraction
coerces once). Set `bad_consumer` only for representation-incompatible uses (e.g.
storing a boxed `PVec` back into the same typed payload site).

- [ ] **Step 4: Emit the payload set + wire into prepare**

Have `analyze_typed_fields` (or a sibling returning both) also produce
`Dict<String,Bool>` of payload keys where
`has_ok_producer and !bad_producer and !bad_consumer`. In `prepare.tw:89`, set
`typed_vector_payloads:` from it (replacing the T2 `Dict.new()`).

- [ ] **Step 5: Gate (analysis only — layout typed, reads not yet routed)**

```bash
target/twk build examples/performance/sort-bench/typed_variant_payload_probe.tw -o /tmp/p.wat
grep -c rt_types__PVecI64 /tmp/p.wat   # >= 1: the variant struct field is now typed
make bundle-cli && make boot-test      # fixed point + green
```
(The dedicated probe is created in T7 Step 1; if running T4 first, add it now.)

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/prepare.tw
git commit -m "backend: mark typed Vector<Int> variant payload sites (storage-site analysis)"
```

---

## Task 5: Route / coerce the extracted payload reads

**Why (#2):** typing the struct field is not enough — the match extraction
`case IntCol(v) => v` does `StructGet(payload) → LocalSet(v)`. `v`'s slot must be
`PVecI64` (typed read) when non-escaping, or the extraction must `box_i64` into a
boxed `v` when it escapes. Mirror the existing field-read retyping.

**Files:** `boot/compiler/backend/route_typed_vec.tw`

- [ ] **Step 1: Thread the payload set into routing**

Extend `route_typed_vectors` / `route_func` to accept `typed_payloads:
Dict<String,Bool>` (from `prepare.tw`), alongside `typed_fields`.

- [ ] **Step 2: Collect typed-payload read slots**

Add `collect_typed_payload_reads(body, typed_payloads, acc)` mirroring
`collect_typed_field_reads` (:322): for each `AMatch` arm binding a payload from a
typed payload site, record the bound slot id. Retype those slots to `PVecI64` when
the bound value does not escape (same escape check the field-read path uses); when
it escapes, leave the slot boxed so `emit_coerce_stack` inserts one `box_i64` at
the `StructGet(PVecI64)`→boxed-slot store.

- [ ] **Step 3: Gate — variant column direct read is typed**

```bash
target/twk build examples/performance/sort-bench/typed_variant_payload_probe.tw -o /tmp/p.wat
grep -c rt_arr__get_i64 /tmp/p.wat     # >= 1: the extracted column reads typed
make bundle-cli && make boot-test
```

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/backend/route_typed_vec.tw
git commit -m "backend: route typed variant-payload reads (retype non-escaping, box on escape)"
```

---

## Task 6: Audit payload emission — incl. the erased bridge (#3)

**Why:** any path that computes a variant payload wasm type from bare
`val_type_of_mono(payload_mono)` loses the typed payload; and the erased-Variant
bridge must box/unbox a `PVecI64` payload or it corrupts the `anyref` Variant.

**Files:** `boot/compiler/codegen/emit/bridge_funcs.tw`, `emit/variants.tw`, `emit/calls.tw`

- [ ] **Step 1: Bridge — box typed payloads on erase, unbox on ingress**

In `emit_sum_to_variant_helper` (`bridge_funcs.tw:19`): when a payload's **layout
type** is `PVecI64`, emit `box_i64` before `emit_box_to_anyref` (so the `anyref`
Variant array holds a boxed `PVec`, not a bare `PVecI64`). In
`emit_variant_to_sum_helper` (:57): when the target payload type is `PVecI64`,
`unbox_i64` after reading the `anyref` payload back. Drive the decision from the
layout's `payload_types` (which now honor `typed_vector_payloads`), not from
`val_type_of_mono`.

- [ ] **Step 2: Audit constructor / extractor paths**

```bash
grep -rn "val_type_of_mono\|payload_types\|StructNew\|StructGet" boot/compiler/codegen/emit/variants.tw boot/compiler/codegen/emit/calls.tw
```
Confirm each variant-construction / match-extraction site derives payload wasm
types from `layout_of_sum_def` (which honors `typed_vector_payloads`), routing any
that recompute from `val_type_of_mono(payload_mono)` through the layout.

- [ ] **Step 3: Gate — self-host + equality/stringify over typed-payload sums**

```bash
make bundle-cli && make boot-test   # exercises Eq/Stringify over sums via the bridge
```
Expected: fixed point + green (the bridge round-trips typed payloads correctly).

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/codegen/emit/bridge_funcs.tw boot/compiler/codegen/emit/variants.tw boot/compiler/codegen/emit/calls.tw
git commit -m "codegen: box/unbox typed variant payloads across the erased bridge; audit payload paths"
```

---

## Task 7: Guards — scoped positive probe, capture tripwire, order_by

**Files:** create `examples/performance/sort-bench/typed_variant_payload_probe.tw`
and `typed_payload_capture_guard.tw`; use `dataframe/bench/order_by_breakdown.tw`.

- [ ] **Step 1: Dedicated positive probe (no bare typed local) — fixes #1**

Create `typed_variant_payload_probe.tw` containing **only** a variant-payload
path: build a `Vector<Int>`, wrap it in a variant, extract and read it via index —
**no** bare non-escaping `collect`+read (which would emit `get_i64` on its own and
mask the variant result). Then `grep -c rt_arr__get_i64` is unambiguous: any hit
comes from the variant payload read.

```bash
target/twk build examples/performance/sort-bench/typed_variant_payload_probe.tw -o /tmp/p.wat
grep -c rt_arr__get_i64 /tmp/p.wat     # >= 1
target/twk run examples/performance/sort-bench/typed_variant_payload_probe.tw   # correct checksum
```

- [ ] **Step 2: Capture tripwire — timing threshold (fixes #6)**

Create `typed_payload_capture_guard.tw`: store a `Vector<Int>` in a variant,
extract it, **capture into a closure** read in a loop (200k reads over a
5000-element column — the `sort_by` shape), print elapsed ms. The captured column
must be boxed (escape → `PVec`), so reads are O(1); a regression to per-read
`unbox_i64` is O(n) and ~1000× slower. Assert the **elapsed prints < 200 ms** (the
M1a branch was ~8100 ms). Timing is the robust, unambiguous tripwire; whole-file
`grep` of `unbox_i64` is unreliable and is NOT the gate.

```bash
timeout 15 target/twk run examples/performance/sort-bench/typed_payload_capture_guard.tw
# must print an elapsed well under 200ms and exit 0 (not time out)
```

- [ ] **Step 3: order_by — improvement without regression**

```bash
timeout 60 target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw
```
Expected: completes (the `timeout` is the loud tripwire); `gather`/`take`/direct
reads improve vs the pre-A baseline (N=1M: gather 3 cols ~412ms, take ~418ms);
`sort` unchanged (boxed comparator — M1b).

- [ ] **Step 4: Commit**

```bash
git add examples/performance/sort-bench/typed_variant_payload_probe.tw examples/performance/sort-bench/typed_payload_capture_guard.tw
git commit -m "probes: scoped typed-payload positive probe + capture timing tripwire + order_by gate"
```

---

## Definition of done (Milestone A)

- `make bundle-cli` fixed point; `make boot-test` green.
- `typed_variant_payload_probe` (variant-only): `get_i64` present; correct checksum.
- **Capture tripwire**: elapsed < 200 ms (was ~8100 ms on the M1a branch).
- `order_by_breakdown` completes; `gather`/`take` improve; `sort` unchanged; no hang.
- Erased-Variant bridge round-trips typed payloads (Eq/Stringify over sums green).
- Three repr paths share one default; `repr_of_mono(Vector<Int>)` = `TypedRef`.

Deferred to **M1b**: typed closure environments (the captured-comparator sort win).
