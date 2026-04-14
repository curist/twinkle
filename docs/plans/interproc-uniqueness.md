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

## Implementation Order

Both the stage0 Rust optimizer (`src/opt/uniqueness.rs`) and the boot compiler
optimizer (`boot/compiler/opt/uniqueness.tw` + `boot/compiler/opt/analysis.tw`)
need both changes, but stage0 is what matters for current boot-compiler perf
since boot/main.tw is compiled by stage0.

1. **Gap 1 in stage0**: add `ARecordGet` case in the forward pass — if base is
   unique/refreshed and dead after, mark extracted value as Shallow unique

2. **Gap 2 in stage0**: add a pre-pass collecting consume evidence across all
   functions; thread "consumed params" set into per-function uniqueness init

3. **Measure**: rebuild boot/main.tw, check `struct.new UserRecord_105` count in
   WAT drops from ~15 definitions firing millions of times to near zero

4. **Mirror to boot compiler**: same changes in the boot optimizer (affects
   stage3+ self-hosted compilations)

## Expected Impact

Concrete hot functions in checker.tw:
- `set_type_map_span`: 1 `struct.new` (11 `struct.get`) per expression → `struct.set`
- `set_subst`: 1 `struct.new` per unification → `struct.set`
- `fresh_meta`: 1 `struct.new` per MetaVar → `struct.set`

With ~10k+ expressions per large module and 3–5 of these fires per expression,
this eliminates O(10k–50k) `struct.new` allocations per large module compilation.
Same pattern exists in lower_core.tw, emit.tw, parser.tw.

Estimated reduction in compile_modules: 30–50%.

Goal: functional struct-threading becomes naturally efficient — no Cell wrappers,
no manual mutation, no code changes to checker.tw or other modules.
