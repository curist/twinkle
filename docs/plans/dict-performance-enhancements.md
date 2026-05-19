# Dict Runtime Performance Enhancements

## Goal

Improve the boot compiler's current persistent dict (`PDict`) runtime without
changing Twinkle `Dict<K,V>` semantics, the public Dict API, or the broader
long-term typed-container plan.

This is an incremental performance plan for the existing erased HAMT
implementation in:

- `boot/compiler/codegen/runtime/dict.tw`
- boot-side codegen/lowering paths that call the `rt.dict` helpers

The main objective is to reduce per-operation trie traversal overhead, eliminate
unnecessary software emulation of hardware instructions, and improve bulk
operations on the insertion-order tracking vector.

## Relationship to Other Plans

This plan follows the same incremental philosophy as the completed
[`pvec-performance-enhancements.md`](archive/pvec-performance-enhancements.md).

- [`persistent-dict.md`](persistent-dict.md) tracks the long-term typed HAMT
  direction.
- [`backend-anyref-elimination.md`](backend-anyref-elimination.md) covers the
  broader typed-container and anyref removal effort.
- This plan assumes the current erased HAMT representation remains in place and
  focuses on making that representation cheaper for the boot compiler.

## Current State

The current boot runtime uses a persistent HAMT (Hash Array Mapped Trie) with
insertion-order tracking:

- `PDict { size: i32, root: HamtNode?, order: PVec }`
- `HamtNode { bitmap: i32, entries: Array }` — sparse array indexed by 5-bit
  hash fragments
- `HamtEntry { hash: i64, key: anyref, val: anyref }` — leaf key-value pair
- `HamtCollision { tag: i32, hash: i64, entries: Array }` — hash-collision
  bucket
- `popcount` computes the compressed index from the bitmap

Important current costs:

- `set` traverses the trie twice: once via `node_get` to check whether the key
  already exists (to decide whether to grow the order vector and bump size),
  then again via `node_set` to perform the actual insert/update.
- `remove` similarly traverses twice: `node_get` to check presence, then
  `node_remove` to do the actual removal.
- `order_remove_key` rebuilds the entire insertion-order PVec element-by-element,
  calling `arr_get` (O(log₃₂ n) per element) and `arr_push` for each
  non-matching key. Total cost is O(n · log₃₂ n).
- `popcount` uses a Kernighan software loop instead of the Wasm `i32.popcnt`
  instruction, adding loop overhead on every HAMT node access.
- `set_in_place` and `remove_in_place` simply delegate to the persistent `set`
  and `remove` paths, performing full path-copy even when the optimizer has
  proven uniqueness.
- `make` allocates a fresh empty PVec inline instead of reusing a shared empty
  PVec global.

## Non-Goals

- Do not change the surface `Dict<K,V>` API.
- Do not change immutable/persistent semantics visible to users.
- Do not introduce mutable-only dicts.
- Do not implement per-concrete typed dict families as part of this plan.
- Do not rewrite `PVec` or unrelated runtime containers.

## Phase 1 — Replace Software Popcount with `i32.popcnt`

`popcount_fn` (line 88) uses a Kernighan bit-counting loop that iterates once
per set bit. Every `node_get`, `node_set`, and `node_remove` call invokes
`popcount` at least once per trie level to compute the compressed array index
from the HAMT bitmap.

WebAssembly has `i32.popcnt` as a single instruction in the MVP spec,
universally supported by all engines.

Target: replace the entire loop body with:

```text
[.LocalGet(0), .I32Popcnt]
```

This eliminates the local variable, the block/loop structure, and the per-bit
iteration. The function signature stays the same.

Implementation notes:

- Add `.I32Popcnt` to the `Instr` enum in `boot/compiler/codegen/wasm_ir.tw` if
  not already present.
- Add an emitter case for `I32Popcnt` in the WAT text emitter
  (`boot/compiler/codegen/wat.tw`) emitting `i32.popcnt`.
- Add an emitter case in the binary Wasm emitter emitting opcode `0x69`.
- Update `popcount_fn` in `dict.tw` to use the single instruction.
- The locals list becomes empty and the body is two instructions.
- Update the Rust stage0 mirror if it tracks boot IR/runtime instruction
  coverage (check `src/runtime/dict.rs` and the Rust `Instr` enum).
- Update or add a test verifying `popcount` uses `I32Popcnt`.

