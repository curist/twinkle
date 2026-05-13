# Length + trie dispatch for string case expressions

## Status

Implemented in the boot compiler's `boot/compiler/codegen/emit/match.tw`.
Eligible string `case` expressions now use length dispatch followed by trie or
small-chain dispatch, with final `rt_str__eq` guards at leaves.

## Context

String `case` expressions are currently compiled as linear if-else chains. Each
arm calls `rt_str__eq` to compare the scrutinee against a string literal, and
the arms are nested recursively via `emit_arm_chain` — the same path as variant
matching.

For a case with N string arms, the worst case is N full string equality checks,
each O(len) in string length. This is O(N × len).

The boot compiler has several large string case expressions:

| File | Arms | Description |
|---|---|---|
| `builtins.tw:59` | 72 | `builtin_abi` — ABI lookup by name |
| `codegen/emit.tw:908` | 29 | intrinsic emission by name |
| `lexer.tw:16` | 22 | keyword recognition |
| `lexer.tw:120` | 25 | keyword token mapping |
| `signatures.tw` | 12, 11 | type name resolution |
| `resolver.tw` | 12, 8, 6 | builtin type/name resolution |
| `lower_core/calls.tw` | 11 | contract dispatch |

## Strategy: length dispatch + character trie

All string literals in a case expression have known lengths at compile time.
Strings of different lengths can never be equal, so dispatching on length first
partitions the arms into groups that share no matches.

Within each length group, a character-position trie discriminates strings by
inspecting individual bytes at specific positions, sharing common prefix work.

Both operations use only `array.len` and `array.get_u` — O(1) wasm operations
already available, no runtime hash function needed.

### Why not hash dispatch

Hash dispatch (compute hash, switch on hash, verify with equality) is a common
strategy for string switches in traditional compilers. For Twinkle/wasm it has
drawbacks:

- Hash values are sparse i32s — `br_table` does not apply, requiring binary
  search or a hash map structure.
- Requires calling `hash_string` (FNV-1a loop over all bytes) even when a
  length check alone would eliminate the string.
- Collision handling adds code complexity with no benefit when all literals are
  known at compile time.

Length + trie avoids all of these: length is a small dense integer suitable for
`br_table`, character comparisons are single-byte O(1) lookups, and there are
no collisions by construction.

### Data: how well length discriminates

Measured from the boot compiler's actual string case expressions:

**`builtins.tw` (72 arms):** 19 distinct lengths (5–23). Largest bucket: 12
strings at len=8. After `char[0]`, the len=8 bucket splits into 3 groups
(d/h/c). After `char[5]`, all 12 are fully distinguished. Max trie depth: 2
levels (char[0] + char[5]).

**`lexer.tw` (22 arms):** 7 distinct lengths (2–8). Largest bucket: 6 strings
at len=3. All 6 are unique on `char[0]` alone. The len=2 bucket (5 keywords)
needs `char[0]` + `char[1]` for `if`/`in`. Max trie depth: 2 levels.

**`emit.tw` (29 arms):** 11 distinct lengths (5–20). Largest bucket: 6 strings
at len=10. All unique on `char[0]`. Max trie depth: 1–2 levels.

In every measured case, length dispatch + 1–2 character checks resolves the
match, compared to up to 72 sequential `rt_str__eq` calls today.

## Design

### Eligibility

A case expression is eligible for length+trie dispatch when:

1. The scrutinee mono type is `String`.
2. Every arm's top-level pattern is either `.LitStr(s)` or `.Wildcard`/`.Var(_)`.
3. There are at least 4 string arms (below that, linear `rt_str__eq` is fine).
4. Each string literal appears **at most once** across all arms.
5. At most one wildcard/variable catch-all arm, and it is the **last** arm.

If any condition fails, fall back to the existing `emit_arm_chain`.

