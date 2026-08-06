# Fixpoint Map Semantic Types — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the ownership fixpoint's int-keyed maps/sets named nominal types (`LocalMap`/`LocalSet`/`BlockMap`/`BlockSet`) so a key's meaning (LocalId vs block id) and set-vs-map intent live in the type, not in comments — as a byte-identical, HAMT-backed refactor.

**Architecture:** Each semantic type is a thin nominal record wrapping the existing persistent `Dict` (`LocalMap<T> = .{ inner: Dict<Int, T> }`, etc.), keeping raw unboxed `Int` keys inside so the HAMT representation and performance are unchanged. Because Twinkle dispatches inherent methods by receiver type through the type's defining module, each type lives in its **own small module** (two types can't share a `get`/`set` method name in one module). No performance change is intended — a representation swap was investigated and rejected (see the design spec).

**Tech Stack:** Twinkle (self-hosted compiler, `boot/`), the boot test suite (`@std.testing`), `make stage2` self-host fixed point, byte-identical A/B verification.

**Design spec:** `docs/plans/fixpoint-map-intmap.md`.

## Global Constraints

- **Backing stays `Dict` (persistent HAMT).** No representation/perf swap — measured and rejected (`docs/plans/performance/compiler.md`, "measured-and-rejected").
- **Byte-identical emitted output** at every task: compiling `boot/main.tw` before and after a task must produce a byte-identical `.wasm`.
- **`make stage2` fixed point** must hold (the compiler rebuilt by itself reaches stage3 == stage4).
- **`make boot-test` green** at every task.
- **Raw `Int` keys inside every wrapper** — never key a `Dict` by a record type (`hash_key` only handles int/string; a record key traps).
- **New types live in their own modules under `boot/compiler/`**, imported by `ownership.tw` — never added inline to `ownership.tw` (function-index-shift crash class).
- Run `target/twk fmt <file>` and `target/twk lint <entry>` after editing any `.tw` file (project rule).

---

### Task 0: Capture the baseline

**Files:**
- Create: `docs/plans/fixpoint-map-baseline.txt` (scratch record; not committed to source)

**Interfaces:**
- Produces: the before-timings Task 2's perf gate compares against.

- [ ] **Step 1: Build the CLI fresh so timings are current**

Run: `make quick-bundle-cli`
Expected: `target/twk` rebuilt, no errors.

- [ ] **Step 2: Record wall + phase timings (3 runs)**

Run: `for i in 1 2 3; do TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/base.wasm 2>&1 | grep -E '^\[time\] (variant_specialize|produce_mutable_decisions):'; /usr/bin/time -p target/twk build boot/main.tw -o /tmp/base.wasm 2>&1 | grep real; done | tee docs/plans/fixpoint-map-baseline.txt`
Expected: three samples of `variant_specialize`, `produce_mutable_decisions`, and `real` wall. Note the medians — these are the regression yardstick for Task 2's perf gate.

- [ ] **Step 3: Snapshot the golden output for byte-identity checks**

Run: `target/twk build boot/main.tw -o /tmp/golden.wasm && shasum /tmp/golden.wasm`
Expected: a sha to compare against after each later task.

---

### Task 1: The four semantic types + unit tests

**Files:**
- Create: `boot/compiler/block_set.tw`
- Create: `boot/compiler/local_set.tw`
- Create: `boot/compiler/local_map.tw`
- Create: `boot/compiler/block_map.tw`
- Create: `boot/tests/suites/semantic_id_maps_suite.tw`
- Modify: `boot/tests/main.tw` (register the suite)

