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

## Findings (post fine-grained instrumentation)

```
[time:mod] boot/compiler/checker.tw    total=701ms  check=411ms  lower=61ms
[time:mod] boot/compiler/lower_core.tw total=468ms  check=329ms  lower=57ms
[time:mod] boot/compiler/emit.tw       total=637ms  check=518ms  lower=76ms
[time:mod] boot/compiler/parser.tw     total=409ms  check=308ms
[time:mod] boot/compiler/pipeline.tw   total=3167ms deps=3158ms  (almost all dep resolution time)
[time:mod] boot/compiler/codegen.tw    total=1485ms deps=1475ms  (same)

[time:opt] funcs=1354  total_rounds=3063  avg_rounds=2.26  at_cap=13
[time:opt] dead_let=202ms  copy_prop=167ms  const_fold=4ms  branch_simp=3ms
[time:opt] annotate_in_place=0.9ms  uniqueness=3001ms  defer_elim=3ms

[time:verify] funcs=1354  total_slots=62185  slot_entries=62185
[time:verify] slot_checks=2971ms  expr_walk=95ms

[time:plan] funcs=1354  slot_reg_calls=63539  unique_types=402  strings=1484
[time:plan] scan=45ms  register_slots=58ms  register_sigs=1362ms  topo_sort=4ms
```

Key findings:
- **compile_modules**: dominated by a handful of large modules (checker.tw 411ms check, lower_core.tw 329ms, emit.tw 518ms). Most time is `check` inside those modules. The deps column for pipeline.tw/codegen.tw just propagates the big modules' costs up.
- **optimize**: `uniqueness_rewrite` alone takes 3001ms — virtually all of the 3383ms total. dead_let+copy_prop are secondary (370ms combined). avg rounds is only 2.26, so the fixed-point loop is not the problem.
- **verify**: slot checks dominate (2971ms vs 95ms for expr walk). With 62185 slots across 1354 functions, this points to an O(slots²) pattern inside `verify_unique_source_local`.
- **plan_wasm_types**: `register_sigs` takes 1362ms of 1471ms total. This is `register_func_sig_by_id` + `register_higher_order_global_func_sigs` + `analyze_builtin_call`. The slot registration itself is only 58ms.

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

### Findings

Per-module sub-step timing shows `check` dominates in the large modules:
checker.tw (411ms), emit.tw (518ms), lower_core.tw (329ms), parser.tw (308ms).
The `deps` column for top-level files like pipeline.tw is just recursively
accumulated cost from those modules, not new work.

### Investigation

- Add env size logging at the start of each module's check pass (how many
  functions and types are in scope) — large envs could explain why check is
  slow in the big files.
- Profile whether it's HM unification depth, Dict lookup count, or sheer
  expression count driving the check cost in those four modules.

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

### Findings

`uniqueness_rewrite` is 3001ms out of 3383ms total — it alone IS the
optimize bottleneck.  The fixed-point loop is cheap: avg 2.26 rounds,
only 13 functions hit the cap.  dead_let+copy_prop together are 370ms,
a secondary target.

### Investigation

- Profile `uniqueness_rewrite` internals: the pre-scan (`collect_local_refs`)
  and forward walk both traverse the full AST.  For 1354 monomorphized
  functions the cumulative size is large.
- Check whether most functions produce zero rewrites — if so, a cheap
  pre-check (does this function contain any VECTOR_SET_UNSAFE?) could skip
  the pass entirely for the majority.
- Investigate dead_let+copy_prop: both do full AST traversals.  A combined
  single-pass could halve that cost.

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

### Findings

The split is decisive: slot_checks=2971ms vs expr_walk=95ms.
With 62185 slots across 1354 functions (avg 45 slots/func), and
slot_entries=62185 (1:1 ratio of slots to entries), the cost is inside
the slot-check loop, not the expression walk.

`verify_unique_source_local` is the prime suspect: it likely scans all
slot entries to prove no two slots share the same source local, giving
O(slots²) per function.  For avg 45 slots, that is ~2000 comparisons
per function × 1354 functions = ~2.7M comparisons.

### Investigation

- Confirm `verify_unique_source_local` is O(slots²) by reading its body.
- Fix: build a `seen_source_locals: Dict<Int, Bool>` once per function
  (O(slots)) instead of re-scanning for each slot.
- Secondary: consider making verify opt-in behind `TWINKLE_VERIFY=1` for
  release builds.

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

### Findings

`register_sigs` takes 1362ms of 1471ms total (93%).  This covers
`register_func_sig_by_id` for closure/ho-global funcs and
`register_higher_order_global_func_sigs` + `analyze_builtin_call`.
Slot registration (63539 calls) takes only 58ms — MonoType registration
itself is cheap.  topo_sort is 4ms (not a problem).

### Investigation

- Add sub-timers inside register_sigs to split: closure_funcs registration
  vs ho_global_funcs vs `register_higher_order_global_func_sigs` vs
  `analyze_builtin_call`.
- `register_higher_order_global_func_sigs` likely iterates all functions
  looking for higher-order usages — check if it is O(funcs²) or uses a Dict.
- `analyze_builtin_call` calls `register_runtime_import` which has a linear
  dedup scan; with many distinct builtins this could be O(builtins²).

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
