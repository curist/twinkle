# Boot Compiler — Builtin Function Registry

Last updated: 2026-03-23

## Problem

The boot compiler hardcodes builtin FuncIds as magic numbers scattered
across two files:

- `lower_core.tw`: 19 hardcoded `FuncId.{ id: N }` references for
  language-level desugaring (string interpolation, for-loop, collect,
  index-assign)
- `opt/pipeline.tw`: 12 hardcoded IDs in `make_prelude_cow_config`
  for the uniqueness optimization pass (8 as `FuncId.{ id: N }`
  literals, 4 as plain integer keys into `fresh_producer_ids` /
  `read_only_ids`)

These numbers must match stage0's `src/ir/lower.rs` constants exactly.
User functions start at `next_func: 41` (USER_FUNC_START), which must
be bumped whenever a new builtin is added in the 0–40 range. The 1000+
range (byte ops, host ops, in-place variants) is a separate ad-hoc
allocation that could silently collide with user FuncIds in a large
program.

## Goal

Centralize all builtin knowledge into a single registry that carries
both FuncId assignments and dispatch metadata (runtime import vs
compiler intrinsic). The boot compiler's internal numbering becomes
fully decoupled from stage0 — Phase D reads the registry to emit the
correct Wasm imports, with no second mapping needed.

## Design

### Types

```tw
// How a builtin is implemented at the Wasm level
pub type BuiltinKind = {
  // Emitted as a Wasm import: (import "rt.arr" "len" (func ...))
  Runtime(RuntimeInfo),
  // Compiler emits inline Wasm instructions (no import)
  Intrinsic,
}

pub type RuntimeInfo = .{
  wasm_module: String,  // "rt.core", "rt.arr", "rt.str", "rt.dict", "host"
  wasm_name: String,    // "len", "concat", "print", etc.
}

pub type BuiltinEntry = .{
  name: String,
  func_id: FuncId,
  kind: BuiltinKind,
}

pub type BuiltinRegistry = .{
  by_name: Dict<String, BuiltinEntry>,
  by_id: Dict<Int, BuiltinEntry>,
  entries: Vector<BuiltinEntry>,
  next_id: Int,
}
```

`entries` preserves registration order for Phase D to iterate when
emitting imports. `by_name` provides O(1) lookup for the lowerer and
optimizer. `by_id` provides O(1) reverse lookup from FuncId for Phase D
to distinguish runtime imports from intrinsics when emitting calls.

### Registration helpers

```tw
pub fn runtime(reg: BuiltinRegistry, name: String,
               wasm_module: String, wasm_name: String) BuiltinRegistry {
  fid := FuncId.{ id: reg.next_id }
  entry := BuiltinEntry.{
    name: name,
    func_id: fid,
    kind: .Runtime(.{ wasm_module: wasm_module, wasm_name: wasm_name }),
  }
  by_name := reg.by_name
  by_id := reg.by_id
  by_name[name] = entry
  by_id[fid.id] = entry
  .{ by_name: by_name, by_id: by_id, entries: reg.entries.push(entry), next_id: reg.next_id + 1 }
}

pub fn intrinsic(reg: BuiltinRegistry, name: String) BuiltinRegistry {
  fid := FuncId.{ id: reg.next_id }
  entry := BuiltinEntry.{ name: name, func_id: fid, kind: .Intrinsic }
  by_name := reg.by_name
  by_id := reg.by_id
  by_name[name] = entry
  by_id[fid.id] = entry
  .{ by_name: by_name, by_id: by_id, entries: reg.entries.push(entry), next_id: reg.next_id + 1 }
}

pub fn id(reg: BuiltinRegistry, name: String) FuncId {
  case reg.by_name[name] {
    .Some(entry) => entry.func_id,
    .None => error("unknown builtin: " + name),
  }
}

pub fn entry(reg: BuiltinRegistry, func_id: FuncId) BuiltinEntry? {
  reg.by_id[func_id.id]
}
```

Both `runtime` and `intrinsic` return the updated registry for
chaining.

### Registration

One function builds the full registry. Dispatch metadata is declared
inline — no separate mapping needed later.

