# Typed `Vector.make` + Bool field/payload parity design

**Date:** 2026-07-09
**Branch context:** `typed-vector-crossfn-abi`
**Status:** draft for review

## Goal

Continue the `PVecBool` follow-up work in the order that can unlock the real dataframe null-mask path:

1. Add a routed, family-typed `Vector.make` producer so `Vector.make(n, false)` can physically produce `PVecBool` when its actual path is typed-safe.
2. Fix the remaining Bool field/payload/gather parity gap so Bool follows the same typed-friendly multi-use paths that already work for Int.

The target shape is the dataframe null mask:

```tw
nulls := Vector.make(n, false)
col := Column.{ nulls }
flag := col.nulls[i]
next := col.nulls.gather(idx)
```

When all consumers are typed-friendly, `nulls` should stay physically `PVecBool`, indexed reads should call `get_bool`, and gathers should call `gather_bool`. When the value reaches a durable erased boundary, it must still box.

## Background

The element-family generalization landed enough of `PVecBool` to type the core read path: a `collect`-produced `Vector<Bool>` read by index/len routes through `PVecBool` and `get_bool`.

Two blockers remain:

- `Vector.make` is only a boxed builtin today. A field initialized from `Vector.make(n, false)` is therefore fed by a boxed producer, and the field analysis correctly demotes it. This blocks the real `Column.nulls = Vector.make(n, false)` path.
- A multi-use Bool field/payload shape still diverges from the analogous Int shape. A collect-produced `Vector<Int>` field can remain typed through field read, equality, and gather; the collect-produced `Vector<Bool>` analogue stays boxed. This points to residual Int-specific routing, layout, emit, or verifier behavior beyond the simple read path.

## Guiding policy

Typed vectors remain a per-storage-site optimization, not a global property of `Vector<T>`.

- `Vector.make(n, false)` must not always produce `PVecBool`.
- It should produce `PVecBool` only when the router proves the result slot is typed-safe for its actual uses.
- Normal `fn(mask: Vector<Bool>)` parameters still use the boxed Vector ABI. A typed argument boxes at that call boundary.
- Durable erased boundaries still receive boxed `PVec` values.

## Recommended architecture

Add typed `Vector.make` as a routed producer, then use it to drive the Bool parity fix.

Do not change the source API. Users still write:

```tw
xs := Vector.make(n, false)
```

Internally:

1. Runtime gains family-specific typed make helpers such as `make_i64` and `make_bool`.
2. The `ElemFamily` descriptor grows a `make_call` runtime symbol.
3. The routing layer treats `Vector.make` result slots as typed-vector producer sources when the result mono maps to a registered family. This must feed every existing producer consumer, not only the local rewrite pass: `compute_eligible_v`, `analyze_typed_fields`, `analyze_typed_payloads`, `slot_typed_after_route`, `materialize_slot_repr`, and capture/free-var support queries all need to see the same make-source abstraction.
4. Emit keeps the existing boxed `rt_arr__make` path by default, and calls `fam.make_call` only when the destination slot has already been physically routed to that typed family.
5. Bool field/payload/gather parity is fixed after Track A so the parity probes include both collect-produced and `Vector.make`-produced Bool values.

This preserves the representation-boundary policy while giving `Vector.make` the same typed-routing opportunity as builder/collect producers.

## Alternatives considered

### Always emit typed make for primitive families

The intrinsic emitter could directly call `make_bool` whenever the result mono is `Vector<Bool>`.

Rejected because it bypasses the router. A value that immediately crosses an erased boundary would become physically typed before the compiler has proven that path is safe, pushing policy decisions into emit and increasing the chance of missed boxes.

### Lower `Vector.make` to collect/builder IR earlier

The compiler could rewrite `Vector.make(n, v)` into a builder loop or collect-shaped producer before routing.

Rejected for this pass because it is broader than needed. It changes the IR shape of a general-purpose builtin to reuse the existing builder candidate machinery. A routed producer case is smaller, keeps the builtin semantics intact, and is easier to verify.

