# Redundant Print Stringify Lint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an auto-fixable lint that replaces redundant `println("${x}")` and `println(x.to_string())`-style calls with direct generic print/error sink calls.

**Architecture:** Build on the generic `Stringify` I/O API from `docs/plans/2026-07-28-generic-stringify-io.md`. The lint is an AST/source-slice rewrite in `compiler.lint`: it recognizes unshadowed calls to the public print/error sink family, checks that the single argument is either exactly one interpolation or a direct zero-arg `.to_string()` call, and emits one `FixEdit` replacing only the argument expression with the underlying value expression. Rule metadata and tests follow the existing `twk lint` finding/auto-fix conventions.

**Tech Stack:** Twinkle boot compiler (`boot/`), structural lint visitor `boot/compiler/lint.tw`, rule registry `boot/compiler/lint_rules.tw`, lint command/tests, generic I/O prelude wrappers from the prerequisite plan.

## Global Constraints

- Prerequisite: `docs/plans/2026-07-28-generic-stringify-io.md` has landed, so `print`, `println`, `eprint`, `eprintln`, and `error` are public `T: Stringify` wrappers.
- Only rewrite direct calls to the public sink family: `print`, `println`, `eprint`, `eprintln`, and `error`.
- Runtime/host behavior must remain unchanged; this lint only rewrites source to the new generic API.
- Auto-fix only when the whole single argument is redundant stringification:
  - `println("${foo}")` → `println(foo)`
  - `println(foo.to_string())` → `println(foo)`
- Do not rewrite interpolation with surrounding text or multiple interpolations:
  - `println("foo=${foo}")` stays as-is.
  - `println("${a}${b}")` stays as-is.
- Do not rewrite when the sink name is shadowed by a local/parameter or a user-defined function.
- Do not rewrite method calls such as `logger.println(x.to_string())`.
- Do not add new parser or type-system behavior.
- Do not run tree-sitter tests.
- After editing `.tw` files, run `target/twk fmt <changed .tw files>` and `target/twk lint boot/main.tw`.

---

## File Structure

- **Modify** `boot/compiler/lint.tw` — add the `redundant-print-stringify` detection, safe shadow tracking, and machine-applicable edits.
- **Modify** `boot/compiler/lint_rules.tw` — add brief/detailed rule explanation.
- **Modify** `boot/tests/suites/lint_pass_suite.tw` — unit tests for positive rewrites and safety boundaries.
- **Modify** `boot/tests/suites/lint_command_suite.tw` — include the new rule in the “every emitted rule id has a description” guard and optionally report rendering coverage.
- **Modify** `docs/plans/2026-07-28-generic-stringify-io.md` or `docs/API.md` only if the generic I/O implementation plan/docs do not already mention the new preferred direct-call style.

---

## Task 1: Add failing lint tests

**Files:**
- Modify: `boot/tests/suites/lint_pass_suite.tw`
- Modify: `boot/tests/suites/lint_command_suite.tw`

**Interfaces:**
- Consumes: existing `findings(src: String) Vector<lint.LintFinding>` helper.
- Produces: failing tests for rule id `redundant-print-stringify`, edit ranges, and no-fix boundaries.

- [ ] **Step 1: Add a helper to apply the first lint edit.**

In `boot/tests/suites/lint_pass_suite.tw`, near `findings`, add:

```twinkle
fn apply_first_edit(src: String, finding: lint.LintFinding) String {
  edit := finding.edits[0]
  src.slice(0, edit.start).concat(edit.replacement).concat(src.slice(edit.end, src.len()))
}
```

- [ ] **Step 2: Add interpolation rewrite coverage.**

Append these tests to `suite()`:

```twinkle
    .test(
      "redundant-print-stringify: fixes single interpolation argument",
      fn() {
        src := "fn demo(foo: String) Void {\n  println(\"${foo}\")\n}\n"
        fs := findings(src)
        try assert.equal(fs.len(), 1)
        try assert.equal(fs[0].rule, "redundant-print-stringify")
        try assert.equal(fs[0].edits.len(), 1)
        try assert.equal(apply_first_edit(src, fs[0]), "fn demo(foo: String) Void {\n  println(foo)\n}\n")
        .Ok({})
      },
    )
    .test(
      "redundant-print-stringify: fixes expression interpolation argument",
      fn() {
        src := "fn demo(x: Int) Void {\n  eprintln(\"${x + 1}\")\n}\n"
        fs := findings(src)
        try assert.equal(fs.len(), 1)
        try assert.equal(fs[0].rule, "redundant-print-stringify")
        try assert.equal(fs[0].edits[0].replacement, "x + 1")
        .Ok({})
      },
    )
```