```tw
pub fn make_builtin_registry() BuiltinRegistry {
  BuiltinRegistry.{ by_name: Dict.new(), by_id: Dict.new(), entries: [], next_id: 0 }
    // ── I/O ──
    .runtime("print",   "rt.core", "print")
    .runtime("println", "rt.core", "println")
    .runtime("error",   "rt.core", "trap")
    .runtime("eprint",  "rt.core", "eprint")
    .runtime("eprintln","rt.core", "eprintln")
    // ── to_string family ──
    .runtime("int_to_string",    "rt.str", "from_i64")
    .runtime("float_to_string",  "rt.str", "from_f64")
    .runtime("bool_to_string",   "rt.str", "from_bool")
    .intrinsic("string_to_string")
    // ── String ops ──
    .runtime("string_len",    "rt.str", "len")
    .runtime("string_concat", "rt.str", "concat")
    .runtime("string_substring", "rt.str", "substring")
    .intrinsic("string_get")
    .intrinsic("string_slice")
    .intrinsic("from_code_point")
    .intrinsic("string_utf8_bytes")
    .intrinsic("string_from_utf8")
    .intrinsic("char_code_at")
    .intrinsic("from_char_code")
    // ── Vector ops ──
    .runtime("vector_len",       "rt.arr", "len")
    .intrinsic("vector_push")
    .runtime("vector_set_unsafe","rt.arr", "set")
    .runtime("vector_concat",    "rt.arr", "concat")
    .runtime("vector_slice",     "rt.arr", "slice")
    .intrinsic("vector_get")
    .intrinsic("vector_set")
    .intrinsic("vector_make")
    // ── Vector builder ──
    .runtime("vector_builder_new",    "rt.arr", "builder_new")
    .runtime("vector_builder_push",   "rt.arr", "builder_push")
    .runtime("vector_builder_freeze", "rt.arr", "builder_freeze")
    .runtime("vector_builder_from",   "rt.arr", "builder_from")
    // ── Dict ops ──
    .runtime("dict_new",    "rt.dict", "make")
    .runtime("dict_set",    "rt.dict", "set")
    .runtime("dict_keys",   "rt.dict", "keys")
    .runtime("dict_get",    "rt.dict", "get_option")
    .intrinsic("dict_get_unsafe")
    .runtime("dict_len",    "rt.dict", "len")
    .runtime("dict_has",    "rt.dict", "has")
    .runtime("dict_remove", "rt.dict", "remove")
    // ── In-place variants (optimizer rewrites) ──
    .intrinsic("vector_set_in_place")
    .runtime("dict_set_in_place",    "rt.dict", "set_in_place")
    .runtime("dict_remove_in_place", "rt.dict", "remove_in_place")
    // ── Range & iterators ──
    .intrinsic("range_from")
    .intrinsic("range")
    .intrinsic("range_step")
    .intrinsic("iterator_next")
    .intrinsic("iterator_unfold")
    // ── Cell ──
    .intrinsic("cell_new")
    .intrinsic("cell_get")
    .intrinsic("cell_set")
    .intrinsic("cell_update")
    // ── Byte ops ──
    .intrinsic("byte_to_int")
    .intrinsic("byte_from_int")
    .intrinsic("byte_to_string")
    // ── Numeric parsing ──
    .intrinsic("int_from_string")
    .intrinsic("float_from_string")
    // ── Host ops ──
    .runtime("host_read_file",  "host", "read_file")
    .runtime("host_write_file", "host", "write_file")
    .runtime("host_write_bytes","host", "write_bytes")
    .runtime("host_mkdirp",     "host", "mkdirp")
    .runtime("host_list_dir",   "host", "list_dir")
    .runtime("host_exists",     "host", "exists")
    .runtime("host_args",       "host", "args")
    .runtime("host_env",        "host", "env")
    .runtime("host_cwd",        "host", "cwd")
    .runtime("host_exit",       "host", "exit")
}
```

Adding a new builtin = one `.runtime(...)` or `.intrinsic(...)` line.
The dispatch metadata travels with the entry from registration through
emission. No magic numbers, no collision risk, no USER_FUNC_START to
bump.

### What changes per file

**`boot/compiler/builtins.tw` (new)**

The types (`BuiltinKind`, `RuntimeInfo`, `BuiltinEntry`,
`BuiltinRegistry`), the registration helpers (`runtime`, `intrinsic`,
`id`), and `make_builtin_registry`.

**`boot/compiler/lower_core.tw`**

1. `LowerCtx` gains a `builtins: BuiltinRegistry` field.

2. `new_ctx` takes a `BuiltinRegistry`, seeds `func_table` with the
   builtin name→FuncId mappings, and sets `next_func` to
   `builtins.next_id`:

   ```tw
   fn new_ctx(check_result: CheckResult, builtins: BuiltinRegistry) LowerCtx {
     func_table: Dict<String, FuncId> = Dict.new()
     for entry in builtins.entries {
       func_table[entry.name] = entry.func_id
     }
     .{
       func_table: func_table,
       next_func: builtins.next_id,
       builtins: builtins,
       ...
     }
   }
   ```

   This also means method calls resolved by the checker (via
   `MethodCallInfo.func_name`) will find builtin functions in
   `func_table` automatically — no special-casing needed.

