# MutDict Publication Adapter — Extraction & Test Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** Completed and archived. The adapter extraction, engineered-hash seam, adversarial builder fixtures, and mutation-proven compaction publication regression are landed.

**Goal:** Extract the bottom-up HAMT builder into a reusable compiler-private `freeze_dense` publication adapter, route `Dict.compact()` through it, and cover the previously-untested deep-prefix and collision builder paths with an engineered-hash test seam.

**Architecture:** The runtime dict (`boot/compiler/codegen/runtime/dict.tw`) is instruction-emitting code — each `*_fn()` returns a Wasm `FuncDef`. This slice pulls `compact()`'s inline tail (`node_build_bottom_up` + bulk order) into two new emitted functions, `freeze_dense(dense, len)` and `build_order_bulk(dense, len)`, matching the design in [mutdict-bottom-up-publication-adapter.md](mutdict-bottom-up-publication-adapter.md) §2. A temporary source-callable test seam (`Dict.build_dense_test` / `Dict.get_by_hash`) lets boot tests drive the builder with engineered hashes, which is the only way to reach the full-hash-collision path (the real `hash_i64` is a non-invertible wyhash mix).

**Tech Stack:** Twinkle (`.tw`), the boot self-hosted compiler, `@std.testing` suites, `twk wat` for codegen inspection.

## Global Constraints

- **Scope is the now-implementable half only.** The forward MutDict lifecycle/poison ABI (design §4) and MutDict-arena consumption (design §7) are **out of scope** — they are blocked on the unbuilt S5 MutDict runtime.
- **Boot-only.** All changes live under `boot/`. No `src/runtime/` mirror is added (design §9 stage0 disposition). The new builtins are registered in `boot/compiler/builtins.tw` but are **not** called from `boot/main.tw`, so `make stage2` stays green exactly as it does for the existing `dict$bench_*` builtins.
- **Do not remove the existing spike surfaces** (`node_build_sequential`/`_editable`, `bench_builders`/`bench_timings`/`bench_builder_rotation`, the bench). Their removal (design §9.7) is gated on real MutDict evidence, which this slice does not produce.
- **`node_build_bottom_up` keeps its name.** Design §2 treats the `build_hamt_bottom_up` rename as cosmetic and optional; renaming would churn the bench call sites and wat commands for no behavioral gain, so this slice keeps the existing private name.
- **Keep `Dict.set` / `Dict.remove` / `Dict.get` / `node_set` / `node_get` untouched.**
- **Observational equivalence is the bar** (design §6), not bit-identical tree shape.
- **Per-task validation** (run from repo root):
  ```bash
  target/twk fmt boot/compiler/codegen/runtime/dict.tw boot/compiler/builtins.tw \
    boot/prelude/signatures/dict.tw boot/tests/suites/<changed>.tw
  target/twk lint boot/main.tw    # baseline: 5 unrelated record-copy-helper findings; acceptance = no NEW finding
  make stage2
  make quick-bundle-cli
  target/twk test
  ```
  Heavy commands (`make stage2`, `make quick-bundle-cli`, `target/twk test`) run **one at a time, never concurrently or backgrounded**.
- **Do not run tree-sitter tests** — this slice does not touch the grammar.

---

## File Structure

- `boot/compiler/codegen/runtime/dict.tw` — add `build_order_bulk_fn()`, `freeze_dense_fn()`, and the temporary `build_dense_test_fn()` / `get_by_hash_fn()`; edit `compact_fn()` to call `freeze_dense`; register all four in `module()`.
- `boot/compiler/builtins.tw` — ABI contracts + `rt(...)` registrations for `dict$build_dense_test` / `dict$get_by_hash` (temporary).
- `boot/prelude/signatures/dict.tw` — source signatures for the two temporary methods.
- `boot/tests/suites/dict_builder_shapes_suite.tw` — **new, temporary** suite driving engineered-hash builder shapes.
- `boot/tests/suites/api_dict_suite.tw` — add one permanent compaction / `order_index`-rewrite test.
- `boot/tests/main.tw` — register the new temporary suite.

---

## Task 1: Extract `freeze_dense` + `build_order_bulk`, route `compact()` through them

Pure refactor of `compact()`'s tail. The safety net is the existing `target/twk test` dict coverage plus the bench parity oracle (`boot/bench/dict_compact_builder_spike.tw`), which already compares builder output against persistent construction at scale.

