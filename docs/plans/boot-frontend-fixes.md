# Boot Frontend — Fixes, Completeness & Refactoring

**Scope:** `boot/compiler/resolver.tw`, `boot/compiler/checker.tw`, and their test suites
**Depends on:** M1–M9 of [boot-type-checker.md](boot-type-checker.md) and M1–M4 of [boot-resolver-method-registry.md](boot-resolver-method-registry.md)
**Context:** Post-review findings from the resolver and type checker implementations

---

## Overview

Code review of the boot resolver and checker surfaced bugs, completeness gaps, and refactoring opportunities. This plan covers everything found — no item is omitted regardless of severity. Phases group items by nature (correctness → completeness → refactoring → tests), not by priority.

---

## Phase 1 — Correctness Fixes

### F1 — Resolver: align param_names and param_types on error recovery ✅

**File:** `resolver.tw:402–419`

When a parameter's type annotation fails to resolve, `param_names` still gets the name but `param_types` skips the entry. This creates a length mismatch: `fn foo(x: Bad, y: Int)` produces `param_names = ["x", "y"]` and `params = [Int]`. The checker indexes both by position (`sig.param_names[i]` / `sig.params[i]` at `checker.tw:1617–1618`), so this causes an index-out-of-bounds trap on error-recovery paths.

**Fix:** When a param type fails to resolve, push `MonoType.Void` as a placeholder into `param_types` so both vectors always have the same length.

**Tests:**
- `fn foo(x: Nonexistent, y: Int) Int { 0 }` — resolve produces diagnostic, `sig.params.len() == sig.param_names.len() == 2`
- Checker doesn't trap when checking a call to such a function

### F2 — Checker: Bool exhaustiveness always reports non-exhaustive ✅

**File:** `checker.tw:1190`, `checker.tw:1161–1163`

`get_variant_names` returns `["true", "false"]` for `Bool`, but the covered-variant collector at lines 1161–1163 only matches `Variant` and `QualifiedVariant` patterns — never `Literal`. So `case b { true => ..., false => ... }` always reports non-exhaustive.

**Fix:** Two options:

- **(a) Extend the collector** to recognize `Literal(BoolLit(true))` as covering `"true"` and `Literal(BoolLit(false))` as covering `"false"`.
- **(b) Remove Bool from `get_variant_names`** and rely on the wildcard/ident early-return for exhaustiveness. This is simpler and matches the fact that Bool patterns are parsed as literals, not variants.

Recommend **(a)** since it catches genuinely non-exhaustive Bool matches (e.g., `case b { true => ... }` missing `false`).

**Tests:**
- `case b { true => 1, false => 0 }` — no diagnostic
- `case b { true => 1 }` — non-exhaustive diagnostic
- `case b { true => 1, _ => 0 }` — no diagnostic (wildcard)

### F3 — Checker: `try` doesn't validate enclosing return type ✅

**File:** `checker.tw:1408–1429`

`try` on `Option` should only be valid inside a function returning `Option`; `try` on `Result` should only be valid inside a function returning `Result`. Currently the checker ignores `ctx.current_ret` entirely.

**Fix:** After determining the `try` flavor (Option vs Result), check `ctx.current_ret`:
- `try` on `Optional(T)`: `current_ret` must be `Optional(_)` (unify outer with `Optional(fresh_meta)`)
- `try` on `Result(T, E)`: `current_ret` must be `Result(_, E)` (unify error types)
- If `current_ret` is `None` (top-level / unannotated), emit a diagnostic

**Tests:**
- `try` on `Option` in `Option`-returning function — ok
- `try` on `Option` in `Result`-returning function — diagnostic
- `try` on `Result` in `Int`-returning function — diagnostic

**Done:** The Result error-type unification now threads `u.ctx` through to the returned `SynthOut`, preserving MetaVar substitutions.

### F4 — Checker: inferred return types not written back to env ✅

**File:** `checker.tw:1631–1633`

When a function has no return type annotation (`sig.ret = .None`), `check_function` synthesizes the body but discards the result type. Other functions calling it see `Void` instead of the actual type.

**Fix:** After `synth_block` for an unannotated function, update the function's sig in `ctx.env` with the inferred return type. This requires updating the `FunctionSig` in `env.functions` and propagating the updated env.