### Fix Bool parity before typed make

The collect-produced Bool parity gap can be worked independently.

Rejected as the first step because it cannot unlock the dataframe null-mask path by itself. The real `Column.nulls` producer is `Vector.make(n, false)`, so the field stays boxed until typed make exists. Bool parity still matters, but Track A supplies the real producer shape that Track B should validate.

## Track A: typed `Vector.make`

### Runtime helpers

Add family-specific helpers in `boot/compiler/codegen/runtime/arr.tw`:

- `make_i64(size: i32, elem: i64) -> PVecI64`
- `make_bool(size: i32, elem: i32) -> PVecBool`

Each helper should build a typed persistent vector using the existing typed builder helpers:

- `builder_new_i64` / `builder_push_i64_raw` / `builder_freeze_i64`
- `builder_new_bool` / `builder_push_bool_raw` / `builder_freeze_bool`

The Bool helper takes a raw `.I32` element. It must not expect boxed `ref.i31`; the intrinsic emitter already has the typed Bool value on the stack.

The Int helper takes raw `.I64`, matching typed Int storage.

### Family metadata

Extend `compiler.elem_family.ElemFamily` with:

```tw
make_call: String // "rt_arr__make_i64" / "rt_arr__make_bool"
```

This keeps emit independent of builtin ids. The runtime symbol belongs in the leaf family descriptor, like `get_call`, `box_call`, and `unbox_call`.

If helper builtins are registered for routing convenience, add the ids to `FamilyIds`; otherwise route can recognize the generic `Vector.make` builtin id plus the result mono family without per-family builtin ids. Prefer the latter unless the existing code structure makes builtin-id symmetry simpler.

### Routing analysis

Teach `route_typed_vec.tw` that a `Vector.make` result can be a typed producer source for the active family.

The current source discovery is `collect_candidates`, and it only recognizes `builder_freeze(builder)` candidates. Field and payload analyses also call this path, so adding make handling only inside `compute_eligible_v` is insufficient: `Column.{ nulls: Vector.make(...) }` would still look like a boxed/foreign producer to `analyze_typed_fields` and be demoted.

Introduce a shared producer-source abstraction used by every route/query path that currently depends on builder candidates:

- `compute_eligible_v`
- `analyze_typed_fields`
- `analyze_typed_payloads`
- `slot_typed_after_route`
- `materialize_slot_repr`
- `free_var_typed_local` / capture support checks

A make-produced slot is eligible only when all of these hold:

- The result slot mono maps to the active `ElemFamily`.
- The call target is the generic `Vector.make` builtin.
- The group containing the result slot passes the existing producer typed-use predicate: index/len, typed field store, typed payload store, typed return, or typed capture are allowed; durable erased uses still escape.
- Field/payload stores still require the whole-program typed site sets to contain those sites.

Do not silently broaden direct producer-to-gather behavior in Track A. Today the producer-side classifier deliberately does not whitelist `gather(v, idx)` for a fresh builder producer; typed gather is a result-propagation rule for an already-typed receiver. Preserve that rule unless Track B explicitly changes it with a parity probe. The dataframe-shaped path can still type as `make -> typed field store -> typed field read -> gather_bool`.

This should reuse the same `v_group_typeable` decision as builder candidates rather than adding a separate oracle. The implementation may model producer sources as a tagged union, for example builder source `{ v, builder }` and make source `{ v }`; only builder sources populate `eligible_b`.

### Emit

Update `emit_intrinsic_vector_make` so it branches on the destination slot's physical type, not only the source-level mono.

The current intrinsic callback path drops that physical type: `emit_op` computes `result_vt`, `emit_call` receives it, but `emit/calls.tw` invokes `fns.emit_intrinsic_call(fid, args, result_idx, result_mono, ctx, buf)` without passing `result_vt`. Track A must first thread `result_vt: ValType?` through `CallEmitFns.emit_intrinsic_call`, `emit/calls.tw`, and `emit_intrinsic_call` in `emit.tw`, then pass it to `emit_intrinsic_vector_make`.

