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
}
