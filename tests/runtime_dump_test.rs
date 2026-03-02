use twinkle::runtime;
use twinkle::wasm::emit::emit_wat;
use twinkle::wasm::linker::link;

fn linked_runtime_wat() -> String {
    let modules = runtime::all_modules();
    let linked = link(modules, None).expect("runtime link failed");
    emit_wat(&linked)
}

// ── 1. The full linked runtime emits valid WAT ─────────────────────────────

#[test]
fn test_runtime_dump_wat() {
    let wat = linked_runtime_wat();
    // Basic sanity: it's a well-formed module
    assert!(wat.starts_with("(module"));
    assert!(wat.ends_with(')'));
    insta::assert_snapshot!(wat);
}

// ── 2. All shared types from rt.types are present after linking ────────────

#[test]
fn test_runtime_types_present() {
    let wat = linked_runtime_wat();
    for ty in &[
        "rt_types__Array",
        "rt_types__String",
        "rt_types__DictEntry",
        "rt_types__Dict",
        "rt_types__ClosureEnv",
        "rt_types__Closure",
        "rt_types__Variant",
        "rt_types__BoxedInt",
        "rt_types__BoxedFloat",
    ] {
        assert!(wat.contains(ty), "missing type {ty} in linked WAT");
    }
}

// ── 3. All runtime functions exported under their expected names ───────────

#[test]
fn test_runtime_exports() {
    let modules = runtime::all_modules();
    let linked = link(modules, None).expect("link failed");

    let exports: Vec<&str> = linked.exports.iter().map(|e| e.wasm_name.as_str()).collect();

    let expected = [
        // rt.arr
        "make", "get", "set", "len", "concat", "slice",
        // rt.str
        "len", "concat", "substring", "eq", "from_i64", "from_f64", "from_bool",
        // rt.dict
        "make", "len", "keys", "has", "get", "set", "remove",
        // rt.core
        "print", "println", "trap", "eq",
    ];

    for name in &expected {
        assert!(exports.contains(name), "missing export: {name}");
    }
}

// ── 4. Host imports are preserved (not resolved away) ─────────────────────

#[test]
fn test_host_imports_preserved() {
    let wat = linked_runtime_wat();
    // All host imports must remain as Wasm imports
    assert!(wat.contains(r#"(import "host" "print""#));
    assert!(wat.contains(r#"(import "host" "println""#));
    assert!(wat.contains(r#"(import "host" "error""#));
    assert!(wat.contains(r#"(import "host" "f64_to_string""#));
}

// ── 5. Inter-module import (rt.dict → rt.str.eq) is resolved ──────────────

#[test]
fn test_inter_module_import_resolved() {
    let wat = linked_runtime_wat();
    // rt.dict imported rt.str.eq; after linking there should be no "rt.str" Wasm import
    assert!(
        !wat.contains(r#"(import "rt.str""#),
        "rt.str import should be resolved (direct call), not kept as Wasm import"
    );
}

// ── 6. Individual module snapshots ────────────────────────────────────────

#[test]
fn test_snapshot_rt_arr() {
    use twinkle::runtime::arr;

    let m = arr::make();
    let linked = link(vec![runtime::types::make(), m], None).expect("link failed");
    let wat = emit_wat(&linked);
    insta::assert_snapshot!(wat);
}

#[test]
fn test_snapshot_rt_str() {
    use twinkle::runtime::str;
    let m = str::make();
    let linked = link(vec![runtime::types::make(), m], None).expect("link failed");
    let wat = emit_wat(&linked);
    insta::assert_snapshot!(wat);
}
