# Runtime Stack Traces (source-mapped trap reporting)

Status: Phases 0–2 complete (end-to-end for `error`/div0/OOB on `twk run`)
Date: 2026-09-05

> **Format superseded (2026-09-07):** the `twinkle.debug` section described below
> embeds source **text** inline (Component 1's file table, the "self-contained
> section" framing, and the Phase-1 "size follow-up" / Phase-4 `--strip-debug`
> notes). That inline-source format is being replaced by disk-backed recovery —
> the section stores only project-relative paths, precomputed line/col, and PC
> maps, and snippets are read from disk at render time on `twk run`. See
> [disk-backed-debug-info.md](disk-backed-debug-info.md). The capture/render
> architecture (Component 5, `childTrapHandler`, the renderer `--lib`) is
> unchanged; only the section payload and the source-recovery step change.

## Goal

When a Twinkle program traps at runtime, capture the failure and render a
beautiful, source-mapped stack trace pointing back at the original `.tw` code —
function names, `file:line:col` per frame, and a source snippet with a caret at
the offending expression. Today we hand traps off to the executing platform
(Deno/V8), which prints a raw wasm stack trace with indexed, mangled function
names and no mapping to Twinkle source.

Target output (the "shine" we are building toward):

```
runtime error: index out of bounds (len 3, index 5)

  src/grid.tw:42:11
   42 |   cells.at(idx)
      |         ^^

  at Vector.at   (src/grid.tw:42:11)
  at Grid.cell   (src/grid.tw:18:5)
  at main        (src/main.tw:7:3)
```

## Decisions (from brainstorming)

- **Error scope: all traps.** Explicit `error(...)`/assert *and* implicit traps
  (out-of-bounds, division by zero, `unreachable`, failed `ref.cast`, null
  externref at the host boundary, OOM, stack overflow).
- **Surface: `twk run` on Deno/V8 first.** The debug-info format is portable, so
  Node host and the browser playground reuse it later. Not in MVP scope.
- **Reporting only — traps stay fatal.** No `catch`/`recover`. Spec §6
  ("unrecoverable") is unchanged. We add a capture-and-render layer, nothing more.
- **Richness: frames + `file:line:col` + snippet & caret** (rustc/Gleam style).
- **Approach A: embedded debug section + host symbolication, reusing the boot
  renderer.** Rejected alternatives: (B) host-side in-memory table only —
  CLI-only, not portable/extensible; (C) shadow call stack — per-call overhead
  against the project's perf focus, and *weaker* for implicit traps (a div0 is a
  raw wasm trap with no shadow entry at the faulting op).
- **Scope: boot compiler + host only — no stage0 changes.** All debug-info
  emission (span threading, `SpanMark`, `twinkle.debug`, the source table) lives
  in the boot backend, which stage0 never runs. The prelude additions reuse only
  constructs stage0 already supports (ordinary functions + the **existing**
  `twinkle_runtime.error` import — no new extern), so stage0 compiles boot source
  unchanged. Double-printing is solved **host-side**, not in the language. This
  is a hard constraint on every component below.

## Guarantee

