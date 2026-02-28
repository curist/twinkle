use std::collections::HashMap;
use std::path::PathBuf;

fn modules_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/modules")
}

fn compile_func_ids(relative: &str) -> HashMap<String, u32> {
    let path = modules_dir().join(relative);
    let (core, _) = twinkle::module::compile_entry(path.to_str().unwrap())
        .expect("compile should succeed");

    core.functions
        .into_iter()
        .map(|f| (f.name, f.func_id.0))
        .collect()
}

#[test]
fn func_ids_are_stable_under_import_order_change() {
    let ids_ab = compile_func_ids("funcid_stability/main_ab.tw");
    let ids_ba = compile_func_ids("funcid_stability/main_ba.tw");

    assert_eq!(ids_ab.get("alpha"), ids_ba.get("alpha"));
    assert_eq!(ids_ab.get("beta"), ids_ba.get("beta"));
}

#[test]
fn func_ids_are_stable_under_unrelated_entry_edit() {
    let ids_base = compile_func_ids("funcid_stability/main_ab.tw");
    let ids_extra = compile_func_ids("funcid_stability/main_ab_extra.tw");

    assert_eq!(ids_base.get("alpha"), ids_extra.get("alpha"));
    assert_eq!(ids_base.get("beta"), ids_extra.get("beta"));
}
