# Self-Describing Diagnostics + Fix-its (v1) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a diagnostic carry its own machine-applicable fix from a single source of truth, surfaced both in the terminal (rustc-style `= help:` preview) and in the editor (LSP quick-fix), with "add missing case arms" as the flagship fix and the existing unused-import fix migrated onto the same generic rail.

**Architecture:** A `fixes(kind)` projection (parallel to the existing `message(kind)` / `help_lines(kind)` / `to_report(kind, ctx)` projections of `DiagKind`) is the single producer of structured fixes. `to_report` renders each fix into `help_lines` for the CLI; the analyzer serializes each fix into a uniform `data.fixes` JSON for the LSP, and one generic `fix_actions` code-action builder consumes it. No new field is added to `Report` (it has 52 construction sites); fixes ride the existing `help_lines` channel on the CLI side.

**Tech Stack:** Twinkle (`boot/` self-hosted compiler), boot test suite (`boot/tests/`), `make boot-test`, `make bundle-cli`.

---

## Background: how the pieces fit today

Two diagnostic representations exist:

- `lib.source.diagnostics.DiagKind` — rich structured compiler diagnostics. Pure
  projections live beside it: `message(kind)`, `help_lines(kind)`, `span(kind)`.
- `compiler.query.diagnostics.Diagnostic` — flatter struct carrying
  `span: Span?`, `severity`, `message: String`, and `data: Json?`. This is what
  reaches the LSP (`lib.lsp.diagnostics.diagnostic_to_json` copies `data` into
  the published diagnostic).

Rendering:

- `compiler.query.diag_render.to_report(kind, ctx) Report` maps a `DiagKind` to a
  `lib.source.report.Report` (title, span labels, `help_lines`).
- `lib.source.render.render(report, reg, config) String` renders a `Report` to a
  terminal string, including a `= help: <text>` block for each `help_lines` entry.

Today only the unused-import fix flows structured data end-to-end:
`analyze.convert_unused_import_diags` builds a bespoke
`{ kind: "unused_import", use_start, use_end, replacement }` JSON onto `data`, and
`code_action.unused_import_actions` decodes that specific shape. Every other
diagnostic has `data: .None` (set by `analyze.wrap_diags`).

The `MissingVariants` diagnostic currently carries
`{ span, scrutinee_ty, missing: Vector<String> }` and renders the prose help line
`"add arm(s): .Foo, .Bar"` — no applicable edit.

Key facts confirmed in the code:

- `check_exhaustiveness(scrut_ty, arms, s, ctx, diags)` is called as
  `synth_case(scrut, arms, expr.span, ...)` and `check_case(..., s, ...)`, so `s`
  is the **whole case expression span**. Therefore the byte just before the
  closing `}` is `s.end - 1`.
- Variant arity is available at the emission site: `ctx.env.lookup_type_def(tid)`
  returns `.Sum(_, _, variants)` where each
  `resolver.ResolvedVariant = .{ name: String, fields: Vector<MonoType> }`, so
  `arity = v.fields.len()`. Builtins: `Optional` → `Some`(1)/`None`(0);
  `Result` → `Ok`(1)/`Err`(1); `Bool` → `true`(0)/`false`(0).

## Uniform `data.fixes` JSON contract

Both fixes serialize to the same shape on the `data` field:

```json
{
  "fixes": [
    {
      "title": "add missing arms",
      "edits": [
        { "start": 42, "end": 42, "replacement": "  .Circle(value0) => {},\n" }
      ]
    }
  ]
}
```

`start == end` means an insertion. Byte offsets are into the source text; the LSP
edge converts them to ranges via `byte_range_to_lsp_range`.

## File structure

- **Create:** none.
- **Modify:**
  - `boot/lib/source/report.tw` — add `FixEdit`, `SuggestedFix` types.
  - `boot/lib/source/diagnostics.tw` — change `MissingVariants` payload; update
    `message`/`help_lines`; add `fixes(kind)`, `missing_arm_text`,
    `fix_preview_lines` helpers.
  - `boot/compiler/checker.tw` — enrich `MissingVariants` emission (arity +
    `insert_at`) via a new `get_variant_specs` helper.
  - `boot/compiler/query/diag_render.tw` — `MissingVariants` arm renders fix
    preview into `help_lines`.
  - `boot/compiler/query/analyze.tw` — generic `data` attachment from `fixes`;
    add `fixes_to_json`; rewrite `convert_unused_import_diags` to build a
    `SuggestedFix` and serialize through the same path.
  - `boot/lib/lsp/code_action.tw` — add generic `fix_actions`; remove
    `unused_import_actions`.
  - `boot/lib/lsp/server_core.tw` — call `fix_actions`.