**Interfaces:**
- Produces (consumed by Task 2 and later families):
  - `block_set.tw`: `pub type BlockSet = .{ inner: Dict<Int, Bool> }`; `pub fn block_set() BlockSet`; `insert(BlockSet, Int) BlockSet`; `contains(BlockSet, Int) Bool`; `len(BlockSet) Int`; `ids(BlockSet) Vector<Int>`.
  - `local_set.tw`: `pub type LocalSet = .{ inner: Dict<Int, Bool> }`; `pub fn local_set() LocalSet`; `insert(LocalSet, Int) LocalSet`; `contains(LocalSet, Int) Bool`; `len(LocalSet) Int`; `ids(LocalSet) Vector<Int>`.
  - `local_map.tw`: `pub type LocalMap<T> = .{ inner: Dict<Int, T> }`; `pub fn local_map<T>() LocalMap<T>`; `get<T>(LocalMap<T>, Int) Option<T>`; `get_or<T>(LocalMap<T>, Int, T) T`; `has<T>(LocalMap<T>, Int) Bool`; `set<T>(LocalMap<T>, Int, T) LocalMap<T>`; `keys<T>(LocalMap<T>) Vector<Int>`; `len<T>(LocalMap<T>) Int`.
  - `block_map.tw`: `pub type BlockMap<T> = .{ inner: Dict<Int, T> }`; `pub fn block_map<T>() BlockMap<T>`; same method set as `LocalMap` but on `BlockMap<T>`.

- [ ] **Step 1: Write `block_set.tw`**

```tw
//! A set of block ids, backed by a persistent `Dict<Int, Bool>` (raw int keys).
//! The type names the intent a bare `Dict<Int, Bool>` cannot: this is a SET, and
//! its ids are BLOCK ids. Backing/perf identical to the Dict it wraps.

pub type BlockSet = .{ inner: Dict<Int, Bool> }

/// Empty block-id set.
pub fn block_set() BlockSet {
  BlockSet.{ inner: Dict.new() }
}

/// Add a block id (persistent: returns a new set).
pub fn insert(s: BlockSet, id: Int) BlockSet {
  BlockSet.{ inner: s.inner.set(id, true) }
}

/// True when the block id is present.
pub fn contains(s: BlockSet, id: Int) Bool {
  s.inner.has(id)
}

/// Number of ids in the set.
pub fn len(s: BlockSet) Int {
  s.inner.len()
}

/// The block ids, in the backing dict's iteration order.
pub fn ids(s: BlockSet) Vector<Int> {
  s.inner.keys()
}
```

- [ ] **Step 2: Write `local_set.tw`** (identical shape, `LocalSet`, "local ids")

```tw
//! A set of local ids, backed by a persistent `Dict<Int, Bool>` (raw int keys).

pub type LocalSet = .{ inner: Dict<Int, Bool> }

/// Empty local-id set.
pub fn local_set() LocalSet {
  LocalSet.{ inner: Dict.new() }
}

/// Add a local id (persistent: returns a new set).
pub fn insert(s: LocalSet, id: Int) LocalSet {
  LocalSet.{ inner: s.inner.set(id, true) }
}

/// True when the local id is present.
pub fn contains(s: LocalSet, id: Int) Bool {
  s.inner.has(id)
}

/// Number of ids in the set.
pub fn len(s: LocalSet) Int {
  s.inner.len()
}

/// The local ids, in the backing dict's iteration order.
pub fn ids(s: LocalSet) Vector<Int> {
  s.inner.keys()
}
```

- [ ] **Step 3: Write `local_map.tw`**

```tw
//! A map from local id to `T`, backed by a persistent `Dict<Int, T>` (raw int
//! keys). Names the key's meaning (local id) that a bare `Dict<Int, T>` cannot.

pub type LocalMap<T> = .{ inner: Dict<Int, T> }

/// Empty local-keyed map.
pub fn local_map<T>() LocalMap<T> {
  LocalMap.{ inner: Dict.new() }
}

/// Value for a local id, or `.None`.
pub fn get<T>(m: LocalMap<T>, k: Int) Option<T> {
  m.inner.get(k)
}

/// Value for a local id, or `default` when absent.
pub fn get_or<T>(m: LocalMap<T>, k: Int, default: T) T {
  case m.inner.get(k) {
    .Some(v) => v,
    .None => default,
  }
}

/// True when the local id is present.
pub fn has<T>(m: LocalMap<T>, k: Int) Bool {
  m.inner.has(k)
}

/// Bind a local id (persistent: returns a new map).
pub fn set<T>(m: LocalMap<T>, k: Int, v: T) LocalMap<T> {
  LocalMap.{ inner: m.inner.set(k, v) }
}

/// The local ids, in the backing dict's iteration order.
pub fn keys<T>(m: LocalMap<T>) Vector<Int> {
  m.inner.keys()
}

/// Number of entries.
pub fn len<T>(m: LocalMap<T>) Int {
  m.inner.len()
}
```

