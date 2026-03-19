# Boot Resolver Hardening Plan

Semantic correctness fixes for `boot/compiler/resolver.tw`. These are not style issues — they are cases where the resolver silently accepts wrong programs or produces misleading internal state.

Ordered by priority (highest first).

---

## 1. Store type arity in `TypeEntry` during Pass 1

**Problem:** `check_user_type_arity` returns "no error" when `entry.def` is `.None` (shell state from Pass 1). Forward references or mutually-recursive types can skip arity checking entirely.

**Fix:** Add `arity: Int` to `TypeEntry`, populate it in `collect_declarations` from `decl.type_params.len()`. Rewrite `check_user_type_arity` to read `entry.arity` instead of extracting it from `entry.def`.

**Affected code:** `TypeEntry` (line ~66), `collect_declarations` (line ~269), `check_user_type_arity` (line ~656).

---

## 2. Diagnose method name collisions instead of silently dropping

**Problem:** `register_methods` deduplicates by name and silently keeps the first registration. Two consequences:
- User-defined inherent methods that collide with builtins are silently ignored.
- Duplicate user methods for the same type are silently ignored.

**Fix:** Choose an explicit policy:
- Builtins are final → emit a diagnostic if a user method collides.
- Duplicate user inherent methods for the same type → emit a diagnostic.

Replace the `if dup { continue }` in `register_methods` with diagnostic emission.

**Affected code:** `register_methods` (line ~107).

---

## 3. Don't produce partial record/sum definitions on field resolution failure

**Problem:** Unresolvable field types are skipped via `continue`, so `set_type_def` is called with a truncated field list. Downstream phases see a type with fewer fields than declared.

**Fix:** Either:
- (a) Introduce `MonoType.ErrorType` and preserve the field with that sentinel, or
- (b) Skip the entire `set_type_def` call if any field fails (leave `def = .None`).

Option (a) is preferable — it lets later passes continue with fewer cascaded failures.

**Affected code:** `resolve_type_decl` record path (line ~368) and sum variant path (line ~382).

---

## 4. Pass type parameters into dependency collection

**Problem:** `collect_type_expr_refs` doesn't know the declaring type's type parameters. A type parameter that shadows a declared type name creates a phantom dependency edge, distorting topo order.

**Fix:** Pass `type_params: Vector<String>` into `collect_type_refs` and `collect_type_expr_refs`. Skip names that appear in `type_params` before calling `add_name_index`.

**Affected code:** `collect_type_refs` (line ~740), `collect_type_expr_refs` (line ~763).

---

## 5. Reserve builtin type names

**Problem:** Users can declare types named `Vector`, `Option`, `Result`, `Dict`, etc. without error, but `resolve_single_name` checks builtins before user types, making the user type unreachable. Silent shadowing with no diagnostic.

**Fix:** In `collect_declarations` (Pass 1), reject type declarations whose name matches a builtin type. Emit a diagnostic like `"type name 'Vector' is reserved"`.

**Affected code:** `collect_declarations` (line ~269).

---

## 6. Add duplicate checks for record fields, sum variants, and function parameters

**Problem:** No uniqueness checks exist for:
- Record field names within a single record
- Sum variant names within a single enum
- Function parameter names

Only `check_dup_type_params` exists (for generic type parameters).

**Fix:** Add duplicate name checks in:
- `resolve_type_decl` for record fields and sum variants
- `resolve_function_decl` for parameter names

Emit diagnostics on collision.

**Affected code:** `resolve_type_decl` (line ~363), `resolve_function_decl` (line ~423).

---

## 7. ~~Clarify topo-sort semantics in comments~~ ✅

Done. Updated `topo_visit` comment to describe the actual semantics: dependency-biased ordering, not cycle verification; cycles tolerated here, caught in Pass 3.

---

## 8. ~~Make `add_function` shadowing semantics explicit~~ ✅

Done. Chose option (b): `add_function` now replaces the old entry in-place when a function with the same name already exists, preventing orphaned vector entries.

---

## 9. ~~Make `ty_to_string` env-aware for diagnostics~~ ✅

Done. Added `ty_to_string_env(env, ty)` that resolves `Named(id)` to actual type names. Original `ty_to_string` kept as debug-only helper with clarifying comment.

---

## 10. ~~Document inherent method policy for non-nominal types~~ ✅

Done. Added comment on `detect_inherent_methods` explaining that inherent methods only apply to nominal user-defined types; builtin container methods go through `register_builtin_methods`.

---

## Not included

The following items from the assessment were noted but not prioritized for this plan:

- **Performance hot spots** (`set_type_def`, `with_types`, `find_type_name` linear scans) — acceptable for bootstrap compiler, revisit only if modules get large.
- **Alias cycle detection rejecting `type A = Vector<A>`** — this is correct behavior for Twinkle (no recursive type aliases). Document in spec if not already there.