- **Test:**
  - `boot/tests/suites/diag_render_suite.tw` — fix preview in `help_lines`.
  - `boot/tests/suites/fix_suite.tw` (new) — `fixes(kind)` projection +
    `fixes_to_json` round-trip + `fix_actions` output, including unused-import
    parity.
  - Register the new suite in `boot/tests/main.tw`.

## Conventions for this plan

- Run a single suite: `TWK_TEST_FILTER='<substring>' target/twk run boot/tests/main.tw`.
- Run all boot tests: `make boot-test`.
- This is boot-only: `MissingVariants`/`UnusedImport` and the rendering live in
  `boot/`; stage0 (Rust) defines its own diagnostics and only needs to *compile*
  `boot/main.tw`, which a boot-internal payload change does not affect. After the
  boot tests pass, run `make bundle-cli` to confirm the self-host loop is green.
- Commit after each task.

---

### Task 1: Add `FixEdit` and `SuggestedFix` types

**Files:**
- Modify: `boot/lib/source/report.tw`

- [ ] **Step 1: Add the two types**

In `boot/lib/source/report.tw`, after the `SpanLabel` type (around line 29) and
before the `Report` type, add:

```tw
/// A single text edit: replace bytes `[start, end)` with `replacement`.
/// An insertion is expressed as `start == end`.
pub type FixEdit = .{ start: Int, end: Int, replacement: String }

/// A named, machine-applicable fix made of one or more edits.
/// A diagnostic may offer several alternative fixes.
pub type SuggestedFix = .{ title: String, edits: Vector<FixEdit> }
```

Do **not** add a field to `Report`.

- [ ] **Step 2: Verify it still builds**

Run: `target/twk check boot/main.tw`
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add boot/lib/source/report.tw
git commit -m "Add SuggestedFix/FixEdit types for self-describing diagnostics"
```

---

### Task 2: Enrich the `MissingVariants` payload

The payload must carry each missing variant's arity and the byte offset where new
arms are inserted, so the fix is a pure projection of the payload.

**Files:**
- Modify: `boot/lib/source/diagnostics.tw:27` (payload), `:180` (`message`),
  `boot/compiler/query/diag_render.tw:111` (`help_lines` in the Report)
- Modify: `boot/compiler/checker.tw` (emission site + new helper)

- [ ] **Step 1: Change the payload type**

In `boot/lib/source/diagnostics.tw`, change the `MissingVariants` variant from:

```tw
  MissingVariants(.{ span: Span, scrutinee_ty: MonoType, missing: Vector<String> }),
```

to:

```tw
  MissingVariants(.{
    span: Span,
    scrutinee_ty: MonoType,
    missing: Vector<.{ name: String, arity: Int }>,
    insert_at: Int,
  }),
```

- [ ] **Step 2: Update `message()` to read `.name`**

In `boot/lib/source/diagnostics.tw`, the `MissingVariants` arm of `message()`
(around line 180) currently does `d.missing.join(", ")`. Change it to join names:

```tw
      .MissingVariants(d) => "non-exhaustive match on `${ty_to_string(d.scrutinee_ty)}`, missing: ${(
        collect v in d.missing { v.name }
      ).join(", ")}",
