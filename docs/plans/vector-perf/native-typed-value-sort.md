# Native Typed Value Sort Implementation Plan

> **Status: ✅ LANDED.** `xs.sort()` on `Vector<Int>` and `Vector<Float>` is
> lowered to native typed kernels (`sort_i64`/`sort_f64` over dense GC arrays),
> with the monomorphization routing hook and stage0 parity in place (commits
> around `48a594a`/`60232f4`/`a66cbe2`). The per-step checkboxes below were not
> ticked during execution — this banner is the completion record. This is the
> first dense typed kernel that won, and the seed of the broader
> [typed-vector-representation.md](typed-vector-representation.md) track. Remaining
> open work is Bool/Byte families (Milestone 3 only covered Float).
>
> **For agentic workers (historical):** executed task-by-task; steps use checkbox
> (`- [ ]`) syntax.

**Goal:** Make `xs.sort()` on `Vector<Int>`/`Vector<Float>` fast by lowering it — with no source changes — to a native runtime kernel that unboxes once into a dense typed GC array, runs a stable merge over raw `i64`/`f64` (inlined compares, no closure, no per-element box/cast), and boxes once on the way out.

**Architecture:** Two new mechanisms. (1) A **type-directed routing hook** in monomorphization: a call to the prelude `Vector.sort` with a primitive element type is rewritten to call a native builtin; all other element types keep the existing generic merge. (2) A **typed GC array** (`rt_types__ArrayI64`, later `…F64`) — the first non-`anyref` runtime storage — backing per-type sort kernels in `rt.arr` (boot) mirrored in stage0. This is the deliberate seed of the broader typed-vector-representation track.

**Tech Stack:** Twinkle (`.tw`), the boot self-hosted compiler (`boot/`), the `rt.arr` persistent-vector runtime (`boot/compiler/codegen/runtime/arr.tw` + `src/runtime/arr.rs`), Wasm GC, Deno self-host loop (`make bundle-cli`).

---

## Background & why this shape

Measured same-machine baselines at N = 1,000,000 (Twinkle cold single-run; Clojure persistent-vector, warmed):

| operation | Twinkle | Clojure pvec |
|---|--:|--:|
| `xs.sort()` (plain value sort) | ~808 ms | ~192 ms |
| `idx.sort_by(key[a] vs key[b])` (argsort) | ~1628 ms | ~310–520 ms |

Our generic sort is ~4× off the persistent-vector reference. Two prior attempts were reverted:

- **Approach A** (in-place quicksort over a uniquely-owned PVec): writes fell to copy-on-write across call boundaries.
- **Approach C** (stable merge over an opaque `anyref` `Scratch<T>` buffer): per-element access was an *un-inlined runtime call* doing `ref.cast anyref→Array` + bounds-checked `array.get/set`, plus two extra full copies. It *regressed* the pure `Vector<Int>` sort ~16%.

The lesson: a dense buffer only wins when its element access is **inlined, typed** (`i64.array.get/set`, no cast, no call) and the hot loop lives **inside one runtime function** (no exposed per-element ops). That is exactly what this plan builds. See [wasm-native-sort.md](wasm-native-sort.md) and [native-sort-dense-merge.md](../archive/native-sort-dense-merge.md) (archived) for the full post-mortems.

## Scope

**In scope (this plan — Phase 1, value sort):**
- Native kernel for `xs.sort()` where the element type is `Int` or `Float`.
- Type-directed routing in both boot and stage0 (parity guardrail).
- A typed `i64` GC array (and `f64`) as the dense buffer.

