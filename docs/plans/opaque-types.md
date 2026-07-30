# Opaque Types Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `pub opaque type` so a module can export a nominal type while hiding its record fields or enum variants from other modules.

**Architecture:** Opaque types keep their full representation in the compiler environment for layout, lowering, methods, and same-module checking. The checker gates representation-dependent source operations by comparing the accessed type's defining module with the module currently being checked. Method calls remain available because they already resolve by receiver type identity.

**Tech Stack:** Twinkle boot compiler (`boot/compiler/*.tw`), boot standard library tests (`boot/tests/suites/*.tw`), multi-module fixtures (`boot/tests/fixtures/multi`), docs (`docs/spec.md`, `docs/API.md`).

## Global Constraints

- Implement in the boot compiler first; only update Rust stage0 if bootstrap requires it.
- Opaque is a module-boundary feature: the defining module can construct, field-access, field-update, and pattern-match its own opaque type.
- Other modules can name an opaque type, pass it, return it, call its public functions and inherent methods, and use it as a contract receiver when the defining module supplies the witness method.
- Other modules cannot construct an opaque record, use a contextual anonymous record literal as that type, read or update its fields, construct its enum variants, or pattern-match its enum variants.
- Do not add per-field visibility in this plan.
- `opaque` is only meaningful on exported types; support `pub opaque type ...` and reject non-`pub` `opaque type ...` with a targeted parser diagnostic.
- Do not allow opaque type aliases; aliases do not create a private representation boundary.
- Keep contract satisfaction based on type-owned inherent methods, not downstream access to representation.
- Update both language docs and grammar artifacts: `docs/spec.md`, `docs/grammar.ebnf`, `tree-sitter-twinkle/grammar.js`, and `tree-sitter-twinkle/queries/highlights.scm`.
- After editing `tree-sitter-twinkle/grammar.js`, run `npx tree-sitter generate` from `tree-sitter-twinkle/` and commit the regenerated `src/` files with the grammar change. Do not run `tree-sitter test` from the agent; ask the human to run it manually.
- After editing `.tw` files, run `target/twk fmt <changed-file>` and `target/twk lint boot/main.tw`.

---

## Semantics

Accepted syntax:

```tw
pub opaque type Regex = .{ program: Vector<Inst>, group_count: Int }

pub opaque type Token = {
  Ident(String),
  Number(Int),
}
```

Forbidden syntax:

```tw
opaque type Local = .{ value: Int }
pub opaque type UserId = Int
```

Diagnostics:

```text
`opaque` is only allowed on `pub type`
opaque type aliases are not allowed
```

Representation access matrix:

| Operation | Defining module | Importing module |
|---|---|---|
| `Opaque.{ field: value }` | allowed | error |
| `x: Opaque = .{ field: value }` | allowed | error |
| `x.field` | allowed | error |
| `x.field = value` | allowed | error |
| `.Variant(...)` / `Opaque.Variant(...)` | allowed | error |
| `case x { .Variant(...) => ... }` | allowed | error |
| `x.method(...)` | allowed | allowed |
| `module.function(x)` | allowed | allowed when function is exported |

Preferred diagnostic shape for representation access outside the defining module:

```text
opaque type `module.Type` does not expose its representation
```

Helpful follow-up line:

```text
use exported functions or inherent methods from the defining module instead
```

---

## File Structure

- `boot/compiler/tokens.tw`: add the `Opaque` token kind.
- `boot/compiler/lexer.tw`: lex `opaque` as a keyword.
- `boot/compiler/ast.tw`: add `is_opaque: Bool` to `TypeDecl`.
- `boot/compiler/parser.tw`: parse optional `opaque` before `type`; reject malformed `opaque` items with existing recovery.
- `boot/compiler/fmt/printer.tw`: preserve `opaque` in formatted public type declarations.
- `tree-sitter-twinkle/grammar.js`: parse `pub opaque type` and reject/avoid highlighting bare `opaque type` as valid syntax.
- `tree-sitter-twinkle/queries/highlights.scm`: highlight `opaque` as a keyword/modifier.
- `tree-sitter-twinkle/src/parser.c`, `tree-sitter-twinkle/src/grammar.json`, `tree-sitter-twinkle/src/node-types.json`: regenerate from `grammar.js`.
- `boot/compiler/resolver.tw`: carry opacity on `TypeEntry`; reject opaque aliases; preserve opacity through exports, imports, remapping, and alias re-exports.
- `boot/compiler/query/analyze.tw`: rename the existing shape-erasing helper currently named `opaque_type_exports`; pass the current canonical module path into type checking.
- `boot/compiler/checker.tw`: track `current_module`, add representation-visibility helpers, and gate field/constructor/variant/pattern operations.
- `boot/lib/source/diagnostics.tw`: add a structured opaque-representation diagnostic.
- `boot/compiler/query/diag_render.tw`: render the new diagnostic with the help text above.
- `boot/tests/suites/parser_suite.tw`: parser coverage for `opaque type`.
- `boot/tests/suites/resolver_suite.tw`: resolver coverage for flags, alias rejection, and export/import metadata.
- `boot/tests/suites/multi_module_suite.tw`: cross-module behavior tests using fixtures.
- `boot/tests/fixtures/multi/opaque_*.tw`: source fixtures for positive and negative multi-module behavior.
- `docs/spec.md`: specify opaque type syntax and module-boundary semantics.
- `docs/grammar.ebnf`: document the `pub opaque type` grammar.
- `docs/API.md`: update stdlib documentation examples for candidate opaque APIs after the feature lands.

