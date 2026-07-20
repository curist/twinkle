# Lattice Generalization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collapse the five parallel per-lattice dataflow helpers in `ownership.tw`
(`merge_*_exit_targeted`, `same_*_map`, `*_map_get`) into three generic functions
driven by a `Lattice<T>` capability record, with byte-identical analysis output.

**Architecture:** The fixpoint in `run_fixpoint` threads five dataflow lattices
(own / valid / prov / field_own / path_prov). Three of them (own/valid/prov) use an
identical *targeted-widening* merge, and all five use structurally identical
change-detection (`same_*_map`) and nested-map access (`*_map_get`). Introduce one
generic `merge_targeted<T>`, one `same_map<T>`, and one `nested_get<T>`, each
parameterized by a small per-lattice witness (`Lattice<T> = .{ default, join, eq }`
for merge; a bare `eq` closure for `same_map`; nothing for `nested_get`). This is a
pure refactor — the analysis is debug-only (`twk ir --cfg`/`--census`, reachable only
from `commands/ir.tw`), generated code is untouched, and every existing suite plus the
census must stay green.

**Tech Stack:** Twinkle boot compiler (`boot/compiler/ownership.tw`), boot test
runner (`target/twk test` via `make boot-test`), `twk ir --cfg` for golden diffing.

**On the commit steps:** each task ends with a `git commit`. These assume the
executor is authorized to commit. If commits require human sign-off in this session,
treat every commit step as "stage the listed files and pause for review" instead —
run the `git add` but hold the `git commit` until approved.

**Feasibility:** De-risked by a throwaway spike (run 2026-07-20) that exercised the
generic `merge_targeted<T>` / `same_map<T>` over `T = Int`, `Bool`, and
`Vector<Int>`, confirming: generic `fn`s over `T` with `Dict<Int, T>` threading,
`fn`-typed capability fields called as `lat.join(a,b)` / `lat.eq(a,b)`, generic
records `MergeOut<T>`, and top-level `fn`s passed as field values all compile and run
correctly through monomorphization. No language limitation was found.

---

## Scope

**In scope:** the mechanically-identical trio. Sizing (`ownership.tw`):

| Family | Current defs | Generalizes to |
|---|---|---|
| `merge_own/valid/prov_exit_targeted` (+ `MergeOwn/Valid/ProvOut` types) | 3 fns + 3 types | `merge_targeted<T>` + `MergeOut<T>` |
| `same_own/valid/prov/field_own/path_prov_map` | 5 fns | `same_map<T>` |
| `own/valid/prov/field_own/path_prov_map_get` | 5 fns | `nested_get<T>` |

**Out of scope (documented follow-ups, not this plan):**
- `join_entry_ownership` / `join_entry_valid` / `join_entry_prov` — structurally
  parallel but each has a different identity element, combine op, per-atom lookup, and
  store-policy, plus the `_assumed` loop-seed wrapper. A richer capability could unify
  them; the payoff is smaller and the risk higher. Leave as-is.
- Extracting the generic core into a new leaf module `boot/compiler/lattice.tw`. Kept
  in `ownership.tw` here to avoid moving the shared int-vector helpers
  (`insert_sorted`, `int_keys_union`, `live_contains_int`); revisit when the file is
  split.
- The `merge_field_own_exit` / `merge_path_prov_exit` plain meets (no widening/lock)
  stay as their own functions; only their `same_*_map` and `*_map_get` companions are
  generalized.

## File Structure

- **Modify:** `boot/compiler/ownership.tw`
  - Add generic core (`Lattice<T>`, `MergeOut<T>`, `merge_targeted<T>`, `same_map<T>`,
    `nested_get<T>`, `join_own_tag`) near the merge section (after `int_keys_union`,
    line ~2275).
  - Add five witnesses (`own_lat`, `valid_lat`, `prov_lat`, and `eq` closures for
    field_own/path_prov).
  - Rewrite the ~11 change-detection / merge / accessor call sites in `run_fixpoint`
    and elsewhere to the generic forms.
  - Delete the 3 old merges + 3 `Merge*Out` types + 5 `same_*_map` + 5 `*_map_get`.
- **Create:** `boot/tests/suites/cfg_lattice_suite.tw` — unit tests that lock the
  generic core against a known table (the spike, promoted to a real test).
