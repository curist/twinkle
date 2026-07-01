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

### 1. String args & returns — **shipped**

The headline follow-up. `pub fn greet(name: String) String` (and
`pub greeting: String`) is now a real export — the scaffold template builds its
`String` value global cleanly with no skip warning. `LibPrimitive` was renamed to
`LibType` with a `Str` variant; the classifier accepts `String`, the
`twinkle.exports` section carries a `str` tag, and `loadLib`'s `coerceLibArg`/
`coerceLibReturn` marshal JS string ↔ guest `String` via the embedded bridge.

* **Eligibility** — `select_lib_exports` accepts `String` in params, returns,
  and value globals; the `twinkle.exports` `kind`/`args`/`ret` metadata gains a
  `str` marshal tag (same vocabulary as `twinkle.externs`).
* **Descriptor decision (resolved)** — rename `LibPrimitive` → `LibType` and add
  a flat `Str` variant now. `LibType` is *the* lib-ABI type descriptor going
  forward, so `LibExport.params: Vector<LibType>` / `ret: LibType` and every call
  site stay stable across all later increments — 2–4 only add *variants*. The
  metadata encoder returns a **JSON descriptor** per arg where leaves stay bare
  strings (`"int"`, `"str"`, matching `twinkle.externs`) and future compounds
  nest as objects (`{"kind":"vector","elem":"str"}`); `twinkle.exports` is already
  a JSON payload, so no format rework is needed later. The classifier
  `lib_type(ty: MonoType) LibType?` and the JS `coerceLib*` switch are written for
  additive growth (each future type = one more `case`/`switch` arm recursing on
  element/field types). Deliberately **not** stubbing `Vector`/`Record`/`Fn`
  variants now — an exhaustive `case` would force dead placeholder arms; those
  variants land *with* the increment that produces and tests them.
* **ABI** — a `String` is a GC ref (`rt_types__String`). An exported function
  returning `String` returns that ref; a `String` param takes one. This reuses
  the bridge the runtime already calls for `extern` `str` marshalling.
* **`loadLib`** — the wrapper converts a JS string → guest `String` for `str`
  args (via the bridge) and guest `String` → JS string for `str` returns. The
  `coerceLibArg`/`coerceLibReturn` switch gains a `str` case backed by the
  bridge handle `loadLib` already holds.

### 2. Function-typed (callback) parameters — **shipped**

`pub fn each_word(text: String, f: fn(String) Void)` and value-returning
callbacks like `fn(Int) Int` now cross the boundary: the host passes a plain JS
callback, driven synchronously per call. `LibType` grew its first recursive
variant `Fn(params, ret)`; codegen emits, per callback signature, a universal
`rt_types__ClosureFunc` trampoline, a native-typed `twinkle.lib.cb_<key>` host
import, and an exported `__lib_make_cb_<key>` constructor; `loadLib` keeps a
callback registry and marshals args/returns with the Increment-1 coercers. See
[lib-export-callback-params.md](archive/lib-export-callback-params.md).

`pub fn each_line(text: String, f: fn(String) Void)` — and value-returning
callbacks like `fn(Int) Bool` / `fn(String) String` — where the host passes a JS
callback. Together with §1, **String + callback params** form what the v1 design
called "approach C".

**Scope.** Callbacks are invoked **synchronously** during the export call (no
JSPI needed). Callback arg and return types are restricted to the §1 set
(primitives + `String`; `Void` allowed as a return). Callbacks whose own
parameters are compound (`Vector`/`Dict`/record) or themselves functions are
skipped with a warning, same as any other ineligible member.

**Why it can't be a plain guest closure handed over from JS.** A Wasm GC
closure's funcref must point at a *wasm* function, and funcrefs are
instance-bound — JS cannot fabricate one. So the guest builds the closure; JS
only supplies a callback id and drives it through an import.

**The uniform-funcref insight.** When the guest calls a closure it can't
statically type (any host-supplied one), it uses the closure's **universal
funcref** (field 0), whose type is the single, arity/type-agnostic
`rt_types__ClosureFunc: (env: anyref, args: anyref-array) → anyref` — args boxed
and packed into an `rt_types__Array`, result boxed. `ref.test` for the concrete
typed struct fails on a host-supplied closure, so the universal path is always
taken. This means one trampoline *type*, not one per signature.

* **`LibType`** gains its first recursive variant,
  `Fn(params: Vector<LibType>, ret: LibType)`. The classifier `lib_type` recurses:
  a param typed `fn(A…) R` is eligible iff every `A` and `R` is an eligible
  `LibType`. The `twinkle.exports` descriptor for such an arg nests —
  `{"kind":"fn","args":[…],"ret":…}` — leaves staying bare strings, exactly the
  JSON nesting §1 established.
