# `redundant-record-prefix` Lint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `twk lint` rule `redundant-record-prefix` that strips the named-constructor prefix from a record literal (`TypeName.{ … }` → `.{ … }`) wherever an expected record type is already known, with an auto-fix under `--fix-redundant-record-prefix`.

**Architecture:** A single expected-type-directed AST walk in `boot/compiler/lint.tw` threading one `Bool` "expected-typed" flag. A `NamedRecord` node is itself proof its name is a record type (you cannot spell `Enum.{…}`/`Int.{…}`), so firing needs no `ResolvedEnv` lookup — only the module `source` string, for the strip edit and its spelling gate. The four expected-typed positions are annotated `let`, declared return, record-field value, and call argument, plus flow-through into `if`/`case`/`cond` arms and block tails.

**Tech Stack:** Twinkle (`.tw`) boot compiler. Tests via the boot test suite (`make boot-test` / `target/twk run boot/tests/main.tw`). Design spec: [lint-redundant-record-prefix.md](lint-redundant-record-prefix.md).

**v1 scope amendment (post-Task-6 corrective, Task 7):** Task 3 shipped call-argument anchoring alongside record-field anchoring, and Task 6's boot self-application applied it. Self-application surfaced a real false-positive class — a generic callee's parameter type can be a type variable resolved by inference (often from a later argument), so a bare `.{ … }` in that position has no known expected type and would not typecheck. Call-argument anchoring was **dropped from v1**; the rule now anchors three positions (annotated `let`, declared return, record-field value). The historical task steps below are left as written for the record; the corrective is tracked separately as Task 7.

## Global Constraints

- Boot-only change: no Rust/stage0 edits, no bundled-payload regen.
- Twinkle style: assignment is rebinding — accumulate with `out = .concat(x)`, never `out2`/`out3` (`twk lint` enforces this). Run `target/twk fmt <file>` and `target/twk lint boot/main.tw` after editing any `.tw` file.
- Threaded context is a ctx-first inherent record: `PrefixCtx = .{ source: String, returns_typed: Bool }`, called as `ctx.walk_expr(expr, expected)`. Per-node varying data (`expected_typed`) stays an explicit parameter; the record holds only per-function invariants.
- Rule id is the kebab string `redundant-record-prefix` everywhere (finding `rule`, `describe` key, `select_edits` match, CLI flag suffix).
- The rule performs **no** `ResolvedEnv` / `record_info` / function-signature lookup. If a task reaches for the env, the design is being misread.
- Build the CLI after boot changes with `make quick-bundle-cli` (or `make bundle-cli` when `target/boot.wasm` is stale) before running `target/twk` manually; the unit suite runs against the boot source directly and does not need a rebuild.
- Out of scope for v1 (leave un-anchored — sound omissions): variant payloads (`Some(Config.{…})`), array elements (`[Config.{…}]`), bare rebinds (`p = Config.{…}`), and closure return positions.

---

## File Structure

- `boot/compiler/lint.tw` — **Modify.** Add the `redundant-record-prefix` section: `PrefixCtx`, `prefix_strip_edits`, `prefix_finding`, the mutually-recursive `walk_expr` / `walk_block` / `walk_stmt`, the per-decl entry `lint_redundant_prefix_decl`, and two hook lines in `lint_module`.
- `boot/compiler/lint_rules.tw` — **Modify.** Add the `"redundant-record-prefix"` arm to `describe`.
- `boot/commands/lint.tw` — **Modify.** Add a `fix_redundant_prefix: Bool` param to `select_edits` and `select_edits_for_test`, a `redundant-record-prefix` clause to the `selected` chain, and the flag + `any_apply` + call-site threading in `run_lint_command`.
- `boot/main.tw` — **Modify.** Register `--fix-redundant-record-prefix`.
- `boot/tests/suites/lint_pass_suite.tw` — **Modify.** Detection/decline/edit fixtures for all four sources.
- `boot/tests/suites/lint_command_suite.tw` — **Modify.** Flag-gating test for `select_edits`; update the two existing `select_edits_for_test` call sites for the new param.

