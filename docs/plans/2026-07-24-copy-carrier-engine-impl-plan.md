# Copy-Carrier Borrow/Effect Engine — Implementation Plan

> **STATUS (Tasks 4–6 landed):** The copy-carrier borrow/effect engine is implemented and
> self-host-stable. `analyze_copy_carriers` proves the param-sourced copy-carrier shape
> (Task 4); the borrow-move + publish suppression are threaded through the forward fixpoint
> (Task 5); and copy-carrier source params are seeded Unique behind the mixed-caller guard
> via `uniform_entry_seeds` (Task 6). Both copy-carrier positives and the `merge_targeted_min`
> fixture flip to `dict$set_in_place` with a `borrow-effect copy-carrier` proof; all six
> negative fixtures are rejected with concrete reasons; the mixed-caller helper stays
> persistent. On `boot/main.tw`, 11 dict sites flip with zero unresolved candidates. The one
> remaining aspiration — `run_fixpoint`'s loop-carried maps (and the `merge_targeted__` call
> inside it) — is **out of scope for the copy-carrier shape** and stays as the tracked marker
> `boot ownership fixpoint maps should produce in-place dict decisions`; see the boundary
> writeup in [fixpoint-map-inplace.md](fixpoint-map-inplace.md). Task 3 was superseded by the
> key-stream-uniqueness checker (see banner at Task 3 below).

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the dict copy-carrier borrow/effect proof engine and its ownership-transfer integration so that a dict aliased from a source param (`out := next`), written by key while the source is read through compatible loans, lowers to in-place `dict$set_in_place` — flipping `merge_targeted_min` and the helper-mediated positive to in-place while keeping every near-miss persistent with a concrete rejection reason.

**Architecture:** A per-function analysis (in `boot/compiler/ownership.tw`) recognizes copy-carrier update sites, models source reads as bounded `Loan`s and carrier writes as `WriteEffect`s, and proves ordering + key-distinctness + no-escape. Uniqueness of the loop key stream is certified compositionally by a conservative whole-program dedupe-helper classification stored on `SummaryTable`. Accepted proofs drive two publication suppressions (the `init_hinge` alias-move and the `dict$keys` read) threaded through the forward fixpoint, plus a `Unique` entry seed for the source param, so the existing `shell_reusable` path fires normally. Failed proofs never reject a program: they keep persistent lowering and render a reason.

**Tech Stack:** Twinkle boot compiler (`boot/`), self-hosted. Iterate with `target/twk run boot/tests/main.tw` (the boot-test path compiles the edited source; `twk ir` uses the stale compiled binary until `make bundle-cli`). Heavy commands run one at a time.

---

## Status at plan authoring (already landed on branch `fixpoint-map-inplace`)

These are **done and verified** by the parent plan
([2026-07-24-ownership-borrow-effect-checker-plan.md](2026-07-24-ownership-borrow-effect-checker-plan.md));
this plan continues from here. Do **not** redo them:

- **Fixtures + assertions** (parent Task 1): nine fixtures under
  `boot/tests/fixtures/sound_uniqueness/` plus positive/negative assertions in
  `mutable_produce_suite.tw` and a remove-safety WAT test in `codegen_emit_suite.tw`.
  Baseline is **10 red**: 2 pre-existing acceptance (`merge_targeted_min` "expected 3
  got 2", boot fixpoint), 2 positives (persistent, need proof), 6 negative rejection
  reasons (persist correctly, reason not rendered yet). The remove-safety test is green.
- **Model types + coarse diagnostic** (parent Task 2): `BorrowRegion`, `BorrowKind`,
  `WriteEffect`, `Loan`, `EffectProof`, `StreamFact`, `borrow_infinity`,
  `keys_proven_distinct`, `loan_ends_before_write`, `borrow_write_conflict`,
  `render_effect_rejection`, `render_effect_proof`, and `copy_carrier_note` (renders
  `copy-carrier-candidate source Lsrc key Lkey` into the update verdict text). Decisions
  unchanged.
- **Infra for this plan:** `SummaryTable` extended with `dedupe_helpers: Dict<Int, Bool>`
  + `table_is_dedupe_helper`; `empty_summary_table` and the two `cfg_summary_suite`
  construction sites updated. The fixture `phase8d_dict_merge_targeted_min.tw`'s
  `int_keys_union` was changed from plain append to contains-guarded dedupe (sanctioned
  by [2026-07-24-merge-targeted-owned-dict-plan.md](2026-07-24-merge-targeted-owned-dict-plan.md)
  Step 2) so its key stream is genuinely unique.

**Verified IR facts driving this plan** (see the parent plan's "Implementation notes"):

- `out := next` is `AInit(ALocal(next))` → `init_hinge` (ownership.tw ~1033). Because
  `next` is read after the alias, `init_hinge` takes the **alias branch** →
  `publish_local(next)` → carrier `persistent(aliased shell)`.
- `dict$keys` has **no** `CallSemantics` → `transfer_call` → `publish_call` publishes its
  dict arg. `Dict.get` is `.ReadOnly` (no publish). `lat_get`'s summary is `p0=Borrowed …
  ret=alias(p2)` so `transfer_summarized_call`'s `.Borrowed => {}` already avoids
  publishing the source for value reads.
- In `merge_targeted_min` the **vector** carrier `next_locked := locked; .append` is
  already `reuse(unique)`; only the **dict** carrier `out := next; out[k]=` is
  `persistent(aliased shell)`, solely because `next` is read after the alias-move.
- The real compiler's `int_keys_union` (ownership.tw:2532) is `out := a; for k in b { out
  = insert_sorted(out, k) }`; `insert_sorted` dedupes via `if x == id { return v }`.

**Non-negotiable soundness invariants** (from the parent plan):

- Over-certifying a dedupe helper, or accepting a non-unique key stream, is a
  **miscompile**. Under-certifying only misses the optimization. Always bias
  conservative: any loan/write pair not explicitly proven safe is a conflict; any key
  local pair not proven distinct is possibly-equal; any source use not a recognized
  bounded loan is an escape.
- Never globally reclassify `Dict.keys` as fresh/Allocate; suppress its publication only
  at proven-safe sites. Never stamp a reusable flag over `persistent(aliased shell)` — the
  carrier must become genuinely `Unique` via seed + suppression so the normal
  `shell_reusable` path fires.

---

## File Structure

- **`boot/compiler/ownership.tw`** — all engine code: dedupe-helper classification, the
  per-function copy-carrier analysis (`CopyCarrierFacts`), the `block_verdicts` rendering
  hook, and the transfer-integration suppressions (`init_hinge`, `transfer_call`,
  `ownership_stage` threading). Single file keeps the analysis holdable in context and
  next to the machinery it hooks.
- **`boot/compiler/summary.tw`** — one line in `compute` to populate
  `SummaryTable.dedupe_helpers`.
- **`boot/compiler/codegen/ownership_verdicts.tw`** — Task 6 entry-seed integration.
- **`boot/tests/suites/mutable_produce_suite.tw`**,
  **`boot/tests/suites/codegen_emit_suite.tw`** — assertions already added (parent Task
  1); this plan only adds a dedupe-classification unit assertion in `cfg_summary_suite.tw`.
- **`docs/plans/fixpoint-map-inplace.md`** — Task 6 boundary note.

---

## Data model (already in `ownership.tw`, restated for reference)

```tw
type BorrowRegion = { DictKeysOrder, DictValue(Int), DictUnknownValue, DictShell, UnknownRegion }
type BorrowKind = { SharedRead, StructuralRead }
type WriteEffect = { DictSet(Int), DictSetUnknownKey, DictRemove(Int), DictRemoveUnknownKey, DictShellWrite, UnknownWrite }
type Loan = .{ source: Int, region: BorrowRegion, kind: BorrowKind, starts_at: Int, ends_at: Int, evidence: String }
type EffectProof = .{ carrier: Int, source: Int, update_result: Int, key: Int?, loans: Vector<Loan>, writes: Vector<WriteEffect>, reason: String }
type StreamFact = { UnknownStream, UniqueIntKeys }
```

New types this plan adds (Task 4):

```tw
type CarrierProof = { CarrierAccept(EffectProof), CarrierReject(String) }
type CopyCarrierFacts = .{
  by_update: Dict<Int, CarrierProof>,  // update-result local -> accept/reject
  suppress_alias: Dict<Int, Bool>,     // AInit result local (carrier) -> suppress source publish
  suppress_read: Dict<Int, Bool>,      // read-call result local -> suppress source publish
  seed_params: Dict<Int, Bool>,        // source param local ids to seed Unique
}
```

Rejection reason vocabulary (stable; asserted by the suite): `get-after-write`,
`keys-after-write`, `non-unique-key-stream`, `source-escape`, `unknown-source-use`,
`remove-conflicts-with-keys`.

---

## Review round 1 — resolutions folded into Tasks 3–6

A subagent review of the first draft found one soundness hole and several blocking
bugs, all verified against the code. Resolutions, applied below:

- **[S1] Default-deny recognizer.** The dedupe recognizer must reject on ANY
  accumulator write it does not explicitly recognize as safe (including `AAssign`
  targeting the accumulator and unknown-callee rebinds), not default-allow. Task 3's
  combinator recognizer enumerates every accumulator-lineage write and requires each to
  be a recognized-safe form; anything else ⇒ not certified.
- **[B1] No `Vector.contains` dependency.** `Vector.contains` is a *prelude* function
  (`boot/prelude/vector.tw:203`), monomorphized to mangled per-specialization ids, so
  `b.method_id("Vector","contains")` traps the whole compiler. The contains-guarded
  shape is DROPPED. The one base dedupe primitive is `insert_sorted`'s
  equality-early-return shape (which uses `ABinOp(.., .., .., Eq)` and `Vector.append`,
  both reliably resolvable), matching the REAL compiler's `int_keys_union`
  (ownership.tw:2532). The fixture `phase8d_dict_merge_targeted_min.tw` is changed from
  its contains-guarded `int_keys_union` to an `insert_sorted`-based one (same shape as
  the helper-mediated positive and the real compiler). The `duplicate_helper_arg`
  negative keeps its contains-guarded `union_like`, which is now simply *unrecognized* →
  its stream is `UnknownStream` → `non-unique-key-stream` rejection (the asserted reason
  still renders).
- **[B2] Import fix.** `summary.tw:21` imports `compiler.ownership` in destructuring form
  only, which does not bind the `ownership` alias. Add the new function to that
  destructuring list and call it unqualified.
- **[S2] Source-not-a-write-base gate** added to Task 4's accept conditions.
- Accuracy: match `.Not` explicitly (`UnOp = { Neg, Not }`, core_ir.tw:82); name the
  classifier `classify_dedupe_helpers` and update the `SummaryTable.dedupe_helpers`
  doc comment to match; declare `analyze_copy_carriers`'s signature in Task 4.

Blocking items B3 (full `cc_suppress` threading surface) and B4 (seed uses param indices
+ mixed-caller guard + summary half) are folded into Tasks 5 and 6 respectively.

---

## Task 3: Dedupe-helper certification (sort-insert primitive + compositional)

**Files:**
- Modify: `boot/compiler/ownership.tw` (add classification near the borrow/effect section,
  ~line 715, after `render_effect_proof`)
- Modify: `boot/compiler/summary.tw` (`compute`, ~line 1261; import list ~line 21)
- Modify: `boot/tests/fixtures/sound_uniqueness/phase8d_dict_merge_targeted_min.tw`
- Modify: `boot/tests/suites/cfg_summary_suite.tw` (classification unit test)

**Interfaces:**
- Produces: `SummaryTable.dedupe_helpers[fid] == true` for certified
  uniqueness-preserving vector combinators/primitives.
- Consumes: `CfgView`, `BuiltinRegistry`, `OptimizerSemantics`.

**Soundness contract (repeat at every step):** certification is a MISCOMPILE if wrong;
non-certification only misses an optimization. Every recognizer below is
default-deny — it returns `true` only when it has structurally confirmed the safe shape,
and `false` on the first thing it does not recognize.

> **✅ DONE — replaced by the A′ key-stream-uniqueness checker.** The unsound Step-4
> recognizer described below (over-certified; two confirmed holes, shipped in `d05096e6`)
> has been **superseded and rebuilt** as the general proof checker specified in
> [2026-07-25-key-stream-uniqueness-design.md](2026-07-25-key-stream-uniqueness-design.md)
> and implemented per [2026-07-25-key-stream-uniqueness-impl-plan.md](archive/2026-07-25-key-stream-uniqueness-impl-plan.md):
> default-deny, O0–O4 obligations, an explicit `DedupeCertificate`, the full adversarial
> negative battery, and an independent review (Task 10) that found no over-certification.
> `SummaryTable.dedupe_helpers` is now `Dict<Int, DedupeCertificate>` populated on all summary
> paths. Do NOT implement Step 4 as written; the Steps 1–3, 5–10 shape below is historical
> context. **Task 4's consumer is now UNBLOCKED** — it may read `dedupe_helpers` /
> `table_is_dedupe_helper` for the stream-uniqueness fact.

- [ ] **Step 1: Change the fixture's `int_keys_union` to the sort-insert shape**

Edit `phase8d_dict_merge_targeted_min.tw` so its `int_keys_union` matches the real
compiler and the helper-mediated positive (an `insert_sorted`-based union), replacing the
contains-guarded body:

```tw
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

fn int_keys_union(a: Vector<Int>, b: Vector<Int>) Vector<Int> {
  out := a
  for k in b {
    out = insert_sorted(out, k)
  }
  out
}
```

Then `target/twk fmt` it and confirm it still runs (`target/twk run … | tail -1` prints `3`).

- [ ] **Step 2: Add op/def/block index helpers**

In `ownership.tw` near the copy-carrier section:

```tw
// A flat program-ordered instruction: `gindex` = block-array-index * 100000 + inst
// index — a sound "before/after" proxy for the structured lowering (preheader < header
// < body < exit). Loop iteration is NOT modeled; cross-iteration safety comes only from
// key-stream uniqueness, never from gindex.
type FlatInst = .{ gindex: Int, result: Int, op: AnfOp }

fn flatten_ops(blocks: Vector<CfgBlock>) Vector<FlatInst> {
  out: Vector<FlatInst> = []
  bi := 0
  for blk in blocks {
    ii := 0
    for inst in blk.instructions {
      out = .append(FlatInst.{ gindex: bi * 100000 + ii, result: inst.anf_local.id, op: inst.op })
      ii = ii + 1
    }
    bi = bi + 1
  }
  out
}

fn build_def_map(blocks: Vector<CfgBlock>) Dict<Int, AnfOp> {
  m: Dict<Int, AnfOp> = Dict.new()
  for blk in blocks {
    for inst in blk.instructions {
      m[inst.anf_local.id] = inst.op
    }
  }
  m
}
```

- [ ] **Step 3: Add a method-id bundle (no `Vector.contains`)**

```tw
type DictVecOps = .{ get: Int, get_unsafe: Int, keys: Int, set: Int, remove: Int, append: Int }

fn dict_vec_ops(b: BuiltinRegistry) DictVecOps {
  DictVecOps.{
    get: b.method_id("Dict", "get").id,
    get_unsafe: b.id("dict$get_unsafe").id,
    keys: b.id("dict$keys").id,
    set: b.method_id("Dict", "set").id,
    remove: b.method_id("Dict", "remove").id,
    append: b.method_id("Vector", "append").id,
  }
}

fn call_target(op: AnfOp) Int? {
  case op {
    .ACall(callee, _) => case callee {
      .AGlobalFunc(fid) => .Some(fid.id),
      _ => .None,
    },
    _ => .None,
  }
}

fn call_args(op: AnfOp) Vector<Atom> {
  case op {
    .ACall(_, args) => args,
    _ => [],
  }
}
```

> `Vector.append`, `Dict.get/set/remove`, `dict$keys`, `dict$get_unsafe` all resolve via
> `BuiltinRegistry` (verified). `Vector.contains` does NOT — it is intentionally absent
> here.

- [ ] **Step 4: Recognize the sort-insert primitive (`insert_sorted`)**

`insert_sorted(v, id)` returns `v` with `id` inserted, deduped: on the append path
`id ∉ v` (guaranteed by the early return), each element of `v` is appended exactly once,
and `id` is appended at most once (flag-guarded). Certify **only** when ALL hold
(default-deny; any deviation ⇒ false). **[round-2 soundness fix] The append-multiplicity
bounds (checks 4–5) are load-bearing — a body with two `append(out, x)` per iteration
double-inserts every element and is NOT dedupe-preserving, so they must be checked, not
assumed:**

1. There is a `Return(ALocal(p))` where `p` is a vector param, and the block returning it
   is the true target of a `CondBranch` whose condition local is defined by
   `ABinOp(.Eq, x, id, _)` — NOTE: `AnfOp.ABinOp(BinOp, Atom, Atom, OpKind)` (anf.tw:45);
   `Eq` is a `BinOp` in **field 0** (core_ir.tw:66), NOT the field-3 `OpKind` slot. `x` is
   the current element of the iteration over `p` (defined by `AIndex`/`AInit` off `p`) and
   `id` is a scalar param. Confirm the `.Eq` spelling with `--anf` of `insert_sorted`.
2. Every `Vector.append(acc, w)` in the function has `w` equal to that loop element `x`
   or the scalar param `id` — never any other/derived value.
3. No `AAssign` targets a vector param, and no vector param is a `Dict.set`/`Dict.remove`
   or in-place-vector base (params are read-only).
4. **Exactly one** `Vector.append(acc, x)` (append of the loop element) exists in the loop
   body — count them; ≠1 ⇒ false. This is the "each element once" invariant.
5. **Every** `Vector.append(acc, id)` is control-dependent on the same monotone boolean
   set-once flag (the `!inserted` guard) — so at most one id-append *executes* even though
   the canonical `insert_sorted` has **two** syntactic id-appends (one in-loop under
   `!inserted and id < x`, one post-loop under `!inserted`; `inserted = true` fires between
   them, making them mutually exclusive). Bound the count of **unguarded** id-appends to
   **zero** (an id-append not dominated by a `!inserted`-flag test ⇒ false). Do NOT bound
   the syntactic occurrence count to one — that would reject the real `insert_sorted`
   (ownership.tw:488–505) and the Step-1 fixture, contradicting Step 8's positive
   assertion. The flag must be a local initialized `false`, assigned only `true`, and never
   reset.

```tw
fn function_is_sort_insert_primitive(f: CfgFunction, ops: DictVecOps) Bool {
  dmap := build_def_map(f.blocks)
  // Implement checks 1–5 over f.blocks using dmap + pred/terminator tracing. Count the
  // element-append and id-append occurrences explicitly (checks 4–5). Return false on the
  // first unrecognized append operand, param mutation, or multiplicity violation.
  scan_sort_insert_shape(f, dmap, ops)
}
```

Implement `scan_sort_insert_shape` with the pred-`CondBranch` tracing pattern (a block's
single pred whose terminator is `CondBranch(cond, thenId, _, elseId, _)`; resolve `cond`
through `dmap`). Keep it tight: unrecognized structure ⇒ false. **Add a NEGATIVE unit test
(Step 8) that a double-appending variant of `insert_sorted` classifies `false`** — this is
the guard against the multiplicity soundness hole at implementation time.

- [ ] **Step 5: Recognize the dedupe combinator (default-deny + affirmative shape)**

A combinator like `int_keys_union(a, b)` is `out := a; for k in b { out = dh(out, k) }`
where `dh` is an already-certified dedupe helper. **[round-2 soundness fix]** Certifying by
"absence of known-bad writes" is disguised default-ALLOW (e.g. `out := a; junk := build();
junk` has no acc write yet certifies). The recognizer must AFFIRMATIVELY confirm the safe
shape: (1) every write to an accumulator-lineage local is a known-dedupe-helper call on the
accumulator (default-deny on all else), AND (2) the function's returned local is in the
accumulator lineage, AND (3) the lineage was extended by ≥1 known-dedupe call (a bare
param passthrough is not a combinator — leave it to `AInit`-alias stream tracking).

```tw
fn function_is_dedupe_combinator(f: CfgFunction, ops: DictVecOps, known: Dict<Int, Bool>) Bool {
  dmap := build_def_map(f.blocks)
  param_ids: Dict<Int, Bool> = Dict.new()
  for p in f.params {
    param_ids[p.id] = true
  }
  // acc_line: locals AInit-aliased from a vector param, extended ONLY by acc = dh(acc,_).
  acc_line: Dict<Int, Bool> = Dict.new()
  saw_acc := false
  for blk in f.blocks {
    for inst in blk.instructions {
      case inst.op {
        .AInit(a) => case atom_local_id(a) {
          .Some(src) => if in_set(param_ids, src) or in_set(acc_line, src) {
            acc_line[inst.anf_local.id] = true
            saw_acc = true
          },
          .None => {},
        },
        _ => {},
      }
    }
  }
  if !saw_acc {
    return false
  }
  // DEFAULT-DENY: every write to an acc-lineage local must be a known dedupe-helper call
  // on the accumulator. Track extensions; reject the first unrecognized acc write.
  ok := true
  extended := false
  for blk in f.blocks {
    for inst in blk.instructions {
      case inst.op {
        .ACall(_, cargs) => case call_target(inst.op) {
          .Some(t) => if in_set(known, t) and cargs.len() >= 1 and in_set(acc_line, atom_or_neg(cargs[0])) {
            acc_line[inst.anf_local.id] = true
            extended = true
          } else if in_set(acc_line, inst.anf_local.id) {
            ok = false // result in acc_line but produced by a non-known call ⇒ unsafe
          } else if t == ops.append and in_set(acc_line, atom_or_neg(get_arg0(inst.op))) {
            ok = false // a bare append on the accumulator is not a dedupe step
          },
          .None => {},
        },
        .AAssign(target, src) => if in_set(acc_line, target.id) {
          case atom_local_id(src) {
            .Some(sid) => if !in_set(acc_line, sid) {
              ok = false // acc rebound from an unrecognized source
            },
            .None => ok = false,
          }
        },
        _ => {},
      }
    }
  }
  // AFFIRMATIVE shape: at least one dedupe extension AND the returned value is acc-lineage.
  ok and extended and return_local_in_set(f, acc_line)
}

// True iff EVERY `Return(Some(ALocal(l)))` in the function returns a local in `set`
// (and at least one Return exists). A `Return(None)`/non-local return ⇒ false.
fn return_local_in_set(f: CfgFunction, set: Dict<Int, Bool>) Bool {
  saw := false
  for blk in f.blocks {
    case blk.terminator {
      .Some(.Return(ret)) => case ret {
        .Some(a) => case atom_local_id(a) {
          .Some(id) => if in_set(set, id) {
            saw = true
          } else {
            return false
          },
          .None => return false,
        },
        .None => return false,
      },
      _ => {},
    }
  }
  saw
}

fn atom_or_neg(a: Atom) Int {
  case atom_local_id(a) {
    .Some(id) => id,
    .None => 0 - 1,
  }
}

fn get_arg0(op: AnfOp) Atom {
  case op {
    .ACall(_, args) => if args.len() > 0 {
      args[0]
    } else {
      .ALitVoid
    },
    _ => .ALitVoid,
  }
}

fn function_is_dedupe_helper(f: CfgFunction, ops: DictVecOps, known: Dict<Int, Bool>) Bool {
  function_is_sort_insert_primitive(f, ops) or function_is_dedupe_combinator(f, ops, known)
}
```

> The `AAssign` accumulator-carry check is what closes S1: `AAssign` targets are inspected
> and any unrecognized source disqualifies. Verify against `--anf` that the loop-carried
> accumulator uses `AAssign(acc, callResult)` (the review confirmed this for
> `int_keys_union`). If the carry is instead a block param, extend `acc_line` seeding to
> include the loop header param bound from an acc-lineage arg; still default-deny.

- [ ] **Step 6: Whole-view classification in SCC/dependency order**

```tw
pub fn classify_dedupe_helpers(view: CfgView, b: BuiltinRegistry, sem: OptimizerSemantics) Dict<Int, Bool> {
  ops := dict_vec_ops(b)
  known: Dict<Int, Bool> = Dict.new()
  pass := 0
  for pass < 4 {
    changed := false
    for f in view.functions {
      if !in_set(known, f.func_id) and function_is_dedupe_helper(f, ops, known) {
        known[f.func_id] = true
        changed = true
      }
    }
    if !changed {
      return known
    }
    pass = pass + 1
  }
  known
}

pub fn with_dedupe_helpers(table: SummaryTable, view: CfgView, b: BuiltinRegistry, sem: OptimizerSemantics) SummaryTable {
  table.dedupe_helpers = classify_dedupe_helpers(view, b, sem)
  table
}
```

Also update the `SummaryTable.dedupe_helpers` doc comment (ownership.tw ~82): name the
classifier `classify_dedupe_helpers` (currently says `classify_helpers`) AND drop the now-
inaccurate "contains- or sorted-guarded append" phrasing — it should read "sort-insert
(equality-early-return) primitives and compositional combinators over them."

- [ ] **Step 7: Populate from `summary.compute` (fix the import)**

In `boot/compiler/summary.tw`, add `with_dedupe_helpers` to the existing
`use compiler.ownership.{ … }` destructuring list (line ~21), then at the end of
`compute` (before `table`):

```tw
  table = with_dedupe_helpers(table, view, b, sem)
  table
```

(Call it **unqualified** — the destructuring import does not bind an `ownership` alias.)

- [ ] **Step 8: Classification unit tests (positive AND negative)**

In `cfg_summary_suite.tw`, compile the helper-mediated positive fixture; assert
`insert_sorted`, `union_keys`, and the (now sort-insert-based) fixture `int_keys_union`
are in `table.dedupe_helpers`, while a plain-append or unrelated function is not. Resolve
func ids via the suite's existing function-name lookup.

**Add these NEGATIVE classification tests (they guard the round-2 soundness fixes — a
regression here is a latent miscompile, so they are not optional):**
- A double-appending variant of `insert_sorted` (two `out = .append(x)` per iteration)
  MUST classify `false` (append-multiplicity check 4).
- A "combinator" that returns a non-accumulator local (`out := a; junk := …; junk`) MUST
  classify `false` (affirmative return-in-lineage check).
- A bare passthrough (`out := a; out`, no dedupe extension) MUST classify `false`
  (extended check).
- A plain-append union (`out := a; for k in b { out = .append(k) }`, no dedupe) MUST
  classify `false` (bare-append-on-accumulator check).

- [ ] **Step 9: fmt, lint, run**

```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/summary.tw \
  boot/tests/fixtures/sound_uniqueness/phase8d_dict_merge_targeted_min.tw \
  boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: the classification unit test passes; the 10 copy-carrier reds are unchanged
(classification is not yet consumed by decisions); no compiler trap (confirms B1 fixed).

- [ ] **Step 10: Commit**

```bash
git add boot/compiler/ownership.tw boot/compiler/summary.tw \
  boot/tests/fixtures/sound_uniqueness/phase8d_dict_merge_targeted_min.tw \
  boot/tests/suites/cfg_summary_suite.tw
git commit -m "ownership: certify sort-insert + compositional dedupe helpers (default-deny)"
```

---

## Task 4: Per-function copy-carrier loan/write proof

**Files:**
- Modify: `boot/compiler/ownership.tw`

**Interfaces:**
- Produces `CopyCarrierFacts` (see Data model) from a function's blocks + `SummaryTable` +
  builtins + semantics, via the entry point (declare this exact signature; referenced by
  Task 6):

  ```tw
  pub fn analyze_copy_carriers(
    blocks: Vector<CfgBlock>,
    params: Vector<LocalId>,
    table: SummaryTable,
    b: BuiltinRegistry,
    sem: OptimizerSemantics,
  ) CopyCarrierFacts
  ```
- Consumes: `dedupe_helpers`, per-callee `Summary.params[i].base_role`, `Summary.ret`.

- [ ] **Step 1: Add `CarrierProof`/`CopyCarrierFacts` types and an empty ctor**

```tw
type CarrierProof = { CarrierAccept(EffectProof), CarrierReject(String) }
type CopyCarrierFacts = .{
  by_update: Dict<Int, CarrierProof>,
  suppress_alias: Dict<Int, Bool>,
  suppress_read: Dict<Int, Bool>,
  seed_params: Dict<Int, Bool>,
}
fn empty_copy_carrier_facts() CopyCarrierFacts {
  CopyCarrierFacts.{ by_update: Dict.new(), suppress_alias: Dict.new(), suppress_read: Dict.new(), seed_params: Dict.new() }
}
```

- [ ] **Step 2: Detect carriers**

Over `flatten_ops(blocks)`: a carrier is `AInit(ALocal(src))` where `src` is a function
param AND the carrier local (or its loop-carried rebinds) is the base of a `Dict.set`.
Record `carrier_local -> (source_param, init_gindex)`. Match the write base to the carrier
by exact local-id equality (verified: `update L2 base=L2`, `update L19 base=L19`). Track
loop-carried rebinds via `AAssign(carrier, newval)` and the block loop-param carrying it —
in practice the base local id equals the `AInit` result id, so equality-match suffices;
if a fixture shows otherwise, extend to the loop param.

- [ ] **Step 3: Collect writes on the carrier**

For each carrier, gather `WriteEffect`s with their gindex:
- `Dict.set(carrier, key, val)` → `DictSet(key_local)`.
- `Dict.remove(carrier, key)` → `DictRemove(key_local)`.
- any other in-place-capable mutation of the carrier → `UnknownWrite`.

- [ ] **Step 4: Collect loans on the source and classify escapes**

Enumerate every use of `source` across `flatten_ops` (via `op_uses`) and terminators:
- the sanctioned carrier `AInit(source)` → not a loan, not an escape (record the carrier
  result local into `suppress_alias`).
- `Dict.get(source, k)` / `dict$get_unsafe(source, k)` → `DictValue(k)` loan, `ends_at =
  gindex`.
- `dict$keys(source)` → `DictKeysOrder` loan, `ends_at = gindex`; record the call result
  local into `suppress_read`.
- user call `h(args)` with `source` at index `i`, where `summary.params[i].base_role ==
  Borrowed` AND `summary.ret` does not alias param `i` (`ret != MayAliasParams` containing
  `i`, and no `ret_paths` own-from-param `i`): bounded read loan. Determine the key: the
  helper arg (other than `source`) whose local equals a carrier write key local ⇒
  `DictValue(thatKey)`; else `DictUnknownValue`. `ends_at = gindex`. Record result local
  into `suppress_read` only when the loan is a value read (not needed for value reads that
  are already non-publishing, but harmless).
- ANY other use (return/edge-arg terminator, `ARecord`/`AVariant`/`ARecordUpdate` value,
  `AGlobalSet`, `AMakeClosure` capture, a call whose param-for-source is not `Borrowed`,
  an unsummarized/unknown callee) ⇒ **escape**. Reason: `unknown-source-use` for an
  unrecognized/unsummarized callee; `source-escape` for a summarized-but-escaping callee
  and for direct return/aggregate/global/closure/edge escape.

If any escape: `by_update[write_result] = CarrierReject(reason)` for every carrier write;
clear this carrier's `suppress_*` entries; continue to the next carrier.

- [ ] **Step 5: Compute the key stream fact**

Build `stream_fact` per vector local: `dict$keys(_)` result ⇒ `UniqueIntKeys`;
`AInit(ALocal(x))` inherits `x`'s fact; a call to a `dedupe_helpers` function whose every
vector argument is `UniqueIntKeys` ⇒ `UniqueIntKeys`; else `UnknownStream`.

> **Coverage note (round-2 nit).** With contains-guard dropped, the `duplicate_helper_arg`
> negative's `union_like` is now *unrecognized* (bare append on the accumulator) ⇒
> `UnknownStream` ⇒ `non-unique-key-stream` — the asserted reason still renders, but it
> exercises the "callee not a dedupe helper" branch, not the "certified helper called with
> a non-unique arg (`[1,1]`)" arg-uniqueness gate. To keep that gate under test, OPTIONALLY
> add a fixture whose union helper is `insert_sorted`-based (so it certifies) but is called
> as `union_keys([1, 1], next.keys())`; it must reject `non-unique-key-stream` because the
> literal arg is not `UniqueIntKeys`. Not required for acceptance, but recommended.

For each
carrier write key `kw`, resolve its producing stream: trace `kw`'s def — `AInit(x)` follows
`x`; `AIndex(vec, idx)` (or the iterator element op for `for k in keys`) ⇒ the stream is
`vec`'s fact. A write whose key stream is not `UniqueIntKeys` ⇒ reject
`non-unique-key-stream`.

- [ ] **Step 6: Ordering + key-alias checks**

Accept a carrier only when, for its writes `W` (keys `kw`) and loans `L`:
- **[S2] the `source` param is never itself a write base.** Before anything else, if
  `source` (or a *different* alias of it that is not the sanctioned `carrier`) is the base
  of any `Dict.set`/`Dict.remove`/in-place mutation, reject `source-escape`. Task 5's
  borrow-move leaves `source` `Unique` and valid on the shared backing, so a second
  in-place writer on `source` would double-mutate one backing. Only the single sanctioned
  `carrier` may be a write base.
- every `DictKeysOrder` loan has `starts_at < min(gindex of W)` (keys created before all
  writes); else `keys-after-write`.
- for every `DictValue(kr)` loan and every write `DictSet(kw)`: either `loan_ends_before_write(loan,
  write.gindex)` (read strictly before that write in program order) OR `kr` and `kw` are
  proven distinct. Two different straight-line key locals are NOT distinct (conservative).
  A same-stream `UniqueIntKeys` loop key is distinct *across iterations* but must still end
  before the *current* iteration's write — which holds when the read precedes the write in
  the body block. A value loan read after a same-possible-key write ⇒ `get-after-write`.
- `DictUnknownValue` loans conflict with all `DictSet` unless the loan ends before the
  write ⇒ else `get-after-write`.
- `DictRemove` with any live `DictKeysOrder` loan ⇒ `remove-conflicts-with-keys`.

On full success: `by_update[write_result] = CarrierAccept(EffectProof.{ carrier, source,
update_result: write_result, key: Some(kw), loans, writes, reason: "" })` and keep the
carrier's `suppress_alias`/`suppress_read` entries and `seed_params[source] = true`.

- [ ] **Step 7: Render from `block_verdicts`**

Extend `block_verdicts` with a `cc: CopyCarrierFacts` parameter (update the single call
site in `ownership_stage`). At the `.Update` dict verdict, replace the coarse
`copy_carrier_note` with a lookup of `cc.by_update[result]`:

```tw
note := case cc.by_update.get(inst.anf_local.id) {
  .Some(.CarrierAccept(p)) => " ${render_effect_proof(p)}",
  .Some(.CarrierReject(r)) => " ${render_effect_rejection(r)}",
  .None => "",
}
```

Compute `cc` once per function in `ownership_stage` (it has `blocks`, `params` via the
function, `table`, `b`, `sem`) and pass it to `block_verdicts`. Keep decisions unchanged in
this task: do NOT set `reusable_shell` from `cc`, do NOT thread suppression into the
fixpoint yet.

- [ ] **Step 8: Run — negatives go green, positives stay red**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: the **six negative** reason tests pass (`copy-carrier-rejected(<reason>)` now
rendered), the remove-safety test stays green, and the **two positive** tests plus the two
pre-existing acceptance tests remain red (still persistent — no decision change yet).
Net: 4 red.

- [ ] **Step 9: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: prove dict copy-carrier loans/writes and render accept/reject"
```

---

## Task 5: Integrate compatible loans into ownership transfer

**Files:**
- Modify: `boot/compiler/ownership.tw`

**Interfaces:**
- Consumes: `CopyCarrierFacts.suppress_alias`, `suppress_read`, `seed_params`.
- Produces: proven carriers become genuinely `Unique` so `shell_reusable` fires; positives
  flip to in-place.

- [ ] **Step 1: Thread a copy-carrier suppression map through the forward fixpoint**

Add a `cc_suppress: Dict<Int, Bool>` (union of `suppress_alias` + `suppress_read`, keyed by
instruction result local) parameter, threaded alongside the existing SCC `suppress` param
(do NOT overload that map — it is keyed by callee id with different semantics). **[B3] Full
threading surface — every function on the forward path must carry it, or the carrier
re-publishes and `shell_reusable` stays false:**

- `run_fixpoint_validated` **and** `run_fixpoint` (there are further forward call sites at
  ~3629 and ~3664 plus the final pass ~3725 — verify with grep) → `stabilize_seeds` (call
  sites ~3713/3714/3720) → `forward_block` → `forward_block_body` → `transfer_op` →
  `transfer_call` → `init_hinge` / `publish_call`.
- **`block_verdicts` must also receive it** — `block_verdicts` re-runs `transfer_op` per
  instruction (~line 2260), so without the map the carrier goes back through the alias
  branch and Step 5's `reuse(unique)` never renders.
- **`ownership_stage` currently passes `no_suppress` to BOTH `block_verdicts` and
  `forward_block`** (~lines 4290–4291) — replace those with the derived `cc_suppress`.
- Decide explicitly for the summary-derivation path: `summarize_function`'s
  `forward_block_body` (~line 5145) and the SEED/FIXVERIFY recompute paths. For the
  self-analysis verdict flip (this task) pass `cc_suppress`; the summary path is Task 6's
  concern (see B4 note there) — pass `Dict.new()` there for now unless Task 6 needs it.
- **Do NOT thread `cc_suppress` into `call_uniques`'s internal
  `run_fixpoint_validated` (~3937) / `transfer_op` (~4007).** `call_uniques` computes the
  caller-side arg-uniqueness used by the Task-6 seed guard; it must see the *unsuppressed*
  ownership facts. Pass `Dict.new()` there.
- At every other top-level/analyze call site where no facts exist, pass `Dict.new()`.

Grep to enumerate the real surface before editing:

```bash
rg -n 'run_fixpoint|stabilize_seeds|forward_block|forward_block_body|transfer_op|transfer_call|block_verdicts|no_suppress' boot/compiler/ownership.tw
```

- [ ] **Step 2: Suppress the `init_hinge` alias-publish**

In `init_hinge`, when `result` (the carrier local) is in `cc_suppress` (alias entry), take
a borrow-move outcome instead of the alias branch: set the carrier's ownership to the
source's ownership without `publish_local(src)` and without invalidating `src`. Guard on
`result` membership so all other `AInit` sites are unchanged.

```tw
// inside init_hinge, before the existing valid/alias branch:
if in_set(cc_suppress, result) {
  o := fact_of(st.own, a)
  return st.set_own_st(result, o)  // borrow-move: carrier inherits source ownership; source stays valid, unpublished
}
```

- [ ] **Step 3: Suppress the `dict$keys` source publish**

In `transfer_call`/`publish_call`, when the call `result` is in `cc_suppress` (read entry),
skip publishing the source arg. The cleanest hook: give `transfer_call` the `cc_suppress`
map and, for the `dict$keys` (no-`CallSemantics`) path, route through a suppressing variant
of `publish_call` that does not publish arg0 when `result` is suppressed. Value reads
(`Dict.get`, `Borrowed` helpers) already do not publish, so no change is needed for them.

- [ ] **Step 4: Seed the source param `Unique` inside the helper's own analysis**

Thread `cc.seed_params` into the `unique_seed` used by `ownership_stage` for this function
so the source dict param is seeded `Unique` when materializing verdicts (mirrors how Task 6
seeds the owned-entry analysis, but here it is the helper's self-analysis that renders the
`reuse(unique)` verdict). Merge `cc.seed_params` into `unique_seed` before
`seed_param_own`.

- [ ] **Step 5: Render the state-consistent selected proof**

In `block_verdicts`, when `cc.by_update[result]` is `CarrierAccept` AND
`shell_reusable(base,last)` is now true, render:

```text
base=reuse(unique borrow-effect copy-carrier source Lsrc key Lkey)
```

by combining the existing `reuse(unique)` shell verdict with `render_effect_proof`. If the
proof exists but `shell_reusable` is still false, render a `persistent(...
borrow-effect-state-check-failed)` note and keep persistent. Do NOT set `reusable_shell`
directly from proof existence — it must come from the seeded+suppressed `Unique` carrier
through the normal path (Step 3 invariant of the parent plan).

- [ ] **Step 6: Run — positives flip**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
target/twk run boot/tests/main.tw
```

Expected: both positive fixtures (`phase8d_dict_copy_carrier_positive`,
`phase8d_dict_copy_carrier_helper_mediated_positive`) select in-place with a `borrow-effect
copy-carrier` proof; the fixture-level `merge_targeted_min` "expected 3 got 2" test passes;
all six negatives remain persistent with reasons; remove-safety green. The boot-main
fixpoint test may still be red (needs Task 6 seeding across the real functions).

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: suppress compatible copy-carrier publications, flip proven carriers in-place"
```

---

## Task 6: Structural seed targets + rebuild acceptance

**Files:**
- Modify: `boot/compiler/ownership.tw`
- Modify: `boot/compiler/codegen/ownership_verdicts.tw`
- Modify: `docs/plans/fixpoint-map-inplace.md`

**Interfaces:**
- Produces: the owned-entry analysis requests `Unique` for copy-carrier source params so
  the real `merge_targeted__`/`run_fixpoint` are analyzed with the seed.

**[B4] Two corrections the review pinned, both required for soundness/correctness:**
- The seeding pipeline works in **param indices**, not local ids. `uniform_entry_seeds`
  (`ownership_verdicts.tw:417`) seeds `target_params(s)` (indices where `base_role ==
  Consumed && !in_place_paths.is_empty()`) and gates each with the mixed-caller guard
  (`call_uniques` → `site.arg_unique[p]`, ~lines 432–449). A new target must be an
  **index** and go through that same guard, or an exported/mixed caller that passes the
  param non-uniquely makes the seed unsound.
- **The summary half.** For the real `merge_targeted__`/`run_fixpoint` the source param is
  summarized `Published` (parent plan Implementation-notes bullet 6). The helper's OWN
  in-place decision comes from the *entry-seeded* owned-entry analysis (this task) and does
  not strictly need the summary flip; but if entry-seeding alone does not flip
  `merge_targeted__`, the follow-on is to make the source param summarize `Consumed +
  flows_to_return + in_place_paths` (so it appears in `target_params` naturally and the
  existing guard applies). Treat that as the fallback lever, measured in Step 3, not
  assumed. Do NOT hand-roll a parallel seed path that bypasses the guard.

- [ ] **Step 1: Expose seed targets as param indices**

```tw
// Param INDICES (not local ids) of copy-carrier source params whose carrier flows to
// return. Maps each accepted source LOCAL back to its param index via f.params.
pub fn structural_seed_params_for_borrow_effects(
  f: CfgFunction,
  b: BuiltinRegistry,
  sem: OptimizerSemantics,
  table: SummaryTable,
) Vector<Int> {
  cc := analyze_copy_carriers(f.blocks, f.params, table, b, sem)
  out: Vector<Int> = []
  idx := 0
  for p in f.params {
    if in_set(cc.seed_params, p.id) {
      out = .append(idx)
    }
    idx = idx + 1
  }
  out
}
```

Return ONLY the copy-carrier source dict param (the dict whose carrier is moved and
returned). Do NOT seed a dedupe helper's retained vector param (`a` in `int_keys_union`)
Unique: a `keys()` result aliases `pd_ORDER` by reference, so a Unique seed there plus a
fresh-vector caller could let an in-place append corrupt the aliased order (reopens the B1
hazard from the seeding side). These are dict source params only, never key-stream vector
params.

- [ ] **Step 2: Integrate with `uniform_entry_seeds` behind the existing guard**

**[B4, round-2] The seed pipeline drives off `target_params(s)` in THREE places, all of
which must widen to the combined set — widening only one leaves the seed unemitted.** Add
one helper and thread it through all three:

```tw
// target_params(s) (Consumed + in_place_paths) UNION the borrow-effect source indices,
// deduplicated. This is the single source of "which param indices of f to seed Unique".
fn seed_param_indices(
  f: cfg.CfgFunction, s: ownership.Summary, b: BuiltinRegistry, sem: OptimizerSemantics,
  table: ownership.SummaryTable,
) Vector<Int> {
  idxs: Dict<Int, Bool> = Dict.new()
  for p in target_params(s) {
    idxs[p] = true
  }
  for p in ownership.structural_seed_params_for_borrow_effects(f, b, sem, table) {
    idxs[p] = true
  }
  collect p in idxs.keys() {
    p
  }
}
```

Then, in `ownership_verdicts.tw`:
1. **`entry_seed_targets` (~line 386):** admit `f` when `seed_param_indices(f, s, b, sem,
   table).len() > 0` (not `target_params(s).len() > 0`). This requires passing `b`/`sem`
   into `entry_seed_targets` and finding `f` for each summarized func — iterate
   `view.functions` (it already does).
2. **The `seen`/`failed` loop (~line 439):** iterate `for p in seed_param_indices(callee_f,
   s, b, sem, table)` instead of `for p in target_params(s)` (look up the callee's
   `CfgFunction` by `site.callee`).
3. **The emission loop (~line 459):** iterate `for p in seed_param_indices(f, s, b, sem,
   table)` instead of `for p in target_params(s)`.

The **same** `call_uniques`/`arg_unique[p]` mixed-caller guard (lines 432–449) then applies
unchanged to every borrow-effect index — a source param that any exported/mixed caller
passes non-uniquely is rejected exactly like a consumed-path target. Never seed an index the
guard rejects. With all three widened, the entry-seed path flips `merge_targeted__` without
needing the summary reclassification; the summary flip stays a genuine measured fallback
(Interfaces note above), not a hidden requirement.

- [ ] **Step 3: Rebuild and measure**

```bash
target/twk lint boot/main.tw
make bundle-cli
target/twk run boot/tests/main.tw
target/twk ir boot/main.tw --census --sites > /tmp/twinkle-sites.txt
rg -n "^run_fixpoint\t|^merge_targeted|^join_entry_ownership_assumed|^join_entry_ownership\t" /tmp/twinkle-sites.txt
```

Expected: focused copy-carrier rows selected; the `merge_targeted__` boot-main test passes.
`run_fixpoint` is **measured, not assumed** — a `merge_targeted__` flip with `run_fixpoint`
still persistent is a documented success.

- [ ] **Step 4: Document the boundary**

If `run_fixpoint` remains persistent, append to `docs/plans/fixpoint-map-inplace.md` that
the borrow/effect checker solved the copy-carrier shape but broader helper
publication/double-embed routes remain (per the dependent plan's boundary note). If it
flips, record that instead.

- [ ] **Step 5: Full suite + parent-plan close-out**

```bash
target/twk run boot/tests/main.tw
```

Expected: 0 failures. Then follow the parent plan's completion (delete its row from
`docs/plans/README.md`, move both plan docs to `archive/` per the
`feedback_plans_readme_remove_when_done` convention) and finish the branch via
`superpowers:finishing-a-development-branch`.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/ownership.tw boot/compiler/codegen/ownership_verdicts.tw docs/plans/fixpoint-map-inplace.md
git commit -m "ownership: seed copy-carrier source params for owned-entry analysis; measure fixpoint"
```

---

## Self-Review

- **Spec coverage:** Covers the parent plan's Tasks 3–6 at implementable granularity:
  compositional unique-key-stream certification (3A contains-guard + 3B sort-insert +
  compositional), read-only helper-mediated source reads via summary `Borrowed` + key-match
  (Task 4 Step 4), loan/write ordering + cross-local key aliasing + escape (Task 4 Steps
  5–6), active rejection diagnostics (Task 4 Step 7), default-deny throughout, transfer
  integration via seed + suppression preserving the normal `shell_reusable` path (Task 5),
  structural seed targets (Task 6). Remove/keys safety is held by the parent Task 1 test.
- **Placeholder scan:** No TBD/TODO; every code step shows code or an exact structural
  rule. The one deferred detail (exact `UnOp` NOT variant, exact iterator-element op for
  `for k in keys`) is flagged with a `--anf`/`--cfg` verification instruction, not left
  blank.
- **Type consistency:** `CopyCarrierFacts`/`CarrierProof` fields are used consistently
  (`by_update`, `suppress_alias`, `suppress_read`, `seed_params`); `dict_vec_ops`,
  `function_is_dedupe_helper`, `classify_dedupe_helpers`, `with_dedupe_helpers`,
  `analyze_copy_carriers`, `structural_seed_params_for_borrow_effects` names are stable
  across tasks.
- **Acceptance:** Negatives green at Task 4; positives + `merge_targeted_min` green at Task
  5; `merge_targeted__` at Task 6; `run_fixpoint` measured. Soundness bias (conservative
  default-deny, no over-certification) stated up front and repeated at each proof step.
