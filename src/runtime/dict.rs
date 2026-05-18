use crate::runtime::types::*;
use crate::wasm::ir::*;

// Field indices
const HE_HASH: u32 = 0;
const HE_KEY: u32 = 1;
const HE_VAL: u32 = 2;
const HN_BITMAP: u32 = 0;
const HN_ENTRIES: u32 = 1;
const HC_HASH: u32 = 1;
const HC_ENTRIES: u32 = 2;
const PD_SIZE: u32 = 0;
const PD_ROOT: u32 = 1;
const PD_ORDER: u32 = 2;

fn ref_hamt_entry_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_HAMT_ENTRY.into()),
    }
}
fn ref_hamt_node() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_HAMT_NODE.into()),
    }
}
fn ref_hamt_node_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_HAMT_NODE.into()),
    }
}
fn ref_hamt_collision_null() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_HAMT_COLLISION.into()),
    }
}
fn ref_hamt_collision() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_HAMT_COLLISION.into()),
    }
}
fn ref_string_local() -> ValType {
    ValType::Ref {
        nullable: true,
        heap: HeapType::Named(T_STRING.into()),
    }
}
fn ref_variant() -> ValType {
    ValType::Ref {
        nullable: false,
        heap: HeapType::Named(T_VARIANT.into()),
    }
}

pub fn make() -> ModuleIR {
    let mut m = ModuleIR::new("rt.dict");

    m.imports.push(ImportDef {
        module: "rt.core".into(),
        name: "eq".into(),
        as_sym: "core_eq".into(),
        params: vec![ValType::Anyref, ValType::Anyref],
        results: vec![ValType::I32],
    });
    m.imports.push(ImportDef {
        module: "rt.arr".into(),
        name: "push".into(),
        as_sym: "arr_push".into(),
        params: vec![ref_pvec(), ValType::Anyref],
        results: vec![ref_pvec()],
    });
    m.imports.push(ImportDef {
        module: "rt.arr".into(),
        name: "len".into(),
        as_sym: "arr_len".into(),
        params: vec![ref_pvec_null()],
        results: vec![ValType::I32],
    });
    m.imports.push(ImportDef {
        module: "rt.arr".into(),
        name: "get".into(),
        as_sym: "arr_get".into(),
        params: vec![ref_pvec_null(), ValType::I32],
        results: vec![ValType::Anyref],
    });

    // Internal helpers
    m.funcs.push(popcount_fn());
    m.funcs.push(arr_insert_at_fn());
    m.funcs.push(arr_replace_at_fn());
    m.funcs.push(arr_remove_at_fn());
    m.funcs.push(hash_i64_fn());
    m.funcs.push(hash_string_fn());
    m.funcs.push(hash_key_fn());
    m.funcs.push(collision_get_fn());
    m.funcs.push(collision_set_fn());
    m.funcs.push(node_get_fn());
    m.funcs.push(node_set_fn());
    m.funcs.push(node_remove_fn());
    m.funcs.push(order_remove_key_fn());

    // Public API
    m.funcs.push(make_fn());
    m.funcs.push(len_fn());
    m.funcs.push(keys_fn());
    m.funcs.push(has_fn());
    m.funcs.push(get_fn());
    m.funcs.push(get_option_fn());
    m.funcs.push(set_fn());
    m.funcs.push(remove_fn());
    m.funcs.push(set_in_place_fn());
    m.funcs.push(remove_in_place_fn());

    for f in &m.funcs {
        m.exports.push(ExportDef {
            wasm_name: f.name.clone(),
            func_sym: f.name.clone(),
        });
    }
    m
}

// ── popcount(v: i32) -> i32 ──────────────────────────────────────────────────
// Brian Kernighan's algorithm: v = v & (v-1) removes lowest set bit.
fn popcount_fn() -> FuncDef {
    FuncDef {
        name: "popcount".into(),
        params: vec![ValType::I32],
        results: vec![ValType::I32],
        locals: vec![ValType::I32],
        body: vec![
            Instr::I32Const(0),
            Instr::LocalSet(1), // count = 0
            Instr::Block {
                label: "exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "loop".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(0),
                        Instr::I32Eqz,
                        Instr::BrIf("exit".into()),
                        // v = v & (v - 1)
                        Instr::LocalGet(0),
                        Instr::LocalGet(0),
                        Instr::I32Const(1),
                        Instr::I32Sub,
                        Instr::I32And,
                        Instr::LocalSet(0),
                        // count++
                        Instr::LocalGet(1),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(1),
                        Instr::Br("loop".into()),
                    ],
                }],
            },
            Instr::LocalGet(1),
        ],
    }
}

// ── arr_insert_at(arr, idx, val) -> Array ─────────────────────────────────────
fn arr_insert_at_fn() -> FuncDef {
    // p0=arr, p1=idx, p2=val; L3=n, L4=new_arr, L5=tail_len
    FuncDef {
        name: "arr_insert_at".into(),
        params: vec![ref_array(), ValType::I32, ValType::Anyref],
        results: vec![ref_array()],
        locals: vec![ValType::I32, ref_array_null(), ValType::I32],
        body: vec![
            Instr::LocalGet(0),
            Instr::ArrayLen,
            Instr::LocalSet(3),
            // new_arr = Array(n+1)
            Instr::RefNull(HeapType::Any),
            Instr::LocalGet(3),
            Instr::I32Const(1),
            Instr::I32Add,
            Instr::ArrayNew(T_ARRAY.into()),
            Instr::LocalSet(4),
            // if idx > 0: copy arr[0..idx] → new_arr[0]
            Instr::LocalGet(1),
            Instr::I32Const(0),
            Instr::I32GtS,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(0),
                    Instr::I32Const(0),
                    Instr::LocalGet(1),
                    Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
                ],
                else_body: vec![],
            },
            // new_arr[idx] = val
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
            Instr::LocalGet(1),
            Instr::LocalGet(2),
            Instr::ArraySet(T_ARRAY.into()),
            // tail_len = n - idx
            Instr::LocalGet(3),
            Instr::LocalGet(1),
            Instr::I32Sub,
            Instr::LocalSet(5),
            // if tail_len > 0: copy arr[idx..n] → new_arr[idx+1]
            Instr::LocalGet(5),
            Instr::I32Const(0),
            Instr::I32GtS,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(4),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(1),
                    Instr::I32Const(1),
                    Instr::I32Add, // dst_off = idx+1
                    Instr::LocalGet(0),
                    Instr::LocalGet(1),
                    Instr::LocalGet(5),
                    Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
                ],
                else_body: vec![],
            },
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
        ],
    }
}

// ── arr_replace_at(arr, idx, val) -> Array ────────────────────────────────────
fn arr_replace_at_fn() -> FuncDef {
    // p0=arr, p1=idx, p2=val; L3=n, L4=new_arr
    FuncDef {
        name: "arr_replace_at".into(),
        params: vec![ref_array(), ValType::I32, ValType::Anyref],
        results: vec![ref_array()],
        locals: vec![ValType::I32, ref_array_null()],
        body: vec![
            Instr::LocalGet(0),
            Instr::ArrayLen,
            Instr::LocalSet(3),
            Instr::RefNull(HeapType::Any),
            Instr::LocalGet(3),
            Instr::ArrayNew(T_ARRAY.into()),
            Instr::LocalSet(4),
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
            Instr::I32Const(0),
            Instr::LocalGet(0),
            Instr::I32Const(0),
            Instr::LocalGet(3),
            Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
            Instr::LocalGet(1),
            Instr::LocalGet(2),
            Instr::ArraySet(T_ARRAY.into()),
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
        ],
    }
}

