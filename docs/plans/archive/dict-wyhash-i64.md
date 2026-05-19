# Hashing Migration: wyhash v3 + 64-bit HAMT Hashes

## Goal

Replace FNV-derived hashing with deterministic wyhash v3, and migrate Dict's
HAMT runtime to carry 64-bit hashes instead of folding back to `i32`.

This is not a replacement for collision handling. The HAMT must continue to
handle full-hash collisions correctly. The goal is to make full-hash collisions
far less likely while preserving deterministic compiler/runtime behavior.

## Motivation

A real compiler workload exposed a full FNV-1a 32-bit hash collision between two
generated function names:

- `user__$f2396_mark_published_version`
- `user__$str_333_get`

The immediate lookup bug was caused by `HamtNode` and `HamtCollision` having
structurally identical Wasm GC layouts, which made runtime type tests dispatch a
collision as if it were a sub-node. That bug is fixed independently, but the
incident showed that 32-bit FNV-1a is too small and too weak for large generated
symbol tables.

Moving to wyhash v3 gives better distribution and a much larger hash space while
keeping hashing fast and non-cryptographic.

A repository grep also shows FNV is used outside runtime Dict hashing. This plan
therefore covers all active FNV users, not only the HAMT.

## Non-Goals

- Do not expose hashing in the Twinkle language surface.
- Do not change dict equality semantics; key comparison remains structural via
  `rt.core.eq`.
- Do not remove collision nodes or collision scans.
- Do not randomize hash seeds. Builds and compiler output must stay
  deterministic.
- Do not attempt a typed-container rewrite as part of this change.

## Current State

Runtime dict hashing is implemented in both the boot compiler runtime and the
Rust stage0 mirror:

- `boot/compiler/codegen/runtime/dict.tw`
- `src/runtime/dict.rs`

Current hash functions:

- `hash_i64(v: i64) -> i32`
- `hash_string(s: String?) -> i32`
- `hash_key(key: anyref) -> i32`

Current HAMT hash storage and flow:

- `HamtEntry.hash` is `i32`
- `HamtCollision.hash` is `i32`
- `node_get`, `node_set`, and `node_remove` accept hash parameters as `i32`
- trie fragments are computed with `hash >> (depth * 5)` and `& 31`
- maximum depth is effectively based on consuming 32 bits

Other active FNV users found by grep:

- `src/query/keys.rs` uses FNV-1a 64-bit for compiler query/cache keys.
- `boot/lib/query/keys.tw` mirrors the query/cache key hashing in Twinkle.
- `boot/compiler/query/fingerprint.tw` has a local FNV-style `mix_word` helper
  because `keys.mix_word` is not exported.
- `boot/tests/suites/query_keys_suite.tw` asserts known FNV outputs.
- checked-in WAT/snapshot files contain generated output from the current Dict
  runtime hash functions and will need regeneration when snapshots are updated.

Archived plan documents mention FNV historically; those references do not need
migration unless they are intentionally refreshed.

## Target State

Runtime hash functions become:

- `hash_i64(v: i64) -> i64`
- `hash_string(s: String?) -> i64`
- `hash_key(key: anyref) -> i64`

HAMT structs carry 64-bit hashes:

- `HamtEntry.hash: i64`
- `HamtCollision.hash: i64`

HAMT operations accept and compare 64-bit hashes:

- `node_get(node, hash: i64, depth: i32, key) -> anyref`
- `node_set(node, hash: i64, depth: i32, key, val) -> HamtNode`
- `node_remove(node, hash: i64, depth: i32, key) -> HamtNode?`
- `collision_set(c, hash: i64, key, val) -> HamtCollision`

Trie indexing remains 5 bits per level:

```text
fragment = i32.wrap_i64(hash >>_u i64(depth * 5)) & 31
bit      = 1 << fragment
```

Depth should allow all 64 bits to contribute. With 5-bit fragments, the usable
fragment depths are `0..=12`; depth 12 shifts by 60 and uses the remaining high
bits. Depth 13 must never compute another `depth * 5` shift for trie indexing.