---

### Task 1: Parser, AST, Formatter, and Tree-sitter Surface

**Files:**
- Modify: `boot/compiler/tokens.tw`
- Modify: `boot/compiler/lexer.tw`
- Modify: `boot/compiler/ast.tw`
- Modify: `boot/compiler/parser.tw`
- Modify: `boot/compiler/fmt/printer.tw`
- Modify: `tree-sitter-twinkle/grammar.js`
- Modify: `tree-sitter-twinkle/queries/highlights.scm`
- Regenerate: `tree-sitter-twinkle/src/parser.c`
- Regenerate: `tree-sitter-twinkle/src/grammar.json`
- Regenerate: `tree-sitter-twinkle/src/node-types.json`
- Test: `boot/tests/suites/parser_suite.tw`

**Interfaces:**
- Produces: `ast.TypeDecl.is_opaque: Bool`
- Produces: `tokens.TokenKind.Opaque`
- Consumes: existing `TypeDecl.{ is_pub, name, name_span, type_params, def, span, doc }`

- [ ] **Step 1: Extend token and AST data types**

Change `boot/compiler/tokens.tw`:

```tw
pub type TokenKind = {
  Use,
  As,
  Fn,
  Pub,
  Opaque,
  Type,
  Extern,
  // existing variants remain unchanged
}
```

Change `boot/compiler/ast.tw`:

```tw
pub type TypeDecl = .{
  is_pub: Bool,
  is_opaque: Bool,
  name: String,
  name_span: Span,
  type_params: Vector<TypeParam>,
  def: TypeDef,
  span: Span,
  doc: String?,
}
```

Update every `TypeDecl.{ ... }` construction in boot sources and tests to include `is_opaque`. For existing type declarations the value is `false`.

- [ ] **Step 2: Lex `opaque` as a keyword**

Change `keyword_or_ident` in `boot/compiler/lexer.tw`:

```tw
fn keyword_or_ident(text: String) TokenKind {
  case text {
    "use" => .Use,
    "as" => .As,
    "fn" => .Fn,
    "pub" => .Pub,
    "opaque" => .Opaque,
    "type" => .Type,
    "extern" => .Extern,
    // existing keyword cases remain unchanged
    _ => .Ident,
  }
}
```

- [ ] **Step 3: Parse `pub opaque type` and reject bare `opaque type`**

Update the top-level item parser so valid starts include these forms:

```tw
pub opaque type Public = .{ value: Int }
pub type Ordinary = .{ value: Int }
type Private = .{ value: Int }
```

Reject this form because opacity only has meaning for exported types:

```tw
opaque type Local = .{ value: Int }
```

Use this normalized parse flow:

```tw
is_pub := false
is_opaque := false

if c.kind() == .Pub {
  is_pub = true
  c = .advance()
}

if c.kind() == .Opaque {
  if !is_pub {
    diagnostics_out = .append(diag.error(c.span(), "`opaque` is only allowed on `pub type`"))
  }
  is_opaque = true
  c = .advance()
}

if c.kind() == .Type {
  return parse_type_decl_after_type(c, is_pub, is_opaque and is_pub, doc)
}
```

If `opaque` is present without a following `type`, emit:

```text
expected 'type' after 'opaque'
```

Construct the declaration with the new field:

```tw
decl := TypeDecl.{ is_pub, is_opaque, name, name_span, type_params, def, span: decl_span, doc: .None }
```

- [ ] **Step 4: Preserve `opaque` in the formatter**

Update `format_type_decl` in `boot/compiler/fmt/printer.tw` so it emits:

```tw
pub opaque type Name = .{ value: Int }
```

and:

```tw
opaque type Name = .{ value: Int }
```

Use this prefix construction:

```tw
prefix := if decl.is_pub and decl.is_opaque {
  "pub opaque type"
} else if decl.is_pub {
  "pub type"
} else if decl.is_opaque {
  "opaque type"
} else {
  "type"
}
```

- [ ] **Step 5: Add parser tests**

