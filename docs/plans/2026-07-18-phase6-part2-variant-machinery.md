# Phase 6 Part 2 — Variant/Decision Machinery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the parameter-side ownership facts from Part 1 into per-call-site ownership-specialization *decisions* — the analysis-only `VariantId`/`SpecializationFacts` the design's Phase 6 produces — starting with the variant-identity & encoding substrate every later stage builds on.

**Architecture:** Part 2 is the deep, multi-subsystem core of Phase 6 (design: `docs/plans/sound-uniqueness/analysis/phase6-design.md`). It decomposes into six stages (identity → field-granular paths → owned-entry re-analysis → call-site decision → SCC variant fixpoint → rendering), each independently shippable and gated behind the previous. This plan makes **Stage 1 (variant identity & encoding)** fully concrete and executable, and scopes **Stages 2–6** as a roadmap — each becomes its own detailed plan when reached, because their exact TDD code depends on forward-analysis internals (`ForwardState` threading, `Unique`-entry seeding, `run_fixpoint`/`run_scc` surgery) and Stage 1's finalized APIs. This mirrors Part 1's "concrete-first, scope-the-rest" split.

**Tech Stack:** Twinkle (`.tw`), boot compiler only. Tests via the boot suite. Analysis-only: `twk ir --census` must stay **0 in-place** through all of Part 2. Build/verify with `make boot-test`.

---

## Where Part 2 sits

Part 1 (landed, commits `fba33602`/`e1492fcb`) reconciled `ParamSummary` to `{ base_role: ParamRole, in_place_paths: Vector<ParamPath>, flows_to_return: Bool }` and populates `in_place_paths` at **shell `[]` granularity only**. Part 2 supplies the rest of the design's Phase 6: field-granular paths, the per-call-site variant decision, the owned-entry re-analysis that proves it, and the SCC variant fixpoint — all still **analysis-only** (no cloned variants emitted; that is codegen Phase 2A).

**Stage map (Stage 1 is the concrete execution plan; Stages 2–6 are the roadmap at the end):**

| Stage | Subsystem | Design refs | Acceptance criteria |
|---|---|---|---|
| **1** | **Variant identity & encoding** (this plan) | D3, D8, D14; "Data model"; Int-key encoding | #14 (determinism substrate) |
| 2 | Field-granular `in_place_paths` + `ConsumedPaths` | Blocker 1/2/3, D6 | #1 (full), #5, #7, #8 |
| 3 | Owned-entry re-analysis (`summarize_variant`) | D10, D11, D13 | #4, #6 (variant side) |
| 4 | Call-site decision + `consume_dead` | D4, D6, D7 | #2, #3, #5, #7, #8 |
| 5 | SCC variant fixpoint + cap | D7, D12 | #11, #12, #13 |
| 6 | Rendering (cfg decisions) | "Rendering"; D14 | #14 (rendered), #3 verdicts |

---

## Stage 1: Variant identity & encoding infrastructure

**Current progress:** Tasks 1–2 are landed (`variant_id` identity types plus canonicalization/downward-closure). Task 3 (deterministic interner + `site_key`) and Task 4 (Stage 1 verification) remain Phase 6 work.