---

## Task 1: Annotated-let detection + strip edit

Introduce the walk with only the annotated-`let` anchor wired, plus the strip edit and spelling gate. Covers function bodies and top-level statements.

**Files:**
- Modify: `boot/compiler/lint.tw` (add section after the inline-record-copy block, ~line 1582, before `// ── L3: record-copy-helper ──`; add two hook lines in `lint_module`, ~lines 62-81)
- Test: `boot/tests/suites/lint_pass_suite.tw`

**Interfaces:**
- Produces:
  - `type PrefixCtx = .{ source: String, returns_typed: Bool }`
  - `fn prefix_strip_edits(source: String, expr: Expr, name: String) Vector<FixEdit>`
  - `fn prefix_finding(ctx: PrefixCtx, expr: Expr, name: String) Vector<LintFinding>`
  - `fn walk_expr(ctx: PrefixCtx, expr: Expr, expected: Bool) Vector<LintFinding>`
  - `fn walk_block(ctx: PrefixCtx, block: Block, tail_expected: Bool) Vector<LintFinding>`
  - `fn walk_stmt(ctx: PrefixCtx, stmt: Stmt) Vector<LintFinding>`
  - `fn lint_redundant_prefix_decl(decl: FunctionDecl, source: String) Vector<LintFinding>`
- Consumes: existing `LintFinding`, `FixEdit`, and the AST types already imported at the top of `lint.tw` (`Block`, `Expr`, `Stmt`, `RecordEntry`, `FunctionDecl`, `LetStmt`, `ForStmt`). Add `CaseArm`, `CondArm` to that `use compiler.ast.{…}` list if the compiler reports them unbound.

- [ ] **Step 1: Write failing tests**

Add a rule filter helper near the other `*_findings` helpers (top of `lint_pass_suite.tw`):

```tw
fn rp_findings(src: String) Vector<lint.LintFinding> {
  collect f in findings(src) {
    if f.rule == "redundant-record-prefix" {
      f
    } else {
      continue
    }
  }
}
```

Add these tests inside `suite()` (chain more `.test(...)` calls):

```tw
.test(
  "redundant-record-prefix: annotated top-level let fires and strips the prefix",
  fn() {
    src := "c: Config = Config.{ a: 1, b: 2 }\n"
    fs := rp_findings(src)
    try assert.equal(fs.len(), 1)
    try assert.equal(apply_first_edit(src, fs[0]), "c: Config = .{ a: 1, b: 2 }\n")
    .Ok({})
  },
)
.test(
  "redundant-record-prefix: annotated let inside a function fires",
  fn() {
    fs := rp_findings("fn f() {\n  c: Config = Config.{ a: 1 }\n  print(1)\n}\n")
    try assert.equal(fs.len(), 1)
    .Ok({})
  },
)
.test(
  "redundant-record-prefix: inferred := binding does not fire",
  fn() {
    try assert.equal(rp_findings("d := Config.{ a: 1 }\n").len(), 0)
    .Ok({})
  },
)
.test(
  "redundant-record-prefix: an already-anonymous literal does not fire",
  fn() {
    try assert.equal(rp_findings("c: Config = .{ a: 1 }\n").len(), 0)
    .Ok({})
  },
)
```

- [ ] **Step 2: Run tests, verify they fail**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -i "redundant-record-prefix\|FAIL"`
Expected: FAIL — `rp_findings` returns 0 findings (rule not implemented), so the first two tests fail their `assert.equal`.

- [ ] **Step 3: Implement the walk (annotated-let anchor only)**

Add to `boot/compiler/lint.tw`, after the inline-record-copy section:

```tw
// ── redundant-record-prefix ──────────────────────────────────────────
/// A `NamedRecord` literal `T.{ … }` sitting where an expected record type is
/// already known can drop its `T` prefix for the contextual `.{ … }` form. A
/// `NamedRecord` node witnesses that its name is a record type (you cannot spell
/// `Enum.{ … }` / `Int.{ … }`), so no env lookup is needed — the walk threads
/// only whether the current position is expected-typed.