Add tests to `boot/tests/suites/parser_suite.tw` near existing type declaration tests:

```tw
fn test_parse_pub_opaque_record_type() Result<Void, String> {
  item := try parse_first_item("pub opaque type Secret = .{ value: Int }\n")
  case item {
    .Type(decl) => {
      try assert.is_true(decl.is_pub)
      try assert.is_true(decl.is_opaque)
      try assert.equal(decl.name, "Secret")
      case decl.def {
        .Record(fields) => try assert.equal(fields[0].name, "value"),
        _ => return .Err("expected record type"),
      }
      .Ok({})
    },
    _ => .Err("expected type item"),
  }
}

fn test_bare_opaque_type_is_rejected() Result<Void, String> {
  parsed := parser.parse("opaque type Hidden = { A, B(Int) }\n", 0)
  msgs := collect d in parsed.diagnostics { d.message() }
  try assert.vec_contains(msgs, "`opaque` is only allowed on `pub type`")
  .Ok({})
}
```

Register both tests in `parser_suite.suite()`.

- [ ] **Step 6: Update tree-sitter grammar and highlights**

Change `tree-sitter-twinkle/grammar.js` so type declarations accept the `pub opaque type` modifier sequence. Keep bare `opaque type` out of the valid type-declaration rule so editor parsing matches the compiler's accepted syntax.

Add `opaque` to `tree-sitter-twinkle/queries/highlights.scm` with the same capture used for `pub`, `type`, and other declaration keywords.

Run from the tree-sitter directory:

```bash
cd tree-sitter-twinkle
npx tree-sitter generate
```

Commit the regenerated files with the grammar change:

```text
tree-sitter-twinkle/src/parser.c
tree-sitter-twinkle/src/grammar.json
tree-sitter-twinkle/src/node-types.json
```

Do not run `tree-sitter test` from the agent. Ask the human to run it manually after the branch is ready.

- [ ] **Step 7: Run parser-focused verification**

Run:

```bash
target/twk run boot/tests/main.tw --filter parser
```

Expected before implementation: parser tests fail because `opaque` is not recognized as the new modifier.

Expected after implementation: parser tests pass.

- [ ] **Step 8: Commit parser surface**

```bash
git add boot/compiler/tokens.tw boot/compiler/lexer.tw boot/compiler/ast.tw boot/compiler/parser.tw boot/compiler/fmt/printer.tw boot/tests/suites/parser_suite.tw tree-sitter-twinkle/grammar.js tree-sitter-twinkle/queries/highlights.scm tree-sitter-twinkle/src/parser.c tree-sitter-twinkle/src/grammar.json tree-sitter-twinkle/src/node-types.json
git commit -m "Add opaque type parser surface"
```

---

### Task 2: Resolver Metadata and Export Boundaries

**Files:**
- Modify: `boot/compiler/resolver.tw`
- Modify: `boot/compiler/query/analyze.tw`
- Modify: `boot/lib/source/diagnostics.tw`
- Modify: `boot/compiler/query/diag_render.tw`
- Test: `boot/tests/suites/resolver_suite.tw`

**Interfaces:**
- Consumes: `ast.TypeDecl.is_opaque: Bool`
- Produces: `resolver.TypeEntry.is_opaque: Bool`
- Produces: `resolver.type_is_opaque(env: ResolvedEnv, tid: TypeId) Bool`
- Produces: `resolver.type_origin_for_id(env: ResolvedEnv, tid: TypeId) String?`

- [ ] **Step 1: Add opacity to resolver type entries**

Change `boot/compiler/resolver.tw`:

```tw
pub type TypeEntry = .{
  id: TypeId,
  arity: Int,
  def: ResolvedTypeDef?,
  span: Span,
  is_extern: Bool,
  is_opaque: Bool,
}
```

Update every `TypeEntry.{ ... }` construction:

- ordinary Twinkle type declarations: `is_opaque: decl.is_opaque`
- extern types: `is_opaque: true`
- builtin/prelude compiler-registered types: `is_opaque: false` unless the type already has no constructible Twinkle representation
- test-created entries: `is_opaque: false`

- [ ] **Step 2: Reject opaque aliases during resolution**

In `resolve_type_decl`, before resolving an alias definition, add:

```tw
if decl.is_opaque {
  case decl.def {
    .Alias(_) => return ResolveResult.{
      env,
      diagnostics: [diag.error(decl.span, "opaque type aliases are not allowed")],
    },
    _ => {},
  }
}
```

Keep record and sum definitions valid.

- [ ] **Step 3: Add resolver visibility helpers**

Add helpers near existing type lookup helpers:

```tw
pub fn type_is_opaque(env: ResolvedEnv, tid: TypeId) Bool {
  case env.lookup_type_by_id(tid) {
    .Some(entry) => entry.is_opaque,
    .None => false,
  }
}

pub fn type_origin_for_id(env: ResolvedEnv, tid: TypeId) String? {
  env.type_origins[tid.id]
}
```