- **Modify:** `boot/tests/main.tw` — register the new suite.
- **Modify (on completion, Task 6):** `docs/plans/sound-uniqueness/analysis/README.md`
  — record the cleanup; `docs/plans/README.md` — drop the active-index row; move this
  doc to `docs/plans/archive/`.

## Preserved invariants (the regression gate)

This is a refactor: **no behavior may change.** After every task:
1. `make boot-test` → `Ran <N> tests: <N> passed` (baseline: 3091 passed, 2026-07-20).
2. Census stays 0 in-place (asserted by `uniqueness_census_suite.tw`).
3. `twk ir --cfg` output is **byte-identical** to the pre-refactor golden (Task 1
   captures it; Task 6 diffs against it).

---

### Task 1: Capture the byte-identical golden baseline

**Files:**
- Create: `/tmp/cfg_golden_before.txt` (throwaway; not committed)

- [ ] **Step 1: Confirm the suite is green before touching anything**

Run: `set -o pipefail; make boot-test 2>&1 | grep -E 'passed|failed'`
Expected: `Ran 3091 tests: 3091 passed` (or the current N; record it).

- [ ] **Step 2: Capture the CFG golden over every sound-uniqueness fixture**

Run (glob the whole fixture dir so nothing is missed — `multi_param_return_alias.tw`
and `mutual_recursion_thread.tw` don't match a `*_main`/`vector_*`/`*sieve*` pattern;
`_lib.tw` files are valid standalone entries and dumping them is harmless and keeps the
golden complete):
```bash
cd /Users/curist/playground/rust/twinkle
for f in boot/tests/fixtures/cfg/sound_uniqueness/*.tw; do
  echo "===== $f ====="; target/twk ir "$f" --cfg 2>&1
done > /tmp/cfg_golden_before.txt
wc -l /tmp/cfg_golden_before.txt
```
Expected: a non-empty dump (hundreds of lines) covering all 22 fixtures. This is the
byte-for-byte golden.

- [ ] **Step 3: Commit nothing** — this task only records state. Proceed to Task 2.

---

### Task 2: Add the generic core + unit test (no call sites changed yet)

**Files:**
- Modify: `boot/compiler/ownership.tw` (insert after `int_keys_union`, ~line 2275)
- Create: `boot/tests/suites/cfg_lattice_suite.tw`
- Modify: `boot/tests/main.tw`

- [ ] **Step 1: Add the generic core to `ownership.tw`**

Insert immediately after `int_keys_union` (currently ends ~line 2275):

```tw
// ── Generic dataflow-lattice core (shared by own/valid/prov widening,
//    and by every lattice's change-detection + nested-map access) ─────
// A per-lattice witness: the absent-key default, the meet (join toward the
// conservative top), and value equality. `merge_targeted` needs all three;
// `same_map` needs only `eq`; `nested_get` needs neither.
pub type Lattice<T> = .{ default: T, join: fn(T, T) T, eq: fn(T, T) Bool }

pub type MergeOut<T> = .{ map: Dict<Int, T>, locked: Vector<Int> }

fn lat_get<T>(m: Dict<Int, T>, k: Int, dflt: T) T {
  case m.get(k) {
    .Some(v) => v,
    .None => dflt,
  }
}

// Targeted-widening exit merge (replaces merge_own/valid/prov_exit_targeted).
// Keep exact exit replacement (`out := next`) while a fact converges; lock a local
// to the conservative meet once it oscillates back to `prev`, or once the per-block
// changed-visit cap forces it. Locked locals accumulate across visits.
// `pub` so the cfg_lattice_suite can lock behavior directly (matches how the other
// cfg suites exercise internal ownership/field_facts helpers).
pub fn merge_targeted<T>(
  old: Dict<Int, T>,
  next: Dict<Int, T>,
  prev: Dict<Int, T>,
  locked: Vector<Int>,
  has_prev: Bool,
  force_lock_changed: Bool,
  lat: Lattice<T>,
) MergeOut<T> {
  out := next
  next_locked := locked
  keys := int_keys_union(old.keys(), next.keys())
  for k in keys {
    old_x := lat_get(old, k, lat.default)
    next_x := lat_get(next, k, lat.default)
    prev_x := lat_get(prev, k, lat.default)
    changed := !lat.eq(old_x, next_x)
    oscillates := has_prev and changed and lat.eq(next_x, prev_x)
    if live_contains_int(next_locked, k) or oscillates or force_lock_changed and changed {
      next_locked = insert_sorted(next_locked, k)
      out[k] = lat.join(old_x, next_x)
    }
  }
  MergeOut.{ map: out, locked: next_locked }
}

// Structural equality over a nested-map value (replaces same_*_map). Keys come from
// `a`, so a length check plus per-key presence-and-eq in `b` is exact.
pub fn same_map<T>(a: Dict<Int, T>, b: Dict<Int, T>, eq: fn(T, T) Bool) Bool {
  if a.keys().len() != b.keys().len() {
    return false
  }
  for k in a.keys() {
    case b.get(k) {
      .Some(bv) => case a.get(k) {
        .Some(av) => if !eq(av, bv) {
          return false
        },
        .None => {},
      },
      .None => return false,
    }
  }
  true
}

// Inner map for a block id, empty if absent (replaces *_map_get).
pub fn nested_get<T>(m: Dict<Int, Dict<Int, T>>, id: Int) Dict<Int, T> {
  case m.get(id) {
    .Some(v) => v,
    .None => Dict.new(),
  }
}

// The own lattice stores Ownership *tags* (0=Unique,1=Shared,2=Unknown); its meet
// routes through join_own so Unknown dominates, then Shared, then Unique.
pub fn join_own_tag(a: Int, b: Int) Int {
  own_of_tag(a).join_own(own_of_tag(b)).own_tag()
}
```

