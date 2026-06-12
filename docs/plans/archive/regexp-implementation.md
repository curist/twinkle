# `@std.regexp` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a pure-Twinkle `@std.regexp` library — parse → program → single-pass Pike VM — with captures, an `Iterator`-based `find_all`, and `replace`/`replace_all`.

**Architecture:** Four files: `regexp.tw` (public types + API) and internal `regexp/parse.tw` (pattern → `Ast`), `regexp/program.tw` (`Ast` → `Program`), `regexp/vm.tw` (single-pass Pike VM over a scalar array). Matching decodes input to a `Vector<Int>` of code points; the VM seeds a start thread per position at lowest priority (leftmost) and uses record-and-continue on `Match` (greedy). Full design: `docs/plans/regexp.md`.

**Tech Stack:** Twinkle (`.tw`), the `target/twk` CLI, the `assert`/`runner` test harness in `boot/tests/`.

---

## Conventions used by every task

- **Build/run a file:** `target/twk run <file>` (compiles + runs; no rebuild needed for local modules).
- **Format after editing:** `target/twk fmt <file>` (idempotent; run before each commit).
- **The dev harness** (set up in Task 1) lets you test the real `boot/stdlib/regexp*` files with no compiler rebuild. Run the whole suite with:
  `target/twk run /tmp/rxdev/tests.tw`
- **Commit** the real source files (`boot/stdlib/regexp.tw`, `boot/stdlib/regexp/*.tw`) as you go. The `/tmp/rxdev` harness is never committed.
- **Variant syntax reminder:** declare `type T = { A, B(Int), C(Int, Int) }`; construct `T.B(5)` or contextually `.B(5)`; match `case x { .B(n) => …, _ => … }`. Enums may be recursive (they are GC references).
- **Vector reminder:** `xs.append(v)` returns a new vector; `xs.set(i, v)` returns a new vector; `xs[i]` reads; `xs.len()`; build loops by rebinding (`acc = acc.append(v)`).

---

## Shared data types (defined in Task 1, referenced everywhere)

These are the canonical signatures. Later tasks must match them exactly.

```tw
// regexp/program.tw
pub type ClassItem = .{ lo: Int, hi: Int }          // inclusive scalar range

pub type Inst = {
  Char(Int),                  // match this exact scalar, advance
  AnyChar,                    // match any scalar except '\n', advance
  Class(Vector<ClassItem>, Bool),  // (ranges, negated); match scalar, advance
  Split(Int, Int),            // fork: first operand = higher priority
  Jmp(Int),
  Save(Int),                  // record current pos into capture slot
  AssertStart,                // zero-width: pos == 0
  AssertEnd,                  // zero-width: pos == len
  Match,                      // accept
}

// regexp/parse.tw
pub type Ast = {
  Empty,
  Lit(Int),                   // a literal scalar
  AnyChar,
  Class(Vector<ClassItem>, Bool),
  Concat(Vector<Ast>),
  Alt(Vector<Ast>),
  Star(Ast),
  Plus(Ast),
  Opt(Ast),
  Repeat(Ast, Int, Int),      // {m,n}; n == -1 means unbounded
  Group(Ast, Int),            // capturing group, 1-based index
  NonCap(Ast),                // (?:...)
  AssertStart,
  AssertEnd,
}

// the parser returns this so callers learn the group count and ignore_case flag
pub type Parsed = .{ ast: Ast, group_count: Int, ignore_case: Bool }

// regexp.tw
pub type RegexError = .{ pos: Int, message: String }
pub type Regexp = .{ program: Vector<Inst>, group_count: Int, ignore_case: Bool }
pub type Match = .{ start: Int, end: Int, groups: Vector<String?> }

// regexp/vm.tw
type Thread = .{ pc: Int, slots: Vector<Int> }   // slots length 2*(group_count+1); -1 = unset
```

---

## Task 1: Dev harness + module skeleton + first green test

**Files:**
- Create: `boot/stdlib/regexp.tw`
- Create: `boot/stdlib/regexp/parse.tw`
- Create: `boot/stdlib/regexp/program.tw`
- Create: `boot/stdlib/regexp/vm.tw`
- Create (not committed): `/tmp/rxdev/twinkle.toml`, `/tmp/rxdev/tests.tw`, symlinks

- [ ] **Step 1: Create the four source files with the shared types and stubs**

`boot/stdlib/regexp/program.tw`:
```tw
/// Internal — unstable. Compiled-program representation. Not part of the
/// @std.regexp public surface.

pub type ClassItem = .{ lo: Int, hi: Int }

pub type Inst = {
  Char(Int),
  AnyChar,
  Class(Vector<ClassItem>, Bool),
  Split(Int, Int),
  Jmp(Int),
  Save(Int),
  AssertStart,
  AssertEnd,
  Match,
}
```

`boot/stdlib/regexp/parse.tw`:
```tw
/// Internal — unstable. Pattern string → Ast.

use .program.{ClassItem}

pub type Ast = {
  Empty,
  Lit(Int),
  AnyChar,
  Class(Vector<ClassItem>, Bool),
  Concat(Vector<Ast>),
  Alt(Vector<Ast>),
  Star(Ast),
  Plus(Ast),
  Opt(Ast),
  Repeat(Ast, Int, Int),
  Group(Ast, Int),
  NonCap(Ast),
  AssertStart,
  AssertEnd,
}

pub type Parsed = .{ ast: Ast, group_count: Int, ignore_case: Bool }
```

`boot/stdlib/regexp/vm.tw`:
```tw
/// Internal — unstable. Single-pass Pike VM.

use .program.{Inst}

type Thread = .{ pc: Int, slots: Vector<Int> }
```

`boot/stdlib/regexp.tw`:
```tw
/// Regular expressions (pure Twinkle). Public surface: `@std.regexp`.
/// See docs/plans/regexp.md for the design and supported subset.

use .regexp.program.{Inst}
use .regexp.parse
use .regexp.vm

pub type RegexError = .{ pos: Int, message: String }
pub type Regexp = .{ program: Vector<Inst>, group_count: Int, ignore_case: Bool }
pub type Match = .{ start: Int, end: Int, groups: Vector<String?> }

/// Placeholder so the module compiles; replaced in Task 4.
pub fn version() Int {
  1
}
```

- [ ] **Step 2: Create the dev harness in /tmp**

Run:
```bash
mkdir -p /tmp/rxdev
printf 'name = "rxdev"\n' > /tmp/rxdev/twinkle.toml
ln -sf "$PWD/boot/stdlib/regexp.tw" /tmp/rxdev/regexp.tw
ln -sf "$PWD/boot/stdlib/regexp" /tmp/rxdev/regexp
ln -sf "$PWD/boot/tests/assert.tw" /tmp/rxdev/assert.tw
ln -sf "$PWD/boot/tests/runner.tw" /tmp/rxdev/runner.tw
```

Create `/tmp/rxdev/tests.tw`:
```tw
use regexp
use runner

runner.run_all([
  runner.suite("smoke")
    .test("module loads", fn() {
      if regexp.version() == 1 { .Ok({}) } else { .Err("bad version") }
    }),
])
```