If `lookup_type_by_id` remains private, keep `type_is_opaque` public and leave `lookup_type_by_id` private.

- [ ] **Step 4: Preserve opacity through remapping and imports**

Update `remap_type_entry` so `is_opaque` survives import remapping:

```tw
TypeEntry.{
  id: remapped_id,
  arity: entry.arity,
  def: remapped_def,
  span: entry.span,
  is_extern: entry.is_extern,
  is_opaque: entry.is_opaque,
}
```

Update `merge_exported_type`, `register_type_entry`, `add_type`, and any helper that rebuilds a `TypeEntry` to copy `is_opaque` from the source entry.

- [ ] **Step 5: Rename the existing shape-erasure helper in query analysis**

`boot/compiler/query/analyze.tw` already has helpers named `opaque_export_type` and `opaque_type_exports`; they erase type definitions for cyclic declaration interfaces, not for the language feature. Rename them to avoid confusion:

```tw
fn shape_erased_export_type(exported: ExportedType) ExportedType {
  exported.entry.def = .None
  exported
}

fn shape_erased_type_exports(exports: ModuleExports) ModuleExports {
  ModuleExports.{
    doc: exports.doc,
    visible_types: collect exported in exports.visible_types {
      shape_erased_export_type(exported)
    },
    support_types: collect exported in exports.support_types {
      shape_erased_export_type(exported)
    },
    functions: exports.functions,
    support_functions: exports.support_functions,
    methods: exports.methods,
    values: exports.values,
  }
}
```

Update the call sites in group analysis to use `shape_erased_type_exports`.

- [ ] **Step 6: Add a structured diagnostic for opaque representation access**

Add to `boot/lib/source/diagnostics.tw`:

```tw
OpaqueRepresentation(.{ span: Span, type_name: String, origin: String })
```

Add message rendering in `message(kind)`:

```tw
.OpaqueRepresentation(d) => "opaque type `${d.origin}.${d.type_name}` does not expose its representation"
```

Add help text in `help_lines(kind)`:

```tw
.OpaqueRepresentation(_) => ["use exported functions or inherent methods from the defining module instead"]
```

Add span handling in `span(kind)`.

Add `diag_render.tw` rendering by routing it through the same simple error path as `NoField` with help lines from `diagnostics.help_lines`.

- [ ] **Step 7: Add resolver tests**

Add to `boot/tests/suites/resolver_suite.tw`:

```tw
fn test_opaque_type_entry_flag() Result<Void, String> {
  parsed := parser.parse("pub opaque type Secret = .{ value: Int }\n", 0)
  resolved := resolver.empty_env().resolve(parsed.value)
  entry := try resolved.env.lookup_type("Secret").ok_or("missing Secret")
  try assert.is_true(entry.is_opaque)
  .Ok({})
}

fn test_ordinary_type_entry_not_opaque() Result<Void, String> {
  parsed := parser.parse("pub type Plain = .{ value: Int }\n", 0)
  resolved := resolver.empty_env().resolve(parsed.value)
  entry := try resolved.env.lookup_type("Plain").ok_or("missing Plain")
  try assert.is_false(entry.is_opaque)
  .Ok({})
}

fn test_opaque_alias_rejected() Result<Void, String> {
  parsed := parser.parse("pub opaque type UserId = Int\n", 0)
  resolved := resolver.empty_env().resolve(parsed.value)
  msgs := collect d in resolved.diagnostics { d.message() }
  try assert.vec_contains(msgs, "opaque type aliases are not allowed")
  .Ok({})
}
```

Register the tests in `resolver_suite.suite()`.

- [ ] **Step 8: Run resolver-focused verification**

Run:

```bash
target/twk run boot/tests/main.tw --filter resolver
```

Expected before implementation: resolver tests fail because `TypeEntry.is_opaque` does not exist and opaque aliases are not rejected.

Expected after implementation: resolver tests pass.

- [ ] **Step 9: Commit resolver metadata**

```bash
git add boot/compiler/resolver.tw boot/compiler/query/analyze.tw boot/lib/source/diagnostics.tw boot/compiler/query/diag_render.tw boot/tests/suites/resolver_suite.tw
git commit -m "Track opaque type metadata in resolver"
```

---

### Task 3: Checker Representation Visibility

**Files:**
- Modify: `boot/compiler/checker.tw`
- Modify: `boot/compiler/query/stage_runner.tw`
- Modify: `boot/compiler/query/analyze.tw`
- Modify: `boot/tests/suites/checker_suite.tw`
- Test: `boot/tests/suites/checker_suite.tw`

