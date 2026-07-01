# Lib-Export Callback Parameters — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a `pub` lib export take a function-typed parameter (`fn(Args…) Ret`) that the host drives with a JS callback — Increment 2 of [lib-export-abi.md](lib-export-abi.md).

**Architecture:** Selection (post-typecheck) accepts `fn` params whose arg/return types are the Increment-1 set (primitives + `String`, `Void` return); a new recursive `LibType.Fn` carries them. A host-supplied closure is always invoked through the closure's **universal funcref** (`rt_types__ClosureFunc: (anyref env, anyref args) → anyref`), so codegen emits, per distinct callback signature, one universal-typed trampoline, one native-typed host import, and one exported constructor that builds the guest closure. `loadLib` keeps a callback registry and marshals args/returns with the Increment-1 coercers.

**Tech Stack:** Twinkle boot compiler (`boot/compiler/**`), WebAssembly GC, JS runtime (`tools/js_runtime/runtime.mjs`), boot test runner + `node --test`.

**Verify loop (whole plan):** boot logic edits are picked up live by `TWK_TEST_FILTER="lib exports" target/twk run boot/tests/main.tw`; anything exercising emitted wasm through JS needs `make stage2` then `cp target/boot.wasm tools/js_runtime/boot.wasm` before `node --test`.

---

### Task 1: `LibType.Fn` variant + recursive classifier

**Files:**
- Modify: `boot/compiler/core_ir.tw` (LibType, ~line 117)
- Modify: `boot/compiler/module_compiler.tw` (`lib_type`, ~line 425; the param loop ~309)
- Test: `boot/tests/suites/lib_export_suite.tw`

- [ ] **Step 1: Write the failing boot test**

Add to `lib_export_suite.tw`. Also extend `lib_type_eq` with a `.Fn` arm that compares recursively (params + ret), so the test can assert the descriptor.

```tw
    .test(
      "fn-typed param is selected with a recursive Fn descriptor",
      fn() {
        src := "${eligible_src()}\npub fn each(name: String, f: fn(String) Void) Void {\n  f(name)\n}\n"
        artifacts := try pipeline.compile_source_lib(src)
        e := try find_export(artifacts.anf.lib_exports, "each").ok_or("each not exported")
        try assert.equal(e.params.len(), 2)
        try assert.equal(lib_type_eq(e.params[0], .Str), true)
        try assert.equal(lib_type_eq(e.params[1], .Fn([.Str], .Void)), true)
        .Ok({})
      },
    )
    .test(
      "value-returning fn-typed param is eligible",
      fn() {
        src := "${eligible_src()}\npub fn pick(n: Int, p: fn(Int) Bool) Bool {\n  p(n)\n}\n"
        artifacts := try pipeline.compile_source_lib(src)
        e := try find_export(artifacts.anf.lib_exports, "pick").ok_or("pick not exported")
        try assert.equal(lib_type_eq(e.params[1], .Fn([.Int], .Bool)), true)
        .Ok({})
      },
    )
    .test(
      "fn with compound arg is skipped with a warning",
      fn() {
        src := "${eligible_src()}\npub fn go(f: fn(Vector<Int>) Void) Void {\n  {}\n}\n"
        artifacts := try pipeline.compile_source_lib(src)
        try assert.equal(find_export(artifacts.anf.lib_exports, "go").is_some(), false)
        try assert.str_contains(warning_text(artifacts.warnings), "`go`")
        .Ok({})
      },
    )
```

Extend `lib_type_eq` (add before the closing `}` of its `case`):

```tw
    .Fn(ps, r) => case want {
      .Fn(wps, wr) => if ps.len() != wps.len() {
        false
      } else {
        ok := lib_type_eq(r, wr)
        for i in 0..ps.len() {
          if !lib_type_eq(ps[i], wps[i]) {
            ok = false
          }
        }
        ok
      },
      _ => false,
    },
```

- [ ] **Step 2: Run to verify it fails**

Run: `TWK_TEST_FILTER="lib exports" target/twk run boot/tests/main.tw`
Expected: compile error `unknown variant .Fn` (production `LibType` has no `Fn`).

- [ ] **Step 3: Add the `Fn` variant**

In `core_ir.tw`, extend `LibType`:

```tw
pub type LibType = { Int, Float, Bool, Void, Str, Fn(Vector<LibType>, LibType) }
```

- [ ] **Step 4: Make the classifier recurse**

