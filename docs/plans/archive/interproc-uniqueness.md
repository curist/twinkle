# Interprocedural Uniqueness: Consume-at-Call-Site Propagation

## Relation to Existing Plans

This extends two existing plans:

- `static-uniqueness-plan.md` Phase R4 ("broader helper summaries") — currently
  scoped to tiny local wrappers; this plan proposes going cross-function-boundary
- `boot-uniqueness-deep-ownership.md` — establishes the Shallow/Deep ownership
  distinction; this plan works within that model and is specifically about
  **shell reuse** (`can_reuse_in_place`), not deep collection mutation

## Problem

Parameters are unconditionally tainted. Every call to a struct-updating helper
does a full struct rebuild even when the caller clearly passes ownership:

```tw
// checker.tw — called once per expression checked:
fn set_type_map_span(ctx: InferCtx, id: Int, span_start: Int, ty: MonoType) InferCtx {
  ctx.type_map[id] = ty      // ctx is param → tainted → 12 struct.get + struct.new
  ctx.expr_spans[id] = span_start  // 2nd update on same local → struct.set (free)
  ctx
}
```

The call sites all pass `ctx` as consumed:
```tw
cur_ctx = set_type_map_span(cur_ctx, ...)     // consume-reassign
.{ ctx: set_type_map_span(r.ctx, ...), ... }  // r.ctx extracted from dying r
```

The optimizer can see the consume-reassign pattern for `ARecordUpdate` directly,
but not when it is hidden inside a function call. Inside `set_type_map_span`, the
parameter `ctx` is tainted regardless of how the caller uses it.

Same applies to `set_subst` and `fresh_meta` — each fires `struct.new` on a
12-field InferCtx on every call. These are the dominant allocation cost in
compile_modules (3558ms), affecting checker.tw (416ms), emit.tw (511ms),
lower_core.tw (301ms), and parser.tw (323ms).

## Two Gaps

### Gap 1 — Field extraction from a dying fresh struct

```tw
fn synth(ctx: InferCtx, expr: Expr, diags: ...) SynthOut {
  r := ctx.synth_inner(expr, diags)       // r: fresh SynthOut
  set_type_map_span(r.ctx, ...)           // r is dead after r.ctx is extracted
}
```

`r` is fresh (newly allocated SynthOut) and dead after `r.ctx` is used.
The extracted `r.ctx` is the sole reference to that InferCtx — it is unique for
**shell-reuse purposes** — but the optimizer does not track this.

Note: this is distinct from deep ownership. Propagating shell-uniqueness through
`ARecordGet` is sound under the `boot-uniqueness-deep-ownership.md` model because
shell reuse (`can_reuse_in_place`) has weaker proof requirements than deep
collection mutation. The extracted InferCtx's inner dict fields are still treated
as tainted for in-place dict/vector rewrites.

**Fix:** in the forward uniqueness pass, when processing `ARecordGet(base, field)`:
if `base` is in `unique` (or `refreshed`) AND `base` is not live after this
binding, mark the extracted value as `Shallow`-unique (eligible for shell reuse,
not deep mutation).

### Gap 2 — Parameters unconditionally tainted

Even with Gap 1 fixed, inside `set_type_map_span(ctx: InferCtx, ...)`, `ctx` is
a parameter — always tainted. The fix for Gap 1 only helps call sites in `synth`
where `r.ctx` is fresh. It doesn't help when `ctx` is itself a param of the
calling function (the majority of call sites, inside `synth_inner`, `check_expr`,
`unify`, etc.).

**Fix:** interprocedural consume analysis — if every static call site of a
function passes argument `i` as a consumed unique local, initialize parameter `i`
as unique rather than tainted.

**Consumed at call site** means:
1. The argument is `ALocal(x)` (not literal/global/field-access)
2. `x` is in the `unique` set at the call point (or `refreshed`)
3. `x` is not live after the call — either consume-reassign pattern
   (`x = f(x, ...)`) or simply dead

**Algorithm:**

Pre-pass over all functions: for each `ACall(f, args)`, record for each arg
position whether that call site consumes. If ALL call sites of `f` consume
argument `i`, then when running the uniqueness pass for `f`, initialize
parameter `i` as `unique` (Shallow) instead of tainted.

Handling edge cases:
- **Recursive calls** that pass the original (non-refreshed) parameter count as
  non-consuming → parameter stays tainted in recursive functions
- **Indirect / value-position uses** of `f` (stored in dict, passed as closure
  arg) → unknown call sites → parameter stays tainted
