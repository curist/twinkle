# Lint Rule Explanations + `--explain` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Give every `twk lint` rule a registered brief/detailed rationale, group the report by rule (id + brief once per group), and add `--explain` to expand briefs to the full rationale; then trim CLAUDE.md's prescriptive rebinding block to a pointer.

**Architecture:** A new boot module `compiler/lint_rules.tw` maps the existing kebab `rule` id → `RuleInfo { brief, detailed }`. The command's report rendering moves into a pure `render_report(findings, indexes, explain) String` that groups by rule, looks up the registry for headers, and prints findings indented underneath. `main.tw` registers an `--explain` flag threaded into the command. `detailed` strings are authored as Zig-style `\\` multi-line literals.

**Tech Stack:** Twinkle (boot self-hosted compiler). Tests run via `target/twk run boot/tests/main.tw` with `TWK_TEST_FILTER`. Spec: `docs/plans/lint-rule-explanations.md`.

**Completed:** Implemented and verified. The linter now has a rule-description registry, grouped reports, `--explain`, test coverage, and CLAUDE.md guidance trimmed to point at `twk lint`.

---

## File Structure

- **Create** `boot/compiler/lint_rules.tw` — `pub type RuleInfo` + `pub fn describe(rule) RuleInfo?`. Owns all rule rationale text. One responsibility: rule → description.
- **Modify** `boot/commands/lint.tw` — make `Finding` `pub`; add pure `pub fn render_report(...)`; replace `report_findings`; add `explain` param to `print_summary`; thread `--explain` through `run_lint_command`.
- **Modify** `boot/main.tw` — register `.add_flag("explain", ...)` on `lint_cmd`.
- **Modify** `CLAUDE.md` — keep rebinding *semantics*, trim *prescriptive* block to a pointer, add a `twk lint` dev-flow hint.
- **Modify** `boot/tests/suites/lint_command_suite.tw` — tests for `describe`, the coverage guard, and `render_report` (default + `--explain`).

The six rule ids the codebase emits (verified): `direct-rebinding`, `record-copy-helper`, `unreachable-code`, `unused-must-use`, `unused-imports`, `inherent-calls`.

---

## Task 1: Rule registry module

**Files:**
- Create: `boot/compiler/lint_rules.tw`
- Test: `boot/tests/suites/lint_command_suite.tw`

- [x] **Step 1: Write the failing test**

Add these imports to the top of `boot/tests/suites/lint_command_suite.tw` (after the existing `use` lines):

```tw
use compiler.lint_rules
```

Add these tests inside `suite()` (chain `.test(...)` calls before the closing of the builder):

```tw
    .test(
      "lint_rules: describe returns brief for a known rule",
      fn() {
        info := try lint_rules.describe("direct-rebinding").ok_or("expected Some")
        try assert.equal(
          info.brief,
          "the temporary only aliases a value; rebind the field/index path directly",
        )
        .Ok({})
      },
    )
    .test(
      "lint_rules: describe returns None for an unknown rule",
      fn() {
        case lint_rules.describe("no-such-rule") {
          .Some(_) => .Err("expected None"),
          .None => .Ok({}),
        }
      },
    )
    .test(
      "lint_rules: every emitted rule id has a description",
      fn() {
        ids := [
          "direct-rebinding",
          "record-copy-helper",
          "unreachable-code",
          "unused-must-use",
          "unused-imports",
          "inherent-calls",
        ]

        for id in ids {
          case lint_rules.describe(id) {
            .Some(info) => {
              try assert.is_true(info.brief.len() > 0)
              try assert.is_true(info.detailed.len() > 0)
            },
            .None => { return .Err("no description for rule: ${id}") },
          }
        }
        .Ok({})
      },
    )
```

- [x] **Step 2: Run test to verify it fails**

Run: `TWK_TEST_FILTER=lint_command target/twk run boot/tests/main.tw 2>&1 | tail -15`
Expected: compile/lint error — `compiler.lint_rules` module does not exist (or `describe` undefined). This confirms the test targets missing code.

- [x] **Step 3: Write the registry**

Create `boot/compiler/lint_rules.tw`:

