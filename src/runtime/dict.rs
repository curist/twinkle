use crate::runtime::types::*;
use crate::wasm::ir::*;

/// Build the `rt.dict` module: persistent (COW) dictionary as an unsorted association list.
/// Key comparison uses `rt.core.eq` (structural equality) so both `$BoxedInt` and `$String`
/// keys work correctly.  v0 uses linear scan; replace with sorted list or HAMT later.
pub fn make() -> ModuleIR {
    let mut m = ModuleIR::new("rt.dict");

    // Import rt.core.eq for structural key comparison
    m.imports.push(ImportDef {
        module: "rt.core".into(),
        name: "eq".into(),
        as_sym: "core_eq".into(),
        params: vec![ValType::Anyref, ValType::Anyref],
        results: vec![ValType::I32],
    });

    m.funcs.push(make_fn());
    m.funcs.push(len_fn());
    m.funcs.push(keys_fn());
    m.funcs.push(has_fn());
    m.funcs.push(get_fn());
    m.funcs.push(get_option_fn());
    m.funcs.push(set_fn());
    m.funcs.push(remove_fn());

    for f in &m.funcs {
        m.exports.push(ExportDef {
            wasm_name: f.name.clone(),
            func_sym: f.name.clone(),
        });
    }

    m
}

fn ref_dict_entry_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_DICT_ENTRY.into()),
    }
}

fn ref_dict_entry() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_DICT_ENTRY.into()),
    }
}

/// Emit instructions that compare `entry.key` (already on stack) to `key` (local p1),
/// yielding an i32 via `core_eq`.  Caller pushes `entry.key: anyref` then `key: anyref`
/// before this sequence; this sequence does:  Call("core_eq") → i32
fn key_eq_call() -> Vec<Instr> {
    vec![Instr::Call("core_eq".into())]
}

/// `make() -> Dict`  — empty dict (zero-length array)
fn make_fn() -> FuncDef {
    FuncDef {
        name: "make".into(),
        params: vec![],
        results: vec![ref_dict()],
        locals: vec![],
        body: vec![Instr::ArrayNewFixed(T_DICT.into(), 0)],
    }
}

/// `len(dict: Dict) -> i32`
fn len_fn() -> FuncDef {
    FuncDef {
        name: "len".into(),
        params: vec![ref_dict_null()],
        results: vec![ValType::I32],
        locals: vec![],
        body: vec![Instr::LocalGet(0), Instr::RefAsNonNull, Instr::ArrayLen],
    }
}

/// `keys(dict: Dict) -> Array`  — returns Array<anyref> of keys
fn keys_fn() -> FuncDef {
    // Locals: p1=n (i32), p2=i (i32), p3=result (ref $Array), p4=entry (ref null $DictEntry)
    FuncDef {
        name: "keys".into(),
        params: vec![ref_dict_null()],
        results: vec![ref_array()],
        locals: vec![
            ValType::I32,
            ValType::I32,
            ref_array_null(),
            ref_dict_entry_null(),
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(1),
            Instr::RefNull(HeapType::None),
            Instr::LocalGet(1),
            Instr::ArrayNew(T_ARRAY.into()),
            Instr::LocalSet(3),
            Instr::I32Const(0),
            Instr::LocalSet(2),
            Instr::Block {
                label: "exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "loop".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(2),
                        Instr::LocalGet(1),
                        Instr::I32GeS,
                        Instr::BrIf("exit".into()),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(2),
                        Instr::ArrayGet(T_DICT.into()),
                        Instr::LocalSet(4),
                        Instr::LocalGet(3),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(2),
                        Instr::LocalGet(4),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_DICT_ENTRY.into(), 0),
                        Instr::ArraySet(T_ARRAY.into()),
                        Instr::LocalGet(2),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(2),
                        Instr::Br("loop".into()),
                    ],
                }],
            },
            Instr::LocalGet(3),
            Instr::RefAsNonNull,
        ],
    }
}

