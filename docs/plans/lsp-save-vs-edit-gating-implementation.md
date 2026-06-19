# LSP Save-vs-Edit Diagnostics Gating — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Split LSP diagnostics by trigger — cheap parse-only analysis of changed documents while editing, full workspace analysis only on open/save — so typing no longer pays for a whole-workspace typecheck.

**Architecture:** A `DiagTier` (`Parse` | `Full`) is threaded through the existing cooperative diagnostics worker. `didChange` queues changed URIs for a parse-only pass; `didOpen`/`didSave`/`didClose` schedule a full pass. Duplicate-publish suppression becomes content-based (per-URI signature) so a parse publish never blocks the save-time full publish. Heavy structural features serve only a cached full snapshot, never inline analysis.

**Tech Stack:** Twinkle (`.tw`), the boot compiler. Build: `target/twk build`. Tests: `target/twk run boot/tests/main.tw`. No Rust/stage0 changes (boot-only).

**Design source:** `docs/plans/lsp-save-vs-edit-gating.md`.

---

## File structure

- `boot/compiler/query/diagnostics.tw` — add `analyze_parse` (parse-only single-doc analysis); expose `diagnostic_key`.
- `boot/lib/lsp/server_core.tw` — `DiagTier` type, new `State` fields, content-based suppression, tier-aware scheduling/publishing, save/close wiring, cache-only heavy-feature gating.
- `boot/lib/lsp/params.tw` — `decode_did_save`.
- `boot/commands/lsp.tw` — deadline-driven debounce, progress only for full runs.
- `boot/tests/suites/query_diagnostics_suite.tw` — `analyze_parse` tests.
- `boot/tests/suites/lsp_server_core_suite.tw` — tier/save/close/gating tests; update existing literals and behavior-dependent tests.

Each task below leaves the build green and the boot suite passing.

---

## Task 1: Parse-only analysis entry (`analyze_parse`)

**Files:**
- Modify: `boot/compiler/query/diagnostics.tw` (add function after `analyze_document`, ends at line 90)
- Test: `boot/tests/suites/query_diagnostics_suite.tw`

- [ ] **Step 1: Write the failing tests**

Add these two tests inside the `suite()` chain in `boot/tests/suites/query_diagnostics_suite.tw` (after the existing `"reports typecheck diagnostics"` test):

```tw
    .test(
      "analyze_parse reports parse diagnostics",
      fn() {
        result := diagnostics.analyze_parse(cache.empty(), input("fn f( {"))
        try assert.is_true(result.diagnostics.len() > 0)
        try assert.equal(result.diagnostics[0].stage, "parse")
        .Ok({})
      },
    )
    .test(
      "analyze_parse ignores type errors (no false positives)",
      fn() {
        // analyze_document flags this as a "check" type mismatch; parse-only must not.
        result := diagnostics.analyze_parse(cache.empty(), input("fn answer() String { 42 }"))
        try assert.equal(result.diagnostics.len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -5`
Expected: FAIL — compile error `analyze_parse` is not defined (or undefined field).

- [ ] **Step 3: Implement `analyze_parse`**

In `boot/compiler/query/diagnostics.tw`, immediately after `analyze_document` (the function ending at line 90), add:

```tw
/// Parse-only single-document analysis. Unlike `analyze_document` this stops
/// after parsing, so it never emits the false resolve/typecheck diagnostics that
/// single-file analysis produces for any document with `use` imports. Used by the
/// LSP edit tier, where only syntactic errors are reliably correct without the
/// import closure.
pub fn analyze_parse(store: cache.Store, input: DocumentInput) AnalysisResult {
  source_hash := keys.hash_text(input.text)
  runner := stage_runner.new(input.uri, source_hash, 0, 0, false)
  parsed_out := stage_runner.parse(runner, store, input.text, 0)
  .{
    cache: parsed_out.store,
    diagnostics: input.convert_diagnostics("parse", parsed_out.value.diagnostics),
  }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -3`