- [ ] **Step 2: Add the new suite locking the generic (promoted spike)**

Create `boot/tests/suites/cfg_lattice_suite.tw`. This mirrors the exact API used by
the sibling suites: `use @std.testing.assert as assert` / `use @std.testing as
runner`, a `pub fn suite() runner.Suite` built with `runner.suite(name).test(desc,
fn() { ... })`, and `try assert.is_true(...)` / `try assert.equal(a, b)` assertions
inside each test closure. Compiler helpers are imported module-qualified
(`use compiler.ownership`), so calls read `ownership.merge_targeted(...)`.

```tw
use @std.testing.assert as assert
use @std.testing as runner

use compiler.ownership
use compiler.ownership.{Lattice}

fn ints_eq(a: Int, b: Int) Bool {
  a == b
}

fn own_witness() Lattice<Int> {
  Lattice.{ default: 2, join: ownership.join_own_tag, eq: ints_eq }
}

// Bool (valid) and Vector<Int> (prov) witnesses are self-contained so the suite
// exercises the generic across all three instantiated T without depending on
// ownership.tw internals (union_sorted/same_live are private there).
fn and_b(a: Bool, b: Bool) Bool {
  a and b
}

fn eq_b(a: Bool, b: Bool) Bool {
  a == b
}

fn contains_i(v: Vector<Int>, x: Int) Bool {
  for e in v {
    if e == x {
      return true
    }
  }
  false
}

fn union_i(a: Vector<Int>, b: Vector<Int>) Vector<Int> {
  out := a
  for x in b {
    if !contains_i(out, x) {
      out = .append(x)
    }
  }
  out
}

fn same_i(a: Vector<Int>, b: Vector<Int>) Bool {
  if a.len() != b.len() {
    return false
  }
  for x, i in a {
    if x != b[i] {
      return false
    }
  }
  true
}

fn valid_witness() Lattice<Bool> {
  Lattice.{ default: true, join: and_b, eq: eq_b }
}

fn prov_witness() Lattice<Vector<Int>> {
  Lattice.{ default: [], join: union_i, eq: same_i }
}

fn get_or_neg(m: Dict<Int, Int>, k: Int) Int {
  case m.get(k) {
    .Some(v) => v,
    .None => 0 - 1,
  }
}

fn get_or_true(m: Dict<Int, Bool>, k: Int) Bool {
  case m.get(k) {
    .Some(v) => v,
    .None => true,
  }
}

fn get_or_empty(m: Dict<Int, Vector<Int>>, k: Int) Vector<Int> {
  case m.get(k) {
    .Some(v) => v,
    .None => [],
  }
}

pub fn suite() runner.Suite {
  runner
    .suite("cfg lattice")
    .test("merge_targeted own: force-lock joins Unique+Shared to Shared", fn() {
      old: Dict<Int, Int> = Dict.new()
      old[5] = 0 // Unique
      next: Dict<Int, Int> = Dict.new()
      next[5] = 1 // Shared
      res := ownership.merge_targeted(old, next, Dict.new(), [], false, true, own_witness())
      try assert.equal(res.locked.len(), 1)
      try assert.equal(res.locked[0], 5)
      try assert.equal(get_or_neg(res.map, 5), 1)
      .Ok({})
    })
    .test("merge_targeted own: unchanged, unlocked key takes next exactly", fn() {
      old: Dict<Int, Int> = Dict.new()
      old[3] = 0
      next: Dict<Int, Int> = Dict.new()
      next[3] = 0
      res := ownership.merge_targeted(old, next, Dict.new(), [], false, false, own_witness())
      try assert.equal(res.locked.len(), 0)
      try assert.equal(get_or_neg(res.map, 3), 0)
      .Ok({})
    })
    .test("merge_targeted own: oscillation back to prev locks + meets", fn() {
      old: Dict<Int, Int> = Dict.new()
      old[7] = 0 // Unique
      next: Dict<Int, Int> = Dict.new()
      next[7] = 1 // Shared
      prev: Dict<Int, Int> = Dict.new()
      prev[7] = 1 // next == prev => oscillates
      res := ownership.merge_targeted(old, next, prev, [], true, false, own_witness())
      try assert.equal(res.locked.len(), 1)
      try assert.equal(get_or_neg(res.map, 7), 1) // join_own(Unique,Shared)=Shared
      .Ok({})
    })
    .test("merge_targeted valid: force-lock meets true & false to false", fn() {
      old: Dict<Int, Bool> = Dict.new()
      old[2] = true
      next: Dict<Int, Bool> = Dict.new()
      next[2] = false
      res := ownership.merge_targeted(old, next, Dict.new(), [], false, true, valid_witness())
      try assert.equal(res.locked.len(), 1)
      try assert.is_false(get_or_true(res.map, 2))
      .Ok({})
    })
    .test("merge_targeted prov: force-lock unions param-origin sets", fn() {
      old: Dict<Int, Vector<Int>> = Dict.new()
      old[4] = [1]
      next: Dict<Int, Vector<Int>> = Dict.new()
      next[4] = [2]
      res := ownership.merge_targeted(old, next, Dict.new(), [], false, true, prov_witness())
      try assert.equal(res.locked.len(), 1)
      try assert.equal(get_or_empty(res.map, 4).len(), 2) // union([1],[2])
      .Ok({})
    })
    .test("same_map: equal compare true, differing compare false", fn() {
      a: Dict<Int, Int> = Dict.new()
      a[1] = 0
      b: Dict<Int, Int> = Dict.new()
      b[1] = 0
      c: Dict<Int, Int> = Dict.new()
      c[1] = 1
      try assert.is_true(ownership.same_map(a, b, ints_eq))
      try assert.is_false(ownership.same_map(a, c, ints_eq))
      .Ok({})
    })
    .test("nested_get: absent id yields empty inner map", fn() {
      outer: Dict<Int, Dict<Int, Int>> = Dict.new()
      inner: Dict<Int, Int> = Dict.new()
      inner[2] = 9
      outer[10] = inner
      try assert.equal(ownership.nested_get(outer, 10).len(), 1)
      try assert.equal(ownership.nested_get(outer, 99).len(), 0)
      .Ok({})
    })
}
```

