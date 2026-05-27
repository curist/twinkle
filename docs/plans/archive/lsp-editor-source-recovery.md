# LSP Editor Source Recovery Plan

Status: archived. The shared editor snapshot and cursor/source context layer is
implemented for completion and signature help. The remaining cross-feature
adoption question is tracked as future LSP follow-up work.

## Goal

Unify how LSP query features handle partially edited or otherwise incomplete
source files. Completion and signature help already use similar ideas — current
source for cursor context, plus the best available semantic cache for lookups —
but the policy and helpers are spread across feature-specific modules.

The goal is to make this an explicit shared editor-query layer so existing and
future LSP features behave consistently while users are typing.

---

## Motivation

Recent LSP work added robust behavior for incomplete source in two different
places:

* Completion uses cursor-hole lexing/reparsing to classify contexts such as
  member access, imports, variants, and general identifiers.
* Signature help uses AST lookup when possible, then falls back to source
  scanning for incomplete calls such as `foo(` or `foo(a,`.
* `server_core.tw` has an editor-oriented snapshot path, currently named
  `completion_snapshot`, that falls back to stale typed/resolved artifacts when
  the current edit cannot fully typecheck.

These pieces solve related problems but are not yet presented as one shared
architecture. That makes future features more likely to duplicate recovery
logic or choose subtly different fallback behavior.

---

## Design Principles

* Use the current document text for syntax and cursor context.
* Use the best available semantic artifacts for lookup:
  `typed.env` first, then `resolved.env`, and stale cache entries where safe.
* Prefer parser/AST paths for complete code.
* Prefer cursor-hole parsing when we need AST shape but must prevent parser
  recovery from consuming text after the cursor.
* Prefer source scanning for constructs the parser cannot represent yet.
* Return empty/null LSP responses rather than failing when recovery is not
  possible.
* Keep feature-specific semantic interpretation in the feature module; share
  cursor-context discovery and snapshot policy.

---

## Proposed Architecture

### Editor snapshot

Move the editor-oriented snapshot recovery logic out of
`boot/lib/lsp/server_core.tw` and into `boot/compiler/query/semantic.tw`.

Add a public helper with an explicit name, for example:

```tw
pub fn snapshot_workspace_for_editor(
  store: cache.Store,
  input: diagnostics.WorkspaceInput,
) SemanticSnapshot
```

Behavior:

1. Try `snapshot_workspace_from_cache` for the fast path.
2. Run `snapshot_workspace` when no matching cached snapshot is available.
3. If current typed analysis is unavailable, attach the latest stale typed
   artifact for the current canonical module.
4. If current resolved analysis is unavailable, attach the latest stale resolved
   artifact for the current canonical module.
5. Preserve current parse/diagnostic results so features can still distinguish
   current errors from stale semantic lookup data.

Then replace `completion_snapshot` in `server_core.tw` with calls to this shared
helper. Completion and signature help should use it immediately; other query
features can adopt it when they need editing-time recovery.

### Cursor context module

Add a shared module, for example:

```text
boot/compiler/query/cursor_context.tw
```

This module should own source-level and cursor-hole helpers used by LSP query
features.

Suggested public model:

```tw
pub type CompletionContext = {
  Member(MemberContext),
  General(String),
  Import(ImportPrefix),
  Variant,
}

pub type MemberContext = .{
  dot_offset: Int,
  receiver_name: String?,
}

pub type CallContext = .{
  name: String,
  paren_offset: Int,
  active_parameter: Int,
}
```

Suggested public helpers:

```tw
pub fn classify_completion(source: String, offset: Int) CompletionContext
pub fn call_at(source: String, offset: Int) CallContext?
pub fn extract_ident_before(source: String, offset: Int) String
```

Internal helpers can include:

* cursor-hole lexing and `CursorHole` lookup
* import-path extraction
* member-dot detection
* bracket-aware backward call scanning
* bracket-aware comma counting
* identifier/qualified-name extraction before `(` or `.`