/// Per-function invariants for the walk. `returns_typed` is whether the enclosing
/// function declares a return type (making its tail/return positions
/// expected-typed).
type PrefixCtx = .{ source: String, returns_typed: Bool }

/// Strip edit for `NamedRecord(name, …)` at `expr`: delete the `name` prefix,
/// leaving `.{ … }`. Declines (empty) unless the literal is spelled exactly
/// `name.{` at the expr's start — refusing qualified paths (`pt.Point.{ … }`)
/// and any trivia between the name and `.{`. Type names are ASCII, so
/// `name.len()` matches the byte span; the equality check declines on any
/// mismatch, so a wrong size can never produce a bad edit.
fn prefix_strip_edits(source: String, expr: Expr, name: String) Vector<FixEdit> {
  start := expr.span.start
  stop := start + name.len()

  if source.slice(start, stop) != name {
    return []
  }
  if source.slice(stop, stop + 2) != ".{" {
    return []
  }

  [.{ start, end: stop, replacement: "" }]
}

fn prefix_finding(ctx: PrefixCtx, expr: Expr, name: String) Vector<LintFinding> {
  edits := prefix_strip_edits(ctx.source, expr, name)

  if edits.len() == 0 {
    return []
  }

  [
    .{
      span: expr.span,
      message: "`${name}.{ }` is in a position with a known expected type; drop the `${name}` prefix and use `.{ }`",
      rule: "redundant-record-prefix",
      edits,
    },
  ]
}

fn walk_expr(ctx: PrefixCtx, expr: Expr, expected: Bool) Vector<LintFinding> {
  case expr.kind {
    .NamedRecord(name, entries) => {
      out: Vector<LintFinding> = if expected {
        prefix_finding(ctx, expr, name)
      } else {
        []
      }

      // A record field always carries a declared type.
      for en in entries {
        case en.value {
          .Some(v) => out = .concat(ctx.walk_expr(v, true)),
          .None => {},
        }
      }

      out
    },
    .Record(entries) => {
      out: Vector<LintFinding> = []

      for en in entries {
        case en.value {
          .Some(v) => out = .concat(ctx.walk_expr(v, true)),
          .None => {},
        }
      }

      out
    },
    .Call(callee, args) => {
      out := ctx.walk_expr(callee, false)

      // Function parameters always carry declared types.
      for a in args {
        out = .concat(ctx.walk_expr(a, true))
      }

      out
    },
    .If(cond, then_blk, else_e) => {
      out := ctx.walk_expr(cond, false)
      out = .concat(ctx.walk_block(then_blk, expected))

      case else_e {
        .Some(e) => out = .concat(ctx.walk_expr(e, expected)),
        .None => {},
      }

      out
    },
    .Case(scrut, arms) => {
      out := ctx.walk_expr(scrut, false)

      for arm in arms {
        out = .concat(ctx.walk_expr(arm.expr, expected))
      }

      out
    },
    .Cond(arms) => {
      out: Vector<LintFinding> = []

      for arm in arms {
        case arm.condition {
          .Some(c) => out = .concat(ctx.walk_expr(c, false)),
          .None => {},
        }
        out = .concat(ctx.walk_expr(arm.body, expected))
      }

      out
    },
    .BlockExpr(block) => ctx.walk_block(block, expected),
    .Binary(_, l, r) => ctx.walk_expr(l, false).concat(ctx.walk_expr(r, false)),
    .Unary(_, inner) => ctx.walk_expr(inner, false),
    .Field(base, _) => ctx.walk_expr(base, false),
    .Index(a, b) => ctx.walk_expr(a, false).concat(ctx.walk_expr(b, false)),
    .Array(elems) => {
      out: Vector<LintFinding> = []

      for el in elems {
        out = .concat(ctx.walk_expr(el, false))
      }

      out
    },
    .Variant(_, args) => {
      out: Vector<LintFinding> = []

      for a in args {
        out = .concat(ctx.walk_expr(a, false))
      }

      out
    },
    .Closure(_, _, body) => ctx.walk_block(body, false),
    .StringInterp(parts) => {
      out: Vector<LintFinding> = []

      for p in parts {
        case p {
          .Interpolation(e) => out = .concat(ctx.walk_expr(e, false)),
          .Literal(_) => {},
        }
      }

      out
    },
    .Collect(ce) => {
      out: Vector<LintFinding> = []

      case ce.iter {
        .Some(e) => out = .concat(ctx.walk_expr(e, false)),
        .None => {},
      }
      case ce.condition {
        .Some(e) => out = .concat(ctx.walk_expr(e, false)),
        .None => {},
      }
      out = .concat(ctx.walk_block(ce.body, false))

      out
    },
    _ => [],
  }
}

