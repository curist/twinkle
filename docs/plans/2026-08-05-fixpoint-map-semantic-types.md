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

- [ ] **Step 8: Byte-identity gate**

Run: `shasum /tmp/t2.wasm /tmp/golden.wasm`
Expected: **equal**. `BlockMap<Bool>` delegates to the same `Dict` and `get_or(id,false)` reproduces `is_processed`'s `.None => false` exactly, so behavior — and thus emitted output — is unchanged. If it differs, stop and diff: an unequal sha means a behavior change slipped in (most likely an iteration-order or accessor-semantics mistake), not an acceptable variant.

- [ ] **Step 9: Self-host fixed point**

Run: `make stage2`
Expected: stage3 == stage4 fixed point holds (the newly-compiled compiler rebuilds itself to a fixed point).

- [ ] **Step 10: Boot test suite**

Run: `make boot-test`
Expected: green.

- [ ] **Step 11: Perf gate — no regression from the wrapper**

Run: `for i in 1 2 3; do TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/t2.wasm 2>&1 | grep -E '^\[time\] (variant_specialize|produce_mutable_decisions):'; /usr/bin/time -p target/twk build boot/main.tw -o /tmp/t2.wasm 2>&1 | grep real; done`
Expected: medians within noise (±~15% on the phases is normal) of the Task 0 baseline. A single `BlockMap<Bool>` should be immeasurable; this step establishes the perf-gate habit for the larger families where the wrapper-allocation count grows. If a clear regression appears, record it — it informs whether later families need the alias fallback (design spec, "Wrapper-cost note").

- [ ] **Step 12: Commit**

```bash
git add boot/compiler/ownership.tw
git commit -m "refactor(ownership): type the fixpoint 'processed' map as BlockMap<Bool>

Replace the raw Dict<Int,Bool> 'processed' map with the BlockMap<Bool> semantic
type (reads via get_or(id,false), writes via set(id, true|false) — 'processed' is
a bool map, not a set: it is written =false). Backing Dict unchanged; output is
byte-identical and make stage2 holds. First adoption of the semantic id-map types.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

- [ ] **Step 13: Review checkpoint — STOP and plan the next family**

Do not continue to other maps yet. With the pattern and all three gates
(byte-identical, `make stage2`, boot-test) proven on `processed`, plan the
remaining families next, each its own task with the same gate sequence (Steps 6–12
above). **Before typing any `Dict<Int,Bool>`, grep it for a `= false` write and
apply the classification rule above** (any `= false` ⇒ `*Map<Bool>`, not `*Set`):

1. **Genuine `LocalSet` sets** (insert-only, `= true` only — verify with grep):
   `moves` (`ownership.tw:405/553`), `unique_locals`, per-block owned sets. Verify
   each has no `= false` write before typing it as a set.
2. **`BlockMap<Bool>` / `LocalMap<Bool>` bool-maps** (they *do* write `= false`, so
   NOT sets): `prev_seen` (`= false` at `6224`), `dirty` (`= false` at `6278`).
3. **`LocalMap<T>`** — `join_entry_ownership`/`_valid`/`_prov` results, the per-block
   own/valid/prov values, and `merge_targeted` (note: `merge_targeted` is `pub` and
   called with `Dict<Int,…>` literals at ~12 sites in
   `boot/tests/suites/cfg_lattice_suite.tw` — migrating those call sites is part of
   this task; retype its `old`/`next`/`prev` params to `LocalMap<T>` and its `MergeOut`).
4. **`BlockMap<T>`** — the `exits` family (`exits`, `exit_valid`, `exit_prov`,
   `exit_field_own`, `exit_path_prov`, `prev_exits`/`_valid`/`_prov`,
   `changed_visits`) and `succ`. Nested cases read as `BlockMap<LocalMap<Int>>`.
   Include the `field_own`/`path_prov` bespoke merges (`merge_field_own_exit`,
   `merge_path_prov_exit`).

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
