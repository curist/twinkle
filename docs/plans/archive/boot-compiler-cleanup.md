# Boot Compiler Cleanup: High-Value Refactors

Four targeted refactors to improve modularity, reduce duplication, and make
the boot compiler easier to maintain. Each is independent and can be done in
any order.

---

## 1. Extract `fix_unused_imports` from `main.tw`

### Problem

`main.tw` contains ~100 lines of import-fix logic (lines 425–525): re-running
full analysis, parsing JSON diagnostic payloads, deduplicating edits by span,
sorting in reverse byte order, and applying string slicing. This is compiler
logic embedded in the CLI entry point.

### Current structure

```
main.tw
  └─ fix_unused_imports()       # 94 lines
       ├─ re-runs analyze.analyze_module
       ├─ parses json diagnostic data fields
       ├─ deduplicates ImportFix records by span
       ├─ sorts edits descending by byte offset
       └─ applies edits via string slicing + fs.write_text
```

### Proposed structure

Create `boot/compiler/fix_unused_imports.tw`:

```tw
pub type ImportFix = .{
  use_start: Int,
  use_end: Int,
  replacement: String,
}

/// Extract unused-import fixes from analysis diagnostics.
pub fn collect_fixes(diagnostics: Vector<AnalysisDiag>) Dict<String, Vector<ImportFix>>

/// Apply fixes to source files on disk. Returns list of (path, edit_count).
pub fn apply_fixes(edits_by_file: Dict<String, Vector<ImportFix>>) Vector<ApplyResult>
```

`main.tw` reduces to:

```tw
fn fix_unused_imports(file: String) {
  // ... setup analyze state (stays here — CLI concern) ...
  edits := fix.collect_fixes(a_result.diagnostics)
  results := fix.apply_fixes(edits)
  for r in results { println("Fixed: ${r.path} (${r.count} edit(s))") }
}
```

### Steps

1. Create `boot/compiler/fix_unused_imports.tw`.
2. Move `ImportFix` type and the diagnostic-parsing / dedup / apply logic.
3. Export `collect_fixes` and `apply_fixes`.
4. Update `main.tw` to call the new module.
5. Verify `twk check --fix-unused-imports` still works on a file with unused
   imports.

---

## 2. Break up `core_linker.tw`

### Problem

`core_linker.tw` is 715 lines handling five distinct responsibilities:

| Responsibility | Lines | Notes |
|---|---|---|
| FuncId assignment + remap tables | 1–61 | Step 1 |
| External ref validation | 64–278 | Step 2, `validate_compiled_module_links` |
| Expression tree remapping | 72–174, 609–694 | Step 3, `remap_expr` + helpers |
| Combined init synthesis | 280–337 | Step 4, `build_combined_init` |
| Dead code elimination (BFS) | 339–607 | Step 5, `compute_reachable` + contract resolution |

The DCE section is particularly problematic: it includes contract-call
resolution logic (`resolve_contract_target_name`, `contract_call_refs`) that
duplicates type-dispatch knowledge from the checker, and a fragile fallback
(`fallback_method_names_for_func`) that parses function names by scanning
for ASCII `.` (byte 46) to guess method associations.

### Proposed structure

Split into three files:

```
boot/compiler/core_linker.tw          (~200 lines)
  ├─ link() — public API, orchestrates steps 1–5
  ├─ FuncId assignment (step 1)
  ├─ external ref validation (step 2)
  ├─ build_combined_init (step 4)
  └─ remap_func_id, remap_expr, remap_exprs (step 3)

boot/compiler/core_linker/dce.tw      (~200 lines)
  ├─ compute_reachable() — public entry
  ├─ BfsState, ReachabilityIndex
  ├─ bfs_reachable, get_body_refs, add_refs
  ├─ collect_func_refs_into, collect_func_refs_list
  └─ fallback_method_names_for_func, append_unique_*

boot/compiler/core_linker/contract_resolve.tw  (~50 lines)
  ├─ resolve_contract_target_name
  ├─ exact_contract_ref
  ├─ contract_call_refs
  └─ fallback_contract_refs
```