**Files:**
- Modify: `boot/compiler/codegen/runtime/dict.tw` (add two `*_fn`, edit `compact_fn`, edit `module()`)

**Interfaces:**
- Consumes: existing `node_build_bottom_up` (`ref Array, i32 lo, i32 hi, i32 depth -> ref HamtNode`), `arr_from_array`, the `HamtEntry`/`PDict` structs, and the constants already in scope in `dict.tw` (`t_ARRAY`, `t_PDICT`, `t_HAMT_NODE`, `he_KEY`, `arr_REF`, `hamt_NODE_NULL`, `pdict_REF`, `pvec_REF`).
- Produces:
  - `build_order_bulk(dense: ref Array, len: i32) -> ref PVec` — order keys pulled from `dense[j].key`.
  - `freeze_dense(dense: ref Array, len: i32) -> ref PDict` — `root = len==0 ? null : node_build_bottom_up(dense,0,len,0)`; `order = build_order_bulk(dense,len)`; `PDict{ size:len, root, order, tombstones:0 }`.

- [ ] **Step 1: Establish the green baseline.**

Run, and confirm both pass before editing:
```bash
target/twk test
target/twk run boot/bench/dict_compact_builder_spike.tw    # every line must end parity=PASS
```
Expected: `target/twk test` green; bench prints `parity=PASS` on all sample lines. If either is red before you start, stop and report.

- [ ] **Step 2: Add `build_order_bulk_fn()` and `freeze_dense_fn()`.**

Insert immediately **before** `fn compact_fn()` in `dict.tw`:

```tw
// ── build_order_bulk(dense: ref Array, len: i32) -> ref PVec ──────────────────
// Insertion-order key vector pulled directly from the dense stream: order[j] is
// dense[j].key, since dense is already in insertion order (order_index == j).
// Subsumes the parallel keys[] array compact() used to maintain.
fn build_order_bulk_fn() FuncDef {
  // p0=dense, p1=len; L2=keys, L3=i
  .{
    name: "build_order_bulk",
    params: [arr_REF, .I32],
    results: [pvec_REF],
    locals: [arr_NULL, .I32],
    body: [
      .RefNull(.Any),
      .LocalGet(1),
      .ArrayNew(t_ARRAY),
      .LocalSet(2),
      .I32Const(0),
      .LocalSet(3),
      .Block(
        "oexit",
        .None,
        [
          .Loop(
            "oloop",
            .None,
            [
              .LocalGet(3),
              .LocalGet(1),
              .I32GeS,
              .BrIf("oexit"),
              .LocalGet(2),
              .RefAsNonNull,
              .LocalGet(3),
              .LocalGet(0),
              .LocalGet(3),
              .ArrayGet(t_ARRAY),
              .RefCast(false, .Named(t_HAMT_ENTRY)),
              .StructGet(t_HAMT_ENTRY, he_KEY),
              .ArraySet(t_ARRAY),
              .LocalGet(3),
              .I32Const(1),
              .I32Add,
              .LocalSet(3),
              .Br("oloop"),
            ],
          ),
        ],
      ),
      .LocalGet(2),
      .RefAsNonNull,
      .Call("arr_from_array"),
    ],
  }
}

// ── freeze_dense(dense: ref Array, len: i32) -> ref PDict ─────────────────────
// The shared publication adapter (design §2): build the persistent root in one
// bottom-up pass and the order vector in bulk, then assemble an ordinary PDict.
// dense[0..len) must be the hole-free, core_eq-unique live stream in insertion
// order; the builder performs no rehash and no node_get.
fn freeze_dense_fn() FuncDef {
  // p0=dense, p1=len; L2=root
  .{
    name: "freeze_dense",
    params: [arr_REF, .I32],
    results: [pdict_REF],
    locals: [hamt_NODE_NULL],
    body: [
      .RefNull(.Named(t_HAMT_NODE)),
      .LocalSet(2),
      .LocalGet(1),
      .I32Const(0),
      .I32GtS,
      .If(
        .None,
        [
          .LocalGet(0),
          .I32Const(0),
          .LocalGet(1),
          .I32Const(0),
          .Call("node_build_bottom_up"),
          .LocalSet(2),
        ],
        [],
      ),
      .LocalGet(1),
      .LocalGet(2),
      .LocalGet(0),
      .LocalGet(1),
      .Call("build_order_bulk"),
      .RefAsNonNull,
      .I32Const(0),
      .StructNew(t_PDICT),
    ],
  }
}
```

