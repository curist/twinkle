# Boot nested variant-pattern lowering safety

## Context

While cleaning up hot-path lookups in boot codegen, a seemingly harmless rewrite
of `boot/compiler/codegen/emit.tw` introduced a stage2 runtime trap:

- stage0 could still build `boot/main.tw`
- the stage1-produced boot compiler could still build stage2
- but running the resulting stage2 Wasm binary trapped with `unreachable`
  even on `--help`

The regression came from rewriting code like this:

```tw
case env.lookup_type_def(tid) {
  .Some(d) => ...
  .None => ...
}
```

into a more compact nested variant-pattern form:

```tw
case env.lookup_type_def(tid) {
  .Some(.Sum(_, _, _)) => ...
  .Some(.Alias(_, type_params, target)) => ...
  _ => ...
}
```

In particular, the nested shape inside `can_match_variant_pattern` appears to be
what pushed the self-hosted compiler into the bad state.

The code has since been restored to a safer two-step form so self-hosting works
again, but the underlying compiler/runtime issue remains unexplained.

## Temporary rule — RESOLVED

Both bugs are now fixed. Nested variant patterns like `.Some(.Sum(...))` are
safe to use in boot compiler source. The self-host loop passes with nested
patterns restored in `can_match_variant_pattern`.

## Problem statement

The boot pipeline currently has a codegen bug around **nested variant patterns**
when the outer type uses a typed sum representation (typed Option/Result struct)
and the inner type is also a sum type with its own typed representation.

This is dangerous because:

- the source-level rewrite looks semantics-preserving
- ordinary stage0 tests did not catch it
- the failure only showed up in later self-host stages
- it can silently discourage reasonable refactors toward clearer pattern code

For this bug class, passing unit-style or stage0-only coverage is not enough.
Any attempted fix must also pass the self-host loop.

## Goal

Make nested variant-pattern matching semantically reliable in the boot compiler,
or clearly document the currently unsupported subset and enforce it explicitly.

The preferred end state is full correctness for nested variant patterns of the
kind used above.

## Non-goals

This plan does not aim to:

- redesign pattern matching syntax
- broaden match ergonomics beyond current semantics
- optimize pattern lowering for speed first
- replace the current match lowering architecture wholesale

Correctness and diagnosability come first.

## Known failing shape

A concrete suspicious shape is:

```tw
case env.lookup_type_def(tid) {
  .Some(.Sum(_, _, _)) => true,
  .Some(.Alias(_, type_params, target)) =>
    can_match_variant_pattern(subst_type_params(target, type_params, args), env),
  _ => false,
}
```

The equivalent two-step form does not trigger the trap:

```tw
opt := env.lookup_type_def(tid)
case opt {
  .Some(def) => {
    case def {
      .Sum(_, _, _) => true,
      .Alias(_, type_params, target) => ...,
      _ => false,
    }
  },
  .None => false,
}
```

This strongly suggests the problem is not the semantic intent but the compiled
handling of nested variant patterns.

## Investigation history

### Phase 1: Rust stage0 universal fallback (fixed)

The first bug found was in the Rust stage0 compiler's pattern codegen at
`src/codegen/emit.rs`. The "universal variant" path of
`emit_pattern_condition()` (lines ~1718-1732) passed `None` for
`expected_mono` when recursing into nested sub-patterns, losing type
information. The three specialized paths (typed IterOption, typed UnfoldStep,
typed general Option/Result) all passed proper field type info.

**Fix applied:** The universal fallback now computes `field_expected` via
`pattern_variant_field_mono()` and passes it to the recursive call, matching
what `emit_pattern_bindings()` already does for the same path. This fix is
correct and necessary — it makes nested patterns work when types use the
universal (untyped `$rt_types__Variant`) representation.

**Validation:** `cargo run -- run boot/repros/nested_variant_minimal.tw` passes.
Direct compilation by the Rust stage0 of user programs with nested variant
patterns produces correct Wasm.

### Phase 2: Boot compiler typed-sum representation mismatch (current)