In `module_compiler.tw`, replace `lib_type` with:

```tw
fn lib_type(ty: MonoType) LibType? {
  case ty {
    .Int => .Some(.Int),
    .Float => .Some(.Float),
    .Bool => .Some(.Bool),
    .Void => .Some(.Void),
    .Never => .Some(.Void),
    .String => .Some(.Str),
    .Function(params, ret) => {
      lib_params: Vector<LibType> = []
      for p in params {
        case lib_type(p) {
          .Some(lp) => lib_params = .append(lp),
          .None => return .None,
        }
      }
      lib_ret := try lib_type(ret).ok_or_none()
      .Some(.Fn(lib_params, lib_ret))
    },
    _ => .None,
  }
}
```

If `ok_or_none()` is unavailable, inline it:

```tw
      lib_ret := case lib_type(ret) {
        .Some(lr) => lr,
        .None => return .None,
      }
      .Some(.Fn(lib_params, lib_ret))
```

- [ ] **Step 5: Run to verify pass**

Run: `TWK_TEST_FILTER="lib exports" target/twk run boot/tests/main.tw`
Expected: PASS (all lib-export tests green). Then run the full suite `target/twk run boot/tests/main.tw` to catch exhaustive-`case` breakage from the new variant (fix any non-recursive `case LibType` sites — at this point only `export_type_kind` in `codegen.tw`, handled in Task 2).

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/core_ir.tw boot/compiler/module_compiler.tw boot/tests/suites/lib_export_suite.tw
git commit -m "Select fn-typed lib-export params as recursive LibType.Fn"
```

---

### Task 2: Nested `fn` descriptor in `twinkle.exports`

**Files:**
- Modify: `boot/compiler/codegen/codegen.tw` (`export_type_kind` ~219, `collect_export_meta` ~232)
- Modify: `boot/compiler/codegen/wasm.tw` (`export_meta_json` ~1461)
- Modify: `boot/compiler/codegen/wasm_ir.tw` (`ExportMeta` ~206)
- Test: `boot/tests/suites/codegen_emit_suite.tw` (or wherever export-meta JSON is asserted; else add a focused check here)

The section is JSON. Leaves stay bare strings; an `fn` arg becomes a nested object `{"kind":"fn","args":[…],"ret":…}`. To carry mixed string/object descriptors, `ExportMeta.args`/`ret` hold **raw JSON value strings** produced by a recursive encoder, and `export_meta_json` emits them raw instead of re-quoting.

- [ ] **Step 1: Write the failing test**

Add a boot test that builds the JSON descriptor and asserts nesting. In `lib_export_suite.tw`, import the encoder is internal, so assert via the emitted section is heavier; instead unit-test the descriptor by exposing a small `pub` encoder. Add to `codegen.tw`:

```tw
pub fn export_type_json(t: LibType) String {
  case t {
    .Int => "\"int\"",
    .Float => "\"float\"",
    .Bool => "\"bool\"",
    .Void => "\"void\"",
    .Str => "\"str\"",
    .Fn(ps, r) => {
      arg_parts := collect p in ps {
        export_type_json(p)
      }
      "{\"kind\":\"fn\",\"args\":[${arg_parts.join(",")}],\"ret\":${export_type_json(r)}}"
    },
  }
}
```

Add a test in `boot/tests/suites/codegen_emit_suite.tw` (import `codegen` and `LibType`):

```tw
    .test(
      "export_type_json nests fn descriptors, leaves stay bare strings",
      fn() {
        try assert.equal(codegen.export_type_json(.Str), "\"str\"")
        try assert.equal(
          codegen.export_type_json(.Fn([.Str], .Void)),
          "{\"kind\":\"fn\",\"args\":[\"str\"],\"ret\":\"void\"}",
        )
        .Ok({})
      },
    )