Expected: `Ran N tests: N passed`.

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/compiler/query/diagnostics.tw boot/tests/suites/query_diagnostics_suite.tw
target/twk lint boot/main.tw
git add boot/compiler/query/diagnostics.tw boot/tests/suites/query_diagnostics_suite.tw
git commit -m "Add parse-only analyze_parse for the LSP edit tier"
```

---

## Task 2: State shape + content-based suppression

This task changes the `State` record shape and replaces version-only duplicate suppression with content-based signatures. It is structural: no trigger behavior changes yet. All `State` literals must be updated in the same task to keep the build green.

**Files:**
- Modify: `boot/compiler/query/diagnostics.tw` (expose `diagnostic_key`)
- Modify: `boot/lib/lsp/server_core.tw` (State type, `initial_state`, suppression helpers, publish sites)
- Modify: `boot/tests/suites/lsp_server_core_suite.tw` (11 `State` literals)

- [ ] **Step 1: Expose `diagnostic_key`**

In `boot/compiler/query/diagnostics.tw`, change the signature of `diagnostic_key` (currently around line 244) from:

```tw
fn diagnostic_key(d: Diagnostic) String {
```
to:
```tw
pub fn diagnostic_key(d: Diagnostic) String {
```

- [ ] **Step 2: Add `DiagTier` and extend `State`**

In `boot/lib/lsp/server_core.tw`, add the tier type just above `pub type State` (line 52) and rewrite the `State` record:

```tw
pub type DiagTier = { Parse, Full }

pub type State = .{
  initialized: Bool,
  shutdown_requested: Bool,
  should_exit: Bool,
  documents: document_store.Store,
  query_cache: query_cache.Store,
  diagnostics_pending: Bool,
  diagnostics_deadline_ms: Float,
  diagnostics_tier: DiagTier,
  parse_dirty_uris: Dict<String, Bool>,
  full_dirty: Bool,
  last_published: Dict<String, String>,
  closure_snapshot: analyze.ClosureSnapshot?,
}
```

(`last_published` replaces the old `last_published_versions: Dict<String, Int>`.)

- [ ] **Step 3: Update `initial_state`**

Replace the body of `initial_state` (lines 66-78) with:

```tw
pub fn initial_state() State {
  .{
    initialized: false,
    shutdown_requested: false,
    should_exit: false,
    documents: document_store.empty(),
    query_cache: query_cache.empty(),
    diagnostics_pending: false,
    diagnostics_deadline_ms: 0.0,
    diagnostics_tier: .Full,
    parse_dirty_uris: Dict.new(),
    full_dirty: false,
    last_published: Dict.new(),
    closure_snapshot: .None,
  }
}
```

- [ ] **Step 4: Replace suppression helpers**

In `boot/lib/lsp/server_core.tw`, replace `should_publish_diagnostics` and `mark_published_version` (lines 844-866) with a signature-based version:

```tw
fn diagnostics_signature(version: Int?, diags: Vector<query_diagnostics.Diagnostic>) String {
  acc := case version {
    .Some(n) => "${n}",
    .None => "none",
  }

  for d in diags {
    acc = "${acc}\n${query_diagnostics.diagnostic_key(d)}"
  }

  acc
}

fn should_publish_diagnostics(
  state: State,
  doc: document_store.Document,
  diags: Vector<query_diagnostics.Diagnostic>,
) Bool {
  sig := diagnostics_signature(doc.version, diags)

  case state.last_published[doc.identity.uri] {
    .Some(prev) => prev != sig,
    .None => true,
  }
}

fn mark_published(
  state: State,
  doc: document_store.Document,
  diags: Vector<query_diagnostics.Diagnostic>,
) State {
  state.last_published[doc.identity.uri] = diagnostics_signature(doc.version, diags)
  state
}
```

- [ ] **Step 5: Update the publish sites**

In `publish_workspace_diagnostics`, change the signature (line 737) to drop the `suppress_duplicates` parameter:

```tw
fn publish_workspace_diagnostics(state: State) Step {
```

There are two publish sites inside it (the main loop near line 783 and the "extra" loop near line 821). In **both**, replace the `should_publish_diagnostics(group_doc, suppress_duplicates)` guard and the `mark_published_version(group_doc)` call. Each site currently reads:

```tw
        .Some(group_doc) => if !published.has(group.uri)
          and state.should_publish_diagnostics(group_doc, suppress_duplicates) {
          outgoing = .append(
            lsp_diagnostics.publish_diagnostics(
              group_doc.uri,
              group_doc.version,
              group_doc.index,
              group.diagnostics,
            ),
          )
          state = .mark_published_version(group_doc)
          published[group.uri] = true
        },
```

Replace both occurrences with:

```tw
        .Some(group_doc) => if !published.has(group.uri)
          and state.should_publish_diagnostics(group_doc, group.diagnostics) {
          outgoing = .append(
            lsp_diagnostics.publish_diagnostics(
              group_doc.uri,
              group_doc.version,
              group_doc.index,
              group.diagnostics,
            ),
          )
          state = .mark_published(group_doc, group.diagnostics)
          published[group.uri] = true
        },
```

- [ ] **Step 6: Update the `publish_due_diagnostics` caller**

In `publish_due_diagnostics` (line 711), change `state.publish_workspace_diagnostics(true)` to `state.publish_workspace_diagnostics()`. (This function is rewritten in Task 3; for now just drop the argument so it compiles.)

- [ ] **Step 7: Update all 11 `State` literals in the test suite**

In `boot/tests/suites/lsp_server_core_suite.tw`, every `server_core.State.{ … }` literal (lines 137, 173, 221, 286, 427, 463, 545, 587, 633, 750, 823) lists all fields explicitly. In each one:

1. Rename `last_published_versions: Dict.new(),` to `last_published: Dict.new(),`.
2. Add three fields next to it:

```tw
          diagnostics_tier: .Full,
          parse_dirty_uris: Dict.new(),
          full_dirty: false,
```

Fast path (verify each site afterward): from the repo root,

```bash
perl -0pi -e 's/last_published_versions: Dict\.new\(\),/last_published: Dict.new(),\n          diagnostics_tier: .Full,\n          parse_dirty_uris: Dict.new(),\n          full_dirty: false,/g' boot/tests/suites/lsp_server_core_suite.tw
target/twk fmt boot/tests/suites/lsp_server_core_suite.tw
```

- [ ] **Step 8: Build and run the suite**

Run: `target/twk build boot/main.tw -o /tmp/check.wasm 2>&1 | tail -3 && target/twk run boot/tests/main.tw 2>&1 | tail -3`
Expected: build succeeds; `Ran N tests: N passed` (behavior unchanged — first publish always happens because no prior signature exists).

- [ ] **Step 9: Format, lint, commit**

```bash
target/twk fmt boot/lib/lsp/server_core.tw boot/compiler/query/diagnostics.tw
target/twk lint boot/main.tw
git add boot/lib/lsp/server_core.tw boot/compiler/query/diagnostics.tw boot/tests/suites/lsp_server_core_suite.tw
git commit -m "Track published diagnostics by content signature; extend LSP State"
```

---

## Task 3: Tier-aware scheduling and parse publishing

Add the tier mechanics. Handlers still trigger `Full` (no user-visible behavior change yet); the parse path is implemented and unit-tested by constructing a `Parse`-tier state directly.

**Files:**
- Modify: `boot/lib/lsp/server_core.tw`
- Test: `boot/tests/suites/lsp_server_core_suite.tw`

- [ ] **Step 1: Write the failing test (parse-tier publish)**

Add to `boot/tests/suites/lsp_server_core_suite.tw` inside the suite chain:

```tw
    .test(
      "parse tier publishes only the dirtied uri and keeps full_dirty",
      fn() {
        opened := document_store.empty().open("file:///a.tw", "fn f( {", .Some(1))
        state := server_core.initial_state()
        state.documents = opened
        state.diagnostics_pending = true
        state.diagnostics_tier = .Parse
        dirty: Dict<String, Bool> = Dict.new()
        dirty["file:///a.tw"] = true
        state.parse_dirty_uris = dirty
        state.full_dirty = true
        step := publish_due_now(state)
        out := try published_for(step, "file:///a.tw")
        try assert.is_true(try diagnostic_count(out) > 0)
        try assert.is_true(step.state.full_dirty)
        try assert.equal(step.state.parse_dirty_uris.len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run test to verify it fails**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -5`
Expected: FAIL — `diagnostics_tier`/`parse_dirty_uris` assignment compiles, but `publish_due_now` still runs the full path, so `full_dirty` is unexpectedly cleared / wrong messages.

- [ ] **Step 3: Implement tier marking**

Replace `mark_diagnostics_pending` (lines 720-724) with:

```tw
fn mark_diagnostics_pending(state: State, tier: DiagTier, uri: String?) Step {
  state.diagnostics_pending = true
  state.diagnostics_tier = tier

  case tier {
    .Parse => {
      state.diagnostics_deadline_ms = date.now() + 150.to_float()
      case uri {
        .Some(u) => state.parse_dirty_uris[u] = true,
        .None => {},
      }
    },
    .Full => state.diagnostics_deadline_ms = date.now(),
  }

  .{ state, outgoing: [] }
}
```

- [ ] **Step 4: Set `full_dirty` on source change**

In `note_changed_document_source` (lines 726-735), add `state.full_dirty = true` before the final `state`:

```tw
fn note_changed_document_source(state: State, uri: String, text: String) State {
  id := case identity.from_file_uri(uri) {
    .Ok(parsed) => parsed,
    .Err(_) => identity.from_path(uri),
  }
  root := project_root_for_document(id.path)
  canonical := imports.canonical_module_path(id.path, imports.make_canonical_roots(root))
  state.query_cache = .note_source_hash(canonical, keys.hash_text(text))
  state.full_dirty = true
  state
}
```

- [ ] **Step 5: Branch `publish_due_diagnostics` and add the parse publisher**

Replace `publish_due_diagnostics` (lines 711-718) with:

```tw
pub fn publish_due_diagnostics(state: State) Step {
  if !(state.diagnostics_pending and date.now() >= state.diagnostics_deadline_ms) {
    return .{ state, outgoing: [] }
  }

  state.diagnostics_pending = false

  case state.diagnostics_tier {
    .Parse => state.publish_parse_diagnostics(),
    .Full => {
      step := state.publish_workspace_diagnostics()
      done := step.state
      done.full_dirty = false
      done.parse_dirty_uris = Dict.new()
      .{ state: done, outgoing: step.outgoing }
    },
  }
}

fn publish_parse_diagnostics(state: State) Step {
  outgoing: Vector<json.Json> = []

  for uri in state.parse_dirty_uris.keys() {
    doc := case state.documents.get(uri) {
      .Some(d) => d,
      .None => { continue },
    }
    result := query_diagnostics.analyze_parse(
      state.query_cache,
      .{ uri: doc.uri, text: doc.text, version: doc.version },
    )
    state.query_cache = result.cache

    if state.should_publish_diagnostics(doc, result.diagnostics) {
      outgoing = .append(
        lsp_diagnostics.publish_diagnostics(doc.uri, doc.version, doc.index, result.diagnostics),
      )
      state = .mark_published(doc, result.diagnostics)
    }
  }

  state.parse_dirty_uris = Dict.new()
  .{ state, outgoing }
}
```

- [ ] **Step 6: Pass an explicit tier from the open/change handlers**

`mark_diagnostics_pending` now requires a tier. Update the two existing callers (keep both `Full` for now — no behavior change this task):

In `handle_did_open` (line 171): `state.mark_diagnostics_pending(.Full, .None)`
In `handle_did_change` (line 228): `state.mark_diagnostics_pending(.Full, .None)`

- [ ] **Step 7: Run the test to verify it passes**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -3`
Expected: `Ran N tests: N passed`.

- [ ] **Step 8: Format, lint, commit**

```bash
target/twk fmt boot/lib/lsp/server_core.tw boot/tests/suites/lsp_server_core_suite.tw
target/twk lint boot/main.tw
git add boot/lib/lsp/server_core.tw boot/tests/suites/lsp_server_core_suite.tw
git commit -m "Add tier-aware diagnostics scheduling and parse-tier publishing"
```

---

## Task 4: Save wiring + flip didChange to the parse tier

Wire `didSave` (capability, handler, params), flip `didChange` to the parse tier, and update the existing tests that depended on full diagnostics firing on change.

**Files:**
- Modify: `boot/lib/lsp/params.tw`
- Modify: `boot/lib/lsp/server_core.tw`
- Test: `boot/tests/suites/lsp_server_core_suite.tw`

- [ ] **Step 1: Write the failing tests**

Add to `boot/tests/suites/lsp_server_core_suite.tw`:

```tw
    .test(
      "didSave schedules a full diagnostics run",
      fn() {
        opened := document_store.empty().open("file:///a.tw", "x := 1\n", .Some(1))
        state := server_core.initial_state()
        state.documents = opened
        params := json.object([
          json.kv("textDocument", json.object([json.kv("uri", .Str("file:///a.tw"))])),
        ])
        msg := protocol.Message.Notification(.{
          method: "textDocument/didSave",
          params: .Some(params),
        })
        step := state.handle_message(msg)
        try assert.is_true(step.state.diagnostics_pending)
        case step.state.diagnostics_tier {
          .Full => .Ok({}),
          _ => assert.fail("expected Full tier"),
        }
      },
    )
    .test(
      "didChange schedules the parse tier",
      fn() {
        opened := document_store.empty().open("file:///a.tw", "old", .Some(1))
        state := server_core.initial_state()
        state.documents = opened
        params := json.object([
          json.kv(
            "textDocument",
            json.object([json.kv("uri", .Str("file:///a.tw")), json.kv("version", .Int(2))]),
          ),
          json.kv("contentChanges", json.array([json.object([json.kv("text", .Str("new"))])])),
        ])
        msg := protocol.Message.Notification(.{
          method: "textDocument/didChange",
          params: .Some(params),
        })
        step := state.handle_message(msg)
        try assert.is_true(step.state.diagnostics_pending)
        try assert.is_true(step.state.full_dirty)
        try assert.is_true(step.state.parse_dirty_uris.has("file:///a.tw"))
        case step.state.diagnostics_tier {
          .Parse => .Ok({}),
          _ => assert.fail("expected Parse tier"),
        }
      },
    )
    .test(
      "parse publish does not suppress the save-time full publish at the same version",
      fn() {
        // Type error at version 1: parse is clean, save must still publish the type error.
        opened := document_store.empty().open("file:///a.tw", "fn answer() String { 42 }", .Some(1))
        state := server_core.initial_state()
        state.documents = opened
        // Parse tier first (clean parse -> publishes empty diagnostics for the uri).
        state.diagnostics_pending = true
        state.diagnostics_tier = .Parse
        dirty: Dict<String, Bool> = Dict.new()
        dirty["file:///a.tw"] = true
        state.parse_dirty_uris = dirty
        state.full_dirty = true
        after_parse := publish_due_now(state)
        // Now a full run at the same version must still publish the type error.
        full_state := after_parse.state
        full_state.diagnostics_pending = true
        full_state.diagnostics_tier = .Full
        full_state.diagnostics_deadline_ms = 0.0
        full_step := full_state.publish_due_diagnostics()
        out := try published_for(full_step, "file:///a.tw")
        try assert.is_true(try diagnostic_count(out) > 0)
        .Ok({})
      },
    )
    .test(
      "rapid edits in two docs withdraw stale diagnostics for both",
      fn() {
        opened := document_store
          .empty()
          .open("file:///a.tw", "fn f( {", .Some(2))
          .open("file:///b.tw", "fn g( {", .Some(2))
        state := server_core.initial_state()
        state.documents = opened
        state.diagnostics_pending = true
        state.diagnostics_tier = .Parse
        dirty: Dict<String, Bool> = Dict.new()
        dirty["file:///a.tw"] = true
        dirty["file:///b.tw"] = true
        state.parse_dirty_uris = dirty
        state.full_dirty = true
        step := publish_due_now(state)
        a := try published_for(step, "file:///a.tw")
        b := try published_for(step, "file:///b.tw")
        try assert.is_true(try diagnostic_count(a) > 0)
        try assert.is_true(try diagnostic_count(b) > 0)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -5`
Expected: FAIL — `textDocument/didSave` not handled (no state change), and `didChange` still uses `.Full`.

- [ ] **Step 3: Add the `didSave` params decoder**

In `boot/lib/lsp/params.tw`, add the type near the other param types (after `DidCloseTextDocumentParams`, line 32) :

```tw
pub type DidSaveTextDocumentParams = .{ text_document: TextDocumentIdentifier }
```

And add the decoder next to `decode_did_close` (after line 100):

```tw
pub fn decode_did_save(params: json.Json?) Result<DidSaveTextDocumentParams, String> {
  value := try required_params(params)
  text_document := try value.decode(json.field("textDocument", text_document_identifier_decoder()))
  .Ok(.{ text_document })
}
```

- [ ] **Step 4: Advertise save in the server capabilities**

In `boot/lib/lsp/server_core.tw`, replace the `textDocumentSync` capability line (line 96):

```tw
            json.kv("textDocumentSync", .Int(2)),
```
with:
```tw
            json.kv(
              "textDocumentSync",
              json.object([
                json.kv("openClose", .Bool(true)),
                json.kv("change", .Int(2)),
                json.kv("save", json.object([json.kv("includeText", .Bool(false))])),
              ]),
            ),
```

- [ ] **Step 5: Route the notification and add the handler**

In `handle_notification` (after the `didChange` arm, line 159), add:

```tw
    "textDocument/didSave" => state.handle_did_save(note),
```

Add the handler next to `handle_did_close`:

```tw
fn handle_did_save(state: State, note: protocol.Notification) Step {
  case params.decode_did_save(note.params) {
    .Ok(_) => state.mark_diagnostics_pending(.Full, .None),
    .Err(_) => .{ state, outgoing: [] },
  }
}
```

- [ ] **Step 6: Flip `didChange` to the parse tier**

In `handle_did_change`, change the final `state.mark_diagnostics_pending(.Full, .None)` (line 228) to:

```tw
      state.mark_diagnostics_pending(.Parse, .Some(doc.uri))
```

- [ ] **Step 7: Update existing tests that assumed full-on-change**

Two existing tests publish full diagnostics after a `didChange`. They must trigger via save now.

In `"didChange publishes diagnostics for open importers"` (around line 310-313) and `"didChange clears stale importer diagnostics"` (around line 449-452), the change-notification message drives `publish_due_now`. After applying the `didChange`, add a `didSave` before publishing. Replace the `step := publish_due_now(state.handle_message(msg))` line in each with:

```tw
        changed := state.handle_message(msg)
        save_params := json.object([
          json.kv("textDocument", json.object([json.kv("uri", .Str("file:///main.tw"))])),
        ])
        save_msg := protocol.Message.Notification(.{
          method: "textDocument/didSave",
          params: .Some(save_params),
        })
        step := publish_due_now(changed.state.handle_message(save_msg))
```

(Use the entry URI those tests already publish for; if it is not `file:///main.tw`, substitute the URI asserted later in the same test.)

- [ ] **Step 8: Update the capability assertion**

In `"initialize returns minimal capabilities"` (line ~55), replace:

```tw
        try assert.equal(try caps.decode(json.field("textDocumentSync", json.int())), 2)
```
with:
```tw
        sync := try caps.decode(json.field("textDocumentSync", json.raw()))
        try assert.equal(try sync.decode(json.field("change", json.int())), 2)
        save := try sync.decode(json.field("save", json.raw()))
        try assert.is_true(!try save.decode(json.field("includeText", json.bool())))
```

- [ ] **Step 9: Update the two earlier `didChange` state-assertion tests**

`"didChange replaces full document text"` and `"didChange applies incremental text change"` assert `diagnostics_pending` is true — still correct. No change needed unless they assert tier; they do not. Run the suite to confirm.

- [ ] **Step 10: Run tests**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -5`
Expected: `Ran N tests: N passed`.

- [ ] **Step 11: Format, lint, commit**

```bash
target/twk fmt boot/lib/lsp/params.tw boot/lib/lsp/server_core.tw boot/tests/suites/lsp_server_core_suite.tw
target/twk lint boot/main.tw
git add boot/lib/lsp/params.tw boot/lib/lsp/server_core.tw boot/tests/suites/lsp_server_core_suite.tw
git commit -m "Run full diagnostics on save, parse-only on edit"
```

---

## Task 5: Cache-only heavy-feature gating

Make `documentSymbol`, `foldingRange`, `inlayHint`, and `semanticTokens` serve only a completed full snapshot and never run inline analysis.

**Files:**
- Modify: `boot/lib/lsp/server_core.tw`
- Test: `boot/tests/suites/lsp_server_core_suite.tw`

- [ ] **Step 1: Write the failing test**

```tw
    .test(
      "document symbols return empty while full_dirty and without inline analysis",
      fn() {
        opened := document_store.empty().open("file:///a.tw", "fn f() Void { }\n", .Some(1))
        state := server_core.initial_state()
        state.documents = opened
        state.full_dirty = true
        req := protocol.Message.Request(.{
          id: .IntId(7),
          method: "textDocument/documentSymbol",
          params: .Some(json.object([
            json.kv("textDocument", json.object([json.kv("uri", .Str("file:///a.tw"))])),
          ])),
        })
        step := state.handle_message(req)
        out := try only_out(step)
        result := try out.decode(json.field("result", json.raw().list()))
        try assert.equal(result.len(), 0)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run test to verify it fails**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -5`
Expected: FAIL — the guard currently checks `diagnostics_pending` (false here), so it proceeds to `workspace_snapshot` and may produce non-empty symbols or run analysis.

- [ ] **Step 3: Add the cache-only accessor**

In `boot/lib/lsp/server_core.tw`, next to `workspace_snapshot` (line 249), add:

```tw
fn workspace_snapshot_cached(state: State, doc: document_store.Document) semantic.SemanticSnapshot? {
  semantic.snapshot_workspace_from_cache(state.query_cache, state.workspace_input(doc))
}
```

- [ ] **Step 4: Re-gate the four heavy features**

The change is the same two-part edit in each handler, applied wherever those two pieces sit (the inlay handler computes offsets between them — leave that ordering untouched):

**(a) Delete** the guard block that was added earlier in each of `handle_document_symbol`, `handle_folding_range`, `handle_inlay_hint`, `handle_semantic_tokens`:

```tw
  if state.diagnostics_pending {
    return state.empty_array_response_step(req)
  }
```

(For `handle_semantic_tokens`, delete its variant that returns the empty-`data` object instead.)

**(b) Replace** the snapshot fetch line in each handler. Currently each reads:

```tw
  snap := state.workspace_snapshot(doc)
  state.query_cache = snap.cache
```

For `handle_document_symbol`, `handle_folding_range`, `handle_inlay_hint`, replace those two lines with:

```tw
  snap := case state.workspace_snapshot_cached(doc) {
    .Some(s) => s,
    .None => { return state.empty_array_response_step(req) },
  }
  if state.full_dirty {
    return state.empty_array_response_step(req)
  }
  state.query_cache = snap.cache
```

For `handle_semantic_tokens`, replace the same two lines with the empty-`data` response shape:

```tw
  snap := case state.workspace_snapshot_cached(doc) {
    .Some(s) => s,
    .None => {
      return .{
        state,
        outgoing: [req.id.success_response(json.object([json.kv("data", json.array([]))]))],
      }
    },
  }
  if state.full_dirty {
    return .{
      state,
      outgoing: [req.id.success_response(json.object([json.kv("data", json.array([]))]))],
    }
  }
  state.query_cache = snap.cache
```

- [ ] **Step 5: Run tests**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -5`
Expected: `Ran N tests: N passed`.

- [ ] **Step 6: Format, lint, commit**

```bash
target/twk fmt boot/lib/lsp/server_core.tw boot/tests/suites/lsp_server_core_suite.tw
target/twk lint boot/main.tw
git add boot/lib/lsp/server_core.tw boot/tests/suites/lsp_server_core_suite.tw
git commit -m "Serve heavy LSP features from cached snapshot only, gated on full_dirty"
```

---

## Task 6: didClose refreshes the workspace and clears full_dirty

**Files:**
- Modify: `boot/lib/lsp/server_core.tw`
- Test: `boot/tests/suites/lsp_server_core_suite.tw`

- [ ] **Step 1: Write the failing tests**

```tw
    .test(
      "closing the last document clears full_dirty",
      fn() {
        opened := document_store.empty().open("file:///a.tw", "x := 1\n", .Some(1))
        state := server_core.initial_state()
        state.documents = opened
        state.full_dirty = true
        params := json.object([
          json.kv("textDocument", json.object([json.kv("uri", .Str("file:///a.tw"))])),
        ])
        msg := protocol.Message.Notification(.{
          method: "textDocument/didClose",
          params: .Some(params),
        })
        step := state.handle_message(msg)
        try assert.is_true(!step.state.full_dirty)
        .Ok({})
      },
    )
    .test(
      "closing one of several documents schedules a full run",
      fn() {
        opened := document_store
          .empty()
          .open("file:///a.tw", "x := 1\n", .Some(1))
          .open("file:///b.tw", "y := 2\n", .Some(1))
        state := server_core.initial_state()
        state.documents = opened
        params := json.object([
          json.kv("textDocument", json.object([json.kv("uri", .Str("file:///a.tw"))])),
        ])
        msg := protocol.Message.Notification(.{
          method: "textDocument/didClose",
          params: .Some(params),
        })
        step := state.handle_message(msg)
        try assert.is_true(step.state.diagnostics_pending)
        case step.state.diagnostics_tier {
          .Full => .Ok({}),
          _ => assert.fail("expected Full tier"),
        }
      },
    )
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -5`
Expected: FAIL — `didClose` neither clears `full_dirty` nor schedules a full run.

- [ ] **Step 3: Update `handle_did_close`**

Replace `handle_did_close` (lines 234-247) with:

```tw
fn handle_did_close(state: State, note: protocol.Notification) Step {
  case params.decode_did_close(note.params) {
    .Ok(close) => {
      uri := close.text_document.uri
      version := case state.documents.get(uri) {
        .Some(doc) => doc.version,
        .None => .None,
      }
      state.documents = .close(uri)
      state.parse_dirty_uris = state.parse_dirty_uris.remove(uri)
      cleared := lsp_diagnostics.clear_diagnostics(uri, version)

      // Closing an unsaved overlay shifts the rest of the workspace back to
      // disk-backed analysis, so refresh the remaining open docs. With nothing
      // open there is nothing to analyze, so just clear the dirty flag.
      if state.documents.docs.values().len() == 0 {
        state.full_dirty = false
        .{ state, outgoing: [cleared] }
      } else {
        refreshed := state.mark_diagnostics_pending(.Full, .None)
        .{ state: refreshed.state, outgoing: [cleared] }
      }
    },
    .Err(_) => .{ state, outgoing: [] },
  }
}
```

- [ ] **Step 4: Run tests**

Run: `target/twk run boot/tests/main.tw 2>&1 | tail -5`
Expected: `Ran N tests: N passed`. (The existing `"didClose removes the document"` and `"supports repeated open change close cycles"` tests should still pass; if either asserts no outgoing, confirm it closes the only open doc so the no-schedule branch is taken — otherwise update its expectation.)

- [ ] **Step 5: Format, lint, commit**

```bash
target/twk fmt boot/lib/lsp/server_core.tw boot/tests/suites/lsp_server_core_suite.tw
target/twk lint boot/main.tw
git add boot/lib/lsp/server_core.tw boot/tests/suites/lsp_server_core_suite.tw
git commit -m "Refresh workspace and clear full_dirty on didClose"
```

---

## Task 7: Scheduler debounce by tier + final validation

The diagnostics worker in `boot/commands/lsp.tw` is the LSP command (not exercised by the boot suite), so this task is validated by build + full suite + a fresh CLI bundle.

**Files:**
- Modify: `boot/commands/lsp.tw`

- [ ] **Step 1: Drive debounce from the per-tier deadline**

In `boot/commands/lsp.tw`, remove the `debounce_ms := 1000` constant (line 22). Replace `schedule_diagnostics` (lines 332-342) with a version that sleeps until the state deadline:

```tw
fn schedule_diagnostics(ctx: LoopCtx) {
  my_gen := ctx.gen.get() + 1
  ctx.gen.set(my_gen)
  deadline := ctx.shared.get().diagnostics_deadline_ms
  Task.spawn(fn() {
    wait := deadline - date.now()
    if wait > 0.0 {
      Task.sleep(wait.to_int())
    }

    if ctx.gen.get() == my_gen {
      ctx.diagnostics.push_job(my_gen)
    }
  })
}
```

If `date` is not yet imported in this file, add `use @std.date` with the other `use` lines. Confirm the import name with `grep -n "date\." boot/lib/lsp/server_core.tw` (server_core already uses `date.now()`), and mirror its import.

- [ ] **Step 2: Show progress only for full runs**

In `run_diagnostics_job` (lines 344-366), gate the progress UI on the tier. Change the opening of the function so progress begins only for `Full`:

```tw
fn run_diagnostics_job(ctx: LoopCtx, job_gen: Int) {
  if ctx.gen.get() != job_gen or !ctx.shared.get().diagnostics_due() {
    return
  }

  is_full := case ctx.shared.get().diagnostics_tier {
    .Full => true,
    .Parse => false,
  }

  token := if is_full {
    ctx.begin_progress("Checking workspace", "Preparing diagnostics", 0)
  } else {
    ""
  }
  if is_full {
    ctx.report_progress(token, "Analyzing open documents", 10)
  }

  step := ctx.shared.get().publish_due_diagnostics()

  if ctx.gen.get() == job_gen {
    if is_full {
      ctx.report_progress(token, "Publishing diagnostics", 90)
    }
    ctx.shared.set(step.state)

    for out in step.outgoing {
      ctx.write_message(out)
    }

    ctx.end_progress(token, "Diagnostics ready")
  } else {
    ctx.end_progress(token, "Diagnostics superseded")
  }
}
```

(`begin_progress`/`report_progress`/`end_progress` already early-return when the token is `""`, so the parse path emits no progress.)

- [ ] **Step 3: Build and run the full suite**

Run:
```bash
target/twk build boot/main.tw -o /tmp/check.wasm 2>&1 | tail -3
target/twk run boot/tests/main.tw 2>&1 | tail -3
```
Expected: build succeeds; `Ran N tests: N passed`.

- [ ] **Step 4: Format, lint, rebuild the CLI**

```bash
target/twk fmt boot/commands/lsp.tw
target/twk lint boot/main.tw
make bundle-cli
```
Expected: `make bundle-cli` completes (self-host stays green and `target/twk` is rebuilt with the new behavior).

- [ ] **Step 5: Commit**

```bash
git add boot/commands/lsp.tw
git commit -m "Debounce diagnostics by tier; progress only for full runs"
```

---

## Final verification checklist

- [ ] `target/twk build boot/main.tw -o /tmp/check.wasm` succeeds.
- [ ] `target/twk run boot/tests/main.tw` reports all tests passing.
- [ ] `target/twk lint boot/main.tw` reports no findings.
- [ ] `make bundle-cli` completes (self-host green).
- [ ] No `cargo test` needed — stage0/Rust untouched (boot-only change).

## Notes for the implementer

- `parse_dirty_uris` is a `Dict<String, Bool>` used as a string set (matching the existing `published`/`seen` pattern in the codebase). Dict API: `Dict.new()`, index-set sugar `d[k] = true`, `.has(k)`, `.remove(k)` (returns a new dict), `.keys()`, `.len()`. (`Set<String>` is avoided as a record field because monomorphizing it inside `server_core` collides with the prelude's `set$*` methods at link time.)
- `query_diagnostics` is the existing alias for `compiler.query.diagnostics` in `server_core.tw`.
- Record literal updates require every field; the `State` shape changed in Task 2, so any new `State.{ … }` literal must include `diagnostics_tier`, `parse_dirty_uris`, `full_dirty`, and `last_published`.
- If a test's entry URI is not `file:///main.tw`, substitute the URI that the test asserts on when adding `didSave` triggers.
