# Compiler-emitted extern FFI metadata

**Status:** Plan, ready to implement
**Date:** 2026-06-07

## Goal

Stop hand-writing how each `extern` function's arguments marshal across the JS
boundary. The compiler already knows the types (`ExternImport.param_tys` /
`return_ty`); emit them into the wasm so the JS runtime auto-marshals. The
manual `imports: { canvas: { fill_rect: { fn, args: ['raw', ...] } } }` spec
disappears — the `extern` block becomes the single source of truth.

## Background

The JS runtime must know, per extern arg, whether a non-numeric value is a Wasm
GC **string** (decode via the bridge) or an opaque **externref** (pass through
untouched — decoding it as a string stack-overflows Safari). It can't tell them
apart at runtime (both are objects) and can't probe safely, so today the caller
hand-declares `args` (introduced as `marshalSpec`, folded into `imports` in
0.4.0). That array is really a description of the extern's FFI ABI — which the
compiler already has and the JS side is blind to.

`extern canvas { fn fill_rect(c: Canvas, x: Int, y: Int, w: Int, h: Int) }` —
the types are right there. We just need to ship them to the runtime.

## Design

- **Custom wasm section `twinkle.externs`** (JSON), emitted into every module the
  compiler produces. Lists each live extern import with per-arg + return kinds.
- **Runtime reads the section** (`WebAssembly.Module.customSections`) and
  auto-marshals from it. A manual `{ fn, args }` override still wins (escape
  hatch for non-Twinkle wasm); a missing section falls back to today's
  string-default behavior. Fully backward compatible.
- **Playground canvas externs collapse to plain functions** — no `args`.

### Section format

`twinkle.externs` content is UTF-8 JSON: an array of entries, one per live
extern import.

```json
[
  {"module":"canvas","name":"fill_rect","args":["ref","i64","i64","i64","i64"],"ret":"void"},
  {"module":"http","name":"fetch","args":["str"],"ret":"str"}
]
```

Kinds: `"str" | "i64" | "f64" | "i32" | "ref" | "void"`. Only `str` vs `ref` is
strictly load-bearing (numbers are `typeof`-detectable), but all kinds are
encoded so the runtime never guesses. `ret` is `"void"` when the extern returns
nothing. The section is tiny (only user-declared externs; `host.*` builtins are
not `extern` imports, so `boot.wasm` itself gets an empty/absent section).

### MonoType → kind

| MonoType | kind |
|---|---|
| `String` | `str` |
| `Int` | `i64` |
| `Float` | `f64` |
| `Bool`, `Byte` | `i32` |
| `ExternRef(_)`, `Optional(ExternRef(_))` | `ref` |
| (return) `Void` / `None` | `void` |

Any other MonoType in an extern signature is a compile error already (extern
signatures are restricted to extern-safe types), so the mapping is total over
what can actually appear.

## Data flow

`LinkedModule` (the backend IR consumed by `wasm.tw`) carries only lowered
`ImportDef`s (ValTypes), not the source `MonoType`s. Rather than reverse-map
ValTypes (a `Ref(_, Named(...))` is *probably* `String` but that's an unsafe
assumption), compute the kinds from the `MonoType`s while they're still in hand
and preserve that metadata through Wasm linking:

1. `emit_module` already has the source `anf.extern_imports`
   (`Dict<Int, ExternImport>`, each with `param_tys` / `return_ty`) while it
   builds the user module's lowered `ImportDef`s.
2. Map those imports to a `Vector<ExternMeta>` there and hang it off
   `WasmModule` next to `imports`.
3. `codegen/linker.tw` carries/merges the module metadata into `LinkedModule`.
4. `codegen/linker_dce.tw` filters `LinkedModule.extern_meta` together with the
   surviving extern `ImportDef`s, so the section describes only live imports.
5. `wasm.tw` JSON-encodes `linked.extern_meta` into the custom section.

This keeps the source types authoritative, adds no ValType guesswork, and still
makes the emitted section match post-Wasm-DCE imports.

## Implementation steps

### 1. Boot compiler — emit the section

- Add
  `pub type ExternMeta = .{ module: String, name: String, args: Vector<String>, ret: String }`
  in the codegen IR layer (for example `wasm_ir.tw`) and a
  `mono_type_kind(MonoType) String` helper near `emit_module` implementing the
  table above.