- **Multiple modules**: this analysis is per-module (after monomorphization)

## Implementation Status

### Gap 1 — DONE (2026-04-15)

Implemented in both optimizers:
- **stage0** (`src/opt/uniqueness.rs`): `ARecordGet` case in `rewrite_expr` — if
  base is unique/refreshed and dead after, extracted value is marked unique
  (+ refreshed if tainted). Source_fresh intentionally NOT set, so dict/vector
  in-place ops remain gated.
- **boot compiler** (`boot/compiler/opt/uniqueness.tw`): matching `ARecordGet` case
  in `rewrite_expr`, using `local_has_shell_ownership` + `live_after_by_binding`.

Measured impact: REC_UPDATE_IN_PLACE 23 → 36 on boot/tests/main.tw (boot compiler).
Small direct improvement; main value is as a prerequisite for Gap 2.

### Gap 2 — DONE (2026-04-17, boot compiler only)

Implemented via structural consume-reassign detection in the pre-pass + `consumed_params`
OR-check in `rewrite_reusable_update`. Boot compiler only (`boot/compiler/opt/`).

**Final approach** — do NOT seed consumed params into `ownership`/`unique`; instead:
1. **Pre-pass** (`boot/compiler/opt/analysis.tw`: `collect_consumed_params`): walk all
   functions; for each external (non-self-recursive) `ACall(callee, args)`, detect each
   argument position using structural `is_consume_reassign` (no uniqueness check needed).
   A param index is "consumed" if it has NO non-consuming external call sites AND has at
   least one external call site.
2. **Rewrite** (`boot/compiler/opt/uniqueness.tw`: `rewrite_reusable_update`): in the
   `ARecordUpdate` case, allow shell-reuse if `local_has_shell_ownership(st, base)` OR
   `set_has(consumed_params, base.id)`. Consumed params remain tainted everywhere else
   (Gap 1 cannot propagate deep ownership through them).
3. **Early-exit bypass**: skip `has_rewritable_cow_op_in_expr` check when
   `consumed_params.len() > 0`, since that check only finds non-tainted bases.
4. **Pipeline** (`boot/compiler/opt/pipeline.tw`): call `collect_consumed_params(module)`
   before per-function loop; thread `f_consumed` into
   `uniqueness_rewrite_with_semantics_and_consumed`.

**Why structural detection is sufficient**: `is_consume_reassign(body, base, result)` checks
if `body` immediately starts with `assign(base = result)`. No uniqueness-at-call-point check
needed because: (a) if the pattern holds at ALL external call sites, the param IS consumed;
(b) we're not seeding into `unique`, so no Gap 1 bleed-through.

**Soundness**: consumed params are only checked in the shell-reuse (`ARecordUpdate`) guard.
They are NOT in the `ownership` set, so `ARecordGet` on them does NOT propagate shell
ownership to extracted fields → deep collection mutation (dict/vector in-place ops) is
still gated correctly.

**Previous attempts that failed**:
- Seed as `unique + refreshed`: `refreshed` bypasses taint gate for dict/vector → soundness bug
- Seed as `unique` only: Gap 1 propagates shell ownership to extracted fields → same bug

**Measured impact** (boot compiler self-compiling boot/main.tw, 85 modules):
- `compile_modules`: 506ms → 452ms (**~11% reduction**)
- `uniqueness` optimizer time: 97ms → 45.8ms (**~53% reduction**)
- `checker.tw` check phase: 18.1ms → 12.5ms
- Boot compiler self-compilation verified correct (stage2.wasm)

The 30–50% compile_modules projection was partially met. Remaining gap: some call sites of
hot functions (`set_type_map_span`, `set_subst`, `fresh_meta`) may not satisfy the structural
consume-reassign pattern when `ctx` is passed through branches or returned in a record field
rather than directly reassigned. A follow-up could examine those call sites.

## Expected Impact (Gap 2, measured)

Concrete hot functions in checker.tw:
- `set_type_map_span`: 1 `struct.new` (11 `struct.get`) per expression → `struct.set`
- `set_subst`: 1 `struct.new` per unification → `struct.set`
- `fresh_meta`: 1 `struct.new` per MetaVar → `struct.set`

Achieved: ~11% compile_modules improvement, ~53% uniqueness optimizer speedup.
Goal of functional struct-threading without Cell wrappers/manual mutation: achieved for
the common consume-reassign pattern. Remaining call sites that don't fit the structural
pattern would require option 2 (spine-level taint refinement) as a follow-on.
