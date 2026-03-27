use std::process::Command;
use std::sync::{Mutex, OnceLock};

fn cli_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn run_filtered_boot_test(filter: &str) {
    let output = Command::new(env!("CARGO_BIN_EXE_twk"))
        .env("TWK_TEST_FILTER", filter)
        .args(["run", "boot/tests/test_api.tw"])
        .output()
        .unwrap_or_else(|e| panic!("failed to run twk for filter {filter:?}: {e}"));

    assert!(
        output.status.success(),
        "boot test run failed for filter {filter:?}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
fn boot_type_identity_phase5_wasm_cli_canaries() {
    let _guard = cli_lock().lock().expect("lock boot cli phase5 test");

    run_filtered_boot_test("real boot lexer module typechecks through compiler import topology");
}