## Phase 2 — Bulk Order Vector Rebuilding in `order_remove_key`

`order_remove_key_fn` (line 821) iterates every element of the insertion-order
PVec, calling `arr_get` (which traverses the PVec trie, O(log₃₂ n) per
element) and `arr_push` to build a new PVec excluding the removed key. Total
cost is O(n · log₃₂ n) for the scan.

The `rt.arr` module already has `to_array` (bulk PVec → flat Array via leaf
copies) and `from_array` (bulk flat Array → PVec). These were optimized in the
PVec performance work to use `array.copy` at the leaf level.

Target behavior:

- Call `to_array(order)` to get a flat Array. This traverses per leaf (not per
  element) via `get_leaf`, so it is O((n/32) · log₃₂ n) for trie navigation
  plus O(n) for bulk `array.copy` at the leaf level.
- Scan the flat array linearly, compacting non-matching keys into a new array —
  O(n) with no trie traversal.
- Call `from_array(compacted)` to reconstruct a PVec via chunk promotion.

The key improvement is replacing per-element PVec traversal (O(n · log₃₂ n))
with per-leaf traversal and bulk copies, which is substantially cheaper in
practice even though both are technically O(n · log₃₂ n) — the constant factor
drops by ~32×.

Implementation notes:

- Import `to_array` and `from_array` from `rt.arr` into the dict module's
  import list (currently only `push`, `len`, and `get` are imported).
- Allocate a flat result array of size `n - 1`. Precondition: this function is
  only called from `remove` after presence has already been confirmed (the key
  is known to exist in the order vector exactly once). This precondition must be
  documented and tested — if the key is absent, the compaction would produce a
  wrong-length array. Key equality is structural (`core_eq`), so the same
  equality function used by the HAMT lookup must be used for the scan.
- Linear scan: for each element, call `core_eq` against the removal key; if not
  equal, copy to the result array at the write cursor.
- Call `from_array(result)` to produce the new PVec.
- Keep the existing loop as a conceptual reference but replace its
  implementation entirely.

## Phase 3 — Eliminate Double Trie Traversal in `set` and `remove`

### `set`

`set_fn` (line 919) currently does:

```text
was_present = node_get(root, hash, 0, key) != null   // traversal 1
new_root    = node_set(root, hash, 0, key, val)      // traversal 2
if !was_present: order = arr_push(order, key)
size += !was_present
```

Both `node_get` and `node_set` traverse the same path from root to the target
slot. The only information needed from the first traversal is a boolean: "did
this key already exist?"

Target: make `node_set` communicate whether the key was already present.

Approach — use a module-level mutable global `$was_replace : i32`:

- Before calling `node_set`, set `$was_replace = 0`.
- Inside `node_set`, when the code path replaces an existing entry (same key),
  set `$was_replace = 1`.
- After `node_set` returns, read `$was_replace` to decide whether to append to
  the order vector and bump size.
- Remove the `node_get` call from `set_fn`.

This avoids changing `node_set`'s return type (which would ripple into recursive
calls) while still communicating the needed flag.

### `remove`

`remove_fn` (line 945) currently does:

```text
was_present = node_get(root, hash, 0, key) != null   // traversal 1
new_root    = node_remove(root, hash, 0, key)        // traversal 2
if was_present: order = order_remove_key(order, key)
size -= was_present
```

A naive approach would be to compare `ref.eq(old_root, new_root)` after
`node_remove`, since persistent data structures typically return the original
reference on no-op. However, the current `node_remove` implementation does not
propagate no-op identity: when the searched path descends into a child
`HamtNode`, the parent always calls `arr_replace_at` and allocates a new parent
node even if the child returned unchanged. So a nested miss can change the root
identity without removing anything.

Two options:

**Option A — fix `node_remove` to propagate no-op identity.** After each
recursive `node_remove(child, ...)` call, compare the returned child with the
original child via `ref.eq`. If they are the same, return the current node
unchanged (skip `arr_replace_at`). This is a small change at each recursion
site in `node_remove_fn` and restores the expected persistent data structure
property. Once `node_remove` correctly returns the original root on miss,
`remove_fn` can use `ref.eq(old_root, new_root)` to detect absence.

