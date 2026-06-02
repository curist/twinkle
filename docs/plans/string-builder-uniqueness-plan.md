# String Builder / Concat Uniqueness Plan

## Goal

Avoid accidental quadratic string construction in Twinkle code by adding a
builder-backed path for repeated `String.concat` patterns.

This plan follows the same static-only constraints as the existing uniqueness
optimizer work:

- no runtime refcounts
- no runtime uniqueness flags
- no user-visible ownership or borrow annotations
- no whole-program alias analysis
- no change to string value semantics

The intended result is that code such as:

```tw
out := ""
for i < n {
  out = out.concat(chunk)
}
out
```

can be compiled as a single transient build region instead of repeatedly
allocating and copying the accumulated prefix.

## Motivation

`String.concat(a, b)` currently allocates a new string and copies both inputs.
That is the right persistent value semantics, but repeated append-style concat
forms can become quadratic in the produced byte length.

A concrete example is `tools/leetcode/problems/p0394_decode_string.tw`:

```tw
fn repeat(s: String, k: Int) String {
  out := ""
  i := 0

  for i < k {
    out = .concat(s)
    i = i + 1
  }

  out
}
```

The decoder also appends single-byte slices and combines prefixes with decoded
chunks:

```tw
cur = .concat(s.slice(i, i + 1))
cur = prefix.first.concat(repeat(cur, k.first))
```

There is already a vector builder family used by `collect` lowering and the
static uniqueness optimizer. There is no analogous string builder API or
optimizer path today.

## Non-Goals

- Do not expose mutable strings.
- Do not make `String.concat` mutate its left operand.
- Do not add ownership annotations to source code.
- Do not optimize every possible string-building shape.
- Do not rewrite regions where accumulator reads would observe different
  intermediate semantics.
- Do not require Unicode scalar or grapheme awareness for the builder itself;
  strings are stored and concatenated as UTF-8 bytes.

## Design Overview

Add an internal string-builder family and teach the optimizer to rewrite safe
consume-reassign `String.concat` regions to it.

Conceptually:

```text
string_builder_from(base: String) -> Builder
string_builder_extend(builder: Builder, chunk: String) -> Void
string_builder_push_byte(builder: Builder, byte: Byte) -> Void   // optional
string_builder_freeze(builder: Builder) -> String
```

The public language continues to expose immutable `String` values. The builder
is compiler/runtime internal, like the vector builder helpers.

> **`from`-only, no `new`.** The vector builder family was simplified in
> `cb53dc8` to always start from the base (`builder_from`) and never `builder_new`
> — see [[project_loop_builder_known_empty_bug]]. `builder_from("")` copies
> nothing, so a dedicated `new` saves nothing and reintroduces the `known_empty`
> false-positive class of bug. The string family mirrors this: only
> `string_builder_from`.

A simple concat loop:

```tw
out := ""
for i < n {
  out = out.concat(chunk)
}
```

lowers to a region equivalent to:

```text
builder := string_builder_from(out)   // out == "" here, so this is empty
loop:
  string_builder_extend(builder, chunk)
out := string_builder_freeze(builder)
```

A non-empty base uses the same `string_builder_from(base)`, provided the same
safety checks used by vector builder regions pass.

## Runtime Builder Shape

Start with a correctness-first builder representation rather than a highly tuned
rope or chunk tree.

`String` is a Wasm GC `(array mut i8)` (`rt_types__String`), and `concat`
allocates a sized array and `ArrayCopy`s both sides. The builder is a dedicated
growable byte buffer that mirrors that representation directly — **not** a
`Vector<Byte>` (which would box every byte as `anyref` and reuse the PVec-trie
builder unnecessarily). New GC type:

```
rt_types__StrBuilder = .{ len: i32 (mut), buf: rt_types__String (mut) }
```

- `from(base)` allocates `buf` with the base's bytes (an empty base copies
  nothing, so this also covers the `out := ""` case) and sets `len`
- `extend(builder, chunk)` ensures capacity (`len + chunk.len`, doubling the
  backing array via `ArrayNew` + `ArrayCopy` when it must grow), then
  `ArrayCopy`s `chunk` in at `len` and bumps `len`
- `freeze(builder)` returns `buf` directly when `len == buf.len`, else a
  fresh exact-size `String` copy