// ── arr_remove_at(arr, idx) -> Array ─────────────────────────────────────────
fn arr_remove_at_fn() -> FuncDef {
    // p0=arr, p1=idx; L2=n, L3=new_arr, L4=tail_len
    FuncDef {
        name: "arr_remove_at".into(),
        params: vec![ref_array(), ValType::I32],
        results: vec![ref_array()],
        locals: vec![ValType::I32, ref_array_null(), ValType::I32],
        body: vec![
            Instr::LocalGet(0),
            Instr::ArrayLen,
            Instr::LocalSet(2),
            Instr::RefNull(HeapType::Any),
            Instr::LocalGet(2),
            Instr::I32Const(1),
            Instr::I32Sub,
            Instr::ArrayNew(T_ARRAY.into()),
            Instr::LocalSet(3),
            // if idx > 0: copy prefix
            Instr::LocalGet(1),
            Instr::I32Const(0),
            Instr::I32GtS,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(3),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::LocalGet(0),
                    Instr::I32Const(0),
                    Instr::LocalGet(1),
                    Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
                ],
                else_body: vec![],
            },
            // tail_len = n - idx - 1
            Instr::LocalGet(2),
            Instr::LocalGet(1),
            Instr::I32Sub,
            Instr::I32Const(1),
            Instr::I32Sub,
            Instr::LocalSet(4),
            // if tail_len > 0: copy suffix
            Instr::LocalGet(4),
            Instr::I32Const(0),
            Instr::I32GtS,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(3),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(1),
                    Instr::LocalGet(0),
                    Instr::LocalGet(1),
                    Instr::I32Const(1),
                    Instr::I32Add,
                    Instr::LocalGet(4),
                    Instr::ArrayCopy(T_ARRAY.into(), T_ARRAY.into()),
                ],
                else_body: vec![],
            },
            Instr::LocalGet(3),
            Instr::RefAsNonNull,
        ],
    }
}

// ── hash_i64(v: i64) -> i32 ──────────────────────────────────────────────────
// Wang/Knuth mix for i64 → i32.
fn hash_i64_fn() -> FuncDef {
    // h = lower XOR upper; h = h * 2654435761; return h
    FuncDef {
        name: "hash_i64".into(),
        params: vec![ValType::I64],
        results: vec![ValType::I32],
        locals: vec![ValType::I32],
        body: vec![
            // lower = i32.wrap_i64(v)
            Instr::LocalGet(0),
            Instr::I32WrapI64,
            // upper = i32.wrap_i64(v >> 32)
            Instr::LocalGet(0),
            Instr::I64Const(32),
            Instr::I64ShrS,
            Instr::I32WrapI64,
            // h = lower XOR upper
            Instr::I32Xor,
            Instr::LocalSet(1),
            // h = h * 2654435761 (Knuth's multiplicative constant)
            Instr::LocalGet(1),
            Instr::I32Const(-1640531527i32), // 2654435769u32 as i32
            Instr::I32Mul,
        ],
    }
}

// ── hash_string(s: ref null $String) -> i32 ──────────────────────────────────
// FNV-1a 32-bit.
fn hash_string_fn() -> FuncDef {
    // p0=s; L1=hash, L2=n, L3=i
    FuncDef {
        name: "hash_string".into(),
        params: vec![ref_string_local()],
        results: vec![ValType::I32],
        locals: vec![ValType::I32, ValType::I32, ValType::I32],
        body: vec![
            // hash = 2166136261 (FNV offset basis as i32: -2128831035)
            Instr::I32Const(-2128831035i32),
            Instr::LocalSet(1),
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
                    label: "loop".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(3),
                        Instr::LocalGet(2),
                        Instr::I32GeS,
                        Instr::BrIf("exit".into()),
                        // byte = s[i] (unsigned)
                        Instr::LocalGet(0),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(3),
                        Instr::ArrayGetU(T_STRING.into()),
                        // hash = hash XOR byte
                        Instr::LocalGet(1),
                        Instr::I32Xor,
                        // hash = hash * 16777619 (FNV prime)
                        Instr::I32Const(16777619),
                        Instr::I32Mul,
                        Instr::LocalSet(1),
                        Instr::LocalGet(3),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(3),
                        Instr::Br("loop".into()),
                    ],
                }],
            },
            Instr::LocalGet(1),
        ],
    }
}

// ── hash_key(key: anyref) -> i32 ─────────────────────────────────────────────
fn hash_key_fn() -> FuncDef {
    // p0=key
    FuncDef {
        name: "hash_key".into(),
        params: vec![ValType::Anyref],
        results: vec![ValType::I32],
        locals: vec![],
        body: vec![
            // if I31: hash_i64(i64.extend_i32_u(i31.get_u(key)))
            Instr::LocalGet(0),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::I31,
            },
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(0),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::I31,
                    },
                    Instr::I31GetU,
                    Instr::I64ExtendI32U,
                    Instr::Call("hash_i64".into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // if BoxedInt: hash_i64(struct.get field 0)
            Instr::LocalGet(0),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_BOXED_INT.into()),
            },
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(0),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_BOXED_INT.into()),
                    },
                    Instr::StructGet(T_BOXED_INT.into(), 0),
                    Instr::Call("hash_i64".into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // else: String
            Instr::LocalGet(0),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_STRING.into()),
            },
            Instr::Call("hash_string".into()),
        ],
    }
}

// ── collision_get(c, key) -> anyref ──────────────────────────────────────────
fn collision_get_fn() -> FuncDef {
    // p0=c (ref null $HamtCollision), p1=key; L2=entries, L3=n, L4=i, L5=entry
    FuncDef {
        name: "collision_get".into(),
        params: vec![ref_hamt_collision_null(), ValType::Anyref],
        results: vec![ValType::Anyref],
        locals: vec![
            ref_array_null(),
            ValType::I32,
            ValType::I32,
            ref_hamt_entry_null(),
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_HAMT_COLLISION.into(), HC_ENTRIES),
            Instr::LocalSet(2),
            Instr::LocalGet(2),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(3),
            Instr::I32Const(0),
            Instr::LocalSet(4),
            Instr::Block {
                label: "exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "scan".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(4),
                        Instr::LocalGet(3),
                        Instr::I32GeS,
                        Instr::BrIf("exit".into()),
                        Instr::LocalGet(2),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(4),
                        Instr::ArrayGet(T_ARRAY.into()),
                        Instr::RefCast {
                            nullable: false,
                            heap: HeapType::Named(T_HAMT_ENTRY.into()),
                        },
                        Instr::LocalSet(5),
                        Instr::LocalGet(5),
                        Instr::StructGet(T_HAMT_ENTRY.into(), HE_KEY),
                        Instr::LocalGet(1),
                        Instr::Call("core_eq".into()),
                        Instr::If {
                            result: None,
                            then_body: vec![
                                Instr::LocalGet(5),
                                Instr::StructGet(T_HAMT_ENTRY.into(), HE_VAL),
                                Instr::Return,
                            ],
                            else_body: vec![],
                        },
                        Instr::LocalGet(4),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(4),
                        Instr::Br("scan".into()),
                    ],
                }],
            },
            Instr::RefNull(HeapType::Any),
        ],
    }
}

