# `@std.regexp` — a pure-Twinkle regular-expression library

**Status:** design approved, pending implementation plan.

## Goal

Add a regular-expression capability to the standard library as `@std.regexp`,
implemented entirely in Twinkle. The immediate motivation is structured-line
parsing (Advent of Code and similar), where extracting fields out of lines like
`Game 1: 3 blue, 4 red` or `mul(2,4)` is awkward with `split` + `Int.from_string`
alone. Regex is also a broadly useful stdlib feature beyond that.

It is a *capability*, not a privileged builtin: no compiler or Rust changes, no
new syntax. It follows the standard pure-Twinkle stdlib-module wiring
(`boot/stdlib/*.tw` → regen `core_lib` → `bundle-cli` → suite + docs).

## Non-goals (v1)

Deliberately excluded; all are additive later and none change the architecture:

- Backreferences (`\1`)
- Lookaround (`(?=…)`, `(?<=…)`)
- Lazy quantifiers (`*?`, `+?`)
- Multiline `(?m)` / dotall `(?s)`
- Named groups
- Non-ASCII case folding (`(?i)` folds ASCII letters only)
- An options object (`compile_with`) — reserved for v2

## Design choices (settled)

- **Match over Unicode scalars**, not bytes: the input is decoded to a
  `Vector<Int>` of code points before matching, so `.` and the classes match one
  scalar regardless of UTF-8 encoding.
- **Captures via `group(i)`, 1-based**, with `group(0)` = the whole match.
- **Single-pass Pike VM** (Thompson NFA simulated in lockstep with capture
  slots): one left-to-right pass, O(n·m), no catastrophic backtracking.
  Unanchored search seeds a fresh start thread per position at lowest priority.
- **Pre-materialized captures**: a `Match` holds the captured substrings
  directly, so it does not keep the source string alive across an
  `Iterator<Match>`.
- **Pure Twinkle**, runtime-independent (works on bare wasm, not just the host).

## Public API

```tw
pub type Regexp                                  // compiled program + group_count + ignore_case
pub type Match = .{ start: Int, end: Int, groups: Vector<String?> }
pub type RegexError = .{ pos: Int, message: String }

// constructors
pub fn compile(pattern: String) Result<Regexp, RegexError>
pub fn must(pattern: String) Regexp              // traps via error("regexp:${pos}: ...") on a bad pattern

// inherent methods on Regexp (dot-sugar)
pub fn test(re: Regexp, s: String) Bool
pub fn find(re: Regexp, s: String) Match?
pub fn find_all(re: Regexp, s: String) Iterator<Match>      // lazy, via Iterator.unfold
pub fn replace(re: Regexp, s: String, repl: String) String       // first match only
pub fn replace_all(re: Regexp, s: String, repl: String) String   // every match

// inherent methods on Match
pub fn group(m: Match, i: Int) String?           // .None if i<0, i>=groups.len, or the group didn't participate
pub fn text(m: Match) String                     // group(0).unwrap()
```

`start`/`end` are scalar offsets into the input. `groups` is length
`group_count + 1`: `groups[0]` is the whole match (always present on a successful
match), `groups[1..]` are the capturing groups in source order, each `.None` if
that group did not participate.

Usage:

```tw
use @std.regexp
use @std.regexp.{Regexp, Match}   // two-line idiom: module alias + the owned types

re := regexp.must("(\\d+) (red|green|blue)")
for m in re.find_all(line) {
  n := Int.from_string(m.group(1).unwrap())
  color := m.group(2).unwrap()
}
```

## Supported syntax (v1 subset)