- [ ] **Step 3: Register both in `module()`.**

In the `funcs := [ ... ]` list in `pub fn module()`, add the two new functions right after `node_build_bottom_up_fn(),` (and before `node_remove_fn(),`):
```tw
    node_build_bottom_up_fn(),
    build_order_bulk_fn(),
    freeze_dense_fn(),
    node_remove_fn(),
```
(Exports are auto-derived from `funcs`, so no separate export edit is needed.)

- [ ] **Step 4: Rewrite `compact_fn()`'s tail to call `freeze_dense`, and drop the now-dead `keys[]` array.**

In `compact_fn()`:
1. Delete the `keys[n]` allocation (the second `RefNull(.Any) / LocalGet(2) / ArrayNew(t_ARRAY) / LocalSet(4)` block that fills local `L4`).
2. Inside the dense-build loop, delete the `keys[j] = key` store (the `LocalGet(4) / RefAsNonNull / LocalGet(5) / LocalGet(7) / ArraySet(t_ARRAY)` sequence). Keep the `dense[j] = HamtEntry{...}` store and the `j++`.
3. Replace the entire post-loop tail — from `// root = j == 0 ? null : node_build_bottom_up(...)` through the final `.StructNew(t_PDICT)` — with:
```tw
      // dense[0..j) is the hole-free live stream; publish via the shared adapter.
      .LocalGet(3),
      .RefAsNonNull,
      .LocalGet(5),
      .Call("freeze_dense"),
```
4. Remove the now-unused locals from `compact_fn`'s `locals` list and fix the comment: `L4` (keys), `L10` (root), `L11` (order), `L12` (keys_trim), `L13` (old_root is still used for `node_get` value recovery — **keep L13**). Renumber remaining local indices if you remove entries from the middle; the safe approach is to leave the removed slots as declared-but-unused placeholders (e.g. keep `arr_NULL` at L4) rather than renumber, to avoid shifting every `LocalGet` — pick whichever the reviewer prefers, but **do not** change any surviving `LocalGet` index without also changing its declaration.

> Reviewer note: the lowest-risk edit keeps all local *indices* stable (leaving L4/L10/L11/L12 declared but unwritten) and only removes the *stores* and the tail. That minimizes the diff and the chance of an index-shift bug.

- [ ] **Step 5: Verify parity and shape.**

```bash
target/twk fmt boot/compiler/codegen/runtime/dict.tw
make stage2
make quick-bundle-cli
target/twk test
target/twk run boot/bench/dict_compact_builder_spike.tw     # still parity=PASS
target/twk wat boot/tests/main.tw --func rt_dict__compact --calls
```
Expected: `target/twk test` green; bench all `parity=PASS`; the `compact` `--calls` output now lists `freeze_dense` (and no longer builds order via a direct `arr_from_array` in `compact` itself — that moved into `build_order_bulk`). `node_build_bottom_up` still appears (now reached through `freeze_dense`).

- [ ] **Step 6: Lint and commit.**

```bash
target/twk lint boot/main.tw    # confirm no NEW finding beyond the baseline 5
git add boot/compiler/codegen/runtime/dict.tw
git commit -m "refactor(rt.dict): extract freeze_dense + build_order_bulk from compact"
```

---

## Task 2: Add the engineered-hash test seam

Two temporary source-callable runtime functions so boot tests can build a dict from explicit `(hash, key, value)` triples and look up by an explicit hash. Modeled on the existing `dict$bench_*` builtins (same wiring shape, same `make stage2` safety).

**Files:**
- Modify: `boot/compiler/codegen/runtime/dict.tw` (add `build_dense_test_fn()`, `get_by_hash_fn()`, register in `module()`)
- Modify: `boot/compiler/builtins.tw` (ABI + `rt(...)`)
- Modify: `boot/prelude/signatures/dict.tw` (source signatures)
- Create: `boot/tests/suites/dict_builder_shapes_suite.tw` (smoke test)
- Modify: `boot/tests/main.tw` (register suite)

