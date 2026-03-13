# Relative Import Plan (`use .foo`)

## Goal

Implement explicit relative imports for sibling/submodule access:

```tw
use .arg
use .command
use .style
```

This removes repeated namespace prefixes in nested modules while preserving a
stable, root-based canonical module identity.

## Motivation

Current imports are root-relative only, so nested modules repeat long prefixes:

```tw
use lib.argparse.arg
use lib.argparse.command
use lib.argparse.style
```

For modules such as `lib.argparse.app`, this is noisy and scales poorly as
submodule count grows.

## Scope

In scope:

- Syntax support for `use .foo` and `use .foo.bar`.
- Resolver support for relative import paths based on importing module path.
- Docs/spec/grammar updates for the new form.
- Test coverage for parser + module resolution + integration behavior.

Out of scope (MVP):

- Parent traversal (`use ..foo`).
- Package manager semantics.
- Implicit bare-name relative resolution (`use foo` remains absolute).

## Semantics (MVP)

1. `use foo` stays absolute (project-root relative).
2. `use .foo` is relative to the importing module's parent namespace.
3. `use .foo.bar` is relative with nested path segments.
4. `use @std.*` behavior is unchanged.
5. No fallback probing between absolute and relative forms.

Example:

- importing file: `<root>/lib/argparse/app.tw` (module `lib.argparse.app`)
- `use .arg` resolves to `lib.argparse.arg` -> `<root>/lib/argparse/arg.tw`
- `use .style` resolves to `lib.argparse.style` -> `<root>/lib/argparse/style.tw`

Root-level module behavior:

- importing file: `<root>/main.tw` (module `main`)
- `use .util` resolves to `util` -> `<root>/util.tw`

## Implementation Plan

## Phase 1: AST + Parser + Pretty Printer

- Extend `ImportDecl` in `src/syntax/ast.rs` to track relative imports
  (e.g. `is_relative: bool`).
- Update `parse_use_decl` in `src/syntax/parser.rs`:
  - accept `use .ident(.ident)*`
  - keep `use ident(.ident)*` and `use @ident(.ident)*` behavior intact
  - produce a clear parse error for invalid forms (`use .`, `use ..foo` in MVP)
- Update `src/syntax/pretty.rs` to print relative imports correctly.
- Update Tree-sitter grammar in `tree-sitter-twinkle/grammar.js` so editor
  parsing/highlighting accepts `use .foo` forms.

Done criteria:

- Parser round-trips all three forms: absolute, relative, stdlib.

## Phase 2: Resolver + Planner Integration

- Add module-path-relative resolution helper in `src/module/loader.rs` or
  `src/module/mod.rs`:
  - derive importing module namespace from `importing_file` relative to project root
  - remove importing file stem to get parent namespace
  - append relative `module_path`
- Update `ModuleSourceAdapter::resolve_import_path` implementations
  (`FsModuleSourceAdapter`, `SourceMapModuleAdapter`) to branch on relative imports.
- Keep canonicalization/cache identity path-based and unchanged.

Done criteria:

- Relative imports resolve deterministically in both filesystem and source-map flows.

## Phase 3: Diagnostics and Error Messages

- Improve unresolved import diagnostics to include original import spelling and
  computed candidate path.
- Ensure diagnostics for relative imports mention relative context when useful.

Done criteria:

- Missing-module errors for relative imports are actionable without inspecting resolver internals.

## Phase 4: Tests

- Parser tests:
  - accepts `use .foo`, `use .foo.bar`, `use .foo as alias`
  - rejects `use .` and `use ..foo` (MVP)
- Module planner/resolver tests:
  - relative import from nested module
  - relative import from root module
  - stdlib + relative imports in same module
  - existing absolute imports unchanged
- Integration tests in `tests/modules/`:
  - working sibling import chain via `use .foo`
  - import aliasing with relative imports

Done criteria:

- New tests pass and existing module tests remain green.

## Phase 5: Adoption in `boot/`

- Update `boot/lib/argparse/app.tw` and `boot/lib/argparse/command.tw` to use
  relative imports where appropriate.
- Keep behavior-only changes; no API changes.

Done criteria:

- `boot/` compile/tests still pass with reduced import verbosity.

## Documentation Changes

- `docs/design/module.md`: relative-import design (already drafted).
- `docs/spec.md`: import syntax and resolution semantics updated with relative form.
- `docs/grammar.ebnf`: `ModulePath` grammar updated for leading-dot variant.
- `tree-sitter-twinkle/grammar.js`: import grammar updated to keep syntax tooling
  aligned with language grammar.

## Risks

1. Parser ambiguity around dot-prefixed syntax.
   - Mitigation: constrain grammar to `.` + `Identifier` only for import context.
2. Resolver drift between filesystem and source-map adapters.
   - Mitigation: shared helper for relative resolution and mirrored tests.
3. Behavior confusion between `use foo` and `use .foo`.
   - Mitigation: strict deterministic semantics and docs examples.

## Rollout Strategy

1. Land parser/AST/pretty + tests.
2. Land resolver/planner support + tests.
3. Land spec/grammar docs.
4. Migrate `boot/lib/argparse` to `use .foo`.
5. Keep `..` traversal as a future extension after real usage feedback.