**Option B — use the `was_replace` global.** Reuse the same module-level
mutable global from the `set` path. Set `was_replace = 0` before calling
`node_remove`; inside `node_remove`, set `was_replace = 1` when actually
removing an entry (the branch that finds a matching `HamtEntry` or removes from
a `HamtCollision`). Read the global after `node_remove` returns.

Option A is preferred because it also reduces unnecessary allocations on miss
(every level of a missed remove currently allocates a new node). Option B works
but leaves the spurious allocation problem in place.

Implementation notes:

- Add a mutable global `was_replace` (i32, initially 0) to the dict module.
- Update `node_set_fn` to set the global to 1 in the "replace existing entry"
  code path (where it finds an existing `HamtEntry` with a matching key).
- **Also update `collision_set`** to set the global to 1 when it replaces an
  existing key inside a collision bucket. Without this, collision-bucket updates
  would be treated as new inserts, duplicating order entries and incrementing
  size incorrectly.
- Update `set_fn` to clear the global before `node_set` and read it after.
- For `remove`: apply Option A — update `node_remove_fn` to check
  `ref.eq(old_child, new_child)` at each recursive call site, returning the
  current node unchanged on no-op. Then use `ref.eq(old_root, new_root)` in
  `remove_fn`.
- Remove the `node_get` calls from both `set_fn` and `remove_fn`.

## Phase 4 — True In-Place Mutation for `set_in_place` and `remove_in_place`

`set_in_place_fn` (line 971) and `remove_in_place_fn` (line 977) currently
delegate to the persistent `set`/`remove` paths, performing full HAMT path-copy
even when the optimizer has proven the dict is uniquely owned.

Unlike PVec's `set_in_place` (which mutates a single leaf array element), HAMT
in-place mutation requires:

- Mutating the `entries` array of each `HamtNode` along the path, rather than
  creating new `HamtNode` structs with copied arrays.
- The `HamtNode.entries` field (in `types.tw`) is currently declared with
  `mutable: false`. This must change to `mutable: true` to allow `struct.set`
  on the entries field.
- The `PDict` fields (`size`, `root`, `order`) must also be mutable for true
  in-place dict updates.

Target behavior for `set_in_place`:

- Traverse the trie to find the target slot.
- If replacing an existing entry: mutate the `entries` array in-place via
  `array.set` at each level. No new `HamtNode` allocations.
- If inserting a new entry: must still allocate new arrays (they grow by one
  slot), but can mutate the parent nodes' `entries` fields to point to the new
  arrays instead of allocating new parent nodes.
- Update `PDict.size` and `PDict.order` in-place via `struct.set`.

Target behavior for `remove_in_place`:

- Similar: mutate entries arrays in-place, shrink by removing the slot.
- Update size and order in-place.

This phase is deliberately later because it requires structural changes to type
declarations and must preserve alias safety — in-place mutation is only safe
when the optimizer guarantees unique ownership.

Implementation notes:

- In `types.tw`, change `HamtNode.bitmap` and `HamtNode.entries` to
  `mutable: true`. Both are needed: `entries` for in-place slot replacement, and
  `bitmap` because insert/remove can add or remove slots, changing which bits
  are set. Without mutable `bitmap`, the in-place path would still need to
  allocate a replacement `HamtNode` whenever the bitmap changes, defeating much
  of the purpose.
- In `types.tw`, change `PDict.size`, `PDict.root`, and `PDict.order` to
  `mutable: true`.
- Add `node_set_in_place` and `node_remove_in_place` internal helpers that
  traverse and mutate rather than copy.
- Keep the persistent `set`/`remove` paths unchanged for ordinary user-visible
  updates.
- This phase can reuse the `was_replace` global from Phase 3.

## Phase 5 — Avoid Redundant Empty PVec Allocation in `make`

`make_fn` (line 851) constructs a fresh empty PVec inline:

```text
I32Const(0), I32Const(0), RefNull(VecInternal), ArrayNewFixed(Array, 0),
StructNew(PVec)
```

This allocates a new zero-length array and PVec struct on every `dict_new()`
call.

The `rt.arr` module has an `empty_pvec` global, but the current IR import model
only represents function imports (`ImportDef`), not global imports. A raw
`GlobalGet("rt_arr__empty_pvec")` inside `rt.dict` would not be linked
correctly without additional linker support.

Options (choose one during implementation):

