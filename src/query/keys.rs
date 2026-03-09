use std::path::Path;

use crate::ir::lower::prelude;

pub const CACHE_SCHEMA_VERSION: u64 = 1;

pub fn hash_text(text: &str) -> u64 {
    fnv1a_64(text.as_bytes())
}

pub fn parse_key(path: &Path, source_hash: u64) -> u64 {
    mix_many(&[
        CACHE_SCHEMA_VERSION,
        hash_text("parse"),
        hash_text(&path.to_string_lossy()),
        source_hash,
    ])
}

pub fn resolve_key(path: &Path, source_hash: u64, deps_hash: u64) -> u64 {
    mix_many(&[
        CACHE_SCHEMA_VERSION,
        hash_text("resolve"),
        hash_text(&path.to_string_lossy()),
        source_hash,
        deps_hash,
    ])
}

pub fn typecheck_key(
    path: &Path,
    source_hash: u64,
    deps_hash: u64,
    allow_internal_host_builtins: bool,
) -> u64 {
    mix_many(&[
        CACHE_SCHEMA_VERSION,
        hash_text("typecheck"),
        hash_text(&path.to_string_lossy()),
        source_hash,
        deps_hash,
        allow_internal_host_builtins as u64,
    ])
}

pub fn lower_key(path: &Path, source_hash: u64, deps_hash: u64, next_global_local_id: u32) -> u64 {
    mix_many(&[
        CACHE_SCHEMA_VERSION,
        hash_text("lower"),
        hash_text(&path.to_string_lossy()),
        source_hash,
        deps_hash,
        prelude::USER_FUNC_START as u64,
        next_global_local_id as u64,
    ])
}

pub fn deps_hash(entries: &[(String, u64)]) -> u64 {
    let mut sorted: Vec<(String, u64)> = entries.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut chunks = Vec::with_capacity(sorted.len() * 2 + 1);
    chunks.push(hash_text("deps"));
    for (path, dep_hash) in sorted {
        chunks.push(hash_text(&path));
        chunks.push(dep_hash);
    }
    mix_many(&chunks)
}

pub fn module_hash(source_hash: u64, deps_hash: u64) -> u64 {
    mix_many(&[
        CACHE_SCHEMA_VERSION,
        hash_text("module"),
        source_hash,
        deps_hash,
    ])
}

pub fn context_hash(entries: &[(String, u64)]) -> u64 {
    let mut sorted: Vec<(String, u64)> = entries.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut chunks = Vec::with_capacity(sorted.len() * 2 + 1);
    chunks.push(hash_text("ctx"));
    for (path, module_hash) in sorted {
        chunks.push(hash_text(&path));
        chunks.push(module_hash);
    }
    mix_many(&chunks)
}

pub fn with_context(base_key: u64, context_hash: u64) -> u64 {
    mix_many(&[
        CACHE_SCHEMA_VERSION,
        hash_text("with_ctx"),
        base_key,
        context_hash,
    ])
}

fn mix_many(words: &[u64]) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for word in words {
        let bytes = word.to_le_bytes();
        h = fnv1a_mix(h, &bytes);
    }
    h
}

fn fnv1a_64(bytes: &[u8]) -> u64 {
    fnv1a_mix(0xcbf29ce484222325u64, bytes)
}

fn fnv1a_mix(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