**Interfaces:**
- Consumes: `resolver.TypeEntry.is_opaque`
- Consumes: `resolver.type_origin_for_id(env, tid)`
- Produces: `checker.check(module: Module, env: ResolvedEnv, current_module: String, lint_mode: Bool) CheckResult`
- Produces: representation access diagnostics for opaque types outside their defining module

- [ ] **Step 1: Thread current module into the checker**

Change `InferCtx` in `boot/compiler/checker.tw`:

```tw
pub type InferCtx = .{
  env: ResolvedEnv,
  current_module: String,
  module_aliases: Dict<String, Bool>,
  // existing fields remain unchanged
}
```

Change `empty_ctx` signature:

```tw
fn empty_ctx(env: ResolvedEnv, current_module: String, module_aliases: Dict<String, Bool>, lint_mode: Bool) InferCtx
```

Change the public checker entrypoint:

```tw
pub fn check(module: Module, env: ResolvedEnv, current_module: String, lint_mode: Bool) CheckResult {
  ctx := empty_ctx(env, current_module, collect_module_aliases(module), lint_mode)
  // existing body remains structurally the same
}
```

Update direct callers:

```tw
// boot/compiler/query/stage_runner.tw
typed := checker.check(module.module, resolved.env, runner.path, lint_mode)
```

Update test helpers in `boot/tests/suites/checker_suite.tw`:

```tw
checker.check(parsed.value, resolved.env, "", false)
```

Use the empty string for single-module unit tests so local type origins without a canonical path remain same-module visible.

- [ ] **Step 2: Add checker helper functions**

Add near the record/field helpers in `checker.tw`:

```tw
fn local_type_origin(ctx: InferCtx, tid: TypeId) String {
  case ctx.env.type_origin_for_id(tid) {
    .Some(origin) => origin,
    .None => "",
  }
}

fn can_access_type_representation(ctx: InferCtx, tid: TypeId) Bool {
  if !ctx.env.type_is_opaque(tid) {
    return true
  }

  origin := local_type_origin(ctx, tid)
  origin == "" or origin == ctx.current_module
}

fn opaque_representation_diag(ctx: InferCtx, tid: TypeId, span: Span) DiagKind {
  type_name := ctx.env.ty_to_string_env(.Named(tid, []))
  origin := local_type_origin(ctx, tid)
  .Error(.OpaqueRepresentation(.{ span, type_name, origin }))
}
```

If `ty_to_string_env(.Named(tid, []))` includes type arguments poorly for generic types, use `record_info` / sum def name when present and fall back to `"<unknown>"`.

- [ ] **Step 3: Gate field access and method fallback correctly**

In `synth_field`, after the base expression is synthesized and zonked:

```tw
case zonked_base {
  .Named(tid, type_args) => case br.ctx.env.lookup_type_def(tid) {
    .Some(.Record(_, type_params, fields)) => {
      if !can_access_type_representation(br.ctx, tid) {
        method_r := try_synth_method_value(zonked_base, field_name, s, br.ctx, br.diags)
        case method_r {
          .Some(out) => return out,
          .None => return .{
            ty: .ErrorType,
            ctx: br.ctx,
            diags: br.diags.append(opaque_representation_diag(br.ctx, tid, s)),
          },
        }
      }

      // existing visible-record field lookup path
    },
    _ => {
      // existing non-record/method logic
    },
  },
  _ => {
    // existing logic
  },
}
```

This preserves `secret.reveal()` on an opaque type while rejecting `secret.value`.

- [ ] **Step 4: Gate named and anonymous record construction**

In `synth_named_record` and `check_record_lit`, when the expected type is `.Named(tid, type_args)` and the resolved def is `.Record(...)`, add the visibility check before field checking:

```tw
if !can_access_type_representation(cur_ctx, tid) {
  return .{
    ctx: cur_ctx,
    diags: diags.append(opaque_representation_diag(cur_ctx, tid, s)),
  }
}
```

For synthesis paths returning a type, use `.ErrorType` as the synthesized type.

This rejects both forms outside the defining module:

```tw
Secret.{ value: 1 }
x: Secret = .{ value: 1 }
```

- [ ] **Step 5: Gate record field rebinding**

In assignment/lvalue checking where `.Field(base, field_name)` is validated, add the same check before `find_record_field_type`:

```tw
if !can_access_type_representation(br.ctx, tid) {
  return .{
    ctx: br.ctx,
    diags: br.diags.append(opaque_representation_diag(br.ctx, tid, lhs.span)),
  }
}
```

This rejects:

```tw
secret.value = 2
```

while still allowing rebinding in the defining module.

- [ ] **Step 6: Gate enum variant construction**

In the variant construction paths around `try_synth_qualified_variant`, `resolve_variant_info`, `synth_variant`, and `check_variant`, reject construction when the target enum type is opaque and not from `ctx.current_module`.

