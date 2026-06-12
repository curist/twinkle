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
- **Pike VM** (Thompson NFA simulated in lockstep with capture slots): linear in
  input × program per anchored run, no catastrophic backtracking.
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

### Unanchored search (v1 strategy)

`find`/`find_all` are unanchored. v1 implements this as: for each start position
`0..len`, run the **anchored** Pike VM from that start; take the first start that
produces a match. The first matching start gives leftmost; the greedy-priority VM
at that start gives the greedy match — together, correct leftmost-greedy. This is
O(n²·m) worst case (n = input scalar length, m = program length), which is fine
for the target inputs. A single-pass lockstep
seed-per-position version (O(n·m)) is a v2 optimization, deferred because its
capture/priority bookkeeping is easy to get wrong.

### `find_all` and empty matches

`find_all` is built with `Iterator.unfold(pos, step)`. After a match `[s, e)`,
the scan resumes at `e`; if the match was empty (`s == e`), it resumes at `e + 1`
to make progress. **Invariant:** scanning stops once the resume position exceeds
`len`. Thus `a*` over `""` yields exactly one match `[0, 0)` and then halts.

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

### `vm` — Pike VM over a scalar array

Runs a `Program` anchored at a given start over the decoded `Vector<Int>`,
stepping positions while advancing a priority-ordered thread list; each thread
carries its capture-slot array. On reaching `Match`, the winning slots are sliced
out of the scalar array into substrings to build the `groups` vector. Priority
ordering (from `Split(x, y)` putting `x` first) yields greedy/leftmost-first
semantics without backtracking.

Because search is unanchored by trying each start position, anchors are absolute,
**not** relative to the search start: `Assert(Start)` checks current input
position `== 0` (not `== search_start`), and `Assert(End)` checks position
`== len`. Getting this wrong is the easiest bug to introduce here.

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
backreferences (would require dropping the linear-time guarantee); single-pass
lockstep unanchored search for O(n·m).
