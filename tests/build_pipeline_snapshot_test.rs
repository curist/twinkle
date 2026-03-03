use std::fs;
use std::path::Path;

fn snapshot_path(name: &str) -> String {
    format!("tests/snapshots/build/{name}.wat")
}

fn check_build_snapshot(tw_path: &str, snapshot_name: &str) {
    let actual = twinkle::cli::build::build_wat(tw_path)
        .unwrap_or_else(|e| panic!("build_wat failed for '{tw_path}': {e}"));

    let snap_path = snapshot_path(snapshot_name);
    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        fs::create_dir_all("tests/snapshots/build").expect("create snapshot directory");
        fs::write(&snap_path, &actual).expect("write snapshot");
        return;
    }

    assert!(
        Path::new(&snap_path).exists(),
        "Snapshot file missing: {snap_path}. Run with UPDATE_SNAPSHOTS=1 to create it.",
    );

    let expected = fs::read_to_string(&snap_path)
        .unwrap_or_else(|_| panic!("could not read snapshot: {}", snap_path));
    assert_eq!(
        actual, expected,
        "Build snapshot mismatch for '{}'",
        tw_path
    );
}

#[test]
fn build_snapshot_hello() {
    check_build_snapshot("tests/run/hello.tw", "hello");
}

#[test]
fn build_snapshot_arithmetic() {
    check_build_snapshot("tests/run/arithmetic.tw", "arithmetic");
}

#[test]
fn build_snapshot_records() {
    check_build_snapshot("tests/run/records.tw", "records");
}