- Add `extern_meta: Vector<ExternMeta>` to both `WasmModule` and `LinkedModule`.
  Update all constructors in runtime modules, tests, and linker fixtures to use
  `[]` when they have no user externs.
- In `emit_module`, populate `user_module.extern_meta` from
  `anf.extern_imports` next to the existing `extern_imports: Vector<ImportDef>`
  construction, before source types are erased to `ValType`s.
- In `codegen/linker.tw`, merge module `extern_meta` into `LinkedModule` using
  the original import `module`/`name` values. Do not namespace or rewrite them:
  they must match `WebAssembly.Module.imports`.
- In `codegen/linker_dce.tw`, filter `linked.extern_meta` to the same set of
  surviving non-runtime extern imports as `linked.imports`, using
  `(module, name)` pairs.
- In `wasm.tw`, add `encode_extern_meta_section(meta) Vector<Byte>` that builds
  the custom-section payload — `emit_name(buf, "twinkle.externs")` followed by
  the UTF-8 JSON bytes — and append it via the existing
  `emit_section_into(buf, 0x00, payload)` in `emit_wasm_parts`, after the
  standard sections. Skip emitting when `meta` is empty.
- Build a small JSON string in Twinkle; no general JSON library is needed, but
  add a tiny `json_escape_string` helper for `module` and `name` rather than
  assuming extern import strings never contain JSON-special characters.

### 2. Runtime — consume the section (`tools/js_runtime/runtime.mjs`)

- In `prepareWasm`, after building `mainModule`, read
  `WebAssembly.Module.customSections(mainModule, "twinkle.externs")`; if present,
  `JSON.parse` the first one into a `module → name → { args, ret }` map.
- `resolveExternImports` / `autoBridgeExternImports`: when an extern has no
  explicit `args` (from the manual override), use the section's `args` for that
  import. Precedence: manual `{ args }` > section > default (string).
- Marshal args per kind: `str` → decode, `ref` → raw, `i64` → bigint→Number,
  `f64` / `i32` → passthrough. (The existing numeric `typeof` fast-path already
  covers the number kinds; `ref` is the new explicit case.)
- Marshal returns per `ret` instead of guessing only from JS `typeof`:
  `str` → encode JS string, `ref` → raw, `i64` → `BigInt(...)` when needed,
  `f64` / `i32` → passthrough, `void` → `undefined`.
- Ensure `host.run_wasm` continues to call `runWasmBytes` / `runWasmBytesAsync`
  on the child bytes normally. The child module parses its own
  `twinkle.externs` section during its own `prepareWasm`; do not reuse the
  parent's parsed metadata.

### 3. Playground worker (`playground/src/worker.js`)

- Drop `RAW`/`args` from `canvasImports`; each entry becomes a plain function:
  `fill_rect: (c, x, y, w, h) => c.fillRect(x, y, w, h)`, etc.
- `timer`/`http` are already plain functions — unchanged.

### 4. Tests

- Boot: compile a tiny program with an `extern` block taking a `ref` + a `str` +
  numbers; assert the `twinkle.externs` section is present and its entries match.
- Runtime: a unit test that section-driven marshaling routes a `ref` arg raw and
  a `str` arg through decode (can use a hand-built module or a fixture compiled
  by boot).
- Integration: the playground canvas example renders (Safari is the regression
  target for the `ref` path).

## Out of scope (deferred)

- **Signature checks** (arity / return-kind validation) — explicitly parked;
  the section enables them later but this change is metadata + marshaling only.
- **stage0 parity** — Rust stage0 need not emit the section: boot is the primary
  compiler, `boot/main.tw` declares no `extern` blocks that go through the
  auto-bridge path, and the runtime tolerates a missing section. Revisit only if
  a stage0-compiled program needs section-driven marshaling.

## Risks

- **Self-host:** changing `LinkedModule` + `wasm.tw` runs through the stage2
  self-host loop — the fixed-point check is the safety net (and proves the new
  section doesn't perturb boot's own output, which should stay section-free).
- **JSON in a custom section** is unconventional but valid; if size ever matters
  a compact binary encoding can replace it without changing the runtime contract
  (the runtime is the only consumer).
- **Override precedence** must be exact: a manual `args` has to win so existing
  callers and non-Twinkle wasm keep working.