// ── collision_set(c, hash, key, val) -> ref $HamtCollision ───────────────────
fn collision_set_fn() -> FuncDef {
    // p0=c, p1=hash, p2=key, p3=val; L4=entries, L5=n, L6=i, L7=entry, L8=new_entries
    FuncDef {
        name: "collision_set".into(),
        params: vec![
            ref_hamt_collision_null(),
            ValType::I32,
            ValType::Anyref,
            ValType::Anyref,
        ],
        results: vec![ref_hamt_collision()],
        locals: vec![
            ref_array_null(),
            ValType::I32,
            ValType::I32,
            ref_hamt_entry_null(),
            ref_array_null(),
        ],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_HAMT_COLLISION.into(), HC_ENTRIES),
            Instr::LocalSet(4),
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(5),
            Instr::I32Const(0),
            Instr::LocalSet(6),
            Instr::Block {
                label: "found_exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "scan".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(6),
                        Instr::LocalGet(5),
                        Instr::I32GeS,
                        Instr::BrIf("found_exit".into()),
                        Instr::LocalGet(4),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(6),
                        Instr::ArrayGet(T_ARRAY.into()),
                        Instr::RefCast {
                            nullable: false,
                            heap: HeapType::Named(T_HAMT_ENTRY.into()),
                        },
                        Instr::LocalSet(7),
                        Instr::LocalGet(7),
                        Instr::StructGet(T_HAMT_ENTRY.into(), HE_KEY),
                        Instr::LocalGet(2),
                        Instr::Call("core_eq".into()),
                        Instr::If {
                            result: None,
                            then_body: vec![
                                // Replace entry at i
                                Instr::LocalGet(1),
                                Instr::LocalGet(2),
                                Instr::LocalGet(3),
                                Instr::StructNew(T_HAMT_ENTRY.into()), // new_entry
                                Instr::LocalSet(7),
                                Instr::LocalGet(4),
                                Instr::RefAsNonNull,
                                Instr::LocalGet(6),
                                Instr::LocalGet(7),
                                Instr::Call("arr_replace_at".into()),
                                Instr::LocalSet(8),
                                Instr::I32Const(0),
                                Instr::LocalGet(0),
                                Instr::RefAsNonNull,
                                Instr::StructGet(T_HAMT_COLLISION.into(), HC_HASH),
                                Instr::LocalGet(8),
                                Instr::RefAsNonNull,
                                Instr::StructNew(T_HAMT_COLLISION.into()),
                                Instr::Return,
                            ],
                            else_body: vec![],
                        },
                        Instr::LocalGet(6),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(6),
                        Instr::Br("scan".into()),
                    ],
                }],
            },
            // Not found: append new entry
            Instr::LocalGet(1),
            Instr::LocalGet(2),
            Instr::LocalGet(3),
            Instr::StructNew(T_HAMT_ENTRY.into()), // new_entry on stack
            Instr::LocalSet(7),
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
            Instr::LocalGet(5),
            Instr::LocalGet(7),
            Instr::Call("arr_insert_at".into()),
            Instr::LocalSet(8),
            Instr::I32Const(0),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_HAMT_COLLISION.into(), HC_HASH),
            Instr::LocalGet(8),
            Instr::RefAsNonNull,
            Instr::StructNew(T_HAMT_COLLISION.into()),
        ],
    }
}

// ── node_get(node, hash, depth, key) -> anyref ───────────────────────────────
fn node_get_fn() -> FuncDef {
    // p0=node, p1=hash, p2=depth, p3=key
    // L4=fragment, L5=bit, L6=bitmap, L7=idx, L8=slot, L9=entries
    FuncDef {
        name: "node_get".into(),
        params: vec![
            ref_hamt_node_null(),
            ValType::I32,
            ValType::I32,
            ValType::Anyref,
        ],
        results: vec![ValType::Anyref],
        locals: vec![
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::Anyref,
            ref_array_null(),
        ],
        body: vec![
            // if node is null: return null
            Instr::LocalGet(0),
            Instr::RefIsNull,
            Instr::If {
                result: None,
                then_body: vec![Instr::RefNull(HeapType::Any), Instr::Return],
                else_body: vec![],
            },
            // Stop before shift counts wrap past the 32-bit hash space.
            Instr::LocalGet(2),
            Instr::I32Const(7),
            Instr::I32GeU,
            Instr::If {
                result: None,
                then_body: vec![Instr::RefNull(HeapType::Any), Instr::Return],
                else_body: vec![],
            },
            // fragment = (hash >> (depth*5)) & 31
            Instr::LocalGet(1), // hash
            Instr::LocalGet(2),
            Instr::I32Const(5),
            Instr::I32Mul, // shift
            Instr::I32ShrU,
            Instr::I32Const(31),
            Instr::I32And,
            Instr::LocalSet(4),
            // bit = 1 << fragment
            Instr::I32Const(1),
            Instr::LocalGet(4),
            Instr::I32Shl,
            Instr::LocalSet(5),
            // bitmap = node.bitmap
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_HAMT_NODE.into(), HN_BITMAP),
            Instr::LocalSet(6),
            // if (bitmap & bit) == 0: not found
            Instr::LocalGet(6),
            Instr::LocalGet(5),
            Instr::I32And,
            Instr::I32Eqz,
            Instr::If {
                result: None,
                then_body: vec![Instr::RefNull(HeapType::Any), Instr::Return],
                else_body: vec![],
            },
            // idx = popcount(bitmap & (bit - 1))
            Instr::LocalGet(6),
            Instr::LocalGet(5),
            Instr::I32Const(1),
            Instr::I32Sub,
            Instr::I32And,
            Instr::Call("popcount".into()),
            Instr::LocalSet(7),
            // slot = node.entries[idx]
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_HAMT_NODE.into(), HN_ENTRIES),
            Instr::LocalSet(9),
            Instr::LocalGet(7),
            Instr::LocalGet(9),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::I32GeU,
            Instr::If {
                result: None,
                then_body: vec![Instr::RefNull(HeapType::Any), Instr::Return],
                else_body: vec![],
            },
            Instr::LocalGet(9),
            Instr::RefAsNonNull,
            Instr::LocalGet(7),
            Instr::ArrayGet(T_ARRAY.into()),
            Instr::LocalSet(8),
            // if HamtEntry: check hash+key
            Instr::LocalGet(8),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_HAMT_ENTRY.into()),
            },
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(8),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_HAMT_ENTRY.into()),
                    },
                    Instr::StructGet(T_HAMT_ENTRY.into(), HE_HASH),
                    Instr::LocalGet(1),
                    Instr::I32Eq,
                    Instr::If {
                        result: None,
                        then_body: vec![
                            // re-cast slot to HamtEntry for key comparison
                            Instr::LocalGet(8),
                            Instr::RefCast {
                                nullable: false,
                                heap: HeapType::Named(T_HAMT_ENTRY.into()),
                            },
                            Instr::StructGet(T_HAMT_ENTRY.into(), HE_KEY),
                            Instr::LocalGet(3),
                            Instr::Call("core_eq".into()),
                            Instr::If {
                                result: None,
                                then_body: vec![
                                    Instr::LocalGet(8),
                                    Instr::RefCast {
                                        nullable: false,
                                        heap: HeapType::Named(T_HAMT_ENTRY.into()),
                                    },
                                    Instr::StructGet(T_HAMT_ENTRY.into(), HE_VAL),
                                    Instr::Return,
                                ],
                                else_body: vec![],
                            },
                        ],
                        else_body: vec![],
                    },
                    Instr::RefNull(HeapType::Any),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // if HamtNode: recurse
            Instr::LocalGet(8),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_HAMT_NODE.into()),
            },
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(8),
                    Instr::RefCast {
                        nullable: true,
                        heap: HeapType::Named(T_HAMT_NODE.into()),
                    },
                    Instr::LocalGet(1),
                    Instr::LocalGet(2),
                    Instr::I32Const(1),
                    Instr::I32Add,
                    Instr::LocalGet(3),
                    Instr::Call("node_get".into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // else HamtCollision
            Instr::LocalGet(8),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_HAMT_COLLISION.into()),
            },
            Instr::LocalGet(3),
            Instr::Call("collision_get".into()),
        ],
    }
}

