# Bug: Wasm trap on module init when resolver tests run

## Symptom

Running `boot/tests/main.tw` (with resolver suite included) traps at init time
in the Wasm backend:

```
wasm trap: wasm `unreachable` instruction executed
```

Backtrace points to `__linked_init` → `__user_init` → deep into user functions
(func_540, func_557, etc.). The crash happens before any test output is printed.

The interpreter backend runs the same code successfully — all resolver tests
pass under `--interpreter`.

## Reproduction

```bash
# Crashes:
cargo run -- run boot/tests/main.tw

# Works:
cargo run -- run --interpreter boot/tests/main.tw
```

Filtering to just resolver tests (`TWK_TEST_FILTER=resolver`) does not help —
the crash is in module initialization, not test execution.

## Context

The crash appeared when `boot/compiler/resolver.tw` was added to the build.
The resolver module introduces:

- A `MonoType` sum type with 13 variants (including recursive variants like
  `Named(TypeId, Vector<MonoType>)`, `Vector(MonoType)`, `Function(...)`)
- A `ResolvedTypeDef` sum type with 3 variants containing `Vector<ResolvedField>`
  and `Vector<ResolvedVariant>`
- `Dict<String, TypeEntry>` and `Dict<String, FunctionSig>` in `ResolvedEnv`
- `Cell<Int>` for `next_type_id`
- Multiple functions that pattern-match on `TypeExprKind` (8 variants from
  `boot/compiler/ast.tw`)

## Likely areas

- **Typed closure specialization** at the Wasm level — the resolver has closures
  passed to `.push()`, iterator patterns, and `case` arms that produce complex
  sum types. The typed-closure / sum-repr boundary machinery is the most common
  source of `ref.cast` failures that manifest as traps.
- **Sum representation** for `MonoType` — 13-variant sum with recursive payloads
  may hit an edge case in the erased↔typed boundary conversion.
- **Module init ordering** — the crash is in `__linked_init`, so it could be a
  type registration or global-init issue with the new module's types.

## Resolution (2026-03-16)

**Root cause:** `synth_case` in `src/types/check.rs` set the case expression's
result type to the first arm's type. When all non-wildcard arms contained
`return` (type `Never`), the case was typed as `Never` even though the wildcard
arm `_ => {}` produced `Void`.

This caused a cascade:
1. Case expression typed as `Never`
2. The `if` block's then-branch typed as `Never`
3. `emit_local_atom` in `src/codegen/emit.rs` sees a `Never`-typed local and
   emits `unreachable` instead of `local.get`
4. Wasm trap when the wildcard arm executes and tries to fall through

**Fix:** In `synth_case`, when iterating over arms, if `result_ty` is `Never`
and a subsequent arm has a non-`Never` type, update `result_ty` to the concrete
type. This mirrors `synth_if`'s handling of one-branch-diverges (line 1980).

**Minimal reproducer:**
```tw
fn lookup(name: String, flag: Int) String {
  if flag == 1 {
    case name {
      "a" => { return "found a" },
      "b" => { return "found b" },
      _ => {},
    }
  }
  "not found"
}
```

The initial hypotheses (typed closure specialization, sum representation, module
init ordering) were all red herrings — the bug was purely in the type checker.
