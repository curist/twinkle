# Ownership Borrow/Effect Checker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace ad-hoc copy-carrier reasoning with a general internal borrow/effect checker over ANF/CFG, so ownership can safely retain uniqueness through compatible reads and reject conflicting writes.

**Architecture:** Add an internal, flow-sensitive checker that models loans from owned values and writes to abstract regions. The checker generates constraints from lowered ANF/CFG, proves that active loans do not conflict with writes, and returns proof facts that ownership transfer can use to suppress only compatible publications. The first consumer is the dict copy-carrier pattern, but the design is intentionally general enough for future vector/record/container borrow precision.

**Lineage.** This is the **post-codegen continuation** of the sound-uniqueness analysis track ([sound-uniqueness/analysis/README.md](sound-uniqueness/analysis/README.md), "Post-codegen analysis precision"). It realizes the read side of an analysis direction that track already names — *"distinguish non-escaping reads from publication"* / *"borrow sites vs update sites"* ([sound-uniqueness/architecture.md](sound-uniqueness/architecture.md)) — for the specific gap diagnosed in [fixpoint-map-inplace.md](fixpoint-map-inplace.md) (a value read out of a dict publishes the dict). It must **extend** the existing publication/summary model (`ParamSummary.base_role = Borrowed|Consumed|Published`, `publish_call`/`publish_atom`, the Phase 5 transport recognizer), not add a parallel analysis, and it keeps that track's invariants: facts are the only source of mutability legality, missing proof stays conservative (`Shared`/`Unknown`), and no reusable flag is stamped over `persistent(aliased shell)`.

**Tech Stack:** Twinkle boot compiler (`boot/`), ownership analysis (`boot/compiler/ownership.tw`), optimizer call semantics (`boot/compiler/opt/semantics.tw`), mutable decision artifacts (`boot/compiler/codegen/ownership_verdicts.tw`), boot fixtures/suites, rebuilt CLI verification.

## Global Constraints

- This is an internal analysis only; do not add user-visible ownership syntax or type annotations.
- Do not rewrite boot compiler helper source to make one benchmark faster; the checker must recognize general ANF/CFG patterns.
- Do not globally classify `Dict.keys` as fresh while runtime `keys()` may return `pd_ORDER` by reference.
- Do not force mutable decisions by setting reusable flags over `persistent(aliased shell)` facts; compatible read handling must preserve genuinely reusable ownership facts.
- Every accepted proof must have a matching rendered proof token; every rejected near-miss fixture must render a concrete rejection reason.
- Conflict checking is default-deny: any loan/write pair not explicitly proven safe is a conflict.
- Key equality is conservative: two key locals may hold the same runtime key unless the checker proves they are distinct through a certified unique key stream.
- Loan lifetime is conservative may-liveness: a loan live on any outgoing path, return, or live-out edge ends at infinity for conflict purposes.
- After every `.tw` edit batch, run `target/twk fmt <changed .tw files>` and `target/twk lint boot/main.tw` before committing.
- Heavy verification commands run one at a time.

---

## File Structure

- Modify `boot/compiler/ownership.tw`: add borrow/effect data structures, constraint generation, conflict checking, diagnostics, and integration into transfer.
- Modify `boot/compiler/codegen/ownership_verdicts.tw`: use checker-derived structural seed targets for uniform owned-entry analysis.
- Modify `boot/tests/suites/mutable_produce_suite.tw`: add positive and negative proof/rejection assertions.
- Modify `boot/tests/suites/codegen_emit_suite.tw`: keep destructive-remove safety guarded.
- Add fixtures under `boot/tests/fixtures/sound_uniqueness/` for copy-carrier positives (raw-builtin and helper-mediated source reads), ordering conflicts, compositional uniqueness failures, cross-local key aliasing, unknown-call escapes, and keys/remove conflicts.
- Do not modify dict runtime files for this plan.

---

## Core Model

The checker tracks three concepts:

```tw
type BorrowRegion = {
  DictKeysOrder,
  DictValue(Int),
  DictUnknownValue,
  DictShell,
  UnknownRegion,
}

type BorrowKind = { SharedRead, StructuralRead }

type WriteEffect = {
  DictSet(Int),
  DictSetUnknownKey,
  DictRemove(Int),
  DictRemoveUnknownKey,
  DictShellWrite,
  UnknownWrite,
}

type Loan = .{
  source: Int,
  region: BorrowRegion,
  kind: BorrowKind,
  starts_at: Int,
  ends_at: Int,
  evidence: String,
}

type EffectProof = .{
  carrier: Int,
  source: Int,
  update_result: Int,
  key: Int?,
  loans: Vector<Loan>,
  writes: Vector<WriteEffect>,
  reason: String,
}
```