**For every trap that surfaces as a catchable throwable with usable wasm
frames, we produce a full Twinkle stack trace.** The trace derives from the
*host* stack (V8's `.stack`), which such traps unwind through — so it exists
regardless of trap *kind*. The panic helper (below) only enriches the
*message*; it is never what makes the trace exist.

| Trap path | Stack trace + carets | Message quality |
|---|---|---|
| `error("…")` | ✓ (host stack) | rich (user text) |
| OOB index read/write | ✓ | rich (len/index) via `rt.panic` |
| div0 / rem0 | ✓ | rich if guarded, else V8's "divide by zero" |
| bare `unreachable`, `ref.cast`, null-deref, host-boundary null | ✓ | generic-but-correct (V8's message) |

The panic helper's job is to push as many rows as practical from
"generic-but-correct" up to "rich". No row above can *lack* a trace.

**Best-effort boundary (not guaranteed).** Some failures cannot be relied upon
to deliver a usable trace, and the renderer degrades gracefully rather than
promising one:

- **OOM** may terminate the V8 isolate/process without a catchable throwable.
- **Stack overflow** surfaces as a `RangeError` whose `.stack` is frequently
  *truncated* by the engine — frames may be missing.
- **Non-`Error` throws** (JS permits `throw 42`) carry no `.stack` at all.

For these, the host prints whatever it has (the raw message and any partial
stack) with a note that the Twinkle trace is unavailable/partial. The renderer
treats "no frames" and "unparseable frame" as normal, expected inputs.

## Architecture

Two halves connected by a debug-info format embedded in the wasm module:

```
COMPILE (boot compiler)                      RUN (twk run, Deno/V8)
─────────────────────                        ──────────────────────
CoreExpr.span  ─┐                            proc.run_wasm host import
                │ thread through              instantiates child module
prepare/ANF  ───┤ lowering                    and calls __twinkle_start()
                │                                      │
emit (Instr) ───┤ SpanMark(span) events               │ child traps
                │                                      ▼
wasm.tw      ───┤ encode: SpanMark →          run_wasm boundary CATCHES
                │  (code_offset → span)        (not re-throw): has .stack,
   ┌────────────┴───────────┐                  .message, AND childBytes
   │ name section            │  ── embed ──►           │
   │ twinkle.debug:          │     in child     childTrapHandler(info, bytes):
   │  file table + source    │     module       render in a DEDICATED library
   │  function/PC ranges     │                  instance (NOT the suspended
   │  offset→span program    │                  boot CLI instance):
   └────────────────────────┘                  render_runtime_trace(
                                                  childBytes, stack, msg)
                                                        │
                                               decode twinkle.debug from
                                               childBytes → frames → Spans
                                               → render via render.tw
                                                        │
                                                        ▼
                                        return string; host prints to
                                        stderr, run_wasm returns nonzero
```

- The **producer's** only job is to preserve source locations that already exist
  (`CoreExpr.span`) down to wasm-emit time and serialize them (with source text)
  into the child module. This requires a **source-data flow** the pipeline does
  not have today, *and* a compilation-wide file-identity scheme it also lacks —
  see "Source-data flow" below.
- **Ownership protocol (resolves the unwind problem).** Today a child trap is
  re-thrown out of `runWasmBytes` and propagates *through* the `run_wasm` host
  import, unwinding the boot `run_file` frame — so any compile-time state
  (`artifacts`, a `FileRegistry`) is gone before an outer handler runs, and
  `PipelineArtifacts` carries no source registry anyway. Rendering therefore
  happens **at the `run_wasm` host boundary** via an opt-in
  **`childTrapHandler` callback** (see Component 5), which depends on **nothing**
  but its explicit arguments — no unwound locals, no ambient registry.
- **Render in a dedicated boot-compiled renderer module — no reentry.**
  `render_runtime_trace` is pure in `(module bytes, stack, message)`, so it does
  **not** need the boot CLI instance that is currently suspended lower on the
  stack. It is built as a small `--lib` artifact and bundled beside `boot.wasm`;
  `childTrapHandler` loads that artifact through the library bridge on first use
  and caches it. This avoids inventing an internal export ABI for the executable
  boot module. Phase 2 first extends the library ABI with a `Byte` leaf so
  `Vector<Byte>` is eligible, then adds a synchronous loader for non-task library
  modules; the async loader reuses the same core. An independent Wasm instance
  works while the boot CLI is suspended on both synchronous and JSPI `run_wasm`
  paths (confirmed by the Phase-0 nested-instance probe).
- **`render_runtime_trace` is a pure function of `(module bytes, stack string,
  message)`.** It decodes the embedded `twinkle.debug` (file table + source +
  offset→span), parses frames, and renders via `render.tw`. This is exactly what
  any external tool would do, so it *is* the single format-of-record — no
  separate in-memory table that could diverge.
- Symbolication, decoding, and rendering all live in boot code and are
  unit-testable in the boot suite from fixture `(bytes, stack, message)` inputs.

## Components

### 1. Debug-info format (two custom sections)

Both emitted via the existing `emit_section_into(0x00, …)` custom-section path in
`boot/compiler/codegen/wasm.tw` (already used for `twinkle.externs` /
`twinkle.exports`).

1. **`name` section** — standard wasm `name` section (id 0, name `"name"`),
   function-names subsection: `funcidx → mangled Twinkle name`. This alone makes
   V8 print a symbolic Twinkle name instead of only `at wasm-function[123]`,
   and standard tooling (`wasm-objdump`, browser devtools) understands it for
   free. This is the **Phase 0 spike** milestone.

2. **`twinkle.debug` section** — extensible payload, versioned with a leading
   version byte and length-prefixed subsections so future readers can add
   inline-frame or variable info without breaking older ones. The section is
   **self-contained** — a reader needs nothing but the module bytes:
   - **File table:** `file_id → { path, source_text }`. A `Span`'s `file_id` is
     meaningful only inside the compiler's ephemeral `FileRegistry`; the trace is
     rendered *after* that state is gone (and, eventually, by other hosts). So
     the section carries the file **path** and the **source text** itself, and
     the renderer reconstructs a `FileRegistry` (or an equivalent line index)
     from this table. `--strip-debug` drops the whole section; a future
     `--debug=paths-only` variant may store a path + content checksum instead of
     inline source (deferred; noted so the format leaves room — a per-file flag
     byte selects inline-source vs checksum-only).
   - **Function table:**
     `funcidx → { name_str, file_id, decl_span, body_start, body_end }`. The
     absolute body range selects the function when an engine's textual index is
     unreliable; the declaration span labels a frame when no fine-grained entry
     matches.
   - **Line program (per function):** a sorted, delta-encoded list of
     `(code_offset → Span)` entries — the same idea as a DWARF line program or a
     JS source-map `mappings` field. Symbolication binary-searches `code_offset`
     within a function to get the enclosing `Span`.

**Offset coordinate system (pinned in Phase 0).** V8 reports frame offsets in
the module's absolute *wire-code* coordinate space, at the trapping opcode for
the innermost frame and at the call opcode for callers, while the emitter builds
each function body in a **temporary buffer** (`encode_code_section_payload` in
`wasm.tw`) whose local origin is the first body byte — *after* the body's
size-LEB and local-declaration bytes, and offset again by the function's start
within the code section. A function-body-relative table therefore will **not**
line up with V8's numbers unless we translate. The format stores the origin it
uses explicitly (one documented choice — `code_offset` is **relative to the
start of the function body's instruction bytes**), and symbolication adds the
per-function body-start base (recorded alongside the function table as the body
is encoded) before comparing against V8's reported offset. **Phase 0 empirically
establishes what V8/Deno actually reports** (module-relative vs
code-section-relative vs body-relative) from a real trap, and the base
arithmetic is fixed to match that observation before Phase 1 relies on it.

The hexadecimal PC is authoritative for selecting the function as well as its
line entry: the Deno standalone executable was observed printing the correct
name but `wasm-function[0]` for every frame, while Node printed the real indices.
The parser retains the `wasm://wasm/<module-id>` token, skips leading JS/import
frames (notably the `twinkle_runtime.error` frame), then accepts the first
contiguous same-module Wasm group and stops at the following JS boundary or
different module token. Within that group, the decoder records each defined
function's absolute instruction range, locates a frame by PC range, and treats
the textual function index/name as optional corroboration. This prevents
overlapping PCs in suspended boot frames from being misread as child frames.
Imported functions have names but no code range.

`Span` is `{ file_id, start, end }` (byte offsets) — the existing type in
`boot/lib/source/span.tw`. `FileRegistry` (`boot/lib/source/registry.tw`)
already converts a `Span` → `LineCol{line, column}` and → source `snippet`; the
renderer feeds it the file table's source text so the same code path works.

Emit granularity, staged: start coarse — entries at **statement boundaries**,
every **call**, and every **trap-producing op** (index read/write, integer
div/rem). That is enough for accurate frames (call-site per frame; trap-site for
the innermost). The format supports finer granularity later without changing
consumers.

### 2. Span threading (Core → ANF → Prepared → emit → encode)

`CoreExpr` already carries `span` (`CoreExpr = .{ kind, ty, span }`). ANF
(`prepared_ir.tw`) does not, and — critically — neither does the *emitted*
`wasm_ir.Instr`. The real pipeline is `Prepared IR → Vector<Instr>` (the
`emit/*.tw` layer) `→ bytes` (`encode_instrs`/`encode_code_section_payload` in
`wasm.tw`). Since `Instr` carries no location, a cursor added only in `wasm.tw`
**cannot recover spans that were dropped at the emit step**. We close the gap in
two places:

- **Carry spans down to the emit layer** via an **inline `span` field** on the
  ANF and Prepared expr/op wrappers, mirroring `CoreExpr.span`, copied at each
  lowering step. Why inline rather than a side table keyed by node id:
  ANF/Prepared nodes have no stable node id (`SlotId`/`LocalId` identify
  values/locals, not expression nodes), and every side table in the backend is
  keyed by local/slot identity, not node identity. The backend rebuilds expr
  nodes constantly; an inline field rides along naturally (each rebuild
  copies/merges it, and `span.merge` already exists), whereas a node-id side
  table breaks the moment a pass rebuilds a node without carrying its id — and
  there are no ids to carry. Inline also matches the one existing span
  precedent (`CoreExpr`).

- **Preserve spans across `Instr` via a zero-byte `SpanMark(Span)` pseudo-
  instruction.** The `emit/*.tw` layer, which already has the Prepared node (and
  now its span), inserts `SpanMark(span)` into the `Vector<Instr>` before the
  instructions for a statement / call / trap-op. `encode_instrs` handles
  `SpanMark` by emitting **no bytes** but recording `(current_byte_offset →
  span)` into the function's line program, then continuing. This composes with
  the recursive `If`/`Block`/`Loop` nesting for free (it's just another entry in
  the instruction stream), needs no wrapper type or parallel positional
  structure, and puts the offset-capture exactly where bytes are produced —
  which also resolves the coordinate-space question, since the offset is read
  from the same buffer being encoded. Cost: one new `Instr` variant that the
  exhaustive `Instr` matches and the ref/DCE scan (`collect_ref_funcs_instr`)
  must handle as a no-op.

### 3. File identity + source-data flow (both missing today)

The file table in §1 needs each file's **path and source text**, keyed by the
`file_id` carried in emitted `Span`s. Two things do not exist today and must be
built **first**, before any offset→span mapping is meaningful:

**(a) Compilation-wide file identity.** Emitted spans do *not* carry unique file
ids: `analyze.tw` parses **every** module with a hardcoded `file_id = 0`
(`parse_runner.parse(state.cache, source.text, 0)`), and the `FileRegistry`
instances in `pipeline.tw` are throwaway helpers for formatting virtual-source
diagnostics — there is no compilation-wide registry behind the spans. So linked
expressions from different modules all share `file_id = 0` and cannot be told
apart. This is the **critical prerequisite**. Phase 0 selected
**analysis-time assignment**: `cache.Store` owns a stable
`canonical_path → file_id` map and the corresponding source records.
`parse_cached` obtains the id before calling `parse(...)`. Because parsed ASTs
and the id allocation live in the same persistent store, a cache hit cannot
resurface an id belonging to another file; a fresh store may assign different
numbers, which is harmless because each emitted debug section carries its own
matching file table. IDs may be sparse in a later compilation because the store
retains allocations for modules outside its current dependency closure.
`FileRegistry` therefore gains explicit-id insertion, and the debug section
contains only source records referenced by the current artifact. A Phase-1 test
asserts distinct modules get distinct ids, cache hits preserve them, sparse
registry reconstruction works, and a span round-trips to the correct source
line.

**(b) Source-data flow to codegen.** Even with ids fixed, codegen never sees
source: `codegen_wasm(anf, env, builtins)` and `PipelineArtifacts` carry none.
Build a serializable **`DebugSourceTable`** (`file_id → { path, source }`) from
the compilation-wide file table established in (a), add it to
`PipelineArtifacts`, and thread it into `codegen_wasm` / `link_program` /
`emit_linked_wasm` so the `twinkle.debug` serializer can write the file table.
Under `--strip-debug` the table is dropped and the section omitted. Because the
table is built from the *same* id assignment the spans use, ids match by
construction — but only once (a) is real, which is why (a) comes first.

### 4. Runtime panic helper (rich trap messages)

Introduce a prelude chokepoint — `rt.panic(msg)` — that is an **ordinary Twinkle
function** formatting a clean human message and calling the **existing**
`twinkle_runtime.error` import (which throws). No new extern, no structured
message prefix — so stage0 compiles it unchanged and non-rendering hosts print a
clean message with nothing leaked. Then:

- **OOB fail-arms call `rt.panic`** with a formatted message including length and
  index. This arm is cold (only taken on failure), so building the message there
  costs nothing on the hot path.
- **div0/rem0:** codegen for `/` and `%` on `Int` gets a guard that calls
  `rt.panic("divide by zero")` when the divisor is zero (turning the raw trap
  into a rich, source-located one). Cost model: the divisor-zero **compare runs
  on every division/remainder**, not only on failure — only the message
  construction and the `rt.panic` call are cold. So this is a real (if small)
  hot-path cost, unlike the OOB case whose check already exists. Plan-time
  detail: benchmark the guard against leaving the native wasm trap (which still
  gets a full trace, just V8's generic "divide by zero" message); guard only if
  the measured cost is acceptable, otherwise ship div0 as generic-message.
- **The trap *kind*** (for the headline) is inferred by the renderer from the
  message text and V8's `.message`, not carried as an in-band tag — keeping the
  message clean for the non-rendering path.

**Output authority (avoids double-printing) — solved host-side.** The existing
`twinkle_runtime.error` import writes the message to stderr *and then* throws. If
left unchanged, the boundary renderer would print the same message again. Rather
than change the language, the **host** owns this:

- When a child run has trace rendering active (i.e. `childTrapHandler` is
  installed — the `twk run` path), the host installs an `error` import variant that **does not
  pre-write to stderr**; it only throws. The boundary handler is then the **sole
  printer** and renders the report (message included) exactly once. The host
  knows whether `childTrapHandler` is active because it constructs the import
  object.
- When rendering is not active (embeddable/web, `--strip-debug`, stage0-built
  modules), the `error` import keeps today's write-then-throw behavior —
  single-print, no boundary rendering.
- **Compiler-error behavior is unchanged**: boot's own compile diagnostics go
  through `eprintln` + `proc.exit`, not this trap path.

Scope guard: we only touch trap sites the language *already* defines as trapping
(spec §6). No new trap sites, just richer messages at existing ones.

### 5. Host capture + rendering (at the `run_wasm` boundary)

The catch lives where the child module runs. `runWasmBytes` /
`runWasmBytesAsync` in `tools/js_runtime/runtime.mjs` currently catch `HostExit`
(→ exit code) and **re-throw everything else**, and they also back the
embeddable and web APIs — whose callers rely on runtime failures being thrown.
So the catch is **opt-in**, never a global behavior change:

- `runWasmBytes{Async}` gain an optional internal
  **`childTrapHandler(trapInfo, childBytes)`**
  option, where `childBytes` is the **JS `Uint8Array`** the runner already holds
  (the wasm-GC `bytesRef` lives only in the `run_wasm` import closure and is
  decoded before the generic runner is called — the generic API must not pretend
  to receive a GC ref). On a child trap: if `childTrapHandler` is provided, call
  it and return its exit code; **otherwise re-throw exactly as today**
  (embeddable/web unaffected).
- The option is consumed only by the `twinkle_runtime.run_wasm` import when it
  launches a child; it is not the outer boot runner's own catch handler. Thus a
  compiler trap still propagates normally. The CLI host installs the child
  handler, while ordinary/embedded invocations supply nothing and are untouched.
- `childTrapHandler` renders in the dedicated renderer module (lazily
  instantiated/cached), prints the returned string to stderr once, and yields a
  nonzero exit code. The synchronous path uses the new synchronous non-task
  library loader; the JSPI path may use its async wrapper.
  `run_file` then proceeds normally with `artifacts` intact — it just observes a
  nonzero result. No reentry into the suspended instance; no ABI change to
  `proc.run_wasm` (it still returns `Int`).

**Marshaling protocol (wasm-GC boundary).** The Twinkle export
`render_runtime_trace(bytes: Vector<Byte>, stack: String, message: String)
String` receives/returns **wasm-GC references** in the *renderer* instance. Since
that is a different instance from the one that ran the child, the host re-encodes
into the renderer via *its* bridge: `makeByteArray(rendererBridge, childBytes)`
for the module bytes, `encodeString` for `stack` and `message`, invoke the
export, then `decodeString` the returned reference. (There is no cross-instance
GC-ref passing.) This plumbing and its round-trip tests live in Phase 2.

`render_runtime_trace` — pure in its arguments:
1. Parses V8's stack string into frames carrying a module token and absolute
   `code_offset` plus optional textual function index and name. It skips leading
   JS/import frames, then retains only the first contiguous same-module Wasm
   group. The parser is
   V8-shaped (handles `at <name> (wasm://…:wasm-function[<idx>]:0x<off>)` and
   bare `at wasm-function[<idx>]:0x<off>`); other engines get their own parser
   later. Encapsulated so the engine-specific bit is isolated. Missing/truncated/
   unparseable stacks yield zero or partial frames — handled, not fatal.
2. Decodes `twinkle.debug` from the module bytes, translates each frame's
   reported offset into the format's coordinate system (subtract the per-function
   body-start base per §1), maps it to a `Span` (binary search in the line
   program; fall back to `decl_span`), and reads path + source text from the file
   table.
3. Reconstructs a `FileRegistry` from the file table and builds a `Report` /
   `SpanLabel` set, rendering via the existing `boot/lib/source/render.tw`
   `render(report, reg, config)`.

The renderer already produces the exact target format (severity headline,
`--> file:line:col`, gutter, source line, `^^^` caret, ANSI). We add at most a
thin trace-shaped entry point over it. When frames are absent (best-effort
boundary), it falls back to printing the raw message + partial stack with an
"unavailable/partial" note.

## Build modes

- **`twk run`:** debug info always emitted (dev workflow).
- **`twk build`:** emit debug info by default; add `--strip-debug` to omit the
  `name` and `twinkle.debug` sections for production artifacts.

## Known limitations

- **Tail-call frame collapse.** The compiler emits wasm tail calls, which
  *replace* the caller frame. Tail-called / tail-recursive frames will not appear
  as separate lines in the host stack. Accepted for MVP. Future extensibility:
  an inline-frame subsection plus an optional `--debug` build that disables tail
  calls for complete traces (out of MVP scope).
- **JSPI async fibers** (Task concurrency) may split or truncate the host stack;
  handled when the concurrency surface adopts traces (out of MVP scope). (Note:
  rendering itself avoids JSPI hazards by running in a dedicated library instance,
  not by reentering the suspended one — see Component 5.)
- **Engine-specific stack format.** MVP parses V8/Deno. Node reuses it (also V8);
  browser/Safari/Firefox parsers are follow-ups.

## Plan

### Phase 0 — Spikes that gate the architecture
- Emit the standard wasm `name` section (funcidx → mangled name). Verify V8/Deno
  prints Twinkle function names in the raw stack, and that the custom-section
  round-trips through `wasm-objdump`.
- **Pin the offset coordinate system empirically.** Force a trap in a known
  function, capture V8's reported `0x<offset>`, and determine whether it is
  module-relative, code-section-relative, or function-body-relative. Record the
  exact base arithmetic needed to translate our recorded offsets into V8's space.
- **Validate the dedicated-renderer path.** Confirm the CLI can instantiate a
  second boot instance and call an export on it while a first child run is in
  flight, on both the synchronous and JSPI-suspending `run_wasm` paths. (Expected
  trivial — independent instances — but proves the boundary wiring.)
- **Decide the file-identity scheme** (Component 3a): analysis-time assignment vs
  link-time remap, including the caching answer. This unblocks Phase 1.
- All four gate Phase 1–2 and must be settled here.

**Phase-0 outcome (2026-09-05):** all gates passed. The standard `name` section
is now emitted and is visible through both `WebAssembly.Module.customSections`
and `wasm-objdump`; Deno and Node show the emitted symbolic names. V8 PCs are
absolute module offsets and identify the exact trap/call opcode. Because the
Deno standalone wrapper reports an unusable textual function index, PC-range
lookup is authoritative. Nested-instance probes succeeded while an outer boot
CLI was suspended in both synchronous and JSPI child execution. The renderer is
a separate boot-compiled library artifact (requiring `Byte` library-ABI support
and a synchronous non-task loader), `childTrapHandler` is scoped to the
`run_wasm` import, and file identity uses a canonical-path allocation owned by
`cache.Store` with explicit sparse-id registry reconstruction.

### Phase 1 — File identity, span threading, source-data flow, `twinkle.debug`

**Phase 1 COMPLETE (2026-09-06, branch `feat/runtime-stack-traces`).** The whole
producer side — file identity, span threading, `SpanMark`, the line program, the
source table, `twinkle.debug` serialization, and the `boot/lib/debug/` codec — is
implemented, unit-tested, and self-host-verified (`make stage2` reaches its fixed
point after every step; full boot suite + JS section tests green). Six commits;
the emitted section decodes to a function/line program whose spans map to the
exact source ranges (`x + 1`, `y * 2` verified byte-for-byte).

**Known follow-up (size):** the boot compiler references nearly every module, so
its own `twinkle.debug` section is large (~5 MB; boot.wasm roughly doubles). The
file table is already filtered to referenced files (a trivial program embeds two
files), but the compiler is inherently the worst case. Pull **Phase 4
`--strip-debug`** forward before any `bundle-cli`/production/self-host artifact
ships, and have the Makefile's boot builds strip. Line-program-only (paths, no
inline source) is the lighter middle option the format's per-file flag byte
already leaves room for.

- **[DONE] File identity** (Component 3a): `cache.Store` owns a stable
  `canonical_path → file_id` allocator (`file_id_for`); `analyze.parse_cached`
  uses it instead of the hardcoded `parse(..., 0)`. `registry.add_file_with_id`
  supports sparse explicit-id reconstruction. Exposed a latent coupling: the
  diagnostic-render registries in `commands/common.tw` and `pipeline.tw` keyed
  source at id 0, so nonzero file_ids broke source snippets — fixed by keying via
  `report.primary_file_id`. Tested by `file_identity_suite` + an end-to-end
  multi-module trap fixture.
- **[DONE] Span threading — mechanism changed from the description above.** The
  ANF/Prepared IRs are **bare sum types**, not `.{ kind, ty }` wrappers, so there
  is no op-wrapper to hang an inline `span` on. Chosen approach (still inline, not
  a side table): **span on the `Let` binding node** — `AnfExpr.Let` and
  `PreparedExpr.Let` gained a `Span` field (`Let(id, op, span, body)`). Every
  `AnfOp`/`PreparedOp` is the RHS of a `Let`, so this names each op inline at
  exactly statement/call/trap-op granularity, at ~⅕ the churn of wrapping the op
  enum (~1255 sites). `lower_anf` originates real spans from `CoreExpr.span`
  (via `LetBinding.span`/`push_accum`); `slot_assign` carries the span across the
  ANF→Prepared boundary (`LowerSpineEntry.span` + `FinishLet(slot, span)`);
  every rewrite pass carries `sp` through, synthetic/compiler-generated lets use
  `span.unknown()` (sentinel `file_id = -1`).
- **[DONE] `SpanMark(Span)` `Instr` variant** (`wasm_ir.tw`), emitted before each
  let-bound op in `emit_let`/`emit_tail_let` (skipping `span.unknown()`). No-op in
  the byte encoder, the WAT emitter, and `collect_ref_funcs_instr` (its `_`
  default). Proven byte-identical output before consumption existed.
- **[DONE] Line-program capture** in `encode_code_section_payload`: each SpanMark
  records `(body-relative offset → span)` into the function's line program;
  `emit_wasm_parts` translates each function's payload-relative body start into an
  absolute module offset (V8's module-absolute PC space). *(Deviation: store only
  `body_start`, not `body_end` — the lookup selects the function by greatest
  `body_start ≤ pc`, which is exact for the contiguous code section; wasm PCs are
  always inside a body.)*
- **[DONE] Source-data flow** (Component 3b): `cache.Store` retains a source
  record per file_id (recorded in `parse_cached`); `module_compiler` builds
  `PipelineArtifacts.debug_files`, threaded through
  `codegen_wasm`/`link_program`/`LinkedModule`/`emit_wasm_parts`.
- **[DONE] `twinkle.debug` serialization** (versioned; file table with path +
  inline source; function table; per-function delta-encoded line program), filtered
  to referenced files.
- **[DONE] `boot/lib/debug/section.tw`**: encode/decode + `span_at`/`lookup`
  (function-by-body_start + line binary search). Unit-tested round-trip, boundary
  cases, PC lookup, version rejection (`debug_section_suite`); JS section-shape
  test in `externs.test.mjs`.

### Phase 2 — Boundary capture + rendering (end-to-end for `error()`)

**Phase 2 COMPLETE (branch `feat/runtime-stack-traces`).** `twk run` renders a
source-mapped trace at the `run_wasm` boundary, verified through the shipped
`target/twk` for `error()`, div0, and OOB (each printed once, nonzero exit;
normal programs and the embeddable/web re-throw path unchanged). The pure
rendering pipeline (`decode_module → parse_v8_stack → symbolicate →
render_trace`, exposed as `render_runtime_trace`) lives in the boot suite; the
host wiring (opt-in `childTrapHandler`, the sync non-task lib loader, the
renderer bundled beside `boot.wasm`, and the pre-print-suppressing `error`
variant) lands in the JS runtime + CLI mains + build. A new `Byte` library-ABI
leaf makes the `Vector<Byte>` module-bytes param eligible and marshals it in
bulk through linear memory. Remaining polish (rich OOB/div0 messages,
prelude-frame suppression, name demangling, `--strip-debug`) is Phase 3–4.

- Add the optional internal `childTrapHandler(trapInfo, childBytes)` option
  (`childBytes` = the runner's `Uint8Array`) to `runWasmBytes{Async}`: invoke it
  on a child trap when provided, else re-throw as today (preserving
  embeddable/web behavior). Consume
  it only inside the `run_wasm` import so traps in the outer compiler are not
  captured as child-program failures.
- Render in the dedicated boot-compiled `--lib` module, bundled beside
  `boot.wasm`; no reentry and no `proc.run_wasm` ABI change. Extend `LibType`,
  export wrappers, metadata, and JS conversion with the `Byte` leaf needed by
  `Vector<Byte>`. Factor the existing library loader into a synchronous core for
  non-task modules plus its current async entry point, so the sync trap path can
  instantiate the renderer lazily.
- Implement the **marshaling protocol**: `makeByteArray(rendererBridge,
  childBytes)`, bridge-encode `stack`/`message`, call `render_runtime_trace`,
  bridge-decode the result; print once; return nonzero exit code. Round-trip
  tests for the marshaling.
- `render_runtime_trace`: V8 stack-string parser + offset translation +
  frame→Span mapping + `FileRegistry` reconstruction from the file table + reuse
  of `render.tw`. Boot unit tests over fixture `(bytes, stack, message)` inputs
  (golden output), including empty/truncated/unparseable stacks.
- Install the **host-side `error` variant** that skips the pre-print when
  `childTrapHandler` is active, so the boundary handler is the sole printer
  (keep today's write-then-throw when rendering is inactive). Verify no
  double-print.
- End-to-end: an `error("…")` fixture run via `twk run` produces the target
  trace exactly once; nonzero exit code.

### Phase 3 — Rich messages via `rt.panic`
- Add the `rt.panic(msg)` prelude helper (an ordinary function calling the
  existing `error` import — no new extern).
- Route OOB fail-arms through `rt.panic` with len/index messages.
- Add the div0/rem0 guard (benchmark first). The renderer infers the headline
  kind from the message text.

### Phase 4 — Polish
- Trap-kind headlines and any kind-specific hints; snippet+caret for the
  innermost frame; ANSI parity with compile diagnostics.
- `twk build --strip-debug`.
- Docs: update `docs/spec.md` §6 (traps now report a source-mapped trace),
  `docs/internals/host-abi.md` (`run_wasm`-boundary trap capture +
  `render_runtime_trace` export),
  and `docs/API.md` if `rt.panic` surfaces.

### Out of MVP scope (future)
Inline-frame recovery for tail-call collapse; Node + browser symbolication;
local-variable names; DWARF export.

## File touch map

- `boot/compiler/core_ir.tw` — source of `span` (no change; already present).
- `boot/compiler/backend/prepared_ir.tw` + ANF nodes — add inline `span`;
  `prepare`/`slot_assign` copy it.
- `boot/compiler/codegen/wasm_ir.tw` — add `SpanMark(Span)` `Instr` variant.
- `boot/compiler/codegen/emit/*.tw` — insert `SpanMark` before
  statements/calls/trap-ops; no-op it in exhaustive `Instr` matches.
- `boot/compiler/codegen/wasm.tw` — `encode_instrs` consumes `SpanMark` (zero
  bytes) into the line program; record per-function body-start base;
  `collect_ref_funcs_instr` no-ops it; serialize `name` + `twinkle.debug` (file
  table + source, function table, line program).
- `boot/compiler/query/cache.tw` + `analyze.tw` — establish compilation-wide
  file identity: allocate stable ids by canonical path in the persistent store
  and replace the hardcoded `parse(..., 0)`.
- `boot/compiler/artifacts.tw` — add `DebugSourceTable` to `PipelineArtifacts`.
- `boot/compiler/pipeline.tw` — build the `DebugSourceTable` from the
  compilation-wide file table (ids matching emitted spans).
- `boot/compiler/codegen/codegen.tw` — `codegen_wasm`/`link_program`/
  `emit_linked_wasm` take the `DebugSourceTable` and hand it to the serializer.
- `boot/lib/debug/` (new) — debug-section encode/decode, symbolication, V8
  stack-string parser, offset translation, and trace model.
- `boot/runtime_trace_renderer.tw` (new) — small `--lib` entry exporting
  `render_runtime_trace`; bundled as the dedicated renderer artifact.
- `boot/lib/source/render.tw` — thin trace-rendering entry over `render()`;
  `registry.tw` gains explicit-id insertion for reconstruction from a sparse
  embedded file table.
- `boot/prelude/…` — `rt.panic` helper (ordinary function over the existing
  `error` import); route OOB fail-arms + div0 codegen through it. No new extern.
- `boot/commands/…` — `twk build --strip-debug`.
- `tools/js_runtime/runtime.mjs` (+ node/deno mains) — add the opt-in internal
  `childTrapHandler(trapInfo, childBytes)` option to the child launch inside
  `runWasmBytes`/`runWasmBytesAsync` (else re-throw as today); install it in the
  CLI host for sync + JSPI, closing over the lazily-instantiated renderer lib;
  add the synchronous non-task library loader and `Byte` marshaling; preserve the
  stack's module token and first child Wasm frame group; the marshaling round-trip
  (`makeByteArray` + `encode`/`decodeString` on the renderer bridge); the `error`
  variant that skips the pre-print when `childTrapHandler` is active; print once;
  return nonzero exit code.

## Verification

- `make boot-test` — new boot suites for debug-section encode/decode, offset→span
  lookup, stack-string parsing, and trace rendering (golden output).
- Integration fixtures that trap (OOB, div0, `error`) under `twk run`, asserting
  the rendered trace contains the expected `file:line:col` and caret.
- `make stage2` / `make bundle-cli` stay green — the boot compiler now emits the
  debug sections for *itself*. This is a **boot-compiler + host feature with no
  stage0 changes** (see Decisions): the debug backend (`SpanMark`, serializer,
  source table) is boot-only and stage0 never runs it; the prelude additions
  (`rt.panic`) are ordinary functions over the **existing** `error` import, which
  stage0 already compiles; the double-print fix and child trap handler live in the host.
  Still verify the bootstrap end-to-end per the stage0-bootstrap-dependency rule,
  but no stage0 source edits are expected or required.