Because all inputs are existing `String` values, `extend` only copies valid
UTF-8 byte sequences, so `freeze` needs no validation step — no `String.from_utf8`
round-trip.

A later optimization can replace the doubling byte buffer with chunked leaves.
That should be a runtime performance follow-up, not a precondition for the
compiler rewrite.

## Prerequisite (RESOLVED): Boot Loop-Builder Rewrites Re-Enabled

The boot loop-builder rewrite was disabled in `c79dd75`; re-enabling it was the
main prerequisite for the loop cases (`repeat`, parser accumulation, formatter
output). **This is now done (`cb53dc8`).**

The disable-comment blamed the boot *backend* for miscompiling builder regions.
That diagnosis was wrong. The real cause was an *optimizer* bug: boot's
`known_empty` analysis produced a **false positive**, so a non-empty accumulator
base was treated as empty and the `builder_new` path silently dropped the base's
pre-loop contents — corrupting self-hosted runs (broken inference, lost
diagnostics). The backend emits builder regions correctly (`collect`
comprehensions already use them).

Fix: always start the rewrite from `builder_from(base)` (never `builder_new`),
which preserves the base's real contents regardless of emptiness, and delete the
now-unused `known_empty` machinery. Self-host fixed point holds and all boot
tests pass. Full write-up: [[project_loop_builder_known_empty_bug]].

The string family must follow the same `from`-only discipline (see Design
Overview) so it can never reintroduce this bug.

## Compiler Integration

### Builtin registration

Add internal-only builtin IDs for the string builder family, similar to the
vector builder helpers. They should not have public canonical names unless a
public builder API is deliberately introduced later.

Required internal functions:

- `string$builder_from`
- `string$builder_extend`
- `string$builder_freeze`

Optional helper:

- `string$builder_push_byte`

`String.concat` remains the public operation and keeps its current semantics.

### Shared builder-family abstraction

`BuilderConfig` (in `boot/compiler/builder_family.tw`) already exists and is
already threaded through the optimizer (`loop_builder.tw`, `semantics.tw`) — the
rewrite paths do **not** hard-code vector IDs. So "generalize the plumbing" is
small. Its current shape:

```tw
type BuilderConfig = .{
  push_id: FuncId,          // the user-facing op that triggers recognition
  builder_new_id: FuncId,
  builder_from_id: FuncId,
  builder_push_id: FuncId,  // the in-builder append it lowers to
  builder_freeze_id: FuncId,
}
```

The real work is a **second** builder family for strings:

- `push_id` = `String.concat`'s method id (the loop matcher
  `loop_push_reassign_elem` already fits `concat(base, chunk)`, and rejects
  self-concat `s = s.concat(s)` for free since it requires `args[1] != base`)
- `builder_push_id` = `string$builder_extend` (concat appends a whole string, so
  the "push" lowers to extend, not a single-element push)
- `builder_from_id` = `string$builder_from`
- `builder_freeze_id` = `string$builder_freeze`
- `builder_new_id` is unused (`from`-only); leave it pointing at `from` or 0

and letting the uniqueness pass try the string family in addition to the vector
one. A single optional `push_byte` slot can be added later if byte-level appends
are wired; the first version routes single-byte slices through `extend`.

### Operation semantics

Extend the uniqueness operation semantics so `String.concat` is known as a
builder-rewritable allocating combinator.

Like `VECTOR_CONCAT`, it should not become a generic one-base COW op with an
in-place variant. It has no true in-place string mutation. Recognition should be
local to builder-region rewrites.

Concretely, the `CallSemantics` row for `String.concat` in
`make_prelude_optimizer_semantics` (`boot/compiler/opt/semantics.tw`) should be:

```tw
calls[b.method_id("String", "concat").id] = CallSemantics.{
  effect: .Allocate,
  fresh_result: true,
  cow_base_arg: .None,        // NOT a one-base COW op; no in-place variant
  in_place_equivalent: .None,
}
```

Note the consequence: with `cow_base_arg: .None`, taint analysis taints all of
`concat`'s args including the base (it is not treated as a reusable COW base).
The loop-region detector keys off `push_id` independently of taint, so confirm
the rewrite still fires under that taint state during implementation — if it
does not, `concat` needs a different registration than this.

## Rewrite Rules

### Straight-line consume-reassign

Rewrite:

```tw
tmp := out.concat(chunk)
out = tmp
```

