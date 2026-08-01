# Prefer Case Dispatch Lint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an auto-fixable `prefer-case-dispatch` lint that replaces `cond` expressions used as equality dispatch over one scrutinee with clearer `case` expressions.

**Architecture:** Implement the rule as a structural lint in `boot/compiler/lint.tw`, alongside existing AST-only lint passes. The rule recognizes only `cond` arms whose non-default conditions are equality comparisons against the same source-sliced scrutinee and whose compared values can be emitted as case patterns; it then reports a finding carrying a whole-expression replacement edit. The command layer only needs to select the new rule's edits under `--fix` or `--fix-prefer-case-dispatch`.

**Tech Stack:** Twinkle boot compiler (`boot/`), existing `LintFinding`/`FixEdit` infrastructure, `target/twk` formatter/linter/test runner.

## Global Constraints

- Treat the boot compiler in `boot/` as the primary implementation.
- Keep the lint structural; do not require typechecking beyond the existing lint pass context.
- Preserve source meaning: a `cond` without `_` rewrites to a `case` without `_`; a `cond` with `_` keeps the default arm.
- Auto-fix only when the shared scrutinee is stable to evaluate once instead of per arm; v1 accepts bare identifiers and field-only paths.
- Auto-fix only when each compared RHS can be a pattern with the same meaning as equality: scalar literals and variant patterns, not bare identifiers or arbitrary expressions.
- After editing `.tw` files, run `target/twk fmt` on changed Twinkle files and `target/twk lint boot/main.tw`.
- Never run `tree-sitter test`.

---

## File Structure

- `boot/compiler/lint.tw`
  - Add detection helpers for equality-dispatch `cond` expressions.
  - Add replacement synthesis that rewrites the entire `cond` span to a `case` expression.
  - Plug the finding into the existing `.Cond(arms)` branch of `lint_expr` while still linting nested condition/body expressions.

- `boot/compiler/lint_rules.tw`
  - Add human-facing rule metadata for `prefer-case-dispatch`.

- `boot/commands/lint.tw`
  - Add selection plumbing so `--fix-prefer-case-dispatch` applies only this rule and `--fix` includes it.

- `boot/main.tw`
  - Register the `twk lint --fix-prefer-case-dispatch` flag.

- `boot/tests/suites/lint_pass_suite.tw`
  - Add structural detection and edit tests for string dispatch, integer dispatch without a default, variant dispatch, and negative cases.

- `boot/tests/suites/lint_command_suite.tw`
  - Add the new rule id to the rule-description guardrail and add one command/report test for the detailed rationale.

- `boot/compiler/query/hover.tw`
  - Apply the new preferred `case` form to the contract-method dispatch sites currently written as equality-dispatch `cond`.

- `examples/performance/awfy/twinkle/json.tw`
  - Apply the new preferred `case` form to `parse_value`.

---

### Task 1: Add Structural Detection and Unit Tests

**Files:**
- Modify: `boot/compiler/lint.tw`
- Modify: `boot/tests/suites/lint_pass_suite.tw`

**Interfaces:**
- Consumes: existing `LintFinding`, `FixEdit`, `Expr`, `CondArm`, `BinOp.Eq`, and `ctx.source` slicing in `boot/compiler/lint.tw`.
- Produces: `prefer-case-dispatch` findings with one `FixEdit` replacing `expr.span`.

- [ ] **Step 1: Add failing tests for accepted rewrites**

Append these tests near the other auto-fixable structural lint tests in `boot/tests/suites/lint_pass_suite.tw`:

```tw
    .test(
      "prefer-case-dispatch: fixes string equality dispatch with default",
      fn() {
        src := "fn f(method_name: String) Int? {\n  cond {\n    method_name == \"len\" => .Some(1),\n    method_name == \"at\" => .Some(2),\n    _ => .None,\n  }\n}\n"
        fs := findings(src)
        try assert.equal(fs.len(), 1)
        try assert.equal(fs[0].rule, "prefer-case-dispatch")
        try assert.equal(fs[0].edits.len(), 1)
        try assert.equal(
          apply_first_edit(src, fs[0]),
          "fn f(method_name: String) Int? {\n  case method_name {\n    \"len\" => .Some(1),\n    \"at\" => .Some(2),\n    _ => .None,\n  }\n}\n",
        )
        .Ok({})
      },
    )
    .test(
      "prefer-case-dispatch: fixes integer equality dispatch without default",
      fn() {
        src := "fn f(c: Int) Int {\n  cond {\n    c == 123 => 1,\n    c == 91 => 2,\n  }\n}\n"
        fs := findings(src)
        try assert.equal(fs.len(), 1)
        try assert.equal(fs[0].rule, "prefer-case-dispatch")
        try assert.equal(
          apply_first_edit(src, fs[0]),
          "fn f(c: Int) Int {\n  case c {\n    123 => 1,\n    91 => 2,\n  }\n}\n",
        )
        .Ok({})
      },
    )
    .test(
      "prefer-case-dispatch: fixes variant equality dispatch",
      fn() {
        src := "fn f(kind: Kind) Int {\n  cond {\n    kind == .A => 1,\n    kind == .B(2) => 2,\n    _ => 0,\n  }\n}\n"
        fs := findings(src)
        try assert.equal(fs.len(), 1)
        try assert.equal(fs[0].rule, "prefer-case-dispatch")
        try assert.equal(
          apply_first_edit(src, fs[0]),
          "fn f(kind: Kind) Int {\n  case kind {\n    .A => 1,\n    .B(2) => 2,\n    _ => 0,\n  }\n}\n",
        )
        .Ok({})
      },
    )
    .test(
      "prefer-case-dispatch: fixes inline cond after case arrow",
      fn() {
        src := "fn f(tag: Tag, method_name: String) Int? {\n  case tag {\n    .Read => cond {\n      method_name == \"len\" => .Some(1),\n      _ => .None,\n    },\n    _ => .None,\n  }\n}\n"
        fs := findings(src)
        try assert.equal(fs.len(), 1)
        try assert.equal(fs[0].rule, "prefer-case-dispatch")
        try assert.equal(
          apply_first_edit(src, fs[0]),
          "fn f(tag: Tag, method_name: String) Int? {\n  case tag {\n    .Read => case method_name {\n      \"len\" => .Some(1),\n      _ => .None,\n    },\n    _ => .None,\n  }\n}\n",
        )
        .Ok({})
      },
    )
```

- [ ] **Step 2: Add failing tests for rejected shapes**

Append these negative tests after the accepted rewrite tests:

```tw
    .test(
      "prefer-case-dispatch: does not flag real predicate dispatch",
      fn() {
        fs := findings("fn f(n: Int) Int {\n  cond {\n    n < 26 => 1,\n    n < 52 => 2,\n    _ => 3,\n  }\n}\n")
        try assert.equal(fs.len(), 0)
        .Ok({})
      },
    )
    .test(
      "prefer-case-dispatch: does not fix bare identifier pattern comparison",
      fn() {
        fs := findings("fn f(x: Int, y: Int) Int {\n  cond {\n    x == y => 1,\n    _ => 0,\n  }\n}\n")
        try assert.equal(fs.len(), 0)
        .Ok({})
      },
    )
    .test(
      "prefer-case-dispatch: does not flag mixed scrutinees",
      fn() {
        fs := findings("fn f(x: Int, y: Int) Int {\n  cond {\n    x == 1 => 1,\n    y == 2 => 2,\n    _ => 0,\n  }\n}\n")
        try assert.equal(fs.len(), 0)
        .Ok({})
      },
    )
    .test(
      "prefer-case-dispatch: does not flag effectful scrutinee expressions",
      fn() {
        fs := findings("fn f() Int {\n  cond {\n    next() == 1 => 1,\n    next() == 2 => 2,\n    _ => 0,\n  }\n}\n")
        try assert.equal(fs.len(), 0)
        .Ok({})
      },
    )
    .test(
      "prefer-case-dispatch: does not flag not-equals dispatch",
      fn() {
        fs := findings("fn f(x: Int) Int {\n  cond {\n    x != 1 => 1,\n    _ => 0,\n  }\n}\n")
        try assert.equal(fs.len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 3: Run the focused test and verify it fails for the new tests**

Run:

```bash
TWK_TEST_FILTER='prefer-case-dispatch' target/twk run boot/tests/main.tw
```

Expected: the command exits non-zero because the new accepted-shape tests find no lint yet.

- [ ] **Step 4: Add helper types and pattern predicates**

In `boot/compiler/lint.tw`, extend the import list to include `CondArm`:

```tw
use compiler.ast.{
  Block, CondArm, Expr, ForStmt, FunctionDecl, LetStmt, Module, RecordEntry, Stmt, TypeExpr,
}
```

Then insert these helpers before the `lint_expr` function:

```tw
// ── prefer-case-dispatch ─────────────────────────────────────────────
type CaseDispatchArm = .{ pattern: Expr?, body: Expr }

