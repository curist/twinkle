# Persistent `Vector<Int>` POC Plan

## Goal

Build a narrow proof-of-concept persistent implementation for `Vector<Int>`
only, replacing the current flat copy-on-write `rt_types__Vector_i64` array
with a bit-partitioned trie plus tail.

This plan is intentionally narrower than
[`persistent-vector.md`](persistent-vector.md):

- only `Vector<Int>` is in scope
- only the existing `bootlib.vector_i64` boundary is in scope
- no generic `Vector<T>` family generation is attempted
- initial integration target is stage0 only
- boot-compiler runtime parity is explicitly deferred

## Context

`Vector<Int>` is currently backed by a flat Wasm GC array. Every `set`, `push`,
`concat`, and `slice` copies the full array, so common persistent operations are
O(n).

The bootlib boundary already exists:

- user code
- `bootlib.vector_i64`
- `__raw_*` substrate imports
- `rt.arr`

That means the runtime representation can change without touching
`boot/lib/vector_i64.tw`, as long as the raw substrate surface stays stable.

For this POC, "integration" means:

- programs compiled by stage0 use the persistent `Vector<Int>` runtime
- the raw `__raw_vector_i64_*` ABI remains stable at the bootlib boundary
- the boot compiler is not required to use the new runtime representation yet

The boot compiler has its own mirrored runtime implementation in
`boot/compiler/codegen/runtime/arr.tw`. Porting the same representation there is
a follow-up task, not part of this POC.

## Runtime Invariants

- `0 <= tail.data.len <= 32`
- `len = trie_element_count + tail.data.len`
- `root = null` iff `len <= 32`
- `shift = 0` iff `root = null`
- when `shift > 0`, `shift = 5 * tree_depth`
- internal nodes always own fixed-width 32-slot child arrays
- null children in `NodeChildren_i64` occur only on the rightmost incomplete path
- traversal for any valid index never reaches a null child
- trie leaves always contain exactly 32 items
- `tail` is never null; the empty vector uses a shared empty leaf
- `tailoff(len) = if len <= 32 { 0 } else { ((len - 1) >> 5) << 5 }`
- persistent updates copy only nodes along the modified path

## Type Layout

Use Wasm GC nominal subtyping for the node family.

```wat
;; Abstract node base
(type $TrieNode_i64 (sub (struct)))

;; Raw leaf storage
(type $LeafData_i64 (array (mut i64)))

;; Leaf wrapper
(type $Leaf_i64 (sub $TrieNode_i64 (struct
  (field $data (ref $LeafData_i64)))))

;; Fixed 32-slot child array
(type $NodeChildren_i64 (array (mut (ref null $TrieNode_i64))))

;; Internal node wrapper
(type $Internal_i64 (sub $TrieNode_i64 (struct
  (field $children (ref $NodeChildren_i64)))))

;; Persistent vector
(type $Vector_i64 (struct
  (field $len i32)
  (field $shift i32)
  (field $root (ref null $Internal_i64))
  (field $tail (ref $Leaf_i64))))
```

Notes:

- `root` is `Internal_i64?`, not `TrieNode_i64?`
- internal child arrays are always length 32
- `Vector_i64` is no longer an array type
- `LeafData_i64` is the only raw `i64` array used by this representation

## Core Operations

### `get_i64(vec, idx) -> i64`

- if `idx >= tailoff(vec.len)`, read from tail
- otherwise descend from `root` using 5-bit slices of `idx`
- cast internal levels to `Internal_i64`
- cast the final node to `Leaf_i64`

### `set_i64(vec, idx, val) -> Vector_i64`

- if the index is in the tail, copy the tail array and update one slot
- otherwise path-copy from root to target leaf
- reuse all unaffected siblings and subtrees

### `push_i64(vec, val) -> Vector_i64`

Case 1: tail has room

- copy tail data
- append `val`

Case 2: tail is full

- promote the old full tail into the trie
- allocate a new one-element tail

Sub-cases:

- no root yet: create a fresh root with slot 0 = old tail, `shift = 5`
- current depth has room: path-copy down the right spine and insert the old tail
- current depth is full: create a new root, put old root in slot 0 and a fresh path in slot 1, then increase `shift` by 5

### `make_i64(len, fill) -> Vector_i64`

- `len == 0`: shared empty vector
- `len <= 32`: tail-only vector
- `len > 32`: push loop from empty

### `concat_i64(a, b) -> Vector_i64`

v1 implementation:

- iterate `b`
- push each element onto `a`

### `slice_i64(vec, start, end) -> Vector_i64`

v1 implementation:

- start from empty
- push elements in `[start, end)`

## `from_leaf_i64` Rule

Add a helper in `rt.arr`:

- `from_leaf_i64(leaf_data: LeafData_i64) -> Vector_i64`

This helper is strict:

- precondition: `leaf_data.len <= 32`
- wraps one raw leaf array into a tail-only persistent vector
- does not normalize arbitrary large flat arrays into trie form

