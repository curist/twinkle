use twinkle::wasm::emit::emit_wat;
use twinkle::wasm::ir::*;
use twinkle::wasm::linker::{LinkError, LinkedModuleIR, link};

// ── helpers ────────────────────────────────────────────────────────────────

fn link_one(module: ModuleIR) -> LinkedModuleIR {
    link(vec![module], None).expect("link failed")
}

// ── test 1: simple struct type + constructor function ──────────────────────

#[test]
fn test_emit_simple_struct() {
    let mut m = ModuleIR::new("user");

    // type Point = .{ x: i64, y: i64 }
    m.types.push(TypeDef::Struct {
        name: "Point".into(),
        supertype: None,
        non_final: false,
        fields: vec![
            FieldDef::named("x", ValType::I64),
            FieldDef::named("y", ValType::I64),
        ],
    });

    // fn make_point(x: i64, y: i64) -> (ref $Point)
    m.funcs.push(FuncDef {
        name: "make_point".into(),
        params: vec![ValType::I64, ValType::I64],
        results: vec![ValType::Ref {
            nullable: false,
            heap: HeapType::Named("Point".into()),
        }],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::LocalGet(1),
            Instr::StructNew("Point".into()),
        ],
    });

    let linked = link_one(m);
    let wat = emit_wat(&linked);
    insta::assert_snapshot!(wat);
}

// ── test 2: import + call ──────────────────────────────────────────────────

#[test]
fn test_emit_import_call() {
    let mut m = ModuleIR::new("user");

    // import host.println(i64) -> void
    m.imports.push(ImportDef {
        module: "host".into(),
        name: "println".into(),
        as_sym: "host_println".into(),
        params: vec![ValType::I64],
        results: vec![],
    });

    // fn greet(n: i64) { host_println(n) }
    m.funcs.push(FuncDef {
        name: "greet".into(),
        params: vec![ValType::I64],
        results: vec![],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::Call("host_println".into())],
    });

    let linked = link_one(m);
    let wat = emit_wat(&linked);
    insta::assert_snapshot!(wat);
}

// ── test 3: cross-module linking resolves import to direct call ────────────

#[test]
fn test_link_two_modules() {
    // Module A: exports "add"
    let mut mod_a = ModuleIR::new("math");
    mod_a.funcs.push(FuncDef {
        name: "add".into(),
        params: vec![ValType::I64, ValType::I64],
        results: vec![ValType::I64],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::LocalGet(1), Instr::I64Add],
    });
    mod_a.exports.push(ExportDef {
        wasm_name: "add".into(),
        func_sym: "add".into(),
    });

    // Module B: imports math.add and wraps it
    let mut mod_b = ModuleIR::new("user");
    mod_b.imports.push(ImportDef {
        module: "math".into(),
        name: "add".into(),
        as_sym: "math_add".into(),
        params: vec![ValType::I64, ValType::I64],
        results: vec![ValType::I64],
    });
    mod_b.funcs.push(FuncDef {
        name: "double_add".into(),
        params: vec![ValType::I64, ValType::I64],
        results: vec![ValType::I64],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::LocalGet(1),
            Instr::Call("math_add".into()),
        ],
    });

    let linked = link(vec![mod_a, mod_b], None).expect("link failed");
    let wat = emit_wat(&linked);

    // After linking, the import should be gone and the call resolved
    assert!(
        !wat.contains("(import \"math\""),
        "inter-module import should be resolved"
    );
    insta::assert_snapshot!(wat);
}

// ── test 4: synthesize __linked_init from multiple start functions ─────────

#[test]
fn test_link_synthesizes_init() {
    let mut mod_a = ModuleIR::new("rt");
    mod_a.funcs.push(FuncDef {
        name: "rt_init".into(),
        params: vec![],
        results: vec![],
        locals: vec![],
        body: vec![Instr::Nop],
    });
    mod_a.start = Some("rt_init".into());

    let mut mod_b = ModuleIR::new("user");
    mod_b.funcs.push(FuncDef {
        name: "user_init".into(),
        params: vec![],
        results: vec![],
        locals: vec![],
        body: vec![Instr::Nop],
    });
    mod_b.start = Some("user_init".into());

    let linked = link(vec![mod_a, mod_b], None).expect("link failed");

    // __linked_init must exist and call both start funcs in order
    let init = linked
        .funcs
        .iter()
        .find(|f| f.name == "__linked_init")
        .expect("__linked_init not found");

    assert_eq!(init.body.len(), 2);
    assert_eq!(init.body[0], Instr::Call("rt__rt_init".into()));
    assert_eq!(init.body[1], Instr::Call("user__user_init".into()));
    assert_eq!(linked.start, Some("__linked_init".into()));

    let wat = emit_wat(&linked);
    insta::assert_snapshot!(wat);
}

// ── test 5: missing export produces LinkError ──────────────────────────────

#[test]
fn test_link_missing_export_error() {
    let mut mod_b = ModuleIR::new("user");
    mod_b.imports.push(ImportDef {
        module: "rt.arr".into(),
        name: "array_new".into(),
        as_sym: "arr_new".into(),
        params: vec![ValType::I64],
        results: vec![ValType::Anyref],
    });
    mod_b.funcs.push(FuncDef {
        name: "main".into(),
        params: vec![],
        results: vec![],
        locals: vec![],
        body: vec![
            Instr::I64Const(10),
            Instr::Call("arr_new".into()),
            Instr::Drop,
        ],
    });

    let result = link(vec![mod_b], None);
    assert!(result.is_err());

    let errs = result.unwrap_err();
    assert_eq!(errs.len(), 1);
    assert!(matches!(
        &errs[0],
        LinkError::MissingExport { module, name }
            if module == "rt.arr" && name == "array_new"
    ));
}