- [ ] **Step 3: Run the harness — verify it loads**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: `Ran 1 tests: 1 passed`

If the symlinked `regexp/` directory fails to resolve as a submodule, fall back to copying instead of symlinking the dir: `rm /tmp/rxdev/regexp && cp -r "$PWD/boot/stdlib/regexp" /tmp/rxdev/regexp` and re-copy on each change (add a tiny `sync` shell alias). Prefer the symlink if it works.

- [ ] **Step 4: Format and commit**

```bash
target/twk fmt boot/stdlib/regexp.tw boot/stdlib/regexp/parse.tw boot/stdlib/regexp/program.tw boot/stdlib/regexp/vm.tw
git add boot/stdlib/regexp.tw boot/stdlib/regexp
git commit -m "regexp: module skeleton and shared types"
```

---

## Task 2: Parser — literals and concatenation

Parse a flat string of literal scalars (and `\` escapes of metacharacters) into `Concat([Lit, Lit, …])`. No metacharacters yet beyond escaping.

**Files:**
- Modify: `boot/stdlib/regexp/parse.tw`
- Test: `/tmp/rxdev/tests.tw`

- [ ] **Step 1: Write the failing test**

Add to `/tmp/rxdev/tests.tw` a suite (and include it in `run_all`):
```tw
use regexp.parse as parse        // local path to the submodule for white-box tests

// helper: parse and return the Ast, trapping on error
fn ast_of(p: String) parse.Ast {
  case parse.parse(p) {
    .Ok(parsed) => parsed.ast,
    .Err(e) => error("parse error at ${e.pos}: ${e.message}"),
  }
}
```
```tw
runner.suite("parse literals")
  .test("single char", fn() {
    case ast_of("a") {
      .Concat(items) => if items.len() == 1 {
        case items[0] { .Lit(c) => if c == 97 { .Ok({}) } else { .Err("wrong scalar") }, _ => .Err("not Lit") }
      } else { .Err("expected 1 item") },
      _ => .Err("expected Concat"),
    }
  })
  .test("escaped dot is a literal", fn() {
    case ast_of("\\.") {
      .Concat(items) => case items[0] { .Lit(c) => if c == 46 { .Ok({}) } else { .Err("not '.'") }, _ => .Err("not Lit") },
      _ => .Err("expected Concat"),
    }
  })
```
Note `parse.parse` is the entry the test calls. The local import alias `regexp.parse` reaches the submodule because the harness has `regexp/` at its root.

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: FAIL (`parse` has no function `parse`).

- [ ] **Step 3: Implement the literal/concat parser**

Add to `boot/stdlib/regexp/parse.tw`. Use an explicit cursor record threaded through productions; metacharacters are added in later tasks, so for now everything non-`\` is a literal and `\X` is a literal `X`.

```tw
type Cursor = .{ chars: Vector<Int>, pos: Int, groups: Int, ignore_case: Bool }

pub fn parse(pattern: String) Result<Parsed, RegexErrorLike> {
  chars := decode(pattern)
  cur := Cursor.{ chars, pos: 0, groups: 0, ignore_case: false }
  // (?i) handling added in Task 11; for now no flags.
  case parse_concat(cur) {
    .Ok(pair) => {
      end := pair.second
      if end.pos != end.chars.len() {
        .Err(err(end.pos, "unexpected ${"'"}${scalar_str(end.chars[end.pos])}${"'"}"))
      } else {
        .Ok(Parsed.{ ast: pair.first, group_count: end.groups, ignore_case: end.ignore_case })
      }
    },
    .Err(e) => .Err(e),
  }
}

// decode a String into its code points
fn decode(s: String) Vector<Int> {
  out: Vector<Int> = []
  for ch in s.chars() {
    case ch.code_point_at(0) { .Some(cp) => { out = out.append(cp) }, .None => {} }
  }
  out
}
```

For the threading return type use a small pair. Reuse `@std.tuple`:
```tw
use @std.tuple
use @std.tuple.{Pair}
```
and have productions return `Result<Pair<Ast, Cursor>, RegexError>`.

`RegexErrorLike` is `RegexError` from `regexp.tw`. To avoid a cyclic import (`regexp.tw` already imports `parse`), define `RegexError` in `parse.tw` and have `regexp.tw` re-export it:
- Move `pub type RegexError = .{ pos: Int, message: String }` into `parse.tw`.
- In `regexp.tw`: `use .regexp.parse.{RegexError}` and drop its own definition.

Helpers:
```tw
fn err(pos: Int, msg: String) RegexError { RegexError.{ pos, message: msg } }

fn parse_concat(cur: Cursor) Result<Pair<Ast, Cursor>, RegexError> {
  items: Vector<Ast> = []
  c := cur
  for c.pos < c.chars.len() {
    next := try parse_atom(c)        // parse one atom (literal for now)
    items = items.append(next.first)
    c = next.second
  }
  .Ok(tuple.pair(Ast.Concat(items), c))
}

fn parse_atom(cur: Cursor) Result<Pair<Ast, Cursor>, RegexError> {
  ch := cur.chars[cur.pos]
  if ch == 92 {                      // backslash
    if cur.pos + 1 >= cur.chars.len() { return .Err(err(cur.pos, "trailing backslash")) }
    esc := cur.chars[cur.pos + 1]
    lit := decode_escape(esc)        // \n \t \r \f \v -> control; else the char itself
    .Ok(tuple.pair(Ast.Lit(lit), advance(cur, 2)))
  } else {
    .Ok(tuple.pair(Ast.Lit(ch), advance(cur, 1)))
  }
}

fn advance(cur: Cursor, n: Int) Cursor {
  cur.pos = cur.pos + n
  cur
}

fn decode_escape(c: Int) Int {
  case c {
    110 => 10,   // \n
    116 => 9,    // \t
    114 => 13,   // \r
    102 => 12,   // \f
    118 => 11,   // \v
    _ => c,      // \\ \. \* etc -> the literal char
  }
}
```
`scalar_str` builds a one-character String from a scalar (use `String.from_code_point` if available, else a small helper). Verify the exact prelude name with `grep -n "from_code_point\|from_char\|code_point" boot/prelude/string.tw`.

- [ ] **Step 4: Run to verify it passes**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: PASS.

- [ ] **Step 5: Format and commit**

```bash
target/twk fmt boot/stdlib/regexp/parse.tw boot/stdlib/regexp.tw
git add boot/stdlib/regexp/parse.tw boot/stdlib/regexp.tw
git commit -m "regexp: parse literals and concatenation"
```

---

## Task 3: Compiler — Ast → Program for literals (with whole-match wrap)

**Files:**
- Modify: `boot/stdlib/regexp/program.tw`
- Test: `/tmp/rxdev/tests.tw`

- [ ] **Step 1: Write the failing test**

```tw
use regexp.program as program
```
```tw
runner.suite("compile literals")
  .test("abc compiles to Save0 Char Char Char Save1 Match", fn() {
    insts := program.compile_concat_lits([97, 98, 99])   // temporary helper, removed in Task 4
    // expect: [Save(0), Char(97), Char(98), Char(99), Save(1), Match]
    if insts.len() != 6 { return .Err("len ${insts.len()}") }
    case insts[0] { .Save(s) => if s != 0 { return .Err("slot0") }, _ => return .Err("not Save") }
    case insts[5] { .Match => .Ok({}), _ => .Err("not Match") }
  })
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: FAIL (no `compile_concat_lits`).

- [ ] **Step 3: Implement the compiler entry + Ast walk**

Implement the real compiler now (the temporary helper just calls it). The compiler emits into a growing `Vector<Inst>` and returns the program; group slots are assigned from the `Ast.Group` indices. Wrap the whole body as `Save(0) <body> Save(1) Match`.

```tw
// program.tw
use .parse.{Ast}

pub fn compile(ast: Ast, group_count: Int) Vector<Inst> {
  body := emit(ast, [])
  out: Vector<Inst> = [Inst.Save(0)]
  for i in body { out = out.append(i) }
  out = out.append(Inst.Save(1))
  out.append(Inst.Match)
}

// emit appends the instructions for `ast` to `acc`, returning the new acc.
fn emit(ast: Ast, acc: Vector<Inst>) Vector<Inst> {
  case ast {
    .Empty => acc,
    .Lit(c) => acc.append(Inst.Char(c)),
    .Concat(items) => {
      a := acc
      for it in items { a = emit(it, a) }
      a
    },
    // other cases added in later tasks
    _ => acc,
  }
}

// temporary helper for Task 3's test; delete in Task 4.
pub fn compile_concat_lits(cs: Vector<Int>) Vector<Inst> {
  items: Vector<Ast> = []
  for c in cs { items = items.append(Ast.Lit(c)) }
  compile(Ast.Concat(items), 0)
}
```
Note the circular dependency: `program.tw` now imports `parse.{Ast}`, and `parse.tw` imports `program.{ClassItem}`. Twinkle allows this only if there is no import cycle. If the resolver rejects the cycle, move `ClassItem` and the two enums (`Inst`, `Ast`) into a single `boot/stdlib/regexp/types.tw` that both import, and keep `parse.tw`/`program.tw` for behavior only. **Verify which case you are in by running the harness; restructure to `types.tw` if you get a cyclic-import error.**

- [ ] **Step 4: Run to verify it passes**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: PASS.

- [ ] **Step 5: Format and commit**

```bash
target/twk fmt boot/stdlib/regexp/program.tw
git add boot/stdlib/regexp
git commit -m "regexp: compile literals with whole-match wrap"
```

---

## Task 4: VM + public API — literal matching end to end

Wire `compile`/`must`/`test`/`find` and implement the single-pass VM (`find_from`) for the instructions that exist so far (`Char`, `Save`, `Match`, plus `Jmp`/`Split`/anchors for later — implement them all in the VM now so later tasks only add parser/compiler cases).

**Files:**
- Modify: `boot/stdlib/regexp/vm.tw`, `boot/stdlib/regexp.tw`
- Test: `/tmp/rxdev/tests.tw`

- [ ] **Step 1: Write the failing tests**

```tw
runner.suite("find literals")
  .test("find substring", fn() {
    m := regexp.must("abc").find("xxabcyy")
    case m { .Some(mm) => if mm.text() == "abc" and mm.start == 2 { .Ok({}) } else { .Err("got ${mm.text()} @${mm.start}") }, .None => .Err("no match") }
  })
  .test("test true/false", fn() {
    if regexp.must("abc").test("zabcz") and !regexp.must("abc").test("abx") { .Ok({}) } else { .Err("bad test") }
  })
  .test("no match", fn() {
    case regexp.must("abc").find("ab") { .None => .Ok({}), .Some(_) => .Err("unexpected match") }
  })
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: FAIL.

- [ ] **Step 3: Implement the VM**

`boot/stdlib/regexp/vm.tw` — the full single-pass Pike VM. This is the highest-risk code; follow the design in `docs/plans/regexp.md` exactly (seed at lowest priority while no match; record-and-continue with lower-priority cut; absolute anchors; per-generation `pc` dedup in `add_thread`).

```tw
use .program.{Inst}

type Thread = .{ pc: Int, slots: Vector<Int> }
type Gen = .{ threads: Vector<Thread>, seen: Vector<Bool> }   // seen indexed by pc

fn fresh_slots(nslots: Int) Vector<Int> {
  s: Vector<Int> = []
  for _ in range(nslots) { s = s.append(-1) }
  s
}

// add_thread: ε-closure with per-generation pc dedup (keep first/highest priority).
fn add_thread(prog: Vector<Inst>, gen: Gen, pc: Int, slots: Vector<Int>, pos: Int, len: Int) Gen {
  if gen.seen[pc] { return gen }
  g := gen
  g.seen = g.seen.set(pc, true)

  case prog[pc] {
    .Jmp(x) => add_thread(prog, g, x, slots, pos, len),
    .Split(x, y) => {
      g = add_thread(prog, g, x, slots, pos, len)
      add_thread(prog, g, y, slots, pos, len)
    },
    .Save(slot) => add_thread(prog, g, pc + 1, slots.set(slot, pos), pos, len),
    .AssertStart => if pos == 0 { add_thread(prog, g, pc + 1, slots, pos, len) } else { g },
    .AssertEnd => if pos == len { add_thread(prog, g, pc + 1, slots, pos, len) } else { g },
    _ => {
      // Char / AnyChar / Class / Match: a runnable/terminal state, keep it
      g.threads = g.threads.append(Thread.{ pc, slots })
      g
    },
  }
}

fn new_gen(prog_len: Int) Gen {
  seen: Vector<Bool> = []
  for _ in range(prog_len) { seen = seen.append(false) }
  Gen.{ threads: [], seen }
}

// Does this scalar match the consuming instruction at pc? (ignore_case handled in Task 11)
fn inst_matches(prog: Vector<Inst>, pc: Int, ch: Int, ignore_case: Bool) Bool {
  case prog[pc] {
    .Char(c) => scalar_eq(c, ch, ignore_case),
    .AnyChar => ch != 10,
    .Class(items, negated) => class_match(items, ch, negated, ignore_case),
    _ => false,
  }
}

// find the leftmost match at or after `start`. ignore_case threaded for Task 11.
pub fn find_from(prog: Vector<Inst>, chars: Vector<Int>, start: Int, ignore_case: Bool, nslots: Int) Vector<Int>? {
  len := chars.len()
  clist := new_gen(prog.len())
  matched: Vector<Int>? = .None
  pos := start

  for true {
    // seed a fresh start thread at lowest priority, only while no match yet
    case matched {
      .None => { clist = add_thread(prog, clist, 0, fresh_slots(nslots), pos, len) },
      .Some(_) => {},
    }

    nlist := new_gen(prog.len())
    ch := if pos < len { chars[pos] } else { -1 }

    i := 0
    stop := false
    for i < clist.threads.len() and !stop {
      t := clist.threads[i]
      case prog[t.pc] {
        .Match => {
          matched = .Some(t.slots)
          stop = true          // cut lower-priority threads in this generation
        },
        _ => {
          if pos < len and inst_matches(prog, t.pc, ch, ignore_case) {
            nlist = add_thread(prog, nlist, t.pc + 1, t.slots, pos + 1, len)
          }
        },
      }
      i = i + 1
    }

    if pos >= len { return matched }
    clist = nlist
    pos = pos + 1
  }

  matched
}

fn scalar_eq(a: Int, b: Int, ignore_case: Bool) Bool {
  if a == b { true } else if ignore_case { fold(a) == fold(b) } else { false }
}

fn fold(c: Int) Int { if c >= 65 and c <= 90 { c + 32 } else { c } }   // ASCII upper -> lower

fn class_match(items: Vector<ClassItem>, ch: Int, negated: Bool, ignore_case: Bool) Bool {
  hit := false
  for it in items {
    if (ch >= it.lo and ch <= it.hi) or (ignore_case and in_folded(it, ch)) { hit = true }
  }
  if negated { !hit } else { hit }
}

fn in_folded(it: ClassItem, ch: Int) Bool {
  f := fold(ch)
  u := if ch >= 97 and ch <= 122 { ch - 32 } else { ch }
  (f >= it.lo and f <= it.hi) or (u >= it.lo and u <= it.hi)
}
```
Import `ClassItem` into `vm.tw` (`use .program.{Inst, ClassItem}`). `find_from` returns the winning **slots** (`Vector<Int>?`); materializing to a `Match` happens in `regexp.tw`.

- [ ] **Step 4: Implement the public API in `regexp.tw`**

```tw
use .regexp.program
use .regexp.parse
use .regexp.vm

pub fn compile(pattern: String) Result<Regexp, RegexError> {
  case parse.parse(pattern) {
    .Ok(p) => {
      prog := program.compile(p.ast, p.group_count)
      .Ok(Regexp.{ program: prog, group_count: p.group_count, ignore_case: p.ignore_case })
    },
    .Err(e) => .Err(e),
  }
}

pub fn must(pattern: String) Regexp {
  case compile(pattern) {
    .Ok(re) => re,
    .Err(e) => error("regexp:${e.pos}: ${e.message}"),
  }
}

pub fn test(re: Regexp, s: String) Bool {
  case find(re, s) { .Some(_) => true, .None => false }
}

pub fn find(re: Regexp, s: String) Match? {
  chars := decode(s)
  nslots := 2 * (re.group_count + 1)
  case vm.find_from(re.program, chars, 0, re.ignore_case, nslots) {
    .Some(slots) => .Some(materialize(chars, slots, re.group_count)),
    .None => .None,
  }
}

fn materialize(chars: Vector<Int>, slots: Vector<Int>, group_count: Int) Match {
  groups: Vector<String?> = []
  for k in range(group_count + 1) {
    lo := slots[2 * k]
    hi := slots[2 * k + 1]
    if lo >= 0 and hi >= 0 { groups = groups.append(.Some(slice_scalars(chars, lo, hi))) }
    else { groups = groups.append(.None) }
  }
  Match.{ start: slots[0], end: slots[1], groups }
}

pub fn group(m: Match, i: Int) String? {
  if i < 0 or i >= m.groups.len() { .None } else { m.groups[i] }
}

pub fn text(m: Match) String {
  case group(m, 0) { .Some(s) => s, .None => "" }
}
```
`decode` (String → `Vector<Int>`) and `slice_scalars(chars, lo, hi)` (build a String from `chars[lo..hi]` via `String.from_code_point` concatenation) live in `regexp.tw`. Reuse the `decode` from `parse.tw` by exposing it `pub` and importing, to keep one definition (DRY).

Delete `program.compile_concat_lits` (Task 3's temporary helper) and its test now that real `find` exists.

- [ ] **Step 5: Run to verify it passes**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: PASS (literal find/test/no-match).

- [ ] **Step 6: Format and commit**

```bash
target/twk fmt boot/stdlib/regexp.tw boot/stdlib/regexp/vm.tw boot/stdlib/regexp/program.tw
git add boot/stdlib/regexp
git commit -m "regexp: single-pass Pike VM and public find/test/compile"
```

---

## Task 5: `.` and character classes

Add `.` (AnyChar) and `[...]` / `[^...]` with ranges and the predefined `\d \w \s` (and standalone `\D \W \S`).

**Files:**
- Modify: `boot/stdlib/regexp/parse.tw`, `boot/stdlib/regexp/program.tw`
- Test: `/tmp/rxdev/tests.tw`

- [ ] **Step 1: Write the failing tests**

```tw
runner.suite("classes")
  .test("dot matches one non-newline", fn() {
    if regexp.must("a.c").test("axc") and !regexp.must("a.c").test("a\nc") { .Ok({}) } else { .Err("dot") }
  })
  .test("digit class", fn() {
    case regexp.must("[0-9]").find("xx7yy") { .Some(m) => if m.text() == "7" { .Ok({}) } else { .Err(m.text()) }, .None => .Err("no") }
  })
  .test("negated class", fn() {
    case regexp.must("[^abc]").find("abQ") { .Some(m) => if m.text() == "Q" { .Ok({}) } else { .Err(m.text()) }, .None => .Err("no") }
  })
  .test("predefined \\d", fn() {
    case regexp.must("\\d").find("a5") { .Some(m) => if m.text() == "5" { .Ok({}) } else { .Err(m.text()) }, .None => .Err("no") }
  })
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: FAIL.

- [ ] **Step 3: Implement parser cases for `.`, `[...]`, `\d \w \s \D \W \S`**

In `parse_atom`, before the literal fallback, handle:
- `ch == 46` (`.`) → `Ast.AnyChar`, advance 1.
- `ch == 91` (`[`) → parse a class (see below).
- `ch == 92` (`\`) with a class escape (`d w s D W S`) → `Ast.Class(ranges, negated)`.

Predefined range tables (as functions returning `Vector<ClassItem>`):
```tw
fn digit_ranges() Vector<ClassItem> { [ClassItem.{ lo: 48, hi: 57 }] }
fn word_ranges() Vector<ClassItem> {
  [ClassItem.{ lo: 48, hi: 57 }, ClassItem.{ lo: 65, hi: 90 }, ClassItem.{ lo: 97, hi: 122 }, ClassItem.{ lo: 95, hi: 95 }]
}
fn space_ranges() Vector<ClassItem> {
  [ClassItem.{ lo: 9, hi: 13 }, ClassItem.{ lo: 32, hi: 32 }]
}
```
`\d`→`Class(digit_ranges(), false)`, `\D`→`Class(digit_ranges(), true)`, similarly `w`/`s`.

Class parser `[...]`: collect items until `]`. Support a leading `^` (negated), ranges `a-z` (two scalars with `-` between), single scalars, escapes (`\]`, `\\`, control escapes), and the predefined `\d \w \s` (append their ranges; `\D \W \S` inside a class is a v1 parse error — message "negated class escape not allowed inside []"). A `-` at the start/end of the class or not between two scalars is a literal `-`. Unterminated class (`[` with no `]`) → error at the `[` position.

```tw
fn parse_class(cur: Cursor) Result<Pair<Ast, Cursor>, RegexError> {
  open := cur.pos
  c := advance(cur, 1)             // consume '['
  negated := false
  if c.pos < c.chars.len() and c.chars[c.pos] == 94 { negated = true; c = advance(c, 1) }
  items: Vector<ClassItem> = []
  for true {
    if c.pos >= c.chars.len() { return .Err(err(open, "unterminated character class")) }
    if c.chars[c.pos] == 93 { return .Ok(tuple.pair(Ast.Class(items, negated), advance(c, 1))) }
    // parse one class element (scalar or predefined set), then optional range '-'
    elem := try class_scalar(c)    // returns Result<Pair<Int? or ranges, Cursor>>; see note
    // ... append a ClassItem (single) or merge predefined ranges, handling 'lo-hi'
    c = elem.second
    items = items.append(elem.first)   // sketch — handle ranges as described
  }
  .Err(err(open, "unterminated character class"))
}
```
Implement `class_scalar`/range handling to pass the tests; keep single scalars as `ClassItem.{ lo: x, hi: x }` and ranges as `ClassItem.{ lo, hi }`.

- [ ] **Step 4: Add compiler cases**

In `emit`:
```tw
.AnyChar => acc.append(Inst.AnyChar),
.Class(items, negated) => acc.append(Inst.Class(items, negated)),
```
(The VM already handles `AnyChar`/`Class` from Task 4.)

- [ ] **Step 5: Run to verify it passes**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: PASS.

- [ ] **Step 6: Format and commit**

```bash
target/twk fmt boot/stdlib/regexp/parse.tw boot/stdlib/regexp/program.tw
git add boot/stdlib/regexp
git commit -m "regexp: dot and character classes"
```

---

## Task 6: Quantifiers `* + ? {m,n}` (greedy)

**Files:**
- Modify: `boot/stdlib/regexp/parse.tw`, `boot/stdlib/regexp/program.tw`
- Test: `/tmp/rxdev/tests.tw`

- [ ] **Step 1: Write the failing tests (includes the spec's greedy guards)**

```tw
runner.suite("quantifiers")
  .test("a+ greedy", fn() { case regexp.must("a+").find("aaa") { .Some(m) => eq(m.text(), "aaa"), .None => .Err("no") } })
  .test("a* greedy", fn() { case regexp.must("a*").find("aaa") { .Some(m) => eq(m.text(), "aaa"), .None => .Err("no") } })
  .test("a? optional", fn() { case regexp.must("ab?c").find("ac") { .Some(m) => eq(m.text(), "ac"), .None => .Err("no") } })
  .test("{2,3} bounded", fn() { case regexp.must("a{2,3}").find("aaaa") { .Some(m) => eq(m.text(), "aaa"), .None => .Err("no") } })
  .test("{2} exact", fn() { case regexp.must("a{2}").find("aaaa") { .Some(m) => eq(m.text(), "aa"), .None => .Err("no") } })
```
Add a helper `fn eq(a: String, b: String) Result<Void, String> { if a == b { .Ok({}) } else { .Err("got ${a}, want ${b}") } }`.

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: FAIL.

- [ ] **Step 3: Implement quantifier parsing**

After `parse_atom` returns an atom in `parse_concat`, peek for a postfix quantifier and wrap:
- `*` → `Ast.Star(atom)`
- `+` → `Ast.Plus(atom)`
- `?` → `Ast.Opt(atom)`
- `{m}` / `{m,}` / `{m,n}` → `Ast.Repeat(atom, m, n)` (n = -1 for unbounded; parse the digits; malformed `{` → treat `{` as a literal, matching common regex leniency, OR error — pick error with message "bad repetition" for v1 and a test).

A quantifier with no preceding atom (e.g. leading `*`) → error "nothing to repeat" at that pos.

- [ ] **Step 4: Implement greedy lowering in `emit`**

Match the invariants in `docs/plans/regexp.md` exactly. Emit relative jumps by tracking current length. Helper to patch `Split`/`Jmp` targets after emitting sub-parts:

```tw
.Star(inner) => {
  // L: Split(body, done) ; body ; Jmp(L) ; done:
  l := acc.len()
  acc1 := acc.append(Inst.Split(l + 1, 0))     // placeholder done target
  body := emit(inner, acc1)
  body = body.append(Inst.Jmp(l))
  done := body.len()
  body.set(l, Inst.Split(l + 1, done))
},
.Plus(inner) => {
  // body ; L: Split(body, done)
  body_start := acc.len()
  body := emit(inner, acc)
  l := body.len()
  body = body.append(Inst.Split(body_start, l + 1))
  body
},
.Opt(inner) => {
  // Split(body, done) ; body ; done:
  l := acc.len()
  acc1 := acc.append(Inst.Split(l + 1, 0))
  body := emit(inner, acc1)
  done := body.len()
  body.set(l, Inst.Split(l + 1, done))
},
.Repeat(inner, m, n) => emit(expand_repeat(inner, m, n), acc),
```
`expand_repeat` builds an `Ast.Concat` of `m` mandatory copies followed by, for bounded `n`, `(n - m)` `Opt` copies, or for unbounded (`n == -1`), one trailing `Star` (if `m == 0`) / the last copy as `Plus` semantics. Simplest correct expansion: `a{m,n}` → `m` copies of `inner` then `(n-m)` copies of `Opt(inner)`; `a{m,}` → `m` copies then `Star(inner)`; `a{m}` → `m` copies. Be careful that the greedy bias of `Opt`/`Star` is preserved (it is, since they lower greedily).

Confirm the `done`-target patching is correct by the tests; the `Split` first operand must be the body (priority), second the exit.

- [ ] **Step 5: Run to verify it passes**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: PASS (greedy + bounded).

- [ ] **Step 6: Format and commit**

```bash
target/twk fmt boot/stdlib/regexp/parse.tw boot/stdlib/regexp/program.tw
git add boot/stdlib/regexp
git commit -m "regexp: greedy quantifiers"
```

---

## Task 7: Capturing groups + `group(i)`

**Files:**
- Modify: `boot/stdlib/regexp/parse.tw`, `boot/stdlib/regexp/program.tw`
- Test: `/tmp/rxdev/tests.tw`

- [ ] **Step 1: Write the failing tests**

```tw
runner.suite("groups")
  .test("two captures", fn() {
    case regexp.must("(\\d+)-(\\d+)").find("ab 12-34 cd") {
      .Some(m) => {
        try eq(m.text(), "12-34")
        try eq(opt_or(m.group(1), "?"), "12")
        try eq(opt_or(m.group(2), "?"), "34")
        .Ok({})
      },
      .None => .Err("no"),
    }
  })
  .test("non-capturing group", fn() {
    case regexp.must("(?:ab)+").find("ababx") { .Some(m) => eq(m.text(), "abab"), .None => .Err("no") }
  })
  .test("optional group didn't participate -> None", fn() {
    case regexp.must("a(b)?c").find("ac") {
      .Some(m) => case m.group(1) { .None => .Ok({}), .Some(s) => .Err("got ${s}") },
      .None => .Err("no"),
    }
  })
```
Helper `fn opt_or(o: String?, d: String) String { case o { .Some(s) => s, .None => d } }`.

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: FAIL.

- [ ] **Step 3: Implement group parsing**

In `parse_atom`, `ch == 40` (`(`):
- `(?:` → parse inner `alt`, expect `)`, return `Ast.NonCap(inner)`.
- `(?i)` is handled in Task 11 (for now, `(?` followed by anything other than `:` → error "unsupported group" — Task 11 relaxes this for `i`).
- otherwise → capturing: assign the next group index (`cur.groups + 1`), **increment the cursor's group counter**, parse inner `alt`, expect `)`, return `Ast.Group(inner, idx)`. (Assign the index *before* parsing inner so nested groups number in opening-paren order.)
- unbalanced `)` or missing `)` → error.

Groups require a real `alt`/`concat` recursion that stops at `)`; thread a "depth"/"in-group" flag or have `parse_concat` stop at `)` and `|`. Refactor `parse_concat` to stop at `)` and `|`, and add `parse_alt` (Task 8) — for now `parse_atom` for a group calls `parse_concat` and expects `)`.

- [ ] **Step 4: Implement compiler cases**

```tw
.Group(inner, idx) => {
  a := acc.append(Inst.Save(2 * idx))
  a = emit(inner, a)
  a.append(Inst.Save(2 * idx + 1))
},
.NonCap(inner) => emit(inner, acc),
```
The VM already records `Save` into slots; `materialize` already reads slot pairs per group. Ensure `nslots = 2 * (group_count + 1)` covers the max index.

- [ ] **Step 5: Run to verify it passes**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: PASS.

- [ ] **Step 6: Format and commit**

```bash
target/twk fmt boot/stdlib/regexp/parse.tw boot/stdlib/regexp/program.tw
git add boot/stdlib/regexp
git commit -m "regexp: capturing and non-capturing groups"
```

---

## Task 8: Alternation `|`

**Files:**
- Modify: `boot/stdlib/regexp/parse.tw`, `boot/stdlib/regexp/program.tw`
- Test: `/tmp/rxdev/tests.tw`

- [ ] **Step 1: Write the failing tests (the spec's alternation guards)**

```tw
runner.suite("alternation")
  .test("a|aa first branch", fn() { case regexp.must("a|aa").find("aa") { .Some(m) => eq(m.text(), "a"), .None => .Err("no") } })
  .test("aa|a first branch", fn() { case regexp.must("aa|a").find("aa") { .Some(m) => eq(m.text(), "aa"), .None => .Err("no") } })
  .test("(a|aa)+ spans input", fn() { case regexp.must("(a|aa)+").find("aa") { .Some(m) => eq(m.text(), "aa"), .None => .Err("no") } })
  .test("color words", fn() { case regexp.must("red|green|blue").find("xbluey") { .Some(m) => eq(m.text(), "blue"), .None => .Err("no") } })
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: FAIL.