**Interfaces:**
- Consumes: `freeze_dense` (Task 1), `node_get` (`ref null HamtNode? , i64 hash, i32 depth, anyref key -> anyref`), `arr_get`/`arr_len` imports, `hash_key`'s unbox idiom, the constants `t_ARRAY`, `t_HAMT_ENTRY`, `t_BOXED_INT`, `t_PDICT`, `pd_ROOT`, `t_VARIANT`, `variant_REF`, `pdict_NULL`.
- Produces (source-visible, temporary):
  - `Dict.build_dense_test(hashes: Vector<Int>, keys: Vector<Int>, vals: Vector<Int>) Dict<Int, Int>`
  - `d.get_by_hash(hash: Int, key: Int) Option<Int>`

- [ ] **Step 1: Write the failing smoke test.**

Create `boot/tests/suites/dict_builder_shapes_suite.tw`:
```tw
use @std.testing.assert as assert
use @std.testing as runner

// Temporary suite: drives node_build_bottom_up via the engineered-hash seam
// (Dict.build_dense_test / d.get_by_hash). Removed with the seam once real
// MutDict tests exist. hash_i64 is a non-invertible wyhash mix, so engineered
// hashes are the only way to reach the collision / deep-prefix builder paths.

pub fn suite() runner.Suite {
  runner
    .suite("dict_builder_shapes")
    .test(
      "seam builds and looks up distinct entries in order",
      fn() {
        d := Dict.build_dense_test([10, 20, 30], [1, 2, 3], [100, 200, 300])
        try assert.equal(d.len(), 3)
        try assert.equal(d.get_by_hash(10, 1), .Some(100))
        try assert.equal(d.get_by_hash(20, 2), .Some(200))
        try assert.equal(d.get_by_hash(30, 3), .Some(300))
        ks := d.keys()
        try assert.equal(ks.len(), 3)
        try assert.equal(ks[0], 1)
        try assert.equal(ks[1], 2)
        try assert.equal(ks[2], 3)
        .Ok({})
      },
    )
}
```

- [ ] **Step 2: Register the suite and run to verify it fails.**

In `boot/tests/main.tw`, add the suite to the registration list next to the other `*_suite` entries (mirror how `api_dict_suite` is added — `use` its module and include `<module>.suite()` in the suites vector).

Run:
```bash
target/twk test
```
Expected: FAIL — `Dict.build_dense_test` / `get_by_hash` are unknown (unresolved method / builtin), or a link error. This confirms the seam is not yet wired.

- [ ] **Step 3: Add the two runtime emit functions.**