| Group | Supported |
|---|---|
| Literals & `.` | any scalar; `.` matches any scalar **except `\n`** |
| Classes | `[abc]`, `[a-z]`, `[^…]`, with `\d \w \s \D \W \S` usable inside |
| Predefined classes | `\d`=`[0-9]`, `\w`=`[0-9A-Za-z_]`, `\s`=`[ \t\n\r\f\v]` (and negations) |
| Quantifiers | `*` `+` `?` `{m}` `{m,}` `{m,n}` — greedy only |
| Groups | `(…)` capturing, `(?:…)` non-capturing |
| Alternation | `a` or `b` or `c` — first branch has priority |
| Anchors | `^` start-of-input, `$` end-of-input |
| Escapes | `\n \t \r \f \v \\` and `\` before any metachar; `\uXXXX` |
| Flags | leading `(?i)` only (case-insensitive, ASCII) |

## Semantics

### `(?i)` case-insensitivity

`(?i)` is legal **only at pattern position 0** and sets a global ASCII-letter
case-fold for the whole pattern. Valid: `(?i)foo`, `foo`. Parse errors in v1:
`foo(?i)bar`, `(?i:foo)`, `(?-i)foo`. Folding applies to ASCII `A–Z`/`a–z` only;
non-ASCII scalars are matched as-is.

The fold covers literal letters **and** ASCII letter ranges/classes alike, so
`(?i)a`, `(?i)[a]`, and `(?i)[a-z]` all match `A`. Implementation: fold the input
scalar and compare against folded literals; for a class, test membership against
both the scalar and its ASCII-folded counterpart (so `[a-z]` admits `A–Z` without
rewriting the range bounds). Non-ASCII scalars pass through unchanged.

### Anchors and `.`

`^` matches only position 0; `$` matches only end-of-input (no multiline in v1).
`.` matches any scalar except `\n`.

### Unanchored search (single-pass Pike VM)

`find`/`find_all` are unanchored, implemented as one left-to-right pass of the
Pike VM (O(n·m); n = input scalar length, m = program length). At each position
`pos` in `0..=len`, **if no match has been recorded yet**, a fresh start thread
(`pc = 0`) is seeded into the current thread list at **lowest priority** (appended
after existing threads). Because older start positions sit ahead of newer ones in
priority order, an older start that can still match wins → **leftmost**. Within a
single start, `Split(x, y)` exploring `x` first gives **greedy / first-alternation**
priority.

**Match handling is record-and-continue, not return-on-first-match.** When a
thread reaches `Match`, record its slots as the current best and **cut all
lower-priority threads in this generation** (they cannot beat it) — but keep
stepping the higher-priority threads already carried into the next generation, so
a greedy loop can extend to a longer match and overwrite the record. Return the
last recorded match when the input is exhausted (or the thread list empties).

> Returning immediately the first time any thread reaches `Match` is a **greedy
> bug**: for `a+` on `"aaa"` the exit branch of the first `a` reaches `Match` at
> `pos 1` while the higher-priority loop thread is still alive, so an
> early return yields `"a"` instead of `"aaa"`. Record-and-continue with the
> lower-priority cut is what makes it greedy *and* leftmost.

Once a match is recorded, **stop seeding new start threads** (a later start cannot
beat an already-found earlier match), but let the in-flight higher-priority
threads finish extending.

### `find_all` and empty matches

The VM exposes `find_from(re, chars, start) Match?` — the same single pass, but
the first seed is placed at `pos = start` and no thread is seeded before `start`,
so it returns the leftmost match at or after `start`. `find` is `find_from(.., 0)`.

`find_all` is built with `Iterator.unfold(start, step)`, where each step calls
`find_from(re, chars, start)`. After a match `[s, e)` the scan resumes at `e`; if
the match was empty (`s == e`), it resumes at `e + 1` to make progress.
**Invariant:** scanning stops once the resume position exceeds `len`. Thus `a*`
over `""` yields exactly one match `[0, 0)` and then halts.

### Replacement (`$` expansion)

`replace`/`replace_all` expand the replacement string as follows:

- `$0 … $9` → the corresponding group's text (`$0` = whole match)
- `$$` → a literal `$`
- a `$` not followed by `$` or a digit → a literal `$`
- a reference to a group that does not exist or did not participate → `""`

`replace_all` reuses the `find_all` scan (including the empty-match advance rule)
and builds the result by **copying the gap, then emitting the replacement**:
walking matches in order, copy the input between the previous match's end and the
current match's start, append the expanded replacement, and continue. On an empty
match, after emitting the replacement, copy the single scalar being stepped over
(if any) so it is not dropped, then advance. Finally append the input tail after
the last match. Contract examples:

```
replace_all(must("a*"), "",  "X") == "X"      // one empty match at [0,0)
replace_all(must("a*"), "b", "X") == "XbX"    // empty at 0, copy 'b', empty at end
replace_all(must("a+"), "baab", "X") == "bXb"
```

## Engine internals

Three internal layers, each independently testable.

### `parse` — pattern → `Ast`

Recursive descent: `alt → concat → repeat → atom`. Produces an `Ast` enum, counts
capturing groups, and returns `Result<Ast, RegexError>` carrying the offending
scalar position. `RegexError` cases surfaced here: unbalanced `(`/`)`, unbalanced
`[`, bad `{m,n}`, trailing `\`, unknown escape, misplaced/garbled `(?i)`/`(?:`.

### `program` — `Ast` → `Program` (`Vector<Inst>`)

Instruction set:

```
Char(scalar)            match one specific scalar, advance
Any                     match any scalar except '\n', advance
Class(ranges, negated)  match scalar against a class, advance
Save(slot)              record current position into capture slot
Split(x, y)             fork; thread x has higher priority than y
Jmp(x)                  jump
Assert(Start | End)     zero-width anchor
Match                   accept
```

Lowering invariants (greedy; in `Split(x, y)` the first operand keeps priority):

```
a*              a+              a?              a|b
L: Split(B, D)  B: Char(a)      Split(B, D)     Split(A, B)
B: Char(a)      L: Split(B, D)  B: Char(a)      A: <a>
   Jmp(L)       D:              D:                 Jmp(D)
D:                                              B: <b>
                                                D:

group k:  Save(2k)  <body>  Save(2k+1)
```

`{m,n}` lowers by bounded repetition. Slot pair `2k`/`2k+1` brackets group `k`;
slot pair `0`/`1` brackets the whole match — the whole program is wrapped as
`Save(0) <body> Save(1) Match`. Greedy comes from putting the loop/continue body
(`B`) before the exit (`D`) in every `Split`.

### `vm` — single-pass Pike VM over a scalar array

`find_from(re, chars, start) Match?` runs one left-to-right pass over the decoded
`Vector<Int>`, advancing a priority-ordered thread list; each thread carries its
capture-slot array.

The core primitive is **`add_thread(list, thread, pos)`** — it appends a thread
*and recursively follows all zero-width instructions* (the ε-closure), so the
list is always ε-closed and runnable:

- `Jmp(x)` → `add_thread(pc := x)`
- `Split(x, y)` → `add_thread(x)` **then** `add_thread(y)` (x first = priority)
- `Save(slot)` → set `slots[slot] = pos`, `add_thread(pc + 1)`
- `Assert(Start)` → if `pos == 0`, `add_thread(pc + 1)`, else drop
- `Assert(End)` → if `pos == len`, `add_thread(pc + 1)`, else drop
- `Char` / `Any` / `Class` / `Match` → append the thread (consuming/terminal)

`add_thread` dedups by `pc` **within a single generation** (a per-step visited
set), keeping the *first* thread to reach each `pc` — which, given priority-order
insertion, has the winning captures. This dedup is what stops the list exploding.

Each step seeds (per the search rules above), then iterates the list in priority
order: `Match` → record + cut; `Char(c)`/`Any`/`Class` matching the current scalar
→ `add_thread(next, pc + 1, pos + 1)`; otherwise drop. On the recorded match the
winning slots are sliced out of the scalar array into substrings to build the
`groups` vector.

Anchors are **absolute**, not relative to `start`: `Assert(Start)` checks input
position `== 0` (not `== start`), `Assert(End)` checks `== len`. Getting this
wrong is the easiest bug to introduce here.

Threads are immutable records (`Thread.{ pc, slots }`) rebound functionally
(`t.with_pc(...)`, slot updates via `Vector.set`), matching Twinkle's value model.

## File layout & wiring

Public API in `regexp.tw`; internals split for testability, mirroring the
`tuple.tw` + `tuple/` convention:

```
boot/stdlib/regexp.tw          public types + API methods
boot/stdlib/regexp/parse.tw    pattern → Ast
boot/stdlib/regexp/program.tw  Ast → Program (+ the Inst type and lowering)
boot/stdlib/regexp/vm.tw       Pike VM over a scalar array
```

The submodules sit on public `@std.regexp.parse` / `.program` / `.vm` paths — a
cosmetic leak of internals. They are documented as **internal and unstable, not
part of stdlib compatibility**; the supported surface is `@std.regexp` only. If
Twinkle later gains private stdlib modules, they can be hidden then.

Wiring steps (no Rust changes): add the files → regenerate `core_lib` →
`make bundle-cli` → add a test suite → document in `docs/API.md`.

## Errors

`compile` returns `Result<Regexp, RegexError>` where `RegexError` is
`.{ pos: Int, message: String }` (`pos` is the scalar offset in the pattern).
`must` calls `compile` and traps with `error("regexp: ${message}")` on failure —
intended for author-written literal patterns where a typo is a programmer bug.

## Testing

Per-layer suites on the existing `assert`/`runner` harness:

- **parse** — pattern → `Ast` shape; every `RegexError` case at the right `pos`.
- **program** — `Ast` → `Inst` sequence; the lowering invariants above.
- **vm** — `Program` + input → match span and captures, including
  capture-didn't-participate → `.None`.
- **end-to-end** — `compile`/`must`/`test`/`find`/`find_all`/`replace`/
  `replace_all`: greedy quantifiers, alternation precedence, classes + negation,
  anchors, `(?i)`, empty-match scanning, `$n` expansion edge cases, and a few
  real AoC-shaped lines (`Game 1: 3 blue, 4 red`, `mul(2,4)`).

## Future (v2+)

`compile_with(pattern, .{ ignore_case, multiline, dotall })`; lazy quantifiers;
inline-scoped flags (`(?i:…)`); multiline/dotall; named groups; lookaround;
backreferences (would require dropping the linear-time guarantee).
