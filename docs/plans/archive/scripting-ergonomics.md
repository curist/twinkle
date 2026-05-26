# Scripting Ergonomics: API Gaps for Text-Processing Tasks

## Context

We removed wasmtime from the stage0 compiler using two Python scripts that:
1. Parsed a Rust source file to identify test functions containing certain patterns
2. Removed those functions (tracking brace-depth to find boundaries) and wrote the result back

These are representative of a class of "text-processing CLI scripts" that any general-purpose
language should handle comfortably. This plan documents what Twinkle would need to make such
scripts ergonomic.

## The scripts in spirit

### Script 1: Find test functions matching a pattern

```
read file → split into lines → scan for `#[test]` markers →
  for each: find the `fn name(` line, extract name →
  track brace depth to find function end →
  check if body contains target strings →
  collect matching names into a set → print
```

### Script 2: Remove matched functions from file

```
same parsing as script 1 →
  mark line ranges (start..end) for deletion →
  filter out marked lines →
  collapse consecutive blank lines →
  write result back to file
```

### What a Twinkle version would look like (pseudocode)

```tw
use @std.fs
use @std.proc

fn main() Result<Void, String> {
  args := proc.args()
  path := args[1]
  content := try fs.read_text(path)
  lines := content.split("\n")

  // Find test functions that call runtime helpers
  runtime_tests: Dict<String, Bool> = Dict.new()
  i := 0
  for i < lines.len() {
    if lines[i].trim() == "#[test]" {
      // find the fn line
      j := i + 1
      for j < lines.len() and !lines[j].trim().starts_with("fn ") {
        j = j + 1
      }
      if j < lines.len() {
        name := extract_fn_name(lines[j])  // needs substring extraction
        fn_end := find_brace_end(lines, j)
        body := lines.slice(j, fn_end + 1).join("\n")
        if body.contains("assert_runtime_output") or body.contains("run_and_capture") {
          runtime_tests[name] = true
        }
      }
    }
    i = i + 1
  }

  // Filter out matched functions
  keep := Vector.make(lines.len(), true)
  // ... second pass marks ranges false ...

  result := collect i in range(lines.len()) {
    if keep[i] { lines[i] } else { continue }
  }
  try fs.write_text(path, result.join("\n"))
  .Ok({})
}
```

## API gaps identified

### Tier 1 — High impact, small effort

These are simple additions to existing prelude modules that directly unblock
common scripting patterns.

#### 1. `String.lines() Vector<String>`
Split on `\n` (and handle `\r\n`). Every text-processing script starts with this.
Currently requires `s.split("\n")` which works but `.lines()` is more intentional
and could handle `\r\n` correctly.

**Location:** `boot/prelude/string.tw`

#### 2. `String.strip_prefix(prefix) String?`
Returns the remainder after removing prefix, or `.None`.
The Python scripts use `line.strip_prefix("fn ")` to detect and extract simultaneously.
Currently requires `if s.starts_with(p) { s.slice(p.len(), s.len()) }` — verbose
and error-prone.

**Location:** `boot/prelude/string.tw`

#### 3. `String.strip_suffix(suffix) String?`
Symmetric with `strip_prefix`.

**Location:** `boot/prelude/string.tw`

#### 4. `String.count(needle) Int`
Count occurrences of a substring. The scripts use `line.count("{")` for brace-depth
tracking. Currently requires a manual loop with `index_of` + advancing position.

**Location:** `boot/prelude/string.tw`

#### 5. `String.replace(old, new) String`
Replace all occurrences. Currently requires `s.split(old).join(new)` which works
but is non-obvious and allocates intermediate vectors.

**Location:** `boot/prelude/string.tw`

#### 6. `Vector.position(pred) Int?` / `Vector.find_index(pred) Int?`
Return the index of the first element matching a predicate.
The scripts frequently need "find the next line where X" — `find` returns the
element but not its position.

**Location:** `boot/prelude/vector.tw`

#### 7. `Vector.flat_map(f) Vector<B>`
Map + flatten. Useful for "expand or remove" patterns (return `[]` to drop,
`[x]` to keep, `[x, y]` to expand).

**Location:** `boot/prelude/vector.tw`

#### 8. `Vector.enumerate() Vector<.{ index: Int, value: T }>`
or support `for value, index in vec` (which already exists via `for x, i in`).
Already supported in the language — no change needed.

### Tier 2 — Medium impact, moderate effort

#### 9. `Dict` as `Set` (or a proper `Set<T>` type)
The scripts use Python `set()` to collect function names. `Dict<String, Bool>`
works but is clunky: `set[k] = true` to add, `set.has(k)` to check.
Options:
- A `Set<T>` type backed by the same HAMT as Dict
- Or just document the `Dict<K, Bool>` pattern and add `Dict.from_keys(vec)`

**Location:** `boot/prelude/set.tw` — Phase 2 done

#### 10. Indexed iterator loops — already supported
Twinkle already supports indexed loops over iterators with `for x, i in iter` and
indexed collects with `collect x, i in iter { ... }`, so a dedicated
`Iterator.enumerate()` is not needed for the common "process lines with line
numbers" pattern.

No API change needed.

#### 11. `Iterator.skip(n)`
Skip first n elements. Useful for "skip header lines" patterns.

**Location:** `boot/prelude/iterator.tw` — Phase 2 done

#### 12. `Iterator.take_while(pred)` / `Iterator.skip_while(pred)`
Common for "consume until condition" patterns, which the brace-tracking logic
needs.

**Location:** `boot/prelude/iterator.tw` — Phase 2 done


### Tier 3 — Nice to have, larger effort

#### 14. Regex or glob matching
The Python scripts use `re.match(r'\s*fn\s+(\w+)', line)` to extract function
names. Without regex, you write manual parsers. A basic regex engine is a big
undertaking, but even a simple `String.match_glob(pattern)` or
`String.find_pattern(pattern)` would help.

#### 15. `stderr` / `eprintln`
Scripts often print diagnostics to stderr. Currently only `println` exists
(stdout). An `eprintln` builtin would be useful.

#### 16. Mutable local collections (or `Cell<Vector<T>>` ergonomics)
The script builds up a `Set` incrementally. In Twinkle, the idiomatic way is
rebinding (`names = names.set(k, true)`) which works but feels heavy for
imperative scripts. Not proposing mutation — just noting the friction.

## What already works well

- `fs.read_text` / `fs.write_text` — file I/O is clean
- `proc.args()` — CLI args work
- `String.split`, `.contains`, `.starts_with`, `.trim`, `.index_of` — core string ops are solid
- `String.lines`, `.strip_prefix`, `.strip_suffix`, `.count`, `.replace` — Phase 1 done
- `Vector.filter`, `.map`, `.fold`, `.join` — collection pipeline is good
- `Vector.position`, `.flat_map` — Phase 1 done
- `for x, i in vec` and `for x, i in iter` — indexed iteration already exists
- `collect` comprehensions with `continue` for filtering — very nice
- String interpolation `"${expr}"` — great for output formatting
- `try` for error propagation — clean Result/Option handling
- `eprint` / `eprintln` — stderr output already available as builtins

### Current patterns for Phase 2 items

Many Phase 2 items are already expressible with existing constructs:

- **Set**: dedicated `Set<K>` type — Phase 2 done
- **Iterator enumerate**: use Twinkle's indexed iterator loop directly — `for x, i in it { ... }`
- **Iterator skip**: `it.skip(n)` — Phase 2 done
- **Iterator take_while / skip_while**: `it.take_while(pred)` / `it.skip_while(pred)` — Phase 2 done
- **String from iterator**: `it.to_vector().join("")`

## Recommended implementation order

**Phase 1** (unblocks most scripting): items 1-7 — **done**
- `lines`, `strip_prefix`, `strip_suffix`, `count`, `replace` on String
- `position` and `flat_map` on Vector

**Phase 2** (convenience): items 9-12 — **done**
- `Set<K>` type backed by HAMT
- Iterator combinators: `skip`, `take_while`, and `skip_while`
- Indexed iterator loops use existing `for x, i in iter` syntax; no `enumerate` API needed

**Phase 3** (power features): items 14-16
- Regex or pattern matching
- ~~stderr output~~ — `eprint` / `eprintln` already exist
