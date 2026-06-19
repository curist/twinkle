# Lint rule explanations + `twk lint --explain`

> **Status: completed.** `twk lint` now has a registered brief/detailed
> rationale for each emitted rule, grouped reporting by rule, and `--explain`
> expansion. The prescriptive rebinding guidance in CLAUDE.md has been trimmed
> to point at the lint rules.

## Completion

Implemented in the boot compiler. The rule registry lives in
`boot/compiler/lint_rules.tw`; `boot/commands/lint.tw` renders grouped reports
and expands details with `--explain`; `boot/main.tw` registers the flag;
`boot/tests/suites/lint_command_suite.tw` covers the registry, grouping,
`--explain`, and colored headers. CLAUDE.md now points readers to `twk lint` for
the rebinding/no-copy-helper guidance.

## Goal

Make `twk lint` teach, not just flag. Today a finding is `file:line:col:
message` and the only rule grouping is the `--fix-<rule>` footer. A reader who
wants to know *why* a pattern is discouraged has nowhere to look. This feature
attaches a registered rationale to each rule and surfaces it in the report.

The kebab `rule` id (`direct-rebinding`, `unreachable-code`, …) stays the single
canonical identifier — it already drives `--fix-<rule>` and the footer. No
numeric codes are introduced.

## Report shape

Findings are **grouped under per-rule headers**. The rule id and its brief
rationale appear once per group; findings sit indented underneath, so the
finding→rule mapping is unambiguous with no per-line tag.

Each finding line keeps its existing `location: message` form (`location` =
`path:line:col`), just indented under its group header. The brief is a single
line — the renderer does no wrapping; keep briefs short.

Default (`twk lint`):

```
direct-rebinding  the temporary only aliases a value; rebind the path directly
  base_env.tw:290:3: `cur` is only an alias for `env`; rebind `env` directly
  argparse/app.tw:36:3: `out` is only an alias for `a`; rebind `a` directly

unreachable-code  code after a diverging statement never runs
  pipeline.tw:12:3: unreachable code

9 finding(s): 0 auto-fixable, 9 report-only
run `twk lint --explain` for the full rationale
```

With `--explain`, the `detailed` block prints (indented) under each group
header, before the findings; the header brief stays as the one-line summary:

```
direct-rebinding  the temporary only aliases a value; rebind the path directly

  A temporary that only aliases a value, gets rebinding-updates applied, then is
  copied back or returned is just ceremony. Twinkle assignment is rebinding, so:

      // Avoid
      d := reg.by_name
      d[internal] = entry
      reg.by_name = d

      // Prefer
      reg.by_name[internal] = entry

  base_env.tw:290:3: `cur` is only an alias for `env`; rebind `env` directly
  ...
```

Ordering is deterministic: rule groups sorted alphabetically by id; findings
within a group sorted by path then byte offset. (Current output is in discovery
order; grouping requires a defined order.)

The summary footer (counts + `--fix-<rule>` hints) stays on stderr as today. The
"run `twk lint --explain`" hint prints only when there are findings and
`--explain` was not passed.

## Components

### Rule registry — `boot/compiler/lint_rules.tw` (new)

```tw
pub type RuleInfo = .{ brief: String, detailed: String }

pub fn describe(rule: String) RuleInfo?
```

A flat `case rule { "direct-rebinding" => .Some(.{ brief, detailed }), … _ =>
.None }`. It must cover every rule the codebase can emit:

- Lints: `direct-rebinding`, `record-copy-helper`, `unreachable-code`,
  `unused-must-use`.
- Rewrites: `inherent-calls`, `unused-imports`.

`brief` is the one-liner shown in the header. `detailed` is the full rationale —
for `direct-rebinding` and `record-copy-helper` it carries the Avoid/Prefer
worked examples relocated from CLAUDE.md. The module lives in `boot/` so the
self-hosted `twk lint` renders it (no Rust/stage0 change — lint output is
boot-only).

