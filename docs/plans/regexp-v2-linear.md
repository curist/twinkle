# `@std.regexp` v2 linear-time extensions

**Status:** proposed follow-up to `docs/plans/regexp.md`.

## Goal

Extend `@std.regexp` with useful regex features that preserve the current Pike VM
architecture and its O(n·m) matching guarantee:

- `compile_with(pattern, options)`
- global options: `ignore_case`, `multiline`, `dotall`
- inline-scoped flags: `(?i:...)`, `(?m:...)`, `(?s:...)`, and combinations
- lazy quantifiers: `*?`, `+?`, `??`, `{m,n}?`
- named capturing groups and named group lookup

Explicitly do **not** include features that require backtracking or otherwise
complicate the linear-time guarantee.

## Non-goals

- Backreferences (`\1`, `\k<name>`) — not regular and would require dropping the
  current linear-time guarantee.
- Lookaround (`(?=...)`, `(?!...)`, `(?<=...)`, `(?<!...)`) — deferred unless a
  future design proves a restricted, linear-time subset fits the VM cleanly.
- Conditional patterns, atomic groups, possessive quantifiers, or other
  backtracking-engine features.
- Changing the default v1 behavior. `compile(pattern)` must keep today’s defaults.

## Public API

Add an options record:

```tw
pub type CompileOptions = .{ ignore_case: Bool, multiline: Bool, dotall: Bool }
```

Add a constructor:

```tw
pub fn compile_with(pattern: String, options: CompileOptions) Result<Regexp, RegexError>
```

`compile(pattern)` becomes equivalent to:

```tw
compile_with(pattern, .{ ignore_case: false, multiline: false, dotall: false })
```

`must(pattern)` continues to call `compile(pattern)`. A separate convenience may
be added if desired:

```tw
pub fn must_with(pattern: String, options: CompileOptions) Regexp
```

Extend `Match` named-group lookup with an inherent method:

```tw
pub fn group_named(m: Match, name: String) String?
```

To support that, `Match` needs access to the compiled regex’s name table or must
materialize names with the match. Prefer pre-materializing a name map/vector in
`Match` only if Twinkle stdlib ergonomics make it simple; otherwise add a method
on `Regexp`:

```tw
pub fn group_named(re: Regexp, m: Match, name: String) String?
```

The preferred API should be decided during implementation based on whether it can
avoid keeping the source string alive and avoid excessive per-match copying.

## Options semantics

### `ignore_case`

`compile_with(..., .{ ignore_case: true, ... })` is equivalent to a leading
`(?i)` for the whole pattern. It remains ASCII-only case folding, matching v1.

If both an option and inline flags are present, inline flags override within their
scope.

### `multiline`

When `multiline` is false, v1 anchor behavior remains:

- `^` matches only input position `0`.
- `$` matches only input end.

When `multiline` is true:

- `^` matches input position `0` or a position immediately after `\n`.
- `$` matches input end or a position immediately before `\n`.

No other newline conventions are added in this plan.

### `dotall`

When `dotall` is false, v1 `.` behavior remains: it matches any scalar except
`\n`.

When `dotall` is true, `.` matches any scalar, including `\n`.

## Inline-scoped flags

Support flag groups:

```regex
(?i:expr)   // ignore_case on inside expr
(?m:expr)   // multiline on inside expr
(?s:expr)   // dotall on inside expr
(?im:expr)  // combinations allowed
(?-i:expr)  // disable a flag inside expr
(?i-m:expr) // enable i, disable m
```

Leading global `(?i)` from v1 remains supported. v2 may also support leading
`(?m)`, `(?s)`, and combinations like `(?im)` as global flags, but scoped flag
groups are the primary feature.

Invalid forms should be parse errors at the opening `(` or flag position, with a
stable message such as `"bad flag group"`.

## Lazy quantifiers

Add lazy forms:

```regex
a*?       // zero or more, prefer shorter
+a??      // ordinary plus followed by lazy optional when parsed that way
(a+?)     // one or more, prefer shorter
x{2,5}?   // bounded repeat, prefer the smallest count that still allows a match
```

Precise supported forms:

- `*?`
- `+?`
- `??`
- `{m}?`
- `{m,}?`
- `{m,n}?`

Lazy quantifiers still participate in leftmost matching. They only reverse the
priority of the quantifier’s continue/exit split. This preserves the Pike VM
model and linear-time behavior.

Examples:

```tw
regexp.must("a+?").find("aaa").unwrap().text() == "a"
regexp.must("a+?a").find("aaa").unwrap().text() == "aa"
regexp.must("<.*?>").find("<a><b>").unwrap().text() == "<a>"
```

## Named groups

Support named capturing groups:

```regex
(?P<name>expr)
(?<name>expr)
```

Both syntaxes are optional; if keeping the parser small is preferred, implement
only `(?<name>expr)` first. Names must follow Twinkle value identifier style
(`snake_case`, lowercase first) unless there is a strong reason to match broader
regex conventions. Duplicate names are parse errors.

Named groups are also numbered capturing groups in source order. This remains
valid:

```tw
m.group(1)
```

Named lookup returns the same capture text:

```tw
m.group_named("id")
```

or, if the API lands on regex-assisted lookup:

```tw
re.group_named(m, "id")
```

## Engine representation changes

The VM can stay a Thompson/Pike simulation. Required internal changes:

### Flags in AST/program

Represent flag-sensitive operations explicitly so each thread does not need to
carry an entire flag environment unless necessary.

Recommended approach:

- Parse with a current `Flags` record:

```tw
type Flags = .{ ignore_case: Bool, multiline: Bool, dotall: Bool }
```

- Attach flags to flag-sensitive AST nodes or directly emit flag-sensitive
  instructions:
  - literal/class matching needs `ignore_case`
  - `Any` needs `dotall`
  - anchors need `multiline`

Instruction options:

```tw
Char(scalar, ignore_case)
Any(dotall)
Class(ranges, negated, ignore_case)
Assert(Start, multiline)
Assert(End, multiline)
```

If enum payload changes are too invasive, add parallel instruction variants.

### Lazy lowering

Greedy lowering currently puts the continue/body branch before the exit branch in
`Split(x, y)`. Lazy lowering reverses that priority:

```text
// greedy a*
Split(body, exit)
body: Char(a); Jmp(split)
exit:

// lazy a*?
Split(exit, body)
body: Char(a); Jmp(split)
exit:
```

For bounded repeats, reverse each optional split introduced by the bounded tail.
Mandatory repetitions remain mandatory and have no split.

### Multiline anchors

`Assert(Start, multiline)` succeeds when:

```tw
pos == 0 or (multiline and pos > 0 and chars[pos - 1] == 10)
```

`Assert(End, multiline)` succeeds when:

```tw
pos == len or (multiline and pos < len and chars[pos] == 10)
```

This requires `add_thread` to know `chars`, not only `pos` and `len`, or to be
passed enough context to inspect adjacent scalars.

### Named group table

Extend `Regexp` with a group-name table:

```tw
pub type Regexp = .{
  program: Vector<types.Inst>,
  group_count: Int,
  ignore_case: Bool,   // replaced or supplemented by options/flags
  group_names: Vector<types.NamedGroup>,
}
```

where:

```tw
type NamedGroup = .{ name: String, index: Int }
```

If `Match.group_named` is chosen, either copy this table into `Match` or add a
small `Dict<String, String?>` of captured named values while materializing. If
`Regexp.group_named(m, name)` is chosen, no per-match name table is needed.

## Parser changes

- Add `CompileOptions` and thread initial `Flags` into `parse`.
- Extend group parsing to distinguish:
  - `(?:...)` non-capturing
  - `(?i:...)` / flag groups
  - `(?<name>...)` named captures
  - leading global `(?i)`, optionally `(?m)`, `(?s)`, combinations
- Extend repeat parsing to detect trailing `?` after a quantifier and mark the
  repeat as lazy.
- Keep parser errors stable and add tests for malformed flag groups, malformed
  named groups, duplicate names, and bad lazy-repeat placements.

## Testing

Add focused tests in the regexp dev harness and port representative cases to the
boot suite.

### Options

```tw
regexp.compile_with("abc", .{ ignore_case: true, multiline: false, dotall: false }).unwrap().test("ABC")
regexp.compile_with("^b", .{ ignore_case: false, multiline: true, dotall: false }).unwrap().test("a\nb")
regexp.compile_with("a.b", .{ ignore_case: false, multiline: false, dotall: true }).unwrap().test("a\nb")
```

### Scoped flags

```tw
regexp.must("a(?i:b)c").test("aBc")
!regexp.must("a(?i:b)c").test("ABc")
regexp.must("(?s:a.b)").test("a\nb")
regexp.must("(?m:^b)").test("a\nb")
```

### Lazy quantifiers

```tw
regexp.must("a+?").find("aaa").unwrap().text() == "a"
regexp.must("a+?a").find("aaa").unwrap().text() == "aa"
regexp.must("<.*?>").find("<a><b>").unwrap().text() == "<a>"
regexp.must("a{2,4}?").find("aaaa").unwrap().text() == "aa"
```

### Named groups

```tw
m := regexp.must("(?<id>%d+)-(?<color>%w+)").find("12-red").unwrap()
m.group(1).unwrap() == "12"
m.group(2).unwrap() == "red"
m.group_named("id").unwrap() == "12"
m.group_named("color").unwrap() == "red"
m.group_named("missing") == .None
```

### Error cases

- duplicate named group
- malformed `(?` flag group
- unclosed named group syntax
- lazy marker with nothing to repeat (`*?a` should still error at `*`)
- unsupported lookaround/backreference syntax still rejects clearly

## Documentation updates

Update `docs/API.md`:

- Add `CompileOptions`, `compile_with`, and possibly `must_with`.
- Document global and scoped flags.
- Document lazy quantifiers.
- Document named groups and lookup API.
- Keep a clear note that backreferences and lookaround are intentionally not
  supported so matching remains linear-time.

## Implementation order

1. Add `CompileOptions` and global `ignore_case`/`multiline`/`dotall` plumbing.
2. Convert instruction representation to carry flag bits where needed.
3. Implement multiline anchors and dotall `.`.
4. Implement scoped flag groups by parsing with threaded `Flags`.
5. Implement lazy quantifier parsing and lowering.
6. Implement named-group parsing, duplicate detection, and lookup API.
7. Update dev harness and boot suite throughout.
8. Update docs.
9. Run `make bundle-cli`, `target/twk run boot/tests/main.tw`, and `make test`.

## Compatibility

All v1 patterns keep their behavior under `compile(pattern)` and `must(pattern)`.
New behavior is opt-in through `compile_with`, scoped flags, lazy suffixes, or
named group syntax. Patterns that currently use unsupported constructs like
`(?i:...)` or `*?` as accidental literals may parse differently after v2; this is
acceptable because those forms are standard regex syntax and currently outside
v1’s supported subset.
