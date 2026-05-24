# Handoff

## Current goal
Continue compiler/formatter bug cleanup from the migration work before removing the Rust `wasmtime` dependency.

The immediate next task is **not** to keep the current Byte-only `$` naming change as-is. Generalize the rule so **all compiler/builtin intrinsic implementation names that are not meant to be user-callable are unrepresentable from user code**, avoiding collisions with user functions.

Suggested convention from the user: use `$` as the internal separator because user identifiers cannot contain `$`.

## Important user preference
- Keep commits scoped to actual fixes.
- Do not accidentally commit unrelated migrated test-suite/handoff changes unless requested.
- Focus on correctness/structural bugs, not metrics.

## Last validated state
After the Byte-only `$` experiment and the other structural fixes, these passed:

```bash
make stage2
make quick-bundle-cli
target/twk run boot/tests/main.tw
```

But the Byte-only `$` change is incomplete by design; it should be generalized before committing.

## Uncommitted work currently in tree
Compiler changes:
- `boot/compiler/opt/defer_elim.tw`
  - Fixed defer snapshot type tracking by seeding parameter/local types into defer elimination type maps.
- `boot/compiler/lower_core/records.tw`
  - Added first-class method-value lowering by creating receiver-capturing closure wrappers.
- `boot/compiler/opt/copy_prop.tw`
- `boot/compiler/opt/pipeline.tw`
  - Copy-prop can receive mono info and avoids inlining Byte int-literal bindings that would erase Byte representation context.
- `boot/compiler/codegen/emit.tw`
- `boot/compiler/codegen/emit/calls.tw`
- `boot/compiler/backend/verify_expr.tw`
  - Byte literal init/assign/direct-call handling and backend verifier allowances for in-range Byte literals.
- Byte-only internal-name experiment touched:
  - `boot/compiler/builtins.tw`
  - `boot/compiler/resolver.tw`
  - `boot/compiler/signatures.tw`
  - `boot/compiler/lower_core/calls.tw`
  - `boot/compiler/checker.tw`
  - `boot/compiler/backend/callable_targets.tw`
  - tests listed below

Tests / suites:
- `boot/tests/main.tw` imports migrated/new suites.
- Untracked suites:
  - `boot/tests/suites/byte_suite.tw`
  - `boot/tests/suites/defer_suite.tw`
  - `boot/tests/suites/first_class_method_suite.tw`
  - `boot/tests/suites/string_edge_cases_suite.tw`
- Existing tests updated for Byte-only `$` experiment:
  - `boot/tests/suites/backend_verify_suite.tw`
  - `boot/tests/suites/base_env_guardrail_suite.tw`
  - `boot/tests/suites/builtins_canonical_suite.tw`

## What was fixed successfully
### Unicode/string edge cases
Already committed separately:
- Commit: `6c4ec6b Fix Unicode string edge cases`
- Fixed `String.from_code_point` invalid input behavior.
- Added lexer support for `\u{...}` escapes.

### `defer` + `continue` / return inside loop
Root cause:
- `boot/compiler/opt/defer_elim.tw` only used `op_result_mono` and missed parameter/local types for defer snapshot locals.

Fix in tree:
- Added local/parameter type map seeding into defer elimination.
- Re-enabled defer tests in `boot/tests/suites/defer_suite.tw`.

### First-class method values
Root cause:
- Field access fallback did not lower `foo.method` as a value.

Fix in tree:
- `boot/compiler/lower_core/records.tw` now resolves missing field access as a method value, hoists a wrapper function, captures the receiver, and returns a closure.
- Re-enabled tests in `boot/tests/suites/first_class_method_suite.tw`.

### Byte suite / Byte literal backend issues
Root causes found:
- Byte contextual literals could be represented as int literals after optimization, causing verifier/codegen mismatches for `i32` Byte slots/args.
- A user helper named `byte_to_int` collided with the compiler’s internal builtin/intrinsic target named `byte_to_int`, producing recursive self-calls.

Fixes in tree:
- Byte literal assignments and init emit `i32.const` for Byte/i32 slots.
- Direct call Byte literal args emit `i32.const` when expected param is Byte.
- Verifier accepts in-range int literals where Byte is expected.
- Copy-prop avoids propagating Byte int-literal bindings with mono information.
- Byte suite helper was restored to `fn byte_to_int(...)` after the Byte-only `$` experiment, so it currently verifies that this particular user name no longer collides.

## Critical next task: generalize `$` internal builtin/intrinsic names
The user explicitly said:
> we should not just update Byte related instrinsic though, we should apply the same rule for all instrinsic, so there won't be user side fn name conflict.

