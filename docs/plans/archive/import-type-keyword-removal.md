# Remove `type` keyword from destructuring imports

## Problem

Destructuring imports currently require the `type` keyword to distinguish type imports from value imports:

```tw
use math.{add, make_vec, type Vec2}
```

Twinkle already enforces a hard rule: PascalCase identifiers are types, snake_case identifiers are values. The parser rejects violations. This means `type` in import lists is purely redundant — the casing already tells us what namespace to look up.

The goal syntax:

```tw
use math.{add, make_vec, Vec2}
```

## Scope

**In scope:** Remove the `type` keyword requirement and infer value vs type from identifier casing.

**Out of scope:** Changing the AST `ImportItem::Value`/`ImportItem::Type` enum — the downstream distinction is still useful, we just derive it from casing instead of a keyword.

## Edge cases

**Same name in both namespaces:** Twinkle has separate value and type namespaces, so a module could theoretically export both a value `Foo` and a type `Foo`. However, this cannot happen in practice — values must be snake_case and types must be PascalCase, so there is no overlap. No ambiguity arises.

**Aliases with `as`:** The imported name (not the alias) determines the namespace:

```tw
use math.{Vec2 as V}        // type import (Vec2 is PascalCase)
use math.{translate as tr}   // value import (translate is snake_case)
```

This is consistent: the source name determines what you're importing; the alias determines the local binding.

## Changes

### Phase 1: Parser changes (Rust)

**`src/syntax/parser.rs` — `parse_import_items()`:**

Current logic:
```rust
if self.peek_is(TokenKind::Type) {
    // consume `type`, parse ident → ImportItem::Type
} else {
    // parse ident → ImportItem::Value
}
```

New logic:
```rust
let name_tok = self.expect(TokenKind::Ident)?;
let alias = ...; // parse optional `as alias`
let is_type = name_tok.text.starts_with(|c: char| c.is_uppercase());
if is_type {
    items.push(ImportItem::Type { name, alias, span });
} else {
    items.push(ImportItem::Value { name, alias, span });
}
```

Also: accept (and ignore) the `type` keyword for backwards compatibility during transition, or reject it immediately. Recommend **rejecting immediately** since the language is pre-1.0 and there are very few `.tw` files to update.

### Phase 2: Parser tests

Update tests in `src/syntax/parser.rs`:
- `test_parse_destructuring_type_imports` — remove `type` from input strings
- `test_parse_destructuring_mixed_imports` — remove `type` from input strings
- `test_parse_destructuring_with_aliases` — remove `type` from input strings

### Phase 3: Boot compiler parser

**`boot/compiler/parser.tw` — `parse_import_items()`:**

Same logic change — infer from casing instead of checking for `TokenKind.Type`.

### Phase 4: Update `.tw` source files

Files that currently use `type` in destructuring imports:

- `boot/main.tw` — 5 destructuring imports with `type`
- `boot/compiler/lexer.tw` — 2 imports
- `boot/compiler/resolver.tw` — 2 imports (with many type names)
- `boot/compiler/checker.tw` — 3 imports (with many type names)
- `boot/compiler/parser.tw` — 3 imports (with many type names)
- `tests/modules/destructure/main.tw` — 1 import
- `tree-sitter-twinkle/test/highlight/keywords.tw` — 1 import

### Phase 5: Update documentation

- `docs/plans/archive/destructuring-imports.md` — update syntax examples
- `docs/spec.md` — update import syntax section
- `CLAUDE.md` — update the destructuring example

### Phase 6: Boot compiler parser tests

- `boot/tests/suites/parser_suite.tw` — update test input strings

## Ordering

Phases 1-2 first (Rust parser + tests), then Phase 4 (update .tw files), then Phase 3 (boot parser), then Phases 5-6. This order lets us validate with `cargo test` before touching everything else.

## Risk

Low. Pre-1.0 language, small number of files to update, no ambiguity in the new design.