/// `has(dict: Dict, key: anyref) -> i32`
fn has_fn() -> FuncDef {
    // Locals: p2=n, p3=i, p4=entry
    FuncDef {
        name: "has".into(),
        params: vec![ref_dict_null(), ValType::Anyref],
        results: vec![ValType::I32],
        locals: vec![ValType::I32, ValType::I32, ref_dict_entry_null()],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(2),
            Instr::I32Const(0),
            Instr::LocalSet(3),
            Instr::Block {
                label: "exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "scan".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(3),
                        Instr::LocalGet(2),
                        Instr::I32GeS,
                        Instr::BrIf("exit".into()),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::ArrayGet(T_DICT.into()),
                        Instr::LocalSet(4),
                        Instr::LocalGet(4),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_DICT_ENTRY.into(), 0),
                        Instr::LocalGet(1),
                    ]
                    .into_iter()
                    .chain(key_eq_call())
                    .chain([
                        Instr::If {
                            result: None,
                            then_body: vec![Instr::I32Const(1), Instr::Return],
                            else_body: vec![],
                        },
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(3),
                        Instr::Br("scan".into()),
                    ])
                    .collect(),
                }],
            },
            Instr::I32Const(0),
        ],
    }
}

/// `get(dict: Dict, key: anyref) -> anyref`  — returns null if absent
fn get_fn() -> FuncDef {
    // Locals: p2=n, p3=i, p4=entry
    FuncDef {
        name: "get".into(),
        params: vec![ref_dict_null(), ValType::Anyref],
        results: vec![ValType::Anyref],
        locals: vec![ValType::I32, ValType::I32, ref_dict_entry_null()],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(2),
            Instr::I32Const(0),
            Instr::LocalSet(3),
            Instr::Block {
                label: "exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "scan".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(3),
                        Instr::LocalGet(2),
                        Instr::I32GeS,
                        Instr::BrIf("exit".into()),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::ArrayGet(T_DICT.into()),
                        Instr::LocalSet(4),
                        Instr::LocalGet(4),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_DICT_ENTRY.into(), 0),
                        Instr::LocalGet(1),
                    ]
                    .into_iter()
                    .chain(key_eq_call())
                    .chain([
                        Instr::If {
                            result: None,
                            then_body: vec![
                                Instr::LocalGet(4),
                                Instr::RefAsNonNull,
                                Instr::StructGet(T_DICT_ENTRY.into(), 1),
                                Instr::Return,
                            ],
                            else_body: vec![],
                        },
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(3),
                        Instr::Br("scan".into()),
                    ])
                    .collect(),
                }],
            },
            Instr::RefNull(HeapType::Any),
        ],
    }
}

fn ref_variant() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_VARIANT.into()),
    }
}

/// `get_option(dict: Dict, key: anyref) -> Variant`
///
/// Like `get` but returns an Option variant: None (type_id=0, variant_id=0)
/// when absent, Some(value) (type_id=0, variant_id=1) when present.
fn get_option_fn() -> FuncDef {
    // Locals: p2=n, p3=i, p4=entry
    FuncDef {
        name: "get_option".into(),
        params: vec![ref_dict_null(), ValType::Anyref],
        results: vec![ref_variant()],
        locals: vec![ValType::I32, ValType::I32, ref_dict_entry_null()],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(2),
            Instr::I32Const(0),
            Instr::LocalSet(3),
            Instr::Block {
                label: "exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "scan".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(3),
                        Instr::LocalGet(2),
                        Instr::I32GeS,
                        Instr::BrIf("exit".into()),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::ArrayGet(T_DICT.into()),
                        Instr::LocalSet(4),
                        Instr::LocalGet(4),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_DICT_ENTRY.into(), 0),
                        Instr::LocalGet(1),
                    ]
                    .into_iter()
                    .chain(key_eq_call())
                    .chain([
                        Instr::If {
                            result: None,
                            then_body: vec![
                                // Found: return Some(value) = Variant(0, 1, [value])
                                Instr::I32Const(0),
                                Instr::I32Const(1),
                                Instr::LocalGet(4),
                                Instr::RefAsNonNull,
                                Instr::StructGet(T_DICT_ENTRY.into(), 1),
                                Instr::ArrayNewFixed(T_ARRAY.into(), 1),
                                Instr::StructNew(T_VARIANT.into()),
                                Instr::Return,
                            ],
                            else_body: vec![],
                        },
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(3),
                        Instr::Br("scan".into()),
                    ])
                    .collect(),
                }],
            },
            // Not found: return None = Variant(0, 0, empty_array)
            Instr::I32Const(0),
            Instr::I32Const(0),
            Instr::RefNull(HeapType::Named(T_ARRAY.into())),
            Instr::StructNew(T_VARIANT.into()),
        ],
    }
}

