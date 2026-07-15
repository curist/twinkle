# Phase 4 Record/Field Ownership — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the Phase 2/3 flat, per-`LocalId` ownership analysis a **path-sensitive field-ownership layer** so the compiler's characteristic idiom — a unique record shell whose fields are dicts/vectors — stops classifying as blanket publication, tracking ownership at `(local, AccessPath)` granularity for non-shell paths. Analysis-only; **no codegen** (`twk ir --census` stays 0 in-place).

**Architecture:** An **additive side map** (Approach C): the existing `own: Dict<Int,Int>` is untouched and *is* the `[]` shell fact; a new `field_own: Dict<Int, Dict<Int,Int>>` (local → PathKey → Ownership tag) holds only non-shell paths, threaded through the same join+fixpoint as `own`/`valid`/`prov`. A new leaf module `field_facts.tw` owns the `PathSeg`/`AccessPath`/`PathKey` types, a **reversible** PathKey codec (so no interned side table is needed — decode is pure arithmetic), and the map operations (`graft`/`project`/`remove_prefix`/`merge`/`clear_all`/`is_unique`). Field facts are **introduced** by construction under a single-retention proof, **preserved** through projection (`ARecordGet`) via move-or-borrow, carried through consuming builtins conservatively, and **demoted** at a single choke point (`set_own_st` clears `field_own[id]` whenever the shell leaves `Unique`).

**Tech Stack:** Twinkle (`.tw`), boot self-hosted compiler, `@std.testing` runner, hand-built single-function `AnfModule` fixtures (stable `LocalId`/`FuncId`, no optimizer) plus `twk ir <fixture> --opt` shape checks, `compiler.opt.semantics` (`call_info`/`EffectKind`), `make bundle-cli` for the CLI.

**Design spec (read before starting):** `docs/plans/sound-uniqueness/analysis/phase4-design.md` — the canonical design this plan implements. Supporting canon: `docs/plans/sound-uniqueness/analysis/records-fields.md` (shell-vs-deep semantics), `fact-lattice.md` (lattice shape), `worked-examples.md` (the `push_scope`/Case B/Case V cases), `README.md` (the Phase 4 bullets). **On any genuine conflict the canonical docs win.**

---

## File structure

- **Create** `boot/compiler/field_facts.tw` (Task 1) — the cohesive, independently-testable path unit: `PathSeg`/`AccessPath` types, the reversible `path_key`/`path_of_key` codec, and the pure map operations over a local's `Dict<Int,Int>` (`graft`, `project`, `remove_prefix`, `merge`, `clear_all`, `set_path`, `is_unique`). A **leaf**: it imports only `compiler.core_ir` (for `FieldId` — actually just `Int` ids), and is imported *by* `ownership.tw` and `cfg.tw`; it imports neither, so the module graph stays acyclic.
- **Modify** `boot/compiler/ownership.tw` — `ForwardState` gains `field_own: Dict<Int, Dict<Int,Int>>`; the `set_own_st` choke point clears it on shell demotion; `AAssign` carries it on rebind; introduction rules (`ARecord`/`AArrayLit`/`ARecordUpdate`); the collection-builtin nested rule in `transfer_builtin_call` (`.Update`) plus the `container_seg` helper; the projection rule (`ARecordGet`) with the block-local **quartet recognizer**; `join_entry_field_own`; `FixResult.exit_field_own` threaded through `run_fixpoint`; the two-verdict accumulator surfaced by `analyze`.
- **Modify** `boot/compiler/cfg.tw` — `BlockFacts` gains `field_own: Dict<Int, Dict<Int,Int>>` and `verdicts: Dict<Int,String>`; `empty_block_facts` seeds them; the `keys().len() == 0` un-analyzed guard covers them; `render_facts` prints field facts (decoding PathKeys via `field_facts`) and the per-update verdict.
- **Modify** `boot/commands/ir.tw` — nothing structural; the `--cfg` path already renders `BlockFacts` (Phase 3 wiring). Confirm field facts + verdicts appear.
- **Create + register** `boot/tests/suites/cfg_field_facts_suite.tw` — TDD gate; register in `boot/tests/main.tw`.

## Conventions (read once)

- **Soundness before coverage.** A path fact is `Unique` only when *proven*; any doubt drops it. The shell stays whatever Phase 2/3 decided, so the worst case is exactly today's behavior. Never mint a `[.f]`/`[Elem]`/`[Val]` claim without the introduction/projection proof.
- **Only `Unique` paths are stored.** `field_own[L]` holds a path key only when that path is proven `Unique`; **absence = no claim**. There is no stored `Shared`/`Unknown` path. (The tag is kept `Int` for symmetry with `own`, but it is always `0`/`Unique`.)
- **Downward-closed.** `field_own[L]` entries are meaningful only while `own[L] == Unique`. This is enforced structurally: `set_own_st(st, id, o)` clears `field_own[id]` whenever `o != Unique`. Introduction/projection therefore set the shell `Unique` **first**, then populate `field_own`.
- **PathKey is reversible (no side table).** Phase 4 path shapes are bounded (depth ≤ 2): `[Elem]`, `[Val]`, `[Field(f)]`, `[Field(f), Elem]`, `[Field(f), Val]`. `path_key`/`path_of_key` are pure arithmetic inverses, so renderers reconstruct the `AccessPath` from the key alone. This realizes Decision 3's "canonical Int PathKey encoding"; the "side table" collapses to pure decode. Deeper paths (Phase 5 variant payloads) extend the scheme.
- **Where each rule lands** (verified against `ownership.tw`):
  - `ARecord` / `AArrayLit` / `ARecordUpdate` / `ARecordGet` / `AVariant` are true `AnfOp`s in `transfer_op` — field rules attach there.
  - The consuming collection builtins are **`ACall`s** routed through `transfer_builtin_call` (`.Allocate`/`.Update`). **Their ids are not `method_id`s.** Verified in `boot/compiler/opt/semantics.tw`: `Dict.set` is `b.method_id("Dict","set")` (retained `[1,2]` = key+value), but vector element-store is `b.id("vector$set_unsafe")` (retained `[2]` = value) and vector append is `vector_builder_config(b).push_id` (retained `[1]`); there is **no** `Vector.append`/`Vector.set` `method_id`. So the `[Elem]`-vs-`[Val]` segment choice **cannot** be a `method_id` comparison in `ownership.tw`. It is carried as a **container-kind signal on the optimizer semantics**, populated at the registration site where those ids are already in scope (Task 4): a `container_kind(sem, fid) → { NotCollection, VectorLike, DictLike }` accessor. `NotCollection` (unknown/ other `.Update` ops like `Dict.remove`, builder pushes) **drops** nested facts — never guesses a segment.
  - `AVariant`'s current transfer (`field_store` each payload, result `Unique`) *is already* the Phase 4 shell-only behavior — **leave it unchanged** (variant payload paths are Phase 5).
- **Determinism.** Iterate blocks/params by id/index order; PathKeys are canonical ints; every rendered/compared line is keyed by a sorted `(LocalId, PathKey)` list, never raw `Dict` iteration order.
- **Boot gotchas (from Phase 2/3):**
  - Dict `[]`-read returns `Option` — use `.get(k)` + unwrap; never `m[k].field`.
  - `for`/assignment are statements — a `case`/`if` arm whose body is a `for` must wrap it in `{ … }`.
  - `target/twk fmt` reformats aggressively; re-locate anchors after `fmt`.
  - `target/twk lint boot/main.tw` enforces inherent-method style + `direct-rebinding`/`record-copy-helper`; a `record-copy-helper` finding means rebind a field (`r.f = v; r`) instead of rebuilding.
- **After editing `.tw`:** `target/twk fmt <files>` then `target/twk lint boot/main.tw`.
- **Test the boot suite:** `target/twk run boot/tests/main.tw`. The CLI flag needs `make bundle-cli`.
- **Test API.** One `pub fn suite() runner.Suite`, fluent `.test("case", fn(){ …; .Ok({}) })`; assertions `try assert.equal(a,b)` / `is_true` / `is_false`; `assert.fail(msg)` returns `Err`. Register in `boot/tests/main.tw`.
- **Commits.** Short imperative subject; body for non-trivial changes; what/why/how, no count metrics.

---

## Guardrails (read before Task 1)

- **G1 — `analyze` stays the empty-field wrapper.** Phase 2/3's `pub fn analyze(view,b,sem)` and `analyze_with_summaries(view,b,sem,table)` have many call sites. Do **not** change their signatures. Every `ForwardState.{ … }` construction adds `field_own: Dict.new()`; an empty `field_own` degrades to exactly today's behavior.
- **G2 — Phase 2/3 fixtures stay green throughout.** Existing fixtures never establish a proven field path (their records either aren't locally-fresh or their fields aren't proven owned), so `field_own` stays empty and every existing ownership/summary assertion is unchanged. A Phase 2/3 regression means a threading or choke-point bug.
- **G3 — Downward-closed at one choke point.** All shell demotion (publish, alias, `Unknown` result, join-lowered) flows through `set_own_st`. Clear `field_own[id]` there, not at each call site. If a fixture ever shows a `[.f]*` fact while `own[id]` is non-`Unique`, the choke point was bypassed.
- **G4 — Introduction is single-retention only.** A value grafts a deep fact only when it is `[]:Unique`, at last-use, **and** stored into exactly one slot (appears once among the aggregate's operands). Duplicate operands (`.{ a: x, b: x }`, repeated array elements) and replicated stores (`Vector.make(n, v)`) graft **nothing**.
- **G5 — The quartet move is a block-local peephole.** The forward pass reaches `R = record_get base.f` before it sees the downstream write-back, so the move cannot be decided forward-locally. A pre-pass recognizes the quartet over a **single block's** linear ANF and annotates the `ARecordGet`; a projection whose write-back lands in another block falls to the **borrow** rule (demote both R and base's `[.f]*`). Never move a field out of a record that stays live past the block.
- **G6 — Module graph is acyclic.** Layering: `ownership.tw → { field_facts.tw, cfg.tw }`, `cfg.tw → field_facts.tw`, `summary.tw → { ownership.tw, cfg.tw }`. `field_facts.tw` imports **neither** `ownership` nor `cfg` (it is a leaf). `cfg.tw` may import `field_facts.tw` (leaf, no cycle) to decode PathKeys for rendering, but must **not** import `ownership`/`summary`.
- **G7 — Reuse the existing fixpoint machinery unchanged.** `field_own` join is a monotone per-path meet on a finite lattice (facts only move down; a path present-and-`Unique` on every processed predecessor survives, else drops). It converges under the existing `fixpoint_widen_cap` + oscillation-locking; do not add a second widening scheme.

---