// ── node_set(node, hash, depth, key, val) -> ref $HamtNode ───────────────────
fn node_set_fn() -> FuncDef {
    // p0=node, p1=hash, p2=depth, p3=key, p4=val
    // L5=new_entry, L6=shift, L7=fragment, L8=bit, L9=bitmap, L10=idx
    // L11=slot, L12=new_entries, L13=old_entry, L14=tmp_node, L15=collision, L16=new_collision
    FuncDef {
        name: "node_set".into(),
        params: vec![
            ref_hamt_node_null(),
            ValType::I32,
            ValType::I32,
            ValType::Anyref,
            ValType::Anyref,
        ],
        results: vec![ref_hamt_node()],
        locals: vec![
            ref_hamt_entry_null(),
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::Anyref,
            ref_array_null(),
            ref_hamt_entry_null(),
            ref_hamt_node_null(),
            ref_hamt_collision_null(),
            ref_hamt_collision_null(),
        ],
        body: vec![
            // new_entry = HamtEntry { hash, key, val }
            Instr::LocalGet(1),
            Instr::LocalGet(3),
            Instr::LocalGet(4),
            Instr::StructNew(T_HAMT_ENTRY.into()),
            Instr::LocalSet(5),
            // === null node: create single-entry node ===
            Instr::LocalGet(0),
            Instr::RefIsNull,
            Instr::If {
                result: None,
                then_body: vec![
                    // fragment = (hash >> (depth*5)) & 31
                    Instr::LocalGet(1),
                    Instr::LocalGet(2),
                    Instr::I32Const(5),
                    Instr::I32Mul,
                    Instr::I32ShrU,
                    Instr::I32Const(31),
                    Instr::I32And,
                    Instr::LocalSet(7),
                    // bit = 1 << fragment
                    Instr::I32Const(1),
                    Instr::LocalGet(7),
                    Instr::I32Shl,
                    Instr::LocalSet(8),
                    // entries = [new_entry]; return HamtNode { bit, entries }
                    Instr::LocalGet(8),
                    Instr::LocalGet(5),
                    Instr::ArrayNewFixed(T_ARRAY.into(), 1),
                    Instr::StructNew(T_HAMT_NODE.into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // shift = depth * 5
            Instr::LocalGet(2),
            Instr::I32Const(5),
            Instr::I32Mul,
            Instr::LocalSet(6),
            // fragment = (hash >> shift) & 31
            Instr::LocalGet(1),
            Instr::LocalGet(6),
            Instr::I32ShrU,
            Instr::I32Const(31),
            Instr::I32And,
            Instr::LocalSet(7),
            // bit = 1 << fragment
            Instr::I32Const(1),
            Instr::LocalGet(7),
            Instr::I32Shl,
            Instr::LocalSet(8),
            // bitmap = node.bitmap
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_HAMT_NODE.into(), HN_BITMAP),
            Instr::LocalSet(9),
            // idx = popcount(bitmap & (bit - 1))
            Instr::LocalGet(9),
            Instr::LocalGet(8),
            Instr::I32Const(1),
            Instr::I32Sub,
            Instr::I32And,
            Instr::Call("popcount".into()),
            Instr::LocalSet(10),
            // === no existing slot: insert ===
            Instr::LocalGet(9),
            Instr::LocalGet(8),
            Instr::I32And,
            Instr::I32Eqz,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_HAMT_NODE.into(), HN_ENTRIES),
                    Instr::LocalGet(10),
                    Instr::LocalGet(5),
                    Instr::Call("arr_insert_at".into()),
                    Instr::LocalSet(12),
                    Instr::LocalGet(9),
                    Instr::LocalGet(8),
                    Instr::I32Or,
                    Instr::LocalGet(12),
                    Instr::RefAsNonNull,
                    Instr::StructNew(T_HAMT_NODE.into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // slot = node.entries[idx]
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_HAMT_NODE.into(), HN_ENTRIES),
            Instr::LocalSet(12),
            Instr::LocalGet(10),
            Instr::LocalGet(12),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::I32GeU,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(12),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(12),
                    Instr::RefAsNonNull,
                    Instr::ArrayLen,
                    Instr::LocalGet(5),
                    Instr::Call("arr_insert_at".into()),
                    Instr::LocalSet(12),
                    Instr::LocalGet(9),
                    Instr::LocalGet(8),
                    Instr::I32Or,
                    Instr::LocalGet(12),
                    Instr::RefAsNonNull,
                    Instr::StructNew(T_HAMT_NODE.into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            Instr::LocalGet(12),
            Instr::RefAsNonNull,
            Instr::LocalGet(10),
            Instr::ArrayGet(T_ARRAY.into()),
            Instr::LocalSet(11),
            // === HamtEntry slot ===
            Instr::LocalGet(11),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_HAMT_ENTRY.into()),
            },
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(11),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_HAMT_ENTRY.into()),
                    },
                    Instr::LocalSet(13),
                    // if old.hash == hash
                    Instr::LocalGet(13),
                    Instr::StructGet(T_HAMT_ENTRY.into(), HE_HASH),
                    Instr::LocalGet(1),
                    Instr::I32Eq,
                    Instr::If {
                        result: None,
                        then_body: vec![
                            // if eq(old.key, key): update
                            Instr::LocalGet(13),
                            Instr::StructGet(T_HAMT_ENTRY.into(), HE_KEY),
                            Instr::LocalGet(3),
                            Instr::Call("core_eq".into()),
                            Instr::If {
                                result: None,
                                then_body: vec![
                                    Instr::LocalGet(0),
                                    Instr::RefAsNonNull,
                                    Instr::StructGet(T_HAMT_NODE.into(), HN_ENTRIES),
                                    Instr::LocalGet(10),
                                    Instr::LocalGet(5),
                                    Instr::Call("arr_replace_at".into()),
                                    Instr::LocalSet(12),
                                    Instr::LocalGet(9),
                                    Instr::LocalGet(12),
                                    Instr::RefAsNonNull,
                                    Instr::StructNew(T_HAMT_NODE.into()),
                                    Instr::Return,
                                ],
                                else_body: vec![
                                    // same hash, different key: collision
                                    Instr::LocalGet(13), // old_entry
                                    Instr::LocalGet(5),  // new_entry
                                    Instr::ArrayNewFixed(T_ARRAY.into(), 2),
                                    Instr::LocalSet(12),
                                    Instr::I32Const(0),
                                    Instr::LocalGet(1), // hash
                                    Instr::LocalGet(12),
                                    Instr::RefAsNonNull,
                                    Instr::StructNew(T_HAMT_COLLISION.into()), // collision
                                    Instr::LocalSet(15),
                                    Instr::LocalGet(0),
                                    Instr::RefAsNonNull,
                                    Instr::StructGet(T_HAMT_NODE.into(), HN_ENTRIES),
                                    Instr::LocalGet(10),
                                    Instr::LocalGet(15),
                                    Instr::Call("arr_replace_at".into()),
                                    Instr::LocalSet(12),
                                    Instr::LocalGet(9),
                                    Instr::LocalGet(12),
                                    Instr::RefAsNonNull,
                                    Instr::StructNew(T_HAMT_NODE.into()),
                                    Instr::Return,
                                ],
                            },
                        ],
                        else_body: vec![
                            // different hash: create sub-node for old entry, then insert new,
                            // unless all 32 hash bits are already consumed.
                            Instr::LocalGet(2),
                            Instr::I32Const(6),
                            Instr::I32GeU,
                            Instr::If {
                                result: None,
                                then_body: vec![
                                    Instr::LocalGet(13),
                                    Instr::LocalGet(5),
                                    Instr::ArrayNewFixed(T_ARRAY.into(), 2),
                                    Instr::LocalSet(12),
                                    Instr::I32Const(0),
                                    Instr::LocalGet(1),
                                    Instr::LocalGet(12),
                                    Instr::RefAsNonNull,
                                    Instr::StructNew(T_HAMT_COLLISION.into()),
                                    Instr::LocalSet(15),
                                    Instr::LocalGet(0),
                                    Instr::RefAsNonNull,
                                    Instr::StructGet(T_HAMT_NODE.into(), HN_ENTRIES),
                                    Instr::LocalGet(10),
                                    Instr::LocalGet(15),
                                    Instr::Call("arr_replace_at".into()),
                                    Instr::LocalSet(12),
                                    Instr::LocalGet(9),
                                    Instr::LocalGet(12),
                                    Instr::RefAsNonNull,
                                    Instr::StructNew(T_HAMT_NODE.into()),
                                    Instr::Return,
                                ],
                                else_body: vec![],
                            },
                            Instr::RefNull(HeapType::Named(T_HAMT_NODE.into())),
                            Instr::LocalGet(13),
                            Instr::StructGet(T_HAMT_ENTRY.into(), HE_HASH),
                            Instr::LocalGet(2),
                            Instr::I32Const(1),
                            Instr::I32Add,
                            Instr::LocalGet(13),
                            Instr::StructGet(T_HAMT_ENTRY.into(), HE_KEY),
                            Instr::LocalGet(13),
                            Instr::StructGet(T_HAMT_ENTRY.into(), HE_VAL),
                            Instr::Call("node_set".into()),
                            Instr::LocalSet(14),
                            Instr::LocalGet(14),
                            Instr::LocalGet(1),
                            Instr::LocalGet(2),
                            Instr::I32Const(1),
                            Instr::I32Add,
                            Instr::LocalGet(3),
                            Instr::LocalGet(4),
                            Instr::Call("node_set".into()),
                            Instr::LocalSet(14),
                            Instr::LocalGet(0),
                            Instr::RefAsNonNull,
                            Instr::StructGet(T_HAMT_NODE.into(), HN_ENTRIES),
                            Instr::LocalGet(10),
                            Instr::LocalGet(14),
                            Instr::Call("arr_replace_at".into()),
                            Instr::LocalSet(12),
                            Instr::LocalGet(9),
                            Instr::LocalGet(12),
                            Instr::RefAsNonNull,
                            Instr::StructNew(T_HAMT_NODE.into()),
                            Instr::Return,
                        ],
                    },
                ],
                else_body: vec![],
            },
            // === HamtNode slot: recurse ===
            Instr::LocalGet(11),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_HAMT_NODE.into()),
            },
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(11),
                    Instr::RefCast {
                        nullable: true,
                        heap: HeapType::Named(T_HAMT_NODE.into()),
                    },
                    Instr::LocalGet(1),
                    Instr::LocalGet(2),
                    Instr::I32Const(1),
                    Instr::I32Add,
                    Instr::LocalGet(3),
                    Instr::LocalGet(4),
                    Instr::Call("node_set".into()),
                    Instr::LocalSet(14),
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_HAMT_NODE.into(), HN_ENTRIES),
                    Instr::LocalGet(10),
                    Instr::LocalGet(14),
                    Instr::Call("arr_replace_at".into()),
                    Instr::LocalSet(12),
                    Instr::LocalGet(9),
                    Instr::LocalGet(12),
                    Instr::RefAsNonNull,
                    Instr::StructNew(T_HAMT_NODE.into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // === HamtCollision slot ===
            Instr::LocalGet(11),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_HAMT_COLLISION.into()),
            },
            Instr::LocalSet(15),
            Instr::LocalGet(15),
            Instr::LocalGet(1),
            Instr::LocalGet(3),
            Instr::LocalGet(4),
            Instr::Call("collision_set".into()),
            Instr::LocalSet(16),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_HAMT_NODE.into(), HN_ENTRIES),
            Instr::LocalGet(10),
            Instr::LocalGet(16),
            Instr::Call("arr_replace_at".into()),
            Instr::LocalSet(12),
            Instr::LocalGet(9),
            Instr::LocalGet(12),
            Instr::RefAsNonNull,
            Instr::StructNew(T_HAMT_NODE.into()),
        ],
    }
}