```

- [ ] **Step 3: Add `get_variant_specs` helper in the checker**

In `boot/compiler/checker.tw`, immediately after `get_variant_names` (ends around
line 3601), add a sibling that also returns arities:

```tw
fn get_variant_specs(scrut_ty: MonoType, ctx: InferCtx) Vector<.{ name: String, arity: Int }>? {
  zonked := zonk(scrut_ty, ctx.subst)

  case zonked {
    .Optional(_) => .Some([.{ name: "Some", arity: 1 }, .{ name: "None", arity: 0 }]),
    .Result(_, _) => .Some([.{ name: "Ok", arity: 1 }, .{ name: "Err", arity: 1 }]),
    .Bool => .Some([.{ name: "true", arity: 0 }, .{ name: "false", arity: 0 }]),
    .Named(tid, _) => {
      def := try ctx.env.lookup_type_def(tid)

      case def {
        .Sum(_, _, variants) => .Some(
          collect v in variants {
            .{ name: v.name, arity: v.fields.len() }
          },
        ),
        _ => .None,
      }
    },
    _ => .None,
  }
}
```

- [ ] **Step 4: Use specs at the emission site**

In `boot/compiler/checker.tw`, `check_exhaustiveness` (around lines 3531-3577),
replace the body from `variant_names := get_variant_names(scrut_ty, ctx)` through
the emission `case variant_names { ... }` with the version below. The `covered`
collection is unchanged; only the source of names and the emitted payload change:

```tw
  variant_specs := get_variant_specs(scrut_ty, ctx)

  case variant_specs {
    .None => diags,
    .Some(specs) => {
      // Collect covered variant names from arm patterns
      covered := collect arm in arms {
        case arm.pattern.kind {
          .Variant(name, _) => name,
          .QualifiedVariant(path, name, _) => {
            // Only count as covered if qualifier matches scrutinee type
            if !qualifier_matches_scrutinee(path, scrut_ty, ctx) {
              continue
            }

            name
          },
          .Literal(lit_expr) => case lit_expr.kind {
            .BoolLit(val) => if val {
              "true"
            } else {
              "false"
            },
            _ => { continue },
          },
          _ => { continue },
        }
      }

      // Find uncovered variants, preserving their arities
      missing := collect spec in specs {
        if covered.any(fn(c) { c == spec.name }) {
          continue
        }

        spec
      }

      if missing.len() > 0 {
        diags.append(
          .Error(
            .MissingVariants(.{
              span: s,
              scrutinee_ty: zonk(scrut_ty, ctx.subst),
              missing,
              insert_at: s.end - 1,
            }),
          ),
        )
      } else {
        diags
      }
    },
  }
```

`get_variant_names` may now be unused. If `target/twk check` reports it unused,
delete it; otherwise leave it.

- [ ] **Step 5: Update the `help_lines` reference in `diag_render`**

In `boot/compiler/query/diag_render.tw`, the `MissingVariants` arm (around line
111) builds a Report whose `help_lines` does `d.missing.join(", ")`. Replace that
arm temporarily with a names-only join so the project compiles; Task 4 rewrites it
to render the fix preview:

```tw
    .MissingVariants(d) => .{
      severity: .Error,
      title: "non-exhaustive match on `${fmt_ty(d.scrutinee_ty, ctx)}`",
      labels: [primary(d.span, .Some("missing variants"))],
      help_lines: ["add arm(s): ${(collect v in d.missing { v.name }).join(", ")}"],
    },
```

- [ ] **Step 6: Verify the project builds**

Run: `target/twk check boot/main.tw`
Expected: no errors (other than possibly an unused `get_variant_names`, handled in
Step 4).

- [ ] **Step 7: Run the existing diag suites to confirm no regression**

Run: `TWK_TEST_FILTER='diag' target/twk run boot/tests/main.tw`
Expected: all pass (message wording for `MissingVariants` is unchanged in output).

- [ ] **Step 8: Commit**

```bash
git add boot/lib/source/diagnostics.tw boot/compiler/checker.tw boot/compiler/query/diag_render.tw
git commit -m "Enrich MissingVariants payload with variant arity and insert offset"
```

---

### Task 3: Add the `fixes(kind)` projection and preview helpers

This is the single source of truth for structured fixes.

**Files:**
- Modify: `boot/lib/source/diagnostics.tw`
- Test: `boot/tests/suites/fix_suite.tw` (new), `boot/tests/main.tw`

- [ ] **Step 1: Write the failing test**

Create `boot/tests/suites/fix_suite.tw`:

```tw
use lib.source.diagnostics as diag
use lib.source.diagnostics.{DiagKind}
use lib.source.span
use tests.assert
use tests.runner

fn s(start: Int, end: Int) span.Span {
  span.new(0, start, end)
}

fn missing_variants_kind() DiagKind {
  DiagKind.Error(
    .MissingVariants(.{
      span: s(0, 30),
      scrutinee_ty: compiler_mono_type_int(),
      missing: [.{ name: "Circle", arity: 1 }, .{ name: "UnitSquare", arity: 0 }],
      insert_at: 29,
    }),
  )
}

// Use Int as a stand-in scrutinee type; fixes() never inspects scrutinee_ty.
fn compiler_mono_type_int() compiler.mono_type.MonoType {
  compiler.mono_type.MonoType.Int
}