## Task 1: `field_facts.tw` — path types, reversible PathKey codec, map ops

Create the pure, independently-tested path unit. No `ownership.tw` changes yet.

**Files:**
- Create: `boot/compiler/field_facts.tw`.
- Create + register: `boot/tests/suites/cfg_field_facts_suite.tw`; `boot/tests/main.tw`.

- [ ] **Step 1: Write the failing unit tests (codec + map ops)**

Create `boot/tests/suites/cfg_field_facts_suite.tw`:

```tw
use @std.testing.assert as assert
use @std.testing as runner

use compiler.field_facts as ff

fn tag_u() Int {
  0
}

// A local's field map with a single Unique path.
fn one(p: ff.AccessPath) Dict<Int, Int> {
  ff.set_path(Dict.new(), p, tag_u())
}

pub fn suite() runner.Suite {
  runner
    .suite("cfg field facts")
    .test("codec: every Phase 4 path round-trips through the key", fn() {
      paths: Vector<ff.AccessPath> = [
        ff.field_path(0),
        ff.field_path(3),
        ff.elem_path(),
        ff.val_path(),
        ff.field_elem(3),
        ff.field_val(5),
      ]
      for p in paths {
        k := ff.path_key(p)
        try assert.is_true(ff.path_eq(ff.path_of_key(k), p))
      }
      .Ok({})
    })
    .test("codec: distinct paths get distinct keys (no collision)", fn() {
      keys: Vector<Int> = [
        ff.path_key(ff.field_path(0)),
        ff.path_key(ff.field_path(1)),
        ff.path_key(ff.elem_path()),
        ff.path_key(ff.val_path()),
        ff.path_key(ff.field_elem(0)),
        ff.path_key(ff.field_val(0)),
      ]
      for a, i in keys {
        for b, j in keys {
          if i != j {
            try assert.is_true(a != b)
          }
        }
      }
      .Ok({})
    })
    .test("is_unique: present -> true, absent -> false", fn() {
      m := one(ff.field_path(2))
      try assert.is_true(ff.is_unique(m, ff.field_path(2)))
      try assert.is_false(ff.is_unique(m, ff.field_path(3)))
      try assert.is_false(ff.is_unique(Dict.new(), ff.elem_path()))
      .Ok({})
    })
    .test("graft: copies shell tag to [.f] and rebases inner [Elem] -> [.f, Elem]", fn() {
      // v has shell Unique and its own [Elem]:Unique (an all-owned vector).
      v_fields := one(ff.elem_path())
      dst := ff.graft(Dict.new(), .Field(1), tag_u(), v_fields)
      try assert.is_true(ff.is_unique(dst, ff.field_path(1)))
      try assert.is_true(ff.is_unique(dst, ff.field_elem(1)))
      .Ok({})
    })
    .test("remove_prefix: drops [.f]* subtree, preserves siblings", fn() {
      m := Dict.new()
      m = ff.set_path(m, ff.field_path(1), tag_u())
      m = ff.set_path(m, ff.field_elem(1), tag_u())
      m = ff.set_path(m, ff.field_path(2), tag_u())
      out := ff.remove_prefix(m, .Field(1))
      try assert.is_false(ff.is_unique(out, ff.field_path(1)))
      try assert.is_false(ff.is_unique(out, ff.field_elem(1)))
      try assert.is_true(ff.is_unique(out, ff.field_path(2)))
      .Ok({})
    })
    .test("project: [.f] -> shell, [.f, Elem] -> [Elem]", fn() {
      m := Dict.new()
      m = ff.set_path(m, ff.field_path(1), tag_u())
      m = ff.set_path(m, ff.field_elem(1), tag_u())
      m = ff.set_path(m, ff.field_path(2), tag_u()) // sibling, must not leak
      pr := ff.project(m, .Field(1))
      try assert.is_true(case pr.shell {
        .Some(t) => t == tag_u(),
        .None => false,
      })
      try assert.is_true(ff.is_unique(pr.fields, ff.elem_path()))
      try assert.is_false(ff.is_unique(pr.fields, ff.field_path(2)))
      .Ok({})
    })
    .test("merge: per-path meet keeps only paths Unique in both", fn() {
      a := Dict.new()
      a = ff.set_path(a, ff.field_path(1), tag_u())
      a = ff.set_path(a, ff.field_path(2), tag_u())
      b := one(ff.field_path(1))
      out := ff.merge(a, b)
      try assert.is_true(ff.is_unique(out, ff.field_path(1)))
      try assert.is_false(ff.is_unique(out, ff.field_path(2)))
      .Ok({})
    })
    .test("clear_all: whole-local demotion drops every path", fn() {
      m := one(ff.field_path(1))
      try assert.equal(ff.clear_all(m).keys().len(), 0)
      .Ok({})
    })
}
```

Register in `boot/tests/main.tw` (`use .suites.cfg_field_facts_suite` + `cfg_field_facts_suite.suite()`).

- [ ] **Step 2: Run it to verify it fails**

Run: `target/twk run boot/tests/main.tw`
Expected: FAIL — no `compiler.field_facts` module.

- [ ] **Step 3: Create `field_facts.tw`**

```tw
//! Path-sensitive field-ownership facts for the Phase 4 ownership analysis.
//!
//! Pure and value-returning (Twinkle immutability), mirroring cfg.tw/summary.tw.
//! A LEAF module: it imports neither ownership.tw nor cfg.tw, so it can be
//! imported by both without a cycle. Only Unique paths are ever stored; absence
//! of a key means "no claim". A local's field map is `Dict<Int, Int>` keyed by
//! PathKey (canonical, reversible) with an Ownership tag value (always 0/Unique
//! in Phase 4, kept Int for symmetry with ownership.own).

pub type PathSeg = { Field(Int), Elem, Val }

// segs == [] is the shell (the [] fact); it is NEVER stored in a field map.
pub type AccessPath = .{ segs: Vector<PathSeg> }

// ── Constructors ────────────────────────────────────────────────────
pub fn shell() AccessPath {
  AccessPath.{ segs: [] }
}

pub fn field_path(f: Int) AccessPath {
  AccessPath.{ segs: [.Field(f)] }
}

pub fn elem_path() AccessPath {
  AccessPath.{ segs: [.Elem] }
}

pub fn val_path() AccessPath {
  AccessPath.{ segs: [.Val] }
}

pub fn field_elem(f: Int) AccessPath {
  AccessPath.{ segs: [.Field(f), .Elem] }
}

pub fn field_val(f: Int) AccessPath {
  AccessPath.{ segs: [.Field(f), .Val] }
}

pub fn seg_eq(a: PathSeg, b: PathSeg) Bool {
  case a {
    .Elem => case b {
      .Elem => true,
      _ => false,
    },
    .Val => case b {
      .Val => true,
      _ => false,
    },
    .Field(fa) => case b {
      .Field(fb) => fa == fb,
      _ => false,
    },
  }
}

pub fn path_eq(a: AccessPath, b: AccessPath) Bool {
  if a.segs.len() != b.segs.len() {
    return false
  }
  for s, i in a.segs {
    if !seg_eq(s, b.segs[i]) {
      return false
    }
  }
  true
}

// ── Reversible PathKey codec (Phase 4 shapes, depth <= 2) ────────────
//   [Elem]            -> 1
//   [Val]             -> 2
//   [Field(f)]        -> 8 + f*4 + 0
//   [Field(f), Elem]  -> 8 + f*4 + 1
//   [Field(f), Val]   -> 8 + f*4 + 2
// Field ids are non-negative; the low 2 bits above 8 encode the optional
// trailing collection seg. Pure inverses -> no interned side table.
pub fn path_key(p: AccessPath) Int {
  segs := p.segs
  cond {
    segs.len() == 0 => 0, // shell; callers must never store this
    segs.len() == 1 => case segs[0] {
      .Elem => 1,
      .Val => 2,
      .Field(f) => 8 + f * 4,
    },
    segs.len() == 2 => case segs[0] {
      .Field(f) => case segs[1] {
        .Elem => 8 + f * 4 + 1,
        .Val => 8 + f * 4 + 2,
        _ => error("field_facts: nested seg must be Elem/Val"),
      },
      _ => error("field_facts: nested path must start at a field in Phase 4"),
    },
    _ => error("field_facts: path deeper than Phase 4 supports"),
  }
}

pub fn path_of_key(k: Int) AccessPath {
  cond {
    k == 1 => elem_path(),
    k == 2 => val_path(),
    k >= 8 => {
      base := k - 8
      f := base / 4
      sub := base % 4
      cond {
        sub == 0 => field_path(f),
        sub == 1 => field_elem(f),
        sub == 2 => field_val(f),
        _ => error("field_facts: bad path key ${k}"),
      }
    },
    _ => error("field_facts: bad path key ${k}"),
  }
}

// ── Map operations over one local's field map (PathKey -> tag) ───────
pub fn set_path(fields: Dict<Int, Int>, p: AccessPath, tag: Int) Dict<Int, Int> {
  k := path_key(p)
  if k == 0 {
    // shell facts live in ownership.own, never in a field map.
    error("field_facts: refusing to store the shell path ([]) in field_own")
  }
  fields[k] = tag
  fields
}

pub fn is_unique(fields: Dict<Int, Int>, p: AccessPath) Bool {
  case fields.get(path_key(p)) {
    .Some(t) => t == 0,
    .None => false,
  }
}

pub fn clear_all(_fields: Dict<Int, Int>) Dict<Int, Int> {
  Dict.new()
}

// Copy value v's facts (its shell tag + its own field paths) under `prefix`.
// Phase 4 prefixes are a single Field(f)/Elem/Val; v's own paths are at most a
// depth-1 collection sub-path ([Elem]/[Val]), so the grafted result is depth <= 2.
// Anything deeper is dropped (sound under-claim). Under an Elem/Val prefix no
// deeper nesting is representable in Phase 4, so only the shell tag is grafted.
pub fn graft(dst: Dict<Int, Int>, prefix: PathSeg, v_shell_tag: Int, v_fields: Dict<Int, Int>) Dict<Int, Int> {
  dst[path_key(AccessPath.{ segs: [prefix] })] = v_shell_tag
  case prefix {
    .Field(_) => {
      for k in v_fields.keys() {
        inner := path_of_key(k)
        if inner.segs.len() == 1 {
          case v_fields.get(k) {
            .Some(t) => dst[path_key(AccessPath.{ segs: [prefix, inner.segs[0]] })] = t,
            .None => {},
          }
        }
      }
      dst
    },
    _ => dst,
  }
}

// Projection inverse: gather every path under `prefix`, stripped of it. The
// [prefix] fact (if present) becomes the projected value's shell; each strict
// descendant [prefix, X] becomes an inner [X]. Siblings are ignored.
pub type Projection = .{ shell: Int?, fields: Dict<Int, Int> }

pub fn project(src: Dict<Int, Int>, prefix: PathSeg) Projection {
  shell: Int? = .None
  inner: Dict<Int, Int> = Dict.new()
  for k in src.keys() {
    p := path_of_key(k)
    if p.segs.len() >= 1 and seg_eq(p.segs[0], prefix) {
      case src.get(k) {
        .Some(t) => if p.segs.len() == 1 {
          shell = .Some(t)
        } else {
          inner[path_key(AccessPath.{ segs: [p.segs[1]] })] = t
        },
        .None => {},
      }
    }
  }
  Projection.{ shell, fields: inner }
}

// Drop the entire [prefix]* subtree (including [prefix] itself); keep siblings.
pub fn remove_prefix(fields: Dict<Int, Int>, prefix: PathSeg) Dict<Int, Int> {
  out: Dict<Int, Int> = Dict.new()
  for k in fields.keys() {
    p := path_of_key(k)
    drop := p.segs.len() >= 1 and seg_eq(p.segs[0], prefix)
    if !drop {
      case fields.get(k) {
        .Some(t) => out[k] = t,
        .None => {},
      }
    }
  }
  out
}

// Per-path meet for joins: only Unique paths are stored, so a path survives iff
// present in both -> key intersection.
pub fn merge(a: Dict<Int, Int>, b: Dict<Int, Int>) Dict<Int, Int> {
  out: Dict<Int, Int> = Dict.new()
  for k in a.keys() {
    if b.has(k) {
      case a.get(k) {
        .Some(t) => out[k] = t,
        .None => {},
      }
    }
  }
  out
}

// Deterministic sorted key list for rendering / comparison.
pub fn sorted_keys(fields: Dict<Int, Int>) Vector<Int> {
  ks := fields.keys()
  ks.sort()
}
```

