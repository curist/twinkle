# Disk-Backed Runtime Debug Info

Status: Planned
Date: 2026-09-07

Follow-on to `docs/plans/runtime-stack-traces.md` (Phases 0–2 complete). That
work embeds each program's source text inline in the `twinkle.debug` section so
the trace renderer is self-contained. This detour replaces inline source with
**disk-backed** recovery: the section carries only paths and locations, and the
renderer reads snippets from the source tree at render time — but only on
`twk run`, where the source is guaranteed present and fresh.

## Motivation

The inline-source format makes the section large: because the boot compiler
references nearly every module, its own `twinkle.debug` roughly doubles
`boot.wasm` (~4.5 MB → ~9 MB), and that now ships in `target/twk`. Source text is
the bulk of the section. Removing it — while keeping full source-mapped traces
for the common `twk run` workflow — is the lever.

## Goal

Unchanged rendering target for `twk run`:

```
runtime error: index out of bounds

  src/grid.tw:42:11
   42 |   cells.at(idx)
      |         ^^

  at Vector.at (src/grid.tw:42:11)
  at Grid.cell (src/grid.tw:18:5)
  at main      (src/main.tw:7:3)
```

When the source is unavailable (a `twk build` artifact run later, a moved file,
`@std`/prelude frames) it degrades to the frame list plus a note, never a crash:

```
runtime error: index out of bounds

  at Vector.at (src/grid.tw:42:11)
  at Grid.cell (src/grid.tw:18:5)
  at main      (src/main.tw:7:3)

  source unavailable
```

## Decisions (from brainstorming)

- **Disk-backed only.** The section never carries source text. There is no
  inline-source mode and no per-file inline/checksum flag. (`--strip-debug` — an
  empty section — is a separate future enhancement, not in scope.)
- **Source restore is scoped to `twk run`.** Only the compile-and-run path
  enriches frames with snippets, because that is the one context where the
  source tree is present, unchanged, and rooted at a root the compiler just
  computed. A `twk build` artifact executed later, or a `proc.run_wasm(bytes)`
  call, renders location-only. This removes every cross-machine, stale-source,
  and render-time-root-rederivation concern.
- **No hashes.** Their only job was detecting stale on-disk source; in the
  `twk run` path source cannot be stale. Deferred behind the format version byte
  for a future standalone-restore path.
- **Locations carry both byte span and line/col.** Line/col is mandatory (the
  only way a source-less artifact can print `file:line:col`). The byte span is
  retained so the `twk run` snippet path stays the *existing* `render.tw` /
  `FileRegistry` code, with source coming from disk instead of the section. The
  cost is a couple of delta-encoded ints per entry.
