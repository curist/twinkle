# Compiler stack-safety for deeply-nested IR

> **Superseded as primary by
> [stack-safety-transform.md](stack-safety-transform.md)** — an ANF→ANF pass that
> auto-rewrites recursion to a heap shadow stack, fixing the compiler *and* user
> programs with one mechanism. This per-pass rewrite remains the **fallback** for
> shapes that transform can't reach. The Phase-2 `prepare.tw` MutVec bailout
> soundness item below is independent of both and still required.

Status: **Paused at green checkpoint**. This branch lands the first stack-safety
work: iterative handling for several deep `Let`/else-`If` paths, temporary safe
fallbacks around remaining recursive hot spots, and clearer CLI stack-overflow
reporting. The `check` path now handles the deep `cond` and side-effecting
statement-sequence repros at much larger sizes. This is not complete:
`build` can still overflow later in backend/codegen walks, and recursive
serializer paths remain future work.

## Problem

The self-hosted compiler runs as a Wasm module in V8 (Deno/Node). Its passes
walk the IR with ordinary recursion, one host call frame per IR nesting level.
Because V8 caps the Wasm execution stack at a fixed size, any sufficiently
**deeply nested** IR overflows the host stack and aborts compilation with
`RangeError: Maximum call stack size exceeded`.

This is **not** a Wasm spec limit (e.g. the 256-arm myth) and **not** specific
to one construct. It is a property of recursive tree-walks over deep IR.

### Reproductions (all overflow around 160–200 nesting levels)

```
# wide cond → nested CoreExpr.If
cond { x == 0 => 0, x == 1 => 1, ... x == 199 => 199, _ => -1 }   # OVERFLOW

# deep hand-written if/else-if (genuinely nested in source)        # OVERFLOW

# long statement sequence WITH SIDE EFFECTS → nested CoreExpr.Let
fn f() Void { println(...); println(...); ...x400... }            # OVERFLOW (ir + build)
```

Even `check` overflows for `cond`/`if`, because `run_check_command` runs the
full `compile_entry_path` pipeline (it lowers).

### Why a long *pure* `let` sequence does NOT overflow