After the stage0 fix, the self-host loop still fails with nested patterns in
boot source. Further investigation revealed a **second, independent bug** in the
boot compiler's own codegen — or more precisely, in how the Rust stage0
compiles nested patterns when typed sum representations are involved.

#### Cross-compilation evidence

The original cross-compilation matrix was reinterpreted:

| Stage1 built from | Stage2 built from | Stage2 works? |
|---|---|---|
| two-step | two-step | YES |
| two-step | nested | YES |
| nested | two-step | NO |
| nested | nested | NO |

Stage1 built from nested source is broken. Since stage1 is compiled by stage0
(Rust), this initially pointed to stage0. The stage0 universal fallback fix
was necessary but not sufficient.

#### The typed-sum representation mismatch

The real issue is a **representation mismatch** when nested variant patterns
cross the typed/universal sum boundary.

The Rust stage0 has two representations for sum types:

1. **Typed sum structs** — e.g. `$user__$Option_opt_t127` with typed fields,
   used when `is_typed_general_option_candidate()` returns true
2. **Universal variant** — `$rt_types__Variant` with `(type_id, variant_idx,
   anyref[])` fields, used as fallback

When processing a nested pattern like `.Some(.Sum(_, _, _))`:

1. The outer `.Some(...)` goes through **Path 3** (typed general Option) because
   `Option<ResolvedTypeDef>` is a typed option candidate
2. Path 3 extracts the inner field via `StructGet` from the typed Option struct
   and coerces to anyref
3. The inner `.Sum(_, _, _)` pattern falls through to **Path 4** (universal
   fallback) because `typed_general_option = None` in the recursive call
4. Path 4 does `ref.cast (ref null $rt_types__Variant)` on the inner value
5. **But the inner value is a typed sum struct** (e.g. `$user__ResolvedTypeDef_sum`),
   **not** a `$rt_types__Variant` — the cast traps at runtime

This is confirmed by building a minimal test file with stage1:

```tw
type MyDef = { Sum(String), Alias(String) }

fn check(x: MyDef?) Bool {
  case x {
    .Some(.Sum(_)) => true,
    _ => false,
  }
}
```

Stage1 generates `i32.const 0` (always false) for the inner condition of even
simple `case x { .Sum(s) => s, .Alias(s) => s }` on user-defined sum types.
This proves the boot compiler's `can_match_variant_pattern` returns false for
all `Named` sum types when compiled with nested patterns — because its own
nested pattern `case env.lookup_type_def(tid) { .Some(.Sum(...)) => true, ... }`
is broken by the same representation mismatch.

#### Why the Rust stage0 direct output works

When the Rust stage0 compiles `/tmp/test_option_sum.tw` directly, the user types
(`MyDef`, `Outer`) may use the universal variant representation (not every sum
gets a typed struct). The nested pattern then goes through the universal fallback
on both levels and works correctly with the stage0 fix applied.

But when stage0 compiles the boot compiler, types like `ResolvedTypeDef` and
`Option<ResolvedTypeDef>` get typed representations because they're used
concretely throughout the codebase. This triggers the Path 3 → Path 4 mismatch.

### Artifacts

- `boot/repros/nested_variant_pattern_repro.tw` — basic nested pattern tests
- `boot/repros/nested_variant_pattern_faithful_nested.tw` — faithful repro (nested form)
- `boot/repros/nested_variant_pattern_faithful_two_step.tw` — faithful repro (two-step form)
- `boot/repros/nested_variant_minimal.tw` — comprehensive nested pattern tests
  covering Color, Shape, Animal, Result, Option<Option>, and TypeDef patterns
- `boot/repros/arm_chain_binding_test.tw` — tests arm-chain matches (< 3 arms)
  that exercise `can_match_variant_pattern` at runtime

## Root cause (updated)

There were **two independent bugs**, both now fixed:

### Bug 1 (fixed): Stage0 universal fallback drops expected_mono

