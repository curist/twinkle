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

// ── Inline wyhash v3 helpers ─────────────────────────────────────────────────
// These return instruction vectors for inlining into hash_i64/hash_string,
// eliminating call overhead for wymix/wyr4/wyr8.

/// Inline wymix: pops (a, b) from Wasm stack, pushes (lo ^ hi).
/// 4 i64.mul — reconstructs low half from partial products instead of a 5th mul.
fn wymix_instrs(
    la: u32,
    lb: u32,
    la_lo: u32,
    la_hi: u32,
    lb_lo: u32,
    lb_hi: u32,
    lt1: u32,
    lt2: u32,
    lcross: u32,
) -> Vec<Instr> {
    vec![
        Instr::LocalSet(lb),
        Instr::LocalSet(la),
        Instr::LocalGet(la),
        Instr::I64Const(4294967295),
        Instr::I64And,
        Instr::LocalSet(la_lo),
        Instr::LocalGet(la),
        Instr::I64Const(32),
        Instr::I64ShrU,
        Instr::LocalSet(la_hi),
        Instr::LocalGet(lb),
        Instr::I64Const(4294967295),
        Instr::I64And,
        Instr::LocalSet(lb_lo),
        Instr::LocalGet(lb),
        Instr::I64Const(32),
        Instr::I64ShrU,
        Instr::LocalSet(lb_hi),
        Instr::LocalGet(la_lo),
        Instr::LocalGet(lb_hi),
        Instr::I64Mul,
        Instr::LocalSet(lt1),
        Instr::LocalGet(la_hi),
        Instr::LocalGet(lb_lo),
        Instr::I64Mul,
        Instr::LocalSet(lt2),
        Instr::LocalGet(la_lo),
        Instr::LocalGet(lb_lo),
        Instr::I64Mul,
        Instr::LocalSet(la),
        Instr::LocalGet(la),
        Instr::I64Const(32),
        Instr::I64ShrU,
        Instr::LocalGet(lt1),
        Instr::I64Const(4294967295),
        Instr::I64And,
        Instr::I64Add,
        Instr::LocalGet(lt2),
        Instr::I64Const(4294967295),
        Instr::I64And,
        Instr::I64Add,
        Instr::LocalSet(lcross),
        Instr::LocalGet(la_hi),
        Instr::LocalGet(lb_hi),
        Instr::I64Mul,
        Instr::LocalGet(lt1),
        Instr::I64Const(32),
        Instr::I64ShrU,
        Instr::I64Add,
        Instr::LocalGet(lt2),
        Instr::I64Const(32),
        Instr::I64ShrU,
        Instr::I64Add,
        Instr::LocalGet(lcross),
        Instr::I64Const(32),
        Instr::I64ShrU,
        Instr::I64Add,
        Instr::LocalGet(la),
        Instr::I64Const(4294967295),
        Instr::I64And,
        Instr::LocalGet(lcross),
        Instr::I64Const(4294967295),
        Instr::I64And,
        Instr::I64Const(32),
        Instr::I64Shl,
        Instr::I64Or,
        Instr::I64Xor,
    ]
}

/// Inline wyr4: reads 4 bytes LE as u64. str_local/off_local must be locals.
fn wyr4_instrs(str_local: u32, off_local: u32) -> Vec<Instr> {
    vec![
        Instr::LocalGet(str_local),
        Instr::RefAsNonNull,
        Instr::LocalGet(off_local),
        Instr::ArrayGetU(T_STRING.into()),
        Instr::I64ExtendI32U,
        Instr::LocalGet(str_local),
        Instr::RefAsNonNull,
        Instr::LocalGet(off_local),
        Instr::I32Const(1),
        Instr::I32Add,
        Instr::ArrayGetU(T_STRING.into()),
        Instr::I64ExtendI32U,
        Instr::I64Const(8),
        Instr::I64Shl,
        Instr::I64Or,
        Instr::LocalGet(str_local),
        Instr::RefAsNonNull,
        Instr::LocalGet(off_local),
        Instr::I32Const(2),
        Instr::I32Add,
        Instr::ArrayGetU(T_STRING.into()),
        Instr::I64ExtendI32U,
        Instr::I64Const(16),
        Instr::I64Shl,
        Instr::I64Or,
        Instr::LocalGet(str_local),
        Instr::RefAsNonNull,
        Instr::LocalGet(off_local),
        Instr::I32Const(3),
        Instr::I32Add,
        Instr::ArrayGetU(T_STRING.into()),
        Instr::I64ExtendI32U,
        Instr::I64Const(24),
        Instr::I64Shl,
        Instr::I64Or,
    ]
}