```

- [ ] **Step 2: Run to verify it fails**

Run: `TWK_TEST_FILTER="export_type_json" target/twk run boot/tests/main.tw`
Expected: FAIL — `export_type_json` undefined (or, if you add it in Step 1, the wiring below is still absent; write the test first, then the function).

- [ ] **Step 3: Wire the raw-JSON descriptor through ExportMeta**

`export_type_kind` was flat-string only. Replace its use in `collect_export_meta` with `export_type_json`, and store raw JSON. Change `collect_export_meta` (codegen.tw):

```tw
fn collect_export_meta(anf: AnfModule) Vector<ExportMeta> {
  collect exp in anf.lib_exports {
    kind := case exp.target {
      .Function(_) => "fn",
      .Value(_) => "value",
    }
    ExportMeta.{
      name: exp.name,
      wasm_name: exp.wasm_name,
      kind,
      args: collect p in exp.params {
        export_type_json(p)
      },
      ret: export_type_json(exp.ret),
    }
  }
}
```

Delete `export_type_kind` (now unused) — this also removes the last non-recursive `case LibType`.

In `wasm.tw`, change `export_meta_json` to emit `args`/`ret` **raw** (they are already JSON values):

```tw
fn export_meta_json(meta: Vector<ExportMeta>) String {
  entries := collect em in meta {
    "{\"name\":${json_string(em.name)},\"wasmName\":${json_string(em.wasm_name)},\"kind\":${json_string(
      em.kind,
    )},\"args\":[${em.args.join(",")}],\"ret\":${em.ret}}"
  }
  "[${entries.join(",")}]"
}
```

`ExportMeta` field types are unchanged (`args: Vector<String>`, `ret: String`) — the strings now hold raw JSON values. Update the doc comment on `ExportMeta` (wasm_ir.tw) to say so.

- [ ] **Step 4: Run to verify pass**

Run: `TWK_TEST_FILTER="export_type_json" target/twk run boot/tests/main.tw` → PASS.
Run: `target/twk run boot/tests/main.tw` → full suite PASS (confirms the `export_meta_json` change didn't break the Increment-1 primitive-tag JSON: `.Str`→`"str"` raw is identical to the old quoted form).

- [ ] **Step 5: Commit**

```bash
git add boot/compiler/codegen/codegen.tw boot/compiler/codegen/wasm.tw boot/compiler/codegen/wasm_ir.tw boot/tests/suites/codegen_emit_suite.tw
git commit -m "Emit nested fn descriptors in twinkle.exports section"
```

---

### Task 3: Codegen — trampoline + host import + constructor per callback signature

**Files:**
- Modify: `boot/compiler/codegen/emit.tw` (`emit_lib_exports` ~374; the import/func assembly ~280–305)
- Test: driven end-to-end by Task 4 (the JS round-trip); add a boot assertion that the constructor export + import appear in the emitted module.

Emit, per distinct callback signature `S` appearing in a lib export's `Fn` params, three artifacts. Signature key: `sig_key(params, ret)` = join of native tags, e.g. `str__void`, `i64_bool`. Native tags map `LibType` → wasm `ValType`:
`Int→.I64`, `Float→.F64`, `Bool→.I32`, `Void→(no result)`, `Str→.Ref(true,.Named("rt_types__String"))`.

**(a) Host import** `__lib_cb_<key>`:
```tw
ImportDef.{
  module: "twinkle.lib",
  name: "cb_${key}",
  as_sym: "__lib_cb_${key}",
  params: [.I32].concat(native_param_val_types),   // cbid + native args
  results: native_result_val_types,                // [] for Void
}
```

**(b) Trampoline** `__lib_cb_tramp_<key>` of the universal type. Params `(env: .Anyref, args: .Anyref)`, result `.Anyref`. Body:
- push `cbid`: `LocalGet(env)`, `.RefCast(false, .I31)`, `.I31GetS` → i32.
- for each arg index `i`: `LocalGet(args)`, `.RefCast(false, .Named("rt_types__Array"))`, `.I32Const(i)`, `.ArrayGet("rt_types__Array")`, then `emit_unbox_from_anyref(param_mono_i, env, buf)` to native.
- `.Call("__lib_cb_${key}")`.
- box the result: for a value return, `emit_box_to_anyref(ret_mono, env, buf)`; for `Void`, push `.I32Const(0)`, `.RefI31` (unit anyref).

`emit_unbox_from_anyref`/`emit_box_to_anyref` (from `emit/anyref.tw`) need a `MonoType` and `env`. Reconstruct the callback's `MonoType`s from the `LibType` via a helper `lib_type_to_mono(t) MonoType` (Int→.Int, Float→.Float, Bool→.Bool, Void→.Void, Str→.String; `Fn` won't recurse here — nested fns are ineligible). Thread `env: ResolvedEnv` into `emit_lib_exports` (it is available at the call site in `emit_wasm_module`).

**(c) Constructor export** `__lib_make_cb_<key>(cbid: i32) → anyref`. Params `[.I32]`, results `[.Anyref]`. Body builds the generic 2-field closure:
- `.RefFunc("__lib_cb_tramp_${key}")`  (field 0, universal funcref)
- `.LocalGet(0)`, `.RefI31`  (field 1, env = boxed cbid)
- `.StructNew("rt_types__Closure")`
Add an `ExportDef.{ wasm_name: "__lib_make_cb_${key}", func_sym: "__lib_make_cb_${key}" }`.

- [ ] **Step 1: Extend `emit_lib_exports` return type**

Change `LibEmitResult` to also carry imports:
```tw
type LibEmitResult = .{ funcs: Vector<FuncDef>, exports: Vector<ExportDef>, imports: Vector<ImportDef> }
```
and its signature to `emit_lib_exports(anf, reg, func_sym_map, env)`.

- [ ] **Step 2: Collect distinct signatures and emit the three artifacts**

Inside `emit_lib_exports`, before the existing `for exp in anf.lib_exports` loop, gather distinct `Fn` signatures across all export params into a `Dict<String, LibType>` keyed by `sig_key`. For each, append the import (b→list), trampoline func, and constructor func + export as specified above. Return them in the extended result.

- [ ] **Step 3: Merge imports at the call site**

In `emit_wasm_module` (~280): pass `env` to `emit_lib_exports`; after `lib_result`, add `extern_imports = extern_imports.concat(lib_result.imports)` (place after the `extern_imports` loop builds its base list, before `runtime_imports`). Ensure `funcs = .concat(lib_result.funcs)` and `user_exports` include `lib_result.exports` (already wired).

- [ ] **Step 4: Boot assertion the artifacts are emitted**

In `codegen_emit_suite.tw`, compile a lib source with a callback param via the wasm-emitting path and assert the WAT contains the constructor export and import names:
```tw
    .test(
      "callback lib export emits constructor + host import",
      fn() {
        src := "pub fn each(name: String, f: fn(String) Void) Void {\n  f(name)\n}\n"
        artifacts := try pipeline.compile_source_lib(src)
        wat := codegen.codegen(artifacts.anf, artifacts.env, artifacts.builtins)
        try assert.str_contains(wat, "__lib_make_cb_str__void")
        try assert.str_contains(wat, "cb_str__void")
        .Ok({})
      },
    )
