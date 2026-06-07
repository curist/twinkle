# Occurrence Map Plan

**Status: COMPLETE / ARCHIVED.** The occurrence index is built (lexical + checker
enrichment), cached as a `SemanticSnapshot` artifact, and consumed by semantic
tokens, definition, references, and document highlights — all agreeing on
shadowed locals and imported aliases through one `SymbolKey`-backed index. Phase
6's shared helpers landed too: `compiler.query.call_resolve` (callee → signature
for hover/signature-help/inlay-hints) and `type_util.subst_type_params_lenient`
(one tolerant substitution, replacing three copies).

Two plan items were assessed and deliberately **not** done; rationale is recorded
inline in Phases 5–6:

- hover, completion, and inlay-hint *type* features stay `type_map`-driven.
  Occurrences carry symbol identity, not `MonoType`, so they cannot supply the
  type text/member lists those features need (and completion additionally needs
  mid-edit cursor-hole reparses the index does not cover).
- The `*_for_type` type-introspection helpers were not extracted: after the
  references migration, `definition` reads declaration spans from cached ASTs
  while hover/completion read resolved types from `ResolvedEnv + MonoType` —
  different input domains with different output shapes, so a shared helper would
  be contrived.

Bootstrapping this work surfaced a pre-existing stage0 resolver bug (records were
resolved before aliases, so an alias-typed record field expanded to `Void`);
fixed separately in `src/types/resolve.rs` with a typecheck regression fixture.

## Goal

Replace ad-hoc identifier classification in editor features with a shared
semantic occurrence map produced by the frontend pipeline.

An occurrence map answers one question consistently:

> For this source span / AST node, what declared symbol does this occurrence
> resolve to, and how should tooling describe it?

This should become the common source of truth for semantic tokens, hover,
definition, references, document highlights, rename/preparation, inlay hints,
and completion context checks.

---

## Motivation

Today several query modules independently re-walk the AST and rebuild partial
scope knowledge:

- semantic tokens classify identifiers as variables, parameters, functions,
  namespaces, properties, enum members, and methods.
- definition/references/document-highlight need declaration/use relationships.
- hover/inlay/completion need type information at expression positions.
- some features infer whether an identifier-like token is a module alias, local
  binding, parameter, type, function, field, or variant from environment lookups
  plus local heuristics.

This creates repeated work and subtle drift. For example, `groups.values()` was
highlighted as a namespace receiver because semantic tokens only checked the
resolved environment and parameter names; it did not have a reusable lexical
binding map for local lets. `semantic_tokens.tw` now consumes the occurrence map,
which keeps that regression covered without a private local-name classifier.

The occurrence map should make these cases direct:

```tw
groups.values()
^^^^^^ local variable use, resolves to the `groups` let binding
       ^^^^^^ inherent method call, resolves to `Dict.values`/receiver method metadata
```

---

## Redundancy Inventory

An audit of the query/LSP layer found several ad-hoc semantic-resolution pockets
beyond semantic tokens. The occurrence map (plus a couple of adjacent shared
helpers) is meant to absorb them. This is the concrete target list.

1. **Symbol identity / lexical binding walks** —
   `references.tw` (private `SymbolId` + scope/reference collector),
   `definition.tw` (`resolve_local_binding`, `find_local_in_block`, pattern
   binding lookup), `semantic_tokens.tw` (temporary `local_names`/`param_names`),
   `completion.tw` (independent in-scope param/let collection). These converge on
   `occurrences.tw` / `SymbolKey`.

2. **Receiver/member type resolution in completion** — `completion.tw` reparses
   with a cursor hole, resolves the receiver by name, scans previous lets, and
   falls back to stale `type_map` ids. Once occurrences exist, member completion
   should be: occurrence before dot → symbol key → symbol/type detail → member
   list, instead of re-deriving local binding types by hand.

