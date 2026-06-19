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
| `didClose` | **Full** (if docs remain) | clear URI, then re-analyze the rest | clear closed URI; refresh remaining docs |

Parse is single-file and purely syntactic, so it is always correct and never
false-positives on `use` imports.

### User-visible consequences (approved)

- Syntax errors stay live while typing (parse tier, ~150 ms debounce).
- Type / cross-file errors **withdraw** while editing and reappear on save. On
  each edit *every* changed document is republished with only its parse
  diagnostics (the parse job drains a dirty set, so edits across multiple files
  don't strand stale squiggles on any of them), so no stale squiggle is left at
  a wrong position on changed text (version-correct).
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
  parse_dirty_uris: Dict<String, Bool>,  // docs awaiting a parse-tier publish (string set)
  full_dirty: Bool,                  // workspace changed since last completed Full run
  last_published: Dict<String, String>,  // URI -> last published signature (replaces last_published_versions)
}
```

`initial_state` seeds `diagnostics_tier: .Full`, `parse_dirty_uris: Dict.new()`
(a `Dict<String, Bool>` used as a string set; `Set<String>` is avoided as a record
field because it collides with the prelude's `set$*` methods at link time),
`full_dirty: false`, `last_published: Dict.new()`.

- `mark_diagnostics_pending(tier, uri?)`: set `diagnostics_pending = true`,
  `diagnostics_tier = tier`, and `diagnostics_deadline_ms = now + 150ms` for
  `Parse` / `now` for `Full`. For `Parse`, add `uri` to `parse_dirty_uris`. The
  150 ms here becomes the single source of debounce truth. (When a `Full` is
  marked, the in-flight tier is `Full`; any queued parse URIs are subsumed by
  the full pass and the set is cleared when the full run completes.)
- `note_changed_document_source`: additionally set `full_dirty = true`.
- `publish_due_diagnostics`: branch on `diagnostics_tier` once the deadline is
  reached:
  - **Parse** → for each URI in `parse_dirty_uris`, run `analyze_parse` on that
    document and publish it at its current version; update `query_cache`; empty
    `parse_dirty_uris`; leave `full_dirty` set; clear `diagnostics_pending`.
  - **Full** → existing `publish_workspace_diagnostics`, then clear
    `full_dirty`, empty `parse_dirty_uris`, and clear `diagnostics_pending`.

### Duplicate suppression must be content-based

Today suppression is version-only (`last_published_versions`): publish iff the
version differs. That breaks here — a parse publish at version `N` would block
the save's full publish at the same `N`, so type errors never reappear. It also
misses an unchanged importer whose diagnostics changed because a dependency
changed.

Replace it with a per-URI **signature**: `version` combined with a digest of the
published diagnostics (concatenate the existing `diagnostic_key` of each). Publish
iff the signature changed. Both tiers go through this:

- parse(`N`, `[]`) and full(`N`, `[typeErr]`) have different signatures → both
  publish;
- an importer whose diagnostics change at the same version → new signature →
  publishes;
- a genuinely unchanged re-publish → same signature → suppressed.

`should_publish_diagnostics` takes the diagnostics list and compares signatures;
`mark_published_version` becomes `mark_published` storing the signature.

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
- `handle_did_change` → `mark_diagnostics_pending(.Parse, doc.uri)` (for each
  changed URI; in practice one per notification).
- `handle_did_close`: keep clearing the closed URI's diagnostics; also drop it
  from `parse_dirty_uris`. Closing an unsaved overlay shifts the rest of the
  workspace back to disk-backed analysis and can change other open docs'
  diagnostics, and would otherwise leave `full_dirty` stuck. So: if any documents
  remain open, `mark_diagnostics_pending(.Full, .None)` to refresh them; if none
  remain, set `full_dirty = false` and leave nothing pending.

### 4. Heavy-feature gating — `boot/lib/lsp/server_core.tw`

The four guards (`handle_document_symbol`, `handle_folding_range`,
`handle_inlay_hint`, `handle_semantic_tokens`) must serve only a completed full
snapshot and never trigger inline analysis. Two changes:

- Add a cache-only accessor `workspace_snapshot_cached(doc) SemanticSnapshot?`
  that calls only `semantic.snapshot_workspace_from_cache` (never the inline
  `snapshot_workspace`).
- Each guard returns empty when `state.full_dirty` **or** the cached snapshot is
  `.None`, and otherwise serves the cached snapshot. The `full_dirty` check alone
  is insufficient: `workspace_snapshot` falls back to inline `snapshot_workspace`
  whenever cached typed/resolved artifacts are missing (e.g. after a full run that
  ended in parse errors), which would block the dispatcher on the request path.

### 5. Scheduler — `boot/commands/lsp.tw`

- `schedule_diagnostics`: sleep `max(0, deadline − now)` read from shared state
  instead of the fixed `debounce_ms`, so the tier's deadline drives debounce
  (Parse ≈150 ms, Full ≈immediate). The constant `debounce_ms` is removed.
- `run_diagnostics_job`: emit work-done progress only when the pending tier is
  `Full`; the parse tier is too fast to warrant progress UI.
- Generation token is unchanged: a later edit or save bumps `gen`, and any stale
  in-flight job self-discards on its generation check.

## Edge cases

- Parse publish uses each document's current version so the editor reconciles
  diagnostics with the buffer it has.
- Rapid edits across files: every changed URI is in `parse_dirty_uris`, so the
  parse job publishes all of them; a later edit supersedes only the *timer* (gen),
  not the accumulated dirty set.
- Save with nothing dirty still runs Full; `analyze_workspace` reuses the closure
  snapshot, so a clean re-check is cheap.
- `didOpen` briefly sets `full_dirty` until its Full run completes; heavy features
  are empty for that short window, as today.
- Closing the last open document clears `full_dirty` (nothing left to analyze) so
  it can't stay stuck.

## Testing

- `boot/tests/suites/query_diagnostics_suite.tw`:
  - `analyze_parse` returns parse diagnostics for broken syntax (e.g. `fn f( {`).
  - `analyze_parse("fn answer() String { 42 }")` returns `[]` — the key guard:
    `analyze_document` flags this as a type error, `analyze_parse` must not.
- `boot/tests/suites/lsp_server_core_suite.tw`:
  - `didChange` → pending, tier `Parse`, the URI in `parse_dirty_uris`,
    `full_dirty` true.
  - `didSave` → pending, tier `Full`.
  - Parse-tier publish touches only the dirtied URIs and leaves `full_dirty` set.
  - Full-tier publish clears `full_dirty` and empties `parse_dirty_uris`.
  - **Reappear-on-save:** a parse publish at version `N` followed by a `didSave`
    at the same version `N` still publishes the full/type diagnostics (content
    signature differs) — the suppression regression this design guards against.
  - **Rapid cross-file edits:** changing doc A then doc B before the parse job
    runs publishes parse diagnostics for *both* (neither is stranded with stale
    full squiggles).
  - **No inline analysis:** a heavy feature with no cached typed snapshot (e.g.
    after a parse-error full run) returns empty rather than running analysis on
    the request path; it serves results once a clean Full completes; it is empty
    while `full_dirty`.
  - **didClose unsticks full_dirty:** closing a dirty document schedules a Full
    run when others remain open, and clears `full_dirty` when none remain.
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