Insert after `freeze_dense_fn()` in `dict.tw`:
```tw
// ── build_dense_test(hashes, keys, vals) -> ref PDict ─── TEST-ONLY (temporary) ─
// Build a PDict directly from parallel engineered (hash, key, value) vectors,
// bypassing hash_key so tests can force deep-prefix and full-hash-collision
// shapes. Keys/vals are stored as their already-boxed anyref elements; only the
// hash is unboxed to i64 (mirroring hash_key's i31 / BoxedInt idiom).
fn build_dense_test_fn() FuncDef {
  // p0=hashes, p1=keys, p2=vals; L3=n, L4=dense, L5=i, L6=hbox, L7=h
  .{
    name: "build_dense_test",
    params: [pvec_NULL, pvec_NULL, pvec_NULL],
    results: [pdict_REF],
    locals: [.I32, arr_NULL, .I32, .Anyref, .I64],
    body: [
      .LocalGet(0),
      .Call("arr_len"),
      .LocalSet(3),
      .RefNull(.Any),
      .LocalGet(3),
      .ArrayNew(t_ARRAY),
      .LocalSet(4),
      .I32Const(0),
      .LocalSet(5),
      .Block(
        "dexit",
        .None,
        [
          .Loop(
            "dloop",
            .None,
            [
              .LocalGet(5),
              .LocalGet(3),
              .I32GeS,
              .BrIf("dexit"),
              // hbox = hashes[i]; h = unbox(hbox)
              .LocalGet(0),
              .LocalGet(5),
              .Call("arr_get"),
              .LocalSet(6),
              .LocalGet(6),
              .RefTest(false, .I31),
              .If(
                .Some(.I64),
                [.LocalGet(6), .RefCast(false, .I31), .I31GetU, .I64ExtendI32U],
                [
                  .LocalGet(6),
                  .RefCast(false, .Named(t_BOXED_INT)),
                  .StructGet(t_BOXED_INT, 0),
                ],
              ),
              .LocalSet(7),
              // dense[i] = HamtEntry{ h, keys[i], vals[i], order_index: i }
              .LocalGet(4),
              .RefAsNonNull,
              .LocalGet(5),
              .LocalGet(7),
              .LocalGet(1),
              .LocalGet(5),
              .Call("arr_get"),
              .LocalGet(2),
              .LocalGet(5),
              .Call("arr_get"),
              .LocalGet(5),
              .StructNew(t_HAMT_ENTRY),
              .ArraySet(t_ARRAY),
              .LocalGet(5),
              .I32Const(1),
              .I32Add,
              .LocalSet(5),
              .Br("dloop"),
            ],
          ),
        ],
      ),
      .LocalGet(4),
      .RefAsNonNull,
      .LocalGet(3),
      .Call("freeze_dense"),
    ],
  }
}

// ── get_by_hash(dict, hash, key) -> variant Option ─── TEST-ONLY (temporary) ────
// Look up using an explicitly supplied hash (not hash_key), so engineered
// collision/deep-prefix entries — whose stored hash is decoupled from the key —
// are reachable. key arrives as i64; box it as i31 to match the stored boxed
// small-int keys (tests keep keys < 2^30). Option wrapping mirrors get_option.
fn get_by_hash_fn() FuncDef {
  // p0=dict, p1=hash(i64), p2=key(i64); L3=keybox, L4=val
  .{
    name: "get_by_hash",
    params: [pdict_NULL, .I64, .I64],
    results: [variant_REF],
    locals: [.Anyref, .Anyref],
    body: [
      .LocalGet(2),
      .I32WrapI64,
      .RefI31,
      .LocalSet(3),
      .LocalGet(0),
      .RefAsNonNull,
      .StructGet(t_PDICT, pd_ROOT),
      .LocalGet(1),
      .I32Const(0),
      .LocalGet(3),
      .Call("node_get"),
      .LocalSet(4),
      .LocalGet(4),
      .RefIsNull,
      .If(
        .Some(variant_REF),
        [.I32Const(0), .I32Const(0), .RefNull(.Named(t_ARRAY)), .StructNew(t_VARIANT)],
        [
          .I32Const(0),
          .I32Const(1),
          .LocalGet(4),
          .ArrayNewFixed(t_ARRAY, 1),
          .StructNew(t_VARIANT),
        ],
      ),
    ],
  }
}
```

Register both in `module()`'s `funcs` list next to the bench functions:
```tw
    bench_timings_fn(),
    build_dense_test_fn(),
    get_by_hash_fn(),
```

- [ ] **Step 4: Wire the builtins.**

In `boot/compiler/builtins.tw`, add ABI contracts next to the `dict$bench_*` entries:
```tw
    "dict$build_dense_test" => abi([pvec_n(), pvec_n(), pvec_n()], [dict_()]),
    "dict$get_by_hash" => abi([dict_n(), .I64, .I64], [variant_()]),
```
And add `rt(...)` registrations next to the `dict$bench_*` registrations:
```tw
    .append(rt("dict$build_dense_test", "rt.dict", "build_dense_test", .Some("Dict.build_dense_test")))
    .append(rt("dict$get_by_hash", "rt.dict", "get_by_hash", .Some("Dict.get_by_hash")))
```

In `boot/prelude/signatures/dict.tw`, add source signatures next to the `bench_*` stubs:
```tw
/// Test-only: build a Dict from engineered (hash, key, value) triples, bypassing hash_key.
pub fn build_dense_test(hashes: Vector<Int>, keys: Vector<Int>, vals: Vector<Int>) Dict<Int, Int> {
  Dict.new()
}

/// Test-only: look up a key using an explicitly supplied hash (for engineered shapes).
pub fn get_by_hash(d: Dict<Int, Int>, hash: Int, key: Int) Option<Int> {
  .None
}
```

- [ ] **Step 5: Rebuild and verify the smoke test passes.**

```bash
target/twk fmt boot/compiler/codegen/runtime/dict.tw boot/compiler/builtins.tw \
  boot/prelude/signatures/dict.tw boot/tests/suites/dict_builder_shapes_suite.tw
make stage2
make quick-bundle-cli
target/twk test
```
Expected: `target/twk test` green, including the new `dict_builder_shapes` smoke test. If `make stage2` fails, the seam ABI is using a feature the emitter can't lower — recheck the ABI helper names (`pvec_n`, `dict_n`, `dict_`, `variant_`) against their definitions in `builtins.tw`.

- [ ] **Step 6: Lint and commit.**

