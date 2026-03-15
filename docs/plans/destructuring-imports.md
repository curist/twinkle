# Destructuring Imports Plan

## Goal

Add explicit unqualified imports via `use module.{...}` while preserving
qualified module access.

Target capability:

* value/function import: `use math.vector.{translate, scale}`
* type import: `use math.vector.{type Vec2, type Transform}`
* mixed import: `use math.vector.{translate, type Vec2}`
* aliasing: `use math.vector.{translate as tr, type Vec2 as V2}`

Explicitly not included in this plan:

* `self` import item (`{self, ...}`)
* wildcard imports (`*`)

## Current Baseline (2026-03-15)

* `use` only supports module path + optional module alias (`as`).
* Imported module exports are available qualified (`vector.translate`,
  `vector.Vec2`).
* `docs/design/module.md` and `docs/spec.md` list destructuring imports as a
  deferred future feature.

## Scope

In scope:

* Parser/AST support for import item lists.
* Resolver/env projection for selected unqualified imports.
* Value/type namespace-correct binding behavior.
* Alias support per imported item.
* Diagnostics and tests for import-item errors/collisions.
* Spec/design docs update from "future" to active behavior.

Out of scope:

* `self` import item.
* wildcard import.
* changing prelude auto-import behavior.
* re-export syntax changes (`pub use` remains unsupported).

## Syntax Proposal

Import forms:

```tw
use foo.bar
use foo.bar as baz
use foo.bar.{x, y}
use foo.bar.{type T, type U}
use foo.bar.{x as x1, type T as T1}
```

Constraints:

1. `use foo.bar as baz` and `use foo.bar.{...}` are distinct forms.
2. Combined `as` + `{...}` module form is not supported in MVP.
3. `type` marker is required for type imports in item lists.
4. Bare items in `{...}` import value namespace symbols (functions/values).

Qualified access behavior:

* `use foo.bar.{x}` still binds module alias `bar` (same as plain `use foo.bar`),
  so both `bar.x` and `x` are valid.
* If a custom module alias is needed, use two lines:
  * `use foo.bar as b`
  * `use foo.bar.{x, type T}`

## Name Resolution Semantics

### Namespace mapping

1. `type T` imports into the type namespace.
2. `x` imports into the value namespace.
3. `type T` does not import value constructors directly; constructor usage remains
   `T.Variant` / `.Variant` as per existing rules.

### Export checks

1. Value import item must match a public function or public value from target module.
2. Type import item must match a public type from target module.
3. Missing exports are hard errors with source span on the offending item.

### Collision rules

1. Value alias/name collisions in current module scope are hard errors.
2. Type alias/name collisions in current module scope are hard errors.
3. Cross-namespace same spelling is allowed (existing model): value `X` and
   type `X` may coexist.

## Compiler Design Changes

### 1. AST

Extend `ImportDecl` with optional item list:

* `items: Option<Vec<ImportItem>>`
* `ImportItem`:
  * `Value { name: String, alias: Option<String>, span }`
  * `Type { name: String, alias: Option<String>, span }`

`module_name()` behavior remains unchanged for module alias binding.

### 2. Parser

Update `parse_use_decl`:

1. Parse existing module path.
2. If next tokens are `.` `{`, parse import items.
3. Else keep current optional `as module_alias`.
4. Reject combined module `as` with item list in this phase.

### 3. Dependency planning

`PlannedDependency` should carry import-item projection info for `Import`
dependencies, so projection can add unqualified names deterministically.

### 4. Env projection

Extend dependency projection flow (`module/env_integration.rs`):

1. Existing behavior: register qualified exports for module alias.
2. New behavior: if import has items, additionally bind selected unqualified
   names into `TypeEnv`/`ValueEnv` and function table/value globals as needed.

Implementation note:

* Preserve snapshot/restore semantics for per-dependency projection.
* Keep prelude projection unchanged.

### 5. Diagnostics

Add targeted errors for:

* unknown imported value/type
* importing type without `type` marker
* duplicate import aliases/items within same list
* unsupported combined form (`use foo as bar.{...}`)

## Test Plan

Parser tests:

1. parse value-only import list.
2. parse type-only import list.
3. parse mixed list with aliasing.
4. reject invalid mixed syntax and combined module-`as` with item list.

Resolver/typecheck tests:

1. unqualified imported value resolves.
2. unqualified imported type resolves.
3. mixed imports work together.
4. missing export produces clear error.
5. per-namespace collision behavior matches rules.

Integration tests:

1. module remains qualified-accessible after destructuring import.
2. two-line custom alias + destructuring workflow works.
3. no regressions for existing plain `use` forms.

## Documentation Updates

Update:

* `docs/grammar.ebnf` import grammar and notes.
* `tree-sitter-twinkle/grammar.js` so editor parsing/highlighting matches parser syntax.
* `docs/design/module.md` section currently saying "No Destructuring Imports (MVP)".
* `docs/spec.md` module-system section to promote destructuring from future to
  supported behavior (without wildcard/self).

## Rollout

1. Land parser + AST + unit tests.
2. Land projection/resolution semantics + typecheck/integration tests.
3. Update docs/spec.
4. Keep wildcard/self out of grammar and parser.

## Exit Criteria

1. All four target forms (value, type, mixed, aliasing) work.
2. Qualified module access continues to work with destructuring import lines.
3. `self` and wildcard remain unsupported with explicit diagnostics.
4. Existing `use foo.bar` / `use foo.bar as baz` behavior remains stable.