Each test closure ends with `.Ok({})` because `runner.test` expects
`fn() Result<Void, String>` (`boot/stdlib/testing.tw`), and `try assert...` only
early-returns on failure — the success path must return `.Ok({})` explicitly, as the
sibling suites do.

Note on the `Lattice` import: `use compiler.ownership` binds the module alias (for
`ownership.merge_targeted`), while `use compiler.ownership.{Lattice}` brings the type
name into scope for the annotation — both lines are needed (same split as `@std.view`
in CLAUDE.md). If `assert.equal` is not the exact name, grep
`tests/suites/cfg_field_facts_suite.tw` for the real ones (`is_true`/`is_false`/
`equal` are used there) and adjust.

- [ ] **Step 3: Register the suite in `boot/tests/main.tw`**

Add the import next to the other suites (near line 28):
```tw
use .suites.cfg_lattice_suite
```
and add its case to the suite list (near line 285, beside `cfg_field_facts_suite.suite()`):
```tw
  cfg_lattice_suite.suite(),
```

- [ ] **Step 4: Build + run the new suite**

Run: `set -o pipefail; make boot-test 2>&1 | grep -E 'passed|failed|error'`
Expected: `Ran <N+7> tests: <N+7> passed` (7 new tests, nothing else changed). The
`set -o pipefail` makes a build/link failure fail the whole line even when `grep`
matches nothing. If it fails to build, the most likely causes are (a) a missing `pub`
on a generic being imported — the core functions are exported in Step 1, confirm the
`use compiler.ownership` names match; (b) an `assert` name mismatch — grep
`tests/suites/cfg_field_facts_suite.tw` for the exact assertion names in use.

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/ownership.tw boot/tests/suites/cfg_lattice_suite.tw boot/tests/main.tw
git commit -m "ownership: add generic lattice core (merge_targeted/same_map/nested_get)

