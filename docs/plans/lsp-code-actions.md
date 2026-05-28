# LSP Code Actions Plan

## Goal

Add practical quick-fix code actions to `twk lsp` that reduce manual editing
for common diagnostics and patterns.

---

## Current Baseline

Implemented:

* `textDocument/codeAction` handler registered and functional.
* `codeActionProvider: true` in server capabilities.
* TextEdit / WorkspaceEdit / CodeAction JSON builders in
  `boot/lib/lsp/code_action.tw`.
* Unused import removal fully implemented as reference: diagnostic carries
  structured `data` (kind, span, replacement text), handler extracts data,
  generates edits, includes bulk "remove all" action.
* Diagnostic `data: json.Json?` field propagated from analyzer through
  diagnostics pipeline to LSP client.
* `byte_range_to_lsp_range` for span-to-LSP-range conversion.

Reference implementation path:
`analyze.tw` (attach data to diagnostic) -> `diagnostics.tw` (propagate) ->
`server_core.tw` (handle_code_action) -> `code_action.tw` (build edits)

---

## Scope

In scope:

* A1: Add missing case arms (exhaustiveness quick-fix)
* A2: Auto-import for unresolved names
* A3: Add type annotations to function signature

* A4: Add type annotations to top-level function declarations
* A5: Remove redundant type annotations from closures

Out of scope:

* Extract variable / extract function refactorings
* Rename symbol
* Advanced refactoring (move module, change signature)
* Code action resolve (lazy edit computation)

---

## Milestones

### A1 — Add Missing Case Arms

Priority: high. Infrastructure is ready — `MissingVariants` diagnostic already
carries the scrutinee type and missing variant names.

**Diagnostic data attachment** (`analyze.tw`):
* In the `MissingVariants` diagnostic path, attach JSON data:
  `{ kind: "missing_variants", case_end: <byte>, missing: [...] }`
* `case_end` points to the closing `}` of the case expression — new arms
  insert before it.

**Code action builder** (`code_action.tw`):
* New `missing_case_arm_actions()` function, parallel to
  `unused_import_actions()`.
* For each diagnostic with `data.kind == "missing_variants"`, generate a
  TextEdit inserting the missing arms (e.g. `  .Foo => {},\n`).
* Single action: "Add missing case arms" that inserts all missing variants.

**Handler integration** (`server_core.tw`):
* `handle_code_action()` calls both `unused_import_actions()` and
  `missing_case_arm_actions()`, concatenating results.

**Challenges:**
* Indentation: need to infer the case arm indentation from context (or use a
  fixed 2-space indent and let `twk fmt` fix it).
* Variant payloads: `Some(T)` needs a placeholder binding
  (`Some(value) => {},`). The `MissingVariants` data currently only has names,
  not arity — may need to look up variant definitions for payload placeholders.

### A2 — Auto-Import for Unresolved Names

Priority: medium. Builds on the module discovery from the completion work.

**Diagnostic data attachment** (`analyze.tw`):
* For unresolved-name errors, attach JSON data:
  `{ kind: "unresolved_name", name: "foo" }`

**Import candidate search** (new query or extension of `completion.tw`):
* Given an unresolved name, search:
  * Exported functions/types from known project modules (already resolved
    during workspace analysis — available in the module graph)
  * Stdlib module exports (parse signature files or use cached resolved envs)
* Return candidate import paths: `[{ path: "lib.json", alias: "json" }, ...]`

**Code action builder** (`code_action.tw`):
* For each candidate, generate a TextEdit inserting a `use` statement at the
  top of the file (after existing `use` declarations).
* Title: `Import "foo" from lib.json` or `Add "use lib.json"`.
* One code action per candidate when ambiguous.

**Challenges:**
* Multi-module workspace: need to search exports across all analyzed modules,
  not just the current file's resolved env.
* Selective vs full import: `use lib.json.{decode}` vs `use lib.json` —
  start with full module import (simpler).
* Insert position: find the last `use` statement span and insert after it, or
  insert at file start if no imports exist.