### Why separate contract resolution

`resolve_contract_target_name` contains a hardcoded type dispatch table
(Int/Float/Bool/Byte/String/Vector/Dict/Optional/Result/Named). This is the
same pattern flagged in `hover.tw`'s `method_function_name`. Isolating it
makes it easier to:

- Keep it in sync with new built-in types
- Eventually replace it with a registry lookup
- Test it independently

### Steps

1. Create `boot/compiler/core_linker/` directory.
2. Extract DCE functions into `dce.tw`.
3. Extract contract resolution into `contract_resolve.tw`.
4. Update imports in `core_linker.tw` to `use .core_linker.dce` and
   `.core_linker.contract_resolve`.
5. Verify `twk build boot/main.tw` produces identical output.

---

## 3. Deduplicate LSP JSON helpers

### Problem

`range_to_json` and `position_to_json` are copy-pasted across four files:

- `boot/lib/lsp/diagnostics.tw`
- `boot/lib/lsp/hover.tw`
- `boot/lib/lsp/definition.tw`
- `boot/lib/lsp/code_action.tw`

Each copy is identical: `Position → { line, character }` and
`TextRange → { start, end }` JSON encoding. Adding a new field (e.g.,
position encoding metadata) requires four edits.

### Proposed structure

Create `boot/lib/lsp/range.tw`:

```tw
use lib.source.line_index
use lib.json

pub fn position_to_json(pos: line_index.Position) json.Json {
  json.object([
    json.kv("line", json.int(pos.line)),
    json.kv("character", json.int(pos.character)),
  ])
}

pub fn range_to_json(range: line_index.TextRange) json.Json {
  json.object([
    json.kv("start", position_to_json(range.start)),
    json.kv("end", position_to_json(range.end)),
  ])
}
```

Update all four files to `use .range` and call `range.range_to_json(...)` /
`range.position_to_json(...)`.

### Steps

1. Create `boot/lib/lsp/range.tw` with the two functions.
2. Update `diagnostics.tw`, `hover.tw`, `definition.tw`, `code_action.tw`
   to import and use the shared module.
3. Remove the local copies.
4. Verify LSP still works (hover, goto-def, diagnostics, code actions).

---

## 4. Fix quadratic workspace diagnostics in LSP

### Problem

`server_core.publish_workspace_diagnostics` (line 293) iterates all open
documents and calls `analyze_workspace` once per document:

```tw
for doc in open_docs {
  result := query_diagnostics.analyze_workspace(cur_cache, .{
    entry_uri: doc.uri,
    ...
  })
  // ...
}
```

If N files from the same project are open, this runs N full frontend analyses.
The cache makes subsequent runs fast (mostly cache hits), but:

- The first analysis after a change still re-analyzes all transitive deps
  once per open file until the `published` set catches up.
- Each iteration rebuilds the overlay and walks the dependency graph.
- With many open files this shows up as a noticeable pause after edits.

### Current flow

```
didChange("foo.tw")
  → publish_workspace_diagnostics
    → for each open doc:
        analyze_workspace(entry=doc, overlay=all_open_docs)
        # walks entire dep graph from doc as entry
        # publishes diagnostics for doc + transitive deps
    → skip docs already in `published` set
```

### Proposed flow

Group open documents by project root before analysis. Run one
`analyze_workspace` per unique (project_root, entry) pair rather than per
open document:

```tw
fn publish_workspace_diagnostics(state: State) Step {
  open_docs := state.documents.docs.values()
  overlay_docs := collect doc in open_docs {
    query_diagnostics.DocumentInput.{ uri: doc.uri, text: doc.text, version: doc.version }
  }

  // Group open docs by project root
  by_project: Dict<String, Vector<document_store.Document>> = Dict.new()
  for doc in open_docs {
    root := project_root_for_document(doc.identity.path)
    existing := case by_project[root] {
      .Some(ds) => ds,
      .None => [],
    }
    by_project[root] = existing.append(doc)
  }

  cur_cache := state.query_cache
  outgoing: Vector<json.Json> = []
  published: Dict<String, Bool> = Dict.new()

  for root in by_project.keys() {
    docs := case by_project[root] { .Some(ds) => ds, .None => { continue } }
    // Use first doc as entry — analysis walks all deps anyway
    entry := docs[0]
    result := query_diagnostics.analyze_workspace(cur_cache, .{
      entry_uri: entry.uri,
      project_root: root,
      open_documents: overlay_docs,
    })
    cur_cache = result.cache

    // Publish diagnostics for all docs in this project
    for group in result.diagnostics_by_uri {
      // ... same publish logic, but now runs once per project ...
    }
  }

  // ...
}
```

