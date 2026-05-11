# Rich Diagnostic Reporting

## Goal

Replace the current flat `path:line:col: error: message` diagnostic output with
rich, Gleam/Rust-style terminal reports — source snippets, underlines, colors,
contextual hints — while keeping diagnostic emission cheap and the architecture
modular enough to swap rendering strategies later.

---

## Current Baseline

Three diagnostic layers exist today:

* **Layer 1: `source_diag.Diagnostic`** (`boot/lib/source/diagnostic.tw`)
  `{ span, severity, message, related }` — emitted by checker, resolver, parser.
  Severity is a runtime enum (`Error | Warning | Hint | Info`), message is a
  pre-formatted string. `related` field exists but is always `[]`.

* **Layer 2: `AnalysisDiag`** (`boot/compiler/query/analyze.tw`)
  `{ identity, version, span?, severity, message, stage, data? }` — wraps inner
  diagnostics with module identity and stage tag. Produced by
  `convert_source_diags()`. Also has `convert_unused_import_diags()` which
  populates `data: Json?` with edit metadata (`kind`, `use_start`, `use_end`,
  `replacement`) for LSP code action roundtripping.

* **Layer 3: `query_diagnostics.Diagnostic`** (`boot/compiler/query/diagnostics.tw`)
  `{ uri, version, span?, severity, message, stage, data? }` — replaces
  `identity` with `uri: String`. Produced by `convert_analysis_diags()` from
  Layer 2. This is what the LSP actually consumes via `diagnostic_to_json()`.

* **Pipeline-level diagnostics**: `synthetic_diag()` in `analyze.tw` creates
  diagnostics for pipeline errors (circular imports, file read failures).
  Currently uses `span: .None`, but these errors always originate from a `use`
  statement or file reference that has a source span.

* **CLI rendering** (`boot/commands/common.tw`): `format_analysis_diags()` takes
  `Vector<AnalysisDiag>`, re-reads source from disk, wraps back into raw
  `Diagnostic`, and calls `format_source_diagnostics()` →
  `path:line:col: severity: msg`. Prepends a stage label.

* **Type display**: checker uses `MonoType` (which includes `MetaVar(Int)` for
  inference variables). Types are zonked via `subst` map before display using
  `ty_to_string_env(env, ty)`. `ty_to_string_env` requires `ResolvedEnv` to
  resolve `Named(TypeId, args)` to human-readable names.

* **ANSI color** helpers exist (`boot/lib/argparse/style.tw`) with NO_COLOR
  support, but are unused in diagnostics.

* **`ast_walk.tw`** provides `find_node_in_expr(expr, offset) → NodeContext?` —
  top-down only, returns the deepest matching leaf, no parent/sibling info.
  Only walks `Block`/`Expr`/`Stmt`, not module-level declarations.

---

## Design Principles

1. **Emission stays cheap.** Diagnostic sites emit a typed enum variant carrying
   spans and already-computed values (types, names, counts). No string
   formatting, no context gathering.
2. **Severity is structural.** Error vs Warning is encoded in the type system,
   not a runtime field.
3. **Context gathering is lazy.** Rich context (parent expression, enclosing
   function) is queried at render time via the path-returning AST walk, only
   when rendering is requested.
4. **Rendering is modular.** The Report shape and renderer are separate from
   DiagKind and the query layer, so the rendering strategy can evolve
   independently.
5. **Red/green TDD.** Build new infrastructure with tests alongside the existing
   system, then replace in one shot once feature-complete.

---

## Architecture

### Diagnostic flow (new)

```
Checker / Resolver / Parser
  │  emit DiagKind variants (cheap, just data)
  ▼
Vector<DiagKind>
  │
  ├──→ has_errors() → pipeline decides to continue or stop
  │
  ├──→ CLI renderer (terminal output, optional)
  │      │  calls find_path() on cached AST for context
  │      │  uses ResolvedEnv for type name display
  │      │  pattern matches DiagKind → builds Report
  │      │  renders with ANSI colors
  │      ▼
  │    styled stderr output
  │
  └──→ LSP path
         │  DiagKind → AnalysisDiag (add identity, stage)
         │  AnalysisDiag → query_diagnostics.Diagnostic (identity → uri)
         │  → diagnostic_to_json() → LSP JSON
         │
         └──→ code_action handler reads DiagKind-specific data for quickfixes
```

### Replacing the diagnostic layers

**Layer 1** (`source_diag.Diagnostic`) is replaced by `DiagKind`. Checker,
resolver, and parser emit `Vector<DiagKind>` directly.

**Layer 2** (`AnalysisDiag`) evolves to carry `DiagKind`:

```twinkle
pub type AnalysisDiag = .{
  identity: identity.SourceIdentity,
  version: Int?,
  kind: DiagKind,        // replaces span? + severity + message
  stage: String,
  data: json.Json?,      // retained for LSP code action roundtripping
}
```

`data: Json?` is retained for the LSP path. It is populated at the conversion
boundary when wrapping `DiagKind` into `AnalysisDiag` — e.g., for
`UnusedImport`, the LSP layer derives edit metadata from the variant + source
text and writes it into `data`. The CLI path ignores `data`. This preserves
the current separation where edit metadata is an LSP concern, not a diagnostic
concern.

**Layer 3** (`query_diagnostics.Diagnostic`) must also be updated. The
`convert_analysis_diags()` function extracts severity and message string from
`DiagKind` for LSP JSON serialization, and passes `data` through as today.

`convert_source_diags()` is replaced: the pipeline wraps `DiagKind` values
with identity/stage directly. `convert_unused_import_diags()` is updated to
read from `UnusedImport` variant fields and populate `data: Json?` for LSP.

---

## Key Types

### DiagKind — the diagnostic itself

```twinkle
pub type DiagKind = {
  Error(ErrorDiag),
  Warning(WarningDiag),
}

pub type ErrorDiag = {
  TypeMismatch(.{ span: Span, expected: MonoType, found: MonoType }),
  UndefinedVar(.{ span: Span, name: String }),
  WrongArity(.{ span: Span, expected: Int, found: Int }),
  MissingVariants(.{ span: Span, scrutinee_ty: MonoType, missing: Vector<String> }),
  DuplicateField(.{ span: Span, name: String, first: Span }),
  /// Bridge variant for unmigrated emission sites during transition.
  Generic(.{ span: Span, message: String }),
  // ... more as needed
}

pub type WarningDiag = {
  UnusedImport(.{ span: Span, binding: String }),
  /// Bridge variant for unmigrated emission sites during transition.
  Generic(.{ span: Span, message: String }),
  // ... more as needed
}
```

Note: `MonoType` is the checker's native type representation. Types carried in
DiagKind variants must be **zonked** (MetaVars resolved via `subst` map) at
emission time, since the substitution map won't be available at render time.

Every diagnostic has a `span: Span`. Even pipeline-level errors (circular
imports, file read failures) originate from a `use` statement or file reference
that has a source location. The existing `synthetic_diag()` with `span: .None`
should be updated to capture the triggering `use` span.

Emission:

```twinkle
// Before (current)
diags.append(diag.error(s, "expected ${fmt(expected)}, found ${fmt(found)}"))

// After — zonk types at emission, store as data
diags.append(.Error(.TypeMismatch(.{
  span: s,
  expected: zonk(expected, ctx.subst),
  found: zonk(found, ctx.subst),
})))
```

Helpers:

```twinkle
pub fn has_errors(diags: Vector<DiagKind>) Bool
pub fn span(kind: DiagKind) Span   // every diagnostic has a span
```

### PathWalk — structural AST queries

```twinkle
pub type NodeRef = {
  FnDecl(FunctionDecl),
  TypeDecl(TypeDecl),
  Stmt(Stmt),
  Expr(Expr),
  Arm(CaseArm),
  Pattern(Pattern),
  Block(Block),
}

pub type AstPath = .{
  nodes: Vector<NodeRef>,  // root-most first, deepest last
}

/// Walk from module root through all declarations to the node at `offset`.
pub fn find_path(module: Module, offset: Int) AstPath
```

Note: this requires a new module-level traversal (walking `FunctionDecl`,
`TypeDecl`, etc.) that the existing `ast_walk.tw` does not provide. The existing
`ast_walk` only enters from `Block`/`Expr`/`Stmt`.

Helper methods on AstPath:

```twinkle
pub fn deepest(path: AstPath) NodeRef?
pub fn parent(path: AstPath) NodeRef?
pub fn enclosing_fn(path: AstPath) FunctionDecl?
```

`siblings()` is deferred — no current DiagKind variant has a clear use for it.
Can be added later when a renderer needs it.

### Render context

The render-time context carries what renderers need to produce rich output:

```twinkle
pub type RenderCtx = .{
  registry: FileRegistry,    // source text for snippets
  env: ResolvedEnv?,         // type name resolution (Named(TypeId) → name)
  module: Module?,           // for find_path() AST queries
  config: RenderConfig,      // color, style
}
```

`env` is needed because `ty_to_string_env(env, ty)` resolves
`Named(TypeId, args)` to human-readable names like `Option<Int>`. Without it,
types would render as opaque IDs.

### Report — render-time structure (deferred design)

Shape TBD. Will emerge from implementing the first few renderers. Expected to
contain: title, labeled source spans, help/hint body lines.

---

## Color and Terminal Detection

ANSI color output is controlled by:

1. **NO_COLOR env var** — already supported by `boot/lib/argparse/style.tw`.
   Any non-empty value disables color.
2. **Explicit flag** — the CLI can accept `--no-color` / `--color` to override.

Terminal detection (isatty) is out of scope for now. The Wasm runtime does not
currently expose an isatty host import. Default behavior: color is enabled
unless NO_COLOR is set or `--no-color` is passed. This can be revisited when
a host-level isatty import is available.

---

## Hint and Info Severities

The current `Severity` enum has four levels: `Error`, `Warning`, `Hint`, `Info`.
The new `DiagKind` starts with two wrappers: `Error(ErrorDiag)` and
`Warning(WarningDiag)`.

`Hint` and `Info` are not currently emitted by any checker/resolver/parser code
path — they exist in the type but are unused. They will not be included in the
initial DiagKind. If needed later, they can be added as `Hint(HintDiag)` and
`Info(InfoDiag)` wrappers following the same pattern.

---

## Milestones

### M1 — DiagKind types + has_errors

* [ ] Define `DiagKind`, `ErrorDiag`, `WarningDiag` in `boot/lib/source/diag.tw`
* [ ] Include `Generic` bridge variants in both `ErrorDiag` and `WarningDiag`
* [ ] `has_errors(diags: Vector<DiagKind>) Bool`
* [ ] `span(kind: DiagKind) Span` — extract primary span from any variant
* [ ] Unit tests for `has_errors`, `span`

### M2 — Path-returning AST walk

* [ ] Define `NodeRef`, `AstPath` in `boot/compiler/query/ast_path.tw`
* [ ] Implement module-level traversal: walk `FunctionDecl`, `TypeDecl`,
  `UseDecl` items to find the one containing the offset, then descend into
  its body/block
* [ ] Implement `find_path(module, offset) AstPath` — full root-to-leaf walk
* [ ] Helper methods: `deepest()`, `parent()`, `enclosing_fn()`
* [ ] Unit tests: parse known `.tw` sources, call `find_path` at various
  offsets, verify path contents and helper results

### M3 — Report rendering

Depends on M2 (helpers used in render context, test assertions).

* [ ] Define `Report` type and `render(report, registry, config) String`
* [ ] `RenderConfig`: color (from NO_COLOR), style (Rich/Short)
* [ ] Implement source snippet display with line numbers and gutter
* [ ] Implement span underlines/carets with labels
* [ ] Implement colored severity headers
* [ ] Implement help/hint body lines
* [ ] Unit tests: render known reports, assert output strings (color and no-color)

### M4 — DiagKind → Report renderers

Depends on M1 (DiagKind types) and M3 (Report type + render).

* [ ] Define `RenderCtx` with `registry`, `env?`, `module?`, `config`
* [ ] Implement `to_report(kind, ctx) Report` for initial ErrorDiag variants
  (start with 5–8 high-impact kinds)
* [ ] Each renderer uses `find_path()` to gather context as needed
* [ ] Type display: use `ty_to_string_env(env, ty)` at render time for
  MonoType values; fall back to structural display if `env` is unavailable
* [ ] Implement `to_report` for WarningDiag variants
* [ ] Unit tests per variant: given kind + parsed AST → expected report

### M5 — Pipeline integration + cutover

* [ ] Migrate emission sites: checker, resolver, parser → emit DiagKind
  (zonk MonoType values at emission time)
* [ ] Migrate `AnalysisDiag` to carry `kind: DiagKind` instead of
  `span? + severity + message`; retain `data: Json?` for LSP
* [ ] Update `synthetic_diag()` to capture the triggering `use` span instead
  of `span: .None`
* [ ] Update `convert_unused_import_diags()` — read from `UnusedImport`
  variant fields, populate `data: Json?` for LSP code actions
* [ ] Update `convert_analysis_diags()` in `query/diagnostics.tw` — extract
  severity and message from `DiagKind` for LSP JSON, pass `data` through
* [ ] Remove `convert_source_diags()`
* [ ] Migrate pipeline: thread `Vector<DiagKind>`, use `has_errors()`
* [ ] Migrate CLI output: use Report renderer for terminal
* [ ] Remove old `diagnostic.tw` formatting functions
* [ ] Integration tests: compile known-bad sources, assert rich output
* [ ] Integration tests: verify LSP diagnostic JSON + code actions still work

---

## Test Plan

Unit tests (per milestone):

* M1: `has_errors`, `span` extraction on constructed DiagKind values
* M2: `find_path` on parsed AST modules at various offsets — verify path
  contents, `parent()`, `enclosing_fn()`, `deepest()` results
* M3: `render` on hand-built Reports, assert exact output (with and without
  color)