Conflict rules:

- `borrow_write_conflict` defaults to `true` for every pair not listed as safe below.
- `DictSet(kw)` conflicts with a live later-observable `DictValue(kr)` loan unless `kw` and `kr` are proven distinct or the loan ends before the write.
- `DictSet(kw)` does not conflict with a `DictValue(kr)` loan when the loan ends before the write.
- In a certified unique key stream, `DictValue(k)` in the current iteration is distinct from writes in other iterations and must still end before the current iteration's `DictSet(k)`.
- `DictSet(k)` does not conflict with a prior `DictKeysOrder` loan when the keys vector is only used as a precomputed iteration stream and the keys loan is created before all carrier writes.
- `DictSetUnknownKey` conflicts with all live `DictValue(_)` loans unless key-disjointness is proven.
- `DictRemove(_)` conflicts with any live `DictKeysOrder` loan under the current runtime.
- `DictShellWrite` and `UnknownWrite` conflict with every live loan from the same source.
- A `Loan` whose value is returned, stored into an escaping aggregate, live-out, or used on an unproven CFG edge has `ends_at = INFINITY` for conflict checks.

### Loans through helper calls

Source reads in real code are frequently mediated by small user helpers (a
defaulting `lat_get`, a `keys`-union), so loan generation must look through calls
rather than treat every user call on a source-derived value as an escape:

- A call to a certified read-only lookup helper on `source` generates the same
  loan as the inlined builtin: `h(source, key, ...)` behaves as
  `Dict.get(source, key)` and yields a `DictValue(key)` loan.
- A call to a certified uniqueness-preserving helper that consumes a
  `source.keys()` value reads the `DictKeysOrder` loan and returns the loop key
  stream.
- A read-through-helper is bounded (its loan `ends_at` is the call index) and is
  **not** a `source` escape **only** when the callee summary classifies the
  parameter that receives the `source`-derived value as `Borrowed`. "Borrowed"
  here means the source dict's **mutable structures** (its shell, order vector,
  HAMT nodes, or element storage) do not escape the callee: that argument is never
  returned/stored/aliased into the result, never mutated in place, and never used
  as an update base. Extracting an **immutable value payload** — a scalar, or a GC
  value object that carrier `set_in_place` never mutates — is permitted; that is
  exactly what a lookup helper does.
- The borrow-vs-mutable-escape check is scoped to the **carrier-source argument
  only**. A *non-carrier* dict whose `keys()` is retained into the read-only key
  stream is fine, because that dict is never written — e.g.
  `int_keys_union(old.keys(), next.keys())` retains `old.keys()` (`out := a`, and
  `insert_sorted` early-returns its input), yet only the `next`-derived argument
  must be `Borrowed`. A checker that rejects on *any* retained dict-derived
  argument would wrongly reject the motivating case.
- If the source's mutable structure escapes, the source argument is mutated, or
  the helper is unrecognized, the value escapes (`ends_at = INFINITY`; rejection
  `source-escape` for a recognized-but-escaping helper, `unknown-source-use` for
  an unrecognized call). This separates an accepted helper-mediated read from both
  the unknown-call negative and the recognized-but-retains-source negative.
- Bounding a *value* loan at the call while a **reference** to the read value
  flows out is sound here because dict values are immutable GC objects and carrier
  `set_in_place` reuses only the order/spine — it never mutates an existing value
  object — so a held value reference stays valid across a later carrier set. A
  future mutable-element container would need this invariant re-checked.

---

### Task 1: Add fixtures for checker obligations

**Files:**
- Create: `boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_positive.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_helper_mediated_positive.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_get_after_set_negative.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_keys_after_set_negative.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_distinct_locals_negative.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_duplicate_helper_arg_negative.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_unknown_call_negative.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_retains_source_negative.tw`
- Create: `boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_keys_borrow_negative.tw`
- Modify: `boot/tests/suites/mutable_produce_suite.tw`
- Modify: `boot/tests/suites/codegen_emit_suite.tw`

