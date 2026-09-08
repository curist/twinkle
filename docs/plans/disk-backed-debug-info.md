# Disk-Backed Runtime Debug Info

Status: Planned
Date: 2026-09-08

Follow-on to `docs/plans/runtime-stack-traces.md` (Phases 0–2 complete). That
work embeds each program's source text inline in the `twinkle.debug` section so
the trace renderer is self-contained. This detour replaces inline source with
**disk-backed** recovery: the section carries only paths and locations, and the
renderer reads snippets from the source tree at render time — but only on
`twk run`, where the source is present and (in practice) fresh.

## Motivation

The inline-source format makes the section large: because the boot compiler
references nearly every module, its own `twinkle.debug` roughly doubles
`boot.wasm` (~4.5 MB → ~9 MB), and that now ships in `target/twk`. Source text is
the bulk of the section. Removing it — while keeping full source-mapped traces
for the common `twk run` workflow — is the lever.

## Decomposition (two milestones)

The original single plan bundled the safe, high-value win (drop source text)
with its riskiest part (relativize paths to a project root, safe-join under a
render-time root, fix `find_project_root`). The size win — the whole point —
comes entirely from *dropping source text*, which is independent of path
relativization. So this splits into:

- **Milestone 1 (this plan): drop source, keep absolute paths, read from disk.**
  The section stores each source file's **absolute** path plus per-entry line/col
  and byte spans, and no source text. On `twk run`, the renderer reads that
  absolute path straight from disk and reuses the existing snippet path. This
  delivers the ~2× size cut **and** working `twk run` snippets, with none of the
  root-computation, relativization, or traversal-guard machinery — and, because
  the path is read directly, no `source_root` argument and no change to the host
  boundary. Unreadable files, `@std`/prelude frames, and any non-`twk run`
  context degrade to `file:line:col` + backtrace.