### A3 — Add Type Annotations to Function Signature

Priority: medium. Function signatures are API contracts and documentation.
Gleam's LSP has a similar action that's well-loved. More valuable than
let-binding annotations because functions are read far more often than written.

**Trigger:** Cursor on a function whose parameters or return type lack explicit
annotations. This is a non-diagnostic code action (source action, not
quick-fix).

**Implementation:**
* In `handle_code_action()`, check if the request range overlaps a function
  declaration in the parsed AST.
* Look up the function's inferred signature from the resolved env
  (`env.lookup_function(decl.name)` -> `FunctionSig`).
* For each unannotated parameter, generate a TextEdit inserting `: Type` after
  the parameter name.
* For a missing return type, generate a TextEdit inserting ` ReturnType` after
  the closing `)` of the parameter list.
* Title: `Add type annotations to function`.
* Kind: `"source"` (not `"quickfix"` — this is a refactoring, not a fix).

**Challenges:**
* Requires parsed AST + resolved env at code action time (same snapshot as
  completion).
* Type rendering: use `ty_to_string_env()` (already available).
* Must detect which params already have annotations vs which are inferred.
* Param spans: need the name-end position of each param to know where to
  insert `: Type`. The AST `Param` node should carry this.
* Return type position: need the `)` position of the param list.

### A4 — Add Type Annotations to Top-Level Function Declarations

Priority: medium. Top-level functions are API boundaries — explicit types
serve as documentation and catch unintended signature changes. Unlike A3
(cursor-triggered source action), this is a diagnostic-driven suggestion that
flags top-level functions with incomplete annotations.

**Trigger:** A top-level `fn` declaration (not nested, not a closure) where
any parameter lacks an explicit type annotation or the return type is omitted.
This is a diagnostic code action (quick-fix), not a cursor-based source action.

**Diagnostic data attachment** (`analyze.tw`):
* Emit a hint-level diagnostic on the function name when annotations are
  incomplete:
  `{ kind: "missing_fn_annotations", name: "foo", span: <fn_name_span> }`
* Severity: `Hint` with `unnecessary` tag — non-intrusive, shows up as a
  suggestion rather than a warning.

**Code action builder** (`code_action.tw`):
* Reuse the same type-rendering logic as A3 (`ty_to_string_env()`).
* For each unannotated parameter, generate a TextEdit inserting `: Type` after
  the parameter name.
* For a missing return type, generate a TextEdit inserting ` ReturnType` after
  the closing `)` of the parameter list.
* Title: `Add type annotations to "foo"`.
* Kind: `"quickfix"`.

**Differences from A3:**
* A3 is a cursor-triggered source action on any function (including local
  `fn`). A4 is a diagnostic-based hint specifically for top-level declarations.
* A4 only fires for functions that are incomplete (at least one param or the
  return type is unannotated). A3 fires whenever the cursor is on a function.

**Challenges:**
* Distinguishing top-level from nested: the analyzer needs to know scope depth
  or check that the function is a direct child of the module.
* Partial annotations: a function might have some params annotated and others
  not — edits should only fill in the missing ones.

### A5 — Remove Redundant Type Annotations from Closures

Priority: low. When a closure is passed to a function whose parameter type is
known (e.g. `xs.map(fn(x: Int) Int { x + 1 })` where `map` expects
`fn(A) B`), the explicit annotations on the closure are redundant — the types
are fully determined by the call context. Removing them reduces noise.

**Trigger:** A closure expression (anonymous `fn`) with explicit type
annotations where all annotated types match the expected type from the calling
context. This is a diagnostic code action (hint-level suggestion).

**Diagnostic data attachment** (`analyze.tw`):
* During type checking, when a closure literal has explicit annotations that
  exactly match the expected function type from context, emit a hint:
  `{ kind: "redundant_closure_annotations", span: <closure_span> }`
* Only fire when **all** annotations are redundant (every param type and the
  return type match the expected type). If the closure has partial annotations
  or the context type is ambiguous, do not suggest removal.