Rule 4 prevents ambiguity in the trie — duplicate literals would mean two arms
compete for the same leaf, and the trie builder cannot preserve source-order
priority without additional complexity.

Rule 5 ensures source-order semantics are preserved. A catch-all before explicit
arms would shadow them in the current if-else chain; length+trie dispatch must
not change that behavior. In practice, the type checker should reject
unreachable arms after a catch-all, so this rule is defensive.

### Lowering overview

```
case s {
  "add" => expr_add,
  "sub" => expr_sub,
  "mul" => expr_mul,
  _ => expr_default,
}
```

Step 1 — group arms by string length:

```
len=3: ["add" => expr_add, "sub" => expr_sub, "mul" => expr_mul]
```

Step 2 — emit length dispatch:

For multiple length buckets, use `br_table` (if range is dense) or if-chain
(if sparse) to route to the correct bucket.

For a single length bucket, emit an explicit length equality check — do **not**
skip straight to the trie. Without the length guard, `array.get_u` would trap
on a shorter runtime string (e.g. `""` when the bucket expects length 3).

```wat
;; multiple buckets:
<scrutinee>
ref.as_non_null
array.len
i32.const <min_len>
i32.sub
br_table $len_5 ... $len_8 $default

;; single bucket (len=3):
<scrutinee>
ref.as_non_null
array.len
i32.const 3
i32.ne
br_if $default       ;; length mismatch → default
```

Step 3 — within each length bucket, emit discrimination:

- **1 string:** direct `rt_str__eq` call
- **2–3 strings:** linear `rt_str__eq` chain (fast enough, avoids trie overhead)
- **4+ strings:** trie on character positions

Step 4 — trie nodes compare a single byte:

```wat
<scrutinee>
ref.as_non_null
i32.const <position>
array.get_u $String   ;; O(1), one byte
;; branch on byte value
```

If a byte value uniquely identifies one string, emit a final `rt_str__eq`
guard (to defend against strings not in the literal set that happen to share
the same length and prefix bytes). If multiple strings share the byte value,
recurse to the next trie level.

### Trie construction (compile-time)

Given a set of equal-length strings to discriminate:

1. Find the character position that maximizes the number of distinct groups,
   excluding positions already tested on the current path. Prefer earlier
   positions on ties (shorter prefix = less work).
2. Partition strings by byte value at that position.
3. For each partition:
   - Size 1: leaf — emit `rt_str__eq` + arm body.
   - Size 2–3: leaf — emit linear `rt_str__eq` chain.
   - Size 4+: recurse to next discriminating position.
4. If no unused discriminating position remains (all remaining strings are
   identical on untested positions), fall back to a linear `rt_str__eq` chain.
   Duplicate literals are rejected by eligibility rule 4, so this should not
   happen in practice, but the guard prevents infinite recursion.

This is a compile-time decision tree, not a runtime trie data structure.

### Wasm emission shape

Outer structure (length dispatch):

```wat
(block $match_end
  (block $default
    (block $len_8
      (block $len_5
        <scrutinee>
        ref.as_non_null
        array.len
        i32.const <min_len>
        i32.sub
        br_table $len_5 ... $len_8 $default
      )
      ;; len=5 bucket: trie or string_eq chain
      ;; each successful leaf: body + store + br $match_end
      ;; if no leaf matches: br $default
      <trie or string_eq chain>
      br $default            ;; no match in this bucket
    )
    ;; len=8 bucket
    <trie or string_eq chain>
    br $default              ;; no match in this bucket
  )
  ;; default body: emit_pattern_bindings + emit_expr + store result
  ;; (bindings needed for .Var catch-all; no-op for .Wildcard)
  ;; if no catch-all: unreachable
  <default_bindings>
  <default_body>
)
```

Trie node (character dispatch within a length bucket):

```wat
<scrutinee>
ref.as_non_null
i32.const <best_pos>
array.get_u $String
;; if byte values are dense enough: br_table
;; otherwise: if-else chain on byte values (typically 3-6 branches)
```