**File:** `src/codegen/emit.rs`
**Function:** `emit_pattern_condition()`, universal variant path
**Status:** Fixed — now passes `field_expected` via `pattern_variant_field_mono()`

### Bug 2 (fixed): Pattern lowering uses uninstantiated field types

**File:** `src/ir/lower.rs`
**Function:** `lower_pattern()`, variant pattern field type resolution
**Status:** Fixed — field types from generic type definitions are now
instantiated with the scrutinee's type arguments before being passed to
recursive pattern lowering calls.

**What went wrong:** When lowering a nested variant pattern like
`.Some(.Sum(_, _, _))`, the inner pattern's scrutinee type was taken from the
raw type definition. For `Option<T>`, the `Some` variant's field type is
stored as `MonoType::Void` (a placeholder). This `Void` was passed as the
scrutinee type for the inner `.Sum(...)` pattern.

Since `Void` is not a `Named { type_id, .. }`, the inner pattern couldn't
resolve its type from the scrutinee. It fell back to a full type scan — 
iterating ALL types looking for one with a "Sum" variant. When multiple types
share a variant name (e.g. `ResolvedTypeDef`, `TypeBody`, `TypeDeclBody`,
`WasmLayout` all have "Sum"), the scan returns the first match, which may be
the wrong type. This caused the inner pattern to use the wrong `type_id`,
producing incorrect tag checks at runtime.

**The fix** instantiates field types using the scrutinee's concrete type
arguments:
- For user-defined generic types with `type_params`, substitutes `Var(name)`
  → `args[i]`
- For prelude types (Option, Result) with `Void` placeholder fields, maps
  them to the corresponding arg from the scrutinee MonoType

## Next steps — COMPLETED

### 1. Understand the inner typed-sum detection gap

When `emit_pattern_condition` recurses for the inner sub-pattern (`.Sum(_, _, _)`
in the example), it receives `expected_mono = Some(Named{ResolvedTypeDef})`.
The inner type IS a sum type with a typed representation, but the recursive call
only gets `expected_mono` — none of the typed-path metadata
(`typed_general_option`, etc.) is propagated.

**Investigate:** When the inner pattern's scrutinee is a typed sum (not
Option/Result, but a user-defined sum with a typed struct), which codegen path
should handle it? The universal fallback assumes `$rt_types__Variant` layout.
If the value is actually a typed sum struct, the code needs to either:

- (a) Detect that the inner value has a typed sum representation and use
  `StructGet` on the typed struct instead of casting to `$rt_types__Variant`
- (b) Convert the typed sum value to a universal `$rt_types__Variant` before
  the recursive pattern check
- (c) Route the inner pattern through a "typed sum" path that knows the struct
  layout

### 2. Check how the boot compiler handles this

Look at `boot/compiler/codegen/emit.tw`'s `emit_variant_pattern_condition`.
It calls `get_sum_layout_ctx(scrutinee_mono, ctx)` to get the struct layout
for the scrutinee type. If `scrutinee_mono` is the inner sum type (e.g.
`Named(tid_ResolvedTypeDef, [])`), `get_sum_layout_ctx` should return the
typed struct layout for that type. The question is whether the `field_instrs`
(which come from `StructGet` on the outer typed struct) produce a value
compatible with the inner struct layout's expectations.

**Key function to examine:** `get_sum_layout_ctx` — does it work correctly
when called with the inner sum type? Does it return a layout that matches the
actual GC type on the Wasm stack?

### 3. Write a targeted repro that triggers the typed path

The existing repros use the Rust stage0 directly, which may use universal
representations for small test types. We need a repro that:

- Forces both outer and inner types to use typed sum structs
- Can be compiled by stage1 (or a debug build of the boot compiler)
- Demonstrates the `i32.const 0` or runtime trap

This might require a repro that imports/uses enough of the type to trigger
`is_typed_general_option_candidate` / typed sum layout selection.

### 4. Fix approach: route inner patterns through typed sum path

The fix likely needs to happen in both the Rust stage0 and the boot compiler:

**In `emit_pattern_condition` (both Rust and boot):** When processing an inner
variant sub-pattern, check whether `expected_mono` indicates a type with a typed
sum representation. If so, don't fall through to the universal variant path —
instead, use the typed sum struct layout to generate the tag check and field
access.

This might mean the `emit_variant_pattern_condition` function (or its Rust
equivalent) needs to accept and use the `expected_mono` to select between typed
and universal representations, rather than always assuming the value on the
stack matches a specific representation.

## Debugging recipes

### Producing stage WAT files

```bash
# Stage1 WAT (Rust stage0 compiles boot source):
cargo run --release -- build boot/main.tw -o /tmp/stage1.wat

# Stage1 binary (needed to produce stage2):
cargo run --release -- build boot/main.tw -o /tmp/stage1.wasm

# Stage2 WAT (stage1 compiles boot source):
BOOT_WASM=/tmp/stage1.wasm node target/twk_cli_sea.cjs build boot/main.tw -o /tmp/stage2.wat

# Stage2 binary (needed to test stage3 / fixed-point):
BOOT_WASM=/tmp/stage1.wasm node target/twk_cli_sea.cjs build boot/main.tw -o /tmp/stage2.wasm

# Stage3 (should match stage2 at fixed-point):
BOOT_WASM=/tmp/stage2.wasm node target/twk_cli_sea.cjs build boot/main.tw -o /tmp/stage3.wasm
```

### Compiling a test file with a specific stage

```bash
# With Rust stage0 (reference — known correct for direct compilation):
cargo run --release -- build /tmp/test.tw -o /tmp/test_rust.wat

# With stage1 (boot compiler compiled by Rust stage0):
BOOT_WASM=/tmp/stage1.wasm node target/twk_cli_sea.cjs build /tmp/test.tw -o /tmp/test_boot.wat
```

Comparing the two WATs for the same input reveals codegen differences between
Rust stage0 and the boot compiler.

### Navigating WAT output

Stage WAT files are large (800k+ lines). Use grep/sed rather than reading them
directly.

```bash
# List all user-defined functions:
grep -n '(func \$user' /tmp/stage1.wat

# Find a specific function by name fragment (boot compiler names include
# the source name; Rust stage0 uses numeric IDs only):
grep -n 'can_match_variant' /tmp/stage2.wat

# Read a function body (e.g. starting at line 471266):
sed -n '471266,471400p' /tmp/stage2.wat

# Count imports (needed to map wasm-function[N] trap indices to WAT):
grep -c '(import ' /tmp/stage2.wat
```

### Mapping trap stack traces to functions

Runtime traps report `wasm-function[N]` indices. These include imported
functions, so subtract the import count to get the index into the defined
function list in the WAT:

```bash
# Example: trap at wasm-function[613], WAT has 22 imports
# → 613 - 22 = 591st defined function
grep -n '(func \$' /tmp/stage2.wat | sed -n '591,591p'
```

### Quick smoke test for nested patterns

```bash
# Minimal test file:
cat > /tmp/test_nested.tw << 'EOF'
type Inner = { A, B(Int) }
type Outer = { None, Some(Inner) }

fn test(x: Outer) String {
  case x {
    .Some(.B(n)) => n.to_string(),
    _ => "other",
  }
}

println(test(.Some(.B(42))))
println(test(.None))
EOF

# Compile with target stage, inspect the WAT for the test function:
BOOT_WASM=/tmp/stage1.wasm node target/twk_cli_sea.cjs build /tmp/test_nested.tw -o /tmp/out.wat
grep -A 40 'func.*test' /tmp/out.wat | head -45

# If the condition is `i32.const 0`, nested patterns are broken in that stage.
```

## Success criteria

- The Rust stage0 fix (Bug 1) remains in place
- The typed-sum mismatch (Bug 2) is fixed in both stage0 and boot compiler
- Self-hosting passes with nested patterns restored in boot source
- The temporary coding rule against nested patterns can be removed
- Cross-compilation matrix passes all four combinations
