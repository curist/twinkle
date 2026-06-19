# LSP Save-vs-Edit Diagnostics Gating (M6)

Status: **Design approved**.

This is the first slice of the non-blocking end-game tracked in
[lsp-task-migration.md](lsp-task-migration.md) (the "Non-blocking end-game"
section, checkpoint M6). It is pure Tier A: it ships on the current cooperative
scheduler and needs no new runtime primitives or compiler-architecture change.

## Problem

Diagnostics currently run a full `analyze_workspace` (cross-file resolve +
typecheck) on **every** `didChange` after a debounce window. Whole-workspace
type checking on each keystroke is the dominant LSP CPU cost. The fix is to split
analysis by trigger: cheap parse-level diagnostics while editing, full workspace
analysis only on open and save.

## Behavior model

| Trigger | Tier | Work | Publishes |
|---|---|---|---|
| `didOpen` | **Full** | `analyze_workspace` | all open docs, full diagnostics |
| `didChange` | **Parse** | parse active doc only | active doc, parse diagnostics only |
| `didSave` | **Full** | `analyze_workspace` | all open docs, full diagnostics |
| `didClose` | — | unchanged | clear diagnostics for the URI |

Parse is single-file and purely syntactic, so it is always correct and never
false-positives on `use` imports.

### User-visible consequences (approved)

- Syntax errors stay live while typing (parse tier, ~150 ms debounce).
- Type / cross-file errors **withdraw** while editing and reappear on save. On
  each edit the active document is republished with only its parse diagnostics,
  so no stale squiggle is ever drawn at a wrong position on changed text
  (version-correct).
- Heavy structural features (document symbols, folding ranges, inlay hints,
  semantic tokens) serve the last completed full snapshot and refresh on save.

### Why edit-tier is parse-only

`analyze_document` runs parse → resolve → typecheck against the builtin env
only; it does **not** load cross-module imports. So single-file resolve/typecheck
emits false positives (every imported name looks unresolved, every cross-module
type looks unknown) for any real workspace file. Only the parse stage is reliably
correct single-file. A richer edit tier (resolve against the cached dependency
export env) is explicitly deferred to a later checkpoint.

## Implementation

### 1. Parse-only analysis entry — `boot/compiler/query/diagnostics.tw`

Add a sibling to `analyze_document` that stops after the parse stage:

```tw
pub fn analyze_parse(store: cache.Store, input: DocumentInput) AnalysisResult {
  // runs stage_runner.parse only;
  // returns input.convert_diagnostics("parse", parsed.diagnostics), or [] when clean
}
```

This is the correctness keystone: a document that parses clean but would fail
typecheck must return `[]`, never a type diagnostic.

### 2. State changes — `boot/lib/lsp/server_core.tw`

```tw
type DiagTier = { Parse, Full }

type State = .{
  // …existing fields…
  diagnostics_tier: DiagTier,        // which work is queued (meaningful while pending)
  diagnostics_target_uri: String?,   // active doc for Parse tier; .None for Full
  full_dirty: Bool,                  // workspace changed since last completed Full run
}
```

`initial_state` seeds `diagnostics_tier: .Full`, `diagnostics_target_uri: .None`,
`full_dirty: false`.

- `mark_diagnostics_pending(tier, uri?)`: set `diagnostics_pending = true`,
  `diagnostics_tier = tier`, `diagnostics_target_uri = uri`, and
  `diagnostics_deadline_ms = now + 150ms` for `Parse` / `now` for `Full`. The
  150 ms here becomes the single source of debounce truth.
- `note_changed_document_source`: additionally set `full_dirty = true`.
- `publish_due_diagnostics`: branch on `diagnostics_tier` once the deadline is
  reached:
  - **Parse** → `analyze_parse` on `diagnostics_target_uri`, publish that one
    URI at its current document version, update `query_cache`, leave
    `full_dirty` set, clear `diagnostics_pending`.
  - **Full** → existing `publish_workspace_diagnostics(true)`, then clear
    `full_dirty` and `diagnostics_pending`.

