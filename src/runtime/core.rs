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
    m.imports.push(ImportDef {
        module: "host".into(),
        name: "eprint".into(),
        as_sym: "host_eprint".into(),
        params: vec![ref_string_null()],
        results: vec![],
    });
    m.imports.push(ImportDef {
        module: "host".into(),
        name: "eprintln".into(),
        as_sym: "host_eprintln".into(),
        params: vec![ref_string_null()],
        results: vec![],
    });

    // Import rt.str.eq for structural string comparison in eq()
    m.imports.push(ImportDef {
        module: "rt.str".into(),
        name: "eq".into(),
        as_sym: F_STR_EQ.into(),
        params: vec![ref_string(), ref_string()],
        results: vec![ValType::I32],
    });

    m.funcs.push(print_fn());
    m.funcs.push(println_fn());
    m.funcs.push(eprint_fn());
    m.funcs.push(eprintln_fn());
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

/// `eprint(s: String)`
fn eprint_fn() -> FuncDef {
    FuncDef {
        name: "eprint".into(),
        params: vec![ref_string_null()],
        results: vec![],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::Call("host_eprint".into())],
    }
}

/// `eprintln(s: String)`
fn eprintln_fn() -> FuncDef {
    FuncDef {
        name: "eprintln".into(),
        params: vec![ref_string_null()],
        results: vec![],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::Call("host_eprintln".into())],
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
/// $String → byte-level comparison via rt_str__eq;
/// otherwise falls back to ref.eq (identity).
///
/// Uses `ref.test` (not `ref.cast`) for type checks to avoid trapping on
/// type mismatch.
fn eq_fn() -> FuncDef {
    let cast_to_eqref = Instr::RefCast {
        nullable: true,
        heap: HeapType::Eq,
    };
    let test_boxed_int = Instr::RefTest {
        nullable: false,
        heap: HeapType::Named(T_BOXED_INT.into()),
    };
    let test_string = Instr::RefTest {
        nullable: false,
        heap: HeapType::Named(T_STRING.into()),
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
            // Check if both are $BoxedInt using ref.test
            Instr::LocalGet(0),
            test_boxed_int.clone(),
            Instr::LocalGet(1),
            test_boxed_int,
            Instr::I32And,
            Instr::If {
                result: None,
                then_body: vec![
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
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // Check if both are $String using ref.test
            Instr::LocalGet(0),
            test_string.clone(),
            Instr::LocalGet(1),
            test_string,
            Instr::I32And,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(0),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_STRING.into()),
                    },
                    Instr::LocalGet(1),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_STRING.into()),
                    },
                    Instr::Call(F_STR_EQ.into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // Fallback: identity (ref.eq)
            Instr::LocalGet(0),
            cast_to_eqref.clone(),
            Instr::LocalGet(1),
            cast_to_eqref,
            Instr::RefEq,
        ],
    }
}