Recommended terminology and guard:

```text
MAX_HASH_FRAGMENT_DEPTH = 12
if depth > MAX_HASH_FRAGMENT_DEPTH { use/return collision path }
```

When splitting an entry at depth 12, do not recurse to depth 13. If two distinct
keys cannot be separated by the final fragment, create a collision node. In other
words, depths `0..=12` may compute fragments, and attempts to split beyond depth
12 transition to the full-hash collision path.

## wyhash Version and Compatibility

Use wyhash v3 specifically. Do not implement an approximate or
"wyhash-inspired" variant under the wyhash name.

Requirements:

- deterministic across hosts
- fixed seed, with no per-process randomization
- mirrored exactly in boot and stage0
- pinned to an authoritative wyhash v3 source or commit before implementation
- documented with the exact wyhash v3 constants, seed choice, algorithm shape,
  and test vectors
- byte-for-byte agreement between Rust and Twinkle implementations for the
  inputs covered by tests

Recommended seed:

```text
WY_SEED = 0
```

wyhash v3 uses widened multiplication internally. Wasm only exposes the low half
of `i64.mul`, so the implementation must add explicit 64x64 -> 128 support, or
an equivalent helper that computes the high and low halves required by wyhash v3.
Spike this helper first: decompose operands into 32-bit halves, combine the
partial products with carries, and add parity tests before wiring it into the
runtime hash path. If the current Wasm IR lacks enough unsigned arithmetic
helpers, extend the IR rather than silently weakening the algorithm.

Treat hash values as unsigned 64-bit bit patterns everywhere. Twinkle `Int` is
signed `i64`, so equality is unaffected, but comments and helper names should use
unsigned terminology where relevant. Use unsigned shifts for fragment extraction
and define wrapping arithmetic behavior explicitly in both implementations.

## Implementation Phases

### Phase 1 — Widen runtime types and signatures

Update shared runtime type definitions:

- `boot/compiler/codegen/runtime/types.tw`
- `src/runtime/types.rs`

Changes:

- `HamtEntry.hash: i32 -> i64`
- `HamtCollision.hash: i32 -> i64`
- keep the distinct `HamtCollision.tag` field so runtime type dispatch remains
  robust under Wasm GC structural type checks

Update runtime function signatures and locals in:

- `boot/compiler/codegen/runtime/dict.tw`
- `src/runtime/dict.rs`

Changes:

- hash helpers return `i64`
- `collision_set` accepts an `i64` hash
- `node_get`, `node_set`, and `node_remove` accept `i64` hash parameters
- hash comparisons use `i64.eq`
- fragment extraction converts from the shifted `i64` to `i32`

Phase 1 and Phase 2 are tightly coupled, but they can be staged safely. A useful
intermediate step is to widen HAMT storage and function signatures first while
keeping the old FNV-derived hash implementations, returning their `i32` results
as `i64` via zero-extension. This validates the runtime type/signature migration
independently of wyhash. Phase 2 can then replace the widened FNV compatibility
hashes with exact wyhash v3 once 64x64 -> 128 multiplication support is ready.
Keep each intermediate commit buildable and covered by the dict behavior tests.

### Phase 2 — Implement wyhash v3 helpers

Add exact wyhash v3 helpers in both implementations:

- constants for the selected wyhash v3 version
- little-endian byte packing helpers
- 64x64 -> 128 multiplication support as needed by v3's mix/finalize steps
- `wyhash_v3_bytes(bytes, seed) -> i64` in Rust and equivalent Twinkle/runtime
  helpers
- integer hashing helpers that feed integer bytes/tags through the same wyhash
  v3 path rather than returning lightly mixed integer values

For runtime strings, read bytes from `rt_types__String` and feed the wyhash v3
short/long input paths exactly. Prefer correctness and parity over a simplified
streaming loop.

### Phase 3 — Migrate all active FNV users

Runtime Dict migration:

- replace FNV-1a string hashing and the current integer fold/multiply hash in
  `boot/compiler/codegen/runtime/dict.tw`
- mirror the same runtime codegen changes in `src/runtime/dict.rs`