That keeps the tail invariant explicit and avoids a misleading “general
flat-array conversion” helper.

## Literal Lowering Rule

This is the most important boundary rule for codegen.

Small int literals:

- if literal length `<= 32`, codegen may emit:
  - `ArrayNewFixed(T_LEAF_DATA_I64, n)`
  - `call rt_arr__from_leaf_i64`

Large int literals:

- if literal length `> 32`, codegen must not call `from_leaf_i64`
- instead lower through a construction path that preserves invariants from the start

Acceptable v1 strategies for large literals:

- repeated `push_i64`
- builder-based construction if we add a dedicated fast path later

## Builder ABI

Keep the existing 3-slot `rt_types__Array` builder ABI for compatibility, but
reinterpret it for `Vector<Int>`.

- slot 0: persistent `Vector_i64`
- slot 1: unused
- slot 2: unused

Operations:

- `builder_new`: `[empty_pvec, unused, unused]`
- `builder_from_i64(vec)`: `[vec, unused, unused]`
- `builder_push_i64(builder, val)`: load slot 0, call `push_i64`, store back into slot 0
- `builder_freeze_i64(builder)`: return slot 0 directly

This preserves the lowering contract without keeping the old flat-buffer
capacity model alive.

## Files In Scope

- `src/runtime/types.rs`
- `src/runtime/arr.rs`
- `src/codegen/emit.rs`
- `src/codegen/prelude.rs`

Expected unchanged file:

- `boot/lib/vector_i64.tw`

Explicitly out of scope for this POC:

- `boot/compiler/codegen/runtime/arr.tw`
- boot-compiler runtime/codegen parity for persistent `Vector<Int>`
- generic `Vector<T>` specialization work

## Phases

### Phase 1: Close the Literal Boundary

Goal:

- eliminate direct codegen construction of `Vector_i64` values

Changes:

- add `from_leaf_i64` to `rt.arr`
- add the corresponding import/prelude entry
- update int vector literal lowering:
  - `len <= 32` uses `LeafData_i64 + from_leaf_i64`
  - `len > 32` uses a separate construction path

### Phase 2: Add Persistent Runtime Types

Changes:

- add `TrieNode_i64`, `LeafData_i64`, `Leaf_i64`, `NodeChildren_i64`, `Internal_i64`
- redefine `Vector_i64` as a struct
- add reference helpers
- add shared empty leaf / empty vector helpers as needed

### Phase 3: Rewrite `rt.arr` i64 Helpers

Changes:

- rewrite `make_i64`, `get_i64`, `set_i64`, `len_i64`, `push_i64`, `concat_i64`, `slice_i64`
- simplify `builder_from_i64`, `builder_push_i64`, and `builder_freeze_i64`

Important:

- Phase 2 and Phase 3 are logically separate but not independently shippable
- once `Vector_i64` stops being an array, the old `array.get/len/copy/set` implementation is invalid

### Phase 4: Finalize Codegen

Changes:

- update `emit_array_literal` to use `T_LEAF_DATA_I64`
- add import plumbing for `rt_arr__from_leaf_i64`
- update literal tests that currently assert `ArrayNewFixed(T_VECTOR_I64, ...)`

### Phase 5: Validation

Existing tests:

- `cargo test --test run_test -- vector`
- `cargo test --test run_test -- collect`
- `cargo test --test runtime_import_boundary_test`

New tests:

- boundary sizes: 32, 33, 1024, 1025
- `push/get/set` at indices 31, 32, 1023, 1024
- large vector smoke test around 10000 elements
- builder path with more than 32 elements
- persistence test: modify a derived vector and verify the original remains unchanged
- literal lowering test:
  - small literal uses `from_leaf_i64`
  - large literal does not

Stage0-only validation is sufficient for this POC. Boot-compiler parity tests are
not required until the mirrored runtime in `boot/compiler/codegen/runtime/arr.tw`
is updated in a follow-up plan.

## Follow-Up

After the stage0 POC is working, the next integration step is:

1. mirror the `Vector_i64` type layout and trie algorithms into `boot/compiler/codegen/runtime/arr.tw`
2. update any boot-compiler-side codegen/runtime assumptions that still treat `Vector_i64` as a flat array
3. add parity validation so both stage0 and boot-compiled paths use the same persistent representation

## Risks

- tail promotion bugs will show up as subtle off-by-one and null-child failures
- the large-literal lowering path is easy to miss because it is the last direct construction site
- builder compatibility is simple in the new design, but collect/lowering still depends on that ABI shape
- runtime snapshots will change significantly once `Vector_i64` is no longer an array

## Summary

This POC keeps the problem intentionally small:

1. only change `Vector<Int>`
2. integrate only with stage0
3. keep `bootlib.vector_i64` unchanged
4. use fixed-width 32-slot internal child arrays
5. keep `from_leaf_i64` strict and small-literal-only
6. preserve builder ABI shape, but simplify its semantics

If this lands cleanly, it becomes the concrete implementation template for the
broader generic-container plan rather than replacing that broader plan.
