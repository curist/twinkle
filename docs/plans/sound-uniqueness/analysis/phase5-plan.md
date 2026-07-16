# Phase 5 Return-path Summaries — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the boot ownership analysis *return-path ownership* — the region handed back through a returned record field (`out.ctx`) or variant payload (`Ok[0].state`) — and the caller-side recovery that keeps transport-wrapper / `Result`-payload state threading from classifying as aggregate publication. Analysis-only: no codegen changes.

**Architecture:** Extend the intraprocedural field-fact layer (`field_facts.tw`) with a tagged variant-payload path segment; add path-attributed provenance (`path_prov`) so a returned record's fields attribute to individual params; revise `Return` to be a non-retaining CFG leaf and `ret` to mean shell ownership; classify per-return-path ownership into a new `Summary.ret_paths`; recover it at the caller under a `last`-gated publish-on-fail rule, a bounded transport-wrapper projection recognizer, and match-arm payload seeding. Canonical design: [phase5-design.md](phase5-design.md).

**Tech Stack:** Twinkle (`.tw`) boot compiler. Build: `make bundle-cli` (→ `target/twk`) or `cargo run --release -- …` for stage0. Boot tests: `target/twk run boot/tests/main.tw`. Ownership modules: `boot/compiler/{field_facts,ownership,summary,cfg}.tw`. Unit suites: `boot/tests/suites/cfg_*_suite.tw`.

**Conventions for every task:** after editing a `.tw` file run `target/twk fmt <file>` then `target/twk lint <entry>`. Run the boot suite with `target/twk run boot/tests/main.tw`. Commit messages: imperative subject, what/why body, no metrics; add the `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` trailer. Heavy verification (`make stage2`, `make bundle-cli`, full suite) runs **sequentially, never backgrounded**.

**Build note:** the ownership modules are embedded into the self-hosted compiler, so a source change is only exercised after rebuilding `target/twk`. During TDD, rebuild the CLI once per task before running the suite: `make quick-bundle-cli` if `target/boot.wasm` is fresh, else `make bundle-cli`. Where a task says "run the suite," it means: rebuild the CLI, then `target/twk run boot/tests/main.tw`.

---

## File structure

| File | Responsibility | Phase 5 change |
|---|---|---|
| `boot/compiler/field_facts.tw` | Leaf path-fact unit: `PathSeg`/`AccessPath`/`FieldMap`, reversible `PathKey` codec, map ops | Add tagged `Payload(tag,index)` segment + negative-range codec + `Payload` prefix in graft/project/remove_prefix |
| `boot/compiler/ownership.tw` | Forward ownership transfer, `ForwardState`, summaries, `summarize_function` | Add `path_prov`; split shell/field prov in aggregate builders; `AVariant` payload facts; remove `Return` publish; `ret_paths` types + classification; caller consumption with `last`+gate; transport recognizer; match-arm payload seed |
| `boot/compiler/summary.tw` | Whole-program SCC summary driver + rendering | Seed/compare/render `ret_paths`; hide-in-progress + strip-on-cap discipline |
| `boot/compiler/cfg.tw` | Structural CFG view; `CfgBlock`, `build_match` | Per-arm payload-projection metadata (`payload_src`) for top-level `Var` payload bindings |
| `boot/tests/suites/cfg_field_facts_suite.tw` | Field-fact unit tests | Payload codec + graft/project round-trip tests |
| `boot/tests/suites/cfg_summary_suite.tw` | Summary unit tests | Re-baseline Return/escape; `ret_paths` classification; caller recovery |
| `boot/tests/suites/cfg_return_paths_suite.tw` (new) | Phase 5 cross-function fixtures | Cases W, R, gate negatives, transport move/borrow, determinism |

Task order is strictly dependency-first: Stage A (codec) → B (provenance) → C (return semantics) → D (summary schema) → E (fixpoint) → F (caller) → G (transport move) → H (match/Case R) → I (rendering + verification).

---

## Stage A — Tagged payload segment + codec

### Task 1: Add `Payload(tag,index)` to `PathSeg` and extend the codec

**Files:**
- Modify: `boot/compiler/field_facts.tw` (`PathSeg` `:12`, `seg_eq` `:45`, `path_key` `:82`, `path_of_key` `:103`, `graft` `:152`, `project` `:180`, `remove_prefix` `:197`)
- Test: `boot/tests/suites/cfg_field_facts_suite.tw`

- [ ] **Step 1: Write the failing codec round-trip test**

Add to `cfg_field_facts_suite.tw` (inside `suite()`), using the module alias already imported as `ff` or `field_facts` — check the file's imports and match it:

```tw
.test(
  "payload path_key round-trips and stays disjoint from field keys",
  fn() {
    // [Payload(tag,i)] and [Payload(tag,i), Field(f)] over a spread.
    for tag in range(3) {
      for i in range(2) {
        p0 := field_facts.AccessPath.{ segs: [.Payload(tag, i)] }
        k0 := field_facts.path_key(p0)
        try assert.equal(k0 < 0, true) // payload keys are negative
        try assert.equal(field_facts.path_eq(field_facts.path_of_key(k0), p0), true)
        for f in range(3) {
          p1 := field_facts.AccessPath.{ segs: [.Payload(tag, i), .Field(f)] }
          k1 := field_facts.path_key(p1)
          try assert.equal(field_facts.path_eq(field_facts.path_of_key(k1), p1), true)
          // disjoint from a positive field key with the same f
          fk := field_facts.path_key(field_facts.field_path(f))
          try assert.equal(k1 == fk, false)
        }
      }
    }
    .Ok({})
  },
)
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `target/twk run boot/tests/main.tw` (after `make quick-bundle-cli`).
Expected: parse/type error — `PathSeg` has no `Payload` variant yet.

- [ ] **Step 3: Extend `PathSeg` and `seg_eq`**

In `field_facts.tw:12`:

```tw
pub type PathSeg = { Field(Int), Elem, Val, Payload(Int, Int) }
```

Add the `Payload` case to `seg_eq` (`:45`):

```tw
.Payload(ta, ia) => case b {
  .Payload(tb, ib) => ta == tb and ia == ib,
  _ => false,
},
```

- [ ] **Step 4: Extend the codec (negative disjoint range)**

Positive keys stay as-is. Payload keys use a reversible pairing into the negatives. Add a helper and the `Payload` cases. In `field_facts.tw`, above `path_key`:

```tw
// Reversible pairing of a nonneg pair -> nonneg (Cantor).
fn pair2(a: Int, b: Int) Int {
  s := a + b
  s * (s + 1) / 2 + b
}
fn unpair2(z: Int) .{ a: Int, b: Int } {
  w := 0
  for (w + 1) * (w + 2) / 2 <= z {
    w = w + 1
  }
  t := w * (w + 1) / 2
  b := z - t
  .{ a: w - b, b }
}
// Payload paths encode (tag, index, fieldslot) where fieldslot = 0 (no field)
// or 1+f. Nested pairing -> a single nonneg, mapped to a disjoint NEGATIVE key.
fn payload_key(tag: Int, index: Int, fieldslot: Int) Int {
  0 - (pair2(pair2(tag, index), fieldslot) + 1)
}
```

Add to `path_key` (`:82`) — a `Payload` case at `segs.len() == 1` and a `Payload`-prefixed case at `segs.len() == 2`:

```tw
// inside segs.len() == 1 case:
.Payload(tag, i) => payload_key(tag, i, 0),
// inside segs.len() == 2 case (segs[0]):
.Payload(tag, i) => case segs[1] {
  .Field(f) => payload_key(tag, i, 1 + f),
  _ => error("field_facts: payload nested seg must be Field in Phase 5"),
},
```

Add to `path_of_key` (`:103`) a branch for `k < 0`:

```tw
k < 0 => {
  z := (0 - k) - 1
  outer := unpair2(z)          // outer.a = pair2(tag,index), outer.b = fieldslot
  ti := unpair2(outer.a)       // ti.a = tag, ti.b = index
  fieldslot := outer.b
  if fieldslot == 0 {
    AccessPath.{ segs: [.Payload(ti.a, ti.b)] }
  } else {
    AccessPath.{ segs: [.Payload(ti.a, ti.b), .Field(fieldslot - 1)] }
  }
},
```

- [ ] **Step 5: Add the `Payload` prefix case to `graft`, `project`, `remove_prefix`**

`graft` (`:152`) currently special-cases `.Field(_)` and drops others. Add `.Payload` alongside `.Field` so a payload prefix carries a `Field` inner:

```tw
// change the match head in graft to handle both Field and Payload prefixes:
case prefix {
  .Field(_) => { /* existing loop grafting [prefix, Elem/Val] */ },
  .Payload(_, _) => {
    for k, v in src.paths {
      inner := path_of_key(k)
      if inner.segs.len() == 1 {
        case inner.segs[0] {
          .Field(_) => fm.paths[path_key(AccessPath.{ segs: [prefix, inner.segs[0]] })] = v,
          _ => {},   // Elem/Val/Payload inners not representable under a payload prefix at depth 2
        }
      }
    }
    fm
  },
  _ => fm,
}
```

`project` (`:180`) and `remove_prefix` (`:197`) already match on any `prefix: PathSeg` structurally via `seg_eq`, so they work for `Payload` unchanged — **verify by reading them**; no edit expected. Add a one-line comment noting payload prefixes are supported.

- [ ] **Step 6: Run the codec test — expect PASS**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the payload round-trip test passes; all existing field-fact tests still pass.

- [ ] **Step 7: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/field_facts.tw boot/tests/suites/cfg_field_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/field_facts.tw boot/tests/suites/cfg_field_facts_suite.tw
git commit -m "field_facts: add tagged Payload path segment and negative-range codec

Phase 5 needs variant-payload ownership keyed by (variant_tag, payload_index)
so an .Err arm can never recover an .Ok payload fact. Encode payload paths in a
disjoint negative PathKey range via a reversible pairing, keeping positive field
keys intact, and extend graft to carry a Field inner under a payload prefix.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage B — Path-attributed provenance

### Task 2: Add `path_prov` to `ForwardState` (inert)

**Files:**
- Modify: `boot/compiler/ownership.tw` (`ForwardState` `:618`, and every `ForwardState.{ … }` constructor site — search `ForwardState.{`)
- Test: existing suites (regression only)

- [ ] **Step 1: Add the field and accessors**

Extend `ForwardState` (`:618`):

```tw
type ForwardState = .{
  own: Dict<Int, Int>,
  valid: Dict<Int, Bool>,
  prov: Dict<Int, Vector<Int>>,
  field_own: Dict<Int, ff.FieldMap>,
  path_prov: Dict<Int, Dict<Int, Vector<Int>>>,   // local -> PathKey -> origins
}
```

Add near `field_own_get` (`:625`):

```tw
fn path_prov_get(st: ForwardState, id: Int) Dict<Int, Vector<Int>> {
  case st.path_prov.get(id) {
    .Some(m) => m,
    .None => Dict.new(),
  }
}
fn set_path_prov(st: ForwardState, id: Int, m: Dict<Int, Vector<Int>>) ForwardState {
  st.path_prov[id] = m
  st
}
fn clear_path_prov(st: ForwardState, id: Int) ForwardState {
  st.path_prov[id] = Dict.new()
  st
}
```

- [ ] **Step 2: Seed `path_prov` empty at every constructor**

Search `ForwardState.{` (the fixpoint entry builder near `:2328`, the initial-entry builder, and any join builder) and add `path_prov: Dict.new()` (or the joined map — Task 9 fills joins). For now every site seeds `Dict.new()`.

- [ ] **Step 3: Fold `path_prov` into the `set_own_st` choke point**

In `set_own_st` (`:642`), when the shell leaves Unique, also clear path_prov:

```tw
st = if o.own_tag() != 0 {
  st.clear_field_own(id).clear_path_prov(id)
} else {
  st
}
```

- [ ] **Step 4: Run the suite — expect PASS (no behavior change yet)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: all green; `path_prov` is populated empty and read nowhere.

- [ ] **Step 5: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw
git commit -m "ownership: add inert path_prov map to ForwardState

Mirrors field_own (local -> PathKey -> origins); cleared through the same
set_own_st choke point. Populated empty for now; later tasks split shell vs
field provenance and read it for return-path classification.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

### Task 3: Split shell vs field provenance in aggregate builders

This flips a returned wrapper param from `Retained` to `Borrowed` (the Blocker 3 groundwork combined with Task 5), and makes multi-accumulator attribution possible.

**Files:**
- Modify: `boot/compiler/ownership.tw` (`ARecord` transfer `:1048`, `AArrayLit` `:1082`, `ARecordUpdate` `:1141`, `publish_local` `:689`)
- Test: `boot/tests/suites/cfg_summary_suite.tw`

- [ ] **Step 1: Write the failing per-path-prov test**

Add a helper to `cfg_summary_suite.tw` that reads a summary's field-level classification is out of scope here; instead assert the intraprocedural split via a returned record's future `ret_paths` — but `ret_paths` doesn't exist yet. So test the *observable* consequence now: a fresh two-field record's shell `prov` no longer conflates origins. Expose a tiny test hook. Add to `cfg_summary_suite.tw`:

```tw
.test(
  "fresh record shell prov is empty (not unioned field origins)",
  fn() {
    // fn f(x, y) { let r = Record{f0:x, f1:y}; r }  => r's SHELL aliases no param.
    b := b_reg()
    two_field := AnfOp.ARecord(TypeId.{ id: 0 }, [
      .{ field: FieldId.{ id: 0 }, value: .ALocal(lid(0)) },
      .{ field: FieldId.{ id: 1 }, value: .ALocal(lid(1)) },
    ])
    body: AnfExpr = .Let(lid(2), two_field, .Atom(.ALocal(lid(2))))
    s := summ1("f", 2, body)
    // With shell/field prov split + Task 5, a returned fresh wrapper does NOT
    // alias params at the shell level: ret is OwnedFresh (tag 0), not MayAliasParams.
    try assert.equal(ret_tag(s.ret), 0)
    .Ok({})
  },
)
```

Note this test also depends on Task 5 (return no longer publishes) to fully pass; mark it `// depends: Task 3 + Task 5` and expect it RED until Task 5. If you prefer strict per-task green, gate the assertion behind Task 5 by first asserting only the field-prov map here via a dedicated internal accessor. Simplest path: keep the assertion, let it go green at Task 5, and in Task 3 assert the weaker invariant below.

Weaker Task-3-local assertion (add as its own test, must go green in Task 3): after building the record, the record local's `prov` is empty. Add an internal test entry point `pub fn debug_shell_prov(...)` is overkill — instead assert via `MayAliasParams` disappearing is deferred to Task 5. **For Task 3, write only the code and rely on Task 5's test**; add the test above now and expect RED until Task 5.

- [ ] **Step 2: Run — expect RED (still MayAliasParams)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the new test fails (`ret_tag` is 1 = MayAliasParams) because the return still publishes and shell prov still conflates.

- [ ] **Step 3: Route field origins to `path_prov`, empty the shell prov**

In `ARecord` (`:1048`) replace the `set_prov_st(result, origins)` (which unions field origins) with shell-empty + per-field path_prov. Current code computes `origins` and grafts `field_own`; add path_prov grafting in the same loop and set shell prov empty:

```tw
.ARecord(_, fields) => {
  operands: Vector<Atom> = collect fa in fields { fa.value }
  for fa in fields {
    st = .field_store(fa.value, last)
  }
  st = .set_result(result, .Unique)
  st = .set_prov_st(result, [])   // SHELL prov: fresh shell aliases no param

  rf := ff.empty()
  pp: Dict<Int, Vector<Int>> = Dict.new()
  for fa in fields {
    if st.single_retention(fa.value, last, operands) {
      seg := ff.PathSeg.Field(fa.field.id)
      rf = .graft(seg, st.atom_field_own(fa.value))
      // path_prov: this field's origins (its own shell prov + rebased inner path_prov)
      pp = graft_path_prov(pp, seg, st, fa.value)
    }
  }
  st = if rf.is_empty() { st } else { st.set_field_own(result, rf) }
  st.set_path_prov(result, pp)
}
```

Add the `graft_path_prov` helper (near the aggregate builders) that mirrors `ff.graft` for provenance — it writes the value's shell prov at `[seg]` (as `Some([...])`, possibly `[]` for fresh) and rebases the value's inner `path_prov` under `seg`:

```tw
// Copy `src`'s provenance under `prefix` into `pp`: the [prefix] entry is the
// value's SHELL prov (empty vector = proven fresh); inner Field paths rebase.
fn graft_path_prov(pp: Dict<Int, Vector<Int>>, prefix: ff.PathSeg, st: ForwardState, a: Atom) Dict<
  Int, Vector<Int>,
> {
  shell_origins := prov_of(st.prov, a)   // [] when the value is fresh
  pp[ff.path_key(ff.AccessPath.{ segs: [prefix] })] = shell_origins
  case atom_local_id(a) {
    .Some(sid) => {
      for k, v in st.path_prov_get(sid) {
        inner := ff.path_of_key(k)
        if inner.segs.len() == 1 {
          case inner.segs[0] {
            .Field(_) => pp[ff.path_key(ff.AccessPath.{ segs: [prefix, inner.segs[0]] })] = v,
            _ => {},
          }
        }
      }
    },
    .None => {},
  }
  pp
}
```

Apply the same shell-empty + `graft_path_prov` treatment to `AArrayLit` (`:1082` — shell prov `[]`; only `[Elem]` when all-owned, so path_prov gets `[Elem]` = union of elem origins if you choose to track it, else leave empty for Elem and rely on field_own only — Elem paths are not `ret_paths` candidates in Phase 5, so **leave AArrayLit path_prov empty**, just set shell prov `[]`).

For `ARecordUpdate` (`:1141`): shell prov should follow the base's shell prov (a rebuilt shell of a param-aliased record still aliases that param at the shell level only via base) — keep `origins := prov_of(st.prov, base)` for the shell (drop the `v` union), and set `path_prov[result]` = base's path_prov with `[.f]*` removed then `graft_path_prov([.f], v)` when single-retention.

- [ ] **Step 4: Publish `path_prov` origins in `publish_local`**

In `publish_local` (`:689`), after publishing `prov` origins, also publish path_prov origins:

```tw
fn publish_local(st: ForwardState, id: Int) ForwardState {
  st = .set_own_st(id, .Shared)
  case st.prov.get(id) {
    .Some(origins) => for o in origins { st = .set_own_st(o, .Shared) },
    .None => {},
  }
  for k, os in st.path_prov_get(id) {
    for o in os { st = .set_own_st(o, .Shared) }
  }
  st
}
```

Note `set_own_st(id, .Shared)` clears `path_prov[id]` via the choke point — capture the origins **before** clearing. Reorder so the path_prov publish reads a local copy captured at entry:

```tw
fn publish_local(st: ForwardState, id: Int) ForwardState {
  pp := st.path_prov_get(id)                 // capture before the choke point clears it
  shell := case st.prov.get(id) { .Some(o) => o, .None => [] }
  st = .set_own_st(id, .Shared)
  for o in shell { st = .set_own_st(o, .Shared) }
  for k, os in pp {
    for o in os { st = .set_own_st(o, .Shared) }
  }
  st
}
```

- [ ] **Step 5: Run — the Task-3 code compiles; the split test stays RED until Task 5**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: everything green **except** the `ret is OwnedFresh` test (still RED — return publish remains). All previously-green tests stay green (shell-prov emptying alone doesn't change escape while the return still publishes, because publish now cascades path_prov). Confirm no *other* regression.

- [ ] **Step 6: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "ownership: split shell vs field provenance in aggregate builders

A freshly-constructed record/variant/array now carries an empty shell prov and
routes each field's origins into path_prov under [.f]; publish_local publishes
path_prov origins so whole-value publication still leaks fields. Groundwork for
per-path return classification; the returned-wrapper ret flip lands with the
Return-publish removal.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

### Task 4: `AVariant` grafts payload facts + path_prov

**Files:**
- Modify: `boot/compiler/ownership.tw` (`AVariant` transfer `:1073`)
- Test: `boot/tests/suites/cfg_field_facts_suite.tw` or `cfg_summary_suite.tw`

- [ ] **Step 1: Write the failing test — a variant over an owned payload carries `[Payload(tag,0)]`**

Build `fn f(x) { let r = Variant#tag0(x); r }` and assert (via a new internal helper that reads the returned local's field_own — reuse the summary path once Task 7 exists). For Task 4, assert the *intraprocedural* fact by extending `analyzed_caller`-style access is heavy; instead assert through the eventual `ret_paths` in Task 8. **For Task 4, write the code and add a focused field_facts-level test** that the `AVariant` transfer produces `[Payload(tag,i)]` by unit-testing a tiny driver. Simplest: defer the assertion to Task 8's variant `ret_paths` test and here assert only that building a variant does not crash and the shell stays Unique:

```tw
.test(
  "variant with single-retention payload keeps shell Unique",
  fn() {
    b := b_reg()
    vop := AnfOp.AVariant(TypeId.{ id: 0 }, VariantId.{ id: 5 }, [.ALocal(lid(0))])
    body: AnfExpr = .Let(lid(1), vop, .Atom(.ALocal(lid(1))))
    s := summ1("f", 1, body)
    try assert.equal(ret_tag(s.ret), 0) // OwnedFresh shell (depends on Task 5 for publish)
    .Ok({})
  },
)
```

- [ ] **Step 2: Run — expect RED (until Task 5) or compile-checked now**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: RED on the `ret_tag == 0` assertion until Task 5; no crash.

- [ ] **Step 3: Graft payload facts in `AVariant`**

Replace the `AVariant` transfer (`:1073`) — keep shell Unique, empty shell prov, and graft each single-retention payload under `[Payload(vid.id, i)]`:

```tw
.AVariant(_, vid, args) => {
  for a in args { st = .field_store(a, last) }
  st = .set_result(result, .Unique)
  st = .set_prov_st(result, [])   // shell aliases no param

  rf := ff.empty()
  pp: Dict<Int, Vector<Int>> = Dict.new()
  for a, i in args {
    if st.single_retention(a, last, args) {
      seg := ff.PathSeg.Payload(vid.id, i)
      rf = .graft(seg, st.atom_field_own(a))
      pp = graft_path_prov(pp, seg, st, a)
    }
  }
  st = if rf.is_empty() { st } else { st.set_field_own(result, rf) }
  st.set_path_prov(result, pp)
}
```

- [ ] **Step 4: Run — code compiles; assertion still gated on Task 5**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: no crash; the `ret_tag == 0` test stays RED until Task 5.

- [ ] **Step 5: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_field_facts_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_field_facts_suite.tw
git commit -m "ownership: AVariant carries tagged payload facts + path_prov

An AVariant(tag, [payload...]) result now grafts single-retention payload
ownership under [Payload(tag, i)] with path-attributed provenance, so return
classification and match-arm seeding can recover variant-payload state.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage C — Return is a non-retaining leaf

### Task 5: Remove `Return`'s publication (keep `ValueBreak`'s)

**Files:**
- Modify: `boot/compiler/ownership.tw` (`forward_block` `:1393`)
- Test: `boot/tests/suites/cfg_summary_suite.tw` (re-baseline)

- [ ] **Step 1: Re-baseline the existing escape tests**

The existing test `t1 aggregate-escape: fn f(x) { Wrapper.{x} } -> p0 retain (blocker 1)` asserts `Retained`. Under Decision 3 a returned wrapper does **not** retain its param. Change its expectation and rename:

```tw
.test(
  "returned wrapper param is Borrowed (return is a hand-off, not retention)",
  fn() {
    b := b_reg()
    body: AnfExpr = .Let(lid(1), wrapper_record(lid(0)), .Atom(.ALocal(lid(1))))
    s := summ1("f", 1, body)
    try assert.equal(p_escape(s, 0), escape_tag(.Borrowed))
    .Ok({})
  },
)
```

Audit the rest of `cfg_summary_suite.tw` for any test asserting `Retained`/`MayAliasParams` that depended on the return-publish (e.g. `fn id(x){x}` retain expectations). A genuine leak (`global_set G0 = x`) must STAY `Retained` — do not change those. Update only return-hand-off cases.

- [ ] **Step 2: Run — expect RED (return still publishes)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the re-baselined tests and the Task-3/Task-4 `ret_tag == 0` tests fail.

- [ ] **Step 3: Remove the Return publish**

In `forward_block` (`:1393`):

```tw
case blk.terminator {
  // Return hands the value to the CALLER; it is not a callee leak. The caller's
  // ret / ret_paths handling accounts for it. Return blocks are CFG leaves, so
  // this does not affect intra-function joins (Case T). ValueBreak still targets
  // a real post-loop successor, so it still publishes.
  .Some(.ValueBreak(a)) => {
    st = .publish_atom(a)
  },
  _ => {},
}
```

- [ ] **Step 4: Run — expect PASS (re-baseline + Task 3/4 tests green)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: returned-wrapper param `Borrowed`; fresh-wrapper `ret` = `OwnedFresh`; variant `ret` = `OwnedFresh`; genuine leaks still `Retained`. Full suite green.

- [ ] **Step 5: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "ownership: Return is a non-retaining leaf, not a publication

A returned value is handed to the caller, so it no longer demotes its param
origins to Retained; genuine leaks (globals/closures/aggregates/aliasing) still
mark Retained independently, and return blocks being CFG leaves keeps Case T
join behavior intact. ValueBreak still publishes (real post-loop successor).
Re-baselines the returned-wrapper escape expectations.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage D — Summary `ret_paths` schema + classification

### Task 6: Add the `ret_paths` schema (seeded, compared, rendered)

**Files:**
- Modify: `boot/compiler/ownership.tw` (types near `:45`; `summarize_function` `:2355` returns `ret_paths: []` for now), `boot/compiler/summary.tw` (`conservative_summary` `:58`, `same_summary` `:113`, `render_summary` `:332`)
- Test: existing suites (regression)

- [ ] **Step 1: Add the types and extend `Summary`**

In `ownership.tw` near `:45`:

```tw
pub type ReturnOwn = { OwnedFresh, OwnedFromParam(Int) }
pub type RetVia = { Direct, Variant(Int, Int) }        // Direct | Variant(tag, payload_index)
pub type ReturnPathOwn = .{ via: RetVia, field: Int?, own: ReturnOwn }
pub type Summary = .{ params: Vector<ParamSummary>, ret: ReturnEffect, ret_paths: Vector<ReturnPathOwn> }
```

Update the `Summary.{ params, ret }` construction in `summarize_function` (`:2355`) to `Summary.{ params, ret, ret_paths: [] }` for now.

- [ ] **Step 2: Seed / compare / render in `summary.tw`**

`conservative_summary` (`:58`): `Summary.{ params, ret: .Shared, ret_paths: [] }`.

`same_summary` (`:113`): after the param + `return_eq` checks, compare `ret_paths` with a canonical-sorted equality. Add:

```tw
fn ret_own_eq(a: ownership.ReturnOwn, b: ownership.ReturnOwn) Bool {
  case a {
    .OwnedFresh => case b { .OwnedFresh => true, _ => false },
    .OwnedFromParam(ka) => case b { .OwnedFromParam(kb) => ka == kb, _ => false },
  }
}
fn via_rank(v: ownership.RetVia) Int {
  case v { .Direct => -1, .Variant(t, i) => t * 4096 + i }
}
fn field_rank(f: Int?) Int { case f { .Some(x) => x, .None => -1 } }
fn rpo_key(r: ownership.ReturnPathOwn) Int { via_rank(r.via) * 100000 + (field_rank(r.field) + 1) }
fn sort_ret_paths(v: Vector<ownership.ReturnPathOwn>) Vector<ownership.ReturnPathOwn> {
  v.sort_by(fn(a, b) { rpo_key(a) - rpo_key(b) })
}
fn ret_paths_eq(a: Vector<ownership.ReturnPathOwn>, b: Vector<ownership.ReturnPathOwn>) Bool {
  if a.len() != b.len() { return false }
  sa := sort_ret_paths(a)
  sb := sort_ret_paths(b)
  for r, i in sa {
    o := sb[i]
    if via_rank(r.via) != via_rank(o.via) or field_rank(r.field) != field_rank(o.field)
      or !ret_own_eq(r.own, o.own) { return false }
  }
  true
}
```

(Confirm `Vector.sort_by` exists — memory references `sort_by`; if the comparator signature differs, use the project's sort helper; otherwise sort via `insert_sorted`-style over `rpo_key`.) In `same_summary`, `return return_eq(a.ret, b.ret) and ret_paths_eq(a.ret_paths, b.ret_paths)`.

`render_summary` (`:332`): append a `ret_paths=…` clause. Add a renderer:

```tw
fn render_ret_paths(rps: Vector<ownership.ReturnPathOwn>) String {
  parts: Vector<String> = []
  for r in sort_ret_paths(rps) {
    via := case r.via { .Direct => "", .Variant(t, i) => "V${t}[${i}]" }
    fld := case r.field { .Some(f) => ".f${f}", .None => "" }
    own := case r.own { .OwnedFresh => "fresh", .OwnedFromParam(k) => "from(p${k})" }
    parts = .append("${via}${fld}=${own}")
  }
  if parts.len() == 0 { "" } else { "  ret_paths=${parts.join(" ")}" }
}
```

Append its result to the `render_summary` output string.

- [ ] **Step 3: Run — expect PASS (regression only)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: green; `ret_paths` empty everywhere so rendering is unchanged and equality is unaffected.

- [ ] **Step 4: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/summary.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/compiler/summary.tw
git commit -m "ownership/summary: add ret_paths schema (seed, compare, render)

Adds ReturnOwn/RetVia/ReturnPathOwn and Summary.ret_paths, seeded empty and
compared canonical-sorted so the SCC fixpoint stays stable. Classification and
consumption land next.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

### Task 7: Classify `Direct` record return paths + shell-level `ret`

**Files:**
- Modify: `boot/compiler/ownership.tw` (`summarize_function` `:2316`–`:2356`)
- Test: `boot/tests/suites/cfg_summary_suite.tw`

- [ ] **Step 1: Write the failing test — multi-accumulator attribution**

```tw
.test(
  "fn f(x,y){ Record{f0:x, f1:y} } -> ret_paths [.f0]=from(p0) [.f1]=from(p1)",
  fn() {
    two := AnfOp.ARecord(TypeId.{ id: 0 }, [
      .{ field: FieldId.{ id: 0 }, value: .ALocal(lid(0)) },
      .{ field: FieldId.{ id: 1 }, value: .ALocal(lid(1)) },
    ])
    s := summ1("f", 2, .Let(lid(2), two, .Atom(.ALocal(lid(2)))))
    try assert.equal(ret_tag(s.ret), 0)              // OwnedFresh shell
    try assert.equal(s.ret_paths.len(), 2)
    // helper below extracts (field, own-param) pairs, sorted
    got := ret_path_pairs(s.ret_paths)
    try assert.equal(same_ints(got, [0, 0, 1, 1]), true) // [f0->p0, f1->p1] flattened
    .Ok({})
  },
)
```

Add the helper:

```tw
// Flatten Direct ret_paths to [field, param, field, param, ...] sorted by field.
fn ret_path_pairs(rps: Vector<ownership.ReturnPathOwn>) Vector<Int> {
  pairs: Vector<Int> = []
  for r in rps {
    case r.via {
      .Direct => case r.field {
        .Some(f) => case r.own {
          .OwnedFromParam(k) => { pairs = .append(f); pairs = .append(k) },
          .OwnedFresh => { pairs = .append(f); pairs = .append(0 - 1) },
        },
        .None => {},
      },
      .Variant(_, _) => {},
    }
  }
  pairs
}
```

- [ ] **Step 2: Run — expect RED (`ret_paths` empty)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: `ret_paths.len()` is 0.

- [ ] **Step 3: Classify shell `ret` and `Direct` paths**

In `summarize_function`, the return loop (`:2318`–`:2352`) currently computes only `ret`. Replace the per-return-block `r` computation so it also builds `ret_paths`. Keep the shell `ret` classification prov-based but shell-only (shell prov is now empty for fresh records, so `MayAliasParams` only fires when the returned local's own shell aliases a param, e.g. `return x`). Add, using `body` (the body-only state) and the returned atom `a`:

```tw
// after computing `body`:
rp_here: Vector<ReturnPathOwn> = []
case atom_local_id(a) {
  .Some(aid) => {
    fm := body.field_own_get(aid)
    pp := body.path_prov_get(aid)
    for k in fm.sorted_keys() {
      p := ff.path_of_key(k)
      // Direct record field: single Field seg.
      if p.segs.len() == 1 {
        case p.segs[0] {
          .Field(f) => case classify_path_own(pp, k, f.params) {
            .Some(own) => rp_here = .append(ReturnPathOwn.{ via: .Direct, field: .Some(f), own }),
            .None => {},
          },
          _ => {},
        }
      }
    }
  },
  .None => {},
}
```

Add `classify_path_own` (three-way path_prov, Decision 2):

```tw
// .None => drop; Some([]) => OwnedFresh; Some([k]) => OwnedFromParam(k); Some([multi]) => drop.
fn classify_path_own(pp: Dict<Int, Vector<Int>>, k: Int, params: Vector<Param>) ReturnOwn? {
  case pp.get(k) {
    .None => .None,
    .Some(os) => cond {
      os.len() == 0 => .Some(.OwnedFresh),
      os.len() == 1 => case param_index_of(params, os[0]) {
        .Some(pi) => .Some(.OwnedFromParam(pi)),
        .None => .None,          // origin is not a parameter -> drop
      },
      _ => .None,
    },
  }
}
```

`param_index_of(params, local_id)` returns the parameter position whose `local.id == local_id` (params are locals 0..n-1 in these fixtures; use the real `f.params` mapping). Reuse/extend the existing `prov_to_indices` logic which already maps origin local-ids to param indices — factor a single-id variant.

Join `rp_here` across return sites with a per-`(via,field)` meet (a path survives only if present & `own`-compatible on every return block). Maintain an accumulator mirroring the existing `ret`/`seen` join:

```tw
ret_paths_acc = if seen { meet_ret_paths(ret_paths_acc, rp_here) } else { rp_here }
```

Add `meet_ret_paths` (intersection by `(via,field)`, `own` must match else drop). Finally return `Summary.{ params, ret, ret_paths: ret_paths_acc }`.

- [ ] **Step 4: Run — expect PASS**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the multi-accumulator test passes; `[.f0]=from(p0)`, `[.f1]=from(p1)`, shell `OwnedFresh`.

- [ ] **Step 5: Add per-path-prov negative tests**

```tw
.test(
  "fn f(x){ Record{f0:x, f1:x} } -> no ret_paths (same origin twice = alias)",
  fn() {
    two := AnfOp.ARecord(TypeId.{ id: 0 }, [
      .{ field: FieldId.{ id: 0 }, value: .ALocal(lid(0)) },
      .{ field: FieldId.{ id: 1 }, value: .ALocal(lid(0)) },
    ])
    s := summ1("f", 1, .Let(lid(1), two, .Atom(.ALocal(lid(1)))))
    try assert.equal(s.ret_paths.len(), 0)  // single_retention fails -> no field claim
    .Ok({})
  },
)
```

Run again — expect PASS.

- [ ] **Step 6: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "ownership: classify Direct record return paths + shell-level ret

Reads each returned record field's field_own with a three-way path_prov
(absent=drop, empty=OwnedFresh, single=OwnedFromParam, multi=drop), joins across
return sites, and classifies ret as shell ownership so a fresh wrapper is
OwnedFresh with per-field ret_paths instead of MayAliasParams.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

### Task 8: Classify `Variant` payload return paths

**Files:**
- Modify: `boot/compiler/ownership.tw` (the same return loop in `summarize_function`)
- Test: `boot/tests/suites/cfg_summary_suite.tw`

- [ ] **Step 1: Write the failing test — variant payload path**

```tw
.test(
  "fn f(x){ Variant#7(Record{f0:x}) } -> ret_paths V7[0].f0=from(p0)",
  fn() {
    inner := AnfOp.ARecord(TypeId.{ id: 0 }, [.{ field: FieldId.{ id: 0 }, value: .ALocal(lid(0)) }])
    vop := AnfOp.AVariant(TypeId.{ id: 0 }, VariantId.{ id: 7 }, [.ALocal(lid(1))])
    body: AnfExpr = .Let(lid(1), inner, .Let(lid(2), vop, .Atom(.ALocal(lid(2)))))
    s := summ1("f", 1, body)
    // one ret_path: via Variant(7,0), field f0, from p0
    try assert.equal(s.ret_paths.len(), 1)
    r := s.ret_paths[0]
    try assert.equal(via_is(r.via, 7, 0), true)
    try assert.equal(field_is(r.field, 0), true)
    try assert.equal(own_is_from(r.own, 0), true)
    .Ok({})
  },
)
```

Add small predicates `via_is`, `field_is`, `own_is_from` to the suite.

- [ ] **Step 2: Run — expect RED**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: `ret_paths.len()` is 0 (variant paths not classified yet).

- [ ] **Step 3: Add the variant-payload classification branch**

In the same `for k in fm.sorted_keys()` loop, handle payload-prefixed paths (`Payload(tag,i)` shell and `Payload(tag,i), Field(f)`):

```tw
if p.segs.len() >= 1 {
  case p.segs[0] {
    .Payload(tag, i) => {
      fld: Int? = if p.segs.len() == 2 {
        case p.segs[1] { .Field(f) => .Some(f), _ => .None }
      } else { .None }
      case classify_path_own(pp, k, f.params) {
        .Some(own) => rp_here = .append(ReturnPathOwn.{ via: .Variant(tag, i), field: fld, own }),
        .None => {},
      }
    },
    _ => {},  // Field handled above; Elem/Val are not ret_path candidates
  }
}
```

Restructure the loop so `Field`-single and `Payload*` are both handled off `p.segs[0]` (fold Task 7's Field branch and this into one `case p.segs[0]`).

- [ ] **Step 4: Run — expect PASS**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the variant-payload test passes.

- [ ] **Step 5: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "ownership: classify variant-payload return paths

Reads [Payload(tag,i)] and [Payload(tag,i), .f] facts on the returned variant
local into RetVia.Variant(tag,i) ret_paths with three-way path_prov, so
Result-payload state transport (Ok[0].state) is summarized with its tag.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage E — Fixpoint discipline

### Task 9: Hide in-progress `ret_paths` in-SCC + strip on cap; join `path_prov`

**Files:**
- Modify: `boot/compiler/summary.tw` (`run_scc` `:286`), `boot/compiler/ownership.tw` (`transfer_summarized_call` consulting the summary; `join_entry_field_own` sibling for path_prov)
- Test: `boot/tests/suites/cfg_summary_suite.tw`

- [ ] **Step 1: Write the failing recursive test**

```tw
.test(
  "recursive transport helper exposes no speculative ret_paths mid-fixpoint; converges empty-or-proven",
  fn() {
    // fn g(x) { case cond { _ => g(x) }; Record{f0:x} }  (self-recursive)
    // Build a minimal self-call + record-return; assert the CONVERGED summary is
    // sound: either the proven [.f0]=from(p0) after convergence, or empty — never
    // a mid-iteration artifact. Concretely assert determinism across two computes.
    funcs := recursive_transport_fixture()  // helper builds the AnfFunctionDef
    t1 := compute_of(funcs)
    t2 := compute_of(funcs)
    s1 := summ_of(t1, 1)
    s2 := summ_of(t2, 1)
    try assert.equal(same_summary_pub(s1, s2), true)  // deterministic + converged
    .Ok({})
  },
)
```

Add `recursive_transport_fixture()` building a self-recursive function, and expose `same_summary` as `pub` (or add `same_summary_pub` wrapper in the suite calling `summary.same_summary`).

- [ ] **Step 2: Run — expect RED or nondeterministic**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: either a mismatch or a speculative path surfaces (recursive read currently uses the in-progress ret_paths).

- [ ] **Step 3: Hide in-progress `ret_paths` during SCC iteration**

In `transfer_summarized_call`, the summary consulted for a callee that is a **member of the SCC currently being iterated** must have `ret_paths` treated as empty. Simplest sound implementation: `run_scc` passes a `scc_set` down and `summarize_function` is told to read callee summaries with `ret_paths` blanked for in-SCC callees. Thread a `Dict<Int, Bool>` "suppress_ret_paths" set into `summarize_function` → `transfer_*` and, in `transfer_summarized_call`, when `in_set(suppress, callee_id)` is true, skip the ret_paths consumption (treat as empty). Escape/`ret`/`params` still flow.

Practical wiring: add an optional parameter carried on `SummaryTable` reads is invasive; instead have `run_scc` set a transient flag by **stripping ret_paths from in-SCC members' summaries in the `table` copy used during iteration**, and doing the real `ret_paths` classification only in a final settle pass after the escape/param/ret fixpoint converges:

```tw
// after the worklist loop converges (or hits cap):
if rounds < cap {
  // FINAL pass: recompute each member once more with full summaries visible,
  // capturing ret_paths now that escape/params/ret are fixed.
  for id in members {
    f := index.get(id)...
    table = table_put(table, id, summarize_function(f, table, b, sem))
  }
} else {
  // cap hit: strip ret_paths for every member (expose empty = conservative).
  for id in members {
    s := table_get(table, id)
    table = table_put(table, id, Summary.{ params: s.params, ret: s.ret, ret_paths: [] })
  }
}
```

During the worklist loop itself, strip ret_paths before storing so in-SCC recursive reads never see them:

```tw
next := summarize_function(f, table, b, sem)
next = Summary.{ params: next.params, ret: next.ret, ret_paths: [] }  // hide mid-fixpoint
```

(The final pass restores ret_paths from the converged escape/param/ret state.) Confirm `same_summary` in the worklist compares the stripped form so termination is by the monotone facts only.

- [ ] **Step 4: Join `path_prov` alongside `field_own`**

`join_entry_field_own` (`:1776`) meets `field_own` per path. Add a sibling `join_entry_path_prov` that meets `path_prov` for the same locals (a path survives only if present on every processed pred; on conflict of origins, keep the **union** — conservative — but since a joined path must be `field_own`-Unique on all preds, and origins should match for a stable OwnedFromParam, take intersection-of-presence with union-of-origins so a divergent origin makes the path multi-origin ⇒ later dropped by `classify_path_own`). Wire it into the `ForwardState.{ … }` entry builder next to `entry_field`.

- [ ] **Step 5: Run — expect PASS + determinism**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw` (twice; outputs stable).
Expected: recursive fixture converges deterministically; no speculative path; non-recursive `ret_paths` (Tasks 7–8) still classified in the final pass.

- [ ] **Step 6: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/summary.tw boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/summary.tw boot/compiler/ownership.tw boot/tests/suites/cfg_summary_suite.tw
git commit -m "summary: hide in-progress ret_paths in-SCC, strip on cap; join path_prov

ret_paths are non-monotone, so within-SCC recursive reads see them empty and a
final settle pass captures them from the converged escape/param/ret facts; a
cap-hit strips them entirely (conservative). path_prov joins in lockstep with
field_own so cross-arm origin divergence degrades to a dropped path.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage F — Caller consumption

### Task 10: `transfer_summarized_call` gains `last` + gate with publish-on-fail

**Files:**
- Modify: `boot/compiler/ownership.tw` (`transfer_call` `:844`, `transfer_summarized_call` `:976`)
- Test: `boot/tests/suites/cfg_return_paths_suite.tw` (new)

- [ ] **Step 1: Create the new suite skeleton + Case W caller test**

Create `boot/tests/suites/cfg_return_paths_suite.tw` mirroring `cfg_summary_suite.tw`'s imports/helpers (copy `lid`, `b_reg`, `sem`, `fdef`, `module_of`, `compute_of`, `analyzed_caller`, `caller_own`, `own_unique`, `own_shared`). Register it in `boot/tests/main.tw` (add to the suite list next to the other cfg suites). First test — a caller recovering `out.ctx`:

```tw
.test(
  "Case W: out := helper(ctx); ctx = out.ctx keeps ctx Unique",
  fn() {
    // helper(x) returns Record{f0:x}; ret_paths [.f0]=from(p0).
    // caller(c): let out = helper(c); let g = record_get out.f0; assign c = g; c
    funcs := case_w_fixture()   // builds helper (id 1) + caller (id 2)
    f := analyzed_caller(funcs, "caller")
    // after the block, c (local 0) is Unique.
    try assert.equal(caller_own(f, 0), own_unique())
    .Ok({})
  },
)
```

Add `case_w_fixture()` building the two functions with the exact ANF (helper returns `ARecord{f0: p0}`; caller does `ACall(helper, [c]) -> out`, `ARecordGet(out, f0) -> g`, `AAssign(c, g)`, return `c`).

- [ ] **Step 2: Run — expect RED**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: `c` is not Unique — the call result carries no field_own and the gate isn't applied yet.

- [ ] **Step 3: Thread `last` and apply the gate**

Change `transfer_summarized_call` (`:976`) signature to accept `last: Vector<Int>` and the `args`' pre-call facts. Its caller `transfer_call` (`:844`) already has `last`; pass it through. After the existing `params`/`ret` handling, add:

```tw
// Return-path recovery. OwnedFromParam(k) refers to args[k].
result_fields := ff.empty()
result_pp: Dict<Int, Vector<Int>> = Dict.new()
for rp in s.ret_paths {
  own_here := case rp.own {
    .OwnedFresh => true,
    .OwnedFromParam(k) => if k < args.len() {
      unique := own_is_unique(st.own, atom_local_id_or(args[k], -1)) and is_atom_last_use(last, args[k])
      if !unique {
        st = .publish_atom(args[k])   // publish-on-fail: may alias args[k]'s region
      }
      unique
    } else { false },
  }
  if own_here {
    // record the recovered path on the result; provenance follows args[k]
    origins := case rp.own { .OwnedFromParam(k) => prov_of(st.prov, args[k]), .OwnedFresh => [] }
    st = record_ret_path(st, result, rp, origins, result_fields_ref)  // see helper
  }
}
```

Because Twinkle is immutable, structure this as building `result_fields`/`result_pp` then a single `set_field_own(result, result_fields).set_path_prov(result, result_pp)` at the end (only when the result shell is Unique, i.e. `ret` was OwnedFresh). Implement `record_ret_path` inline: for `via == Direct`, `field == Some(f)` → set `[.f]` in `result_fields`/`result_pp`; for `via == Variant(tag,i)` → set `[Payload(tag,i)]` (+ `[Payload(tag,i), .f]` when `field == Some(f)`). Add helpers `atom_local_id_or`, `is_atom_last_use` (wrap `is_last_use` over `atom_local_id`).

- [ ] **Step 4: Run — expect PASS (Case W caller)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: after `ctx = out.ctx`, `c` is Unique (the existing `ARecordGet` move recovers `[.f0]` from the result's field_own once the whole-record last-use / transport move applies — for this fixture `out` is dead after the projection, so Phase 4's whole-record last-use already fires).

- [ ] **Step 5: Add the gate-failure publish test**

```tw
.test(
  "gate fail: ctx read after the call is published (not left Unique)",
  fn() {
    // caller2(c): let out = helper(c); let g = record_get out.f0; let _ = record_get c.f9; ...
    // c is read again after the call -> not last-use at the call -> publish c.
    funcs := case_w_gatefail_fixture()
    f := analyzed_caller(funcs, "caller")
    try assert.equal(caller_own(f, 0), own_shared())  // c published
    .Ok({})
  },
)
```

Run — expect PASS.

- [ ] **Step 6: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw boot/tests/main.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw boot/tests/main.tw
git commit -m "ownership: recover return paths at the caller with publish-on-fail

transfer_summarized_call now takes last and, per ret_path, gates OwnedFromParam(k)
on args[k] Unique+last-use: on success the result carries the recovered field/
payload facts (provenance = args[k]'s), on failure it publishes args[k] rather
than leaving an unpublished alias. OwnedFresh paths are unconditional.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage G — Transport-wrapper projection move

### Task 11: Bounded transport recognizer + `ARecordGet` move condition

**Files:**
- Modify: `boot/compiler/ownership.tw` (`BlockPrep` `:511`, `block_prep` `:513`, `ARecordGet` transfer `:1107`, add `recognize_transport_moves`)
- Test: `boot/tests/suites/cfg_return_paths_suite.tw`

- [ ] **Step 1: Write the failing sibling-read test**

```tw
.test(
  "transport move: out.ctx moves even though out.ty is read after",
  fn() {
    // caller(c): out=helper(c); g=record_get out.f0; ty=record_get out.f1; assign c=g; use ty; c
    // out is live for out.f1, but out.f0 is dead-through-out -> move licensed.
    funcs := case_w_sibling_fixture()
    f := analyzed_caller(funcs, "caller")
    try assert.equal(caller_own(f, 0), own_unique())
    .Ok({})
  },
)
```

- [ ] **Step 2: Run — expect RED**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: `c` not Unique — Phase 4 whole-record last-use fails because `out` is live for `out.f1`, and no transport recognizer exists yet.

- [ ] **Step 3: Add `recognize_transport_moves`**

Add a block-local recognizer mirroring `recognize_quartet_moves` (which returns `Dict<Int,Bool>` of licensed `ARecordGet` result-locals). A projection `R = record_get out.f` is transport-move-licensed when, strictly after it in the block: `out.f` is not read again; `out` is not published/returned/stored/passed-to-call/aliased/used-by-terminator-or-successor-arg **except** for other `record_get out.g` sibling reads; and `out` is dead after the block (reuse `exit_mentions_local` `:210`). Model it on `quartet_ok` (`:242`) — scan forward, allow sibling `ARecordGet(out, g)` (g != f) and their pure downstream reads, reject any other mention of `out` or a second `record_get out.f`.

```tw
fn transport_ok(blk: CfgBlock, scan: BlockScan, i: Int, projected: Int, out_id: Int, fid: Int) Bool {
  insts := blk.instructions
  for j in range_from(i + 1, insts.len()) {
    op := insts[j].op
    // a second read of out.f fails
    if is_record_get_of(op, out_id, fid) { return false }
    // sibling read out.g (g != fid) is allowed and does not mention-fail
    is_sibling := case op {
      .ARecordGet(base, f2, _) => atom_is_local(base, out_id) and f2.id != fid,
      _ => false,
    }
    if is_sibling { } else if op_mentions_local(op, out_id) {
      return false   // any other mention of out fails
    }
  }
  !exit_mentions_local(blk, out_id)
}
fn recognize_transport_moves(blk: CfgBlock, scan: BlockScan) Dict<Int, Bool> {
  out: Dict<Int, Bool> = Dict.new()
  for inst, i in blk.instructions {
    case inst.op {
      .ARecordGet(base, f, _) => case atom_local_id(base) {
        .Some(bid) => if transport_ok(blk, scan, i, inst.anf_local.id, bid, f.id) {
          out[inst.anf_local.id] = true
        },
        .None => {},
      },
      _ => {},
    }
  }
  out
}
```

Extend `BlockPrep` (`:511`) with `transport: Dict<Int, Bool>` and set it in `block_prep` (`:513`): `transport: recognize_transport_moves(blk, scan)`. Thread it into `forward_block_body` → `transfer_op` next to `quartet`.

- [ ] **Step 4: Consult it in `ARecordGet`**

In the `ARecordGet` transfer (`:1110`), change the move condition:

```tw
.Some(bid) => if is_last_use(last, bid) or quartet_has(quartet, result) or transport_has(transport, result) {
```

Add `transport_has` (twin of `quartet_has`). The move mechanics (transfer `[.f]*`, `remove_prefix` on base) are unchanged.

- [ ] **Step 5: Run — expect PASS**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: sibling-read test passes; the earlier Case W tests still pass.

- [ ] **Step 6: Add a borrow negative (published `out`)**

```tw
.test(
  "transport borrow: publishing out forces borrow, both demoted",
  fn() {
    // caller(c): out=helper(c); g=record_get out.f0; global_set G0 = out; ...
    funcs := case_w_published_out_fixture()
    f := analyzed_caller(funcs, "caller")
    try assert.equal(caller_own(f, 0), own_shared())   // no clean recovery
    .Ok({})
  },
)
```

Run — expect PASS.

- [ ] **Step 7: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw
git commit -m "ownership: bounded transport-wrapper projection move

A block-local recognizer licenses out.f to move even while out stays live for
sibling reads (out.g), the finer path-liveness Phase 4 deferred; publishing/
storing/re-reading out falls back to borrow. Wired through BlockPrep and the
ARecordGet move condition, mirroring the quartet recognizer.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage H — Match-arm payload seeding + Case R

### Task 12: CFG per-arm payload metadata

**Files:**
- Modify: `boot/compiler/cfg.tw` (`CfgBlock` `:55`, `build_match` `:531`, add `PayloadSrc`)
- Test: `boot/tests/suites/cfg_return_paths_suite.tw` (structural)

- [ ] **Step 1: Add `PayloadSrc` and the block field**

In `cfg.tw`:

```tw
// Set only for a match-arm block whose incoming pattern is a top-level
// Variant(tag, [Var(v)]) single-payload binding; drives Phase 5 payload seeding.
pub type PayloadSrc = .{ scrutinee: Int, variant_tag: Int, payload_index: Int, binding: Int }
```

Add `payload_src: PayloadSrc?` to `CfgBlock` (`:55`), default `.None` in every `CfgBlock.{ … }` builder (search the file; the primary one is near `:320` where `bound: []`).

- [ ] **Step 2: Populate it in `build_match`**

`build_match` (`:531`) sets `bound`; also detect a top-level single-`Var` variant pattern and set `payload_src`. Add a recognizer:

```tw
// Only Variant(_, vid, [Var(v)]) with a single Var payload; else .None (under-claim).
fn arm_payload_src(scrutinee: Atom, pattern: CorePattern) PayloadSrc? {
  case atom_local_id(scrutinee) {
    .Some(sid) => case pattern {
      .Variant(_, vid, subs) => if subs.len() == 1 {
        case subs[0] {
          .Var(v) => .Some(PayloadSrc.{ scrutinee: sid, variant_tag: vid.id, payload_index: 0, binding: v.id }),
          _ => .None,
        }
      } else { .None },
      _ => .None,
    },
    .None => .None,
  }
}
```

In the arm loop (`:543`), after `set_block_bound`, set the payload src:

```tw
ctx = .set_block_payload_src(nb.id, arm_payload_src(scrutinee, arm.pattern))
```

Add `set_block_payload_src` mirroring `set_block_bound` (`:525`).

- [ ] **Step 3: Write a structural test**

Assert that for a `case scrutinee { .Ok(v) => v, ... }` module, the arm block has `payload_src` set with the right tag/binding. Build via `cfg.build_view` and inspect `f.blocks`. Add the test to the new suite.

- [ ] **Step 4: Run — expect PASS**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: metadata present on the `.Ok`/`.Err` arm blocks; `.None` for non-`Var` patterns.

- [ ] **Step 5: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/cfg.tw boot/tests/suites/cfg_return_paths_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/cfg.tw boot/tests/suites/cfg_return_paths_suite.tw
git commit -m "cfg: record per-arm payload-projection metadata for match seeding

A match-arm block over a top-level Variant(tag, [Var(v)]) pattern now records
(scrutinee, variant_tag, payload_index, binding) so Phase 5 can seed the bound
payload local from the scrutinee's tagged payload facts; other patterns
under-claim (None).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

### Task 13: Seed the payload-bound local; Case R end-to-end

**Files:**
- Modify: `boot/compiler/ownership.tw` (block-entry seeding in `forward_block_body`/the per-block entry state builder near `:2328`, using `blk.payload_src`)
- Test: `boot/tests/suites/cfg_return_paths_suite.tw`

- [ ] **Step 1: Write the failing Case R test**

```tw
.test(
  "Case R: case load(state){.Ok(v)=>v, .Err(e)=>return ...} keeps loaded.state Unique",
  fn() {
    // load(s) -> Result-shaped variant with Ok[0].state=from(p0), Err[0].state=from(p0).
    // caller(acc): out=load(acc); case out { .Ok(v)=> v ; .Err(e)=> return e } ; ... use v.state
    funcs := case_r_fixture()
    f := analyzed_caller(funcs, "caller")
    // the Ok arm's bound payload v recovers .state Unique; assert via the join local.
    try assert.equal(caller_own(f, ok_arm_state_local()), own_unique())
    .Ok({})
  },
)
```

`case_r_fixture()` builds `load` returning an `AVariant` wrapping `ARecord{state: p0}` on the Ok path (and similarly on Err), and a caller that matches. Keep it minimal but exercising the tagged payload + record field.

- [ ] **Step 2: Run — expect RED**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the payload-bound `v` is seeded Unknown (current behavior), so `.state` is not Unique.

- [ ] **Step 3: Seed the payload-bound local at block entry**

Where per-block entry `ForwardState` is built for the forward pass (the entry builder that today seeds params/joins — near `:2328` and in `run_fixpoint`), when `blk.payload_src` is `.Some(ps)`, project the scrutinee's `[Payload(tag,i)]*` facts into the bound local:

```tw
case blk.payload_src {
  .Some(ps) => {
    src_fields := st.field_own_get(ps.scrutinee)
    src_pp := st.path_prov_get(ps.scrutinee)
    proj := src_fields.project(.Payload(ps.variant_tag, ps.payload_index))
    case proj.shell {
      .Some(t) => {
        st = .set_own_st(ps.binding, ff_tag_to_own(t))
        st = .set_field_own(ps.binding, proj.fields)
        st = .set_path_prov(ps.binding, project_path_prov(src_pp, .Payload(ps.variant_tag, ps.payload_index)))
        // move: remove the payload subtree from the scrutinee when it is dead after the match
        if scrutinee_dead_after(blk, ps.scrutinee) {
          st = .set_field_own(ps.scrutinee, src_fields.remove_prefix(.Payload(ps.variant_tag, ps.payload_index)))
        }
      },
      .None => {},   // no owned payload fact -> leave bound local Unknown (seed as today)
    }
  },
  .None => {},
}
```

Add `project_path_prov` (mirror `ff.project` for the parallel `path_prov` map) and `ff_tag_to_own`. `scrutinee_dead_after` reuses `exit_mentions_local` + block liveness. The `.Err` arm returns (leaf), so it contributes nothing to any join — no special handling needed.

- [ ] **Step 4: Run — expect PASS**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: the Ok-arm payload `v` recovers `.state` Unique; the Err/return arm does not corrupt the join.

- [ ] **Step 5: Add the tag-isolation negative**

```tw
.test(
  "tag isolation: Err arm cannot recover an Ok-only payload fact",
  fn() {
    // load returns Ok with owned payload, Err with a SHARED/foreign payload.
    funcs := case_r_tag_isolation_fixture()
    f := analyzed_caller(funcs, "caller")
    try assert.equal(caller_own(f, err_arm_payload_local()), own_shared())
    .Ok({})
  },
)
```

Run — expect PASS (the `[Payload(Err,0)]` projection finds no owned fact).

- [ ] **Step 6: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/ownership.tw boot/tests/suites/cfg_return_paths_suite.tw
git commit -m "ownership: seed match-arm payload locals from tagged scrutinee facts

A top-level Variant(tag,[Var(v)]) arm seeds v by projecting the scrutinee's
[Payload(tag,i)]* facts (field_own + path_prov), moving them out when the
scrutinee is dead after the match; the tag keeps arms disjoint so an Err arm
cannot recover an Ok payload. Completes Case R: handled Result state stays
Unique while return arms are leaf exits.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Stage I — Rendering + full verification

### Task 14: Render the transport verdict; determinism

**Files:**
- Modify: `boot/compiler/cfg.tw` or `boot/compiler/summary.tw` (`render_view`/`render_cfg` path), `boot/compiler/ownership.tw` (verdict string)
- Test: `boot/tests/suites/cfg_return_paths_suite.tw`

- [ ] **Step 1: Write the determinism test**

```tw
.test(
  "cfg render with ret_paths + transport verdict is byte-identical across builds",
  fn() {
    funcs := case_w_sibling_fixture()
    b := b_reg()
    a := render_cfg_for_entry_of(funcs)   // helper: build_view -> summary.compute -> render_cfg
    b2 := render_cfg_for_entry_of(funcs)
    try assert.equal(a == b2, true)
    .Ok({})
  },
)
```

- [ ] **Step 2: Run — expect RED if the verdict/summary line isn't rendered yet**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: RED only if the render helper isn't wired; otherwise PASS (determinism should already hold — this test also guards it).

- [ ] **Step 3: Add the transport verdict to `render_view`**

At each `ARecordGet` transport site, print `transport=move([.f] from pK)` / `transport=borrow(<reason>)` alongside Phase 4's shell/field verdict, driven by `BlockPrep.transport` and the classified `path_prov`. The `ret_paths` summary line already renders via `render_summary` (Task 6). Keep every rejection reasoned (no silent bail).

- [ ] **Step 4: Run — expect PASS + eyeball a real dump**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Then eyeball: `target/twk ir boot/compiler/checker.tw --cfg 2>&1 | grep -E 'ret_paths|transport' | head`
Expected: transport helpers in the real checker show `ret_paths` and `transport=` verdicts; determinism test green.

- [ ] **Step 5: fmt + lint + commit**

```bash
target/twk fmt boot/compiler/ownership.tw boot/compiler/summary.tw boot/compiler/cfg.tw boot/tests/suites/cfg_return_paths_suite.tw
target/twk lint boot/main.tw
git add -A
git commit -m "cfg/ownership: render ret_paths summary and transport-move verdict

twk ir --cfg now prints the per-function ret_paths line and, per transport
projection site, move([.f] from pK) or borrow(reason), keeping the print-facts
discipline; output is deterministic (byte-identical across builds).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

### Task 15: Full verification + census guard + self-host

**Files:** none (verification only), plus doc bookkeeping.

- [ ] **Step 1: Census still zero in-place**

Run: `target/twk ir boot/main.tw --census 2>&1 | tail -20`
Expected: **0 in-place** (Phase 5 changes no codegen). If nonzero, a transfer edit leaked into a decision path — stop and investigate.

- [ ] **Step 2: Full boot suite (sequential)**

Run: `make quick-bundle-cli && target/twk run boot/tests/main.tw`
Expected: all suites green, including the re-baselined `cfg_summary_suite` and the new `cfg_return_paths_suite`.

- [ ] **Step 3: Self-host fixed point (sequential, not backgrounded)**

Run: `make stage2`
Expected: reaches the self-host fixed point (boot compiles boot to a stable `target/boot.wasm`). Phase 5 is boot-only and adds no stage0-parity construct, so stage0 needs no change; if `make stage2` fails in stage0, a Phase 5 construct leaked into boot *source* usage — revert that usage (analysis code must not require new stage0 support).

- [ ] **Step 4: Rust reference sanity (targeted)**

Run: `cargo test --release -p twinkle --test cow_analysis -- --ignored --nocapture` (reference distribution only; not a gate). Confirm it still runs.

- [ ] **Step 5: Update the analysis README checkboxes**

In `docs/plans/sound-uniqueness/analysis/README.md`, tick the four Phase 5 bullets (`[x]`) and change the Phase 5 status line / the top `README.md` "Current focus" to reflect Phase 5 done, Phase 6 next. Follow the memory guidance: on completion, remove the plan's row from any plans index (there is none here) — just update the phase status.

- [ ] **Step 6: Commit the bookkeeping**

```bash
target/twk fmt docs/plans/sound-uniqueness/analysis/README.md 2>/dev/null || true
git add docs/plans/sound-uniqueness/
git commit -m "docs/sound-uniqueness: mark Phase 5 return-path summaries done

Cases W and R classify as ownership-preserving handoffs; generated code
unchanged (census still 0 in-place). Phase 6 (ownership-specialization decision
facts) is next.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-review checklist (run before executing)

- **Spec coverage:** Task 1 (tagged payload segment + codec) ↔ Decision 4; Tasks 2–4 (path_prov + shell/field split + AVariant) ↔ Decision 2; Task 5 (Return leaf) ↔ Decision 3; Tasks 6–8 (ret_paths schema + Direct/Variant classification + shell ret) ↔ Decisions 1, 7, 8; Task 9 (fixpoint hide/strip + path_prov join) ↔ Decisions 10 + review pt 4; Task 10 (caller gate + publish-on-fail) ↔ Decision 5 + review pt 1; Task 11 (transport recognizer) ↔ Decision 9 / Fork 2A; Tasks 12–13 (cfg metadata + payload seeding) ↔ Fork 3-i + Blocker 1; Task 14 (render) ↔ design "Rendering"; Task 15 ↔ acceptance 10–12. Acceptance 1–9 map to tests across Tasks 5, 7, 8, 10, 11, 13, 1.
- **No parameter `in_place_paths` / specialization** appears in any task (correctly Phase 6).
- **Type consistency:** `PathSeg.Payload(Int,Int)`, `ReturnPathOwn.{via,field,own}`, `RetVia.Variant(Int,Int)`, `PayloadSrc.{scrutinee,variant_tag,payload_index,binding}` are used identically across tasks. `path_prov` is `Dict<Int, Dict<Int, Vector<Int>>>` throughout.
- **Known cross-task dependency:** the Task 3/4 `ret_tag == 0` tests only go green at Task 5 (labeled). Execute Tasks 3→4→5 as a unit if strict per-task green is required; otherwise accept the labeled RED until Task 5.
- **Fixture helpers** (`case_w_fixture`, `case_r_fixture`, etc.) are named per task; implement each in the suite when first referenced, mirroring `cfg_summary_suite.tw`'s ANF-builder style.