With that plumbing:

- If the destination valtype is a typed family ref, emit:
  1. checked i32 narrowing for the length
  2. the fill element in its typed scalar representation
  3. `.Call(fam.make_call)`
  4. `LocalSet(result_idx)`
- Otherwise, keep the existing boxed path:
  1. checked i32 narrowing for the length
  2. `emit_erased_container_ingress` for the fill element
  3. `.Call("rt_arr__make")`
  4. `LocalSet(result_idx)`

The emitter should fail loudly if the destination is a typed PVec family but the result mono does not agree with that family. That catches routing/layout drift early.

### Track A probes

Add executable probes under `examples/performance/sort-bench/`.

1. `typed_bool_make_read_probe.tw`

   Shape:

   ```tw
   xs_false := Vector.make(8, false)
   xs_true := Vector.make(8, true)
   println(xs_false[0].to_string())
   println(xs_true[7].to_string())
   ```

   Expected: output is correct, WAT contains `rt_arr__get_bool`, and the typed make call appears when inspecting calls. This probe intentionally avoids `set_at`: `Vector.set` / `set_at` is boxed-only today and typed set is not in scope.

2. `typed_bool_make_boxed_probe.tw`

   Shape:

   ```tw
   fn any_true(flags: Vector<Bool>) Bool {
     for f in flags {
       if f { return true }
     }
     false
   }

   xs := Vector.make(8, false)
   println(xs[0].to_string())
   println(any_true(xs).to_string())
   ```

   Expected: the indexed read keeps the producer typed (`rt_arr__get_bool` is present), and the normal parameter call boxes that typed value (`rt_arr__box_bool` is present). No `PVecBool` is passed directly as a normal `Vector<Bool>` parameter.

3. `typed_int_make_read_probe.tw`

   Shape:

   ```tw
   xs := Vector.make(8, 7)
   println(xs[0].to_string())
   println(xs[7].to_string())
   ```

   Expected: Int behavior mirrors Bool through `make_i64` / `get_i64`, preventing the new producer kind from being Bool-only.

## Track B: Bool field/payload/gather parity

### Starting point

After Track A, test two Bool producer shapes:

- collect-produced Bool vector
- `Vector.make`-produced Bool vector

Both should behave like their Int analogues when the consumers are equivalent and typed-friendly.

### Diagnostic invariant

For every tested shape:

- If `Vector<Int>` routes to `PVecI64`, `get_i64`, and `gather_i64`, then the corresponding `Vector<Bool>` shape should route to `PVecBool`, `get_bool`, and `gather_bool` unless Bool hits a documented representation-boundary rule that Int does not hit.
- If Bool stays boxed while Int types, the compiler must explain that through a specific unsupported edge, not through an unexamined Int hardcode.

### Likely investigation points

Use the parity probes to isolate where Bool diverges:

1. **Producer collection** — confirm Bool builder/collect and Bool make results enter the same candidate path as Int for the active family.
2. **Field analysis** — confirm `analyze_typed_fields` records Bool field sites when every producer and consumer is typed-friendly.
3. **Payload analysis** — confirm `analyze_typed_payloads` records Bool variant payload sites under the same conditions.
4. **Consumer classification** — confirm Bool structural equality, field reads, payload reads, and gather-on-an-already-typed receiver are classified as typed-friendly for the active family. If the desired parity probe includes direct `producer.gather(idx)`, make that an explicit classifier change rather than inheriting it accidentally from typed make.
5. **Materialization/retyping** — confirm typed field/payload reads and gather results are retyped to `PVecBool`, not left as boxed `PVec`.
6. **Emit/verifier edges** — confirm bridge/equality/coercion code uses `box_bool`/`unbox_bool` where Bool crosses erased edges, and does not reject valid `PVecBool` field/payload stores.