pub fn suite() runner.Suite {
  runner.suite("fix")
    .test(
      "fixes: missing variants produces one fix with arm-text edits",
      fn() {
        fixes := diag.fixes(missing_variants_kind())
        try assert.equal(fixes.len(), 1)
        fix := fixes[0]
        try assert.equal(fix.title, "add missing arms")
        try assert.equal(fix.edits.len(), 1)
        edit := fix.edits[0]
        try assert.equal(edit.start, 29)
        try assert.equal(edit.end, 29)
        try assert.equal(
          edit.replacement,
          "  .Circle(value0) => {},\n  .UnitSquare => {},\n",
        )
        .Ok({})
      },
    )
}
```

Add `use compiler.mono_type` to the import block (placed before the
`compiler_mono_type_int` helper reference) — adjust the helper to use the alias:
replace `compiler.mono_type.MonoType` references with `mono_type.MonoType` and add
`use compiler.mono_type as mono_type` at the top.

Register the suite in `boot/tests/main.tw`: add `use .suites.fix_suite` with the
other `use .suites.*` lines (alphabetical, near `diag_render_suite`), and add
`fix_suite.suite(),` to the suite list (the `[ ... ]` array around line 113+).

- [ ] **Step 2: Run the test to verify it fails**

Run: `TWK_TEST_FILTER='fix::' target/twk run boot/tests/main.tw`
Expected: FAIL — `fixes` is not defined on `diag`.

- [ ] **Step 3: Implement `fixes` and the arm-text helper**

In `boot/lib/source/diagnostics.tw`, add `use .report.{SuggestedFix, FixEdit}` to
the import block (next to `use .registry` / `use .span.{Span}`). Then, after
`help_lines()` (around line 256), add:

```tw
/// Render a single case arm with placeholder bindings for its payload.
/// Arity 0 → `  .Foo => {},`; arity N → `  .Bar(value0, value1) => {},`.
/// Fixed 2-space indent; `twk fmt` normalizes afterward.
pub fn missing_arm_text(name: String, arity: Int) String {
  binders := collect i in range(arity) {
    "value${i.to_string()}"
  }
  payload := if arity == 0 {
    ""
  } else {
    "(${binders.join(", ")})"
  }
  "  .${name}${payload} => {},\n"
}

/// Structured, machine-applicable fixes for a diagnostic. Pure projection of the
/// payload — the single source of truth consumed by both the terminal renderer
/// (via fix_preview_lines) and the LSP data channel (via serialization).
pub fn fixes(kind: DiagKind) Vector<SuggestedFix> {
  case kind {
    .Error(e) => case e {
      .MissingVariants(d) => {
        arms := collect v in d.missing {
          missing_arm_text(v.name, v.arity)
        }
        replacement := arms.join("")
        [SuggestedFix.{
          title: "add missing arms",
          edits: [FixEdit.{ start: d.insert_at, end: d.insert_at, replacement }],
        }]
      },
      _ => [],
    },
    .Warning(_) => [],
  }
}

