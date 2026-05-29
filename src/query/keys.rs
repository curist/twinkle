use std::path::Path;

use crate::ir::lower::prelude;

pub const CACHE_SCHEMA_VERSION: u64 = 2;

// -- wyhash v3 --
//
// Authoritative source: wyhash by Wang Yi
//   https://github.com/wangyi-fudan/wyhash
//   Pinned to wyhash v3 final (commit 991aa3d, 2023-08-20, wyhash.h)
//
// Constants, algorithm shape, and seed (0) are identical to the runtime Dict
// hash in src/runtime/dict.rs and mirrored in boot/lib/query/keys.tw.

const SECRET: [u64; 4] = [
    0xa0761d6478bd642f,
    0xe7037ed1a0b428db,
    0x8ebc6af09c88c6e3,
    0x589965cc75374cc3,
];

fn wymix(a: u64, b: u64) -> u64 {
    let full = (a as u128) * (b as u128);
    (full as u64) ^ ((full >> 64) as u64)
}

fn wyr3(p: &[u8], len: usize) -> u64 {
    ((p[0] as u64) << 16) | ((p[len >> 1] as u64) << 8) | (p[len - 1] as u64)
}

fn wyr4(p: &[u8]) -> u64 {
    u32::from_le_bytes([p[0], p[1], p[2], p[3]]) as u64
}

fn wyr8(p: &[u8]) -> u64 {
    u64::from_le_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]])
}

fn wyhash(key: &[u8], seed: u64) -> u64 {
    let len = key.len();
    let mut seed = seed ^ SECRET[0];
    let (a, b);

    if len <= 16 {
        if len >= 4 {
            let mid = (len >> 3) << 2;
            a = (wyr4(key) << 32) | wyr4(&key[mid..]);
            b = (wyr4(&key[len - 4..]) << 32) | wyr4(&key[len - 4 - mid..]);
        } else if len > 0 {
            a = wyr3(key, len);
            b = 0;
        } else {
            a = 0;
            b = 0;
        }
    } else {
        let mut p = 0;
        let mut i = len;
        if i > 48 {
            let mut see1 = seed;
            let mut see2 = seed;
            loop {
                seed = wymix(wyr8(&key[p..]) ^ SECRET[1], wyr8(&key[p + 8..]) ^ seed);
                see1 = wymix(
                    wyr8(&key[p + 16..]) ^ SECRET[2],
                    wyr8(&key[p + 24..]) ^ see1,
                );
                see2 = wymix(
                    wyr8(&key[p + 32..]) ^ SECRET[3],
                    wyr8(&key[p + 40..]) ^ see2,
                );
                p += 48;
                i -= 48;
                if i <= 48 {
                    break;
                }
            }
            seed ^= see1 ^ see2;
        }
        while i > 16 {
            seed = wymix(wyr8(&key[p..]) ^ SECRET[1], wyr8(&key[p + 8..]) ^ seed);
            p += 16;
            i -= 16;
        }
        a = wyr8(&key[len - 16..]);
        b = wyr8(&key[len - 8..]);
    }

    wymix(SECRET[1] ^ (len as u64), wymix(a ^ SECRET[1], b ^ seed))
}

pub fn hash_text(text: &str) -> u64 {
    wyhash(text.as_bytes(), 0)
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
    let mut bytes = Vec::with_capacity(words.len() * 8);
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    wyhash(&bytes, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_wyhash_known_vectors() {
        // These values are asserted in boot/tests/suites/query_keys_suite.tw
        // to ensure stage0/boot parity.
        assert_eq!(hash_text(""), 0x42bc986dc5eec4d3);
        assert_eq!(hash_text("a"), 0x6cf84e5a2465e867);
        assert_eq!(hash_text("hello"), 0xfaacec54df7a6205);
        assert_eq!(hash_text("parse"), 0x9afabf3faeae06da);
        assert_eq!(hash_text("fn main() {}"), 0xa94ded41a4c17c49);
    }

    #[test]
    fn test_collision_pair_no_longer_collides() {
        let h1 = hash_text("user__$f2396_mark_published_version");
        let h2 = hash_text("user__$str_333_get");
        assert_ne!(
            h1, h2,
            "FNV collision pair must not collide under wyhash v3"
        );
    }

    #[test]
    fn test_key_functions_pinned() {
        // Pinned values — also asserted in boot/tests/suites/query_keys_suite.tw
        let source_hash = hash_text("fn main() {}");
        assert_eq!(source_hash, 0xa94ded41a4c17c49);

        let pk = parse_key(Path::new("test.tw"), source_hash);
        assert_eq!(pk as i64, -8184021968052220895i64);

        let dh = deps_hash(&[("a.tw".to_string(), 100u64), ("b.tw".to_string(), 200u64)]);
        assert_eq!(dh as i64, -3408744791725452791i64);

        let mh = module_hash(source_hash, dh);
        assert_eq!(mh as i64, 5800117160120315122i64);
    }

    #[test]
    fn test_mix_many_deterministic() {
        let a = mix_many(&[1, 2, 3]);
        let b = mix_many(&[1, 2, 3]);
        assert_eq!(a, b);
        let c = mix_many(&[1, 2, 4]);
        assert_ne!(a, c);
    }
}
