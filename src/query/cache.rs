use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::module::artifacts::{LoweredModule, ResolvedModule, TypedModule};

use super::api::ParsedModule;
use super::graph::DependencyGraph;

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
        if let Some(entry) = self.parse.get(module) {
            if entry.key == key {
                self.stats.parse_hits += 1;
                return Some(entry.value.clone());
            }
        }
        self.stats.parse_misses += 1;
        None
    }

    pub fn put_parsed(&mut self, module: &Path, key: u64, value: ParsedModule) {
        self.parse
            .insert(module.to_path_buf(), CacheEntry { key, value });
    }

    pub fn get_resolved(&mut self, module: &Path, key: u64) -> Option<ResolvedModule> {
        if let Some(entry) = self.resolve.get(module) {
            if entry.key == key {
                self.stats.resolve_hits += 1;
                return Some(entry.value.clone());
            }
        }
        self.stats.resolve_misses += 1;
        None
    }

    pub fn put_resolved(&mut self, module: &Path, key: u64, value: ResolvedModule) {
        self.resolve
            .insert(module.to_path_buf(), CacheEntry { key, value });
    }

    pub fn get_typed(&mut self, module: &Path, key: u64) -> Option<TypedModule> {
        if let Some(entry) = self.typecheck.get(module) {
            if entry.key == key {
                self.stats.typecheck_hits += 1;
                return Some(entry.value.clone());
            }
        }
        self.stats.typecheck_misses += 1;
        None
    }

    pub fn put_typed(&mut self, module: &Path, key: u64, value: TypedModule) {
        self.typecheck
            .insert(module.to_path_buf(), CacheEntry { key, value });
    }

    pub fn get_lowered(&mut self, module: &Path, key: u64) -> Option<LoweredModule> {
        if let Some(entry) = self.lower.get(module) {
            if entry.key == key {
                self.stats.lower_hits += 1;
                return Some(entry.value.clone());
            }
        }
        self.stats.lower_misses += 1;
        None
    }

    pub fn put_lowered(&mut self, module: &Path, key: u64, value: LoweredModule) {
        self.lower
            .insert(module.to_path_buf(), CacheEntry { key, value });
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
    with_global_cache(|cache| cache.clear());
}

pub fn global_cache_stats() -> CacheStats {
    with_global_cache(|cache| cache.stats())
}