or the ANF equivalent of:

```tw
out = out.concat(chunk)
```

when:

- `out` is unique, refreshed, builder-safe, or source-fresh under the same rules
  used by vector builder regions (the `known_empty` predicate no longer exists —
  `builder_from` handles empty and non-empty bases uniformly)
- `chunk` does not syntactically alias `out`
- no later use observes the pre-rewrite `out` value
- no intervening operation retains or captures `out`

### Loop consume-reassign

Rewrite append-like loops when the accumulator is only consumed and reassigned
inside the region:

```tw
for condition {
  out = out.concat(chunk)
}
```

The loop rewrite must preserve the existing vector-builder negative rule:
accumulator reads inside the rewritten region are rejected unless the compiler
has explicit builder-aware semantics for that read.

For example, this should not rewrite initially:

```tw
for condition {
  if out.len() > limit {
    return out
  }
  out = out.concat(chunk)
}
```

A future extension can support this by freezing before reads or by adding
builder-aware read operations, but that is not part of the initial rollout.

### Mixed append forms

Support mixed string-building regions where all operations append bytes to the
same logical accumulator:

```tw
out = out.concat(prefix)
out = out.concat(body)
out = out.concat(suffix)
```

This maps naturally to repeated `string_builder_extend` calls.

If `string_builder_push_byte` exists, byte-level appends can be optimized too,
but it is acceptable for the first version to route single-byte string slices
through `extend`.

### Dead-base concat

Mirror the vector concat dead-base path where safe:

```tw
next := out.concat(chunk)
// no later use of out
```

This can start a builder from `out`, extend with `chunk`, and bind `next` to the
frozen result. This is lower priority than consume-reassign loops, but it helps
straight-line helper code.

## Safety Rules

The string rewrite should inherit the vector builder safety posture:

- Reject self-concat such as `s = s.concat(s)`.
- Reject regions where the accumulator is read in the middle of the region.
- Reject regions where the accumulator is passed to an unknown call.
- Reject regions where the accumulator is stored, returned, or captured before
  the builder is frozen.
- Reject branches unless all reachable arms preserve compatible builder state.
- Treat early-return joins the same way as the existing uniqueness optimizer:
  only reachable continuation states participate in the join.

The builder is an implementation detail. Source-visible behavior must remain as
if every `String.concat` allocated a fresh immutable string at that point.

## Unicode and Byte Semantics

The builder operates on UTF-8 bytes, not Unicode scalars or grapheme clusters.
This matches current `String.concat`, `String.slice`, string indexing, and byte
iteration behavior.

Appending existing `String` values preserves UTF-8 validity by construction.
Appending raw bytes should only be exposed internally if the compiler can prove
validity, or should validate at freeze time.

For user code that naturally builds bytes, `Vector<Byte>` plus
`String.from_utf8` remains the explicit checked path.

## Rollout

> **Order note.** The original S4 (straight-line) → S5 (loop) ordering is
> inverted from the actual difficulty. The loop path reuses the existing,
> now-working `rewrite_loop_region` machinery, while straight-line needs a brand
> new region detector (the current builder rewrite is loop-only). Implement the
> loop case (S5) first, then straight-line (S4).

### Phase S0: Re-enable boot loop-builder rewrites — ✅ DONE (`cb53dc8`)

See the "Prerequisite (RESOLVED)" section above. The loop-builder engine the
string loop case depends on is live and self-host-green.

### Phase S1: Characterize current behavior

Add focused fixtures that demonstrate the current repeated-concat shapes and
serve as regression tests once the builder path exists.

Suggested fixtures:

- simple `out = out.concat(chunk)` loop
- empty accumulator returned after the loop
- non-empty initializer followed by appends
- straight-line concat chain
- self-concat negative case
- accumulator-read-in-loop negative case
- p0394-style repeat helper

The initial tests can assert structural lowering once builder IDs exist, plus
runtime equivalence with optimization enabled and disabled.

### Phase S2: Add internal string builder runtime helpers

Implement `string$builder_from / extend / freeze` in the boot runtime codegen
and stage0 runtime/codegen for bootstrap parity. (No `_new` — see Design
Overview.) Follow the established recipe in
`reference_runtime_builtin_wiring.md`: append-at-end FuncId discipline, wire
both compilers, regen core_lib, `make bundle-cli`, then suite + docs.