fn walk_block(ctx: PrefixCtx, block: Block, tail_expected: Bool) Vector<LintFinding> {
  out: Vector<LintFinding> = []

  for stmt in block.stmts {
    out = .concat(ctx.walk_stmt(stmt))
  }

  case block.tail {
    .Some(e) => out = .concat(ctx.walk_expr(e, tail_expected)),
    .None => {},
  }

  out
}

fn walk_stmt(ctx: PrefixCtx, stmt: Stmt) Vector<LintFinding> {
  case stmt {
    .Let(ls) => {
      annotated := case ls.ty {
        .Some(_) => true,
        .None => false,
      }
      ctx.walk_expr(ls.value, annotated)
    },
    .Return(rs) => case rs.value {
      .Some(e) => ctx.walk_expr(e, ctx.returns_typed),
      .None => [],
    },
    .Expr(es) => ctx.walk_expr(es.expr, false),
    .For(fs) => {
      out: Vector<LintFinding> = []

      case fs.iter {
        .Some(e) => out = .concat(ctx.walk_expr(e, false)),
        .None => {},
      }
      case fs.condition {
        .Some(e) => out = .concat(ctx.walk_expr(e, false)),
        .None => {},
      }
      out = .concat(ctx.walk_block(fs.body, false))

      out
    },
    .Break(bs) => case bs.value {
      .Some(e) => ctx.walk_expr(e, false),
      .None => [],
    },
    .Continue(_) => [],
    .Defer(ds) => ctx.walk_expr(ds.expr, false),
    .ErrorStmt(_) => [],
  }
}

/// Entry point per function: the body's tail/return positions are expected-typed
/// exactly when the function declares a return type.
fn lint_redundant_prefix_decl(decl: FunctionDecl, source: String) Vector<LintFinding> {
  returns_typed := case decl.return_type {
    .Some(_) => true,
    .None => false,
  }
  ctx := PrefixCtx.{ source, returns_typed }
  ctx.walk_block(decl.body, returns_typed)
}
```

- [ ] **Step 4: Hook it into `lint_module`**

In `lint_module` (`boot/compiler/lint.tw`, ~line 62), append to the `.Function(decl)` arm, after the existing `lint_block(...)` concat:

```tw
        out = .concat(lint_redundant_prefix_decl(decl, source))
```

In the `.Stmt(stmt)` arm (~line 73), after the existing `out = .concat(lint_stmt(stmt, cur))` and before the `case stmt { .Let(ls) => … }`, add the top-level walk (no enclosing function, so returns are not expected-typed). Bind the ctx to a local first — do not call the method directly on the record literal:

```tw
        top_ctx := PrefixCtx.{ source, returns_typed: false }
        out = .concat(top_ctx.walk_stmt(stmt))
```

- [ ] **Step 5: Run tests, verify they pass**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -20`
Expected: PASS — all four Task 1 tests green, no other suite regressions.

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/compiler/lint.tw boot/tests/suites/lint_pass_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/lint.tw boot/tests/suites/lint_pass_suite.tw
git commit -m "feat(lint): redundant-record-prefix detection for annotated lets

