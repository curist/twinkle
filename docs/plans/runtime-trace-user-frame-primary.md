# Runtime Trace: User Frame as Primary (Phase 4.1)

Status: Design (approved)
Date: 2026-09-09

Phase 4.1 of the runtime source-mapped stack traces feature
(`docs/plans/runtime-stack-traces.md`). Phases 0–2 and the disk-backed debug
info milestones (M1 + M2) are landed on `main`. This phase is the first
rendering-polish item: make the primary snippet/caret land on the user's call
site instead of a prelude/stdlib internal.

## Motivation

`render_trace` picks the **innermost frame that resolves at all** as the
"primary" — the frame that gets the source snippet + caret. Since Milestone 1,
prelude and stdlib code compiles **with** debug info, so those frames resolve.
The consequences today:

- `error("msg")` traps in the prelude `error` shim, so the caret points at
  `@std/io.tw:19:3` rather than the user's `error(...)` call site.
- Any trap whose innermost resolving frame is a stdlib op called from user code
  (e.g. a Vector out-of-bounds inside `@std`) points the caret into the stdlib
  rather than at the user line that made the call.

The user-meaningful location — the one a developer wants the caret on — is the
first **user** frame in the stack. The module's own doc comment already claims
"frames that do not map to Twinkle source ... are dropped so the trace shows
only user-meaningful frames"; this phase makes reality match that intent.

## Key lever (from Milestone 2)

M2 changed `twinkle.debug` to store each file's path as a **project-relative**
string for user files and an **`@`-logical** name (`@std/…`, `@extern/…`) for
stdlib/prelude/extern files. `ResolvedFrame.location.file` carries that stored
string verbatim. Therefore:

> **A frame is a user frame ⇔ its resolved `location.file` does not start with `@`.**

This is the exact inverse of what `safe_join` already refuses, so no new data,
no flag byte, and no change to `symbolicate.tw` are needed. The classification
is a pure string check on information the renderer already has.

## Scope

- **In scope:** two-tier primary/backtrace selection in `trace.tw` that prefers
  user frames; a fallback that preserves current behavior when no user frame
  resolves; the doc-comment correction; boot + CLI e2e test coverage (including
  flipping the one existing e2e that encodes the old deferred behavior).
- **Out of scope (other Phase 4 items, untouched):** name demangling
  (`user__$f350_deeper` → `deeper`), the "runtime" headline prefix, ANSI-color
  parity with compile diagnostics.
- **Unchanged:** `symbolicate.tw` (pure resolution), `section.tw` (v3 format),
  the host boundary / `run_wasm` ABI, and all M1/M2 degradation rules.

## Decisions

- **Policy lives in the renderer (`trace.tw`), not in symbolication.**
  `symbolicate.tw`'s job is mapping PCs to spans/locations; deciding which
  resolved frame is "primary" is presentation policy. Keep the layers separate.
  No `is_user` field is added to `ResolvedFrame` (it would be consumed only by
  `trace.tw` — YAGNI); the predicate reads `rf.location.file` directly.
- **User frame predicate:** `is_user_frame(rf)` is true iff `rf.location` is
  `.Some(loc)` and `!loc.file.starts_with("@")`. A frame with no resolved
  location is not a user frame (it is dropped by the existing backtrace logic
  regardless).
- **Two-tier selection.** Let `user_frames` be the subset of `resolved` for
  which `is_user_frame` holds, in stack order.
  - **`user_frames` non-empty** → the primary snippet/caret is the **first**
    user frame; the backtrace renders **only** the user frames (interior and
    leading `@` frames are omitted).
  - **`user_frames` empty** but some frame resolves (an all-stdlib trap with no
    user frame in the retained group) → **fall back to current behavior**:
    primary = the first resolving frame (an `@` frame, which renders
    location-only because `safe_join` refuses `@` paths), backtrace = all
    resolved frames. This guarantees the trace is never empty and preserves the
    M1/M2 all-stdlib degradation exactly.
  - **No frame resolves at all** → unchanged `(no Twinkle stack trace
    available)`.