The check should run after the variant name has been resolved to a concrete `tid`, before argument checking:

```tw
if !can_access_type_representation(ctx, tid) {
  return .{
    ty: .ErrorType,
    ctx,
    diags: diags.append(opaque_representation_diag(ctx, tid, s)),
  }
}
```

This rejects both forms outside the defining module:

```tw
Token.Ident("x")
.Ident("x")
```

when the expected type is an imported opaque enum.

- [ ] **Step 7: Gate enum pattern matching**

In `check_variant_pattern`, after resolving the variant against the expected scrutinee type, reject representation access when the enum is opaque and not from `ctx.current_module`:

```tw
if !can_access_type_representation(ctx, tid) {
  return .{
    ctx,
    diags: diags.append(opaque_representation_diag(ctx, tid, s)),
  }
}
```

This rejects:

```tw
case token {
  .Ident(name) => name,
  .Number(n) => n.to_string(),
}
```

outside the defining module.

- [ ] **Step 8: Add single-module checker tests for local access**

Add to `boot/tests/suites/checker_suite.tw`:

```tw
fn test_opaque_record_local_module_can_use_fields() Result<Void, String> {
  src := "opaque type Secret = .{ value: Int }\nfn reveal(s: Secret) Int { s.value }\nfn main() Int { reveal(Secret.{ value: 42 }) }"
  result := check_src(src)
  if result.diagnostics.len() > 0 {
    .Err("expected no diagnostics, got ${result.diagnostics[0].message()}")
  } else {
    .Ok({})
  }
}

fn test_opaque_enum_local_module_can_match() Result<Void, String> {
  src := "opaque type Token = { Ident(String), Number(Int) }\nfn name(t: Token) String { case t { .Ident(s) => s, .Number(n) => n.to_string() } }"
  result := check_src_with_env(src, test_env())
  if result.diagnostics.len() > 0 {
    .Err("expected no diagnostics, got ${result.diagnostics[0].message()}")
  } else {
    .Ok({})
  }
}
```

Register both tests.

- [ ] **Step 9: Run checker-focused verification**

Run:

```bash
target/twk run boot/tests/main.tw --filter checker
```

Expected before implementation: the checker or parser-facing tests fail because opacity is not represented in the checker.

Expected after implementation: checker tests pass and existing checker behavior is unchanged for ordinary types.

- [ ] **Step 10: Commit checker visibility**

```bash
git add boot/compiler/checker.tw boot/compiler/query/stage_runner.tw boot/compiler/query/analyze.tw boot/tests/suites/checker_suite.tw
git commit -m "Enforce opaque type representation boundaries"
```

---

### Task 4: Cross-Module Behavior and Method Ergonomics

**Files:**
- Create: `boot/tests/fixtures/multi/opaque_secret.tw`
- Create: `boot/tests/fixtures/multi/opaque_secret_use_ok.tw`
- Create: `boot/tests/fixtures/multi/opaque_secret_field_bad.tw`
- Create: `boot/tests/fixtures/multi/opaque_secret_construct_bad.tw`
- Create: `boot/tests/fixtures/multi/opaque_token.tw`
- Create: `boot/tests/fixtures/multi/opaque_token_match_bad.tw`
- Modify: `boot/tests/suites/multi_module_suite.tw`

**Interfaces:**
- Consumes: `checker.check(..., current_module, ...)`
- Consumes: opaque diagnostics from checker
- Produces: regression fixtures proving imported opaque types remain usable through functions and methods

- [ ] **Step 1: Add an opaque record provider fixture**

Create `boot/tests/fixtures/multi/opaque_secret.tw`:

```tw
pub opaque type Secret = .{ value: Int }

pub fn new(value: Int) Secret {
  Secret.{ value }
}

pub fn reveal(s: Secret) Int {
  s.value
}

pub fn bump(s: Secret) Secret {
  Secret.{ value: s.value + 1 }
}
```

- [ ] **Step 2: Add a positive consumer fixture**

Create `boot/tests/fixtures/multi/opaque_secret_use_ok.tw`:

```tw
use .opaque_secret
use .opaque_secret.{Secret}

fn main() Int {
  s: Secret = opaque_secret.new(41)
  s.bump().reveal()
}
```

Expected result: compiles and runs to `42` if executed as a program.

- [ ] **Step 3: Add negative record field access fixture**

Create `boot/tests/fixtures/multi/opaque_secret_field_bad.tw`:

```tw
use .opaque_secret
use .opaque_secret.{Secret}

fn main() Int {
  s: Secret = opaque_secret.new(1)
  s.value
}
```

Expected diagnostic substring:

```text
opaque type
```

- [ ] **Step 4: Add negative record construction fixture**