Introduce a Lattice<T> capability record plus one generic targeted-widening
merge, change-detection, and nested-map accessor, with a unit suite locking
their behavior over Int/Bool/Vector<Int>. No call sites migrated yet."
```

---

### Task 3: Migrate the three widened merges to `merge_targeted<T>`

**Files:**
- Modify: `boot/compiler/ownership.tw` (`run_fixpoint` call sites ~2975-3004; delete
  `merge_own/valid/prov_exit_targeted` + `MergeOwn/Valid/ProvOut` types)

- [ ] **Step 1: Add the valid/prov witnesses next to the core**

After `join_own_tag`, add:

```tw
fn and_bool(a: Bool, b: Bool) Bool {
  a and b
}

fn bool_eq(a: Bool, b: Bool) Bool {
  a == b
}

fn own_lat() Lattice<Int> {
  Lattice.{ default: own_tag(.Unknown), join: join_own_tag, eq: fn(a, b) { a == b } }
}

fn valid_lat() Lattice<Bool> {
  Lattice.{ default: true, join: and_bool, eq: bool_eq }
}

fn prov_lat() Lattice<Vector<Int>> {
  Lattice.{ default: [], join: union_sorted, eq: same_live }
}
```

- [ ] **Step 2: Rewrite the three merge call sites in `run_fixpoint`**

Replace the `merge_own_exit_targeted(...)` / `merge_valid_exit_targeted(...)` /
`merge_prov_exit_targeted(...)` calls (currently ~2975-2998) with:

```tw
        own_merge := merge_targeted(
          old_own,
          st.own,
          own_map_get(prev_exits, blk.id.id),
          locked_get(locked_own, blk.id.id),
          has_prev,
          force_lock_changed,
          own_lat(),
        )
        valid_merge := merge_targeted(
          old_valid,
          st.valid,
          valid_map_get(prev_exit_valid, blk.id.id),
          locked_get(locked_valid, blk.id.id),
          has_prev,
          force_lock_changed,
          valid_lat(),
        )
        prov_merge := merge_targeted(
          old_prov,
          st.prov,
          prov_map_get(prev_exit_prov, blk.id.id),
          locked_get(locked_prov, blk.id.id),
          has_prev,
          force_lock_changed,
          prov_lat(),
        )
```
(The `own_map_get`/`valid_map_get`/`prov_map_get` accessors are migrated in Task 5;
leave them for now. `.map`/`.locked` field reads downstream are unchanged since
`MergeOut<T>` keeps those field names.)

- [ ] **Step 3: Delete the three old merges and their result types**

Delete `type MergeOwnOut` + `fn merge_own_exit_targeted` (~2289-2314),
`type MergeValidOut` + `fn merge_valid_exit_targeted` (~2395-2420), and
`type MergeProvOut` + `fn merge_prov_exit_targeted` (~2455-2480).

- [ ] **Step 4: Build + full suite**

Run: `set -o pipefail; make boot-test 2>&1 | grep -E 'passed|failed|error'`
Expected: same total as end of Task 2, all passed. A remaining reference to a deleted
name will surface as a build error — grep `merge_own_exit_targeted\|merge_valid_exit_targeted\|merge_prov_exit_targeted\|MergeOwnOut\|MergeValidOut\|MergeProvOut` and confirm zero hits.

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: migrate own/valid/prov exit merges to merge_targeted<T>

Replace the three byte-identical targeted-widening merges (and their MergeOut
result types) with one generic driven by per-lattice witnesses."
```

