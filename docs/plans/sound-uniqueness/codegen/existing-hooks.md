# Existing Mutable Hook Inventory

**Status:** Verified against the boot compiler on `main` (2026-07-20, Phase 7A).
Names, ABIs, runtime symbols, and current emission state below are checked, not
assumed. Re-verify if the builtin/runtime tables move.

The first codegen implementation should reuse current mechanisms. This doc names
the hooks that already exist and the current emission state each is in.

## Headline finding: hooks survive, the rewrite pass was removed

The reusable surface below survives from the previous COW/uniqueness era:
runtime/builtin helpers for vector/dict updates and builder families, plus backend
`can_reuse` emit support for record shell updates. Ownership-specialized variants
are later compiler cloning/routing work, not a runtime helper target. What was
removed is the *pass that selected ownership-driven mutable code*:
`boot/compiler/opt/pipeline.tw` documents that "the uniqueness/liveness rewrite
(COW -> in-place, builder-region) was removed; this pipeline now only runs defer
elimination + the general peephole passes." The active optimizer is
`eliminate_defers` followed by a fixed point of
`dead_let_elim` / `copy_propagate` / `constant_fold` / `branch_simplify`. None of
them select vector/dict in-place helpers, flip record `can_reuse=true`, or choose
optimizer-selected builder regions. Semantic builder lowering such as `collect`
still emits builders; that is not an ownership optimization decision.

Consequence for the codegen track: we are not building hooks from scratch. We are
re-driving surviving hooks from the **new sound facts** (`ownership.tw`
`Summary`/`ParamSummary`/`VariantId`, `select_variant`, `summarize_variant`)
instead of the removed unsound COW analysis. The old `in_place_equivalent`
metadata in `opt/semantics.tw` is still present and still maps persistent op →
mutable target; today it is consumed only by `census.tw` (candidate counting) and
by `cow_config_from_semantics` (which has no active downstream consumer).

## Verified hook table

| Family | Persistent op (builtin) | Mutable target (builtin) | Runtime symbol | ABI (params → results) | Backend state |
|---|---|---|---|---|---|
| Vector indexed update | `vector$set_unsafe` (→ `rt.arr.set`) | `vector$set_in_place` | `rt.arr.set_in_place` | `[pvec?, i32, anyref] → [pvec]` (identical to persistent) | Callable; never emitted |
| Dict set | `Dict.set` method | `dict$set_in_place` | `rt.dict.set_in_place` | `[dict?, anyref, anyref] → [dict]` | Callable; never emitted |
| Dict remove | `Dict.remove` method | `dict$remove_in_place` | `rt.dict.remove_in_place` | `[dict?, anyref] → [dict]` | Callable; never emitted |
| Vector builder | persistent append/build | `vector$builder_new/from/push/freeze` | `rt.arr.builder_*` | `new []→[arr]`, `from [pvec?]→[arr]`, `push [arr?, anyref]→[]`, `freeze [arr?]→[pvec]` | Callable (also used by `collect`); optimizer region-select removed |
| Vector builder (typed) | — | `vector$builder_{new,push,freeze}_{i64,bool}` | `rt.arr.builder_*_{i64,bool}` | `new []→[arr]`, `push [arr?, anyref]→[]`, `freeze [arr?]→[pvec_i64/pvec_bool]` | Callable shims for the vector family; selected by typed-vector routing, not ownership decisions |
| String builder | persistent `String.concat` loop | `string$builder_from/extend/freeze` | `rt.str.builder_*` | `from [str?]→[sb]`, `extend [sb?, str?]→[]`, `freeze [sb?]→[str]` | Callable; no in-place equivalent; region-select removed |
| Record shell update | `ARecordUpdate(.., can_reuse=false)` → copy | `ARecordUpdate(.., can_reuse=true)` → `struct.set` + return same ref | n/a (backend emit) | n/a | `emit_record_update` honors `can_reuse=true`; lower_anf always constructs `false`, no pass flips it |

The verified builtin/ABI table lives in `boot/compiler/builtins.tw`; the semantics
metadata (`effect`, `cow_base_arg`, `in_place_equivalent`, `retained_args`) lives
in `boot/compiler/opt/semantics.tw`; the two record-update construction sites are
`boot/compiler/lower_anf.tw` (both pass `false`); the record in-place emit is
`boot/compiler/codegen/emit/records.tw:emit_record_update`.

## Hook categories

### Vector indexed update

- Existing unsafe/in-place vector set helper used by previous optimization work.
- Persistent fallback remains ordinary vector set/update lowering.
- First target for emitted-code changes because the argument/result shape is small
  and easy to inspect in WAT.

### Vector builders

- Existing `vector$builder_new`, `vector$builder_from`, `vector$builder_push`, and
  `vector$builder_freeze` family.
