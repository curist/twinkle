# Disk-Backed Debug Info — Milestone 2 (Portability / Hardening)

Status: Complete (landed on feat/runtime-stack-traces)
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
- **The root is absolute, and absolutization is caller-independent.** This is
  the load-bearing decision. `find_project_root` returns a relative root in the
  common cases — `"."` for `cd myproject && twk run src/main.tw` (manifest at
  the launch CWD), `"boot"` for `twk build boot/main.tw`. A relative root breaks
  `strip_root` (normalized canonicals like `src/main.tw` do not start with
  `"./"`, so every file misclassifies as `@extern` → location-only, *worse* than
  M1) and the renderer's containment check. So M2 absolutizes **at the shared
  producer layer** (`module_compiler`, where debug files are classified), not in
  one command: it computes an absolute `project_root` (join `proc.cwd()` if
  relative, then normalize), absolutizes the classification roots
  (`canonical_roots.stdlib_root`/`prelude_root`) and each canonical the same
  way, and classifies in that single absolute space. Because this lives in the
  compile path both `twk run` and `twk build` share, build artifacts get correct
  project-relative paths too — matching the Motivation. The `find_project_root`
  fallback fix (below) is still required so an out-of-CWD absolute entry
  (`twk run /tmp/foo.tw`) roots at `/tmp`, not the CWD.
- **`source_root` is threaded, not recomputed.** The compiler computes the
  absolute `project_root` once, at the producer layer above, and carries it on
  `PipelineArtifacts`; the `twk run` command hands that same absolute value to
  the renderer as `source_root`. `twk build` computes it identically for
  relativization but ships no `source_root` (artifacts render location-only).
  The JS host never re-derives path logic (avoids a second, drift-prone
  `find_project_root`).
- **No `source_root` ⇒ location-only for every frame.** A renderer holding
  relative paths but no root must **never** join them against the process CWD —
  that could surface an unrelated file that happens to share the relative path.
  Absence of a root is an unconditional location-only signal.
- **`find_project_root` no-manifest fallback returns the entry's own
  directory**, not `path.normalize(".")` (the launching CWD). This matters for a
  manifest-less run given by an *absolute or out-of-CWD* path
  (`twk run /tmp/foo.tw`): the fallback then roots at `/tmp`, and the producer's
  absolutization leaves that unchanged, so `foo.tw` strips to `foo.tw`. Without
  the fix it would return `"."`, absolutize to the CWD, and a `/tmp` file would
  not be under the CWD → `@extern`/location-only. For a manifest-less run given
  relative (`twk run ./foo.tw`, `dir == "."`) the fallback returns `"."` and the
  producer absolutizes it to the CWD, which correctly contains the file — so
  that case works via producer absolutization regardless. Rust stage0's
  `find_project_root` is left unchanged (only boot emits `twinkle.debug`, and
  boot always has a manifest). Behavior note: `pipeline.tw`/`base_env.tw` pass
  `"."` (unchanged by the fix); `test.tw` passes `proc.cwd()` and keeps doing
  so.
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
COMPILE (twk run / twk build — shared)        RENDER (twk run only)
─────────────────────────────                ─────────────────────────────
project_root = find_project_root(dir)         child traps → childTrapHandler(info, bytes)
producer layer (module_compiler):                     │
  abs_root  = abspath(project_root, cwd)      render_runtime_trace(bytes, stack,
  classify abspath(canonical) vs abs_root:                    message, source_root)
    under abs_root      → "src/grid.tw"               │
    stdlib / prelude    → "@std/…"  (logical) decode v3 twinkle.debug
    outside all roots   → "@extern/<base>"    parse stack → frames → symbolicate
  → store in twinkle.debug (v3)                       │
  → PipelineArtifacts.project_root = abs_root  per frame:
                                                source_root present AND path not "@…"?
run_file (twk run only):                        ├ yes → safe_join(source_root, path)
  source_root = artifacts.project_root (abs)          │    ├ under root & readable → snippet
  proc.run_wasm(bytes, argv, source_root)             │    └ escapes / unreadable → location-only
                                                └ no  → location-only
                                                            │
                                              location-only frames still print file:line:col
                                              (embedded line/col) + backtrace; primary snippet
                                              omitted + "source unavailable"; print once; exit 1
