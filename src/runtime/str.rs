use crate::runtime::types::*;
use crate::wasm::ir::*;

/// Build the `rt.str` module: string operations.
pub fn make() -> ModuleIR {
    let mut m = ModuleIR::new("rt.str");

    // from_f64 delegates to a host import (IEEE 754 → decimal is non-trivial in Wasm GC)
    m.imports.push(ImportDef {
        module: "host".into(),
        name: "f64_to_string".into(),
        as_sym: "host_f64_to_string".into(),
        params: vec![ValType::F64],
        results: vec![ref_string()],
    });

    m.funcs.push(len_fn());
    m.funcs.push(concat_fn());
    m.funcs.push(substring_fn());
    m.funcs.push(eq_fn());
    m.funcs.push(cmp_fn());
    m.funcs.push(from_i64_fn());
    m.funcs.push(from_f64_fn());
    m.funcs.push(from_bool_fn());

    for f in &m.funcs {
        m.exports.push(ExportDef {
            wasm_name: f.name.clone(),
            func_sym: f.name.clone(),
        });
    }

    m
}

/// `len(s: String) -> i32`
fn len_fn() -> FuncDef {
    FuncDef {
        name: "len".into(),
        params: vec![ref_string_null()],
        results: vec![ValType::I32],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::RefAsNonNull, Instr::ArrayLen],
    }
}

/// `concat(a: String, b: String) -> String`
fn concat_fn() -> FuncDef {
    // Locals: p2=len_a, p3=len_b, p4=total, p5=result
    FuncDef {
        name: "concat".into(),
        params: vec![ref_string_null(), ref_string_null()],
        results: vec![ref_string()],
        locals: vec![ValType::I32, ValType::I32, ValType::I32, ref_string_null()],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(2),
            Instr::LocalGet(1),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(3),
            Instr::LocalGet(2),
            Instr::LocalGet(3),
            Instr::I32Add,
            Instr::LocalSet(4),
            Instr::I32Const(0),
            Instr::LocalGet(4),
            Instr::ArrayNew(T_STRING.into()),
            Instr::LocalSet(5),
            // copy a into result[0..len_a]
            Instr::LocalGet(5),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(2),
            Instr::ArrayCopy(T_STRING.into(), T_STRING.into()),
            // copy b into result[len_a..total]
            Instr::LocalGet(5),
            Instr::RefAsNonNull,
            Instr::LocalGet(2),
            Instr::LocalGet(1),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(3),
            Instr::ArrayCopy(T_STRING.into(), T_STRING.into()),
            Instr::LocalGet(5),
            Instr::RefAsNonNull,
        ],
    }
}

/// `substring(s: String, start: i32, end: i32) -> String`
fn substring_fn() -> FuncDef {
    // Locals: p3=len/new_len, p4=result
    FuncDef {
        name: "substring".into(),
        params: vec![ref_string_null(), ValType::I32, ValType::I32],
        results: vec![ref_string()],
        locals: vec![ValType::I32, ref_string_null()],
        body: vec![
            // p3 = len(s)
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(3),
            // Clamp start into [len if start < 0, min(start, len)]
            Instr::LocalGet(1),
            Instr::I32Const(0),
            Instr::I32LtS,
            Instr::If {
                result: None,
                then_body: vec![Instr::LocalGet(3), Instr::LocalSet(1)],
                else_body: vec![],
            },
            Instr::LocalGet(1),
            Instr::LocalGet(3),
            Instr::I32GtS,
            Instr::If {
                result: None,
                then_body: vec![Instr::LocalGet(3), Instr::LocalSet(1)],
                else_body: vec![],
            },
            // Clamp end into [len if end < 0, min(end, len)]
            Instr::LocalGet(2),
            Instr::I32Const(0),
            Instr::I32LtS,
            Instr::If {
                result: None,
                then_body: vec![Instr::LocalGet(3), Instr::LocalSet(2)],
                else_body: vec![],
            },
            Instr::LocalGet(2),
            Instr::LocalGet(3),
            Instr::I32GtS,
            Instr::If {
                result: None,
                then_body: vec![Instr::LocalGet(3), Instr::LocalSet(2)],
                else_body: vec![],
            },
            // end must be >= start
            Instr::LocalGet(2),
            Instr::LocalGet(1),
            Instr::I32LtS,
            Instr::If {
                result: None,
                then_body: vec![Instr::LocalGet(1), Instr::LocalSet(2)],
                else_body: vec![],
            },
            // p3 = new_len = end - start
            Instr::LocalGet(2),
            Instr::LocalGet(1),
            Instr::I32Sub,
            Instr::LocalSet(3),
            Instr::I32Const(0),
            Instr::LocalGet(3),
            Instr::ArrayNew(T_STRING.into()),
            Instr::LocalSet(4),
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::LocalGet(1),
            Instr::LocalGet(3),
            Instr::ArrayCopy(T_STRING.into(), T_STRING.into()),
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
        ],
    }
}