3. All 19 hardcoded `FuncId.{ id: N }` become `ctx.builtins.id("name")`:

   ```tw
   // Before:
   CoreExpr.{ kind: .GlobalFunc(FuncId.{ id: 9 }), ... }  // string_concat

   // After:
   CoreExpr.{ kind: .GlobalFunc(ctx.builtins.id("string_concat")), ... }
   ```

   Affected sites (by desugaring category):
   - **String interpolation** (6 sites): `int_to_string`,
     `float_to_string`, `bool_to_string`, `byte_to_string`,
     `string_to_string`, `string_concat`
   - **For-loop desugaring** (5 sites): `string_len`, `dict_keys`,
     `vector_len` (×3)
   - **Collect desugaring** (6 sites): `vector_builder_new` (×2),
     `vector_builder_push` (×2), `vector_builder_freeze` (×2)
   - **Index-assign** (2 sites): `dict_set`, `vector_set_unsafe`

**`boot/compiler/opt/pipeline.tw`**

`make_prelude_cow_config` takes a `BuiltinRegistry` parameter instead
of hardcoding FuncIds:

```tw
pub fn make_prelude_cow_config(builtins: BuiltinRegistry) CowConfig {
  cow_ops: Dict<Int, CowOpEntry> = Dict.new()
  cow_ops[builtins.id("vector_set_unsafe").id] =
    CowOpEntry.{ base_arg: 0, in_place_id: .Some(builtins.id("vector_set_in_place")) }
  cow_ops[builtins.id("dict_set").id] =
    CowOpEntry.{ base_arg: 0, in_place_id: .Some(builtins.id("dict_set_in_place")) }
  // ...
  CowConfig.{
    cow_ops: cow_ops,
    builder: BuilderConfig.{
      push_id: builtins.id("vector_push"),
      builder_new_id: builtins.id("vector_builder_new"),
      // ...
    },
    // ...
  }
}
```

**`boot/compiler/opt/*` passes, `anf.tw`, `core_ir.tw`**

No changes. The optimizer passes are FuncId-agnostic — they receive
`CowConfig` which already abstracts the FuncIds. The IR types stay
the same.

### Phase D: WAT emission

Phase D iterates `builtins.entries` to emit Wasm imports and handle
intrinsics. No second mapping is needed — the registry already carries
everything:

```tw
fn emit_imports(builtins: BuiltinRegistry) Vector<WasmImport> {
  imports: Vector<WasmImport> = []
  for entry in builtins.entries {
    case entry.kind {
      .Runtime(info) => {
        imports = imports.push(WasmImport.{
          module: info.wasm_module,
          name: info.wasm_name,
          func_id: entry.func_id,
        })
      },
      .Intrinsic => {},  // handled inline by the emitter
    }
  }
  imports
}

fn emit_call(func_id: FuncId, builtins: BuiltinRegistry, ...) {
  case builtins.entry(func_id) {
    .Some(entry) => case entry.kind {
      .Runtime(_) => emit_wasm_call(func_id),      // (call $rt_arr__len)
      .Intrinsic => emit_inline_intrinsic(entry),   // inline wasm instructions
    },
    .None => emit_wasm_call(func_id),  // user function
  }
}
```

## Milestones

### M1: Create `builtins.tw` with `BuiltinRegistry`

Define the types (`BuiltinKind`, `RuntimeInfo`, `BuiltinEntry`,
`BuiltinRegistry`), the registration helpers (`runtime`, `intrinsic`,
`id`, `entry`), and `make_builtin_registry` with the full builtin list.

Add tests verifying: auto-incrementing IDs, name lookup, runtime vs
intrinsic dispatch kind is correct for known entries.

### M2: Thread `BuiltinRegistry` through `lower_core.tw`

Add `builtins` field to `LowerCtx`. Update `new_ctx` to accept and
seed from the registry. Replace all 19 hardcoded `FuncId.{ id: N }`
with `ctx.builtins.id("name")`. Update `next_func` to start from
`builtins.next_id`.

### M3: Update `pipeline.tw`

Change `make_prelude_cow_config` to take `BuiltinRegistry`. Update
`optimize_module` to pass it through. All 12 hardcoded IDs in
the COW config become registry lookups (8 `FuncId.{ id: N }` literals
and 4 plain integer keys).

### M4: Update callers and tests

Update the compilation entry point to create a `BuiltinRegistry` and
pass it to `new_ctx`. Verify all existing tests still pass — the
actual FuncId numbers will change (from stage0's convention to
auto-incremented), but the boot compiler's IR and optimizer don't
depend on specific numbers.

## Risks

**Name typos:** `builtins.id("vetor_len")` would trap at runtime
instead of failing at compile time. Mitigation: the existing test
suite exercises all desugaring paths, so a typo would surface as a
test failure immediately.

**Test FuncId assertions:** Any tests that assert specific FuncId
numbers (e.g., `assert.int_eq(fid.id, 12)`) will break because the
auto-assigned IDs differ from the old hardcoded ones. These assertions
should be updated to use registry lookups or removed in favor of
structural checks.

**Phase D dependency:** Phase D must iterate `builtins.entries` to
emit Wasm imports and route intrinsic calls. This is straightforward —
the registry already carries all the metadata Phase D needs.