Create `boot/tests/fixtures/multi/opaque_secret_construct_bad.tw`:

```tw
use .opaque_secret.{Secret}

fn main() Secret {
  Secret.{ value: 1 }
}
```

Expected diagnostic substring:

```text
does not expose its representation
```

- [ ] **Step 5: Add opaque enum fixtures**

Create `boot/tests/fixtures/multi/opaque_token.tw`:

```tw
pub opaque type Token = {
  Ident(String),
  Number(Int),
}

pub fn ident(name: String) Token {
  .Ident(name)
}

pub fn describe(t: Token) String {
  case t {
    .Ident(name) => name,
    .Number(n) => n.to_string(),
  }
}
```

Create `boot/tests/fixtures/multi/opaque_token_match_bad.tw`:

```tw
use .opaque_token
use .opaque_token.{Token}

fn main() String {
  t: Token = opaque_token.ident("x")
  case t {
    .Ident(name) => name,
    .Number(n) => n.to_string(),
  }
}
```

Expected diagnostic substring:

```text
opaque type
```

- [ ] **Step 6: Add multi-module tests**

Add tests to `boot/tests/suites/multi_module_suite.tw` using the suite's existing `module_compiler.compile_entry` and `format_compile_error` pattern:

```tw
fn test_opaque_record_methods_cross_module() Result<Void, String> {
  dir := fixtures_dir()
  case module_compiler.compile_entry("${dir}/opaque_secret_use_ok.tw") {
    .Ok(_) => .Ok({}),
    .Err(err) => .Err("expected opaque positive fixture to compile, got: ${format_compile_error(err)}"),
  }
}

fn test_opaque_record_field_hidden_cross_module() Result<Void, String> {
  dir := fixtures_dir()
  case module_compiler.compile_entry("${dir}/opaque_secret_field_bad.tw") {
    .Ok(_) => .Err("expected opaque field access to fail"),
    .Err(err) => {
      msg := format_compile_error(err)
      try assert.str_contains(msg, "opaque type")
      .Ok({})
    },
  }
}

fn test_opaque_record_constructor_hidden_cross_module() Result<Void, String> {
  dir := fixtures_dir()
  case module_compiler.compile_entry("${dir}/opaque_secret_construct_bad.tw") {
    .Ok(_) => .Err("expected opaque construction to fail"),
    .Err(err) => {
      msg := format_compile_error(err)
      try assert.str_contains(msg, "does not expose its representation")
      .Ok({})
    },
  }
}

fn test_opaque_enum_pattern_hidden_cross_module() Result<Void, String> {
  dir := fixtures_dir()
  case module_compiler.compile_entry("${dir}/opaque_token_match_bad.tw") {
    .Ok(_) => .Err("expected opaque enum pattern to fail"),
    .Err(err) => {
      msg := format_compile_error(err)
      try assert.str_contains(msg, "opaque type")
      .Ok({})
    },
  }
}
```

Register the tests in `multi_module_suite.suite()` near the other import-boundary tests.

- [ ] **Step 7: Run multi-module verification**

Run:

```bash
target/twk run boot/tests/main.tw --filter multi
```

Expected before implementation: positive fixture may compile by accident, but negative fixtures do not produce opaque-boundary diagnostics.

Expected after implementation: positive fixture compiles; negative fixtures fail with opaque-boundary diagnostics.

- [ ] **Step 8: Commit cross-module coverage**

```bash
git add boot/tests/fixtures/multi/opaque_secret.tw boot/tests/fixtures/multi/opaque_secret_use_ok.tw boot/tests/fixtures/multi/opaque_secret_field_bad.tw boot/tests/fixtures/multi/opaque_secret_construct_bad.tw boot/tests/fixtures/multi/opaque_token.tw boot/tests/fixtures/multi/opaque_token_match_bad.tw boot/tests/suites/multi_module_suite.tw
git commit -m "Cover opaque type module boundaries"
```

---

### Task 5: Documentation and Candidate Stdlib Migration Notes

**Files:**
- Modify: `docs/spec.md`
- Modify: `docs/grammar.ebnf`
- Modify: `docs/API.md`
- Modify: `docs/plans/README.md`
- Test: documentation review through grep and compiler verification commands

**Interfaces:**
- Consumes: finalized syntax and semantics from Tasks 1-4
- Produces: documented language behavior and active-plan index entry

- [ ] **Step 1: Update the language spec**

Add to `docs/spec.md` near records/modules:

````md
### Opaque exported types

A module may export a nominal type while hiding its representation:

```tw
pub opaque type Secret = .{ value: Int }
```

Other modules may name `Secret`, pass it, return it, and call exported functions
or inherent methods whose first parameter is `Secret`. They may not construct the
record, read or update its fields, construct enum variants, or pattern-match enum
variants. The defining module can use the representation normally.

