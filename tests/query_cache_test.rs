use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use twinkle::module::{check_entry, compile_entry};
use twinkle::query::cache::{global_cache_stats, reset_global_cache};

fn test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn modules_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/modules")
}

fn tmp_case_dir(case: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    std::env::temp_dir().join(format!("twinkle_query_cache_{}_{}", case, nanos))
}

#[test]
fn query_cache_hits_parse_resolve_typecheck_on_second_check() {
    let _guard = test_lock().lock().expect("test lock poisoned");
    reset_global_cache();

    let main = modules_dir().join("simple/main.tw");
    check_entry(main.to_str().unwrap()).expect("first check should succeed");
    let first = global_cache_stats();

    check_entry(main.to_str().unwrap()).expect("second check should succeed");
    let second = global_cache_stats();

    assert!(first.parse_misses > 0);
    assert!(first.resolve_misses > 0);
    assert!(first.typecheck_misses > 0);

    assert!(second.parse_hits > first.parse_hits);
    assert!(second.resolve_hits > first.resolve_hits);
    assert!(second.typecheck_hits > first.typecheck_hits);
}

#[test]
fn query_cache_hits_lower_on_second_compile() {
    let _guard = test_lock().lock().expect("test lock poisoned");
    reset_global_cache();

    let main = modules_dir().join("simple/main.tw");
    compile_entry(main.to_str().unwrap()).expect("first compile should succeed");
    let first = global_cache_stats();

    compile_entry(main.to_str().unwrap()).expect("second compile should succeed");
    let second = global_cache_stats();

    assert!(first.lower_misses > 0);
    assert!(second.lower_hits > first.lower_hits);
}

#[test]
fn query_cache_invalidates_reverse_dependents_when_dep_changes() {
    let _guard = test_lock().lock().expect("test lock poisoned");
    reset_global_cache();

    let dir = tmp_case_dir("reverse_dep");
    fs::create_dir_all(&dir).expect("create temp test dir");
    let main = dir.join("main.tw");
    let dep = dir.join("dep.tw");

    fs::write(
        &main,
        "use dep\n\nfn run() Int {\n  dep.value()\n}\n\nrun()\n",
    )
    .expect("write main.tw");
    fs::write(&dep, "pub fn value() Int {\n  1\n}\n").expect("write dep.tw");

    check_entry(main.to_str().unwrap()).expect("first check should succeed");
    check_entry(main.to_str().unwrap()).expect("second check should succeed");
    let before_change = global_cache_stats();

    fs::write(&dep, "pub fn value() Int {\n  2\n}\n").expect("rewrite dep.tw");
    check_entry(main.to_str().unwrap()).expect("third check should succeed");
    let after_change = global_cache_stats();

    // dep parse must miss due source hash change; resolve/typecheck should rerun
    // for dep and for main (reverse-dependent invalidation).
    assert!(after_change.parse_misses > before_change.parse_misses);
    assert!(after_change.resolve_misses >= before_change.resolve_misses + 2);
    assert!(after_change.typecheck_misses >= before_change.typecheck_misses + 2);

    let _ = fs::remove_dir_all(&dir);
}
