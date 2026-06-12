# Dict/Set Amortized O(log n) Remove Implementation Plan

> **Status: COMPLETE / ARCHIVED (2026-06-12).** Implemented in the boot runtime: `Dict.remove`/`Set.remove` now tombstone insertion-order slots and compact by density, making bulk remove linear. Validation passed with self-host convergence, boot tests, Rust tests, and dict/set benchmarks. The original per-task commit checkpoints below were folded into the final implementation commit.

> **For agentic workers:** This plan is archived. Steps remain checked below as the implementation record.

**Goal:** Make `Dict.remove`/`Set.remove` amortized O(log n) instead of O(n) per call, eliminating the O(n²) bulk-remove cliff (`dict_int_remove`: ~10 s at 32k today).

**Architecture:** Stop rebuilding the insertion-order vector on every remove. Instead, each HAMT entry stores its slot index in the order vector; `remove` tombstones that exact slot (a persistent O(log n) `arr_set` write) and bumps a tombstone counter; a density-triggered `compact` (when dead ≥ live) rebuilds the order vector and refreshes entry indices, amortizing to O(log n) per remove. `keys()` stays O(1) when there are no tombstones and filters them out otherwise. Boot-only (`boot/compiler/codegen/runtime/`); stage0's Rust runtime is left as the existing-behavior reference.

**Tech Stack:** Twinkle runtime emitter (Wasm-GC instruction arrays in `.tw`). Validation via self-host convergence (`make stage2`), `make boot-test`, and the `boot/bench/` dict suite.

---

## Background — why this works

Today `remove` calls `order_remove_key` (`dict.tw`), which on every call does `arr_to_array` (O(n) flatten) → count scan → compact scan → `arr_from_array` (O(n) rebuild). The HAMT removal itself is O(log n); the order-vector maintenance is the whole cost.

Insertion-order semantics to preserve exactly:
- `keys()` is insertion order of first insertion of currently-present keys.
- `set` appends to `order` only for **new** keys (not replacements) — confirmed in `set_fn`.
- Therefore **remove-then-reinsert puts a key at the end** (it becomes a brand-new entry pushed to the tail). The tombstone design preserves this for free: the old slot stays a distinct `null` tombstone, the reinsert pushes a fresh entry at the tail — no duplicate, no dedup logic.
- Dicts are persistent values; equality is order-independent and must ignore `order_index`.

## File Structure

- `boot/compiler/codegen/runtime/types.tw` — add `HamtEntry.order_index` and `PDict.tombstones` fields.
- `boot/compiler/codegen/runtime/dict.tw` — field-index constants, two new globals, index-stamping in `node_set`/`collision_set`, index publishing in `node_remove`, rewritten `remove`/`remove_in_place`, new `compact`, tombstone-aware `keys`, `set`/`set_in_place`/`make` updates, drop `order_remove_key`.
- `boot/tests/suites/api_dict_suite.tw`, `boot/tests/suites/api_set_suite.tw` — correctness tests.
- `boot/bench/README.md` — refresh the `dict_int_remove` baseline after the fix.

No stage0/Rust changes. No source-language or public-API changes.

## Reference: current field/op facts (verify before editing — repo line numbers drift)

- `dict.tw` field constants: `he_hash := 0`, `he_key := 1`, `he_val := 2`, `pd_size := 0`, `pd_root := 1`, `pd_order := 2`.
- `dict.tw` already imports `rt.arr` `len` as `arr_len` (`params: [pvec_null] -> i32`) in the module imports list.
- Persistent ops callable from `dict.tw`: `arr_push`, `arr_to_array`, `arr_from_array`, `arr_make_empty`, `arr_len`, and `arr_set` (`rt.arr` `set`, signature `set(vec: PVec?, idx: i32, val: anyref) -> PVec`) — `rt.arr` functions link as `arr_<name>`. `arr_set_in_place` is `rt.arr` `set_in_place` (mutating).
- The three `StructNew(t_hamt_entry)` sites today: one in `node_set` (top, builds `new_entry` L5 from `[hash, key, val]`), two in `collision_set` (replace-path and append-path).
- `node_set` reuses the single top-built `new_entry` (L5) for inserts **and** the replace site.
- `node_remove` matches a key at two sites: a direct entry (matched entry in L11) and a collision-array entry.
- Existing precedent for a "side-channel" flag: the `was_replace` i32 global, set inside `node_set`/`collision_set` and read by `set`.