Fix the first divergence found. Avoid adding Bool-specific exceptions where a family-generic fix is possible.

### Track B probes

Strengthen or replace `examples/performance/sort-bench/typed_bool_field_payload_probe.tw` so it has side-by-side Int and Bool shapes.

The probe should cover:

- record field layout and field read
- variant payload layout and payload read
- structural equality for records/variants containing the vector
- `.gather(idx)` on a typed Bool receiver
- the same operations for Int as a parity baseline

Add a dataframe-shaped null-mask microprobe:

```tw
type Column = .{ nulls: Vector<Bool> }

idx := [0, 1, 2, 3]
nulls := Vector.make(4, false)
col := Column.{ nulls }
read := col.nulls[0]
taken := col.nulls.gather(idx)
println(read.to_string())
println(taken[0].to_string())
```

Expected: when all uses are typed-friendly, WAT contains `rt_arr__get_bool` and `rt_arr__gather_bool`, and record layout includes `PVecBool`.

## End-to-end validation

After Tracks A and B, run the normal verification loop:

```bash
target/twk fmt <edited .tw files>
cargo run --release -- build boot/main.tw -o /tmp/x.wasm
make bundle-cli 2>&1 | tail -3
make boot-test 2>&1 | grep -iE "Ran [0-9]+ tests|Failed"
target/twk run examples/performance/dataframe/bench/order_by_breakdown.tw
target/twk build examples/performance/dataframe/bench/order_by_breakdown.tw -o /tmp/order_by.wat
```

Inspect the dataframe WAT for Bool-family calls:

```bash
grep -c 'rt_arr__get_bool' /tmp/order_by.wat
grep -c 'rt_arr__gather_bool' /tmp/order_by.wat
grep -c 'rt_arr__make_bool' /tmp/order_by.wat
```

The benchmark remains a signal, not the primary correctness oracle. The probes should identify correctness and routing failures more directly.

## Sequencing

1. Add probes that currently demonstrate the missing typed `Vector.make` path.
2. Add typed make runtime helpers and family metadata.
3. Route `Vector.make` as a family-aware producer.
4. Emit typed make only for physically typed destinations.
5. Verify Track A probes and boxed-boundary behavior.
6. Strengthen Bool parity probes with collect-produced and make-produced shapes.
7. Fix the first Bool-vs-Int divergence in field/payload/gather routing.
8. Verify dataframe-shaped null-mask probe.
9. Run the full verification loop and record the new status in the vector performance docs.

## Non-goals

- Do not add normal typed parameter ABI. Normal `Vector<Bool>` and `Vector<Int>` function parameters remain boxed.
- Do not make `Vector<Bool>` globally represented as `PVecBool`.
- Do not add typed `take` or other helper families in this pass.
- Do not activate Float unless a tiny metadata-only follow-up is obvious after the Bool/Int work is stable.
- Do not weaken the representation-boundary policy to chase a benchmark number.

## Risks and mitigations

- **Risk:** typed make bypasses routing and creates invalid physical stores.
  **Mitigation:** emit typed make only when the destination valtype is already a typed PVec family.

- **Risk:** field/payload site sets remain untagged, so a Bool pass could misread an Int typed site.
  **Mitigation:** preserve the existing family filter on the read/result slot, and add parity probes that mix Int and Bool in the same program.

- **Risk:** Bool boxed encoding diverges from existing `ref.i31` encoding.
  **Mitigation:** typed make uses raw `.I32` internally; all durable boxed crossings continue through existing `box_bool`/`unbox_bool` or erased ingress paths.

- **Risk:** the dataframe benchmark does not move even after probes pass.
  **Mitigation:** inspect WAT for `make_bool`, `get_bool`, and `gather_bool` on the null-mask path. If the calls are present but the number does not move, treat that as performance triage, not a routing correctness failure.