**Out of scope (deferred, with rationale):**
- `xs.sort_by(closure)` with an arbitrary user comparator — the comparator is opaque; it cannot go native without boxing args per comparison (that was Approach C's trap). It keeps the existing generic merge.
- `Bool`/`String`/record/other element types — keep the generic merge (Bool sort is not a workload we care about; not worth a third typed array).
- **Argsort** (`idx.sort_by(key[a] vs key[b])`) and dataframe `order_by` — reuses this kernel core but is a distinct entry point + integration. It is **Phase 2**, a separate plan (sketched in the appendix).
- Radix/counting sort; full `Vector<T>` typed representation everywhere.

## Success criteria

- `xs.sort()` on `Vector<Int>` at N = 1M drops from ~808 ms toward ~200 ms (target: beat or match Clojure's ~192 ms), measured by a new microbenchmark.
- `Vector<String>.sort()` still compiles and runs correctly via the generic path (proven by WAT inspection: no native sort op emitted for `String`).
- Stable sort preserved; all existing vector/API suites green.
- `make bundle-cli` reaches its self-host fixed point; boot and stage0 in parity.

## File structure

```
boot/prelude/signatures/vector.tw          internal builtin signature stub(s) for the native sort op(s)
boot/prelude/vector.tw                      sort<T:Ord>/sort_by unchanged (generic merge = the fallback body)
boot/compiler/builtins.tw                   ABI + rt(...) registration for vector$sort_i64 / sort_f64
boot/compiler/codegen/runtime/arr.tw        sort_i64_fn()/sort_f64_fn() kernels; i64/f64 GC array helpers
boot/compiler/codegen/bridge.tw             declare rt_types__ArrayI64 / rt_types__ArrayF64 GC array types
boot/compiler/codegen/wasm_layout.tw        layout/heaptype plumbing for the typed arrays if needed
boot/compiler/monomorphize.tw              routing hook in rewrite_calls (Vector.sort<prim> -> native op)
boot/tests/suites/api_vector_suite.tw       correctness + stability + Float tests
examples/.../bench (new)                     value-sort microbenchmark

src/runtime/arr.rs                          stage0 kernels + typed array parity
src/codegen/*, src/intrinsics/*, src/types/* stage0 builtin wiring (mirror builtins.tw touch points)
src/<monomorphize>.rs                        stage0 routing hook parity
docs/plans/wasm-native-sort.md              record results
```

## Conventions (read before starting)

- **Prelude/compiler edits need `make bundle-cli`** (slow self-host; must print `Fixed point reached`). Test-only edits run via `make boot-test`. Run `make fmt` after `.tw` edits — but note it dirties 4 unrelated compiler files (`checker.tw`, `linker.tw`, `wasm.tw`, `wasm_ir.tw`) and drops a real comment in `linker.tw` via a known fmt bug; `git checkout --` those and `git add` only intended files.
- `boot/lib/module/core_lib.tw` is **generated/gitignored** — never commit it. It is regenerated from `boot/prelude/**` by `python3 tools/generate_core_lib.py` (run inside `make bundle-cli`).
- **Do not run full `cargo test`** (too slow). The self-host convergence in `make bundle-cli` is the stage0 gate; add targeted `cargo test --release <filter>` only where noted.
- **New builtin FuncIds are appended at the end** of the registry (append-at-end discipline). Runtime ops *called by name from prelude/compiler* need `include_in_signature_registry: true` + a `signatures.rs contract()` arm in stage0 — the builder-ops `false,false` precedent does NOT apply (this cost Approach C a re-review cycle). See memory `reference-runtime-builtin-wiring` and the archived `docs/plans/archive/vector-gather.md` for the exact touch-point map.
- **rt.arr DSL bodies are given as algorithm + helper signatures + template refs + named verification points + a build/verify acceptance test**, not fabricated instruction-by-instruction. This matches the established repo convention (see `docs/plans/archive/vector-gather.md` Task 6). The build/verify loop is the source of truth for the exact DSL.

---

## Task 1: Routing spike — prove `Vector.sort<Int>` reaches a native op (boot only, no typed array yet)

**Why first:** the type-directed routing is brand-new machinery (no element-type-directed op exists today). De-risk it with a *correct-but-not-yet-fast* stub op before investing in the typed GC array and kernel. The stub stores into the existing `anyref` array and unboxes-to-compare — fine for proving routing.

**Files:**
- Modify: `boot/prelude/signatures/vector.tw` (add internal stub `sort_i64`)
- Modify: `boot/compiler/builtins.tw` (ABI ~line 129 block; `rt(...)` ~line 509 block)
- Modify: `boot/compiler/codegen/runtime/arr.tw` (add `sort_i64_fn()`, register in the FuncDef list ~line 149)
- Modify: `boot/compiler/monomorphize.tw` (routing hook in `rewrite_calls`, ~line 1396–1442)

- [ ] **Step 1: Add the internal signature stub**

In `boot/prelude/signatures/vector.tw`, after the `gather` stub, add:

```tw
// Internal: native typed value sort over Vector<Int>. Not a user-facing API;
// `xs.sort()` is routed here by the compiler for primitive element types.
pub fn sort_i64(xs: Vector<Int>) Vector<Int> {
  xs
}
```

- [ ] **Step 2: Register ABI + runtime mapping in `builtins.tw`**

With the other `abi(...)` entries (near line 129, next to `vector$gather`):

```tw
"vector$sort_i64" => abi([pvec_n()], [pvec_()]),
```

With the other `rt(...)` registrations (near line 509). Canonical is `.None` — internal only, never resolved as a user method:

```tw
rt("vector$sort_i64", "rt.arr", "sort_i64", .None),
```

- [ ] **Step 3: Implement a STUB `sort_i64_fn()` in `arr.tw` (correct, not fast)**

Add `sort_i64_fn()` and register it in the FuncDef list (the vector containing `gather_fn(), drop_last_fn(), …` near line 149) by appending `sort_i64_fn(),`.

Stub algorithm — reuse existing `anyref` helpers; correctness only:

```
sort_i64(vec: PVec?) -> PVec:
  n = len(vec)                              // existing "len"
  if n <= 1: return vec
  // copy into a fresh anyref builder-backed working array, stable-merge by
  // unboxing each element to i64 and comparing with i64.lt_s, then freeze.
  // Reuse get(vec,i) -> anyref -> cast BoxedInt -> i64 for the compare key.
  // (Speed is irrelevant in this task; this is replaced in Task 2.)
```

Template refs: model the function skeleton on `gather_fn()`/`concat_fn()` in the same file; model boxed-int read on `get_fn` (arr.tw ~line 1268). Verification point: how a `BoxedInt` element is read out and unboxed to `i64` — confirm against the existing `==`/Int read path while implementing; the build/verify loop (Step 6) catches mismatches.

- [ ] **Step 4: Add the routing hook in `monomorphize.tw`**

In `rewrite_calls` (the `.Call(callee, args)` arm at ~line 1396 that builds `type_args` and looks up `spec_key`), before the normal specialized-call rewrite, special-case the prelude `Vector.sort` FuncId:

```tw
// Type-directed fast path: Vector.sort over a primitive element type lowers to a
// native typed kernel instead of the generic Ord merge. Non-primitive element
// types fall through to the normal generic specialization below.
if ctx.is_vector_sort(fid) and type_args.len() == 1 {
  case type_args[0] {
    .Int => return .Call(.{ kind: .GlobalFunc(ctx.vector_sort_i64_fid) }, new_args),
    _ => {},   // Float added in Task 3; others -> generic
  }
}
```

`ctx.is_vector_sort(fid)` resolves the FuncId of the prelude `Vector.sort` (Ord variant) — implement by looking it up via the canonical-name/registry the ctx already exposes (mirror how other named prelude functions are identified in this file). `ctx.vector_sort_i64_fid` is the builtin FuncId for `vector$sort_i64`. Verification point: confirm `type_args` here is the monomorphized element-type list (it feeds `spec_key`), and that `.Int` is the right `MonoType` constructor (check `mono_type.tw`).

- [ ] **Step 5: Add a routing-proof test program**

```bash
mkdir -p /tmp/sortspike && printf 'name="s"\n' > /tmp/sortspike/twinkle.toml
cat > /tmp/sortspike/main.tw <<'EOF'
ints: Vector<Int> = [3, 1, 2, 1]
strs: Vector<String> = ["c", "a", "b"]
println("${ints.sort()}")
println("${strs.sort()}")
EOF
```

- [ ] **Step 6: Build, verify routing via WAT, verify correctness**

```bash
make bundle-cli                                   # must print "Fixed point reached"
target/twk run /tmp/sortspike/main.tw             # expect: [1, 1, 2, 3] then [a, b, c]
target/twk build /tmp/sortspike/main.tw -o /tmp/sortspike/out.wat
grep -c "sort_i64" /tmp/sortspike/out.wat         # expect: >= 1 (Int routed to native op)
```

Expected: correct sorted output, AND the WAT contains a call to `rt.arr.sort_i64` (Int routed) while `String.sort()` does not route (it still calls the generic prelude sort — confirm the `String` path is unchanged by inspecting that the generic sort function is still present/called).

- [ ] **Step 7: Run the boot sort suite (behavior unchanged)**

```bash
make boot-test
```
Expected: all green (the existing sort/sort_by tests now exercise the routed Int path for `Vector<Int>` and the generic path for others).

- [ ] **Step 8: Commit**

```bash
git add boot/prelude/signatures/vector.tw boot/compiler/builtins.tw \
        boot/compiler/codegen/runtime/arr.tw boot/compiler/monomorphize.tw
git commit -m "sort: route Vector.sort<Int> to a native op (spike, generic-speed stub kernel)"
```

---

## Task 2: Typed `i64` GC array + the real `sort_i64` kernel

**Files:**
- Modify: `boot/compiler/codegen/bridge.tw` (declare `rt_types__ArrayI64`, near the `Array` decl ~line 41)
- Modify: `boot/compiler/codegen/runtime/arr.tw` (i64-array helpers + replace `sort_i64_fn()` body)
- Modify: `boot/tests/suites/api_vector_suite.tw` (stability + correctness)

- [ ] **Step 1: Declare the typed i64 GC array**

In `boot/compiler/codegen/bridge.tw`, next to the existing anyref `Array` (line 41), add a mutable `i64` array type:

```tw
.Array("ArrayI64", FieldDef.{ name: .None, mutable: true, ty: .I64 }),
```

This emits a `(type $rt_types__ArrayI64 (array (mut i64)))` GC type. Verification point: confirm the codegen path that turns these `.Array(...)` decls into Wasm type entries handles `.I64` element fields (the existing `Array` uses `.Anyref`); check `wasm_layout.tw` for any element-type assumption and extend if needed.

- [ ] **Step 2: Add i64-array helpers in `arr.tw`**

Add small helpers mirroring `t_array()`/`arr_ref()`/`arr_null()` (arr.tw ~line 22) for the new type, e.g. `t_array_i64()` → `"rt_types__ArrayI64"`, plus ref/null `ValType` helpers. These are used only inside the kernel.

- [ ] **Step 3: Write the stability test (failing until the real kernel lands)**

Append to the sort test group in `boot/tests/suites/api_vector_suite.tw` (the `SortStabItem` type/helpers already exist from prior work; reuse them — but note value sort sorts the *records'* Ord, so use Int keys here):

```tw
    .test(
      "native Int value sort is stable and correct on adversarial input",
      fn() {
        // duplicate-heavy, neither ascending nor descending -> dense merge path
        dup := collect i in range(4000) { i * 7919 % 13 }
        sorted := dup.sort()
        try assert.equal(sorted.len(), 4000)
        try assert.is_true(sort_suite_is_sorted(sorted))
        try assert.equal(sort_suite_sum(sorted), sort_suite_sum(dup))
        // extremes
        ex: Vector<Int> = [9223372036854775807, -9223372036854775808, 0, -1, 1]
        s2 := ex.sort()
        try assert.equal(s2[0], -9223372036854775808)
        try assert.equal(s2[4], 9223372036854775807)
        .Ok({})
      },
    )
```

- [ ] **Step 4: Run to confirm it passes with the stub, then replace the kernel body**

Run `make boot-test` — the stub already produces correct sorts, so this test PASSES now (locks behavior). Then replace `sort_i64_fn()`'s body with the real typed kernel:

```
sort_i64(vec: PVec?) -> PVec:
  n = len(vec); if n <= 1: return vec
  src = ArrayI64 of length n      // array.new_default i64
  aux = ArrayI64 of length n
  i = 0
  loop i<n: src[i] = unbox_int(get(vec,i)); i++     // O(n) boxed reads, the only boxing
  // stable bottom-up merge, ping-pong src<->aux, width=1,2,4,...:
  //   compare src[p] vs src[q] with i64.lt_s; tie -> take left (stable); copy with array.get/set i64
  //   swap src/aux each pass; track which holds the result
  result = <buffer holding sorted data>
  builder = builder_new()
  i = 0
  loop i<n: builder_push(builder, box_int(result[i])); i++   // O(n) boxing out
  return builder_freeze(builder)
```

All `array.get/set` are on the typed `i64` array — inlined, no cast, no call. The merge is the same algorithm as the reverted dense merge (commit `8551c24` `merge_run`/`merge_sort_dense`) but over raw `i64` inside this one function. Verification points: `array.new` for `ArrayI64`, `i64.lt_s`, and box/unbox of `BoxedInt` (model on `get_fn`/the Int read path). Build/verify loop is the source of truth.

- [ ] **Step 5: Build, verify, benchmark sanity**

```bash
make bundle-cli
make boot-test                         # stability + correctness green
target/twk run /tmp/sortspike/main.tw  # still [1, 1, 2, 3] / [a, b, c]
```

- [ ] **Step 6: Commit**

```bash
git add boot/compiler/codegen/bridge.tw boot/compiler/codegen/runtime/arr.tw \
        boot/tests/suites/api_vector_suite.tw boot/compiler/codegen/wasm_layout.tw
git commit -m "sort: native i64 value-sort kernel over a typed GC array"
```

---

## Task 3: Float value sort

**Files:**
- Modify: `boot/compiler/codegen/bridge.tw` (`rt_types__ArrayF64`)
- Modify: `boot/compiler/codegen/runtime/arr.tw` (`sort_f64_fn()`)
- Modify: `boot/compiler/builtins.tw`, `boot/prelude/signatures/vector.tw` (`vector$sort_f64` stub + wiring)
- Modify: `boot/compiler/monomorphize.tw` (extend routing: `.Float`)
- Modify: `boot/tests/suites/api_vector_suite.tw`

- [ ] **Step 1: Float test (negatives, zero, ordering)**

```tw
    .test(
      "native Float value sort orders negatives, zero, positives",
      fn() {
        xs: Vector<Float> = [3.5, -1.0, 0.0, -2.5, 1.25]
        s := xs.sort()
        try assert.is_true(s[0] == -2.5)
        try assert.is_true(s[1] == -1.0)
        try assert.is_true(s[4] == 3.5)
        .Ok({})
      },
    )
```

- [ ] **Step 2: Implement `sort_f64`**

Declare `rt_types__ArrayF64` (`.Array("ArrayF64", … ty: .F64)`). `sort_f64_fn()` mirrors `sort_i64_fn()` but uses an `f64` array and **`f64.lt`** for the compare. Wire `vector$sort_f64` (ABI `[pvec_n()]→[pvec_()]`, `rt(... .None)`, signature stub `sort_f64(xs: Vector<Float>) Vector<Float>`). Box/unbox uses `BoxedFloat`. Note: Twinkle has no `Float: Ord`-NaN policy in scope here; `f64.lt` orders NaN to one end deterministically — acceptable for Phase 1 (document it).

- [ ] **Step 3: Extend the routing hook**

In `monomorphize.tw` `rewrite_calls`, extend the `case type_args[0]`:

```tw
    .Int => return .Call(.{ kind: .GlobalFunc(ctx.vector_sort_i64_fid) }, new_args),
    .Float => return .Call(.{ kind: .GlobalFunc(ctx.vector_sort_f64_fid) }, new_args),
    _ => {},   // Bool/String/records fall through to the generic merge
```

- [ ] **Step 4: Build, test, commit**

```bash
make bundle-cli && make boot-test
git add boot/compiler/codegen/bridge.tw boot/compiler/codegen/runtime/arr.tw \
        boot/compiler/builtins.tw boot/prelude/signatures/vector.tw \
        boot/compiler/monomorphize.tw boot/compiler/codegen/wasm_layout.tw \
        boot/tests/suites/api_vector_suite.tw
git commit -m "sort: native Float (f64) value-sort path"
```

---

## Task 4: stage0 (Rust) parity

Mirror the native ops + typed arrays + routing in the Rust reference compiler so `cargo run` matches boot and the self-host loop stays valid. Use the `gather`/`drop_last` touch points (archived `vector-gather.md` Task 7) as the map.

**Files:**
- Modify: `src/runtime/arr.rs` (`sort_i64`/`sort_f64` kernels + typed array types)
- Modify: `src/types/env.rs`, `src/codegen/prelude.rs`, `src/intrinsics/registry.rs`, `src/intrinsics/signatures.rs`, `src/ir/lower.rs` (builtin wiring; **`include_in_signature_registry: true` + `signatures.rs contract()` arm** for each op — these ops are referenced by name)
- Modify: the stage0 monomorphization pass (routing hook parity)

- [ ] **Step 1: Implement the kernels + typed arrays in `src/runtime/arr.rs`**

Replicate `sort_i64_fn`/`sort_f64_fn` and the `rt_types__ArrayI64`/`F64` type emission, same algorithm as Tasks 2–3.

- [ ] **Step 2: Wire each op across the stage0 touch points**

For every line found by `grep -rn "gather" src/`, add the analogous `sort_i64`/`sort_f64` entry (new prelude-id constants, twinkle/runtime names, ABI, signature). Honor `include_in_signature_registry: true` + the `contract()` arm.

- [ ] **Step 3: Mirror the routing hook**

In stage0's monomorphization call-rewrite, add the same `Vector.sort<prim>` → native-op redirect as boot's `monomorphize.tw`.

- [ ] **Step 4: Build stage0, verify parity, self-host**

```bash
cargo build --release
cargo run --release -- run /tmp/sortspike/main.tw      # same output as boot
make bundle-cli                                          # full self-host; "Fixed point reached"
```

- [ ] **Step 5: Targeted Rust test + commit**

```bash
cargo test --release vector
git add src/
git commit -m "stage0: native typed value-sort parity (sort_i64/sort_f64 + routing)"
```

---

## Task 5: Microbenchmark, gate, and documentation

**Files:**
- Create: a value-sort microbenchmark (mirror `examples/dataframe/bench/order_by_micro.tw` structure)
- Modify: `docs/plans/wasm-native-sort.md` (results), memory `project-native-sort-dense-merge`

- [ ] **Step 1: Add the microbenchmark**

Create a small project that times `xs.sort()` on `Vector<Int>` and `Vector<Float>` at N = 10k/100k/1M (deterministic LCG fill, checksum), matching the timing style of `order_by_micro.tw`.

- [ ] **Step 2: Run the gate**

```bash
target/twk run <bench>.tw            # record Twinkle numbers
clojure /tmp/clj_value_sort.clj      # Clojure reference (~192 ms value sort @1M)
```

- [ ] **Step 3: Evaluate**

PASS = `xs.sort()` Int @1M drops materially from ~808 ms toward ~200 ms. Record numbers. If still far off, profile the box/unbox endpoints vs the merge body (the merge should now be pure `i64`; if endpoints dominate, that points at the next lever — typed PVec leaves, i.e. the broader typed-representation track).

- [ ] **Step 4: Record results + status**

Update `docs/plans/wasm-native-sort.md` ("Attempts so far": add the value-sort result; this is the first dense-typed kernel that *won*, validating the model A and C failed) and the project memory. Commit:

```bash
git add docs/plans/wasm-native-sort.md <bench files>
git commit -m "sort: value-sort microbenchmark + record native typed kernel results"
```

---

## Appendix — Phase 2: argsort + dataframe `order_by`

Tracked in [native-key-index-argsort.md](native-key-index-argsort.md).

Reuses this plan's typed buffer + merge core. New entry point: a native **argsort** that sorts an index array against a dense typed key buffer (and a null-rank), comparison inlined, no closure. Surface (general builtin family):

```tw
Vector.argsort(keys: Vector<K>) Vector<Int>                              // K in {Int,Float,Bool}
Vector.argsort_nulls(keys: Vector<K>, nulls: Vector<Bool>, descending: Bool) Vector<Int>
```

Dataframe `sort_indices_by_column` (`examples/dataframe/frame/table.tw:241`) replaces its `Int`/`Float`/`Bool` `idx.sort_by(closure)` arms with native null-aware argsort helpers; `String` keeps the closure path. `order_by`'s gather/`take` is a separate, smaller follow-up (it is ~1/40th of `order_by`'s reads; the v1 `Vector.gather` is already a constant-factor builder loop — a structural trie-aware gather is the only further lever there). Null semantics: null = +infinity (Asc → last, Desc → first), matching the current comparator.

---

## Self-Review

- **Spec coverage:** routing mechanism (Task 1 + extended in 3/4), typed `i64`/`f64` GC array + kernels (Tasks 2–3), stage0 parity (Task 4), benchmark gate + docs (Task 5). `String`/arbitrary-`sort_by`/argsort explicitly scoped out with rationale; argsort handed to a Phase 2 plan.
- **Placeholder scan:** test code, the routing hook, acceptance commands, and ABI/registration lines are literal. The rt.arr/arr.rs DSL kernel bodies are given as algorithm + helper signatures + template refs (`gather_fn`/`concat_fn`/`get_fn`) + named verification points + a build/verify acceptance test — the documented repo convention for `rt.arr` ops (see archived `vector-gather.md`), because the instruction DSL must be validated by the build loop, not fabricated blind.
- **Type/name consistency:** `vector$sort_i64`/`vector$sort_f64`, `rt_types__ArrayI64`/`ArrayF64`, `ctx.vector_sort_i64_fid`/`vector_sort_f64_fid`/`is_vector_sort`, `sort_i64`/`sort_f64` used consistently across tasks. Routing keyed on `type_args[0]` `MonoType` (`.Int`/`.Float`).
- **Sequencing:** Task 1 de-risks routing with a correct stub before the typed array exists; Task 2 adds the typed buffer + real kernel; Task 3 extends to Float; Task 4 brings stage0 to parity (required before the self-host gate is meaningful); Task 5 measures. Each task ends green and committed.
- **Risk flag:** the typed GC array (Task 2 Step 1) is the single biggest unknown — it is the first non-`anyref` runtime storage. If `bridge.tw`/`wasm_layout.tw` resist a non-`anyref` element field, that surfaces immediately in Task 2's build/verify and may need a small codegen extension before the kernel.
```
