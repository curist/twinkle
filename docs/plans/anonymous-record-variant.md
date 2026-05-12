# Anonymous Record Variants in Enum Types

## Goal

Allow anonymous record types as variant payloads in enum (sum) type declarations:

```tw
type ErrorDiag = {
  TypeMismatch(.{ span: Span, expected: MonoType, found: MonoType }),
  UndefinedVar(.{ span: Span, name: String }),
  Generic(.{ span: Span, message: String }),
}
```

Instead of the current workaround of defining named payload types separately:

```tw
pub type TypeMismatchDiag = .{ span: Span, expected: MonoType, found: MonoType }
pub type ErrorDiag = {
  TypeMismatch(TypeMismatchDiag),
  // ...
}
```

---

## Current State

- **Boot parser** (`boot/compiler/parser.tw`): already parses `.{ field: Type }` in type position and produces `TypeExprKind.Record(Vector<RecordField>)`. No change needed.
- **AST** (`boot/compiler/ast.tw`): `TypeExprKind` already has a `Record` variant. No change needed.
- **Resolver** (`boot/compiler/resolver.tw`): explicitly rejects anonymous records in `resolve_type_expr` (line ~1611) with "anonymous record types not supported in type expressions".
- **Tree-sitter grammar** (`tree-sitter-twinkle/grammar.js`): `_base_type` does NOT include `record_type_def`, so `type_list` (used for variant payloads) does not allow anonymous records. Needs updating.

---

## Design

When a variant has a single `.{ ... }` (Record) payload, the compiler synthesizes a record type internally. The synthesized type is **scoped to its parent enum**, not registered as a module-level type name. This means multiple enums can have variants with the same name without collision:

```tw
type Foo = {
  Bar(.{ x: Int }),
}

type FooFoo = {
  Bar(.{ x: Int }),
}

// Both coexist — Foo.Bar and FooFoo.Bar are distinct types
```

The synthesized types are registered with display names like `"Foo.Bar"` and `"FooFoo.Bar"` for diagnostics and cross-module export, but are not added to the module's `type_bindings` namespace (users cannot refer to them as standalone type names).

### Construction

Anonymous literal with expected type inference (existing mechanism):

```tw
e := ErrorDiag.TypeMismatch(.{ span: s, expected: t1, found: t2 })
```

### Pattern matching

Unchanged — `d` receives the synthesized record type:

```tw
case e {
  .TypeMismatch(d) => d.span,
  .UndefinedVar(d) => d.name,
}
```

### Export

Synthesized types are automatically included as `support_types` in module exports via the existing `collect_hidden_support_types` traversal. Because they are registered with `register_type_entry` (which populates `type_id_index` and `type_names`), `find_type_name` resolves them correctly.

**Cross-module qualification:** `merged_export_type_name` currently uses `name.contains(".")` as a heuristic to detect already-qualified names and skip re-qualification. This breaks with synthesized `"Foo.Bar"` names (local but dotted). The fix is to use `origin` instead — see Implementation step 6.

### Inherent methods

Synthesized types are NOT added to `local_type_names`, so `detect_inherent_methods` will not register inherent methods for them. Users cannot define inherent methods on anonymous record variant types — if they need that, they should define the record type explicitly.

---

## Implementation

### 1. Tree-sitter grammar (`tree-sitter-twinkle/grammar.js`)

Add `record_type_def` to `_base_type`:

```js
_base_type: $ => choice(
  $.primitive_type,
  $.generic_type,
  $.type_name,
  $.record_type_def,   // add this
),
```

Then regenerate and rebuild (must be done manually):

```bash
cd tree-sitter-twinkle
npx tree-sitter generate
npx tree-sitter build --wasm   # requires Docker
```

### 2. Add `set_type_def_by_id` helper (`boot/compiler/resolver.tw`)

The existing `set_type_def` looks up the TypeId via `type_bindings[name]`, which requires the type to be bound in the module namespace. Synthesized anonymous types are registered (in `type_id_index`) but not bound. Add a variant that operates directly on a TypeId:

```tw
fn set_type_def_by_id(env: ResolvedEnv, tid: TypeId, def: ResolvedTypeDef) ResolvedEnv {
  new_types := collect entry in env.types {
    if entry.id.id == tid.id {
      entry.def = .Some(def)
      entry
    } else {
      entry
    }
  }
  env.with_types(new_types, env.type_names)
}
```