The module should avoid semantic lookups. It should only answer questions about
current text and cursor shape.

---

## Feature Refactors

### Completion

Refactor `boot/compiler/query/completion.tw` so it delegates context
classification to `cursor_context.classify_completion`.

Keep completion-specific behavior in `completion.tw`:

* resolving receiver types
* mapping record fields and methods to completion items
* module member lookup
* local variable/function/type completion
* import filesystem discovery

### Signature help

Refactor `boot/compiler/query/signature_help.tw` so the source-scanning fallback
uses `cursor_context.call_at`.

Keep signature-help-specific behavior in `signature_help.tw`:

* AST-based call search for complete code
* function/method signature resolution
* receiver-adjusted method display
* signature response construction

### Server core

Replace the feature-specific snapshot helper with the shared editor snapshot:

```tw
snap := semantic.snapshot_workspace_for_editor(state.query_cache, workspace_input(state, doc))
```

Use this for completion and signature help first. Leave hover, definition, and
document symbols on the stricter workspace snapshot unless they later need the
same incomplete-source behavior.

---

## Implementation Checkpoints

### Checkpoint A — Name and centralize editor snapshots

Goal: make the cache-recovery policy explicit before moving feature logic.

Checklist:

* Add `semantic.snapshot_workspace_for_editor(store, input)`.
* Preserve the current fast path through `snapshot_workspace_from_cache`.
* Preserve the current full-analysis path through `snapshot_workspace`.
* Attach latest stale `typed` when the current snapshot has no typed artifact.
* Attach latest stale `resolved` when the current snapshot has no resolved
  artifact.
* Keep current diagnostics and current parsed artifact from the latest analysis.
* Replace `completion_snapshot` in `server_core.tw` with the new semantic helper.
* Use the new helper for completion and signature help.
* Leave hover, definition, document symbols, and formatting on their existing
  snapshot paths.
* Run existing completion and signature-help LSP suites before continuing.

Audit notes:

* This checkpoint should be behavior-preserving.
* The main visible code change should be naming and ownership: server code asks
  the query layer for an editor snapshot instead of owning recovery details.

### Checkpoint B — Introduce `cursor_context.tw` without feature migration

Goal: create the shared module and move only leaf-level source helpers first.

Checklist:

* Add `boot/compiler/query/cursor_context.tw`.
* Move or duplicate temporarily the source-only helpers needed by both features:
  cursor-hole lookup, identifier extraction, qualified-name extraction,
  bracket-aware scanning, and comma counting.
* Keep the new helpers free of semantic lookup, LSP JSON types, and
  feature-specific completion/signature result types.
* Add small public types for shared cursor concepts, such as `ImportPrefix`,
  `MemberContext`, and `CallContext`.
* Keep existing completion and signature-help modules calling their old local
  paths until the new module is compiled and imported cleanly.
* Run formatter on the new Twinkle file.
* Run the existing boot tests before migrating behavior.

Audit notes:

* This checkpoint should either be behavior-neutral or limited to mechanical
  helper extraction.
* If temporary duplication is used, mark it clearly and remove it in later
  checkpoints.

### Checkpoint C — Route signature-help fallback through `cursor_context.call_at`

Goal: make incomplete-call detection shared while keeping signature resolution
inside `signature_help.tw`.

Checklist:

* Implement `cursor_context.call_at(source, offset) CallContext?`.
* Ensure `call_at` finds the innermost incomplete call at the cursor.
* Ensure `call_at` tracks active parameter using bracket-aware comma counting.
* Ensure `call_at` handles qualified names such as `io.write_stdout_text(`.
* Replace `source_scan_signature_help`'s local scanner with `call_at`.
* Keep signature lookup, full-name fallback, last-segment fallback, and response
  rendering in `signature_help.tw`.
* Remove signature-help-local scanning helpers that are no longer used.
* Run signature-help tests focused on incomplete direct, qualified, nested, and
  comma-separated calls.

Audit notes:

* The diff should show signature help delegating cursor discovery but retaining
  semantic ownership.
* Existing complete-code AST behavior should remain the first path.

### Checkpoint D — Route completion classification through `cursor_context`

Goal: make completion context classification shared while keeping completion
semantics in `completion.tw`.

Checklist:

* Implement `cursor_context.classify_completion(source, offset)`.
* Preserve current handling for general identifier prefixes.
* Preserve current handling for member completion after `.` and partial member
  names.
* Preserve current handling for variant contexts after syntactic markers such as
  `{`, `=>`, `,`, `:=`, and `=`.
* Preserve current import-path detection, including newline-boundary behavior.
* Return enough member context for completion to reuse the dot offset and, where
  useful, the receiver name.
* Replace `completion.classify_context` with the shared classifier.
* Remove completion-local cursor-hole/import/source helper code that is no
  longer used.
* Run completion tests focused on member, import, variant, and general contexts.

Audit notes:

* The diff should show completion still owning item production and typed receiver
  lookup.
* Any changed completion output should be intentional and covered by tests.

### Checkpoint E — Keep semantic lookup feature-specific

Goal: avoid over-generalizing before the shared cursor layer has proven stable.

Checklist:

* Keep receiver type lookup, field/method completion, local-variable completion,
  and import filesystem discovery in `completion.tw`.
* Keep function/method signature lookup, receiver-adjusted method signatures,
  and signature label construction in `signature_help.tw`.
* Share only cursor/source context and editor snapshot policy.
* Document any semantic helper that starts to look reusable, but defer moving it
  unless both features truly need it.

Audit notes:

* This checkpoint is a review gate rather than a large code change.
* If a proposed helper requires `CheckResult`, `ResolvedEnv`, or LSP response
  types, it probably does not belong in `cursor_context.tw` yet.

### Checkpoint F — Add drift-prevention tests and cleanup

Goal: make the shared behavior auditable and hard to regress.

Checklist:

* Add or update tests that exercise incomplete source through the public LSP
  request paths.
* Cover warm-cache behavior where current source is incomplete but stale
  semantic data can answer the query.
* Cover cases where text after the cursor must not be consumed as part of the
  current expression.
* Cover module-qualified calls and module-member completion.
* Cover import completion across line boundaries.
* Confirm UTF-16 position conversion remains tested at the LSP boundary.
* Remove temporary duplicated helpers introduced during migration.
* Run formatter on edited `.tw` files.
* Run the relevant boot LSP suites.

Audit notes:

* Tests should describe user-visible editing scenarios, not implementation
  internals.
* After this checkpoint, completion and signature help should depend on the same
  source/cursor primitives.

### Follow-up checkpoint — Consider adoption by other features

Goal: decide where editor snapshots are useful beyond completion and signature
help.

Checklist:

* Review hover behavior on incomplete source.
* Review definition behavior on incomplete source.
* Review future references, rename, and document-highlight plans.
* Adopt `snapshot_workspace_for_editor` only where stale semantic data is safe
  and useful.
* Keep stricter snapshots for features where stale answers could produce
  surprising edits or navigation.

---

## Test Plan

Reuse existing completion and signature-help protocol tests, and add coverage for
shared incomplete-source behavior:

* Member completion after `value.` and `value.pa`.
* Member completion where text after the cursor would otherwise be parsed as a
  continuation.
* Import completion after `use`, `use @std.`, relative imports, and partial
  path segments.
* Signature help for incomplete direct calls, module-qualified calls, nested
  calls, and calls with commas.
* Signature help fallback when AST lookup finds a call but semantic resolution
  fails.
* Warm-cache scenarios where the current document has parse/check errors but
  stale semantic data is still useful.
* UTF-16 position mapping remains covered at the LSP boundary.

---

## Exit Criteria

LSP query features that need editing-time recovery share one snapshot policy and
one source/cursor context module. Completion and signature help keep their
current user-visible behavior, but no longer duplicate low-level incomplete
source handling.
