use std::collections::HashSet;
use std::path::Path;

#[cfg(test)]
use std::path::PathBuf;

use anyhow::{Result, anyhow};

use crate::ir::lower::LowerInput;
use crate::query::api::{
    ParsedModule, lower_stage, parse_source_module, resolve_stage_internal, typecheck_stage,
};
use crate::query::cache::with_global_cache;
use crate::query::keys as query_keys;
use crate::syntax::ast::SourceFile;
use crate::syntax::span::FileRegistry;
use crate::types::env::{TypeEnv, ValueEnv};
use crate::types::type_map::TypeMap;

use super::artifacts::{LoweredModule, ResolvedModule, TypedModule};

#[derive(Debug, Clone)]
pub(super) struct StageResult<T> {
    pub value: T,
    pub cache_hit: bool,
}

pub(super) struct ModuleStageRunner<'a> {
    canonical: &'a Path,
    source_hash: u64,
    deps_hash: u64,
    context_hash: u64,
    is_internal: bool,
}

impl<'a> ModuleStageRunner<'a> {
    pub(super) fn new(
        canonical: &'a Path,
        source_hash: u64,
        deps_hash: u64,
        context_hash: u64,
        is_internal: bool,
    ) -> Self {
        Self {
            canonical,
            source_hash,
            deps_hash,
            context_hash,
            is_internal,
        }
    }

    pub(super) fn parse(&self, source: &str) -> Result<StageResult<ParsedModule>> {
        let parse_key = query_keys::parse_key(self.canonical, self.source_hash);
        let (cached_parsed, had_parse_entry) = with_global_cache(|cache| {
            let had = cache.has_parse_entry(self.canonical);
            let parsed = cache.get_parsed(self.canonical, parse_key);
            (parsed, had)
        });

        if let Some(parsed) = cached_parsed {
            return Ok(StageResult {
                value: parsed,
                cache_hit: true,
            });
        }

        if had_parse_entry {
            with_global_cache(|cache| cache.invalidate_changed_module(self.canonical));
        }

        let parsed = parse_source_module(source, self.canonical)?;
        with_global_cache(|cache| cache.put_parsed(self.canonical, parse_key, parsed.clone()));

        Ok(StageResult {
            value: parsed,
            cache_hit: false,
        })
    }

    pub(super) fn resolve(
        &self,
        ast: &SourceFile,
        type_env: TypeEnv,
        value_env: ValueEnv,
        file_registry: &FileRegistry,
    ) -> Result<StageResult<ResolvedModule>> {
        let resolve_key = query_keys::with_context(
            query_keys::resolve_key(self.canonical, self.source_hash, self.deps_hash),
            self.context_hash,
        );
        if let Some(cached) =
            with_global_cache(|cache| cache.get_resolved(self.canonical, resolve_key))
        {
            return Ok(StageResult {
                value: cached,
                cache_hit: true,
            });
        }

        let type_env_for_errs = type_env.clone();
        let resolved = match resolve_stage_internal(ast, type_env, value_env, self.is_internal) {
            Ok(resolved) => resolved,
            Err(errors) => {
                let msgs: Vec<String> = errors
                    .iter()
                    .map(|e| e.format(file_registry, Some(&type_env_for_errs)))
                    .collect();
                return Err(anyhow!("{}", msgs.join("\n")));
            }
        };
        with_global_cache(|cache| {
            cache.put_resolved(self.canonical, resolve_key, resolved.clone())
        });
        Ok(StageResult {
            value: resolved,
            cache_hit: false,
        })
    }

    pub(super) fn typecheck(
        &self,
        ast: &SourceFile,
        resolved: ResolvedModule,
        module_aliases: HashSet<String>,
        file_registry: &FileRegistry,
    ) -> Result<StageResult<TypedModule>> {
        let typecheck_key = query_keys::with_context(
            query_keys::typecheck_key(self.canonical, self.source_hash, self.deps_hash),
            self.context_hash,
        );
        if let Some(cached) =
            with_global_cache(|cache| cache.get_typed(self.canonical, typecheck_key))
        {
            return Ok(StageResult {
                value: cached,
                cache_hit: true,
            });
        }

        let type_env_for_errs = resolved.type_env.clone();
        let typed =
            match typecheck_stage(ast, resolved, module_aliases) {
                Ok(typed) => typed,
                Err(errors) => {
                    let msgs: Vec<String> = errors
                        .iter()
                        .map(|e| e.format(file_registry, Some(&type_env_for_errs)))
                        .collect();
                    return Err(anyhow!("{}", msgs.join("\n")));
                }
            };
        with_global_cache(|cache| cache.put_typed(self.canonical, typecheck_key, typed.clone()));
        Ok(StageResult {
            value: typed,
            cache_hit: false,
        })
    }

    pub(super) fn lower(
        &self,
        ast: &SourceFile,
        type_map: TypeMap,
        input: LowerInput,
        alias: &str,
        file_registry: &FileRegistry,
    ) -> Result<StageResult<LoweredModule>> {
        let lower_key = query_keys::with_context(
            query_keys::lower_key(
                self.canonical,
                self.source_hash,
                self.deps_hash,
                input.next_global_local_id,
            ),
            self.context_hash,
        );

        if let Some(cached) =
            with_global_cache(|cache| cache.get_lowered(self.canonical, lower_key))
        {
            return Ok(StageResult {
                value: cached,
                cache_hit: true,
            });
        }

        let lowered = match lower_stage(ast, type_map, input, alias) {
            Ok(lowered) => lowered,
            Err(errs) => {
                let msgs: Vec<String> = errs.iter().map(|e| e.format(file_registry)).collect();
                return Err(anyhow!("Lowering failed:\n{}", msgs.join("\n")));
            }
        };
        with_global_cache(|cache| cache.put_lowered(self.canonical, lower_key, lowered.clone()));
        Ok(StageResult {
            value: lowered,
            cache_hit: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use crate::module::context::default_module_aliases;
    use crate::query::cache::reset_global_cache;
    use crate::query::keys;

    use super::*;

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn runner_for<'a>(path: &'a Path, source: &str, is_internal: bool) -> ModuleStageRunner<'a> {
        ModuleStageRunner::new(path, keys::hash_text(source), 0, 0, is_internal)
    }

    #[test]
    fn parse_reports_cache_hit_on_second_run() {
        let _guard = test_lock().lock().expect("test lock poisoned");
        reset_global_cache();

        let path = PathBuf::from("/virtual/stage_runner_parse/main.tw");
        let source = "println(\"ok\")\n";
        let runner = runner_for(&path, source, false);

        let first = runner.parse(source).expect("first parse should succeed");
        let second = runner.parse(source).expect("second parse should succeed");

        assert!(!first.cache_hit);
        assert!(second.cache_hit);
    }

    #[test]
    fn resolve_reports_cache_hit_on_second_run() {
        let _guard = test_lock().lock().expect("test lock poisoned");
        reset_global_cache();

        let path = PathBuf::from("/virtual/stage_runner_resolve/main.tw");
        let source = "println(\"ok\")\n";
        let runner = runner_for(&path, source, false);

        let parsed = runner.parse(source).expect("parse should succeed").value;

        let first = runner
            .resolve(
                &parsed.ast,
                TypeEnv::new(),
                ValueEnv::new(),
                &parsed.file_registry,
            )
            .expect("first resolve should succeed");
        let second = runner
            .resolve(
                &parsed.ast,
                TypeEnv::new(),
                ValueEnv::new(),
                &parsed.file_registry,
            )
            .expect("second resolve should succeed");

        assert!(!first.cache_hit);
        assert!(second.cache_hit);
    }
}