(If `Dict.has`/`Vector.sort` names differ in this tree, grep `boot/prelude/*.tw` and adjust — `d.keys()` and a numeric sort are both available; `sort()` may be `sorted()`.)

- [ ] **Step 4: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — codec round-trip/collision, `is_unique`, `graft`, `remove_prefix`, `project`, `merge`, `clear_all`.

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/field_facts.tw boot/tests/suites/cfg_field_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/field_facts.tw boot/tests/suites/cfg_field_facts_suite.tw boot/tests/main.tw
git commit -m "field_facts: path types + reversible PathKey codec + map ops

New leaf module for Phase 4 path-sensitive ownership: PathSeg/AccessPath, a
reversible arithmetic PathKey codec (no interned side table needed for the
Phase 4 path shapes), and pure map operations (graft/project/remove_prefix/
merge/clear_all/is_unique). Only Unique paths are stored; absence means no claim."
```

---

## Task 2: Thread `field_own` through the analysis (plumbing, no introduction)

Add `field_own` to `ForwardState`, enforce the downward-closed invariant at the `set_own_st` choke point, carry it on rebind, join it, and thread it through the fixpoint. **No introduction rule yet**, so `field_own` stays empty everywhere and every Phase 2/3 assertion is unchanged (G2). This proves the plumbing in isolation.

**Files:**
- Modify: `boot/compiler/ownership.tw`.
- Test: `boot/tests/suites/cfg_field_facts_suite.tw` (add an "empty degrades to today" guard).

- [ ] **Step 1: Write the failing test (plumbing guard)**

Add to `cfg_field_facts_suite.tw` (new imports + harness mirroring `cfg_ownership_facts_suite.tw`):

```tw
use compiler.anf.{AnfExpr, AnfFunctionDef, AnfModule, AnfOp}
use compiler.builtins
use compiler.cfg
use compiler.core_ir.{FuncId, LocalId}
use compiler.mono_type.{MonoType}
use compiler.opt.semantics as semantics
use compiler.opt.semantics.{make_prelude_optimizer_semantics}
use compiler.ownership

fn lid(id: Int) LocalId {
  LocalId.{ id }
}

fn b_reg() builtins.BuiltinRegistry {
  builtins.make_builtin_registry()
}

fn sem() semantics.OptimizerSemantics {
  make_prelude_optimizer_semantics(b_reg())
}

fn dict_new_call(b: builtins.BuiltinRegistry) AnfOp {
  .ACall(.AGlobalFunc(b.method_id("Dict", "new")), [])
}

fn module_of(name: String, body: AnfExpr) AnfModule {
  func: AnfFunctionDef = AnfFunctionDef.{
    func_id: FuncId.{ id: 1 },
    name,
    is_init: false,
    params: [],
    op_result_mono: Dict.new(),
    body,
    return_ty: MonoType.Int,
  }
  AnfModule.{
    functions: [func],
    init_func_id: .None,
    extern_imports: Dict.new(),
    global_monos: Dict.new(),
    lib_exports: [],
  }
}

fn analyzed_func(m: AnfModule) cfg.CfgFunction {
  b := b_reg()
  v := cfg.build_view(m, b)
  a := ownership.analyze(v, b, sem())
  case cfg.function_named(a, "f") {
    .Some(f) => f,
    .None => error("no f"),
  }
}

// exit field-fact count for a local in block 0.
fn field_count(f: cfg.CfgFunction, local_id: Int) Int {
  case f.blocks[0].exit.field_own.get(local_id) {
    .Some(m) => m.keys().len(),
    .None => 0,
  }
}
```

```tw
    .test("plumbing: a plain fresh dict has no field facts (empty degrades to today)", fn() {
      b := b_reg()
      // f() { d := Dict.new(); d }  -> d is Unique shell, NO field paths yet.
      body: AnfExpr = .Let(lid(0), dict_new_call(b), .Atom(.ALocal(lid(0))))
      f := analyzed_func(module_of("f", body))
      try assert.equal(field_count(f, 0), 0)
      .Ok({})
    })
```

- [ ] **Step 2: Run to verify it fails**

Expected: FAIL — `cfg.CfgFunction` / `BlockFacts` has no `field_own`, and `ForwardState` has no `field_own`. (This step also forces the Task 7 `BlockFacts.field_own` slot to exist; add the minimal `field_own: Dict<Int, Dict<Int,Int>>` field to `BlockFacts` and `empty_block_facts` now — its rendering is Task 7.)

- [ ] **Step 3: Add `field_own` to `BlockFacts` and `ForwardState`**

In `cfg.tw`, extend `BlockFacts` and `empty_block_facts`:

```tw
pub type BlockFacts = .{
  ownership: Dict<Int, Int>,
  binding_valid: Dict<Int, Bool>,
  live: Vector<Int>,                    // UNCHANGED — keep Vector<Int> (CFG/joins key on Int ids)
  field_own: Dict<Int, Dict<Int, Int>>, // NEW: local -> (PathKey -> tag); non-shell paths only
  verdicts: Dict<Int, String>,          // NEW: result local -> two-verdict string (Task 7)
}

