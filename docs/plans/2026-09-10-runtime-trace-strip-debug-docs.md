# Runtime trace Phase 4.5 (`--strip-debug`) + 4.6 (docs)

Status: design approved, ready for implementation plan.

The last two phases of the runtime source-mapped stack-trace feature (roadmap:
`docs/plans/runtime-stack-traces.md`). Everything else (capture/render, disk-backed
debug info M1+M2, Phase 4.1 user-frame-primary, Phase 3 rich OOB, Phase 4.2
demangling, Phase 4.4 ANSI color) is already on `main`.

## Goals

- **Phase 4.5 — `twk build --strip-debug`.** A build flag that omits the standard
  `name` custom section and the `twinkle.debug` custom section from the emitted
  `.wasm`, for smaller production artifacts. Default off; current behavior
  unchanged. A stripped artifact traps natively with no Twinkle trace — the
  renderer's existing "no debug section" fallback renders the headline plus
  "(no Twinkle stack trace available)".
- **Phase 4.6 — docs.** Document the now-shipped trap-trace behavior and fix a
  stale host-ABI signature.

Out of scope / decided: the boot/bundle build (`make bundle-cli`) does **not**
adopt `--strip-debug` — `boot.wasm` keeps its `twinkle.debug` section (~6.6MB
post-M1) so the self-hosted compiler's own traps stay source-mapped. `rt.panic`
stays internal (not documented as public).

## Background (verified in-tree)

The wasm bytes are produced at write time: `write_wasm(artifacts, out)` →
`pipeline.emit_wasm(artifacts)` (`Vector<Byte>`) or `pipeline.emit_wasm_buffer(artifacts)`
(`Buffer`/linear-memory — the **default** path; the `Vector` path is the
`TWINKLE_WRITE_BYTES_FALLBACK` env branch). Both descend through
`codegen_wasm(_buffer)` → `emit_linked_wasm(_buffer)` →
`emit_linked_wasm_buf(linked)`, where the two custom sections are emitted:

- the `name` section at `boot/compiler/codegen/wasm.tw:1731-1733`
  (`if imports.len() > 0 or funcs.len() > 0 { emit_section_into(0x00, encode_name_section_payload(...)) }`);
- the `twinkle.debug` section at `wasm.tw:1740-1787`
  (`if module_debug.len() > 0 { ... emit_section_into(0x00, dbg_payload) }`).

`PipelineArtifacts` (`boot/compiler/artifacts.tw:25`) is constructed at exactly
one site (`boot/compiler/module_compiler.tw:316`). `host-abi.md:165` is **stale**:
it lists `run_wasm` as `(ref null $Array, ref null $Array) → (i64)` (2 args), but
M2 widened it to 3 (`bytes, argv, source_root`).

## Part A — `--strip-debug` (Phase 4.5)

Emission-time gate, with the flag carried on `PipelineArtifacts`. Because the
default output path uses the `Buffer` encoder, a post-emit byte-strip is
awkward; gating at the single `emit_linked_wasm_buf` choke point covers both
encoders uniformly.

1. **`boot/compiler/artifacts.tw`** — add field `strip_debug: Bool` to
   `PipelineArtifacts`.
