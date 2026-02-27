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

## B-004: Module-level globals not accessible from functions at runtime

**Spec §8.1:** Top-level value bindings are module globals, accessible from all
functions in the module.

**Current state:** The type checker adds top-level `Let` bindings to `ValueEnv`,
so type-checking of references inside functions passes. But the interpreter has
no globals store — the values only exist in the `__init__` frame, so a function
called from `__init__` that references a module-level name will fail at runtime
with an undefined local.

**What's needed:**
* Add a `globals: HashMap<LocalId, Value>` (or similar) to the `Interpreter` struct.
* After evaluating each top-level binding in `__init__`, store the result in globals.
* When `Local(id)` is not found in the current call frame, fall back to globals.
* Ensure the lowerer assigns stable `LocalId`s to module-level bindings so the
  interpreter can look them up by id.

---

## B-005: Module-scope rebinding not rejected

**Spec §8.1:** "Rebinding (`=`) is not allowed at module scope — each name may
only be bound once."

**Current state:** `synth_assign` falls back to `value_env.lookup` when a name
is not found in `local_env`. Module-level bindings live in `value_env`, so a
top-level `x = expr` (rebinding a module constant) passes type-checking.

**What's needed:**
* Track whether the type checker is currently at module scope vs. inside a function.
* In `synth_assign`, reject rebinding when the target name is found only in
  `value_env` (i.e., it is a module-level binding, not a local).
* Emit a clear error: "rebinding is not allowed at module scope".

---

## B-006: Closure rebinding of captured variables not rejected

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