- Typed vector builder hooks (`vector$builder_new_i64`, `vector$builder_push_i64`,
  `vector$builder_freeze_i64`, and the `bool` equivalents) are already present and
  covered by current builder ABI shims.
- Some builder use is semantic, notably `collect`, and must keep working without
  an ownership optimization decision.
- Optimizer-selected builder regions should be treated separately from semantic
  builder lowering.
- Runtime `rt.arr` also exports lower-level/extend helpers such as
  `builder_extend`, but these are not currently first-cut boot builtin/shim
  targets until cataloged.

### String builders

- Existing `string$builder_from`, `string$builder_extend`, and
  `string$builder_freeze` family.
- `compiler.builder_family.string_builder_config` and optimizer semantics already
  model `String.concat` as a builder-region candidate with no in-place equivalent.
- String builder buffers use the same erased-builder ABI shim path as vector
  builders, but cast to `rt_types__StrBuilder`.

### Dict in-place helpers

- Existing runtime/helper path for dict set/update.
- Remove support must be verified separately from set/update.
- The catalog must record insertion-order and old-version observability
  requirements before emission uses these helpers.

### Record shell update

- The in-place slot is the `can_reuse: Bool` field of `ARecordUpdate(Atom,
  FieldId, Atom, Bool, TypeId)` (`boot/compiler/anf.tw`). Verified: `emit_record_update`
  (`codegen/emit/records.tw`) honors `can_reuse=true` by emitting `struct.set` and
  returning the same ref; `can_reuse=false` copies all fields into a fresh struct.
- The slot is threaded through `slot_assign`/`closure_convert`, but both lower_anf
  construction sites pass `false`, and no pass flips it — so no in-place record
  update is emitted today.
- Shell reuse is distinct from ownership of vector/dict storage stored in fields.

### Ownership-specialized function variants

- No runtime hook is required: this is compiler cloning plus call-site rewriting.
- The original function remains the generic/persistent fallback.
- Clone names should encode or otherwise link to the canonical `VariantId` for
  inspection, while preserving stable internal ids for codegen.
- Recursive and mutually-recursive clones must route in-SCC calls to the matching
  owned clone/peer clone, not accidentally to the generic function.
- [Worked-examples Case V](../analysis/worked-examples.md#case-v--graph_sccvisit-the-whole-compiler-idiom-real)
  is the anchor: `graph_scc.visit`'s generic body stays conservative, and the
  `visit[unique:p0]` clone may consume the variant-qualified shell-reuse verdicts.

### Record-backed field collection updates

- This is a composed use of existing hooks, not a new runtime primitive: project a
  field, call the existing dict/vector mutable helper when field backing ownership
  is licensed, then use the record-shell slot only when shell reuse is separately
  licensed.
- Shell-only acceptance is valid: a record update may reuse the shell even when the
  projected dict/vector update stays persistent.
- Field backing acceptance is path-sensitive and must preserve sibling reads,
  old-version observability, and nested-value publication rules.

## Verification before use

Done for the first-cut families (Phase 7A): exact helper/op names, operand order,
result behavior, ABI, and current emission state are recorded in the verified hook
table above and mapped to targets in [operation-catalog.md](operation-catalog.md).
Verified specifics worth carrying forward:

- The vector in-place target has the **same operand/result shape** as its
  persistent form (`[pvec?, i32, anyref] → [pvec]`), so the rewrite is a
  drop-in call-target swap, not an argument remap.
- All in-place helpers **return the updated reference** (they do not mutate through
  a caller-visible handle), so the result binding is unchanged from the persistent
  form. `rt.arr.set_in_place` mutates the leaf/tail in place and returns the vector;
  the dict helpers mutate the HAMT root and return the dict.
- Typed vector builder shims (`_i64`/`_bool`) are monomorphized forms of the vector
  builder family, selected by existing typed-vector routing, not separate decisions.

For ownership-specialized variants (Phase 8G — not yet inventoried here because no
runtime hook exists; this is compiler cloning + call-site rewriting), still record:

- clone naming and symbol policy;
- call-site rewrite point;
- generic fallback path;
- recursive/mutual-recursive routing behavior;
- cap/fallback behavior when a variant key is unavailable.

## Non-hooks

`@std.buffer` is user-facing workaround scaffolding, not an internal hook for this
track. Its retirement policy lives in
[../migration/buffer-cleanup.md](../migration/buffer-cleanup.md).

`Cell`, `Task`, `Channel`, host I/O, and unknown/indirect calls are publication or
boundary surfaces for ownership analysis, not mutable-lowering targets for this
track. They should stay persistent/conservative unless a later design explicitly
adds a new private intrinsic family.

String slicing/indexing, vector slicing/concat/gather/drop, dict reads, and record
field reads are read/share operations or persistent constructors in the first-cut
catalog. They matter as blockers/borrow evidence for decisions, but are not
standalone mutable targets here.