// ── node_remove(node, hash, depth, key) -> ref null $HamtNode ────────────────
fn node_remove_fn() -> FuncDef {
    // p0=node, p1=hash, p2=depth, p3=key
    // L4=fragment, L5=bit, L6=bitmap, L7=idx, L8=slot, L9=entries, L10=n_entries
    // L11=old_entry, L12=new_entries, L13=sub, L14=new_sub
    // L15=collision, L16=c_entries, L17=c_n, L18=found_j, L19=j, L20=ce, L21=new_c
    FuncDef {
        name: "node_remove".into(),
        params: vec![
            ref_hamt_node_null(),
            ValType::I32,
            ValType::I32,
            ValType::Anyref,
        ],
        results: vec![ref_hamt_node_null()],
        locals: vec![
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ValType::Anyref,
            ref_array_null(),
            ValType::I32,
            ref_hamt_entry_null(),
            ref_array_null(),
            ref_hamt_node_null(),
            ref_hamt_node_null(),
            ref_hamt_collision_null(),
            ref_array_null(),
            ValType::I32,
            ValType::I32,
            ValType::I32,
            ref_hamt_entry_null(),
            ref_hamt_collision_null(),
        ],
        body: vec![
            // if null: return null
            Instr::LocalGet(0),
            Instr::RefIsNull,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::RefNull(HeapType::Named(T_HAMT_NODE.into())),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // fragment = (hash >> (depth*5)) & 31
            Instr::LocalGet(1),
            Instr::LocalGet(2),
            Instr::I32Const(5),
            Instr::I32Mul,
            Instr::I32ShrU,
            Instr::I32Const(31),
            Instr::I32And,
            Instr::LocalSet(4),
            // bit = 1 << fragment
            Instr::I32Const(1),
            Instr::LocalGet(4),
            Instr::I32Shl,
            Instr::LocalSet(5),
            // bitmap = node.bitmap
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_HAMT_NODE.into(), HN_BITMAP),
            Instr::LocalSet(6),
            // if (bitmap & bit) == 0: key not here
            Instr::LocalGet(6),
            Instr::LocalGet(5),
            Instr::I32And,
            Instr::I32Eqz,
            Instr::If {
                result: None,
                then_body: vec![Instr::LocalGet(0), Instr::Return],
                else_body: vec![],
            },
            // idx = popcount(bitmap & (bit-1))
            Instr::LocalGet(6),
            Instr::LocalGet(5),
            Instr::I32Const(1),
            Instr::I32Sub,
            Instr::I32And,
            Instr::Call("popcount".into()),
            Instr::LocalSet(7),
            // slot = node.entries[idx]
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_HAMT_NODE.into(), HN_ENTRIES),
            Instr::LocalSet(9),
            Instr::LocalGet(9),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(10),
            Instr::LocalGet(9),
            Instr::RefAsNonNull,
            Instr::LocalGet(7),
            Instr::ArrayGet(T_ARRAY.into()),
            Instr::LocalSet(8),
            // === HamtEntry slot ===
            Instr::LocalGet(8),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_HAMT_ENTRY.into()),
            },
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(8),
                    Instr::RefCast {
                        nullable: false,
                        heap: HeapType::Named(T_HAMT_ENTRY.into()),
                    },
                    Instr::LocalSet(11),
                    // if old.hash != hash || !eq(old.key, key): return node
                    Instr::LocalGet(11),
                    Instr::StructGet(T_HAMT_ENTRY.into(), HE_HASH),
                    Instr::LocalGet(1),
                    Instr::I32Eq,
                    Instr::If {
                        result: None,
                        then_body: vec![
                            Instr::LocalGet(11),
                            Instr::StructGet(T_HAMT_ENTRY.into(), HE_KEY),
                            Instr::LocalGet(3),
                            Instr::Call("core_eq".into()),
                            Instr::If {
                                result: None,
                                then_body: vec![
                                    // Remove this entry
                                    Instr::LocalGet(10),
                                    Instr::I32Const(1),
                                    Instr::I32Eq,
                                    Instr::If {
                                        result: None,
                                        then_body: vec![
                                            Instr::RefNull(HeapType::Named(T_HAMT_NODE.into())),
                                            Instr::Return,
                                        ],
                                        else_body: vec![],
                                    },
                                    Instr::LocalGet(9),
                                    Instr::RefAsNonNull,
                                    Instr::LocalGet(7),
                                    Instr::Call("arr_remove_at".into()),
                                    Instr::LocalSet(12),
                                    // bitmap - bit (clears the bit since it was set)
                                    Instr::LocalGet(6),
                                    Instr::LocalGet(5),
                                    Instr::I32Sub,
                                    Instr::LocalGet(12),
                                    Instr::RefAsNonNull,
                                    Instr::StructNew(T_HAMT_NODE.into()),
                                    Instr::Return,
                                ],
                                else_body: vec![Instr::LocalGet(0), Instr::Return],
                            },
                        ],
                        else_body: vec![Instr::LocalGet(0), Instr::Return],
                    },
                ],
                else_body: vec![],
            },
            // === HamtNode slot: recurse ===
            Instr::LocalGet(8),
            Instr::RefTest {
                nullable: false,
                heap: HeapType::Named(T_HAMT_NODE.into()),
            },
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(8),
                    Instr::RefCast {
                        nullable: true,
                        heap: HeapType::Named(T_HAMT_NODE.into()),
                    },
                    Instr::LocalSet(13),
                    Instr::LocalGet(13),
                    Instr::LocalGet(1),
                    Instr::LocalGet(2),
                    Instr::I32Const(1),
                    Instr::I32Add,
                    Instr::LocalGet(3),
                    Instr::Call("node_remove".into()),
                    Instr::LocalSet(14),
                    // if new_sub is null:
                    Instr::LocalGet(14),
                    Instr::RefIsNull,
                    Instr::If {
                        result: None,
                        then_body: vec![
                            Instr::LocalGet(10),
                            Instr::I32Const(1),
                            Instr::I32Eq,
                            Instr::If {
                                result: None,
                                then_body: vec![
                                    Instr::RefNull(HeapType::Named(T_HAMT_NODE.into())),
                                    Instr::Return,
                                ],
                                else_body: vec![],
                            },
                            Instr::LocalGet(9),
                            Instr::RefAsNonNull,
                            Instr::LocalGet(7),
                            Instr::Call("arr_remove_at".into()),
                            Instr::LocalSet(12),
                            Instr::LocalGet(6),
                            Instr::LocalGet(5),
                            Instr::I32Sub,
                            Instr::LocalGet(12),
                            Instr::RefAsNonNull,
                            Instr::StructNew(T_HAMT_NODE.into()),
                            Instr::Return,
                        ],
                        else_body: vec![],
                    },
                    // new_sub is non-null: replace
                    Instr::LocalGet(9),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(7),
                    Instr::LocalGet(14),
                    Instr::Call("arr_replace_at".into()),
                    Instr::LocalSet(12),
                    Instr::LocalGet(6),
                    Instr::LocalGet(12),
                    Instr::RefAsNonNull,
                    Instr::StructNew(T_HAMT_NODE.into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // === HamtCollision slot ===
            Instr::LocalGet(8),
            Instr::RefCast {
                nullable: true,
                heap: HeapType::Named(T_HAMT_COLLISION.into()),
            },
            Instr::LocalSet(15),
            Instr::LocalGet(15),
            Instr::RefAsNonNull,
            Instr::StructGet(T_HAMT_COLLISION.into(), HC_ENTRIES),
            Instr::LocalSet(16),
            Instr::LocalGet(16),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::LocalSet(17),
            // scan for key; found_j = -1 initially
            Instr::I32Const(-1),
            Instr::LocalSet(18),
            Instr::I32Const(0),
            Instr::LocalSet(19),
            Instr::Block {
                label: "find_exit".into(),
                result: None,
                body: vec![Instr::Loop {
                    label: "find".into(),
                    result: None,
                    body: vec![
                        Instr::LocalGet(19),
                        Instr::LocalGet(17),
                        Instr::I32GeS,
                        Instr::BrIf("find_exit".into()),
                        Instr::LocalGet(16),
                        Instr::RefAsNonNull,
                        Instr::LocalGet(19),
                        Instr::ArrayGet(T_ARRAY.into()),
                        Instr::RefCast {
                            nullable: false,
                            heap: HeapType::Named(T_HAMT_ENTRY.into()),
                        },
                        Instr::LocalSet(20),
                        Instr::LocalGet(20),
                        Instr::StructGet(T_HAMT_ENTRY.into(), HE_KEY),
                        Instr::LocalGet(3),
                        Instr::Call("core_eq".into()),
                        Instr::If {
                            result: None,
                            then_body: vec![
                                Instr::LocalGet(19),
                                Instr::LocalSet(18),
                                Instr::Br("find_exit".into()),
                            ],
                            else_body: vec![],
                        },
                        Instr::LocalGet(19),
                        Instr::I32Const(1),
                        Instr::I32Add,
                        Instr::LocalSet(19),
                        Instr::Br("find".into()),
                    ],
                }],
            },
            // if found_j == -1: key not in collision, return unchanged
            Instr::LocalGet(18),
            Instr::I32Const(1),
            Instr::I32Add,
            Instr::I32Eqz,
            Instr::If {
                result: None,
                then_body: vec![Instr::LocalGet(0), Instr::Return],
                else_body: vec![],
            },
            // Remove found entry from collision
            Instr::LocalGet(16),
            Instr::RefAsNonNull,
            Instr::LocalGet(18),
            Instr::Call("arr_remove_at".into()),
            Instr::LocalSet(12),
            // if remaining == 1: inline the surviving entry
            Instr::LocalGet(12),
            Instr::RefAsNonNull,
            Instr::ArrayLen,
            Instr::I32Const(1),
            Instr::I32Eq,
            Instr::If {
                result: None,
                then_body: vec![
                    // surviving = new_c_entries[0]
                    Instr::LocalGet(12),
                    Instr::RefAsNonNull,
                    Instr::I32Const(0),
                    Instr::ArrayGet(T_ARRAY.into()), // HamtEntry as anyref
                    Instr::LocalSet(8),              // reuse slot
                    Instr::LocalGet(9),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(7),
                    Instr::LocalGet(8),
                    Instr::Call("arr_replace_at".into()),
                    Instr::LocalSet(12),
                    Instr::LocalGet(6),
                    Instr::LocalGet(12),
                    Instr::RefAsNonNull,
                    Instr::StructNew(T_HAMT_NODE.into()),
                    Instr::Return,
                ],
                else_body: vec![],
            },
            // else: rebuild collision
            Instr::I32Const(0),
            Instr::LocalGet(15),
            Instr::RefAsNonNull,
            Instr::StructGet(T_HAMT_COLLISION.into(), HC_HASH),
            Instr::LocalGet(12),
            Instr::RefAsNonNull,
            Instr::StructNew(T_HAMT_COLLISION.into()),
            Instr::LocalSet(21),
            Instr::LocalGet(9),
            Instr::RefAsNonNull,
            Instr::LocalGet(7),
            Instr::LocalGet(21),
            Instr::Call("arr_replace_at".into()),
            Instr::LocalSet(12),
            Instr::LocalGet(6),
            Instr::LocalGet(12),
            Instr::RefAsNonNull,
            Instr::StructNew(T_HAMT_NODE.into()),
        ],
    }
}