- [ ] **Step 4: Write `block_map.tw`** (identical shape, `BlockMap<T>`, "block id")

```tw
//! A map from block id to `T`, backed by a persistent `Dict<Int, T>` (raw int
//! keys). Names the key's meaning (block id).

pub type BlockMap<T> = .{ inner: Dict<Int, T> }

/// Empty block-keyed map.
pub fn block_map<T>() BlockMap<T> {
  BlockMap.{ inner: Dict.new() }
}

/// Value for a block id, or `.None`.
pub fn get<T>(m: BlockMap<T>, k: Int) Option<T> {
  m.inner.get(k)
}

/// Value for a block id, or `default` when absent.
pub fn get_or<T>(m: BlockMap<T>, k: Int, default: T) T {
  case m.inner.get(k) {
    .Some(v) => v,
    .None => default,
  }
}

/// True when the block id is present.
pub fn has<T>(m: BlockMap<T>, k: Int) Bool {
  m.inner.has(k)
}

/// Bind a block id (persistent: returns a new map).
pub fn set<T>(m: BlockMap<T>, k: Int, v: T) BlockMap<T> {
  BlockMap.{ inner: m.inner.set(k, v) }
}

/// The block ids, in the backing dict's iteration order.
pub fn keys<T>(m: BlockMap<T>) Vector<Int> {
  m.inner.keys()
}

/// Number of entries.
pub fn len<T>(m: BlockMap<T>) Int {
  m.inner.len()
}
```

- [ ] **Step 5: Write the unit-test suite**

Create `boot/tests/suites/semantic_id_maps_suite.tw`:

```tw
use @std.testing.assert as assert
use @std.testing as runner
use compiler.block_set.{BlockSet}
use compiler.block_set
use compiler.local_map.{LocalMap}
use compiler.local_map

pub fn suite() runner.Suite {
  runner
    .suite("semantic_id_maps")
    .test(
      "BlockSet insert/contains/len",
      fn() {
        s := block_set.block_set()
        s = s.insert(3)
        s = s.insert(7)
        s = s.insert(3)
        try assert.is_true(s.contains(3))
        try assert.is_true(s.contains(7))
        try assert.is_false(s.contains(4))
        try assert.equal(s.len(), 2)
        .Ok({})
      },
    )
    .test(
      "LocalMap get/get_or/set/has/len",
      fn() {
        m: LocalMap<Int> = local_map.local_map()
        m = m.set(5, 50)
        m = m.set(9, 90)
        try assert.equal(m.get_or(5, 0 - 1), 50)
        try assert.equal(m.get_or(42, 0 - 1), 0 - 1)
        try assert.is_true(m.has(9))
        try assert.is_false(m.has(9999))
        try assert.equal(m.len(), 2)
        v := case m.get(9) {
          .Some(x) => x,
          .None => 0 - 1,
        }
        try assert.equal(v, 90)
        .Ok({})
      },
    )
    .test(
      "set is persistent — old value unchanged after fork",
      fn() {
        m0: LocalMap<Int> = local_map.local_map()
        m0 = m0.set(1, 100)
        m1 := m0.set(1, 200)
        try assert.equal(m0.get_or(1, 0), 100)
        try assert.equal(m1.get_or(1, 0), 200)
        .Ok({})
      },
    )
}
```

- [ ] **Step 6: Register the suite in `boot/tests/main.tw`**

Add near the other `use .suites.*` lines (keep alphabetical-ish with neighbors):

```tw
use .suites.semantic_id_maps_suite
```

And add to the `runner.run_all([ ... ])` list (near `cfg_lattice_suite.suite(),`):

```tw
  semantic_id_maps_suite.suite(),
```

- [ ] **Step 7: Format + lint the new files**

Run: `for f in boot/compiler/block_set.tw boot/compiler/local_set.tw boot/compiler/local_map.tw boot/compiler/block_map.tw boot/tests/suites/semantic_id_maps_suite.tw boot/tests/main.tw; do target/twk fmt $f; done`
Expected: idempotent (running twice makes no further change).

- [ ] **Step 8: Rebuild the CLI and run the boot suite (smoke gate for the crash class)**