```
(Confirm `compile_source_lib`'s artifacts expose `env`/`builtins`; if not, use the existing wasm-emit test helper in the suite.)

- [ ] **Step 5: Run to verify fail → implement → pass**

Run: `TWK_TEST_FILTER="callback lib export emits" target/twk run boot/tests/main.tw`
Expected: FAIL first (names absent), PASS after Steps 1–3. Then full suite: `target/twk run boot/tests/main.tw` → PASS.

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/codegen/emit.tw boot/tests/suites/codegen_emit_suite.tw
git commit -m "Emit callback trampoline, host import, and closure constructor for lib exports"
```

---

### Task 4: Runtime — callback registry + `fn`-arg marshalling + rebuild

**Files:**
- Modify: `tools/js_runtime/runtime.mjs` (`coerceLibArg`/`coerceLibReturn` ~1160; `loadLibBytes` loop ~1206; add import provision)
- Test: `tools/js_runtime/runtime.test.mjs`

- [ ] **Step 1: Write the failing JS test**

```js
test("loadLib drives host callbacks (Void and value-returning)", async () => {
  const src = [
    "pub fn each_word(text: String, f: fn(String) Void) Void {",
    "  for w in text.split(\" \") {",
    "    f(w)",
    "  }",
    "}",
    "",
    "pub fn transform(n: Int, f: fn(Int) Int) Int {",
    "  f(n)",
    "}",
  ].join("\n");
  const wasm = await compile({ source: src }, { lib: true });
  const lib = await loadLib(wasm);

  const seen = [];
  lib.each_word("a b c", (w) => seen.push(w));
  assert.deepEqual(seen, ["a", "b", "c"]);

  assert.equal(lib.transform(21n, (n) => n * 2n), 42n);
});
```
(Confirm `String.split` exists in the prelude; if not, use a fixed two-line `text` and a helper that calls `f` twice.)

- [ ] **Step 2: Provide the callback imports before instantiation**

