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

When a variant has a single `.{ ... }` (Record) payload, the compiler synthesizes a named record type using the **variant name** as the type name. This type is registered in the same module scope as any other type declaration.

```tw
// Written:
type ErrorDiag = {
  TypeMismatch(.{ span: Span, expected: MonoType, found: MonoType }),
}

// Equivalent to:
type TypeMismatch = .{ span: Span, expected: MonoType, found: MonoType }
type ErrorDiag = {
  TypeMismatch(TypeMismatch),
}
```

### Construction

Anonymous literal with expected type inference (existing mechanism):

```tw
e := ErrorDiag.TypeMismatch(.{ span: s, expected: t1, found: t2 })
```

Named constructor also works since the synthesized type is a regular record:

```tw
e := ErrorDiag.TypeMismatch(TypeMismatch.{ span: s, expected: t1, found: t2 })
```

### Pattern matching

Unchanged — `d` receives the synthesized record type:

```tw
case e {
  .TypeMismatch(d) => d.span,
  .UndefinedVar(d) => d.name,
}
```

### Name collision

- Variant name conflicts with a reserved type name → compile error at the variant declaration site.
- Variant name conflicts with a user-defined type in the same module → compile error.
- Two enums in the same module with an anonymous record variant sharing a name → compile error on the second one.

### Export

Synthesized types are automatically included as `support_types` in module exports via the existing `collect_hidden_support_types` traversal, which already follows `Named(tid, [])` references transitively. No special export handling needed.

### Inherent methods

Synthesized types participate in method detection (`detect_inherent_methods`) because they are registered as regular `TypeEntry` entries. Functions with a `TypeMismatch` first parameter in the same module become inherent methods on `TypeMismatch`.

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

### 2. Resolver Pass 1 — `collect_declarations` (`boot/compiler/resolver.tw`)

After registering the enum type itself, scan its variants for anonymous record payloads and pre-register a `TypeEntry` for each:

```tw
.Type(decl) => {
  // ... existing type registration ...
  case decl.def {
    .Sum(variants) => {
      for variant in variants {
        if variant.fields.len() == 1 {
          case variant.fields[0].kind {
            .Record(_) => {
              if is_reserved_type_name(variant.name) {
                // error: conflicts with reserved name
              } else if cur.has_type(variant.name) {
                // error: conflicts with existing type
              } else {
                synth_tid := TypeId.{ id: next_available_type_id(cur) }
                cur = cur.add_type(
                  .{ id: synth_tid, arity: 0, def: .None, span: variant.span },
                  variant.name
                )
              }
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

### 3. Resolver Pass 2 — `resolve_type_decl` `.Sum` branch (`boot/compiler/resolver.tw`)

Replace the `collect` comprehension with a `for` loop that can mutate `cur` (needed to call `set_type_def` for each synthesized type):

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
      cur = cur.set_type_def(variant.name, .Record(variant.name, [], resolved_record_fields))
      variant_tid := case cur.type_bindings[variant.name] {
        .Some(tid) => tid,
        .None => error("unreachable: synthesized type not registered")
      }
      resolved_variants = resolved_variants.append(
        .{ name: variant.name, fields: [MonoType.Named(variant_tid, [])] }
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

### 4. `resolve_references` — `local_type_names` (`boot/compiler/resolver.tw`)

Add synthesized variant type names so `detect_inherent_methods` picks them up:

```tw
local_type_names: Dict<String, Bool> = Dict.new()
for decl in type_decls {
  local_type_names[decl.name] = true
  // Add synthesized record type names from anonymous record variants
  case decl.def {
    .Sum(variants) => {
      for variant in variants {
        if variant.fields.len() == 1 {
          case variant.fields[0].kind {
            .Record(_) => { local_type_names[variant.name] = true },
            _ => {},
          }
        }
      }
    },
    _ => {},
  }
}
```

---

## What does NOT need to change

- `resolve_type_expr`: still rejects `.Record(...)` in all other type positions (function params, record fields, aliases). The synthesis happens before this call in the variant resolution loop.
- Checker: variant payloads become `Named(synth_tid, [])` — indistinguishable from any other named type payload. All existing construction and pattern matching logic works unchanged.
- Codegen / lowering: same reason — the synthesized type is a regular record type.
- `collect_hidden_support_types`: already traverses `Named` types recursively; synthesized types get exported as support types automatically.

---

## Tests

- Single-field anonymous record variant: construction, pattern match, field access.
- Multi-field anonymous record (the common case).
- Cross-module: export and import of an enum with anonymous record variants.
- Name collision errors: reserved name, existing user type, two enums with same variant name.
- Non-anonymous variants in the same enum still work (mixed enum).
- Generic enum with anonymous record variant (e.g. `type Foo<T> = { Bar(.{ value: T }) }`).