- [ ] **Step 3: Implement `parse_alt`**

Introduce the top of the grammar: `parse_alt` parses one `parse_concat`, then while the next char is `|`, consume it and parse another `concat`, collecting branches. One branch → return it directly; multiple → `Ast.Alt(branches)`. Make `parse` and group-parsing call `parse_alt` (not `parse_concat`) as the entry, and make `parse_concat` stop at `|` and `)`.

- [ ] **Step 4: Implement alternation lowering**

```tw
.Alt(branches) => emit_alt(branches, acc),
```
For branches `[b0, b1, …, bk]`, chain `Split`s so `b0` has highest priority:
```
Split(B0, L1) ; B0 ; Jmp(End)
L1: Split(B1, L2) ; B1 ; Jmp(End)
…
Lk: Bk
End:
```
Implement by recursion/iteration, emitting each `Split` with a placeholder exit and patching the `Jmp(End)` targets once `End` (final length) is known. The first `Split` operand is the branch body (priority), preserving first-branch-wins.

- [ ] **Step 5: Run to verify it passes**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: PASS — and re-run the whole suite to confirm quantifier/group tests still pass.

- [ ] **Step 6: Format and commit**

```bash
target/twk fmt boot/stdlib/regexp/parse.tw boot/stdlib/regexp/program.tw
git add boot/stdlib/regexp
git commit -m "regexp: alternation with first-branch priority"
```

