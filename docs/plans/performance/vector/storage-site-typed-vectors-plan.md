# Milestone A — Storage-Site Typed Vectors — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the conservative typed-vector storage policy to typed sum/variant payloads (`IntCol(Vector<Int>)` → `PVecI64` in the variant struct) so variant-held columns get typed *direct* reads — without reintroducing the M1a per-read pathology.

**Architecture:** Mirror the proven S2.2 typed-record-field mechanism for variant payloads. A whole-program analysis marks `(TypeId, VariantId, payloadIdx)` sites typed; `wasm_layout` emits `PVecI64` for those payload slots; `emit_coerce_stack` places one `box_i64`/`unbox_i64` at each `PVecI64↔PVec` crossing. `repr_of_mono(Vector<Int>)` stays `TypedRef` (default); `PVecI64` is site-driven only. Escape analysis keeps captured/escaping vectors boxed, so no typed vector ever lands in the `anyref` closure env.

**Tech Stack:** Boot compiler (`boot/`). Verify: `make bundle-cli` (self-host to fixed point), `make boot-test`, and `.tw` probes built to `.wat` then grepped. Boot-only.

**Spec:** [storage-site-typed-vectors.md](storage-site-typed-vectors.md).

---

## Key facts (do not re-derive)

- **S2.2 record-field mechanism (the template):** `env.typed_vector_fields: Dict<String,Bool>` keyed `"${tid.id}:${fieldIdx}"`. `wasm_layout.tw:264` emits `PVecI64` when the key is present, else `val_type_of_mono`. Produced by `analyze_typed_fields` (`backend/route_typed_vec.tw:799`); threaded `PreparedModule.typed_vector_fields` (`prepare.tw:37,89`) → `env` (`codegen.tw:86`) → layout.
- **Sum layout:** `layout_of_sum_def` (`wasm_layout.tw:296`) builds each variant's `payload_types := collect f in v.fields { val_type_of_mono(subst_type_params(f, …)) }`. This is the site to override (mirrors the record-field override).
- **Three repr paths** in `backend/repr_assign.tw` all return `TypedRef` for vectors post-revert: `repr_of_mono` (:231), `repr_of_named_cached` (:260), `cached_repr_of_mono` (:315).
- **Candidate classifier:** `backend/repr_policy.tw` `elem_repr_of_vector(Vector<Int>) = Some(.I64)` (pure, no cycle).
- **Coercion** (`emit/coercions.tw`) already handles `PVecI64↔PVec` both ways (`box_i64`/`unbox_i64`) and boxes `Vector<Int>` on anyref erase.

## Note on granularity

Plumbing tasks (T1–T3, T6) carry complete code. The analysis extension (T4) is
specified as **exact target + concrete test gate** — it extends an ~800-line
producer/consumer/alias analysis, so its internal code is emergent and the *test*
(WAT grep + probe) is the contract. T5 (the M1a regression guard) is concrete.

## File structure

| File | Change | Task |
|------|--------|------|
| `boot/compiler/backend/repr_policy.tw` | rename `elem_repr_of_vector`→`candidate_typed_vec_family` | T1 |
| `boot/compiler/backend/repr_assign.tw` | route the 3 vector arms through one shared default helper | T1 |
| `boot/compiler/resolver.tw` | add `typed_vector_payloads` to `ResolvedEnv` | T2 |
| `boot/compiler/backend/prepare.tw` | add `typed_vector_payloads` to `PreparedModule`; thread it | T2 |
| `boot/compiler/codegen/codegen.tw` | thread `typed_vector_payloads` onto env | T2 |
| `boot/compiler/codegen/wasm_layout.tw` | `layout_of_sum_def` typed-payload override | T3 |
| `boot/compiler/backend/route_typed_vec.tw` | extend analysis to produce typed payload set | T4 |
| `examples/performance/sort-bench/*.tw`, `boot/tests/suites/*` | probes + guards | T5 |
| audit: `emit/variants.tw`, `emit/calls.tw`, constructor/match paths | consult typed-payload layout | T6 |

---

## Task 1: Classifier unification (candidate ≠ actual)

**Why:** Amendment 1 — `repr_of_mono` must not return `TypedVec` by default; the
classifier only names the *candidate* family. Make the three repr paths share one
default so they cannot diverge again (the activation exposed a divergence).

**Files:**
- Modify: `boot/compiler/backend/repr_policy.tw`
- Modify: `boot/compiler/backend/repr_assign.tw`

- [ ] **Step 1: Rename the classifier to say "candidate"**

In `repr_policy.tw`, rename `elem_repr_of_vector` → `candidate_typed_vec_family`
(same body/signature) and update its doc to "the candidate typed element family;
callers decide separately whether a *site* is typed." Update the one importer
(`repr_assign.tw`'s `use compiler.backend.repr_policy.{elem_repr_of_vector}` — if
present post-revert; if not imported, skip).

- [ ] **Step 2: Add one shared vector-default helper in repr_assign**