// ── order_remove_key(order, key) -> ref $PVec ────────────────────────────────
fn order_remove_key_fn() -> FuncDef {
    // p0=order, p1=key; L2=n, L3=i, L4=result, L5=k
    FuncDef {
        name: "order_remove_key".into(),
        params: vec![ref_pvec(), ValType::Anyref],
        results: vec![ref_pvec()],
        locals: vec![ValType::I32, ValType::I32, ref_pvec_null(), ValType::Anyref],
        body: vec![
            Instr::LocalGet(0),
            Instr::Call("arr_len".into()),
            Instr::LocalSet(2),
            // result = empty PVec
            Instr::I32Const(0),
            Instr::I32Const(0),
            Instr::RefNull(HeapType::Named(T_VEC_INTERNAL.into())),
            Instr::ArrayNewFixed(T_ARRAY.into(), 0),
            Instr::StructNew(T_PVEC.into()),
            Instr::LocalSet(4),
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
                        Instr::Call("arr_get".into()),
                        Instr::LocalSet(5),
                        Instr::LocalGet(5),
                        Instr::LocalGet(1),
                        Instr::Call("core_eq".into()),
                        Instr::I32Eqz,
                        Instr::If {
                            result: None,
                            then_body: vec![
                                Instr::LocalGet(4),
                                Instr::RefAsNonNull,
                                Instr::LocalGet(5),
                                Instr::Call("arr_push".into()),
                                Instr::LocalSet(4),
                            ],
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
            Instr::LocalGet(4),
            Instr::RefAsNonNull,
        ],
    }
}

// ── make() -> PDict ──────────────────────────────────────────────────────────
fn make_fn() -> FuncDef {
    FuncDef {
        name: "make".into(),
        params: vec![],
        results: vec![ref_pdict()],
        locals: vec![],
        body: vec![
            Instr::I32Const(0),                                  // size = 0
            Instr::RefNull(HeapType::Named(T_HAMT_NODE.into())), // root = null
            // order = empty PVec
            Instr::I32Const(0),
            Instr::I32Const(0),
            Instr::RefNull(HeapType::Named(T_VEC_INTERNAL.into())),
            Instr::ArrayNewFixed(T_ARRAY.into(), 0),
            Instr::StructNew(T_PVEC.into()),
            Instr::StructNew(T_PDICT.into()),
        ],
    }
}

// ── len(dict: PDict?) -> i32 ─────────────────────────────────────────────────
fn len_fn() -> FuncDef {
    FuncDef {
        name: "len".into(),
        params: vec![ref_pdict_null()],
        results: vec![ValType::I32],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PDICT.into(), PD_SIZE),
        ],
    }
}

