# Twinkle Backlog

Known gaps between the spec and the current stage-0 implementation.
Items here are accepted work — they need to be done, just not yet.

---

## B-001: `pub` value bindings at module scope

**Spec §8.2:** `pub PI: Float = 3.14159` should export a module-level constant.

**Current state:** `parse_item` returns a parse error when `pub` precedes anything
other than `fn` or `type`. The `Stmt::Let` AST node has no `is_pub` field.

**What's needed:**
* Add `is_pub: bool` to `Stmt::Let` (or wrap top-level lets in a dedicated `Item::Let`).
* Parse `pub name := expr` and `pub name: T = expr` at top level.
* Enforce visibility in the module system: mark the value as exported in `CompilationContext`.
* Reject access to private top-level values from other modules.

---

## B-004: Closure rebinding of captured variables not rejected

**Spec §7.7.4:** "A closure may reference captured variables, but may **not**
rebind them using `=`."

**Current state:** `LocalEnv` uses a flat scope stack with no function-boundary
marker. When a lambda is typechecked inside a function, the outer function's
locals are still in scope. A lambda that does `x = x + 1` for an outer `x` passes
type-checking because `local_env.lookup` finds `x` in a parent scope.

**What's needed:**
* Introduce a function-boundary concept in `LocalEnv` (e.g., a depth counter or
  a sentinel scope that marks the closure boundary).
* In `synth_assign` for `ExprKind::Ident`, if the target variable is found beyond
  a function boundary, emit an error: "cannot rebind variable defined in outer scope".
* Add a typecheck/fail test for closure rebinding attempts.