type CaseDispatchCandidate = .{ scrutinee: Expr, arms: Vector<CaseDispatchArm> }

type EqualityDispatchParts = .{ scrutinee: Expr, pattern: Expr }

fn same_source_expr(a: Expr, b: Expr, source: String) Bool {
  source.slice(a.span.start, a.span.end) == source.slice(b.span.start, b.span.end)
}

fn stable_case_scrutinee_expr(e: Expr) Bool {
  case e.kind {
    .Ident(_) => true,
    .Field(base, _) => stable_case_scrutinee_expr(base),
    _ => false,
  }
}

fn case_pattern_expr(e: Expr) Bool {
  case e.kind {
    .IntLit(_) => true,
    .StringLit(_) => true,
    .BoolLit(_) => true,
    .Variant(_, args) => {
      for a in args {
        if !case_pattern_expr(a) {
          return false
        }
      }
      true
    },
    _ => false,
  }
}

fn equality_dispatch_parts(cond: Expr) EqualityDispatchParts? {
  case cond.kind {
    .Binary(.Eq, left, right) => if stable_case_scrutinee_expr(left) and case_pattern_expr(right) {
      .Some(.{ scrutinee: left, pattern: right })
    } else if stable_case_scrutinee_expr(right) and case_pattern_expr(left) {
      .Some(.{ scrutinee: right, pattern: left })
    } else {
      .None
    },
    _ => .None,
  }
}

fn case_dispatch_candidate(arms: Vector<CondArm>, source: String) CaseDispatchCandidate? {
  converted: Vector<CaseDispatchArm> = []
  scrutinee: Expr? = .None

  for arm in arms {
    case arm.condition {
      .Some(c) => {
        parts := try equality_dispatch_parts(c)

        case scrutinee {
          .Some(s) => if !same_source_expr(s, parts.scrutinee, source) {
            return .None
          },
          .None => scrutinee = .Some(parts.scrutinee),
        }

        converted = .append(.{ pattern: .Some(parts.pattern), body: arm.body })
      },
      .None => converted = .append(.{ pattern: .None, body: arm.body }),
    }
  }

  s := try scrutinee
  .Some(.{ scrutinee: s, arms: converted })
}
```

- [ ] **Step 5: Add replacement rendering and the finding constructor**

Continue in `boot/compiler/lint.tw` after the helpers from Step 4:

```tw
fn leading_line_indent(source: String, start: Int) String {
  line_start := start

  for line_start > 0 and source.slice(line_start - 1, line_start) != "\n" {
    line_start = line_start - 1
  }

  i := line_start

  for i < source.len() {
    ch := source.slice(i, i + 1)

    if ch != " " and ch != "\t" {
      return source.slice(line_start, i)
    }

    i = i + 1
  }

  source.slice(line_start, i)
}

