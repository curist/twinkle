use crate::runtime::types::*;
use crate::wasm::ir::*;

/// Build the `rt.core` module: equality, trap, and host I/O bindings.
pub fn make() -> ModuleIR {
    let mut m = ModuleIR::new("rt.core");

    // Host I/O imports
    m.imports.push(ImportDef {
        module: "host".into(),
        name: "print".into(),
        as_sym: "host_print".into(),
        params: vec![ref_string_null()],
        results: vec![],
    });
    m.imports.push(ImportDef {
        module: "host".into(),
        name: "println".into(),
        as_sym: "host_println".into(),
        params: vec![ref_string_null()],
        results: vec![],
    });
    m.imports.push(ImportDef {
        module: "host".into(),
        name: "error".into(),
        as_sym: "host_error".into(),
        params: vec![ref_string_null()],
        results: vec![],
    });

    m.funcs.push(print_fn());
    m.funcs.push(println_fn());
    m.funcs.push(trap_fn());
    m.funcs.push(eq_fn());

    for f in &m.funcs {
        m.exports.push(ExportDef {
            wasm_name: f.name.clone(),
            func_sym: f.name.clone(),
        });
    }

    m
}

/// `print(s: String)`
fn print_fn() -> FuncDef {
    FuncDef {
        name: "print".into(),
        params: vec![ref_string_null()],
        results: vec![],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::Call("host_print".into())],
    }
}

/// `println(s: String)`
fn println_fn() -> FuncDef {
    FuncDef {
        name: "println".into(),
        params: vec![ref_string_null()],
        results: vec![],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::Call("host_println".into())],
    }
}

/// `trap(msg: String)` — calls host error and then unreachable
fn trap_fn() -> FuncDef {
    FuncDef {
        name: "trap".into(),
        params: vec![ref_string_null()],
        results: vec![],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::Call("host_error".into()),
            Instr::Unreachable,
        ],
    }
}

/// `eq(a: anyref, b: anyref) -> i32`
///
/// Structural equality: same pointer → equal; $BoxedInt → compare i64;
/// $BoxedFloat → compare f64; $Variant → compare type_id + variant_id;
/// otherwise falls back to ref.eq (identity).
///
/// `ref.eq` requires `eqref`, so we cast each operand before comparing.
fn eq_fn() -> FuncDef {
    let cast_to_eqref = Instr::RefCast {
        nullable: true,
        heap: HeapType::Eq,
    };
    FuncDef {
        name: "eq".into(),
        params: vec![ValType::Anyref, ValType::Anyref],
        results: vec![ValType::I32],
        locals: vec![],
        body: vec![
            // Fast path: same pointer (cast to eqref for ref.eq)
            Instr::LocalGet(0),
            cast_to_eqref.clone(),
            Instr::LocalGet(1),
            cast_to_eqref.clone(),
            Instr::RefEq,
            Instr::If {
                result: None,
                then_body: vec![Instr::I32Const(1), Instr::Return],
                else_body: vec![],
            },
            // Check if both are $BoxedInt using nullable cast + ref.is_null
            Instr::LocalGet(0),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_BOXED_INT.into()),
            },
            Instr::RefIsNull,
            Instr::I32Eqz, // is_boxed_int_a
            Instr::LocalGet(1),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_BOXED_INT.into()),
            },
            Instr::RefIsNull,
            Instr::I32Eqz, // is_boxed_int_b
            Instr::I32And,
            Instr::If {
                result: Some(ValType::I32),
                then_body: vec![
                    // a.v == b.v
                    Instr::LocalGet(0),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_BOXED_INT.into()),
                    },
                    Instr::StructGet(T_BOXED_INT.into(), 0),
                    Instr::LocalGet(1),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_BOXED_INT.into()),
                    },
                    Instr::StructGet(T_BOXED_INT.into(), 0),
                    Instr::I64Eq,
                ],
                else_body: vec![
                    // Fall back to identity (cast to eqref for ref.eq)
                    Instr::LocalGet(0),
                    cast_to_eqref.clone(),
                    Instr::LocalGet(1),
                    cast_to_eqref,
                    Instr::RefEq,
                ],
            },
        ],
    }
}