---

## Task 9: Anchors `^` and `$`

**Files:**
- Modify: `boot/stdlib/regexp/parse.tw`, `boot/stdlib/regexp/program.tw`
- Test: `/tmp/rxdev/tests.tw`

- [ ] **Step 1: Write the failing tests**

```tw
runner.suite("anchors")
  .test("^ anchors at start", fn() {
    if regexp.must("^ab").test("abc") and !regexp.must("^ab").test("xab") { .Ok({}) } else { .Err("^") }
  })
  .test("$ anchors at end", fn() {
    if regexp.must("bc$").test("abc") and !regexp.must("bc$").test("abcd") { .Ok({}) } else { .Err("$") }
  })
  .test("^...$ full match", fn() {
    if regexp.must("^a.c$").test("axc") and !regexp.must("^a.c$").test("xaxc") { .Ok({}) } else { .Err("both") }
  })
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: FAIL.

- [ ] **Step 3: Implement parser + compiler cases**

Parser: `^` → `Ast.AssertStart`, `$` → `Ast.AssertEnd` (as atoms; they take no quantifier — a quantifier on an anchor is an error "nothing to repeat" or ignore; pick error). Compiler:
```tw
.AssertStart => acc.append(Inst.AssertStart),
.AssertEnd => acc.append(Inst.AssertEnd),
```
The VM already handles `AssertStart`/`AssertEnd` as **absolute** (`pos == 0` / `pos == len`) in `add_thread` from Task 4 — verify that code is present and correct; this is the easiest bug to introduce.

- [ ] **Step 4: Run to verify it passes**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: PASS.

- [ ] **Step 5: Format and commit**

```bash
target/twk fmt boot/stdlib/regexp/parse.tw boot/stdlib/regexp/program.tw
git add boot/stdlib/regexp
git commit -m "regexp: absolute ^ and $ anchors"
```

---

## Task 10: `find_all` (Iterator) + empty-match scanning

**Files:**
- Modify: `boot/stdlib/regexp.tw`
- Test: `/tmp/rxdev/tests.tw`

- [ ] **Step 1: Write the failing tests**

```tw
runner.suite("find_all")
  .test("all numbers", fn() {
    nums: Vector<String> = []
    for m in regexp.must("\\d+").find_all("a12 b3 c456") { nums = nums.append(m.text()) }
    if nums.len() == 3 and nums[0] == "12" and nums[1] == "3" and nums[2] == "456" { .Ok({}) } else { .Err("got ${nums.len()}") }
  })
  .test("empty pattern over empty string yields one", fn() {
    n := 0
    for _ in regexp.must("a*").find_all("") { n = n + 1 }
    if n == 1 { .Ok({}) } else { .Err("count ${n}") }
  })
  .test("a* over bb yields empties without looping", fn() {
    parts: Vector<String> = []
    for m in regexp.must("a*").find_all("bb") { parts = parts.append(m.text()) }
    // empty at 0, step over 'b', empty at 1, step over 'b', empty at 2 -> 3 matches
    if parts.len() == 3 { .Ok({}) } else { .Err("count ${parts.len()}") }
  })
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: FAIL.

