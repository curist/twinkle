use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::module::artifacts::{LoweredModule, ResolvedModule, TypedModule};

use super::api::ParsedModule;
use super::graph::DependencyGraph;

/// Whether stage results are stored into the global query cache.
///
/// The cache only pays off when the same module is compiled more than once in
/// a process (incremental/LSP scenarios). A one-shot CLI `build` deduplicates
/// modules itself, so every `get_*` misses and every `put_*` would just deep
/// clone a stage artifact that is never read again. CLI batch builds disable
/// puts via [`set_cache_puts_enabled`] to avoid that overhead.
static CACHE_PUTS_ENABLED: AtomicBool = AtomicBool::new(true);

/// Enable or disable storing stage results into the global query cache.
pub fn set_cache_puts_enabled(enabled: bool) {
    CACHE_PUTS_ENABLED.store(enabled, Ordering::Relaxed);
}

fn cache_puts_enabled() -> bool {
    CACHE_PUTS_ENABLED.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub parse_hits: u64,
    pub parse_misses: u64,
    pub resolve_hits: u64,
    pub resolve_misses: u64,
    pub typecheck_hits: u64,
    pub typecheck_misses: u64,
    pub lower_hits: u64,
    pub lower_misses: u64,
}

#[derive(Debug, Clone)]
struct CacheEntry<T> {
    key: u64,
    value: T,
}

#[derive(Debug, Default)]
pub struct QueryStageCache {
    parse: HashMap<PathBuf, CacheEntry<ParsedModule>>,
    resolve: HashMap<PathBuf, CacheEntry<ResolvedModule>>,
    typecheck: HashMap<PathBuf, CacheEntry<TypedModule>>,
    lower: HashMap<PathBuf, CacheEntry<LoweredModule>>,
    module_hashes: HashMap<PathBuf, u64>,
    dep_graph: DependencyGraph,
    stats: CacheStats,
}

impl QueryStageCache {
    pub fn clear(&mut self) {
        self.parse.clear();
        self.resolve.clear();
        self.typecheck.clear();
        self.lower.clear();
        self.module_hashes.clear();
        self.dep_graph = DependencyGraph::default();
        self.stats = CacheStats::default();
    }

    pub fn stats(&self) -> CacheStats {
        self.stats.clone()
    }

    pub fn module_hash(&self, module: &Path) -> Option<u64> {
        self.module_hashes.get(module).copied()
    }

    pub fn has_parse_entry(&self, module: &Path) -> bool {
        self.parse.contains_key(module)
    }

    pub fn set_module_hash(&mut self, module: &Path, hash: u64) {
        self.module_hashes.insert(module.to_path_buf(), hash);
    }

    pub fn set_dependencies(&mut self, module: &Path, deps: &[PathBuf]) {
        self.dep_graph.set_dependencies(module, deps);
    }

    pub fn invalidate_changed_module(&mut self, module: &Path) {
        let affected = self.dep_graph.reverse_dependents_closure(module);
        for m in affected {
            self.resolve.remove(&m);
            self.typecheck.remove(&m);
            self.lower.remove(&m);
            self.module_hashes.remove(&m);
        }
    }

    pub fn get_parsed(&mut self, module: &Path, key: u64) -> Option<ParsedModule> {
        if let Some(entry) = self.parse.get(module)
            && entry.key == key
        {
            self.stats.parse_hits += 1;
            return Some(entry.value.clone());
        }
        self.stats.parse_misses += 1;
        None
    }

    pub fn put_parsed(&mut self, module: &Path, key: u64, value: &ParsedModule) {
        if !cache_puts_enabled() {
            return;
        }
        self.parse.insert(
            module.to_path_buf(),
            CacheEntry { key, value: value.clone() },
        );
    }

    pub fn get_resolved(&mut self, module: &Path, key: u64) -> Option<ResolvedModule> {
        if let Some(entry) = self.resolve.get(module)
            && entry.key == key
        {
            self.stats.resolve_hits += 1;
            return Some(entry.value.clone());
        }
        self.stats.resolve_misses += 1;
        None
    }

    pub fn put_resolved(&mut self, module: &Path, key: u64, value: &ResolvedModule) {
        if !cache_puts_enabled() {
            return;
        }
        self.resolve.insert(
            module.to_path_buf(),
            CacheEntry { key, value: value.clone() },
        );
    }

    pub fn get_typed(&mut self, module: &Path, key: u64) -> Option<TypedModule> {
        if let Some(entry) = self.typecheck.get(module)
            && entry.key == key
        {
            self.stats.typecheck_hits += 1;
            return Some(entry.value.clone());
        }
        self.stats.typecheck_misses += 1;
        None
    }

    pub fn put_typed(&mut self, module: &Path, key: u64, value: &TypedModule) {
        if !cache_puts_enabled() {
            return;
        }
        self.typecheck.insert(
            module.to_path_buf(),
            CacheEntry { key, value: value.clone() },
        );
    }

    pub fn get_lowered(&mut self, module: &Path, key: u64) -> Option<LoweredModule> {
        if let Some(entry) = self.lower.get(module)
            && entry.key == key
        {
            self.stats.lower_hits += 1;
            return Some(entry.value.clone());
        }
        self.stats.lower_misses += 1;
        None
    }

    pub fn put_lowered(&mut self, module: &Path, key: u64, value: &LoweredModule) {
        if !cache_puts_enabled() {
            return;
        }
        self.lower.insert(
            module.to_path_buf(),
            CacheEntry { key, value: value.clone() },
        );
    }
}

static GLOBAL_QUERY_CACHE: OnceLock<Mutex<QueryStageCache>> = OnceLock::new();

fn global_cache() -> &'static Mutex<QueryStageCache> {
    GLOBAL_QUERY_CACHE.get_or_init(|| Mutex::new(QueryStageCache::default()))
}

pub fn with_global_cache<R>(f: impl FnOnce(&mut QueryStageCache) -> R) -> R {
    let mut guard = global_cache()
        .lock()
        .expect("global query cache mutex poisoned");
    f(&mut guard)
}

pub fn reset_global_cache() {
    // Restore the default (puts enabled) alongside clearing entries so that
    // resetting the cache always returns it to a usable state, regardless of
    // any earlier `set_cache_puts_enabled(false)` (e.g. from a CLI batch build).
    set_cache_puts_enabled(true);
    with_global_cache(|cache| cache.clear());
}

pub fn global_cache_stats() -> CacheStats {
    with_global_cache(|cache| cache.stats())
}
