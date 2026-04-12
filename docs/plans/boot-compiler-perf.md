# Boot Compiler Performance Investigation

## Baseline (boot/main.tw, 84 modules, Node/Wasm)

```
compile_modules   3735ms   (84 modules — parse/resolve/check/lower per file)
optimize          3383ms   (fixed-point opt pipeline over all functions)
verify            3086ms   (backend IR verifier over all PreparedFuncs)
plan_wasm_types   1397ms
emit_wasm_binary   743ms
emit_module        616ms
link               612ms
prepare_backend    327ms
lower_anf           49ms
core_link           63ms
monomorphize        21ms
closure_convert     11ms
─────────────────────────
total            ~14 000ms
```

Top four bottlenecks account for ~11.6s (83%).  The plan below investigates
each in turn before committing to any fix.

---

## 1. `compile_modules` — 3735ms (84 modules)

Average ~44ms per module.  Each module runs:
parse → resolve → check → lower_core

### Hypotheses

**H1.1 — Per-module env cloning is expensive.**
`compile_module` calls `clone_env(base_env)` and `extend_types_from` for every
module and every dependency fan-out.  If `TypeEnv` / `ValueEnv` hold large
persistent Dicts or Vectors that are fully copied on every call, this adds up
fast across 84 modules with transitive dependencies.

**H1.2 — Prelude modules re-compiled per entry point.**
The 40+ prelude files are loaded and compiled as dependencies for every user
module.  If prelude modules are individually cached but their
parse/resolve/check results are not, each entry point re-does that work.

**H1.3 — Type checker cost grows super-linearly.**
HM unification with MetaVar is amortized O(n) in theory, but the checker's
env lookup (Dict<String, _>) may hash large strings many times, or the
constraint solving may traverse deep type trees repeatedly.

**H1.4 — Hot modules are disproportionately expensive.**
A few large modules (resolver.tw, checker.tw, emit.tw) may dominate.  We
cannot tell yet because `compile_modules` is a single aggregate timing.

### Investigation

- Add per-module timing inside `compile_module` (log to stderr when
  `TWINKLE_TIMINGS=1`).  Format: `[time] module <path>: <ms>ms`.
- Add sub-step timing inside `compile_module`: separate timers for parse,
  resolve, check, lower_core.
- Count env clone size: log `base_env.types.len()` and `env.functions.len()`
  at the start of each module to see if the environment explodes.

---

## 2. `optimize` — 3383ms

`optimize_module` runs a fixed-point loop (cap 10 rounds) of four passes over
every function, then three post-loop passes.

### Hypotheses

**H2.1 — Many functions run close to the 10-round cap.**
If most functions need 8–10 rounds to converge, the total work is ~10×
the single-pass cost.  The loop exits early only when all four passes
report `changed = false`.