Leaf (final candidate in bucket/trie branch):

```wat
<scrutinee>
<string_literal>
call $rt_str__eq
if
  <arm body>
  local.set $result
  br $match_end
end
br $default              ;; guard failed, no more candidates — route to default
```

In a `Chain` (2–3 candidates), non-final candidates fall through to the next
`rt_str__eq` check on guard failure rather than branching to `$default`
immediately. Only the final candidate in the chain emits `br $default`.

### Control flow on miss

Every length bucket and trie node must explicitly `br $default` when no leaf
matches. Without this, a failed bucket would fall through into the next
bucket's code, silently executing the wrong arm.

Each successful leaf emits: arm body + result store + `br $match_end`.
Each failed leaf (guard returned false) falls through to the next candidate
in the bucket, or `br $default` if no candidates remain.

### Why always keep the `rt_str__eq` guard

The trie discriminates strings within the known literal set, but the scrutinee
at runtime could be *any* string. A string not in the literal set might still
match the same length and share the same discriminating bytes. The final
`rt_str__eq` ensures correctness; the trie just avoids calling it N times.

Always emit the `rt_str__eq` guard for every string leaf, even the last one in
a bucket. String cases are never truly exhaustive over the `String` type — the
type system cannot prevent arbitrary runtime strings — so omitting the guard
would risk executing the wrong arm for an input that happens to share length
and discriminating bytes with a literal.

### Length density and br_table eligibility

Not all length ranges are dense. For `builtins.tw`, lengths span 5–23 (range 19)
with 19 values occupied — fully dense, ideal for `br_table`. For sparser ranges,
options are:

- **Dense (>50% occupied):** `br_table` with unused entries pointing to default.
- **Sparse (<50% occupied):** if-else chain or binary search on length.
  At the outer level this is comparing small integers, so even linear scan is
  cheap for < 10 distinct lengths.

The eligibility threshold can be tuned; start with `br_table` when
`(max_len - min_len + 1) <= 2 * distinct_lengths`.

## Integration

### Change point

Same as variant br_table: the entry point is `emit_match_op` in
`boot/compiler/codegen/emit/match.tw`. After the existing variant br_table
eligibility check (from the separate br_table plan), add a string trie
eligibility check. If eligible, call `emit_string_trie_match`; otherwise fall
through to `emit_arm_chain`.

### No IR changes

Like the variant br_table optimization, this is purely a codegen strategy.
`CorePattern.LitStr`, `AnfMatchArm`, `PreparedPattern.LitStr` are unchanged.
The trie is built at emission time from the literal strings in the arms.

### Wasm instructions used

All already available and serialized:
- `Block`, `Br`, `BrTable` — control flow (from variant br_table work)
- `ArrayLen` — string length (already used in `hash_string`)
- `ArrayGetU` — byte access (already used in `hash_string`)
- `Call("rt_str__eq")` — string equality (already used in current if-else chain)

### Prerequisite

The variant br_table plan should land first. It introduces the `br_table`
emission patterns (nested blocks, label management, result store per arm) that
this plan reuses for both the outer length dispatch and inner byte dispatch.

## Steps

### Step 1: Implement compile-time trie builder

Add a function (in `match.tw` or a new `match_string.tw` helper) that takes a
list of `(string_literal, arm_index)` pairs of equal length and produces a
discrimination tree:

```
type TrieNode = {
  Leaf(String, Int),                          // literal, arm_index
  Chain(Vector<.{ literal: String, arm_index: Int }>),  // 2-3 strings, linear eq
  Branch(Int, Vector<.{ byte: Int, child: TrieNode }>), // char position, branches
}
```

**Verify:** unit-test or manual check that the trie correctly partitions the
string sets from `builtins.tw` (len=8 bucket: 12 strings) and `lexer.tw`
(len=2 bucket: 5 keywords).