```

`source_root` flows: the boot `run` command holds the absolute
`project_root` (from `PipelineArtifacts`) and passes it as the new 3rd argument
of `proc.run_wasm(bytes, argv, source_root)`. That reaches the **`run_wasm` host
import** (the point where the child is executed), which decodes the string and
captures it in the child's `childTrapHandler` closure, which passes it to
`render_runtime_trace`'s 4th argument. Only top-level `twk run` sets it; nested
`proc.run_wasm` calls and pre-built artifacts pass `""` (location-only).

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

Only manifest-less runs change behavior, and only to return the (now absolute)
start dir instead of `"."`. This fix alone does **not** rescue a bare
`twk run foo.tw` — that still enters with `dir == "."` — which is why
`run_file` absolutizes the entry path *before* calling the compiler (Component
3); from an absolute entry, `dir` is absolute and the fallback yields the
absolute entry directory. Callers that pass `"."` (`pipeline.tw:46`,
`base_env.tw:315`) are unchanged; `test.tw:22` (which passes `proc.cwd()` with a
comment about `dirname(".")`) now gets an absolute CWD on the no-manifest path —
an improvement, and it keeps its own `proc.cwd()` call regardless.
`module_loader_suite` gains a no-manifest case asserting the entry-dir fallback.
(Rust stage0's `find_project_root` is left as-is: only the boot compiler emits
`twinkle.debug`, and boot always has a manifest, so self-host is unaffected.)

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
- **Absolutize once, at the producer layer.** `debug_files` are assembled in
  `module_compiler.tw:300-302`, exactly where `project_root` (`:58`) and
  `canonical_roots` (`:59`) already exist. Do the absolutization + classification
  **there**, caller-independently (so `twk run` and `twk build` behave the same):
  - `abspath(p)` = `if is_absolute(p) { normalize(p) } else { normalize(join(proc.cwd(), p)) }` (idempotent on already-absolute paths).
  - Compute `abs_root = abspath(project_root)`, `abs_stdlib = abspath(canonical_roots.stdlib_root)`, `abs_prelude = abspath(canonical_roots.prelude_root)`.
  - For each `DebugFile.path` (the canonical form `analyze.tw:421` produced, carried verbatim to `module_compiler.tw:301` — exactly what `strip_root` consumes), classify `abspath(canonical)` against those three absolute roots via the shared classifier, then format the `@`-marked/relative string.
  This is strictly smaller than threading roots down through `LinkedModule` →
  `emit_wasm_parts`; `wasm.tw`'s file-table emit and line/col precompute are
  unchanged from M1 (they serialize whatever `path` now holds). No absolute path
  reaches the section.
- **Carry the root.** Store `project_root: String` = `abs_root` on
  `PipelineArtifacts` so `run_file` reads the single absolute value for
  `source_root` (no re-derivation).

### 3. Root plumbing (`boot/commands/run.tw`, `proc`, host)

- **Extern + wrapper + ABI.** Widen `run_wasm` to carry the root:
  `run_wasm(bytes, argv, source_root: String) Int`. Touch points:
  - `boot/stdlib/proc.tw` — the real source of the `extern twinkle_runtime {
    fn run_wasm(bytes, argv) Int }` decl and the `proc.run_wasm` wrapper (add
    the `source_root` param to both). `boot/lib/module/core_lib.tw` embeds this
    as a generated string literal — regenerate it, don't hand-edit.
  - `boot/compiler/builtins.tw:371` — `"host_run_wasm" => abi([arr_n(),
    arr_n()], [.I64])` becomes `abi([arr_n(), arr_n(), str_n()], [.I64])`
    (String host args use `str_n()`, cf. `host_env`/`host_read_file`).
  - `boot/compiler/codegen/emit/runtime_abi.tw:21` needs **no** change — the new
    arg is a `String`, not a `Vector`, so it crosses no PVec→Array boundary.
  Empty string means "no root" (location-only).
- **`run_file` (`boot/commands/run.tw`).** Pass `artifacts.project_root`
  (already absolute — absolutized at the producer layer, Component 2) as
  `source_root`; no entry-path handling needed in the command. Other
  `proc.run_wasm` callers — `test.tw:100`, any nested run — pass `""`.
- **Host (`tools/js_runtime/runtime.mjs`).** `source_root` arrives as the new
  3rd wasm arg of the **`run_wasm` host import** — the sync binding (~`:500`)
  and the async `suspendHost` binding (~`:1687`, an arity change). Each
  `decodeString`s it and captures it in the child's trap handler closure —
  `makeChildTrapHandler(runtime, sourceRoot)` (~`:516`/`:1704`) →
  `renderChildTrace` → `render_runtime_trace(childBytes, stack, message,
  source_root)` (~`:1316`). It does **not** flow through `prepareWasm`/`runtime`
  opts of the outer boot instance. Nested `run_wasm` launches inside a guest
  pass `""`, preserving M1's location-only behavior for `proc.run_wasm(bytes)`.
- **Bootstrap caveat.** Because `run.tw` uses `proc.run_wasm`, the emitted
  `boot.wasm` import arity and the bundled `runtime.mjs` binding must change
  together in the same `make bundle-cli`. Stage0 needs no change (generic extern
  support; the JS host implements the import).

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

Shipped implementation note: this snippet's absolute-`rel` case relies on
`path.join`'s handling of an absolute second segment; this codebase's
`path.join` is Node-style (concatenates rather than letting an absolute
second segment override the first), so the shipped `safe_join`
(`boot/lib/debug/symbolicate.tw`) additionally guards `path.is_absolute(rel)`
up front — otherwise `join("/p", "/abs/x")` normalizes to `"/p/abs/x"`, which
would still pass the prefix check below.

- `source_root` must be **absolute** for the containment check to be sound. It
  is absolute by construction: the producer layer absolutizes `project_root`
  (Component 2) and stores it on `PipelineArtifacts`, and `run_file` forwards
  that value — so `source_root` is absolute even for `twk run ./foo.tw`. No
  re-absolutization is needed at the renderer.
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

**Cross-version note (M1 v2 artifact under a v3 renderer).** `decode` rejects a
mismatched version byte (returns `.None`), so a stale v2 artifact rendered by a
v3 renderer falls to the no-debug-section fallback — headline + "(no Twinkle
stack trace available)", losing even the embedded line/col. This is acceptable
because the branch is unreleased (no shipped v2 artifacts exist), and it is the
safe failure. It is called out only so the degradation surface is complete.

## Testing

- **`find_project_root`** — `module_loader_suite`: no-manifest start returns the
  entry directory (not `.`); nearest-manifest behavior unchanged.
- **Producer classification** — a unit test over the shared classifier:
  in-root → relative, stdlib/prelude → `@std/…`, outside-root → `@extern/…`.
  Include the **`project_root == "."` regression case** (B1): a project whose
  manifest is at the launch CWD must still classify its files as in-root
  relative (`src/main.tw`), not `@extern` — this is the case the absolutization
  fixes. A compiled-section check (as in M1) asserts stored paths are
  relative/logical, never absolute — run it over a **`twk build`** output too
  (relative entry, no `run_file`), asserting the entry appears as a
  project-relative path, not `@extern/…`, confirming the producer-layer
  absolutization is caller-independent.
- **safe-join** — table tests: normal relative resolves under root; `..`-escape
  refused; absolute `rel` refused; `@…` refused; empty root refused.
- **Renderer** (boot suite): with a temp-dir root and a real file under it,
  present ⇒ snippet; file removed ⇒ location-only + `source unavailable`;
  empty `source_root` ⇒ location-only even though the file exists; a `@std/…`
  frame ⇒ location-only. Reuse the M1 stable-`/tmp` fixture approach.
- **CLI e2e** (`cli.test.mjs`): `twk run src/main.tw` launched *from* a project
  root (so `project_root == "."` before absolutization) shows a relative-path
  snippet; the same artifact run later without a root shows location-only; a
  `twk run ./foo.tw` of a single relative-path file with no manifest resolves
  its snippet via entry-path absolutization + the entry-dir fallback.
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
   Recommendation: `@` prefix (decided, pending objection).

Resolved during design review: `source_root`/root absolutization happens in
`run_file` via `proc.cwd()` (one authoritative absolute root used by producer
classification and renderer safe-join), which is also the fix for the `"."`-root
regression and for relative single-file runs.