3. **Method/call resolution duplicated** — `hover.tw`, `signature_help.tw`,
   `inlay_hints.tw`, `definition.tw`, `semantic_tokens.tw` each separately inspect
   combinations of `typed.method_calls`, `typed.type_map`,
   `env.lookup_function`, `env.lookup_type_method`, and module-qualified names.
   Extract a shared `resolved_call_info` / `callee_info` helper built around
   checker `method_calls` plus resolver origins.

4. **Type substitution duplicated despite `type_util.tw`** — `hover.tw`,
   `completion.tw`, `signature_help.tw` each define a local
   `substitute_type_params`, while `type_util.subst_type_params` exists. The query
   copies are tolerant of missing args where `type_util` traps, so the fix is a
   single query-safe wrapper, not three copies.

5. **Field / variant lookup repeated** — `definition.tw`, `references.tw`,
   `hover.tw`, `completion.tw` each do "given type + field/variant name, find
   resolved info / declaration / display text". Centralize as type-introspection
   helpers over `ResolvedEnv + MonoType`: `field_info_for_type`,
   `variant_info_for_type`, `methods_for_type` (some already private in
   `hover.tw`).

6. **AST cursor/path walking duplicated** — `ast_walk.tw`, `ast_path.tw`,
   `definition.tw`, `signature_help.tw`, `completion.tw`, `semantic.tw`. This is
   query infra rather than checker reuse; longer term either `ast_path.tw` becomes
   the shared parent-chain cursor API, or occurrence/call/context indexes replace
   many of these walks. Out of scope for the core occurrence map; tracked here so
   it is not lost.

Suggested order (drives the phase plan below): wire occurrences into the
snapshot → migrate semantic tokens and delete `local_names` → migrate
definition/references to `SymbolKey` → extract shared call/member/type-
introspection helpers → replace duplicated type substitution with a `type_util`
wrapper.

---

## Non-goals

- Do not run lowering, monomorphization, optimization, linking, or codegen for
  editor queries.
- Do not replace the type checker or resolver. The occurrence map records their
  decisions in a tooling-friendly shape.
- Do not require a perfect rename implementation in the first iteration.
- Do not invent structural record identity. Record fields should still respect
  nominal type information from checking.

---

## Current Baseline

`SemanticSnapshot` already exposes:

- `parsed`: AST and source spans.
- `resolved`: resolved environment and diagnostics.
- `typed`: checker result, including the checked environment and selected typed
  metadata such as expression type maps and method-call metadata.

Limitations:

- `ResolvedEnv` is mostly an environment of declarations/imports/module values,
  not a lexical occurrence table.
- Local lets, loop binders, collect binders, closure parameters, pattern binders,
  and shadowing are not exposed as reusable symbol identities.
- Query features manually traverse AST scopes and often duplicate partial logic.
- Many lookups are name-based, so shadowing and imported aliases are easy to get
  wrong.
- Spans are available in the AST, but declaration/use relationships are not
  represented uniformly.

---

## Target Architecture

Add a frontend-produced per-file occurrence index to semantic snapshots:

```tw
pub type SemanticSnapshot = .{
  ...
  occurrences: OccurrenceIndex?,
}
```

The index should be built after parse/resolve/check, using the AST plus resolver
and checker metadata. It should be cached as a query artifact keyed by the same
semantic inputs as typed results. Cross-file queries should load the per-file
indexes for candidate modules from the query cache instead of relying on a single
snapshot-local index.

High-level shape:

```tw
pub type SymbolKind = {
  Module,
  Type,
  TypeParameter,
  Function,
  Method,
  Parameter,
  Local,
  ModuleValue,
  Field,
  Variant,
  ExternNamespace,
}

pub type SymbolKey = {
  Local(String, Int),                  // declaring module path + per-file local id
  Function(String, String),            // canonical module path + function name
  TypeDef(String, String),             // canonical module path + type name
  TypeParam(String, Int),              // declaring module path + per-file local id
  ModuleAlias(String, Int),            // importing module path + per-file local id
  ModuleValue(String, String),         // canonical module path + value name
  Field(String, String, String),       // canonical module path + type name + field
  Variant(String, String, String),     // canonical module path + type name + variant
  ExternNamespace(String),
}

pub type SymbolDef = .{
  key: SymbolKey,
  name: String,
  kind: SymbolKind,
  declaration_span: Span?,
  declaration_uri: String?,
  detail: SymbolDetail,
}

pub type OccurrenceRole = { Declaration, Reference, Write, Import, Shorthand }

pub type Occurrence = .{
  symbol_key: SymbolKey,
  name: String,
  kind: SymbolKind,
  role: OccurrenceRole,
  span: Span,
  expr_id: Int?,
  type_expr_span: Span?,
}

pub type OccurrenceIndex = .{
  module_path: String,
  uri: String,
  symbols: Vector<SymbolDef>,
  occurrences: Vector<Occurrence>,
  by_expr: Dict<Int, Int>,             // expr id -> occurrence index
  by_symbol: Dict<String, Vector<Int>>, // encoded symbol key -> occurrence indexes
  sorted_spans: Vector<Int>,           // occurrence indexes sorted by span start/end
}
```

`Dict` keys are limited to `Int`/`String` by the runtime `hash_key` (it only
handles i31, boxed int, and string; other key shapes trap). `SymbolKey` is an
enum, so it cannot be a `Dict` key directly. The occurrence module must provide a
total, injective `encode_symbol_key(SymbolKey) String` and key `by_symbol` on the
encoded string. The same encoding is what cross-file consumers compare when
matching symbols across per-file indexes.

The exact representation can differ, but it should support:

- lookup by arbitrary cursor offset using `span.contains(offset)` semantics,
- lookup by expression id,
- declaration-to-references queries within one file and across cached indexes,
- occurrence-to-definition queries,
- semantic-token classification without redoing scope resolution.

`sorted_spans` is intentionally not a start-offset dictionary. LSP cursor hits
usually land inside an identifier, not exactly at its first byte. The helper API
should provide `occurrence_at_offset(index, offset)` and may implement it with a
sorted interval list plus local scan, or with a linear scan initially if the
helper hides the implementation.

---

## Symbol Identity Rules

### Per-file indexes, cross-file keys

Occurrence indexes are per file because parse/check artifacts and source spans
are per file. Symbol keys, however, must be comparable across files for imported
functions/types/fields/variants/module values. This lets `references.tw` search
candidate modules by loading each candidate module's occurrence index and
matching `SymbolKey` values.

Local lexical symbols remain file-local. Their keys include the declaring module
path plus a per-file local id, so they are unique within a workspace query but do
not need to be stable across edits.

### Relationship to existing `references.SymbolId`

`references.tw` already has a private `SymbolId` enum with variants such as
`Local`, `Func`, `TypeDef`, `Variant`, and `Field`. The occurrence map should not
introduce a second ambiguous `SymbolId` name. Use `SymbolKey` in the shared
occurrence module and migrate `references.tw` by either:

- replacing its private `SymbolId` with `occurrences.SymbolKey`, or
- temporarily adding conversion helpers while references is migrated.

The final state should have one shared identity type for query features.

### Cross-module symbols

Imported functions/types/module values should preserve canonical origin metadata
where available:

- function origin: canonical module path + source function name,
- type origin: canonical module path + source type name,
- module alias/import: importing module path + local symbol key,
- extern namespace: namespace string plus declaration/import location.

### Local lexical symbols

Local symbols should be allocated for:

- function parameters,
- closure parameters,
- `let` bindings,
- rebindings/write occurrences,
- `for` pattern/index binders,
- `collect` pattern/index binders,
- pattern variables in `case` arms.

Lexical scopes should model shadowing explicitly. A reference occurrence should
point to the nearest in-scope symbol, not merely carry a name.

### Field and method symbols

Field and method occurrences need type-aware classification:

- record field access should identify the receiver type and field name,
- record literal/update entries should identify fields when contextual type is
  known,