fn render_case_dispatch(c: CaseDispatchCandidate, indent: String, source: String) String {
  arm_indent := "${indent}  "
  lines: Vector<String> = ["case ${source.slice(c.scrutinee.span.start, c.scrutinee.span.end)} {"]

  for arm in c.arms {
    lhs := case arm.pattern {
      .Some(p) => source.slice(p.span.start, p.span.end),
      .None => "_",
    }
    body := source.slice(arm.body.span.start, arm.body.span.end)
    lines = .append("${arm_indent}${lhs} => ${body},")
  }

  lines = .append("${indent}}")
  lines.join("\n")
}

fn prefer_case_dispatch_finding(expr: Expr, arms: Vector<CondArm>, source: String) LintFinding? {
  indent := leading_line_indent(source, expr.span.start)
  candidate := try case_dispatch_candidate(arms, source)
  replacement := render_case_dispatch(candidate, indent, source)
  .Some(
    .{
      span: expr.span,
      message: "`cond` is doing equality dispatch over one value; use `case` instead",
      rule: "prefer-case-dispatch",
      edits: [FixEdit.{ start: expr.span.start, end: expr.span.end, replacement }],
    },
  )
}
```

- [ ] **Step 6: Wire the rule into the `.Cond` branch without skipping nested lints**

Replace the current `.Cond(arms)` branch inside `lint_expr` with:

```tw
    .Cond(arms) => {
      out: Vector<LintFinding> = []

      case prefer_case_dispatch_finding(expr, arms, ctx.source) {
        .Some(f) => out = .append(f),
        .None => {},
      }

      for arm in arms {
        case arm.condition {
          .Some(c) => out = .concat(lint_expr(c, ctx)),
          .None => {},
        }
        out = .concat(lint_expr(arm.body, ctx))
      }
      out
    },
```

- [ ] **Step 7: Run the focused structural lint tests**

Run:

```bash
TWK_TEST_FILTER='prefer-case-dispatch' target/twk run boot/tests/main.tw
```

Expected: the accepted rewrite tests and negative tests pass.

---

### Task 2: Add Rule Metadata and Command Fix Plumbing

**Files:**
- Modify: `boot/compiler/lint_rules.tw`
- Modify: `boot/commands/lint.tw`
- Modify: `boot/main.tw`
- Modify: `boot/tests/suites/lint_command_suite.tw`

**Interfaces:**
- Consumes: `LintFinding.rule == "prefer-case-dispatch"` and its `edits` from Task 1.
- Produces: `twk lint --fix-prefer-case-dispatch`, inclusion in `twk lint --fix`, and `--explain` rationale.

- [ ] **Step 1: Add failing command/rule tests**

In `boot/tests/suites/lint_command_suite.tw`, add `"prefer-case-dispatch"` to the `ids` vector in `"lint_rules: every emitted rule id has a description"`:

```tw
          "prefer-case-dispatch",