**Code action builder** (`code_action.tw`):
* For each annotated parameter, generate a TextEdit removing the `: Type`
  suffix (keeping just the parameter name).
* For the return type, generate a TextEdit removing the type after `)`.
* Title: `Remove redundant type annotations from closure`.
* Kind: `"quickfix"`.

**Example:**
```tw
// Before (redundant — map's signature determines the types):
xs.map(fn(x: Int) Int { x + 1 })

// After:
xs.map(fn(x) { x + 1 })
```

**Challenges:**
* Context dependency: the expected type must be fully resolved at the closure
  site. If the outer function is generic and the concrete type depends on other
  arguments, the closure's context type might not be known until full
  inference completes.
* Partial redundancy: if only some annotations are redundant, the action
  should not fire — removing some but not all annotations would be confusing.
* Return type omission: need to verify that removing the return type doesn't
  change the inferred type (e.g. when the closure body has multiple return
  paths).

---

## Architecture

All code actions follow the same pattern established by unused imports:

```
Compiler diagnostic (with structured `data` JSON)
  -> LSP diagnostic published to client
  -> Client sends textDocument/codeAction with context diagnostics
  -> Handler extracts `data.kind` and dispatches to builder
  -> Builder returns CodeAction JSON with TextEdit(s)
```

For non-diagnostic actions (A3), the handler inspects the request range
against the AST instead of relying on diagnostic data.

### Files to modify:

| File | Changes |
|------|---------|
| `boot/compiler/query/analyze.tw` | Attach `data` to MissingVariants and unresolved-name diagnostics |
| `boot/lib/lsp/code_action.tw` | New builder functions per action type |
| `boot/lib/lsp/server_core.tw` | Dispatch to new builders in `handle_code_action()` |

### Files to read (no changes expected):

| File | Purpose |
|------|---------|
| `boot/compiler/query/completion.tw` | Module discovery for auto-import candidates |
| `boot/lib/source/diagnostics.tw` | MissingVariants diagnostic structure |
| `boot/compiler/unused_imports.tw` | Reference pattern for diagnostic data |

---

## Test Plan

Tests follow the same didOpen -> didChange -> request pattern as completion
tests.

* A1: Source with non-exhaustive case -> codeAction request -> verify TextEdit
  inserts missing arms
* A2: Source with unresolved name that exists in a known module -> codeAction
  request -> verify TextEdit inserts `use` statement
* A3: Source with unannotated function -> codeAction request at function
  position -> verify TextEdit inserts param and return type annotations
* A4: Source with top-level function missing type annotations -> verify
  hint diagnostic is emitted and quick-fix inserts correct annotations
* A5: Source with closure whose annotations match expected context type ->
  verify hint diagnostic and quick-fix removes the redundant annotations

Testing infrastructure note: the existing `open_then_complete` pattern in
`lsp_completion_suite.tw` can be adapted to an `open_then_code_action` helper
that sends a `textDocument/codeAction` request instead of completion.

---

## Risks and Mitigations

* **Edit position accuracy:** Byte offsets must correctly map to LSP ranges.
  Mitigated by reusing `byte_range_to_lsp_range` (proven in unused imports).
* **Indentation mismatch:** Generated code may not match user's style.
  Mitigated by keeping edits minimal and relying on `twk fmt` for cleanup.
* **Stale snapshot:** Code actions use the same stale-fallback as completion.
  Mitigated by the existing `completion_snapshot` pattern.

---

## Exit Criteria

* A1 (missing case arms) generates correct, applicable edits for sum types
  and Option/Result
* A2 (auto-import) suggests correct `use` statements for at least
  project-local modules
* A3 (type annotations) inserts correct inferred types for function params
  and return type
* A4 (top-level annotation hints) emits diagnostics for incompletely
  annotated top-level functions and generates correct quick-fix edits
* A5 (closure annotation removal) detects fully redundant closure annotations
  and generates edits that remove them without changing semantics
* Code actions are validated by protocol-level tests