- [ ] **Step 3: Add `.to_string()` rewrite coverage for stdout/stderr/error sinks.**

Add:

```twinkle
    .test(
      "redundant-print-stringify: fixes direct to_string argument",
      fn() {
        src := "fn demo(foo: String) Void {\n  print(foo.to_string())\n}\n"
        fs := findings(src)
        try assert.equal(fs.len(), 1)
        try assert.equal(fs[0].rule, "redundant-print-stringify")
        try assert.equal(apply_first_edit(src, fs[0]), "fn demo(foo: String) Void {\n  print(foo)\n}\n")
        .Ok({})
      },
    )
    .test(
      "redundant-print-stringify: fixes error to_string while preserving trap call",
      fn() {
        src := "fn demo(err: String) Int {\n  error(err.to_string())\n}\n"
        fs := findings(src)
        try assert.equal(fs.len(), 1)
        try assert.equal(fs[0].rule, "redundant-print-stringify")
        try assert.equal(apply_first_edit(src, fs[0]), "fn demo(err: String) Int {\n  error(err)\n}\n")
        .Ok({})
      },
    )
```

- [ ] **Step 4: Add safety-boundary tests for interpolation.**

Add:

```twinkle
    .test(
      "redundant-print-stringify: does not fix interpolation with surrounding text",
      fn() {
        fs := findings("fn demo(foo: String) Void {\n  println(\"foo=${foo}\")\n}\n")
        try assert.equal(fs.len(), 0)
        .Ok({})
      },
    )
    .test(
      "redundant-print-stringify: does not fix multiple interpolations",
      fn() {
        fs := findings("fn demo(a: String, b: String) Void {\n  println(\"${a}${b}\")\n}\n")
        try assert.equal(fs.len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 5: Add safety-boundary tests for calls.**

Add:

```twinkle
    .test(
      "redundant-print-stringify: does not fix non-sink calls",
      fn() {
        fs := findings("fn consume(s: String) Void {}\nfn demo(foo: String) Void {\n  consume(foo.to_string())\n}\n")
        try assert.equal(fs.len(), 0)
        .Ok({})
      },
    )
    .test(
      "redundant-print-stringify: does not fix method calls named println",
      fn() {
        fs := findings("fn demo(logger: Logger, foo: String) Void {\n  logger.println(foo.to_string())\n}\n")
        try assert.equal(fs.len(), 0)
        .Ok({})
      },
    )
    .test(
      "redundant-print-stringify: does not fix locally shadowed sink name",
      fn() {
        fs := findings("fn demo(println: fn(String) Void, foo: String) Void {\n  println(foo.to_string())\n}\n")
        try assert.equal(fs.len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 6: Update emitted-rule description guard.**

In `boot/tests/suites/lint_command_suite.tw`, add the new id to the `ids` vector in `"lint_rules: every emitted rule id has a description"`:

```twinkle
"redundant-print-stringify",
```

- [ ] **Step 7: Run the lint suites and confirm the new tests fail.**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: tests added in this task fail because no `redundant-print-stringify` findings are emitted and the rule registry does not yet describe the rule.

---

## Task 2: Track local shadowing in the structural lint context

**Files:**
- Modify: `boot/compiler/lint.tw`
- Modify: `boot/tests/suites/lint_pass_suite.tw`

**Interfaces:**
- Produces: `LintCtx.values: Dict<String, Bool>` containing names bound in the current lexical scope.
- Consumers: Task 3 uses `values` to skip calls where `print`/`println`/`error` is a local value rather than the public prelude wrapper.

- [ ] **Step 1: Extend `LintCtx`.**

Change:

```twinkle
type LintCtx = .{ types: Dict<String, String>, source: String }
```

to:

```twinkle
type LintCtx = .{ types: Dict<String, String>, values: Dict<String, Bool>, source: String, env: ResolvedEnv }
```

- [ ] **Step 2: Add context constructors/helpers.**

Near `LintCtx`, add:

```twinkle
fn empty_lint_ctx(env: ResolvedEnv, source: String) LintCtx {
  .{ types: Dict.new(), values: Dict.new(), source, env }
}

fn ctx_with_value(ctx: LintCtx, name: String) LintCtx {
  ctx.values[name] = true
  ctx
}

fn param_values(decl: FunctionDecl) Dict<String, Bool> {
  values: Dict<String, Bool> = Dict.new()

  for p in decl.params {
    values[p.name] = true
  }

  values
}
```

Keep the existing `param_types(decl)` helper; it still supplies annotated nominal types for record-copy lints.

- [ ] **Step 3: Initialize function lint contexts with params and env.**

In `lint_module`, change the function case from:

```twinkle
out = .concat(lint_block(decl.body, .{ types: param_types(decl), source }))
```

to:

```twinkle
out = .concat(lint_block(decl.body, .{ types: param_types(decl), values: param_values(decl), source, env }))
```

For top-level statements, start with `empty_lint_ctx(env, source)` and thread it through statement order so top-level `let` names also shadow later calls.

Use this shape:

```twinkle
cur := empty_lint_ctx(env, source)

for item in module.items {
  case item {
    .Function(decl) => {
      out = .concat(lint_record_copy(decl, env))
      out = .concat(lint_constant_fn(decl, source))
      out = .concat(lint_block(decl.body, .{ types: param_types(decl), values: param_values(decl), source, env }))
    },
    .Stmt(stmt) => {
      out = .concat(lint_stmt(stmt, cur))
      case stmt {
        .Let(ls) => cur = ctx_with_value(cur, ls.name),
        _ => {},
      }
    },
    _ => {},
  }
}
```

- [ ] **Step 4: Thread `values` through block `let` statements.**

In `lint_block`, ensure a let binding is not visible while linting its own initializer, but is visible afterward. Replace the existing annotated-let update inside the `else` branch with this order:

```twinkle
case stmt {
  .Let(ls) => {
    case inline_copy_finding_for_let(block, i, ls, cur) {
      .Some(f) => out = .append(f),
      .None => {},
    }
  },
  _ => {},
}
out = .concat(lint_stmt(stmt, cur))
case stmt {
  .Let(ls) => {
    cur = ctx_with_value(cur, ls.name)
    case ls.ty {
      .Some(t) => case type_name(t) {
        .Some(n) => cur.types[ls.name] = n,
        .None => {},
      },
      .None => {},
    }
  },
  _ => {},
}
```

This preserves existing record-copy behavior for later statements and avoids treating a binding as shadowing itself.

- [ ] **Step 5: Run existing lint tests.**

Run:

```bash
target/twk fmt boot/compiler/lint.tw boot/tests/suites/lint_pass_suite.tw
target/twk run boot/tests/main.tw
```

Expected: existing lint tests still pass; new redundant-stringify tests still fail because detection is not implemented.

---

## Task 3: Detect redundant print/error stringification and emit fixes

**Files:**
- Modify: `boot/compiler/lint.tw`

**Interfaces:**
- Consumes: `LintCtx.values`, `LintCtx.source`, and `LintCtx.env` from Task 2.
- Produces: `LintFinding { rule: "redundant-print-stringify", edits: [FixEdit] }` for safe matches.

- [ ] **Step 1: Add sink-name and origin helpers.**

In `boot/compiler/lint.tw`, near the expression lint helpers, add:

```twinkle
fn is_print_stringify_sink_name(name: String) Bool {
  name == "print" or name == "println" or name == "eprint" or name == "eprintln" or name == "error"
}

fn is_public_prelude_io_sink(ctx: LintCtx, name: String) Bool {
  if !is_print_stringify_sink_name(name) {
    return false
  }

  if ctx.values.has(name) {
    return false
  }

  case ctx.env.lookup_function_origin(name) {
    .Some(origin) => origin.func_name == name and origin.module_path.ends_with("/prelude/io.tw"),
    .None => false,
  }
}
```

This depends on the generic I/O plan's `boot/prelude/io.tw` wrappers and the signature-stem origin fix.

- [ ] **Step 2: Add argument simplification helper.**

Add:

```twinkle
type RedundantStringifyArg = .{ replacement_start: Int, replacement_end: Int, replacement: String, kind: String }

fn redundant_stringify_arg(arg: Expr, source: String) RedundantStringifyArg? {
  case arg.kind {
    .StringInterp(parts) => {
      if parts.len() != 1 {
        return .None
      }

      inner := case parts[0] {
        .Interpolation(e) => e,
        .Literal(_) => return .None,
      }

      return .Some(
        .{
          replacement_start: arg.span.start,
          replacement_end: arg.span.end,
          replacement: source.slice(inner.span.start, inner.span.end),
          kind: "interpolation",
        },
      )
    },
    .Call(callee, call_args) => {
      if call_args.len() != 0 {
        return .None
      }

      case callee.kind {
        .Field(base, method_name) => if method_name == "to_string" {
          return .Some(
            .{
              replacement_start: arg.span.start,
              replacement_end: arg.span.end,
              replacement: source.slice(base.span.start, base.span.end),
              kind: "to_string",
            },
          )
        },
        _ => {},
      }
      .None
    },
    _ => .None,
  }
}
```

- [ ] **Step 3: Add finding constructor.**

Add:

```twinkle
fn redundant_print_stringify_finding(call: Expr, sink: String, arg: RedundantStringifyArg) LintFinding {
  .{
    span: call.span,
    message: "`${sink}` accepts any `Stringify` value; pass the value directly instead of redundant ${arg.kind}",
    rule: "redundant-print-stringify",
    edits: [FixEdit.{ start: arg.replacement_start, end: arg.replacement_end, replacement: arg.replacement }],
  }
}
```

- [ ] **Step 4: Integrate into `lint_expr` call handling.**

Replace the `.Call(callee, args)` branch with one that checks the current call before recursively linting children:

```twinkle
    .Call(callee, args) => {
      out: Vector<LintFinding> = []

      case callee.kind {
        .Ident(name) => if args.len() == 1 and is_public_prelude_io_sink(ctx, name) {
          case redundant_stringify_arg(args[0], ctx.source) {
            .Some(arg) => out = .append(redundant_print_stringify_finding(expr, name, arg)),
            .None => {},
          }
        },
        _ => {},
      }

      out = .concat(lint_expr(callee, ctx))

      for a in args {
        out = .concat(lint_expr(a, ctx))
      }
      out
    },
```

This still visits nested expressions, so unrelated findings inside the argument are preserved.

- [ ] **Step 5: Run lint tests.**

Run:

```bash
target/twk fmt boot/compiler/lint.tw
target/twk run boot/tests/main.tw
```

Expected: the positive rewrite tests now pass. The rule-description guard still fails until Task 4 adds `lint_rules` metadata.

---

## Task 4: Register rule explanation and command metadata

**Files:**
- Modify: `boot/compiler/lint_rules.tw`
- Modify: `boot/tests/suites/lint_command_suite.tw`

**Interfaces:**
- Consumes: rule id `redundant-print-stringify` from Task 3.
- Produces: `twk lint --explain` content and complete rule-id coverage.

- [ ] **Step 1: Add rule metadata.**

In `boot/compiler/lint_rules.tw`, add a new case arm before `_ => .None`:

```twinkle
    "redundant-print-stringify" => .Some(
      .{
        brief: "print/error sinks accept Stringify values directly; skip explicit stringification",
        detailed:
          \The print/error sink family is generic over `T: Stringify`, so wrapping
          \a value in a one-hole interpolation or calling `.to_string()` before
          \printing is redundant ceremony:
          \
          \    // Avoid
          \    println("${foo}")
          \    eprintln(err.to_string())
          \
          \    // Prefer
          \    println(foo)
          \    eprintln(err)
          \
          \The rule only auto-fixes whole-argument stringification. It does not
          \rewrite formatted messages such as `println("foo=${foo}")`.
        ,
      },
    ),
```

- [ ] **Step 2: Keep the rule-id coverage test exact.**

Confirm `boot/tests/suites/lint_command_suite.tw` includes:

```twinkle
"redundant-print-stringify",
```

in the emitted rule ids vector.

- [ ] **Step 3: Add a render-report smoke assertion.**

Add this test to `lint_command_suite.tw`:

```twinkle
    .test(
      "render_report: redundant-print-stringify shows direct Stringify guidance",
      fn() {
        findings := [
          lint.Finding.{
            path: "a.tw",
            start: 5,
            message: "`println` accepts any `Stringify` value; pass the value directly instead of redundant interpolation",
            rule: "redundant-print-stringify",
            edits: [FixEdit.{ start: 10, end: 18, replacement: "foo" }],
          },
        ]
        out := lint.render_report(findings, Dict.new(), true, false)
        try assert.is_true(out.contains("print/error sinks accept Stringify values directly"))
        try assert.is_true(out.contains("println(foo)"))
        .Ok({})
      },
    )
```

- [ ] **Step 4: Run command/lint tests.**

Run:

```bash
target/twk fmt boot/compiler/lint_rules.tw boot/tests/suites/lint_command_suite.tw
target/twk run boot/tests/main.tw
```

Expected: rule metadata tests pass.

---

## Task 5: Verify auto-fix behavior through `twk lint --fix`

**Files:**
- Modify: `boot/tests/suites/lint_command_suite.tw` if the existing command suite has a file-backed `--fix` helper; otherwise keep this as a manual verification step in the task.

**Interfaces:**
- Consumes: `FixEdit` emitted by Task 3 and command-level edit application already used by other auto-fixable rules.
- Produces: confidence that `--fix` and `--fix-redundant-print-stringify` apply the rewrite correctly.

- [ ] **Step 1: Add direct `apply_edits` coverage for this rule's edit shape.**

If no file-backed lint-command test helper exists, add this unit test to `lint_command_suite.tw`:

```twinkle
    .test(
      "apply_edits: redundant-print-stringify edit replaces only the argument",
      fn() {
        src := "println(\"${foo}\")\n"
        out := lint.apply_edits(src, [FixEdit.{ start: 8, end: 16, replacement: "foo" }])
        try assert.equal(out, "println(foo)\n")
        .Ok({})
      },
    )
```

- [ ] **Step 2: Manually verify command-level auto-fix on a temporary file.**

Run:

```bash
cat > /tmp/redundant_print_stringify.tw <<'TW'
fn demo(foo: String, err: String) Void {
  println("${foo}")
  eprintln(err.to_string())
}
TW
target/twk lint /tmp/redundant_print_stringify.tw --fix-redundant-print-stringify
cat /tmp/redundant_print_stringify.tw
```

Expected file content:

```twinkle
fn demo(foo: String, err: String) Void {
  println(foo)
  eprintln(err)
}
```

- [ ] **Step 3: Verify formatted output remains stable.**

Run:

```bash
target/twk fmt /tmp/redundant_print_stringify.tw
target/twk lint /tmp/redundant_print_stringify.tw
```

Expected: formatter succeeds and the lint no longer reports `redundant-print-stringify` for the fixed file.

---

## Task 6: Documentation and final verification

**Files:**
- Modify: `docs/API.md` if the I/O section does not already mention direct `Stringify` values.
- Modify: `docs/plans/2026-07-28-generic-stringify-io.md` only to add a short cross-reference if desired.

**Interfaces:**
- Consumes: implemented lint rule and generic I/O docs.
- Produces: documented preferred style and verified tree.

- [ ] **Step 1: Add a short preferred-style note if absent.**

In `docs/API.md`, under the I/O table from the generic I/O plan, add:

```markdown
Because these functions are generic over `Stringify`, pass values directly:
`println(value)` instead of `println("${value}")` or `println(value.to_string())`.
`twk lint --fix-redundant-print-stringify` rewrites the redundant forms when the
whole argument is only stringification.
```

- [ ] **Step 2: Format/lint changed Twinkle files.**

Run:

```bash
target/twk fmt boot/compiler/lint.tw boot/compiler/lint_rules.tw boot/tests/suites/lint_pass_suite.tw boot/tests/suites/lint_command_suite.tw
target/twk lint boot/main.tw
```

Expected: formatter is idempotent and linter reports no new relevant findings.

- [ ] **Step 3: Run boot tests.**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: PASS.

- [ ] **Step 4: Run full test target if the generic I/O branch is being finalized.**

Run:

```bash
make test
```

Expected: Rust and boot suites pass.

- [ ] **Step 5: Commit.**

```bash
git add boot/compiler/lint.tw boot/compiler/lint_rules.tw boot/tests/suites/lint_pass_suite.tw boot/tests/suites/lint_command_suite.tw docs/API.md docs/plans/2026-07-28-redundant-print-stringify-lint.md
git commit -m "Add redundant print stringification lint"
```

---

## Self-Review Notes

- The plan depends explicitly on the generic I/O plan, because the auto-fix is only semantics-preserving once print/error sinks accept `T: Stringify` directly.
- Both requested patterns are covered: one-hole interpolation and direct `.to_string()` arguments.
- The auto-fix replaces only the argument expression, preserving call formatting, callee spelling, and surrounding statement structure.
- Safety boundaries cover formatted strings, multiple interpolations, non-sink calls, method calls, and local shadowing.
- Rule metadata and command-level fix behavior follow the existing `twk lint` architecture: `LintFinding.edits`, `lint_rules.describe`, grouped reports, and per-rule `--fix-<rule>` flags.