```tw
// backend/repr_assign.tw — single source of truth for the *default* vector repr.
// Vectors are TypedRef (boxed PVec ABI) by default; a PVecI64 physical repr comes
// only from site-aware sources (route_typed_vec slots, typed-storage layout),
// never from this default. Keeps the three repr paths from diverging.
fn vector_default_repr(mono: MonoType) ReprKind {
  .TypedRef(mono)
}
```

Replace the vector arms at `repr_of_mono` (:231), `repr_of_named_cached` (:260),
and `cached_repr_of_mono` (:315) so each calls `vector_default_repr(mono)`
(wrapping in the local `.{ repr: …, cache }` shape where the cached paths need it).

- [ ] **Step 3: Self-host + commit**

```bash
make bundle-cli && make boot-test
git add boot/compiler/backend/repr_policy.tw boot/compiler/backend/repr_assign.tw
git commit -m "backend: unify vector repr default; candidate family classifier (no behavior change)"
```
Expected: fixed point; boot suite green; no behavior change (all three still return `TypedRef`).

---

## Task 2: `typed_vector_payloads` plumbing (inert)

**Why:** carry the typed-payload decision from analysis to layout, mirroring
`typed_vector_fields`. Empty for now → inert.

**Files:**
- Modify: `boot/compiler/resolver.tw` (:129 area — `ResolvedEnv`)
- Modify: `boot/compiler/backend/prepare.tw` (:37 `PreparedModule`, :84/:89 returns)
- Modify: `boot/compiler/codegen/codegen.tw` (:86 area)

- [ ] **Step 1: Add the field to `ResolvedEnv`**

In `resolver.tw`, next to `typed_vector_fields: Dict<String, Bool>` (:129), add
`typed_vector_payloads: Dict<String, Bool>`, and in the constructor (:149) add
`typed_vector_payloads: Dict.new()`.

- [ ] **Step 2: Add it to `PreparedModule` and thread it**

In `prepare.tw`: add `typed_vector_payloads: Dict<String, Bool>` to
`PreparedModule` (:37); in both returns (:84 depth-guard early return and :89) add
`typed_vector_payloads: Dict.new()` for now (analysis wires it in T4).

- [ ] **Step 3: Thread onto env in codegen**

In `codegen.tw` next to `env2.typed_vector_fields = prepared.typed_vector_fields`
(:86), add `env2.typed_vector_payloads = prepared.typed_vector_payloads`.

- [ ] **Step 4: Self-host + commit**

```bash
make bundle-cli && make boot-test
git add boot/compiler/resolver.tw boot/compiler/backend/prepare.tw boot/compiler/codegen/codegen.tw
git commit -m "backend: add typed_vector_payloads plumbing (empty, inert)"
```
Expected: fixed point; green; inert (dict always empty).

---

## Task 3: Typed variant payload layout override

**Why:** the site where a typed payload becomes physically `PVecI64` — mirrors the
S2.2 record-field override.

**Files:**
- Modify: `boot/compiler/codegen/wasm_layout.tw` (`layout_of_sum_def`, ~:296–320)
- Test: `boot/tests/suites/wasm_layout_suite.tw`

- [ ] **Step 1: Failing test — a flagged variant payload becomes PVecI64**

Add a test mirroring `test_layout_record_vector_int_field_is_pveci64`: build an
env with a sum type (one variant, one `Vector<Int>` payload), set
`base.typed_vector_payloads["${tid}:0:0"] = true`, call `layout_of`, assert the
variant's `payload_types[0]` is `.Ref(_, .Named("rt_types__PVecI64"))`. Run
`target/twk run boot/tests/main.tw` → FAIL (still `PVec`).

- [ ] **Step 2: Implement the override in `layout_of_sum_def`**

In the `payload_types := collect f in v.fields { … }` loop, key each payload by
`"${tid.id}:${variantIdx}:${payloadIdx}"` and emit
`.Ref(true, .Named("rt_types__PVecI64"))` when
`env.typed_vector_payloads.has(key)`, else the existing
`val_type_of_mono(field_ty, env)`. (Mirror `wasm_layout.tw:264`.)

- [ ] **Step 3: Test passes; self-host; commit**

```bash
target/twk run boot/tests/main.tw   # new test PASS
make bundle-cli && make boot-test
git add boot/compiler/codegen/wasm_layout.tw boot/tests/suites/wasm_layout_suite.tw
git commit -m "codegen: typed Vector<Int> variant payloads in layout_of_sum_def"
```

---

## Task 4: Extend the analysis to variant payloads

**Why:** decide *which* `(TypeId, VariantId, payloadIdx)` sites are typed, under
the amended eligibility (producers write typed-routable values; consumers are
typed-compatible or one-time boxed egress). Populates `typed_vector_payloads`.

**Files:**
- Modify: `boot/compiler/backend/route_typed_vec.tw` (extend `analyze_typed_fields`
  or add `analyze_typed_payloads`)
- Modify: `boot/compiler/backend/prepare.tw` (:89 — set `typed_vector_payloads` from analysis)

- [ ] **Step 1: Producers = typed vectors into variant construction**

In the producer scan (`scan_func_producers_consumers` / `collect_candidates`),
recognize a typed `v`-group whose sole escape is a **variant construction** with
`v` in payload slot `(tid, variantId, payloadIdx)` — the variant analogue of a
single typed-field store. Mark `has_ok_producer` for that payload key. A `v` that
also escapes elsewhere disqualifies the key (`bad_producer`).