A 400-statement pure-`let` function builds fine — but only because the
**optimizer collapses** the pure bindings before the deep walk. Add side
effects (so the spine can't collapse) and it overflows. So the apparent
"sequences are fine" is misleading; the underlying walk is recursive.

## What is already fixed, and why it was special

`case` literal arm chains were flattened by emitting the arms as a flat sequence.
That worked because `case` lowers all the way
to a flat **Wasm instruction vector** (a single `Block` whose body is a flat
`Vector<Instr>` iterated by a `for`-loop in the serializer), so the emitted
structure is constant-depth. `cond` and statement sequences cannot be flattened
the same way: Core IR has **no flat statement/sequence node** — sequences are
inherently nested `Let`, and `cond` desugars to nested `If` (`lower_core.tw`
`lower_cond`). Flattening `cond` into a `Let`-spine was tried and reverted: the
`Let`-spine is still deep `Let` nesting and overflows identically.

## Mitigations that do NOT work (verified)

- **`--stack-size` (Deno and Node):** the flag is honored only to *lower* the
  limit (e.g. `--stack-size=100` makes a 50-arm `cond` overflow). Raising it
  (8 MB, 32 MB, even 100 MB) does **not** move the threshold — V8 clamps the
  effective Wasm execution stack to a fixed size.
- **`ulimit -s 65520` (max on macOS) + `--stack-size=60000`:** still overflows.
- Node vs Deno: identical behavior.

Conclusion: the host stack cannot be grown enough to matter; the fix must remove
the deep recursion from the passes.

### Why Wasm tail calls are not enough

Twinkle-generated Wasm already uses `return_call` / `return_call_ref` for true
tail-position calls (see [archive/wasm-tail-calls.md](archive/wasm-tail-calls.md)).
That helps tail-recursive programs and any compiler helpers that are genuinely
tail-recursive, but it does not make general tree rewrites stack-safe. Most
problematic walkers recurse into children and then rebuild or combine IR nodes
after the recursive call, so the recursion is not in tail position. Multi-child
nodes such as `If`/`Match` have the same issue. These passes still need explicit
work stacks, trampolines, or flatter IR shapes.

## Root cause: recursive tree-walks in every pass

The IR is walked recursively (depth = IR nesting) in, at least:

- `lower_core` — `lower_expr` / `lower_block` (and the per-kind lowerers)
  recurse into children; `lower_cond` builds a nested `If`. `lower_stmts` has
  been rewritten to build `Let` spines iteratively, so long statement blocks no
  longer spend compiler stack during statement-chain construction.
- Core-level optimization passes (`opt/*`: copy-prop, use-count, uniqueness,
  dce, etc.) — each walks `CoreExpr` recursively.
- `monomorphize`, `core_linker`, `anf_analysis`. Several hot-path walks are now
  explicit worklists: linker function-reference collection and remapping of deep
  `Let`/else-`If` spines, monomorphization collection/rewriting of deep
  `Let`/else-`If` spines, and ANF free/init/assigned/use-count analyses used by
  optimization setup. Other walkers remain recursive.
- `lower_anf` — `CoreExpr` → ANF, still recursive for general trees, but now
  handles deep `Let` spines and else-`If` spines iteratively, including the
  pre-lowering Core max-local/global-mono scans. ANF then has nested `Let`/`If`.
- `backend/prepare` — ANF → `PreparedExpr`; slot lowering now uses an explicit
  work stack for deep `Let`/`AIf` spines, closure-conversion and boundary local
  scans avoid recursive max-local walks, and deep prepared bodies skip the
  typed-vector routing/verifier optimization checks as temporary safe fallbacks.
  Other backend helper walkers remain recursive.
- `codegen/emit` — `emit_expr` recurses on `PreparedExpr` (notably the `AIf`
  else-spine for `cond`/`if`). Some pre-emission collectors and wasm-plan scans
  now use worklists, and `emit_if_op` has a first stack-safe else-if-spine path,
  but deep `cond` build still overflows in emission.
- `codegen/wasm.tw` `encode_instrs_cached` + `collect_ref_funcs_instr`, and
  `codegen/wat.tw` `emit_instr` — recurse on nested `Instr`. (An iterative
  rewrite of the binary serializer was prototyped this session and reverted as
  not-yet-load-bearing; the approach is recorded below.)

The binding constraint shifts as you fix layers: with `case` flat and the
serializer recursive, `cond` dies in lowering/opt (`ir` overflows) well before
codegen. So a partial fix only moves the wall.

## Approach options

1. **Per-pass explicit-stack / trampoline.** Rewrite each recursive tree-walk to
   use an explicit work stack (as the `case` arm-chain and the prototyped binary
   serializer do). Mechanical but pervasive; high surface area; must preserve
   exact output (validated by self-host fixed point + suite).

2. **Shared iterative traversal combinator.** Provide one stack-safe traversal
   (cf. the designed `fold_core_expr` / `fold_children` combinator in
   [archive/fold-core-expr.md](archive/fold-core-expr.md)) and refactor passes
   to express themselves as folds/visitors over it, so fixing the combinator
   fixes every pass that adopts it. Highest leverage, but requires reshaping
   passes into the combinator's shape and only covers passes that adopt it.
   Binding-aware passes (scope-sensitive) need care.

3. **Introduce a flat sequence/multiway node in the IR.** Add a flat
   `Seq(Vector<...>)` and/or `CondChain`/`Switch` node carried Core → ANF →
   Prepared → emit, so sequences and `cond`/`if`-chains are stored flat and
   walked by a `for`-loop. Removes the nesting at the source for the common
   shapes, but touches every pass's node-handling and the type/printer code.

4. **Graceful degradation (cheap, partial).** Detect excessive nesting depth and
   emit a clear diagnostic ("expression nesting too deep; refactor") instead of
   a host `RangeError`. Not a fix, but turns a crash into an actionable error.
   Could ship immediately as a stopgap independent of the real fix.

### Recommended sequence

- **Phase 0 (stopgap):** Option 4 — clear stack-overflow reporting, so users get
  an actionable message rather than a raw Wasm stack trace. The CLI wrappers now
  catch host `RangeError` stack overflows and print a compiler stack-exhaustion
  diagnostic; a pre-pass depth guard remains an option once it can avoid false
  positives on real compiler/test-suite code.
- **Phase 1:** Pick Option 1 or 2 and make the **lowering + opt** passes
  stack-safe first (that's the earliest wall — `check`/`ir` overflow). Kickoff:
  statement-chain lowering, key linker/monomorphize/ANF scans, and lower_anf's
  deep `Let`/else-`If` paths now use explicit worklists. The optimizer also
  detects very deep ANF and skips the recursive optimization suite after defer
  elimination, preserving correctness while avoiding a crash. Continue replacing
  the remaining recursive optimizer walkers so this fallback can be removed.
- **Phase 2:** Make `prepare` + `emit_expr` stack-safe, and finish any remaining
  general-recursion gaps in `lower_anf`. **The `prepare` typed-vector routing
  bailout is now a correctness item, not just a missed optimization — see
  "Phase 2 detail" below.**

### Phase 2 detail: the `prepare` typed-vector routing bailout (now a correctness concern)

`prepare_backend_with_mutable_decisions` (`backend/prepare.tw:203`) guards the
recursive typed-vector analyses with a **module-wide** early return: if *any*
function's prepared depth exceeds 512, the whole module skips
`analyze_typed_repr` + `route_typed_vectors` and falls back to persistent/boxed.
For the ordinary typed-vector path this is a safe (if coarse) missed
optimization. **For MutVec it is a soundness hole:** the MutVec rewrite runs
earlier (codegen) and commits to `mutvec_*` ops whose handle slots have **no
valid boxed form**, so skipping the repr handoff left them boxed and produced a
runtime `illegal cast` (diagnosed 2026-08-03 on branch `mutvec-slice1-phase4`:
`owned indexed set` compiled fine in isolation but the full test suite has a
>512-deep function, tripping the module-wide bailout and force-boxing every
function including the shallow MutVec ones).

Interim stopgap already shipped on `mutvec-slice1-phase4`: `prepare.tw` also
calls `mutvec_repr.assign_mutvec_reprs` inside the bailout branch so handle slots
are never left boxed. It is correct but re-walks deep functions recursively (a
smaller instance of the very risk the bailout exists for). **This phase
supersedes and removes that stopgap.**

Concrete work for this phase (all validated by the **byte-identical WAT** gate —
these are pure traversal-order refactors, so `boot/main.tw`'s emitted WAT must be
unchanged; capture `shasum` before, compare after each commit):

1. Convert the spine-recursive `PreparedExpr` walkers in
   `backend/route_typed_vec.tw` (~30: `build_copy_map`, the `collect_*_results`
   / `collect_*_reads` / `collect_candidates` / `collect_swapped_set_sites`
   families, `classify_expr`, …), `backend/typed_param_abi.tw`
   (`collect_make_closure_sites`, `return_atom_slots_expr`, `analyze_typed_params`,
   `analyze_typed_captures`), and `backend/mutvec_repr.tw` (`seed_expr`,
   `propagate_expr`) from spine recursion to iteration. Recipe (identical to
   `prepared_depth_exceeds_at`): the `.Let(slot, op, body) => …self(body, acc')`
   tail becomes `cur = body` inside a `for !done` loop with a rebound accumulator;
   the base arm sets `done = true`; the `_op` helpers that recurse only into
   control-flow children (`AIf`/`AMatch`/`ALoop`/`ADefer`) stay recursive (bounded
   by nesting, not spine length). One walker per commit so a byte-identical
   regression is bisectable.
2. Narrow `prepare.tw:203` from a module-wide early return to a **per-function**
   guard: thread a `deep_funcs` set (funcs still over a raised, control-flow-only
   limit) into routing so only those functions stay boxed while the rest of the
   module optimizes. `mutvec_repr.assign_mutvec_reprs` stays on the main path
   (now stack-safe after the `seed`/`propagate` conversion).
3. Remove the interim MutVec bailout guard (step above) once (1)+(2) land.

Acceptance for this sub-phase: a deep-spine function coexisting with a claimable
MutVec/typed region compiles without overflow **and** the region still optimizes
(not force-boxed); `TWINKLE_MUTVEC=1` full boot-suite runs with no `illegal
cast`; self-host fixed point holds; byte-identical WAT for non-deep modules.
- **Phase 3:** Make the serializers (`wasm.tw` `encode_instrs_cached` +
  `collect_ref_funcs_instr`; `wat.tw` `emit_instr`) iterative. The binary
  serializer prototype from this session is a good starting point.
- Validate each phase by raising the reproduction's N and confirming the wall
  moved; final acceptance: a 1000-arm `cond` / 1000-statement side-effecting
  function compiles, plus self-host fixed point and full suite green.

### Prototyped serializer approach (for Phase 3 reference)

Make `encode_instrs_cached` iterative with an explicit work stack of
`{ WInstr(Instr, LabelCtx), WByte(Int) }`: handle `If`/`Block`/`Loop` inline
(emit opcode + blocktype, push the body children reversed + the trailing `end`
byte as work items, maintaining `LabelCtx` per item), and delegate leaf
instructions to the unchanged `encode_instr_cached`. `collect_ref_funcs_instr`
becomes a simple worklist over a `Vector<Instr>`. The mutual
`encode_instrs_cached ↔ encode_instr_cached` recursion is what overflows; making
the former iterative breaks it. Byte-output must stay identical (self-host
catches regressions).

## Scope / risk

- Large, cross-cutting; touches the whole pass infrastructure. High blast radius
  (every program's codegen). Each change must preserve exact behavior, validated
  by `make bundle-cli` self-host fixed point + the full boot suite.
- Best done from a clean baseline in a dedicated session, incrementally, with
  the reproductions above as the moving acceptance test.

## Out of scope / non-goals

- Deeply nested *source* (e.g. hand-written 200-level `if/else`) is pathological
  and lower priority; the realistic target is flat-source constructs that
  *desugar* deep (`cond`, long statement sequences).
- Raising the host stack (verified non-viable).