**What it delivers:** a self-contained leaf module owning the variant-identity types and their canonicalization + deterministic interning. No consumer wires it yet (Stages 3–5 do), but it is fully testable in isolation and it resolves the **load-bearing determinism/encoding question** the design flags (`variant_key`/`site_key` must be pure functions of canonical inputs, `VariantId` numbering stable across builds — acceptance #14). Every later stage keys its memo and decision tables on this module.

**Design decisions realized here:**
- **D3 (downward-closed under the shell):** a `(k, [f])` requirement implies `(k, [])`; canonicalization enforces it.
- **D14 (determinism):** `UniqueKey` is canonical-sorted `(param, path)`, equal keys dedup, `VariantId`s are numbered in **creation order** via a deterministic interner.
- **D8 (representation-neutral):** the identity is an abstract `VariantId`; nothing here commits to clone-vs-annotation.
- **Int-key encoding (design "Data model" patch):** rather than a collision-prone hashed Int, a **canonical-string interner** maps each canonicalized `VariantId` to a dense creation-order Int — giving both the `Dict<Int, …>` memo key and D14's stable numbering in one structure. `site_key(func, local)` is a separate reversible pairing.

### File structure

- **Create `boot/compiler/variant_id.tw`** (leaf module, no compiler imports): owns `ParamPath` (moved from `ownership.tw`), `UniqueReq`, `UniqueKey`, `VariantId`, canonicalization (`canonicalize_key`, `downward_close`, comparators), the canonical-string codec (`variant_canonical_string`), the interner (`VariantInterner`, `intern`, `variant_of_id`), and `site_key`. Dependency direction: `ownership.tw` will `use compiler.variant_id` (Stages 3–5), so this module must NOT import `ownership.tw` — hence `ParamPath` lives here, not there.
- **Modify `boot/compiler/ownership.tw`:** remove the Part-1 `ParamPath` definition and import it from `variant_id` instead (one type relocation; `ParamSummary` and the `ipp` logic are unchanged in behavior).
- **Test `boot/tests/suites/cfg_summary_suite.tw`:** add a `use compiler.variant_id` and Stage-1 unit tests (canonicalization, downward-closure, interner determinism).

### Task 1: Create the `variant_id` leaf module with types + relocate `ParamPath`

**Files:**
- Create: `boot/compiler/variant_id.tw`
- Modify: `boot/compiler/ownership.tw` (remove `ParamPath` def ~line 44, add import)

- [x] **Step 1: Write a failing test that imports the new module**

In `boot/tests/suites/cfg_summary_suite.tw`, add `use compiler.variant_id` to the imports at the top, and add this test to `suite()`:

```tw
    .test(
      "variant_id: VariantId and UniqueReq construct",
      fn() {
        v := variant_id.VariantId.{ func: 7, unique: [variant_id.UniqueReq.{ param: 0, path: [] }] }
        try assert.equal(v.func, 7)
        try assert.equal(v.unique.len(), 1)
        try assert.equal(v.unique[0].param, 0)
        try assert.equal(v.unique[0].path.len(), 0)
        .Ok({})
      },
    )
```

- [x] **Step 2: Run to verify it fails (module does not exist)**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -20`
Expected: an unresolved-import / unknown-module error for `compiler.variant_id`.

- [x] **Step 3: Create `boot/compiler/variant_id.tw` with the identity types**

```tw
//! Variant identity for Phase 6 ownership specialization (analysis-only).
//!
//! A `VariantId` names an ownership-specialized instance of a function: the
//! function plus the set of `(param, field-only path)` requirements that must be
//! proven Unique at a call site to select it. This module is a LEAF (no compiler
//! imports) so `ownership.tw`/`summary.tw` may depend on it without a cycle.

// [] = the shell / whole collection; [f] = a direct record field in executable
// Phase 6. The Vector shape reserves future field chains, but Stage 2 does not
// create requirements for unsupported deeper mutations. FIELD-ONLY: payload / Elem /
// Val segments are return-path/read facts, never a UniqueReq path (design Blocker 1). Relocated here from
// ownership.tw so this module stays a leaf.
pub type ParamPath = Vector<Int>

pub type UniqueReq = .{ param: Int, path: ParamPath }
pub type UniqueKey = Vector<UniqueReq>            // canonical-sorted; [] => generic variant
pub type VariantId = .{ func: Int, unique: UniqueKey }
```

- [x] **Step 4: Import `ParamPath` into `ownership.tw` and remove its local definition**

In `boot/compiler/ownership.tw`, delete the Part-1 `ParamPath` definition (the `pub type ParamPath = Vector<Int>` line and its comment, near line 44) and add an import near the other `use` lines at the top of the file:

```tw
use compiler.variant_id.{ParamPath}
```

Everything in `ownership.tw` that referenced `ParamPath` (the `ParamSummary.in_place_paths` field, the `ipp` case) now resolves to the imported type — no other change.

- [x] **Step 5: Run the boot suite to verify green**

Run: `make boot-test 2>&1 | grep -E 'Ran [0-9]+ tests|error|Error|FAIL' | tail -5`
Expected: `Ran N tests: N passed` (the new import-smoke test passes; Part 1 tests still pass — the `ParamPath` relocation is behavior-preserving).

- [x] **Step 6: Commit**

```bash
target/twk fmt boot/compiler/variant_id.tw boot/compiler/ownership.tw
git add boot/compiler/variant_id.tw boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "phase6: add variant_id leaf module with VariantId identity types

Introduce the Phase 6 ownership-specialization identity (UniqueReq/UniqueKey/
VariantId) in a new leaf module and relocate ParamPath there so ownership.tw
can depend on it without a cycle. Types only; canonicalization + interner follow."
```

### Task 2: Canonicalization — sort, dedup, downward-closure (D3, D14)

**Files:**
- Modify: `boot/compiler/variant_id.tw`
- Test: `boot/tests/suites/cfg_summary_suite.tw`

- [x] **Step 1: Write the failing tests**

Add to `suite()`:

```tw
    .test(
      "variant_id: canonicalize sorts by (param, path) and dedups",
      fn() {
        // unsorted + duplicate reqs
        raw: variant_id.UniqueKey = [
          variant_id.UniqueReq.{ param: 1, path: [] },
          variant_id.UniqueReq.{ param: 0, path: [3] },
          variant_id.UniqueReq.{ param: 0, path: [] },
          variant_id.UniqueReq.{ param: 0, path: [3] }, // dup
        ]
        got := variant_id.canonicalize_key(raw)
        // expected order: (0,[]), (0,[3]), (1,[])   — dup collapsed
        try assert.equal(got.len(), 3)
        try assert.equal(got[0].param, 0)
        try assert.equal(got[0].path.len(), 0)
        try assert.equal(got[1].param, 0)
        try assert.equal(got[1].path[0], 3)
        try assert.equal(got[2].param, 1)
        .Ok({})
      },
    )
    .test(
      "variant_id: downward_close adds the shell for every field req (D3)",
      fn() {
        // (0,[3]) with no (0,[]) present must gain (0,[])
        raw: variant_id.UniqueKey = [variant_id.UniqueReq.{ param: 0, path: [3] }]
        got := variant_id.downward_close(raw)
        try assert.equal(got.len(), 2)
        try assert.equal(got[0].param, 0)
        try assert.equal(got[0].path.len(), 0) // shell added, sorts first
        try assert.equal(got[1].path[0], 3)
        .Ok({})
      },
    )
```

- [x] **Step 2: Run to verify failure**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -iE 'canonicalize_key|downward_close|error' | tail -5`
Expected: unresolved-name errors for `canonicalize_key` / `downward_close`.

- [x] **Step 3: Implement the comparators, canonicalization, and downward-closure**

Add to `boot/compiler/variant_id.tw`:

```tw
// Lexicographic order on field-only paths: a prefix is smaller, so the shell []
// sorts before [f]. Delegates to the prelude's tested Vector.compare<T: Ord>.
pub fn path_cmp(a: ParamPath, b: ParamPath) Order {
  a.compare(b)
}

pub fn req_cmp(a: UniqueReq, b: UniqueReq) Order {
  c := a.param.compare(b.param)
  case c {
    .Eq => path_cmp(a.path, b.path),
    _ => c,
  }
}

fn req_eq(a: UniqueReq, b: UniqueReq) Bool {
  case req_cmp(a, b) {
    .Eq => true,
    _ => false,
  }
}

// Sort by (param, path) then drop adjacent duplicates. Deterministic: the sort is
// total (req_cmp never returns .Eq for distinct reqs) so ordering is build-stable.
pub fn canonicalize_key(reqs: UniqueKey) UniqueKey {
  sorted := reqs.sort_by(req_cmp)
  out: UniqueKey = []
  for r in sorted {
    keep := if out.len() == 0 {
      true
    } else {
      !req_eq(out[out.len() - 1], r)
    }
    if keep {
      out = .append(r)
    }
  }
  out
}

// D3: a (k,[f]) requirement implies (k,[]). Ensure every param that appears also
// has its shell req, then canonicalize. (Part cap keeps paths at depth <= 1, so the
// only prefix to add is the shell.)
pub fn downward_close(reqs: UniqueKey) UniqueKey {
  augmented := reqs
  seen_shell: Dict<Int, Bool> = Dict.new()
  for r in reqs {
    if r.path.len() == 0 {
      seen_shell[r.param] = true
    }
  }
  for r in reqs {
    if r.path.len() > 0 {
      case seen_shell.get(r.param) {
        .Some(_) => {},
        .None => {
          augmented = .append(UniqueReq.{ param: r.param, path: [] })
          seen_shell[r.param] = true
        },
      }
    }
  }
  canonicalize_key(augmented)
}
```

> **API confirmed:** `Vector.sort_by<T>(xs, cmp: fn(T,T) Order)` exists (`boot/prelude/vector.tw:344`) and `Int.compare(a,b) Order` exists (`boot/prelude/int.tw:3`); the inherent-method call `reqs.sort_by(req_cmp)` and `a[i].compare(b[i])` are exactly the in-repo idiom (`summary.tw:191` sorts `ret_paths` the same way). No adaptation needed.

- [x] **Step 4: Run to verify the two tests pass**

Run: `make boot-test 2>&1 | grep -E 'Ran [0-9]+ tests|FAIL' | tail -3`
Expected: `Ran N tests: N passed`.

- [x] **Step 5: Commit**

```bash
target/twk fmt boot/compiler/variant_id.tw
git add boot/compiler/variant_id.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "phase6: canonicalize + downward-close UniqueKey (D3/D14)

Sort UniqueKey by (param, path), drop duplicates, and close downward under the
shell so (k,[f]) implies (k,[]). Deterministic total order for build-stable keys."
```

### Task 3: Deterministic interner — canonical string → creation-order Int (D14, Int-key encoding)

**Files:**
- Modify: `boot/compiler/variant_id.tw`
- Test: `boot/tests/suites/cfg_summary_suite.tw`

- [ ] **Step 1: Write the failing tests**

Add to `suite()`:

```tw
    .test(
      "variant_id: intern gives one id per canonical key, in creation order",
      fn() {
        vi := variant_id.new_interner()
        va := variant_id.VariantId.{ func: 7, unique: [variant_id.UniqueReq.{ param: 0, path: [] }] }
        vb := variant_id.VariantId.{ func: 7, unique: [variant_id.UniqueReq.{ param: 1, path: [] }] }
        // va2: va's key written with a redundant DUPLICATE shell req; canonicalize_key
        // collapses it to va's canonical key, so it must intern to va's id (dedup path).
        va2 := variant_id.VariantId.{ func: 7, unique: [
          variant_id.UniqueReq.{ param: 0, path: [] },
          variant_id.UniqueReq.{ param: 0, path: [] },
        ] }

        r1 := variant_id.intern(vi, va)
        r2 := variant_id.intern(r1.interner, vb)
        r3 := variant_id.intern(r2.interner, va2) // canonically equal to va

        try assert.equal(r1.id, 0)        // creation order
        try assert.equal(r2.id, 1)
        try assert.equal(r3.id, 0)        // deduped back to va's id
        .Ok({})
      },
    )
    .test(
      "variant_id: intern is order-insensitive within a key and round-trips by id",
      fn() {
        vi := variant_id.new_interner()
        // same key, reqs supplied in two different orders
        k1: variant_id.UniqueKey = [
          variant_id.UniqueReq.{ param: 0, path: [3] },
          variant_id.UniqueReq.{ param: 0, path: [] },
        ]
        k2: variant_id.UniqueKey = [
          variant_id.UniqueReq.{ param: 0, path: [] },
          variant_id.UniqueReq.{ param: 0, path: [3] },
        ]
        r1 := variant_id.intern(vi, variant_id.VariantId.{ func: 9, unique: k1 })
        r2 := variant_id.intern(r1.interner, variant_id.VariantId.{ func: 9, unique: k2 })
        try assert.equal(r1.id, r2.id) // canonicalization makes them one variant

        got := variant_id.variant_of_id(r2.interner, r1.id)
        case got {
          .Some(v) => {
            try assert.equal(v.func, 9)
            try assert.equal(v.unique.len(), 2)
            try assert.equal(v.unique[0].path.len(), 0) // canonical: shell first
          },
          .None => return .Err("expected a variant for id"),
        }
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify failure**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -iE 'new_interner|intern|variant_of_id|error' | tail -5`
Expected: unresolved-name errors.

- [ ] **Step 3: Implement the canonical-string codec and the interner**

Add to `boot/compiler/variant_id.tw`:

```tw
// Injective canonical serialization of a VariantId. Requires the key already
// canonicalized (sorted, deduped, downward-closed). Delimiters ':' ';' '|' '.' are
// safe because every component is a non-negative Int. Two VariantIds serialize
// equal iff they are the same variant — the basis for dedup + build-stable ids.
pub fn variant_canonical_string(v: VariantId) String {
  parts: Vector<String> = []
  for r in v.unique {
    segs: Vector<String> = collect s in r.path {
      "${s}"
    }
    parts = .append("${r.param}:${segs.join(".")}")
  }
  "f${v.func}|${parts.join(";")}"
}

// A VariantId with its key canonicalized + downward-closed (D3).
pub fn canonicalize_variant(v: VariantId) VariantId {
  VariantId.{ func: v.func, unique: downward_close(v.unique) }
}

pub type VariantInterner = .{
  ids: Dict<String, Int>,     // canonical string -> dense creation-order id
  by_id: Vector<VariantId>,   // id -> canonicalized VariantId (creation order)
}

pub type InternResult = .{ interner: VariantInterner, id: Int }

pub fn new_interner() VariantInterner {
  VariantInterner.{ ids: Dict.new(), by_id: [] }
}

// Intern a VariantId: canonicalize it, then return its dense id — reusing the
// existing id for an equal canonical key, or assigning the next creation-order id.
// The returned interner carries any newly-assigned id (threaded, not mutated).
pub fn intern(vi: VariantInterner, v: VariantId) InternResult {
  cv := canonicalize_variant(v)
  s := variant_canonical_string(cv)
  case vi.ids.get(s) {
    .Some(id) => InternResult.{ interner: vi, id },
    .None => {
      id := vi.by_id.len()
      vi.ids[s] = id
      vi.by_id = vi.by_id.append(cv)
      InternResult.{ interner: vi, id }
    },
  }
}

pub fn variant_of_id(vi: VariantInterner, id: Int) VariantId? {
  if id >= 0 and id < vi.by_id.len() {
    .Some(vi.by_id[id])
  } else {
    .None
  }
}

// Injective pairing of a (func_id, local_id) pair into one Int for the
// CallDecision table key (both ids are dense, non-negative, per-mono-instance
// stable). Szudzik-style. The CallDecision record also stores site_func/site_local
// as fields (design "Data model"), so this is a lookup key only — no un-pairing is
// needed in Part 2; add one only if a future consumer must recover (func, local).
pub fn site_key(func_id: Int, local_id: Int) Int {
  if func_id >= local_id {
    func_id * func_id + func_id + local_id
  } else {
    local_id * local_id + func_id
  }
}
```

> **Note:** `intern` returns an `InternResult` record (Twinkle has no anonymous multi-value return) so callers thread the interner: `r := intern(vi, v); vi = r.interner; use r.id`. This threading is exactly how Stages 4–5 accumulate variants during the SCC walk.

- [ ] **Step 4: Run to verify the interner tests pass**

Run: `make boot-test 2>&1 | grep -E 'Ran [0-9]+ tests|FAIL' | tail -3`
Expected: `Ran N tests: N passed`.

- [ ] **Step 5: Add a determinism guard test (build-stability of the string codec)**

```tw
    .test(
      "variant_id: canonical string is stable and injective for distinct keys",
      fn() {
        va := variant_id.VariantId.{ func: 7, unique: [
          variant_id.UniqueReq.{ param: 0, path: [] },
          variant_id.UniqueReq.{ param: 0, path: [3] },
        ] }
        vb := variant_id.VariantId.{ func: 7, unique: [
          variant_id.UniqueReq.{ param: 0, path: [] },
          variant_id.UniqueReq.{ param: 0, path: [4] },
        ] }
        sa := variant_id.variant_canonical_string(variant_id.canonicalize_variant(va))
        sb := variant_id.variant_canonical_string(variant_id.canonicalize_variant(vb))
        try assert.equal(sa, "f7|0:;0:3")     // exact, human-checkable form
        try assert.is_true(sa != sb)          // distinct keys => distinct strings
        .Ok({})
      },
    )
```

- [ ] **Step 6: Run + commit**

Run: `make boot-test 2>&1 | grep -E 'Ran [0-9]+ tests|FAIL' | tail -3`  → `Ran N tests: N passed`.

```bash
target/twk fmt boot/compiler/variant_id.tw
git add boot/compiler/variant_id.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "phase6: deterministic VariantId interner + site_key encoding (D14)

Canonical-string codec + a creation-order interner give a build-stable Int memo
key per VariantId (dedup on canonical key), and site_key pairs (func,local) for
the CallDecision table. Resolves the Int-key encoding the design flagged."
```

### Task 4: Stage 1 verification

**Files:** none (verification only)

- [ ] **Step 1: Census unchanged (analysis-only invariant holds)**

Run: `target/twk ir boot/main.tw --census 2>&1 | awk 'NR>1 && NF>=3 {s+=$NF} END{print "total in_place:", s+0}'`
Expected: `total in_place: 0`.

- [ ] **Step 2: Full boot suite + self-host fixed point**

Run: `make boot-test 2>&1 | grep -E 'Ran [0-9]+ tests|Fixed point|FAIL' | tail -4`
Expected: `Fixed point reached: stage3 == stage4` and `Ran N tests: N passed`. (Run alone; no parallel heavy `twk`.)

- [ ] **Step 3: Determinism smoke — the module compiles into two identical builds**

Run: `target/twk build boot/main.tw -o /tmp/p6s1a.wasm && target/twk build boot/main.tw -o /tmp/p6s1b.wasm && cmp /tmp/p6s1a.wasm /tmp/p6s1b.wasm && echo IDENTICAL`
Expected: `IDENTICAL` (byte-identical builds — the new module introduces no build nondeterminism).

---

## Roadmap: Stages 2–6 (each gets its own detailed plan)

These stages are **scoped, not coded** here — their exact TDD steps depend on forward-analysis internals and Stage 1's APIs, and must be written with those in hand (fabricating them now would violate the no-placeholders rule). Each is independently shippable, gated behind the previous, and keeps `twk ir --census` at 0 in-place. When you reach a stage, write its plan via the writing-plans skill using the entry points below.

### Stage 2 — Field-granular `in_place_paths` + `ConsumedPaths` (Blocker 1/2/3, D6)

**Scope:** extend Part 1's shell-only `in_place_paths` to direct field paths
(`add_type` → `paths{[],[.types]}`), add per-local read-validity tracking so a
consumed `.types` can coexist with a live `.values` read, and make alias
completeness an explicit precondition for any collection-backing consume.

**Executable path cap:** Phase 6 parameter requirements are only `[]` and direct
record fields `[f]`. `ParamPath = Vector<Int>` leaves room for future field chains,
but Stage 2 does not create a requirement for an unsupported deeper mutation. It may
only record a direct ancestor when that ancestor is itself an independently supported
mutation site. `Elem`/`Val`/`Payload` never enter `UniqueReq` keys.

**Entry points:**
- Requirement collection: at each `ARecordUpdate(base, f, v, …)` (`ownership.tw:1605`) and each consuming call (`consume_call_base`, `ownership.tw:902`; `cow_base_arg`, `:895`), map `prov_of(st.prov, base)` → a single param `k`; when it resolves, add field path `[f.id]` (downward-closed to include `[]`) to param `k`'s candidate set. This replaces Part 1's coarse `role == Consumed => [[]]` with a per-op-collected set. Convert `ff.AccessPath` → direct-field `ParamPath` here, **rejecting** `Elem`/`Val`/`Payload` segments and rejecting unsupported deeper paths unless a supported direct ancestor is independently mutated.
- `ConsumedPaths`: add a 6th field to `ForwardState` (`ownership.tw:729`), `ConsumedPaths = Dict<Int, Vector<ParamPath>>` (design "Data model for partial validity"), with the three D6 read rules (whole-value use illegal; consumed-path read illegal; disjoint sibling read legal).
- `ConsumedPaths` transfer/merge: selecting an owned variant adds consumed paths for the argument local; rebinding that local to the post-call result clears the old consumed set; legal carrier moves preserve the consumed set, while illegal whole/carried uses force the call site back to generic; joins and loop back-edges merge by **union** per carried local; all-edge rebinding follows the incoming value's consumed set so normal rebind flow clears stale consumed paths.
- Alias-completeness gate: reuse `own`/`prov`/`field_own`/`path_prov` rather than adding a new points-to analysis. A selected shell/collection path needs `Unique` plus precise provenance; a selected field path needs both `field_own` and matching `path_prov`. Missing, multi-origin, or untracked alias facts drop the selected path. Collection `[]` and collection-valued `[f]` require this because pre-captured aliases observe backing mutation; record-shell `[]` keeps D6's disjoint-sibling allowance.
- Requirement collection participates in the SCC fixpoint (a member's candidate set grows when an in-SCC callee gains an `in_place_path`); compare via the Part-1 `same_param_paths` already in `same_summary`.

**Acceptance:** #1 (full, `paths{[],[.types]}`), #5 (mixed-ownership record), #7 (path-aware gate), #8 (collection alias completeness). **Depends on:** Stage 1 (`ParamPath` home).

### Stage 3 — Owned-entry re-analysis `summarize_variant` (D10, D11, D13)

**Scope:** re-run the forward transfer over a callee body with keyed `(param, path)` slots seeded `Unique` at entry (instead of `Unknown`), producing the specialized `Summary` for a `VariantId`. This is what turns candidate `in_place_paths` into accepted in-place facts and turns `OwnedFromParam(k)` into a real unique hand-off — closing the Phase 5 param-threaded gate **without changing the gate** (`ownership.tw:1174` fires unchanged because the param now enters `Unique`).

**Entry points:**
- `summarize_function` (`ownership.tw:3255`) seeds entry `own`/`path_prov` — currently every reference param enters via `join_entry_*` as `Unknown`. Add a `summarize_variant(f, key, …)` that seeds the keyed slots `Unique` + `path_prov` naming param `k` (mirror `seed_param_prov`, `ownership.tw:2308`), reusing the SAME transfer (D13 — no second proof engine). Memoize per `VariantId` (interned id from Stage 1).

**Acceptance:** #4 (Result-payload param scrutinee), #6 (param-threaded transport, per-variant). **Depends on:** Stages 1–2.

### Stage 4 — Call-site decision + `consume_dead` (D4, D6, D7)

**Scope:** at each user call, form `candidate_key` from pre-call per-path facts, reduce to `selected_key` by the key-level `consume_dead` fixed point (drop violating paths, re-close downward, repeat), select `VariantId` (or generic if empty/over-cap), apply the specialized return-path facts, and partially-invalidate the consumed paths of the arg. This is the executable check for the Phase 6 theorem: the pre-update logical version must have no observable continuation except producing the post-update value.

**Entry points:**
- `transfer_summarized_call` (`ownership.tw:1112`) already snapshots pre-call arg facts (`arg_unique`, `:1125`) and has the recovery gate (`:1174`). Add the key-selection + `CallDecision` emission alongside it, reading `ConsumedPaths` (Stage 2) for the D6 liveness rules and the Stage-2 alias-completeness predicate before accepting any selected path. Emit `CallDecision` keyed by `site_key` (Stage 1). The generic path stays exactly today's behavior.
- Fallback reasons are part of the decision: not unique, consumed path observed later, whole carrier used later, alias set incomplete, over cap, or no non-empty key after downward closure.

**Acceptance:** #2 (Cases B∩C), #3 (one VariantId two sites vs generic), #5, #7, #8. **Depends on:** Stages 1–3.

### Stage 5 — SCC variant fixpoint + cap (D7, D12)

**Scope:** demand-driven variants processed in the existing callee-first SCC order, with the variant memo iterated to a fixpoint. Two disciplines: in-place capability **ascends** (bottom = generic; a within-SCC recursive call reads the previous iteration's approximant), `ret_paths` ride the existing Phase 5 `suppress` (read empty in-SCC, published at the fixed point). Per-`(mono-instance, func)` variant-count cap (default 4); over-budget new keys route that call site to generic (no cell stripping).

**Entry points:**
- `run_scc` (`summary.tw:380`) and `compute` (`summary.tw:508`): thread a `VariantInterner` + `Dict<Int, Summary>` variant memo through the driver; return `SpecializationFacts` (`{ variants, decisions }`) instead of only `SummaryTable`. Reuse the existing worklist/`same_summary` machinery (D2) — add a variant axis, not a new driver. Termination by finite lattice height (the cap is a separate count limit, review #2).

**Acceptance:** #11 (recursive convergence, Case V), #12 (late cross-member demand), #13 (cap + fallback). **Depends on:** Stages 1–4.

### Stage 6 — Rendering the decisions (design "Rendering", D14)

**Scope:** print per-function preconditions/postconditions (`p0=Consumed paths{[],[.types]} → []=OwnedFromParam(0)`) and per call site the selected variant + licensing proof (`call build_env#L10 → add_type[unique:0,.types] (…)` / `→ add_type[generic] (…)`), in `twk ir --cfg`. Never a silent generic fallback — every fallback prints its reason.

**Entry points:**
- `render_summary` (`summary.tw:467`, already prints `in_place_paths` from Part 1) gains the postcondition arrow; `render_cfg` (`summary.tw:496`) threads the `CallDecision` table to `cfg.tw`'s view renderer **as data** (`variant` id + `proof` string), preserving cfg.tw's no-`ownership`-import rule.

**Acceptance:** #14 (rendered determinism, byte-identical `--cfg` across builds). **Depends on:** Stages 1–5.

---

## Self-Review (Stage 1)

- **Spec coverage (Stage 1 scope):** the identity types (D8), canonicalization + downward-closure (D3), determinism via canonical-string interner with creation-order ids (D14), and `site_key` encoding are each covered by a task. Tasks 1–2 are already checked off; Tasks 3–4 remain. Stages 2–6 map the remaining acceptance criteria (#1–#13, plus rendered #14) to scoped roadmap entries with entry-point anchors — no design bullet is unassigned.
- **Placeholder scan:** every Stage-1 step shows exact code or an exact command + expected output. The two `> Note` callouts flag real API-shape checks (the `Vector` sort signature; the `InternResult` threading idiom) rather than deferring content — the surrounding code is complete.
- **Type consistency:** `ParamPath`, `UniqueReq`, `UniqueKey`, `VariantId`, `VariantInterner`, `InternResult`, and the functions `path_cmp`/`req_cmp`/`canonicalize_key`/`downward_close`/`variant_canonical_string`/`canonicalize_variant`/`new_interner`/`intern`/`variant_of_id`/`site_key` are used with identical signatures across Tasks 1–3 and referenced consistently by the Stage 2–6 roadmap.
- **API shapes verified:** `Vector.sort_by<T>(xs, cmp: fn(T,T) Order)` (`vector.tw:344`), `Int.compare → Order` (`int.tw:3`), and `Vector.join`/`String` interpolation (used in Part 1) all exist and are used per the in-repo idiom (`summary.tw:191`). `intern` returns a named `InternResult` because Twinkle has no anonymous multi-value return; callers thread `vi = r.interner`.