/// `eq(a: String, b: String) -> i32`  — byte-by-byte equality
fn eq_fn() -> FuncDef {
    // Locals: p2=len, p3=i
    FuncDef {
        name: "eq".into(),
        params: vec![ref_string_null(), ref_string_null()],
        results: vec![ValType::I32],
        locals: vec![ValType::I32, ValType::I32],
        body: vec![
            // p2 = len(a)
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(2),
            // if len(a) != len(b): return 0
            Instr::LocalGet(2),
            Instr::LocalGet(1),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::I32Ne,
            Instr::If {
                result: None,
                then_body: vec![Instr::I32Const(0), Instr::Return],
                else_body: vec![],
            },
            // p3 = 0
            Instr::I32Const(0),
            Instr::LocalSet(3),
            Instr::Block {
                label: "exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "cmp".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(3),
                        Instr::LocalGet(2),
                        Instr::I32GeS,
                        Instr::BrIf("exit".into()),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::ArrayGetU(T_STRING.into()),
                        Instr::LocalGet(1),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::ArrayGetU(T_STRING.into()),
                        Instr::I32Ne,
                        Instr::If {
                            result: None,
                            then_body: vec![Instr::I32Const(0), Instr::Return],
                            else_body: vec![],
                        },
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(3),
                        Instr::Br("cmp".into()),
                    ],
                }],
            },
            Instr::I32Const(1),
        ],
    }
}

/// `cmp(a: String, b: String) -> i32`  — returns -1, 0, or 1 (lexicographic)
///
/// Locals: p2=min_len, p3=i, p4=byte_a, p5=byte_b, p6=len_a, p7=len_b
fn cmp_fn() -> FuncDef {
    FuncDef {
        name: "cmp".into(),
        params: vec![ref_string_null(), ref_string_null()],
        results: vec![ValType::I32],
        locals: vec![
            ValType::I32, // p2 = min_len
            ValType::I32, // p3 = i
            ValType::I32, // p4 = byte_a
            ValType::I32, // p5 = byte_b
            ValType::I32, // p6 = len_a
            ValType::I32, // p7 = len_b
        ],
        body: vec![
            // p6 = len(a)
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(6),
            // p7 = len(b)
            Instr::LocalGet(1),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(7),
            // p2 = min(len_a, len_b)
            Instr::LocalGet(6),
            Instr::LocalGet(7),
            Instr::LocalGet(6),
            Instr::LocalGet(7),
            Instr::I32LeS,
            Instr::Select,
            Instr::LocalSet(2),
            // p3 = 0
            Instr::I32Const(0),
            Instr::LocalSet(3),
            // byte-by-byte comparison loop
            Instr::Block {
                label: "done".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "cmp_loop".into(),
                    result: None,
                    body: vec![
                        // if i >= min_len: break
                        Instr::LocalGet(3),
                        Instr::LocalGet(2),
                        Instr::I32GeS,
                        Instr::BrIf("done".into()),
                        // p4 = a[i]
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::ArrayGetU(T_STRING.into()),
                        Instr::LocalSet(4),
                        // p5 = b[i]
                        Instr::LocalGet(1),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::ArrayGetU(T_STRING.into()),
                        Instr::LocalSet(5),
                        // if byte_a < byte_b: return -1
                        Instr::LocalGet(4),
                        Instr::LocalGet(5),
                        Instr::I32LtU,
                        Instr::If {
                            result: None,
                            then_body: vec![Instr::I32Const(-1), Instr::Return],
                            else_body: vec![],
                        },
                        // if byte_a > byte_b: return 1
                        Instr::LocalGet(4),
                        Instr::LocalGet(5),
                        Instr::I32GtU,
                        Instr::If {
                            result: None,
                            then_body: vec![Instr::I32Const(1), Instr::Return],
                            else_body: vec![],
                        },
                        // i++
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(3),
                        Instr::Br("cmp_loop".into()),
                    ],
                }],
            },
            // All shared bytes equal — compare lengths
            // if len_a < len_b: return -1
            Instr::LocalGet(6),
            Instr::LocalGet(7),
            Instr::I32LtS,
            Instr::If {
                result: None,
                then_body: vec![Instr::I32Const(-1), Instr::Return],
                else_body: vec![],
            },
            // if len_a > len_b: return 1
            Instr::LocalGet(6),
            Instr::LocalGet(7),
            Instr::I32GtS,
            Instr::If {
                result: None,
                then_body: vec![Instr::I32Const(1), Instr::Return],
                else_body: vec![],
            },
            // equal
            Instr::I32Const(0),
        ],
    }
}