- inherent method calls should use checker method-call metadata,
- module-qualified calls should resolve the namespace occurrence separately from
  the function/method occurrence.

For fields, a synthetic symbol identity based on canonical type origin + field
name is preferred for cross-file references. A nominal type id + field name is
acceptable internally only if it can be converted to a stable `SymbolKey` before
it leaves the occurrence builder.

### Variants

Variant occurrences should identify the enum type when checker information makes
that available. Qualified variants and bare `.Variant` should share the same
variant symbol when resolved.

---

## Work Plan

### Phase 1 — Define occurrence data model

Purpose: add shared types without changing query behavior.

Checklist:

- [x] Add `boot/compiler/query/occurrences.tw` with symbol, occurrence, and
      index types, using `SymbolKey` rather than a second `SymbolId`.
- [x] Add helper lookups: occurrence at arbitrary cursor offset, occurrences for
      symbol, definition for occurrence.
- [x] Add a total, injective `encode_symbol_key(SymbolKey) String` and key
      `by_symbol` on it (Dict keys can only be Int/String at runtime).
- [x] Decide the initial `sorted_spans` implementation behind the helper API:
      hidden linear scan over sorted spans.
- [x] Add conversion helpers for LSP token kinds only as consumers, not core
      occurrence concepts. `semantic_tokens.tw` owns `token_kind(SymbolKind)`;
      the occurrence module stays free of LSP-specific kinds.
- [x] Document which source constructs should emit declaration and reference
      occurrences.

Acceptance:

- New module compiles and has focused unit coverage for index lookup helpers,
  including cursor offsets in the middle of an identifier.

---

### Phase 2 — Build lexical occurrence index from AST

Purpose: centralize local scope tracking that semantic tokens and references
currently duplicate.

Checklist:

- [x] Implement an AST walk that allocates local symbols for params, lets,
      closures, loops, collect, and pattern binders.
- [x] Track lexical scopes and shadowing.
- [x] Emit declaration occurrences and identifier reference occurrences.
- [x] Attach expression ids where available.
- [x] Add regression coverage for local shadowing, nested blocks, closures,
      loops, collect, and pattern variables.
- [x] Mark the current `semantic_tokens.tw` local-name tracker as temporary and
      avoid extending it beyond regression fixes.

Acceptance:

- `groups.values()` records `groups` as a local reference to the `groups` let
  declaration.
- No query consumer has to independently track local names for this case.

---

### Phase 3 — Integrate resolver/checker metadata

Purpose: enrich occurrences with global, type, method, field, and variant
resolution.

Checklist:

- [x] Emit symbols/occurrences for function declarations and function calls.
- [x] Emit symbols/occurrences for type declarations, type references, and type
      parameters.
- [x] Emit selective import occurrences from `use` declarations.
- [x] Use checker `method_calls` metadata for method occurrences.
- [x] Use expression/type maps for precise field and variant classification when
      available.
- [x] Represent unresolved/error occurrences conservatively so editor features
      still return partial results.

Acceptance:

- Occurrence index can distinguish local receivers, module-qualified calls,
  record fields, methods, enum variants, type refs, and type parameters.

---

### Phase 4 — Cache and expose in `SemanticSnapshot`

Purpose: make occurrences a normal query artifact.

Checklist:

- [x] Add a small occurrence builder layered after typed results.
- [x] Store/retrieve per-file occurrence indexes in `compiler.query.cache`.
- [x] Add `occurrences` to `SemanticSnapshot` and populate it for the snapshot's
      entry file.
- [x] Add a helper for workspace consumers to load/build occurrence indexes for
      candidate modules from the cache.
- [x] Ensure parse or type errors still return partial lexical occurrences when
      useful.
- [x] Add cache coverage showing snapshots store occurrence indexes.
- [x] Add invalidation tests showing occurrences update after edits.

Acceptance:

- LSP handlers can obtain `snap.occurrences` for the active file alongside
  parsed/resolved/typed artifacts without running lower/codegen.
