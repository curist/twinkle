# LSP Watched Files Plan

## Goal

Teach `twk lsp` to react to workspace file changes that do not flow through
open editor buffers, so the server stops serving a stale on-disk snapshot for:

* unopened file edits
* newly created `.tw` files
* deleted `.tw` files
* rename flows represented as delete + create notifications

This plan is intentionally scoped to correctness first. It does not attempt to
turn the LSP into a fully incremental dependency-aware engine in the same step.

---

## Current Baseline (2026-03-23)

Implemented today:

* `initialize`, `initialized`, `shutdown`, `exit`
* `textDocument/didOpen`, `textDocument/didChange`, `textDocument/didClose`
* `textDocument/hover`
* `textDocument/definition`
* `textDocument/completion`

Current workspace-state model:

* `initialize_session` walks the project/stdlib/prelude roots once and builds
  `base_sources` from all discovered `.tw` files on disk.
* `AnalysisSession` keeps unsaved editor content in `overlays`.
* analysis clones `base_sources`, overlays open-buffer text on top, and runs
  analysis from that combined map.

Current known gap:

* after initialization, unopened on-disk file changes are invisible to the
  session unless the server restarts.
* newly added files are never inserted into `base_sources`.
* deleted files remain in `base_sources`.
* rename flows keep the old path alive until restart and do not load the new
  path unless it is opened in the editor.

This means cross-file diagnostics and navigation can become stale even while
the actively edited file remains fresh through overlay updates.

---

## Scope

In scope:

* LSP protocol support for `workspace/didChangeWatchedFiles`
* session cache updates for create/change/delete events
* preserving overlay precedence for open files with unsaved edits
* diagnostics republishing after watched-file updates
* tests for unopened-file edits, create, delete, and rename-like flows

Out of scope:

* fine-grained dependency invalidation
* client-specific file-watcher setup beyond minimal LSP capability support
* `workspace/didRenameFiles` / `didCreateFiles` / `didDeleteFiles`
* latency optimization for very large workspaces

---

## Design Direction

### 1) Treat watched-file notifications as mutations to `base_sources`

`base_sources` is the authoritative disk snapshot for unopened files. The
watched-file handler should update that map directly:

* `Created`: read the file and insert it if it is an in-scope `.tw` file
* `Changed`: reread the file and replace the cached disk contents
* `Deleted`: remove the file from the cache

Non-`.tw` files should be ignored unless a future feature explicitly needs
other extensions.

### 2) Keep overlays authoritative for open documents

Open-buffer overlays must continue to win over `base_sources`. A file watcher
notification for a currently open path should refresh the disk cache only; it
must not overwrite unsaved text stored in `overlays`.

This preserves the standard LSP expectation that editor contents, not the disk,
drive semantic queries while a file is open.

### 3) Republish diagnostics from the updated workspace snapshot

After any watched-file change, the server should recompute diagnostics from the
new combined snapshot and publish updates for affected paths.

Correctness-first behavior:

* recompute full workspace diagnostics after each watched-file batch
* publish diagnostics for every module that currently has diagnostics
* clear diagnostics for paths that previously had diagnostics but no longer do
* clear diagnostics for deleted files

This is broader than ideal, but it avoids subtle stale-diagnostic cases before
dependency-aware invalidation exists.

### 4) Prefer simple capability negotiation

Initial implementation should advertise watched-file support in the server's
`initialize` result rather than starting with dynamic registration machinery.

If the active editor only sends notifications after explicit dynamic
registration, add that as a follow-up. The first implementation should keep the
server-side state model and test surface simple.

---

## Milestones

### W1 — Session-Level Disk Snapshot Updates

Code changes:

* add `AnalysisSession` helpers for:
  * inserting/updating disk-backed sources
  * removing disk-backed sources
  * checking whether a path is currently overlaid
* centralize path normalization and `.tw` path filtering

Likely files:

* `src/lsp/session.rs`

Acceptance:

* unit tests prove `base_sources` changes affect future analysis
* open-buffer overlays still take precedence over refreshed disk contents
* deleted files disappear from the analyzed source map after removal

### W2 — Protocol Handling for `workspace/didChangeWatchedFiles`

Code changes:

* extend `handle_lsp_message` to accept
  `workspace/didChangeWatchedFiles`
* parse file event arrays and map them to session update operations
* ignore unreadable/malformed paths without panicking
* advertise watched-file support in `initialize`

Likely files:

* `src/cli/lsp.rs`

Acceptance:

* protocol tests can send watched-file notifications without receiving
  `Method not found`
* create/change/delete events update session state as expected
* unsupported or malformed notifications fail safely

### W3 — Diagnostics Refresh and Clearing

Code changes:

* track the last published diagnostic paths for the session
* after watched-file batches, recompute workspace diagnostics
* publish current diagnostics and clear paths that are no longer reporting
* explicitly clear diagnostics for deleted files

Likely files:

* `src/cli/lsp.rs`
* `src/lsp/session.rs`

Acceptance:

* changing an unopened dependency republishes diagnostics in dependent files
* deleting a file clears diagnostics previously published for that path
* fixing a watched file removes stale diagnostics without restart

### W4 — Rename-Flow and Multi-File Coverage

Code changes:

* add protocol smoke tests for rename-like sequences (`Deleted` old path,
  `Created` new path)
* cover cases where a new module becomes importable without opening it first
* cover cases where a removed module invalidates existing imports

Likely files:

* `tests/` LSP protocol test suite

Acceptance:

* rename-like flows no longer require server restart
* import resolution and diagnostics reflect new module topology after watched
  file events

---

## Test Plan

Unit tests:

* `AnalysisSession` updates cached disk sources on create/change/delete
* overlay text wins over refreshed disk text for open files
* non-`.tw` events are ignored

Integration/protocol tests:

* initialize -> watched change on unopened dependency -> diagnostics refresh
* initialize -> watched create of new module -> import starts resolving
* initialize -> watched delete of imported module -> diagnostics appear/refresh
* initialize -> watched delete + create rename flow -> new path resolves, old
  path is cleared

Manual smoke target:

* edit an unopened dependency on disk while an importing file stays open in the
  editor and verify hover/definition/diagnostics reflect the new state without
  restarting the LSP server

---

## Risks and Mitigations

* client watcher behavior varies:
  start with straightforward capability advertising and confirm behavior in the
  target editor; add dynamic registration only if needed.

* diagnostics fan-out may be noisy or expensive:
  prefer full-workspace republish for correctness first, then optimize with
  dependency-aware invalidation later.

* open-file disk events can race with unsaved buffers:
  keep `overlays` authoritative and treat watched-file events as disk-cache
  refreshes only.

* deleted files may leave stale diagnostics behind:
  track last-published diagnostic paths explicitly so clears are intentional and
  testable.

---

## Exit Criteria

* unopened `.tw` file edits are reflected without restarting `twk lsp`
* created and deleted `.tw` files update semantic results and diagnostics
* rename-like delete/create flows converge to the new workspace shape
* open unsaved buffers remain authoritative over disk changes
* watched-file behavior is covered by protocol tests