* M4: `to_report` per DiagKind variant with parsed AST context, assert
  labels/hints/context content

Integration tests (M5):

* Compile `.tw` sources with known errors, capture stderr, assert report format
* Verify NO_COLOR suppresses ANSI escapes
* Verify LSP diagnostic JSON conversion from DiagKind produces equivalent
  output to current system
* Verify LSP code actions (unused import removal) work with new variant-based
  data extraction

---

## Risks and Mitigations

* **Scope creep in DiagKind variants:** Start with 5–8 high-impact error
  variants. `Generic` bridge variant covers unmigrated sites during transition.
* **AST not available at render time:** Pipeline already caches parsed modules
  per file. Verify the cached AST survives until diagnostic rendering.
* **Zonking at emission time:** DiagKind variants carrying MonoType must zonk
  before storing, since `subst` map won't be available later. This is a small
  cost (already done today for string formatting).
* **Module-level walk (M2):** The existing `ast_walk` only walks
  Block/Expr/Stmt. `find_path` must implement a new top-level traversal over
  FunctionDecl/TypeDecl items. This is straightforward but non-trivial.
* **LSP code action continuity (M5):** The unused import quickfix flow depends
  on `data: Json?` in AnalysisDiag. This field is retained; the conversion
  layer populates it from `UnusedImport` variant fields. `code_action.tw`
  continues reading `data` as before — no change needed there.
* **Three-layer migration (M5):** All three diagnostic layers
  (`source_diag.Diagnostic` → `AnalysisDiag` → `query_diagnostics.Diagnostic`)
  must be updated together. The cutover is atomic — old and new cannot mix
  at the layer boundaries.
* **`Generic` exit criterion:** The exit criterion "no string formatting" does
  not apply to `Generic` variants, which are transitional. Tracked: all
  `Generic` usages should be migrated to typed variants eventually, but this
  can happen after the initial cutover.

---

## Future Scope: LSP Code Actions from Diagnostics

Beyond unused import removal (which already works), the typed `DiagKind`
variants open up structured code action support. These are not in scope for the
initial milestones but are natural follow-ups once the infrastructure is in
place.

### High value (clear fix, low ambiguity)

* **Missing match variants** — non-exhaustive `case` → insert missing arms with
  `todo` bodies. `MissingVariants` already carries the variant names. The code
  action computes insertion point from the `case` expression span.
* **Missing record fields** — record literal is missing required fields → insert
  them with placeholder values. The checker already knows which fields are
  missing.
* **Unused variable** — prefix identifier with `_` to suppress warning. Requires
  adding an `UnusedVar` warning variant first.

### Medium value (sometimes actionable)

* **Type mismatch: `T` vs `T?`** — found `Option<T>` where `T` expected or vice
  versa → suggest wrapping with `Some(...)` or unwrapping. Common pattern,
  detectable by inspecting the `expected` and `found` types in `TypeMismatch`.
* **Type mismatch: `T` vs `String`** — suggest `to_string()` or the appropriate
  conversion function from prelude.
* **Duplicate record field** — remove the duplicate entry.
* **Unreachable pattern** — a match arm shadowed by an earlier arm → remove it.

### Lower value (nice-to-have)

* **Undefined variable / typo** — fuzzy match against in-scope names → "did you
  mean `foo_bar`?" with a replace action. Requires scope info (locals + env)
  which is only available at emission time, making it expensive to support
  without violating the cheap-emission principle.
* **Missing import** — reference to a name that exists in another module →
  auto-add `use` statement. Requires a project-wide name index (which modules
  export which names) that doesn't exist today. More of a tooling/LSP feature
  than a diagnostic feature.
* **Wrong arity** — insert placeholder arguments or remove excess ones.
* **Private access** — accessing a non-`pub` name → suggest making it public
  (if in the same project).

The LSP integration pattern for all of these: the `DiagKind` variant carries
the structured data, the LSP conversion layer inspects the variant and populates
`data: Json?` on `AnalysisDiag` with the edit payload, and `code_action.tw`
reads `data` to construct workspace edits — same pattern as unused imports.

---

## Exit Criteria

* All compiler errors/warnings render with source snippets, underlines, and
  colored severity headers in terminal output
* NO_COLOR produces plain text without ANSI escapes
* Diagnostic emission sites contain no string formatting — just typed variants
  (`Generic` bridge is acceptable during transition but tracked for migration)
* LSP diagnostic path produces equivalent information to today, including
  code actions (unused import removal)
* All three diagnostic layers updated: `DiagKind` replaces
  `source_diag.Diagnostic`, `AnalysisDiag` carries `kind: DiagKind` +
  `data: Json?` for LSP, `query_diagnostics.Diagnostic` extracts from
  `DiagKind`
* Old `format_diagnostic*` functions removed