/// `set(dict: Dict, key: anyref, val: anyref) -> Dict`  — COW insert/update
fn set_fn() -> FuncDef {
    // Locals: p3=n (i32), p4=i (i32), p5=found (i32), p6=result (ref $Dict),
    //         p7=new_entry (ref $DictEntry), p8=entry (ref null $DictEntry),
    //         p9=result_len (i32)
    FuncDef {
        name: "set".into(),
        params: vec![ref_dict_null(), ValType::Anyref, ValType::Anyref],
        results: vec![ref_dict()],
        locals: vec![
            ValType::I32,          // p3 = n
            ValType::I32,          // p4 = i (loop cursor)
            ValType::I32,          // p5 = found
            ref_dict_null(),       // p6 = result
            ref_dict_entry(),      // p7 = new_entry
            ref_dict_entry_null(), // p8 = entry scratch
            ValType::I32,          // p9 = result_len
        ],
        body: vec![
            // p3 = len(dict)
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(3),
            // p7 = new DictEntry { key=p1, val=p2 }
            Instr::LocalGet(1),
            Instr::LocalGet(2),
            Instr::StructNew(T_DICT_ENTRY.into()),
            Instr::LocalSet(7),
            // Scan to check if key exists → p5
            Instr::I32Const(0),
            Instr::LocalSet(4),
            Instr::I32Const(0),
            Instr::LocalSet(5),
            Instr::Block {
                label: "found_exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "scan".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(4),
                        Instr::LocalGet(3),
                        Instr::I32GeS,
                        Instr::BrIf("found_exit".into()),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(4),
                        Instr::ArrayGet(T_DICT.into()),
                        Instr::LocalSet(8),
                        Instr::LocalGet(8),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_DICT_ENTRY.into(), 0),
                        Instr::LocalGet(1),
                    ]
                    .into_iter()
                    .chain(key_eq_call())
                    .chain([
                        Instr::If {
                            result: None,
                            then_body: vec![
                                Instr::I32Const(1),
                                Instr::LocalSet(5),
                                Instr::Br("found_exit".into()),
                            ],
                            else_body: vec![],
                        },
                        Instr::LocalGet(4),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(4),
                        Instr::Br("scan".into()),
                    ])
                    .collect(),
                }],
            },
            // p9 = found ? n : n+1
            Instr::LocalGet(5),
            Instr::If {
                result: Some(ValType::I32),
                then_body: vec![Instr::LocalGet(3)],
                else_body: vec![Instr::LocalGet(3), Instr::I32Const(1), Instr::I32Add],
            },
            Instr::LocalSet(9),
            // result = array.new $Dict (fill=null, len=p9)
            Instr::RefNull(HeapType::Named(T_DICT_ENTRY.into())),
            Instr::LocalGet(9),
            Instr::ArrayNew(T_DICT.into()),
            Instr::LocalSet(6),
            // Copy entries from dict to result, replacing the matched key
            Instr::I32Const(0),
            Instr::LocalSet(4),
            Instr::Block {
                label: "copy_exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "copy".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(4),
                        Instr::LocalGet(3),
                        Instr::I32GeS,
                        Instr::BrIf("copy_exit".into()),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(4),
                        Instr::ArrayGet(T_DICT.into()),
                        Instr::LocalSet(8),
                        Instr::LocalGet(8),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_DICT_ENTRY.into(), 0),
                        Instr::LocalGet(1),
                    ]
                    .into_iter()
                    .chain(key_eq_call())
                    .chain([
                        Instr::If {
                            result: None,
                            then_body: vec![
                                Instr::LocalGet(6),
                                Instr::RefAsNonNull,
                                Instr::LocalGet(4),
                                Instr::LocalGet(7),
                                Instr::ArraySet(T_DICT.into()),
                            ],
                            else_body: vec![
                                Instr::LocalGet(6),
                                Instr::RefAsNonNull,
                                Instr::LocalGet(4),
                                Instr::LocalGet(8),
                                Instr::ArraySet(T_DICT.into()),
                            ],
                        },
                        Instr::LocalGet(4),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(4),
                        Instr::Br("copy".into()),
                    ])
                    .collect(),
                }],
            },
            // If not found: append new_entry at position n
            Instr::LocalGet(5),
            Instr::I32Eqz,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(6),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(3),
                    Instr::LocalGet(7),
                    Instr::ArraySet(T_DICT.into()),
                ],
                else_body: vec![],
            },
            Instr::LocalGet(6),
            Instr::RefAsNonNull,
        ],
    }
}