/// Inline wyr8: reads 8 bytes LE as u64. off_scratch holds base offset; clobbered.
fn wyr8_instrs(str_local: u32, off_scratch: u32) -> Vec<Instr> {
    let mut v = wyr4_instrs(str_local, off_scratch);
    v.extend_from_slice(&[
        Instr::LocalGet(off_scratch),
        Instr::I32Const(4),
        Instr::I32Add,
        Instr::LocalSet(off_scratch),
    ]);
    v.extend(wyr4_instrs(str_local, off_scratch));
    v.extend_from_slice(&[Instr::I64Const(32), Instr::I64Shl, Instr::I64Or]);
    v
}

// ── hash_i64(v: i64) -> i64 ──────────────────────────────────────────────────
// wyhash v3 for an 8-byte LE value. seed=0. Fully inlined.
fn hash_i64_fn() -> FuncDef {
    // L0=v(param), L1-L9=wymix scratch
    let mix = wymix_instrs(1, 2, 3, 4, 5, 6, 7, 8, 9);
    let mut body = vec![
        // outer wymix arg1: secret[1] ^ len(=8)
        Instr::I64Const(-1800455987208640293i64),
        Instr::I64Const(8),
        Instr::I64Xor,
        // a = swap32(v) = (v << 32) | (v >>u 32)
        Instr::LocalGet(0),
        Instr::I64Const(32),
        Instr::I64Shl,
        Instr::LocalGet(0),
        Instr::I64Const(32),
        Instr::I64ShrU,
        Instr::I64Or,
        // a ^ secret[1]
        Instr::I64Const(-1800455987208640293i64),
        Instr::I64Xor,
        // b ^ seed = v ^ secret[0]
        Instr::LocalGet(0),
        Instr::I64Const(-6884282663029611473i64),
        Instr::I64Xor,
    ];
    body.extend(mix.iter().cloned());
    body.extend(mix);
    FuncDef {
        name: "hash_i64".into(),
        params: vec![ValType::I64],
        results: vec![ValType::I64],
        locals: vec![ValType::I64; 9],
        body,
    }
}