Compiler query/cache migration:

- replace FNV helpers in `src/query/keys.rs`; this path already uses 64-bit
  FNV-1a, but the key values intentionally change when switching algorithms
- replace FNV helpers in `boot/lib/query/keys.tw`
- remove or replace the local FNV `mix_word` in
  `boot/compiler/query/fingerprint.tw`; prefer exporting a shared wyhash-based
  word-mixing helper from `boot/lib/query/keys.tw` so query fingerprints do not
  duplicate hashing internals
- bump `cache_schema_version()` / `CACHE_SCHEMA_VERSION` because cache key values
  will intentionally change
- update `boot/tests/suites/query_keys_suite.tw` expected values against the new
  wyhash v3 reference outputs

Generated snapshots:

- update checked-in WAT/snapshot files that include the old runtime hash function
  bodies or function signatures as part of the normal snapshot update workflow,
  especially `tests/snapshots/build/**` and
  `tests/snapshots/runtime_dump_test__runtime_dump_wat.snap`

### Phase 4 — Adjust HAMT depth/collision behavior

Update all places that assume 32 hash bits:

- `node_get` null-return depth guard
- `node_set` transition from sub-node creation to collision creation
- `node_remove` recursion/collision handling if it has a depth guard

Use the 64-bit maximum depth consistently. The final full-hash collision path
must still compare keys with `rt.core.eq` inside `collision_get` /
`collision_set`.

### Phase 5 — Tests and regression coverage

Add boot-level coverage for:

- the known FNV collision pair both retrieve correctly after migration, as a
  regression test for the original workload
- true full-hash collision behavior using a private runtime test hook, fabricated
  hashes, or another deterministic way to force two distinct keys onto the same
  64-bit hash
- updating one forced-colliding key preserves the other
- removing one forced-colliding key preserves the other
- insertion order remains unchanged
- generated-symbol-like strings survive larger dict construction

Include deterministic wyhash v3 test vectors so stage0 and boot stay aligned.
Cover both byte/string hashing and word/composite query-key hashing. The tests
should make it obvious if one implementation accidentally uses a simplified
mixer, signed shift, wrong endian order, or stale FNV path.

### Phase 6 — Bootstrap and verify

Recommended validation sequence:

```bash
cargo test --release
target/twk run boot/tests/main.tw
make stage2
make quick-bundle-cli
```

Also inspect generated WAT for a small dict program to confirm:

- `HamtEntry` and `HamtCollision` hash fields are emitted as `i64`
- `node_get` / `node_set` signatures carry `i64` hashes
- `HamtCollision` remains structurally distinct from `HamtNode`

## Risks and Mitigations

### Boot/stage0 divergence

Risk: duplicated runtime implementations drift.

Mitigation: implement stage0 and boot changes together, and add deterministic
hash-output or behavior tests that exercise both paths during bootstrap.

### Incomplete 64-bit depth migration

Risk: some code still assumes seven 5-bit levels from the old 32-bit hash.

Mitigation: search for depth constants and comments in both runtime copies, and
prefer named helper comments around the new maximum depth logic.

### wyhash v3 parity

Risk: a Wasm-friendly implementation accidentally diverges from wyhash v3.

Mitigation: implement the required high-half multiply exactly, add wyhash v3 test
vectors, and verify stage0/boot parity. Do not call a simplified mixer wyhash.

### Collision logic regressions

Risk: fewer collisions in normal use can hide collision-path bugs.

Mitigation: keep explicit collision regression tests using either fixed known
hashes through a runtime test hook, or carefully chosen strings if exact hash
outputs are stabilized for tests.

## Open Questions

- Do we want a private runtime-only hash test hook for deterministic regression
  tests, or should tests stay purely behavioral through `Dict`?
- Where should shared query-key word mixing live so `boot/compiler/query/fingerprint.tw`
  does not duplicate hash internals?
- Should the 64-bit hash be treated as signed or unsigned in comments and helper
  names? Wasm comparisons for equality are unaffected, but shifts for fragment
  extraction must be unsigned.