**H2.2 — `collect_free_locals` / `collect_bound_locals` are called on every round.**
`compute_pinned` runs once per module, but each round of `copy_prop` and
`dead_let_elim` may do their own internal AST traversals that are O(AST size).
For large functions (e.g. the boot compiler's own codegen helpers), this
could be the dominant term.

**H2.3 — `uniqueness_rewrite` is expensive on large functions.**
The uniqueness pass (Phase 1–2 complete) does a pre-scan + forward walk with
a `HashSet` of unique locals.  On the fully monomorphized module it sees
all specialised copies, so the total work scales with the number of
monomorphic function variants.

**H2.4 — `annotate_in_place` runs on every function unconditionally.**
Even functions with no record-update ops pay the traversal cost.

### Investigation

- Count rounds per function: add a counter and log functions that hit the cap
  or that need > 3 rounds (those are where the fixed-point is slow to close).
- Time each of the four pipeline passes individually across the module (sum
  across all functions).
- Time `annotate_in_place` and `uniqueness_rewrite` separately from the loop.
- Report function count and total node count (a proxy for AST size) at the
  start of `optimize_module`.

---

## 3. `verify` — 3086ms

`verify_prepared_module` walks every `PreparedFunc`'s full body expression
tree, checking slot membership, metadata, and expression shapes.

### Hypotheses

**H3.1 — Verify does redundant work that prepare_backend already enforces.**
`prepare_backend` constructs the PreparedModule.  If its invariants are strong
enough, many verify checks are guaranteed-true by construction and pay the
traversal cost for no diagnostic benefit in a correct build.

**H3.2 — `slot_info` / `verify_slot_membership` is O(slots) per call.**
If `pf.slots` is a `Dict<Int, SlotInfo>` these lookups should be O(1), but
if the dict is implemented as a sorted vector or if key hashing is slow for
integer keys, it could be O(n) or have high constant factor.

**H3.3 — `verify_unique_source_local` builds a reverse map on every call.**
If the uniqueness check scans `pf.slots` linearly for each slot to prove
no two slots share the same source local, that is O(slots²) per function.

**H3.4 — Verify is O(total IR nodes), which is large after monomorphization.**
After mono, each generic function is expanded into N copies.  Verify sees all
of them.  The total IR node count may be large enough that even a fast linear
pass takes seconds.

### Investigation

- Add a counter for total slots visited and total expression nodes visited
  across the whole module, logged under `TWINKLE_TIMINGS`.
- Profile which check inside `verify_prepared_func` is the hot path by adding
  per-check sub-timers (slot checks vs. expression walk).
- Measure the function count and average slot count per function going into
  verify.
- Consider making verify opt-in (e.g. `TWINKLE_VERIFY=1`) so it can be
  skipped in production builds while remaining available for CI.

---

## 4. `plan_wasm_types` — 1397ms

`plan_wasm_types` scans all prepared bodies and builds the `WasmTypeRegistry`:
registers mono types, string pool entries, runtime imports, closure func sigs.

### Hypotheses

**H4.1 — `register_mono` is called for every slot in every function.**
For a large module with many monomorphic copies, this means O(functions ×
avg_slots) calls.  Each `register_mono` recursively walks the `MonoType` tree
and inserts into `WasmTypeRegistry` dicts.

**H4.2 — `register_runtime_import` scans `runtime_imports` linearly.**
The deduplication check iterates the whole imports vector on every call.
If there are many distinct builtin call sites this is O(builtins²).
(Already uses `as_sym` string comparison — likely not the dominant cost but
worth confirming.)

**H4.3 — `topo_sort_type_defs` is O(n²).**
The topo sort in `wasm_plan_impl.tw` may use a naive dependency traversal
that is quadratic in the number of struct types registered.

**H4.4 — String interning in `register_string` hashes large WAT strings.**
String pool entries include a getter symbol name built by interpolation, which
is hashed on every `dict.set` call.

### Investigation

- Count the total number of `register_mono` calls and unique types registered.
- Time `topo_sort_type_defs` separately (it is a single call at the end of
  `plan_wasm_types`).
- Count `runtime_imports.len()` at the point of each `register_runtime_import`
  call to confirm whether the linear scan is a real cost.
- Log total string pool size at the end of planning.

---

## 5. Remaining steps (< 750ms each)

`emit_wasm_binary` (743ms), `emit_module` (616ms), `link` (612ms) are
significant but secondary.  Investigation can wait until the top four are
addressed.  Quick notes:

- **emit_wasm_binary**: encodes the full linked Wasm binary.  Cost is
  proportional to module size (instruction count × LEB128 encoding).  Likely
  hard to improve without shrinking the module first.
- **emit_module**: emits WAT instructions from PreparedIR.  One pass over all
  functions; cost follows IR size.
- **link**: DCE + renumber + merge.  May have O(n²) in symbol resolution if
  the import/export tables are scanned linearly per reference.

---

## Suggested investigation order

1. Add per-module + per-sub-step timing to `compile_module` — quick win to
   identify hot modules and whether clone_env or type-checking dominates.
2. Add round-count logging and per-pass timing to `optimize_module` — reveals
   whether the bottleneck is the fixed-point convergence or the post-loop passes.
3. Add slot/node count logging to `verify_prepared_module` — determines whether
   3s is unavoidable given IR size or whether specific checks are pathological.
4. Add type-registration count and topo-sort timer to `plan_wasm_types`.

All four can be added behind `TWINKLE_TIMINGS=1` incrementally without
changing semantics.  Once the root cause per step is confirmed, a separate
fix plan can be drafted for each.
