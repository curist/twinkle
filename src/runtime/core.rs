use crate::runtime::types::*;
use crate::wasm::ir::*;

/// Build the `rt.core` module: equality, trap, and host I/O bindings.
pub fn make() -> ModuleIR {
    let mut m = ModuleIR::new("rt.core");

    m.imports.push(ImportDef {
        module: "twinkle_runtime".into(),
        name: "print".into(),
        as_sym: "host_print".into(),
        params: vec![ref_string_null()],
        results: vec![],
    });
    m.imports.push(ImportDef {
        module: "twinkle_runtime".into(),
        name: "println".into(),
        as_sym: "host_println".into(),
        params: vec![ref_string_null()],
        results: vec![],
    });
    m.imports.push(ImportDef {
        module: "twinkle_runtime".into(),
        name: "error".into(),
        as_sym: "host_error".into(),
        params: vec![ref_string_null()],
        results: vec![],
    });
    m.imports.push(ImportDef {
        module: "twinkle_runtime".into(),
        name: "eprint".into(),
        as_sym: "host_eprint".into(),
        params: vec![ref_string_null()],
        results: vec![],
    });
    m.imports.push(ImportDef {
        module: "twinkle_runtime".into(),
        name: "eprintln".into(),
        as_sym: "host_eprintln".into(),
        params: vec![ref_string_null()],
        results: vec![],
    });
    m.imports.push(ImportDef {
        module: "rt.str".into(),
        name: "eq".into(),
        as_sym: "rt_str__eq".into(),
        params: vec![ref_string(), ref_string()],
        results: vec![ValType::I32],
    });
    m.imports.push(ImportDef {
        module: "rt.arr".into(),
        name: "len".into(),
        as_sym: "rt_arr__len".into(),
        params: vec![ref_pvec_null()],
        results: vec![ValType::I32],
    });
    m.imports.push(ImportDef {
        module: "rt.arr".into(),
        name: "get".into(),
        as_sym: "rt_arr__get".into(),
        params: vec![ref_pvec_null(), ValType::I32],
        results: vec![ValType::Anyref],
    });
    m.imports.push(ImportDef {
        module: "rt.dict".into(),
        name: "len".into(),
        as_sym: "rt_dict__len".into(),
        params: vec![ref_pdict_null()],
        results: vec![ValType::I32],
    });
    m.imports.push(ImportDef {
        module: "rt.dict".into(),
        name: "get".into(),
        as_sym: "rt_dict__get".into(),
        params: vec![ref_pdict_null(), ValType::Anyref],
        results: vec![ValType::Anyref],
    });
    m.imports.push(ImportDef {
        module: "rt.dict".into(),
        name: "keys".into(),
        as_sym: "rt_dict__keys".into(),
        params: vec![ref_pdict_null()],
        results: vec![ref_pvec()],
    });

    m.funcs.push(print_fn());
    m.funcs.push(println_fn());
    m.funcs.push(eprint_fn());
    m.funcs.push(eprintln_fn());
    m.funcs.push(trap_fn());
    m.funcs.push(eq_array_fn());
    m.funcs.push(eq_vec_fn());
    m.funcs.push(eq_dict_fn());
    m.funcs.push(eq_variant_fn());
    m.funcs.push(eq_fn());

    for f in &m.funcs {
        m.exports.push(ExportDef {
            wasm_name: f.name.clone(),
            func_sym: f.name.clone(),
        });
    }

    m
}

fn print_fn() -> FuncDef {
    FuncDef {
        name: "print".into(),
        params: vec![ref_string_null()],
        results: vec![],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::Call("host_print".into())],
    }
}

fn println_fn() -> FuncDef {
    FuncDef {
        name: "println".into(),
        params: vec![ref_string_null()],
        results: vec![],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::Call("host_println".into())],
    }
}

fn eprint_fn() -> FuncDef {
    FuncDef {
        name: "eprint".into(),
        params: vec![ref_string_null()],
        results: vec![],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::Call("host_eprint".into())],
    }
}

fn eprintln_fn() -> FuncDef {
    FuncDef {
        name: "eprintln".into(),
        params: vec![ref_string_null()],
        results: vec![],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::Call("host_eprintln".into())],
    }
}

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