fn empty_block_facts() BlockFacts {
  BlockFacts.{ ownership: Dict.new(), binding_valid: Dict.new(), live: [], field_own: Dict.new(), verdicts: Dict.new() }
}
```

**Do not change `live`'s type** — only append `field_own`/`verdicts`. Every `BlockFacts.{ … }` construction elsewhere (grep `BlockFacts.{`) must add the two new empty fields.

Extend the "un-analyzed" guard (`cfg.tw:829`-ish) to also require `field_own`/`verdicts` empty before treating a block as un-analyzed. (`cfg.tw` gains `use compiler.field_facts as ff` in Task 7, not here.)

In `ownership.tw`:

```tw
type ForwardState = .{
  own: Dict<Int, Int>,
  valid: Dict<Int, Bool>,
  prov: Dict<Int, Vector<Int>>,
  field_own: Dict<Int, Dict<Int, Int>>,
}
```

Grep every `ForwardState.{` construction and add `field_own: Dict.new()`. Add helpers:

```tw
fn field_own_get(st: ForwardState, id: Int) Dict<Int, Int> {
  case st.field_own.get(id) {
    .Some(m) => m,
    .None => Dict.new(),
  }
}

fn set_field_own(st: ForwardState, id: Int, m: Dict<Int, Int>) ForwardState {
  st.field_own[id] = m
  st
}

fn clear_field_own(st: ForwardState, id: Int) ForwardState {
  st.field_own[id] = Dict.new()
  st
}
```

- [ ] **Step 4: Enforce downward-closure at the `set_own_st` choke point**

```tw
fn set_own_st(st: ForwardState, id: Int, o: Ownership) ForwardState {
  st.own = set_own(st.own, id, o)
  // Downward-closed invariant: a non-Unique shell can carry no field facts.
  st = if own_tag(o) != 0 {
    st.clear_field_own(id)
  } else {
    st
  }
  st
}
```

- [ ] **Step 5: Carry `field_own` on rebind (`AAssign`)**

In `transfer_op`'s `.AAssign(local, a)` arm, after setting own/prov, carry the source's field facts iff the rebound shell is `Unique` (else `set_own_st` already cleared it):

```tw
    .AAssign(local, a) => {
      o := fact_of(st.own, a)
      st = .set_own_st(local.id, o)
      st = .set_prov_st(local.id, prov_of(st.prov, a))
      st = if own_tag(o) == 0 {
        st.set_field_own(local.id, atom_field_own(st, a))
      } else {
        st
      }
      st.set_valid(local.id, true)
    },
```

with:

```tw
fn atom_field_own(st: ForwardState, a: Atom) Dict<Int, Int> {
  case atom_local_id(a) {
    .Some(id) => field_own_get(st, id),
    .None => Dict.new(),
  }
}
```

- [ ] **Step 6: Join + fixpoint threading**

Add the `field_own` twin of `join_entry_ownership` (`ownership.tw:831`), using the **same two cases** and the **same skip-unprocessed** discipline, with `ff.merge` as the per-path meet, gated on the joined shell being `Unique`:

```tw
use compiler.field_facts as ff
```

```tw
fn field_own_map_get(m: Dict<Int, Dict<Int, Dict<Int, Int>>>, id: Int) Dict<Int, Dict<Int, Int>> {
  case m.get(id) {
    .Some(v) => v,
    .None => Dict.new(),
  }
}

fn field_of(fo: Dict<Int, Dict<Int, Int>>, id: Int) Dict<Int, Int> {
  case fo.get(id) {
    .Some(m) => m,
    .None => Dict.new(),
  }
}

fn field_of_atom(fo: Dict<Int, Dict<Int, Int>>, a: Atom) Dict<Int, Int> {
  case atom_local_id(a) {
    .Some(id) => field_of(fo, id),
    .None => Dict.new(),
  }
}

// entry.field_own per live-in local at blk: block param -> meet over positional
// edge args; live-through local -> meet over same-id. Only processed preds
// contribute. A path survives only if Unique on EVERY contributing pred. Cleared
// when the joined shell (entry_own) is non-Unique.
fn join_entry_field_own(
  blk: CfgBlock,
  exit_field: Dict<Int, Dict<Int, Dict<Int, Int>>>,
  entry_own: Dict<Int, Int>,
  processed: Dict<Int, Bool>,
) Dict<Int, Dict<Int, Int>> {
  entry: Dict<Int, Dict<Int, Int>> = Dict.new()
  for lid in blk.entry.live {
    pidx := param_index(blk, lid)
    acc: Dict<Int, Int>? = .None
    for pe in blk.preds {
      if is_processed(processed, pe.target.id) {
        fo := field_own_map_get(exit_field, pe.target.id)
        src := case pidx {
          .Some(i) => if i < pe.args.len() {
            field_of_atom(fo, pe.args[i])
          } else {
            Dict.new()
          },
          .None => field_of(fo, lid),
        }
        acc = case acc {
          .Some(cur) => .Some(ff.merge(cur, src)),
          .None => .Some(src),
        }
      }
    }
    case acc {
      .Some(m) => {
        // downward-closed: only keep when the joined shell is Unique.
        shell_unique := case entry_own.get(lid) {
          .Some(t) => t == 0,
          .None => false,
        }
        if shell_unique and m.keys().len() > 0 {
          entry[lid] = m
        }
      },
      .None => {},
    }
  }
  entry
}
```

Thread `exit_field_own` through the fixpoint exactly as Phase 3 threaded `exit_prov` (`ownership.tw` `run_fixpoint`/`FixResult`): extend `FixResult` with `exit_field_own: Dict<Int, Dict<Int, Dict<Int, Int>>>`; in each per-block step build `entry_field := join_entry_field_own(blk, exit_field, entry_own, processed)`, put it in the `ForwardState.{ … , field_own: entry_field }`; on change-detection compare with `same_field_own_map` (below) alongside own/valid/prov; store `next` (or the target-widened value — for `field_own` the conservative merge under G7 is `ff.merge(old, next)`, since meet is the sound widening). Add:

```tw
fn same_field_map(a: Dict<Int, Int>, b: Dict<Int, Int>) Bool {
  if a.keys().len() != b.keys().len() {
    return false
  }
  for k in a.keys() {
    if !b.has(k) {
      return false
    }
  }
  true
}

fn same_field_own_map(a: Dict<Int, Dict<Int, Int>>, b: Dict<Int, Dict<Int, Int>>) Bool {
  if a.keys().len() != b.keys().len() {
    return false
  }
  for k in a.keys() {
    case a.get(k) {
      .Some(av) => case b.get(k) {
        .Some(bv) => if !same_field_map(av, bv) {
          return false
        },
        .None => return false,
      },
      .None => {},
    }
  }
  true
}
```

Materialize `blk.exit.field_own` (and `blk.entry.field_own`) in `analyze_function`'s final pass the same way `ownership`/`prov` are materialized.

- [ ] **Step 7: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw`
Expected: PASS — the plumbing guard (empty field facts) **and** every Phase 2/3 facts/summary test (G2). If a Phase 3 test regresses, a `ForwardState.{ … }` site is missing `field_own: Dict.new()`, or the choke-point clear fired on a `Unique` set.

- [ ] **Step 8: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/cfg.tw boot/tests/suites/cfg_field_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/compiler/cfg.tw boot/tests/suites/cfg_field_facts_suite.tw
git commit -m "ownership: thread field_own side map (plumbing, no introduction)

Add field_own to ForwardState/BlockFacts, join it by the same param/live-through
+ skip-unprocessed cases as own (per-path meet via ff.merge, gated on a Unique
shell), thread it through the fixpoint, carry it on rebind, and enforce the
downward-closed invariant at the single set_own_st choke point. No introduction
rule yet, so field_own stays empty and all Phase 2/3 facts are unchanged."
```

---

## Task 3: Introduction for record/array ops (single-retention graft)

Introduce deep facts at `ARecord`/`AArrayLit`/`ARecordUpdate` under the single-retention proof (G4). This is the `push_scope`/Case B shape's construction side.

**Files:**
- Modify: `boot/compiler/ownership.tw` (`transfer_op` arms + a single-retention helper).
- Test: `boot/tests/suites/cfg_field_facts_suite.tw`.

- [ ] **Step 1: Write the failing tests**

Need an op to build a fresh, owned collection field and a record over it. Add helpers + tests:

```tw
fn field_atom(fid: Int, v: LocalId) anf.FieldAtom {
  // FieldAtom.{ field, value }
  .{ field: FieldId.{ id: fid }, value: .ALocal(v) }
}
```

(Import `compiler.anf` and `compiler.core_ir.{FieldId, TypeId}` as needed; `.ARecord(TypeId, Vector<FieldAtom>)`.)

```tw
    .test("intro record: fresh dict field -> shell has [.f]:Unique", fn() {
      b := b_reg()
      // f() { d := Dict.new(); r := Wrapper.{ f0: d } }  -> r has [.f0]:Unique.
      rec: AnfOp = .ARecord(TypeId.{ id: 0 }, [field_atom(0, lid(0))])
      body: AnfExpr = .Let(lid(0), dict_new_call(b), .Let(lid(1), rec, .Atom(.ALocal(lid(1)))))
      f := analyzed_func(module_of("f", body))
      try assert.is_true(has_field_path(f, 1, ff.field_path(0)))
      .Ok({})
    })
    .test("intro record negative: same value in two fields -> no deep claim", fn() {
      b := b_reg()
      // f() { d := Dict.new(); r := Two.{ a: d, b: d } }  -> neither field claimed.
      rec: AnfOp = .ARecord(TypeId.{ id: 0 }, [field_atom(0, lid(0)), field_atom(1, lid(0))])
      body: AnfExpr = .Let(lid(0), dict_new_call(b), .Let(lid(1), rec, .Atom(.ALocal(lid(1)))))
      f := analyzed_func(module_of("f", body))
      try assert.is_false(has_field_path(f, 1, ff.field_path(0)))
      try assert.is_false(has_field_path(f, 1, ff.field_path(1)))
      .Ok({})
    })
    .test("intro record negative: shared field value -> no [.f] claim", fn() {
      b := b_reg()
      // f() { d := Dict.new(); keep := d (alias); r := Wrapper.{ f0: d } }
      // d is aliased by keep, so it is NOT single-retention -> no claim.
      inner: AnfExpr = .Let(
        lid(2),
        .ARecord(TypeId.{ id: 0 }, [field_atom(0, lid(0))]),
        .Atom(.ALocal(lid(1))), // return keep so d has a live alias at the record
      )
      body: AnfExpr = .Let(lid(0), dict_new_call(b), .Let(lid(1), .AInit(.ALocal(lid(0))), inner))
      f := analyzed_func(module_of("f", body))
      try assert.is_false(has_field_path(f, 2, ff.field_path(0)))
      .Ok({})
    })
    .test("intro update: refresh .types keeps sibling .values facts", fn() {
      b := b_reg()
      // f() { t := Dict.new(); v := Dict.new(); r := Ctx.{ types: t, values: v };
      //       t2 := Dict.new(); r2 := (r.types = t2) }  -> r2 has [.types] and [.values].
      mk: AnfOp = .ARecord(TypeId.{ id: 0 }, [field_atom(0, lid(0)), field_atom(1, lid(1))])
      upd: AnfOp = .ARecordUpdate(.ALocal(lid(2)), FieldId.{ id: 0 }, .ALocal(lid(3)), false, TypeId.{ id: 0 })
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(
          lid(1),
          dict_new_call(b),
          .Let(lid(2), mk, .Let(lid(3), dict_new_call(b), .Let(lid(4), upd, .Atom(.ALocal(lid(4)))))),
        ),
      )
      f := analyzed_func(module_of("f", body))
      try assert.is_true(has_field_path(f, 4, ff.field_path(0)))  // refreshed .types
      try assert.is_true(has_field_path(f, 4, ff.field_path(1)))  // carried .values
      .Ok({})
    })
```

with the helper:

```tw
fn has_field_path(f: cfg.CfgFunction, local_id: Int, p: ff.AccessPath) Bool {
  case f.blocks[0].exit.field_own.get(local_id) {
    .Some(m) => ff.is_unique(m, p),
    .None => false,
  }
}
```

(`ARecordUpdate(Atom, FieldId, Atom, Bool, TypeId)` — five positions: **base, field, value, `Bool`, `TypeId`** (`anf.tw:50`). The 4th is a `Bool` (pass `false`), the 5th is a `TypeId` (not `MonoType`). `ARecordGet(Atom, FieldId, TypeId)` — three positions, 3rd is a `TypeId` (`anf.tw:49`). `FieldId` comes from `compiler.core_ir`, `TypeId` from `compiler.mono_type` (`anf.tw:7-11`); import both in the suite.)

- [ ] **Step 2: Run to verify they fail**

Expected: FAIL — no introduction yet, so `has_field_path` is false for the positive cases.

- [ ] **Step 3: Single-retention helper**

```tw
// A value operand is single-retention at this op iff it is a local that is
// []:Unique, at last-use, and appears exactly once among `operands`.
fn store_count(operands: Vector<Atom>, id: Int) Int {
  n := 0
  for a in operands {
    case atom_local_id(a) {
      .Some(oid) => if oid == id {
        n = n + 1
      },
      .None => {},
    }
  }
  n
}

fn single_retention(st: ForwardState, v: Atom, last: Vector<Int>, operands: Vector<Atom>) Bool {
  case atom_local_id(v) {
    .Some(id) => own_tag2(fact_of(st.own, v)) == 0 and is_last_use(last, id) and store_count(operands, id) == 1,
    .None => false,
  }
}

fn own_tag2(o: Ownership) Int {
  own_tag(o)
}
```

- [ ] **Step 4: Introduction in `transfer_op`**

Extend the `ARecord`/`AArrayLit`/`ARecordUpdate` arms (keep the existing own/prov/`field_store` logic; **add** the field-fact side). `field_store` still runs for the move/alias demotion of the shell source; the graft is the additional deep claim.

```tw
    .ARecord(_, fields) => {
      origins: Vector<Int> = []
      operands: Vector<Atom> = collect fa in fields {
        fa.value
      }
      for fa in fields {
        origins = union_sorted(origins, prov_of(st.prov, fa.value))
        st = .field_store(fa.value, last)
      }
      st = .set_result(result, .Unique) // sets shell Unique; choke point leaves field_own[result] intact (empty)
      st = .set_prov_st(result, origins)
      // Graft each single-retention field value's facts under [.f].
      rf: Dict<Int, Int> = Dict.new()
      for fa in fields {
        if single_retention(st, fa.value, last, operands) {
          rf = ff.graft(rf, .Field(fa.field.id), 0, atom_field_own(st, fa.value))
        }
      }
      st.set_field_own(result, rf)
    },
    .AArrayLit(elems) => {
      origins: Vector<Int> = []
      for a in elems {
        origins = union_sorted(origins, prov_of(st.prov, a))
        st = .field_store(a, last)
      }
      st = .set_result(result, .Unique)
      st = .set_prov_st(result, origins)
      // [Elem]:Unique when EVERY element is single-retention. An EMPTY literal is
      // [Elem]:Unique vacuously (no shared inner) — this bootstraps build-from-empty
      // vectors so the Task 4 consuming rule has an all-owned base to preserve.
      all_owned := true
      for a in elems {
        if !single_retention(st, a, last, elems) {
          all_owned = false
        }
      }
      st = if all_owned {
        st.set_field_own(result, ff.set_path(Dict.new(), ff.elem_path(), 0))
      } else {
        st
      }
      st
    },
    .ARecordUpdate(base, f, v, _, _) => {
      origins := union_sorted(prov_of(st.prov, base), prov_of(st.prov, v))
      base_fields := atom_field_own(st, base)
      st = .consume_base(result, base, last) // sets result shell Unique (if base Unique+last-use) else Unknown
      st = .field_store(v, last)
      st = .set_prov_st(result, origins)
      // Only build field facts when the result shell is Unique (else choke point cleared it).
      st = if own_is_unique(st.own, result) {
        // remove the whole [.f]* subtree first, then graft the (single-retention) replacement.
        rf := ff.remove_prefix(base_fields, .Field(f.id))
        rf = if single_retention(st, v, last, [v]) {
          ff.graft(rf, .Field(f.id), 0, atom_field_own(st, v))
        } else {
          rf
        }
        st.set_field_own(result, rf)
      } else {
        st
      }
      st
    },
```

`own_is_unique(own, id)` wraps the existing `fact_of_local(st.own, id) == .Unique` (`fact_of_local` is at `ownership.tw:805`); add it once and reuse it here and in Task 4:

```tw
fn own_is_unique(own: Dict<Int, Int>, id: Int) Bool {
  case fact_of_local(own, id) {
    .Unique => true,
    _ => false,
  }
}
```

(Here `v` is the *replacement value* — a single operand — so `single_retention(st, v, last, [v])` is correct: there is no sibling operand to alias against in an `ARecordUpdate`. Contrast Task 4, where a `Dict.set` retains **two** operands and the list must include both.)

- [ ] **Step 5: Run tests to verify they pass**

Expected: PASS — record `[.f0]`; duplicate/aliased negatives claim nothing; update refreshes `.types` and carries `.values`. Phase 2/3 green (G2).

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_field_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_field_facts_suite.tw
git commit -m "ownership: single-retention field introduction (record/array/update)

ARecord/AArrayLit graft [.f]/[Elem]:Unique only for a value that is Unique, at
last-use, and stored into exactly one slot; duplicate operands and aliased
values claim nothing. ARecordUpdate removes the whole [.f]* subtree then grafts
the single-retention replacement, carrying sibling field facts unchanged."
```

---

## Task 4: Collection-builtin nested facts (`transfer_builtin_call`)

Handle the consuming collection builtins: `Vector.make` never claims `[Elem]`; the element-store ops keep `[Elem]`/`[Val]:Unique` only when the stored element is itself single-retention (Decision 5). This is Case V's inner-append shape.

**Verified ids (`boot/compiler/opt/semantics.tw:84-160`)** — the element-store builtins are **not** `method_id`s:
- Vector element store: `b.id("vector$set_unsafe")` — `.Update`, `cow_base_arg .Some(0)`, `retained_args .Some([2])` (value).
- Vector append/push: `vector_builder_config(b).push_id` — `.Update`, `cow_base_arg .Some(0)`, `retained_args .Some([1])` (value).
- Dict set: `b.method_id("Dict","set")` — `.Update`, `retained_args .Some([1, 2])` (**key and value**).
- `Vector.make`: `b.method_id("Vector","make")` — `.Allocate`, `retained_args .Some([1])` (fill).

So the container signal lives on the **optimizer semantics** (populated where those ids are in scope), not a `method_id` compare in `ownership.tw`.

**Scope note (empty-collection bootstrap):** this rule *preserves/keeps* `[Elem]`/`[Val]` on an already-all-owned base and *drops* it on a shared insert — it does **not mint** a nested fact on an empty base. A base therefore gets `[Elem]` only from an all-owned array literal (Task 3), so build-by-insertion dicts (`Dict.new` + `set`, Case B) never accumulate `[Val]` under these strict rules. Whether empty collections should mint a **vacuous** `[Elem]`/`[Val]:Unique` (sound: an empty collection has no shared inner) to bootstrap the inductive case is an **open refinement** — confirm against `records-fields.md` Case B before adding it; leaving it out is a sound under-claim. The Task 4 fixtures use the array-literal bootstrap (vectors) so they are constructible without it.

**Files:**
- Modify: `boot/compiler/opt/semantics.tw` (`ContainerKind` enum + `container_kinds` map on `OptimizerSemantics` + `container_kind` accessor).
- Modify: `boot/compiler/ownership.tw` (`transfer_call`/`transfer_builtin_call` thread the callee `FuncId` + `sem`; `container_seg`; the `.Update` nested rule).
- Test: `boot/tests/suites/cfg_field_facts_suite.tw`.

- [ ] **Step 1: Write the failing tests**

```tw
// Vector element store: vector$set_unsafe(base, idx, v) -> Update, retained [2].
fn vec_set_call(b: builtins.BuiltinRegistry, base: LocalId, idx: Int, v: LocalId) AnfOp {
  .ACall(.AGlobalFunc(b.id("vector$set_unsafe")), [.ALocal(base), .ALitInt(idx), .ALocal(v)])
}

// Vector.make(3, fill) -> Allocate, retained [1]. Size is an inline literal atom.
fn vec_make_call(b: builtins.BuiltinRegistry, fill: LocalId) AnfOp {
  .ACall(.AGlobalFunc(b.method_id("Vector", "make")), [.ALitInt(3), .ALocal(fill)])
}
```

```tw
    .test("consume keep: storing an owned inner into an all-owned vector keeps [Elem]", fn() {
      b := b_reg()
      // f() { d := Dict.new(); a := [d] (all-owned literal -> [Elem]);
      //       x := Dict.new(); r := vector$set_unsafe(a, 0, x) }  -> r keeps [Elem].
      arr: AnfOp = .AArrayLit([.ALocal(lid(0))])
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), arr, .Let(lid(2), dict_new_call(b), .Let(lid(3), vec_set_call(b, lid(1), 0, lid(2)), .Atom(.ALocal(lid(3)))))),
      )
      f := analyzed_func(module_of("f", body))
      try assert.is_true(has_field_path(f, 3, ff.elem_path()))
      .Ok({})
    })
    .test("consume drop: storing a SHARED inner drops [Elem]", fn() {
      b := b_reg()
      // x is aliased (kept live at the return), so storing it is not single-retention.
      arr: AnfOp = .AArrayLit([.ALocal(lid(0))])
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(
          lid(1),
          arr,
          .Let(
            lid(2),
            dict_new_call(b),
            .Let(
              lid(3),
              .AInit(.ALocal(lid(2))),           // alias of x (keeps x live)
              .Let(lid(4), vec_set_call(b, lid(1), 0, lid(2)), .Atom(.ALocal(lid(3)))),
            ),
          ),
        ),
      )
      f := analyzed_func(module_of("f", body))
      try assert.is_false(has_field_path(f, 4, ff.elem_path()))
      .Ok({})
    })
    .test("Vector.make never claims [Elem] (replicated reference)", fn() {
      b := b_reg()
      // f() { x := Dict.new(); r := Vector.make(3, x) } -> NO [Elem].
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), vec_make_call(b, lid(0)), .Atom(.ALocal(lid(1)))),
      )
      f := analyzed_func(module_of("f", body))
      try assert.is_false(has_field_path(f, 1, ff.elem_path()))
      .Ok({})
    })
```

(`b.id("vector$set_unsafe")` returns the raw prelude `FuncId`; confirm the exact name against `semantics.tw:91`. If the boot `BuiltinRegistry` exposes `id` under a different name, grep `pub fn id(` in `boot/compiler/builtins.tw`.)

- [ ] **Step 2: Run to verify they fail**

Expected: FAIL — the keep case has no `[Elem]` (no consume rule yet); the drop/`make` negatives already pass (no rule adds anything) — fine, they lock the sound default.

- [ ] **Step 3: `ContainerKind` on optimizer semantics**

In `boot/compiler/opt/semantics.tw`, add the enum, a `container_kinds` map on `OptimizerSemantics`, populate it in `make_prelude_optimizer_semantics` (where `b` and `builder := vector_builder_config(b)` are already in scope), and a total accessor:

```tw
pub type ContainerKind = { NotCollection, VectorLike, DictLike }
```

Add `container_kinds: Dict<Int, Int>` to the `OptimizerSemantics` record (tag: `1` vector, `2` dict; absent => `NotCollection`). In `make_prelude_optimizer_semantics`, alongside the `calls[...]` assignments:

```tw
  container_kinds: Dict<Int, Int> = Dict.new()
  container_kinds[b.id("vector$set_unsafe").id] = 1
  container_kinds[builder.push_id.id] = 1          // vector append/push
  container_kinds[b.method_id("Dict", "set").id] = 2
```

Add `container_kinds` to the returned `OptimizerSemantics.{ … }`. (Confirm `builder.push_id` is the vector-append id; grep `vector_builder_config` in `boot/compiler/builder_family.tw`. If append is exposed elsewhere, key that id instead. Do **not** key `Dict.remove`/builder-internal pushes — leaving them `NotCollection` drops nested facts, which is sound.) Then:

```tw
pub fn container_kind(sem: OptimizerSemantics, fid: FuncId) ContainerKind {
  case sem.container_kinds.get(fid.id) {
    .Some(1) => .VectorLike,
    .Some(2) => .DictLike,
    _ => .NotCollection,
  }
}
```

- [ ] **Step 4: Thread `fid`/`sem` + the consume rule in `ownership.tw`**

`transfer_call` already has `fid` (in the `.Some(fid)` arm) and `sem`; pass both to `transfer_builtin_call`:

```tw
      .Some(cs) => st.transfer_builtin_call(result, fid, cs, args, last, sem),
```

`transfer_builtin_call` gains `fid: FuncId` and `sem: OptimizerSemantics` params. Map the container kind to a segment:

```tw
// `ff.PathSeg` is qualified: ownership.tw has `use compiler.field_facts as ff`,
// which binds the module alias but not the type unqualified. Either annotate
// `ff.PathSeg?` (shown) or add `use compiler.field_facts.{PathSeg}`. The `.Elem`/
// `.Val` variant literals resolve contextually from the `ff.PathSeg?` return type.
fn container_seg(sem: OptimizerSemantics, fid: FuncId) ff.PathSeg? {
  case semantics.container_kind(sem, fid) {
    .VectorLike => .Some(.Elem),
    .DictLike => .Some(.Val),
    .NotCollection => .None,
  }
}
```

The single-retention check for the stored element must count the candidate across **all retained operands** (Dict.set retains key+value — a value reused as the key aliases itself):

```tw
fn retained_atoms(cs: CallSemantics, args: Vector<Atom>) Vector<Atom> {
  out: Vector<Atom> = []
  for i in retained_arg_indices(cs, args) {
    if i < args.len() {
      out = .append(args[i])
    }
  }
  out
}

// The stored VALUE is the last retained operand (append [1] -> arg1;
// vector$set_unsafe [2] -> arg2; Dict.set [1,2] -> arg2).
fn stored_element_atom(cs: CallSemantics, args: Vector<Atom>) Atom? {
  ri := retained_arg_indices(cs, args)
  if ri.len() == 0 {
    return .None
  }
  k := ri[ri.len() - 1]
  if k < args.len() {
    .Some(args[k])
  } else {
    .None
  }
}
```

Extend only the `.Update` branch (`.Allocate` is untouched — `Vector.make` adds no field claim, giving never-`[Elem]` for free):

```tw
    .Update => {
      // Capture base facts BEFORE consume (consume leaves base.field_own intact).
      base_fields := case cs.cow_base_arg {
        .Some(k) => if k < args.len() {
          atom_field_own(st, args[k])
        } else {
          Dict.new()
        },
        .None => Dict.new(),
      }
      st = st
        .consume_call_base(result, cs, args, last)
        .carry_base_prov(result, cs, args)
        .absorb_retained_call_args(result, args, retained_arg_indices(cs, args), last)
      // Nested facts survive only when the result shell is Unique.
      st = if own_is_unique(st.own, result) {
        rf := case container_seg(sem, fid) {
          .Some(seg) => {
            // A known collection op: untouched elements are structurally
            // preserved (carry base's [seg]*), then keep or drop the touched seg.
            carried := base_fields
            keep := case stored_element_atom(cs, args) {
              .Some(x) => single_retention(st, x, last, retained_atoms(cs, args)),
              .None => false,
            }
            if keep {
              carried
            } else {
              ff.remove_prefix(carried, seg)
            }
          },
          // Unknown Update op: we do NOT know it preserves inner structure -> drop
          // every nested fact (sound). Shell stays whatever consume_call_base set.
          .None => Dict.new(),
        }
        st.set_field_own(result, rf)
      } else {
        st
      }
      st
    },
```

- [ ] **Step 5: Run tests to verify they pass**

Expected: PASS — owned store keeps `[Elem]`; shared store and `Vector.make` claim nothing. Phase 2/3 green.

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/opt/semantics.tw boot/compiler/ownership.tw boot/tests/suites/cfg_field_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/opt/semantics.tw boot/compiler/ownership.tw boot/tests/suites/cfg_field_facts_suite.tw
git commit -m "ownership: nested-collection facts for consuming builtins

vector$set_unsafe/vector append/Dict.set keep [Elem]/[Val]:Unique only when the
stored element is single-retention across all retained operands (so Dict.set(d,x,x)
does not mint [Val]); a shared or aliased element drops the nested path. Unknown
Update ops drop nested facts. Vector.make never claims [Elem]. Container kind
(Vector/Dict) is carried on the optimizer semantics, keyed by the real builtin
ids (vector\$set_unsafe / builder push / Dict.set), not method_id compares."
```

---

## Task 5: Projection — `ARecordGet` rebase + whole-record last-use move / borrow

Make `ARecordGet(base, f)` move the field's facts to R (rebase `[.f]* → []*`) at whole-record last-use, or **borrow** (demote both) otherwise. No quartet yet (Task 6).

**Files:**
- Modify: `boot/compiler/ownership.tw` (`transfer_op` `.ARecordGet`).
- Test: `boot/tests/suites/cfg_field_facts_suite.tw`.

- [ ] **Step 1: Write the failing tests**

```tw
fn record_get(base: LocalId, fid: Int) AnfOp {
  .ARecordGet(.ALocal(base), FieldId.{ id: fid }, TypeId.{ id: 0 })
}

fn is_shell_unique(f: cfg.CfgFunction, local_id: Int) Bool {
  case f.blocks[0].exit.ownership.get(local_id) {
    .Some(t) => t == 0,
    .None => false,
  }
}
```

```tw
    .test("projection move: last-use base, deeply-owned field -> R Unique + base [.f] removed", fn() {
      b := b_reg()
      // f() { d := Dict.new(); r := Wrapper.{ f0: d }; g := r.f0 }  (r dead after get)
      rec: AnfOp = .ARecord(TypeId.{ id: 0 }, [field_atom(0, lid(0))])
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), rec, .Let(lid(2), record_get(lid(1), 0), .Atom(.ALocal(lid(2))))),
      )
      f := analyzed_func(module_of("f", body))
      try assert.is_true(is_shell_unique(f, 2))            // R moved out Unique
      try assert.is_false(has_field_path(f, 1, ff.field_path(0)))  // base's [.f0] removed
      .Ok({})
    })
    .test("projection borrow: base still live -> R Shared and base [.f] cleared", fn() {
      b := b_reg()
      // f() { d := Dict.new(); r := Wrapper.{ f0: d }; g := r.f0; r }  (r live at return)
      rec: AnfOp = .ARecord(TypeId.{ id: 0 }, [field_atom(0, lid(0))])
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), rec, .Let(lid(2), record_get(lid(1), 0), .Atom(.ALocal(lid(1))))),
      )
      f := analyzed_func(module_of("f", body))
      try assert.is_false(is_shell_unique(f, 2))           // R borrowed -> not Unique
      try assert.is_false(has_field_path(f, 1, ff.field_path(0)))  // base [.f0] cleared too
      .Ok({})
    })