* **Codegen** emits, per distinct callback signature `S` that appears in a lib
  export param (only in a `--lib` build):
  * a wasm **trampoline** of type `rt_types__ClosureFunc`. Body: unbox `cbid`
    from `env` (i31), unpack the args array and `emit_unbox_from_anyref` each
    element to its native type, call the per-signature host import, then
    `emit_box_to_anyref` the result. All four helpers already exist.
  * a host **import** `__lib_cb_<S>(cbid: i32, …native args) → native ret` — its
    params/result are native boundary types (i64/f64/i32/`String` ref), so the
    JS side reuses §1's `coerceLibArg`/`coerceLibReturn` unchanged.
  * an exported **constructor** `__lib_make_cb_<S>(cbid: i32) → anyref` building
    the generic 2-field `rt_types__Closure { funcref: trampoline, env: i31(cbid) }`
    guest-side, where `ref.func` is legal.
* **`loadLib`** keeps a per-lib callback **registry** (monotonic id → JS fn, so a
  callback reentering the lib is safe). For an arg whose descriptor is `fn`:
  allocate a `cbid`, register the JS callback, call `__lib_make_cb_<S>(cbid)` to
  get the closure ref, pass it into the export, and unregister after the
  top-level export returns. The provided `__lib_cb_<S>` import looks up `cbid`,
  marshals the guest args → JS per the nested descriptor, invokes the callback,
  and marshals the return JS → guest.

Data flow: `guest each_line → CallRef(universal) → trampoline → import
__lib_cb_S → registry[cbid] → jsFn → marshalled return → trampoline boxes →
guest`.

### 3. Compound values — `Vector`, `Dict`, records — **shipped**

`Vector`, `Dict`, and records cross the boundary as **plain JS arrays / objects /
Maps** (JSON-ish copy at the edge), as args and returns, recursively
(`Vector<Record>`, `Dict<String, Vector<Row>>`, record-with-`Vector`-field).
Functions inside compounds stay out. The bridge gained `boxed_int/boxed_float`
accessors; codegen emits generic vec/dict primitives (reusing
`rt_arr__to_array`/`from_array` and the `rt_dict__*` ABI) plus per-record
read/make helpers; the host walks the recursive descriptor. See
[lib-export-compound-values.md](archive/lib-export-compound-values.md).

**Why copy, not live handles.** The host gets ordinary JS values, matching
`loadLib`'s purpose. Live GC-ref handles would need the *same* per-type guest
accessors (the bridge cannot read a PVec/HAMT/struct) for worse ergonomics; a
guest JSON-string codec needs per-type parsers anyway and loses i64 precision.

**Uniform intermediate — a flat-array tree.** The bridge can only build/read the
flat `rt_types__Array`, so every compound marshals through a tree of
`rt_types__Array` whose leaves are bridge-readable boxed scalars. Per distinct
compound monotype at the boundary, codegen emits two recursive guest-side
helpers (mirroring Increment 2's per-signature emission):

* `__lib_read_<key>(guest) → rt_types__Array` — flattens a node, recursively
  calling sub-helpers so the top call yields a complete flat tree;
* `__lib_make_<key>(rt_types__Array) → guest` — rebuilds the PVec / PDict /
  struct, recursively.

JS makes one top-level call per direction, then walks the **recursive
`twinkle.exports` descriptor** (already JSON) in parallel with the flat tree:

* `{"kind":"vec","elem":D}` → JS array;
* `{"kind":"dict","key":D,"val":D}` → JS object (String keys) / Map (otherwise);
* `{"kind":"rec","name":N,"fields":[[name,D],…]}` → JS object.

Records: the classifier runs post-typecheck, where field names and declaration
order are in the `ResolvedEnv`; make/read use `StructNew`/`StructGet` in
wasm-struct field order (= declaration order).

**Enabler — bridge boxed-scalar accessors.** JS can read i31 and `String`, but a
`Vector<Int>` leaf is an i64 in a `BoxedInt` struct the bridge cannot open. So the
embedded bridge (`bridge.tw`) gains `boxed_int_new/get` and `boxed_float_new/get`
(mirroring `i31_new/get`), rebuilt into `bridge_bytes.mjs` by `make stage2`. Then
leaves are: int→`BoxedInt`, float→`BoxedFloat`, bool→i31, str→`String` ref — all
JS-readable.

Build order (one branch, incremental tasks): bridge boxed accessors → `LibType`
compound variants + recursive classifier → recursive descriptor → Vector helpers
→ record helpers → Dict helpers → recursive JS marshaller → nesting.

### 4. Returned closures — **shipped**

A guest closure returned from an export (`pub fn adder(n: Int) fn(Int) Int`)
becomes a **callable JS function**. The inverse of §2: rather than a per-signature
constructor, one generic `__lib_apply(closure, args)` helper invokes the closure
through its universal funcref (field 0, `rt_types__ClosureFunc`) — a copy of the
compiler's own universal-call path. The host wraps the returned closure ref in a
JS function that boxes args into a flat array, calls `__lib_apply`, and
unmarshals the boxed result per the `fn` descriptor. Captured env, arguments, and
`String`/leaf results round-trip. (Closures capture immutably, per the language —
there is no mutable captured state.)

---

## Non-goals (for this plan)

* ~~Multiple lib entries per project — still a single `[lib] entry`.~~ Shipped
  later in [platform-build-bundles.md](archive/platform-build-bundles.md); this plan
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