`detailed` strings are authored as **Zig-style `\\` multi-line literals**
(`docs/spec.md` §Multiline String Literals), which fit this content exactly:
marker indentation is excluded from the value, so the block indents to match the
surrounding `case` arm while staying flush-left in the rendered output; and there
is no escape processing, so embedded `"` and `.{ ... }` code examples need no
escaping. The renderer indents the whole block under the header, so authors write
the text flush-left:

```tw
"direct-rebinding" => .Some(.{
  brief: "the temporary only aliases a value; rebind the field/index path directly",
  detailed:
    \\A temporary that only aliases a value, gets rebinding-updates applied, then
    \\is copied back or returned is just ceremony. Twinkle assignment is rebinding:
    \\
    \\    // Avoid
    \\    d := reg.by_name
    \\    d[internal] = entry
    \\    reg.by_name = d
    \\
    \\    // Prefer
    \\    reg.by_name[internal] = entry
  ,
})
```

### Report rendering — `boot/commands/lint.tw`

Replace the flat `report_findings` with a **pure** renderer:

```tw
fn render_report(findings: Vector<Finding>, explain: Bool) String
```

Returning the stdout body as a string makes it unit-testable instead of printing
inline. The command prints the returned string to stdout; the summary footer and
hint remain `eprintln` to stderr (preserving the existing stream split). Grouping
groups by `f.rule`, looks up `describe(rule)` for the header brief (and, when
`explain`, the detailed block), and falls back to id-only if a rule has no entry
(guarded against in testing).

### Flag plumbing — `main.tw`

Add a boolean `--explain` to the `lint` command's argument parsing and thread it
through `run_lint_command` into the renderer. `--explain` takes no argument and
expands all fired rules.

**Non-goal:** a standalone `twk lint --explain <rule>` lookup that prints one
rule's text without running a lint pass. YAGNI; revisit only if asked.

### CLAUDE.md trim

The "Immutability and Rebinding" section has two parts:

1. **Semantics** — assignment is rebinding not mutation; the `p.x = 1 →
   RecordUpdate(p, x, 1)` / `arr[i]=v` / `m[k]=v` desugaring table. This is
   language reference needed to read any Twinkle code. **Keep it.**
2. **Prescription** — the "Do NOT write `with_*` helpers / use field rebinding
   directly" block with worked examples. This is exactly what the
   `direct-rebinding` and `record-copy-helper` rules now encode. **Trim to a
   pointer**: a short line that the rebinding/no-`with_*` rules are enforced by
   `twk lint` (rules `direct-rebinding`, `record-copy-helper`), with the worked
   examples now living in the lint `detailed` text.

Add a dev-flow hint alongside the existing `twk fmt` guidance: after editing a
`.tw` file, run `twk lint <entry>` (and `twk fmt`). The worked Avoid/Prefer
examples are removed from CLAUDE.md and reappear in the registry's `detailed`
strings — single source of truth.

## Testing

In `lint_command_suite` (and/or a dedicated suite):

- **Grouping**: a fixture with findings from two rules renders two headers, each
  brief shown once, findings indented under the right header, deterministic
  order.
- **Brief always on**: default render includes the brief header, not the detailed
  block.
- **`--explain`**: render with `explain: true` includes the detailed block under
  the header.
- **Coverage guard**: `describe(id)` returns `.Some` for every rule id the
  codebase can emit (enumerated list). Catches a new rule shipped without a
  description.

Render is pure, so these assert on the returned string rather than captured IO.

## Out of scope

- Numeric/short rule codes (kebab id is canonical).
- Standalone `--explain <rule>` lookup decoupled from a run.
- Per-rule severity levels or configurable enable/disable (no config — house
  rule of the linter).
- Stage0/Rust changes — lint output is boot-only.

## Relationship to existing work

Builds directly on the shipped linter (`docs/plans/archive/linter.md`): rules L2–L5 +
R1/R2 already emit a stable kebab `rule`. This adds the description layer and the
report grouping on top, and completes the CLAUDE.md consolidation the
`direct-rebinding` rule began (see
`docs/plans/archive/rebinding-through-path-lint.md`).