**Interfaces:**
- Consumes: existing `produce_for`, `render_candidate_rows`, `rendered_line_for_family`, and `compile_fixture_to_linked_wat` helpers.
- Produces: tests that distinguish accepted proofs from rejected near misses.

- [ ] **Step 1: Create the positive copy-carrier fixture**

Use the existing `phase8d_dict_merge_targeted_min.tw` shape or create a smaller `phase8d_dict_copy_carrier_positive.tw` with this source shape intact:

```tw
type Out = .{ map: Dict<Int, Int> }

fn merge_like(old: Dict<Int, Int>, next: Dict<Int, Int>) Out {
  out := next
  keys := old.keys()
  for k in keys {
    before := case next.get(k) { .Some(v) => v, .None => 0 }
    out[k] = before + 1
  }
  Out.{ map: out }
}

fn caller(n: Int) Dict<Int, Int> {
  old: Dict<Int, Int> = Dict.new()
  state: Dict<Int, Int> = Dict.new()
  i := 0
  for i < n {
    old[i] = i
    state[i] = i
    out := merge_like(old, state)
    state = out.map
    i = i + 1
  }
  state
}

println(caller(3).len().to_string())
```

The fixture must use `next` for reads and `out` for writes; do not rewrite reads to `out`.

Also create `boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_helper_mediated_positive.tw`, which reads `source` **only through user helpers** (a defaulting lookup helper and a compositional-uniqueness helper), matching the motivating `merge_targeted_min` shape rather than raw builtins:

```tw
type Out = .{ map: Dict<Int, Int> }

fn lookup(m: Dict<Int, Int>, k: Int, dflt: Int) Int {
  case m.get(k) {
    .Some(v) => v,
    .None => dflt,
  }
}

fn insert_sorted(v: Vector<Int>, id: Int) Vector<Int> {
  out: Vector<Int> = []
  inserted := false
  for x in v {
    if x == id {
      return v
    }
    if !inserted and id < x {
      out = .append(id)
      inserted = true
    }
    out = .append(x)
  }
  if !inserted {
    out = .append(id)
  }
  out
}

fn union_keys(a: Vector<Int>, b: Vector<Int>) Vector<Int> {
  out := a
  for k in b {
    out = insert_sorted(out, k)
  }
  out
}

fn merge_helper_mediated(old: Dict<Int, Int>, next: Dict<Int, Int>) Out {
  out := next
  keys := union_keys(old.keys(), next.keys())
  for k in keys {
    old_x := lookup(old, k, 0)
    next_x := lookup(next, k, 0)
    out[k] = old_x + next_x
  }
  Out.{ map: out }
}

fn caller(n: Int) Dict<Int, Int> {
  old: Dict<Int, Int> = Dict.new()
  state: Dict<Int, Int> = Dict.new()
  i := 0
  for i < n {
    old[i] = i
    state[i] = i
    out := merge_helper_mediated(old, state)
    state = out.map
    i = i + 1
  }
  state
}

println(caller(3).len().to_string())
```

This positive exercises lookup-helper recognition (`lookup(next, ...)`), compositional uniqueness through the `insert_sorted` shape that actually ships in `int_keys_union` (not just a contains-guard), and a source `keys()` loan flowing through a helper — none of which `merge_like`'s raw-builtin `next.get(k)` / `old.keys()` reads cover. It also guards the source-scoping rule: `union_keys` retains its *first* argument (`out := a`, `insert_sorted` returns `v`), so the fixture must still be accepted even though `old.keys()` is retained — only `next.keys()` must be `Borrowed`. A checker that passes `merge_like` but rejects this fixture would still fail the acceptance slice on `merge_targeted_min`.

- [ ] **Step 2: Create get-after-set negative**

Create a fixture where `next.get(k)` occurs after `out[k] = ...` for the same key. The helper dict-set must stay persistent and render `copy-carrier-rejected(get-after-write)`.

- [ ] **Step 3: Create keys-after-set negative**

Create a fixture where `next.keys()` occurs after `out[k] = ...`. The helper dict-set must stay persistent and render `copy-carrier-rejected(keys-after-write)`.

- [ ] **Step 4: Create cross-local key-alias negative**

Create `boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_distinct_locals_negative.tw`:

```tw
type Out = .{ map: Dict<Int, Int>, observed: Int }

fn distinct_locals_negative(next: Dict<Int, Int>, a: Int, b: Int) Out {
  out := next
  out[a] = 1
  observed := case next.get(b) {
    .Some(v) => v,
    .None => 0,
  }
  Out.{ map: out, observed }
}

fn caller() Int {
  state: Dict<Int, Int> = Dict.new()
  state[1] = 10
  o := distinct_locals_negative(state, 1, 1)
  o.observed + o.map.len()
}

println(caller().to_string())
```

The key locals `a` and `b` are distinct locals but may hold the same runtime key. The helper dict-set must stay persistent and render `copy-carrier-rejected(get-after-write)`.

- [ ] **Step 5: Create compositional uniqueness negative**

Create a fixture where a deduping-shaped helper receives a non-unique vector argument:

```tw
fn union_like(a: Vector<Int>, b: Vector<Int>) Vector<Int> {
  out := a
  for k in b {
    if !out.contains(k) { out = .append(k) }
  }
  out
}

fn duplicate_helper_arg_negative(next: Dict<Int, Int>) Dict<Int, Int> {
  out := next
  keys := union_like([1, 1], next.keys())
  for k in keys {
    before := case next.get(k) { .Some(v) => v, .None => 0 }
    out[k] = before + 1
  }
  out
}
```

The dict-set must stay persistent and render `copy-carrier-rejected(non-unique-key-stream)`.

- [ ] **Step 6: Create the two escape negatives**

Create `phase8d_dict_copy_carrier_unknown_call_negative.tw`: `source` is passed to an *unrecognized* user helper after `carrier := source` and before/around the carrier write. The dict-set must stay persistent and render `copy-carrier-rejected(unknown-source-use)`. This exercises the **recognition** gate.

Create `phase8d_dict_copy_carrier_retains_source_negative.tw`: `source` is passed to a helper that *is* structurally recognized (lookup- or union-shaped) but whose summary lets the source's mutable structure escape — e.g. it stashes the dict into a returned aggregate, or returns the dict/order it received rather than an immutable payload. The dict-set must stay persistent and render `copy-carrier-rejected(source-escape)`. This exercises the **retain** gate (Task 3 Step 5.5), which the unknown-call negative does not — without it, the retain branch is only asserted in prose, never tested.

Pin the reason vocabulary here (used by Task 2's diagnostics and by the acceptance slice's greps): `unknown-source-use` for an unrecognized call on `source`; `source-escape` for a recognized-but-escaping helper and for a direct return / aggregate-store / global / CFG-edge escape.

- [ ] **Step 7: Create keys/remove negative**

Create a fixture with `ks := d.keys(); d = d.remove(k); use ks`. WAT must contain `rt_dict__remove` and not `rt_dict__remove_in_place`.

- [ ] **Step 8: Add assertions**

In `mutable_produce_suite.tw`, assert:

- Positive fixtures (`phase8d_dict_copy_carrier_positive` and `phase8d_dict_copy_carrier_helper_mediated_positive`): selected dict-set and proof contains `borrow-effect` or `copy-carrier`.
- Each negative fixture: persistent decision and matching rejection reason.

In `codegen_emit_suite.tw`, assert keys/remove keeps persistent remove.

- [ ] **Step 9: Run red/green baseline**

Run:

```bash
target/twk fmt boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_positive.tw
target/twk fmt boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_helper_mediated_positive.tw
target/twk fmt boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_get_after_set_negative.tw
target/twk fmt boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_keys_after_set_negative.tw
target/twk fmt boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_distinct_locals_negative.tw
target/twk fmt boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_duplicate_helper_arg_negative.tw
target/twk fmt boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_unknown_call_negative.tw
target/twk fmt boot/tests/fixtures/sound_uniqueness/phase8d_dict_copy_carrier_retains_source_negative.tw
target/twk fmt boot/tests/fixtures/sound_uniqueness/phase8e_dict_remove_keys_borrow_negative.tw
target/twk fmt boot/tests/suites/mutable_produce_suite.tw
target/twk fmt boot/tests/suites/codegen_emit_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: positive proof tests are red; safety negatives are green or red only because the new rejection reason is not rendered yet.

---

### Task 2: Add borrow/effect data structures and diagnostics without decisions

**Files:**
- Modify: `boot/compiler/ownership.tw`

**Interfaces:**
- Produces: proof/rejection diagnostics in CFG/census output without changing selected mutable decisions.

- [ ] **Step 1: Add borrow/effect types**

Add the model types from the Core Model section near `BlockPrep`.

- [ ] **Step 2: Add conflict helpers**

Add pure helpers:

```tw
fn borrow_write_conflict(loan: Loan, write: WriteEffect) Bool
fn loan_ends_before_write(loan: Loan, write_index: Int) Bool
fn keys_proven_distinct(a: Int, b: Int, stream: StreamFact) Bool
fn render_effect_proof(p: EffectProof) String
fn render_effect_rejection(reason: String) String
```

`borrow_write_conflict` must default to `true` and return `false` only for explicitly proven-safe combinations.

- [ ] **Step 3: Generate diagnostics for obvious candidates**

Find `carrier := source` followed by `Dict.set(carrier, key, value)`. Render either:

```text
copy-carrier-candidate source Lsrc key Lkey
```

or:

```text
copy-carrier-rejected(<reason>)
```

Do not suppress publication and do not select new in-place decisions in this task.

- [ ] **Step 4: Verify diagnostics only**

Run:

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: diagnostics appear for positive and negative fixtures; selected decisions are unchanged.

---

### Task 3: Implement compositional unique-key-stream and read-only source-helper recognition

**Files:**
- Modify: `boot/compiler/ownership.tw`

**Interfaces:**
- Produces: call-site `UniqueStream` facts for vectors used as loop key streams, and `lookup-helper` / bounded-loan facts for source reads mediated by certified read-only helpers.

- [ ] **Step 1: Add stream facts**

Add:

```tw
type StreamFact = { UnknownStream, UniqueIntKeys }
```

Track stream facts per local in the proof recognizer, not in the main ownership lattice.

- [ ] **Step 2: Mark direct unique producers**

`Dict.keys(d)` produces `UniqueIntKeys` because dictionary keys are unique, even though the vector may alias dict order.

- [ ] **Step 3: Certify uniqueness-preserving helpers compositionally**

A helper like `int_keys_union(a, b)` is uniqueness-preserving only when every vector argument is already `UniqueIntKeys`. At a call site:

1. Check the callee body has a dedupe-preserving shape (`insert_sorted` or contains-guarded append).
2. Check every vector input argument has `UniqueIntKeys`.
3. Check the callee does not mutate its vector parameters in place: each is only iterated/read (`for k in b`, `contains`), never assigned through, used as an in-place-update base, or passed to a mutating position. This is required because a `source.keys()` argument aliases the source dict's `pd_ORDER` by reference (Step 2) — a dedupe-shaped helper that appended into that vector in place would corrupt the source's order before the merge loop runs, with no carrier write involved. This mirrors the non-mutation requirement on the lookup helper (Step 4).
4. Mark the call result `UniqueIntKeys` only if all three checks pass.

- [ ] **Step 4: Certify read-only lookup helpers**

Add:

```tw
fn function_is_dict_lookup_helper(f: CfgFunction, b: BuiltinRegistry, sem: OptimizerSemantics) Bool {
  // Structural scan, not name-based. True only when the dict param is used solely
  // as arg0 of `Dict.get` (the sole surface lookup; it already returns Option, and
  // the internal `dict$get_unsafe` if a helper reads via it), the return is that
  // get's payload/default or an immutable value derived from it, and the dict param
  // is never stored, returned as a shell, embedded in an aggregate, mutated in
  // place, passed to an unknown/user call, or used as an update base.
  scan_dict_lookup_helper_shape(f, b, sem)
}
```

Also add `scan_dict_lookup_helper_shape(f, b, sem) Bool` in the same section. It must certify the `lat_get(m, k, default)` shape and reject any helper that stores, returns-as-shell, embeds, or mutates the dict param. Do not key on helper names. This is what lets `merge_targeted_min` — which reads `next` only through `lat_get`, never through a raw `Dict.get` — be recognized instead of treated as a source escape.

- [ ] **Step 5: Bound loans that flow through certified read-only helpers**

A source read is frequently mediated by a helper, so a call to a certified lookup helper or a certified uniqueness-preserving helper on a source-derived value must be treated as a bounded read, not an escape, when the callee summary classifies **the parameter that receives the source-derived value** as `Borrowed` in the mutable-structure sense of the Core Model ("Loans through helper calls"): the source dict's shell/order/nodes/elements do not escape, and the argument is not mutated in place. Extracting an immutable value payload is allowed:

1. `h(source, key, ...)` for a certified lookup helper generates a `DictValue(key)` loan on `source`, exactly like `Dict.get(source, key)`.
2. A certified uniqueness-preserving helper consuming `source.keys()` reads the `DictKeysOrder` loan and produces the loop key stream; that loan ends at the call.
3. Such a loan's `ends_at` is the call index, and the call is not a `source` escape.
4. **Scope the check to the source-derived argument only.** Other (non-carrier) dict arguments retained into the read-only key stream do not disqualify the call — `int_keys_union(old.keys(), next.keys())` retains `old.keys()` (`out := a`, `insert_sorted` returns its input) yet only `next.keys()` must be `Borrowed`. Rejecting on any retained dict-derived argument would reject the motivating case.
5. If the source-derived argument's mutable structure escapes, that argument is mutated, or the helper is unrecognized, fall back to escape handling (`ends_at = INFINITY`, reason `source-escape` for a recognized-but-escaping helper / `unknown-source-use` for an unrecognized call).

- [ ] **Step 6: Verify uniqueness and helper-mediated recognition**

Run the boot suite. Expected: the duplicate-helper-arg negative renders `copy-carrier-rejected(non-unique-key-stream)` and remains persistent; the helper-mediated positive renders a `copy-carrier-candidate` (its `lookup(next, ...)` and `union_keys(..., next.keys())` reads recognized as bounded loans, not escapes).

---

### Task 4: Implement full loan/write checking for dict copy-carriers

**Files:**
- Modify: `boot/compiler/ownership.tw`

**Interfaces:**
- Consumes: `UniqueIntKeys` facts and lookup-helper / bounded-loan facts from Task 3.
- Produces: accepted `EffectProof` only when ordering, key-disjointness, and escape checks pass.

- [ ] **Step 1: Generate loans**

Generate loans for:

- `Dict.keys(source)` → `Loan(region: DictKeysOrder, kind: StructuralRead)`.
- `Dict.get(source, key)` → `Loan(region: DictValue(key), kind: SharedRead)`.
- Recognized lookup helper calls → same as `Dict.get(source, key)`.

Compute `ends_at` with conservative may-liveness. If a loan value is returned, stored into an escaping aggregate, live-out, or used on an unproven CFG edge, set `ends_at` to `INFINITY`.

- [ ] **Step 2: Generate writes**

Generate writes for:

- `Dict.set(carrier, key, value)` → `DictSet(key)`.
- `Dict.remove(carrier, key)` → `DictRemove(key)`.
- Unknown carrier mutations → `UnknownWrite`.

- [ ] **Step 3: Check ordering and key aliasing**

Accept only when:

- keys/order loans used as key streams are created before all carrier writes;
- value loans for key local `kr` end before every carrier write whose key local `kw` is not proven distinct from `kr`;
- no value loan for a possibly-same key is created after a carrier write;
- every written loop key stream is `UniqueIntKeys`, which proves distinctness across different loop iterations of the same stream;
- two different straight-line key locals are treated as possibly equal unless the checker has a proof of distinctness.

- [ ] **Step 4: Check escapes**

Reject when `source` is used outside accepted loans and the sanctioned carrier move, including terminators, CFG edge args, live-out, aggregate stores, globals, closure captures, unknown calls, and unrecognized user calls. A read of `source` through a certified read-only lookup or uniqueness-preserving helper (Task 3 Step 5) is an accepted bounded loan, not an escape; a call is an escape when it passes `source` to any *other* user helper, or to a recognized helper whose summary lets the source-derived argument's mutable structure escape or mutates it. The check is on the source-derived argument only — a co-argument that is a different, unwritten dict does not make the call an escape.

- [ ] **Step 5: Render active rejection reasons**

Use stable reasons:

- `get-after-write`
- `keys-after-write`
- `non-unique-key-stream`
- `source-escape`
- `remove-conflicts-with-keys`
- `unknown-source-use`

The cross-local alias fixture must render `get-after-write` because two different key locals are not proven distinct.

- [ ] **Step 6: Verify proof-only behavior**

Run boot tests. Expected: positives have accepted proof diagnostics, negatives have expected rejection reasons, selected decisions are unchanged until Task 5.

---

### Task 5: Integrate compatible loans into ownership transfer

**Files:**
- Modify: `boot/compiler/ownership.tw`

**Interfaces:**
- Produces: accepted compatible loans suppress publication so carrier remains genuinely reusable.

- [ ] **Step 1: Thread accepted proof facts into transfer**

Pass accepted proof facts into `forward_block_body` / `transfer_op`.

- [ ] **Step 2: Suppress only compatible publications**

At call sites covered by accepted loans:

- `Dict.keys(source)`: do not publish `source`; result ownership remains non-unique unless separately proven.
- `Dict.get(source, key)`: do not publish `source`; preserve ordinary result handling.
- recognized lookup helper: treat the source dict param as borrowed for this call site only.

- [ ] **Step 3: Keep normal mutable selection path**

Do not set reusable flags directly from proof existence. The carrier must be `Unique`, valid, and last-use according to normal `shell_reusable`.

- [ ] **Step 4: Render state-consistent selected proof**

Selected update verdicts should include:

```text
base=reuse(unique borrow-effect copy-carrier source Lsrc key Lkey)
```

If proof exists but state is not reusable, render a failed state-check reason and keep persistent.

- [ ] **Step 5: Verify behavior**

Run boot tests. Expected: positive copy-carrier and merge-targeted fixtures select in-place; all negatives remain persistent with rejection reasons.

---

### Task 6: Add structural seed targets and rebuild acceptance

**Files:**
- Modify: `boot/compiler/ownership.tw`
- Modify: `boot/compiler/codegen/ownership_verdicts.tw`
- Modify: `docs/plans/fixpoint-map-inplace.md` if measuring boot-main changes.

**Interfaces:**
- Produces: uniform owned-entry seeding can request analysis for helpers with accepted borrow/effect proofs.

- [ ] **Step 1: Expose seed target params**

Add `pub fn structural_seed_params_for_borrow_effects(f: CfgFunction, b: BuiltinRegistry, sem: OptimizerSemantics, table: SummaryTable) Vector<Int>` returning param indices whose ownership is required by accepted effect proofs and whose carrier flows to return.

Return **only** the copy-carrier *source* param (the dict whose carrier is moved and returned). Do **not** seed a uniqueness-preserving helper's retained vector param (`a` in `int_keys_union`) as `Unique`: because that vector can be a `keys()` result aliasing a dict's `pd_ORDER` by reference, a `Unique` seed there combined with a caller passing a fresh vector would let an in-place append corrupt the aliased order — reopening the B1 hazard from the seeding side. Seed targets are dict source params only, never the key-stream vector params.

- [ ] **Step 2: Integrate with uniform_entry_seeds**

In `ownership_verdicts.tw`, include these structural seed targets alongside existing consumed-path targets. Preserve mixed-caller/export/closure safety.

- [ ] **Step 3: Rebuild and verify**

Run:

```bash
target/twk lint boot/main.tw
make bundle-cli
target/twk run boot/tests/main.tw
target/twk ir boot/main.tw --census --sites > /tmp/twinkle-sites.txt
rg -n "^run_fixpoint\t|^merge_targeted|^join_entry_ownership_assumed|^join_entry_ownership\t" /tmp/twinkle-sites.txt
```

Expected: focused copy-carrier rows are selected. `run_fixpoint` is measured, not assumed.

- [ ] **Step 4: Document boundary**

If `run_fixpoint` remains persistent, append a note to `docs/plans/fixpoint-map-inplace.md` saying the borrow/effect checker solved the copy-carrier shape but broader helper publication/double-embed routes remain.

---

## Self-Review

- **Spec coverage:** This plan generalizes copy-carrier reasoning into borrow/effect constraints, includes compositional uniqueness, cross-local key aliasing, read-only helper-mediated source reads (lookup + uniqueness pass-through), active rejection diagnostics, default-deny conflict handling, and state-consistent ownership integration.
- **Placeholder scan:** No unresolved placeholders remain.
- **Type consistency:** Planned names are consistent: `BorrowRegion`, `BorrowKind`, `WriteEffect`, `Loan`, `EffectProof`, `StreamFact`, `function_is_dict_lookup_helper`, `scan_dict_lookup_helper_shape`, and `structural_seed_params_for_borrow_effects`.
- **Acceptance:** The plan requires positive selected decisions, negative rejection reasons, remove/keys safety, rebuilt CLI evidence, and explicit measurement of `run_fixpoint`.