### 3. Save wiring — `boot/lib/lsp/server_core.tw` + `params.tw`

- Capability: replace `json.kv("textDocumentSync", .Int(2))` with an object:

  ```tw
  json.kv("textDocumentSync", json.object([
    json.kv("openClose", .Bool(true)),
    json.kv("change", .Int(2)),
    json.kv("save", json.object([json.kv("includeText", .Bool(false))])),
  ]))
  ```

- `handle_notification`: add `"textDocument/didSave" => state.handle_did_save(note)`.
- `handle_did_save`: decode the URI and call `mark_diagnostics_pending(.Full, .None)`.
  Do not touch `documents` — the buffer is already current from prior `didChange`;
  the optional `text` field is ignored.
- `params.tw`: add `DidSaveTextDocumentParams = .{ text_document: TextDocumentIdentifier }`
  and `decode_did_save`, mirroring `decode_did_close`.
- `handle_did_open` → `mark_diagnostics_pending(.Full, .None)`.
- `handle_did_change` → `mark_diagnostics_pending(.Parse, doc.uri)`.

### 4. Heavy-feature gating — `boot/lib/lsp/server_core.tw`

Change the four guards in `handle_document_symbol`, `handle_folding_range`,
`handle_inlay_hint`, and `handle_semantic_tokens` from
`if state.diagnostics_pending` to `if state.full_dirty`. They then serve the last
clean full snapshot and go empty only while editing-before-save, never running a
full analysis inline on the dispatcher.

### 5. Scheduler — `boot/commands/lsp.tw`

- `schedule_diagnostics`: sleep `max(0, deadline − now)` read from shared state
  instead of the fixed `debounce_ms`, so the tier's deadline drives debounce
  (Parse ≈150 ms, Full ≈immediate). The constant `debounce_ms` is removed.
- `run_diagnostics_job`: emit work-done progress only when the pending tier is
  `Full`; the parse tier is too fast to warrant progress UI.
- Generation token is unchanged: a later edit or save bumps `gen`, and any stale
  in-flight job self-discards on its generation check.

## Edge cases

- Parse publish uses the document's current version so the editor reconciles
  diagnostics with the buffer it has.
- Rapid edits: only the latest parse job survives (generation supersession).
- Save with nothing dirty still runs Full; `analyze_workspace` reuses the closure
  snapshot, so a clean re-check is cheap.
- `didOpen` briefly sets `full_dirty` until its Full run completes; heavy features
  are empty for that short window, as today.

## Testing

- `boot/tests/suites/query_diagnostics_suite.tw`:
  - `analyze_parse` returns parse diagnostics for broken syntax (e.g. `fn f( {`).
  - `analyze_parse("fn answer() String { 42 }")` returns `[]` — the key guard:
    `analyze_document` flags this as a type error, `analyze_parse` must not.
- `boot/tests/suites/lsp_server_core_suite.tw`:
  - `didChange` → pending, tier `Parse`, `diagnostics_target_uri` set,
    `full_dirty` true.
  - `didSave` → pending, tier `Full`.
  - Parse-tier publish touches only the active URI and leaves `full_dirty` set.
  - Full-tier publish clears `full_dirty`.
  - A heavy feature (e.g. `documentSymbol`) returns empty while `full_dirty` and
    serves results after a Full run completes.
  - `initialize` capability advertises `textDocumentSync.save`.
  - Audit existing assertions that diagnostics fire on `didChange` and update them
    to the parse-tier expectation.

## Validation

- `target/twk fmt` the edited `.tw` files.
- `target/twk lint boot/main.tw`.
- `target/twk build boot/main.tw -o /tmp/check.wasm` (catch compile/type errors).
- `target/twk run boot/tests/main.tw` (full boot suite).
- No `cargo test` required: stage0/Rust are untouched (this is boot-only).

## Out of scope (future checkpoints)

- Resolve-on-edit against the cached dependency export env (the richer edit tier).
- Serving heavy features from stale snapshots rather than empty (M4).
- Worker-process isolation and the airtight non-blocking guarantee (M0/Tier B).