- [ ] **Step 2: Consumers = match-arm payload reads**

Recognize `case … .Variant(x) => …` payload bindings: a consumer that reads `x`
only via typed index/`len`, or transfers it into another typed site, is
typed-compatible; a consumer that lets `x` **escape to `anyref`/closure** is a
one-time boxed egress (allowed — do NOT mark `bad_consumer`; the coercion boxes
once). Only `typed payload → boxed durable → repeated typed read-back` is
forbidden, which escape analysis on the extracted local already prevents by
boxing it. Mark `bad_consumer` only for genuinely representation-incompatible uses
(e.g. a store back into a boxed-`PVec` payload of the same site).

- [ ] **Step 3: Emit the payload key set + wire into prepare**

Have the analysis also return a `Dict<String,Bool>` of payload keys
(`"${tid}:${variantId}:${payloadIdx}"`). In `prepare.tw:89`, set
`typed_vector_payloads:` from it (replacing the T2 `Dict.new()`).

- [ ] **Step 4: Gate — the positive probe routes typed**

```bash
target/twk build examples/performance/sort-bench/typed_variant_column_probe.tw -o /tmp/tvc.wat
grep -c rt_arr__get_i64 /tmp/tvc.wat     # expect >= 1 (was 0)
make bundle-cli && make boot-test        # fixed point + green
```

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/backend/route_typed_vec.tw boot/compiler/backend/prepare.tw
git commit -m "backend: type Vector<Int> variant payloads via extended storage-site analysis"
```

---

## Task 5: The M1a regression guard + workload checks

**Why:** the guard test is the deliverable — it makes the pathology fail loudly.

**Files:**
- Create: `examples/performance/sort-bench/typed_payload_capture_guard.tw`
- Use: `typed_variant_column_probe.tw`, `dataframe/bench/order_by_breakdown.tw`, `/tmp/cap.tw`-style

- [ ] **Step 1: Author the capture guard probe**

A program that stores a `Vector<Int>` in a variant, extracts it, and **captures it
into a closure** read in a loop (the `sort_by` shape). Build to WAT.

- [ ] **Step 2: Assert zero per-call unbox on the read path**

```bash
target/twk build examples/performance/sort-bench/typed_payload_capture_guard.tw -o /tmp/g.wat
# The captured column must be boxed (escape → PVec), so the closure/trampoline
# body must contain NO rt_arr__unbox_i64 on the read path:
grep -c rt_arr__unbox_i64 /tmp/g.wat    # must be 0 inside the closure read path
```
Also run it: the loop must complete in low-ms (not seconds).

- [ ] **Step 3: order_by — improvement without regression**

```bash
timeout 60 target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw
```
Expected: `gather`/`take`/direct reads improve vs the pre-A baseline; `sort`
unchanged (boxed comparator — M1b); **completes** (no hang). The `timeout` is the
loud-failure tripwire.

- [ ] **Step 4: Commit**

```bash
git add examples/performance/sort-bench/typed_payload_capture_guard.tw
git commit -m "probes: typed-payload capture guard (zero per-call unbox) + order_by gate"
```

---

## Task 6: Audit all payload paths for typed layout (the caution)

**Why:** any constructor/extractor/helper that rebuilds payload valtypes from bare
`val_type_of_mono(payload_mono)` silently falls back to `PVec` and loses typing.

**Files:**
- Audit/modify: `boot/compiler/codegen/emit/variants.tw`, `emit/calls.tw`, and any
  path that computes a variant payload wasm type.

- [ ] **Step 1: Find payload-valtype reconstruction sites**

```bash
grep -rn "val_type_of_mono\|payload_types\|StructNew\|StructGet" boot/compiler/codegen/emit/variants.tw | head -40
```
For each site that computes a variant payload wasm type, confirm it derives from
the **layout** (`layout_of_sum_def` result, which now honors `typed_vector_payloads`)
rather than recomputing from `val_type_of_mono(payload_mono)`. Route any that
recompute through the layout.

- [ ] **Step 2: Gate — end-to-end typed variant construction + read**

```bash
target/twk run examples/performance/sort-bench/typed_variant_column_probe.tw  # match=true, typed reads
make bundle-cli && make boot-test
```

- [ ] **Step 3: Commit**

```bash
git add boot/compiler/codegen/emit/variants.tw boot/compiler/codegen/emit/calls.tw
git commit -m "codegen: variant payload paths consult typed-site layout (no PVec fallback)"
```

---

## Definition of done (Milestone A)

- `make bundle-cli` fixed point; `make boot-test` green.
- `typed_variant_column_probe`: `get_i64` present; `match=true`.
- **Capture guard**: zero per-call `unbox_i64` on the read path; loop completes low-ms.
- `order_by_breakdown` completes; `gather`/`take` improve; `sort` unchanged; no hang.
- The three repr paths share one default; `repr_of_mono(Vector<Int>)` = `TypedRef`.

Deferred to **M1b**: typed closure environments (the captured-comparator sort win).
