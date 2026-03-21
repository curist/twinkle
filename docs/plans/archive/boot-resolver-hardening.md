# Boot Resolver Hardening Plan

Semantic correctness fixes for `boot/compiler/resolver.tw`. These are not style issues — they are cases where the resolver silently accepts wrong programs or produces misleading internal state.

Ordered by priority (highest first).

---

## 1. ~~Store type arity in `TypeEntry` during Pass 1~~ ✅

Done. Added `arity: Int` to `TypeEntry`, populated in `collect_declarations`. `check_user_type_arity` now reads `entry.arity` directly, eliminating the `.None` def bypass.

---

## 2. ~~Diagnose method name collisions instead of silently dropping~~ ✅

Done. `detect_inherent_methods` now checks for collisions with existing method registrations and emits a diagnostic instead of silently dropping the user method.

---

## 3. ~~Don't produce partial record/sum definitions on field resolution failure~~ ✅

Done (option a). Unresolvable field types now use `MonoType.ErrorType` sentinel instead of being skipped via `continue`. Type definitions always have the correct field count.

---

## 4. ~~Pass type parameters into dependency collection~~ ✅

Done. `collect_type_refs` and `collect_type_expr_refs` now receive `type_params` and skip names that match, preventing phantom dependency edges when a type parameter shadows a declared type name.

---

## 5. ~~Reserve builtin type names~~ ✅

Done. `collect_declarations` rejects type declarations whose name matches a builtin type (Int, Float, Bool, Byte, String, Void, Never, Vector, Dict, Option, Result, Cell, Range, Iterator).

---

## 6. ~~Add duplicate checks for record fields, sum variants, and function parameters~~ ✅

Done. Added `check_dup_names` helper. Duplicate record fields, sum variant names, and function parameter names now produce diagnostics.

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