/// `remove(dict: Dict, key: anyref) -> Dict`
fn remove_fn() -> FuncDef {
    // Two-pass approach:
    // Pass 1: scan to check if key exists → found flag
    // Pass 2: copy non-matching entries into a correctly-sized result array
    // Locals: p2=n, p3=i (scan), p4=j (write), p5=result, p6=entry, p7=found, p8=result_len
    FuncDef {
        name: "remove".into(),
        params: vec![ref_dict_null(), ValType::Anyref],
        results: vec![ref_dict()],
        locals: vec![
            ValType::I32,          // p2 = n
            ValType::I32,          // p3 = i
            ValType::I32,          // p4 = j (write cursor)
            ref_dict_null(),       // p5 = result
            ref_dict_entry_null(), // p6 = entry
            ValType::I32,          // p7 = found
            ValType::I32,          // p8 = result_len
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(2),
            // Pass 1: scan for key
            Instr::I32Const(0),
            Instr::LocalSet(3),
            Instr::I32Const(0),
            Instr::LocalSet(7),
            Instr::Block {
                label: "scan_exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "scan".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(3),
                        Instr::LocalGet(2),
                        Instr::I32GeS,
                        Instr::BrIf("scan_exit".into()),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::ArrayGet(T_DICT.into()),
                        Instr::LocalSet(6),
                        Instr::LocalGet(6),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_DICT_ENTRY.into(), 0),
                        Instr::LocalGet(1),
                    ]
                    .into_iter()
                    .chain(key_eq_call())
                    .chain([
                        Instr::If {
                            result: None,
                            then_body: vec![
                                Instr::I32Const(1),
                                Instr::LocalSet(7),
                                Instr::Br("scan_exit".into()),
                            ],
                            else_body: vec![],
                        },
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(3),
                        Instr::Br("scan".into()),
                    ])
                    .collect(),
                }],
            },
            // result_len = found ? n-1 : n
            Instr::LocalGet(7),
            Instr::If {
                result: Some(ValType::I32),
                then_body: vec![Instr::LocalGet(2), Instr::I32Const(1), Instr::I32Sub],
                else_body: vec![Instr::LocalGet(2)],
            },
            Instr::LocalSet(8),
            // result = array.new $Dict (null, result_len)
            Instr::RefNull(HeapType::Named(T_DICT_ENTRY.into())),
            Instr::LocalGet(8),
            Instr::ArrayNew(T_DICT.into()),
            Instr::LocalSet(5),
            // Pass 2: copy non-matching entries
            Instr::I32Const(0),
            Instr::LocalSet(3),
            Instr::I32Const(0),
            Instr::LocalSet(4),
            Instr::Block {
                label: "copy_exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "copy".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(3),
                        Instr::LocalGet(2),
                        Instr::I32GeS,
                        Instr::BrIf("copy_exit".into()),
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::ArrayGet(T_DICT.into()),
                        Instr::LocalSet(6),
                        Instr::LocalGet(6),
                        Instr::RefAsNonNull,
                        Instr::StructGet(T_DICT_ENTRY.into(), 0),
                        Instr::LocalGet(1),
                    ]
                    .into_iter()
                    .chain(key_eq_call())
                    .chain([
                        Instr::If {
                            result: None,
                            // skip matching entry
                            then_body: vec![],
                            else_body: vec![
                                Instr::LocalGet(5),
                                Instr::RefAsNonNull,
                                Instr::LocalGet(4),
                                Instr::LocalGet(6),
                                Instr::ArraySet(T_DICT.into()),
                                Instr::LocalGet(4),
                                Instr::I32Const(1),
                                Instr::I32Add,
                                Instr::LocalSet(4),
                            ],
                        },
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(3),
                        Instr::Br("copy".into()),
                    ])
                    .collect(),
                }],
            },
            Instr::LocalGet(5),
            Instr::RefAsNonNull,
        ],
    }
}
