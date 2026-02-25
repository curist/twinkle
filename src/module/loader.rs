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
