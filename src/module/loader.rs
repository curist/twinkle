use std::env;
use std::path::{Path, PathBuf};

/// Walk up from `start` looking for `twinkle.toml`.
/// Falls back to `start` if not found.
/// If `TWINKLE_ROOT` env var is set, uses that instead.
pub fn find_project_root(start: &Path) -> PathBuf {
    if let Ok(root) = env::var("TWINKLE_ROOT") {
        return PathBuf::from(root);
    }

    let mut dir = start.to_path_buf();
    loop {
        if dir.join("twinkle.toml").exists() {
            return dir;
        }
        if !dir.pop() {
            break;
        }
    }

    start.to_path_buf()
}

/// Resolve module path segments to a file path.
/// e.g. root + ["math", "vector"] → root/math/vector.tw
pub fn resolve_module_path(root: &Path, module_path: &[String]) -> PathBuf {
    let mut path = root.to_path_buf();
    for segment in module_path {
        path.push(segment);
    }
    path.set_extension("tw");
    path
}

fn resolve_stdlib_root() -> PathBuf {
    if let Ok(root) = env::var("TWINKLE_STDLIB_ROOT") {
        return PathBuf::from(root);
    }
    if let Ok(root) = env::var("TWINKLE_ROOT") {
        return PathBuf::from(root).join("stdlib");
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("stdlib")
}

pub fn resolve_stdlib_module_path_from_root(stdlib_root: &Path, module_path: &[String]) -> PathBuf {
    let rel = if module_path.first().is_some_and(|s| s == "std") {
        &module_path[1..]
    } else {
        module_path
    };

    let mut path = stdlib_root.to_path_buf();
    for segment in rel {
        path.push(segment);
    }
    path.set_extension("tw");
    path
}

/// Resolve `@...` stdlib imports to `stdlib/*.tw` files.
/// e.g. `@std.path` => `<stdlib_root>/path.tw`
pub fn resolve_stdlib_module_path(module_path: &[String]) -> PathBuf {
    resolve_stdlib_module_path_from_root(&resolve_stdlib_root(), module_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_stdlib_module_path_maps_std_prefix_to_stdlib_root() {
        let p = resolve_stdlib_module_path(&["std".to_string(), "path".to_string()]);
        assert!(
            p.ends_with("stdlib/path.tw"),
            "unexpected stdlib path: {}",
            p.display()
        );
    }
}