fn eq_array_fn() -> FuncDef {
    FuncDef {
        name: "eq_array".into(),
        params: vec![ref_array_null(), ref_array_null()],
        results: vec![ValType::I32],
        locals: vec![ValType::I32, ValType::I32],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefIsNull,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(1),
                    Instr::RefIsNull,
                    Instr::If {
                        result: None,
                        then_body: vec![Instr::I32Const(1), Instr::Return],
                        else_body: vec![],
                    },
                    Instr::I32Const(0),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            Instr::LocalGet(1),
            Instr::RefIsNull,
            Instr::If {
                result: None,
                then_body: vec![Instr::I32Const(0), Instr::Return],
                else_body: vec![],
            },
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(2),
            Instr::LocalGet(1),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalGet(2),
            Instr::I32Ne,
            Instr::If {
                result: None,
                then_body: vec![Instr::I32Const(0), Instr::Return],
                else_body: vec![],
            },
            Instr::I32Const(0),
            Instr::LocalSet(3),
            Instr::Block {
                label: "exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "loop".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(3),
                        Instr::LocalGet(2),
                        Instr::I32GeS,
                        Instr::BrIf("exit".into()),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::ArrayGet(T_ARRAY.into()),
                        Instr::LocalGet(1),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::ArrayGet(T_ARRAY.into()),
                        Instr::Call("eq".into()),
                        Instr::I32Eqz,
                        Instr::If {
                            result: None,
                            then_body: vec![Instr::I32Const(0), Instr::Return],
                            else_body: vec![],
                        },
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(3),
                        Instr::Br("loop".into()),
                    ],
                }],
            },
            Instr::I32Const(1),
        ],
    }
}

fn eq_vec_fn() -> FuncDef {
    FuncDef {
        name: "eq_vec".into(),
        params: vec![ref_pvec(), ref_pvec()],
        results: vec![ValType::I32],
        locals: vec![ValType::I32, ValType::I32],
        body: vec![
            Instr::LocalGet(0),
            Instr::Call("rt_arr__len".into()),
            Instr::LocalSet(2),
            Instr::LocalGet(1),
            Instr::Call("rt_arr__len".into()),
            Instr::LocalGet(2),
            Instr::I32Ne,
            Instr::If {
                result: None,
                then_body: vec![Instr::I32Const(0), Instr::Return],
                else_body: vec![],
            },
            Instr::I32Const(0),
            Instr::LocalSet(3),
            Instr::Block {
                label: "exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "loop".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(3),
                        Instr::LocalGet(2),
                        Instr::I32GeS,
                        Instr::BrIf("exit".into()),
                        Instr::LocalGet(0),
                        Instr::LocalGet(3),
                        Instr::Call("rt_arr__get".into()),
                        Instr::LocalGet(1),
                        Instr::LocalGet(3),
                        Instr::Call("rt_arr__get".into()),
                        Instr::Call("eq".into()),
                        Instr::I32Eqz,
                        Instr::If {
                            result: None,
                            then_body: vec![Instr::I32Const(0), Instr::Return],
                            else_body: vec![],
                        },
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(3),
                        Instr::Br("loop".into()),
                    ],
                }],
            },
            Instr::I32Const(1),
        ],
    }
}

fn eq_dict_fn() -> FuncDef {
    FuncDef {
        name: "eq_dict".into(),
        params: vec![ref_pdict(), ref_pdict()],
        results: vec![ValType::I32],
        locals: vec![
            ValType::I32,
            ref_pvec(),
            ValType::I32,
            ValType::Anyref,
            ValType::Anyref,
            ValType::Anyref,
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::Call("rt_dict__len".into()),
            Instr::LocalSet(2),
            Instr::LocalGet(1),
            Instr::Call("rt_dict__len".into()),
            Instr::LocalGet(2),
            Instr::I32Ne,
            Instr::If {
                result: None,
                then_body: vec![Instr::I32Const(0), Instr::Return],
                else_body: vec![],
            },
            Instr::LocalGet(0),
            Instr::Call("rt_dict__keys".into()),
            Instr::LocalSet(3),
            Instr::I32Const(0),
            Instr::LocalSet(4),
            Instr::Block {
                label: "exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "loop".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(4),
                        Instr::LocalGet(2),
                        Instr::I32GeS,
                        Instr::BrIf("exit".into()),
                        Instr::LocalGet(3),
                        Instr::LocalGet(4),
                        Instr::Call("rt_arr__get".into()),
                        Instr::LocalSet(5),
                        Instr::LocalGet(0),
                        Instr::LocalGet(5),
                        Instr::Call("rt_dict__get".into()),
                        Instr::LocalSet(6),
                        Instr::LocalGet(1),
                        Instr::LocalGet(5),
                        Instr::Call("rt_dict__get".into()),
                        Instr::LocalSet(7),
                        Instr::LocalGet(7),
                        Instr::RefIsNull,
                        Instr::If {
                            result: None,
                            then_body: vec![Instr::I32Const(0), Instr::Return],
                            else_body: vec![],
                        },
                        Instr::LocalGet(6),
                        Instr::LocalGet(7),
                        Instr::Call("eq".into()),
                        Instr::I32Eqz,
                        Instr::If {
                            result: None,
                            then_body: vec![Instr::I32Const(0), Instr::Return],
                            else_body: vec![],
                        },
                        Instr::LocalGet(4),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(4),
                        Instr::Br("loop".into()),
                    ],
                }],
            },
            Instr::I32Const(1),
        ],
    }
}