- Cross-file consumers can load per-file occurrence indexes for candidate
  modules without inventing a separate workspace-wide index format.

---

### Phase 5 — Migrate consumers incrementally

Purpose: remove duplicated scope/classification logic while keeping behavior
stable.

Recommended order:

1. `semantic_tokens.tw` — done
   - classify tokens from occurrence kind/role;
   - keep syntax-only fallback for comments/literals/unknown nodes if needed.
2. `definition.tw` — done for occurrence-backed symbols
   - use occurrence-to-definition mapping.
   - keep targeted syntax fallback for imports/module aliases and type-definition
     queries that are not represented as ordinary identifier occurrences.
3. `references.tw` and `document_highlight.tw` — done
   - replace or bridge the private `references.SymbolId` with shared
     `occurrences.SymbolKey`;
   - group by `SymbolKey`, not by name/span heuristics;
   - load per-file occurrence indexes for candidate modules from the cache.
4. `hover.tw` — not migrated to occurrences (by design)
   - hover answers "what is the type of the expression here", which is a
     `type_map` lookup keyed by expression id. Occurrences deliberately carry
     symbol identity/kind/role, not `MonoType`, so they cannot supply the type
     text hover needs. Hover keeps its `type_map`-driven path; what it shared
     with the other call consumers (callee → signature) moved to
     `call_resolve` (Phase 6).
5. `completion.tw` — not migrated to occurrences (by design)
   - member completion needs the receiver's `MonoType` to enumerate fields and
     methods, which occurrences do not carry. It also runs on mid-edit source
     where occurrences (built from the last good parse) lag the cursor, which is
     exactly why completion reparses with a cursor hole. The occurrence index
     does not replace either need, so completion keeps its receiver resolution.
6. `inlay_hints.tw` — type hints stay `type_map`-driven (same reason as hover);
   parameter-name hints now resolve the callee via the shared `call_resolve`
   helper instead of a private resolver.

Checklist:

- [x] Migrate semantic tokens behind existing LSP regression tests.
- [x] Migrate the occurrence-backed consumers behind tests: definition,
      references, and document highlights consume occurrences. hover, completion,
      and inlay-hint *type* features stay `type_map`-driven because occurrences
      carry symbol identity, not `MonoType` — see notes 4–6 above.
- [x] Replace the private `references.SymbolId` and `definition.tw` local-binding
      walks (`resolve_local_binding`, `find_local_in_block`) with `SymbolKey`.
- [x] Delete obsolete local-scope walkers once no consumer needs them. The
      semantic-token bridge and definition/references local walkers are gone.
      `completion.tw`'s receiver param/let collection stays: it serves mid-edit
      cursor-hole reparses that the occurrence index does not cover.
- [x] Keep focused regression cases for shadowing, imports, methods, fields, and
      variants.

Acceptance:

- Semantic tokens no longer maintain an independent local-name context.
- Definition/references agree on shadowed local names and imported aliases.
- Receiver/callee resolution for hover, signature help, and inlay hints lives in
  one `call_resolve` helper rather than three private copies. (Member completion
  keeps its own receiver path for mid-edit reparses, per note 5.)

---

### Phase 6 — Extract shared call / type-introspection helpers

Purpose: collapse the non-lexical duplication surfaced in the redundancy
inventory (items 3–5). These helpers complement the occurrence index rather than
living inside it, so they are sequenced after consumers read occurrences.

Checklist:

- [x] Add a shared callee resolver: `compiler.query.call_resolve` exposes
      `resolve_callee` (callee → `FunctionSig` + receiver flag + receiver expr)
      and one `instantiate_receiver_sig`. hover, signature help, and inlay hints
      call it instead of re-inspecting `method_calls` +
      `lookup_function`/`lookup_registered_function`. definition and semantic
      tokens do not need it: definition resolves to declaration spans (not
      signatures) and semantic tokens classify from occurrences.