```

- [ ] **Step 2: Run to verify they fail**

Expected: FAIL — current `.ARecordGet` always sets `Unknown` and never touches field facts, so the move case's R is not Unique and base's `[.f0]` is not removed.

- [ ] **Step 3: Implement projection (move + borrow)**

Replace the `.ARecordGet(base, _, _)` arm (note it must now read `f`):

```tw
    .ARecordGet(base, f, _) => {
      pr := ff.project(atom_field_own(st, base), .Field(f.id))
      moved := case atom_local_id(base) {
        .Some(bid) => is_last_use(last, bid), // whole-record last-use (quartet: Task 6)
        .None => false,
      }
      st = if moved {
        case pr.shell {
          .Some(t) => {
            // move: R takes the projected shell + strict-descendant facts;
            // remove base's [.f]* subtree.
            st = .set_own_st(result, own_of_tag(t))
            st = .set_field_own(result, pr.fields)
            case atom_local_id(base) {
              .Some(bid) => st.set_field_own(bid, ff.remove_prefix(field_own_get(st, bid), .Field(f.id))),
              .None => st,
            }
          },
          .None => st.set_result(result, .Unknown), // field not deeply owned -> plain borrow, no claim
        }
      } else {
        // borrow: aliasing both sides -> R Shared, base's [.f]* cleared.
        st = .set_own_st(result, .Shared)
        case atom_local_id(base) {
          .Some(bid) => st.set_field_own(bid, ff.remove_prefix(field_own_get(st, bid), .Field(f.id))),
          .None => st,
        }
      }
      // R's prov still follows base's origins (unchanged from Phase 2/3).
      st.set_prov_st(result, prov_of(st.prov, base))
    },