**Caveat:** This only works if functions are checked in dependency order, or if a two-pass approach is used (infer sigs first, then check bodies). For now, a single-pass approach with source-order checking is acceptable — mutual recursion between unannotated functions remains unsupported. Document this limitation.

**Tests:**
- `fn double(x: Int) { x * 2 }` followed by `fn use_it() Int { double(3) }` — `use_it` returns `Int`, not `Void`
- `fn no_ret() { }` — inferred as `Void`

**Done:** Comment added in `check_function` documenting the source-order limitation.

### F5 — Checker: `QualifiedVariant` path discarded in pattern matching and exhaustiveness ✅

**File:** `checker.tw:1053, 1162–1163`

`check_pattern` extracts just the `name` from `QualifiedVariant(path, name, sub_pats)`, discarding the qualifier. In exhaustiveness checking, covered-name collection also only uses the bare name. This means `Color.Red` and `Shape.Red` are treated identically — if two sum types share a variant name, the checker could wrongly attribute a pattern to the wrong type.

**Fix:** When the scrutinee type is known (it always is in `check_variant_pattern`), validate that the qualifier path matches the scrutinee's type name. If the qualifier doesn't match, emit a diagnostic. For exhaustiveness, only count a `QualifiedVariant` as covering a variant if the qualifier matches the scrutinee type.

**Tests:**
- `case c { .Red => 1 }` where `c: Color` and Color has Red/Green/Blue — works as before
- `case c { Color.Red => 1, Color.Green => 2, Color.Blue => 3 }` — exhaustive, no diagnostic
- `case c { Shape.Red => 1 }` where `c: Color` — diagnostic (wrong qualifier)

**Done:** `validate_qualifier` checks qualifier path against scrutinee type name, emitting diagnostic on mismatch. Exhaustiveness filtering updated to only count `QualifiedVariant` as covering when qualifier matches.

### F6 — Resolver: circular alias detection doesn't follow through wrapper types ✅

**File:** `resolver.tw:861–869`

`is_circular_alias` only follows `Named(tid, _)` targets. An alias like `type A = A?` resolves to `Optional(Named(id_of_A, []))`, and the `Optional` wrapper stops the traversal — the cycle is undetected. Similarly for `type A = Vector<A>`, `type A = fn() A`, etc.

**Fix:** Extend `is_circular_alias` to recurse into `Optional(inner)`, `Vector(inner)`, `Dict(k, v)`, `Result(ok, err)`, and `Function(params, ret)`, checking all inner types for cycles. The function already has a visited set to prevent infinite loops.

**Tests:**
- `type A = A?` — circular alias diagnostic
- `type A = Vector<A>` — circular alias diagnostic
- `type A = fn() A` — circular alias diagnostic
- `type A = Vector<Int>` — no diagnostic (not circular)

### F7 — Resolver: duplicate type parameter names not validated ✅

**File:** `resolver.tw:336, 400`

`type Foo<T, T> = ...` is silently accepted. `resolve_single_name` always resolves `T` to the first occurrence, so the second parameter is unreachable. Similarly for `fn bar<A, A>(...)`.

**Fix:** Before processing type params, check for duplicates and emit a diagnostic. A simple linear scan of the `type_params` vector is sufficient.

**Tests:**
- `type Foo<T, T> = .{ a: T }` — diagnostic "duplicate type parameter T"
- `fn bar<A, A>(x: A) A { x }` — diagnostic
- `type Foo<A, B> = .{ a: A, b: B }` — no diagnostic

### F8 — Checker: `check_function` context fragility — `current_ret` not restored ✅ (tested, not yet hardened)

**File:** `checker.tw:1622–1623, 1629/1633`

`check_function` mutates `fn_ctx.current_ret` and `fn_ctx.type_var_scope` via COW field assignment, but `pop_scope` only drops the scope frame — it doesn't restore `current_ret` or `type_var_scope`. The returned `ctx` carries the inner function's `current_ret`. Currently safe because each function starts from `empty_ctx` in the top-level loop, but fragile if function checking ever becomes nested (e.g., for closures calling back to `check_function`).

**Fix:** Save `current_ret` and `type_var_scope` before entering the function, restore them after `pop_scope`. Alternatively, since `check_function` already pushes/pops a scope, bundle these values into the scope or snapshot/restore pattern:

```tw
saved_ret := fn_ctx.current_ret
saved_tvs := fn_ctx.type_var_scope
// ... check body ...
result_ctx.current_ret = saved_ret
result_ctx.type_var_scope = saved_tvs
```