```tw
/// Human-facing rationale for each `twk lint` rule, keyed by its kebab `rule`
/// id. `brief` is the one-line header summary; `detailed` is the full rationale
/// shown under `--explain`. Authored as Zig-style multi-line literals so the
/// embedded Avoid/Prefer code blocks need no escaping.

pub type RuleInfo = .{ brief: String, detailed: String }

pub fn describe(rule: String) RuleInfo? {
  case rule {
    "direct-rebinding" => .Some(.{
      brief: "the temporary only aliases a value; rebind the field/index path directly",
      detailed:
        \\A temporary that only aliases a value, gets rebinding-updates applied,
        \\then is copied back or returned is just ceremony. Twinkle assignment is
        \\rebinding, so the field/index path can be updated in place:
        \\
        \\    // Avoid
        \\    d := reg.by_name
        \\    d[internal] = entry
        \\    reg.by_name = d
        \\
        \\    // Prefer
        \\    reg.by_name[internal] = entry
      ,
    }),
    "record-copy-helper" => .Some(.{
      brief: "rebuilds a record by copying fields; rebind the changed field directly",
      detailed:
        \\A function that returns a record literal copying most fields verbatim
        \\from a parameter of the same type is a `with_*` helper. Rebind the
        \\field you mean instead:
        \\
        \\    // Avoid
        \\    fn with_docs(s: State, docs: Store) State {
        \\      .{ initialized: s.initialized, docs, cache: s.cache }
        \\    }
        \\
        \\    // Prefer
        \\    s.docs = new_store
      ,
    }),
    "unreachable-code" => .Some(.{
      brief: "code after a diverging statement never runs",
      detailed:
        \\A statement following `return`, `break`, `continue`, or `error(...)` in
        \\the same block can never execute. Usually a logic error: either the
        \\early exit is wrong, or the trailing code should move before it.
      ,
    }),
    "unused-must-use" => .Some(.{
      brief: "a Result or Option is silently discarded",
      detailed:
        \\A statement-position `Result<_, _>` or `T?` that is neither `try`-ed,
        \\matched, nor bound drops a possible error or `None`. Handle it with
        \\`try`, `case`, `.ok_or(...)`, or bind it explicitly.
      ,
    }),
    "unused-imports" => .Some(.{
      brief: "an imported name is never used",
      detailed:
        \\This `use` brings in a name nothing references. Remove it. Auto-fixable
        \\with `twk lint --fix-unused-imports`.
      ,
    }),
    "inherent-calls" => .Some(.{
      brief: "a free-function call can use inherent-method syntax",
      detailed:
        \\When `M.f(x, ...)` resolves to the inherent method of `x`'s type, the
        \\receiver form reads better: `x.f(...)`. Auto-fixable with
        \\`twk lint --fix-inherent-calls`.
        \\
        \\    // Avoid            // Prefer
        \\    Vector.map(xs, f)   xs.map(f)
      ,
    }),
    _ => .None,
  }
}
```

- [x] **Step 4: Run test to verify it passes**

Run: `TWK_TEST_FILTER=lint_command target/twk run boot/tests/main.tw 2>&1 | tail -6`
Expected: all `lint_command` tests pass (count increased by 3, 0 failed).

- [x] **Step 5: Commit**

```bash
git add boot/compiler/lint_rules.tw boot/tests/suites/lint_command_suite.tw
git commit -m "Add lint rule description registry"
```

---

## Task 2: Pure grouped report renderer (default mode)

**Files:**
- Modify: `boot/commands/lint.tw` (make `Finding` pub; add `render_report`; add `use compiler.lint_rules`)
- Test: `boot/tests/suites/lint_command_suite.tw`

- [x] **Step 1: Write the failing test**

Add to `suite()` in `boot/tests/suites/lint_command_suite.tw`:

```tw
    .test(
      "render_report: groups findings by rule with brief headers, alphabetical",
      fn() {
        findings := [
          lint.Finding.{
            path: "z.tw", start: 0, message: "code after a return never runs",
            rule: "unreachable-code", edits: [],
          },
          lint.Finding.{
            path: "a.tw", start: 5, message: "`cur` is only an alias for `env`",
            rule: "direct-rebinding", edits: [],
          },
        ]
        out := lint.render_report(findings, Dict.new(), false)
        // direct-rebinding sorts before unreachable-code (index_of returns Int?)
        dr := try out.index_of("direct-rebinding").ok_or("direct-rebinding missing")
        uc := try out.index_of("unreachable-code").ok_or("unreachable-code missing")
        try assert.is_true(dr < uc)
        // brief appears in the header
        try assert.is_true(out.contains("rebind the field/index path directly"))
        // the finding line is present and indented
        try assert.is_true(out.contains("\n  a.tw:"))
        // default mode does not include the detailed block
        try assert.is_true(!out.contains("// Avoid"))
        .Ok({})
      },
    )
```

Note: `Dict.new()` is the empty line-index map; with no index, `location` renders `path:?:?`, which is fine — the test asserts on grouping, not line numbers.

- [x] **Step 2: Run test to verify it fails**

Run: `TWK_TEST_FILTER=lint_command target/twk run boot/tests/main.tw 2>&1 | tail -15`
Expected: compile error — `Finding` not accessible (`type Finding` is private) and `render_report` undefined.

- [x] **Step 3: Make `Finding` public and add the renderer**

In `boot/commands/lint.tw`, add the registry import after the other `use` lines:

```tw
use compiler.lint_rules
```

Change the `Finding` type to `pub` (line ~27):

```tw
pub type Finding = .{ path: String, start: Int, message: String, rule: String, edits: Vector<FixEdit> }
```

Replace the existing `report_findings` function with the grouped, pure renderer plus a thin IO wrapper:

```tw
/// Indent every line of `text` by `prefix`.
fn indent_block(text: String, prefix: String) String {
  out := ""
  first := true

  for line in text.lines() {
    sep := if first { "" } else { "\n" }
    out = "${out}${sep}${prefix}${line}"
    first = false
  }

  out
}

/// Distinct rule ids present in `findings`, sorted alphabetically.
fn distinct_rules(findings: Vector<Finding>) Vector<String> {
  seen: Dict<String, Bool> = Dict.new()
  rules: Vector<String> = []

  for f in findings {
    if !seen.has(f.rule) {
      seen[f.rule] = true
      rules = .append(f.rule)
    }
  }

  rules.sort_by(String.compare)
}

/// Findings for one rule, sorted by path then start offset.
fn findings_for(findings: Vector<Finding>, rule: String) Vector<Finding> {
  group: Vector<Finding> = []

  for f in findings {
    if f.rule == rule {
      group = .append(f)
    }
  }

  group.sort_by(fn(a, b) {
    case String.compare(a.path, b.path) {
      .Eq => Int.compare(a.start, b.start),
      other => other,
    }
  })
}

/// Render the findings body as a string: grouped by rule, each group headed by
/// `<rule>  <brief>` (and, when `explain`, the detailed rationale), with
/// findings indented underneath. Pure given a prebuilt line-index map.
pub fn render_report(
  findings: Vector<Finding>,
  indexes: Dict<String, line_index.LineIndex>,
  explain: Bool,
) String {
  out := ""
  first_group := true

  for rule in distinct_rules(findings) {
    brief := case lint_rules.describe(rule) {
      .Some(info) => info.brief,
      .None => "",
    }
    header := if brief.len() > 0 { "${rule}  ${brief}" } else { rule }
    group_sep := if first_group { "" } else { "\n\n" }
    out = "${out}${group_sep}${header}"
    first_group = false

    if explain {
      case lint_rules.describe(rule) {
        .Some(info) => out = "${out}\n\n${indent_block(info.detailed, "  ")}\n",
        .None => {},
      }
    }

    for f in findings_for(findings, rule) {
      out = "${out}\n  ${location(f.path, f.start, indexes)}: ${f.message}"
    }
  }

  out
}
```

- [x] **Step 4: Run test to verify it passes**

Run: `TWK_TEST_FILTER=lint_command target/twk run boot/tests/main.tw 2>&1 | tail -6`
Expected: all `lint_command` tests pass.