This reduces N analyses per project to 1 (or at worst, one per distinct
project root among open files).

### Caveat

This assumes all files in the same project root share a dependency graph
reachable from any single entry. If two open files in the same project have
disjoint dependency graphs, the single-entry analysis may miss diagnostics
for the second file's unique deps. If this is a concern, a safer approach is
to track which open-doc URIs were covered by each analysis result and only
run additional analyses for uncovered docs.

### Steps

1. Add project-root grouping logic in `publish_workspace_diagnostics`.
2. Run one `analyze_workspace` per project root group.
3. Track covered URIs from each analysis to decide if additional entries
   need analysis.
4. Test with multiple open files from the same project — verify diagnostics
   appear for all of them.
5. Test with files from different projects open simultaneously.

---

## 5. Extract shared AST offset walker for hover and definition

### Problem

`hover.tw` (861 lines) and `definition.tw` (812 lines) are the two largest
files in the query layer. Both implement recursive AST walks that find the
deepest node containing a byte offset, following the same structural pattern:

1. Check `span.contains(offset)` — return early if false
2. Walk children depth-first
3. Return the first child match, or fall back to the current node

Both files independently implement walkers for the same AST node types:

| AST level | hover.tw | definition.tw |
|---|---|---|
| Items | inline loop in `hover()` | `find_*_ref_in_items()` |
| Block | `hover_block()` | `find_*_ref_in_block()` |
| Stmt | `hover_stmt()` | `find_*_ref_in_stmt()` |
| Expr | `hover_expr()` + `hover_expr_children()` | `find_expr_ref_in_expr()` |
| Type | `hover_type_expr()` | `find_type_ref()` |
| Pattern | `hover_pattern()` | `find_pattern_variant_in_pattern()` |
| Expr list | `hover_exprs()` | `find_expr_refs()` |
| Record entries | `hover_record_entries()` | `find_expr_ref_in_entries()` |
| Collect | `hover_collect()` | `find_expr_ref_in_collect()` |

The block and stmt walkers are nearly identical in structure — only the
"what to do when you find the node" part differs.

### Proposed structure

Create `boot/compiler/query/ast_walk.tw` with a shared offset-based walker
that returns a `NodeContext` — enough information for both hover and
definition to do their job without re-walking:

```tw
/// A node found at a byte offset, with context about where it sits.
pub type NodeContext = {
  ExprNode(Expr, ExprContext),
  StmtNode(Stmt),
  TypeNode(TypeExpr),
  PatternNode(Pattern),
  ItemNode(Item),
}

pub type ExprContext = .{
  /// The parent expr, if this node is a child of another expr.
  parent: Expr?,
  /// If this is a method call (.Field), the base expression.
  method_base: Expr?,
}

/// Find the deepest AST node containing `byte_offset`.
pub fn find_node_at_offset(items: Vector<Item>, byte_offset: Int) NodeContext?
```

Internally, the shared walker handles the mechanical span-checking and
child descent. Hover and definition then pattern-match on `NodeContext` to
do their specific work (type lookup vs reference resolution).

### What stays separate

- **hover.tw**: type display logic (`sig_to_string`, `instantiate_receiver_sig`,
  `expr_type`, doc comment extraction), method hover
  (`hover_method`, `hover_recorded_method`, `method_function_name`)
- **definition.tw**: reference resolution (`resolve_ref`, `resolve_local_binding`,
  `find_local_in_block`, `dependency_path_for_alias`), the `Ref` type system

