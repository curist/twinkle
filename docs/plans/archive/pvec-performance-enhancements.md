# PVec Runtime Performance Enhancements

## Goal

Improve the boot compiler's current persistent vector (`PVec`) runtime without
changing Twinkle `Vector<T>` semantics, the public Vector API, or the broader
long-term typed-container plan.

This is an incremental performance plan for the existing erased PVec
implementation in:

- `boot/compiler/codegen/runtime/arr.tw`
- boot-side codegen/lowering paths that call the `rt.arr` helpers

The main objective is to reduce avoidable allocation, helper-call overhead, and
per-element tree traversal in compiler-heavy vector construction and host-boundary
conversion paths.

## Relationship to Other Plans

This plan is narrower than [`persistent-vector.md`](persistent-vector.md):

- `persistent-vector.md` tracks the long-term representation direction,
  including typed/specialized vector families.
- This plan assumes the current erased PVec representation remains in place and
  focuses on making that representation cheaper for the boot compiler.

This plan should not block or redesign
[`backend-anyref-elimination.md`](backend-anyref-elimination.md). Any changes here
should be compatible with later typed-container specialization, but they do not
attempt to implement it.

## Current State

The current boot runtime uses a persistent bit-partitioned trie with a tail:

- `PVec { len, shift, root, tail }`
- branching factor 32
- `get` / `set` traverse the trie when an index is outside the tail
- `push` appends to the tail until full, then promotes the full tail into the
  trie
- builders use a transient 32-element tail buffer and freeze into a PVec

Important current costs:

- `builder_push` stores its current tail length in a `BoxedInt`, allocating a new
  box on every push into a non-full builder tail.
- When a builder tail is full, `builder_push` constructs a temporary PVec, calls
  general `push`, then unwraps the result back into builder state.
- `from_array` only zero-copy wraps arrays with length `<= 32`; larger arrays are
  rebuilt by pushing every element.
- `to_array` copies out by calling `get` once per element, causing repeated trie
  traversal for non-tail elements.
- `builder_extend`, `concat`, and `slice` are correctness-first loops over `get`
  plus `builder_push`.
- `vector__set_in_place` currently lowers to the persistent `rt_arr__set` path,
  so uniqueness rewrites do not yet get a true in-place vector update.

## Non-Goals

- Do not change the surface `Vector<T>` API.
- Do not change immutable/persistent semantics visible to users.
- Do not introduce mutable-only vectors.
- Do not implement per-concrete typed vector families as part of this plan.
- Do not rewrite `Dict` or unrelated runtime containers.
- Do not require RRB-tree concat/slice for this pass.

## Target State

The existing erased PVec implementation should be faster in common boot compiler
paths:

- builder push should avoid per-element `BoxedInt` allocation
- builder full-tail promotion should avoid unnecessary temporary PVec round trips
- host-boundary conversion should use leaf/block copies where possible
- concat/slice/extend should avoid repeated full trie traversal where practical
- generated WAT should remain valid and snapshots should continue to describe the
  actual runtime implementation

The first implementation pass should prefer small, measurable improvements over a
large representation rewrite.

## Phase 1 — Builder Tail Length Without Boxing

Change the builder layout from:

```text
[0] = pvec_so_far : PVec
[1] = tail_len    : BoxedInt
[2] = tail_buf    : Array
```

to:

```text
[0] = pvec_so_far : PVec
[1] = tail_len    : i31ref
[2] = tail_buf    : Array
```

Rationale:

- `tail_len` is always in `0..=32`.
- `i31ref` is allocation-free and already used elsewhere for small scalar values.
- This removes one `BoxedInt` allocation from every builder push that does not
  flush the tail.

Implementation notes:

- Update `builder_new`, `builder_from`, `builder_push`, and `builder_freeze` in
  `boot/compiler/codegen/runtime/arr.tw`.
- Replace `StructNew(BoxedInt)` / `StructGet(BoxedInt, 0)` with `RefI31` /
  `I31GetU` or `I31GetS` consistently.
- Keep the builder as an `Array` to preserve the current ABI contract used by
  codegen shims.
- Add or update boot runtime tests that inspect builder behavior through public
  vector construction, not by depending on internal layout unless a runtime IR
  test is already appropriate.

## Phase 2 — Cheaper Builder Full-Tail Promotion

Refactor the full-tail branch of `builder_push`.

Current behavior conceptually does:

```text
temp = PVec { prefix.len + 32, prefix.shift, prefix.root, full_tail }
result = push(temp, elem)
builder.prefix = PVec { result.len - 1, result.shift, result.root, empty_tail }
builder.tail_buf = [elem, null, ...]
builder.tail_len = 1
```

This is correct but routes through the general persistent `push` path and creates
intermediate PVec values.

Target behavior:

- Add an internal helper that promotes a full builder tail into the prefix trie
  without also appending the next element.
- After promotion, keep the incoming element in a fresh builder tail buffer at
  index `0`.
- Preserve alias safety: promoted full tails must not be mutated later by the
  builder.

Possible helper shape:

```text
builder_promote_full_tail(prefix: PVec, full_tail: Array) -> PVec
```