### Step 2: Implement length grouping and dispatch emission

Add `emit_string_trie_match` that:

1. Groups arms by string literal length (using UTF-8 byte length, matching
   `array.len` semantics). Reuse `ctx.registry.string_pool[s]` for literal
   getters, same as `emit_pattern_condition` does today.
2. Identifies the default arm (if any). If the default pattern is `.Var(sid)`,
   the default body must call
   `emit_pattern_bindings(default_pattern, scrutinee_instrs, scrutinee_mono, ctx)`
   before `emit_expr` to bind the scrutinee to the variable.
3. Determines whether to use `br_table` or if-chain for length dispatch.
4. Emits the outer length dispatch structure (blocks + br_table/if-chain).
5. For each length bucket, delegates to step 3's trie node emission.
6. Each bucket ends with `br $default` if no leaf matched.

**Verify:** emit WAT output (`target/twk ir --opt` or similar) for a small
test case with known strings and inspect the generated dispatch structure.

### Step 3: Implement trie node emission

Emit wasm for each `TrieNode`:

- `Leaf`: `rt_str__eq` + arm body
- `Chain`: linear `rt_str__eq` chain (2–3 calls)
- `Branch`: `array.get_u` at position + if-chain on byte values (or `br_table`
  if byte values happen to be dense), each child recursing

**Verify:** check that the emitted code for `builtins.tw`'s 72-arm case reads
the length/bytes at most ~3 times (length + 1–2 char positions) instead of 72
`rt_str__eq` calls.

### Step 4: Wire eligibility check into `emit_match_op`

In `emit_match_op`, after the variant br_table check:

```
if is_string_trie_eligible(arms, scrutinee_mono, ctx) {
  emit_string_trie_match(...)
} else {
  emit_arm_chain(...)
}
```

The eligibility check needs `scrutinee_mono` to verify the scrutinee is
`String` (rule 1).

### Step 5: Bootstrap verification

```bash
make stage2
target/twk run boot/tests/main.tw
```

Stage2 must pass all tests. Compare behavior on edge cases:
- Empty string arm
- Single-char string arms
- Wildcard-only fallback (`_ => ...`)
- Variable catch-all (`other => use(other)`) — must bind scrutinee
- All arms same length — must emit length equality guard before trie byte reads
- Input string matching length but not any literal (must route to default)

### Step 6: Measure

A/B timing comparison:

```bash
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/before.wasm
# ... rebuild with string trie ...
TWINKLE_TIMINGS=1 target/twk build boot/main.tw -o /tmp/after.wasm
```

Expected impact:
- `compile_modules` may improve if `builtins.tw`'s 72-arm case or lexer keyword
  matching is measurable in the profile (these run per-module or per-token).
- Binary size should decrease slightly (fewer `rt_str__eq` call sequences).
- Keyword lexing throughput may improve noticeably (22 arms → ~2 comparisons).

## Risks

- **Complexity vs payoff:** This is more complex than the variant br_table
  change. The trie builder, length grouping, and multi-level emission are new
  code. If measurement shows the large string cases are not hot paths (e.g.
  `builtin_abi` runs only at startup), the payoff may be small.

- **String encoding:** The trie assumes byte-level access via `array.get_u`.
  Twinkle strings are currently UTF-8 byte arrays, so this is correct. If
  string representation changes (e.g. to UTF-16), the trie byte values would
  need adjustment. All current string literals in case arms are ASCII, so this
  is not a practical concern.

- **Interaction with the nested pattern bug:** Same as variant br_table — this
  change does not alter pattern matching semantics, only the dispatch mechanism.
  Pattern bindings and sub-patterns are not involved (string patterns have no
  sub-patterns).

- **Code size:** Deep tries with many branches could generate more wasm
  instructions than the linear chain for small cases. The eligibility threshold
  (4+ arms) guards against this, and the `Chain` leaf type keeps small buckets
  as simple linear comparisons.