---

## Task 1: Plumb `order_index` + `tombstones` fields and stamp on insert (behavior-preserving)

This task adds the data and the index stamping, but leaves `remove` using the existing `order_remove_key`. Observable behavior is unchanged; the new fields are written but not yet read. This is a self-hosting checkpoint.

**Files:**
- Modify: `boot/compiler/codegen/runtime/types.tw`
- Modify: `boot/compiler/codegen/runtime/dict.tw`
- Test: `boot/tests/suites/api_dict_suite.tw`, `boot/tests/suites/api_set_suite.tw`

- [x] **Step 1: Add characterization tests that must stay green through Tasks 1–2**

These assert the observable contract the refactor must not break. Add to `boot/tests/suites/api_dict_suite.tw` (follow the file's existing `test(...)`/`assert` style; adapt helper names to those already imported in the file):

```tw
test("dict remove then reinsert puts key at end", fn() {
  d: Dict<Int, Int> = Dict.new()
  d[1] = 10
  d[2] = 20
  d[3] = 30
  d = .remove(2)
  d[2] = 99
  try assert.equal(d.keys(), [1, 3, 2])      // 2 reinserted at the end
  try assert.equal(d.get(2), .Some(99))
  try assert.equal(d.len(), 3)
})

test("dict remove preserves persistence of aliased version", fn() {
  a: Dict<Int, Int> = Dict.new()
  a[1] = 1
  a[2] = 2
  b := a.remove(1)
  try assert.equal(a.keys(), [1, 2])         // original unchanged
  try assert.equal(b.keys(), [2])
  try assert.equal(a.get(1), .Some(1))
})

test("dict heavy interleaved set/remove keeps order and contents", fn() {
  d: Dict<Int, Int> = Dict.new()
  for i in range(200) {
    d[i] = i
  }
  for i in range(200) {
    if i % 2 == 0 {
      d = .remove(i)
    }
  }
  try assert.equal(d.len(), 100)
  expected: Vector<Int> = collect i in range(200) {
    i
  }
  // surviving keys are the odd ones, still in ascending insertion order
  odds := expected.filter(fn(x) { x % 2 == 1 })
  try assert.equal(d.keys(), odds)
  for k in d.keys() {
    try assert.equal(d.get(k), .Some(k))
  }
})

test("dict equality ignores insertion order and tombstone history", fn() {
  a: Dict<Int, Int> = Dict.new()
  a[1] = 1
  a[2] = 2
  a[3] = 3
  b: Dict<Int, Int> = Dict.new()
  b[3] = 3
  b[1] = 1
  b[9] = 9
  b = .remove(9)        // forces a different order/tombstone history
  b[2] = 2
  try assert.is_true(a == b)
})
```

Add the analogous insert/remove/reinsert + persistence cases to `boot/tests/suites/api_set_suite.tw` using `Set.new()/.insert/.remove/.contains/.to_vector`.

- [x] **Step 2: Run the new tests against current code to confirm they pass (characterization baseline)**

Run: `make boot-test 2>&1 | tail -20`
Expected: PASS (these describe current behavior; they must already hold).

- [x] **Step 3: Add the struct fields in `types.tw`**

In `HamtEntry`, append a field after `val`:

```tw
.{ name: .Some("order_index"), mutable: false, ty: .I32 },
```

In `PDict`, append a field after `order`:

```tw
.{ name: .Some("tombstones"), mutable: true, ty: .I32 },
```

- [x] **Step 4: Add field-index constants, two globals, and the `arr_set` imports in `dict.tw`**

Next to the existing `he_*`/`pd_*` constants add:

```tw
he_order_index := 3
pd_tombstones := 3
```

In the module `globals:` list (where `was_replace` is declared) add:

```tw
GlobalDef.{ name: "dict_order_index", mutable: true, ty: .I32, init: [.I32Const(0)] },
GlobalDef.{ name: "dict_removed_index", mutable: true, ty: .I32, init: [.I32Const(0)] },
```

`dict.tw` currently imports `rt.arr`'s `push`/`len`/`get`/`to_array`/`from_array`/`make_empty` but **not** `set` or `set_in_place`. The tombstone writes in Task 2 need them. Add both to the `imports:` list alongside the existing `rt.arr` entries:

```tw
.{
  module: "rt.arr",
  name: "set",
  as_sym: "arr_set",
  params: [pvec_null, .I32, .Anyref],
  results: [pvec_ref],
},
.{
  module: "rt.arr",
  name: "set_in_place",
  as_sym: "arr_set_in_place",
  params: [pvec_null, .I32, .Anyref],
  results: [pvec_ref],
},
```

(Adding them in Task 1 is harmless — they are unused until Task 2. Verify `rt.arr` exports `set`/`set_in_place` with these signatures; `arr.tw` defines `set` at `// set(vec: PVec?, idx: i32, val: anyref) -> PVec` and `set_in_place` alongside it.)

- [x] **Step 5: Stamp the insert index at the three entry-creation sites**

`node_set` top build (`[hash, key, val] StructNew(t_hamt_entry)`): push the global before `StructNew`:

```tw
.LocalGet(1),                       // hash
.LocalGet(3),                       // key
.LocalGet(4),                       // val
.GlobalGet("dict_order_index"),     // order_index  ← added
.StructNew(t_hamt_entry),
.LocalSet(5),
```

`collision_set` **append path** (the build just before `arr_insert_at`, near the end of the function): push `.GlobalGet("dict_order_index")` before its `StructNew(t_hamt_entry)`.

`collision_set` **replace path** (inside the `core_eq` match, where it currently rebuilds the entry then `arr_replace_at`): the replacement must **preserve the old entry's index**. At that point the scanned old entry is still in `L7`. Build the replacement as:

```tw
.LocalGet(1),                                   // hash
.LocalGet(2),                                   // key
.LocalGet(3),                                   // val
.LocalGet(7),                                   // old entry (still holds the match)
.StructGet(t_hamt_entry, he_order_index),       // preserve its index
.StructNew(t_hamt_entry),
.LocalSet(7),
```

- [x] **Step 6: Preserve the index at the `node_set` replace site**

At the `node_set` key-match replace branch (currently `arr_replace_at(entries, idx, L5)` with the matched old entry in `L13`): replace the use of `L5` with a freshly built entry carrying the old index. I.e. where it pushes `.LocalGet(5)` as the replacement entry argument to `arr_replace_at`, substitute:

```tw
.LocalGet(1),                                   // hash
.LocalGet(3),                                   // key
.LocalGet(4),                                   // val
.LocalGet(13),                                  // old matched entry
.StructGet(t_hamt_entry, he_order_index),       // preserve its index
.StructNew(t_hamt_entry),
```

(The `node_set` deep-collision-creation path that builds a `HamtCollision` from `[old_entry L13, new_entry L5]` directly — the `depth >= 12` branch — needs no change: `L13` is the unchanged old entry and `L5` already carries the global insert index.)

- [x] **Step 6b: Guard the recursive old-entry reinsertion in `node_set` (critical)**

There is a second, subtler `node_set` path that re-stamps an existing entry: the `depth < 12` collision **split**. When the new key collides with an existing entry (`L13`) at a slot, `node_set` recursively reinserts **the old entry** into a fresh deeper node:

```tw
.RefNull(.Named(t_hamt_node)),
.LocalGet(13), .StructGet(t_hamt_entry, he_hash),
.LocalGet(2), .I32Const(1), .I32Add,          // depth + 1
.LocalGet(13), .StructGet(t_hamt_entry, he_key),
.LocalGet(13), .StructGet(t_hamt_entry, he_val),
.Call("node_set"),                            // ← rebuilds L13's entry, re-stamps with global
.LocalSet(14),
// ... then recursively inserts the NEW key into L14 ...
.Call("node_set"),
```

After Step 5 stamps `node_set`'s top build from `dict_order_index`, this recursive call would rebuild the **old** entry with the **new** key's index — corrupting `L13.order_index`, so a later remove of that old key tombstones the wrong slot. This is the core correctness hazard.

Fix: temporarily point the global at the old entry's own index for the old-entry recursive call, then restore it (from `L5`, which holds the new entry already stamped with the new index at Step 5 and is never overwritten) before the new-key recursive call.

Immediately **before** the old-entry recursive call (before the `.RefNull(.Named(t_hamt_node))` that begins its argument list), insert:

```tw
.LocalGet(13),
.StructGet(t_hamt_entry, he_order_index),
.GlobalSet("dict_order_index"),
```

Immediately **after** that call's `.LocalSet(14)`, and before the new-key recursive `node_set` argument list begins, insert:

```tw
.LocalGet(5),
.StructGet(t_hamt_entry, he_order_index),
.GlobalSet("dict_order_index"),
```

This is self-consistent under recursion: every `node_set` frame saves/restores around its own split, and the old entry being pushed deeper is a single key (it lands in an empty node without further splitting), so the diverted global is restored before any new-key work. The interleaved-set/remove characterization test (Step 1) — which builds 200 keys and removes half — exercises collisions and is the guard for this.

- [x] **Step 7: Set `dict_order_index` before `node_set` in `set` and `set_in_place`**

At the top of `set_fn`'s body (right after computing the hash, before `node_set`), insert:

```tw
.LocalGet(0),
.RefAsNonNull,
.StructGet(t_pdict, pd_order),
.Call("arr_len"),
.GlobalSet("dict_order_index"),
```

Add the identical snippet at the top of `set_in_place_fn`'s body. (Both append the new key at `arr_len(order)`, so the stamped index matches the slot `arr_push` will fill.)

- [x] **Step 8: Add the `tombstones` field to every `StructNew(t_pdict)` site**

Run: `grep -n "StructNew(t_pdict)" boot/compiler/codegen/runtime/dict.tw`

For each site, push the tombstone count before `StructNew(t_pdict)`:
- `make_fn` (empty dict): push `.I32Const(0)`.
- `set_fn` (new dict from `[new_size, new_root, new_order]`): push the source dict's count — `.LocalGet(0), .RefAsNonNull, .StructGet(t_pdict, pd_tombstones)` — before `StructNew(t_pdict)` (set never creates a tombstone, so it carries the count through unchanged).
- `remove_fn`: leave as-is for now (rewritten in Task 2). To keep Task 1 compiling, push `.LocalGet(0), .RefAsNonNull, .StructGet(t_pdict, pd_tombstones)` before its `StructNew(t_pdict)` so the field count is correct.

(`set_in_place_fn`/`remove_in_place_fn` mutate `p0` via `StructSet` and do not `StructNew` a `PDict`, so they need no tombstone field here.)

- [x] **Step 9: Self-host and run the full suite (behavior unchanged)**

Run: `make stage2 && make boot-test 2>&1 | tail -20`
Expected: self-host converges (regenerates `target/boot.wasm`); all tests PASS including the Step 1 characterization tests. Behavior is identical to before — the new fields are written but not yet read.

- [x] **Step 10: Commit**

```bash
git add boot/compiler/codegen/runtime/types.tw boot/compiler/codegen/runtime/dict.tw boot/tests/suites/api_dict_suite.tw boot/tests/suites/api_set_suite.tw
git commit -m "dict: store order_index per entry + tombstones field (no behavior change)

Plumbs the data needed for amortized O(log n) remove: each HamtEntry records
its slot in the insertion-order vector, and PDict gains a tombstone counter.
Indices are stamped on insert and preserved on replace; nothing reads them yet,
so observable behavior is unchanged. Self-host + boot-test green."
```

---

## Task 2: Tombstone-based remove + compaction + tombstone-aware keys

Swap the algorithm. `remove` tombstones the exact slot using the stored index; `keys` skips tombstones; `compact` rebuilds when dead ≥ live. The Task 1 characterization tests stay green; the bench flips from quadratic to linear.

**Files:**
- Modify: `boot/compiler/codegen/runtime/dict.tw`

- [x] **Step 1: Publish the removed entry's index from `node_remove`**

At the **direct-entry match** in `node_remove` (the `core_eq` true branch, matched entry in `L11`), at the very start of that branch insert:

```tw
.LocalGet(11),
.StructGet(t_hamt_entry, he_order_index),
.GlobalSet("dict_removed_index"),
```

At the **collision-array match** in `node_remove` (where it finds the matching entry in the collision's entry array before dropping it — the matched entry is in the loop's entry local, e.g. `ce`), insert the same three instructions using that matched-entry local:

```tw
.LocalGet(<matched collision entry local>),
.StructGet(t_hamt_entry, he_order_index),
.GlobalSet("dict_removed_index"),
```

(Read the current `node_remove` body to confirm the exact local holding the matched collision entry.)

- [x] **Step 2: Add the `compact` function**

Add this `FuncDef` builder and register it in the module `funcs:` list:

```tw
// ── compact(dict: PDict?) -> PDict ───────────────────────────────────────────
// Rebuild order (dropping null tombstones) and refresh every entry's stored
// order_index. O(n log n); run only when dead >= live, so amortized O(log n)
// per remove. Fully persistent: builds fresh structures.
fn compact_fn() FuncDef {
  // p0=dict; L1=flat order, L2=n, L3=new_root, L4=new_order, L5=i, L6=j,
  // L7=key, L8=hash, L9=val
  FuncDef.{
    name: "compact",
    params: [pdict_null],
    results: [pdict_ref],
    locals: [arr_null, .I32, hamt_node_null, pvec_null, .I32, .I32, .Anyref, .I64, .Anyref],
    body: [
      .LocalGet(0),
      .RefAsNonNull,
      .StructGet(t_pdict, pd_order),
      .RefAsNonNull,
      .Call("arr_to_array"),
      .LocalSet(1),
      .LocalGet(1),
      .RefAsNonNull,
      .ArrayLen,
      .LocalSet(2),
      .RefNull(.Named(t_hamt_node)),
      .LocalSet(3),
      .Call("arr_make_empty"),
      .LocalSet(4),
      .I32Const(0),
      .LocalSet(5),
      .I32Const(0),
      .LocalSet(6),
      .Block(
        "exit",
        .None,
        [
          .Loop(
            "loop",
            .None,
            [
              .LocalGet(5),
              .LocalGet(2),
              .I32GeS,
              .BrIf("exit"),
              .LocalGet(1),
              .RefAsNonNull,
              .LocalGet(5),
              .ArrayGet(t_array),
              .LocalSet(7),
              .LocalGet(7),
              .RefIsNull,
              .I32Eqz,
              .If(
                .None,
                [
                  .LocalGet(7),
                  .Call("hash_key"),
                  .LocalSet(8),
                  .LocalGet(0),
                  .RefAsNonNull,
                  .StructGet(t_pdict, pd_root),
                  .LocalGet(8),
                  .I32Const(0),
                  .LocalGet(7),
                  .Call("node_get"),
                  .LocalSet(9),
                  .LocalGet(6),
                  .GlobalSet("dict_order_index"),
                  .LocalGet(3),
                  .LocalGet(8),
                  .I32Const(0),
                  .LocalGet(7),
                  .LocalGet(9),
                  .Call("node_set"),
                  .LocalSet(3),
                  .LocalGet(4),
                  .RefAsNonNull,
                  .LocalGet(7),
                  .Call("arr_push"),
                  .LocalSet(4),
                  .LocalGet(6),
                  .I32Const(1),
                  .I32Add,
                  .LocalSet(6),
                ],
                [],
              ),
              .LocalGet(5),
              .I32Const(1),
              .I32Add,
              .LocalSet(5),
              .Br("loop"),
            ],
          ),
        ],
      ),
      .LocalGet(6),
      .LocalGet(3),
      .LocalGet(4),
      .RefAsNonNull,
      .I32Const(0),
      .StructNew(t_pdict),
    ],
  }
}
```

- [x] **Step 3: Rewrite `remove_fn` to tombstone + maybe compact**

Replace the body of `remove_fn` with:

```tw
fn remove_fn() FuncDef {
  // p0=dict, p1=key; L2=hash, L3=old_root, L4=new_root, L5=was_removed,
  // L6=new_order, L7=new_tombstones, L8=new_size, L9=removed_index, L10=new_dict
  FuncDef.{
    name: "remove",
    params: [pdict_null, .Anyref],
    results: [pdict_ref],
    locals: [.I64, hamt_node_null, hamt_node_null, .I32, pvec_null, .I32, .I32, .I32, pdict_null],
    body: [
      .LocalGet(1),
      .Call("hash_key"),
      .LocalSet(2),
      .LocalGet(0),
      .RefAsNonNull,
      .StructGet(t_pdict, pd_root),
      .LocalSet(3),
      .LocalGet(3),
      .LocalGet(2),
      .I32Const(0),
      .LocalGet(1),
      .Call("node_remove"),
      .LocalSet(4),
      .LocalGet(3),
      .LocalGet(4),
      .RefEq,
      .I32Eqz,
      .LocalSet(5),
      .LocalGet(0),
      .RefAsNonNull,
      .StructGet(t_pdict, pd_order),
      .LocalSet(6),
      .LocalGet(0),
      .RefAsNonNull,
      .StructGet(t_pdict, pd_tombstones),
      .LocalSet(7),
      .LocalGet(0),
      .RefAsNonNull,
      .StructGet(t_pdict, pd_size),
      .LocalSet(8),
      .LocalGet(5),
      .If(
        .None,
        [
          .GlobalGet("dict_removed_index"),
          .LocalSet(9),
          .LocalGet(6),
          .RefAsNonNull,
          .LocalGet(9),
          .RefNull(.None_),
          .Call("arr_set"),
          .LocalSet(6),
          .LocalGet(7),
          .I32Const(1),
          .I32Add,
          .LocalSet(7),
          .LocalGet(8),
          .I32Const(1),
          .I32Sub,
          .LocalSet(8),
        ],
        [],
      ),
      .LocalGet(8),
      .LocalGet(4),
      .LocalGet(6),
      .RefAsNonNull,
      .LocalGet(7),
      .StructNew(t_pdict),
      .LocalSet(10),
      // compact when a removal pushed dead >= live. No `size > 0` guard: when
      // the last key is removed (size 0, tombstones 1) this still fires and
      // compact rebuilds a clean empty dict, so an emptied dict never keeps a
      // tombstoned order vector across reuse.
      .LocalGet(5),
      .LocalGet(7),
      .LocalGet(8),
      .I32GeS,
      .I32And,
      .If(
        .Some(pdict_ref),
        [.LocalGet(10), .RefAsNonNull, .Call("compact")],
        [.LocalGet(10), .RefAsNonNull],
      ),
    ],
  }
}
```

- [x] **Step 4: Rewrite `remove_in_place_fn` to tombstone in place + maybe compact**

`remove_in_place` operates on a uniquely-owned dict, so it mutates `p0` via `StructSet` and may tombstone the order vector in place with `arr_set_in_place`. Replace its body with:

```tw
fn remove_in_place_fn() FuncDef {
  // p0=dict, p1=key; L2=hash, L3=old_root, L4=new_root, L5=was_removed, L6=removed_index
  FuncDef.{
    name: "remove_in_place",
    params: [pdict_null, .Anyref],
    results: [pdict_ref],
    locals: [.I64, hamt_node_null, hamt_node_null, .I32, .I32],
    body: [
      .LocalGet(1),
      .Call("hash_key"),
      .LocalSet(2),
      .LocalGet(0),
      .RefAsNonNull,
      .StructGet(t_pdict, pd_root),
      .LocalSet(3),
      .LocalGet(3),
      .LocalGet(2),
      .I32Const(0),
      .LocalGet(1),
      .Call("node_remove"),
      .LocalSet(4),
      .LocalGet(3),
      .LocalGet(4),
      .RefEq,
      .I32Eqz,
      .LocalSet(5),
      .LocalGet(0),
      .RefAsNonNull,
      .LocalGet(4),
      .StructSet(t_pdict, pd_root),
      .LocalGet(5),
      .If(
        .None,
        [
          .GlobalGet("dict_removed_index"),
          .LocalSet(6),
          // order = arr_set_in_place(order, removed_index, null)
          .LocalGet(0),
          .RefAsNonNull,
          .LocalGet(0),
          .RefAsNonNull,
          .StructGet(t_pdict, pd_order),
          .RefAsNonNull,
          .LocalGet(6),
          .RefNull(.None_),
          .Call("arr_set_in_place"),
          .StructSet(t_pdict, pd_order),
          // tombstones += 1
          .LocalGet(0),
          .RefAsNonNull,
          .LocalGet(0),
          .RefAsNonNull,
          .StructGet(t_pdict, pd_tombstones),
          .I32Const(1),
          .I32Add,
          .StructSet(t_pdict, pd_tombstones),
          // size -= 1
          .LocalGet(0),
          .RefAsNonNull,
          .LocalGet(0),
          .RefAsNonNull,
          .StructGet(t_pdict, pd_size),
          .I32Const(1),
          .I32Sub,
          .StructSet(t_pdict, pd_size),
        ],
        [],
      ),
      // compact when a removal pushed dead >= live (no `size > 0` guard, so the
      // final remove-to-empty also compacts to a clean empty dict).
      .LocalGet(5),
      .LocalGet(0),
      .RefAsNonNull,
      .StructGet(t_pdict, pd_tombstones),
      .LocalGet(0),
      .RefAsNonNull,
      .StructGet(t_pdict, pd_size),
      .I32GeS,
      .I32And,
      .If(
        .Some(pdict_ref),
        [.LocalGet(0), .RefAsNonNull, .Call("compact")],
        [.LocalGet(0), .RefAsNonNull],
      ),
    ],
  }
}
```

If `rt.arr` `set_in_place` is not callable as `arr_set_in_place` from `dict.tw`, add it to the module imports list the same way `arr_len` is imported (`{ module: "rt.arr", name: "set_in_place", as_sym: "arr_set_in_place", params: [pvec_null, .I32, .Anyref], results: [pvec_ref] }`), or fall back to `arr_set` (persistent) — correctness first; the in-place win on `order` is secondary.

- [x] **Step 5: Make `keys` tombstone-aware**

Replace `keys_fn` so it returns the order vector directly when clean and a filtered copy otherwise:

```tw
fn keys_fn() FuncDef {
  // p0=dict; L1=flat, L2=n, L3=dst, L4=i, L5=j, L6=elem
  FuncDef.{
    name: "keys",
    params: [pdict_null],
    results: [pvec_ref],
    locals: [arr_null, .I32, arr_null, .I32, .I32, .Anyref],
    body: [
      .LocalGet(0),
      .RefAsNonNull,
      .StructGet(t_pdict, pd_tombstones),
      .I32Eqz,
      .If(
        .Some(pvec_ref),
        [.LocalGet(0), .RefAsNonNull, .StructGet(t_pdict, pd_order)],
        [
          .LocalGet(0),
          .RefAsNonNull,
          .StructGet(t_pdict, pd_order),
          .RefAsNonNull,
          .Call("arr_to_array"),
          .LocalSet(1),
          .LocalGet(1),
          .RefAsNonNull,
          .ArrayLen,
          .LocalSet(2),
          .RefNull(.None_),
          .LocalGet(0),
          .RefAsNonNull,
          .StructGet(t_pdict, pd_size),
          .ArrayNew(t_array),
          .LocalSet(3),
          .I32Const(0),
          .LocalSet(4),
          .I32Const(0),
          .LocalSet(5),
          .Block(
            "kexit",
            .None,
            [
              .Loop(
                "kloop",
                .None,
                [
                  .LocalGet(4),
                  .LocalGet(2),
                  .I32GeS,
                  .BrIf("kexit"),
                  .LocalGet(1),
                  .RefAsNonNull,
                  .LocalGet(4),
                  .ArrayGet(t_array),
                  .LocalSet(6),
                  .LocalGet(6),
                  .RefIsNull,
                  .I32Eqz,
                  .If(
                    .None,
                    [
                      .LocalGet(3),
                      .RefAsNonNull,
                      .LocalGet(5),
                      .LocalGet(6),
                      .ArraySet(t_array),
                      .LocalGet(5),
                      .I32Const(1),
                      .I32Add,
                      .LocalSet(5),
                    ],
                    [],
                  ),
                  .LocalGet(4),
                  .I32Const(1),
                  .I32Add,
                  .LocalSet(4),
                  .Br("kloop"),
                ],
              ),
            ],
          ),
          .LocalGet(3),
          .RefAsNonNull,
          .Call("arr_from_array"),
        ],
      ),
    ],
  }
}
```

- [x] **Step 6: Drop `order_remove_key`**

Remove the `order_remove_key_fn()` definition and its entry in the module `funcs:` list. Confirm nothing else references it:

Run: `grep -n "order_remove_key" boot/compiler/codegen/runtime/dict.tw`
Expected: no matches.

- [x] **Step 7: Confirm no other consumer reads `pd_order` expecting it tombstone-free**

Run: `grep -n "pd_order" boot/compiler/codegen/runtime/dict.tw`
Expected matches only in: `keys_fn` (handles tombstones), `set_fn`/`set_in_place_fn` (append; tombstones irrelevant), `remove_fn`/`remove_in_place_fn`/`compact_fn`. Then verify the prelude routes `Dict.values`/iteration and `Set.to_vector` through `keys()` (not a raw order read):

Run: `grep -rn "keys\|values\|to_vector\|pd_order\|dict\$order" boot/prelude boot/compiler/codegen/emit.tw | grep -i dict`
Expected: `values`/iteration/`to_vector` build on `keys()`; no direct order access bypassing it. If any path reads the order vector directly, route it through `keys()`.

- [x] **Step 8: Self-host and run the full suite**

Run: `make stage2 && make boot-test 2>&1 | tail -20`
Expected: self-host converges; all tests PASS, including the Task 1 characterization tests (order, persistence, equality, interleaved set/remove).

- [x] **Step 9: Commit**

```bash
git add boot/compiler/codegen/runtime/dict.tw
git commit -m "dict: amortized O(log n) remove via tombstoned order vector

remove now tombstones the removed key's exact order slot (located via the
entry's stored order_index, published by node_remove) instead of rebuilding
the whole insertion-order vector. A density-triggered compact() (dead >= live)
rebuilds order and refreshes indices, amortizing to O(log n) per remove. keys()
stays O(1) when clean and filters tombstones otherwise. Behavior is unchanged
(characterization tests green); bulk remove drops from O(n^2) to O(n)."
```

---

## Task 3: Validate the complexity win and refresh baselines

**Files:**
- Modify: `boot/bench/README.md`

- [x] **Step 1: Re-run the remove benchmark and confirm it is now linear**

Run: `target/twk run boot/bench/dict_int_remove.tw`
Expected: per-doubling ratio trends to ~2× (linear bulk) instead of ~4× (quadratic). At 32k it should be a few ms, not ~10 000 ms.

- [x] **Step 2: Confirm no regression on the other dict/set benches**

Run: `for f in dict_int_build dict_int_get dict_int_has dict_str_build dict_str_get dict_str_has set_int_build set_int_contains set_str_build set_str_contains; do echo "== $f =="; target/twk run boot/bench/$f.tw | tail -1; done`
Expected: build/get/has/contains within noise of the Phase 0 baselines in `boot/bench/README.md` (keys() stays O(1) on the clean path; set/get untouched).

- [x] **Step 3: Update the remove baseline in `boot/bench/README.md`**

In the "Dict / Set benchmark suite" section, update the `remove` line from the quadratic Phase 0 numbers to the new linear measurement, and replace the "remove is O(n) per call → O(n²)" note with a one-line record that tombstoned removal made it amortized O(log n)/call (linear bulk), pointing at this plan.

- [x] **Step 4: Final full verification**

Run: `make stage2 && make boot-test 2>&1 | tail -20`
Expected: self-host converges; full suite PASS.

- [x] **Step 5: Commit**

```bash
git add boot/bench/README.md
git commit -m "bench: record dict remove drop from O(n^2) to amortized O(log n)

dict_int_remove flips from ~4x/doubling (10s@32k) to linear after the
tombstoned-order-vector remove. Other dict/set benches unchanged."
```

---

## Risks & watch-items

- **`order_index` staleness** is the core hazard. It is stamped on insert, preserved on replace (Task 1 Steps 5–6), and only ever invalidated by `compact`, which rebuilds every entry. Any new entry-creation path added later must stamp it. The interleaved-set/remove characterization test is the guard.
- **Compaction churn** at the threshold boundary: the dead ≥ live (50%) hysteresis means a steady-state alternating add/remove cannot trigger repeated compactions.
- **`arr_set`/`arr_set_in_place` semantics on a `null` value**: the order `Array` element type is `anyref`, so storing `null` is valid; `keys`/`compact` skip nulls via `RefIsNull`.
- **Equality must ignore `order_index`**: confirmed structural/membership equality (Task 1 Step 1 equality test). If equality ever compared raw `HamtEntry` structs, the new field would break it — the test catches that.
- **`keys()` O(1) fast path**: preserved only while `tombstones == 0`. A dict with pending tombstones pays O(n) for `keys()`/`values()`/iteration until the next compaction — acceptable, since those already materialize an O(n) vector.

## Success criteria

- `dict_int_remove` bulk benchmark is linear (≈2×/doubling), no longer ~10 s at 32k.
- All other dict/set benches within noise of Phase 0 baselines.
- `make stage2` converges and `make boot-test` passes, including new order/persistence/equality/interleaved characterization tests.
- No source-language or public-API change; `Dict`/`Set` semantics (insertion order, persistence, order-independent equality) preserved.