Add an expected-type-directed AST walk that strips a named-constructor
prefix (T.{ … } → .{ … }) where the expected record type is known. This
task wires the annotated-let anchor plus the strip edit and spelling gate."
```

---

## Task 2: Return-position anchor + flow-through

The walk already forwards `expected` through `if`/`case`/`cond`/block tails and honors `ctx.returns_typed` at `.Return`; this task adds tests that lock the return + flow-through behavior in. No new production code is expected — if a test fails, fix the corresponding arm in `walk_expr`/`walk_stmt` from Task 1.

**Files:**
- Test: `boot/tests/suites/lint_pass_suite.tw`
- Modify (only if a test fails): `boot/compiler/lint.tw`

**Interfaces:**
- Consumes: `lint_redundant_prefix_decl`, `walk_block`, `walk_stmt`, `walk_expr`, `rp_findings` (Task 1).

- [ ] **Step 1: Write failing/locking tests**

```tw
.test(
  "redundant-record-prefix: a declared-return tail fires and strips",
  fn() {
    src := "fn f() Config {\n  Config.{ a: 1 }\n}\n"
    fs := rp_findings(src)
    try assert.equal(fs.len(), 1)
    try assert.equal(apply_first_edit(src, fs[0]), "fn f() Config {\n  .{ a: 1 }\n}\n")
    .Ok({})
  },
)
.test(
  "redundant-record-prefix: an explicit return of a named literal fires",
  fn() {
    fs := rp_findings("fn f() Config {\n  return Config.{ a: 1 }\n}\n")
    try assert.equal(fs.len(), 1)
    .Ok({})
  },
)
.test(
  "redundant-record-prefix: both if-arm tails in a declared-return body fire",
  fn() {
    fs := rp_findings(
      "fn f() Config {\n  if c {\n    Config.{ a: 1 }\n  } else {\n    Config.{ a: 2 }\n  }\n}\n",
    )
    try assert.equal(fs.len(), 2)
    .Ok({})
  },
)
.test(
  "redundant-record-prefix: a tail with no declared return type does not fire",
  fn() {
    try assert.equal(rp_findings("fn f() {\n  Config.{ a: 1 }\n}\n").len(), 0)
    .Ok({})
  },
)
```

- [ ] **Step 2: Run tests**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -20`
Expected: PASS (the Task 1 walk already implements return + flow-through). If any fail, correct the matching `.If`/`.Case`/`.Return` arm and re-run.

- [ ] **Step 3: Format and commit**

```bash
target/twk fmt boot/tests/suites/lint_pass_suite.tw
git add boot/tests/suites/lint_pass_suite.tw boot/compiler/lint.tw
git commit -m "test(lint): lock redundant-record-prefix return + flow-through anchoring"
```

---

## Task 3: Record-field and call-argument anchors

Lock in that record-field values and call arguments are always expected-typed — even inside an unanchored (`:=`) enclosing position — and that nested named literals produce one finding per strippable level.

**Files:**
- Test: `boot/tests/suites/lint_pass_suite.tw`
- Modify (only if a test fails): `boot/compiler/lint.tw`

**Interfaces:**
- Consumes: `walk_expr` `.Record`/`.NamedRecord`/`.Call` arms and `rp_findings` (Task 1).

- [ ] **Step 1: Write failing/locking tests**

```tw
.test(
  "redundant-record-prefix: a named literal as a record field value fires",
  fn() {
    src := "c: Wrap = .{ inner: Inner.{ a: 1 } }\n"
    fs := rp_findings(src)
    try assert.equal(fs.len(), 1)
    try assert.equal(apply_first_edit(src, fs[0]), "c: Wrap = .{ inner: .{ a: 1 } }\n")
    .Ok({})
  },
)
.test(
  "redundant-record-prefix: nested named literals each fire",
  fn() {
    fs := rp_findings("c: Wrap = Wrap.{ inner: Inner.{ a: 1 } }\n")
    try assert.equal(fs.len(), 2)
    .Ok({})
  },
)
.test(
  "redundant-record-prefix: a call argument fires even inside a := binding",
  fn() {
    src := "d := process(Config.{ a: 1 })\n"
    fs := rp_findings(src)
    try assert.equal(fs.len(), 1)
    try assert.equal(apply_first_edit(src, fs[0]), "d := process(.{ a: 1 })\n")
    .Ok({})
  },
)
```