**Tests:**
- Two functions checked sequentially — second function's `current_ret` is independent of first's

**Status:** Tested and currently safe (each function starts from top-level ctx). Save/restore not yet implemented — should be added before nested function checking (closures) is wired through `check_function`.

### F9 — Checker: `check_return` doesn't produce `Never` type ✅

**File:** `checker.tw:1590–1607`

After a `return` statement, the checker continues type-checking subsequent statements without marking control as diverged. `check_stmt` returns `CheckOut` (no type), so there's no direct type confusion, but the type map doesn't record `return` as `Never`, and dead code after `return` is silently accepted.

**Fix:** Record `Never` in the type map for the `return` statement's span. This is informational — the checker already doesn't propagate types from statements, so no behavioral change is needed beyond the type map entry.

**Tests:**
- `return 42` — type map records `Never` at the return's span

**Done:** `stmt_diverges`/`expr_diverges`/`block_diverges` helpers detect divergence through `return`, `error()` calls, and `if/else` where all branches diverge. `synth_block` returns `Never` for diverging blocks. `check_return` records `Never` in the type map at the return's span.

---

## Phase 2 — Completeness

### C1 — Type alias expansion in field access and pattern matching ✅

**Files:** `checker.tw:557` (synth_field), `checker.tw:785` (check_variant_lit), `checker.tw:1132` (check_variant_pattern)

When a `Named(tid, args)` type resolves to an `Alias`, the checker emits "field access on non-record type" or falls through to a generic error instead of expanding the alias to its target type.