// ── hash_string(s: ref null $String) -> i64 ──────────────────────────────────
// wyhash v3 for a byte array. seed=0. Fully inlined.
fn hash_string_fn() -> FuncDef {
    // L0=s(param), L1=n(i32), L2=seed(i64), L3=a(i64), L4=b(i64),
    // L5=i(i32), L6=see1(i64), L7=see2(i64),
    // L8=off_scratch(i32), L9-L17=wymix scratch
    let mix = || wymix_instrs(9, 10, 11, 12, 13, 14, 15, 16, 17);

    // ── len == 0 ──
    let mut len0 = vec![
        Instr::I64Const(-1800455987208640293i64),
        Instr::I64Const(-1800455987208640293i64),
        Instr::LocalGet(2),
    ];
    len0.extend(mix());
    len0.extend(mix());
    len0.push(Instr::Return);

    // ── len 1-3 ──
    let mut len1_3 = vec![
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::I32Const(0),
        Instr::ArrayGetU(T_STRING.into()),
        Instr::I32Const(16),
        Instr::I32Shl,
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::LocalGet(1),
        Instr::I32Const(1),
        Instr::I32ShrU,
        Instr::ArrayGetU(T_STRING.into()),
        Instr::I32Const(8),
        Instr::I32Shl,
        Instr::I32Or,
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::LocalGet(1),
        Instr::I32Const(1),
        Instr::I32Sub,
        Instr::ArrayGetU(T_STRING.into()),
        Instr::I32Or,
        Instr::I64ExtendI32U,
        Instr::LocalSet(3),
        Instr::I64Const(0),
        Instr::LocalSet(4),
        Instr::I64Const(-1800455987208640293i64),
        Instr::LocalGet(1),
        Instr::I64ExtendI32U,
        Instr::I64Xor,
        Instr::LocalGet(3),
        Instr::I64Const(-1800455987208640293i64),
        Instr::I64Xor,
        Instr::LocalGet(4),
        Instr::LocalGet(2),
        Instr::I64Xor,
    ];
    len1_3.extend(mix());
    len1_3.extend(mix());
    len1_3.push(Instr::Return);

    // ── len 4-16 ──
    let mut len4_16 = vec![Instr::I32Const(0), Instr::LocalSet(8)];
    len4_16.extend(wyr4_instrs(0, 8));
    len4_16.extend_from_slice(&[
        Instr::I64Const(32),
        Instr::I64Shl,
        Instr::LocalGet(1),
        Instr::I32Const(3),
        Instr::I32ShrU,
        Instr::I32Const(2),
        Instr::I32Shl,
        Instr::LocalSet(8),
    ]);
    len4_16.extend(wyr4_instrs(0, 8));
    len4_16.extend_from_slice(&[Instr::I64Or, Instr::LocalSet(3)]);
    len4_16.extend_from_slice(&[
        Instr::LocalGet(1),
        Instr::I32Const(4),
        Instr::I32Sub,
        Instr::LocalSet(8),
    ]);
    len4_16.extend(wyr4_instrs(0, 8));
    len4_16.extend_from_slice(&[
        Instr::I64Const(32),
        Instr::I64Shl,
        Instr::LocalGet(1),
        Instr::I32Const(4),
        Instr::I32Sub,
        Instr::LocalGet(1),
        Instr::I32Const(3),
        Instr::I32ShrU,
        Instr::I32Const(2),
        Instr::I32Shl,
        Instr::I32Sub,
        Instr::LocalSet(8),
    ]);
    len4_16.extend(wyr4_instrs(0, 8));
    len4_16.extend_from_slice(&[Instr::I64Or, Instr::LocalSet(4)]);
    len4_16.extend_from_slice(&[
        Instr::I64Const(-1800455987208640293i64),
        Instr::LocalGet(1),
        Instr::I64ExtendI32U,
        Instr::I64Xor,
        Instr::LocalGet(3),
        Instr::I64Const(-1800455987208640293i64),
        Instr::I64Xor,
        Instr::LocalGet(4),
        Instr::LocalGet(2),
        Instr::I64Xor,
    ]);
    len4_16.extend(mix());
    len4_16.extend(mix());
    len4_16.push(Instr::Return);

    // ── long loop body (len > 48) ──
    let mut long_loop = vec![
        Instr::LocalGet(5),
        Instr::I32Const(48),
        Instr::I32Add,
        Instr::LocalGet(1),
        Instr::I32GtU,
        Instr::BrIf("long_exit".into()),
        Instr::LocalGet(5),
        Instr::LocalSet(8),
    ];
    long_loop.extend(wyr8_instrs(0, 8));
    long_loop.extend_from_slice(&[
        Instr::I64Const(-1800455987208640293i64),
        Instr::I64Xor,
        Instr::LocalGet(5),
        Instr::I32Const(8),
        Instr::I32Add,
        Instr::LocalSet(8),
    ]);
    long_loop.extend(wyr8_instrs(0, 8));
    long_loop.extend_from_slice(&[Instr::LocalGet(2), Instr::I64Xor]);
    long_loop.extend(mix());
    long_loop.extend_from_slice(&[
        Instr::LocalSet(2),
        Instr::LocalGet(5),
        Instr::I32Const(16),
        Instr::I32Add,
        Instr::LocalSet(8),
    ]);
    long_loop.extend(wyr8_instrs(0, 8));
    long_loop.extend_from_slice(&[
        Instr::I64Const(-8167223561372836125i64),
        Instr::I64Xor,
        Instr::LocalGet(5),
        Instr::I32Const(24),
        Instr::I32Add,
        Instr::LocalSet(8),
    ]);
    long_loop.extend(wyr8_instrs(0, 8));
    long_loop.extend_from_slice(&[Instr::LocalGet(6), Instr::I64Xor]);
    long_loop.extend(mix());
    long_loop.extend_from_slice(&[
        Instr::LocalSet(6),
        Instr::LocalGet(5),
        Instr::I32Const(32),
        Instr::I32Add,
        Instr::LocalSet(8),
    ]);
    long_loop.extend(wyr8_instrs(0, 8));
    long_loop.extend_from_slice(&[
        Instr::I64Const(6380440055042464963i64),
        Instr::I64Xor,
        Instr::LocalGet(5),
        Instr::I32Const(40),
        Instr::I32Add,
        Instr::LocalSet(8),
    ]);
    long_loop.extend(wyr8_instrs(0, 8));
    long_loop.extend_from_slice(&[Instr::LocalGet(7), Instr::I64Xor]);
    long_loop.extend(mix());
    long_loop.extend_from_slice(&[
        Instr::LocalSet(7),
        Instr::LocalGet(5),
        Instr::I32Const(48),
        Instr::I32Add,
        Instr::LocalSet(5),
        Instr::Br("long_loop".into()),
    ]);

    let long_then = vec![
        Instr::LocalGet(2),
        Instr::LocalSet(6),
        Instr::LocalGet(2),
        Instr::LocalSet(7),
        Instr::I32Const(0),
        Instr::LocalSet(5),
        Instr::Block {
            label: "long_exit".into(),
            result: None,
            body: vec![Instr::Loop {
                label: "long_loop".into(),
                result: None,
                body: long_loop,
            }],
        },
        Instr::LocalGet(2),
        Instr::LocalGet(6),
        Instr::I64Xor,
        Instr::LocalGet(7),
        Instr::I64Xor,
        Instr::LocalSet(2),
    ];

    // ── medium loop ──
    let mut med_loop = vec![
        Instr::LocalGet(1),
        Instr::LocalGet(5),
        Instr::I32Sub,
        Instr::I32Const(16),
        Instr::I32LeU,
        Instr::BrIf("med_exit".into()),
        Instr::LocalGet(5),
        Instr::LocalSet(8),
    ];
    med_loop.extend(wyr8_instrs(0, 8));
    med_loop.extend_from_slice(&[
        Instr::I64Const(-1800455987208640293i64),
        Instr::I64Xor,
        Instr::LocalGet(5),
        Instr::I32Const(8),
        Instr::I32Add,
        Instr::LocalSet(8),
    ]);
    med_loop.extend(wyr8_instrs(0, 8));
    med_loop.extend_from_slice(&[Instr::LocalGet(2), Instr::I64Xor]);
    med_loop.extend(mix());
    med_loop.extend_from_slice(&[
        Instr::LocalSet(2),
        Instr::LocalGet(5),
        Instr::I32Const(16),
        Instr::I32Add,
        Instr::LocalSet(5),
        Instr::Br("med_loop".into()),
    ]);

    // ── final: a = wyr8(p + n - 16), b = wyr8(p + n - 8) ──
    let mut final_part = vec![
        Instr::LocalGet(1),
        Instr::I32Const(16),
        Instr::I32Sub,
        Instr::LocalSet(8),
    ];
    final_part.extend(wyr8_instrs(0, 8));
    final_part.extend_from_slice(&[
        Instr::LocalSet(3),
        Instr::LocalGet(1),
        Instr::I32Const(8),
        Instr::I32Sub,
        Instr::LocalSet(8),
    ]);
    final_part.extend(wyr8_instrs(0, 8));
    final_part.extend_from_slice(&[
        Instr::LocalSet(4),
        Instr::I64Const(-1800455987208640293i64),
        Instr::LocalGet(1),
        Instr::I64ExtendI32U,
        Instr::I64Xor,
        Instr::LocalGet(3),
        Instr::I64Const(-1800455987208640293i64),
        Instr::I64Xor,
        Instr::LocalGet(4),
        Instr::LocalGet(2),
        Instr::I64Xor,
    ]);
    final_part.extend(mix());
    final_part.extend(mix());

    // ── Assemble body ──
    let mut body = vec![
        Instr::LocalGet(0),
        Instr::RefAsNonNull,
        Instr::ArrayLen,
        Instr::LocalSet(1),
        Instr::I64Const(-6884282663029611473i64),
        Instr::LocalSet(2),
        // len == 0
        Instr::LocalGet(1),
        Instr::I32Eqz,
        Instr::If {
            result: None,
            then_body: len0,
            else_body: vec![],
        },
        // len 1-3
        Instr::LocalGet(1),
        Instr::I32Const(4),
        Instr::I32LtU,
        Instr::If {
            result: None,
            then_body: len1_3,
            else_body: vec![],
        },
        // len 4-16
        Instr::LocalGet(1),
        Instr::I32Const(17),
        Instr::I32LtU,
        Instr::If {
            result: None,
            then_body: len4_16,
            else_body: vec![],
        },
        // len > 16
        Instr::LocalGet(1),
        Instr::I32Const(48),
        Instr::I32GtU,
        Instr::If {
            result: None,
            then_body: long_then,
            else_body: vec![Instr::I32Const(0), Instr::LocalSet(5)],
        },
        // medium loop
        Instr::Block {
            label: "med_exit".into(),
            result: None,
            body: vec![Instr::Loop {
                label: "med_loop".into(),
                result: None,
                body: med_loop,
            }],
        },
    ];
    body.extend(final_part);

    FuncDef {
        name: "hash_string".into(),
        params: vec![ref_string_local()],
        results: vec![ValType::I64],
        locals: vec![
            ValType::I32,
            ValType::I64,
            ValType::I64,
            ValType::I64,
            ValType::I32,
            ValType::I64,
            ValType::I64,
            ValType::I32,
            ValType::I64,
            ValType::I64,
            ValType::I64,
            ValType::I64,
            ValType::I64,
            ValType::I64,
            ValType::I64,
            ValType::I64,
            ValType::I64,
        ],
        body,
    }
}