**Option A — export a `make_empty` function from `rt.arr`.** Add a trivial
exported function `fn make_empty() -> PVec { GlobalGet(empty_pvec) }` to
`rt.arr`. Import it from `rt.dict` as a normal function import. `make_fn` calls
`Call("arr_make_empty")` instead of inline PVec construction. This is the
simplest approach and fits the existing import model.

**Option B — add global import/export support to the linker.** Extend
`ImportDef` / `ExportDef` in the IR to support `global` imports/exports, then
import `rt_arr__empty_pvec` directly. This is the clean solution but requires
linker changes beyond the scope of dict optimization.

**Option C — add an `empty_pvec` global to `rt.dict` itself.** Duplicate the
empty PVec as a module-level global in `rt.dict`, avoiding cross-module
concerns entirely. Minor duplication, but since the empty PVec is immutable and
tiny, this is harmless.

Option A is recommended unless global import support is already planned.

Impact is low since `dict_new()` is typically called at initialization time, not
in hot loops. This saves 2 allocations (Array + PVec struct) per call.

## Testing Strategy

Boot-level behavior tests should cover:

- `popcount` uses `I32Popcnt` instruction (no loop)
- `order_remove_key` calls `to_array` and `from_array` instead of per-element
  `arr_get`/`arr_push`
- `set` and `remove` no longer call `node_get`
- `set` uses the `was_replace` global to detect existing keys
- `collision_set` also sets the `was_replace` global on replacement
- `node_remove` returns original node identity on miss (`ref.eq` propagation)
- `set_in_place` and `remove_in_place` use `struct.set` / `array.set` (Phase 4)
- `make` avoids inline PVec construction (Phase 5)
- Existing dict behavior tests continue to pass (insertion order, get/set/has
  correctness, collision handling)

## Validation

Recommended validation sequence after each phase:

```bash
target/twk run boot/tests/main.tw
cargo test --release
make stage2
make quick-bundle-cli
```

For performance validation, compare same-session build timings before and after
changes. Dict operations are exercised heavily during name resolution, type
checking, and codegen (symbol tables, type environments, module registries).

Useful inspection commands:

```bash
target/twk build boot/main.tw -o /tmp/boot.wat
rg "popcount|order_remove_key|node_get|was_replace" /tmp/boot.wat
```

## Risks and Mitigations

### Mutable struct fields for in-place paths (Phase 4)

Risk: making `HamtNode.entries` and `PDict` fields mutable could allow
accidental mutation in persistent code paths.

Mitigation: only the `*_in_place` helpers use `struct.set` / `array.set`. The
persistent `set`/`remove` paths continue to allocate new nodes. The optimizer
guarantees uniqueness before rewriting to in-place calls.

### `was_replace` global state (Phase 3)

Risk: the mutable global introduces implicit state that could be misread if
`node_set` is called from an unexpected context.

Mitigation: the global is cleared immediately before `node_set` and read
immediately after. No other code path reads or writes it. Wasm execution is
single-threaded.

### `node_remove` no-op propagation (Phase 3)

Risk: adding `ref.eq` child checks at each recursion site in `node_remove`
changes an existing function that is correctness-critical. A subtle mistake
could cause `remove` to silently skip actual removals.

Mitigation: the change is mechanical — at each recursive call, compare old and
new child, return current node if unchanged. This is a standard persistent data
structure optimization. Validate with existing remove/dict behavior tests and
add a targeted test for nested-miss identity preservation.

### `was_replace` must cover collision paths (Phase 3)

Risk: if `collision_set` is not updated to set the `was_replace` global,
collision-bucket key replacements are treated as new inserts — duplicating
order entries and incrementing size.

Mitigation: audit all `node_set`/`collision_set` code paths that can replace an
existing key. Add a test that updates a key known to be in a collision bucket
and verifies size does not grow.

### Import changes (Phase 2)

Risk: adding new imports from `rt.arr` to the dict module could affect module
linking order.

Mitigation: `rt.arr` is already imported by `rt.dict` for `push`, `len`, and
`get`. Adding `to_array` and `from_array` to the existing import set is
straightforward.

## Open Questions

- For Phase 4, should `HamtCollision` entries also be mutable, or is collision
  handling rare enough to keep persistent?
- Which compiler workloads exercise dict operations most heavily for
  benchmarking? Name resolution and type environment construction are likely
  candidates.
- For Phase 5, should we invest in global import/linker support (Option B) as
  part of this plan, or defer it and use the simpler function-export approach
  (Option A)?