- **Milestone 2 (deferred — see Non-goals): portability / hardening.** Project-
  relative paths, a render-time `source_root`, safe-join with traversal
  rejection, fixing `find_project_root`'s no-manifest fallback, and threading the
  project root through the pipeline. This layer only matters for **shipped
  artifacts** (don't leak build-machine paths; tolerate untrusted sections) —
  i.e. the standalone-restore scope already deferred in
  `runtime-stack-traces.md`. It is written up here so the format leaves room for
  it, but it is not built in this plan.

## Goal (Milestone 1)

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
  source tree is present. A `twk build` artifact executed later, or a
  `proc.run_wasm(bytes)` call, renders location-only.
- **Milestone 1 stores absolute paths and reads them directly.** No project-root
  computation at compile time and no `source_root` at render time. This keeps
  the renderer signature at the just-shipped 3 args
  (`render_runtime_trace(bytes, stack, message)`) and leaves the `run_wasm` host
  boundary untouched. Relativization (and the portability it buys) is
  Milestone 2.
- **No hashes.** Their only job was detecting stale on-disk source. Deferred
  behind the format version byte.
- **Staleness is a documented limitation, not a detected error.** A long-running
  program that traps after its source was edited on disk may render a snippet
  from the edited bytes against compile-time spans — i.e. a wrong snippet. There
  is no tripwire (consistent with "no hash"): the byte offsets still point
  somewhere in the file, the caret may just be off. Documented, not guarded.
- **Locations carry both byte span and line/col, both absolute.** Line/col is
  mandatory (the only way a source-less frame can print `file:line:col`). The
  byte span is retained so the snippet path stays the *existing* `render.tw` /
  `FileRegistry` code, with source coming from disk. **All four line/col fields
  are stored absolute, not delta-encoded** — the optimizer reorders and inlines
  code, so source positions are non-monotonic across line-program entries, and
  the `push_uleb` encoder is unsigned-only; a negative delta would corrupt output
  or hang. This matches how `span.file_id/start/end` are already stored (absolute,
  for the same reason). Only `offset` (monotonic) stays delta-encoded.

## Architecture (Milestone 1)

```
COMPILE (twk run / twk build)              RENDER (twk run only, at run_wasm boundary)
──────────────────────────────            ─────────────────────────────────────────────
serialize twinkle.debug:                  child traps → childTrapHandler(info, bytes)
  file_id → absolute path                          │
  line program per fn:                     render_runtime_trace(bytes, stack, message)
    (pc → byte span + abs line/col)                │
  NO source text, NO hash                  decode twinkle.debug (no source inside)
                                           parse stack → frames → symbolicate
                                                    │
                                           per frame:
                                             ├─ read absolute path via @std.fs
                                             │    readable → FileRegistry from disk
                                             │    source → render.tw snippet
                                             └─ unreadable / logical (@std) → location-only
                                                    │
                                           print once to stderr; nonzero exit
```

The host boundary is unchanged from Phase 2: `childTrapHandler` already forwards
`rendererWasm`; no `source_root` is threaded.

## Components (Milestone 1)

### 1. Format (`boot/lib/debug/section.tw`)

Bump `version()` to 2. Changes:

- **File table entry:** `{ file_id, path }` — drop the inline-source flag byte
  and the source string. `path` is the absolute path the compiler already holds.
- **Line-program entry:** `{ offset, file_id, start, end, start_line, start_col,
  end_line, end_col }`. `offset` stays delta-encoded; `file_id/start/end` stay
  absolute (as today); the four new line/col fields are **absolute** too.
- `decode`/`encode`, `span_at`, `lookup`, and `decode_module` update to the new
  shape. `file_source` (inline-source lookup) is removed.

### 2. Compile-time producer

- **Drop source text.** Where the file table is built
  (`boot/compiler/codegen/wasm.tw`, from `cache.Store` records threaded via
  `PipelineArtifacts.debug_files`), stop writing the source string; keep the
  absolute path unchanged. No project-root computation, no relativization.
- **Line/col precompute.** For every line-program span, compute `(line, col)` for
  `start` and `end` using the source the compiler already holds
  (`cache.Store.file_records` / `PipelineArtifacts.debug_files`). Reuse
  `FileRegistry.line_col` (`boot/lib/source`) exactly, so the precomputed columns
  are codepoint columns identical to what the renderer would have produced from
  the same source — no UTF-8/byte drift between producer and renderer.

### 3. Render-time renderer (`boot/runtime_trace_renderer.tw` + `boot/lib/debug/*`)

- **Signature unchanged:** `render_runtime_trace(bytes: Vector<Byte>, stack:
  String, message: String) String`.
- **`symbolicate` / registry:** stop reconstructing a `FileRegistry` from
  embedded source. For a frame whose absolute path is readable, read it via
  `@std.fs` and build the registry from disk source; feed the existing
  byte-`Span` through `render.tw` for the snippet + caret.
- **Location-only path:** when the file is unreadable or the path is logical
  (`@std`/prelude, which never resolves to a real file), render the headline +
  `file:line:col` (from embedded absolute line/col) + backtrace + a
  `source unavailable` note. No snippet, never a crash.

### 4. Host plumbing (`tools/js_runtime/*`)

- No change to the `run_wasm` boundary or `childTrapHandler` — it already
  forwards `rendererWasm`, and Milestone 1 adds no `source_root`.
- The renderer lib now imports `@std.fs`; the sync lib loader (`loadLibSync`)
  instantiates it with a host adapter, so file reads resolve through the same
  host. **Gate C0 below confirms this actually works** before the renderer is
  rewritten.

## Plan (Milestone 1)

### Phase C0 — `@std.fs`-in-lib spike (gate)
- Before any format change: confirm a `--lib` artifact can import `@std.fs` and
  read a file through `loadLibSync`'s host adapter. The current renderer lib
  imports no externs; this is the one unproven capability the whole milestone
  rests on. A throwaway lib that reads a known file and returns its length is
  enough. If the sync loader can't supply the fs imports, resolve that here
  (extend the adapter) before proceeding.

### Phase A — Format v2
- Rewrite the `section.tw` codec for the new file-table and line-program shape
  (absolute line/col, no source string); bump the version; drop `file_source`
  and the inline-source flag.
- Update `debug_section_suite` round-trip/boundary/lookup tests.

### Phase B — Producer
- Stop writing source text into the section.
- Precompute absolute line/col for every line-program span via
  `FileRegistry.line_col`.
- Update codegen/wasm tests and any fixtures asserting the old section shape.

### Phase C — Renderer
- Switch source recovery to disk (`@std.fs`) keyed on the embedded absolute path;
  keep the `render.tw` snippet path; degrade to location-only on unreadable /
  logical paths.
- Boot suite: renderer against a temp-dir file (present / missing), location-only
  path. CLI e2e: `twk run` shows a snippet; a pre-built artifact run later shows
  location-only.

### Phase D — Verify
- `make stage2` fixed point; `make bundle-cli`; confirm `boot.wasm` drops back
  toward its pre-debug size. Re-run the boot + JS suites and the `twk run`
  trap fixtures (error / div0 / OOB).

## Non-goals (deferred)

- **Milestone 2: project-relative paths + `source_root` + safe-join + portability.**
  Store paths relative to `find_project_root(entry)`; add a render-time
  `source_root` parameter (this breaks the 3-arg format-of-record and re-widens
  the `run_wasm` boundary); safe-join each relative path beneath `source_root`
  and reject traversal; fix `find_project_root`'s no-manifest fallback (today it
  returns `path.normalize(".")` — the launching CWD — **not** the entry's
  directory, so a bare `twk run /tmp/trap.tw` can't relativize the entry file);
  thread the project root through `PipelineArtifacts` → pipeline → codegen →
  linker → wasm. Reuse the private `strip_root` helper (analyze.tw) rather than a
  second implementation. This is what makes shipped artifacts safe to relocate
  and share; Milestone 1 deliberately does not need it.
- `--strip-debug` (empty section for production).
- Content hashes / staleness detection.
- Standalone-artifact source restore with an explicitly supplied root, and
  cross-machine crash reports.
- Rendering polish tracked in `runtime-stack-traces.md` Phase 4 (prelude-frame
  suppression, name demangling, ANSI parity), independent of this format change.
