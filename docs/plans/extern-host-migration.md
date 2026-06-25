# Extern Host Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the magic `__host_*` builtin path with declarative `extern twinkle_runtime { … }` declarations everywhere, deleting the per-function compiler wiring and its `emit.rs`/`context.tw` special-casing.

**Architecture:** The `extern` mechanism already covers String/Int/Float/Bool/ExternRef host calls declaratively (see `boot/stdlib/math.tw`, the buffer ops in `boot/stdlib/fs.tw`). The only thing it cannot express today is the `Vector<Byte>` / `Vector<String>` boundary, which the `__host_*` path handles with dedicated `rt.arr` (`from_array`/`to_array`/`from_read_file_result`) conversions driven by hardcoded magic-name/FuncId predicates. We first **generalize extern to marshal vectors** (Phase 0), then migrate every host fn to `extern twinkle_runtime` (Phases 1–2), then **delete** the magic path from both compilers and the runtime (Phase 3).

**Tech Stack:** Boot compiler (`boot/compiler/**.tw`, self-hosted Twinkle), Rust stage0 (`src/**`), JS runtime (`tools/js_runtime/runtime.mjs`).

**Namespace decision:** the new extern module is the plain identifier `twinkle_runtime` (no quoted/alias syntax — that's speculative generality for an internal-only namespace; see plan discussion). It also becomes the import-module key in `makeHostImports` (replacing `host`). `extern Math` is unchanged (it is globalThis-backed, semantically distinct).

---

## Background: the machinery being changed

Read these before starting; tasks reference them.

**Two host-call mechanisms today:**

1. **Magic `__host_*`** — registered in boot `boot/compiler/base_env.tw` (as both `host_x` and `__host_x`), aliased/stripped in `boot/compiler/lower_core/context.tw:44` and `:136`, and in stage0 `src/intrinsics/registry.rs` (`INTRINSIC_SPECS` + `COMMON_BOOTSTRAP_FUNC_NAMES`), `src/ir/lower.rs` (FuncId constants), `src/types/env.rs` (builtin types), `src/codegen/emit.rs` (vector boundary). Emits wasm imports into module `host` with the bare name (`host.read_file`).

2. **`extern` blocks** — parsed in `boot/compiler/parser.tw:1210` (`parse_extern`), carried as `ExternImport` (`boot/compiler/core_ir.tw:110`), emitted as wasm imports in `boot/compiler/codegen/emit.tw:254-272`, with FFI metadata in the `twinkle.externs` custom section built by `collect_extern_meta` (`boot/compiler/codegen/codegen.tw:194`) using `mono_type_kind` (`codegen.tw:179`). The runtime auto-bridges via `bridgeExternImports` / `makeMarshalArgs` / `marshalReturn` (`tools/js_runtime/runtime.mjs:221`).

**Vector boundary (the gap):** `mono_type_kind` maps `Vector<Byte>` → `"ref"` (fallthrough), and `emit.tw:256` maps the param ValType via `val_type_of_mono`, which yields `$PVec` (`rt_types__PVec`). The host side needs the flat `$Array` (`rt_types__Array`). The magic path bridges with `rt_arr__to_array` (PVec→Array, before call) and `rt_arr__from_array` (Array→PVec, after call); `read_file`'s `Result<Vector<Byte>,String>` uses `rt_arr__from_read_file_result`. Boot inserts these inline (see `emit_intrinsic_string_from_utf8` at `boot/compiler/codegen/emit.tw:2106` for the exact pattern); stage0 drives them from FuncId predicates `is_host_vector_arg` / `has_host_vector_args` / `is_host_vector_returning` / `is_host_read_file` (`src/codegen/emit.rs:7797-7828`).

**Host fns and their shapes** (from `base_env.tw:426` + `registry.rs:519`):

| Fits extern today (String/Int/Float/Bool/Void) | Needs vector marshalling |
|---|---|
| `write_file(String,String)→Void`, `mkdirp(String)→Void`, `exists(String)→Bool`, `stdin_eof()→Bool`, `cwd()→String`, `exit(Int)→Never`, `now()→Float`, `sleep(Int)→Void` | `read_file(String)→Result<Vector<Byte>,String>`, `write_bytes(String,Vector<Byte>)→Void`, `stdin_read_chunk(Int)→Vector<Byte>`, `stdin_read_timeout(Int,Int)→Vector<Byte>`, `stdout_write_bytes(Vector<Byte>)→Void`, `list_dir(String)→Vector<String>`, `args()→Vector<String>`, `env(String)→Vector<String>`, `run_wasm(Vector<Byte>,Vector<Byte>)→…` |

That's **17 host fns** total (8 primitive + 9 vector). `stdin_eof` (`__host_stdin_eof`, `registry.rs:552`) is a primitive Bool but lives in `io.tw`, so it's migrated alongside the io vector fns in Task 2.1.

`run_wasm` is the boot compiler's own self-test harness import; treat it like the other `Vector<Byte>` params. `parse_int`/`parse_float` (module `host`, `emit.rs:6593`) are pure-Wasm helpers, **not** part of this migration — leave them, but note they currently sit under import module `host`; Phase 3 must keep that working when `host` is otherwise retired (Task 3.4).

---

## File Structure

- `tools/js_runtime/runtime.mjs` — extern bridge gains `bytes`/`strvec` marshal kinds; `makeHostImports` host object re-keyed to `twinkle_runtime`.
- `boot/compiler/codegen/codegen.tw` — `mono_type_kind` gains vector kinds.
- `boot/compiler/codegen/emit.tw` — extern import wasm signatures map vectors to `$Array`; extern call sites insert `rt.arr` conversions.
- `src/codegen/emit.rs` — extern import emission learns the vector boundary from the `ExternImport` signature (generalizing the FuncId-keyed predicates); magic predicates deleted in Phase 3.
- `boot/stdlib/{io,fs,proc,time,date}.tw` — host calls become `extern twinkle_runtime` declarations.
- `boot/compiler/base_env.tw`, `boot/compiler/lower_core/context.tw` — magic registration + prefix juggling deleted in Phase 3.
- `src/intrinsics/registry.rs`, `src/ir/lower.rs`, `src/types/env.rs` — magic host specs/constants/types deleted in Phase 3.

---

## Phase 0 — Generalize extern vector marshalling

This is a self-contained capability with its own test surface. Ship it before touching any stdlib.

### Task 0.1: Runtime bridge marshals byte/string vectors

**Files:**
- Modify: `tools/js_runtime/runtime.mjs:221-300` (`bridgeExternImports`, `makeMarshalArgs`, `marshalReturn`)
- Reference (existing array codec): the `decodeByteArray` helper used by the `host` object (`runtime.mjs`, search `decodeByteArray`) and the string-array building used by `args`/`list_dir` (search `list_dir` in `makeHostImports`).

- [ ] **Step 1: Add a failing round-trip test against a real instance**

The byte/string codec runs **through wasm linear memory**: `decodeByteArray(b, arrRef)` (`runtime.mjs:110`) calls `b.array_len()`, `b.bulk_bytes_read()`, and reads `b.memory.buffer`; `decodeStringArray` calls `b.array_get`/`decodeString`. A plain JS fake object exercises none of that, so a "unit test" with a `{ bytes: [...] }` fixture would be green and prove nothing. Test the marshalling the only way that's real: round-trip through an actual compiled module.

The end-to-end probe in **Task 0.5** already does this (read a file, get bytes back). For a tighter Phase-0 regression, add an `echo_bytes`-style fixture: compile `extern twinkle_runtime { fn echo_bytes(xs: Vector<Byte>) Vector<Byte> }` with a host `echo_bytes` that returns its argument, instantiate via the normal `run()` path, and assert the bytes survive the trip. Put it where `runtime.mjs` is already exercised against real modules (search `tools/js_runtime/` for an existing `*.test.mjs` that instantiates a module; if none, the Task 0.5 probe is the authoritative test and this step folds into it).

Do **not** assert against the closures directly: `makeMarshalArgs`/`marshalReturn` are locals inside `bridgeExternImports` (`runtime.mjs:241,252`) capturing `b`. If you want a unit-level seam, that's a real refactor (extract `b`-parametrized, exported helpers) — only do it if Step 3 needs the extraction anyway; otherwise rely on the round-trip.

- [ ] **Step 2: Run it, confirm it fails**

Run the round-trip fixture (e.g. `node --test tools/js_runtime/extern_vec.test.mjs`, or the Task 0.5 probe).
Expected: FAIL — the `bytes`/`strvec` kinds do not exist yet, so the bridge falls through to `decodeString` and corrupts/throws on the `$Array` ref.

- [ ] **Step 3: Implement the kinds in the bridge**

In `makeMarshalArgs`, before the `decodeString` fallback, add:

```js
if (k === "bytes") return decodeByteArray(b, arg);          // $Array → Uint8Array
if (k === "strvec") return decodeStringArray(b, arg);       // $Array → string[]
```

In `marshalReturn`, add cases:

```js
case "bytes":  return encodeByteArray(b, result);           // Uint8Array → $Array
case "strvec": return encodeStringArray(b, result);         // string[] → $Array
```

Reuse the existing `$Array` codec: `decodeStringArray`/`decodeByteArray` are already module-level (`runtime.mjs:101,110`); add the encode direction (`encodeByteArray`/`encodeStringArray`) at module level too so both the bridge and the (still-present, until Phase 3) `host` object share them. `marshalReturn`'s `readfile` case is added in Step 3 of Task 0.2's runtime-side counterpart — see the read_file return contract in Task 2.2.

- [ ] **Step 4: Run the test, confirm pass**

Run: `node --test tools/js_runtime/extern_vec.test.mjs`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add tools/js_runtime/runtime.mjs tools/js_runtime/extern_vec.test.mjs
git commit -m "runtime: marshal byte/string vectors across the extern bridge"
```

### Task 0.2: Boot codegen emits the vector boundary for extern imports

**Files:**
- Modify: `boot/compiler/codegen/codegen.tw:179` (`mono_type_kind`)
- Modify: `boot/compiler/codegen/emit.tw:254-272` (extern import wasm signatures)
- Modify: `boot/compiler/codegen/emit.tw` (extern call-site emission — search the function that emits `Call($extern_…)`; mirror the `rt_arr__to_array`/`rt_arr__from_array` insertion shown at `emit.tw:2098-2120`)
- Test: `boot/tests/suites/codegen_emit_suite.tw`

- [ ] **Step 1: Add a failing codegen test**

In `boot/tests/suites/codegen_emit_suite.tw`, add a case compiling a module with:

```tw
extern twinkle_runtime {
  fn echo_bytes(xs: Vector<Byte>) Vector<Byte>
}
pub fn go(xs: Vector<Byte>) Vector<Byte> { twinkle_runtime.echo_bytes(xs) }
```

Assert the emitted WAT/binary: the import `twinkle_runtime.echo_bytes` has param/result `(ref $rt_types__Array)` (not `$PVec`), and the call site is wrapped `rt_arr__to_array` … `rt_arr__from_array`. (Match the assertion style already used in that suite — grep the suite for an existing import-signature assertion.)

- [ ] **Step 2: Run it, confirm it fails**

Run: `target/twk run boot/tests/suites/codegen_emit_suite.tw`
Expected: FAIL — extern param emits `$PVec`, no `rt.arr` wrap, kind is `ref`.

- [ ] **Step 3: Extend `mono_type_kind`**

```tw
fn mono_type_kind(ty: MonoType) String {
  case ty {
    .String => "str",
    .Int => "i64",
    .Float => "f64",
    .Bool => "i32",
    .Byte => "i32",
    .ExternRef(_) => "ref",
    .Optional(inner) => mono_type_kind(inner),
    .Vector(.Byte) => "bytes",
    .Vector(.String) => "strvec",
    _ => "ref",
  }
}
```

- [ ] **Step 4: Map vector params/results to `$Array` in the import signature**

At `emit.tw:255-262`, replace `val_type_of_mono(ty, env)` with a helper that returns `$Array` for `Vector<Byte>`/`Vector<String>` and `val_type_of_mono` otherwise:

```tw
fn extern_boundary_val_type(ty: MonoType, env: ResolvedEnv) ValType {
  case ty {
    .Vector(.Byte) => .Ref(true, .Named("rt_types__Array")),
    .Vector(.String) => .Ref(true, .Named("rt_types__Array")),
    _ => val_type_of_mono(ty, env),
  }
}
```

Use it for both `params` and `results`. For `read_file`'s `Result<Vector<Byte>,String>` return, the result type is a sum, not a bare `Vector`. The key fact: `rt_arr__from_read_file_result` runs **inside wasm, after the import returns** — it consumes the raw host `$Array` and *produces* the `Result`. So the import's wasm result ValType must be the **raw `$Array`** the host actually returns, NOT the `Result` variant type. (Don't "keep the import result as the sum/variant ValType" — that's the post-conversion shape, which lives only after the `from_read_file_result` call in Step 5.) Then in Step 5 the call site wraps the import result with `rt_arr__from_read_file_result` to rebuild the `Result`. See Task 2.2 for the JS-side return contract the host must satisfy.

Detecting this shape in `mono_type_kind` is more than a one-liner: the return is a named/builtin variant (`Result`) parameterized by `Vector<Byte>`/`String`, and today `mono_type_kind` (`codegen.tw:179`) has no case for named generics — it falls through to `_ => "ref"`. Add a dedicated `readfile` kind only when the return matches *exactly* `Result<Vector<Byte>, String>` (match the named variant + its type args); leave every other Result/named type on the general `ref` path. Keep this detection narrow — `readfile` is the one bespoke shape in the whole migration.

- [ ] **Step 5: Insert `rt.arr` conversions at the extern call site**

At the extern-call emission, for each arg whose `ext.param_tys[i]` is `Vector<Byte>`/`Vector<String>`, emit `.Call("rt_arr__to_array")` after pushing the atom (PVec→Array). For a `Vector<_>` return, emit `.Call("rt_arr__from_array")` after the call (Array→PVec). For the read-file Result return, emit `.Call("rt_arr__from_read_file_result")`. Ensure the `rt.arr` imports are registered when any extern needs them (they already exist for the magic path — see `wasm_plan_impl.tw:280-296`; make their registration depend on extern usage too, not only the magic FuncIds).

- [ ] **Step 6: Run the test, confirm pass**

Run: `target/twk run boot/tests/suites/codegen_emit_suite.tw`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add boot/compiler/codegen/codegen.tw boot/compiler/codegen/emit.tw boot/tests/suites/codegen_emit_suite.tw
git commit -m "boot codegen: marshal Vector<Byte>/Vector<String> over extern imports"
```

### Task 0.3: Allow vector types in extern signatures (typechecker)

**Files:**
- Modify: the extern signature validation (grep `boot/compiler/` for where extern param/return types are checked against an extern-safe whitelist — search `extern` in `boot/compiler/checker*` / `verify*` / `resolve*`; if no whitelist exists, vectors may already pass and this task is a no-op verification).
- Test: `boot/tests/suites/checker_suite.tw`

- [ ] **Step 1: Add a test** that an `extern twinkle_runtime { fn f(xs: Vector<Byte>) Vector<String> }` declaration typechecks with no diagnostic.
- [ ] **Step 2: Run, confirm pass-or-fail.** If it already passes (no whitelist), record that and skip to commit. If it fails on an "extern-safe type" error, add `Vector<Byte>`/`Vector<String>` to the allowed set.
- [ ] **Step 3: Commit** (only if a code change was needed).

```bash
git commit -am "boot checker: permit byte/string vectors in extern signatures"
```

### Task 0.4: stage0 emits the vector boundary from extern signatures

stage0 must be able to **compile** the migrated boot stdlib (Phases 1–2 put `Vector<Byte>` externs in boot source). Today stage0's vector boundary is FuncId-keyed (magic only). Generalize it to also fire for `ExternImport`s whose `param_tys`/`return_ty` are vectors.

**Files:**
- Modify: `src/codegen/emit.rs:334-470` and `:8956` (extern import emission)
- Reference: `src/codegen/emit.rs:7797-7828` (`is_host_vector_arg` etc.) and the `rt_arr__from_array`/`to_array`/`from_read_file_result` import helpers nearby.
- Test: `tests/` (add a Rust test compiling a boot snippet with a `Vector<Byte>` extern, or extend an existing emit test — search `tests/` for extern coverage).

- [ ] **Step 1: Add a failing Rust test** that stage0 compiles a module with `extern twinkle_runtime { fn echo_bytes(xs: Vector<Byte>) Vector<Byte> }` and the emitted import uses `$Array` param/result with `rt.arr` conversions at the call.

Run: `cargo test --release extern_vec_boundary -- --nocapture`
Expected: FAIL.

- [ ] **Step 2: In extern import emission, detect vector params/returns from `ext.param_tys`/`ext.return_ty`** (using a `MonoType::Vector(box Byte|String)` match) and:
  - declare the import param/result as `ref_array()` instead of `ref_pvec()`,
  - wrap the call with the existing `rt_arr` conversions (reuse `ensure_rt_arr_from_array_import` / the to_array import, and `from_read_file_result` for the read-file Result shape).

Factor a helper `extern_arg_is_host_vector(ty: &MonoType) -> Option<VecKind>` and route both the signature and the call-site wrapping through it. Keep the FuncId-keyed magic predicates for now (Phase 3 deletes them).

- [ ] **Step 3: Run the test, confirm pass.**
- [ ] **Step 4: Commit.**

```bash
git commit -am "stage0: marshal vector externs from signatures, not magic FuncIds"
```

### Task 0.5: End-to-end Phase 0 validation

- [ ] **Step 1:** Add a temporary `extern twinkle_runtime { fn read_file_raw(path: String) Vector<Byte>!String }`-style probe to a scratch `boot/repros/extern_vec_probe.tw` that reads a file via a new `twinkle_runtime` host entry, and wire a matching `twinkle_runtime` entry in `makeHostImports` (temporary; mirror the existing `read_file`).
- [ ] **Step 2:** Run it: `target/twk run boot/repros/extern_vec_probe.tw` — confirm bytes round-trip.
- [ ] **Step 3:** Run `make boot-test` and `make rust-test`; confirm green.
- [ ] **Step 4:** Remove the probe; commit Phase 0 close-out.

```bash
git rm boot/repros/extern_vec_probe.tw
git commit -am "extern: validate end-to-end vector marshalling (probe removed)"
```

---

## Phase 1 — Namespace + migrate the primitive host fns

### Task 1.1: Add the `twinkle_runtime` import module to the runtime

**Files:** `tools/js_runtime/runtime.mjs` (`makeHostImports`)

- [ ] **Step 1:** Add a `twinkle_runtime` object to the object returned by `makeHostImports`, initially aliasing the same functions the `host` object exposes (`write_file`, `mkdirp`, `exists`, `cwd`, `exit`, `now`, `sleep`, and later the vector ones). Keep `host` in place — both keys resolve during migration.

```js
return {
  host: { /* existing — unchanged this phase */ },
  twinkle_runtime: { write_file, mkdirp, exists, cwd, exit, now, sleep, /* … */ },
  Math: globalThis.Math,
};
```
(Define the fns once, reference from both objects.)

- [ ] **Step 2:** `make boot-test` — confirm nothing broke (no boot code uses `twinkle_runtime` yet).
- [ ] **Step 3:** Commit.

```bash
git commit -am "runtime: expose twinkle_runtime import module alongside host"
```

### Task 1.2: Migrate the primitive host fns in stdlib to extern

Do **one stdlib file per commit** so each is independently bisectable. Order: `time.tw`/`date.tw` (`now`, `sleep`), then `proc.tw` (`exit`, `cwd`, `env` is vector — defer to Phase 2), then `fs.tw` (`write_file`, `mkdirp`, `exists`), then `io.tw` primitive parts.

For each file:

- [ ] **Step 1:** Add an `extern twinkle_runtime { … }` block declaring the fns it uses, e.g. in `fs.tw`:

```tw
extern twinkle_runtime {
  fn write_file(path: String, text: String) Void
  fn mkdirp(path: String) Void
  fn exists(path: String) Bool
  fn read_buffer_len_raw(path: String) Int   // fold the existing extern host block in here
  fn read_buffer_raw(path: String, ptr: Int, len: Int) Int
  fn write_buffer_raw(path: String, ptr: Int, len: Int) Void
}
```

- [ ] **Step 2:** Replace `__host_write_file(...)` → `twinkle_runtime.write_file(...)`, `__host_mkdirp` → `twinkle_runtime.mkdirp`, `__host_exists` → `twinkle_runtime.exists`, and `host.read_buffer_raw` → `twinkle_runtime.read_buffer_raw` (retire the old `extern host` block — its three fns move into the `twinkle_runtime` block, and add their `twinkle_runtime` entries in `makeHostImports`).
- [ ] **Step 3:** `target/twk fmt <file>` then `target/twk lint <entry>`.
- [ ] **Step 4:** `make boot-test`. Expected: green.
- [ ] **Step 5:** Commit, e.g. `git commit -am "fs: call host via extern twinkle_runtime"`.

Repeat for each file. The corresponding `twinkle_runtime` entries must exist in `makeHostImports` (added in Task 1.1 / extended here).

### Task 1.3: Rebundle and full-suite checkpoint

- [ ] **Step 1:** `make bundle-cli` (rebuilds `target/boot.wasm` via self-host, then `target/twk`).
- [ ] **Step 2:** `make test` (boot + rust). Confirm green.
- [ ] **Step 3:** Commit any regenerated artifacts.

---

## Phase 2 — Migrate the vector-trafficking host fns

Depends on Phase 0. Same one-file-per-commit discipline.

### Task 2.1: Migrate byte/string-vector host fns to extern

**Files:** `boot/stdlib/{io,fs,proc}.tw`, `tools/js_runtime/runtime.mjs`

- [ ] **Step 1:** For each vector host fn, add it to the file's `extern twinkle_runtime { … }` block with its real Twinkle signature, e.g. in `io.tw`:

```tw
extern twinkle_runtime {
  fn stdin_read_chunk(max_bytes: Int) Vector<Byte>
  fn stdin_read_timeout(max_bytes: Int, timeout_ms: Int) Vector<Byte>
  fn stdin_eof() Bool
  fn stdout_write_bytes(bytes: Vector<Byte>) Void
}
```
and in `fs.tw`: `read_bytes`→ uses `read_file(path) Vector<Byte>!FsError`-shaped extern, `write_bytes`, `list_dir`; in `proc.tw`: `args`, `env`, `run_wasm`.

- [ ] **Step 2:** Replace each `__host_*` call with `twinkle_runtime.*`.
- [ ] **Step 3:** In `makeHostImports`, move the corresponding entries to the `twinkle_runtime` object (they already produce/consume `$Array` for the magic path — the extern bridge now does the marshalling via the `bytes`/`strvec`/`readfile` kinds, so the host fn bodies stay the same; verify the kind tags in the emitted `twinkle.externs` section match what the fn expects).
- [ ] **Step 4:** `target/twk fmt` + `lint`; `make boot-test`.
- [ ] **Step 5:** Commit per file.

### Task 2.2: read_file Result shape

`fs.read_bytes` currently calls `__host_read_file` returning `Result<Vector<Byte>,String>` via `rt_arr__from_read_file_result`. This is the **one bespoke shape** in the migration (the `readfile` kind from Task 0.2 Step 4). The extern path (Task 0.2 Step 4–5 + Task 0.4) must reproduce it for an `extern twinkle_runtime { fn read_file(path: String) Vector<Byte>!String }` declaration.

**JS return contract (pin this down before migrating).** The wasm import returns the **raw host `$Array`** shape that `rt_arr__from_read_file_result` decodes inside wasm — *not* a `Result` struct. So the host `read_file` fn keeps returning exactly what it returns today for the magic path, and the bridge's `marshalReturn` `case "readfile"` must encode it into that same `$Array` shape. Before writing code:
- **Step 0 (discovery):** Read the current `host.read_file` body in `makeHostImports` and `rt_arr__from_read_file_result` (boot `wasm_plan_impl.tw` / `rt.arr`, and stage0's `from_read_file_result` helper) to capture the exact `$Array` encoding of `Ok bytes` vs `Err msg` (e.g. a sentinel-prefixed byte array, or a tagged two-element array). Write the observed contract into a comment next to the `readfile` case so it isn't reverse-engineered twice.
- The `readfile` `marshalReturn` case then mirrors that encoding; the host fn body is unchanged from the magic path.

- [ ] **Step 1:** Migrate `read_bytes` to the extern form.
- [ ] **Step 2:** `make boot-test` — focus on `boot/tests/suites/stdlib_host_suite.tw`. Verify both the `Ok` (file present) and `Err` (missing file) arms round-trip — the `Err` path is the one the naive "encode a Uint8Array" reading misses.
- [ ] **Step 3:** Commit.

### Task 2.3: Rebundle + full checkpoint

- [ ] `make bundle-cli && make test`. Confirm green. Commit artifacts.

---

## Phase 3 — Delete the magic `__host_*` path

Only start once **no** boot source references `__host_*` (verify: `grep -rn "__host_" boot/`).

### Task 3.1: Remove boot-side magic registration

**Files:** `boot/compiler/base_env.tw:376-395,426-481`, `boot/compiler/lower_core/context.tw:44,136`

- [ ] **Step 1:** Delete the `host_*` and `__host_*` `builtin_sig` registrations in `base_env.tw` (`add_internal_host_builtins` and the `host_*` block).
- [ ] **Step 2:** Delete the `starts_with("host_")` alias synthesis (`context.tw:44`) and the `starts_with("__host_")` strip-fallback (`context.tw:136`). Keep `buf_` handling (Buffer intrinsics are a separate system).
- [ ] **Step 3:** `make boot-test`. Expected: green (nothing references the magic names now).
- [ ] **Step 4:** Commit.

### Task 3.2: Remove stage0 magic specs and predicates

**Files:** `src/intrinsics/registry.rs` (`HOST_*` specs + bootstrap names), `src/ir/lower.rs` (`HOST_*` FuncId constants), `src/types/env.rs` (`__host_*` builtin types), `src/codegen/emit.rs:2553-2557,7708-7713,7797-7828` (FuncId-keyed vector predicates and `__host_run_wasm` special cases)

- [ ] **Step 1:** Delete the `HOST_*` `spec!` entries and their `COMMON_BOOTSTRAP_FUNC_NAMES`/`LEGACY_BOOTSTRAP_FUNC_NAMES` rows.
- [ ] **Step 2:** Delete `HOST_*` constants in `lower.rs` and the `__host_*` `env.builtins.insert` calls in `env.rs`.
- [ ] **Step 3:** Delete `is_host_vector_arg`/`has_host_vector_args`/`is_host_vector_returning`/`is_host_read_file` and the `entry.twinkle_name == "__host_run_wasm"` special cases — these are now fully covered by the signature-driven extern path (Task 0.4).
- [ ] **Step 4:** `cargo build --release` — fix fallout. `make rust-test`.
- [ ] **Step 5:** `make bundle-cli` — the self-host loop must still compile `boot/main.tw` with stage0. This is the real proof the migration is complete. Then `make test`.
- [ ] **Step 6:** Commit.

### Task 3.3: Retire the `host` runtime object

**Files:** `tools/js_runtime/runtime.mjs`

- [ ] **Step 1:** Remove the `host` object from `makeHostImports` **except** any imports still needed by non-migrated intrinsics (verify: nothing in emitted wasm still imports module `host` for the migrated fns — grep emitted WAT of `boot/main.tw`).
- [ ] **Step 2:** Confirm `make test` green.
- [ ] **Step 3:** Commit.

### Task 3.4: Keep `host.parse_int`/`parse_float` working

`src/codegen/emit.rs:6593` (`ensure_host_parse_float_import`) and boot's int/float parse helpers import module `host`. These are **not** part of the OS-host surface and were intentionally excluded.

- [ ] **Step 1:** Decide: either (a) leave module `host` alive solely for `parse_int`/`parse_float`, or (b) move them under `twinkle_runtime` for consistency (rename `as_sym`/module in both compilers + `makeHostImports`).
- [ ] **Step 2:** If (b), make the rename and re-test; if (a), add a code comment at `ensure_host_parse_float_import` noting `host` is retained only for these pure helpers.
- [ ] **Step 3:** `make test`. Commit.

---

## Phase 4 — Cleanups surfaced during review

Independent of the migration; can land any time.

### Task 4.1: Tie the `resolve.rs` view-name allowlist to a real fix

**Files:** `src/types/resolve.rs:734` (the `matches!(type_name, "U8View" | "I64View" | "F64View")` permissive branch)

- [ ] **Step 1:** Add a `TODO` referencing the underlying cause (stage0 not preserving qualified type names in `pub type X = mod.X` re-exports) and this plan, so the allowlist isn't mistaken for intended behavior.
- [ ] **Step 2:** (Optional, larger) Implement qualified-name preservation in stage0 alias resolution so the allowlist can be deleted. If out of scope, leave the TODO.
- [ ] **Step 3:** Commit.

### Task 4.2: Note the double-read in `fs.read_buffer`

**Files:** `boot/stdlib/fs.tw:38-54`, `tools/js_runtime/runtime.mjs` (`read_buffer_len_raw` / `read_buffer_raw`)

- [ ] **Step 1:** Add a comment documenting that `read_buffer` reads the file twice (size, then fill) and the TOCTOU window, or (better) collapse to a single host call that allocates and fills, returning the length. If collapsing, update both the extern signature and the host fn.
- [ ] **Step 2:** `make boot-test`. Commit.

---

## Self-Review checklist (run before execution)

- **Spec coverage:** Phase 0 = vector capability (runtime/boot/checker/stage0); Phases 1–2 = migrate all 17 host fns (8 primitive + 9 vector) + namespace; Phase 3 = delete magic from both compilers + runtime; Phase 4 = review smells. Every host fn in the Background table is migrated in Task 1.2 or 2.1 (`stdin_eof` rides along with the io fns in 2.1).
- **Ordering invariant:** stage0 vector support (Task 0.4) lands before boot stdlib uses vector externs (Phase 2), and `make bundle-cli` self-host gates Phase 3 deletion — stage0 must compile the migrated boot.
- **Type consistency:** kind tags are `bytes` (Vector<Byte>), `strvec` (Vector<String>), `readfile` (Result<Vector<Byte>,String>), reused identically in `mono_type_kind` (boot), the stage0 emitter, and `bridgeExternImports` (runtime).
- **Namespace:** `twinkle_runtime` everywhere; `host` retained only for `parse_int`/`parse_float` per Task 3.4.
