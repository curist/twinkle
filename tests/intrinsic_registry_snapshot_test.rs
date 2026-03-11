use std::fs;
use std::path::Path;

fn snapshot_path(name: &str) -> String {
    format!("tests/snapshots/build/intrinsics/{name}.wat")
}

fn check_build_snapshot(tw_path: &str, snapshot_name: &str) {
    let actual = twinkle::cli::build::build_wat(tw_path)
        .unwrap_or_else(|e| panic!("build_wat failed for '{tw_path}': {e}"));

    let snap_path = snapshot_path(snapshot_name);
    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        fs::create_dir_all("tests/snapshots/build/intrinsics")
            .expect("create intrinsic snapshot directory");
        fs::write(&snap_path, &actual).expect("write intrinsic snapshot");
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
fn build_snapshot_intrinsic_iterator_unfold() {
    check_build_snapshot(
        "tests/run/iterator_unfold_callback_typing.tw",
        "iterator_unfold_callback_typing",
    );
}

#[test]
fn build_snapshot_intrinsic_cell_update() {
    check_build_snapshot("tests/run/cell_update.tw", "cell_update");
}

#[test]
fn build_snapshot_intrinsic_string_utf8() {
    check_build_snapshot("tests/run/string_utf8.tw", "string_utf8");
}