These are genuinely different responsibilities. The shared walker only
extracts the "find which node the cursor is on" boilerplate.

### Expected impact

- Each file shrinks by ~200–300 lines (the mechanical walking code)
- New LSP features (rename, references, semantic tokens) can reuse the walker
  instead of writing yet another AST traversal
- Bug fixes to span handling (e.g., edge cases at node boundaries) happen
  in one place

### Steps

1. Create `boot/compiler/query/ast_walk.tw` with `NodeContext` types and
   `find_node_at_offset`.
2. Implement the shared walker covering items, blocks, stmts, exprs, types,
   and patterns.
3. Refactor `hover.tw` to call `find_node_at_offset` and match on the
   result, removing its own walking code.
4. Refactor `definition.tw` similarly — the `find_*_ref_in_*` functions
   collapse into pattern matches on `NodeContext`.
5. Verify hover and goto-definition still work on: identifiers, field
   access, method calls, type annotations, patterns, imports, doc comments.

---

## 6. Fix incomplete `method_function_name` dispatch in hover

### Problem

`hover.tw` has a `method_function_name` function (lines 555–596) that
resolves method-call hover info by dispatching on the receiver's type:

```tw
fn method_function_name(base: Expr, method: String, typed: CheckResult) String? {
  case typed.type_map[base.id] {
    .Some(.Named(tid, _)) => typed.env.lookup_type_method(tid, method),
    .Some(.Vector(_)) => typed.env.lookup_method("Vector", method),
    .Some(.Dict(_, _)) => typed.env.lookup_method("Dict", method),
    .Some(.String) => typed.env.lookup_method("String", method),
    .Some(.Int) => typed.env.lookup_method("Int", method),
    .Some(.Float) => typed.env.lookup_method("Float", method),
    .Some(.Bool) => typed.env.lookup_method("Bool", method),
    .Some(.Byte) => typed.env.lookup_method("Byte", method),
    _ => {},
  }
  // fallback: try base as type name
  case base.kind {
    .Ident(type_name) => typed.env.lookup_method(type_name, method),
    _ => .None,
  }
}
```

This is missing:

| Type | MonoType variant | Status |
|---|---|---|
| Option | `.Optional(_)` | Missing |
| Result | `.Result(_, _)` | Missing |
| Range | `.Range` (if exists) | Missing |
| Iterator | `.Iterator(_)` (if exists) | Missing |
| Cell | `.Cell(_)` (if exists) | Missing |

When hovering on `opt.unwrap_or(default)` or `result.map(f)`, no signature
is displayed because the dispatch silently falls through.

The same dispatch pattern appears in `core_linker.tw`'s
`resolve_contract_target_name` (which does cover Optional and Result).

### Proposed fix

Add the missing cases. This is a small, surgical change:

```tw
.Some(.Optional(_)) => typed.env.lookup_method("Option", method),
.Some(.Result(_, _)) => typed.env.lookup_method("Result", method),
```

Additionally, consider whether a more general approach could replace the
hardcoded dispatch entirely. The checker already has a `method_calls` map
that records resolved method information for each call site. The
`hover_recorded_method` function already uses this. If
`hover_recorded_method` is reliably populated, `method_function_name` may
only need to serve as a fallback — and the fallback could use a single
`mono_type_to_method_namespace` helper shared with `core_linker.tw`.

### Steps

1. Add missing `Optional` and `Result` cases to `method_function_name`.
2. Check if `Range`, `Iterator`, `Cell` have MonoType variants and add
   those too.
3. Verify hover works on `opt.unwrap_or(...)`, `result.map(...)`, etc.
4. Consider extracting a shared `mono_type_to_method_namespace(ty) String?`
   helper used by both `hover.tw` and `core_linker/contract_resolve.tw`.

---

## Non-Goals

- Full rewrite of any module.
- Changing the cache invalidation strategy.
- Adding new LSP capabilities (completion, semantic tokens).
- Performance optimization of the analysis pipeline itself.
- Changing the `analyze.tw` / `stage_runner.tw` shared architecture.
