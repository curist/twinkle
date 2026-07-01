# Lib-Export Compound Values — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans (or subagent-driven-development) to implement this task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Let `Vector`, `Dict`, and records (recursively, as args and returns) cross the lib-export boundary as plain JS arrays / objects / Maps — Increment 3 of [lib-export-abi.md](lib-export-abi.md).

**Architecture:** JSON-ish copy at the edge through a uniform flat-`rt_types__Array` tree. Per distinct compound monotype at the boundary, codegen emits recursive guest-side `__lib_read_<key>`/`__lib_make_<key>` helpers that convert guest⇄flat-tree (reusing `rt_arr__to_array`/`rt_arr__from_array` for vectors, `StructGet`/`StructNew` for records, dict runtime ops for dicts). The `twinkle.exports` descriptor grows recursive `vec`/`dict`/`rec` shapes. JS walks the descriptor + flat tree with the Increment-1/2 coercers. The embedded bridge gains boxed-scalar accessors so i64/f64 leaves are JS-readable.

**Tech Stack:** Twinkle boot compiler, WebAssembly GC, embedded bridge (`bridge.tw` → `bridge_bytes.mjs`), JS runtime, boot test runner + `node --test`.

**Verify loop:** boot library-logic edits are picked up live by `TWK_TEST_FILTER=… target/twk run boot/tests/main.tw`; anything touching emitted wasm, the bridge, or the JS round-trip needs `make stage2` then `cp target/boot.wasm tools/js_runtime/boot.wasm` before `node --test`. `make stage2` also regenerates `bridge_bytes.mjs`.

**Key symbols (verified in tree):**
- `rt_types__BoxedInt` = `Struct("BoxedInt", [{v, immut, i64}], final)`; `rt_types__BoxedFloat` = same with f64 (`boot/compiler/codegen/runtime/types.tw:121`).
- `rt_arr__to_array(PVec) → rt_types__Array` and `rt_arr__from_array(rt_types__Array) → PVec` (`boot/compiler/codegen/runtime/arr.tw`, used by the extern boundary).
- Bridge funcs live in `bridge.tw` `funcs()`; types in `types()`. Pattern: `i31_new`/`i31_get`.
- `emit_box_to_anyref`/`emit_unbox_from_anyref` (`emit/anyref.tw`) box/unbox a MonoType leaf.

---

### Task 1: Bridge boxed-scalar accessors

**Files:** Modify `boot/compiler/codegen/bridge.tw` (`types()` ~38, `funcs()` ~55). Test: `tools/js_runtime/runtime.test.mjs`.

- [ ] **Step 1: Failing JS test** — a boxed-int round-trip through the bridge. Add to `runtime.test.mjs`:

```js
test("bridge round-trips boxed int and float", async () => {
  const b = instantiateBridge();
  const bi = b.boxed_int_new(9007199254740993n); // > 2^53, must stay exact
  assert.equal(b.boxed_int_get(bi), 9007199254740993n);
  const bf = b.boxed_float_new(3.5);
  assert.equal(b.boxed_float_get(bf), 3.5);
});
```
`instantiateBridge` is currently module-private in `runtime.mjs`; export it (`export function instantiateBridge`) and import it in the test.

- [ ] **Step 2: Run → fail** — `node --test --test-name-pattern="boxed int and float" tools/js_runtime/runtime.test.mjs` → `b.boxed_int_new is not a function`.