- [ ] **Step 3: Implement `find_all` with `Iterator.unfold`**

```tw
type ScanState = .{ re: Regexp, chars: Vector<Int>, pos: Int }

pub fn find_all(re: Regexp, s: String) Iterator<Match> {
  chars := decode(s)
  Iterator.unfold(ScanState.{ re, chars, pos: 0 }, fn(st: ScanState) {
    if st.pos > st.chars.len() {
      UnfoldStep.Done
    } else {
      nslots := 2 * (st.re.group_count + 1)
      case vm.find_from(st.re.program, st.chars, st.pos, st.re.ignore_case, nslots) {
        .Some(slots) => {
          m := materialize(st.chars, slots, st.re.group_count)
          next := if m.end > m.start { m.end } else { m.end + 1 }
          UnfoldStep.Yield(m, ScanState.{ re: st.re, chars: st.chars, pos: next })
        },
        .None => UnfoldStep.Done,
      }
    }
  })
}
```
Confirm `find_from` finds the leftmost match **at or after** `st.pos` (Task 4 seeds the first thread at `pos = start`; verify no thread is seeded before `start`). The `pos > len` guard plus the empty-match `end + 1` advance gives the termination invariant.

- [ ] **Step 4: Run to verify it passes**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: PASS (including no infinite loop on `a*`).