```

(`own_of_tag` already exists at `ownership.tw:29`. The `set_own_st(result, .Shared)` in the borrow branch also clears any `field_own[result]` via the choke point — correct.)

- [ ] **Step 4: Run tests to verify they pass**

Expected: PASS — move: R Unique, base `[.f0]` gone; borrow: R not Unique, base `[.f0]` cleared. Phase 2/3 green.

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_field_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_field_facts_suite.tw
git commit -m "ownership: field projection move/borrow at ARecordGet

Whole-record last-use projection MOVES base.[.f]* to R (rebased to []*) and
removes the subtree from base; a live base BORROWS -> R Shared and base's [.f]*
cleared (both demote). Field-not-owned projections stay a plain borrow."
```

---

## Task 6: Quartet shell-writeback recognizer + move

Recognize the `get→consume→update` quartet as a **block-local linear-ANF pre-pass**, annotate the `ARecordGet`, and let projection move the field even when `base` is not whole-record-dead (G5). **The write-back binds a fresh local** (`env2 := env.types = t2`), not an `AAssign` of `env` — the essential proof is that `base`'s *only* remaining use is the matching `ARecordUpdate` and `base` is **dead after the block** (not live-out), so `R` mutating the field backing in place cannot corrupt a surviving view of `base`.

**Files:**
- Modify: `boot/compiler/ownership.tw` (recognizer + a per-op `quartet_move` set, consulted in `.ARecordGet`).
- Test: `boot/tests/suites/cfg_field_facts_suite.tw`.

- [ ] **Step 1: Write the failing test (the push_scope idiom)**

```tw
    .test("quartet move: get -> Dict.set -> update licenses field in-place", fn() {
      b := b_reg()
      // f() {
      //   d := Dict.new(); env := Ctx.{ types: d };
      //   t := env.types;                 // projection (env still live -> update below)
      //   t2 := Dict.set(t, 1, 2);        // consume/produce
      //   env2 := (env.types = t2);       // record_update base=env, field types (env dead after)
      //   env2                            // returns the fresh binding; env NOT live-out
      // }
      mk: AnfOp = .ARecord(TypeId.{ id: 0 }, [field_atom(0, lid(0))])
      set2: AnfOp = .ACall(.AGlobalFunc(b.method_id("Dict", "set")), [.ALocal(lid(2)), .ALitInt(1), .ALitInt(2)])
      upd: AnfOp = .ARecordUpdate(.ALocal(lid(1)), FieldId.{ id: 0 }, .ALocal(lid(3)), false, TypeId.{ id: 0 })
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(
          lid(1),
          mk,
          .Let(
            lid(2),
            record_get(lid(1), 0),
            .Let(lid(3), set2, .Let(lid(4), upd, .Atom(.ALocal(lid(4))))),
          ),
        ),
      )
      f := analyzed_func(module_of("f", body))
      // The projected t (L2) is moved Unique so the Dict.set can (later) go in place,
      // and env2 retains [.types]:Unique.
      try assert.is_true(is_shell_unique(f, 2))
      try assert.is_true(has_field_path(f, 4, ff.field_path(0)))
      .Ok({})
    })
```

