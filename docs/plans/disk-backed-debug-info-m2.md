# Disk-Backed Debug Info — Milestone 2 (Portability / Hardening)

Status: Design (brainstorming)
Date: 2026-09-08

Follow-on to Milestone 1 (`docs/plans/disk-backed-debug-info.md`, landed on
branch `feat/runtime-stack-traces`). M1 dropped inline source text from the
`twinkle.debug` section and made `twk run` read snippets from disk — but it
stored **absolute** build-machine paths and read them directly, with no
render-time root. M2 makes the section **portable**: paths are stored
**project-relative**, and the renderer re-joins them under a **`source_root`**
supplied at render time, safe-joining to reject traversal.

## Motivation

M1's absolute paths have two problems for anything beyond the local machine:

1. **They leak the build machine's directory layout** into every shipped
   `twk build` artifact (`/Users/alice/work/proj/src/grid.tw`).
2. **They don't relocate.** Move or share the project and the absolute path no
   longer resolves — the snippet silently degrades to location-only even though
   the source is right there under a different prefix.

M2 stores paths relative to the project root and resolves them against a root
determined at run time, so the same artifact renders correct snippets wherever
the project tree lives, and carries no absolute build paths. This is the
"portability / hardening" layer M1 explicitly deferred; it only matters for
shipped/relocatable artifacts, which is why it was split out.

## Scope

- **In scope:** project-relative path storage; a render-time `source_root`
  threaded from the compiler's already-computed `project_root`; safe-join with
  traversal rejection; the `find_project_root` no-manifest fix; the
  location-only degradation rules that keep a missing/absent root from ever
  reading the wrong file.