- **Project-relative paths.** The section stores paths relative to the run's
  project root (`find_project_root(entry)`; single file with no manifest ⇒ the
  entry's directory, so the path is the basename). Absolute build-machine paths
  are never embedded. Logical `@std`/prelude paths resolve outside any user root
  and therefore fall to location-only.
- **Trust boundary in the renderer, in Twinkle.** Debug-section decoding and
  source recovery stay entirely in the renderer; the host does not grow a second
  implementation. The renderer safe-joins each relative path beneath the passed
  `source_root` and rejects traversal, so even a malformed section cannot turn
  trace rendering into arbitrary file access.

## Architecture

```
COMPILE (twk run / twk build)              RENDER (twk run only, at run_wasm boundary)
──────────────────────────────            ─────────────────────────────────────────────
find_project_root(entry) = R              child traps → childTrapHandler(info, bytes)
                                                    │
serialize twinkle.debug:                  render_runtime_trace(bytes, stack, message,
  file_id → path relative to R                                  source_root = R)
  line program per fn:                            │
    (pc → byte span + line/col)           decode twinkle.debug (no source inside)
  NO source text, NO hash                 parse stack → frames → symbolicate
                                                    │
                                          per frame: safe-join(path, source_root)
                                            ├─ under root + readable → FileRegistry
                                            │    from disk source → render.tw snippet
                                            └─ missing / escapes / logical → location-only
                                                    │
                                          print once to stderr; nonzero exit
```

The host passes `source_root` only on the `twk run` path; every other run
context passes none, yielding location-only traces.

## Components

### 1. Format (`boot/lib/debug/section.tw`)

Bump `version()` to 2. Changes:

- **File table entry:** `{ file_id, path }` — drop the inline-source flag byte
  and the source string.
- **Line-program entry:** `{ offset, file_id, start, end, start_line, start_col,
  end_line, end_col }`, delta-encoded (offset already is; line/col deltas from
  the previous entry). Today's entry is `{ offset, span{file_id,start,end} }`;
  the four line/col fields are added.
- `decode`/`encode`, `span_at`, `lookup`, and `decode_module` update to the new
  shape. `file_source` (inline-source lookup) is removed.

### 2. Compile-time producer

- **Path relativization.** Where the file table is built
  (`boot/compiler/codegen/wasm.tw`, from `cache.Store` records threaded via
  `PipelineArtifacts.debug_files`), store each path relative to the compilation's
  project root instead of the canonical/absolute path. The root is
  `find_project_root(entry)` (`lib.module.loader`), already computed during
  `compile_entry`; thread it to the serializer.
- **Line/col precompute.** For every line-program span, compute
  `(line, col)` for `start` and `end` using a line index over the source the
  compiler already holds (`cache.Store.file_records` /
  `PipelineArtifacts.debug_files`). `boot/lib/source` already has line/col
  machinery (`FileRegistry.line_col`); reuse or factor a small line-index helper.
- No source bytes are written into the section.

### 3. Render-time renderer (`boot/runtime_trace_renderer.tw` + `boot/lib/debug/*`)

- **Signature:** `render_runtime_trace(bytes: Vector<Byte>, stack: String,
  message: String, source_root: String) String`. An empty `source_root` means
  "no source lookup" (location-only for every frame).
- **`symbolicate` / registry:** stop reconstructing a `FileRegistry` from
  embedded source. Instead, for a frame whose file resolves under `source_root`,
  read the file via `@std.fs` and build the registry from disk source; feed the
  existing byte-`Span` path through `render.tw` for the snippet + caret.
- **Location-only path:** when there is no `source_root`, the path escapes the
  root, or the file is unreadable, render the headline + `file:line:col` (from
  embedded line/col) + backtrace + a `source unavailable` note. No snippet.
- **Safe join:** normalize `join(source_root, rel_path)` and require the result
  to remain under `source_root`; otherwise location-only. Never read absolute or
  `..`-escaping paths.

### 4. Host plumbing (`tools/js_runtime/*`)

- The `run_wasm` import already builds the child `childTrapHandler`. Extend the
  handler to pass `source_root` to `render_runtime_trace`. The boot CLI computes
  it from the entry path it launched the child with (same `find_project_root`
  rule); it is threaded onto `runtime` like `rendererWasm` and forwarded to
  nested `run_wasm` calls.
- The renderer lib now imports `@std.fs`; the sync lib loader (`loadLibSync`)
  already instantiates it with a host adapter, so file reads resolve through the
  same host. Confirm the loader/bridge provide the fs imports the renderer needs.
- `twk build` artifacts and raw `proc.run_wasm(bytes)` continue to pass no
  `source_root` ⇒ location-only.

## Plan

### Phase A — Format v2
- Rewrite the `section.tw` codec for the new file-table and line-program shape;
  bump the version; drop `file_source` and the inline-source flag.
- Update `debug_section_suite` round-trip/boundary/lookup tests.

### Phase B — Producer
- Thread the project root to the serializer; relativize paths.
- Precompute line/col for every line-program span from in-memory source.
- Update codegen/wasm tests and any fixtures asserting the old section shape.

### Phase C — Renderer + host
- Add the `source_root` parameter; switch source recovery to disk with safe-join
  and location-only degradation; keep the `render.tw` snippet path.
- Thread `source_root` through the `run_wasm` boundary; give the renderer lib
  `@std.fs`.
- Boot suite: renderer against a temp-dir root (present / missing / traversal),
  location-only path. CLI e2e: `twk run` shows a snippet; a pre-built artifact
  shows location-only.

### Phase D — Verify
- `make stage2` fixed point; `make bundle-cli`; confirm `boot.wasm` drops back
  toward its pre-debug size. Re-run the boot + JS suites and the `twk run`
  trap fixtures (error / div0 / OOB).

## Non-goals (deferred)

- `--strip-debug` (empty section for production).
- Content hashes / staleness detection.
- Standalone-artifact source restore with an explicitly supplied root, and
  cross-machine crash reports.
- Rendering polish tracked in `runtime-stack-traces.md` Phase 4 (prelude-frame
  suppression, name demangling, ANSI parity), independent of this format change.