```bash
target/twk lint boot/main.tw    # no NEW finding beyond baseline
git add boot/compiler/codegen/runtime/dict.tw boot/compiler/builtins.tw \
  boot/prelude/signatures/dict.tw boot/tests/suites/dict_builder_shapes_suite.tw boot/tests/main.tw
git commit -m "test(rt.dict): add engineered-hash builder test seam"
```

---

## Task 3: Cover empty, singleton, deep-prefix, and full-hash-collision shapes

Add the adversarial builder fixtures the `compact()` seam can't produce. These test `node_build_bottom_up`'s singleton-leaf, deep-recursion, and collision branches directly.

**Files:**
- Modify: `boot/tests/suites/dict_builder_shapes_suite.tw`

**Interfaces:**
- Consumes: `Dict.build_dense_test`, `d.get_by_hash` (Task 2).

- [ ] **Step 1: Add the empty and singleton fixtures.**

Add these `.test(...)` cases to the `suite()` chain in `dict_builder_shapes_suite.tw`:
```tw
    .test(
      "empty dense input yields empty dict",
      fn() {
        empty: Vector<Int> = []
        d := Dict.build_dense_test(empty, empty, empty)
        try assert.equal(d.len(), 0)
        try assert.equal(d.keys().len(), 0)
        try assert.equal(d.get_by_hash(0, 0), .None)
        .Ok({})
      },
    )
    .test(
      "singleton dense input yields a one-entry dict",
      fn() {
        d := Dict.build_dense_test([42], [7], [700])
        try assert.equal(d.len(), 1)
        try assert.equal(d.get_by_hash(42, 7), .Some(700))
        try assert.equal(d.get_by_hash(42, 8), .None)
        ks := d.keys()
        try assert.equal(ks.len(), 1)
        try assert.equal(ks[0], 7)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Add the deep-shared-prefix fixture.**

Two entries whose hashes share the low 50 bits (10 five-bit fragments) and diverge only at fragment 10, forcing a deep single-child `HamtNode` chain. Add:
```tw
    .test(
      "deep shared hash prefix builds a nested node chain",
      fn() {
        // low 50 bits equal (fragments 0..9 shared); differ at bit 50 (fragment 10)
        shared_lo := 5
        h_a := shared_lo
        h_b := shared_lo + (1 << 50)
        d := Dict.build_dense_test([h_a, h_b], [111, 222], [1, 2])
        try assert.equal(d.len(), 2)
        try assert.equal(d.get_by_hash(h_a, 111), .Some(1))
        try assert.equal(d.get_by_hash(h_b, 222), .Some(2))
        // wrong hash must miss even with a real key
        try assert.equal(d.get_by_hash(h_a, 222), .None)
        .Ok({})
      },
    )
```

- [ ] **Step 3: Add the full-hash-collision fixture.**

Two `core_eq`-distinct keys with the **same** full 64-bit hash → a `HamtCollision` at depth ≥ 12, both retrievable via `collision_get`'s `core_eq` scan. Add:
```tw
    .test(
      "equal full hash with distinct keys forms a retrievable collision",
      fn() {
        h := 1 << 40                 // any shared 64-bit hash
        d := Dict.build_dense_test([h, h], [11, 22], [1, 2])
        try assert.equal(d.len(), 2)
        try assert.equal(d.get_by_hash(h, 11), .Some(1))
        try assert.equal(d.get_by_hash(h, 22), .Some(2))
        try assert.equal(d.get_by_hash(h, 33), .None)
        ks := d.keys()
        try assert.equal(ks.len(), 2)
        try assert.equal(ks[0], 11)   // insertion order preserved through collision
        try assert.equal(ks[1], 22)
        .Ok({})
      },
    )