fn eq_variant_fn() -> FuncDef {
    FuncDef {
        name: "eq_variant".into(),
        params: vec![
            ValType::Ref {
                nullable: false,
                heap: HeapType::Named(T_VARIANT.into()),
            },
            ValType::Ref {
                nullable: false,
                heap: HeapType::Named(T_VARIANT.into()),
            },
        ],
        results: vec![ValType::I32],
        locals: vec![ref_array_null(), ref_array_null()],
        body: vec![
            Instr::LocalGet(0),
            Instr::StructGet(T_VARIANT.into(), 0),
            Instr::LocalGet(1),
            Instr::StructGet(T_VARIANT.into(), 0),
            Instr::I32Ne,
            Instr::If {
                result: None,
                then_body: vec![Instr::I32Const(0), Instr::Return],
                else_body: vec![],
            },
            Instr::LocalGet(0),
            Instr::StructGet(T_VARIANT.into(), 1),
            Instr::LocalGet(1),
            Instr::StructGet(T_VARIANT.into(), 1),
            Instr::I32Ne,
            Instr::If {
                result: None,
                then_body: vec![Instr::I32Const(0), Instr::Return],
                else_body: vec![],
            },
            Instr::LocalGet(0),
            Instr::StructGet(T_VARIANT.into(), 2),
            Instr::LocalSet(2),
            Instr::LocalGet(1),
            Instr::StructGet(T_VARIANT.into(), 2),
            Instr::LocalSet(3),
            Instr::LocalGet(2),
            Instr::LocalGet(3),
            Instr::Call("eq_array".into()),
        ],
    }
}

fn eq_fn() -> FuncDef {
    FuncDef {
        name: "eq".into(),
        params: vec![ValType::Anyref, ValType::Anyref],
        results: vec![ValType::I32],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Eq,
            },
            Instr::LocalGet(1),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Eq,
            },
            Instr::RefEq,
            Instr::If {
                result: None,
                then_body: vec![Instr::I32Const(1), Instr::Return],
                else_body: vec![],
            },
            Instr::LocalGet(0),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::I31,
            },
            Instr::LocalGet(1),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::I31,
            },
            Instr::I32And,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(0),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::I31,
                    },
                    Instr::I31GetU,
                    Instr::LocalGet(1),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::I31,
                    },
                    Instr::I31GetU,
                    Instr::I32Eq,
                    Instr::Return,
                ],
                else_body: vec![],
            },
            Instr::LocalGet(0),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_BOXED_INT.into()),
            },
            Instr::LocalGet(1),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_BOXED_INT.into()),
            },
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
            Instr::LocalGet(0),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_BOXED_FLOAT.into()),
            },
            Instr::LocalGet(1),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_BOXED_FLOAT.into()),
            },
            Instr::I32And,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(0),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_BOXED_FLOAT.into()),
                    },
                    Instr::StructGet(T_BOXED_FLOAT.into(), 0),
                    Instr::LocalGet(1),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_BOXED_FLOAT.into()),
                    },
                    Instr::StructGet(T_BOXED_FLOAT.into(), 0),
                    Instr::F64Eq,
                    Instr::Return,
                ],
                else_body: vec![],
            },
            Instr::LocalGet(0),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_STRING.into()),
            },
            Instr::LocalGet(1),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_STRING.into()),
            },
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
                    Instr::Call("rt_str__eq".into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            Instr::LocalGet(0),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_PVEC.into()),
            },
            Instr::LocalGet(1),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_PVEC.into()),
            },
            Instr::I32And,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(0),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_PVEC.into()),
                    },
                    Instr::LocalGet(1),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_PVEC.into()),
                    },
                    Instr::Call("eq_vec".into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            Instr::LocalGet(0),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_PDICT.into()),
            },
            Instr::LocalGet(1),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_PDICT.into()),
            },
            Instr::I32And,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(0),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_PDICT.into()),
                    },
                    Instr::LocalGet(1),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_PDICT.into()),
                    },
                    Instr::Call("eq_dict".into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            Instr::LocalGet(0),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_VARIANT.into()),
            },
            Instr::LocalGet(1),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_VARIANT.into()),
            },
            Instr::I32And,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(0),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_VARIANT.into()),
                    },
                    Instr::LocalGet(1),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_VARIANT.into()),
                    },
                    Instr::Call("eq_variant".into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            Instr::LocalGet(0),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Eq,
            },
            Instr::LocalGet(1),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Eq,
            },
            Instr::RefEq,
        ],
    }
}
