# Compiler stack-safety for deeply-nested IR

Status: **Planned** (not started). Drafted after the wide-`case`/`cond` overflow
investigation on branch `dense-int-case-lowering`.

## Problem

The self-hosted compiler runs as a Wasm module in V8 (Deno/Node). Its passes
walk the IR with ordinary recursion, one host call frame per IR nesting level.
Because V8 caps the Wasm execution stack at a fixed size, any sufficiently
**deeply nested** IR overflows the host stack and aborts compilation with
`RangeError: Maximum call stack size exceeded`.

This is **not** a Wasm spec limit (e.g. the 256-arm myth) and **not** specific
to one construct. It is a property of recursive tree-walks over deep IR.

### Reproductions (all overflow around ~160–200 nesting levels)

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

`case` literal arm chains were flattened (commit on branch: "emit literal case
arm chains as a flat sequence"). That worked because `case` lowers all the way
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

## Root cause: recursive tree-walks in every pass

The IR is walked recursively (depth = IR nesting) in, at least:

- `lower_core` — `lower_expr` / `lower_block` / `lower_stmts` (and the per-kind
  lowerers) recurse into children; `lower_cond` builds a nested `If`.
- Core-level optimization passes (`opt/*`: copy-prop, use-count, uniqueness,
  dce, etc.) — each walks `CoreExpr` recursively.
- `monomorphize`, `core_linker`, `anf_analysis`.
- `lower_anf` — `CoreExpr` → ANF, recursive; ANF then has nested `Let`/`If`.
- `backend/prepare` — ANF → `PreparedExpr`, recursive.
- `codegen/emit` — `emit_expr` recurses on `PreparedExpr` (notably the `AIf`
  else-spine for `cond`/`if`).
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
   (cf. the designed `fold_core_expr` / `fold_children` combinator, see memory
   `fold-core-expr`) and refactor passes to express themselves as folds/visitors
   over it, so fixing the combinator fixes every pass that adopts it. Highest
   leverage, but requires reshaping passes into the combinator's shape and only
   covers passes that adopt it. Binding-aware passes (scope-sensitive) need care.

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

- **Phase 0 (stopgap):** Option 4 — a depth guard with a real diagnostic, so
  users get a clear message rather than a stack-overflow crash. Small, shippable.
- **Phase 1:** Pick Option 1 or 2 and make the **lowering + opt** passes
  stack-safe first (that's the earliest wall — `check`/`ir` overflow). Validate
  `ir`/`check` no longer overflow on `cond`/`if`/deep-seq.
- **Phase 2:** Make `lower_anf` + `prepare` + `emit_expr` stack-safe.
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
  by `make bundle-cli` self-host fixed point + the full boot suite (2817 tests).
- Best done from a clean baseline in a dedicated session, incrementally, with
  the reproductions above as the moving acceptance test.

## Out of scope / non-goals

- Deeply nested *source* (e.g. hand-written 200-level `if/else`) is pathological
  and lower priority; the realistic target is flat-source constructs that
  *desugar* deep (`cond`, long statement sequences).
- Raising the host stack (verified non-viable).
