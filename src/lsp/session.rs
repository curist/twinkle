use std::collections::HashMap;
use std::fs;
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
    source_roots: Vec<PathBuf>,
    base_sources: HashMap<PathBuf, String>,
    overlays: HashMap<PathBuf, String>,
}

impl AnalysisSession {
    pub fn new(
        project_root: impl AsRef<Path>,
        stdlib_root: impl AsRef<Path>,
        base_sources: HashMap<PathBuf, String>,
    ) -> Self {
        let project_root = canonicalize_or_self(project_root.as_ref());
        let stdlib_root = canonicalize_or_self(stdlib_root.as_ref());
        let source_roots = dedup_roots(vec![project_root.clone(), stdlib_root.clone()]);
        Self::new_with_source_roots(project_root, stdlib_root, source_roots, base_sources)
    }

    pub fn new_with_source_roots(
        project_root: impl AsRef<Path>,
        stdlib_root: impl AsRef<Path>,
        source_roots: Vec<PathBuf>,
        base_sources: HashMap<PathBuf, String>,
    ) -> Self {
        let project_root = canonicalize_or_self(project_root.as_ref());
        let stdlib_root = canonicalize_or_self(stdlib_root.as_ref());
        let base_sources = normalize_sources(&project_root, base_sources);
        Self {
            project_root,
            stdlib_root,
            source_roots: dedup_roots(source_roots),
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

    pub fn sync_disk_file(&mut self, path: impl AsRef<Path>) -> Result<bool> {
        let normalized = self.normalize_path(path.as_ref());
        if !self.should_track_path(&normalized) {
            return Ok(false);
        }

        match fs::read_to_string(&normalized) {
            Ok(text) => {
                self.base_sources.insert(normalized, text);
                Ok(true)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                self.base_sources.remove(&normalized);
                Ok(true)
            }
            Err(err) => Err(err.into()),
        }
    }

    pub fn remove_disk_file(&mut self, path: impl AsRef<Path>) -> bool {
        let normalized = self.normalize_path(path.as_ref());
        if !self.should_track_path(&normalized) {
            return false;
        }
        self.base_sources.remove(&normalized).is_some()
    }

    pub fn open_document_paths(&self) -> Vec<PathBuf> {
        self.overlays.keys().cloned().collect()
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

    fn should_track_path(&self, path: &Path) -> bool {
        path.extension().is_some_and(|ext| ext == "tw")
            && self.source_roots.iter().any(|root| path.starts_with(root))
    }
}

fn canonicalize_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn dedup_roots(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in roots {
        let canonical = canonicalize_or_self(&root);
        if !out.iter().any(|existing| existing == &canonical) {
            out.push(canonical);
        }
    }
    out
}

fn normalize_sources(
    project_root: &Path,
    sources: HashMap<PathBuf, String>,
) -> HashMap<PathBuf, String> {
    sources
        .into_iter()
        .map(|(path, text)| {
            let normalized = if path.is_absolute() {
                canonicalize_or_self(&path)
            } else {
                canonicalize_or_self(&project_root.join(path))
            };
            (normalized, text)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_test_dir(case: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("twinkle_lsp_session_{case}_{nanos}"))
    }

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
    q := add(3, 4)
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

        // Hover on `add` in `q := add(3, 4)` — line 7, col 9
        let hover_add2 = session
            .hover(&main_path, &main_path, PositionUtf16::new(7, 9))
            .expect("hover should not error");
        eprintln!("hover_add2 = {:?}", hover_add2);
        assert!(
            hover_add2.is_some(),
            "hover on destructured `add` call should return type"
        );
    }

    #[test]
    fn sync_disk_file_updates_unopened_dependency_snapshot() {
        let root = temp_test_dir("sync_dependency");
        fs::create_dir_all(&root).expect("create temp dir");

        let project_root = root.join("project");
        let stdlib_root = root.join("stdlib");
        fs::create_dir_all(&project_root).expect("create project root");
        fs::create_dir_all(&stdlib_root).expect("create stdlib root");

        let main_path = project_root.join("main.tw");
        let helper_path = project_root.join("helper.tw");

        let main_source = "use helper\nvalue := helper.answer()\n";
        let helper_source = "pub fn answer() Int { 42 }\n";
        fs::write(&main_path, main_source).expect("write main");
        fs::write(&helper_path, helper_source).expect("write helper");

        let mut base_sources = HashMap::new();
        base_sources.insert(main_path.clone(), main_source.to_string());
        base_sources.insert(helper_path.clone(), helper_source.to_string());

        let mut session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);
        let before = session
            .diagnostics(&main_path, &helper_path)
            .expect("diagnostics before change");
        assert!(
            before.is_empty(),
            "expected helper diagnostics to start clean"
        );

        fs::write(&helper_path, "pub fn answer() Int { missing_name }\n").expect("rewrite helper");
        assert!(
            session.sync_disk_file(&helper_path).expect("sync helper"),
            "expected helper path to be tracked"
        );

        let after = session
            .diagnostics(&main_path, &helper_path)
            .expect("diagnostics after sync");
        assert!(
            after.iter().any(|diag| diag.code == "E_UNDEFINED_VARIABLE"),
            "expected synced helper diagnostics, got: {after:?}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sync_disk_file_preserves_open_overlay_precedence() {
        let root = temp_test_dir("overlay_precedence");
        fs::create_dir_all(&root).expect("create temp dir");

        let project_root = root.join("project");
        let stdlib_root = root.join("stdlib");
        fs::create_dir_all(&project_root).expect("create project root");
        fs::create_dir_all(&stdlib_root).expect("create stdlib root");

        let main_path = project_root.join("main.tw");
        fs::write(&main_path, "value := 42\n").expect("write main");

        let mut base_sources = HashMap::new();
        base_sources.insert(main_path.clone(), "value := 42\n".to_string());

        let mut session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);
        session.did_open(&main_path, "value := \"hi\"\n".to_string());
        fs::write(&main_path, "value := true\n").expect("update disk file");
        assert!(
            session.sync_disk_file(&main_path).expect("sync main"),
            "expected main path to be tracked"
        );

        let hover = session
            .hover(&main_path, &main_path, PositionUtf16::new(0, 9))
            .expect("hover after sync");
        assert_eq!(hover.as_deref(), Some("String"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_disk_file_drops_deleted_module_from_snapshot() {
        let root = temp_test_dir("remove_deleted");
        fs::create_dir_all(&root).expect("create temp dir");

        let project_root = root.join("project");
        let stdlib_root = root.join("stdlib");
        fs::create_dir_all(&project_root).expect("create project root");
        fs::create_dir_all(&stdlib_root).expect("create stdlib root");

        let main_path = project_root.join("main.tw");
        let helper_path = project_root.join("helper.tw");

        let main_source = "use helper\nvalue := helper.answer()\n";
        let helper_source = "pub fn answer() Int { 42 }\n";
        fs::write(&main_path, main_source).expect("write main");
        fs::write(&helper_path, helper_source).expect("write helper");

        let mut base_sources = HashMap::new();
        base_sources.insert(main_path.clone(), main_source.to_string());
        base_sources.insert(helper_path.clone(), helper_source.to_string());

        let mut session = AnalysisSession::new(&project_root, &stdlib_root, base_sources);
        assert!(
            session.remove_disk_file(&helper_path),
            "expected helper removal"
        );

        let analysis = session
            .analyze_entry(&main_path)
            .expect("analysis after removal");
        assert!(
            !analysis
                .modules
                .contains_key(&helper_path.canonicalize().expect("canonical helper")),
            "expected helper module to be absent after removal"
        );

        let _ = fs::remove_dir_all(&root);
    }
}