// ── keys(dict: PDict?) -> PVec ───────────────────────────────────────────────
fn keys_fn() -> FuncDef {
    FuncDef {
        name: "keys".into(),
        params: vec![ref_pdict_null()],
        results: vec![ref_pvec()],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PDICT.into(), PD_ORDER),
        ],
    }
}

// ── has(dict: PDict?, key: anyref) -> i32 ────────────────────────────────────
fn has_fn() -> FuncDef {
    FuncDef {
        name: "has".into(),
        params: vec![ref_pdict_null(), ValType::Anyref],
        results: vec![ValType::I32],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PDICT.into(), PD_ROOT),
            Instr::LocalGet(1),
            Instr::Call("hash_key".into()),
            Instr::I32Const(0),
            Instr::LocalGet(1),
            Instr::Call("node_get".into()),
            Instr::RefIsNull,
            Instr::I32Eqz,
        ],
    }
}

// ── get(dict: PDict?, key: anyref) -> anyref ─────────────────────────────────
fn get_fn() -> FuncDef {
    FuncDef {
        name: "get".into(),
        params: vec![ref_pdict_null(), ValType::Anyref],
        results: vec![ValType::Anyref],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PDICT.into(), PD_ROOT),
            Instr::LocalGet(1),
            Instr::Call("hash_key".into()),
            Instr::I32Const(0),
            Instr::LocalGet(1),
            Instr::Call("node_get".into()),
        ],
    }
}