2. **`boot/compiler/module_compiler.tw:316`** — set `strip_debug: false` in the
   one artifacts constructor (the compile stage doesn't know the build flag).
3. **`boot/commands/build.tw`** — read `strip := parsed.has_flag("strip-debug")`
   once in `run_build_command`/`run_build_lib_project`, and rebind
   `artifacts.strip_debug = strip` after each `pipeline.compile_entry_path*`
   call. Setting it on the artifacts means every emit path — single-file and the
   `--node`/`--web`/`--lib` copies, both encoders — honors it with no per-site
   change.
4. **Thread the bool internally** (public `emit_wasm(artifacts)` /
   `emit_wasm_buffer(artifacts)` signatures stay unchanged, so `run.tw:57`,
   `test.tw:99`, and `boot/tests/gen_bridge_wasm.tw` are untouched):
   - `pipeline.emit_wasm`/`emit_wasm_buffer` pass `artifacts.strip_debug` into
     `codegen_wasm`/`codegen_wasm_buffer` (new trailing `strip_debug: Bool` param).
   - `codegen_wasm`/`codegen_wasm_buffer` (`codegen.tw`) pass it to
     `emit_linked_wasm`/`emit_linked_wasm_buffer`.
   - `emit_linked_wasm`/`emit_linked_wasm_buffer` (`wasm.tw`) pass it to
     `emit_linked_wasm_buf`.
   - `emit_linked_wasm_buf(linked, strip_debug)` gates both section blocks on
     `!strip_debug`.
5. **`boot/main.tw`** — register `.add_flag("strip-debug", "Omit the name and twinkle.debug custom sections from the .wasm output")` on the build command.

Scope note: the flag gates the binary custom sections, so it applies to `.wasm`
output; `.wat` output (`emit_wat` → `codegen`) does not emit these binary
sections and is unaffected.

### Testing (Part A)

- **`boot/tests/suites/`** (a codegen/build suite, e.g. a new
  `strip_debug_suite.tw` or the existing codegen-integration suite): compile a
  tiny source to bytes twice via the pipeline — once default, once with
  `strip_debug` set — and assert:
  - default bytes: `section.decode_module(bytes)` is `.Some(_)` **and** a `name`
    section is present;
  - stripped bytes: `section.decode_module(bytes)` is `.None` **and** no `name`
    section is present, while the module still decodes as valid wasm (magic +
    other sections intact).
  Drive this at the `emit_wasm` (`Vector<Byte>`) level so the test can scan
  bytes directly: obtain a `PipelineArtifacts` from a small source and emit it
  twice — once with `strip_debug` left `false`, once rebound to `true`. Use a
  helper to scan for a custom section by name (id 0, name bytes), mirroring
  `section.decode_module`'s framing walk.
- **`tools/js_runtime/cli.test.mjs`**: `twk build --strip-debug -o out.wasm` on a
  trapping program, then `twk run out.wasm` (pre-built artifact path) traps with
  exit 1 and stderr containing "no Twinkle stack trace available" (the no-debug
  fallback), i.e. no `file:line:col` snippet. Pass `NO_COLOR: "1"` to keep output
  deterministic (consistent with the other trace CLI tests).

## Part B — docs (Phase 4.6)

- **`docs/spec.md` §6 "Traps (unrecoverable)"** (around line 244): after the
  existing sentence, add that on `twk run` an unrecoverable trap now prints a
  source-mapped trace — severity headline, `file:line:col`, a source snippet with
  a caret at the failing expression, and a backtrace of readable frame names —
  colored like compile diagnostics (honoring `NO_COLOR`). Note that
  `twk build --strip-debug` omits the debug sections, so a stripped artifact
  reports the trap message without a source-mapped trace.
- **`docs/internals/host-abi.md`**: correct the `run_wasm` row (`:165`) to the
  3-arg form `(bytes, argv, source_root)` with a one-line description of
  `source_root` (the absolute dir project-relative debug paths are safe-joined
  against for snippet reads). Add a short subsection documenting the
  trap-rendering boundary: on a child-module trap, the host captures the trap
  message + V8 stack and calls the renderer lib's `render_runtime_trace(bytes,
  stack, message, source_root)` export (loaded beside `boot.wasm`) to produce the
  source-mapped trace printed to stderr; without a renderer it re-throws
  unchanged (embeddable/web unaffected). Mention `--strip-debug` disables useful
  output here.
- **`docs/API.md`**: at the indexing / `error()` surface, note that an
  out-of-bounds access, division by zero, or `error(msg)` traps with a
  source-mapped trace under `twk run`. Do **not** document `rt.panic` (internal).

### Testing (Part B)

Docs-only; no automated test. Verify by reading and by confirming the
`run_wasm` signature and `render_runtime_trace` export names match the source
(`boot/runtime_trace_renderer.tw`, `tools/js_runtime/runtime.mjs`,
`docs/plans/runtime-stack-traces.md`).

## Verification (sequential, foreground; never full `cargo test`)

1. After editing any `.tw`: `target/twk fmt <file>` then `target/twk lint boot/main.tw`
   (~4 pre-existing findings in section.tw/builtins.tw/census.tw are unrelated).
2. `target/twk run boot/tests/main.tw` (Part A codegen test; picked up without a
   rebuild).
3. `make bundle-cli` (needed so the real `target/twk` learns the new
   `--strip-debug` flag for the CLI e2e test), then
   `cp target/boot.wasm tools/js_runtime/boot.wasm` and `deno test -A tools/js_runtime/`
   (a pre-existing `web.test.mjs` JSPI env-quirk failure is unrelated).
4. Manual: `target/twk build /tmp/trap.tw --strip-debug -o /tmp/stripped.wasm`
   then `target/twk run /tmp/stripped.wasm` → native trap + "no Twinkle stack
   trace available"; and without `--strip-debug` → full source-mapped trace.
5. Roadmap: mark Phase 4.5 + 4.6 complete in `docs/plans/runtime-stack-traces.md`
   (the feature is then fully done) and archive this doc + the roadmap per the
   plans-README convention once merged.

## Workflow

One feature branch (`runtime-trace-strip-debug`). Subagent-driven execution with
a task review after each part and a whole-branch review before finishing.
Commits: imperative subject, what/why/how, no line/count metrics, with the
`Co-Authored-By` / `Claude-Session` trailers.