The returned PVec should represent the old prefix plus the full tail, with an
empty tail, so the builder can continue accumulating into a fresh transient tail
buffer.

## Phase 3 — Bulk Array/PVec Boundary Conversion

Improve conversion helpers used at host and runtime boundaries:

- `from_array(arr: Array) -> PVec`
- `to_array(vec: PVec?) -> Array`

### `from_array`

Current behavior for arrays longer than a tail repeatedly calls `push`.

Target behavior:

- Preserve zero-copy wrapping for `len <= 32`.
- For longer arrays, build full 32-element leaves with `array.copy` instead of
  appending each element individually.
- Construct internal trie nodes from leaves in blocks where practical.
- Keep correctness-first fallback code for edge cases if needed.

### `to_array`

Current behavior calls `get` per element.

Target behavior:

- Allocate the final flat `Array` once.
- Copy each complete leaf/tail segment into the result with `array.copy`.
- Traverse leaves sequentially rather than restarting a root-to-leaf traversal for
  every index.

This phase is likely valuable for boot compiler operations that cross host
boundaries, including file IO, emitted Wasm bytes, process/stdio APIs, and any
host calls that require flat arrays.

## Phase 4 — Leaf-Oriented Extend, Concat, and Slice

Once leaf traversal/copy helpers exist, reuse them in:

- `builder_extend`
- `concat`
- `slice`

Current behavior uses `get` plus `builder_push` per element.

Target behavior:

- Append complete leaves/tail chunks to builders where possible.
- Avoid repeated root-to-leaf traversal for consecutive indexes.
- Keep existing fast paths for empty left/right vectors in `concat`.
- Do not attempt RRB concat/slice unless profiling shows this remains a bottleneck
  after bulk leaf copying.

Possible helper shapes:

```text
copy_vec_range_to_builder(builder, vec, start, end) -> void
copy_vec_range_to_array(dst, dst_off, vec, start, end) -> void
```

These helpers should be internal to `rt.arr` and should not change the user-facing
Vector API.

## Phase 5 — Optional True In-Place Set Fast Path

`vector__set_in_place` is currently emitted as a call to persistent `rt_arr__set`.
This preserves correctness but gives no runtime benefit after uniqueness rewrites.

Optional target:

- Add a real in-place update helper for vectors proven unique by the optimizer.
- Mutate the affected tail/leaf path directly where ownership is guaranteed.
- Keep persistent `set` unchanged for ordinary user-visible updates.

This phase is deliberately later because it depends on optimizer ownership
invariants and is easier to get wrong than builder-local transient mutation.

## Testing Strategy

Boot-level behavior tests should cover:

- building vectors through literals, `collect`, and repeated `append`
- builder-seeded construction paths, including append after `builder_from`
- concat and slice equivalence against existing behavior
- conversion round trips at host-like boundaries where available
- alias-safety regressions: building from an existing vector must not mutate or
  corrupt the original vector
- vector updates preserving previous versions

Runtime IR/WAT-oriented tests can additionally verify:

- builder tail length no longer allocates `BoxedInt` in the hot push path
- new helper functions are exported/imported only as intended
- `vector__set_in_place` behavior remains explicitly documented if it is still a
  persistent fallback

## Validation

Recommended validation sequence after each phase:

```bash
target/twk run boot/tests/main.tw
cargo test --release
make stage2
make quick-bundle-cli
```

For performance validation, compare same-session build timings before and after
changes. Prefer end-to-end compiler workloads over microbenchmarks, but add small
focused benchmarks if a regression is hard to localize.

Useful inspection commands:

```bash
target/twk build boot/main.tw -o /tmp/boot.wat
rg "BoxedInt|rt_arr__builder_push|rt_arr__to_array|rt_arr__from_array" /tmp/boot.wat
```

## Risks and Mitigations

### Builder aliasing bugs

Risk: transient builder buffers are accidentally shared with persistent vectors
and then mutated.

Mitigation: only mutate the current transient tail buffer. Once a full tail is
promoted into a PVec, allocate a fresh builder tail buffer before accepting more
pushes.

### Complex bulk conversion logic

Risk: leaf/block conversion code becomes harder to reason about than the current
simple per-element loops.

Mitigation: stage bulk helpers behind tests, keep fallback loops during early
implementation, and validate round trips over boundary sizes around tail and tree
thresholds.

### Optimizer contract drift

Risk: changes to builder internals break codegen ABI shims or uniqueness rewrite
assumptions.

Mitigation: keep the external builder helper signatures unchanged unless there is
an explicit coordinated codegen update.

### Performance tradeoff uncertainty

Risk: larger helper bodies increase code size or compile time more than they
improve runtime speed.

Mitigation: implement phases independently and measure. The i31 tail-length
change is small; bulk conversion can be deferred or narrowed if measurements do
not justify it.

## Open Questions

- Should `from_array` build trie nodes directly, or should it use a specialized
  bulk builder path first?
- Do we need a dedicated runtime test hook for PVec leaf traversal, or are public
  Vector/host-boundary tests sufficient?
- Is true `vector__set_in_place` worth implementing before typed vector families,
  or should it wait for the broader backend-anyref work?
- Which compiler workloads should become the standard perf comparison for PVec
  changes?