/// `from_i64(n: i64) -> String`
///
/// Locals: p1=neg (i32), p2=work (i64), p3=pos (i32), p4=result_len (i32),
///         p5=buf (String scratch, 20 bytes), p6=result (String)
fn from_i64_fn() -> FuncDef {
    FuncDef {
        name: "from_i64".into(),
        params: vec![ValType::I64],
        results: vec![ref_string()],
        locals: vec![
            ValType::I32,      // p1 = neg
            ValType::I64,      // p2 = work
            ValType::I32,      // p3 = pos (descends from 19)
            ValType::I32,      // p4 = result_len
            ref_string_null(), // p5 = buf (scratch)
            ref_string_null(), // p6 = result
        ],
        body: vec![
            // Special case: n == 0 → return "0"
            Instr::LocalGet(0),
            Instr::I64Eqz,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::I32Const(48),
                    Instr::ArrayNewFixed(T_STRING.into(), 1),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // Special case: i64::MIN — negation would overflow, emit literal string
            Instr::LocalGet(0),
            Instr::I64Const(i64::MIN),
            Instr::I64Eq,
            Instr::If {
                result: None,
                then_body: vec![
                    // "-9223372036854775808"  (20 bytes)
                    Instr::I32Const(45), // '-'
                    Instr::I32Const(57), // '9'
                    Instr::I32Const(50), // '2'
                    Instr::I32Const(50), // '2'
                    Instr::I32Const(51), // '3'
                    Instr::I32Const(51), // '3'
                    Instr::I32Const(55), // '7'
                    Instr::I32Const(50), // '2'
                    Instr::I32Const(48), // '0'
                    Instr::I32Const(51), // '3'
                    Instr::I32Const(54), // '6'
                    Instr::I32Const(56), // '8'
                    Instr::I32Const(53), // '5'
                    Instr::I32Const(52), // '4'
                    Instr::I32Const(55), // '7'
                    Instr::I32Const(55), // '7'
                    Instr::I32Const(53), // '5'
                    Instr::I32Const(56), // '8'
                    Instr::I32Const(48), // '0'
                    Instr::I32Const(56), // '8'
                    Instr::ArrayNewFixed(T_STRING.into(), 20),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // neg = (n < 0)
            Instr::LocalGet(0),
            Instr::I64Const(0),
            Instr::I64LtS,
            Instr::LocalSet(1),
            // work = neg ? -n : n  (safe: i64::MIN already handled above)
            Instr::LocalGet(1),
            Instr::If {
                result: Some(ValType::I64),
                then_body: vec![Instr::I64Const(0), Instr::LocalGet(0), Instr::I64Sub],
                else_body: vec![Instr::LocalGet(0)],
            },
            Instr::LocalSet(2),
            // buf = array.new $String (fill=0, len=20)
            Instr::I32Const(0),
            Instr::I32Const(20),
            Instr::ArrayNew(T_STRING.into()),
            Instr::LocalSet(5),
            // pos = 19
            Instr::I32Const(19),
            Instr::LocalSet(3),
            // digit extraction loop
            Instr::Loop {
                label: "digits".into(),
                result: None,
                body: vec![
                    // buf[pos] = '0' + (work % 10)
                    Instr::LocalGet(5),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(3),
                    Instr::LocalGet(2),
                    Instr::I64Const(10),
                    Instr::I64RemS,
                    Instr::I32WrapI64,
                    Instr::I32Const(48),
                    Instr::I32Add,
                    Instr::ArraySet(T_STRING.into()),
                    // work /= 10
                    Instr::LocalGet(2),
                    Instr::I64Const(10),
                    Instr::I64DivS,
                    Instr::LocalSet(2),
                    // pos--
                    Instr::LocalGet(3),
                    Instr::I32Const(1),
                    Instr::I32Sub,
                    Instr::LocalSet(3),
                    // continue if work != 0
                    Instr::LocalGet(2),
                    Instr::I64Eqz,
                    Instr::I32Eqz,
                    Instr::BrIf("digits".into()),
                ],
            },
            // if neg: buf[pos] = '-'; pos--
            Instr::LocalGet(1),
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(5),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(3),
                    Instr::I32Const(45), // '-'
                    Instr::ArraySet(T_STRING.into()),
                    Instr::LocalGet(3),
                    Instr::I32Const(1),
                    Instr::I32Sub,
                    Instr::LocalSet(3),
                ],
                else_body: vec![],
            },
            // result_len = 19 - pos
            Instr::I32Const(19),
            Instr::LocalGet(3),
            Instr::I32Sub,
            Instr::LocalSet(4),
            // result = array.new $String (0, result_len)
            Instr::I32Const(0),
            Instr::LocalGet(4),
            Instr::ArrayNew(T_STRING.into()),
            Instr::LocalSet(6),
            // array.copy result 0 buf (pos+1) result_len
            Instr::LocalGet(6),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(5),
            Instr::RefAsNonNull,
            Instr::LocalGet(3),
            Instr::I32Const(1),
            Instr::I32Add,
            Instr::LocalGet(4),
            Instr::ArrayCopy(T_STRING.into(), T_STRING.into()),
            Instr::LocalGet(6),
            Instr::RefAsNonNull,
        ],
    }
}

/// `from_f64(n: f64) -> String`  — delegates to host (IEEE 754 formatting is non-trivial)
fn from_f64_fn() -> FuncDef {
    FuncDef {
        name: "from_f64".into(),
        params: vec![ValType::F64],
        results: vec![ref_string()],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::Call("host_f64_to_string".into())],
    }
}

/// `from_bool(b: i32) -> String`
fn from_bool_fn() -> FuncDef {
    FuncDef {
        name: "from_bool".into(),
        params: vec![ValType::I32],
        results: vec![ref_string()],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::If {
                result: Some(ref_string()),
                then_body: vec![
                    Instr::I32Const(116),
                    Instr::I32Const(114),
                    Instr::I32Const(117),
                    Instr::I32Const(101),
                    Instr::ArrayNewFixed(T_STRING.into(), 4),
                ],
                else_body: vec![
                    Instr::I32Const(102),
                    Instr::I32Const(97),
                    Instr::I32Const(108),
                    Instr::I32Const(115),
                    Instr::I32Const(101),
                    Instr::ArrayNewFixed(T_STRING.into(), 5),
                ],
            },
        ],
    }
}