// ── hash_key(key: anyref) -> i64 ─────────────────────────────────────────────
fn hash_key_fn() -> FuncDef {
    // p0=key
    FuncDef {
        name: "hash_key".into(),
        params: vec![ValType::Anyref],
        results: vec![ValType::I64],
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
    // p0=c, p1=hash(i64), p2=key, p3=val; L4=entries, L5=n, L6=i, L7=entry, L8=new_entries
    FuncDef {
        name: "collision_set".into(),
        params: vec![
            ref_hamt_collision_null(),
            ValType::I64,
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
            ValType::I64,
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
            // Stop before shift counts wrap past the 64-bit hash space.
            Instr::LocalGet(2),
            Instr::I32Const(13),
            Instr::I32GeU,
            Instr::If {
                result: None,
                then_body: vec![Instr::RefNull(HeapType::Any), Instr::Return],
                else_body: vec![],
            },
            // fragment = i32.wrap(hash >>u i64(depth*5)) & 31
            Instr::LocalGet(1), // hash (i64)
            Instr::LocalGet(2),
            Instr::I32Const(5),
            Instr::I32Mul, // shift (i32)
            Instr::I64ExtendI32U,
            Instr::I64ShrU,
            Instr::I32WrapI64,
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
                    Instr::I64Eq,
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
            ValType::I64,
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
                    // fragment = i32.wrap(hash >>u i64(depth*5)) & 31
                    Instr::LocalGet(1),
                    Instr::LocalGet(2),
                    Instr::I32Const(5),
                    Instr::I32Mul,
                    Instr::I64ExtendI32U,
                    Instr::I64ShrU,
                    Instr::I32WrapI64,
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
            // fragment = i32.wrap(hash >>u i64(shift)) & 31
            Instr::LocalGet(1),
            Instr::LocalGet(6),
            Instr::I64ExtendI32U,
            Instr::I64ShrU,
            Instr::I32WrapI64,
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
                    Instr::I64Eq,
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
                            // unless all 64 hash bits are already consumed.
                            Instr::LocalGet(2),
                            Instr::I32Const(12),
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
            ValType::I64,
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
            // fragment = i32.wrap(hash >>u i64(depth*5)) & 31
            Instr::LocalGet(1),
            Instr::LocalGet(2),
            Instr::I32Const(5),
            Instr::I32Mul,
            Instr::I64ExtendI32U,
            Instr::I64ShrU,
            Instr::I32WrapI64,
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
                    Instr::I64Eq,
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
    // p0=dict, p1=key, p2=val; L3=hash(i64), L4=old_root, L5=new_root, L6=was_present, L7=new_order
    FuncDef {
        name: "set".into(),
        params: vec![ref_pdict_null(), ValType::Anyref, ValType::Anyref],
        results: vec![ref_pdict()],
        locals: vec![
            ValType::I64,
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
    // p0=dict, p1=key; L2=hash(i64), L3=old_root, L4=new_root, L5=was_present, L6=new_order
    FuncDef {
        name: "remove".into(),
        params: vec![ref_pdict_null(), ValType::Anyref],
        results: vec![ref_pdict()],
        locals: vec![
            ValType::I64,
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

// ── set_in_place / remove_in_place: mutate PDict header in place ─────────────
fn set_in_place_fn() -> FuncDef {
    // p0=dict, p1=key, p2=val; L3=hash(i64), L4=old_root, L5=new_root, L6=was_present
    FuncDef {
        name: "set_in_place".into(),
        params: vec![ref_pdict_null(), ValType::Anyref, ValType::Anyref],
        results: vec![ref_pdict()],
        locals: vec![
            ValType::I64,
            ref_hamt_node_null(),
            ref_hamt_node_null(),
            ValType::I32,
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
            // Mutate root in place
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::LocalGet(5),
            Instr::StructSet(T_PDICT.into(), PD_ROOT),
            // If new key (not replace): push to order, bump size
            Instr::LocalGet(6),
            Instr::I32Eqz,
            Instr::If {
                result: None,
                then_body: vec![
                    // dict.order = arr_push(dict.order, key)
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PDICT.into(), PD_ORDER),
                    Instr::LocalGet(1),
                    Instr::Call("arr_push".into()),
                    Instr::StructSet(T_PDICT.into(), PD_ORDER),
                    // dict.size += 1
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PDICT.into(), PD_SIZE),
                    Instr::I32Const(1),
                    Instr::I32Add,
                    Instr::StructSet(T_PDICT.into(), PD_SIZE),
                ],
                else_body: vec![],
            },
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
        ],
    }
}

fn remove_in_place_fn() -> FuncDef {
    // p0=dict, p1=key; L2=hash(i64), L3=old_root, L4=new_root, L5=was_present
    FuncDef {
        name: "remove_in_place".into(),
        params: vec![ref_pdict_null(), ValType::Anyref],
        results: vec![ref_pdict()],
        locals: vec![
            ValType::I64,
            ref_hamt_node_null(),
            ref_hamt_node_null(),
            ValType::I32,
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
            // Always mutate root (no-op on miss: new_root == old_root)
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
            Instr::LocalGet(4),
            Instr::StructSet(T_PDICT.into(), PD_ROOT),
            Instr::LocalGet(5),
            Instr::If {
                result: None,
                then_body: vec![
                    // dict.order = order_remove_key(dict.order, key)
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PDICT.into(), PD_ORDER),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(1),
                    Instr::Call("order_remove_key".into()),
                    Instr::StructSet(T_PDICT.into(), PD_ORDER),
                    // dict.size -= 1
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::LocalGet(0),
                    Instr::RefAsNonNull,
                    Instr::StructGet(T_PDICT.into(), PD_SIZE),
                    Instr::I32Const(1),
                    Instr::I32Sub,
                    Instr::StructSet(T_PDICT.into(), PD_SIZE),
                ],
                else_body: vec![],
            },
            Instr::LocalGet(0),
            Instr::RefAsNonNull,
        ],
    }
}

#[cfg(test)]
mod tests {
    /// Reference wymix: 64x64→128 multiply, then XOR high and low halves.
    fn wymix_ref(a: u64, b: u64) -> u64 {
        let full = (a as u128) * (b as u128);
        let lo = full as u64;
        let hi = (full >> 64) as u64;
        lo ^ hi
    }

    /// Simulate the 4-partial-product decomposition used in the Wasm codegen,
    /// to verify it produces the same result as native 128-bit multiply.
    fn wymix_decomposed(a: u64, b: u64) -> u64 {
        let a_lo = a & 0xFFFFFFFF;
        let a_hi = a >> 32;
        let b_lo = b & 0xFFFFFFFF;
        let b_hi = b >> 32;

        let t1 = a_lo.wrapping_mul(b_hi);
        let t2 = a_hi.wrapping_mul(b_lo);
        let p0 = a_lo.wrapping_mul(b_lo);

        let cross = (p0 >> 32)
            .wrapping_add(t1 & 0xFFFFFFFF)
            .wrapping_add(t2 & 0xFFFFFFFF);

        let high = a_hi
            .wrapping_mul(b_hi)
            .wrapping_add(t1 >> 32)
            .wrapping_add(t2 >> 32)
            .wrapping_add(cross >> 32);

        let low = (p0 & 0xFFFFFFFF) | ((cross & 0xFFFFFFFF) << 32);
        low ^ high
    }

    #[test]
    fn test_wymix_decomposition_matches_native() {
        let cases: &[(u64, u64)] = &[
            (0, 0),
            (1, 1),
            (0xFFFFFFFF, 0xFFFFFFFF),
            (0x0123456789ABCDEF, 0xFEDCBA9876543210),
            (0xFFFFFFFFFFFFFFFF, 0xFFFFFFFFFFFFFFFF),
            // wyhash v3 secret constants
            (0xa0761d6478bd642f, 0xe7037ed1a0b428db),
            (0x8ebc6af09c88c6e3, 0x589965cc75374cc3),
        ];

        for &(a, b) in cases {
            let expected = wymix_ref(a, b);
            let actual = wymix_decomposed(a, b);
            assert_eq!(
                actual, expected,
                "wymix mismatch for (0x{a:016x}, 0x{b:016x}): \
                 decomposed=0x{actual:016x}, native=0x{expected:016x}"
            );
        }
    }

    #[test]
    fn test_wymix_known_vectors() {
        // Verify specific outputs so boot compiler tests can assert the same values.
        assert_eq!(wymix_ref(0, 0), 0);
        assert_eq!(wymix_ref(1, 1), 1);
        assert_eq!(wymix_ref(0xFFFFFFFF, 0xFFFFFFFF), 0xFFFFFFFE00000001);
        assert_eq!(
            wymix_ref(0x0123456789ABCDEF, 0xFEDCBA9876543210),
            0x2317228F48165BB2
        );
        assert_eq!(
            wymix_ref(0xFFFFFFFFFFFFFFFF, 0xFFFFFFFFFFFFFFFF),
            0xFFFFFFFFFFFFFFFF
        );
        assert_eq!(
            wymix_ref(0xa0761d6478bd642f, 0xe7037ed1a0b428db),
            0x1ff5c2923a788d2c
        );
    }

    // --- wyhash v3 reference implementation for test vectors ---
    //
    // Authoritative source: wyhash by Wang Yi
    //   https://github.com/wangyi-fudan/wyhash
    //   Pinned to wyhash v3 final (commit 991aa3d, 2023-08-20, wyhash.h)
    //
    // Constants (_wyp): the four secret primes used by wyhash v3.
    // Seed: 0 (deterministic, no per-process randomization).
    // Mix: wymix(a,b) = lo ^ hi of 128-bit multiply a*b.
    //
    // The same algorithm and constants are mirrored in:
    //   - src/runtime/dict.rs  (Wasm IR codegen: wymix_instrs, hash_i64_fn, hash_string_fn)
    //   - boot/compiler/codegen/runtime/dict.tw  (boot compiler mirror)
    //   - src/query/keys.rs  (native Rust, compile-time hashing)
    //   - boot/lib/query/keys.tw  (native Twinkle, compile-time hashing)

    const SECRET: [u64; 4] = [
        0xa0761d6478bd642f,
        0xe7037ed1a0b428db,
        0x8ebc6af09c88c6e3,
        0x589965cc75374cc3,
    ];

    fn wyr3(p: &[u8], len: usize) -> u64 {
        ((p[0] as u64) << 16) | ((p[len >> 1] as u64) << 8) | (p[len - 1] as u64)
    }

    fn wyr4(p: &[u8]) -> u64 {
        u32::from_le_bytes([p[0], p[1], p[2], p[3]]) as u64
    }

    fn wyr8(p: &[u8]) -> u64 {
        u64::from_le_bytes([p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7]])
    }

    fn wyhash_ref(key: &[u8], seed: u64) -> u64 {
        let len = key.len();
        let mut seed = seed ^ SECRET[0];
        let (a, b);

        if len <= 16 {
            if len >= 4 {
                let mid = (len >> 3) << 2;
                a = (wyr4(key) << 32) | wyr4(&key[mid..]);
                b = (wyr4(&key[len - 4..]) << 32) | wyr4(&key[len - 4 - mid..]);
            } else if len > 0 {
                a = wyr3(key, len);
                b = 0;
            } else {
                a = 0;
                b = 0;
            }
        } else {
            let mut p = 0;
            let mut i = len;
            if i > 48 {
                let mut see1 = seed;
                let mut see2 = seed;
                loop {
                    seed = wymix_ref(wyr8(&key[p..]) ^ SECRET[1], wyr8(&key[p + 8..]) ^ seed);
                    see1 = wymix_ref(
                        wyr8(&key[p + 16..]) ^ SECRET[2],
                        wyr8(&key[p + 24..]) ^ see1,
                    );
                    see2 = wymix_ref(
                        wyr8(&key[p + 32..]) ^ SECRET[3],
                        wyr8(&key[p + 40..]) ^ see2,
                    );
                    p += 48;
                    i -= 48;
                    if i <= 48 {
                        break;
                    }
                }
                seed ^= see1 ^ see2;
            }
            while i > 16 {
                seed = wymix_ref(wyr8(&key[p..]) ^ SECRET[1], wyr8(&key[p + 8..]) ^ seed);
                p += 16;
                i -= 16;
            }
            a = wyr8(&key[len - 16..]);
            b = wyr8(&key[len - 8..]);
        }

        wymix_ref(SECRET[1] ^ (len as u64), wymix_ref(a ^ SECRET[1], b ^ seed))
    }

    /// Reference hash_i64: treat i64 as 8 LE bytes, run wyhash v3 with seed=0.
    fn hash_i64_ref(v: i64) -> u64 {
        wyhash_ref(&v.to_le_bytes(), 0)
    }

    /// Reference hash_string: run wyhash v3 on raw bytes with seed=0.
    fn hash_string_ref(s: &[u8]) -> u64 {
        wyhash_ref(s, 0)
    }

    #[test]
    fn test_wyhash_i64_vectors() {
        // Pinned vectors: if any value changes, the Wasm codegen in hash_i64_fn
        // and the boot compiler's runtime/dict.tw must be updated to match.
        assert_eq!(hash_i64_ref(0), 0x426aa7db91aa5b32);
        assert_eq!(hash_i64_ref(1), 0x609b1ba462acd964);
        assert_eq!(hash_i64_ref(-1), 0x2dd1be9335d66126);
        assert_eq!(hash_i64_ref(42), 0xc73c7e9ff277cd8f);
        assert_eq!(hash_i64_ref(i64::MAX), 0xcea58c444358fd3b);
        assert_eq!(hash_i64_ref(i64::MIN), 0x00a0852a528a546b);
    }

    #[test]
    fn test_wyhash_string_vectors() {
        // Pinned vectors: must match the Wasm codegen in hash_string_fn and the
        // boot compiler's runtime/dict.tw. Also mirrored in query/keys.rs for
        // the strings shared with the query-key hash path.
        assert_eq!(hash_string_ref(b""), 0x42bc986dc5eec4d3);
        assert_eq!(hash_string_ref(b"a"), 0x6cf84e5a2465e867);
        assert_eq!(hash_string_ref(b"ab"), 0x172ba773b8ebb6d8);
        assert_eq!(hash_string_ref(b"abc"), 0xb4808df22d44ffcf);
        assert_eq!(hash_string_ref(b"abcd"), 0xe73573b4c2ddfea0);
        assert_eq!(hash_string_ref(b"hello"), 0xfaacec54df7a6205);
        assert_eq!(hash_string_ref(b"hello world"), 0x19f24a02fe04c3ca);
        assert_eq!(
            hash_string_ref(b"user__$f2396_mark_published_version"),
            0x892f9a20308b0d45
        );
        assert_eq!(hash_string_ref(b"user__$str_333_get"), 0x23152e5b139e8c4b);

        // The original FNV collision pair must not collide under wyhash v3
        assert_ne!(
            hash_string_ref(b"user__$f2396_mark_published_version"),
            hash_string_ref(b"user__$str_333_get"),
        );
    }
}