- [ ] **Step 2: Run to verify it fails**

Expected: FAIL — `env` is live at the `record_get` (used by the later `record_update`), so Task 5's whole-record last-use test classifies the projection as a **borrow**: `t` is not Unique and `[.types]` is cleared.

- [ ] **Step 3: Block-local quartet recognizer**

Add a pre-pass over a block's linear instruction list that finds each quartet and records the `ARecordGet`'s result local as a licensed move. Run it in `forward_block_body` (or in the block setup) and pass the resulting set into the transfer.

```tw
// A set of ARecordGet result-locals whose projection is licensed to MOVE by the
// quartet shell-writeback proof, even though base is not whole-record-dead.
// Recognized only within a single block's straight-line ANF.
fn recognize_quartet_moves(blk: CfgBlock) Dict<Int, Bool> {
  moves: Dict<Int, Bool> = Dict.new()
  insts := blk.instructions
  for inst, i in insts {
    case inst.op {
      .ARecordGet(base, f, _) => case atom_local_id(base) {
        .Some(bid) => {
          rget := inst.anf_local.id
          if quartet_ok(blk, i, bid, f.id, rget) {
            moves[rget] = true
          }
        },
        .None => {},
      },
      _ => {},
    }
  }
  moves
}

// True iff, strictly after the projection at index `i` in this block:
//  - there is EXACTLY ONE ARecordUpdate(base=bid, field=fid, v=_) (the write-back),
//    and its replacement `v` is not `bid` itself;
//  - `bid` is mentioned by NO other instruction (not published, passed to a call,
//    stored, aliased, or read for another field);
//  - `bid.fid` is not read again (no second ARecordGet(bid, fid));
//  - `bid` does NOT escape the block: it is not in the terminator payload or any
//    outgoing edge arg (so `bid` is dead after the block — R's in-place mutation of
//    the field backing cannot corrupt a surviving view of `bid`).
// The write-back's result binds a fresh local (the idiom `env2 := env.f = v`); no
// AAssign of `bid` is required. Any violation -> false (fall back to borrow).
fn quartet_ok(blk: CfgBlock, i: Int, bid: Int, fid: Int, rget: Int) Bool {
  insts := blk.instructions
  matched := false
  for j in range(i + 1, insts.len()) {
    op := insts[j].op
    is_match := case op {
      .ARecordUpdate(base2, f2, _v, _, _) => atom_is_local(base2, bid) and f2.id == fid,
      _ => false,
    }
    if is_match {
      if matched {
        return false // a second matching update -> ambiguous, fail
      }
      matched = true
      // the write-back legitimately uses bid as base; reject if it ALSO uses bid
      // as the replacement value (self-insert).
      case op {
        .ARecordUpdate(_, _, v, _, _) => if atom_is_local(v, bid) {
          return false
        },
        _ => {},
      }
    } else {
      // a second read of bid.fid, or ANY other mention of bid, fails the proof.
      if is_record_get_of(op, bid, fid) {
        return false
      }
      if op_mentions_local(op, bid) {
        return false
      }
    }
  }
  matched and !exit_mentions_local(blk, bid)
}
```

Provide the small predicates by **reusing** the existing use extraction — the code around `ownership.tw:95` already builds a per-op use list via `push_uid` (it covers `ARecordGet`/`ARecordUpdate` bases). Expose it as `op_uses(op) Vector<Int>` and reuse it; do **not** hand-roll a second use list, or the proof can miss a hidden use of `bid` and unsoundly move (execution risk below).

```tw
fn op_mentions_local(op: AnfOp, id: Int) Bool {
  for u in op_uses(op) {
    if u == id {
      return true
    }
  }
  false
}

fn atom_is_local(a: Atom, id: Int) Bool {
  case atom_local_id(a) {
    .Some(x) => x == id,
    .None => false,
  }
}

fn is_record_get_of(op: AnfOp, bid: Int, fid: Int) Bool {
  case op {
    .ARecordGet(base, f, _) => atom_is_local(base, bid) and f.id == fid,
    _ => false,
  }
}

// bid must be dead after the block: not returned/broken and not fed on any
// outgoing edge. Grep the real Terminator variants (ownership.tw uses
// .Return(.Some(a)) / .ValueBreak(a)) and the CfgEdge arg accessor; adjust names.
fn exit_mentions_local(blk: CfgBlock, id: Int) Bool {
  term := case blk.terminator {
    .Some(.Return(.Some(a))) => atom_is_local(a, id),
    .Some(.ValueBreak(a)) => atom_is_local(a, id),
    _ => false,
  }
  if term {
    return true
  }
  for e in blk.succs {
    for a in e.args {
      if atom_is_local(a, id) {
        return true
      }
    }
  }
  false
}
```

(`blk.succs`/`CfgEdge.args` are the outgoing edges used by `join_entry_ownership`; confirm the field names in `cfg.tw`. If the CFG stores successor edge args differently, use whatever `join_entry_ownership` reads for positional edge args — the invariant is "`bid` is not fed to any successor param".)

- [ ] **Step 4: Consult the quartet set in `.ARecordGet`**

Thread the `moves` set from `forward_block_body` into `transfer_op` (add a `quartet: Dict<Int,Bool>` param, defaulting empty for the non-block callers), and widen the `moved` test:

```tw
      moved := case atom_local_id(base) {
        .Some(bid) => is_last_use(last, bid) or quartet_has(quartet, result),
        .None => false,
      }
```

```tw
fn quartet_has(q: Dict<Int, Bool>, id: Int) Bool {
  case q.get(id) {
    .Some(v) => v,
    .None => false,
  }
}
```

In `forward_block_body`, compute `moves := recognize_quartet_moves(blk)` once and pass it to each `transfer_op`.

- [ ] **Step 5: Run tests to verify they pass**

Expected: PASS — the quartet licenses the projected `t` as Unique and `env2` keeps `[.types]:Unique`. Task 5's borrow test (a live `base` with **no** matching write-back) still borrows. Phase 2/3 green.

Also add a **negative**: a `record_get` whose base is later published (e.g. returned as-is or passed to `Dict.new`-unrelated call) must NOT be recognized as a quartet — assert `is_shell_unique(f, <rget>)` is false.

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_field_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_field_facts_suite.tw
git commit -m "ownership: block-local quartet shell-writeback projection move

Recognize the get->consume->update->rebind quartet over a single block's linear
ANF and license the projection to move base.[.f]* into R even when base is not
whole-record-dead, provided base is used for nothing but the matching write-back
and base.f is not re-read. Cross-block or any other use falls back to borrow."
```

---

## Task 7: Two-verdict output + field-fact rendering

Render the field facts and the per-`ARecordUpdate`/quartet two-verdict line in `twk ir --cfg`, keyed and sorted deterministically.

**Files:**
- Modify: `boot/compiler/ownership.tw` (verdict accumulation in the materialize pass), `boot/compiler/cfg.tw` (`render_facts` prints field facts + verdicts; `use compiler.field_facts`).
- Test: `boot/tests/suites/cfg_field_facts_suite.tw` + a CLI determinism check.

- [ ] **Step 1: Write the failing test (render shape + determinism)**

```tw
    .test("render: field facts + two-verdict line appear and are deterministic", fn() {
      b := b_reg()
      // The push_scope quartet from Task 6.
      mk: AnfOp = .ARecord(TypeId.{ id: 0 }, [field_atom(0, lid(0))])
      set2: AnfOp = .ACall(.AGlobalFunc(b.method_id("Dict", "set")), [.ALocal(lid(2)), .ALitInt(1), .ALitInt(2)])
      upd: AnfOp = .ARecordUpdate(.ALocal(lid(1)), FieldId.{ id: 0 }, .ALocal(lid(3)), false, TypeId.{ id: 0 })
      body: AnfExpr = .Let(
        lid(0),
        dict_new_call(b),
        .Let(lid(1), mk, .Let(lid(2), record_get(lid(1), 0), .Let(lid(3), set2, .Let(lid(4), upd, .Atom(.ALocal(lid(4))))))),
      )
      m := module_of("f", body)
      bb := b_reg()
      v := cfg.build_view(m, bb)
      a1 := ownership.analyze(v, bb, sem())
      a2 := ownership.analyze(v, bb, sem())
      out1 := cfg.render_view(a1)
      out2 := cfg.render_view(a2)
      try assert.is_true(out1.contains("field_facts="))
      try assert.is_true(out1.contains("field=in-place"))
      try assert.is_true(out1 == out2)
      .Ok({})
    })
```

- [ ] **Step 2: Run to verify it fails**

Expected: FAIL — `render_facts` prints only ownership tags; no `field_facts=` / verdict line yet.

- [ ] **Step 3: Accumulate verdicts in the materialize pass**

In `analyze_function`'s final materialize pass (the one that fills `blk.exit`/`blk.entry`), after replaying `forward_block_body` for a block, walk its instructions once with the final entry state to compute per-site verdicts and store them in `blk.exit.verdicts` (result local → string). For an `ARecordUpdate(base, f, v)` (and the matching quartet `ARecordGet`), emit two orthogonal verdicts from the *pre-op* state:

- **shell** = `reuse(unique)` when `base` is `[]:Unique`+last-use (the `consume_base` precondition), else `persistent(<reason>)` (`aliased shell` / `base still live`).
- **field** = `in-place([.f] unique)` when the projected field / result carries `[.f]:Unique`, else `persistent(<reason>)` (`outer owned but inner shared`, `insufficient deep ownership`, `borrow-projection: base still live`).

Store as e.g. `L4 = record_update L1.f0  shell=reuse(unique) field=in-place([.f0] unique)`. Keep the reason strings exactly those the design's Output section lists (records-fields.md hard requirement: **never a silent bail**). This is a read-only derivation over final facts — no new fixpoint.

- [ ] **Step 4: Render field facts + verdicts in `cfg.tw`**

Add `use compiler.field_facts as ff` to `cfg.tw` (leaf import, G6). In `render_facts`, after the ownership line, append (only when non-empty), sorted by `(LocalId, PathKey)`:

```tw
// field_facts={L7:[.types]=U,[.types,Elem]=U; L9:[Elem]=U}
```

Render a path from its key via `ff.path_of_key` → segments (`.Field(f)` → `.f${f}`... use the *field id* as printed; `Elem`/`Val` → `Elem`/`Val`). Append each block's `verdicts` (sorted by result local) as their own lines. Guard: emit nothing when both maps are empty so un-analyzed / field-free functions render exactly as today.

- [ ] **Step 5: Boot tests, rebuild CLI, smoke + determinism**

```bash
target/twk run boot/tests/main.tw
make bundle-cli
printf 'type Ctx = .{ types: Dict<Int, Int> }\nfn build() Ctx {\n  c := Ctx.{ types: Dict.new() }\n  c.types = Dict.set(c.types, 1, 2)\n  c\n}\n' > /tmp/pf.tw
target/twk ir /tmp/pf.tw --cfg | grep -E "field_facts=|field=" | head
target/twk ir /tmp/pf.tw --cfg > /tmp/a.txt
target/twk ir /tmp/pf.tw --cfg > /tmp/b.txt
diff /tmp/a.txt /tmp/b.txt && echo DETERMINISTIC
```

Expected: field facts + a two-verdict line appear; `diff` clean. (If the `c.types = …` sugar doesn't lower to an `ARecordUpdate` over a quartet after `--opt`, adjust the fixture to the explicit projection form; the point is a locally-fresh record whose dict field is refreshed.)

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/cfg.tw boot/tests/suites/cfg_field_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/compiler/cfg.tw boot/tests/suites/cfg_field_facts_suite.tw
git commit -m "cfg/ownership: render field facts + two-verdict update lines

twk ir --cfg now prints per-boundary field_facts (sorted by LocalId, PathKey via
the reversible codec) and, at each record-update/quartet site, the orthogonal
shell/field verdicts with an explicit rejection reason -- never a silent bail."
```

