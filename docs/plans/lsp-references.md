# LSP Find References Plan

## Goal

Implement `textDocument/references` so users can find uses of a symbol across a
Twinkle workspace.

---

## Scope

In scope:

* Functions, top-level bindings, local bindings, parameters, types, variants,
  record fields, and imports.
* Current document plus project modules reachable from the workspace graph.
* Include/exclude declaration based on `ReferenceParams.context.includeDeclaration`.

Out of scope for the first pass:

* Dynamic references through method values if the receiver type is unavailable.
* External package references.

---

## Design

Find references should be built around a stable symbol identity abstraction. The
query should first resolve the symbol under the cursor to an identity, then walk
candidate modules collecting spans that resolve to the same identity.

Potential identity forms:

* local binding: module key + binding id/scope path/name span
* top-level value: canonical module path + exported/internal name
* type: canonical module path + type name
* variant: parent type id + variant name
* field: record type id + field name
* import alias/item: imported target identity plus import binding span

This same identity layer should later support rename and document highlight.

---

## Implementation Steps

1. Add `ReferenceParams` decoding in `params.tw`.
2. Add a symbol-at-position query that returns the resolved identity and
   declaration span.
3. Add a reference collector over parsed/resolved/typed modules.
4. Add JSON location helpers or reuse definition helpers.
5. Advertise `referencesProvider: true`.
6. Handle `textDocument/references` in `server_core.tw`.
7. Add tests for local, module-level, imported, type, variant, and field
   references.

---

## Test Plan

* Local variable references are scoped correctly and ignore shadowed names.
* Imported function references match the imported target.
* Type references include annotations and constructors where appropriate.
* Variant references include patterns and expression constructors.
* Field references are type-aware and do not match unrelated records with the
  same field name.
* `includeDeclaration` controls declaration inclusion.
* Multibyte text before references maps locations correctly.

---

## Exit Criteria

Find references returns precise, scope-aware locations for the common Twinkle
symbol kinds and provides the reusable symbol-identity foundation for rename.
