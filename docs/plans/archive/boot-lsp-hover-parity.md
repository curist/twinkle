# Boot LSP hover parity plan

## Goal

Close the remaining hover behavior gaps between the Rust stage0 LSP implementation and the boot compiler LSP implementation.

## Status

Complete. Boot hover now covers symbol-aware calls and methods, scoped binders, user docs, builtin/prelude docs, builtin type docs, variants in expressions and patterns, imported variants, nested patterns, and UTF-16/multibyte stability. The boot hover suite includes parity coverage for these cases, and the self-hosted bundle path has been verified.

Boot hover should resolve the same user-facing symbols as goto-definition where hover is meaningful, display stable identifier ranges, include available documentation, and behave correctly for UTF-16 LSP positions.

## Current state

Boot hover already covers basic expression type hovers and some declaration/type syntax cases:

- literals and ordinary expressions via `typed.type_map`
- local variable references in expression positions
- function call identifiers where the callee expression has a function type
- type annotation names
- function declaration names
- type declaration names
- top-level binding names
- case-pattern variant constructors, including `Result.Ok` and `Result.Err`

Recent goto-definition work added stronger symbol identity and scoped binder handling. Hover should reuse the same ideas instead of continuing to infer everything from expression spans alone.

## Stage0 coverage to mirror

Use `tests/lsp_hover_test.rs` as the reference and keep `boot/tests/suites/lsp_hover_suite.tw` in sync where the boot LSP can express the same scenario.

Reference coverage includes:

- inferred type at expression position
- null outside expressions
- type annotation names
- inherent method names
- function declaration names
- type declaration names
- top-level binding declaration names
- case-arm variant constructors
- `Result` case-arm variants
- builtin function docs
- builtin docs after multibyte lines
- method-call docs
- user `///` docs on functions
- builtin function parameter names
- qualified builtin functions such as `Int.from_string`
- builtin type docs such as `Option<Int>`
- user `///` docs on types
- method hover stability across positions within the identifier
- method hover stability with multibyte prefix lines

## Missing or partial areas

### Symbol-aware function and method hover

Boot hover still relies heavily on expression type lookup. It should resolve symbols using the same identity model as goto-definition:

- free functions
- imported functions
- module-qualified functions
- receiver methods
- qualified builtin methods/functions such as `Int.from_string`

Hover content should use the resolved function signature, preserving parameter names where known.

### Binder hover

Hover should understand binders, not just references:

- function parameters
- local `let` and top-level bindings
- `for` value and index binders
- `collect` value and index binders
- case-pattern binders
- closure parameters

This likely needs checker metadata for binder types, not just expression type maps. Goto-definition now has precise binder spans; hover should use those spans and attach type information to them.

### Documentation display

Boot parser/AST does not yet expose user `///` docs in a way hover can render. Add doc metadata for:

- function declarations
- type declarations
- possibly enum variants later, if desired

Hover should render the signature/type plus docs in a stable format.

### Builtin and prelude documentation

Stage0 exposes docs for builtins and builtin types. Boot needs an equivalent source of documentation for:

- hardcoded I/O builtins such as `println`
- signature-file builtins such as `Int.from_string`
- builtin types such as `Option` and `Result`
- prelude-backed methods such as `Vector.len` or `String.graphemes`

Prefer deriving docs from prelude/signature sources when possible. Use a small hardcoded registry only where no source file exists.

### Variant hover completeness

Boot currently covers case-pattern variants for named sums and `Result`. Extend and verify:

- `Option.Some` / `Option.None`
- expression constructors such as `.Ok(1)`
- qualified variant constructors
- imported enum variants
- nested patterns

### UTF-16 and multibyte stability

Boot LSP should be stable across UTF-16 positions and within identifier spans. Add boot tests corresponding to the stage0 multibyte/grapheme cases.

## Proposed design

### Reuse goto-definition reference discovery

Avoid duplicating symbol discovery logic independently in hover. Extract shared helper concepts or mirror the same structure:

- import references
- type references
- expression identifiers and fields
- variant references
- scoped binders

Hover then resolves the found reference to display metadata rather than a location.

### Add first-class AST metadata instead of span guessing

Prefer storing identifier spans and docs when parsing:

- `FunctionDecl.name_span`
- `TypeDecl.name_span`
- `Param.name_span`
- `LetStmt.name_span`
- `ForStmt.pattern_span` / `index_span`
- `CollectExpr.pattern_span` / `index_span`
- doc comments on declarations

Some span metadata already exists for params, lets, for, and collect; keep expanding this approach rather than reconstructing spans from declaration starts.

### Add hover metadata from type checking

Expression type maps are insufficient for binders and pattern variables. Add or expose type maps for:

- local binding symbols
- parameter symbols
- for/collect binders
- case-pattern binders

If the checker already has these types internally, preserve them in `CheckResult` for query consumers.

### Centralize hover formatting

Keep markdown/LSP wrapping in `boot/lib/lsp/hover.tw`, but centralize semantic hover content formatting in query code:

- function signatures with parameter names
- type names and applied types
- constructor signatures
- optional docs appended after the signature/type

## Implementation phases

### Phase 1: symbol-aware call hover

Add boot tests for:

- receiver method name hover with parameter names
- module-qualified function hover
- qualified builtin function hover, initially signature-only if docs are not ready
- hover stability across characters within a method name

Implement by resolving expression refs through `ResolvedEnv`, method tables, and function origins, similar to goto-definition.

### Phase 2: binder hover

Add boot tests for:

- function parameters
- local and top-level binding names
- `for` value/index binders
- `collect` value/index binders
- case-pattern binders
- closure parameters

Expose binder type information from checking and have hover return the binder type over the identifier span.

### Phase 3: user docs

Parse leading `///` comments and attach them to function/type declarations.

Add boot tests for:

- user function call docs
- user function declaration-name docs
- user type declaration-name docs
- imported user docs if query cache/source metadata supports it

### Phase 4: builtin and prelude docs

Create a doc source for builtin/prelude symbols.

Add boot tests for:

- `println` docs and named parameter signature
- `Int.from_string` docs
- `Option<Int>` docs
- prelude method docs such as `xs.len()` or a documented string/vector method

### Phase 5: variant and UTF-16 completion

Add boot tests for:

- `Option` variants
- expression variant constructors
- imported variants
- nested patterns
- multibyte prefix line stability
- positions at multiple characters within a method identifier

Fix any remaining span and UTF-16 conversion issues found by these tests.

## Acceptance criteria

- Boot hover suite covers every stage0 hover scenario that is applicable to boot LSP.
- Hover ranges are identifier spans where the hover target is an identifier.
- Function hovers include parameter names when known.
- User and builtin docs appear in hover content.
- UTF-16 LSP positions work across multibyte text.
- `tools/boot-test-fast.sh` passes.
- `make bundle-cli` passes before landing changes that affect the bundled CLI.

## Suggested commit breakdown

- Add hover parity audit tests for one missing class at a time.
- Implement the smallest query/checker/parser support for that class.
- Keep parser metadata changes separate from hover behavior when practical.
- Commit docs/user-doc support separately from builtin-doc support.