---

## Task 8: Determinism gate, census guard, full verification, tracking

**Files:**
- Modify: `docs/plans/sound-uniqueness/analysis/README.md`, `docs/plans/README.md`.

- [ ] **Step 1: `--census` still shows 0 in-place (no codegen)**

```bash
printf 'type Ctx = .{ types: Dict<Int, Int> }\nfn build() Ctx {\n  c := Ctx.{ types: Dict.new() }\n  c.types = Dict.set(c.types, 1, 2)\n  c\n}\n' > /tmp/cen.tw
target/twk ir /tmp/cen.tw --census
```

Expected: `dict_set` candidates ≥ 1, **in_place = 0** (Phase 4 changes no codegen — Acceptance 9).

- [ ] **Step 2: Downward-closed invariant sweep (Acceptance 6)**

Add a suite assertion that walks every fixture's blocks and fails if any local has a non-empty `field_own` while its `own` is non-`Unique`. Add it as a `.test` that runs the invariant over the Task 3–6 fixtures (a shared helper `assert_downward_closed(f)` iterating `f.blocks` entry+exit). Run `target/twk run boot/tests/main.tw`.

- [ ] **Step 3: Full verification**

Run sequentially (never concurrently — concurrent `twk` pegs CPU):

```bash
make boot-test
make stage2
```

Expected: boot suites green; `make stage2` reaches the self-host fixed point (`stage3 == stage4`). Phase 4 is boot-only and adds **no** stage0-parity construct (`field_facts.tw` is new boot compiler code; stage0 compiles boot source but never needs to *understand* the new analysis — confirm no new syntax was used that stage0 can't parse).

- [ ] **Step 4: Mark the README bullets**

In `docs/plans/sound-uniqueness/analysis/README.md` Phase 4 section, check off the delivered bullets (field-backing, field-sensitivity, nested inner, projection move/borrow, two-verdict output). Leave Phase 5/6 deferrals unchecked. Do **not** archive `phase4-design.md` (analysis track keeps design docs); this *plan* is what gets archived on completion.

- [ ] **Step 5: Archive the plan (on completion)**

When the plan is fully delivered and verified, move this file to
`docs/plans/archive/sound-uniqueness-phase4-plan.md` (matching the Phase 0–3
plans already there). The sound-uniqueness track is indexed in
`docs/plans/README.md` by a single **folder-level** row (`sound-uniqueness/`),
not per-phase rows — leave that row in place; the track is not yet complete.

- [ ] **Step 6: Commit**

```bash
target/twk lint boot/main.tw
git add docs/plans/sound-uniqueness/analysis/README.md docs/plans/README.md
git commit -m "docs/sound-uniqueness: track Phase 4 field-ownership delivery"
```

---

## Self-review

**1. Spec coverage** (against `phase4-design.md`):
- Additive `field_own` side map (Decision 1), `AccessPath`/`PathSeg` (Decision 2), reversible PathKey (Decision 3) → Tasks 1–2. ✓
- Single-retention introduction, `ARecord`/`AArrayLit`/`ARecordUpdate` subtree-remove-then-graft (Decisions 4/6, intro table) → Task 3. ✓
- Collection-builtin nested facts, `Vector.make` never-`[Elem]`, container kind on the optimizer semantics (Decision 5, "where each rule lands") → Task 4. ✓ (empty-collection vacuous minting flagged as an open refinement, not silently assumed)
- Projection move/borrow, rebase, demote-both (Decision 4, projection section) → Task 5. ✓
- Quartet shell-writeback as block-local pre-pass (Decision 4, the tightening) → Task 6. ✓
- Two-verdict output + field-fact rendering, no silent bail (Output/Rendering) → Task 7. ✓
- Downward-closed at `set_own_st` choke point, demotion cascade (Threading and demotion) → Tasks 2 + 8 sweep. ✓
- Join/fixpoint/determinism (same param/live-through + skip-unprocessed, per-path meet) → Task 2. ✓
- Acceptance 1–10 mapped: field-backing (T3), field-sensitivity (T3 sibling), nested inner (T4), move/borrow (T5/T6), demotion cascade (T2/T5 clears), downward-closed sweep (T8), alias-creation guards (T3/T4 negatives), determinism (T1 codec + T7 render), census-0 (T8), full verify (T8). ✓

**2. Placeholder scan:** Task 7's verdict/render steps describe the render format by contract; all novel algorithmic code (codec, map ops, single-retention, introduction arms, projection, container kind, the `quartet_ok` recognizer) is shown in full. No `TBD`/blank markers remain. The only inline verify-during-execution notes are *name confirmations against the live code* (the exact `vector$set_unsafe`/`builder.push_id` ids, the `Terminator`/`CfgEdge.args` field names, the `op_uses` helper name) — each says exactly what to grep and substitute.

**Review pass (post-subagent-review fixes applied):** ANF constructor arities corrected (`ARecordGet(Atom, FieldId, TypeId)`; `ARecordUpdate(Atom, FieldId, Atom, Bool, TypeId)` — 4th is `false`, 5th `TypeId`); `BlockFacts.live` kept `Vector<Int>`; the container signal moved off nonexistent `Vector.append`/`Vector.set` method-ids onto a `ContainerKind` carried on the optimizer semantics keyed by the real `vector$set_unsafe`/`builder.push_id`/`Dict.set` ids, with `NotCollection` dropping nested facts; single-retention now counts the candidate across **all** retained operands (so `Dict.set(d, x, x)` mints no `[Val]`); the quartet recognizer no longer requires an `AAssign` and adds a mandatory block-exit escape check; the plan/design PathKey conflict reconciled (design Decision 3 updated to the reversible codec); `set_path` rejects the shell key; `ff.PathSeg` qualification noted.

**3. Type consistency:** `field_own: Dict<Int, Dict<Int, Int>>` used identically in `ForwardState`, `BlockFacts`, `FixResult.exit_field_own`, and every helper (`field_own_get`/`set_field_own`/`field_of`/`join_entry_field_own`). `ff.graft`/`project`/`remove_prefix`/`merge`/`is_unique`/`path_key`/`path_of_key` signatures match between Task 1's module and Tasks 3–7's call sites. `single_retention`/`container_seg`/`stored_element_atom`/`recognize_quartet_moves`/`quartet_has` defined once and reused. `Projection.{ shell, fields }` consistent between Task 1 and Task 5.

**Open risks to watch during execution:**
- **Task 2 threads `field_own` + a new join through the fixpoint** (`run_fixpoint`/`FixResult`/`analyze_function` materialize) exactly where Phase 3 threaded `prov`. Mirror the `exit_prov` plumbing precisely; a missing `field_own: Dict.new()` on any `ForwardState.{ … }` construction breaks compilation, and a missed materialize site leaves `blk.exit.field_own` empty (silent under-claim, caught by the Task 3 positive tests).
- **Choke-point ordering (G3):** introduction/projection MUST `set_own_st(result, .Unique)` **before** populating `field_own[result]`, or the clear wipes the fresh claim. Every task that adds facts follows set-shell-then-populate.
- **`op_uses` reuse (Task 6):** the recognizer's `op_mentions_local` must use the *same* use-extraction the analysis already trusts (the `push_uid` list around `ownership.tw:95`), including `ARecordGet`/`ARecordUpdate` bases — otherwise the quartet proof can miss a hidden use of `base` and unsoundly move. Grep and reuse; do not hand-roll a second use list.
- **Quartet is single-block only (G5):** `recognize_quartet_moves` scans one block's `instructions`; a projection whose update lands in a successor block is *not* a quartet and must borrow. Do not follow edges.
- **`--opt` shape survival:** fixtures are hand-built ANF (stable ids), but the CLI smoke uses real `.tw` through `--opt`; the optimizer may inline/reshape the quartet. If the CLI verdict line doesn't appear, inspect `twk ir /tmp/pf.tw --opt` and adjust the fixture to the shape that survives (design Testing note: each fixture is checked against `--opt`).
- **Determinism:** PathKeys are canonical ints; every rendered field-fact / verdict line is sorted by `(LocalId, PathKey)` / result local. The Task 7 determinism assertion + the Task 1 codec collision test gate it. Never let `Dict.keys()` iteration order reach output.
- **Cost:** the quartet recognizer is an extra O(n²)-worst per block linear scan (each `record_get` scans to block end). Blocks are short; fine for analysis, but do not lift this onto a hot codegen path in later phases without bounding it.
- **Nested dict coverage is literal-bootstrapped only (deferred refinement):** with empty-array `[Elem]` minting but **no** `Dict.new` `[Val]` minting, vectors accumulate `[Elem]` from a literal base but dicts built by `Dict.new`+`set` (Case B, the census-dominant idiom) get **no** `[Val]` in this plan. The retained-operand single-retention dedup (so `Dict.set(d, x, x)` mints no `[Val]`) is therefore correct-but-latent — it only becomes observable once `Dict.new` mints a vacuous `[Val]`. Before committing to that (a small `.Allocate`-branch signal for empty-collection constructors), confirm against `records-fields.md` Case B; when added, also add the `Dict.set(d, x, x)` negative the reviewer asked for. Phase 4 stays sound without it (under-claim).
