# Full Lib-Export ABI

## Goal

Extend the embeddable `lib` build ([embeddable-lib-build.md](embeddable-lib-build.md))
from its primitives-only v1 to a **full host-callable ABI**: let a `pub` member
cross the JS↔guest boundary even when it takes or returns a `String`, a
`Vector`, a `Dict`, a record, or a function (callback). The host should be able
to drive a built Twinkle program library-style for realistic APIs — "compute and
return text", "hand me a list", "call this back per row" — not just integer math.

v1 already established the machinery this builds on:

* selection of the entry module's eligible `pub` surface, captured post-typecheck
  in `module_compiler.select_lib_exports` and threaded as `anf.lib_exports`;
* a `twinkle.exports` custom section mirroring `twinkle.externs`;
* synthesized getter functions for value globals, and `ExportDef`s that seed
  Wasm DCE;
* a compiler-free `loadLib` (Node + web) that reads the section and builds typed
  wrappers, with `Int`↔`BigInt` coercion at the boundary.

This plan keeps all of that and widens the **type marshalling** at the edge. It
does **not** change the selection/emission/DCE architecture — only the set of
types each layer accepts and how `loadLib` converts them.

> Depends on the v1 lib build being landed. The JS↔Wasm-GC bridge is already
> embedded in `runtime.mjs`, so the same single-file `<name>.lib.wasm` guarantee
> holds for the wider ABI.

---

## Scope

Delivered in increments, each independently shippable. Ordered by value and by
how much new boundary machinery each one needs.

### 1. String args & returns

The headline follow-up. `pub fn greet(name: String) String` becomes a real
export (today it is skipped with a warning, which is what the scaffold template
demonstrates).

* **Eligibility** — `select_lib_exports` accepts `String` in params, returns,
  and value globals. The `LibPrimitive` enum grows beyond the four primitives, or
  a sibling `LibType` is introduced that carries `String` alongside the
  primitives; the `twinkle.exports` `kind`/`args`/`ret` metadata gains a `str`
  marshal tag (same vocabulary as `twinkle.externs`).
* **ABI** — a `String` is a GC ref (`rt_types__String`). An exported function
  returning `String` returns that ref; a `String` param takes one. This reuses
  the bridge the runtime already calls for `extern` `str` marshalling.
* **`loadLib`** — the wrapper converts a JS string → guest `String` for `str`
  args (via the bridge) and guest `String` → JS string for `str` returns. The
  `coerceLibArg`/`coerceLibReturn` switch gains a `str` case backed by the
  bridge handle `loadLib` already holds.

### 2. Function-typed (callback) parameters

`pub fn each_line(text: String, f: fn(String) Void)`, where the host passes a JS
callback. This is essentially a **dynamic `extern`**: it reuses the runtime's
host-function wrapping (`resolveExternImports`), bound per call instead of at
instantiate time.

* The exported function takes a guest closure; `loadLib` wraps the JS callback in
  a guest-callable closure for the duration of the call.
* Together with §1, **String + callback params** form what the v1 design called
  "approach C".

### 3. Compound values — `Vector`, `Dict`, records

Let lists, maps, and records cross the boundary. Two candidate strategies, to be
chosen during design:

* **JSON-ish encode/decode at the edge** — marshal to/from a plain JS
  value (array / object) by serializing at the boundary rather than exposing live
  GC refs. Simpler host ergonomics; copying cost at the edge.
* **Live GC-ref handles** — hand the host an opaque handle plus accessor helpers
  (`length`, `at`, field reads). No copy; more host API surface.

Element/field types are themselves drawn from this ABI (primitives, `String`,
nested compounds), so the marshaller is recursive. Records map to their nominal
struct; the `twinkle.exports` metadata must describe field names and types.

### 4. Returned closures

Handing JS a callable handle to a guest closure (`pub fn make_counter() fn() Int`),
leaning on the typed-closure struct (`$closure_*`) codegen already emits, plus a
JS `apply(closureRef, args)` helper in the runtime.

---

## Non-goals (for this plan)

* ~~Multiple lib entries per project — still a single `[lib] entry`.~~ Shipped
  later in [platform-build-bundles.md](platform-build-bundles.md); this plan
  predates it and assumed a single entry.
* Changing the v1 selection/emission/DCE architecture; this plan only widens
  type marshalling.
* `hard error` mode for ineligible members — warn-and-skip remains the policy for
  anything still outside the supported set.

---

## Testing

Each increment extends the two v1 test seams:

* **Boot** (`lib_export_suite.tw`) — assert the newly-eligible members are
  selected (not skipped), and that the `twinkle.exports` metadata carries the
  right marshal tags / field descriptions.
* **Runtime** (`runtime.test.mjs`) — build a fixture lib via `compile(..., { lib: true })`
  and `loadLib` it, then assert round-trips: a `String` echo, a callback invoked
  per element, a returned list/record read back, a returned closure applied.

---

## Affected Components

| Component | Change |
|-----------|--------|
| `boot/compiler/core_ir.tw` | widen `LibPrimitive`/add `LibType` to carry `String`, compound, and fn-typed exports; extend `LibExport` metadata (e.g. record field descriptors) |
| `boot/compiler/module_compiler.tw` | `select_lib_exports` accepts the wider type set; refine skip warnings to only fire for genuinely-unsupported members |
| `boot/compiler/codegen/{emit,wasm,wat}.tw` | emit getters/exports for ref-typed returns; richer `twinkle.exports` marshal tags |
| `tools/js_runtime/runtime.mjs` | `coerceLibArg`/`coerceLibReturn` gain `str`/compound/callback/closure cases backed by the embedded bridge; per-call host-callback binding |
| `tools/js_runtime/{index,web}.mjs` | surface any new `loadLib` options (e.g. closure `apply` helper) |
| Scaffold | once §1 lands, the template's `pub greeting: String` exports cleanly with no skip warning |