```

- [ ] **Step 4: Run and verify all shapes pass.**

```bash
target/twk fmt boot/tests/suites/dict_builder_shapes_suite.tw
make quick-bundle-cli
target/twk test
```
Expected: green, all five `dict_builder_shapes` tests passing. (No `make stage2` needed — only test source changed.) If the collision test fails to *find* a key, the `core_eq` key-representation assumption is off: confirm test keys are small (< 2^30) so both the stored and the re-boxed lookup key are `i31`.

- [ ] **Step 5: Commit.**

```bash
git add boot/tests/suites/dict_builder_shapes_suite.tw
git commit -m "test(rt.dict): cover empty/singleton/deep-prefix/collision builder shapes"
```

---

## Task 4: Permanent compaction + `order_index`-rewrite test

A real-key test that forces `compact()` (dead ≥ live) and then does a further persistent `remove`, proving the conversion pass rewrote `order_index = output_position` (design §7) so the correct order slot is tombstoned afterward.

**Files:**
- Modify: `boot/tests/suites/api_dict_suite.tw`

**Interfaces:**
- Consumes: ordinary `Dict.new`, `d[k]=v`, `.remove`, `.len`, `.keys`, `d[k]` — no seam.

- [ ] **Step 1: Add the test.**

Add this `.test(...)` to the `suite()` chain in `api_dict_suite.tw`:
```tw
    .test(
      "compaction after heavy removal preserves order and later removal",
      fn() {
        // Build 2N keys, then remove the odd-indexed half one at a time. The
        // removals drive dead >= live, triggering internal compact(); compact
        // rebuilds order and must rewrite each surviving entry's order_index.
        d: Dict<Int, Int> = Dict.new()
        n := 64
        i := 0
        for i < 2 * n {
          d[i] = i * 10
          i = i + 1
        }
        i = 1
        for i < 2 * n {
          d = .remove(i)
          i = i + 2
        }
        // Survivors are the even keys 0,2,4,...,2n-2 in insertion order.
        try assert.equal(d.len(), n)
        ks := d.keys()
        try assert.equal(ks.len(), n)
        for k, j in ks {
          try assert.equal(k, j * 2)
          try assert.equal(d.get(k), .Some(j * 20))
        }
        // A further removal after compaction must tombstone the correct slot:
        // remove key 0 (order position 0); everything else stays intact.
        d = .remove(0)
        try assert.equal(d.len(), n - 1)
        try assert.equal(d.get(0), .None)
        try assert.equal(d.get(2), .Some(20))
        ks2 := d.keys()
        try assert.equal(ks2.len(), n - 1)
        try assert.equal(ks2[0], 2)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run and verify it passes.**

```bash
target/twk fmt boot/tests/suites/api_dict_suite.tw
make quick-bundle-cli
target/twk test
```
Expected: green. If the survivors' order or a `get` is wrong, `order_index` was not rewritten correctly during compaction — inspect `compact_fn` (the `order_index: j` field of each `dense[j]`) and `build_order_bulk`.

- [ ] **Step 3: Commit.**

```bash
git add boot/tests/suites/api_dict_suite.tw
git commit -m "test(dict): cover compaction order-index rewrite and post-compaction removal"
```

---

## Self-Review

**Spec coverage** (against design §2/§6/§8 now-implementable subset):
- `freeze_dense` / `build_order_bulk` extraction (design §2) → Task 1. ✔
- `compact()` routed through the adapter (design §2/§7 compact customer) → Task 1. ✔
- Test seam for engineered hashes (design §8 "required") → Task 2. ✔
- empty / singleton / deep-prefix / full-hash-collision / insertion-order fixtures (design §8) → Tasks 2–3. ✔
- `order_index` rewrite + post-publication persistent removal (design §7/§8) → Task 4. ✔
- **Deferred, explicitly out of scope:** forward lifecycle/poison tests, MutDict-arena consumption, spike-surface cleanup, ping-pong (all gated on S5 MutDict — design §3/§4/§9.7). The `core_eq`-equal-collapse case is a **producer** concern (it violates `freeze_dense`'s `core_eq`-unique precondition) and is already covered by ordinary `Dict.set` overwrite tests in `api_dict_suite`; not re-tested here.

**Placeholder scan:** No TBD/TODO. Every emit function has a full instruction body; every test has full Twinkle. The one judgment call left to the reviewer (local-index removal vs. leave-declared in Task 1 Step 4) is stated explicitly with the recommended low-risk option.

**Type consistency:** `freeze_dense(dense: ref Array, len: i32) -> ref PDict` and `build_order_bulk(dense: ref Array, len: i32) -> ref PVec` are used consistently in Task 1 (compact call), Task 2 (`build_dense_test` calls `freeze_dense`). Seam source types `Dict.build_dense_test(Vector<Int>×3) Dict<Int,Int>` and `d.get_by_hash(Int, Int) Option<Int>` match their ABI (`[pvec_n×3]→[dict_]`, `[dict_n,.I64,.I64]→[variant_]`) and their test call sites.