Run: `make quick-bundle-cli && make boot-test`
Expected: build succeeds (no PVec-OOB crash from the new modules — the standalone-module placement is specifically to avoid the index-shift class), and the new `semantic_id_maps` tests pass along with the full suite.

- [ ] **Step 9: Confirm nothing else changed — byte-identical output**

Run: `target/twk build boot/main.tw -o /tmp/t1.wasm && shasum /tmp/t1.wasm /tmp/golden.wasm`
Expected: both shas equal (types exist but nothing adopts them yet → identical output).

- [ ] **Step 10: Commit**

```bash
git add boot/compiler/block_set.tw boot/compiler/local_set.tw boot/compiler/local_map.tw boot/compiler/block_map.tw boot/tests/suites/semantic_id_maps_suite.tw boot/tests/main.tw
git commit -m "feat(ownership): add LocalMap/LocalSet/BlockMap/BlockSet semantic id-map types

Thin nominal wrappers over persistent Dict<Int,_> (raw int keys inside, HAMT
backing unchanged) so a fixpoint map's key meaning and set-vs-map intent live in
the type. Unused so far; adoption follows. No output/perf change (byte-identical).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Adopt `BlockMap<Bool>` for `processed` (derisking first family)

`processed` is a block-id → `Bool` map (**not a set** — it is written `= false` at
`ownership.tw:6226` and `8229`, and `is_processed` returns the stored bool), so its
faithful type is `BlockMap<Bool>`, not `BlockSet`. It is still a well-bounded first
adoption: reads go through the `is_processed` accessor, and the writes map 1:1 to
`set(id, true|false)`. This proves the map type, the byte-identity gate, and the
`make stage2` gate on real fixpoint code before the larger families.

> **Classification rule (applies to every `Dict<Int,Bool>` in the refactor):** a
> `Dict<Int,Bool>` with *any* `= false` write is a bool **map** (`LocalMap<Bool>` /
> `BlockMap<Bool>`), never a set. Only insert-only ones (`= true` exclusively —
> e.g. `moves`, `unique_locals`) are `LocalSet` / `BlockSet`.

**Files:**
- Modify: `boot/compiler/ownership.tw` (`is_processed`, `all_processed`, `FixState.processed`, the `join_entry_*` / merge fn signatures that thread `processed`, the 4 `all_processed` call sites, and the `processed[...] = true|false` write sites in `run_fixpoint`)

**Interfaces:**
- Consumes: `BlockMap`, `block_map()`, `get_or`, `set` from `boot/compiler/block_map.tw` (Task 1).
- Produces: no new external interface; `processed` is now typed `BlockMap<Bool>` throughout.

- [ ] **Step 1: Import the type into `ownership.tw`**

Add with the other `use compiler.*` imports near the top of `boot/compiler/ownership.tw`:

```tw
use compiler.block_map.{BlockMap}
use compiler.block_map
```

- [ ] **Step 2: Retype the accessor `is_processed` and change its read**

At `is_processed` (~`ownership.tw:5146`), change the parameter type and use `get_or`
(preserving the exact `.None => false` semantics):

```tw
fn is_processed(processed: BlockMap<Bool>, id: Int) Bool {
  processed.get_or(id, false)
}
```

- [ ] **Step 3: Retype `all_processed` and build a `BlockMap<Bool>`**

At `all_processed` (~`ownership.tw:5423`), change the return type to `BlockMap<Bool>`
and build via `set`:

```tw
fn all_processed(blocks: Vector<CfgBlock>) BlockMap<Bool> {
  done: BlockMap<Bool> = block_map.block_map()
  for blk in blocks {
    done = done.set(blk.id.id, true)
  }
  done
}
```

The 4 callers (`ownership.tw:7021`, `7395`, `8091`, `8544`) do `done := all_processed(blocks)`
and thread `done` into functions taking `processed`; those params are retyped in
Step 4, so no call-site change is needed beyond that — but verify each `done` is only
passed onward (never used as a raw `Dict`) after retyping.

- [ ] **Step 4: Retype every `processed: Dict<Int, Bool>` parameter**

Run: `grep -n 'processed: Dict<Int, Bool>' boot/compiler/ownership.tw`
For each hit (the `join_entry_ownership`/`_valid`/`_prov`/`_field`/`_path` and merge helpers), change `processed: Dict<Int, Bool>` → `processed: BlockMap<Bool>`. These functions only *read* `processed` via `is_processed`, so no body change beyond the type.

- [ ] **Step 5: Retype the `FixState` field and fix its build/write sites**

Change `processed: Dict<Int, Bool>` → `processed: BlockMap<Bool>` in the `FixState` type (`ownership.tw:5787` block).
Then find where `processed` is created and written in `run_fixpoint`:

Run: `grep -nE 'processed = Dict.new\(\)|processed\[|processed: Dict<Int, Bool> = ' boot/compiler/ownership.tw`
Change initialization `Dict.new()` → `block_map.block_map()`, each `processed[bid] = true` → `processed = processed.set(bid, true)`, and each `processed[bid] = false` (at ~`6226`, ~`8229`) → `processed = processed.set(bid, false)`. (Rebinding-style updates; keep the same variable name per the `direct-rebinding` rule.)

- [ ] **Step 6: Format + lint**

Run: `target/twk fmt boot/compiler/ownership.tw && target/twk lint boot/main.tw`
Expected: idempotent format; lint reports nothing new.

- [ ] **Step 7: Typecheck / build**

Run: `target/twk build boot/main.tw -o /tmp/t2.wasm`
Expected: compiles. If it errors on a residual `processed[...]`/`.get(...)` site, fix that site to the `BlockMap` API (`get_or`/`set`) and rebuild.

- [ ] **Step 8: Establish the baseline is green BEFORE the change (measure the delta)**

`processed` is used by `boot/main.tw` (the compiler itself), so this task changes
the compiler *source* — `boot/main.tw`'s compiled output legitimately differs from
any pre-change golden. **A golden-sha comparison is therefore NOT the gate here** (it
only worked for Task 1 because those types were unused). The behavior-preserving gate
is the self-host fixed point plus the test suite. Because the base branch also carries
unrelated in-flight work, confirm the base is green *first* so a failure is
attributable to this task:

Run (on the commit BEFORE your change): `make stage2`
Expected: prints `Fixed point reached: stage3 == stage4`. If the base already fails,
stop and report — do not attribute it to this task.

- [ ] **Step 9: Self-host fixed point AFTER the change (primary gate)**

Run: `make stage2`
Expected: `Fixed point reached: stage3 == stage4`. Because `BlockMap<Bool>` delegates
to the same `Dict` and `get_or(id,false)` reproduces `is_processed`'s `.None => false`
exactly, the refactored compiler must still reach the fixed point. A `make stage2`
failure here means a behavior change slipped in (most likely an accessor-semantics or
write-site mistake) — fix it, don't accept it.

- [ ] **Step 10: Rebuild the CLI and run the boot suite**

Run: `make quick-bundle-cli && make boot-test`
Expected: green (the fixed-point compiler compiles and passes the full suite).

- [ ] **Step 11: Perf sanity — no regression from the wrapper**

Run: `for i in 1 2; do TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/t2.wasm 2>&1 | grep -E '^\[time\] (variant_specialize|produce_mutable_decisions):'; done`
Expected: medians within noise (±~15% on the phases is normal) of the Task 0 baseline
(`variant_specialize` ~8.6–9.1s). A single `BlockMap<Bool>` should be immeasurable;
this establishes the perf-sanity habit for the larger families where the
wrapper-allocation count grows. If a clear regression appears, record it — it informs
whether later families need the alias fallback (design spec, "Wrapper-cost note").

- [ ] **Step 12: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "refactor(ownership): type the fixpoint 'processed' map as BlockMap<Bool>

Replace the raw Dict<Int,Bool> 'processed' map with the BlockMap<Bool> semantic
type (reads via get_or(id,false), writes via set(id, true|false) — 'processed' is
a bool map, not a set: it is written =false). Backing Dict unchanged; behavior-
preserving, so the make stage2 fixed point still holds and boot-test is green.
First adoption of the semantic id-map types.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

- [ ] **Step 13: Review checkpoint — STOP and plan the next family**

Adopt the remaining families one task at a time, each with the same gate sequence
(Steps 6–12 above; note the gate is `make stage2` fixed point + boot-test, **not** a
golden-sha compare). **Before typing any `Dict<Int,Bool>`, grep it for a `= false`
write and apply the classification rule** (any `= false` ⇒ `*Map<Bool>`, not `*Set`).

**Execution status (as executed — supersedes the original outline):**
- **F3 — DONE** (commit `6369a465`): `prev_seen`, `dirty` → `BlockMap<Bool>` (bool-maps).
- **F4 — in progress**: `changed_visits` → `BlockMap<Int>`; `succ`, `locked_own`/`_valid`/`_prov`
  → `BlockMap<Vector<Int>>` (independent block-keyed value maps).
- **F5 — TODO (all inner maps → `LocalMap<T>`, one task).** *Boundary redrawn 2026-08-06:*
  the plan's original F5(own/valid/prov)/F6(field_own/path_prov) split can't separate the
  inner-map conversion, because all five local-keyed lattices share the generic helpers
  `nested_get<T>` and `same_map<T>` (own/valid/prov additionally share
  `merge_targeted<T>`/`MergeOut<T>`/`lat_get<T>`). Converting one lattice's inner value type
  forces the shared helpers, so all five convert together or not at all. F5 therefore converts
  **every local-keyed inner map** to `LocalMap<T>` in one task: `ForwardState.{own,valid,prov,
  field_own,path_prov}`, the exit/prev-exit **inner values** (outer `Dict<Int,_>` keying
  stays — that is F6), the `join_entry_*`/`seed_param_*` producers, `merge_targeted`+`MergeOut`
  (own/valid/prov) and `merge_field_own_exit`/`merge_path_prov_exit` (field/path), the shared
  `nested_get`/`same_map`, and the read helpers (`fact_of`/`fact_of_local`/`prov_of`/
  `valid_of_local`/`own_is_unique`/`own_is_shared`/`local_reusable`/…). The typechecker bounds
  the set — adjacent same-typed maps (`suppress`/`unique_seed`/`moves`/… and the raw inner
  path-maps) are not connected to the roots, so touching them creates type errors; leave them
  (deferred). Also migrate the ~12 `Dict<Int,…>` call sites in
  `boot/tests/suites/cfg_lattice_suite.tw`.
- **F6 — TODO (the `exits` family outer keying → `BlockMap`).** With F5's inner values already
  `LocalMap<T>`, F6 is the mechanical **outer** conversion: `exits`, `exit_valid`, `exit_prov`,
  `exit_field_own`, `exit_path_prov`, `prev_exits`/`_valid`/`_prov` from `Dict<Int, LocalMap<…>>`
  to `BlockMap<LocalMap<…>>` (in `ForwardState`/`FixResult`/`warm_state` and the run-loop locals),
  retyping `nested_get`'s outer to `BlockMap`. **Use type aliases for the nested types** —
  `type OwnExits = BlockMap<LocalMap<Int>>`, `type ValidExits = BlockMap<LocalMap<Bool>>`,
  `type ProvExits = BlockMap<LocalMap<Vector<Int>>>` (and the deeper `exit_path_prov`) —
  defined in `ownership.tw`. **Type aliases are confirmed supported** (spike: simple,
  nested, and generic `type X<T> = …` aliases all compile; they are non-nominal, zero-cost,
  interchangeable with the underlying type, and method-transparent). Aliases here are pure
  readability — the key/value distinction is already carried by `BlockMap`/`LocalMap`; do
  **not** use nominal wrappers for the nested types (extra alloc per nested value, and a
  module each, for no added safety).

**Deferred (separate follow-up, not this plan):** the ~30+ adjacent-analysis
`Dict<Int,Bool>` cataloged in the [Follow-up section](#follow-up-deferred-ownershiptw-internal-setflag-maps)
— `moves`/`unique_locals`/`suppress`/`region`/`lineage`/… → shared `LocalSet`/`BlockSet`
(+ `LocalMap<Bool>` for `valid`), concept in the variable name, not per-concept types.

Each family that iterates a map with `.keys()` and re-looks-up can keep that shape
(`for k in m.keys() { v := m.get_or(k, dflt) }`) since the wrappers expose
`keys`/`get_or`; there is no `for k, v in m` sugar on the wrappers.

---

## Self-review notes

- **Spec coverage:** Task 1 delivers the four semantic types (spec "Type design");
  Task 2 + the Step-13 outline cover the full fixpoint surface incl. `dirty`/`succ`
  and the `cfg_lattice_suite` migration (spec Slice 2). The rejected repr swap is
  correctly absent. Slice 0 baseline = Task 0.
- **No `IntMap`/`IntSet` core:** dropped as YAGNI (the fixpoint only needs the four
  semantic types); refine the spec's "core + wrappers" wording to "four semantic
  wrappers" when convenient — not blocking.
- **Placeholder scan:** every code step has concrete code; adoption steps that can't
  pre-quote every site give the exact `grep` to enumerate them plus the exact
  rewrite rule. The gate steps (byte-identity, stage2, boot-test) are the tests for
  the refactor tasks (unit tests don't fit a byte-identity-gated rename).
- **Type consistency:** method names are consistent across tasks (`insert`/`contains`
  for sets; `get`/`get_or`/`has`/`set`/`keys`/`len` for maps); constructors are
  `block_set()`/`local_set()`/`local_map()`/`block_map()`.

---

## Follow-up (deferred): `ownership.tw` internal set/flag maps

Beyond the run_fixpoint dataflow maps, `ownership.tw` carries ~30+ `Dict<Int,Bool>`
scattered through its *adjacent* analyses (copy-carrier, loop-region detection,
combinator lineage, ownership-specialization seeds). These are arguably the *worse*
semantic offenders: a bare `Dict<Int,Bool>` in a helper reveals neither what the
`Int` is (LocalId vs block id) nor that it is a set — and, unlike the fixpoint maps,
most have no named accessor lifting the intent. **Deferred** (keeps the fixpoint
refactor focused); wrap them in the *same* `LocalSet`/`BlockSet` types (plus one new
`LocalMap<Bool>` for the genuine bool-maps). No new machinery — only reach.

Cataloged from commit `61d1c353` (131 `Dict<Int,Bool>` occurrences; classify each
against the rule — any `= false` write ⇒ `*Map<Bool>`, else a set; **confirm each
key-kind at adoption**):

| Group / analysis | Identifiers (usage count) | Likely key | Kind → type |
|---|---|---|---|
| Forward-transfer suppression/seeds (in `ForwardState` + transfer fns) | `suppress` (21), `unique_seed` (16), `cc_suppress` (9), `selected` (2), `no_suppress` | LocalId | set → `LocalSet` |
| Copy-carrier facts (`CopyCarrierFacts`) | `suppress_alias`, `suppress_read`, `seed_params`, `write_keys` (2) | LocalId | set → `LocalSet` |
| Quartet / transport / moves (`BlockPrep`) | `moves` (3), `transport` (4), `quartet` (2) | LocalId | set → `LocalSet` |
| Loop-region detection | `region` (10), `checked_blocks`, `seen` (loop ones) | block id | set → `BlockSet` |
| Combinator lineage (`CombinatorLineage.locals`) | `lineage` (5), `locals` | LocalId | set → `LocalSet` |
| Ownership-spec seeds (`EntrySeedFacts`) | `unique_locals`, `reusable_shell` (2), `field_backing_reusable` (2) | LocalId | set → `LocalSet` |
| Local-def analysis (`LocalDefCounts`) | `assign_targets` (2) | LocalId | set → `LocalSet` |
| CFG prune | `changed`, `selected` | block id | set → `BlockSet` |
| Binding validity | `valid` (4) | LocalId | **bool-map** (writes false) → `LocalMap<Bool>` |

Also present (secondary, `Dict<Int, *>` value maps — same opacity, out of this
catalog's `Bool` focus): `LocalDefCounts.counts: Dict<Int,Int>`, various
`Dict<Int, Vector<Int>>`, etc. — fold into the sweep if pursued.

**Scope caveat:** several of these ride the `ForwardState` transfer record and are
threaded through many functions (hence the high counts), so wrapping them is a
larger, more cross-cutting change than the localized run_fixpoint maps — do it as its
own staged effort after the fixpoint families land, one analysis-group per task with
the same `make stage2` + boot-test gate.