Keep the initial implementation simple and correctness-first (a `Vector<Byte>`
buffer). Optimize the representation later if profiling shows it matters.

### Phase S3: Add the string builder family + concat semantics

Small, not a redesign — `BuilderConfig` already exists and is already threaded.
Add a `string_builder_config(b)` constructor alongside `vector_builder_config`,
register the `String.concat` `CallSemantics` row (see "Operation semantics"),
and let the uniqueness pass try the string family in addition to the vector one.

### Phase S5: Loop string concat regions (do before S4)

The boot loop-builder engine is already re-enabled (S0). With the string family
from S3, the existing `loop_push_reassign_elem` / `rewrite_loop_region` path
should fire on `out = out.concat(chunk)` loops directly, preserving the existing
accumulator-read negative rule.

This unlocks `repeat`-style helpers and many parser/formatter string assembly
loops without a public string builder API.

### Phase S4: Straight-line string concat regions

Teach the optimizer to rewrite safe straight-line `String.concat` consume-reassign
chains into string builder regions. This needs a **new** straight-line builder-
region detector (the existing builder rewrite is loop-only; straight-line COW
today relies on in-place mutation, which strings can't use), so it is genuinely
more work than S5 despite the lower phase number.

Start with the narrowest shapes:

```tw
out = out.concat(chunk)
out = out.concat(more)
```

and only then add dead-base concat if it falls out naturally.

### Phase S6: Source cleanup opportunities

After the compiler rewrite is in place, revisit code such as
`p0394_decode_string.tw`. Prefer writing clear immutable string code and relying
on the optimizer for obvious builder regions. Only switch user code to explicit
`Vector<Byte>` buffers when the algorithm is naturally byte-oriented or when the
optimizer intentionally rejects the shape.

## Testing Strategy

For each phase, keep the same guardrails used by the uniqueness optimizer:

- structural IR checks for builder calls
- runtime checks with optimization enabled
- runtime checks with optimization disabled
- negative fixtures for rejected unsafe shapes
- bootstrap parity between the Rust stage0 path and the boot compiler path where
  both implement the relevant feature

Important negative fixtures:

```tw
s = s.concat(s)
```

```tw
for condition {
  if s.len() > limit {
    return s
  }
  s = s.concat(chunk)
}
```

```tw
for condition {
  unknown(s)
  s = s.concat(chunk)
}
```

These should remain unoptimized unless the compiler gains explicit semantics for
those reads or calls.

## Open Questions

### Should there be a public `StringBuilder` API?

Probably not for the initial version. An internal builder keeps the language
surface small and lets ordinary immutable string code optimize when safe.

A public API can be considered later if there are important string-building
cases the optimizer cannot or should not infer.

### Should there be a separate `new` (empty) builder? — RESOLVED: no

No. The vector family was reduced to `from`-only in `cb53dc8` after a `new`-path
emptiness flag (`known_empty`) caused a self-host miscompile
([[project_loop_builder_known_empty_bug]]). `string_builder_from("")` is free,
so a `new` variant saves nothing and reintroduces that bug class. String family
is `from`-only.

### Should the builder use `Vector<Byte>` internally? — RESOLVED: no

Use a dedicated `rt_types__StrBuilder { len, buf }` growable byte buffer (see
Runtime Builder Shape). A `Vector<Byte>` would box every byte as `anyref` and
drag in the PVec-trie builder. A dedicated chunked byte builder can replace the
doubling buffer later without changing the source language or optimizer shape.

### Should string interpolation lower through the builder?

Eventually, yes. Interpolation is another natural string-building form. It can
share the same builder family after concat regions are working.

This should be a follow-up so the initial rollout stays focused on
`String.concat`.

### Should `String.from_utf8` be optimized with builder knowledge?

Only if needed. The concat builder appends existing valid strings, so it should
not need a public validation path. Byte-oriented user code should continue to
use `String.from_utf8` explicitly.

## Completion Criteria

This plan is complete when:

- internal string builder helpers exist
- safe straight-line and loop `String.concat` consume-reassign regions rewrite to
  builder operations
- unsafe accumulator-read, self-alias, and opaque-call cases remain rejected
- `p0394`-style repeated string construction can be written naturally without
  requiring explicit byte-vector buffering
- existing vector/dict/record uniqueness behavior is unchanged