// ── get_option(dict: PDict?, key: anyref) -> Variant ─────────────────────────
fn get_option_fn() -> FuncDef {
    // L2=val
    FuncDef {
        name: "get_option".into(),
        params: vec![ref_pdict_null(), ValType::Anyref],
        results: vec![ref_variant()],
        locals: vec![ValType::Anyref],
        body: vec![
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PDICT.into(), PD_ROOT),
            Instr::LocalGet(1),
            Instr::Call("hash_key".into()),
            Instr::I32Const(0),
            Instr::LocalGet(1),
            Instr::Call("node_get".into()),
            Instr::LocalSet(2),
            Instr::LocalGet(2),
            Instr::RefIsNull,
            Instr::If {
                result: Some(ref_variant()),
                then_body: vec![
                    // None = Variant(0, 0, null_array)
                    Instr::I32Const(0),
                    Instr::I32Const(0),
                    Instr::RefNull(HeapType::Named(T_ARRAY.into())),
                    Instr::StructNew(T_VARIANT.into()),
                ],
                else_body: vec![
                    // Some(val) = Variant(0, 1, [val])
                    Instr::I32Const(0),
                    Instr::I32Const(1),
                    Instr::LocalGet(2),
                    Instr::ArrayNewFixed(T_ARRAY.into(), 1),
                    Instr::StructNew(T_VARIANT.into()),
                ],
            },
        ],
    }
}

// ── set(dict: PDict?, key: anyref, val: anyref) -> PDict ─────────────────────
fn set_fn() -> FuncDef {
    // p0=dict, p1=key, p2=val; L3=hash, L4=old_root, L5=new_root, L6=was_present, L7=new_order
    FuncDef {
        name: "set".into(),
        params: vec![ref_pdict_null(), ValType::Anyref, ValType::Anyref],
        results: vec![ref_pdict()],
        locals: vec![
            ValType::I32,
            ref_hamt_node_null(),
            ref_hamt_node_null(),
            ValType::I32,
            ref_pvec_null(),
        ],
        body: vec![
            Instr::LocalGet(1),
            Instr::Call("hash_key".into()),
            Instr::LocalSet(3),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PDICT.into(), PD_ROOT),
            Instr::LocalSet(4),
            // was_present = node_get(root, hash, 0, key) != null
            Instr::LocalGet(4),
            Instr::LocalGet(3),
            Instr::I32Const(0),
            Instr::LocalGet(1),
            Instr::Call("node_get".into()),
            Instr::RefIsNull,
            Instr::I32Eqz,
            Instr::LocalSet(6),
            // new_root = node_set(root, hash, 0, key, val)
            Instr::LocalGet(4),
            Instr::LocalGet(3),
            Instr::I32Const(0),
            Instr::LocalGet(1),
            Instr::LocalGet(2),
            Instr::Call("node_set".into()),
            Instr::LocalSet(5),
            // new_order: append key only if was not present
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PDICT.into(), PD_ORDER),
            Instr::LocalSet(7),
            Instr::LocalGet(6),
            Instr::I32Eqz,
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(7),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(1),
                    Instr::Call("arr_push".into()),
                    Instr::LocalSet(7),
                ],
                else_body: vec![],
            },
            // new_size = was_present ? size : size + 1
            // Equivalent to: size + (1 - was_present) = size + eqz(was_present)
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PDICT.into(), PD_SIZE),
            Instr::LocalGet(6),
            Instr::I32Eqz,
            Instr::I32Add,
            Instr::LocalGet(5),
            Instr::LocalGet(7),
            Instr::RefAsNonNull,
            Instr::StructNew(T_PDICT.into()),
        ],
    }
}

// ── remove(dict: PDict?, key: anyref) -> PDict ───────────────────────────────
fn remove_fn() -> FuncDef {
    // p0=dict, p1=key; L2=hash, L3=old_root, L4=new_root, L5=was_present, L6=new_order
    FuncDef {
        name: "remove".into(),
        params: vec![ref_pdict_null(), ValType::Anyref],
        results: vec![ref_pdict()],
        locals: vec![
            ValType::I32,
            ref_hamt_node_null(),
            ref_hamt_node_null(),
            ValType::I32,
            ref_pvec_null(),
        ],
        body: vec![
            Instr::LocalGet(1),
            Instr::Call("hash_key".into()),
            Instr::LocalSet(2),
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PDICT.into(), PD_ROOT),
            Instr::LocalSet(3),
            // was_present = node_get(root, hash, 0, key) != null
            Instr::LocalGet(3),
            Instr::LocalGet(2),
            Instr::I32Const(0),
            Instr::LocalGet(1),
            Instr::Call("node_get".into()),
            Instr::RefIsNull,
            Instr::I32Eqz,
            Instr::LocalSet(5),
            // new_root = node_remove(root, hash, 0, key)
            Instr::LocalGet(3),
            Instr::LocalGet(2),
            Instr::I32Const(0),
            Instr::LocalGet(1),
            Instr::Call("node_remove".into()),
            Instr::LocalSet(4),
            // new_order: remove key only if was present
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PDICT.into(), PD_ORDER),
            Instr::LocalSet(6),
            Instr::LocalGet(5),
            Instr::If {
                result: None,
                then_body: vec![
                    Instr::LocalGet(6),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(1),
                    Instr::Call("order_remove_key".into()),
                    Instr::LocalSet(6),
                ],
                else_body: vec![],
            },
            // new_size = was_present ? size - 1 : size
            // Equivalent to: size - was_present
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::StructGet(T_PDICT.into(), PD_SIZE),
            Instr::LocalGet(5),
            Instr::I32Sub,
            Instr::LocalGet(4),
            Instr::LocalGet(6),
            Instr::RefAsNonNull,
            Instr::StructNew(T_PDICT.into()),
        ],
    }
}

// ── set_in_place / remove_in_place: alias persistent versions for v1 ─────────
fn set_in_place_fn() -> FuncDef {
    FuncDef {
        name: "set_in_place".into(),
        params: vec![ref_pdict_null(), ValType::Anyref, ValType::Anyref],
        results: vec![ref_pdict()],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::LocalGet(1),
            Instr::LocalGet(2),
            Instr::Call("set".into()),
        ],
    }
}

fn remove_in_place_fn() -> FuncDef {
    FuncDef {
        name: "remove_in_place".into(),
        params: vec![ref_pdict_null(), ValType::Anyref],
        results: vec![ref_pdict()],
        locals: vec![],
        body: vec![
            Instr::LocalGet(0),
            Instr::LocalGet(1),
            Instr::Call("remove".into()),
        ],
    }
}