- [ ] **Step 2: Run tests**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -20`
Expected: PASS. If a field/arg case fails, verify the `.Record`/`.NamedRecord` arms recurse field values with `true` and `.Call` recurses args with `true`.

- [ ] **Step 3: Format and commit**

```bash
target/twk fmt boot/tests/suites/lint_pass_suite.tw
git add boot/tests/suites/lint_pass_suite.tw boot/compiler/lint.tw
git commit -m "test(lint): lock redundant-record-prefix field + call-argument anchoring"
```

---

## Task 4: Spelling-gate decline + `describe` rationale

Add the qualified-path decline test and register the human-facing rationale so `twk lint --explain` documents the rule.

**Files:**
- Modify: `boot/compiler/lint_rules.tw` (add a `case` arm in `describe`)
- Test: `boot/tests/suites/lint_pass_suite.tw`

**Interfaces:**
- Consumes: `describe` (`boot/compiler/lint_rules.tw:7`), returning `RuleInfo? = .{ brief, detailed }`.

- [ ] **Step 1: Write failing tests**

```tw
.test(
  "redundant-record-prefix: a qualified constructor path is declined by the spelling gate",
  fn() {
    fs := rp_findings("c: Config = mod.Config.{ a: 1 }\n")
    // The NamedRecord name is `Config`, but the source at the literal's start is
    // `mod.Config`, so `source.slice(start, start+len) != name` — no edit.
    try assert.equal(fs.len(), 0)
    .Ok({})
  },
)
```

For the `describe` entry, add a suite check (place near other rule-metadata tests, or in `lint_pass_suite.tw`):

```tw
.test(
  "redundant-record-prefix: describe returns a rationale",
  fn() {
    case lint_rules.describe("redundant-record-prefix") {
      .Some(info) => {
        try assert.is_true(info.brief.len() > 0)
        .Ok({})
      },
      .None => .Err("expected a rationale for redundant-record-prefix"),
    }
  },
)
```

Add `use compiler.lint_rules` to the suite's imports if not already present.

- [ ] **Step 2: Run tests, verify failure**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -i "redundant-record-prefix\|FAIL"`
Expected: the `describe` test fails (`describe` returns `.None`); the qualified-path test may already pass (spelling gate from Task 1) — that is fine, it locks the behavior.

- [ ] **Step 3: Add the `describe` arm**

In `boot/compiler/lint_rules.tw`, inside the `case rule { … }` block (e.g. after the `"inline-record-copy"` arm), add:

```tw
    "redundant-record-prefix" => .Some(
      .{
        brief: "a named record literal in a known-type position can drop its type prefix",
        detailed:
          \\A `TypeName.{ … }` literal sitting where the expected record type is
          \\already known — an annotated binding, a declared return position, a
          \\record field, or a call argument — can drop its prefix for the
          \\contextual anonymous form:
          \\
          \\    // Avoid
          \\    cfg: Config = Config.{ a: 1, b: 2 }
          \\    fn make() Config { Config.{ a: 1, b: 2 } }
          \\
          \\    // Prefer
          \\    cfg: Config = .{ a: 1, b: 2 }
          \\    fn make() Config { .{ a: 1, b: 2 } }
        ,
      },
    ),
```