- [x] **Step 5: Commit**

```bash
git add boot/commands/lint.tw boot/tests/suites/lint_command_suite.tw
git commit -m "Add grouped pure render_report for lint findings"
```

---

## Task 3: `--explain` expands the detailed block

**Files:**
- Test: `boot/tests/suites/lint_command_suite.tw` (renderer already supports `explain`; this proves it)

- [x] **Step 1: Write the failing test**

Add to `suite()`:

```tw
    .test(
      "render_report: --explain includes the detailed rationale",
      fn() {
        findings := [
          lint.Finding.{
            path: "a.tw", start: 5, message: "`cur` is only an alias for `env`",
            rule: "direct-rebinding", edits: [],
          },
        ]
        plain := lint.render_report(findings, Dict.new(), false)
        explained := lint.render_report(findings, Dict.new(), true)
        try assert.is_true(!plain.contains("// Avoid"))
        try assert.is_true(explained.contains("// Avoid"))
        try assert.is_true(explained.contains("// Prefer"))
        // detailed block is indented under the header
        try assert.is_true(explained.contains("\n      // Avoid"))
        .Ok({})
      },
    )
```

Note: `// Avoid` is authored flush-left in the registry; the renderer prepends 2 spaces, and the example body lines are themselves indented 4 spaces in the literal, so the rendered line is `\n      // Avoid` (6 leading spaces).

- [x] **Step 2: Run test to verify it fails or passes**

Run: `TWK_TEST_FILTER=lint_command target/twk run boot/tests/main.tw 2>&1 | tail -8`
Expected: PASS (the renderer from Task 2 already implements `explain`). If it fails on the exact indentation assertion, adjust the expected leading-space count in the test to match the registry literal's indentation — do not change the registry to satisfy a miscount.

- [x] **Step 3: Commit**

```bash
git add boot/tests/suites/lint_command_suite.tw
git commit -m "Cover --explain detailed rationale rendering"
```

---

## Task 4: Wire `--explain` through the command

**Files:**
- Modify: `boot/main.tw` (register flag)
- Modify: `boot/commands/lint.tw` (`run_lint_command`, `print_summary`)

- [x] **Step 1: Register the flag in `main.tw`**

In `boot/main.tw`, extend the `lint_cmd` builder (currently ends at `.add_flag("fix-inherent-calls", ...)`):

```tw
lint_cmd := file_command("lint", "Review code: report lints and rewrites").add_flag(
  "fix",
  "Apply all auto-fixable rewrites",
).add_flag("fix-unused-imports", "Apply only the unused-import rewrite").add_flag(
  "fix-inherent-calls",
  "Apply only the inherent-method-call rewrite",
).add_flag("explain", "Show the full rationale for each reported rule")
```

- [x] **Step 2: Thread `explain` into the command and renderer**

In `boot/commands/lint.tw`, update `print_summary` to take an `explain` flag and print the hint, and replace the two `report_findings(...)` + `print_summary(...)` call sites.

Change `print_summary`'s signature and add the hint at its end:

```tw
fn print_summary(findings: Vector<Finding>, explain: Bool) {
```

Add, just before `print_summary`'s closing brace (after the `if fixable > 0 { ... }` block):

```tw
  if !explain {
    eprintln("run `twk lint --explain` for the full rationale")
  }
```

In `run_lint_command`, read the flag near the other flags:

```tw
  explain := parsed.has_flag("explain")
```

Replace the report block in the no-apply branch:

```tw
    println(render_report(findings, index_sources(findings), explain))
    print_summary(findings, explain)
    proc.exit(1)
```

Replace the trailing report block (after applying fixes):

```tw
  if remaining.len() > 0 {
    println(render_report(remaining, index_sources(remaining), explain))
    print_summary(remaining, explain)
  }
```

- [x] **Step 3: Verify the boot suite still builds and passes**

Run: `TWK_TEST_FILTER=lint_command target/twk run boot/tests/main.tw 2>&1 | tail -6`
Expected: all `lint_command` tests pass (the renderer/registry units are unchanged; this confirms the command file still compiles with the new signatures).