/// Human-readable preview lines for a fix, for terminal `= help:` rendering.
/// Title line followed by each replacement line (trailing newline trimmed).
pub fn fix_preview_lines(fix: SuggestedFix) Vector<String> {
  lines: Vector<String> = [fix.title + ":"]

  for edit in fix.edits {
    for piece in edit.replacement.split("\n") {
      trimmed := piece.trim()

      if trimmed != "" {
        lines = .append("    ${trimmed}")
      }
    }
  }

  lines
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `TWK_TEST_FILTER='fix::' target/twk run boot/tests/main.tw`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add boot/lib/source/diagnostics.tw boot/tests/suites/fix_suite.tw boot/tests/main.tw
git commit -m "Add fixes() projection and fix preview/arm-text helpers"
```

---

### Task 4: Render the fix preview into CLI `help_lines`

The terminal renderer already prints `help_lines`. Feed the fix preview into them
so `twk check`/`build` shows the suggested arms.

**Files:**
- Modify: `boot/compiler/query/diag_render.tw`
- Test: `boot/tests/suites/diag_render_suite.tw`

- [ ] **Step 1: Write the failing test**

In `boot/tests/suites/diag_render_suite.tw`, add a test inside `suite()` (chain a
new `.test(...)`). Note the existing `ctx()` helper and `s(start, end)` helper at
the top of that file:

```tw
    .test(
      "MissingVariants: help lines include the fix preview arms",
      fn() {
        kind := DiagKind.Error(
          .MissingVariants(.{
            span: s(0, 30),
            scrutinee_ty: MonoType.Int,
            missing: [.{ name: "Circle", arity: 1 }, .{ name: "UnitSquare", arity: 0 }],
            insert_at: 29,
          }),
        )
        r := diag_render.to_report(kind, ctx())
        try assert.equal(r.help_lines[0], "add missing arms:")
        try assert.equal(r.help_lines[1], ".Circle(value0) => {},")
        try assert.equal(r.help_lines[2], ".UnitSquare => {},")
        .Ok({})
      },
    )
```

The `fix_preview_lines` helper indents preview lines with four spaces; the render
layer renders `= help: <line>`. Assert on the trimmed content the test expects:
adjust the expected strings to match `fix_preview_lines` output exactly, which is
`"    .Circle(value0) => {},"` (four leading spaces). Update the asserts to:

```tw
        try assert.equal(r.help_lines[0], "add missing arms:")
        try assert.equal(r.help_lines[1], "    .Circle(value0) => {},")
        try assert.equal(r.help_lines[2], "    .UnitSquare => {},")
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `TWK_TEST_FILTER='help lines include the fix preview' target/twk run boot/tests/main.tw`
Expected: FAIL — current `help_lines` is `["add arm(s): .Circle, .UnitSquare"]`.

- [ ] **Step 3: Render the fix preview in `to_report`**

In `boot/compiler/query/diag_render.tw`, add `use lib.source.diagnostics as diag`
to the import block (it already imports `DiagKind`/`ErrorDiag`/`WarningDiag` from
that module; add the module alias). Replace the `MissingVariants` arm (edited in
Task 2 Step 5) with one that renders the fix preview. `Vector` has no `flatten`
(confirmed), so build the help lines with an explicit nested loop:

```tw
    .MissingVariants(d) => {
      help: Vector<String> = []
      for fix in diag.fixes(.Error(.MissingVariants(d))) {
        for line in diag.fix_preview_lines(fix) {
          help = .append(line)
        }
      }
      .{
        severity: .Error,
        title: "non-exhaustive match on `${fmt_ty(d.scrutinee_ty, ctx)}`",
        labels: [primary(d.span, .Some("missing variants"))],
        help_lines: help,
      }
    },
```

Verify with `target/twk check boot/main.tw`.

- [ ] **Step 4: Run the test to verify it passes**

Run: `TWK_TEST_FILTER='help lines include the fix preview' target/twk run boot/tests/main.tw`
Expected: PASS.

- [ ] **Step 5: Manual end-to-end CLI check**

Create `/tmp/exhaust.tw`:

```tw
type Shape = { Circle(Float), Rect(Float, Float), UnitSquare }

s := Shape.Circle(1.0)
x := case s {
  .Circle(r) => 1,
}
```

Run: `target/twk check /tmp/exhaust.tw`
Expected: the error report ends with help lines listing the missing arms, e.g.:

```
  = help: add missing arms:
  = help:     .Rect(value0, value1) => {},
  = help:     .UnitSquare => {},
```

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/query/diag_render.tw boot/tests/suites/diag_render_suite.tw
git commit -m "Render missing-arm fix preview in terminal diagnostics"
```

---

### Task 5: Serialize fixes into the uniform `data.fixes` JSON

Attach structured fixes to every diagnostic's `data` so the LSP can consume them.

**Files:**
- Modify: `boot/compiler/query/analyze.tw`
- Test: `boot/tests/suites/fix_suite.tw`

- [ ] **Step 1: Write the failing test**

In `boot/tests/suites/fix_suite.tw`, add `use lib.json` and a test that exercises
`fixes_to_json`. Because `fixes_to_json` will be defined in `analyze.tw`, expose
it there as `pub fn`. Add to the import block `use compiler.query.analyze`, then:

```tw
    .test(
      "fixes_to_json: serializes fixes into the data.fixes shape",
      fn() {
        data := analyze.fixes_to_json(diag.fixes(missing_variants_kind()))
        json_data := case data {
          .Some(j) => j,
          .None => { return assert.fail("expected Some(data)") },
        }
        fixes := try json.decode(json_data, json.field("fixes", json.list(json.raw())))
        try assert.equal(fixes.len(), 1)
        title := try json.decode(fixes[0], json.field("title", json.string()))
        try assert.equal(title, "add missing arms")
        edits := try json.decode(fixes[0], json.field("edits", json.list(json.raw())))
        start := try json.decode(edits[0], json.field("start", json.int()))
        try assert.equal(start, 29)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `TWK_TEST_FILTER='fixes_to_json' target/twk run boot/tests/main.tw`
Expected: FAIL — `fixes_to_json` is not defined.

- [ ] **Step 3: Implement `fixes_to_json`**

In `boot/compiler/query/analyze.tw`, add `use lib.source.report as report` to the
import block (it imports `lib.json` and `lib.source.diagnostics as diag` already,
but not `lib.source.report` — confirmed). Then add:

```tw
/// Serialize structured fixes into the uniform `data.fixes` JSON the LSP
/// code-action builder consumes. Returns `.None` when there are no fixes.
pub fn fixes_to_json(fixes: Vector<report.SuggestedFix>) json.Json? {
  if fixes.len() == 0 {
    return .None
  }

  fix_objs := collect fix in fixes {
    edit_objs := collect e in fix.edits {
      json.object([
        json.kv("start", .Int(e.start)),
        json.kv("end", .Int(e.end)),
        json.kv("replacement", .Str(e.replacement)),
      ])
    }
    json.object([
      json.kv("title", .Str(fix.title)),
      json.kv("edits", json.array(edit_objs)),
    ])
  }

  .Some(json.object([json.kv("fixes", json.array(fix_objs))]))
}
```

The JSON builder names (`json.object`, `json.kv`, `json.array`, `.Int`, `.Str`)
match the existing `convert_unused_import_diags` in this file.

- [ ] **Step 4: Run the test to verify it passes**

Run: `TWK_TEST_FILTER='fixes_to_json' target/twk run boot/tests/main.tw`
Expected: PASS.

- [ ] **Step 5: Attach fixes generically in `wrap_diags`**

In `boot/compiler/query/analyze.tw`, `wrap_diags` currently sets `data: .None` for
every diagnostic. Change it to attach fixes:

```tw
fn wrap_diags(
  source: overlay.SourceText,
  stage: String,
  diagnostics: Vector<diag.DiagKind>,
  type_names: Dict<Int, String>,
) Vector<AnalysisDiag> {
  collect d in diagnostics {
    AnalysisDiag.{
      identity: source.identity,
      version: source.version,
      kind: d,
      stage,
      data: fixes_to_json(diag.fixes(d)),
      type_names,
    }
  }
}
```

Ensure `diag.fixes` is in scope (the file already imports
`lib.source.diagnostics as diag`).

- [ ] **Step 6: Run the full diag/fix suites**

Run: `TWK_TEST_FILTER='fix' target/twk run boot/tests/main.tw`
Then: `TWK_TEST_FILTER='diag' target/twk run boot/tests/main.tw`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/query/analyze.tw boot/tests/suites/fix_suite.tw
git commit -m "Serialize and attach structured fixes to diagnostic data"
```

---

### Task 6: Migrate unused-import onto the uniform fix shape

Rewrite the bespoke unused-import JSON to build a `SuggestedFix` and serialize it
through `fixes_to_json`, so both diagnostics share one shape.

**Files:**
- Modify: `boot/compiler/query/analyze.tw` (`convert_unused_import_diags`)

- [ ] **Step 1: Rewrite `convert_unused_import_diags`**

In `boot/compiler/query/analyze.tw`, replace the body of
`convert_unused_import_diags` so it emits the uniform shape:

```tw
fn convert_unused_import_diags(
  source: overlay.SourceText,
  result: unused_imports.UnusedImportResult,
) Vector<AnalysisDiag> {
  collect item in result.items {
    binding := case item.kind {
      .Warning(.UnusedImport(d)) => d.binding,
      _ => "import",
    }
    fix := report.SuggestedFix.{
      title: "remove unused import `${binding}`",
      edits: [report.FixEdit.{
        start: item.edit.use_span.start,
        end: item.edit.use_span.end,
        replacement: item.edit.replacement,
      }],
    }
    AnalysisDiag.{
      identity: source.identity,
      version: source.version,
      kind: item.kind,
      stage: "import",
      data: fixes_to_json([fix]),
      type_names: empty_type_names(),
    }
  }
}
```

Confirm `report.SuggestedFix`/`report.FixEdit` resolve under the alias used in
Task 5 Step 3.

- [ ] **Step 2: Verify build**

Run: `target/twk check boot/main.tw`
Expected: no errors.

- [ ] **Step 3: Add a parity test for the serialized unused-import shape**

In `boot/tests/suites/fix_suite.tw`, add a test that builds a `SuggestedFix` for a
synthetic unused import and asserts it serializes to the same `data.fixes` shape
(title prefix `remove unused import`, one edit with `replacement`):

```tw
    .test(
      "unused-import fix serializes to the uniform shape",
      fn() {
        fix := report.SuggestedFix.{
          title: "remove unused import `foo`",
          edits: [report.FixEdit.{ start: 0, end: 15, replacement: "" }],
        }
        data := case analyze.fixes_to_json([fix]) {
          .Some(j) => j,
          .None => { return assert.fail("expected Some(data)") },
        }
        fixes := try json.decode(data, json.field("fixes", json.list(json.raw())))
        try assert.equal(fixes.len(), 1)
        title := try json.decode(fixes[0], json.field("title", json.string()))
        try assert.equal(title, "remove unused import `foo`")
        .Ok({})
      },
    )
```

Add `use lib.source.report as report` to `fix_suite.tw` imports.

- [ ] **Step 4: Run the test**

Run: `TWK_TEST_FILTER='uniform shape' target/twk run boot/tests/main.tw`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/query/analyze.tw boot/tests/suites/fix_suite.tw
git commit -m "Migrate unused-import diagnostic onto the uniform fix shape"
```

---

### Task 7: Generic `fix_actions` code-action builder

Replace the unused-import-specific decoder with one that consumes `data.fixes`.

**Files:**
- Modify: `boot/lib/lsp/code_action.tw`
- Test: `boot/tests/suites/fix_suite.tw`

- [ ] **Step 1: Write the failing test**

In `boot/tests/suites/fix_suite.tw`, add `use lib.lsp.code_action as code_action`
and `use lib.text.line_index`, then a test that feeds a diagnostic JSON (with a
`data.fixes` payload and a `message`) into `fix_actions` and asserts a CodeAction
is produced:

```tw
    .test(
      "fix_actions: builds a quickfix from data.fixes",
      fn() {
        index := line_index.new("case s {\n  .Circle(r) => 1,\n}\n")
        data := case analyze.fixes_to_json(diag.fixes(missing_variants_kind())) {
          .Some(j) => j,
          .None => { return assert.fail("expected data") },
        }
        diag_json := json.object([
          json.kv("message", .Str("non-exhaustive match")),
          json.kv("data", data),
        ])
        actions := code_action.fix_actions("file:///a.tw", index, [diag_json])
        try assert.equal(actions.len(), 1)
        title := try json.decode(actions[0], json.field("title", json.string()))
        try assert.equal(title, "add missing arms")
        kind := try json.decode(actions[0], json.field("kind", json.string()))
        try assert.equal(kind, "quickfix")
        .Ok({})
      },
    )
```

(`missing_variants_kind`'s `insert_at: 29` must be within the index text length;
the sample text above is long enough.)

- [ ] **Step 2: Run the test to verify it fails**

Run: `TWK_TEST_FILTER='builds a quickfix from data.fixes' target/twk run boot/tests/main.tw`
Expected: FAIL — `fix_actions` is not defined.

- [ ] **Step 3: Implement `fix_actions`**

In `boot/lib/lsp/code_action.tw`, add a generic builder that decodes the uniform
`data.fixes` shape. It reuses the existing `text_edit`, `workspace_edit`,
`code_action`, and `byte_range_to_lsp_range` helpers:

```tw
/// Build quickfix code actions from any diagnostic carrying a `data.fixes`
/// payload (the uniform shape produced by analyze.fixes_to_json). Each fix
/// becomes one CodeAction whose WorkspaceEdit applies all of the fix's edits.
pub fn fix_actions(
  uri: String,
  index: line_index.LineIndex,
  context_diagnostics: Vector<json.Json>,
) Vector<json.Json> {
  actions: Vector<json.Json> = []

  for diag in context_diagnostics {
    data := case json.decode(diag, json.field("data", json.raw())) {
      .Ok(d) => d,
      .Err(_) => { continue },
    }
    fixes := case json.decode(data, json.field("fixes", json.list(json.raw()))) {
      .Ok(fs) => fs,
      .Err(_) => { continue },
    }

    for fix in fixes {
      title := case json.decode(fix, json.field("title", json.string())) {
        .Ok(t) => t,
        .Err(_) => "apply fix",
      }
      edit_objs := case json.decode(fix, json.field("edits", json.list(json.raw()))) {
        .Ok(es) => es,
        .Err(_) => { continue },
      }

      text_edits: Vector<json.Json> = []
      for e in edit_objs {
        start := case json.decode(e, json.field("start", json.int())) {
          .Ok(v) => v,
          .Err(_) => { continue },
        }
        end := case json.decode(e, json.field("end", json.int())) {
          .Ok(v) => v,
          .Err(_) => { continue },
        }
        replacement := case json.decode(e, json.field("replacement", json.string())) {
          .Ok(v) => v,
          .Err(_) => { continue },
        }
        range := byte_range_to_lsp_range(index, start, end)
        text_edits = .append(text_edit(range, replacement))
      }

      if text_edits.len() > 0 {
        ws_edit := workspace_edit(uri, text_edits)
        actions = .append(code_action(title, "quickfix", ws_edit, [diag]))
      }
    }
  }

  actions
}
```

- [ ] **Step 4: Remove `unused_import_actions`**

Delete the `unused_import_actions` function from `boot/lib/lsp/code_action.tw`
(its behavior is now covered by `fix_actions` consuming the migrated unused-import
fix data).

- [ ] **Step 5: Run the test to verify it passes**

Run: `TWK_TEST_FILTER='builds a quickfix from data.fixes' target/twk run boot/tests/main.tw`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add boot/lib/lsp/code_action.tw boot/tests/suites/fix_suite.tw
git commit -m "Add generic fix_actions code-action builder, drop unused_import_actions"
```

---

### Task 8: Wire `fix_actions` into the LSP server

**Files:**
- Modify: `boot/lib/lsp/server_core.tw:457-471` (`handle_code_action`)

- [ ] **Step 1: Replace the call**

In `boot/lib/lsp/server_core.tw`, `handle_code_action` calls
`lsp_code_action.unused_import_actions(...)`. Change it to:

```tw
  actions := lsp_code_action.fix_actions(
    doc.uri,
    doc.index,
    action_params.context_diagnostics,
  )
```

- [ ] **Step 2: Verify build**

Run: `target/twk check boot/main.tw`
Expected: no errors.

- [ ] **Step 3: Run the LSP code-action / diagnostics suites**

Run: `TWK_TEST_FILTER='lsp' target/twk run boot/tests/main.tw`
Expected: all pass. If a suite asserts the old `unused_import_actions` behavior
directly, update it to call `fix_actions` and to feed diagnostics with the new
`data.fixes` shape (mirror the Task 7 test setup).

- [ ] **Step 4: Commit**

```bash
git add boot/lib/lsp/server_core.tw
git commit -m "Wire LSP code actions to the generic fix_actions builder"
```

---

### Task 9: Full verification and self-host

**Files:** none (verification only).

- [ ] **Step 1: Run the entire boot test suite**

Run: `make boot-test`
Expected: all suites pass.

- [ ] **Step 2: Rebuild the self-hosted compiler**

Run: `make bundle-cli`
Expected: the self-host loop completes and `target/twk` is rebuilt without errors.
This confirms the boot-internal payload change does not break bootstrap.

- [ ] **Step 3: Re-run the manual CLI check against the fresh binary**

Run: `target/twk check /tmp/exhaust.tw` (file from Task 4 Step 5)
Expected: the missing-arm fix preview appears in the help lines.

- [ ] **Step 4: Manual LSP smoke (optional but recommended)**

If a test LSP harness or editor is available, open a file with a non-exhaustive
`case` and confirm an "add missing arms" quick-fix appears and applies the arms,
and that unused-import removal still works as a quick-fix.

- [ ] **Step 5: Format touched files**

Run: `make fmt` (or `target/twk fmt <file>` for each touched `.tw` file)
Expected: idempotent; commit any formatting changes.

```bash
git add -A
git commit -m "Format self-describing-diagnostics changes"
```

---

## Self-review checklist (completed by plan author)

- **Spec coverage:** SuggestedFix type (Task 1); single-source `fixes(kind)`
  projection (Task 3); MissingVariants flagship fix with arity-aware arm text
  (Tasks 2-4); CLI preview via `help_lines` with no Report churn (Task 4); uniform
  `data.fixes` JSON + generic serializer (Task 5); unused-import migration proving
  parity (Task 6); single generic `fix_actions` consumer replacing the bespoke
  decoder (Tasks 7-8); both-surfaces requirement met (CLI = Task 4, editor =
  Tasks 7-8); self-host verification (Task 9).
- **Type consistency:** `SuggestedFix.{ title, edits }`, `FixEdit.{ start, end,
  replacement }`, `fixes(kind) Vector<SuggestedFix>`, `fixes_to_json(...) json.Json?`,
  `fix_actions(uri, index, context_diagnostics)` — names used consistently across
  tasks. `MissingVariants` payload `{ span, scrutinee_ty, missing:
  Vector<.{name, arity}>, insert_at }` used consistently in checker, diagnostics,
  diag_render, and tests.
- **Out of scope (follow-ons on the same rail):** A2 auto-import, A3/A4 type
  annotations, A5 redundant-closure-annotations, LSP rename, code-action resolve
  (lazy edits), multi-file edits.

---

## Verified API idioms

These were confirmed against the codebase while writing the plan:

- `collect i in range(arity) { ... }` and `i.to_string()` are valid (both
  `Int.to_string(x)` and `x.to_string()` are used throughout `boot/`).
- `String.split("\n")` and `String.trim()` exist and are used in `boot/lib/source/`.
- `Vector` has **no** `flatten` — Task 4 builds help lines with an explicit loop.
- `analyze.tw` does **not** import `lib.source.report` today — Task 5 adds
  `use lib.source.report as report`; it already imports `lib.json` and
  `lib.source.diagnostics as diag`.