- **Out of scope (unchanged from M1's non-goals):** content hashes / staleness
  detection; `--strip-debug`; cross-machine crash-report upload; Phase-4
  rendering polish (prelude-frame suppression, name demangling, ANSI parity).
- **Still `twk run`-only for snippets.** `twk build` artifacts run later, and
  raw `proc.run_wasm(bytes)`, receive no `source_root` and render location-only
  — exactly as in M1.

## Decisions

- **Paths are project-relative or logical.** A file under the project root is
  stored relative to it (`src/grid.tw`). A file under stdlib/prelude, or outside
  every known root, is stored as a **logical name** the renderer never
  disk-reads. No absolute path is ever embedded.
- **The `@` prefix is the logical marker.** Project-relative paths never begin
  with `@`; logical names always do (`@std/vector.tw` for stdlib/prelude,
  `@extern/<basename>` for outside-root files). The renderer partitions on the
  string alone — no per-file flag byte. This reuses the `@std/…` convention the
  compiler already prints in diagnostics (via `display_module_name`), so a
  logical frame reads as `@std/vector.tw:12:3`. Accepted limitation: a project
  directory literally named `@…` would be misclassified as logical; module
  directories are snake_case, so this is treated as a non-issue.
- **Version byte 2 → 3.** The field layout is identical to M1's v2
  (`FileEntry{file_id, path}`, `LineEntry` with absolute line/col); only the
  meaning of `path` changes (absolute → relative/logical). Bumping the version
  keeps `decode` honest rather than silently reinterpreting the path semantics,
  and lets a reader tell a v2 (absolute) section from a v3 (relative) one.
- **`source_root` is threaded, not recomputed.** The compiler computes
  `project_root` once (`module_compiler.tw:58`,
  `find_project_root(dirname(entry))`); M2 carries it on `PipelineArtifacts` so
  the producer relativizes with it and the `twk run` command hands the same
  value to the renderer. The JS host never re-derives path logic (avoids a
  second, drift-prone `find_project_root`).
- **No `source_root` ⇒ location-only for every frame.** A renderer holding
  relative paths but no root must **never** join them against the process CWD —
  that could surface an unrelated file that happens to share the relative path.
  Absence of a root is an unconditional location-only signal.
- **`find_project_root` no-manifest fallback returns the entry's own
  directory**, not `path.normalize(".")` (the launching CWD). Every real caller
  already passes the entry directory (`find_project_root(path.dirname(file))`),
  so this fixes the single-file `twk run /tmp/foo.tw` case at the source and
  matches what Rust stage0 already does (`find_project_root(parent)`).
- **Nearest `twinkle.toml` wins.** `find_project_root` already walks up and
  stops at the first manifest, so a file inside a nested project relativizes to
  the nested root. No change; called out for reviewer clarity.
- **Staleness stays documented, not detected.** As in M1: no hash, no tripwire.
- **Trust boundary stays in the renderer, in Twinkle.** Debug-section decoding
  and source recovery live entirely in the renderer lib; the host grows no
  second implementation. Safe-join is the guard that a malformed/hostile section
  cannot turn trace rendering into arbitrary file reads.

## Architecture

```
COMPILE (twk run / twk build)                 RENDER (twk run only)
─────────────────────────────                ─────────────────────────────
dir = dirname(entry)                          child traps → childTrapHandler(info, bytes)
project_root = find_project_root(dir)                 │
canonical_roots (already computed)            render_runtime_trace(bytes, stack,
                                                              message, source_root)
producer classifies each file path:                   │
  under project_root  → "src/grid.tw"         decode v3 twinkle.debug
  stdlib / prelude    → "@std/…"  (logical)   parse stack → frames → symbolicate
  outside all roots   → "@extern/<base>"              │
  → store in twinkle.debug (v3)               per frame:
                                                source_root present AND path not "@…"?
run_file:                                       ├ yes → safe_join(source_root, path)
  source_root = artifacts.project_root                │    ├ under root & readable → snippet
  proc.run_wasm(bytes, argv, source_root)             │    └ escapes / unreadable → location-only
                                                └ no  → location-only
                                                            │
                                              location-only frames still print file:line:col
                                              (embedded line/col) + backtrace; primary snippet
                                              omitted + "source unavailable"; print once; exit 1
```

`source_root` flows: `PipelineArtifacts.project_root` → `run_file` →
`proc.run_wasm(bytes, argv, source_root)` extern → host stores it on the run
opts → `childTrapHandler` passes it to `render_runtime_trace`'s 4th argument.
Only top-level `twk run` sets it; nested `proc.run_wasm` calls and pre-built
artifacts leave it empty.

## Components

### 1. `find_project_root` fix (`boot/lib/module/loader.tw`)

Preserve the original start directory and return it on the no-manifest path:

```tw
pub fn find_project_root(start: String) String {
  origin := path.normalize(start)
  dir := origin
  for true {
    if fs.exists(path.join(dir, "twinkle.toml")) {
      return dir
    }
    parent := path.dirname(dir)
    if parent == dir {
      return origin   // no manifest: the entry's own directory, not CWD
    }
    dir = parent
  }
  origin
}
```

Only manifest-less runs change behavior, and only to resolve from the entry
directory instead of `.`. `module_loader_suite` gains a no-manifest case
asserting the entry-dir fallback. (Rust stage0's `find_project_root` is left
as-is: only the boot compiler emits `twinkle.debug`, and boot always has a
manifest, so self-host is unaffected.)

### 2. Producer relativization (`boot/compiler/…`)

- **Expose the classification (not the formatting).** `strip_root` and the
  project/stdlib/prelude/basename logic currently live privately in
  `query/analyze.tw` behind `display_module_name`, which formats its own display
  strings (notably it returns prelude files as a *bare* relative name). Factor
  only the **classifier** into a small shared helper (in `lib.source` or
  `lib.module`) — one that returns a category + the relative remainder, e.g.
  `PathClass = { Project(String), Std(String), Prelude(String), Extern(String) }`
  — leaving each consumer to format its own strings. `display_module_name` keeps
  producing its current display forms from that classification (behavior
  unchanged); the debug producer formats the **`@`-marked** debug form from the
  same classification: `Project(rel) → rel`, `Std(rel)/Prelude(rel) → "@std/${rel}"`,
  `Extern(base) → "@extern/${base}"`. One classification definition, two
  formatters — so the prelude bare-vs-`@std` difference is a formatting choice,
  not a fork in the path logic.
- **Carry the root.** Add `project_root: String` (and, if convenient, the
  `canonical_roots`) to `PipelineArtifacts` so both the producer and `run_file`
  read the single computed value.
- **Relativize at emit.** Where the file table is built
  (`boot/compiler/codegen/wasm.tw`, from the threaded `DebugFile` records), map
  each `DebugFile.path` (canonical/absolute) through the shared classifier
  before storing it. No absolute path reaches the section. Line/col precompute
  is unchanged from M1.

### 3. Root plumbing (`boot/commands/run.tw`, `proc`, host)

- **Extern + wrapper.** Widen `run_wasm` to carry the root:
  `run_wasm(bytes, argv, source_root: String) Int` (extern
  `twinkle_runtime.run_wasm`; `proc.run_wasm` wrapper). Empty string means "no
  root" (location-only).
- **`run_file`.** Pass `artifacts.project_root` (absolutized — see safe-join) as
  `source_root`; existing non-`run` callers (`test.tw`, nested runs) pass `""`.
- **Host.** `runWasmBytes{Async}` / `prepareWasm` store the passed
  `source_root` on `runtime`; `makeChildTrapHandler` forwards it to
  `render_runtime_trace(childBytes, stack, message, source_root)`. Nested
  `run_wasm` launches forward `""` (they carry no root), preserving M1's
  location-only behavior for `proc.run_wasm(bytes)`.

### 4. Renderer (`boot/runtime_trace_renderer.tw`, `boot/lib/debug/*`)

- **Signature.** `render_runtime_trace(bytes: Vector<Byte>, stack: String,
  message: String, source_root: String) String`. Empty `source_root` ⇒
  location-only for every frame.
- **Per-frame resolution** (in `symbolicate.tw`/`trace.tw`): a frame's
  `file:line:col` and backtrace line always come from the embedded line/col +
  the stored (relative/logical) path — so the backtrace renders regardless of
  disk. The **snippet** is attempted only when: `source_root` is non-empty AND
  the path does not start with `@` AND `safe_join` yields a path under the root
  AND that file reads via `@std.fs`. Otherwise location-only.
- **`build_disk_registry`** (from M1) changes to: skip `@…` paths; for the
  rest, `safe_join(source_root, path)` and read the joined absolute path from
  disk. Unreadable/escaping ⇒ skip (that frame renders location-only).

### 5. Safe-join (`boot/lib/debug/*`, renderer-side)

```tw
// Return the absolute path to read, or .None to refuse (⇒ location-only).
fn safe_join(source_root: String, rel: String) String? {
  if source_root == "" or rel.starts_with("@") {
    return .None                      // no root, or logical path
  }
  joined := path.normalize(path.join(source_root, rel))
  root   := path.normalize(source_root)
  prefix := if root.ends_with("/") { root } else { "${root}/" }
  if joined == root or joined.starts_with(prefix) {
    .Some(joined)
  } else {
    .None                             // escaped the root (`..`, absolute rel, …)
  }
}
```

- `source_root` must be **absolute** for the containment check to be sound. It
  is absolutized before reaching the renderer: `run_file` resolves
  `project_root` against `proc.cwd()` (or the host absolutizes) and normalizes,
  so `twk run ./foo.tw` yields an absolute root.
- `path.normalize` collapses `..`, so a `rel` of `../../etc/passwd` normalizes
  to a path outside `prefix` and is refused. An absolute `rel` (shouldn't occur
  in a well-formed v3 section, but a hostile one could) fails the containment
  check and is refused.

### Degradation table (renderer)

| Condition | Result |
|---|---|
| `source_root` empty (artifact, nested run, non-`twk run`) | location-only, all frames |
| path starts with `@` (stdlib/prelude/extern) | location-only for that frame |
| `safe_join` refuses (escapes root / absolute rel) | location-only for that frame |
| joined file unreadable (moved/deleted) | location-only for that frame |
| joined file readable | snippet + caret from disk |

In every location-only case the frame still prints `file:line:col` from the
embedded line/col and appears in the backtrace; when the *primary* frame has no
snippet, the report omits the source block and appends `source unavailable`
(identical to M1). No path is ever read outside the safe-joined root, and a
malformed section can at worst force location-only.

## Testing

- **`find_project_root`** — `module_loader_suite`: no-manifest start returns the
  entry directory (not `.`); nearest-manifest behavior unchanged.
- **Producer classification** — a unit test over the shared classifier:
  in-root → relative, stdlib/prelude → `@std/…`, outside-root → `@extern/…`.
  A compiled-section check (as in M1) asserts stored paths are relative/logical,
  never absolute.
- **safe-join** — table tests: normal relative resolves under root; `..`-escape
  refused; absolute `rel` refused; `@…` refused; empty root refused.
- **Renderer** (boot suite): with a temp-dir root and a real file under it,
  present ⇒ snippet; file removed ⇒ location-only + `source unavailable`;
  empty `source_root` ⇒ location-only even though the file exists; a `@std/…`
  frame ⇒ location-only. Reuse the M1 stable-`/tmp` fixture approach.
- **CLI e2e** (`cli.test.mjs`): `twk run` on a project with a `twinkle.toml`
  shows a relative-path snippet; the same artifact run later without a root
  shows location-only; a `twk run` of a single file with no manifest resolves
  its snippet via the entry-dir fallback.
- **Self-host / size** — `make stage2` + `bundle-cli`; boot + JS suites green;
  confirm boot.wasm size is unchanged from M1 (paths are shorter, if anything)
  and no absolute build paths appear in its section.

## Non-goals (deferred)

- Content hashes / staleness detection (documented limitation only).
- `--strip-debug` (empty section for production).
- Cross-machine crash-report upload / an explicitly user-supplied root beyond
  `twk run`.
- Phase-4 rendering polish (prelude-frame suppression so `error()` traps pick
  the user call site as primary, name demangling, ANSI parity) — tracked in
  `runtime-stack-traces.md`, independent of this format change.

## Open questions for review

1. **Marker: `@`-prefix vs. explicit per-file kind flag.** The design uses the
   `@` prefix so the path string self-describes joinability and no flag byte is
   added. An explicit one-byte kind (0=relative, 1=logical) is unambiguous even
   for pathological `@`-named project dirs, at the cost of a per-file byte.
   Recommendation: `@` prefix.
2. **Absolutizing `source_root`:** do it in `run_file` (via `proc.cwd()`) so the
   value on the wire is already absolute, or in the JS host. Recommendation:
   `run_file`, so the one authoritative root is absolute everywhere downstream.