- [x] **Step 4: Commit**

```bash
git add boot/main.tw boot/commands/lint.tw
git commit -m "Wire --explain flag into the lint command"
```

---

## Task 5: Trim CLAUDE.md to a pointer

**Files:**
- Modify: `CLAUDE.md`

- [x] **Step 1: Edit the Immutability and Rebinding section**

Keep the *semantics* (the opening paragraph and the `p.x = 1 → RecordUpdate(...)` / `arr[i]=v` / `m[k]=v` desugaring code block). Remove the *prescriptive* "Do NOT write `with_*` helpers" block and its `❌ Don't write this / ✅ Write this instead` worked example. Replace that removed prescriptive block with:

```markdown
The "rebind the field/index path directly, don't write `with_*` copy helpers"
guidance is enforced by `twk lint` (rules `direct-rebinding` and
`record-copy-helper`). Run `twk lint <entry>` after editing; use
`twk lint --explain` for the worked Avoid/Prefer examples.
```

- [x] **Step 2: Add the lint step to the dev-flow guidance**

In the "Format Twinkle source" section (which tells you to run `target/twk fmt`), add a sibling note:

```markdown
After editing a `.tw` file, also run the linter to catch house-rule violations:
```bash
target/twk lint boot/main.tw   # or the relevant entry file
```
The linter is report-only by default; `--explain` prints the rationale for each
rule that fired.
```

- [x] **Step 3: Verify the section reads correctly**

Run: `git diff CLAUDE.md`
Expected: semantics + desugaring block retained; prescriptive `with_*` block replaced by the pointer; dev-flow lint hint added. No other sections touched.

- [x] **Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "Trim CLAUDE.md rebinding prescription to a twk lint pointer"
```

---

## Task 6: Rebundle, dogfood, full verification

**Files:** none (build + verify)

- [x] **Step 1: Run the full boot suite from source**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -4`
Expected: `Ran NNNN tests: NNNN passed` (0 failed).

- [x] **Step 2: Rebundle the CLI**

Run: `make bundle-cli 2>&1 | tail -5`
Expected: `Fixed point reached: stage3 == stage4` then `Built Deno Twinkle CLI: target/twk`.

- [x] **Step 3: Dogfood default report**

Run: `target/twk lint boot/main.tw 2>&1 | head -30`
Expected: findings grouped under rule headers (e.g. `direct-rebinding  the temporary only aliases…`), each finding indented `  path:line:col: message`; footer ends with `run \`twk lint --explain\` for the full rationale`.

- [x] **Step 4: Dogfood `--explain`**

Run: `target/twk lint --explain boot/main.tw 2>&1 | head -40`
Expected: each rule header followed by the indented detailed rationale (the `direct-rebinding` group shows the Avoid/Prefer block), then its findings. No `run --explain` hint line.

- [x] **Step 5: Commit any formatting fixups**

Run: `target/twk fmt boot/compiler/lint_rules.tw boot/commands/lint.tw boot/tests/suites/lint_command_suite.tw`
Then if anything changed:

```bash
git add -A
git commit -m "Apply twk fmt to lint-explanation sources"
```

---

## Self-Review Notes

- **Spec coverage:** registry module (Task 1) ↔ spec §"Rule registry"; pure grouped renderer (Task 2) ↔ §"Report shape" + §"Report rendering"; `--explain` (Tasks 3–4) ↔ §"Flag plumbing" + delivery model; CLAUDE.md trim keeping semantics (Task 5) ↔ §"CLAUDE.md trim"; coverage guard + render tests (Tasks 1–3) ↔ §"Testing"; multi-line `\\` authoring ↔ §"Rule registry".
- **Type consistency:** `RuleInfo { brief, detailed }`, `describe(rule) RuleInfo?`, `render_report(findings, indexes, explain) String`, `print_summary(findings, explain)`, `Finding` made `pub` — names used identically across Tasks 1–4.
- **Non-goals respected:** no numeric codes, no standalone `--explain <rule>` lookup, no stage0/Rust change (lint output is boot-only).