```

Then append this test near the existing `render_report` rule-rationale tests:

```tw
    .test(
      "render_report: prefer-case-dispatch explains case equality dispatch",
      fn() {
        findings := [
          lint.Finding.{
            path: "a.tw",
            start: 5,
            message: "`cond` is doing equality dispatch over one value; use `case` instead",
            rule: "prefer-case-dispatch",
            edits: [FixEdit.{ start: 5, end: 20, replacement: "case x {\n  1 => y,\n}" }],
          },
        ]
        out := lint.render_report(findings, Dict.new(), true, false)
        try assert.is_true(out.contains("use `case` for equality dispatch over one value"))
        try assert.is_true(out.contains("cond {"))
        try assert.is_true(out.contains("case method_name"))
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run the focused command tests and verify they fail**

Run:

```bash
TWK_TEST_FILTER='prefer-case-dispatch' target/twk run boot/tests/main.tw
```

Expected: the command/rule metadata test fails because the rule has no description yet.

- [ ] **Step 3: Add the rule description**

In `boot/compiler/lint_rules.tw`, add this case before `_ => .None`:

```tw
    "prefer-case-dispatch" => .Some(
      .{
        brief: "use `case` for equality dispatch over one value",
        detailed:
          \\A `cond` is best for ordered predicate checks such as ranges,
          \\thresholds, and mixed boolean conditions. When every arm compares the
          \\same value to a literal or variant, `case` says the same thing with
          \\less ceremony and lets the reader see the dispatch key once:
          \\
          \\    // Avoid
          \\    cond {
          \\      method_name == "len" => len_doc,
          \\      method_name == "at" => at_doc,
          \\      _ => .None,
          \\    }
          \\
          \\    // Prefer
          \\    case method_name {
          \\      "len" => len_doc,
          \\      "at" => at_doc,
          \\      _ => .None,
          \\    }
          \\
          \\The auto-fix preserves whether a default arm is present.
        ,
      },
    ),
```

- [ ] **Step 4: Add command-line flag registration**

In `boot/main.tw`, add this flag to `lint_cmd` after the other `--fix-...` flags:

```tw
  .add_flag("fix-prefer-case-dispatch", "Apply only the prefer-case-dispatch rewrite")
```

- [ ] **Step 5: Select the new rule's edits in `twk lint`**

In `boot/commands/lint.tw`, update `select_edits` to accept `fix_case_dispatch: Bool`, include the selected expression, and update all call sites.

The function signature should become:

```tw
fn select_edits(
  findings: Vector<Finding>,
  fix_unused: Bool,
  fix_inherent: Bool,
  fix_inline_copy: Bool,
  fix_redundant: Bool,
  fix_case_dispatch: Bool,
) Dict<String, Vector<FixEdit>> {
```

The `selected :=` expression should include:

```tw
      or f.rule == "prefer-case-dispatch" and fix_case_dispatch
```

In `run_lint_command`, add:

```tw
  fix_case_dispatch := fix_all or parsed.has_flag("fix-prefer-case-dispatch")
```

Update `any_apply` to include `fix_case_dispatch`, and pass `fix_case_dispatch` to `select_edits(...)`.

- [ ] **Step 6: Run the command/rule tests**

Run:

```bash
TWK_TEST_FILTER='lint_rules' target/twk run boot/tests/main.tw
TWK_TEST_FILTER='render_report: prefer-case-dispatch' target/twk run boot/tests/main.tw
```

Expected: both filtered runs pass.

---

### Task 3: Apply the Preferred Form to Existing Trigger Sites

**Files:**
- Modify: `boot/compiler/query/hover.tw`
- Modify: `examples/performance/awfy/twinkle/json.tw`

**Interfaces:**
- Consumes: `case` expression syntax already supported by parser/checker/formatter.
- Produces: source tree that does not immediately violate the new lint rule at the known examples.

- [ ] **Step 1: Rewrite contract-method hover dispatch**

In `boot/compiler/query/hover.tw`, replace the `.IndexRead` and `.IndexWrite` branches in `contract_method_hover` with:

```tw
    .IndexRead => case method_name {
      "len" => .Some("fn len(self: Self) Int\n\nContract method from `IndexRead`."),
      "at" => .Some(
        "fn at(self: Self, index: Int) E\n\nContract method from `IndexRead`.",
      ),
      _ => .None,
    },
    .IndexWrite => case method_name {
      "set_at" => .Some(
        "fn set_at(self: Self, index: Int, value: E) Self\n\nContract method from `IndexWrite`.",
      ),
      "append" => .Some(
        "fn append(self: Self, value: E) Self\n\nContract method from `IndexWrite`.",
      ),
      _ => .None,
    },
```

- [ ] **Step 2: Rewrite AWFY JSON value dispatch**

In `examples/performance/awfy/twinkle/json.tw`, replace the `cond` in `parse_value` with:

```tw
  case c {
    123 => parse_obj(s, p),
    91 => parse_arr(s, p),
    34 => {
      sr := parse_str(s, p)
      PResult.{ value: .JStr(sr.str), pos: sr.pos }
    },
    116 => PResult.{ value: .JBool(true), pos: p + 4 },
    102 => PResult.{ value: .JBool(false), pos: p + 5 },
    110 => PResult.{ value: .JNull, pos: p + 4 },
    _ => {
      nr := parse_num(s, p)
      PResult.{ value: .JNum(nr.num), pos: nr.pos }
    },
  }
```

- [ ] **Step 3: Format changed Twinkle files**

Run:

```bash
target/twk fmt boot/compiler/lint.tw boot/compiler/lint_rules.tw boot/commands/lint.tw boot/main.tw boot/tests/suites/lint_pass_suite.tw boot/tests/suites/lint_command_suite.tw boot/compiler/query/hover.tw examples/performance/awfy/twinkle/json.tw
```

Expected: formatter exits successfully.

- [ ] **Step 4: Verify the known trigger sites build/check**

Run:

```bash
TWK_TEST_FILTER='lsp hover' target/twk run boot/tests/main.tw
target/twk build examples/performance/awfy/twinkle/json.tw -o /tmp/awfy-json.wasm
```

Expected: the LSP hover filtered run passes, and the AWFY JSON example builds.

---

### Task 4: End-to-End Lint Fix Verification

**Files:**
- Test only: temporary files under `/tmp`
- Verify: changed files from Tasks 1-3

**Interfaces:**
- Consumes: `twk lint --fix-prefer-case-dispatch` flag from Task 2.
- Produces: evidence that CLI autofix rewrites both with-default and without-default forms.

- [ ] **Step 1: Create a temporary lint fixture**

Run:

```bash
cat > /tmp/prefer_case_dispatch.tw <<'EOF'
fn doc(method_name: String) Int? {
  cond {
    method_name == "len" => .Some(1),
    method_name == "at" => .Some(2),
    _ => .None,
  }
}

fn tag(c: Int) Int {
  cond {
    c == 123 => 1,
    c == 91 => 2,
  }
}
EOF
```

- [ ] **Step 2: Confirm lint reports the new rule**

Run:

```bash
target/twk lint /tmp/prefer_case_dispatch.tw --explain
```

Expected: exits non-zero and prints `prefer-case-dispatch` with both findings.

- [ ] **Step 3: Apply only the new rule's autofix**

Run:

```bash
target/twk lint /tmp/prefer_case_dispatch.tw --fix-prefer-case-dispatch
```

Expected: prints `Fixed: /tmp/prefer_case_dispatch.tw` and reports no remaining `prefer-case-dispatch` finding for that file.

- [ ] **Step 4: Inspect the rewritten temporary file**

Run:

```bash
rg -n "case method_name|case c|cond" /tmp/prefer_case_dispatch.tw
```

Expected: output contains `case method_name` and `case c`; output does not contain `cond`.

- [ ] **Step 5: Run focused tests for the feature**

Run:

```bash
TWK_TEST_FILTER='prefer-case-dispatch' target/twk run boot/tests/main.tw
TWK_TEST_FILTER='lint_rules' target/twk run boot/tests/main.tw
TWK_TEST_FILTER='render_report: prefer-case-dispatch' target/twk run boot/tests/main.tw
```

Expected: all filtered runs pass.

- [ ] **Step 6: Run project lint and relevant full suites**

Run:

```bash
target/twk lint boot/main.tw
make boot-test
```

Expected: `target/twk lint boot/main.tw` reports no actionable findings introduced by this work, and `make boot-test` passes.

- [ ] **Step 7: Commit**

Run:

```bash
git status --short
git add boot/compiler/lint.tw boot/compiler/lint_rules.tw boot/commands/lint.tw boot/main.tw boot/tests/suites/lint_pass_suite.tw boot/tests/suites/lint_command_suite.tw boot/compiler/query/hover.tw examples/performance/awfy/twinkle/json.tw
git commit -m "Add prefer-case-dispatch lint"
```

Expected: commit succeeds with the lint implementation, tests, command plumbing, and source cleanups together.

---

## Self-Review Notes

- Spec coverage: the plan covers detection, autofix, missing-default preservation, rule metadata, command flag selection, known source rewrites, and verification.
- Placeholder scan: no task contains open-ended placeholders; each implementation step names concrete files, code, or commands.
- Type consistency: the rule id is consistently `prefer-case-dispatch`; the command flag is consistently `--fix-prefer-case-dispatch`; lint findings carry `FixEdit` replacements through existing command plumbing.