The lib imports `twinkle.lib.cb_<key>`. In `loadLibBytes`, before `instantiateWithExternRetry`, build the import object from `readExportMeta`'s descriptors: for every `fn` descriptor found in any export's `args`, add `hostImports["twinkle.lib"]["cb_" + key(desc)]` = a function `(cbid, ...nativeArgs) => { … }` that:
- looks up `registry.get(cbid)`,
- marshals each native arg → JS via `coerceLibReturn(nativeArgs[i], desc.args[i], b)` (guest→JS uses the *return* coercer direction: e.g. a guest `String` ref → JS string, a `BigInt` stays),
- calls the JS fn, then marshals its result JS → guest via `coerceLibArg(result, desc.ret, b)` (JS→guest direction).

`key(desc)` mirrors the boot `sig_key`: join native tags of `desc.args` then `_` + ret tag (or `void`). Add a helper `cbKey(desc)` in runtime.mjs.

- [ ] **Step 3: Marshal `fn` args in the export wrapper**

Extend `coerceLibArg` to handle an object descriptor:
```js
function coerceLibArg(value, kind, b, instance, registry) {
  if (kind && typeof kind === "object" && kind.kind === "fn") {
    const cbid = registry.add(value);
    return instance.exports["__lib_make_cb_" + cbKey(kind)](cbid);
  }
  switch (kind) { /* …existing int/float/bool/void/str… */ }
}
```
`registry` is a small `{ next: 1, map: new Map(), add(fn){…}, get(id){…}, drop(id){…} }`. In the export wrapper, allocate ids before the call and `registry.drop` them after the top-level export returns (wrap the `fn(...coerced)` call in try/finally to drop). Thread `instance` and `registry` into the wrapper closure created in the `loadLibBytes` export loop.

- [ ] **Step 4: Rebuild the compiler and stage boot.wasm, then run**

Run:
```bash
make stage2 && cp target/boot.wasm tools/js_runtime/boot.wasm
node --test --test-name-pattern="host callbacks" tools/js_runtime/runtime.test.mjs
```
Expected: FAIL before Steps 2–3, PASS after. Then the whole JS suite: `node --test tools/js_runtime/*.test.mjs`.

- [ ] **Step 5: Commit**

```bash
git add tools/js_runtime/runtime.mjs tools/js_runtime/runtime.test.mjs
git commit -m "loadLib: drive host callbacks for fn-typed lib params"
```

---

### Task 5: Full verification + docs

- [ ] **Step 1: Full suites**

Run: `target/twk run boot/tests/main.tw` (boot), `node --test tools/js_runtime/*.test.mjs` (JS). Both green.

- [ ] **Step 2: End-to-end smoke**

Build a scratch lib with a callback export and `loadLib` it via a one-off `node --input-type=module` script (mirror the Increment-1 smoke). Confirm the callback fires and a value-returning callback round-trips.

- [ ] **Step 3: fmt + lint**

Run: `target/twk fmt <each edited .tw>` and `target/twk lint boot/main.tw` (expect "No findings").

- [ ] **Step 4: Mark Increment 2 shipped**

In `docs/plans/lib-export-abi.md`, change the Increment 2 heading to `### 2. Function-typed (callback) parameters — **shipped**` and add a one-line summary mirroring Increment 1's.

- [ ] **Step 5: Commit**

```bash
git add docs/plans/lib-export-abi.md
git commit -m "docs(plans): mark lib-export ABI Increment 2 shipped"
```

---

## Self-Review Notes

- **Spec coverage:** LibType.Fn (Task 1), nested JSON descriptor (Task 2), per-signature trampoline/import/constructor (Task 3), runtime registry + marshalling (Task 4), verification/docs (Task 5) — covers every bullet of Increment 2 in lib-export-abi.md.
- **Marshalling reuse:** the host import is native-typed, so the JS side reuses `coerceLibArg`/`coerceLibReturn` (Increment 1) with the direction flipped per boundary crossing — guest→JS on the way in to the callback, JS→guest on the way out. No new bridge ops.
- **Reentrancy:** a monotonic id + Map registry, with ids dropped in a `finally` after the top-level export returns, keeps nested lib calls safe.
- **Open confirmations for the implementer (resolve during Task, not blocking):** `ok_or_none()` availability (Task 1 has an inline fallback); whether `compile_source_lib` artifacts expose `env`/`builtins` for the WAT assertion (Task 3 Step 4) — use the suite's existing wasm-emit helper if not; prelude `String.split` (Task 4 Step 1 has a fallback).