Place this next to the existing `set_type_def` (around line 1431).

### 3. Resolver Pass 1 — `collect_declarations` (`boot/compiler/resolver.tw`)

After registering the enum type itself, scan its variants for anonymous record payloads and pre-register a `TypeEntry` for each. Use `register_type_entry` (not `add_type`) so the type gets a `type_id_index` entry and display name but no `type_bindings` entry:

```tw
.Type(decl) => {
  // ... existing type registration ...
  case decl.def {
    .Sum(variants) => {
      for variant in variants {
        if variant.fields.len() == 1 {
          case variant.fields[0].kind {
            .Record(_) => {
              synth_tid := TypeId.{ id: next_available_type_id(cur) }
              display_name := "${decl.name}.${variant.name}"
              cur = cur.register_type_entry(
                .{ id: synth_tid, arity: decl.type_params.len(), def: .None, span: variant.span },
                display_name
              )
            },
            _ => {},
          }
        }
      }
    },
    _ => {},
  }
},
```

Notes:
- `arity` matches the parent enum's type parameter count, since the anonymous record may reference the enum's type variables (e.g. `type Foo<T> = { Bar(.{ value: T }) }`).
- No collision check needed — the display name `"EnumName.VariantName"` is unique as long as variant names within an enum are unique (already enforced by `check_dup_names`).
- **Ordering invariant:** The parent enum must be registered via `add_type` (which calls `bind_type`) before `register_type_entry` is called for synthesized variants. This ensures `type_bindings` is non-empty, preventing `with_types`'s bootstrap path (line ~271) from accidentally binding synthesized types into the user namespace.

### 4. Resolver Pass 2 — `resolve_type_decl` `.Sum` branch (`boot/compiler/resolver.tw`)

Replace the `collect` comprehension with a `for` loop. For anonymous record variants, resolve the record fields, set the type def via `set_type_def_by_id`, and emit a `Named(synth_tid, ...)` variant payload:

```tw
.Sum(variants) => {
  variant_names := collect v in variants { v.name }
  diags = diags.concat(check_dup_names(variant_names, "variant", decl.span))

  cur := env
  resolved_variants: Vector<ResolvedVariant> = []
  for variant in variants {
    is_anon_record := variant.fields.len() == 1 and
      case variant.fields[0].kind { .Record(_) => true, _ => false }

    if is_anon_record {
      record_type_expr := variant.fields[0]
      record_field_exprs := case record_type_expr.kind { .Record(fs) => fs, _ => error("unreachable") }
      field_names := collect f in record_field_exprs { f.name }
      diags = diags.concat(check_dup_names(field_names, "field", record_type_expr.span))
      resolved_record_fields: Vector<ResolvedField> = collect field in record_field_exprs {
        r := cur.resolve_type_expr(field.ty, names)
        diags = diags.concat(r.diagnostics)
        ty := case r.ty { .Some(t) => t, .None => MonoType.ErrorType }
        ResolvedField.{ name: field.name, ty }
      }

      display_name := "${decl.name}.${variant.name}"
      synth_tid := case cur.type_index[display_name] {
        .Some(idx) => cur.types[idx].id,
        .None => error("unreachable: synthesized type not registered"),
      }
      cur = cur.set_type_def_by_id(synth_tid, .Record(display_name, type_params, resolved_record_fields))

      // Build type args matching the parent enum's type params
      synth_args: Vector<MonoType> = collect tp in type_params { MonoType.Var(tp.name) }
      resolved_variants = resolved_variants.append(
        .{ name: variant.name, fields: [MonoType.Named(synth_tid, synth_args)] }
      )
    } else {
      resolved_fields: Vector<MonoType> = collect field_ty in variant.fields {
        r := cur.resolve_type_expr(field_ty, names)
        diags = diags.concat(r.diagnostics)
        case r.ty { .Some(ty) => ty, .None => MonoType.ErrorType }
      }
      resolved_variants = resolved_variants.append(
        .{ name: variant.name, fields: resolved_fields }
      )
    }
  }
  new_env := cur.set_type_def(decl.name, .Sum(decl.name, type_params, resolved_variants))
  .{ env: new_env, diagnostics: diags }
}
```

### 5. No changes to `resolve_references` / `local_type_names`