- **`snippet_ok` is unaffected in spirit.** It keys off
  `reg.line_col(primary.span)` on the disk registry. When the primary is a user
  frame, `reg` contains that file iff it safe-joined and read from disk → full
  snippet; if the user source is gone → location-only + `source unavailable`.
  When the primary is an all-`@` fallback frame, `reg` never contains `@` files
  → location-only. Both are the existing, correct behaviors.

## Architecture

All changes are in `boot/lib/debug/trace.tw`. `render_trace`'s shape is
unchanged; only how `primary` and the backtrace list are derived changes.

```
render_trace(section, frames, message, config, source_root):
  reg      = build_disk_registry(section, source_root)   # unchanged
  resolved = symbolicate(section, frames)                # unchanged
  users    = [ rf for rf in resolved if is_user_frame(rf) ]

  chosen   = if users is non-empty { users } else { resolved }
  primary  = first frame in `chosen` that has a span         # SpanLabel?
  bt       = backtrace lines for the frames in `chosen`      # was: over `resolved`

  # headline + snippet + backtrace assembly, snippet_ok, and the three
  # output branches (no-bt note / snippet / source-unavailable) are unchanged.
```

Concretely:

- **`is_user_frame(rf: ResolvedFrame) Bool`** — new private helper.
- **`primary_label(resolved)`** — takes the already-chosen frame list (or gains
  the two-tier logic internally) and returns the first frame with a span, as
  today. Its body is otherwise the current loop.
- **`backtrace_lines(resolved)`** — takes the chosen frame list. Its per-frame
  formatting (`  at name (file:line:col)`, `<anonymous>` for unnamed) is
  unchanged.
- The `chosen = if users non-empty { users } else { resolved }` decision is
  computed once in `render_trace` and passed to both, so primary and backtrace
  always agree on which frames they describe.

Because there is **no ABI change**, no bootstrap sequencing is required (unlike
M2's `run_wasm` widening). The renderer is rebuilt from source directly.

## Testing

**Boot suites** (`boot/tests/suites/trace_render_suite.tw` and
`runtime_trace_renderer_suite.tw`), building sections manually with mixed
relative and `@std/…` file entries:

1. **`@std`-then-user stack** (the headline fix): innermost frame resolves to
   `@std/io.tw`, next frame to a user `main.tw`. Assert the snippet/caret is on
   `main.tw` and the backtrace does **not** contain the `@std/io.tw` line.
2. **Trap directly in user code** (regression guard): a user-only div0 stack
   (no `@` frame) renders exactly as it does today — primary on the user frame,
   backtrace user-only.
3. **Interior `@` frame between two user frames**: user → `@std` → user. Assert
   the first user frame is primary and **both** user frames appear in the
   backtrace while the interior `@std` line is dropped.
4. **All-`@` stack** (fallback): only `@std/…` frames resolve, no user frame.
   Assert the primary is the first `@` frame, output is location-only
   (`source unavailable`), the backtrace lists the `@` frames, and it is
   **never** the empty `(no Twinkle stack trace available)` note.
5. **Nothing resolves**: unchanged `(no Twinkle stack trace available)`.

**CLI e2e** (`tools/js_runtime/cli.test.mjs`):

- **Flip the existing test** "twk run degrades to a location-only trace when the
  trap surfaces through the prelude error() shim". That test currently encodes
  the *old, deferred* behavior (its comment calls choosing the user frame
  "explicitly deferred rendering polish"). Rewrite it to assert that an
  `error("…")` program now renders a **snippet + caret on the user's `error(…)`
  call site** (relative path), with exit 1. `buildRenderer()` compiles the
  renderer from source with the current compiler, so it picks up the change
  without a full rebuild.

**Ship.** After the boot + JS suites are green, one `make bundle-cli` rebuilds
`target/renderer.wasm` so the shipped `target/twk` renders the new primary.
Confirm manually: `twk run` on an `error(…)` fixture points the caret at the
user call line, and a normal program still exits 0.

## Non-goals (deferred, tracked in `runtime-stack-traces.md`)

- Name demangling of mangled frame names in the backtrace.
- "runtime" headline prefix and ANSI-color parity with compile diagnostics.
- Any change to what the stack parser retains (`stack.tw`), to symbolication, or
  to the debug-section format.