- [~] Type-introspection helpers over `ResolvedEnv + MonoType` — not extracted.
      After the references migration the remaining field/variant lookups split by
      input domain: `definition.tw` reads declaration *spans* from cached parsed
      ASTs, while `hover.tw`/`completion.tw` read resolved field/variant *types*
      from `ResolvedEnv + MonoType`, and even those two differ in output shape
      (hover: one variant's display text; completion: all fields/methods as
      items). A single `*_for_type` helper would serve only part of one consumer,
      so this was assessed and deliberately left in place rather than forced.
- [x] Replace the three local `substitute_type_params` copies (`hover.tw`,
      `completion.tw`, `signature_help.tw`) with `type_util.subst_type_params_lenient`,
      a query-safe variant of `subst_type_params` that leaves an unmatched type
      variable unchanged instead of trapping.

Acceptance:

- Callee → signature resolution for the call consumers lives in `call_resolve`.
- No query module defines its own `substitute_type_params`.

Out of scope (tracked, not scheduled): consolidating the AST cursor/path walkers
(`ast_walk.tw`, `ast_path.tw`, and the per-feature walks in `definition.tw`,
`signature_help.tw`, `completion.tw`, `semantic.tw`) into a shared parent-chain
cursor API. Revisit once occurrence/call indexes have removed their semantic
duties.

---

## Testing Strategy

Use red/green tests under `boot/tests` for each migration.

Core occurrence tests:

- local let declaration/use,
- rebinding/write occurrence,
- function parameter declaration/use,
- nested scope shadowing,
- closure parameter shadowing outer locals,
- loop and collect binders,
- case pattern binders,
- module alias and module-qualified function call,
- selective import of function/type/variant,
- type parameter declaration/use,
- method call on local receiver,
- record field declaration/access/literal entry,
- enum variant declaration/use,
- unresolved identifier fallback.

Consumer regression tests:

- semantic token classification for the same constructs,
- go-to-definition from each reference to the correct declaration,
- references grouped by `SymbolKey` rather than text name,
- document highlight only highlights the same binding under shadowing.

---

## Open Questions

- Should occurrences be produced by the checker directly, or by a query pass
  after checking?

  Initial recommendation: build it as a query pass after checking. This avoids
  bloating checker responsibilities while still using checker metadata. If the
  pass needs too much duplicated resolution logic, move specific hooks into the
  checker later.

- How precise should partial/error files be?

  Initial recommendation: emit syntax/lexical occurrences even when typecheck
  fails, then enrich only the parts that checker metadata can prove.

- Do symbol keys need to be stable across edits?

  Initial recommendation: cross-file declaration keys must be stable enough to
  compare separate per-file indexes in one workspace query. Local lexical keys do
  not need to be stable across edits; they only need to be unique within the
  declaring module's current occurrence index.

- Should module aliases be symbols?

  Initial recommendation: yes. They have declarations (`use foo as bar`) and
  references (`bar.fn()`), and semantic tokens need to distinguish them from
  locals.

---

## Risks

- The occurrence builder may duplicate parts of resolver/checker if the boundary
  is not kept clear.
- A poorly specified cursor lookup structure could force consumers back to
  ad-hoc span scans; hide the implementation behind `occurrence_at_offset` from
  the start.
- Cross-file references can drift if per-file indexes use snapshot-local ids for
  imported/global symbols; require stable `SymbolKey` values for cross-module
  symbols.
- Field and variant resolution may require additional checker metadata for full
  precision.
- Migrating all query consumers at once would be risky; migrate incrementally.
- Partial/error recovery can complicate the data model if unresolved occurrences
  are not represented explicitly.

---

## Success Criteria

- Editor features classify and navigate identifiers from shared per-file
  occurrence indexes rather than independent heuristics.
- Shadowing behavior is consistent across semantic tokens, definition,
  references, and document highlights.
- The `groups.values()` class of bug is impossible because local receiver
  references resolve to local symbols before namespace classification.
- Query features remain frontend-only and do not invoke lowering/codegen.