---

### Task 4: Migrate all five `same_*_map` to `same_map<T>`

**Files:**
- Modify: `boot/compiler/ownership.tw` (`run_fixpoint` change-detection ~3012-3017;
  delete the five `same_*_map`)

- [ ] **Step 1: Rewrite the change-detection disjunction in `run_fixpoint`**

Replace the `!same_own_map(...) or !same_valid_map(...) or ...` block (~3012-3017)
with the generic calls, passing each lattice's `eq`:

```tw
      if !already
        or !same_map(old_own, next_own, fn(a, b) { a == b })
        or !same_map(old_valid, next_valid, bool_eq)
        or !same_map(old_prov, next_prov, same_live)
        or !same_map(old_field, next_field, fn(a, b) { a.same(b) })
        or !same_map(old_pp, next_pp, same_pp_local) {
```
(`a.same(b)` is `ff.FieldMap.same`; `same_pp_local` already exists at ~2758.)

- [ ] **Step 2: Delete the five old `same_*_map`**

Delete `same_own_map` (~2251), `same_valid_map` (~2377), `same_prov_map` (~2430),
`same_field_own_map` (~2696), `same_path_prov_map` (~2740).

- [ ] **Step 3: Build + full suite**

Run: `set -o pipefail; make boot-test 2>&1 | grep -E 'passed|failed|error'`
Expected: same total, all passed. Grep `same_own_map\|same_valid_map\|same_prov_map\|same_field_own_map\|same_path_prov_map` → zero hits.

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: migrate five same_*_map change-detectors to same_map<T>"
```

---

### Task 5: Migrate all five `*_map_get` to `nested_get<T>`

**Files:**
- Modify: `boot/compiler/ownership.tw` (all call sites; delete the five accessors)

- [ ] **Step 1: Replace every accessor call**

`own_map_get(m, id)` / `valid_map_get(m, id)` / `prov_map_get(m, id)` /
`field_own_map_get(m, id)` / `path_prov_map_get(m, id)` all become
`nested_get(m, id)`.

**Do the definition deletions FIRST, then the call-site replace.** Two gotchas: (1)
macOS/BSD `sed` does **not** support `\b`; (2) `own_map_get` is a substring of
`field_own_map_get`, and `prov_map_get` of `path_prov_map_get`, so an unordered
replace corrupts the longer names into `field_nested_get(` / `path_nested_get(`.
Delete the five `fn *_map_get` definitions first (so the sed only touches call
sites), then run this **longest-name-first** ordered replace (no `\b` needed —
ordering guarantees the longer name is consumed before its substring):

```bash
cd /Users/curist/playground/rust/twinkle
sed -i '' \
  -e 's/field_own_map_get(/nested_get(/g' \
  -e 's/path_prov_map_get(/nested_get(/g' \
  -e 's/own_map_get(/nested_get(/g' \
  -e 's/valid_map_get(/nested_get(/g' \
  -e 's/prov_map_get(/nested_get(/g' \
  boot/compiler/ownership.tw
```
(Type inference resolves each `nested_get`'s `T` from the argument's map type.)

- [ ] **Step 2: Confirm exactly one `nested_get` definition remains**

Because the definition deletions were done first, only the generic `pub fn
nested_get<T>` (added in Task 2) should remain. Grep to confirm exactly one:

```bash
grep -n 'fn nested_get' boot/compiler/ownership.tw
```
Expected: a single line, the `<T>` generic. Remove any others.

- [ ] **Step 3: Build + full suite**

Run: `set -o pipefail; make boot-test 2>&1 | grep -E 'passed|failed|error'`
Expected: same total, all passed.

- [ ] **Step 4: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "ownership: migrate five *_map_get accessors to nested_get<T>"
```

---

### Task 6: Verify byte-identical output + format/lint + docs

**Files:**
- Modify: `docs/plans/sound-uniqueness/analysis/README.md`

- [ ] **Step 1: Re-capture the CFG dump and diff against the golden**

Run (same whole-dir glob as Task 1 Step 2, so the before/after sets match exactly):
```bash
cd /Users/curist/playground/rust/twinkle
for f in boot/tests/fixtures/cfg/sound_uniqueness/*.tw; do
  echo "===== $f ====="; target/twk ir "$f" --cfg 2>&1
done > /tmp/cfg_golden_after.txt
diff /tmp/cfg_golden_before.txt /tmp/cfg_golden_after.txt && echo "BYTE-IDENTICAL"
```
Expected: `BYTE-IDENTICAL` (empty diff; `diff` exits non-zero on any difference, so the
`&& echo` only prints when identical). Any diff is a behavior change — stop and bisect
which task's commit introduced it (`git stash`; re-run per commit).

- [ ] **Step 2: Format + lint the changed file**

Run:
```bash
target/twk fmt boot/compiler/ownership.tw boot/tests/suites/cfg_lattice_suite.tw
target/twk lint boot/main.tw
```
Expected: fmt idempotent (re-run yields no change); lint reports no new violations.
If lint flags a `*_map_get`-style helper-call that should be an inherent method, apply
the rewrite (mirrors commit `d0c650c6` for `variant_id`).

- [ ] **Step 3: Re-run the full suite after fmt (fmt can change semantics — see the
  `!()`-parens gotcha)**

Run: `set -o pipefail; make boot-test 2>&1 | grep -E 'passed|failed'`
Expected: all passed, same total.

- [ ] **Step 4: Confirm self-host still builds (the analysis lives in boot; a stage2
  break would mean stage0 can't compile the new generics)**

Run: `set -o pipefail; make stage2 2>&1 | tail -5`
Expected: builds `target/boot.wasm` with no error. (Analysis is `twk ir`-only, but
`boot/main.tw` imports `ownership`, so stage0 must still compile it.)

- [ ] **Step 5: Record the cleanup in the analysis README**

In `docs/plans/sound-uniqueness/analysis/README.md`, under a new "Post-completion
cleanups" note (or the deferrals table), add a line recording that the five parallel
dataflow lattices were unified behind `Lattice<T>` / `merge_targeted` / `same_map` /
`nested_get`, byte-identical, and that `join_entry_*` unification + `lattice.tw`
extraction remain optional follow-ups.

- [ ] **Step 6: Archive this plan**

This plan lives at `docs/plans/lattice-generalization.md` while active. On completion,
move it to the archive and drop its row from the active index:
```bash
cd /Users/curist/playground/rust/twinkle
git mv docs/plans/lattice-generalization.md docs/plans/archive/lattice-generalization.md
```
Then remove the "Lattice generalization" row from the Active Plan Index in
`docs/plans/README.md` (convention: delete the row and move the doc to `archive/`,
don't mark it Done in place).

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/ownership.tw boot/tests/suites/cfg_lattice_suite.tw \
        docs/plans/sound-uniqueness/analysis/README.md docs/plans/README.md \
        docs/plans/archive/lattice-generalization.md
git commit -m "ownership: verify lattice generalization byte-identical; archive plan

Full boot suite + census 0 + byte-identical twk ir --cfg over the
sound-uniqueness fixtures; self-host green. Notes join_entry_* unification
and lattice.tw extraction as optional follow-ups."
```

---

## Self-Review

- **Spec coverage:** the three families in the Scope table each get a migration task
  (3/4/5) plus a lock-down unit suite (2) and a byte-identical gate (1/6). ✓
- **Type consistency:** `MergeOut<T>` keeps the field names `.map`/`.locked` used by
  the existing `run_fixpoint` downstream reads (`next_own = own_merge.map` etc.), so
  those lines are untouched. `Lattice<T>` fields (`default`/`join`/`eq`) are used
  consistently by `merge_targeted`; `same_map` takes a bare `eq` closure (not the full
  witness) since it needs no default/join; `nested_get` takes no witness. ✓
- **Placeholder scan:** all code steps show full code; commands show expected output;
  no "TBD"/"handle edge cases". The one deferred adaptation (the `test`/`expect` API
  in Task 2 Step 2) is explicitly flagged with how to resolve it (copy the existing
  `cfg_field_facts_suite` header). ✓
- **Risk:** every task ends at a green full suite; Task 6 adds a byte-identical diff
  and self-host gate, so any semantic drift is caught at the task that caused it.