**Fix:** Add an `expand_alias` helper that, given a `Named(tid, args)` type, checks if the def is `Alias(_, params, target)`, applies `subst_vars(target, build_var_map(params, args))`, and returns the expanded type. Call this at the top of `synth_field`, `check_variant_lit`, and `check_variant_pattern` before structural matching. Guard against infinite expansion with a depth limit (aliases can't be circular — the resolver already rejects those).

**Tests:**
- `type Pt = .{ x: Int, y: Int }; type MyPt = Pt` — `p.x` where `p: MyPt` → `Int`
- `type MaybeInt = Int?` — `case m { .Some(x) => x, .None => 0 }` where `m: MaybeInt` → `Int`

### C2 — `element_type_of` for Iterator and Range ✅

**File:** `checker.tw:1254–1266`

Currently falls back to `MonoType.Int` for anything that isn't Vector, String, or Dict. This silently mis-types `for x in iter` where `iter: Iterator<String>`.

**Fix:** Add cases for:
- `Named(tid, [elem])` where `tid` matches the Range TypeId → `Int` (Range always iterates Int)
- `Named(tid, [elem])` where `tid` matches the Iterator TypeId → `elem`
- Otherwise → emit a diagnostic and return `fresh_meta`

TypeIds for Range and Iterator are known builtins (Range=TypeId(3), Iterator=TypeId(4) per MEMORY.md). The checker can compare `tid.id` directly or look up by name in the env.

**Tests:**
- `for x in range(10) { x }` — `x: Int`
- `for item in iter` where `iter: Iterator<String>` — `item: String`
- `for x in 42` — diagnostic

### C3 — `Break`, `Continue`, `Defer`, `Assign` handling in check_stmt ✅

**File:** `checker.tw:1570`

These are silently dropped by the `_ =>` catch-all. `Break` and `Continue` should be recognized (and could produce `Never` type if needed). `Defer` should type-check its body expression. `Assign` should validate the assigned value matches the target's type.

**Fix:**
- `Break` / `Continue` — add explicit arms; type-check any value expression on `break val`.
- `Defer` — synth the deferred expression to catch type errors in it.
- `Assign` — synth the value and unify with the target's type (look up target in locals).

**Tests:**
- `for x in xs { if x > 10 { break } }` — no error
- `defer println("done")` — no error
- `defer 42 + true` — type error diagnostic
- `x = 42` where `x: Int` — no error
- `x = "hi"` where `x: Int` — type mismatch diagnostic

### C4 — Top-level statement checking ✅

**File:** `checker.tw:1647–1655`

The `check` entry point only dispatches on `.Function` items; `.Stmt` items are silently ignored.

**Fix:** Add a `.Stmt(stmt)` arm that calls `check_stmt` on the top-level statement. Top-level scope is the outermost scope frame (already pushed by `empty_ctx`).

**Tests:**
- `x := 42\nprintln(x)` — no error, type map populated
- `x: Bool = 42` — type mismatch diagnostic

### C5 — `synth_call_general` closure call asymmetry

**File:** `checker.tw:514–536`

When the callee is not a direct ident (e.g., a closure stored in a local), args are synthesized (synth mode) rather than checked against parameter types. This means anonymous record/variant literals as arguments fail.

**Fix:** After synthesizing the callee and determining it has `Function(param_tys, ret)` type, re-check each arg in check mode against the corresponding `param_tys[i]`. If the callee type is still a MetaVar after synthesis, fall back to synth mode for args (current behavior).

**Tests:**
- `f := fn(x: Int) Int { x + 1 }; f(5)` — returns `Int`
- Closure-as-value passed anonymous record literal — should work with the fix

### C6 — Exhaustiveness: list missing variants in diagnostic ✅

**File:** `checker.tw:1177`

Currently emits a generic "non-exhaustive match" message without listing which variants are uncovered.

**Fix:** Build a comma-separated string of `missing` variant names and include it: `"non-exhaustive match, missing: .Foo, .Bar"`.

**Tests:**
- `case shape { .Circle(r) => r }` where Shape has Circle/Rect/UnitSquare → diagnostic mentions `.Rect, .UnitSquare`

### C7 — Named record constructor form (`Point.{ x: 1 }`) in synthesis mode

**File:** `checker.tw:439`

Anonymous `.{ x: 1 }` in synth mode falls through to "cannot synthesize type for this expression". Named constructors like `Point.{ x: 1, y: 2 }` may also fail depending on how the parser emits them. If it's parsed as a qualified path + record literal, neither `synth_field` nor `synth_call` handles it.

**Fix:** Determine how the parser represents `Point.{ ... }`. If it produces a `NamedRecord(type_path, entries)` AST node (or equivalent), add a synth arm that resolves `Point` to its TypeId and delegates to `check_record_lit` with the resolved type as expected. If the parser lowers it differently (e.g., `Field` chain), the fix goes there instead.

**Tests:**
- `p := Point.{ x: 1, y: 2 }` in synth mode — type is `Named(point_id)`
- `Point.{ x: 1 }` missing field `y` — diagnostic

### C8 — Ambiguous MetaVar detection (unsolved type variables)

The plan (M5) mentions detecting ambiguous types where MetaVars are never resolved. Currently there is no post-function-check pass. `fn f() Void { identity({}) }` would leave an unsolved MetaVar without a diagnostic.

**Fix:** After checking each function body, walk the type map entries for that function's span range and report any remaining unsolved MetaVars (after zonking). Emit "ambiguous type — cannot infer type for this expression".

**Tests:**
- `identity(x)` where `identity<T>` and `x` has no type context — diagnostic
- `identity(42)` — no diagnostic (MetaVar solved to Int)

### C9 — Latent `Var` hazard in unification ✅

**File:** `checker.tw:294–304`

If a raw `Var("T")` reaches `unify` (e.g., from an uninstantiated field type or an expected type that wasn't substituted), it falls through to a type mismatch error. Currently `instantiate` replaces `Var` with `MetaVar` in function sigs, and `subst_vars` replaces them in field types, so this is unlikely in practice — but any path that feeds unsubstituted types into `unify` would hit a spurious error.

**Fix:** Add a defensive check: when `unify` encounters `Var(name)` against a non-`Var` type, emit a more descriptive internal error ("uninstantiated type variable 'T' reached unification — this is a compiler bug"). This doesn't fix the root cause but makes it diagnosable. Alternatively, assert that `Var` should never reach `unify` and treat it as an internal error.

**Tests:**
- Ensure no existing tests produce "uninstantiated type variable" errors (regression guard)

### C10 — Resolver: document intentional omissions ✅

**File:** `resolver.tw`

Several intentional omissions have no comments explaining them:
- Imports (`Use` items) silently ignored in Pass 1 and Pass 2 (lines 234–263, 276–301)
- Top-level `Stmt` items silently ignored
- Pre-populated env functions not checked for duplicates in Pass 1 (user shadowing prelude is intentional but undocumented)

**Fix:** Add comments at each `_ => {}` catch-all explaining what is intentionally skipped and why:
- `// Use items: imports deferred to multi-module Phase E`
- `// Stmt items: top-level statements don't need name resolution`
- In Pass 1 fn dedup: `// Note: seen_fns is local; user functions intentionally shadow prelude functions`

No behavioral change, documentation only.

---

## Phase 3 — Refactoring ✅

### R1 — Extract `build_var_map` ✅

**Files:** `checker.tw:560–564`, `613–616`, `754–758`, `1105–1108`

The same loop building `Dict<String, MonoType>` from `type_params` + `type_args` appears 4 times.

**Extract:**
```tw
fn build_var_map(type_params: Vector<String>, type_args: Vector<MonoType>) Dict<String, MonoType> {
  m: Dict<String, MonoType> = Dict.new()
  for i in range(type_params.len()) {
    if i < type_args.len() { m[type_params[i]] = type_args[i] }
  }
  m
}
```

Replace all 4 sites. Pure refactor, no behavior change.

### R2 — Extract sum variant resolution helper ✅

**Files:** `checker.tw:680–798` (check_variant_lit), `checker.tw:1058–1142` (check_variant_pattern)

These two functions have nearly identical structure: zonk expected → match Optional/Result/Named → look up variant → check/bind fields. The only difference is that `check_variant_lit` uses `synth(args[i], ...)` + `unify` while `check_variant_pattern` uses `check_pattern(sub_pats[i], ...)`.

**Extract:** A `resolve_variant_info` helper that does the common work:

```tw
type VariantInfo = .{
  field_types: Vector<MonoType>,  // substituted field types for this variant
}

fn resolve_variant_info(name: String, expected: MonoType, ctx: InferCtx) Result<VariantInfo, String>
```

Returns field types for the matched variant, or an error message. Both callers use this to get field types, then diverge for their own checking logic.

### R3 — `ResolvedEnv` COW update helpers ✅

**File:** `resolver.tw` (6 construction sites: lines 84, 167, 242, 326, 395, 439)

Every mutation requires reconstructing all fields. Add helpers:

```tw
fn env_with_types(env: ResolvedEnv, types: Vector<TypeEntry>, type_names: Vector<String>) ResolvedEnv
fn env_with_functions(env: ResolvedEnv, functions: Vector<FunctionSig>) ResolvedEnv
fn env_with_methods(env: ResolvedEnv, methods: Dict<String, Vector<MethodEntry>>) ResolvedEnv
```

Replace the 6 construction sites. This reduces blast radius when `ResolvedEnv` gains new fields.

### R4 — Move `ty_to_string` to resolver.tw as pub export ✅

**Files:** `checker.tw:364–400`, `resolver_suite.tw` (has `mono_to_string` with same logic)

Both the checker and the test suite independently implement MonoType→String conversion.

**Fix:** Move `ty_to_string` (or rename to `mono_to_string`) into `resolver.tw` as a `pub` function. Update the checker and test suite to import it. The checker already imports from `resolver`, so this adds no new dependency.

### R5 — Eliminate duplicate `find_type_name_by_id` in checker ✅

**File:** `checker.tw:1487` vs `resolver.tw:882`

The checker defines its own `find_type_name_by_id` doing the same iteration as the resolver's `find_type_name`.

**Fix:** Make the resolver's version `pub` (if not already) and import it in the checker. Remove the checker's duplicate.

### R6 — Eliminate duplicate `lookup_type_def` in checker ✅

**File:** `checker.tw:540–547`

The checker defines `lookup_type_def(env, tid)` which iterates `env.types` by id — the same operation as `resolver.lookup_type` but returning only the `.def`. The checker should call the resolver's `lookup_type` and extract `.def` from the result.

**Fix:** If `resolver.lookup_type` is already pub, use it. Otherwise make it pub and import it. Remove `lookup_type_def` from the checker.

### R7 — Split `synth_binary` into sub-functions ✅

**File:** `checker.tw:1310+`

`synth_binary` handles 11 binary operators (arithmetic, comparison, equality, range, logical, string concat) in one function. Each case is short but the function is long and mixes concerns.

**Fix:** Extract helpers:
- `synth_arith_op(op, lhs, rhs, s, ctx, diags)` — `+`, `-`, `*`, `/`, `%`
- `synth_cmp_op(op, lhs, rhs, s, ctx, diags)` — `<`, `<=`, `>`, `>=`
- `synth_logic_op(op, lhs, rhs, s, ctx, diags)` — `&&`, `||`
- `synth_eq_op(lhs, rhs, s, ctx, diags)` — `==`, `!=`
- Range `..` and string `+` can stay inline or get their own helpers.

### R8 — Resolver: use `Dict<String, Int>` for lookup indexes ✅

**File:** `resolver.tw:175–205`

`lookup_type`, `lookup_function`, `has_type`, `has_function` all do O(N) vector scans. For a module with N types and M functions, each lookup is O(N)/O(M). `resolve_type_expr` calls these for every type expression, making resolution O(N*M) in the worst case.

**Fix:** Add `Dict<String, Int>` index fields to `ResolvedEnv` (or maintain them alongside the vectors). Lookups become O(1). The COW helpers from R3 should maintain these indexes.

This is a performance improvement, not correctness. Acceptable to defer if the boot compiler only processes small modules.

### R9 — Resolver: topo-sort state clarity ✅

**File:** `resolver.tw:706–735`

`topo_visit` treats state=1 (in-progress / cycle) and state=2 (done) identically with a single early-return and the comment "Already done or in-progress (cycle)". This conflates two distinct states and makes the code confusing.

**Fix:** Either:
- **(a)** Split into two separate checks with distinct comments: `if state == 2 { return } // already visited` and `if state == 1 { return } // cycle detected, reported in Pass 3`
- **(b)** Add a comment explaining why they share the same branch: "Both cases are no-ops here; cycle detection is deferred to detect_circular_aliases"

Also consider using `Dict<Int, Int>` (type index → state) instead of `Dict<String, Int>` (type name → state) to decouple from name uniqueness.

### R10 — Resolver: minor algorithmic improvements ✅

Low-priority performance improvements that don't affect correctness:

**`is_circular_alias` visited set** (`resolver.tw:850–880`): Uses `Vector<String>` with linear scan — O(depth²). Switch to `Dict<String, Bool>` for O(depth).

**`add_name_index` dedup** (`resolver.tw:808–823`): Linear scan of `acc` for each new entry — O(N²) in the number of type references per declaration. Switch to `Dict<String, Bool>` for O(N).

These are bounded by the number of types in a single module, so practically fine for now.

---

## Phase 4 — Test Coverage Expansion

### T1 — Resolver error path tests

Add tests for currently uncovered resolver error paths:

- Builtin generic arity errors: `Vector<Int, String>`, `Dict<Int>`, `Result<Int>` (resolver.tw:594–621)
- Qualified type path error: `module.Type` → "not yet supported"
- Function param without annotation: `fn foo(x) Int { 0 }` → diagnostic
- Duplicate type parameter names: `type Foo<T, T> = ...` (currently silently accepted — test documents behavior until F7 lands)
- Mutually-referencing non-alias record types (exercises topo-sort)
- Anonymous record and anonymous sum in type expression position (resolver.tw:471–473)
- `.ErrorType` on a type expression (resolver.tw:473 — silently returns `None`)
- `register_methods` called twice on same type — verify methods merge correctly
- Inherent method on generic type: `fn area(b: Box<Int>) Float` registers on `Box`
- Circular alias chain of length 4+

### T2 — Checker error path tests

Add tests for currently uncovered checker error paths:

- Wrong variant arg count: `.Some(1, 2)`, `.None(1)`, `.Ok()`, `.Err(1, 2)`
- Unknown variant name: `.Foo` where `Option` expected
- User-defined variant in check mode: `type Shape = { Circle(Float) }; fn f() Shape { .Circle(1.0) }`
- User sum patterns: `case shape { .Circle(r) => r, .Rect(w, h) => w }`
- Wrong sub-pattern count in user sum pattern: `.Circle(r, s)` where Circle has 1 field
- Field access on non-record Named type (Sum): `s.foo` where `s: Shape`
- Field access on primitive: `x.foo` where `x: Int`
- Field access on MetaVar base (checker.tw:594–602)
- Record literal with extra unknown field: `.{ x: 1, z: 3 }` for `Point.{ x, y }`
- Record literal in non-Named context (checker.tw:673)
- Dict/String indexing: `d["key"]` → `V?`, `s[0]` → `Byte`
- Indexing non-indexable type: `42[0]` → diagnostic
- Range operator: `1..10` → `Range`
- String concatenation: `"a" + "b"` → `String`
- Bitwise operators: `a & b`, `a | b`, `a ^ b` where `a, b: Int` → `Int`
- Bitwise not: `~a` where `a: Int` → `Int`
- Occurs check: trigger infinite type diagnostic
- Scope shadowing: inner `x` shadows outer `x`, outer inaccessible after scope exit
- `Byte` literal synthesis (if Byte literals exist in the AST)
- Literal patterns in case: `case x { 1 => "one", _ => "other" }`

### T3 — Checker synthesis path tests

Add tests for uncovered synthesis paths:

- Closure stored in local, then called: `f := fn(x: Int) Int { x + 1 }; f(5)` (exercises `synth_call_general`)
- Field access on generic record: `fn get_val(b: Box<Int>) Int { b.value }`
- Function-as-value: `fn wrap() fn(Int) Int { add_one }` (exercises `synth_ident` function path)
- Bare `return` with no value in Void-returning function
- `collect x, i in xs { ... }` with index binding
- `for` over Dict — iterates keys
- `for` over String — iterates Bytes
- `check_return` without `current_ret` (top-level return, if reachable)

### T4 — Improve `find_offset` robustness in checker_suite

**File:** `checker_suite.tw`

The `type_at(src, fragment, ...)` helper uses `find_offset` which returns the first occurrence of `fragment` in `src`. If the same text appears twice, it checks the wrong expression.

**Fix:** Add a `find_offset_nth(src, fragment, n)` variant that finds the nth occurrence, or add an `after` parameter: `find_offset_after(src, fragment, start_pos)`. Use this in tests where ambiguity is possible.

### T5 — Test infrastructure helpers

**File:** `boot/tests/assert.tw`

Add missing assertion helpers to reduce test boilerplate:

- `assert.vec_len(v, expected_len)` — asserts vector length
- `assert.vec_contains(v, item)` — asserts vector contains item (for String vectors)

---

## Milestone Dependencies

```
F1 (param alignment) ─────────────────────┐
F2 (Bool exhaustiveness) ──────────────────┤
F3 (try validation) ───────────────────────┤
F4 (inferred return types) ────────────────┤
F5 (qualified variant paths) ──────────────┤── Phase 1 (no internal ordering deps)
F6 (circular alias wrappers) ──────────────┤
F7 (dup type params) ─────────────────────┤
F8 (context restore) ─────────────────────┤
F9 (return Never) ─────────────────────────┘
                                           │
C1 (alias expansion) ─── benefits from R1 ┐
C2 (element_type_of) ─────────────────────┤
C3 (break/continue/defer/assign) ─────────┤
C4 (top-level stmts) ─────────────────────┤
C5 (closure call asymmetry) ──────────────┤── Phase 2
C6 (list missing variants) ───────────────┤
C7 (named record constructors) ───────────┤
C8 (ambiguous MetaVar) ───────────────────┤
C9 (Var hazard) ───────────────────────────┤
C10 (document omissions) ─────────────────┘
                                           │
R1 (build_var_map) ────────────────────────┐
R2 (variant resolution helper) ── needs R1 ┤
R3 (env COW helpers) ──────────────────────┤
R4 (ty_to_string shared) ─────────────────┤── Phase 3
R5 (find_type_name dup) ──────────────────┤
R6 (lookup_type_def dup) ─────────────────┤
R7 (split synth_binary) ──────────────────┤
R8 (lookup indexes) ── needs R3 ───────────┤
R9 (topo-sort clarity) ───────────────────┤
R10 (minor algorithmic) ──────────────────┘
                                           │
T1–T5 (test expansion) ───────────────────── Phase 4 (can run alongside any phase)
```

Phase 1 items are independent. R1 should land before R2 and C1. R3 should land before R8. Test expansion (Phase 4) can happen alongside any phase.

---

## Files to Modify

- **`boot/compiler/resolver.tw`** — F1, F6, F7, C10, R3, R4, R5, R6, R8, R9, R10
- **`boot/compiler/checker.tw`** — F2, F3, F4, F5, F8, F9, C1–C9, R1, R2, R5, R6, R7
- **`boot/tests/suites/resolver_suite.tw`** — T1, R4 (import shared `mono_to_string`)
- **`boot/tests/suites/checker_suite.tw`** — T2, T3, T4
- **`boot/tests/assert.tw`** — T5