Synthesized types are NOT added to `local_type_names`. This means `detect_inherent_methods` won't try to register inherent methods for them. This is intentional — anonymous record variant types are internal to the enum and should not have their own method namespace.

### 6. Fix `merged_export_type_name` to use origin (`boot/compiler/resolver.tw`)

**Problem:** The current implementation uses `name.contains(".")` as a heuristic to detect already-qualified names (e.g. `"a.Foo"` from a previous import) and skip re-qualification. With synthesized `"Foo.Bar"` display names, a local type now contains `"."` — the heuristic would skip qualification, causing cross-module collisions when two modules define enums with the same variant name.

**How it works today:**
1. Module `a` defines `type Foo`. Exports with name `"Foo"`, origin `"path/to/a::Foo"`.
2. Module `b` imports from `a`. `merged_export_type_name("a", "Foo")` → `"a.Foo"`.
3. Module `b` re-exports `Foo` as support type with name `"a.Foo"`, origin `"path/to/a::Foo"`.
4. Module `c` imports from `b`. `merged_export_type_name("b", "a.Foo")` → `"a.Foo"` (pass-through because `contains(".")`). Correct — double-qualifying to `"b.a.Foo"` would be wrong.

**The fix:** Change the function signature to accept `origin` and compare the export name against the origin's local part. If they match, the name hasn't been qualified yet and needs qualifying. If they differ, it was already qualified during a prior import:

```tw
fn merged_export_type_name(alias: String, name: String, origin: String?) String {
  needs_qualify := case origin {
    .Some(o) => {
      // Origin format is "module_key::TypeName".
      // If name matches the local part, it hasn't been qualified yet.
      parts := o.split("::")
      if parts.len() >= 2 {
        name == parts[parts.len() - 1]
      } else {
        true
      }
    },
    .None => true,
  }
  if needs_qualify { qualify_name(alias, name) } else { name }
}
```

**Update all call sites** to pass `exported.origin`:

```tw
// In register_imported_interface_types (lines ~856-869):
target_name := merged_export_type_name(alias, exported.name, exported.origin)

// In plan_export_type_ids (line ~1714):
target_name := merged_export_type_name(alias, exported.name, exported.origin)
```

**Verification that the origin-based check is correct:**

| Scenario | name | origin | local part | match? | action |
|---|---|---|---|---|---|
| Local `Foo` exported | `"Foo"` | `"a::Foo"` | `"Foo"` | yes | qualify → `"a.Foo"` |
| Local `Foo.Bar` (synth) exported | `"Foo.Bar"` | `"a::Foo.Bar"` | `"Foo.Bar"` | yes | qualify → `"a.Foo.Bar"` |
| Re-exported `a.Foo` from prior import | `"a.Foo"` | `"a::Foo"` | `"Foo"` | no | pass-through `"a.Foo"` |
| Re-exported `a.Foo.Bar` from prior import | `"a.Foo.Bar"` | `"a::Foo.Bar"` | `"Foo.Bar"` | no | pass-through `"a.Foo.Bar"` |
| No origin (root module) | `"Foo"` | `.None` | — | — | qualify → `"a.Foo"` |

---

## What does NOT need to change

- **`resolve_type_expr`**: still rejects `.Record(...)` in all other type positions. The synthesis happens in the variant resolution loop before `resolve_type_expr` is called for field types.
- **Checker**: variant payloads become `Named(synth_tid, args)` — indistinguishable from any other named type payload. All existing construction and pattern matching logic works unchanged.
- **Codegen / lowering**: same reason — the synthesized type is a regular record type.
- **`collect_hidden_support_types`**: uses `find_type_name(tid)` which goes through `type_id_index` → `type_names`. Since we register with `register_type_entry`, the synthesized type is findable and gets exported as a support type automatically.
- **Diagnostics**: `ty_to_string_env` calls `find_type_name(tid)` for `Named` types. The display name `"Foo.Bar"` will appear in error messages, which is clear and correct.

---

## Tests

- Single-field anonymous record variant: construction, pattern match, field access.
- Multi-field anonymous record (the common case).
- Two enums with same variant name coexist without collision.
- Cross-module: export and import of an enum with anonymous record variants.
- Non-anonymous variants in the same enum still work (mixed enum).
- Generic enum with anonymous record variant (e.g. `type Foo<T> = { Bar(.{ value: T }) }`).
- Diagnostic messages show `Foo.Bar` as the type name.