- [ ] **Step 4: Run tests, verify pass**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/lint_rules.tw boot/tests/suites/lint_pass_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/lint_rules.tw boot/tests/suites/lint_pass_suite.tw
git commit -m "feat(lint): document redundant-record-prefix rationale for --explain"
```

---

## Task 5: `--fix-redundant-record-prefix` flag wiring

Make the edits applyable through the CLI, gated behind the rule's own fix flag, following the `--fix-inline-record-copy` convention.

**Files:**
- Modify: `boot/commands/lint.tw` (`select_edits` ~line 487, `select_edits_for_test` ~line 537, `run_lint_command` ~lines 569-633)
- Modify: `boot/main.tw` (flag registration, ~line 37)
- Test: `boot/tests/suites/lint_command_suite.tw`

**Interfaces:**
- Produces: `select_edits` / `select_edits_for_test` gain a trailing `fix_redundant_prefix: Bool` parameter (9th Bool, appended after `fix_record_copy`).
- Consumes: `lint.select_edits_for_test`, `lint.Finding`, `FixEdit` (already imported in the command suite).

- [ ] **Step 1: Write the failing flag test**

Add to `lint_command_suite.tw` (mirror the existing "record-copy-helper is applied only when its flag is set" test at line 115). Note all `select_edits_for_test` calls now pass **nine** trailing Bools:

```tw
.test(
  "select_edits: redundant-record-prefix is applied only when its flag is set",
  fn() {
    findings := [
      lint.Finding.{
        path: "a.tw",
        start: 0,
        message: "drop prefix",
        rule: "redundant-record-prefix",
        edits: [FixEdit.{ start: 12, end: 18, replacement: "" }],
      },
    ]

    none := lint.select_edits_for_test(
      findings, false, false, false, false, false, false, false, false, false,
    )
    try assert.is_false(none.has("a.tw"))

    some := lint.select_edits_for_test(
      findings, false, false, false, false, false, false, false, false, true,
    )
    selected := try some["a.tw"].ok_or("expected redundant-record-prefix edit")
    try assert.equal(selected.len(), 1)
    try assert.equal(selected[0].start, 12)
    .Ok({})
  },
)
```

- [ ] **Step 2: Run test, verify failure**

Run: `target/twk run boot/tests/main.tw 2>&1 | grep -i "redundant-record-prefix\|arity\|FAIL"`
Expected: FAIL — `select_edits_for_test` currently takes 8 Bools, so the 9-Bool calls are an arity error (and the new rule is unselected).

- [ ] **Step 3: Extend `select_edits` and `select_edits_for_test`**

In `boot/commands/lint.tw`, add the trailing parameter to both `select_edits` (line 487) and `select_edits_for_test` (line 537) signatures:

```tw
  fix_record_copy: Bool,
  fix_redundant_prefix: Bool,
) Dict<String, Vector<FixEdit>> {
```

Add the clause to the `selected :=` chain (after the `record-copy-helper` line, ~line 508):

```tw
      or f.rule == "redundant-record-prefix" and fix_redundant_prefix
```

Pass it through in `select_edits_for_test`'s body (append `fix_redundant_prefix` as the last argument to the inner `select_edits(...)` call).

- [ ] **Step 4: Thread the flag in `run_lint_command`**

In `run_lint_command` (`boot/commands/lint.tw`), after the `fix_record_copy := …` line (~577):

```tw
  fix_redundant_prefix := fix_all or parsed.has_flag("fix-redundant-record-prefix")
```

Add it to the `any_apply` chain (~580-587):

```tw
    or fix_redundant_prefix
```

Append `fix_redundant_prefix` as the final argument to the `select_edits(...)` call inside the fixpoint loop (~623-633).

- [ ] **Step 5: Update the two existing `select_edits_for_test` call sites**

In `lint_command_suite.tw`, the existing tests at ~lines 96, 127, and 140 call `select_edits_for_test` with eight trailing Bools. Append one more `false` to each so they pass nine (the record-copy-helper test's second call at line 140 keeps its `true` as the 8th and gets a trailing `false` as the 9th).

- [ ] **Step 6: Register the CLI flag**

In `boot/main.tw`, after the `.add_flag("fix-inline-record-copy", …)` line (~37), add:

```tw
  .add_flag("fix-redundant-record-prefix", "Apply only the redundant record-type-prefix rewrite")
```

- [ ] **Step 7: Run tests, verify pass**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -20`
Expected: PASS — the new flag-gating test and all previously-updated call sites are green.

- [ ] **Step 8: Format, lint, commit**

```bash
target/twk fmt boot/commands/lint.tw boot/main.tw boot/tests/suites/lint_command_suite.tw
target/twk lint boot/main.tw
git add boot/commands/lint.tw boot/main.tw boot/tests/suites/lint_command_suite.tw
git commit -m "feat(lint): apply redundant-record-prefix under --fix-redundant-record-prefix"
```

---

## Task 6: Boot self-application + byte-identical validation

Rebuild the CLI, run the fixer across the boot compiler, and prove the rewrite is behavior-preserving (self-host stays green and byte-identical).

**Files:**
- Modify: whatever boot `.tw` files the fixer rewrites (mechanical prefix strips).

**Interfaces:** none (integration gate).

- [ ] **Step 1: Rebuild the CLI with the new rule**

Run: `make quick-bundle-cli`
Expected: `target/twk` rebuilds from the updated boot source without error.

- [ ] **Step 2: Preview findings on the boot entry**

Run: `target/twk lint boot/main.tw 2>&1 | grep -c redundant-record-prefix`
Expected: a nonzero count (boot source has many `T.{ … }` in annotated/return positions). If zero, the walk is not reaching boot code — stop and investigate before applying.

- [ ] **Step 3: Apply the fix across boot, then format**

Run:
```bash
target/twk lint --fix-redundant-record-prefix boot/main.tw
git diff --stat
```
Then format every touched file: `target/twk fmt <each changed .tw>` (or `make fmt` if available).
Expected: the diff is only prefix deletions (`Config.{` → `.{`); `fmt` adds no further churn.

- [ ] **Step 4: Boot test suite stays green**

Run: `make boot-test`
Expected: PASS.

- [ ] **Step 5: Self-host byte-identical check**

Run (sequentially — never concurrently, per repo guidance): `make stage2`
Expected: the rebuilt `target/boot.wasm` builds and the self-host loop stays green; the prefix strip is behavior-preserving, so codegen must be unchanged. If `make` exposes a byte-identity/self-host-diff target, run it and confirm no diff.

- [ ] **Step 6: Commit the boot rewrite**

```bash
git add -A
git commit -m "style(boot): drop redundant record-type prefixes via redundant-record-prefix

Apply the new lint's --fix across the boot compiler: named record literals
in annotated, return, field, and argument positions lose their now-redundant
type prefix. Behavior-preserving — self-host stays green and byte-identical."
```

- [ ] **Step 7: Update the plans index**

Remove the umbrella doc's Pattern A "remains" wording is no longer needed once this lands; update the `Rebinding-ceremony fixers` row in `docs/plans/README.md` to note Pattern A is done, and cross-check `lint-rebinding-fixers.md`'s Pattern A section still points at the spec. Commit:

```bash
git add docs/plans/README.md docs/plans/lint-rebinding-fixers.md
git commit -m "docs(plans): mark rebinding-fixer Pattern A landed"
```

---

## Self-Review

**Spec coverage:**
- Core mechanism (Bool-threaded `walk_expr`, no env) → Task 1.
- Strip edit + spelling gate → Task 1 (impl), Task 4 (qualified-path decline).
- Four sources: annotated let → Task 1; return → Task 2; record-field + call-arg → Task 3.
- Flow-through (`if`/`case`/`cond`/block tails) → Task 1 impl, Task 2 tests.
- Rule identity + `describe` → Task 4. Fix flag → Task 5. fmt orthogonality + boot byte-identical validation → Task 6.
- Out-of-scope items (variant/array/rebind/closure) are left un-anchored by the Task 1 walk (those arms recurse with `false`).

**Placeholder scan:** No TBD/TODO; every code step carries literal code; every test has concrete input and expected output.

**Type consistency:** `PrefixCtx`, `walk_expr(ctx, expr, expected)`, `walk_block(ctx, block, tail_expected)`, `walk_stmt(ctx, stmt)`, `lint_redundant_prefix_decl(decl, source)`, `prefix_strip_edits(source, expr, name)`, and `prefix_finding(ctx, expr, name)` are named identically across tasks. The rule string `redundant-record-prefix` and flag `--fix-redundant-record-prefix` are consistent in `lint.tw`, `lint_rules.tw`, `commands/lint.tw`, and `main.tw`. `select_edits`/`select_edits_for_test` gain one appended `fix_redundant_prefix: Bool` (9th Bool), and every call site (2 in `run_lint_command`, 3 in the command suite) is updated in Task 5.