- [ ] **Step 5: Format and commit**

```bash
target/twk fmt boot/stdlib/regexp.tw
git add boot/stdlib/regexp.tw
git commit -m "regexp: find_all iterator with empty-match scanning"
```

---

## Task 11: `(?i)` case-insensitive (including classes)

**Files:**
- Modify: `boot/stdlib/regexp/parse.tw`
- Test: `/tmp/rxdev/tests.tw`

- [ ] **Step 1: Write the failing tests**

```tw
runner.suite("ignore case")
  .test("(?i) literal", fn() {
    if regexp.must("(?i)abc").test("ABC") and regexp.must("(?i)abc").test("AbC") { .Ok({}) } else { .Err("lit") }
  })
  .test("(?i) class range", fn() {
    case regexp.must("(?i)[a-z]+").find("XYz") { .Some(m) => eq(m.text(), "XYz"), .None => .Err("no") }
  })
  .test("(?i) only at position 0", fn() {
    case regexp.compile("ab(?i)c") { .Err(_) => .Ok({}), .Ok(_) => .Err("should reject mid-pattern (?i)") }
  })
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: FAIL (`(?i)` currently unsupported / not detected).

- [ ] **Step 3: Implement `(?i)` detection in `parse`**

At the very start of `parse`, after `decode`, check whether the first four scalars are `( ? i )`. If so, set `ignore_case = true` and advance the cursor past them. Anywhere else, the existing group parser already rejects `(?` + non-`:` (Task 7) — keep that so `ab(?i)c` errors. Thread `ignore_case` into `Parsed`. The VM already folds via the `ignore_case` argument (`scalar_eq` and `class_match`/`in_folded` from Task 4), so no compiler/VM change is needed — only the parser flag.

- [ ] **Step 4: Run to verify it passes**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: PASS.

- [ ] **Step 5: Format and commit**

```bash
target/twk fmt boot/stdlib/regexp/parse.tw
git add boot/stdlib/regexp/parse.tw
git commit -m "regexp: leading (?i) case-insensitive flag"
```

---

## Task 12: `replace` / `replace_all` + `$` expansion

**Files:**
- Modify: `boot/stdlib/regexp.tw`
- Test: `/tmp/rxdev/tests.tw`

- [ ] **Step 1: Write the failing tests (includes the spec's contract examples)**

```tw
runner.suite("replace")
  .test("replace first", fn() { eq(regexp.must("\\d+").replace("a1b2", "#"), "a#b2") })
  .test("replace_all", fn() { eq(regexp.must("\\d+").replace_all("a1b2", "#"), "a#b#") })
  .test("group ref", fn() { eq(regexp.must("(\\w+)@(\\w+)").replace_all("x@y", "$2.$1"), "y.x") })
  .test("dollar literal", fn() { eq(regexp.must("a").replace_all("a", "$$"), "$") })
  .test("empty match a* on empty", fn() { eq(regexp.must("a*").replace_all("", "X"), "X") })
  .test("empty match a* on b", fn() { eq(regexp.must("a*").replace_all("b", "X"), "XbX") })
  .test("a+ on baab", fn() { eq(regexp.must("a+").replace_all("baab", "X"), "bXb") })
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: FAIL.

- [ ] **Step 3: Implement `replace`/`replace_all` + `$` expansion**

```tw
pub fn replace(re: Regexp, s: String, repl: String) String { replace_n(re, s, repl, 1) }
pub fn replace_all(re: Regexp, s: String, repl: String) String { replace_n(re, s, repl, -1) }

fn replace_n(re: Regexp, s: String, repl: String, limit: Int) String {
  chars := decode(s)
  len := chars.len()
  out := ""
  last := 0
  pos := 0
  count := 0
  nslots := 2 * (re.group_count + 1)

  for pos <= len {
    if limit >= 0 and count >= limit { break }
    case vm.find_from(re.program, chars, pos, re.ignore_case, nslots) {
      .Some(slots) => {
        m := materialize(chars, slots, re.group_count)
        out = out.concat(slice_scalars(chars, last, m.start))   // copy gap before match
        out = out.concat(expand(repl, m))
        count = count + 1
        if m.end > m.start {
          last = m.end
          pos = m.end
        } else {
          // empty match: copy the scalar we step over so it isn't dropped
          if m.end < len { out = out.concat(slice_scalars(chars, m.end, m.end + 1)) }
          last = m.end + 1
          pos = m.end + 1
        }
      },
      .None => { break },
    }
  }
  if last < len { out = out.concat(slice_scalars(chars, last, len)) }
  out
}

fn expand(repl: String, m: Match) String {
  rs := decode(repl)
  out := ""
  i := 0
  for i < rs.len() {
    if rs[i] == 36 {                       // '$'
      if i + 1 < rs.len() and rs[i + 1] == 36 { out = out.concat("$"); i = i + 2 }
      else if i + 1 < rs.len() and rs[i + 1] >= 48 and rs[i + 1] <= 57 {
        g := rs[i + 1] - 48
        out = out.concat(opt_or(group(m, g), ""))
        i = i + 2
      } else { out = out.concat("$"); i = i + 1 }     // lone '$'
    } else {
      out = out.concat(slice_scalars(rs, i, i + 1))
      i = i + 1
    }
  }
  out
}

fn opt_or(o: String?, d: String) String { case o { .Some(s) => s, .None => d } }
```
Verify `slice_scalars` handles `lo == hi` (empty) and the tail copy guards `last <= len`. The empty-match contract examples (`"" → "X"`, `"b" → "XbX"`) pin the edge cases.

- [ ] **Step 4: Run to verify it passes**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: PASS (all replacement contracts).

- [ ] **Step 5: Format and commit**

```bash
target/twk fmt boot/stdlib/regexp.tw
git add boot/stdlib/regexp.tw
git commit -m "regexp: replace and replace_all with $ expansion"
```

---

## Task 13: Parser error cases (`RegexError`)

Harden the parser so every malformed pattern yields a `RegexError` at the right position instead of trapping or mis-parsing.

**Files:**
- Modify: `boot/stdlib/regexp/parse.tw`
- Test: `/tmp/rxdev/tests.tw`

- [ ] **Step 1: Write the failing tests**

```tw
fn err_at(p: String, want_pos: Int) Result<Void, String> {
  case regexp.compile(p) {
    .Err(e) => if e.pos == want_pos { .Ok({}) } else { .Err("pos ${e.pos}, want ${want_pos}") },
    .Ok(_) => .Err("expected error for ${p}"),
  }
}
```
```tw
runner.suite("errors")
  .test("unbalanced open paren", fn() { err_at("a(b", 1) })
  .test("unbalanced close paren", fn() { err_at("ab)", 2) })
  .test("unterminated class", fn() { err_at("a[bc", 1) })
  .test("trailing backslash", fn() { err_at("ab\\", 2) })
  .test("nothing to repeat", fn() { err_at("*ab", 0) })
  .test("bad repetition", fn() { err_at("a{2,x}", 1) })
```

- [ ] **Step 2: Run to verify it fails**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: FAIL on whichever cases aren't yet handled.

- [ ] **Step 3: Implement / confirm each error path**

Ensure: an unconsumed `)` after the top-level `parse_alt` errors at its position; a `(` whose group never closes errors at the `(`; a `[` that never closes errors at the `[`; a trailing `\` errors at the `\`; a quantifier (`* + ? {`) with no preceding atom errors "nothing to repeat" at that pos; a malformed `{m,n}` errors "bad repetition" at the `{`. Adjust the exact `pos` values in the tests to match your implementation if they differ — the point is a correct, stable position, not a specific number; update the test to the position you emit.

- [ ] **Step 4: Run to verify it passes**

Run: `target/twk run /tmp/rxdev/tests.tw`
Expected: PASS.

- [ ] **Step 5: Format and commit**

```bash
target/twk fmt boot/stdlib/regexp/parse.tw
git add boot/stdlib/regexp/parse.tw
git commit -m "regexp: parser error positions"
```

---

## Task 14: Wire `@std.regexp` — boot suite, embed, bundle, docs

The files already live at `boot/stdlib/regexp*`, so they become `@std.regexp` once embedded. This task adds the boot-suite, regenerates the embedded stdlib, rebuilds the CLI (slow), and documents the module.

**Files:**
- Create: `boot/tests/suites/stdlib_regexp_suite.tw`
- Modify: `boot/tests/main.tw`
- Modify: `docs/API.md`

- [ ] **Step 1: Write the boot test suite (port the harness suites)**

Create `boot/tests/suites/stdlib_regexp_suite.tw`, importing the real module as `@std.regexp` and the harness as `tests.assert`/`tests.runner`. Port the most important cases from the dev harness (greedy guards, alternation guards, classes, groups, anchors, find_all empty-match, `(?i)`, replace contracts). Shape:
```tw
use @std.regexp
use @std.regexp.{Regexp, Match}

use tests.assert
use tests.runner

pub fn suite() runner.Suite {
  runner.suite("stdlib regexp")
    .test("a+ greedy", fn() {
      try assert.equal(regexp.must("a+").find("aaa").map_text(), "aaa")  // inline the text() extraction
      .Ok({})
    })
    // … port the rest …
}
```
For each `.test`, extract `find(...)`’s text with a small local helper or `case`, and assert with `assert.equal`. Keep assertions exactly mirroring the dev-harness tests so parity is obvious.

- [ ] **Step 2: Wire the suite into `boot/tests/main.tw`**

Add the import line alongside the other `use .suites.stdlib_*` lines:
```tw
use .suites.stdlib_regexp_suite
```
and add to the suite list (near `stdlib_path_suite.suite(),`):
```tw
  stdlib_regexp_suite.suite(),
```

- [ ] **Step 3: Regenerate the embedded stdlib and rebuild the CLI**

Run:
```bash
make bundle-cli
```
Expected: completes without error and rebuilds `target/twk`. (`core_lib.tw` is regenerated automatically and is gitignored — do not commit it.)

- [ ] **Step 4: Run the boot test suite**

Run: `target/twk run boot/tests/main.tw`
Expected: the run reports the new `stdlib regexp` suite passing along with the rest; `0 failed`.

- [ ] **Step 5: Document in `docs/API.md`**

Add a `@std.regexp` entry to the Standard Library section: the public surface (`compile`, `must`, `test`, `find`, `find_all`, `replace`, `replace_all`, `Match.group`, `Match.text`), the supported subset table (from `docs/plans/regexp.md`), and a short AoC-style example. Add `regexp` to the `use @std.X` module list line.

- [ ] **Step 6: Format and commit**

```bash
make fmt
git add boot/tests/suites/stdlib_regexp_suite.tw boot/tests/main.tw docs/API.md
git commit -m "regexp: wire @std.regexp into the test suite and docs"
```

- [ ] **Step 7: Final full check**

Run: `make test`
Expected: Rust suite and boot suite both green.

---

## Self-review notes (cross-checked against `docs/plans/regexp.md`)

- **Spec coverage:** literals/`.`/classes (T2,T5), quantifiers (T6), groups + `group(i)` (T7), alternation (T8), anchors (T9), `find`/`test` (T4), `find_all` + empty matches (T10), `(?i)` incl. classes (T11), `replace`/`replace_all` + `$` rules (T12), `RegexError` (T13), single-pass record-and-continue VM (T4), `@std` wiring + docs (T14). The greedy/alternation guard cases from the spec appear verbatim in T6/T8; the replacement contract examples in T12.
- **Type consistency:** `Inst`, `Ast`, `ClassItem`, `Regexp`, `Match`, `RegexError`, `Thread`, `Parsed` are defined once (Task 1) and used unchanged; `find_from` returns `Vector<Int>?` (slots) and `materialize` builds the `Match` in `regexp.tw` throughout.
- **Known restructure point:** if the resolver rejects the `parse.tw` ↔ `program.tw` type cycle, move the three shared types into `boot/stdlib/regexp/types.tw` (Task 3, Step 3). This is the one structural contingency; everything else is additive.
- **Two prelude names to confirm before coding** (grep, don't assume): the String→scalar builder (`String.from_code_point` vs other) and that `for x in <Iterator>` is in scope without extra import (it is — verified `Iterator.unfold` + `for` works).
