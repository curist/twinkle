# Fold the JS↔Wasm-GC Bridge Into the Runtime

## Goal

Remove `bridge.wasm` as a separately-shipped asset by **embedding its bytes in
the JS runtime**, so every entry point (Node CLI, Deno CLI, web, embeddable lib)
gets the bridge without loading a separate file.

## Background

`boot/compiler/codegen/bridge.tw` is the Twinkle-owned source for a small,
fixed Wasm-GC helper module. `make stage2` regenerates it via
`boot/tests/gen_bridge_wasm.tw` → `tools/bridge.wasm`, which is then copied into
`tools/js_runtime/bridge.wasm` (playground) and bundled into the
`@twinkle-lang/twinkle` npm package.

The bridge exists because **JS cannot directly construct or read Wasm GC values**
(`String`, `Array`, `Variant`, `i31`). It exports helpers
(`string_new/get/len/set`, `array_*`, `variant_new`, `i31_*`, and four `bulk_*`
linear-memory fast paths) plus its own one-page "staging" memory.

Key facts that make this change safe and simple:

* The bridge is instantiated **standalone — it imports nothing**
  (`new WebAssembly.Instance(bridgeModule)`).
* The **main program module does not import from the bridge.** Only the JS
  runtime (`runtime.mjs`) calls bridge exports, to marshal host-ABI values
  (extern `String` args/returns, `Result` Ok/Err wrapping, argv/stdin arrays).
* Cross-module GC refs work via Wasm GC **structural type canonicalization** —
  the bridge's type definitions are structurally identical to the program's.

Because the program is never runnable without `runtime.mjs` anyway (it also
needs host imports and the marshalling glue), the bridge belongs **with the
runtime**, not as a standalone artifact and not inlined into each program.

## Non-goal

Inlining the bridge into each emitted program module. That bloats every artifact
and forces a second linear memory alongside the conditional `@std.buffer`
memory, for no real gain — the runtime is always present regardless.

## Approach

* Generate `tools/js_runtime/bridge_bytes.mjs` exporting the bridge as an
  embedded `Uint8Array` (base64-decoded once at module load), produced by the
  same `make stage2` step that currently writes `tools/bridge.wasm` — the same
  generated-asset pattern as `boot/lib/module/core_lib.tw`.
* `runtime.mjs` imports those bytes directly instead of receiving a
  `bridgeBytes` argument; `instantiateBridge` reads the embedded module.
* Simplify the entry points that currently source the bridge from disk/network:
  * `node_main.mjs` / `deno_main.mjs` drop `loadBridgeWasm()` and the
    `BRIDGE_WASM` env override / disk reads;
  * `web.mjs` drops the `fetch("./bridge.wasm")` asset load;
  * the embeddable `loadLib` (see
    [embeddable-lib-build.md](embeddable-lib-build.md)) needs only the program
    wasm.
* Keep `tools/bridge.wasm` as the generated intermediate (and dev/debug
  artifact); the *shipped* form is the embedded bytes. Decide whether to keep
  `tools/bridge.wasm` tracked or gitignored alongside `bridge_bytes.mjs`.

## API impact

`runtime.mjs`'s public helpers (`runWasmBytes`, `runWasmBytesAsync`,
`makeHostImports`, `run`, `command`) stop taking `bridgeBytes`. This is a
contained signature simplification; update the runtime tests and any callers.

## Testing

* `make npm-test` (`node --test tools/js_runtime/*.test.mjs`) stays green with
  the bridge sourced from the embedded bytes.
* The generated `bridge_bytes.mjs` matches a fresh
  `gen_bridge_wasm.tw` build (guard against a stale embed).
* Playground and `target/twk` still run a String-using program end-to-end.

## Affected components

| Component | Change |
|-----------|--------|
| `Makefile` (`stage2`) | also emit `tools/js_runtime/bridge_bytes.mjs` |
| `boot/tests/gen_bridge_wasm.tw` (or a sibling) | additionally write the embedded `.mjs` form |
| `tools/js_runtime/runtime.mjs` | import embedded bytes; drop `bridgeBytes` params |
| `tools/js_runtime/node_main.mjs` / `deno_main.mjs` | drop `loadBridgeWasm()` |
| `tools/js_runtime/web.mjs` | drop `bridge.wasm` fetch |
| `tools/build_npm_pkg.sh` / `build_deno_cli.sh` | stop staging a standalone `bridge.wasm` |