Opaque aliases are rejected because aliases do not create a representation
boundary:

```tw
pub opaque type UserId = Int // error
```
````

Also update the top-level item list to mention `pub opaque type` and state that bare `opaque type` is rejected because non-public types already have private module visibility.

- [ ] **Step 2: Update the EBNF grammar doc**

Update `docs/grammar.ebnf` so type declarations include the exact accepted modifier sequence. Use this shape, adjusted to match the file's existing notation:

```ebnf
type_decl ::= "type" type_name type_params? "=" type_def
            | "pub" "type" type_name type_params? "=" type_def
            | "pub" "opaque" "type" type_name type_params? "=" (record_type | sum_type)
```

Do not include bare `"opaque" "type"` as an accepted production.

- [ ] **Step 3: Update API docs for current examples**

In `docs/API.md`, add a short note near `@std.heap`, `@std.regexp`, `@std.view`, and `@std.buffer`:

```md
These APIs are natural candidates for `pub opaque type` once the feature is enabled:
callers should use constructors and inherent methods rather than depend on record
fields that are representation details.
```

Do not change the documented source declarations to `pub opaque type` until the boot compiler and bundled stdlib are migrated in a separate change.

- [ ] **Step 4: Add active plan index entry**

In `docs/plans/README.md`, add:

```md
| Opaque types | `pub opaque type` exports nominal types while hiding record fields or enum variants across module boundaries | Planned | [opaque-types.md](opaque-types.md) |
```

Place it under active cross-cutting plans.

- [ ] **Step 5: Run documentation grep checks**

Run:

```bash
rg -n "opaque type|pub opaque type|opaque-types" docs/spec.md docs/grammar.ebnf docs/API.md docs/plans/README.md docs/plans/opaque-types.md
```

Expected: the new syntax and plan link appear in all intended docs.

- [ ] **Step 6: Commit documentation**

```bash
git add docs/spec.md docs/grammar.ebnf docs/API.md docs/plans/README.md docs/plans/opaque-types.md
git commit -m "Document opaque type plan"
```

---

### Task 6: Final Verification and Formatting

**Files:**
- Verify all modified `.tw` and `.md` files

**Interfaces:**
- Consumes: all prior tasks
- Produces: complete implementation ready for review

- [ ] **Step 1: Format changed Twinkle files**

Run `target/twk fmt` on each changed `.tw` file from the tasks above. At minimum:

```bash
target/twk fmt boot/compiler/tokens.tw
target/twk fmt boot/compiler/lexer.tw
target/twk fmt boot/compiler/ast.tw
target/twk fmt boot/compiler/parser.tw
target/twk fmt boot/compiler/fmt/printer.tw
target/twk fmt boot/compiler/resolver.tw
target/twk fmt boot/compiler/query/analyze.tw
target/twk fmt boot/compiler/checker.tw
target/twk fmt boot/compiler/query/stage_runner.tw
target/twk fmt boot/lib/source/diagnostics.tw
target/twk fmt boot/compiler/query/diag_render.tw
target/twk fmt boot/tests/suites/parser_suite.tw
target/twk fmt boot/tests/suites/resolver_suite.tw
target/twk fmt boot/tests/suites/checker_suite.tw
target/twk fmt boot/tests/suites/multi_module_suite.tw
```

- [ ] **Step 2: Run linter**

Run:

```bash
target/twk lint boot/main.tw
```

Expected: no new lint findings introduced by opaque type work.

- [ ] **Step 3: Run boot test suite**

Run:

```bash
target/twk run boot/tests/main.tw
```

Expected: all boot tests pass.

- [ ] **Step 4: Run Rust tests if stage0 was touched**

If any files under `src/` were changed, run:

```bash
cargo test --release
```

Expected: Rust tests pass.

- [ ] **Step 5: Build the standalone CLI if bootstrap-sensitive files changed**

Run:

```bash
make quick-bundle-cli
```

Expected: `target/twk` is rebuilt successfully from the current `target/boot.wasm`.

- [ ] **Step 6: Commit verification follow-up**

If formatting or verification required changes:

```bash
git add boot docs
git commit -m "Finish opaque type verification"
```

If no files changed, record the verification commands and outputs in the implementation summary instead of creating an empty commit.

---

## Self-Review

**Spec coverage:** The plan covers syntax, parser/formatter support, resolver metadata, export/import preservation, checker enforcement, cross-module positive and negative cases, diagnostics, docs, and verification.

**Placeholder scan:** The plan intentionally fixes syntax, diagnostics, helper names, fixture names, and verification commands. It contains no deferred implementation holes.

**Type consistency:** The plan consistently uses `is_opaque` on `TypeDecl` and `TypeEntry`, `current_module` on `InferCtx`, and `can_access_type_representation` as the checker gate.
