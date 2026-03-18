use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::module::{WorkspaceAnalysis, analyze_entry_from_source_map};

use super::completion::{CompletionItem, completions_at_module};
use super::definition::{DefinitionTarget, definition_at_workspace};
use super::diagnostics::{LspDiagnostic, query_diagnostic_to_lsp};
use super::hover_at_module;
use super::position::PositionUtf16;

#[derive(Debug, Clone)]
pub struct AnalysisSession {
    project_root: PathBuf,
    stdlib_root: PathBuf,
    base_sources: HashMap<PathBuf, String>,
    overlays: HashMap<PathBuf, String>,
}

impl AnalysisSession {
    pub fn new(
        project_root: impl AsRef<Path>,
        stdlib_root: impl AsRef<Path>,
        base_sources: HashMap<PathBuf, String>,
    ) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            stdlib_root: stdlib_root.as_ref().to_path_buf(),
            base_sources,
            overlays: HashMap::new(),
        }
    }

    pub fn did_open(&mut self, path: impl AsRef<Path>, text: String) {
        self.overlays
            .insert(self.normalize_path(path.as_ref()), text);
    }

    pub fn did_change(&mut self, path: impl AsRef<Path>, text: String) {
        self.overlays
            .insert(self.normalize_path(path.as_ref()), text);
    }

    pub fn did_close(&mut self, path: impl AsRef<Path>) {
        self.overlays.remove(&self.normalize_path(path.as_ref()));
    }

    pub fn analyze_entry(&self, entry_path: impl AsRef<Path>) -> Result<WorkspaceAnalysis> {
        let mut sources = self.base_sources.clone();
        for (path, text) in &self.overlays {
            sources.insert(path.clone(), text.clone());
        }
        analyze_entry_from_source_map(
            &self.normalize_path(entry_path.as_ref()),
            &sources,
            &self.project_root,
            &self.stdlib_root,
        )
    }

    pub fn hover(
        &self,
        entry_path: impl AsRef<Path>,
        module_path: impl AsRef<Path>,
        position: PositionUtf16,
    ) -> Result<Option<String>> {
        let analysis = self.analyze_entry(entry_path)?;
        let module_key = self.normalize_path(module_path.as_ref());
        let Some(module) = analysis.modules.get(&module_key) else {
            return Ok(None);
        };
        Ok(hover_at_module(module, position))
    }

    /// Return LSP diagnostics for a module within the workspace rooted at `entry_path`.
    pub fn diagnostics(
        &self,
        entry_path: impl AsRef<Path>,
        module_path: impl AsRef<Path>,
    ) -> Result<Vec<LspDiagnostic>> {
        let analysis = self.analyze_entry(entry_path)?;
        let module_key = self.normalize_path(module_path.as_ref());

        let query_diags = analysis
            .diagnostics
            .get(&module_key)
            .cloned()
            .unwrap_or_default();

        // Find the file_registry for span→UTF-16 conversion.
        // Try the analyzed module first, then the fallback registries for
        // modules that failed resolve/typecheck but succeeded parsing.
        let empty_registry = crate::syntax::span::FileRegistry::new();
        let registry = analysis
            .modules
            .get(&module_key)
            .map(|m| &m.file_registry)
            .or_else(|| analysis.file_registries.get(&module_key))
            .unwrap_or(&empty_registry);

        Ok(query_diags
            .iter()
            .filter_map(|d| query_diagnostic_to_lsp(d, registry))
            .collect())
    }

    /// Return LSP diagnostics for all modules in the workspace.
    pub fn all_diagnostics(
        &self,
        entry_path: impl AsRef<Path>,
    ) -> Result<Vec<(PathBuf, Vec<LspDiagnostic>)>> {
        let analysis = self.analyze_entry(entry_path)?;
        let empty_registry = crate::syntax::span::FileRegistry::new();

        let mut result = Vec::new();
        for (path, query_diags) in &analysis.diagnostics {
            let registry = analysis
                .modules
                .get(path)
                .map(|m| &m.file_registry)
                .or_else(|| analysis.file_registries.get(path))
                .unwrap_or(&empty_registry);
            let lsp_diags: Vec<LspDiagnostic> = query_diags
                .iter()
                .filter_map(|d| query_diagnostic_to_lsp(d, registry))
                .collect();
            if !lsp_diags.is_empty() {
                result.push((path.clone(), lsp_diags));
            }
        }
        Ok(result)
    }

    pub fn completion(
        &self,
        entry_path: impl AsRef<Path>,
        module_path: impl AsRef<Path>,
        position: PositionUtf16,
    ) -> Result<Vec<CompletionItem>> {
        let analysis = self.analyze_entry(entry_path)?;
        let module_key = self.normalize_path(module_path.as_ref());
        Ok(completions_at_module(&analysis, &module_key, position))
    }

    pub fn definition(
        &self,
        entry_path: impl AsRef<Path>,
        module_path: impl AsRef<Path>,
        position: PositionUtf16,
    ) -> Result<Option<DefinitionTarget>> {
        let analysis = self.analyze_entry(entry_path)?;
        Ok(definition_at_workspace(
            &analysis,
            &self.normalize_path(module_path.as_ref()),
            position,
        ))
    }

    fn normalize_path(&self, path: &Path) -> PathBuf {
        let normalized = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.project_root.join(path)
        };
        normalized.canonicalize().unwrap_or(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_uses_overlay_updates_without_touching_base_sources() {
        let project_root = PathBuf::from("/virtual/lsp_session");
        let stdlib_root = project_root.join("stdlib");
        let entry = project_root.join("main.tw");

        let mut base_sources = HashMap::new();
        base_sources.insert(entry.clone(), "value := 42\n".to_string());

        let mut session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);
        let before = session
            .hover(&entry, &entry, PositionUtf16::new(0, 9))
            .expect("hover before edit");
        assert_eq!(before.as_deref(), Some("Int"));

        session.did_change(&entry, "value := \"hi\"\n".to_string());
        let after = session
            .hover(&entry, &entry, PositionUtf16::new(0, 9))
            .expect("hover after edit");
        assert_eq!(after.as_deref(), Some("String"));

        session.did_close(&entry);
        let reverted = session
            .hover(&entry, &entry, PositionUtf16::new(0, 9))
            .expect("hover after close");
        assert_eq!(reverted.as_deref(), Some("Int"));
    }

    #[test]
    fn hover_works_with_destructuring_imports() {
        let project_root = PathBuf::from("/virtual/destructure_test");
        let stdlib_root = project_root.join("stdlib");
        let main_path = project_root.join("main.tw");
        let math_path = project_root.join("math.tw");

        let mut base_sources = HashMap::new();
        base_sources.insert(
            math_path.clone(),
            r#"pub type Vec2 = .{ x: Int, y: Int }
pub fn add(a: Int, b: Int) Int { a + b }
pub fn make_vec(x: Int, y: Int) Vec2 { Vec2.{ x: x, y: y } }
"#
            .to_string(),
        );
        base_sources.insert(
            main_path.clone(),
            r#"use math.{add, make_vec, Vec2}

fn main() {
    result := add(1, 2)
    v: Vec2 = make_vec(10, 20)
    println("${result}")
    println("${v.x}")
    q := math.add(3, 4)
    println("${q}")
}
"#
            .to_string(),
        );

        let session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);

        // Check diagnostics first
        let diags = session
            .diagnostics(&main_path, &main_path)
            .expect("diagnostics should not error");
        for d in &diags {
            eprintln!("  diag: {:?}", d);
        }
        assert!(diags.is_empty(), "Expected no diagnostics, got: {diags:?}");

        // Hover over `add` in `result := add(1, 2)` — line 3, col 14
        let hover_add = session
            .hover(&main_path, &main_path, PositionUtf16::new(3, 14))
            .expect("hover should not error");
        eprintln!("hover_add = {:?}", hover_add);
        assert!(
            hover_add.is_some(),
            "Expected hover on `add` to return a type"
        );

        // Hover over `Vec2` in the type annotation `v: Vec2 = ...` — line 4
        let hover_vec2 = session
            .hover(&main_path, &main_path, PositionUtf16::new(4, 7))
            .expect("hover should not error");
        eprintln!("hover_vec2 = {:?}", hover_vec2);

        // Hover on import line items
        // `use math.{add, make_vec, Vec2}` — line 0
        let hover_import_add = session
            .hover(&main_path, &main_path, PositionUtf16::new(0, 11))
            .expect("hover should not error");
        eprintln!("hover_import_add (line 0, col 11) = {:?}", hover_import_add);
        assert!(
            hover_import_add.is_some(),
            "hover on `add` in import list should return type info"
        );
        assert!(
            hover_import_add.as_deref().unwrap().contains("fn("),
            "hover on `add` should show function signature"
        );

        let hover_import_vec2 = session
            .hover(&main_path, &main_path, PositionUtf16::new(0, 26))
            .expect("hover should not error");
        eprintln!(
            "hover_import_vec2 (line 0, col 26) = {:?}",
            hover_import_vec2
        );
        assert!(
            hover_import_vec2.is_some(),
            "hover on `Vec2` in import list should return type info"
        );

        // Goto definition on `add` call — line 3, col 14
        let def_add = session
            .definition(&main_path, &main_path, PositionUtf16::new(3, 14))
            .expect("definition should not error");
        eprintln!("def_add = {:?}", def_add);

        // Goto definition on `make_vec` call — line 4, col 16
        let def_make_vec = session
            .definition(&main_path, &main_path, PositionUtf16::new(4, 16))
            .expect("definition should not error");
        eprintln!("def_make_vec = {:?}", def_make_vec);

        // Goto definition on import line — line 0, col 4 (on `math`)
        let def_import = session
            .definition(&main_path, &main_path, PositionUtf16::new(0, 4))
            .expect("definition should not error");
        eprintln!("def_import (math) = {:?}", def_import);

        assert!(def_add.is_some(), "goto-def on `add` call should resolve");
        assert_eq!(def_add.as_ref().unwrap().path, math_path);
        assert!(
            def_make_vec.is_some(),
            "goto-def on `make_vec` call should resolve"
        );
        assert_eq!(def_make_vec.as_ref().unwrap().path, math_path);
        assert!(
            def_import.is_some(),
            "goto-def on `math` module should resolve"
        );

        // Goto definition from import line items
        // `use math.{add, make_vec, Vec2}` — `add` starts at col 10
        let def_import_add = session
            .definition(&main_path, &main_path, PositionUtf16::new(0, 11))
            .expect("definition should not error");
        eprintln!("def_import_add = {:?}", def_import_add);
        assert!(
            def_import_add.is_some(),
            "goto-def on `add` in import line should resolve"
        );
        assert_eq!(def_import_add.as_ref().unwrap().path, math_path);

        // Goto definition on `Vec2` type annotation — line 4, col 7
        let def_vec2_type = session
            .definition(&main_path, &main_path, PositionUtf16::new(4, 7))
            .expect("definition should not error");
        eprintln!("def_vec2_type = {:?}", def_vec2_type);
        assert!(
            def_vec2_type.is_some(),
            "goto-def on `Vec2` type annotation should resolve"
        );
        assert_eq!(def_vec2_type.as_ref().unwrap().path, math_path);

        // Hover and goto-def on `math` in `math.add(3, 4)` — line 7, col 9
        let hover_math_alias = session
            .hover(&main_path, &main_path, PositionUtf16::new(7, 9))
            .expect("hover should not error");
        eprintln!("hover_math_alias = {:?}", hover_math_alias);
        assert!(
            hover_math_alias.is_some(),
            "hover on `math` module alias should return info"
        );
        assert!(
            hover_math_alias.as_deref().unwrap().contains("math"),
            "hover on module alias should mention module name"
        );

        let def_math_alias = session
            .definition(&main_path, &main_path, PositionUtf16::new(7, 9))
            .expect("definition should not error");
        eprintln!("def_math_alias = {:?}", def_math_alias);
        assert!(
            def_math_alias.is_some(),
            "goto-def on `math` module alias should resolve"
        );
        assert_eq!(def_math_alias.as_ref().unwrap().path, math_path);
    }
}