Current partial experiment changed only:
- `byte_to_int` → `byte$to_int`
- `byte_from_int` → `byte$from_int`
- `byte_to_string` → `byte$to_string`

This is insufficient. Need design/apply a consistent scheme.

### Existing partial scheme
`boot/compiler/builtins.tw` currently documents that internal-only `canonical = .None` builtins use double underscore (`vector__set_unsafe`) while method-derived names use single underscore. This does **not** prevent user functions from colliding with canonical builtin implementation names like `int_to_string`, `vector_len`, etc.

### Suggested direction
Use `$` in compiler-only implementation names for all builtin/intrinsic/runtime method targets that should not be writable by user code.

Potential rule:
- Public/canonical surface remains unchanged (`Byte.to_int`, `Vector.len`, `String.from_utf8`, etc.).
- Internal implementation names use `$`, e.g.:
  - `int$to_string`
  - `float$to_string`
  - `bool$to_string`
  - `byte$to_int`
  - `byte$from_int`
  - `byte$to_string`
  - `string$len`, `string$slice`, `string$utf8_bytes`, etc.
  - `vector$len`, `vector$append`, etc.
  - `dict$new`, `dict$get`, etc.
- Free functions that are truly public by name (`print`, `println`, `range`, etc.) may keep user-visible names if intended to be callable directly.
- Host/runtime private functions (`host_*`, `vector__builder_*`, etc.) should also be considered; if user code cannot/should not call them, they should probably use `$` too or another unrepresentable naming form.

Need decide exact boundary between:
1. public free builtins intentionally callable by user identifier; and
2. implementation targets reachable only through canonical methods/imported stdlib plumbing.

### Files likely needing broad update
Search for string-name dependencies before editing:
```bash
rg -n 'int_to_string|float_to_string|bool_to_string|string_|vector_|dict_|cell_|option_|result_|iterator_|task_|byte_|host_' boot/compiler boot/tests/suites
```

Likely files:
- `boot/compiler/builtins.tw`
  - builtin specs and ABI table are authoritative.
- `boot/compiler/signatures.tw`
  - `to_internal_name` currently emits single-underscore method names. Needs a generalized `$` separator rule for receiver groups, probably replacing or refining the existing double-underscore comment.
- `boot/compiler/lower_core/calls.tw`
  - `prelude_method_alias` mirrors `to_internal_name`; must match exactly.
- `boot/compiler/resolver.tw`
  - hardcoded bound method mapping currently uses many single-underscore internal names.
- `boot/compiler/checker.tw`
  - stringify resolution maps primitive types to internal names.
- `boot/compiler/codegen/emit.tw`
  - intrinsic dispatch string matches.
- `boot/compiler/backend/callable_targets.tw`
  - builtin wrapper/materialization checks by name.
- `boot/compiler/opt/semantics.tw`
  - optimizer semantics lookup by builtin name/id.
- `boot/compiler/lower_core/iteration.tw`
  - direct references such as `ctx.builtins.id("vector_len")`.
- `boot/compiler/codegen/emit/runtime_abi.tw` and related emit helpers may match names for shims.
- Tests in `boot/tests/suites/*` that assert internal names.

Use `rg` extensively; many string comparisons are exact.

## Reproduction for the original collision
Before the Byte-only `$` experiment, this pattern caused recursive self-call because user `byte_to_int` collided with internal `byte_to_int`:

```tw
fn byte_to_int(n: Byte) Int {
  n.to_int()
}

println(byte_to_int(65).to_string())
```

The generalized fix should keep this passing, and equivalent user names like `int_to_string`, `vector_len`, etc. should not collide with compiler internal targets.

Consider adding tests that define user-space functions with old internal-looking names and call methods inside them, e.g.:
- `fn byte_to_int(n: Byte) Int { n.to_int() }`
- `fn int_to_string(n: Int) String { n.to_string() }`
- `fn vector_len<T>(xs: Vector<T>) Int { xs.len() }`

## Temporary/cleanup notes
- `/tmp/byte_min.tw`, `/tmp/byte_context.tw`, `/tmp/byte_assert.tw`, etc. were used for repros; no need to preserve.
- A temporary `boot/tests/byte_main.tw` was created and removed.

## Suggested next steps in fresh session
1. Read this `HANDOFF.md` and inspect `git status`.
2. Decide and document the generalized internal naming rule.
3. Replace the Byte-only `$` experiment with a comprehensive `$` internal-name migration.
4. Add/adjust guardrail tests proving user functions can use old underscore names without colliding.
5. Run:
   ```bash
   make stage2
   make quick-bundle-cli
   target/twk run boot/tests/main.tw
   ```
6. Only after the generalized fix is clean, consider committing scoped structural fixes separately from unrelated migrated-suite/handoff changes if requested.