- [ ] **Step 3: Add the types** in `bridge.tw` `types()` (structurally identical to the main module's, so cross-instance refs unify):

```tw
    .Struct("BoxedInt", [.{ name: .Some("v"), mutable: false, ty: .I64 }], .None, true),
    .Struct("BoxedFloat", [.{ name: .Some("v"), mutable: false, ty: .F64 }], .None, true),
```

Add helper refs near `i31_ref()`:
```tw
fn boxed_int_null() ValType { .Ref(true, .Named("BoxedInt")) }
fn boxed_int_ref() ValType { .Ref(false, .Named("BoxedInt")) }
fn boxed_float_null() ValType { .Ref(true, .Named("BoxedFloat")) }
fn boxed_float_ref() ValType { .Ref(false, .Named("BoxedFloat")) }
```

- [ ] **Step 4: Add the funcs** in `funcs()`:

```tw
    .{
      name: "boxed_int_new",
      params: [.I64],
      results: [boxed_int_ref()],
      locals: [],
      body: [.LocalGet(0), .StructNew("BoxedInt")],
    },
    .{
      name: "boxed_int_get",
      params: [boxed_int_null()],
      results: [.I64],
      locals: [],
      body: [.LocalGet(0), .StructGet("BoxedInt", 0)],
    },
    .{
      name: "boxed_float_new",
      params: [.F64],
      results: [boxed_float_ref()],
      locals: [],
      body: [.LocalGet(0), .StructNew("BoxedFloat")],
    },
    .{
      name: "boxed_float_get",
      params: [boxed_float_null()],
      results: [.F64],
      locals: [],
      body: [.LocalGet(0), .StructGet("BoxedFloat", 0)],
    },
```

- [ ] **Step 5: Rebuild bridge + run** — `make stage2` (regenerates `bridge_bytes.mjs`), `cp target/boot.wasm tools/js_runtime/boot.wasm`, then the test → PASS. Also run the existing "embedded bridge bytes match" guard test.

- [ ] **Step 6: Commit** — `git add boot/compiler/codegen/bridge.tw tools/js_runtime/runtime.mjs tools/js_runtime/runtime.test.mjs && git commit -m "Add boxed_int/boxed_float accessors to the embedded bridge"`

---

### Task 2: `LibType` compound variants + recursive classifier

**Files:** `boot/compiler/core_ir.tw` (LibType), `boot/compiler/module_compiler.tw` (`lib_type`), test `boot/tests/suites/lib_export_suite.tw`.

The classifier needs record field names/order. A record is `MonoType.Named(tid, args)`; the `ResolvedEnv` (already passed to `select_lib_exports`) resolves a `tid` to its `TypeEntry` with an ordered field list. **Confirm at task start:** the env accessor for a record type's fields (grep `ResolvedVariant`/`TypeEntry`/`fields` in `resolver.tw`); records are product types (single variant / field list).

- [ ] **Step 1: Failing tests** in `lib_export_suite.tw` — extend `lib_type_eq` with `.Vec`, `.Dct`, `.Rec` arms (recursive compare), then:

```tw
    .test(
      "Vector/Dict/record params are selected as recursive descriptors",
      fn() {
        src := "pub type Pt = .{ x: Int, y: Int }\npub fn f(xs: Vector<Int>, m: Dict<String, Int>, p: Pt) Pt {\n  p\n}\n"
        artifacts := try pipeline.compile_source_lib(src)
        e := try find_export(artifacts.anf.lib_exports, "f").ok_or("f missing")
        try assert.equal(lib_type_eq(e.params[0], .Vec(.Int)), true)
        try assert.equal(lib_type_eq(e.params[1], .Dct(.Str, .Int)), true)
        // record: name + ordered fields
        try assert.equal(is_rec_named(e.params[2], "Pt", [("x", LibType.Int), ("y", LibType.Int)]), true)
        .Ok({})
      },
    )
```
(`is_rec_named` is a small test helper matching the `.Rec` variant.)

- [ ] **Step 2: Run → fail** (unknown variants).

- [ ] **Step 3: Extend `LibType`** in `core_ir.tw`. Records need a name + ordered `(String, LibType)` fields:

```tw
pub type LibField = .{ name: String, ty: LibType }
pub type LibType = {
  Int, Float, Bool, Void, Str,
  Fn(Vector<LibType>, LibType),
  Vec(LibType),
  Dct(LibType, LibType),
  Rec(String, Vector<LibField>),
}
```

- [ ] **Step 4: Recurse in `lib_type`** (`module_compiler.tw`). Add before the `_ => .None` arm:

```tw
    .Vector(elem) => case lib_type(elem) {
      .Some(le) => .Some(.Vec(le)),
      .None => .None,
    },
    .Dict(k, v) => {
      lk := case lib_type(k) { .Some(x) => x, .None => return .None }
      lv := case lib_type(v) { .Some(x) => x, .None => return .None }
      .Some(.Dct(lk, lv))
    },
    .Named(tid, _) => classify_record(tid, env),  // record fields via env; .None if not a plain record or a field is ineligible
```
`classify_record` looks up the record's ordered fields in `env`, recursing `lib_type` on each field type; returns `.Some(.Rec(name, fields))` or `.None`. `lib_type` must therefore take `env` (thread it through — currently `lib_type(ty)`; add `env: ResolvedEnv`). Enums (multi-variant Named) → `.None`.

- [ ] **Step 5: Run → pass**; then full boot suite (`target/twk run boot/tests/main.tw`) to catch non-recursive `case LibType` sites (there are none left after Increment 2 except `export_type_json`, handled next task; `cb_sig_key`/`lib_type_native_*`/`lib_type_to_mono` in emit.tw are callback-only and must gain compound arms returning an error/unreachable since compounds never appear as callback arg tags in this task — add `_ => error(...)` or explicit arms).

- [ ] **Step 6: Commit** — "Select Vector/Dict/record lib-export params as recursive LibType".

---

### Task 3: Recursive `vec`/`dict`/`rec` descriptors in `twinkle.exports`

**Files:** `boot/compiler/codegen/codegen.tw` (`export_type_json`), test `lib_export_suite.tw`.

- [ ] **Step 1: Failing test** — assert nested JSON:

```tw
        try assert.equal(codegen.export_type_json(.Vec(.Int)), "{\"kind\":\"vec\",\"elem\":\"int\"}")
        try assert.equal(codegen.export_type_json(.Dct(.Str, .Int)), "{\"kind\":\"dict\",\"key\":\"str\",\"val\":\"int\"}")
        try assert.equal(
          codegen.export_type_json(.Rec("Pt", [.{ name: "x", ty: .Int }])),
          "{\"kind\":\"rec\",\"name\":\"Pt\",\"fields\":[[\"x\",\"int\"]]}",
        )
```

- [ ] **Step 2: Run → fail** (missing `case` arms / compile error).

- [ ] **Step 3: Extend `export_type_json`** with arms:

```tw
    .Vec(e) => "{\"kind\":\"vec\",\"elem\":${export_type_json(e)}}",
    .Dct(k, v) => "{\"kind\":\"dict\",\"key\":${export_type_json(k)},\"val\":${export_type_json(v)}}",
    .Rec(name, fields) => {
      parts := collect f in fields {
        "[${json_string(f.name)},${export_type_json(f.ty)}]"
      }
      "{\"kind\":\"rec\",\"name\":${json_string(name)},\"fields\":[${parts.join(",")}]}"
    },
```
(`json_string` is defined in `wasm.tw`; if not visible from `codegen.tw`, inline `"\"${name}\""` — record/field names are identifiers, no escaping needed.)

- [ ] **Step 4: Run → pass**; full boot suite.

- [ ] **Step 5: Commit** — "Emit recursive vec/dict/rec descriptors in twinkle.exports".

---

### Task 4: Vector marshalling helpers (guest-side) + JS

**Files:** `boot/compiler/codegen/emit.tw` (extend the lib-export emission with compound helpers), `tools/js_runtime/runtime.mjs` (recursive marshaller), tests both sides.

**Design.** Reuse `rt_arr__to_array`/`rt_arr__from_array`. Per distinct `Vec(elem)` monotype at the boundary emit:
- `__lib_read_<key>(v: anyref) → rt_types__Array`: `RefCast` v to `rt_types__PVec`, `Call rt_arr__to_array` → flat array of the vector's (boxed) elements. **If elem is itself compound**, additionally map each element through `__lib_read_<elemkey>` into a fresh array (loop with `array_new`-style ops via the runtime `rt_arr` builders) so the tree is fully flattened. For leaf elem, `rt_arr__to_array` already yields JS-readable boxed leaves.
- `__lib_make_<key>(a: rt_types__Array) → anyref`: for leaf elem, `Call rt_arr__from_array`. For compound elem, first rebuild each element via `__lib_make_<elemkey>`, then `rt_arr__from_array`.

`<key>` is a sanitized monotype key (extend the callback `cb_sig_key` scheme to a general `lib_type_key(LibType) String`, e.g. `vec_int`, `vec_rec_Pt`, `dict_str_int`).

**Confirm at task start:** whether a boundary `Vector<Int>` is the generic boxed PVec (elements already `BoxedInt`, so `rt_arr__to_array` yields `BoxedInt` refs) or a typed `PVecI64`. If typed, coerce to the boxed family first (grep `route_typed_vec`), or require the boundary value be generic. Validate with the leaf round-trip below before building nesting.

- [ ] **Step 1: Failing JS test** — `Vector<Int>` and `Vector<String>` echo:

```js
test("loadLib round-trips Vector args/returns", async () => {
  const src = [
    "pub fn dbl(xs: Vector<Int>) Vector<Int> {",
    "  collect x in xs { x * 2 }",
    "}",
    "pub fn shout(ws: Vector<String>) Vector<String> {",
    "  collect w in ws { \"${w}!\" }",
    "}",
  ].join("\n");
  const lib = await loadLib(await compile({ source: src }, { lib: true }));
  assert.deepEqual(lib.dbl([1n, 2n, 3n]), [2n, 4n, 6n]);
  assert.deepEqual(lib.shout(["a", "b"]), ["a!", "b!"]);
});
```

- [ ] **Step 2:** `make stage2` + stage boot.wasm, run → fail.

- [ ] **Step 3: Boot codegen.** In `emit.tw`, generalize the callback collection to also gather compound monotypes from export params/returns, and emit read/make helpers + `ExportDef`s (`__lib_read_<key>`, `__lib_make_<key>`). Follow `emit_callback_artifacts`' structure. Add `lib_type_key`. Read/make bodies as above using `rt_arr__to_array`/`rt_arr__from_array` + `emit_box_to_anyref`/`emit_unbox_from_anyref` for the compound-element loop.

- [ ] **Step 4: JS marshaller.** Add recursive `jsToGuest(value, desc, b, instance)` / `guestToJs(ref, desc, b, instance)`:
  - leaf: existing coercers (`int`→boxed via `b.boxed_int_new`/`get`, `float`→boxed, `bool`→i31, `str`→string).
  - `vec`: `guestToJs` calls `instance.exports["__lib_read_"+key(desc)](ref)`, then reads the flat array (`b.array_len`/`b.array_get`) mapping each through `guestToJs(elem, desc.elem)`; `jsToGuest` builds a flat array (`b.array_new`/`array_set`) of `jsToGuest(x, desc.elem)` then calls `__lib_make_<key>`.
  Wire `coerceLibArg`/`coerceLibReturn` to dispatch to these when `kind` is an object with `kind==="vec"` (later `dict`/`rec`). `key(desc)` mirrors boot `lib_type_key`.

- [ ] **Step 5:** run → pass; full JS suite.

- [ ] **Step 6: Commit** — "Marshal Vector args/returns across the lib boundary".

---

### Task 5: Record marshalling helpers + JS

**Files:** `emit.tw`, `runtime.mjs`, tests.

Per `Rec(name, fields)`:
- `__lib_read_<key>(s: anyref) → rt_types__Array`: `RefCast` to the record struct type, build a `rt_types__Array` of `fields.len()`; for each field i, `StructGet(recSym, i)`, `emit_box_to_anyref(field_mono)` (or recursive `__lib_read_<fieldkey>` for compound fields), `array_set`.
- `__lib_make_<key>(a: rt_types__Array) → anyref`: for each field i, `array_get(a, i)`, `emit_unbox_from_anyref(field_mono)` (or `__lib_make_<fieldkey>`), then `StructNew(recSym)`.

**Confirm at task start:** the record's wasm struct symbol from the monotype (grep how a `Named(tid)` record maps to its `recSym` in the type registry / `wasm_layout`), and that struct field order matches the descriptor's declaration order.

- [ ] **Step 1: Failing JS test** — record round-trip (object ⇄ struct):

```js
test("loadLib round-trips a record", async () => {
  const src = [
    "pub type Pt = .{ x: Int, y: Int }",
    "pub fn mk(a: Int, b: Int) Pt { Pt.{ x: a, y: b } }",
    "pub fn swap(p: Pt) Pt { Pt.{ x: p.y, y: p.x } }",
  ].join("\n");
  const lib = await loadLib(await compile({ source: src }, { lib: true }));
  assert.deepEqual(lib.mk(1n, 2n), { x: 1n, y: 2n });
  assert.deepEqual(lib.swap({ x: 3n, y: 4n }), { x: 4n, y: 3n });
});
```

- [ ] **Step 2:** rebuild + run → fail.
- [ ] **Step 3:** emit record helpers (Step design above).
- [ ] **Step 4:** JS `rec` case — `guestToJs` builds `{ [fieldName]: guestToJs(elem_i, fieldDesc_i) }`; `jsToGuest` builds the flat array in field order from the object.
- [ ] **Step 5:** run → pass; full JS suite.
- [ ] **Step 6: Commit** — "Marshal record args/returns across the lib boundary".

---

### Task 6: Dict marshalling helpers + JS

**Files:** `emit.tw`, `runtime.mjs`, tests.

**Confirm at task start:** dict runtime symbols for build + iterate (grep `runtime/dict.tw` and prelude FuncIds 13 `dict_set`, 22 `dict_new`, 14 `dict_keys`, 21 `dict_get`, 24 `dict_get_unsafe`, 27 `dict_len`). A `for k, v in dict` lowers to `dict$get_unsafe` iteration — mirror that in the read helper.

Per `Dct(k, v)`:
- `__lib_read_<key>(d: anyref) → rt_types__Array`: build a flat array of length `2 * dict_len` as alternating `[k0, v0, k1, v1, …]`, boxing keys/values (or recursing for compound). Iterate in insertion order.
- `__lib_make_<key>(a: rt_types__Array) → anyref`: `dict_new`, then loop pairs calling `dict_set` with unboxed/rebuilt key+value.

- [ ] **Step 1: Failing JS test** — `Dict<String,Int>` (→ JS object) and `Dict<Int,Int>` (→ Map):

```js
test("loadLib round-trips a Dict", async () => {
  const src = [
    "pub fn inc_all(m: Dict<String, Int>) Dict<String, Int> {",
    "  out := m",
    "  for k, v in m { out[k] = v + 1 }",
    "  out",
    "}",
  ].join("\n");
  const lib = await loadLib(await compile({ source: src }, { lib: true }));
  assert.deepEqual(lib.inc_all({ a: 1n, b: 2n }), { a: 2n, b: 3n });
});
```

- [ ] **Step 2:** rebuild + run → fail.
- [ ] **Step 3:** emit dict helpers.
- [ ] **Step 4:** JS `dict` case — String-keyed → object, else → Map; `guestToJs`/`jsToGuest` walk the alternating flat array.
- [ ] **Step 5:** run → pass; full JS suite.
- [ ] **Step 6: Commit** — "Marshal Dict args/returns across the lib boundary".

---

### Task 7: Nesting + finalization

- [ ] **Step 1: Failing nested test** — `Vector<Record>`, record-with-`Vector`-field, `Dict<String, Vector<Int>>`:

```js
test("loadLib round-trips nested compounds", async () => {
  const src = [
    "pub type Row = .{ id: Int, tags: Vector<String> }",
    "pub fn rows() Vector<Row> {",
    "  collect i in range(2) { Row.{ id: i, tags: [\"t${i}\"] } }",
    "}",
  ].join("\n");
  const lib = await loadLib(await compile({ source: src }, { lib: true }));
  assert.deepEqual(lib.rows(), [
    { id: 0n, tags: ["t0"] },
    { id: 1n, tags: ["t1"] },
  ]);
});
```

- [ ] **Step 2:** rebuild + run. If the recursive helper emission from Tasks 4–5 already covers nesting (it should — read/make recurse via `__lib_<read|make>_<elemkey>`), this passes; otherwise fix the compound-element loop.
- [ ] **Step 3:** Full boot suite + full JS suite green.
- [ ] **Step 4:** `target/twk fmt` edited `.tw` files; `target/twk lint boot/main.tw` (No findings).
- [ ] **Step 5: End-to-end smoke** — scaffold a project, build `--lib` with a `Vector<Record>` export, `loadLib` the on-disk artifact, assert the nested round-trip.
- [ ] **Step 6:** Mark Increment 3 shipped in `docs/plans/lib-export-abi.md`; archive this plan and drop its README row.
- [ ] **Step 7: Commit** — "docs(plans): mark lib-export ABI Increment 3 shipped".

---

## Self-Review Notes

- **Spec coverage:** bridge enabler (T1), selection + descriptor (T2–T3), the three compound marshallers (T4–T6), recursion + finalize (T7) — covers every bullet of Increment 3 in lib-export-abi.md.
- **Reuse:** `rt_arr__to_array`/`rt_arr__from_array` for vectors, `StructGet`/`StructNew` for records, dict runtime ops for dicts — no new trie/HAMT code. The recursive JS marshaller and the recursive descriptor are the Increment-1/2 foundation extended.
- **Confirm-at-task flags (resolve in the TDD loop, not blocking the plan):** typed-`PVecI64` vs generic boxed PVec at the boundary (T4); record `recSym` + field order from the type registry (T5); dict iterate/build symbols and insertion order (T6); `env`-based record field lookup in the classifier (T2).
- **Risk:** the typed-vector representation (T4 flag) is the deepest unknown; validate the `Vector<Int>` leaf round-trip before building nesting on top.
